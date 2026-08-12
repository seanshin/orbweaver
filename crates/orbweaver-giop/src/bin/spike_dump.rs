//! Diagnostic companion to `spike-interop`: sends exactly one request and
//! dumps both directions of the wire, so a mismatch against a reference ORB's
//! trace can be read off directly instead of inferred.

use orbweaver_cdr::Endian;
use orbweaver_giop::{Ior, encode_request};
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
    let path = std::env::args().nth(1).unwrap_or_else(|| "spikes/echo.ior".into());
    let op = std::env::args().nth(2).unwrap_or_else(|| "ping".into());

    let ior = Ior::parse(std::fs::read_to_string(&path)?.trim())?;
    let p = ior.primary()?;
    println!("endpoint {}:{}  object_key {} bytes\n", p.host, p.port, p.object_key.len());

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
        let msg = encode_request(endian, id, &p.object_key, &op, true, |_| {});
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
