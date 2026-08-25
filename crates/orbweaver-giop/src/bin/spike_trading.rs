//! Serves `CosTrading::Lookup` so a **foreign** trading client can call it.
//! `docs/decisions/D022` T4.
//!
//! TEST FIXTURE. The point of opening the trading service is the oracle — an
//! ORB that is not ours resolving a trader and calling `query` — so this
//! binary exists to be pointed at by `spikes/trading_client.py`, which is
//! omniORB's Python COS stubs and no code of ours.
//!
//! Usage: `spike-trading [ior-path] [--hold] [--port N]`
//!
//! With `--hold` the serving window stays open after the local checks, which
//! is how `spikes/service_sweep.sh` and the omniORB client reach it.
//!
//! # The store it serves
//!
//! One service type, `moe::Expert`, and five offers of it — enough that a
//! query can fit under one `how_many` and not under another, which is the
//! distinction D022 §5 is entirely about. Two of the five carry gaps
//! (`specialization`, `latency_p50` absent) so that a foreign client sees a
//! `PropertySeq` shorter than ten and can tell an absent property from an
//! empty one.

use std::io::Write;

use orbweaver_giop::server::Server;
use orbweaver_giop::trading_server::TradingServer;
use orbweaver_giop::{Ior, Result};
use orbweaver_trading::service_type::{PropertyKind, PropertyMode, PropertySchema, ServiceType};
use orbweaver_trading::{Offer, Residency};

fn expert(id: &str, specialization: Option<&str>, cost: f64, p50: Option<f64>) -> Offer {
    Offer {
        id: id.to_owned(),
        specialization: specialization.map(str::to_owned),
        cost,
        latency_p50: p50,
        latency_p99: cost * 20.0,
        load: 0.25,
        residency: Residency::Resident,
        mem_footprint: 1_048_576,
        placement_node: "node-a".to_owned(),
        route_freq: 16,
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hold = args.iter().any(|a| a == "--hold");
    let port: u16 =
        args.windows(2).find(|w| w[0] == "--port").and_then(|w| w[1].parse().ok()).unwrap_or(0);
    let out = args
        .iter()
        .find(|a| !a.starts_with("--") && a.parse::<u16>().is_err())
        .cloned()
        .unwrap_or_else(|| "spikes/trading.ior".to_owned());

    match run(&out, port, hold) {
        Ok(0) => {
            println!("\ntrading-service: PASS");
            std::process::ExitCode::SUCCESS
        }
        Ok(n) => {
            println!("\ntrading-service: FAIL — {n} check(s) failed");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            println!("\ntrading-service: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(out: &str, port: u16, hold: bool) -> Result<u32> {
    let key = b"TradingService".to_vec();
    let server = Server::bind(&format!("127.0.0.1:{port}"), key.clone())?;
    let bound = server.local_addr()?.port();

    let trader = TradingServer::new("127.0.0.1", bound, key);
    trader.with_store(|s| {
        s.declare(
            ServiceType::declare(
                "moe::Expert",
                "IDL:moe/Expert:1.0",
                vec![
                    PropertySchema::new("cost", PropertyKind::Float, PropertyMode::Normal),
                    PropertySchema::new(
                        "placement_node",
                        PropertyKind::Text,
                        PropertyMode::MandatoryReadonly,
                    ),
                ],
            )
            .expect("the fixture type is legal"),
        )
        .expect("nothing is declared twice");

        for offer in [
            expert("math-fast", Some("math"), 1.0, Some(2.0)),
            expert("math-slow", Some("math"), 2.0, Some(9.0)),
            expert("code-mid", Some("code"), 3.0, Some(5.0)),
            // Two gaps, so a foreign client can see a short `PropertySeq`.
            expert("untimed", Some("math"), 4.0, None),
            expert("unlabelled", None, 5.0, Some(7.0)),
        ] {
            s.register("moe::Expert", offer).expect("the fixture offers are legal");
        }
    });

    let ior = trader.lookup_ior();
    write_ior(out, &ior)?;
    println!("trading: serving CosTrading::Lookup on 127.0.0.1:{bound}, IOR in {out}");

    // Local checks, so this binary is a fixture that also measures. The
    // omniORB half is `spikes/trading_client.py`.
    let mut failures = 0;
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
        "five offers of one service type are registered",
        trader.with_store(|s| s.store().len()) == 5,
    );

    if hold {
        println!(
            "\nHOLDING — CosTrading::Lookup stays served at 127.0.0.1:{bound}; \
             point a foreign trading client at {out}"
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
