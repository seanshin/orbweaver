//! One sentence for §4.4, whichever layer a reader happens to hit.
//!
//! `docs/PLAN.md` §4.4 defers `valuetype`, abstract interfaces and `fixed` from
//! the v1 wire, and three layers refuse an *instance* of one: the CDR path
//! (`orbweaver_dynamic::encode`/`decode`), the AnyJSON path
//! (`anyjson::to_json`/`from_json`), and the generated Python runtime
//! (`crates/orbweaver-gen/src/python_rt.py`, `_DEFERRED`). Two of the three
//! named the section; the AnyJSON layer said `"tk_value cannot cross yet"` and
//! `"Struct([…]) is not a value of IDL:m/Money:1.0"` — and the AnyJSON layer is
//! the one a peer-fed document actually meets.
//!
//! What this file pins is the **pair inside this crate**. The Python half is
//! held to the same sentence by `orbweaver-gen`'s `python_target`, because only
//! that crate can run the runtime it emits, and Python cannot share a Rust
//! constant. Two tests, one sentence, and neither of them is hope.
//!
//! # What must never become symmetric
//!
//! D008 decided that a `TypeCode` is a value whose AnyJSON form is the
//! structural one, and that has to hold for a type whose instances this ORB
//! will not marshal: a **description** of a deferred type crosses, an
//! **instance** does not. So the refusal has to say which half was refused, or
//! a reader who meets it concludes the type is unreachable and stops sending
//! the description too. That is what the tail of the sentence is for, and the
//! description half is asserted here beside the refusal so the two cannot drift
//! into agreement.

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_dynamic::anyjson::{LocalReferences, from_json, tc_from_json, tc_to_json, to_json};
use orbweaver_dynamic::{Value, decode, encode};
use orbweaver_giop::typecode::{Member, TypeCode, ValueMember};

/// §4.4's three constructs, each with the words a refusal has to name it by.
fn deferred() -> Vec<(TypeCode, &'static str)> {
    vec![
        (
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
            "valuetype Money",
        ),
        (
            TypeCode::AbstractInterface {
                id: "IDL:m/Describable:1.0".into(),
                name: "Describable".into(),
            },
            "abstract interface Describable",
        ),
        (TypeCode::Fixed { digits: 9, scale: 2 }, "fixed<9,2>"),
    ]
}

/// The whole sentence, built here from its parts rather than imported, so that
/// changing the wording in `lib.rs` is a change this test has to be told about.
fn sentence(what: &str) -> String {
    format!(
        "{what} is not marshalled by the v1 wire (docs/PLAN.md §4.4); the TypeCode describing it \
         reads, the value behind it does not"
    )
}

/// Every AnyJSON refusal of a deferred type is that one sentence, in both
/// directions — and the CDR path's read direction is the same sentence again.
///
/// `from_json` is the direction that matters most: a document naming one of
/// these was written by a *peer*, and `{"_t": {"kind":"value",…}, "_v": {…}}`
/// is exactly what a conformant sender produces for a `valuetype`. The reader
/// has to learn that the `_t` half was understood and the `_v` half is where v1
/// stops, which is a different fact from "this runtime has a hole".
#[test]
fn deferred_sentences_agree_across_the_layers() {
    for (tc, what) in deferred() {
        let want = sentence(what);

        // ── AnyJSON, out ────────────────────────────────────────────────────
        // Every `Value` shape a caller might reach for, including the two that
        // would have looked plausible: a valuetype's state is member-by-member
        // like a struct's, and until 2026-08-20 the registry recorded one as an
        // object reference.
        for v in [
            Value::Struct(vec![("units".into(), Value::LongLong(1))]),
            Value::ObjRef(None),
            Value::LongLong(1),
        ] {
            let mut h = LocalReferences::new();
            let err = to_json(&tc, &v, &mut h).expect_err("an instance has no AnyJSON form");
            assert_eq!(err.message, want, "anyjson::to_json({what})");
        }

        // ── AnyJSON, in ─────────────────────────────────────────────────────
        for text in ["{}", "null", "{\"units\":\"1\"}", "[]"] {
            let j = orbweaver_dynamic::json::Json::parse(text).expect("test document");
            let err = from_json(&tc, &j, &LocalReferences::new())
                .expect_err("an instance has no AnyJSON form");
            assert_eq!(err.message, want, "anyjson::from_json({what}, {text})");
        }

        // ── and it travels ──────────────────────────────────────────────────
        // A struct holding one is refused at the member, with the member's path
        // in front of the reason, rather than written as far as the member.
        let holder = TypeCode::Struct {
            id: "IDL:m/Holder:1.0".into(),
            name: "Holder".into(),
            members: vec![Member { name: "body".into(), tc: tc.clone() }],
        };
        let err = from_json(
            &holder,
            &orbweaver_dynamic::json::Json::parse("{\"body\": {}}").expect("test document"),
            &LocalReferences::new(),
        )
        .expect_err("a struct holding a deferred type has no AnyJSON form");
        assert_eq!(err.path, "body", "{err}");
        assert_eq!(err.message, want, "{err}");
    }

    // ── the CDR path, read direction ────────────────────────────────────────
    // Word for word the same, both byte orders. `fixed` is not in this loop and
    // the next assertion is why.
    for (tc, what) in deferred().into_iter().filter(|(t, _)| !matches!(t, TypeCode::Fixed { .. })) {
        for endian in [Endian::Big, Endian::Little] {
            let err = decode(&mut Decoder::new(&[0u8; 16], endian), &tc)
                .expect_err("an instance cannot be read");
            assert_eq!(err.message, sentence(what), "decode({what}, {endian:?})");

            // The write direction shares the head and then says what the writer
            // could not do, which is a different fact from what the reader could
            // not do. The head is the part a reader searches on.
            let err = encode(&mut Encoder::new(endian), &tc, &Value::ObjRef(None))
                .expect_err("an instance has no encoding");
            let head = format!("{what} is not marshalled by the v1 wire (docs/PLAN.md §4.4)");
            assert!(err.message.starts_with(&head), "encode({what}, {endian:?}): {err}");
        }
    }
}

/// Measured, not desired: the CDR path has no `fixed` arm at all, so its
/// refusal names neither the construct nor §4.4.
///
/// `fixed` never reaches the CDR encoder in practice — there is no
/// `Value::Fixed` to hand it and both emitters skip the declaration — so
/// nothing has ever been wrong on the wire because of this. It is still the
/// third layer disagreeing with the other two about the same construct, and
/// the AnyJSON layer now says the sentence for all three, so the gap is here
/// rather than everywhere.
///
/// **Out of this batch's footprint** (`crates/orbweaver-dynamic/src/lib.rs`
/// beyond the shared sentence), reported instead of fixed. The day a `fixed`
/// arm lands, this test goes red: delete it and drop the `filter` above.
#[test]
fn the_cdr_path_does_not_yet_name_the_section_for_fixed() {
    let tc = TypeCode::Fixed { digits: 9, scale: 2 };
    let err =
        decode(&mut Decoder::new(&[0u8; 16], Endian::Big), &tc).expect_err("no value of it reads");
    assert_eq!(err.message, "cannot decode fixed yet", "{err}");
    let err = encode(&mut Encoder::new(Endian::Big), &tc, &Value::LongLong(1))
        .expect_err("no value of it writes");
    assert_eq!(err.message, "expected a value of type fixed, got a long long", "{err}");
}

/// The other half of D008, asserted beside the refusal it must not become.
///
/// All three descriptions cross structurally and come back byte-identical. If
/// a future edit ever makes `tc_to_json` refuse a deferred type "for
/// consistency", this is the test that says what was lost: a peer's `any` that
/// merely *mentions* a valuetype becomes unreadable in its entirety, for a
/// value nobody was going to ask for.
#[test]
fn the_description_of_a_deferred_type_still_crosses_both_ways() {
    for (tc, what) in deferred() {
        let doc = tc_to_json(&tc);
        let text = doc.to_string();
        assert!(!text.contains("void"), "{what} fell through to a short name: {text}");
        let back = tc_from_json(&doc, "").unwrap_or_else(|e| panic!("{what}: {text}: {e}"));
        assert_eq!(back, tc, "{what}: {text}");

        // And as a value in its own right, which is the slot an IFR description
        // actually occupies.
        let mut h = LocalReferences::new();
        let carried = Value::TypeCode(Box::new(tc.clone()));
        let j = to_json(&TypeCode::TypeCode, &carried, &mut h)
            .unwrap_or_else(|e| panic!("{what} as a value: {e}"));
        let read = from_json(&TypeCode::TypeCode, &j, &h)
            .unwrap_or_else(|e| panic!("{what} as a value, back: {e}"));
        assert_eq!(read, carried, "{what}");
    }
}
