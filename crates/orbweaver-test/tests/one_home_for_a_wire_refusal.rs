//! One home for a wire refusal — **across crates**, which is where the pin
//! that already existed stopped.
//!
//! Four constructs cannot go on this wire, and they are two families rather
//! than one: `docs/PLAN.md` §4.4 defers `valuetype`, abstract interfaces and
//! `fixed` — a wire form the specification defines that this version has not
//! implemented — while `native X;` has no wire form to implement, in v1 or in
//! any later version. `orbweaver-dynamic` owns one head per family
//! ([`orbweaver_dynamic::deferred_wire_head`],
//! [`orbweaver_dynamic::unmarshallable_wire_head`]) and every layer's tail is
//! its own, because "the property did not measure it", "the generator skipped
//! it" and "your document stops here" are three different things to say about
//! one fact.
//!
//! # Why this file is in `orbweaver-test` and not beside the heads
//!
//! `orbweaver-dynamic`'s own `deferred_sentence_agreement` holds the layers
//! *inside that crate* equal, and `orbweaver-gen`'s `python_target` holds the
//! generated Python runtime equal to them across a crate boundary. Neither
//! could see the two Rust crates downstream, because until 2026-08-24 the
//! heads were `pub(crate)` — so a layer outside could not call them and had to
//! write its own sentence, and the pin could not read the sentence it wrote.
//!
//! Measured that day, by running each layer rather than reading it: **twelve
//! literals in two crates for the four facts the heads own** — `orbweaver-gen`
//! four (`deferred_value`, `deferred_fixed`, `deferred_abstract`,
//! `unmarshallable_native`) and `orbweaver-test` eight (`json_unmapped` and
//! `why_unsupported`, four families each). One of the twelve had already gone
//! false: `prop.rs` told a contract-check reader that `from_json` answers
//! `"cannot cross yet"` for a `fixed`, and that layer stopped saying it on
//! 2026-08-21 when its deferred arm landed. Nothing was red, because the
//! fact's scope is the workspace and the pin's scope was a crate.
//!
//! *네 구성체, 두 계열. 문장의 머리는 한 곳에 있고 꼬리는 계층마다 다르다 — 각
//! 계층이 독자에게 말해야 하는 것이 서로 다르기 때문이다. 이 파일이 크레이트
//! 경계를 넘어 그 머리를 고정한다.*

use std::collections::BTreeSet;

use orbweaver_dynamic::{deferred_wire_head, unmarshallable_wire_head};
use orbweaver_gen::emit;
use orbweaver_giop::typecode::TypeCode;
use orbweaver_registry::Registry;

/// One file per family, each written so the family is reached through a member,
/// a signature and a bare declaration — the three ways a layer meets one.
const FAMILIES: &[(&str, &str)] = &[
    (
        "valuetype",
        r#"module w1 {
             valuetype Money { public long units; };
             struct Held { Money amount; };
             interface Wallet { Money balance(); };
           };"#,
    ),
    (
        "abstract interface",
        r#"module w2 {
             abstract interface Describable { string label(); };
             struct Held { Describable it; };
             interface Registrar { Describable lookup(); };
           };"#,
    ),
    (
        "fixed",
        r#"module w3 {
             typedef fixed<9,2> Amount;
             struct Held { Amount total; };
             interface Billing { Amount sum(); };
           };"#,
    ),
    (
        "native",
        r#"module w4 {
             native Handle;
             struct Held { Handle token; };
             interface Broker { Handle acquire(); };
           };"#,
    ),
];

fn registry_of(src: &str) -> Registry {
    let spec = orbweaver_idl::check(src).expect("the fixture checks out");
    let mut registry = Registry::new();
    registry.load(&spec).expect("the fixture loads");
    registry
}

/// The head a sentence about `tc` must start with, or `None` for a type the
/// wire does carry — asked of the same crate that writes the head, so the
/// classifier and the wording cannot drift apart.
fn required_head(tc: &TypeCode) -> Option<String> {
    if let Some(what) = orbweaver_dynamic::deferred_wire_name(tc) {
        return Some(deferred_wire_head(&what));
    }
    orbweaver_dynamic::unmarshallable_wire_name(tc).map(|what| unmarshallable_wire_head(&what))
}

/// Every head the two families can produce over the fixtures above, as whole
/// strings — what a layer's sentence has to begin with, whichever of the four
/// it is talking about.
fn heads_over(registry: &Registry) -> BTreeSet<String> {
    let mut heads = BTreeSet::new();
    let ids: Vec<String> = registry.ids().map(|i| i.to_string()).collect();
    for id in ids {
        if let Some(tc) = registry.typecode(&id) {
            collect_heads(tc, &mut heads, 0);
        }
    }
    heads
}

fn collect_heads(tc: &TypeCode, out: &mut BTreeSet<String>, depth: usize) {
    if depth > 8 {
        return;
    }
    if let Some(h) = required_head(tc) {
        out.insert(h);
    }
    match tc {
        TypeCode::Alias { aliased, .. } => collect_heads(aliased, out, depth + 1),
        TypeCode::Sequence { element, .. } | TypeCode::Array { element, .. } => {
            collect_heads(element, out, depth + 1)
        }
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
            for m in members {
                collect_heads(&m.tc, out, depth + 1);
            }
        }
        TypeCode::Union { discriminator, cases, .. } => {
            collect_heads(discriminator, out, depth + 1);
            for c in cases {
                collect_heads(&c.tc, out, depth + 1);
            }
        }
        _ => {}
    }
}

/// A sentence that talks about one of the four must open with that family's
/// head. Anything else — a constant with no `const` form, a recursive type, a
/// sampler gap — is not this rule's business and is left alone.
fn is_about_a_wire_family(sentence: &str) -> bool {
    sentence.contains("§4.4") || sentence.contains("no wire form at all")
}

/// Where the force comes from: `heads` is **computed** by calling the same
/// functions the layers are supposed to call, so a layer that keeps a literal
/// passes today and fails the moment the head is reworded — which is the only
/// event this rule exists to survive. `contains` rather than `starts_with`
/// because every layer prefixes its own subject (`member amount: …`, `Held is
/// not taken across AnyJSON …: …`), and that prefix is the layer's own job.
fn assert_reads_a_head(layer: &str, family: &str, sentence: &str, heads: &BTreeSet<String>) {
    if !is_about_a_wire_family(sentence) {
        return;
    }
    assert!(
        heads.iter().any(|h| sentence.contains(h.as_str())),
        "{layer} writes its own sentence for a {family}:\n  {sentence}\n\
         it must read one of the heads orbweaver-dynamic publishes:\n{}",
        heads.iter().map(|h| format!("  {h}\n")).collect::<String>()
    );
}

/// The generator's skip reasons, both targets, all four families.
#[test]
fn the_generator_does_not_write_its_own_wire_refusal() {
    for (family, src) in FAMILIES {
        let registry = registry_of(src);
        let heads = heads_over(&registry);
        assert!(!heads.is_empty(), "{family}: the fixture reaches no wire family");
        for (id, why) in &emit(&registry, "g").skipped {
            assert_reads_a_head(&format!("orbweaver-gen (rust, {id})"), family, why, &heads);
        }
        for (id, why) in &orbweaver_gen::python::emit_python(&registry, "g").skipped {
            assert_reads_a_head(&format!("orbweaver-gen (python, {id})"), family, why, &heads);
        }
    }
}

/// The property's two limits — "not sampled" and "not taken across AnyJSON" —
/// are the pair that had drifted, and the AnyJSON one is what a contract-check
/// reader is shown.
#[test]
fn the_property_does_not_write_its_own_wire_refusal() {
    for (family, src) in FAMILIES {
        let registry = registry_of(src);
        let heads = heads_over(&registry);
        let report = orbweaver_test::check(&registry, 4);
        for finding in &report.findings {
            assert_reads_a_head(
                &format!("orbweaver-test property ({})", finding.rule),
                family,
                &finding.message,
                &heads,
            );
        }
    }
}

/// The **count** is classified the same way the sentence is written — by asking
/// the crate that owns the heads, not by matching a fragment of one.
///
/// This is the shape the batch that wrote this file was caught by. Until
/// 2026-08-24 `deferred_wire_gaps` filtered on the literal `"§4.4"`, and the
/// four families' count included a `native` only because that sentence
/// mentioned the section in order to say it does not apply. Moving the wording
/// into the shared head — which names no section, correctly — took the count
/// from 18 to 14 with nothing else changed. The harness caught it; nothing in
/// the test suite did.
///
/// What makes this test load-bearing rather than decorative: a `native`
/// finding's message contains **no** `§4.4` at all, so a classifier that looks
/// for one cannot see it.
#[test]
fn a_native_is_counted_though_its_sentence_names_no_section() {
    let (_, src) = FAMILIES.iter().find(|(f, _)| *f == "native").expect("the native fixture");
    let registry = registry_of(src);
    let report = orbweaver_test::check(&registry, 4);

    let cited: Vec<&str> = report
        .findings
        .iter()
        .filter(|f| f.message.contains("§4.4"))
        .map(|f| f.message.as_str())
        .collect();
    assert!(
        cited.is_empty(),
        "a native's refusal must not name the section it is not deferred under: {cited:?}"
    );

    let gaps = orbweaver_test::deferred_wire_gaps(&report);
    assert!(
        !gaps.is_empty(),
        "the native declarations are the whole content of this fixture and none was counted; \
         the classifier is reading a fragment of a sentence rather than asking for the head"
    );
}

/// The two heads are different sentences and must stay different: swapping
/// them is the one substitution that reads as correct and tells the reader the
/// opposite of the truth about whether to wait for a later version.
#[test]
fn the_two_families_do_not_share_a_head() {
    let deferred = deferred_wire_head("X");
    let native = unmarshallable_wire_head("X");
    assert_ne!(deferred, native);
    assert!(
        deferred.contains("§4.4"),
        "the deferred head must name the section it defers under: {deferred}"
    );
    assert!(!native.contains("yet"), "a native's head must not promise a later version: {native}");
}
