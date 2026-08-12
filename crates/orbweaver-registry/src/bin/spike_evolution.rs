//! Does the §5.3 rule table describe reality?
//!
//! The table is a set of claims about what deployed peers survive. This spike
//! stops them being claims: it runs the differ over a contract pair, then puts
//! the same edit on the wire against a third-party ORB built from the old
//! contract and checks that the consequence is the one predicted.
//!
//! The case that matters is the quiet one. A breaking change that raised
//! `MARSHAL` would be an inconvenience; a struct whose members were swapped
//! returns a plausible wrong number to a caller that has no way to tell, which
//! is why the differ has to catch it before it ships rather than after.
//!
//! Usage: `spike-evolution <v1.idl> <v2.idl> <v1b.idl> <ior-file>`

use std::time::Duration;

use orbweaver_giop::{Connection, Error, Ior};
use orbweaver_registry::Registry;
use orbweaver_registry::diff::{Verdict, diff};

const OK: &str = "ok  ";
const NO: &str = "FAIL";

/// The values the client sends. Distinct, and distinct from any index, so a
/// swap cannot coincide with a correct answer.
const PX: i32 = 11;
const PY: i32 = 22;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut fails = 0u32;

    let ior_path = match args.as_slice() {
        [flag, ior_path] if flag == "--updated" => {
            fails += match after_the_release(ior_path) {
                Ok(n) => n,
                Err(e) => {
                    println!("  {NO} wire check did not run: {e}");
                    1
                }
            };
            return verdict(fails);
        }
        [v1, v2, v1b, ior_path] => {
            fails += match offline(v1, v2, v1b) {
                Ok(n) => n,
                Err(e) => {
                    println!("  {NO} could not analyse the contracts: {e}");
                    1
                }
            };
            ior_path
        }
        _ => {
            eprintln!(
                "usage: spike-evolution <v1.idl> <v2.idl> <v1b.idl> <ior-file>\n\
                        spike-evolution --updated <ior-file>"
            );
            return std::process::ExitCode::from(2);
        }
    };

    fails += match on_the_wire(ior_path) {
        Ok(n) => n,
        Err(e) => {
            println!("  {NO} wire check did not run: {e}");
            1
        }
    };
    verdict(fails)
}

fn verdict(fails: u32) -> std::process::ExitCode {

    if fails == 0 {
        println!("\ncontract evolution: PASS — every verdict matched what the wire did");
        std::process::ExitCode::SUCCESS
    } else {
        println!("\ncontract evolution: FAIL — {fails} case(s)");
        std::process::ExitCode::FAILURE
    }
}

fn load(path: &str) -> Result<Registry, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let spec = orbweaver_idl::parse(&src).map_err(|e| format!("{path}: {e}"))?;
    let mut r = Registry::new();
    r.load(&spec).map_err(|e| format!("{path}: {e}"))?;
    Ok(r)
}

/// What the differ says, before anything is deployed.
fn offline(v1: &str, v2: &str, v1b: &str) -> Result<u32, String> {
    let (a, b, c) = (load(v1)?, load(v2)?, load(v1b)?);
    let mut fails = 0u32;

    println!("── the differ, on the contract pair ──");
    let risky = diff(&a, &b);
    for ch in &risky {
        println!("     {ch}");
    }

    // The reordered struct must be caught, and caught as breaking.
    let caught = risky
        .iter()
        .any(|c| c.verdict == Verdict::Breaking && c.what.contains("reordered"));
    if caught {
        println!("  {OK} the swapped struct members are flagged BREAKING");
    } else {
        println!("  {NO} the swapped struct members were not flagged breaking");
        fails += 1;
    }

    let added = risky
        .iter()
        .any(|c| c.verdict == Verdict::ServerFirst && c.what.contains("\"total\" added"));
    if added {
        println!("  {OK} the added operation is flagged server-first, not breaking");
    } else {
        println!("  {NO} the added operation carried the wrong verdict");
        fails += 1;
    }

    // A differ that answers "breaking" to everything gives no usable advice,
    // so the additive-only revision has to come back clean of breakage.
    let safe = diff(&a, &c);
    if safe.iter().all(|ch| ch.verdict == Verdict::ServerFirst) && !safe.is_empty() {
        println!("  {OK} the additive-only revision is separated from the risky one");
    } else {
        println!("  {NO} the additive-only revision was not distinguished: {safe:?}");
        fails += 1;
    }

    Ok(fails)
}

/// What actually happens, against an ORB that is not ours.
fn on_the_wire(ior_path: &str) -> Result<u32, String> {
    let text = std::fs::read_to_string(ior_path).map_err(|e| format!("{ior_path}: {e}"))?;
    let ior = Ior::parse(text.trim()).map_err(|e| e.to_string())?;
    let mut conn =
        Connection::connect(&ior, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    let mut fails = 0u32;

    println!("\n── the same edit, against an omniORB peer built from v1 ──");

    // Baseline. Without this the interesting result below could be explained
    // by the call being broken for some unrelated reason.
    match conn.invoke("first", |e| {
        e.put_i32(PX);
        e.put_i32(PY);
    }) {
        Ok(r) => match r.body().and_then(|mut b| Ok(b.get_i32()?)) {
            Ok(PX) => println!("  {OK} v1 client: first({{px:{PX}, py:{PY}}}) -> {PX}"),
            Ok(v) => {
                println!("  {NO} v1 client: first() -> {v}, expected {PX}");
                fails += 1;
            }
            Err(e) => {
                println!("  {NO} v1 client: first(): {e}");
                fails += 1;
            }
        },
        Err(e) => {
            println!("  {NO} v1 client: first(): {e}");
            fails += 1;
        }
    }

    // The breaking change, on the wire. A client generated from v2 emits its
    // members in v2's declaration order; that is all a swap does.
    match conn.invoke("first", |e| {
        e.put_i32(PY);
        e.put_i32(PX);
    }) {
        Ok(r) => match r.body().and_then(|mut b| Ok(b.get_i32()?)) {
            // The assertion is deliberately the wrong answer: the prediction
            // being tested is "corrupts silently", and getting PX back here
            // would mean the rule is wrong and the change is in fact safe.
            Ok(PY) => println!(
                "  {OK} v2 client: first() -> {PY}, the OTHER member — wrong answer, no error"
            ),
            Ok(PX) => {
                println!("  {NO} v2 client: the swap did not change the answer; §5.3 is wrong");
                fails += 1;
            }
            Ok(v) => {
                println!("  {NO} v2 client: first() -> {v}, expected the swapped value {PY}");
                fails += 1;
            }
            Err(e) => {
                println!("  {NO} v2 client: first(): {e}");
                fails += 1;
            }
        },
        Err(e) => {
            // Also a failure of the prediction, though a much less dangerous
            // one: a peer that rejects the message has protected its caller.
            println!("  {NO} v2 client: the peer raised {e} instead of answering wrongly");
            fails += 1;
        }
    }

    // Why the added operation is "server-first" rather than "compatible": a
    // new client is only safe once the server it reaches has been updated.
    match conn.invoke("total", |e| {
        e.put_i32(PX);
        e.put_i32(PY);
    }) {
        Err(Error::SystemException { ref id, .. }) if id.contains("BAD_OPERATION") => {
            println!("  {OK} v2 client: total() on the old server -> BAD_OPERATION");
        }
        Err(e) => {
            println!("  {NO} v2 client: total() failed with {e}, expected BAD_OPERATION");
            fails += 1;
        }
        Ok(_) => {
            println!("  {NO} v2 client: the old server answered an operation it cannot have");
            fails += 1;
        }
    }

    Ok(fails)
}

/// The other half of "server-first": once the server has taken the additive
/// release, both the old client and the new one are served.
fn after_the_release(ior_path: &str) -> Result<u32, String> {
    let text = std::fs::read_to_string(ior_path).map_err(|e| format!("{ior_path}: {e}"))?;
    let ior = Ior::parse(text.trim()).map_err(|e| e.to_string())?;
    let mut conn =
        Connection::connect(&ior, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    let mut fails = 0u32;

    println!("\n── after the additive release, against the same peer ──");

    let put = |e: &mut orbweaver_cdr::Encoder| {
        e.put_i32(PX);
        e.put_i32(PY);
    };

    match conn.invoke("first", put).and_then(|r| Ok(r.body()?.get_i32()?)) {
        Ok(PX) => println!("  {OK} the un-recompiled v1 client still gets {PX}"),
        Ok(v) => {
            println!("  {NO} the v1 client got {v}; an additive release moved something");
            fails += 1;
        }
        Err(e) => {
            println!("  {NO} the v1 client broke: {e}");
            fails += 1;
        }
    }

    match conn.invoke("total", put).and_then(|r| Ok(r.body()?.get_i32()?)) {
        Ok(v) if v == PX + PY => println!("  {OK} the v2 client's new operation now answers"),
        Ok(v) => {
            println!("  {NO} total() -> {v}, expected {}", PX + PY);
            fails += 1;
        }
        Err(e) => {
            println!("  {NO} total() still fails after the release: {e}");
            fails += 1;
        }
    }

    Ok(fails)
}
