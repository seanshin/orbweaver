//! `CosTrading::Lookup` on the wire, with a nil iterator.
//! `docs/decisions/D022` T4.
//!
//! # What this servant is for
//!
//! `PLAN-SERVICES` §3 deferred the standard `CosTrading` facade *until a
//! foreign trading client is named*, and D022 §2 reads a request to open the
//! service as that naming. The point of opening it is the oracle: an ORB that
//! is not ours resolving `TradingService` and calling `query`. So this file's
//! job is narrow — read CDR, ask [`orbweaver_trading::lookup`], write CDR —
//! and it decides nothing. Every judgement `query` makes is in that crate,
//! where it is tested without a socket, and the reason is the one `CLAUDE.md`
//! gives: the refusal a query that cannot be answered gets is a sentence more
//! than one layer says, so it belongs to one function that all of them call.
//!
//! # The iterator, and why there is not one
//!
//! `query` declares `out OfferIterator offer_itr`. **This servant always
//! writes nil there, and never truncates to make that true.** An
//! `OfferIterator` is a POA-hosted object per query with a lifecycle, which is
//! the reference-outliving-its-value hazard `COMPONENTS.md` records as
//! deliberately not built for `DynAny`; D022 §7 forbids one until a query that
//! cannot fit is named. The specification's escape is that when the matches
//! are at most `how_many`, all of them go in `offers` and `offer_itr` is nil —
//! so a nil iterator here always means *that is all of them*, and a query
//! whose answer would not fit is refused instead. See
//! [`orbweaver_trading::lookup`] for the argument and the sentence.
//!
//! *반복자는 언제나 nil이고, 그것을 참으로 만들기 위해 잘라내지 않는다. nil
//! 반복자는 언제나 "이것이 전부"를 뜻한다.*
//!
//! # How a refusal reaches the client
//!
//! Nine of the ten exceptions `query` declares are user exceptions and are
//! raised as such, each carrying the offending string the IDL says it carries.
//! The tenth case is not one of the ten: a query whose answer does not fit is
//! refused with **`NO_IMPLEMENT`**, following this workspace's own rule
//! (`server.rs`) that `BAD_OPERATION` means *the contract does not declare
//! this* and `NO_IMPLEMENT` means *declared and deliberately not served*. The
//! `OfferIterator` is declared by the contract and deliberately not built, so
//! `NO_IMPLEMENT` is the sentence in system-exception form.
//!
//! **A system exception carries no text**, which is the honest limitation of
//! that choice, so the bound is put where a client can read it instead: `Lookup`
//! inherits `ImportAttributes`, and [`ImportAttributes::max_return_card`]
//! — answered here from [`orbweaver_trading::lookup::MAX_RETURN_CARD`], the same
//! constant the refusal sentence quotes — is the specification's own name for
//! exactly this number. A client can ask the trader its bound before it asks a
//! question, and this servant logs the full sentence on the refusing path.
//!
//! [`ImportAttributes::max_return_card`]: self#attributes
//!
//! # What this trader admits about itself / 이 트레이더가 자신에 대해 말하는 것
//!
//! `Lookup` inherits three attribute interfaces and every one of their twenty
//! attributes is answered, because a trader that does not answer them is a
//! trader a foreign client cannot characterise. The answers are what D022 §7
//! requires and are on the wire rather than in a comment:
//!
//! - **`TraderComponents`** — `lookup_if` is this object; `register_if`,
//!   `link_if`, `proxy_if` and `admin_if` are **nil**, which is the
//!   specification's own way of saying an interface is not supported.
//! - **`SupportAttributes`** — `supports_modifiable_properties`,
//!   `supports_dynamic_properties` and `supports_proxy_offers` are all
//!   **false**, and `type_repos` is **nil**: D022 §7 forbids dynamic
//!   properties, proxy offers, and a `ServiceTypeRepository` servant, and this
//!   is where a client finds that out.
//! - **`ImportAttributes`** — the cardinalities come from
//!   [`orbweaver_trading::lookup`]'s constants, and `def_hop_count`,
//!   `max_hop_count`, `def_follow_policy` and `max_follow_policy` say what a
//!   trader with **no links** must say: zero hops, `local_only`.
//!
//! # Two things this servant cannot do, named rather than left silent
//!
//! - **It does not decode a policy's `any`.** `CosTrading::Policy` is
//!   `{ PolicyName name; any value; }` inside a sequence, so the `any` is
//!   never the last thing in the request body, and CDR gives an `any` no
//!   length prefix — skipping one requires walking its `TypeCode`, which this
//!   crate has no walker for. The servant therefore reads the sequence length
//!   and the **first** policy's name and refuses there, which is sound because
//!   *every* policy name is refused: no value is ever needed to decide.
//!   Implementing an import policy means writing that walk first.
//! - **An offer's `reference` is nil unless the deployment supplied one.**
//!   `CosTrading::Offer` carries an `Object`, and this project's offers are
//!   capability descriptors rather than object references — `orbweaver-trading`
//!   records that selection results cross the MCP face as capability handles,
//!   never IORs (integration point IF1). So the servant does not invent one:
//!   [`TradingServer::set_reference`] lets a deployment bind an IOR to an offer
//!   id, and an offer with none gets the nil reference and its identity in the
//!   `id` property.

use std::collections::BTreeMap;

use orbweaver_cdr::Encoder;
use orbweaver_trading::lookup::{
    Answer, DesiredProps, MAX_MATCH_CARD, MAX_RETURN_CARD, MAX_SEARCH_CARD, Request,
};
use orbweaver_trading::service_type::{Refusal, RefusalKind, TypedOfferStore};
use orbweaver_trading::{Offer, Residency};

use crate::guarded::Guarded;
use crate::server::{
    Dispatch, DispatchBody, Request as GiopRequest, SharedDispatch, SystemException,
};
use crate::typecode::{self, TypeCode};
use crate::{IiopProfile, Ior, Version, codeset};

/// `CosTrading::Lookup`.
pub const LOOKUP_ID: &str = "IDL:omg.org/CosTrading/Lookup:1.0";
/// `CosTrading::TraderComponents`, which `Lookup` inherits.
pub const TRADER_COMPONENTS_ID: &str = "IDL:omg.org/CosTrading/TraderComponents:1.0";
/// `CosTrading::SupportAttributes`, which `Lookup` inherits.
pub const SUPPORT_ATTRIBUTES_ID: &str = "IDL:omg.org/CosTrading/SupportAttributes:1.0";
/// `CosTrading::ImportAttributes`, which `Lookup` inherits.
pub const IMPORT_ATTRIBUTES_ID: &str = "IDL:omg.org/CosTrading/ImportAttributes:1.0";

/// `CosTrading::IllegalServiceType`.
pub const ILLEGAL_SERVICE_TYPE_ID: &str = "IDL:omg.org/CosTrading/IllegalServiceType:1.0";
/// `CosTrading::UnknownServiceType`.
pub const UNKNOWN_SERVICE_TYPE_ID: &str = "IDL:omg.org/CosTrading/UnknownServiceType:1.0";
/// `CosTrading::IllegalConstraint`.
pub const ILLEGAL_CONSTRAINT_ID: &str = "IDL:omg.org/CosTrading/IllegalConstraint:1.0";
/// `CosTrading::IllegalPropertyName`.
pub const ILLEGAL_PROPERTY_NAME_ID: &str = "IDL:omg.org/CosTrading/IllegalPropertyName:1.0";
/// `CosTrading::DuplicatePropertyName`.
pub const DUPLICATE_PROPERTY_NAME_ID: &str = "IDL:omg.org/CosTrading/DuplicatePropertyName:1.0";
/// `CosTrading::Lookup::IllegalPreference` — nested in `Lookup`, so its
/// repository id carries the interface as a scope. Getting this wrong is
/// invisible to us and fatal to a client, which is why every one of these ids
/// is checked against the omniORB stubs by `spikes/trading_client.py`.
pub const ILLEGAL_PREFERENCE_ID: &str = "IDL:omg.org/CosTrading/Lookup/IllegalPreference:1.0";
/// `CosTrading::Lookup::IllegalPolicyName`.
pub const ILLEGAL_POLICY_NAME_ID: &str = "IDL:omg.org/CosTrading/Lookup/IllegalPolicyName:1.0";

/// `moe::Residency`, the `TypeCode` a residency property's `any` carries.
pub const RESIDENCY_ID: &str = "IDL:moe/Residency:1.0";

/// `HowManyProps::none`, the `SpecifiedProps` discriminator.
const HOW_MANY_NONE: u32 = 0;
/// `HowManyProps::some`.
const HOW_MANY_SOME: u32 = 1;
/// `HowManyProps::all`.
const HOW_MANY_ALL: u32 = 2;
/// `FollowOption::local_only` — the only one a trader with no links can mean.
const FOLLOW_LOCAL_ONLY: u32 = 0;

/// The user exceptions this servant raises. Every one carries exactly one
/// string, which is what the IDL declares for each.
#[derive(Debug, Clone)]
enum UserExc {
    IllegalServiceType(String),
    UnknownServiceType(String),
    IllegalConstraint(String),
    IllegalPropertyName(String),
    DuplicatePropertyName(String),
    IllegalPreference(String),
    IllegalPolicyName(String),
}

impl UserExc {
    /// Writes the exception body: repository id first, then the member.
    fn write(&self, out: &mut Encoder) {
        let (id, member) = match self {
            UserExc::IllegalServiceType(s) => (ILLEGAL_SERVICE_TYPE_ID, s),
            UserExc::UnknownServiceType(s) => (UNKNOWN_SERVICE_TYPE_ID, s),
            UserExc::IllegalConstraint(s) => (ILLEGAL_CONSTRAINT_ID, s),
            UserExc::IllegalPropertyName(s) => (ILLEGAL_PROPERTY_NAME_ID, s),
            UserExc::DuplicatePropertyName(s) => (DUPLICATE_PROPERTY_NAME_ID, s),
            UserExc::IllegalPreference(s) => (ILLEGAL_PREFERENCE_ID, s),
            UserExc::IllegalPolicyName(s) => (ILLEGAL_POLICY_NAME_ID, s),
        };
        out.put_str(id);
        out.put_str(member);
    }
}

/// A failure a handler raises.
enum Raise {
    User(UserExc),
    System(SystemException),
}

impl From<UserExc> for Raise {
    fn from(e: UserExc) -> Self {
        Raise::User(e)
    }
}

impl From<SystemException> for Raise {
    fn from(e: SystemException) -> Self {
        Raise::System(e)
    }
}

/// A `MARSHAL` for arguments that did not decode.
fn marshal() -> Raise {
    Raise::System(SystemException::marshal())
}

/// The nil object reference: empty type id, no profiles (§9.3.6).
fn nil_ref() -> Ior {
    Ior { type_id: String::new(), profiles: Vec::new() }
}

/// What a [`Refusal`] becomes on the wire.
///
/// **Classifies on [`RefusalKind`], never on the message text.** The sentences
/// belong to `orbweaver-trading` and will change; the kinds are the value that
/// crate publishes so that a classifier here does not have to retype half a
/// sentence and go quietly false — `CLAUDE.md`, *a classifier is a sentence
/// too*. The `match` is exhaustive on purpose: a new refusal class added over
/// there fails to compile here rather than falling into a catch-all.
///
/// `arg` is the request argument the refusal is about, because every one of
/// these exceptions carries the offending string and a client reads *that*,
/// not our prose.
fn raise_for(refusal: &Refusal, req: &DecodedQuery) -> Raise {
    match refusal.kind {
        RefusalKind::IllegalServiceType => {
            UserExc::IllegalServiceType(req.service_type.clone()).into()
        }
        RefusalKind::UnknownServiceType => {
            UserExc::UnknownServiceType(req.service_type.clone()).into()
        }
        RefusalKind::IllegalConstraint => UserExc::IllegalConstraint(req.constraint.clone()).into(),
        RefusalKind::IllegalPreference => UserExc::IllegalPreference(req.preference.clone()).into(),
        RefusalKind::IllegalPolicyName => {
            UserExc::IllegalPolicyName(req.policies.first().cloned().unwrap_or_default()).into()
        }
        RefusalKind::IllegalPropertyName => {
            UserExc::IllegalPropertyName(req.offending_property()).into()
        }
        RefusalKind::DuplicatePropertyName => {
            UserExc::DuplicatePropertyName(req.offending_property()).into()
        }
        // The answer does not fit, so it would need the `OfferIterator` this
        // trader does not create. `NO_IMPLEMENT`, per this workspace's rule:
        // declared by the contract, deliberately not served.
        RefusalKind::DoesNotFit => Raise::System(SystemException::no_implement()),
        // Registration-time classes. `query` registers nothing, so reaching
        // one of these means the engine grew a path this arm has not been
        // taught, and `INTERNAL` says exactly that rather than guessing at a
        // `CosTrading` exception that would mislead the client.
        RefusalKind::DuplicateServiceType
        | RefusalKind::PropertyTypeMismatch
        | RefusalKind::MissingMandatoryProperty
        | RefusalKind::ReadonlyPropertyModified
        | RefusalKind::UnsupportedSuperTypes
        | RefusalKind::Store => Raise::System(SystemException::internal()),
    }
}

/// One decoded `query` request. Kept whole because the exceptions carry the
/// caller's own strings back, so the servant needs them after the engine has
/// refused.
#[derive(Debug)]
struct DecodedQuery {
    service_type: String,
    constraint: String,
    preference: String,
    policies: Vec<String>,
    desired: DesiredProps,
    how_many: u32,
    /// Whether decoding stopped at a policy's `any`, leaving `desired` and
    /// `how_many` as placeholders rather than as the caller's values. See
    /// [`decode_query`] and the module header's first named limitation.
    stopped_at_a_policy: bool,
}

impl DecodedQuery {
    /// The property name a property refusal is about. The engine refuses at
    /// the first offending name and its message quotes it; taking the first
    /// requested name would be wrong, so this finds the one the engine
    /// rejected the same way the engine did — by asking which names are not
    /// projectable, in order.
    fn offending_property(&self) -> String {
        let DesiredProps::Some(names) = &self.desired else {
            return String::new();
        };
        let mut seen: Vec<&str> = Vec::new();
        for name in names {
            match orbweaver_trading::service_type::property_name(name) {
                None => return name.clone(),
                Some(known) if seen.contains(&known) => return name.clone(),
                Some(known) => seen.push(known),
            }
        }
        String::new()
    }
}

/// The state one lock covers.
#[derive(Debug, Default)]
struct State {
    store: TypedOfferStore,
    references: BTreeMap<String, Ior>,
}

/// A `CosTrading::Lookup` servant over an [`TypedOfferStore`].
#[derive(Debug)]
pub struct TradingServer {
    host: String,
    port: u16,
    key: Vec<u8>,
    state: Guarded<State>,
}

impl TradingServer {
    /// A trader serving one object key, reachable at `host:port`.
    pub fn new(host: impl Into<String>, port: u16, key: Vec<u8>) -> TradingServer {
        TradingServer {
            host: host.into(),
            port,
            key,
            state: Guarded::new("the trader's offers", State::default()),
        }
    }

    /// The object key this servant answers for — what
    /// [`Server::bind`](crate::server::Server::bind) must be given.
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    /// The reference to publish, as `TradingService` or otherwise.
    ///
    /// Nothing registers this: `Orb::register_initial_reference` is the
    /// deployment's call, and until a deployment makes it the reserved
    /// `TradingService` slot goes on refusing by name. That is D022 §8's
    /// position and it stays true — what changes with T4 is that there is now
    /// something to register.
    pub fn lookup_ior(&self) -> Ior {
        Ior {
            type_id: LOOKUP_ID.to_owned(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: self.host.clone(),
                port: self.port,
                object_key: self.key.clone(),
                components: vec![codeset::server_component()],
            }],
        }
    }

    /// Reads or writes the offer store under the servant's lock.
    ///
    /// The only way in: registration and type declaration are
    /// `orbweaver-trading`'s API, not this servant's, because `Lookup` is the
    /// import side of the trader and `Register` — the export side — is one of
    /// the interfaces this trader answers `nil` for. A deployment populates
    /// the store through here; a client cannot.
    pub fn with_store<R>(&self, f: impl FnOnce(&mut TypedOfferStore) -> R) -> R {
        self.state.write(|s| f(&mut s.store))
    }

    /// Binds an object reference to an offer id, so that `query` returns
    /// something importable for it. See the module header's second named
    /// limitation for why this is the deployment's job.
    pub fn set_reference(&self, offer_id: impl Into<String>, ior: Ior) {
        self.state.write(|s| {
            s.references.insert(offer_id.into(), ior);
        });
    }

    fn handle(&self, req: &GiopRequest, out: &mut Encoder) -> Result<(), Raise> {
        let mut args = req.body().map_err(|_| marshal())?;
        match req.operation.as_str() {
            "_is_a" => {
                let id = args.get_string().map_err(|_| marshal())?;
                out.put_bool(matches!(
                    id.as_str(),
                    LOOKUP_ID
                        | TRADER_COMPONENTS_ID
                        | SUPPORT_ATTRIBUTES_ID
                        | IMPORT_ATTRIBUTES_ID
                        | "IDL:omg.org/CORBA/Object:1.0"
                ));
            }
            "_non_existent" => out.put_bool(false),

            "query" => {
                let decoded = decode_query(&mut args)?;
                let request = Request {
                    service_type: &decoded.service_type,
                    constraint: &decoded.constraint,
                    preference: &decoded.preference,
                    policies: &decoded.policies,
                    desired: decoded.desired.clone(),
                    how_many: decoded.how_many,
                };
                // Written from inside the read section, as the naming
                // servant's `list` is: the answer borrows the store, so
                // copying it out to write outside the lock would clone every
                // offer to save a lock other readers can hold anyway.
                self.state.read(|s| {
                    let answer = match s.store.answer(&request) {
                        Ok(a) => a,
                        Err(refusal) => {
                            if refusal.kind == RefusalKind::DoesNotFit {
                                // A system exception carries no text, so the
                                // sentence is logged rather than lost. See the
                                // module header.
                                eprintln!("orbweaver trader refused a query: {refusal}");
                            }
                            return Err(raise_for(&refusal, &decoded));
                        }
                    };
                    // A request whose decoding stopped at a policy must have
                    // been refused for that policy. If it was not, the
                    // engine's order of checks moved and `desired`/`how_many`
                    // above are placeholders, not the caller's values —
                    // answering would be answering a question nobody asked.
                    if decoded.stopped_at_a_policy {
                        return Err(Raise::System(SystemException::internal()));
                    }
                    write_answer(out, &answer, &s.references)?;
                    Ok::<(), Raise>(())
                })?;
            }

            // TraderComponents. `lookup_if` is this object; the four
            // interfaces this trader does not have are nil, which is how the
            // specification says "not supported".
            "_get_lookup_if" => self.lookup_ior().write_to(out).map_err(|_| marshal())?,
            "_get_register_if" | "_get_link_if" | "_get_proxy_if" | "_get_admin_if" => {
                nil_ref().write_to(out).map_err(|_| marshal())?
            }

            // SupportAttributes. All three false and `type_repos` nil: D022 §7
            // forbids dynamic properties, proxy offers and a
            // ServiceTypeRepository servant, and this is where a client is
            // told so.
            "_get_supports_modifiable_properties"
            | "_get_supports_dynamic_properties"
            | "_get_supports_proxy_offers" => out.put_bool(false),
            "_get_type_repos" => nil_ref().write_to(out).map_err(|_| marshal())?,

            // ImportAttributes. Every cardinality comes from the engine's own
            // constants, so the attribute a client reads and the bound a
            // refusal quotes cannot disagree.
            "_get_def_search_card" | "_get_max_search_card" => out.put_u32(MAX_SEARCH_CARD),
            "_get_def_match_card" | "_get_max_match_card" => out.put_u32(MAX_MATCH_CARD),
            "_get_def_return_card" | "_get_max_return_card" | "_get_max_list" => {
                out.put_u32(MAX_RETURN_CARD)
            }
            // No links: zero hops, and the only follow policy that can mean
            // anything is `local_only`.
            "_get_def_hop_count" | "_get_max_hop_count" => out.put_u32(0),
            "_get_def_follow_policy" | "_get_max_follow_policy" => out.put_u32(FOLLOW_LOCAL_ONLY),

            // Every operation and attribute `Lookup` and its three bases
            // declare has an arm above, so a name reaching here is one the
            // contract does not declare — including `_set_` on any of the
            // twenty, every one of which is `readonly`.
            _ => return Err(SystemException::bad_operation().into()),
        }
        Ok(())
    }
}

/// Decodes `query`'s six in-parameters.
fn decode_query(args: &mut orbweaver_cdr::Decoder<'_>) -> Result<DecodedQuery, Raise> {
    let service_type = args.get_string().map_err(|_| marshal())?;
    let constraint = args.get_string().map_err(|_| marshal())?;
    let preference = args.get_string().map_err(|_| marshal())?;

    // `sequence<Policy>`, `Policy = { PolicyName name; any value; }`. Only the
    // first name is read and no `any` is ever decoded — see the module
    // header's first named limitation. Sound because every policy name is
    // refused, so no value can change the answer.
    let count = args.get_u32().map_err(|_| marshal())?;
    // A `Policy` is a string length plus a `TypeCode` kind: eight bytes at the
    // very least. Without this a length prefix of 2^32-1 is an allocation.
    args.validate_count(count, 8).map_err(|_| marshal())?;
    let mut policies = Vec::new();
    if count > 0 {
        policies.push(args.get_string().map_err(|_| marshal())?);
        // **Decoding stops here**, and it has to. The stream is now positioned
        // at that policy's `any`, which cannot be skipped without walking its
        // `TypeCode`, so `desired_props` and `how_many` behind it are
        // unreadable. They are given placeholders and the request is marked:
        // the engine refuses at the policy name before it consults either, and
        // `handle` refuses to answer a marked request that somehow got past it
        // rather than trusting that ordering to stay true.
        return Ok(DecodedQuery {
            service_type,
            constraint,
            preference,
            policies,
            desired: DesiredProps::All,
            how_many: 0,
            stopped_at_a_policy: true,
        });
    }

    // `union SpecifiedProps switch (HowManyProps) { case some: PropertyNameSeq prop_names; };`
    let desired = match args.get_u32().map_err(|_| marshal())? {
        HOW_MANY_NONE => DesiredProps::None,
        HOW_MANY_ALL => DesiredProps::All,
        HOW_MANY_SOME => {
            let n = args.get_u32().map_err(|_| marshal())?;
            let n = args.validate_count(n, 4).map_err(|_| marshal())?;
            let mut names = Vec::with_capacity(n);
            for _ in 0..n {
                names.push(args.get_string().map_err(|_| marshal())?);
            }
            DesiredProps::Some(names)
        }
        // A discriminator outside the enum. The union has no default arm, so
        // there is nothing to decode and nothing to guess.
        _ => return Err(marshal()),
    };

    let how_many = args.get_u32().map_err(|_| marshal())?;
    Ok(DecodedQuery {
        service_type,
        constraint,
        preference,
        policies,
        desired,
        how_many,
        stopped_at_a_policy: false,
    })
}

/// Writes `query`'s three out-parameters: `offers`, then the **nil**
/// `offer_itr`, then an empty `limits_applied`.
fn write_answer(
    out: &mut Encoder,
    answer: &Answer<'_>,
    references: &BTreeMap<String, Ior>,
) -> Result<(), Raise> {
    out.put_u32(answer.offers.len() as u32);
    for offer in &answer.offers {
        match references.get(&offer.id) {
            Some(ior) => ior.write_to(out).map_err(|_| marshal())?,
            None => nil_ref().write_to(out).map_err(|_| marshal())?,
        }
        write_properties(out, offer, &answer.properties)?;
    }

    // The nil iterator, which here always means "that is all of them".
    nil_ref().write_to(out).map_err(|_| marshal())?;

    out.put_u32(answer.limits_applied.len() as u32);
    for name in &answer.limits_applied {
        out.put_str(name);
    }
    Ok(())
}

/// Writes an offer's `PropertySeq`, skipping the properties it does not carry.
///
/// Skipping rather than writing an empty value is the point: `CosTrading`'s
/// `PropertySeq` lists what an offer *has*, so an absent `specialization` is
/// an absent property and not the empty string. The engine's `Option` fields
/// and the wire's sequence say the same thing, and `EXIST specialization` in a
/// constraint asks about exactly this.
fn write_properties(out: &mut Encoder, offer: &Offer, wanted: &[&str]) -> Result<(), Raise> {
    let present: Vec<&str> = wanted.iter().copied().filter(|n| carries(offer, n)).collect();
    out.put_u32(present.len() as u32);
    for name in present {
        out.put_str(name);
        write_property_value(out, offer, name)?;
    }
    Ok(())
}

/// Whether the offer carries a value for this property. Only the two `Option`
/// fields can answer `false`.
fn carries(offer: &Offer, name: &str) -> bool {
    match name {
        "specialization" => offer.specialization.is_some(),
        "latency_p50" => offer.latency_p50.is_some(),
        _ => true,
    }
}

/// Writes one property value as a `CosTrading::PropertyValue`, which is an
/// `any`.
///
/// Uses [`typecode::encode_any_with`] rather than building the value in a side
/// buffer, because an `any`'s value is aligned from where the whole `any`
/// lands and a property is never at offset zero.
fn write_property_value(out: &mut Encoder, offer: &Offer, name: &str) -> Result<(), Raise> {
    let residency_tc = || TypeCode::Enum {
        id: RESIDENCY_ID.to_owned(),
        name: "Residency".to_owned(),
        members: vec![
            "OFFLOADED".to_owned(),
            "PREFETCHING".to_owned(),
            "RESIDENT".to_owned(),
            "ACTIVE".to_owned(),
        ],
    };
    let r = match name {
        "id" => text(out, &offer.id),
        "specialization" => text(out, offer.specialization.as_deref().unwrap_or_default()),
        "placement_node" => text(out, &offer.placement_node),
        "cost" => float(out, offer.cost),
        "latency_p50" => float(out, offer.latency_p50.unwrap_or_default()),
        "latency_p99" => float(out, offer.latency_p99),
        "load" => float(out, offer.load),
        "mem_footprint" => counter(out, offer.mem_footprint),
        "route_freq" => counter(out, offer.route_freq),
        "residency" => typecode::encode_any_with(out, &residency_tc(), |e| {
            e.put_u32(match offer.residency {
                Residency::Offloaded => 0,
                Residency::Prefetching => 1,
                Residency::Resident => 2,
                Residency::Active => 3,
            })
        }),
        // `write_properties` filters on the same closed list the engine
        // publishes, so this is unreachable through the wire path; `INTERNAL`
        // rather than a panic, because a servant thread that unwinds takes the
        // connection with it.
        _ => return Err(Raise::System(SystemException::internal())),
    };
    r.map_err(|_| marshal())
}

fn text(out: &mut Encoder, v: &str) -> crate::Result<()> {
    typecode::encode_any_with(out, &TypeCode::String(0), |e| e.put_str(v))
}

fn float(out: &mut Encoder, v: f64) -> crate::Result<()> {
    typecode::encode_any_with(out, &TypeCode::Double, |e| e.put_f64(v))
}

fn counter(out: &mut Encoder, v: u64) -> crate::Result<()> {
    typecode::encode_any_with(out, &TypeCode::ULongLong, |e| e.put_u64(v))
}

impl SharedDispatch for TradingServer {
    fn knows(&self, object_key: &[u8]) -> bool {
        object_key == self.key
    }

    fn dispatch_body(
        &self,
        request: &GiopRequest,
        out: &mut Encoder,
    ) -> std::result::Result<DispatchBody, SystemException> {
        match self.handle(request, out) {
            Ok(()) => Ok(DispatchBody::Return),
            Err(Raise::System(ex)) => Err(ex),
            Err(Raise::User(ex)) => {
                ex.write(out);
                Ok(DispatchBody::UserException)
            }
        }
    }

    fn dispatch(
        &self,
        request: &GiopRequest,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        match self.dispatch_body(request, out)? {
            DispatchBody::Return => Ok(()),
            DispatchBody::UserException => Err(SystemException::unknown_user_exception()),
        }
    }
}

/// The `&mut self` shape as well, forwarding to the shared one so there is
/// exactly one implementation of the trading semantics.
impl Dispatch for TradingServer {
    fn knows(&self, object_key: &[u8]) -> bool {
        SharedDispatch::knows(self, object_key)
    }

    fn dispatch_body(
        &mut self,
        request: &GiopRequest,
        out: &mut Encoder,
    ) -> std::result::Result<DispatchBody, SystemException> {
        SharedDispatch::dispatch_body(self, request, out)
    }

    fn dispatch(
        &mut self,
        request: &GiopRequest,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        SharedDispatch::dispatch(self, request, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{Server, decode_request};
    use crate::{DEFAULT_MAX_MESSAGE_SIZE, Error};
    use orbweaver_cdr::{Decoder, Endian};
    use orbweaver_trading::service_type::{
        PropertyKind, PropertyMode, PropertySchema, ServiceType,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    const T: Duration = Duration::from_secs(5);
    const KEY: &[u8] = b"TradingService";

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
            route_freq: 7,
        }
    }

    /// A trader with one declared type and `n` offers of it.
    fn trader(host: &str, port: u16, n: usize) -> TradingServer {
        let t = TradingServer::new(host, port, KEY.to_vec());
        t.with_store(|s| {
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
        });
        t
    }

    /// Dispatches one request straight at the servant, with no socket.
    fn call(
        t: &TradingServer,
        op: &str,
        endian: Endian,
        version: Version,
        write: impl Fn(&mut Encoder),
    ) -> std::result::Result<(DispatchBody, Vec<u8>), SystemException> {
        let wire = crate::encode_request(version, endian, 1, KEY, op, true, write).unwrap();
        let msg = crate::read_message(&mut &wire[..], DEFAULT_MAX_MESSAGE_SIZE).unwrap();
        let req = decode_request(msg).unwrap();
        let mut out = Encoder::new(endian);
        let body = SharedDispatch::dispatch_body(t, &req, &mut out)?;
        Ok((body, out.finish().unwrap()))
    }

    /// Writes `query`'s six in-parameters.
    #[allow(clippy::too_many_arguments)]
    fn write_query(
        e: &mut Encoder,
        service_type: &str,
        constraint: &str,
        preference: &str,
        policies: &[&str],
        desired: u32,
        prop_names: &[&str],
        how_many: u32,
    ) {
        e.put_str(service_type);
        e.put_str(constraint);
        e.put_str(preference);
        e.put_u32(policies.len() as u32);
        for p in policies {
            e.put_str(p);
            // `Policy::value`, an `any`. The servant refuses at the name and
            // never reads this, which is what makes writing one here a test.
            typecode::encode_any_with(e, &TypeCode::ULong, |x| x.put_u32(0)).unwrap();
        }
        e.put_u32(desired);
        if desired == HOW_MANY_SOME {
            e.put_u32(prop_names.len() as u32);
            for n in prop_names {
                e.put_str(n);
            }
        }
        e.put_u32(how_many);
    }

    fn plain_query(service_type: &str, how_many: u32) -> impl Fn(&mut Encoder) + use<'_> {
        move |e| write_query(e, service_type, "", "", &[], HOW_MANY_ALL, &[], how_many)
    }

    /// `query`'s three out-parameters, read back exactly as a generated client
    /// would.
    struct Decoded {
        offers: Vec<(Ior, Vec<String>)>,
        iterator: Ior,
        limits_applied: Vec<String>,
    }

    fn read_answer(raw: &[u8], endian: Endian) -> Decoded {
        read_answer_from(&mut Decoder::new(raw, endian))
    }

    /// Reads the answer out of a decoder already positioned at the body.
    ///
    /// Takes the decoder rather than bytes because an `any`'s value is aligned
    /// from where the whole `any` lands: a reply read out of a GIOP message
    /// starts at the body offset, not at zero, and copying the bytes to a
    /// fresh buffer first would decode the padding from the wrong origin.
    fn read_answer_from(d: &mut Decoder<'_>) -> Decoded {
        let n = d.get_u32().unwrap() as usize;
        let mut offers = Vec::with_capacity(n);
        for _ in 0..n {
            let reference = Ior::read_from(d).unwrap();
            let pn = d.get_u32().unwrap() as usize;
            let mut names = Vec::with_capacity(pn);
            for _ in 0..pn {
                names.push(d.get_string().unwrap());
                // Skip the `any`: its TypeCode, then its value.
                match typecode::decode(d).unwrap() {
                    TypeCode::String(_) => {
                        d.get_string().unwrap();
                    }
                    TypeCode::Double => {
                        d.get_f64().unwrap();
                    }
                    TypeCode::ULongLong => {
                        d.get_u64().unwrap();
                    }
                    TypeCode::Enum { .. } => {
                        d.get_u32().unwrap();
                    }
                    other => panic!("a property carried an unexpected TypeCode: {other:?}"),
                }
            }
            offers.push((reference, names));
        }
        let iterator = Ior::read_from(d).unwrap();
        let ln = d.get_u32().unwrap() as usize;
        let limits_applied = (0..ln).map(|_| d.get_string().unwrap()).collect();
        assert_eq!(d.remaining(), 0, "the reply body was fully consumed");
        Decoded { offers, iterator, limits_applied }
    }

    fn user_exception(body: DispatchBody, raw: &[u8], endian: Endian) -> (String, String) {
        assert!(matches!(body, DispatchBody::UserException), "expected a user exception");
        let mut d = Decoder::new(raw, endian);
        (d.get_string().unwrap(), d.get_string().unwrap())
    }

    /// The shape D022 §5 argues for, over both byte orders and both GIOP
    /// layouts: every match in `offers`, and a nil iterator that therefore
    /// means "that is all of them".
    #[test]
    fn an_answer_that_fits_returns_every_match_with_a_nil_iterator() {
        for version in [Version::V1_0, Version::V1_2] {
            for endian in [Endian::Big, Endian::Little] {
                let t = trader("127.0.0.1", 1, 3);
                let (body, raw) =
                    call(&t, "query", endian, version, plain_query("moe::Expert", 10)).unwrap();
                assert!(matches!(body, DispatchBody::Return), "{version:?}/{endian:?}");
                let got = read_answer(&raw, endian);
                assert_eq!(got.offers.len(), 3, "{version:?}/{endian:?}");
                assert!(got.iterator.is_nil(), "{version:?}/{endian:?}: the iterator is nil");
                assert!(
                    got.limits_applied.is_empty(),
                    "{version:?}/{endian:?}: nothing was clamped, so nothing is reported"
                );
                assert_eq!(got.offers[0].1.len(), 10, "all ten properties");
            }
        }
    }

    /// The refusal D022 §5 requires. The choice of exception is this
    /// workspace's own rule: `NO_IMPLEMENT` is *declared and deliberately not
    /// served*, which is exactly what the `OfferIterator` is.
    #[test]
    fn a_query_that_does_not_fit_is_refused_and_no_truncated_answer_is_written() {
        let t = trader("127.0.0.1", 1, 5);
        let err = call(&t, "query", Endian::Little, Version::V1_2, plain_query("moe::Expert", 2))
            .unwrap_err();
        assert_eq!(err.id, crate::server::NO_IMPLEMENT, "{err:?}");
    }

    /// The bound the refusal is about is readable from the wire, and it is the
    /// *same constant* — a client that reads `max_return_card` and a client
    /// that reads the refusal cannot be told different numbers.
    #[test]
    fn max_return_card_reports_the_bound_the_refusal_is_about() {
        let t = trader("127.0.0.1", 1, 0);
        for op in ["_get_max_return_card", "_get_def_return_card", "_get_max_list"] {
            let (_, raw) = call(&t, op, Endian::Little, Version::V1_2, |_| {}).unwrap();
            let got = Decoder::new(&raw, Endian::Little).get_u32().unwrap();
            assert_eq!(got, MAX_RETURN_CARD, "{op}");
        }
        let said = orbweaver_trading::lookup::cannot_answer_completely(9, 1).unwrap();
        assert!(said.contains(&format!("`max_return_card` is {MAX_RETURN_CARD}")), "{said}");
    }

    /// D022 §7's prohibitions, as a foreign client learns them.
    #[test]
    fn a_trader_with_no_links_no_proxies_and_no_repository_says_so_on_the_wire() {
        let t = trader("127.0.0.1", 4242, 0);

        for op in [
            "_get_supports_modifiable_properties",
            "_get_supports_dynamic_properties",
            "_get_supports_proxy_offers",
        ] {
            let (_, raw) = call(&t, op, Endian::Little, Version::V1_2, |_| {}).unwrap();
            assert!(!Decoder::new(&raw, Endian::Little).get_bool().unwrap(), "{op}");
        }

        // The four interfaces this trader does not have, plus the type
        // repository D022 §7 forbids: nil, which is how the specification
        // spells "not supported".
        for op in [
            "_get_register_if",
            "_get_link_if",
            "_get_proxy_if",
            "_get_admin_if",
            "_get_type_repos",
        ] {
            let (_, raw) = call(&t, op, Endian::Little, Version::V1_2, |_| {}).unwrap();
            let ior = Ior::read_from(&mut Decoder::new(&raw, Endian::Little)).unwrap();
            assert!(ior.is_nil(), "{op} must be nil");
        }

        // `lookup_if` is this object, and usable rather than merely non-nil.
        let (_, raw) = call(&t, "_get_lookup_if", Endian::Little, Version::V1_2, |_| {}).unwrap();
        let me = Ior::read_from(&mut Decoder::new(&raw, Endian::Little)).unwrap();
        assert_eq!(me.type_id, LOOKUP_ID);
        assert_eq!(me.primary().unwrap().port, 4242);
        assert_eq!(me.primary().unwrap().object_key, KEY);

        // No links: zero hops, and `local_only` is the only follow policy that
        // can mean anything.
        for op in ["_get_def_hop_count", "_get_max_hop_count"] {
            let (_, raw) = call(&t, op, Endian::Little, Version::V1_2, |_| {}).unwrap();
            assert_eq!(Decoder::new(&raw, Endian::Little).get_u32().unwrap(), 0, "{op}");
        }
        for op in ["_get_def_follow_policy", "_get_max_follow_policy"] {
            let (_, raw) = call(&t, op, Endian::Little, Version::V1_2, |_| {}).unwrap();
            assert_eq!(
                Decoder::new(&raw, Endian::Little).get_u32().unwrap(),
                FOLLOW_LOCAL_ONLY,
                "{op}"
            );
        }
    }

    /// Every user exception `query` can raise, with the repository id a
    /// foreign client narrows on and the member the IDL declares.
    ///
    /// The nested ids are the ones worth pinning: `IllegalPreference` and
    /// `IllegalPolicyName` are declared *inside* `Lookup`, so their ids carry
    /// the interface as a scope, and a client whose stub expects
    /// `.../Lookup/IllegalPreference:1.0` gets `UNKNOWN` from an id that
    /// merely looks right.
    #[test]
    fn each_refusal_raises_the_exception_the_idl_declares_carrying_the_callers_own_string() {
        let t = trader("127.0.0.1", 1, 1);
        let e = Endian::Little;
        #[allow(clippy::type_complexity)]
        let cases: Vec<(&str, Box<dyn Fn(&mut Encoder)>, &str, &str)> = vec![
            (
                "an illegal service type name",
                Box::new(|x: &mut Encoder| {
                    write_query(x, "1illegal", "", "", &[], HOW_MANY_ALL, &[], 10)
                }),
                ILLEGAL_SERVICE_TYPE_ID,
                "1illegal",
            ),
            (
                "a service type nobody declared",
                Box::new(|x: &mut Encoder| {
                    write_query(x, "moe::Nope", "", "", &[], HOW_MANY_ALL, &[], 10)
                }),
                UNKNOWN_SERVICE_TYPE_ID,
                "moe::Nope",
            ),
            (
                "a constraint that does not parse",
                Box::new(|x: &mut Encoder| {
                    write_query(x, "moe::Expert", "cost <<", "", &[], HOW_MANY_ALL, &[], 10)
                }),
                ILLEGAL_CONSTRAINT_ID,
                "cost <<",
            ),
            (
                "a preference that does not parse",
                Box::new(|x: &mut Encoder| {
                    write_query(x, "moe::Expert", "", "SIDEWAYS", &[], HOW_MANY_ALL, &[], 10)
                }),
                ILLEGAL_PREFERENCE_ID,
                "SIDEWAYS",
            ),
            (
                "an import policy this trader does not implement",
                Box::new(|x: &mut Encoder| {
                    write_query(
                        x,
                        "moe::Expert",
                        "",
                        "",
                        &["exact_type_match"],
                        HOW_MANY_ALL,
                        &[],
                        10,
                    )
                }),
                ILLEGAL_POLICY_NAME_ID,
                "exact_type_match",
            ),
            (
                "a property an offer does not carry",
                Box::new(|x: &mut Encoder| {
                    write_query(x, "moe::Expert", "", "", &[], HOW_MANY_SOME, &["throughput"], 10)
                }),
                ILLEGAL_PROPERTY_NAME_ID,
                "throughput",
            ),
            (
                "the same property asked for twice",
                Box::new(|x: &mut Encoder| {
                    write_query(x, "moe::Expert", "", "", &[], HOW_MANY_SOME, &["cost", "cost"], 10)
                }),
                DUPLICATE_PROPERTY_NAME_ID,
                "cost",
            ),
        ];

        for (what, write, want_id, want_member) in cases {
            let (body, raw) = call(&t, "query", e, Version::V1_2, &write).unwrap();
            let (id, member) = user_exception(body, &raw, e);
            assert_eq!(id, want_id, "{what}");
            assert_eq!(member, want_member, "{what}: the caller's own string comes back");
        }
    }

    /// `ORDER BY` is this engine's extension and not TCL, and on this
    /// interface the ordering is `pref` — so a wire constraint carrying one is
    /// refused as an illegal *constraint* rather than blamed on a preference
    /// the caller never wrote.
    #[test]
    fn a_wire_constraint_carrying_order_by_is_refused_as_a_constraint() {
        let t = trader("127.0.0.1", 1, 1);
        let e = Endian::Little;
        let (body, raw) = call(&t, "query", e, Version::V1_2, |x| {
            write_query(
                x,
                "moe::Expert",
                "cost < 9 ORDER BY cost ASC",
                "",
                &[],
                HOW_MANY_ALL,
                &[],
                10,
            )
        })
        .unwrap();
        let (id, member) = user_exception(body, &raw, e);
        assert_eq!(id, ILLEGAL_CONSTRAINT_ID);
        assert_eq!(member, "cost < 9 ORDER BY cost ASC");
    }

    /// A property an offer does not carry is *absent* from its `PropertySeq`,
    /// not present-and-empty. The wire and the engine's `Option` say the same
    /// thing, which is what makes `EXIST specialization` mean anything.
    #[test]
    fn an_absent_property_is_missing_from_the_sequence_rather_than_empty() {
        let t = TradingServer::new("127.0.0.1", 1, KEY.to_vec());
        t.with_store(|s| {
            s.declare(ServiceType::declare("moe::Raw", "IDL:moe/Raw:1.0", vec![]).unwrap())
                .unwrap();
            let mut gapped = offer("g", 1.0);
            gapped.specialization = None;
            gapped.latency_p50 = None;
            s.register("moe::Raw", gapped).unwrap();
        });
        let e = Endian::Little;
        let (_, raw) = call(&t, "query", e, Version::V1_2, plain_query("moe::Raw", 10)).unwrap();
        let got = read_answer(&raw, e);
        let names = &got.offers[0].1;
        assert_eq!(names.len(), 8, "eight of ten: {names:?}");
        assert!(!names.contains(&"specialization".to_owned()), "{names:?}");
        assert!(!names.contains(&"latency_p50".to_owned()), "{names:?}");
        assert!(names.contains(&"residency".to_owned()), "{names:?}");
    }

    /// An offer's `reference` is nil unless a deployment bound one — the
    /// module header's second named limitation, measured in both directions.
    #[test]
    fn an_offers_reference_is_nil_until_a_deployment_binds_one() {
        let t = trader("127.0.0.1", 1, 2);
        let e = Endian::Little;
        let (_, raw) = call(&t, "query", e, Version::V1_2, plain_query("moe::Expert", 10)).unwrap();
        let got = read_answer(&raw, e);
        assert!(got.offers.iter().all(|(r, _)| r.is_nil()), "nothing was bound yet");

        t.set_reference(
            "e0000",
            Ior {
                type_id: "IDL:moe/Expert:1.0".to_owned(),
                profiles: vec![IiopProfile {
                    version: Version::V1_2,
                    host: "10.0.0.1".to_owned(),
                    port: 9999,
                    object_key: b"e0000".to_vec(),
                    components: vec![],
                }],
            },
        );
        let (_, raw) = call(&t, "query", e, Version::V1_2, plain_query("moe::Expert", 10)).unwrap();
        let got = read_answer(&raw, e);
        assert_eq!(got.offers[0].0.primary().unwrap().port, 9999, "the bound one comes back");
        assert!(got.offers[1].0.is_nil(), "the unbound one is still nil");
    }

    /// `_is_a` must answer for `Lookup` **and its three bases**, because a
    /// foreign client narrows to whichever one its stub was generated from.
    #[test]
    fn is_a_answers_for_lookup_and_every_interface_it_inherits() {
        let t = trader("127.0.0.1", 1, 0);
        let e = Endian::Little;
        for id in [
            LOOKUP_ID,
            TRADER_COMPONENTS_ID,
            SUPPORT_ATTRIBUTES_ID,
            IMPORT_ATTRIBUTES_ID,
            "IDL:omg.org/CORBA/Object:1.0",
        ] {
            let (_, raw) = call(&t, "_is_a", e, Version::V1_2, |x| x.put_str(id)).unwrap();
            assert!(Decoder::new(&raw, e).get_bool().unwrap(), "{id}");
        }
        for id in ["IDL:omg.org/CosTrading/Register:1.0", "IDL:omg.org/CosNaming/NamingContext:1.0"]
        {
            let (_, raw) = call(&t, "_is_a", e, Version::V1_2, |x| x.put_str(id)).unwrap();
            assert!(!Decoder::new(&raw, e).get_bool().unwrap(), "{id}");
        }
    }

    /// A `readonly attribute` has no setter, so `_set_` on any of the twenty
    /// is a name the contract does not declare — `BAD_OPERATION`, not
    /// `NO_IMPLEMENT`, which here is reserved for *declared and deliberately
    /// not served*.
    #[test]
    fn a_setter_on_a_readonly_attribute_is_bad_operation_not_no_implement() {
        let t = trader("127.0.0.1", 1, 0);
        for op in ["_set_max_return_card", "_set_lookup_if", "describe_type", "export"] {
            let err = call(&t, op, Endian::Little, Version::V1_2, |x| x.put_u32(0)).unwrap_err();
            assert_eq!(err.id, crate::server::BAD_OPERATION, "{op}");
        }
    }

    /// Every repository id this servant puts on the wire, written out again.
    ///
    /// **Deliberately a second hand-typed copy, and the negative control is
    /// why.** Un-nesting `ILLEGAL_PREFERENCE_ID` to
    /// `IDL:omg.org/CosTrading/IllegalPreference:1.0` — the exact mistake the
    /// constant's own doc warns about — left every other test in this module
    /// **green**, because they all assert against the constant they are
    /// testing. `spikes/trading_client.py` went red immediately
    /// (`UNKNOWN(UNKNOWN_UserException)`), which is the right answer but needs
    /// omniORB installed and a fixture running.
    ///
    /// So this test is the oracle-free half: the authority for these strings
    /// is the published OMG IDL, not us, and two independently typed copies of
    /// a string we do not own is exactly the right shape — a change to one is
    /// caught by the other. A test computing the expected value from the
    /// constant would be a tautology, which is what it was.
    #[test]
    fn every_repository_id_is_the_one_the_omg_idl_declares() {
        // Interfaces.
        assert_eq!(LOOKUP_ID, "IDL:omg.org/CosTrading/Lookup:1.0");
        assert_eq!(TRADER_COMPONENTS_ID, "IDL:omg.org/CosTrading/TraderComponents:1.0");
        assert_eq!(SUPPORT_ATTRIBUTES_ID, "IDL:omg.org/CosTrading/SupportAttributes:1.0");
        assert_eq!(IMPORT_ATTRIBUTES_ID, "IDL:omg.org/CosTrading/ImportAttributes:1.0");
        // Exceptions declared at module scope.
        assert_eq!(ILLEGAL_SERVICE_TYPE_ID, "IDL:omg.org/CosTrading/IllegalServiceType:1.0");
        assert_eq!(UNKNOWN_SERVICE_TYPE_ID, "IDL:omg.org/CosTrading/UnknownServiceType:1.0");
        assert_eq!(ILLEGAL_CONSTRAINT_ID, "IDL:omg.org/CosTrading/IllegalConstraint:1.0");
        assert_eq!(ILLEGAL_PROPERTY_NAME_ID, "IDL:omg.org/CosTrading/IllegalPropertyName:1.0");
        assert_eq!(DUPLICATE_PROPERTY_NAME_ID, "IDL:omg.org/CosTrading/DuplicatePropertyName:1.0");
        // Exceptions declared *inside* `Lookup`, whose ids carry the
        // interface as a scope. These two are the ones the control killed.
        assert_eq!(ILLEGAL_PREFERENCE_ID, "IDL:omg.org/CosTrading/Lookup/IllegalPreference:1.0");
        assert_eq!(ILLEGAL_POLICY_NAME_ID, "IDL:omg.org/CosTrading/Lookup/IllegalPolicyName:1.0");
    }

    /// A `SpecifiedProps` discriminator outside `HowManyProps` decodes to
    /// nothing — the union has no default arm, so there is nothing to guess.
    #[test]
    fn a_union_discriminator_outside_the_enum_is_a_marshal_error() {
        let t = trader("127.0.0.1", 1, 0);
        let err = call(&t, "query", Endian::Little, Version::V1_2, |x| {
            x.put_str("moe::Expert");
            x.put_str("");
            x.put_str("");
            x.put_u32(0);
            x.put_u32(77); // not none, some or all
            x.put_u32(10);
        })
        .unwrap_err();
        assert_eq!(err.id, crate::server::MARSHAL);
    }

    /// The property this servant must not spend: it opens a lock section and
    /// never calls out from inside one.
    ///
    /// Dispatched **directly**, not over a socket, because the tripwire counts
    /// per thread — a violation on a connection thread is invisible to the
    /// thread asserting. The completion flag is here because
    /// `complaints_about` absorbs panics, so an empty list has to be told
    /// apart from a closure that stopped early.
    #[test]
    fn no_operation_of_this_servant_calls_out_from_inside_the_offer_lock() {
        let t = trader("127.0.0.1", 1, 2);
        let mut finished = false;
        let complaints = crate::guarded::complaints_about(|| {
            for (op, write) in operations() {
                let wire =
                    crate::encode_request(Version::V1_2, Endian::Little, 1, KEY, op, true, |e| {
                        write(e)
                    })
                    .unwrap();
                let msg = crate::read_message(&mut &wire[..], DEFAULT_MAX_MESSAGE_SIZE).unwrap();
                let req = decode_request(msg).unwrap();
                let mut out = Encoder::new(Endian::Little);
                assert!(SharedDispatch::knows(&t, &req.object_key), "{op}");
                let body = SharedDispatch::dispatch_body(&t, &req, &mut out);
                // Every row must reach its *body*: a sweep that stops at an
                // argument check measures the argument checks and calls that
                // coverage.
                assert!(
                    matches!(body, Ok(DispatchBody::Return)),
                    "{op} did not reach its body: {body:?}"
                );
            }
            finished = true;
        });
        assert_eq!(
            complaints.first(),
            None,
            "the trading servant violated the lock discipline: {complaints:?}"
        );
        assert!(finished, "the sweep did not run to the end, so it measured nothing");
    }

    /// Every operation and attribute this servant serves, each in a form that
    /// *succeeds*.
    #[allow(clippy::type_complexity)]
    fn operations() -> Vec<(&'static str, Box<dyn Fn(&mut Encoder)>)> {
        let mut rows: Vec<(&'static str, Box<dyn Fn(&mut Encoder)>)> = vec![
            ("_is_a", Box::new(|e: &mut Encoder| e.put_str(LOOKUP_ID))),
            ("_non_existent", Box::new(|_: &mut Encoder| {})),
            (
                "query",
                Box::new(|e: &mut Encoder| {
                    write_query(e, "moe::Expert", "", "", &[], HOW_MANY_ALL, &[], 10)
                }),
            ),
        ];
        for attr in ATTRIBUTES {
            rows.push((attr, Box::new(|_: &mut Encoder| {})));
        }
        rows
    }

    /// The twenty attributes `Lookup` inherits, as `_get_` operation names.
    /// Named here so the sweep and the coverage story cannot disagree about
    /// how many there are.
    const ATTRIBUTES: [&str; 20] = [
        "_get_lookup_if",
        "_get_register_if",
        "_get_link_if",
        "_get_proxy_if",
        "_get_admin_if",
        "_get_supports_modifiable_properties",
        "_get_supports_dynamic_properties",
        "_get_supports_proxy_offers",
        "_get_type_repos",
        "_get_def_search_card",
        "_get_max_search_card",
        "_get_def_match_card",
        "_get_max_match_card",
        "_get_def_return_card",
        "_get_max_return_card",
        "_get_max_list",
        "_get_def_hop_count",
        "_get_max_hop_count",
        "_get_def_follow_policy",
        "_get_max_follow_policy",
    ];

    /// Over a real socket, so the servant is measured through the same path a
    /// foreign client takes and not only through a direct dispatch.
    #[test]
    fn the_servant_answers_over_a_socket_and_refuses_over_one_too() {
        let server = Server::bind("127.0.0.1:0", KEY.to_vec()).unwrap();
        let port = server.local_addr().unwrap().port();
        let t = Arc::new(trader("127.0.0.1", port, 3));
        let ior = t.lookup_ior();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let servant = t.clone();
        let thread = std::thread::spawn(move || {
            server.serve_shared(&*servant, move || flag.load(Ordering::SeqCst)).unwrap();
        });

        let mut conn = crate::Connection::connect(&ior, T).unwrap();

        let reply = conn
            .invoke("query", |e| {
                write_query(e, "moe::Expert", "cost < 2", "MIN cost", &[], HOW_MANY_ALL, &[], 10)
            })
            .unwrap();
        let got = read_answer_from(&mut reply.body().unwrap());
        assert_eq!(got.offers.len(), 2, "cost < 2 matched e0000 and e0001");
        assert!(got.iterator.is_nil());

        let err = conn
            .invoke("query", |e| write_query(e, "moe::Expert", "", "", &[], HOW_MANY_ALL, &[], 1))
            .unwrap_err();
        match err {
            Error::SystemException { id, .. } => assert_eq!(id, crate::server::NO_IMPLEMENT),
            other => panic!("expected NO_IMPLEMENT over the socket, got {other:?}"),
        }

        stop.store(true, Ordering::SeqCst);
        drop(conn);
        thread.join().unwrap();
    }
}
