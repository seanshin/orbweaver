//! Diagnostic companion to `spike-interop`: sends exactly one request and
//! dumps both directions of the wire, so a mismatch against a reference ORB's
//! trace can be read off directly instead of inferred.

use orbweaver_cdr::Endian;
use orbweaver_giop::{Ior, Version, encode_request};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

fn hex_dump(label: &str, bytes: &[u8]) {
    println!("{label} ({} bytes)", bytes.len());
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' })
            .collect();
        println!("  {:04x}  {:<47}  {ascii}", i * 16, hex.join(" "));
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `--address` prints the endpoint and stops. **This tool dials**: it makes
    // a real call and prints what came back, which is what it is for — and it
    // is therefore the wrong instrument to point at a fixture whose traffic is
    // being recorded. Measured 2026-08-29: using it to find an address for a
    // readiness probe in `spikes/wide_rust.sh` injected a call into the tap's
    // conversation and took that script from 0 failures to 10. A probe must
    // not be a caller, and a decoder that can only decode by dialling leaves a
    // shell no choice but to parse CDR out of hex, which this repository has
    // already refused once.
    //
    // *이 도구는 다이얼한다. 기록 중인 대화에 겨누면 안 된다 — 0 실패를 10으로
    // 만들었다. **탐침은 호출자여서는 안 된다.***
    let args: Vec<String> = std::env::args().skip(1).collect();
    let address_only = args.first().map(String::as_str) == Some("--address");
    let rest: Vec<&String> =
        if address_only { args[1..].iter().collect() } else { args.iter().collect() };
    let path = rest.first().map(|s| s.to_string()).unwrap_or_else(|| "spikes/echo.ior".into());
    let op = rest.get(1).map(|s| s.to_string()).unwrap_or_else(|| "ping".into());

    let ior = Ior::parse(std::fs::read_to_string(&path)?.trim())?;
    let p = ior.primary()?;
    if address_only {
        println!("{}:{}", p.host, p.port);
        return Ok(());
    }
    println!(
        "endpoint {}:{}  object_key {} bytes  (IIOP {}.{})\n",
        p.host,
        p.port,
        p.object_key.len(),
        p.version.major,
        p.version.minor
    );

    // What the target says about security, which for most legacy targets is
    // nothing at all. §4.8 names that as the common case: where a target cannot
    // enforce a caller identity, the bridge is the only enforcement point and
    // the catalogue has to say so. Printed here so the claim is measured on
    // real IORs rather than assumed.
    match orbweaver_giop::csiv2::advertised(&p.components) {
        None => println!("csiv2   the target advertises no mechanism list"),
        Some(Err(e)) => println!("csiv2   TAG_CSI_SEC_MECH_LIST present but unreadable: {e}"),
        Some(Ok(list)) => {
            println!("csiv2   {} mechanism(s), stateful={}", list.mechanisms.len(), list.stateful);
            match list.identity_assertion() {
                Some(sas) => println!(
                    "csiv2   accepts an asserted identity, token types {:#x}",
                    sas.supported_identity_types
                ),
                None => println!("csiv2   no mechanism accepts an asserted identity"),
            }
        }
    }

    // The transport-identity row of the same §4.8 table: does this target
    // advertise a TLS listener at all? Printed for the same reason as the
    // csiv2 lines — "we can see SSLIOP endpoints" is a claim to measure on
    // real IORs, not to assume. Dialing one is D002's business, not ours yet.
    match orbweaver_giop::ssliop::advertised(&p.components) {
        None => println!("ssliop  no TAG_SSL_SEC_TRANS"),
        Some(Err(e)) => println!("ssliop  TAG_SSL_SEC_TRANS present but unreadable: {e}"),
        Some(Ok(ssl)) => {
            println!(
                "ssliop  supports={:#06x} requires={:#06x} port={}",
                ssl.target_supports, ssl.target_requires, ssl.port
            );
            if let Some((host, port)) = orbweaver_giop::ssliop::ssl_endpoint(p) {
                println!("ssliop  TLS endpoint would be {host}:{port}");
            }
        }
    }
    println!();

    let endian = match std::env::args().nth(3).as_deref() {
        Some("big") => Endian::Big,
        _ => Endian::Little,
    };
    let calls: u32 = std::env::args().nth(4).and_then(|s| s.parse().ok()).unwrap_or(1);
    println!("byte order: {endian:?}, sequential calls on one connection: {calls}\n");

    let mut sock = TcpStream::connect((p.host.as_str(), p.port))?;
    sock.set_read_timeout(Some(Duration::from_secs(3)))?;
    sock.set_nodelay(true)?;

    for id in 1..=calls {
        let version = Version::negotiate(p.version);
        let msg = encode_request(version, endian, id, &p.object_key, &op, true, |_| {})?;
        if id == 1 {
            hex_dump("REQUEST", &msg);
        }
        sock.write_all(&msg)?;
        sock.flush()?;

        let mut buf = vec![0u8; 4096];
        match sock.read(&mut buf) {
            Ok(0) => {
                println!("call {id}: peer closed the connection without replying");
                break;
            }
            Ok(n) => {
                if id == 1 {
                    println!();
                    hex_dump("RESPONSE", &buf[..n]);
                } else {
                    println!("call {id}: {n} bytes back, type={}", buf[7]);
                }
            }
            Err(e) => {
                println!("call {id}: no response — {e}");
                break;
            }
        }
    }
    Ok(())
}
