//! Serves `CosTrading::Lookup` over a population **loaded from
//! `corpus/state/`**, so a foreign trading client can check the wire against a
//! population that was stated rather than typed twice. D026 §5 S1b.
//!
//! TEST FIXTURE.
//!
//! # Why this exists beside `spike-trading` rather than replacing it
//!
//! `crates/orbweaver-giop/src/bin/spike_trading.rs` builds its five offers
//! inline and `spikes/trading_client.py` checks them against expectations
//! typed a second time at the other end — including the expected ranking
//! `["math-fast", "math-slow", "untimed"]`, written as a literal in the
//! client. That pair is a good measurement of the servant and it stays. What
//! it cannot be is a measurement of the *ranking*: the population and the
//! expectation were written by one author in two places, so if the ranker
//! regressed and the expectation were wrong in the same direction, nothing
//! would notice.
//!
//! This binary and `spikes/seed_trading_client.py` read **one file** with two
//! readers that share no code — this one through
//! `orbweaver_test::state` and `orbweaver_dynamic::json`, that one through
//! Python's stdlib `json` and omniORB's own COS stubs. Neither end can
//! silently agree with itself.
//!
//! D026 §3, and it is not a formality: **the seeded population does not become
//! the only population.** `spike-trading` keeps its inline offers, `wire-fuzz`
//! keeps its generated ones, and nothing here retires an ad-hoc case.
//!
//! # The licence boundary
//!
//! omniORB is a **fixture, never a dependency**. It appears nowhere in this
//! crate's tree; the Python client runs it as a separate process over TCP and
//! we read what it prints. `cargo tree --workspace` stays free of it.
//!
//! Usage: `spike-seeded-trading [ior-path] [--hold] [--port N]`

use std::io::Write;

use orbweaver_giop::server::Server;
use orbweaver_giop::trading_server::TradingServer;
use orbweaver_giop::{Ior, Result};
use orbweaver_test::state::MoeExperts;
use orbweaver_trading::service_type::{PropertyKind, PropertyMode, PropertySchema, ServiceType};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hold = args.iter().any(|a| a == "--hold");
    let port: u16 =
        args.windows(2).find(|w| w[0] == "--port").and_then(|w| w[1].parse().ok()).unwrap_or(0);
    let out = args
        .iter()
        .find(|a| !a.starts_with("--") && a.parse::<u16>().is_err())
        .cloned()
        .unwrap_or_else(|| "spikes/seeded-trading.ior".to_owned());

    match run(&out, port, hold) {
        Ok(0) => {
            println!("\nseeded-trading: PASS");
            std::process::ExitCode::SUCCESS
        }
        Ok(n) => {
            println!("\nseeded-trading: FAIL — {n} check(s) failed");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            println!("\nseeded-trading: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Maps the seed's property spellings onto the trader's enums.
///
/// By name, and refusing an unknown one. A `_ => Normal` here would let a
/// misspelt mode in the seed become the most permissive mode on the wire,
/// silently — the shape of defect CLAUDE.md's cascade rule is about.
fn kind_of(s: &str) -> std::result::Result<PropertyKind, String> {
    match s {
        "text" => Ok(PropertyKind::Text),
        "float" => Ok(PropertyKind::Float),
        "counter" => Ok(PropertyKind::Counter),
        "state" => Ok(PropertyKind::State),
        other => Err(format!("`{other}` is not a property kind (text, float, counter, state)")),
    }
}

fn mode_of(s: &str) -> std::result::Result<PropertyMode, String> {
    match s {
        "normal" => Ok(PropertyMode::Normal),
        "readonly" => Ok(PropertyMode::Readonly),
        "mandatory" => Ok(PropertyMode::Mandatory),
        "mandatory_readonly" => Ok(PropertyMode::MandatoryReadonly),
        other => Err(format!(
            "`{other}` is not a property mode \
             (normal, readonly, mandatory, mandatory_readonly)"
        )),
    }
}

fn run(out: &str, port: u16, hold: bool) -> Result<u32> {
    let seed = match MoeExperts::load() {
        Ok(s) => s,
        Err(e) => {
            println!("  FAIL the seed did not load: {e}");
            return Ok(1);
        }
    };
    println!(
        "seed     corpus/state/moe-experts.json — {} offer(s) of {}",
        seed.offers.len(),
        seed.service_type_name
    );

    let key = b"SeededTradingService".to_vec();
    let server = Server::bind(&format!("127.0.0.1:{port}"), key.clone())?;
    let bound = server.local_addr()?.port();
    let trader = TradingServer::new("127.0.0.1", bound, key);

    let mut schema = Vec::new();
    for (name, kind, mode) in &seed.properties {
        let (k, m) = match (kind_of(kind), mode_of(mode)) {
            (Ok(k), Ok(m)) => (k, m),
            (Err(e), _) | (_, Err(e)) => {
                println!("  FAIL service_type property `{name}`: {e}");
                return Ok(1);
            }
        };
        schema.push(PropertySchema::new(name.clone(), k, m));
    }

    let declared = ServiceType::declare(&seed.service_type_name, &seed.interface_id, schema);
    let declared = match declared {
        Ok(d) => d,
        Err(e) => {
            println!("  FAIL the seeded service type is not legal: {e}");
            return Ok(1);
        }
    };

    let mut failures = 0;
    trader.with_store(|s| {
        if let Err(e) = s.declare(declared) {
            println!("  FAIL declare: {e}");
            failures += 1;
            return;
        }
        for offer in seed.offers.iter().cloned() {
            let id = offer.id.clone();
            if let Err(e) = s.register(&seed.service_type_name, offer) {
                println!("  FAIL register {id}: {e}");
                failures += 1;
            }
        }
    });

    let ior = trader.lookup_ior();
    write_ior(out, &ior)?;
    println!("seeded-trading: serving CosTrading::Lookup on 127.0.0.1:{bound}, IOR in {out}");

    // What was actually seeded, echoed from the loaded population rather than
    // from a second list — a reader comparing this against the file is
    // comparing the file against itself, which is why it is a report and not
    // a check. The checks are on the wire, in seed_trading_client.py.
    for offer in &seed.offers {
        println!(
            "  offer  {:<12} spec={:<8} cost={:<4} p50={:<6} node={}",
            offer.id,
            offer.specialization.as_deref().unwrap_or("-"),
            offer.cost,
            offer.latency_p50.map(|v| v.to_string()).unwrap_or_else(|| "-".to_owned()),
            offer.placement_node
        );
    }

    let mut check = |what: &str, ok: bool| {
        if ok {
            println!("  ok   {what}");
        } else {
            println!("  FAIL {what}");
            failures += 1;
        }
    };
    check(
        "the published IOR names CosTrading::Lookup",
        ior.type_id == orbweaver_giop::trading_server::LOOKUP_ID,
    );
    check(
        "the IOR carries a dialable profile",
        ior.primary().map(|p| p.port == bound).unwrap_or(false),
    );
    check(
        "every seeded offer registered",
        trader.with_store(|s| s.store().len()) == seed.offers.len(),
    );

    if hold {
        println!(
            "\nHOLDING — the seeded population is served at 127.0.0.1:{bound}; \
             point spikes/seed_trading_client.py at {out}"
        );
        server.serve_shared(&trader, || false)?;
    }
    Ok(failures)
}

fn write_ior(path: &str, ior: &Ior) -> Result<()> {
    let orb = orbweaver_giop::orb::Orb::new();
    let text = orb.object_to_string(ior)?;
    let mut f = std::fs::File::create(path).expect("the IOR path is writable");
    writeln!(f, "{text}").expect("the IOR is written");
    Ok(())
}
