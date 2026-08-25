//! What `CosTrading::Lookup::query` decides, with no wire in scope.
//! `docs/decisions/D022` T4's engine half.
//!
//! # Why this is here and not in the servant
//!
//! D022 T4 puts a `Lookup` servant on our POA. Everything that servant
//! *decides* — which constraint parses, which preference is legal, which
//! import policies are refused, and above all **whether the answer fits** — is
//! decided here, where it can be tested without a socket. The servant reads
//! CDR, calls [`TypedOfferStore::answer`], and writes CDR. That split is not
//! tidiness: the refusal sentence below is a fact several layers say, and
//! `CLAUDE.md` requires it to live in one function reachable from every layer
//! that owes it — the servant, our own Rust callers, and the MCP face.
//!
//! # The bound, and what happens at it / 한계와 그 지점에서 일어나는 일
//!
//! `query` has an `out OfferIterator offer_itr`. **This trader never creates
//! one** (D022 §7): an `OfferIterator` is a POA-hosted object per query with a
//! lifecycle, which is exactly the reference-outliving-its-value hazard
//! `COMPONENTS.md` records as deliberately not built for `DynAny`. The
//! specification makes the escape legal — when the number of matching offers
//! is at most `how_many`, all of them are returned in `offers` and `offer_itr`
//! is nil — and that escape is what this module implements.
//!
//! So there are exactly two outcomes and no third:
//!
//! - **the answer fits** — every matching offer is in `offers`, the iterator
//!   is nil, and the answer is complete and conformant;
//! - **the answer does not fit** — the query is **refused**, by
//!   [`cannot_answer_completely`], naming the iterator it would have needed and
//!   quoting both bounds.
//!
//! Truncating to `how_many` and returning a nil iterator anyway is the third
//! outcome this module refuses to have. A nil iterator *means* "that is all of
//! them"; returning one over a truncated list is a false statement on the
//! wire, and it is false in the direction that loses offers the caller asked
//! for and cannot then ask for again. A refusal the caller can act on — widen
//! `how_many`, or tighten the constraint — is worth more than an answer it
//! cannot tell is short.
//!
//! *결과는 둘뿐이다: 완전히 들어가면 반복자는 nil이고, 들어가지 않으면 거부한다.
//! `how_many`로 잘라내고 nil 반복자를 붙이는 것은 "이것이 전부"라는 거짓말이며,
//! 호출자가 다시 물을 수도 없는 방향의 거짓말이다.*
//!
//! # Where the bound is visible to a client
//!
//! [`MAX_RETURN_CARD`] is not only in this sentence: `Lookup` inherits
//! `ImportAttributes`, whose `max_return_card` is the specification's own name
//! for exactly this number, and D022 T4's servant answers it from this
//! constant. A client can read the bound before it asks, and the refusal
//! quotes it again when it asks anyway.

use crate::query::{ParseError, Query, Selection};
use crate::service_type::{Refusal, RefusalKind, TypedOfferStore};
use crate::{Offer, preference::Preference, query::FIELD_LIST};

/// The most offers this trader will return in one answer.
///
/// A **bound, deliberately, not a floor**: it is the number
/// `ImportAttributes::max_return_card` reports on the wire and the number
/// [`cannot_answer_completely`] quotes, and both must be this same constant so
/// that a client that reads the attribute and a client that reads the refusal
/// are told the same thing.
///
/// Chosen rather than derived. It exists so that a query over a store that has
/// grown without anyone noticing is refused rather than answered with a reply
/// message of unbounded size; 512 offers of ten properties is a reply in the
/// low hundreds of kilobytes, comfortably inside
/// `orbweaver_giop::DEFAULT_MAX_MESSAGE_SIZE`, and far above anything the MoE
/// control plane registers. Raising it is a decision about message size, and
/// the constant is the one place to make it.
pub const MAX_RETURN_CARD: u32 = 512;

/// How many offers this trader will consider matching in one query.
///
/// Equal to [`MAX_RETURN_CARD`] because this trader returns everything it
/// matched or refuses: matching more than it can return would be work whose
/// only possible outcome is the refusal.
pub const MAX_MATCH_CARD: u32 = MAX_RETURN_CARD;

/// How many offers this trader will search in one query.
///
/// Zero means unlimited, which is the specification's convention for the
/// cardinality attributes and is honest here: the store is a `BTreeMap` in
/// this process and every query walks all of it.
pub const MAX_SEARCH_CARD: u32 = 0;

/// Which properties the caller asked to have returned —
/// `CosTrading::Lookup::SpecifiedProps`, whose discriminator is
/// `HowManyProps {none, some, all}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesiredProps {
    /// `none`: return the offers, carrying no properties.
    None,
    /// `some`: return only these properties. An unknown name is refused.
    Some(Vec<String>),
    /// `all`: return every property the offer has.
    All,
}

/// One `Lookup::query` call, with the wire already decoded.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    /// `in ServiceTypeName type`.
    pub service_type: &'a str,
    /// `in Constraint constr`. Empty or blank means *every offer of the
    /// type*, which is the only reading available: this engine's grammar has
    /// no `TRUE` literal, and refusing an empty constraint would make "list
    /// the offers of this type" unaskable.
    pub constraint: &'a str,
    /// `in Preference pref`. Empty or blank means `FIRST` — the
    /// specification's default preference, and the only one that adds no
    /// ordering of its own. [`Preference::parse`] refuses an empty string on
    /// purpose (an empty preference says nothing), so the defaulting is done
    /// here, where "the caller said nothing" and "the caller wrote nonsense"
    /// are still distinguishable.
    pub preference: &'a str,
    /// `in PolicySeq policies`, by name. Any name at all is refused — see
    /// [`TypedOfferStore::answer`].
    pub policies: &'a [String],
    /// `in SpecifiedProps desired_props`.
    pub desired: DesiredProps,
    /// `in unsigned long how_many`.
    pub how_many: u32,
}

/// A complete answer to a [`Request`]. There is no incomplete one — see the
/// module header.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer<'a> {
    /// Every matching offer, in preference order. `out OfferSeq offers`.
    pub offers: Vec<&'a Offer>,
    /// The property names to project onto each offer, in the engine's own
    /// field order. Empty for [`DesiredProps::None`].
    pub properties: Vec<&'static str>,
    /// `out PolicyNameSeq limits_applied`.
    ///
    /// **Always empty, and that is the point.** This out parameter names the
    /// policies that clamped the answer; this trader never clamps, because a
    /// clamped answer is the truncation the module header refuses. A trader
    /// that refuses instead of clamping has nothing to report here, and a
    /// non-empty `limits_applied` beside a nil iterator would be the very
    /// claim this design avoids making.
    pub limits_applied: Vec<String>,
    /// What [`Selection::gap_note`] said, if anything: the offers the
    /// constraint could not answer about, and the ones the preference could
    /// not place. Not a wire field — `query` has nowhere to put it — but the
    /// servant logs it and our own callers read it.
    pub gap_note: Option<String>,
}

/// The one home for the sentence a query that cannot be answered completely is
/// refused with.
///
/// Returns `None` when the answer fits, so that the fitting case and the
/// refusing case are decided in one place by one function rather than by a
/// comparison written out at each layer that has to make it. Both branches
/// name `CosTrading::OfferIterator` — the construct whose absence is the
/// reason — and both quote [`MAX_RETURN_CARD`].
///
/// *완전히 답할 수 없는 질의를 거부하는 문장의 유일한 집. 들어가면 `None`을
/// 반환하므로, 판정 자체가 이 함수 하나에 산다.*
pub fn cannot_answer_completely(matched: usize, how_many: u32) -> Option<String> {
    let bound = MAX_RETURN_CARD;
    if matched > bound as usize {
        return Some(format!(
            "this query matched {matched} offers and this trader returns at most {bound} in one \
             answer: the rest would have to be returned through a `CosTrading::OfferIterator`, \
             which this trader does not create, so raising `how_many` cannot help — tighten the \
             constraint (`max_return_card` is {bound})"
        ));
    }
    if matched > how_many as usize {
        return Some(format!(
            "this query matched {matched} offers and `how_many` was {how_many}: the remaining {} \
             would have to be returned through a `CosTrading::OfferIterator`, which this trader \
             does not create — re-ask with `how_many` at least {matched}, or tighten the \
             constraint (`max_return_card` is {bound})",
            matched - how_many as usize
        ));
    }
    None
}

impl TypedOfferStore {
    /// Answers one `Lookup::query`, or refuses it.
    ///
    /// The order of the checks is the order the specification lists the
    /// exceptions in, and it is load-bearing: a caller that gets both a bad
    /// type name and a bad constraint is told about the type name, every time,
    /// on every ORB. An order that depends on which check happens to run first
    /// is an interoperability difference nobody wrote down.
    pub fn answer<'a>(&'a self, req: &Request<'_>) -> Result<Answer<'a>, Refusal> {
        // 1. IllegalServiceType, then UnknownServiceType.
        let service_type = self.service_type(req.service_type)?;

        // 2. IllegalConstraint.
        let constraint = parse_constraint(req.constraint)?;

        // 3. IllegalPreference.
        let preference = parse_preference(req.preference)?;

        // 4. IllegalPolicyName. Every name, because this trader implements no
        //    import policy at all: the cardinalities it applies are its own
        //    constants, not something a caller negotiates.
        if let Some(name) = req.policies.first() {
            return Err(Refusal::new(
                RefusalKind::IllegalPolicyName,
                format!(
                    "this trader implements no import policy, and this query carries {name:?} \
                     (of {} in the sequence): the cardinalities it applies are fixed — \
                     `max_search_card` {MAX_SEARCH_CARD}, `max_match_card` {MAX_MATCH_CARD}, \
                     `max_return_card` {MAX_RETURN_CARD} — and it has no links, so `hop_count` \
                     and `follow_policy` say nothing here either. Send an empty `PolicySeq`",
                    req.policies.len()
                ),
            ));
        }

        // 5. IllegalPropertyName, from `desired_props`.
        let properties = project(&req.desired)?;

        // 6. Run it. The constraint decides membership, the type decides the
        //    population, the preference decides the order.
        let selection = constraint
            .select_preferring(self.store(), &preference)
            .map_err(|e| Refusal::new(RefusalKind::IllegalPreference, e.message))?;
        let selection: Selection<'a> = self.narrow(service_type.name(), selection);

        // 7. Flatten the engine's two "this offer matched" lists into the
        //    wire's one. See `matching_offers`.
        let gap_note = selection.gap_note();
        let offers = matching_offers(selection);

        // 8. Does it fit? This is the only place that asks.
        if let Some(said) = cannot_answer_completely(offers.len(), req.how_many) {
            return Err(Refusal::new(RefusalKind::DoesNotFit, said));
        }

        Ok(Answer { gap_note, offers, properties, limits_applied: Vec::new() })
    }
}

/// Every offer that satisfied the constraint, in one sequence: the ones the
/// preference could order, then the ones it could not.
///
/// **The wire has one `OfferSeq` and the engine has three lists, and the
/// difference is not cosmetic.** [`Selection::unranked`] holds offers the
/// constraint answered `Yes` about but the preference could not place — `MAX
/// latency_p50` over an expert nobody has timed. The engine leaves them out of
/// the ordered answer on a recorded argument (`crate::preference`): a router
/// taking the head of the list must not be handed an unmeasured expert. That
/// argument is about being **first**, and it survives here — they go last.
///
/// Dropping them instead would make `query` report fewer matches than matched,
/// which is the same false statement as truncating to `how_many`: the caller
/// cannot tell the answer is short, and cannot ask again for what is missing.
/// [`Selection::unanswerable`] is a different thing and stays out — those
/// offers did not match, the constraint could not say — and
/// [`Answer::gap_note`] reports them.
///
/// *와이어에는 `OfferSeq`가 하나뿐이고 엔진에는 목록이 셋이다. 순위를 매길 수
/// 없던 제안을 버리면 일치 개수를 줄여 보고하는 셈이며, 이는 잘라내기와 같은
/// 거짓말이다. 그래서 맨 뒤에 붙인다 — 엔진의 논거는 **맨 앞**에 대한 것이었고,
/// 그 논거는 여기서도 지켜진다.*
fn matching_offers(selection: Selection<'_>) -> Vec<&Offer> {
    let mut offers = selection.matched;
    offers.extend(selection.unranked);
    offers
}

/// Parses a constraint, defaulting a blank one to *every offer of the type*.
///
/// The blank case is expressed as a query that is `Yes` for every offer
/// without naming a property, because naming one would make an offer that does
/// not carry it `Unknown` — an empty constraint must not quietly filter.
/// `EXIST id` is that query: `id` is the one property every offer has by
/// construction, so it is a constant `Yes`.
fn parse_constraint(text: &str) -> Result<Query, Refusal> {
    let blank = text.trim().is_empty();
    let text = if blank { "EXIST id" } else { text };
    let query = Query::parse(text).map_err(|e: ParseError| {
        Refusal::new(
            RefusalKind::IllegalConstraint,
            format!(
                "this constraint did not parse — {e}. This trader's constraint language is the \
                 subset `crate::query` documents: comparisons over {FIELD_LIST}, combined with \
                 AND, OR, NOT and parentheses, plus EXIST. Its keywords are uppercase"
            ),
        )
    })?;
    // `ORDER BY` is this engine's own extension, not TCL, and on this
    // interface the ordering is the `pref` parameter. Refusing it here rather
    // than letting `select_preferring` refuse it later matters: a caller who
    // sent a blank preference had `FIRST` defaulted in for them, and would
    // otherwise be told their query fought a preference they never wrote.
    if query.has_order() {
        return Err(Refusal::new(
            RefusalKind::IllegalConstraint,
            "this constraint carries 'ORDER BY', which is this engine's own extension and not \
             part of the Trader Constraint Language: on `Lookup::query` the ordering is the \
             `pref` parameter, so say it there — 'MIN <field>' for ascending, 'MAX <field>' for \
             descending"
                .to_owned(),
        ));
    }
    Ok(query)
}

/// Parses a preference, defaulting a blank one to `FIRST`.
fn parse_preference(text: &str) -> Result<Preference, Refusal> {
    let text = if text.trim().is_empty() { "FIRST" } else { text };
    Preference::parse(text).map_err(|e| {
        Refusal::new(
            RefusalKind::IllegalPreference,
            format!(
                "this preference did not parse — {e}. This trader's preference language is the \
                 five forms `crate::preference` documents, and its keywords are uppercase"
            ),
        )
    })
}

/// Turns `desired_props` into the property names to project, refusing a name
/// that is not one of the ten.
fn project(desired: &DesiredProps) -> Result<Vec<&'static str>, Refusal> {
    match desired {
        DesiredProps::None => Ok(Vec::new()),
        DesiredProps::All => Ok(crate::service_type::ALL_PROPERTIES.to_vec()),
        DesiredProps::Some(names) => {
            let mut out = Vec::with_capacity(names.len());
            for name in names {
                let Some(known) = crate::service_type::property_name(name) else {
                    return Err(Refusal::new(
                        RefusalKind::IllegalPropertyName,
                        format!(
                            "this query asked for a property {name:?}, which an offer does not \
                             carry: the properties are {FIELD_LIST}"
                        ),
                    ));
                };
                if out.contains(&known) {
                    return Err(Refusal::new(
                        RefusalKind::DuplicatePropertyName,
                        format!(
                            "this query asked for the property {name:?} twice: `desired_props` \
                             is a set, and asking twice would return it twice"
                        ),
                    ));
                }
                out.push(known);
            }
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Residency;
    use crate::service_type::{PropertyKind, PropertyMode, PropertySchema, ServiceType};

    fn offer(id: &str, cost: f64) -> Offer {
        Offer {
            id: id.to_owned(),
            specialization: Some("math".to_owned()),
            cost,
            latency_p50: Some(10.0),
            latency_p99: 20.0,
            load: 0.5,
            residency: Residency::Resident,
            mem_footprint: 1024,
            placement_node: "node-a".to_owned(),
            route_freq: 0,
        }
    }

    fn store_of(n: usize) -> TypedOfferStore {
        let mut s = TypedOfferStore::new();
        s.declare(
            ServiceType::declare(
                "moe::Expert",
                "IDL:moe/Expert:1.0",
                vec![PropertySchema::new(
                    "specialization",
                    PropertyKind::Text,
                    PropertyMode::Mandatory,
                )],
            )
            .unwrap(),
        )
        .unwrap();
        for i in 0..n {
            s.register("moe::Expert", offer(&format!("e{i:04}"), i as f64)).unwrap();
        }
        s
    }

    fn req<'a>(constraint: &'a str, how_many: u32) -> Request<'a> {
        Request {
            service_type: "moe::Expert",
            constraint,
            preference: "",
            policies: &[],
            desired: DesiredProps::All,
            how_many,
        }
    }

    #[test]
    fn an_answer_that_fits_carries_every_match_and_reports_no_limit() {
        let s = store_of(3);
        let a = s.answer(&req("cost < 10", 10)).unwrap();
        assert_eq!(a.offers.len(), 3);
        assert!(a.limits_applied.is_empty(), "nothing was clamped, so nothing is reported");
        assert_eq!(a.properties.len(), 10, "DesiredProps::All is all ten");
    }

    #[test]
    fn the_boundary_is_at_most_not_fewer_than() {
        let s = store_of(3);
        assert!(s.answer(&req("cost < 10", 3)).is_ok(), "matched == how_many fits");
        let e = s.answer(&req("cost < 10", 2)).unwrap_err();
        assert_eq!(e.kind, RefusalKind::DoesNotFit);
    }

    #[test]
    fn a_query_that_does_not_fit_is_refused_naming_the_iterator_and_both_bounds() {
        let s = store_of(5);
        let e = s.answer(&req("cost < 10", 2)).unwrap_err();
        assert_eq!(e.kind, RefusalKind::DoesNotFit);
        assert!(e.message.contains("CosTrading::OfferIterator"), "{}", e.message);
        assert!(e.message.contains("matched 5 offers"), "{}", e.message);
        assert!(e.message.contains("`how_many` was 2"), "{}", e.message);
        assert!(e.message.contains("at least 5"), "the way out is named: {}", e.message);
        assert!(
            e.message.contains(&format!("`max_return_card` is {MAX_RETURN_CARD}")),
            "the trader's own bound is quoted too: {}",
            e.message
        );
    }

    #[test]
    fn the_two_branches_of_the_refusal_are_different_sentences() {
        // Below the trader's own bound: raising `how_many` is the way out.
        let fits_if_widened = cannot_answer_completely(5, 2).expect("5 > 2");
        assert!(fits_if_widened.contains("re-ask with `how_many` at least 5"));

        // Above it: raising `how_many` cannot help, and the sentence says so.
        let beyond = cannot_answer_completely(MAX_RETURN_CARD as usize + 1, u32::MAX)
            .expect("beyond the bound, whatever how_many says");
        assert!(beyond.contains("raising `how_many` cannot help"), "{beyond}");
        assert!(!beyond.contains("re-ask with"), "{beyond}");

        assert_eq!(cannot_answer_completely(0, 0), None, "nothing matched, so nothing is missing");
        assert_eq!(cannot_answer_completely(512, u32::MAX), None, "the bound itself fits");
    }

    #[test]
    fn a_blank_constraint_means_every_offer_of_the_type_and_does_not_filter() {
        let mut s = store_of(2);
        // An offer with a gap in it: a blank constraint must still return it.
        let mut gapped = offer("z", 0.0);
        gapped.latency_p50 = None;
        s.register("moe::Expert", gapped).unwrap();

        let a = s.answer(&req("", 10)).unwrap();
        assert_eq!(a.offers.len(), 3, "a blank constraint filters nothing");
        assert!(a.offers.iter().any(|o| o.id == "z"));
        let a = s.answer(&req("   ", 10)).unwrap();
        assert_eq!(a.offers.len(), 3, "whitespace is blank too");
    }

    #[test]
    fn a_blank_preference_is_first_and_a_bad_one_is_refused() {
        let s = store_of(3);
        let mut r = req("", 10);
        r.preference = "";
        assert!(s.answer(&r).is_ok());

        r.preference = "MAX cost";
        let ordered = s.answer(&r).unwrap();
        assert_eq!(ordered.offers[0].id, "e0002", "MAX cost puts the dearest first");

        r.preference = "SOMEHOW";
        let e = s.answer(&r).unwrap_err();
        assert_eq!(e.kind, RefusalKind::IllegalPreference);
        assert!(e.message.contains("did not parse"), "{}", e.message);
    }

    #[test]
    fn refusals_come_in_the_order_the_specification_lists_them() {
        let s = store_of(1);
        let bad_everything = Request {
            service_type: "1illegal",
            constraint: "!!!",
            preference: "!!!",
            policies: &["exact_type_match".to_owned()],
            desired: DesiredProps::Some(vec!["nope".to_owned()]),
            how_many: 0,
        };
        assert_eq!(
            s.answer(&bad_everything).unwrap_err().kind,
            RefusalKind::IllegalServiceType,
            "the type name is checked first"
        );

        let mut r = bad_everything.clone();
        r.service_type = "moe::Unknown";
        assert_eq!(s.answer(&r).unwrap_err().kind, RefusalKind::UnknownServiceType);

        r.service_type = "moe::Expert";
        assert_eq!(s.answer(&r).unwrap_err().kind, RefusalKind::IllegalConstraint);

        r.constraint = "cost < 10";
        assert_eq!(s.answer(&r).unwrap_err().kind, RefusalKind::IllegalPreference);

        r.preference = "FIRST";
        let e = s.answer(&r).unwrap_err();
        assert_eq!(e.kind, RefusalKind::IllegalPolicyName);
        assert!(e.message.contains("exact_type_match"), "the policy is named: {}", e.message);
        assert!(e.message.contains("empty `PolicySeq`"), "the way out is named: {}", e.message);

        r.policies = &[];
        let e = s.answer(&r).unwrap_err();
        assert_eq!(e.kind, RefusalKind::IllegalPropertyName);
        assert!(e.message.contains("nope"), "{}", e.message);

        r.desired = DesiredProps::None;
        r.how_many = 10;
        let a = s.answer(&r).unwrap();
        assert!(a.properties.is_empty(), "DesiredProps::None projects nothing");
    }

    #[test]
    fn asking_for_the_same_property_twice_is_refused() {
        let s = store_of(1);
        let mut r = req("", 10);
        r.desired = DesiredProps::Some(vec!["cost".to_owned(), "cost".to_owned()]);
        assert_eq!(s.answer(&r).unwrap_err().kind, RefusalKind::DuplicatePropertyName);
    }

    #[test]
    fn some_properties_are_projected_in_the_order_asked_for() {
        let s = store_of(1);
        let mut r = req("", 10);
        r.desired = DesiredProps::Some(vec!["load".to_owned(), "cost".to_owned()]);
        assert_eq!(s.answer(&r).unwrap().properties, ["load", "cost"]);
    }

    #[test]
    fn a_gap_the_constraint_could_not_answer_about_is_reported_beside_the_answer() {
        let mut s = store_of(1);
        let mut gapped = offer("z", 0.0);
        gapped.specialization = None;
        // Registering it needs a type without the mandatory property.
        s.declare(ServiceType::declare("moe::Raw", "IDL:moe/Raw:1.0", vec![]).unwrap()).unwrap();
        s.register("moe::Raw", gapped).unwrap();

        let mut r = req("specialization == 'math'", 10);
        r.service_type = "moe::Raw";
        let a = s.answer(&r).unwrap();
        assert!(a.offers.is_empty());
        assert!(a.gap_note.is_some(), "the unanswerable offer is reported, not hidden");
    }
}
