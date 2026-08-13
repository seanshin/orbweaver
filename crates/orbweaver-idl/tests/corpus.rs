//! The acceptance criterion for the parser: agree with the oracle.
//!
//! `omniidl` accepts every file in `corpus/golden/` and rejects every file in
//! `corpus/negative/`. Anywhere we disagree, we are wrong — the corpus is not
//! a sample of our behaviour, it is the definition of it.

use std::path::{Path, PathBuf};

fn corpus(dir: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join("corpus").join(dir);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "idl"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no IDL found in {}", root.display());
    files
}

#[test]
fn golden_corpus_is_accepted() {
    let mut failures = Vec::new();
    for path in corpus("golden") {
        let src = std::fs::read_to_string(&path).unwrap();
        if let Err(e) = orbweaver_idl::parse(&src) {
            failures.push(format!("{}: {e}", path.file_name().unwrap().to_string_lossy()));
        }
    }
    assert!(
        failures.is_empty(),
        "the oracle accepts these and we do not:\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn requirement_benchmark_is_accepted() {
    let mut failures = Vec::new();
    for path in corpus("requirements/generated") {
        let src = std::fs::read_to_string(&path).unwrap();
        if let Err(e) = orbweaver_idl::parse(&src) {
            failures.push(format!("{}: {e}", path.file_name().unwrap().to_string_lossy()));
        }
    }
    assert!(failures.is_empty(), "benchmark files must parse:\n  {}", failures.join("\n  "));
}

/// The identity-pragma cases are ordinary valid IDL as well as id fixtures.
///
/// `corpus/pragma/` exists to pin what `#pragma prefix`/`version`/`ID` derive
/// (that comparison lives in `orbweaver-registry`, which is what holds ids).
/// What belongs *here* is the other half: omniidl accepts all sixteen files,
/// so a pragma that made one of them stop parsing or stop checking would be a
/// front-end regression wearing an id-fixture's clothes.
#[test]
fn the_pragma_corpus_is_accepted_and_semantically_clean() {
    let mut failures = Vec::new();
    for path in corpus("pragma") {
        let src = std::fs::read_to_string(&path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        match orbweaver_idl::check(&src) {
            Ok(_) => {}
            Err(ds) => failures.extend(ds.into_iter().map(|d| format!("{name}: {d}"))),
        }
    }
    assert!(
        failures.is_empty(),
        "the oracle accepts these and we do not:\n  {}",
        failures.join("\n  ")
    );
}

/// Every negative, syntactic and semantic alike.
///
/// The parser alone could only reject the syntactic ones, and the exclusion
/// list that acknowledged this is now gone: `check` runs the semantic pass too,
/// so agreement with the oracle is complete rather than partial.
#[test]
fn every_negative_is_rejected() {
    let mut wrongly_accepted = Vec::new();
    for path in corpus("negative") {
        let src = std::fs::read_to_string(&path).unwrap();
        if orbweaver_idl::check(&src).is_ok() {
            wrongly_accepted.push(path.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    assert!(
        wrongly_accepted.is_empty(),
        "the oracle rejects these and we accepted them: {wrongly_accepted:?}"
    );
}

/// Golden files must survive the semantic pass, not only the parser.
#[test]
fn golden_corpus_is_semantically_clean() {
    let mut failures = Vec::new();
    for path in corpus("golden") {
        let src = std::fs::read_to_string(&path).unwrap();
        if let Err(ds) = orbweaver_idl::check(&src) {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            for d in ds {
                failures.push(format!("{name}: {d}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "the oracle accepts these and we do not:\n  {}",
        failures.join("\n  ")
    );
}

/// So must the requirement benchmark, which is what assumption B measured.
#[test]
fn requirement_benchmark_is_semantically_clean() {
    let mut failures = Vec::new();
    for path in corpus("requirements/generated") {
        let src = std::fs::read_to_string(&path).unwrap();
        if let Err(ds) = orbweaver_idl::check(&src) {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            for d in ds {
                failures.push(format!("{name}: {d}"));
            }
        }
    }
    assert!(failures.is_empty(), "benchmark must be clean:\n  {}", failures.join("\n  "));
}
