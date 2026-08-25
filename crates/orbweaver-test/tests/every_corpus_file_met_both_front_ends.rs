//! A corpus file that no second front end has ever seen is an unmeasured file.
//!
//! # What went wrong
//!
//! `corpus/golden/34-corba-principal.idl` landed in `0b8a387` and
//! `corpus/negative/n23`–`n30` in `14228da`, and neither batch ran
//! `spikes/differential.sh`. Seven of those eight files diverge between omniidl
//! and JacORB, and nobody found out until the coordinator's full harness run
//! days later — by which time the batches that could have explained the
//! divergences were finished and gone. Nothing was wrong with the differential:
//! it was simply never run, because agents on this project are told not to run
//! `run_checks.sh` (it takes a machine-wide lock) and nothing named the
//! standalone gate they should have run instead.
//!
//! # Why this is a test and not a sentence
//!
//! The sentence was tried. `CLAUDE.md`'s corpus rule says additions "go in with
//! the change that motivated them", and the differential has been in the
//! command list the whole time; that is a **command** named in a document, and
//! this project has already measured what happens to those — the lint
//! `spikes/idl_lint.py` outlived its own instruction by several phases. What is
//! needed is the *capability*: adding a corpus file must not be possible
//! without the differential having been run over it.
//!
//! So the differential's verdict stops being an event and becomes data.
//! `spikes/differential.sh --record` writes `corpus/differential-results.tsv`,
//! and this test — which needs no oracle installed, and runs in the
//! `cargo test --workspace` every batch already runs — asks only whether the
//! set of files in that record is the set of files on disk. Adding a corpus
//! file therefore goes red here until somebody runs
//!
//! ```text
//! ./spikes/differential.sh --require omniidl,jacorb_idl --record
//! ```
//!
//! which is the only way to write the row, and which cannot be done with one
//! oracle: `--record` refuses unless both are present, because a record made
//! from omniidl alone would say "measured" about a file JacORB had never seen,
//! and that is this same hole one directory further along.
//!
//! # What this does *not* claim
//!
//! Nothing about the verdicts in that file being today's. Only the differential
//! can say that, and it rewrites the file whole every time it is asked to. This
//! is a membership check and says so in three places — here, in the record's own
//! header, and in the failure message.
//!
//! 코퍼스 파일이 두 번째 프런트엔드를 한 번도 거치지 않았다면 그것은 측정되지 않은
//! 파일이다. 문서에 적힌 **명령**은 이미 한 번 실패했으므로, differential의 판정을
//! 사건이 아니라 데이터로 바꾸고 — 오라클이 없어도 읽을 수 있는 기록으로 —
//! 그 기록의 소속만 여기서 검사한다. 판정이 오늘의 것인지는 주장하지 않는다.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The directories `spikes/differential.sh` enumerates, in its own order.
///
/// Listed here rather than derived, because there is nothing to derive them
/// from — the script names them literally. If the script grows a directory and
/// this does not, the new directory's files are simply not gated, so the two
/// lists are named together in the script's comment as well as here.
const ENUMERATED: &[&str] =
    &["corpus/golden", "corpus/requirements/generated", "corpus/negative", "spikes"];

fn idl_files(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "idl"))
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    out.sort();
    out
}

#[test]
fn every_corpus_file_has_a_recorded_differential_verdict() {
    let root = root();
    let results = root.join("corpus/differential-results.tsv");
    let text = std::fs::read_to_string(&results).unwrap_or_else(|e| {
        panic!(
            "corpus/differential-results.tsv is missing ({e}). Write it with:\n  \
             ./spikes/differential.sh --require omniidl,jacorb_idl --record"
        )
    });

    // file → the directory it was found in, so a duplicate basename is caught
    // rather than silently sharing one row: the record is keyed by basename,
    // which is the key `corpus/divergences.tsv` uses too.
    let mut on_disk: BTreeMap<String, String> = BTreeMap::new();
    let mut duplicates = Vec::new();
    for dir in ENUMERATED {
        for file in idl_files(&root.join(dir)) {
            if let Some(first) = on_disk.insert(file.clone(), (*dir).to_owned()) {
                duplicates.push(format!("{file}: in both {first} and {dir}"));
            }
        }
    }
    assert!(
        duplicates.is_empty(),
        "two corpus files share a basename, so they would share one recorded verdict:\n{}",
        duplicates.join("\n")
    );

    let recorded: BTreeMap<&str, &str> = text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut cols = l.split('\t');
            Some((cols.next()?, cols.next()?))
        })
        .collect();

    let unmeasured: Vec<&String> =
        on_disk.keys().filter(|f| !recorded.contains_key(f.as_str())).collect();
    let vanished: Vec<&&str> = recorded.keys().filter(|f| !on_disk.contains_key(**f)).collect();

    assert!(
        unmeasured.is_empty(),
        "{} corpus file(s) have never been through both front ends:\n  {}\n\
         An unmeasured check is a failure, never a pass. Run:\n  \
         ./spikes/differential.sh --require omniidl,jacorb_idl --record\n\
         (This is a membership check; it says nothing about the recorded verdicts \
         being today's.)",
        unmeasured.len(),
        unmeasured.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n  ")
    );
    assert!(
        vanished.is_empty(),
        "{} recorded file(s) no longer exist — re-record so the file stops \
         describing a corpus that is gone:\n  {}",
        vanished.len(),
        vanished.iter().map(|s| **s).collect::<Vec<_>>().join("\n  ")
    );

    // The directory a file is filed under is the verdict it claims, and the
    // record carries that claim. Checking it here costs one comparison and
    // catches a record written against a different corpus layout.
    let mut misfiled = Vec::new();
    for (file, dir) in &on_disk {
        let want = if dir.ends_with("negative") { "reject" } else { "accept" };
        if recorded.get(file.as_str()) != Some(&want) {
            misfiled.push(format!(
                "{file}: {dir} claims {want}, the record says {:?}",
                recorded.get(file.as_str())
            ));
        }
    }
    assert!(misfiled.is_empty(), "{}", misfiled.join("\n"));
}
