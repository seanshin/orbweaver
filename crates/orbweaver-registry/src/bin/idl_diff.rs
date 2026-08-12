//! `idl-diff` — the release gate for §5.3.
//!
//! Compares a released contract against a proposed one and refuses the
//! proposal if it would break deployed peers. The refusal is the product; a
//! report nobody is obliged to read would not have caught the swapped struct
//! members that `spike-evolution` shows omniORB accepting in silence.
//!
//! ```text
//! idl-diff <released.idl> <proposed.idl> [--approve <reason>] [--quiet]
//! ```
//!
//! Exit status: 0 accepted, 1 refused, 2 could not run.
//!
//! `--approve` does not make a change safe. It records that somebody took
//! responsibility for it, and the reason is printed alongside the findings so
//! the decision travels with the diff instead of living in a chat log.

use orbweaver_registry::Registry;
use orbweaver_registry::diff::diff;

fn main() -> std::process::ExitCode {
    let mut released = None;
    let mut proposed = None;
    let mut approval: Option<String> = None;
    let mut quiet = false;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--approve" => match args.next() {
                // An empty reason is the same as no reason, and an approval
                // with no reason is what the flag exists to prevent.
                Some(r) if !r.trim().is_empty() => approval = Some(r),
                _ => {
                    eprintln!("--approve needs a reason");
                    return std::process::ExitCode::from(2);
                }
            },
            "--quiet" => quiet = true,
            "-h" | "--help" => {
                println!("usage: idl-diff <released.idl> <proposed.idl> [--approve <reason>]");
                return std::process::ExitCode::SUCCESS;
            }
            _ if released.is_none() => released = Some(a),
            _ if proposed.is_none() => proposed = Some(a),
            other => {
                eprintln!("unexpected argument: {other}");
                return std::process::ExitCode::from(2);
            }
        }
    }
    let (Some(released), Some(proposed)) = (released, proposed) else {
        eprintln!("usage: idl-diff <released.idl> <proposed.idl> [--approve <reason>]");
        return std::process::ExitCode::from(2);
    };

    let (a, b) = match (load(&released), load(&proposed)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    };

    let changes = diff(&a, &b);
    let blocking = changes.iter().filter(|c| c.verdict.blocks_release()).count();

    if !quiet {
        if changes.is_empty() {
            println!("no change between {released} and {proposed}");
        }
        for c in &changes {
            println!("{c}");
        }
    }

    match (blocking, approval) {
        (0, _) => {
            if !quiet {
                println!("\naccepted: nothing here breaks a deployed peer");
            }
            std::process::ExitCode::SUCCESS
        }
        (n, Some(reason)) => {
            println!("\naccepted under approval: {reason}");
            println!("{n} change(s) will break deployed peers; that is now a decision on record");
            std::process::ExitCode::SUCCESS
        }
        (n, None) => {
            println!("\nrefused: {n} change(s) break deployed peers");
            println!(
                "a released type is not editable in place — publish a new version of the \
                 interface, or re-run with --approve <reason> (docs/PLAN.md §5.3)"
            );
            std::process::ExitCode::FAILURE
        }
    }
}

fn load(path: &str) -> Result<Registry, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let spec = orbweaver_idl::parse(&src).map_err(|e| format!("{path}: {e}"))?;
    let mut r = Registry::new();
    r.load(&spec).map_err(|e| format!("{path}: {e}"))?;
    Ok(r)
}
