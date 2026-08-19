//! A GIOP 1.2 `wchar` that is itself a byte-order mark — U+FEFF, and its
//! reverse U+FFFE — as the one peer on this host that can be asked reads and
//! writes it.
//!
//! Every byte sequence below came off the wire between JacORB 3.9 and our
//! own stack (`spike-wide`, `crates/orbweaver-object/src/bin/spike_wide.rs`,
//! and the hand-built client of `spikes/jacorb_wchar11.py` following an IIOP
//! 1.2 profile with the octets given verbatim) on 2026-08-19, recorded by
//! `spikes/jacorb_giop11_tap.py` and driven by `spikes/wide_rust.sh` —
//! clause (b) of the licensing boundary, the output of a program we ran over
//! IDL types the OMG defines. Nothing of JacORB's is linked, vendored or
//! redistributed. Negotiated char=UTF-8, wchar=UTF-16.
//!
//! The finding this revises is fact 6 of `wide_1_1_from_a_peer.rs`: both
//! writers wrote U+FEFF at 1.2 as `02 fe ff` — count two, the unit, no mark —
//! and both readers took the `fe ff` for the mark §9.3.1.6 (CORBA 3.4 Part 2)
//! says to remove: "if a BOM is present at the beginning of a wchar or wstring
//! received in a GIOP message, the ORB shall remove the BOM before passing the
//! value to the user". JacORB handed its user U+0000; our reader refused the
//! empty remainder. The same paragraph says how a writer that means the
//! character avoids that — "if an ORB decides to use BOM to indicate
//! endianness, it shall add the BOM …" — and whether JacORB's reader would
//! honour that had not been asked. Now it has, and the facts are:
//!
//! 7. **JacORB's 1.2 reader honours a leading mark in either order, removes
//!    it, and reads the unit after it in the mark's order** — in a big-endian
//!    and a little-endian message alike (its own replies are big-endian
//!    only). `04 fe ff fe ff` and `04 ff fe ff fe` reach its user as U+FEFF;
//!    `04 fe ff ff fe` and `04 ff fe fe ff` as U+FFFE; the controls
//!    `04 fe ff d5 5c` and `04 ff fe 5c d5` as U+D55C, `04 fe ff 00 41` as
//!    U+0041. Unmarked is big-endian whatever the message's order: `02 5c d5`
//!    in a little-endian message is U+5CD5 to it, which is §9.3.1.6's third
//!    bullet and what our reader has done since `wide_chars_from_a_peer.rs`.
//!
//! 8. **JacORB's 1.2 writer never marks**, and writes the two units its own
//!    reader cannot read back: `02 fe ff` for U+FEFF and `02 ff fe` for
//!    U+FFFE (its `WideClient` to our server; its `WideServer`'s echo of our
//!    marked request). Given `02 fe ff` its reader hands its user U+0000 and
//!    echoes `02 00 00`; given `02 ff fe` it read past the value — U+0008
//!    came back from a big-endian message in one run, U+0000 in the next —
//!    so what it does with a bare reversed mark is not a value at all and is
//!    recorded here as prose, not pinned.
//!
//! What changed in `codeset.rs` on the strength of that: `put_wchar` at 1.2
//! writes U+FEFF as `04 fe ff fe ff` and U+FFFE as `04 fe ff ff fe`, in
//! either stream order (the mark states the big-endian order every other 1.2
//! unit of ours is in already), and every other unit exactly as before; and
//! `get_wchar` at 1.2 reads a two-octet body that is exactly a mark as the
//! unit it is — `02 fe ff` is U+FEFF, `02 ff fe` is U+FFFE — because a
//! `wchar` is never empty, so no marking writer produces those octets and
//! the only writer that does (JacORB's, and ours until this) means the
//! character. Measured after the change with the same fixtures: JacORB's user
//! gets U+FEFF and U+FFFE back from our real server's marked replies, and
//! our real client gets both back from JacORB's bare echoes, in both request
//! byte orders. So the one code point that crossed neither stack now crosses
//! both ways.

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_giop::Version;
use orbweaver_giop::codeset::{CodeSetId, WideCodec};
use orbweaver_giop::server::{decode_request, encode_reply};
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, ReplyStatus, decode_reply, read_message};

fn utf16() -> WideCodec {
    WideCodec::new(Version::V1_2, CodeSetId::UTF_16).expect("1.2 + UTF-16 is always valid")
}

fn ucs2() -> WideCodec {
    WideCodec::new(Version::V1_2, CodeSetId::UCS_2).expect("1.2 + UCS-2 is always valid")
}

/// Fact 7, as `spikes/jacorb_wchar11.py client --expect-minor 2` sent it and
/// JacORB's `WideServer` echoed it (`d12.be.out` / `d12.le.out` of
/// `spikes/wide_rust.sh`; the same echo from a big-endian and a little-endian
/// message): the octets given verbatim, the octets JacORB's writer produced
/// for the character its user received, and that character.
const JACORB_READER_1_2: &[(&[u8], &[u8], char)] = &[
    (&[0x04, 0xfe, 0xff, 0xfe, 0xff], &[0x02, 0xfe, 0xff], '\u{FEFF}'),
    (&[0x04, 0xff, 0xfe, 0xff, 0xfe], &[0x02, 0xfe, 0xff], '\u{FEFF}'),
    (&[0x04, 0xfe, 0xff, 0xff, 0xfe], &[0x02, 0xff, 0xfe], '\u{FFFE}'),
    (&[0x04, 0xff, 0xfe, 0xfe, 0xff], &[0x02, 0xff, 0xfe], '\u{FFFE}'),
    (&[0x04, 0xfe, 0xff, 0xd5, 0x5c], &[0x02, 0xd5, 0x5c], '한'),
    (&[0x04, 0xff, 0xfe, 0x5c, 0xd5], &[0x02, 0xd5, 0x5c], '한'),
    (&[0x04, 0xfe, 0xff, 0x00, 0x41], &[0x02, 0x00, 0x41], 'A'),
    (&[0x02, 0xd5, 0x5c], &[0x02, 0xd5, 0x5c], '한'),
    (&[0x02, 0x5c, 0xd5], &[0x02, 0x5c, 0xd5], '\u{5CD5}'),
    (&[0x04, 0xff, 0xfe, 0x00, 0x00], &[0x02, 0x00, 0x00], '\u{0000}'),
];

/// Fact 8: what JacORB writes for the two units at 1.2 — its `WideClient`'s
/// request bodies to our real server, and its `WideServer`'s echoes of our
/// marked requests, the same three octets each way — and what its own reader
/// makes of the first: U+0000, echoed as `02 00 00`.
const JACORB_WRITES_FEFF_1_2: &[u8] = &[0x02, 0xfe, 0xff];
const JACORB_WRITES_FFFE_1_2: &[u8] = &[0x02, 0xff, 0xfe];
const JACORB_READS_ITS_OWN_FEFF_AS: &[u8] = &[0x02, 0x00, 0x00];

/// The whole `echo_wchar(U+FEFF)` request JacORB's `WideClient` wrote at 1.2
/// (`a2.tap.log`, its third request on the connection, id 4, no contexts):
///
/// ```text
/// [1] C->S GIOP 1.2 Request size=55 BE id=4 op=echo_wchar
///     0000  47 49 4f 50 01 02 00 00 00 00 00 37 00 00 00 04  GIOP.......7....
///     0010  03 00 00 00 00 00 00 00 00 00 00 0d 4f 72 62 77  ............Orbw
///     0020  65 61 76 65 72 57 69 64 65 00 00 00 00 00 00 0b  eaverWide.......
///     0030  65 63 68 6f 5f 77 63 68 61 72 00 00 00 00 00 00  echo_wchar......
///     0040  02 fe ff                                         ...
/// ```
///
/// Header; id 4; response flags 3 and the reserved octets; KeyAddr and the
/// 13-octet key; the operation; no contexts; the body on its 8-octet boundary.
const JACORB_REQUEST_FEFF_1_2: &[u8] = &[
    0x47, 0x49, 0x4f, 0x50, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x37, 0x00, 0x00, 0x00, 0x04,
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d, 0x4f, 0x72, 0x62, 0x77,
    0x65, 0x61, 0x76, 0x65, 0x72, 0x57, 0x69, 0x64, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0b,
    0x65, 0x63, 0x68, 0x6f, 0x5f, 0x77, 0x63, 0x68, 0x61, 0x72, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x02, 0xfe, 0xff,
];

/// Our real server's reply to it (same log), the one JacORB's user read as
/// U+FEFF:
///
/// ```text
/// [1] S->C GIOP 1.2 Reply size=17 BE id=4 status=0 for=echo_wchar
///     0000  47 49 4f 50 01 02 00 01 00 00 00 11 00 00 00 04  GIOP............
///     0010  00 00 00 00 00 00 00 00 04 fe ff fe ff           .............
/// ```
const OUR_REPLY_FEFF_1_2_BE: &[u8] = &[
    0x47, 0x49, 0x4f, 0x50, 0x01, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x11, 0x00, 0x00, 0x00, 0x04,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0xfe, 0xff, 0xfe, 0xff,
];

/// JacORB's whole reply to our real client's marked U+FEFF request at 1.2 —
/// the same 27 octets to our big-endian and our little-endian request
/// (`b2.tap.log`, id 3 under `Connection`'s numbering): its bare `02 fe ff`.
///
/// ```text
/// [1] S->C GIOP 1.2 Reply size=15 BE id=3 status=0 for=echo_wchar
///     0000  47 49 4f 50 01 02 00 01 00 00 00 0f 00 00 00 03  GIOP............
///     0010  00 00 00 00 00 00 00 00 02 fe ff                 ...........
/// ```
const JACORB_REPLY_FEFF_1_2: &[u8] = &[
    0x47, 0x49, 0x4f, 0x50, 0x01, 0x02, 0x00, 0x01, 0x00, 0x00, 0x00, 0x0f, 0x00, 0x00, 0x00, 0x03,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xfe, 0xff,
];

/// The writer: the two units a reader would remove go behind a mark, in both
/// stream orders, and every other unit keeps the bare form JacORB and omniORB
/// write. This is the negative control's positive half: before the change
/// this writer produced `JACORB_WRITES_FEFF_1_2` here (`cargo test -p
/// orbweaver-giop --test wide_1_2_from_a_peer`, run red once, output in the
/// commit message).
#[test]
fn at_1_2_our_writer_marks_the_two_units_a_reader_would_remove_and_no_other() {
    for endian in [Endian::Big, Endian::Little] {
        for (c, want) in [
            ('\u{FEFF}', &[0x04, 0xfe, 0xff, 0xfe, 0xff][..]),
            ('\u{FFFE}', &[0x04, 0xfe, 0xff, 0xff, 0xfe][..]),
            ('한', &[0x02, 0xd5, 0x5c][..]),
            ('w', &[0x02, 0x00, 0x77][..]),
            ('\u{FEFE}', &[0x02, 0xfe, 0xfe][..]),
            ('\u{FFFF}', &[0x02, 0xff, 0xff][..]),
        ] {
            let mut e = Encoder::new(endian);
            utf16().put_wchar(&mut e, c).expect("one unit");
            let ours = e.finish().expect("finish");
            assert_eq!(ours, want, "{endian:?} {c:?}");
            assert_ne!(
                ours, JACORB_WRITES_FEFF_1_2,
                "{endian:?} {c:?}: the bare mark is the defect"
            );
            assert_ne!(ours, JACORB_WRITES_FFFE_1_2, "{endian:?} {c:?}: so is its reverse");
        }
    }
}

/// Our reader against fact 7: every form JacORB's reader read, read here to
/// the character its user got, in both stream orders; JacORB's echo of it read
/// to the same character; and the character re-encoded to the form our writer
/// now emits — which JacORB reads (fact 7 again), so the round trip closes.
#[test]
fn at_1_2_our_reader_reads_every_form_the_peer_read_and_jacorbs_echoes_of_them() {
    for &(sent, echoed, c) in JACORB_READER_1_2 {
        for endian in [Endian::Big, Endian::Little] {
            let mut d = Decoder::new(sent, endian);
            assert_eq!(
                utf16().get_wchar(&mut d).expect("decodes"),
                c,
                "{endian:?} sent {sent:02x?}"
            );
            assert!(d.is_empty(), "{endian:?} sent {sent:02x?}: nothing after the count");

            let mut d = Decoder::new(echoed, endian);
            assert_eq!(
                utf16().get_wchar(&mut d).expect("decodes"),
                c,
                "{endian:?} JacORB's echo {echoed:02x?}"
            );

            let mut e = Encoder::new(endian);
            utf16().put_wchar(&mut e, c).expect("one unit");
            let ours = e.finish().expect("finish");
            let mut d = Decoder::new(&ours, endian);
            assert_eq!(
                utf16().get_wchar(&mut d).expect("decodes"),
                c,
                "{endian:?} {c:?} round trip"
            );
        }
    }
}

/// Fact 8 at our reader: JacORB's bare `02 fe ff` is U+FEFF here and its bare
/// `02 ff fe` is U+FFFE — the unit read as an unmarked one — while its own
/// reading of `02 fe ff`, `02 00 00`, is U+0000 to us as it was to its user.
/// A four-octet marked form still has its mark removed, so a mark followed by
/// a mark is one character, not two.
#[test]
fn at_1_2_a_body_that_is_exactly_a_mark_is_the_unit_and_a_marked_mark_is_one_character() {
    for endian in [Endian::Big, Endian::Little] {
        let mut d = Decoder::new(JACORB_WRITES_FEFF_1_2, endian);
        assert_eq!(utf16().get_wchar(&mut d).expect("decodes"), '\u{FEFF}', "{endian:?}");
        let mut d = Decoder::new(JACORB_WRITES_FFFE_1_2, endian);
        assert_eq!(utf16().get_wchar(&mut d).expect("decodes"), '\u{FFFE}', "{endian:?}");
        let mut d = Decoder::new(JACORB_READS_ITS_OWN_FEFF_AS, endian);
        assert_eq!(utf16().get_wchar(&mut d).expect("decodes"), '\u{0000}', "{endian:?}");
    }
    // Still refused: a count of zero, an odd count, and a mark followed by two
    // units — a marked value with nothing after the mark cannot occur, and a
    // count of two that is a mark is now the unit, so neither is in this list.
    for raw in [
        &[0x00u8][..],
        &[0x03, 0xfe, 0xff, 0x00][..],
        &[0x06, 0xfe, 0xff, 0xfe, 0xff, 0xfe, 0xff][..],
    ] {
        let mut d = Decoder::new(raw, Endian::Big);
        assert!(utf16().get_wchar(&mut d).is_err(), "{raw:02x?} is not one wchar");
    }
}

/// UCS-2 has no mark (§9.3.1.6's BOM paragraph opens "if UTF-16 is selected
/// as the TCS-W"), so U+FEFF is written bare in the message's order and read
/// back as itself — the same two-octet rule closes that round trip too. No
/// UCS-2 peer exists on this host; this is self-consistency, and says so.
#[test]
fn at_1_2_a_ucs_2_wchar_that_is_a_mark_is_written_bare_in_the_messages_order_and_read_back() {
    for (endian, feff, fffe) in [
        (Endian::Big, &[0x02, 0xfe, 0xff][..], &[0x02, 0xff, 0xfe][..]),
        (Endian::Little, &[0x02, 0xff, 0xfe][..], &[0x02, 0xfe, 0xff][..]),
    ] {
        for (c, want) in [('\u{FEFF}', feff), ('\u{FFFE}', fffe)] {
            let mut e = Encoder::new(endian);
            ucs2().put_wchar(&mut e, c).expect("one unit");
            let ours = e.finish().expect("finish");
            assert_eq!(ours, want, "{endian:?} {c:?}: UCS-2, no mark, the message's order");
            let mut d = Decoder::new(&ours, endian);
            assert_eq!(ucs2().get_wchar(&mut d).expect("decodes"), c, "{endian:?} {c:?}");
        }
    }
}

/// JacORB's whole 1.2 request for U+FEFF through our server's request decoder,
/// our real server's whole reply reproduced by our reply encoder, and JacORB's
/// whole reply to our marked request through our client's reply decoder — the
/// version, order, id and operation from its bytes, the character from the
/// body.
#[test]
fn jacorbs_1_2_feff_request_and_reply_decode_through_our_paths_and_our_reply_is_the_recorded_one() {
    let raw =
        read_message(&mut JACORB_REQUEST_FEFF_1_2.to_vec().as_slice(), DEFAULT_MAX_MESSAGE_SIZE)
            .expect("frames");
    let req = decode_request(raw).expect("decodes");
    assert_eq!(req.version, Version::V1_2);
    assert_eq!(req.endian, Endian::Big);
    assert_eq!(req.request_id, 4);
    assert_eq!(req.operation, "echo_wchar");
    assert_eq!(req.object_key, b"OrbweaverWide");
    let mut body = req.body().expect("body");
    let w = WideCodec::new(req.version, CodeSetId::UTF_16).expect("codec");
    assert_eq!(w.get_wchar(&mut body).expect("wchar"), '\u{FEFF}');
    assert!(body.is_empty(), "nothing after the wchar");

    let msg = encode_reply(Version::V1_2, Endian::Big, 4, ReplyStatus::NoException, None, |e| {
        w.put_wchar(e, '\u{FEFF}').expect("wchar");
    })
    .expect("encodes");
    assert_eq!(msg, OUR_REPLY_FEFF_1_2_BE, "the reply JacORB's user read as U+FEFF");

    let raw =
        read_message(&mut JACORB_REPLY_FEFF_1_2.to_vec().as_slice(), DEFAULT_MAX_MESSAGE_SIZE)
            .expect("frames");
    let reply = decode_reply(raw).expect("decodes");
    assert_eq!(reply.version, Version::V1_2);
    assert_eq!(reply.endian, Endian::Big);
    assert_eq!(reply.request_id, 3);
    assert_eq!(reply.status, ReplyStatus::NoException);
    let mut body = reply.body().expect("body");
    assert_eq!(utf16().get_wchar(&mut body).expect("wchar"), '\u{FEFF}');
    assert!(body.is_empty(), "nothing after the wchar");
}
