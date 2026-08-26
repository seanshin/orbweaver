//! A client reaches an event channel holding a **name**, and never its IOR.
//!
//! D021 §3 settles registration as CosNaming — `CosEventChannelAdmin` declares
//! no factory, the one in the standard is CosNotification's and is deferred, so
//! the available standard route is a channel published under a name. D029 §6.1
//! calls that the **Location** row: *the caller must not be able to tell where
//! the target runs.*
//!
//! # This is a leak hunt, not a demo
//!
//! Priority zero says transparency is hunted rather than confirmed, and a demo
//! confirms. A client that resolves a name and receives an event has shown
//! that resolving *works*; it has shown nothing about whether the client knew
//! where the channel was, because a client that had been handed the IOR would
//! print the same line. **The two paths have to be told apart by an experiment
//! that only one of them survives**, and there is exactly one: move the
//! channel.
//!
//! So the shape here is
//!
//! 1. publish, resolve, connect, receive — and record what the client observed;
//! 2. **stop the channel's server and start it again at a different address**,
//!    with the same channel name and the same object keys, and re-publish;
//! 3. resolve, connect, receive again — and assert the observations are
//!    *identical*, while the address the test can see differs.
//!
//! and the negative control is the leak itself: the same client handed the IOR
//! it resolved in step 1 cannot do step 3. If that control passed, the address
//! would not have moved and steps 1 and 3 would be measuring nothing.
//!
//! *투명성은 확인이 아니라 사냥이다. IOR을 건네받은 클라이언트도 똑같은 줄을
//! 출력하므로, 두 경로는 **한쪽만 살아남는 실험**으로만 구분된다 — 채널을
//! 옮기는 것.*
//!
//! # What the client is allowed to hold
//!
//! An [`Orb`] whose initial-references table the deployment filled in, and two
//! strings: `"corbaloc:rir:NameService"` and the channel's name. That is
//! D021 §3's third bullet — `resolve_initial_references("NameService")` — and
//! it is why [`reach_by_name`] takes `&Orb` and `&str` and **no [`Ior`] of the
//! channel**. The signature is the claim: a parameter that is not there cannot
//! be smuggled in.
//!
//! # What is *not* claimed here
//!
//! That the channel survived. It did not — step 2 stops a server and starts a
//! different one, which is a redeployment and not a migration, because the ORB
//! cannot stop what it handed out (D029 §3.1) and a channel carries no state
//! across it. What is claimed is narrower and is the Location row exactly: the
//! **client** could not tell, and needed no new information to keep working.
//!
//! And one limit that is easy to read past, so it is stated as a limit rather
//! than left to be inferred from the code: step 3 **re-runs the whole
//! bootstrap**. It therefore measures that a *new* client is unaffected by the
//! move, and measures **nothing** about an already-attached consumer surviving
//! one — that consumer is dropped, and the client finds out by failing. A test
//! that asserted the existing proxy kept working would be asserting something
//! this ORB does not do. The gap is named in D029 §6.1's Location subsection
//! as the next thing to close; naming it is worth more than a green row.
//!
//! *3단계는 부트스트랩을 처음부터 다시 한다. 따라서 **새** 클라이언트가 영향을
//! 받지 않음을 재고, 이미 붙어 있던 소비자의 생존은 재지 않는다 — 그 소비자는
//! 끊기고, 클라이언트는 실패로 알게 된다.*

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use orbweaver_cdr::Endian;
use orbweaver_giop::event_server::{
    CHANNEL_BINDING_KIND, Delivery, EventChannelServer, EventSink, PushConsumerServant,
    channel_binding_name, client, publish_channels,
};
use orbweaver_giop::guarded::{Section, complaints_about};
use orbweaver_giop::naming::{
    NameComponent, NamingContext, parse_stringified_name, stringify_name,
};
use orbweaver_giop::naming_server::NamingServer;
use orbweaver_giop::orb::Orb;
use orbweaver_giop::typecode::{Any, TypeCode};
use orbweaver_giop::{Connection, Ior, Result};

const T: Duration = Duration::from_secs(5);
const OUTBOUND_T: Duration = Duration::from_millis(500);
const NS_KEY: &[u8] = b"NameService";
const CHANNEL: &str = "alerts";
/// The one string a client is given besides the channel name. `rir:` addresses
/// nothing dialable — it is a lookup in the ORB's own table — which is what
/// makes it the right bootstrap to test: it carries no address at all.
const NS_URL: &str = "corbaloc:rir:NameService";

// ─────────────────────────────────────────────────────────────────────────────
// The servers, each stoppable by the test that started it
// ─────────────────────────────────────────────────────────────────────────────

/// A naming server on loopback.
///
/// Stopped by a flag this test holds, which is worth saying out loud: D029
/// §3.1's gap is that **the ORB** cannot stop a `Server` it handed out. The
/// direct caller of `serve_shared` can, because it owns the stop closure. This
/// test is that caller, which is the only reason step 2 is reachable at all.
struct Naming {
    ior: Ior,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Naming {
    fn start() -> Self {
        let server = Orb::new().server("127.0.0.1:0", NS_KEY.to_vec()).unwrap();
        let port = server.local_addr().unwrap().port();
        let servant = Arc::new(NamingServer::new("127.0.0.1", port, NS_KEY.to_vec()));
        let ior = servant.root_ior();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            server.serve_shared(&*servant, move || flag.load(Ordering::SeqCst)).unwrap();
        });
        Naming { ior, stop, thread: Some(thread) }
    }

    /// A connected context, for the *deployer* to publish through.
    fn context(&self) -> NamingContext {
        NamingContext::connect(&self.ior, T).unwrap()
    }

    /// An ORB configured the way a deployment would configure a client's:
    /// `NameService` in the initial-references table and nothing else.
    fn client_orb(&self) -> Orb {
        let mut orb = Orb::new();
        orb.register_initial_reference("NameService", self.ior.clone()).unwrap();
        orb
    }

    fn shutdown(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.thread.take().unwrap().join();
    }
}

/// An event-channel server on loopback, serving one channel called [`CHANNEL`].
///
/// The channel created with the server *is* [`CHANNEL`] — `EventChannelServer::new`
/// names it after the base key — so the base key is the channel name, and the
/// object keys are therefore identical between the two incarnations in step 2.
/// That is deliberate: if the keys differed, the client's observations would
/// differ for a reason that has nothing to do with location.
struct Channel {
    servant: Arc<EventChannelServer>,
    port: u16,
    delivery: Option<Delivery>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Channel {
    fn start() -> Self {
        let server = Orb::new().server("127.0.0.1:0", CHANNEL.as_bytes().to_vec()).unwrap();
        let port = server.local_addr().unwrap().port();
        let servant =
            Arc::new(EventChannelServer::new("127.0.0.1", port, CHANNEL.as_bytes().to_vec()));
        let delivery = servant.start_delivery_with(OUTBOUND_T);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let serving = Arc::clone(&servant);
        let thread = std::thread::spawn(move || {
            server.serve_shared(&*serving, move || flag.load(Ordering::SeqCst)).unwrap();
        });
        Channel { servant, port, delivery: Some(delivery), stop, thread: Some(thread) }
    }

    /// Pushes one `unsigned long` in as a supplier would, over the wire.
    fn push(&self, ior: &Ior, value: u32, endian: Endian) {
        let mut conn = dial(ior, endian);
        let admin = client::for_suppliers(&mut conn).unwrap();
        drop(conn);
        let mut conn = dial(&admin, endian);
        let proxy = client::obtain_push_consumer(&mut conn).unwrap();
        drop(conn);
        let mut conn = dial(&proxy, endian);
        client::connect_push_supplier(&mut conn, &client::nil_ref()).unwrap();
        client::push(&mut conn, &TypeCode::ULong, |e| e.put_u32(value)).unwrap();
    }

    fn shutdown(mut self) {
        drop(self.delivery.take());
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.thread.take().unwrap().join();
    }
}

/// A collecting `PushConsumer` on its own loopback server — the client's ear.
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

fn dial(ior: &Ior, endian: Endian) -> Connection {
    let mut conn = Connection::connect(ior, T).unwrap();
    conn.set_endian(endian);
    conn
}

// ─────────────────────────────────────────────────────────────────────────────
// The client under test, and what it is allowed to hold
// ─────────────────────────────────────────────────────────────────────────────

/// The client. Given an ORB, a URL that carries no address, a channel name and
/// its own consumer — **and no channel reference**, which this signature
/// enforces rather than a comment asking nicely.
///
/// Returns the object key of the channel it reached, which is the only thing
/// about the target it can see and is part of the reference rather than of the
/// address. See [`Observed`].
fn reach_by_name(orb: &Orb, channel: &str, consumer: &Ior, endian: Endian) -> Result<Vec<u8>> {
    let ns = orb.string_to_object(NS_URL).expect("the deployment registered NameService");
    let mut ctx = NamingContext::connect(&ns, T)?;
    ctx.connection().set_endian(endian);
    let found = ctx.resolve(&channel_binding_name(channel))?;
    attach(&found, consumer, endian)
}

/// The half both paths share, so the only difference between them is where the
/// reference came from.
fn attach(channel: &Ior, consumer: &Ior, endian: Endian) -> Result<Vec<u8>> {
    let key = channel.primary().expect("an IIOP profile").object_key.clone();
    let mut conn = dial_checked(channel, endian)?;
    let admin = client::for_consumers(&mut conn)?;
    drop(conn);
    let mut conn = dial_checked(&admin, endian)?;
    let proxy = client::obtain_push_supplier(&mut conn)?;
    drop(conn);
    let mut conn = dial_checked(&proxy, endian)?;
    client::connect_push_consumer(&mut conn, consumer)?;
    Ok(key)
}

/// As [`dial`], but surfacing the failure instead of unwrapping — the control
/// path is *expected* to fail, and a panic is not a measurement.
fn dial_checked(ior: &Ior, endian: Endian) -> Result<Connection> {
    let mut conn = Connection::connect(ior, T)?;
    conn.set_endian(endian);
    Ok(conn)
}

/// Everything the client under test can see.
///
/// **The address is not a field.** That is the whole design of this struct: if
/// it were here, comparing two `Observed`s across a move would fail for the
/// right reason by accident, and the test would be asserting "the address
/// changed" rather than "nothing the client sees changed". The test reads the
/// address separately, out of band, because *the test* is allowed to know
/// where the channel is and the client is not.
#[derive(Debug, PartialEq, Eq)]
struct Observed {
    /// The events that reached the consumer in this phase.
    events: Vec<u32>,
    /// The object key of the channel it reached. Part of the reference and not
    /// of the address, so it must be **unchanged** across a move — a key that
    /// moved would mean the client was reaching a different object and the
    /// event it received would be evidence of nothing.
    channel_key: Vec<u8>,
}

fn ulong(any: &Any) -> u32 {
    assert_eq!(any.tc, TypeCode::ULong);
    any.value_decoder().get_u32().unwrap()
}

/// The events a sink received after `baseline`.
///
/// A baseline and not a `clear()`: the sink has no clear, and inventing one for
/// a test would put a mutation on a servant's collecting end that only a test
/// wants. Slicing is also the more honest instrument — it cannot hide an event
/// that arrived late from the previous phase, it counts it.
fn since(sink: &EventSink, baseline: usize) -> Vec<u32> {
    sink.snapshot()[baseline..].iter().map(ulong).collect()
}

/// The port a reference points at. The test may look; the client may not.
fn port_of(ior: &Ior) -> u16 {
    ior.primary().expect("an IIOP profile").port
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. The mapping — the decision, without a wire under it
// ─────────────────────────────────────────────────────────────────────────────

/// The mapping decision, asserted where a reader will find it failing.
///
/// This is the sentence `CHANNEL_BINDING_KIND`'s documentation states, in the
/// form that goes red if the code stops meaning it.
#[test]
fn a_channel_is_one_component_whose_id_is_its_name_and_whose_kind_is_constant() {
    assert_eq!(
        channel_binding_name("alerts"),
        vec![NameComponent { id: "alerts".into(), kind: CHANNEL_BINDING_KIND.into() }]
    );
    assert_eq!(CHANNEL_BINDING_KIND, "EventChannel");
}

/// Injectivity, on the hazard the naming rule leaves open.
///
/// `is_channel_name_safe` forbids `/` and the minted segments; it has no
/// reason to forbid `.`, which is the id/kind separator in the **stringified**
/// name. So a channel whose name contains a `.` must survive the round trip,
/// and must not collide with the name a client would get by concatenating the
/// id and the kind itself. This is the case that would have made the mapping
/// wrong if the name had been built as a string.
#[test]
fn a_dot_in_a_channel_name_survives_the_stringified_form() {
    for name in ["a.b", "a", "b", "a.b.c", "plain"] {
        let structured = channel_binding_name(name);
        let round = parse_stringified_name(&stringify_name(&structured)).unwrap();
        assert_eq!(round, structured, "{name} did not survive the stringified form");
    }
    assert_ne!(channel_binding_name("a.b"), channel_binding_name("a"));
    assert_ne!(
        stringify_name(&channel_binding_name("a.b")),
        stringify_name(&[NameComponent { id: "a".into(), kind: "b".into() }])
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. The lock discipline — publication is the deployer's call, off the lock path
// ─────────────────────────────────────────────────────────────────────────────

/// `publish_channels` dials a peer, so the rule this module has documented
/// since the concurrency batch applies to it: **no lock may be held across an
/// outbound call.** It is kept structurally — the registry lock is dropped
/// before the loop that binds — and this is that claim as a measurement.
///
/// The negative control is in the same test and is the reason it is
/// trustworthy: with a section open, the same call complains. Without the
/// control, an empty complaint list would be satisfied just as well by
/// `assert_nothing_held` having no body, which is a defect this repository has
/// already had once.
#[test]
fn publishing_holds_no_lock_and_the_control_says_the_tripwire_is_live() {
    let naming = Naming::start();
    let channel = Channel::start();

    let quiet = complaints_about(|| {
        let mut ctx = naming.context();
        publish_channels(&channel.servant, &mut ctx).unwrap();
    });
    assert!(quiet.is_empty(), "publication complained with no lock held: {quiet:?}");

    // ── negative control: the identical call with a section open ──
    let loud = complaints_about(|| {
        let _held = Section::enter("a pretend channel lock");
        let mut ctx = naming.context();
        let _ = publish_channels(&channel.servant, &mut ctx);
    });
    // Emptiness, never a count: `guarded` stops at the first complaint in a
    // debug build and carries on in a release one, so the number of complaints
    // is a property of the profile and only their presence is a property of
    // the code.
    assert!(
        !loud.is_empty(),
        "the control did not move: a section was open and nothing complained, \
         so the empty list above is evidence of nothing"
    );

    channel.shutdown();
    naming.shutdown();
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. The leak hunt
// ─────────────────────────────────────────────────────────────────────────────

/// The Location row for channels, measured: a client that holds a name keeps
/// working when the channel moves, and one that holds the IOR does not.
///
/// Both halves are in one test on purpose. They are not two properties — they
/// are a claim and its control, and separating them would let the control be
/// skipped, deleted or left failing while the claim went on reporting green.
#[test]
fn a_client_holding_a_name_survives_the_channel_moving_and_one_holding_an_ior_does_not() {
    let e = Endian::native();
    let naming = Naming::start();
    let consumer = Consumer::start(b"ear");
    let orb = naming.client_orb();

    // ── step 1: publish, resolve, connect, receive ──
    let first = Channel::start();
    let published = {
        let mut ctx = naming.context();
        publish_channels(&first.servant, &mut ctx).unwrap()
    };
    assert_eq!(published.len(), 1, "one channel, one binding");
    assert_eq!(published[0].channel, CHANNEL);
    assert_eq!(published[0].name, channel_binding_name(CHANNEL));

    let base = consumer.sink.snapshot().len();
    let key = reach_by_name(&orb, CHANNEL, &consumer.ior, e).unwrap();
    first.push(&published[0].ior, 7, e);
    assert!(consumer.sink.wait_for(base + 1, T), "the first event never arrived");
    let before = Observed { events: since(&consumer.sink, base), channel_key: key };
    assert_eq!(before.events, vec![7]);

    // What the *test* is allowed to know, and the reference the leaky client
    // would have kept.
    let first_port = port_of(&published[0].ior);
    let handed = published[0].ior.clone();

    // ── step 2: the channel moves ──
    first.shutdown();
    let second = Channel::start();
    assert_ne!(
        second.port, first_port,
        "the channel did not actually move: the OS reissued the port, so nothing below \
         distinguishes the two paths and this run measures nothing"
    );
    let republished = {
        let mut ctx = naming.context();
        publish_channels(&second.servant, &mut ctx).unwrap()
    };
    assert_eq!(republished[0].name, published[0].name, "the name is what did not change");
    assert_ne!(port_of(&republished[0].ior), first_port, "the address is what did");

    // ── step 3: the same client, given nothing new ──
    let base = consumer.sink.snapshot().len();
    let key = reach_by_name(&orb, CHANNEL, &consumer.ior, e).unwrap();
    second.push(&republished[0].ior, 7, e);
    assert!(consumer.sink.wait_for(base + 1, T), "the event after the move never arrived");
    let after = Observed { events: since(&consumer.sink, base), channel_key: key };

    assert_eq!(
        before, after,
        "the client's observations changed across the move, which is the Location leak"
    );

    // ── the control: the leak. The same client, handed the reference. ──
    let leaked = attach(&handed, &consumer.ior, e);
    assert!(
        leaked.is_err(),
        "the negative control did not move: a client holding the pre-move IOR still \
         reached the channel, so the assertion above is not about location at all"
    );

    second.shutdown();
    consumer.shutdown();
    naming.shutdown();
}

/// The name is load-bearing, not decorative: a name that was never published
/// does not resolve.
///
/// Without this, a `resolve` that answered *anything* for *any* name would
/// satisfy the test above, because the reference it hands back is the one the
/// client then dials.
#[test]
fn a_name_that_was_never_published_does_not_resolve() {
    let naming = Naming::start();
    let channel = Channel::start();
    {
        let mut ctx = naming.context();
        publish_channels(&channel.servant, &mut ctx).unwrap();
    }
    let mut ctx = naming.context();
    assert!(ctx.resolve(&channel_binding_name("no-such-channel")).is_err());
    // …and the kind is part of the name, not ornament: the same id with the
    // habitual empty kind is a different name and is not bound.
    assert!(ctx.resolve(&[NameComponent::new(CHANNEL)]).is_err());
    // The one that was published still resolves, so the two failures above are
    // about the names and not about the context being broken.
    assert!(ctx.resolve(&channel_binding_name(CHANNEL)).is_ok());
    drop(ctx);

    channel.shutdown();
    naming.shutdown();
}

/// Publication is idempotent, which is what `rebind` buys and what a moved
/// channel needs. Running it twice is not an error and leaves one binding.
#[test]
fn publishing_twice_is_not_an_error() {
    let naming = Naming::start();
    let channel = Channel::start();
    let mut ctx = naming.context();
    publish_channels(&channel.servant, &mut ctx).unwrap();
    publish_channels(&channel.servant, &mut ctx).unwrap();
    let (bindings, _) = ctx.list(64).unwrap();
    assert_eq!(bindings.len(), 1, "rebind left more than one binding: {bindings:?}");
    drop(ctx);

    channel.shutdown();
    naming.shutdown();
}

/// Several channels, several bindings — E2's shape reaching E3's.
#[test]
fn every_channel_of_a_server_gets_its_own_binding() {
    let naming = Naming::start();
    let channel = Channel::start();
    channel.servant.create_channel("beta").unwrap();
    channel.servant.create_channel("gamma").unwrap();

    let mut ctx = naming.context();
    let published = publish_channels(&channel.servant, &mut ctx).unwrap();
    assert_eq!(published.len(), 3);

    let mut names: Vec<&str> = published.iter().map(|p| p.channel.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, [CHANNEL, "beta", "gamma"]);

    // Every binding resolves, and to a *different* object — the injectivity the
    // mapping's documentation argues for, measured on the wire.
    let mut keys: Vec<Vec<u8>> = Vec::new();
    for p in &published {
        let found = ctx.resolve(&channel_binding_name(&p.channel)).unwrap();
        assert_eq!(found, p.ior);
        keys.push(found.primary().unwrap().object_key.clone());
    }
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), 3, "two channels resolved to one object key");
    drop(ctx);

    channel.shutdown();
    naming.shutdown();
}

/// Both byte orders, on the client's own encoder.
///
/// An encoder that only works native-endian passes every local test and fails
/// in the field, and this path has **two** of them: the `resolve` that carries
/// a `Name` and reads an IOR back, and the three channel calls that carry and
/// return references. Both are set from the same knob here, so a run covers
/// the naming leg and the event leg together.
///
/// This does not vary the *server's* reply order — a server replies in the
/// order it was asked in, which is what makes setting the client's enough.
#[test]
fn resolving_and_receiving_work_in_both_byte_orders() {
    for endian in [Endian::Little, Endian::Big] {
        let naming = Naming::start();
        let consumer = Consumer::start(b"ear");
        let channel = Channel::start();

        let published = {
            let mut ctx = naming.context();
            publish_channels(&channel.servant, &mut ctx).unwrap()
        };
        let orb = naming.client_orb();
        let base = consumer.sink.snapshot().len();
        let key = reach_by_name(&orb, CHANNEL, &consumer.ior, endian).unwrap();
        assert_eq!(key, CHANNEL.as_bytes(), "{endian:?}: reached the wrong object");
        channel.push(&published[0].ior, 11, endian);
        assert!(consumer.sink.wait_for(base + 1, T), "no event under {endian:?}");
        assert_eq!(since(&consumer.sink, base), vec![11]);

        channel.shutdown();
        consumer.shutdown();
        naming.shutdown();
    }
}
