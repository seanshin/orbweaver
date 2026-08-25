//! The operator surface, measured through the **process** rather than the
//! library.
//!
//! D015 §3.1 names three clauses of its acceptance sentence that reached no
//! declarative surface: *how long* (the handle TTL — a `const` and a builder
//! reachable only from tests, `ttl` appearing zero times in all three of this
//! crate's binaries), *how often* (the quota's two numbers) and *who may call
//! what* (the allowlist). A configuration file supplies all three now, and the
//! only test that can say so is one that starts the real binary: a parser test
//! proves a document was read, not that anything was installed.
//!
//! Each test below is one of the three settings **taking effect** — an expired
//! root handle, an exhausted budget, an operation reachable that no flag
//! named — plus the two properties that make the file safe to ship: an absent
//! or empty configuration changes nothing and widens nothing, and a malformed
//! one stops the process naming the file and the key.
//!
//! # Harness discipline
//!
//! Per `CLAUDE.md`, and copied from `serving_audit.rs` for the same reasons:
//! every wait is deadline-bounded on a reader thread rather than a spin,
//! nothing is piped into `grep -q`, and the child is driven to EOF instead of
//! polled. The listener is bound and never `accept()`ed, and every frame here
//! is one the gate or the mapper stops, so nothing reaches the wire.
//!
//! The one deliberate sleep is in `an_expiry_from_the_file_retires_the_root_handle`:
//! a TTL is a duration and the only way to observe one elapsing is to let it
//! elapse. It sleeps past a one-second lifetime, which bounds nothing and
//! measures the one thing under test.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use orbweaver_dynamic::json::Json;
use orbweaver_giop::{IiopProfile, Ior, Version};
use orbweaver_mcp::ToolError;

/// The sentence a tool error reaches the protocol stream as — computed by
/// calling the function that writes it and the encoder that escapes it, never
/// retyped. A test that matched a hand-written prefix would go green the day
/// the wording changed for a good reason, which is the defect `CLAUDE.md`
/// records under *a classifier is a sentence too*.
fn on_the_wire(e: &ToolError) -> String {
    Json::String(e.to_string()).to_string()
}

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

const ACCOUNT: &str = "IDL:bank/Account:1.0";

/// Reads a stream to EOF on its own thread, one line per message.
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

/// What the process wrote where, and what it exited with.
struct Ran {
    err: Vec<String>,
    out: Vec<String>,
    code: Option<i32>,
}

impl Ran {
    /// stderr with the root handle's token replaced, so two runs that differ
    /// only in 128 bits of entropy compare equal.
    fn steady_err(&self) -> Vec<String> {
        self.err
            .iter()
            .map(|l| {
                if l.starts_with("root handle: ") {
                    "root handle: <token>".to_owned()
                } else {
                    l.clone()
                }
            })
            .collect()
    }

    /// The audit lines, in order.
    fn ledger(&self) -> Vec<&String> {
        self.err.iter().filter(|l| l.starts_with("ALLOW ") || l.starts_with("REFUSE ")).collect()
    }
}

/// A scratch directory holding a contract, an IOR pointing at a bound-and-never-
/// accepted listener, and whatever configuration a test wrote.
struct Fixture {
    dir: std::path::PathBuf,
    idl: String,
    ior: String,
    // Held so the port stays bound for the life of the fixture: a `connect`
    // completes from the backlog and nothing ever accepts.
    _listener: std::net::TcpListener,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir()
            .join(format!("orbweaver-mcp-deployment-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let idl = dir.join("bank.idl");
        std::fs::write(&idl, IDL).expect("writes the contract");
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
        let ior_path = dir.join("account.ior");
        std::fs::write(&ior_path, ior.to_stringified().expect("stringifies"))
            .expect("writes the IOR");
        Self {
            idl: idl.to_str().expect("utf-8").to_owned(),
            ior: ior_path.to_str().expect("utf-8").to_owned(),
            dir,
            _listener: listener,
        }
    }

    /// Writes a configuration file and returns its path.
    fn config(&self, name: &str, text: &str) -> String {
        let path = self.dir.join(name);
        std::fs::write(&path, text).expect("writes the configuration");
        path.to_str().expect("utf-8").to_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Runs the binary and returns both streams. `frames` is given the root handle;
/// `pause` is waited out after the handle is issued and before the frames go in.
///
/// A run that never prints a root handle — a refused configuration, for
/// instance — returns whatever it did print, which is what the refusal tests
/// read.
fn run(fx: &Fixture, extra: &[&str], pause: Duration, frames: impl Fn(&str) -> Vec<String>) -> Ran {
    let mut child = Command::new(env!("CARGO_BIN_EXE_orbweaver-mcp-server"))
        .args(["--idl", &fx.idl])
        .args(["--ior", &fx.ior])
        .args(["--as", "alice"])
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the server binary is built by `cargo test`");

    let err = lines_of(child.stderr.take().expect("piped"));
    let out = lines_of(child.stdout.take().expect("piped"));

    let mut preamble = Vec::new();
    let mut handle = None;
    while let Ok(line) = err.recv_timeout(DEADLINE) {
        if let Some(h) = line.strip_prefix("root handle: ") {
            handle = Some(h.to_owned());
            preamble.push(line);
            break;
        }
        preamble.push(line);
    }

    {
        let mut stdin = child.stdin.take().expect("piped");
        if let Some(handle) = &handle {
            let _ = writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize"}}"#);
            if !pause.is_zero() {
                std::thread::sleep(pause);
            }
            for frame in frames(handle) {
                let _ = writeln!(stdin, "{frame}");
            }
        }
        // Dropped: EOF ends the loop, so nothing is polled or killed.
    }

    let status = child.wait().expect("the server exits on EOF");
    preamble.extend(drain(&err));
    Ran { err: preamble, out: drain(&out), code: status.code() }
}

/// One `invoke_operation` frame.
fn call(id: u32, handle: &str, operation: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"invoke_operation","arguments":{{"handle":"{handle}","operation":"{operation}"}}}}}}"#
    )
}

// ── Defaults ───────────────────────────────────────────────────────────────

/// The property everything else rests on: **an empty configuration is
/// indistinguishable from no configuration.**
///
/// Not "produces the same verdicts" — byte-for-byte the same stderr, with only
/// the root handle's 128 bits of entropy normalised away. If a document that
/// says nothing produced one extra line, some setting would be installing
/// itself by default, and the deployment that never opts in would be the one
/// that finds out.
#[test]
fn an_empty_configuration_says_exactly_what_no_configuration_says() {
    let fx = Fixture::new("defaults");
    let expose = format!("{ACCOUNT}.deposit");
    let empty = fx.config("empty.json", "{}");

    let without = run(&fx, &["--expose", &expose], Duration::ZERO, |h| {
        vec![call(2, h, "deposit"), call(3, h, "balance")]
    });
    let with = run(&fx, &["--expose", &expose, "--config", &empty], Duration::ZERO, |h| {
        vec![call(2, h, "deposit"), call(3, h, "balance")]
    });

    assert_eq!(without.code, Some(0), "{:#?}", without.err);
    assert_eq!(with.steady_err(), without.steady_err(), "an empty document changed the run");
    assert_eq!(with.out, without.out, "an empty document changed the protocol stream");
    // And the run really did decide something, so this is not two empty lists.
    assert_eq!(without.ledger().len(), 2, "{:#?}", without.err);
}

/// Nothing about the file's absence installs anything: the three announcements
/// a configured deployment reads are absent, which is how an operator can tell
/// an unconfigured process from one whose file went missing.
#[test]
fn without_a_configuration_nothing_is_announced_and_nothing_is_installed() {
    let fx = Fixture::new("silent");
    let ran = run(&fx, &["--expose", ACCOUNT], Duration::ZERO, |h| vec![call(2, h, "deposit")]);
    for said in ["handles: ", "quota: ", "audit ledger: ", "search: "] {
        assert!(
            !ran.err.iter().any(|l| l.starts_with(said)),
            "{said:?} was announced by a run that configured nothing:\n{:#?}",
            ran.err
        );
    }
}

// ── Default-deny ───────────────────────────────────────────────────────────

/// **No form of the file widens exposure.** An empty document, an empty
/// `expose`, and a document that configures everything *except* exposure all
/// leave a flagless run finding nothing — and the process says so at startup,
/// which is the line an operator reads when an agent reports an empty catalog.
#[test]
fn no_configuration_form_widens_an_exposure_nobody_wrote() {
    let fx = Fixture::new("deny");
    let forms = [
        ("empty.json", "{}"),
        ("empty-list.json", r#"{"expose":[]}"#),
        ("elsewhere.json", r#"{"handles":{"ttl_seconds":600},"search":{"default_limit":5}}"#),
    ];
    for (name, text) in forms {
        let path = fx.config(name, text);
        let ran = run(&fx, &["--config", &path], Duration::ZERO, |h| {
            vec![call(2, h, "deposit"), call(3, h, "balance")]
        });
        assert_eq!(ran.code, Some(0), "{name}: {:#?}", ran.err);
        assert!(
            ran.err.iter().any(|l| l.starts_with("no --expose given:")),
            "{name}: an empty allowlist must say so:\n{:#?}",
            ran.err
        );
        let ledger = ran.ledger();
        assert_eq!(ledger.len(), 2, "{name}: {:#?}", ran.err);
        for line in ledger {
            assert!(line.starts_with("REFUSE "), "{name}: default-deny did not hold: {line}");
        }
    }
}

// ── Who may call what ──────────────────────────────────────────────────────

/// *Who may call what*, without a rebuild **and without a flag**: the operation
/// the agent reaches is named in a file and nowhere else, and its neighbour on
/// the same interface is still refused.
#[test]
fn an_exposure_from_the_file_is_the_one_the_gate_enforces() {
    let fx = Fixture::new("expose");
    let path = fx.config("expose.json", &format!(r#"{{"expose":["{ACCOUNT}.deposit"]}}"#));
    let ran = run(&fx, &["--config", &path], Duration::ZERO, |h| {
        vec![call(2, h, "deposit"), call(3, h, "balance")]
    });

    assert_eq!(ran.code, Some(0), "{:#?}", ran.err);
    assert!(
        !ran.err.iter().any(|l| l.starts_with("no --expose given:")),
        "the file named an interface:\n{:#?}",
        ran.err
    );
    let ledger = ran.ledger();
    assert_eq!(ledger.len(), 2, "{:#?}", ran.err);
    assert!(
        ledger[0].starts_with(&format!("ALLOW caller=alice target={ACCOUNT} operation=deposit")),
        "the file's grant did not reach the gate: {}",
        ledger[0]
    );
    assert!(
        ledger[1].starts_with(&format!("REFUSE caller=alice target={ACCOUNT} operation=balance")),
        "the file's grant widened past what it named: {}",
        ledger[1]
    );
}

/// A file adds to the command line rather than replacing it: both are the
/// operator naming something explicitly, and an allowlist that silently dropped
/// half of what was written would be the worst of the two failure directions
/// even though it errs closed.
#[test]
fn the_file_and_the_flag_are_both_in_force() {
    let fx = Fixture::new("union");
    let path = fx.config("half.json", &format!(r#"{{"expose":["{ACCOUNT}.deposit"]}}"#));
    let ran = run(
        &fx,
        &["--config", &path, "--expose", &format!("{ACCOUNT}.balance")],
        Duration::ZERO,
        |h| vec![call(2, h, "deposit"), call(3, h, "balance")],
    );
    let ledger = ran.ledger();
    assert_eq!(ledger.len(), 2, "{:#?}", ran.err);
    for line in ledger {
        assert!(line.starts_with("ALLOW "), "one of the two grants was dropped: {line}");
    }
}

// ── How often ──────────────────────────────────────────────────────────────

/// *How often*: the budget in the file is the budget the seat enforces, and its
/// refusal is the same one `--quota` earns — the arithmetic in the line, and no
/// invitation to retry a window this process will never open.
#[test]
fn a_quota_from_the_file_refuses_the_call_over_the_budget() {
    let fx = Fixture::new("quota");
    let path = fx.config(
        "quota.json",
        &format!(r#"{{"expose":["{ACCOUNT}.deposit"],"quota":{{"limit":1,"scope":"caller"}}}}"#),
    );
    let ran = run(&fx, &["--config", &path], Duration::ZERO, |h| {
        vec![call(2, h, "deposit"), call(3, h, "deposit")]
    });

    assert!(
        ran.err.iter().any(|l| l.starts_with("quota: 1 calls per caller")),
        "a budget an operator forgot is one they debug as a policy failure:\n{:#?}",
        ran.err
    );
    let ledger = ran.ledger();
    assert_eq!(ledger.len(), 2, "{:#?}", ran.err);
    assert!(ledger[0].starts_with("ALLOW "), "{}", ledger[0]);
    assert!(
        ledger[1].contains("why=quota exhausted:") && ledger[1].contains("1 of 1 calls"),
        "the budget's refusal, with its arithmetic: {}",
        ledger[1]
    );
    assert!(
        ledger[1].contains("does not renew"),
        "this process opens no windows and must not invite a retry: {}",
        ledger[1]
    );
}

/// A flag beats the file, so an invocation that worked before a configuration
/// existed still means what it meant. Two of two calls are allowed under a
/// budget of two, over a file that says one.
#[test]
fn a_flag_beats_the_files_budget() {
    let fx = Fixture::new("override");
    let path = fx.config(
        "quota.json",
        &format!(r#"{{"expose":["{ACCOUNT}.deposit"],"quota":{{"limit":1}}}}"#),
    );
    let ran = run(&fx, &["--config", &path, "--quota", "2"], Duration::ZERO, |h| {
        vec![call(2, h, "deposit"), call(3, h, "deposit")]
    });
    assert!(ran.err.iter().any(|l| l.starts_with("quota: 2 calls per caller")), "{:#?}", ran.err);
    let ledger = ran.ledger();
    assert_eq!(ledger.len(), 2, "{:#?}", ran.err);
    for line in ledger {
        assert!(line.starts_with("ALLOW "), "the file's smaller budget won: {line}");
    }
}

// ── How long ───────────────────────────────────────────────────────────────

/// *How long*: the clause with no surface at all before this — `ttl` appeared
/// zero times in all three of this crate's binaries and
/// `CapabilityTable::with_ttl` was reachable only from tests.
///
/// The root handle is issued at startup and the call is made after the
/// configured lifetime has elapsed, so the reference the agent holds is one the
/// table no longer resolves. The expected sentence is **computed by calling the
/// function that writes it** rather than retyped, so this test fails when the
/// wording changes instead of quietly passing over a different message.
#[test]
fn an_expiry_from_the_file_retires_the_root_handle() {
    let fx = Fixture::new("ttl");
    let path = fx.config(
        "ttl.json",
        &format!(r#"{{"expose":["{ACCOUNT}"],"handles":{{"ttl_seconds":1}}}}"#),
    );
    let ran = run(&fx, &["--config", &path], Duration::from_millis(1_800), |h| {
        vec![call(2, h, "deposit")]
    });

    assert!(
        ran.err.iter().any(|l| l == "handles: a capability expires 1 second(s) after it is issued"),
        "an expiry policy is said out loud:\n{:#?}",
        ran.err
    );
    let handle = ran
        .err
        .iter()
        .find_map(|l| l.strip_prefix("root handle: "))
        .expect("a root handle was issued");
    let expected = on_the_wire(&ToolError::UnknownHandle(handle.to_owned()));
    assert!(
        ran.out.iter().any(|l| l.contains(&expected)),
        "the handle outlived its configured lifetime; expected {expected:?} in:\n{:#?}",
        ran.out
    );
}

/// The same interface, the same call, the same frames — without the file the
/// handle is still live, so the test above is measuring the configuration and
/// not the sleep. Its negative control, kept beside it rather than in a commit
/// message.
#[test]
fn without_the_expiry_the_same_wait_leaves_the_handle_live() {
    let fx = Fixture::new("ttl-control");
    let ran = run(&fx, &["--expose", ACCOUNT], Duration::from_millis(1_800), |h| {
        vec![call(2, h, "deposit")]
    });
    let handle = ran
        .err
        .iter()
        .find_map(|l| l.strip_prefix("root handle: "))
        .expect("a root handle was issued");
    let unknown = on_the_wire(&ToolError::UnknownHandle(handle.to_owned()));
    assert!(
        !ran.out.iter().any(|l| l.contains(&unknown)),
        "the default lifetime is fifteen minutes, not two seconds:\n{:#?}",
        ran.out
    );
    let ledger = ran.ledger();
    assert_eq!(ledger.len(), 1, "{:#?}", ran.err);
    assert!(ledger[0].starts_with("ALLOW "), "{}", ledger[0]);
}

// ── Refusals ───────────────────────────────────────────────────────────────

/// A malformed configuration stops the process, names the file and the key, and
/// serves nothing — no root handle, no listener dialled, nothing partially
/// applied. Including a key no setting is named by: a typo an operator wrote is
/// a setting they believe is in force, and ignoring it is the silent skip that
/// hides everything else.
#[test]
fn a_malformed_configuration_stops_the_process_naming_the_file_and_the_key() {
    let fx = Fixture::new("refuse");
    let cases: &[(&str, &str, &str)] = &[
        ("ttl-type.json", r#"{"handles":{"ttl_seconds":"15m"}}"#, "handles.ttl_seconds"),
        ("ttl-zero.json", r#"{"handles":{"ttl_seconds":0}}"#, "handles.ttl_seconds"),
        ("typo.json", r#"{"handles":{"ttl_second":900}}"#, "handles.ttl_second"),
        ("scope.json", r#"{"quota":{"limit":5,"scope":"tenant"}}"#, "quota.scope"),
        ("section.json", r#"{"exposure":["IDL:bank/Account:1.0"]}"#, "exposure"),
        ("expose-type.json", r#"{"expose":"IDL:bank/Account:1.0"}"#, "expose"),
        ("broken.json", r#"{"expose":["#, ""),
    ];
    for (name, text, key) in cases {
        let path = fx.config(name, text);
        let ran = run(&fx, &["--config", &path], Duration::ZERO, |_| Vec::new());
        assert_eq!(ran.code, Some(2), "{name} was not refused:\n{:#?}", ran.err);
        assert!(
            ran.err.iter().any(|l| l.contains(&path) && (key.is_empty() || l.contains(key))),
            "{name}: the refusal must name the file and {key:?}:\n{:#?}",
            ran.err
        );
        assert!(
            !ran.err.iter().any(|l| l.starts_with("root handle: ")),
            "{name}: a refused configuration served a session:\n{:#?}",
            ran.err
        );
        assert!(ran.out.is_empty(), "{name}: a refused configuration answered a frame");
    }
}

/// A path that will not open is a refusal, not a fall back to the defaults: a
/// deployment whose policy file was renamed must not quietly start with no
/// policy at all.
#[test]
fn a_configuration_that_will_not_open_is_refused_rather_than_defaulted() {
    let fx = Fixture::new("missing");
    let path = fx.dir.join("was-here.json");
    let path = path.to_str().expect("utf-8");
    let ran = run(&fx, &["--config", path, "--expose", ACCOUNT], Duration::ZERO, |_| Vec::new());
    assert_eq!(ran.code, Some(2), "{:#?}", ran.err);
    assert!(ran.err.iter().any(|l| l.contains(path)), "{:#?}", ran.err);
    assert!(!ran.err.iter().any(|l| l.starts_with("root handle: ")), "{:#?}", ran.err);
}

/// Two policy files with no stated precedence is a deployment nobody can read
/// off the command line.
#[test]
fn two_configurations_are_a_usage_error() {
    let fx = Fixture::new("twice");
    let a = fx.config("a.json", "{}");
    let b = fx.config("b.json", "{}");
    let ran = run(&fx, &["--config", &a, "--config", &b], Duration::ZERO, |_| Vec::new());
    assert_eq!(ran.code, Some(2), "{:#?}", ran.err);
}
