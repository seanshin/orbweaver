//! Request multiplexing and connection pooling, against a peer that does what
//! no fixture we own will do.
//!
//! The case that matters — a reply overtaking an older one on the same
//! connection — cannot be produced by this project's own server, which reads
//! one request per connection and answers it before reading the next. Neither
//! omniORB nor JacORB produces it in their default configurations either (see
//! `spike-mux`, which measures that and says so). So the oracle here is the
//! specification: a scripted TCP peer that answers in whatever order the test
//! chooses, built out of this crate's own encoders so the bytes are the ones a
//! real peer would send.
//!
//! That makes these tests *self-tests of a decoded property*, not interop
//! results, and they are labelled as such on purpose — the same posture
//! `fragment_reception.rs` takes for the fragments no peer will send us.

use orbweaver_cdr::Endian;
use orbweaver_giop::guarded::Guarded;
use orbweaver_giop::mux::{Mux, Sent};
use orbweaver_giop::pool::{Limits, Pool};
use orbweaver_giop::server::{
    decode_request, encode_close_connection, encode_message_error, encode_reply,
};
use orbweaver_giop::{
    Connection, DEFAULT_MAX_MESSAGE_SIZE, Error, IiopProfile, Ior, MsgType, ReplyStatus, Version,
    fragment_message, read_message,
};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

/// Every wait in here answers to this. A concurrency test that can hang is not
/// a test.
const T: Duration = Duration::from_secs(10);

fn ior_at(addr: std::net::SocketAddr, key: &[u8], minor: u8) -> Ior {
    Ior {
        type_id: "IDL:test/Scripted:1.0".into(),
        profiles: vec![IiopProfile {
            version: Version { major: 1, minor },
            host: addr.ip().to_string(),
            port: addr.port(),
            object_key: key.to_vec(),
            components: Vec::new(),
        }],
    }
}

/// Reads one request and answers `id` with a `long` of `value`.
fn reply_long(s: &mut TcpStream, version: Version, endian: Endian, id: u32, value: i32) {
    let msg =
        encode_reply(version, endian, id, ReplyStatus::NoException, None, |e| e.put_i32(value))
            .expect("reply encodes");
    s.write_all(&msg).expect("reply goes out");
    s.flush().expect("flush");
}

/// Waits for one request and returns its id and operation.
fn take_request(s: &mut TcpStream) -> (u32, String, Version, Endian) {
    let msg = read_message(s, DEFAULT_MAX_MESSAGE_SIZE).expect("a request arrives");
    let req = decode_request(msg).expect("a request decodes");
    (req.request_id, req.operation.clone(), req.version, req.endian)
}

fn body_i32(reply: &orbweaver_giop::Reply) -> i32 {
    reply.body().expect("body").get_i32().expect("a long")
}

/// A peer whose script the test writes. Returns the listener's address and a
/// channel the script's thread reports on, so a failing script fails the test
/// instead of hanging it.
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

// ─────────────────────────────────────────────────────────────────────────────
// Multiplexing
// ─────────────────────────────────────────────────────────────────────────────

/// The whole point, measured rather than asserted: two requests on one
/// connection, answered newest-first, and each caller gets **its own** reply.
///
/// A correlation bug does not fail loudly here — it hands caller A caller B's
/// value — so the values differ and both are checked.
#[test]
fn replies_that_arrive_out_of_order_reach_the_right_caller() {
    let (addr, done) = scripted(|l| {
        let (mut s, _) = l.accept().expect("accept");
        let (first, _, v, e) = take_request(&mut s);
        let (second, _, _, _) = take_request(&mut s);
        // Answer the *newer* request first. This is the case GIOP's request_id
        // exists for and the case no fixture we own will produce.
        reply_long(&mut s, v, e, second, 222);
        reply_long(&mut s, v, e, first, 111);
    });

    let ior = ior_at(addr, b"key", 2);
    let mux = Mux::connect(&ior, T).expect("connect");
    assert!(mux.multiplexes(), "a 1.2 cleartext connection must multiplex");

    let a = mux.send(b"key", "first", |_| {}).expect("first goes out");
    let b = mux.send(b"key", "second", |_| {}).expect("second goes out");
    assert!(b.request_id() > a.request_id(), "ids are allocated in wire order");

    // Collect in *send* order, which is the opposite of the answer order: the
    // first caller must wait through a reply that is not its own.
    let ra = a.wait(T).expect("first is answered");
    let rb = b.wait(T).expect("second is answered");
    match (ra, rb) {
        (Sent::Reply(x), Sent::Reply(y)) => {
            assert_eq!(body_i32(&x), 111, "the first caller got somebody else's reply");
            assert_eq!(body_i32(&y), 222, "the second caller got somebody else's reply");
        }
        _ => panic!("expected two replies"),
    }

    let stats = mux.stats();
    assert_eq!(stats.peak_in_flight, 2, "two requests were outstanding at once");
    assert_eq!(stats.answered, 2);
    assert_eq!(stats.out_of_order, 1, "one reply overtook an older request");
    assert_eq!(stats.orphaned, 0);
    done.recv_timeout(T).expect("the peer finished its script");
}

/// One caller's deadline is one caller's problem: the connection stays usable
/// and the other callers on it are unaffected.
#[test]
fn a_caller_that_gives_up_does_not_take_the_connection_with_it() {
    let (addr, done) = scripted(|l| {
        let (mut s, _) = l.accept().expect("accept");
        let (_ignored, _, v, e) = take_request(&mut s);
        let (second, _, _, _) = take_request(&mut s);
        reply_long(&mut s, v, e, second, 7);
        // The first request is never answered. Then a third arrives and is.
        let (third, _, _, _) = take_request(&mut s);
        reply_long(&mut s, v, e, third, 9);
    });

    let ior = ior_at(addr, b"key", 2);
    let mux = Mux::connect(&ior, T).expect("connect");
    let abandoned = mux.send(b"key", "never_answered", |_| {}).expect("goes out");
    let answered = mux.send(b"key", "answered", |_| {}).expect("goes out");

    match answered.wait(T) {
        Ok(Sent::Reply(r)) => assert_eq!(body_i32(&r), 7),
        other => panic!("the answered call must succeed, got {other:?}"),
    }

    let waited = std::time::Instant::now();
    match abandoned.wait(Duration::from_millis(300)) {
        Err(f) => match f.error {
            Error::Timeout { request_id, .. } => assert_ne!(request_id, 0),
            other => panic!("expected a call timeout, got {other}"),
        },
        Ok(_) => panic!("nothing answered this one"),
    }
    assert!(waited.elapsed() < T, "the timeout must be the caller's, not the socket's");
    assert!(!f_unusable(&mux), "one caller's patience says nothing about the connection");

    // And the connection really is still good.
    match mux.call_on(b"key", "after", |_| {}, T) {
        Ok(Sent::Reply(r)) => assert_eq!(body_i32(&r), 9),
        other => panic!("the connection must still carry calls, got {other:?}"),
    }
    done.recv_timeout(T).expect("the peer finished its script");
}

fn f_unusable(mux: &Mux) -> bool {
    !mux.is_usable()
}

/// §13.5.1: a `CloseConnection` says *every* outstanding request went
/// unprocessed. With N in flight, all N callers have to hear that — and hear
/// it as re-sendable, which is what makes the pool's retry legitimate.
#[test]
fn close_connection_tells_every_waiting_caller_the_request_was_not_processed() {
    let (addr, done) = scripted(|l| {
        let (mut s, _) = l.accept().expect("accept");
        let (_a, _, v, e) = take_request(&mut s);
        let (_b, _, _, _) = take_request(&mut s);
        let bye = encode_close_connection(v, e).expect("close encodes");
        s.write_all(&bye).expect("goodbye goes out");
        s.flush().expect("flush");
    });

    let ior = ior_at(addr, b"key", 2);
    let mux = Mux::connect(&ior, T).expect("connect");
    let a = mux.send(b"key", "one", |_| {}).expect("goes out");
    let b = mux.send(b"key", "two", |_| {}).expect("goes out");

    for pending in [a, b] {
        let f = pending.wait(T).expect_err("a closed connection answers nobody");
        assert!(matches!(f.error, Error::ConnectionClosed), "got {}", f.error);
        assert!(f.unsent, "§13.5.1 says these were not processed");
    }
    assert!(!mux.is_usable(), "a closed connection must not be handed out again");
    done.recv_timeout(T).expect("the peer finished its script");
}

/// Multiplexing must not create the interleaving the reassembler was written
/// to reject. Here the *peer* does it — a fragmented reply with another
/// message pushed into the middle — and the requirement is that nobody gets a
/// plausible wrong value.
#[test]
fn a_message_interleaved_into_a_fragmented_reply_faults_instead_of_misattributing() {
    let (addr, done) = scripted(|l| {
        let (mut s, _) = l.accept().expect("accept");
        let (first, _, v, e) = take_request(&mut s);
        let (second, _, _, _) = take_request(&mut s);
        // A reply big enough to fragment, then somebody else's reply shoved
        // between the pieces.
        let big = encode_reply(v, e, first, ReplyStatus::NoException, None, |enc| {
            enc.put_octet_seq(&vec![0u8; 4096]);
        })
        .expect("reply encodes");
        let pieces = fragment_message(big, 512).expect("fragments");
        assert!(pieces.len() > 2, "the test needs a genuinely fragmented reply");
        s.write_all(&pieces[0]).expect("leading piece");
        let interloper =
            encode_reply(v, e, second, ReplyStatus::NoException, None, |enc| enc.put_i32(5))
                .expect("reply encodes");
        s.write_all(&interloper).expect("the interleaved message");
        for p in &pieces[1..] {
            let _ = s.write_all(p);
        }
        let _ = s.flush();
    });

    let ior = ior_at(addr, b"key", 2);
    let mux = Mux::connect(&ior, T).expect("connect");
    let a = mux.send(b"key", "fragmented", |_| {}).expect("goes out");
    let b = mux.send(b"key", "interleaved", |_| {}).expect("goes out");

    let fa = a.wait(T).expect_err("an interleaved reply must not be accepted");
    let fb = b.wait(T).expect_err("and its neighbour must not be handed the pieces");
    for f in [&fa, &fb] {
        assert!(
            matches!(f.error, Error::UnexpectedMessage(_) | Error::Desynchronized),
            "expected a refusal, got {}",
            f.error
        );
        assert!(!f.unsent, "these requests may well have run; re-sending is not safe");
    }
    assert!(!mux.is_usable());
    done.recv_timeout(T).expect("the peer finished its script");
}

/// Answers `id` with a reply big enough to fragment — and then sends only its
/// leading piece, which is the state §9.4.9 calls a message in progress and the
/// state every test below interrupts.
fn start_a_fragmented_reply(s: &mut TcpStream, version: Version, endian: Endian, id: u32) {
    let big = encode_reply(version, endian, id, ReplyStatus::NoException, None, |e| {
        e.put_octet_seq(&vec![0u8; 4096]);
    })
    .expect("reply encodes");
    let pieces = fragment_message(big, 512).expect("fragments");
    assert!(pieces.len() > 2, "the test needs a genuinely fragmented reply");
    s.write_all(&pieces[0]).expect("the leading piece goes out");
    s.flush().expect("flush");
}

/// One `CloseConnection`, two truths — and a fault that used to tell both
/// callers the same one.
///
/// §13.5.1 promises that requests *without replies* were not processed, and it
/// is the whole basis of the pool's re-send. The caller whose reply had already
/// begun arriving is the one request that promise does not describe: the peer
/// demonstrably processed it. Telling that caller it may re-send would run a
/// non-idempotent operation twice; telling the *other* caller it may not turns
/// a routine server shutdown into a failed call. Neither is acceptable, so the
/// answer is per caller.
#[test]
fn a_close_between_reply_fragments_is_re_sendable_for_everyone_but_the_call_it_cut() {
    let (idtx, idrx) = mpsc::channel();
    let (addr, done) = scripted(move |l| {
        let (mut s, _) = l.accept().expect("accept");
        let (first, _, v, e) = take_request(&mut s);
        let (_second, _, _, _) = take_request(&mut s);
        idtx.send(first).expect("report which call gets cut");
        start_a_fragmented_reply(&mut s, v, e, first);
        s.write_all(&encode_close_connection(v, e).expect("close encodes")).expect("goodbye");
        s.flush().expect("flush");
    });

    let ior = ior_at(addr, b"key", 2);
    let mux = Mux::connect(&ior, T).expect("connect");
    let cut = mux.send(b"key", "half_answered", |_| {}).expect("goes out");
    let untouched = mux.send(b"key", "never_answered", |_| {}).expect("goes out");
    assert_eq!(cut.request_id(), idrx.recv_timeout(T).expect("the peer names the call it cuts"));

    let fa = cut.wait(T).expect_err("half a reply is not an answer");
    let fb = untouched.wait(T).expect_err("and the other call was never answered at all");

    assert!(
        matches!(
            fa.error,
            Error::InterruptedMidReassembly { control: MsgType::CloseConnection, .. }
        ),
        "the cut call must hear that its reply had started: got {}",
        fa.error
    );
    assert!(!fa.unsent, "the peer had begun answering this one, so re-sending would repeat it");
    assert!(matches!(fb.error, Error::ConnectionClosed), "got {}", fb.error);
    assert!(fb.unsent, "§13.5.1 covers a request that got nothing back");
    assert!(
        fa.error.is_orderly_close() && fb.error.is_orderly_close(),
        "both callers met a teardown, and neither may read it as corruption"
    );
    assert!(!mux.is_usable(), "a closed connection must not be handed out again");
    done.recv_timeout(T).expect("the peer finished its script");
}

/// The other control message, and the opposite conclusion. §9.4.8's
/// `MessageError` says the peer could not parse something **we** sent; it is a
/// report rather than damage, so it is not corruption — but it names nothing,
/// so it makes no request safe to re-send and it is not a goodbye either.
#[test]
fn a_message_error_between_reply_fragments_is_a_report_nobody_may_re_send() {
    let (addr, done) = scripted(|l| {
        let (mut s, _) = l.accept().expect("accept");
        let (id, _, v, e) = take_request(&mut s);
        start_a_fragmented_reply(&mut s, v, e, id);
        s.write_all(&encode_message_error(e).expect("message error encodes")).expect("the report");
        s.flush().expect("flush");
    });

    let ior = ior_at(addr, b"key", 2);
    let mux = Mux::connect(&ior, T).expect("connect");
    let f =
        mux.call_on(b"key", "half_answered", |_| {}, T).expect_err("half a reply is not an answer");
    assert!(
        matches!(f.error, Error::InterruptedMidReassembly { control: MsgType::MessageError, .. }),
        "got {}",
        f.error
    );
    assert!(!f.unsent, "a MessageError names no request, so it promises nothing about any of them");
    assert!(!f.error.is_orderly_close(), "a report is not a goodbye");
    done.recv_timeout(T).expect("the peer finished its script");
}

/// The invoker that is not the mux: one connection, one call, and the same
/// distinction. A variant nobody surfaces is not a fix, so this checks the
/// simplest caller in the crate sees it too — and that it is *not* quietly
/// rewritten into [`Error::ConnectionClosed`] on the way out.
#[test]
fn a_single_connection_reports_a_close_between_reply_fragments_as_a_teardown() {
    let (idtx, idrx) = mpsc::channel();
    let (addr, done) = scripted(move |l| {
        let (mut s, _) = l.accept().expect("accept");
        let (id, _, v, e) = take_request(&mut s);
        idtx.send(id).expect("report the id");
        start_a_fragmented_reply(&mut s, v, e, id);
        s.write_all(&encode_close_connection(v, e).expect("close encodes")).expect("goodbye");
        s.flush().expect("flush");
        // Held open until the client hangs up, so the goodbye cannot be raced
        // away by a reset.
        let _ = read_message(&mut s, DEFAULT_MAX_MESSAGE_SIZE);
    });

    let ior = ior_at(addr, b"key", 2);
    let mut conn = Connection::connect(&ior, T).expect("connect");
    let err = conn.invoke_nullary("half_answered").expect_err("half a reply is not an answer");
    assert!(err.is_orderly_close(), "the peer said goodbye; got {err}");
    let sent = idrx.recv_timeout(T).expect("the peer reported the id");
    match err {
        Error::InterruptedMidReassembly { control, partial, request_id, received } => {
            assert_eq!(control, MsgType::CloseConnection);
            assert_eq!(partial, MsgType::Reply);
            assert_eq!(request_id, sent, "the caller must be able to name the call that was cut");
            assert_eq!(received, 1, "only the leading piece arrived");
        }
        other => panic!("expected an interrupted reassembly, got {other}"),
    }
    assert!(!conn.is_usable(), "the message can never complete now; the connection is spent");
    drop(conn);
    done.recv_timeout(T).expect("the peer finished its script");
}

/// The version rule, at the boundary it is decided on: GIOP 1.1 refuses to put
/// a second request in flight, and still carries calls one at a time.
#[test]
fn giop_1_1_refuses_to_pipeline_and_serializes_instead() {
    let (addr, done) = scripted(|l| {
        let (mut s, _) = l.accept().expect("accept");
        for value in [1, 2] {
            let (id, _, v, e) = take_request(&mut s);
            assert_eq!(v.minor, 1, "the profile advertised 1.1, so the request must be 1.1");
            reply_long(&mut s, v, e, id, value);
        }
    });

    let ior = ior_at(addr, b"key", 1);
    let mux = Mux::connect(&ior, T).expect("connect");
    assert!(!mux.multiplexes(), "1.1 must not multiplex");
    assert_eq!(mux.version(), Version::V1_1);

    match mux.send(b"key", "pipelined", |_| {}) {
        Err(Error::MultiplexingUnsupported { version }) => assert_eq!(version, Version::V1_1),
        other => panic!("1.1 must refuse to pipeline, got {other:?}"),
    }

    for expected in [1, 2] {
        match mux.call_on(b"key", "serial", |_| {}, T) {
            Ok(Sent::Reply(r)) => assert_eq!(body_i32(&r), expected),
            other => panic!("a 1.1 call must still work, got {other:?}"),
        }
    }
    assert_eq!(mux.stats().peak_in_flight, 1, "1.1 keeps exactly one request in flight");
    done.recv_timeout(T).expect("the peer finished its script");
}

/// Concurrency from real threads, not from one thread pretending: K callers,
/// one connection, and the connection's own counter has to witness the
/// overlap. The peer holds every request until all K have arrived, so a
/// serializing client would deadlock against the rendezvous and fail on the
/// deadline rather than pass quietly.
#[test]
fn many_threads_share_one_connection() {
    const K: u32 = 8;
    let (addr, done) = scripted(|l| {
        let (mut s, _) = l.accept().expect("accept");
        let mut seen = Vec::new();
        for _ in 0..K {
            let (id, _, v, e) = take_request(&mut s);
            seen.push((id, v, e));
        }
        // Answer in reverse, so correlation is under test at the same time.
        for (i, (id, v, e)) in seen.iter().enumerate().rev() {
            reply_long(&mut s, *v, *e, *id, i as i32);
        }
    });

    let ior = ior_at(addr, b"key", 2);
    let mux = Mux::connect(&ior, T).expect("connect");
    let answers: Vec<i32> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..K)
            .map(|_| {
                let m = mux.clone();
                scope.spawn(move || match m.call_on(b"key", "concurrent", |_| {}, T) {
                    Ok(Sent::Reply(r)) => body_i32(&r),
                    other => panic!("every caller must be answered, got {other:?}"),
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("no caller panicked")).collect()
    });

    let mut sorted = answers.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..K as i32).collect::<Vec<_>>(), "each caller got a distinct reply");
    let stats = mux.stats();
    assert_eq!(stats.peak_in_flight, K as usize, "all {K} were on the wire at once");
    assert_eq!(stats.answered, K as u64);
    assert!(stats.out_of_order > 0, "answering in reverse must be visible in the counters");
    done.recv_timeout(T).expect("the peer finished its script");
}

// ─────────────────────────────────────────────────────────────────────────────
// Pooling
// ─────────────────────────────────────────────────────────────────────────────

/// Two references, one endpoint, one connection — with different object keys,
/// which is the case that proves the key is per *endpoint* and not per
/// reference.
#[test]
fn two_references_to_one_endpoint_share_a_connection() {
    let (addr, done) = scripted(|l| {
        let (mut s, _) = l.accept().expect("accept");
        for value in [10, 20] {
            let (id, _, v, e) = take_request(&mut s);
            reply_long(&mut s, v, e, id, value);
        }
        // A second connection would be a test failure, and this is where it
        // would show up: nothing ever accepts one.
    });

    let pool = Pool::new();
    let one = ior_at(addr, b"object-one", 2);
    let two = ior_at(addr, b"object-two", 2);
    assert_eq!(body_i32(&pool.invoke(&one, "a", |_| {}).expect("first call")), 10);
    assert_eq!(body_i32(&pool.invoke(&two, "b", |_| {}).expect("second call")), 20);

    let stats = pool.stats();
    assert_eq!(stats.dialed, 1, "the second reference must not have dialed");
    assert_eq!(stats.reused, 1);
    assert_eq!(pool.size(), 1);
    done.recv_timeout(T).expect("the peer finished its script");
}

/// A pooled connection is one the caller never asked for, so a server closing
/// it must not surface as their error. §13.5.1 makes the re-send legitimate;
/// this checks it actually happens and that the caller sees only the answer.
#[test]
fn a_pooled_connection_closed_under_us_is_retried_invisibly() {
    let (addr, done) = scripted(|l| {
        // First connection: say goodbye instead of answering.
        let (mut first, _) = l.accept().expect("accept");
        let (_id, _, v, e) = take_request(&mut first);
        first.write_all(&encode_close_connection(v, e).expect("close encodes")).expect("goodbye");
        first.flush().expect("flush");
        drop(first);
        // Second connection: answer properly.
        let (mut second, _) = l.accept().expect("accept the retry");
        let (id, _, v, e) = take_request(&mut second);
        reply_long(&mut second, v, e, id, 42);
    });

    let pool = Pool::new();
    let ior = ior_at(addr, b"key", 2);
    let reply = pool.invoke(&ior, "op", |_| {}).expect("the retry must be invisible to the caller");
    assert_eq!(body_i32(&reply), 42);

    let stats = pool.stats();
    assert_eq!(stats.retried, 1, "exactly one re-send");
    assert_eq!(stats.dialed, 2, "the closed connection was discarded and a fresh one dialed");
    done.recv_timeout(T).expect("the peer finished its script");
}

/// A server that answers every request with `CloseConnection` is refusing, and
/// the pool must stop rather than loop: the promise is spent once.
#[test]
fn a_peer_that_only_says_goodbye_is_reported_not_retried_forever() {
    let (addr, done) = scripted(|l| {
        for _ in 0..2 {
            let (mut s, _) = l.accept().expect("accept");
            let (_id, _, v, e) = take_request(&mut s);
            let _ = s.write_all(&encode_close_connection(v, e).expect("close encodes"));
            let _ = s.flush();
        }
        // A third accept would mean the pool looped. The script ends here, so
        // a looping pool would fail on its own connect rather than spin.
    });

    let pool = Pool::new();
    let ior = ior_at(addr, b"key", 2);
    let err = pool.invoke(&ior, "op", |_| {}).expect_err("a refusing server must be reported");
    assert!(matches!(err, Error::ConnectionClosed), "got {err}");
    assert_eq!(pool.stats().retried, 1, "once, not until it works");
    done.recv_timeout(T).expect("the peer finished its script");
}

/// The retry has a limit that is not a count: a call the peer had *begun*
/// answering is never re-sent, however invisible the connection was.
///
/// This is the one place the pool's kindness would become a defect. Hiding a
/// close is legitimate because §13.5.1 says the request was not processed —
/// and that sentence stops being true the moment a reply starts coming back.
/// Re-sending here would turn one server shutdown into one duplicated
/// operation, silently, on a connection the caller never asked for.
#[test]
fn a_call_the_peer_had_begun_answering_is_reported_rather_than_retried() {
    let (addr, done) = scripted(|l| {
        let (mut s, _) = l.accept().expect("accept");
        let (id, _, v, e) = take_request(&mut s);
        start_a_fragmented_reply(&mut s, v, e, id);
        s.write_all(&encode_close_connection(v, e).expect("close encodes")).expect("goodbye");
        s.flush().expect("flush");
        // No second accept: a pool that retried would find nothing here, and
        // the counter below says whether it tried.
    });

    let pool = Pool::new();
    let ior = ior_at(addr, b"key", 2);
    let err =
        pool.invoke(&ior, "half_answered", |_| {}).expect_err("half a reply is not an answer");
    assert!(
        matches!(err, Error::InterruptedMidReassembly { control: MsgType::CloseConnection, .. }),
        "got {err}"
    );
    let stats = pool.stats();
    assert_eq!(stats.retried, 0, "the operation may have run; a hidden re-send would repeat it");
    assert_eq!(stats.dialed, 1, "and nothing may have been dialed to repeat it on");
    done.recv_timeout(T).expect("the peer finished its script");
}

/// The bound is a bound. With one slot and a busy connection there is nothing
/// to evict, and the pool refuses rather than dialing anyway — an unbounded
/// pool being the file-descriptor leak this exists to prevent.
#[test]
fn the_pool_refuses_rather_than_exceeding_its_bound() {
    let (busy_addr, busy_done) = scripted(|l| {
        let (mut s, _) = l.accept().expect("accept");
        let (id, _, v, e) = take_request(&mut s);
        // Hold the call open long enough for the bound to be tested, then
        // answer so nothing leaks.
        std::thread::sleep(Duration::from_millis(400));
        reply_long(&mut s, v, e, id, 1);
    });
    let idle = TcpListener::bind("127.0.0.1:0").expect("bind");
    let idle_addr = idle.local_addr().expect("addr");

    let pool = Pool::with_limits(Limits { max_total: 1, ..Limits::default() });
    let (mux, key) = pool.acquire(&ior_at(busy_addr, b"key", 2)).expect("the first fits");
    let pending = mux.send(&key, "slow", |_| {}).expect("goes out");

    // A different endpoint: nothing in the pool can serve it, and the one
    // connection there is has a call on it.
    let refused = pool.acquire(&ior_at(idle_addr, b"key", 2));
    match refused {
        Err(Error::PoolExhausted { limit }) => assert_eq!(limit, 1),
        other => panic!("the bound must be refused, not exceeded: {other:?}"),
    }
    assert_eq!(pool.stats().refused, 1);

    pending.wait(T).expect("the held call still completes");
    busy_done.recv_timeout(T).expect("the peer finished its script");
    drop(idle);
}

/// Idle connections are the ones eviction may take, and a connection is idle
/// only when nothing is in flight on it. With a zero idle bound the pool
/// re-dials every time — which is the wrong policy but the right test of the
/// mechanism.
#[test]
fn an_idle_connection_is_evicted_rather_than_reused() {
    let (addr, done) = scripted(|l| {
        for value in [1, 2] {
            let (mut s, _) = l.accept().expect("accept");
            let (id, _, v, e) = take_request(&mut s);
            reply_long(&mut s, v, e, id, value);
        }
    });

    let pool = Pool::with_limits(Limits { max_idle: Duration::ZERO, ..Limits::default() });
    let ior = ior_at(addr, b"key", 2);
    assert_eq!(body_i32(&pool.invoke(&ior, "a", |_| {}).expect("first")), 1);
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(body_i32(&pool.invoke(&ior, "b", |_| {}).expect("second")), 2);

    let stats = pool.stats();
    assert_eq!(stats.dialed, 2, "an expired connection must not be handed out");
    assert!(stats.idle_evicted >= 1, "and it must be dropped, not merely skipped");
    done.recv_timeout(T).expect("the peer finished its script");
}

// ─────────────────────────────────────────────────────────────────────────────
// The lock discipline
// ─────────────────────────────────────────────────────────────────────────────

/// The rule the pool is most likely to break, checked directly: dialing while
/// a lock section is open is caught, so a pool that dialed inside its own lock
/// could not pass its own tests.
///
/// This is why `Pool::acquire` is written as look, then dial, then file —
/// three steps and two lock sections, with the blocking one in between.
#[test]
fn dialing_from_inside_a_lock_section_is_refused() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let ior = ior_at(addr, b"key", 2);

    let state = Guarded::new("a servant that should not dial", ());
    let tripped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.read(|_| {
            let _ = Connection::connect(&ior, T);
        });
    }));
    assert!(tripped.is_err(), "a dial from inside a lock section must be caught");
    assert_eq!(orbweaver_giop::guarded::section_held(), None, "the section must have closed");
}

/// And the same tripwire covers the pool's own entry points, because the pool
/// keeps its state in a `Guarded` like everybody else.
#[test]
fn acquiring_from_inside_a_lock_section_is_refused() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let ior = ior_at(addr, b"key", 2);
    let pool = Pool::new();

    let state = Guarded::new("a servant holding its own state", ());
    let tripped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.read(|_| {
            let _ = pool.acquire(&ior);
        });
    }));
    assert!(tripped.is_err(), "the pool must not be reachable from inside a servant's lock");
    assert_eq!(orbweaver_giop::guarded::section_held(), None);
}

/// A multiplexed invocation is a blocking call like any other, so the outbound
/// tripwire has to cover it too — otherwise the one path built for
/// concurrency would be the one path that could deadlock a servant.
#[test]
fn a_multiplexed_call_from_inside_a_lock_section_is_refused() {
    let (addr, done) = scripted(|l| {
        let (mut s, _) = l.accept().expect("accept");
        let (id, _, v, e) = take_request(&mut s);
        reply_long(&mut s, v, e, id, 1);
    });
    let ior = ior_at(addr, b"key", 2);
    let mux = Mux::connect(&ior, T).expect("connect");

    let state = Guarded::new("a servant mid-operation", ());
    let tripped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        state.read(|_| {
            let _ = mux.call_on(b"key", "op", |_| {}, T);
        });
    }));
    assert!(tripped.is_err(), "an invocation from inside a lock section must be caught");

    // The connection is untouched by the refusal, and the peer's one scripted
    // reply is still there to be collected.
    match mux.call_on(b"key", "op", |_| {}, T) {
        Ok(Sent::Reply(r)) => assert_eq!(body_i32(&r), 1),
        other => panic!("the connection must still work, got {other:?}"),
    }
    done.recv_timeout(T).expect("the peer finished its script");
}
