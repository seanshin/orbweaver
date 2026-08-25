//! §4.8's audit requirement, met by the **process** and not only by the library.
//!
//! `orbweaver-mcp-server` drained `Bridge::audit()` in its `--dry-run` branch
//! and nowhere else, so a serving deployment kept every ALLOW and REFUSE line
//! in memory and dropped them at exit. Every library test still passed: the
//! chain wrote the lines, `Bridge::audit()` returned them, and nobody read it.
//! That is the shape of gap only a process-level test can close, so these tests
//! spawn the real binary, drive real `tools/call` frames over stdio, and assert
//! the decisions are **outside** the process — in a stream an operator has.
//!
//! The second test is the §4.5 #2 seat reaching the same place: a `--quota`
//! refusal has to be readable in that ledger *and* tellable apart from a policy
//! refusal, or an operator debugging a stuck agent cannot tell "you may not"
//! from "not right now".
//!
//! # Harness discipline
//!
//! Per `CLAUDE.md`: every wait here is deadline-bounded (`recv_timeout` on a
//! reader thread, never a spin), nothing is piped into `grep -q`, and the child
//! is driven to EOF rather than polled. The listener is bound and never
//! `accept()`ed — the frames below are chosen so that nothing reaches the wire:
//! a call the gate allows fails at argument mapping, and every other one is
//! refused by the gate.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use orbweaver_dynamic::json::Json;
use orbweaver_giop::{IiopProfile, Ior, Version};

/// How long any single line of output may take to arrive. Generous: it bounds
/// a hang, it does not measure anything.
const DEADLINE: Duration = Duration::from_secs(30);

const IDL: &str = "module bank {
  interface Account {
    //@ ai_effect: read_only
    long balance();
    //@ ai_effect: idempotent
    void deposit(in long cents);
  };
};
";

/// The same estate one annotation short: `sweep` is what every legacy contract
/// on every legacy disk looks like to this bridge — an operation nobody ever
/// described. Kept in its own contract so the tests above measure an annotated
/// deployment and the ones below measure an unannotated one.
const UNANNOTATED_IDL: &str = "module bank {
  interface Account {
    //@ ai_effect: read_only
    long balance();
    void sweep();
  };
};
";

const ACCOUNT: &str = "IDL:bank/Account:1.0";

/// Reads a stream to EOF on its own thread, one line per message.
///
/// A thread rather than an inline read so that every wait in a test body can be
/// deadline-bounded: a blocking read on a pipe whose writer is alive and silent
/// has no deadline at all.
fn lines_of(stream: impl std::io::Read + Send + 'static) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { return };
            if tx.send(line).is_err() {
                return;
            }
        }
    });
    rx
}

/// Everything remaining on a receiver, until its sender closes or the deadline
/// passes.
fn drain(rx: &mpsc::Receiver<String>) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(line) = rx.recv_timeout(DEADLINE) {
        out.push(line);
    }
    out
}

/// What the process wrote where.
struct Served {
    /// stderr, minus the root-handle line: the diagnostics and the ledger.
    err: Vec<String>,
    /// stdout: the protocol, one JSON object per line.
    out: Vec<String>,
}

/// Runs the real binary against a target that never answers, feeding it the
/// frames `frames(root_handle)` builds, and returns both streams.
fn serve(name: &str, extra: &[&str], frames: impl Fn(&str) -> Vec<String>) -> Served {
    serve_contract(name, IDL, extra, frames)
}

/// The same, over a named contract, so a test can measure an unannotated
/// deployment without changing what the others measure.
fn serve_contract(
    name: &str,
    idl: &str,
    extra: &[&str],
    frames: impl Fn(&str) -> Vec<String>,
) -> Served {
    let dir =
        std::env::temp_dir().join(format!("orbweaver-mcp-audit-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let idl_path = dir.join("bank.idl");
    let ior_path = dir.join("account.ior");
    std::fs::write(&idl_path, idl).expect("writes the contract");

    // A listener that is bound and never accepted: `connect` completes from the
    // backlog, which is all the server needs to start serving.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
    let ior = Ior {
        type_id: ACCOUNT.into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "127.0.0.1".into(),
            port: listener.local_addr().expect("bound").port(),
            object_key: b"acct-1".to_vec(),
            components: Vec::new(),
        }],
    };
    std::fs::write(&ior_path, ior.to_stringified().expect("stringifies")).expect("writes the IOR");

    let mut child = Command::new(env!("CARGO_BIN_EXE_orbweaver-mcp-server"))
        .args(["--idl", idl_path.to_str().expect("utf-8")])
        .args(["--ior", ior_path.to_str().expect("utf-8")])
        .args(["--as", "alice"])
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the server binary is built by `cargo test`");

    let err = lines_of(child.stderr.take().expect("piped"));
    let out = lines_of(child.stdout.take().expect("piped"));

    // The root handle is printed to stderr before the loop starts; the audit
    // lines follow it on the same stream, which is the property under test.
    let mut preamble = Vec::new();
    let handle = loop {
        let line = err
            .recv_timeout(DEADLINE)
            .unwrap_or_else(|e| panic!("no root handle ({e}); stderr so far: {preamble:#?}"));
        if let Some(h) = line.strip_prefix("root handle: ") {
            break h.to_owned();
        }
        preamble.push(line);
    };

    {
        let mut stdin = child.stdin.take().expect("piped");
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize"}}"#)
            .expect("the server is reading");
        for frame in frames(&handle) {
            writeln!(stdin, "{frame}").expect("the server is reading");
        }
        // Dropped here: EOF is what ends the loop, so the process exits on its
        // own and nothing has to be polled or killed.
    }

    let status = child.wait().expect("the server exits on EOF");
    // The startup diagnostics came before the root handle; they are part of
    // what an operator reads and are put back in front of the ledger.
    preamble.extend(drain(&err));
    let served = Served { err: preamble, out: drain(&out) };
    let _ = std::fs::remove_dir_all(&dir);
    assert!(status.success(), "{status}: {:#?}", served.err);
    served
}

/// One `invoke_operation` frame.
fn call(id: u32, handle: &str, operation: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"invoke_operation","arguments":{{"handle":"{handle}","operation":"{operation}"}}}}}}"#
    )
}

/// A served call leaves its ALLOW and its REFUSE where an operator can read
/// them, and leaves stdout a protocol stream.
#[test]
fn a_served_call_writes_its_audit_line_out_of_the_process() {
    // Only `deposit` is exposed, so `balance` is a refusal the gate makes.
    let served = serve("audit", &["--expose", &format!("{ACCOUNT}.deposit")], |h| {
        vec![
            // Allowed by policy, then refused at argument mapping: the decision
            // was ALLOW and the audit line has to say so.
            call(2, h, "deposit"),
            // Not among the exposed operations: refused by the gate.
            call(3, h, "balance"),
        ]
    });

    assert!(
        served.err.contains(&format!("ALLOW caller=alice target={ACCOUNT} operation=deposit")),
        "the ALLOW never left the process:\n{:#?}",
        served.err
    );
    let refuse = served
        .err
        .iter()
        .find(|l| l.starts_with(&format!("REFUSE caller=alice target={ACCOUNT} operation=balance")))
        .unwrap_or_else(|| panic!("the REFUSE never left the process:\n{:#?}", served.err));
    assert!(refuse.contains(" why="), "a refusal without its reason: {refuse}");

    // And the ledger did not land on the protocol stream: stdout is JSON, one
    // object per line, and nothing else. Ever.
    assert_eq!(served.out.len(), 3, "{:#?}", served.out);
    for line in &served.out {
        assert!(Json::parse(line).is_ok(), "stdout is the protocol: {line}");
        assert!(!line.contains("ALLOW ") && !line.contains("REFUSE "), "{line}");
    }
}

/// §4.5 #2 reaching the ledger: a `--quota` refusal is in the audit stream, and
/// an operator can tell it from a policy refusal without knowing which
/// operations are exposed.
#[test]
fn a_quota_refusal_reaches_the_ledger_and_reads_differently_from_a_policy_refusal() {
    let served = serve(
        "quota",
        &["--expose", &format!("{ACCOUNT}.deposit"), "--quota", "1", "--quota-scope", "caller"],
        |h| vec![call(2, h, "deposit"), call(3, h, "deposit"), call(4, h, "balance")],
    );

    let audit: Vec<&String> =
        served.err.iter().filter(|l| l.starts_with("ALLOW ") || l.starts_with("REFUSE ")).collect();
    assert_eq!(audit.len(), 3, "{:#?}", served.err);
    assert!(audit[0].starts_with("ALLOW "), "{}", audit[0]);
    assert!(
        audit[1].contains("why=quota exhausted:") && audit[1].contains("1 of 1 calls"),
        "the budget's refusal, with its arithmetic: {}",
        audit[1]
    );
    assert!(
        audit[1].contains("does not renew"),
        "this process opens no windows and must not invite a retry: {}",
        audit[1]
    );
    assert!(
        !audit[2].contains("quota"),
        "and a policy refusal is still a policy refusal: {}",
        audit[2]
    );
    // The operator is told about the budget at startup, not only when it bites.
    assert!(
        served.err.iter().any(|l| l.starts_with("quota: 1 calls per caller")),
        "{:#?}",
        served.err
    );
}

/// The bounded ledger, measured where it can actually hurt: the **stream** an
/// operator keeps must stay complete even when the in-memory ledger is dropping
/// lines behind it.
///
/// This is the test the watermark bug would fail. `emit_audit` used to hold an
/// index into `Bridge::audit()`; the moment a bounded ledger drops its oldest
/// line every index means a different line than it did, so an index-based
/// watermark re-emits the tail once and then skips a line for the rest of the
/// session. Both failures are invisible to a library test — the chain is
/// perfectly happy — and both corrupt the one record §4.8 is about. Run with a
/// ceiling of two against four decisions, it is unmissable.
#[test]
fn a_ledger_small_enough_to_drop_still_emits_every_line_exactly_once() {
    let served = serve(
        "bounded",
        &["--expose", &format!("{ACCOUNT}.deposit"), "--audit-capacity", "2"],
        |h| {
            vec![
                call(2, h, "deposit"),
                call(3, h, "balance"),
                call(4, h, "deposit"),
                call(5, h, "balance"),
            ]
        },
    );

    let audit: Vec<&String> =
        served.err.iter().filter(|l| l.starts_with("ALLOW ") || l.starts_with("REFUSE ")).collect();
    // Four decisions, in call order, none lost to the ceiling and none doubled
    // by it — while the ledger held at most two of them at any moment.
    assert_eq!(audit.len(), 4, "every decision is on the stream once: {:#?}", served.err);
    for (i, line) in audit.iter().enumerate() {
        let expected = if i % 2 == 0 { "ALLOW " } else { "REFUSE " };
        assert!(line.starts_with(expected), "line {i} out of order or duplicated: {line}");
    }
    // Nothing was dropped before it left the process: the ceiling bounds what
    // is *held*, and emission happens per frame, ahead of it.
    assert!(
        !served.err.iter().any(|l| l.starts_with("audit: ")),
        "no line should have been dropped unemitted: {:#?}",
        served.err
    );
    // And the operator was told the ledger is bounded at startup, in the same
    // stream as the lines it will eventually elide.
    assert!(
        served.err.iter().any(|l| l.starts_with("audit ledger: the newest 2 lines")),
        "{:#?}",
        served.err
    );
}

/// **The estate defect at the process boundary.** Expose an operation whose
/// contract states no `ai_effect` and the deployment must refuse it, tell the
/// operator how big the silence is *before* anything is called, and name the
/// annotation rather than the allowlist.
///
/// The library test `an_operation_the_contract_says_nothing_about_is_refused`
/// pins the gate; this pins the thing an operator actually runs. RC-5 was a
/// property of a real process's real output, and a library assertion would not
/// have caught it any more than it caught the audit lines that never left the
/// process.
#[test]
fn an_unannotated_operation_is_refused_by_the_process_and_the_silence_is_counted() {
    let served = serve_contract(
        "unannotated",
        UNANNOTATED_IDL,
        &["--expose", &format!("{ACCOUNT}.sweep")],
        |h| vec![call(2, h, "sweep")],
    );

    // Said before the loop, not discovered one refusal at a time.
    //
    // The expected text is **computed by calling the function that writes it**
    // rather than retyped here. This assertion used to quote
    // `"carry no ai_effect and will be REFUSED"`, a fragment of a sentence
    // another crate owns — the classifier-is-a-sentence shape — and it went red
    // on 2026-08-26 for the right reason: the sentence moved to one home
    // (`orbweaver_forge::effect`) and the retyped copy did not move with it.
    let expected = orbweaver_forge::effect::annotate_or_assume(
        &orbweaver_forge::effect::OFFER_AUTHOR,
        Some("--assume-effect <value>"),
    );
    assert!(
        served.err.iter().any(|l| {
            l.contains(orbweaver_forge::effect::SILENCE)
                && l.contains("REFUSED")
                && l.contains(&expected)
        }),
        "the size of the silence must be stated at startup, in the words \
         `orbweaver_forge::effect` writes:\n  {expected}\n{:#?}",
        served.err
    );
    let refuse = served
        .err
        .iter()
        .find(|l| l.starts_with("REFUSE ") && l.contains("operation=sweep"))
        .unwrap_or_else(|| panic!("no refusal for sweep:\n{:#?}", served.err));
    // The actionable half: what is missing, and where it goes. Computed from
    // the one home rather than retyped — and note the gate's offer is
    // deliberately narrower than the author's (`OFFER_GATE`, two poles), so
    // asserting the author's list here would be asserting the wrong sentence.
    assert!(refuse.contains("carries no ai_effect"), "{refuse}");
    assert!(
        refuse.contains(&orbweaver_forge::effect::annotate_or_assume(
            &orbweaver_forge::effect::OFFER_GATE,
            None
        )),
        "{refuse}"
    );
    // And it must not read as a permissions misconfiguration, which is the
    // failure mode the estate recorded arriving by another road.
    assert!(!refuse.contains("is not exposed"), "{refuse}");
    assert!(
        !served.err.iter().any(|l| l.starts_with("ALLOW ")),
        "nothing may be allowed:\n{:#?}",
        served.err
    );
}

/// The operator's one declaration, at the process boundary: `--assume-effect`
/// covers every silence at once, and the deployment says out loud that the
/// allows now rest on an assumption nobody's contract makes.
///
/// This is what keeps failing closed usable. Without it, a legacy estate is
/// seventy-six refusals an operator clears one at a time, and a gate cleared
/// that way is a gate that has been routed around.
#[test]
fn one_assumption_covers_every_silence_and_the_process_says_it_is_an_assumption() {
    let served = serve_contract(
        "assumed",
        UNANNOTATED_IDL,
        &["--expose", &format!("{ACCOUNT}.sweep"), "--assume-effect", "read_only"],
        |h| vec![call(2, h, "sweep")],
    );

    assert!(
        served.err.iter().any(|l| {
            l.contains("--assume-effect \"read_only\"")
                && l.contains("This is an assumption made here, not a statement in any contract.")
        }),
        "the assumption must be disclosed at startup:\n{:#?}",
        served.err
    );
    // Allowed by policy, then failed at argument mapping — nothing reached the
    // wire, and the decision the ledger records is the policy's.
    assert!(
        served.err.contains(&format!("ALLOW caller=alice target={ACCOUNT} operation=sweep")),
        "{:#?}",
        served.err
    );
}

/// Runs the binary in `--dry-run` and returns its one stdout document.
///
/// A helper of its own rather than [`serve_contract`] with a flag: a dry run
/// prints no root handle and never enters the loop, so waiting for one would
/// wait out the deadline and report a hang as a missing handle.
fn dry_run(name: &str, idl: &str, extra: &[&str]) -> Json {
    let dir = std::env::temp_dir().join(format!("orbweaver-mcp-dry-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let idl_path = dir.join("bank.idl");
    std::fs::write(&idl_path, idl).expect("writes the contract");
    let out = Command::new(env!("CARGO_BIN_EXE_orbweaver-mcp-server"))
        .args(["--idl", idl_path.to_str().expect("utf-8")])
        .args(["--as", "alice", "--dry-run"])
        .args(extra)
        .output()
        .expect("the server binary is built by `cargo test`");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "{}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    Json::parse(&String::from_utf8_lossy(&out.stdout)).expect("one JSON object on stdout")
}

/// The dry-run document an operator signs, over an unannotated contract, under
/// both postures. This is the "unusable gate" finding closed: the report only
/// carries signal when its answers vary, and every row that rests on an
/// assumption has to say so or the page is indistinguishable from an annotated
/// deployment's.
#[test]
fn the_dry_run_document_names_the_posture_and_marks_what_rests_on_it() {
    let doc = dry_run("dry-refuse", UNANNOTATED_IDL, &["--expose", ACCOUNT]);
    assert_eq!(doc.get("unannotated_effect").and_then(Json::as_str), Some("refuse"), "{doc}");
    let summary = doc.get("summary").expect("a summary");
    // Two operations, two different answers: `balance` is described and
    // `sweep` is not. A document whose every row said the same word would be
    // correct and carry nothing.
    assert_eq!(summary.get("allow"), Some(&Json::Number("1".into())), "{doc}");
    assert_eq!(summary.get("need_effect"), Some(&Json::Number("1".into())), "{doc}");

    let doc = dry_run(
        "dry-assume",
        UNANNOTATED_IDL,
        &["--expose", ACCOUNT, "--assume-effect", "read_only"],
    );
    assert_eq!(doc.get("unannotated_effect").and_then(Json::as_str), Some("read_only"), "{doc}");
    // Both are `allow` now, and only one of them is the contract's word.
    assert_eq!(
        doc.get("summary").and_then(|s| s.get("allow")),
        Some(&Json::Number("2".into())),
        "{doc}"
    );
    assert!(doc.to_string().contains(r#""effect_stated_by":"exposure""#), "{doc}");
}
