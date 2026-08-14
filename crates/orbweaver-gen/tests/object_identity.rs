//! Many objects of one interface, from one generated skeleton.
//!
//! Until this batch a generated skeleton had no object-key scheme, so it
//! answered every key identically and one process could serve exactly one
//! object. Not one of the five hand-written servants in this workspace works
//! that way: `naming_server.rs` holds a context per key, `ifr.rs` an
//! `InterfaceDef` per repository id, `tenant_service.rs` a factory per tenant.
//! A generated skeleton that cannot do that is one that can never replace them.
//!
//! `corpus/golden/26-object-identity.idl` is the contract. The servant *body*
//! below is hand-written — that is the point, bodies are hand-written — and
//! what is not hand-written is any of the dispatch: no `impl Dispatch`, no
//! `knows` over raw bytes, no `Ior` assembled from an `IiopProfile`.
//!
//! What each test is for:
//!
//! * [`the_key_scheme_is_reversible_and_has_no_escaping_to_get_wrong`] — the
//!   derivation, including the oids that would break a scheme that split on a
//!   separator instead of stripping a prefix;
//! * [`two_interfaces_sharing_a_root_cannot_read_each_others_keys`];
//! * [`one_process_serves_many_directories_over_real_giop`] — the claim
//!   itself, with a minted reference dialled back;
//! * [`an_unknown_object_is_refused_before_the_operation_is_looked_at`];
//! * [`a_moved_object_answers_with_a_location_forward`] — the seam that used
//!   to be a silent `None`;
//! * [`the_generated_map_adapter_serves_a_value_per_object`];
//! * [`servants_routes_between_two_generated_skeletons_by_key`].

mod emitted;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_gen::rt::{self, Dispatch, ObjRef, ObjectHome, Servants, Server};
use orbweaver_giop::server::{Request, decode_request};
use orbweaver_giop::{
    Connection, DEFAULT_MAX_MESSAGE_SIZE, Error as GiopError, Ior, Version, encode_request,
    read_message,
};

use emitted::f_24_skeleton_surface::gc24::{GaugeRefs, GaugeSkeleton};
use emitted::f_26_object_identity::gc26::{
    DirectoryClient, DirectoryFault, DirectoryObject, DirectoryObjects, DirectoryRefs,
    DirectoryServant, DirectorySkeleton, DirectoryTarget, NotBound,
};

const ROOT: &[u8] = b"dirsvc";
const TYPE_ID: &str = "IDL:gc26/Directory:1.0";
const VERSIONS: [Version; 3] = [Version::V1_0, Version::V1_1, Version::V1_2];

// ── The hand-written half: what an application author writes ─────────────────

/// One node of the tree.
#[derive(Debug, Default, Clone)]
struct Node {
    label: String,
    bindings: BTreeMap<String, String>,
}

/// Every node in one servant, keyed by oid.
///
/// Nothing here mentions GIOP, an object key, an `Ior` or an operation name.
/// The only thing it does that a single-object servant would not is read
/// `__at.oid()` — which is the whole feature.
#[derive(Debug, Default)]
struct Tree {
    nodes: BTreeMap<String, Node>,
    minted: u64,
    /// oid → the oid it moved to. See the forward test.
    moved: BTreeMap<String, String>,
}

impl Tree {
    /// A tree with a root node under the default (empty) oid.
    fn new() -> Self {
        let mut t = Self::default();
        t.nodes.insert(String::new(), Node { label: "root".into(), ..Node::default() });
        t
    }

    fn node(&self, at: &DirectoryTarget<'_>) -> Result<&Node, DirectoryFault> {
        self.nodes.get(at.oid()).ok_or_else(|| rt::raise::object_not_exist().did_not_run().into())
    }

    fn node_mut(&mut self, at: &DirectoryTarget<'_>) -> Result<&mut Node, DirectoryFault> {
        self.nodes
            .get_mut(at.oid())
            .ok_or_else(|| rt::raise::object_not_exist().did_not_run().into())
    }
}

impl DirectoryServant for Tree {
    /// A moved node is still *known* — deliberately.
    ///
    /// `Server` asks `knows` first and answers `OBJECT_NOT_EXIST` before it
    /// ever asks `forward`, so a servant that forgets a forwarded object the
    /// moment it moves can never forward anything. This is the one ordering
    /// rule a generated skeleton cannot enforce for you.
    fn knows(&self, at: &DirectoryTarget<'_>) -> bool {
        self.nodes.contains_key(at.oid()) || self.moved.contains_key(at.oid())
    }

    fn forward(&mut self, at: &DirectoryTarget<'_>) -> Option<Ior> {
        let to = self.moved.get(at.oid())?;
        Some(at.refs().ior(to))
    }

    fn label(&mut self, at: &DirectoryTarget<'_>) -> Result<String, DirectoryFault> {
        Ok(self.node(at)?.label.clone())
    }

    fn count(&mut self, at: &DirectoryTarget<'_>) -> Result<i32, DirectoryFault> {
        Ok(self.node(at)?.bindings.len() as i32)
    }

    fn child(&mut self, at: &DirectoryTarget<'_>, leaf: String) -> Result<ObjRef, DirectoryFault> {
        match self.node(at)?.bindings.get(&leaf) {
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
        if let Some(oid) = self.node(at)?.bindings.get(&leaf) {
            return Ok(at.sibling(oid));
        }
        self.minted += 1;
        let oid = format!("n{}", self.minted);
        self.nodes.insert(oid.clone(), Node { label: leaf.clone(), ..Node::default() });
        self.node_mut(at)?.bindings.insert(leaf, oid.clone());
        Ok(at.sibling(&oid))
    }

    fn drop_binding(
        &mut self,
        at: &DirectoryTarget<'_>,
        leaf: String,
    ) -> Result<(), DirectoryFault> {
        self.node_mut(at)?.bindings.remove(&leaf);
        Ok(())
    }
}

// ── Harness ──────────────────────────────────────────────────────────────────

fn home_at(port: u16) -> ObjectHome {
    ObjectHome::new("127.0.0.1", port, ROOT.to_vec())
}

fn refs_at(port: u16) -> DirectoryRefs {
    DirectoryRefs::new(home_at(port))
}

/// Runs `f` against a live server whose dispatcher is the generated skeleton.
fn with_server<F: FnOnce(&Ior)>(tree: Tree, f: F) {
    let server = Server::bind("127.0.0.1:0", ROOT.to_vec()).expect("bind");
    let addr = server.local_addr().expect("addr");
    let ior = server.ior(TYPE_ID, "127.0.0.1").expect("ior");
    let home = ObjectHome::of(&server, "127.0.0.1").expect("home");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let t = std::thread::spawn(move || {
        let mut skeleton = DirectorySkeleton::new(DirectoryRefs::new(home), tree);
        server.serve(&mut skeleton, || flag.load(Ordering::SeqCst)).expect("serve");
    });

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&ior)));

    stop.store(true, Ordering::SeqCst);
    let _ = std::net::TcpStream::connect(addr); // wake the accept loop
    t.join().expect("the server thread must not panic");
    if let Err(p) = outcome {
        std::panic::resume_unwind(p);
    }
}

fn connect(ior: &Ior, version: Version, endian: Endian) -> Connection {
    let mut conn = Connection::connect(ior, Duration::from_secs(5)).expect("connect");
    conn.cap_version(version);
    conn.set_endian(endian);
    conn
}

/// One decoded `Request` addressed to `key`, built without a socket.
fn request<F: FnOnce(&mut Encoder)>(key: &[u8], operation: &str, args: F) -> Request {
    let wire = encode_request(Version::V1_2, Endian::Big, 1, key, operation, true, args)
        .expect("encode request");
    let mut cursor: &[u8] = &wire;
    let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("frame");
    decode_request(msg).expect("decode request")
}

// ── The key scheme ───────────────────────────────────────────────────────────

/// Reversible, and reversible for oids a naive scheme would mangle.
///
/// The oid is the *whole* remainder after a fixed prefix, never a field in a
/// split. That is why `a/b`, `IDL:m/I:1.0` and an oid containing the infix
/// itself all round-trip: there is no escaping, so there is no escaping to get
/// wrong. `ifr.rs` derives its keys the same way and for the same reason —
/// a table of minted keys stops working across a restart and grows without
/// bound as clients look things up.
#[test]
fn the_key_scheme_is_reversible_and_has_no_escaping_to_get_wrong() {
    let refs = refs_at(9999);
    assert_eq!(DirectoryRefs::KEY_INFIX, "/Directory/");
    assert_eq!(DirectoryRefs::TYPE_ID, TYPE_ID);

    // The default object is the bare root key: what `Server::bind` was given
    // and what `Server::ior` publishes.
    assert_eq!(refs.key_of(""), ROOT.to_vec());
    assert_eq!(refs.oid_of(ROOT), Some(""));

    for oid in ["n1", "a/b", "IDL:m/I:1.0", "/Directory/", "한글", "  ", "x".repeat(300).as_str()]
    {
        let key = refs.key_of(oid);
        assert_eq!(refs.oid_of(&key), Some(oid), "{oid:?} did not survive the round trip");
        assert!(key.starts_with(ROOT), "{oid:?}");
    }

    // Keys this home did not derive are not ours, whatever they look like.
    for foreign in [&b"other"[..], b"dirsv", b"dirsvcX", b"dirsvc/Gauge/n1"] {
        assert_eq!(refs.oid_of(foreign), None, "{foreign:?}");
    }
    // Not valid UTF-8 after the infix: not an oid, so not ours.
    let mut bad = refs.key_of("");
    bad.extend_from_slice(DirectoryRefs::KEY_INFIX.as_bytes());
    bad.push(0xFF);
    assert_eq!(refs.oid_of(&bad), None);
}

/// The infix is the interface name, so two skeletons over one root key are
/// separated by construction rather than by convention. The prefix is matched
/// at a fixed offset, which is what makes it impossible for an oid under one
/// interface to be read as a key under another.
#[test]
fn two_interfaces_sharing_a_root_cannot_read_each_others_keys() {
    let dirs = refs_at(1);
    let gauges = GaugeRefs::new(home_at(1));
    assert_ne!(DirectoryRefs::KEY_INFIX, GaugeRefs::KEY_INFIX);

    for oid in ["n1", "/Gauge/n1", "x"] {
        assert_eq!(gauges.oid_of(&dirs.key_of(oid)), None, "{oid:?} leaked into Gauge");
        assert_eq!(dirs.oid_of(&gauges.key_of(oid)), None, "{oid:?} leaked into Directory");
    }
    // The root is the one key they do share, and it is the same object in both
    // key spaces — which is why `knows` and not the scheme is what separates
    // two skeletons that were deliberately given one root.
    assert_eq!(dirs.oid_of(gauges.root_key()), Some(""));
}

// ── The claim ────────────────────────────────────────────────────────────────

/// One process, one skeleton, many objects — over real GIOP, every version and
/// both byte orders.
///
/// The load-bearing assertion is not that the calls succeed: it is that
/// `label` and `count` answer *differently* under different keys. A skeleton
/// that ignored the object key would pass every compile check, serve every
/// request, and answer the root's label to every node.
#[test]
fn one_process_serves_many_directories_over_real_giop() {
    with_server(Tree::new(), |ior| {
        for version in VERSIONS {
            for endian in [Endian::Big, Endian::Little] {
                let what = format!("{version} {endian:?}");
                let mut root = DirectoryClient::new(connect(ior, version, endian));
                assert_eq!(root.label().expect("label"), "root", "{what}");

                let leaf = format!("kid-{version}-{endian:?}");
                let minted = root.make_child(leaf.clone()).expect("make_child");
                let child_ior = minted.0.clone().expect("a minted reference is not nil");
                assert_eq!(child_ior.type_id, TYPE_ID, "{what}");

                // The minted reference is dialable: it carries a real IIOP
                // profile, and the object it names is a *different* object.
                let mut child = DirectoryClient::new(connect(&child_ior, version, endian));
                assert_eq!(child.label().expect("label"), leaf, "{what}");
                assert_eq!(child.count().expect("count"), 0, "{what}");

                // And the same reference comes back through `child`.
                let again = root.child(leaf.clone()).expect("child");
                assert_eq!(again, minted, "{what}");

                // A oneway still knows which node it was called on.
                child.make_child("gone".into()).expect("make_child");
                assert_eq!(child.count().expect("count"), 1, "{what}");
                child.drop_binding("gone".into()).expect("drop_binding");
                assert_eq!(child.count().expect("count"), 0, "{what}");
                assert!(root.count().expect("count") > 0, "{what}");

                // A user exception still travels, per object.
                match child.child("absent".into()) {
                    Err(GiopError::UserException { id, .. }) => {
                        assert_eq!(id, "IDL:gc26/NotBound:1.0", "{what}");
                    }
                    other => panic!("{what}: expected NotBound, got {other:?}"),
                }
            }
        }
    });
}

/// A key the servant does not hold is `OBJECT_NOT_EXIST`, and the operation is
/// never reached. Both gates are generated: the scheme decides whether the key
/// is ours at all, `knows` decides whether the object is there.
#[test]
fn an_unknown_object_is_refused_before_the_operation_is_looked_at() {
    let refs = refs_at(1);
    let mut skeleton = DirectorySkeleton::new(refs.clone(), Tree::new());

    assert!(skeleton.knows(ROOT), "the default object is there");
    assert!(!skeleton.knows(&refs.key_of("nope")), "no such node");
    assert!(!skeleton.knows(b"someone-elses-key"), "not our key space at all");

    // Driven directly, past the server's own gate, the answer is the same one
    // `Server` would have given.
    let req = request(&refs.key_of("nope"), "count", |_| {});
    let mut out = Encoder::continuing_at(Endian::Big, 24);
    let err = skeleton.dispatch_body(&req, &mut out).expect_err("no such object");
    assert_eq!(err.id, rt::OBJECT_NOT_EXIST);

    // A live server refuses it too, and refuses it the same way for a
    // `LocateRequest` — the two cannot disagree, because both ask `knows`.
    with_server(Tree::new(), |ior| {
        let mut absent = ior.clone();
        absent.profiles[0].object_key = refs.key_of("nope");
        let mut client = DirectoryClient::new(connect(&absent, Version::V1_2, Endian::Big));
        match client.count() {
            Err(GiopError::SystemException { id, .. }) => assert_eq!(id, rt::OBJECT_NOT_EXIST),
            other => panic!("expected OBJECT_NOT_EXIST, got {other:?}"),
        }
    });
}

// ── LOCATION_FORWARD ─────────────────────────────────────────────────────────

/// The seam that used to be a generated `None`.
///
/// Everything mechanical is generated: the key is decoded, the identity is
/// handed to the servant, and a returned reference becomes a
/// `LOCATION_FORWARD` reply instead of an invocation. The decision — *when* to
/// redirect and *where to* — is the servant's, because no clause of any IDL
/// contract describes where an object lives.
///
/// Two limits are asserted here because they are the ones that surprise:
/// a moved object must still be `knows`n or `Server` refuses it before asking,
/// and the client follows the forward transparently (§9.4.3.2), so the proof
/// that it happened is that a call on the *old* reference is answered by the
/// *new* object.
#[test]
fn a_moved_object_answers_with_a_location_forward() {
    let mut tree = Tree::new();
    tree.nodes.insert("new".into(), Node { label: "relocated".into(), ..Node::default() });
    tree.moved.insert("old".into(), "new".into());

    // At the dispatcher, first: the generated `forward` decodes the key and
    // hands back exactly the reference the servant minted.
    let refs = refs_at(4321);
    let mut skeleton = DirectorySkeleton::new(
        refs.clone(),
        Tree { moved: BTreeMap::from([("old".to_owned(), "new".to_owned())]), ..Tree::new() },
    );
    let to = skeleton
        .forward(&request(&refs.key_of("old"), "count", |_| {}))
        .expect("a moved object forwards");
    assert_eq!(to.type_id, TYPE_ID);
    assert_eq!(to.profiles[0].object_key, refs.key_of("new"));
    assert_eq!(to.profiles[0].port, 4321, "the published port, not the bind port");
    // An object that has not moved does not forward, and neither does a key
    // that is not ours.
    assert!(skeleton.forward(&request(ROOT, "count", |_| {})).is_none());
    assert!(skeleton.forward(&request(b"foreign", "count", |_| {})).is_none());

    // Then over the wire, where §9.4.3.2 says the client must not notice.
    with_server(tree, |ior| {
        let mut old = ior.clone();
        old.profiles[0].object_key =
            DirectoryRefs::new(ObjectHome::new("127.0.0.1", ior.profiles[0].port, ROOT.to_vec()))
                .key_of("old");
        let mut client = DirectoryClient::new(connect(&old, Version::V1_2, Endian::Big));
        assert_eq!(
            client.label().expect("the forward is followed"),
            "relocated",
            "the old reference must be answered by the new object"
        );
    });
}

// ── The other shape ──────────────────────────────────────────────────────────

/// One value per object, no identity argument: the shape most people reach for
/// first. It is generated *in terms of* the identity-taking trait rather than
/// instead of it, so both are available and neither is the odd one out.
#[test]
fn the_generated_map_adapter_serves_a_value_per_object() {
    /// A leaf that holds nothing and knows only its own name.
    struct Leaf(String);

    impl DirectoryObject for Leaf {
        fn label(&mut self) -> Result<String, DirectoryFault> {
            Ok(self.0.clone())
        }
        fn count(&mut self) -> Result<i32, DirectoryFault> {
            Ok(0)
        }
        fn child(&mut self, leaf: String) -> Result<ObjRef, DirectoryFault> {
            Err(DirectoryFault::NotBound(NotBound { missing: leaf }))
        }
        fn make_child(&mut self, _leaf: String) -> Result<ObjRef, DirectoryFault> {
            Err(rt::raise::no_permission().did_not_run().into())
        }
        fn drop_binding(&mut self, _leaf: String) -> Result<(), DirectoryFault> {
            Ok(())
        }
    }

    let mut objects: DirectoryObjects<Leaf> = DirectoryObjects::new();
    assert!(objects.is_empty());
    objects.insert("", Leaf("default".into()));
    objects.insert("a", Leaf("alpha".into()));
    objects.insert("b", Leaf("beta".into()));
    assert_eq!(objects.len(), 3);
    assert_eq!(objects.oids().collect::<Vec<_>>(), ["", "a", "b"]);

    let refs = refs_at(1);
    let mut skeleton = DirectorySkeleton::new(refs.clone(), objects);
    for (oid, want) in [("", "default"), ("a", "alpha"), ("b", "beta")] {
        assert!(skeleton.knows(&refs.key_of(oid)), "{oid:?}");
        let req = request(&refs.key_of(oid), "_get_label", |_| {});
        let mut out = Encoder::continuing_at(Endian::Big, 24);
        skeleton.dispatch_body(&req, &mut out).expect("dispatch");
        let body = out.finish().expect("finish");
        let mut d = rt::Decoder::new(&body, Endian::Big);
        assert_eq!(d.get_string().expect("label"), want, "{oid:?}");
    }

    // Removing an object retires its key, which is what `retire` means in
    // `tenant_service.rs` and what `unbind` means in the naming server.
    assert!(skeleton.servant.remove("a").is_some());
    assert!(!skeleton.knows(&refs.key_of("a")));
    let req = request(&refs.key_of("a"), "_get_label", |_| {});
    let mut out = Encoder::continuing_at(Endian::Big, 24);
    let err = skeleton.dispatch_body(&req, &mut out).expect_err("retired");
    assert_eq!(err.id, rt::OBJECT_NOT_EXIST);
    assert_eq!(err.completed, rt::Completion::No, "nothing ran, so a retry elsewhere is safe");
}

/// Two interfaces, one process, one `Server`: the arrangement `ifr.rs` and
/// `tenant_service.rs` both need and no single generated skeleton can provide.
/// Routing is `knows`, in insertion order — which is the second reason `knows`
/// is a required method rather than a defaulted one.
#[test]
fn servants_routes_between_two_generated_skeletons_by_key() {
    /// A gauge that exists only to be a different interface.
    struct Dial;
    impl emitted::f_24_skeleton_surface::gc24::GaugeServant for Dial {
        fn knows(&self, at: &emitted::f_24_skeleton_surface::gc24::GaugeTarget<'_>) -> bool {
            at.is_default()
        }
        fn latest(
            &mut self,
            _at: &emitted::f_24_skeleton_surface::gc24::GaugeTarget<'_>,
        ) -> Result<emitted::f_24_skeleton_surface::gc24::Reading, GaugeFault> {
            Ok(Reading { at: 1.0, sequence_no: 1, unit: "C".into() })
        }
        fn label(
            &mut self,
            _at: &emitted::f_24_skeleton_surface::gc24::GaugeTarget<'_>,
        ) -> Result<String, GaugeFault> {
            Ok("dial".into())
        }
        fn set_label(
            &mut self,
            _at: &emitted::f_24_skeleton_surface::gc24::GaugeTarget<'_>,
            _value: String,
        ) -> Result<(), GaugeFault> {
            Ok(())
        }
        fn record(
            &mut self,
            _at: &emitted::f_24_skeleton_surface::gc24::GaugeTarget<'_>,
            _sample: f64,
            _unit: String,
        ) -> Result<emitted::f_24_skeleton_surface::gc24::Reading, GaugeFault> {
            Err(rt::raise::no_implement().did_not_run().into())
        }
        fn scale_all(
            &mut self,
            _at: &emitted::f_24_skeleton_surface::gc24::GaugeTarget<'_>,
            _e: f64,
        ) -> Result<i32, GaugeFault> {
            Ok(0)
        }
        fn reset(
            &mut self,
            _at: &emitted::f_24_skeleton_surface::gc24::GaugeTarget<'_>,
        ) -> Result<(), GaugeFault> {
            Ok(())
        }
        fn split(
            &mut self,
            _at: &emitted::f_24_skeleton_surface::gc24::GaugeTarget<'_>,
        ) -> Result<(f64, String), GaugeFault> {
            Ok((1.0, "C".into()))
        }
    }
    use emitted::f_24_skeleton_surface::gc24::{GaugeFault, Reading};

    let dirs = refs_at(1);
    // A second root, one segment below the first, so neither key space can
    // reach into the other.
    let gauges = GaugeRefs::new(ObjectHome::new("127.0.0.1", 1, b"dirsvc/g".to_vec()));

    let mut all = Servants::new()
        .with(DirectorySkeleton::new(dirs.clone(), Tree::new()))
        .with(GaugeSkeleton::new(gauges.clone(), Dial));
    assert_eq!(all.len(), 2);

    assert!(all.knows(&dirs.key_of("")), "the directory root");
    assert!(all.knows(gauges.root_key()), "the gauge");
    assert!(!all.knows(b"neither"), "and nothing else");

    // Each key reaches its own interface, and an operation belonging to the
    // other one is `BAD_OPERATION` rather than silently served.
    let mut out = Encoder::continuing_at(Endian::Big, 24);
    all.dispatch_body(&request(dirs.root_key(), "count", |_| {}), &mut out).expect("count");

    let mut out = Encoder::continuing_at(Endian::Big, 24);
    all.dispatch_body(&request(gauges.root_key(), "_get_label", |_| {}), &mut out)
        .expect("_get_label");
    let body = out.finish().expect("finish");
    let mut d = rt::Decoder::new(&body, Endian::Big);
    assert_eq!(d.get_string().expect("label"), "dial");

    let mut out = Encoder::continuing_at(Endian::Big, 24);
    let err = all
        .dispatch_body(&request(gauges.root_key(), "count", |_| {}), &mut out)
        .expect_err("count is not a Gauge operation");
    assert_eq!(err.id, rt::BAD_OPERATION);

    // An empty multiplexer knows nothing, rather than everything — the
    // opposite of what `Dispatch::knows` defaults to.
    let mut none = Servants::new();
    assert!(none.is_empty());
    assert!(!none.knows(ROOT));
    let mut out = Encoder::continuing_at(Endian::Big, 24);
    let err = none.dispatch_body(&request(ROOT, "count", |_| {}), &mut out).expect_err("empty");
    assert_eq!(err.id, rt::OBJECT_NOT_EXIST);
}
