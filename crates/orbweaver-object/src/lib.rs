//! The CORBA object model: a POA, object references as values, and the
//! pseudo-operations every ORB probes with.
//!
//! `docs/PLAN.md` §4.7 argues that references, identity and lifecycle are what
//! make a *conversation* possible, and that the AI path needs conversations:
//! `search_interfaces` → `describe_interface` → `invoke_operation` is a
//! workflow in which something must hold a reference between steps.
//!
//! # An IOR is a bearer address
//!
//! Anything holding one and able to reach the network can invoke the target
//! directly. That is fine between native peers inside a trust boundary and is
//! exactly what must not cross the MCP boundary, where a raw IOR would route
//! around `orbweaver-guard` — past the authorization checks, the destructive
//! approvals and the audit log. Capability handles (§4.7, Phase 3.5) are the
//! answer there; this crate deliberately deals in raw references, because it
//! sits on the native side of that line.

#![deny(missing_docs)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use orbweaver_cdr::{Decoder, Encoder};
use orbweaver_giop::server::{Completion, Request, SystemException};
use orbweaver_giop::{IiopProfile, Ior, Version};
use orbweaver_registry::Registry;

pub mod expert_service;
pub mod policy;
pub mod residency;
pub mod tenant_service;

use policy::{
    IdAssignmentPolicy, ImplicitActivationPolicy, LifespanPolicy, Policies,
    RequestProcessingPolicy, ServantRetentionPolicy, ThreadPolicy,
};

/// Repository id every CORBA object answers to.
pub const OBJECT_ID: &str = "IDL:omg.org/CORBA/Object:1.0";

/// The ORB's fourth responsibility, on this side of the crate boundary: it
/// hands out the root POA (D019 §5).
///
/// # Why this is a trait and not a method on `Orb`
///
/// `orbweaver-object` depends on `orbweaver-giop`, so [`Poa`] is a type the
/// ORB's own crate cannot name. That direction is not incidental — the POA
/// dispatches on a transport the GIOP layer owns, and reversing it would make
/// the dependency graph cyclic. An extension trait is what Rust offers for
/// *"this crate adds an operation to that crate's type"*, and the result reads
/// the way D019 asks it to read: a consumer asks the ORB, rather than
/// constructing a POA and a `Server` separately and hoping the two agree about
/// the object key.
///
/// This is the honest shape rather than the one D019 §5 pictured, and the
/// difference is worth stating: the ORB **hands out** a root POA, it does not
/// *own* one. Nothing here is stored on the `Orb`, because a POA holds live
/// servant state and an `Orb` that owned one would need interior mutability
/// chosen before there is a caller that needs it — the same reason
/// [`Orb`](orbweaver_giop::orb::Orb)'s initial references table does not have
/// it either.
///
/// *크레이트 의존 방향이 한쪽이므로 확장 트레이트가 정직한 모양이다. ORB는 루트
/// POA를 **내어주지만** 소유하지는 않는다.*
pub trait OrbPoa {
    /// `PortableServer::POA::create_POA` (CORBA 3.4 §15.3.8.5), minus the
    /// policy list — policies are chosen here with [`Poa::with_lifespan`] and
    /// its neighbours, which is the same set under Rust spelling.
    ///
    /// `type_id` is the repository id every reference this POA mints will
    /// claim. The specification's `create_POA` has no such argument because a
    /// C++ POA learns the type from the servant it activates; ours mints
    /// references directly, so it has to be told.
    fn create_poa(&self, name: &str, type_id: &str) -> Poa;

    /// The root POA — `create_poa` under the `ObjectId` CORBA 3.4 §8.5.2
    /// reserves for it, `RootPOA`.
    ///
    /// The name is the specification's and is not ours to invent; it is the
    /// name a peer asks for with `corbaloc:rir:RootPOA`, and registering this
    /// POA's reference under that key is what makes such a request resolvable.
    fn root_poa(&self, type_id: &str) -> Poa;
}

impl OrbPoa for orbweaver_giop::orb::Orb {
    fn create_poa(&self, name: &str, type_id: &str) -> Poa {
        Poa::new(name, type_id)
    }

    fn root_poa(&self, type_id: &str) -> Poa {
        Poa::new("RootPOA", type_id)
    }
}

/// A servant's identity within a POA.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(pub Vec<u8>);

impl ObjectId {
    /// An id from readable text, which is what a persistent servant usually has.
    pub fn from_name(name: &str) -> Self {
        ObjectId(name.as_bytes().to_vec())
    }

    /// The id as text, when it happens to be text.
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.0).ok()
    }
}

/// How long a reference is meant to stay valid.
///
/// This is the specification's **Lifespan policy**, CORBA 3.4 §15.3.8.2, under
/// a shorter name — and it is the only one of the seven §15.3.8 policies that
/// was named here before D020. [`policy::LifespanPolicy`] carries the spec's
/// value names, and [`Poa::policies`] maps this onto it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifespan {
    /// Valid only while this process lives. The id carries a nonce so a
    /// reference from a previous run is recognisably stale rather than
    /// silently landing on whatever now occupies the same id.
    Transient,
    /// Meant to outlive the process, so the id must be reproducible.
    Persistent,
}

/// What to do when an object id is not currently activated.
///
/// This ranges over two of the three values of the specification's **Request
/// Processing policy**, CORBA 3.4 §15.3.8.6, which nothing said until D020:
/// [`Reject`](Self::Reject) is `USE_ACTIVE_OBJECT_MAP_ONLY` and
/// [`AskLocator`](Self::AskLocator) is `USE_SERVANT_MANAGER`.
/// `USE_DEFAULT_SERVANT` has no analogue here — see
/// [`policy::RequestProcessingPolicy`], which also records where our
/// `AskLocator` with no locator diverges from the section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownIdPolicy {
    /// Raise `OBJECT_NOT_EXIST`. §15.3.8.6's `USE_ACTIVE_OBJECT_MAP_ONLY`.
    Reject,
    /// Ask the locator, which may activate one or forward the caller.
    /// §15.3.8.6's `USE_SERVANT_MANAGER`.
    AskLocator,
}

/// What a locator decided about an inactive object id.
#[derive(Debug, Clone)]
pub enum Located {
    /// Serve it here; the servant is now active.
    Here,
    /// Send the caller elsewhere, transparently (§9.4.3.2).
    Forward(Ior),
    /// No such object.
    Unknown,
}

/// Consulted when a request arrives for an id that is not active.
///
/// This is the hook that produces `LOCATION_FORWARD`: a real deployment uses it
/// to move objects between processes without callers noticing, and TAO's
/// ImplRepository forwards every first call this way.
pub trait ServantLocator {
    /// Decides what to do with `id`.
    fn locate(&mut self, id: &ObjectId) -> Located;
}

/// A value that is distinct across processes *and* within one.
///
/// Across processes, so a transient reference held over a restart is
/// recognisably stale rather than landing on whatever occupies that id now.
/// Within a process, so two POAs of the same name do not adopt each other's
/// references.
///
/// An earlier version took the address of a temporary `Box`, on the stated
/// reasoning that avoiding the clock kept tests deterministic. Distinctness,
/// not determinism, is what the field is for, and the temporary was freed
/// before the next call — so the allocator handed back the same address and two
/// POAs created in sequence shared an incarnation. That is precisely the
/// staleness this field exists to detect, and it survived review by passing:
/// the allocator happened to vary the address until a rebuild stopped it.
fn next_incarnation() -> u64 {
    static SEED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    static MINTED: AtomicU64 = AtomicU64::new(0);
    const ODD: u64 = 0x9E37_79B9_7F4A_7C15;

    let seed = *SEED.get_or_init(|| {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        // The pid alone repeats after a wrap and the clock alone can be coarse
        // or stepped backwards; neither failure mode is shared with the other.
        nanos.rotate_left(17) ^ u64::from(std::process::id()).wrapping_mul(ODD)
    });
    seed.wrapping_add(MINTED.fetch_add(1, Ordering::Relaxed).wrapping_mul(ODD))
}

/// A Portable Object Adapter: servant identity, lifecycle and reference
/// creation.
#[derive(Debug)]
pub struct Poa {
    name: String,
    active: HashMap<ObjectId, ()>,
    lifespan: Lifespan,
    unknown_id: UnknownIdPolicy,
    /// Distinguishes references minted by this run from earlier ones.
    incarnation: u64,
    /// The type every reference this POA mints claims to be.
    type_id: String,
    /// Where references made here tell callers to connect.
    published: Option<(String, u16)>,
    next_transient: AtomicU64,
}

impl Poa {
    /// A POA whose references claim `type_id`.
    ///
    /// `pub(crate)` since D019 step 4: a POA is obtained from the ORB, through
    /// [`OrbPoa::create_poa`] or [`OrbPoa::root_poa`]. See [`OrbPoa`] for why
    /// that is an extension trait in this crate rather than an inherent method
    /// on `Orb`.
    pub(crate) fn new(name: &str, type_id: &str) -> Self {
        Self {
            name: name.to_owned(),
            active: HashMap::new(),
            lifespan: Lifespan::Transient,
            unknown_id: UnknownIdPolicy::Reject,
            incarnation: next_incarnation(),
            type_id: type_id.to_owned(),
            published: None,
            next_transient: AtomicU64::new(1),
        }
    }

    /// Sets the lifespan policy.
    pub fn with_lifespan(mut self, l: Lifespan) -> Self {
        self.lifespan = l;
        self
    }

    /// Sets what happens when an id is not active.
    pub fn with_unknown_id(mut self, p: UnknownIdPolicy) -> Self {
        self.unknown_id = p;
        self
    }

    /// Sets the host and port that references made here advertise.
    ///
    /// Separate from the bind address on purpose: behind NAT or in a container
    /// they differ, and publishing the bind address is the failure Phase 0
    /// assumption D reproduced.
    pub fn publish_at(mut self, host: &str, port: u16) -> Self {
        self.published = Some((host.to_owned(), port));
        self
    }

    /// The POA's name, which prefixes every object key it mints.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What this POA behaves as, for each of the seven policies of
    /// CORBA 3.4 §15.3.8.
    ///
    /// **Computed, never stored, and not configurable.** Two fields vary —
    /// [`Lifespan`] gives §15.3.8.2 and [`UnknownIdPolicy`] gives §15.3.8.6 —
    /// and the other five are the same for every `Poa` that can exist today.
    /// That they are constants is the finding rather than an oversight in the
    /// report: a policy nobody can select still has a value, and four of these
    /// had never been written down. See [`policy`] for each one's section, its
    /// meaning, and whether we chose it.
    ///
    /// The answer for §15.3.8.4 is [`IdAssignmentPolicy::Either`], which is
    /// **ours and not the specification's** — this POA accepts both assignment
    /// models on one adapter. D020 Stage A records that; Stage B makes the two
    /// specified values real.
    ///
    /// Every field is backed by a behavioural test in this crate except
    /// §15.3.8.1 (the concurrency lives in `orbweaver_giop::Server`) and
    /// §15.3.8.3 (a policy about servants, in a map that holds none). Those
    /// two say so in their own documentation rather than being covered by a
    /// test that would pass whatever they said.
    pub fn policies(&self) -> Policies {
        Policies {
            // §15.3.8.1 — implicit, and decided a layer away: the ORB assigns
            // requests to threads (one per connection, in `Server`).
            thread: ThreadPolicy::OrbCtrlModel,
            // §15.3.8.2 — the one policy a caller chose.
            lifespan: match self.lifespan {
                Lifespan::Transient => LifespanPolicy::Transient,
                Lifespan::Persistent => LifespanPolicy::Persistent,
            },
            // §15.3.8.3 — `active` is `HashMap<ObjectId, ()>`: ids, not
            // servants, so the policy has nothing here to be about.
            id_uniqueness: None,
            // §15.3.8.4 — the divergence. `activate` is USER_ID and
            // `activate_new` is SYSTEM_ID, on the same POA.
            id_assignment: IdAssignmentPolicy::Either,
            // §15.3.8.5 — what a locator resolves is inserted into `active`
            // and survives the request, which is RETAIN however the hook is
            // spelled.
            servant_retention: ServantRetentionPolicy::Retain,
            // §15.3.8.6 — the correspondence `UnknownIdPolicy` already had
            // without saying so.
            request_processing: match self.unknown_id {
                UnknownIdPolicy::Reject => RequestProcessingPolicy::UseActiveObjectMapOnly,
                UnknownIdPolicy::AskLocator => RequestProcessingPolicy::UseServantManager,
            },
            // §15.3.8.7 — nothing becomes active as a side effect of anything.
            implicit_activation: ImplicitActivationPolicy::NoImplicitActivation,
        }
    }

    /// Activates `id`, so requests for it are served here.
    pub fn activate(&mut self, id: ObjectId) {
        self.active.insert(id, ());
    }

    /// Activates a fresh transient id and returns it.
    pub fn activate_new(&mut self) -> ObjectId {
        let n = self.next_transient.fetch_add(1, Ordering::Relaxed);
        let id = ObjectId(format!("obj{n}").into_bytes());
        self.activate(id.clone());
        id
    }

    /// Deactivates `id`. Later requests for it are unknown.
    pub fn deactivate(&mut self, id: &ObjectId) -> bool {
        self.active.remove(id).is_some()
    }

    /// Whether `id` is currently activated.
    pub fn is_active(&self, id: &ObjectId) -> bool {
        self.active.contains_key(id)
    }

    /// The object key a reference to `id` carries.
    ///
    /// A transient key includes the incarnation, so a reference from a previous
    /// run is recognisably stale instead of silently reaching whatever occupies
    /// that id now — which would be the worst kind of correct-looking bug.
    ///
    /// # The unstated invariant, and it is not enforced here
    ///
    /// A key is `name` `/` \[`incarnation` `/`\] `id`, concatenated, and
    /// **nothing constrains the components**. So the relationship between a POA
    /// and the keys it mints has an integrity rule nobody wrote down: *no POA
    /// name may be a `/`-delimited prefix of another POA's name plus an object
    /// id.* Measured 2026-08-25 and left as it is (D023 R1 changes no
    /// behaviour): with [`Lifespan::Persistent`], `Poa::new("Root")` with the
    /// id `POA/x` and `Poa::new("Root/POA")` with the id `x` mint the **same
    /// bytes**, and each POA's [`Poa::parse_key`] accepts the other's key —
    /// [`Poa::name`] calls itself "the POA's name, which prefixes every object
    /// key it mints" without saying that a prefix must be unambiguous.
    /// `two_poas_whose_names_are_prefixes_mint_the_same_object_key` pins the
    /// persistent case as measured. Whether the incarnation makes the transient
    /// case immune is **unmeasured** — it does not obviously, since an object id
    /// may contain the hex an incarnation would.
    ///
    /// [`tenant_service::is_key_safe`](crate::tenant_service) enforces exactly
    /// this rule for the other key space in this crate — every string that
    /// becomes part of a tenant key is refused if it is empty or contains `/` —
    /// and **neither names the other**, which is how one key space came to have
    /// the rule and the other to have only the habit. No caller in this
    /// workspace puts a `/` in a POA name or an object id today, so no fix is
    /// applied here: refusing one would change behaviour for
    /// [`ObjectId::from_name`]'s one data-driven caller (`residency::reconcile`
    /// mints ids from expert ids), and escaping the separator would change
    /// every key already minted, including persistent ones already handed out.
    ///
    /// *POA와 그 키 사이의 관계에도 무결성 규칙이 있으나 적혀 있지 않았다:
    /// 한 POA의 이름이 다른 POA의 이름 + 객체 id의 접두사가 되어서는 안 된다.
    /// 같은 크레이트의 다른 키 공간은 이 규칙을 강제하고, 서로를 이름하지 않는다.*
    pub fn object_key(&self, id: &ObjectId) -> Vec<u8> {
        let mut key = Vec::new();
        key.extend_from_slice(self.name.as_bytes());
        key.push(b'/');
        if self.lifespan == Lifespan::Transient {
            key.extend_from_slice(format!("{:x}", self.incarnation).as_bytes());
            key.push(b'/');
        }
        key.extend_from_slice(&id.0);
        key
    }

    /// Recovers an object id from a key this POA minted, if it did.
    pub fn parse_key(&self, key: &[u8]) -> Option<ObjectId> {
        let rest = key.strip_prefix(self.name.as_bytes())?.strip_prefix(b"/")?;
        if self.lifespan == Lifespan::Transient {
            let want = format!("{:x}/", self.incarnation);
            let rest = rest.strip_prefix(want.as_bytes())?;
            return Some(ObjectId(rest.to_vec()));
        }
        Some(ObjectId(rest.to_vec()))
    }

    /// Builds a reference to `id`.
    pub fn reference(&self, id: &ObjectId) -> Option<Ior> {
        let (host, port) = self.published.clone()?;
        Some(Ior {
            type_id: self.type_id.clone(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host,
                port,
                object_key: self.object_key(id),
                // §7.10.2.4: no TAG_CODE_SETS is a declaration of no wchar
                // support, and a conformant client then refuses inside itself
                // without sending anything (measured, omniORB 4.3.4). D009's
                // L2, landed with the rest of its cause rather than one site
                // at a time: the conversion lists stay empty, so this
                // advertises UTF-8 — which we have — and nothing we do not.
                components: vec![orbweaver_giop::codeset::server_component()],
            }],
        })
    }

    /// Decides how to handle a request addressed to `key`.
    pub fn dispatch_target(
        &mut self,
        key: &[u8],
        locator: Option<&mut dyn ServantLocator>,
    ) -> Target {
        let Some(id) = self.parse_key(key) else {
            // A key we did not mint, or a stale transient one from an earlier
            // incarnation. Either way it names nothing here.
            return Target::Unknown;
        };
        if self.is_active(&id) {
            return Target::Active(id);
        }
        match (self.unknown_id, locator) {
            (UnknownIdPolicy::AskLocator, Some(l)) => match l.locate(&id) {
                Located::Here => {
                    self.activate(id.clone());
                    Target::Active(id)
                }
                Located::Forward(ior) => Target::Forward(ior),
                Located::Unknown => Target::Unknown,
            },
            _ => Target::Unknown,
        }
    }
}

/// What a POA decided about an incoming request.
#[derive(Debug)]
pub enum Target {
    /// Serve it, for this object id.
    Active(ObjectId),
    /// Reply `LOCATION_FORWARD` with this reference.
    Forward(Ior),
    /// Reply `OBJECT_NOT_EXIST`.
    Unknown,
}

// ─────────────────────────────────────────────────────────────────────────────
// Object references as values
// ─────────────────────────────────────────────────────────────────────────────

/// Writes an object reference as an operation argument or result.
///
/// §9.3.6 marshals a reference **inline**, not as an encapsulation, which is
/// the form a `Registry::lookup` return value or an `Object`-typed parameter
/// takes.
pub fn put_reference(e: &mut Encoder, ior: Option<&Ior>) -> Result<(), orbweaver_giop::Error> {
    match ior {
        Some(i) => i.write_to(e),
        // A nil reference is an empty type id and no profiles, which is
        // distinct from an absent field.
        None => Ior { type_id: String::new(), profiles: Vec::new() }.write_to(e),
    }
}

/// Reads an object reference, returning `None` for the nil reference.
pub fn get_reference(d: &mut Decoder<'_>) -> Result<Option<Ior>, orbweaver_giop::Error> {
    let ior = Ior::read_from(d)?;
    Ok(if ior.is_nil() { None } else { Some(ior) })
}

/// Whether two references denote the same object, as far as can be told.
///
/// §7.2.1 permits `_is_equivalent` to answer `false` for two references that do
/// denote the same object, so it can **confirm** identity and never refute it.
/// Anything treating a `false` as proof of difference is wrong, which is why
/// this is documented rather than left to intuition.
pub fn is_equivalent(a: &Ior, b: &Ior) -> bool {
    match (a.profiles.first(), b.profiles.first()) {
        (Some(x), Some(y)) => x.object_key == y.object_key && x.host == y.host && x.port == y.port,
        (None, None) => a.type_id == b.type_id,
        _ => false,
    }
}

/// A hash consistent with [`is_equivalent`]: equivalent references hash alike.
///
/// The converse does not hold, and must not be assumed — `_hash` exists to
/// bucket references, not to compare them.
pub fn reference_hash(ior: &Ior, maximum: u32) -> u32 {
    // FNV-1a: small, stable across runs, and not trying to be a good hash for
    // anything but bucketing.
    let mut h: u32 = 0x811c_9dc5;
    if let Some(p) = ior.profiles.first() {
        for b in p.host.as_bytes().iter().chain(&p.port.to_be_bytes()).chain(&p.object_key) {
            h ^= u32::from(*b);
            h = h.wrapping_mul(0x0100_0193);
        }
    }
    if maximum == 0 { h } else { h % maximum }
}

// ─────────────────────────────────────────────────────────────────────────────
// Pseudo-operations
// ─────────────────────────────────────────────────────────────────────────────

/// Serves the operations every ORB probes with, so a servant need not.
///
/// Without `_is_a` there is no narrowing, and every peer observed here calls at
/// least one of these before or during ordinary use.
pub struct ObjectOps<'a> {
    /// Where inheritance answers come from — locally, per §4.7.
    pub registry: &'a Registry,
    /// The repository id of the object being addressed.
    pub type_id: &'a str,
    /// The reference to that object, for identity questions.
    pub reference: Option<&'a Ior>,
}

impl ObjectOps<'_> {
    /// Whether `operation` is one of the pseudo-operations.
    pub fn handles(operation: &str) -> bool {
        matches!(
            operation,
            "_is_a" | "_non_existent" | "_not_existent" | "_is_equivalent" | "_hash" | "_interface"
        )
    }

    /// Serves a pseudo-operation, writing the reply body into `out`.
    pub fn dispatch(&self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        let mut args = request.body().map_err(|_| SystemException::marshal())?;
        match request.operation.as_str() {
            "_is_a" => {
                let want = args.get_string().map_err(|_| SystemException::marshal())?;
                out.put_bool(self.registry.is_a(self.type_id, &want) || want == OBJECT_ID);
            }
            // The typo'd spelling exists in GIOP 1.0/1.1 peers because of an
            // error in CORBA 2.0-2.2; 1.2 uses only `_non_existent`.
            "_non_existent" | "_not_existent" => out.put_bool(false),
            "_is_equivalent" => {
                let other = get_reference(&mut args).map_err(|_| SystemException::marshal())?;
                let same = match (self.reference, other.as_ref()) {
                    (Some(a), Some(b)) => is_equivalent(a, b),
                    _ => false,
                };
                out.put_bool(same);
            }
            "_hash" => {
                let maximum = args.get_u32().unwrap_or(0);
                out.put_u32(self.reference.map_or(0, |r| reference_hash(r, maximum)));
            }
            // Answering this needs an Interface Repository object to hand back,
            // which we do not expose. Saying so beats returning a nil the caller
            // will dereference.
            "_interface" => {
                return Err(SystemException {
                    id: "IDL:omg.org/CORBA/NO_IMPLEMENT:1.0".into(),
                    minor: 0,
                    completed: Completion::No,
                });
            }
            _ => return Err(SystemException::bad_operation()),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_cdr::{Encoder as Enc, Endian};

    fn ior(host: &str, port: u16, key: &[u8]) -> Ior {
        Ior {
            type_id: "IDL:m/I:1.0".into(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: host.into(),
                port,
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

    #[test]
    fn object_keys_round_trip_through_the_poa_that_minted_them() {
        let poa = Poa::new("RootPOA", "IDL:m/I:1.0");
        let id = ObjectId::from_name("servant-1");
        let key = poa.object_key(&id);
        assert_eq!(poa.parse_key(&key), Some(id));
    }

    /// A transient reference from a previous run must be recognisably stale,
    /// not silently land on whatever occupies that id now.
    #[test]
    fn a_transient_key_from_another_incarnation_is_not_recognised() {
        let a = Poa::new("RootPOA", "IDL:m/I:1.0");
        let b = Poa::new("RootPOA", "IDL:m/I:1.0");
        let id = ObjectId::from_name("x");
        assert_ne!(a.object_key(&id), b.object_key(&id), "incarnations must differ");
        assert_eq!(b.parse_key(&a.object_key(&id)), None);
    }

    /// The two-POA test above passed for a while against an implementation
    /// that reused a freed heap address, because the allocator happened to
    /// vary it. Asking for many at once removes the luck.
    #[test]
    fn every_poa_in_a_process_gets_its_own_incarnation() {
        let id = ObjectId::from_name("x");
        let keys: std::collections::BTreeSet<Vec<u8>> =
            (0..64).map(|_| Poa::new("RootPOA", "IDL:m/I:1.0").object_key(&id)).collect();
        assert_eq!(keys.len(), 64, "two POAs minted the same transient key");
    }

    #[test]
    fn a_persistent_key_is_reproducible_across_runs() {
        let a = Poa::new("RootPOA", "IDL:m/I:1.0").with_lifespan(Lifespan::Persistent);
        let b = Poa::new("RootPOA", "IDL:m/I:1.0").with_lifespan(Lifespan::Persistent);
        let id = ObjectId::from_name("well-known");
        assert_eq!(a.object_key(&id), b.object_key(&id));
        assert_eq!(b.parse_key(&a.object_key(&id)), Some(id));
    }

    #[test]
    fn a_key_from_another_poa_is_not_ours() {
        let mine = Poa::new("RootPOA", "IDL:m/I:1.0").with_lifespan(Lifespan::Persistent);
        let other = Poa::new("OtherPOA", "IDL:m/I:1.0").with_lifespan(Lifespan::Persistent);
        let id = ObjectId::from_name("x");
        assert_eq!(mine.parse_key(&other.object_key(&id)), None);
    }

    /// **A finding, pinned as measured and not endorsed** — the companion to
    /// the test above, which shows a key from *another* POA being refused.
    ///
    /// A POA name and an object id are concatenated with `/` and neither is
    /// constrained, so two POAs whose names are prefixes of one another mint
    /// the identical key and each accepts the other's. `Poa::object_key`'s docs
    /// state the invariant this violates and why no fix is applied inside a
    /// naming batch. Enforcing it turns this test red, which is the signal
    /// wanted: `tenant_service::is_key_safe` already enforces the same rule for
    /// the other key space in this crate.
    #[test]
    fn two_poas_whose_names_are_prefixes_mint_the_same_object_key() {
        let outer = Poa::new("Root", "IDL:m/I:1.0").with_lifespan(Lifespan::Persistent);
        let inner = Poa::new("Root/POA", "IDL:m/I:1.0").with_lifespan(Lifespan::Persistent);
        let (nested, plain) = (ObjectId::from_name("POA/x"), ObjectId::from_name("x"));

        assert_eq!(
            outer.object_key(&nested),
            inner.object_key(&plain),
            "measured: two POAs mint the identical object key"
        );
        // And each adopts the other's reference, which is the consequence that
        // matters: a request lands on whichever POA is asked first.
        assert_eq!(inner.parse_key(&outer.object_key(&nested)), Some(plain));
        assert_eq!(outer.parse_key(&inner.object_key(&ObjectId::from_name("x"))), Some(nested));
    }

    #[test]
    fn activation_controls_whether_a_request_is_served() {
        let mut poa = Poa::new("P", "IDL:m/I:1.0");
        let id = poa.activate_new();
        let key = poa.object_key(&id);
        assert!(matches!(poa.dispatch_target(&key, None), Target::Active(_)));
        poa.deactivate(&id);
        assert!(matches!(poa.dispatch_target(&key, None), Target::Unknown));
    }

    struct Forwarder(Ior);
    impl ServantLocator for Forwarder {
        fn locate(&mut self, _id: &ObjectId) -> Located {
            Located::Forward(self.0.clone())
        }
    }

    struct Activator;
    impl ServantLocator for Activator {
        fn locate(&mut self, _id: &ObjectId) -> Located {
            Located::Here
        }
    }

    /// The hook that produces LOCATION_FORWARD, which Phase 1 could follow but
    /// never send.
    #[test]
    fn a_locator_can_forward_or_activate() {
        let mut poa = Poa::new("P", "IDL:m/I:1.0").with_unknown_id(UnknownIdPolicy::AskLocator);
        let key = poa.object_key(&ObjectId::from_name("absent"));

        let mut fwd = Forwarder(ior("elsewhere", 9, b"k"));
        match poa.dispatch_target(&key, Some(&mut fwd)) {
            Target::Forward(to) => assert_eq!(to.primary().unwrap().host, "elsewhere"),
            other => panic!("{other:?}"),
        }

        let mut act = Activator;
        assert!(matches!(poa.dispatch_target(&key, Some(&mut act)), Target::Active(_)));
        assert!(poa.is_active(&ObjectId::from_name("absent")), "locating activates it");
    }

    #[test]
    fn without_a_locator_an_inactive_id_is_unknown() {
        let mut poa = Poa::new("P", "IDL:m/I:1.0").with_unknown_id(UnknownIdPolicy::AskLocator);
        let key = poa.object_key(&ObjectId::from_name("absent"));
        assert!(matches!(poa.dispatch_target(&key, None), Target::Unknown));
    }

    // ── the seven policies, each claim against the behaviour ────────────────
    //
    // D020 Stage A's oracle. Every one of these asserts the value
    // `Poa::policies()` reports **and** the behaviour that value names, in the
    // same test, so that changing the claim alone makes it red. A report
    // nothing checks against behaviour is the green-while-measuring-nothing
    // class this project has found six times in a week.
    //
    // Two of the seven have no test here and say so instead:
    //   §15.3.8.1 Thread — the concurrency is `orbweaver_giop::Server`'s, and
    //     `Poa` has no thread field to observe.
    //   §15.3.8.3 Object Id Uniqueness — the map holds ids, not servants, so
    //     there is nothing for the policy to constrain and nothing a test
    //     could refute.

    /// §15.3.8.2 — TRANSIENT claimed, and TRANSIENT behaved: the key carries
    /// the incarnation, so the next run's POA refuses it.
    #[test]
    fn transient_is_claimed_and_a_transient_key_does_not_survive_the_process() {
        let a = Poa::new("P", "IDL:m/I:1.0");
        let b = Poa::new("P", "IDL:m/I:1.0");
        assert_eq!(a.policies().lifespan, LifespanPolicy::Transient);
        assert_eq!(b.parse_key(&a.object_key(&ObjectId::from_name("x"))), None);
    }

    /// §15.3.8.2 — PERSISTENT claimed, and PERSISTENT behaved: the key is
    /// reproducible, so another instantiation of the same POA accepts it.
    #[test]
    fn persistent_is_claimed_and_a_persistent_key_outlives_the_process() {
        let a = Poa::new("P", "IDL:m/I:1.0").with_lifespan(Lifespan::Persistent);
        let b = Poa::new("P", "IDL:m/I:1.0").with_lifespan(Lifespan::Persistent);
        assert_eq!(a.policies().lifespan, LifespanPolicy::Persistent);
        let id = ObjectId::from_name("well-known");
        assert_eq!(b.parse_key(&a.object_key(&id)), Some(id));
    }

    /// §15.3.8.6 — USE_ACTIVE_OBJECT_MAP_ONLY claimed, and behaved: a locator
    /// that *would* have said `Here` is offered and never asked.
    #[test]
    fn use_active_object_map_only_is_claimed_and_the_map_is_the_only_source() {
        let mut poa = Poa::new("P", "IDL:m/I:1.0");
        assert_eq!(
            poa.policies().request_processing,
            RequestProcessingPolicy::UseActiveObjectMapOnly
        );
        let absent = ObjectId::from_name("absent");
        let key = poa.object_key(&absent);
        let mut always_here = Activator;
        assert!(matches!(poa.dispatch_target(&key, Some(&mut always_here)), Target::Unknown));
        assert!(!poa.is_active(&absent), "the locator must not have been consulted");
    }

    /// §15.3.8.6 — USE_SERVANT_MANAGER claimed, and behaved: the same locator,
    /// the same absent id, and now it is asked.
    #[test]
    fn use_servant_manager_is_claimed_and_the_manager_is_consulted() {
        let mut poa = Poa::new("P", "IDL:m/I:1.0").with_unknown_id(UnknownIdPolicy::AskLocator);
        assert_eq!(poa.policies().request_processing, RequestProcessingPolicy::UseServantManager);
        let absent = ObjectId::from_name("absent");
        let key = poa.object_key(&absent);
        let mut always_here = Activator;
        assert!(matches!(poa.dispatch_target(&key, Some(&mut always_here)), Target::Active(_)));
    }

    /// §15.3.8.5 — RETAIN claimed, and behaved: what the manager resolved
    /// survives the request that resolved it, so a second dispatch with **no**
    /// manager at all is still served.
    ///
    /// This is the claim D020 §3 row 5 guessed the other way round, from the
    /// name `ServantLocator` — which is the specification's NON_RETAIN half.
    /// The behaviour is the RETAIN one.
    #[test]
    fn retain_is_claimed_and_a_located_id_outlives_the_request_that_located_it() {
        let mut poa = Poa::new("P", "IDL:m/I:1.0").with_unknown_id(UnknownIdPolicy::AskLocator);
        assert_eq!(poa.policies().servant_retention, ServantRetentionPolicy::Retain);
        let key = poa.object_key(&ObjectId::from_name("x"));
        let mut once = Activator;
        assert!(matches!(poa.dispatch_target(&key, Some(&mut once)), Target::Active(_)));
        // No locator this time. Under NON_RETAIN this would be `Unknown`.
        assert!(matches!(poa.dispatch_target(&key, None), Target::Active(_)));
    }

    /// §15.3.8.7 — NO_IMPLICIT_ACTIVATION claimed, and behaved: minting a
    /// reference is the operation a POA with IMPLICIT_ACTIVATION would
    /// activate on, and afterwards the id is still inactive and still unknown
    /// to a request.
    #[test]
    fn no_implicit_activation_is_claimed_and_minting_a_reference_activates_nothing() {
        let mut poa = Poa::new("P", "IDL:m/I:1.0").publish_at("h", 1);
        assert_eq!(
            poa.policies().implicit_activation,
            ImplicitActivationPolicy::NoImplicitActivation
        );
        let never = ObjectId::from_name("never-activated");
        let key = poa.object_key(&never);
        assert!(poa.reference(&never).is_some(), "a reference is mintable regardless");
        assert!(!poa.is_active(&never), "and minting it must not have activated it");
        assert!(matches!(poa.dispatch_target(&key, None), Target::Unknown));
    }

    /// §15.3.8.4 — the divergence, claimed and behaved. The section makes id
    /// assignment a **per-POA** choice; this POA answers to both models at
    /// once, which is why the reported value is one the specification does not
    /// have. `IdAssignmentPolicy::default()` is the spec's `SYSTEM_ID`, and
    /// the two disagreeing here *is* the divergence.
    #[test]
    fn id_assignment_is_ours_because_one_poa_answers_to_both_models() {
        let mut poa = Poa::new("P", "IDL:m/I:1.0");
        assert_eq!(poa.policies().id_assignment, IdAssignmentPolicy::Either);
        assert_ne!(
            poa.policies().id_assignment,
            IdAssignmentPolicy::default(),
            "§15.3.8.4 defaults to SYSTEM_ID; we behave as neither of its values"
        );

        // USER_ID: the application chose the id.
        let user = ObjectId::from_name("chosen-by-the-application");
        poa.activate(user.clone());
        // SYSTEM_ID: the POA chose it. On the same adapter, in the same test.
        let system = poa.activate_new();

        assert_ne!(user, system);
        assert!(poa.is_active(&user) && poa.is_active(&system));
        let ukey = poa.object_key(&user);
        let skey = poa.object_key(&system);
        assert!(matches!(poa.dispatch_target(&ukey, None), Target::Active(_)));
        assert!(matches!(poa.dispatch_target(&skey, None), Target::Active(_)));
    }

    /// §15.3.8.3 — reported as **not applicable**, and this is a claim about
    /// the report and not about behaviour. There is no servant in the map for
    /// a uniqueness policy to be about, so no test can refute either value;
    /// answering `None` rather than picking one is the honest result.
    #[test]
    fn object_id_uniqueness_reports_not_applicable_rather_than_a_value() {
        assert_eq!(Poa::new("P", "IDL:m/I:1.0").policies().id_uniqueness, None);
    }

    /// §15.3.8.1 — reported as ORB_CTRL_MODEL, **unobservable from here**, and
    /// this test says only that the report is constant across every POA this
    /// crate can build. It is not evidence about threading; the evidence would
    /// have to come from `orbweaver_giop::Server`.
    #[test]
    fn the_thread_model_is_reported_but_not_measured_here() {
        for p in [
            Poa::new("P", "IDL:m/I:1.0"),
            Poa::new("P", "IDL:m/I:1.0").with_lifespan(Lifespan::Persistent),
            Poa::new("P", "IDL:m/I:1.0").with_unknown_id(UnknownIdPolicy::AskLocator),
        ] {
            assert_eq!(p.policies().thread, ThreadPolicy::OrbCtrlModel);
        }
    }

    /// Nothing this crate can build breaks a constraint §15.3.8 states between
    /// policies — including `IMPLICIT_ACTIVATION requires SYSTEM_ID and
    /// RETAIN` (§15.3.8.7, verbatim), which our `Either` would violate if
    /// implicit activation were ever turned on.
    #[test]
    fn no_poa_we_can_build_violates_a_policy_constraint() {
        for p in [
            Poa::new("P", "IDL:m/I:1.0"),
            Poa::new("P", "IDL:m/I:1.0").with_lifespan(Lifespan::Persistent),
            Poa::new("P", "IDL:m/I:1.0").with_unknown_id(UnknownIdPolicy::AskLocator),
            Poa::new("P", "IDL:m/I:1.0")
                .with_lifespan(Lifespan::Persistent)
                .with_unknown_id(UnknownIdPolicy::AskLocator),
        ] {
            assert!(p.policies().spec_violations().is_empty(), "{:?}", p.policies());
        }
    }

    // ── references as values ────────────────────────────────────────────────

    #[test]
    fn references_round_trip_as_operation_arguments() {
        let r = ior("10.0.0.1", 4001, b"servant");
        let mut e = Enc::new(Endian::Little);
        put_reference(&mut e, Some(&r)).unwrap();
        let bytes = e.finish().unwrap();
        let mut d = Decoder::new(&bytes, Endian::Little);
        assert_eq!(get_reference(&mut d).unwrap(), Some(r));
    }

    /// A nil reference is a legal value and distinct from an absent field.
    #[test]
    fn the_nil_reference_round_trips_as_none() {
        let mut e = Enc::new(Endian::Big);
        put_reference(&mut e, None).unwrap();
        let bytes = e.finish().unwrap();
        let mut d = Decoder::new(&bytes, Endian::Big);
        assert_eq!(get_reference(&mut d).unwrap(), None);
    }

    /// §7.2.1 lets `_is_equivalent` say false about two references to the same
    /// object, so it confirms identity and never refutes it. Treating a false
    /// as proof of difference is the bug this documents.
    #[test]
    fn is_equivalent_confirms_but_cannot_refute() {
        let a = ior("h", 1, b"k");
        assert!(is_equivalent(&a, &a.clone()));
        assert!(!is_equivalent(&a, &ior("h", 1, b"other")));
        // Same object reached by two addresses: a legal false.
        assert!(!is_equivalent(&a, &ior("alias", 1, b"k")));
    }

    #[test]
    fn equivalent_references_hash_alike() {
        let a = ior("h", 1, b"k");
        let b = ior("h", 1, b"k");
        assert_eq!(reference_hash(&a, 0), reference_hash(&b, 0));
        assert_ne!(reference_hash(&a, 0), reference_hash(&ior("h", 1, b"j"), 0));
        assert!(reference_hash(&a, 16) < 16, "a maximum bounds the result");
    }

    // ── pseudo-operations ───────────────────────────────────────────────────

    fn registry_with_inheritance() -> Registry {
        let spec = orbweaver_idl::parse(
            "module m { interface A { long f(); }; interface B : A { long g(); }; };",
        )
        .unwrap();
        let mut r = Registry::new();
        r.load(&spec).unwrap();
        r
    }

    #[test]
    fn pseudo_operations_are_recognised() {
        for op in ["_is_a", "_non_existent", "_not_existent", "_is_equivalent", "_hash"] {
            assert!(ObjectOps::handles(op), "{op}");
        }
        assert!(!ObjectOps::handles("ping"));
    }

    /// `_is_a` is answered from the registry rather than the network, which is
    /// the point of §4.7: it works when the target is unreachable.
    #[test]
    fn is_a_is_answered_locally_including_the_object_base() {
        let reg = registry_with_inheritance();
        assert!(reg.is_a("IDL:m/B:1.0", "IDL:m/A:1.0"));
        assert!(reg.is_a("IDL:m/B:1.0", OBJECT_ID));
        assert!(!reg.is_a("IDL:m/A:1.0", "IDL:m/B:1.0"));
    }
}
