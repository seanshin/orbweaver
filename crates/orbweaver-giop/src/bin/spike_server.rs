//! Phase 1 proof for the serving half: a stock ORB calls *us*.
//!
//! Phase 0 showed we could call an existing CORBA system. This is the
//! direction that was never tested, and `docs/PLAN.md` §7 commits to both.
//!
//! Serves `spikes/echo.idl`, publishes an IOR, and waits for an omniORB client.
//!
//! Usage: `spike-server <ior-out-path> [host] [port]`

use orbweaver_cdr::Encoder;
use orbweaver_giop::server::{Dispatch, Request, Server, SystemException};

/// Matches `spikes/echo.idl`.
const TYPE_ID: &str = "IDL:spike/Echo:1.0";

struct Echo {
    calls: u32,
    /// Versions actually seen on the wire. Recorded because "we tested three
    /// GIOP versions" is only true if the peer really used three, and an ORB
    /// that ignores a version option would otherwise produce three identical
    /// passes that prove one thing.
    seen: std::collections::BTreeSet<(u8, u8)>,
}

impl Dispatch for Echo {
    fn dispatch(&mut self, req: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        self.calls += 1;
        if self.seen.insert((req.version.major, req.version.minor)) {
            println!("first request at {} ({:?})", req.version, req.endian);
        }
        let mut args = req.body().map_err(|_| SystemException::marshal())?;

        match req.operation.as_str() {
            "ping" => out.put_i32(42),

            "add" => {
                let a = args.get_i32().map_err(|_| SystemException::marshal())?;
                let b = args.get_i32().map_err(|_| SystemException::marshal())?;
                out.put_i32(a.wrapping_add(b));
            }

            "echo_string" => {
                // Bytes are passed through rather than decoded to a Rust
                // string: without a negotiated codeset we have no basis for
                // claiming what they mean, and echoing them verbatim is the
                // one answer that is correct under any codeset.
                let s = args.get_string_bytes().map_err(|_| SystemException::marshal())?;
                out.put_string_bytes(s);
            }

            "scale" => {
                let v = args.get_f64().map_err(|_| SystemException::marshal())?;
                let by = args.get_f64().map_err(|_| SystemException::marshal())?;
                out.put_f64(v * by);
            }

            "echo_ragged" => {
                // octet, long, short, double, octet — the padding case.
                let a = args.get_u8().map_err(|_| SystemException::marshal())?;
                let b = args.get_i32().map_err(|_| SystemException::marshal())?;
                let c = args.get_i16().map_err(|_| SystemException::marshal())?;
                let d = args.get_f64().map_err(|_| SystemException::marshal())?;
                let e = args.get_u8().map_err(|_| SystemException::marshal())?;
                out.put_octet(a);
                out.put_i32(b);
                out.put_i16(c);
                out.put_f64(d);
                out.put_octet(e);
            }

            // Object pseudo-operations. Without `_is_a` there is no narrowing,
            // and every ORB probes with one or both of these.
            "_is_a" => {
                let id = args.get_string().map_err(|_| SystemException::marshal())?;
                out.put_bool(id == TYPE_ID || id == "IDL:omg.org/CORBA/Object:1.0");
            }
            "_non_existent" | "_not_existent" => out.put_bool(false),

            _ => return Err(SystemException::bad_operation()),
        }
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_path = std::env::args().nth(1).unwrap_or_else(|| "spikes/server.ior".into());
    let host = std::env::args().nth(2).unwrap_or_else(|| "127.0.0.1".into());
    let port = std::env::args().nth(3).unwrap_or_else(|| "0".into());

    let server = Server::bind(&format!("127.0.0.1:{port}"), b"OrbweaverEcho".to_vec())?;
    let bound = server.local_addr()?;

    // The published host is separate from the bind address on purpose: behind
    // NAT or in a container they differ, and publishing the bind address is
    // the failure Phase 0 assumption D reproduced.
    let ior = server.ior(TYPE_ID, &host)?;
    std::fs::write(&out_path, ior.to_stringified()?)?;

    println!("listening on {bound}, publishing {host}:{}", bound.port());
    println!("IOR written to {out_path}");
    println!("READY");

    let mut echo = Echo { calls: 0, seen: Default::default() };
    server.serve(&mut echo, || false)?;
    Ok(())
}
