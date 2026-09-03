//! Self-consistency proof for the first-party CosEvent channel: OUR supplier
//! and OUR consumer through OUR channel, one process, loopback — then a
//! deliberately dead consumer that must be cut loose without the live one
//! missing an event, with every drop counted and printed, and finally OUR
//! `PullSupplier` that the channel has to go and **ask**.
//!
//! Self-consistency only: it proves the halves agree, not that either matches
//! the specification. The independent checks are omniORB's python consumer
//! attaching to this channel and omniORB's python *supplier* being pulled from
//! by it (below).
//!
//! Usage: `spike-events [ior-out-path] [--hold] [--source-endian big|little]`
//!
//! Default IOR path is `spikes/events.ior`. With `--hold` the server keeps
//! running after the checks so an external client can attach; the process is
//! stopped by killing it — there is no remote shutdown, and `destroy` is
//! refused by design. `--source-endian` sets the byte order the channel asks
//! its suppliers in, which is in practice the order pulled events are captured
//! in, so the cross-ORB pull check can be run in both — an encoder that only
//! works native-endian passes every local test and fails in the field.
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
//!
//! # The other cross-ORB direction: omniORB as a supplier we pull from
//!
//! `spikes/event_pull_supplier.py` is the mirror of that check and the peer
//! oracle for the supplier side of pull: an omniORB `CosEventComm::PullSupplier`
//! servant, handed to `for_suppliers().obtain_pull_consumer()` and connected
//! with `connect_pull_supplier`, which **our** channel then fetches from with
//! `try_pull` and fans out to a consumer of the peer's own. Run it against
//! `--hold` in both byte orders:
//!
//! ```text
//! cargo run -q --bin spike-events -- /tmp/ev.ior --hold --source-endian big &
//! python3 spikes/event_pull_supplier.py /tmp/ev.ior
//! ```

use orbweaver_cdr::Endian;
use orbweaver_giop::event_server::{
    ChannelHandle, EventChannelServer, EventSource, MAX_CONSECUTIVE_FAILURES, PUSH_CONSUMER_ID,
    PullSupplierServant, PushConsumerServant, client,
};
use orbweaver_giop::orb::Orb;
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
    // The byte order the channel asks its suppliers in. A supplier replies in
    // the order it was asked in, so this is the order pulled events are
    // captured in, and the cross-ORB pull check is meant to be run in both.
    let source_endian = match args.iter().position(|a| a == "--source-endian") {
        Some(i) => match args.get(i + 1).map(String::as_str) {
            Some("big") => Endian::Big,
            Some("little") => Endian::Little,
            other => {
                println!(
                    "event-channel: FAIL — --source-endian wants big or little, got {other:?}"
                );
                return std::process::ExitCode::FAILURE;
            }
        },
        None => Endian::native(),
    };
    let out_path = args
        .iter()
        .enumerate()
        .find(|(i, a)| !(a.starts_with("--") || *i > 0 && args[i - 1] == "--source-endian"))
        .map(|(_, a)| a.clone())
        .unwrap_or_else(|| "spikes/events.ior".into());

    match run(&out_path, hold, source_endian) {
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
///
/// Dropping it stops its server: the fixture's own teardown route for these
/// helpers is drop (the pull phase already ends with one), so the serve loop
/// is wired to it — the server's own flag (D034), raised and then joined.
struct Consumer {
    ior: Ior,
    sink: orbweaver_giop::event_server::EventSink,
    stop: orbweaver_giop::server::StopFlag,
    serving: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Consumer {
    fn drop(&mut self) {
        self.stop.raise();
        if let Some(serving) = self.serving.take() {
            let _ = serving.join();
        }
    }
}

fn start_consumer(key: &[u8]) -> Consumer {
    let server = Orb::new().server("127.0.0.1:0", key.to_vec()).expect("bind consumer");
    let port = server.local_addr().expect("addr").port();
    let servant = PushConsumerServant::new(key.to_vec());
    let ior = servant.ior("127.0.0.1", port);
    let sink = servant.sink();
    let stop = server.stop_flag();
    let watch = stop.clone();
    let serving = std::thread::spawn(move || {
        let _ = server.serve_shared(&servant, move || watch.raised());
    });
    Consumer { ior, sink, stop, serving: Some(serving) }
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

/// A `CosEventComm::PullSupplier` of our own on its own loopback server — the
/// thing the channel has to go and ask, rather than one that calls.
///
/// Dropping it stops its server, exactly as [`Consumer`]'s drop does.
struct Supplier {
    ior: Ior,
    source: EventSource,
    stop: orbweaver_giop::server::StopFlag,
    serving: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Supplier {
    fn drop(&mut self) {
        self.stop.raise();
        if let Some(serving) = self.serving.take() {
            let _ = serving.join();
        }
    }
}

fn start_supplier(key: &[u8]) -> Supplier {
    let server = Orb::new().server("127.0.0.1:0", key.to_vec()).expect("bind supplier");
    let port = server.local_addr().expect("addr").port();
    let servant = PullSupplierServant::new(key.to_vec());
    let ior = servant.ior("127.0.0.1", port);
    let source = servant.source();
    let stop = server.stop_flag();
    let watch = stop.clone();
    let serving = std::thread::spawn(move || {
        let _ = server.serve_shared(&servant, move || watch.raised());
    });
    Supplier { ior, source, stop, serving: Some(serving) }
}

/// Attaches `supplier` to a fresh ProxyPullConsumer: the channel will now come
/// and ask it for events.
fn attach_supplier(channel: &Ior, supplier: &Ior) -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = Connection::connect(channel, T)?;
    let admin = client::for_suppliers(&mut conn)?;
    drop(conn);
    let mut conn = Connection::connect(&admin, T)?;
    let proxy = client::obtain_pull_consumer(&mut conn)?;
    drop(conn);
    let mut conn = Connection::connect(&proxy, T)?;
    client::connect_pull_supplier(&mut conn, supplier)?;
    Ok(())
}

fn run(
    out_path: &str,
    hold: bool,
    source_endian: Endian,
) -> Result<(), Box<dyn std::error::Error>> {
    let server = Orb::new().server("127.0.0.1:0", b"EventChannel".to_vec())?;
    let port = server.local_addr()?.port();
    let channel = EventChannelServer::new("127.0.0.1", port, b"EventChannel".to_vec());
    let channel_ior = channel.channel_ior();
    let handle = channel.handle();
    // A short push timeout so the dead-consumer phase is measured in
    // milliseconds, not connect-timeout multiples.
    let _delivery = channel.start_delivery_with(Duration::from_millis(500));
    handle.set_source_endian(source_endian);
    std::fs::write(out_path, channel_ior.to_stringified()?)?;
    println!("listening on 127.0.0.1:{port} (asking suppliers {source_endian:?}-endian)");
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

    // **What is asserted here is that nothing vanished unattributed — not that
    // a backlog existed.**
    //
    // This read `wait_until(T, |s| s.dropped_on_failure_disconnect > 0)`, and
    // the deadline was added on 2026-08-19 after it failed once on a loaded CI
    // runner. The deadline was the right repair for the reason given then and
    // the wrong assertion to keep: it waits for a counter to become positive,
    // which requires the dead consumer to have HAD a backlog, and whether it
    // did is a race. On Linux the failing pushes come back `Connection refused`
    // immediately, so the three-failure threshold is reached before anything
    // queues and the honest answer is `0 queued event(s) dropped`. Measured
    // 2026-08-30, CI run for 6ca51ac: `fanned_out=32 dropped=3` where the run
    // before it had `31` and `2`, over a commit that touched two documents and
    // a markdown parser. A test that turns *there was nothing to count* into a
    // failure is asserting the timing, not the property.
    //
    // The property is that every discarded event names a cause, and
    // `split_adds_up` is that invariant with nothing to race against: it
    // compares the total against the sum of the causes. The *loudness* half —
    // that the drops are attributed to the cut consumer and to nothing else —
    // is already asserted, in phase 1, by a leg that passed on both runs; this
    // one was restating it through a counter that happened to be non-zero.
    //
    // *여기서 단언하는 것은 **원인 없이 사라진 것이 없다**이지 백로그가 있었다가
    // 아니다. 마감 시한은 옳은 수리였고 유지할 단언은 아니었다 — 리눅스에서는
    // 큐가 쌓이기 전에 임계값에 닿으므로 정직한 답이 0이다. **셀 것이 없었다**를
    // 실패로 바꾸는 테스트는 성질이 아니라 타이밍을 단언한다.*
    require(
        handle.wait_until(T, |s| s.split_adds_up()),
        "every discarded event must name a cause: the total and the per-cause \
         counts disagree, so something was thrown away without saying why",
    )?;
    let stats = handle.stats();
    println!(
        "  drop report: accepted={} fanned_out={} delivered={} dropped={} \
         (overflow={} unrelayable={} on_disconnect={} on_failure_disconnect={} at_stop={}) \
         push_failures={} disconnected_for_failure={}",
        stats.accepted,
        stats.fanned_out,
        stats.delivered,
        stats.dropped,
        stats.dropped_overflow,
        stats.unrelayable,
        stats.dropped_on_disconnect,
        stats.dropped_on_failure_disconnect,
        stats.dropped_at_stop,
        stats.push_failures,
        stats.disconnected_for_failure
    );
    require(
        stats.push_failures == u64::from(MAX_CONSECUTIVE_FAILURES),
        "exactly the threshold's worth of failures should have been spent on the dead consumer",
    )?;
    require(stats.split_adds_up(), "the per-cause drop counters must account for every drop")?;
    // The whole point of the split, asserted over real sockets rather than in
    // a unit test's memory: this phase disconnected a dead consumer and did
    // nothing else, so every drop must carry that cause and no other. Before
    // the split all five causes were one number and this could not be said.
    require(
        stats.dropped == stats.dropped_on_failure_disconnect,
        "every drop here is the cut consumer's backlog — no other cause may be mixed in",
    )?;
    require(
        stats.dropped_overflow == 0,
        "nothing overflowed: 26 events, a bound of 64, so a drop counted as back-pressure \
         would be a miscount",
    )?;
    // Fan-out, measured rather than assumed: the second consumer was attached
    // for phase 2, so more queue slots were filled than events were accepted.
    // Not an equality — the dead proxy is cut partway through phase 2, so how
    // many of the six events it was still connected for is a race, and an
    // exact number here would be a flaky assertion about scheduling.
    require(
        stats.fanned_out > stats.accepted,
        "fan-out must have made more queue entries than events accepted",
    )?;
    ok("drops were counted by cause and reported, none silent, none miscounted");

    // ── phase 3: the fourth side of the 2×2 — a supplier the channel asks ──
    // The consumer here is the same live one: an event fetched from a
    // `PullSupplier` must be indistinguishable, downstream, from one that was
    // pushed in. That sameness is the design, so it is what is measured.
    let pull_supplier = start_supplier(b"PullSupplier");
    const K: u32 = 5;
    for i in 0..K {
        pull_supplier
            .source
            .offer(&TypeCode::ULong, source_endian, move |e| e.put_u32(2000 + i))?;
    }
    attach_supplier(&channel_ior, &pull_supplier.ior)?;
    require(
        handle.wait_until(T, |s| s.sourced == u64::from(K)),
        "phase 3: the channel did not fetch every offered event",
    )?;
    require(
        handle.wait_until(T, |s| s.delivered == u64::from(N + M + K)),
        "phase 3: fetched events did not reach the consumer",
    )?;
    let pulled: Vec<u32> = live
        .sink
        .snapshot()
        .iter()
        .skip((N + M) as usize)
        .map(|a| a.value_decoder().get_u32().unwrap_or(u32::MAX))
        .collect();
    require(
        pulled == (2000..2000 + K).collect::<Vec<u32>>(),
        "phase 3: the fetched events arrived out of order or corrupted",
    )?;
    // The design decision, as a number rather than a paragraph: `pull` blocks
    // and `try_pull` does not, and a shared round may not be held by one
    // silent supplier.
    require(
        pull_supplier.source.pull_calls() == 0,
        "phase 3: the channel must never call the blocking pull",
    )?;
    require(
        pull_supplier.source.try_pull_calls() >= u64::from(K),
        "phase 3: the channel must have asked with try_pull at least once per event",
    )?;
    let stats = handle.stats();
    require(
        stats.pull_failures == 0 && stats.pull_suppliers_connected == 1,
        "phase 3: the supplier should still be connected with no failures",
    )?;
    require(
        stats.split_adds_up(),
        "phase 3: the per-cause drop counters must still account for every drop",
    )?;
    println!(
        "  pull report: sourced={} try_pull={} pull={} pull_failures={} suppliers_connected={}",
        stats.sourced,
        pull_supplier.source.try_pull_calls(),
        pull_supplier.source.pull_calls(),
        stats.pull_failures,
        stats.pull_suppliers_connected
    );
    ok("channel pulled 5 events out of a PullSupplier with try_pull, in order, none blocking");

    drop(supplier);

    if hold {
        println!(
            "HOLDING — channel stays up; a ulong event is pushed once a second. \
             Point a CosEventComm consumer at {out_path}, or a CosEventComm \
             PullSupplier at its SupplierAdmin — the channel asks \
             {source_endian:?}-endian and will come and fetch"
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
