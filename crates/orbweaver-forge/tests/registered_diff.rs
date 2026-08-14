//! D005 option B, measured: a regeneration diffed against what is registered.
//!
//! `docs/decisions/D005-contract-stability.md` is APPROVED and its second
//! adopted item is option B — **`orbweaver_forge::validate_against`, which
//! already wraps §5.3's differ and which the pipeline simply never calls, wired
//! into the S4 gate against the registered contract.** This file is that change
//! and its measurement.
//!
//! Two things are being measured here and they pull in opposite directions:
//!
//! - **What B catches.** A regeneration that renames the contract breaks every
//!   deployed reference to it, and the gate now refuses it unless the
//!   regeneration is declared. `a_full_regeneration_fires_on_every_repository_id`
//!   measures that on the recorded parking bytes and prints the count, because
//!   the count *is* D005's warning about routine approval.
//! - **What B cannot catch.** The differ never reads annotations, so a
//!   regeneration that keeps every identifier and changes only `//@ ai_authz`
//!   is *compatible* by §5.3 and passes this gate.
//!   `option_b_is_blind_to_the_scope_that_option_c_binds` asserts both halves of
//!   that sentence against the same bytes — B silent, C refusing — which is why
//!   C landed first and why B does not subsume it. The one near-miss is
//!   `a_scope_that_is_also_an_idl_constant_warns_and_still_lands`: a contract
//!   that also declares its scope as an IDL constant has that value compared,
//!   and §5.3 calls it conditionally breaking, which is a warning and lands.
//!
//! **B가 잡는 것과 못 잡는 것을 같은 파일에서 같은 바이트로 측정한다.** 스코프
//! 표류는 §5.3 기준으로 호환이며 B에게는 보이지 않는다.

use std::path::{Path, PathBuf};

use orbweaver_forge::pipeline::{
    BatchReport, ItemStatus, Pipeline, Registered, SUPERSEDED_FILE, StageId, ValidateStage,
    Workspace, record_supersede, run_batch, run_pipeline,
};
use orbweaver_forge::{Severity, validate};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// A fresh directory to play a registry of record in.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("forge-registered-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn register_contract(dir: &Path, name: &str, idl: &str) {
    std::fs::write(dir.join(name), idl).expect("registered contract");
}

/// S4 alone over one item, which is the whole of B's surface.
fn gate_one(gate: &mut ValidateStage, id: &str, idl: &str) -> BatchReport {
    run_batch(gate, &[(id.to_owned(), idl.to_owned())], 1)
}

const RELEASED: &str = "module m {
  //@ ai_desc: One account.
  struct Account { long id; };
  //@ ai_desc: What an agent may do with an account.
  interface Ledger {
    //@ ai_desc: Returns the account.
    //@ ai_effect: read_only
    Account fetch(in long id);
  };
};
";

/// The same contract with the interface renamed — which is what a regeneration
/// does, and what the differ reads as a removal.
const RENAMED: &str = "module m {
  //@ ai_desc: One account.
  struct Account { long id; };
  //@ ai_desc: What an agent may do with an account.
  interface AccountLedger {
    //@ ai_desc: Returns the account.
    //@ ai_effect: read_only
    Account fetch(in long id);
  };
};
";

/// Harm 1 of D005, at the gate: a regeneration that renames the contract makes
/// every deployed reference unresolvable, and an undeclared one is refused.
#[test]
fn an_undeclared_breaking_regeneration_is_refused() {
    let dir = scratch("breaking");
    register_contract(&dir, "R01.idl", RELEASED);

    let mut gate = ValidateStage::against(Registered::at(&dir));
    let report = gate_one(&mut gate, "R01", RENAMED);

    assert!(!report.all_valid(), "{report}");
    assert_eq!(
        report.affected(0, "evolution/BREAKING"),
        ["R01".to_owned()],
        "the cause is named and carries its affected item: {report}"
    );
    let ItemStatus::Invalid { repair_prompt } = &report.items[0].status else { panic!("{report}") };
    assert!(repair_prompt.contains("new version"), "{repair_prompt}");
    assert!(repair_prompt.contains("m/Ledger"), "the refusal names the id: {repair_prompt}");

    let outcome = gate.outcomes().pop().expect("one item was gated");
    assert!(outcome.compared, "the comparison ran");
    assert_eq!(outcome.against, Some(dir.join("R01.idl")), "and it names what it ran against");
    assert!(!outcome.blocking.is_empty());
    assert_eq!(outcome.superseded, None, "nothing declared it");

    // The same file with no baseline passes, which is the whole delta this
    // change makes and the reason the before/after is asserted rather than
    // described.
    let mut blind = ValidateStage::new();
    assert!(gate_one(&mut blind, "R01", RENAMED).all_valid(), "S4 before option B");

    let _ = std::fs::remove_dir_all(&dir);
}

/// An additive regeneration is not a refusal. A gate that fires on every change
/// is a gate people route around, and §5.3 says adding an operation is a
/// rollout-ordering problem rather than a break.
#[test]
fn an_additive_regeneration_is_allowed_and_says_so_without_blocking() {
    let dir = scratch("additive");
    register_contract(&dir, "R02.idl", RELEASED);
    let widened = RELEASED.replace(
        "    Account fetch(in long id);",
        "    Account fetch(in long id);\n\
         \x20   //@ ai_desc: How many accounts there are.\n\
         \x20   //@ ai_effect: read_only\n\
         \x20   long count();",
    );

    let mut gate = ValidateStage::against(Registered::at(&dir));
    let report = gate_one(&mut gate, "R02", &widened);
    assert!(report.all_valid(), "{report}");
    assert!(report.causes[0].is_empty(), "no cause fired: {report}");

    let outcome = gate.outcomes().pop().expect("gated");
    assert!(outcome.compared, "it was compared, and cleanly");
    assert!(outcome.blocking.is_empty());

    // The advice is still produced — "compatible" and "unexamined" must not
    // look alike from the outside.
    let advisory = orbweaver_forge::validate_against(&widened, RELEASED);
    assert!(advisory.findings.iter().any(|f| f.rule == "evolution/server-first"), "{advisory:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The judgement D005 leaves open and this change had to make: a first
/// generation has nothing to diff against, and **must not be refused for it.**
///
/// Silence, not a pass: the outcome records that nothing was compared, which is
/// what keeps "checked and clean" apart from "never checked".
#[test]
fn a_first_generation_has_no_baseline_and_is_not_refused_for_it() {
    let dir = scratch("first-generation");
    register_contract(&dir, "R03.idl", RELEASED);

    let mut gate = ValidateStage::against(Registered::at(&dir));
    // R99 is not registered: nothing in the directory answers to that id.
    let report = gate_one(&mut gate, "R99", RENAMED);
    assert!(report.all_valid(), "a first generation is not refused: {report}");

    let outcome = gate.outcomes().pop().expect("gated");
    assert_eq!(outcome.against, None, "nothing was registered under this id");
    assert!(!outcome.compared, "and the record says the comparison did not run");
    assert!(outcome.blocking.is_empty());

    // And the verdict is exactly the baseline-free gate's, findings and all —
    // no baseline means no change in behaviour, not a weaker check.
    let mut blind = ValidateStage::new();
    let plain = gate_one(&mut blind, "R99", RENAMED);
    assert_eq!(plain.items[0].status, report.items[0].status);
    assert_eq!(validate(RENAMED).findings, orbweaver_forge::validate(RENAMED).findings);

    let _ = std::fs::remove_dir_all(&dir);
}

/// A registered contract that cannot be read is a **failure**, never the
/// silence a missing one earns. The harness rule — an unmeasured check is a
/// failure, never a pass — applied to the gate itself.
#[test]
fn a_registered_contract_that_cannot_be_read_is_a_failure() {
    let dir = scratch("unreadable");
    // A directory where a contract should be: `read_to_string` fails, the file
    // exists, and the two must not look alike.
    std::fs::create_dir_all(dir.join("R04.idl")).expect("a directory in its place");

    let mut gate = ValidateStage::against(Registered::at(&dir));
    let report = gate_one(&mut gate, "R04", RELEASED);
    assert!(!report.all_valid(), "{report}");
    assert_eq!(report.affected(0, "evolution/registered-unreadable"), ["R04".to_owned()]);

    let outcome = gate.outcomes().pop().expect("gated");
    assert!(outcome.against.is_some(), "something was registered");
    assert!(!outcome.compared, "and it was never compared");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The annotated file is the contract when there is one, exactly as
/// [`Workspace::gated_artifact`] resolves it — so a previous run's `--out`
/// directory is a usable registry of record with no second layout.
#[test]
fn the_registered_contract_is_the_annotated_file_when_one_exists() {
    let dir = scratch("resolution");
    register_contract(&dir, "R05.idl", RELEASED);
    register_contract(&dir, "R05.sidl.idl", RENAMED);

    let mut gate = ValidateStage::against(Registered::at(&dir));
    // Proposing the *draft's* contract against a registry holding the annotated
    // one must diff against the annotated one, so this is a rename either way.
    let report = gate_one(&mut gate, "R05", RELEASED);
    assert!(!report.all_valid(), "{report}");
    let outcome = gate.outcomes().pop().expect("gated");
    assert_eq!(outcome.against, Some(dir.join("R05.sidl.idl")));

    let _ = std::fs::remove_dir_all(&dir);
}

/// **What option B cannot see**, on the recorded bytes rather than on a claim.
///
/// D005: *"A regeneration that keeps every identifier and changes only the
/// scope produces zero changes from the differ."* Both halves are asserted
/// here — B silent, C refusing — because the sentence is the whole reason the
/// approved order put C first.
#[test]
fn option_b_is_blind_to_the_scope_that_option_c_binds() {
    use orbweaver_forge::annotate::check_against_brief;
    use orbweaver_forge::ingest::Brief;

    let dir = scratch("scope-drift");
    let recorded = read("spikes/e2e/recorded/PARKING.sidl.idl");
    register_contract(&dir, "PARKING.sidl.idl", &recorded);

    let drifted = recorded.replace("//@ ai_authz: gate:operate", "//@ ai_authz: parking.gate.open");
    assert_ne!(drifted, recorded, "the substitution applied");

    let mut gate = ValidateStage::against(Registered::at(&dir));
    let report = gate_one(&mut gate, "PARKING", &drifted);
    assert!(report.all_valid(), "option B passes a scope drift: {report}");
    let outcome = gate.outcomes().pop().expect("gated");
    assert!(outcome.compared, "it really was compared — this is blindness, not a skip");
    assert!(outcome.blocking.is_empty(), "and §5.3 found nothing: {:?}", outcome.blocking);

    // The same bytes, refused by the rule that reads annotations. B and C see
    // disjoint halves of one regeneration.
    let brief = Brief::parse(&read("spikes/e2e/recorded/PARKING.brief.json")).expect("the brief");
    let draft = read("spikes/e2e/recorded/PARKING.idl");
    let c = check_against_brief(Some(&brief), &draft, &drifted);
    assert!(
        c.findings.iter().any(|f| f.rule == "s3/authz-not-the-stated-scope"),
        "option C refuses what option B passed: {:?}",
        c.findings
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The one case where B sees a scope at all — and it still does not refuse it.
///
/// The recorded parking contract happens to state its scope **twice**: as
/// `//@ ai_authz: gate:operate`, which the differ never reads, and as
/// `const string GATE_OPERATE_PERMISSION = "gate:operate"`, which it does. A
/// regeneration that moves both produces a `constant value changed` change —
/// and §5.3 calls that *conditionally breaking*, which this crate maps to a
/// **warning**, so the item lands anyway.
///
/// So the honest statement of B's reach is narrower than "it warns about
/// scopes": it sees a scope only when the contract's author chose to model it
/// as an IDL constant, which is a property of that contract's style and not of
/// the gate, and even then it does not refuse. Nothing here should be read as
/// option B covering any part of option C's job.
///
/// **B가 스코프를 보는 유일한 경우조차 거부가 아니라 경고다.** 그것도 계약이
/// 스코프를 IDL 상수로 선언했을 때에 한한다.
#[test]
fn a_scope_that_is_also_an_idl_constant_warns_and_still_lands() {
    let dir = scratch("scope-constant");
    let recorded = read("spikes/e2e/recorded/PARKING.sidl.idl");
    register_contract(&dir, "PARKING.sidl.idl", &recorded);
    assert!(
        recorded.contains("const string GATE_OPERATE_PERMISSION = \"gate:operate\""),
        "this measurement is about that constant; if it is gone, re-derive the claim"
    );

    let both_moved = recorded.replace("gate:operate", "parking.gate.open");
    let mut gate = ValidateStage::against(Registered::at(&dir));
    let report = gate_one(&mut gate, "PARKING", &both_moved);
    assert!(report.all_valid(), "a scope change still lands: {report}");
    let outcome = gate.outcomes().pop().expect("gated");
    assert!(outcome.blocking.is_empty(), "nothing refused it: {:?}", outcome.blocking);

    let findings = orbweaver_forge::validate_against(&both_moved, &recorded);
    let f = findings
        .findings
        .iter()
        .find(|f| f.rule == "evolution/conditionally-breaking")
        .expect("the constant's value moved and §5.3 says so");
    assert_eq!(f.severity, Severity::Warning, "and a warning does not refuse");
    assert!(f.message.contains("gate:operate"), "{}", f.message);

    let _ = std::fs::remove_dir_all(&dir);
}

/// D005's warning, measured rather than repeated: a full regeneration renames
/// everything, so the gate fires on **every** id at once, and an approval that
/// covers a list this long is the approval that stops being a signal.
#[test]
fn a_full_regeneration_fires_on_every_repository_id() {
    let dir = scratch("every-id");
    let recorded = read("spikes/e2e/recorded/PARKING.sidl.idl");
    register_contract(&dir, "PARKING.sidl.idl", &recorded);

    // The measured second run renamed the module and the interface; every id
    // under them moves with them.
    let regenerated =
        recorded.replace("ParkingFacility", "ParkingLot").replace("ParkingControl", "GateControl");
    assert_ne!(regenerated, recorded);

    let mut gate = ValidateStage::against(Registered::at(&dir));
    let report = gate_one(&mut gate, "PARKING", &regenerated);
    assert!(!report.all_valid(), "{report}");

    let outcome = gate.outcomes().pop().expect("gated");
    println!(
        "a module+interface rename produced {} breaking change(s) in one item:\n  {}",
        outcome.blocking.len(),
        outcome.blocking.join("\n  ")
    );
    assert!(
        outcome.blocking.len() >= 2,
        "one rename is many refusals, which is D005's routine-approval hazard: {:?}",
        outcome.blocking
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Option D's frame, as far as a gate can carry it: a declared regeneration
/// lands, and what it waved through is written down.
#[test]
fn a_declared_regeneration_lands_and_records_every_change_it_covered() {
    let dir = scratch("supersede");
    let out = scratch("supersede-out");
    register_contract(&dir, "R06.idl", RELEASED);

    let reason = "R06 regenerated after the requirement changed; no deployed consumer yet";
    let mut gate = ValidateStage::against(Registered::at(&dir)).superseding(reason);
    let report = gate_one(&mut gate, "R06", RENAMED);
    assert!(report.all_valid(), "a declared regeneration lands: {report}");

    let outcomes = gate.outcomes();
    let outcome = outcomes.last().expect("gated");
    assert_eq!(outcome.superseded.as_deref(), Some(reason));
    assert!(!outcome.blocking.is_empty(), "what it covered is still recorded");

    let path = record_supersede(&outcomes, &out).expect("written").expect("something to write");
    let text = std::fs::read_to_string(&path).expect("readable");
    assert!(path.ends_with(SUPERSEDED_FILE), "{}", path.display());
    assert!(text.contains(reason), "{text}");
    assert!(text.contains("m/Ledger"), "the change itself, not just a count:\n{text}");
    let rows = text.lines().filter(|l| !l.starts_with('#')).count();
    assert_eq!(rows, outcome.blocking.len(), "one row per change:\n{text}");

    // A clean run writes nothing: a file that exists whether or not anything
    // was waved through is a file nobody reads.
    let mut clean = ValidateStage::against(Registered::at(&dir));
    let _ = gate_one(&mut clean, "R06", RELEASED);
    assert_eq!(record_supersede(&clean.outcomes(), &out).expect("no error"), None);

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&out);
}

/// An empty reason is not a declaration. `--supersede ""` must not be a way to
/// turn the gate off without saying anything.
#[test]
fn an_empty_supersede_reason_declares_nothing() {
    let dir = scratch("empty-reason");
    register_contract(&dir, "R07.idl", RELEASED);

    let mut gate = ValidateStage::against(Registered::at(&dir)).superseding("   ");
    let report = gate_one(&mut gate, "R07", RENAMED);
    assert!(!report.all_valid(), "still refused: {report}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The wiring, through `run_pipeline` rather than through the stage alone: a
/// refused regeneration is *dropped*, so it never reaches S5 and never
/// registers. That is B's actual claim — it does not make regeneration stable,
/// it makes an unstable regeneration refuse to land.
#[test]
fn the_pipeline_drops_a_refused_regeneration_before_registration() {
    let dir = scratch("pipeline-registry");
    let workspace_dir = scratch("pipeline-ws");
    register_contract(&dir, "R08.idl", RELEASED);
    let workspace = Workspace::new(&workspace_dir);

    let mut pipeline = Pipeline {
        ingest: None,
        synthesize: None,
        annotate: None,
        validate: ValidateStage::against(Registered::at(&dir)),
        first: StageId::Validate,
        last: StageId::Validate,
        max_rounds: 1,
    };
    let report = run_pipeline(&mut pipeline, &workspace, &[("R08".to_owned(), RENAMED.to_owned())])
        .expect("S4 alone runs");

    assert!(!report.all_valid(), "{report}");
    assert_eq!(report.dropped, vec![("R08".to_owned(), StageId::Validate)]);
    let outcome = pipeline.validate.outcomes().pop().expect("the ledger outlives the run");
    assert_eq!(outcome.id, "R08");
    assert!(outcome.compared);

    // And the finding a human reads is an error, not advice.
    let findings = orbweaver_forge::validate_against(RENAMED, RELEASED);
    assert!(
        findings
            .findings
            .iter()
            .any(|f| f.severity == Severity::Error && f.rule.starts_with("evolution/")),
        "{findings:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&workspace_dir);
}

/// The CLI seam, with no model anywhere: `--only s4 --registered <dir>` refuses
/// the regeneration and says what it compared, and `--supersede` lands it and
/// leaves the record behind.
#[test]
fn the_cli_compares_against_a_registered_directory_and_records_a_supersede() {
    use std::process::Command;

    let registry = scratch("cli-registry");
    let out = scratch("cli-out");
    register_contract(&registry, "R09.idl", RELEASED);
    // S4's input is whatever the workspace holds for the id.
    register_contract(&out, "R09.sidl.idl", RENAMED);

    let run = |extra: &[&str]| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_forge-pipeline"));
        cmd.args(["--only", "s4", "--out"]);
        cmd.arg(&out);
        cmd.arg("--registered");
        cmd.arg(&registry);
        cmd.args(extra);
        cmd.output().expect("runs")
    };

    let refused = run(&[]);
    let text = String::from_utf8_lossy(&refused.stdout).into_owned();
    assert_eq!(refused.status.code(), Some(1), "an undeclared breaking change is refused:\n{text}");
    assert!(text.contains("evolution/BREAKING"), "{text}");
    assert!(text.contains("compared 1 item(s) against"), "it says what it compared:\n{text}");
    assert!(text.contains("annotations are not compared"), "and what it did not:\n{text}");

    let reason = "R09 regenerated on purpose; no consumer is deployed";
    let declared = run(&["--supersede", reason]);
    let text = String::from_utf8_lossy(&declared.stdout).into_owned();
    assert_eq!(declared.status.code(), Some(0), "a declared regeneration lands:\n{text}");
    let record =
        std::fs::read_to_string(out.join(SUPERSEDED_FILE)).expect("the record was written");
    assert!(record.contains(reason), "{record}");

    // A declaration with nothing to declare against is a flag that does
    // nothing, which is worse than an error.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_forge-pipeline"));
    cmd.args(["--only", "s4", "--out"]);
    cmd.arg(&out);
    cmd.args(["--supersede", reason]);
    let lonely = cmd.output().expect("runs");
    assert_eq!(lonely.status.code(), Some(2), "{}", String::from_utf8_lossy(&lonely.stderr));

    let _ = std::fs::remove_dir_all(&registry);
    let _ = std::fs::remove_dir_all(&out);
}
