//! §7.4 I2, proven: pipeline output is exposed safely.
//!
//! S5 registration feeds the catalog with exposure **off** by default. These
//! tests drive a real batch through `run_batch` with a scripted generator
//! (the same deterministic-fake pattern as `tests/pipeline.rs`), register it,
//! and then stand where an agent stands: a `Bridge` over the resulting
//! registry with `Exposure::nothing()` must show *nothing* — search empty,
//! describe refused — and one `allow_interface` must expose exactly one
//! interface while its neighbour stays invisible. That is the I2 sentence
//! made executable: a generated interface becoming agent-visible requires
//! the same explicit allowlist as a hand-written one.

use std::collections::HashMap;
use std::path::PathBuf;

use orbweaver_dynamic::json::Json;
use orbweaver_forge::pipeline::{
    BatchReport, EXPOSURE_TODO_FILE, Generator, ItemReport, ItemStatus, RegisterError, register,
    run_batch,
};
use orbweaver_mcp::Bridge;
use orbweaver_mcp::policy::Exposure;

/// Two generated interfaces: one to allowlist, one to leave as the invisible
/// neighbour.
const THERMOSTAT: &str = "module gen { \
     //@ ai_desc: Reads a room temperature\n \
     interface Thermostat { \
     //@ ai_desc: the current reading\n \
     //@ ai_effect: read_only\n \
     double reading(); }; };";
const LEDGER: &str = "module gen { \
     //@ ai_desc: Aggregate ledger over all accounts\n \
     interface Ledger { \
     //@ ai_effect: read_only\n \
     long long total(); }; };";
const THERMOSTAT_ID: &str = "IDL:gen/Thermostat:1.0";
const LEDGER_ID: &str = "IDL:gen/Ledger:1.0";

/// The project's dominant failure, for the gate test: valid-looking data
/// carrying IDL that S4 rejects.
const CLASH: &str = "module m { struct Position { double x; }; struct T { Position position; }; };";

/// A deterministic fake in the `tests/pipeline.rs` pattern: a script instead
/// of a model, here a fixed requirement → IDL map.
struct Mapped {
    by_requirement: HashMap<String, String>,
}

impl Generator for Mapped {
    fn generate(&mut self, requirement: &str, _repair: Option<&str>) -> Result<String, String> {
        self.by_requirement
            .get(requirement)
            .cloned()
            .ok_or_else(|| format!("unscripted requirement {requirement:?}"))
    }
}

fn generated_batch() -> BatchReport {
    let reqs = vec![
        ("ledger".to_owned(), "an aggregate ledger".to_owned()),
        ("thermostat".to_owned(), "a room thermostat".to_owned()),
    ];
    let mut generator = Mapped {
        by_requirement: HashMap::from([
            ("an aggregate ledger".to_owned(), LEDGER.to_owned()),
            ("a room thermostat".to_owned(), THERMOSTAT.to_owned()),
        ]),
    };
    let report = run_batch(&mut generator, &reqs, 3);
    assert!(report.all_valid(), "{report}");
    report
}

fn out_dir(name: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("register-{name}-{}", std::process::id()))
}

fn exposed_ids(bridge: &Bridge<'_>, query: &str) -> Vec<String> {
    let hits = bridge.search(query, 10);
    let Some(Json::Array(list)) = hits.get("interfaces") else { panic!("{hits}") };
    list.iter().map(|h| h.get("id").and_then(Json::as_str).expect("an id").to_owned()).collect()
}

/// The I2 proof. Registration alone exposes nothing; one explicit allowlist
/// entry exposes exactly one interface.
#[test]
fn a_registered_batch_is_invisible_until_a_human_allowlists_it() {
    let report = generated_batch();
    let registration = register(&report, &out_dir("invisible")).expect("registers");
    assert_eq!(
        registration.exposable,
        vec![LEDGER_ID.to_owned(), THERMOSTAT_ID.to_owned()],
        "both generated interfaces are on the menu"
    );

    // Exposure::nothing(): THE GENERATED INTERFACE IS INVISIBLE. Search
    // returns nothing — not for the empty query, not for its own name — and
    // describe refuses.
    let dark = Bridge::new(&registration.registry, Exposure::nothing(), "session-a");
    for query in ["", "thermostat", "ledger", "temperature"] {
        assert_eq!(exposed_ids(&dark, query), Vec::<String>::new(), "query {query:?}");
    }
    let hits = dark.search("", 10).to_string();
    assert!(!hits.contains("Thermostat") && !hits.contains("Ledger"), "{hits}");
    assert!(dark.describe(THERMOSTAT_ID).is_err());
    assert!(dark.describe(LEDGER_ID).is_err());

    // One explicit allowlist entry — the same step a hand-written interface
    // requires — and exactly that interface becomes visible.
    let allowed = Exposure::nothing().allow_interface(THERMOSTAT_ID);
    let lit = Bridge::new(&registration.registry, allowed, "session-b");
    assert_eq!(exposed_ids(&lit, ""), vec![THERMOSTAT_ID.to_owned()]);
    assert!(lit.describe(THERMOSTAT_ID).is_ok());

    // The neighbour from the same batch stays invisible.
    assert!(lit.describe(LEDGER_ID).is_err());
    assert!(!lit.search("ledger", 10).to_string().contains("Ledger"));
}

/// The worksheet: every exposable id present, every one exposed=no, and the
/// header says why the default is no.
#[test]
fn the_worksheet_lists_every_exposable_interface_as_unexposed() {
    let report = generated_batch();
    let dir = out_dir("worksheet");
    let registration = register(&report, &dir).expect("registers");

    let text = std::fs::read_to_string(dir.join(EXPOSURE_TODO_FILE)).expect("worksheet written");
    let rows: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(rows.len(), registration.exposable.len(), "one row per exposable id:\n{text}");
    for id in &registration.exposable {
        let row = rows
            .iter()
            .find(|r| r.starts_with(&format!("{id}\t")))
            .unwrap_or_else(|| panic!("{id} missing from the worksheet:\n{text}"));
        assert_eq!(row.split('\t').nth(1), Some("exposed=no"), "{row}");
    }
    // The header carries the reason, not just the rule.
    assert!(text.contains("7.4 I2"), "{text}");
    assert!(text.contains("a projection that exposes by default exposes"), "{text}");
}

/// Registration is a gate, not a formality: a report is plain data anyone can
/// construct, and a `Valid` marking over IDL that S4 rejects is refused —
/// before any worksheet exists to mislead anyone.
#[test]
fn a_report_that_lies_about_validity_is_refused_at_the_gate() {
    let forged = BatchReport {
        items: vec![ItemReport {
            id: "forged".into(),
            status: ItemStatus::Valid,
            rounds: 1,
            idl: Some(CLASH.into()),
        }],
        first_pass_valid: 1,
        rounds_used: 1,
        max_rounds: 1,
        causes: vec![],
    };
    let dir = out_dir("forged");
    let err = register(&forged, &dir).expect_err("the gate refuses");
    let RegisterError::Rejected { id, repair_prompt } = &err else { panic!("{err}") };
    assert_eq!(id, "forged");
    assert!(repair_prompt.contains("identifier-case-clash"), "{repair_prompt}");
    assert!(!dir.join(EXPOSURE_TODO_FILE).exists(), "no worksheet for a refused registration");
}

/// A `Valid` item with no IDL at all is the same lie in a different shape.
#[test]
fn a_valid_marking_without_idl_is_refused() {
    let forged = BatchReport {
        items: vec![ItemReport {
            id: "empty".into(),
            status: ItemStatus::Valid,
            rounds: 1,
            idl: None,
        }],
        first_pass_valid: 1,
        rounds_used: 1,
        max_rounds: 1,
        causes: vec![],
    };
    let err = register(&forged, &out_dir("empty")).expect_err("refused");
    assert_eq!(err, RegisterError::MissingIdl { id: "empty".into() });
}

/// Items the loop could not fix never reach the catalog: S5 registers the
/// valid subset and the invalid item contributes nothing exposable.
#[test]
fn only_valid_items_reach_the_catalog() {
    let report = BatchReport {
        items: vec![
            ItemReport {
                id: "good".into(),
                status: ItemStatus::Valid,
                rounds: 1,
                idl: Some(THERMOSTAT.into()),
            },
            ItemReport {
                id: "bad".into(),
                status: ItemStatus::Invalid { repair_prompt: "still broken".into() },
                rounds: 3,
                idl: Some(CLASH.into()),
            },
        ],
        first_pass_valid: 1,
        rounds_used: 3,
        max_rounds: 3,
        causes: vec![],
    };
    let registration = register(&report, &out_dir("subset")).expect("the valid subset registers");
    assert_eq!(registration.exposable, vec![THERMOSTAT_ID.to_owned()]);
    assert!(
        registration.registry.get("IDL:m/Position:1.0").is_none(),
        "nothing of the invalid item"
    );
}

/// The CLI end to end: `--register` after a green batch prints the exposable
/// count and the worksheet path, and the worksheet is all exposed=no.
#[cfg(unix)]
#[test]
fn the_cli_registers_with_exposure_off_by_default() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let dir = out_dir("cli");
    let req_dir = dir.join("requirements");
    let out = dir.join("out");
    std::fs::create_dir_all(&req_dir).unwrap();
    std::fs::write(req_dir.join("probe.txt"), "a liveness probe").unwrap();

    let script = dir.join("generate.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\necho 'module cli { interface Probe { void ping(); }; };'\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_forge-pipeline"))
        .args(["--requirements".as_ref(), req_dir.as_os_str()])
        .args(["--generator".as_ref(), script.as_os_str()])
        .args(["--out".as_ref(), out.as_os_str()])
        .args(["--max-rounds", "2", "--register"])
        .output()
        .expect("forge-pipeline runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "exit codes unchanged: 0 for a green batch\n{stdout}");
    assert!(stdout.contains("1 exposable interface(s)"), "{stdout}");
    let worksheet = out.join(EXPOSURE_TODO_FILE);
    assert!(stdout.contains(&worksheet.display().to_string()), "{stdout}");

    let text = std::fs::read_to_string(&worksheet).unwrap();
    assert!(text.contains("IDL:cli/Probe:1.0\texposed=no"), "{text}");
}
