//! **What a caller is told about an object key nobody activated** — the
//! question a hand-written C peer asked by dialling us on 2026-08-26, measured
//! here rather than argued.
//!
//! The peer's finding: `spike-server` answers `ping` on a key nothing ever
//! activated, because `Dispatch::knows` defaults to accepting every key. The
//! question it raised was not *is this a bug* but **is this default right**,
//! and that is a real question: a single-object server has one key, and a
//! multi-object server overrides `knows` and does check.
//!
//! # The answer: the default is wrong, and not for the reason it looks like
//!
//! Not because accepting every key is never right — it is exactly right for
//! `orbweaver_gen`'s `Servants` multiplexer, which asks its entries, and for
//! `PyServant`, which bridges whatever arrives. **It is wrong because meaning
//! it and forgetting it are spelled the same way.**
//!
//! The argument and its evidence live on `orbweaver_giop::server::Dispatch::knows`
//! and are not restated here. In one line, so a reader of this file knows what
//! it is gating: CORBA 3.4 §15.3.8.6 makes *what happens to an id the Active
//! Object Map does not hold* a policy with three values and a stated default,
//! that default is `USE_ACTIVE_OBJECT_MAP_ONLY`, and accepting every key is
//! `USE_DEFAULT_SERVANT` — a policy the specification requires a POA to be
//! **created with** and to have a servant **registered** for. In CORBA the
//! permissive policy cannot be reached by omission. Here omission is the only
//! way anyone reaches it.
//!
//! # What this file pins, and why pinning today's answer is not endorsing it
//!
//! Every test below computes what a caller *should* see from `AMBIENT_STATES`
//! and then goes and looks on a socket. So the gate is not "an unactivated key
//! is served"; it is **"the wire agrees with what the fixture says it does"**.
//!
//! That constant used to be `server::default_knows_policy()` — the crate's own
//! sentence about what its two `knows` defaults enacted. **D036 (approved
//! 2026-08-29) deleted those defaults**, so the crate publishes no such
//! sentence any more and there is nothing crate-wide left to disagree with:
//! every servant states its own answer. The fact moved to where the answer now
//! lives, which is the fixture below.
//!
//! # Blast radius — a figure that is computed, beside the reading it replaced
//!
//! **Today's figure is not written here.** It is computed from the tree by
//! `no_servant_a_build_emits_answers_for_a_key_nobody_activated` and
//! printed under `--nocapture`, because the reading that used to stand in this
//! paragraph had already drifted and nothing could go red on it.
//!
//! The **2026-08-26** reading, kept as the dated record of what the C peer's
//! sweep found rather than as a claim about today: 72
//! `Dispatch`/`SharedDispatch` implementations, 46 overriding `knows`, 26
//! inheriting the default; 12 of 12 hand-written `orbweaver-giop` servants
//! overriding with real checks (`NamingServer` looks the key up in its context
//! tree, `TradingServer` and the two event servants compare against their key,
//! `EventChannelServer` routes it through four maps); 6 of 6 production
//! servants in the sibling crates overriding (`RepositoryServer`,
//! `ExpertService`, `TenantService`); 16 of 16 emitted skeletons overriding,
//! with `orbweaver_gen`'s servant trait declaring its own `knows` **required,
//! with no default** — that layer already made this decision, the other way,
//! and pinned it.
//!
//! One line of that reading was **already false when it was written down as a
//! standing fact**: it named `orbweaver-gen/src/pyservant.rs` as a production
//! inheritor, and the seam refactor moved that servant to
//! `crate::seam::ForeignServant`, which overrides `knows`. Nothing noticed,
//! because the guard beside the list asserted the *list* was non-empty. That is
//! the whole reason the roster below is read out of the tree.
//!
//! So "checking the key is ceremony" is a claim only the inheritors act on, and
//! **the production inheritors are defects rather than deliberate**: they are
//! computed by the test at the bottom of this file, because a crate this one
//! does not own is where the repair has to land.
//!
//! # What a caller can tell, which is the reason any of this matters
//!
//! A reference is an address plus a key. When `knows` accepts every key, the
//! key contributes **nothing** to selecting a target, so what a caller holds is
//! not what names the target — the address is. A caller can establish that with
//! one call: fabricate any key at the endpoint and be answered. Under
//! §15.3.8.6's default it would be told `OBJECT_NOT_EXIST` and would learn
//! nothing. That is a fact about *what implements the reference*, which is
//! D029 §6.1's **Backend** row, and it is measured by
//! `a_key_nobody_activated_cannot_be_told_apart_from_one_that_was`.
//!
//! # Both byte orders throughout, and decoded values are compared, never buffers.

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{
    Dispatch, OBJECT_NOT_EXIST, Request, Serialized, SharedDispatch, SystemException,
    UnknownKeyPolicy, key_policy_of,
};
use orbweaver_giop::{Connection, IiopProfile, LocateResult, Version};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Every wait answers to this. A test that can hang is not a test.
const T: Duration = Duration::from_secs(10);

const BOTH_ORDERS: [Endian; 2] = [Endian::Big, Endian::Little];

/// All three, because the reply and locate framing differ across them and a
/// claim about what a caller is told should hold for every caller.
const EVERY_VERSION: [Version; 3] = [Version::V1_0, Version::V1_1, Version::V1_2];

/// The key the server is given, published in its IOR, and activated.
const ACTIVATED: &[u8] = b"OrbweaverEcho";

/// A key nothing ever activated. Deliberately adjacent to [`ACTIVATED`] — a
/// servant that checked only a prefix would accept it, and a key drawn from
/// thin air would never have shown that.
const NEVER_ACTIVATED: &[u8] = b"OrbweaverEchoX";

/// What the servants answer, so a reply proves *which* servant replied rather
/// than only that one did.
const ANSWER: i32 = 42;

// ─────────────────────────────────────────────────────────────────────────────
// The two servant shapes the whole question is about
// ─────────────────────────────────────────────────────────────────────────────

/// The shape that inherits the default: a `Dispatch` with no `knows`.
///
/// This is `spike-server`'s `Echo` reproduced — deliberately reproduced rather
/// than imported, because `spike-server` lives in `orbweaver-object` and a test
/// in this crate cannot reach it. What is being gated is the **trait default**,
/// which is this crate's, and any servant of this shape gets the same answer.
/// The reproduction is faithful in the only respect that matters: it declares
/// no `knows`.
struct Ambient;

/// What [`Ambient`] and [`SharedAmbient`] state, as data.
///
/// These fixtures exist to **be** the permissive answer so a caller can be
/// asked what it sees when a servant gives it. Before D036 they gave it by
/// inheriting a trait default and this constant was the crate's
/// `default_knows_policy()`; now they give it by writing `true`, and the
/// constant sits beside them because that is where the answer is.
const AMBIENT_STATES: UnknownKeyPolicy = UnknownKeyPolicy::ServeAnyway;

impl Dispatch for Ambient {
    /// `true`, and D036 made saying so compulsory — which is the whole point of
    /// this fixture. `Ambient` exists to BE the permissive answer, so that the
    /// wire can be asked what a caller sees when a servant gives it. It used to
    /// give it by inheriting; it gives it by stating it now, and the property
    /// under test is unchanged because the answer is unchanged.
    fn knows(&self, _object_key: &[u8]) -> bool {
        true
    }

    fn dispatch(&mut self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        if request.operation != "ping" {
            return Err(SystemException::bad_operation());
        }
        out.put_i32(ANSWER);
        Ok(())
    }
}

/// The shape that checks: `knows` is one `==` against the key the server was
/// given. This is what `TradingServer` and both event servants already do, and
/// what §15.3.8.6's default asks for.
struct Activated {
    key: Vec<u8>,
}

impl Dispatch for Activated {
    fn dispatch(&mut self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        if request.operation != "ping" {
            return Err(SystemException::bad_operation());
        }
        out.put_i32(ANSWER);
        Ok(())
    }

    fn knows(&self, object_key: &[u8]) -> bool {
        object_key == self.key
    }
}

/// The same two shapes on the `&self` trait, so the claim covers the path a
/// concurrent servant takes as well as the serialized one. `Server` reaches
/// `Dispatch` only by wrapping it in [`Serialized`], and the two `serve_one`
/// bodies are separate code — `the_two_serve_one_paths_enact_the_same_policy`
/// is what stops them drifting.
struct SharedAmbient;

impl SharedDispatch for SharedAmbient {
    /// The `SharedDispatch` half of `Ambient`, and stated for the same reason:
    /// this fixture is the permissive answer, held so a caller can be asked
    /// what it sees.
    fn knows(&self, _object_key: &[u8]) -> bool {
        true
    }

    fn dispatch(&self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        if request.operation != "ping" {
            return Err(SystemException::bad_operation());
        }
        out.put_i32(ANSWER);
        Ok(())
    }
}

struct SharedActivated {
    key: Vec<u8>,
}

impl SharedDispatch for SharedActivated {
    fn dispatch(&self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        if request.operation != "ping" {
            return Err(SystemException::bad_operation());
        }
        out.put_i32(ANSWER);
        Ok(())
    }

    fn knows(&self, object_key: &[u8]) -> bool {
        object_key == self.key
    }
}

/// A servant that hosts nothing at all — `knows` is `false` for every key.
/// Present so [`key_policy_of`] is measured on a third shape and not only on
/// the two it obviously separates: this one accepts *no* key, which is still
/// [`UnknownKeyPolicy::RefuseAsNotExist`], because the policy is about whether
/// an unknown key is refused and never about how many keys are known.
struct HostsNothing;

impl SharedDispatch for HostsNothing {
    fn dispatch(&self, _request: &Request, _out: &mut Encoder) -> Result<(), SystemException> {
        Err(SystemException::object_not_exist())
    }

    fn knows(&self, _object_key: &[u8]) -> bool {
        false
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
}

fn serving<D: Dispatch + Send + 'static>(servant: D) -> Running {
    let server =
        Orb::new().server("127.0.0.1:0", ACTIVATED.to_vec()).expect("binds a loopback port");
    let addr = server.local_addr().expect("has an address");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let mut servant = servant;
    let thread = std::thread::spawn(move || {
        let _ = server.serve(&mut servant, move || flag.load(Ordering::SeqCst));
    });
    Running { addr, stop, thread }
}

fn serving_shared<D: SharedDispatch + Send + 'static>(servant: D) -> Running {
    let server =
        Orb::new().server("127.0.0.1:0", ACTIVATED.to_vec()).expect("binds a loopback port");
    let addr = server.local_addr().expect("has an address");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let thread = std::thread::spawn(move || {
        let _ = server.serve_shared(&servant, move || flag.load(Ordering::SeqCst));
    });
    Running { addr, stop, thread }
}

/// Dials `at` asking for `key`, at `version` in `endian`, with a read deadline
/// so a wedged peer fails the test rather than hanging it.
fn dial(at: SocketAddr, key: &[u8], version: Version, endian: Endian) -> Connection {
    let profile = IiopProfile {
        version,
        host: at.ip().to_string(),
        port: at.port(),
        object_key: key.to_vec(),
        components: Vec::new(),
    };
    let mut conn = Connection::connect_to(&profile, T).expect("connects");
    conn.set_endian(endian);
    conn
}

/// What a caller saw, as a value rather than as a buffer — which is what gets
/// compared, per the wire rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WhatTheCallerSaw {
    /// A normal reply carrying the servant's answer.
    Answered(i32),
    /// `OBJECT_NOT_EXIST`.
    NotExist,
}

/// The answer [`UnknownKeyPolicy`] predicts for a key nobody activated.
///
/// The whole point of the file goes through this function: the expectation is
/// **derived from the policy**, never typed in beside it.
fn predicted(policy: UnknownKeyPolicy) -> WhatTheCallerSaw {
    match policy {
        UnknownKeyPolicy::ServeAnyway => WhatTheCallerSaw::Answered(ANSWER),
        UnknownKeyPolicy::RefuseAsNotExist => WhatTheCallerSaw::NotExist,
    }
}

/// The same prediction for the §9.4.5 probe path.
fn predicted_probe(policy: UnknownKeyPolicy) -> LocateResult {
    match policy {
        UnknownKeyPolicy::ServeAnyway => LocateResult::Here,
        UnknownKeyPolicy::RefuseAsNotExist => LocateResult::Unknown,
    }
}

fn ping(at: SocketAddr, key: &[u8], version: Version, endian: Endian) -> WhatTheCallerSaw {
    let mut conn = dial(at, key, version, endian);
    match conn.invoke("ping", |_: &mut Encoder| {}) {
        Ok(reply) => WhatTheCallerSaw::Answered(
            reply.body().expect("a body").get_i32().expect("an i32 answer"),
        ),
        Err(orbweaver_giop::Error::SystemException { ref id, .. }) if id == OBJECT_NOT_EXIST => {
            WhatTheCallerSaw::NotExist
        }
        Err(e) => panic!("neither an answer nor OBJECT_NOT_EXIST: {e}"),
    }
}

fn probe(at: SocketAddr, key: &[u8], version: Version, endian: Endian) -> LocateResult {
    dial(at, key, version, endian).locate_key(key).expect("the probe is answered")
}

// ─────────────────────────────────────────────────────────────────────────────
// 1 · What a caller sees, on a socket, for a key nobody activated
// ─────────────────────────────────────────────────────────────────────────────

/// The finding the C peer made, reproduced and pinned against the sentence this
/// crate publishes rather than against a literal.
#[test]
fn a_key_nobody_activated_is_answered_exactly_as_the_published_policy_says() {
    let s = serving(Ambient);
    let want = predicted(AMBIENT_STATES);
    for version in EVERY_VERSION {
        for endian in BOTH_ORDERS {
            assert_eq!(
                ping(s.addr, NEVER_ACTIVATED, version, endian),
                want,
                "a key nobody activated, at {version:?}/{endian:?}: the wire disagrees with \
                 what the fixture states. One of the two moved without the other."
            );
        }
    }
    s.shut_down();
}

/// The half that makes it a transparency question rather than a curiosity: the
/// activated key and a key nobody activated are **indistinguishable**.
///
/// This is the D029 §6.1 Backend observation. A caller that can fabricate a key
/// and be answered has learned that the object key selects nothing here — that
/// what is behind this endpoint is one undifferentiated servant and not a POA
/// with an active object map. Under §15.3.8.6's default it would have been told
/// `OBJECT_NOT_EXIST` and learned nothing at all.
#[test]
fn a_key_nobody_activated_cannot_be_told_apart_from_one_that_was() {
    let s = serving(Ambient);
    for version in EVERY_VERSION {
        for endian in BOTH_ORDERS {
            let activated = ping(s.addr, ACTIVATED, version, endian);
            let fabricated = ping(s.addr, NEVER_ACTIVATED, version, endian);
            assert_eq!(
                activated, fabricated,
                "at {version:?}/{endian:?}: the two answers differ, so this servant now \
                 distinguishes its key and the Backend observation this test records has \
                 been closed — move D029 §6.1's row rather than editing this assertion."
            );
        }
    }
    s.shut_down();
}

/// The other arm, on the same wire: a servant whose `knows` is one `==` refuses.
///
/// This is the control that the machinery *can* say `OBJECT_NOT_EXIST` at all.
/// Without it the test above is a green light over an untested path — the
/// green-while-measuring-nothing class.
#[test]
fn a_servant_that_checks_its_key_refuses_with_object_not_exist() {
    let s = serving(Activated { key: ACTIVATED.to_vec() });
    for version in EVERY_VERSION {
        for endian in BOTH_ORDERS {
            assert_eq!(
                ping(s.addr, ACTIVATED, version, endian),
                WhatTheCallerSaw::Answered(ANSWER),
                "the activated key at {version:?}/{endian:?}"
            );
            assert_eq!(
                ping(s.addr, NEVER_ACTIVATED, version, endian),
                WhatTheCallerSaw::NotExist,
                "a key nobody activated at {version:?}/{endian:?}"
            );
        }
    }
    s.shut_down();
}

// ─────────────────────────────────────────────────────────────────────────────
// 2 · The probe path must say the same thing as the request path
// ─────────────────────────────────────────────────────────────────────────────

/// **The invariant that holds whichever way the default goes**, and the one
/// worth having a gate for.
///
/// §9.4.5's `OBJECT_HERE` means *this ORB will accept requests for this
/// object*. A servant whose probe and whose invocation disagree about a key is
/// telling one of the two callers a falsehood, and which one depends only on
/// which message they sent. That is the exact shape the `serve_one` reorder
/// closed on 2026-08-26 for a *moved* key, running here for an *unknown* one.
///
/// It holds today because `Dispatch::locate`'s default is written in terms of
/// `knows`. It is gated because that is a property of the default body, not a
/// law: a servant that puts its key check in `dispatch_body` instead of in
/// `knows` breaks it, and two in this repository do — see the test at the
/// bottom of this file.
#[test]
fn the_probe_path_and_the_request_path_agree_about_an_unactivated_key() {
    for (label, running, checks) in [
        ("inherits the default", serving(Ambient), false),
        ("checks its key", serving(Activated { key: ACTIVATED.to_vec() }), true),
    ] {
        let policy = if checks { UnknownKeyPolicy::RefuseAsNotExist } else { AMBIENT_STATES };
        for version in EVERY_VERSION {
            for endian in BOTH_ORDERS {
                assert_eq!(
                    ping(running.addr, NEVER_ACTIVATED, version, endian),
                    predicted(policy),
                    "{label}: request path at {version:?}/{endian:?}"
                );
                assert_eq!(
                    probe(running.addr, NEVER_ACTIVATED, version, endian),
                    predicted_probe(policy),
                    "{label}: probe path at {version:?}/{endian:?} disagrees with the request \
                     path — a caller is told 'here' and another 'nowhere' about one key"
                );
            }
        }
        running.shut_down();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3 · The two serve_one bodies are separate code and must not drift
// ─────────────────────────────────────────────────────────────────────────────

/// `SharedDispatch::serve_one` and `Serialized::serve_one` are two hand-written
/// bodies asking the same three questions. `serve_one_ordering` pins the order
/// they ask in; this pins that they reach the same **verdict** about a key
/// nobody activated, which the order alone does not give.
#[test]
fn the_two_serve_one_paths_enact_the_same_policy() {
    let serialized = serving(Ambient);
    let native = serving_shared(SharedAmbient);
    let want = predicted(AMBIENT_STATES);
    for version in EVERY_VERSION {
        for endian in BOTH_ORDERS {
            assert_eq!(
                ping(serialized.addr, NEVER_ACTIVATED, version, endian),
                want,
                "Serialized path at {version:?}/{endian:?}"
            );
            assert_eq!(
                ping(native.addr, NEVER_ACTIVATED, version, endian),
                want,
                "native SharedDispatch path at {version:?}/{endian:?}"
            );
        }
    }
    serialized.shut_down();
    native.shut_down();

    let checked_serialized = serving(Activated { key: ACTIVATED.to_vec() });
    let checked_native = serving_shared(SharedActivated { key: ACTIVATED.to_vec() });
    for version in EVERY_VERSION {
        for endian in BOTH_ORDERS {
            assert_eq!(
                ping(checked_serialized.addr, NEVER_ACTIVATED, version, endian),
                WhatTheCallerSaw::NotExist,
                "Serialized path, checking servant, at {version:?}/{endian:?}"
            );
            assert_eq!(
                ping(checked_native.addr, NEVER_ACTIVATED, version, endian),
                WhatTheCallerSaw::NotExist,
                "native path, checking servant, at {version:?}/{endian:?}"
            );
        }
    }
    checked_serialized.shut_down();
    checked_native.shut_down();
}

// ─────────────────────────────────────────────────────────────────────────────
// 4 · key_policy_of measures rather than declares
// ─────────────────────────────────────────────────────────────────────────────

/// Three shapes, three answers, and the middle one is the one a *declaration*
/// would have got wrong: `HostsNothing` accepts no key at all and is still
/// `RefuseAsNotExist`, because the policy is about whether an unknown key is
/// refused and not about how many keys are known.
#[test]
fn key_policy_of_measures_the_servant_rather_than_believing_it() {
    assert_eq!(
        key_policy_of(&Serialized::new(Ambient), ACTIVATED),
        AMBIENT_STATES,
        "a servant that states `true` enacts ServeAnyway — measured from the servant, not \
         read off its source"
    );
    assert_eq!(
        key_policy_of(&Serialized::new(Activated { key: ACTIVATED.to_vec() }), ACTIVATED),
        UnknownKeyPolicy::RefuseAsNotExist
    );
    assert_eq!(
        key_policy_of(&SharedActivated { key: ACTIVATED.to_vec() }, ACTIVATED),
        UnknownKeyPolicy::RefuseAsNotExist
    );
    assert_eq!(
        key_policy_of(&HostsNothing, ACTIVATED),
        UnknownKeyPolicy::RefuseAsNotExist,
        "a servant that hosts nothing refuses unknown keys — it refuses every key"
    );
}

/// A servant that checks only a **prefix** of the key is a real mistake and an
/// easy one, and it is why `key_policy_of`'s probes are drawn from the
/// activated key rather than from thin air: an unrelated probe is refused by
/// this servant, so a measurement built only from unrelated keys would call it
/// `RefuseAsNotExist` and be right by accident while the servant still serves
/// every key that starts with the right bytes.
#[test]
fn a_prefix_check_is_caught_because_the_probes_are_adjacent() {
    struct PrefixOnly;
    impl SharedDispatch for PrefixOnly {
        fn dispatch(&self, _r: &Request, _o: &mut Encoder) -> Result<(), SystemException> {
            Ok(())
        }
        fn knows(&self, object_key: &[u8]) -> bool {
            object_key.starts_with(ACTIVATED)
        }
    }
    assert!(
        PrefixOnly.knows(NEVER_ACTIVATED),
        "the servant does serve a key nobody activated — that is the defect being detected"
    );
    assert_eq!(
        key_policy_of(&PrefixOnly, ACTIVATED),
        UnknownKeyPolicy::RefuseAsNotExist,
        "a prefix check refuses *some* unknown key, so it is not ServeAnyway"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 5 · Negative controls, with the counter moving
// ─────────────────────────────────────────────────────────────────────────────

/// The controls, run as one test so the counter is a number rather than a set
/// of independent green lights.
///
/// Each row perturbs one thing the tests above depend on and asserts the
/// perturbation is **seen**. A control that is not itself asserted is a
/// comment.
#[test]
fn every_claim_above_has_a_perturbation_that_is_seen() {
    let mut seen = 0usize;

    // (1) The prediction function is not a constant: the two policies predict
    //     different answers on the request path. If they ever collapsed, every
    //     assertion in this file would pass for both servants.
    if predicted(UnknownKeyPolicy::ServeAnyway) != predicted(UnknownKeyPolicy::RefuseAsNotExist) {
        seen += 1;
    }

    // (2) …and on the probe path.
    if predicted_probe(UnknownKeyPolicy::ServeAnyway)
        != predicted_probe(UnknownKeyPolicy::RefuseAsNotExist)
    {
        seen += 1;
    }

    // (3) The wire really can carry `OBJECT_NOT_EXIST` for this key. Without
    //     this the pinned "is answered" result is green over an untried path.
    let checking = serving(Activated { key: ACTIVATED.to_vec() });
    if ping(checking.addr, NEVER_ACTIVATED, Version::V1_2, Endian::Big)
        == WhatTheCallerSaw::NotExist
    {
        seen += 1;
    }
    // (4) …and the probe path really can say `Unknown`.
    if probe(checking.addr, NEVER_ACTIVATED, Version::V1_2, Endian::Little) == LocateResult::Unknown
    {
        seen += 1;
    }
    checking.shut_down();

    // (5) `NEVER_ACTIVATED` is genuinely a different key from `ACTIVATED`. A
    //     typo making them equal would turn every test here into a tautology.
    if NEVER_ACTIVATED != ACTIVATED {
        seen += 1;
    }

    // (6) `key_policy_of` can return both values — a function that always said
    //     `ServeAnyway` would pass test 4's first row and nothing else would
    //     notice.
    if key_policy_of(&Serialized::new(Ambient), ACTIVATED)
        != key_policy_of(&SharedActivated { key: ACTIVATED.to_vec() }, ACTIVATED)
    {
        seen += 1;
    }

    // (7) A servant that checks its key **does** tell the two keys apart.
    //     Found by running control B — flipping both trait defaults to `false`
    //     while leaving `default_knows_policy()` alone — and watching
    //     `a_key_nobody_activated_cannot_be_told_apart_from_one_that_was` stay
    //     green: under a blanket `false` the activated key is refused too, so
    //     the two answers are still equal and the test is satisfied by a server
    //     that serves nothing. Indistinguishability is only evidence about
    //     transparency alongside a demonstration that distinguishing is
    //     possible at all, and this is that demonstration.
    let checks = serving(Activated { key: ACTIVATED.to_vec() });
    let told_apart = ping(checks.addr, ACTIVATED, Version::V1_2, Endian::Big)
        != ping(checks.addr, NEVER_ACTIVATED, Version::V1_2, Endian::Big);
    checks.shut_down();
    if told_apart {
        seen += 1;
    }

    assert_eq!(seen, 7, "a control stopped moving the counter; the tests above are not evidence");
}

// ─────────────────────────────────────────────────────────────────────────────
// 6 · What is left undone — read out of the tree, never typed
// ─────────────────────────────────────────────────────────────────────────────

/// Who implements `Dispatch`, and which of them override `knows`, **computed**.
///
/// This module exists because the list it replaces was typed. The typed one
/// held `orbweaver-gen/src/pyservant.rs` as a production inheritor, and on
/// 2026-08-26 that was true; the seam refactor then moved the servant to
/// `crate::seam::ForeignServant`, **which overrides `knows`**, leaving
/// `pyservant.rs` a 24-line re-export with no `Dispatch` impl in it at all. The
/// guard beside the list was `!PRODUCTION_INHERITORS.is_empty()`, a property of
/// a literal three lines above it, so it could not notice — and neither could a
/// path-existence check, because the file still exists. It is the *property*
/// that moved out from under the name. That is CLAUDE.md's rule about a control
/// that names a live subject, arriving in a roster rather than in a control.
///
/// The parsing is deliberately small but not naive, because this tree contains
/// both of the decoys a bare grep falls for: a `code.contains("impl
/// SharedDispatch for NamingServer")` assertion in
/// `naming_no_outbound_call.rs`, and `skeleton.rs`'s `writeln!` template that
/// emits `impl<S: {servant}> __rt::Dispatch for {skel}<S>`. Neither is an
/// implementation. Comments, strings and char literals are blanked first, so
/// they are not, and so a brace inside a literal cannot close an impl body.
/// Matching `fn knows` only inside the brace-matched body of the impl is what
/// keeps the *generated servant trait's* own `knows` — a different trait, in
/// the same files — from counting.
mod roster {
    use std::path::{Path, PathBuf};

    /// One `impl … Dispatch for …` found in the tree.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct DispatchImpl {
        /// Path relative to the workspace root, `/`-separated.
        pub file: String,
        pub line: usize,
        /// The type the trait is implemented for, as written.
        pub subject: String,
        pub overrides_knows: bool,
        /// Inherits `knows` but checks the key by another route — `Poa`'s
        /// `dispatch_target` or `parse_key` — so the request path refuses while
        /// the probe path, which only ever consults `knows`, still answers
        /// `ObjectHere`. This is the disagreement
        /// `the_probe_path_and_the_request_path_agree_about_an_unactivated_key`
        /// exists to catch.
        pub checks_in_another_hook: bool,
        /// `knows`'s whole body is `true`: it never reads the key it is given.
        ///
        /// **This is the population D029 §6.1's Backend row is about after
        /// D036, and it is not the one that row was written against.** Before
        /// D036 the leak was spelled *inheriting a permissive default* and the
        /// cell counted inheritors; the default is gone and inheriting no
        /// longer compiles, so the same leak is now spelled *writing `true`*.
        /// D036 says so in as many words — a servant that writes it leaks
        /// exactly as one that inherited it did — which is why retiring the
        /// inheritor count without putting this one in its place would have
        /// been a row quietly losing its leak rather than closing it.
        ///
        /// A **lower bound**, in one direction only: every unconditional
        /// `true` answers for a key nobody activated, but a `knows` that
        /// consults something can still answer for a superset of the keys its
        /// servant holds, and nothing here sees that.
        pub answers_unconditionally: bool,
        /// `knows` has a branch whose whole answer is `true` — a `match` arm, an
        /// `if`/`else` side, or the body itself.
        ///
        /// **The superset case, and the reason it is a separate field.**
        /// [`answers_unconditionally`] asks whether the body IS `true`, which
        /// D029 §6.1's Backend cell named as its own lower bound: *a `knows`
        /// that consults something can still answer for a superset of the keys
        /// its servant holds, and nothing here sees that.* This is what sees
        /// part of it. Measured 2026-08-31, one deployable servant matched and
        /// it was not a fixture: `seam::ForeignServant::knows` reads
        /// `match &self.identity { Some(i) => …, None => true }`, so one built
        /// without a home answers for every key a caller can fabricate.
        ///
        /// A **lower bound too**, and in the same direction: a `knows` that
        /// answers from a set which is merely wider than the servant's holds no
        /// `true` literal at all and is invisible here. What this closes is the
        /// gap between *the body is `true`* and *some path is `true`*, which is
        /// where the one real instance was hiding.
        pub answers_true_on_some_path: bool,
        /// The impl is not compiled into any artifact a deployment runs: it
        /// sits inside a `#[cfg(test)]` item, or its file is a cargo test or
        /// bench target.
        ///
        /// **A fact about what the compiler emits, not a judgement about which
        /// servants are "production"** — and that distinction is why the field
        /// exists at all. This file's own documentation refuses the judgement
        /// for a good reason: *deciding which servants are production by
        /// matching path substrings would put the same hand-typed classifier
        /// back one layer down.* `#[cfg(test)]` is not that classifier; it is
        /// the condition under which the code is emitted, so a reader
        /// disputing it is disputing `rustc` rather than somebody's taste in
        /// directory names.
        pub test_only: bool,
    }

    #[derive(Debug, Default)]
    pub struct Scan {
        pub files_read: usize,
        pub impls: Vec<DispatchImpl>,
    }

    impl Scan {
        pub fn overriders(&self) -> Vec<&DispatchImpl> {
            self.impls.iter().filter(|i| i.overrides_knows).collect()
        }
        pub fn inheritors(&self) -> Vec<&DispatchImpl> {
            self.impls.iter().filter(|i| !i.overrides_knows).collect()
        }
        pub fn wrong_hook(&self) -> Vec<&DispatchImpl> {
            self.impls.iter().filter(|i| i.checks_in_another_hook).collect()
        }
        /// Every servant whose `knows` never reads the key.
        pub fn unconditional(&self) -> Vec<&DispatchImpl> {
            self.impls.iter().filter(|i| i.answers_unconditionally).collect()
        }
        /// Every servant with a branch that answers `true` without reading it.
        pub fn true_on_some_path(&self) -> Vec<&DispatchImpl> {
            self.impls.iter().filter(|i| i.answers_true_on_some_path).collect()
        }
        /// The same, restricted to what a build emits — the reachable set.
        pub fn true_on_some_path_in_a_build(&self) -> Vec<&DispatchImpl> {
            self.impls.iter().filter(|i| i.answers_true_on_some_path && !i.test_only).collect()
        }
        /// The same, restricted to impls a build actually emits — which is the
        /// set a caller could ever reach, and therefore the one the Backend row
        /// is a claim about.
        pub fn unconditional_in_a_build(&self) -> Vec<&DispatchImpl> {
            self.impls.iter().filter(|i| i.answers_unconditionally && !i.test_only).collect()
        }
    }

    /// Refuse, rather than report a clean tree.
    ///
    /// Every clause is the same lesson one level up from CLAUDE.md's ledger
    /// control: *a green that means nothing occurred reads exactly like a green
    /// that means the property held.* A scan that read no files, parsed no
    /// impls, or classified every impl the same way has produced no evidence
    /// about `knows` at all and must not come back quiet.
    ///
    /// `overriders().is_empty()` is the strip-removed-nothing clause: before
    /// either answer means anything, the classifier has to be shown answering
    /// **both** ways on this tree. `inheritors().is_empty()` is loud for the
    /// opposite reason — it may well be good news, but "the finding is closed"
    /// is a conclusion a person retires this test for, not one a scan reaches
    /// by going quiet.
    pub fn verdict(scan: &Scan) -> Result<(), String> {
        if scan.files_read == 0 {
            return Err("the roster scan read no .rs files at all: it is not measuring the \
                        workspace, and its silence is not evidence about `knows`"
                .into());
        }
        if scan.impls.is_empty() {
            return Err(format!(
                "the roster scan read {} files and parsed no `Dispatch` impl out of any of \
                 them. Either the trait was renamed or the parser broke; either way this test \
                 is measuring nothing",
                scan.files_read
            ));
        }
        if scan.overriders().is_empty() {
            return Err(format!(
                "the roster scan found {} `Dispatch` impls and says not one of them overrides \
                 `knows`. A classifier that only ever gives one answer is not evidence for \
                 that answer — see CLAUDE.md, indistinguishability",
                scan.impls.len()
            ));
        }
        // **These two clauses used to run the other way, and flipping them is
        // the retirement this scan's own guard demanded.** They said: a scan
        // that reports no inheritors, or no inheritor checking in another
        // hook, has either found the finding CLOSED — *retire the test
        // deliberately and record it* — or broken, and neither may pass
        // quietly. On 2026-08-29 the first reading became true: D036 made
        // `knows` required, so a `Dispatch` with no `knows` does not compile,
        // and the two `spikes/` servants that checked in `dispatch_body` moved
        // their check into `knows` in the same batch. The guard was obeyed
        // rather than deleted: what was an error is now the expectation, and
        // what was the expectation is now the error.
        if !scan.inheritors().is_empty() {
            return Err(format!(
                "the roster scan says {} of {} `Dispatch` impls inherit a `knows`. D036 made \
                 that method required, so such an impl does not compile — this is the SCAN \
                 reporting something the compiler already refused, or the trait having grown \
                 a default again. Neither is allowed to pass quietly",
                scan.inheritors().len(),
                scan.impls.len()
            ));
        }
        // **The population D036 left behind, and the same guard over it.**
        // `answers_unconditionally` is what the Backend row counts now, so it
        // owes the same two refusals `overrides_knows` owes: a classifier that
        // has only ever given one answer on this tree is not evidence for that
        // answer, in either direction.
        if scan.unconditional().is_empty() {
            return Err(format!(
                "the roster scan found {} `Dispatch` impls and says not one of them answers an                  unconditional `true`. If that is true the Backend leak is closed in this tree                  and D029 §6.1's row should be MOVED by a person, not discovered by a scan                  going quiet; if it is not true the body test broke. Neither reading is                  allowed to be a silent pass",
                scan.impls.len()
            ));
        }
        if scan.unconditional().len() == scan.impls.len() {
            return Err(format!(
                "the roster scan says all {} `Dispatch` impls answer an unconditional `true`.                  A classifier that only ever gives one answer is not evidence for that answer                  — see CLAUDE.md, indistinguishability",
                scan.impls.len()
            ));
        }
        // And the same for the reachability half, which is the one that turns
        // a count into a claim about what a caller could dial.
        if scan.impls.iter().all(|i| i.test_only) || !scan.impls.iter().any(|i| i.test_only) {
            return Err(format!(
                "the roster scan put all {} `Dispatch` impls on one side of `#[cfg(test)]`.                  This workspace has servants on both sides, so the reachability classifier is                  stuck and `unconditional_in_a_build()` means nothing",
                scan.impls.len()
            ));
        }
        Ok(())
    }

    fn is_ident(b: u8) -> bool {
        b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
    }

    fn utf8_len(b: u8) -> usize {
        if b < 0x80 {
            1
        } else if b >> 5 == 0b110 {
            2
        } else if b >> 4 == 0b1110 {
            3
        } else {
            4
        }
    }

    /// Replace every comment, string, byte-string and char literal with spaces,
    /// preserving byte offsets and newlines so line numbers still hold.
    ///
    /// Lifetimes are code and stay: `'a` is told from `'a'` by looking for a
    /// closing quote one scalar along.
    pub fn blank_noncode(src: &str) -> String {
        let b = src.as_bytes();
        let n = b.len();
        let mut out = b.to_vec();
        let mut i = 0usize;
        let blank = |out: &mut Vec<u8>, from: usize, to: usize| {
            for byte in out.iter_mut().take(to.min(n)).skip(from) {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
        };
        while i < n {
            if b[i] == b'/' && i + 1 < n && b[i + 1] == b'/' {
                let s = i;
                while i < n && b[i] != b'\n' {
                    i += 1;
                }
                blank(&mut out, s, i);
                continue;
            }
            if b[i] == b'/' && i + 1 < n && b[i + 1] == b'*' {
                let s = i;
                let mut depth = 1usize;
                i += 2;
                while i < n && depth > 0 {
                    if b[i] == b'/' && i + 1 < n && b[i + 1] == b'*' {
                        depth += 1;
                        i += 2;
                    } else if b[i] == b'*' && i + 1 < n && b[i + 1] == b'/' {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                blank(&mut out, s, i);
                continue;
            }
            // An identifier is consumed whole, so that the `r` in `for` or the
            // `b` in `body` is never read as a literal prefix.
            if is_ident(b[i]) && !b[i].is_ascii_digit() {
                let s = i;
                while i < n && is_ident(b[i]) {
                    i += 1;
                }
                let word = &b[s..i];
                if word == b"r" || word == b"br" {
                    let lit = i;
                    let mut hashes = 0usize;
                    while i < n && b[i] == b'#' {
                        hashes += 1;
                        i += 1;
                    }
                    // `r#type` is a raw identifier, not a raw string: only a
                    // quote after the hashes makes this a literal.
                    if i < n && b[i] == b'"' {
                        i += 1;
                        while i < n {
                            if b[i] == b'"' {
                                let mut k = i + 1;
                                let mut got = 0usize;
                                while k < n && b[k] == b'#' && got < hashes {
                                    got += 1;
                                    k += 1;
                                }
                                if got == hashes {
                                    i = k;
                                    break;
                                }
                            }
                            i += 1;
                        }
                        blank(&mut out, lit, i);
                    }
                }
                continue;
            }
            if b[i] == b'"' {
                let s = i;
                i += 1;
                while i < n {
                    if b[i] == b'\\' {
                        i += 2;
                        continue;
                    }
                    if b[i] == b'"' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                blank(&mut out, s, i);
                continue;
            }
            if b[i] == b'\'' {
                let is_char = if i + 1 < n && b[i + 1] == b'\\' {
                    true
                } else {
                    let mut k = i + 1;
                    if k < n {
                        k += utf8_len(b[k]);
                    }
                    k < n && b[k] == b'\''
                };
                if is_char {
                    let s = i;
                    i += 1;
                    while i < n {
                        if b[i] == b'\\' {
                            i += 2;
                            continue;
                        }
                        if b[i] == b'\'' {
                            i += 1;
                            break;
                        }
                        i += 1;
                    }
                    blank(&mut out, s, i);
                } else {
                    i += 1; // a lifetime, which is code
                }
                continue;
            }
            i += 1;
        }
        String::from_utf8(out).expect("whole bytes were replaced, so this is still UTF-8")
    }

    fn word_at(b: &[u8], at: usize, word: &[u8]) -> bool {
        b.len() >= at + word.len()
            && &b[at..at + word.len()] == word
            && (at == 0 || !is_ident(b[at - 1]))
            && b.get(at + word.len()).map(|c| !is_ident(*c)).unwrap_or(true)
    }

    /// `fn <name>` anywhere in this body.
    fn declares_fn(body: &[u8], name: &[u8]) -> bool {
        let mut i = 0usize;
        while i < body.len() {
            if word_at(body, i, b"fn") {
                let mut j = i + 2;
                while j < body.len() && (body[j] as char).is_whitespace() {
                    j += 1;
                }
                if word_at(body, j, name) {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// The brace-matched body of `fn <name>` inside `body`, braces excluded.
    ///
    /// `declares_fn` above answers *is it there*; this answers *what does it
    /// say*, and the two are kept apart because a required method in a trait
    /// declaration has the first and not the second.
    fn fn_body<'a>(body: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
        let mut i = 0usize;
        while i < body.len() {
            if word_at(body, i, b"fn") {
                let mut j = i + 2;
                while j < body.len() && (body[j] as char).is_whitespace() {
                    j += 1;
                }
                if word_at(body, j, name) {
                    // The first `{` at nesting zero after the signature opens
                    // the body; a `;` first means there is none.
                    let mut k = j + name.len();
                    let (mut angle, mut paren) = (0i32, 0i32);
                    while k < body.len() {
                        match body[k] {
                            b'<' => angle += 1,
                            b'>' => angle -= 1,
                            b'(' | b'[' => paren += 1,
                            b')' | b']' => paren -= 1,
                            b';' if angle <= 0 && paren <= 0 => return None,
                            b'{' if angle <= 0 && paren <= 0 => break,
                            _ => {}
                        }
                        k += 1;
                    }
                    if k >= body.len() {
                        return None;
                    }
                    let open = k;
                    let mut depth = 0i32;
                    while k < body.len() {
                        if body[k] == b'{' {
                            depth += 1;
                        } else if body[k] == b'}' {
                            depth -= 1;
                            if depth == 0 {
                                return Some(&body[open + 1..k]);
                            }
                        }
                        k += 1;
                    }
                    return None;
                }
            }
            i += 1;
        }
        None
    }

    /// Byte ranges of every item guarded by `#[cfg(test)]`.
    ///
    /// The attribute's item is taken as *everything up to the end of the next
    /// brace-matched block*, which covers the `mod tests { … }` this workspace
    /// uses everywhere and is why `server.rs`'s eleven fixture servants — all
    /// of them below its line-2196 `#[cfg(test)]` — are not counted as
    /// something a deployment runs.
    ///
    /// `#[cfg(all(test, …))]` and `#[cfg_attr(test, …)]` are **not** matched,
    /// deliberately: this returns spans it is sure about, so a miss classifies
    /// an impl as reachable, which is the direction that goes red rather than
    /// quiet.
    fn cfg_test_spans(code: &str) -> Vec<(usize, usize)> {
        let b = code.as_bytes();
        let n = b.len();
        let needle = b"#[cfg(test)]";
        let mut spans = Vec::new();
        let mut i = 0usize;
        while i + needle.len() <= n {
            if &b[i..i + needle.len()] == needle {
                let mut k = i + needle.len();
                while k < n && b[k] != b'{' && b[k] != b';' {
                    k += 1;
                }
                if k < n && b[k] == b'{' {
                    let open = k;
                    let mut depth = 0i32;
                    while k < n {
                        if b[k] == b'{' {
                            depth += 1;
                        } else if b[k] == b'}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        k += 1;
                    }
                    spans.push((open, k.min(n)));
                }
                i = k.max(i + needle.len());
                continue;
            }
            i += 1;
        }
        spans
    }

    /// Whether a path is a cargo target that only a test run builds.
    ///
    /// Cargo's own layout rule, not a naming convention: `tests/` and
    /// `benches/` directly beneath a package root are test and bench targets.
    /// The package root is located by walking up to the directory holding a
    /// `Cargo.toml`, so `src/tests/mod.rs` — which *is* compiled into the
    /// library — is not mistaken for one.
    fn is_test_target(rel: &str, root: &Path) -> bool {
        let parts: Vec<&str> = rel.split('/').collect();
        for (i, part) in parts.iter().enumerate() {
            if *part != "tests" && *part != "benches" {
                continue;
            }
            if i == 0 {
                return true;
            }
            let pkg: PathBuf = root.join(parts[..i].join("/"));
            if pkg.join("Cargo.toml").is_file() {
                return true;
            }
        }
        false
    }

    fn strip_where(s: &str) -> &str {
        let b = s.as_bytes();
        let mut i = 0usize;
        while i < b.len() {
            if word_at(b, i, b"where") {
                return s[..i].trim_end();
            }
            i += 1;
        }
        s
    }

    /// What `at_impl` reads off one `impl … Dispatch for …`, before the file's
    /// `#[cfg(test)]` spans decide whether a build emits it.
    struct Found {
        subject: String,
        overrides: bool,
        elsewhere: bool,
        unconditional: bool,
        true_on_some_path: bool,
        /// Offset of the impl's opening brace, for the span test.
        at: usize,
    }

    fn at_impl(code: &str, start: usize) -> Option<Found> {
        let b = code.as_bytes();
        let n = b.len();
        let mut i = start + 4;
        while i < n && (b[i] as char).is_whitespace() {
            i += 1;
        }
        // impl generics, if any
        if i < n && b[i] == b'<' {
            let mut d = 0i32;
            while i < n {
                match b[i] {
                    b'<' => d += 1,
                    b'>' => {
                        d -= 1;
                        if d == 0 {
                            i += 1;
                            break;
                        }
                    }
                    b'{' | b';' => return None,
                    _ => {}
                }
                i += 1;
            }
        }
        let head = i;
        let mut d = 0i32;
        let mut p = 0i32;
        let mut for_at: Option<usize> = None;
        let mut brace: Option<usize> = None;
        let mut j = i;
        while j < n {
            match b[j] {
                b'<' => d += 1,
                b'>' => d -= 1,
                b'(' | b'[' => p += 1,
                b')' | b']' => p -= 1,
                b'{' if d <= 0 && p <= 0 => {
                    brace = Some(j);
                    break;
                }
                b';' if d <= 0 && p <= 0 => return None,
                b'f' if d == 0 && p == 0 && for_at.is_none() && word_at(b, j, b"for") => {
                    for_at = Some(j);
                }
                _ => {}
            }
            j += 1;
        }
        let brace = brace?;
        let for_at = for_at?;
        // The trait path is what stands between the impl generics and `for`.
        let path = code[head..for_at].trim();
        let last = path.rsplit("::").next().unwrap_or(path);
        let last = last.split('<').next().unwrap_or(last).trim();
        if last != "Dispatch" && last != "SharedDispatch" {
            return None;
        }
        let subject = strip_where(code[for_at + 3..brace].trim()).trim().to_string();

        let mut depth = 0i32;
        let mut k = brace;
        let mut end = n;
        while k < n {
            if b[k] == b'{' {
                depth += 1;
            } else if b[k] == b'}' {
                depth -= 1;
                if depth == 0 {
                    end = k;
                    break;
                }
            }
            k += 1;
        }
        let body = &b[brace..end.min(n)];
        let overrides = declares_fn(body, b"knows");
        // Comments were blanked to spaces before this ran, so a body that is
        // nothing but `true` trims to exactly that. The test is on the whole
        // body rather than on a `contains`, because `k == KEY || true` and
        // `if x { true } else { … }` both contain it and neither is this.
        let knows_body = fn_body(body, b"knows").map(|b| String::from_utf8_lossy(b).into_owned());
        let unconditional =
            knows_body.as_deref().map(|t| matches!(t.trim(), "true" | "true;")).unwrap_or(false);
        // A branch whose whole answer is `true`: a `match` arm (`=> true,`), an
        // `if`/`else` side (`{ true }`), or the body itself. Comments were
        // blanked to spaces before this ran, so a `true` in prose is invisible;
        // `k == KEY || true` is deliberately NOT matched, because the `true`
        // there is an operand rather than a branch's whole answer and catching
        // it would make this flag every short-circuit in the tree.
        let true_on_some_path = knows_body
            .as_deref()
            .map(|t| {
                unconditional
                    || t.contains("=> true,")
                    || t.contains("=> true }")
                    || t.contains("{ true }")
                    || t.split_whitespace()
                        .collect::<Vec<_>>()
                        .windows(2)
                        .any(|w| w == ["{", "true"] || w == ["true", "}"])
            })
            .unwrap_or(false);
        // **The wrong-hook hunt, widened to the population D036 left behind.**
        // It used to require `!overrides`, which made it a subset of the
        // inheritors — and when D036 emptied that set the hunt went vacuous
        // while the class it names stayed open. This file said so and said the
        // class "would need a different scan"; it needs the same scan asking
        // about the body instead of about the absence of one. A servant that
        // says `true` here and consults the POA on the request path has a
        // §9.4.5 probe answering `ObjectHere` for a key it would refuse a call
        // on, whether it wrote that `true` or inherited it.
        let elsewhere = (!overrides || unconditional)
            && (code[brace..end.min(n)].contains("dispatch_target")
                || code[brace..end.min(n)].contains("parse_key"));
        Some(Found { subject, overrides, elsewhere, unconditional, true_on_some_path, at: brace })
    }

    /// `root` decides only whether `rel` is a cargo test target; pass
    /// `Path::new("")` when scanning source that is not on disk, and every impl
    /// is then classified by `#[cfg(test)]` alone.
    pub fn scan_source_at(rel: &str, src: &str, root: &Path, out: &mut Vec<DispatchImpl>) {
        let code = blank_noncode(src);
        let b = code.as_bytes();
        let spans = cfg_test_spans(&code);
        let target_only = is_test_target(rel, root);
        let mut i = 0usize;
        while i < b.len() {
            if word_at(b, i, b"impl")
                && let Some(found) = at_impl(&code, i)
            {
                out.push(DispatchImpl {
                    file: rel.to_string(),
                    line: 1 + b[..i].iter().filter(|c| **c == b'\n').count(),
                    subject: found.subject,
                    overrides_knows: found.overrides,
                    checks_in_another_hook: found.elsewhere,
                    answers_unconditionally: found.unconditional,
                    answers_true_on_some_path: found.true_on_some_path,
                    test_only: target_only
                        || spans.iter().any(|(s, e)| found.at > *s && found.at < *e),
                });
            }
            i += 1;
        }
    }

    /// `scan_source_at` with no package root, for source written in a test.
    pub fn scan_source(rel: &str, src: &str, out: &mut Vec<DispatchImpl>) {
        scan_source_at(rel, src, Path::new(""), out)
    }

    pub fn workspace_root() -> Result<PathBuf, String> {
        let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        loop {
            if let Ok(text) = std::fs::read_to_string(d.join("Cargo.toml"))
                && text.contains("[workspace]")
            {
                return Ok(d);
            }
            if !d.pop() {
                return Err(format!(
                    "no Cargo.toml with a [workspace] table above {}: the roster cannot be \
                     computed and must not report a clean tree",
                    env!("CARGO_MANIFEST_DIR")
                ));
            }
        }
    }

    pub fn scan_tree(root: &Path) -> Scan {
        let mut scan = Scan::default();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                let p = e.path();
                let Some(name) = p.file_name().and_then(|s| s.to_str()) else { continue };
                if p.is_dir() {
                    // `target/` holds build output, and a dot directory holds
                    // another checkout of this same tree.
                    if name == "target" || name.starts_with('.') {
                        continue;
                    }
                    stack.push(p);
                } else if name.ends_with(".rs") {
                    let Ok(src) = std::fs::read_to_string(&p) else { continue };
                    scan.files_read += 1;
                    let rel = p.strip_prefix(root).unwrap_or(&p).to_string_lossy().into_owned();
                    scan_source_at(&rel.replace('\\', "/"), &src, root, &mut scan.impls);
                }
            }
        }
        scan.impls.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
        scan
    }
}

/// **A roster that changed jobs twice, and is named for the second one.**
///
/// It was `the_inheritors_of_the_default_are_named_where_a_change_would_be_made`
/// until 2026-08-31. That name described a set the compiler has kept empty
/// since D036, and a test named for a set that cannot be non-empty tells its
/// next reader it is asserting the compiler — which one of its assertions still
/// is, and says so. The rest of it measures something else now.
///
/// It used to hand whoever changed `default_knows_policy()` today's list of
/// inheritors at the point of change. **There is no default to change any
/// more** — D036 (approved 2026-08-29) made `knows` required — so *inheriting*
/// is now unrepresentable rather than detectable, and the compiler enforces
/// what this test used to enumerate. Asserting it is empty is asserting the
/// compiler, which is why the assertion below says so rather than pretending
/// to measure it.
///
/// What the compiler does **not** enforce is the other half, and that is this
/// test's job now: a servant that writes `true` here while checking the key in
/// `dispatch_body`. Its §9.4.5 probe then answers `ObjectHere` for a key it
/// would refuse a call on — the request/probe disagreement the `serve_one`
/// reorder closed for a *moved* key, and which two `spikes/` servants had for
/// an *unknown* one until D036's batch moved their check into `knows`. That
/// class survives D036 and can be written again tomorrow.
///
/// The list that used to be here is why the scan reads the tree rather than a
/// literal: it named
/// `orbweaver-gen/src/pyservant.rs`, the seam refactor moved that servant into
/// `crate::seam::ForeignServant` — which *does* override `knows` — and the
/// guard, `!PRODUCTION_INHERITORS.is_empty()`, went on being green over a
/// literal it had itself been typed beside.
///
/// The blast-radius figures that stood in this file's module documentation —
/// 72 impls, 46 overriding, 26 inheriting — were the reading of **2026-08-26**
/// and are kept there as that dated record rather than maintained. Today's
/// figure is printed by this test (`--nocapture`) and is never retyped, which
/// is the whole point: a count that has to be maintained by hand is a sentence
/// that will drift, and this one already had.
///
/// **The split this file used to refuse, and the reason it is now computed.**
/// This paragraph said the production/fixture split stays prose, because
/// *deciding which servants are "production" by matching path substrings would
/// put the same hand-typed classifier back one layer down*. That refusal was
/// right about path substrings and it left the row with nothing to assert: a
/// count of servants that answer for every key is not a claim about anything
/// until you can say which of them a caller could dial.
///
/// `#[cfg(test)]` is not that classifier. It is the condition under which the
/// code is emitted, so `test_only` is a fact about the build rather than a
/// judgement about naming, and a reader who disputes it is disputing `rustc`.
/// The cargo half — `tests/` and `benches/` directly beneath a package root —
/// is cargo's own layout rule, and it is checked by locating the `Cargo.toml`
/// rather than by matching the word, which is why `src/tests/` is correctly
/// read as library code. The control asserts both halves.
///
/// What stays prose is only what should: which of the fixtures below would
/// matter if something ever served them.
///
/// The ready-made repair exists and is not wired up: `orbweaver_object::Poa`
/// has `parse_key` and `dispatch_target`, and `Target` already distinguishes
/// `Forward` from `Unknown` — which a boolean `knows` cannot, and which is the
/// reason the right hook for a POA-backed servant is `locate` and not `knows`.
#[test]
fn no_servant_a_build_emits_answers_for_a_key_nobody_activated() {
    let root = roster::workspace_root().unwrap_or_else(|why| panic!("{why}"));
    let scan = roster::scan_tree(&root);
    roster::verdict(&scan).unwrap_or_else(|why| panic!("{why}"));

    let inheritors = scan.inheritors();
    let wrong_hook = scan.wrong_hook();
    let unconditional = scan.unconditional();
    let reachable = scan.unconditional_in_a_build();
    println!(
        "roster, computed from {} .rs files under {}: {} Dispatch/SharedDispatch impls, \
         {} override `knows`, {} inherit it, of which {} check the key in another hook; \
         {} answer an unconditional `true`, {} of them in code a build emits",
        scan.files_read,
        root.display(),
        scan.impls.len(),
        scan.overriders().len(),
        inheritors.len(),
        wrong_hook.len(),
        unconditional.len(),
        reachable.len(),
    );
    let superset = scan.true_on_some_path_in_a_build();
    println!(
        "  of those, {} answer `true` on SOME path without reading the key, {} in code a \
         build emits",
        scan.true_on_some_path().len(),
        superset.len(),
    );
    for i in &superset {
        println!(
            "  true-on-a-path  {}:{}  {}   ** a build emits this **",
            i.file, i.line, i.subject
        );
    }
    for i in &unconditional {
        println!(
            "  unconditional  {}:{}  {}{}",
            i.file,
            i.line,
            i.subject,
            if i.test_only { "   (no build emits it)" } else { "   ** a build emits this **" }
        );
    }
    for i in &inheritors {
        println!(
            "  inherits  {}:{}  {}{}",
            i.file,
            i.line,
            i.subject,
            if i.checks_in_another_hook { "   (checks in dispatch_body, not knows)" } else { "" }
        );
    }

    let named = |v: &[&roster::DispatchImpl]| -> String {
        v.iter()
            .map(|i| format!("{}:{} {}", i.file, i.line, i.subject))
            .collect::<Vec<_>>()
            .join("; ")
    };
    // Asserting the compiler, and saying so. After D036 a `Dispatch` with no
    // `knows` does not compile, so this cannot fail for the reason it used to.
    // It is kept because the scan is what would notice if the trait ever grew a
    // default again — and because a reader who finds a non-empty list here
    // learns that the scan itself has broken, not that the tree has.
    assert!(
        inheritors.is_empty(),
        "{} implementation(s) appear to inherit a `knows` that D036 made required. That does \
         not compile, so this is the SCAN reporting something the compiler already refused: {}",
        inheritors.len(),
        named(&inheritors),
    );

    // **`wrong_hook` stopped being a subset of `inheritors`, and stopped being
    // vacuous with it.** It required `!overrides`, so when D036 emptied the
    // inheritors it emptied this too — while the class it names stayed open,
    // which this file recorded and said would need a different scan. It needed
    // the same scan asking about the *body* rather than about the absence of
    // one. A servant that says `true` here and consults the POA on the request
    // path has a §9.4.5 probe answering `ObjectHere` for a key it would refuse a
    // call on, and it is spelled the same whether the `true` was written or
    // inherited.
    //
    // **It runs before the broader assertion below, and that ordering is what
    // keeps it reachable at all.** `wrong_hook` implies `answers_unconditionally`
    // now that inheriting cannot compile, so it is a strict subset: asserted
    // second, it could never have failed without the broader one failing first,
    // and an assertion that cannot be reached is one more green that means
    // nothing happened. Measured — control 2 below fired the wrong message
    // until they were swapped.
    //
    // The hunt finds one today and it is **deliberate**: the test-private
    // `ExpertHost` in `what_a_caller_can_tell_about_load.rs`, whose rustdoc
    // gives the reason — *the object's existence is the POA's decision here, not
    // a second one taken in front of it.* So the assertion is the same shape as
    // the one above rather than `is_empty()`: what must not happen is a servant
    // a build emits having that disagreement. A deliberate fixture is allowed to
    // hold it, and is allowed **because nothing serves it** — which is a
    // property the scan checks, not a name it was told to skip.
    let wrong_hook_in_a_build: Vec<&roster::DispatchImpl> =
        wrong_hook.iter().copied().filter(|i| !i.test_only).collect();
    assert!(
        wrong_hook_in_a_build.is_empty(),
        "{} servant(s) that a build EMITS answer `true` from `knows` while checking the key on \
         the request path, so their probe path answers `ObjectHere` for a key their own POA \
         calls Unknown. The repair is `orbweaver_object::Poa::serves`, the read-only half of \
         `dispatch_target`, which exists for exactly this: {}",
        wrong_hook_in_a_build.len(),
        named(&wrong_hook_in_a_build),
    );
    // **The superset case, measured rather than described.** D029 §6.1's Backend
    // cell named this as its own lower bound — *a `knows` that consults
    // something can still answer for a superset of the keys its servant holds,
    // and nothing here sees that.* Part of it is visible: a `knows` with a
    // branch whose whole answer is `true`. Measured 2026-08-31, the tree holds
    // exactly ONE such servant that a build emits, and it is not a fixture:
    // `seam::ForeignServant`, whose `knows` reads
    // `match &self.identity { Some(i) => …, None => true }`.
    //
    // **Pinned at one, with its name, and the number is not to be edited.** A
    // second such servant is a second endpoint where a caller can fabricate a
    // key and be answered, and what to do about the first is a design question
    // — a `ForeignServant` usually exists before its server binds, so an
    // identity cannot simply be required — which is why it is asked in a
    // decision rather than settled here. A reader who finds this red has added
    // one: name it in that decision, do not widen the number.
    //
    // The bound is a lower one in the same direction as everything else on this
    // row: a `knows` answering from a set merely wider than its servant's holds
    // no `true` literal and is invisible to any of this.
    let superset_known: &[&str] = &["crates/orbweaver-gen/src/seam.rs"];
    let unexpected: Vec<&roster::DispatchImpl> =
        superset.iter().copied().filter(|i| !superset_known.contains(&i.file.as_str())).collect();
    assert!(
        unexpected.is_empty(),
        "{} servant(s) a build emits answer `true` on some path of `knows` without reading \
         the key, beyond the one D029 §6.1's Backend cell names. Each is an endpoint where a \
         caller can fabricate an object key and be answered: {}",
        unexpected.len(),
        named(&unexpected),
    );
    assert_eq!(
        superset.len(),
        1,
        "the Backend row names exactly one such servant and the roster found {}. If one was \
         REMOVED, that is the row moving and D029 §6.1 is what to edit — a count that quietly \
         drops is indistinguishable from a scan that stopped looking: {}",
        superset.len(),
        named(&superset),
    );

    // **The measurement this test carries now, and the one D029 §6.1's Backend
    // row is a claim about.**
    //
    // D036 deleted the default and made every servant state its answer; it
    // closed nothing, and says so. What it did was make the population
    // *nameable*: the leak is no longer "inherits a permissive default", it is
    // "answers `true` without reading the key", and the reachable half of that
    // set is what a caller could ever dial. That set is empty in this tree, and
    // this is the assertion that keeps it empty — the first servant compiled
    // into a binary or a library that answers for every key fails here, by name
    // and line.
    //
    // It is **not** the same claim as "the leak is closed", and the row is not
    // moved on the strength of it. Two things stay open above this line and are
    // written here so a green run cannot be read as more than it is: a `knows`
    // that consults something can still answer for a superset of the keys its
    // servant holds, which this scan cannot see; and every fixture below is a
    // servant that would leak if anything ever served it.
    assert!(
        reachable.is_empty(),
        "{} servant(s) that a build EMITS answer an unconditional `true` from `knows`, so a \
         caller can fabricate any object key at their endpoint and be answered. That is \
         D029 §6.1's Backend leak with a deployment behind it, not a fixture: {}",
        reachable.len(),
        named(&reachable),
    );
}

/// The control for the roster above, which is the half that makes it evidence.
///
/// A computed roster fails differently from a typed one: it does not go stale,
/// it goes **quiet**. A parser that stopped recognising `impl … Dispatch for …`
/// would report a workspace with no inheritors in it, and "no inheritors" is
/// indistinguishable from "the finding is repaired" unless somebody made it
/// impossible to say quietly. So this test does two things, and CLAUDE.md's
/// ledger-control rule is why it is both rather than either:
///
/// * **Synthesise the subject.** The classification is exercised on source
///   written here, not on whatever the tree happens to contain today — so this
///   control cannot itself be invalidated by a servant moving. The synthetic
///   text carries both of the decoys that are really in this workspace (a trait
///   name inside a string literal, and a `writeln!` template that emits one)
///   plus a `fn knows` on a *different* trait, which is the one a bare grep
///   over `spikes/e2e/servant.rs` gets wrong.
/// * **Make the strip refuse when it removed nothing.** `verdict` is fed
///   scans that are broken in each of the five available ways and must return
///   `Err` for every one — and then a scan that is merely unremarkable, which
///   it must **accept**. Without that last row a `verdict` that refused
///   everything would pass this test while measuring nothing, which is the
///   defect one level up.
#[test]
fn the_roster_refuses_to_be_quiet_about_finding_nothing() {
    const SYNTHETIC: &str = r##"
        impl Dispatch for Overrider {
            fn dispatch_body(&mut self) {}
            fn knows(&self, k: &[u8]) -> bool { k == b"x" }
        }
        impl<D: Dispatch> Dispatch for Inheritor<D> where D: Send {
            // fn knows(&self) {}                       <- a comment
            fn dispatch_body(&mut self) { let _ = "fn knows"; let _ = '}'; }
        }
        impl<D: Dispatch> crate::server::SharedDispatch for WrongHook<D> {
            fn dispatch_body(&mut self) { self.poa.dispatch_target(k, None); }
        }
        trait GeneratedServant {
            fn knows(&self, k: &[u8]) -> bool;
        }
        // impl Dispatch for InAComment {}
        fn decoys() {
            let _ = code.contains("impl SharedDispatch for InAStringLiteral");
            let _ = writeln!(s, "impl<S: {servant}> __rt::Dispatch for {skel}<S> {{");
            let _ = r#"impl Dispatch for InARawString { fn knows() {} }"#;
        }
        impl Debug for NotEvenDispatch { fn fmt(&self) {} }
        impl Dispatch for Unconditional {
            fn knows(&self, _k: &[u8]) -> bool { true }
        }
        impl Dispatch for LooksUnconditional {
            // `contains("true")` says yes to both of these and both do read
            // the key, which is why the body is compared whole.
            fn knows(&self, k: &[u8]) -> bool { k == b"x" || true }
        }
        impl Dispatch for AlsoLooksUnconditional {
            fn knows(&self, k: &[u8]) -> bool { if k.is_empty() { true } else { false } }
        }
        #[cfg(test)]
        mod tests {
            impl Dispatch for BehindCfgTest {
                fn knows(&self, _k: &[u8]) -> bool { true }
            }
        }
        impl Dispatch for SupersetOnOnePath {
            // The shape that was really in this tree: it READS the key on one
            // path and answers `true` on another, so `answers_unconditionally`
            // is false and the servant still answers for every key a caller
            // fabricates when the option is `None`.
            fn knows(&self, k: &[u8]) -> bool {
                match &self.home { Some(h) => h.oid_of(k).is_some(), None => true }
            }
        }
    "##;

    let mut found = Vec::new();
    roster::scan_source("synthetic.rs", SYNTHETIC, &mut found);
    let seen: Vec<(&str, bool, bool, bool, bool, bool)> = found
        .iter()
        .map(|i| {
            (
                i.subject.as_str(),
                i.overrides_knows,
                i.checks_in_another_hook,
                i.answers_unconditionally,
                i.test_only,
                i.answers_true_on_some_path,
            )
        })
        .collect();
    assert_eq!(
        seen,
        // The last column is `answers_true_on_some_path`, and the two rows that
        // differ from `answers_unconditionally` are the whole point of adding
        // it: `AlsoLooksUnconditional` has a `{ true }` branch and reads the
        // key, and `SupersetOnOnePath` is the shape that was really in the tree.
        vec![
            ("Overrider", true, false, false, false, false),
            ("Inheritor<D>", false, false, false, false, false),
            ("WrongHook<D>", false, true, false, false, false),
            ("Unconditional", true, false, true, false, true),
            ("LooksUnconditional", true, false, false, false, false),
            ("AlsoLooksUnconditional", true, false, false, false, true),
            ("BehindCfgTest", true, false, true, true, true),
            ("SupersetOnOnePath", true, false, false, false, true),
        ],
        "the roster parser mis-read source written to be read: it either lost an impl, \
         counted a decoy, or put an impl in the wrong class. Anything it reports about the \
         real tree is worth nothing until this row is right"
    );

    // The cargo-target half of `test_only`, which the synthetic source above
    // cannot show because it has no path: the same text, read as a file under
    // a package's `tests/`, puts every impl out of a deployment's reach.
    let root = roster::workspace_root().unwrap_or_else(|why| panic!("{why}"));
    let mut as_target = Vec::new();
    roster::scan_source_at(
        "crates/orbweaver-giop/tests/synthetic.rs",
        SYNTHETIC,
        &root,
        &mut as_target,
    );
    assert!(
        as_target.iter().all(|i| i.test_only),
        "an impl in a package's `tests/` directory was classified as something a build emits. \
         Cargo builds that directory only for a test run, so `unconditional_in_a_build()` \
         would be counting fixtures: {:?}",
        as_target.iter().filter(|i| !i.test_only).map(|i| &i.subject).collect::<Vec<_>>()
    );
    let mut as_src = Vec::new();
    roster::scan_source_at("crates/orbweaver-giop/src/tests/x.rs", SYNTHETIC, &root, &mut as_src);
    assert!(
        as_src.iter().any(|i| !i.test_only),
        "`src/tests/` was read as a cargo test target. It is not one — it is compiled into the \
         library — and a rule that cannot tell those apart is the path-substring classifier \
         this file refuses"
    );

    // Every way a scan can be broken, and the one way it can be ordinary.
    let one = |overrides_knows: bool,
               checks_in_another_hook: bool,
               answers_unconditionally: bool,
               test_only: bool| roster::DispatchImpl {
        file: "synthetic.rs".into(),
        line: 1,
        subject: "Synthetic".into(),
        overrides_knows,
        checks_in_another_hook,
        answers_unconditionally,
        // The control's synthetic rows exercise `verdict`, which does not read
        // this field; the parser rows above are what exercise it.
        answers_true_on_some_path: answers_unconditionally,
        test_only,
    };
    // A scan this workspace could produce and which nothing is wrong with:
    // both `knows` answers present, and servants on both sides of `#[cfg(test)]`.
    let ordinary = || vec![one(true, false, false, false), one(true, false, true, true)];
    let refused: [(&str, roster::Scan); 7] = [
        ("read no files", roster::Scan { files_read: 0, impls: vec![] }),
        ("parsed no impls", roster::Scan { files_read: 400, impls: vec![] }),
        (
            "every impl inherits — the classifier is stuck",
            roster::Scan {
                files_read: 400,
                impls: vec![one(false, true, false, false), one(false, true, false, true)],
            },
        ),
        (
            "an impl inherits, which D036 made impossible to compile",
            roster::Scan {
                files_read: 400,
                impls: vec![one(true, false, false, false), one(false, true, true, true)],
            },
        ),
        (
            "nothing answers unconditionally — either the leak closed or the body test broke",
            roster::Scan {
                files_read: 400,
                impls: vec![one(true, false, false, false), one(true, false, false, true)],
            },
        ),
        (
            "everything answers unconditionally — the body test is stuck",
            roster::Scan {
                files_read: 400,
                impls: vec![one(true, false, true, false), one(true, false, true, true)],
            },
        ),
        (
            "every impl on one side of `#[cfg(test)]` — the reachability half is stuck",
            roster::Scan {
                files_read: 400,
                impls: vec![one(true, false, false, false), one(true, false, true, false)],
            },
        ),
    ];
    for (why, scan) in &refused {
        assert!(
            roster::verdict(scan).is_err(),
            "verdict() accepted a scan that measured nothing: {why}"
        );
    }
    // After D036 the unremarkable scan is one where everything overrides.
    let ok = roster::Scan { files_read: 400, impls: ordinary() };
    assert!(
        roster::verdict(&ok).is_ok(),
        "verdict() refuses every scan it is given, including an unremarkable one. A check that \
         can only say no is not a control, and the seven rows above prove nothing about it: \
         {:?}",
        roster::verdict(&ok)
    );
}
