//! §9.6's restart at the origin, and — the half that matters more — when it
//! must **not** happen.
//!
//! `crates/orbweaver-gen/tests/forward_fallback.rs` measures the shape the
//! peer oracle (`spikes/perm_fallback.sh`) measures: two live servers, the
//! forwarded-to one stopped cleanly, the client called again. A server of
//! ours stops with a `CloseConnection`, so that shape only ever exercises the
//! one failure §13.5.1 makes safe to re-send. The rule in `Connection::invoke`
//! has more arms than that, and each of the others needs a peer that misbehaves
//! in exactly one way — which is what a scripted TCP peer, built out of this
//! crate's own encoders, is for. Self-tests of a decoded property, not interop
//! results, labelled as such on purpose.
//!
//! What each test pins, both request byte orders, replies in the other:
//!
//! * `CloseConnection` on the forwarded connection under a temporary forward:
//!   the request did not run, the forwarded-to port is closed, so the same
//!   call restarts at the origin and is answered there;
//! * the socket dropped mid-wait with **no** `CloseConnection` under a
//!   temporary forward: completion unknown, so that call is an error and the
//!   origin sees nothing — and the *next* call, whose request never left,
//!   restarts;
//! * `CloseConnection` under a **permanent** forward: the origin was replaced,
//!   nothing falls back, the old address sees nothing;
//! * `CloseConnection` from a forwarded-to peer that is still listening: the
//!   forwarding information is reused — the peer is redialled and answers —
//!   and the origin is not asked at all;
//! * and one about what a hop must **not** change: a caller's
//!   `cap_version` is still in force at the forwarded-to endpoint and at the
//!   origin after a restart, measured from the version on the wire.

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_giop::server::{
    Request, decode_request, encode_close_connection, encode_location_forward, encode_reply,
};
use orbweaver_giop::{
    Connection, DEFAULT_MAX_MESSAGE_SIZE, Error, Forward, IiopProfile, Ior, ReplyStatus, Version,
    read_message,
};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

const T: Duration = Duration::from_secs(10);

fn ior_at(addr: std::net::SocketAddr, key: &[u8]) -> Ior {
    Ior {
        type_id: "IDL:test/Restart:1.0".into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: addr.ip().to_string(),
            port: addr.port(),
            object_key: key.to_vec(),
            components: Vec::new(),
        }],
    }
}

fn other(e: Endian) -> Endian {
    match e {
        Endian::Big => Endian::Little,
        Endian::Little => Endian::Big,
    }
}

/// Reads one request whole. The peers here answer **in the version they were
/// asked in**, as a real server does, which is what lets one of them measure a
/// caller's version cap rather than assume it.
fn take_request(s: &mut TcpStream) -> Request {
    let msg = read_message(s, DEFAULT_MAX_MESSAGE_SIZE).expect("a request arrives");
    decode_request(msg).expect("a request decodes")
}

/// Answers `req` with a `long`, in the byte order opposite to the request's.
fn reply_long(s: &mut TcpStream, req: &Request, value: i32) {
    let msg = encode_reply(
        req.version,
        other(req.endian),
        req.request_id,
        ReplyStatus::NoException,
        None,
        |e: &mut Encoder| e.put_i32(value),
    )
    .expect("reply encodes");
    s.write_all(&msg).expect("reply goes out");
    s.flush().expect("flush");
}

fn forward_to(s: &mut TcpStream, req: &Request, to: &Forward) {
    let msg = encode_location_forward(req.version, other(req.endian), req.request_id, to)
        .expect("encodes");
    s.write_all(&msg).expect("forward goes out");
    s.flush().expect("flush");
}

fn close(s: &mut TcpStream, req: &Request) {
    let msg = encode_close_connection(req.version, other(req.endian)).expect("encodes");
    s.write_all(&msg).expect("close goes out");
    s.flush().expect("flush");
}

/// A peer whose script the test writes; the channel says it finished, so a
/// failing script fails the test instead of hanging it.
fn scripted<F>(script: F) -> (std::net::SocketAddr, mpsc::Receiver<()>)
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

/// The origin: forwards its first request to `to`, then answers every later
/// request — each on its own connection, since each is a restart — with
/// `answer`, and reports how many requests it saw after the forward.
fn origin(
    to: Forward,
    answer: i32,
    expect_after: usize,
) -> (std::net::SocketAddr, mpsc::Receiver<()>) {
    scripted(move |l| {
        let (mut s, _) = l.accept().expect("first connection");
        let req = take_request(&mut s);
        forward_to(&mut s, &req, &to);
        for _ in 0..expect_after {
            let (mut s, _) = l.accept().expect("a restart connection");
            let req = take_request(&mut s);
            reply_long(&mut s, &req, answer);
        }
    })
}

/// Temporary forward, then the forwarded-to peer says `CloseConnection` and
/// stops listening: the same call restarts at the origin.
#[test]
fn close_connection_under_a_temporary_forward_restarts_at_the_origin_in_the_same_call() {
    for endian in [Endian::Big, Endian::Little] {
        let (target, target_done) = scripted(|l| {
            let (mut s, _) = l.accept().expect("the forwarded call");
            // The listener goes BEFORE CloseConnection is sent, so the
            // client's redial of the forwarding information is refused
            // outright. Dropping it with the script let the redial race the
            // drop on Linux: connect() landed in the backlog, the listener
            // closed, and the re-issued request died with ECONNRESET — an
            // unknown-completion failure the client correctly does not
            // restart from. CI 2 of 5 runs (b5e2fb0); never on macOS.
            drop(l);
            let req = take_request(&mut s);
            reply_long(&mut s, &req, 42);
            let req = take_request(&mut s);
            close(&mut s, &req);
        });
        let to = Forward::Temporary(ior_at(target, b"new"));
        let (orig, orig_done) = origin(to, 1, 1);
        let old = ior_at(orig, b"old");

        let mut c = Connection::connect(&old, T).expect("connect");
        c.set_endian(endian);
        assert_eq!(body_i32(&c.invoke_nullary("op").expect("forwarded")), 42, "{endian:?}");
        assert!(c.forwarded().is_some_and(|f| !f.is_permanent()), "{endian:?}");
        assert_eq!(c.origin(), &old, "{endian:?}");

        let reply = c.invoke_nullary("op").expect("restarted at the origin in this call");
        assert_eq!(body_i32(&reply), 1, "{endian:?}: answered by the origin");
        assert!(c.forwarded().is_none(), "{endian:?}: at the origin again");
        assert!(c.is_usable(), "{endian:?}");
        assert_eq!(c.endian(), endian, "{endian:?}: byte order kept across the restart");
        target_done.recv_timeout(T).expect("target script finished");
        orig_done.recv_timeout(T).expect("origin script finished");
    }
}

/// Temporary forward, then the forwarded-to peer drops the socket with no
/// `CloseConnection` while a request is outstanding: completion unknown, so
/// that call is an error and the origin is not asked. The next call — nothing
/// of it has left — restarts.
#[test]
fn a_dropped_socket_after_a_full_request_is_not_restarted_but_the_next_call_is() {
    for endian in [Endian::Big, Endian::Little] {
        let (target, target_done) = scripted(|l| {
            let (mut s, _) = l.accept().expect("the forwarded call");
            let req = take_request(&mut s);
            reply_long(&mut s, &req, 42);
            let _ = take_request(&mut s);
            drop(s); // EOF, or a reset: no word on whether it ran
        });
        let to = Forward::Temporary(ior_at(target, b"new"));
        let (orig, orig_done) = origin(to, 1, 1);
        let old = ior_at(orig, b"old");

        let mut c = Connection::connect(&old, T).expect("connect");
        c.set_endian(endian);
        assert_eq!(body_i32(&c.invoke_nullary("op").expect("forwarded")), 42, "{endian:?}");

        let err = c.invoke_nullary("op").expect_err("completion unknown: not re-issued");
        assert!(matches!(err, Error::Io(_)), "{endian:?}: {err}");
        target_done.recv_timeout(T).expect("target script finished");
        assert!(!c.is_usable(), "{endian:?}: poisoned");
        assert!(c.forwarded().is_some(), "{endian:?}: still where it was");
        // The origin must not have been asked: its script would only have
        // accepted, and the channel would not have fired yet either way, so
        // the proof is the count — one restart is all it accepts, and it is
        // this next call that takes it.
        let reply = c.invoke_nullary("op").expect("the next call restarts");
        assert_eq!(body_i32(&reply), 1, "{endian:?}: answered by the origin");
        assert!(c.forwarded().is_none(), "{endian:?}");
        assert!(c.is_usable(), "{endian:?}");
        orig_done.recv_timeout(T).expect("origin script finished");
    }
}

/// Permanent forward, then `CloseConnection` and the port closed: the origin
/// *is* the forwarded-to reference now, so nothing falls back and the old
/// address sees nothing.
#[test]
fn close_connection_under_a_permanent_forward_does_not_go_back() {
    for endian in [Endian::Big, Endian::Little] {
        let (target, target_done) = scripted(|l| {
            let (mut s, _) = l.accept().expect("the forwarded call");
            let req = take_request(&mut s);
            reply_long(&mut s, &req, 42);
            let req = take_request(&mut s);
            close(&mut s, &req);
        });
        let new = ior_at(target, b"new");
        let (orig, orig_done) = origin(Forward::Permanent(new.clone()), 1, 0);
        let old = ior_at(orig, b"old");

        let mut c = Connection::connect(&old, T).expect("connect");
        c.set_endian(endian);
        assert_eq!(body_i32(&c.invoke_nullary("op").expect("forwarded")), 42, "{endian:?}");
        assert!(c.forwarded().is_some_and(Forward::is_permanent), "{endian:?}");
        assert_eq!(c.origin(), &new, "{endian:?}: the permanent forward replaced the origin");
        orig_done.recv_timeout(T).expect("the origin's script is finished: it accepts no restart");

        let err = c.invoke_nullary("op").expect_err("nowhere to fall back to");
        // Reuse of the forwarding information is not the question here — the
        // permanent target *is* the origin — and its port is closed: what
        // comes back is the closed connection, not a restart.
        assert!(matches!(err, Error::ConnectionClosed), "{endian:?}: {err}");
        assert!(c.forwarded().is_some_and(Forward::is_permanent), "{endian:?}");
        assert!(c.invoke_nullary("op").is_err(), "{endian:?}: and stays down");
        target_done.recv_timeout(T).expect("target script finished");
    }
}

/// Temporary forward, then `CloseConnection` from a peer that is still there:
/// "a client can attempt to reuse the forwarding information it has" — the
/// forwarded-to peer is redialled and answers, and the origin hears nothing.
#[test]
fn close_connection_from_a_living_target_is_reused_not_restarted() {
    for endian in [Endian::Big, Endian::Little] {
        let (target, target_done) = scripted(|l| {
            let (mut s, _) = l.accept().expect("the forwarded call");
            let req = take_request(&mut s);
            reply_long(&mut s, &req, 42);
            let req = take_request(&mut s);
            close(&mut s, &req);
            drop(s);
            let (mut s, _) = l.accept().expect("the redial");
            let req = take_request(&mut s);
            reply_long(&mut s, &req, 43);
        });
        let to = Forward::Temporary(ior_at(target, b"new"));
        let (orig, orig_done) = origin(to, 1, 0);
        let old = ior_at(orig, b"old");

        let mut c = Connection::connect(&old, T).expect("connect");
        c.set_endian(endian);
        assert_eq!(body_i32(&c.invoke_nullary("op").expect("forwarded")), 42, "{endian:?}");
        orig_done.recv_timeout(T).expect("origin script finished: it accepts no restart");

        let reply = c.invoke_nullary("op").expect("reused the forwarding information");
        assert_eq!(body_i32(&reply), 43, "{endian:?}: answered by the redialled target");
        assert!(c.forwarded().is_some_and(|f| !f.is_permanent()), "{endian:?}: still forwarded");
        assert_eq!(c.origin(), &old, "{endian:?}");
        target_done.recv_timeout(T).expect("target script finished");
    }
}

/// A caller's **version cap** is the caller's, and neither a forward nor a
/// restart repeals it.
///
/// `Connection::cap_version` is how a caller declares which GIOP version its
/// own encoding assumes — the `wstring` length rules differ between 1.1 and
/// 1.2, so it is a statement about the octets the caller will write, not a
/// preference. Following a forward replaces the socket, and §9.6's restart
/// replaces it again; both are transparent by design, so the caller cannot
/// see either happen. Until this test the new connection negotiated from the
/// forwarded-to profile alone and a 1.2 target raised the version straight
/// back to 1.2 under a caller who had never withdrawn the cap.
///
/// Carrying it needs no policy, which is why this is a fix: §9.4.1 gives the
/// profile a *ceiling*, a cap is another ceiling, and the version spoken is
/// the lower of the two — so a profile can lower a cap further but can never
/// contradict it. Every profile here advertises 1.2, so 1.1 on the wire can
/// only have come from the caller.
#[test]
fn a_version_cap_survives_a_forward_and_a_restart() {
    for endian in [Endian::Big, Endian::Little] {
        // Every request any peer saw, in order, with who saw it: the version
        // on the wire is what is under test, so it is read off the requests
        // rather than off the client's own field alone.
        let seen: Arc<Mutex<Vec<(&'static str, Version)>>> = Arc::new(Mutex::new(Vec::new()));

        let at_target = Arc::clone(&seen);
        let (target, target_done) = scripted(move |l| {
            let (mut s, _) = l.accept().expect("the forwarded call");
            // Before the CloseConnection, so the redial is refused rather
            // than reset — the same lesson as the first test in this file.
            drop(l);
            let req = take_request(&mut s);
            at_target.lock().expect("lock").push(("target", req.version));
            reply_long(&mut s, &req, 42);
            let req = take_request(&mut s);
            at_target.lock().expect("lock").push(("target", req.version));
            close(&mut s, &req);
        });
        let to = Forward::Temporary(ior_at(target, b"new"));

        let at_origin = Arc::clone(&seen);
        let (orig, orig_done) = scripted(move |l| {
            let (mut s, _) = l.accept().expect("first connection");
            let req = take_request(&mut s);
            at_origin.lock().expect("lock").push(("origin", req.version));
            forward_to(&mut s, &req, &to);
            let (mut s, _) = l.accept().expect("the restart connection");
            let req = take_request(&mut s);
            at_origin.lock().expect("lock").push(("origin", req.version));
            reply_long(&mut s, &req, 1);
        });
        let old = ior_at(orig, b"old");

        let mut c = Connection::connect(&old, T).expect("connect");
        c.set_endian(endian);
        assert_eq!(c.version(), Version::V1_2, "{endian:?}: 1.2 is what the profile advertises");
        c.cap_version(Version::V1_1);
        assert_eq!(c.version(), Version::V1_1, "{endian:?}: and 1.1 is what the caller declared");

        assert_eq!(body_i32(&c.invoke_nullary("op").expect("forwarded")), 42, "{endian:?}");
        assert_eq!(c.version(), Version::V1_1, "{endian:?}: the hop did not repeal the cap");

        let reply = c.invoke_nullary("op").expect("restarted at the origin in this call");
        assert_eq!(body_i32(&reply), 1, "{endian:?}: answered by the origin");
        assert_eq!(c.version(), Version::V1_1, "{endian:?}: nor did the restart");

        target_done.recv_timeout(T).expect("target script finished");
        orig_done.recv_timeout(T).expect("origin script finished");
        assert_eq!(
            *seen.lock().expect("lock"),
            vec![
                ("origin", Version::V1_1),
                ("target", Version::V1_1),
                ("target", Version::V1_1),
                ("origin", Version::V1_1),
            ],
            "{endian:?}: every request on the wire, hop and restart alike, was 1.1"
        );
    }
}
