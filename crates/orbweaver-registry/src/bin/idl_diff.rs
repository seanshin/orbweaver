//! `idl-diff` — the release gate for §5.3.
//!
//! Compares a released contract against a proposed one and refuses the
//! proposal if it would break deployed peers. The refusal is the product; a
//! report nobody is obliged to read would not have caught the swapped struct
//! members that `spike-evolution` shows omniORB accepting in silence.
//!
//! ```text
//! idl-diff [-I <dir>]... <released.idl> <proposed.idl> [--approve <reason>] [--quiet]
//! ```
//!
//! Exit status: 0 accepted, 1 refused, 2 could not run.
//!
//! `--approve` does not make a change safe. It records that somebody took
//! responsibility for it, and the reason is printed alongside the findings so
//! the decision travels with the diff instead of living in a chat log.
//!
//! # `#include` is resolved, and that is the whole gate
//!
//! This read its two files with the *string* entry point, which by its own
//! documentation cannot resolve a relative `#include`. Two revisions differing
//! only inside a shared header therefore compared as two identical translation
//! units, and the gate that exists to catch a breaking change printed "no
//! change" and exited 0. `corpus/evolution/` is that case: `ledger.idl` is
//! byte-identical between v1 and v2, and both changes — a struct member's type
//! and an inherited operation's existence — are in `common.idl`.
//!
//! For the same reason this refuses to issue a verdict at all when the registry
//! reports [`Registry::unresolved`]: a diff of two partial graphs says nothing
//! about the contracts, and saying nothing loudly is the only safe answer a
//! gate has.
//!
//! *포함된 헤더에만 있는 파괴적 변경을 게이트가 통과시켰다. 이제 `#include`를
//! 해석하며, 해석하지 못한 참조가 있으면 판정 자체를 거부한다.*

use orbweaver_registry::diff::diff;
use orbweaver_registry::{Registry, Strictness, take_include_dirs};

fn main() -> std::process::ExitCode {
    let mut released = None;
    let mut proposed = None;
    let mut approval: Option<String> = None;
    let mut quiet = false;

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
                println!(
                    "usage: idl-diff [-I <dir>]... <released.idl> <proposed.idl> \
                     [--approve <reason>]"
                );
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

    let (a, b) = match (load(&released, &search), load(&proposed, &search)) {
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

/// Builds a registry from one contract file, `#include`s resolved.
///
/// An unresolved reference is fatal *here* rather than a note in the report.
/// The report is advisory by nature — a reader decides what to do with it —
/// but the exit code is not, and an exit code computed from a graph with a base
/// interface missing is a claim this tool has no evidence for. Exit 2 is "could
/// not run", which is exactly what happened.
fn load(path: &str, search: &orbweaver_idl::SearchPath) -> Result<Registry, String> {
    let contract =
        orbweaver_registry::Contract::load(std::path::Path::new(path), search, Strictness::Grammar)
            .map_err(|e| e.message)?;
    let mut r = Registry::new();
    r.load(&contract.spec).map_err(|e| format!("{path}: {e}"))?;
    if !r.unresolved().is_empty() {
        let mut msg = format!("{path}: cannot diff a contract with unresolved references:");
        for u in r.unresolved() {
            msg.push_str(&format!("\n  {u}"));
        }
        msg.push_str("\n  a missing `#include`, or a `-I <dir>` this run was not given");
        return Err(msg);
    }
    Ok(r)
}
