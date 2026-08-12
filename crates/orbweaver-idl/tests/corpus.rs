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

/// Syntactic negatives only. Several negative-corpus files are *syntactically*
/// valid and rejected by the oracle on semantic grounds — duplicate members,
/// identifier clashes — which is the semantic pass's job, not the parser's.
/// Claiming those here would overstate what this crate does.
#[test]
fn syntactic_negatives_are_rejected() {
    let semantic_only = [
        "n02-identifier-clash.idl",
        "n03-scope-clash.idl",
        "n04-unknown-type.idl",
        "n05-unqualified-typecode.idl",
        "n06-duplicate-member.idl",
        "n07-union-dup-label.idl",
        "n09-struct-scope-clash.idl",
        "n10-operation-name-clash.idl",
    ];
    let mut wrongly_accepted = Vec::new();
    for path in corpus("negative") {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if semantic_only.contains(&name.as_str()) {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        if orbweaver_idl::parse(&src).is_ok() {
            wrongly_accepted.push(name);
        }
    }
    assert!(
        wrongly_accepted.is_empty(),
        "these are malformed and we accepted them: {wrongly_accepted:?}"
    );
}
