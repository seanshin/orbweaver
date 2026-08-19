//! `idl-diff --approve` as a record: the whole approve → replay → invalidate
//! sequence over `corpus/evolution/moe`, driven through the binary, because
//! the exit code is the deliverable and a library test cannot see it.
//!
//! What this holds:
//!
//! 1. Without an approval the in-place edit is refused (exit 1) — unchanged.
//! 2. `--approve` with no `--approver` and no `ORBWEAVER_APPROVER` is exit 2:
//!    a decision with no name on it is not a decision on record.
//! 3. `--approve --approver` exits 0 and writes one row per blocking finding.
//! 4. A re-run without `--approve` reads the store, reports each finding as
//!    `[approved by …]`, exits 0, and writes nothing.
//! 5. One byte appended to the proposed file: exit 1 again, and the output
//!    says the approval on record was for a different revision.
//! 6. The negative control for the store itself: blank the approver column
//!    and the store is refused whole, exit 2 — with or without `--approve`.
//! 7. Replay: a fresh store for the same diff is byte-identical apart from
//!    the `approved_at` column, and identical in full under
//!    `SOURCE_DATE_EPOCH`. Re-approving onto an existing store appends nothing.
//!
//! *승인은 파일에 기록되고, 이름 없는 승인은 거부되며, 한 바이트만 바뀌어도
//! 기록은 더 이상 적용되지 않는다. 재실행은 시각 열을 빼면 바이트까지 같다.*

use std::path::{Path, PathBuf};
use std::process::Command;

fn corpus(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join("corpus").join(rel)
}

fn scratch() -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("orbweaver-approval-replay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn idl_diff(args: &[&str], env: &[(&str, &str)]) -> Run {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_idl-diff"));
    cmd.args(args).env_remove("ORBWEAVER_APPROVER").env_remove("SOURCE_DATE_EPOCH");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("idl-diff runs");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// The store's rows without their last column, for the "apart from the
/// timestamp" comparison.
fn without_timestamp(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .expect("store readable")
        .lines()
        .map(|l| {
            l.rsplit_once('\t').map(|(head, _)| head.to_owned()).unwrap_or_else(|| l.to_owned())
        })
        .collect()
}

#[test]
fn an_approval_is_a_record_that_replays_and_invalidates() {
    let released = corpus("evolution/moe/v1.0/moe.idl");
    let proposed = corpus("evolution/moe/v1.1-in-place/moe.idl");
    let (released, proposed) = (released.to_str().unwrap(), proposed.to_str().unwrap());
    let dir = scratch();
    let store = dir.join("moe.approvals.tsv");
    let store_s = store.to_str().unwrap();
    let reason = "v1.1 rollout, every peer rebuilt against golden 22";

    // 1. Refused without an approval, as before.
    let r = idl_diff(&[released, proposed, "--approvals", store_s], &[]);
    assert_eq!(r.code, 1, "{}{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("refused: 2 change(s)"), "{}", r.stdout);
    assert!(!store.exists(), "nothing written without --approve");

    // 2. An approval with no name is not given.
    let r = idl_diff(&[released, proposed, "--approve", reason, "--approvals", store_s], &[]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("--approver"), "{}", r.stderr);
    assert!(!store.exists(), "a refused approval writes nothing");

    // 3. With a name: accepted, two rows.
    let r = idl_diff(
        &[released, proposed, "--approve", reason, "--approver", "harness", "--approvals", store_s],
        &[],
    );
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("recorded to"), "{}", r.stdout);
    let rows: Vec<String> = std::fs::read_to_string(&store)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(str::to_owned)
        .collect();
    assert_eq!(rows.len(), 2, "{rows:?}");
    for row in &rows {
        let cells: Vec<&str> = row.split('\t').collect();
        assert_eq!(cells.len(), 10, "{row}");
        assert_eq!(cells[4], "IDL:moe/Capability:1.0");
        assert_eq!(cells[5], "BREAKING");
        assert_eq!(cells[7], reason);
        assert_eq!(cells[8], "harness");
        assert!(cells[9].ends_with('Z') && cells[9].len() == 20, "ISO 8601 UTC: {}", cells[9]);
    }
    assert!(rows[0].contains("latency_p50_ms") && rows[1].contains("specialization"), "{rows:?}");
    let first_bytes = std::fs::read(&store).unwrap();

    // 4. Read back: approved, exit 0, store untouched.
    let r = idl_diff(&[released, proposed, "--approvals", store_s], &[]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(
        r.stdout.matches(&format!("[approved by harness: {reason}]")).count(),
        2,
        "{}",
        r.stdout
    );
    assert!(r.stdout.contains("accepted under approval on record"), "{}", r.stdout);
    assert_eq!(std::fs::read(&store).unwrap(), first_bytes, "reading does not write");

    // 7a. Re-approving onto the same store appends nothing.
    let r = idl_diff(
        &[
            released,
            proposed,
            "--approve",
            "again",
            "--approver",
            "someone",
            "--approvals",
            store_s,
        ],
        &[],
    );
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(std::fs::read(&store).unwrap(), first_bytes, "already covered: nothing appended");

    // 5. One byte of the proposed file: the rows are for other bytes.
    let edited = dir.join("moe.idl");
    let mut bytes = std::fs::read(proposed).unwrap();
    bytes.push(b'\n');
    std::fs::write(&edited, bytes).unwrap();
    let r = idl_diff(&[released, edited.to_str().unwrap(), "--approvals", store_s], &[]);
    assert_eq!(r.code, 1, "{}{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("was for a different revision"), "{}", r.stdout);
    assert!(r.stdout.contains("proposed contract has changed"), "{}", r.stdout);
    assert!(r.stdout.contains("an edited file needs a new approval"), "{}", r.stdout);

    // 6. Negative control: a nameless row refuses the whole store.
    let blank = dir.join("blank.tsv");
    let text = std::fs::read_to_string(&store).unwrap().replace("\tharness\t", "\t\t");
    std::fs::write(&blank, text).unwrap();
    let blank_s = blank.to_str().unwrap();
    let r = idl_diff(&[released, proposed, "--approvals", blank_s], &[]);
    assert_eq!(r.code, 2, "{}{}", r.stdout, r.stderr);
    assert!(r.stderr.contains("approver is blank"), "{}", r.stderr);
    assert!(r.stderr.contains("refused whole"), "{}", r.stderr);
    let before = std::fs::read(&blank).unwrap();
    let r = idl_diff(
        &[released, proposed, "--approve", reason, "--approver", "harness", "--approvals", blank_s],
        &[],
    );
    assert_eq!(r.code, 2, "a refused store is not appended to: {}{}", r.stdout, r.stderr);
    assert_eq!(std::fs::read(&blank).unwrap(), before);

    // 7b. Replay into a fresh store: identical apart from the timestamp column.
    let again = dir.join("again.tsv");
    let r = idl_diff(
        &[
            released,
            proposed,
            "--approve",
            reason,
            "--approver",
            "harness",
            "--approvals",
            again.to_str().unwrap(),
        ],
        &[],
    );
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert_eq!(without_timestamp(&store), without_timestamp(&again));

    // 7c. And identical in full when the clock is pinned.
    let pinned = [dir.join("pinned-1.tsv"), dir.join("pinned-2.tsv")];
    for p in &pinned {
        let r = idl_diff(
            &[
                released,
                proposed,
                "--approve",
                reason,
                "--approver",
                "harness",
                "--approvals",
                p.to_str().unwrap(),
            ],
            &[("SOURCE_DATE_EPOCH", "1787097600")],
        );
        assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
        assert!(r.stdout.contains("2026-08-19T00:00:00Z"), "{}", r.stdout);
    }
    assert_eq!(std::fs::read(&pinned[0]).unwrap(), std::fs::read(&pinned[1]).unwrap());

    // The approver may come from the environment instead of the flag.
    let env_store = dir.join("env.tsv");
    let r = idl_diff(
        &[released, proposed, "--approve", reason, "--approvals", env_store.to_str().unwrap()],
        &[("ORBWEAVER_APPROVER", "  from-env  ")],
    );
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(std::fs::read_to_string(&env_store).unwrap().contains("\tfrom-env\t"));

    let _ = std::fs::remove_dir_all(&dir);
}

/// The default store is `<proposed>.approvals.tsv`, and nothing under
/// `corpus/evolution/` carries one: the corpus pairs are the harness's negative
/// controls, and a store beside one would turn a refusal into an acceptance
/// with nothing else changed. This test is what goes red if somebody commits one.
#[test]
fn no_corpus_pair_carries_a_default_store() {
    let root = corpus("evolution");
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.to_string_lossy().ends_with(".approvals.tsv") {
                panic!(
                    "{} would silently approve a corpus negative control; approvals for corpus \
                     pairs live in a harness scratch directory, never in the tree",
                    path.display()
                );
            }
        }
    }
}
