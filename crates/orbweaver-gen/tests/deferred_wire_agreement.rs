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
/// findings, in both targets — file by file, so a new golden file that breaks
/// the agreement names itself.
#[test]
fn over_the_golden_corpus_the_rule_names_every_generator_skip() {
    let mut all_rule = BTreeSet::new();
    let mut all_rust = BTreeSet::new();
    let mut all_fixed = BTreeSet::new();
    for (name, src) in golden() {
        let s = sets(&src);
        assert_eq!(s.rust, s.rule, "{name}: Rust emitter vs the rule");
        assert_eq!(s.python, s.rule, "{name}: Python emitter vs the rule");
        all_rule.extend(s.rule);
        all_rust.extend(s.rust);
        all_fixed.extend(s.rule_fixed);
    }
    // The set itself, so the numbers in the record are checked, not typed:
    // three from 21 and eight from `deferred-reach` reach `fixed`; four from
    // 20 and five more from `deferred-reach` reach a valuetype or an abstract
    // interface. Twenty declarations, one list, both halves of the gate.
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
        ]
    );
    // The surplus the previous batch pinned as the generator's divergence,
    // asserted empty. Written as a difference rather than deleted, because
    // "the rule names something the generator serves" is the failure this file
    // exists to catch and it must have a live assertion, not a comment.
    let surplus: Vec<&str> = all_rule.difference(&all_rust).map(String::as_str).collect();
    assert!(surplus.is_empty(), "the rule names what the generator serves: {surplus:?}");
    // The `fixed` half unchanged at eleven, so a change to the valuetype half
    // cannot quietly move it.
    assert_eq!(all_fixed.len(), 11, "{all_fixed:?}");
    assert_eq!(all_rule.len(), 20);
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
    // exception. The reason is pinned because it is wrong about the cause: the
    // registry folded the expression fine and could not *coerce* it, there
    // being no `ConstValue` for a decimal. If it ever names §4.4 instead, the
    // constant joins both sets at once and this block becomes the wrong shape.
    for (target, skipped) in [("Rust", &s.rust_skipped), ("Python", &s.python_skipped)] {
        let (_, why) = skipped
            .iter()
            .find(|(id, _)| id == "m::C")
            .unwrap_or_else(|| panic!("{target}: the fixed constant was emitted: {skipped:?}"));
        assert!(
            why.contains("could not evaluate") && !why.contains("4.4"),
            "{target}: the reason changed — a §4.4 reason puts m::C in the emitter's set, and \
             the rule must then name it too: {why}"
        );
    }
    for kept in ["m::Holder", "m::Lookup", "m::Plain", "m::Nesting"] {
        assert!(!s.rule.contains(kept), "{kept} is servable: {:?}", s.rule);
    }
    assert!(s.rule.contains("m::Nesting::Ticket"), "{:?}", s.rule);
    assert!(s.rule.contains("m::K"), "the inherited operation cascades: {:?}", s.rule);
}
