//! What a conformant peer writes for a `valuetype` and an `abstract interface`
//! — the two constructs `docs/PLAN.md` §4.4 defers, recorded as bytes rather
//! than read off a table.
//!
//! Every byte sequence below was captured from omniORB 4.3.4 (omniORBpy,
//! `cdrMarshal(CORBA._tc_TypeCode, tc, endian)`, the TypeCode built by
//! `omniidl -bpython` from `corpus/golden/20-deferred-valuetype.idl` and
//! `corpus/golden/deferred-reach.idl`) on 2026-08-20 by
//! `spikes/valuetype_capture.py`, which retakes them from the live fixture and
//! compares outside the padding — clause (b) of the licensing boundary, the
//! output of a program we ran, describing IDL we wrote.
//!
//! # What the peer gives them, measured
//!
//! **A `valuetype` is `TCKind` 29** (`tk_value`), and its parameter list is
//! not an object reference's: repository id, name, a `short` ValueModifier, a
//! `TypeCode` for the concrete base, a `ulong` count, and then per member a
//! name, a `TypeCode` and a `short` Visibility (CORBA 3.4 Part 2, Table 9.2).
//! `gc20::Money` is `VM_NONE` (0), no base, three public members. Absence of a
//! base is **`tk_null`** — kind 0 — which is what omniORB writes and what we
//! write; `gc20::Named : Money` carries the whole of `Money`'s `tk_value`
//! inline where `Money` has a `tk_null`.
//!
//! **An `abstract interface` is `TCKind` 32** (`tk_abstract_interface`), and
//! its parameter list is exactly `tk_objref`'s: repository id and name,
//! nothing else. That identity is why recording one as the other was invisible
//! — the *TypeCode* bytes differ only in the kind ordinal, while the bytes of
//! a *value* differ completely — and it is what this file exists to make
//! impossible to reintroduce.
//!
//! 30 (`tk_value_box`), 31 (`tk_native`) and 33 (`tk_local_interface`) are not
//! here and are not decoded: nothing in this project has been shown one by a
//! peer, and an ordinal nobody measured is a guess. The decoder answers
//! "unknown or unsupported TCKind" for them, which names itself.
//!
//! # What this file does *not* claim
//!
//! That we can marshal a value. §4.4 defers the value's wire form and this
//! batch did not implement it: describing a type is not marshalling one. The
//! last test here is the other half of the honesty — the dynamic path refuses
//! to start a value of one of these types, naming §4.4, and both emitters skip
//! them (`crates/orbweaver-gen/tests/deferred_wire_agreement.rs`). Reading the
//! peer's *description* is what a catalogue, an `any` and an Interface
//! Repository need, and it is all that is claimed.
//!
//! *valuetype은 29, abstract interface는 32 — 표에서 읽은 것이 아니라 피어의
//! 바이트에서 측정했다. 타입을 기술하는 것과 값을 마샬하는 것은 다른 주장이며,
//! 후자는 여전히 §4.4로 거부된다.*

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_giop::typecode::{Member, TypeCode, ValueMember, decode, encode};

/// `gc20._tc_Money`, TCKind 29, little-endian stream -> valuetype_typecode_from_a_peer.rs
const GC20_MONEY: &[u8] = &[
    0x1d, 0x00, 0x00, 0x00, 0x72, 0x00, 0x00, 0x00, 0x01, 0xf8, 0x36, 0x01, 0x13, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x67, 0x63, 0x32, 0x30, 0x2f, 0x4d, 0x6f, 0x6e, 0x65, 0x79, 0x3a, 0x31,
    0x2e, 0x30, 0x00, 0x05, 0x06, 0x00, 0x00, 0x00, 0x4d, 0x6f, 0x6e, 0x65, 0x79, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x63, 0x75, 0x72, 0x72,
    0x65, 0x6e, 0x63, 0x79, 0x00, 0x00, 0x00, 0x00, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x75, 0x6e, 0x69, 0x74, 0x73, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x6e, 0x61, 0x6e, 0x6f,
    0x73, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00,
];
/// `gc20._tc_Money`, TCKind 29, big-endian stream -> valuetype_typecode_from_a_peer.rs
const GC20_MONEY_BIG_ENDIAN_STREAM: &[u8] = &[
    0x00, 0x00, 0x00, 0x1d, 0x00, 0x00, 0x00, 0x72, 0x01, 0x53, 0x99, 0x05, 0x13, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x67, 0x63, 0x32, 0x30, 0x2f, 0x4d, 0x6f, 0x6e, 0x65, 0x79, 0x3a, 0x31,
    0x2e, 0x30, 0x00, 0x01, 0x06, 0x00, 0x00, 0x00, 0x4d, 0x6f, 0x6e, 0x65, 0x79, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x63, 0x75, 0x72, 0x72,
    0x65, 0x6e, 0x63, 0x79, 0x00, 0x00, 0x00, 0x00, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x75, 0x6e, 0x69, 0x74, 0x73, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x6e, 0x61, 0x6e, 0x6f,
    0x73, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00,
];
/// `gc20._tc_Named`, TCKind 29, little-endian stream -> valuetype_typecode_from_a_peer.rs
const GC20_NAMED: &[u8] = &[
    0x1d, 0x00, 0x00, 0x00, 0xbe, 0x00, 0x00, 0x00, 0x01, 0x1b, 0x70, 0x05, 0x13, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x67, 0x63, 0x32, 0x30, 0x2f, 0x4e, 0x61, 0x6d, 0x65, 0x64, 0x3a, 0x31,
    0x2e, 0x30, 0x00, 0x01, 0x06, 0x00, 0x00, 0x00, 0x4e, 0x61, 0x6d, 0x65, 0x64, 0x00, 0x00, 0x00,
    0x1d, 0x00, 0x00, 0x00, 0x72, 0x00, 0x00, 0x00, 0x01, 0x98, 0x93, 0x6f, 0x13, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x67, 0x63, 0x32, 0x30, 0x2f, 0x4d, 0x6f, 0x6e, 0x65, 0x79, 0x3a, 0x31,
    0x2e, 0x30, 0x00, 0x05, 0x06, 0x00, 0x00, 0x00, 0x4d, 0x6f, 0x6e, 0x65, 0x79, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x63, 0x75, 0x72, 0x72,
    0x65, 0x6e, 0x63, 0x79, 0x00, 0x00, 0x00, 0x00, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x75, 0x6e, 0x69, 0x74, 0x73, 0x00, 0x00, 0x00,
    0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x6e, 0x61, 0x6e, 0x6f,
    0x73, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    0x06, 0x00, 0x00, 0x00, 0x6c, 0x61, 0x62, 0x65, 0x6c, 0x00, 0x00, 0x00, 0x12, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
];
/// `gc20._tc_Describable`, TCKind 32, little-endian stream -> valuetype_typecode_from_a_peer.rs
const GC20_DESCRIBABLE: &[u8] = &[
    0x20, 0x00, 0x00, 0x00, 0x34, 0x00, 0x00, 0x00, 0x01, 0x03, 0x97, 0x05, 0x19, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x67, 0x63, 0x32, 0x30, 0x2f, 0x44, 0x65, 0x73, 0x63, 0x72, 0x69, 0x62,
    0x61, 0x62, 0x6c, 0x65, 0x3a, 0x31, 0x2e, 0x30, 0x00, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x00,
    0x44, 0x65, 0x73, 0x63, 0x72, 0x69, 0x62, 0x61, 0x62, 0x6c, 0x65, 0x00,
];
/// `gc20._tc_Describable`, TCKind 32, big-endian stream -> valuetype_typecode_from_a_peer.rs
const GC20_DESCRIBABLE_BIG_ENDIAN_STREAM: &[u8] = &[
    0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x34, 0x01, 0x03, 0x97, 0x05, 0x19, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x67, 0x63, 0x32, 0x30, 0x2f, 0x44, 0x65, 0x73, 0x63, 0x72, 0x69, 0x62,
    0x61, 0x62, 0x6c, 0x65, 0x3a, 0x31, 0x2e, 0x30, 0x00, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x00,
    0x44, 0x65, 0x73, 0x63, 0x72, 0x69, 0x62, 0x61, 0x62, 0x6c, 0x65, 0x00,
];
/// `gcdr._tc_Memo`, TCKind 15, little-endian stream -> valuetype_typecode_from_a_peer.rs
const GCDR_MEMO: &[u8] = &[
    0x0f, 0x00, 0x00, 0x00, 0x86, 0x00, 0x00, 0x00, 0x01, 0x99, 0x9a, 0x05, 0x12, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x67, 0x63, 0x64, 0x72, 0x2f, 0x4d, 0x65, 0x6d, 0x6f, 0x3a, 0x31, 0x2e,
    0x30, 0x00, 0x0c, 0x01, 0x05, 0x00, 0x00, 0x00, 0x4d, 0x65, 0x6d, 0x6f, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x62, 0x6f, 0x64, 0x79, 0x00, 0x00, 0x00, 0x00,
    0x1d, 0x00, 0x00, 0x00, 0x46, 0x00, 0x00, 0x00, 0x01, 0x98, 0x93, 0x6f, 0x12, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x67, 0x63, 0x64, 0x72, 0x2f, 0x4e, 0x6f, 0x74, 0x65, 0x3a, 0x31, 0x2e,
    0x30, 0x00, 0xa7, 0x05, 0x05, 0x00, 0x00, 0x00, 0x4e, 0x6f, 0x74, 0x65, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x74, 0x65, 0x78, 0x74,
    0x00, 0x00, 0x00, 0x00, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
];
/// `gcdr._tc_Tagged`, TCKind 15, little-endian stream -> valuetype_typecode_from_a_peer.rs
const GCDR_TAGGED: &[u8] = &[
    0x0f, 0x00, 0x00, 0x00, 0x74, 0x00, 0x00, 0x00, 0x01, 0x5f, 0x99, 0x05, 0x14, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x67, 0x63, 0x64, 0x72, 0x2f, 0x54, 0x61, 0x67, 0x67, 0x65, 0x64, 0x3a,
    0x31, 0x2e, 0x30, 0x00, 0x07, 0x00, 0x00, 0x00, 0x54, 0x61, 0x67, 0x67, 0x65, 0x64, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x73, 0x75, 0x62, 0x6a, 0x65, 0x63, 0x74, 0x00,
    0x20, 0x00, 0x00, 0x00, 0x34, 0x00, 0x00, 0x00, 0x01, 0x98, 0x93, 0x6f, 0x19, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x67, 0x63, 0x64, 0x72, 0x2f, 0x44, 0x65, 0x73, 0x63, 0x72, 0x69, 0x62,
    0x61, 0x62, 0x6c, 0x65, 0x3a, 0x31, 0x2e, 0x30, 0x00, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x00,
    0x44, 0x65, 0x73, 0x63, 0x72, 0x69, 0x62, 0x61, 0x62, 0x6c, 0x65, 0x00,
];

// ── what each recording must decode to ──────────────────────────────────────

fn money() -> TypeCode {
    TypeCode::Value {
        id: "IDL:gc20/Money:1.0".into(),
        name: "Money".into(),
        modifier: 0,
        base: None,
        members: vec![
            ValueMember { name: "currency".into(), tc: TypeCode::String(0), visibility: 1 },
            ValueMember { name: "units".into(), tc: TypeCode::Long, visibility: 1 },
            ValueMember { name: "nanos".into(), tc: TypeCode::Long, visibility: 1 },
        ],
    }
}

fn named() -> TypeCode {
    TypeCode::Value {
        id: "IDL:gc20/Named:1.0".into(),
        name: "Named".into(),
        modifier: 0,
        base: Some(Box::new(money())),
        members: vec![ValueMember { name: "label".into(), tc: TypeCode::String(0), visibility: 1 }],
    }
}

fn describable() -> TypeCode {
    TypeCode::AbstractInterface {
        id: "IDL:gc20/Describable:1.0".into(),
        name: "Describable".into(),
    }
}

fn note() -> TypeCode {
    TypeCode::Value {
        id: "IDL:gcdr/Note:1.0".into(),
        name: "Note".into(),
        modifier: 0,
        base: None,
        members: vec![ValueMember { name: "text".into(), tc: TypeCode::String(0), visibility: 1 }],
    }
}

fn memo() -> TypeCode {
    TypeCode::Struct {
        id: "IDL:gcdr/Memo:1.0".into(),
        name: "Memo".into(),
        members: vec![Member { name: "body".into(), tc: note() }],
    }
}

fn tagged() -> TypeCode {
    TypeCode::Struct {
        id: "IDL:gcdr/Tagged:1.0".into(),
        name: "Tagged".into(),
        members: vec![Member {
            name: "subject".into(),
            tc: TypeCode::AbstractInterface {
                id: "IDL:gcdr/Describable:1.0".into(),
                name: "Describable".into(),
            },
        }],
    }
}

/// (what, bytes, the stream's byte order, the TypeCode it describes).
type Recording = (&'static str, &'static [u8], Endian, TypeCode);

fn recordings() -> Vec<Recording> {
    vec![
        ("gc20::Money", GC20_MONEY, Endian::Little, money()),
        ("gc20::Money, big-endian stream", GC20_MONEY_BIG_ENDIAN_STREAM, Endian::Big, money()),
        ("gc20::Named", GC20_NAMED, Endian::Little, named()),
        ("gc20::Describable", GC20_DESCRIBABLE, Endian::Little, describable()),
        (
            "gc20::Describable, big-endian stream",
            GC20_DESCRIBABLE_BIG_ENDIAN_STREAM,
            Endian::Big,
            describable(),
        ),
        ("gcdr::Memo", GCDR_MEMO, Endian::Little, memo()),
        ("gcdr::Tagged", GCDR_TAGGED, Endian::Little, tagged()),
    ]
}

/// Every recording decodes to the type it describes, in both stream orders.
///
/// The big-endian recordings are the ones a decoder gets wrong: omniORB writes
/// the encapsulation **little-endian inside a big-endian stream** (the flag at
/// byte 8 is `01` in both), so a decoder that carried the stream's order into
/// the body would read every length and every kind reversed.
#[test]
fn a_valuetype_and_an_abstract_interface_from_a_peer_read() {
    let mut failures = Vec::new();
    for (what, bytes, endian, want) in recordings() {
        match decode(&mut Decoder::new(bytes, endian)) {
            Err(e) => failures.push(format!("{what}: does not decode: {e}")),
            Ok(got) if got != want => {
                failures.push(format!("{what}: decoded {got:?}\n    wanted {want:?}"))
            }
            Ok(_) => {}
        }
    }
    assert!(failures.is_empty(), "{} failure(s):\n  {}", failures.len(), failures.join("\n  "));
}

/// The kind ordinals themselves, asserted separately from the shapes.
///
/// A shape comparison would pass if both ends agreed on the wrong ordinal, and
/// both ends here are ours. 29 and 32 are the peer's numbers and the first
/// bytes of the recordings are where they live.
#[test]
fn the_peer_writes_29_for_a_valuetype_and_32_for_an_abstract_interface() {
    assert_eq!(GC20_MONEY[0], 29, "tk_value");
    assert_eq!(GC20_NAMED[0], 29, "tk_value with a concrete base");
    assert_eq!(GC20_DESCRIBABLE[0], 32, "tk_abstract_interface");
    // Big-endian stream: the kind is the *last* byte of the first word.
    assert_eq!(GC20_MONEY_BIG_ENDIAN_STREAM[3], 29);
    assert_eq!(GC20_DESCRIBABLE_BIG_ENDIAN_STREAM[3], 32);
    // And the concrete base's absence is tk_null (0), not tk_void (1): the
    // `00 00 00 00` at the base slot of `Money`, four bytes after the
    // ValueModifier short.
    // Byte 48: 8 (kind, length) + 4 (flag, padding) + 4 + 19 (the id and its
    // one padding byte, 4 + 19 + 1) ... + 4 + 6 + 2 (the name) + 2 (the
    // ValueModifier short) — the four bytes the base TypeCode's kind occupies.
    assert_eq!(&GC20_MONEY[48..52], &[0, 0, 0, 0], "no concrete base is tk_null, not tk_void");
}

/// The direction our own round trip could never check: re-encoding what the
/// peer described reproduces the peer's bytes, everywhere but the padding.
///
/// The peer does not zero its padding — the three bytes after an
/// encapsulation's flag came back `01 38 58 01` in one run and `01 13 0b 01`
/// in the next, same TypeCode — so the comparison walks the layout and skips
/// what it finds to be padding rather than listing offsets. Listing them is
/// how the sibling union script was green for a week against a fixture that
/// padded differently.
///
/// The big-endian recordings are left out on purpose: omniORB writes the
/// encapsulation little-endian inside a big-endian stream and we write it in
/// the stream's order, both legal (the encapsulation carries its own flag), so
/// the bytes differ by design and only the decoded shape can be compared —
/// which the test above does.
#[test]
fn re_encoding_reproduces_what_the_peer_wrote() {
    let mut failures = Vec::new();
    for (what, original, endian, tc) in
        recordings().into_iter().filter(|(_, _, e, _)| *e == Endian::Little)
    {
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
        let pad = padding(original);
        let diff: Vec<usize> =
            (0..ours.len()).filter(|i| ours[*i] != original[*i] && !pad.contains(i)).collect();
        if !diff.is_empty() {
            failures.push(format!(
                "{what}: differs from the peer at {:?} (ours {:02x?}, theirs {:02x?})",
                &diff[..diff.len().min(8)],
                diff.iter().take(8).map(|i| ours[*i]).collect::<Vec<_>>(),
                diff.iter().take(8).map(|i| original[*i]).collect::<Vec<_>>(),
            ));
        }
    }
    assert!(failures.is_empty(), "{} failure(s):\n  {}", failures.len(), failures.join("\n  "));
}

/// Describing is not marshalling. The TypeCode above round-trips; a *value* of
/// that type is still refused, with §4.4 named — the claim this file makes is
/// exactly as wide as what was implemented.
///
/// The refusal lives in `orbweaver-dynamic` and is exercised there
/// (`default_value` over a `TypeCode::Value`); what is asserted here is the
/// half `orbweaver-giop` owns: an encoder handed a `Value` writes a TypeCode
/// and there is no `Value` variant it could write an instance from, because
/// the codec in this crate marshals type *descriptions* and nothing else.
#[test]
fn the_typecode_encodes_and_carries_no_instance_with_it() {
    let mut e = Encoder::new(Endian::Little);
    encode(&mut e, &money()).expect("a valuetype's TypeCode encodes");
    let bytes = e.finish().expect("finish");
    // Everything in the buffer is description: the id, the name, the member
    // names and their TypeCodes. Nothing in it is a value tag (0x7fffff00..)
    // and nothing in it is a member's value.
    assert_eq!(decode(&mut Decoder::new(&bytes, Endian::Little)).expect("decode"), money());
    assert!(
        !bytes.windows(4).any(|w| w == [0x00, 0xff, 0xff, 0x7f] || w == [0x7f, 0xff, 0xff, 0x00]),
        "a value tag has no business in a TypeCode"
    );
}

// ── a padding mask, derived from the layout ─────────────────────────────────
//
// The same walk `spikes/valuetype_capture.py` runs, in Rust, over the kinds
// these recordings contain. An unwalked kind panics rather than contributing
// "no padding": a mask that quietly covers nothing turns this test green.

fn padding(buf: &[u8]) -> Vec<usize> {
    // Only ever called for the little-endian recordings; the big-endian ones
    // are compared by shape, for the reason the test above states.
    let mut w = Walk { buf, pad: Vec::new() };
    let end = w.typecode(0, 0, true);
    assert_eq!(end, buf.len(), "walked {end} of {} bytes", buf.len());
    w.pad
}

struct Walk<'a> {
    buf: &'a [u8],
    pad: Vec<usize>,
}

impl Walk<'_> {
    fn word(&self, pos: usize, little: bool) -> usize {
        let b: [u8; 4] = self.buf[pos..pos + 4].try_into().expect("four bytes");
        if little { u32::from_le_bytes(b) as usize } else { u32::from_be_bytes(b) as usize }
    }

    fn align(&mut self, mut pos: usize, base: usize, n: usize) -> usize {
        while (pos - base) % n != 0 {
            self.pad.push(pos);
            pos += 1;
        }
        pos
    }

    fn string(&mut self, pos: usize, base: usize, little: bool) -> usize {
        let pos = self.align(pos, base, 4);
        pos + 4 + self.word(pos, little)
    }

    fn typecode(&mut self, pos: usize, base: usize, little: bool) -> usize {
        let pos = self.align(pos, base, 4);
        let kind = self.word(pos, little);
        let pos = pos + 4;
        match kind {
            // Empty parameter list.
            0..=13 | 23..=26 => pos,
            // A bound, inline, no encapsulation.
            18 | 27 => pos + 4,
            // Complex: an encapsulation of its own, with its own byte order.
            14 | 15 | 22 | 29 | 32 => {
                let pos = self.align(pos, base, 4);
                let len = self.word(pos, little);
                let inner_base = pos + 4;
                let inner_little = self.buf[inner_base] == 1;
                let end = inner_base + len;
                let at = self.body(inner_base + 1, inner_base, inner_little, kind);
                assert_eq!(at, end, "walked to {at}, the encapsulation ends at {end}");
                end
            }
            other => panic!("no walk for TCKind {other}"),
        }
    }

    fn body(&mut self, pos: usize, base: usize, little: bool, kind: usize) -> usize {
        let pos = self.string(pos, base, little); // repository id
        let mut pos = self.string(pos, base, little); // name
        match kind {
            14 | 32 => pos, // tk_objref, tk_abstract_interface: nothing more
            15 | 22 => {
                pos = self.align(pos, base, 4);
                let n = self.word(pos, little);
                pos += 4;
                for _ in 0..n {
                    pos = self.string(pos, base, little);
                    pos = self.typecode(pos, base, little);
                }
                pos
            }
            29 => {
                pos = self.align(pos, base, 2);
                pos += 2; // ValueModifier
                pos = self.typecode(pos, base, little); // concrete base, or tk_null
                pos = self.align(pos, base, 4);
                let n = self.word(pos, little);
                pos += 4;
                for _ in 0..n {
                    pos = self.string(pos, base, little);
                    pos = self.typecode(pos, base, little);
                    pos = self.align(pos, base, 2);
                    pos += 2; // Visibility
                }
                pos
            }
            other => panic!("no body walk for TCKind {other}"),
        }
    }
}
