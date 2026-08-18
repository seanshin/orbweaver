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

/// A value for `tc`, minimal but complete — every member present.
fn witness(tc: &TypeCode, depth: usize) -> Option<Value> {
    if depth > 8 {
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
        TypeCode::Sequence { .. } => Value::List(Vec::new()),
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
    for id in wanted {
        let tc = reg
            .typecode(id)
            .unwrap_or_else(|| panic!("{id} is not in the IFR subset contract any more"))
            .clone();
        let v = witness(&tc, 0)
            .unwrap_or_else(|| panic!("{id} has a member with no value shape at all"));

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
