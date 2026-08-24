//! The catalogue shows what a contract declares.
//!
//! # Why the property is stated over the whole corpus and not over a keyword
//!
//! The batch that added this section was handed one word — *constants* — and
//! the rule underneath it is wider: the page is titled *what exists*, and
//! until 2026-08-24 the only [`orbweaver_registry::Entry`] variant it reached
//! was `Interface`. Measured over `corpus/golden/`, that was **151 of 208
//! registry entries** drawn nowhere, across seven kinds. A test that checked
//! *constants are on the page* would have gone green with structs, unions,
//! enums, exceptions, typedefs, valuetypes and natives still missing, and
//! would then have stayed green the day an eighth kind arrived.
//!
//! So the assertion is a **partition**: every id the registry holds is either
//! an interface row or a declaration row, over every golden file. It cannot be
//! satisfied by a kind somebody remembered, and a new `Entry` variant or a new
//! `TypeCode` construct fails it on the first file that declares one.
//!
//! # 규칙이지 키워드가 아니다
//!
//! 배치가 받은 단어는 "상수"였지만 규칙은 "카탈로그는 계약이 선언한 것을
//! 보여준다"이다. 그래서 검사는 상수 한 종류가 아니라 **분할**을 주장한다:
//! 레지스트리의 모든 id는 인터페이스 행이거나 선언 행이다.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use orbweaver_console::declarations::{self, Value};
use orbweaver_console::{catalog, load};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_idl::include::SearchPath;
use orbweaver_mcp::interceptor::Chain;
use orbweaver_mcp::policy::{Approval, Exposure};
use orbweaver_registry::{ConstValue, Entry, Registry};

fn golden() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/golden");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "idl"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no IDL in {}", root.display());
    files
}

fn registry_of(path: &Path) -> Registry {
    let mut registry = Registry::new();
    load::load_into(&mut registry, path, &SearchPath::new())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    registry
}

fn catalog_of(registry: &Registry) -> catalog::Catalog {
    let exposure = Exposure::nothing();
    let mut chain = Chain::standard(exposure.clone());
    catalog::build(&mut chain, registry, &exposure, None, Approval::default())
}

/// The property: nothing a contract declares is invisible to the one surface a
/// human reads.
///
/// Stated as a partition of `registry.ids()` rather than as a count, because a
/// count is satisfied by the wrong rows adding up.
#[test]
fn every_entry_in_the_registry_reaches_a_row() {
    let mut missing: Vec<String> = Vec::new();
    let mut entries = 0usize;
    let mut declared = 0usize;
    for path in golden() {
        let registry = registry_of(&path);
        let view = catalog_of(&registry);
        let on_page: BTreeSet<&str> = view
            .interfaces
            .iter()
            .map(|i| i.id.as_str())
            .chain(view.declarations.iter().map(|d| d.id.as_str()))
            .collect();
        for id in registry.ids() {
            entries += 1;
            if !matches!(registry.get(id), Some(Entry::Interface(_))) {
                declared += 1;
            }
            if !on_page.contains(id.as_str()) {
                missing.push(format!("{}: {id}", path.display()));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "{} of {entries} registry entries reach no row on the catalogue page:\n{}",
        missing.len(),
        missing.join("\n")
    );
    // The batch's own measurement, kept as a floor rather than as a comment:
    // 151 non-interface entries across the golden corpus on 2026-08-24. A
    // floor and not an equality, because the corpus grows and this test is not
    // the place a new golden file has to be registered.
    assert!(
        declared >= 151,
        "the golden corpus held 151 non-interface entries when this landed; it now holds \
         {declared}, so either the corpus shrank or the walk stopped seeing them"
    );
}

/// Every `TypeCode` construct the golden corpus derives is spelled by
/// something other than the fallback.
///
/// The second green-while-empty class: two substrings are not a rule. This
/// walks every entry's own `TypeCode` and asserts the keyword column is never
/// the `"type"` catch-all — the arm that exists only for a shape an ingested
/// peer might send, and which a missing arm would silently fall into.
#[test]
fn no_declared_construct_falls_into_the_catch_all_keyword() {
    let mut fell: Vec<String> = Vec::new();
    for path in golden() {
        let registry = registry_of(&path);
        for row in declarations::collect(&registry) {
            if row.keyword == "type" {
                fell.push(format!("{}: {} is drawn as the catch-all", path.display(), row.id));
            }
        }
    }
    assert!(fell.is_empty(), "{}", fell.join("\n"));
}

/// Every keyword arm is executed, including the four the golden corpus cannot
/// reach.
///
/// The first green-while-empty class: the layer under test never reaches the
/// code. Four arms of `keyword` are unreachable from IDL — a `#!(no such
/// thing)` — because [`orbweaver_registry`] turns an `interface` declaration
/// into `Entry::Interface` and never into `Entry::Type(ObjRef)`, so an
/// `ObjRef`, an `AbstractInterface` or an abstract `Value` only ever arrives as
/// a **registry entry** off a remote Interface Repository. `Registry::ingest`
/// is that route, and `define_ingested` is how it gets there, so that is how
/// they are exercised here. The test above cannot reach them and would stay
/// green with all four spelling the catch-all.
#[test]
fn the_keyword_arms_no_idl_can_reach_are_exercised_through_ingestion() {
    let named = |name: &str| (format!("IDL:remote/{name}:1.0"), name.to_owned());
    let cases: Vec<(String, TypeCode, &str)> = vec![
        {
            let (id, name) = named("Ref");
            (id.clone(), TypeCode::ObjRef { id, name }, "interface")
        },
        {
            let (id, name) = named("Abs");
            (id.clone(), TypeCode::AbstractInterface { id, name }, "abstract interface")
        },
        {
            let (id, name) = named("AbsVal");
            (
                id.clone(),
                TypeCode::Value { id, name, modifier: 2, base: None, members: Vec::new() },
                "abstract valuetype",
            )
        },
        {
            let (id, name) = named("Val");
            (
                id.clone(),
                TypeCode::Value { id, name, modifier: 0, base: None, members: Vec::new() },
                "valuetype",
            )
        },
    ];
    let mut registry = Registry::new();
    for (id, tc, _) in &cases {
        registry
            .define_ingested(id.clone(), Entry::Type(tc.clone()), "a peer's IFR")
            .expect("a peer's description enters the registry");
    }
    let rows = declarations::collect(&registry);
    for (id, _, keyword) in &cases {
        let row = rows.iter().find(|r| &r.id == id).unwrap_or_else(|| panic!("{id} has no row"));
        assert_eq!(&row.keyword, keyword, "{id}");
        assert!(row.ingested(), "{id} is a peer's description and says so");
    }
}

/// The anti-drift pin: the console and the §5.3 differ spell one value one way.
///
/// Read out of the differ's **own** change text rather than out of a second
/// copy of its rules, so the day somebody changes either renderer this goes
/// red instead of a release note and a catalogue row quietly disagreeing about
/// the same number.
#[test]
fn the_page_spells_a_value_the_way_the_differ_spells_it() {
    // One of each shape the two renderers have an opinion about, `fixed` first
    // — it is the one with two plausible spellings and the one the constant
    // batch made exact.
    let cases = [
        ("fixed", "9.9d", "9.91d"),
        ("fixed", "0.001d", "-0.001d"),
        ("unsigned long long", "1", "18446744073709551615"),
        ("double", "1.5", "2.25"),
        ("string", "\"a\"", "\"b\""),
        ("boolean", "TRUE", "FALSE"),
        ("short", "-32768", "32767"),
    ];
    for (ty, old_literal, new_literal) in cases {
        let source = |literal: &str| format!("module d {{ const {ty} K = {literal}; }};");
        let load = |text: &str| {
            let mut r = Registry::new();
            r.load(&orbweaver_idl::parse(text).expect("parses")).expect("loads");
            r
        };
        let old = load(&source(old_literal));
        let new = load(&source(new_literal));

        let changes = orbweaver_registry::diff::diff(&old, &new);
        let change = changes
            .iter()
            .find(|c| c.what.starts_with("constant value changed"))
            .unwrap_or_else(|| panic!("{ty} {old_literal} -> {new_literal}: {changes:?}"));

        let rows = declarations::collect(&new);
        let row = rows.iter().find(|r| r.id == "IDL:d/K:1.0").expect("the constant row");
        let Value::Folded(spelled) = &row.value else { panic!("{ty} {new_literal} did not fold") };

        // The differ's sentence ends "... to <value>". If the console spells
        // the same value differently, the suffix does not match.
        assert!(
            change.what.ends_with(&format!(" to {spelled}")),
            "the differ says {:?} and the page says {spelled:?}",
            change.what
        );
    }
}

/// A constant the registry could not fold says so, and shows no number.
///
/// `Entry::Const { value: None }` exists so that nothing downstream invents a
/// plausible wrong one, and a page is downstream. `const octet O = 300;` is
/// out of range, which is one of the three documented ways to get there.
#[test]
fn an_unevaluated_constant_says_so_rather_than_showing_a_number() {
    let mut registry = Registry::new();
    registry
        .load(&orbweaver_idl::parse("module d { const octet O = 300; };").expect("parses"))
        .expect("loads");
    assert!(
        matches!(registry.get("IDL:d/O:1.0"), Some(Entry::Const { value: None, .. })),
        "the fixture needs a constant the registry refuses to fold"
    );

    let rows = declarations::collect(&registry);
    let row = rows.iter().find(|r| r.id == "IDL:d/O:1.0").expect("the constant row");
    assert_eq!(row.value, Value::Unevaluated);

    let view = catalog_of(&registry);
    let text = catalog::render_text(&view);
    assert!(text.contains("could not evaluate"), "{text}");
    // Not "0", and not "300" either: neither is a value this registry holds.
    assert!(!text.contains("value: 0\n"), "a guessed zero reached the page:\n{text}");
    assert!(!text.contains("value: 300"), "a truncated value reached the page:\n{text}");
}

/// The file the batch was pointed at, end to end through the binary's own
/// rendering: 22 constants and a union, none of which had a row.
#[test]
fn the_const_corpus_renders_every_constant_with_its_exact_value() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/golden/33-const-values.idl")
        .canonicalize()
        .expect("the corpus file");
    let registry = registry_of(&path);
    let view = catalog_of(&registry);

    assert!(view.interfaces.is_empty(), "this file declares no interface, which is the point");
    assert_eq!(view.declarations.len(), 23, "22 constants and one union");

    let text = catalog::render_text(&view);
    // A page with no interface used to say "the catalog is empty" over these.
    assert!(!text.contains("the catalog is empty"), "{text}");

    // The values the constant batch made exact, and which no `f64` holds. A
    // page that routed one through a binary float prints
    // 9.9000000000000003552713678800500929355621337890625 or 9.9000000000000004
    // — so the assertion is the whole line, not a prefix.
    for (id, value) in [
        ("IDL:gc31/TAX_RATE:1.0", "9.9"),
        ("IDL:gc31/UNIT_PRICE:1.0", "1.005"),
        ("IDL:gc31/EPSILON:1.0", "-0.001"),
        ("IDL:gc31/DERIVED:1.0", "99999.98"),
        ("IDL:gc31/WIDEST:1.0", "1234567890123456789012345678901"),
        ("IDL:gc31/ULL_MAX:1.0", "18446744073709551615"),
        ("IDL:gc31/ULL_MAX_HEX:1.0", "18446744073709551615"),
        ("IDL:gc31/PERMISSIONS:1.0", "493"),
        ("IDL:gc31/S_MIN:1.0", "-32768"),
        ("IDL:gc31/WITHIN_AFTER_FOLDING:1.0", "30000"),
        ("IDL:gc31/CAPTION:1.0", "\"balance\""),
    ] {
        let row = view
            .declarations
            .iter()
            .find(|d| d.id == id)
            .unwrap_or_else(|| panic!("{id} has no row"));
        assert_eq!(row.value, Value::Folded(value.to_owned()), "{id}");
        assert!(text.contains(&format!("value: {value}\n")), "{id} is not on the page:\n{text}");
    }

    // `const fixed` declares no precision, so the type column says `fixed` and
    // not `fixed<0,0>` — a declared precision of nothing is not IDL anybody
    // can write.
    let tax = view.declarations.iter().find(|d| d.id == "IDL:gc31/TAX_RATE:1.0").expect("row");
    assert_eq!(tax.declared, "fixed");

    // The union in the same file: its branches are named, and the page says in
    // words that the labels are not spelled rather than leaving the gap.
    let wide = view.declarations.iter().find(|d| d.id == "IDL:gc31/Wide:1.0").expect("row");
    assert_eq!(wide.keyword, "union");
    assert_eq!(wide.members, ["case → long saturated", "default → short ordinary"]);
    assert!(wide.note.as_ref().is_some_and(|n| n.contains("unsigned long long")), "{wide:?}");
}

/// A struct's members, a typedef's aliased type and an enum's enumerators are
/// on the page — the neighbours of the keyword the batch was handed.
#[test]
fn a_types_own_shape_is_on_the_page() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/golden/19-realistic-service.idl")
        .canonicalize()
        .expect("the corpus file");
    let rows = declarations::collect(&registry_of(&path));
    let row = |id: &str| {
        rows.iter().find(|r| r.id == id).unwrap_or_else(|| panic!("{id} has no row")).clone()
    };

    let track = row("IDL:tms/Track:1.0");
    assert_eq!(track.keyword, "struct");
    assert_eq!(
        track.members,
        [
            "long id",
            "TrackClass klass",
            "Position pos",
            "double course",
            "double speed",
            "string designation",
        ]
    );

    let seq = row("IDL:tms/TrackSeq:1.0");
    assert_eq!(seq.keyword, "typedef");
    assert_eq!(seq.declared, "sequence<Track>");

    let class = row("IDL:tms/TrackClass:1.0");
    assert_eq!(class.keyword, "enum");
    assert_eq!(class.members, ["UNKNOWN", "SURFACE", "AIR", "SUBSURFACE"]);

    let fault = row("IDL:tms/NoSuchTrack:1.0");
    assert_eq!(fault.keyword, "exception");
    assert_eq!(fault.members, ["long id"]);
}

/// A `native` reaches the page, with the sentence that says why nothing can
/// marshal it.
///
/// [`TypeCode::Native`]'s own documentation says "the catalogue can draw it".
/// It could not, for the three days between that sentence being written and
/// this test.
#[test]
fn a_native_is_drawn_with_the_reason_nothing_marshals_it() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/golden/31-native-type.idl")
        .canonicalize()
        .expect("the corpus file");
    let rows = declarations::collect(&registry_of(&path));
    let row = rows.iter().find(|r| r.id == "IDL:gn31/Handle:1.0").expect("the native row");
    assert_eq!(row.keyword, "native");
    assert!(row.note.as_ref().is_some_and(|n| n.contains("no CDR encoding")), "{row:?}");
}

/// Spelling a type that contains itself terminates, and stops at the name.
///
/// `corpus/golden/15` is a struct holding a sequence of itself. Expanding a
/// named member type would not terminate; the page stops at the name, which is
/// also what a reader is looking for.
#[test]
fn a_recursive_type_is_spelled_by_name_rather_than_expanded() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/golden/15-forward-recursive.idl")
        .canonicalize()
        .expect("the corpus file");
    let registry = registry_of(&path);
    let rows = declarations::collect(&registry);
    let tree = rows.iter().find(|r| r.id == "IDL:gc15/Tree:1.0").expect("the recursive struct");
    assert_eq!(tree.members, ["string label", "TreeSeq kids"]);

    // And directly, on the shape the registry produces for an indirection.
    assert_eq!(
        declarations::spell(&TypeCode::Recursive("IDL:gc15/Tree:1.0".to_owned())),
        "(recursive: IDL:gc15/Tree:1.0)"
    );
}

/// The declarations table is data a peer can have written, and goes through
/// the same escaping every other field does.
///
/// An Interface Repository serves types as well as interfaces, so a hostile id
/// arrives on this table by exactly the route `tests/escaping.rs` documents
/// for the interface cards — and this table did not exist when that test was
/// written.
#[test]
fn a_hostile_ingested_type_renders_inert() {
    const HOSTILE: &str = r#"IDL:evil/<script>alert("pwned")</script>:1.0"#;
    let mut registry = Registry::new();
    registry
        .define_ingested(
            HOSTILE.to_owned(),
            Entry::Type(TypeCode::Struct {
                id: HOSTILE.to_owned(),
                name: r#"<img src=x onerror=alert(1)>"#.to_owned(),
                members: vec![orbweaver_giop::typecode::Member {
                    name: r#"</td><script>alert(2)</script>"#.to_owned(),
                    tc: TypeCode::Long,
                }],
            }),
            r#"<script>alert("source")</script>"#,
        )
        .expect("a peer's description enters the registry");
    registry
        .define_ingested(
            "IDL:evil/K:1.0".to_owned(),
            Entry::Const {
                tc: TypeCode::String(0),
                value: Some(ConstValue::Str(r#"</span><script>alert(3)</script>"#.to_owned())),
            },
            "peer",
        )
        .expect("a peer's constant enters the registry");

    let view = catalog_of(&registry);
    assert_eq!(view.declarations.len(), 2, "both entries are on the page");
    assert!(view.declarations.iter().all(|d| d.ingested()), "both are marked as a peer's");

    let page = catalog::render_html(&view);
    // The same structural check `tests/escaping.rs` argues for: no element on
    // the page is one this crate did not write.
    for tag in tags(&page) {
        assert!(OURS.contains(&tag.as_str()), "an element nobody here wrote: <{tag}>");
    }
    assert!(page.contains("&lt;script&gt;"), "the payload was dropped, not escaped");
    assert!(page.contains("ingested from"), "provenance survives the escaping");
}

/// Kept in step with `tests/escaping.rs` by hand and by the assertion above:
/// every element name this crate writes.
const OURS: [&str; 18] = [
    "html", "head", "meta", "title", "style", "body", "main", "h1", "h2", "p", "div", "span",
    "table", "tr", "th", "td", "b", "footer",
];

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
