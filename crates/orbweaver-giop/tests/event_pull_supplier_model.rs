//! The **supplier** side of the CosEvent pull model, over real sockets: the
//! channel as a client of a `CosEventComm::PullSupplier`, and with it all four
//! of CosEvent's models.
//!
//! # What this measures that the consumer half could not
//!
//! `tests/event_pull_model.rs` covers the direction where the channel is
//! asked. This one covers the direction where the channel **asks**, which is a
//! new outbound direction for this servant and therefore a new set of ways to
//! be wrong: a supplier that never answers, a supplier that says it is
//! finished, a byte order chosen by somebody else, and a shutdown that has to
//! leave the drop accounting exactly as true as it found it.
//!
//! The oracle here is CORBA 3.4's `CosEventComm` and `CosEventChannelAdmin`
//! chapters plus our own halves over loopback, with the same limit stated on
//! the consumer half: it proves we do what we read. The independent half — an
//! omniORB `PullSupplier` our channel pulls from — is `spikes/event_pull_supplier.py`
//! against `spike-events --hold`, and it is a *peer* measurement rather than a
//! self-test, which is the standard the rest of this service is held to.
//!
//! # The 2×2, walked
//!
//! [`all_four_models_carry_the_event_they_were_given`] is the point of the
//! batch rather than a test of one operation: each side of a CosEvent channel
//! is either pushed to or pulled from, the channel is the other half of both,
//! and until `obtain_pull_consumer` existed two of the four combinations could
//! not be created at all. The test creates each one over the wire and asserts
//! an event crosses it.

use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use orbweaver_cdr::Endian;
use orbweaver_giop::event_server::{
    ALREADY_CONNECTED_ID, CORBA_OBJECT_ID, ChannelHandle, DISCONNECTED_ID, Delivery,
    EventChannelServer, EventSink, EventSource, MAX_CONSECUTIVE_FAILURES, PROXY_PULL_CONSUMER_ID,
    PROXY_PULL_SUPPLIER_ID, PULL_SUPPLIER_ID, PullSupplierServant, PushConsumerServant, client,
};
use orbweaver_giop::server::Server;
use orbweaver_giop::typecode::{Any, TypeCode};
use orbweaver_giop::{Connection, Error, IiopProfile, Ior, Version};

/// Generous: every deadline a test asserts on is one it set itself.
const T: Duration = Duration::from_secs(5);
/// Short enough that a supplier which will not answer costs milliseconds
/// rather than connect-timeout multiples.
const OUTBOUND_T: Duration = Duration::from_millis(500);
/// Fast enough that a test does not spend its life waiting out barren rounds.
/// The default is `DEFAULT_SOURCE_POLL`; a test that used it would be
/// measuring the constant rather than the loop.
const POLL: Duration = Duration::from_millis(5);
/// How long "and it stayed still" is given. Twenty `POLL` intervals: a channel
/// that were still asking would ask twenty times in here, so this is a margin
/// rather than a coin toss.
const STILL: Duration = Duration::from_millis(100);

/// A channel on loopback with both outbound threads running.
struct Channel {
    ior: Ior,
    handle: ChannelHandle,
    delivery: Option<Delivery>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Channel {
    fn start() -> Self {
        let server = Server::bind("127.0.0.1:0", b"EventChannel".to_vec()).unwrap();
        let port = server.local_addr().unwrap().port();
        let channel =
            Arc::new(EventChannelServer::new("127.0.0.1", port, b"EventChannel".to_vec()));
        let ior = channel.channel_ior();
        let handle = channel.handle();
        // Both threads, and a short outbound timeout so the failure paths are
        // measured rather than waited out.
        let delivery = channel.start_delivery_with(OUTBOUND_T);
        handle.set_source_poll(POLL);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        // `serve_shared`: a blocking `pull` occupies its own connection's
        // thread, and the serialized path would let it occupy the only one.
        let thread = std::thread::spawn(move || {
            server.serve_shared(&*channel, move || flag.load(Ordering::SeqCst)).unwrap();
        });
        Channel { ior, handle, delivery: Some(delivery), stop, thread: Some(thread) }
    }

    fn dial(&self, ior: &Ior) -> Connection {
        Connection::connect(ior, T).unwrap()
    }

    fn channel_conn(&self) -> Connection {
        self.dial(&self.ior)
    }

    /// A `ProxyPullConsumer` with `supplier` connected to it: the shape the
    /// whole batch is about.
    fn pull_consumer_proxy(&self, supplier: &Ior) -> Ior {
        let (proxy, mut conn) = self.pull_consumer_proxy_conn();
        client::connect_pull_supplier(&mut conn, supplier).unwrap();
        drop(conn);
        proxy
    }

    /// A freshly minted `ProxyPullConsumer` and an open connection to it.
    fn pull_consumer_proxy_conn(&self) -> (Ior, Connection) {
        let mut conn = self.channel_conn();
        let admin = client::for_suppliers(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&admin);
        let proxy = client::obtain_pull_consumer(&mut conn).unwrap();
        assert_eq!(proxy.type_id, PROXY_PULL_CONSUMER_ID);
        drop(conn);
        let conn = self.dial(&proxy);
        (proxy, conn)
    }

    /// A connected `ProxyPushConsumer`, ready to be pushed into.
    fn push_consumer_proxy(&self) -> Connection {
        let mut conn = self.channel_conn();
        let admin = client::for_suppliers(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&admin);
        let proxy = client::obtain_push_consumer(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&proxy);
        client::connect_push_supplier(&mut conn, &client::nil_ref()).unwrap();
        conn
    }

    /// A `ProxyPushSupplier` with `consumer` attached.
    fn push_supplier_proxy(&self, consumer: &Ior) {
        let mut conn = self.channel_conn();
        let admin = client::for_consumers(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&admin);
        let proxy = client::obtain_push_supplier(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&proxy);
        client::connect_push_consumer(&mut conn, consumer).unwrap();
    }

    /// A `ProxyPullSupplier` with a nil `PullConsumer` connected, and an open
    /// connection to pull on.
    fn pull_supplier_proxy(&self) -> Connection {
        let mut conn = self.channel_conn();
        let admin = client::for_consumers(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&admin);
        let proxy = client::obtain_pull_supplier(&mut conn).unwrap();
        assert_eq!(proxy.type_id, PROXY_PULL_SUPPLIER_ID);
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

/// A `CosEventComm::PullSupplier` of ours on its own loopback server — the
/// object `PLAN-DEFERRED` §10's trigger named.
struct Supplier {
    ior: Ior,
    source: EventSource,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Supplier {
    fn start(key: &[u8]) -> Self {
        let server = Server::bind("127.0.0.1:0", key.to_vec()).unwrap();
        let port = server.local_addr().unwrap().port();
        let servant = Arc::new(PullSupplierServant::new(key.to_vec()));
        let ior = servant.ior("127.0.0.1", port);
        assert_eq!(ior.type_id, PULL_SUPPLIER_ID);
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

/// A collecting `CosEventComm::PushConsumer` on its own loopback server.
struct Consumer {
    ior: Ior,
    sink: EventSink,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Consumer {
    fn start(key: &[u8]) -> Self {
        let server = Server::bind("127.0.0.1:0", key.to_vec()).unwrap();
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

/// A reference to a port that was bound and then released: dialling it is
/// refused immediately, which is what an unreachable supplier looks like
/// without waiting out a connect timeout.
fn dead_supplier_ior() -> Ior {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    Ior {
        type_id: PULL_SUPPLIER_ID.into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "127.0.0.1".into(),
            port,
            object_key: b"DeadSupplier".to_vec(),
            components: Vec::new(),
        }],
    }
}

fn ulong(any: &Any) -> u32 {
    assert_eq!(any.tc, TypeCode::ULong);
    any.value_decoder().get_u32().unwrap()
}

fn user_exception_id(e: &Error) -> &str {
    match e {
        Error::UserException { id, .. } => id,
        other => panic!("expected a user exception, got {other:?}"),
    }
}

fn system_exception_id(e: &Error) -> &str {
    match e {
        Error::SystemException { id, .. } => id,
        other => panic!("expected a system exception, got {other:?}"),
    }
}

/// A sleeping, deadline-bounded wait on the counters. `ChannelHandle::wait_until`
/// is woken by the delivery thread's progress; the source thread notifies it
/// too, and this is where that is relied on.
fn until(
    handle: &ChannelHandle,
    what: &str,
    pred: impl FnMut(&orbweaver_giop::event_server::ChannelStats) -> bool,
) {
    assert!(handle.wait_until(T, pred), "{what}: {:?}", handle.stats());
}

/// Asserts that a supplier has **stopped being asked**.
///
/// Two things can move `try_pull_calls` just after a disconnect or a `stop`,
/// and only one of them is a defect:
///
/// - a round the channel had already committed to before the flag changed can
///   still land. That is the module's stated bound — at most one, within the
///   outbound timeout — and it is permitted;
/// - a round issued *after* the flag changed is the channel still asking, and
///   that is the defect.
///
/// Sampling once and asserting equality after a sleep cannot tell them apart,
/// and picks whichever one the scheduler hands it. That is precisely how the
/// three sites this replaces were green on macOS — 20 serial runs, 5
/// concurrent whole-suite runs, a 200 µs source poll — and red on CI Linux
/// under five concurrent whole-suite runs: the sample was taken an instant
/// before the permitted call landed, so the permitted call read as the defect.
///
/// So the permitted round is waited out on the channel's own observable first,
/// and only then is stillness asserted. That is a *stronger* claim than the
/// one it replaces, not a looser one: the old form would have passed on a
/// channel that asked once more, if the sample happened to fall after it.
fn stops_being_asked(handle: &ChannelHandle, source: &EventSource, what: &str) {
    assert!(handle.wait_source_idle(T), "{what}: a taken source round never finished");
    let settled = source.try_pull_calls();
    std::thread::sleep(STILL);
    assert_eq!(source.try_pull_calls(), settled, "{what}");
}

/// A one-shot barrier the source thread is held at, for the control below.
///
/// The source loop calls it after taking a round and before the commit point,
/// so a test that holds it there owns the ordering the CI failure produced by
/// luck. Deadline-bounded on purpose: `Delivery`'s drop joins the source
/// thread, so a barrier with no deadline of its own would turn a failing test
/// into a hanging one.
#[derive(Default)]
struct HeldRound {
    /// `(a round is being held, the test has released it)`.
    state: Mutex<(bool, bool)>,
    cv: Condvar,
}

impl HeldRound {
    /// Called on the source thread. Blocks the first round until `release`;
    /// every round after that passes straight through.
    fn hold(&self) {
        let deadline = Instant::now() + T;
        let mut s = self.state.lock().unwrap();
        if s.1 {
            return;
        }
        s.0 = true;
        self.cv.notify_all();
        while !s.1 {
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return; // the test has gone wrong; do not also hang it
            };
            s = self.cv.wait_timeout(s, left).unwrap().0;
        }
    }

    /// Whether a round reached the barrier before `timeout`.
    fn wait_until_held(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut s = self.state.lock().unwrap();
        loop {
            if s.0 {
                return true;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            s = self.cv.wait_timeout(s, left).unwrap().0;
        }
    }

    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.cv.notify_all();
    }
}

/// The path the deferral was about, in both byte orders: a supplier that must
/// be **asked** hands the channel events, and a push consumer that never asked
/// for anything receives them.
///
/// The string is not decoration. Its length prefix is the field that notices a
/// byte order swapped between the supplier's reply and the consumer's request,
/// which a palindromic `ulong` would not — and this path has two byte-order
/// hops rather than the push path's one, because the channel chooses the order
/// it *asks* in and then relays what it was given.
#[test]
fn a_channel_pulls_from_a_supplier_and_a_push_consumer_receives_it() {
    for endian in [Endian::Big, Endian::Little] {
        let chan = Channel::start();
        chan.handle.set_source_endian(endian);

        let consumer = Consumer::start(b"PulledConsumer");
        chan.push_supplier_proxy(&consumer.ior);

        let supplier = Supplier::start(b"Supplier");
        for i in 0..4u32 {
            supplier
                .source
                .offer(&TypeCode::ULong, endian, move |e| e.put_u32(0xABC0 + i))
                .unwrap();
        }
        supplier.source.offer(&TypeCode::String(0), endian, |e| e.put_str("함정")).unwrap();
        chan.pull_consumer_proxy(&supplier.ior);

        until(&chan.handle, &format!("{endian:?}: five events delivered"), |s| s.delivered == 5);
        assert!(
            supplier.source.wait_until_drained(T),
            "{endian:?}: the supplier still holds events"
        );

        let got = consumer.sink.snapshot();
        assert_eq!(got.len(), 5, "{endian:?}");
        let numbers: Vec<u32> = got[..4].iter().map(ulong).collect();
        assert_eq!(numbers, vec![0xABC0, 0xABC1, 0xABC2, 0xABC3], "{endian:?}: order or value");
        assert_eq!(got[4].tc, TypeCode::String(0), "{endian:?}");
        assert_eq!(got[4].value_decoder().get_string().unwrap(), "함정", "{endian:?}");
        assert_eq!(got[4].endian, endian, "{endian:?}: the relay kept the captured byte order");

        let stats = chan.handle.stats();
        assert_eq!(stats.sourced, 5, "{endian:?}: every event was fetched, not pushed");
        assert_eq!(stats.accepted, 5, "{endian:?}: a fetched event is an accepted event");
        assert_eq!(stats.pull_suppliers_connected, 1, "{endian:?}");
        assert_eq!(stats.dropped, 0, "{endian:?}");
        assert_eq!(stats.unrelayable, 0, "{endian:?}");
        assert_eq!(stats.pull_failures, 0, "{endian:?}");
        assert!(stats.split_adds_up(), "{endian:?}: {stats:?}");

        supplier.shutdown();
        consumer.shutdown();
        chan.shutdown();
    }
}

/// **The batch, in one test.** Each side of a CosEvent channel is either
/// pushed to or pulled from, so there are four models; two of them could not
/// be created at all until `obtain_pull_consumer` answered with a reference,
/// and this walks all four over the wire and asserts an event crosses each.
///
/// The operation pairs are written out because they are the thing being
/// claimed: a model is *created* by the two `obtain_*` calls that select it,
/// and a test that only checked the payload could pass while creating the same
/// model twice.
#[test]
fn all_four_models_carry_the_event_they_were_given() {
    for (supplier_pulls, consumer_pulls, payload) in
        [(false, false, 0xF0u32), (false, true, 0xF1), (true, false, 0xF2), (true, true, 0xF3)]
    {
        let model = format!(
            "{}/{}",
            if supplier_pulls { "pull" } else { "push" },
            if consumer_pulls { "pull" } else { "push" }
        );
        let chan = Channel::start();
        chan.handle.set_source_endian(Endian::native());

        // ── the consumer side, first: a fan-out reaches only what is already
        // connected, so a model wired the other way round would measure the
        // channel's queue rather than its plumbing. ──
        let mut puller = None;
        let mut consumer = None;
        if consumer_pulls {
            puller = Some(chan.pull_supplier_proxy());
        } else {
            let c = Consumer::start(b"FourModelConsumer");
            chan.push_supplier_proxy(&c.ior);
            consumer = Some(c);
        }

        // ── the supplier side ──
        let mut supplier = None;
        let mut pusher = None;
        if supplier_pulls {
            let s = Supplier::start(b"FourModelSupplier");
            s.source
                .offer(&TypeCode::ULong, Endian::native(), move |e| e.put_u32(payload))
                .unwrap();
            chan.pull_consumer_proxy(&s.ior);
            supplier = Some(s);
        } else {
            let mut conn = chan.push_consumer_proxy();
            client::push(&mut conn, &TypeCode::ULong, move |e| e.put_u32(payload)).unwrap();
            pusher = Some(conn);
        }

        // ── the event crosses ──
        if let Some(mut conn) = puller {
            let got = client::pull(&mut conn).unwrap_or_else(|e| panic!("{model}: {e:?}"));
            assert_eq!(ulong(&got), payload, "{model}");
            drop(conn);
        } else {
            let c = consumer.as_ref().expect("a push consumer or a pull one");
            assert!(c.sink.wait_for(1, T), "{model}: nothing was delivered");
            assert_eq!(ulong(&c.sink.snapshot()[0]), payload, "{model}");
        }

        let stats = chan.handle.stats();
        assert_eq!(stats.accepted, 1, "{model}");
        assert_eq!(
            stats.sourced,
            u64::from(supplier_pulls),
            "{model}: `sourced` is what the channel had to go and ask for"
        );
        assert_eq!(stats.pull_suppliers_connected, usize::from(supplier_pulls), "{model}");
        assert_eq!(stats.pull_consumers_connected, usize::from(consumer_pulls), "{model}");
        assert_eq!(stats.dropped, 0, "{model}");
        assert!(stats.split_adds_up(), "{model}: {stats:?}");

        drop(pusher);
        if let Some(s) = supplier {
            s.shutdown();
        }
        if let Some(c) = consumer {
            c.shutdown();
        }
        chan.shutdown();
    }
}

/// The design decision, measured instead of asserted in a comment: the channel
/// polls with `try_pull` and **never** calls the blocking `pull`.
///
/// `pull` is specified to block until the supplier has something. A channel
/// that called it would hold a thread on somebody else's clock, and because
/// the source round is shared, one silent supplier would be every other
/// supplier's outage. Making the servant count both operations is what turns
/// that paragraph into something a test can fail on: replace `try_pull` with
/// `pull` in `source_pull` and this goes red on the first assertion.
#[test]
fn the_channel_asks_with_try_pull_and_never_blocks_in_pull() {
    let chan = Channel::start();
    let supplier = Supplier::start(b"QuietSupplier");
    chan.pull_consumer_proxy(&supplier.ior);

    // A supplier holding nothing at all is the case a blocking `pull` would
    // never return from. Wait out several poll intervals, sleeping.
    let deadline = Instant::now() + Duration::from_millis(400);
    while Instant::now() < deadline && supplier.source.try_pull_calls() < 3 {
        std::thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(supplier.source.pull_calls(), 0, "the channel must never call the blocking pull");
    assert!(
        supplier.source.try_pull_calls() >= 3,
        "the channel keeps asking: {} try_pull(s)",
        supplier.source.try_pull_calls()
    );
    let stats = chan.handle.stats();
    assert_eq!(stats.sourced, 0, "an empty supplier yields nothing");
    assert_eq!(stats.pull_failures, 0, "and an empty answer is not a failure");
    assert_eq!(stats.pull_suppliers_connected, 1, "it is still connected");

    // And it is still live: something offered now is fetched.
    supplier.source.offer(&TypeCode::ULong, Endian::native(), |e| e.put_u32(11)).unwrap();
    until(&chan.handle, "the offered event was fetched", |s| s.sourced == 1);
    assert_eq!(supplier.source.pull_calls(), 0, "still never the blocking one");

    supplier.shutdown();
    chan.shutdown();
}

/// A supplier that cannot be reached is governed by the same
/// `MAX_CONSECUTIVE_FAILURES` the push direction has — and because a
/// `ProxyPullConsumer` holds no queue, the disconnect drops nothing and adds
/// no cause to the split.
///
/// That last clause is the one worth pinning. Every other way this channel
/// gives up on a peer abandons a backlog and has to say which cause it was; a
/// new drop counter here that nobody added to `by_cause` would break
/// `split_adds_up`, and a drop counted with no backlog behind it would be a
/// fabrication. Neither happens, and both are asserted.
#[test]
fn a_supplier_that_cannot_be_reached_is_released_after_the_threshold() {
    let chan = Channel::start();
    chan.pull_consumer_proxy(&dead_supplier_ior());

    until(&chan.handle, "the unreachable supplier was released", |s| {
        s.disconnected_for_failure == 1
    });
    let stats = chan.handle.stats();
    assert_eq!(
        stats.pull_failures,
        u64::from(MAX_CONSECUTIVE_FAILURES),
        "exactly the threshold's worth of failures was spent on it"
    );
    assert_eq!(stats.push_failures, 0, "nothing was pushed, so nothing failed to be pushed");
    assert_eq!(stats.pull_suppliers_connected, 0, "the supplier reference was released");
    assert_eq!(stats.sourced, 0);
    assert_eq!(stats.dropped, 0, "this proxy never held an event, so nothing was abandoned");
    assert_eq!(stats.dropped_on_failure_disconnect, 0, "and no cause may be invented for it");
    assert!(stats.split_adds_up(), "{stats:?}");

    // The proxy stays known and reconnectable, the same choice every other
    // proxy here makes — and a live supplier attached to it now is served.
    let supplier = Supplier::start(b"RevivedSupplier");
    supplier.source.offer(&TypeCode::ULong, Endian::native(), |e| e.put_u32(42)).unwrap();
    chan.pull_consumer_proxy(&supplier.ior);
    until(&chan.handle, "the second supplier was pulled from", |s| s.sourced == 1);
    assert_eq!(chan.handle.stats().disconnected_for_failure, 1, "and no second give-up");

    supplier.shutdown();
    chan.shutdown();
}

/// A supplier that answers `Disconnected` is released **without** a failure
/// counted. It did not fail; it said it was finished, and the standard gives
/// it that word for exactly this. Counting it as a failure would spend two of
/// three retries re-asking a peer that had already answered.
#[test]
fn a_supplier_that_says_disconnected_is_released_without_a_failure() {
    let chan = Channel::start();
    let supplier = Supplier::start(b"FinishedSupplier");
    supplier.source.offer(&TypeCode::ULong, Endian::native(), |e| e.put_u32(7)).unwrap();
    chan.pull_consumer_proxy(&supplier.ior);
    until(&chan.handle, "the one offered event was fetched", |s| s.sourced == 1);

    supplier.source.disconnect();
    until(&chan.handle, "the finished supplier was released", |s| s.pull_suppliers_connected == 0);

    let stats = chan.handle.stats();
    assert_eq!(stats.pull_failures, 0, "a supplier that says Disconnected has not failed");
    assert_eq!(stats.disconnected_for_failure, 0, "and this channel did not give up on it");
    assert_eq!(stats.sourced, 1);
    assert_eq!(stats.dropped, 0);
    assert!(stats.split_adds_up(), "{stats:?}");

    // It stopped being asked, rather than being asked forever.
    stops_being_asked(&chan.handle, &supplier.source, "a released supplier is not re-asked");

    supplier.shutdown();
    chan.shutdown();
}

/// The state machine §2.3 defines for the proxy a supplier connects to: a nil
/// `PullSupplier` is `BAD_PARAM` — unlike a nil `PullConsumer`, because this
/// one is dialled and that one never is — a second connect is
/// `AlreadyConnected`, and `disconnect_pull_consumer` is idempotent and leaves
/// the key reconnectable.
#[test]
fn the_pull_supplier_connect_and_disconnect_state_machine() {
    let chan = Channel::start();
    let supplier = Supplier::start(b"StateMachineSupplier");
    let (_, mut conn) = chan.pull_consumer_proxy_conn();

    let err = client::connect_pull_supplier(&mut conn, &client::nil_ref())
        .expect_err("a nil PullSupplier is an address nothing can dial");
    assert_eq!(system_exception_id(&err), "IDL:omg.org/CORBA/BAD_PARAM:1.0");
    assert_eq!(chan.handle.stats().pull_suppliers_connected, 0, "and it was not recorded");

    client::connect_pull_supplier(&mut conn, &supplier.ior).unwrap();
    let err =
        client::connect_pull_supplier(&mut conn, &supplier.ior).expect_err("a second connect");
    assert_eq!(user_exception_id(&err), ALREADY_CONNECTED_ID);

    supplier.source.offer(&TypeCode::ULong, Endian::native(), |e| e.put_u32(1)).unwrap();
    until(&chan.handle, "the event was fetched", |s| s.sourced == 1);

    client::disconnect_pull_consumer(&mut conn).unwrap();
    assert_eq!(chan.handle.stats().pull_suppliers_connected, 0);
    // Nothing was queued here, so nothing was dropped — the assertion the
    // split exists for, made where a new counter would have been tempting.
    assert_eq!(chan.handle.stats().dropped, 0);
    assert!(chan.handle.stats().split_adds_up());

    // It really stopped asking, and offering more does not reach the channel.
    // The offer goes in *before* the settle, so a channel that were still
    // asking would have something to find and both assertions would catch it.
    supplier.source.offer(&TypeCode::ULong, Endian::native(), |e| e.put_u32(2)).unwrap();
    stops_being_asked(&chan.handle, &supplier.source, "a disconnected proxy is not asked");
    assert_eq!(chan.handle.stats().sourced, 1, "and nothing more was fetched");

    // Idempotent, and the key stays reconnectable.
    client::disconnect_pull_consumer(&mut conn).unwrap();
    client::connect_pull_supplier(&mut conn, &supplier.ior).unwrap();
    until(&chan.handle, "the reconnected proxy fetched the second event", |s| s.sourced == 2);

    drop(conn);
    supplier.shutdown();
    chan.shutdown();
}

/// **The control.** The race the three settle-and-pin sites above were
/// sampling through, made to happen every run instead of once in a few hundred
/// on a loaded Linux box.
///
/// The channel takes a round, and is held there with nothing on the wire yet;
/// the disconnect lands; the round is released. That is the exact interleaving
/// CI produced by luck — `disconnect_pull_consumer` returned and a `try_pull`
/// followed it — and the only ordering under which the commit point does
/// anything at all.
///
/// Its own negative control: delete the commit-point check in `source_pull`
/// (`source_still_wanted` / `SourceOutcome::Cancelled`) and this fails on the
/// `try_pull_calls` assertion with `left: 1  right: 0`, which is the CI
/// diagnostic one proxy earlier in the same story.
#[test]
fn a_round_taken_before_a_disconnect_is_cancelled_rather_than_issued() {
    let chan = Channel::start();
    let supplier = Supplier::start(b"HeldRoundSupplier");
    let (_, mut conn) = chan.pull_consumer_proxy_conn();

    let gate = Arc::new(HeldRound::default());
    let held = Arc::clone(&gate);
    chan.handle.set_source_gate(move |_proxy| held.hold());

    // Connecting is what gives the source loop a round to take, so the barrier
    // is installed first: the first round there can ever be is the held one.
    client::connect_pull_supplier(&mut conn, &supplier.ior).unwrap();
    assert!(gate.wait_until_held(T), "the source thread never took a round");
    assert_eq!(
        supplier.source.try_pull_calls(),
        0,
        "the barrier is before the commit point: nothing may have gone out yet"
    );

    // The disconnect lands while the round is held — and returns, which is the
    // moment the property is about.
    client::disconnect_pull_consumer(&mut conn).unwrap();
    gate.release();

    assert!(chan.handle.wait_source_idle(T), "the held round never finished");
    assert_eq!(
        supplier.source.try_pull_calls(),
        0,
        "a round taken before the disconnect must not be issued after it"
    );
    let stats = chan.handle.stats();
    assert_eq!(stats.pull_rounds_cancelled, 1, "it was thrown away at the commit point");
    assert_eq!(stats.pull_failures, 0, "a cancelled round is not a failure — nobody failed");
    assert_eq!(stats.sourced, 0, "and nothing was fetched");
    assert_eq!(stats.dropped, 0, "this proxy holds no queue, so there is nothing to abandon");
    assert!(stats.split_adds_up(), "{stats:?}");

    // And the proxy is still the reconnectable key every other one here is:
    // the round that was cancelled cost the supplier nothing.
    client::connect_pull_supplier(&mut conn, &supplier.ior).unwrap();
    supplier.source.offer(&TypeCode::ULong, Endian::native(), |e| e.put_u32(9)).unwrap();
    until(&chan.handle, "the reconnected proxy was pulled from", |s| s.sourced == 1);

    drop(conn);
    supplier.shutdown();
    chan.shutdown();
}

/// `stop()` with a supplier connected, which is the question the module docs
/// answer in prose and this answers in counters: a supplier connection is not
/// a queue, so nothing is discarded on its account and the drop split stays
/// true. The connectedness survives — the same choice a push proxy's consumer
/// reference makes — and it is the *thread* that stops, not the state.
#[test]
fn stopping_the_channel_leaves_the_supplier_connected_and_the_split_true() {
    let chan = Channel::start();
    let supplier = Supplier::start(b"StoppedSupplier");
    // A pull consumer nobody drains, so `stop` has a real backlog to count and
    // the assertion below is that the supplier side added nothing to it.
    let puller = chan.pull_supplier_proxy();
    for i in 0..3u32 {
        supplier.source.offer(&TypeCode::ULong, Endian::native(), move |e| e.put_u32(i)).unwrap();
    }
    chan.pull_consumer_proxy(&supplier.ior);
    until(&chan.handle, "three events were fetched", |s| s.sourced == 3);
    assert_eq!(chan.handle.stats().queued, 3, "and queued for the consumer that never came");

    chan.handle.stop();
    let stats = chan.handle.stats();
    assert_eq!(stats.dropped_at_stop, 3, "the pull consumer's backlog, and only that");
    assert_eq!(stats.dropped, 3, "the supplier connection is not a queue and drops nothing");
    assert_eq!(stats.dropped_on_disconnect, 0);
    assert_eq!(stats.dropped_on_failure_disconnect, 0);
    assert!(stats.split_adds_up(), "{stats:?}");
    assert_eq!(
        stats.pull_suppliers_connected, 1,
        "the proxy keeps its supplier reference: the thread stopped, the state did not"
    );

    // And the supplier stops being asked, because the thread that asked is
    // gone. `stop` fails the same commit point a disconnect does, so a round
    // not yet past it is cancelled and one already past it completes — at most
    // one extra call, never a stream. Wait that round out, then prove it is
    // still: the two are different claims and only the second is a defect.
    stops_being_asked(&chan.handle, &supplier.source, "a stopped channel asks nobody");

    drop(puller);
    supplier.shutdown();
    chan.shutdown();
}

/// `_is_a` on the new object: its own interface and `CORBA::Object`, and
/// nothing else. Every ORB probes before it trusts a narrow, and a proxy that
/// claimed to be the pull *supplier* would be narrowed to an interface whose
/// operations it does not serve.
#[test]
fn is_a_on_a_pull_consumer_proxy_answers_for_its_own_interface_only() {
    let chan = Channel::start();
    let (_, mut conn) = chan.pull_consumer_proxy_conn();

    for (id, expected) in
        [(PROXY_PULL_CONSUMER_ID, true), (CORBA_OBJECT_ID, true), (PROXY_PULL_SUPPLIER_ID, false)]
    {
        let reply = conn.invoke("_is_a", move |e| e.put_str(id)).unwrap();
        assert_eq!(reply.body().unwrap().get_bool().unwrap(), expected, "_is_a {id}");
    }
    let reply = conn.invoke_nullary("_non_existent").unwrap();
    assert!(!reply.body().unwrap().get_bool().unwrap());

    // And the operations the *other* pull proxy declares are not this one's.
    for op in ["pull", "try_pull", "disconnect_pull_supplier"] {
        let err = conn.invoke_nullary(op).expect_err("a ProxyPullConsumer declares none of these");
        assert_eq!(
            system_exception_id(&err),
            orbweaver_giop::server::BAD_OPERATION,
            "{op} on a ProxyPullConsumer"
        );
    }

    drop(conn);
    chan.shutdown();
}

/// Our own `PullSupplier` servant answered from the other end: this is the
/// fixture the channel pulls from, so a defect in *it* would read as a defect
/// in the channel. `pull` on an empty one blocks and then reports
/// `TIMEOUT`/`COMPLETED_NO`; `try_pull` answers at once with a `tk_null` `any`
/// and a false flag; a disconnected one raises `Disconnected`.
#[test]
fn the_pull_supplier_fixture_keeps_the_contract_the_channel_relies_on() {
    let supplier = Supplier::start(b"ContractSupplier");
    let mut conn = Connection::connect(&supplier.ior, T).unwrap();

    assert!(client::try_pull(&mut conn).unwrap().is_none(), "an empty supplier has nothing");
    assert_eq!(supplier.source.try_pull_calls(), 1);

    supplier.source.offer(&TypeCode::String(0), conn.endian(), |e| e.put_str("함정")).unwrap();
    let got = client::pull(&mut conn).unwrap();
    assert_eq!(got.value_decoder().get_string().unwrap(), "함정");
    assert_eq!(supplier.source.pull_calls(), 1, "this test is the only caller of the blocking one");

    supplier.source.disconnect();
    for op in ["pull", "try_pull"] {
        let err = conn.invoke_nullary(op).expect_err("disconnected");
        assert_eq!(user_exception_id(&err), DISCONNECTED_ID, "{op} after disconnect");
    }

    drop(conn);
    supplier.shutdown();
}
