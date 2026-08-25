//! OMG IDL 4.2 front end.
//!
//! `omniidl` and `tao_idl` remain the conformance authority; this exists to be
//! *ours* — MIT, and able to carry the SIDL semantics deployed compilers reject
//! (`docs/PHASE0.md`, assumption C). Owning the parser is what made the
//! structured-comment fallback possible at all.
//!
//! # What correctness means here
//!
//! Not taste, and not a reading of the grammar: **agreement with the oracle**.
//! The parser must accept every file in `corpus/golden/` and reject every file
//! in `corpus/negative/`, because that is what `omniidl` does with them. The
//! corpus is the specification of this crate's behaviour.

//!
//! # A file is not a translation unit
//!
//! The corpus is made entirely of self-contained files, and for a long time so
//! was this crate's model of the world: `#include` was skipped along with every
//! other `#` directive. The estate in `spikes/estate/` measured what that
//! costs — 11 of 13 files rejected here and accepted by `omniidl`, because a
//! type declared in `01-common.idl` is not declared as far as we were
//! concerned. [`include`] resolves them; [`check_file`] is the entry point that
//! uses it, and [`check`] is the string entry point that cannot, and says so
//! instead of pretending the include was not there.

#![deny(missing_docs)]

pub mod ast;
pub mod include;
pub mod lex;
pub mod parse;
pub mod rules;
pub mod sema;

pub use include::{SearchPath, Unit, preprocess, preprocess_file};
pub use parse::{ParseError, parse};
pub use sema::{
    Analysis, DEFERRED_WIRE_RULE, DeferredWireUse, Diagnostic, analyse, deferred_wire_types,
};

/// Parses and analyses `src`, returning either the checked spec or everything
/// wrong with it.
///
/// Diagnostics come back as a list rather than one error because the
/// self-repair loop fixes a batch per round, and reporting only the first
/// problem would make each round correct exactly one thing.
///
/// A string has no directory, so a `#include` in `src` can only resolve if it
/// is absolute. One that does not is reported here rather than skipped: the
/// skip was the defect, and it turned one missing file into a diagnostic per
/// name the file declared. Use [`check_file`] when there is a path.
pub fn check(src: &str) -> std::result::Result<ast::Spec, Vec<Diagnostic>> {
    check_unit(&preprocess(src, None, &SearchPath::new()))
}

/// Resolves `path`'s includes, then parses and analyses the whole unit.
///
/// The [`Unit`] comes back either way, because it is what maps a diagnostic's
/// position in the spliced text back to the file and line somebody wrote —
/// see [`Unit::render`].
pub fn check_file(
    path: &std::path::Path,
    search: &SearchPath,
) -> std::io::Result<(Unit, std::result::Result<ast::Spec, Vec<Diagnostic>>)> {
    let unit = preprocess_file(path, search)?;
    let result = check_unit(&unit);
    Ok((unit, result))
}

/// Parses and analyses an already-resolved unit.
///
/// A unit with unresolved includes is **not** analysed. Analysing a
/// translation unit with a piece missing produces a diagnostic for every name
/// the missing piece declared — 90 of them across the thirteen-file estate,
/// all of them consequences of one cause. Reporting the cause and stopping is
/// both shorter and true.
pub fn check_unit(unit: &Unit) -> std::result::Result<ast::Spec, Vec<Diagnostic>> {
    if !unit.errors.is_empty() {
        return Err(unit.errors.clone());
    }
    parse(&unit.text)
        .map_err(|e| vec![Diagnostic { message: e.message, span: e.span, rule: e.rule }])
        .and_then(|spec| {
            let analysis = analyse(&spec);
            if analysis.is_ok() { Ok(spec) } else { Err(analysis.diagnostics) }
        })
}
