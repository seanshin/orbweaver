//! Risk R7: an IOR that names an address the client cannot be served at, and
//! the rewrite that fixes it — measured on real sockets, not asserted.
//!
//! Phase 0 assumption D recorded the hazard (an ORB publishes the address it
//! *believes* it has) and unit tests cover the rewriting rules. Neither shows
//! the failure: a unit test cannot tell a correct rewrite from a plausible
//! one, because nothing in it ever dials. This spike makes the dial the
//! measurement.
//!
//! ```text
//! spike-nat prove <bind-host> <claimed-host>...   the demonstration
//! spike-nat serve <bind-addr> <ior-path>          publish-time, for a container
//! spike-nat call  <ior-path>                      dial and invoke
//! ```
//!
//! # What `prove` measures
//!
//! A real servant is bound to `<bind-host>:0` and answers `ping` with 42. For
//! each `<claimed-host>` it then, in order:
//!
//! 1. checks the servant is alive at the address it really bound, so a later
//!    failure is attributable to the address and not to a dead server;
//! 2. builds the reference an ORB would publish if it believed it were at
//!    `<claimed-host>` — same object key, same IIOP version, same port — and
//!    **requires the dial to fail**;
//! 3. rewrites that reference through an [`orbweaver_giop::nat::EndpointMap`]
//!    and **requires the same reference, so repaired, to complete a call**;
//! 4. checks the repair moved the address and nothing else: object key, IIOP
//!    version, profile count and a foreign profile's bytes all survive.
//!
//! # What it does not measure
//!
//! There is no NAT here. Both addresses are on this machine, and the client is
//! not in another routing domain — what makes the claimed address unusable is
//! that the servant is not listening on it, where in a container it would be
//! the namespace boundary. The mechanism is the same and the proof is not:
//! this shows an IOR naming the wrong address does not dial and that rewriting
//! it makes the call work. `spikes/nat/` holds the container probe that would
//! close the remaining gap; it has not been run (no Docker in this
//! environment) and says so.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_giop::nat::{EndpointMap, RawIor, RawProfile, Rule, rewrite_stringified};
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{Dispatch, Request, Server, SystemException};
use orbweaver_giop::{Connection, Ior, TAG_INTERNET_IOP};

/// Long enough that a slow machine is not a failure, short enough that an
/// unroutable address does not hold the run for the OS's own 75 seconds.
const DIAL: Duration = Duration::from_secs(3);

const OK: &str = "ok  ";
const NO: &str = "FAIL";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("prove") if args.len() >= 3 => prove(&args[1], &args[2..]),
        Some("serve") if args.len() == 3 => serve(&args[1], &args[2]),
        Some("call") if args.len() == 2 => call(&args[1]),
        _ => {
            eprintln!(
                "usage:\n  \
                 spike-nat prove <bind-host> <claimed-host>...\n  \
                 spike-nat serve <bind-addr> <ior-path>\n  \
                 spike-nat call  <ior-path>"
            );
            return std::process::ExitCode::from(2);
        }
    };
    match result {
        Ok(0) => std::process::ExitCode::SUCCESS,
        Ok(_) => std::process::ExitCode::FAILURE,
        Err(e) => {
            println!("  {NO} {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

type Fallible<T> = Result<T, Box<dyn std::error::Error>>;

/// Answers `ping` with 42, so a completed call is distinguishable from a
/// completed TCP handshake.
struct Pong;

const NAT_KEY: &[u8] = b"nat-servant";

impl Dispatch for Pong {
    // D029 §6.1's backend row: a servant that inherits `Dispatch::knows`'s
    // accept-every-key default answers for keys nobody activated, so the object
    // key selects nothing and the ADDRESS is the only thing naming a target — a
    // caller establishes that in one call. §15.3.8.6's own default
    // (`USE_ACTIVE_OBJECT_MAP_ONLY`) says `OBJECT_NOT_EXIST` and tells it
    // nothing. This compares against the key this process was bound with rather
    // than a literal typed a second time.
    fn knows(&self, object_key: &[u8]) -> bool {
        object_key == NAT_KEY
    }

    fn dispatch(&mut self, req: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        match req.operation.as_str() {
            "ping" => {
                out.put_i32(42);
                Ok(())
            }
            _ => Err(SystemException::bad_operation()),
        }
    }
}

/// A live servant, with its `Server` still reachable from out here so the
/// publish-time path can be exercised on the real thing rather than on a
/// stand-in. `Server::serve` takes `&self`, so an `Arc` is all it takes.
struct Servant {
    server: Arc<Server>,
    addr: SocketAddr,
    key: Vec<u8>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Servant {
    fn start(bind: &str) -> Fallible<Servant> {
        let key = NAT_KEY.to_vec();
        let server = Arc::new(Orb::new().server(bind, key.clone())?);
        let addr = server.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let serving = Arc::clone(&server);
        let thread = std::thread::spawn(move || {
            let _ = serving.serve(&mut Pong, move || flag.load(Ordering::SeqCst));
        });
        Ok(Servant { server, addr, key, stop, thread: Some(thread) })
    }

    fn shutdown(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The reference an ORB publishes when it believes it is at `host`: the real
/// key, the real IIOP version, the real port, and the wrong address. Plus a
/// profile of a tag we do not decode, because a rewrite that quietly drops one
/// is the failure mode worse than not rewriting.
fn published_as(host: &str, port: u16, key: &[u8]) -> Fallible<String> {
    let profile = orbweaver_giop::IiopProfile {
        version: orbweaver_giop::Version::V1_2,
        host: host.to_owned(),
        port,
        object_key: key.to_vec(),
        components: vec![orbweaver_giop::nat::alternate_address(host, port, Endian::Little)?],
    };
    let raw = RawIor {
        type_id: "IDL:spike/Echo:1.0".into(),
        profiles: vec![
            RawProfile {
                tag: TAG_INTERNET_IOP,
                body: profile.encapsulate(Endian::Little)?.finish()?,
            },
            foreign_profile(),
        ],
        endian: Endian::Little,
    };
    Ok(raw.to_stringified()?)
}

/// `TAG_MULTIPLE_COMPONENTS` (1) with an opaque body: a profile a client might
/// be able to use and we certainly cannot read.
fn foreign_profile() -> RawProfile {
    RawProfile { tag: 1, body: vec![1, 0x0b, 0xad, 0x1d, 0xea, 0, 0, 0] }
}

fn ping(ior: &Ior) -> Fallible<i32> {
    let mut conn = Connection::connect(ior, DIAL)?;
    Ok(conn.invoke_nullary("ping")?.body()?.get_i32()?)
}

fn prove(bind_host: &str, claimed: &[String]) -> Fallible<u32> {
    let servant = Servant::start(&format!("{bind_host}:0"))?;
    let real = servant.addr;
    println!("servant bound at {real} (object key {:?})", String::from_utf8_lossy(&servant.key));

    let mut fails = 0u32;

    // The control. Without it, every failure below could be a dead server.
    match ping(&Ior::parse(&published_as(bind_host, real.port(), &servant.key)?)?) {
        Ok(42) => println!("  {OK} control: ping() -> 42 at the address it really bound"),
        other => {
            println!("  {NO} control: the servant is not answering at its own address: {other:?}");
            servant.shutdown();
            return Ok(1);
        }
    }

    for host in claimed {
        println!("\nclaimed address {host}:{} — an ORB that believes it is there", real.port());
        let naive = published_as(host, real.port(), &servant.key)?;

        // (1) The failure. This is the half a unit test cannot supply.
        let started = std::time::Instant::now();
        match ping(&Ior::parse(&naive)?) {
            Err(e) => println!(
                "  {OK} unrewritten IOR did not dial after {:.2}s: {e}",
                started.elapsed().as_secs_f32()
            ),
            Ok(v) => {
                println!("  {NO} unrewritten IOR reached something and got {v} — this address is");
                println!("       not unreachable here, so the case proves nothing. Pick another.");
                fails += 1;
                continue;
            }
        }

        // (2) The repair, through the read-time entry point: a client fixing a
        // reference it received.
        let map = EndpointMap::new().with(Rule::endpoint(
            host,
            real.port(),
            &real.ip().to_string(),
            real.port(),
        ));
        let (fixed, report) = rewrite_stringified(&naive, &map)?;
        println!("  ..   map {map} → {report}");
        match ping(&Ior::parse(&fixed)?) {
            Ok(42) => println!("  {OK} rewritten IOR completed ping() -> 42"),
            Ok(v) => {
                println!("  {NO} rewritten IOR answered {v}, expected 42");
                fails += 1;
            }
            Err(e) => {
                println!("  {NO} rewritten IOR did not dial: {e}");
                fails += 1;
            }
        }

        // (3) What must not have changed while the address did.
        let before = RawIor::parse(&naive)?;
        let after = RawIor::parse(&fixed)?;
        let bp = before.to_ior()?;
        let ap = after.to_ior()?;
        let mut kept = Vec::new();
        if ap.profiles[0].object_key != bp.profiles[0].object_key {
            kept.push("object key");
        }
        if ap.profiles[0].version != bp.profiles[0].version {
            kept.push("IIOP version");
        }
        if after.type_id != before.type_id {
            kept.push("type id");
        }
        if after.profiles.len() != before.profiles.len() {
            kept.push("profile count");
        }
        if after.profiles.last() != Some(&foreign_profile()) {
            kept.push("the profile we cannot read");
        }
        if kept.is_empty() {
            println!(
                "  {OK} untouched: object key, IIOP version, type id, {} profiles including the \
                 one we cannot read",
                after.profiles.len()
            );
        } else {
            println!("  {NO} the rewrite changed what it must not: {}", kept.join(", "));
            fails += 1;
        }
    }

    fails += publish_time(bind_host)?;

    servant.shutdown();
    println!(
        "\nnat rewriting: {}",
        if fails == 0 { "PASS".to_string() } else { format!("FAIL — {fails} case(s)") }
    );
    Ok(fails)
}

/// The other half of the answer: the same repair made where the address
/// enters the wire, on a servant bound the way a container binds — wide.
///
/// `0.0.0.0` is the sharpest case for publish time. It is bindable and
/// unpublishable, so an ORB that publishes what it bound emits a reference no
/// client can dial, and there is no client-side rewrite that can guess what it
/// should have said.
fn publish_time(reachable_host: &str) -> Fallible<u32> {
    println!("\npublish time — a servant bound wide, as a container binds");
    let servant = Servant::start("0.0.0.0:0")?;
    let port = servant.addr.port();
    let mut fails = 0u32;

    match servant.server.ior_mapped("IDL:spike/Echo:1.0", &EndpointMap::new()) {
        Err(e) => println!("  {OK} with no map, publishing is refused rather than wrong: {e}"),
        Ok(ior) => {
            println!(
                "  {NO} published {}:{} — a wildcard address is not dialable",
                ior.profiles[0].host, ior.profiles[0].port
            );
            fails += 1;
        }
    }

    let map = EndpointMap::new().with(Rule::endpoint("0.0.0.0", port, reachable_host, port));
    let ior = servant.server.ior_mapped("IDL:spike/Echo:1.0", &map)?;
    println!("  ..   map {map} → published {}:{}", ior.profiles[0].host, ior.profiles[0].port);
    match ping(&ior) {
        Ok(42) => println!("  {OK} the published reference completed ping() -> 42"),
        Ok(v) => {
            println!("  {NO} published reference answered {v}, expected 42");
            fails += 1;
        }
        Err(e) => {
            println!("  {NO} published reference did not dial: {e}");
            fails += 1;
        }
    }
    if ior.profiles[0].object_key != servant.key {
        println!("  {NO} publish-time rewriting altered the object key");
        fails += 1;
    }

    servant.shutdown();
    Ok(fails)
}

/// Container side: bind wide, publish through [`orbweaver_giop::nat::PUBLISH_MAP_ENV`].
fn serve(bind: &str, ior_path: &str) -> Fallible<u32> {
    let map = EndpointMap::from_env()?.unwrap_or_default();
    let server = Orb::new().server(bind, b"nat-servant".to_vec())?;
    let bound = server.local_addr()?;
    println!("bound {bound}, {}={map}", orbweaver_giop::nat::PUBLISH_MAP_ENV);
    // Inside a container the bind is wildcard and the map is what makes the
    // reference publishable. With neither, `ior_mapped` refuses here instead
    // of writing a reference that fails at every client — which is the whole
    // point of doing this at publish time.
    let ior = server.ior_mapped("IDL:spike/Echo:1.0", &map)?;
    let text = ior.to_stringified()?;
    std::fs::write(ior_path, format!("{text}\n"))?;
    println!("published {}:{} for bind {bound}", ior.profiles[0].host, ior.profiles[0].port);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    server.serve(&mut Pong, move || flag.load(Ordering::SeqCst))?;
    Ok(0)
}

/// Client side: dial whatever the reference says, and say how long it took to
/// fail. The timing is the point in a container run — a wrong address hangs,
/// it does not error immediately.
fn call(ior_path: &str) -> Fallible<u32> {
    let text = std::fs::read_to_string(ior_path)?;
    let ior = Ior::parse(text.trim())?;
    let p = ior.primary()?;
    println!("dialing {}:{}", p.host, p.port);
    let started = std::time::Instant::now();
    match ping(&ior) {
        Ok(42) => {
            println!("  {OK} ping() -> 42 in {:.2}s", started.elapsed().as_secs_f32());
            Ok(0)
        }
        Ok(v) => {
            println!("  {NO} ping() -> {v}");
            Ok(1)
        }
        Err(e) => {
            println!("  {NO} {e} after {:.2}s", started.elapsed().as_secs_f32());
            Ok(1)
        }
    }
}
