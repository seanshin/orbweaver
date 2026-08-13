//! Stream E batch 1: can we *ask about* an object without invoking it?
//!
//! `LocateRequest` send is on the carried-forward list every phase report has
//! repeated: the server side has answered locates since Phase 2, but nothing
//! here had ever sent one. The batch unit is PLAN §7.3's — this one capability,
//! against both peers, at all three GIOP versions — and both answers are
//! measured, because a locate that can only produce "here" has not been tested
//! against anything.
//!
//! Usage: `spike-locate <ior-file>`

use std::time::Duration;

use orbweaver_giop::{Connection, Ior, LocateResult, Version};

const OK: &str = "ok  ";
const NO: &str = "FAIL";

fn main() -> std::process::ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: spike-locate <ior-file>");
        return std::process::ExitCode::from(2);
    };
    match run(&path) {
        Ok(0) => {
            println!("\nlocate: PASS — both answers, all three versions");
            std::process::ExitCode::SUCCESS
        }
        Ok(n) => {
            println!("\nlocate: FAIL — {n} case(s)");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            println!("  {NO} {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(path: &str) -> Result<u32, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let ior = Ior::parse(text.trim()).map_err(|e| e.to_string())?;
    let real_key = ior.primary().map_err(|e| e.to_string())?.object_key.clone();

    // A key the target cannot know. Derived from the real one by corruption
    // rather than invented, so it exercises the same length and shape and the
    // refusal cannot be an artifact of a malformed probe.
    let mut bogus_key = real_key.clone();
    for b in &mut bogus_key {
        *b ^= 0xFF;
    }

    let mut fails = 0u32;
    for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
        let mut conn =
            Connection::connect(&ior, Duration::from_secs(5)).map_err(|e| e.to_string())?;
        conn.cap_version(version);

        match conn.locate() {
            Ok(LocateResult::Here) => {
                println!("  {OK} {version}: the real key locates as OBJECT_HERE");
            }
            Ok(other) => {
                println!("  {NO} {version}: real key -> {other:?}, expected Here");
                fails += 1;
            }
            Err(e) => {
                println!("  {NO} {version}: locate failed: {e}");
                fails += 1;
                continue;
            }
        }

        // The negative answer, on the same connection: a locate must not
        // poison it, and the peer may answer UNKNOWN_OBJECT or — as some ORBs
        // legitimately do at 1.2 — OBJECT_NOT_EXIST as a locate exception.
        // Either says "not here"; silently answering Here would be the bug.
        match conn.locate_key(&bogus_key) {
            Ok(LocateResult::Unknown) => {
                println!("  {OK} {version}: a corrupted key is UNKNOWN_OBJECT");
            }
            Err(orbweaver_giop::Error::SystemException { ref id, .. })
                if id.contains("OBJECT_NOT_EXIST") =>
            {
                println!("  {OK} {version}: a corrupted key raises OBJECT_NOT_EXIST");
            }
            Ok(other) => {
                println!("  {NO} {version}: corrupted key -> {other:?}");
                fails += 1;
            }
            Err(e) => {
                println!("  {NO} {version}: corrupted-key locate failed oddly: {e}");
                fails += 1;
            }
        }
    }
    Ok(fails)
}
