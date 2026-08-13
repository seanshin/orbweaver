//! The §5.1 loop, proven without a model.
//!
//! Every test drives `run_batch` with a scripted stage, because what is under
//! test is the loop's mechanics: that round 1 finishes producing before
//! anything is gated, that the first-pass rate is measured before any repair,
//! that failures are handed back grouped by cause, and that exhausting the
//! rounds is reported plainly. A real model would add noise to every one of
//! those assertions and certainty to none.
//!
//! The scripted failure is the project's dominant one — the case-insensitive
//! identifier clash Phase 0 measured — so the repair prompt these tests assert
//! on is the exact text a real generator will see most often.

use std::collections::HashSet;

use orbweaver_forge::Report;
use orbweaver_forge::pipeline::{ItemStatus, Stage, StageId, gate_for, run_batch};

/// The dominant failure: the parameter `position` clashes with `Position`
/// ignoring case.
const CLASH: &str = "module m { struct Position { double x; };
     interface Tracker { void go(in Position position); }; };";
/// What a generator that read the repair prompt would produce.
const FIXED: &str = "module m { struct Position { double x; };
     interface Tracker { void go(in Position the_position); }; };";
/// Valid on the first try.
const CLEAN: &str = "module m { struct Point { long x; long y; };
     interface Locator { Point origin(); }; };";

/// A stage with a script instead of a model. It fixes the clash if and only if
/// the repair prompt names the cause — which is precisely the §3.3 claim about
/// diagnostics, restated as a test double.
#[derive(Default)]
struct Scripted {
    /// Inputs that get the clash on round 1.
    clash_on_first: HashSet<String>,
    /// Inputs that emit the clash no matter what they are told.
    stubborn: HashSet<String>,
    /// Inputs whose production fails outright (an API outage, scripted).
    broken: HashSet<String>,
    /// Every call, in order: (input, repair prompt handed in).
    calls: Vec<(String, Option<String>)>,
}

impl Stage for Scripted {
    fn id(&self) -> StageId {
        StageId::Synthesize
    }

    fn produce(&mut self, input: &str, repair: Option<&str>) -> Result<String, String> {
        self.calls.push((input.to_owned(), repair.map(str::to_owned)));
        if self.broken.contains(input) {
            return Err("model API unavailable".into());
        }
        if self.stubborn.contains(input) {
            return Ok(CLASH.into());
        }
        match repair {
            Some(r) if r.contains("identifier-case-clash") => Ok(FIXED.into()),
            Some(_) => Ok(CLASH.into()),
            None if self.clash_on_first.contains(input) => Ok(CLASH.into()),
            None => Ok(CLEAN.into()),
        }
    }

    fn gate(&self, input: &str, output: &str) -> Report {
        gate_for(StageId::Synthesize, input, output)
    }
}

fn requirements(texts: &[&str]) -> Vec<(String, String)> {
    texts.iter().enumerate().map(|(i, t)| (format!("r{:02}", i + 1), (*t).to_owned())).collect()
}

/// The whole loop on the measured Phase 0 shape: some items clash, one round
/// of cause-grouped feedback fixes all of them.
#[test]
fn a_batch_with_the_dominant_failure_converges_in_two_rounds() {
    let reqs = requirements(&["a point", "a position record", "a track record"]);
    let mut stage = Scripted {
        clash_on_first: HashSet::from([
            "a position record".to_owned(),
            "a track record".to_owned(),
        ]),
        ..Scripted::default()
    };
    let report = run_batch(&mut stage, &reqs, 5);

    // The report says which stage it is about — the whole point of the split.
    assert_eq!(report.stage, StageId::Synthesize);
    // First-pass rate reflects round 1 exactly, measured before any repair.
    assert_eq!(report.first_pass_valid, 1);
    assert!((report.first_pass_rate() - 1.0 / 3.0).abs() < 1e-9);
    // The loop converged in one repair round, and stopped there.
    assert_eq!(report.rounds_used, 2);
    assert!(report.all_valid(), "{report}");
    // Rounds are per item: the clean one never saw round 2.
    assert_eq!(report.items[0].rounds, 1);
    assert_eq!(report.items[1].rounds, 2);
    assert_eq!(report.items[2].rounds, 2);
    // Causes are counted per round: one cause, two affected items, then none.
    assert_eq!(report.causes.len(), 2);
    assert_eq!(
        report.affected(0, "identifier-case-clash"),
        ["r02", "r03"],
        "the cause names its items, not just how many"
    );
    assert!(report.causes[1].is_empty());
    // The final artifact is the repaired text, kept on the report.
    assert_eq!(report.items[1].output.as_deref(), Some(FIXED));
}

/// The repair prompt the loop hands a producer is the gate's own text: the
/// cause stated once with occurrences under it, carrying the concrete fix —
/// not a line-by-line complaint list.
#[test]
fn the_producer_is_handed_the_cause_grouped_repair_prompt() {
    let reqs = requirements(&["a position record"]);
    let mut stage = Scripted {
        clash_on_first: HashSet::from(["a position record".to_owned()]),
        ..Scripted::default()
    };
    let report = run_batch(&mut stage, &reqs, 3);
    assert!(report.all_valid(), "{report}");

    let repair_calls: Vec<&String> = stage.calls.iter().filter_map(|(_, r)| r.as_ref()).collect();
    assert_eq!(repair_calls.len(), 1, "one repair round");
    let prompt = repair_calls[0];
    // Grouped by cause: the rule named once, with its occurrence count.
    assert_eq!(prompt.matches("[identifier-case-clash]").count(), 1, "{prompt}");
    assert!(prompt.contains("occurrence(s)"), "{prompt}");
    // And it carries the concrete fix, phrased as an edit.
    assert!(prompt.contains(r#"Rename "position""#), "{prompt}");
    assert!(prompt.contains("Renaming the *type* is usually wrong"), "{prompt}");
}

/// §5.1 rule 1, asserted on the call log: every item is produced before any
/// verdict exists. If gating were interleaved, a failed early item would be
/// regenerated (a call with a repair prompt) before the last first-pass call —
/// so the first `len` calls being repair-free round-1 calls in order is exactly
/// the property.
#[test]
fn round_one_produces_every_item_before_any_gating() {
    let texts = ["one", "a position record", "three", "another position record"];
    let reqs = requirements(&texts);
    let mut stage = Scripted {
        clash_on_first: HashSet::from([
            "a position record".to_owned(),
            "another position record".to_owned(),
        ]),
        ..Scripted::default()
    };
    run_batch(&mut stage, &reqs, 3);

    assert!(stage.calls.len() > texts.len(), "there was a repair round");
    for (call, requirement) in stage.calls.iter().zip(texts) {
        assert_eq!(call.0, requirement, "round 1 covers the batch in order");
        assert_eq!(call.1, None, "no verdict reached the producer mid-pass: {:?}", call.0);
    }
}

/// The honesty rule: when the rounds run out, the report says so plainly and
/// names the item — never only a final number.
#[test]
fn exhausting_the_rounds_is_reported_plainly() {
    let reqs = requirements(&["fine", "hopeless"]);
    let mut stage =
        Scripted { stubborn: HashSet::from(["hopeless".to_owned()]), ..Scripted::default() };
    let report = run_batch(&mut stage, &reqs, 3);

    assert!(!report.all_valid());
    assert_eq!(report.rounds_used, 3, "it kept trying to the limit");
    let item = &report.items[1];
    assert_eq!(item.rounds, 3);
    let ItemStatus::Invalid { repair_prompt } = &item.status else {
        panic!("still invalid: {:?}", item.status)
    };
    assert!(repair_prompt.contains("identifier-case-clash"), "the trail ends with the prompt");
    // The cause was seen every round — three entries, one affected item each.
    assert!(
        (0..report.causes.len()).all(|r| report.affected(r, "identifier-case-clash") == ["r02"]),
        "the same item, every round"
    );

    let text = report.to_string();
    assert!(text.contains("NOT all valid"), "{text}");
    assert!(text.contains("still failing after 3 round(s)"), "{text}");
    assert!(text.contains("r02"), "the failed item is named: {text}");
    // And the two §5.1 numbers are stated separately, under the stage's name.
    assert!(text.contains("S2 synthesize: 2 item(s)"), "{text}");
    assert!(text.contains("first-pass: 1/2"), "{text}");
    assert!(text.contains("rounds: 3 used"), "{text}");
}

/// A producer error is a per-item failure the report carries — an API outage
/// must not read as invalid IDL and must not panic the batch.
#[test]
fn a_producer_error_is_reported_not_panicked() {
    let reqs = requirements(&["fine", "outage"]);
    let mut stage =
        Scripted { broken: HashSet::from(["outage".to_owned()]), ..Scripted::default() };
    let report = run_batch(&mut stage, &reqs, 2);

    assert!(!report.all_valid());
    let item = &report.items[1];
    let ItemStatus::Error { message } = &item.status else { panic!("{:?}", item.status) };
    assert_eq!(message, "model API unavailable");
    assert_eq!(item.output, None, "it never produced anything and the report invents nothing");
    // Counted as a cause, distinct from any IDL rule: unmeasured is a failure.
    assert_eq!(report.affected(0, "producer-error"), ["r02"]);
    // The good item is unaffected.
    assert_eq!(report.items[0].status, ItemStatus::Valid);
    assert_eq!(report.first_pass_valid, 1);
    let text = report.to_string();
    assert!(text.contains("model API unavailable"), "{text}");
}

/// The CLI end to end, with a shell script standing where a model-wrapping
/// script will stand later: same argument contract, no network.
#[cfg(unix)]
#[test]
fn the_cli_runs_the_loop_against_an_external_command() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("forge-pipeline-e2e-{}", std::process::id()));
    let req_dir = dir.join("requirements");
    let out_dir = dir.join("out");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&req_dir).unwrap();

    std::fs::write(req_dir.join("good.txt"), "a point type").unwrap();
    std::fs::write(req_dir.join("bad.txt"), "please clash").unwrap();

    // The stand-in producer: $1 = input file, $2 = repair prompt file.
    // Captures to variables before matching — the harness rules apply to test
    // fixtures too.
    let script = dir.join("generate.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n\
             if [ \"$#\" -ge 2 ]; then\n\
             \x20 repair=$(cat \"$2\")\n\
             \x20 case \"$repair\" in *identifier-case-clash*) echo '{FIXED}'; exit 0 ;; esac\n\
             fi\n\
             req=$(cat \"$1\")\n\
             case \"$req\" in\n\
             \x20 *clash*) echo '{CLASH}' ;;\n\
             \x20 *) echo '{CLEAN}' ;;\n\
             esac\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    // `--generator` is the pre-split spelling and still works: the run record's
    // command line must not rot because the pipeline grew stages.
    let output = Command::new(env!("CARGO_BIN_EXE_forge-pipeline"))
        .args(["--requirements".as_ref(), req_dir.as_os_str()])
        .args(["--generator".as_ref(), script.as_os_str()])
        .args(["--out".as_ref(), out_dir.as_os_str()])
        .args(["--max-rounds", "3"])
        .output()
        .expect("forge-pipeline runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "exit 0 iff all valid\n{stdout}");

    // The range is inferred from the producers given: S2 through the gate.
    assert!(stdout.contains("range: S2 synthesize → S4 validate"), "{stdout}");
    // The §5.1 summary, with its two numbers stated separately, per stage.
    assert!(stdout.contains("S2 synthesize: 2 item(s)"), "{stdout}");
    assert!(stdout.contains("first-pass: 1/2"), "{stdout}");
    assert!(stdout.contains("rounds: 2 used"), "{stdout}");
    assert!(stdout.contains("[identifier-case-clash] 1 item(s): bad"), "{stdout}");
    // S4 ran over what S2 produced, and says the files it gated were drafts —
    // no annotation stage was in the range.
    assert!(stdout.contains("S4 validate: 2 item(s)"), "{stdout}");
    assert!(stdout.contains("S4 gated 0 annotated file(s) and 2 unannotated draft(s)"), "{stdout}");

    // The IDL landed under the item's id, repaired in place.
    let bad = std::fs::read_to_string(out_dir.join("bad.idl")).unwrap();
    assert!(bad.contains("the_position"), "{bad}");
    let good = std::fs::read_to_string(out_dir.join("good.idl")).unwrap();
    assert!(good.contains("struct Point"), "{good}");
}

/// The CLI's exit code is honest: a producer that never converges means a
/// nonzero exit and a report that says which item is still failing.
#[cfg(unix)]
#[test]
fn the_cli_exits_nonzero_when_the_rounds_run_out() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("forge-pipeline-stubborn-{}", std::process::id()));
    let req_dir = dir.join("requirements");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&req_dir).unwrap();
    std::fs::write(req_dir.join("hopeless.txt"), "anything").unwrap();

    let script = dir.join("generate.sh");
    std::fs::write(&script, format!("#!/bin/sh\necho '{CLASH}'\n")).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_forge-pipeline"))
        .args(["--requirements".as_ref(), req_dir.as_os_str()])
        .args(["--synthesize".as_ref(), script.as_os_str()])
        .args(["--out".as_ref(), dir.join("out").as_os_str()])
        .args(["--max-rounds", "2"])
        .output()
        .expect("forge-pipeline runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(stdout.contains("NOT all valid"), "{stdout}");
    assert!(stdout.contains("hopeless"), "{stdout}");
    // The repair prompt is printed so a human can pick up where it stopped.
    assert!(stdout.contains("identifier-case-clash"), "{stdout}");
    // And the item is named as dropped, so the absence of an S4 report is not
    // read as "S4 was clean".
    assert!(stdout.contains("rejected at S2 synthesize"), "{stdout}");
}
