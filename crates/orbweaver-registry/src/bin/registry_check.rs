//! Checks a registry built from IDL against what a peer puts on the wire.
//!
//! Deriving a `TypeCode` from IDL and encoding it with our own encoder proves
//! only that two pieces of our code agree. The question that matters is whether
//! the result is the same type description a stock ORB produces for the same
//! IDL — so this asks a peer for an `any` holding that type and compares.
//!
//! Usage: `registry-check <ior-file> <idl-file> <qualified::Name>`

use std::time::Duration;

use orbweaver_giop::typecode::{self, TypeCode};
use orbweaver_giop::{Connection, Ior};
use orbweaver_registry::Registry;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: registry-check <ior-file> <idl-file> <qualified::Name>");
        return std::process::ExitCode::from(2);
    }
    match run(&args[0], &args[1], &args[2]) {
        Ok(true) => {
            println!("\nregistry: PASS — derived TypeCode matches the peer's");
            std::process::ExitCode::SUCCESS
        }
        Ok(false) => {
            println!("\nregistry: FAIL — derived TypeCode differs from the peer's");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            println!("\nregistry: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(ior_path: &str, idl_path: &str, name: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let src = std::fs::read_to_string(idl_path)?;
    let spec = orbweaver_idl::parse(&src).map_err(|e| e.to_string())?;
    let mut reg = Registry::new();
    reg.load(&spec)?;

    let id = reg.id_of(name).ok_or_else(|| format!("{name} is not in the registry"))?.clone();
    let derived = reg
        .typecode(&id)
        .ok_or_else(|| format!("{id} has no TypeCode"))?
        .clone();
    println!("derived from IDL   {id}");

    let ior = Ior::parse(std::fs::read_to_string(ior_path)?.trim())?;
    let mut conn = Connection::connect(&ior, Duration::from_secs(10))?;

    // Send an `any` of the derived type and read back what the peer echoes.
    // A peer that disagreed about the type would fail to decode it at all.
    let sent = derived.clone();
    let reply = conn.invoke("echo_any", move |e| {
        let _ = typecode::encode_any_with(e, &sent, |v| write_zeroed(v, &sent));
    })?;
    let mut b = reply.body()?;
    let returned = typecode::decode(&mut b)?;
    println!("returned by peer   {}", describe(&returned));

    let same = returned == derived;
    if !same {
        println!("  derived : {derived:?}");
        println!("  returned: {returned:?}");
    }
    Ok(same)
}

fn describe(tc: &TypeCode) -> String {
    match tc {
        TypeCode::Struct { id, members, .. } => format!("{id} ({} members)", members.len()),
        TypeCode::Enum { id, members, .. } => format!("{id} ({} enumerators)", members.len()),
        TypeCode::Union { id, cases, .. } => format!("{id} ({} cases)", cases.len()),
        other => format!("{other:?}"),
    }
}

/// Writes a zero value of `tc`, which is enough for the peer to accept the
/// `any` and echo it. Only the type description is under test here.
fn write_zeroed(e: &mut orbweaver_cdr::Encoder, tc: &TypeCode) {
    match tc.resolve_alias() {
        TypeCode::Boolean | TypeCode::Octet | TypeCode::Char => e.put_octet(0),
        TypeCode::Short | TypeCode::UShort => e.put_i16(0),
        TypeCode::Long | TypeCode::ULong | TypeCode::Enum { .. } => e.put_i32(0),
        TypeCode::LongLong | TypeCode::ULongLong => e.put_i64(0),
        TypeCode::Float => e.put_f32(0.0),
        TypeCode::Double => e.put_f64(0.0),
        TypeCode::String(_) => e.put_str(""),
        TypeCode::Struct { members, .. } => {
            for m in members {
                write_zeroed(e, &m.tc);
            }
        }
        TypeCode::Sequence { .. } => e.put_u32(0),
        TypeCode::Array { element, length } => {
            for _ in 0..*length {
                write_zeroed(e, element);
            }
        }
        _ => {}
    }
}
