//! The tenant-facing half of the MoE control plane: `ModelFactory`,
//! `ComposedModel`, `PolicyDomain` and `EnterpriseExpert` from
//! `corpus/golden/23-moe-enterprise.idl`, served on our own
//! [`Server`](orbweaver_giop::server::Server).
//!
//! PLAN-SERVICES §5 folds CosLifeCycle and CosProperty into one batch: the
//! factory is the GenericFactory *shape* with the standard's genericity
//! dropped (a typed `create(in Manifest)` rather than a stringly-typed
//! `create(key, criteria)`), and CosProperty is the [`Manifest`] struct rather
//! than a second service nobody has asked for. The contract is
//! corpus/golden/23 and nothing else: every operation it declares is served,
//! including the two `EnterpriseExpert` inherits from `::moe::Expert`, and
//! nothing it does not declare exists on the wire.
//!
//! # Tenancy is the substance here
//!
//! A [`Manifest`] carries `tenant_id` and `residency_region`, so this batch is
//! not an operation list — it is an isolation claim, and an isolation claim is
//! worth exactly as much as the layer that enforces it. What follows is the
//! division of labour, stated so a reader never has to infer it.
//!
//! ## The object key is the call's tenant context
//!
//! There is no per-call tenant credential on this contract. `moe::CallContext`
//! carries `request_id`, `trace_id` and `step` and no tenant; `Request`
//! exposes no service context to a servant; and the IDL declares no `in string
//! tenant` anywhere. So the tenant a call belongs to is **the tenant named by
//! the object key it is addressed to**, and every key this service mints
//! except one names exactly one tenant:
//!
//! ```text
//! <base>/t/<tenant>/factory              ModelFactory     — one per tenant
//! <base>/t/<tenant>/model/<serial>       ComposedModel
//! <base>/t/<tenant>/expert/<capability>  EnterpriseExpert
//! <base>/t/<tenant>/policy/<domain>      PolicyDomain
//! <base>/shared/base/<base_model>        ::moe::Expert    — the shared base
//! ```
//!
//! One factory *object* per tenant is not an invention: the IDL declares an
//! interface, not an instance count, and CosLifeCycle has always had many
//! factories found by a finder. It is what makes `create` and `retire`
//! checkable at all — a single shared factory would have no tenant context,
//! and `retire(in ComposedModel m)` would then be a call this layer could not
//! tell from a legitimate one.
//!
//! ## What is true here, and enforced by test
//!
//! 1. **No reference argument crosses a tenant.** `bind_expert`, `set_policy`,
//!    `clone_model`, `retire` and `deploy` all take a reference; each is
//!    refused with `NO_PERMISSION` when the argument's key names a tenant
//!    other than the target object's. The tenancy check runs **before** the
//!    existence check, so a refusal never discloses whether the other tenant's
//!    object exists.
//! 2. **`create` cannot mint into another tenant.** A manifest whose
//!    `tenant_id` differs from the factory's own tenant is `NO_PERMISSION`,
//!    not a silently relabelled object.
//! 3. **Keys cannot be forged out of manifest strings.** Every string that
//!    becomes part of a key — `tenant_id`, `base_model`, `version`,
//!    `policy_domain`, each `experts` element — is refused if it is empty or
//!    contains `/`. Without that, a tenant called `a/model/1` could name
//!    another tenant's key, and a forgeable credential is not one.
//! 4. **`retire` really destroys.** The key leaves the served set, so the
//!    `Server` answers `OBJECT_NOT_EXIST` to a later request *and*
//!    `UnknownObject` to a later `LocateRequest`; serials are never reused, so
//!    a stale reference can never land on a later model.
//! 5. **`check_residency` refuses.** A node in another region is `false`, and
//!    so is a node nobody declared — an undeclared node cannot be *shown* to
//!    be in region, and default-deny is the only safe direction.
//! 6. **Nothing enumerates.** No operation returns another tenant's reference
//!    and none lists anything; the contract declares no `list`, so there is
//!    nothing to refuse, only something not to add.
//!
//! ## `base()` — the crossing the manifest draws and this cannot remove
//!
//! `EnterpriseExpert::base()` hands back the *shared* base expert. The
//! manifest's whole shape is "shared base by reference, owned delta only", so
//! the crossing is the design, not a leak to be plugged; the annotation
//! (`ai_authz: moe.enterprise.base.read`) says who may ask, and the servant's
//! job is to make the crossing **visible and bounded** rather than to pretend
//! it does not happen. Three things do that:
//!
//! - **The type bounds it.** The IDL returns `::moe::Expert`, not
//!   `EnterpriseExpert`. The reference therefore has no `get_tenant_id`, no
//!   `adapter_delta` and no tenant in its key: what crosses is the shared
//!   base's identity and compute, and no tenant's adapter can be reached
//!   through it. `_is_a` on it answers `IDL:moe/Expert:1.0` and nothing more.
//! - **It is inert as an argument.** Its key names no tenant, so every
//!   tenancy-checked operation refuses it — `bind_expert(base())` is
//!   `BAD_PARAM`, not a quiet composition of a tenantless object into a
//!   tenant's model.
//! - **It is counted and audited.** Every call increments
//!   [`TenantService::base_crossings`] for the calling tenant and appends an
//!   audit entry, so "how often did this tenant leave its own boundary" is a
//!   number somebody can read rather than a property nobody measured.
//!
//! What this layer cannot do is *hide* the sharing: two tenants on the same
//! `base_model` necessarily receive the **same** reference, which is a
//! cross-tenant correlator by construction. A deployment that cannot accept
//! that does not want a servant patch — it wants a base per tenant, which is
//! the decision to stop sharing.
//!
//! ## What this layer cannot enforce, and who must
//!
//! - **Caller identity.** An IOR is a bearer address (see the crate docs):
//!   whoever holds tenant A's factory key *is* tenant A here. This service can
//!   guarantee that it never hands one tenant another's reference; it cannot
//!   guarantee that one was not obtained some other way. **Owner: the MCP
//!   boundary** — capability handles instead of raw IORs (PLAN §4.7, Phase
//!   3.5) and the guard chain's authn stage (F4). Between native peers,
//!   CSIv2 identity (PHASE5).
//! - **The `ai_authz` scopes.** `moe.enterprise.model.create`, `.retire`,
//!   `.deploy`, `.compose`, `.policy.write`, `.base.read`,
//!   `.weights.read`. A servant has no principal to check a scope against, so
//!   checking one here would be theatre. **Owner: the guard, at the MCP
//!   boundary.** What the wire surface owes is not to make that enforcement
//!   impossible, and it does not: every gated operation is a distinct
//!   operation name on a distinct object key, and **the tenant is in the key
//!   of every request**, so a guard decides per (principal, scope, tenant,
//!   object) without decoding a single argument body. `set_policy` carries its
//!   own scope because whoever sets policy lifts every other gate — and it is
//!   correspondingly a distinct operation on the model's key, never folded
//!   into `bind_expert`.
//! - **Approval for `ai_effect: destructive`.** `retire`, `deploy`, `create`,
//!   `clone_model`, `bind_expert` and `set_policy` are all declared
//!   destructive and ride the existing approval gate at the MCP boundary. The
//!   servant's part of that bargain is to not soften them: `retire` destroys.
//! - **Placement itself.** `check_residency` *answers about* a node; nothing
//!   in this contract places anything, so a caller that never asks places
//!   wherever it likes. **Owner: whoever places** — the §6 loading policy
//!   (`orbweaver-trading`) and the deployment, which must consult the domain
//!   before placing rather than after.
//!
//! Federation is deliberately **not** built. PLAN-DEFERRED §7 makes F5 the
//! trigger only in the shape "one naming/trading domain per tenant, in
//! separate processes"; this is the other shape — one graph, per-tenant keys,
//! isolation as an authorization property — so the trigger has not fired, and
//! adding a second isolation mechanism would repeat the mistake that chapter
//! names.
//!
//! # The three relationships a manifest holds
//!
//! Three of [`Manifest`]'s six members are not data *about* the model — they
//! **name other objects**: `base_model`, `experts` and `policy_domain`. The
//! standard has a service for exactly this — `CosRelationship`, whose whole
//! subject is roles, cardinality and referential integrity — and this module
//! implements none of it. What it does implement is three particular
//! relationships with three particular integrity rules, enforced by code and,
//! until D023 R1, written down nowhere at all. They are written down *here*
//! because this is where they are enforced: a rule whose home is somebody
//! else's document drifts from the code on the next change, silently, because
//! nothing compiles a sentence.
//!
//! Each is a **string**, never a reference, so a holder of a manifest can read
//! all three names and reach none of the objects — see *What nothing checks*.
//!
//! ## `base_model` — model → the shared base (N:1, immutable)
//!
//! - **Role.** A `ComposedModel` names the one `::moe::Expert` it is a delta
//!   over. The inverse role — *which models are over this base* — exists in no
//!   direction: `bases` is a set of keys and carries no back-pointer.
//! - **Cardinality.** Exactly one, never empty (`is_key_safe` refuses both an
//!   empty string and one containing `/`). Many models, of many tenants, name
//!   the same base; that sharing is the design, and the module docs on `base()`
//!   are where its consequences are argued.
//! - **Who creates it.** `create`, from the manifest, and it **mints the
//!   target if absent** (`bases.insert`) — as does `provision_expert`. So this
//!   relationship cannot dangle: `base()` has no dangling answer to give.
//! - **Who changes it.** *Nobody.* No operation of corpus/golden/23 re-points a
//!   model's base — not `bind_expert`, not `set_policy`, not `deploy`. A
//!   different base is a different model, and `create` is how one is made.
//! - **Integrity.** `bind_expert` refuses (`BAD_PARAM`) an expert whose own
//!   `base_model` differs from the model's: an adapter delta over another base
//!   is meaningless, and composing one would be a silent correctness failure.
//!
//! ## `experts` — model → its own adapters (1:N, append-only)
//!
//! - **Role.** A `ComposedModel` names the tenant's own `EnterpriseExpert`s by
//!   capability id. Each named expert lives in *this* tenant's key space, which
//!   is why the shared base — whose key names no tenant — is inert as an
//!   argument here.
//! - **Cardinality.** Zero or more; empty at creation is legal and common.
//!   Intended to be a set: `bind_expert` refuses a repeat. `create` does not —
//!   see *What nothing checks*.
//! - **Who creates links.** Two paths with **two different rules**, which is
//!   the fact this section exists to state. `create` **materialises** every
//!   capability the manifest names into a hollow `EnterpriseExpert` (cost
//!   `0.0`, empty delta, the model's own base) rather than refusing an unknown
//!   one: a manifest is a declaration of intent, the adapter bytes arrive out
//!   of band, and refusing would make the order of two unrelated deployment
//!   steps load bearing. `bind_expert` **requires the target to exist already**
//!   and answers `OBJECT_NOT_EXIST` otherwise.
//! - **Who changes it.** `bind_expert`, on the *model's* key, under
//!   `ai_authz: moe.enterprise.compose`. It only ever **appends**: the contract
//!   declares no unbind, so a composition is monotone for the life of the
//!   object.
//! - **Integrity, on the `bind_expert` path.** Same tenant (`NO_PERMISSION`,
//!   checked before existence so a refusal discloses nothing), right kind
//!   (`BAD_PARAM`), served (`OBJECT_NOT_EXIST`), same base (`BAD_PARAM`), not
//!   already bound (`BAD_PARAM`).
//!
//! ## `policy_domain` — model → its governing domain (N:1, replaceable)
//!
//! - **Role.** A `ComposedModel` names the one `PolicyDomain` that governs it:
//!   the domain is what `authorize` and `check_residency` are asked of, and
//!   what every audit line this model produces is labelled with.
//! - **Cardinality.** Exactly one, never empty. Many of a tenant's models may
//!   name one domain, and re-pointing one model leaves the others alone.
//! - **Who creates it.** `create` **mints the domain if absent**, taking its
//!   region from the manifest, and refuses (`BAD_PARAM`) if it exists with a
//!   *different* region — a domain governs one region, and two answers to
//!   `check_residency` would depend on which manifest was read last.
//! - **Who changes it.** `set_policy`, on the model's key, under
//!   `ai_authz: moe.enterprise.policy.write`. It **replaces, never mints**: an
//!   unserved domain is `OBJECT_NOT_EXIST`, and a domain whose region the
//!   model's manifest does not claim is `BAD_PARAM`.
//! - **What replacing does not do.** The domain left behind is not destroyed.
//!   Nothing in this contract destroys a `PolicyDomain` or an
//!   `EnterpriseExpert`; `retire` takes out the model and nothing it points at.
//!
//! ## Why the two mutators differ, which is itself a relationship rule
//!
//! `bind_expert` appends to a set that starts empty and has no maximum;
//! `set_policy` replaces a member that is always exactly one. That difference
//! in **cardinality** is why they are two operations with two `ai_authz`
//! scopes rather than one `update`: appending an adapter can never remove a
//! governor, and re-pointing a governor can never smuggle an adapter in. The
//! scopes already differed and the reason was already enforced; what was
//! missing is the sentence saying the reason is the cardinality.
//!
//! ## What nothing checks
//!
//! 1. **Nothing on the wire navigates any of the three.** `get_manifest` hands
//!    back six strings; no operation corpus/golden/23 declares turns a
//!    capability id or a domain name into a reference. So the only way to
//!    obtain the argument `bind_expert` and `set_policy` require is out of band
//!    ([`TenantService::expert_reference`], [`TenantService::policy_reference`],
//!    [`TenantService::provision_expert`]). This is `COMPONENTS.md`'s gap row —
//!    *`bind_expert`/`set_policy` take references no operation of the contract
//!    returns* — stated from the relationship end: **the relationships have no
//!    inverse role and no navigation operation**, which is the half
//!    `CosRelationship` would have carried.
//! 2. **`create` does not apply `bind_expert`'s two integrity checks.**
//!    Measured 2026-08-25 and reported as a **finding**, deliberately not
//!    repaired: D023 §6 says R1 changes no behaviour. A manifest may name the
//!    same capability **twice**, which `bind_expert` refuses as a repeat; and a
//!    manifest may name a capability whose `EnterpriseExpert` already exists
//!    **over a different base**, which `bind_expert` refuses as a foreign base
//!    — `or_insert_with` keeps the existing object, so the model ends up
//!    composed from an adapter its own base does not match. Both are pinned
//!    below as *measurements of today's behaviour, not endorsements of it*:
//!    making the two paths agree turns those tests red, which is the signal
//!    wanted rather than an obstacle to it.
//! 3. **Dangling is impossible rather than detected.** Not because anything
//!    looks for it: `create` materialises its targets, `bind_expert` and
//!    `set_policy` require theirs to be served, and no operation destroys a
//!    target. The graph only grows, so there is no moment at which a
//!    referential-integrity check would have anything to find.
//!
//! ## `clone_model` over all three — CosLifeCycle's own question
//!
//! `CosCompoundLifeCycle` defines `copy`/`move` **over a relationship graph**,
//! with a traversal criterion per role — *deep* (copy the target too),
//! *shallow* (drop the link), or *reference* (keep pointing at the same
//! target). `clone_model` is that operation, and the criterion it applies was
//! behaviour nobody had written down. Measured:
//!
//! - **`base_model` — reference.** The string is copied; `create`'s
//!   `bases.insert` is idempotent, so the clone and the source hand back the
//!   *same* `base()` reference. The base is neither followed nor copied.
//! - **`experts` — reference.** The ids are copied verbatim, and `create`'s
//!   materialisation loop then finds every key already present, so
//!   `or_insert_with` does nothing: **no adapter is duplicated**, and the clone
//!   composes the source's own expert objects, deltas and all.
//! - **`policy_domain` — reference.** The domain already exists with the region
//!   the copied manifest claims, so the region check passes and the clone joins
//!   the source's domain rather than getting one of its own.
//!
//! All three roles are therefore traversed with **reference** semantics and
//! none with *deep* or *shallow*. `version` is the one member replaced, a
//! duplicate is `BAD_PARAM` through the same gate as `create`, the serial is
//! fresh, and the clone is **not** deployed. The compact form of that whole
//! paragraph — and what the test asserts, because it is the form a mistake
//! cannot slip past — is that [`TenantService::served`] grows by **exactly
//! one**.
//!
//! *매니페스트가 쥔 세 개의 참조는 관계이며, 관계에는 역할·다중도·무결성 규칙이
//! 있다. 셋 다 규칙은 이미 강제되고 있었고 적혀 있지 않았을 뿐이다. `create`와
//! `bind_expert`의 규칙이 다르다는 것은 **발견**이며 이 배치에서 고치지 않는다.
//! `clone_model`은 셋 모두를 **참조**로 따라간다 — 깊은 복사도 절단도 아니다.*
//!
//! # Marshalling: by hand, against the declared layout
//!
//! Same trade as [`crate::expert_service`], for the same reason, and pinned
//! the same way: `manifest_members_are_in_the_idls_declaration_order` decodes
//! an encoded [`Manifest`] with the primitive getters in order, in both byte
//! orders and with both an empty and a multi-element `experts` sequence, so a
//! member reordered here fails independently of [`Manifest::read_from`].
//!
//! ## Two `moe::Capability`s
//!
//! corpus/golden/23 re-declares the `moe` base module because a single-file
//! oracle needs it, and its `Capability` is `{ CapabilityId id; float cost; }`
//! — **two members**, where corpus/golden/22's has nine. [`Capability`] here
//! is 23's and [`crate::expert_service::Capability`] is 22's; they are not the
//! same struct and neither is a subset view of the other. That divergence is a
//! fact about the corpus, not a modelling choice made here, and the two types
//! are kept apart rather than unified so that neither file's wire layout is
//! decided by the other's.
//!
//! # What the contract does not carry
//!
//! - **No exceptions.** Not one operation declares `raises`, so every refusal
//!   is a system exception, exactly as in [`crate::expert_service`].
//! - **No data plane.** PLAN-MOE §5: no accelerator, no kernel, no weights
//!   exist in this repository. `infer` and `process` therefore marshal the
//!   round trip honestly and return the activation **unchanged** rather than
//!   fabricating a transform. What is observable about them — and what the
//!   tests assert — is the refusal shape (`infer` before `deploy` is
//!   `BAD_INV_ORDER`) and the audit line, not a tensor.
//! - **No cost for the shared base.** `describe()` on it answers
//!   `cost = 0.0`, for the reason F4 left `specialization` empty: the manifest
//!   has no member for it and a guess would put a fabricated number where a
//!   selection query can read it.
//! - **No grant operation, no adapter upload, no node table.** Grants, adapter
//!   deltas and the node → region mapping arrive out of band
//!   ([`TenantService::grant`], [`TenantService::provision_expert`],
//!   [`TenantService::declare_node`]) because the contract declares no
//!   operation for any of them. Inventing a wire `grant` would be inventing an
//!   authorization surface — the one thing a servant must never do quietly.
//!
//! # Sharing: one `RwLock` over the whole graph, and why not one per tenant
//!
//! This servant implements [`SharedDispatch`], so two calls may run at once.
//! The obvious sharding — a lock per tenant, since tenancy is the whole point
//! of the module — is **not** what it does, and the reason is worth stating
//! because it looks like the natural answer:
//!
//! - **`create` is not confined to one tenant's maps.** It mints into
//!   `policies`, `experts` and `models`, inserts the *shared* base into
//!   `bases`, and takes a serial from `next_serial` — a counter that is global
//!   by design, because §4's fourth claim is that a serial is never reused and
//!   a per-tenant counter would make "never reused" a per-tenant property
//!   instead. Sharding would put one operation across two locks, which is the
//!   ordering hazard [`orbweaver_giop::guarded`] exists to make impossible.
//! - **The duplicate-version check reads every model**, not one tenant's. It
//!   could be indexed per tenant; the point is that today's isolation proof
//!   holds over one consistent view of the graph, and a sharded version would
//!   need its own proof rather than inheriting this one. Isolation is the
//!   property this module is *for*, so it does not get re-argued to save a
//!   lock.
//!
//! What it does take is the **read** half. `get_manifest`, `describe`,
//! `adapter_delta`, `get_tenant_id`, `authorize`, `check_residency` and
//! `_is_a` change nothing, and they are the operations a tenant's control loop
//! and its policy decision point actually call in a loop. Those now overlap —
//! with each other and with another tenant's — while `create`, `retire`,
//! `deploy`, `bind_expert`, `set_policy`, `infer`, `audit`, `process` and
//! `base` take the write half. (`infer`, `process`, `audit` and `base` are
//! writes because they *append to the audit log*, which is state; the module
//! docs already note that `audit` changes state while carrying no `ai_effect`
//! annotation, and this is where that observation has a consequence.)
//!
//! Nothing here dials anything — reference arguments are resolved by key and
//! never invoked, which §4 states as a security property and which happens
//! also to mean no outbound call can be made from inside the lock.
//!
//! One thing stopped being unreachable. `knows` and the dispatch are two
//! separate looks at the graph, so a model can be retired between them: the
//! `OBJECT_NOT_EXIST` arms below, written as exceptions rather than `expect`s
//! precisely because a wire-reachable servant must have no panic path, are now
//! *reachable* rather than merely defensive. The comment on them was right for
//! a reason that has changed.
//!
//! # An observation for the contract owner
//!
//! corpus/golden/23's annotations do not cover every operation: `get_manifest`,
//! `infer`, `audit`, and the two inherited `::moe::Expert` operations carry no
//! `ai_effect`. `audit` is the interesting one — it *changes state* (it
//! appends) while carrying no effect annotation, so a guard reading
//! annotations alone would classify a state change as unclassified rather than
//! as read-only or destructive. This servant makes the narrowest honest choice
//! available to it — the log is append-only, with no wire operation that reads
//! or truncates it — and records the gap here rather than editing the corpus,
//! which is F1's territory and not this batch's footprint.

use std::collections::{BTreeMap, BTreeSet};

use orbweaver_cdr::{Decoder, Encoder};
use orbweaver_giop::guarded::Guarded;
use orbweaver_giop::server::{Completion, Dispatch, Request, SharedDispatch, SystemException};
use orbweaver_giop::{IiopProfile, Ior, Version};

use crate::expert_service::EXPERT_ID;
use crate::{OBJECT_ID, get_reference, put_reference};

/// Repository id of `moe::enterprise::ModelFactory`.
pub const MODEL_FACTORY_ID: &str = "IDL:moe/enterprise/ModelFactory:1.0";
/// Repository id of `moe::enterprise::ComposedModel`.
pub const COMPOSED_MODEL_ID: &str = "IDL:moe/enterprise/ComposedModel:1.0";
/// Repository id of `moe::enterprise::PolicyDomain`.
pub const POLICY_DOMAIN_ID: &str = "IDL:moe/enterprise/PolicyDomain:1.0";
/// Repository id of `moe::enterprise::EnterpriseExpert`.
pub const ENTERPRISE_EXPERT_ID: &str = "IDL:moe/enterprise/EnterpriseExpert:1.0";

/// The refusals this contract can express, re-exported rather than
/// re-declared: they are OMG repository ids, not this module's invention, and
/// a second definition of the same string is a second thing to keep in step.
///
/// `BAD_PARAM` — a malformed manifest, a reference that is not one of ours, a
/// duplicate version, a bind that contradicts the base or repeats itself.
/// `NO_PERMISSION` — a tenancy crossing. `BAD_INV_ORDER` — an operation the
/// object is not in a state for (`infer` before `deploy`, a second `deploy`).
/// `OBJECT_NOT_EXIST` — a retired object, whether it was addressed directly or
/// handed in as an argument.
pub use crate::expert_service::{BAD_INV_ORDER, BAD_PARAM, NO_PERMISSION};
pub use orbweaver_giop::server::OBJECT_NOT_EXIST;

// ─────────────────────────────────────────────────────────────────────────────
// corpus/golden/23's types, member for member
// ─────────────────────────────────────────────────────────────────────────────

/// `moe::enterprise::Manifest`, member for member and in declaration order.
///
/// Verified against `corpus/golden/23-moe-enterprise.idl` lines 18–25:
///
/// | # | IDL | CDR | Rust |
/// |---|---|---|---|
/// | 1 | `string tenant_id` | string | `String` |
/// | 2 | `string base_model` | string | `String` |
/// | 3 | `sequence< ::moe::CapabilityId> experts` | ulong count, then that many strings | `Vec<String>` |
/// | 4 | `string policy_domain` | string | `String` |
/// | 5 | `string version` | string | `String` |
/// | 6 | `string residency_region` | string | `String` |
///
/// Every member is a string or a sequence of them, so nothing here is
/// alignment-sensitive past the 4-byte length prefixes — which is precisely
/// why an ordering mistake would round-trip cleanly through
/// [`Manifest::read_from`] and has to be pinned member by member instead.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Manifest {
    /// `string tenant_id` — the tenant this model belongs to. The key minted
    /// for the model repeats it, and the two must agree; `create` refuses
    /// otherwise.
    pub tenant_id: String,
    /// `string base_model` — **a relationship**: names the shared base, which
    /// is *not* owned by this tenant. N:1, immutable, mints its own target.
    /// Its role, cardinality and integrity rule live in one place, the module
    /// docs' *three relationships* section; see also the docs on `base()`.
    pub base_model: String,
    /// `sequence< ::moe::CapabilityId> experts` — **a relationship**: the
    /// tenant's own adapters, by capability id. 1:N and append-only. Empty at
    /// creation is legal and common; `create` materialises what it names and
    /// `bind_expert` is what grows it afterwards, under two rules that differ.
    /// Module docs, *three relationships*.
    pub experts: Vec<String>,
    /// `string policy_domain` — **a relationship**: names the `PolicyDomain`
    /// governing this model. Exactly one, replaceable. `set_policy` changes it,
    /// which is why it carries its own scope — the reason is the cardinality,
    /// and the module docs' *three relationships* section is where that is
    /// argued.
    pub policy_domain: String,
    /// `string version` — unique within a tenant; `clone_model` exists to make
    /// a second one.
    pub version: String,
    /// `string residency_region` — the region placements must stay inside.
    /// `PolicyDomain::check_residency` is the operation that answers about it.
    pub residency_region: String,
}

impl Manifest {
    /// Marshals the struct in declaration order. Struct members are laid out
    /// consecutively with each member's own alignment (§9.3.2.5); a
    /// `sequence` is an unsigned long count followed by that many elements
    /// (§9.3.2.7), with no encapsulation.
    pub fn write_to(&self, out: &mut Encoder) {
        out.put_str(&self.tenant_id);
        out.put_str(&self.base_model);
        out.put_u32(self.experts.len() as u32);
        for id in &self.experts {
            out.put_str(id);
        }
        out.put_str(&self.policy_domain);
        out.put_str(&self.version);
        out.put_str(&self.residency_region);
    }

    /// Demarshals what [`Manifest::write_to`] wrote.
    ///
    /// The element count is checked against the remaining bytes before
    /// anything is allocated — a four-byte count claiming four billion strings
    /// is the classic wire-parsing denial of service, and `validate_count` is
    /// what the CDR layer provides to refuse it.
    pub fn read_from(d: &mut Decoder<'_>) -> orbweaver_cdr::Result<Self> {
        let tenant_id = d.get_string()?;
        let base_model = d.get_string()?;
        let count = d.get_u32()?;
        // A string is at least its own 4-byte length prefix, so that is the
        // minimum element size this count has to be plausible against.
        let n = d.validate_count(count, 4)?;
        let mut experts = Vec::with_capacity(n);
        for _ in 0..n {
            experts.push(d.get_string()?);
        }
        Ok(Manifest {
            tenant_id,
            base_model,
            experts,
            policy_domain: d.get_string()?,
            version: d.get_string()?,
            residency_region: d.get_string()?,
        })
    }
}

/// `moe::Activation`, from corpus/golden/23 line 8:
/// `struct Activation { Tensor data; string dtype; string shape; };`
/// with `typedef sequence<octet> Tensor`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Activation {
    /// `Tensor data` — `sequence<octet>`: a count and that many bytes.
    pub data: Vec<u8>,
    /// `string dtype`.
    pub dtype: String,
    /// `string shape`.
    pub shape: String,
}

impl Activation {
    /// Marshals the struct in declaration order.
    pub fn write_to(&self, out: &mut Encoder) {
        out.put_octet_seq(&self.data);
        out.put_str(&self.dtype);
        out.put_str(&self.shape);
    }

    /// Demarshals what [`Activation::write_to`] wrote.
    pub fn read_from(d: &mut Decoder<'_>) -> orbweaver_cdr::Result<Self> {
        let data = d.get_octet_seq()?.to_vec();
        Ok(Activation { data, dtype: d.get_string()?, shape: d.get_string()? })
    }
}

/// `moe::CallContext`, from corpus/golden/23 line 9:
/// `struct CallContext { string request_id; string trace_id; unsigned long step; };`
///
/// Note what it does **not** carry: a tenant. That absence is why the object
/// key is this service's tenant context — see the module docs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CallContext {
    /// `string request_id`.
    pub request_id: String,
    /// `string trace_id`.
    pub trace_id: String,
    /// `unsigned long step`.
    pub step: u32,
}

impl CallContext {
    /// Marshals the struct in declaration order.
    pub fn write_to(&self, out: &mut Encoder) {
        out.put_str(&self.request_id);
        out.put_str(&self.trace_id);
        out.put_u32(self.step);
    }

    /// Demarshals what [`CallContext::write_to`] wrote.
    pub fn read_from(d: &mut Decoder<'_>) -> orbweaver_cdr::Result<Self> {
        Ok(CallContext {
            request_id: d.get_string()?,
            trace_id: d.get_string()?,
            step: d.get_u32()?,
        })
    }
}

/// `moe::Capability` **as corpus/golden/23 declares it**:
/// `struct Capability { CapabilityId id; float cost; };` — two members.
///
/// Not [`crate::expert_service::Capability`], which is corpus/golden/22's
/// nine-member struct of the same scoped name. See the module docs.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Capability {
    /// `CapabilityId id` (`typedef string`).
    pub id: String,
    /// `float cost` — IDL `float` is 4 bytes, so `f32` and not `f64`.
    pub cost: f32,
}

impl Capability {
    /// Marshals the struct in declaration order.
    pub fn write_to(&self, out: &mut Encoder) {
        out.put_str(&self.id);
        out.put_f32(self.cost);
    }

    /// Demarshals what [`Capability::write_to`] wrote.
    pub fn read_from(d: &mut Decoder<'_>) -> orbweaver_cdr::Result<Self> {
        Ok(Capability { id: d.get_string()?, cost: d.get_f32()? })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Keys, which are this service's tenant credential
// ─────────────────────────────────────────────────────────────────────────────

/// What kind of object a key names.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// `<base>/t/<tenant>/factory`.
    Factory,
    /// `<base>/t/<tenant>/model/<serial>`.
    Model,
    /// `<base>/t/<tenant>/expert/<capability>`.
    Expert,
    /// `<base>/t/<tenant>/policy/<domain>`.
    Policy,
    /// `<base>/shared/base/<base_model>` — the one key with no tenant.
    Base,
}

/// A parsed object key: what it names, and for whom.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Addressed {
    /// `None` only for the shared base, which belongs to no tenant.
    tenant: Option<String>,
    kind: Kind,
}

/// Whether a string may become part of an object key.
///
/// Empty is refused because an empty component makes two different keys equal
/// after concatenation; `/` is refused because it is the separator, and a
/// tenant able to put one in its own id could name another tenant's key. The
/// key is the credential here, so this is not input hygiene — it is the
/// credential's integrity.
fn is_key_safe(s: &str) -> bool {
    !s.is_empty() && !s.contains('/')
}

// ─────────────────────────────────────────────────────────────────────────────
// The service state
// ─────────────────────────────────────────────────────────────────────────────

/// One line of a tenant's audit trail.
///
/// The log is **per tenant**, because the isolation property under test is per
/// tenant; the domain that produced the line is a label on the entry rather
/// than a separate log, so a cross-tenant leak would be visible as one entry in
/// the wrong log instead of having to be reassembled from several.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// The `PolicyDomain` the entry was recorded through, or the model's
    /// domain for an entry the service raised itself. Empty when the
    /// originating object has no domain — an `EnterpriseExpert` has a tenant
    /// but no governing domain.
    pub domain: String,
    /// `CallContext::request_id`, empty for a service-raised entry.
    pub request_id: String,
    /// `CallContext::trace_id`, empty for a service-raised entry.
    pub trace_id: String,
    /// `CallContext::step`, zero for a service-raised entry.
    pub step: u32,
    /// What happened. For a wire `audit` call this is the caller's `event`
    /// string verbatim; for a service-raised entry it names the operation.
    pub event: String,
}

/// A `ComposedModel` instance.
#[derive(Debug, Clone)]
struct Model {
    manifest: Manifest,
    /// `deploy` sets it; `infer` refuses until it is set. A second `deploy` is
    /// `BAD_INV_ORDER` for the reason F3 refuses a missing edge: "already
    /// there" is a different answer from "done".
    deployed: bool,
}

/// An `EnterpriseExpert` instance: a tenant's own adapter over a shared base.
#[derive(Debug, Clone)]
struct ExpertObject {
    tenant: String,
    capability: String,
    /// Which shared base this adapter was trained against. `bind_expert`
    /// refuses an expert whose base differs from the model's — an adapter
    /// delta is meaningless over a different base, and composing one would be
    /// a silent correctness failure rather than a loud one.
    base_model: String,
    cost: f32,
    /// `adapter_delta()`'s bytes. No weights exist in this repository
    /// (PLAN-MOE §5); whatever a deployment loaded arrives out of band.
    delta: Vec<u8>,
}

/// A `PolicyDomain` instance.
///
/// No `tenant` field: the key carries it, and a second copy would be a second
/// answer to "whose is this" that could drift from the one the tenancy checks
/// actually read.
#[derive(Debug, Clone)]
struct PolicyObject {
    name: String,
    /// From the manifest that first named this domain. A second manifest
    /// naming the same domain with a different region is refused rather than
    /// merged: a domain governs one region, and two answers to
    /// `check_residency` would depend on which manifest was read last.
    region: String,
    /// `(principal, capability)` pairs `authorize` answers `true` for.
    /// Default-deny: an ungranted pair is `false`, never an error.
    grants: BTreeSet<(String, String)>,
}

/// `moe::enterprise`'s four interfaces, served together.
///
/// One servant and many objects, because the objects share state that has to
/// move together: `create` mints a model, its experts and its policy domain in
/// one step, and `retire` has to remove exactly one of them. They stay
/// distinct *objects* — one key each, one repository id each, [`Dispatch::knows`]
/// answering for all of them — because the contract declares four interfaces
/// and a client narrows to one.
/// Everything this service serves, behind one lock.
///
/// See the module docs on sharing for why it is one lock over the whole graph
/// and not one per tenant. Nothing immutable lives here: the key prefix and
/// the published address are constants a reader must never take a lock to
/// look at.
#[derive(Debug)]
struct TenantState {
    factories: BTreeSet<Vec<u8>>,
    models: BTreeMap<Vec<u8>, Model>,
    experts: BTreeMap<Vec<u8>, ExpertObject>,
    policies: BTreeMap<Vec<u8>, PolicyObject>,
    /// Shared base experts, by key. A set and not a map: the only thing a base
    /// object knows is its own name, which its key already carries.
    bases: BTreeSet<Vec<u8>>,
    /// Placement node → region. A deployment fact the contract has no member
    /// for, so it arrives out of band rather than being guessed from a node's
    /// name — inventing a naming convention would make `check_residency`
    /// answer confidently about nodes nobody described.
    nodes: BTreeMap<String, String>,
    audits: BTreeMap<String, Vec<AuditEntry>>,
    crossings: BTreeMap<String, u64>,
    /// Never reset and never reused, so a reference to a retired model can
    /// never land on a later one. Global rather than per tenant, which is one
    /// of the reasons the lock is global too.
    next_serial: u64,
}

/// The key prefix, and the arithmetic that turns names into object keys.
///
/// Split out from the state because it is a **constant**, and because every
/// operation that mints or resolves needs it while holding the lock: a key
/// built inside the section from a value that cannot change is not shared
/// state, and treating it as such would put the whole key space behind the
/// same contention as the graph.
#[derive(Debug)]
struct Keys {
    base: String,
}

/// `moe::enterprise`'s four interfaces, served together — see the type docs
/// below the state it holds.
#[derive(Debug)]
pub struct TenantService {
    host: String,
    port: u16,
    keys: Keys,
    state: Guarded<TenantState>,
}

impl Keys {
    fn factory_key(&self, tenant: &str) -> Vec<u8> {
        format!("{}/t/{tenant}/factory", self.base).into_bytes()
    }

    fn model_key(&self, tenant: &str, serial: u64) -> Vec<u8> {
        format!("{}/t/{tenant}/model/{serial}", self.base).into_bytes()
    }

    fn expert_key(&self, tenant: &str, capability: &str) -> Vec<u8> {
        format!("{}/t/{tenant}/expert/{capability}", self.base).into_bytes()
    }

    fn policy_key(&self, tenant: &str, domain: &str) -> Vec<u8> {
        format!("{}/t/{tenant}/policy/{domain}", self.base).into_bytes()
    }

    fn base_key(&self, base_model: &str) -> Vec<u8> {
        format!("{}/shared/base/{base_model}", self.base).into_bytes()
    }

    /// The base model a shared-base key names.
    fn base_name(&self, key: &[u8]) -> Option<String> {
        let s = std::str::from_utf8(key).ok()?;
        let prefix = format!("{}/shared/base/", self.base);
        s.strip_prefix(&prefix).map(str::to_owned)
    }

    /// Parses a key this service could have minted.
    ///
    /// Anchored at `<base>/`, so a key from another service — or a key with
    /// our prefix embedded somewhere inside it — is not ours.
    fn parse(&self, key: &[u8]) -> Option<Addressed> {
        let s = std::str::from_utf8(key).ok()?;
        let rest = s.strip_prefix(&self.base)?.strip_prefix('/')?;
        if let Some(model) = rest.strip_prefix("shared/base/") {
            return if is_key_safe(model) {
                Some(Addressed { tenant: None, kind: Kind::Base })
            } else {
                None
            };
        }
        let (tenant, rest) = rest.strip_prefix("t/")?.split_once('/')?;
        if !is_key_safe(tenant) {
            return None;
        }
        let kind = if rest == "factory" {
            Kind::Factory
        } else {
            let (what, name) = rest.split_once('/')?;
            if !is_key_safe(name) {
                return None;
            }
            match what {
                "model" => Kind::Model,
                "expert" => Kind::Expert,
                "policy" => Kind::Policy,
                _ => return None,
            }
        };
        Some(Addressed { tenant: Some(tenant.to_owned()), kind })
    }
}

impl TenantService {
    /// A service whose references point at `host:port`, with every key it
    /// mints under `base`.
    ///
    /// `host` is separate from the bind address on purpose — Phase 0
    /// assumption D. `base` must not be empty; it may contain anything else,
    /// because key parsing anchors on the full `<base>/t/` prefix rather than
    /// searching for it.
    pub fn new(host: impl Into<String>, port: u16, base: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port,
            keys: Keys { base: base.into() },
            state: Guarded::new(
                "the tenant object graph",
                TenantState {
                    factories: BTreeSet::new(),
                    models: BTreeMap::new(),
                    experts: BTreeMap::new(),
                    policies: BTreeMap::new(),
                    bases: BTreeSet::new(),
                    nodes: BTreeMap::new(),
                    audits: BTreeMap::new(),
                    crossings: BTreeMap::new(),
                    next_serial: 1,
                },
            ),
        }
    }

    /// Points the references this service mints at a different address.
    ///
    /// For a server bound to port 0, where the port is only known after the
    /// bind. Still `&mut self` while everything else became `&self`, and
    /// deliberately: the published address is a construction-time property,
    /// and the exclusive borrow is the type system saying *before you share
    /// it* — a reference already handed to a client cannot be un-minted, so
    /// changing the address mid-service would only produce two answers to
    /// "where is this object".
    pub fn publish_at(&mut self, host: impl Into<String>, port: u16) {
        self.host = host.into();
        self.port = port;
    }

    fn ior_for(&self, type_id: &str, key: &[u8]) -> Ior {
        Ior {
            type_id: type_id.to_owned(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: self.host.clone(),
                port: self.port,
                object_key: key.to_vec(),
                // §7.10.2.4: no TAG_CODE_SETS is a declaration of no wchar
                // support, and a conformant client then refuses inside itself
                // without sending anything (measured, omniORB 4.3.4). D009's
                // L2, landed with the rest of its cause rather than one site
                // at a time: the conversion lists stay empty, so this
                // advertises UTF-8 — which we have — and nothing we do not.
                components: vec![orbweaver_giop::codeset::server_component()],
            }],
        }
    }

    // ── provisioning, which the contract has no operations for ──────────────

    /// Mints (or returns) tenant `tenant`'s `ModelFactory` reference.
    ///
    /// Not a wire operation, and deliberately: the IDL declares no operation
    /// that produces a factory, so a bootstrap operation would be one this
    /// contract does not have. A deployment hands each tenant its own factory
    /// reference the way any well-known reference is handed out — and, at the
    /// MCP boundary, as a capability handle rather than as the IOR.
    ///
    /// `None` for a tenant id that cannot be part of a key.
    pub fn provision_factory(&self, tenant: &str) -> Option<Ior> {
        if !is_key_safe(tenant) {
            return None;
        }
        let key = self.keys.factory_key(tenant);
        self.state.write(|s| s.factories.insert(key.clone()));
        Some(self.ior_for(MODEL_FACTORY_ID, &key))
    }

    /// Mints (or replaces) an `EnterpriseExpert` for `tenant`.
    ///
    /// The adapter bytes arrive here because no weights exist in this
    /// repository (PLAN-MOE §5) and the contract declares no upload. The
    /// shared base named by `base_model` is minted alongside if it does not
    /// exist yet — an adapter over a base nobody serves would make `base()`
    /// answer with a dangling reference.
    ///
    /// `None` if any component cannot be part of a key.
    pub fn provision_expert(
        &self,
        tenant: &str,
        capability: &str,
        base_model: &str,
        cost: f32,
        delta: &[u8],
    ) -> Option<Ior> {
        if !is_key_safe(tenant) || !is_key_safe(capability) || !is_key_safe(base_model) {
            return None;
        }
        let base = self.keys.base_key(base_model);
        let key = self.keys.expert_key(tenant, capability);
        // One section for both inserts: an expert whose shared base is not
        // there yet is a `base()` that dangles, and the two must never be
        // observable apart.
        self.state.write(|s| {
            s.bases.insert(base);
            s.experts.insert(
                key.clone(),
                ExpertObject {
                    tenant: tenant.to_owned(),
                    capability: capability.to_owned(),
                    base_model: base_model.to_owned(),
                    cost,
                    delta: delta.to_vec(),
                },
            );
        });
        Some(self.ior_for(ENTERPRISE_EXPERT_ID, &key))
    }

    /// Records that `node` is in `region`.
    ///
    /// The only source of truth `check_residency` has. An undeclared node is
    /// refused rather than assumed local — see the module docs.
    pub fn declare_node(&self, node: &str, region: &str) {
        self.state.write(|s| s.nodes.insert(node.to_owned(), region.to_owned()));
    }

    /// Grants `principal` the capability `target` inside one of `tenant`'s
    /// policy domains, so `authorize` answers `true` for the pair.
    ///
    /// Out of band for the reason given in the module docs: a wire `grant`
    /// would be an authorization surface this contract does not declare, and a
    /// servant that invents one has quietly become the policy decision point.
    /// `false` if the domain does not exist.
    pub fn grant(&self, tenant: &str, domain: &str, principal: &str, target: &str) -> bool {
        let key = self.keys.policy_key(tenant, domain);
        self.state.write(|s| match s.policies.get_mut(&key) {
            Some(p) => p.grants.insert((principal.to_owned(), target.to_owned())),
            None => false,
        })
    }

    // ── read-only views, for a control loop and for tests ───────────────────
    //
    // Every one of these returns an **owned** value. Returning a borrow would
    // mean returning the lock guard that keeps it alive, and a guard the
    // caller holds is a lock held across whatever the caller does next — which
    // is the one thing `orbweaver_giop::guarded` exists to prevent.

    /// A reference to `tenant`'s `PolicyDomain` named `domain`, if it exists.
    pub fn policy_reference(&self, tenant: &str, domain: &str) -> Option<Ior> {
        let key = self.keys.policy_key(tenant, domain);
        self.state
            .read(|s| s.policies.contains_key(&key))
            .then(|| self.ior_for(POLICY_DOMAIN_ID, &key))
    }

    /// A reference to `tenant`'s `EnterpriseExpert` for `capability`.
    pub fn expert_reference(&self, tenant: &str, capability: &str) -> Option<Ior> {
        let key = self.keys.expert_key(tenant, capability);
        self.state
            .read(|s| s.experts.contains_key(&key))
            .then(|| self.ior_for(ENTERPRISE_EXPERT_ID, &key))
    }

    /// A reference to the shared base expert for `base_model`, typed
    /// `::moe::Expert` — the same reference every tenant on that base gets.
    pub fn shared_base_reference(&self, base_model: &str) -> Option<Ior> {
        let key = self.keys.base_key(base_model);
        self.state.read(|s| s.bases.contains(&key)).then(|| self.ior_for(EXPERT_ID, &key))
    }

    /// The manifest of the model at `key`, if it is still served.
    pub fn manifest_at(&self, key: &[u8]) -> Option<Manifest> {
        self.state.read(|s| s.models.get(key).map(|m| m.manifest.clone()))
    }

    /// `tenant`'s audit trail, oldest first.
    pub fn audit_log(&self, tenant: &str) -> Vec<AuditEntry> {
        self.state.read(|s| s.audits.get(tenant).cloned().unwrap_or_default())
    }

    /// How many times `tenant` has crossed its own boundary through `base()`.
    pub fn base_crossings(&self, tenant: &str) -> u64 {
        self.state.read(|s| s.crossings.get(tenant).copied().unwrap_or(0))
    }

    /// How many objects this service currently serves — the number `retire`
    /// decrements and nothing else does.
    pub fn served(&self) -> usize {
        self.state.read(|s| {
            s.factories.len() + s.models.len() + s.experts.len() + s.policies.len() + s.bases.len()
        })
    }
}

impl TenantState {
    fn record(&mut self, tenant: &str, domain: &str, ctx: &CallContext, event: &str) {
        self.audits.entry(tenant.to_owned()).or_default().push(AuditEntry {
            domain: domain.to_owned(),
            request_id: ctx.request_id.clone(),
            trace_id: ctx.trace_id.clone(),
            step: ctx.step,
            event: event.to_owned(),
        });
    }

    // ── the operations ──────────────────────────────────────────────────────

    /// Resolves a reference argument against the calling object's tenant.
    ///
    /// The order of the checks is the security property, not a style: the
    /// tenancy check runs **before** the existence check, so a caller can
    /// never learn whether another tenant's object exists by comparing
    /// `NO_PERMISSION` against `OBJECT_NOT_EXIST`. The address in the argument
    /// is not consulted at all — the key is the identity we minted, we never
    /// dial an argument reference, and §7.2.1 makes address comparison unable
    /// to refute identity anyway.
    fn resolve(
        &self,
        keys: &Keys,
        tenant: &str,
        argument: Option<Ior>,
        want: Kind,
    ) -> Result<Vec<u8>, SystemException> {
        // A nil reference, or one with no IIOP profile: nothing to resolve.
        let ior = argument.ok_or_else(|| system(BAD_PARAM))?;
        let profile = ior.primary().map_err(|_| system(BAD_PARAM))?;
        let key = profile.object_key.clone();
        // Not a key we mint, or the shared base, which names no tenant and is
        // therefore inert as an argument — the module docs' bound on the
        // `base()` crossing.
        let Some(addressed) = keys.parse(&key) else {
            return Err(system(BAD_PARAM));
        };
        let Some(owner) = addressed.tenant else {
            return Err(system(BAD_PARAM));
        };
        if owner != tenant {
            return Err(system(NO_PERMISSION));
        }
        if addressed.kind != want {
            return Err(system(BAD_PARAM));
        }
        let served = match want {
            Kind::Model => self.models.contains_key(&key),
            Kind::Expert => self.experts.contains_key(&key),
            Kind::Policy => self.policies.contains_key(&key),
            Kind::Factory | Kind::Base => false,
        };
        // The right kind, this tenant's, and gone: that is `OBJECT_NOT_EXIST`
        // and not `BAD_PARAM`. Retiring twice must say the object is gone.
        if served { Ok(key) } else { Err(system(OBJECT_NOT_EXIST)) }
    }

    /// `ComposedModel create(in Manifest m)`, returning the new model's key.
    ///
    /// The key and not the reference: minting an `Ior` needs the published
    /// address, which lives outside the lock because it cannot change. The
    /// caller wraps it once the section has closed.
    fn create(
        &mut self,
        keys: &Keys,
        tenant: &str,
        m: Manifest,
    ) -> Result<Vec<u8>, SystemException> {
        // Cross-tenant creation, refused before anything is validated: the
        // answer must not depend on whether the other tenant's manifest was
        // well formed.
        if m.tenant_id != tenant {
            return Err(system(NO_PERMISSION));
        }
        for part in [&m.tenant_id, &m.base_model, &m.policy_domain, &m.version, &m.residency_region]
        {
            if !is_key_safe(part) {
                return Err(system(BAD_PARAM));
            }
        }
        if m.experts.iter().any(|c| !is_key_safe(c)) {
            return Err(system(BAD_PARAM));
        }
        // A version is a tenant's handle on a model; two models sharing one
        // would make `clone_model`'s output ambiguous to its own caller.
        if self
            .models
            .values()
            .any(|x| x.manifest.tenant_id == m.tenant_id && x.manifest.version == m.version)
        {
            return Err(system(BAD_PARAM));
        }
        let pkey = keys.policy_key(tenant, &m.policy_domain);
        match self.policies.get(&pkey) {
            Some(p) if p.region != m.residency_region => return Err(system(BAD_PARAM)),
            Some(_) => {}
            None => {
                self.policies.insert(
                    pkey,
                    PolicyObject {
                        name: m.policy_domain.clone(),
                        region: m.residency_region.clone(),
                        grants: BTreeSet::new(),
                    },
                );
            }
        }
        // The manifest may name experts nobody provisioned yet. They are minted
        // empty rather than refused: a manifest is a declaration of intent, the
        // adapter bytes arrive out of band, and refusing here would make the
        // order in which a deployment does two unrelated things load bearing.
        for capability in &m.experts {
            let key = keys.expert_key(tenant, capability);
            self.experts.entry(key).or_insert_with(|| ExpertObject {
                tenant: tenant.to_owned(),
                capability: capability.clone(),
                base_model: m.base_model.clone(),
                cost: 0.0,
                delta: Vec::new(),
            });
        }
        self.bases.insert(keys.base_key(&m.base_model));
        let serial = self.next_serial;
        self.next_serial += 1;
        let key = keys.model_key(tenant, serial);
        let domain = m.policy_domain.clone();
        let version = m.version.clone();
        self.models.insert(key.clone(), Model { manifest: m, deployed: false });
        self.record(tenant, &domain, &CallContext::default(), &format!("create {version}"));
        Ok(key)
    }

    /// `ComposedModel clone_model(in ComposedModel src, in string new_version)`.
    fn clone_model(
        &mut self,
        keys: &Keys,
        tenant: &str,
        src: Option<Ior>,
        new_version: &str,
    ) -> Result<Vec<u8>, SystemException> {
        let key = self.resolve(keys, tenant, src, Kind::Model)?;
        let source = self.models.get(&key).ok_or_else(SystemException::object_not_exist)?;
        let mut manifest = source.manifest.clone();
        manifest.version = new_version.to_owned();
        // Through `create`, so a clone gets the same validation, the same
        // duplicate-version refusal and the same fresh serial as an original.
        // A clone is *not* deployed: it is a new object, and deployment is a
        // decision about a particular one. Both halves happen inside the one
        // section the caller opened, so no other request can slip between the
        // read of the source and the mint of the copy.
        self.create(keys, tenant, manifest)
    }

    /// `void retire(in ComposedModel m)`. `ai_effect: destructive`, and it is.
    fn retire(&mut self, keys: &Keys, tenant: &str, m: Option<Ior>) -> Result<(), SystemException> {
        let key = self.resolve(keys, tenant, m, Kind::Model)?;
        // `resolve` already proved this is served, so the `ok_or_else` arms
        // here and below are unreachable — written as an exception rather than
        // an `expect` because a servant reached from the wire must have no
        // panic path at all, not even one that "cannot" be taken.
        let model = self.models.remove(&key).ok_or_else(SystemException::object_not_exist)?;
        // The tenant's experts and policy domains survive: they belong to the
        // tenant, not to the model, and the contract declares no operation
        // that destroys either. Retiring a model must not silently take out
        // the adapters its sibling versions are still composed from.
        self.record(
            tenant,
            &model.manifest.policy_domain,
            &CallContext::default(),
            &format!("retire {}", model.manifest.version),
        );
        Ok(())
    }

    /// `void deploy(in ComposedModel m)`.
    fn deploy(&mut self, keys: &Keys, tenant: &str, m: Option<Ior>) -> Result<(), SystemException> {
        let key = self.resolve(keys, tenant, m, Kind::Model)?;
        let model = self.models.get_mut(&key).ok_or_else(SystemException::object_not_exist)?;
        if model.deployed {
            return Err(system(BAD_INV_ORDER));
        }
        model.deployed = true;
        let (domain, version) =
            (model.manifest.policy_domain.clone(), model.manifest.version.clone());
        self.record(tenant, &domain, &CallContext::default(), &format!("deploy {version}"));
        Ok(())
    }

    /// `void bind_expert(in EnterpriseExpert ex)`.
    fn bind_expert(
        &mut self,
        keys: &Keys,
        tenant: &str,
        model_key: &[u8],
        ex: Option<Ior>,
    ) -> Result<(), SystemException> {
        let expert_key = self.resolve(keys, tenant, ex, Kind::Expert)?;
        let expert = self.experts.get(&expert_key).ok_or_else(SystemException::object_not_exist)?;
        let (capability, base_model) = (expert.capability.clone(), expert.base_model.clone());
        let model = self.models.get_mut(model_key).ok_or_else(SystemException::object_not_exist)?;
        // An adapter delta is meaningless over a different base. Composing one
        // would be a silent correctness failure; this makes it a loud one.
        if model.manifest.base_model != base_model {
            return Err(system(BAD_PARAM));
        }
        if model.manifest.experts.contains(&capability) {
            return Err(system(BAD_PARAM));
        }
        model.manifest.experts.push(capability.clone());
        let domain = model.manifest.policy_domain.clone();
        self.record(tenant, &domain, &CallContext::default(), &format!("bind {capability}"));
        Ok(())
    }

    /// `void set_policy(in PolicyDomain p)`.
    fn set_policy(
        &mut self,
        keys: &Keys,
        tenant: &str,
        model_key: &[u8],
        p: Option<Ior>,
    ) -> Result<(), SystemException> {
        let policy_key = self.resolve(keys, tenant, p, Kind::Policy)?;
        let policy =
            self.policies.get(&policy_key).ok_or_else(SystemException::object_not_exist)?;
        let (name, region) = (policy.name.clone(), policy.region.clone());
        let model = self.models.get_mut(model_key).ok_or_else(SystemException::object_not_exist)?;
        // A domain governs one region and the manifest declares one. Letting
        // them disagree would make `check_residency` answer about a region the
        // model's own manifest does not claim.
        if model.manifest.residency_region != region {
            return Err(system(BAD_PARAM));
        }
        model.manifest.policy_domain = name.clone();
        self.record(tenant, &name, &CallContext::default(), "set_policy");
        Ok(())
    }

    /// `::moe::Activation infer(in ::moe::Activation x, in ::moe::CallContext ctx)`.
    fn infer(
        &mut self,
        tenant: &str,
        model_key: &[u8],
        x: Activation,
        ctx: &CallContext,
    ) -> Result<Activation, SystemException> {
        let model = self.models.get(model_key).ok_or_else(SystemException::object_not_exist)?;
        if !model.deployed {
            return Err(system(BAD_INV_ORDER));
        }
        let domain = model.manifest.policy_domain.clone();
        let version = model.manifest.version.clone();
        self.record(tenant, &domain, ctx, &format!("infer {version}"));
        // Unchanged, on purpose: PLAN-MOE §5. See the module docs.
        Ok(x)
    }

    /// `::moe::Expert base()` — the crossing, made visible.
    fn base(&mut self, keys: &Keys, expert_key: &[u8]) -> Result<Vec<u8>, SystemException> {
        let expert = self.experts.get(expert_key).ok_or_else(SystemException::object_not_exist)?;
        let (tenant, base_model) = (expert.tenant.clone(), expert.base_model.clone());
        let key = keys.base_key(&base_model);
        if !self.bases.contains(&key) {
            // Unreachable through any path that mints an expert, both of which
            // mint the base too — but a dangling reference is the one answer
            // this operation must never give.
            return Err(system(OBJECT_NOT_EXIST));
        }
        *self.crossings.entry(tenant.clone()).or_insert(0) += 1;
        self.record(
            &tenant,
            "",
            &CallContext::default(),
            &format!("base crossing to {base_model}"),
        );
        Ok(key)
    }

    /// The expert at `key`, or `OBJECT_NOT_EXIST`.
    ///
    /// Every one of these exists so that no path reachable from the wire can
    /// index a map and panic. `Server`'s `knows` has already vouched for the
    /// key — but under concurrent dispatch that vouching happened in a
    /// *separate* look at the graph, so a `retire` in between is real and this
    /// arm is now reachable rather than merely defensive. An unreachable arm
    /// that returns an exception cost nothing; a reachable one that panicked
    /// would have cost the connection.
    fn expert_at(&self, key: &[u8]) -> Result<&ExpertObject, SystemException> {
        self.experts.get(key).ok_or_else(SystemException::object_not_exist)
    }

    /// The policy domain at `key`, or `OBJECT_NOT_EXIST`.
    fn policy_at(&self, key: &[u8]) -> Result<&PolicyObject, SystemException> {
        self.policies.get(key).ok_or_else(SystemException::object_not_exist)
    }
}

fn system(id: &str) -> SystemException {
    SystemException { id: id.to_owned(), minor: 0, completed: Completion::No }
}

// ─────────────────────────────────────────────────────────────────────────────
// Dispatch
// ─────────────────────────────────────────────────────────────────────────────

impl TenantService {
    fn type_id_of(kind: &Kind) -> &'static str {
        match kind {
            Kind::Factory => MODEL_FACTORY_ID,
            Kind::Model => COMPOSED_MODEL_ID,
            Kind::Expert => ENTERPRISE_EXPERT_ID,
            Kind::Policy => POLICY_DOMAIN_ID,
            Kind::Base => EXPERT_ID,
        }
    }

    /// Parses a key this service could have minted. Needs no lock: the key
    /// prefix is a constant.
    fn parse(&self, key: &[u8]) -> Option<Addressed> {
        self.keys.parse(key)
    }

    /// The key of a model that may or may not exist — for a test that needs to
    /// address one that never did.
    #[cfg(test)]
    fn model_key(&self, tenant: &str, serial: u64) -> Vec<u8> {
        self.keys.model_key(tenant, serial)
    }

    /// The region a tenant's policy domain governs, if the domain exists.
    ///
    /// A read accessor rather than a test reaching into the map: the map is
    /// behind a lock now, and a test that could borrow through it could hold
    /// it. The tests that used `svc.policies[&key].region` ask this instead.
    #[cfg(test)]
    fn policy_region(&self, tenant: &str, domain: &str) -> Option<String> {
        let key = self.keys.policy_key(tenant, domain);
        self.state.read(|s| s.policies.get(&key).map(|p| p.region.clone()))
    }

    /// The region a declared node is in, if it was declared.
    #[cfg(test)]
    fn node_region(&self, node: &str) -> Option<String> {
        self.state.read(|s| s.nodes.get(node).cloned())
    }

    /// `(base_model, cost, delta length)` of a tenant's expert object.
    ///
    /// The same accessor pattern as the two above, and for the same reason.
    /// It exists because the `experts` relationship's create-path rule — a
    /// capability the manifest names is *materialised hollow*, not refused —
    /// is a statement about the target's contents, and the wire has no
    /// operation that returns all three (`describe` gives the cost,
    /// `adapter_delta` the bytes, and nothing at all gives the base).
    #[cfg(test)]
    fn expert_shape(&self, tenant: &str, capability: &str) -> Option<(String, f32, usize)> {
        let key = self.keys.expert_key(tenant, capability);
        self.state
            .read(|s| s.experts.get(&key).map(|e| (e.base_model.clone(), e.cost, e.delta.len())))
    }

    // ── the wire operations, each one lock section deep ─────────────────────
    //
    // This layer exists so that **taking the lock happens in exactly one place
    // per operation**. `handle` decodes and replies; these open the section
    // and delegate to `TenantState`, which does the work with no idea that a
    // lock exists. The rule to keep is that nothing in this block calls
    // anything else in this block: nesting two would be a torn request and a
    // re-entrant lock, and `orbweaver_giop::guarded` refuses the second.

    fn create(&self, tenant: &str, m: Manifest) -> Result<Ior, SystemException> {
        let key = self.state.write(|s| s.create(&self.keys, tenant, m))?;
        Ok(self.ior_for(COMPOSED_MODEL_ID, &key))
    }

    fn clone_model(
        &self,
        tenant: &str,
        src: Option<Ior>,
        new_version: &str,
    ) -> Result<Ior, SystemException> {
        let key = self.state.write(|s| s.clone_model(&self.keys, tenant, src, new_version))?;
        Ok(self.ior_for(COMPOSED_MODEL_ID, &key))
    }

    fn retire(&self, tenant: &str, m: Option<Ior>) -> Result<(), SystemException> {
        self.state.write(|s| s.retire(&self.keys, tenant, m))
    }

    fn deploy(&self, tenant: &str, m: Option<Ior>) -> Result<(), SystemException> {
        self.state.write(|s| s.deploy(&self.keys, tenant, m))
    }

    fn bind_expert(
        &self,
        tenant: &str,
        model_key: &[u8],
        ex: Option<Ior>,
    ) -> Result<(), SystemException> {
        self.state.write(|s| s.bind_expert(&self.keys, tenant, model_key, ex))
    }

    fn set_policy(
        &self,
        tenant: &str,
        model_key: &[u8],
        p: Option<Ior>,
    ) -> Result<(), SystemException> {
        self.state.write(|s| s.set_policy(&self.keys, tenant, model_key, p))
    }

    /// A write, because it appends to the audit log — see the module docs on
    /// which operations are reads and why `audit` being one of the writers is
    /// worth noticing.
    fn infer(
        &self,
        tenant: &str,
        model_key: &[u8],
        x: Activation,
        ctx: &CallContext,
    ) -> Result<Activation, SystemException> {
        self.state.write(|s| s.infer(tenant, model_key, x, ctx))
    }

    fn base(&self, expert_key: &[u8]) -> Result<Ior, SystemException> {
        let key = self.state.write(|s| s.base(&self.keys, expert_key))?;
        Ok(self.ior_for(EXPERT_ID, &key))
    }

    /// `_is_a`, answered per object.
    ///
    /// `EnterpriseExpert : ::moe::Expert`, so its objects answer for both —
    /// inheritance is part of the contract and a narrow to the base would
    /// otherwise fail. The shared base answers for `::moe::Expert` only, which
    /// is what makes a client unable to narrow it to something with a tenant
    /// on it.
    fn is_a(kind: &Kind, want: &str) -> bool {
        want == OBJECT_ID
            || want == Self::type_id_of(kind)
            || (*kind == Kind::Expert && want == EXPERT_ID)
    }

    /// Serves one operation.
    ///
    /// Arguments are decoded before any lock is taken; each arm then opens
    /// exactly one section — `read` where the operation changes nothing,
    /// `write` where it does — and the reply is written from what the section
    /// returned. Two sections in one operation would be a torn request *and* a
    /// re-entrant lock, and [`orbweaver_giop::guarded`] refuses the second.
    fn handle(&self, req: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        let mut args = req.body().map_err(|_| SystemException::marshal())?;
        // `Server` has already refused any key `knows` rejects — in a separate
        // look at the graph, so this can still fail if the object was retired
        // in between, which is why it is a `?` and not an `expect`.
        let addressed =
            self.parse(&req.object_key).ok_or_else(SystemException::object_not_exist)?;

        match req.operation.as_str() {
            "_is_a" => {
                let want = args.get_string().map_err(|_| SystemException::marshal())?;
                out.put_bool(Self::is_a(&addressed.kind, &want));
                return Ok(());
            }
            "_non_existent" | "_not_existent" => {
                out.put_bool(false);
                return Ok(());
            }
            _ => {}
        }

        // Every branch below is one interface's operation set, and an
        // operation belonging to another interface falls through to
        // BAD_OPERATION — which is what makes these distinct objects rather
        // than one object with a union of operations.
        let tenant = addressed.tenant.clone().unwrap_or_default();
        match addressed.kind {
            Kind::Factory => match req.operation.as_str() {
                "create" => {
                    let m =
                        Manifest::read_from(&mut args).map_err(|_| SystemException::marshal())?;
                    let r = self.create(&tenant, m)?;
                    put_reference(out, Some(&r)).map_err(|_| SystemException::marshal())
                }
                "clone_model" => {
                    let src = get_reference(&mut args).map_err(|_| SystemException::marshal())?;
                    let new_version = args.get_string().map_err(|_| SystemException::marshal())?;
                    let r = self.clone_model(&tenant, src, &new_version)?;
                    put_reference(out, Some(&r)).map_err(|_| SystemException::marshal())
                }
                "retire" => {
                    let m = get_reference(&mut args).map_err(|_| SystemException::marshal())?;
                    self.retire(&tenant, m)
                }
                "deploy" => {
                    let m = get_reference(&mut args).map_err(|_| SystemException::marshal())?;
                    self.deploy(&tenant, m)
                }
                _ => Err(SystemException::bad_operation()),
            },
            Kind::Model => match req.operation.as_str() {
                "get_manifest" => self.state.read(|s| {
                    let model = s
                        .models
                        .get(&req.object_key)
                        .ok_or_else(SystemException::object_not_exist)?;
                    model.manifest.write_to(out);
                    Ok(())
                }),
                "infer" => {
                    let x =
                        Activation::read_from(&mut args).map_err(|_| SystemException::marshal())?;
                    let ctx = CallContext::read_from(&mut args)
                        .map_err(|_| SystemException::marshal())?;
                    let y = self.infer(&tenant, &req.object_key, x, &ctx)?;
                    y.write_to(out);
                    Ok(())
                }
                "bind_expert" => {
                    let ex = get_reference(&mut args).map_err(|_| SystemException::marshal())?;
                    self.bind_expert(&tenant, &req.object_key, ex)
                }
                "set_policy" => {
                    let p = get_reference(&mut args).map_err(|_| SystemException::marshal())?;
                    self.set_policy(&tenant, &req.object_key, p)
                }
                _ => Err(SystemException::bad_operation()),
            },
            Kind::Policy => match req.operation.as_str() {
                "authorize" => {
                    let principal = args.get_string().map_err(|_| SystemException::marshal())?;
                    let target = args.get_string().map_err(|_| SystemException::marshal())?;
                    let granted = self.state.read(|s| {
                        // Default-deny: an ungranted pair is `false`, never an
                        // error — `authorize` is a question, and refusing to
                        // answer it is not the same as answering no.
                        Ok(s.policy_at(&req.object_key)?.grants.contains(&(principal, target)))
                    })?;
                    out.put_bool(granted);
                    Ok(())
                }
                "check_residency" => {
                    let node = args.get_string().map_err(|_| SystemException::marshal())?;
                    let in_region = self.state.read(|s| {
                        let region = &s.policy_at(&req.object_key)?.region;
                        // Default-deny: a node nobody declared cannot be shown
                        // to be in region, and "unknown" answering true is how
                        // a residency guarantee quietly becomes decoration.
                        Ok(s.nodes.get(&node) == Some(region))
                    })?;
                    out.put_bool(in_region);
                    Ok(())
                }
                "audit" => {
                    let ctx = CallContext::read_from(&mut args)
                        .map_err(|_| SystemException::marshal())?;
                    let event = args.get_string().map_err(|_| SystemException::marshal())?;
                    self.state.write(|s| {
                        let name = s.policy_at(&req.object_key)?.name.clone();
                        s.record(&tenant, &name, &ctx, &event);
                        Ok(())
                    })
                }
                _ => Err(SystemException::bad_operation()),
            },
            Kind::Expert => match req.operation.as_str() {
                "get_tenant_id" => {
                    let owner =
                        self.state.read(|s| Ok(s.expert_at(&req.object_key)?.tenant.clone()))?;
                    out.put_str(&owner);
                    Ok(())
                }
                "base" => {
                    let r = self.base(&req.object_key)?;
                    put_reference(out, Some(&r)).map_err(|_| SystemException::marshal())
                }
                "adapter_delta" => self.state.read(|s| {
                    out.put_octet_seq(&s.expert_at(&req.object_key)?.delta);
                    Ok(())
                }),
                // Inherited from `::moe::Expert`. Served, because inheritance
                // is part of the contract and a half-served interface is worse
                // than an unserved one.
                "describe" => self.state.read(|s| {
                    let expert = s.expert_at(&req.object_key)?;
                    Capability { id: expert.capability.clone(), cost: expert.cost }.write_to(out);
                    Ok(())
                }),
                "process" => {
                    let x =
                        Activation::read_from(&mut args).map_err(|_| SystemException::marshal())?;
                    let ctx = CallContext::read_from(&mut args)
                        .map_err(|_| SystemException::marshal())?;
                    self.state.write(|s| {
                        let capability = s.expert_at(&req.object_key)?.capability.clone();
                        s.record(&tenant, "", &ctx, &format!("process {capability}"));
                        Ok::<(), SystemException>(())
                    })?;
                    // Unchanged: PLAN-MOE §5, as for `infer`.
                    x.write_to(out);
                    Ok(())
                }
                _ => Err(SystemException::bad_operation()),
            },
            Kind::Base => match req.operation.as_str() {
                "describe" => {
                    let name = self
                        .keys
                        .base_name(&req.object_key)
                        .ok_or_else(SystemException::object_not_exist)?;
                    // cost 0.0 and not a guess — see the module docs.
                    Capability { id: name, cost: 0.0 }.write_to(out);
                    Ok(())
                }
                "process" => {
                    let x =
                        Activation::read_from(&mut args).map_err(|_| SystemException::marshal())?;
                    let _ctx = CallContext::read_from(&mut args)
                        .map_err(|_| SystemException::marshal())?;
                    // No audit line: the shared base belongs to no tenant, so
                    // there is no log this entry could honestly go in. The
                    // crossing is recorded where it happens — in `base()`, on
                    // the tenant's side of the boundary.
                    x.write_to(out);
                    Ok(())
                }
                _ => Err(SystemException::bad_operation()),
            },
        }
    }
}

impl SharedDispatch for TenantService {
    /// Many object keys, one servant — see the type docs. A retired model's
    /// key is not here any more, which is what turns `retire` into a real
    /// `OBJECT_NOT_EXIST` for both `Request` and `LocateRequest`.
    fn knows(&self, object_key: &[u8]) -> bool {
        self.state.read(|s| {
            s.factories.contains(object_key)
                || s.models.contains_key(object_key)
                || s.experts.contains_key(object_key)
                || s.policies.contains_key(object_key)
                || s.bases.contains(object_key)
        })
    }

    fn dispatch(&self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        self.handle(request, out)
    }
}

/// The `&mut self` shape too, forwarding, so a caller already written against
/// [`Server::serve`](orbweaver_giop::server::Server::serve) keeps working.
impl Dispatch for TenantService {
    fn knows(&self, object_key: &[u8]) -> bool {
        SharedDispatch::knows(self, object_key)
    }

    fn dispatch(&mut self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        SharedDispatch::dispatch(self, request, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_cdr::Endian;

    use orbweaver_giop::orb::Orb;
    use orbweaver_giop::{Connection, Error, Reply};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    const T: Duration = Duration::from_secs(5);

    // ── the wire layout ─────────────────────────────────────────────────────

    /// corpus/golden/23 lines 18–25, decoded with the primitive getters rather
    /// than with `read_from`, so a pair of members swapped in *both*
    /// directions still fails here. Both byte orders, and both an empty and a
    /// multi-element `sequence<::moe::CapabilityId>` — an empty sequence is a
    /// bare zero count with no elements after it, and getting that wrong
    /// desynchronises every member that follows.
    #[test]
    fn manifest_members_are_in_the_idls_declaration_order() {
        for experts in [vec![], vec!["math".to_owned(), "vision".to_owned(), "code".to_owned()]] {
            let m = Manifest {
                tenant_id: "acme".into(),
                base_model: "llama-70b".into(),
                experts: experts.clone(),
                policy_domain: "acme-default".into(),
                version: "3.1".into(),
                residency_region: "eu-west".into(),
            };
            for endian in [Endian::Big, Endian::Little] {
                let mut e = Encoder::new(endian);
                m.write_to(&mut e);
                let bytes = e.finish().unwrap();
                let what = format!("{endian:?}, {} experts", experts.len());

                let mut d = Decoder::new(&bytes, endian);
                assert_eq!(d.get_string().unwrap(), "acme", "1 tenant_id {what}");
                assert_eq!(d.get_string().unwrap(), "llama-70b", "2 base_model {what}");
                let count = d.get_u32().unwrap();
                assert_eq!(count as usize, experts.len(), "3 experts count {what}");
                for (i, want) in experts.iter().enumerate() {
                    assert_eq!(&d.get_string().unwrap(), want, "3 experts[{i}] {what}");
                }
                assert_eq!(d.get_string().unwrap(), "acme-default", "4 policy_domain {what}");
                assert_eq!(d.get_string().unwrap(), "3.1", "5 version {what}");
                assert_eq!(d.get_string().unwrap(), "eu-west", "6 residency_region {what}");
                assert_eq!(d.remaining(), 0, "nothing after the last member {what}");

                let mut d = Decoder::new(&bytes, endian);
                assert_eq!(Manifest::read_from(&mut d).unwrap(), m, "round trip {what}");
            }
        }
    }

    /// The three remaining structs, in declaration order, both byte orders.
    /// `Activation::data` is a `sequence<octet>` and gets an empty case for
    /// the same reason the manifest's sequence does.
    #[test]
    fn the_other_structs_are_in_the_idls_declaration_order() {
        for endian in [Endian::Big, Endian::Little] {
            for data in [vec![], vec![1u8, 2, 3, 4, 5]] {
                let a =
                    Activation { data: data.clone(), dtype: "f16".into(), shape: "1x4096".into() };
                let mut e = Encoder::new(endian);
                a.write_to(&mut e);
                let bytes = e.finish().unwrap();
                let mut d = Decoder::new(&bytes, endian);
                assert_eq!(d.get_octet_seq().unwrap(), &data[..], "1 data {endian:?}");
                assert_eq!(d.get_string().unwrap(), "f16", "2 dtype {endian:?}");
                assert_eq!(d.get_string().unwrap(), "1x4096", "3 shape {endian:?}");
                let mut d = Decoder::new(&bytes, endian);
                assert_eq!(Activation::read_from(&mut d).unwrap(), a);
            }

            let c = CallContext { request_id: "r-1".into(), trace_id: "t-9".into(), step: 7 };
            let mut e = Encoder::new(endian);
            c.write_to(&mut e);
            let bytes = e.finish().unwrap();
            let mut d = Decoder::new(&bytes, endian);
            assert_eq!(d.get_string().unwrap(), "r-1", "1 request_id {endian:?}");
            assert_eq!(d.get_string().unwrap(), "t-9", "2 trace_id {endian:?}");
            assert_eq!(d.get_u32().unwrap(), 7, "3 step {endian:?}");
            let mut d = Decoder::new(&bytes, endian);
            assert_eq!(CallContext::read_from(&mut d).unwrap(), c);

            // 23's Capability is two members, not 22's nine.
            let cap = Capability { id: "math".into(), cost: 0.5 };
            let mut e = Encoder::new(endian);
            cap.write_to(&mut e);
            let bytes = e.finish().unwrap();
            let mut d = Decoder::new(&bytes, endian);
            assert_eq!(d.get_string().unwrap(), "math", "1 id {endian:?}");
            assert_eq!(d.get_f32().unwrap(), 0.5, "2 cost {endian:?}");
            assert_eq!(d.remaining(), 0, "…and nothing else: 23 declares two members");
            let mut d = Decoder::new(&bytes, endian);
            assert_eq!(Capability::read_from(&mut d).unwrap(), cap);
        }
    }

    /// A four-byte count is attacker-controlled, and a manifest claiming four
    /// billion capability ids must be refused before anything is allocated.
    #[test]
    fn an_implausible_sequence_count_is_refused_rather_than_allocated() {
        let mut e = Encoder::new(Endian::Little);
        e.put_str("acme");
        e.put_str("llama");
        e.put_u32(u32::MAX);
        let bytes = e.finish().unwrap();
        let mut d = Decoder::new(&bytes, Endian::Little);
        assert!(Manifest::read_from(&mut d).is_err());
    }

    // ── a service, locally ──────────────────────────────────────────────────

    fn manifest(tenant: &str, version: &str, region: &str) -> Manifest {
        Manifest {
            tenant_id: tenant.into(),
            base_model: "llama-70b".into(),
            experts: Vec::new(),
            policy_domain: format!("{tenant}-default"),
            version: version.into(),
            residency_region: region.into(),
        }
    }

    /// Two tenants, each with a factory, an expert and a model.
    fn two_tenants() -> (TenantService, Ior, Ior, Vec<u8>, Vec<u8>) {
        let svc = TenantService::new("127.0.0.1", 4002, "MoE");
        let a = svc.provision_factory("acme").unwrap();
        let b = svc.provision_factory("globex").unwrap();
        svc.provision_expert("acme", "math", "llama-70b", 1.5, b"acme-delta").unwrap();
        svc.provision_expert("globex", "math", "llama-70b", 2.5, b"globex-delta").unwrap();
        let ma = svc.create("acme", manifest("acme", "1.0", "eu-west")).unwrap();
        let mb = svc.create("globex", manifest("globex", "1.0", "us-east")).unwrap();
        let (ka, kb) =
            (ma.primary().unwrap().object_key.clone(), mb.primary().unwrap().object_key.clone());
        (svc, a, b, ka, kb)
    }

    fn key_of(ior: &Ior) -> Vec<u8> {
        ior.primary().unwrap().object_key.clone()
    }

    /// Isolation is the property this module exists for, so it is the property
    /// concurrency has to be shown not to have cost.
    ///
    /// Two tenants' clients hammer the servant at once — reads on both sides,
    /// creates on both sides, and a stream of cross-tenant attempts that must
    /// all be refused. Afterwards: every model belongs to the tenant that
    /// created it, no audit line has crossed, and the serial counter handed
    /// out no number twice. The last one is the interesting check under
    /// concurrency: a global counter read and incremented under one lock
    /// cannot collide, and a per-tenant one would have had to prove itself
    /// again.
    #[test]
    fn concurrent_tenants_neither_collide_nor_leak() {
        const N: usize = 4;
        const EACH: usize = 8;
        let (svc, _, _, model_a, model_b) = two_tenants();

        std::thread::scope(|scope| {
            for i in 0..N {
                let svc = &svc;
                let (model_a, model_b) = (&model_a, &model_b);
                scope.spawn(move || {
                    let (mine, theirs, other_model) = if i % 2 == 0 {
                        ("acme", "globex", model_b)
                    } else {
                        ("globex", "acme", model_a)
                    };
                    for step in 0..EACH {
                        // A create of my own, with a version nobody else uses.
                        let version = format!("{mine}-{i}-{step}");
                        let region = if mine == "acme" { "eu-west" } else { "us-east" };
                        let made = svc
                            .create(
                                mine,
                                Manifest {
                                    tenant_id: mine.to_owned(),
                                    base_model: "llama-70b".into(),
                                    experts: vec![],
                                    policy_domain: format!("{mine}-default"),
                                    version: version.clone(),
                                    residency_region: region.into(),
                                },
                            )
                            .expect("a tenant may always create its own model");
                        // A read of my own, which must be mine.
                        let got = svc.manifest_at(&key_of(&made)).expect("just created");
                        assert_eq!(got.tenant_id, mine);
                        assert_eq!(got.version, version);
                        // And a crossing, which must always be refused.
                        let stolen = svc.ior_for(COMPOSED_MODEL_ID, other_model);
                        assert_eq!(
                            svc.retire(mine, Some(stolen)).unwrap_err().id,
                            NO_PERMISSION,
                            "{mine} reached into {theirs} on step {step}"
                        );
                    }
                });
            }
        });

        // Nothing crossed: every audit line in a tenant's log is that
        // tenant's, and the two originals are untouched.
        for (tenant, other) in [("acme", "globex"), ("globex", "acme")] {
            assert!(
                !svc.audit_log(tenant).iter().any(|e| e.event.contains(other)),
                "{tenant}'s log mentions {other}"
            );
        }
        assert!(svc.manifest_at(&model_a).is_some(), "acme's model survived every attempt");
        assert!(svc.manifest_at(&model_b).is_some(), "globex's model survived every attempt");

        // No serial was handed out twice: every create returned a distinct
        // key, so the count of models is exactly what was created.
        let created = N * EACH + 2;
        assert_eq!(
            svc.served(),
            created + 2 /* factories */ + 2 /* experts */ + 2 /* policy domains */ + 1, /* shared base */
            "a serial collision or a lost create"
        );
    }

    /// Every key names exactly one tenant, and parses back to it. The shared
    /// base is the single deliberate exception.
    #[test]
    fn every_minted_key_names_its_tenant_and_the_shared_base_names_none() {
        let (svc, factory_a, _, model_a, _) = two_tenants();
        let expert_a = svc.expert_reference("acme", "math").unwrap();
        let policy_a = svc.policy_reference("acme", "acme-default").unwrap();
        for (key, kind) in [
            (key_of(&factory_a), Kind::Factory),
            (model_a, Kind::Model),
            (key_of(&expert_a), Kind::Expert),
            (key_of(&policy_a), Kind::Policy),
        ] {
            let a = svc.parse(&key).expect("a key we minted parses");
            assert_eq!(a.tenant.as_deref(), Some("acme"), "{:?}", String::from_utf8_lossy(&key));
            assert_eq!(a.kind, kind);
            assert!(SharedDispatch::knows(&svc, &key));
        }
        let shared = svc.shared_base_reference("llama-70b").unwrap();
        let parsed = svc.parse(&key_of(&shared)).unwrap();
        assert_eq!(parsed.tenant, None, "the shared base belongs to no tenant");
        assert_eq!(parsed.kind, Kind::Base);
    }

    /// A key is a credential, so the strings it is built from may not contain
    /// the separator. Without this a tenant called `globex/model/1` could name
    /// another tenant's object.
    #[test]
    fn a_tenant_cannot_forge_a_key_out_of_manifest_strings() {
        let svc = TenantService::new("h", 1, "MoE");
        assert!(svc.provision_factory("globex/model/1").is_none(), "a slashed tenant id");
        assert!(svc.provision_factory("").is_none(), "an empty tenant id");
        svc.provision_factory("acme").unwrap();

        for bad in ["acme/x", ""] {
            let mut m = manifest("acme", "1.0", "eu-west");
            m.base_model = bad.into();
            assert_eq!(svc.create("acme", m).unwrap_err().id, BAD_PARAM, "base_model {bad:?}");
            let mut m = manifest("acme", "1.0", "eu-west");
            m.version = bad.into();
            assert_eq!(svc.create("acme", m).unwrap_err().id, BAD_PARAM, "version {bad:?}");
            let mut m = manifest("acme", "1.0", "eu-west");
            m.policy_domain = bad.into();
            assert_eq!(svc.create("acme", m).unwrap_err().id, BAD_PARAM, "policy_domain {bad:?}");
            let mut m = manifest("acme", "1.0", "eu-west");
            m.experts = vec![bad.into()];
            assert_eq!(svc.create("acme", m).unwrap_err().id, BAD_PARAM, "experts {bad:?}");
        }
        assert_eq!(svc.served(), 1, "not one refusal left anything behind");
    }

    /// The isolation property, in the shape the task names: an operation whose
    /// tenant context is B may not reach a reference minted for A. Every
    /// reference-taking operation is covered, because one uncovered operation
    /// is the whole hole.
    #[test]
    fn no_reference_argument_crosses_a_tenant() {
        let (svc, _, _, model_a, model_b) = two_tenants();
        let expert_a = svc.expert_reference("acme", "math").unwrap();
        let policy_a = svc.policy_reference("acme", "acme-default").unwrap();
        let model_a_ior = svc.ior_for(COMPOSED_MODEL_ID, &model_a);

        // globex's model, handed acme's expert and acme's policy domain.
        assert_eq!(
            svc.bind_expert("globex", &model_b, Some(expert_a)).unwrap_err().id,
            NO_PERMISSION,
            "bind_expert across tenants"
        );
        assert_eq!(
            svc.set_policy("globex", &model_b, Some(policy_a)).unwrap_err().id,
            NO_PERMISSION,
            "set_policy across tenants"
        );
        // globex's factory, handed acme's model.
        assert_eq!(
            svc.retire("globex", Some(model_a_ior.clone())).unwrap_err().id,
            NO_PERMISSION,
            "retire across tenants"
        );
        assert_eq!(
            svc.deploy("globex", Some(model_a_ior.clone())).unwrap_err().id,
            NO_PERMISSION,
            "deploy across tenants"
        );
        assert_eq!(
            svc.clone_model("globex", Some(model_a_ior), "2.0").unwrap_err().id,
            NO_PERMISSION,
            "clone_model across tenants"
        );
        // …and a manifest naming somebody else.
        assert_eq!(
            svc.create("globex", manifest("acme", "9.9", "eu-west")).unwrap_err().id,
            NO_PERMISSION,
            "create with another tenant's manifest"
        );
        // Nothing moved: acme's model is intact and globex gained nothing.
        assert_eq!(svc.manifest_at(&model_a), Some(manifest("acme", "1.0", "eu-west")));
        assert!(svc.manifest_at(&model_b).unwrap().experts.is_empty());
    }

    /// A cross-tenant refusal must not double as an existence oracle: the
    /// answer for a model that exists and one that never did has to be the
    /// same exception, or `NO_PERMISSION` vs `OBJECT_NOT_EXIST` enumerates the
    /// neighbour's objects one probe at a time.
    #[test]
    fn a_cross_tenant_refusal_does_not_disclose_existence() {
        let (svc, _, _, model_a, _) = two_tenants();
        let real = svc.ior_for(COMPOSED_MODEL_ID, &model_a);
        let never = svc.ior_for(COMPOSED_MODEL_ID, &svc.model_key("acme", 9_999));
        assert_eq!(svc.retire("globex", Some(real)).unwrap_err().id, NO_PERMISSION, "exists");
        assert_eq!(
            svc.retire("globex", Some(never)).unwrap_err().id,
            NO_PERMISSION,
            "never existed — and answers identically"
        );
    }

    /// `retire` destroys: the key leaves the served set, so the `Server`
    /// answers `OBJECT_NOT_EXIST`; a second retire says the object is gone
    /// rather than that the argument was bad; and the serial is never reused,
    /// so a stale reference cannot land on the replacement.
    #[test]
    fn retire_removes_the_object_and_its_serial_is_never_reused() {
        let (svc, _, _, model_a, model_b) = two_tenants();
        let ior_a = svc.ior_for(COMPOSED_MODEL_ID, &model_a);
        assert!(SharedDispatch::knows(&svc, &model_a));
        svc.retire("acme", Some(ior_a.clone())).unwrap();
        assert!(
            !SharedDispatch::knows(&svc, &model_a),
            "the key is gone, so the Server says OBJECT_NOT_EXIST"
        );
        assert_eq!(svc.manifest_at(&model_a), None);
        assert_eq!(
            svc.retire("acme", Some(ior_a)).unwrap_err().id,
            OBJECT_NOT_EXIST,
            "retiring twice says gone, not bad argument"
        );
        // The other tenant is untouched.
        assert!(SharedDispatch::knows(&svc, &model_b));
        // A replacement gets a fresh serial, so the retired reference stays dead.
        let replacement = svc.create("acme", manifest("acme", "1.0", "eu-west")).unwrap();
        assert_ne!(key_of(&replacement), model_a, "a retired serial is never reused");
        assert!(!SharedDispatch::knows(&svc, &model_a));
        // The tenant's expert and policy domain survive: they are the tenant's,
        // not the model's, and no operation in the contract destroys them.
        assert!(svc.expert_reference("acme", "math").is_some());
        assert!(svc.policy_reference("acme", "acme-default").is_some());
    }

    /// `base()` is the crossing the manifest draws. It is served, counted,
    /// audited — and bounded: the reference it returns names no tenant, so
    /// every tenancy-checked operation refuses it, and it answers `_is_a` for
    /// `::moe::Expert` and nothing narrower.
    #[test]
    fn the_shared_base_crosses_the_boundary_visibly_and_reaches_nothing() {
        let (svc, _, _, model_a, _) = two_tenants();
        let ka = key_of(&svc.expert_reference("acme", "math").unwrap());
        let kb = key_of(&svc.expert_reference("globex", "math").unwrap());

        let from_a = svc.base(&ka).unwrap();
        let from_b = svc.base(&kb).unwrap();
        assert_eq!(from_a, from_b, "the same base is the same object for both tenants");
        assert_eq!(from_a.type_id, EXPERT_ID, "::moe::Expert, not EnterpriseExpert");
        assert_eq!(svc.parse(&key_of(&from_a)).unwrap().tenant, None);

        // Counted and audited, per tenant.
        assert_eq!(svc.base_crossings("acme"), 1);
        assert_eq!(svc.base_crossings("globex"), 1);
        assert!(svc.audit_log("acme").iter().any(|e| e.event.contains("base crossing")));

        // Inert as an argument: the shared base cannot be composed into a model.
        assert_eq!(
            svc.bind_expert("acme", &model_a, Some(from_a.clone())).unwrap_err().id,
            BAD_PARAM,
            "bind_expert(base()) is refused — the base belongs to no tenant"
        );
        assert_eq!(
            svc.set_policy("acme", &model_a, Some(from_a)).unwrap_err().id,
            BAD_PARAM,
            "…and it is not a policy domain either"
        );

        // It narrows to ::moe::Expert only.
        assert!(TenantService::is_a(&Kind::Base, EXPERT_ID));
        assert!(TenantService::is_a(&Kind::Base, OBJECT_ID));
        assert!(!TenantService::is_a(&Kind::Base, ENTERPRISE_EXPERT_ID));
        // …while a tenant expert answers for both, because it inherits.
        assert!(TenantService::is_a(&Kind::Expert, EXPERT_ID));
        assert!(TenantService::is_a(&Kind::Expert, ENTERPRISE_EXPERT_ID));
        assert!(!TenantService::is_a(&Kind::Expert, COMPOSED_MODEL_ID));
    }

    /// The residency refusal the manifest exists to make possible, including
    /// the default-deny answer for a node nobody declared.
    #[test]
    fn check_residency_refuses_a_node_outside_the_manifests_region() {
        let (svc, _, _, _, _) = two_tenants();
        svc.declare_node("gpu-eu-1", "eu-west");
        svc.declare_node("gpu-us-1", "us-east");
        let region_of = |svc: &TenantService, tenant: &str| {
            svc.policy_region(tenant, &format!("{tenant}-default")).expect("the domain exists")
        };
        assert_eq!(region_of(&svc, "acme"), "eu-west");
        assert_eq!(region_of(&svc, "globex"), "us-east");
        let allows = |svc: &TenantService, tenant: &str, node: &str| {
            svc.node_region(node).is_some()
                && svc.node_region(node) == svc.policy_region(tenant, &format!("{tenant}-default"))
        };
        assert!(allows(&svc, "acme", "gpu-eu-1"), "in region");
        assert!(!allows(&svc, "acme", "gpu-us-1"), "another region is refused");
        assert!(!allows(&svc, "acme", "gpu-undeclared"), "an undeclared node is refused");
    }

    /// A policy domain governs one region. A second manifest naming the same
    /// domain with another region is a contract error, and `set_policy` may
    /// not move a model into a domain whose region its manifest does not claim.
    #[test]
    fn a_policy_domain_and_a_manifest_may_not_disagree_about_the_region() {
        let (svc, _, _, model_a, _) = two_tenants();
        let mut second = manifest("acme", "2.0", "us-east");
        second.policy_domain = "acme-default".into();
        assert_eq!(svc.create("acme", second).unwrap_err().id, BAD_PARAM, "one domain, one region");

        // A second domain, in a second region, is fine — until a model whose
        // manifest says eu-west tries to adopt it.
        let mut other_region = manifest("acme", "2.0", "us-east");
        other_region.policy_domain = "acme-us".into();
        svc.create("acme", other_region).unwrap();
        let us = svc.policy_reference("acme", "acme-us").unwrap();
        assert_eq!(svc.set_policy("acme", &model_a, Some(us)).unwrap_err().id, BAD_PARAM);
    }

    /// `bind_expert` composes an adapter onto a base. An adapter trained
    /// against another base is meaningless here, and binding the same
    /// capability twice would make the manifest's sequence a multiset.
    #[test]
    fn bind_expert_refuses_a_foreign_base_and_a_repeat() {
        let (svc, _, _, model_a, _) = two_tenants();
        svc.provision_expert("acme", "vision", "mistral-8x7b", 1.0, b"").unwrap();
        let other_base = svc.expert_reference("acme", "vision").unwrap();
        assert_eq!(
            svc.bind_expert("acme", &model_a, Some(other_base)).unwrap_err().id,
            BAD_PARAM,
            "an adapter over a different base"
        );
        let math = svc.expert_reference("acme", "math").unwrap();
        svc.bind_expert("acme", &model_a, Some(math.clone())).unwrap();
        assert_eq!(svc.manifest_at(&model_a).unwrap().experts, vec!["math".to_owned()]);
        assert_eq!(
            svc.bind_expert("acme", &model_a, Some(math)).unwrap_err().id,
            BAD_PARAM,
            "binding the same capability twice"
        );
    }

    /// Each tenant's audit trail contains its own calls and nothing else —
    /// the property the per-tenant log exists to make checkable.
    #[test]
    fn an_audit_trail_holds_one_tenants_calls_and_no_others() {
        let (svc, _, _, model_a, model_b) = two_tenants();
        let ior_a = svc.ior_for(COMPOSED_MODEL_ID, &model_a);
        let ior_b = svc.ior_for(COMPOSED_MODEL_ID, &model_b);
        svc.deploy("acme", Some(ior_a)).unwrap();
        svc.deploy("globex", Some(ior_b)).unwrap();
        let ctx_a = CallContext { request_id: "acme-req".into(), trace_id: "ta".into(), step: 1 };
        let ctx_b = CallContext { request_id: "globex-req".into(), trace_id: "tb".into(), step: 1 };
        svc.infer("acme", &model_a, Activation::default(), &ctx_a).unwrap();
        svc.infer("globex", &model_b, Activation::default(), &ctx_b).unwrap();

        assert!(svc.audit_log("acme").iter().any(|e| e.request_id == "acme-req"));
        assert!(
            !svc.audit_log("acme").iter().any(|e| e.request_id == "globex-req"),
            "another tenant's call is not in this tenant's log"
        );
        assert!(!svc.audit_log("globex").iter().any(|e| e.request_id == "acme-req"));
        assert!(svc.audit_log("nobody").is_empty(), "a tenant with no calls has no log");
    }

    /// `infer` before `deploy` is a missing edge, not a silent success, and a
    /// second `deploy` says so too.
    #[test]
    fn infer_refuses_until_the_model_is_deployed() {
        let (svc, _, _, model_a, _) = two_tenants();
        let ior = svc.ior_for(COMPOSED_MODEL_ID, &model_a);
        let ctx = CallContext::default();
        assert_eq!(
            svc.infer("acme", &model_a, Activation::default(), &ctx).unwrap_err().id,
            BAD_INV_ORDER
        );
        svc.deploy("acme", Some(ior.clone())).unwrap();
        svc.infer("acme", &model_a, Activation::default(), &ctx).expect("deployed");
        assert_eq!(svc.deploy("acme", Some(ior)).unwrap_err().id, BAD_INV_ORDER, "deployed twice");
    }

    /// Two models may not share a version within a tenant, and `clone_model`
    /// goes through the same gate as `create` — it produces a fresh,
    /// undeployed object with the source's manifest and a new version.
    #[test]
    fn clone_model_mints_a_fresh_undeployed_object() {
        let (svc, _, _, model_a, _) = two_tenants();
        let ior = svc.ior_for(COMPOSED_MODEL_ID, &model_a);
        let math = svc.expert_reference("acme", "math").unwrap();
        svc.bind_expert("acme", &model_a, Some(math)).unwrap();
        svc.deploy("acme", Some(ior.clone())).unwrap();

        assert_eq!(
            svc.clone_model("acme", Some(ior.clone()), "1.0").unwrap_err().id,
            BAD_PARAM,
            "a duplicate version"
        );
        let clone = svc.clone_model("acme", Some(ior), "1.1").unwrap();
        let ck = key_of(&clone);
        assert_ne!(ck, model_a);
        let cloned = svc.manifest_at(&ck).unwrap();
        assert_eq!(cloned.version, "1.1");
        assert_eq!(cloned.experts, vec!["math".to_owned()], "the composition came along");
        assert_eq!(cloned.tenant_id, "acme");
        // Undeployed: deployment is a decision about a particular object.
        assert_eq!(
            svc.infer("acme", &ck, Activation::default(), &CallContext::default()).unwrap_err().id,
            BAD_INV_ORDER
        );
    }

    // ── the three relationships (D023 R1) ───────────────────────────────────
    //
    // One test per rule written down in the module docs' *three relationships*
    // section, each shaped so that the rule being different makes it red. Where
    // a rule cannot be observed through any surface the tests reach, it is said
    // so in the doc comment rather than covered by something adjacent.

    /// `base_model`: N:1, **minted by `create`** so it can never dangle, and
    /// **re-pointed by nothing**. The second half is the one no document had:
    /// the two operations that do change a manifest leave the base alone, so a
    /// model's base is fixed for the life of the object.
    ///
    /// The integrity rule on the other side — `bind_expert` refuses an adapter
    /// over a different base — is already covered by
    /// `bind_expert_refuses_a_foreign_base_and_a_repeat` and not repeated here.
    #[test]
    fn the_base_model_relationship_is_minted_by_create_and_repointed_by_nothing() {
        let svc = TenantService::new("127.0.0.1", 4002, "MoE");
        svc.provision_factory("acme").unwrap();
        assert!(svc.shared_base_reference("llama-70b").is_none(), "nobody has named it yet");

        let model = key_of(&svc.create("acme", manifest("acme", "1.0", "eu-west")).unwrap());
        let base = svc
            .shared_base_reference("llama-70b")
            .expect("create mints the base its manifest names");

        svc.provision_expert("acme", "math", "llama-70b", 1.5, b"d").unwrap();
        let math = svc.expert_reference("acme", "math").unwrap();
        svc.bind_expert("acme", &model, Some(math)).unwrap();
        let domain = svc.policy_reference("acme", "acme-default").unwrap();
        svc.set_policy("acme", &model, Some(domain)).unwrap();

        assert_eq!(
            svc.manifest_at(&model).unwrap().base_model,
            "llama-70b",
            "neither mutator of the manifest re-points the base"
        );
        assert_eq!(key_of(&svc.shared_base_reference("llama-70b").unwrap()), key_of(&base));
    }

    /// `experts` has **two link-creating paths with two different rules**, and
    /// they disagree about a target that does not exist. `create` materialises
    /// the capability into a hollow adapter — the model's own base, no cost, no
    /// delta — because the bytes arrive out of band; `bind_expert` refuses one
    /// that is not served. Either rule alone is defensible and neither was
    /// written down.
    #[test]
    fn create_materialises_an_expert_the_manifest_names_and_bind_expert_refuses_an_absent_one() {
        let svc = TenantService::new("127.0.0.1", 4002, "MoE");
        svc.provision_factory("acme").unwrap();
        let mut m = manifest("acme", "1.0", "eu-west");
        m.experts = vec!["vision".into()];
        let model = key_of(&svc.create("acme", m).unwrap());

        assert!(svc.expert_reference("acme", "vision").is_some(), "materialised, not refused");
        assert_eq!(
            svc.expert_shape("acme", "vision"),
            Some(("llama-70b".to_owned(), 0.0, 0)),
            "and hollow: the model's base, no cost, no delta"
        );
        assert_eq!(svc.manifest_at(&model).unwrap().experts, vec!["vision".to_owned()]);

        let absent = svc.ior_for(ENTERPRISE_EXPERT_ID, &svc.keys.expert_key("acme", "audio"));
        assert_eq!(
            svc.bind_expert("acme", &model, Some(absent)).unwrap_err().id,
            OBJECT_NOT_EXIST,
            "the other path requires its target to exist already"
        );
        assert_eq!(
            svc.manifest_at(&model).unwrap().experts,
            vec!["vision".to_owned()],
            "and a refused bind does not grow the sequence"
        );
    }

    /// **A finding, pinned as measured and not endorsed.**
    ///
    /// `bind_expert` enforces two integrity rules on the `experts` relationship
    /// that `create` does not: no repeat, and no adapter over a foreign base.
    /// D023 §6 forbids repairing that inside a naming batch, so this records
    /// what the two paths do today, 2026-08-25. **Making them agree turns this
    /// test red, which is the reason to write it rather than an obstacle to
    /// doing so** — a rule enforced on one of two paths is exactly the shape
    /// that stays invisible until somebody measures both.
    #[test]
    fn the_create_path_does_not_apply_bind_experts_two_integrity_rules() {
        let svc = TenantService::new("127.0.0.1", 4002, "MoE");
        svc.provision_factory("acme").unwrap();

        // (a) A repeat. `bind_expert` answers BAD_PARAM to a second bind of one
        // capability; `create` accepts the multiset and the manifest keeps it.
        let mut twice = manifest("acme", "1.0", "eu-west");
        twice.experts = vec!["math".into(), "math".into()];
        let model = key_of(&svc.create("acme", twice).unwrap());
        assert_eq!(
            svc.manifest_at(&model).unwrap().experts,
            vec!["math".to_owned(), "math".to_owned()],
            "measured: create does not deduplicate what bind_expert refuses"
        );

        // (b) A foreign base. The adapter exists over `mistral-8x7b` and the
        // manifest names it beside a `llama-70b` base — the exact pairing
        // `bind_expert_refuses_a_foreign_base_and_a_repeat` refuses. Here
        // `or_insert_with` keeps the existing object, so the model is composed
        // from an adapter its own base does not match.
        svc.provision_expert("acme", "vision", "mistral-8x7b", 1.0, b"v").unwrap();
        let mut foreign = manifest("acme", "2.0", "eu-west");
        foreign.experts = vec!["vision".into()];
        let other = key_of(&svc.create("acme", foreign).unwrap());
        assert_eq!(svc.manifest_at(&other).unwrap().base_model, "llama-70b");
        assert_eq!(
            svc.expert_shape("acme", "vision").map(|(base, _, _)| base),
            Some("mistral-8x7b".to_owned()),
            "measured: create composed an adapter trained over another base"
        );
    }

    /// `policy_domain`: exactly one, **minted by `create`**, **replaced by
    /// `set_policy`**, and the domain left behind is not destroyed — nothing in
    /// this contract destroys a relationship target. The asymmetry with
    /// `bind_expert` is the point: one appends, one replaces, which is why they
    /// are two operations with two scopes.
    #[test]
    fn set_policy_replaces_a_domain_create_minted_and_destroys_nothing() {
        let svc = TenantService::new("127.0.0.1", 4002, "MoE");
        svc.provision_factory("acme").unwrap();
        assert!(svc.policy_reference("acme", "acme-default").is_none());

        let model = key_of(&svc.create("acme", manifest("acme", "1.0", "eu-west")).unwrap());
        let first = svc.policy_reference("acme", "acme-default").expect("create mints the domain");
        assert_eq!(
            svc.policy_region("acme", "acme-default").as_deref(),
            Some("eu-west"),
            "with the manifest's region, which is what it will be asked about"
        );

        // `set_policy` never mints: an unserved domain is gone, not created.
        let absent = svc.ior_for(POLICY_DOMAIN_ID, &svc.keys.policy_key("acme", "acme-spare"));
        assert_eq!(svc.set_policy("acme", &model, Some(absent)).unwrap_err().id, OBJECT_NOT_EXIST);
        assert!(svc.policy_reference("acme", "acme-spare").is_none(), "and still does not exist");

        // A second domain, minted the only way there is, then adopted.
        let mut second = manifest("acme", "2.0", "eu-west");
        second.policy_domain = "acme-spare".into();
        svc.create("acme", second).unwrap();
        let spare = svc.policy_reference("acme", "acme-spare").unwrap();
        svc.set_policy("acme", &model, Some(spare)).unwrap();
        assert_eq!(svc.manifest_at(&model).unwrap().policy_domain, "acme-spare", "replaced");
        assert_eq!(
            svc.policy_reference("acme", "acme-default").map(|r| key_of(&r)),
            Some(key_of(&first)),
            "and the governor left behind still exists: replacing destroys nothing"
        );
    }

    /// CosCompoundLifeCycle's own question, answered by measurement:
    /// `clone_model` traverses **all three roles with `reference` semantics** —
    /// it copies the names and shares every target, never *deep* and never
    /// *shallow*. The compact form of that, and the one a mistake cannot slip
    /// past, is that exactly one new object exists afterwards.
    #[test]
    fn clone_model_traverses_all_three_relationships_by_reference() {
        let (svc, _, _, model_a, _) = two_tenants();
        let math = svc.expert_reference("acme", "math").unwrap();
        svc.bind_expert("acme", &model_a, Some(math.clone())).unwrap();
        let source = svc.manifest_at(&model_a).unwrap();
        let before = svc.served();

        let src = svc.ior_for(COMPOSED_MODEL_ID, &model_a);
        let clone = key_of(&svc.clone_model("acme", Some(src), "1.1").unwrap());
        let copy = svc.manifest_at(&clone).unwrap();

        assert_eq!(copy.base_model, source.base_model, "the base name came across");
        assert_eq!(copy.experts, source.experts, "so did the capability ids");
        assert_eq!(copy.policy_domain, source.policy_domain, "and the domain name");
        assert_eq!(copy.residency_region, source.residency_region);
        assert_ne!(copy.version, source.version, "version is the one member replaced");

        // Every target is the same object, not a copy of one.
        assert_eq!(key_of(&svc.expert_reference("acme", "math").unwrap()), key_of(&math));
        assert_eq!(
            svc.expert_shape("acme", "math"),
            Some(("llama-70b".to_owned(), 1.5, b"acme-delta".len())),
            "the adapter was shared, not duplicated with its delta"
        );
        assert_eq!(
            svc.served(),
            before + 1,
            "one new object — the model — and nothing it points at"
        );
    }

    // ── served over the wire ────────────────────────────────────────────────

    struct Served {
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<TenantService>>,
        probe: Ior,
    }

    impl Served {
        fn start(mut svc: TenantService) -> (Self, u16) {
            let server = Orb::new().server("127.0.0.1:0", b"MoE".to_vec()).unwrap();
            let port = server.local_addr().unwrap().port();
            svc.publish_at("127.0.0.1", port);
            let probe = svc.provision_factory("probe").unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let thread = std::thread::spawn(move || {
                server.serve(&mut svc, || flag.load(Ordering::SeqCst)).unwrap();
                svc
            });
            (Served { stop, thread: Some(thread), probe }, port)
        }

        fn shutdown(mut self, last: Connection) -> TenantService {
            self.stop.store(true, Ordering::SeqCst);
            drop(last);
            drop(Connection::connect(&self.probe, T));
            self.thread.take().unwrap().join().unwrap()
        }
    }

    /// The single reference a reply body carries.
    fn reference_in(reply: Reply) -> Ior {
        get_reference(&mut reply.body().expect("a reply body"))
            .expect("a decodable reference")
            .expect("a non-nil reference")
    }

    fn exception_id(r: Result<Reply, Error>) -> String {
        match r {
            Err(Error::SystemException { id, .. }) => id,
            Err(other) => format!("<{other}>"),
            Ok(_) => "<no exception>".to_owned(),
        }
    }

    /// The whole batch over a real socket: two tenants mint models through
    /// their own factories, the cross-tenant calls are refused, a retire makes
    /// one reference stop existing while the other still answers, and the
    /// manifest that comes back over the wire is the one the IDL declares —
    /// with an empty `experts` sequence first and a two-element one after two
    /// binds.
    #[test]
    fn the_isolation_properties_hold_over_the_wire() {
        let svc = TenantService::new("127.0.0.1", 0, "MoE");
        let factory_a = svc.provision_factory("acme").unwrap();
        let factory_b = svc.provision_factory("globex").unwrap();
        svc.provision_expert("acme", "math", "llama-70b", 1.5, b"acme-delta").unwrap();
        svc.provision_expert("acme", "code", "llama-70b", 2.0, b"acme-code").unwrap();
        svc.provision_expert("globex", "math", "llama-70b", 3.0, b"globex-delta").unwrap();
        let expert_a = svc.expert_reference("acme", "math").unwrap();
        let expert_b = svc.expert_reference("globex", "math").unwrap();
        let code_a = svc.expert_reference("acme", "code").unwrap();

        let (served, port) = Served::start(svc);
        let at = |ior: &Ior| {
            let mut i = ior.clone();
            i.profiles[0].port = port;
            i
        };
        let (factory_a, factory_b) = (at(&factory_a), at(&factory_b));
        let (expert_a, expert_b, code_a) = (at(&expert_a), at(&expert_b), at(&code_a));

        let mut fa = Connection::connect(&factory_a, T).unwrap();
        let ma = reference_in(
            fa.invoke("create", |e| manifest("acme", "1.0", "eu-west").write_to(e)).unwrap(),
        );
        // A manifest naming somebody else, through acme's factory.
        assert_eq!(
            exception_id(fa.invoke("create", |e| manifest("globex", "9.9", "eu-west").write_to(e))),
            NO_PERMISSION,
            "create with another tenant's manifest"
        );
        drop(fa);

        let mut fb = Connection::connect(&factory_b, T).unwrap();
        let mb = reference_in(
            fb.invoke("create", |e| manifest("globex", "1.0", "us-east").write_to(e)).unwrap(),
        );
        // globex's factory, handed acme's model.
        let ma_arg = ma.clone();
        assert_eq!(
            exception_id(fb.invoke("retire", move |e| put_reference(e, Some(&ma_arg)).unwrap())),
            NO_PERMISSION,
            "retire across tenants"
        );
        drop(fb);

        // The manifest as the IDL declares it, over the wire, with an empty
        // sequence — then two binds and a two-element one.
        let mut ca = Connection::connect(&ma, T).unwrap();
        let got =
            Manifest::read_from(&mut ca.invoke_nullary("get_manifest").unwrap().body().unwrap())
                .unwrap();
        assert_eq!(got, manifest("acme", "1.0", "eu-west"));
        assert!(got.experts.is_empty(), "an empty sequence<CapabilityId> round trips");
        for ex in [&expert_a, &code_a] {
            let arg = ex.clone();
            ca.invoke("bind_expert", move |e| put_reference(e, Some(&arg)).unwrap()).unwrap();
        }
        // acme's model, handed globex's expert.
        let foreign = expert_b.clone();
        assert_eq!(
            exception_id(
                ca.invoke("bind_expert", move |e| put_reference(e, Some(&foreign)).unwrap())
            ),
            NO_PERMISSION,
            "bind_expert across tenants"
        );
        let got =
            Manifest::read_from(&mut ca.invoke_nullary("get_manifest").unwrap().body().unwrap())
                .unwrap();
        assert_eq!(
            got.experts,
            vec!["math".to_owned(), "code".to_owned()],
            "and a two-element one"
        );
        drop(ca);

        // The two experts hand back the same shared base, typed ::moe::Expert.
        let mut ea = Connection::connect(&expert_a, T).unwrap();
        assert_eq!(
            ea.invoke_nullary("get_tenant_id").unwrap().body().unwrap().get_string().unwrap(),
            "acme"
        );
        assert_eq!(
            ea.invoke_nullary("adapter_delta").unwrap().body().unwrap().get_octet_seq().unwrap(),
            b"acme-delta"
        );
        let base_a = reference_in(ea.invoke_nullary("base").unwrap());
        drop(ea);
        let mut eb = Connection::connect(&expert_b, T).unwrap();
        let base_b = reference_in(eb.invoke_nullary("base").unwrap());
        assert_eq!(base_a, base_b, "one shared base, two tenants");
        assert_eq!(base_a.type_id, EXPERT_ID);
        drop(eb);

        // The base answers the inherited operations and narrows no further.
        let mut cb = Connection::connect(&base_a, T).unwrap();
        let cap =
            Capability::read_from(&mut cb.invoke_nullary("describe").unwrap().body().unwrap())
                .unwrap();
        assert_eq!(cap, Capability { id: "llama-70b".into(), cost: 0.0 });
        assert!(
            !cb.invoke("_is_a", |e| e.put_str(ENTERPRISE_EXPERT_ID))
                .unwrap()
                .body()
                .unwrap()
                .get_bool()
                .unwrap(),
            "the shared base is not an EnterpriseExpert"
        );
        assert_eq!(
            exception_id(cb.invoke_nullary("get_tenant_id")),
            orbweaver_giop::server::BAD_OPERATION,
            "…and has no tenant to give"
        );
        drop(cb);

        // Retire acme's model: its reference stops existing, globex's answers.
        let mut fa = Connection::connect(&factory_a, T).unwrap();
        let arg = ma.clone();
        fa.invoke("retire", move |e| put_reference(e, Some(&arg)).unwrap()).unwrap();
        drop(fa);
        let mut gone = Connection::connect(&ma, T).unwrap();
        assert_eq!(
            exception_id(gone.invoke_nullary("get_manifest")),
            OBJECT_NOT_EXIST,
            "a retired reference stops existing"
        );
        drop(gone);
        let mut alive = Connection::connect(&mb, T).unwrap();
        let other =
            Manifest::read_from(&mut alive.invoke_nullary("get_manifest").unwrap().body().unwrap())
                .unwrap();
        assert_eq!(other.tenant_id, "globex", "the other tenant's model still answers");

        let svc = served.shutdown(alive);
        assert_eq!(svc.base_crossings("acme"), 1);
        assert_eq!(svc.base_crossings("globex"), 1);
    }

    /// One object, one interface: an operation belonging to another interface
    /// is `BAD_OPERATION` rather than served by whichever object happens to
    /// have the state for it.
    #[test]
    fn each_object_answers_only_for_its_own_interface() {
        let (svc, factory_a, _, model_a, _) = two_tenants();
        let (served, port) = Served::start(svc);
        let mut factory = factory_a;
        factory.profiles[0].port = port;
        let mut model =
            Ior { type_id: COMPOSED_MODEL_ID.to_owned(), profiles: factory.profiles.clone() };
        model.profiles[0].object_key = model_a;

        let mut c = Connection::connect(&factory, T).unwrap();
        for (id, want) in [(MODEL_FACTORY_ID, true), (OBJECT_ID, true), (COMPOSED_MODEL_ID, false)]
        {
            let reply = c.invoke("_is_a", move |e| e.put_str(id)).unwrap();
            assert_eq!(reply.body().unwrap().get_bool().unwrap(), want, "factory _is_a {id}");
        }
        assert_eq!(
            exception_id(c.invoke_nullary("get_manifest")),
            orbweaver_giop::server::BAD_OPERATION,
            "a ComposedModel operation on the factory"
        );
        drop(c);

        let mut c = Connection::connect(&model, T).unwrap();
        assert!(!c.invoke_nullary("_non_existent").unwrap().body().unwrap().get_bool().unwrap());
        assert_eq!(
            exception_id(c.invoke("create", |e| Manifest::default().write_to(e))),
            orbweaver_giop::server::BAD_OPERATION,
            "a ModelFactory operation on a model"
        );
        served.shutdown(c);
    }

    /// `COMPONENTS.md`'s gap row, stated from the relationship end: the three
    /// names a manifest holds have **no inverse role and no navigation
    /// operation**. Over the wire a client reads six strings and cannot turn
    /// any of them into the reference `bind_expert` and `set_policy` demand —
    /// the only producers are out of band, which is what the gap row means by
    /// *references no operation of the contract returns*.
    #[test]
    fn no_operation_of_the_contract_navigates_a_models_relationships() {
        let (svc, factory_a, _, model_a, _) = two_tenants();
        let math = svc.expert_reference("acme", "math").unwrap();
        svc.bind_expert("acme", &model_a, Some(math)).unwrap();
        let (served, port) = Served::start(svc);
        let mut model =
            Ior { type_id: COMPOSED_MODEL_ID.to_owned(), profiles: factory_a.profiles.clone() };
        model.profiles[0].port = port;
        model.profiles[0].object_key = model_a;

        let mut c = Connection::connect(&model, T).unwrap();
        let m = Manifest::read_from(&mut c.invoke_nullary("get_manifest").unwrap().body().unwrap())
            .unwrap();
        assert_eq!(m.experts, vec!["math".to_owned()], "the ids are readable");
        assert_eq!(m.policy_domain, "acme-default");
        assert_eq!(m.base_model, "llama-70b");

        // And unresolvable: every shape a navigation operation could take is an
        // operation this interface does not declare.
        for op in ["get_experts", "experts", "get_policy", "get_policy_domain", "get_base"] {
            assert_eq!(
                exception_id(c.invoke_nullary(op)),
                orbweaver_giop::server::BAD_OPERATION,
                "{op} would be the navigation the contract does not have"
            );
        }
        served.shutdown(c);
    }

    /// A key this service never minted is nobody's, whatever it looks like.
    #[test]
    fn an_unknown_object_key_is_not_ours() {
        let (svc, _, _, model_a, _) = two_tenants();
        assert!(SharedDispatch::knows(&svc, &model_a));
        assert!(!SharedDispatch::knows(&svc, b"MoE"), "the base prefix alone names nothing");
        assert!(
            !SharedDispatch::knows(&svc, b"MoE/t/acme/model/9999"),
            "a plausible key we never minted"
        );
        assert!(!SharedDispatch::knows(&svc, b"MoE/t/nobody/factory"));
        assert!(!SharedDispatch::knows(&svc, b"NameService"));
        assert!(svc.parse(b"MoE/t/acme").is_none(), "a truncated key parses as nothing");
        assert!(svc.parse(b"other/t/acme/factory").is_none(), "another service's prefix");
    }
}
