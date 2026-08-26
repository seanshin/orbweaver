//! **A redirect emitted for a *name* rather than for an object** — the thing
//! four records name as the blocker on D029 §6.1's lifecycle row, built here so
//! that what it *is* and what it still **cannot** do are both measured rather
//! than argued.
//!
//! Everything below runs. The long part is the design finding, because the
//! valuable half of this batch is not the servant — it is the reason the
//! servant does not close the row.
//!
//! # What a name-keyed redirect is
//!
//! A servant whose object keys are **names**, which hosts no objects at all.
//! `knows` is `false` for every key, truthfully; `redirect` is a lookup in a
//! name table; `locate` answers the same thing one message earlier. A caller
//! holding a reference to it invokes an operation, gets a `LOCATION_FORWARD`
//! carrying whatever currently serves that name, follows it, and is answered.
//! When the name is rebound to a different backend the caller's *reference is
//! unchanged* and the next call lands on the new backend. That is measured by
//! `the_same_name_reaches_the_new_backend_after_a_rebind`.
//!
//! **This was unwritable on this ORB until 2026-08-26, and not for want of a
//! hook.** `serve_one` asked `knows` before `redirect`, so a servant whose
//! `knows` is `false` was refused with `OBJECT_NOT_EXIST` before `redirect` was
//! reached. Its only recourse was the default `knows` of `true`, which forwards
//! **or** refuses uniformly and cannot say *this name I redirect, that name
//! does not exist*. Telling those two apart is the entire content of a
//! name-keyed redirect, and it is
//! `a_name_the_forwarder_does_not_hold_is_refused_rather_than_forwarded`.
//! The argument for the order is `orbweaver_giop::server::serve_one_ordering`.
//!
//! # The design question, answered: is `LOCATION_FORWARD` even the mechanism?
//!
//! **Yes — but it can never be emitted by the party that went away, and that is
//! the whole difficulty.** A forward is a *reply*. A reply requires a listener.
//! A server that has been removed is, by definition, not listening. So
//! "`LOCATION_FORWARD` served by the removed server" is a contradiction in
//! terms: a server still able to answer has not been removed, it has been
//! *relieved*. The mechanism is right and the emitter cannot be the thing that
//! moved.
//!
//! It follows that a redirect for a name needs **a third endpoint that outlives
//! both**, and — this is the part that decides everything else — **the client's
//! reference must have pointed at that endpoint from the start.** A client
//! holding backend A's IOR cannot be redirected after A dies, by anybody, at any
//! layer. That is not a gap in this ORB; it is what an IOR *is*, an address plus
//! a key. Nothing in GIOP lets a third party answer a TCP connection to an
//! address nobody is bound to.
//!
//! **The obvious alternative, refused rather than missed: a tombstone.** Leave
//! the removed server's ORB listening at the same address, hosting nothing,
//! answering every request with a forward. It is a real design and it is a
//! worse one, for three reasons that are each fatal on their own. It means the
//! process has **not** stopped, which contradicts the whole of D034 — a
//! shutdown that keeps a listener is a shutdown that never returns its port,
//! its file descriptors, or its memory. It does not survive the cases "removed"
//! usually means: process death, a crash, an evicted container, a machine that
//! went away — none of which leave anything behind to be a tombstone. And it is
//! **unbounded**: every server ever removed must be tombstoned for as long as
//! any client might still hold its reference, which nothing can know, so the
//! tombstones accumulate for the lifetime of the deployment. An indirection
//! that is entered *before* the first call has none of these properties, which
//! is the argument for X below and against this.
//!
//! **So `corbaname:` is not the answer either, and it is worth saying why**,
//! because it is the first thing that comes to mind. A `corbaname:` URL is
//! resolved on the
//! *client*, once, at bind time; what the client holds afterwards is the
//! resolved IOR, which is exactly as dead as before. `corbaname:` moves the
//! moment of resolution earlier. It does not make the reference indirect. The
//! servant below is the alternative: the reference points at the forwarder for
//! the whole of its life and resolution happens **per invocation, on the server
//! side**, which is the only place it can happen late enough to matter.
//!
//! A servant of this shape is an **Implementation Repository in all but name**,
//! and that brings the honest part: **CORBA does not specify one.** The
//! specification names the IMR as something an object adapter may consult and
//! leaves it implementation-defined. The wire half is specified — §9.4.3.2's
//! `LOCATION_FORWARD` and §9.4.5's `OBJECT_FORWARD`, both implemented, both
//! measured here and in `locate_forward_and_reply_contexts.rs`. The **policy**
//! half — what a name resolves to now, and who says so — has no specification to
//! implement from.
//!
//! # Why this does not close D029 §6.1's lifecycle row, stated plainly
//!
//! **The row does not move, and this file is not an argument that it should.**
//! What moved is that the mechanism the row waits on is now implementable, is
//! implemented, and that its remaining blocker is a **decision** rather than a
//! missing capability. Nothing here removes a server, and nothing here puts a
//! client behind an indirect reference. Both ends are up throughout.
//!
//! The missing thing, named as precisely as it can be:
//!
//! > **X — that the reference `Orb::server` hands out is *indirect*: its IIOP
//! > profile carries a name-resolving endpoint's address and a name, rather than
//! > the servant's own address and an object key.**
//!
//! X is **not** a successor registry, and this file deliberately does not build
//! one. The mapping already has an owner: for a name it is CosNaming's
//! `rebind`, which this ORB already serves — `docs/SERVICES-COVERAGE.md` owns
//! the count, which is deliberately not retyped here — and which the successor
//! already
//! calls as part of coming up. Nothing new is needed to *know* where a name
//! points. What is missing is upstream of that, and it is a decision because it
//! is four decisions wearing one coat:
//!
//! 1. **It changes every IOR this project emits.** D019 step 4 made
//!    `Orb::server` and `Orb::pool` the only public way to obtain transport;
//!    whose host and port go into the IIOP profile is that path's most
//!    wire-visible promise, and every peer that has recorded one of our
//!    references has recorded that answer.
//! 2. **It inverts a layer.** CosNaming is a servant built *on* the ORB.
//!    Making reference-minting consult it makes the ORB depend on a service that
//!    depends on the ORB. D019's title is *the ORB has no object*; this would
//!    give it one.
//! 3. **It displaces the leak rather than closing it, and a decision must say by
//!    how much.** The forwarding endpoint can itself die. D029 §6.1's
//!    event-channel item 1 already names this exact shape for the bootstrap
//!    address — *"the leak is displaced, not closed — from N channels to one"*.
//!    Displacement from N to 1 is a real gain and is **not** closure; whoever
//!    proposes X has to state which of the two is being claimed.
//! 4. **It does not fix the stale binding**, and D029 §6.1's event-channel item
//!    4 already says why: *"a binding outlives its channel"* — unbinding is
//!    deliberately separate from the channel going away. So a name-keyed
//!    forwarder faithfully redirects a caller to an IOR that is **also** dead,
//!    and the caller learns by dialling and failing exactly one hop later than
//!    before. Closing *that* needs something to notice a server is gone and
//!    retract its binding, which is liveness detection, which is a fifth
//!    decision and a much larger one than X.
//!
//! A fifth consequence, for whoever writes X up: it re-opens **D013**, which
//! decided reference identity in the pool under the assumption that an IOR names
//! an object. Two references minted for one name, and a reference minted for a
//! name against one minted directly for the object, are the same object by
//! `is_equivalent` and different by address.
//!
//! # What no peer has been asked, and why nothing here asks one
//!
//! *A peer is the oracle for a forward*, and omniORB following our
//! `LOCATION_FORWARD` is already measured by `spikes/perm_fallback.sh`. This
//! file adds **no new wire shape**: `encode_location_forward` is untouched, and
//! a forward emitted because a *name* resolved is byte-for-byte the same message
//! as a forward emitted because an *object* moved — the reply carries an IOR and
//! says nothing about why. There is therefore nothing for a foreign peer to
//! distinguish, and a new spike asking one to follow this particular forward
//! would measure the code path already measured. That is the honest reason there
//! is no peer leg here, and it is a claim about the bytes, which
//! `the_forward_a_name_produces_is_the_same_message_an_object_move_produces`
//! checks rather than asserts in prose.
//!
//! Both byte orders throughout, and decoded values are compared — never buffers.

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{
    Dispatch, LocateStatus, Request, ServeStep, Served, SharedDispatch, SystemException,
    decode_request, encode_location_forward, serve_one_ordering,
};
use orbweaver_giop::{
    Connection, DEFAULT_MAX_MESSAGE_SIZE, Forward, IiopProfile, Ior, LocateResult, ReplyStatus,
    Version, decode_reply, encode_request, read_message,
};
use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Every wait answers to this. A test that can hang is not a test.
const T: Duration = Duration::from_secs(10);

const BOTH_ORDERS: [Endian; 2] = [Endian::Big, Endian::Little];

/// The two names the forwarder holds, and one it does not.
const INVENTORY: &[u8] = b"inventory";
const ORDERS: &[u8] = b"orders";
const NOT_BOUND: &[u8] = b"no-such-name";

// ─────────────────────────────────────────────────────────────────────────────
// The name table, and the servant that redirects for it
// ─────────────────────────────────────────────────────────────────────────────

/// Where each name currently points. Shared with the test so a rebind can
/// happen *while a client's reference stays exactly as it was* — which is the
/// property under measurement, not an implementation convenience.
type Names = Arc<Mutex<HashMap<Vec<u8>, Ior>>>;

/// A servant keyed by names, hosting no objects.
///
/// The three answers are the whole of it, and each is the truth rather than a
/// convenient answer:
///
/// - `knows` is **`false`**, always. It hosts nothing; there is no object here
///   for any key. Before the `serve_one` reorder this truthful answer made the
///   servant unreachable, and the workaround was to lie.
/// - `redirect` is a lookup. `Some` for a name that is bound, `None` for one
///   that is not — and `None` then falls through to `knows`, which refuses.
/// - `locate` answers the same question one message earlier, so a caller that
///   probes and a caller that invokes are told the same thing. That agreement
///   is `the_probe_and_the_request_agree_about_every_name`.
struct NameForwarder {
    names: Names,
    /// Every key `redirect` was asked about, in order. Exists because the
    /// reorder means `redirect` now sees keys the servant does not know, and a
    /// claim about what a servant is *asked* should be measured too.
    asked: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl NameForwarder {
    fn look_up(&self, name: &[u8]) -> Option<Forward> {
        // A name-keyed redirect is Temporary on purpose: the binding is the
        // authority and it can change again, so the client must come back
        // through this endpoint rather than re-point its reference. A permanent
        // forward would tell the client to forget the name -- which is the one
        // thing that must not happen, because the name is the only durable part.
        self.names.lock().expect("names").get(name).cloned().map(Forward::Temporary)
    }
}

impl Dispatch for NameForwarder {
    fn dispatch(&mut self, _request: &Request, _out: &mut Encoder) -> Result<(), SystemException> {
        // Unreachable: every request either forwards or is refused by `knows`.
        // Left as a wrong answer rather than `unreachable!()` so a regression
        // fails a test instead of panicking in a serving thread.
        Err(SystemException::object_not_exist())
    }

    fn knows(&self, _object_key: &[u8]) -> bool {
        false
    }

    fn redirect(&mut self, request: &Request) -> Option<Forward> {
        self.asked.lock().expect("asked").push(request.object_key.clone());
        self.look_up(&request.object_key)
    }

    fn locate(&self, object_key: &[u8]) -> LocateStatus {
        match self.look_up(object_key) {
            Some(to) => LocateStatus::ObjectForward(to),
            None => LocateStatus::UnknownObject,
        }
    }
}

/// A backend that answers with its own name, so a test can prove **which**
/// server replied rather than only that one did.
struct Backend {
    id: &'static str,
    key: Vec<u8>,
}

impl Dispatch for Backend {
    fn dispatch(&mut self, _request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        out.put_string_bytes(self.id.as_bytes());
        Ok(())
    }

    fn knows(&self, object_key: &[u8]) -> bool {
        object_key == self.key
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Harness
// ─────────────────────────────────────────────────────────────────────────────

struct Running {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

impl Running {
    fn shut_down(self) {
        self.stop.store(true, Ordering::SeqCst);
        // The accept loop checks the flag between connections, so give it one.
        let _ = TcpStream::connect(self.addr);
        let _ = self.thread.join();
    }

    /// The IOR a client would hold for `key` at this server.
    fn ior(&self, key: &[u8]) -> Ior {
        Ior {
            type_id: "IDL:test/Named:1.0".into(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: self.addr.ip().to_string(),
                port: self.addr.port(),
                object_key: key.to_vec(),
                components: Vec::new(),
            }],
        }
    }
}

fn serving<D: Dispatch + Send + 'static>(servant: D, key: &[u8]) -> Running {
    let server = Orb::new().server("127.0.0.1:0", key.to_vec()).expect("binds a loopback port");
    let addr = server.local_addr().expect("has an address");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let mut servant = servant;
    let thread = std::thread::spawn(move || {
        let _ = server.serve(&mut servant, move || flag.load(Ordering::SeqCst));
    });
    Running { addr, stop, thread }
}

/// Dials `at` asking for `key`, in `endian`, with a read deadline so a wedged
/// peer fails the test rather than hanging it.
fn dial(at: SocketAddr, key: &[u8], endian: Endian) -> Connection {
    let profile = IiopProfile {
        version: Version::V1_2,
        host: at.ip().to_string(),
        port: at.port(),
        object_key: key.to_vec(),
        components: Vec::new(),
    };
    let mut conn = Connection::connect_to(&profile, T).expect("connects");
    conn.set_endian(endian);
    conn
}

/// Every key `redirect` was asked about, in order.
type Asked = Arc<Mutex<Vec<Vec<u8>>>>;

/// A forwarder in front of two backends, with `INVENTORY` bound to the first.
struct Fixture {
    fwd: Running,
    first: Running,
    second: Running,
    /// The live table. A test rebinds through this while a client's reference
    /// stays exactly as it was — which is the property, not a convenience.
    names: Names,
    asked: Asked,
}

impl Fixture {
    fn shut_down(self) {
        self.fwd.shut_down();
        self.first.shut_down();
        self.second.shut_down();
    }

    fn bind(&self, name: &[u8], to: &Running) {
        self.names.lock().expect("names").insert(name.to_vec(), to.ior(b"backend"));
    }
}

fn a_forwarder_and_two_backends() -> Fixture {
    let first = serving(Backend { id: "first", key: b"backend".to_vec() }, b"backend");
    let second = serving(Backend { id: "second", key: b"backend".to_vec() }, b"backend");

    let names: Names = Arc::new(Mutex::new(HashMap::new()));
    names.lock().expect("names").insert(INVENTORY.to_vec(), first.ior(b"backend"));

    let asked: Asked = Arc::new(Mutex::new(Vec::new()));
    let fwd =
        serving(NameForwarder { names: Arc::clone(&names), asked: Arc::clone(&asked) }, INVENTORY);
    Fixture { fwd, first, second, names, asked }
}

/// What the backend said, or a panic naming the endian that broke.
fn call(at: SocketAddr, name: &[u8], endian: Endian) -> String {
    let mut conn = dial(at, name, endian);
    let reply = conn
        .invoke("who", |_: &mut Encoder| {})
        .unwrap_or_else(|e| panic!("{endian:?}: the call must be served through the name: {e:?}"));
    assert_eq!(reply.status, ReplyStatus::NoException, "{endian:?}");
    reply.body().expect("a body").get_string().expect("a string")
}

// ─────────────────────────────────────────────────────────────────────────────
// The property
// ─────────────────────────────────────────────────────────────────────────────

/// A caller holding a reference to the forwarder, and knowing only a **name**,
/// is served by whatever currently answers to that name.
#[test]
fn a_bound_name_reaches_whatever_currently_serves_it() {
    let f = a_forwarder_and_two_backends();

    for endian in BOTH_ORDERS {
        assert_eq!(
            call(f.fwd.addr, INVENTORY, endian),
            "first",
            "{endian:?}: the name must reach the backend it is bound to"
        );
    }

    f.shut_down();
}

/// **The transparency claim this whole file exists for.** The client's
/// reference does not change; the binding does; the next call lands on the new
/// backend and the caller is never told that anything happened.
///
/// This is the half `corbaname:` cannot do. A client that resolved the name once
/// and kept the answer would still be calling `first` here.
#[test]
fn the_same_name_reaches_the_new_backend_after_a_rebind() {
    let f = a_forwarder_and_two_backends();

    for endian in BOTH_ORDERS {
        assert_eq!(call(f.fwd.addr, INVENTORY, endian), "first", "{endian:?}: before the rebind");

        f.bind(INVENTORY, &f.second);

        assert_eq!(
            call(f.fwd.addr, INVENTORY, endian),
            "second",
            "{endian:?}: after the rebind the same reference must reach the new backend"
        );

        // And back, so the test is not passing on a one-way latch.
        f.bind(INVENTORY, &f.first);
        assert_eq!(call(f.fwd.addr, INVENTORY, endian), "first", "{endian:?}: rebound back");
    }

    f.shut_down();
}

/// **The distinction that was impossible before the `serve_one` reorder**, and
/// the reason this file could not have been written yesterday.
///
/// A forwarder must be able to say *this name I redirect, that name does not
/// exist*. With `knows` asked first, a servant whose `knows` is `false` refused
/// everything and one whose `knows` was `true` forwarded everything; there was
/// no third answer. This test is red on either of those.
#[test]
fn a_name_the_forwarder_does_not_hold_is_refused_rather_than_forwarded() {
    let f = a_forwarder_and_two_backends();

    for endian in BOTH_ORDERS {
        let mut conn = dial(f.fwd.addr, NOT_BOUND, endian);
        match conn.invoke("who", |_: &mut Encoder| {}) {
            Err(orbweaver_giop::Error::SystemException { ref id, .. })
                if id.contains("OBJECT_NOT_EXIST") => {}
            other => panic!(
                "{endian:?}: an unbound name must be OBJECT_NOT_EXIST, not a forward and not \
                 a reply. Got {other:?}"
            ),
        }
        // And the bound one still works on a fresh connection, so the refusal
        // is about the name and not about the forwarder having given up.
        assert_eq!(call(f.fwd.addr, INVENTORY, endian), "first", "{endian:?}");
    }

    f.shut_down();
}

/// The probe and the request must agree about every name — a bound one, and an
/// unbound one. They disagreed for most of 2026-08-26, in both directions at
/// different hours of the day, which is why this asserts both.
#[test]
fn the_probe_and_the_request_agree_about_every_name() {
    let f = a_forwarder_and_two_backends();
    f.bind(ORDERS, &f.second);

    for endian in BOTH_ORDERS {
        for (name, expected) in
            [(INVENTORY, Some("first")), (ORDERS, Some("second")), (NOT_BOUND, None)]
        {
            let mut conn = dial(f.fwd.addr, name, endian);
            let probed = conn.locate().expect("the probe is answered");
            drop(conn);

            match (probed, expected) {
                (LocateResult::Forward(_), Some(who)) => {
                    assert_eq!(
                        call(f.fwd.addr, name, endian),
                        who,
                        "{endian:?}: the probe said elsewhere; the request must agree and land"
                    );
                }
                (LocateResult::Unknown, None) => {
                    let mut conn = dial(f.fwd.addr, name, endian);
                    assert!(
                        matches!(
                            conn.invoke("who", |_: &mut Encoder| {}),
                            Err(orbweaver_giop::Error::SystemException { .. })
                        ),
                        "{endian:?}: the probe said nowhere; the request must agree"
                    );
                }
                (got, want) => panic!(
                    "{endian:?}: probe and expectation disagree for {}: got {got:?}, wanted {want:?}",
                    String::from_utf8_lossy(name)
                ),
            }
        }
    }

    f.shut_down();
}

/// `redirect` is now asked about keys the servant does not know — that is the
/// whole of what the reorder changed, so it is measured rather than described.
///
/// Before, a `knows` of `false` ended the request and this list would be empty.
#[test]
fn redirect_is_asked_about_keys_the_servant_does_not_know() {
    let f = a_forwarder_and_two_backends();

    let mut conn = dial(f.fwd.addr, NOT_BOUND, Endian::Big);
    let _ = conn.invoke("who", |_: &mut Encoder| {});
    drop(conn);

    let seen = f.asked.lock().expect("asked").clone();
    assert!(
        seen.iter().any(|k| k == NOT_BOUND),
        "`redirect` must be consulted for a key `knows` rejects -- it is the only set of \
         keys a forward is for. Saw {seen:?}"
    );

    f.shut_down();
}

// ─────────────────────────────────────────────────────────────────────────────
// The claims the module documentation makes, checked rather than asserted
// ─────────────────────────────────────────────────────────────────────────────

/// The module documentation says a forward emitted because a **name** resolved
/// is the same message as one emitted because an **object** moved, and uses that
/// to explain why no new peer leg is owed. This checks it instead of trusting
/// it: the same `Forward` through the same encoder is the same reply, whatever
/// made the servant produce it.
///
/// Decoded values on both sides — the padding content of a CDR buffer is
/// undefined and this compares what the two mean, not what they contain.
#[test]
fn the_forward_a_name_produces_is_the_same_message_an_object_move_produces() {
    let to = Ior {
        type_id: "IDL:test/Named:1.0".into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "backend.example".into(),
            port: 9009,
            object_key: b"backend".to_vec(),
            components: Vec::new(),
        }],
    };

    for endian in BOTH_ORDERS {
        for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
            // Produced by a name lookup...
            let names: Names = Arc::new(Mutex::new(HashMap::new()));
            names.lock().expect("names").insert(INVENTORY.to_vec(), to.clone());
            let by_name = NameForwarder { names, asked: Arc::new(Mutex::new(Vec::new())) }
                .look_up(INVENTORY)
                .expect("the name is bound");

            // ...and by an object having moved. Same value, so the wire cannot
            // tell them apart, which is the claim.
            let by_move = Forward::Temporary(to.clone());
            assert_eq!(by_name, by_move, "{version} {endian:?}");

            let a = encode_location_forward(version, endian, 7, &by_name).expect("encodes");
            let b = encode_location_forward(version, endian, 7, &by_move).expect("encodes");

            // Our own encoder against our own encoder, so a byte comparison is
            // the claim itself rather than a bet on undefined padding.
            assert_eq!(a, b, "{version} {endian:?}: indistinguishable on the wire");

            // And what they mean, decoded, because that is the property a peer
            // actually acts on.
            let got =
                decode_reply(read_message(&mut &a[..], DEFAULT_MAX_MESSAGE_SIZE).expect("frames"))
                    .expect("decodes");
            assert_eq!(got.status, by_move.reply_status(version), "{version} {endian:?}");
            assert_eq!(
                Ior::read_from(&mut got.body().expect("a body")).expect("an IOR"),
                to,
                "{version} {endian:?}: the reference a name produced is the reference"
            );
        }
    }
}

/// Both `serve_one` implementations ask in the order `serve_one_ordering`
/// documents, and there is exactly one argument for that order rather than two
/// copies of it drifting apart.
///
/// The recording servant answers `None`/`true` throughout, so nothing
/// short-circuits and all three questions are reached.
#[test]
fn the_two_serve_one_paths_ask_in_the_documented_order() {
    #[derive(Default)]
    struct Recorder(Arc<Mutex<Vec<ServeStep>>>);

    impl Dispatch for Recorder {
        fn dispatch(&mut self, _r: &Request, _o: &mut Encoder) -> Result<(), SystemException> {
            self.0.lock().expect("log").push(ServeStep::Dispatch);
            Ok(())
        }
        fn knows(&self, _k: &[u8]) -> bool {
            self.0.lock().expect("log").push(ServeStep::Knows);
            true
        }
        fn redirect(&mut self, _r: &Request) -> Option<Forward> {
            self.0.lock().expect("log").push(ServeStep::Redirect);
            None
        }
    }

    impl SharedDispatch for Recorder {
        fn dispatch(&self, _r: &Request, _o: &mut Encoder) -> Result<(), SystemException> {
            self.0.lock().expect("log").push(ServeStep::Dispatch);
            Ok(())
        }
        fn knows(&self, _k: &[u8]) -> bool {
            self.0.lock().expect("log").push(ServeStep::Knows);
            true
        }
        fn redirect(&self, _r: &Request) -> Option<Forward> {
            self.0.lock().expect("log").push(ServeStep::Redirect);
            None
        }
    }

    let wire = encode_request(Version::V1_2, Endian::Big, 1, b"backend", "who", true, |_| {})
        .expect("encodes");
    let request =
        decode_request(read_message(&mut &wire[..], DEFAULT_MAX_MESSAGE_SIZE).expect("frames"))
            .expect("decodes");

    // The shared path.
    let log = Arc::new(Mutex::new(Vec::new()));
    let shared = Recorder(Arc::clone(&log));
    let mut out = Encoder::new(Endian::Big);
    let served = SharedDispatch::serve_one(&shared, &request, &mut out).expect("served");
    assert!(matches!(served, Served::Body(_)), "the recorder answers");
    assert_eq!(
        log.lock().expect("log").as_slice(),
        serve_one_ordering(),
        "SharedDispatch::serve_one must ask in the documented order"
    );

    // The serialized (compatibility) path, over the same servant type.
    let log = Arc::new(Mutex::new(Vec::new()));
    let serialized = orbweaver_giop::server::Serialized::new(Recorder(Arc::clone(&log)));
    let mut out = Encoder::new(Endian::Big);
    let served = SharedDispatch::serve_one(&serialized, &request, &mut out).expect("served");
    assert!(matches!(served, Served::Body(_)), "the recorder answers");
    assert_eq!(
        log.lock().expect("log").as_slice(),
        serve_one_ordering(),
        "Serialized::serve_one must ask in the same order as the shared path"
    );
}
