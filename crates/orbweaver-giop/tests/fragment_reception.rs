//! Fragment *reception*, measured against shapes our own emitter never makes.
//!
//! The harness's fragmentation group is honest about a gap it cannot close:
//! neither available peer emits GIOP fragments — omniORB's `giopMaxMsgSize` is
//! a hard cap that raises `MARSHAL` rather than a split threshold, and JacORB
//! 3.9 has no fragmentation property at all — so the independent evidence runs
//! one way only. We fragment, they reassemble. Our *reader* has only ever been
//! fed our own writer's output.
//!
//! **Amended 2026-08-14, and the amendment matters more than the file.** That
//! paragraph is wrong about omniORB. `spike-mux` asked omniORB 4.3.4 for a
//! 1 MB `sequence<octet>` and it answered in **two** pieces — a leading `Reply`
//! with the more-fragments bit and one `Fragment` — reproducibly, at GIOP 1.2
//! *and* at 1.1, with no configuration. So a peer does fragment; it simply had
//! never been asked for a reply big enough. JacORB 3.9 still does not, at any
//! version, for the same request.
//!
//! Two things follow. The 1.2 reassembler has now been fed a real peer's
//! fragments and not only our own emitter's, which is the independent evidence
//! this file was written to stand in for. And at 1.1 the same reply is
//! *refused* — `Error::FragmentUnsupported`, because a 1.1 `Fragment` carries
//! no request id — which is a live interop limit against a stock omniORB
//! rather than a theoretical one. The hand-built streams below are still the
//! oracle for every shape a peer *may* send; what changed is that "no peer
//! fragments" was an assumption nobody had tested with a large enough
//! argument.
//!
//! That is a weak test of a receiver, because one emitter produces one shape.
//! A conformant peer may legally split at any 8-aligned boundary, including
//! ones that land in the middle of a request header, and may send fragments
//! our emitter would never generate. This file builds those streams by hand
//! from §9.4.9 and asserts what the reader does with each — which makes the
//! specification the oracle where no peer can be.
//!
//! **Second amendment, same day.** Every case here assumed a peer that is
//! either correct or broken. The third case is the ordinary one: a peer that
//! does not vanish but *speaks* — a `CloseConnection` or a `MessageError`
//! between two fragments. The last section is about that, and it is the only
//! place in this file where the specification does not settle the answer by
//! itself: §9.4.9 forbids the interruption and §13.5.1 permits the message, so
//! what the reader owes the caller is not a verdict on the peer but a report
//! precise enough to choose between re-dialing and giving up.
//!
//! 수신 측은 지금까지 **우리 송신기의 출력만** 먹어봤다. 송신기 하나는 형태
//! 하나를 만든다. 그래서 여기서는 규격이 허용하는 다른 형태들을 손으로 만든다.
//! 마지막 절은 "사라지는 피어"가 아니라 "말을 거는 피어" — 조각 사이에 도착한
//! `CloseConnection`/`MessageError` — 를 다룬다.

use std::io::Cursor;

use orbweaver_cdr::Endian;
use orbweaver_giop::{Error, HEADER_LEN, MAGIC, MsgType, Version, read_message};

/// A GIOP 1.2 Request whose body is `body`, built by hand so the test controls
/// every byte the reader will see.
fn request(request_id: u32, body: &[u8], endian: Endian) -> Vec<u8> {
    let mut m = Vec::new();
    m.extend_from_slice(MAGIC);
    m.push(1);
    m.push(2);
    m.push(endian.as_flag());
    m.push(MsgType::Request as u8);
    m.extend_from_slice(&[0, 0, 0, 0]); // size, patched below
    m.extend_from_slice(&u32_bytes(request_id, endian));
    m.extend_from_slice(body);
    patch_size(&mut m, endian);
    m
}

/// One `Fragment` message carrying `payload` for `request_id`.
fn fragment(request_id: u32, payload: &[u8], more: bool, endian: Endian) -> Vec<u8> {
    fragment_versioned(request_id, payload, more, endian, Version { major: 1, minor: 2 })
}

fn fragment_versioned(
    request_id: u32,
    payload: &[u8],
    more: bool,
    endian: Endian,
    version: Version,
) -> Vec<u8> {
    let mut m = Vec::new();
    m.extend_from_slice(MAGIC);
    m.push(version.major);
    m.push(version.minor);
    m.push(endian.as_flag() | if more { 0b10 } else { 0 });
    m.push(MsgType::Fragment as u8);
    m.extend_from_slice(&[0, 0, 0, 0]);
    m.extend_from_slice(&u32_bytes(request_id, endian));
    m.extend_from_slice(payload);
    patch_size(&mut m, endian);
    m
}

fn u32_bytes(v: u32, endian: Endian) -> [u8; 4] {
    match endian {
        Endian::Big => v.to_be_bytes(),
        Endian::Little => v.to_le_bytes(),
    }
}

fn patch_size(m: &mut [u8], endian: Endian) {
    let size = (m.len() - HEADER_LEN) as u32;
    m[8..12].copy_from_slice(&u32_bytes(size, endian));
}

/// Sets the more-fragments bit on an already-built message.
fn with_more(mut m: Vec<u8>) -> Vec<u8> {
    m[6] |= 0b10;
    m
}

fn read(stream: &[u8]) -> Result<orbweaver_giop::RawMessage, Error> {
    read_message(&mut Cursor::new(stream), 1024 * 1024)
}

/// A bodiless control message — `CloseConnection` or `MessageError`. §9.4.7 and
/// §9.4.8 both give them a header and nothing else.
fn control(msg_type: MsgType, endian: Endian, version: Version) -> Vec<u8> {
    let mut m = Vec::new();
    m.extend_from_slice(MAGIC);
    m.push(version.major);
    m.push(version.minor);
    m.push(endian.as_flag());
    m.push(msg_type as u8);
    m.extend_from_slice(&[0, 0, 0, 0]); // message_size = 0
    m
}

const V1_2: Version = Version { major: 1, minor: 2 };

/// The shape our emitter never produces: a split so early that the leading
/// message holds nothing but the request id, with the whole body arriving in
/// continuations. Legal — §9.4.9 constrains the split to `message_size + 12`
/// divisible by 8, not to any minimum useful content.
#[test]
fn a_peer_may_split_immediately_after_the_request_id() {
    for endian in [Endian::Big, Endian::Little] {
        let body: Vec<u8> = (0..64u8).collect();
        let head = with_more(request(42, &[], endian)); // 12 + 4 = 16, 8-aligned
        let mut stream = head;
        stream.extend(fragment(42, &body[..32], true, endian));
        stream.extend(fragment(42, &body[32..], false, endian));

        let msg = read(&stream).expect("reassembles");
        assert_eq!(msg.msg_type, MsgType::Request);
        assert_eq!(msg.fragments, 3);
        assert!(!msg.more_fragments);
        assert_eq!(&msg.bytes[HEADER_LEN + 4..], &body[..], "{endian:?}");
        // The rewritten header must describe the whole thing, or a decoder
        // that trusts message_size reads a truncated body.
        let size = u32::from_be_bytes([msg.bytes[8], msg.bytes[9], msg.bytes[10], msg.bytes[11]]);
        let size = match endian {
            Endian::Big => size,
            Endian::Little => size.swap_bytes(),
        };
        assert_eq!(size as usize, msg.bytes.len() - HEADER_LEN);
    }
}

/// Many small fragments, which is what a peer with a small transmit buffer
/// produces and what our emitter, driven by a threshold, does not.
#[test]
fn many_small_fragments_reassemble_in_order() {
    let body: Vec<u8> = (0..200u8).collect();
    let mut stream = with_more(request(7, &[], Endian::Big));
    for (i, chunk) in body.chunks(8).enumerate() {
        let last = (i + 1) * 8 >= body.len();
        stream.extend(fragment(7, chunk, !last, Endian::Big));
    }
    let msg = read(&stream).expect("reassembles");
    assert_eq!(&msg.bytes[HEADER_LEN + 4..], &body[..]);
    assert_eq!(msg.fragments, 26);
}

/// A fragment with an empty payload is legal and must not stall the reader or
/// corrupt the stream — it carries the more-fragments bit and nothing else.
#[test]
fn an_empty_fragment_is_carried_without_effect() {
    let mut stream = with_more(request(3, &[], Endian::Big));
    stream.extend(fragment(3, &[], true, Endian::Big));
    stream.extend(fragment(3, b"12345678", false, Endian::Big));
    let msg = read(&stream).expect("reassembles");
    assert_eq!(&msg.bytes[HEADER_LEN + 4..], b"12345678");
}

/// A fragment belonging to another request is a desynchronised connection, not
/// a body to concatenate. Silently appending it would splice one caller's
/// arguments into another caller's call — the worst outcome available here.
#[test]
fn a_fragment_for_another_request_is_refused() {
    let mut stream = with_more(request(1, &[], Endian::Big));
    stream.extend(fragment(999, b"stolen__", false, Endian::Big));
    assert!(matches!(read(&stream), Err(Error::Desynchronized)));
}

/// A non-fragment arriving mid-reassembly is the same class: GIOP 1.2 does not
/// allow another message to interrupt a fragmented one on the same connection.
#[test]
fn an_interleaved_message_is_refused() {
    let mut stream = with_more(request(1, &[], Endian::Big));
    stream.extend(request(2, b"whole___", Endian::Big));
    assert!(matches!(read(&stream), Err(Error::UnexpectedMessage(MsgType::Request))));
}

/// GIOP 1.1 fragments carry no request id and restart alignment, so
/// concatenation is not reassembly. Refusing is the documented behaviour and
/// the honest one; this pins it so a future change has to argue with a test.
#[test]
fn a_1_1_fragmented_message_is_refused_rather_than_guessed() {
    let mut head = request(1, b"12345678", Endian::Big);
    head[5] = 1; // 1.1
    patch_size(&mut head, Endian::Big);
    let stream = with_more(head);
    assert!(matches!(read(&stream), Err(Error::FragmentUnsupported)));
}

/// A peer that changes version mid-message is broken. The reader must not
/// treat a 1.1 fragment as a 1.2 one: in 1.1 the four bytes we would read as a
/// request id are body, so a match would be a coincidence and a mismatch would
/// be reported as the wrong error.
#[test]
fn a_fragment_at_another_version_does_not_pass_as_a_continuation() {
    let mut stream = with_more(request(1, &[], Endian::Big));
    stream.extend(fragment_versioned(
        1,
        b"payload_",
        false,
        Endian::Big,
        Version { major: 1, minor: 1 },
    ));
    let got = read(&stream);
    assert!(
        got.is_err(),
        "a version change mid-message must not be accepted; got {:?}",
        got.map(|m| m.fragments)
    );
}

/// The reader must not follow an unbounded fragment chain: the sender chooses
/// how many pieces to send, so the bound is ours to impose. Measured with a
/// stream that never ends rather than argued from the constant.
#[test]
fn an_endless_fragment_chain_is_bounded() {
    let mut stream = with_more(request(5, &[], Endian::Big));
    for _ in 0..orbweaver_giop::MAX_FRAGMENTS + 8 {
        stream.extend(fragment(5, b"12345678", true, Endian::Big));
    }
    assert!(matches!(read(&stream), Err(Error::MessageTooLarge { .. })));
}

/// A reassembled message must also respect the size limit, which is about the
/// total and not about any one piece: 4096 fragments of 1 KB is 4 MB, and each
/// piece passes a per-message check that the whole does not.
#[test]
fn the_size_limit_applies_to_the_reassembled_total() {
    let mut stream = with_more(request(6, &[], Endian::Big));
    for _ in 0..64 {
        stream.extend(fragment(6, &[0u8; 1024], true, Endian::Big));
    }
    stream.extend(fragment(6, b"12345678", false, Endian::Big));
    let got = read_message(&mut Cursor::new(&stream[..]), 8 * 1024);
    assert!(
        matches!(got, Err(Error::MessageTooLarge { .. })),
        "64 KB reassembled under an 8 KB limit must be refused; got {:?}",
        got.map(|m| m.bytes.len())
    );
}

/// A stray `Fragment` with nothing to continue. Our own emitter cannot produce
/// one, and a peer that does is either desynchronised or probing.
#[test]
fn a_leading_fragment_is_not_a_message() {
    let stream = fragment(1, b"12345678", false, Endian::Big);
    let got = read(&stream);
    assert!(
        matches!(got, Err(Error::UnexpectedMessage(MsgType::Fragment))),
        "a Fragment with no message to continue must be refused; got {got:?}"
    );
}

/// Truncation mid-chain is what a peer that dies produces, and it must be an
/// error rather than a short message that decodes into something plausible.
#[test]
fn a_chain_cut_short_is_an_error_not_a_short_message() {
    let mut stream = with_more(request(9, &[], Endian::Big));
    stream.extend(fragment(9, b"12345678", true, Endian::Big));
    // …and then the peer vanishes, with the more-fragments bit still set.
    assert!(read(&stream).is_err());
}

// ─────────────────────────────────────────────────────────────────────────────
// The peer that does not vanish but speaks
//
// Everything above is about a peer that is either correct or broken. These are
// about the third case, which is the common one in production and the one the
// two rules disagree about: §9.4.9 says nothing may interrupt a fragmented
// message, and §13.5.1 says a server may send `CloseConnection` whenever it
// likes — including, therefore, halfway through a reply. The reader cannot
// obey both, so what it must do instead is report the collision precisely
// enough that the caller can decide: an orderly close is a reason to dial
// again, corruption is a reason to stop, and the two must not arrive wearing
// the same error.
//
// 규격 두 조항이 충돌한다. 리더는 어느 한쪽을 고르는 대신, 호출자가 "다시 걸기"와
// "포기"를 구분할 수 있도록 정확히 보고한다.
// ─────────────────────────────────────────────────────────────────────────────

/// The case the file was written without: the peer speaks instead of vanishing.
///
/// It must not read as [`Error::Desynchronized`] — the stream was never
/// desynchronized, every byte of that `CloseConnection` was well framed — and
/// it must not read as a plain [`Error::ConnectionClosed`] either, because
/// §13.5.1's "was not processed" is *false* about a request whose reply had
/// already started. So it is its own answer, and it carries the id of the one
/// call the promise does not cover.
#[test]
fn a_close_between_fragments_is_a_teardown_that_names_the_call_it_cut() {
    for endian in [Endian::Big, Endian::Little] {
        let mut stream = with_more(request(42, &[], endian));
        stream.extend(fragment(42, b"12345678", true, endian));
        stream.extend(control(MsgType::CloseConnection, endian, V1_2));

        let err = read(&stream).expect_err("a goodbye is not a continuation");
        assert!(err.is_orderly_close(), "{endian:?}: the peer said goodbye, got {err}");
        match err {
            Error::InterruptedMidReassembly { control, partial, request_id, received } => {
                assert_eq!(control, MsgType::CloseConnection, "{endian:?}");
                assert_eq!(partial, MsgType::Request, "{endian:?}");
                assert_eq!(request_id, 42, "{endian:?}: the caller must be able to name its call");
                assert_eq!(received, 2, "{endian:?}: the leading message and one fragment");
            }
            other => panic!("{endian:?}: expected an interrupted reassembly, got {other}"),
        }
    }
}

/// The distinction has to be readable from the value. A caller that has to
/// match on the text of a message to tell "the peer closed" from "the stream is
/// corrupt" is one rewording away from retrying corruption or abandoning a
/// service that is merely restarting.
#[test]
fn teardown_and_corruption_are_told_apart_without_reading_the_message() {
    let mut closed = with_more(request(1, &[], Endian::Big));
    closed.extend(control(MsgType::CloseConnection, Endian::Big, V1_2));

    let mut stolen = with_more(request(1, &[], Endian::Big));
    stolen.extend(fragment(999, b"stolen__", false, Endian::Big));

    let mut interleaved = with_more(request(1, &[], Endian::Big));
    interleaved.extend(request(2, b"whole___", Endian::Big));

    let mut reported = with_more(request(1, &[], Endian::Big));
    reported.extend(control(MsgType::MessageError, Endian::Big, V1_2));

    assert!(read(&closed).expect_err("closed").is_orderly_close());
    for (what, stream) in
        [("a fragment for another request", stolen), ("an interleaved message", interleaved)]
    {
        let err = read(&stream).expect_err(what);
        assert!(!err.is_orderly_close(), "{what} is corruption, not a goodbye: {err}");
    }
    // And a `MessageError` is neither: an orderly message that is nonetheless
    // not a close, so it must not be retried as one.
    let err = read(&reported).expect_err("a message error");
    assert!(!err.is_orderly_close(), "a report is not a goodbye: {err}");
}

/// §9.4.8's `MessageError` is a report about something **we** sent, not damage
/// to what the peer is sending: the message is well framed and the conversation
/// is simply over. What it must never do is claim a request is safe to re-send
/// — it names nothing, so there is no request it could be talking about.
#[test]
fn a_message_error_between_fragments_is_a_report_not_corruption() {
    let mut stream = with_more(request(8, &[], Endian::Big));
    stream.extend(control(MsgType::MessageError, Endian::Big, V1_2));

    match read(&stream).expect_err("a report is not a continuation") {
        Error::InterruptedMidReassembly { control, partial, request_id, received } => {
            assert_eq!(control, MsgType::MessageError);
            assert_eq!(partial, MsgType::Request);
            assert_eq!(request_id, 8);
            assert_eq!(received, 1, "only the leading message had arrived");
        }
        other => panic!("expected an interrupted reassembly, got {other}"),
    }
}

/// At GIOP 1.1 the reader refuses fragmentation outright, and that refusal must
/// win over the close that follows it — not by accident of ordering but because
/// stopping early is the point: nothing after the leading message is consumed,
/// so the reason reported is the one thing we genuinely cannot do.
///
/// Preferring the close would be an availability bug in the opposite direction
/// from the one this section fixes: `FragmentUnsupported` is permanent for this
/// peer at this version, a close is retryable, and reporting the retryable one
/// sends the pool round again to be told the same thing on a fresh connection.
#[test]
fn at_1_1_the_refusal_to_fragment_wins_and_leaves_the_close_unread() {
    let v1_1 = Version { major: 1, minor: 1 };
    let mut head = request(1, b"12345678", Endian::Big);
    head[5] = 1; // 1.1
    patch_size(&mut head, Endian::Big);
    let head = with_more(head);
    let head_len = head.len();

    let mut stream = head;
    stream.extend(control(MsgType::CloseConnection, Endian::Big, v1_1));

    let mut cursor = Cursor::new(&stream[..]);
    let err = read_message(&mut cursor, 1024 * 1024).expect_err("1.1 cannot reassemble");
    assert!(matches!(err, Error::FragmentUnsupported), "got {err}");
    assert!(!err.is_orderly_close(), "this is not retryable and must not look it");
    assert_eq!(
        cursor.position() as usize,
        head_len,
        "the refusal must consume nothing beyond the message it refuses"
    );
}

/// The other side of the same line: a `CloseConnection` that interrupts nothing
/// is an ordinary message and must stay one. The new answer above exists for a
/// half-received message; a reader that started reporting every close as an
/// error would break the orderly shutdown every ORB performs.
#[test]
fn a_close_that_interrupts_nothing_is_still_an_ordinary_message() {
    for msg_type in [MsgType::CloseConnection, MsgType::MessageError] {
        let stream = control(msg_type, Endian::Big, V1_2);
        let msg = read(&stream).expect("a control message between messages is a message");
        assert_eq!(msg.msg_type, msg_type);
        assert_eq!(msg.fragments, 1);
        assert!(!msg.more_fragments);
    }
}
