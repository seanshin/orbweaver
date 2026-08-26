//! A naming server and an event channel in one process, with the channel
//! published under its name — the fixture a foreign ORB's client points at.
//!
//! Usage: `spike-channel-by-name [ns-ior-path] [--hold] [--channel NAME]`
//!
//! # Why this is its own binary and not a flag on `spike-events`
//!
//! Two reasons, and the second is the operational one.
//!
//! - **`spike-events`' output stays byte-identical.** That is the discipline
//!   E2 held and there is no reason to spend it here: a flag would have to be
//!   parsed out of the positional-argument search that binary already has, and
//!   the compatibility claim would then rest on a flag nobody passes.
//! - **The harness kills fixtures by name.** `service_sweep.sh` and
//!   `run_checks.sh` `fkill spike-events` and `fkill spike-names`; a second
//!   process answering to either name is a fixture one run can kill out from
//!   under another, which is the "two runs at once destroy each other's peers"
//!   hazard `CLAUDE.md` records costing two diagnoses. A new name cannot be
//!   caught by an existing pattern.
//!
//! # What it is for
//!
//! `channel_found_by_name.rs` measures the Location claim with **our** client
//! at both ends, which is a self-test: a convention both ends apply cannot be
//! refuted by a round trip. This fixture exists so the client can be somebody
//! else's — omniORB resolving the name out of our naming server, narrowing to
//! `CosEventChannelAdmin::EventChannel`, and receiving an event it was never
//! handed an address for. See `spikes/event_by_name_client.py`.
//!
//! *한쪽 컨벤션은 왕복으로 반박되지 않는다. 그래서 클라이언트는 남의 것이어야
//! 한다.*
//!
//! # Both ports are ephemeral
//!
//! Neither server binds a fixed port, so this can be started beside anything
//! else. The naming root's IOR is the only thing written to disk, and it is
//! the only thing the peer is given: the channel's address is what the peer is
//! supposed to *not* be told.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use orbweaver_cdr::Endian;
use orbweaver_giop::event_server::{EventChannelServer, channel_binding_name, publish_channels};
use orbweaver_giop::naming::{NamingContext, stringify_name};
use orbweaver_giop::naming_server::NamingServer;
use orbweaver_giop::orb::Orb;
use orbweaver_giop::typecode::TypeCode;

const T: Duration = Duration::from_secs(5);

/// Publishes every channel into the naming context at `ns_ior`, retrying while
/// the naming server is still getting to its first `accept`.
///
/// A completed `connect` does not mean the server can accept yet — the TCP
/// handshake finishes from the listen backlog — so the first attempt can fail
/// or time out for a reason that is gone a few milliseconds later. The wait is
/// **sleeping and deadline-bounded**: a spin loop finishes in microseconds and
/// does not wait at all, which is the assumption A failure this project has
/// already paid for once.
///
/// Every attempt is reported, so a fixture that is retrying looks different in
/// the log from a fixture that is wedged.
fn publish_with_retry(
    ns_ior: &orbweaver_giop::Ior,
    channel: &EventChannelServer,
) -> orbweaver_giop::Result<Vec<orbweaver_giop::event_server::PublishedChannel>> {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let tried = (|| {
            let mut ctx = NamingContext::connect(ns_ior, T)?;
            publish_channels(channel, &mut ctx)
        })();
        match tried {
            Ok(published) => {
                if attempt > 1 {
                    println!("  ..   published on attempt {attempt}");
                }
                return Ok(published);
            }
            Err(e) if std::time::Instant::now() < deadline => {
                println!("  ..   attempt {attempt} to publish failed ({e}); retrying");
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return Err(e),
        }
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            println!("\nchannel-by-name: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> orbweaver_giop::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hold = args.iter().any(|a| a == "--hold");
    let channel_name = match args.iter().position(|a| a == "--channel") {
        Some(i) => args.get(i + 1).cloned().unwrap_or_else(|| "alerts".into()),
        None => "alerts".into(),
    };
    // The positional argument, skipping flags and the word that follows
    // `--channel`. Same shape `spike-events` uses, for the same reason.
    let out_path = args
        .iter()
        .enumerate()
        .find(|(i, a)| !(a.starts_with("--") || (*i > 0 && args[i - 1] == "--channel")))
        .map(|(_, a)| a.clone())
        .unwrap_or_else(|| "spikes/channel-names.ior".into());

    // ── the naming server ──
    let ns_server = Orb::new().server("127.0.0.1:0", b"NameService".to_vec())?;
    let ns_port = ns_server.local_addr()?.port();
    let ns = Arc::new(NamingServer::new("127.0.0.1", ns_port, b"NameService".to_vec()));
    let ns_ior = ns.root_ior();
    let stop = Arc::new(AtomicBool::new(false));
    {
        let flag = stop.clone();
        let serving = Arc::clone(&ns);
        std::thread::spawn(move || {
            let _ = ns_server.serve_shared(&*serving, move || flag.load(Ordering::SeqCst));
        });
    }

    // ── the event channel, on its own ephemeral port ──
    let key = channel_name.as_bytes().to_vec();
    let ch_server = Orb::new().server("127.0.0.1:0", key.clone())?;
    let ch_port = ch_server.local_addr()?.port();
    let channel = Arc::new(EventChannelServer::new("127.0.0.1", ch_port, key));
    let handle = channel.handle();
    let _delivery = channel.start_delivery_with(Duration::from_millis(500));
    {
        let flag = stop.clone();
        let serving = Arc::clone(&channel);
        std::thread::spawn(move || {
            let _ = ch_server.serve_shared(&*serving, move || flag.load(Ordering::SeqCst));
        });
    }

    // Said *before* the first outbound call, so a fixture that wedges below is
    // diagnosable. The first version of this printed nothing until after
    // publication, and when publication hung the log was empty and the runner
    // could only report "never wrote its IOR" — a fixture that cannot say how
    // far it got costs the diagnosis it was built to give.
    println!("naming  listening on 127.0.0.1:{ns_port}");
    println!("channel listening on 127.0.0.1:{ch_port}");

    // ── publication: the deployer's call, holding both servants ──
    //
    // This process is the deployer. It is the shape `publish_channels`'
    // documentation argues for and the reason that function is not a method:
    // binding is an outbound call, and the code making it here is wiring, not
    // a servant answering a request.
    //
    // **Retried, on a sleeping deadline.** This dials a server this same
    // process started microseconds ago, and *a completed connect does not mean
    // the server can accept yet* — on macOS loopback a single `accept()` misses
    // a fresh connection often enough to matter. The TCP handshake completes
    // from the listen backlog, so `connect` returns happily and the `invoke`
    // then blocks for a reply nobody is going to read the request for. Measured
    // here: two runs in five hung with an empty log and no timeout to end them.
    // *연결 성공은 수락 준비를 뜻하지 않는다 — 잠자는 데드라인으로 재시도한다.*
    let published = publish_with_retry(&ns_ior, &channel)?;

    std::fs::write(&out_path, ns_ior.to_stringified()?)?;
    println!("naming IOR written to {out_path}");
    for p in &published {
        println!("published channel {:?} as {}", p.channel, stringify_name(&p.name));
    }
    println!("resolve {}", stringify_name(&channel_binding_name(&channel_name)));
    println!("READY");

    // A self-check, so a failing fixture says so before a peer blames itself:
    // the name resolves, and to the channel's own reference.
    let mut ctx = NamingContext::connect(&ns_ior, T)?;
    let found = ctx.resolve(&channel_binding_name(&channel_name))?;
    drop(ctx);
    if found != published[0].ior {
        println!("channel-by-name: FAIL — the name resolved to something else");
        return Ok(());
    }
    println!("  ok   the name resolves to the channel");

    // The claim this fixture exists to make: the peer is handed this file and
    // nothing else, so the channel's endpoint must not be reachable from it.
    //
    // **Decoded, never grepped.** The first version of this check lived in the
    // shell and was `grep -q "$ch_port" "$NS_IOR"` — a search for a decimal
    // port in a file that is `IOR:` followed by hex, where the port is two CDR
    // bytes. It could not match, so it could not go red, and it reported `ok`
    // over a naming IOR that contained the very port it was handed. Its
    // negative control found that in one run; nothing else would have.
    // *십진수 포트를 16진 IOR에서 grep 하던 검사는 붉어질 수 없었다 — 부정
    // 대조군이 한 번에 잡아냈다.*
    let written = std::fs::read_to_string(&out_path)?;
    let round_tripped = Orb::new()
        .string_to_object(written.trim())
        .map_err(|_| orbweaver_giop::Error::BadIor("the fixture wrote an IOR it cannot read"))?;
    let advertised: Vec<u16> = round_tripped.profiles.iter().map(|p| p.port).collect();
    if advertised.contains(&ch_port) {
        println!(
            "channel-by-name: FAIL — the file the peer is given advertises the channel's \
             port {ch_port}: {advertised:?}"
        );
        return Ok(());
    }
    println!(
        "  ok   the peer's only file advertises {advertised:?} and not the channel's {ch_port}"
    );

    if hold {
        println!(
            "HOLDING — a ulong event is pushed once a second. Point a CosEventComm \
             consumer at the channel by resolving {} out of {out_path} — the peer is \
             never given the channel's address",
            stringify_name(&channel_binding_name(&channel_name))
        );
        let mut tick: u32 = 0;
        loop {
            tick += 1;
            if let Err(e) = handle.publish(&TypeCode::ULong, Endian::native(), |e| e.put_u32(tick))
            {
                eprintln!("spike-channel-by-name: publish failed: {e}");
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    println!("\nchannel-by-name: PASS");
    stop.store(true, Ordering::SeqCst);
    Ok(())
}
