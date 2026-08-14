//! Measures the limit stream E removed: K clients holding sessions on ONE
//! server at the same time, and the cap that bounds them.
//!
//! Until this batch `Server` accepted one connection, served it to
//! completion, and only then accepted the next — every service we shipped
//! told its harness the foreign client had to be the only client while it
//! ran. This spike is the counter-measurement, against a real servant (the
//! CosNaming server, F6's), not a toy one.
//!
//! Usage: `spike-concurrent [clients]` (default 8).
//!
//! # What is measured, and what is not
//!
//! - **Overlap** is read off the server's own counters
//!   ([`orbweaver_giop::server::ServerStats::peak_active`]), not inferred
//!   from timing. Every client binds a name, waits at a deadline-bounded
//!   rendezvous until all K have arrived and are still connected, then
//!   resolves every other client's binding. A server that serialized
//!   connections would leave client 2 unanswered while client 1 waits, and
//!   the rendezvous deadline would fail this run instead of hanging it.
//! - **The cap**: a second server with `max_connections` 2 is filled, and the
//!   next client is refused — accepted, told `CloseConnection` (§9.4.7: not
//!   processed, re-send elsewhere), closed, and counted.
//! - **Not measured here**: parallel *dispatch*. One servant sits behind one
//!   mutex, so operations still run one at a time; what overlaps is
//!   connections. Saying otherwise would oversell exactly the thing this
//!   spike exists to quantify.
//!
//! Self-consistency only, as with `spike-names`: our clients against our
//! server, one process, loopback. The independent peer check is a foreign
//! ORB, and the point of this batch for that check is that the foreign client
//! no longer has to be alone.

use orbweaver_giop::naming::{NameComponent, NamingContext};
use orbweaver_giop::naming_server::NamingServer;
use orbweaver_giop::server::{DEFAULT_MAX_CONNECTIONS, Server, ServerStats};
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, IiopProfile, Ior, MsgType, Version, read_message};
use std::io::Read;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// Every wait in here answers to this. A concurrency spike that can hang is
/// not a measurement.
const T: Duration = Duration::from_secs(10);

fn main() -> std::process::ExitCode {
    let clients: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .filter(|&k: &usize| k >= 2)
        .unwrap_or(8);

    match run(clients) {
        Ok(()) => {
            println!("\nconcurrency: PASS");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            println!("\nconcurrency: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

type Fallible = Result<(), Box<dyn std::error::Error>>;

fn ok(what: &str) {
    println!("  ok   {what}");
}

fn require(cond: bool, what: &str) -> Fallible {
    if cond { Ok(()) } else { Err(what.into()) }
}

fn n(id: &str) -> NameComponent {
    NameComponent::new(id)
}

/// A reference that is stored, never dialled: 192.0.2.1 is TEST-NET.
fn dummy(key: &[u8]) -> Ior {
    Ior {
        type_id: "IDL:spike/Echo:1.0".into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "192.0.2.1".into(),
            port: 4000,
            object_key: key.to_vec(),
            components: Vec::new(),
        }],
    }
}

/// A sleeping, deadline-bounded rendezvous. Returns false if the others never
/// arrived — the harness rule's wait loop, with a real wait in it.
fn all_arrived(k: usize, count: &AtomicUsize, within: Duration) -> bool {
    count.fetch_add(1, Ordering::SeqCst);
    let deadline = Instant::now() + within;
    while count.load(Ordering::SeqCst) < k {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    true
}

/// A naming server on loopback, with its counters readable from out here.
struct Served {
    root: Ior,
    stats: ServerStats,
    stop: std::sync::Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn serve_naming(cap: usize) -> Result<Served, Box<dyn std::error::Error>> {
    let mut server = Server::bind("127.0.0.1:0", b"NameService".to_vec())?;
    server.set_max_connections(cap);
    let port = server.local_addr()?.port();
    let ns = NamingServer::new("127.0.0.1", port, b"NameService".to_vec());
    let root = ns.root_ior();
    let stats = server.stats();
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let flag = std::sync::Arc::clone(&stop);
    let thread = std::thread::spawn(move || {
        let _ = server.serve_shared(&ns, move || flag.load(Ordering::SeqCst));
    });
    Ok(Served { root, stats, stop, thread: Some(thread) })
}

impl Served {
    fn shutdown(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(k: usize) -> Fallible {
    // ── phase 1: K clients, all connected at once, all answered ──
    let served = serve_naming(DEFAULT_MAX_CONNECTIONS)?;
    println!("K = {k} clients against one NamingServer (cap {DEFAULT_MAX_CONNECTIONS})");

    let arrived = AtomicUsize::new(0);
    let failures = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for i in 0..k {
            let served = &served;
            let arrived = &arrived;
            let failures = &failures;
            scope.spawn(move || {
                let mine = format!("client{i}");
                let attempt = (|| -> Fallible {
                    let mut ctx = NamingContext::connect(&served.root, T)?;
                    ctx.bind(&[n(&mine)], &dummy(mine.as_bytes()))?;
                    // Everybody is bound AND still connected before anybody
                    // reads: this is the overlap the old server forbade.
                    require(
                        all_arrived(k, arrived, T),
                        "clients never overlapped: the server is serializing connections",
                    )?;
                    for other in 0..k {
                        let name = format!("client{other}");
                        let got = ctx.resolve(&[n(&name)])?;
                        require(
                            got.primary()?.object_key == name.as_bytes(),
                            "resolve returned somebody else's reference",
                        )?;
                    }
                    Ok(())
                })();
                if let Err(e) = attempt {
                    eprintln!("  FAIL client {i}: {e}");
                    failures.fetch_add(1, Ordering::SeqCst);
                }
            });
        }
    });

    require(failures.load(Ordering::SeqCst) == 0, "at least one client failed; see above")?;
    ok(&format!("{k} clients each bound a name and resolved all {k}"));

    let peak = served.stats.peak_active();
    println!(
        "  measured overlap: peak concurrent connections {peak} of {k} \
         (accepted {}, refused {})",
        served.stats.accepted(),
        served.stats.refused()
    );
    require(
        peak >= k as u64,
        "the server never held all K connections at once — this is the failure this spike exists to catch",
    )?;
    require(served.stats.refused() == 0, "nothing should have been refused under the default cap")?;
    ok("the server itself reports the overlap; it is not inferred from timing");
    served.shutdown();
    ok("serve returned with the stop flag alone — no nudge connection, no leftover threads");

    // ── phase 2: the cap refuses, observably ──
    let capped = serve_naming(2)?;
    let mut held = Vec::new();
    for i in 0..2 {
        let mut ctx = NamingContext::connect(&capped.root, T)?;
        ctx.bind(&[n(&format!("held{i}"))], &dummy(b"held"))?;
        held.push(ctx);
    }
    require(capped.stats.active() == 2, "both clients should be counted live")?;
    ok("cap 2: two clients admitted and serving");

    // The third only reads: a refusal that raced the peer's own write could
    // be answered by a reset instead, and the goodbye is what is measured.
    let addr = {
        let p = capped.root.primary()?;
        format!("{}:{}", p.host, p.port)
    };
    let mut over = TcpStream::connect(&addr)?;
    over.set_read_timeout(Some(T))?;
    let msg = read_message(&mut over, DEFAULT_MAX_MESSAGE_SIZE)?;
    require(
        msg.msg_type == MsgType::CloseConnection,
        "over the cap a client must be told CloseConnection, not queued in silence",
    )?;
    let mut rest = Vec::new();
    over.read_to_end(&mut rest)?;
    require(rest.is_empty(), "the refused connection must then be closed")?;
    require(capped.stats.refused() == 1, "the refusal must be counted, not only logged")?;
    println!(
        "  cap behaviour: accepted {}, refused {} (limit {})",
        capped.stats.accepted(),
        capped.stats.refused(),
        2
    );
    ok("the client over the cap got §9.4.7's goodbye — not processed, safe to re-send elsewhere");

    // The refusal disturbed nobody, and a freed slot is reusable.
    let mut first = held.remove(0);
    first.resolve(&[n("held1")])?;
    ok("the admitted clients kept working through the refusal");
    drop(first);
    let deadline = Instant::now() + T;
    while capped.stats.active() > 1 {
        require(Instant::now() < deadline, "a dropped client never freed its slot")?;
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut next = NamingContext::connect(&capped.root, T)?;
    next.resolve(&[n("held0")])?;
    ok("a freed slot admitted the next client");
    drop((held, next));
    capped.shutdown();

    Ok(())
}
