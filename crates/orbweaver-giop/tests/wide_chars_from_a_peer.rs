//! `wchar` and `wstring` as a conformant peer actually writes and reads them.
//!
//! Every byte sequence below came out of omniORB 4.3.4 on a little-endian host
//! — clause (b) of the licensing boundary, the output of a program we ran over
//! IDL types the OMG defines. Nothing of omniORB's is linked, vendored or
//! redistributed; what is recorded is what CORBA 3.4 Part 2 §9.3.1.6 requires,
//! with a second implementation as the witness that we read it right.
//!
//! The one fact they all establish: **a UTF-16 wide value carries its own byte
//! order and the enclosing stream has no say in it.** The peer's writer emits
//! the identical octets whichever order the stream is in, and the peer's reader
//! returns the identical character whichever order the stream is in. Our
//! reader took the units from the stream, which is a rule our own round trip
//! could never fail on, because our writer always emits a byte-order mark and
//! the mark makes the two conventions agree.

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_giop::Version;
use orbweaver_giop::codeset::{CodeSetId, WideCodec};

fn codec() -> WideCodec {
    WideCodec::new(Version::V1_2, CodeSetId::UTF_16).expect("1.2 + UTF-16 is always valid")
}

// ── what the peer writes ────────────────────────────────────────────────────

/// `cdrMarshal(CORBA._tc_WCharSeq, ['w', 'A', '한'], endian)` — a
/// `sequence<wchar>` of U+0077, U+0041, U+D55C.
///
/// The four-octet element count is the only part that follows the stream. Each
/// element is an octet count of 2 and then the code unit **big-endian, with no
/// mark**, in both orders. The elements also abut one another with no padding,
/// which is Table 9.1's `wchar` alignment of 1 for GIOP 1.2 and later.
const WCHAR_SEQ_BIG: &[u8] =
    &[0x00, 0x00, 0x00, 0x03, 0x02, 0x00, 0x77, 0x02, 0x00, 0x41, 0x02, 0xd5, 0x5c];
const WCHAR_SEQ_LITTLE: &[u8] =
    &[0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x77, 0x02, 0x00, 0x41, 0x02, 0xd5, 0x5c];

/// `cdrMarshal(CORBA._tc_wstring, "wA", endian)`.
///
/// An octet count that follows the stream, then `FF FE` and little-endian
/// units — again in **both** orders. omniORB always marks, and always marks
/// little-endian, whatever the message is doing around it.
const WSTRING_BIG: &[u8] = &[0x00, 0x00, 0x00, 0x06, 0xff, 0xfe, 0x77, 0x00, 0x41, 0x00];
const WSTRING_LITTLE: &[u8] = &[0x06, 0x00, 0x00, 0x00, 0xff, 0xfe, 0x77, 0x00, 0x41, 0x00];

/// `cdrMarshal(CORBA._tc_wstring, "", endian)`: a zero octet count and nothing
/// else, in both orders. There is no order to mark when there are no units.
const WSTRING_EMPTY: &[u8] = &[0x00, 0x00, 0x00, 0x00];

#[test]
fn a_wchar_from_a_peer_reads_the_same_in_either_stream_order() {
    for (endian, bytes) in [(Endian::Big, WCHAR_SEQ_BIG), (Endian::Little, WCHAR_SEQ_LITTLE)] {
        let mut d = Decoder::new(bytes, endian);
        assert_eq!(d.get_u32().expect("element count"), 3);
        let got: String = (0..3).map(|_| codec().get_wchar(&mut d).expect("wchar")).collect();
        assert_eq!(got, "wA한", "{endian:?}");
        assert!(d.is_empty(), "{endian:?}: elements are unaligned and abut");
    }
}

/// The direction our own round trip can never check: the peer's exact octets
/// back out again. No padding is skipped here because there is none — a
/// GIOP 1.2 `wchar` aligns to 1.
#[test]
fn re_encoding_a_wchar_reproduces_what_the_peer_wrote() {
    for (endian, bytes) in [(Endian::Big, WCHAR_SEQ_BIG), (Endian::Little, WCHAR_SEQ_LITTLE)] {
        let mut e = Encoder::new(endian);
        e.put_u32(3);
        for c in "wA한".chars() {
            codec().put_wchar(&mut e, c).expect("wchar");
        }
        assert_eq!(e.finish().expect("finish"), bytes, "{endian:?}");
    }
}

#[test]
fn a_wstring_from_a_peer_reads_the_same_in_either_stream_order() {
    for (endian, bytes) in [(Endian::Big, WSTRING_BIG), (Endian::Little, WSTRING_LITTLE)] {
        let mut d = Decoder::new(bytes, endian);
        assert_eq!(codec().get_wstring(&mut d).expect("wstring"), "wA", "{endian:?}");
    }
    for endian in [Endian::Big, Endian::Little] {
        let mut d = Decoder::new(WSTRING_EMPTY, endian);
        assert_eq!(codec().get_wstring(&mut d).expect("empty wstring"), "", "{endian:?}");
    }
}

/// **The defect, and the only oracle that could have found it.**
///
/// omniORB writes a byte-order mark on every non-empty `wstring`, so its
/// *writer* can never produce the case that was wrong. Its *reader* can, and
/// was asked directly:
///
/// ```text
/// cdrUnmarshal(CORBA._tc_wstring, <len> + body, endian)
///
///   body            endian=big   endian=little
///   00 77           U+0077       U+0077
///   77 00           U+7700       U+7700
///   fe ff 00 77     U+0077       U+0077
///   ff fe 77 00     U+0077       U+0077
///   fe ff 77 00     U+7700       U+7700
///   ff fe 00 77     U+7700       U+7700
/// ```
///
/// Twelve readings, six answers, and the stream's byte order changes none of
/// them. That is §9.3.1.6's three bullets exactly: `FE FF` is big-endian,
/// `FF FE` is little-endian, **neither is big-endian**.
///
/// Reading the units in the stream's order agreed with the peer on five of
/// those six bodies and got the sixth — a bare `00 77` in a little-endian
/// message — exactly backwards, returning U+7700 where a conformant peer
/// returns U+0077. Our writer always marks, so no round trip here ever
/// produced the sixth body to be wrong about.
#[test]
fn a_wstring_takes_its_order_from_its_own_mark_and_not_from_the_stream() {
    // (body, what omniORB 4.3.4 returned for it, in either stream order)
    let peer: &[(&[u8], &str)] = &[
        (&[0x00, 0x77], "\u{0077}"),
        (&[0x77, 0x00], "\u{7700}"),
        (&[0xfe, 0xff, 0x00, 0x77], "\u{0077}"),
        (&[0xff, 0xfe, 0x77, 0x00], "\u{0077}"),
        (&[0xfe, 0xff, 0x77, 0x00], "\u{7700}"),
        (&[0xff, 0xfe, 0x00, 0x77], "\u{7700}"),
    ];
    for (body, expected) in peer {
        for endian in [Endian::Big, Endian::Little] {
            let mut wire = Encoder::new(endian);
            wire.put_u32(body.len() as u32);
            wire.put_bytes(body);
            let raw = wire.finish().expect("finish");

            let mut d = Decoder::new(&raw, endian);
            let got = codec().get_wstring(&mut d).expect("wstring");
            assert_eq!(&got, expected, "{endian:?} stream, body {body:02x?}");
        }
    }
}

/// §9.3.1.6: "if a BOM is present at the beginning of a wchar or wstring
/// received in a GIOP message, the ORB **shall** remove the BOM before passing
/// the value to the user". A `wchar` is a count and then that many octets, so a
/// marked one has a count of four — which used to be refused outright as
/// malformed, because we never emit one and so never met one.
#[test]
fn a_marked_wchar_is_legal_and_the_mark_is_removed() {
    let cases: &[(&[u8], char)] = &[
        (&[0x02, 0x00, 0x77], 'w'),
        (&[0x04, 0xfe, 0xff, 0x00, 0x77], 'w'),
        (&[0x04, 0xff, 0xfe, 0x77, 0x00], 'w'),
        (&[0x04, 0xfe, 0xff, 0x77, 0x00], '\u{7700}'),
    ];
    for (raw, expected) in cases {
        for endian in [Endian::Big, Endian::Little] {
            let mut d = Decoder::new(raw, endian);
            assert_eq!(
                codec().get_wchar(&mut d).expect("wchar"),
                *expected,
                "{endian:?} {raw:02x?}"
            );
        }
    }

    // Still refused: a count that is not a whole number of units, a count of
    // zero, a pair of units, and an odd count that `chunks_exact` would have
    // quietly truncated to one unit while consuming three octets.
    for raw in [
        &[0x01u8, 0x00][..],
        &[0x00][..],
        &[0x04, 0x00, 0x77, 0x00, 0x41][..],
        &[0x03, 0x00, 0x77, 0xAA][..],
        &[0x05, 0xfe, 0xff, 0x00, 0x77, 0xAA][..],
    ] {
        let mut d = Decoder::new(raw, Endian::Big);
        assert!(codec().get_wchar(&mut d).is_err(), "{raw:02x?} should not decode");
    }
}
