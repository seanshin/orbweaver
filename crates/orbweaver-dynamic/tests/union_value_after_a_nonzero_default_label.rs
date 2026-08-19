//! A union *value* under a TypeCode whose default member a peer labelled with
//! a non-zero value — R18's second half.
//!
//! `orbweaver-giop/tests/union_default_label_from_a_peer.rs` shows that the
//! TypeCode a conformant third peer writes with **any** value in the default
//! member's label slot (CORBA 3.4 Part 2 §9.3.5.1.4: "may be any valid value
//! of the discriminant type, and has no semantic significance") decodes to
//! the shape the zero label decodes to. This file takes the same bytes one
//! step further, to where a wrong reading would actually cost something: a
//! value of that union, marshalled under the TypeCode read from the peer's
//! bytes, in both byte orders — and, for the label a peer is allowed to
//! choose that collides with a real case, a discriminator equal to it selects
//! **the real case**, not the default. That is the half of the claim a
//! TypeCode comparison cannot state, because `select_case` is what would have
//! believed a kept label.
//!
//! The builder is a copy of the giop test's, checked the same way — against
//! our own encoder on the zero label — because this crate can see the giop
//! encoder and the giop test cannot see this crate's value codec.

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_dynamic::{Value, decode, encode};
use orbweaver_giop::typecode::{TypeCode, UnionCase};

/// The union TypeCode encapsulation, built by hand from a shape with the
/// default member labelless, with `default_label` in the default member's
/// slot at the discriminator's width, in the stream's byte order.
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
    orbweaver_giop::typecode::encode(&mut inner, discriminator).expect("discriminator");
    inner.put_i32(*default_index);
    inner.put_u32(cases.len() as u32);
    for (i, c) in cases.iter().enumerate() {
        inner.align_to(width.min(8));
        let label = if *default_index >= 0 && i == *default_index as usize {
            in_order(&default_label.to_be_bytes()[8 - width..])
        } else {
            in_order(&c.label)
        };
        inner.put_bytes(&label);
        inner.put_str(&c.name);
        orbweaver_giop::typecode::encode(&mut inner, &c.tc).expect("member");
    }
    let mut outer = Encoder::new(endian);
    outer.put_u32(16); // tk_union
    outer.put_encapsulation(inner);
    outer.finish().expect("finish")
}

fn case(label: &[u8], name: &str, tc: TypeCode) -> UnionCase {
    UnionCase { label: label.to_vec(), name: name.into(), tc }
}

fn union(name: &str, discriminator: TypeCode, cases: Vec<UnionCase>) -> TypeCode {
    TypeCode::Union {
        id: format!("IDL:udef/{name}:1.0"),
        name: name.into(),
        discriminator: Box::new(discriminator),
        default_index: 1,
        cases,
    }
}

fn hue() -> TypeCode {
    TypeCode::Enum {
        id: "IDL:udef/Hue:1.0".into(),
        name: "Hue".into(),
        members: vec!["RED".into(), "GREEN".into(), "BLUE".into()],
    }
}

/// One union per discriminator kind the giop recordings cover — the same six
/// IDL declarations, `case X: T a; default: U b;` — with, for each, the
/// non-zero labels a peer might write, and the values to marshal under it:
/// `(discriminator, the branch name IDL says it selects, that branch's
/// value)`. The last discriminator in each list is the label a real case
/// carries, and its expected branch is that case.
struct Kind {
    what: &'static str,
    tc: TypeCode,
    labels: Vec<u64>,
    values: Vec<(Value, &'static str, Value)>,
}

fn kinds() -> Vec<Kind> {
    vec![
        Kind {
            what: "long",
            tc: union(
                "DL",
                TypeCode::Long,
                vec![case(&[0, 0, 0, 1], "a", TypeCode::Long), case(&[], "b", TypeCode::String(0))],
            ),
            labels: vec![i32::MAX as u32 as u64, i32::MIN as u32 as u64, u32::MAX as u64, 1],
            values: vec![
                (Value::Long(i32::MAX), "b", Value::String("max".into())),
                (Value::Long(i32::MIN), "b", Value::String("min".into())),
                (Value::Long(-1), "b", Value::String("minus one".into())),
                (Value::Long(1), "a", Value::Long(42)),
            ],
        },
        Kind {
            what: "short",
            tc: union(
                "DS",
                TypeCode::Short,
                vec![case(&[0, 1], "a", TypeCode::Short), case(&[], "b", TypeCode::String(0))],
            ),
            labels: vec![i16::MAX as u16 as u64, i16::MIN as u16 as u64, u16::MAX as u64, 1],
            values: vec![
                (Value::Short(i16::MAX), "b", Value::String("max".into())),
                (Value::Short(i16::MIN), "b", Value::String("min".into())),
                (Value::Short(-1), "b", Value::String("minus one".into())),
                (Value::Short(1), "a", Value::Short(7)),
            ],
        },
        Kind {
            what: "long long",
            tc: union(
                "DLL",
                TypeCode::LongLong,
                vec![
                    case(&[0, 0, 0, 0, 0, 0, 0, 1], "a", TypeCode::Long),
                    case(&[], "b", TypeCode::String(0)),
                ],
            ),
            labels: vec![i64::MAX as u64, i64::MIN as u64, u64::MAX, 1],
            values: vec![
                (Value::LongLong(i64::MAX), "b", Value::String("max".into())),
                (Value::LongLong(i64::MIN), "b", Value::String("min".into())),
                (Value::LongLong(-1), "b", Value::String("minus one".into())),
                (Value::LongLong(1), "a", Value::Long(42)),
            ],
        },
        Kind {
            what: "boolean",
            tc: union(
                "DB",
                TypeCode::Boolean,
                vec![case(&[1], "yes", TypeCode::Long), case(&[], "no", TypeCode::Octet)],
            ),
            labels: vec![0xff, 1],
            values: vec![
                (Value::Bool(false), "no", Value::Octet(9)),
                (Value::Bool(true), "yes", Value::Long(1)),
            ],
        },
        Kind {
            what: "char",
            tc: union(
                "DC",
                TypeCode::Char,
                vec![case(b"a", "a", TypeCode::Long), case(&[], "b", TypeCode::String(0))],
            ),
            labels: vec![0xff, 0x7f, u64::from(b'a')],
            values: vec![
                (Value::Char(0xff), "b", Value::String("377".into())),
                (Value::Char(0x7f), "b", Value::String("177".into())),
                (Value::Char(b'z'), "b", Value::String("z".into())),
                (Value::Char(b'a'), "a", Value::Long(97)),
            ],
        },
        Kind {
            what: "enum",
            tc: union(
                "DE",
                hue(),
                vec![
                    case(&[0, 0, 0, 0], "warm", TypeCode::Octet),
                    case(&[], "named", TypeCode::String(0)),
                ],
            ),
            // The zero label collides with `case RED:` here — the one kind
            // where *our* label collides — so the collision under test is
            // RED's ordinal, which is 0 and is exercised by the zero pass; the
            // non-zero labels are the two unused enumerators and two ordinals
            // no enumerator has.
            labels: vec![1, 2, 3, u32::MAX as u64],
            values: vec![
                (Value::Enum("GREEN".into()), "named", Value::String("green".into())),
                (Value::Enum("BLUE".into()), "named", Value::String("blue".into())),
                (Value::Enum("RED".into()), "warm", Value::Octet(200)),
            ],
        },
    ]
}

fn branch_of<'a>(tc: &'a TypeCode, disc: &Value) -> &'a str {
    let TypeCode::Union { cases, default_index, .. } = tc else { panic!() };
    // The same probe `select_case` uses, done in the open: the discriminator
    // in big-endian, matched against the canonical labels.
    let mut probe = Encoder::new(Endian::Big);
    encode(&mut probe, discriminator_of(tc), disc).expect("disc");
    let label = probe.finish().expect("probe");
    match cases.iter().find(|c| c.label == label) {
        Some(c) => &c.name,
        None => &cases[*default_index as usize].name,
    }
}

fn discriminator_of(tc: &TypeCode) -> &TypeCode {
    let TypeCode::Union { discriminator, .. } = tc else { panic!() };
    discriminator
}

/// The builder reproduces the giop encoder on the zero label, both orders.
#[test]
fn the_hand_builder_agrees_with_the_giop_encoder_on_the_zero_label() {
    for k in kinds() {
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            orbweaver_giop::typecode::encode(&mut e, &k.tc).expect("encode");
            assert_eq!(hand_built(&k.tc, endian, 0), e.finish().expect("finish"), "{}", k.what);
        }
    }
}

/// Under a TypeCode read from a peer that labelled the default member with a
/// non-zero value — every value in the matrix, both stream orders — a value
/// of the union encodes and decodes back to itself in both byte orders, lands
/// on the branch IDL says, and a discriminator equal to a real case's label
/// selects that case even when the peer put the same value in the default's
/// slot. Every TypeCode is also `==` the one the zero label reads to.
#[test]
fn a_value_marshals_the_same_under_any_default_label_a_peer_wrote() {
    let mut failures = Vec::new();
    let mut typecodes = 0;
    let mut values = 0;
    for k in kinds() {
        for stream in [Endian::Big, Endian::Little] {
            for label in &k.labels {
                let bytes = hand_built(&k.tc, stream, *label);
                let read = orbweaver_giop::typecode::decode(&mut Decoder::new(&bytes, stream))
                    .expect("the peer's TypeCode decodes");
                typecodes += 1;
                if read != k.tc {
                    failures.push(format!(
                        "{} {stream:?} label {label:#x}: TypeCode read as {read:?}",
                        k.what
                    ));
                    continue;
                }
                for (disc, want_branch, member) in &k.values {
                    let value = Value::Union {
                        discriminator: Box::new(disc.clone()),
                        value: Some(Box::new(member.clone())),
                    };
                    let got_branch = branch_of(&read, disc);
                    if got_branch != *want_branch {
                        failures.push(format!(
                            "{} {stream:?} label {label:#x}: {disc:?} selects {got_branch}, IDL \
                             says {want_branch}",
                            k.what
                        ));
                    }
                    for out in [Endian::Big, Endian::Little] {
                        let mut e = Encoder::new(out);
                        if let Err(err) = encode(&mut e, &read, &value) {
                            failures.push(format!(
                                "{} {stream:?} label {label:#x}: {disc:?} does not encode via \
                                 {out:?}: {err}",
                                k.what
                            ));
                            continue;
                        }
                        let wire = e.finish().expect("finish");
                        match decode(&mut Decoder::new(&wire, out), &read) {
                            Ok(back) if back == value => values += 1,
                            Ok(back) => failures.push(format!(
                                "{} {stream:?} label {label:#x}: {value:?} came back as {back:?} \
                                 via {out:?}",
                                k.what
                            )),
                            Err(err) => failures.push(format!(
                                "{} {stream:?} label {label:#x}: {disc:?} does not decode via \
                                 {out:?}: {err}",
                                k.what
                            )),
                        }
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{} failure(s):\n  {}", failures.len(), failures.join("\n  "));
    assert_eq!(typecodes, 42, "six kinds × their labels × two stream orders");
    assert_eq!(values, 2 * (4 * 4 + 4 * 4 + 4 * 4 + 2 * 2 + 3 * 4 + 4 * 3) * 2);
}
