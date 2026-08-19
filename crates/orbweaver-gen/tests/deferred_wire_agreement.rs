//! The S4 rule and the generator's refusal name the same set — measured, not
//! assumed, over the golden corpus and over every shape the rule knows.
//!
//! `docs/PLAN.md` §4.4 defers `valuetype`, abstract interfaces and `fixed`.
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
//! **Every id the generator skips for §4.4 is a declaration the rule names.**
//! That is the direction that matters for the gate: nothing the generator will
//! refuse gets past S4's refusing form. Over `corpus/golden/` that set is
//! `gc21::{Amount, Invoice, Billing}`.
//!
//! **The rule names more, and this test pins exactly what.** The generator's
//! closure follows `TypeCode::Fixed` and nothing else: the registry records a
//! `valuetype` and an abstract interface as `TypeCode::ObjRef` — deliberately,
//! so `_is_a` and the catalogue keep working — and the generator therefore
//! emits a **reference** for them and skips nothing. `gc20::Wallet::balance()`
//! generates as returning an object reference where the peer will send a
//! value. The rule reports all four of golden 20's declarations; the generator
//! reports none. That is a real divergence in the generator, recorded here as
//! a pinned set rather than hidden by loosening the assertion: when the
//! generator starts refusing valuetypes (or marshalling them), the second
//! assertion goes red and this file is where the new agreement is written down.
//!
//! *생성기는 `fixed`만 거부하고 valuetype은 객체 참조로 내보낸다. 규칙은 넷을
//! 모두 잡는다. 그 차이를 느슨한 단언으로 감추지 않고 집합으로 고정한다.*

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
    rust: BTreeSet<String>,
    python: BTreeSet<String>,
    /// Everything the two emitters skipped, with the reason — for the one
    /// shape they refuse under a different name (see the constant below).
    rust_skipped: Vec<(String, String)>,
    python_skipped: Vec<(String, String)>,
}

fn sets(src: &str) -> Sets {
    let spec = orbweaver_idl::check(src).expect("checks out");
    let mut registry = Registry::new();
    registry.load(&spec).expect("loads");

    let uses = orbweaver_idl::deferred_wire_types(&spec);
    let rule: BTreeSet<String> = uses.iter().map(|d| d.declaration.clone()).collect();
    let rule_fixed: BTreeSet<String> =
        uses.iter().filter(|d| d.family() == "fixed").map(|d| d.declaration.clone()).collect();

    let qualified = |skipped: &[(String, String)]| -> BTreeSet<String> {
        skipped
            .iter()
            .filter(|(_, why)| why.contains("4.4"))
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
    Sets { rule, rule_fixed, rust, python, rust_skipped, python_skipped }
}

/// Over the golden corpus: the generator's §4.4 skips are exactly the rule's
/// `fixed` findings, in both targets, and the rule's surplus is exactly the
/// valuetype/abstract-interface findings — file by file, so a new golden file
/// that breaks either half names itself.
#[test]
fn over_the_golden_corpus_the_rule_names_every_generator_skip() {
    let mut all_rule = BTreeSet::new();
    let mut all_rust = BTreeSet::new();
    for (name, src) in golden() {
        let s = sets(&src);
        assert_eq!(s.rust, s.rule_fixed, "{name}: Rust emitter vs the rule's fixed findings");
        assert_eq!(s.python, s.rule_fixed, "{name}: Python emitter vs the rule's fixed findings");
        assert!(s.rust.is_subset(&s.rule), "{name}: a skip the rule did not name: {:?}", s.rust);
        all_rule.extend(s.rule);
        all_rust.extend(s.rust);
    }
    // The sets themselves, so the numbers in the record are checked, not
    // typed: three from 21, eight from `deferred-reach`, and the valuetype
    // side — four from 20, four more from `deferred-reach` — that only the
    // rule names.
    assert_eq!(
        all_rust.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "gc21::Amount",
            "gc21::Billing",
            "gc21::Invoice",
            "gcdr::Cashier",
            "gcdr::Column",
            "gcdr::Ledger",
            "gcdr::Overdrawn",
            "gcdr::Payment",
            "gcdr::Rate",
            "gcdr::Rates",
            "gcdr::Teller",
        ]
    );
    // The pinned divergence, stated as the set it is. When the generator
    // starts refusing what the wire cannot carry, this is the line to change.
    let surplus: Vec<&str> = all_rule.difference(&all_rust).map(String::as_str).collect();
    assert_eq!(
        surplus,
        [
            "gc20::Describable",
            "gc20::Money",
            "gc20::Named",
            "gc20::Wallet",
            "gcdr::Describable",
            "gcdr::Memo",
            "gcdr::Note",
            "gcdr::Registrar",
        ]
    );
    assert_eq!(all_rule.len(), 19);
}

/// Every shape the rule's own tests know for `fixed`, through both closures:
/// a sequence element, a union case, an exception reached only through
/// `raises`, an attribute, a parameter, an inherited operation, two hops of
/// struct nesting, an array typedef, a nested typedef inside an interface —
/// and, as the negative control, a struct holding a *reference* to a deferred
/// interface, which neither closure may follow.
///
/// One shape agrees on the verdict and not on the reason, and is pinned as
/// such: a `const fixed<3,1>`. Both emitters skip it, but because the registry
/// stores no value for a fixed literal — "could not evaluate its expression" —
/// which is a consequence of §4.4 reported as if it were a folding failure.
/// The rule names the cause. Recorded, not repaired: the emitters are outside
/// this batch's footprint.
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
        const fixed<3,1> C = 12.5D;
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
    let mut except_const = s.rule.clone();
    assert!(except_const.remove("m::C"), "the constant is the rule's: {:?}", s.rule);
    assert_eq!(s.rust, except_const, "Rust emitter vs the rule");
    assert_eq!(s.python, except_const, "Python emitter vs the rule");
    for (target, skipped) in [("Rust", &s.rust_skipped), ("Python", &s.python_skipped)] {
        let (_, why) = skipped
            .iter()
            .find(|(id, _)| id == "m::C")
            .unwrap_or_else(|| panic!("{target}: the fixed constant was emitted: {skipped:?}"));
        assert!(
            why.contains("could not evaluate"),
            "{target}: the reason changed — if it now names §4.4, fold m::C into the equality \
             above: {why}"
        );
    }
    for kept in ["m::Holder", "m::Lookup", "m::Plain", "m::Nesting"] {
        assert!(!s.rule.contains(kept), "{kept} is servable: {:?}", s.rule);
    }
    assert!(s.rule.contains("m::Nesting::Ticket"), "{:?}", s.rule);
    assert!(s.rule.contains("m::K"), "the inherited operation cascades: {:?}", s.rule);
}
