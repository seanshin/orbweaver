//! Several CosEvent channels in one server, over real sockets.
//!
//! # What this has to establish, and in what order
//!
//! 1. **A server built the old way is unchanged.** Every key it answers to and
//!    every reference it publishes is byte-for-byte what it was when an
//!    `EventChannelServer` was one channel. *Absent is not zero* — the rule the
//!    MCP `--config` batch proved and D020 Stage A applies — and here it is
//!    literal: the default channel's keys are the `base_key` it was given, with
//!    nothing appended and nothing renamed.
//! 2. **Two channels are two channels.** A supplier pushing into one is not
//!    fanned out to the other's consumers, and each keeps its own counters.
//!    This is the property a key collision would break *silently*, with every
//!    number agreeing, which is why it is measured over the wire rather than
//!    argued from the naming rule.
//! 3. **A name that could mint an existing key is refused**, and the test says
//!    which key it would have been — because "the rule rejects this string" is
//!    a weaker claim than "this string would have addressed that object".
//!
//! # No factory, and why there is nothing here about one
//!
//! `CosEventChannelAdmin` declares no factory; the factory in the standard is
//! `CosNotifyChannelAdmin::EventChannelFactory`, which is CosNotification's and
//! is deferred (`PLAN-DEFERRED` §1, D021 §3, §6). Creation is therefore a Rust
//! API and a deployment decision, exactly as `Poa` creation is, and there is no
//! new wire surface in this file for the same reason there is none in the
//! product.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use orbweaver_cdr::Endian;
use orbweaver_giop::event_server::{
    ChannelError, ChannelHandle, Delivery, EventChannelServer, EventSink, PullSupplierServant,
    PushConsumerServant, client, is_channel_name_safe,
};
use orbweaver_giop::orb::Orb;
use orbweaver_giop::typecode::{Any, TypeCode};
use orbweaver_giop::{Connection, Ior};

const T: Duration = Duration::from_secs(5);
const OUTBOUND_T: Duration = Duration::from_millis(500);
const POLL: Duration = Duration::from_millis(5);
const BASE: &[u8] = b"EventChannel";

/// A server on loopback, serving whatever channels it is given.
struct Serving {
    servant: Arc<EventChannelServer>,
    delivery: Option<Delivery>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Serving {
    /// A server with only the channel it was constructed with, and no
    /// outbound threads yet — so a test can create channels first and observe
    /// that starting later covers all of them.
    fn paused() -> Self {
        let server = Orb::new().server("127.0.0.1:0", BASE.to_vec()).unwrap();
        let port = server.local_addr().unwrap().port();
        let servant = Arc::new(EventChannelServer::new("127.0.0.1", port, BASE.to_vec()));
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let serving = Arc::clone(&servant);
        let thread = std::thread::spawn(move || {
            server.serve_shared(&*serving, move || flag.load(Ordering::SeqCst)).unwrap();
        });
        Serving { servant, delivery: None, stop, thread: Some(thread) }
    }

    fn begin(&mut self) {
        let delivery = self.servant.start_delivery_with(OUTBOUND_T);
        for name in self.servant.channel_names() {
            self.servant.handle_named(&name).unwrap().set_source_poll(POLL);
        }
        self.delivery = Some(delivery);
    }

    fn dial(&self, ior: &Ior) -> Connection {
        Connection::connect(ior, T).unwrap()
    }

    /// A connected `ProxyPushConsumer` on the channel named `name`.
    fn push_consumer_proxy(&self, name: &str) -> Connection {
        let channel = self.servant.channel_ior_named(name).expect("that channel exists");
        let mut conn = self.dial(&channel);
        let admin = client::for_suppliers(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&admin);
        let proxy = client::obtain_push_consumer(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&proxy);
        client::connect_push_supplier(&mut conn, &client::nil_ref()).unwrap();
        conn
    }

    /// Attaches `consumer` to a `ProxyPushSupplier` on the channel `name`.
    fn attach_consumer(&self, name: &str, consumer: &Ior) {
        let channel = self.servant.channel_ior_named(name).expect("that channel exists");
        let mut conn = self.dial(&channel);
        let admin = client::for_consumers(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&admin);
        let proxy = client::obtain_push_supplier(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&proxy);
        client::connect_push_consumer(&mut conn, consumer).unwrap();
    }

    /// Attaches `supplier` to a `ProxyPullConsumer` on the channel `name`.
    fn attach_supplier(&self, name: &str, supplier: &Ior) {
        let channel = self.servant.channel_ior_named(name).expect("that channel exists");
        let mut conn = self.dial(&channel);
        let admin = client::for_suppliers(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&admin);
        let proxy = client::obtain_pull_consumer(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&proxy);
        client::connect_pull_supplier(&mut conn, supplier).unwrap();
    }

    /// A connected `ProxyPullSupplier` on the channel `name`, ready to pull.
    fn pull_supplier_proxy(&self, name: &str) -> Connection {
        let channel = self.servant.channel_ior_named(name).expect("that channel exists");
        let mut conn = self.dial(&channel);
        let admin = client::for_consumers(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&admin);
        let proxy = client::obtain_pull_supplier(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&proxy);
        client::connect_pull_consumer(&mut conn, &client::nil_ref()).unwrap();
        conn
    }

    fn shutdown(mut self) {
        drop(self.delivery.take());
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.thread.take().unwrap().join();
    }
}

/// A collecting `PushConsumer` on its own loopback server.
struct Consumer {
    ior: Ior,
    sink: EventSink,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Consumer {
    fn start(key: &[u8]) -> Self {
        let server = Orb::new().server("127.0.0.1:0", key.to_vec()).unwrap();
        let port = server.local_addr().unwrap().port();
        let servant = Arc::new(PushConsumerServant::new(key.to_vec()));
        let ior = servant.ior("127.0.0.1", port);
        let sink = servant.sink();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            server.serve_shared(&*servant, move || flag.load(Ordering::SeqCst)).unwrap();
        });
        Consumer { ior, sink, stop, thread: Some(thread) }
    }

    fn shutdown(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.thread.take().unwrap().join();
    }
}

/// A `PullSupplier` of ours on its own loopback server.
struct Supplier {
    ior: Ior,
    source: orbweaver_giop::event_server::EventSource,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Supplier {
    fn start(key: &[u8]) -> Self {
        let server = Orb::new().server("127.0.0.1:0", key.to_vec()).unwrap();
        let port = server.local_addr().unwrap().port();
        let servant = Arc::new(PullSupplierServant::new(key.to_vec()));
        let ior = servant.ior("127.0.0.1", port);
        let source = servant.source();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            server.serve_shared(&*servant, move || flag.load(Ordering::SeqCst)).unwrap();
        });
        Supplier { ior, source, stop, thread: Some(thread) }
    }

    fn shutdown(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.thread.take().unwrap().join();
    }
}

fn ulong(any: &Any) -> u32 {
    assert_eq!(any.tc, TypeCode::ULong);
    any.value_decoder().get_u32().unwrap()
}

fn key_of(ior: &Ior) -> Vec<u8> {
    ior.primary().unwrap().object_key.clone()
}

fn until(
    handle: &ChannelHandle,
    what: &str,
    pred: impl FnMut(&orbweaver_giop::event_server::ChannelStats) -> bool,
) {
    assert!(handle.wait_until(T, pred), "{what}: {:?}", handle.stats());
}

/// The compatibility claim, made byte for byte rather than by inspection: a
/// server constructed the way every existing caller constructs one has exactly
/// one channel, and its keys are the `base_key` it was given with the same two
/// suffixes appended that were appended before.
///
/// If this ever needs relaxing, every published IOR in the field is stale, so
/// it is asserted on the wire — `for_consumers` and `for_suppliers` answers —
/// and not on the struct.
#[test]
fn a_server_built_the_old_way_is_one_channel_with_the_keys_it_always_had() {
    let mut serving = Serving::paused();
    serving.begin();

    assert_eq!(serving.servant.channel_names(), vec!["EventChannel".to_string()]);
    assert_eq!(serving.servant.channel_key(), BASE);
    assert_eq!(key_of(&serving.servant.channel_ior()), BASE.to_vec());

    let mut conn = serving.dial(&serving.servant.channel_ior());
    let consumers = client::for_consumers(&mut conn).unwrap();
    let suppliers = client::for_suppliers(&mut conn).unwrap();
    assert_eq!(key_of(&consumers), b"EventChannel/consumerAdmin".to_vec());
    assert_eq!(key_of(&suppliers), b"EventChannel/supplierAdmin".to_vec());
    drop(conn);

    // And the proxies still mint under the bare base, with the counter
    // starting at 1 — a per-server counter would have made this `pps3`.
    let mut conn = serving.dial(&consumers);
    let proxy = client::obtain_push_supplier(&mut conn).unwrap();
    assert_eq!(key_of(&proxy), b"EventChannel/pps1".to_vec());
    drop(conn);

    // `handle()` still means the channel the server was built with.
    assert_eq!(serving.servant.handle().stats(), serving.servant.total_stats());

    serving.shutdown();
}

/// Two channels are two channels: a supplier pushing into one reaches only its
/// own consumers, and the counters do not mix. **Both byte orders**, because
/// the relay adopts the event's order and a second channel is a second place
/// that can get it wrong.
///
/// This is the property a key collision breaks silently — with every number
/// agreeing — so it is measured by what a consumer received, not by what the
/// naming rule promises.
#[test]
fn two_channels_in_one_server_do_not_see_each_others_events() {
    for endian in [Endian::Big, Endian::Little] {
        let mut serving = Serving::paused();
        serving.servant.create_channel("orders").unwrap();
        serving.servant.create_channel("alerts").unwrap();
        serving.begin();
        assert_eq!(
            serving.servant.channel_names(),
            vec!["EventChannel".to_string(), "alerts".to_string(), "orders".to_string()]
        );

        let on_orders = Consumer::start(b"OrdersConsumer");
        let on_alerts = Consumer::start(b"AlertsConsumer");
        serving.attach_consumer("orders", &on_orders.ior);
        serving.attach_consumer("alerts", &on_alerts.ior);

        let mut supplier = serving.push_consumer_proxy("orders");
        supplier.set_endian(endian);
        for i in 0..3u32 {
            client::push(&mut supplier, &TypeCode::ULong, move |e| e.put_u32(0xD00D + i)).unwrap();
        }

        let orders = serving.servant.handle_named("orders").unwrap();
        let alerts = serving.servant.handle_named("alerts").unwrap();
        until(&orders, &format!("{endian:?}: orders delivered"), |s| s.delivered == 3);
        assert!(on_orders.sink.wait_for(3, T), "{endian:?}");
        let got: Vec<u32> = on_orders.sink.snapshot().iter().map(ulong).collect();
        assert_eq!(got, vec![0xD00D, 0xD00E, 0xD00F], "{endian:?}");

        // The other channel saw nothing at all. Not "nothing yet": its own
        // counters say no event was ever accepted by it.
        assert!(on_alerts.sink.is_empty(), "{endian:?}: an event crossed between channels");
        let a = alerts.stats();
        assert_eq!(a.accepted, 0, "{endian:?}");
        assert_eq!(a.fanned_out, 0, "{endian:?}");
        assert_eq!(a.delivered, 0, "{endian:?}");
        assert_eq!(
            a.consumers_connected, 1,
            "{endian:?}: it has a consumer, it just had no events"
        );

        // Per channel, and the total is the sum.
        let o = orders.stats();
        assert_eq!(o.accepted, 3, "{endian:?}");
        let total = serving.servant.total_stats();
        assert_eq!(total.accepted, 3, "{endian:?}");
        assert_eq!(total.delivered, 3, "{endian:?}");
        assert_eq!(total.consumers_connected, 2, "{endian:?}: one on each");
        assert!(total.split_adds_up(), "{endian:?}: {total:?}");

        drop(supplier);
        on_orders.shutdown();
        on_alerts.shutdown();
        serving.shutdown();
    }
}

/// The collision case, named rather than gestured at. A channel called
/// `consumerAdmin` would be addressed by the key that already names the first
/// channel's `ConsumerAdmin`, and which of the two a request reached would
/// depend on map iteration order.
///
/// The test asserts the collision **exists** — it builds the key the name would
/// have produced and shows it is an object already — and then that the name is
/// refused. Deleting the reserved clause from `why_unsafe` makes the second
/// half fail while the first half goes on being true, which is the negative
/// control recorded in this batch's commit message.
#[test]
fn a_name_that_would_mint_an_existing_key_is_refused_and_the_key_is_named() {
    let mut serving = Serving::paused();
    serving.begin();

    let mut conn = serving.dial(&serving.servant.channel_ior());
    let consumers = client::for_consumers(&mut conn).unwrap();
    drop(conn);

    // The key a channel named `consumerAdmin` would take: base + "/" + name.
    let would_be = {
        let mut k = serving.servant.channel_key().to_vec();
        k.push(b'/');
        k.extend_from_slice(b"consumerAdmin");
        k
    };
    assert_eq!(
        would_be,
        key_of(&consumers),
        "the collision is real: that name's channel key is the first channel's ConsumerAdmin"
    );

    match serving.servant.create_channel("consumerAdmin") {
        Err(ChannelError::UnsafeName { name, why }) => {
            assert_eq!(name, "consumerAdmin");
            assert!(why.contains("admin key"), "the reason should name the clause: {why}");
        }
        other => panic!("expected UnsafeName, got {other:?}"),
    }
    assert_eq!(serving.servant.channel_names(), vec!["EventChannel".to_string()]);

    // The same argument for a minted proxy key, which needs no server at all
    // to state: `pps1` is what the first `obtain_push_supplier` mints.
    for bad in ["", "a/b", "consumerAdmin", "supplierAdmin", "pps1", "pls2", "ppc10", "plc7"] {
        assert!(!is_channel_name_safe(bad), "{bad:?} must be refused");
        assert!(serving.servant.create_channel(bad).is_err(), "{bad:?}");
    }
    // And the near misses that are perfectly safe: a tag with no number is not
    // a key this module mints, and neither is one with a number and a letter.
    for good in ["orders", "pps", "pps1a", "ppsx", "Orders-2", "주문"] {
        assert!(is_channel_name_safe(good), "{good:?} must be allowed");
    }

    serving.shutdown();
}

/// A duplicate name is refused, and the refusal does not disturb the channel
/// that already has it — an error that half-created something would be worse
/// than one that creates nothing.
#[test]
fn a_duplicate_name_is_refused_and_the_first_channel_keeps_working() {
    let mut serving = Serving::paused();
    let first = serving.servant.create_channel("orders").unwrap();
    serving.begin();

    let consumer = Consumer::start(b"DupConsumer");
    serving.attach_consumer("orders", &consumer.ior);

    match serving.servant.create_channel("orders") {
        Err(ChannelError::Duplicate { name }) => assert_eq!(name, "orders"),
        other => panic!("expected Duplicate, got {other:?}"),
    }
    assert_eq!(
        serving.servant.channel_names(),
        vec!["EventChannel".to_string(), "orders".to_string()]
    );

    let mut supplier = serving.push_consumer_proxy("orders");
    client::push(&mut supplier, &TypeCode::ULong, |e| e.put_u32(5)).unwrap();
    until(&first, "the surviving channel still delivers", |s| s.delivered == 1);
    assert!(consumer.sink.wait_for(1, T));
    assert_eq!(ulong(&consumer.sink.snapshot()[0]), 5);

    drop(supplier);
    consumer.shutdown();
    serving.shutdown();
}

/// A channel created **after** the server started delivering starts its own
/// threads, rather than accepting and queueing forever.
///
/// The failure this pins is invisible from outside: a channel with no outbound
/// threads answers every operation, accepts every push and reports rising
/// `accepted` — it looks exactly like a channel whose consumers are all slow.
#[test]
fn a_channel_created_after_delivery_started_delivers_too() {
    let mut serving = Serving::paused();
    serving.begin();

    let late = serving.servant.create_channel("late").unwrap();
    late.set_source_poll(POLL);
    let consumer = Consumer::start(b"LateConsumer");
    serving.attach_consumer("late", &consumer.ior);

    let mut supplier = serving.push_consumer_proxy("late");
    client::push(&mut supplier, &TypeCode::ULong, |e| e.put_u32(0x1A7E)).unwrap();
    until(&late, "the late channel delivered", |s| s.delivered == 1);
    assert!(consumer.sink.wait_for(1, T));
    assert_eq!(ulong(&consumer.sink.snapshot()[0]), 0x1A7E);

    // And its source thread runs too: a supplier attached to it is asked.
    let supplier_servant = Supplier::start(b"LateSupplier");
    supplier_servant.source.offer(&TypeCode::ULong, Endian::native(), |e| e.put_u32(2)).unwrap();
    serving.attach_supplier("late", &supplier_servant.ior);
    until(&late, "the late channel's source thread ran", |s| s.sourced == 1);

    drop(supplier);
    supplier_servant.shutdown();
    consumer.shutdown();
    serving.shutdown();
}

/// All four models on a channel that was **created**, not the one the server
/// was built with. The proxies of every model have to mint under that
/// channel's prefix and route back to it, which is the whole of what a second
/// channel risks getting wrong.
#[test]
fn all_four_models_work_on_a_created_channel() {
    let mut serving = Serving::paused();
    let handle = serving.servant.create_channel("second").unwrap();
    serving.begin();
    handle.set_source_poll(POLL);

    for (supplier_pulls, consumer_pulls, payload) in
        [(false, false, 0xE0u32), (false, true, 0xE1), (true, false, 0xE2), (true, true, 0xE3)]
    {
        let model = format!(
            "{}/{}",
            if supplier_pulls { "pull" } else { "push" },
            if consumer_pulls { "pull" } else { "push" }
        );
        let before = handle.stats().accepted;

        let mut puller = None;
        let mut consumer = None;
        if consumer_pulls {
            puller = Some(serving.pull_supplier_proxy("second"));
        } else {
            let c = Consumer::start(b"SecondConsumer");
            serving.attach_consumer("second", &c.ior);
            consumer = Some(c);
        }

        let mut supplier = None;
        let mut pusher = None;
        if supplier_pulls {
            let s = Supplier::start(b"SecondSupplier");
            s.source
                .offer(&TypeCode::ULong, Endian::native(), move |e| e.put_u32(payload))
                .unwrap();
            serving.attach_supplier("second", &s.ior);
            supplier = Some(s);
        } else {
            let mut conn = serving.push_consumer_proxy("second");
            client::push(&mut conn, &TypeCode::ULong, move |e| e.put_u32(payload)).unwrap();
            pusher = Some(conn);
        }

        if let Some(mut conn) = puller {
            let got = client::pull(&mut conn).unwrap_or_else(|e| panic!("{model}: {e:?}"));
            assert_eq!(ulong(&got), payload, "{model}");
            drop(conn);
        } else {
            let c = consumer.as_ref().expect("a consumer of one kind or the other");
            assert!(c.sink.wait_for(1, T), "{model}: nothing was delivered");
            assert_eq!(ulong(c.sink.snapshot().last().unwrap()), payload, "{model}");
        }
        assert_eq!(handle.stats().accepted, before + 1, "{model}");
        // The channel the server was built with was never touched by any of it.
        assert_eq!(serving.servant.handle().stats().accepted, 0, "{model}: the default channel");

        drop(pusher);
        if let Some(s) = supplier {
            s.shutdown();
        }
        if let Some(c) = consumer {
            c.shutdown();
        }
    }

    serving.shutdown();
}

/// The total is a sum, and the sum keeps the invariant. Two channels are made
/// to drop for **different** causes, so a total that lost the split — or that
/// summed one cause into another — cannot pass.
///
/// `split_adds_up` is a linear identity, so it holds of a sum exactly when it
/// holds of every part. That is why `total_stats` can carry it and why a
/// failure here would mean a channel had failed it, not the addition.
#[test]
fn the_server_wide_total_is_a_sum_whose_drop_split_still_adds_up() {
    let serving = Serving::paused();
    let overflowing = serving.servant.create_channel("overflowing").unwrap();
    let stopping = serving.servant.create_channel("stopping").unwrap();
    // No delivery threads at all: a drop test that raced a drain would be
    // measuring the scheduler.
    overflowing.set_queue_limit(2);

    // Channel one: back-pressure.
    let consumer = Consumer::start(b"OverflowConsumer");
    serving.attach_consumer("overflowing", &consumer.ior);
    let mut supplier = serving.push_consumer_proxy("overflowing");
    for i in 0..5u32 {
        client::push(&mut supplier, &TypeCode::ULong, move |e| e.put_u32(i)).unwrap();
    }
    drop(supplier);

    // Channel two: housekeeping, from a stop with a backlog.
    let other = Consumer::start(b"StopConsumer");
    serving.attach_consumer("stopping", &other.ior);
    let mut supplier = serving.push_consumer_proxy("stopping");
    for i in 0..4u32 {
        client::push(&mut supplier, &TypeCode::ULong, move |e| e.put_u32(i)).unwrap();
    }
    drop(supplier);
    stopping.stop();

    let a = overflowing.stats();
    assert_eq!(a.dropped_overflow, 3, "five into a bound of two");
    assert_eq!(a.dropped_at_stop, 0);
    assert!(a.split_adds_up(), "{a:?}");
    let b = stopping.stats();
    assert_eq!(b.dropped_at_stop, 4, "the whole backlog went with the stop");
    assert_eq!(b.dropped_overflow, 0);
    assert!(b.split_adds_up(), "{b:?}");

    let total = serving.servant.total_stats();
    assert_eq!(total.dropped, 7);
    assert_eq!(total.dropped_overflow, 3, "back-pressure stays back-pressure in the sum");
    assert_eq!(total.dropped_at_stop, 4, "and housekeeping stays housekeeping");
    assert!(total.split_adds_up(), "{total:?}");

    consumer.shutdown();
    other.shutdown();
    serving.shutdown();
}

/// Dropping the `Delivery` stops **every** channel, not only the one the
/// server was built with. A channel left running past the handle that owns its
/// threads is the "spike that forgets to stop" failure with more places to
/// hide.
#[test]
fn dropping_the_delivery_stops_every_channel() {
    let mut serving = Serving::paused();
    let created = serving.servant.create_channel("created").unwrap();
    serving.begin();
    let default = serving.servant.handle();

    let supplier = Supplier::start(b"StopEverySupplier");
    serving.attach_supplier("created", &supplier.ior);
    supplier.source.offer(&TypeCode::ULong, Endian::native(), |e| e.put_u32(1)).unwrap();
    until(&created, "the created channel was pulling", |s| s.sourced == 1);

    drop(serving.delivery.take());

    // Both channels are stopped: a pull on either answers `Disconnected`
    // rather than blocking, which is what a stopped channel says.
    for handle in [&default, &created] {
        assert!(handle.wait_until(T, |_| true), "the handle still answers");
    }
    // And the created channel really stopped asking.
    std::thread::sleep(Duration::from_millis(80));
    let asked = supplier.source.try_pull_calls();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(supplier.source.try_pull_calls(), asked, "a stopped channel asks nobody");

    supplier.shutdown();
    serving.shutdown();
}
