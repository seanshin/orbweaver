//! The seven POA policies of CORBA 3.4 §15.3.8, and the value this POA
//! behaves as for each.
//!
//! # Why this module exists
//!
//! Every wire-level decision in this workspace carries its section — `§7.11.3`
//! in the lexer, `§9.4.9` in the fragment code — and until this module landed
//! `orbweaver-object` cited *CORBA — Part 1: Interfaces, v3.4* **zero times**,
//! while the POA is the half of CORBA a server author actually meets. A POA
//! has seven policies whether or not anybody names them; not naming them does
//! not make the choice absent, it makes it a fact with no home. So this module
//! writes the seven down, cited, together with **which of them we chose and
//! which we merely fell into** ([`Stance`]).
//!
//! *일곱 정책은 이름을 붙이든 안 붙이든 값을 가진다. 이름을 붙이지 않는다고
//! 선택이 사라지는 것이 아니라, 그 사실이 살 집이 없어질 뿐이다.*
//!
//! # What this module is not
//!
//! It changes no behaviour and configures nothing. [`crate::Poa::policies`]
//! **computes** its answer from the POA's existing fields; a `Policies` value
//! is a report, never a setting. Making [`IdAssignmentPolicy`] real — so that
//! `activate` and `activate_new` cannot both be legal on one adapter — is
//! D020 Stage B.
//!
//! # The one divergence
//!
//! [`IdAssignmentPolicy::Either`] is **ours, not the specification's**. See its
//! documentation: §15.3.8.4 makes id assignment a per-POA choice, and this POA
//! answers to both models at once.

/// How we came to behave as a given policy value — and the honest word matters.
///
/// The point of this module is the distinction between the first two variants.
/// A value we picked and enforce is a decision; a value we happen to exhibit
/// and never wrote down is a decision too, just one nobody made on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stance {
    /// We picked this value, it is enforced, and a caller can select the other
    /// one. [`LifespanPolicy`] alone.
    Chosen,
    /// We behave as this value and had never said so until this module. The
    /// alternative values are not available; nothing rejects them, because
    /// nothing can express them.
    Implicit,
    /// The policy has no shape in this design, so neither value is true or
    /// false here. Recording the reason is the whole obligation — implementing
    /// it would be inventing a distinction to satisfy a table.
    NotApplicableByDesign,
    /// We behave as something the specification does not offer. `Implicit`'s
    /// worse cousin: an unnamed choice is a gap, an unnamed *divergence* is a
    /// gap that will surprise a peer. [`IdAssignmentPolicy::Either`] alone.
    Divergence,
}

/// One of the seven policies named by CORBA 3.4 §15.3.8.
///
/// Implemented by each policy enum so the table — name, section, stance, the
/// specification's default — is available to a reader and to a test, rather
/// than living only in prose that nothing compiles.
pub trait Policy: Copy + Sized + 'static {
    /// The specification's name for the policy, as a server author would
    /// search for it.
    const NAME: &'static str;
    /// The section of *CORBA — Part 1: Interfaces, v3.4* that defines it.
    const SECTION: &'static str;
    /// Where we stand on it.
    const STANCE: Stance;
    /// The value the specification says a POA gets when the policy is not
    /// supplied to `create_POA`. Every enum here also derives `Default` to the
    /// same value, and [`tests::every_default_is_the_specifications_default`]
    /// holds the two together.
    const SPEC_DEFAULT: Self;
}

// ─────────────────────────────────────────────────────────────────────────────
// 1 · Thread — §15.3.8.1
// ─────────────────────────────────────────────────────────────────────────────

/// **Thread policy** — CORBA 3.4 §15.3.8.1. Which threads carry a request into
/// implementation code.
///
/// **Stance: [implicit](Stance::Implicit).** No `Poa` field names a threading
/// model and no caller can choose one. The choice was made a layer away, in
/// `orbweaver_giop::Server`: `serve_shared` gives every accepted connection a
/// thread and lets them all enter the servant at once, which is
/// [`OrbCtrlModel`](Self::OrbCtrlModel) — the ORB assigning requests to
/// threads. (`Server::serve` is that same function behind a per-message mutex,
/// which is [`SingleThreadModel`](Self::SingleThreadModel)-shaped, but it is a
/// property of the *server*, not of the adapter.)
///
/// **Not observable from this crate**, and no test here claims otherwise:
/// `Poa` has no thread field, `dispatch_target` takes `&mut self`, and the
/// concurrency lives in another crate that this stage's footprint excludes.
/// An honest "not measured here" is the result; see the commit message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThreadPolicy {
    /// The ORB assigns requests to threads; concurrent requests may arrive on
    /// several at once. The specification's default, and ours.
    #[default]
    OrbCtrlModel,
    /// Requests for this POA are processed sequentially, so upcalls are safe
    /// for multi-thread-unaware implementation code.
    SingleThreadModel,
    /// Requests for every main-thread POA are processed sequentially on one
    /// distinguished thread.
    MainThreadModel,
}

impl Policy for ThreadPolicy {
    const NAME: &'static str = "ThreadPolicy";
    const SECTION: &'static str = "CORBA 3.4 §15.3.8.1";
    const STANCE: Stance = Stance::Implicit;
    const SPEC_DEFAULT: Self = Self::OrbCtrlModel;
}

// ─────────────────────────────────────────────────────────────────────────────
// 2 · Lifespan — §15.3.8.2
// ─────────────────────────────────────────────────────────────────────────────

/// **Lifespan policy** — CORBA 3.4 §15.3.8.2. Whether the objects this POA
/// implements may outlive the process that created them.
///
/// **Stance: [chosen](Stance::Chosen).** The one policy of the seven that was
/// named before this module: [`crate::Lifespan`] is a real field, a caller
/// selects it with [`crate::Poa::with_lifespan`], and — by coincidence rather
/// than by reading §15.3.8.2 — we default to `TRANSIENT` exactly as the
/// specification does.
///
/// **Observable**, which is why this claim is behaviourally tested: a
/// transient object key carries the POA's incarnation, so a key minted by one
/// run is refused by the next; a persistent key does not, so it is
/// reproducible across runs. See `Poa::object_key` and `Poa::parse_key`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LifespanPolicy {
    /// The objects cannot outlive the POA instance that first created them.
    /// The specification's default, and ours.
    #[default]
    Transient,
    /// The objects can outlive the process that first created them, so their
    /// keys must be reproducible.
    Persistent,
}

impl Policy for LifespanPolicy {
    const NAME: &'static str = "LifespanPolicy";
    const SECTION: &'static str = "CORBA 3.4 §15.3.8.2";
    const STANCE: Stance = Stance::Chosen;
    const SPEC_DEFAULT: Self = Self::Transient;
}

// ─────────────────────────────────────────────────────────────────────────────
// 3 · Object Id Uniqueness — §15.3.8.3
// ─────────────────────────────────────────────────────────────────────────────

/// **Object Id Uniqueness policy** — CORBA 3.4 §15.3.8.3. Whether one servant
/// may answer to more than one object id.
///
/// **Stance: [not applicable by design](Stance::NotApplicableByDesign)**, and
/// [`Policies::id_uniqueness`] reports `None` rather than a value.
///
/// The specification's Active Object Map maps `ObjectId → Servant`. Ours is
/// `HashMap<ObjectId, ()>`: it holds **ids, not servants**. The servant is the
/// `Dispatch` implementation the server calls, and a skeleton serving more
/// than one object takes its identity as an explicit `Target` argument. So
/// "one servant, many ids" has no shape here to be either true or false in —
/// there is no servant in the map for the policy to be about. Implementing
/// `UNIQUE_ID` would mean inventing the association first, purely so that a
/// row in a table could be filled in.
///
/// **Not observable**, necessarily: a policy with nothing to constrain cannot
/// be refuted by behaviour. No test here claims to cover it, and the report
/// answering `None` is the claim.
///
/// *명세의 Active Object Map은 서번트를 담고 우리 것은 id를 담는다. "서번트 하나,
/// id 여럿"은 여기서 참도 거짓도 될 모양이 없다.*
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IdUniquenessPolicy {
    /// A servant activated with this POA supports exactly one object id. The
    /// specification's default.
    #[default]
    UniqueId,
    /// A servant activated with this POA may support one or more object ids.
    MultipleId,
}

impl Policy for IdUniquenessPolicy {
    const NAME: &'static str = "IdUniquenessPolicy";
    const SECTION: &'static str = "CORBA 3.4 §15.3.8.3";
    const STANCE: Stance = Stance::NotApplicableByDesign;
    const SPEC_DEFAULT: Self = Self::UniqueId;
}

// ─────────────────────────────────────────────────────────────────────────────
// 4 · Id Assignment — §15.3.8.4 — the divergence
// ─────────────────────────────────────────────────────────────────────────────

/// **Id Assignment policy** — CORBA 3.4 §15.3.8.4. Whether object ids in this
/// POA are chosen by the application or by the POA.
///
/// **Stance: [divergence](Stance::Divergence).** §15.3.8.4 makes this a
/// **per-POA** choice: a POA assigns ids "only by the application" or "only by
/// the POA". Ours does both, on the same adapter —
/// [`crate::Poa::activate`] takes an id the caller chose (`USER_ID`) and
/// [`crate::Poa::activate_new`] mints one (`SYSTEM_ID`) — and nothing stops a
/// caller mixing them. That is not a naming gap; it is a POA the specification
/// does not describe, which is why it gets [`Either`](Self::Either) rather
/// than a footnote.
///
/// This value was found by *writing the table*, not by reading the code.
///
/// **Observable**, and tested: both methods succeed on one POA, and
/// [`Policies::id_assignment`] answers `Either` while `IdAssignmentPolicy`'s
/// own `Default` is the specification's `SystemId` — the two disagreeing is
/// the divergence, compiled.
///
/// **Stage A only records it.** D020 Stage B makes `SystemId` and `UserId`
/// real, refusing the crossing with `WrongPolicy`, and keeps `Either` as the
/// backward-compatible default so that four servants and twelve spike
/// binaries keep compiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IdAssignmentPolicy {
    /// Ids are assigned only by the POA. The specification's default — and
    /// **not** what [`crate::Poa`] behaves as; see [`Either`](Self::Either).
    #[default]
    SystemId,
    /// Ids are assigned only by the application.
    UserId,
    /// **Ours, not the specification's.** Both assignment models are legal on
    /// one adapter: `activate(id)` is `USER_ID` behaviour, `activate_new()` is
    /// `SYSTEM_ID` behaviour, and today every `Poa` accepts both.
    ///
    /// It exists for backward compatibility and nothing else. `naming_server`,
    /// `event_server`, `expert_service`, `tenant_service` and twelve spike
    /// binaries are built on a surface that offers both, so a stage that
    /// simply picked one would stop them compiling. A divergence that is named
    /// and defaulted-to is a different thing from one nobody noticed — but it
    /// is still a divergence, and a new POA should not want it.
    Either,
}

impl Policy for IdAssignmentPolicy {
    const NAME: &'static str = "IdAssignmentPolicy";
    const SECTION: &'static str = "CORBA 3.4 §15.3.8.4";
    const STANCE: Stance = Stance::Divergence;
    const SPEC_DEFAULT: Self = Self::SystemId;
}

// ─────────────────────────────────────────────────────────────────────────────
// 5 · Servant Retention — §15.3.8.5
// ─────────────────────────────────────────────────────────────────────────────

/// **Servant Retention policy** — CORBA 3.4 §15.3.8.5. Whether what the POA
/// resolves survives the request that resolved it.
///
/// **Stance: [implicit](Stance::Implicit).** Nothing names it and nothing can
/// select the other value; we behave as [`Retain`](Self::Retain).
///
/// **This is a correction to what the design was assumed to do.** The hook is
/// spelled [`crate::ServantLocator`], and a `ServantLocator` is the
/// specification's `NON_RETAIN` half of servant management — one servant per
/// call, `preinvoke`/`postinvoke`. Measured, our `dispatch_target` does the
/// opposite: on [`crate::Located::Here`] it calls `activate` and the id stays
/// in the map, so the *next* request for that id is served without the locator
/// being consulted at all. That is `RETAIN` with a `ServantActivator`
/// (§15.3.8.6, "RETAIN and USE_SERVANT_MANAGER"), under a name borrowed from
/// the other half.
///
/// The map retaining **ids rather than servants** does not change the answer:
/// what matters for this policy is whether the resolution survives the
/// request, and ours does.
///
/// **Observable**, and tested: locate once with a locator, then dispatch the
/// same key with `None` and it is still `Active`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ServantRetentionPolicy {
    /// The POA retains what it activated in its Active Object Map. The
    /// specification's default, and ours.
    #[default]
    Retain,
    /// Nothing is retained; every request resolves afresh.
    NonRetain,
}

impl Policy for ServantRetentionPolicy {
    const NAME: &'static str = "ServantRetentionPolicy";
    const SECTION: &'static str = "CORBA 3.4 §15.3.8.5";
    const STANCE: Stance = Stance::Implicit;
    const SPEC_DEFAULT: Self = Self::Retain;
}

// ─────────────────────────────────────────────────────────────────────────────
// 6 · Request Processing — §15.3.8.6
// ─────────────────────────────────────────────────────────────────────────────

/// **Request Processing policy** — CORBA 3.4 §15.3.8.6. What happens when a
/// request names an id the Active Object Map does not hold.
///
/// **Stance: [implicit](Stance::Implicit), and partly so.** This is the one
/// policy of the seven that an existing type already ranges over without
/// saying it does: [`crate::UnknownIdPolicy::Reject`] *is*
/// [`UseActiveObjectMapOnly`](Self::UseActiveObjectMapOnly) and
/// [`crate::UnknownIdPolicy::AskLocator`] *is*
/// [`UseServantManager`](Self::UseServantManager). What makes it implicit
/// rather than chosen is that the correspondence was never written down, and
/// the third value has no analogue here:
/// [`UseDefaultServant`](Self::UseDefaultServant) needs a servant to default
/// to, and this POA's map holds ids (see [`IdUniquenessPolicy`]).
///
/// **A known divergence inside the mapping, recorded and not fixed here.**
/// §15.3.8.6 says `USE_SERVANT_MANAGER` with no servant manager registered
/// raises `OBJ_ADAPTER` with standard minor code 4. `AskLocator` with no
/// locator passed answers [`crate::Target::Unknown`], which the server turns
/// into `OBJECT_NOT_EXIST`. Changing that is a behaviour change and Stage A
/// makes none.
///
/// **Observable**, and tested on both values: under `Reject` a locator that
/// would have said `Here` is never asked; under `AskLocator` it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RequestProcessingPolicy {
    /// An id absent from the Active Object Map is `OBJECT_NOT_EXIST`; nothing
    /// else is consulted. The specification's default, and ours.
    #[default]
    UseActiveObjectMapOnly,
    /// An id absent from the map is dispatched to a registered default
    /// servant. **No analogue here** — nothing registers a servant with a POA
    /// that holds only ids.
    UseDefaultServant,
    /// An id absent from the map is given to a registered servant manager,
    /// which may locate one or forward the caller.
    UseServantManager,
}

impl Policy for RequestProcessingPolicy {
    const NAME: &'static str = "RequestProcessingPolicy";
    const SECTION: &'static str = "CORBA 3.4 §15.3.8.6";
    const STANCE: Stance = Stance::Implicit;
    const SPEC_DEFAULT: Self = Self::UseActiveObjectMapOnly;
}

// ─────────────────────────────────────────────────────────────────────────────
// 7 · Implicit Activation — §15.3.8.7
// ─────────────────────────────────────────────────────────────────────────────

/// **Implicit Activation policy** — CORBA 3.4 §15.3.8.7. Whether a servant can
/// become active as a side effect of something else.
///
/// **Stance: [implicit](Stance::Implicit)** — and behaving as the
/// specification's own default, which is the least costly kind of unstated
/// choice but an unstated choice all the same. Nothing here activates an
/// object except an explicit [`crate::Poa::activate`],
/// [`crate::Poa::activate_new`], or a locator answering
/// [`crate::Located::Here`].
///
/// **Observable**, and tested: [`crate::Poa::reference`] mints a reference for
/// an id that was never activated — the operation a POA with
/// `IMPLICIT_ACTIVATION` would activate on — and afterwards the id is still
/// inactive and a request for it is still `Unknown`.
///
/// §15.3.8.7, verbatim: `IMPLICIT_ACTIVATION` **also requires the `SYSTEM_ID`
/// and `RETAIN` policies**. That is a constraint between policies, which is a
/// thing a `Policies` type can carry and a hand-written adapter cannot — see
/// [`Policies::spec_violations`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImplicitActivationPolicy {
    /// The POA supports implicit activation of servants. Requires `SYSTEM_ID`
    /// and `RETAIN`.
    ImplicitActivation,
    /// The POA does not. The specification's default, and ours.
    #[default]
    NoImplicitActivation,
}

impl Policy for ImplicitActivationPolicy {
    const NAME: &'static str = "ImplicitActivationPolicy";
    const SECTION: &'static str = "CORBA 3.4 §15.3.8.7";
    const STANCE: Stance = Stance::Implicit;
    const SPEC_DEFAULT: Self = Self::NoImplicitActivation;
}

// ─────────────────────────────────────────────────────────────────────────────
// The seven together
// ─────────────────────────────────────────────────────────────────────────────

/// What a POA behaves as, for each of the seven policies of §15.3.8.
///
/// **A report, not a setting.** [`crate::Poa::policies`] computes every field
/// from the POA's existing state; nothing here is stored and nothing here is
/// configurable in D020 Stage A. Four of the seven are the same for every
/// `Poa` that exists today, and saying so out loud is the point — a constant
/// is what an unstated choice looks like once it is stated.
///
/// Marked `#[non_exhaustive]`: Stage B and Stage C add fields, and a report
/// type gaining a field should not be a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Policies {
    /// §15.3.8.1. Always [`ThreadPolicy::OrbCtrlModel`]; not observable from
    /// this crate.
    pub thread: ThreadPolicy,
    /// §15.3.8.2. From the POA's [`crate::Lifespan`] — the one policy a caller
    /// selects, and the only field here that varies with a choice someone made
    /// deliberately.
    pub lifespan: LifespanPolicy,
    /// §15.3.8.3. Always `None`: **not applicable by design**, because this
    /// POA's Active Object Map holds ids and not servants, so neither
    /// `UNIQUE_ID` nor `MULTIPLE_ID` is true or false here. See
    /// [`IdUniquenessPolicy`] for the reason at length.
    pub id_uniqueness: Option<IdUniquenessPolicy>,
    /// §15.3.8.4. Always [`IdAssignmentPolicy::Either`], which is ours and not
    /// the specification's.
    pub id_assignment: IdAssignmentPolicy,
    /// §15.3.8.5. Always [`ServantRetentionPolicy::Retain`].
    pub servant_retention: ServantRetentionPolicy,
    /// §15.3.8.6. From the POA's [`crate::UnknownIdPolicy`].
    pub request_processing: RequestProcessingPolicy,
    /// §15.3.8.7. Always [`ImplicitActivationPolicy::NoImplicitActivation`].
    pub implicit_activation: ImplicitActivationPolicy,
}

impl Policies {
    /// The combinations §15.3.8 forbids, as sentences, for this set of values.
    ///
    /// The chapter states three constraints between policies in the sections
    /// read for D020, and a `Policies` value is where they can be checked at
    /// all — a POA assembled field by field has nowhere to put them. Empty for
    /// every `Poa` this crate builds today; it exists so that Stage B and
    /// Stage C, which make three of these fields selectable, cannot introduce
    /// a combination the specification rules out without something going red.
    ///
    /// This **reports**; it refuses nothing. Stage A changes no behaviour.
    pub fn spec_violations(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.implicit_activation == ImplicitActivationPolicy::ImplicitActivation {
            if self.id_assignment != IdAssignmentPolicy::SystemId {
                out.push("§15.3.8.7: IMPLICIT_ACTIVATION also requires the SYSTEM_ID policy");
            }
            if self.servant_retention != ServantRetentionPolicy::Retain {
                out.push("§15.3.8.7: IMPLICIT_ACTIVATION also requires the RETAIN policy");
            }
        }
        if self.request_processing == RequestProcessingPolicy::UseActiveObjectMapOnly
            && self.servant_retention != ServantRetentionPolicy::Retain
        {
            out.push("§15.3.8.6: USE_ACTIVE_OBJECT_MAP_ONLY also requires the RETAIN policy");
        }
        if self.servant_retention == ServantRetentionPolicy::NonRetain
            && self.request_processing == RequestProcessingPolicy::UseActiveObjectMapOnly
        {
            out.push(
                "§15.3.8.5: NON_RETAIN requires either USE_DEFAULT_SERVANT or \
                 USE_SERVANT_MANAGER",
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every enum's `Default` is the value §15.3.8 says an unsupplied policy
    /// takes. Two ways of writing the same fact — `#[default]` and
    /// `SPEC_DEFAULT` — held together so neither can drift alone.
    #[test]
    fn every_default_is_the_specifications_default() {
        assert_eq!(ThreadPolicy::default(), ThreadPolicy::SPEC_DEFAULT);
        assert_eq!(LifespanPolicy::default(), LifespanPolicy::SPEC_DEFAULT);
        assert_eq!(IdUniquenessPolicy::default(), IdUniquenessPolicy::SPEC_DEFAULT);
        assert_eq!(IdAssignmentPolicy::default(), IdAssignmentPolicy::SPEC_DEFAULT);
        assert_eq!(ServantRetentionPolicy::default(), ServantRetentionPolicy::SPEC_DEFAULT);
        assert_eq!(RequestProcessingPolicy::default(), RequestProcessingPolicy::SPEC_DEFAULT);
        assert_eq!(ImplicitActivationPolicy::default(), ImplicitActivationPolicy::SPEC_DEFAULT);
    }

    /// Exactly one policy is chosen, one is a divergence, one does not apply,
    /// and four are implicit. If that census changes, a stage moved a policy
    /// between states and this is where it says so.
    #[test]
    fn the_census_of_stances_is_one_chosen_one_divergent_one_inapplicable_four_implicit() {
        let stances = [
            ThreadPolicy::STANCE,
            LifespanPolicy::STANCE,
            IdUniquenessPolicy::STANCE,
            IdAssignmentPolicy::STANCE,
            ServantRetentionPolicy::STANCE,
            RequestProcessingPolicy::STANCE,
            ImplicitActivationPolicy::STANCE,
        ];
        let count = |s: Stance| stances.iter().filter(|x| **x == s).count();
        assert_eq!(count(Stance::Chosen), 1, "only Lifespan was ever named");
        assert_eq!(count(Stance::Divergence), 1, "only Id Assignment");
        assert_eq!(count(Stance::NotApplicableByDesign), 1, "only Id Uniqueness");
        assert_eq!(count(Stance::Implicit), 4);
        assert_eq!(stances.len(), 7, "§15.3.8 names seven");
    }

    /// Every policy carries its section, and it is Chapter 15 — the POA sat in
    /// Chapter 11 in CORBA 2.x, and D020's own first draft cited 11.
    #[test]
    fn every_policy_cites_a_section_of_chapter_fifteen() {
        for s in [
            ThreadPolicy::SECTION,
            LifespanPolicy::SECTION,
            IdUniquenessPolicy::SECTION,
            IdAssignmentPolicy::SECTION,
            ServantRetentionPolicy::SECTION,
            RequestProcessingPolicy::SECTION,
            ImplicitActivationPolicy::SECTION,
        ] {
            assert!(s.starts_with("CORBA 3.4 §15.3.8."), "{s}");
        }
    }

    fn ours() -> Policies {
        Policies {
            thread: ThreadPolicy::OrbCtrlModel,
            lifespan: LifespanPolicy::Transient,
            id_uniqueness: None,
            id_assignment: IdAssignmentPolicy::Either,
            servant_retention: ServantRetentionPolicy::Retain,
            request_processing: RequestProcessingPolicy::UseActiveObjectMapOnly,
            implicit_activation: ImplicitActivationPolicy::NoImplicitActivation,
        }
    }

    #[test]
    fn the_constraints_of_15_3_8_are_reported_when_broken() {
        // IMPLICIT_ACTIVATION over Either and NonRetain breaks §15.3.8.7
        // twice: it requires SYSTEM_ID, and it requires RETAIN.
        let bad = Policies {
            implicit_activation: ImplicitActivationPolicy::ImplicitActivation,
            servant_retention: ServantRetentionPolicy::NonRetain,
            ..ours()
        };
        let v = bad.spec_violations();
        assert!(v.iter().any(|s| s.contains("SYSTEM_ID")), "{v:?}");
        assert!(v.iter().any(|s| s.contains("RETAIN policy")), "{v:?}");
        // NON_RETAIN with USE_ACTIVE_OBJECT_MAP_ONLY breaks §15.3.8.5/.6.
        assert!(v.iter().any(|s| s.contains("USE_ACTIVE_OBJECT_MAP_ONLY")), "{v:?}");
        assert!(v.iter().any(|s| s.contains("NON_RETAIN")), "{v:?}");
    }

    #[test]
    fn what_we_behave_as_breaks_none_of_them() {
        assert!(ours().spec_violations().is_empty());
    }
}
