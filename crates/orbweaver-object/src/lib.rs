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
pub mod residency;
pub mod tenant_service;

/// Repository id every CORBA object answers to.
pub const OBJECT_ID: &str = "IDL:omg.org/CORBA/Object:1.0";

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownIdPolicy {
    /// Raise `OBJECT_NOT_EXIST`.
    Reject,
    /// Ask the locator, which may activate one or forward the caller.
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
    pub fn new(name: &str, type_id: &str) -> Self {
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
                components: Vec::new(),
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
                components: Vec::new(),
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
