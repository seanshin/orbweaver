//! D029 §3.1 / §5 O1: **the ORB can stop what it handed out**, and what a peer
//! mid-call observes across that.
//!
//! # Why the measurement is a socket and not a counter
//!
//! D029 §5 O1 names the oracle: *"a peer mid-call when shutdown lands …  the
//! measurement is what the client **sees**, not what our counters say."* So the
//! client here is a bare [`TcpStream`] speaking GIOP bytes. It shares no state
//! with the server, holds no handle to it, and learns everything it knows by
//! reading. Every assertion below is about **messages that came back off the
//! wire**; `ServerStats` appears in exactly one test, and there it is the
//! subject rather than the evidence.
//!
//! `spikes/half_reply_peer.py` is the shape D029 points at — a peer that can be
//! held at a chosen point. This is its mirror: the *servant* is held, so the
//! shutdown provably lands while a request is inside it, and the peer is the
//! one doing the observing.
//!
//! # The bound these tests refute if it stops being true
//!
//! The bound is stated on `Orb::shutdown` and is not restated here. What is
//! here is its refutation shape, which is what a test is for:
//!
//! 1. a request already inside the servant is **answered in full**;
//! 2. a request whose bytes had arrived but which had not been read is
//!    **never answered**;
//! 3. the connection ends with a `CloseConnection` (§9.4.10) and nothing else.
//!
//! (3) is what makes (2) obligatory rather than tidy: §9.4.7 makes that goodbye
//! mean *"not processed, re-send elsewhere"*, so a request read after the flag
//! and then dropped would turn the goodbye into a lie about a request that had
//! been processed, and a peer acting correctly on it would execute the
//! operation twice. See `docs/decisions/D034-stopping-what-the-orb-handed-out.md`.
//!
//! # Both byte orders, and every version
//!
//! The bound is not a byte-order property, which is exactly why it is measured
//! in both: an assertion that only ever ran little-endian is an assertion about
//! this machine. Six combinations per case.
//!
//! *측정은 카운터가 아니라 소켓이다. 클라이언트는 서버와 아무 상태도 공유하지 않고,
//! 아는 것은 전부 읽어서 안다.*

use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{Dispatch, Request, SystemException};
use orbweaver_giop::{MsgType, Version, decode_reply, encode_request, read_message};

const KEY: &[u8] = b"StopProbe";
const DEADLINE: Duration = Duration::from_secs(10);
const ANSWER: i32 = 42;
/// Big enough that a `read` returning nothing is a peer that hung up rather
/// than a peer that is slow, and small enough that a wedged test fails inside
/// one `cargo test` rather than hanging the suite.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

const VERSIONS: [Version; 3] = [Version::V1_0, Version::V1_1, Version::V1_2];
const ENDIANS: [Endian; 2] = [Endian::Big, Endian::Little];

/// A servant that can be held inside one call, at a point the test chooses.
///
/// The hold is a channel rendezvous rather than a sleep, so *"shutdown landed
/// while a request was inside the servant"* is a fact the test establishes
/// rather than a race it hopes for. `recv_timeout` bounds it, so a broken test
/// fails instead of hanging — the harness rule about wait loops, in servant
/// form.
struct Held {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    held: bool,
}

impl Dispatch for Held {
    fn dispatch(&mut self, req: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        match req.operation.as_str() {
            "held" => {
                if !self.held {
                    self.held = true;
                    let _ = self.entered.send(());
                    let _ = self.release.recv_timeout(DEADLINE);
                }
                out.put_i32(ANSWER);
                Ok(())
            }
            "ping" => {
                out.put_i32(ANSWER);
                Ok(())
            }
            _ => Err(SystemException::bad_operation()),
        }
    }
}

/// Answers everything at once; for the cases where nothing needs holding.
struct Ping;

impl Dispatch for Ping {
    fn dispatch(&mut self, req: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        if req.operation == "ping" {
            out.put_i32(ANSWER);
            return Ok(());
        }
        Err(SystemException::bad_operation())
    }
}

/// One message read off the peer's socket, reduced to what these tests assert
/// about.
#[derive(Debug, PartialEq, Eq)]
enum Seen {
    /// A reply, its request id, and its decoded body — **decoded**, because
    /// CDR padding content is undefined and comparing raw buffers is the
    /// project's own recorded way of producing false failures.
    Reply(u32, i32),
    /// §9.4.10's goodbye. Carries its length so "and nothing else" is
    /// assertable: a `CloseConnection` has no body.
    Goodbye(usize),
    /// The socket closed without another GIOP message.
    Eof,
    /// Anything else at all — kept as a variant rather than a panic so the
    /// assertion failure prints what actually arrived.
    Other(String),
}

/// Reads every message the peer sends until the conversation ends, so the
/// assertion can be about the **whole** exchange rather than its first message.
///
/// This is the difference between "we got our reply" and "we got our reply and
/// nothing that should not have followed it", and the second is the claim.
fn drain(mut s: TcpStream) -> Vec<Seen> {
    s.set_read_timeout(Some(READ_TIMEOUT)).expect("a read timeout the test can fail on");
    let mut seen = Vec::new();
    loop {
        match read_message(&mut s, 64 * 1024) {
            Ok(msg) => match msg.msg_type {
                MsgType::Reply => {
                    let len = msg.bytes.len();
                    match decode_reply(msg) {
                        Ok(r) => match answer(&r) {
                            Ok(v) => seen.push(Seen::Reply(r.request_id, v)),
                            Err(e) => seen.push(Seen::Other(format!("undecodable body: {e}"))),
                        },
                        Err(e) => seen.push(Seen::Other(format!("bad reply of {len}B: {e}"))),
                    }
                }
                MsgType::CloseConnection => {
                    seen.push(Seen::Goodbye(msg.bytes.len()));
                    // §9.4.10 ends the conversation; anything after it would be
                    // a different defect and there is nothing left to read.
                    return seen;
                }
                other => seen.push(Seen::Other(format!("{other:?}"))),
            },
            Err(orbweaver_giop::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                ) =>
            {
                seen.push(Seen::Eof);
                return seen;
            }
            Err(e) => {
                seen.push(Seen::Other(format!("read failed: {e}")));
                return seen;
            }
        }
    }
}

/// The one `long` every operation here returns, **decoded** — never compared
/// as raw bytes, because CDR padding content is undefined by the specification
/// and comparing buffers is this project's recorded way of manufacturing false
/// failures.
fn answer(reply: &orbweaver_giop::Reply) -> Result<i32, String> {
    let mut body = reply.body().map_err(|e| e.to_string())?;
    body.get_i32().map_err(|e| e.to_string())
}

/// A request on the wire, ready to write.
fn request(version: Version, endian: Endian, id: u32, operation: &str) -> Vec<u8> {
    encode_request(version, endian, id, KEY, operation, true, |_| {})
        .expect("our own encoder must produce our own request")
}

/// **The bound, measured from a peer's socket.**
///
/// One connection carries two pipelined requests. The servant is held inside
/// the first; the shutdown lands while it is held; the second request's bytes
/// are already at the server and have not been read.
///
/// What the peer must see: the first reply in full, then the goodbye, and
/// **no reply to the second request** — which is the whole of the bound, and
/// each third of it fails independently.
#[test]
fn a_peer_mid_call_gets_its_reply_and_then_the_goodbye() {
    for version in VERSIONS {
        for endian in ENDIANS {
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let mut servant = Held { entered: entered_tx, release: release_rx, held: false };

            let orb = Orb::new();
            let server = orb.server("127.0.0.1:0", KEY.to_vec()).expect("bind");
            let addr = server.local_addr().expect("bound address");

            // `|| false` on purpose. It is the shape 17 of this workspace's 63
            // serve sites use, and before D034 it meant *this server cannot be
            // stopped without killing the process*. If the ORB's flag were not
            // OR'd in, this test would hang rather than fail — which is why the
            // client's reads are bounded.
            let serving = std::thread::spawn(move || {
                server.serve(&mut servant, || false).expect("serve");
            });

            let mut peer = TcpStream::connect(addr).expect("connect");
            peer.set_nodelay(true).expect("nodelay");
            // Both requests written before the servant is released, so the
            // second one's bytes are at the server while the first is inside
            // the servant. Whether the kernel has delivered them by the time
            // the flag goes up does not matter: the claim is that the second is
            // *never answered*, which holds either way.
            peer.write_all(&request(version, endian, 1, "held")).expect("write held");
            peer.write_all(&request(version, endian, 2, "ping")).expect("write ping");

            // Not a sleep: the servant says when it is inside.
            entered_rx.recv_timeout(DEADLINE).expect("the servant must be reached");

            let report = orb.shutdown();
            assert_eq!(report.servers(), 1, "{version} {endian:?}: the one live server");
            assert_eq!(report.already_gone(), 0, "{version} {endian:?}");

            release_tx.send(()).expect("release the servant");

            let seen = drain(peer);
            // Asserted **before** the join, deliberately. A build where the
            // ORB's flag never reaches the serving loop leaves `serve` running
            // forever, and joining first would turn a refutation into a hung
            // test — which is the one failure mode a harness cannot read. This
            // way the control prints its diff and the leaked thread dies with
            // the process.
            assert_eq!(
                seen,
                vec![Seen::Reply(1, ANSWER), Seen::Goodbye(orbweaver_giop::HEADER_LEN)],
                "{version} {endian:?}: the in-flight call is answered in full, the pipelined \
                 one is never answered, and the connection ends with an empty CloseConnection"
            );
            serving.join().expect("the serving thread must end");
        }
    }
}

/// The control for the test above, and the reason it is a separate test rather
/// than an assertion inside it.
///
/// A single exchange that passes tells you nothing about *why*: the reply could
/// have arrived because the shutdown is graceful, or because the shutdown never
/// reached the server at all and the ordinary path served both requests. Those
/// two produce different transcripts, and this pins the second one — **an ORB
/// nobody asks to stop answers both**.
///
/// Without this, `a_peer_mid_call_gets_its_reply_and_then_the_goodbye` would
/// still pass on a build where `Orb::shutdown` did nothing whatsoever, as long
/// as the second reply happened to be slow.
#[test]
fn an_orb_nobody_stops_answers_both_pipelined_requests() {
    for version in VERSIONS {
        for endian in ENDIANS {
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let mut servant = Held { entered: entered_tx, release: release_rx, held: false };

            let orb = Orb::new();
            let server = orb.server("127.0.0.1:0", KEY.to_vec()).expect("bind");
            let addr = server.local_addr().expect("bound address");
            let stop = Arc::new(AtomicBool::new(false));
            let flag = Arc::clone(&stop);

            let serving = std::thread::spawn(move || {
                server.serve(&mut servant, move || flag.load(Ordering::SeqCst)).expect("serve");
            });

            let mut peer = TcpStream::connect(addr).expect("connect");
            peer.set_nodelay(true).expect("nodelay");
            peer.write_all(&request(version, endian, 1, "held")).expect("write held");
            peer.write_all(&request(version, endian, 2, "ping")).expect("write ping");

            entered_rx.recv_timeout(DEADLINE).expect("the servant must be reached");
            assert!(!orb.is_shutdown(), "{version} {endian:?}: nothing asked this ORB to stop");
            release_tx.send(()).expect("release the servant");

            // Read exactly the two replies, then stop the server the old way.
            peer.set_read_timeout(Some(READ_TIMEOUT)).expect("read timeout");
            for id in [1u32, 2] {
                let msg = read_message(&mut peer, 64 * 1024).expect("a reply");
                assert_eq!(msg.msg_type, MsgType::Reply, "{version} {endian:?}: reply {id}");
                let reply = decode_reply(msg).expect("decodable");
                assert_eq!(reply.request_id, id, "{version} {endian:?}");
                assert_eq!(
                    answer(&reply).expect("body"),
                    ANSWER,
                    "{version} {endian:?}: request {id} was served"
                );
            }

            stop.store(true, Ordering::SeqCst);
            let seen = drain(peer);
            assert_eq!(
                seen,
                vec![Seen::Goodbye(orbweaver_giop::HEADER_LEN)],
                "{version} {endian:?}: the caller's own predicate still ends it the same way"
            );
            serving.join().expect("the serving thread must end");
        }
    }
}

/// Serves on a thread and waits [`DEADLINE`] for `serve` to return `Ok(())`.
///
/// Never joins unbounded, and the reason is a control this file's own author
/// ran: with the ORB's flag not OR'd into the serving loop, the first draft of
/// `the_caller_can_ask_which_flag_it_was…` **hung instead of failing**, because
/// it called `serve` on the test's own thread and `serve` never returned. A
/// hang is the one failure mode a harness cannot read — it looks identical to a
/// slow machine — so every `serve` in this file is bounded.
///
/// *멈춘 테스트는 하네스가 읽을 수 없는 유일한 실패다. 느린 기계와 구분되지 않는다.*
fn serves_and_returns(
    server: orbweaver_giop::server::Server,
    stop: impl Fn() -> bool + Sync + Send + 'static,
) -> bool {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(server.serve(&mut Ping, stop).is_ok());
    });
    matches!(rx.recv_timeout(DEADLINE), Ok(true))
}

/// The caller's predicate and the ORB's flag are the **same event**, and
/// `serve` does not tell them apart (D034 §5). What a caller can ask is
/// `Server::stop_requested`, and that is the one place the two differ.
#[test]
fn the_caller_can_ask_which_flag_it_was_and_gets_the_same_return_either_way() {
    // Stopped by the caller: serve returns Ok, and the ORB's flag stays down.
    let orb = Orb::new();
    let server = orb.server("127.0.0.1:0", KEY.to_vec()).expect("bind");
    let asked = server.stop_flag();
    assert!(!server.stop_requested(), "nothing has asked yet");
    assert!(serves_and_returns(server, || true), "the caller's own predicate ends it");
    assert!(!asked.raised(), "the caller stopped it; the ORB's half stayed down");
    assert!(!orb.is_shutdown(), "and the ORB was never asked");

    // Stopped by the ORB: serve returns the same Ok, and the flag is up.
    let orb = Orb::new();
    let server = orb.server("127.0.0.1:0", KEY.to_vec()).expect("bind");
    let asked = server.stop_flag();
    orb.shutdown();
    assert!(server.stop_requested(), "the ORB asked");
    // `|| false` — the shape 17 of this workspace's serve sites use, and before
    // D034 the shape that could not be stopped at all.
    assert!(serves_and_returns(server, || false), "the ORB's flag ends it, and the same Ok");
    assert!(asked.raised());
}

/// A stopped ORB **hands out no new transport** (D034 §7), or `shutdown` would
/// mean "stop the ones I have already given" and the next line could undo it.
///
/// The two halves refuse differently on purpose and the difference is the
/// resource, not a lapse: a server would take a port, so it is refused before
/// the bind; a pool takes an `Arc`, so it is handed out already closed and
/// refuses at the call that would have dialled.
#[test]
fn a_stopped_orb_hands_out_no_new_transport() {
    let orb = Orb::new();
    orb.shutdown();
    assert!(orb.is_shutdown());

    match orb.server("127.0.0.1:0", KEY.to_vec()) {
        Err(orbweaver_giop::Error::Stopped { what }) => assert_eq!(what, "a server"),
        other => panic!("a stopped ORB must refuse to bind, got {other:?}"),
    }

    let pool = orb.pool();
    assert!(pool.is_closed(), "a pool from a stopped ORB is born closed");
    let ior = orbweaver_giop::Ior {
        type_id: "IDL:Probe:1.0".into(),
        profiles: vec![orbweaver_giop::IiopProfile {
            version: Version::V1_2,
            host: "127.0.0.1".into(),
            port: 1,
            object_key: KEY.to_vec(),
            components: vec![],
        }],
    };
    match pool.acquire(&ior) {
        Err(orbweaver_giop::Error::Stopped { what }) => assert_eq!(what, "a pooled connection"),
        other => panic!("a closed pool must not dial, got {other:?}"),
    }
}

/// A pool's close means **nobody new gets a connection**, and deliberately not
/// *"calls in flight are aborted"* (D034 §6).
///
/// Measured as the boundary rather than asserted as a sentence: a `Mux` taken
/// before the close still carries a call, and an `acquire` after it does not
/// dial. If closing ever grew teeth and started killing held connections, the
/// first half of this goes red.
#[test]
fn closing_a_pool_stops_new_dials_and_leaves_a_held_connection_working() {
    let orb = Orb::new();
    let server = orb.server("127.0.0.1:0", KEY.to_vec()).expect("bind");
    let ior = server.ior("IDL:Probe:1.0", "127.0.0.1").expect("ior");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let serving = std::thread::spawn(move || {
        server.serve(&mut Ping, move || flag.load(Ordering::SeqCst)).expect("serve");
    });

    let pool = orb.pool();
    let (mux, key) = pool.acquire(&ior).expect("a first connection");
    assert_eq!(pool.size(), 1);

    pool.close();
    assert!(pool.is_closed());
    assert_eq!(pool.size(), 0, "the pooled connections were dropped");

    // The held Mux still works: the caller owns that connection and the close
    // did not reach into it.
    match mux.call_on(&key, "ping", |_| {}, READ_TIMEOUT) {
        Ok(orbweaver_giop::mux::Sent::Reply(r)) => {
            assert_eq!(answer(&r).expect("body"), ANSWER, "the held connection still carries")
        }
        other => panic!("the close must not reach into a Mux the caller holds, got {other:?}"),
    }

    // And nothing new is dialled.
    match pool.acquire(&ior) {
        Err(orbweaver_giop::Error::Stopped { .. }) => {}
        other => panic!("a closed pool must not dial, got {other:?}"),
    }

    stop.store(true, Ordering::SeqCst);
    serving.join().expect("serving ends");
}

/// `wait_until_stopped` must be able to answer **false**, or it is a sleep with
/// a return type.
///
/// Held servant, shutdown, and a deadline shorter than the hold: the servers
/// have not gone quiet and the answer says so. Then the release, and the same
/// question answered `true`.
#[test]
fn wait_until_stopped_answers_false_while_a_servant_is_still_held() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut servant = Held { entered: entered_tx, release: release_rx, held: false };

    let orb = Orb::new();
    let server = orb.server("127.0.0.1:0", KEY.to_vec()).expect("bind");
    let addr = server.local_addr().expect("bound");
    let serving = std::thread::spawn(move || {
        server.serve(&mut servant, || false).expect("serve");
    });

    let mut peer = TcpStream::connect(addr).expect("connect");
    peer.write_all(&request(Version::V1_2, Endian::Big, 1, "held")).expect("write");
    entered_rx.recv_timeout(DEADLINE).expect("inside the servant");

    orb.shutdown();
    assert!(
        !orb.wait_until_stopped(Duration::from_millis(200)),
        "a server with a call inside its servant has not gone quiet"
    );

    release_tx.send(()).expect("release");
    assert!(orb.wait_until_stopped(DEADLINE), "and then it does");

    drop(drain(peer));
    serving.join().expect("serving ends");
}

/// A handout that was dropped is **already stopped**, and that is sound rather
/// than convenient: `serve_shared` borrows the `Server`, so no serving loop can
/// outlive it.
///
/// Reported rather than silent — a caller expecting four stopped servers and
/// getting zero finds out here, not from a port that is still bound.
#[test]
fn a_dropped_server_counts_as_already_gone_rather_than_as_stopped() {
    let orb = Orb::new();
    let live = orb.server("127.0.0.1:0", KEY.to_vec()).expect("bind");
    drop(orb.server("127.0.0.1:0", KEY.to_vec()).expect("bind"));

    let report = orb.shutdown();
    assert_eq!(report.servers(), 1, "one was still alive");
    assert_eq!(report.already_gone(), 1, "one had been dropped");
    assert!(live.stop_requested());
}

/// An `Orb` clone is a handle to the **same** ORB for lifecycle purposes.
///
/// The alternative — a clone that cannot stop what the original handed out —
/// would recreate, one level up, the exact *gives and cannot take back*
/// asymmetry D029 §3.1 opened this work to close.
#[test]
fn shutting_down_a_clone_stops_the_originals_servers() {
    let orb = Orb::new();
    let server = orb.server("127.0.0.1:0", KEY.to_vec()).expect("bind");
    let handle = orb.clone();
    assert_eq!(handle.shutdown().servers(), 1);
    assert!(server.stop_requested(), "the clone reached the original's server");
    assert!(orb.is_shutdown(), "and the original knows it is stopped");
}

/// Equality is equality of **configuration**, stated as a test because the
/// hand-written `PartialEq` is the kind of thing a later reader would otherwise
/// have to infer from its body (D034; `Orb`'s `PartialEq` docs).
#[test]
fn orb_equality_is_about_configuration_and_not_about_being_stopped() {
    let a = Orb::new();
    let b = Orb::new();
    assert_eq!(a, b, "two ORBs configured alike");
    a.shutdown();
    assert_eq!(a, b, "and still alike: being asked to stop is not a configuration");
    assert!(a.is_shutdown() && !b.is_shutdown(), "they are nonetheless different ORBs");
}

/// Idempotence, because `shutdown` is reachable from every clone and a
/// supervisor will call it more than once.
#[test]
fn shutdown_is_idempotent_and_the_second_one_says_so() {
    let orb = Orb::new();
    let _server = orb.server("127.0.0.1:0", KEY.to_vec()).expect("bind");
    assert_eq!(orb.shutdown().servers(), 1);
    let again = orb.shutdown();
    assert_eq!(again.servers(), 1, "the server is still alive, and still stopped");
    assert_eq!(again.pools(), 0);
}
