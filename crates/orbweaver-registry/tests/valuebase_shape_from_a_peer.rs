//! `ValueBase` is a **valuetype**, and the registry called it an object
//! reference — the bytes a peer writes, beside the ones we derive.
//!
//! `TypeSpec::ValueBase` mapped to `TypeCode::ObjRef { IDL:omg.org/CORBA/
//! ValueBase:1.0 }`, so `struct Envelope { ValueBase payload; }` generated as
//! a *reference* and the dynamic path put an IOR on the wire where a
//! conformant peer sends a value. It is the same defect the `valuetype` batch
//! closed on 2026-08-20, surviving in the one spelling that has no declaration
//! to hang a fix on: S4's closure named these declarations the whole time
//! (`sema.rs`, `TypeSpec::ValueBase`), and because the generators were handed
//! an `ObjRef` they skipped nothing — so the rule and the generator disagreed
//! and no test could see it, there being no corpus file that wrote the
//! keyword.
//!
//! The bytes below were captured from omniORB 4.3.4 (omniORBpy,
//! `cdrMarshal(CORBA._tc_TypeCode, tc, endian)` over the TypeCode
//! `omniidl -bpython` built from `corpus/golden/32-valuebase.idl` — the corpus
//! file this test loads, not a paraphrase of it) on 2026-08-21 by
//! `spikes/native_capture.py`, which retakes them from the live fixture and
//! compares outside the padding. Clause (b) of the licensing boundary: the
//! output of a program we ran, describing IDL we wrote.
//!
//! The one field a reasoned answer would have got wrong is in there:
//! `ValueBase` is the *abstract* base of every valuetype, and omniORB writes
//! its ValueModifier as **VM_NONE (0)**, not VM_ABSTRACT (2). Measured, not
//! deduced.
//!
//! Each recording is taken in a little-endian and a big-endian stream. In the
//! big-endian one the outer struct header is big-endian (`00 00 00 0f`) and
//! the encapsulation flag is still `01` — omniORB writes the body
//! little-endian inside a big-endian stream, which is precisely the case a
//! decoder written native-endian passes and a real peer breaks.
//!
//! *`ValueBase`는 valuetype이다. 레지스트리는 객체 참조라고 기록했고, 그것을
//! 피어의 것과 비교하는 곳이 없었다 — 코퍼스에 그 키워드를 쓴 파일이 하나도
//! 없었기 때문이다.*

use std::path::{Path, PathBuf};

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_giop::typecode::{TypeCode, decode, encode};
use orbweaver_registry::Registry;

/// `gvb32._tc_Envelope`, TCKind 15, little-endian stream
const GVB32_ENVELOPE_LITTLE: &[u8] = &[
    0x0f, 0x00, 0x00, 0x00, 0x9c, 0x00, 0x00, 0x00, 0x01, 0x78, 0x06, 0x05, 0x17, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x67, 0x76, 0x62, 0x33, 0x32, 0x2f, 0x45, 0x6e, 0x76, 0x65, 0x6c, 0x6f,
    0x70, 0x65, 0x3a, 0x31, 0x2e, 0x30, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x45, 0x6e, 0x76, 0x65,
    0x6c, 0x6f, 0x70, 0x65, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00,
    0x70, 0x61, 0x79, 0x6c, 0x6f, 0x61, 0x64, 0x00, 0x1d, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
    0x01, 0x1a, 0xaa, 0x6b, 0x20, 0x00, 0x00, 0x00, 0x49, 0x44, 0x4c, 0x3a, 0x6f, 0x6d, 0x67, 0x2e,
    0x6f, 0x72, 0x67, 0x2f, 0x43, 0x4f, 0x52, 0x42, 0x41, 0x2f, 0x56, 0x61, 0x6c, 0x75, 0x65, 0x42,
    0x61, 0x73, 0x65, 0x3a, 0x31, 0x2e, 0x30, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x56, 0x61, 0x6c, 0x75,
    0x65, 0x42, 0x61, 0x73, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x08, 0x00, 0x00, 0x00, 0x72, 0x6f, 0x75, 0x74, 0x69, 0x6e, 0x67, 0x00, 0x12, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
];
/// `gvb32._tc_Envelope`, TCKind 15, big-endian stream
const GVB32_ENVELOPE_BIG: &[u8] = &[
    0x00, 0x00, 0x00, 0x0f, 0x00, 0x00, 0x00, 0x9c, 0x01, 0xa9, 0x56, 0x04, 0x17, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x67, 0x76, 0x62, 0x33, 0x32, 0x2f, 0x45, 0x6e, 0x76, 0x65, 0x6c, 0x6f,
    0x70, 0x65, 0x3a, 0x31, 0x2e, 0x30, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x45, 0x6e, 0x76, 0x65,
    0x6c, 0x6f, 0x70, 0x65, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00,
    0x70, 0x61, 0x79, 0x6c, 0x6f, 0x61, 0x64, 0x00, 0x1d, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00,
    0x01, 0x1a, 0xaa, 0x6b, 0x20, 0x00, 0x00, 0x00, 0x49, 0x44, 0x4c, 0x3a, 0x6f, 0x6d, 0x67, 0x2e,
    0x6f, 0x72, 0x67, 0x2f, 0x43, 0x4f, 0x52, 0x42, 0x41, 0x2f, 0x56, 0x61, 0x6c, 0x75, 0x65, 0x42,
    0x61, 0x73, 0x65, 0x3a, 0x31, 0x2e, 0x30, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x56, 0x61, 0x6c, 0x75,
    0x65, 0x42, 0x61, 0x73, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x08, 0x00, 0x00, 0x00, 0x72, 0x6f, 0x75, 0x74, 0x69, 0x6e, 0x67, 0x00, 0x12, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
];

const ENVELOPE: &str = "IDL:gvb32/Envelope:1.0";

fn recordings() -> Vec<(&'static str, &'static [u8], Endian)> {
    vec![
        ("Envelope LE", GVB32_ENVELOPE_LITTLE, Endian::Little),
        ("Envelope BE", GVB32_ENVELOPE_BIG, Endian::Big),
    ]
}

fn golden() -> Registry {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join("corpus/golden/32-valuebase.idl");
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let spec =
        orbweaver_idl::parse(&src).unwrap_or_else(|e| panic!("{} must parse: {e}", path.display()));
    let mut reg = Registry::new();
    reg.load(&spec).unwrap_or_else(|e| panic!("{} must load: {e}", path.display()));
    reg
}

/// What omniORB wrote, decoded, **equals** what the registry derives from the
/// same IDL — the whole `TypeCode`, not a projection. A projection is where a
/// wrong kind hides, and a wrong kind is exactly what was here.
#[test]
fn the_registry_derives_the_valuebase_typecode_omniidl_derives() {
    let ours = golden().typecode(ENVELOPE).unwrap_or_else(|| panic!("{ENVELOPE}")).clone();
    let mut failures = Vec::new();
    for (what, bytes, endian) in recordings() {
        match decode(&mut Decoder::new(bytes, endian)) {
            Err(e) => failures.push(format!("{what}: the peer's bytes do not decode: {e}")),
            Ok(peer) if peer != ours => {
                failures.push(format!("{what}: not equal\n    peer {peer:?}\n    ours {ours:?}"))
            }
            Ok(_) => {}
        }
    }
    assert!(failures.is_empty(), "{} failure(s):\n  {}", failures.len(), failures.join("\n  "));
}

/// The composition a peer actually meets: our `TypeCode` encoded by us, in
/// either byte order, read back, equals the peer's IDL-derived one.
#[test]
fn ours_re_encoded_reads_back_equal_to_the_peers_in_either_byte_order() {
    let ours = golden().typecode(ENVELOPE).unwrap_or_else(|| panic!("{ENVELOPE}")).clone();
    let peer = decode(&mut Decoder::new(GVB32_ENVELOPE_LITTLE, Endian::Little))
        .expect("the peer's bytes decode");
    let mut failures = Vec::new();
    for out in [Endian::Big, Endian::Little] {
        let mut e = Encoder::new(out);
        encode(&mut e, &ours).expect("encode");
        let wire = e.finish().expect("finish");
        match decode(&mut Decoder::new(&wire, out)) {
            Err(e) => failures.push(format!("via {out:?}: ours does not decode: {e}")),
            Ok(back) if back != peer => {
                failures.push(format!("via {out:?}: {back:?} != the peer's {peer:?}"))
            }
            Ok(_) => {}
        }
    }
    assert!(failures.is_empty(), "{} failure(s):\n  {}", failures.len(), failures.join("\n  "));
}

/// The one sentence, said on our side rather than left to a wall of `Debug`
/// output: a `ValueBase` member is a value, its modifier is the one the peer
/// writes, and a reference in the same file is still a reference.
#[test]
fn the_registry_never_calls_a_valuebase_an_object_reference() {
    let reg = golden();
    let payload = match reg.typecode(ENVELOPE) {
        Some(TypeCode::Struct { members, .. }) => members[0].tc.clone(),
        other => panic!("gvb32::Envelope is not a struct: {other:?}"),
    };
    match &payload {
        TypeCode::Value { id, name, modifier, base, members } => {
            assert_eq!(id, "IDL:omg.org/CORBA/ValueBase:1.0");
            assert_eq!(name, "ValueBase");
            // VM_NONE. omniORB writes 0 here for the abstract base of every
            // valuetype, which is why this is asserted rather than reasoned:
            // VM_ABSTRACT (2) is what the name suggests and not what the peer
            // sends.
            assert_eq!(*modifier, 0, "ValueModifier");
            assert!(base.is_none(), "concrete base: {base:?}");
            assert!(members.is_empty(), "state members: {members:?}");
        }
        other => panic!("a ValueBase member is not a valuetype: {other:?}"),
    }
    // The negative control in the same file: `Depot` holds a `Courier`, an
    // interface the rule refuses for an unrelated reason (its operations carry
    // a `ValueBase`). A reference to it is still a reference — the split is a
    // distinction, not a rename.
    let agent = match reg.typecode("IDL:gvb32/Depot:1.0") {
        Some(TypeCode::Struct { members, .. }) => members[0].tc.clone(),
        other => panic!("gvb32::Depot is not a struct: {other:?}"),
    };
    assert!(matches!(&agent, TypeCode::ObjRef { name, .. } if name == "Courier"), "{agent:?}");
}
