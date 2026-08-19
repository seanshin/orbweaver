//! `idl-diff` — the release gate for §5.3.
//!
//! Compares a released contract against a proposed one and refuses the
//! proposal if it would break deployed peers. The refusal is the product; a
//! report nobody is obliged to read would not have caught the swapped struct
//! members that `spike-evolution` shows omniORB accepting in silence.
//!
//! ```text
//! idl-diff [-I <dir>]... <released.idl> <proposed.idl>
//!          [--approve <reason> --approver <name>] [--approvals <file>] [--quiet]
//! ```
//!
//! Exit status: 0 accepted, 1 refused, 2 could not run.
//!
//! # An approval is a record, not a flag
//!
//! `--approve` does not make a change safe. It records that somebody took
//! responsibility for it — and until 2026-08-19 "records" meant *printed*: the
//! reason went to stdout, the run exited 0, and the next run had no way to
//! know a decision had ever been taken. Now every blocking finding accepted
//! under `--approve` is appended to an approval store
//! ([`orbweaver_registry::approval`]): `--approvals <file>`, or by default
//! `<proposed>.approvals.tsv` beside the proposed contract, so the decision
//! travels in version control with the revision it is about.
//!
//! **Who.** `--approver <name>` (or `ORBWEAVER_APPROVER`) is required whenever
//! `--approve` is given; without it the run exits 2. There is no identity in
//! this binary to infer one from, and a decision with no name on it is not a
//! decision on record. Nothing is signed or verified — this is the name that
//! used to be in a chat log, put where a reviewer can diff it.
//!
//! **Which bytes.** A row binds to the SHA-256 of both translation units. On a
//! later run the store is read (only if it exists — nothing is read or written
//! when no `--approve` is given and no store is there), a blocking finding
//! covered by a row is reported as `[approved by <who>: <reason>]` and does not
//! fail the gate, and a finding whose only row was given for *other* bytes is
//! still refused, saying so: an edited contract needs a new approval.
//!
//! **Idempotent.** A finding already covered is not written again, so a re-run
//! of the same diff under the same approval leaves the store byte-identical;
//! a fresh store for the same diff differs from the last one only in
//! `approved_at` (findings are in the differ's stable order), and not even in
//! that under `SOURCE_DATE_EPOCH`.
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
//! gate has. The fingerprint covers every file in the unit for the same
//! reason: an approval of a header-only change must stop applying when the
//! header changes again.
//!
//! *포함된 헤더에만 있는 파괴적 변경을 게이트가 통과시켰다. 이제 `#include`를
//! 해석하며, 해석하지 못한 참조가 있으면 판정 자체를 거부한다. 승인은 이름과
//! 바이트에 묶인 기록이며, 출력 한 줄이 아니다.*

use std::path::{Path, PathBuf};

use orbweaver_registry::approval::{self, Approval};
use orbweaver_registry::diff::{Change, diff};
use orbweaver_registry::{Registry, Strictness, take_include_dirs};

const USAGE: &str = "usage: idl-diff [-I <dir>]... <released.idl> <proposed.idl> \
                     [--approve <reason> --approver <name>] [--approvals <file>] [--quiet]";

fn main() -> std::process::ExitCode {
    let mut released = None;
    let mut proposed = None;
    let mut reason: Option<String> = None;
    let mut approver: Option<String> = None;
    let mut store_path: Option<PathBuf> = None;
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
                Some(r) if !r.trim().is_empty() => reason = Some(r),
                _ => {
                    eprintln!("--approve needs a reason");
                    return std::process::ExitCode::from(2);
                }
            },
            "--approver" => match args.next() {
                Some(n) if !n.trim().is_empty() => approver = Some(n.trim().to_owned()),
                _ => {
                    eprintln!("--approver needs a name");
                    return std::process::ExitCode::from(2);
                }
            },
            "--approvals" => match args.next() {
                Some(p) if !p.trim().is_empty() => store_path = Some(PathBuf::from(p)),
                _ => {
                    eprintln!("--approvals needs a file");
                    return std::process::ExitCode::from(2);
                }
            },
            "--quiet" => quiet = true,
            "-h" | "--help" => {
                println!("{USAGE}");
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
        eprintln!("{USAGE}");
        return std::process::ExitCode::from(2);
    };

    // The approver decision, made once and before any file is read: an
    // approval names somebody or it is not given. `ORBWEAVER_APPROVER` is the
    // default for a person who approves often; the flag wins when both are set.
    let approver = approver.or_else(|| {
        std::env::var("ORBWEAVER_APPROVER")
            .ok()
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    });
    if reason.is_some() && approver.is_none() {
        eprintln!(
            "--approve needs --approver <name> (or ORBWEAVER_APPROVER): a decision with no name \
             on it is not a decision on record"
        );
        return std::process::ExitCode::from(2);
    }

    let (a, b) = match (load(&released, &search), load(&proposed, &search)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    };
    let (a, released_sha) = a;
    let (b, proposed_sha) = b;

    // The store: read if it is there, whether or not this run approves. A
    // named store that is not there is "nothing on record" — the safe
    // direction is a refusal, and the output names the path so a typo shows.
    let store_path = store_path.unwrap_or_else(|| approval::default_store(Path::new(&proposed)));
    let store = match approval::read_store(&store_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    };

    let changes = diff(&a, &b);
    let mut uncovered: Vec<&Change> = Vec::new();
    let mut covered = 0usize;
    let mut stale = 0usize;

    if !quiet && changes.is_empty() {
        println!("no change between {released} and {proposed}");
    }
    for c in &changes {
        if !quiet {
            println!("{c}");
        }
        if !c.verdict.blocks_release() {
            continue;
        }
        let row = store.as_ref().and_then(|s| s.covering(&released_sha, &proposed_sha, c));
        match row {
            Some(r) => {
                covered += 1;
                if !quiet {
                    println!("    [approved by {}: {}] {}", r.approver, r.reason, r.approved_at);
                }
            }
            None => {
                if let Some(s) =
                    store.as_ref().and_then(|s| s.stale_for(&released_sha, &proposed_sha, c))
                {
                    stale += 1;
                    if !quiet {
                        println!(
                            "    [approval by {} on {} was for a different revision — the \
                             {} has changed since; not applied]",
                            s.approver,
                            s.approved_at,
                            which_side_changed(s, &released_sha, &proposed_sha)
                        );
                    }
                }
                uncovered.push(c);
            }
        }
    }
    if let (false, Some(s)) = (quiet, &store) {
        println!(
            "\napprovals: {} — {} row(s) on record, {covered} apply to this diff",
            s.path.display(),
            s.approvals.len()
        );
    }

    match (uncovered.len(), reason) {
        (0, _) if covered == 0 => {
            if !quiet {
                println!("\naccepted: nothing here breaks a deployed peer");
            }
            std::process::ExitCode::SUCCESS
        }
        (0, _) => {
            println!(
                "\naccepted under approval on record: {covered} change(s) that break deployed \
                 peers are approved in {}",
                store_path.display()
            );
            std::process::ExitCode::SUCCESS
        }
        (n, Some(reason)) => {
            // Only the approver can reach here; the check above guarantees it.
            let approver = approver.unwrap_or_default();
            let approved_at = approval::now_iso8601();
            let rows: Vec<Approval> = uncovered
                .iter()
                .map(|c| Approval {
                    released: released.clone(),
                    proposed: proposed.clone(),
                    released_sha256: released_sha.clone(),
                    proposed_sha256: proposed_sha.clone(),
                    id: c.id.clone(),
                    verdict: c.verdict.label().to_owned(),
                    what: c.what.clone(),
                    reason: reason.clone(),
                    approver: approver.clone(),
                    approved_at: approved_at.clone(),
                })
                .collect();
            if let Err(e) = approval::append(&store_path, &rows) {
                eprintln!("{e}");
                return std::process::ExitCode::from(2);
            }
            println!("\naccepted under approval: {reason}");
            println!(
                "{n} change(s) will break deployed peers; recorded to {} as {approver}, \
                 {approved_at}",
                store_path.display()
            );
            std::process::ExitCode::SUCCESS
        }
        (n, None) => {
            println!("\nrefused: {n} change(s) break deployed peers");
            if covered > 0 {
                println!("{covered} other(s) are approved on record and are not what refuses this");
            }
            if stale > 0 {
                println!(
                    "{stale} of them carried an approval for a different revision of the \
                     contract; an edited file needs a new approval"
                );
            }
            println!(
                "a released type is not editable in place — publish a new version of the \
                 interface, or re-run with --approve <reason> --approver <name> \
                 (docs/PLAN.md §5.3)"
            );
            std::process::ExitCode::FAILURE
        }
    }
}

/// Names the side whose bytes no longer match a stale row: a reader deciding
/// whether to re-approve wants to know which file moved.
fn which_side_changed(row: &Approval, released_sha: &str, proposed_sha: &str) -> &'static str {
    match (row.released_sha256 == released_sha, row.proposed_sha256 == proposed_sha) {
        (false, false) => "released and proposed contracts have both",
        (false, true) => "released contract",
        _ => "proposed contract",
    }
}

/// Builds a registry from one contract file, `#include`s resolved, and the
/// fingerprint of every file that went into it.
///
/// An unresolved reference is fatal *here* rather than a note in the report.
/// The report is advisory by nature — a reader decides what to do with it —
/// but the exit code is not, and an exit code computed from a graph with a base
/// interface missing is a claim this tool has no evidence for. Exit 2 is "could
/// not run", which is exactly what happened.
fn load(path: &str, search: &orbweaver_idl::SearchPath) -> Result<(Registry, String), String> {
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
    let sha = approval::fingerprint(&contract.unit.files).map_err(|e| e.to_string())?;
    Ok((r, sha))
}
