//! What §4.5 can say about a value's own type (AnyJSON v1.1, D008).
//!
//! §8's acceptance criterion is that `any -> JSON -> any` reproduces identical
//! CDR. These are the cases it could not reach while `_t` was a bare name:
//! a constructed type in an `any`, a `TypeCode` as a value in its own right,
//! and a bound that a name silently dropped.

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_dynamic::anyjson::{LocalReferences, from_json, to_json};
use orbweaver_dynamic::{Value, decode, encode, json::Json};
use orbweaver_giop::typecode::{Member, TypeCode, UnionCase};

/// Encode, cross to JSON and back, and require identical CDR both ways round.
fn survives(tc: &TypeCode, v: &Value) {
    let mut h = LocalReferences::new();
    let j = to_json(tc, v, &mut h).unwrap_or_else(|e| panic!("to_json: {e}"));
    let text = j.to_string();
    let reparsed = Json::parse(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
    let back = from_json(tc, &reparsed, &h).unwrap_or_else(|e| panic!("from_json {text}: {e}"));

    for endian in [Endian::Big, Endian::Little] {
        let mut a = Encoder::new(endian);
        encode(&mut a, tc, v).expect("encode original");
        let mut b = Encoder::new(endian);
        encode(&mut b, tc, &back).expect("encode round-tripped");
        assert_eq!(a.finish().unwrap(), b.finish().unwrap(), "{endian:?}: {text}");
    }
}

/// And the CDR round trip on its own, which the JSON one rides on.
fn cdr_survives(tc: &TypeCode, v: &Value) {
    for endian in [Endian::Big, Endian::Little] {
        let mut e = Encoder::new(endian);
        encode(&mut e, tc, v).unwrap_or_else(|err| panic!("{endian:?} encode: {err}"));
        let bytes = e.finish().unwrap();
        let back = decode(&mut Decoder::new(&bytes, endian), tc)
            .unwrap_or_else(|err| panic!("{endian:?} decode: {err}"));
        assert_eq!(&back, v, "{endian:?}");
    }
}

fn tagged() -> TypeCode {
    // corpus/golden/12's `struct Tagged`, minus the `any` member, which would
    // make this test about nesting rather than about the type name.
    TypeCode::Struct {
        id: "IDL:gc12/Tagged:1.0".into(),
        name: "Tagged".into(),
        members: vec![
            Member { name: "name".into(), tc: TypeCode::String(0) },
            Member { name: "n".into(), tc: TypeCode::Long },
        ],
    }
}

fn tagged_value() -> Value {
    Value::Struct(vec![("name".into(), Value::String("x".into())), ("n".into(), Value::Long(7))])
}

/// The shape `corpus/golden/12` declares with `sequence<any>` and
/// `put(in any value)`. Before D008 the mapping wrote this document and then
/// refused to read it back, so the failure landed on the return leg.
#[test]
fn an_any_carrying_a_struct_survives_the_crossing() {
    survives(&TypeCode::Any, &Value::Any(Box::new(tagged()), Box::new(tagged_value())));
}

/// A bound is part of the TypeCode a peer receives. `string<5>` and `string`
/// were the same word to v1, and different bytes to the peer.
#[test]
fn a_bound_inside_an_any_is_not_lost_to_its_type_name() {
    let bounded = TypeCode::String(5);
    survives(
        &TypeCode::Any,
        &Value::Any(Box::new(bounded), Box::new(Value::String("abcde".into()))),
    );

    // Stated as bytes as well, since that is the claim: the TypeCode the peer
    // reads must be the bounded one.
    let mut h = LocalReferences::new();
    let v = Value::Any(Box::new(TypeCode::String(5)), Box::new(Value::String("abcde".into())));
    let j = to_json(&TypeCode::Any, &v, &mut h).unwrap();
    let back = from_json(&TypeCode::Any, &j, &h).unwrap();
    let Value::Any(tc, _) = back else { panic!("not an any") };
    assert_eq!(*tc, TypeCode::String(5), "the bound did not survive: {j}");
}

/// `::CORBA::TypeCode` as a value: what `corpus/golden/12`'s `describe()`
/// returns and what every `ir-subset` description is made of.
#[test]
fn a_typecode_is_a_value_the_dynamic_path_can_carry() {
    for carried in [
        TypeCode::Long,
        TypeCode::String(0),
        TypeCode::WString(64),
        TypeCode::Sequence { element: Box::new(TypeCode::Double), bound: 3 },
        TypeCode::Array { element: Box::new(TypeCode::Octet), length: 4 },
        TypeCode::Any,
        TypeCode::TypeCode,
        tagged(),
        TypeCode::Alias {
            id: "IDL:m/Meters:1.0".into(),
            name: "Meters".into(),
            aliased: Box::new(TypeCode::Long),
        },
        TypeCode::Enum {
            id: "IDL:m/Colour:1.0".into(),
            name: "Colour".into(),
            members: vec!["RED".into(), "GREEN".into()],
        },
        TypeCode::ObjRef { id: "IDL:m/I:1.0".into(), name: "I".into() },
    ] {
        let v = Value::TypeCode(Box::new(carried.clone()));
        cdr_survives(&TypeCode::TypeCode, &v);
        survives(&TypeCode::TypeCode, &v);
    }
}

/// A union's labels are the discriminator already encoded in its own type, so
/// a `char` label and a `long` label of the same ordinal are different bytes.
#[test]
fn a_union_typecode_keeps_its_labels_exactly() {
    let u = TypeCode::Union {
        id: "IDL:m/U:1.0".into(),
        name: "U".into(),
        discriminator: Box::new(TypeCode::Long),
        default_index: -1,
        cases: vec![
            UnionCase { label: vec![0, 0, 0, 1], name: "a".into(), tc: TypeCode::Long },
            UnionCase { label: vec![0, 0, 0, 2], name: "b".into(), tc: TypeCode::String(0) },
        ],
    };
    let v = Value::TypeCode(Box::new(u.clone()));
    cdr_survives(&TypeCode::TypeCode, &v);
    survives(&TypeCode::TypeCode, &v);

    // Exactly, and also *readably*. Round-tripping alone would pass just as
    // well on base64, which is what this carried until the byte order of a
    // label became knowable — so the document is asserted, not just the trip.
    let mut h = LocalReferences::new();
    let text = to_json(&TypeCode::TypeCode, &v, &mut h).unwrap().to_string();
    assert!(text.contains("\"label\":1"), "a label should read as its value: {text}");
    assert!(text.contains("\"label\":2"), "a label should read as its value: {text}");
    assert!(!text.contains("_raw"), "no label here is undecodable: {text}");
}

/// An enum discriminator's labels read as enumerator *names*, which is the
/// point of writing them as values: `"RED"` says what `[0,0,0,0]` cannot.
#[test]
fn an_enum_discriminated_union_labels_by_name() {
    let colour = TypeCode::Enum {
        id: "IDL:m/Colour:1.0".into(),
        name: "Colour".into(),
        members: vec!["RED".into(), "GREEN".into()],
    };
    let u = TypeCode::Union {
        id: "IDL:m/V:1.0".into(),
        name: "V".into(),
        discriminator: Box::new(colour),
        default_index: -1,
        cases: vec![UnionCase { label: vec![0, 0, 0, 1], name: "g".into(), tc: TypeCode::Long }],
    };
    let v = Value::TypeCode(Box::new(u));
    cdr_survives(&TypeCode::TypeCode, &v);
    survives(&TypeCode::TypeCode, &v);

    let mut h = LocalReferences::new();
    let text = to_json(&TypeCode::TypeCode, &v, &mut h).unwrap().to_string();
    assert!(text.contains("\"label\":\"GREEN\""), "{text}");
}

/// The `_raw` fallback, executed rather than merely written. A label whose
/// bytes do not decode as its discriminator means the TypeCode is malformed;
/// this mapping renders it tagged instead of refusing, because a renderer that
/// will not render the evidence is how a malformed contract gets blamed on the
/// reader. Written and never run is how `corpus/golden/28` found the Rust
/// emitter's keyword list missing `yield`.
#[test]
fn a_label_that_does_not_decode_crosses_tagged_and_comes_back_exact() {
    let u = TypeCode::Union {
        id: "IDL:m/Bad:1.0".into(),
        name: "Bad".into(),
        // A `string` cannot be a discriminator; a label under one cannot decode.
        discriminator: Box::new(TypeCode::String(0)),
        default_index: -1,
        cases: vec![UnionCase { label: vec![9, 9, 9, 9], name: "x".into(), tc: TypeCode::Long }],
    };
    let v = Value::TypeCode(Box::new(u));
    let mut h = LocalReferences::new();
    let j = to_json(&TypeCode::TypeCode, &v, &mut h).expect("to_json");
    let text = j.to_string();
    assert!(text.contains("_raw"), "an undecodable label should be tagged: {text}");
    assert_eq!(from_json(&TypeCode::TypeCode, &j, &h).expect("from_json"), v, "{text}");
}

/// Every v1 document this project could produce must still parse and still
/// mean the same thing — the compatibility half of D008's additive claim.
#[test]
fn every_v1_any_document_still_reads() {
    for (text, expect) in [
        (r#"{"_t":"double","_v":3.5}"#, TypeCode::Double),
        (r#"{"_t":"long","_v":7}"#, TypeCode::Long),
        (r#"{"_t":"string","_v":"x"}"#, TypeCode::String(0)),
        (r#"{"_t":"unsigned long long","_v":"18446744073709551615"}"#, TypeCode::ULongLong),
        (r#"{"_t":"boolean","_v":true}"#, TypeCode::Boolean),
    ] {
        let j = Json::parse(text).unwrap();
        let h = LocalReferences::new();
        let back = from_json(&TypeCode::Any, &j, &h)
            .unwrap_or_else(|e| panic!("v1 document {text} no longer reads: {e}"));
        let Value::Any(tc, _) = back else { panic!("{text} did not read as an any") };
        assert_eq!(*tc, expect, "{text}");
    }
}
