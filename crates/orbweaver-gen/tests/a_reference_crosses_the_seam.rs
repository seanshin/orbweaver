//! Whether a caller can tell that the servant which handed it a **reference**
//! was written in another language.
//!
//! `docs/decisions/D029-*.md` §6.1's Language row closed the construction leak
//! — a foreign servant can be a target — and §6.1.1 lists what is left. This
//! file is about the largest of those: **a foreign servant was a singleton
//! leaf.** Measured 2026-08-26, before `orbweaver_gen::seam` existed, over
//! `corpus/golden/16-object-refs.idl`:
//!
//! ```text
//! MINT  -> refused IDL:omg.org/CORBA/MARSHAL:1.0 completed=No
//! OID   -> call 0 = {"args":{"name":"x"},"id":"IDL:gc16/Registry:1.0","op":"lookup"}
//! OID   -> call 1 = {"args":{"name":"x"},"id":"IDL:gc16/Registry:1.0","op":"lookup"}
//! KNOWS a key from another home -> true
//! ```
//!
//! The two call documents above were addressed to **two different object
//! keys** and are identical. So a foreign servant could not tell which object
//! it was, claimed every key in the process, and could not answer with a
//! reference to anything — which is not a missing convenience. A servant that
//! cannot hand out a reference cannot participate in naming, in trading or in
//! any forward.
//!
//! # The shape of the measurement
//!
//! `corpus/golden/26-object-identity.idl`, which was written for the Rust
//! skeleton batch and holds exactly the three hazards: identity is per call
//! (`label`, `count` answer differently under different keys), a reference is
//! **minted** to another object of the same interface (`child`, `make_child`),
//! and a oneway still has an identity (`drop_binding`).
//!
//! Two servants for it. One is the generated Rust skeleton with a hand-written
//! `Tree` behind it. The other is [`ForeignServant`] with [`Mirror`] behind it,
//! which answers the AnyJSON documents a foreign runtime would send and holds
//! the same tree. **Both are handed byte-identical requests and their replies
//! are compared byte for byte**, over three GIOP versions, both byte orders and
//! every object in the tree.
//!
//! Comparing bytes rather than decoded values is deliberate and is not the
//! thing `CLAUDE.md` forbids: that rule is about a *foreign* peer whose CDR
//! padding the specification leaves undefined. Both encoders here are ours, so
//! any difference in the bytes is a difference a caller could observe — which
//! is what is being hunted. The minted references are *also* compared decoded,
//! by [`the_minted_reference_is_the_same_reference_decoded`], because bytes
//! being equal and an IOR meaning the same thing are two claims.

mod emitted;

use std::collections::BTreeMap;

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_dynamic::json::Json;
use orbweaver_gen::rt::{self, Dispatch, DispatchBody, ObjRef, ObjectHome, SystemException};
use orbweaver_gen::seam::{Answerer, ForeignServant, OWN_OBJECT_PREFIX, key_infix_of};
use orbweaver_giop::server::{Request, decode_request};
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, Ior, Version, encode_request, read_message};
use orbweaver_registry::{Contract, Registry, Strictness};

use emitted::f_26_object_identity::gc26::{
    DirectoryFault, DirectoryRefs, DirectoryServant, DirectorySkeleton, DirectoryTarget, NotBound,
};

const TYPE_ID: &str = "IDL:gc26/Directory:1.0";
const ROOT: &[u8] = b"dirs";
const HOST: &str = "127.0.0.1";
const PORT: u16 = 4711;
const VERSIONS: [Version; 3] = [Version::V1_0, Version::V1_1, Version::V1_2];
const ORDERS: [Endian; 2] = [Endian::Big, Endian::Little];

/// The tree both servants hold: oid → (label, leaf → child oid).
fn tree() -> BTreeMap<String, (String, BTreeMap<String, String>)> {
    let mut t = BTreeMap::new();
    t.insert(
        String::new(),
        ("root".to_owned(), BTreeMap::from([("docs".to_owned(), "n1".to_owned())])),
    );
    t.insert("n1".to_owned(), ("docs".to_owned(), BTreeMap::new()));
    t
}

// ── The Rust half: what an application author writes today ───────────────────

#[derive(Debug)]
struct Tree(BTreeMap<String, (String, BTreeMap<String, String>)>);

impl DirectoryServant for Tree {
    fn knows(&self, at: &DirectoryTarget<'_>) -> bool {
        self.0.contains_key(at.oid())
    }

    fn label(&mut self, at: &DirectoryTarget<'_>) -> Result<String, DirectoryFault> {
        Ok(self.node(at)?.0.clone())
    }

    fn count(&mut self, at: &DirectoryTarget<'_>) -> Result<i32, DirectoryFault> {
        Ok(self.node(at)?.1.len() as i32)
    }

    fn child(&mut self, at: &DirectoryTarget<'_>, leaf: String) -> Result<ObjRef, DirectoryFault> {
        match self.node(at)?.1.get(&leaf) {
            // One call, and the servant has a publishable reference to another
            // object of its own interface. No host, no port, no profile.
            Some(oid) => Ok(at.sibling(oid)),
            None => Err(DirectoryFault::NotBound(NotBound { missing: leaf })),
        }
    }

    fn make_child(
        &mut self,
        at: &DirectoryTarget<'_>,
        leaf: String,
    ) -> Result<ObjRef, DirectoryFault> {
        // Deterministic rather than counted, so the two servants mint the same
        // oid without sharing a counter: what is being compared is the seam,
        // not the servant's bookkeeping.
        let _ = self.node(at)?;
        Ok(at.sibling(&format!("{}+{leaf}", at.oid())))
    }

    fn drop_binding(
        &mut self,
        at: &DirectoryTarget<'_>,
        _leaf: String,
    ) -> Result<(), DirectoryFault> {
        let _ = self.node(at)?;
        Ok(())
    }
}

impl Tree {
    fn node(
        &self,
        at: &DirectoryTarget<'_>,
    ) -> Result<&(String, BTreeMap<String, String>), DirectoryFault> {
        self.0.get(at.oid()).ok_or_else(|| rt::raise::object_not_exist().did_not_run().into())
    }
}

// ── The foreign half: what a runtime in any language sends back ──────────────

/// A servant in some other language, answering AnyJSON documents.
///
/// Every answer below is written the way a foreign runtime writes one: values,
/// never bytes; `oid:<oid>` where a reference goes, never an address. It holds
/// the same tree as [`Tree`] and reads [`CALL_OBJECT`] to find its node — which
/// is the thing it could not do at all before, and the reason the two servants
/// can be compared at every object rather than only at the default one.
///
/// [`CALL_OBJECT`]: orbweaver_gen::seam::CALL_OBJECT
struct Mirror {
    tree: BTreeMap<String, (String, BTreeMap<String, String>)>,
    /// Every call document the seam put to it, for the identity assertions.
    seen: Vec<Json>,
}

impl Mirror {
    fn new() -> Self {
        Self { tree: tree(), seen: Vec::new() }
    }
}

fn obj(pairs: impl IntoIterator<Item = (&'static str, Json)>) -> Json {
    Json::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn ok(returns: Json) -> Json {
    obj([("ok", obj([("returns", returns), ("outputs", obj([]))]))])
}

/// A reference to an object this servant hosts, as the far side names one.
fn own(oid: &str) -> Json {
    obj([("_ref", Json::String(format!("{OWN_OBJECT_PREFIX}{oid}")))])
}

fn system(id: &str, completed: u32) -> Json {
    obj([(
        "system_exception",
        obj([
            ("id", Json::String(id.to_owned())),
            ("minor", Json::Number("0".to_owned())),
            ("completed", Json::Number(completed.to_string())),
        ]),
    )])
}

impl Answerer for Mirror {
    fn ask(&mut self, call: &Json) -> Result<Json, String> {
        self.seen.push(call.clone());
        let op = match call.get("op") {
            Some(Json::String(s)) => s.clone(),
            _ => return Err("no operation".into()),
        };
        let oid = match call.get("oid") {
            Some(Json::String(s)) => s.clone(),
            _ => return Err("no object".into()),
        };
        let leaf = match call.get("args").and_then(|a| a.get("leaf")) {
            Some(Json::String(s)) => s.clone(),
            _ => String::new(),
        };
        let Some((label, bindings)) = self.tree.get(&oid) else {
            // The same answer `Tree` gives for an oid it does not hold, spelled
            // the way a foreign runtime spells one: an id and an ordinal.
            return Ok(system("IDL:omg.org/CORBA/OBJECT_NOT_EXIST:1.0", 1));
        };
        Ok(match op.as_str() {
            "_get_label" => ok(Json::String(label.clone())),
            "count" => ok(Json::Number(bindings.len().to_string())),
            "child" => match bindings.get(&leaf) {
                Some(child) => ok(own(child)),
                None => obj([(
                    "user_exception",
                    obj([
                        ("id", Json::String("IDL:gc26/NotBound:1.0".to_owned())),
                        ("members", obj([("missing", Json::String(leaf))])),
                    ]),
                )]),
            },
            "make_child" => ok(own(&format!("{oid}+{leaf}"))),
            "drop_binding" => ok(Json::Null),
            other => return Err(format!("no such operation {other}")),
        })
    }
}

// ── Harness ──────────────────────────────────────────────────────────────────

fn registry() -> Registry {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/golden/26-object-identity.idl");
    let contract = Contract::load(&path, &Default::default(), Strictness::Checked)
        .expect("the corpus contract must load");
    let mut registry = Registry::new();
    registry.load(&contract.spec).expect("the contract must build a registry");
    registry
}

fn home() -> ObjectHome {
    ObjectHome::new(HOST, PORT, ROOT.to_vec())
}

fn rust_servant() -> DirectorySkeleton<Tree> {
    DirectorySkeleton::new(DirectoryRefs::new(home()), Tree(tree()))
}

fn foreign_servant() -> ForeignServant<Mirror> {
    ForeignServant::new(&registry(), TYPE_ID, Mirror::new())
        .expect("the contract names this interface")
        .with_home(home())
}

/// The key for one object, spelled the way the *generated* skeleton spells it.
fn key(oid: &str) -> Vec<u8> {
    DirectoryRefs::new(home()).key_of(oid)
}

fn request(
    version: Version,
    endian: Endian,
    key: &[u8],
    operation: &str,
    expect_reply: bool,
    args: impl FnOnce(&mut Encoder),
) -> Request {
    let wire = encode_request(version, endian, 1, key, operation, expect_reply, args)
        .expect("encode request");
    let mut cursor: &[u8] = &wire;
    let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("frame");
    decode_request(msg).expect("decode request")
}

type Answer = Result<(DispatchBody, Vec<u8>), SystemException>;

fn answer<D: Dispatch>(servant: &mut D, req: &Request, endian: Endian) -> Answer {
    // 24 is where a GIOP 1.2 request body starts; the origin is what makes the
    // reply's alignment a real test rather than a formality.
    let mut out = Encoder::continuing_at(endian, 24);
    match servant.dispatch_body(req, &mut out) {
        Ok(kind) => Ok((kind, out.finish().expect("finish"))),
        Err(e) => Err(e),
    }
}

fn render(a: &Answer) -> String {
    match a {
        Ok((kind, body)) => format!("{kind:?} {body:02x?}"),
        Err(e) => format!("{} minor={} completed={:?}", e.id, e.minor, e.completed),
    }
}

/// Every call the comparison makes, in order, against one pair of servants.
///
/// Ranged over **objects** as well as operations, which is the axis that could
/// not exist before: two calls to two different objects used to produce the
/// same call document.
fn script(version: Version, endian: Endian) -> Vec<(String, Request)> {
    let mut calls = Vec::new();
    for oid in ["", "n1", "absent"] {
        let k = key(oid);
        let at = |what: &str| format!("{what} at {oid:?}");
        calls.push((at("_get_label"), request(version, endian, &k, "_get_label", true, |_| {})));
        calls.push((at("count"), request(version, endian, &k, "count", true, |_| {})));
        calls.push((
            at("child bound"),
            request(version, endian, &k, "child", true, |e| e.put_str("docs")),
        ));
        calls.push((
            at("child unbound"),
            request(version, endian, &k, "child", true, |e| e.put_str("nothing")),
        ));
        calls.push((
            at("make_child"),
            request(version, endian, &k, "make_child", true, |e| e.put_str("new")),
        ));
        calls.push((
            at("drop_binding (oneway)"),
            request(version, endian, &k, "drop_binding", false, |e| e.put_str("docs")),
        ));
        calls.push((
            at("_is_a"),
            request(version, endian, &k, "_is_a", true, |e| e.put_str(TYPE_ID)),
        ));
    }
    calls
}

/// The transcript of one whole run, as strings a divergence can be read out of.
fn transcript(perturb: bool) -> Vec<(String, String, String)> {
    let mut rows = Vec::new();
    for version in VERSIONS {
        for endian in ORDERS {
            let mut rust = rust_servant();
            let mut foreign = foreign_servant();
            for (what, req) in script(version, endian) {
                let a = answer(&mut rust, &req, endian);
                let mut b = answer(&mut foreign, &req, endian);
                if perturb && what.starts_with("make_child") {
                    // The control: one answer moved, whichever kind it is.
                    // Both arms matter — for the object that does not exist
                    // `make_child` is a refusal and not a body, and a control
                    // that could only perturb bodies would have said nothing
                    // about whether the refusal path is compared at all.
                    match &mut b {
                        Ok((_, body)) => match body.first_mut() {
                            Some(first) => *first = first.wrapping_add(1),
                            None => body.push(0),
                        },
                        Err(e) => e.minor = e.minor.wrapping_add(1),
                    }
                }
                rows.push((format!("{version:?}/{endian:?} {what}"), render(&a), render(&b)));
            }
        }
    }
    rows
}

// ── The gate ─────────────────────────────────────────────────────────────────

/// **The gate.** A caller cannot tell which language served it, including for
/// the operations that answer with a reference.
///
/// Goes red if a new difference appears anywhere in the grid: 3 GIOP versions ×
/// 2 byte orders × 3 objects × 7 calls.
#[test]
fn every_operation_answers_identically_whichever_language_serves_it() {
    let rows = transcript(false);
    let divergent: Vec<&(String, String, String)> = rows.iter().filter(|r| r.1 != r.2).collect();
    assert!(
        divergent.is_empty(),
        "{} of {} calls answer differently depending on the servant's language:\n{}",
        divergent.len(),
        rows.len(),
        divergent
            .iter()
            .map(|(what, a, b)| format!("  {what}\n    rust    {a}\n    foreign {b}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    // A grid that shrank to nothing would also report no divergence.
    assert_eq!(rows.len(), 3 * 2 * 3 * 7, "the grid is the measurement");
}

/// The control for the gate above: perturb one answer and require it be seen.
///
/// Without this the gate is the shape `CLAUDE.md` calls green-while-measuring-
/// nothing — a comparison of two things that were never going to differ.
#[test]
fn the_comparison_can_see_a_difference() {
    let rows = transcript(true);
    let divergent = rows.iter().filter(|r| r.1 != r.2).count();
    assert_eq!(
        divergent,
        3 * 2 * 3,
        "one perturbed answer per (version, order, object) must be seen"
    );
}

// ── What the gate is made of, asserted separately ────────────────────────────

/// The reference a foreign servant mints is the reference a Rust one mints —
/// decoded, not only as bytes.
///
/// Bytes being equal and an IOR meaning the same thing are two claims, and the
/// second is the one a caller acts on: it dials the host and port and sends the
/// object key.
#[test]
fn the_minted_reference_is_the_same_reference_decoded() {
    for endian in ORDERS {
        let req = request(Version::V1_2, endian, &key(""), "child", true, |e| e.put_str("docs"));
        let from_rust = decode_objref(&answer(&mut rust_servant(), &req, endian), endian);
        let from_foreign = decode_objref(&answer(&mut foreign_servant(), &req, endian), endian);
        assert_eq!(from_rust, from_foreign, "{endian:?}");

        let ior = from_rust.expect("a bound child answers with a reference");
        assert_eq!(ior.type_id, TYPE_ID, "the repository id is the contract's");
        let profile = ior.profiles.first().expect("one IIOP profile");
        assert_eq!(profile.host, HOST);
        assert_eq!(profile.port, PORT);
        assert_eq!(profile.object_key, key("n1"), "the key the sibling is served under");
    }
}

/// The type a minted reference advertises comes from the **contract**, and the
/// far side cannot choose it.
///
/// A control as much as an assertion: a servant that could name the type could
/// name the wrong one, and a caller would narrow against it and dial something
/// that is not what it thinks. The far side names only an oid — there is no
/// slot in the seam's grammar for a repository id, which is what makes this
/// impossible rather than checked.
#[test]
fn the_far_side_cannot_choose_what_type_it_minted() {
    let req =
        request(Version::V1_2, Endian::Little, &key(""), "child", true, |e| e.put_str("docs"));
    let ior = decode_objref(&answer(&mut foreign_servant(), &req, Endian::Little), Endian::Little)
        .expect("a reference");
    assert_eq!(ior.type_id, TYPE_ID);
    // And it is the id the registry holds for the declared return type, not a
    // constant retyped here.
    assert!(registry().interface(&ior.type_id).is_some(), "the contract declares it");
}

/// A foreign servant is told which object it is, and it is told the same thing
/// the generated `DirectoryTarget::oid()` would have said.
#[test]
fn a_foreign_servant_is_told_which_object_it_is() {
    let mut foreign = foreign_servant();
    for oid in ["", "n1", "a/slash/and:colon"] {
        let req = request(Version::V1_2, Endian::Little, &key(oid), "count", true, |_| {});
        let _ = answer(&mut foreign, &req, Endian::Little);
    }
    let seen: Vec<String> = foreign
        .answerer()
        .seen
        .iter()
        .map(|c| match c.get("oid") {
            Some(Json::String(s)) => s.clone(),
            other => panic!("every call document carries an oid, got {other:?}"),
        })
        .collect();
    assert_eq!(seen, ["", "n1", "a/slash/and:colon"]);
}

/// A servant with no home is told the empty oid, and is told it always.
///
/// The key is never absent, so a far side has no rule to get wrong. Empty is
/// truthful: without a home a servant serves one object and cannot tell them
/// apart.
#[test]
fn a_homeless_servant_is_still_told_an_object_and_it_is_the_default_one() {
    let mut plain =
        ForeignServant::new(&registry(), TYPE_ID, Mirror::new()).expect("the interface");
    for k in [key(""), key("n1"), b"somebody-elses".to_vec()] {
        let req = request(Version::V1_2, Endian::Little, &k, "count", true, |_| {});
        let _ = answer(&mut plain, &req, Endian::Little);
    }
    for call in &plain.answerer().seen {
        assert_eq!(call.get("oid"), Some(&Json::String(String::new())));
    }
}

/// A foreign servant answers for its own home's keys and no others.
///
/// Before, it claimed every key in the process, so two servants behind one
/// `Servants` router would have been decided by insertion order.
#[test]
fn a_foreign_servant_answers_only_for_its_own_home() {
    let foreign = foreign_servant();
    assert!(foreign.knows(&key("")), "the default object");
    assert!(foreign.knows(&key("n1")), "a derived object");
    assert!(!foreign.knows(b"somebody-elses-key"), "a key this home did not derive");
    assert!(!foreign.knows(b""), "an empty key is not this home's root key");

    // The control: with no home the old behaviour is kept exactly, because
    // every deployment of this seam before homes existed relies on it.
    let plain = ForeignServant::new(&registry(), TYPE_ID, Mirror::new()).expect("the interface");
    assert!(plain.knows(b"somebody-elses-key"));
}

/// A servant with no home cannot mint, and says so as a refusal rather than by
/// inventing an address.
#[test]
fn without_a_home_there_is_nothing_to_mint_under() {
    let mut plain =
        ForeignServant::new(&registry(), TYPE_ID, Mirror::new()).expect("the interface");
    let req =
        request(Version::V1_2, Endian::Little, &key(""), "child", true, |e| e.put_str("docs"));
    let e = answer(&mut plain, &req, Endian::Little).expect_err("no home, no address");
    assert_eq!(e.id, "IDL:omg.org/CORBA/MARSHAL:1.0");
}

/// A handle nobody issued still cannot be turned into an address by guessing.
///
/// The minting prefix widened what a handle may name; this is the control that
/// it did not widen it to everything. `local-9` is the shape an *issued* handle
/// has, which is why it is the one worth forging.
#[test]
fn a_forged_handle_is_still_refused() {
    struct Forger(&'static str);
    impl Answerer for Forger {
        fn ask(&mut self, _call: &Json) -> Result<Json, String> {
            Ok(ok(obj([("_ref", Json::String(self.0.to_owned()))])))
        }
    }
    for forged in ["local-9", "n1", "", "oid", ":n1"] {
        let mut s = ForeignServant::new(&registry(), TYPE_ID, Forger(forged))
            .expect("the interface")
            .with_home(home());
        let req =
            request(Version::V1_2, Endian::Little, &key(""), "child", true, |e| e.put_str("docs"));
        let e = answer(&mut s, &req, Endian::Little)
            .expect_err("a handle nobody issued names no address");
        assert_eq!(e.id, "IDL:omg.org/CORBA/MARSHAL:1.0", "forged {forged:?}");
    }
}

/// The infix a foreign servant keys its objects under is the one the generated
/// skeleton bakes in — by calling the same function, so there is nothing to
/// hold equal for long.
///
/// This is what makes the two servants interchangeable behind one key space
/// rather than merely similar: a caller holding a reference minted by one can
/// be served by the other.
#[test]
fn one_spelling_of_the_key_scheme() {
    assert_eq!(key_infix_of(TYPE_ID), DirectoryRefs::KEY_INFIX);
    assert_eq!(foreign_servant().identity().expect("a home").own_infix(), DirectoryRefs::KEY_INFIX);
    // And every key the two derive is the same key, oids with separators
    // included — the oid is the whole remainder, so there is nothing to escape.
    let refs = DirectoryRefs::new(home());
    for oid in ["", "n1", "a/b", "a:b", DirectoryRefs::KEY_INFIX] {
        assert_eq!(refs.key_of(oid), key(oid));
        assert_eq!(
            foreign_servant().identity().expect("a home").oid_of(&refs.key_of(oid)),
            Some(oid)
        );
    }
}

// ── Reading a reference back out of a reply ──────────────────────────────────

fn decode_objref(a: &Answer, endian: Endian) -> Option<Ior> {
    let (kind, body) = a.as_ref().expect("a reply");
    assert!(matches!(kind, DispatchBody::Return), "expected a normal return");
    // `continuing_at(_, 24)` and 24 is 8-aligned, so offset 0 of the finished
    // body has the alignment the encoder wrote at — the origin rule, honoured
    // by not moving.
    let mut d = Decoder::new(body, endian);
    <ObjRef as rt::Cdr>::get(&mut d).expect("an object reference in the reply body").0
}
