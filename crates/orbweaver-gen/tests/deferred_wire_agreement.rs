//! The S4 rule and the generator's refusal name the same set — measured, not
//! assumed, over the golden corpus and over every shape the rule knows.
//!
//! `docs/PLAN.md` §4.4 defers `valuetype`, abstract interfaces and `fixed`.
//! **The set this test holds equal is five families, not three.** The fourth
//! is `native X;`, and the difference is not pedantry: §4.4's three have a
//! wire form the specification defines and this version does not implement,
//! while a native has none to implement in any version. It was missing from
//! both sides for exactly that reason, inverted — because §4.4 did not name
//! it, the rule did not, so the generator could not refuse it without
//! breaking this test, so the registry kept recording it as
//! `TypeCode::ObjRef` and both emitters emitted an object reference: an IOR
//! on the wire where nothing at all should go. The previous batch fixed the
//! same wrong answer for `valuetype` and abstract interfaces and left this
//! one with an honest note ("no rule names it, so a change there would be a
//! claim no gate checks"). The fix for that was to make the rule name it.
//!
//! `ValueBase` is the same defect in the one spelling that has no declaration
//! behind it: the keyword names the abstract base of every valuetype, the
//! rule named it the whole time, and the registry mapped it to an object
//! reference — so the generators skipped nothing and nothing was red. It is
//! not a fifth family; it is a valuetype, and it is here because until
//! `corpus/golden/32` no corpus file wrote the keyword, so the two sets were
//! never compared over one.
//!
//! The fifth is `::CORBA::Principal`, and it is the family this test was
//! **green over** for as long as it had existed. Both sides were empty: the
//! rule did not name it (it has no `Definition` to become a finding) and the
//! emitters refused it out of their own catch-alls, whose sentences carry no
//! published head — and `sets()` below reads the heads to tell a wire refusal
//! from an ordinary skip, so it filtered those skips out. Two empty sets are
//! equal. **An equality between two things computed by the same filter proves
//! nothing about what the filter cannot see**, which is why the count on the
//! set below is asserted as a list and not as a number, and why
//! `orbweaver-test`'s `one_home_for_a_wire_refusal` grew a fixture-shaped
//! assertion in the same batch.
//!
//! Two places compute what that costs a contract: the front end's
//! [`orbweaver_idl::deferred_wire_types`], which S4 reports (`wire/deferred-type`,
//! a warning by default and a refusal under `--wire v1`), and this crate's
//! `representable` closure, which skips an item with the section named. They
//! were written independently, from the AST and from the registry's `TypeCode`s
//! respectively, and a contract that one refuses and the other serves is a
//! contract that passed the gate and then failed generation — the seam this
//! test exists to close.
//!
//! # What agreement means here, exactly
//!
//! **The two sets are equal.** Every id the generator skips for §4.4 is a
//! declaration the rule names, and every declaration the rule names is one the
//! generator skips: nothing the generator will refuse gets past S4's refusing
//! form, and nothing S4 refuses is quietly generated anyway.
//!
//! It was not always equal, and what the surplus was is worth keeping. Until
//! 2026-08-20 the generator's closure followed `TypeCode::Fixed` and nothing
//! else, because the registry recorded a `valuetype` and an abstract interface
//! as `TypeCode::ObjRef` — "so `_is_a` and the catalogue keep working" — and
//! both emitters therefore emitted a **reference** for them and skipped
//! nothing. `gc20::Wallet::balance()` generated as returning an object
//! reference where a conformant peer sends a value. The rule named all four of
//! golden 20's declarations and the generator named none; the surplus was
//! pinned here as the exact set `{gc20::Describable, Money, Named, Wallet,
//! gcdr::Describable, Memo, Note, Registrar}` rather than hidden by a looser
//! assertion, and that pin is what this batch had to delete. The registry now
//! records `TypeCode::Value` and `TypeCode::AbstractInterface`, which nothing
//! can mistake for a reference, and the surplus is empty.
//!
//! *두 집합은 같다. 예전에는 생성기가 `fixed`만 거부하고 valuetype은 객체
//! 참조로 내보냈다 — 레지스트리가 둘 다 `ObjRef`로 기록했기 때문이다. 그
//! 차이를 느슨한 단언으로 감추지 않고 집합으로 고정해 두었고, 이번 배치가
//! 그 고정을 지웠다.*

use std::collections::BTreeSet;
use std::path::PathBuf;

use orbweaver_gen::emit;
use orbweaver_gen::python::emit_python;
use orbweaver_registry::Registry;

fn golden() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/golden");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("corpus/golden")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "idl"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&p).expect("readable");
            (name, text)
        })
        .collect()
}

/// The three sets over one file: what the rule names, what the Rust emitter
/// skips for §4.4, what the Python emitter skips for §4.4 — all as qualified
/// IDL names, which is the spelling the rule uses and the registry can give
/// for any id it loaded.
struct Sets {
    rule: BTreeSet<String>,
    rule_fixed: BTreeSet<String>,
    rule_native: BTreeSet<String>,
    rule_withdrawn: BTreeSet<String>,
    rust: BTreeSet<String>,
    python: BTreeSet<String>,
    /// Everything the two emitters skipped, with the reason — for the one
    /// shape they refuse under a different name (see the constant below).
    rust_skipped: Vec<(String, String)>,
    python_skipped: Vec<(String, String)>,
}

/// The subject-independent part of a head, taken by calling it with a sentinel.
fn marker(head: fn(&str) -> String) -> String {
    const SENTINEL: &str = "\u{0}";
    head(SENTINEL).replace(SENTINEL, "")
}

fn sets(src: &str) -> Sets {
    let spec = orbweaver_idl::check(src).expect("checks out");
    let mut registry = Registry::new();
    registry.load(&spec).expect("loads");

    let uses = orbweaver_idl::deferred_wire_types(&spec);
    let rule: BTreeSet<String> = uses.iter().map(|d| d.declaration.clone()).collect();
    let rule_fixed: BTreeSet<String> =
        uses.iter().filter(|d| d.family() == "fixed").map(|d| d.declaration.clone()).collect();
    let rule_native: BTreeSet<String> =
        uses.iter().filter(|d| d.family() == "natives").map(|d| d.declaration.clone()).collect();
    let rule_withdrawn: BTreeSet<String> = uses
        .iter()
        .filter(|d| d.family() == "withdrawn types")
        .map(|d| d.declaration.clone())
        .collect();

    // The classifier asks the crate that owns the two heads what they say,
    // rather than keeping a fragment of one. It matched the literal `"4.4"`
    // until 2026-08-24, and a native was in this set only because the
    // generator's tail names the section in order to deny it — so the
    // equality below rested on wording that sentence is free to change. It
    // would have failed loudly rather than silently, which is the only reason
    // it is a smaller finding than its twin in `orbweaver-test`.
    let deferred_marker = marker(orbweaver_dynamic::deferred_wire_head);
    let native_marker = marker(orbweaver_dynamic::unmarshallable_wire_head);
    // The fifth family's marker, added 2026-08-26 — and the reason this list
    // is a list rather than a call to one function is worth a sentence. Both
    // emitters refused a `Principal` out of their own catch-all until that
    // day, so its skips carried no head at all, so **this filter could not see
    // them and the equality below held over a divergence**: the rule named
    // nothing for `corpus/golden/34` and the emitters skipped five
    // declarations, and both sets came out empty. A classifier that reads the
    // published heads is only as complete as the set of published heads.
    let withdrawn_marker = marker(orbweaver_dynamic::withdrawn_wire_head);
    let qualified = |skipped: &[(String, String)]| -> BTreeSet<String> {
        skipped
            .iter()
            .filter(|(_, why)| {
                why.contains(&deferred_marker)
                    || why.contains(&native_marker)
                    || why.contains(&withdrawn_marker)
            })
            .map(|(id, _)| {
                registry.qualified_name(id).unwrap_or_else(|| panic!("{id} has no name")).to_owned()
            })
            .collect()
    };
    let name = |skipped: &[(String, String)]| -> Vec<(String, String)> {
        skipped
            .iter()
            .map(|(id, why)| (registry.qualified_name(id).unwrap_or(id).to_owned(), why.clone()))
            .collect()
    };
    let rust_skipped = name(&emit(&registry, "g").skipped);
    let python_skipped = name(&emit_python(&registry, "g").skipped);
    let rust = qualified(&emit(&registry, "g").skipped);
    let python = qualified(&emit_python(&registry, "g").skipped);
    Sets {
        rule,
        rule_fixed,
        rule_native,
        rule_withdrawn,
        rust,
        python,
        rust_skipped,
        python_skipped,
    }
}

/// Over the golden corpus: the generator's §4.4 skips are exactly the rule's
/// findings, in both targets — file by file, so a new golden file that breaks
/// the agreement names itself.
#[test]
fn over_the_golden_corpus_the_rule_names_every_generator_skip() {
    let mut all_rule = BTreeSet::new();
    let mut all_rust = BTreeSet::new();
    let mut all_fixed = BTreeSet::new();
    let mut all_native = BTreeSet::new();
    let mut all_withdrawn = BTreeSet::new();
    for (name, src) in golden() {
        let s = sets(&src);
        assert_eq!(s.rust, s.rule, "{name}: Rust emitter vs the rule");
        assert_eq!(s.python, s.rule, "{name}: Python emitter vs the rule");
        all_rule.extend(s.rule);
        all_rust.extend(s.rust);
        all_fixed.extend(s.rule_fixed);
        all_native.extend(s.rule_native);
        all_withdrawn.extend(s.rule_withdrawn);
    }
    // The set itself, so the numbers in the record are checked, not typed:
    // three from 21 and eight from `deferred-reach` reach `fixed`; four from
    // 20 and five more from `deferred-reach` reach a valuetype or an abstract
    // interface; six from 31 reach a `native`, which §4.4 does not defer
    // because there is nothing to defer; four from 32 reach `ValueBase`,
    // which is a valuetype and had been an object reference in the registry
    // for as long as the keyword had been parsed; five from 34 reach
    // `::CORBA::Principal`, which is neither deferred nor never-marshallable —
    // it was withdrawn — and which **neither half of this gate could see**
    // until 2026-08-26, because the rule did not name it and the emitters
    // refused it out of their own catch-alls, so the two empty sets agreed.
    // Thirty-five declarations, one list, both halves of the gate.
    assert_eq!(
        all_rust.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "gc20::Describable",
            "gc20::Money",
            "gc20::Named",
            "gc20::Wallet",
            "gc21::Amount",
            "gc21::Billing",
            "gc21::Invoice",
            "gcdr::Cashier",
            "gcdr::Column",
            "gcdr::Describable",
            "gcdr::Ledger",
            "gcdr::Memo",
            "gcdr::Note",
            "gcdr::Overdrawn",
            "gcdr::Payment",
            "gcdr::Rate",
            "gcdr::Rates",
            "gcdr::Registrar",
            "gcdr::Tagged",
            "gcdr::Teller",
            "gn31::Booking",
            "gn31::Broker",
            "gn31::Handle",
            "gn31::Roster",
            "gn31::Session",
            "gn31::Slot",
            "gp34::Caller",
            "gp34::Envelope",
            "gp34::Gateway",
            "gp34::Manifest",
            "gp34::Roll",
            "gvb32::Cargo",
            "gvb32::Courier",
            "gvb32::Envelope",
            "gvb32::Manifest",
        ]
    );
    // The surplus the previous batch pinned as the generator's divergence,
    // asserted empty. Written as a difference rather than deleted, because
    // "the rule names something the generator serves" is the failure this file
    // exists to catch and it must have a live assertion, not a comment.
    let surplus: Vec<&str> = all_rule.difference(&all_rust).map(String::as_str).collect();
    assert!(surplus.is_empty(), "the rule names what the generator serves: {surplus:?}");
    // The `fixed` half unchanged at eleven, so a change to the valuetype half
    // cannot quietly move it — and the native half pinned separately for the
    // same reason. `natives` is a family of its own precisely because the
    // sentence differs: the other three are §4.4 deferrals and this one is
    // not deferred at all.
    assert_eq!(all_fixed.len(), 11, "{all_fixed:?}");
    assert_eq!(
        all_native.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "gn31::Booking",
            "gn31::Broker",
            "gn31::Handle",
            "gn31::Roster",
            "gn31::Session",
            "gn31::Slot",
        ]
    );
    // The fifth family, pinned separately for the reason the fourth is: its
    // sentence differs from every other family's, and a set that only counted
    // would let a `Principal` finding drift into the valuetype bucket — which
    // is `family()`'s `else` arm — without anything going red. The negative
    // controls in the batch that added it moved this set, not the total.
    //
    // Note the two `gp34` declarations that are **absent**: `gp34::Desk` holds
    // a reference to the unservable `Gateway`, and a reference is an IOR
    // whatever the interface's operations take; `gp34::Described` holds the
    // sibling predeclared name `::CORBA::TypeCode`, which is `tk_TypeCode` and
    // marshals perfectly well. Both would have been swept in by a rule that
    // keyed on the scope rather than on the name.
    assert_eq!(
        all_withdrawn.iter().map(String::as_str).collect::<Vec<_>>(),
        ["gp34::Caller", "gp34::Envelope", "gp34::Gateway", "gp34::Manifest", "gp34::Roll"]
    );
    assert_eq!(all_rule.len(), 35);
}

/// The three ways a reader is told something false about a `native`, asserted
/// over every layer that produces a sentence about one.
///
/// The name of the rule is `wire/deferred-type` for all four families and it is
/// imprecise for this one — renaming it would break every consumer for a word,
/// and the imprecision is answered in the *message* instead. So the message is
/// what has to be checked. A refusal that said "yet" would promise a version
/// that cannot come, and one that carried §4.4's deferral claim would send the
/// reader to a plan entry that does not name the construct and never will.
///
/// Both were live in shipped code on 2026-08-21, in the two layers this file
/// does not reach (`anyjson::from_json`, `dynany::default_value`); those are
/// held by `orbweaver-dynamic`'s `deferred_sentence_agreement`. What is held
/// here is the pair this file *is* about — the front end's rule and the two
/// emitters — because each of them writes its own sentence, and the reason the
/// native family survived six phases is that nobody compared them.
///
/// # The third way, and how this test came to have it
///
/// The first negative control run against this test came back **green**. It
/// replaced the rule's `fix()` advice with *"wait for §4.4 to land natives, or
/// declare the type in IDL…"*, which is the exact falsehood the test exists to
/// forbid — and it contains neither "yet" nor the deferral claim, so both
/// assertions passed. Two substrings are not the rule; the rule is that **every
/// mention of §4.4 in a sentence about a native is a denial**, since these
/// layers have to name the section in order to say it does not apply (the
/// emitters' reason string is what `sets()` above reads to tell a wire refusal
/// from an ordinary skip). So the check is now about the mention: a negation
/// has to sit in front of it.
#[test]
fn no_layer_calls_a_native_deferred_and_none_of_them_says_yet() {
    // §4.4's deferral claim, spelled out rather than imported: this crate
    // cannot see `orbweaver_dynamic::deferred_wire_head`, and a change to that
    // wording is a change this test has to be told about.
    const DEFERRAL_CLAIM: &str = "is not marshalled by the v1 wire (docs/PLAN.md §4.4)";
    // How far back a denial may sit from the mention it denies. Wide enough for
    // "so this is **not** one of docs/PLAN.md §4.4's three deferrals" and for
    // "**Not** deferred like §4.4's three constructs", narrow enough that a
    // negation about some other clause cannot launder a promise.
    const WINDOW: usize = 40;

    let src = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/golden/31-native-type.idl"),
    )
    .expect("corpus/golden/31-native-type.idl");
    let spec = orbweaver_idl::check(&src).expect("checks out");

    // The front end's rule: the message a reader of an S4 report meets, and
    // the fix it offers them.
    let uses = orbweaver_idl::deferred_wire_types(&spec);
    let natives: Vec<_> = uses.iter().filter(|d| d.family() == "natives").collect();
    assert!(!natives.is_empty(), "31-native-type declares one");
    let mut sentences: Vec<String> = natives.iter().flat_map(|d| [d.message(), d.fix()]).collect();

    // Both emitters: the reason string attached to every skip caused by a
    // native. It is the string `sets()` above reads to tell a wire refusal
    // from an ordinary skip, so it has to name §4.4 — and it names it only to
    // say the section does not apply, which is the distinction being pinned.
    let s = sets(&src);
    let native_ids: Vec<&str> = natives.iter().map(|d| d.declaration.as_str()).collect();
    for (name, why) in s.rust_skipped.iter().chain(&s.python_skipped) {
        if native_ids.iter().any(|d| d == name) {
            sentences.push(why.clone());
        }
    }
    assert!(sentences.len() > natives.len() * 2, "no emitter reason was collected: {sentences:?}");

    for s in &sentences {
        assert!(!s.contains("yet"), "a native is not waiting on an implementation: {s}");
        assert!(!s.contains(DEFERRAL_CLAIM), "a native must not be called a §4.4 deferral: {s}");
        for (at, _) in s.match_indices("§4.4") {
            let before = &s[at.saturating_sub(WINDOW)..at].to_lowercase();
            assert!(
                before.contains("not"),
                "a native sentence names §4.4 without denying it — \
                 \"…{before}§4.4…\" promises a section that will never carry one: {s}"
            );
        }
    }
}

/// The same three prohibitions over the fifth family, and a fourth that only
/// applies to it.
///
/// `Principal` is the one construct in this rule's set that a contract author
/// can be given a **replacement** for rather than only a redesign: caller
/// identity did not vanish when CORBA 3.0 removed the type, it moved into a
/// CSIv2 service context. So the fix has to name where it went. A fix that only
/// said "do not use it" would be true and useless, and this project has already
/// measured what a true-and-useless refusal costs — see the `<anonymous>`
/// sweep in `orbweaver-dynamic`.
///
/// The §4.4 window check below is the one this file's own negative control
/// taught it: two forbidden substrings are not the rule, the rule is that every
/// mention of the section in a sentence about this family is a denial.
#[test]
fn no_layer_calls_a_withdrawn_type_deferred_and_none_of_them_says_yet() {
    const DEFERRAL_CLAIM: &str = "is not marshalled by the v1 wire (docs/PLAN.md §4.4)";
    const WINDOW: usize = 40;

    let src = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/golden/34-corba-principal.idl"),
    )
    .expect("corpus/golden/34-corba-principal.idl");
    let spec = orbweaver_idl::check(&src).expect("checks out");

    let uses = orbweaver_idl::deferred_wire_types(&spec);
    let withdrawn: Vec<_> = uses.iter().filter(|d| d.family() == "withdrawn types").collect();
    assert_eq!(withdrawn.len(), 5, "34-corba-principal reaches one five ways: {withdrawn:?}");
    let mut sentences: Vec<String> =
        withdrawn.iter().flat_map(|d| [d.message(), d.fix()]).collect();

    let s = sets(&src);
    let ids: Vec<&str> = withdrawn.iter().map(|d| d.declaration.as_str()).collect();
    for (name, why) in s.rust_skipped.iter().chain(&s.python_skipped) {
        if ids.iter().any(|d| d == name) {
            sentences.push(why.clone());
        }
    }
    assert!(sentences.len() > withdrawn.len() * 2, "no emitter reason collected: {sentences:?}");

    for s in &sentences {
        assert!(!s.contains("yet"), "a withdrawn type is not waiting on a release: {s}");
        assert!(!s.contains(DEFERRAL_CLAIM), "a withdrawn type is not a §4.4 deferral: {s}");
        for (at, _) in s.match_indices("§4.4") {
            let before = &s[at.saturating_sub(WINDOW)..at].to_lowercase();
            assert!(
                before.contains("not"),
                "a withdrawn type's sentence names §4.4 without denying it — \"…{before}§4.4…\": {s}"
            );
        }
    }

    // And the half a `native`'s fix cannot have: where the thing the author
    // wanted actually lives now. `orbweaver_giop::csiv2` is the layer that
    // carries it, so the advice points at something this ORB implements rather
    // than at a specification the author would have to go and read.
    let fixes: Vec<String> = withdrawn.iter().map(|d| d.fix()).collect();
    for fix in &fixes {
        assert!(
            fix.contains("IdentityToken") && fix.contains("CSIv2"),
            "the fix must name where caller identity went, not only that it left: {fix}"
        );
    }
}

/// Every shape the rule's own tests know for `fixed`, through both closures:
/// a sequence element, a union case, an exception reached only through
/// `raises`, an attribute, a parameter, an inherited operation, two hops of
/// struct nesting, an array typedef, a nested typedef inside an interface —
/// and, as the negative control, a struct holding a *reference* to a deferred
/// interface, which neither closure may follow.
///
/// One shape is on neither side and is pinned as such: a `const fixed`. Both
/// emitters skip it — the registry has no `ConstValue` for a decimal, so
/// `emit_const` bails before it ever reaches the type mapper that would name
/// §4.4 — and the rule does not name it, because a constant is not marshalled
/// and refusing a whole file under `--wire v1` for one would be a false
/// refusal. The two sets agree; the *reason* the emitters give is still
/// imprecise ("could not evaluate its expression" for what is really "there is
/// no decimal type here yet"), and that is asserted below so a change to it
/// cannot pass unnoticed. It was written as `const fixed<3,1>` until
/// 2026-08-20, which omniidl refuses outright — a fixture the oracle would
/// never have compiled.
#[test]
fn every_fixed_shape_agrees_between_the_rule_and_both_emitters() {
    let src = "module m {
        typedef sequence<fixed<5,1> > Seq;
        union U switch (long) { case 1: fixed<3,0> f; default: long n; };
        exception Bad { fixed<2,1> why; };
        typedef fixed<4,2> Rate;
        interface I {
          attribute Rate spot;
          void g(inout Rate x);
        };
        interface J { void h() raises (Bad); };
        interface K : J { void ping(); };
        const fixed C = 12.5D;
        typedef fixed<7,3> Arr[4];
        struct Deep { Seq s; };
        struct Deeper { Deep d; };
        struct Holder { I ref; };
        interface Lookup { I find(); };
        interface Nesting { typedef fixed<4,0> Ticket; void ping(); };
        struct Plain { long a; string b; };
    };";
    let s = sets(src);
    assert_eq!(s.rule, s.rule_fixed, "nothing here is a valuetype");
    assert!(!s.rule.contains("m::C"), "a constant is not marshalled: {:?}", s.rule);
    assert_eq!(s.rust, s.rule, "Rust emitter vs the rule");
    assert_eq!(s.python, s.rule, "Python emitter vs the rule");
    // The constant is skipped by both, under a reason that is not §4.4's — so
    // it lands in neither set and the equality above holds without an
    // exception.
    //
    // **The reason changed on 2026-08-21, and the change is the point.** It
    // used to be "could not evaluate", and this block pinned that while saying
    // in as many words that it was *wrong about the cause*: the registry had
    // no `ConstValue` for a decimal, so a perfectly well-formed `12.5D` was
    // reported as an expression nobody could work out. Underneath that, the
    // lexer had already folded the decimal to an `f64` — so even the value the
    // registry refused to store was not the one the author wrote.
    //
    // Both are fixed. The value now reaches here exactly, and the assertion is
    // now that the reason **names it**: a skip that quotes the value it is
    // skipping cannot be the old "could not evaluate" one, and a target that
    // silently rounded a decimal could never produce this string. Still not
    // §4.4 — a constant is not marshalled — so the set shape above is
    // unchanged.
    for (target, skipped) in [("Rust", &s.rust_skipped), ("Python", &s.python_skipped)] {
        let (_, why) = skipped
            .iter()
            .find(|(id, _)| id == "m::C")
            .unwrap_or_else(|| panic!("{target}: the fixed constant was emitted: {skipped:?}"));
        assert!(
            why.contains("12.5") && !why.contains("4.4"),
            "{target}: the skip must name the exact decimal it is skipping, and must not name \
             §4.4 — a §4.4 reason puts m::C in the emitter's set, and the rule must then name \
             it too: {why}"
        );
        assert!(
            !why.contains("could not evaluate"),
            "{target}: the value evaluates now; that reason described a defect, not the \
             target: {why}"
        );
    }
    for kept in ["m::Holder", "m::Lookup", "m::Plain", "m::Nesting"] {
        assert!(!s.rule.contains(kept), "{kept} is servable: {:?}", s.rule);
    }
    assert!(s.rule.contains("m::Nesting::Ticket"), "{:?}", s.rule);
    assert!(s.rule.contains("m::K"), "the inherited operation cascades: {:?}", s.rule);
}
