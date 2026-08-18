//! S4 over a directory of contracts that include each other.
//!
//! Nineteen call sites read a `.idl` **path** and handed the text to a string
//! entry point that cannot resolve a relative `#include`; two commits fixed
//! them. `forge-pipeline`'s S4 survived both sweeps because it is a different
//! shape: it does not read a path at all, it is *given* its item, and the item
//! was a `(id, text)` pair. Text has no directory, so pointed at the thirteen
//! contracts of `spikes/estate/idl` it refused all thirteen and exited 1 —
//! invisible only because `spikes/estate/run.sh` amalgamates the files into one
//! unit before the pipeline sees them.
//!
//! The fix is not "always take a path". An item may genuinely originate as
//! text — a model writes IDL that was never a file — so the item **carries**
//! where it came from, and an item with no origin still says so. Both halves
//! are pinned here, because a change that made everything pass would have
//! removed a check rather than fixed one.
//!
//! *항목이 출처를 들고 다닌다. 출처가 없으면 없다고 말하고, 그래도 실패한다.*

use std::path::PathBuf;

use orbweaver_forge::pipeline::{
    Item, ItemStatus, Pipeline, StageId, ValidateStage, Workspace, run_batch, run_pipeline,
};
use orbweaver_forge::{RELEASED_UNREADABLE, Severity, Source, validate_source};
use orbweaver_idl::SearchPath;

const HEADER: &str = "\
#ifndef ESTATE_COMMON_IDL
#define ESTATE_COMMON_IDL
module Common {
  typedef string RefNo;
  exception Denied { string why; };
};
#endif
";

const BOOKING: &str = "\
#include \"00-common.idl\"

module Booking {
  interface Desk {
    Common::RefNo reserve(in string who) raises (Common::Denied);
  };
};
";

/// A directory nobody's pipeline produced — the way an estate arrives.
fn estate(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("forge-origin-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creates");
    for (file, text) in files {
        std::fs::write(dir.join(file), text).expect("writes");
    }
    dir
}

fn gate_items(items: &[Item]) -> Vec<(String, ItemStatus)> {
    let mut gate = ValidateStage::new();
    run_batch(&mut gate, items, 1).items.into_iter().map(|i| (i.id, i.status)).collect()
}

fn refusal(status: &ItemStatus) -> &str {
    match status {
        ItemStatus::Invalid { repair_prompt } => repair_prompt,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// The defect, at the size it was found: a directory of contracts that include
/// each other, gated one item at a time.
#[test]
fn an_item_that_carries_its_path_resolves_the_include_beside_it() {
    let dir = estate("resolves", &[("00-common.idl", HEADER), ("01-booking.idl", BOOKING)]);
    let ws = Workspace::new(&dir);

    let ids = ws.ids_ready_for(StageId::Validate);
    assert_eq!(ids, vec!["00-common".to_owned(), "01-booking".to_owned()]);
    let items: Vec<Item> =
        ids.iter().map(|id| ws.load_item(StageId::Validate, id).expect("reads")).collect();
    assert_eq!(
        items[1].origin.as_deref(),
        Some(dir.join("01-booking.idl").as_path()),
        "the item knows the file it was read from"
    );

    let mut pipeline = Pipeline::gate_only();
    let report = run_pipeline(&mut pipeline, &ws, &items).expect("runs");
    assert!(report.all_valid(), "the estate gates clean:\n{report}");
}

/// The other half, and the one a "always take a path" fix would have deleted.
///
/// A model may write IDL that was never a file. Resolving that against the
/// process's working directory would make one contract mean different things
/// depending on where the validator was invoked, so it stays refused — and the
/// message stays the one that names the real cause.
#[test]
fn the_same_text_with_no_origin_is_still_refused_and_still_says_why() {
    let items = vec![Item::new("01-booking", BOOKING)];
    assert_eq!(items[0].origin, None, "a `(id, text)` item has no origin, and says so");

    let gated = gate_items(&items);
    let prompt = refusal(&gated[0].1);
    assert!(prompt.contains("[include-not-found]"), "{prompt}");
    assert!(
        prompt.contains("this source was supplied as text, not read from a file"),
        "the diagnostic still names the real cause:\n{prompt}"
    );
}

/// An include that resolves to nothing must still fail when the origin *is*
/// known — otherwise the fix removed the check instead of the defect.
#[test]
fn an_unresolvable_include_still_fails_when_the_origin_is_known() {
    let dir = estate("missing", &[("01-booking.idl", BOOKING)]);
    let ws = Workspace::new(&dir);
    let items = vec![ws.load_item(StageId::Validate, "01-booking").expect("reads")];

    let gated = gate_items(&items);
    let prompt = refusal(&gated[0].1);
    assert!(prompt.contains("[include-not-found]"), "{prompt}");
    assert!(
        prompt.contains(&dir.join("00-common.idl").display().to_string()),
        "and it names the directory it actually searched:\n{prompt}"
    );
}

/// A resolved unit is several files spliced together, and a line number in the
/// splice points into a document nobody can open. §3.3 hands these straight
/// back to a generator, so the position is mapped back to the file the line was
/// written in and the message says which file that was.
#[test]
fn a_finding_written_in_an_included_header_names_the_header() {
    let header = "\
module Common {
  struct Version { unsigned long version; };
};
";
    let dir = estate(
        "header-fault",
        &[("00-common.idl", header), ("01-booking.idl", "#include \"00-common.idl\"\n")],
    );
    let text = std::fs::read_to_string(dir.join("01-booking.idl")).expect("reads");
    let report =
        validate_source(Source::from_file(&text, &dir.join("01-booking.idl")), &SearchPath::new());

    let clash = report
        .findings
        .iter()
        .find(|f| f.severity == Severity::Error)
        .expect("the header's clash is a refusal");
    assert!(clash.message.contains("00-common.idl"), "it names the header: {}", clash.message);
    assert!(clash.message.contains("included from"), "with the chain: {}", clash.message);
    assert_eq!(clash.line, 2, "and the line is the header's own line, not the splice's");
}

/// The baseline is a path too.
///
/// `--registered` compares against a released contract, and a released contract
/// read as a string loses every name its headers declared — so a resolved
/// proposal against an unresolved baseline reports the shared header as newly
/// added. Fixing one side only would have been a worse gate than fixing
/// neither.
#[test]
fn a_released_contract_resolves_its_own_includes() {
    let dir = estate("released", &[("00-common.idl", HEADER), ("01-booking.idl", BOOKING)]);
    let released = dir.join("01-booking.idl");
    let text = std::fs::read_to_string(&released).expect("reads");

    let report = orbweaver_forge::validate_source_against(
        Source::from_file(&text, &released),
        Source::from_file(&text, &released),
        &SearchPath::new(),
    );
    assert!(report.is_ok(), "a contract compared with itself changes nothing:\n{report:?}");
    assert!(
        !report.findings.iter().any(|f| f.rule.starts_with("evolution/")),
        "and no name from the shared header reads as newly added:\n{report:?}"
    );
}

/// An unmeasured check is a failure, never a pass.
///
/// A released contract that will not resolve produced *no diff at all*, and the
/// old code returned the proposal's clean verdict unchanged — which every
/// caller read as "compared, and nothing breaks".
#[test]
fn a_baseline_that_does_not_hold_up_is_reported_as_never_compared() {
    let report = orbweaver_forge::validate_source_against(
        Source::anonymous("module Booking { interface Desk { void reserve(); }; };"),
        Source::anonymous("#include \"nowhere.idl\"\nmodule Booking {};"),
        &SearchPath::new(),
    );
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule == RELEASED_UNREADABLE)
        .unwrap_or_else(|| panic!("the comparison says it did not run:\n{report:?}"));
    assert_eq!(finding.severity, Severity::Error);
    assert!(finding.message.contains("never ran"), "{}", finding.message);
}

/// S5 re-checks every item before registering it. Re-checking the *text* with
/// no origin would reject exactly the estates S4 had just passed.
#[test]
fn registration_resolves_the_same_includes_the_gate_did() {
    let dir = estate("register", &[("00-common.idl", HEADER), ("01-booking.idl", BOOKING)]);
    let ws = Workspace::new(&dir);
    let items: Vec<Item> = ws
        .ids_ready_for(StageId::Validate)
        .iter()
        .map(|id| ws.load_item(StageId::Validate, id).expect("reads"))
        .collect();

    let mut gate = ValidateStage::new();
    let batch = run_batch(&mut gate, &items, 1);
    assert!(batch.all_valid(), "S4 passes:\n{batch}");
    assert_eq!(
        batch.items[1].origin.as_deref(),
        Some(dir.join("01-booking.idl").as_path()),
        "and the report carries the origin on to S5"
    );

    let registration = orbweaver_forge::pipeline::register(&batch, &dir).expect("S5 registers");
    assert!(
        registration.exposable.iter().any(|id| id.contains("Booking/Desk")),
        "the interface behind the include is in the catalog: {:?}",
        registration.exposable
    );
}

/// `-I` is for the angled form; the quoted form needs nothing, which is the
/// whole reason an estate whose headers sit beside it gates with no flags.
#[test]
fn the_search_path_serves_the_angled_form() {
    let headers = estate("angled-headers", &[("00-common.idl", HEADER)]);
    let dir = estate(
        "angled",
        &[("01-booking.idl", &BOOKING.replace("\"00-common.idl\"", "<00-common.idl>"))],
    );
    let ws = Workspace::new(&dir);
    let items = vec![ws.load_item(StageId::Validate, "01-booking").expect("reads")];

    // Without it, refused — the angled form never looks beside the includer.
    assert!(matches!(gate_items(&items)[0].1, ItemStatus::Invalid { .. }));

    let search: SearchPath = [headers.as_path()].into_iter().collect();
    let mut gate = ValidateStage::new().searching(search);
    assert!(run_batch(&mut gate, &items, 1).all_valid());
}
