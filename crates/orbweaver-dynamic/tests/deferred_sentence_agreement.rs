//! One sentence per family, whichever layer a reader happens to hit — **and
//! three families, not one**.
//!
//! `docs/PLAN.md` §4.4 defers `valuetype`, abstract interfaces and `fixed` from
//! the v1 wire. `native X;` is a fourth construct the wire cannot carry and it
//! is **not** deferred: §4.4's three have a wire form the specification defines
//! and this version has not implemented, and a native has none to implement, in
//! v1 or in any later version. `::CORBA::Principal` is a fifth and is neither:
//! GIOP 1.0 carried one in every request header and CORBA 3.0 removed the type,
//! so the wire form existed and was **withdrawn**. Five layers refuse an
//! *instance* of one of the five: the CDR path
//! (`orbweaver_dynamic::encode`/`decode`), the AnyJSON path
//! (`anyjson::to_json`/`from_json`), the dynamic navigator's starting value
//! (`dynany::default_value`), and the generated Python runtime
//! (`crates/orbweaver-gen/src/python_rt.py`, `_DEFERRED`, `_UNMARSHALLABLE`
//! and `_WITHDRAWN`).
//!
//! Two of the three §4.4 layers named the section until 2026-08-21; the AnyJSON
//! layer said `"tk_value cannot cross yet"` and `"Struct([…]) is not a value of
//! IDL:m/Money:1.0"`, and the AnyJSON layer is the one a peer-fed document
//! actually meets. `native` arrived a day later with the helper not on its
//! branch and repeated the whole shape in one commit: **five sentences for one
//! fact**, of which two told the reader something false — AnyJSON's read
//! direction answered `"IDL:m/Handle:1.0 cannot cross yet"` and the navigator's
//! default pointed at §4.4. Both invite the reader to wait for a release that
//! will never carry it.
//!
//! What this file pins is the **Rust set inside this crate**. The Python half
//! is held to the same two sentences by `orbweaver-gen`'s `python_target`,
//! because only that crate can run the runtime it emits, and Python cannot
//! share a Rust constant.
//!
//! # What must never become symmetric
//!
//! Two things, and they are different things.
//!
//! **Description versus instance.** D008 decided that a `TypeCode` is a value
//! whose AnyJSON form is the structural one, and that has to hold for a type
//! whose instances this ORB will not marshal: a *description* crosses, an
//! *instance* does not. So the refusal has to say which half was refused, or a
//! reader who meets it concludes the type is unreachable and stops sending the
//! description too.
//!
//! **Deferred versus never versus withdrawn.** The three families' tails say
//! different things and swapping any two is a lie: §4.4's says the description
//! still crosses and the value is waiting on this project; a native's says
//! there is nothing to wait for because there was never a wire form, and the
//! fix is to declare in IDL what the language type holds; a withdrawn type's
//! says there is nothing to wait for because the specification took the wire
//! form away, and the fix is to find where the OMG moved the thing you wanted.
//! So neither of the last two may contain the §4.4 deferral head, and neither
//! may contain the word **"yet"** — those are the two ways a reader is told
//! something false, and both were live in shipped code for *each* of those two
//! families, five days apart, in the same two layers. Asserted below, beside
//! the equalities.

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_dynamic::anyjson::{LocalReferences, from_json, tc_from_json, tc_to_json, to_json};
use orbweaver_dynamic::dynany::default_value;
use orbweaver_dynamic::{Value, decode, encode};
use orbweaver_giop::typecode::{Member, TypeCode, ValueMember};

/// Which boundary a construct hits, which is which sentence it is refused with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Boundary {
    /// `docs/PLAN.md` §4.4: a wire form the specification defines and this
    /// version has not implemented.
    Deferred,
    /// No wire form to implement, in this version or any later one.
    Never,
    /// A wire form the specification defined and then **removed**. Not
    /// [`Boundary::Deferred`] — nothing is coming — and not [`Boundary::Never`]
    /// — conformant ORBs marshalled one for a decade. The distinction is what
    /// a contract author does next: a native means "you modelled a language
    /// type", a withdrawn one means "the OMG moved this".
    Withdrawn,
}

/// The five constructs, each with the words a refusal has to name it by and the
/// boundary it hits.
fn families() -> Vec<(TypeCode, &'static str, Boundary)> {
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
            "valuetype Money (IDL:m/Money:1.0)",
            Boundary::Deferred,
        ),
        (
            TypeCode::AbstractInterface {
                id: "IDL:m/Describable:1.0".into(),
                name: "Describable".into(),
            },
            "abstract interface Describable (IDL:m/Describable:1.0)",
            Boundary::Deferred,
        ),
        (TypeCode::Fixed { digits: 9, scale: 2 }, "fixed<9,2>", Boundary::Deferred),
        (
            TypeCode::Native { id: "IDL:m/Handle:1.0".into(), name: "Handle".into() },
            "native Handle (IDL:m/Handle:1.0)",
            Boundary::Never,
        ),
        // The fifth. No constructor arguments, because there is nothing to
        // vary: `tk_Principal` is a primitive kind with no name, no id and no
        // members, which is also why its subject is a fixed string rather than
        // something built from a `TypeCode`'s parts.
        (TypeCode::Principal, PRINCIPAL, Boundary::Withdrawn),
    ]
}

/// The fifth family's subject, spelled here rather than imported for the reason
/// the heads below are: a change to `orbweaver_dynamic::principal_subject` is a
/// change this test has to be told about.
const PRINCIPAL: &str = "predeclared type ::CORBA::Principal (IDL:omg.org/CORBA/Principal:1.0)";

/// The §4.4 head, built here from its parts rather than imported, so that
/// changing the wording in `lib.rs` is a change this test has to be told about.
fn deferred_head(what: &str) -> String {
    format!("{what} is not marshalled by the v1 wire (docs/PLAN.md §4.4)")
}

/// The whole sentence a peer-fed §4.4 construct is refused with.
fn deferred_sentence(what: &str) -> String {
    format!(
        "{}; the TypeCode describing it reads, the value behind it does not",
        deferred_head(what)
    )
}

/// The fourth family's head — the counterpart of [`deferred_head`], written out
/// for the same reason.
fn never_head(what: &str) -> String {
    format!(
        "{what} has no wire form at all: it names a type only a language mapping knows, and no \
         version of the wire marshals one"
    )
}

/// The whole sentence a peer-fed `native` is refused with. Its tail says the
/// opposite of [`deferred_sentence`]'s, which is the whole point of it.
fn never_sentence(what: &str) -> String {
    format!(
        "{}; this is not one of docs/PLAN.md §4.4's deferrals — those have a wire form this \
         version has not implemented, and there is none here to implement",
        never_head(what)
    )
}

/// The fifth family's head, written out for the reason the other two are.
///
/// It names no section at all — see [`withdrawn_sentence`] for where the
/// denial goes and why it is in the tail rather than here.
fn withdrawn_head(what: &str) -> String {
    format!(
        "{what} was withdrawn from CORBA: GIOP 1.0 carried one in every request header, GIOP 1.1 \
         dropped that field and CORBA 3.0 removed the type — so this version marshals no value \
         for one, and no later version will"
    )
}

/// The whole sentence a peer-fed `Principal` is refused with.
///
/// Two tails, and both are load-bearing. D008's asymmetry is the first —
/// `tc_to_json` writes `{"kind":"principal"}` and `tc_from_json` reads it back,
/// so the description crosses whole and only the value stops. The denial is the
/// second: a reader who met §4.4 in this project's other refusals will search
/// for it, and finding nothing is not the same as being told the section does
/// not apply.
fn withdrawn_sentence(what: &str) -> String {
    format!(
        "{}; the TypeCode describing it reads, the value behind it does not. This is not one of \
         docs/PLAN.md §4.4's deferrals: those wait on this project, and a type the specification \
         has removed waits on nobody",
        withdrawn_head(what)
    )
}

fn head(what: &str, b: Boundary) -> String {
    match b {
        Boundary::Deferred => deferred_head(what),
        Boundary::Never => never_head(what),
        Boundary::Withdrawn => withdrawn_head(what),
    }
}

fn sentence(what: &str, b: Boundary) -> String {
    match b {
        Boundary::Deferred => deferred_sentence(what),
        Boundary::Never => never_sentence(what),
        Boundary::Withdrawn => withdrawn_sentence(what),
    }
}

/// Every layer's refusal of an unmarshallable construct comes from its family's
/// one source — asserted by **equality**, so a layer that invents its own
/// sentence goes red rather than merely reading differently.
///
/// `from_json` is the direction that matters most: a document naming one of
/// these was written by a *peer*, and `{"_t": {"kind":"value",…}, "_v": {…}}`
/// is exactly what a conformant sender produces for a `valuetype`. The reader
/// has to learn that the `_t` half was understood and the `_v` half is where v1
/// stops, which is a different fact from "this runtime has a hole".
#[test]
fn refusals_agree_across_the_layers() {
    for (tc, what, boundary) in families() {
        let want = sentence(what, boundary);

        // ── AnyJSON, out ────────────────────────────────────────────────────
        // Every `Value` shape a caller might reach for, including the two that
        // would have looked plausible: a valuetype's state is member-by-member
        // like a struct's, and until 2026-08-20 the registry recorded one as an
        // object reference — and a `native` until 2026-08-21.
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

        // ── the navigator's starting value ──────────────────────────────────
        // Its own tail on the shared head, the same shape the CDR write
        // direction uses: what a *navigator* could not do is a different fact
        // from what a reader could not do, and the head is the searchable part.
        let err = default_value(&tc).expect_err("there is no value to start one at");
        assert_eq!(
            err.message,
            format!("{}, so there is nothing to start a value of it at", head(what, boundary)),
            "dynany::default_value({what})"
        );

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
        .expect_err("a struct holding an unmarshallable type has no AnyJSON form");
        assert_eq!(err.path, "body", "{err}");
        assert_eq!(err.message, want, "{err}");

        // A `typedef` renames a construct without giving the wire a way to
        // carry it, so the alias resolves to the same sentence about the same
        // construct — not about the alias.
        let alias = TypeCode::Alias {
            id: "IDL:m/Renamed:1.0".into(),
            name: "Renamed".into(),
            aliased: Box::new(tc.clone()),
        };
        let err = default_value(&alias).expect_err("an alias carries nothing new");
        assert!(err.message.starts_with(&head(what, boundary)), "typedef {what}: {err}");
    }

    // ── the CDR path ────────────────────────────────────────────────────────
    // Word for word, both byte orders. `fixed` is not in this loop and
    // `the_cdr_path_does_not_yet_name_the_section_for_fixed` is why.
    for (tc, what, boundary) in
        families().into_iter().filter(|(t, _, _)| !matches!(t, TypeCode::Fixed { .. }))
    {
        for endian in [Endian::Big, Endian::Little] {
            let err = decode(&mut Decoder::new(&[0u8; 16], endian), &tc)
                .expect_err("an instance cannot be read");
            assert_eq!(err.message, sentence(what, boundary), "decode({what}, {endian:?})");

            let err = encode(&mut Encoder::new(endian), &tc, &Value::ObjRef(None))
                .expect_err("an instance has no encoding");
            match boundary {
                // §4.4's write direction shares the head and then says what the
                // writer could not do, which is a different fact from what the
                // reader could not do. The head is the part a reader searches on.
                Boundary::Deferred => assert!(
                    err.message.starts_with(&head(what, boundary)),
                    "encode({what}, {endian:?}): {err}"
                ),
                // A native's two directions are the same fact — neither will
                // ever be implemented — so the write direction keeps no tail of
                // its own and the whole sentence is equal. A withdrawn type is
                // the same shape for a different reason: the specification
                // removed both directions at once.
                Boundary::Never | Boundary::Withdrawn => {
                    assert_eq!(err.message, sentence(what, boundary), "encode({what}, {endian:?})")
                }
            }
        }
    }
}

/// The distinction itself, asserted rather than left to the wording.
///
/// These are the two ways a reader is told something false about a `native`,
/// and both were live in shipped code on 2026-08-21: the AnyJSON read direction
/// said `"cannot cross yet"` and the navigator's default said `"see
/// docs/PLAN.md §4.4"`. Neither is a typo — each sends the reader to wait for a
/// version that will never exist, or to a plan entry that does not name the
/// construct and never will. A sentence that merely *mentions* §4.4 is fine and
/// necessary; what must not appear is §4.4's **deferral claim**, which is what
/// `deferred_head` spells.
///
/// # And the fifth family, held to the same two prohibitions
///
/// `Principal` is swept by this test as of 2026-08-26, and it is the case that
/// shows the two prohibitions are about the *reader* rather than about
/// `native`. Both falsehoods were live in shipped code for this construct too,
/// in the same two layers: `from_json` answered `"principal cannot cross yet"`
/// and `decode` `"cannot decode principal yet"` — found by asking what the
/// fifth family says, not by asking what a `Principal` says, because nobody was
/// going to grep for a sentence that had no home.
#[test]
fn a_type_with_no_later_version_is_never_called_deferred_and_never_says_yet() {
    for (tc, what, boundary) in families() {
        if boundary == Boundary::Deferred {
            continue;
        }
        let sentences = {
            let mut h = LocalReferences::new();
            vec![
                encode(&mut Encoder::new(Endian::Little), &tc, &Value::ObjRef(None))
                    .expect_err("no encoding")
                    .message,
                decode(&mut Decoder::new(&[0u8; 16], Endian::Little), &tc)
                    .expect_err("no bytes")
                    .message,
                to_json(&tc, &Value::Struct(Vec::new()), &mut h).expect_err("no form").message,
                from_json(
                    &tc,
                    &orbweaver_dynamic::json::Json::parse("{}").expect("document"),
                    &LocalReferences::new(),
                )
                .expect_err("no form")
                .message,
                default_value(&tc).expect_err("no starting value").message,
                sentence(what, boundary),
                head(what, boundary),
            ]
        };
        for s in &sentences {
            assert!(s.contains(what), "a refusal must name the construct: {s}");
            assert!(
                !s.contains(&deferred_head(what)),
                "{boundary:?}: this must not carry §4.4's deferral claim: {s}"
            );
            assert!(
                !s.contains("yet"),
                "{boundary:?}: this is not waiting on an implementation: {s}"
            );
            assert_ne!(s, &deferred_sentence(what), "the families' sentences are not one");
        }
        // And the denial is said out loud rather than merely omitted: a reader
        // who searched for §4.4 because another layer named it has to find the
        // sentence that says the section does not apply.
        let whole = sentence(what, boundary);
        assert!(
            whole.to_lowercase().contains("this is not one of docs/plan.md §4.4's deferrals"),
            "{whole}"
        );
        // The heads are three sentences, not one with variations. Swapping any
        // two reads as correct and tells the contract author to do the wrong
        // thing next.
        assert_ne!(head(what, boundary), deferred_head(what));
        if boundary == Boundary::Withdrawn {
            assert_ne!(head(what, boundary), never_head(what), "a withdrawn type is not a native");
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
/// **Out of the 2026-08-21 batch's footprint** (`crates/orbweaver-dynamic/src/lib.rs`
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
/// All four descriptions cross structurally and come back byte-identical. If
/// a future edit ever makes `tc_to_json` refuse one of them "for consistency",
/// this is the test that says what was lost: a peer's `any` that merely
/// *mentions* a valuetype becomes unreadable in its entirety, for a value
/// nobody was going to ask for. The `native` is here for a stronger version of
/// the same reason — nothing will ever carry its value, so the description is
/// all a peer can be told about it.
#[test]
fn the_description_of_an_unmarshallable_type_still_crosses_both_ways() {
    for (tc, what, _) in families() {
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
