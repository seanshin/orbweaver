//! Error-message quality, measured over the corpus.
//!
//! §3.3 calls diagnostics a product surface and says error quality is a tested
//! feature rather than a nicety. This is that test. The self-repair loop is
//! only as good as what it feeds on, and "invalid IDL" would make the loop
//! guess — which is how one round becomes three.

use std::path::{Path, PathBuf};

use orbweaver_forge::{Severity, validate};

fn corpus(dir: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(dir);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "idl"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no IDL in {}", root.display());
    files
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn the_gate_accepts_everything_the_oracle_accepts() {
    let mut wrong = Vec::new();
    for dir in ["corpus/golden", "corpus/requirements/generated", "spikes"] {
        for path in corpus(dir) {
            let report = validate(&read(&path));
            if !report.is_ok() {
                let why: Vec<String> = report
                    .findings
                    .iter()
                    .filter(|f| f.severity == Severity::Error)
                    .map(|f| format!("{f}"))
                    .collect();
                wrong.push(format!("{}: {}", path.display(), why.join("; ")));
            }
        }
    }
    assert!(wrong.is_empty(), "the gate rejected valid IDL:\n  {}", wrong.join("\n  "));
}

#[test]
fn the_gate_rejects_every_negative() {
    let mut accepted = Vec::new();
    for path in corpus("corpus/negative") {
        if validate(&read(&path)).is_ok() {
            accepted.push(path.display().to_string());
        }
    }
    assert!(accepted.is_empty(), "these should have been rejected:\n  {}", accepted.join("\n  "));
}

/// The claim §3.3 makes, measured rather than asserted.
///
/// One exemption, and it is named: a missing separator is reported wherever the
/// grammar noticed, which is not reliably where the semicolon belongs. Guessing
/// there would produce a confident wrong instruction, which costs a round.
#[test]
fn every_rejection_a_generator_can_act_on_carries_a_fix() {
    const NO_UNAMBIGUOUS_FIX: &[&str] = &["n01-missing-semicolon.idl"];

    let mut missing = Vec::new();
    for path in corpus("corpus/negative") {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let report = validate(&read(&path));
        let first = report
            .findings
            .iter()
            .find(|f| f.severity == Severity::Error)
            .unwrap_or_else(|| panic!("{name} was not rejected"));
        let exempt = NO_UNAMBIGUOUS_FIX.contains(&name.as_str());
        match (&first.fix, exempt) {
            (Some(_), false) | (None, true) => {}
            (None, false) => missing.push(format!("{name} [{}]", first.rule)),
            // An exemption that stopped applying is worth knowing: it means a
            // fix became possible and the list should shrink.
            (Some(_), true) => {
                missing.push(format!("{name} now has a fix — remove it from the exemption list"))
            }
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n  "));
}

/// A fix that does not name the offending text is advice, not an edit.
#[test]
fn a_fix_names_the_thing_it_wants_changed() {
    for path in corpus("corpus/negative") {
        let src = read(&path);
        for f in validate(&src).findings.iter().filter(|f| f.severity == Severity::Error) {
            let Some(fix) = &f.fix else { continue };
            // Either the fix quotes the source text, or the rule is one where
            // the edit is structural and naming a token would mislead.
            let structural = f.rule.starts_with("duplicate-union");
            assert!(
                structural || fix.contains(&f.source),
                "{}: the fix for {:?} never mentions it: {fix}",
                path.display(),
                f.source
            );
        }
    }
}

/// Every finding must be locatable. A line of 0 on a file-level finding is
/// fine; a line of 0 on something that has a span is a lost source position.
#[test]
fn findings_that_come_from_source_carry_a_position() {
    for path in corpus("corpus/negative") {
        for f in validate(&read(&path)).findings.iter().filter(|f| !f.source.is_empty()) {
            assert!(f.line > 0, "{}: {f:?} has source text but no line", path.display());
        }
    }
}
