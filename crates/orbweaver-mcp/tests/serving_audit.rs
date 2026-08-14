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
    void deposit(in long cents);
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
    let dir =
        std::env::temp_dir().join(format!("orbweaver-mcp-audit-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let idl_path = dir.join("bank.idl");
    let ior_path = dir.join("account.ior");
    std::fs::write(&idl_path, IDL).expect("writes the contract");

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
