//! A forward **chain**, and where §9.6's restart goes when the chain had more
//! than one kind of hop in it.
//!
//! `forward_restart.rs` measures one hop at a time, which is what
//! [`Connection`] sees: it applies each hop as it takes it, so a chain is only
//! ever a sequence of single hops to it. The pool does not work that way — it
//! walks the whole chain inside one call and then tells the reference what
//! happened — and that is where the ordering of the hops starts to matter:
//!
//! * `permanent → temporary` re-points the reference at the permanent hop
//!   *and* caches the temporary one relative to it, so a §9.6 restart returns
//!   to the permanent hop. Reporting only the last hop cached the temporary
//!   target against the address the caller started from, and the restart went
//!   back through a hop the servant had already told the client to stop using
//!   — within the spec, one hop more than needed. That is what this file pins.
//! * `temporary → permanent` ends re-pointed with nothing cached, because a
//!   permanent hop supersedes the forwarding information it replaces.
//!
//! The peer is scripted TCP built out of this crate's own encoders — a
//! self-test of a decoded property, not an interop result, the same posture
//! `mux_pool.rs` and `forward_restart.rs` take. The axis varied is the
//! **reply's** byte order, for `mux_pool.rs`'s reason: the pool dials with the
//! connection's own order, so a scripted peer can vary what this client
//! *decodes* and nothing can vary what it encodes.

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_giop::pool::Pool;
use orbweaver_giop::server::{
    Request, decode_request, encode_close_connection, encode_location_forward, encode_reply,
};
use orbweaver_giop::{
    Connection, DEFAULT_MAX_MESSAGE_SIZE, Forward, IiopProfile, Invoker, Ior, ReplyStatus, Version,
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

/// How long the original waits, after forwarding, for a request that must not
/// come.
///
/// Proving a *negative* needs a window rather than an event: the restart it
/// must not receive would arrive within milliseconds on loopback, so this is
/// two orders of magnitude of margin, and its price is a second and a half per
/// byte order. It is a read deadline, so the wait sleeps in the kernel rather
/// than spinning.
const WATCH: Duration = Duration::from_millis(1500);

/// A value no other peer in this file ever sends, so a caller that reads it
/// has been answered by the original — which is the failure under test.
const ORIGINAL_ANSWERS: i32 = 99;

fn ior_at(addr: SocketAddr, key: &[u8]) -> Ior {
    Ior {
        type_id: "IDL:test/Chain:1.0".into(),
        profiles: vec![IiopProfile {
            // 1.2, because status 4 — `LOCATION_FORWARD_PERM` — is a 1.2 word:
            // below it a permanent redirect travels as status 3 and there is
            // no permanent hop to chain anything after.
            version: Version::V1_2,
            host: addr.ip().to_string(),
            port: addr.port(),
            object_key: key.to_vec(),
            components: Vec::new(),
        }],
    }
}

fn take_request(s: &mut TcpStream) -> Request {
    let msg = read_message(s, DEFAULT_MAX_MESSAGE_SIZE).expect("a request arrives");
    decode_request(msg).expect("a request decodes")
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

fn close(s: &mut TcpStream, req: &Request, endian: Endian) {
    let msg = encode_close_connection(req.version, endian).expect("encodes");
    s.write_all(&msg).expect("close goes out");
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

/// The original: forwards its one request **permanently**, and then stays on
/// that connection for [`WATCH`], answering anything else that arrives with
/// [`ORIGINAL_ANSWERS`] and counting it. A restart that comes back here is
/// therefore visible twice over: in the count, and in the value the caller
/// reads.
///
/// Staying on the connection rather than hanging up is what makes the count
/// mean anything. The pool would *reuse* this connection for a restart aimed
/// here, so an original that closed after forwarding would see no second dial
/// and no second request — and a count of zero would be true of both the
/// right behaviour and the wrong one. The wait is one blocking read under a
/// deadline, which is the sleeping wait loop in its smallest form.
fn original(
    permanently_to: Ior,
    reply_endian: Endian,
    seen_after: Arc<AtomicUsize>,
) -> (SocketAddr, mpsc::Receiver<()>) {
    scripted(move |l| {
        let (mut s, _) = l.accept().expect("the first call");
        let req = take_request(&mut s);
        assert_eq!(req.object_key, b"old", "the first call addresses the original key");
        forward_to(&mut s, &req, reply_endian, &Forward::Permanent(permanently_to));

        s.set_read_timeout(Some(WATCH)).expect("a deadline on the watch");
        if let Ok(msg) = read_message(&mut s, DEFAULT_MAX_MESSAGE_SIZE) {
            let req = decode_request(msg).expect("a request decodes");
            seen_after.fetch_add(1, Ordering::SeqCst);
            reply_long(&mut s, &req, reply_endian, ORIGINAL_ANSWERS);
        }
    })
}

/// The permanent hop: answers the forwarded call with a **temporary** forward,
/// and then answers the restart — on the same connection, because the pool
/// kept it, which is why the restart costs no dial at all.
fn permanent_hop(
    temporarily_to: Ior,
    reply_endian: Endian,
    seen: Arc<AtomicUsize>,
) -> (SocketAddr, mpsc::Receiver<()>) {
    scripted(move |l| {
        let (mut s, _) = l.accept().expect("the forwarded call");
        let req = take_request(&mut s);
        assert_eq!(req.object_key, b"perm", "the permanent hop is addressed by its own key");
        seen.fetch_add(1, Ordering::SeqCst);
        forward_to(&mut s, &req, reply_endian, &Forward::Temporary(temporarily_to));

        let req = take_request(&mut s);
        assert_eq!(req.object_key, b"perm", "the restart comes back to the permanent hop");
        seen.fetch_add(1, Ordering::SeqCst);
        reply_long(&mut s, &req, reply_endian, 7);
    })
}

/// The temporary hop: answers once, then dies the one way §13.5.1 makes safe
/// to re-send from — `CloseConnection`, with the listener already gone so the
/// pool's own redial is refused outright rather than landing in a backlog
/// Linux then turns into an RST (`forward_restart.rs`'s lesson, same shape).
fn temporary_hop(reply_endian: Endian) -> (SocketAddr, mpsc::Receiver<()>) {
    scripted(move |l| {
        let (mut s, _) = l.accept().expect("the temporary hop");
        drop(l);
        let req = take_request(&mut s);
        assert_eq!(req.object_key, b"tmp", "the temporary hop is addressed by its own key");
        reply_long(&mut s, &req, reply_endian, 42);
        let req = take_request(&mut s);
        close(&mut s, &req, reply_endian);
    })
}

/// `permanent → temporary`, then the temporary target dies: the restart lands
/// at the **permanent** hop, and the original is never asked again.
///
/// Four things are asserted about the same run, because each of them fails on
/// its own under the last-hop-only rule this replaces: the value the second
/// call returns (the permanent hop's, not the original's), the count of
/// requests at each peer, where [`Reference::ior`] points, and the number of
/// dials — the restart reuses the pooled connection to the permanent hop, so
/// it costs none.
#[test]
fn a_permanent_then_temporary_chain_restarts_at_the_permanent_hop() {
    for reply_endian in [Endian::Big, Endian::Little] {
        let label = format!("reply {reply_endian:?}");
        let (temp_addr, temp_done) = temporary_hop(reply_endian);
        let temp_ior = ior_at(temp_addr, b"tmp");

        let perm_seen = Arc::new(AtomicUsize::new(0));
        let (perm_addr, perm_done) = permanent_hop(temp_ior, reply_endian, Arc::clone(&perm_seen));
        let perm_ior = ior_at(perm_addr, b"perm");

        let orig_seen = Arc::new(AtomicUsize::new(0));
        let (orig_addr, orig_done) =
            original(perm_ior.clone(), reply_endian, Arc::clone(&orig_seen));
        let old = ior_at(orig_addr, b"old");

        let pool = Pool::new();
        let mut r = pool.reference(old.clone());
        assert!(r.forwarded().is_none(), "{label}: nothing followed before any call");

        // Call one walks the whole chain: original → permanent → temporary.
        assert_eq!(
            body_i32(&r.invoke("op", |_| {}).expect("the chain is followed")),
            42,
            "{label}: answered by the temporary hop"
        );
        assert_eq!(
            addressed(r.ior()),
            addressed(&perm_ior),
            "{label}: the permanent hop re-pointed the reference"
        );
        assert_eq!(
            r.forwarded().map(Forward::is_permanent),
            Some(false),
            "{label}: the temporary hop is the redirect in force"
        );

        // Call two goes straight to the cached temporary target, which closes
        // on it: §9.6's restart, and the address it restarts at is the
        // reference as it now stands.
        let reply = r.invoke("op", |_| {}).expect("the restart is answered");
        assert_eq!(
            body_i32(&reply),
            7,
            "{label}: answered by the permanent hop — {ORIGINAL_ANSWERS} would be the original"
        );
        assert_eq!(addressed(r.ior()), addressed(&perm_ior), "{label}: still the permanent hop");
        assert!(r.forwarded().is_none(), "{label}: at the reference itself again");
        assert_eq!(
            pool.stats().dialed,
            3,
            "{label}: one dial per peer — the restart reused the pooled permanent connection"
        );

        temp_done.recv_timeout(T).unwrap_or_else(|_| panic!("{label}: the temporary hop finished"));
        perm_done.recv_timeout(T).unwrap_or_else(|_| panic!("{label}: the permanent hop finished"));
        // Only now: the original's window has to close before its count means
        // anything.
        orig_done.recv_timeout(T).unwrap_or_else(|_| panic!("{label}: the original finished"));
        assert_eq!(
            perm_seen.load(Ordering::SeqCst),
            2,
            "{label}: the permanent hop saw the forwarded call and the restart"
        );
        assert_eq!(
            orig_seen.load(Ordering::SeqCst),
            0,
            "{label}: the original was never asked again"
        );
    }
}

/// The other ordering, for the rule rather than the case: `temporary →
/// permanent` ends re-pointed with **nothing** cached, because a permanent hop
/// clears the forwarding information it supersedes. The next call therefore
/// goes to the reference itself and is answered there, with no restart in it.
///
/// Without this the accumulation could be "keep the last of each kind" and
/// pass the test above; that rule would leave a temporary target cached across
/// a permanent hop that had just replaced it.
#[test]
fn a_temporary_then_permanent_chain_leaves_nothing_cached() {
    for reply_endian in [Endian::Big, Endian::Little] {
        let label = format!("reply {reply_endian:?}");

        // The permanent hop, which answers both calls — on **one** connection:
        // the pool dialled it for the hop and keeps it, so the second call
        // finds it rather than dialling again. A script that insisted on two
        // accepts would hang here, and that is the fact it would be asserting.
        let (perm_addr, perm_done) = scripted(move |l| {
            let (mut s, _) = l.accept().expect("the permanent hop's one connection");
            for _ in 0..2 {
                let req = take_request(&mut s);
                assert_eq!(req.object_key, b"perm", "addressed by its own key");
                reply_long(&mut s, &req, reply_endian, 7);
            }
        });
        let perm_ior = ior_at(perm_addr, b"perm");

        // The temporary hop, which forwards permanently onwards.
        let onwards = perm_ior.clone();
        let (temp_addr, temp_done) = scripted(move |l| {
            let (mut s, _) = l.accept().expect("the temporary hop");
            let req = take_request(&mut s);
            assert_eq!(req.object_key, b"tmp");
            forward_to(&mut s, &req, reply_endian, &Forward::Permanent(onwards));
        });
        let temp_ior = ior_at(temp_addr, b"tmp");

        // The original, which forwards temporarily and is then done.
        let (orig_addr, orig_done) = scripted(move |l| {
            let (mut s, _) = l.accept().expect("the first call");
            let req = take_request(&mut s);
            assert_eq!(req.object_key, b"old");
            forward_to(&mut s, &req, reply_endian, &Forward::Temporary(temp_ior));
        });
        let old = ior_at(orig_addr, b"old");

        let pool = Pool::new();
        let mut r = pool.reference(old);
        assert_eq!(body_i32(&r.invoke("op", |_| {}).expect("the chain is followed")), 7, "{label}");
        assert_eq!(
            addressed(r.ior()),
            addressed(&perm_ior),
            "{label}: the permanent hop re-pointed the reference"
        );
        assert_eq!(
            r.forwarded().map(Forward::is_permanent),
            Some(true),
            "{label}: the permanent hop is the redirect in force"
        );
        // The proof that nothing stale is cached: the temporary hop's script
        // has ended and its listener is gone, so a second call sent through
        // the old forwarding information could not be answered at all.
        temp_done.recv_timeout(T).unwrap_or_else(|_| panic!("{label}: the temporary hop finished"));
        orig_done.recv_timeout(T).unwrap_or_else(|_| panic!("{label}: the original finished"));
        assert_eq!(
            body_i32(&r.invoke("op", |_| {}).expect("answered at the reference itself")),
            7,
            "{label}"
        );
        perm_done.recv_timeout(T).unwrap_or_else(|_| panic!("{label}: the permanent hop finished"));
    }
}

/// The same chain one layer down, where it was never broken: `Connection`
/// applies each hop as it takes it, so this is a regression pin rather than a
/// fix — and having both halves measured against the same script is what makes
/// "the pool now does what the connection does" a checked sentence.
#[test]
fn the_connection_half_of_the_same_chain_restarts_at_the_permanent_hop_too() {
    for reply_endian in [Endian::Big, Endian::Little] {
        let label = format!("reply {reply_endian:?}");
        let (temp_addr, temp_done) = temporary_hop(reply_endian);
        let temp_ior = ior_at(temp_addr, b"tmp");

        let perm_reached = Arc::new(AtomicUsize::new(0));
        let perm_seen = Arc::clone(&perm_reached);
        // The connection redials the permanent hop rather than reusing a
        // pooled one, so this script has to accept twice.
        let (perm_addr, perm_done) = scripted(move |l| {
            let (mut s, _) = l.accept().expect("the forwarded call");
            let req = take_request(&mut s);
            assert_eq!(req.object_key, b"perm");
            perm_seen.fetch_add(1, Ordering::SeqCst);
            forward_to(&mut s, &req, reply_endian, &Forward::Temporary(temp_ior));
            let (mut s, _) = l.accept().expect("the restart");
            let req = take_request(&mut s);
            assert_eq!(req.object_key, b"perm", "the restart comes back to the permanent hop");
            perm_seen.fetch_add(1, Ordering::SeqCst);
            reply_long(&mut s, &req, reply_endian, 7);
        });
        let perm_ior = ior_at(perm_addr, b"perm");
        let perm_here = perm_ior.clone();

        // A `Connection` restart *redials*, so the proof that it did not come
        // back here is this script ending — and its listener with it. A
        // restart aimed at the original would be refused outright and the
        // call below would be an error rather than a 7.
        let (orig_addr, orig_done) = scripted(move |l| {
            let (mut s, _) = l.accept().expect("the first call");
            let req = take_request(&mut s);
            assert_eq!(req.object_key, b"old");
            forward_to(&mut s, &req, reply_endian, &Forward::Permanent(perm_here));
        });
        let old = ior_at(orig_addr, b"old");

        let mut c = Connection::connect(&old, T).expect("connect");
        assert_eq!(
            body_i32(&c.invoke_nullary("op").expect("the chain is followed")),
            42,
            "{label}"
        );
        assert_eq!(
            addressed(c.origin()),
            addressed(&perm_ior),
            "{label}: the permanent hop became the origin"
        );
        let reply = c.invoke_nullary("op").expect("the restart is answered");
        assert_eq!(body_i32(&reply), 7, "{label}: answered by the permanent hop");
        assert!(c.forwarded().is_none(), "{label}: at the origin again");

        temp_done.recv_timeout(T).unwrap_or_else(|_| panic!("{label}: the temporary hop finished"));
        perm_done.recv_timeout(T).unwrap_or_else(|_| panic!("{label}: the permanent hop finished"));
        orig_done.recv_timeout(T).unwrap_or_else(|_| panic!("{label}: the original finished"));
        assert_eq!(
            perm_reached.load(Ordering::SeqCst),
            2,
            "{label}: the permanent hop saw the forwarded call and the restart"
        );
    }
}
