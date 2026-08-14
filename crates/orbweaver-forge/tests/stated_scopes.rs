//! D005 option C, measured: the scope a requirement states, bound to the
//! `ai_authz` S3 emits.
//!
//! `docs/decisions/D005-contract-stability.md` was drafted from one measured
//! regeneration (`docs/pipeline-runs/2026-08-14-end-to-end.md`, Cause A) in
//! which S1–S3 re-ran over an unchanged requirement, passed every gate 1/1, and
//! produced a contract asking for `parkinglot.barrier.open` where the
//! requirement says `gate:operate`. This file is that finding as a test.
//!
//! Two provenances, kept apart on purpose:
//!
//! - **The first run's contract is committed** — `spikes/e2e/recorded/` holds
//!   its brief, its draft and its annotated form, with `pipeline.log` for
//!   provenance. The check must be **silent** on it, and that is measured
//!   against the real bytes.
//! - **The second run's contract is not.** D005 says so in its own limits
//!   section: it exists only as a six-row table transcribed into the run
//!   record, and re-running the producer produces a third contract rather than
//!   the second. So the drifted contract below is **reconstructed from that
//!   table** — module `ParkingLot`, interface `GateControl`, the three renamed
//!   operations, `string` floor labels, a sequence per call, and
//!   `parkinglot.barrier.open`. It reproduces the *class*, never the instance,
//!   and is labelled as such wherever it is used.
//!
//! The third fixture is the one D005 calls decisive and no run has ever
//! produced: the recorded contract with **every identifier kept** and only the
//! scope changed. It passes all eight hops of the end-to-end run today.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use orbweaver_forge::annotate::{check_against, check_against_brief};
use orbweaver_forge::ingest::{Brief, scope_tokens};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The brief the live run of 2026-08-14 produced, from the tree.
fn recorded_brief() -> Brief {
    Brief::parse(&read("spikes/e2e/recorded/PARKING.brief.json"))
        .expect("the recorded brief parses")
}

/// A "before" for the contract-identity check: the annotated file without its
/// structured comments, which is what S2 handed S3.
fn strip_annotations(idl: &str) -> String {
    idl.lines().filter(|l| !l.trim_start().starts_with("//@")).collect::<Vec<_>>().join("\n")
}

/// The reconstructed second contract. **Not** the second run's bytes — see the
/// module docs and D005's limits section.
const DRIFTED: &str = r#"module ParkingLot {

  //@ ai_desc: How a floor is labelled, as printed on the signage.
  typedef string FloorLabel;

  //@ ai_desc: Free capacity on one floor.
  struct FloorAvailability {
    //@ ai_desc: The floor this row describes.
    FloorLabel label;
    //@ ai_desc: Free spaces on that floor.
    unsigned long remaining_spaces;
  };

  //@ ai_desc: Availability for every floor the facility has.
  typedef sequence<FloorAvailability> FloorAvailabilitySeq;

  //@ ai_desc: The state of an entry barrier.
  enum BarrierState { BARRIER_CLOSED, BARRIER_MOVING, BARRIER_OPEN };

  //@ ai_desc: Raised when the barrier is already open.
  exception BarrierAlreadyOpen {
    //@ ai_desc: The barrier's state when the request was refused.
    BarrierState state;
  };

  //@ ai_desc: Read-and-control interface for one parking lot.
  interface GateControl {
    //@ ai_desc: Returns free capacity for every floor in one call.
    //@ ai_effect: read_only
    FloorAvailabilitySeq get_floor_availability();

    //@ ai_desc: Returns the state of the named barrier.
    //@ ai_effect: read_only
    BarrierState get_barrier_state(in string target_barrier);

    //@ ai_desc: Opens the entry barrier for the identified vehicle.
    //@ ai_effect: destructive
    //@ ai_authz: parkinglot.barrier.open
    //@ ai_pii: high
    void open_entry_barrier(in wstring plate_number)
      raises (BarrierAlreadyOpen);
  };
};
"#;

/// The measured first run, from the tree: the check must say nothing.
///
/// A rule that fires on correct output is a rule people route around, so this
/// is the load-bearing half of the measurement and not a formality.
#[test]
fn the_recorded_contract_binds_its_scope_and_the_check_is_silent() {
    let brief = recorded_brief();
    assert_eq!(
        brief.stated_scopes(),
        BTreeMap::from([("open_entry_gate".to_owned(), "gate:operate".to_owned())]),
        "S1 recorded the token and the requirement states it verbatim"
    );

    let draft = read("spikes/e2e/recorded/PARKING.idl");
    let annotated = read("spikes/e2e/recorded/PARKING.sidl.idl");
    let report = check_against_brief(Some(&brief), &draft, &annotated);
    assert!(report.is_ok(), "{:?}", report.findings);
    assert!(
        !report.findings.iter().any(|f| f.rule == "s3/authz-not-the-stated-scope"),
        "{:?}",
        report.findings
    );

    // D005's compensating obligation, as far as a crate can carry it: the ten
    // questions S1 asked and nobody answered are in front of whoever reads the
    // last stage report before registration. Advice, so it blocks nothing.
    let unanswered: Vec<&orbweaver_forge::Finding> =
        report.findings.iter().filter(|f| f.rule == "s3/unanswered-question").collect();
    assert_eq!(unanswered.len(), brief.open_questions.len(), "every one of them travels");
    assert!(!unanswered.is_empty(), "this brief has open questions and they were never answered");
    assert!(unanswered.iter().all(|f| f.severity == orbweaver_forge::Severity::Advice));
}

/// The drifted contract, reconstructed from the run record's table.
#[test]
fn the_drifted_contract_is_refused_by_the_binding_alone() {
    let brief = recorded_brief();
    let before = strip_annotations(DRIFTED);

    // Everything else in the project accepts it — that is Cause A's whole
    // point, and it is asserted rather than asserted-about.
    assert!(orbweaver_forge::validate(DRIFTED).is_ok(), "valid IDL");
    assert!(check_against(&before, DRIFTED).is_ok(), "{:?}", check_against(&before, DRIFTED));

    let report = check_against_brief(Some(&brief), &before, DRIFTED);
    let f = report
        .findings
        .iter()
        .find(|f| f.rule == "s3/authz-not-the-stated-scope")
        .expect("the binding refuses it");
    assert!(f.message.contains("gate:operate"), "{}", f.message);
    assert!(f.message.contains("parkinglot.barrier.open"), "{}", f.message);
    assert!(f.message.contains("refuses every caller"), "{}", f.message);
}

/// The case D005 calls decisive: every identifier kept, only `ai_authz`
/// changed. This regeneration passes all eight hops of the end-to-end run and
/// every gate in the project — the incidental name drift is the only reason the
/// measured one was caught at all.
#[test]
fn a_regeneration_that_changes_only_the_scope_is_caught_only_by_the_binding() {
    let brief = recorded_brief();
    let recorded = read("spikes/e2e/recorded/PARKING.sidl.idl");
    let drifted = recorded.replace("//@ ai_authz: gate:operate", "//@ ai_authz: parking.gate.open");
    assert_ne!(drifted, recorded, "the substitution applied");
    let before = read("spikes/e2e/recorded/PARKING.idl");

    // Every check this project had before this change passes it.
    assert!(orbweaver_forge::validate(&drifted).is_ok());
    assert!(check_against(&before, &drifted).is_ok());
    assert!(check_against_brief(None, &before, &drifted).is_ok(), "and so does S3 with no brief");

    let report = check_against_brief(Some(&brief), &before, &drifted);
    let f = report
        .findings
        .iter()
        .find(|f| f.rule == "s3/authz-not-the-stated-scope")
        .expect("the binding refuses it");
    assert!(f.message.contains("gate:operate"), "{}", f.message);
}

/// The false-positive measurement, over the frozen requirement benchmark.
///
/// The rule can only fire on a requirement that *states* a scope-shaped token,
/// so scanning the twenty inputs is an upper bound on how often it can fire at
/// all — whatever S1 records, and whatever S2 and S3 then write. Printed as a
/// count so the number in the run record is reproducible from this test.
#[test]
fn the_binding_cannot_fire_on_the_requirements_corpus() {
    let dir = root().join("corpus/requirements/inputs");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    assert_eq!(files.len(), 20, "the frozen benchmark is twenty requirements");

    let mut with_tokens: Vec<String> = Vec::new();
    for path in &files {
        let text = std::fs::read_to_string(path).expect("readable");
        let tokens = scope_tokens(&text);
        if !tokens.is_empty() {
            with_tokens.push(format!(
                "{}: {}",
                path.file_name().unwrap().to_string_lossy(),
                tokens.join(", ")
            ));
        }
    }
    println!(
        "corpus/requirements/inputs: {}/{} requirement(s) contain a scope-shaped token",
        with_tokens.len(),
        files.len()
    );
    assert!(
        with_tokens.is_empty(),
        "the rule would have something to demand on {with_tokens:?} — check each one before \
         accepting the number"
    );

    // And the one requirement written to state a scope does state one, so the
    // zero above is a property of these twenty and not of the scanner.
    assert_eq!(scope_tokens(&read("spikes/e2e/requirement.txt")), ["gate:operate"]);
}

/// The plumbing, end to end with a scripted producer: S3's second input reaches
/// the prompt, and S3's gate reaches the same token.
///
/// No model anywhere. The producer is a shell script that copies the prompt it
/// was handed to a file the test reads, then prints a fixed answer — which is
/// how "the model was told" and "the model was graded" are measured separately.
#[cfg(unix)]
#[test]
fn the_command_stage_hands_s3_the_scopes_and_grades_it_on_them() {
    use std::os::unix::fs::PermissionsExt;

    use orbweaver_forge::pipeline::{CommandStage, ItemStatus, StageId, Workspace, run_batch};

    let dir = std::env::temp_dir().join(format!("forge-stated-scopes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    let workspace = Workspace::new(&dir);

    let brief = recorded_brief();
    workspace.store(StageId::Ingest, "PARKING", &brief.to_text()).expect("brief stored");
    let draft = read("spikes/e2e/recorded/PARKING.idl");
    let annotated = read("spikes/e2e/recorded/PARKING.sidl.idl");

    let run = |name: &str, answer: &str| {
        let answer_file = dir.join(format!("{name}.answer"));
        std::fs::write(&answer_file, answer).expect("answer");
        let seen = dir.join(format!("{name}.seen-prompt"));
        let script = dir.join(format!("{name}.sh"));
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\ncp \"$FORGE_PROMPT\" {seen}\ncat {answer_file}\n",
                seen = seen.display(),
                answer_file = answer_file.display()
            ),
        )
        .expect("script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        let mut stage = CommandStage::new(StageId::Annotate, script.to_string_lossy(), &dir)
            .with_briefs(&workspace);
        let report = run_batch(&mut stage, &[("PARKING".to_owned(), draft.clone())], 1);
        (std::fs::read_to_string(&seen).expect("the producer saw a prompt"), report)
    };

    let (prompt, report) = run("kept", &annotated);
    assert!(
        prompt.contains("SCOPES THE REQUIREMENT STATES"),
        "the producer was handed the block:\n{prompt}"
    );
    assert!(prompt.contains("open_entry_gate: gate:operate"), "{prompt}");
    assert!(report.all_valid(), "{report}");

    let drifted =
        annotated.replace("//@ ai_authz: gate:operate", "//@ ai_authz: parking.gate.open");
    let (_, report) = run("drifted", &drifted);
    assert!(!report.all_valid(), "{report}");
    assert_eq!(
        report.affected(0, "s3/authz-not-the-stated-scope"),
        ["PARKING".to_owned()],
        "the cause is named and carries its affected item: {report}"
    );
    let ItemStatus::Invalid { repair_prompt } = &report.items[0].status else { panic!("{report}") };
    assert!(
        repair_prompt.contains("gate:operate"),
        "the repair round carries the token itself:\n{repair_prompt}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
