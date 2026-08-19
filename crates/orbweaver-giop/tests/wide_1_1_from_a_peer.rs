//! A GIOP 1.1 `wstring` as the one 1.1 wide-text peer on this host actually
//! writes and reads it.
//!
//! Every byte sequence below came off the wire between JacORB 3.9 and our
//! `spike-server` / `spike-interop` on 2026-08-19, recorded by
//! `spikes/jacorb_giop11_tap.py` and printed by `spikes/jacorb_giop11.sh` —
//! clause (b) of the licensing boundary, the output of a program we ran over
//! IDL types the OMG defines. Nothing of JacORB's is linked, vendored or
//! redistributed. omniORB declines 1.1 wide text outright (`BAD_PARAM` minor
//! 23, spike-interop case 9) and omniORBpy cannot unmarshal its own 1.1
//! `wchar` (D010 B5), so JacORB is the only witness there is.
//!
//! The negotiated codesets were char=UTF-8, wchar=UTF-16, and the two facts
//! the bytes establish are:
//!
//! 1. **A 1.1 `wstring` carries no byte-order mark, and a mark is not read as
//!    one.** JacORB writes `count=13` for a twelve-unit text — the units and
//!    the terminator, nothing else. Given our marked `count=14`, its user
//!    received `U+FEFF` + text, and its echo came back as fourteen units with
//!    the mark as the first *character* — which our reader then stripped as
//!    a mark, so the round trip in `spike-interop` was green in 4/4 exchanges
//!    while the value JacORB's user saw was wrong. That is the reader-strips-
//!    what-the-writer-emits shape again: a convention both ends apply cannot
//!    be refuted by a round trip. At 1.2 the same peer strips the mark.
//!
//! 2. **An unmarked 1.1 `wstring` is read in the message's order.** Our first
//!    unmarked writer followed §9.3.1.6's third bullet ("neither, it's
//!    big-endian") and put big-endian units into a little-endian message;
//!    JacORB echoed every unit swapped — `00 77` came back as U+7700. In a
//!    big-endian message the same octets came back unchanged.
//!
//! §9.3.1.6 (CORBA 3.4 Part 2) makes the mark the writer's option ("if an ORB
//! decides to use BOM …") and does not name a GIOP version for its bullets,
//! which are phrased "after the length indication" — a 1.2 shape; a 1.1
//! `wchar` has no length indication. Whether the paragraph binds a 1.1
//! `wstring` at all is therefore ambiguous, and the decision recorded in
//! `codeset.rs` (`unmarked_order`, `WideCodec::put_wstring`) is to follow
//! the peer that can be measured. §9.3.2.7's 1.1 sentence — "an unsigned long
//! indicating the length of the string in octets or unsigned integers
//! (determined by the transfer syntax for wchar) followed by the individual
//! wide characters … the string length includes the null character" — is
//! what both sides count by.

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_giop::Version;
use orbweaver_giop::codeset::{CodeSetId, WideCodec};

fn codec() -> WideCodec {
    WideCodec::new(Version::V1_1, CodeSetId::UTF_16).expect("1.1 + UTF-16 is always valid")
}

/// The text `spikes/jacorb_giop11.sh` sends as `TEXT_BMP`: twelve BMP units.
const TEXT_BMP: &str = "wide 함정 전투체계";

/// The text it sends as `TEXT_ASTRAL`: ten characters, eleven UTF-16 units,
/// because U+1F600 is a surrogate pair and therefore two 1.1 "wide characters".
const TEXT_ASTRAL: &str = "pair 😀 end";

/// `Client11.java` → our `spike-server`, `echo_wstring(TEXT_BMP)`, GIOP 1.1,
/// big-endian message. From `f.tap.log`:
///
/// ```text
/// request body: wstring 1.1 count=13 (wide chars incl. terminator)
///   body=00770069006400650020d568c8150020c804d22cccb4acc40000
/// ```
///
/// Twelve units, a terminator, no mark; the count is thirteen wide characters.
const JACORB_BMP: &[u8] = &[
    0x00, 0x00, 0x00, 0x0d, // 13
    0x00, 0x77, 0x00, 0x69, 0x00, 0x64, 0x00, 0x65, 0x00, 0x20, // "wide "
    0xd5, 0x68, 0xc8, 0x15, 0x00, 0x20, // "함정 "
    0xc8, 0x04, 0xd2, 0x2c, 0xcc, 0xb4, 0xac, 0xc4, // "전투체계"
    0x00, 0x00, // terminator
];

/// The same call with `TEXT_ASTRAL`, from the same log:
///
/// ```text
/// request body: wstring 1.1 count=12 (wide chars incl. terminator)
///   body=00700061006900720020d83dde0000200065006e00640000
/// ```
///
/// Eleven units — the surrogate pair `d83d de00` is two of them — plus a
/// terminator; twelve.
const JACORB_ASTRAL: &[u8] = &[
    0x00, 0x00, 0x00, 0x0c, // 12
    0x00, 0x70, 0x00, 0x61, 0x00, 0x69, 0x00, 0x72, 0x00, 0x20, // "pair "
    0xd8, 0x3d, 0xde, 0x00, // U+1F600
    0x00, 0x20, 0x00, 0x65, 0x00, 0x6e, 0x00, 0x64, // " end"
    0x00, 0x00, // terminator
];

/// What we wrote for `TEXT_BMP` before the fix, from the same log's reply
/// side — `count=14`, a mark, then the units and the terminator:
///
/// ```text
/// reply body: wstring 1.1 count=14 (wide chars incl. terminator)
///   body=feff00770069006400650020d568c8150020c804d22cccb4acc40000
/// ```
///
/// and JacORB's `Client11` reported the value its user received as
/// `U+FEFF U+0077 U+0069 …`. In the reverse direction (`r.tap.log`) JacORB
/// echoed these fourteen units back byte for byte, mark first.
const OURS_BEFORE_THE_FIX: &[u8] = &[
    0x00, 0x00, 0x00, 0x0e, // 14
    0xfe, 0xff, // the mark JacORB read as U+FEFF text
    0x00, 0x77, 0x00, 0x69, 0x00, 0x64, 0x00, 0x65, 0x00, 0x20, 0xd5, 0x68, 0xc8, 0x15, 0x00, 0x20,
    0xc8, 0x04, 0xd2, 0x2c, 0xcc, 0xb4, 0xac, 0xc4, 0x00, 0x00,
];

/// The BMP text re-encoded byte for byte to what JacORB wrote — count of
/// thirteen, no mark — and JacORB's own bytes decoded to the original text.
///
/// This is the negative control's positive half: before the fix this writer
/// produced `OURS_BEFORE_THE_FIX` here (`cargo test -p orbweaver-giop --test
/// wide_1_1_from_a_peer`, run red once, output in the commit message).
#[test]
fn a_1_1_wstring_is_written_as_the_peer_writes_it_and_reads_what_the_peer_wrote() {
    for (text, peer) in [(TEXT_BMP, JACORB_BMP), (TEXT_ASTRAL, JACORB_ASTRAL)] {
        let mut e = Encoder::new(Endian::Big);
        codec().put_wstring(&mut e, text).expect("wstring");
        let ours = e.finish().expect("finish");
        assert_eq!(ours, peer, "{text:?}: our 1.1 bytes must be JacORB's 1.1 bytes");
        assert_ne!(ours, OURS_BEFORE_THE_FIX, "the marked form is the recorded defect");

        let mut d = Decoder::new(peer, Endian::Big);
        assert_eq!(codec().get_wstring(&mut d).expect("decode"), text, "{text:?}");
        assert!(d.is_empty(), "{text:?}: nothing after the terminator");
    }
}

/// The count on the wire is what JacORB counted: units plus the terminator,
/// and never a fourteenth for a mark. Checked from the length field alone so
/// that a future writer that marks again cannot pass by re-encoding to bytes
/// that merely *contain* the peer's.
#[test]
fn the_1_1_count_is_the_units_plus_the_terminator_and_no_mark() {
    for (text, expected) in [(TEXT_BMP, 13u32), (TEXT_ASTRAL, 12u32)] {
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            codec().put_wstring(&mut e, text).expect("wstring");
            let raw = e.finish().expect("finish");
            let mut d = Decoder::new(&raw, endian);
            assert_eq!(d.get_u32().expect("count"), expected, "{text:?} {endian:?}");
            let first_two = &raw[4..6];
            assert!(
                first_two != [0xfe, 0xff] && first_two != [0xff, 0xfe],
                "{text:?} {endian:?}: a 1.1 wstring carries no mark; JacORB reads one as U+FEFF"
            );
        }
    }
}

/// Measured against the same peer, `spike-interop` little-endian client
/// (`r.tap.log`, exchange [3]): our big-endian units in a little-endian
/// message
///
/// ```text
/// [3] C->S GIOP 1.1 Request size=102 LE id=10 op=echo_wstring
///     request body: wstring 1.1 count=13 body=00770069006400650020d568c8150020c804d22cccb4acc40000
/// [3] S->C GIOP 1.1 Reply size=42 BE id=10 status=0 for=echo_wstring
///     reply body: wstring 1.1 count=13 body=7700690064006500200068d515c8200004c82cd2b4ccc4ac0000
/// ```
///
/// came back with every unit swapped, and after the writer followed the
/// message's order
///
/// ```text
/// [3] C->S GIOP 1.1 Request size=102 LE id=10 op=echo_wstring
///     request body: wstring 1.1 count=13 body=7700690064006500200068d515c8200004c82cd2b4ccc4ac0000
/// [3] S->C GIOP 1.1 Reply size=42 BE id=10 status=0 for=echo_wstring
///     reply body: wstring 1.1 count=13 body=00770069006400650020d568c8150020c804d22cccb4acc40000
/// ```
///
/// it came back as the text. So an unmarked 1.1 `wstring` follows the message
/// in both directions here: written in the stream's order, read in the
/// stream's order, and JacORB's big-endian reply decodes to the same string
/// whichever order our request went out in.
#[test]
fn an_unmarked_1_1_wstring_follows_the_message_in_both_orders() {
    // Written little-endian into a little-endian message: the octets JacORB
    // read correctly.
    const OURS_LITTLE: &[u8] = &[
        0x0d, 0x00, 0x00, 0x00, // 13
        0x77, 0x00, 0x69, 0x00, 0x64, 0x00, 0x65, 0x00, 0x20, 0x00, 0x68, 0xd5, 0x15, 0xc8, 0x20,
        0x00, 0x04, 0xc8, 0x2c, 0xd2, 0xb4, 0xcc, 0xc4, 0xac, 0x00, 0x00,
    ];
    let mut e = Encoder::new(Endian::Little);
    codec().put_wstring(&mut e, TEXT_BMP).expect("wstring");
    assert_eq!(e.finish().expect("finish"), OURS_LITTLE);

    // And read back the same way — our own little-endian body, and JacORB's
    // big-endian one, each in the message it travelled in.
    for (endian, raw) in [(Endian::Little, OURS_LITTLE), (Endian::Big, JACORB_BMP)] {
        let mut d = Decoder::new(raw, endian);
        assert_eq!(codec().get_wstring(&mut d).expect("decode"), TEXT_BMP, "{endian:?}");
    }

    // The 1.2 rule is different and stays different: unmarked is big-endian
    // whatever the stream (tests/wide_chars_from_a_peer.rs, omniORB 4.3.4).
    let v12 = WideCodec::new(Version::V1_2, CodeSetId::UTF_16).expect("codec");
    let body_1_2 = [0x02, 0x00, 0x00, 0x00, 0x00, 0x77]; // a little-endian count of two octets
    let mut d = Decoder::new(&body_1_2, Endian::Little);
    assert_eq!(v12.get_wstring(&mut d).expect("decode"), "w", "1.2 unmarked is big-endian");
    let mut d = Decoder::new(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x77, 0x00, 0x00], Endian::Little);
    assert_eq!(codec().get_wstring(&mut d).expect("decode"), "\u{7700}", "1.1 unmarked follows");
}

/// The reader's decision, taken deliberately: a leading mark at 1.1 is still
/// removed, because §9.3.1.6's "if a BOM is present at the beginning of a
/// wchar or wstring received in a GIOP message, the ORB shall remove the BOM
/// before passing the value to the user" names no version, and a peer that
/// marks at 1.1 — as we did — is then read as it meant to be, in the order it
/// marked. The cost is named: an unmarked 1.1 peer whose user's text begins
/// with a genuine U+FEFF loses it here, exactly as at any 1.2 reader.
#[test]
fn a_marked_1_1_wstring_from_a_peer_is_still_read_as_marked() {
    // Our own pre-fix bytes: JacORB's user got the mark; ours does not.
    let mut d = Decoder::new(OURS_BEFORE_THE_FIX, Endian::Big);
    assert_eq!(codec().get_wstring(&mut d).expect("decode"), TEXT_BMP);

    // A little-endian mark in a big-endian message says the units are
    // little-endian, and that wins over the stream.
    let marked_le = [0x00, 0x00, 0x00, 0x03, 0xff, 0xfe, 0x77, 0x00, 0x00, 0x00];
    let mut d = Decoder::new(&marked_le, Endian::Big);
    assert_eq!(codec().get_wstring(&mut d).expect("decode"), "w");
}
