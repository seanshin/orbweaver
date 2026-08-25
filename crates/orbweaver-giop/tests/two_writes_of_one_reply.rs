//! A peer that closes the connection **between two writes of one reply**, and
//! what each caller on that connection is told.
//!
//! `docs/decisions/D010` §4 B5 records this shape as class B — buildable, but
//! its oracle absent — on the ground that *"neither installed ORB will shut
//! down inside the window between two fragments on command"*. That is true of
//! omniORB and JacORB and it is the wrong conclusion, because the peer this
//! needs is not an ORB: it is a socket that writes GIOP by hand. `mux_pool.rs`
//! already builds one in-process and measures the claim once. This file is
//! about the axes that one does not vary, each of which is a way the claim
//! could be true by accident.
//!
//! **Byte order.** [`Connection`]'s `endian` is `Endian::native()`, and the
//! scripted peers in `mux_pool.rs` reply in whatever order the request arrived
//! in — so on any one machine every existing measurement of this path runs in
//! one byte order. GIOP flags are per message: a peer may answer a
//! little-endian request in big-endian bytes, and the id that decides which
//! caller is which is read out of *the reply's* header. So the peer here picks
//! its reply's byte order independently of the request's, and CLAUDE.md's "test
//! both byte orders" is a rule about this exact hazard.
//!
//! **Which caller was cut.** A peer that always cuts the first request cannot
//! distinguish "the answer follows the request id on the wire" from "the first
//! caller is the one that hears about it". Here the peer cuts either one.
//!
//! **Who reads the socket.** [`crate::mux`]'s reader is whichever caller is
//! waiting, so the caller that collects first is the one that reads the close
//! and records the fault. Both orders are run: the truth a caller is told must
//! not depend on whether it was the leader.
//!
//! **How much of the reply arrived, and how long the window was.** `received`
//! counts the leading message plus every fragment before the interruption, and
//! the window is the peer's to choose. Both are varied so neither is a
//! constant the assertions happen to match.
//!
//! Every byte the peer writes is built here from §9.4, **not** from this
//! crate's encoders — the same posture `fragment_reception.rs` takes, for the
//! same reason: an encoder and a decoder that share a bug agree with each
//! other. The separate-process, other-language version of the same peer is
//! `spikes/half_reply_peer.py`, driven by `spikes/half_reply.sh`.
//!
//! *한 응답의 두 번의 쓰기 **사이에** 연결을 끊는 피어. 설치된 두 ORB는 명령으로
//! 그렇게 하지 못하지만, 이 피어는 ORB가 아니라 손으로 GIOP를 쓰는 소켓이다.
//! 바이트 순서 · 잘린 쪽 · 소켓을 읽는 쪽 · 도착한 조각 수 · 창의 길이를 모두
//! 바꿔가며 잰다.*

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

use orbweaver_cdr::Endian;
use orbweaver_giop::mux::{Failed, Mux};
use orbweaver_giop::{Error, HEADER_LEN, IiopProfile, Ior, MAGIC, MsgType, Version};

/// Every wait answers to this. A socket test that can hang is not a test.
const T: Duration = Duration::from_secs(10);

// ─────────────────────────────────────────────────────────────────────────────
// The bytes, built from §9.4 rather than from our own encoders
// ─────────────────────────────────────────────────────────────────────────────

fn u32_bytes(v: u32, endian: Endian) -> [u8; 4] {
    match endian {
        Endian::Big => v.to_be_bytes(),
        Endian::Little => v.to_le_bytes(),
    }
}

fn u32_at(b: &[u8], endian: Endian) -> u32 {
    let w = [b[0], b[1], b[2], b[3]];
    match endian {
        Endian::Big => u32::from_be_bytes(w),
        Endian::Little => u32::from_le_bytes(w),
    }
}

/// A GIOP 1.2 message header (§9.4.1) with `payload` after it.
fn message(msg_type: MsgType, endian: Endian, more: bool, payload: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(HEADER_LEN + payload.len());
    m.extend_from_slice(MAGIC);
    m.push(1);
    m.push(2);
    m.push(endian.as_flag() | if more { 0b10 } else { 0 });
    m.push(msg_type as u8);
    m.extend_from_slice(&u32_bytes(payload.len() as u32, endian));
    m.extend_from_slice(payload);
    // §9.4.9: every piece of a fragmented message but the last is a multiple of
    // eight octets, header included. Asserted rather than assumed, because a
    // peer that got this wrong would be testing the reader's tolerance instead
    // of the thing this file is about.
    if more {
        assert_eq!(m.len() % 8, 0, "a non-final piece must be 8-aligned; got {}", m.len());
    }
    m
}

/// The **first** write of a reply that will never have a second one: a leading
/// `Reply` with the more-fragments bit set.
///
/// `ReplyHeader_1_2` is `request_id`, `reply_status`, then the service context
/// list; with no contexts that is twelve octets, which leaves the body starting
/// at offset 24 — already 8-aligned — so eight octets of body make a 32-octet
/// piece. Nothing past the request id is ever decoded, because the message can
/// never complete; it is written correctly anyway so that what is measured is
/// the interruption and not a malformed reply.
fn first_write_of_a_reply(request_id: u32, endian: Endian) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&u32_bytes(request_id, endian));
    payload.extend_from_slice(&u32_bytes(0, endian)); // NO_EXCEPTION
    payload.extend_from_slice(&u32_bytes(0, endian)); // no service contexts
    payload.extend_from_slice(&[0u8; 8]); // eight octets of body
    message(MsgType::Reply, endian, true, &payload)
}

/// A §9.4.9 continuation: `FragmentHeader_1_2` is the request id, then payload.
fn another_write_of_the_same_reply(request_id: u32, endian: Endian) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&u32_bytes(request_id, endian));
    payload.extend_from_slice(&[0u8; 8]);
    message(MsgType::Fragment, endian, true, &payload)
}

/// §9.4.7 and §9.4.8: a header and nothing else.
fn control(msg_type: MsgType, endian: Endian) -> Vec<u8> {
    message(msg_type, endian, false, &[])
}

/// Reads one whole `Request` off the socket by hand and returns its id.
///
/// Hand-parsed for the same reason the replies are hand-built: `decode_request`
/// and `encode_request` share a definition of the header, and a peer that used
/// it would agree with the client about a layout neither had checked.
fn take_request_id(s: &mut TcpStream) -> u32 {
    let mut header = [0u8; HEADER_LEN];
    s.read_exact(&mut header).expect("a GIOP header arrives");
    assert_eq!(&header[..4], MAGIC, "not a GIOP message");
    assert_eq!(header[7], MsgType::Request as u8, "expected a Request");
    assert_eq!(header[6] & 0b10, 0, "the client fragmented a request this test never made big");
    let endian = if header[6] & 1 == 1 { Endian::Little } else { Endian::Big };
    let size = u32_at(&header[8..], endian) as usize;
    let mut body = vec![0u8; size];
    s.read_exact(&mut body).expect("the whole request arrives");
    // `RequestHeader_1_2` opens with the request id, whatever follows it.
    u32_at(&body, endian)
}

// ─────────────────────────────────────────────────────────────────────────────
// The peer
// ─────────────────────────────────────────────────────────────────────────────

/// What the peer was told to do. Every field is an axis the existing
/// measurement holds constant.
#[derive(Debug, Clone, Copy)]
struct Script {
    /// The byte order the peer answers in — deliberately not the request's.
    reply_endian: Endian,
    /// Which of the two requests gets the reply that is never finished.
    cut: usize,
    /// Extra `Fragment`s written before the peer stops. `received` must be
    /// one more than this.
    continuations: usize,
    /// How long the peer waits between the last write of the reply and the
    /// control message. The window, made a knob so the result cannot be a
    /// race that happens to land the same way every time.
    window: Duration,
    /// `CloseConnection` (a goodbye) or `MessageError` (a report).
    control: MsgType,
    /// Which caller collects first, and so becomes the reader that sees the
    /// interruption and records the fault.
    collects_first: usize,
}

/// The peer, on its own thread, reporting the id it cut so the test can check
/// the client heard about the same call the peer chose.
fn peer(script: Script) -> (SocketAddr, mpsc::Receiver<u32>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        // Blocking `accept` on a listener that was bound before the client was
        // told the port: the harness's accept rule is about a *non-blocking*
        // single accept, which this is not.
        let (mut s, _) = listener.accept().expect("accept");
        let ids: Vec<u32> = (0..2).map(|_| take_request_id(&mut s)).collect();
        let cut = ids[script.cut];

        // Write one. From here the peer owes a continuation and will not send
        // one.
        s.write_all(&first_write_of_a_reply(cut, script.reply_endian)).expect("the first write");
        for _ in 0..script.continuations {
            s.write_all(&another_write_of_the_same_reply(cut, script.reply_endian))
                .expect("another write of the same reply");
        }
        s.flush().expect("flush");

        // The window. The client is blocked in `read` across it, so its length
        // decides nothing — which is exactly what varying it measures.
        std::thread::sleep(script.window);

        // Write two, and it is not the reply.
        s.write_all(&control(script.control, script.reply_endian)).expect("the control message");
        s.flush().expect("flush");
        tx.send(cut).expect("report the call that was cut");

        // Held open until the client hangs up. Closing with the goodbye still
        // in flight can reach the client as a reset instead, which would make
        // this a test of macOS's socket teardown rather than of the reader.
        let _ = s.read(&mut [0u8; 1]);
    });
    (addr, rx)
}

fn ior_at(addr: SocketAddr) -> Ior {
    Ior {
        type_id: "IDL:test/HalfAnswered:1.0".into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: addr.ip().to_string(),
            port: addr.port(),
            object_key: b"key".to_vec(),
            components: Vec::new(),
        }],
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// What each caller is owed
// ─────────────────────────────────────────────────────────────────────────────

/// The caller whose reply had begun: it hears that, it hears which call, and it
/// is **not** told the call is safe to re-send.
fn check_the_call_that_was_cut(f: &Failed, script: &Script, id: u32, what: &str) {
    match f.error {
        Error::InterruptedMidReassembly { control, partial, request_id, received } => {
            assert_eq!(control, script.control, "{what}: the wrong control message");
            assert_eq!(partial, MsgType::Reply, "{what}: a reply was what was cut");
            assert_eq!(request_id, id, "{what}: the caller must be able to name its own call");
            assert_eq!(
                received,
                1 + script.continuations,
                "{what}: the leading message plus every continuation that arrived"
            );
        }
        ref other => {
            panic!("{what}: the cut call must hear that its reply had started, got {other}")
        }
    }
    assert!(!f.unsent, "{what}: the peer had begun answering this one; re-sending would repeat it");
    assert_eq!(
        f.error.is_orderly_close(),
        script.control == MsgType::CloseConnection,
        "{what}: a goodbye is retryable and a report is not, and they must not swap"
    );
}

/// The other caller on the same connection: it got nothing back, so §13.5.1
/// still covers it — but only when the control message was a goodbye. §9.4.8's
/// `MessageError` names nothing and therefore promises nothing.
fn check_the_other_call(f: &Failed, script: &Script, what: &str) {
    match script.control {
        MsgType::CloseConnection => {
            assert!(
                matches!(f.error, Error::ConnectionClosed),
                "{what}: somebody else's cut reply is not this caller's business, got {}",
                f.error
            );
            assert!(f.unsent, "{what}: §13.5.1 covers a request that got nothing back");
            assert!(f.error.is_orderly_close(), "{what}: this caller met a teardown");
        }
        _ => {
            assert!(
                matches!(f.error, Error::UnexpectedMessage(MsgType::MessageError)),
                "{what}: got {}",
                f.error
            );
            assert!(!f.unsent, "{what}: a MessageError names nothing, so it frees nobody");
        }
    }
}

/// One connection, one script, both callers checked.
fn run(script: Script) {
    let what = format!(
        "{:?} reply, cut #{}, {} continuation(s), {}ms window, {:?}, #{} collects first",
        script.reply_endian,
        script.cut,
        script.continuations,
        script.window.as_millis(),
        script.control,
        script.collects_first,
    );
    let (addr, cut_id) = peer(script);
    let mux = Mux::connect(&ior_at(addr), T).expect("connect");
    assert!(mux.multiplexes(), "{what}: a 1.2 cleartext connection must multiplex");

    // Sent under the write half one after the other, so the order the peer
    // reads them in is the order they went out in.
    let a = mux.send(b"key", "first", |_| {}).expect("the first request goes out");
    let b = mux.send(b"key", "second", |_| {}).expect("the second request goes out");
    let ids = [a.request_id(), b.request_id()];
    assert!(ids[1] > ids[0], "{what}: ids are allocated in wire order");

    let mut pending = [Some(a), Some(b)];
    let mut failed: [Option<Failed>; 2] = [None, None];
    let order = [script.collects_first, 1 - script.collects_first];
    for i in order {
        let p = pending[i].take().expect("each caller collects once");
        failed[i] = Some(p.wait(T).err().unwrap_or_else(|| {
            panic!("{what}: caller #{i} was answered, and half a reply is not an answer")
        }));
    }

    let cut = cut_id.recv_timeout(T).expect("the peer names the call it cut");
    assert_eq!(cut, ids[script.cut], "{what}: the peer cut the request the script named");

    let other = 1 - script.cut;
    check_the_call_that_was_cut(failed[script.cut].as_ref().expect("cut"), &script, cut, &what);
    check_the_other_call(failed[other].as_ref().expect("other"), &script, &what);

    assert!(!mux.is_usable(), "{what}: the message can never complete now");
    drop(mux); // lets the peer's held-open read return
}

/// Every axis at both of its values, crossed with byte order.
fn matrix(control: MsgType) -> Vec<Script> {
    let mut out = Vec::new();
    for (n, reply_endian) in [Endian::Big, Endian::Little].into_iter().enumerate() {
        for cut in 0..2 {
            for continuations in 0..2 {
                for collects_first in 0..2 {
                    // The window alternates across the matrix rather than
                    // doubling it: what matters is that both a zero window and
                    // a real one appear against every other axis.
                    let slow = (n + cut + continuations + collects_first) % 2 == 0;
                    out.push(Script {
                        reply_endian,
                        cut,
                        continuations,
                        window: Duration::from_millis(if slow { 80 } else { 0 }),
                        control,
                        collects_first,
                    });
                }
            }
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// The measurements
// ─────────────────────────────────────────────────────────────────────────────

/// The claim, in both halves, over sixteen peers.
///
/// The call whose reply had begun is told it was interrupted and told **not**
/// to re-send; the other caller multiplexed on the same connection is told the
/// connection closed and told it may. The two answers come out of one event,
/// which is the whole reason [`orbweaver_giop::mux::Failed::unsent`] takes a
/// request id instead of describing the connection.
#[test]
fn a_close_between_two_writes_of_one_reply_answers_each_caller_about_its_own_call() {
    for script in matrix(MsgType::CloseConnection) {
        run(script);
    }
}

/// The negative arm, and the reason the first test is not just "the other
/// caller is always free to re-send". §9.4.8's `MessageError` is a report about
/// something *we* sent; it names no request, so it makes none of them
/// re-sendable — including the untouched one that a `CloseConnection` would
/// have freed.
#[test]
fn a_message_error_between_two_writes_frees_neither_caller() {
    for script in matrix(MsgType::MessageError) {
        run(script);
    }
}
