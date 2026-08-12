//! Proves fragment reassembly against a peer that really fragments.
//!
//! A self round-trip only shows our splitter agrees with our joiner. The
//! interesting question is whether a stock ORB's fragments reassemble, and
//! whether ours are accepted — so the peer is run with a small
//! `giopMaxMsgSize` to force it.
//!
//! Usage: `spike-fragment <ior-file> [sizes...]`

use orbweaver_giop::{Connection, Ior};
use std::time::Duration;

fn main() -> std::process::ExitCode {
    let path = std::env::args().nth(1).unwrap_or_else(|| "spikes/echo.ior".into());
    let sizes: Vec<usize> = std::env::args()
        .skip(2)
        .filter_map(|s| s.parse().ok())
        .collect();
    let sizes = if sizes.is_empty() { vec![100usize, 8_000, 40_000, 250_000] } else { sizes };

    match run(&path, &sizes) {
        Ok(0) => {
            println!("\nfragmentation: PASS");
            std::process::ExitCode::SUCCESS
        }
        Ok(n) => {
            println!("\nfragmentation: FAIL — {n} case(s)");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            println!("\nfragmentation: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(path: &str, sizes: &[usize]) -> Result<u32, Box<dyn std::error::Error>> {
    let ior = Ior::parse(std::fs::read_to_string(path)?.trim())?;
    let mut fails = 0;

    for &n in sizes {
        let mut conn = Connection::connect(&ior, Duration::from_secs(20))?;
        // Small enough that our own outbound messages split too, so both
        // directions of fragmentation are exercised rather than just receive.
        conn.set_fragment_threshold(4096);

        // Inbound: the peer returns a large sequence and must fragment it.
        let got = conn
            .invoke("blob", |e| e.put_u32(n as u32))
            .and_then(|r| {
                let mut b = r.body()?;
                Ok(b.get_octet_seq()?.to_vec())
            })?;
        let expected: Vec<u8> = (0..n).map(|i| (i % 251) as u8).collect();
        if got != expected {
            println!("  FAIL blob({n}) returned {} bytes, content mismatch", got.len());
            fails += 1;
            continue;
        }

        // Outbound: send the same payload back and have the peer checksum it,
        // which fails loudly if our fragments reassembled wrong at their end.
        let sum_expected: u64 = expected.iter().map(|&b| b as u64).sum::<u64>() % 2_147_483_647;
        let payload = expected.clone();
        let sum = conn
            .invoke("blob_sum", move |e| e.put_octet_seq(&payload))
            .and_then(|r| Ok(r.body()?.get_i32()?))?;
        if sum as u64 != sum_expected {
            println!("  FAIL blob_sum({n}) -> {sum}, expected {sum_expected}");
            fails += 1;
            continue;
        }
        let frags = conn.max_reply_fragments();
        if n > 4096 && frags < 2 {
            // The peer sent it whole, so nothing was reassembled and this case
            // proves only that a large unfragmented message works.
            println!("  note {n} bytes ok, but the peer did not fragment ({frags} piece)");
        } else {
            println!("  ok   {n} bytes both ways — inbound arrived in {frags} fragment(s)");
        }
    }
    Ok(fails)
}
