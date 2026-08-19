//! A union `TypeCode` with a `default:` branch as a conformant peer writes it.
//!
//! Every byte sequence below was captured from omniORB 4.3.4 (omniORBpy,
//! `cdrMarshal(CORBA._tc_TypeCode, tc, endian)`) on 2026-08-19 by
//! `spikes/union_default_capture.py`, which retakes them from the live
//! fixture and compares outside the padding — clause (b) of the licensing
//! boundary, the output of a program we ran, describing IDL we wrote.
//!
//! They exist because our own round trip could not have found the defect they
//! pin. The registry stores a bare `default:` case with an **empty** label,
//! and `encode` wrote it as it stood — zero bytes — while `decode` read a
//! label of the discriminator's width for every case; our own encoding of
//! `corpus/golden/06`'s `WithDefault` failed to decode with "implausible CDR
//! length prefix", and omniORB refused it with `MARSHAL_PassEndOfMessage`.
//! Every gate that touched the shape ran both ends through the same encoder.
//!
//! What the specification says (CORBA 3.4 Part 2, §9.3.5.1.4, "Encoding the
//! tk_union Default Case"): "The discriminant value used in the actual
//! typecode parameter associated with the default member position in the
//! list, may be any valid value of the discriminant type, and has no semantic
//! significance (i.e., it should be ignored and is only included for
//! syntactic completeness of union type code marshaling)." So: a label of the
//! discriminator's width, always present, value ignored. (The "zero octet"
//! the `TypeCode` *interface* returns from `member_label` for the default
//! member is a different thing — an `any` in the API, not bytes on the wire.)
//!
//! What omniORB writes for that value, measured: a value no other case uses —
//! signed integers count up from the type's minimum (`long` → `0x80000000`,
//! `short` → `0x8000`, `long long` → `0x8000_0000_0000_0000`), unsigned ones
//! down from the maximum, `char` down from `'\377'`, `boolean` the value left
//! free, an enum the first unused enumerator. And it ignores the value when
//! reading: handed a TypeCode whose default was labelled 1 next to `case 1:`,
//! omniORB still selected `case 1:` for a discriminator of 1.
//!
//! What JacORB 3.9 writes, measured the same day: zeros of the discriminator's
//! width. What it *reads*: one octet, which must be 0 or the TypeCode is
//! refused (`BAD_PARAM: Label type does not match discriminator type`) —
//! the interface's "zero octet" leaked into its wire reader. So omniORB's
//! `0x80000000` passes JacORB little-endian (`00 00 00 80`, first byte 0) and
//! would fail it big-endian; a labelled default's own label (`case 2:
//! default:` written as 2) fails it little-endian.
//!
//! What we write, therefore: **zeros of the discriminator's width, always** —
//! legal by §9.3.5.1.4, ignored by omniORB, the one value JacORB accepts in
//! both byte orders. What we read: the slot, ignored — the default comes back
//! with no label, as the registry builds it, so a peer that wrote a colliding
//! value (the specification does not forbid it) cannot make the default steal
//! a real branch from anything that selects by comparing labels.

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_giop::typecode::{TypeCode, decode, encode};

/// `union DL switch (long) { case 1: long a; default: string b; }`.
/// The default's label, at bytes 68..72, is `00 00 00 80`: `i32::MIN`,
/// little-endian.
const LONG_DEFAULT: &[u8] = &[
    0x10, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01, 0xb8, 0x3f, 0x03, 0x10, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x75, 0x64, 0x65, 0x66, 0x2f, 0x44, 0x4c, 0x3a, 0x31, 0x2e, 0x30, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x44, 0x4c, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x61, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x02, 0x00, 0x00, 0x00, 0x62, 0x00, 0x00, 0x00,
    0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// The same union in a big-endian stream. omniORB still writes the
/// encapsulation little-endian (flag `01` at byte 8): only the kind and the
/// length flip. A decoder that took the stream's order into the
/// encapsulation would read every label reversed.
const LONG_DEFAULT_BIG_ENDIAN_STREAM: &[u8] = &[
    0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x50, 0x01, 0xd6, 0xbd, 0x07, 0x10, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x75, 0x64, 0x65, 0x66, 0x2f, 0x44, 0x4c, 0x3a, 0x31, 0x2e, 0x30, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x44, 0x4c, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x61, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x02, 0x00, 0x00, 0x00, 0x62, 0x00, 0x00, 0x00,
    0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// `union DS switch (short) { case 1: short a; default: string b; }` —
/// default label `00 80` (`i16::MIN`), two bytes, then two of padding.
const SHORT_DEFAULT: &[u8] = &[
    0x10, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01, 0x50, 0xc5, 0x07, 0x10, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x75, 0x64, 0x65, 0x66, 0x2f, 0x44, 0x53, 0x3a, 0x31, 0x2e, 0x30, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x44, 0x53, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x61, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x00, 0x80, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x62, 0x00, 0x00, 0x00,
    0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// `union DB switch (boolean) { case TRUE: long yes; default: octet no; }` —
/// default label `00`, the value `TRUE` left free, one byte.
const BOOLEAN_DEFAULT: &[u8] = &[
    0x10, 0x00, 0x00, 0x00, 0x4c, 0x00, 0x00, 0x00, 0x01, 0x51, 0xc5, 0x07, 0x10, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x75, 0x64, 0x65, 0x66, 0x2f, 0x44, 0x42, 0x3a, 0x31, 0x2e, 0x30, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x44, 0x42, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x79, 0x65, 0x73, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x6e, 0x6f, 0x00, 0x00,
    0x0a, 0x00, 0x00, 0x00,
];

/// `union DC switch (char) { case 'a': long a; default: string b; }` —
/// default label `ff`, counting down from `'\377'`.
const CHAR_DEFAULT: &[u8] = &[
    0x10, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x01, 0xa9, 0x9a, 0x02, 0x10, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x75, 0x64, 0x65, 0x66, 0x2f, 0x44, 0x43, 0x3a, 0x31, 0x2e, 0x30, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x44, 0x43, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x61, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x61, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x62, 0x00, 0x00, 0x00,
    0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// `enum Hue { RED, GREEN, BLUE }; union DE switch (Hue) { case RED: octet
/// warm; default: string named; }` — default label `01 00 00 00`, `GREEN`,
/// the first enumerator no case names.
const ENUM_DEFAULT: &[u8] = &[
    0x10, 0x00, 0x00, 0x00, 0xa4, 0x00, 0x00, 0x00, 0x01, 0xd6, 0xc3, 0x07, 0x10, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x75, 0x64, 0x65, 0x66, 0x2f, 0x44, 0x45, 0x3a, 0x31, 0x2e, 0x30, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x44, 0x45, 0x00, 0x00, 0x11, 0x00, 0x00, 0x00, 0x45, 0x00, 0x00, 0x00,
    0x01, 0xe3, 0x93, 0x02, 0x11, 0x00, 0x00, 0x00, 0x49, 0x44, 0x4c, 0x3a, 0x75, 0x64, 0x65, 0x66,
    0x2f, 0x48, 0x75, 0x65, 0x3a, 0x31, 0x2e, 0x30, 0x00, 0x89, 0xd0, 0x07, 0x04, 0x00, 0x00, 0x00,
    0x48, 0x75, 0x65, 0x00, 0x03, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x52, 0x45, 0x44, 0x00,
    0x06, 0x00, 0x00, 0x00, 0x47, 0x52, 0x45, 0x45, 0x4e, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00,
    0x42, 0x4c, 0x55, 0x45, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x77, 0x61, 0x72, 0x6d, 0x00, 0x00, 0x00, 0x00,
    0x0a, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x6e, 0x61, 0x6d, 0x65,
    0x64, 0x00, 0x00, 0x00, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// `union DLL switch (long long) { case 1: long a; default: string b; }` —
/// default label `00 00 00 00 00 00 00 80` (`i64::MIN`), 8-aligned.
const LONG_LONG_DEFAULT: &[u8] = &[
    0x10, 0x00, 0x00, 0x00, 0x60, 0x00, 0x00, 0x00, 0x01, 0x8d, 0xc2, 0x07, 0x11, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x75, 0x64, 0x65, 0x66, 0x2f, 0x44, 0x4c, 0x4c, 0x3a, 0x31, 0x2e, 0x30,
    0x00, 0x21, 0x15, 0x03, 0x04, 0x00, 0x00, 0x00, 0x44, 0x4c, 0x4c, 0x00, 0x17, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x61, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x02, 0x00, 0x00, 0x00, 0x62, 0x00, 0x00, 0x00,
    0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// `corpus/golden/06-union.idl`'s `WithDefault` — the union the defect was
/// found on. Four members: `case 2: case 3:` is two, as the registry also
/// expands it.
const GOLDEN_06_WITH_DEFAULT: &[u8] = &[
    0x10, 0x00, 0x00, 0x00, 0xa4, 0x00, 0x00, 0x00, 0x01, 0x5d, 0x99, 0x07, 0x19, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x67, 0x63, 0x30, 0x36, 0x2f, 0x57, 0x69, 0x74, 0x68, 0x44, 0x65, 0x66,
    0x61, 0x75, 0x6c, 0x74, 0x3a, 0x31, 0x2e, 0x30, 0x00, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x00,
    0x57, 0x69, 0x74, 0x68, 0x44, 0x65, 0x66, 0x61, 0x75, 0x6c, 0x74, 0x00, 0x03, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00,
    0x6f, 0x6e, 0x65, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x0d, 0x00, 0x00, 0x00,
    0x74, 0x77, 0x6f, 0x5f, 0x6f, 0x72, 0x5f, 0x74, 0x68, 0x72, 0x65, 0x65, 0x00, 0x00, 0x00, 0x00,
    0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x0d, 0x00, 0x00, 0x00,
    0x74, 0x77, 0x6f, 0x5f, 0x6f, 0x72, 0x5f, 0x74, 0x68, 0x72, 0x65, 0x65, 0x00, 0x00, 0x00, 0x00,
    0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x06, 0x00, 0x00, 0x00,
    0x6f, 0x74, 0x68, 0x65, 0x72, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00,
];

/// `corpus/golden/29-labelled-default.idl`'s `Coded`: `case 1: long one;
/// case 2: default: string rest;`. omniidl makes **three** members of it —
/// `(1, one)`, `(2, rest)` and a default `rest` — and since 2026-08-19 so
/// does the registry; until then it kept `case 2: default:` as one case,
/// labelled 2, at `default_index`. See
/// [`a_labelled_default_is_one_member_per_label_default_included`] here for
/// the peer's side, and `orbweaver-registry/tests/union_shape_from_a_peer.rs`
/// for ours held equal to it.
const GOLDEN_29_CODED: &[u8] = &[
    0x10, 0x00, 0x00, 0x00, 0x74, 0x00, 0x00, 0x00, 0x01, 0x61, 0x99, 0x07, 0x13, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x67, 0x63, 0x32, 0x39, 0x2f, 0x43, 0x6f, 0x64, 0x65, 0x64, 0x3a, 0x31,
    0x2e, 0x30, 0x00, 0x03, 0x06, 0x00, 0x00, 0x00, 0x43, 0x6f, 0x64, 0x65, 0x64, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x04, 0x00, 0x00, 0x00, 0x6f, 0x6e, 0x65, 0x00, 0x03, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
    0x05, 0x00, 0x00, 0x00, 0x72, 0x65, 0x73, 0x74, 0x00, 0x00, 0x00, 0x00, 0x12, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x05, 0x00, 0x00, 0x00, 0x72, 0x65, 0x73, 0x74,
    0x00, 0x00, 0x00, 0x00, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// (what, bytes, the stream's byte order, expected members as
/// `(name, label)` with the default's label empty, expected default index)
type Recording = (&'static str, &'static [u8], Endian, Vec<(&'static str, Vec<u8>)>, i32);

fn recordings() -> Vec<Recording> {
    vec![
        ("long", LONG_DEFAULT, Endian::Little, vec![("a", vec![0, 0, 0, 1]), ("b", vec![])], 1),
        (
            "long, big-endian stream",
            LONG_DEFAULT_BIG_ENDIAN_STREAM,
            Endian::Big,
            vec![("a", vec![0, 0, 0, 1]), ("b", vec![])],
            1,
        ),
        ("short", SHORT_DEFAULT, Endian::Little, vec![("a", vec![0, 1]), ("b", vec![])], 1),
        ("boolean", BOOLEAN_DEFAULT, Endian::Little, vec![("yes", vec![1]), ("no", vec![])], 1),
        ("char", CHAR_DEFAULT, Endian::Little, vec![("a", vec![b'a']), ("b", vec![])], 1),
        (
            "enum",
            ENUM_DEFAULT,
            Endian::Little,
            vec![("warm", vec![0, 0, 0, 0]), ("named", vec![])],
            1,
        ),
        (
            "long long",
            LONG_LONG_DEFAULT,
            Endian::Little,
            vec![("a", vec![0, 0, 0, 0, 0, 0, 0, 1]), ("b", vec![])],
            1,
        ),
        (
            "golden 06 WithDefault",
            GOLDEN_06_WITH_DEFAULT,
            Endian::Little,
            vec![
                ("one", vec![0, 0, 0, 1]),
                ("two_or_three", vec![0, 0, 0, 2]),
                ("two_or_three", vec![0, 0, 0, 3]),
                ("other", vec![]),
            ],
            3,
        ),
        (
            "golden 29 Coded",
            GOLDEN_29_CODED,
            Endian::Little,
            vec![("one", vec![0, 0, 0, 1]), ("rest", vec![0, 0, 0, 2]), ("rest", vec![])],
            2,
        ),
    ]
}

fn cases_of(tc: &TypeCode) -> (Vec<(String, Vec<u8>)>, i32) {
    let TypeCode::Union { cases, default_index, .. } = tc else { panic!("not a union: {tc:?}") };
    (cases.iter().map(|c| (c.name.clone(), c.label.clone())).collect(), *default_index)
}

/// Every recording decodes, both byte orders of stream, and the default
/// member comes back with **no** label whatever omniORB put in the slot —
/// §9.3.5.1.4's "should be ignored", made a shape rather than a convention.
#[test]
fn a_defaulted_union_from_a_peer_reads_and_its_default_label_is_ignored() {
    let mut failures = Vec::new();
    for (what, bytes, endian, members, default_index) in recordings() {
        match decode(&mut Decoder::new(bytes, endian)) {
            Err(e) => failures.push(format!("{what}: does not decode: {e}")),
            Ok(tc) => {
                let want: Vec<(String, Vec<u8>)> =
                    members.into_iter().map(|(n, l)| (n.to_owned(), l)).collect();
                let got = cases_of(&tc);
                if got != (want.clone(), default_index) {
                    failures.push(format!(
                        "{what}: decoded {got:?}, wanted {want:?} default {default_index}"
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{} failure(s):\n  {}", failures.len(), failures.join("\n  "));
}

// ── a padding mask, derived from the layout ─────────────────────────────────
//
// Re-encoding must reproduce the peer's bytes, and the peer does not zero its
// padding (`01 b8 3f 03` after one flag, `01 d6 bd 07` after another — same
// union, two runs), so the comparison walks the TypeCode as Table 9.2 lays it
// out and skips what it finds to be padding. Listing offsets instead is how
// `union_labels_from_a_peer.rs`'s sibling script was green for a week against
// a fixture that padded differently.

struct Walk<'a> {
    buf: &'a [u8],
    base: usize,
    little: bool,
    pos: usize,
    pad: Vec<usize>,
    /// Absolute offsets of the default member's label slot(s), the bytes
    /// §9.3.5.1.4 says a reader ignores — a nested union could contribute
    /// another, so a list.
    default_slots: Vec<std::ops::Range<usize>>,
}

impl Walk<'_> {
    fn align(&mut self, n: usize) {
        while self.pos % n != 0 {
            self.pad.push(self.base + self.pos);
            self.pos += 1;
        }
    }
    fn u(&mut self, n: usize) -> u64 {
        self.align(n);
        let b = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        let mut v = 0u64;
        if self.little {
            for x in b.iter().rev() {
                v = (v << 8) | u64::from(*x);
            }
        } else {
            for x in b {
                v = (v << 8) | u64::from(*x);
            }
        }
        v
    }
    fn string(&mut self) {
        let n = self.u(4) as usize;
        self.pos += n;
    }
    /// Walks one TypeCode, returns its kind.
    fn typecode(&mut self) -> u64 {
        let kind = self.u(4);
        match kind {
            0..=13 | 23..=26 => {}
            18 | 27 => {
                self.u(4);
            }
            15 | 16 | 17 | 19 | 21 | 22 => {
                let n = self.u(4) as usize;
                let start = self.pos;
                let mut inner = Walk {
                    buf: &self.buf[start..start + n],
                    base: self.base + start,
                    little: self.little,
                    pos: 0,
                    pad: Vec::new(),
                    default_slots: Vec::new(),
                };
                inner.encapsulated(kind);
                self.pad.extend(inner.pad);
                self.default_slots.extend(inner.default_slots);
                self.pos = start + n;
            }
            other => panic!("TypeCode kind {other} is not walked here"),
        }
        kind
    }
    fn encapsulated(&mut self, kind: u64) {
        let flag = self.buf[self.pos];
        self.pos += 1;
        self.little = flag == 1;
        self.align(4);
        if kind == 19 {
            self.typecode();
            self.u(4);
            return;
        }
        self.string();
        self.string();
        match kind {
            21 => {
                self.typecode();
            }
            17 => {
                for _ in 0..self.u(4) {
                    self.string();
                }
            }
            15 | 22 => {
                for _ in 0..self.u(4) {
                    self.string();
                    self.typecode();
                }
            }
            _ => {
                let disc = self.typecode();
                let width = match disc {
                    8 | 9 => 1,
                    2 | 4 => 2,
                    23 | 24 => 8,
                    3 | 5 | 17 => 4,
                    other => panic!("discriminator kind {other} has no label width here"),
                };
                let default_index = self.u(4) as i32 as i64;
                for i in 0..self.u(4) as i64 {
                    self.align(width);
                    if i == default_index {
                        self.default_slots.push(self.base + self.pos..self.base + self.pos + width);
                    }
                    self.u(width);
                    self.string();
                    self.typecode();
                }
            }
        }
    }
}

/// Offsets in `buf` (a TypeCode from its kind) that are alignment padding,
/// and the default member's label slot(s).
fn layout(buf: &[u8], endian: Endian) -> (Vec<usize>, Vec<std::ops::Range<usize>>) {
    let mut w = Walk {
        buf,
        base: 0,
        little: endian == Endian::Little,
        pos: 0,
        pad: Vec::new(),
        default_slots: Vec::new(),
    };
    w.typecode();
    assert_eq!(w.pos, buf.len(), "walked {} of {} bytes", w.pos, buf.len());
    (w.pad, w.default_slots)
}

/// The direction our own round trip could never check: re-encoding what the
/// peer described reproduces the peer's bytes everywhere but the padding and
/// the default member's label slot — which must be there, at the
/// discriminator's width, and which we fill with zeros where omniORB writes
/// a value no other case uses (see the module doc for why zeros).
///
/// The encoder is handed the registry's shape — no label on the default —
/// explicitly, not by trusting the decoder to have blanked it: a decoder
/// that kept the peer's value and an encoder that wrote it back would pass a
/// byte comparison without either writing a label of its own. That pair was
/// the old code, and it was green on the peer's bytes.
///
/// The big-endian-stream recording is left out here, on purpose: omniORB
/// writes the encapsulation little-endian inside a big-endian stream and we
/// write it in the stream's order, both legal (the encapsulation carries its
/// own flag), so the bytes differ by design and only the decoded shape can be
/// compared — which the other tests do.
#[test]
fn re_encoding_reproduces_what_the_peer_wrote() {
    let mut failures = Vec::new();
    for (what, original, endian, _, _) in
        recordings().into_iter().filter(|(_, _, endian, _, _)| *endian == Endian::Little)
    {
        let mut tc = decode(&mut Decoder::new(original, endian)).expect("decode");
        if let TypeCode::Union { cases, default_index, .. } = &mut tc {
            cases[*default_index as usize].label.clear();
        }
        let mut e = Encoder::new(endian);
        encode(&mut e, &tc).expect("encode");
        let ours = e.finish().expect("finish");
        if ours.len() != original.len() {
            failures.push(format!(
                "{what}: ours is {} bytes, the peer's {}",
                ours.len(),
                original.len()
            ));
            continue;
        }
        let (pad, slots) = layout(original, endian);
        assert_eq!(slots.len(), 1, "{what}: one union, one default slot");
        let slot = &slots[0];
        assert!(
            ours[slot.clone()].iter().all(|b| *b == 0),
            "{what}: our default label at {slot:?} is not zeros: {:02x?}",
            &ours[slot.clone()]
        );
        // The slot must sit where the peer's does and be as wide, or the
        // bytes after it could not line up — which the comparison below is
        // about to require of them.
        let diff: Vec<String> = ours
            .iter()
            .zip(original)
            .enumerate()
            .filter(|(i, (a, b))| a != b && !pad.contains(i) && !slot.contains(i))
            .map(|(i, (a, b))| format!("byte {i}: ours {a:#04x}, peer's {b:#04x}"))
            .collect();
        if !diff.is_empty() {
            failures.push(format!("{what}: {}", diff.join(", ")));
        }
    }
    assert!(failures.is_empty(), "{} failure(s):\n  {}", failures.len(), failures.join("\n  "));
}

/// The old encoder wrote zero label bytes for a bare default and the decoder
/// read the discriminator's width — an encoding this test file's other tests
/// would catch on the peer's bytes but not on ours alone. Pinned separately:
/// what omniORB described, re-encoded by us, decodes back to the same union
/// in both byte orders. Vacuous under a self-consistent codec; not vacuous
/// after `re_encoding_reproduces_what_the_peer_wrote` has held.
#[test]
fn a_defaulted_union_survives_either_byte_order() {
    for (what, original, endian, _, _) in recordings() {
        let tc = decode(&mut Decoder::new(original, endian)).expect("decode");
        for out in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(out);
            encode(&mut e, &tc).expect("encode");
            let bytes = e.finish().expect("finish");
            let back = decode(&mut Decoder::new(&bytes, out)).expect("decode ours");
            assert_eq!(cases_of(&back), cases_of(&tc), "{what} via {out:?}");
        }
    }
}

/// How many members `case 2: default: string rest;` is, on the peer's side:
/// three — `(1, one)`, `(2, rest)`, default `rest` — one per label with the
/// `default` a member of its own, where it was written, which is also how
/// `case 2: case 3:` expands. This test pins the *peer's* count and the fact
/// that the label 2 survives our decoder next to the ignored default one.
///
/// Until 2026-08-19 this test was named for the disagreement: the registry
/// kept two members, the `default` folded onto the labelled case, and
/// `default_index` on it. Measured that day: omniORB decoded the two-member
/// `Coded` and selected `one` for 1, `rest` for 2 and by default for 99 and
/// `i32::MIN` — not an interoperability failure at the value level, but a
/// different `member_count` and `default_index` on the wire and a `TypeCode`
/// no peer's IDL-derived one equalled. The registry now derives the peer's
/// list (source order, the default a labelless member of its own), and
/// `orbweaver-registry/tests/union_shape_from_a_peer.rs` holds it `==` to
/// what these bytes decode to, for all four corpus unions with a default,
/// both stream orders. This crate cannot see the registry, so the peer's
/// half of the fact stays pinned here.
#[test]
fn a_labelled_default_is_one_member_per_label_default_included() {
    let tc = decode(&mut Decoder::new(GOLDEN_29_CODED, Endian::Little)).expect("decode");
    let (cases, default_index) = cases_of(&tc);
    assert_eq!(cases.len(), 3, "omniidl writes one member per label, default included");
    assert_eq!(default_index, 2, "the default member sits where `default:` was written: last");
    assert_eq!(cases[1], ("rest".to_owned(), vec![0, 0, 0, 2]), "the label 2 is a real case");
    assert_eq!(cases[2], ("rest".to_owned(), vec![]), "the default is the labelless one");
}

// ── R18: a conformant third peer writes a NON-ZERO default label ─────────────
//
// Everything above is the intersection of two peers: omniORB writes an unused
// value and ignores what it reads, JacORB writes zeros and reads one octet
// that must be zero. §9.3.5.1.4 permits a third peer to write **any valid
// value of the discriminator type** in the slot — the value that collides
// with a real case included — and the two fixtures available cannot produce
// that half. omniORB's own captures above already carry non-zero labels
// (`00 00 00 80`, `00 80`, `ff`, `01 00 00 00`, `..80`), so the recordings
// prove "omniORB's value is ignored". The tests below prove "any value is":
// hand-built encapsulations, one per discriminator kind already recorded
// here, in both stream byte orders, with the slot set to the type's maximum,
// its minimum, the label of a real case, and a bit pattern that is not a
// valid value at all — and the decoded TypeCode must be **structurally
// equal** to the one the zero label yields, which is also the one omniORB's
// value yields. Nothing about the peer's bytes is edited; the base shape is
// each recording decoded, and the builder is checked against our own encoder
// on the zero label first, so a builder that put the slot in the wrong place
// would fail its own sanity check before it could pass this one.
//
// (`orbweaver-dynamic/tests/union_value_after_a_nonzero_default_label.rs`
// takes the same bytes one step further: a *value* of each union decodes and
// re-encodes under the TypeCode read from them, and a discriminator equal to
// the non-zero default label still selects the real case, not the default.)

/// The union TypeCode encapsulation §9.3.5.1.4 describes, built by hand from
/// a decoded shape, with `default_label` in the default member's slot at the
/// discriminator's width — in the stream's byte order, which is where we put
/// an encapsulation's flag.
fn hand_built(tc: &TypeCode, endian: Endian, default_label: u64) -> Vec<u8> {
    let TypeCode::Union { id, name, discriminator, default_index, cases } = tc else {
        panic!("not a union: {tc:?}")
    };
    let width = match discriminator.as_ref() {
        TypeCode::Boolean | TypeCode::Char | TypeCode::Octet => 1,
        TypeCode::Short | TypeCode::UShort => 2,
        TypeCode::LongLong | TypeCode::ULongLong => 8,
        _ => 4,
    };
    let in_order = |be: &[u8]| -> Vec<u8> {
        match endian {
            Endian::Big => be.to_vec(),
            Endian::Little => be.iter().rev().copied().collect(),
        }
    };
    let mut inner = Encoder::encapsulation(endian);
    inner.put_str(id);
    inner.put_str(name);
    encode(&mut inner, discriminator).expect("discriminator");
    inner.put_i32(*default_index);
    inner.put_u32(cases.len() as u32);
    for (i, c) in cases.iter().enumerate() {
        inner.align_to(width.min(8));
        let label = if *default_index >= 0 && i == *default_index as usize {
            in_order(&default_label.to_be_bytes()[8 - width..])
        } else {
            assert_eq!(c.label.len(), width, "{name}.{}: label width", c.name);
            in_order(&c.label)
        };
        inner.put_bytes(&label);
        inner.put_str(&c.name);
        encode(&mut inner, &c.tc).expect("member");
    }
    let mut outer = Encoder::new(endian);
    outer.put_u32(16); // tk_union
    outer.put_encapsulation(inner);
    outer.finish().expect("finish")
}

/// (what, the recording, non-zero labels a conformant peer could write:
/// (name, value))
type ThirdPeer = (&'static str, &'static [u8], Vec<(&'static str, u64)>);

fn third_peer_labels() -> Vec<ThirdPeer> {
    vec![
        (
            "long",
            LONG_DEFAULT,
            vec![
                ("i32::MAX", i32::MAX as u32 as u64),
                ("i32::MIN (omniORB's)", i32::MIN as u32 as u64),
                ("-1", u32::MAX as u64),
                ("1, the label of `case 1:`", 1),
            ],
        ),
        (
            "short",
            SHORT_DEFAULT,
            vec![
                ("i16::MAX", i16::MAX as u16 as u64),
                ("i16::MIN (omniORB's)", i16::MIN as u16 as u64),
                ("-1", u16::MAX as u64),
                ("1, the label of `case 1:`", 1),
            ],
        ),
        (
            "long long",
            LONG_LONG_DEFAULT,
            vec![
                ("i64::MAX", i64::MAX as u64),
                ("i64::MIN (omniORB's)", i64::MIN as u64),
                ("-1", u64::MAX),
                ("1, the label of `case 1:`", 1),
            ],
        ),
        (
            "boolean",
            BOOLEAN_DEFAULT,
            vec![("TRUE, the label of `case TRUE:`", 1), ("0xff, not a boolean at all", 0xff)],
        ),
        (
            "char",
            CHAR_DEFAULT,
            vec![
                ("'\\377' (omniORB's)", 0xff),
                ("'\\177'", 0x7f),
                ("'a', the label of `case 'a':`", u64::from(b'a')),
            ],
        ),
        (
            "enum",
            ENUM_DEFAULT,
            vec![
                ("GREEN, unused (omniORB's)", 1),
                ("BLUE, unused", 2),
                ("3, past the last enumerator", 3),
                ("0xffffffff, not an ordinal at all", u32::MAX as u64),
            ],
        ),
    ]
}

/// The builder reproduces our own encoder on the zero label, byte for byte,
/// in both orders — so a slot it wrote anywhere else would show here first.
#[test]
fn the_hand_builder_agrees_with_our_encoder_on_the_zero_label() {
    for (what, recording, _) in third_peer_labels() {
        let base = decode(&mut Decoder::new(recording, Endian::Little)).expect("decode");
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            encode(&mut e, &base).expect("encode");
            let ours = e.finish().expect("finish");
            assert_eq!(hand_built(&base, endian, 0), ours, "{what} {endian:?}");
        }
    }
}

/// Any value in the default member's label slot — maximum, minimum, a real
/// case's own label, a pattern that is not a value of the type — decodes to
/// the TypeCode the zero label decodes to: default member labelless, the
/// same `default_index`, every real case with its own label. Both stream
/// orders. Twenty-one labels × two orders = forty-two encapsulations, and
/// each must also differ from the zero-label bytes, or a builder that
/// silently wrote zeros would pass this by tautology.
#[test]
fn a_third_peers_non_zero_default_label_is_ignored_whatever_it_is() {
    let mut failures = Vec::new();
    let mut measured = 0;
    for (what, recording, labels) in third_peer_labels() {
        let base = decode(&mut Decoder::new(recording, Endian::Little)).expect("decode");
        for endian in [Endian::Big, Endian::Little] {
            let zeros = hand_built(&base, endian, 0);
            for (label_name, value) in &labels {
                let bytes = hand_built(&base, endian, *value);
                assert_ne!(
                    bytes, zeros,
                    "{what} {endian:?} label {label_name}: builder wrote zeros"
                );
                measured += 1;
                match decode(&mut Decoder::new(&bytes, endian)) {
                    Err(e) => failures.push(format!("{what} {endian:?} label {label_name}: {e}")),
                    Ok(tc) if tc != base => failures.push(format!(
                        "{what} {endian:?} label {label_name}: decoded {:?}, wanted {:?}",
                        cases_of(&tc),
                        cases_of(&base)
                    )),
                    Ok(tc) => {
                        // And what we then write for it is what we always
                        // write: zeros in the slot, decodable in either order.
                        for out in [Endian::Big, Endian::Little] {
                            let mut e = Encoder::new(out);
                            encode(&mut e, &tc).expect("encode");
                            let ours = e.finish().expect("finish");
                            assert_eq!(ours, hand_built(&base, out, 0), "{what} via {out:?}");
                            let back = decode(&mut Decoder::new(&ours, out)).expect("decode ours");
                            assert_eq!(back, base, "{what} {label_name} via {out:?}");
                        }
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{} failure(s):\n  {}", failures.len(), failures.join("\n  "));
    assert_eq!(measured, 42, "the matrix is six kinds × their labels × two orders");
}
