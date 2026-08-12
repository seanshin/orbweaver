//! Can we call an interface we compiled nothing for?
//!
//! Every call in this spike is built at runtime from the IDL text alone: the
//! registry says what `add` looks like, `orbweaver_dynamic::encode` turns the
//! arguments into bytes, and a stock ORB on the other end answers. No stub, no
//! generated type, no operation name known at compile time.
//!
//! That is `docs/PLAN.md` §4.6's `invoke_operation` reduced to the part that
//! has to be true for the rest to matter, and it is checked against a peer we
//! did not write — a dynamic invoker that only agrees with our own decoder has
//! not been tested at all.
//!
//! Usage: `spike-dynamic <ior-file> <idl-file> <interface-id>`

use std::collections::BTreeMap;
use std::time::Duration;

use orbweaver_dynamic::Value;
use orbweaver_dynamic::invoke::{InvokeError, invoke};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_giop::{Connection, Ior};
use orbweaver_registry::Registry;

const OK: &str = "ok  ";
const NO: &str = "FAIL";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [ior_path, idl_path, interface_id] = args.as_slice() else {
        eprintln!("usage: spike-dynamic <ior-file> <idl-file> <interface-id>");
        return std::process::ExitCode::from(2);
    };

    match run(ior_path, idl_path, interface_id) {
        Ok(0) => {
            println!("\ndynamic invocation: PASS — calls built from IDL text alone");
            std::process::ExitCode::SUCCESS
        }
        Ok(n) => {
            println!("\ndynamic invocation: FAIL — {n} case(s)");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            println!("  {NO} {e}");
            println!("\ndynamic invocation: FAIL");
            std::process::ExitCode::FAILURE
        }
    }
}

fn args_of(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
}

fn run(ior_path: &str, idl_path: &str, interface_id: &str) -> Result<u32, String> {
    let src = std::fs::read_to_string(idl_path).map_err(|e| format!("{idl_path}: {e}"))?;
    let spec = orbweaver_idl::parse(&src).map_err(|e| format!("{idl_path}: {e}"))?;
    let mut registry = Registry::new();
    registry.load(&spec).map_err(|e| e.to_string())?;

    let text = std::fs::read_to_string(ior_path).map_err(|e| format!("{ior_path}: {e}"))?;
    let ior = Ior::parse(text.trim()).map_err(|e| e.to_string())?;

    let mut fails = 0u32;

    // Both byte orders, because a marshaller that only works native-endian
    // passes every local test and fails in the field.
    for endian in [orbweaver_cdr::Endian::Big, orbweaver_cdr::Endian::Little] {
        let label = match endian {
            orbweaver_cdr::Endian::Big => "big-endian",
            orbweaver_cdr::Endian::Little => "little-endian",
        };
        println!("── {label}, nothing generated ──");

        let mut conn =
            Connection::connect(&ior, Duration::from_secs(5)).map_err(|e| e.to_string())?;
        conn.set_endian(endian);

        let mut case = |op: &str, args: BTreeMap<String, Value>, want: Value| match invoke(
            &mut conn,
            &registry,
            interface_id,
            op,
            &args,
        ) {
            Ok(out) if out.returns == want => {
                println!("  {OK} {op}() -> {}", summarise(&out.returns));
            }
            Ok(out) => {
                println!("  {NO} {op}() -> {:?}, expected {want:?}", out.returns);
                fails += 1;
            }
            Err(e) => {
                println!("  {NO} {op}(): {e}");
                fails += 1;
            }
        };

        case("ping", args_of(&[]), Value::Long(42));
        case(
            "add",
            args_of(&[("a", Value::Long(1_000_000)), ("b", Value::Long(337))]),
            Value::Long(1_000_337),
        );
        case(
            "echo_string",
            args_of(&[("msg", Value::String("built at runtime".into()))]),
            Value::String("built at runtime".into()),
        );
        case(
            "scale",
            args_of(&[("v", Value::Double(1.5)), ("by", Value::Double(4.0))]),
            Value::Double(6.0),
        );

        // The alignment case. Ragged's members are octet/long/short/double/octet,
        // so every padding rule in CDR has to be applied in the right place by
        // code that has never seen the type.
        let ragged = Value::Struct(vec![
            ("a".into(), Value::Octet(0xAA)),
            ("b".into(), Value::Long(-7)),
            ("c".into(), Value::Short(9)),
            ("d".into(), Value::Double(2.5)),
            ("e".into(), Value::Octet(0xBB)),
        ]);
        case("echo_ragged", args_of(&[("v", ragged.clone())]), ragged);

        // An `any` built dynamically: the TypeCode goes on the wire ahead of the
        // value, and the value keeps aligning against the outer stream.
        let any = Value::Any(Box::new(TypeCode::Double), Box::new(Value::Double(-0.125)));
        case("echo_any", args_of(&[("v", any.clone())]), any);

        // A sequence, and then the peer's own arithmetic over it — so the check
        // is that omniORB read our bytes as the values we meant, not merely
        // that it echoed them back.
        let payload: Vec<Value> = (0..64u16).map(|i| Value::Octet((i % 251) as u8)).collect();
        let expected: i32 = payload
            .iter()
            .map(|v| match v {
                Value::Octet(b) => i32::from(*b),
                _ => 0,
            })
            .sum();
        case(
            "blob_sum",
            args_of(&[("b", Value::List(payload))]),
            Value::Long(expected % 2_147_483_647),
        );

        case(
            "echo_wstring",
            args_of(&[("w", Value::WString("동적 호출".into()))]),
            Value::WString("동적 호출".into()),
        );
    }

    // The diagnostics are the product for a caller that is guessing, so they
    // are checked here rather than trusted: a wrong call must fail locally,
    // before anything reaches the peer.
    println!("── refusals, all of them local ──");
    let mut conn = Connection::connect(&ior, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    for (what, op, args, expect) in [
        ("an argument left out", "add", args_of(&[("a", Value::Long(1))]), "missing b"),
        (
            "a value of the wrong type",
            "add",
            args_of(&[("a", Value::Long(1)), ("b", Value::String("two".into()))]),
            "got a string",
        ),
        (
            "an operation that does not exist",
            "Add",
            args_of(&[("a", Value::Long(1)), ("b", Value::Long(2))]),
            "did you mean",
        ),
    ] {
        match invoke(&mut conn, &registry, interface_id, op, &args) {
            Err(InvokeError::Marshalling(e)) if e.to_string().contains(expect) => {
                println!("  {OK} {what}: {e}");
            }
            Err(e) => {
                println!("  {NO} {what}: expected a message containing {expect:?}, got {e}");
                fails += 1;
            }
            Ok(_) => {
                println!("  {NO} {what} was accepted and sent");
                fails += 1;
            }
        }
    }

    // The connection must still be usable: a refusal that poisoned it would
    // make the diagnostics worse than useless to an agent that retries.
    match invoke(&mut conn, &registry, interface_id, "ping", &args_of(&[])) {
        Ok(out) if out.returns == Value::Long(42) => {
            println!("  {OK} the connection still works after three refusals")
        }
        other => {
            println!("  {NO} a refused call damaged the connection: {other:?}");
            fails += 1;
        }
    }

    Ok(fails)
}

fn summarise(v: &Value) -> String {
    match v {
        Value::String(s) | Value::WString(s) => format!("{s:?}"),
        Value::Struct(m) => format!("{{{} member(s)}}", m.len()),
        Value::List(items) => format!("[{} item(s)]", items.len()),
        Value::Any(tc, inner) => format!("any({:?}) = {}", tc.kind(), summarise(inner)),
        other => format!("{other:?}"),
    }
}
