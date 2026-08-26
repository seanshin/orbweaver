//! What a permanent forward means for the **other handles** on one reference.
//!
//! `forward_chain.rs` measures one `Reference` walking a chain. This measures
//! what happens to its clones, which is where §9.6 stops being about a call
//! and starts being about an object: `LOCATION_FORWARD_PERM` is the servant
//! saying *the object moved*, and if two handles on one reference can disagree
//! about that, one of them dials an address the servant has superseded.
//!
//! The disagreement is silent by construction. §9.6 leaves the old address
//! valid, so the stale handle's calls are **answered** — nothing errors,
//! nothing logs, and the only symptom is a forward per call that no test can
//! go red on. That is why the cost is counted at the peer here rather than
//! asserted at the client: the count is the only thing the two behaviours
//! differ in.
//!
//! The pattern that pays it is the one [`Invoker`] asks for. `invoke` takes
//! `&mut self`, so a reference used from more than one caller must be cloned —
//! and a template that is cloned per call never makes a call itself, so under
//! per-handle re-pointing it never learns, and every clone starts from the
//! address the object left. `a_reference_cloned_per_call_pays_the_forward_once`
//! is that measurement.
//!
//! And the boundary of that sharing, measured in the same shape rather than
//! described: two `Pool::reference` calls for one IOR are two references and
//! neither hears the other's forward —
//! `two_references_to_one_object_each_pay_the_forward_once`. The number it
//! reports is what `docs/decisions/D013-*.md` decides on.
//!
//! The peer is scripted TCP built out of this crate's own encoders — a
//! self-test of a decoded property, not an interop result, the same posture
//! `forward_chain.rs` and `mux_pool.rs` take. The axis varied is the **reply's**
//! byte order, for `mux_pool.rs`'s reason: the pool dials with the connection's
//! own order, so a scripted peer can vary what this client *decodes* and
//! nothing can vary what it encodes.

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_giop::guarded::{Guarded, complaints_about};
use orbweaver_giop::orb::Orb;
use orbweaver_giop::pool::Reference;
use orbweaver_giop::server::{Request, decode_request, encode_location_forward, encode_reply};
use orbweaver_giop::{
    DEFAULT_MAX_MESSAGE_SIZE, Forward, IiopProfile, Invoker, Ior, ReplyStatus, Version,
    read_message,
};
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// Every wait answers to this. A test that can hang is not a test.
const T: Duration = Duration::from_secs(10);

/// How long a peer stays on its connection waiting for a request that may not
/// come.
///
/// Both peers here are counting, and a count only means something once the
/// window in which it could still change has closed — so every script ends on
/// a lapsed read deadline rather than on an expected number of requests. A
/// script that stopped after N requests would be asserting N by hanging, and
/// the whole subject of this file is a difference in N.
///
/// It is a read deadline, so the wait sleeps in the kernel rather than
/// spinning, and it bounds the runtime: one window per byte order per test.
const WATCH: Duration = Duration::from_millis(1200);

/// What the object answers, wherever it is answering from.
const ANSWER: i32 = 7;

fn ior_at(addr: SocketAddr, key: &[u8]) -> Ior {
    Ior {
        type_id: "IDL:test/Moved:1.0".into(),
        profiles: vec![IiopProfile {
            // 1.2, because status 4 — `LOCATION_FORWARD_PERM` — is a 1.2 word:
            // below it a permanent redirect travels as status 3 and there is
            // no permanent hop for a clone to miss.
            version: Version::V1_2,
            host: addr.ip().to_string(),
            port: addr.port(),
            object_key: key.to_vec(),
            components: Vec::new(),
        }],
    }
}

fn reply_long(s: &mut TcpStream, req: &Request, endian: Endian, value: i32) {
    let msg = encode_reply(
        req.version,
        endian,
        req.request_id,
        ReplyStatus::NoException,
        None,
        |e: &mut Encoder| e.put_i32(value),
    )
    .expect("reply encodes");
    s.write_all(&msg).expect("reply goes out");
    s.flush().expect("flush");
}

fn forward_to(s: &mut TcpStream, req: &Request, endian: Endian, to: &Forward) {
    let msg = encode_location_forward(req.version, endian, req.request_id, to).expect("encodes");
    s.write_all(&msg).expect("forward goes out");
    s.flush().expect("flush");
}

/// A peer whose script the test writes; the channel says it finished, so a
/// failing script fails the test instead of hanging it.
fn scripted<F>(script: F) -> (SocketAddr, mpsc::Receiver<()>)
where
    F: FnOnce(TcpListener) + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        script(listener);
        let _ = tx.send(());
    });
    (addr, rx)
}

fn body_i32(reply: &orbweaver_giop::Reply) -> i32 {
    reply.body().expect("body").get_i32().expect("a long")
}

/// Host, port and object key — what "which peer answered, addressed how" comes
/// down to. Compared instead of whole IORs so the assertion is about where the
/// reference points and not about an IOR round-tripping field for field.
fn addressed(ior: &Ior) -> (String, u16, Vec<u8>) {
    let p = ior.primary().expect("a profile");
    (p.host.clone(), p.port, p.object_key.clone())
}

/// Where the object has moved to: answers every request it is given with
/// [`ANSWER`] and counts them.
///
/// One connection, because the pool keys on the endpoint and every handle here
/// shares one pool — a script that accepted twice would be asserting a dial
/// per handle, which is the pooling this crate already measures elsewhere.
fn landing(reply_endian: Endian, seen: Arc<AtomicUsize>) -> (SocketAddr, mpsc::Receiver<()>) {
    scripted(move |l| {
        let (mut s, _) = l.accept().expect("the object's new home is called at least once");
        s.set_read_timeout(Some(WATCH)).expect("a deadline on the watch");
        while let Ok(msg) = read_message(&mut s, DEFAULT_MAX_MESSAGE_SIZE) {
            let req = decode_request(msg).expect("a request decodes");
            assert_eq!(req.object_key, b"new", "the landing is addressed by its own key");
            seen.fetch_add(1, Ordering::SeqCst);
            reply_long(&mut s, &req, reply_endian, ANSWER);
        }
    })
}

/// The address the object has left: forwards **every** request it is given —
/// which is what a servant that has moved an object does, not just for the
/// first caller — and counts them.
///
/// The count is the measurement. A peer that forwarded once and then hung up
/// would make the two behaviours differ in an error rather than in a number,
/// and §9.6's point is that they do not: the old address stays valid, so the
/// stale handle is *served*, expensively.
fn left_behind(
    to: Ior,
    for_good: bool,
    reply_endian: Endian,
    seen: Arc<AtomicUsize>,
) -> (SocketAddr, mpsc::Receiver<()>) {
    scripted(move |l| {
        let (mut s, _) = l.accept().expect("the first call");
        s.set_read_timeout(Some(WATCH)).expect("a deadline on the watch");
        while let Ok(msg) = read_message(&mut s, DEFAULT_MAX_MESSAGE_SIZE) {
            let req = decode_request(msg).expect("a request decodes");
            assert_eq!(req.object_key, b"old", "a call here addresses the address it left");
            seen.fetch_add(1, Ordering::SeqCst);
            let forward = if for_good {
                Forward::Permanent(to.clone())
            } else {
                Forward::Temporary(to.clone())
            };
            forward_to(&mut s, &req, reply_endian, &forward);
        }
    })
}

/// The two peers, wired to each other, plus what the test needs to read them.
struct Moved {
    old: Ior,
    new: Ior,
    at_old: Arc<AtomicUsize>,
    at_new: Arc<AtomicUsize>,
    old_done: mpsc::Receiver<()>,
    new_done: mpsc::Receiver<()>,
}

impl Moved {
    fn set_up(for_good: bool, reply_endian: Endian) -> Moved {
        let at_new = Arc::new(AtomicUsize::new(0));
        let (new_addr, new_done) = landing(reply_endian, Arc::clone(&at_new));
        let new = ior_at(new_addr, b"new");

        let at_old = Arc::new(AtomicUsize::new(0));
        let (old_addr, old_done) =
            left_behind(new.clone(), for_good, reply_endian, Arc::clone(&at_old));
        let old = ior_at(old_addr, b"old");

        Moved { old, new, at_old, at_new, old_done, new_done }
    }

    /// Both windows closed, so both counts are final. Returns
    /// `(requests at the old address, requests at the new one)`.
    fn counts(self, label: &str) -> (usize, usize) {
        self.new_done.recv_timeout(T).unwrap_or_else(|_| panic!("{label}: the landing finished"));
        self.old_done
            .recv_timeout(T)
            .unwrap_or_else(|_| panic!("{label}: the address it left finished"));
        (self.at_old.load(Ordering::SeqCst), self.at_new.load(Ordering::SeqCst))
    }
}

/// A permanent forward taken through one handle is seen by every other handle
/// on the same reference — the one it was cloned from, and one cloned off it
/// before the object moved.
///
/// Four observations, because each fails on its own under per-handle
/// re-pointing: what `ior()` answers on the handle that heard the forward, on
/// a clone taken before it, and on the template both came from; and then where
/// each of them actually dials, counted at the two peers.
#[test]
fn clones_agree_where_a_permanent_forward_moved_the_object() {
    for reply_endian in [Endian::Big, Endian::Little] {
        let label = format!("reply {reply_endian:?}");
        let m = Moved::set_up(true, reply_endian);

        let pool = Orb::new().pool();
        let mut template = pool.reference(m.old.clone());
        let mut before = template.clone();
        let mut caller = template.clone();

        assert_eq!(addressed(caller.ior()), addressed(&m.old), "{label}: nobody has called yet");
        assert_eq!(addressed(before.ior()), addressed(&m.old), "{label}: nor the clone");

        assert_eq!(
            body_i32(&caller.invoke("op", |_| {}).expect("the forward is followed")),
            ANSWER,
            "{label}: the object answers from where it moved to"
        );

        assert_eq!(
            addressed(caller.ior()),
            addressed(&m.new),
            "{label}: the handle that heard the forward"
        );
        assert_eq!(
            addressed(before.ior()),
            addressed(&m.new),
            "{label}: a clone taken before the object moved"
        );
        assert_eq!(
            addressed(template.ior()),
            addressed(&m.new),
            "{label}: the template both were cloned from, which has never called"
        );

        // And where each dials next. Every one of these is answered either
        // way — that is §9.6 — so the difference is only ever in the counts.
        for (what, r) in [("the clone", &mut before), ("the template", &mut template)] {
            assert_eq!(
                body_i32(&r.invoke("op", |_| {}).expect("answered")),
                ANSWER,
                "{label}: {what} is answered"
            );
        }
        let mut after = template.clone();
        assert_eq!(
            body_i32(&after.invoke("op", |_| {}).expect("answered")),
            ANSWER,
            "{label}: a clone taken after the move is answered"
        );

        drop(pool);
        let (at_old, at_new) = m.counts(&label);
        assert_eq!(
            at_old, 1,
            "{label}: only the call that heard the forward went to the old address"
        );
        assert_eq!(at_new, 4, "{label}: every call after it went straight to the object");
    }
}

/// The cost, in the shape the API asks for: a reference held as a template and
/// cloned per call.
///
/// `Invoker::invoke` takes `&mut self`, so this is how a reference is used
/// from more than one place. The template itself never invokes, so under
/// per-handle re-pointing it never learns and every clone starts from the
/// address the object left: three calls, three forwards, and it does not
/// converge — the fourth would pay too. Shared, the first call is the only one
/// that pays.
#[test]
fn a_reference_cloned_per_call_pays_the_forward_once_not_every_call() {
    for reply_endian in [Endian::Big, Endian::Little] {
        let label = format!("reply {reply_endian:?}");
        let m = Moved::set_up(true, reply_endian);

        let pool = Orb::new().pool();
        let mut template = pool.reference(m.old.clone());

        for call in 1..=3 {
            let mut handle = template.clone();
            assert_eq!(
                body_i32(&handle.invoke("op", |_| {}).expect("answered")),
                ANSWER,
                "{label}: call {call}"
            );
        }
        assert_eq!(
            addressed(template.ior()),
            addressed(&m.new),
            "{label}: the template learned it from its clones"
        );

        drop(pool);
        let (at_old, at_new) = m.counts(&label);
        assert_eq!(at_old, 1, "{label}: the forward is paid once, not per call");
        assert_eq!(at_new, 3, "{label}: all three calls reached the object");
    }
}

/// What sharing across clones does **not** buy: two `Pool::reference` calls
/// for one IOR are two references, and neither hears the other's forward.
///
/// This is `_duplicate` against `string_to_object`, and the number here is the
/// whole argument about whether the pool needs an identity map — so it is
/// measured rather than reasoned. It is **one forward per independently
/// created reference, once**, not one per call: a second reference pays on its
/// own first call and re-points itself with the same `moved` cell a clone
/// would have shared, so the third call through it costs nothing. Seven calls
/// through three independently created references cost three requests at the
/// address the object left.
///
/// **omniORB 4.3.4 measured in the identical shape, 2026-08-21: the same three
/// and seven.** Two `string_to_object` calls on one IOR string, a third after
/// the move, against `spike-server` forwarding `LOCATION_FORWARD_PERM`
/// (`ORBWEAVER_FORWARD_STATUS=permanent`) — `_is_equivalent` answers true and
/// each proxy still pays its own forward exactly once. So the reference ORB
/// does not give its user agreement between independently created references
/// either, which is the fact `docs/decisions/D013-*.md` §5 turns on. The
/// experiment is a separate process over TCP, never a dependency
/// (CLAUDE.md, licensing boundary); it is not committed as a gate, which
/// D013 §8 records as unmeasured-here.
///
/// **This test pins the cost, not a virtue.** If the identity map D013
/// describes is ever built, `at_old` becomes 1 and this assertion goes red on
/// purpose — read D013 before changing the number.
#[test]
fn two_references_to_one_object_each_pay_the_forward_once() {
    /// Calls through the second reference after the first has been told.
    /// More than one, because the question this answers is whether the cost
    /// is per reference or per call, and a single call cannot tell them
    /// apart. Five is what the omniORB run used, so the two numbers compare.
    const THROUGH_THE_SECOND: usize = 5;

    for reply_endian in [Endian::Big, Endian::Little] {
        let label = format!("reply {reply_endian:?}");
        let m = Moved::set_up(true, reply_endian);

        let pool = Orb::new().pool();
        // Independently created, not cloned: this is the `string_to_object`
        // shape, and the two share the pool — and so the connection — while
        // sharing nothing about where the object is.
        let mut first = pool.reference(m.old.clone());
        let mut second = pool.reference(m.old.clone());

        assert_eq!(
            body_i32(&first.invoke("op", |_| {}).expect("the forward is followed")),
            ANSWER,
            "{label}: the first reference is answered from where the object moved to"
        );
        assert_eq!(addressed(first.ior()), addressed(&m.new), "{label}: and it was re-pointed");
        assert_eq!(
            addressed(second.ior()),
            addressed(&m.old),
            "{label}: the other reference was not told — it is not a clone"
        );

        for call in 1..=THROUGH_THE_SECOND {
            assert_eq!(
                body_i32(&second.invoke("op", |_| {}).expect("answered")),
                ANSWER,
                "{label}: call {call} through the second reference"
            );
        }
        assert_eq!(
            addressed(second.ior()),
            addressed(&m.new),
            "{label}: which paid its own forward on its first call and then knew"
        );

        // And one created after both had been re-pointed: it starts from the
        // IOR it was handed, because nothing in the pool remembers.
        let mut third = pool.reference(m.old.clone());
        assert_eq!(
            body_i32(&third.invoke("op", |_| {}).expect("answered")),
            ANSWER,
            "{label}: a reference created after the move is still answered"
        );

        drop(pool);
        let (at_old, at_new) = m.counts(&label);
        assert_eq!(
            at_old, 3,
            "{label}: one forward per independently created reference, not one per call"
        );
        assert_eq!(
            at_new,
            1 + THROUGH_THE_SECOND + 1,
            "{label}: every call is answered, which is why nothing goes red on its own"
        );
    }
}

/// The other half of the split, pinned so the doc comment is a measured claim:
/// a **temporary** forward is routing state and stays with the handle that
/// took it.
///
/// §9.6 keeps the original address authoritative for a temporary hop — the
/// servant did not say the object moved, and a client that republished the
/// temporary target as the object's address would be publishing something
/// nobody said. So `ior()` does not move for anybody, a clone does not inherit
/// the cache, and the clone's call is forwarded afresh. That costs one forward
/// per handle and then stops, which is the difference from the permanent case:
/// it self-corrects, and a stale permanent address does not.
#[test]
fn a_temporary_forward_stays_with_the_handle_that_took_it() {
    for reply_endian in [Endian::Big, Endian::Little] {
        let label = format!("reply {reply_endian:?}");
        let m = Moved::set_up(false, reply_endian);

        let pool = Orb::new().pool();
        let mut caller = pool.reference(m.old.clone());
        let mut other = caller.clone();

        assert_eq!(body_i32(&caller.invoke("op", |_| {}).expect("followed")), ANSWER, "{label}");
        assert_eq!(
            caller.forwarded().map(Forward::is_permanent),
            Some(false),
            "{label}: a temporary redirect is in force on the handle that took it"
        );
        assert_eq!(
            addressed(caller.ior()),
            addressed(&m.old),
            "{label}: and it did not move the reference"
        );
        assert!(other.forwarded().is_none(), "{label}: the other handle followed nothing");
        assert_eq!(addressed(other.ior()), addressed(&m.old), "{label}");

        // The second handle is forwarded afresh, and then has the cache too.
        assert_eq!(body_i32(&other.invoke("op", |_| {}).expect("followed")), ANSWER, "{label}");
        assert_eq!(body_i32(&other.invoke("op", |_| {}).expect("cached")), ANSWER, "{label}");

        drop(pool);
        let (at_old, at_new) = m.counts(&label);
        assert_eq!(at_old, 2, "{label}: one forward per handle, and then it stops");
        assert_eq!(at_new, 3, "{label}");
    }
}

/// `Reference` is still `Send` **and** `Sync`, and the shared address really
/// does cross a thread boundary.
///
/// The compile-time half is the pin the pool needs — a `Pool` is `Send + Sync`
/// so that one pool serves every thread, and a reference that stopped being
/// either would take that away from the handle a generated stub holds. The
/// runtime half is what makes it more than a type assertion: `&Reference` is
/// borrowed into a scoped thread, which is what requires `Sync`, and the
/// forward one thread hears is what the next thread's clone dials.
///
/// Sequenced rather than concurrent, deliberately: two threads racing the
/// first call could both reach the old address before either had heard the
/// forward, and the count would then be measuring the scheduler.
#[test]
fn the_shared_address_is_send_and_sync_and_crosses_a_thread() {
    fn require<T: Send + Sync>() {}
    require::<Reference>();

    for reply_endian in [Endian::Big, Endian::Little] {
        let label = format!("reply {reply_endian:?}");
        let m = Moved::set_up(true, reply_endian);

        let pool = Orb::new().pool();
        let template = pool.reference(m.old.clone());
        let shared = &template;

        for call in 1..=2 {
            let what = &label;
            std::thread::scope(|s| {
                s.spawn(move || {
                    let mut handle = shared.clone();
                    assert_eq!(
                        body_i32(&handle.invoke("op", |_| {}).expect("answered")),
                        ANSWER,
                        "{what}: call {call} from its own thread"
                    );
                });
            });
        }

        drop(pool);
        let (at_old, at_new) = m.counts(&label);
        assert_eq!(at_old, 1, "{label}: the second thread's clone had already been told");
        assert_eq!(at_new, 2, "{label}");
    }
}

/// The lock cost, asked of the thing that enforces it rather than asserted in
/// prose: the shared address is read on the call path and **never held across
/// the wire**.
///
/// [`orbweaver_giop::guarded`]'s tripwire is what makes that checkable —
/// `Guarded::read` returns the value out of a closure instead of lending a
/// guard, and every outbound call asks whether a section is open. A normal
/// call therefore draws no complaint; and the cell is genuinely inside the
/// discipline rather than beside it, which is the second half: reaching a
/// reference from inside a servant's own lock is caught, and the complaint
/// names the address.
///
/// Asserted through `complaints_about` and not by catching a panic: the
/// discipline panics in a debug build and complains in a release one, and the
/// property is what both are reactions to.
#[test]
fn the_shared_address_is_never_held_across_the_wire() {
    let at_new = Arc::new(AtomicUsize::new(0));
    let (new_addr, new_done) = landing(Endian::Big, Arc::clone(&at_new));

    let pool = Orb::new().pool();
    let mut r = pool.reference(ior_at(new_addr, b"new"));

    let said = complaints_about(|| {
        assert_eq!(body_i32(&r.invoke("op", |_| {}).expect("answered")), ANSWER);
    });
    assert!(said.is_empty(), "a pooled call must draw no complaint of its own, got {said:?}");

    let state = Guarded::new("a servant holding its own state", ());
    let said = complaints_about(|| {
        state.read(|_| {
            let _ = r.ior();
        });
    });
    assert!(
        said.first().is_some_and(|c| {
            c.contains("an object reference's address")
                && c.contains("a servant holding its own state")
        }),
        "the shared address must be inside the lock discipline, got {said:?}"
    );

    drop(pool);
    new_done.recv_timeout(T).expect("the landing finished");
    assert_eq!(at_new.load(Ordering::SeqCst), 1);
}
