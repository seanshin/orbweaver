//! Self-consistency proof for the first-party CosEvent push channel: OUR
//! supplier and OUR consumer through OUR channel, one process, loopback —
//! then a deliberately dead consumer that must be cut loose without the live
//! one missing an event, with every drop counted and printed.
//!
//! Self-consistency only: it proves the halves agree, not that either matches
//! the specification. The independent check is omniORB's python consumer
//! attaching to this channel (below).
//!
//! Usage: `spike-events [ior-out-path] [--hold]`
//!
//! Default IOR path is `spikes/events.ior`. With `--hold` the server keeps
//! running after the checks so an external client can attach; the process is
//! stopped by killing it — there is no remote shutdown, and `destroy` is
//! refused by design.
//!
//! # Cross-ORB oracle (integration's job, not run here)
//!
//! No omniEvents fixture exists (`brew info omnievents`: "No available
//! formula"), but omniORBpy ships the `CosEventComm` stubs, so the
//! independent peer is an omniORB *consumer* connecting to our channel and
//! receiving what our supplier pushes. Start
//! `spike-events spikes/events.ior --hold`, then:
//!
//! ```text
//! python3 - <<'EOF'
//! import sys, time
//! from omniORB import CORBA
//! import CosEventComm, CosEventComm__POA, CosEventChannelAdmin
//!
//! class Consumer(CosEventComm__POA.PushConsumer):
//!     def __init__(self): self.got = []
//!     def push(self, data): self.got.append(data.value(CORBA.TC_ulong))
//!     def disconnect_push_consumer(self): pass
//!
//! orb = CORBA.ORB_init(sys.argv)
//! poa = orb.resolve_initial_references("RootPOA")
//! poa._get_the_POAManager().activate()
//! servant = Consumer()
//!
//! channel = orb.string_to_object(open('spikes/events.ior').read().strip())
//! channel = channel._narrow(CosEventChannelAdmin.EventChannel)
//! proxy = channel.for_consumers().obtain_push_supplier()
//! proxy.connect_push_consumer(servant._this())
//! time.sleep(2)  # --hold pushes a ulong once a second after HOLDING
//! print("received:", servant.got)
//! assert len(servant.got) >= 1, "no event arrived from the held channel"
//! print("PASS")
//! EOF
//! ```
//!
//! PASS is at least one received ulong. The python client no longer has to
//! be the only client: `Server` serves its connections concurrently, so the
//! `for_consumers()`/`obtain_push_supplier()` chain above may ride one
//! connection or several, and other clients may hold sessions alongside it —
//! `spike-concurrent` measures that overlap, and
//! `concurrent_suppliers_and_outbound_delivery_do_not_deadlock` proves it
//! against this channel's outbound pushes. Since stream E's second batch the
//! channel is a `SharedDispatch` servant, so two operations run at once and a
//! slow consumer is bounded by the delivery thread's push timeout alone —
//! `an_inbound_push_is_served_while_an_outbound_push_is_blocked` is the test
//! that holds an outbound push open on purpose and serves inbound work
//! through it.
//!
//! Measured at landing (2026-08-13, omniORBpy 4.3.4): the snippet
//! printed `received: [57, 58]` then `PASS` — an ORB we did not write
//! narrowed our channel, connected its own servant, and decoded two events
//! our channel pushed to it.

use orbweaver_cdr::Endian;
use orbweaver_giop::event_server::{
    ChannelHandle, EventChannelServer, MAX_CONSECUTIVE_FAILURES, PUSH_CONSUMER_ID,
    PushConsumerServant, client,
};
use orbweaver_giop::server::Server;
use orbweaver_giop::typecode::TypeCode;
use orbweaver_giop::{Connection, IiopProfile, Ior, Version};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const T: Duration = Duration::from_secs(5);

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hold = args.iter().any(|a| a == "--hold");
    let out_path = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "spikes/events.ior".into());

    match run(&out_path, hold) {
        Ok(()) => {
            println!("\nevent-channel: PASS");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            println!("\nevent-channel: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn ok(what: &str) {
    println!("  ok   {what}");
}

fn require(cond: bool, what: &str) -> Result<(), Box<dyn std::error::Error>> {
    if cond { Ok(()) } else { Err(what.into()) }
}

/// A consumer servant of our own on its own loopback server, collecting into
/// an [`orbweaver_giop::event_server::EventSink`].
struct Consumer {
    ior: Ior,
    sink: orbweaver_giop::event_server::EventSink,
}

fn start_consumer(key: &[u8]) -> Consumer {
    let server = Server::bind("127.0.0.1:0", key.to_vec()).expect("bind consumer");
    let port = server.local_addr().expect("addr").port();
    let servant = PushConsumerServant::new(key.to_vec());
    let ior = servant.ior("127.0.0.1", port);
    let sink = servant.sink();
    std::thread::spawn(move || {
        let _ = server.serve_shared(&servant, || false);
    });
    Consumer { ior, sink }
}

/// A reference to a port that was bound and then released: dialling it is
/// refused immediately — a dead consumer, without a connect-timeout wait.
fn dead_consumer_ior() -> Ior {
    let l = TcpListener::bind("127.0.0.1:0").expect("probe port");
    let port = l.local_addr().expect("addr").port();
    drop(l);
    Ior {
        type_id: PUSH_CONSUMER_ID.into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "127.0.0.1".into(),
            port,
            object_key: b"DeadConsumer".to_vec(),
            components: Vec::new(),
        }],
    }
}

/// Attaches `consumer` to a fresh ProxyPushSupplier. One connection at a
/// time, so each hop dials and hangs up.
fn attach(channel: &Ior, consumer: &Ior) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = Connection::connect(channel, T)?;
    let admin = client::for_consumers(&mut conn)?;
    drop(conn);
    let mut conn = Connection::connect(&admin, T)?;
    let proxy = client::obtain_push_supplier(&mut conn)?;
    drop(conn);
    let mut conn = Connection::connect(&proxy, T)?;
    client::connect_push_consumer(&mut conn, consumer)?;
    Ok(())
}

fn run(out_path: &str, hold: bool) -> Result<(), Box<dyn std::error::Error>> {
    let server = Server::bind("127.0.0.1:0", b"EventChannel".to_vec())?;
    let port = server.local_addr()?.port();
    let channel = EventChannelServer::new("127.0.0.1", port, b"EventChannel".to_vec());
    let channel_ior = channel.channel_ior();
    let handle = channel.handle();
    // A short push timeout so the dead-consumer phase is measured in
    // milliseconds, not connect-timeout multiples.
    let _delivery = channel.start_delivery_with(Duration::from_millis(500));
    std::fs::write(out_path, channel_ior.to_stringified()?)?;
    println!("listening on 127.0.0.1:{port}");
    println!("IOR written to {out_path}");
    println!("READY");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn({
        let servant = channel;
        move || {
            let _ = server.serve_shared(&servant, || flag.load(Ordering::SeqCst));
        }
    });

    // ── phase 1: N events through a supplier proxy, in order ──
    let live = start_consumer(b"LiveConsumer");
    attach(&channel_ior, &live.ior)?;
    ok("consumer connected via for_consumers/obtain_push_supplier/connect_push_consumer");

    let mut conn = Connection::connect(&channel_ior, T)?;
    let s_admin = client::for_suppliers(&mut conn)?;
    drop(conn);
    let mut conn = Connection::connect(&s_admin, T)?;
    let s_proxy = client::obtain_push_consumer(&mut conn)?;
    drop(conn);
    let mut supplier = Connection::connect(&s_proxy, T)?;
    client::connect_push_supplier(&mut supplier, &client::nil_ref())?;
    ok("supplier connected via for_suppliers/obtain_push_consumer/connect_push_supplier");

    const N: u32 = 20;
    for i in 0..N {
        client::push(&mut supplier, &TypeCode::ULong, move |e| e.put_u32(i))?;
    }
    require(
        handle.wait_until(T, |s| s.delivered == u64::from(N)),
        "phase 1: not every event was delivered",
    )?;
    let got: Vec<u32> = live
        .sink
        .snapshot()
        .iter()
        .map(|a| a.value_decoder().get_u32().unwrap_or(u32::MAX))
        .collect();
    require(
        got == (0..N).collect::<Vec<u32>>(),
        "phase 1: events arrived out of order or corrupted",
    )?;
    ok("push(any) x20 through the supplier proxy arrived, all 20, in order");

    // ── phase 2: a dead consumer is cut; the live one never misses ──
    // One connection is served at a time: hang up the supplier before the
    // attach dials the channel, then re-dial the same proxy — its connected
    // state lives on the object key, not on the connection.
    drop(supplier);
    attach(&channel_ior, &dead_consumer_ior())?;
    ok("second consumer connected, then its socket closed (dead)");
    let mut supplier = Connection::connect(&s_proxy, T)?;

    const M: u32 = 6;
    for i in 0..M {
        client::push(&mut supplier, &TypeCode::ULong, move |e| e.put_u32(1000 + i))?;
    }
    require(
        handle.wait_until(T, |s| s.disconnected_for_failure == 1),
        "phase 2: the dead consumer was never disconnected",
    )?;
    require(
        handle.wait_until(T, |s| s.delivered == u64::from(N + M)),
        "phase 2: the live consumer missed events while the dead one failed",
    )?;
    let tail: Vec<u32> = live
        .sink
        .snapshot()
        .iter()
        .skip(N as usize)
        .map(|a| a.value_decoder().get_u32().unwrap_or(u32::MAX))
        .collect();
    require(
        tail == (1000..1000 + M).collect::<Vec<u32>>(),
        "phase 2: the live consumer's events arrived out of order",
    )?;
    ok("dead consumer disconnected after the failure threshold; live one got all 6, in order");

    let stats = handle.stats();
    println!(
        "  drop report: accepted={} delivered={} dropped={} push_failures={} \
         disconnected_for_failure={} unrelayable={}",
        stats.accepted,
        stats.delivered,
        stats.dropped,
        stats.push_failures,
        stats.disconnected_for_failure,
        stats.unrelayable
    );
    require(
        stats.push_failures == u64::from(MAX_CONSECUTIVE_FAILURES),
        "exactly the threshold's worth of failures should have been spent on the dead consumer",
    )?;
    require(stats.dropped > 0, "the dead consumer's backlog must be counted as dropped, loudly")?;
    ok("drops were counted and reported, none silent");

    drop(supplier);

    if hold {
        println!(
            "HOLDING — channel stays up; a ulong event is pushed once a second. \
             Point a CosEventComm consumer at {out_path}"
        );
        hold_and_tick(&handle);
    }
    Ok(())
}

/// Publishes a ulong once a second forever, so an attached external consumer
/// has something to receive. In-process publishing: the supplier side needs
/// no socket, and the single served connection stays free for the guest.
fn hold_and_tick(handle: &ChannelHandle) -> ! {
    let mut i: u32 = 0;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let n = i;
        if let Err(e) = handle.publish(&TypeCode::ULong, Endian::Big, move |e2| e2.put_u32(n)) {
            eprintln!("spike-events: publish failed: {e}");
        }
        i = i.wrapping_add(1);
    }
}
