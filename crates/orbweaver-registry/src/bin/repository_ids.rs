//! Prints the repository id of every definition in an IDL file.
//!
//! Usage: `repository-ids [-I <dir>]... <file.idl>...`, one
//! `<file>\t<qualified>\t<id>` row
//! per definition, sorted — the shape `corpus/pragma/expected.tsv` records, so
//! a differential run against `omniidl` is a `diff` rather than an eyeball.
//!
//! It exists because `#pragma prefix` made ids stop being derivable from the
//! IDL by inspection. Before the pragma batch a reader could work an id out
//! from the module path; now the answer depends on where a pragma was written,
//! and a question that can only be answered by running the compiler needs a
//! way to run the compiler.
//!
//! `#include` is resolved first, for the same reason: a `#pragma prefix` in an
//! included header is part of the id of everything after it, so a run that
//! skipped the include would print ids the wire never carries. Every file in
//! `corpus/pragma/` is self-contained, so `expected.tsv` is unaffected — which
//! is exactly why the gap could sit there unmeasured.

use orbweaver_idl::SearchPath;
use orbweaver_registry::{Registry, Strictness, take_include_dirs};

fn main() -> std::process::ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let search = match take_include_dirs(&mut args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    };
    if args.is_empty() {
        eprintln!("usage: repository-ids [-I <dir>]... <file.idl>...");
        return std::process::ExitCode::from(2);
    }
    let mut failed = false;
    for path in &args {
        if let Err(e) = dump(path, &search) {
            eprintln!("{path}: {e}");
            failed = true;
        }
    }
    if failed { std::process::ExitCode::FAILURE } else { std::process::ExitCode::SUCCESS }
}

fn dump(path: &str, search: &SearchPath) -> Result<(), Box<dyn std::error::Error>> {
    let reg: Registry =
        orbweaver_registry::registry_from_files(&[path], search, Strictness::Grammar)?;
    for u in reg.unresolved() {
        eprintln!("{path}: warning: {u}");
    }

    let file = std::path::Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned());

    // Keyed by id rather than by name: the id is what goes on the wire, and
    // sorting by it makes two ORBs' output comparable line by line.
    let mut rows: Vec<(String, String)> = reg
        .ids()
        .map(|id| (id.clone(), reg.qualified_name(id).unwrap_or_default().to_owned()))
        .collect();
    rows.sort();
    for (id, qualified) in rows {
        println!("{file}\t{qualified}\t{id}");
    }
    Ok(())
}
