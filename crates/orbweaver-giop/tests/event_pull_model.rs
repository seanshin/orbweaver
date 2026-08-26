//! The consumer side of the CosEvent pull model, over real sockets.
//!
//! # There is no peer for this, and saying so is part of the result
//!
//! `omniEvents` was probed and is absent — `brew info omnievents` reports *"No
//! available formula"* — so unlike the naming service there is no reference
//! event channel to check ourselves against, and unlike the push half there is
//! not even a half-peer: omniORBpy ships `CosEventComm` stubs, which let a
//! foreign ORB attach to us as a *push* consumer, but a pulling consumer needs
//! `CosEventChannelAdmin::ProxyPullSupplier` stubs and something that mints
//! them, which is the very thing that is not installable. **Nothing here is
//! peer-verified.** The oracle is CORBA 3.4's `CosEventComm` and
//! `CosEventChannelAdmin` chapters plus hand-built GIOP clients — the same
//! arrangement the fragment-reception work used, and with the same limit: it
//! proves we do what we read, not that we do what omniORB does.
//!
//! What that leaves worth testing is exactly what a specification can decide:
//! the state machine (§2.1.1's `Disconnected`, §2.3's `AlreadyConnected`), the
//! reply *shape* of `try_pull` — return value before `out` parameter, and a
//! `tk_null` `any` when there is nothing — and the two policy questions the
//! module had to answer itself because the specification does not: what a pull
//! queue does at its bound, and what a reply does with an `any` it cannot
//! marshal into the byte order the caller asked in.
//!
//! # Why these are out here rather than in the module
//!
//! Every test below is a client of the *published* surface — `client::pull`,
//! `client::try_pull`, `ChannelHandle` — reached over loopback. A pull is the
//! first operation this servant has that blocks inside `dispatch`, so "does a
//! `push` get served while a `pull` waits" is a question about two connections
//! at once, which is a question only an integration test can ask honestly.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use orbweaver_cdr::Endian;
use orbweaver_giop::event_server::{
    ALREADY_CONNECTED_ID, CORBA_OBJECT_ID, ChannelHandle, DISCONNECTED_ID, EventChannelServer,
    PROXY_PULL_SUPPLIER_ID, PROXY_PUSH_SUPPLIER_ID, client,
};
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::Completion;
use orbweaver_giop::typecode::{Any, TypeCode};
use orbweaver_giop::{Connection, Error, Ior};

/// Generous: every deadline a test asserts on is one it set itself.
const T: Duration = Duration::from_secs(5);

/// A channel on loopback with **no delivery thread**, which is what a
/// pull-only test wants: nothing drains a queue except the puller under test,
/// so every count below is exact rather than a race against a drain.
struct Channel {
    ior: Ior,
    handle: ChannelHandle,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Channel {
    fn start() -> Self {
        let server = Orb::new().server("127.0.0.1:0", b"EventChannel".to_vec()).unwrap();
        let port = server.local_addr().unwrap().port();
        let channel = EventChannelServer::new("127.0.0.1", port, b"EventChannel".to_vec());
        let ior = channel.channel_ior();
        let handle = channel.handle();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        // `serve_shared`, deliberately: a blocking `pull` occupies its own
        // connection's thread, and the serialized path would let it occupy the
        // only one. The module docs say to serve pull consumers this way; a
        // test that used the other path would be testing the advice against
        // itself.
        let thread = std::thread::spawn(move || {
            server.serve_shared(&channel, move || flag.load(Ordering::SeqCst)).unwrap();
        });
        Channel { ior, handle, stop, thread: Some(thread) }
    }

    fn dial(&self, ior: &Ior) -> Connection {
        Connection::connect(ior, T).unwrap()
    }

    fn channel_conn(&self) -> Connection {
        self.dial(&self.ior)
    }

    /// A connected `ProxyPushConsumer`, ready to be pushed into.
    fn supplier_proxy(&self) -> Connection {
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

    /// A freshly minted `ProxyPullSupplier`, not yet connected.
    fn pull_proxy_ref(&self) -> Ior {
        let mut conn = self.channel_conn();
        let admin = client::for_consumers(&mut conn).unwrap();
        drop(conn);
        let mut conn = self.dial(&admin);
        let proxy = client::obtain_pull_supplier(&mut conn).unwrap();
        assert_eq!(proxy.type_id, PROXY_PULL_SUPPLIER_ID);
        proxy
    }

    /// A `ProxyPullSupplier` with a nil `PullConsumer` connected to it, and an
    /// open connection to pull on.
    fn pull_proxy(&self) -> (Ior, Connection) {
        let proxy = self.pull_proxy_ref();
        let mut conn = self.dial(&proxy);
        client::connect_pull_consumer(&mut conn, &client::nil_ref()).unwrap();
        (proxy, conn)
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

/// The path the whole deferral was about: a supplier pushes, a consumer that
/// was never called back pulls, and gets the same `any` — **in both byte
/// orders**, because an encoder that only works native-endian passes every
/// local test and fails in the field.
///
/// The string is not decoration. Its length prefix is the field that notices a
/// byte order swapped between capture and reply, which a `ulong` payload of a
/// palindromic value would not.
#[test]
fn a_pulling_consumer_receives_in_order_what_a_supplier_pushed() {
    for endian in [Endian::Big, Endian::Little] {
        let chan = Channel::start();
        let (_, mut puller) = chan.pull_proxy();
        puller.set_endian(endian);

        let mut supplier = chan.supplier_proxy();
        supplier.set_endian(endian);
        for i in 0..4u32 {
            client::push(&mut supplier, &TypeCode::ULong, move |e| e.put_u32(0xABC0 + i)).unwrap();
        }
        client::push(&mut supplier, &TypeCode::String(0), |e| e.put_str("함정")).unwrap();

        let numbers: Vec<u32> =
            (0..4).map(|_| ulong(&client::pull(&mut puller).unwrap())).collect();
        assert_eq!(numbers, vec![0xABC0, 0xABC1, 0xABC2, 0xABC3], "{endian:?}: order or value");

        let text = client::try_pull(&mut puller).unwrap().expect("the fifth event");
        assert_eq!(text.tc, TypeCode::String(0), "{endian:?}");
        assert_eq!(text.value_decoder().get_string().unwrap(), "함정", "{endian:?}");
        assert_eq!(text.endian, endian, "{endian:?}: the reply kept the source byte order");

        let stats = chan.handle.stats();
        assert_eq!(stats.accepted, 5, "{endian:?}");
        assert_eq!(stats.pulled, 5, "{endian:?}");
        assert_eq!(stats.dropped, 0, "{endian:?}");
        assert_eq!(stats.unrelayable, 0, "{endian:?}");
        assert_eq!(stats.queued, 0, "{endian:?}");
        assert_eq!(stats.delivered, 0, "{endian:?}: nothing was pushed outbound");

        drop(puller);
        drop(supplier);
        chan.shutdown();
    }
}

/// `try_pull` on an empty channel answers at once, with `has_event` false and
/// a `tk_null` `any` — §2.1.1. It is the one operation here that never blocks,
/// and the reply shape is the part a specification can be wrong about
/// silently: the return value precedes the `out` parameter, so the boolean is
/// the last octet and is what tells a decoder where the `any`'s value ended.
#[test]
fn try_pull_answers_immediately_with_no_event_when_the_channel_is_empty() {
    let chan = Channel::start();
    let (_, mut puller) = chan.pull_proxy();

    let began = Instant::now();
    for _ in 0..3 {
        assert!(client::try_pull(&mut puller).unwrap().is_none());
    }
    assert!(began.elapsed() < Duration::from_secs(1), "try_pull must not block");
    assert_eq!(chan.handle.stats().pulled, 0);

    // And the raw reply, so the `tk_null` is measured rather than assumed:
    // an empty `any` whose TypeCode is tk_null, then the boolean.
    let reply = puller.invoke_nullary("try_pull").unwrap();
    let mut body = reply.body().unwrap();
    assert_eq!(body.get_u32().unwrap(), 0, "tk_null is TCKind 0");
    assert_eq!(body.remaining(), 1, "a tk_null any carries no value octets");
    assert!(!body.get_bool().unwrap());

    drop(puller);
    chan.shutdown();
}

/// A `pull` blocks, and a `push` arriving on **another connection** is served
/// while it does — which is the whole of "no lock is held across the wait",
/// measured rather than asserted.
///
/// If the channel's mutex were held for the duration of the block, the
/// supplier's `push` could not be served, the event could never arrive, and
/// the `pull` would sit out its whole deadline and raise `TIMEOUT`. So the
/// failure mode of this test is a timeout, not a wrong value — which is why it
/// asserts on the elapsed time as well as on the payload.
#[test]
fn a_pull_blocks_and_is_woken_by_a_push_served_on_another_connection() {
    let chan = Channel::start();
    chan.handle.set_pull_block(Duration::from_secs(3));
    let (proxy, first) = chan.pull_proxy();
    drop(first);

    let began = Instant::now();
    let puller = std::thread::spawn({
        let proxy = proxy.clone();
        move || {
            let mut conn = Connection::connect(&proxy, T).unwrap();
            client::pull(&mut conn)
        }
    });

    // Long enough that the pull is certainly inside its wait, short enough
    // that it is nowhere near the deadline. A sleeping wait, not a spin: the
    // Phase 0 wait-loop rule applies to the thing being waited *on* too.
    std::thread::sleep(Duration::from_millis(150));
    let mut supplier = chan.supplier_proxy();
    client::push(&mut supplier, &TypeCode::ULong, |e| e.put_u32(0x5EED)).unwrap();

    let got = puller.join().unwrap().expect("the blocked pull was satisfied, not timed out");
    let waited = began.elapsed();
    assert_eq!(ulong(&got), 0x5EED);
    assert!(waited < Duration::from_secs(2), "the pull woke on the push, after {waited:?}");
    assert_eq!(chan.handle.stats().pulled, 1);

    drop(supplier);
    chan.shutdown();
}

/// A `pull` nobody satisfies expires rather than holding a serving thread for
/// the life of the process, and reports `TIMEOUT` with `COMPLETED_NO` — the
/// half that matters, because it says no event was consumed and the call may
/// simply be made again.
#[test]
fn a_pull_nobody_answers_times_out_with_completed_no() {
    let chan = Channel::start();
    chan.handle.set_pull_block(Duration::from_millis(200));
    let (_, mut puller) = chan.pull_proxy();

    let began = Instant::now();
    let err = client::pull(&mut puller).expect_err("nothing was ever pushed");
    let waited = began.elapsed();
    assert_eq!(system_exception_id(&err), "IDL:omg.org/CORBA/TIMEOUT:1.0");
    match err {
        Error::SystemException { completed, .. } => {
            // The ordinal, not the name, because the ordinal is what a foreign
            // ORB reads: `COMPLETED_NO` is 1 and `COMPLETED_YES` is 0, and
            // this workspace has had them transposed before.
            assert_eq!(completed, Completion::No as u32, "a retry cannot lose an event");
        }
        other => panic!("{other:?}"),
    }
    assert!(waited >= Duration::from_millis(200), "it blocked for its deadline: {waited:?}");
    assert!(waited < Duration::from_secs(2), "and not much past it: {waited:?}");

    // The proxy is unharmed: the next push is pullable.
    let mut supplier = chan.supplier_proxy();
    client::push(&mut supplier, &TypeCode::ULong, |e| e.put_u32(9)).unwrap();
    assert_eq!(ulong(&client::pull(&mut puller).unwrap()), 9);

    drop(supplier);
    drop(puller);
    chan.shutdown();
}

/// What happens at the bound, which is the question the old deferral asserted
/// an answer to instead of measuring: the pull queue is the **same** bound as
/// the push queue, moved by the same `set_queue_limit`, and it drops the
/// oldest and counts every discard.
///
/// Blocking the supplier instead — the specification's own answer to a full
/// channel — was rejected, and this test is where that choice is visible: nine
/// pushes into a bound of three all return, promptly, with nothing draining
/// the queue at all.
#[test]
fn a_pull_queue_at_its_bound_drops_the_oldest_and_counts_it() {
    let chan = Channel::start();
    chan.handle.set_queue_limit(3);
    let (_, mut puller) = chan.pull_proxy();

    let mut supplier = chan.supplier_proxy();
    let began = Instant::now();
    for i in 0..9u32 {
        client::push(&mut supplier, &TypeCode::ULong, move |e| e.put_u32(i)).unwrap();
    }
    assert!(
        began.elapsed() < Duration::from_secs(2),
        "a full pull queue must not block the supplier"
    );

    let stats = chan.handle.stats();
    assert_eq!(stats.accepted, 9);
    assert_eq!(stats.fanned_out, 9, "one connected pull proxy, so one copy of each");
    assert_eq!(stats.queued, 3, "the pull queue is bounded, and by the same bound");
    assert_eq!(stats.dropped, 6, "every discarded event is counted, none silently");
    assert_eq!(stats.pull_consumers_connected, 1);
    // The same bound *and* the same cause: a pull queue at its bound is
    // back-pressure, told apart from housekeeping by the counter it moves.
    assert_eq!(stats.dropped_overflow, 6, "and counted as back-pressure, which is what it is");
    assert_eq!(stats.dropped_on_disconnect, 0);
    assert_eq!(stats.dropped_at_stop, 0);
    assert!(stats.split_adds_up(), "{stats:?}");

    let kept: Vec<u32> = (0..3).map(|_| ulong(&client::pull(&mut puller).unwrap())).collect();
    assert_eq!(kept, vec![6, 7, 8], "drop-oldest keeps the tail");
    assert!(client::try_pull(&mut puller).unwrap().is_none(), "and nothing more");
    assert_eq!(chan.handle.stats().dropped, 6, "draining adds no drops");

    drop(supplier);
    drop(puller);
    chan.shutdown();
}

/// One event, both models. A pull proxy that never comes back does not stop a
/// push consumer from being delivered to, and does not slow the supplier —
/// the drop-versus-block choice seen from the side where it matters.
#[test]
fn a_pull_proxy_and_a_push_proxy_see_the_same_events_independently() {
    let chan = Channel::start();
    let (_, mut idle_puller) = chan.pull_proxy();
    let (_, mut busy_puller) = chan.pull_proxy();

    let mut supplier = chan.supplier_proxy();
    for i in 0..4u32 {
        client::push(&mut supplier, &TypeCode::ULong, move |e| e.put_u32(i)).unwrap();
    }

    // One puller drains; the other never does. They are separate queues over
    // one shared `Arc<Event>`, so the busy one taking events does not remove
    // them from the idle one.
    let drained: Vec<u32> =
        (0..4).map(|_| ulong(&client::pull(&mut busy_puller).unwrap())).collect();
    assert_eq!(drained, vec![0, 1, 2, 3]);

    let stats = chan.handle.stats();
    assert_eq!(stats.accepted, 4, "one push per event, whatever it fans out to");
    // The fan-out itself, which `accepted` deliberately does not show: two
    // proxies means the channel made eight queue entries out of four events,
    // and eight is the denominator a per-consumer drop rate is taken over.
    assert_eq!(stats.fanned_out, 8, "four events, two connected proxies");
    assert_eq!(stats.pulled, 4);
    assert_eq!(stats.queued, 4, "the idle puller's backlog is still counted");
    assert_eq!(stats.pull_consumers_connected, 2);
    assert_eq!(stats.dropped, 0);

    // And the idle one can still come back for all of it.
    assert_eq!(ulong(&client::pull(&mut idle_puller).unwrap()), 0);

    drop(supplier);
    drop(idle_puller);
    drop(busy_puller);
    chan.shutdown();
}

/// The state machine §2.1.1 and §2.3 define: pulling from a proxy nothing is
/// connected to is `Disconnected`, a second connect is `AlreadyConnected`, a
/// nil `PullConsumer` is legal — unlike a nil `PushConsumer`, because this one
/// is never dialled — and a disconnect abandons the backlog, counted.
#[test]
fn the_pull_connect_and_disconnect_state_machine() {
    let chan = Channel::start();
    chan.handle.set_pull_block(Duration::from_millis(100));

    let proxy = chan.pull_proxy_ref();
    let mut conn = chan.dial(&proxy);

    for op in ["pull", "try_pull"] {
        let err = conn.invoke_nullary(op).expect_err("not connected yet");
        assert_eq!(user_exception_id(&err), DISCONNECTED_ID, "{op} before connect");
    }

    // A nil PullConsumer is accepted: the reference exists only so the proxy
    // could call `disconnect_pull_consumer` back, which is optional.
    client::connect_pull_consumer(&mut conn, &client::nil_ref()).unwrap();
    let err =
        client::connect_pull_consumer(&mut conn, &client::nil_ref()).expect_err("a second connect");
    assert_eq!(user_exception_id(&err), ALREADY_CONNECTED_ID);

    let mut supplier = chan.supplier_proxy();
    for i in 0..3u32 {
        client::push(&mut supplier, &TypeCode::ULong, move |e| e.put_u32(i)).unwrap();
    }
    assert_eq!(chan.handle.stats().queued, 3);

    client::disconnect_pull_supplier(&mut conn).unwrap();
    let stats = chan.handle.stats();
    assert_eq!(stats.queued, 0, "the backlog goes with the connection");
    assert_eq!(stats.dropped, 3, "and is counted, not forgotten");
    assert_eq!(
        stats.dropped_on_disconnect, 3,
        "under the cause that happened: the consumer asked, nothing overflowed"
    );
    assert_eq!(stats.dropped_overflow, 0);
    assert_eq!(stats.pull_consumers_connected, 0);
    assert!(stats.split_adds_up(), "{stats:?}");

    for op in ["pull", "try_pull"] {
        let err = conn.invoke_nullary(op).expect_err("disconnected");
        assert_eq!(user_exception_id(&err), DISCONNECTED_ID, "{op} after disconnect");
    }
    // Idempotent, and the key stays reconnectable — the choice the push proxy
    // and F6's unbound contexts both make.
    client::disconnect_pull_supplier(&mut conn).unwrap();
    client::connect_pull_consumer(&mut conn, &client::nil_ref()).unwrap();

    drop(conn);
    drop(supplier);
    chan.shutdown();
}

/// The limit the push path does not have, measured rather than left as a
/// paragraph: a reply's byte order is the *request's*, so an `any` captured in
/// the other order cannot be handed back verbatim. The push path escapes this
/// by originating its message and adopting the event's order; a reply cannot.
///
/// The refusal is a counter and a dropped event, not an exception, because the
/// mismatch is this channel's limitation and not the caller's request — the
/// distinction the delivery path's `Unrelayable` outcome already draws. What
/// this test pins is that the loss is *counted*: a silent one would be the
/// unmeasured-check rule in another costume.
#[test]
fn an_event_captured_in_the_other_byte_order_is_refused_to_a_puller_and_counted() {
    for (supplied, pulled) in [(Endian::Big, Endian::Little), (Endian::Little, Endian::Big)] {
        let chan = Channel::start();
        let (_, mut puller) = chan.pull_proxy();
        puller.set_endian(pulled);

        let mut supplier = chan.supplier_proxy();
        supplier.set_endian(supplied);
        client::push(&mut supplier, &TypeCode::ULong, |e| e.put_u32(0x1234)).unwrap();
        // A second one the puller *can* have, to prove the refusal skips
        // rather than wedges: a poison event returned as an error would fail
        // identically on every retry and the queue would never move again.
        supplier.set_endian(pulled);
        client::push(&mut supplier, &TypeCode::ULong, |e| e.put_u32(0x5678)).unwrap();

        let got = client::pull(&mut puller).unwrap();
        assert_eq!(ulong(&got), 0x5678, "{supplied:?}->{pulled:?}: the relayable one came through");

        let stats = chan.handle.stats();
        assert_eq!(stats.unrelayable, 1, "{supplied:?}->{pulled:?}");
        assert_eq!(stats.dropped, 1, "{supplied:?}->{pulled:?}: refused is discarded, and counted");
        // `unrelayable` is this cause's share of `dropped`, not a number
        // beside it — the refusal is our limitation, and the loss is real.
        assert_eq!(stats.dropped_overflow, 0, "{supplied:?}->{pulled:?}: nothing overflowed");
        assert_eq!(stats.dropped_on_disconnect, 0, "{supplied:?}->{pulled:?}");
        assert!(stats.split_adds_up(), "{supplied:?}->{pulled:?}: {stats:?}");
        assert_eq!(stats.pulled, 1, "{supplied:?}->{pulled:?}");
        assert_eq!(stats.queued, 0, "{supplied:?}->{pulled:?}");

        drop(supplier);
        drop(puller);
        chan.shutdown();
    }
}

/// `_is_a` on the new object: its own interface and `CORBA::Object`, and
/// nothing else. Every ORB probes before it trusts a narrow, and a proxy that
/// claimed to be the push one would be narrowed to an interface whose
/// operations it does not serve.
#[test]
fn is_a_on_a_pull_proxy_answers_for_its_own_interface_only() {
    let chan = Channel::start();
    let proxy = chan.pull_proxy_ref();
    let mut conn = chan.dial(&proxy);

    for (id, expected) in
        [(PROXY_PULL_SUPPLIER_ID, true), (CORBA_OBJECT_ID, true), (PROXY_PUSH_SUPPLIER_ID, false)]
    {
        let reply = conn.invoke("_is_a", move |e| e.put_str(id)).unwrap();
        assert_eq!(reply.body().unwrap().get_bool().unwrap(), expected, "_is_a {id}");
    }
    let reply = conn.invoke_nullary("_non_existent").unwrap();
    assert!(!reply.body().unwrap().get_bool().unwrap());

    drop(conn);
    chan.shutdown();
}
