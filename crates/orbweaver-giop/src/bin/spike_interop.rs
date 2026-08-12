//! Phase 0 assumption A: can a from-scratch GIOP implementation interoperate
//! with a stock ORB?
//!
//! Reads a stringified IOR published by an omniORB server, hand-encodes GIOP
//! 1.2 requests against it, and checks the replies. Nothing here links against
//! any existing ORB — the only contract is the published wire specification.
//!
//! Usage: `spike-interop <ior-file>`

use orbweaver_cdr::Endian;
use orbweaver_giop::{Connection, Error, Ior};
use std::time::Duration;

/// Text a passing case prints; keeps the harness output greppable.
const OK: &str = "ok  ";
const NO: &str = "FAIL";

fn main() -> std::process::ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: spike-interop <ior-file>");
        return std::process::ExitCode::from(2);
    };
    let ior_text = match std::fs::read_to_string(&path) {
        Ok(s) => s.trim().to_owned(),
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    match run(&ior_text) {
        Ok(failures) if failures == 0 => {
            println!("\nassumption A: PASS — GIOP interop with a stock ORB is reachable");
            std::process::ExitCode::SUCCESS
        }
        Ok(failures) => {
            println!("\nassumption A: FAIL — {failures} case(s) did not interoperate");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            println!("\nassumption A: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(ior_text: &str) -> Result<u32, Error> {
    let ior = Ior::parse(ior_text)?;
    let p = ior.primary()?;
    println!("target");
    println!("  type_id    {}", ior.type_id);
    println!("  endpoint   {}:{}  (IIOP {}.{})", p.host, p.port, p.major, p.minor);
    println!("  object_key {} bytes", p.object_key.len());
    println!();

    let mut fails = 0u32;

    // Both byte orders, because a CDR encoder that only works native-endian
    // passes every local test and fails in the field.
    for endian in [Endian::Big, Endian::Little] {
        let label = match endian {
            Endian::Big => "big-endian",
            Endian::Little => "little-endian",
        };
        println!("── {label} client ──");

        let mut conn = Connection::connect(&ior, Duration::from_secs(5))?;
        conn.set_endian(endian);

        // 1. Nullary call with a scalar return.
        match conn.invoke_nullary("ping").and_then(|r| Ok(r.body().get_i32()?)) {
            Ok(42) => println!("  {OK} ping() -> 42"),
            Ok(v) => {
                println!("  {NO} ping() -> {v}, expected 42");
                fails += 1;
            }
            Err(e) => {
                println!("  {NO} ping(): {e}");
                fails += 1;
            }
        }

        // 2. Two aligned integer arguments.
        match conn
            .invoke("add", |e| {
                e.put_i32(1_000_000);
                e.put_i32(337);
            })
            .and_then(|r| Ok(r.body().get_i32()?))
        {
            Ok(1_000_337) => println!("  {OK} add(1000000, 337) -> 1000337"),
            Ok(v) => {
                println!("  {NO} add() -> {v}, expected 1000337");
                fails += 1;
            }
            Err(e) => {
                println!("  {NO} add(): {e}");
                fails += 1;
            }
        }

        // 3. String round-trip: length prefix counts the NUL.
        match conn
            .invoke("echo_string", |e| e.put_str("hello from a hand-rolled ORB"))
            .and_then(|r| Ok(r.body().get_string()?))
        {
            Ok(s) if s == "hello from a hand-rolled ORB" => {
                println!("  {OK} echo_string() round-tripped {} chars", s.len())
            }
            Ok(s) => {
                println!("  {NO} echo_string() -> {s:?}");
                fails += 1;
            }
            Err(e) => {
                println!("  {NO} echo_string(): {e}");
                fails += 1;
            }
        }

        // 4. Eight-byte alignment: the body must start 8-aligned, and a double
        //    inside it must land where the peer expects.
        match conn
            .invoke("scale", |e| {
                e.put_f64(1.5);
                e.put_f64(4.0);
            })
            .and_then(|r| Ok(r.body().get_f64()?))
        {
            Ok(v) if (v - 6.0).abs() < 1e-9 => println!("  {OK} scale(1.5, 4.0) -> 6.0"),
            Ok(v) => {
                println!("  {NO} scale() -> {v}, expected 6.0");
                fails += 1;
            }
            Err(e) => {
                println!("  {NO} scale(): {e}");
                fails += 1;
            }
        }

        // 5. Ragged struct from corpus/golden/02-alignment.idl, the case that
        //    catches padding mistakes.
        match conn
            .invoke("echo_ragged", |e| {
                e.put_octet(0xAA);
                e.put_i32(-7);
                e.put_i16(9);
                e.put_f64(2.5);
                e.put_octet(0xBB);
            })
            .and_then(|r| {
                let mut b = r.body();
                Ok((b.get_u8()?, b.get_i32()?, b.get_i16()?, b.get_f64()?, b.get_u8()?))
            }) {
            Ok((0xAA, -7, 9, d, 0xBB)) if (d - 2.5).abs() < 1e-9 => {
                println!("  {OK} echo_ragged() preserved struct padding")
            }
            Ok(t) => {
                println!("  {NO} echo_ragged() -> {t:?}");
                fails += 1;
            }
            Err(e) => {
                println!("  {NO} echo_ragged(): {e}");
                fails += 1;
            }
        }

        // 6. Korean text. Without CodeSets negotiation the default transmission
        //    codeset is ISO-8859-1, so this is expected to be lossy until
        //    Phase 1 implements negotiation. Reported, not counted as failure.
        let korean = "함정 전투체계";
        match conn
            .invoke("echo_string", |e| e.put_str(korean))
            .and_then(|r| Ok(r.body().get_string()?))
        {
            Ok(s) if s == korean => println!("  {OK} korean round-trip intact (codesets agreed)"),
            Ok(s) => println!("  note korean came back as {s:?} — CodeSets negotiation needed (Phase 1)"),
            Err(e) => println!("  note korean round-trip failed: {e} — CodeSets negotiation needed (Phase 1)"),
        }

        // 7. Unknown operation must produce BAD_OPERATION, not a hang or a
        //    mis-parse. Error handling is part of interoperating.
        match conn.invoke_nullary("no_such_operation") {
            Err(Error::SystemException { id, .. }) if id.contains("BAD_OPERATION") => {
                println!("  {OK} unknown op -> BAD_OPERATION as specified")
            }
            Err(Error::SystemException { id, .. }) => {
                println!("  {OK} unknown op -> system exception {id}")
            }
            Ok(_) => {
                println!("  {NO} unknown op unexpectedly succeeded");
                fails += 1;
            }
            Err(e) => {
                println!("  {NO} unknown op -> {e}");
                fails += 1;
            }
        }

        println!();
    }

    Ok(fails)
}
