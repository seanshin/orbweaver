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

use std::path::{Path, PathBuf};
use std::process::Command;

use orbweaver_dynamic::json::Json;
use orbweaver_forge::pipeline::{
    Item, ItemStatus, Pipeline, StageId, ValidateStage, Workspace, run_batch, run_pipeline,
};
use orbweaver_forge::{
    RELEASED_UNREADABLE, Severity, Source, validate_against, validate_source, validate_unit_against,
};
use orbweaver_idl::{SearchPath, preprocess_file};

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

/// `corpus/include/`, which is where the multi-file cases live.
fn corpus_include() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join("corpus").join("include")
}

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

// ─── `--against` over a multi-file contract ──────────────────────────────────
//
// The library above was already right: `validate_source_against` resolves both
// sides from their paths and `a_released_contract_resolves_its_own_includes`
// pins it. The COMMAND was not. `sidl-validate --against` resolved both files
// itself — it has to, to report an unresolvable include as one cause rather
// than as ninety — and then handed the two resolved units' `.text` back to the
// STRING entry point, which preprocessed each splice a second time.
//
// A splice is not a file. It carries the `#ifndef` of every header it
// contains, and a guard that is not the first directive of the text it sits in
// is conditional compilation, which this front end refuses on purpose. So the
// §5.3 comparison over a guarded multi-file contract — the ordinary shape of a
// released contract — never ran at all: it was refused with two
// `unsupported-directive` errors pointing at line 1 of a header.
//
// *라이브러리는 이미 옳았고 커맨드가 틀렸다. 해석된 유닛의 텍스트를 문자열
// 진입점에 되돌려 주면 스플라이스가 두 번 전처리된다.*

fn sidl_validate(args: &[&Path]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_sidl-validate"))
        .args(args)
        .output()
        .expect("sidl-validate runs");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

/// The defect, at the size it was found: the corpus pair, through the binary.
///
/// `corpus/include/evo-proposed.idl` is `evo-released.idl` with its `#include`
/// pointed one file sideways and nothing else changed, so the breaking change —
/// a removed struct member — is in a file neither root names. This is the case
/// the manifest cannot express (`corpus/include/cases.tsv` says why) and it is
/// gated here instead.
#[test]
fn the_command_compares_a_guarded_multi_file_contract_and_finds_the_header_change() {
    let dir = corpus_include();
    let (ok, out) = sidl_validate(&[
        Path::new("--against"),
        &dir.join("evo-released.idl"),
        &dir.join("evo-proposed.idl"),
    ]);

    assert!(
        !out.contains("unsupported-directive"),
        "the guards of the spliced headers are guards, not conditional compilation:\n{out}"
    );
    assert!(
        out.contains("[evolution/BREAKING]")
            && out.contains("IDL:depot.example/Depot/OrderLine:1.0")
            && out.contains("member \"bay\" removed"),
        "the comparison runs, and reports the change made in the included header:\n{out}"
    );
    assert!(!ok, "and a breaking change exits 1:\n{out}");
}

/// The control the case is worth nothing without: the same two files with the
/// baseline and the proposal being the same contract.
///
/// Without it, a `--against` that reported `evolution/BREAKING` on everything
/// would pass the test above.
#[test]
fn the_command_compares_that_contract_with_itself_and_reports_nothing() {
    let dir = corpus_include();
    let released = dir.join("evo-released.idl");
    let (ok, out) = sidl_validate(&[Path::new("--against"), &released, &released]);
    assert!(ok, "a contract compared with itself changes nothing:\n{out}");
    assert!(!out.contains("evolution/"), "{out}");
    assert!(out.contains("0 rejected"), "{out}");
}

/// One convention for who owns a position, not two.
///
/// `validate_unit_for` does not map positions and `validate_unit_against_for`
/// does not either; the binary's one printer maps both with the unit it
/// resolved. So a finding written in an included header prints identically
/// whether or not a baseline was given — and if the two forms ever start
/// disagreeing about where a line was written, this is what says so.
#[test]
fn the_against_form_places_a_header_finding_exactly_where_the_plain_form_does() {
    let header = "\
#ifndef ESTATE_AUDIT_IDL
#define ESTATE_AUDIT_IDL
module Common {
  interface Audit {
    void note(in string what);
  };
};
#endif
";
    let dir = estate(
        "against-positions",
        &[("00-audit.idl", header), ("01-desk.idl", "#include \"00-audit.idl\"\n")],
    );
    let root = dir.join("01-desk.idl");

    let (plain_ok, plain) = sidl_validate(&[&root]);
    let (against_ok, against) = sidl_validate(&[Path::new("--against"), &root, &root]);

    assert!(plain_ok && against_ok, "advice does not fail a run:\n{plain}\n{against}");
    assert!(
        plain.contains(&format!("{}:5:", dir.join("00-audit.idl").display())),
        "the plain form names the header and the header's own line:\n{plain}"
    );
    assert_eq!(against, plain, "and the `--against` form says exactly the same thing");
}

// ─── the position a *machine* reader is handed ───────────────────────────────
//
// The three output forms of this one binary used to disagree about what a line
// number means. The human printer mapped positions back to the file the line
// was written in; `--json` and `--repair-prompt` did not, and served the
// SPLICE's line under the ROOT file's name. Both halves present, the pair
// false — a reader is told to look at line 8 of a file whose line 8 is
// somebody else's declaration, and nothing anywhere goes red.
//
// `--repair-prompt` is the one that costs something: it is read by a model
// (§3.3's self-repair loop, and `spikes/e2e/producer.sh` pastes it into the
// prompt verbatim), so a wrong line sends the repair to the wrong file.
//
// The contract that settles it: `line`/`column` mean the position **in the
// file the text was written in**, which is what the library and the human
// printer have always meant by them; a finding written somewhere other than
// the report's own file names that file in a per-finding `"file"`, emitted
// only when it differs, so a self-contained contract's document is unchanged.
//
// *줄 번호는 스플라이스가 아니라 작성된 파일의 줄이다 — 사람용 출력만이 아니라
// 세 형식 모두에서.*

/// A header with an un-annotated operation, and a root that only includes it.
///
/// The operation is written at `00-audit.idl:5`; in the splice it lands at
/// line 7, which is the number the machine forms used to print.
fn audit_estate(name: &str) -> PathBuf {
    let header = "\
#ifndef ESTATE_AUDIT_IDL
#define ESTATE_AUDIT_IDL
module Common {
  interface Audit {
    void note(in string what);
  };
};
#endif
";
    estate(name, &[("00-audit.idl", header), ("01-desk.idl", "#include \"00-audit.idl\"\n")])
}

fn findings_of(json: &Json) -> &[Json] {
    let files = json.get("files").expect("a files array");
    let Json::Array(files) = files else { panic!("files is an array, got {}", files.kind()) };
    let Json::Array(findings) = files[0].get("findings").expect("findings") else {
        panic!("findings is an array")
    };
    findings
}

/// `--json` names the file the line is a line of.
///
/// Measured before the fix, on exactly this estate:
/// `{"file":"…/01-desk.idl", … "line":7 …}` — the root, and the splice's line.
/// `01-desk.idl` is one line long.
#[test]
fn the_json_form_reports_the_line_in_the_file_it_was_written_in() {
    let dir = audit_estate("json-positions");
    let (_, out) = sidl_validate(&[Path::new("--json"), &dir.join("01-desk.idl")]);
    let json = Json::parse(&out).unwrap_or_else(|e| panic!("`--json` emits JSON: {e:?}\n{out}"));

    let header = dir.join("00-audit.idl").display().to_string();
    let findings = findings_of(&json);
    assert!(!findings.is_empty(), "the header's un-annotated operation is reported:\n{out}");
    for f in findings {
        assert_eq!(
            f.get("line"),
            Some(&Json::Number("5".into())),
            "the header's own line, not the splice's 7:\n{out}"
        );
        assert_eq!(
            f.get("file").and_then(|j| j.as_str()),
            Some(header.as_str()),
            "and the finding names the file that line is a line of:\n{out}"
        );
    }
}

/// The same fact for the form a *model* reads.
///
/// A generator handed `line 7` for a one-line root file either edits the wrong
/// file or gives up, and the loop reports neither.
#[test]
fn the_repair_prompt_sends_a_generator_to_the_file_it_must_edit() {
    let header = "\
#ifndef ESTATE_VERSION_IDL
#define ESTATE_VERSION_IDL
module Common {
  struct Version { unsigned long version; };
};
#endif
";
    let dir = estate(
        "repair-positions",
        &[("00-audit.idl", header), ("01-desk.idl", "#include \"00-audit.idl\"\n")],
    );
    let (ok, out) = sidl_validate(&[Path::new("--repair-prompt"), &dir.join("01-desk.idl")]);
    assert!(!ok, "the clash is a refusal:\n{out}");

    assert!(
        out.contains("    line 4, column 34:"),
        "the clash's own line in the header, not line 6 of the splice:\n{out}"
    );
    assert!(
        out.contains(&format!("written in {}:4", dir.join("00-audit.idl").display())),
        "and the prompt names the file to edit, with the include chain:\n{out}"
    );
}

/// The half that must not move: a self-contained contract.
///
/// The whole corpus is this shape and so is every `--json` consumer today, so
/// the new per-finding `file` is emitted **only** when it differs from the
/// report's. Absent means "the file this report is about".
#[test]
fn a_self_contained_contract_carries_no_per_finding_file() {
    let dir = estate(
        "single-file",
        &[("solo.idl", "module Common {\n  interface Audit {\n    void note();\n  };\n};\n")],
    );
    let root = dir.join("solo.idl");
    let (_, out) = sidl_validate(&[Path::new("--json"), &root]);
    let json = Json::parse(&out).unwrap_or_else(|e| panic!("`--json` emits JSON: {e:?}\n{out}"));

    let findings = findings_of(&json);
    assert!(!findings.is_empty(), "the un-annotated operation is reported:\n{out}");
    for f in findings {
        assert_eq!(f.get("file"), None, "no per-finding file when there is nowhere else:\n{out}");
        assert_eq!(f.get("line"), Some(&Json::Number("3".into())), "{out}");
    }
}

/// A finding about the file as a whole does not claim line 1.
///
/// `Finding::line == 0` is documented as "about the file as a whole" and every
/// `evolution/*` finding is one. Fed to `Unit::locate` — which maps a *span*,
/// and clamps with `.max(1)` because a span's line is 1-based — it came back as
/// line 1 of the root, a position nothing was written at.
/// `orbweaver_forge::written_in` is the one place that knows the difference.
#[test]
fn a_finding_about_the_whole_file_names_the_file_and_no_line() {
    let dir = corpus_include();
    let (ok, out) = sidl_validate(&[
        Path::new("--against"),
        &dir.join("evo-released.idl"),
        &dir.join("evo-proposed.idl"),
    ]);
    assert!(!ok, "a breaking change exits 1:\n{out}");

    let root = dir.join("evo-proposed.idl").display().to_string();
    assert!(
        out.contains(&format!("{root}: error: IDL:depot.example/Depot/OrderLine:1.0")),
        "the file, and no line:\n{out}"
    );
    assert!(
        !out.contains(&format!("{root}:1:0")),
        "and not line 1, where nothing is written:\n{out}"
    );
}

/// One position per finding, on every path.
///
/// The printer prefixed the located `file:line:column` in front of `Display`'s
/// own `line:column`, so every diagnostic this binary ever emitted carried two
/// positions — and over a multi-file contract the second one was a line in the
/// splice. `Finding::rendered_at` is the one renderer; `Display` delegates to
/// it, so the two cannot drift apart again.
#[test]
fn a_finding_prints_its_position_once() {
    let dir = audit_estate("one-position");
    let (_, out) = sidl_validate(&[&dir.join("01-desk.idl")]);

    let header = dir.join("00-audit.idl").display().to_string();
    let diagnostics: Vec<&str> = out.lines().filter(|l| l.starts_with(&header)).collect();
    assert!(!diagnostics.is_empty(), "the header's findings are printed:\n{out}");
    for line in &diagnostics {
        let rest = line
            .strip_prefix(&format!("{header}:5:10: "))
            .unwrap_or_else(|| panic!("the located position, once and in front:\n{line}"));
        assert!(
            rest.starts_with("advice: ") || rest.starts_with("error: "),
            "the severity follows the position directly — no second `line:column`:\n{line}"
        );
    }
    assert!(!out.contains("7:10"), "and the splice's position appears nowhere:\n{out}");
}

/// Why the unit entry point exists, pinned as the thing that goes wrong
/// without it.
///
/// Not a hypothetical: this is the call the binary used to make. Handing a
/// resolved unit's text back to the string form re-preprocesses the splice, and
/// the splice's second `#ifndef` is refused — so the comparison is replaced by
/// a diagnostic about a file that is perfectly well-formed.
#[test]
fn a_splice_handed_back_to_the_string_form_is_refused_and_the_unit_form_is_not() {
    let dir = corpus_include();
    let search = SearchPath::new();
    let released = preprocess_file(&dir.join("evo-released.idl"), &search).expect("reads");
    let proposed = preprocess_file(&dir.join("evo-proposed.idl"), &search).expect("reads");
    assert!(released.is_ok() && proposed.is_ok(), "both roots resolve");

    let by_text = validate_against(&proposed.text, &released.text);
    let refusals: Vec<&str> = by_text
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .map(|f| &*f.rule)
        .collect();
    assert_eq!(
        refusals,
        vec!["unsupported-directive", "unsupported-directive"],
        "the old call: the splice's guards read as conditional compilation, and no diff runs"
    );
    assert!(
        !by_text.findings.iter().any(|f| f.rule.starts_with("evolution/")),
        "which is the part that matters — an unmeasured comparison, reported as a refusal"
    );

    let by_unit = validate_unit_against(&proposed, &released);
    let evolution: Vec<&str> = by_unit
        .findings
        .iter()
        .filter(|f| f.rule.starts_with("evolution/"))
        .map(|f| &*f.rule)
        .collect();
    assert_eq!(evolution, vec!["evolution/BREAKING"], "the unit form compares the contracts");
}
