//! `gen-python` — write a Python client package for each IDL file.
//!
//! ```text
//! gen-python --out <dir> [-I <dir>]... <file.idl>...
//! ```
//!
//! One package per file, named after the file's stem, plus the hand-written
//! `_rt.py` inside each. The output directory is what a consumer puts on
//! `sys.path`; nothing else is needed, because a generated package imports
//! only its own runtime and CPython's standard library.
//!
//! Each file is read as a **translation unit**: what it includes is part of
//! what it says, and `-I` adds a directory to resolve `#include` against as
//! `sidl-validate` does. The reason is the same one the Rust target has, and
//! the second emitter is where it stops being one emitter's opinion: a class
//! generated from a file read as a string has no method for an operation an
//! included header declares, so the Python client cannot make a call the peer
//! would have answered — and the id it would have sent is missing the prefix
//! that header set. *포함은 계약의 일부다.*
//!
//! Skips are printed with their reasons and are **not** failures: a deferred
//! wire type (§4.4) and a type with no AnyJSON form are decisions, and the
//! exit code is reserved for a file that would not generate at all.

use std::path::Path;

use orbweaver_gen::python::emit_python;
use orbweaver_registry::{Contract, Registry, Strictness, take_include_dirs};

fn main() -> std::process::ExitCode {
    let mut out_dir: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    let search = match take_include_dirs(&mut argv) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    };
    let mut args = argv.into_iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out_dir = args.next(),
            other => files.push(other.to_owned()),
        }
    }
    let (Some(out_dir), false) = (out_dir, files.is_empty()) else {
        eprintln!("usage: gen-python --out <dir> [-I <dir>]... <file.idl>...");
        return std::process::ExitCode::from(2);
    };

    let mut failed = 0usize;
    let mut emitted = 0usize;
    for path in &files {
        let stem = Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().replace(['-', '.'], "_"))
            .unwrap_or_default();
        // A Python package name may not start with a digit, and the corpus is
        // numbered.
        let package =
            if stem.starts_with(|c: char| c.is_ascii_digit()) { format!("g{stem}") } else { stem };

        // Exit 2 stays "could not run": a path this process cannot open is a
        // mistyped argument, not a defective contract, and `Contract::load`
        // folds the two into one error.
        if let Err(e) = std::fs::File::open(path) {
            eprintln!("{path}: {e}");
            return std::process::ExitCode::from(2);
        }
        // The front end gates generation over the whole unit, exactly as it
        // does for Rust: stubs describing calls nobody can make are worse than
        // no stubs, and so are stubs missing the calls a header declares.
        let contract = match Contract::load(Path::new(path), &search, Strictness::Checked) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{path}: rejected by the front end:");
                for line in e.message.lines().take(3) {
                    eprintln!("  {line}");
                }
                failed += 1;
                continue;
            }
        };
        let mut registry = Registry::new();
        if let Err(e) = registry.load(&contract.spec) {
            eprintln!("{path}: {e}");
            failed += 1;
            continue;
        }
        // Not gated on `Registry::unresolved()`, for the reason `gen-corpus`
        // states at the same point: that list also holds names whose only
        // problem is that the registry's resolver does not search an inherited
        // interface's scope, and refusing on it would refuse legal IDL both
        // oracles accept. The include class is covered by `Contract::load`.
        let package_dir = Path::new(&out_dir).join(&package);
        let generated = emit_python(&registry, &package);
        emitted += generated.emitted;
        for (name, source) in &generated.files {
            let target = package_dir.join(name);
            if let Some(parent) = target.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                eprintln!("{}: {e}", parent.display());
                return std::process::ExitCode::from(2);
            }
            if let Err(e) = std::fs::write(&target, source) {
                eprintln!("{}: {e}", target.display());
                return std::process::ExitCode::from(2);
            }
        }
        println!("{package}: {} item(s), {} file(s)", generated.emitted, generated.files.len());
        for (id, why) in &generated.skipped {
            println!("  skipped {id}: {why}");
        }
    }

    println!("generated {emitted} item(s) from {} file(s) into {out_dir}", files.len());
    if failed > 0 {
        println!("{failed} file(s) failed");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
