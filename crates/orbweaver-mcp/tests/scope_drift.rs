//! D005's scope drift, caught by the **process** an operator runs.
//!
//! The library half is `orbweaver_mcp::token`'s own tests: a [`ScopeMap`] that
//! cannot produce a scope an exposed operation requires is reported as
//! unsatisfiable, with the operations it takes down named. This file asserts the
//! other half — that an operator actually meets the finding, in the place they
//! already look and through a channel a script cannot ignore.
//!
//! That distinction is the same one `serving_audit.rs` was written for: the
//! library kept the ledger and the deployment threw it away, and every library
//! test still passed. A finding nobody's exit code reads is a finding in the
//! same position.
//!
//! # The scenario is D005's, verbatim
//!
//! > A deployment whose identity provider issues the scope the requirement
//! > literally states — `gate:operate` — against a contract that demands
//! > `parkinglot.barrier.open` **refuses every legitimate caller**. The refusal
//! > is well-formed, correctly audited, and indistinguishable from a permissions
//! > misconfiguration.
//!
//! # Harness discipline
//!
//! Per `CLAUDE.md`: no `--ior` and no socket (a dry run dials nothing), the
//! child is driven to exit rather than polled, and `output()` reads both pipes
//! to EOF, so there is no wait to get wrong here at all.

use std::process::Command;

use orbweaver_dynamic::json::Json;

/// The contract, with the scope the generator drifted to.
const IDL: &str = "module parkinglot {
  interface ParkingControl {
    //@ ai_authz: parkinglot.barrier.open
    void open_barrier();
    //@ ai_authz: parkinglot.barrier.open
    void close_barrier();
    long vehicle_count();
  };
};
";

const CONTROL: &str = "IDL:parkinglot/ParkingControl:1.0";

struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
}

/// Runs the real binary in `--dry-run` against `IDL`, with whatever scope
/// arguments the case needs.
fn dry_run(name: &str, extra: &[&str]) -> Ran {
    let dir = std::env::temp_dir().join(format!("orbweaver-scope-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let idl_path = dir.join("parkinglot.idl");
    std::fs::write(&idl_path, IDL).expect("writes the contract");

    let out = Command::new(env!("CARGO_BIN_EXE_orbweaver-mcp-server"))
        .args(["--idl", idl_path.to_str().expect("utf-8")])
        .args(["--expose", CONTROL])
        .args(["--as", "alice"])
        .arg("--dry-run")
        .args(extra)
        .output()
        .expect("the server binary is built by `cargo test`");

    Ran {
        code: out.status.code(),
        out: String::from_utf8_lossy(&out.stdout).into_owned(),
        err: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// The finding, in all three channels an operator might read: the document, the
/// diagnostics, and the exit code.
#[test]
fn a_scope_no_token_can_satisfy_is_a_finding_a_pipeline_cannot_miss() {
    let ran = dry_run(
        "drift",
        &["--map-scope", "gate:operate=gate:operate", "--token-scope", "gate:operate"],
    );

    assert_eq!(ran.code, Some(3), "a finding must not exit zero\nstderr:\n{}", ran.err);

    // stdout is still exactly one JSON object — a second document beside it
    // would break every pipeline that parses this.
    let doc = Json::parse(ran.out.trim()).unwrap_or_else(|e| panic!("{e}\n{}", ran.out));
    let scope_map = doc.get("scope_map").expect("the section is present when a map is configured");
    assert_eq!(scope_map.get("ok"), Some(&Json::Bool(false)), "{scope_map}");

    let Some(Json::Array(unsatisfiable)) = scope_map.get("unsatisfiable") else {
        panic!("{scope_map}")
    };
    assert_eq!(unsatisfiable.len(), 1, "{scope_map}");
    assert_eq!(
        unsatisfiable[0].get("scope").and_then(Json::as_str),
        Some("parkinglot.barrier.open")
    );
    // The blast radius, not merely the name: two operations go dark.
    let Some(Json::Array(wanted_by)) = unsatisfiable[0].get("wanted_by") else {
        panic!("{scope_map}")
    };
    let mut ops: Vec<&str> =
        wanted_by.iter().filter_map(|w| w.get("operation").and_then(Json::as_str)).collect();
    ops.sort_unstable();
    assert_eq!(ops, ["close_barrier", "open_barrier"]);

    // And the sentence a human reads, which has to name the symptom they will
    // otherwise chase: the identity team, checking a correct IdP.
    assert!(ran.err.contains("parkinglot.barrier.open"), "{}", ran.err);
    assert!(ran.err.contains("permissions misconfiguration"), "{}", ran.err);

    // The survey itself still says what it always said, so the section is an
    // addition rather than a replacement.
    assert_eq!(
        doc.get("summary").and_then(|s| s.get("need_scope")),
        Some(&Json::Number("2".into())),
        "{doc}"
    );
}

/// The map repaired: the same contract, the same IdP, one line of translation.
/// `ok`, exit zero, and the two operations the drift took down come back.
#[test]
fn the_mapping_is_what_repairs_it_without_touching_the_contract() {
    let ran = dry_run(
        "mapped",
        &["--map-scope", "gate:operate=parkinglot.barrier.open", "--token-scope", "gate:operate"],
    );

    assert_eq!(ran.code, Some(0), "stderr:\n{}", ran.err);
    let doc = Json::parse(ran.out.trim()).unwrap_or_else(|e| panic!("{e}\n{}", ran.out));
    let scope_map = doc.get("scope_map").expect("present");
    assert_eq!(scope_map.get("ok"), Some(&Json::Bool(true)), "{scope_map}");
    assert_eq!(
        doc.get("summary").and_then(|s| s.get("allow")),
        Some(&Json::Number("3".into())),
        "the two barrier operations and the count are all reachable: {doc}"
    );
    // The caller's scopes in the report are the *contract's* vocabulary, which
    // is the only vocabulary the gate speaks.
    assert_eq!(
        doc.get("scopes"),
        Some(&Json::Array(vec![Json::String("parkinglot.barrier.open".into())])),
        "{doc}"
    );
}

/// A token scope the map places nowhere grants nothing and is named — ignored is
/// not silent — while the deployment as a whole stays healthy, so this is a note
/// and not an outage.
#[test]
fn a_token_scope_placed_nowhere_is_named_and_is_not_a_finding() {
    let ran = dry_run(
        "unmapped",
        &[
            "--map-scope",
            "gate:operate=parkinglot.barrier.open",
            "--token-scope",
            "gate:operate",
            "--token-scope",
            "billing:read",
        ],
    );

    assert_eq!(ran.code, Some(0), "an unplaced scope is a note, not a blocker\n{}", ran.err);
    assert!(ran.err.contains("billing:read"), "{}", ran.err);
    let doc = Json::parse(ran.out.trim()).unwrap_or_else(|e| panic!("{e}\n{}", ran.out));
    let scope_map = doc.get("scope_map").expect("present");
    assert_eq!(
        scope_map.get("unmapped_token_scopes"),
        Some(&Json::Array(vec![Json::String("billing:read".into())])),
        "{scope_map}"
    );
    assert_eq!(scope_map.get("issued_declared"), Some(&Json::Bool(true)));
}

/// The regression guard the whole surface is conditional on: with no
/// `--map-scope`, the document is exactly what it has always been. A deployment
/// that does not use the feature must not be able to tell it exists.
#[test]
fn without_a_mapping_the_document_is_unchanged() {
    let ran = dry_run("none", &["--scope", "parkinglot.barrier.open"]);
    assert_eq!(ran.code, Some(0), "{}", ran.err);
    let doc = Json::parse(ran.out.trim()).unwrap_or_else(|e| panic!("{e}\n{}", ran.out));
    assert!(doc.get("scope_map").is_none(), "{doc}");
    assert_eq!(
        doc.get("summary").and_then(|s| s.get("allow")),
        Some(&Json::Number("3".into())),
        "--scope still names contract scopes directly: {doc}"
    );
}

/// A malformed mapping is a usage error rather than a silently-empty map: a
/// `--map-scope` that placed nothing would look exactly like a healthy
/// deployment, which is the failure mode this whole file is about.
#[test]
fn a_malformed_mapping_is_refused_rather_than_ignored() {
    let ran = dry_run("malformed", &["--map-scope", "gate:operate"]);
    assert_eq!(ran.code, Some(2), "{}", ran.err);
    assert!(ran.err.contains("--map-scope"), "{}", ran.err);
    assert!(ran.out.trim().is_empty(), "nothing on the protocol stream: {}", ran.out);
}
