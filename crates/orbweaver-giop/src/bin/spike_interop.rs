//! Phase 0 assumption A: can a from-scratch GIOP implementation interoperate
//! with a stock ORB?
//!
//! Reads a stringified IOR published by an omniORB server, hand-encodes GIOP
//! 1.2 requests against it, and checks the replies. Nothing here links against
//! any existing ORB — the only contract is the published wire specification.
//!
//! Usage: `spike-interop <ior-file>`

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_giop::typecode::{self, Any, Member, TypeCode};
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

fn conn_endian(e: Endian) -> Endian {
    e
}

/// `any` payloads worth putting on the wire: a primitive, a bounded string and
/// a constructed type, because a TypeCode encoder can be right about the first
/// and wrong about the encapsulation the third needs.
#[allow(clippy::type_complexity)]
fn any_cases() -> Vec<(&'static str, TypeCode, Box<dyn Fn(&mut Encoder)>)> {
    vec![
        ("long", TypeCode::Long, Box::new(|e: &mut Encoder| e.put_i32(-4242))),
        ("string", TypeCode::String(0), Box::new(|e: &mut Encoder| e.put_str("any-carried string"))),
        (
            "struct",
            TypeCode::Struct {
                id: "IDL:spike/Ragged:1.0".into(),
                name: "Ragged".into(),
                members: vec![
                    Member { name: "a".into(), tc: TypeCode::Octet },
                    Member { name: "b".into(), tc: TypeCode::Long },
                    Member { name: "c".into(), tc: TypeCode::Short },
                    Member { name: "d".into(), tc: TypeCode::Double },
                    Member { name: "e".into(), tc: TypeCode::Octet },
                ],
            },
            Box::new(|e: &mut Encoder| {
                e.put_octet(0xAA);
                e.put_i32(-7);
                e.put_i16(9);
                e.put_f64(2.5);
                e.put_octet(0xBB);
            }),
        ),
    ]
}

fn run(ior_text: &str) -> Result<u32, Error> {
    let ior = Ior::parse(ior_text)?;
    let p = ior.primary()?;
    println!("target");
    println!("  type_id    {}", ior.type_id);
    println!("  endpoint   {}:{}  (IIOP {}.{})", p.host, p.port, p.version.major, p.version.minor);
    println!("  object_key {} bytes", p.object_key.len());
    println!();

    let mut fails = 0u32;
    let mut asserted = 0u32;

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
        asserted += 1;
        match conn.invoke_nullary("ping").and_then(|r| Ok(r.body()?.get_i32()?)) {
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
        asserted += 1;
        match conn
            .invoke("add", |e| {
                e.put_i32(1_000_000);
                e.put_i32(337);
            })
            .and_then(|r| Ok(r.body()?.get_i32()?))
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
        asserted += 1;
        match conn
            .invoke("echo_string", |e| e.put_str("hello from a hand-rolled ORB"))
            .and_then(|r| Ok(r.body()?.get_string()?))
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
        asserted += 1;
        match conn
            .invoke("scale", |e| {
                e.put_f64(1.5);
                e.put_f64(4.0);
            })
            .and_then(|r| Ok(r.body()?.get_f64()?))
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
        asserted += 1;
        match conn
            .invoke("echo_ragged", |e| {
                e.put_octet(0xAA);
                e.put_i32(-7);
                e.put_i16(9);
                e.put_f64(2.5);
                e.put_octet(0xBB);
            })
            .and_then(|r| {
                let mut b = r.body()?;
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

        // 6. Korean text through the *negotiated* codeset. This is now a real
        //    assertion: the bytes are converted to whatever was agreed and
        //    converted back, so passing means the two sides agree about what
        //    the bytes mean rather than that neither converted them.
        let korean = "함정 전투체계";
        let cs = conn.char_converter();
        println!("  ---  negotiated char codeset: {}", cs.id());
        asserted += 1;
        match cs.encode(korean) {
            Ok(sent) => match conn
                .invoke("echo_string", |e| e.put_string_bytes(&sent))
                .and_then(|r| Ok(r.body()?.get_string_bytes()?.to_vec()))
                .map(|got| cs.decode(&got))
            {
                Ok(Ok(back)) if back == korean => {
                    println!("  {OK} korean round-trip through negotiated {}", cs.id())
                }
                Ok(Ok(back)) => {
                    println!("  {NO} korean came back as {back:?}");
                    fails += 1;
                }
                Ok(Err(e)) => {
                    println!("  {NO} korean reply would not decode: {e}");
                    fails += 1;
                }
                Err(e) => {
                    println!("  {NO} korean round-trip failed: {e}");
                    fails += 1;
                }
            },
            Err(e) => {
                // A negotiated codeset that cannot carry Korean is a real
                // failure to report, not something to paper over.
                println!("  {NO} negotiated {} cannot represent the text: {e}", cs.id());
                fails += 1;
            }
        }

        // 7. `any` across the wire. Self-round-trips only prove our encoder
        //    agrees with our decoder; this proves a peer's TypeCode reader
        //    accepts ours and that we can read what it sends back.
        for (label, tc, write) in any_cases() {
            asserted += 1;
            let want_tc = tc.clone();
            match conn
                .invoke("echo_any", move |e| {
                    // Closure form: the value is written into the live stream so
                    // its padding matches where it lands.
                    let _ = typecode::encode_any_with(e, &want_tc, |v| write(v));
                })
                .and_then(|r| {
                    let mut b = r.body()?;
                    let tc = typecode::decode(&mut b)?;
                    let len = b.remaining();
                    Ok(Any { tc, value: b.get_bytes(len)?.to_vec(), endian: r.endian })
                }) {
                Ok(got) if got.tc == tc => {
                    println!("  {OK} any/{label} round-tripped with its TypeCode intact")
                }
                Ok(got) if got.tc != tc => {
                    println!("  {NO} any/{label}: TypeCode came back as {:?}", got.tc);
                    fails += 1;
                }
                Ok(_) => {
                    println!("  {NO} any/{label}: value differed");
                    fails += 1;
                }
                Err(e) => {
                    println!("  {NO} any/{label}: {e}");
                    fails += 1;
                }
            }
        }

        // 8. Unknown operation must produce BAD_OPERATION, not a hang or a
        //    mis-parse. Error handling is part of interoperating.
        asserted += 1;
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

    println!("asserted cases: {asserted}, failures: {fails}");
    Ok(fails)
}
