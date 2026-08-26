//! Phase 1 proof for the serving half: a stock ORB calls *us*.
//!
//! Phase 0 showed we could call an existing CORBA system. This is the
//! direction that was never tested, and `docs/PLAN.md` §7 commits to both.
//!
//! Serves `spikes/echo.idl`, publishes an IOR, and waits for an omniORB client.
//!
//! Usage: `spike-server <ior-out-path> [host] [port]`
//!
//! Environment, all optional:
//!
//! * `ORBWEAVER_FRAGMENT_THRESHOLD=<bytes>` — fragment outbound messages
//!   above it;
//! * `ORBWEAVER_FORWARD_PING=1` — the first `ping` is answered with a
//!   `LOCATION_FORWARD` to this same server (the one-server shape
//!   `run_checks.sh` measures);
//! * `ORBWEAVER_FORWARD_TO=<ior-file>` — every `ping` is answered with a
//!   forward to the reference in that file **for as long as the file is
//!   there**; once it is removed, `ping` is served here. This is the
//!   two-server shape `spikes/perm_fallback.sh` needs: the file is the
//!   forwarded-to server's published IOR, and removing it is how the harness
//!   tells this server the target is gone, so that a client which restarts at
//!   the original address gets an answer it can be seen to have got here;
//! * `ORBWEAVER_FORWARD_STATUS=permanent|temporary` — which status the
//!   `ORBWEAVER_FORWARD_TO` forward travels under (default temporary);
//! * `ORBWEAVER_PING_ANSWER=<long>` — what `ping` returns (default 42), so two
//!   servers of this binary can be told apart from the client's side.
//!
//! Every forward and every locally served `ping` is logged on stdout with a
//! running count, so a harness reading the log can say how many requests
//! reached this address before and after a change it made — the count is the
//! measurement; the client's answer only corroborates it.
//!
//! **The count is taken from a `ROW` line, not from the English beside it.**
//! `spikes/perm_fallback.sh` used to `grep -c "forwarded ping()"` and
//! `grep -c "served ping()"`, which made the wording of two `println!`s below
//! part of a gate; the tab-separated rows this binary now also prints are the
//! channel a script keys on (see `rows/mod.rs`). The prose is unchanged and
//! stays: it is what a human reads when the gate goes red.

use orbweaver_cdr::Encoder;
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{Dispatch, Request, SystemException};
use orbweaver_giop::{Forward, IiopProfile, Ior, Version};
use orbweaver_object::{ObjectOps, get_reference, is_equivalent, put_reference};
use orbweaver_registry::{Registry, Strictness};

/// The `ROW` channel `spikes/perm_fallback.sh` (and, when it is next touched,
/// `spikes/jacorb_giop11.sh` and `run_checks.sh`) keys its counts on, so that
/// no verdict depends on the wording of a `println!` below. See its own docs.
#[path = "rows/mod.rs"]
mod rows;

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
    /// `ORBWEAVER_FORWARD_TO`: while this file holds an IOR, `ping` is
    /// forwarded to it; once the file is gone, `ping` is served here.
    forward_to: Option<std::path::PathBuf>,
    /// `ORBWEAVER_FORWARD_STATUS`: whether the `forward_to` redirect is
    /// `LOCATION_FORWARD_PERM` rather than `LOCATION_FORWARD`.
    for_good: bool,
    /// `ORBWEAVER_PING_ANSWER`.
    ping_answer: i32,
    /// Forwards emitted and `ping`s served here, for the log lines.
    forwarded: u32,
    pinged: u32,
    calls: u32,
    /// Versions actually seen on the wire. Recorded because "we tested three
    /// GIOP versions" is only true if the peer really used three, and an ORB
    /// that ignores a version option would otherwise produce three identical
    /// passes that prove one thing.
    seen: std::collections::BTreeSet<(u8, u8)>,
}

impl Dispatch for Echo {
    fn redirect(&mut self, request: &Request) -> Option<Forward> {
        if request.operation != "ping" {
            return None;
        }
        if let Some(path) = &self.forward_to {
            // The two-server shape. The file's presence is the switch: the
            // harness removes it when it stops the forwarded-to server, and
            // from then on `ping` is served here, so a client that comes back
            // to this address is answered — and counted — here.
            let Ok(text) = std::fs::read_to_string(path) else {
                return None;
            };
            return match Ior::parse(text.trim()) {
                Ok(to) => {
                    self.forwarded += 1;
                    let status =
                        if self.for_good { "LOCATION_FORWARD_PERM" } else { "LOCATION_FORWARD" };
                    println!(
                        "forwarded ping() #{} with {status} to {}:{}",
                        self.forwarded,
                        to.primary().map(|p| p.host.as_str()).unwrap_or("?"),
                        to.primary().map(|p| p.port).unwrap_or(0)
                    );
                    rows::Row {
                        seat: rows::SERVE,
                        event: rows::event::FORWARDED,
                        op: "ping",
                        giop: &rows::giop(request.version),
                        endian: rows::endian(request.endian),
                        n: &self.forwarded.to_string(),
                        note: if self.for_good {
                            rows::note::PERMANENT
                        } else {
                            rows::note::TEMPORARY
                        },
                        ..Default::default()
                    }
                    .emit();
                    Some(if self.for_good {
                        Forward::Permanent(to)
                    } else {
                        Forward::Temporary(to)
                    })
                }
                Err(e) => {
                    eprintln!("ORBWEAVER_FORWARD_TO holds no IOR ({e}); serving ping() here");
                    None
                }
            };
        }
        // Forward exactly once, so a client that follows it gets an answer on
        // the retry rather than looping.
        if let Some(to) = self.forward_ping_to.take() {
            // Logged so a test can prove the forward was actually emitted,
            // rather than inferring it from a call that succeeded anyway.
            println!(
                "emitted LOCATION_FORWARD for ping() to {} bytes of key",
                to.primary().map(|p| p.object_key.len()).unwrap_or(0)
            );
            self.forwarded += 1;
            rows::Row {
                seat: rows::SERVE,
                event: rows::event::FORWARDED,
                op: "ping",
                giop: &rows::giop(request.version),
                endian: rows::endian(request.endian),
                n: &self.forwarded.to_string(),
                note: rows::note::ONCE,
                ..Default::default()
            }
            .emit();
            return Some(Forward::Temporary(to));
        }
        None
    }

    fn dispatch(&mut self, req: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        self.calls += 1;
        let (gv, end) = (rows::giop(req.version), rows::endian(req.endian));
        if self.seen.insert((req.version.major, req.version.minor)) {
            println!("first request at {} ({:?})", req.version, req.endian);
            rows::Row {
                seat: rows::SERVE,
                event: rows::event::FIRST,
                giop: &gv,
                endian: end,
                ..Default::default()
            }
            .emit();
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

            "ping" => {
                self.pinged += 1;
                println!("served ping() #{} here -> {}", self.pinged, self.ping_answer);
                rows::Row {
                    seat: rows::SERVE,
                    event: rows::event::SERVED,
                    op: "ping",
                    giop: &gv,
                    endian: end,
                    n: &self.pinged.to_string(),
                    got: &self.ping_answer.to_string(),
                    ..Default::default()
                }
                .emit();
                out.put_i32(self.ping_answer);
            }

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
    rows::header();
    let out_path = std::env::args().nth(1).unwrap_or_else(|| "spikes/server.ior".into());
    let host = std::env::args().nth(2).unwrap_or_else(|| "127.0.0.1".into());
    let port = std::env::args().nth(3).unwrap_or_else(|| "0".into());

    let mut server = Orb::new().server(&format!("127.0.0.1:{port}"), b"OrbweaverEcho".to_vec())?;
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
    // fixture's own interface definition is loaded here — through the resolving
    // loader, because `_is_a` is an inheritance answer and inheritance is
    // precisely what an unresolved `#include` costs.
    let registry: Registry = orbweaver_registry::registry_from_files(
        &["spikes/echo.idl"],
        &orbweaver_idl::SearchPath::new(),
        Strictness::Grammar,
    )?;

    // A second reference, on the same server, that `ping` is forwarded to.
    // Both keys reach the same servant, so a client that follows the forward
    // gets an answer and one that ignores it gets nothing.
    let forward_ping_to = std::env::var("ORBWEAVER_FORWARD_PING").ok().map(|_| ior.clone());
    if forward_ping_to.is_some() {
        println!("ping() will answer with LOCATION_FORWARD");
    }
    // The two-server shape: forward `ping` to another server's published IOR
    // for as long as that file exists.
    let forward_to = std::env::var_os("ORBWEAVER_FORWARD_TO").map(std::path::PathBuf::from);
    let for_good = match std::env::var("ORBWEAVER_FORWARD_STATUS").ok().as_deref() {
        None | Some("temporary") => false,
        Some("permanent") => true,
        Some(other) => {
            return Err(format!(
                "ORBWEAVER_FORWARD_STATUS={other}: expected permanent or temporary"
            )
            .into());
        }
    };
    if let Some(path) = &forward_to {
        println!(
            "ping() will answer with {} to the IOR in {} while that file exists",
            if for_good { "LOCATION_FORWARD_PERM" } else { "LOCATION_FORWARD" },
            path.display()
        );
    }
    let ping_answer = match std::env::var("ORBWEAVER_PING_ANSWER") {
        Ok(v) => v.parse::<i32>().map_err(|e| format!("ORBWEAVER_PING_ANSWER={v}: {e}"))?,
        Err(_) => 42,
    };

    let mut echo = Echo {
        self_ref: ior.clone(),
        registry,
        forward_ping_to,
        forward_to,
        for_good,
        ping_answer,
        forwarded: 0,
        pinged: 0,
        calls: 0,
        seen: Default::default(),
    };
    server.serve(&mut echo, || false)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use orbweaver_cdr::Endian;
    use orbweaver_giop::server::decode_request;

    use super::*;

    fn a_reference(port: u16) -> Ior {
        Ior {
            type_id: TYPE_ID.into(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: "127.0.0.1".into(),
                port,
                object_key: b"OrbweaverEcho".to_vec(),
                components: vec![],
            }],
        }
    }

    /// A real GIOP `Request`, encoded and decoded as the wire does it.
    fn request(op: &str) -> Request {
        let bytes = orbweaver_giop::encode_request(
            Version::V1_2,
            Endian::Big,
            1,
            b"OrbweaverEcho",
            op,
            true,
            |_| {},
        )
        .unwrap();
        decode_request(orbweaver_giop::read_message(&mut &bytes[..], 1 << 20).unwrap()).unwrap()
    }

    fn servant(forward_to: Option<std::path::PathBuf>, for_good: bool) -> Echo {
        Echo {
            self_ref: a_reference(1),
            registry: Registry::default(),
            forward_ping_to: None,
            forward_to,
            for_good,
            ping_answer: 2,
            forwarded: 0,
            pinged: 0,
            calls: 0,
            seen: Default::default(),
        }
    }

    /// `spikes/perm_fallback.sh` counted `grep -c "forwarded ping()"` and
    /// `grep -c "served ping()"` — two `println!` sentences from this file
    /// standing in for two classes. These are the rows that replaced them.
    /// Reword either sentence and this test stays green; change a token in a
    /// row and it goes red.
    #[test]
    fn the_forward_and_the_local_answer_are_told_apart_by_a_row() {
        // Served here: the `n` column is the running count the script reads
        // before and after it kills the forwarded-to server.
        let mut echo = servant(None, false);
        let req = request("ping");
        assert!(echo.redirect(&req).is_none());
        let _ = rows::captured::drain();
        let mut out = Encoder::new(req.endian);
        echo.dispatch(&req, &mut out).unwrap();
        assert_eq!(
            rows::captured::drain(),
            vec![
                "ROW\tserve\tfirst\t-\t1.2\tBE\t-\t-\t-\t-\t-".to_owned(),
                "ROW\tserve\tserved\tping\t1.2\tBE\t-\t1\t-\t2\t-".to_owned(),
            ]
        );

        // Forwarded, both statuses. The file's presence is the switch, so the
        // target's published IOR is written where the servant looks for it.
        for (for_good, note) in [(false, "temporary"), (true, "permanent")] {
            let path = std::env::temp_dir()
                .join(format!("orbweaver-rows-{}-{for_good}.ior", std::process::id()));
            std::fs::write(&path, a_reference(4321).to_stringified().unwrap()).unwrap();
            let mut echo = servant(Some(path.clone()), for_good);
            let _ = rows::captured::drain();
            assert!(echo.redirect(&req).is_some());
            assert_eq!(
                rows::captured::drain(),
                vec![format!("ROW\tserve\tforwarded\tping\t1.2\tBE\t-\t1\t-\t-\t{note}")]
            );

            // The file gone is how the harness says the target died: from
            // here on `ping` is served at this address, and no row says
            // `forwarded`.
            std::fs::remove_file(&path).unwrap();
            let _ = rows::captured::drain();
            assert!(echo.redirect(&req).is_none());
            assert!(rows::captured::drain().is_empty());
        }
    }
}
