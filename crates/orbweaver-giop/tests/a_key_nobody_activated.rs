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
//! Every test below computes what a caller *should* see from
//! `server::default_knows_policy()` and then goes and looks on a socket. So the
//! gate is not "an unactivated key is served"; it is **"the wire agrees with
//! the sentence this crate publishes about the wire"**. Change a `knows`
//! default body without changing that function and this goes red; change that
//! function without changing a body and this goes red. Changing both together
//! is green — that is correct, and it is also the dangerous case, which is why
//! `the_inheritors_of_the_default_are_named_where_a_change_would_be_made`
//! carries the list of what else must move.
//!
//! # Blast radius, measured 2026-08-26 — the numbers, not an assumption
//!
//! **72** `Dispatch`/`SharedDispatch` implementations in this workspace. **46**
//! override `knows`; **26** inherit the default.
//!
//! * **12 of 12** hand-written servants in `orbweaver-giop` override it, and
//!   with real checks: `NamingServer` looks the key up in its context tree,
//!   `TradingServer` and the two event servants compare against their key,
//!   `EventChannelServer` routes it through four maps.
//! * **6 of 6** production servants in the sibling crates override it
//!   (`RepositoryServer`, `ExpertService`, `TenantService`).
//! * **16 of 16** emitted skeletons override it, and `orbweaver_gen`'s servant
//!   trait declares its own `knows` **required, with no default** — that layer
//!   already made this decision, the other way, and pinned it.
//!
//! So "checking the key is ceremony" is a claim only the inheritors act on, and
//! **the production inheritors are defects rather than deliberate**: they are
//! named in the test at the bottom of this file, because a crate this one does
//! not own is where the repair has to land.
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
    UnknownKeyPolicy, default_knows_policy, key_policy_of,
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

impl Dispatch for Ambient {
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
    let want = predicted(default_knows_policy());
    for version in EVERY_VERSION {
        for endian in BOTH_ORDERS {
            assert_eq!(
                ping(s.addr, NEVER_ACTIVATED, version, endian),
                want,
                "a key nobody activated, at {version:?}/{endian:?}: the wire disagrees with \
                 server::default_knows_policy(). One of the two moved without the other."
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
        let policy =
            if checks { UnknownKeyPolicy::RefuseAsNotExist } else { default_knows_policy() };
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
    let want = predicted(default_knows_policy());
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
        default_knows_policy(),
        "a servant with no `knows` enacts whatever the trait default enacts, by definition"
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
// 6 · What is left undone, named where a change would be made
// ─────────────────────────────────────────────────────────────────────────────

/// **Not a gate — a list, asserted so it cannot rot silently.**
///
/// The repair for the 26 inheritors cannot land in this crate. This test holds
/// the names so that whoever changes `default_knows_policy()` finds them at the
/// point of change rather than by re-running the sweep, and asserts the list is
/// non-empty so deleting the finding requires deleting the test.
///
/// Measured 2026-08-26. Each entry is a servant that inherits `knows` and is
/// **production rather than fixture**:
///
/// 1. `orbweaver-gen/src/pyservant.rs` — `PyServant<A>`, the Python-servant
///    bridge. No key check anywhere. This is the one that is a live defect
///    rather than a fixture convenience, because it is on the language-
///    transparency path D029 §6.1 calls closed.
/// 2. `orbweaver-object/src/bin/spike_server.rs` — `Echo`, i.e. **spike-server**
///    itself, the fixture omniORB and JacORB are pointed at. Every gate that
///    measures `OBJECT_NOT_EXIST` behaviour against it is measuring a servant
///    that accepts all keys.
/// 3. `spikes/e2e/servant.rs` and `spikes/estate/servant.rs` — `PoaFront<D>`.
///    These *do* check, through `poa.dispatch_target`, but in `dispatch_body`
///    and not in `knows`. So their request path refuses and their **probe path
///    still answers `ObjectHere`** — the request/probe disagreement
///    `the_probe_path_and_the_request_path_agree_about_an_unactivated_key`
///    exists to catch, live in the tree today, in two files.
/// 4. `orbweaver-giop/src/bin/` — `spike_mux`, `spike_nat`, `spike_orb_shutdown`;
///    and `orbweaver-object/src/bin/spike_wide.rs`,
///    `orbweaver-registry/src/bin/spike_ingest.rs` (`TrackManager`). Fixtures.
///
/// The ready-made repair exists and is not wired up: `orbweaver_object::Poa`
/// has `parse_key` and `dispatch_target`, and `Target` already distinguishes
/// `Forward` from `Unknown` — which a boolean `knows` cannot, and which is the
/// reason the right hook for a POA-backed servant is `locate` and not `knows`.
/// **No implementation in the workspace calls it from either.**
#[test]
fn the_inheritors_of_the_default_are_named_where_a_change_would_be_made() {
    /// Kept as data rather than only as prose so the count is checkable.
    const PRODUCTION_INHERITORS: [&str; 2] =
        ["orbweaver-gen/src/pyservant.rs", "orbweaver-object/src/bin/spike_server.rs"];
    /// Servants that check the key in `dispatch_body` instead of `knows`, so
    /// their probe path and request path disagree.
    const CHECK_IN_THE_WRONG_HOOK: [&str; 2] =
        ["spikes/e2e/servant.rs", "spikes/estate/servant.rs"];

    assert!(
        !PRODUCTION_INHERITORS.is_empty() && !CHECK_IN_THE_WRONG_HOOK.is_empty(),
        "the finding was emptied rather than repaired"
    );
    assert_eq!(
        default_knows_policy(),
        UnknownKeyPolicy::ServeAnyway,
        "default_knows_policy() moved. Before this lands, the servants listed in this test's \
         documentation inherit the old default and must be given a `knows` of their own: \
         {PRODUCTION_INHERITORS:?}, plus the wrong-hook pair {CHECK_IN_THE_WRONG_HOOK:?} and \
         the fixture binaries the docs name."
    );
}
