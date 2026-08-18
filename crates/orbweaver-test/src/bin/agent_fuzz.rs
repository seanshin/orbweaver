//! `agent-fuzz` — does any parser an *agent* reaches panic, or allocate from a
//! number the agent wrote?
//!
//! Usage: `agent-fuzz [--cases N] [--seed S]`
//!
//! The sibling of `wire-fuzz`, over the other untrusted boundary. `wire-fuzz`
//! covers the decoders a peer reaches before any policy runs; this covers the
//! parsers reachable through `tools/call`, which since AnyJSON v1.1 include one
//! that reads a whole `TypeCode` out of the agent's document. §9.0's R11/R12
//! say an agent is untrusted exactly as a peer is, so a green wire run is not
//! evidence about this boundary and never was.
//!
//! Exits non-zero on any finding, printing the seed and the document. Two
//! things count as findings and they are different failures:
//!
//! - a **panic**, which is a process an agent can stop, and
//! - an **allocation** a number in the document commanded, which is a process
//!   an agent can starve. `wire-fuzz` names this one in its own comments —
//!   "twelve bytes must not buy a multi-gigabyte allocation" — and cannot check
//!   it, because the allocation that matters aborts instead of unwinding.
//!
//! The run prints how many documents actually reached each parser, because a
//! fuzz whose documents all bounce off `Json::parse` is green and worthless and
//! the exit code cannot tell the two apart.

use orbweaver_test::agent;
use orbweaver_test::prop::DEFAULT_SEED;

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
                eprintln!("agent-fuzz: unknown argument {other:?}");
                return std::process::ExitCode::from(2);
            }
        }
    }

    let findings = agent::panic_freedom(cases, seed);
    let reach = agent::reach(cases, seed);
    let panics = findings.iter().filter(|f| f.rule == agent::PANIC_RULE).count();
    let allocations = findings.iter().filter(|f| f.rule == agent::ALLOCATION_RULE).count();
    println!(
        "agent-fuzz: {cases} case(s) x {} target(s), seed {seed:#x}: {panics} panic(s), \
         {allocations} allocation finding(s)",
        agent::target_names().len(),
    );
    println!(
        "  documents: {} uniform, {} mutated, {} truncated  ->  {} parsed as JSON, {} refused \
         for nesting depth",
        reach.uniform, reach.mutated, reach.truncated, reach.parsed, reach.too_deep,
    );
    println!(
        "  types:     {} read by tc_from_json, {} round-tripped through tc_to_json, {} carried a \
         _raw escape, {} over the allocation budget",
        reach.type_codes, reach.type_code_round_trips, reach.raw_escapes, reach.over_budget,
    );
    println!(
        "  values:    {} read by from_json, {} rendered back by to_json, {} encoded to CDR, {} \
         resolved a reference handle",
        reach.values, reach.values_rendered, reach.values_encoded, reach.references,
    );
    // A zero on any of those is not a pass. The exit code cannot say so, so
    // this does: an unmeasured check is a failure, never a pass (CLAUDE.md).
    for (what, count) in [
        ("documents parsed as JSON", reach.parsed),
        ("types read by tc_from_json", reach.type_codes),
        ("types round-tripped through tc_to_json", reach.type_code_round_trips),
        ("_raw escapes", reach.raw_escapes),
        ("values read by from_json", reach.values),
        ("values rendered back by to_json", reach.values_rendered),
        ("values encoded to CDR", reach.values_encoded),
        ("reference handles resolved", reach.references),
        ("documents refused for nesting depth", reach.too_deep),
    ] {
        if count == 0 {
            println!(
                "  WARNING: no {what} were reached; the target(s) behind that number returned \
                 early on every case and their green result measures nothing"
            );
        }
    }
    if findings.is_empty() {
        println!("agent-fuzz: PASS");
        std::process::ExitCode::SUCCESS
    } else {
        for f in &findings {
            println!("  FAIL [{}] {}", f.rule, f.message);
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
