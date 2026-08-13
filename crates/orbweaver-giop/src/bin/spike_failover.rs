//! Stream E: does a client survive an IOR whose first endpoint is dead?
//!
//! Unit tests prove failover against local `TcpListener`s, which can accept a
//! connection but never speak GIOP. This spike closes the remaining gap
//! against a real peer: it takes the fixture's published IOR, prepends a
//! profile that is certainly dead — the real primary re-pointed at a loopback
//! port the OS just proved free, so key and IIOP version stay genuine — then a real
//! call. Passing requires both halves at once: the dead profile skipped, and
//! the surviving profile good for an actual invocation, not merely a TCP
//! handshake.
//!
//! Usage: `spike-failover <ior-file>`

use std::time::Duration;

use orbweaver_giop::{Connection, Error, Ior};

/// A loopback port with provably nothing behind it.
///
/// "Port 1 is dead" was an assumption, and the CI runner refuted it: something
/// there accepts TCP on port 1 (an Azure networking layer, most likely), so
/// the all-dead IOR connected and the check failed for a reason that had
/// nothing to do with failover. Binding an ephemeral port and dropping the
/// listener yields a port the OS just proved free — the refusal is then
/// attributable to nothing listening, not to a guess about the environment.
fn provably_dead_port() -> Result<u16, String> {
    let listener =
        std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("bind: {e}"))?;
    let port = listener.local_addr().map_err(|e| format!("local_addr: {e}"))?.port();
    drop(listener);
    Ok(port)
}

const OK: &str = "ok  ";
const NO: &str = "FAIL";

fn main() -> std::process::ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: spike-failover <ior-file>");
        return std::process::ExitCode::from(2);
    };
    match run(&path) {
        Ok(0) => {
            println!("\nfailover: PASS — a dead first profile does not cost the call");
            std::process::ExitCode::SUCCESS
        }
        Ok(n) => {
            println!("\nfailover: FAIL — {n} case(s)");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            println!("  {NO} {e}");
            println!("\nfailover: FAIL");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(path: &str) -> Result<u32, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let real = Ior::parse(text.trim()).map_err(|e| e.to_string())?;

    // The dead profile keeps the real profile's key and IIOP version, for the
    // same reason spike-locate corrupts rather than invents its bogus key: a
    // skip must be attributable to the endpoint being down, never to the
    // profile being malformed. The endpoint itself is loopback at a port the
    // OS just proved free — see provably_dead_port for why "port 1" was wrong.
    let dead_port = provably_dead_port()?;
    let mut dead = real.primary().map_err(|e| e.to_string())?.clone();
    dead.host = "127.0.0.1".into();
    dead.port = dead_port;
    let mut synthetic = real.clone();
    synthetic.profiles.insert(0, dead);
    println!(
        "  synthetic IOR: {} profile(s), first one dead at 127.0.0.1:{dead_port}",
        synthetic.profiles.len(),
    );

    let mut fails = 0u32;

    let mut conn = match Connection::connect(&synthetic, Duration::from_secs(5)) {
        Ok(c) => {
            println!("  {OK} connect skipped the dead profile");
            c
        }
        Err(e) => {
            println!("  {NO} connect did not fail over: {e}");
            return Ok(1);
        }
    };

    // The connection must be good for a real call, because a failover that
    // lands on an endpoint which accepts TCP but cannot serve the object
    // would pass a handshake-only check and still lose every request.
    match conn.invoke_nullary("ping").and_then(|r| Ok(r.body()?.get_i32()?)) {
        Ok(42) => println!("  {OK} ping() -> 42 over the surviving profile"),
        Ok(v) => {
            println!("  {NO} ping() -> {v}, expected 42");
            fails += 1;
        }
        Err(e) => {
            println!("  {NO} ping() over the surviving profile: {e}");
            fails += 1;
        }
    }

    // The negative half: with every profile dead, the error must say how many
    // endpoints were tried and why the last one failed. An unmeasured refusal
    // is not a refusal.
    let mut all_dead = real.clone();
    for p in &mut all_dead.profiles {
        p.host = "127.0.0.1".into();
        p.port = provably_dead_port()?;
    }
    let endpoints: usize = all_dead.profiles.iter().map(|p| p.endpoints().len()).sum();
    match Connection::connect(&all_dead, Duration::from_secs(5)) {
        Err(Error::AllEndpointsFailed { tried, last }) if tried == endpoints => {
            println!("  {OK} all-dead IOR reports {tried} endpoint(s) tried; last: {last}");
        }
        Err(e) => {
            println!("  {NO} all-dead IOR gave the wrong error: {e}");
            fails += 1;
        }
        Ok(_) => {
            println!("  {NO} all-dead IOR connected; a provably-free port answered");
            fails += 1;
        }
    }

    Ok(fails)
}
