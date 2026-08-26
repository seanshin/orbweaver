//! D032 §4 clause 5, over **every** target this crate emits for, in
//! `cargo test --workspace`.
//!
//! `spikes/binding_suite.sh` runs this for one language as that language's
//! clause-5 cell, through the `binding-words` binary. This file runs it for all
//! of them at once and needs no harness, which matters for the reason
//! `run_checks.sh`'s own formatting comment gives: *a check that lives only in
//! the harness is a check a plain `cargo test` does not cover.*
//!
//! It ranges over [`orbweaver_gen::targets::TARGETS`] rather than naming
//! languages, so a third target is covered by adding its row there and nothing
//! else — which is the whole of what "one suite, parameterised by language"
//! buys, in its smallest instance.

use orbweaver_gen::targets;
use orbweaver_registry::Registry;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The contract set clause 5 names, and only it.
///
/// Not `corpus/golden/*.idl`: the clause says *"exercised by
/// `28-target-keywords.idl`"*, and widening the input until the count looks
/// good is `a check tuned until it is quiet` — it would report a word as
/// executed because some unrelated contract happened to name it, which is the
/// accident this instrument exists to stop being the mechanism.
fn contract_set() -> Registry {
    let path = root().join("corpus/golden/28-target-keywords.idl");
    let src = std::fs::read_to_string(&path).expect("read the keyword contract");
    let spec = orbweaver_idl::parse(&src).expect("the keyword contract parses");
    let mut r = Registry::new();
    r.load(&spec).expect("the keyword contract loads");
    r
}

fn allowed(language: &str) -> Vec<(String, String)> {
    let path = root().join("spikes/bindings/keywords-not-executed.tsv");
    let text = std::fs::read_to_string(&path).expect("read the allow file");
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            assert!(f.len() >= 3, "not three tab-separated fields: {l:?}");
            (f[0] == language).then(|| (f[1].to_owned(), f[2..].join(" ")))
        })
        .collect()
}

/// Every reserved word of every target is either executed by the contract set
/// or named in the allow file — and every allow row is still needed.
///
/// Both halves, because an exception nobody removed is the same defect as an
/// exception nobody wrote: the file stops describing the tree and the next
/// reader believes it either way.
#[test]
fn every_targets_reserved_words_are_executed_or_accounted_for() {
    let registry = contract_set();
    let mut complaints: Vec<String> = Vec::new();

    for t in targets::TARGETS {
        let (hit, miss) = targets::keyword_coverage(t, &registry);
        let allow = allowed(t.language);

        for w in &miss {
            if !allow.iter().any(|(a, _)| a == w) {
                complaints.push(format!(
                    "{}: \"{w}\" is reserved and its escaped spelling \"{}\" appears nowhere \
                     in what the emitter wrote for corpus/golden/28-target-keywords.idl. \
                     Give it a home in that contract, or a row in \
                     spikes/bindings/keywords-not-executed.tsv saying why it has none.",
                    t.language,
                    (t.escape)(w)
                ));
            }
        }
        for (w, _) in &allow {
            if hit.iter().any(|h| h == w) {
                complaints.push(format!(
                    "{}: \"{w}\" has a row in spikes/bindings/keywords-not-executed.tsv, \
                     but the contract set DOES exercise it now — delete the row",
                    t.language
                ));
            } else if !miss.iter().any(|m| m == w) {
                complaints.push(format!(
                    "{}: \"{w}\" has a row in spikes/bindings/keywords-not-executed.tsv \
                     but is not one of this target's reserved words at all",
                    t.language
                ));
            }
        }
    }

    assert!(complaints.is_empty(), "clause 5 is not met:\n  {}", complaints.join("\n  "));
}

/// The allow file names no language this crate does not emit for.
///
/// A row for `cobol` would sit there forever describing nothing, and — worse —
/// would read to the next person as evidence that a COBOL target exists. The
/// `bears_on` shape: a name the owning list does not have is a failure naming
/// the bad name, not an empty pass.
#[test]
fn the_allow_file_names_only_targets_that_exist() {
    let path = root().join("spikes/bindings/keywords-not-executed.tsv");
    let text = std::fs::read_to_string(&path).expect("read the allow file");
    for l in text.lines().filter(|l| !l.trim().is_empty() && !l.starts_with('#')) {
        let language = l.split('\t').next().unwrap_or_default();
        assert!(
            targets::target(language).is_some(),
            "spikes/bindings/keywords-not-executed.tsv has a row for {language:?}, \
             which is not a target this crate emits for ({})",
            targets::TARGETS.iter().map(|t| t.language).collect::<Vec<_>>().join(", ")
        );
    }
}
