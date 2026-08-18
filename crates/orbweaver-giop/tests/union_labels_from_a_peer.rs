//! A union `TypeCode` as a conformant peer actually writes it.
//!
//! Both byte sequences below were captured from omniORB 4.3.4 marshalling an
//! `any` on a little-endian host — clause (b) of the licensing boundary, the
//! output of a program we ran, describing IDL we wrote. Nothing of omniORB's
//! is linked, vendored or redistributed; what is recorded here is what the OMG
//! specification requires, with a second implementation as the witness that we
//! read it correctly.
//!
//! They are here because our own round trip could not have found either bug.
//! Labels were stored and re-emitted as raw bytes, so encode and decode agreed
//! with each other in any byte order, and 1200 tests stayed green while:
//!
//! - a `long long` discriminated union **could not be decoded at all**, and
//!   said so as `"string length must include the NUL"` — a diagnostic pointing
//!   four fields past the actual fault, at a string, because an unaligned
//!   8-byte read had shifted everything after it;
//! - a `long` discriminated union decoded and then missed **every** branch,
//!   with a refusal that blamed the caller's discriminator.

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_giop::typecode::{TypeCode, decode, encode};

/// `union U switch (long) { case 1: long as_long; case 2: string as_text; }`.
///
/// Bytes 8..12 are the encapsulation's byte-order flag followed by three bytes
/// omniORB does not zero — the padding rule in `CLAUDE.md`, visible. Their
/// content is undefined and this test does not read them.
const LONG_DISCRIMINATED: &[u8] = &[
    0x10, 0x00, 0x00, 0x00, 0x58, 0x00, 0x00, 0x00, 0x01, 0x1d, 0x1c, 0x6f, 0x0d, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x75, 0x74, 0x2f, 0x55, 0x3a, 0x31, 0x2e, 0x30, 0x00, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x55, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
    0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x61, 0x73, 0x5f, 0x6c,
    0x6f, 0x6e, 0x67, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00,
    0x61, 0x73, 0x5f, 0x74, 0x65, 0x78, 0x74, 0x00, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// `union W switch (long long) { case 1: long a; case 2: string b;
/// case 3: double c; }` — the one that could not be decoded at all, because an
/// 8-byte label must be 8-aligned inside the encapsulation and was read where
/// it was not.
const LONG_LONG_DISCRIMINATED: &[u8] = &[
    0x10, 0x00, 0x00, 0x00, 0x74, 0x00, 0x00, 0x00, 0x01, 0x1d, 0x03, 0x6d, 0x0e, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x75, 0x74, 0x32, 0x2f, 0x57, 0x3a, 0x31, 0x2e, 0x30, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x57, 0x00, 0x00, 0x00, 0x17, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x61, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x62, 0x00, 0x00, 0x00,
    0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x63, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00,
];

fn cases_of(tc: &TypeCode) -> Vec<(String, Vec<u8>)> {
    let TypeCode::Union { cases, .. } = tc else { panic!("not a union: {tc:?}") };
    cases.iter().map(|c| (c.name.clone(), c.label.clone())).collect()
}

#[test]
fn a_long_discriminated_union_from_a_little_endian_peer_reads() {
    let tc = decode(&mut Decoder::new(LONG_DISCRIMINATED, Endian::Little)).expect("decode");
    assert_eq!(
        cases_of(&tc),
        vec![("as_long".to_owned(), vec![0, 0, 0, 1]), ("as_text".to_owned(), vec![0, 0, 0, 2]),],
        "labels are held big-endian whatever the wire said"
    );
}

#[test]
fn a_long_long_discriminated_union_from_a_little_endian_peer_reads() {
    let tc = decode(&mut Decoder::new(LONG_LONG_DISCRIMINATED, Endian::Little)).expect("decode");
    assert_eq!(
        cases_of(&tc),
        vec![
            ("a".to_owned(), vec![0, 0, 0, 0, 0, 0, 0, 1]),
            ("b".to_owned(), vec![0, 0, 0, 0, 0, 0, 0, 2]),
            ("c".to_owned(), vec![0, 0, 0, 0, 0, 0, 0, 3]),
        ],
    );
}

/// The other direction, and the one our own round trip could never check:
/// re-encoding must reproduce the peer's bytes, not merely something we can
/// read back. Padding is excluded — its content is undefined and omniORB does
/// not zero it, so comparing it would be comparing noise.
#[test]
fn re_encoding_reproduces_what_the_peer_wrote() {
    for (what, original) in [("long", LONG_DISCRIMINATED), ("long long", LONG_LONG_DISCRIMINATED)] {
        let tc = decode(&mut Decoder::new(original, Endian::Little)).expect("decode");
        let mut e = Encoder::new(Endian::Little);
        encode(&mut e, &tc).expect("encode");
        let ours = e.finish().expect("finish");
        assert_eq!(ours.len(), original.len(), "{what}: length");
        for (i, (a, b)) in ours.iter().zip(original).enumerate() {
            // 8..12 is the encapsulation flag plus three undefined bytes.
            if (9..12).contains(&i) {
                continue;
            }
            assert_eq!(a, b, "{what}: byte {i} differs: ours {a:#04x}, peer's {b:#04x}");
        }
    }
}

/// Both directions at once, for the orders a peer can choose. This is the
/// property the raw-byte implementation satisfied *vacuously* — it agreed with
/// itself — so it is asserted here against the peer's bytes above rather than
/// on its own.
#[test]
fn a_union_typecode_survives_either_byte_order() {
    let tc = decode(&mut Decoder::new(LONG_LONG_DISCRIMINATED, Endian::Little)).expect("decode");
    for endian in [Endian::Big, Endian::Little] {
        let mut e = Encoder::new(endian);
        encode(&mut e, &tc).expect("encode");
        let bytes = e.finish().expect("finish");
        let back = decode(&mut Decoder::new(&bytes, endian)).expect("decode");
        assert_eq!(cases_of(&back), cases_of(&tc), "{endian:?}");
    }
}
