//! One home for the annotate-or-assume sentence.
//!
//! An operation whose contract states no `ai_effect` has exactly two ways out,
//! and **four layers of this system say so** — S4's `sidl/missing-ai_effect`
//! fix hint, S3's `s3/missing-ai_effect` fix hint, the MCP gate's
//! `Denied::EffectUnstated` remedy, the MCP server's startup summary, and the
//! console's catalog legend. That is a sentence many layers say, which
//! CLAUDE.md calls a fact; it had no owner, so each layer wrote its own.
//!
//! # What was measured, 2026-08-26
//!
//! They were **not the same string**, and the divergence was not cosmetic:
//!
//! | layer | values it named |
//! |---|---|
//! | S4 `sidl/missing-ai_effect` | three — `read_only`, `idempotent`, `destructive` |
//! | S3 `s3/missing-ai_effect` | three, byte-identical to S4's |
//! | S3 `s3/effect-unknown` | **four** — the only site that names `safe` |
//! | MCP `Denied::remedy` | **two** — `read_only`, `destructive` |
//! | MCP server startup | none |
//! | console legend | none |
//!
//! Six sites, four vocabularies. Nothing was red, because no test compares a
//! sentence in one crate with a sentence in another — and until 2026-08-26 no
//! test *could*: `orbweaver-forge` depended on `orbweaver-mcp`, so the two
//! halves of this fact sat on opposite sides of an edge pointing the wrong way.
//!
//! # The difference is real and survives
//!
//! The obvious cleanup — one constant, one list, every layer says the same
//! four words — would be a regression dressed as a tidy-up, so this module
//! does not do it. **What each layer offers is genuinely different**, and the
//! difference is a parameter rather than a fork:
//!
//! - S3 and S4 are talking to the **author of a contract**, who is choosing
//!   what to write. [`OFFER_AUTHOR`] names the three values a generator should
//!   ever emit.
//! - The gate is talking to a **refused caller**, whose operator is choosing
//!   between letting a class of calls through and sending it to a human.
//!   [`OFFER_GATE`] names those two poles and no middle, deliberately: a
//!   remedy is not a menu of ways to get past a gate.
//! - The server and the console are talking to an **operator**, who is not
//!   editing the contract at all, so they name no values and the flag instead.
//!
//! What is shared is the *sentence* — the condition ([`SILENCE`]), the two
//! acts ([`annotate`], [`assume`]), and the fact that there are exactly two of
//! them ([`annotate_or_assume`]). A layer that reworded its half in isolation
//! is what this module makes impossible.
//!
//! # The gate that holds it
//!
//! `crates/orbweaver-test/tests/one_home_for_the_effect_sentence.rs` computes
//! the expected text by **calling the functions below**, exactly as
//! `one_home_for_a_wire_refusal.rs` does for the wire families. A layer that
//! keeps a literal passes today and fails the moment the wording changes,
//! which is the only event this module exists to survive.
//!
//! *`ai_effect`가 없는 오퍼레이션의 출구는 둘뿐이고, 네 계층이 그 문장을
//! 말한다 — 사실이다. 여섯 군데가 네 가지 어휘로 말하고 있었다. 계층마다
//! **무엇을 제시하는지는 실제로 다르며**, 그 차이는 분기가 아니라 매개변수로
//! 남는다. 공유되는 것은 문장이다.*

/// The condition, as every layer names it.
///
/// Deliberately begins **after** the article, so a layer that opens a sentence
/// with it (`"An operation whose…"`) and a layer that embeds it mid-sentence
/// (`"an operation whose…"`) both contain this exact substring and the gate can
/// look for one string rather than two casings.
pub const SILENCE: &str = "operation whose contract states no ai_effect";

/// What an exposure that has declared nothing has done — a **statement of
/// posture, not a remedy.**
///
/// The console's legend reports this and offers no way out, because a catalog
/// page renders and decides nothing; the server's startup summary reports it
/// and then names the flag. Kept beside the remedy halves rather than in either
/// of those crates because it is the same fact seen from the operator's side.
pub const NO_ASSUMPTION: &str = "this exposure declares no assumption for the silences";

/// The values S3 and S4 offer the **author of a contract**.
///
/// Three: the two poles plus `idempotent`, which is the one a generator most
/// often actually wants and the reason a two-value hint reads as a false
/// choice to somebody writing IDL.
///
/// `safe` and the underscore-less `readonly` are absent on purpose. Both are
/// *accepted* by the gate — see `orbweaver_mcp::policy::is_harmless` — and
/// neither should be *recommended*: a hint is what a generator will copy, and
/// two spellings of one value in the corpus is a diff nobody wants to read.
pub const OFFER_AUTHOR: [&str; 3] = ["read_only", "idempotent", "destructive"];

/// The values the **gate** offers a refused caller: the two poles, and no
/// middle.
///
/// **This is two rather than three and that is not an oversight.** A remedy is
/// read by the agent that was just refused, and `Denied::remedy`'s own rule —
/// written at that site — is that a remedy names an act belonging to somebody
/// who is not the caller and never a route the caller can take itself. The
/// choice in front of the operator reading it is *let this class of call
/// through* or *send it to a human*; `idempotent` is a third answer to a
/// question the operator is not being asked here, and listing it invites the
/// contract to be edited to get past a gate.
///
/// A future batch that "fixes" this by widening it to [`OFFER_AUTHOR`] is
/// making a policy change, not a consistency fix.
pub const OFFER_GATE: [&str; 2] = ["read_only", "destructive"];

/// Every value the gate lets through without a human, plus the one that needs
/// one — the full vocabulary, for a layer that is enumerating rather than
/// advising.
///
/// `s3/effect-unknown` is the one site that needs this: it fires when a
/// contract states a value nobody recognises, and the useful answer there is
/// the whole set rather than a recommendation.
pub const OFFER_ALL: [&str; 4] = ["read_only", "idempotent", "safe", "destructive"];

/// The `ai_effect` values the gate lets through **without a human**.
///
/// # This is the vocabulary itself, not a copy of it
///
/// Until 2026-08-26 there were three hand-maintained copies of this list —
/// `orbweaver_mcp::policy::is_harmless` (the predicate the gate actually
/// asks), `orbweaver_forge::annotate::UNGATED_EFFECTS`, and
/// `orbweaver_test::contract::UNGATED_EFFECTS` — and the middle one carried a
/// doc comment saying so: *"Mirrored from `orbweaver-mcp`'s
/// `policy::is_harmless` … by way of `orbweaver-test`'s
/// `contract::UNGATED_EFFECTS`."* A classifier that mirrors a predicate
/// another crate owns is CLAUDE.md's *a classifier is a sentence too*, and it
/// fails the way that class always fails: silently, when the owner's list
/// changes for a good reason.
///
/// It could not be shared before, because `orbweaver-forge` depended on
/// `orbweaver-mcp` and the list had to travel *upstream*. With that edge
/// reversed the mirror becomes the original: `policy::is_harmless` now reads
/// [`is_harmless`] below, and the other two constants delegate here. There is
/// consequently **nothing left to test** about their agreement — the drift is
/// impossible rather than detectable — which CLAUDE.md names as a reason to
/// record the fact rather than to add a check.
///
/// `readonly` is here and absent from every `OFFER_*` list on purpose: the gate
/// accepts the underscore-less spelling because contracts in the field carry
/// it, and no hint should recommend a second spelling of one value.
pub const UNGATED: [&str; 4] = ["read_only", "readonly", "idempotent", "safe"];

/// The one `ai_effect` value that means *needs a human* on purpose.
///
/// Everything outside [`UNGATED`] needs one too — a typo included, which is
/// the direction that costs something and is chosen deliberately (see
/// `orbweaver_mcp::policy::Effect`). This constant is what a layer
/// *enumerating* the vocabulary names, never what the gate decides by.
pub const GATED: [&str; 1] = ["destructive"];

/// **The gate's predicate**, and the one implementation of it.
///
/// `orbweaver_mcp::policy::is_harmless` is this function; the trim is part of
/// the rule, because an annotation's value arrives with the whitespace the
/// author left around it.
pub fn is_harmless(value: &str) -> bool {
    UNGATED.contains(&value.trim())
}

/// `a`, `b` or `c` — the one rendering of a list of choices.
fn or_list(values: &[&str]) -> String {
    let quoted: Vec<String> = values.iter().map(|v| format!("`{v}`")).collect();
    match quoted.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, head)) => format!("{} or {last}", head.join(", ")),
    }
}

/// **The annotation half**: the edit that makes a silent operation speak.
///
/// `offer` is which values this layer names — see the module docs for why that
/// is a parameter and not a constant.
pub fn annotate(offer: &[&str]) -> String {
    format!("annotate the operation with `//@ ai_effect:` naming {}", or_list(offer))
}

/// **The assumption half**: the operator's standing declaration about silences.
///
/// `flag` is the command-line spelling where the layer has one to name. The
/// gate's remedy passes `None` — not because it lacks the flag but because
/// naming it would address the reader as somebody who can run it, and the
/// reader there is the refused agent.
pub fn assume(flag: Option<&str>) -> String {
    match flag {
        None => "an operator declares what this exposure assumes for the operations that state \
                 none"
            .to_owned(),
        Some(flag) => format!(
            "an operator declares what this exposure assumes for the operations that state none \
             ({flag})"
        ),
    }
}

/// **Both halves, and the fact that there are exactly two.**
///
/// Every layer that offers a way out offers this pair. A layer that offered
/// one of them alone would be telling a reader that the other does not exist.
pub fn annotate_or_assume(offer: &[&str], flag: Option<&str>) -> String {
    format!("{}, or {}", annotate(offer), assume(flag))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_of_choices_reads_as_english() {
        assert_eq!(or_list(&["a"]), "`a`");
        assert_eq!(or_list(&["a", "b"]), "`a` or `b`");
        assert_eq!(or_list(&["a", "b", "c"]), "`a`, `b` or `c`");
        assert_eq!(or_list(&[]), "");
    }

    /// Every value any layer *recommends* must be one the gate actually
    /// recognises, or a hint is telling an author to write something that will
    /// be treated as a typo.
    #[test]
    fn every_offered_value_is_in_the_gates_vocabulary() {
        for v in OFFER_ALL.iter().chain(&OFFER_AUTHOR).chain(&OFFER_GATE) {
            assert!(
                UNGATED.contains(v) || GATED.contains(v),
                "{v:?} is offered by a hint and the gate does not recognise it"
            );
        }
        // And the predicate agrees with the two lists it is built from.
        assert!(UNGATED.iter().all(|v| is_harmless(v)));
        assert!(GATED.iter().all(|v| !is_harmless(v)));
        // The trim is part of the rule, not of the caller.
        assert!(is_harmless("  read_only\n"));
        assert!(!is_harmless("destructve"), "a typo needs a human, not a pass");
    }

    /// The two offers are different sizes and must stay that way; see
    /// [`OFFER_GATE`] for the argument.
    #[test]
    fn the_gate_offers_two_poles_and_the_author_three_values() {
        assert_eq!(OFFER_GATE.len(), 2);
        assert_eq!(OFFER_AUTHOR.len(), 3);
        assert!(OFFER_GATE.iter().all(|v| OFFER_AUTHOR.contains(v)));
        assert!(OFFER_AUTHOR.iter().all(|v| OFFER_ALL.contains(v)));
    }

    /// The flag is named where a layer has one and withheld where naming it
    /// would address the caller.
    #[test]
    fn the_assumption_half_names_a_flag_only_when_given_one() {
        assert!(!assume(None).contains("--"));
        assert!(assume(Some("--assume-effect <value>")).contains("--assume-effect <value>"));
        // Both halves are the same sentence about the same actor.
        assert!(assume(None).starts_with("an operator declares"));
    }

    /// A remedy built from these may never address the caller — the rule
    /// `orbweaver_mcp::policy::REMEDY_FORBIDDEN` enforces, checked here too so
    /// that the wording cannot acquire a second person on its way in.
    #[test]
    fn no_half_of_the_sentence_addresses_the_reader() {
        for text in [annotate(&OFFER_AUTHOR), assume(None), annotate_or_assume(&OFFER_GATE, None)] {
            for forbidden in ["you ", "your ", "yourself"] {
                assert!(!text.contains(forbidden), "{text} contains {forbidden:?}");
            }
        }
    }
}
