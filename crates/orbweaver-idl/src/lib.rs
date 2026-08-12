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

#![deny(missing_docs)]

pub mod ast;
pub mod lex;
pub mod parse;
pub mod sema;

pub use parse::{ParseError, parse};
pub use sema::{Analysis, Diagnostic, analyse};

/// Parses and analyses `src`, returning either the checked spec or everything
/// wrong with it.
///
/// Diagnostics come back as a list rather than one error because the
/// self-repair loop fixes a batch per round, and reporting only the first
/// problem would make each round correct exactly one thing.
pub fn check(src: &str) -> std::result::Result<ast::Spec, Vec<Diagnostic>> {
    let spec = parse(src)
        .map_err(|e| vec![Diagnostic { message: e.message, span: e.span, rule: e.rule }])?;
    let analysis = analyse(&spec);
    if analysis.is_ok() { Ok(spec) } else { Err(analysis.diagnostics) }
}
