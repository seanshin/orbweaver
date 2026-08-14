//! `wire-fuzz` — does any decoder panic on bytes a peer chose?
//!
//! Usage: `wire-fuzz [--cases N] [--seed S]`
//!
//! Exits non-zero on the first panic found, printing the seed and the bytes.
//! Green means every input produced `Ok` or `Err`, which is the only pass this
//! check recognises: a decoder that panics is a process a stranger can stop.
//!
//! The run prints how many inputs actually parsed as a message, because a fuzz
//! that bounces off the header check every time is green and worthless, and the
//! two are indistinguishable from the exit code alone.

use orbweaver_test::prop::DEFAULT_SEED;
use orbweaver_test::wire;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cases = 2_000usize;
    let mut seed = DEFAULT_SEED;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--cases" if i + 1 < args.len() => {
                cases = args[i + 1].parse().unwrap_or(cases);
                i += 2;
            }
            "--seed" if i + 1 < args.len() => {
                seed = parse_seed(&args[i + 1]).unwrap_or(seed);
                i += 2;
            }
            other => {
                eprintln!("wire-fuzz: unknown argument {other:?}");
                return std::process::ExitCode::from(2);
            }
        }
    }

    let findings = wire::panic_freedom(cases, seed);
    let reach = wire::reach(cases, seed);
    println!(
        "wire-fuzz: {cases} case(s) x {} target(s), seed {seed:#x}: {} panic(s)",
        wire::target_names().len(),
        findings.len()
    );
    println!(
        "  bytes: {} uniform, {} mutated, {} truncated  ->  {} parsed as a GIOP message, {} as an \
         IOR encapsulation; {} were valid UTF-8 and {} of those parsed as a stringified IOR",
        reach.uniform,
        reach.mutated,
        reach.truncated,
        reach.parsed,
        reach.encapsulations,
        reach.utf8,
        reach.stringified_from_bytes
    );
    println!(
        "  text:  {} uniform, {} mutated, {} truncated  ->  {} stringified IOR(s), {} trace \
         span(s), {} repository id(s), {} identifier(s) accepted",
        reach.text_uniform,
        reach.text_mutated,
        reach.text_truncated,
        reach.stringified_iors,
        reach.trace_spans,
        reach.repository_ids,
        reach.identifiers
    );
    // A zero on any of those is not a pass. The exit code cannot say so, so
    // this does: an unmeasured check is a failure, never a pass (CLAUDE.md).
    for (what, count) in [
        ("IOR encapsulations", reach.encapsulations),
        ("stringified IORs", reach.stringified_iors),
        ("trace spans", reach.trace_spans),
        ("repository ids", reach.repository_ids),
        ("identifiers", reach.identifiers),
    ] {
        if count == 0 {
            println!(
                "  WARNING: no {what} were reached; the target(s) behind that number returned \
                 early on every case and their green result measures nothing"
            );
        }
    }
    if findings.is_empty() {
        println!("wire-fuzz: PASS");
        std::process::ExitCode::SUCCESS
    } else {
        for f in &findings {
            println!("  FAIL {}", f.message);
            if let Some(fix) = &f.fix {
                println!("       {fix}");
            }
        }
        std::process::ExitCode::FAILURE
    }
}

fn parse_seed(s: &str) -> Option<u64> {
    match s.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}
