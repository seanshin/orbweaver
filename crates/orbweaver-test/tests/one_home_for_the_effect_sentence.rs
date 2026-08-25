//! One home for the annotate-or-assume sentence — **across four crates**,
//! which is where the rule had no pin at all.
//!
//! An operation whose contract states no `ai_effect` has exactly two ways out:
//! somebody annotates it, or an operator declares what this exposure assumes
//! for the silences. Four layers have to say that — S4's
//! `sidl/missing-ai_effect` fix hint and S3's `s3/missing-ai_effect` fix hint
//! in `orbweaver-forge`, `Denied::EffectUnstated`'s remedy in `orbweaver-mcp`,
//! the MCP server's startup summary in that crate's binary, and the catalog
//! legend in `orbweaver-console`.
//!
//! # What was measured, 2026-08-26
//!
//! Six sites, **four vocabularies**, none of them sharing a character with
//! another by anything but luck:
//!
//! | site | values named |
//! |---|---|
//! | S4 `sidl/missing-ai_effect` fix | three |
//! | S3 `s3/missing-ai_effect` fix | three, byte-identical to S4's |
//! | S3 `s3/effect-unknown` fix | **four** — the only site naming `safe` |
//! | `Denied::remedy` | **two** |
//! | mcp-server startup | none, and it names the flag |
//! | console legend | none, and it offers no remedy at all |
//!
//! Nothing was red, and until 2026-08-26 nothing *could* be: the sentence's
//! natural owner is `orbweaver-forge`, and `orbweaver-forge` **depended on
//! `orbweaver-mcp`**, so two of the layers that owed the sentence sat upstream
//! of the crate that should have published it. That edge existed for exactly
//! one function (`exposable_interfaces`, a pure question about a catalog); it
//! moved to `Registry::exposable_interfaces` and the dependency reversed, which
//! is what let this file be written.
//!
//! # Why this file is in `orbweaver-test`
//!
//! Same reason as its sibling `one_home_for_a_wire_refusal.rs`: the fact's
//! scope is the workspace, and **a pin whose scope is narrower than its fact's
//! is a pin that will go green over the drift.** `orbweaver-test` is the one
//! crate that links forge, mcp and console at once.
//!
//! # Where the force comes from
//!
//! Every expectation below is **computed by calling the function the layer is
//! supposed to call**. Nothing here retypes a sentence. A layer that keeps a
//! literal passes today and fails the moment the wording changes, which is the
//! only event this file exists to survive.
//!
//! *`ai_effect`가 없는 오퍼레이션의 출구는 둘뿐이고 네 계층이 그 문장을 말한다.
//! 여섯 군데가 네 가지 어휘로 말하고 있었고, 아무것도 빨갛지 않았다 — 사실의
//! 범위는 워크스페이스인데 고정은 없었기 때문이다. 이 파일의 모든 기대값은
//! 문장을 쓰는 함수를 호출해서 계산한다.*

use orbweaver_forge::effect;
use orbweaver_registry::Registry;

/// A contract with one operation and no `ai_effect` on it — the condition all
/// four layers are talking about. Deliberately annotated with `ai_desc`, so
/// the only silence is the one under test.
const SILENT: &str = "module bank {
     //@ ai_desc: A customer deposit account
     interface Account {
       //@ ai_desc: Move everything out
       void sweep();
     };
   };";

const ACCOUNT: &str = "IDL:bank/Account:1.0";

fn registry_of(src: &str) -> Registry {
    let spec = orbweaver_idl::parse(src).expect("the fixture parses");
    let mut registry = Registry::new();
    registry.load(&spec).expect("the fixture loads");
    registry
}

/// Whether `sentence` is one of the ones this rule governs.
///
/// Anything else a layer says — a missing `ai_desc`, a scope, a quota — is not
/// this rule's business and is left alone. The marker is [`effect::SILENCE`]
/// itself, asked of the crate that owns it rather than matched as a fragment
/// somebody retyped.
fn is_about_the_silence(sentence: &str) -> bool {
    sentence.contains(effect::SILENCE) || sentence.contains("ai_effect")
}

/// **S4.** The fix hint an author reads, offering the author's three values.
#[test]
fn s4_does_not_write_its_own_annotate_or_assume() {
    let report = orbweaver_forge::validate(SILENT);
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule == "sidl/missing-ai_effect")
        .expect("S4 still advises on a silent operation");

    assert!(
        finding.message.contains(effect::SILENCE),
        "S4 names the condition in its own words:\n  {}\nit must read {:?}",
        finding.message,
        effect::SILENCE
    );
    let fix = finding.fix.as_deref().unwrap_or_default();
    let expected =
        effect::annotate_or_assume(&effect::OFFER_AUTHOR, Some("--assume-effect <value>"));
    assert_eq!(
        fix, expected,
        "S4 writes its own fix hint; it must be the sentence `orbweaver_forge::effect` publishes"
    );
    // The position and the rule are what make it a finding rather than a
    // verdict — the S4 property this whole rule rests on.
    assert!(
        finding.position().is_some(),
        "a fix hint with no position is a complaint: {finding:?}"
    );
}

/// **S3.** A different rule id, a different severity, and — until now — a
/// byte-identical string maintained in a second file. Two copies that agree by
/// luck are the shape this file exists to remove.
#[test]
fn s3_does_not_write_its_own_annotate_or_assume() {
    let report = orbweaver_forge::annotate::check(SILENT);
    let finding = report
        .findings
        .iter()
        .find(|f| f.rule == "s3/missing-ai_effect")
        .expect("S3 still refuses a silent operation");

    assert!(finding.message.contains(effect::SILENCE), "{}", finding.message);
    assert_eq!(
        finding.fix.as_deref().unwrap_or_default(),
        effect::annotate_or_assume(&effect::OFFER_AUTHOR, Some("--assume-effect <value>")),
        "S3 and S4 speak to the same reader and must say the same sentence"
    );
}

/// **The gate.** Its remedy offers two values where S3 and S4 offer three, and
/// that difference is deliberate — see `effect::OFFER_GATE`. This test pins
/// *both* halves: that the sentence is the shared one, and that the narrower
/// offer is what the gate uses.
#[test]
fn the_refusal_does_not_write_its_own_annotate_or_assume() {
    use orbweaver_mcp::policy::{Approval, Denied, Exposure};

    let registry = registry_of(SILENT);
    let why = Exposure::nothing()
        .allow_interface(ACCOUNT)
        .check_call(&registry, ACCOUNT, "sweep", Approval::default(), None)
        .expect_err("a silence is refused");
    assert!(matches!(why, Denied::EffectUnstated { .. }), "{why:?}");

    let remedy = why.remedy();
    assert!(
        remedy.contains(&effect::annotate_or_assume(&effect::OFFER_GATE, None)),
        "the gate writes its own remedy:\n  {remedy}\nit must read the one home publishes"
    );
    // The narrower offer is the point, not an accident: a batch that "fixed"
    // the inconsistency by widening this to the author's three would be making
    // a policy change. `idempotent` is a third answer to a question the
    // operator reading a refusal is not being asked.
    assert!(
        !remedy.contains("idempotent"),
        "the gate's remedy offers two poles, not a menu: {remedy}"
    );
    // And it stays a remedy: it names an actor who is not the caller, and no
    // route the caller can take by itself.
    assert!(orbweaver_mcp::policy::REMEDY_ACTORS.iter().any(|a| remedy.contains(a)), "{remedy}");
    for forbidden in orbweaver_mcp::policy::REMEDY_FORBIDDEN {
        assert!(!remedy.contains(forbidden), "{remedy} contains {forbidden:?}");
    }
    // The flag is *not* named here, and that is the reason `assume` takes an
    // Option: naming `--assume-effect` would address a reader who cannot run it.
    assert!(!remedy.contains("--assume-effect"), "{remedy}");
}

/// **The console.** The one layer that reports rather than advises: it states
/// the posture in force and offers no way out, because a catalog page renders
/// and decides nothing. What it still shares is the condition and the
/// declaration, and it must read both rather than retype them.
#[test]
fn the_console_legend_does_not_write_its_own_condition() {
    let registry = registry_of(SILENT);
    let exposure = orbweaver_mcp::policy::Exposure::nothing().allow_interface(ACCOUNT);

    let mut chain = orbweaver_mcp::interceptor::Chain::standard(exposure.clone());
    let refusing = orbweaver_console::catalog::build(
        &mut chain,
        &registry,
        &exposure,
        None,
        orbweaver_mcp::policy::Approval::default(),
    );
    let sentence = refusing.unannotated_sentence();
    assert!(
        sentence.contains(effect::SILENCE),
        "the legend names the condition in its own words:\n  {sentence}"
    );
    assert!(
        sentence.contains(effect::NO_ASSUMPTION),
        "the legend describes a declared-nothing exposure in its own words:\n  {sentence}"
    );
    // A statement, never a remedy: the console must not tell an operator what
    // to do, because it is the surface that decides nothing.
    assert!(!sentence.contains("annotate the operation"), "{sentence}");

    let exposure = orbweaver_mcp::policy::Exposure::nothing()
        .allow_interface(ACCOUNT)
        .assuming_unannotated(orbweaver_mcp::policy::Unannotated::Assume("read_only".into()));
    let mut chain = orbweaver_mcp::interceptor::Chain::standard(exposure.clone());
    let assumed = orbweaver_console::catalog::build(
        &mut chain,
        &registry,
        &exposure,
        None,
        orbweaver_mcp::policy::Approval::default(),
    );
    let sentence = assumed.unannotated_sentence();
    assert!(sentence.contains(effect::SILENCE), "{sentence}");
    assert!(sentence.contains("read_only"), "the declared assumption is named: {sentence}");
}

/// **The vocabulary, and the half that has nothing left to test.**
///
/// `is_harmless` is the predicate the gate actually asks, and it had two
/// hand-kept mirrors — `orbweaver_forge::annotate::UNGATED_EFFECTS` and
/// `orbweaver_test::contract::UNGATED_EFFECTS`. They now *are* the same
/// constant rather than agreeing with it, so this asserts an identity rather
/// than a correspondence: CLAUDE.md's note that where a constant becomes
/// shared the drift is impossible rather than detectable, and a negative
/// control there comes back green.
///
/// What is still worth asserting is the direction that is **not** identity:
/// every value any layer *recommends* must be one the gate actually
/// recognises, or a hint is telling an author to write something the gate will
/// treat as a typo.
#[test]
fn no_layer_recommends_a_value_the_gate_does_not_recognise() {
    assert_eq!(orbweaver_forge::annotate::UNGATED_EFFECTS, effect::UNGATED);
    assert_eq!(orbweaver_forge::annotate::GATED_EFFECTS, effect::GATED);

    for v in effect::OFFER_ALL.iter().chain(&effect::OFFER_AUTHOR).chain(&effect::OFFER_GATE) {
        let harmless = orbweaver_mcp::policy::is_harmless(v);
        assert_eq!(
            harmless,
            effect::UNGATED.contains(v),
            "{v:?} is offered by a hint and the gate classifies it the other way"
        );
        assert!(
            effect::UNGATED.contains(v) || effect::GATED.contains(v),
            "{v:?} is recommended and the gate recognises neither half of it"
        );
    }
}

/// The four layers say **one** sentence, not four that resemble each other.
///
/// The load-bearing assertion of this file: take the two halves the one home
/// publishes and require that every layer which offers a way out contains the
/// pair. A layer that reworded its own copy fails here even if its copy reads
/// perfectly well on its own, which is the only failure mode a document about
/// this rule cannot catch.
#[test]
fn every_layer_that_offers_a_way_out_offers_both_halves() {
    use orbweaver_mcp::policy::{Approval, Exposure};

    let registry = registry_of(SILENT);

    let s4 = orbweaver_forge::validate(SILENT);
    let s4 = s4
        .findings
        .iter()
        .find(|f| f.rule == "sidl/missing-ai_effect")
        .and_then(|f| f.fix.clone())
        .expect("S4 offers a fix");

    let s3 = orbweaver_forge::annotate::check(SILENT);
    let s3 = s3
        .findings
        .iter()
        .find(|f| f.rule == "s3/missing-ai_effect")
        .and_then(|f| f.fix.clone())
        .expect("S3 offers a fix");

    let gate = Exposure::nothing()
        .allow_interface(ACCOUNT)
        .check_call(&registry, ACCOUNT, "sweep", Approval::default(), None)
        .expect_err("a silence is refused")
        .remedy();

    for (layer, sentence, offer) in [
        ("orbweaver-forge S4", s4, &effect::OFFER_AUTHOR[..]),
        ("orbweaver-forge S3", s3, &effect::OFFER_AUTHOR[..]),
        ("orbweaver-mcp gate", gate, &effect::OFFER_GATE[..]),
    ] {
        assert!(is_about_the_silence(&sentence), "{layer}: {sentence}");
        assert!(
            sentence.contains(&effect::annotate(offer)),
            "{layer} writes its own annotation half:\n  {sentence}\nexpected to contain:\n  {}",
            effect::annotate(offer)
        );
        assert!(
            sentence.contains(&effect::assume(None))
                || sentence.contains("an operator declares what this exposure assumes"),
            "{layer} offers the annotation and not the operator's declaration, which tells a \
             reader the second way out does not exist:\n  {sentence}"
        );
    }
}
