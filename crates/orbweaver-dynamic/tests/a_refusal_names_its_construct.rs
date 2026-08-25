//! A refusal names the construct it is refusing, and a catch-all never answers
//! for a variant nobody has thought about.
//!
//! Two failure shapes, one rule, both measured in `orbweaver-dynamic` on
//! 2026-08-25.
//!
//! **A refusal that names nothing.** `anyjson::type_name` named fifteen
//! primitives and asked everything else for a repository id, so the seven
//! variants that carry none — `sequence`, `array`, `any`, `typecode`, `void`,
//! `null` and `Principal` — were refused as `<anonymous>`. The read direction
//! is the one that matters: a **peer-fed** document naming a `void` was
//! answered `"<anonymous> cannot cross yet"`, and `from_json_at`'s own doc
//! comment claimed *"Only primitives for now, and the decoder says so rather
//! than guessing"* — it did not say so; it said `<anonymous>`.
//!
//! This class had been diagnosed once already, in that same file, and closed
//! the wrong way: a `fixed` was refused `"… is not a value of <anonymous>"`
//! until 2026-08-21, and the repair was a *guard above the mismatch arm*
//! rather than the function. That took the one witness out of reach and left
//! the defect, so the same sentence stayed live for seven other variants for
//! four more days. **A guard that stops one caller reaching a defect is not a
//! fix for the defect.**
//!
//! **A catch-all that would answer for a thirty-fourth variant.**
//! `tc_to_json`'s tail was `short_name(other).unwrap_or("void")` under the
//! comment *"Every remaining variant has a short name and returned above"* —
//! true of all thirty-three variants the day it was written, and a silent lie
//! the day a thirty-fourth arrives, because the description of a construct
//! nobody had thought about would cross the wire as the string `"void"`. That
//! is the exact silent wrong answer the `Value`, `AbstractInterface` and
//! `Native` arms were each added to prevent, one after-the-fact discovery at a
//! time.
//!
//! # What this file can and cannot measure
//!
//! The exhaustiveness itself has **no test in it** — it is carried by the
//! compiler, so the drift is impossible rather than detectable, and a negative
//! control for it is a *build* control (add a variant to
//! `orbweaver_giop::typecode::TypeCode` and watch `tc_to_json`, `type_name`
//! and `describe` refuse to compile), not a red assertion. What is asserted
//! here is the behaviour that was wrong and is now right, and the property
//! that made the tail dangerous: **only `void` crosses as `"void"`**.

use orbweaver_dynamic::anyjson::{LocalReferences, from_json, tc_from_json, tc_to_json, to_json};
use orbweaver_dynamic::json::Json;
use orbweaver_dynamic::{Value, dynany};
use orbweaver_giop::typecode::{Member, TypeCode, UnionCase, ValueMember};

/// One short tag per `TypeCode` variant.
///
/// **Exhaustive on purpose**, and that is the whole of its job: a
/// thirty-fourth variant makes this file fail to build, so the sweep below
/// cannot quietly stop covering the set it claims to cover. A sweep whose
/// coverage is a hand-written list is the class this file is about, wearing
/// the test suite's coat.
fn tag(tc: &TypeCode) -> &'static str {
    match tc {
        TypeCode::Null => "null",
        TypeCode::Void => "void",
        TypeCode::Short => "short",
        TypeCode::Long => "long",
        TypeCode::UShort => "ushort",
        TypeCode::ULong => "ulong",
        TypeCode::Float => "float",
        TypeCode::Double => "double",
        TypeCode::Boolean => "boolean",
        TypeCode::Char => "char",
        TypeCode::Octet => "octet",
        TypeCode::Any => "any",
        TypeCode::TypeCode => "typecode",
        TypeCode::Principal => "principal",
        TypeCode::LongLong => "longlong",
        TypeCode::ULongLong => "ulonglong",
        TypeCode::LongDouble => "longdouble",
        TypeCode::WChar => "wchar",
        TypeCode::String(_) => "string",
        TypeCode::WString(_) => "wstring",
        TypeCode::Fixed { .. } => "fixed",
        TypeCode::ObjRef { .. } => "objref",
        TypeCode::Struct { .. } => "struct",
        TypeCode::Union { .. } => "union",
        TypeCode::Enum { .. } => "enum",
        TypeCode::Sequence { .. } => "sequence",
        TypeCode::Array { .. } => "array",
        TypeCode::Alias { .. } => "alias",
        TypeCode::Except { .. } => "except",
        TypeCode::Value { .. } => "value",
        TypeCode::AbstractInterface { .. } => "abstract_interface",
        TypeCode::Native { .. } => "native",
        TypeCode::Recursive(_) => "recursive",
    }
}

/// One inhabitant of every variant `tag` knows about.
fn every_variant() -> Vec<TypeCode> {
    vec![
        TypeCode::Null,
        TypeCode::Void,
        TypeCode::Short,
        TypeCode::Long,
        TypeCode::UShort,
        TypeCode::ULong,
        TypeCode::Float,
        TypeCode::Double,
        TypeCode::Boolean,
        TypeCode::Char,
        TypeCode::Octet,
        TypeCode::Any,
        TypeCode::TypeCode,
        TypeCode::Principal,
        TypeCode::LongLong,
        TypeCode::ULongLong,
        TypeCode::LongDouble,
        TypeCode::WChar,
        TypeCode::String(0),
        TypeCode::String(5),
        TypeCode::WString(0),
        TypeCode::WString(5),
        TypeCode::Fixed { digits: 9, scale: 2 },
        TypeCode::ObjRef { id: "IDL:m/I:1.0".into(), name: "I".into() },
        TypeCode::Struct {
            id: "IDL:m/Point:1.0".into(),
            name: "Point".into(),
            members: vec![Member { name: "px".into(), tc: TypeCode::String(0) }],
        },
        TypeCode::Union {
            id: "IDL:m/U:1.0".into(),
            name: "U".into(),
            discriminator: Box::new(TypeCode::Long),
            default_index: -1,
            cases: vec![UnionCase {
                label: 1i32.to_be_bytes().to_vec(),
                name: "a".into(),
                tc: TypeCode::Long,
            }],
        },
        TypeCode::Enum { id: "IDL:m/E:1.0".into(), name: "E".into(), members: vec!["RED".into()] },
        TypeCode::Sequence { element: Box::new(TypeCode::Long), bound: 0 },
        TypeCode::Sequence { element: Box::new(TypeCode::Octet), bound: 7 },
        TypeCode::Array { element: Box::new(TypeCode::Long), length: 3 },
        TypeCode::Alias {
            id: "IDL:m/Renamed:1.0".into(),
            name: "Renamed".into(),
            aliased: Box::new(TypeCode::Long),
        },
        TypeCode::Except {
            id: "IDL:m/Bad:1.0".into(),
            name: "Bad".into(),
            members: vec![Member { name: "why".into(), tc: TypeCode::String(0) }],
        },
        TypeCode::Value {
            id: "IDL:m/Money:1.0".into(),
            name: "Money".into(),
            modifier: 0,
            base: None,
            members: vec![ValueMember {
                name: "units".into(),
                tc: TypeCode::LongLong,
                visibility: 1,
            }],
        },
        TypeCode::AbstractInterface { id: "IDL:m/D:1.0".into(), name: "D".into() },
        TypeCode::Native { id: "IDL:m/Handle:1.0".into(), name: "Handle".into() },
        TypeCode::Recursive("IDL:m/Loop:1.0".into()),
    ]
}

/// The table covers the whole enum, which is what makes the sweeps below a
/// sweep rather than a sample.
#[test]
fn the_table_covers_every_typecode_variant() {
    let mut seen: Vec<&'static str> = every_variant().iter().map(tag).collect();
    seen.sort_unstable();
    seen.dedup();
    // Counted from `orbweaver_giop::typecode::TypeCode` on 2026-08-25; `tag`
    // is exhaustive, so this figure cannot drift without the build breaking
    // first — it is here to catch a variant *dropped from the table* while
    // `tag` still knows it.
    assert_eq!(seen.len(), 33, "the table lost a variant: {seen:?}");
}

/// Documents of every JSON shape, so each type meets one it cannot accept.
fn documents() -> Vec<Json> {
    ["null", "[]", "{}", "\"x\"", "1", "true", "{\"_d\":0}"]
        .iter()
        .map(|t| Json::parse(t).expect("test document"))
        .collect()
}

/// Values of every shape a caller might hand across, for the same reason.
fn values() -> Vec<Value> {
    vec![
        Value::WChar('한'),
        Value::LongLong(1),
        Value::ObjRef(None),
        Value::Struct(vec![("nope".into(), Value::LongLong(1))]),
        Value::List(vec![Value::LongLong(1)]),
    ]
}

/// The sweep. **No refusal from either direction may say `<anonymous>`.**
///
/// This is the anti-regression half, and it is what went red before the
/// repair: `from_json(void, null)` answered `"<anonymous> cannot cross yet"`
/// and `to_json(sequence<long>, WChar)` answered `"WChar('한') is not a value
/// of <anonymous>"`.
#[test]
fn no_anyjson_refusal_from_either_direction_says_anonymous() {
    let mut refusals = 0usize;
    for tc in every_variant() {
        for v in values() {
            let mut h = LocalReferences::new();
            if let Err(e) = to_json(&tc, &v, &mut h) {
                refusals += 1;
                assert!(
                    !e.message.contains("<anonymous>"),
                    "to_json({}) named nothing: {e}",
                    tag(&tc)
                );
            }
        }
        for j in documents() {
            if let Err(e) = from_json(&tc, &j, &LocalReferences::new()) {
                refusals += 1;
                assert!(
                    !e.message.contains("<anonymous>"),
                    "from_json({}, {j}) named nothing: {e}",
                    tag(&tc)
                );
            }
        }
    }
    // An unmeasured check is a failure, never a pass: if a future edit makes
    // every one of these documents acceptable, this test would pass while
    // asserting nothing at all.
    assert!(refusals > 100, "the sweep only reached {refusals} refusals");
}

/// The write direction, by name and by equality.
///
/// One case per shape `type_name` had no answer for. The bound is in the
/// subject because it is in the type: `string<5>` and `string` are the same
/// word to AnyJSON v1 and different TypeCode bytes to a peer, which is the
/// distinction D008 exists for.
#[test]
fn a_value_of_the_wrong_shape_is_refused_by_a_name_the_type_actually_has() {
    let cases = [
        (TypeCode::Sequence { element: Box::new(TypeCode::Long), bound: 0 }, "sequence<long>"),
        (TypeCode::Sequence { element: Box::new(TypeCode::Octet), bound: 7 }, "sequence<octet, 7>"),
        (TypeCode::Array { element: Box::new(TypeCode::Long), length: 3 }, "long[3]"),
        (TypeCode::Any, "any"),
        (TypeCode::TypeCode, "typecode"),
        (TypeCode::Void, "void"),
        (TypeCode::Null, "null"),
        (TypeCode::Principal, "principal"),
        (TypeCode::String(5), "string<5>"),
        (TypeCode::WString(5), "wstring<5>"),
        (
            TypeCode::Struct {
                id: "IDL:m/Point:1.0".into(),
                name: "Point".into(),
                members: vec![Member { name: "px".into(), tc: TypeCode::Long }],
            },
            "IDL:m/Point:1.0",
        ),
    ];
    for (tc, want) in cases {
        let mut h = LocalReferences::new();
        let err = to_json(&tc, &Value::WChar('한'), &mut h)
            .expect_err("a wchar is not a value of any of these");
        assert_eq!(err.message, format!("WChar('한') is not a value of {want}"), "{err}");
    }
}

/// The read direction — the one a peer-fed document meets.
///
/// Exactly three types are left to `from_json_at`'s own `"cannot cross yet"`
/// tail: everything else has an arm, and the four the wire cannot carry are
/// guarded before it by their own families' sentences. `orbweaver-test`'s
/// `json_unmapped` states that set in prose; this is where it is measured.
#[test]
fn the_three_types_left_to_cannot_cross_yet_are_refused_by_name() {
    for (tc, want) in
        [(TypeCode::Void, "void"), (TypeCode::Null, "null"), (TypeCode::Principal, "principal")]
    {
        for j in documents() {
            let err =
                from_json(&tc, &j, &LocalReferences::new()).expect_err("none of these can be read");
            assert_eq!(err.message, format!("{want} cannot cross yet"), "{j}: {err}");
        }
    }
}

/// The catch-all's replacement, said as a property: **only `void` crosses as
/// `"void"`.**
///
/// It held before the repair too — all thirty-three variants had an arm on the
/// day it was written, which is exactly why the tail looked harmless. What
/// changed is that the property is now carried by the compiler rather than by
/// a comment, and this test is what says out loud what the compiler is
/// carrying.
#[test]
fn only_void_crosses_as_void_and_every_description_reads_back() {
    for tc in every_variant() {
        let doc = tc_to_json(&tc);
        let text = doc.to_string();
        if matches!(tc, TypeCode::Void) {
            assert_eq!(doc, Json::String("void".into()));
        } else {
            assert_ne!(doc, Json::String("void".into()), "{} crossed as void", tag(&tc));
        }
        // D008: the description crosses even for the four whose *instance*
        // does not, so every one of the thirty-three must read back identical.
        let back = tc_from_json(&doc, "").unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(back, tc, "{text}");
    }
}

/// The navigator's starting value refuses `Principal` without sending anyone
/// to §4.4, and every other variant either starts or says why by name.
///
/// Not a repair of this batch — `default_within` was already exhaustive, and
/// it is the model the two functions above were rewritten against. It is swept
/// here so that the three layers are measured by one file.
#[test]
fn the_navigators_refusals_name_their_construct_too() {
    for tc in every_variant() {
        let Err(e) = dynany::default_value(&tc) else { continue };
        assert!(!e.message.contains("<anonymous>"), "{}: {e}", tag(&tc));
        assert!(!e.message.contains("an indirection"), "{}: {e}", tag(&tc));
    }
}
