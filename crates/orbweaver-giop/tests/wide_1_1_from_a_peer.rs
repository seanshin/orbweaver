//! A GIOP 1.1 `wstring` — and, from the second section on, a GIOP 1.1 `wchar`
//! — as the one 1.1 wide-text peer on this host actually writes and reads them.
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
//! The negotiated codesets were char=UTF-8, wchar=UTF-16, and the first two
//! facts the bytes establish are:
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

// ─────────────────────────────────────────────────────────────────────────────
// The single wide character, second half of D010 B5
// ─────────────────────────────────────────────────────────────────────────────
//
// Everything below came off the wire between JacORB 3.9 and the hand-built
// GIOP 1.1 peer in `spikes/jacorb_wchar11.py`, driven by
// `spikes/jacorb_wchar11.sh`, on 2026-08-19, over `spikes/wide.idl`
// (`IDL:spike/Wide:1.0`, `wchar echo_wchar(in wchar c)`), negotiated
// char=UTF-8 wchar=UTF-16. `spikes/echo.idl` has no `wchar` operation, which
// is why the 1.1 `wchar` — two octets, no length indication, nowhere to put a
// mark — had met no peer before this. The same clause (b) as above: the
// output of a program we ran, over types the OMG defines.
//
// The facts the bytes establish:
//
// 3. **A 1.1 `wchar` is its two octets in the MESSAGE's order, and nothing
//    else.** JacORB writes `d5 5c` for U+D55C in its (always big-endian)
//    messages. Given our reply `5c d5` in a *little-endian* message its user
//    received U+D55C; given `d5 5c` in a little-endian message (the control)
//    its user received U+5CD5, and every other unit swapped the same way,
//    4/4. In the other direction our little-endian request `5c d5` came back
//    as `d5 5c` (U+D55C) and the control's `d5 5c` in a little-endian request
//    came back as `5c d5` (U+5CD5). `WideCodec::put_wchar` and `get_wchar`
//    have always used the stream's order at 1.1; the measurement agrees.
//
// 4. **U+FEFF is data at 1.1.** A 1.1 `wchar` has no length, so §9.3.1.6's
//    "first two bytes (after the length indication)" has nowhere to apply;
//    JacORB writes `fe ff` for U+FEFF and echoes it, and its user gets U+FEFF
//    back from our `fe ff`. Our reader hands it over as data too — the 1.1
//    arm of `get_wchar` never looked for a mark.
//
// 5. **What a `wchar` cannot carry.** A character above the BMP is two UTF-16
//    units; a Java `char` is one, so JacORB's client can only *ask* for a
//    lone surrogate, and it writes one as `d8 3d` and echoes one back. Given
//    four octets `d8 3d de 00` as one wchar, JacORB read the first two and
//    ignored the rest (U+D83D back). Our reader refuses `d8 3d` — not a
//    character — and our writer refuses U+1F600 rather than splitting it.
//    Recorded as behaviour, not as a pass: neither side is wrong about a
//    value the type cannot hold.

use orbweaver_giop::server::{decode_request, encode_reply};
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, ReplyStatus, decode_reply, read_message};

/// The four units `spikes/jacorb_wchar11.sh` sends as `UNITS`, and what
/// JacORB's client wrote for each (`a.srv.log`, "request body: wchar 1.1
/// body=…"): 'w', whose swap U+7700 is a different valid character; '한';
/// U+FEFF as data; and a lone high surrogate.
const JACORB_WCHAR_W: &[u8] = &[0x00, 0x77];
const JACORB_WCHAR_HAN: &[u8] = &[0xd5, 0x5c];
const JACORB_WCHAR_FEFF: &[u8] = &[0xfe, 0xff];
const JACORB_WCHAR_LONE_SURROGATE: &[u8] = &[0xd8, 0x3d];

/// The whole `echo_wchar(U+D55C)` request JacORB's `WideClient` wrote — its
/// second request on the connection, so no service context is in it. From
/// `a.srv.log`:
///
/// ```text
/// [1] C->S GIOP 1.1 Request BE id=2 op=echo_wchar
///     request body: wchar 1.1 body=d55c read in the message's order (BE) -> U+D55C
///     0000  47 49 4f 50 01 01 00 00 00 00 00 36 00 00 00 00  GIOP.......6....
///     0010  00 00 00 02 01 00 00 00 00 00 00 0d 4f 72 62 77  ............Orbw
///     0020  65 61 76 65 72 57 69 64 65 00 00 00 00 00 00 0b  eaverWide.......
///     0030  65 63 68 6f 5f 77 63 68 61 72 00 00 00 00 00 00  echo_wchar......
///     0040  d5 5c                                            .\
/// ```
///
/// Header; no contexts; id 2; response expected and the three 1.1 reserved
/// octets; the 13-octet key; the operation; an empty principal; and the two
/// octets — 2-aligned, which after the principal's count they already are.
const JACORB_REQUEST_HAN: &[u8] = &[
    0x47, 0x49, 0x4f, 0x50, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x02, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d, 0x4f, 0x72, 0x62, 0x77,
    0x65, 0x61, 0x76, 0x65, 0x72, 0x57, 0x69, 0x64, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0b,
    0x65, 0x63, 0x68, 0x6f, 0x5f, 0x77, 0x63, 0x68, 0x61, 0x72, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xd5, 0x5c,
];

/// The whole reply JacORB's `WideServer` wrote to our `echo_wchar(U+D55C)`
/// request — the same 26 octets whether our request was big-endian (`d5 5c`)
/// or little-endian (`5c d5`): JacORB replies big-endian, and it read both.
/// From `d.be.log` and `d.le.log`:
///
/// ```text
/// S->C GIOP 1.1 Reply BE id=4 status=0
///     0000  47 49 4f 50 01 01 00 01 00 00 00 0e 00 00 00 00  GIOP............
///     0010  00 00 00 04 00 00 00 00 d5 5c                    .........\
/// ```
const JACORB_REPLY_HAN: &[u8] = &[
    0x47, 0x49, 0x4f, 0x50, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x0e, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0xd5, 0x5c,
];

/// What our side wrote back to JacORB's `echo_wchar(U+D55C)` — request id 2 —
/// and JacORB's user reported receiving U+D55C from: a big-endian reply
/// (`a.srv.log`) and a little-endian one with the unit in the message's
/// order (`b.srv.log`).
///
/// ```text
/// [1] S->C GIOP 1.1 Reply BE id=2 for=echo_wchar
///     0000  47 49 4f 50 01 01 00 01 00 00 00 0e 00 00 00 00  GIOP............
///     0010  00 00 00 02 00 00 00 00 d5 5c                    .........\
/// [1] S->C GIOP 1.1 Reply LE id=2 for=echo_wchar
///     0000  47 49 4f 50 01 01 01 01 0e 00 00 00 00 00 00 00  GIOP............
///     0010  02 00 00 00 00 00 00 00 5c d5                    ........\.
/// ```
///
/// The control — `d5 5c` in the little-endian frame — reached JacORB's user
/// as U+5CD5 (`c.client.log`), so these two are the only forms it reads as
/// U+D55C.
const OUR_REPLY_HAN_BE: &[u8] = &[
    0x47, 0x49, 0x4f, 0x50, 0x01, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x0e, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0xd5, 0x5c,
];
const OUR_REPLY_HAN_LE: &[u8] = &[
    0x47, 0x49, 0x4f, 0x50, 0x01, 0x01, 0x01, 0x01, 0x0e, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x5c, 0xd5,
];

/// The two octets follow the message: JacORB's big-endian octets decode here
/// in a big-endian stream, we re-encode them byte for byte, and in a
/// little-endian stream we write the swap — the octets JacORB read correctly
/// in a little-endian message — and read the swap back.
#[test]
fn a_1_1_wchar_is_written_and_read_in_the_messages_order_as_the_peer_does() {
    for (peer, c) in
        [(JACORB_WCHAR_W, 'w'), (JACORB_WCHAR_HAN, '한'), (JACORB_WCHAR_FEFF, '\u{FEFF}')]
    {
        let mut d = Decoder::new(peer, Endian::Big);
        assert_eq!(codec().get_wchar(&mut d).expect("decode"), c, "{c:?}: JacORB's octets");
        assert!(d.is_empty(), "{c:?}: two octets and nothing else");

        let mut e = Encoder::new(Endian::Big);
        codec().put_wchar(&mut e, c).expect("encode");
        assert_eq!(e.finish().expect("finish"), peer, "{c:?}: our big-endian octets are JacORB's");

        let swapped = [peer[1], peer[0]];
        let mut e = Encoder::new(Endian::Little);
        codec().put_wchar(&mut e, c).expect("encode");
        assert_eq!(
            e.finish().expect("finish"),
            swapped,
            "{c:?}: little-endian message, unit swapped"
        );
        let mut d = Decoder::new(&swapped, Endian::Little);
        assert_eq!(
            codec().get_wchar(&mut d).expect("decode"),
            c,
            "{c:?}: read in the message's order"
        );

        // And the control JacORB's user saw swapped: JacORB's octets in a
        // little-endian stream are the other character here as well.
        let mut d = Decoder::new(peer, Endian::Little);
        let other = codec().get_wchar(&mut d).expect("decode");
        assert_ne!(other, c, "{c:?}: big-endian octets in a little-endian message are not {c:?}");
    }
}

/// JacORB's whole request through our server's request decoder, and its whole
/// reply through our client's reply decoder — the version, the byte order and
/// the operation read from its bytes, and the character from the body.
#[test]
fn jacorbs_1_1_wchar_request_and_reply_decode_through_our_paths() {
    let raw = read_message(&mut JACORB_REQUEST_HAN.to_vec().as_slice(), DEFAULT_MAX_MESSAGE_SIZE)
        .expect("frames");
    let req = decode_request(raw).expect("decodes");
    assert_eq!(req.version, Version::V1_1);
    assert_eq!(req.endian, Endian::Big);
    assert_eq!(req.request_id, 2);
    assert_eq!(req.operation, "echo_wchar");
    assert_eq!(req.object_key, b"OrbweaverWide");
    let mut body = req.body().expect("body");
    let w =
        WideCodec::new(req.version, CodeSetId::UTF_16).expect("codec from the request's version");
    assert_eq!(w.get_wchar(&mut body).expect("wchar"), '한');
    assert!(body.is_empty(), "nothing after the wchar");

    let raw = read_message(&mut JACORB_REPLY_HAN.to_vec().as_slice(), DEFAULT_MAX_MESSAGE_SIZE)
        .expect("frames");
    let reply = decode_reply(raw).expect("decodes");
    assert_eq!(reply.version, Version::V1_1);
    assert_eq!(reply.endian, Endian::Big);
    assert_eq!(reply.request_id, 4);
    assert_eq!(reply.status, ReplyStatus::NoException);
    let mut body = reply.body().expect("body");
    assert_eq!(codec().get_wchar(&mut body).expect("wchar"), '한');
    assert!(body.is_empty(), "nothing after the wchar");
}

/// Our server's reply for `echo_wchar(U+D55C)` at 1.1, in each byte order, is
/// octet for octet the reply JacORB's user read as U+D55C — and not the
/// control it read as U+5CD5.
#[test]
fn our_1_1_wchar_reply_is_the_one_jacorbs_user_read_in_both_orders() {
    for (endian, want) in [(Endian::Big, OUR_REPLY_HAN_BE), (Endian::Little, OUR_REPLY_HAN_LE)] {
        let msg = encode_reply(Version::V1_1, endian, 2, ReplyStatus::NoException, None, |e| {
            codec().put_wchar(e, '한').expect("wchar");
        })
        .expect("encodes");
        assert_eq!(msg, want, "{endian:?}");
    }
    let mut e = Encoder::new(Endian::Little);
    codec().put_wchar(&mut e, '한').expect("wchar");
    assert_ne!(e.finish().expect("finish"), JACORB_WCHAR_HAN, "we do not write JacORB's control");
}

/// What a `wchar` cannot carry, on both sides. JacORB passes a lone surrogate
/// through as two octets and takes the first two of a four-octet "pair"; we
/// refuse the lone unit as not a character and refuse to split U+1F600. U+FEFF
/// is a character at 1.1, both ways.
#[test]
fn a_1_1_wchar_refuses_what_is_not_a_character_and_keeps_feff_as_data() {
    let mut d = Decoder::new(JACORB_WCHAR_LONE_SURROGATE, Endian::Big);
    assert!(codec().get_wchar(&mut d).is_err(), "a lone surrogate is not a char");

    let pair_as_one_wchar = [0xd8, 0x3d, 0xde, 0x00];
    let mut d = Decoder::new(&pair_as_one_wchar, Endian::Big);
    assert!(codec().get_wchar(&mut d).is_err(), "the first two octets are a lone surrogate");
    assert_eq!(d.remaining(), 2, "two octets consumed, as JacORB consumed them");

    let mut e = Encoder::new(Endian::Big);
    assert!(codec().put_wchar(&mut e, '\u{1F600}').is_err(), "two units are not one wchar");

    let mut e = Encoder::new(Endian::Big);
    codec().put_wchar(&mut e, '\u{FEFF}').expect("FEFF is a character");
    assert_eq!(e.finish().expect("finish"), JACORB_WCHAR_FEFF);
    let mut d = Decoder::new(JACORB_WCHAR_FEFF, Endian::Big);
    assert_eq!(codec().get_wchar(&mut d).expect("decode"), '\u{FEFF}', "data, not a mark, at 1.1");
}

// ─────────────────────────────────────────────────────────────────────────────
// The same contract with OUR stack in each seat, and one 1.2 finding
// ─────────────────────────────────────────────────────────────────────────────
//
// `spikes/wide_rust.sh` (2026-08-19) put `spike-wide` — `Server` + a
// hand-written `Dispatch`, `Connection` + `WideCodec`, in
// `crates/orbweaver-object/src/bin/spike_wide.rs` — on the wire for
// `spikes/wide.idl` in place of the hand-built Python peer above, recorded by
// `spikes/jacorb_giop11_tap.py`. Measured, and checked live against the
// constants of the previous section on every run of that script:
//
// * JacORB's `echo_wchar(U+D55C)` request as our **real server** received it
//   is `JACORB_REQUEST_HAN` octet for octet, and our real server's reply is
//   `OUR_REPLY_HAN_BE` octet for octet; JacORB's user got U+D55C.
// * Our **own client's** big-endian 1.1 request for U+D55C is
//   `JACORB_REQUEST_HAN` octet for octet as well (same key, same layout, same
//   id 2), and our real server's replies to our own client are
//   `OUR_REPLY_HAN_BE` and `OUR_REPLY_HAN_LE`, so the little-endian form
//   JacORB's user read as U+D55C is what the real server writes when the
//   request is little-endian — the case JacORB's own client cannot elicit,
//   since our server answers in the request's order and JacORB requests
//   big-endian only.
// * JacORB's reply to our real client's U+D55C request, in either order, is
//   `JACORB_REPLY_HAN` apart from the request id (4 was the hand-built
//   client's numbering, 2 is `Connection`'s).
// * JacORB's lone surrogate `d8 3d` reaching our real reader is refused —
//   MARSHAL to JacORB's user. The hand-built peer passed it through as
//   octets; the Rust reader does what
//   [`a_1_1_wchar_refuses_what_is_not_a_character_and_keeps_feff_as_data`]
//   says it does, now on the wire.
//
// And one finding at 1.2, from the self-consistency arm and then against
// JacORB in both directions with the same binary:
//
// 6. **U+FEFF as a 1.2 `wchar` crossed neither stack** (as first measured;
//    revised the same day, below). Both writers — ours and JacORB 3.9's —
//    wrote it as `02 fe ff`: an octet count of two and the unit, no mark
//    before it. Both readers then took a leading `fe ff` for the mark
//    §9.3.1.6 says an ORB "shall remove … before passing the value to the
//    user", and were left with nothing: JacORB hands its user U+0000 and
//    echoes `02 00 00`; our reader refused the empty remainder with MARSHAL.
//    Under the paragraph as written a writer that means the character has to
//    put a mark in front of it (`04 fe ff fe ff`), which neither did.
//    **Revised:** JacORB's reader was then asked about the marked form and
//    honours it (fact 7, `tests/wide_1_2_from_a_peer.rs`, where the 1.2
//    octets and their tests now live); `put_wchar` at 1.2 marks U+FEFF and
//    U+FFFE, `get_wchar` at 1.2 reads a bare two-octet mark as the unit it
//    is, and the code point crosses both ways. JacORB's own writer still
//    writes `02 fe ff` (fact 8). The test below keeps only what this file
//    is about — the 1.1 arm, where U+FEFF is data with no mark to be
//    confused with — and that the bare 1.2 form is no longer ours.

/// What JacORB writes for `echo_wchar(U+FEFF)` at GIOP 1.2, and what we
/// wrote before the writer learned to mark: pinned in full, with its reader's
/// matrix, in `tests/wide_1_2_from_a_peer.rs`; here only as the form our
/// writer no longer produces.
const JACORB_WCHAR_FEFF_1_2: &[u8] = &[0x02, 0xfe, 0xff];

/// Fact 6 as revised: the 1.1 arm keeps U+FEFF as data (fact 4), which is why
/// the same unit round-trips at 1.1 in every seat of `spikes/wide_rust.sh`;
/// and at 1.2 our writer no longer produces the bare form JacORB does — the
/// marked form and both readers' behaviour are `wide_1_2_from_a_peer.rs`'s.
#[test]
fn at_1_2_a_wchar_that_is_itself_a_mark_is_no_longer_written_bare_and_at_1_1_it_never_was_a_mark() {
    let w12 = WideCodec::new(Version::V1_2, CodeSetId::UTF_16).expect("1.2 + UTF-16");
    for endian in [Endian::Big, Endian::Little] {
        let mut e = Encoder::new(endian);
        w12.put_wchar(&mut e, '\u{FEFF}').expect("FEFF is a character");
        let ours = e.finish().expect("finish");
        assert_ne!(ours, JACORB_WCHAR_FEFF_1_2, "{endian:?}: JacORB's bare form was the defect");
        assert_eq!(ours[0], 4, "{endian:?}: a mark and the unit — four octets");
        let mut d = Decoder::new(&ours, endian);
        assert_eq!(w12.get_wchar(&mut d).expect("our own marked form"), '\u{FEFF}', "{endian:?}");
    }
    let mut e = Encoder::new(Endian::Big);
    codec().put_wchar(&mut e, '\u{FEFF}').expect("FEFF");
    let bytes = e.finish().expect("finish");
    assert_eq!(bytes, JACORB_WCHAR_FEFF, "1.1: two octets, no count, nothing to take for a mark");
    let mut d = Decoder::new(&bytes, Endian::Big);
    assert_eq!(codec().get_wchar(&mut d).expect("data at 1.1"), '\u{FEFF}');
}
