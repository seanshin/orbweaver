//! Whether an agent can read an Interface Repository through the bridge.
//!
//! The bridge speaks AnyJSON (§4.5), so before D008 it could not: every
//! Interface Repository description is made of `::CORBA::TypeCode`, the
//! mapping had no form for one, and the ten items `gen-python` skipped over
//! `corpus/services/ir-subset.idl` were the same ten an agent could not read.
//!
//! This is a claim about the **agent path**, so it is asserted over the real
//! contract, by repository id, rather than over a TypeCode written here. A
//! test that builds its own type proves the codec; only the contract proves
//! the reach.

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_dynamic::anyjson::{LocalReferences, from_json, to_json};
use orbweaver_dynamic::{Value, encode};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_registry::Registry;

fn ir_subset() -> Registry {
    let text = std::fs::read_to_string("../../corpus/services/ir-subset.idl")
        .expect("corpus/services/ir-subset.idl");
    let spec = orbweaver_idl::parse(&text).expect("the IFR subset must parse");
    let mut reg = Registry::new();
    reg.load(&spec).expect("the IFR subset must load");
    reg
}

/// How deep a witness nests before a sequence is allowed to be empty.
///
/// The IFR subset has no recursive type, so no description here reaches it;
/// it is the same ceiling `prop.rs`'s sampler keeps, and it exists so that the
/// one shape which *would* need it — a description that one day names itself
/// through a sequence — terminates as an empty list at the ceiling instead of
/// never, rather than so that anything below the ceiling may be empty.
const MAX_DEPTH: usize = 8;

/// A value for `tc`, minimal but complete — every member present, and **every
/// sequence carrying one element** below [`MAX_DEPTH`].
///
/// It used to give `Sequence => []` unconditionally, which made this the third
/// witness (after `prop.rs`'s and `python_target.rs`'s, 1b6b4c8) whose green
/// proved nothing about a sequence's *contents*: `OperationDescription`'s
/// parameters, exceptions and contexts, and `FullInterfaceDescription`'s
/// operations and attributes — the parts of an IFR answer an agent actually
/// reads — crossed as `[]` every time, so a mapping that mangled a
/// `ParameterDescription` inside `ParDescriptionSeq` would have passed here.
/// Now the element is produced one level down and a member without a value
/// shape fails the test loudly through `?` instead of hiding as an empty list.
fn witness(tc: &TypeCode, depth: usize) -> Option<Value> {
    if depth > MAX_DEPTH {
        return None;
    }
    Some(match tc {
        TypeCode::Boolean => Value::Bool(true),
        TypeCode::Octet => Value::Octet(1),
        TypeCode::Char => Value::Char(b'x'),
        TypeCode::WChar => Value::WChar('한'),
        TypeCode::Short => Value::Short(-1),
        TypeCode::UShort => Value::UShort(1),
        TypeCode::Long => Value::Long(-1),
        TypeCode::ULong => Value::ULong(1),
        TypeCode::LongLong => Value::LongLong(-1),
        TypeCode::ULongLong => Value::ULongLong(1),
        TypeCode::Float => Value::Float(0.5),
        TypeCode::Double => Value::Double(0.25),
        TypeCode::String(_) => Value::String("x".into()),
        TypeCode::WString(_) => Value::WString("한".into()),
        TypeCode::Enum { members, .. } => Value::Enum(members.first()?.clone()),
        // One element, at the elements' depth. The empty list is the honest
        // value only at the ceiling; anywhere else it is a witness that
        // measures nothing about what the sequence carries.
        TypeCode::Sequence { element, .. } => {
            if depth >= MAX_DEPTH {
                Value::List(Vec::new())
            } else {
                Value::List(vec![witness(element, depth + 1)?])
            }
        }
        TypeCode::Array { element, length } => {
            Value::List((0..*length).map(|_| witness(element, depth + 1)).collect::<Option<_>>()?)
        }
        TypeCode::ObjRef { .. } => Value::ObjRef(None),
        TypeCode::Alias { aliased, .. } => witness(aliased, depth + 1)?,
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => Value::Struct(
            members
                .iter()
                .map(|m| Some((m.name.clone(), witness(&m.tc, depth + 1)?)))
                .collect::<Option<Vec<_>>>()?,
        ),
        // The point of the exercise.
        TypeCode::TypeCode => Value::TypeCode(Box::new(TypeCode::Sequence {
            element: Box::new(TypeCode::Long),
            bound: 0,
        })),
        _ => return None,
    })
}

/// How many lists in `v` carry at least one element, and how many carry none.
fn lists(v: &Value) -> (usize, usize) {
    match v {
        Value::List(items) => {
            let (mut full, mut empty) = if items.is_empty() { (0, 1) } else { (1, 0) };
            for item in items {
                let (f, e) = lists(item);
                full += f;
                empty += e;
            }
            (full, empty)
        }
        Value::Struct(members) => members.iter().fold((0, 0), |(f, e), (_, m)| {
            let (mf, me) = lists(m);
            (f + mf, e + me)
        }),
        _ => (0, 0),
    }
}

/// Every description the IFR facade hands back must survive the agent's
/// mapping and reproduce the same CDR. Named by repository id, so a contract
/// that drops one fails here rather than quietly shrinking the claim.
#[test]
fn every_ifr_description_crosses_to_the_agent_and_back() {
    let reg = ir_subset();
    let wanted = [
        "IDL:omg.org/CORBA/AttributeDescription:1.0",
        "IDL:omg.org/CORBA/ParameterDescription:1.0",
        "IDL:omg.org/CORBA/ExceptionDescription:1.0",
        "IDL:omg.org/CORBA/OperationDescription:1.0",
        "IDL:omg.org/CORBA/InterfaceDef/FullInterfaceDescription:1.0",
    ];
    // The two descriptions made of sequences of the others, and what an agent
    // reads out of an IFR answer. Named, so that a description losing its
    // sequence members fails here rather than quietly shrinking the claim.
    let carries_sequences = [
        "IDL:omg.org/CORBA/OperationDescription:1.0",
        "IDL:omg.org/CORBA/InterfaceDef/FullInterfaceDescription:1.0",
    ];
    for id in wanted {
        let tc = reg
            .typecode(id)
            .unwrap_or_else(|| panic!("{id} is not in the IFR subset contract any more"))
            .clone();
        let v = witness(&tc, 0)
            .unwrap_or_else(|| panic!("{id} has a member with no value shape at all"));
        // The witness is worth the assertions below only if its sequences
        // hold something. This is the check that was missing while the
        // witness gave `[]` for every sequence and the test stayed green.
        let (full, empty) = lists(&v);
        if carries_sequences.contains(&id) {
            assert!(full >= 1, "{id}: every sequence in the witness is empty ({empty} of them)");
            assert_eq!(empty, 0, "{id}: {empty} sequence(s) crossed empty and proved nothing");
        }

        let mut h = LocalReferences::new();
        let j = to_json(&tc, &v, &mut h)
            .unwrap_or_else(|e| panic!("{id}: an agent cannot be shown this: {e}"));
        let back = from_json(&tc, &j, &h)
            .unwrap_or_else(|e| panic!("{id}: an agent cannot send this back: {e}"));

        // Passing is not enough: it must pass *because* a structural TypeCode
        // crossed. A contract that quietly lost its `::CORBA::TypeCode`
        // members would round-trip perfectly and prove nothing, which is the
        // shape of every green test that stopped testing anything.
        let text = j.to_string();
        assert!(
            text.contains("\"kind\":\"seq\""),
            "{id} crossed without a structural TypeCode in it: {text}"
        );

        for endian in [Endian::Big, Endian::Little] {
            let mut a = Encoder::new(endian);
            encode(&mut a, &tc, &v).expect("encode original");
            let mut b = Encoder::new(endian);
            encode(&mut b, &tc, &back).expect("encode round-tripped");
            assert_eq!(a.finish().unwrap(), b.finish().unwrap(), "{id} at {endian:?}: {j}");
        }
    }
}
