//! What `#include` resolution buys the §5.3 gate, and what its absence cost.
//!
//! Twelve binaries read a `.idl` **path**, threw the path away, and handed the
//! text to `orbweaver_idl::parse` — the *string* entry point, which by its own
//! documentation cannot resolve a relative `#include`. The consequence was
//! invisible because an unresolved name was dropped rather than reported: the
//! console drew 58 of the estate's 76 reachable operations and said nothing
//! about the other 18.
//!
//! `corpus/evolution/` is the case where being wrong about it is worst. Two
//! revisions of one contract whose root file is byte-identical; both breaking
//! changes are in the shared header. Read as strings, the two revisions are the
//! same translation unit and the release gate accepts them.
//!
//! *두 리비전의 루트 파일은 바이트까지 동일하고, 파괴적 변경은 공유 헤더에만
//! 있다. 문자열로 읽으면 게이트는 이를 통과시킨다.*

use std::path::{Path, PathBuf};

use orbweaver_idl::SearchPath;
use orbweaver_registry::diff::diff;
use orbweaver_registry::{Registry, Strictness, UnresolvedKind, registry_from_files};

fn corpus(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join("corpus").join(rel)
}

fn resolved(rel: &str) -> Registry {
    registry_from_files(&[corpus(rel)], &SearchPath::new(), Strictness::Grammar)
        .unwrap_or_else(|e| panic!("{rel} must load: {e}"))
}

/// Reads the root file as a string, the way every affected binary did.
fn unresolved_the_old_way(rel: &str) -> Registry {
    let src = std::fs::read_to_string(corpus(rel)).expect("readable");
    let spec = orbweaver_idl::parse(&src).expect("the root file parses on its own");
    let mut reg = Registry::new();
    reg.load(&spec).expect("loads");
    reg
}

/// The measurement the batch turned on: the gate's verdict, both ways.
#[test]
fn a_breaking_change_in_an_included_header_reaches_the_differ() {
    let old_a = unresolved_the_old_way("evolution/v1/ledger.idl");
    let old_b = unresolved_the_old_way("evolution/v2/ledger.idl");
    assert!(
        diff(&old_a, &old_b).is_empty(),
        "the defect this test exists for: read as strings the two revisions are indistinguishable, \
         so `idl-diff` printed `no change` and exited 0 over two breaking changes"
    );

    let changes = diff(&resolved("evolution/v1/ledger.idl"), &resolved("evolution/v2/ledger.idl"));
    let blocking: Vec<_> = changes.iter().filter(|c| c.verdict.blocks_release()).collect();
    assert_eq!(
        blocking.len(),
        2,
        "both header-only changes must block the release, got: {changes:?}"
    );
    let text = blocking.iter().map(|c| c.to_string()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("amount_minor"), "the retyped struct member is missing from {text}");
    assert!(text.contains("restamp"), "the removed inherited operation is missing from {text}");
}

/// The base an included file declares has to end up in the inheritance graph,
/// not merely stop being an error.
#[test]
fn an_inherited_base_declared_next_door_is_resolved() {
    let reg = resolved("evolution/v1/ledger.idl");
    assert!(reg.unresolved().is_empty(), "nothing should be unresolved: {:?}", reg.unresolved());

    let journal = reg.id_of("Ledger::Journal").expect("Journal is registered").clone();
    let recorded = reg.id_of("Ledger::Recorded").expect("Recorded is registered").clone();
    assert!(
        reg.is_a(&journal, &recorded),
        "Journal inherits Recorded across the `#include`; {journal} did not"
    );
    assert!(
        reg.resolve_operation(&journal, "restamp").is_some(),
        "an inherited operation is only reachable if the base resolved"
    );
}

/// Silence is what made the console defect invisible. A base that cannot be
/// resolved is now on the record — and the load still succeeds, because
/// `Registry::load` is documented as accumulating and cannot know whether the
/// name arrives in a later call.
#[test]
fn an_unresolvable_base_is_recorded_rather_than_dropped() {
    let spec =
        orbweaver_idl::parse("module Ledger { interface Journal : Recorded { void touch(); }; };")
            .expect("parses");
    let mut reg = Registry::new();
    reg.load(&spec).expect("a partial unit still loads");

    let noted = reg.unresolved();
    assert_eq!(noted.len(), 1, "expected exactly one recorded gap, got {noted:?}");
    assert_eq!(noted[0].kind, UnresolvedKind::Base);
    assert_eq!(noted[0].name, "Recorded");
    assert_eq!(noted[0].at, "Ledger::Journal");
    assert!(
        noted[0].to_string().contains("not declared in this unit"),
        "the marker has to read as something to fix: {}",
        noted[0]
    );
}

/// The same treatment for `raises`, which costs the caller the ability to
/// recognise the exception it is handed.
#[test]
fn an_unresolvable_raises_is_recorded_too() {
    let spec = orbweaver_idl::parse(
        "module Ledger { interface Journal { void touch() raises (::Freight::NotFound); }; };",
    )
    .expect("parses");
    let mut reg = Registry::new();
    reg.load(&spec).expect("loads");

    let noted = reg.unresolved();
    assert_eq!(noted.len(), 1, "expected exactly one recorded gap, got {noted:?}");
    assert_eq!(noted[0].kind, UnresolvedKind::Raises);
    assert_eq!(noted[0].name, "::Freight::NotFound");
    assert_eq!(noted[0].at, "Ledger::Journal::touch");
}

/// `-I` is the C convention `omniidl` implements and the one every tool that
/// takes a path now shares, so it is parsed in one place rather than four.
#[test]
fn include_dirs_are_taken_out_of_an_argument_list() {
    let mut args: Vec<String> =
        ["-I", "one", "-Itwo", "a.idl", "--quiet", "b.idl"].iter().map(|s| s.to_string()).collect();
    let search = orbweaver_registry::take_include_dirs(&mut args).expect("both forms parse");
    assert_eq!(search.dirs(), [PathBuf::from("one"), PathBuf::from("two")]);
    assert_eq!(args, ["a.idl", "--quiet", "b.idl"], "everything else keeps its order");

    let mut dangling = vec!["-I".to_string()];
    assert!(orbweaver_registry::take_include_dirs(&mut dangling).is_err(), "-I needs a directory");
}

/// A `#include` that resolves to nothing is refused with the cause, not carried
/// into analysis as one diagnostic per name the missing file declared.
#[test]
fn an_unresolvable_include_is_refused_with_the_cause() {
    let dir = std::env::temp_dir().join("orbweaver-include-resolution-test");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("root.idl");
    std::fs::write(&path, "#include \"nowhere.idl\"\nmodule M { interface I : Base {}; };\n")
        .expect("write");

    let err = registry_from_files(&[&path], &SearchPath::new(), Strictness::Grammar)
        .expect_err("an unresolvable include must not load");
    assert!(
        err.message.contains("nowhere.idl"),
        "the refusal has to name the file that was not found: {}",
        err.message
    );
    let _ = std::fs::remove_file(&path);
}
