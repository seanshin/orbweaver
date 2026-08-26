//! Measures request multiplexing and connection pooling against whatever peer
//! it is pointed at.
//!
//! Usage:
//!
//! ```text
//! spike-mux                                  # our own server, in process
//! spike-mux <ior-file> [calls] [1.0|1.1|1.2] # a foreign peer, over the wire
//! ```
//!
//! # What is measured, and what a run cannot prove
//!
//! Three things, and they are not the same claim:
//!
//! 1. **Several requests on one connection at once.** Read off
//!    [`MuxStats::peak_in_flight`], which counts requests *written and
//!    unanswered* — the peer has the bytes — rather than callers queued for
//!    anything. A counter outside the lock could not tell those apart, which
//!    is the mistake the concurrent-dispatch batch made and recorded.
//! 2. **Correlation.** Every reply is checked against the operation its caller
//!    asked for, because a correlation bug does not fail loudly: it hands
//!    caller A caller B's answer, and both calls "succeed".
//! 3. **Out-of-order replies**, from [`MuxStats::out_of_order`]. This is the
//!    one a peer has to volunteer, and **a zero here is a fact about the peer,
//!    not a failure of the client** — so it is reported and never scored.
//!
//! # What the peers actually did (2026-08-14)
//!
//! Both of them answer out of order, in their **default** configurations, with
//! no client-side or server-side option set:
//!
//! - **omniORB 4.3.4** (`spikes/echo_server.py`), 12 pipelined calls:
//!   `peak_in_flight=12`, `out_of_order` 4–8 across runs. It also **fragmented
//!   the 1 MB reply** (`max_reply_fragments=2`) while other requests were in
//!   flight, and did not interleave anything into it — which is the §9.4.9
//!   behaviour the reassembler assumes, observed rather than assumed.
//! - **JacORB 3.9** (`spikes/jacorb/Server.java`), 12 pipelined calls:
//!   `peak_in_flight=12`, `out_of_order` 6–10 across runs, never fragmenting.
//!
//! This corrects an expectation worth recording because it was wrong in the
//! *safe* direction: omniORB's documented default is one thread per connection
//! (`threadPerConnectionPolicy = 1`), which reads as "answers strictly in
//! order", and the plan for this measurement was to have to start it in
//! thread-pool mode before out-of-order replies could appear at all. It needed
//! no such thing. A client that assumed reply order would have been wrong
//! against a stock omniORB.
//!
//! Our own server does read one request per connection and answers it before
//! reading the next, so the in-process mode measures (1) and (2) and cannot
//! measure (3). Saying so is the point: a self-test presented as an interop
//! result is the failure this file exists to avoid.

use orbweaver_cdr::Encoder;
use orbweaver_giop::mux::{Mux, MuxStats, Sent};
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{Dispatch, Request, SystemException};
use orbweaver_giop::{IiopProfile, Ior, Version};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Every wait answers to this. A concurrency spike that can hang is not a
/// measurement.
const T: Duration = Duration::from_secs(20);

type Fallible = Result<(), Box<dyn std::error::Error>>;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first() {
        None => in_process(),
        Some(path) => {
            let calls: usize =
                args.get(1).and_then(|a| a.parse().ok()).filter(|&n: &usize| n >= 2).unwrap_or(8);
            let cap = match args.get(2).map(String::as_str) {
                None | Some("1.2") => None,
                Some("1.1") => Some(Version::V1_1),
                Some("1.0") => Some(Version::V1_0),
                Some(other) => {
                    println!("mux: FAIL — unknown version {other}, expected 1.0, 1.1 or 1.2");
                    return std::process::ExitCode::FAILURE;
                }
            };
            against_peer(path, calls, cap)
        }
    };
    match outcome {
        Ok(()) => {
            println!("\nmux: PASS");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            println!("\nmux: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn ok(what: &str) {
    println!("  ok   {what}");
}

fn note(what: &str) {
    println!("  --   {what}");
}

fn require(cond: bool, what: &str) -> Fallible {
    if cond { Ok(()) } else { Err(what.into()) }
}

fn report(stats: MuxStats) {
    println!(
        "  stats: sent={} answered={} peak_in_flight={} out_of_order={} orphaned={} \
         max_reply_fragments={}",
        stats.sent,
        stats.answered,
        stats.peak_in_flight,
        stats.out_of_order,
        stats.orphaned,
        stats.max_reply_fragments
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Against our own server
// ─────────────────────────────────────────────────────────────────────────────

/// Answers `echo(long) -> long`, and knows every key so one connection can
/// address more than one object.
struct Echo;

impl Dispatch for Echo {
    fn dispatch(&mut self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        let n = request.body().and_then(|mut b| b.get_i32().map_err(Into::into)).unwrap_or(-1);
        out.put_i32(n);
        Ok(())
    }
}

fn in_process() -> Fallible {
    println!("multiplexing and pooling, against our own server (self-test only)");

    let server = Orb::new().server("127.0.0.1:0", b"echo".to_vec())?;
    let addr = server.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let serving = std::thread::spawn(move || {
        let _ = server.serve(&mut Echo, || flag.load(Ordering::SeqCst));
    });

    let outcome = (|| -> Fallible {
        let ior = ior_at(addr, b"echo");
        let mux = Mux::connect(&ior, T)?;
        require(mux.multiplexes(), "a 1.2 cleartext connection must multiplex")?;
        ok("the connection multiplexes");

        // Pipeline first, collect afterwards: nothing may be waited on until
        // every request is out, or there is nothing concurrent about it.
        const K: i32 = 16;
        let pending: Vec<_> = (0..K)
            .map(|i| mux.send(b"echo", "echo", move |e: &mut Encoder| e.put_i32(i)))
            .collect::<Result<_, _>>()?;
        let ids: Vec<u32> = pending.iter().map(|p| p.request_id()).collect();
        let mut answers = Vec::new();
        for (i, p) in pending.into_iter().enumerate() {
            match p.wait(T)? {
                Sent::Reply(r) => answers.push((i as i32, r.body()?.get_i32()?)),
                Sent::Forward(_) => return Err("no forward was expected".into()),
            }
        }
        require(
            answers.iter().all(|(asked, got)| asked == got),
            "every reply must carry its own caller's argument back",
        )?;
        ok(&format!("{K} pipelined calls, each answered with its own argument"));
        require(
            ids.windows(2).all(|w| w[0] < w[1]),
            "request ids must be allocated in the order they are written",
        )?;
        ok("request ids are allocated in wire order (§13.5.1: unique within the connection)");

        let stats = mux.stats();
        report(stats);
        require(stats.peak_in_flight > 1, "more than one request must have been outstanding")?;
        ok(&format!("{} requests were on the wire at once", stats.peak_in_flight));
        require(stats.orphaned == 0, "no reply may be left unmatched")?;
        note(
            "out-of-order replies NOT measured here: this server reads one request per \
             connection and answers it before reading the next",
        );

        // Pooling: two references to one endpoint, different objects.
        let pool = Orb::new().pool();
        for key in [b"echo".as_slice(), b"other".as_slice()] {
            let reply = pool.invoke(&ior_at(addr, key), "echo", |e: &mut Encoder| e.put_i32(5))?;
            require(reply.body()?.get_i32()? == 5, "a pooled call must answer")?;
        }
        let ps = pool.stats();
        println!("  pool:  dialed={} reused={} size={}", ps.dialed, ps.reused, pool.size());
        require(ps.dialed == 1, "two references to one endpoint must share one connection")?;
        require(ps.reused >= 1, "the second reference must be a reuse")?;
        ok("two references to one endpoint shared one connection");
        Ok(())
    })();

    stop.store(true, Ordering::SeqCst);
    let _ = serving.join();
    outcome
}

fn ior_at(addr: std::net::SocketAddr, key: &[u8]) -> Ior {
    Ior {
        type_id: "IDL:spike/Echo:1.0".into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: addr.ip().to_string(),
            port: addr.port(),
            object_key: key.to_vec(),
            components: Vec::new(),
        }],
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Against a foreign peer
// ─────────────────────────────────────────────────────────────────────────────

/// Pipelines `calls` requests at the peer named by the IOR in `path`,
/// alternating an expensive operation with a cheap one.
///
/// The alternation is what gives the peer the *chance* to answer out of order:
/// a `ping` issued after a large `blob` can overtake it only if the peer
/// dispatches the two concurrently. Whether it does is the peer's
/// configuration, and the run reports what happened rather than requiring it.
fn against_peer(path: &str, calls: usize, cap: Option<Version>) -> Fallible {
    let text = std::fs::read_to_string(path)?;
    let ior = Ior::parse(text.trim())?;
    let profile = ior.primary()?;
    println!(
        "multiplexing against {}:{} ({}), {calls} calls on one connection",
        profile.host,
        profile.port,
        Version::negotiate(profile.version)
    );

    // Capping is how 1.0 and 1.1 get measured against a peer that advertises
    // 1.2: §9.4.1 lets a client speak below what the profile offers, never
    // above. It is also the only way to exercise the refusal against a real
    // ORB rather than against a scripted socket.
    let mux = match cap {
        Some(v) => {
            let mut conn = orbweaver_giop::Connection::connect(&ior, T)?;
            conn.cap_version(v);
            Mux::over(conn)
        }
        None => Mux::connect(&ior, T)?,
    };
    if !mux.multiplexes() {
        note(&format!(
            "speaking {}, where more than one request in flight is refused; calls will be \
             serialized",
            mux.version()
        ));
        match mux.send(mux.object_key().to_vec().as_slice(), "ping", |_: &mut Encoder| {}) {
            Err(orbweaver_giop::Error::MultiplexingUnsupported { version }) => {
                ok(&format!("pipelining is refused at {version}, against this peer"));
            }
            other => {
                return Err(format!("{} must refuse to pipeline: {other:?}", mux.version()).into());
            }
        }
    }

    // Big enough that marshalling it costs the peer something and gives a
    // later `ping` something to overtake, small enough to stay under every
    // default message ceiling.
    //
    // Below GIOP 1.2 it is deliberately small, because a reply this size is
    // one omniORB *fragments* — and a 1.1 fragment carries no request id, so
    // `read_message` refuses it (pre-existing, and the reason multiplexing
    // stops at 1.2 in the first place). The large case is not skipped; it is
    // measured on its own connection at the end, where its failure is the
    // measurement rather than a broken run.
    let blob: u32 = if mux.multiplexes() { 1_000_000 } else { 4_096 };
    let key = mux.object_key().to_vec();

    let started = Instant::now();
    let mut expect = Vec::new();
    let mut pending = Vec::new();
    for i in 0..calls {
        let heavy = i % 2 == 0;
        let sent = if mux.multiplexes() {
            if heavy {
                mux.send(&key, "blob", move |e: &mut Encoder| e.put_u32(blob))
            } else {
                mux.send(&key, "ping", |_: &mut Encoder| {})
            }
        } else {
            // Below 1.2 the pipelined API refuses by design, so the honest
            // comparison is the serialized one.
            let answered = if heavy {
                mux.call_on(&key, "blob", |e: &mut Encoder| e.put_u32(blob), T)
            } else {
                mux.call_on(&key, "ping", |_: &mut Encoder| {}, T)
            };
            check(answered?, heavy, blob)?;
            continue;
        };
        pending.push(sent?);
        expect.push(heavy);
    }
    let all_out = started.elapsed();

    for (p, heavy) in pending.into_iter().zip(expect) {
        check(p.wait(T)?, heavy, blob)?;
    }
    ok(&format!("{calls} calls, each answered with its own operation's result"));
    println!(
        "  timing: all requests written in {all_out:?}, all replies in {:?}",
        started.elapsed()
    );

    let stats = mux.stats();
    report(stats);
    if mux.multiplexes() {
        require(
            stats.peak_in_flight > 1,
            "the peer accepted no second request before answering the first — \
             pipelining did not happen",
        )?;
        ok(&format!("{} requests were on this peer's wire at once", stats.peak_in_flight));
    }
    if stats.out_of_order > 0 {
        ok(&format!(
            "{} replies arrived while an older request was still outstanding — this peer \
             really does answer out of order",
            stats.out_of_order
        ));
    } else {
        note(
            "this peer answered strictly in order: OUT-OF-ORDER CORRELATION IS UNMEASURED \
             in this run. Expected when the version is capped below 1.2, since then only one \
             request is in flight and there is nothing to reorder; against a 1.2 peer it \
             means the server dispatched this connection's requests one at a time",
        );
    }
    require(stats.orphaned == 0, "no reply may be left unmatched")?;
    require(mux.is_usable(), "the connection must survive the whole run")?;

    // Pooling, against the same peer: a second reference to the same endpoint
    // must not dial again.
    let pool = Orb::new().pool();
    let mut second = ior.clone();
    if let Some(p) = second.profiles.first_mut() {
        p.object_key.push(b'!'); // a different object at the same endpoint
    }
    let live = pool.invoke(&ior, "ping", |_: &mut Encoder| {})?;
    require(live.body()?.get_i32()? == 42, "ping must answer 42")?;
    // The second reference names an object the peer does not have; what is
    // being measured is that no second connection was dialed, so any reply —
    // including OBJECT_NOT_EXIST — settles it.
    let _ = pool.invoke(&second, "ping", |_: &mut Encoder| {});
    let ps = pool.stats();
    println!("  pool:  dialed={} reused={} size={}", ps.dialed, ps.reused, pool.size());
    require(ps.dialed == 1, "a second reference to one endpoint must not dial again")?;
    ok("two references to one endpoint shared one connection");

    if let Some(v) = cap {
        fragment_probe(&ior, v)?;
    }
    Ok(())
}

/// Asks a peer for a reply big enough to fragment, at a version where a
/// fragment carries no request id, and records what happens.
///
/// On its own connection, and last, because the expected outcome *kills the
/// connection*: `read_message` refuses a GIOP 1.1 `Fragment` (it has no
/// `FragmentHeader`, so it cannot be attributed to a request) and the framing
/// is unknowable from there.
///
/// This is the concrete cost of the version rule, measured instead of
/// asserted. It is also why multiplexing stops at 1.2: with one request in
/// flight this refusal fails one call, and with N in flight it would fail all
/// N for a decision only the peer made.
fn fragment_probe(ior: &Ior, v: Version) -> Fallible {
    const BIG: u32 = 1_000_000;
    let mut conn = orbweaver_giop::Connection::connect(ior, T)?;
    conn.cap_version(v);
    let mux = Mux::over(conn);
    let key = mux.object_key().to_vec();
    match mux.call_on(&key, "blob", |e: &mut Encoder| e.put_u32(BIG), T) {
        Ok(Sent::Reply(r)) => {
            let n = r.body()?.get_u32()?;
            require(n == BIG, "the reply must carry the size that was asked for")?;
            note(&format!(
                "this peer answered a {BIG}-byte reply at {v} without fragmenting \
                 (max_reply_fragments={})",
                mux.stats().max_reply_fragments
            ));
        }
        Ok(Sent::Forward(_)) => return Err("no forward was expected".into()),
        Err(f) => {
            note(&format!(
                "this peer FRAGMENTS a {BIG}-byte reply at {v}, and a fragment below GIOP 1.2 \
                 carries no request id, so the reply is refused: {f}. Pre-existing (see \
                 read_message), and the reason multiplexing stops at 1.2",
            ));
        }
    }
    Ok(())
}

fn check(sent: Sent, heavy: bool, blob: u32) -> Fallible {
    match sent {
        Sent::Reply(r) => {
            let mut b = r.body()?;
            if heavy {
                let n = b.get_u32()?;
                require(n == blob, "a blob reply must carry the size that was asked for")?;
            } else {
                let n = b.get_i32()?;
                require(n == 42, "a ping reply must be 42, not another call's answer")?;
            }
            Ok(())
        }
        Sent::Forward(_) => Err("no forward was expected".into()),
    }
}
