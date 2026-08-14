//! Stage isolation: each of S1, S2, S3 and S4 runnable and measurable alone,
//! and a failure attributable to the stage that caused it.
//!
//! This is the property that justifies splitting the pipeline at all. A
//! pipeline that only runs end to end can tell you the output is wrong; these
//! tests assert that this one tells you **which stage** was wrong, that a later
//! stage can be re-run over artifacts an earlier one left, and that a human
//! edit to an intermediate artifact reaches the stage that consumes it.
//!
//! Every producer here is a scripted fake. No model is called and none is
//! needed: what is measured is the machinery, and the machinery's numbers are
//! the only numbers these tests are entitled to.

use std::collections::HashMap;
use std::path::PathBuf;

use orbweaver_forge::ingest::{Brief, Effect, Entity, Field, OperationSketch};
use orbweaver_forge::pipeline::{
    ItemStatus, Pipeline, Stage, StageId, ValidateStage, Workspace, gate_for, run_batch,
    run_pipeline,
};
use orbweaver_forge::{Report, validate};

fn tmp(name: &str) -> PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("forge-stages-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// The brief a well-behaved S1 produces for the one requirement used here.
fn ledger_brief() -> Brief {
    Brief {
        requirement: "은행 계좌 간 이체. 잔액 부족을 예외로 알린다. 금액 단위는 원.".into(),
        summary: "Transfers between bank accounts".into(),
        entities: vec![Entity {
            name: "Account".into(),
            description: "one bank account".into(),
            fields: vec![Field {
                name: "number".into(),
                shape: "account number".into(),
                ..Field::default()
            }],
        }],
        operations: vec![OperationSketch {
            name: "transfer".into(),
            entity: Some("Account".into()),
            inputs: vec![Field {
                name: "amount".into(),
                shape: "an amount of money".into(),
                unit: Some("KRW".into()),
                pii: None,
            }],
            outputs: vec![],
            effect: Effect::Destructive,
            errors: vec!["InsufficientFunds".into()],
            authz: Some("ledger.transfer".into()),
        }],
        constraints: vec!["금액 단위는 원".into()],
        open_questions: vec![],
    }
}

const DRAFT: &str = "module bank {
     struct Account { string number; };
     interface Ledger {
       void transfer(in string source, in string target, in long long amount);
     };
   };";

const ANNOTATED: &str = "module bank {
     struct Account { string number; };
     interface Ledger {
       //@ ai_desc: Moves an amount from one account to another
       //@ ai_effect: destructive
       //@ ai_authz: ledger.transfer
       void transfer(in string source, in string target,
         //@ ai_unit: KRW
         in long long amount);
     };
   };";

/// The same file with the scope dropped — the corpus's dominant finding.
const UNSCOPED: &str = "module bank {
     struct Account { string number; };
     interface Ledger {
       //@ ai_desc: Moves an amount from one account to another
       //@ ai_effect: destructive
       void transfer(in string source, in string target, in long long amount);
     };
   };";

/// A stage that replays a script keyed by round, so a test can say exactly
/// what each round produces and then read the loop's own numbers back.
struct Replay {
    stage: StageId,
    /// Output per call, in order. The last entry repeats if the loop asks for
    /// more.
    outputs: Vec<String>,
    calls: Vec<(String, Option<String>)>,
}

impl Replay {
    fn new(stage: StageId, outputs: &[&str]) -> Replay {
        Replay {
            stage,
            outputs: outputs.iter().map(|s| (*s).to_owned()).collect(),
            calls: Vec::new(),
        }
    }
}

impl Stage for Replay {
    fn id(&self) -> StageId {
        self.stage
    }

    fn produce(&mut self, input: &str, repair: Option<&str>) -> Result<String, String> {
        let i = self.calls.len().min(self.outputs.len().saturating_sub(1));
        self.calls.push((input.to_owned(), repair.map(str::to_owned)));
        Ok(self.outputs[i].clone())
    }

    fn gate(&self, input: &str, output: &str) -> Report {
        gate_for(self.stage, input, output)
    }
}

/// A stage that maps its input to an output, so a full-pipeline test can drive
/// several items without a script per round.
struct Mapped {
    stage: StageId,
    by_input: HashMap<String, String>,
    fallback: String,
    inputs_seen: Vec<String>,
}

impl Stage for Mapped {
    fn id(&self) -> StageId {
        self.stage
    }

    fn produce(&mut self, input: &str, _repair: Option<&str>) -> Result<String, String> {
        self.inputs_seen.push(input.to_owned());
        Ok(self.by_input.get(input).cloned().unwrap_or_else(|| self.fallback.clone()))
    }

    fn gate(&self, input: &str, output: &str) -> Report {
        gate_for(self.stage, input, output)
    }
}

fn mapped(stage: StageId, fallback: &str) -> Mapped {
    Mapped {
        stage,
        by_input: HashMap::new(),
        fallback: fallback.to_owned(),
        inputs_seen: Vec::new(),
    }
}

// ── S1 alone ────────────────────────────────────────────────────────────────

/// S1 is a stage: it runs over a set on its own and its report is about S1.
#[test]
fn s1_runs_and_measures_alone() {
    let good = ledger_brief().to_json().to_string();
    let mut stage = Replay::new(StageId::Ingest, &[good.as_str()]);
    let report = run_batch(&mut stage, &[("R02".into(), "은행 계좌 간 이체".into())], 3);

    assert_eq!(report.stage, StageId::Ingest);
    assert_eq!(report.first_pass_valid, 1);
    assert_eq!(report.rounds_used, 1);
    assert!(report.to_string().contains("S1 ingest: 1 item(s)"), "{report}");
}

/// An S1 that answers in prose fails in S1's own numbers, with a repair prompt
/// that says what shape was expected — and the failure is nowhere near S2.
#[test]
fn s1_failures_are_s1s_own_and_carry_a_repair_prompt() {
    let good = ledger_brief().to_json().to_string();
    let mut stage =
        Replay::new(StageId::Ingest, &["Certainly! Here is a summary of the requirement.", &good]);
    let report = run_batch(&mut stage, &[("R02".into(), "은행 계좌 간 이체".into())], 3);

    assert_eq!(report.first_pass_valid, 0, "round 1 was prose");
    assert_eq!(report.rounds_used, 2, "one repair round fixed it");
    assert_eq!(report.affected(0, "s1/not-a-brief"), ["R02"]);
    assert!(report.all_valid(), "{report}");
    let repair = stage.calls[1].1.as_deref().expect("a repair prompt on round 2");
    assert!(repair.contains("s1/not-a-brief"), "{repair}");
    assert!(repair.contains("no prose"), "{repair}");
}

// ── S3 alone, and why it is a stage ─────────────────────────────────────────

/// The claim in the module docs, as a measurement: the corpus's dominant
/// finding — a mutating operation with no scope — is invisible to S4 and is an
/// error in S3's own gate.
#[test]
fn s3_measures_the_failure_s4_is_entitled_to_ignore() {
    assert!(validate(UNSCOPED).is_ok(), "unannotated-but-valid IDL is valid CORBA");

    let mut stage = Replay::new(StageId::Annotate, &[UNSCOPED, ANNOTATED]);
    let report = run_batch(&mut stage, &[("R02".into(), DRAFT.into())], 3);

    assert_eq!(report.stage, StageId::Annotate);
    assert_eq!(report.first_pass_valid, 0, "S3's own first-pass rate, not S2's");
    assert_eq!(report.affected(0, "s3/missing-ai_authz"), ["R02"]);
    assert_eq!(report.rounds_used, 2);
    assert!(report.all_valid(), "{report}");

    let repair = stage.calls[1].1.as_deref().expect("a repair prompt");
    assert!(repair.contains("s3/missing-ai_authz"), "{repair}");
    assert!(repair.contains("any caller who reaches the bridge"), "{repair}");
}

/// S3 adds comments. A stage that rewrites the contract while it is in there
/// is caught by S3's gate, not discovered in a diff review.
#[test]
fn s3_may_not_change_the_contract_it_annotates() {
    let rewritten = "module bank {
         struct Account { string number; };
         interface Ledger {
           //@ ai_desc: Moves an amount
           //@ ai_effect: destructive
           //@ ai_authz: ledger.transfer
           long transfer(in string source, in string target, in long long amount);
         };
       };";
    let mut stage = Replay::new(StageId::Annotate, &[rewritten]);
    let report = run_batch(&mut stage, &[("R02".into(), DRAFT.into())], 1);
    assert!(!report.all_valid(), "the return type changed");
    assert_eq!(report.affected(0, "s3/contract-changed"), ["R02"]);
}

// ── attribution across the whole pipeline ───────────────────────────────────

/// The reason for the split, stated as a test. S2 is perfect and S3 forgets
/// scopes; the numbers say so, item for item, stage for stage.
#[test]
fn a_failure_is_attributed_to_the_stage_that_caused_it() {
    let dir = tmp("attribution");
    let workspace = Workspace::new(&dir);
    let mut ingest = mapped(StageId::Ingest, &ledger_brief().to_json().to_string());
    let mut synthesize = mapped(StageId::Synthesize, DRAFT);
    let mut annotate = mapped(StageId::Annotate, UNSCOPED);
    let mut pipeline = Pipeline {
        ingest: Some(&mut ingest),
        synthesize: Some(&mut synthesize),
        annotate: Some(&mut annotate),
        validate: ValidateStage::default(),
        first: StageId::Ingest,
        last: StageId::Validate,
        max_rounds: 1,
    };
    let report = run_pipeline(&mut pipeline, &workspace, &[("R02".into(), "이체".into())])
        .expect("the run starts");

    assert!(report.stage(StageId::Ingest).expect("S1 ran").all_valid());
    assert!(report.stage(StageId::Synthesize).expect("S2 ran").all_valid());
    let s3 = report.stage(StageId::Annotate).expect("S3 ran");
    assert!(!s3.all_valid(), "S3 is where it failed");
    assert_eq!(s3.affected(0, "s3/missing-ai_authz"), ["R02"]);
    // S4 never saw the item, and the report says so rather than leaving a
    // suspiciously clean silence.
    assert!(report.stage(StageId::Validate).is_none());
    assert_eq!(report.dropped, vec![("R02".to_owned(), StageId::Annotate)]);
    assert!(report.to_string().contains("rejected at S3 annotate"), "{report}");
    assert!(!report.all_valid());
}

/// Every stage's artifact lands under its own name, and S4 gates the annotated
/// one when there is one.
#[test]
fn each_stage_leaves_its_own_artifact_and_s4_gates_the_last() {
    let dir = tmp("artifacts");
    let workspace = Workspace::new(&dir);
    let mut ingest = mapped(StageId::Ingest, &ledger_brief().to_json().to_string());
    let mut synthesize = mapped(StageId::Synthesize, DRAFT);
    let mut annotate = mapped(StageId::Annotate, ANNOTATED);
    let mut pipeline = Pipeline {
        ingest: Some(&mut ingest),
        synthesize: Some(&mut synthesize),
        annotate: Some(&mut annotate),
        validate: ValidateStage::default(),
        first: StageId::Ingest,
        last: StageId::Validate,
        max_rounds: 1,
    };
    let report = run_pipeline(&mut pipeline, &workspace, &[("R02".into(), "이체".into())])
        .expect("the run starts");
    assert!(report.all_valid(), "{report}");

    assert!(dir.join("R02.brief.json").exists());
    assert!(dir.join("R02.idl").exists());
    assert!(dir.join("R02.sidl.idl").exists());
    assert!(workspace.gated_artifact("R02").unwrap().ends_with("R02.sidl.idl"));

    // The brief on disk is the indented form a human can edit, and it parses.
    let text = std::fs::read_to_string(dir.join("R02.brief.json")).unwrap();
    assert!(text.contains("\n  \"summary\""), "the artifact is readable:\n{text}");
    assert_eq!(Brief::parse(&text).expect("round-trips"), ledger_brief());
}

/// A human corrects S1's reading and re-runs from S2 — the point of having an
/// inspectable intermediate at all. The correction must reach S2.
#[test]
fn an_edited_brief_reaches_s2_on_a_rerun_that_skips_s1() {
    let dir = tmp("rerun");
    let workspace = Workspace::new(&dir);

    // First run: S1 only.
    let mut ingest = mapped(StageId::Ingest, &ledger_brief().to_json().to_string());
    let mut first = Pipeline {
        ingest: Some(&mut ingest),
        synthesize: None,
        annotate: None,
        validate: ValidateStage::default(),
        first: StageId::Ingest,
        last: StageId::Ingest,
        max_rounds: 1,
    };
    let report = run_pipeline(&mut first, &workspace, &[("R02".into(), "이체".into())])
        .expect("S1 alone runs");
    assert!(report.all_valid(), "{report}");

    // A human disagrees with the reading and edits the artifact.
    let mut corrected = ledger_brief();
    corrected.open_questions.push("이체 한도가 명시되지 않음".into());
    corrected.operations[0].authz = Some("ledger.transfer.high_value".into());
    std::fs::write(dir.join("R02.brief.json"), corrected.to_text()).unwrap();

    // Second run: from S2, with no requirements directory in sight.
    let mut synthesize = mapped(StageId::Synthesize, DRAFT);
    let items: Vec<(String, String)> = workspace
        .ids_ready_for(StageId::Synthesize)
        .into_iter()
        .map(|id| {
            let text = workspace.load(StageId::Synthesize, &id).expect("the brief is there");
            (id, text)
        })
        .collect();
    assert_eq!(items.len(), 1, "the workspace found the item without being told");
    let mut second = Pipeline {
        ingest: None,
        synthesize: Some(&mut synthesize),
        annotate: None,
        validate: ValidateStage::default(),
        first: StageId::Synthesize,
        last: StageId::Validate,
        max_rounds: 1,
    };
    let report = run_pipeline(&mut second, &workspace, &items).expect("S2 alone runs");
    assert!(report.all_valid(), "{report}");
    assert!(report.stage(StageId::Ingest).is_none(), "S1 did not run again");

    // The edit reached the producer, in the brief's rendered form.
    let seen = &synthesize.inputs_seen[0];
    assert!(seen.contains("ledger.transfer.high_value"), "the correction reached S2:\n{seen}");
    assert!(seen.contains("이체 한도"), "{seen}");
}

/// `--only s4` in library form: re-gate a directory of files with no producer
/// at all. The smallest useful run, and the one that proves the gate does not
/// need the stages in front of it.
#[test]
fn s4_re_gates_existing_files_with_no_producers() {
    let dir = tmp("regate");
    let workspace = Workspace::new(&dir);
    workspace.store(StageId::Annotate, "good", ANNOTATED).expect("writes");
    workspace
        .store(
            StageId::Annotate,
            "bad",
            "module m { struct Position { double x; };
             interface T { void go(in Position position); }; };",
        )
        .expect("writes");

    let ids = workspace.ids_ready_for(StageId::Validate);
    assert_eq!(ids, vec!["bad".to_owned(), "good".to_owned()]);
    let items: Vec<(String, String)> = ids
        .into_iter()
        .map(|id| {
            let text = workspace.load(StageId::Validate, &id).expect("readable");
            (id, text)
        })
        .collect();

    let mut pipeline = Pipeline::gate_only();
    let report = run_pipeline(&mut pipeline, &workspace, &items).expect("runs");
    let s4 = report.stage(StageId::Validate).expect("S4 ran");
    assert_eq!(s4.items.len(), 2);
    assert_eq!(s4.first_pass_valid, 1);
    assert_eq!(s4.affected(0, "identifier-case-clash"), ["bad"]);
    // S4 rewrites nothing, so its loop is one round however many are allowed.
    assert_eq!(s4.rounds_used, 1);
}

/// Running without an annotation pass is the pre-split pipeline and a
/// legitimate thing to ask for. It must never be silent: "S4 passed" means
/// something different about a file no annotation stage has seen.
#[test]
fn a_stage_with_no_producer_is_skipped_out_loud() {
    let dir = tmp("skipped");
    let workspace = Workspace::new(&dir);
    let mut synthesize = mapped(StageId::Synthesize, DRAFT);
    let mut pipeline = Pipeline {
        ingest: None,
        synthesize: Some(&mut synthesize),
        annotate: None,
        validate: ValidateStage::default(),
        first: StageId::Synthesize,
        last: StageId::Validate,
        max_rounds: 1,
    };
    let report = run_pipeline(&mut pipeline, &workspace, &[("R02".into(), "이체".into())])
        .expect("the run starts");

    assert!(report.all_valid(), "{report}");
    assert_eq!(report.skipped, vec![StageId::Annotate]);
    assert!(report.to_string().contains("S3 annotate: SKIPPED"), "{report}");
    // S4 gated the unannotated draft, because that is all there was.
    assert!(workspace.gated_artifact("R02").unwrap().ends_with("R02.idl"));
    assert!(!dir.join("R02.sidl.idl").exists());
}

/// A range with nothing in it is an error, not a clean empty run: an unmeasured
/// check is a failure, never a pass.
#[test]
fn a_range_that_can_run_nothing_is_refused() {
    let dir = tmp("nothing");
    let workspace = Workspace::new(&dir);
    let mut pipeline = Pipeline {
        ingest: None,
        synthesize: None,
        annotate: None,
        validate: ValidateStage::default(),
        first: StageId::Annotate,
        last: StageId::Annotate,
        max_rounds: 1,
    };
    let err = run_pipeline(&mut pipeline, &workspace, &[("R02".into(), DRAFT.into())])
        .expect_err("refused");
    assert!(err.to_string().contains("nothing to run"), "{err}");
}

/// The stage report a rerun produces is the same shape as the one a full run
/// produces, because it is the same code path with the artifacts already there.
#[test]
fn resuming_and_running_through_produce_the_same_artifacts() {
    let through = tmp("through");
    let resumed = tmp("resumed");

    let run = |dir: &PathBuf, first: StageId, items: &[(String, String)]| {
        let workspace = Workspace::new(dir);
        let mut ingest = mapped(StageId::Ingest, &ledger_brief().to_json().to_string());
        let mut synthesize = mapped(StageId::Synthesize, DRAFT);
        let mut annotate = mapped(StageId::Annotate, ANNOTATED);
        let mut pipeline = Pipeline {
            ingest: Some(&mut ingest),
            synthesize: Some(&mut synthesize),
            annotate: Some(&mut annotate),
            validate: ValidateStage::default(),
            first,
            last: StageId::Validate,
            max_rounds: 1,
        };
        let report = run_pipeline(&mut pipeline, &workspace, items).expect("runs");
        assert!(report.all_valid(), "{report}");
    };

    run(&through, StageId::Ingest, &[("R02".into(), "이체".into())]);

    // The resumed run starts from an S1 artifact copied in, exactly as a human
    // handing over a corrected brief would leave it.
    std::fs::copy(through.join("R02.brief.json"), resumed.join("R02.brief.json")).unwrap();
    let workspace = Workspace::new(&resumed);
    let brief = workspace.load(StageId::Synthesize, "R02").expect("the brief");
    run(&resumed, StageId::Synthesize, &[("R02".to_owned(), brief)]);

    for name in ["R02.idl", "R02.sidl.idl"] {
        assert_eq!(
            std::fs::read_to_string(through.join(name)).unwrap(),
            std::fs::read_to_string(resumed.join(name)).unwrap(),
            "{name} differs between running through and resuming"
        );
    }
}

/// The prompts are artifacts of this crate, printable, so a wrapper script
/// cannot drift from the checker that grades it.
#[cfg(unix)]
#[test]
fn the_cli_prints_each_stages_prompt() {
    use std::process::Command;
    for (tag, must_contain) in [
        ("s1", "open_questions"),
        ("s2", "Do NOT write //@ ai_*"),
        ("s3", "MUST carry //@ ai_authz"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_forge-pipeline"))
            .args(["--print-prompt", tag])
            .output()
            .expect("runs");
        assert!(out.status.success(), "{tag}");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains(must_contain), "{tag} prompt:\n{text}");
    }
    // S4 has no prompt because it is a check, and asking for one says so.
    let out = Command::new(env!("CARGO_BIN_EXE_forge-pipeline"))
        .args(["--print-prompt", "s4"])
        .output()
        .expect("runs");
    assert_eq!(out.status.code(), Some(2));
}

/// The CLI end to end over all three producing stages, with the uniform
/// command contract: `<cmd> <input> [<repair>]`, `FORGE_STAGE` in the
/// environment. One wrapper serves every stage.
#[cfg(unix)]
#[test]
fn the_cli_runs_all_three_stages_through_one_wrapper() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let dir = tmp("cli-three");
    let req_dir = dir.join("requirements");
    let out_dir = dir.join("out");
    std::fs::create_dir_all(&req_dir).unwrap();
    std::fs::write(req_dir.join("R02.txt"), "은행 계좌 간 이체").unwrap();

    // One script, three stages, told apart by FORGE_STAGE — which is exactly
    // what a model wrapper does, minus the model.
    let script = dir.join("stage.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n\
             test -f \"$FORGE_PROMPT\" || exit 3\n\
             case \"$FORGE_STAGE\" in\n\
             \x20 s1) cat <<'EOF'\n{brief}\nEOF\n ;;\n\
             \x20 s2) cat <<'EOF'\n{draft}\nEOF\n ;;\n\
             \x20 s3) cat <<'EOF'\n{annotated}\nEOF\n ;;\n\
             \x20 *) exit 4 ;;\n\
             esac\n",
            brief = ledger_brief().to_json(),
            draft = DRAFT,
            annotated = ANNOTATED
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_forge-pipeline"))
        .args(["--requirements".as_ref(), req_dir.as_os_str()])
        .args(["--ingest".as_ref(), script.as_os_str()])
        .args(["--synthesize".as_ref(), script.as_os_str()])
        .args(["--annotate".as_ref(), script.as_os_str()])
        .args(["--out".as_ref(), out_dir.as_os_str()])
        .args(["--max-rounds", "2", "--register"])
        .output()
        .expect("forge-pipeline runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}\n{}", String::from_utf8_lossy(&output.stderr));

    for stage in ["S1 ingest: 1 item(s)", "S2 synthesize: 1 item(s)", "S3 annotate: 1 item(s)"] {
        assert!(stdout.contains(stage), "{stdout}");
    }
    assert!(stdout.contains("S4 gated 1 annotated file(s) and 0 unannotated draft(s)"), "{stdout}");
    assert!(stdout.contains("S5: registered 1 item(s)"), "{stdout}");
    assert!(out_dir.join("R02.brief.json").exists());
    assert!(out_dir.join("R02.sidl.idl").exists());

    // And re-running one stage alone needs nothing but the workspace.
    let again = Command::new(env!("CARGO_BIN_EXE_forge-pipeline"))
        .args(["--annotate".as_ref(), script.as_os_str()])
        .args(["--out".as_ref(), out_dir.as_os_str()])
        .args(["--only", "s3"])
        .output()
        .expect("forge-pipeline runs");
    let stdout = String::from_utf8_lossy(&again.stdout);
    assert!(again.status.success(), "{stdout}");
    assert!(stdout.contains("range: S3 annotate → S3 annotate"), "{stdout}");
    assert!(stdout.contains("S3 annotate: 1 item(s)"), "{stdout}");
    assert!(!stdout.contains("S2 synthesize"), "S2 did not run: {stdout}");
}

/// An item the loop could not fix leaves its artifact on disk anyway: a failed
/// batch you can inspect beats one you cannot.
#[test]
fn a_failed_items_artifact_is_still_written() {
    let dir = tmp("failed-artifact");
    let workspace = Workspace::new(&dir);
    let mut annotate = mapped(StageId::Annotate, UNSCOPED);
    let mut pipeline = Pipeline {
        ingest: None,
        synthesize: None,
        annotate: Some(&mut annotate),
        validate: ValidateStage::default(),
        first: StageId::Annotate,
        last: StageId::Validate,
        max_rounds: 1,
    };
    let report =
        run_pipeline(&mut pipeline, &workspace, &[("R02".into(), DRAFT.into())]).expect("runs");
    assert!(!report.all_valid());
    let written = std::fs::read_to_string(dir.join("R02.sidl.idl")).expect("written anyway");
    assert!(written.contains("ai_effect: destructive"));
    let s3 = report.stage(StageId::Annotate).expect("S3 ran");
    assert!(matches!(s3.items[0].status, ItemStatus::Invalid { .. }));
}
