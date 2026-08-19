//! `--dry-run-handle`: the CLI's value-carrying dry run holds a reference
//! without dialing it.
//!
//! 4bb9742 gave the command line a dry run that takes values and reported the
//! gap it left: handles resolve against the run's own capability table, a
//! `--dry-run` issues none, so a declared object reference predicted
//! `would_not_marshal` from the CLI however valid its target — the one
//! instrument an operator has could not ask about `heartbeat(in Expert e, …)`
//! at all. `--dry-run-handle <name>=<IOR|file>` is the repair, and this file
//! measures it where the finding was: in the process, not the library
//! (`guard.rs`'s `a_declared_handle_resolves_in_the_static_dry_run_and_dials_nothing`
//! already holds the library half against a detonating transport).
//!
//! # What is measured
//!
//! - With the flag, `corpus/golden/22`'s `ExpertRegistry::heartbeat` given
//!   `{"e":{"_ref":"expert"},…}` predicts `allow` / `marshals`; without it, or
//!   with a name nothing declared, the same command predicts `marshal` /
//!   `would_not_marshal` with `at e: no reference is held under handle …`.
//! - The IOR is parsed and never connected. The reference points at a
//!   listener this test owns and blocks on `accept()`; after the process has
//!   exited, nothing has arrived within a bounded wait. Not the `Detonator`
//!   the library tests use — a process cannot be handed a panicking transport
//!   — but the same shape: the target is instrumented, and the proof is that
//!   it saw nothing.
//! - The transcript-leak property: nothing about the target — host, port,
//!   object key, the stringified IOR — reaches stdout, stderr or the ledger,
//!   and every ledger line is a `DRYRUN-` line.
//! - The usage errors: a handle nothing can name, a malformed IOR, a missing
//!   file, a name declared twice — each is exit 2 with the flag named, before
//!   anything is predicted.
//!
//! # Harness discipline
//!
//! Per `CLAUDE.md`: the child runs to exit and both pipes are read to EOF by
//! `output()`; the one wait that could hang — the accept thread — is
//! deadline-bounded through `recv_timeout` and sleeps rather than spins.
//! Nothing is piped into `grep -q`.

use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use orbweaver_dynamic::json::Json;
use orbweaver_giop::{IiopProfile, Ior, Version};

const IDL: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../corpus/golden/22-moe-control-plane.idl");
const REGISTRY: &str = "IDL:moe/ExpertRegistry:1.0";
const KEY: &[u8] = b"very-distinctive-object-key";
/// `Capability`, the struct `heartbeat` takes beside the reference.
const CAP: &str = r#"{"id":"x","cost":1,"latency_p99_ms":1,"load":0.5,"state":"RESIDENT","mem_footprint":1,"route_freq":0,"placement_node":"n","contract_version":"1.0"}"#;

/// How long the listener is given, after the process has exited, to report a
/// connection that would already have completed if one had been made.
const GRACE: Duration = Duration::from_millis(300);

/// A target that records contact: a bound listener and the channel its accept
/// thread reports on. Loopback `connect` completes against the backlog whether
/// or not anybody accepts, so the report is what a `connect` from the child
/// would have produced.
struct Target {
    ior: Ior,
    contacted: mpsc::Receiver<std::net::SocketAddr>,
}

fn target() -> Target {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
    let port = listener.local_addr().expect("bound").port();
    let (tx, contacted) = mpsc::channel();
    std::thread::spawn(move || {
        // Blocks until a peer connects or the test ends; a connection is the
        // failure, so it is reported rather than served.
        if let Ok((_, peer)) = listener.accept() {
            let _ = tx.send(peer);
        }
    });
    let ior = Ior {
        type_id: "IDL:moe/Expert:1.0".into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "127.0.0.1".into(),
            port,
            object_key: KEY.to_vec(),
            components: Vec::new(),
        }],
    };
    Target { ior, contacted }
}

struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
}

/// The value-carrying dry run of `heartbeat`, plus whatever `extra` adds.
fn heartbeat(name: &str, args: &str, extra: &[&str]) -> Ran {
    let out = Command::new(env!("CARGO_BIN_EXE_orbweaver-mcp-server"))
        .args(["--idl", IDL])
        .args(["--expose", REGISTRY])
        .args(["--as", "alice", "--scope", "moe.registry.write"])
        .arg(format!("--dry-run={REGISTRY}.heartbeat"))
        .args(["--dry-run-args", args])
        .args(extra)
        .output()
        .unwrap_or_else(|e| panic!("{name}: the server binary is built by `cargo test`: {e}"));
    Ran {
        code: out.status.code(),
        out: String::from_utf8_lossy(&out.stdout).into_owned(),
        err: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("orbweaver-dryhandle-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

fn document(ran: &Ran) -> Json {
    Json::parse(ran.out.trim())
        .unwrap_or_else(|e| panic!("{e}\nstdout:\n{}\nstderr:\n{}", ran.out, ran.err))
}

fn field<'a>(doc: &'a Json, key: &str) -> &'a str {
    doc.get(key).and_then(Json::as_str).unwrap_or_default()
}

/// The repair and its negative control, over the same command: with the flag
/// the reference resolves and the payload marshals; drop the flag and the same
/// arguments predict `marshal` in the mapper's own sentence.
#[test]
fn a_held_reference_resolves_from_the_command_line_and_nothing_is_dialed() {
    let dir = scratch("held");
    let ior_path = dir.join("expert.ior");
    let target = target();
    let stringified = target.ior.to_stringified().expect("stringifies");
    std::fs::write(&ior_path, &stringified).expect("writes the IOR");
    let flag = format!("expert={}", ior_path.display());
    let args = format!(r#"{{"e":{{"_ref":"expert"}},"updated_cap":{CAP}}}"#);

    let held = heartbeat("held", &args, &["--dry-run-handle", &flag]);
    assert_eq!(held.code, Some(0), "stderr:\n{}", held.err);
    let doc = document(&held);
    assert_eq!(field(&doc, "would"), "allow", "{doc}");
    assert_eq!(field(&doc, "payload"), "marshals", "{doc}");
    // The document says what was held, by name, as a token the library
    // issued: `cap_` plus 128 bits of hex, as a live `resolve` would have.
    let token = doc.get("handles").and_then(|h| h.get("expert")).and_then(Json::as_str);
    let token = token.unwrap_or_else(|| panic!("handles.expert: {doc}"));
    assert!(token.starts_with("cap_") && token.len() == 4 + 32, "{token}");
    assert!(held.err.contains(&format!("dry-run handle expert: {token}")), "{}", held.err);

    // The negative control: the same command without the flag. Before this
    // flag existed, this was every answer the CLI could give.
    let dropped = heartbeat("dropped", &args, &[]);
    assert_eq!(dropped.code, Some(0), "stderr:\n{}", dropped.err);
    let doc = document(&dropped);
    assert_eq!(field(&doc, "would"), "marshal", "{doc}");
    assert_eq!(field(&doc, "payload"), "would_not_marshal", "{doc}");
    assert!(
        field(&doc, "payload_why").contains(r#"at e: no reference is held under handle "expert""#),
        "{doc}"
    );
    assert!(doc.get("handles").is_none(), "nothing declared, no section: {doc}");

    // A name nothing declared is left as written and earns the same sentence
    // — the flag is not a wildcard — and the unnamed handle is said out loud.
    let misspelt = format!(r#"{{"e":{{"_ref":"exprt"}},"updated_cap":{CAP}}}"#);
    let unnamed = heartbeat("unnamed", &misspelt, &["--dry-run-handle", &flag]);
    assert_eq!(unnamed.code, Some(0), "stderr:\n{}", unnamed.err);
    let doc = document(&unnamed);
    assert_eq!(field(&doc, "would"), "marshal", "{doc}");
    assert!(field(&doc, "payload_why").contains(r#"under handle "exprt""#), "{doc}");
    assert!(unnamed.err.contains("--dry-run-handle expert is named by nothing"), "{}", unnamed.err);

    // The transcript-leak property, over all three runs: nothing about the
    // target reached anything an operator or an agent reads, and every ledger
    // line is a question. The token itself is stripped first, so a port that
    // happened to appear in 32 hex digits could not fail this by chance.
    let port = target.ior.profiles[0].port.to_string();
    for (name, ran) in [("held", &held), ("dropped", &dropped), ("unnamed", &unnamed)] {
        let text = format!("{}\n{}", ran.out, ran.err).replace(token, "");
        for leak in ["127.0.0.1", port.as_str(), "very-distinctive", stringified.as_str()] {
            assert!(!text.contains(leak), "{name}: {leak:?} reached the process's output:\n{text}");
        }
        for line in ran.err.lines().filter(|l| l.contains("caller=")) {
            assert!(line.starts_with("DRYRUN-"), "{name}: a question counted as a call: {line}");
        }
    }

    // Parsed, never connected. All three processes have exited; a `connect`
    // any of them made would have completed against the backlog and be on the
    // channel now. The wait is bounded and sleeps in `recv_timeout`.
    match target.contacted.recv_timeout(GRACE) {
        Err(mpsc::RecvTimeoutError::Timeout) => {}
        Ok(peer) => panic!("a dry run dialed the target: connection from {peer}"),
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("the accept thread died"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The IOR may be given inline as well as by file, and a value that is
/// neither is refused before anything is read.
#[test]
fn the_reference_is_taken_inline_or_from_a_file_and_refused_otherwise() {
    let dir = scratch("inline");
    let target = target();
    let stringified = target.ior.to_stringified().expect("stringifies");
    let args = format!(r#"{{"e":{{"_ref":"expert"}},"updated_cap":{CAP}}}"#);

    let inline =
        heartbeat("inline", &args, &["--dry-run-handle", &format!("expert={stringified}")]);
    assert_eq!(inline.code, Some(0), "stderr:\n{}", inline.err);
    assert_eq!(field(&document(&inline), "payload"), "marshals", "{}", inline.out);

    let missing = dir.join("nobody-wrote-this.ior");
    let unread =
        heartbeat("unread", &args, &["--dry-run-handle", &format!("expert={}", missing.display())]);
    assert_eq!(unread.code, Some(2), "a missing file is a usage error: {}", unread.err);
    assert!(unread.err.contains("--dry-run-handle expert"), "{}", unread.err);
    assert!(unread.out.is_empty(), "nothing is predicted: {}", unread.out);

    let garbage = dir.join("garbage.ior");
    std::fs::write(&garbage, "IOR:zz\n").expect("writes");
    let unparsed = heartbeat(
        "unparsed",
        &args,
        &["--dry-run-handle", &format!("expert={}", garbage.display())],
    );
    assert_eq!(unparsed.code, Some(2), "{}", unparsed.err);
    assert!(unparsed.err.contains("--dry-run-handle expert"), "{}", unparsed.err);

    let shapeless = heartbeat("shapeless", &args, &["--dry-run-handle", "expert"]);
    assert_eq!(shapeless.code, Some(2), "{}", shapeless.err);
    assert!(shapeless.err.contains("<name>=<IOR:...|file>"), "{}", shapeless.err);

    let twice = heartbeat(
        "twice",
        &args,
        &[
            "--dry-run-handle",
            &format!("expert={stringified}"),
            "--dry-run-handle",
            &format!("expert={stringified}"),
        ],
    );
    assert_eq!(twice.code, Some(2), "{}", twice.err);
    assert!(twice.err.contains("declared twice"), "{}", twice.err);

    match target.contacted.recv_timeout(GRACE) {
        Err(mpsc::RecvTimeoutError::Timeout) => {}
        Ok(peer) => panic!("a dry run dialed the target: connection from {peer}"),
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("the accept thread died"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A reference nothing can name is refused, like values with no operation to
/// apply them to: a run that held one and printed a survey would read as a
/// report about it.
#[test]
fn a_handle_nothing_can_name_is_a_usage_error() {
    let target = target();
    let stringified = target.ior.to_stringified().expect("stringifies");
    let out = Command::new(env!("CARGO_BIN_EXE_orbweaver-mcp-server"))
        .args(["--idl", IDL])
        .args(["--expose", REGISTRY])
        .args(["--as", "alice"])
        .arg(format!("--dry-run={REGISTRY}.heartbeat"))
        .args(["--dry-run-handle", &format!("expert={stringified}")])
        .output()
        .expect("the server binary is built by `cargo test`");
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--dry-run-handle needs --dry-run-args"), "{err}");
    assert!(out.stdout.is_empty(), "nothing is predicted");
    assert!(
        matches!(target.contacted.recv_timeout(GRACE), Err(mpsc::RecvTimeoutError::Timeout)),
        "a refused run dialed the target"
    );
}
