//! Prints the repository id of every definition in an IDL file.
//!
//! Usage: `repository-ids <file.idl>...`, one `<file>\t<qualified>\t<id>` row
//! per definition, sorted — the shape `corpus/pragma/expected.tsv` records, so
//! a differential run against `omniidl` is a `diff` rather than an eyeball.
//!
//! It exists because `#pragma prefix` made ids stop being derivable from the
//! IDL by inspection. Before the pragma batch a reader could work an id out
//! from the module path; now the answer depends on where a pragma was written,
//! and a question that can only be answered by running the compiler needs a
//! way to run the compiler.

use orbweaver_registry::Registry;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: repository-ids <file.idl>...");
        return std::process::ExitCode::from(2);
    }
    let mut failed = false;
    for path in &args {
        if let Err(e) = dump(path) {
            eprintln!("{path}: {e}");
            failed = true;
        }
    }
    if failed { std::process::ExitCode::FAILURE } else { std::process::ExitCode::SUCCESS }
}

fn dump(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let src = std::fs::read_to_string(path)?;
    let spec = orbweaver_idl::parse(&src).map_err(|e| e.to_string())?;
    let mut reg = Registry::new();
    reg.load(&spec)?;

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
