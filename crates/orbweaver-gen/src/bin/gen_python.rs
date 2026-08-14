//! `gen-python` — write a Python client package for each IDL file.
//!
//! ```text
//! gen-python --out <dir> <file.idl>...
//! ```
//!
//! One package per file, named after the file's stem, plus the hand-written
//! `_rt.py` inside each. The output directory is what a consumer puts on
//! `sys.path`; nothing else is needed, because a generated package imports
//! only its own runtime and CPython's standard library.
//!
//! Skips are printed with their reasons and are **not** failures: a deferred
//! wire type (§4.4) and a type with no AnyJSON form are decisions, and the
//! exit code is reserved for a file that would not generate at all.

use std::path::Path;

use orbweaver_gen::python::emit_python;
use orbweaver_registry::Registry;

fn main() -> std::process::ExitCode {
    let mut out_dir: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out_dir = args.next(),
            other => files.push(other.to_owned()),
        }
    }
    let (Some(out_dir), false) = (out_dir, files.is_empty()) else {
        eprintln!("usage: gen-python --out <dir> <file.idl>...");
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

        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{path}: {e}");
                return std::process::ExitCode::from(2);
            }
        };
        // The front end gates generation, exactly as it does for Rust: stubs
        // describing calls nobody can make are worse than no stubs.
        let spec = match orbweaver_idl::check(&src) {
            Ok(s) => s,
            Err(diags) => {
                eprintln!("{path}: rejected by the front end:");
                for d in diags.iter().take(3) {
                    eprintln!("  {d}");
                }
                failed += 1;
                continue;
            }
        };
        let mut registry = Registry::new();
        if let Err(e) = registry.load(&spec) {
            eprintln!("{path}: {e}");
            failed += 1;
            continue;
        }
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
