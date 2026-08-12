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
use orbweaver_giop::{IiopProfile, Ior, Version};
use orbweaver_object::{ObjectOps, get_reference, is_equivalent, put_reference};
use orbweaver_registry::Registry;

/// Matches `spikes/echo.idl`.
const TYPE_ID: &str = "IDL:spike/Echo:1.0";

struct Echo {
    /// Our own reference, so `get_self` and `same_as` have something to
    /// return and compare against.
    self_ref: Ior,
    /// Answers `_is_a` locally from the inheritance graph (PLAN §4.7).
    registry: Registry,
    /// When set, the next call to `ping` is answered with LOCATION_FORWARD to
    /// this reference instead of a result — the ServantLocator behaviour that
    /// moves an object without the caller noticing.
    forward_ping_to: Option<Ior>,
    calls: u32,
    /// Versions actually seen on the wire. Recorded because "we tested three
    /// GIOP versions" is only true if the peer really used three, and an ORB
    /// that ignores a version option would otherwise produce three identical
    /// passes that prove one thing.
    seen: std::collections::BTreeSet<(u8, u8)>,
}

impl Dispatch for Echo {
    fn forward(&mut self, request: &Request) -> Option<Ior> {
        // Forward exactly once, so a client that follows it gets an answer on
        // the retry rather than looping.
        if request.operation == "ping"
            && let Some(to) = self.forward_ping_to.take()
        {
            // Logged so a test can prove the forward was actually emitted,
            // rather than inferring it from a call that succeeded anyway.
            println!(
                "emitted LOCATION_FORWARD for ping() to {} bytes of key",
                to.primary().map(|p| p.object_key.len()).unwrap_or(0)
            );
            return Some(to);
        }
        None
    }

    fn dispatch(&mut self, req: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        self.calls += 1;
        if self.seen.insert((req.version.major, req.version.minor)) {
            println!("first request at {} ({:?})", req.version, req.endian);
        }
        // Every ORB probes with these, and without `_is_a` there is no
        // narrowing at all.
        if ObjectOps::handles(&req.operation) {
            return ObjectOps {
                registry: &self.registry,
                type_id: TYPE_ID,
                reference: Some(&self.self_ref),
            }
            .dispatch(req, out);
        }

        let mut args = req.body().map_err(|_| SystemException::marshal())?;

        match req.operation.as_str() {
            "get_self" => {
                put_reference(out, Some(&self.self_ref)).map_err(|_| SystemException::marshal())?;
            }

            "same_as" => {
                let other = get_reference(&mut args).map_err(|_| SystemException::marshal())?;
                out.put_bool(other.as_ref().is_some_and(|o| is_equivalent(&self.self_ref, o)));
            }

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

            "echo_wstring" => {
                // The wire form depends on the request's GIOP version, so the
                // codec is built from it rather than assumed.
                let w = orbweaver_giop::codeset::WideCodec::new(
                    req.version,
                    orbweaver_giop::codeset::CodeSetId::UTF_16,
                )
                .map_err(|_| SystemException::marshal())?;
                let s = w.get_wstring(&mut args).map_err(|_| SystemException::marshal())?;
                w.put_wstring(out, &s).map_err(|_| SystemException::marshal())?;
            }

            "blob" => {
                let n = args.get_u32().map_err(|_| SystemException::marshal())? as usize;
                out.put_u32(n as u32);
                for i in 0..n {
                    out.put_octet((i % 251) as u8);
                }
            }

            "blob_sum" => {
                let bytes = args.get_octet_seq().map_err(|_| SystemException::marshal())?;
                let sum: u64 = bytes.iter().map(|&b| b as u64).sum();
                out.put_i32((sum % 2_147_483_647) as i32);
            }

            "echo_any" => {
                // Relayed verbatim: decode the TypeCode to prove we can, then
                // echo the original bytes. Re-encoding would test our encoder
                // against itself instead of against the peer's.
                let before = args.offset();
                orbweaver_giop::typecode::decode(&mut args)
                    .map_err(|_| SystemException::marshal())?;
                let raw = req.body().map_err(|_| SystemException::marshal())?;
                let all = raw.buffer();
                out.put_bytes(&all[before..]);
            }

            _ => return Err(SystemException::bad_operation()),
        }
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_path = std::env::args().nth(1).unwrap_or_else(|| "spikes/server.ior".into());
    let host = std::env::args().nth(2).unwrap_or_else(|| "127.0.0.1".into());
    let port = std::env::args().nth(3).unwrap_or_else(|| "0".into());

    let mut server = Server::bind(&format!("127.0.0.1:{port}"), b"OrbweaverEcho".to_vec())?;
    // Neither available peer emits GIOP fragments, so the only way to test
    // fragment handling against an independent implementation is to make *us*
    // the fragmenting side and see whether they reassemble.
    if let Some(t) = std::env::var("ORBWEAVER_FRAGMENT_THRESHOLD").ok().and_then(|v| v.parse().ok())
    {
        server.set_fragment_threshold(t);
        println!("fragmenting outbound messages above {t} bytes");
    }
    let bound = server.local_addr()?;

    // The published host is separate from the bind address on purpose: behind
    // NAT or in a container they differ, and publishing the bind address is
    // the failure Phase 0 assumption D reproduced.
    let ior = server.ior(TYPE_ID, &host)?;
    std::fs::write(&out_path, ior.to_stringified()?)?;
    let _ = (IiopProfile {
        version: Version::V1_2,
        host: host.clone(),
        port: bound.port(),
        object_key: vec![],
        components: vec![],
    },);

    println!("listening on {bound}, publishing {host}:{}", bound.port());
    println!("IOR written to {out_path}");
    println!("READY");

    // `_is_a` is answered from IDL rather than over the network, so the
    // fixture's own interface definition is loaded here.
    let mut registry = Registry::new();
    let idl = std::fs::read_to_string("spikes/echo.idl")?;
    registry.load(&orbweaver_idl::parse(&idl).map_err(|e| e.to_string())?)?;

    // A second reference, on the same server, that `ping` is forwarded to.
    // Both keys reach the same servant, so a client that follows the forward
    // gets an answer and one that ignores it gets nothing.
    let forward_ping_to = std::env::var("ORBWEAVER_FORWARD_PING").ok().map(|_| ior.clone());
    if forward_ping_to.is_some() {
        println!("ping() will answer with LOCATION_FORWARD");
    }

    let mut echo = Echo {
        self_ref: ior.clone(),
        registry,
        forward_ping_to,
        calls: 0,
        seen: Default::default(),
    };
    server.serve(&mut echo, || false)?;
    Ok(())
}
