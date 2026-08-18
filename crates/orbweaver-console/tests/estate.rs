//! The console over a contract set with an estate's shape: files that include
//! each other, a prefix on one of them, and not one annotation anywhere.
//!
//! Every file in `corpus/` is self-contained and annotated, so a corpus of any
//! size cannot go red on either of the two defects measured here — a base
//! declared in another file, and a contract that describes itself to nobody.
//! `spikes/estate/` is where both were found; it is deliberately **not** what
//! this test reads. That estate is a consumer of the shipped tools and
//! "nothing here is allowed to become a gate's input" (`spikes/estate/run.sh`),
//! so the shape is reproduced in this crate's own fixtures and the numbers from
//! the real estate are quoted in the report, not asserted here.
//!
//! 코퍼스는 전부 자기완결 파일이라 여기서 재는 두 결함에 절대 빨간불이 켜지지
//! 않는다. 실제 estate는 게이트의 입력이 되면 안 되므로, 모양만 이 크레이트의
//! 픽스처로 재현한다.

use std::path::PathBuf;

use orbweaver_console::html::escape;
use orbweaver_console::{catalog, load};
use orbweaver_idl::include::SearchPath;
use orbweaver_mcp::interceptor::Chain;
use orbweaver_mcp::policy::{Approval, Exposure, Unannotated};
use orbweaver_registry::Registry;

const STOCK: &str = "IDL:DepotOps/StockControl:1.0";

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/estate").join(name)
}

fn estate() -> Registry {
    let mut registry = Registry::new();
    load::load_into(&mut registry, &fixture("depot.idl"), &SearchPath::new())
        .expect("the include resolves from the including file's own directory");
    registry
}

fn view(registry: &Registry, exposure: Exposure) -> catalog::Catalog {
    let mut chain = Chain::standard(exposure.clone());
    catalog::build(&mut chain, registry, &exposure, None, Approval::default())
}

fn row<'a>(view: &'a catalog::Catalog, id: &str) -> &'a catalog::InterfaceRow {
    view.interfaces.iter().find(|i| i.id == id).expect("the interface")
}

/// The operation an agent can call and the page could not see. On the real
/// estate this was 18 operations across nine interfaces, every one of them an
/// inherited `describe` or `_get_label`.
#[test]
fn an_operation_inherited_across_a_file_boundary_is_on_the_page() {
    let view = view(&estate(), Exposure::nothing().allow_interface(STOCK));
    let names: Vec<&str> = row(&view, STOCK).operations.iter().map(|o| o.name.as_str()).collect();
    assert!(names.contains(&"describe"), "inherited from the included header: {names:?}");
    assert!(names.contains(&"_get_label"), "an attribute is callable too: {names:?}");
    assert!(names.contains(&"purge"), "and its own operations are still there: {names:?}");
}

/// A repository id is identity on the wire. The included header's prefix ends
/// with the header, so an operator allowlisting what the page printed is
/// allowlisting an id a deployed object actually has.
#[test]
fn the_page_prints_the_ids_the_files_themselves_produce() {
    let view = view(&estate(), Exposure::nothing());
    let ids: Vec<&str> = view.interfaces.iter().map(|i| i.id.as_str()).collect();
    assert!(ids.contains(&STOCK), "{ids:?}");
    assert!(ids.contains(&"IDL:meridian.example/Common/Describable:1.0"), "{ids:?}");
}

/// The estate annotates nothing, and the page has to say so in words. A blank
/// cell is an invitation to a reader to supply the meaning, and the reader is
/// deciding what an agent may reach.
#[test]
fn an_unannotated_contract_states_its_silences_rather_than_leaving_blanks() {
    let view = view(&estate(), Exposure::nothing().allow_interface(STOCK));
    let stock = row(&view, STOCK);
    assert_eq!(stock.ai_desc, None, "the fixture annotates nothing");
    assert!(stock.operations.iter().all(|o| o.effect.is_none()));

    let html = catalog::render_html(&view);
    assert!(html.contains("no ai_desc"), "the missing description is drawn, not skipped");
    assert!(html.contains("none stated"), "the missing effect is words, not an em dash");
    assert!(
        !html.contains(">—<"),
        "an em dash alone in a cell is the page inviting the reader to guess"
    );

    let text = catalog::render_text(&view);
    assert!(text.contains("desc: absent"), "{text}");
    assert!(text.contains("effect=absent"), "{text}");
}

/// The contract-shaped counts are all zero on an estate like this, and the
/// gate's answers are not. Both are on the page, because only one of them is
/// about what would happen.
#[test]
fn the_gates_own_answers_are_counted_beside_the_contracts_properties() {
    let view = view(&estate(), Exposure::nothing().allow_interface(STOCK));
    assert_eq!(view.destructive_count(), 0, "nothing here is marked destructive");
    assert_eq!(view.gated_count(), 0, "nothing here names a scope");

    let counts: std::collections::BTreeMap<&str, usize> =
        view.would_counts.iter().map(|(w, n)| (w.as_str(), *n)).collect();
    // Four declared operations refused for a silence, and one `_get_label`
    // allowed — a getter is a read stated by the grammar rather than by an
    // annotation, which is the gate's rule and not this crate's. The page
    // carries the gate's split rather than a count of its own.
    assert_eq!(counts["need_effect"], 4, "{counts:?}");
    assert_eq!(counts["allow"], 1, "{counts:?}");
    assert_eq!(counts["not_exposed"], 2, "the header nobody allowlisted: {counts:?}");
    assert_eq!(view.operation_count(), 7, "{counts:?}");

    let html = catalog::render_html(&view);
    assert!(html.contains("need_effect"), "the gate's own word is on the page");
    assert!(html.contains("operations it was asked about"), "{html}");
}

/// `unannotated_effect` conditions every row underneath it: the same table of
/// `allow`s means one thing when the silences are refused and another when an
/// operator has declared an assumption for them. The survey states it once and
/// the page has to carry it.
#[test]
fn the_posture_that_conditions_every_row_is_on_the_page() {
    let refusing = view(&estate(), Exposure::nothing().allow_interface(STOCK));
    assert_eq!(refusing.unannotated_effect, "refuse");
    assert!(catalog::render_html(&refusing).contains("declares no assumption"));
    assert!(catalog::render_text(&refusing).contains("unannotated-effect=refuse"));

    let assuming = view(
        &estate(),
        Exposure::nothing()
            .allow_interface(STOCK)
            .assuming_unannotated(Unannotated::Assume("read_only".to_owned())),
    );
    assert_eq!(assuming.unannotated_effect, "read_only");
    let html = catalog::render_html(&assuming);
    assert!(html.contains("treated as read_only"), "{html}");
    assert!(html.contains(&escape("an operator declared that assumption")), "{html}");
}
