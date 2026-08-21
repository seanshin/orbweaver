//! What a conformant peer does with `native X;` — recorded, and it is a
//! refusal rather than a byte sequence.
//!
//! Every other `*_from_a_peer.rs` in this project holds our codec to bytes
//! omniORB wrote. This one exists because omniORB writes none: it was asked
//! for a `tk_native` TypeCode by all four routes it has and produced one by
//! none of them. That answer is the measurement, and it is what
//! `TypeCode::Native` is built from — a variant with **no `TcKind`**, whose
//! `kind()` is `None` and which `encode` refuses by name.
//!
//! Captured from omniORB 4.3.4 / omniidl on 2026-08-21 by
//! `spikes/native_capture.py`, which re-runs every probe below against the
//! live fixture. Clause (b) of the licensing boundary: omniidl is run as an
//! external program and its text output is read; nothing is linked, vendored
//! or redistributed.
//!
//! The strings live here as `const` items and the script greps them out, so
//! the script and this file are held to the *same* text. A refusal
//! paraphrased in one of the two places is a recording that has quietly
//! stopped being one.
//!
//! # Why this matters more than a missing feature
//!
//! Until 2026-08-21 the registry recorded a `native` as `TypeCode::ObjRef`,
//! so both emitters emitted an object reference for it and the dynamic path
//! marshalled an **IOR** — for a type that has no wire form at all. It
//! survived the batch that fixed the identical defect for `valuetype` and
//! abstract interfaces because `docs/PLAN.md` §4.4 does not name it, so S4's
//! `wire/deferred-type` rule did not either, and `deferred_wire_agreement`
//! would have gone red at any generator that started refusing it. The gap in
//! the rule was holding the wrong answer in place.
//!
//! *피어에게 물었고, 네 경로 모두에서 거부가 돌아왔다. 그 거부가 측정값이다.*

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_giop::typecode::{TypeCode, decode, encode};

/// `omniidl -bcxx` over a file whose only unusual line is `native Handle;`.
/// Exit status 1, and this on stdout. The C++ back end does not warn and
/// continue — it stops.
const CXX_REFUSAL: &str = "Unsupported IDL construct found in input (native)";

/// `omniidl -bpython` over the same file. Exit status 0, this on stderr, and
/// **nothing generated** for the declaration.
const PYTHON_WARNING: &str = "ignoring declaration of native Handle";

/// What importing what it generated then costs. The struct's descriptor
/// references `omniORB.typeMapping["IDL:gn31/Handle:1.0"]`, the ignored
/// declaration never registered it, and the module cannot be loaded at all —
/// so the Python back end's "warning" is a hard failure one step later.
const PYTHON_IMPORT_ERROR: &str = "KeyError 'IDL:gn31/Handle:1.0'";

/// The ordinal, and the fourth route: `CORBA.tk_native._v` is 31 and that is
/// all it is. The ORB has `create_value_tc` and `create_abstract_interface_tc`
/// and fourteen more `create_*_tc` factories and **no `create_native_tc`**;
/// `tcInternal.createTypeCode((tv_native, id, name))` raises `CORBA.INTERNAL`.
///
/// So the kind ordinal is known and its *parameter list* is not. CORBA 3.4
/// Part 2 Table 9.2 would tell us what a conformant peer ought to write, and
/// this project does not encode from the table when it can encode from a
/// measurement — an ordinal nobody has seen written is a guess, which is the
/// rule `TcKind`'s own documentation already stated for 30, 31 and 33.
const TK_NATIVE: u32 = 31;

fn native() -> TypeCode {
    TypeCode::Native { id: "IDL:gn31/Handle:1.0".into(), name: "Handle".into() }
}

/// The recordings are non-empty and say what they say. Trivial as an
/// assertion; its job is to fail the day somebody edits one of them without
/// re-running `spikes/native_capture.py`, which greps for exactly these
/// strings.
#[test]
fn the_recorded_refusals_are_the_ones_the_capture_script_looks_for() {
    for (what, text) in [
        ("C++ back end", CXX_REFUSAL),
        ("Python back end", PYTHON_WARNING),
        ("Python import", PYTHON_IMPORT_ERROR),
    ] {
        assert!(!text.is_empty(), "{what}: the recording is empty");
    }
    assert!(CXX_REFUSAL.contains("native"));
    assert!(PYTHON_WARNING.contains("native"));
    assert!(PYTHON_IMPORT_ERROR.contains("IDL:gn31/Handle:1.0"));
}

/// Our decoder answers 31 the way the peer does: it has no such TypeCode.
///
/// A stream carrying kind 31 is refused with a message that names itself,
/// rather than decoded into a variant nobody has measured the parameter list
/// of. Both byte orders, because a decoder that only works native-endian
/// passes every local test and fails in the field.
#[test]
fn a_peer_that_sends_tk_native_is_refused_by_name_in_both_byte_orders() {
    for endian in [Endian::Little, Endian::Big] {
        let mut e = Encoder::new(endian);
        e.put_u32(TK_NATIVE);
        // A plausible complex parameter list, so the refusal is about the kind
        // and not about running out of bytes.
        let body = {
            let mut inner = Encoder::new(endian);
            inner.put_str("IDL:gn31/Handle:1.0");
            inner.put_str("Handle");
            inner.finish().expect("finish")
        };
        e.put_u32(body.len() as u32 + 1);
        e.put_u8(u8::from(endian == Endian::Little));
        for b in &body {
            e.put_u8(*b);
        }
        let wire = e.finish().expect("finish");
        let err = decode(&mut Decoder::new(&wire, endian))
            .expect_err("kind 31 must not decode")
            .to_string();
        assert!(
            err.contains("TCKind"),
            "{endian:?}: the refusal must name the kind it does not know: {err}"
        );
    }
}

/// And symmetrically on the way out: we never write a kind for one.
///
/// `encode` refusing is the whole safety property. The alternative that was
/// live until 2026-08-21 — a `TypeCode::ObjRef` carrying the native's
/// repository id — encoded perfectly, produced a `tk_objref` a peer would
/// happily read, and described an object reference for something that is not
/// one.
#[test]
fn we_never_encode_a_native_and_the_refusal_names_it() {
    for endian in [Endian::Little, Endian::Big] {
        let mut e = Encoder::new(endian);
        let err = encode(&mut e, &native()).expect_err("a native must not encode").to_string();
        assert!(err.contains("native"), "{endian:?}: {err}");
    }
    // Nested, so the cascade is executed too: a struct holding one is not
    // encodable either, and the failure is the same sentence rather than a
    // truncated message about the struct.
    let holder = TypeCode::Struct {
        id: "IDL:gn31/Session:1.0".into(),
        name: "Session".into(),
        members: vec![orbweaver_giop::typecode::Member { name: "token".into(), tc: native() }],
    };
    let mut e = Encoder::new(Endian::Little);
    let err =
        encode(&mut e, &holder).expect_err("a struct holding one must not encode").to_string();
    assert!(err.contains("native"), "{err}");
}

/// `kind()` is `None`, which is the fact the other two tests rest on, asserted
/// on its own so a change to it names itself rather than surfacing as two
/// encode failures.
#[test]
fn a_native_has_no_tckind_because_the_peer_has_none_to_give() {
    assert!(native().kind().is_none(), "{:?}", native().kind());
    // It still has a repository id: describing one is not marshalling one, and
    // the registry, the catalogue and the IFR all ask.
    assert_eq!(native().repository_id(), Some("IDL:gn31/Handle:1.0"));
    // The neighbours the same argument applies to, kept absent for the same
    // reason: 30 `tk_value_box` and 33 `tk_local_interface`. Nothing has shown
    // this project one.
    for unknown in [30u32, 31, 33] {
        let mut e = Encoder::new(Endian::Little);
        e.put_u32(unknown);
        e.put_u32(1);
        e.put_u8(1);
        let wire = e.finish().expect("finish");
        assert!(
            decode(&mut Decoder::new(&wire, Endian::Little)).is_err(),
            "TCKind {unknown} must not decode"
        );
    }
}
