//! The console must not attack its own operator.
//!
//! Since remote IFR ingestion the catalog holds repository ids and interface
//! names **a peer chose**. `Registry::define_ingested` marks their provenance
//! and refuses to let them overwrite anything, which is the trust boundary at
//! the registry; it does not and cannot make the strings safe to put in a
//! document. The document is this crate's problem.
//!
//! The attack is short. A peer's Interface Repository serves an interface whose
//! repository id contains `<script>`. An operator opens the console to decide
//! what an agent may reach — which is precisely the page where the ingested
//! entry is most likely to be looked at, and precisely the person whose browser
//! is worth the most. If the id renders as markup, the console has delivered
//! `docs/PLAN.md` §9.0's "tool poisoning via remote metadata" to the one reader
//! it exists to protect.
//!
//! So the payload is put through every field the console renders from data —
//! ingested id, ingestion source label, `ai_desc` prose, an allowlist line, a
//! diff's change text, and every one of D004's nine trace keys — and the page
//! is then checked **structurally**: every element in it must be one of the
//! literal tags this crate emits.
//!
//! That check rather than a substring search, because a substring search is
//! both too weak and too strong. Too weak: it only finds the payloads somebody
//! thought of. Too strong, and this is the mistake worth recording — the first
//! version of this test forbade the text `onerror=`, which appears in a
//! **correctly** escaped page as part of `&lt;img src=x onerror=alert(1)&gt;`.
//! Inert text that reads like an attack is not an attack, and a test that
//! cannot tell them apart fails on working code. Enumerating the tags that
//! exist answers the actual question: is there an element here that nobody in
//! this crate wrote?

use orbweaver_console::{catalog, contract, html, traces};
use orbweaver_mcp::interceptor::Chain;
use orbweaver_mcp::policy::{Approval, Exposure};
use orbweaver_registry::{Entry, InterfaceEntry, Registry};

/// One string carrying every shape that turns text into markup: an element, an
/// attribute break-out, a quote break-out and an entity.
const PAYLOAD: &str = r#"<script>alert("xss")</script><img src=x onerror=alert(1)>"'&"#;

/// A repository id shaped like one a hostile Interface Repository would serve.
const HOSTILE_ID: &str = r#"IDL:evil/<script>alert("pwned")</script>:1.0"#;

/// The complete set of elements `orbweaver-console` ever writes. Every one is
/// a `&'static str` literal inside the crate; nothing derived from data can
/// reach a tag position, which is what makes an allowlist the right shape of
/// assertion rather than a fragile one.
const OURS: [&str; 18] = [
    "html", "head", "meta", "title", "style", "body", "main", "h1", "h2", "p", "div", "span",
    "table", "tr", "th", "td", "b", "footer",
];

/// Every element name that appears in `page`, opening and closing alike.
fn tags(page: &str) -> Vec<String> {
    let bytes = page.as_bytes();
    let mut found = Vec::new();
    for (i, b) in bytes.iter().enumerate() {
        if *b != b'<' {
            continue;
        }
        let mut at = i + 1;
        if bytes.get(at) == Some(&b'/') {
            at += 1;
        }
        // `<!doctype` and a stray `<` in text both fall out here.
        if !bytes.get(at).is_some_and(u8::is_ascii_alphabetic) {
            continue;
        }
        let end = bytes[at..]
            .iter()
            .position(|c| !c.is_ascii_alphanumeric())
            .map_or(bytes.len(), |n| at + n);
        found.push(page[at..end].to_ascii_lowercase());
    }
    found
}

/// No element on the page is one this crate did not write, and the payload is
/// still there — escaped.
fn assert_inert(page: &str, what: &str) {
    for tag in tags(page) {
        assert!(OURS.contains(&tag.as_str()), "{what}: an element nobody here wrote: <{tag}>");
    }
    // A page that dropped the payload instead of escaping it would pass the
    // check above and lie about what is in the catalog. Rendering untrusted
    // input is the job; rendering it inert is the requirement.
    assert!(page.contains("&lt;script&gt;"), "{what}: the payload was dropped, not escaped");
}

fn catalog_of(registry: &Registry, exposure: Exposure) -> catalog::Catalog {
    let mut chain = Chain::standard(exposure.clone());
    catalog::build(&mut chain, registry, &exposure, None, Approval::default())
}

/// The case named in the task: an ingested-looking interface name containing
/// markup.
#[test]
fn an_ingested_interface_name_containing_markup_renders_as_text() {
    let mut registry = Registry::new();
    registry
        .define_ingested(
            HOSTILE_ID.to_owned(),
            Entry::Interface(InterfaceEntry::default()),
            PAYLOAD,
        )
        .expect("a peer's description enters the registry");

    let view = catalog_of(&registry, Exposure::nothing());
    let row = view.interfaces.iter().find(|i| i.id == HOSTILE_ID).expect("the ingested row");
    assert!(row.ingested(), "the row is marked as coming off the wire");

    let page = catalog::render_html(&view);
    assert_inert(&page, "catalog with a hostile ingested id");

    // The operator still learns the two facts the page exists to tell them.
    assert!(page.contains("ingested from"), "provenance is still visible");
    assert!(page.contains("not exposed"), "exposure is still visible");
}

/// `ai_desc` is prose somebody else wrote. On an ingested entry it is a
/// repository description that came off a foreign wire.
#[test]
fn ai_desc_prose_containing_markup_renders_as_text() {
    let idl = format!(
        "module poisoned {{
           //@ ai_desc: {PAYLOAD}
           interface Widget {{
             //@ ai_effect: destructive
             void wipe();
           }};
         }};"
    );
    let spec = orbweaver_idl::parse(&idl).expect("parses");
    let mut registry = Registry::new();
    registry.load(&spec).expect("loads");

    let view =
        catalog_of(&registry, Exposure::nothing().allow_interface("IDL:poisoned/Widget:1.0"));
    let row = &view.interfaces[0];
    assert_eq!(row.ai_desc.as_deref(), Some(PAYLOAD), "the raw prose reaches the renderer");

    assert_inert(&catalog::render_html(&view), "catalog with a hostile ai_desc");
}

/// An allowlist line an operator pasted from somewhere is data too, and it
/// reaches the page through a different path — the unknown-exposure list.
#[test]
fn an_allowlist_line_containing_markup_renders_as_text() {
    let registry = Registry::new();
    let view = catalog_of(&registry, Exposure::nothing().allow_interface(HOSTILE_ID));
    assert_eq!(view.unknown_exposures, vec![HOSTILE_ID.to_owned()]);
    assert_inert(&catalog::render_html(&view), "catalog with a hostile allowlist line");
}

/// The differ's `what` text is built from repository ids and member names, so
/// an ingested id reaches the diff page the same way.
#[test]
fn a_diff_over_hostile_identifiers_renders_as_text() {
    let mut old = Registry::new();
    old.define_ingested(HOSTILE_ID.to_owned(), Entry::Interface(InterfaceEntry::default()), "peer")
        .expect("registers");
    let new = Registry::new();

    let view = contract::ContractDiff::new(PAYLOAD, PAYLOAD, &old, &new);
    assert!(!view.changes.is_empty(), "removing the interface is a change");
    assert_inert(&contract::render_html(&view), "diff over a hostile id");
}

/// Every one of D004's nine keys carries a value written by something the
/// console does not control, so every one of them is a way in.
#[test]
fn every_trace_field_containing_markup_renders_as_text() {
    let escaped = PAYLOAD.replace('\\', "\\\\").replace('"', "\\\"");
    let mut line = String::from("{");
    for (i, key) in traces::KEYS.iter().enumerate() {
        if i > 0 {
            line.push(',');
        }
        line.push_str(&format!("\"{key}\":\"{escaped}\""));
    }
    line.push('}');

    let mut log = traces::TraceLog::default();
    log.read(PAYLOAD, &line);
    assert_eq!(log.total(), 1, "the line parses: {line}");

    let span = log.spans().next().expect("a span");
    for key in traces::KEYS {
        assert_eq!(span.field(key).text(), Some(PAYLOAD), "{key} reaches the renderer raw");
    }
    // A payload in `decision` is not a decision. It must not be classified as
    // one, and it must not be rendered as a call that happened.
    assert!(!span.decision.real_call());
    assert!(!span.decision.hypothetical());

    assert_inert(&traces::render_html(&log), "traces with hostile fields");
}

/// An unreadable line's diagnostic quotes the parser's message and the file
/// name, both of which are attacker-influenced.
#[test]
fn an_unreadable_line_reports_without_becoming_markup() {
    let mut log = traces::TraceLog::default();
    log.read(PAYLOAD, &format!("{PAYLOAD}\n"));
    assert_eq!(log.unreadable.len(), 1);
    assert_inert(&traces::render_html(&log), "traces with a hostile unreadable line");
}

/// A key outside D004's table is named on the page, and a name is data.
#[test]
fn an_unknown_trace_key_is_named_without_becoming_markup() {
    let mut log = traces::TraceLog::default();
    log.read("f", &format!("{{\"session\":\"s\",\"{}\":\"x\"}}", "<script>k</script>"));
    assert_eq!(log.extra_keys(), vec!["<script>k</script>".to_owned()]);
    assert_inert(&traces::render_html(&log), "traces with a hostile extra key");
}

/// The oracle has to be able to fail, or every test above is decorative.
///
/// A page that pasted the payload in unescaped is built by hand, and the
/// scanner is required to see an element nobody here wrote. The same page also
/// contains the escaped form, so this pins the discrimination rather than a
/// blanket refusal: `&lt;script&gt;` alone must not trip it.
#[test]
fn the_check_can_fail() {
    let clean = html::page("t", html::Markup::text(PAYLOAD));
    assert!(
        tags(&clean).iter().all(|t| OURS.contains(&t.as_str())),
        "an escaped page must not trip the scanner"
    );

    let injected = format!("<main><p>{PAYLOAD}</p></main>&lt;script&gt;");
    let unknown: Vec<String> =
        tags(&injected).into_iter().filter(|t| !OURS.contains(&t.as_str())).collect();
    // Opening and closing `script`, then the `img`: the scanner counts both
    // ends, because a page containing only `</script>` is still a page whose
    // escaping failed.
    assert_eq!(unknown, ["script", "script", "img"], "the scanner missed an injected element");
}

/// The invariant the other tests rest on: there is no way to put a byte in a
/// page except through `Markup::text`, and `Markup::text` escapes. If this ever
/// stops being true the tests above stop being proofs and become samples.
#[test]
fn the_only_door_data_has_into_a_page_escapes() {
    let page = html::page(PAYLOAD, html::Markup::text(PAYLOAD));
    assert_inert(&page, "a page built entirely from the payload");
}

/// A page is one file. An operator reading it offline reads what was written,
/// and nothing on the page reports the catalog to anybody.
#[test]
fn a_rendered_page_fetches_nothing() {
    let mut registry = Registry::new();
    registry
        .define_ingested(HOSTILE_ID.to_owned(), Entry::Interface(InterfaceEntry::default()), "peer")
        .expect("registers");
    let pages = [
        catalog::render_html(&catalog_of(&registry, Exposure::nothing())),
        contract::render_html(&contract::ContractDiff::new("a", "b", &registry, &Registry::new())),
        traces::render_html(&traces::TraceLog::default()),
    ];
    for page in pages {
        for outbound in ["src=", "href=", "@import", "http://", "https://", "url("] {
            assert!(!page.contains(outbound), "a page reaches out: {outbound}");
        }
    }
}
