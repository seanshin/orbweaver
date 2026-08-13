//! The property, measured over the whole golden corpus at once.
//!
//! §5.1: work the whole set at once and verify the whole set at once. The
//! corpus exists precisely so that "does our marshalling hold" is one question
//! with one answer rather than twenty-three. This test is the codified form of
//! the batch run that landed this crate — the run is repeatable in CI instead
//! of being a number in a commit message.
//!
//! The contract half is *counted* rather than asserted empty: it is advice, the
//! corpus is deliberately full of unannotated interfaces (most of it exists to
//! exercise the type system, not SIDL), and a test that demanded silence would
//! have to be satisfied by annotating files for the test's benefit.

use std::path::{Path, PathBuf};

use orbweaver_forge::Severity;
use orbweaver_registry::Registry;
use orbweaver_test::{contract, prop};

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

fn registry_of(path: &Path) -> Registry {
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let spec = orbweaver_idl::parse(&src)
        .unwrap_or_else(|d| panic!("{} does not parse: {d:?}", path.display()));
    let mut r = Registry::new();
    r.load(&spec).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    r
}

/// Every type in the golden corpus, both byte orders, every alignment phase.
#[test]
fn every_golden_type_is_byte_stable() {
    let mut defects = Vec::new();
    let mut types = 0usize;
    for path in corpus("corpus/golden") {
        let reg = registry_of(&path);
        for id in reg.ids().cloned().collect::<Vec<_>>() {
            let Some(tc) = reg.typecode(&id) else { continue };
            types += 1;
            for f in prop::roundtrip_property(tc, 16) {
                if f.severity == Severity::Error {
                    defects.push(format!("{}: {f}", path.display()));
                }
            }
        }
    }
    // 66 named types across 23 files when this landed. A floor rather than an
    // equality: the corpus grows with the change that motivated it, and a test
    // that has to be edited for every addition gets edited without being read.
    assert!(types >= 60, "the corpus should cover far more than {types} types");
    assert!(defects.is_empty(), "byte instability:\n  {}", defects.join("\n  "));
}

/// The gaps are part of the measurement. An unmeasured arm reported as covered
/// is the harness failure `CLAUDE.md` names; this keeps the list visible and
/// pins its size so a new gap has to be acknowledged.
#[test]
fn the_coverage_gaps_over_the_corpus_are_the_known_ones() {
    let mut gaps = Vec::new();
    for path in corpus("corpus/golden") {
        let reg = registry_of(&path);
        for id in reg.ids().cloned().collect::<Vec<_>>() {
            let Some(tc) = reg.typecode(&id) else { continue };
            for f in prop::roundtrip_property(tc, 8) {
                if f.severity != Severity::Error {
                    gaps.push(format!("{}: {} {}", path.display(), f.rule, f.source));
                }
            }
        }
    }
    let recursive = gaps.iter().filter(|g| g.contains("prop/unmeasured")).count();
    let unsupported = gaps.iter().filter(|g| g.contains("prop/unsupported-type")).count();
    // The recursive gap closed: markers now resolve against the enclosing type
    // the path is standing on, so `corpus/golden/15`'s trees are generated with
    // children and round-trip like anything else. It is asserted at zero rather
    // than removed, because the class can come back — a cycle whose marker
    // names a type that is not enclosing is still unresolvable, and this is
    // where that would show up.
    assert_eq!(
        recursive,
        0,
        "a recursive arm is unmeasured again:\n  {}",
        gaps.iter()
            .filter(|g| g.contains("prop/unmeasured"))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    assert!(
        unsupported > 0,
        "corpus/golden/21-deferred-fixed.idl must report `fixed` as uncovered (§4.4)"
    );
    // Everything non-Error is one of these two, or a new gap class has appeared
    // and deserves a look rather than a silent pass.
    assert_eq!(
        recursive + unsupported,
        gaps.len(),
        "unexpected gap class:\n  {}",
        gaps.join("\n  ")
    );
}

/// The contract half runs clean over the corpus in the sense that matters:
/// it produces findings, never a crash, and never an error-severity verdict.
#[test]
fn contract_advice_over_the_corpus_never_reaches_error() {
    let mut total = 0usize;
    for dir in ["corpus/golden", "corpus/annotations", "corpus/requirements/generated"] {
        for path in corpus(dir) {
            let src = std::fs::read_to_string(&path).expect("read");
            let Ok(spec) = orbweaver_idl::parse(&src) else { continue };
            let mut reg = Registry::new();
            if reg.load(&spec).is_err() {
                continue;
            }
            for f in contract::contract_findings(&reg) {
                assert_ne!(f.severity, Severity::Error, "{}: {f}", path.display());
                total += 1;
            }
        }
    }
    assert!(total > 0, "checks that find nothing on our own corpus are not worth having");
}
