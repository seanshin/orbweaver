//! A union with a `default:` branch, from IDL to the wire and back.
//!
//! The registry gives a bare `default:` case an **empty** label — it is the
//! branch selected by *not* matching, so there is no discriminator value to
//! store — and `orbweaver_giop::typecode::encode` used to write that label as
//! it stood: zero bytes. `decode` reads a label of the discriminator's width
//! for every case, as CORBA 3.4 Part 2 Table 9.2 lays it out (`{discriminant
//! type (label value), string (member name), TypeCode (member type)}`), so our
//! own encoding of `corpus/golden/06`'s `WithDefault` could not be read back:
//! the member name's length was read as the label and the decoder failed a
//! field later on "implausible CDR length prefix", in both byte orders.
//! Reported 2026-08-19 by the batch that added `golden/29`, and never red
//! before because every gate that touched the shape ran both ends through the
//! same encoder — a convention both ends apply cannot be refuted by a round
//! trip.
//!
//! §9.3.5.1.4: "The discriminant value used in the actual typecode parameter
//! associated with the default member position in the list, may be any valid
//! value of the discriminant type, and has no semantic significance (i.e., it
//! should be ignored and is only included for syntactic completeness of union
//! type code marshaling)." A label of the discriminator's width, then, whose
//! value the reader ignores — and the decoder does ignore it, handing back
//! the registry's empty label whatever the wire said, which is what makes the
//! comparison below structural rather than a mask.

use std::path::{Path, PathBuf};

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_giop::typecode::{TypeCode, UnionCase, decode, encode};
use orbweaver_registry::Registry;

fn golden(name: &str) -> Registry {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join("corpus/golden").join(name);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let spec =
        orbweaver_idl::parse(&src).unwrap_or_else(|e| panic!("{} must parse: {e}", path.display()));
    let mut reg = Registry::new();
    reg.load(&spec).unwrap_or_else(|e| panic!("{} must load: {e}", path.display()));
    reg
}

/// What a union `TypeCode` must carry through the wire unchanged: everything
/// except the label of the default member, which the wire carries only "for
/// syntactic completeness".
type Shape = (String, String, TypeCode, i32, Vec<(Vec<u8>, String, TypeCode)>);

fn shape(tc: &TypeCode) -> Shape {
    let TypeCode::Union { id, name, discriminator, default_index, cases } = tc else {
        panic!("not a union: {tc:?}")
    };
    let cases = cases
        .iter()
        .enumerate()
        .map(|(i, UnionCase { label, name, tc })| {
            let is_default = *default_index >= 0 && i == *default_index as usize;
            (if is_default { Vec::new() } else { label.clone() }, name.clone(), tc.clone())
        })
        .collect();
    (id.clone(), name.clone(), (**discriminator).clone(), *default_index, cases)
}

/// A bare-`default:` union over each discriminator kind that gives a label a
/// different width, shaped as the registry produces them: an empty label at
/// `default_index`.
fn bare_defaults() -> Vec<(&'static str, TypeCode, usize)> {
    let hue = TypeCode::Enum {
        id: "IDL:x/Hue:1.0".into(),
        name: "Hue".into(),
        members: vec!["RED".into(), "GREEN".into(), "BLUE".into()],
    };
    [
        ("short", TypeCode::Short, vec![0, 1], 2usize),
        ("boolean", TypeCode::Boolean, vec![1], 1),
        ("char", TypeCode::Char, vec![b'a'], 1),
        ("enum", hue, vec![0, 0, 0, 0], 4),
        ("long long", TypeCode::LongLong, vec![0, 0, 0, 0, 0, 0, 0, 1], 8),
        ("unsigned long", TypeCode::ULong, vec![0, 0, 0, 1], 4),
    ]
    .into_iter()
    .map(|(what, disc, first, width)| {
        (
            what,
            TypeCode::Union {
                id: format!("IDL:x/{what}:1.0"),
                name: "U".into(),
                discriminator: Box::new(disc),
                default_index: 1,
                cases: vec![
                    UnionCase { label: first, name: "a".into(), tc: TypeCode::Long },
                    UnionCase { label: Vec::new(), name: "b".into(), tc: TypeCode::String(0) },
                ],
            },
            width,
        )
    })
    .collect()
}

/// Every union with a `default:` in the two golden files plus the kinds above,
/// one pass, both byte orders, every failure reported before any assertion —
/// a list of items with one cause reads as one cause, a stop at the first item
/// does not.
///
/// `golden/29`'s three unions all label their default (`case 2: default:`),
/// so the registry stores a value for that case and they were never affected;
/// they are here because a fix that made a bare default read back by breaking
/// a labelled one would still be a fix for one item.
#[test]
fn every_defaulted_union_reads_back_as_itself() {
    let mut unions: Vec<(String, TypeCode)> = [
        ("06-union.idl", "IDL:gc06/WithDefault:1.0"),
        ("29-labelled-default.idl", "IDL:gc29/Coded:1.0"),
        ("29-labelled-default.idl", "IDL:gc29/Spread:1.0"),
        ("29-labelled-default.idl", "IDL:gc29/Tint:1.0"),
    ]
    .into_iter()
    .map(|(file, id)| {
        (id.to_owned(), golden(file).typecode(id).unwrap_or_else(|| panic!("{file}: {id}")).clone())
    })
    .collect();
    unions.extend(bare_defaults().into_iter().map(|(what, tc, _)| (what.to_owned(), tc)));

    let mut failures = Vec::new();
    for (id, tc) in &unions {
        let TypeCode::Union { default_index, .. } = tc else { panic!("{id} is not a union") };
        assert!(*default_index >= 0, "{id} has a default branch, which is the point");
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            encode(&mut e, tc).expect("encode");
            let bytes = e.finish().expect("finish");
            match decode(&mut Decoder::new(&bytes, endian)) {
                Err(err) => failures.push(format!("{id} {endian:?}: does not decode: {err}")),
                Ok(back) if shape(&back) != shape(tc) => failures.push(format!(
                    "{id} {endian:?}: decoded to a different union:\n    sent {tc:?}\n    got  {back:?}"
                )),
                Ok(_) => {}
            }
        }
    }
    assert!(failures.is_empty(), "{} failure(s):\n  {}", failures.len(), failures.join("\n  "));
}

/// The default member's label occupies the discriminator's width on the wire,
/// whatever value it holds. Checked on the bytes, not through the decoder,
/// because a decoder that skipped the label and an encoder that wrote none
/// would agree with each other perfectly.
#[test]
fn the_default_label_is_written_at_the_discriminator_width() {
    let mut unions = bare_defaults();
    unions.push((
        "IDL:gc06/WithDefault:1.0",
        golden("06-union.idl").typecode("IDL:gc06/WithDefault:1.0").expect("gc06").clone(),
        4,
    ));
    let mut failures = Vec::new();
    for (id, tc, width) in unions {
        let TypeCode::Union { cases, default_index, .. } = &tc else {
            panic!("{id} is not a union")
        };
        assert!(
            cases[*default_index as usize].label.is_empty(),
            "{id}: a bare default has no label"
        );
        // The same union with a value in the default's label slot. The wire
        // never sees that value, but always sees its width, so the two must
        // encode to the same length — the label is a slot, not an option.
        let mut valued = tc.clone();
        if let TypeCode::Union { cases, .. } = &mut valued {
            cases[*default_index as usize].label = vec![0x7f; width];
        }
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            encode(&mut e, &tc).expect("encode");
            let bare = e.finish().expect("finish");
            let mut e = Encoder::new(endian);
            encode(&mut e, &valued).expect("encode");
            let valued = e.finish().expect("finish");
            if bare.len() != valued.len() {
                failures.push(format!(
                    "{id} {endian:?}: a bare default encodes to {} bytes, a valued one to {}",
                    bare.len(),
                    valued.len()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{} failure(s):\n  {}", failures.len(), failures.join("\n  "));
}
