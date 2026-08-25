//! One home for a wire refusal — **across crates**, which is where the pin
//! that already existed stopped.
//!
//! Five constructs cannot go on this wire, and they are three families rather
//! than one: `docs/PLAN.md` §4.4 defers `valuetype`, abstract interfaces and
//! `fixed` — a wire form the specification defines that this version has not
//! implemented — while `native X;` has no wire form to implement, in v1 or in
//! any later version, and `::CORBA::Principal` had one that the specification
//! **took back** (GIOP 1.0 carried it in every request header; CORBA 3.0
//! removed the type). `orbweaver-dynamic` owns one head per family
//! ([`orbweaver_dynamic::deferred_wire_head`],
//! [`orbweaver_dynamic::unmarshallable_wire_head`],
//! [`orbweaver_dynamic::withdrawn_wire_head`]) and every layer's tail is
//! its own, because "the property did not measure it", "the generator skipped
//! it" and "your document stops here" are three different things to say about
//! one fact.
//!
//! # The fifth family, and why this file needed a second kind of assertion
//!
//! The `Principal` family landed on 2026-08-26, and it arrived through a hole
//! this file could not see. Every test below classifies a sentence *first* —
//! "is this one about a wire family?" — and only then demands it read a
//! published head. Until that day no head existed for a `Principal`, so every
//! layer that met one wrote its own sentence, and none of those sentences
//! looked like a wire refusal to the classifier: `orbweaver-gen` said `"no
//! static mapping for Principal"`, its Python half `"no AnyJSON form for
//! Principal"`, `prop.rs` two more. **A classifier keyed on the sentences that
//! exist cannot see a family whose sentence does not exist yet**, and that is
//! not a defect in the classifier — it is the reason a rule about wording needs
//! a second rule about coverage.
//!
//! So [`every_layer_that_meets_one_reads_a_head`] asserts the other direction:
//! over a fixture whose every declaration reaches one family, every skip both
//! emitters report **must** read a head. Nothing is classified; the fixture is
//! the classification. It is the assertion that goes red the day a sixth
//! construct the wire cannot carry gets an arm in the type mapper and no head
//! of its own.
//!
//! *분류 후 단언은 아직 존재하지 않는 문장을 볼 수 없다. 그래서 반대 방향의
//! 단언이 하나 더 있다 — 고정된 픽스처의 모든 스킵은 공표된 머리를 읽어야 한다.*
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

use orbweaver_dynamic::{deferred_wire_head, unmarshallable_wire_head, withdrawn_wire_head};
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
    // The fifth, and the only one with no `declaration` line: nothing declares
    // `::CORBA::Principal`, the front end predeclares it. So the fixture
    // reaches it the three ways a contract can — a member, a return, a typedef
    // — and the "bare declaration" row of the shape above is absent because
    // there is no such row to write.
    (
        "principal",
        r#"module w5 {
             typedef ::CORBA::Principal Caller;
             struct Held { ::CORBA::Principal who; };
             interface Broker { ::CORBA::Principal whoami(); };
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
    if let Some(what) = orbweaver_dynamic::unmarshallable_wire_name(tc) {
        return Some(unmarshallable_wire_head(&what));
    }
    orbweaver_dynamic::withdrawn_wire_name(tc).map(|what| withdrawn_wire_head(&what))
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

/// The subject-independent part of a head — what every sentence built from it
/// shares, whatever construct it is about. Taken by calling the head with a
/// sentinel, so it is the function's wording and not a copy of it.
fn marker(head: fn(&str) -> String) -> String {
    const SENTINEL: &str = "\u{0}";
    head(SENTINEL).replace(SENTINEL, "")
}

/// A sentence that talks about one of the five must contain that family's
/// head. Anything else — a constant with no `const` form, a recursive type, a
/// sampler gap — is not this rule's business and is left alone.
///
/// # What this classifier can and cannot see
///
/// The three computed markers catch the drift that actually happens: a layer
/// that keeps the head's *wording* and rebuilds the **subject** its own way.
/// That is the 2026-08-25 defect (`abstract interface Describable` versus
/// `abstract interface gc20::Describable` — one fact, two spellings, a reader
/// grepping either finding half the layers), and it is invisible to a check
/// that only asks whether both mention §4.4.
///
/// The two literals beside them are kept, not replaced: they catch a stray that
/// names the section in wording of its own, which no marker can match.
///
/// What none of the five can see is a sentence with **nothing in common with
/// any head** — which is precisely the state every `Principal` refusal was in
/// until the fifth head existed. That gap is covered by
/// [`every_layer_that_meets_one_reads_a_head`], which classifies by fixture
/// rather than by sentence, and this comment is here so the next person does
/// not tighten the classifier and believe the gap closed.
fn is_about_a_wire_family(sentence: &str) -> bool {
    sentence.contains("§4.4")
        || sentence.contains("no wire form at all")
        || [
            marker(deferred_wire_head),
            marker(unmarshallable_wire_head),
            marker(withdrawn_wire_head),
        ]
        .iter()
        .any(|m| sentence.contains(m.as_str()))
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

/// **Every** skip either emitter reports over a fixture whose whole content
/// reaches one family reads a published head — no classification, because the
/// fixture is the classification.
///
/// This is the assertion the file was missing, and the shape of what it missed
/// is worth keeping. `the_generator_does_not_write_its_own_wire_refusal` above
/// asks each sentence whether it is about a wire family *before* demanding a
/// head, so a family with no head yet answers "no" to that question in every
/// layer at once and the whole file goes quiet about it. Measured 2026-08-26:
/// five `gp34` declarations were skipped by both emitters with `"no static
/// mapping for Principal"` and `"no AnyJSON form for Principal"`, S4 named none
/// of them, and `deferred_wire_agreement`'s two sets agreed **because both were
/// empty**. Three gates green over one construct the wire cannot carry.
///
/// A fixture that reaches nothing else cannot answer "no" that way: every skip
/// in it is caused by the family, so every skip owes a head. The day a sixth
/// construct gets an arm in the type mapper and no head of its own, this test
/// is what says so — provided its fixture is added to `FAMILIES`, which is the
/// one thing still done by hand and is why the list is at the top of the file
/// where it can be seen.
#[test]
fn every_layer_that_meets_one_reads_a_head() {
    for (family, src) in FAMILIES {
        let registry = registry_of(src);
        let heads = heads_over(&registry);
        assert!(!heads.is_empty(), "{family}: the fixture reaches no wire family");
        for (target, skipped) in [
            ("rust", emit(&registry, "g").skipped),
            ("python", orbweaver_gen::python::emit_python(&registry, "g").skipped),
        ] {
            assert!(
                !skipped.is_empty(),
                "{family}: the {target} emitter skipped nothing, so this fixture measures nothing"
            );
            for (id, why) in &skipped {
                assert!(
                    heads.iter().any(|h| why.contains(h.as_str())),
                    "{target} skips {id} for a {family} without reading a published head:\n  \
                     {why}\nit must contain one of:\n{}",
                    heads.iter().map(|h| format!("  {h}\n")).collect::<String>()
                );
            }
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

/// The three heads are different sentences and must stay different: swapping
/// any two is the substitution that reads as correct and tells the reader the
/// opposite of the truth about whether to wait for a later version.
///
/// The third is not a spare copy of the second. Both say "no later version
/// carries this", and they say it for opposite reasons a contract author has to
/// act on differently: a `native` never had a wire form, so the fix is to
/// declare in IDL what the language type actually contains, while a `Principal`
/// **had** one — every GIOP 1.0 request header carried it — so the author is
/// not being told they modelled something wrong, they are being told the
/// specification moved caller identity somewhere else.
#[test]
fn the_three_families_do_not_share_a_head() {
    let deferred = deferred_wire_head("X");
    let native = unmarshallable_wire_head("X");
    let withdrawn = withdrawn_wire_head("X");
    assert_ne!(deferred, native);
    assert_ne!(deferred, withdrawn);
    assert_ne!(native, withdrawn);
    assert!(
        deferred.contains("§4.4"),
        "the deferred head must name the section it defers under: {deferred}"
    );
    assert!(!native.contains("yet"), "a native's head must not promise a later version: {native}");
    assert!(
        !withdrawn.contains("yet"),
        "a withdrawn type's head must not promise a later version: {withdrawn}"
    );
    // The one thing the third head must *not* borrow from the first. §4.4 is
    // the list of what this project owes; a type the OMG removed is not on it,
    // and a head that named the section would send the reader to a plan entry
    // that will never mention `Principal`. The *sentence* names it — to deny
    // it — and that denial is pinned in `orbweaver-dynamic`'s own tests.
    assert!(
        !withdrawn.contains("§4.4"),
        "a withdrawn type is not deferred under any section: {withdrawn}"
    );
}
