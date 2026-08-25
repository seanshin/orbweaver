//! The generated file names nothing the contract can rename.
//!
//! Two halves of one rule, and they are fixed by opposite means:
//!
//! * **What the contract names** is spelled by one function — `ident` for
//!   Rust, [`orbweaver_gen::python::python_name`] for Python — at every site
//!   that writes a name *and* at every site that looks one up. Where Rust will
//!   not let the contract's name stand at all (`Self`, and the prelude
//!   constructors a pattern cannot bind), that function moves it.
//! * **What the generator names** is reached through a path the contract
//!   cannot bind: `__rt`/`__Cdr`, whose leading underscores no IDL identifier
//!   can spell, or an absolute `::std::…`.
//!
//! The second half is the one nothing can make impossible: an emitter that
//! writes a bare `Result<` tomorrow compiles fine here and stops compiling in
//! a consumer's crate whose contract happens to declare `struct Result`. So it
//! is scanned, over every contract in the corpus, in this file.
//!
//! Measured 2026-08-25, before the fix: 2793 probes (147 identifiers × 19
//! positions) through both emitters, compiling the Rust and importing the
//! Python — 92 Rust failures and 39 Python ones, in six root causes. After:
//! 0 and 0.

use orbweaver_registry::Registry;

/// Names the emitted Rust must never spell bare, with where each came from.
///
/// Every one of these was measured breaking a real contract, not supposed:
/// `rt`, `Cdr` and `orbweaver_gen` as `E0255`/`E0659` (the import redefined or
/// made ambiguous by a module of that name), `Result`/`Option`/`Vec`/`String`
/// as `E0107` and `E0391` (the contract's type used where the prelude's was
/// meant, one of them a type-alias cycle), `std` as `E0405`.
const MUST_NOT_APPEAR_BARE: &[&str] = &[
    "rt::",
    "Cdr",
    "std::",
    "Result<",
    "Option<",
    "Vec<",
    "String",
    "Into<",
    "Iterator<",
    "PartialEq for",
    "Default for",
];

/// Whether the character before an occurrence already qualifies it.
///
/// `__rt::` and `__Cdr` are the runtime under names IDL cannot spell;
/// `::std::`, `crate::…::Result` and `GiopError::Cdr` are paths. What is left
/// — an occurrence at a word boundary with no `_` or `:` in front — is a bare
/// name, and a bare name is one a contract can shadow.
fn already_qualified(before: Option<char>) -> bool {
    matches!(before, Some(c) if c == '_' || c == ':' || c.is_alphanumeric())
}

fn bare_references(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (n, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        // Documentation is prose about the runtime, and says the path a
        // consumer would write (`orbweaver_gen::rt::…`) rather than the alias.
        if trimmed.starts_with("//") {
            continue;
        }
        // The import is where the two names are *bound*; every other line has
        // to reach them through what it binds.
        if trimmed.starts_with("use ::orbweaver_gen::rt::") {
            continue;
        }
        for needle in MUST_NOT_APPEAR_BARE {
            let mut from = 0;
            while let Some(at) = line[from..].find(needle) {
                let at = from + at;
                let end = at + needle.len();
                let before = line[..at].chars().next_back();
                // `Strings` is not `String`, and `Cdrs` would not be `Cdr`.
                let joined_after = needle.ends_with(char::is_alphanumeric)
                    && line[end..].starts_with(|c: char| c.is_alphanumeric() || c == '_');
                if !already_qualified(before) && !joined_after {
                    found.push(format!("line {}: bare `{needle}` in: {}", n + 1, line.trim()));
                }
                from = end;
            }
        }
    }
    found
}

fn emit(idl: &str) -> String {
    let spec = orbweaver_idl::parse(idl).expect("parses");
    let mut registry = Registry::new();
    registry.load(&spec).expect("loads");
    orbweaver_gen::emit(&registry, "g").source
}

/// Over every contract in the corpus, not over one probe: the property is about
/// the *templates*, so the widest input available is the right one.
#[test]
fn the_emitted_rust_reaches_the_runtime_and_the_prelude_only_through_paths_idl_cannot_bind() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let mut seen = 0usize;
    let mut findings = Vec::new();
    for dir in ["golden", "services"] {
        for entry in std::fs::read_dir(root.join(dir)).expect("the corpus directory") {
            let path = entry.expect("entry").path();
            if path.extension().is_none_or(|x| x != "idl") {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("read");
            let Ok(spec) = orbweaver_idl::parse(&src) else { continue };
            let mut registry = Registry::new();
            if registry.load(&spec).is_err() {
                continue;
            }
            seen += 1;
            let generated = orbweaver_gen::emit(&registry, "g");
            for finding in bare_references(&generated.source) {
                findings.push(format!("{}: {finding}", path.display()));
            }
        }
    }
    assert!(seen > 20, "the corpus sweep measured only {seen} file(s)");
    assert!(
        findings.is_empty(),
        "the emitted code names {} thing(s) a contract could rename:\n{}",
        findings.len(),
        findings.join("\n")
    );
}

/// The names Rust refuses outright, in the positions that refuse them.
///
/// `Self` is the keyword a raw identifier cannot spell — fifteen of nineteen
/// positions emitted it verbatim — and `Ok`/`Err`/`Some`/`None` are the
/// prelude constructors a pattern matches instead of binding. Both are the one
/// class qualification cannot reach, because the offending name is the
/// contract's own, so the emitted name moves.
#[test]
fn a_name_rust_will_not_bind_is_moved_rather_than_escaped() {
    let source = emit(
        "module m {\n\
           struct _Self { long a; };\n\
           const long Ok = 1;\n\
           interface I { long op(in long None, in long Err, in long Some); };\n\
           typedef sequence<long> i32;\n\
         };",
    );
    for wanted in [
        // `r#Self` is not a raw identifier, it is an error.
        "pub struct Self_ {",
        "pub const Ok_: i32 = 1;",
        // A parameter is a pattern; `None` there matched `Option::None`.
        "None_: i32",
        "Err_: i32",
        "Some_: i32",
        // A contract type named for a primitive shadowed it in its own module.
        "pub type i32_ =",
    ] {
        assert!(source.contains(wanted), "missing {wanted}\n{source}");
    }
    // And the unescaped spellings are gone, not merely joined.
    for forbidden in ["pub struct Self ", "pub const Ok:", "pub type i32 ="] {
        assert!(!source.contains(forbidden), "still emitted {forbidden}\n{source}");
    }
}

/// A declaration outside any `module` lands at file scope, and file scope had
/// no import: `impl __Cdr for TopS` with nothing in scope named `__Cdr`.
///
/// Every corpus file opens a module, which is the whole reason nothing was
/// red. The generated crate did not compile and no test said so.
#[test]
fn a_declaration_outside_any_module_gets_the_runtime_too() {
    let source = emit("struct TopS { long a; };");
    assert!(source.contains("pub struct TopS {"), "{source}");
    assert!(
        source.contains("use ::orbweaver_gen::rt::{self as __rt, Cdr as __Cdr};"),
        "a file-scope declaration has no runtime in scope:\n{source}"
    );
}

/// A name the *generator* mints lands in the same namespace the contract's
/// names land in, and the two collided: `System`, the fault variant for the
/// vocabulary IDL cannot declare, against `exception System`; and `Unlisted_`,
/// a union's escape arm, against a branch of that name. Both declared the
/// variant twice and the crate did not compile (`E0428`).
///
/// Which one moves is the same question `raises_of` already answered for two
/// exceptions sharing a last segment: the *derived* name pays. So the fault's
/// `System` — which every servant in the workspace matches on, and which is
/// documented as always present — stays, and the exception's derived variant
/// becomes `System_`; the union's arm is minted rather than derived and the
/// branch name is the one a caller writes, so there the mint pays.
#[test]
fn a_minted_variant_and_a_contract_name_cannot_be_the_same_variant() {
    let source = emit(
        "module m {\n\
           exception System { long a; };\n\
           union U switch (long) { case 1: long Unlisted_; case 2: long b; };\n\
           interface I { void op() raises (System); };\n\
         };",
    );
    assert!(source.contains("Self::System(__ex)"), "the fault lost its own variant:\n{source}");
    assert!(source.contains("System_("), "the derived variant did not move:\n{source}");
    assert!(source.contains("Unlisted__("), "the minted arm did not move:\n{source}");
    assert!(source.contains("pub Unlisted_:") || source.contains("Unlisted_("), "{source}");
}
