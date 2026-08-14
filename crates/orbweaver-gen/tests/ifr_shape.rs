//! The measurement: can a generated skeleton express a hand-written servant?
//!
//! `crates/orbweaver-registry/src/ifr.rs` is a hand-written `Dispatch` serving
//! the read-only Interface Repository. It is the best candidate of the five
//! hand-written servants in this workspace — read-only, so there is no state to
//! diverge, and its object-key scheme is already documented and reversible.
//!
//! This file replaces its **dispatch** with generated code and keeps its
//! **body** by hand. That is the claim being tested, stated precisely: bodies
//! are hand-written, dispatch is not. What has to disappear is the
//! `impl Dispatch`, the `match (target, operation)`, the key parsing, the
//! `Ior`/`IiopProfile` assembly and the `_is_a` id list — and it does. What is
//! written below is three servant traits' worth of method bodies, each of which
//! reads a registry and answers.
//!
//! `ifr.rs` itself is **not modified**; it is imported and driven as the
//! oracle. Every case in [`compared`] sends the same GIOP request to both
//! servants and requires the reply bytes to be identical — the §8 rule
//! ("static result equals dynamic result") applied to a different pair.
//!
//! # What the generated skeleton cannot express
//!
//! Named rather than skipped: [`NOT_COMPARED`] lists every case where the two
//! answer differently, with the cause, and
//! [`the_divergences_are_the_ones_named_and_they_still_diverge`] fails if one
//! of them quietly starts agreeing. A list of known gaps nobody re-measures is
//! a list of things that were once true.
//!
//! # The shape of the arrangement
//!
//! `ifr.rs` is one servant answering as three interfaces depending on the key.
//! Generated, that is three skeletons behind an `rt::Servants`, each with a
//! `knows` that claims a disjoint part of one shared key space —
//! `with_infix("/ifr/")`, the same infix `ifr.rs` uses, so the object keys and
//! therefore the minted references are byte-identical too.

mod emitted;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_gen::rt::{self, Dispatch, ObjRef, ObjectHome, Servants};
use orbweaver_giop::server::{Request, decode_request};
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, Version, encode_request, read_message};
use orbweaver_registry::{Entry, Registry, ifr};

use emitted::f_ir_subset::CORBA::{
    ContainedFault, ContainedRefs, ContainedServant, ContainedSkeleton, ContainedTarget,
    DefinitionKind, InterfaceDefFault, InterfaceDefRefs, InterfaceDefServant, InterfaceDefSkeleton,
    InterfaceDefTarget, RepositoryFault, RepositoryRefs, RepositoryServant, RepositorySkeleton,
    RepositoryTarget,
};

const ROOT: &[u8] = b"ifr-root";
const HOST: &str = "127.0.0.1";
const PORT: u16 = 4242;

/// The key space `ifr.rs` uses, adopted verbatim so the two servants mint the
/// same object keys and their `lookup_id` replies can be compared as bytes.
const INFIX: &str = "/ifr/";

/// The registry both servants describe.
const SUBJECT: &str = "
module bank {
  //@ ai_desc: An amount, in whole units and hundredths
  struct Money { long units; long cents; };
  //@ ai_desc: Which currency an amount is denominated in
  enum Currency { KRW, USD };
  //@ ai_desc: The withdrawal was refused
  exception Denied { string why; };
  //@ ai_desc: Anything that can hold an account
  interface Party {
    //@ ai_desc: The party's registered identifier
    //@ ai_effect: read_only
    readonly attribute string party_id;
  };
  //@ ai_desc: A bank account
  interface Account : Party {
    //@ ai_desc: What is in the account right now
    //@ ai_effect: read_only
    Money balance();
    //@ ai_desc: Takes an amount out of the account
    //@ ai_effect: destructive
    //@ ai_authz: bank.account.withdraw
    void withdraw(in Money amount) raises (Denied);
  };
};
";

const ACCOUNT: &str = "IDL:bank/Account:1.0";
const PARTY: &str = "IDL:bank/Party:1.0";
const MONEY: &str = "IDL:bank/Money:1.0";
const CURRENCY: &str = "IDL:bank/Currency:1.0";
const DENIED: &str = "IDL:bank/Denied:1.0";
const ABSENT: &str = "IDL:bank/Nope:1.0";

/// `create_module(in RepositoryId, in Identifier, in VersionSpec)` — three
/// arguments, and writing one made the generated skeleton answer `MARSHAL`
/// where the hand-written one answered `NO_PERMISSION`. That divergence was a
/// malformed request, not a defect; the real ordering difference it exposed is
/// the fifth entry of `NOT_COMPARED`.
const CREATE_ARGS: &[&str] = &["IDL:bank/New:1.0", "New", "1.0"];

fn registry() -> Registry {
    let spec = orbweaver_idl::parse(SUBJECT).expect("the subject IDL parses");
    let mut r = Registry::new();
    r.load(&spec).expect("loads");
    r
}

// ── The hand-written half: three servant bodies, no dispatch ─────────────────

/// The `Contained` triple `ifr.rs` computes, reproduced here.
///
/// `split_repository_id` alone is wrong under `#pragma prefix`, which reads
/// every leading segment as an enclosing module; the registry recorded the
/// qualified name when it loaded the IDL and the count of its segments is what
/// says how much of the path is prefix. Both helpers `ifr.rs` uses for the
/// fallback are public, so the fallback is shared rather than re-derived.
fn name_of(registry: &Registry, id: &str) -> String {
    match registry.qualified_name(id) {
        Some(q) => q.rsplit("::").next().unwrap_or(q).to_owned(),
        None => ifr::split_repository_id(id).0,
    }
}

fn absolute_name_of(registry: &Registry, id: &str) -> String {
    match registry.qualified_name(id) {
        Some(q) => format!("::{q}"),
        None => ifr::absolute_name(id),
    }
}

fn def_kind_of(registry: &Registry, id: &str) -> DefinitionKind {
    use orbweaver_giop::typecode::TypeCode;
    match registry.get(id) {
        Some(Entry::Interface(_)) => DefinitionKind::dk_Interface,
        Some(Entry::Const { .. }) => DefinitionKind::dk_Constant,
        Some(Entry::Type(tc)) => match tc {
            TypeCode::Struct { .. } => DefinitionKind::dk_Struct,
            TypeCode::Union { .. } => DefinitionKind::dk_Union,
            TypeCode::Enum { .. } => DefinitionKind::dk_Enum,
            TypeCode::Alias { .. } => DefinitionKind::dk_Alias,
            TypeCode::Except { .. } => DefinitionKind::dk_Exception,
            _ => DefinitionKind::dk_none,
        },
        None => DefinitionKind::dk_none,
    }
}

/// The `Repository` object: the root key, and nothing else.
struct RepositoryFacade {
    registry: Registry,
    /// For minting an `InterfaceDef` reference to an interface entry.
    defs: InterfaceDefRefs,
    /// For minting the weaker `Contained` reference to everything else, so a
    /// client that narrows locally narrows to something we actually serve.
    others: ContainedRefs,
}

impl RepositoryFacade {
    fn entry_reference(&self, id: &str) -> ObjRef {
        match self.registry.get(id) {
            None => InterfaceDefRefs::nil(),
            Some(Entry::Interface(_)) => self.defs.reference(id),
            Some(_) => self.others.reference(id),
        }
    }
}

impl RepositoryServant for RepositoryFacade {
    fn knows(&self, at: &RepositoryTarget<'_>) -> bool {
        at.is_default()
    }

    fn def_kind(&mut self, _at: &RepositoryTarget<'_>) -> Result<DefinitionKind, RepositoryFault> {
        Ok(DefinitionKind::dk_Repository)
    }

    fn lookup_id(
        &mut self,
        _at: &RepositoryTarget<'_>,
        search_id: String,
    ) -> Result<ObjRef, RepositoryFault> {
        Ok(self.entry_reference(&search_id))
    }

    /// The policy refusal, and the reason this operation is in the contract at
    /// all: `BAD_OPERATION` would say "no such operation", which invites a
    /// retry against another reference. `NO_PERMISSION` says the operation
    /// exists and the answer is no.
    fn create_module(
        &mut self,
        _at: &RepositoryTarget<'_>,
        _new_id: String,
        _new_name: String,
        _new_version: String,
    ) -> Result<ObjRef, RepositoryFault> {
        Err(rt::raise::other(ifr::NO_PERMISSION).did_not_run().into())
    }
}

/// An `InterfaceDef` per interface entry.
struct InterfaceDefFacade {
    registry: Registry,
    defs: InterfaceDefRefs,
}

impl InterfaceDefServant for InterfaceDefFacade {
    /// Every derived key naming a registered *interface*, and no other. This
    /// is what makes the three skeletons disjoint inside one key space.
    fn knows(&self, at: &InterfaceDefTarget<'_>) -> bool {
        !at.is_default() && self.registry.interface(at.oid()).is_some()
    }

    fn def_kind(
        &mut self,
        at: &InterfaceDefTarget<'_>,
    ) -> Result<DefinitionKind, InterfaceDefFault> {
        Ok(def_kind_of(&self.registry, at.oid()))
    }

    fn id(&mut self, at: &InterfaceDefTarget<'_>) -> Result<String, InterfaceDefFault> {
        Ok(at.oid().to_owned())
    }

    fn name(&mut self, at: &InterfaceDefTarget<'_>) -> Result<String, InterfaceDefFault> {
        Ok(name_of(&self.registry, at.oid()))
    }

    fn absolute_name(&mut self, at: &InterfaceDefTarget<'_>) -> Result<String, InterfaceDefFault> {
        Ok(absolute_name_of(&self.registry, at.oid()))
    }

    fn base_interfaces(
        &mut self,
        at: &InterfaceDefTarget<'_>,
    ) -> Result<Vec<ObjRef>, InterfaceDefFault> {
        let Some(iface) = self.registry.interface(at.oid()) else {
            return Err(rt::SystemException::bad_operation().into());
        };
        // `sibling` would do here — a base of an interface is another
        // `InterfaceDef` — but going through the same `defs` the repository
        // mints with keeps one answer for "what is a reference to an entry".
        Ok(iface.bases.iter().map(|b| self.defs.reference(b)).collect())
    }

    /// The IR operation, which is a different question from `_is_a`: "does the
    /// interface I describe derive from this id", not "is this reference one".
    fn is_a(
        &mut self,
        at: &InterfaceDefTarget<'_>,
        derived_from: String,
    ) -> Result<bool, InterfaceDefFault> {
        if self.registry.interface(at.oid()).is_none() {
            return Err(rt::SystemException::bad_operation().into());
        }
        Ok(self.registry.is_a(at.oid(), &derived_from))
    }

    fn create_module(
        &mut self,
        _at: &InterfaceDefTarget<'_>,
        _new_id: String,
        _new_name: String,
        _new_version: String,
    ) -> Result<ObjRef, InterfaceDefFault> {
        Err(rt::raise::other(ifr::NO_PERMISSION).did_not_run().into())
    }
}

/// A `Contained` per non-interface entry: structs, unions, enums, aliases,
/// exceptions and constants.
struct ContainedFacade {
    registry: Registry,
}

impl ContainedServant for ContainedFacade {
    fn knows(&self, at: &ContainedTarget<'_>) -> bool {
        !at.is_default()
            && self.registry.get(at.oid()).is_some()
            && self.registry.interface(at.oid()).is_none()
    }

    fn def_kind(&mut self, at: &ContainedTarget<'_>) -> Result<DefinitionKind, ContainedFault> {
        Ok(def_kind_of(&self.registry, at.oid()))
    }

    fn id(&mut self, at: &ContainedTarget<'_>) -> Result<String, ContainedFault> {
        Ok(at.oid().to_owned())
    }

    fn name(&mut self, at: &ContainedTarget<'_>) -> Result<String, ContainedFault> {
        Ok(name_of(&self.registry, at.oid()))
    }

    fn absolute_name(&mut self, at: &ContainedTarget<'_>) -> Result<String, ContainedFault> {
        Ok(absolute_name_of(&self.registry, at.oid()))
    }
}

// ── Harness ──────────────────────────────────────────────────────────────────

fn home() -> ObjectHome {
    ObjectHome::new(HOST, PORT, ROOT.to_vec())
}

/// The generated arrangement: three skeletons over one key space.
fn generated() -> Servants {
    let r = registry();
    let defs = InterfaceDefRefs::with_infix(home(), INFIX);
    let others = ContainedRefs::with_infix(home(), INFIX);
    Servants::new()
        .with(RepositorySkeleton::new(
            RepositoryRefs::with_infix(home(), INFIX),
            RepositoryFacade { registry: r.clone(), defs: defs.clone(), others: others.clone() },
        ))
        .with(InterfaceDefSkeleton::new(
            defs.clone(),
            InterfaceDefFacade { registry: r.clone(), defs },
        ))
        .with(ContainedSkeleton::new(others, ContainedFacade { registry: r }))
}

/// The oracle: the hand-written servant, unmodified.
fn hand_written() -> ifr::RepositoryServer {
    ifr::RepositoryServer::new(HOST, PORT, ROOT.to_vec(), registry())
}

fn entry_key(id: &str) -> Vec<u8> {
    hand_written().entry_key(id)
}

/// What a servant answered: bytes under a status, a system exception, or
/// "not my object".
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Body(rt::DispatchBody, Vec<u8>),
    Raised { id: String, minor: u32, completed: rt::Completion },
    Unknown,
}

fn request(key: &[u8], operation: &str, args: &[&str]) -> Request {
    let wire = encode_request(Version::V1_2, Endian::Big, 1, key, operation, true, |e| {
        for a in args {
            e.put_str(a);
        }
    })
    .expect("encode request");
    let mut cursor: &[u8] = &wire;
    let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("frame");
    decode_request(msg).expect("decode request")
}

/// Drives one servant exactly as `Server` would: `knows` first, then a body
/// written into an encoder at the origin a real reply occupies.
fn ask<D: Dispatch>(d: &mut D, key: &[u8], operation: &str, args: &[&str]) -> Answer {
    if !d.knows(key) {
        return Answer::Unknown;
    }
    let req = request(key, operation, args);
    let mut out = Encoder::continuing_at(Endian::Big, 24);
    match d.dispatch_body(&req, &mut out) {
        Ok(kind) => Answer::Body(kind, out.finish().expect("finish")),
        Err(ex) => Answer::Raised { id: ex.id, minor: ex.minor, completed: ex.completed },
    }
}

/// One case: an object key, an operation, and an argument.
struct Case {
    what: &'static str,
    key: Vec<u8>,
    op: &'static str,
    args: Vec<&'static str>,
}

fn case(what: &'static str, key: Vec<u8>, op: &'static str, args: &[&'static str]) -> Case {
    Case { what, key, op, args: args.to_vec() }
}

/// Every case the two servants must answer identically.
fn compared() -> Vec<Case> {
    let mut v = Vec::new();

    // ── the Repository object, at the root key ──
    for id in [
        ifr::REPOSITORY_ID,
        ifr::CONTAINER_ID,
        ifr::IR_OBJECT_ID,
        ifr::OBJECT_ID,
        ifr::INTERFACE_DEF_ID,
        ifr::CONTAINED_ID,
    ] {
        v.push(case("repository _is_a", ROOT.to_vec(), "_is_a", &[id]));
    }
    v.push(case("repository _non_existent", ROOT.to_vec(), "_non_existent", &[]));
    v.push(case("repository _get_def_kind", ROOT.to_vec(), "_get_def_kind", &[]));
    for id in [ACCOUNT, PARTY, MONEY, CURRENCY, DENIED, ABSENT] {
        v.push(case("lookup_id", ROOT.to_vec(), "lookup_id", &[id]));
    }
    v.push(case("repository create_module", ROOT.to_vec(), "create_module", CREATE_ARGS));

    // ── an InterfaceDef, at a derived key ──
    for id in [ACCOUNT, PARTY] {
        let key = entry_key(id);
        for want in [
            ifr::INTERFACE_DEF_ID,
            ifr::CONTAINER_ID,
            ifr::CONTAINED_ID,
            ifr::IDL_TYPE_ID,
            ifr::IR_OBJECT_ID,
            ifr::OBJECT_ID,
            ifr::REPOSITORY_ID,
        ] {
            v.push(case("interfacedef _is_a", key.clone(), "_is_a", &[want]));
        }
        v.push(case("interfacedef _non_existent", key.clone(), "_non_existent", &[]));
        for op in
            ["_get_def_kind", "_get_id", "_get_name", "_get_absolute_name", "_get_base_interfaces"]
        {
            v.push(case("interfacedef accessor", key.clone(), op, &[]));
        }
        for want in [ACCOUNT, PARTY, ABSENT, ifr::OBJECT_ID] {
            v.push(case("interfacedef is_a", key.clone(), "is_a", &[want]));
        }
        v.push(case("interfacedef create_module", key.clone(), "create_module", CREATE_ARGS));
        v.push(case("interfacedef unknown op", key.clone(), "no_such_operation", &[]));
    }

    // ── a Contained, at a derived key, for every non-interface entry kind ──
    for id in [MONEY, CURRENCY, DENIED] {
        let key = entry_key(id);
        v.push(case("contained _non_existent", key.clone(), "_non_existent", &[]));
        for op in ["_get_def_kind", "_get_id", "_get_name", "_get_absolute_name"] {
            v.push(case("contained accessor", key.clone(), op, &[]));
        }
    }

    // ── keys neither servant serves ──
    v.push(case("absent entry", entry_key(ABSENT), "_get_id", &[]));
    v.push(case("foreign key", b"somebody-elses".to_vec(), "_get_id", &[]));
    v.push(case("truncated root", b"ifr-roo".to_vec(), "_get_id", &[]));

    v
}

// ── The comparison ───────────────────────────────────────────────────────────

/// Every case in [`compared`], byte for byte, against `ifr.rs` itself.
#[test]
fn the_generated_skeleton_answers_what_the_hand_written_servant_answers() {
    let mut hand = hand_written();
    let mut from_idl = generated();
    let mut wrong = Vec::new();
    for c in compared() {
        let want = ask(&mut hand, &c.key, c.op, &c.args);
        let got = ask(&mut from_idl, &c.key, c.op, &c.args);
        if want != got {
            wrong.push(format!(
                "{} — {} {:?} on {:?}\n    hand-written: {want:?}\n    generated:    {got:?}",
                c.what,
                c.op,
                &c.args,
                String::from_utf8_lossy(&c.key)
            ));
        }
    }
    assert!(wrong.is_empty(), "{} of the cases differ:\n{}", wrong.len(), wrong.join("\n"));
}

/// The comparison would pass on two servants that both answered nothing, so
/// this is the negative control: the cases must actually exercise something.
#[test]
fn the_comparison_is_not_vacuous() {
    let cases = compared();
    // Pinned, not bounded: a matrix that silently shrinks is a comparison
    // that silently weakens, and ">= 60" would not have noticed.
    assert_eq!(cases.len(), 71, "the compared matrix changed size");
    let mut from_idl = generated();
    let (mut bodies, mut raised, mut unknown) = (0, 0, 0);
    let mut nonempty = 0;
    for c in &cases {
        match ask(&mut from_idl, &c.key, c.op, &c.args) {
            Answer::Body(_, b) => {
                bodies += 1;
                if !b.is_empty() {
                    nonempty += 1;
                }
            }
            Answer::Raised { .. } => raised += 1,
            Answer::Unknown => unknown += 1,
        }
    }
    assert!(bodies > 40, "{bodies} bodies");
    assert_eq!(bodies, nonempty, "every compared body carries a value");
    assert!(raised >= 4, "{raised} raises");
    assert_eq!(unknown, 3, "three keys are served by neither");

    // And the oracle must not be answering trivially either: a `lookup_id`
    // reply has to be a real reference with the address in it.
    let mut hand = hand_written();
    let Answer::Body(_, body) = ask(&mut hand, ROOT, "lookup_id", &[ACCOUNT]) else {
        panic!("lookup_id must answer with a body");
    };
    let mut d = rt::Decoder::new(&body, Endian::Big);
    let ior = orbweaver_giop::Ior::read_from(&mut d).expect("a reference");
    assert_eq!(ior.type_id, ifr::INTERFACE_DEF_ID);
    assert_eq!(ior.profiles[0].port, PORT);
    assert_eq!(ior.profiles[0].object_key, entry_key(ACCOUNT));
}

/// The generated arrangement reproduces the hand-written key scheme exactly —
/// which is why a minted reference can be compared as bytes at all.
#[test]
fn the_generated_key_space_is_the_hand_written_one() {
    let hand = hand_written();
    let defs = InterfaceDefRefs::with_infix(home(), INFIX);
    for id in [ACCOUNT, PARTY, MONEY, ABSENT] {
        assert_eq!(defs.key_of(id), hand.entry_key(id), "{id}");
        assert_eq!(defs.oid_of(&hand.entry_key(id)), Some(id), "{id}");
    }
    assert_eq!(defs.root_key(), hand.root_key());
    assert_eq!(defs.reference(ACCOUNT), ObjRef(Some(hand.entry_ior(ACCOUNT))));
    assert_eq!(
        ContainedRefs::with_infix(home(), INFIX).reference(MONEY),
        ObjRef(Some(hand.entry_ior(MONEY)))
    );
    assert_eq!(RepositoryRefs::with_infix(home(), INFIX).ior(""), hand.root_ior());
}

/// The three generated skeletons claim disjoint parts of one key space, which
/// is the only thing that makes `rt::Servants` deterministic here.
#[test]
fn the_three_skeletons_claim_disjoint_parts_of_one_key_space() {
    let r = registry();
    let defs = InterfaceDefRefs::with_infix(home(), INFIX);
    let others = ContainedRefs::with_infix(home(), INFIX);
    let repo = RepositorySkeleton::new(
        RepositoryRefs::with_infix(home(), INFIX),
        RepositoryFacade { registry: r.clone(), defs: defs.clone(), others: others.clone() },
    );
    let idef =
        InterfaceDefSkeleton::new(defs.clone(), InterfaceDefFacade { registry: r.clone(), defs });
    let cont = ContainedSkeleton::new(others, ContainedFacade { registry: r });

    let mut keys = vec![ROOT.to_vec()];
    keys.extend([ACCOUNT, PARTY, MONEY, CURRENCY, DENIED, ABSENT].map(entry_key));
    keys.push(b"foreign".to_vec());
    for key in keys {
        let claims = [repo.knows(&key), idef.knows(&key), cont.knows(&key)];
        let n = claims.iter().filter(|c| **c).count();
        assert!(n <= 1, "{:?} is claimed by {claims:?}", String::from_utf8_lossy(&key));
    }
}

// ── What is still hand-written, and why ──────────────────────────────────────

/// Where the generated skeleton and `ifr.rs` answer differently, with the cause.
///
/// Each entry is a `(what, why)` pair, and every one of them is re-measured by
/// [`the_divergences_are_the_ones_named_and_they_still_diverge`]. Two are
/// contract-level (a generated skeleton can only be as expressive as the IDL);
/// two are the same root cause, which is worth saying once:
///
/// **`ifr.rs` varies the identity of an object with the object, inside one key
/// space.** A key under `/ifr/` is an `InterfaceDef` or a `Contained` or a
/// `Contained`-plus-`IDLType` depending on what the registry holds under it,
/// and one hand-written `match` covers all three. A generated skeleton is per
/// interface: it can hold disjoint parts of a key space (that is what this file
/// does) but each part answers one fixed `_is_a` list and serves one fixed
/// operation set. Reproducing `ifr.rs`'s per-entry-kind `_is_a` exactly would
/// need an interface per entry kind in the contract, which the IR IDL does not
/// have.
const NOT_COMPARED: [(&str, &str); 5] = [
    (
        "describe_interface",
        "the operation the whole facade exists for, and it is not in the contract: its \
         reply carries `CORBA::TypeCode` members, and this workspace's registry loads \
         `::CORBA::TypeCode` as `void` (measured: an operation declared to return one \
         generates `-> ()`). Generating it would produce a silently empty reply, so the \
         corpus file leaves it out. The cause is in `orbweaver-registry`, not in the \
         generator.",
    ),
    (
        "_is_a on a non-interface entry",
        "`ifr.rs` answers IDLType true for a struct/enum/alias entry and false for a \
         constant, from one key space. The generated `Contained` skeleton answers one \
         fixed inheritance chain for every key it claims. Same root cause as the next.",
    ),
    (
        "create_module on a non-interface entry",
        "`ifr.rs` refuses every mutating operation with NO_PERMISSION on every key, by \
         matching the operation *name*. A generated skeleton answers BAD_OPERATION for an \
         operation the addressed interface does not declare — and `Contained` does not \
         derive from `Container`. The refusal is expressible (see `create_module` on the \
         Repository and InterfaceDef keys, which is compared and agrees); what is not \
         expressible is refusing an operation the contract never declared.",
    ),
    (
        "_set_ on a readonly attribute",
        "`ifr.rs` refuses `_set_id` with NO_PERMISSION by name-shape. A generated skeleton \
         emits no setter arm for a readonly attribute, so it answers BAD_OPERATION — which \
         is the more defensible answer, since the IR IDL declares no such operation either. \
         Recorded as a divergence rather than a defect on either side.",
    ),
    (
        "a malformed body under a refused operation",
        "`ifr.rs` refuses a mutating operation *before* it touches the argument body, so a \
         `create_module` with the wrong arguments is NO_PERMISSION. A generated skeleton \
         decodes the arguments the contract declares and then calls the servant, so the \
         same request is MARSHAL — the servant never runs and never gets to refuse. This \
         one cost the batch a round: the first comparison sent one string to a \
         three-argument operation and read the difference as a defect. Ordering a refusal \
         ahead of decoding is a policy the contract cannot state; a servant that needs it \
         must override `forward`-style, at the `Dispatch` level, or accept MARSHAL for a \
         request that was malformed anyway.",
    ),
];

/// Every named divergence must still be one.
///
/// A list of known gaps that nobody re-measures is a list of things that were
/// once true. This drives every measurable one and requires it to disagree; the
/// first is a contract-level absence, so its *cause* is measured instead.
#[test]
fn the_divergences_are_the_ones_named_and_they_still_diverge() {
    assert_eq!(NOT_COMPARED.len(), 5);
    let mut hand = hand_written();
    let mut from_idl = generated();

    // 1 — describe_interface is not in the generated contract at all.
    let key = entry_key(ACCOUNT);
    assert!(
        matches!(ask(&mut hand, &key, "describe_interface", &[]), Answer::Body(..)),
        "the hand-written servant serves it"
    );
    match ask(&mut from_idl, &key, "describe_interface", &[]) {
        Answer::Raised { id, .. } => assert_eq!(id, rt::BAD_OPERATION),
        other => panic!("the generated skeleton must not pretend to serve it: {other:?}"),
    }
    // And the cause, measured rather than asserted from a comment: the
    // registry loads `::CORBA::TypeCode` as `void`, so an operation declared to
    // return one would generate `-> ()` and reply with nothing at all. That is
    // an `orbweaver-registry` defect; what belongs here is the evidence for why
    // the contract leaves the operation out rather than generating it wrong.
    let probe = orbweaver_idl::parse(
        "module probe { interface Described { ::CORBA::TypeCode shape(); }; };",
    )
    .expect("parses");
    let mut pr = Registry::new();
    pr.load(&probe).expect("loads");
    let sig = pr
        .interface("IDL:probe/Described:1.0")
        .and_then(|i| i.operations.get("shape"))
        .expect("the probe operation");
    assert!(
        matches!(sig.returns, orbweaver_giop::typecode::TypeCode::Void),
        "measured 2026-08-14: the registry loads ::CORBA::TypeCode as {:?}, not a TypeCode. \
         If this now holds a TypeCode, `describe_interface` has become generatable and this \
         entry of NOT_COMPARED should go.",
        sig.returns
    );

    // 2 and 3 — a non-interface entry.
    let money = entry_key(MONEY);
    assert_ne!(
        ask(&mut hand, &money, "_is_a", &[ifr::IDL_TYPE_ID]),
        ask(&mut from_idl, &money, "_is_a", &[ifr::IDL_TYPE_ID]),
    );
    assert_ne!(
        ask(&mut hand, &money, "create_module", CREATE_ARGS),
        ask(&mut from_idl, &money, "create_module", CREATE_ARGS),
    );

    // 4 — the setter of a readonly attribute.
    let (a, b) = (
        ask(&mut hand, &key, "_set_id", &[ABSENT]),
        ask(&mut from_idl, &key, "_set_id", &[ABSENT]),
    );
    match (a, b) {
        (Answer::Raised { id: a, .. }, Answer::Raised { id: b, .. }) => {
            assert_eq!(a, ifr::NO_PERMISSION);
            assert_eq!(b, rt::BAD_OPERATION);
        }
        other => panic!("both must refuse, differently: {other:?}"),
    }

    // 5 — a refused operation whose body does not match the contract. The
    // round this batch spent on it is the reason it is written down.
    let short = [ABSENT];
    match (
        ask(&mut hand, ROOT, "create_module", &short),
        ask(&mut from_idl, ROOT, "create_module", &short),
    ) {
        (Answer::Raised { id: a, .. }, Answer::Raised { id: b, .. }) => {
            assert_eq!(a, ifr::NO_PERMISSION, "refused before the body is read");
            assert_eq!(b, rt::MARSHAL, "the arguments are decoded before the servant is called");
        }
        other => panic!("both must refuse, differently: {other:?}"),
    }
    // With a well-formed body the two agree, which is what makes this an
    // ordering difference rather than a missing refusal.
    assert_eq!(
        ask(&mut hand, ROOT, "create_module", CREATE_ARGS),
        ask(&mut from_idl, ROOT, "create_module", CREATE_ARGS),
    );
}
