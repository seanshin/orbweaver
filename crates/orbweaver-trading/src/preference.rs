//! The preference expression: a **second** language beside the constraint,
//! and the standard's own way of saying which of the qualifying offers comes
//! first. D022 T2.
//!
//! ```text
//! preference := "MAX" numeric_field
//!             | "MIN" numeric_field
//!             | "WITH" constraint
//!             | "RANDOM" seed
//!             | "FIRST"
//! seed       := non-negative integer
//! ```
//!
//! It is a separate module because it is a separate language — `CosTrading::
//! Lookup::query` takes `Constraint` and `Preference` as two parameters of
//! two grammars, and D022 §3 lists the second as one of the four things
//! `query` drags in. It shares this crate's lexer, fields and diagnostics
//! and nothing else; `WITH` is the one place the constraint language appears
//! inside it, and it appears as a nested [`Query`] rather than as a second
//! copy of the same rules.
//!
//! # `ORDER BY` did not go away
//!
//! [`Query`]'s `ORDER BY field ASC|DESC` is still how our own callers order,
//! and the MoE contract's queries use it. This is the wire-facing form
//! **beside** it, not a replacement, and the two are deliberately not the
//! same size:
//!
//! | | `ORDER BY` | `Preference` |
//! |---|---|---|
//! | numeric field, ascending | `ORDER BY f ASC` | `MIN f` — **identical answer** |
//! | numeric field, descending | `ORDER BY f DESC` | `MAX f` — **identical answer** |
//! | text or `residency` field | `ORDER BY id ASC` | *refused* |
//! | group by a constraint | *cannot say it* | `WITH <constraint>` |
//! | seeded shuffle | *cannot say it* | `RANDOM <seed>` |
//! | the store's own order | *cannot say it* | `FIRST` |
//!
//! Each can say something the other cannot, and that is accepted rather than
//! papered over: they answer to different callers. A query may carry only
//! one of them — [`Query::select_preferring`] refuses a query that has both,
//! because two orderings for one answer is a choice, and picking silently is
//! the kind of choice this workspace records instead of making.
//!
//! # Chosen, not cited
//!
//! The Trader preference expression is defined in the OMG **Trading Object
//! Service** specification, a separate document from *CORBA — Part 1:
//! Interfaces v3.4*; the copy of Part 1 available to this batch contains no
//! preference grammar and no statement of any of these semantics (its only
//! mention of trading is the `TradingService` initial-reference row). The
//! five form names are used because they are the names the service is known
//! by; **every semantic decision below was made here, for a reason recorded
//! beside it, and none is quoted from a normative text nobody read.** Where
//! a decision diverges from what a conformant trader would do, it is marked
//! as a question for the wire facade (D022 T4).

use std::cmp::Ordering;
use std::fmt;

use crate::Offer;
use crate::query::{
    Field, KEYWORDS, Kind, ParseError, Query, Tok, Truth, describe, has_value, lex, offer_cmp,
};

/// Why a preference could not be parsed, or could not be applied.
///
/// Parse refusals are [`ParseError`] and name a byte position, exactly as
/// the constraint language's do. This is the other kind: a preference that
/// parsed but cannot be used against the query it was handed to, which has
/// no position because the defect is not in either text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreferenceError {
    /// What went wrong, phrased as something to fix.
    pub message: String,
}

impl fmt::Display for PreferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for PreferenceError {}

/// The numeric fields, which are the ones `MAX` and `MIN` accept.
const NUMERIC_FIELD_LIST: &str = "cost, latency_p50, latency_p99, load, mem_footprint, route_freq";

/// The five preference forms, spelled as this engine spells keywords.
const FORM_LIST: &str = "'MAX <numeric field>', 'MIN <numeric field>', \
                         'WITH <constraint>', 'RANDOM <seed>' or 'FIRST'";

const FORM_KEYWORDS: [&str; 5] = ["MAX", "MIN", "WITH", "RANDOM", "FIRST"];

/// A parsed preference. Build one with [`Preference::parse`] and run it with
/// [`Query::select_preferring`].
#[derive(Debug, Clone, PartialEq)]
pub struct Preference {
    form: Form,
}

#[derive(Debug, Clone, PartialEq)]
enum Form {
    Max(Field),
    Min(Field),
    With(Box<Query>),
    Random(u64),
    First,
}

impl fmt::Display for Preference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.form {
            Form::Max(field) => write!(f, "'MAX {}'", field.name()),
            Form::Min(field) => write!(f, "'MIN {}'", field.name()),
            Form::With(_) => f.write_str("'WITH <constraint>'"),
            Form::Random(seed) => write!(f, "'RANDOM {seed}'"),
            Form::First => f.write_str("'FIRST'"),
        }
    }
}

impl Preference {
    /// Parses a preference. Refusals name the byte position and what was
    /// expected there, the same bar the constraint language meets.
    ///
    /// **An empty preference is refused**, not defaulted. A trader may well
    /// have a documented default for the empty string; no text saying so was
    /// available to this batch, and inventing one here would be a semantic
    /// nobody could check. The facade that receives an empty `Preference`
    /// over the wire (D022 T4) supplies a form explicitly, and that is the
    /// right place for the default because that is where the specification's
    /// sentence about it applies.
    pub fn parse(text: &str) -> Result<Preference, ParseError> {
        let toks = lex(text)?;
        let (at, head) = toks[0].clone();
        let keyword = match head {
            Tok::Ident(ref name) if FORM_KEYWORDS.contains(&name.as_str()) => name.clone(),
            Tok::Ident(ref name) if FORM_KEYWORDS.contains(&name.to_ascii_uppercase().as_str()) => {
                return Err(ParseError {
                    at,
                    message: format!(
                        "expected a preference, found {name:?}: keywords are uppercase — \
                         write '{}'",
                        name.to_ascii_uppercase()
                    ),
                });
            }
            Tok::End => {
                return Err(ParseError {
                    at,
                    message: format!(
                        "an empty preference says nothing: expected one of {FORM_LIST}"
                    ),
                });
            }
            other => {
                return Err(ParseError {
                    at,
                    message: format!(
                        "expected a preference, found {}: the forms are {FORM_LIST}",
                        describe(&other)
                    ),
                });
            }
        };

        // `WITH` reads the whole rest of the text as a constraint, so it is
        // handled before the token stream is walked any further.
        //
        // The constraint is parsed over **this** text with the keyword
        // blanked out, rather than over the substring after it, so that
        // every byte offset the sub-parse produces is already an offset the
        // caller can point at. Adding the keyword's length to the returned
        // `ParseError::at` was the obvious way to do it and it was wrong:
        // some of those refusals name a *second* position inside their own
        // sentence — "expected ')' to close the '(' at byte 1" — and a
        // re-based `at` beside an un-re-based byte 1 is a message that
        // disagrees with itself. Blanking costs one allocation and cannot
        // drift, because there is nothing left to keep in step.
        if keyword == "WITH" {
            let mut masked = String::with_capacity(text.len());
            let body_at = at + keyword.len();
            masked.push_str(&" ".repeat(body_at));
            masked.push_str(&text[body_at..]);
            return Query::parse_constraint(&masked)
                .map(|q| Preference { form: Form::With(Box::new(q)) });
        }

        let form = match keyword.as_str() {
            "FIRST" => Form::First,
            "RANDOM" => Form::Random(parse_seed(&toks)?),
            _ => {
                let field = parse_numeric_field(&toks, &keyword)?;
                if keyword == "MAX" { Form::Max(field) } else { Form::Min(field) }
            }
        };
        // Everything but WITH is exactly two tokens (one, for FIRST).
        let consumed = if matches!(form, Form::First) { 1 } else { 2 };
        let (tail_at, tail) = toks[consumed].clone();
        if tail != Tok::End {
            let so_far = Preference { form: form.clone() };
            return Err(ParseError {
                at: tail_at,
                message: format!(
                    "expected the end of the preference after {so_far}, found {}: a \
                     preference is one form, and {} carries no direction of its own",
                    describe(&tail),
                    match form {
                        Form::Max(_) => "MAX is already descending, so it",
                        Form::Min(_) => "MIN is already ascending, so it",
                        _ => "it",
                    }
                ),
            });
        }
        Ok(Preference { form })
    }

    /// Whether this preference can place `offer` in the ordered answer.
    ///
    /// `false` puts the offer in [`crate::query::Selection::unranked`] —
    /// it qualifies under the constraint and nobody can say where it ranks.
    pub(crate) fn can_place(&self, offer: &Offer) -> bool {
        match &self.form {
            // The same question `ORDER BY` asks, through the same function.
            Form::Max(field) | Form::Min(field) => has_value(offer, *field),
            // **The T2 decision that had to be consistent with T1.** An
            // offer whose `WITH` constraint is unanswerable is neither in
            // the preferred group nor in the other one, and putting it in
            // either would be deciding by fiat exactly what `Truth` exists
            // to stop. So it is not placed at all — the same answer, and the
            // same bucket, this crate already gives an offer with no value
            // for the `ORDER BY` field, and for the same recorded reason:
            // "unknown sorts last" still picked an unmeasured expert
            // whenever nothing was measured. `is_complete()` then refuses,
            // and a caller who wants an answer over the gap guards the
            // preference's constraint with `EXIST`, exactly as they would
            // guard the query's.
            Form::With(q) => q.evaluate(offer) != Truth::Unknown,
            // Both place everything: one shuffles the whole candidate set,
            // the other declines to order it.
            Form::Random(_) | Form::First => true,
        }
    }

    /// Where `a` goes relative to `b`. `Ordering::Equal` leaves them to the
    /// caller's ascending-id tie-break, so every preference produces a total
    /// order and the same store answers the same query the same way.
    pub(crate) fn rank(&self, a: &Offer, b: &Offer) -> Ordering {
        match &self.form {
            Form::Min(field) => offer_cmp(a, b, *field),
            Form::Max(field) => offer_cmp(b, a, *field),
            // Two groups, satisfied first. `Unknown` never reaches here —
            // `can_place` set it aside — so this is a two-way split.
            Form::With(q) => {
                let key = |o: &Offer| u8::from(q.evaluate(o) != Truth::Yes);
                key(a).cmp(&key(b))
            }
            Form::Random(seed) => shuffle_key(*seed, &a.id).cmp(&shuffle_key(*seed, &b.id)),
            // `FIRST` asks for no ordering, so it gets the tie-break alone,
            // which is the store's own order. See the type docs.
            Form::First => Ordering::Equal,
        }
    }
}

fn parse_seed(toks: &[(usize, Tok)]) -> Result<u64, ParseError> {
    let (at, tok) = toks[1].clone();
    match tok {
        Tok::Number(ref n) if !n.starts_with('-') && !n.contains('.') => n
            .parse::<u64>()
            .map_err(|_| ParseError { at, message: format!("seed {n} does not fit in 64 bits") }),
        other => Err(ParseError {
            at,
            message: format!(
                "'RANDOM' must name its seed — write 'RANDOM <non-negative integer>' — \
                 found {}. This engine replays recorded traces bit for bit (see the crate \
                 docs, Determinism discipline), so an unseeded shuffle would make a \
                 replayed trace stop reproducing. The seed is in the text so that a trace \
                 carrying the query carries the order it produced.",
                describe(&other)
            ),
        }),
    }
}

fn parse_numeric_field(toks: &[(usize, Tok)], keyword: &str) -> Result<Field, ParseError> {
    let (at, tok) = toks[1].clone();
    let name = match tok {
        Tok::Ident(name) => name,
        other => {
            return Err(ParseError {
                at,
                message: format!(
                    "expected a numeric field after '{keyword}', found {}: the numeric \
                     fields are {NUMERIC_FIELD_LIST}",
                    describe(&other)
                ),
            });
        }
    };
    let Some(field) = Field::from_name(&name) else {
        let upper = name.to_ascii_uppercase();
        let hint = if KEYWORDS.contains(&upper.as_str()) && upper != name {
            format!(": keywords are uppercase — write '{upper}'")
        } else {
            format!(": the numeric fields are {NUMERIC_FIELD_LIST}")
        };
        return Err(ParseError {
            at,
            message: format!("expected a numeric field after '{keyword}', found {name:?}{hint}"),
        });
    };
    match field.kind() {
        Kind::Float | Kind::Counter => Ok(field),
        // Refused rather than quietly ordered. `residency` and the text
        // fields have a total order and `ORDER BY` will happily use it —
        // this is the one place the wire-facing form is narrower than ours,
        // and it is narrower on purpose: `MAX` and `MIN` mean "the largest
        // value of a number", and reading them as "the last enumerator" or
        // "the last string alphabetically" would be this engine deciding
        // what a standard word means. A caller who wants that ordering has
        // `ORDER BY`, which is ours and says so.
        Kind::Text | Kind::State => Err(ParseError {
            at,
            message: format!(
                "'{keyword} {}' orders by a value that is not a number: {keyword} and MIN \
                 take one of {NUMERIC_FIELD_LIST}. To order by {} use the query's own \
                 'ORDER BY {} {}'.",
                field.name(),
                field.name(),
                field.name(),
                if keyword == "MAX" { "DESC" } else { "ASC" }
            ),
        }),
    }
}

/// A deterministic 64-bit key for `RANDOM`, mixing the seed with the offer
/// id.
///
/// **Fixed for ever, and that is the point.** `RANDOM` in a trader exists to
/// spread load across equally good offers, which is ordinarily a reason to
/// reach for entropy — and entropy is exactly what this crate's determinism
/// discipline forbids, because `replay` must reproduce a recorded trace bit
/// for bit on every run and every platform. So `RANDOM` here is a *seeded
/// permutation*: a pure function of the seed and the set of offer ids, with
/// no clock, no OS entropy and no dependency. Vary the seed per invocation
/// and successive queries spread; record the seed in the trace and the
/// replay reproduces.
///
/// Written out rather than reached for: `DefaultHasher` is explicitly not
/// stable across Rust releases, and an ordering that changed under a
/// compiler upgrade would be a replay divergence nothing would catch. This
/// is an FNV-1a pass over the id, seeded, then a splitmix64 finaliser — both
/// published algorithm descriptions implemented here, in the same sense as
/// the rest of this workspace's first-party code. Its output is pinned by a
/// golden-order test: changing this function is a red test, not a silent
/// change of what a recorded trace means.
fn shuffle_key(seed: u64, id: &str) -> u64 {
    let mut h = seed ^ 0x9E37_79B9_7F4A_7C15;
    for byte in id.as_bytes() {
        h ^= u64::from(*byte);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OfferStore, Residency};

    /// Three offers, one of them registered the v1.0 way: no
    /// `specialization`, no `latency_p50`. The gap is in the fixture because
    /// every decision in this module has an answer for it.
    fn store() -> OfferStore {
        let mut s = OfferStore::new();
        for (id, spec, p50, cost, footprint, freq) in [
            ("expert-a", Some("math"), Some(10.0), 3.0, 100u64, 48u64),
            ("expert-b", Some("code"), Some(30.0), 1.0, 300, 16),
            ("expert-c", None, None, 2.0, 200, 96),
        ] {
            s.register(Offer {
                id: id.to_owned(),
                specialization: spec.map(str::to_owned),
                cost,
                latency_p50: p50,
                latency_p99: 100.0,
                load: 0.5,
                residency: Residency::Resident,
                mem_footprint: footprint,
                placement_node: "node-a".to_owned(),
                route_freq: freq,
            })
            .expect("fixture registers");
        }
        s
    }

    fn ids(offers: &[&Offer]) -> Vec<String> {
        offers.iter().map(|o| o.id.clone()).collect()
    }

    /// Every offer qualifies, so the constraint is out of the way and each
    /// table row is purely about the preference.
    fn all() -> Query {
        Query::parse("cost > 0").expect("parses")
    }

    /// The preference table: expression, the order it produces, and the
    /// offers it could not place. Every unanswerable case is here, not a
    /// sample of them.
    #[test]
    fn every_preference_form_orders_the_store_the_way_it_says_it_does() {
        let s = store();
        let q = all();
        for (text, expect_matched, expect_unranked) in [
            // MAX and MIN over a field every offer carries.
            ("MAX route_freq", vec!["expert-c", "expert-a", "expert-b"], vec![]),
            ("MIN route_freq", vec!["expert-b", "expert-a", "expert-c"], vec![]),
            ("MIN cost", vec!["expert-b", "expert-c", "expert-a"], vec![]),
            ("MAX cost", vec!["expert-a", "expert-c", "expert-b"], vec![]),
            ("MAX mem_footprint", vec!["expert-b", "expert-c", "expert-a"], vec![]),
            // MAX/MIN over the field expert-c never recorded: it qualifies,
            // and nobody can say where it ranks.
            ("MIN latency_p50", vec!["expert-a", "expert-b"], vec!["expert-c"]),
            ("MAX latency_p50", vec!["expert-b", "expert-a"], vec!["expert-c"]),
            // A field every offer carries equally: the tie-break is all
            // there is, and it is ascending id.
            ("MAX latency_p99", vec!["expert-a", "expert-b", "expert-c"], vec![]),
            // WITH: the satisfying group first, the rest after, each group
            // internally in id order.
            ("WITH cost < 2.5", vec!["expert-b", "expert-c", "expert-a"], vec![]),
            ("WITH route_freq > 40", vec!["expert-a", "expert-c", "expert-b"], vec![]),
            // WITH over the gap — the T2 decision. expert-c's constraint is
            // unanswerable, so it is not placed in either group.
            ("WITH specialization == 'math'", vec!["expert-a", "expert-b"], vec!["expert-c"]),
            ("WITH latency_p50 < 20", vec!["expert-a", "expert-b"], vec!["expert-c"]),
            // …and EXIST closes it, exactly as it does for a query.
            (
                "WITH EXIST specialization AND specialization == 'math'",
                vec!["expert-a", "expert-b", "expert-c"],
                vec![],
            ),
            ("WITH NOT EXIST specialization", vec!["expert-c", "expert-a", "expert-b"], vec![]),
            // WITH may use the whole T1 grammar, including OR and parens.
            (
                "WITH (specialization == 'code' OR route_freq > 40) AND cost < 2.5",
                vec!["expert-b", "expert-c", "expert-a"],
                vec![],
            ),
            // FIRST promises the store's order and nothing else.
            ("FIRST", vec!["expert-a", "expert-b", "expert-c"], vec![]),
        ] {
            let pref = Preference::parse(text).unwrap_or_else(|e| panic!("{text:?}: {e}"));
            let sel = q.select_preferring(&s, &pref).unwrap_or_else(|e| panic!("{text:?}: {e}"));
            assert_eq!(ids(&sel.matched), *expect_matched, "{text:?}");
            assert_eq!(ids(&sel.unranked), *expect_unranked, "{text:?}");
            assert!(sel.unanswerable.is_empty(), "{text:?}: the constraint judged every offer");
            assert_eq!(sel.is_complete(), expect_unranked.is_empty(), "{text:?}");
        }
    }

    /// `MAX f` and `MIN f` are `ORDER BY f DESC` and `ORDER BY f ASC`
    /// exactly — same offers, same order, same offers set aside. The two
    /// languages overlap here and must not drift apart on the overlap;
    /// everywhere else each says something the other cannot.
    #[test]
    fn max_and_min_agree_with_order_by_offer_for_offer_including_the_gap() {
        let s = store();
        for (pref_text, order_text) in [
            ("MAX route_freq", "cost > 0 ORDER BY route_freq DESC"),
            ("MIN route_freq", "cost > 0 ORDER BY route_freq ASC"),
            ("MAX cost", "cost > 0 ORDER BY cost DESC"),
            ("MIN cost", "cost > 0 ORDER BY cost ASC"),
            ("MAX mem_footprint", "cost > 0 ORDER BY mem_footprint DESC"),
            // The one that matters: both must set expert-c aside, not sort
            // it last.
            ("MIN latency_p50", "cost > 0 ORDER BY latency_p50 ASC"),
            ("MAX latency_p50", "cost > 0 ORDER BY latency_p50 DESC"),
        ] {
            let by_pref = all()
                .select_preferring(&s, &Preference::parse(pref_text).expect(pref_text))
                .expect(pref_text);
            let by_order = Query::parse(order_text).expect(order_text).select_reporting(&s);
            assert_eq!(ids(&by_pref.matched), ids(&by_order.matched), "{pref_text}");
            assert_eq!(ids(&by_pref.unranked), ids(&by_order.unranked), "{pref_text}");
            assert_eq!(by_pref.is_complete(), by_order.is_complete(), "{pref_text}");
        }
    }

    /// `RANDOM` is a seeded permutation, which is the only shape of
    /// "random" a deterministic replay engine can hold. Same seed, same
    /// order, for ever; a different seed, a different order.
    #[test]
    fn random_is_seeded_and_replays_to_the_same_order() {
        let s = store();
        let q = all();
        let run = |text: &str| {
            let pref = Preference::parse(text).expect(text);
            ids(&q.select_preferring(&s, &pref).expect(text).matched)
        };
        // Pinned, not merely stable: these are the orders a recorded trace
        // must still produce after any change to this crate, and they were
        // *computed* from the algorithm's description by a second
        // implementation before being written here, not copied out of a run
        // of this one. If `shuffle_key` changes, this is the test that says
        // so. Six seeds, six distinct permutations of three offers — every
        // order there is, so no assertion here is satisfiable by an
        // implementation that shuffles less than fully.
        assert_eq!(run("RANDOM 0"), ["expert-b", "expert-c", "expert-a"]);
        assert_eq!(run("RANDOM 1"), ["expert-c", "expert-b", "expert-a"]);
        assert_eq!(run("RANDOM 3"), ["expert-b", "expert-a", "expert-c"]);
        assert_eq!(run("RANDOM 6"), ["expert-a", "expert-b", "expert-c"]);
        assert_eq!(run("RANDOM 8"), ["expert-a", "expert-c", "expert-b"]);
        assert_eq!(run("RANDOM 9"), ["expert-c", "expert-a", "expert-b"]);
        assert_eq!(run("RANDOM 1"), ["expert-c", "expert-b", "expert-a"], "twice is the point");
        assert_eq!(
            run("RANDOM 18446744073709551615"),
            run("RANDOM 18446744073709551615"),
            "the whole seed range is usable and still replays"
        );
        // Different seeds do actually differ — a "shuffle" that ignored its
        // seed would pass every assertion above.
        let orders: std::collections::BTreeSet<_> =
            (0u64..24).map(|seed| run(&format!("RANDOM {seed}"))).collect();
        assert_eq!(orders.len(), 6, "three offers have six orders and 24 seeds reach them all");
        // And it is a permutation: everything that qualifies is still there.
        for order in &orders {
            let mut sorted = order.clone();
            sorted.sort();
            assert_eq!(sorted, ["expert-a", "expert-b", "expert-c"]);
        }
    }

    /// `FIRST` promises the store's order and says so, rather than promising
    /// nothing. Our store has one — ascending id, whatever the registration
    /// order — so a promise we keep is worth more than a freedom we do not
    /// use. A trader whose store had no such order could return anything and
    /// still be answering `FIRST`; ours is the narrower answer.
    #[test]
    fn first_promises_the_stores_own_order() {
        let mut s = OfferStore::new();
        for id in ["expert-z", "expert-m", "expert-a"] {
            let mut o = store().get("expert-a").expect("fixture").clone();
            o.id = id.to_owned();
            s.register(o).expect("registers");
        }
        let pref = Preference::parse("FIRST").expect("parses");
        let sel = all().select_preferring(&s, &pref).expect("applies");
        assert_eq!(ids(&sel.matched), ["expert-a", "expert-m", "expert-z"]);
        assert!(sel.is_complete());
    }

    /// The preference and the query's own `ORDER BY` are two answers to one
    /// question, and the engine refuses rather than picking.
    #[test]
    fn a_query_that_already_orders_refuses_a_preference_instead_of_choosing() {
        let s = store();
        let q = Query::parse("cost > 0 ORDER BY route_freq DESC").expect("parses");
        let pref = Preference::parse("MIN cost").expect("parses");
        let err = q.select_preferring(&s, &pref).expect_err("two orderings");
        assert!(err.message.contains("two orderings for one answer"), "{err}");
        assert!(err.message.contains("'MIN cost'"), "{err}: it names which preference");
        // Dropping the ORDER BY is the fix the message names, and it works.
        let q = Query::parse("cost > 0").expect("parses");
        assert!(q.select_preferring(&s, &pref).is_ok());
        // And ORDER BY still works for our own callers, untouched.
        let q = Query::parse("cost > 0 ORDER BY route_freq DESC").expect("parses");
        assert_eq!(ids(&q.select(&s)), ["expert-c", "expert-a", "expert-b"]);
    }

    /// The `WITH` decision, stated as the sentence it is: an offer whose
    /// preference-constraint is unanswerable is in neither group, and
    /// `is_complete()` refuses — the same answer T1 gave for `NOT` over an
    /// unrecorded field, for the same reason.
    #[test]
    fn with_over_an_unanswerable_offer_places_it_in_neither_group() {
        let s = store();
        let pref = Preference::parse("WITH specialization == 'math'").expect("parses");
        let sel = all().select_preferring(&s, &pref).expect("applies");
        // expert-a satisfies it, expert-b does not, expert-c cannot say.
        assert_eq!(ids(&sel.matched), ["expert-a", "expert-b"]);
        assert_eq!(ids(&sel.unranked), ["expert-c"], "neither preferred nor un-preferred");
        assert!(sel.unanswerable.is_empty(), "the *constraint* judged it fine");
        assert!(!sel.is_complete(), "an order with an unplaced offer in it is a refusal");
        assert!(sel.gap_note().expect("a note").contains("expert-c"));

        // The same intent, guarded: now every offer is placed and the answer
        // is complete. Consistency with T1 is the point — `EXIST` closes a
        // preference's gap exactly as it closes a constraint's.
        let pref = Preference::parse("WITH EXIST specialization AND specialization == 'math'")
            .expect("parses");
        let sel = all().select_preferring(&s, &pref).expect("applies");
        assert_eq!(ids(&sel.matched), ["expert-a", "expert-b", "expert-c"]);
        assert!(sel.is_complete());
    }

    /// The constraint decides membership; the preference decides order. They
    /// ask different questions and their gaps land in different buckets.
    #[test]
    fn the_constraints_gap_and_the_preferences_gap_are_different_buckets() {
        let s = store();
        // The *constraint* cannot judge expert-c: it never reaches the
        // preference at all.
        let q = Query::parse("specialization != 'vision'").expect("parses");
        let sel = q.select_preferring(&s, &Preference::parse("FIRST").expect("p")).expect("ok");
        assert_eq!(ids(&sel.matched), ["expert-a", "expert-b"]);
        assert_eq!(ids(&sel.unanswerable), ["expert-c"]);
        assert!(sel.unranked.is_empty());
        // The *preference* cannot place expert-c, having been judged fine.
        let sel = all()
            .select_preferring(&s, &Preference::parse("MIN latency_p50").expect("p"))
            .expect("ok");
        assert!(sel.unanswerable.is_empty());
        assert_eq!(ids(&sel.unranked), ["expert-c"]);
    }

    // ---- diagnostics: position and expectation, both, as in T1 ----

    #[test]
    fn every_preference_refusal_names_its_byte_position_and_an_expectation() {
        for (text, at, expected) in [
            ("", 0usize, "an empty preference says nothing"),
            ("   ", 3, "an empty preference says nothing"),
            ("BEST route_freq", 0, "expected a preference, found identifier \"BEST\""),
            ("BEST route_freq", 0, "the forms are 'MAX <numeric field>'"),
            ("42", 0, "expected a preference, found number 42"),
            ("'MAX'", 0, "expected a preference, found string 'MAX'"),
            // Lowercase, the same rule as the constraint language.
            ("max route_freq", 0, "keywords are uppercase — write 'MAX'"),
            ("random 7", 0, "keywords are uppercase — write 'RANDOM'"),
            ("first", 0, "keywords are uppercase — write 'FIRST'"),
            // MAX/MIN want a numeric field, and say which are numeric.
            ("MAX", 3, "expected a numeric field after 'MAX', found the end"),
            ("MIN 7", 4, "expected a numeric field after 'MIN', found number 7"),
            ("MAX nonsuch", 4, "the numeric fields are cost, latency_p50"),
            ("MAX specialization", 4, "orders by a value that is not a number"),
            ("MIN residency", 4, "orders by a value that is not a number"),
            ("MIN residency", 4, "use the query's own 'ORDER BY residency ASC'"),
            ("MAX placement_node", 4, "use the query's own 'ORDER BY placement_node DESC'"),
            ("MAX asc", 4, "keywords are uppercase — write 'ASC'"),
            // A direction is not part of this language.
            ("MAX route_freq DESC", 15, "MAX is already descending"),
            ("MIN cost ASC", 9, "MIN is already ascending"),
            ("FIRST cost", 6, "expected the end of the preference after 'FIRST'"),
            // RANDOM says why it needs a seed.
            ("RANDOM", 6, "'RANDOM' must name its seed"),
            ("RANDOM", 6, "replayed trace stop reproducing"),
            ("RANDOM -1", 7, "must name its seed"),
            ("RANDOM 1.5", 7, "must name its seed"),
            ("RANDOM route_freq", 7, "must name its seed"),
            ("RANDOM 99999999999999999999999", 7, "does not fit in 64 bits"),
            ("RANDOM 7 8", 9, "expected the end of the preference after 'RANDOM 7'"),
            // WITH delegates to the constraint language over this text's own
            // byte offsets — including the positions a message names inside
            // its own sentence, which is where the first attempt went wrong.
            ("WITH", 4, "expected a condition here"),
            ("WITH nonsuch == 1", 5, "unknown field \"nonsuch\""),
            ("WITH cost", 9, "expected a comparison operator"),
            ("WITH cost == ", 13, "expected a number for cost"),
            ("WITH cost == 1 AND", 18, "expected a condition here"),
            ("WITH (cost == 1", 15, "expected ')' to close the '(' at byte 5"),
            ("WITH cost == 1 ORDER BY cost ASC", 15, "'ORDER BY' is not allowed here"),
            ("WITH exist cost", 5, "keywords are uppercase — write 'EXIST'"),
        ] {
            let err = Preference::parse(text).expect_err(text);
            assert_eq!(err.at, at, "{text:?} → {err}");
            assert!(err.message.contains(expected), "{text:?} → {err}");
        }
    }

    /// A `WITH` constraint is the whole constraint language, so it inherits
    /// its nesting bound too — at a position in the preference's own bytes,
    /// like every other position it reports.
    #[test]
    fn a_with_constraint_inherits_the_constraint_languages_nesting_bound() {
        let depth = usize::try_from(crate::query::MAX_DEPTH).expect("fits") + 1;
        let err = Preference::parse(&format!("WITH {}cost == 1", "(".repeat(depth)))
            .expect_err("too deep");
        assert_eq!(err.at, 5 + depth - 1, "the preference's own bytes");
        assert!(err.message.contains("levels deep"), "{err}");
    }

    /// Every position a `WITH` refusal names — the `ParseError::at` **and**
    /// any byte the sentence itself quotes — is an offset into the
    /// preference text the caller holds. This is the one that was wrong:
    /// `at` was re-based and the "'(' at byte N" inside the message was not,
    /// so a single refusal named two different places for one bracket.
    #[test]
    fn a_with_refusal_names_positions_in_the_preference_text_including_inside_its_message() {
        let text = "WITH (cost == 1 AND (load < 1";
        let err = Preference::parse(text).expect_err("unclosed");
        assert_eq!(err.at, text.len());
        assert!(err.message.contains("'(' at byte 20"), "{err}");
        assert_eq!(&text[20..21], "(", "byte 20 really is that bracket");
        // Leading whitespace shifts everything, and everything shifts with it.
        let text = "   WITH (cost == 1";
        let err = Preference::parse(text).expect_err("unclosed");
        assert_eq!(err.at, text.len());
        assert!(err.message.contains("'(' at byte 8"), "{err}");
        assert_eq!(&text[8..9], "(", "byte 8 really is that bracket");
    }
}
