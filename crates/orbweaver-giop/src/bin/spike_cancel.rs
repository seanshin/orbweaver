//! Stream E: `CancelRequest` send — whatever the peer does with one, our
//! side must stay coherent.
//!
//! §9.4.4 makes cancellation advisory: the target MAY ignore it and no reply
//! ever correlates with it, so "the peer cancelled the work" is not a claim
//! any client can verify. On top of that, our connection holds one request in
//! flight and blocks until its reply, so from the public API there is never
//! an id mid-flight to cancel. What *is* measurable — and what this spike
//! measures, per PLAN §7.3's batch unit of one capability × both peers ×
//! GIOP 1.0/1.1/1.2 — is what the peer does with one, and that our side
//! survives it either way:
//!
//! - the peer ignores it: the same connection must still answer; or
//! - the peer refuses it and closes — omniORB 4.3.4 does this for any
//!   CancelRequest below GIOP 1.2, even as the first message on a fresh
//!   connection (verified with independently built bytes, so it is peer
//!   policy, not our encoding): then our client must fail *cleanly* — an
//!   error, a connection that reports unusable, and a fresh connection
//!   that works. A hang, a misparse, or a poisoned stream still claiming
//!   usable is the failure this spike exists to catch.
//!
//! Usage: `spike-cancel <ior-file>`

use std::time::Duration;

use orbweaver_giop::{Connection, Ior, Version};

const OK: &str = "ok  ";
const NO: &str = "FAIL";

fn main() -> std::process::ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: spike-cancel <ior-file>");
        return std::process::ExitCode::from(2);
    };
    match run(&path) {
        Ok(0) => {
            println!("\ncancel: PASS — ignored or refused cleanly at all three versions");
            std::process::ExitCode::SUCCESS
        }
        Ok(n) => {
            println!("\ncancel: FAIL — {n} case(s)");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            println!("  {NO} {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn ping(conn: &mut Connection) -> Result<i32, String> {
    let reply = conn.invoke_nullary("ping").map_err(|e| e.to_string())?;
    let mut b = reply.body().map_err(|e| e.to_string())?;
    b.get_i32().map_err(|e| e.to_string())
}

fn run(path: &str) -> Result<u32, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let ior = Ior::parse(text.trim()).map_err(|e| e.to_string())?;

    let mut fails = 0u32;
    for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
        let mut conn =
            Connection::connect(&ior, Duration::from_secs(5)).map_err(|e| e.to_string())?;
        conn.cap_version(version);

        // A call must work before the cancel, or the "still works after"
        // half would measure nothing about the cancel.
        match ping(&mut conn) {
            Ok(42) => println!("  {OK} {version}: ping answers before the cancel"),
            Ok(v) => {
                println!("  {NO} {version}: ping answered {v}, expected 42");
                fails += 1;
                continue;
            }
            Err(e) => {
                println!("  {NO} {version}: ping failed before any cancel: {e}");
                fails += 1;
                continue;
            }
        }

        // An id that was never issued — exactly what a real client can send,
        // since nothing is ever mid-flight on this connection.
        if let Err(e) = conn.cancel(9999) {
            println!("  {NO} {version}: sending CancelRequest failed: {e}");
            fails += 1;
            continue;
        }

        // Sending the message must not itself poison the connection: the
        // bytes went out and nothing has been read.
        if !conn.is_usable() {
            println!("  {NO} {version}: connection reports poisoned by merely sending a cancel");
            fails += 1;
            continue;
        }

        // The claim under test: whichever way the peer answers §9.4.4's
        // latitude, our side stays coherent.
        match ping(&mut conn) {
            Ok(42) => {
                println!(
                    "  {OK} {version}: cancel(9999) ignored by the peer — ping still answers on the same connection"
                );
            }
            Ok(v) => {
                println!("  {NO} {version}: post-cancel ping answered {v}, expected 42");
                fails += 1;
            }
            Err(e) => {
                // The refuse-and-close outcome. Clean means: the connection
                // knows it is dead, and a fresh one works immediately.
                if conn.is_usable() {
                    println!(
                        "  {NO} {version}: post-cancel ping failed ({e}) but the connection still claims usable"
                    );
                    fails += 1;
                    continue;
                }
                let mut fresh =
                    Connection::connect(&ior, Duration::from_secs(5)).map_err(|e| e.to_string())?;
                fresh.cap_version(version);
                match ping(&mut fresh) {
                    Ok(42) => println!(
                        "  {OK} {version}: peer refuses a cancel at this version and closes ({e}); ours fails clean and a fresh connection works"
                    ),
                    other => {
                        println!(
                            "  {NO} {version}: peer closed on the cancel and a fresh connection did not recover: {other:?}"
                        );
                        fails += 1;
                    }
                }
            }
        }
    }
    Ok(fails)
}
