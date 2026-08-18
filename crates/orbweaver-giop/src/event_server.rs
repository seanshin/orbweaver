//! First-party CosEvent channel: `CosEventChannelAdmin` plus the
//! `CosEventComm` push pair, and — since the deferral below was re-measured —
//! the **consumer side of the pull model** (PLAN-SERVICES §4).
//!
//! The oracle direction was settled by measurement, not preference:
//! `brew info omnievents` reports *"No available formula"*, so no reference
//! event channel is installable as a fixture — but omniORBpy ships the
//! `CosEventComm` stubs. So the F6 arrangement transfers: **we serve the
//! channel, and an ORB we did not write attaches to it as a consumer.** The
//! attach recipe lives in `spike-events`' module docs.
//!
//! # What is served
//!
//! | object | operations |
//! |---|---|
//! | `EventChannel` | `for_consumers`, `for_suppliers` |
//! | `ConsumerAdmin` | `obtain_push_supplier` |
//! | `SupplierAdmin` | `obtain_push_consumer` |
//! | `ProxyPushSupplier` | `connect_push_consumer`, `disconnect_push_supplier` |
//! | `ProxyPushConsumer` | `connect_push_supplier`, `push`, `disconnect_push_consumer` |
//! | `ProxyPullSupplier` | `connect_pull_consumer`, `pull`, `try_pull`, `disconnect_pull_supplier` |
//!
//! plus `_is_a`/`_non_existent` on every one of them, because every ORB probes
//! before it trusts a narrow. Each admin and each proxy is a **distinct object
//! key on this one servant**, exactly as F6's nested contexts are — see
//! [`Dispatch::knows`], which is answered from the proxy tables.
//!
//! # What is refused, and why
//!
//! - **The *supplier* side of the pull model** — `obtain_pull_consumer`,
//!   `connect_pull_supplier`, `disconnect_pull_consumer` — answers
//!   `NO_IMPLEMENT`. This is the direction in which the **channel** is the
//!   puller: a `ProxyPullConsumer` exists to invoke `pull` on a supplier's own
//!   reference, and `CosEventComm::PullSupplier::pull` is specified to block
//!   until that supplier has something to give. Every outbound call this
//!   module makes today is bounded by [`DEFAULT_PUSH_TIMEOUT`] and is expected
//!   to return; a blocking `pull` is expected *not* to, so the channel would
//!   hold a thread per connected supplier on somebody else's clock, with no
//!   bound it owns. Polling `try_pull` instead bounds the call but makes the
//!   channel invent an interval — latency traded against wasted invocations —
//!   **for no named supplier**: nothing in this workspace is a `PullSupplier`,
//!   F3 and F4 publish in-process, and remote suppliers push.
//!
//!   The clause this bullet used to open with did not survive being measured.
//!   "The same unbounded buffer this module spends its bounded queue avoiding"
//!   described a design nobody had to choose: a `ProxyPullSupplier` holds
//!   events in the same [`DEFAULT_QUEUE_LIMIT`] deque a `ProxyPushSupplier`
//!   already holds them in, discards the same oldest event, and counts it in
//!   the same [`ChannelStats::dropped`]. Only the second clause — *for no
//!   named consumer* — was load-bearing, and it is false on the consumer side
//!   and still true on the supplier side, which is why the two halves parted.
//! - **`EventChannel::destroy`.** Still refused, and the reason has changed.
//!   Half of the old one is answered: [`crate::guarded`] now catches a lock
//!   held across a callback structurally rather than by care, and the delivery
//!   thread is already the place this module makes outbound calls with nothing
//!   held — `destroy` could enqueue the `disconnect_push_consumer` callbacks
//!   there and return, and the failures would have somewhere to go. What is
//!   not answered got sharper instead of softer. `destroy` is an
//!   **unauthenticated remote operation that ends the channel for every other
//!   client**, and this servant has no notion of who is calling; the object
//!   keys it would invalidate are bound into a [`crate::server::Server`] it
//!   does not own, so a destroyed channel cannot be recreated without
//!   restarting the process. [`ChannelHandle::stop`] already gives in-process
//!   shutdown to the one caller with a reason to want it. No caller, and a
//!   remote footgun with no authorization story: the F6 `destroy` precedent
//!   stands.
//! - **Typed channels** (`CosTypedEventChannelAdmin`). Out of scope entirely;
//!   events are `any`.
//!
//! # Pulling: the two questions a queue drained from the far end asks
//!
//! A `ProxyPullSupplier` is a queue this channel fills and a consumer empties,
//! so it has to answer two things the push path answers elsewhere.
//!
//! 1. **At the bound it drops the oldest; it never blocks a supplier.** The
//!    bound is [`DEFAULT_QUEUE_LIMIT`], shared with the push proxies and moved
//!    by the same [`ChannelHandle::set_queue_limit`], and an overflow is
//!    counted in the same [`ChannelStats::dropped`] and logged per event. The
//!    specification's own answer to a full channel — block the supplier — was
//!    considered and rejected here for the reason rule 2 of the fan-out
//!    section already gives: a supplier blocked by one slow puller is one
//!    consumer wedging the channel for every other consumer, which is the
//!    failure this module is built around avoiding. `pull` blocks the *caller*
//!    that asked to wait, which is the one thread that consented to it.
//! 2. **At the reply it can refuse, and it says so in a counter.** The push
//!    path relays an `any`'s value bytes verbatim by adopting the event's byte
//!    order on the outbound connection — it originates that message, so it may
//!    choose. A reply may not: its byte order is the request's, chosen by the
//!    peer and framed by [`crate::server::Server`]. So an event captured in one
//!    byte order cannot be handed to a puller asking in the other, exactly as
//!    it cannot be relayed to a landing offset of a different alignment. Both
//!    are one predicate, [`relay_check`], used by the delivery thread and by
//!    `pull` alike; a refusal counts in [`ChannelStats::unrelayable`], the
//!    event is discarded and counted in [`ChannelStats::dropped`], and the
//!    puller is handed the next event rather than an exception — the channel's
//!    limitation is not the caller's fault, the same distinction
//!    [`ChannelState::record`] already draws on the push side.
//!
//! `pull` blocks, per `CosEventComm`, but bounded by [`DEFAULT_PULL_BLOCK`]
//! and woken early by an arriving event, by a disconnect and by
//! [`ChannelHandle::stop`]. On expiry it raises `TIMEOUT` with
//! `COMPLETED_NO` — nothing was consumed, so a client that calls `pull` again
//! has the unbounded block the specification describes, while a client that
//! has gone away stops costing a serving thread. Note the shape this needs:
//! under [`crate::server::Server::serve_shared`] a blocked `pull` occupies one
//! connection's thread and no more, but under the serialized
//! [`crate::server::Server::serve`] it occupies the only one, and the `push`
//! that would satisfy it cannot be served until the block expires. Serve a
//! channel with pull consumers on the shared path.
//!
//! # Fan-out: the part that is not hand-waved
//!
//! Delivering to a connected consumer means **this server acts as a client** —
//! it invokes `push` on the consumer's own reference. Three consequences, each
//! of which is a rule here:
//!
//! 1. **No lock may be held across an outbound call.** The consumer we are
//!    pushing to is free to call back into this channel (a supplier and a
//!    consumer in one process is the normal shape for a relay). If the
//!    delivery thread held the state lock while blocked in `push`, that
//!    re-entrant request would block the serving thread on the same lock while
//!    the consumer waits for our reply — a deadlock across two processes with
//!    no timeout to break it. So the delivery loop locks, *copies out* the one
//!    job it will attempt, unlocks, invokes, and re-locks only to record the
//!    outcome. [`ChannelState::take_next`] and [`ChannelState::record`] are
//!    the two halves; nothing between them touches the mutex.
//!
//!    **This rule is now enforced rather than observed.** Concurrent dispatch
//!    (stream E) makes it easier to violate, not harder: the servant side is
//!    no longer single-file behind the server's mutex, so a `push` arriving
//!    while a delivery is in flight is the ordinary case rather than the rare
//!    one. [`Shared::lock`] therefore returns a guard that registers with
//!    [`crate::guarded`], and every outbound `connect`/`invoke` refuses to
//!    block while one is open — in a debug build, by panicking in the test
//!    that did it.
//! 2. **A dead or slow consumer must not wedge the channel.** Each proxy has
//!    its own bounded queue ([`DEFAULT_QUEUE_LIMIT`]); on overflow the
//!    **oldest** event is dropped, counted in [`ChannelStats::dropped`] and
//!    logged. Never silently — the harness rule about unmeasured checks
//!    applies to discarded data too. Slowness is bounded by the push timeout,
//!    which is the socket read timeout on the outbound connection.
//! 3. **Repeated failure disconnects.** After
//!    [`MAX_CONSECUTIVE_FAILURES`] consecutive failed pushes the proxy is
//!    disconnected as though `disconnect_push_supplier` had been called: its
//!    consumer reference is released, its queued events are dropped (counted),
//!    and a line is logged. Three, not one: a single failure is a transport
//!    hiccup, and a consumer that has restarted deserves the two retries a
//!    fresh connect gets. The proxy object key stays alive and reconnectable,
//!    the same choice F6 made for unbound contexts.
//!
//! # Relaying an `any` verbatim
//!
//! An event's value bytes are captured raw and relayed raw. That is only sound
//! when the destination offset has the same alignment as the source (see
//! [`crate::typecode::encode_any_at_same_alignment`]) and the destination is
//! written in the same byte order, so delivery does both explicitly: the
//! outbound connection is switched to the *event's* byte order, and the
//! outbound body alignment is **measured** through the real request encoder
//! rather than recomputed here. A mismatch that cannot be honoured is refused
//! and counted in [`ChannelStats::unrelayable`] — re-marshalling an arbitrary
//! `any` means walking its `TypeCode`, which is `orbweaver-dynamic`'s job, not
//! this module's.
//!
//! # In-process publishing
//!
//! F3's residency transitions and F4's telemetry batches are produced *inside*
//! this process. Making them marshal through a loopback socket to reach a
//! channel in the same address space would be a cost paid for nothing, so
//! [`ChannelHandle::publish`] enqueues directly, with the value marshalled at
//! the alignment the relay expects.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use orbweaver_cdr::{Encoder, Endian};

use crate::guarded::{Guarded, Section};
use crate::server::{Dispatch, DispatchBody, Request, SharedDispatch, SystemException};
use crate::typecode::{self, Any, TypeCode};
use crate::{Connection, IiopProfile, Ior, Result, Version, codeset};

// ─────────────────────────────────────────────────────────────────────────────
// Repository ids
// ─────────────────────────────────────────────────────────────────────────────

/// Repository id of `CosEventChannelAdmin::EventChannel`.
pub const EVENT_CHANNEL_ID: &str = "IDL:omg.org/CosEventChannelAdmin/EventChannel:1.0";
/// Repository id of `CosEventChannelAdmin::ConsumerAdmin`.
pub const CONSUMER_ADMIN_ID: &str = "IDL:omg.org/CosEventChannelAdmin/ConsumerAdmin:1.0";
/// Repository id of `CosEventChannelAdmin::SupplierAdmin`.
pub const SUPPLIER_ADMIN_ID: &str = "IDL:omg.org/CosEventChannelAdmin/SupplierAdmin:1.0";
/// Repository id of `CosEventChannelAdmin::ProxyPushSupplier` — the object a
/// *consumer* connects itself to.
pub const PROXY_PUSH_SUPPLIER_ID: &str = "IDL:omg.org/CosEventChannelAdmin/ProxyPushSupplier:1.0";
/// Repository id of `CosEventChannelAdmin::ProxyPushConsumer` — the object a
/// *supplier* pushes into.
pub const PROXY_PUSH_CONSUMER_ID: &str = "IDL:omg.org/CosEventChannelAdmin/ProxyPushConsumer:1.0";
/// Repository id of `CosEventComm::PushConsumer`, the reference a consumer
/// hands us and the interface [`PushConsumerServant`] implements.
pub const PUSH_CONSUMER_ID: &str = "IDL:omg.org/CosEventComm/PushConsumer:1.0";
/// Repository id of `CosEventComm::PushSupplier`.
pub const PUSH_SUPPLIER_ID: &str = "IDL:omg.org/CosEventComm/PushSupplier:1.0";
/// Repository id of `CosEventChannelAdmin::ProxyPullSupplier` — the object a
/// *consumer* pulls out of.
pub const PROXY_PULL_SUPPLIER_ID: &str = "IDL:omg.org/CosEventChannelAdmin/ProxyPullSupplier:1.0";
/// Repository id of `CosEventComm::PullConsumer`, the reference a pulling
/// consumer may hand us. It is never dialled — see `connect_pull_consumer` —
/// so unlike a `PushConsumer` a nil one is legal.
pub const PULL_CONSUMER_ID: &str = "IDL:omg.org/CosEventComm/PullConsumer:1.0";
/// Repository id of `CORBA::Object`, which `_is_a` answers true for everywhere.
pub const CORBA_OBJECT_ID: &str = "IDL:omg.org/CORBA/Object:1.0";

/// Repository id of `CosEventChannelAdmin::AlreadyConnected`.
pub const ALREADY_CONNECTED_ID: &str = "IDL:omg.org/CosEventChannelAdmin/AlreadyConnected:1.0";
/// Repository id of `CosEventChannelAdmin::TypeError`.
pub const TYPE_ERROR_ID: &str = "IDL:omg.org/CosEventChannelAdmin/TypeError:1.0";
/// Repository id of `CosEventComm::Disconnected`.
pub const DISCONNECTED_ID: &str = "IDL:omg.org/CosEventComm/Disconnected:1.0";

// ─────────────────────────────────────────────────────────────────────────────
// Policy constants
// ─────────────────────────────────────────────────────────────────────────────

/// Events buffered per connected consumer before the oldest is dropped.
///
/// Control-plane granularity (PLAN-SERVICES §4: never per token) means a
/// healthy consumer is never near this; a consumer that is, is already behind.
pub const DEFAULT_QUEUE_LIMIT: usize = 64;

/// Consecutive failed pushes after which a proxy disconnects its consumer.
///
/// One failure is a hiccup and two cover a consumer that restarted between
/// them, since every attempt after a failure redials. Three consecutive
/// failures is a consumer that is not coming back on this reference.
pub const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// Connect and reply timeout for an outbound `push`.
///
/// This is what bounds a *slow* consumer: the delivery thread can be held for
/// at most this long by any one consumer, and the servant thread is never held
/// at all.
pub const DEFAULT_PUSH_TIMEOUT: Duration = Duration::from_secs(2);

/// How long a `pull` blocks before raising `TIMEOUT`.
///
/// `CosEventComm::pull` blocks until an event is available, with no bound at
/// all. An unbounded block is a serving thread a vanished client can hold for
/// the life of the process, so the block is bounded and the expiry is reported
/// as `TIMEOUT`/`COMPLETED_NO`: nothing was consumed, so a client that calls
/// `pull` again is indistinguishable from one that blocked the whole time,
/// and a client that does not call again stops costing anything.
pub const DEFAULT_PULL_BLOCK: Duration = Duration::from_secs(5);

/// How long a blocked `pull` waits between re-checks of its own proxy.
///
/// Every state change that matters notifies `wake`, so this is insurance
/// against a missed notification rather than the mechanism — the same 50ms
/// the delivery loop uses, and for the same reason.
const PULL_POLL: Duration = Duration::from_millis(50);

/// Object-key suffix of the channel's `ConsumerAdmin`.
const CONSUMER_ADMIN_SUFFIX: &[u8] = b"/consumerAdmin";
/// Object-key suffix of the channel's `SupplierAdmin`.
const SUPPLIER_ADMIN_SUFFIX: &[u8] = b"/supplierAdmin";

// ─────────────────────────────────────────────────────────────────────────────
// Raised failures
// ─────────────────────────────────────────────────────────────────────────────

/// The user exceptions this servant raises. Both are memberless in the
/// standard, so each body is just its repository id — the shape
/// [`crate::Error::UserException`] hands back to our own client.
///
/// [`TYPE_ERROR_ID`] has no variant here on purpose: `TypeError` is raised by
/// the *typed* channel surface, which is out of scope, so a variant for it
/// would be a shape nothing can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserExc {
    AlreadyConnected,
    Disconnected,
}

impl UserExc {
    fn id(self) -> &'static str {
        match self {
            UserExc::AlreadyConnected => ALREADY_CONNECTED_ID,
            UserExc::Disconnected => DISCONNECTED_ID,
        }
    }

    fn write(self, out: &mut Encoder) {
        out.put_str(self.id());
    }
}

/// A failure a handler raises: a CosEvent user exception, or a system
/// exception for the failures CORBA itself defines.
enum Raise {
    User(UserExc),
    System(SystemException),
}

impl From<UserExc> for Raise {
    fn from(e: UserExc) -> Self {
        Raise::User(e)
    }
}

impl From<SystemException> for Raise {
    fn from(e: SystemException) -> Self {
        Raise::System(e)
    }
}

fn marshal() -> Raise {
    Raise::System(SystemException::marshal())
}

/// `BAD_PARAM`, which §2.3.6 of the service specification requires for a nil
/// reference handed to `connect_push_consumer`.
fn bad_param() -> Raise {
    Raise::System(SystemException {
        id: "IDL:omg.org/CORBA/BAD_PARAM:1.0".into(),
        minor: 0,
        completed: crate::server::Completion::No,
    })
}

/// `TIMEOUT`, for a `pull` that reached [`DEFAULT_PULL_BLOCK`] with nothing to
/// hand back.
///
/// `COMPLETED_NO` is the load-bearing half: it says no event was consumed, so
/// a retry cannot lose one. That is what makes a bounded `pull` a faithful
/// stand-in for the specification's unbounded one rather than a truncation of
/// it.
fn pull_timed_out() -> Raise {
    Raise::System(SystemException {
        id: "IDL:omg.org/CORBA/TIMEOUT:1.0".into(),
        minor: 0,
        completed: crate::server::Completion::No,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// State
// ─────────────────────────────────────────────────────────────────────────────

/// One captured event, shared by reference between every proxy queue it was
/// fanned out to.
#[derive(Debug)]
struct Event {
    /// The `any`, value bytes verbatim in the byte order they arrived in.
    any: Any,
    /// The CDR alignment (message offset mod 8) at which the **value** bytes
    /// began — that is, just after the `TypeCode`. The value carries padding
    /// computed for that position, so relaying it anywhere else is only sound
    /// when it lands at the same alignment; [`deliver`] measures where it
    /// will land and refuses a mismatch rather than emitting garbage.
    value_align: usize,
}

/// A `ProxyPushSupplier`: the channel's outbound half towards one consumer.
#[derive(Debug, Default)]
struct ProxySupplier {
    /// The consumer's reference, set by `connect_push_consumer`.
    consumer: Option<Ior>,
    /// Bounded, drop-oldest. Only the delivery thread drains it.
    queue: VecDeque<Arc<Event>>,
    consecutive_failures: u32,
}

/// A `ProxyPullSupplier`: the channel's outbound half towards one consumer
/// that asks rather than one that is called.
///
/// The same bounded, drop-oldest deque a [`ProxySupplier`] has — that
/// sameness is the whole finding of the deferral this implements — but drained
/// by the consumer's own `pull`/`try_pull` instead of by the delivery thread,
/// so there is no connection, no timeout and no failure count here. There is
/// nothing to fail: the channel never dials a pulling consumer.
#[derive(Debug, Default)]
struct ProxyPullSupplier {
    /// Whether `connect_pull_consumer` has been called. Its own flag and not
    /// `consumer.is_some()`, because a nil `PullConsumer` is legal — the
    /// reference exists only so the proxy could call `disconnect_pull_consumer`
    /// back, which the standard makes optional and this channel never does.
    connected: bool,
    /// The consumer's reference, when it gave a non-nil one. Recorded and not
    /// dialled; see the module docs on why the supplier side of pull is not
    /// served.
    consumer: Option<Ior>,
    /// Bounded, drop-oldest. Only a `pull`/`try_pull` drains it.
    queue: VecDeque<Arc<Event>>,
}

/// A `ProxyPushConsumer`: the channel's inbound half from one supplier.
#[derive(Debug, Default)]
struct ProxyConsumer {
    /// Whether `connect_push_supplier` has been called. A nil supplier
    /// reference is legal (the standard only needs it to call
    /// `disconnect_push_supplier` back, which is optional), so connectedness
    /// is its own flag rather than `supplier.is_some()`.
    connected: bool,
    /// The supplier's reference, when it gave a non-nil one.
    supplier: Option<Ior>,
}

/// Counters the channel reports rather than discarding.
///
/// Every number here exists because something was thrown away or refused;
/// a channel that dropped events and said nothing would be the "unmeasured
/// check reported as a pass" failure in another costume.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChannelStats {
    /// Events accepted from suppliers (each fans out to every consumer).
    pub accepted: u64,
    /// Successful outbound `push` invocations.
    pub delivered: u64,
    /// Queued events discarded: by overflow (drop-oldest) or by a disconnect
    /// abandoning a backlog.
    pub dropped: u64,
    /// Outbound `push` invocations that failed.
    pub push_failures: u64,
    /// Proxies disconnected for reaching [`MAX_CONSECUTIVE_FAILURES`].
    pub disconnected_for_failure: u64,
    /// Events refused delivery because the destination's CDR alignment
    /// differs from where the `any` was captured. See the module docs.
    pub unrelayable: u64,
    /// Events handed to a consumer by `pull` or `try_pull`.
    ///
    /// The pull counterpart of `delivered`, and separate from it on purpose:
    /// one number counts what the channel managed to push out, the other what
    /// a consumer came and took. Adding them would hide which half is moving.
    pub pulled: u64,
    /// Events sitting in proxy queues right now — push and pull alike, because
    /// a queued event is a queued event whichever end will drain it.
    pub queued: usize,
    /// Push proxies with a consumer reference attached.
    pub consumers_connected: usize,
    /// Pull proxies a consumer has connected itself to.
    pub pull_consumers_connected: usize,
}

/// Everything both the servant thread and the delivery thread touch.
#[derive(Debug)]
struct ChannelState {
    proxy_suppliers: BTreeMap<Vec<u8>, ProxySupplier>,
    proxy_pull_suppliers: BTreeMap<Vec<u8>, ProxyPullSupplier>,
    proxy_consumers: BTreeMap<Vec<u8>, ProxyConsumer>,
    minted: u64,
    queue_limit: usize,
    /// How long a `pull` blocks before it raises `TIMEOUT`.
    pull_block: Duration,
    stopped: bool,
    /// Round-robin cursor, so one busy proxy cannot starve the others.
    cursor: Vec<u8>,
    /// Jobs taken out of a queue but not yet recorded. Part of "idle".
    in_flight: usize,
    stats: ChannelStats,
}

/// One delivery attempt, copied out from under the lock.
struct Job {
    proxy: Vec<u8>,
    consumer: Ior,
    event: Arc<Event>,
}

/// What one delivery attempt did.
enum Outcome {
    Delivered,
    Failed(String),
    Unrelayable(String),
}

impl ChannelState {
    fn new() -> Self {
        Self {
            proxy_suppliers: BTreeMap::new(),
            proxy_pull_suppliers: BTreeMap::new(),
            proxy_consumers: BTreeMap::new(),
            minted: 0,
            queue_limit: DEFAULT_QUEUE_LIMIT,
            pull_block: DEFAULT_PULL_BLOCK,
            stopped: false,
            cursor: Vec::new(),
            in_flight: 0,
            stats: ChannelStats::default(),
        }
    }

    fn refresh_gauges(&mut self) {
        self.stats.queued = self.proxy_suppliers.values().map(|p| p.queue.len()).sum::<usize>()
            + self.proxy_pull_suppliers.values().map(|p| p.queue.len()).sum::<usize>();
        self.stats.consumers_connected =
            self.proxy_suppliers.values().filter(|p| p.consumer.is_some()).count();
        self.stats.pull_consumers_connected =
            self.proxy_pull_suppliers.values().filter(|p| p.connected).count();
    }

    fn stats(&mut self) -> ChannelStats {
        self.refresh_gauges();
        self.stats
    }

    /// Whether the **delivery thread** has nothing left to do.
    ///
    /// Pull queues are deliberately not part of this. They are drained by a
    /// consumer's own `pull`, on a schedule this process does not control, so
    /// counting them would make [`ChannelHandle::wait_idle`] wait for a
    /// stranger — a caller asking "has everything I published gone out?" would
    /// block on a consumer that has simply not come back yet.
    fn idle(&self) -> bool {
        self.in_flight == 0
            && self.proxy_suppliers.values().all(|p| p.queue.is_empty() || p.consumer.is_none())
    }

    /// Fans one event out to every connected proxy, dropping the oldest where
    /// a queue is full. Returns nothing: a supplier's `push` succeeds once the
    /// channel has taken the event, which is what `oneway`-adjacent event
    /// delivery means.
    fn fan_out(&mut self, event: Event) {
        let event = Arc::new(event);
        let limit = self.queue_limit;
        self.stats.accepted += 1;
        for (key, proxy) in self.proxy_suppliers.iter_mut() {
            if proxy.consumer.is_none() {
                continue;
            }
            proxy.queue.push_back(Arc::clone(&event));
            while proxy.queue.len() > limit {
                proxy.queue.pop_front();
                self.stats.dropped += 1;
                // Loud, per event. Control-plane granularity means a healthy
                // channel prints none of these at all, so a stream of them is
                // the signal, not noise.
                eprintln!(
                    "orbweaver: event channel dropped the oldest event for proxy {} \
                     (queue limit {limit}, {} dropped in total)",
                    String::from_utf8_lossy(key),
                    self.stats.dropped
                );
            }
        }
        // The pull proxies take the same event under the same bound. That the
        // two loops are the same loop is the measured answer to the deferral:
        // a pull queue was never going to be a different buffer.
        for (key, proxy) in self.proxy_pull_suppliers.iter_mut() {
            if !proxy.connected {
                continue;
            }
            proxy.queue.push_back(Arc::clone(&event));
            while proxy.queue.len() > limit {
                proxy.queue.pop_front();
                self.stats.dropped += 1;
                eprintln!(
                    "orbweaver: event channel dropped the oldest event for pull proxy {} \
                     (queue limit {limit}, {} dropped in total)",
                    String::from_utf8_lossy(key),
                    self.stats.dropped
                );
            }
        }
    }

    /// The next event `key`'s puller may have, skipping any this channel
    /// cannot hand back at `at` in a stream of `endian`.
    ///
    /// An event that cannot be relayed is **discarded and counted**, not
    /// returned as an error: the mismatch is this module's limitation, not the
    /// caller's request, which is the distinction [`ChannelState::record`]
    /// already draws for the push path. Returning it as an exception would
    /// also wedge the queue permanently — the same event would fail the same
    /// way on every retry.
    fn take_pull_event(&mut self, key: &[u8], at: usize, endian: Endian) -> Option<Arc<Event>> {
        loop {
            let event = self.proxy_pull_suppliers.get_mut(key)?.queue.pop_front()?;
            match relay_check(&event, at, endian) {
                Ok(()) => {
                    self.stats.pulled += 1;
                    return Some(event);
                }
                Err(why) => {
                    self.stats.unrelayable += 1;
                    self.stats.dropped += 1;
                    eprintln!(
                        "orbweaver: event channel cannot hand an event to the puller on {}: \
                         {why}; the event was dropped",
                        String::from_utf8_lossy(key)
                    );
                }
            }
        }
    }

    /// Picks the next delivery, round-robin from the cursor. Increments
    /// `in_flight`; the matching [`ChannelState::record`] decrements it.
    fn take_next(&mut self) -> Option<Job> {
        let keys: Vec<Vec<u8>> = self.proxy_suppliers.keys().cloned().collect();
        if keys.is_empty() {
            return None;
        }
        let start = keys.iter().position(|k| k.as_slice() > self.cursor.as_slice()).unwrap_or(0);
        for i in 0..keys.len() {
            let key = &keys[(start + i) % keys.len()];
            let proxy = self.proxy_suppliers.get_mut(key).expect("key came from this map");
            if let Some(consumer) = proxy.consumer.clone()
                && let Some(event) = proxy.queue.pop_front()
            {
                self.cursor = key.clone();
                self.in_flight += 1;
                return Some(Job { proxy: key.clone(), consumer, event });
            }
        }
        None
    }

    fn record(&mut self, proxy_key: &[u8], outcome: Outcome) {
        self.in_flight = self.in_flight.saturating_sub(1);
        let name = String::from_utf8_lossy(proxy_key).into_owned();
        let Some(proxy) = self.proxy_suppliers.get_mut(proxy_key) else {
            return; // disconnected and forgotten while the push was in flight
        };
        match outcome {
            Outcome::Delivered => {
                self.stats.delivered += 1;
                proxy.consecutive_failures = 0;
            }
            Outcome::Unrelayable(why) => {
                // Our limitation, not the consumer's: it must not count
                // towards disconnecting a consumer that is answering fine.
                self.stats.unrelayable += 1;
                eprintln!("orbweaver: event channel cannot relay to {name}: {why}");
            }
            Outcome::Failed(why) => {
                self.stats.push_failures += 1;
                proxy.consecutive_failures += 1;
                let n = proxy.consecutive_failures;
                eprintln!(
                    "orbweaver: event channel push to {name} failed ({n}/{MAX_CONSECUTIVE_FAILURES}): {why}"
                );
                if n >= MAX_CONSECUTIVE_FAILURES {
                    let abandoned = proxy.queue.len();
                    proxy.queue.clear();
                    proxy.consumer = None;
                    proxy.consecutive_failures = 0;
                    self.stats.dropped += abandoned as u64;
                    self.stats.disconnected_for_failure += 1;
                    eprintln!(
                        "orbweaver: event channel disconnected {name} after \
                         {MAX_CONSECUTIVE_FAILURES} consecutive failures; \
                         {abandoned} queued event(s) dropped"
                    );
                }
            }
        }
    }
}

/// The name this channel's lock section is reported under when the discipline
/// is violated. See [`crate::guarded`].
const CHANNEL_LOCK: &str = "the event channel's state";

/// The mutex plus the two condition variables the delivery loop waits on.
///
/// A [`crate::guarded::Guarded`] would be the ordinary choice for a servant's
/// state, and it is what the other servants in this batch use — but a
/// `Condvar` cannot wait on a closure, and this channel's delivery loop and
/// its `wait_idle` both need one. So the mutex stays, and the discipline is
/// joined from the other end: [`Shared::lock`] hands back a guard that
/// registers a [`Section`], so this module's rule 1 is enforced by the same
/// tripwire that enforces everybody else's.
#[derive(Debug)]
struct Shared {
    state: Mutex<ChannelState>,
    /// Raised when there is work, or when the channel is stopping.
    wake: Condvar,
    /// Raised after every recorded outcome, for [`ChannelHandle::wait_until`].
    progress: Condvar,
}

/// The channel state, held — and *registered as held*, which is the point.
///
/// There is no way to reach [`ChannelState`] except through one of these, and
/// no way to hold one across an outbound call without
/// [`crate::guarded::assert_nothing_held`] firing from inside `connect` or
/// `invoke`. Rule 1 of the module docs stops being a rule people remember.
#[derive(Debug)]
struct Held<'a> {
    // Declared first so the mutex is released before the section is closed:
    // the marker must outlive what it marks, or there is an instant where a
    // thread holds the lock and the tripwire cannot see it.
    state: MutexGuard<'a, ChannelState>,
    section: Section,
}

impl std::ops::Deref for Held<'_> {
    type Target = ChannelState;

    fn deref(&self) -> &ChannelState {
        &self.state
    }
}

impl std::ops::DerefMut for Held<'_> {
    fn deref_mut(&mut self) -> &mut ChannelState {
        &mut self.state
    }
}

impl Shared {
    /// A poisoned mutex here means a servant panicked mid-operation. The state
    /// behind it is a set of independent counters and queues, none of which is
    /// left half-updated by an unwind, so recovering is better than turning
    /// one panic into a permanently dead channel.
    fn lock(&self) -> Held<'_> {
        let section = Section::enter(CHANNEL_LOCK);
        Held { state: self.state.lock().unwrap_or_else(|e| e.into_inner()), section }
    }

    /// Waits on `cv` until it is notified or `left` elapses, handing the lock
    /// back afterwards.
    ///
    /// The [`Section`] is carried *through* the wait rather than closed and
    /// reopened. A condvar wait releases the mutex, so nothing is excluded
    /// while it runs — but the thread is still inside a critical section it
    /// intends to resume, and an outbound call from in there is the same
    /// mistake as one made with the mutex in hand.
    fn wait<'a>(&'a self, cv: &Condvar, held: Held<'a>, left: Duration) -> Held<'a> {
        let Held { state, section } = held;
        let (state, _) = cv.wait_timeout(state, left).unwrap_or_else(|e| e.into_inner());
        Held { state, section }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Handle
// ─────────────────────────────────────────────────────────────────────────────

/// A cloneable view of a running channel: counters, in-process publishing and
/// shutdown, usable while the servant itself is owned by a serving thread.
#[derive(Debug, Clone)]
pub struct ChannelHandle {
    shared: Arc<Shared>,
}

impl ChannelHandle {
    /// The counters, including the current queue depth.
    pub fn stats(&self) -> ChannelStats {
        self.shared.lock().stats()
    }

    /// Overrides the per-consumer queue bound, for push and pull proxies
    /// alike — they share it, which is the point.
    pub fn set_queue_limit(&self, events: usize) {
        self.shared.lock().queue_limit = events.max(1);
    }

    /// Overrides how long a `pull` blocks before raising `TIMEOUT`.
    ///
    /// Takes effect for calls that arrive after it; a `pull` already blocking
    /// keeps the deadline it entered with, because moving somebody else's
    /// deadline underneath them is how a "timeout" becomes unbounded again.
    pub fn set_pull_block(&self, block: Duration) {
        self.shared.lock().pull_block = block;
    }

    /// Publishes an event from inside this process, with no socket involved.
    ///
    /// The value is marshalled at an 8-aligned `any` start — the alignment a
    /// GIOP 1.2 request body gives it — so the delivery path can relay the
    /// bytes verbatim under exactly the rule it applies to captured events.
    pub fn publish<F>(&self, tc: &TypeCode, endian: Endian, write_value: F) -> Result<()>
    where
        F: FnOnce(&mut Encoder),
    {
        // Origin 0 means the `any` begins at offset 0, which is 8-aligned;
        // the value bytes therefore carry the same padding a wire capture at
        // a 1.2 body start would have.
        let mut e = Encoder::new(endian);
        typecode::encode(&mut e, tc)?;
        let tc_len = e.len();
        write_value(&mut e);
        let bytes = e.finish()?;
        let any = Any { tc: tc.clone(), value: bytes[tc_len..].to_vec(), endian };
        self.shared.lock().fan_out(Event { any, value_align: tc_len % 8 });
        self.shared.wake.notify_all();
        Ok(())
    }

    /// Blocks until `pred` holds of the counters, or `timeout` elapses.
    /// Returns whether it held.
    pub fn wait_until(
        &self,
        timeout: Duration,
        mut pred: impl FnMut(&ChannelStats) -> bool,
    ) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = self.shared.lock();
        loop {
            let stats = state.stats();
            if pred(&stats) {
                return true;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            state = self.shared.wait(&self.shared.progress, state, left);
        }
    }

    /// Blocks until every connected proxy's queue is empty and no push is in
    /// flight. Returns whether that happened before `timeout`.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = self.shared.lock();
        loop {
            if state.idle() {
                return true;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            state = self.shared.wait(&self.shared.progress, state, left);
        }
    }

    /// Stops the delivery thread and the channel with it. Queued events are
    /// not delivered; they are counted as dropped, because pretending a
    /// stopped channel delivered them would be exactly the silent truncation
    /// this module refuses. Pull queues are emptied and counted the same way,
    /// and a `pull` blocked on a stopped channel is woken and answered
    /// `Disconnected` rather than left to time out.
    pub fn stop(&self) {
        let mut state = self.shared.lock();
        state.stopped = true;
        let abandoned: usize = state.proxy_suppliers.values().map(|p| p.queue.len()).sum::<usize>()
            + state.proxy_pull_suppliers.values().map(|p| p.queue.len()).sum::<usize>();
        if abandoned > 0 {
            state.stats.dropped += abandoned as u64;
            eprintln!("orbweaver: event channel stopped with {abandoned} undelivered event(s)");
            for proxy in state.proxy_suppliers.values_mut() {
                proxy.queue.clear();
            }
            for proxy in state.proxy_pull_suppliers.values_mut() {
                proxy.queue.clear();
            }
        }
        drop(state);
        self.shared.wake.notify_all();
        self.shared.progress.notify_all();
    }
}

/// The delivery thread, joined on drop.
///
/// Held by whoever started it; dropping it stops the channel, so a spike that
/// forgets to stop cannot leave a thread pushing into a torn-down fixture.
#[derive(Debug)]
pub struct Delivery {
    handle: ChannelHandle,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Delivery {
    /// The same handle the channel exposes, for callers that only kept this.
    pub fn handle(&self) -> ChannelHandle {
        self.handle.clone()
    }
}

impl Drop for Delivery {
    fn drop(&mut self) {
        self.handle.stop();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The delivery loop
// ─────────────────────────────────────────────────────────────────────────────

/// Where a `push` argument lands in an outbound request, **measured** through
/// the real request encoder rather than recomputed.
///
/// Recomputing it would mean restating [`crate::encode_request`]'s header
/// layout here, where it could drift silently; asking the encoder cannot.
/// `usize::MAX` on failure, which no alignment equals, so an unencodable
/// request is refused rather than guessed at.
fn outbound_any_alignment(conn: &Connection) -> usize {
    let at = std::cell::Cell::new(usize::MAX);
    let encoded = crate::encode_request(
        conn.version(),
        conn.endian(),
        0,
        conn.object_key(),
        "push",
        true,
        |e| at.set(e.position()),
    );
    match encoded {
        Ok(_) => at.get() % 8,
        Err(_) => usize::MAX,
    }
}

/// Whether `event`'s captured value bytes may be written verbatim at offset
/// `at` into a stream of byte order `endian`.
///
/// One predicate for both directions. The delivery thread asks it about an
/// outbound request body and `pull` asks it about a reply body, and they have
/// to agree: an `any` captured raw carries padding computed for exactly one
/// alignment and is only readable in exactly one byte order. Two copies of
/// this rule would be two chances to relax one of them.
fn relay_check(event: &Event, at: usize, endian: Endian) -> std::result::Result<(), String> {
    let any = &event.any;
    if endian != any.endian {
        return Err(format!(
            "the event was captured {:?}-endian and this stream is {endian:?}-endian",
            any.endian
        ));
    }
    let mut probe = Encoder::continuing_at(endian, at);
    if typecode::encode(&mut probe, &any.tc).is_err() {
        return Err("the TypeCode did not re-encode".into());
    }
    let value_landing = (at + probe.len()) % 8;
    if value_landing != event.value_align {
        return Err(format!(
            "value captured at alignment {} would land at {value_landing}",
            event.value_align
        ));
    }
    Ok(())
}

/// One outbound `push`, with **no lock held** — see the module docs.
fn deliver(
    conns: &mut HashMap<Vec<u8>, (Ior, Connection)>,
    job: &Job,
    timeout: Duration,
) -> Outcome {
    let fresh = match conns.get(&job.proxy) {
        // A reconnect to a different consumer must not reuse the old socket.
        Some((ior, conn)) => *ior != job.consumer || !conn.is_usable(),
        None => true,
    };
    if fresh {
        conns.remove(&job.proxy);
        match Connection::connect(&job.consumer, timeout) {
            Ok(conn) => {
                conns.insert(job.proxy.clone(), (job.consumer.clone(), conn));
            }
            Err(e) => return Outcome::Failed(format!("connect: {e}")),
        }
    }
    let (_, conn) = conns.get_mut(&job.proxy).expect("inserted just above");

    // The captured value bytes are opaque: they are only readable in the byte
    // order they arrived in, so the relay adopts it rather than re-encoding.
    conn.set_endian(job.event.any.endian);
    let any = &job.event.any;

    // Where will the value bytes land? The `any` starts at the measured body
    // alignment, and our re-encoding of its TypeCode — probed here at that
    // same alignment, so the probe's length is exact — comes first. A
    // destination that differs from where the value was captured is refused
    // and counted, not guessed at: the bytes carry padding for one position
    // only, and re-marshalling an arbitrary value means walking its TypeCode,
    // which is orbweaver-dynamic's job.
    let landing = outbound_any_alignment(conn);
    if landing == usize::MAX {
        return Outcome::Unrelayable("the outbound request header did not encode".into());
    }
    if let Err(why) = relay_check(&job.event, landing, conn.endian()) {
        return Outcome::Unrelayable(why);
    }

    let result = conn.invoke("push", |e| {
        // Proved encodable by the probe just above, at this very alignment.
        let _ = typecode::encode_any_at_same_alignment(e, any);
    });
    match result {
        Ok(_) => Outcome::Delivered,
        Err(e) => {
            conns.remove(&job.proxy); // a failed invoke may have poisoned it
            Outcome::Failed(e.to_string())
        }
    }
}

fn delivery_loop(shared: Arc<Shared>, timeout: Duration) {
    // Connections live here, in the delivery thread's own state, never behind
    // the mutex: a socket the servant thread could reach is a socket the
    // servant thread could block on.
    let mut conns: HashMap<Vec<u8>, (Ior, Connection)> = HashMap::new();
    loop {
        let job = {
            let mut state = shared.lock();
            loop {
                if state.stopped {
                    return;
                }
                if let Some(job) = state.take_next() {
                    break job;
                }
                state = shared.wait(&shared.wake, state, Duration::from_millis(50));
            }
        };
        // ── no lock is held across this call. See the module docs. The block
        // above ends here, which closes the lock section; `deliver` connects
        // and invokes, and both would refuse to run if it had not. ──
        let outcome = deliver(&mut conns, &job, timeout);
        shared.lock().record(&job.proxy, outcome);
        shared.progress.notify_all();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The servant
// ─────────────────────────────────────────────────────────────────────────────

/// Which of the channel's objects a request addressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Channel,
    ConsumerAdmin,
    SupplierAdmin,
    ProxySupplier,
    ProxyConsumer,
    ProxyPullSupplier,
}

impl Target {
    fn repository_id(self) -> &'static str {
        match self {
            Target::Channel => EVENT_CHANNEL_ID,
            Target::ConsumerAdmin => CONSUMER_ADMIN_ID,
            Target::SupplierAdmin => SUPPLIER_ADMIN_ID,
            Target::ProxySupplier => PROXY_PUSH_SUPPLIER_ID,
            Target::ProxyConsumer => PROXY_PUSH_CONSUMER_ID,
            Target::ProxyPullSupplier => PROXY_PULL_SUPPLIER_ID,
        }
    }
}

/// An in-memory CosEvent push channel behind [`crate::server::Server`].
///
/// One instance serves the channel, both admins and every proxy either admin
/// mints, each as its own object key on this one dispatch — the F6 shape.
/// `host` and `port` are what go into minted references and are the caller's
/// to publish correctly (Phase 0 assumption D: the bind address and the
/// publishable address differ behind NAT).
///
/// # Sharing: the lock was already here
///
/// This servant needed no new state to implement [`SharedDispatch`]. It has
/// been interior-mutable since it was written, because the delivery thread and
/// the serving thread have always been two threads over one [`Shared`]; the
/// `&mut self` on its old `Dispatch` was a formality the type system asked for
/// and the implementation never used. Concurrent dispatch simply adds more
/// threads on the side that already existed.
///
/// The sharing decision is therefore **one `Mutex`, not an `RwLock`**: almost
/// every operation here writes (a `push` enqueues, an `obtain_*` mints, a
/// `connect_*` attaches), the two condition variables are bound to it, and a
/// reader-writer lock would buy nothing for `_is_a` alone. What the batch
/// actually buys this servant is that a slow *consumer* — the thing this
/// module spends its bounded queue and its push timeout on — is now on the
/// delivery thread's clock only, with no server-wide mutex behind it.
///
/// [`SharedDispatch`]: crate::server::SharedDispatch
#[derive(Debug)]
pub struct EventChannelServer {
    host: String,
    port: u16,
    base: Vec<u8>,
    consumer_admin: Vec<u8>,
    supplier_admin: Vec<u8>,
    shared: Arc<Shared>,
}

impl EventChannelServer {
    /// A channel rooted at `base_key`, minting references that point at
    /// `host:port`. No delivery thread runs until
    /// [`EventChannelServer::start_delivery`] is called — a channel with no
    /// delivery thread accepts and queues, which is what the queue-accounting
    /// tests need.
    pub fn new(host: impl Into<String>, port: u16, base_key: Vec<u8>) -> Self {
        let mut consumer_admin = base_key.clone();
        consumer_admin.extend_from_slice(CONSUMER_ADMIN_SUFFIX);
        let mut supplier_admin = base_key.clone();
        supplier_admin.extend_from_slice(SUPPLIER_ADMIN_SUFFIX);
        Self {
            host: host.into(),
            port,
            base: base_key,
            consumer_admin,
            supplier_admin,
            shared: Arc::new(Shared {
                state: Mutex::new(ChannelState::new()),
                wake: Condvar::new(),
                progress: Condvar::new(),
            }),
        }
    }

    /// Starts the delivery thread with [`DEFAULT_PUSH_TIMEOUT`].
    pub fn start_delivery(&self) -> Delivery {
        self.start_delivery_with(DEFAULT_PUSH_TIMEOUT)
    }

    /// Starts the delivery thread, bounding each outbound push by `timeout`.
    pub fn start_delivery_with(&self, timeout: Duration) -> Delivery {
        let shared = Arc::clone(&self.shared);
        let thread = std::thread::Builder::new()
            .name("orbweaver-event-delivery".into())
            .spawn(move || delivery_loop(shared, timeout))
            .expect("spawning the event delivery thread");
        Delivery { handle: self.handle(), thread: Some(thread) }
    }

    /// A handle usable after the servant has been moved into a serving thread.
    pub fn handle(&self) -> ChannelHandle {
        ChannelHandle { shared: Arc::clone(&self.shared) }
    }

    /// The channel's own object key — what [`crate::server::Server`] must be
    /// bound with for the two to describe the same object.
    pub fn channel_key(&self) -> &[u8] {
        &self.base
    }

    /// A publishable reference to the channel itself.
    pub fn channel_ior(&self) -> Ior {
        self.ior_for(&self.base, EVENT_CHANNEL_ID)
    }

    fn ior_for(&self, key: &[u8], type_id: &str) -> Ior {
        Ior {
            type_id: type_id.to_owned(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: self.host.clone(),
                port: self.port,
                object_key: key.to_vec(),
                // §7.10.2.4: a profile with no `TAG_CODE_SETS` declares no
                // `wchar` support, and a conformant client then refuses to
                // marshal a `wstring` to it. See `codeset::server_component`.
                components: vec![codeset::server_component()],
            }],
        }
    }

    fn route(&self, key: &[u8]) -> Option<Target> {
        if key == self.base {
            return Some(Target::Channel);
        }
        if key == self.consumer_admin {
            return Some(Target::ConsumerAdmin);
        }
        if key == self.supplier_admin {
            return Some(Target::SupplierAdmin);
        }
        let state = self.shared.lock();
        if state.proxy_suppliers.contains_key(key) {
            return Some(Target::ProxySupplier);
        }
        if state.proxy_consumers.contains_key(key) {
            return Some(Target::ProxyConsumer);
        }
        if state.proxy_pull_suppliers.contains_key(key) {
            return Some(Target::ProxyPullSupplier);
        }
        None
    }

    fn mint(&self, tag: &str) -> Vec<u8> {
        let mut state = self.shared.lock();
        state.minted += 1;
        let mut key = self.base.clone();
        key.extend_from_slice(format!("/{tag}{}", state.minted).as_bytes());
        key
    }

    /// Dispatches one operation, writing the result body into `out`.
    ///
    /// Invariant every arm keeps, inherited from F6: nothing is written into
    /// `out` until the operation can no longer raise a *user* exception,
    /// because the buffer travels whole under a single reply status.
    fn invoke_operation(&self, req: &Request, out: &mut Encoder) -> std::result::Result<(), Raise> {
        let target = self
            .route(&req.object_key)
            .ok_or_else(|| Raise::System(SystemException::object_not_exist()))?;
        let mut args = req.body().map_err(|_| marshal())?;

        // Answered identically for every object, before the per-target arms.
        match req.operation.as_str() {
            "_is_a" => {
                let id = args.get_string().map_err(|_| marshal())?;
                out.put_bool(id == target.repository_id() || id == CORBA_OBJECT_ID);
                return Ok(());
            }
            "_non_existent" => {
                out.put_bool(false);
                return Ok(());
            }
            _ => {}
        }

        match (target, req.operation.as_str()) {
            // ── EventChannel ──
            (Target::Channel, "for_consumers") => {
                let ior = self.ior_for(&self.consumer_admin, CONSUMER_ADMIN_ID);
                ior.write_to(out).map_err(|_| marshal())?;
            }
            (Target::Channel, "for_suppliers") => {
                let ior = self.ior_for(&self.supplier_admin, SUPPLIER_ADMIN_ID);
                ior.write_to(out).map_err(|_| marshal())?;
            }

            // ── ConsumerAdmin ──
            (Target::ConsumerAdmin, "obtain_push_supplier") => {
                let key = self.mint("pps");
                self.shared.lock().proxy_suppliers.insert(key.clone(), ProxySupplier::default());
                self.ior_for(&key, PROXY_PUSH_SUPPLIER_ID).write_to(out).map_err(|_| marshal())?;
            }

            (Target::ConsumerAdmin, "obtain_pull_supplier") => {
                let key = self.mint("pls");
                self.shared
                    .lock()
                    .proxy_pull_suppliers
                    .insert(key.clone(), ProxyPullSupplier::default());
                self.ior_for(&key, PROXY_PULL_SUPPLIER_ID).write_to(out).map_err(|_| marshal())?;
            }

            // ── SupplierAdmin ──
            (Target::SupplierAdmin, "obtain_push_consumer") => {
                let key = self.mint("ppc");
                self.shared.lock().proxy_consumers.insert(key.clone(), ProxyConsumer::default());
                self.ior_for(&key, PROXY_PUSH_CONSUMER_ID).write_to(out).map_err(|_| marshal())?;
            }

            // ── ProxyPushSupplier: the consumer's end ──
            (Target::ProxySupplier, "connect_push_consumer") => {
                let consumer = Ior::read_from(&mut args).map_err(|_| marshal())?;
                if consumer.is_nil() {
                    // §2.3.6: a nil PushConsumer is BAD_PARAM. Accepting one
                    // would queue events for a reference nothing can dial.
                    return Err(bad_param());
                }
                let mut state = self.shared.lock();
                let proxy = state
                    .proxy_suppliers
                    .get_mut(&req.object_key)
                    .ok_or_else(|| Raise::System(SystemException::object_not_exist()))?;
                if proxy.consumer.is_some() {
                    return Err(UserExc::AlreadyConnected.into());
                }
                proxy.consumer = Some(consumer);
                proxy.consecutive_failures = 0;
            }
            (Target::ProxySupplier, "disconnect_push_supplier") => {
                let mut state = self.shared.lock();
                if let Some(proxy) = state.proxy_suppliers.get_mut(&req.object_key) {
                    let abandoned = proxy.queue.len() as u64;
                    proxy.queue.clear();
                    proxy.consumer = None;
                    proxy.consecutive_failures = 0;
                    state.stats.dropped += abandoned;
                    if abandoned > 0 {
                        eprintln!(
                            "orbweaver: event channel disconnected {} with {abandoned} \
                             queued event(s) dropped",
                            String::from_utf8_lossy(&req.object_key)
                        );
                    }
                }
                // The key stays known and reconnectable, the choice F6 made
                // for unbound contexts. Idempotent: a second disconnect is
                // not an error in the standard.
            }

            // ── ProxyPushConsumer: the supplier's end ──
            (Target::ProxyConsumer, "connect_push_supplier") => {
                // A nil PushSupplier is legal: the reference exists only so
                // the proxy can call disconnect_push_supplier back, which is
                // optional.
                let supplier = Ior::read_from(&mut args).map_err(|_| marshal())?;
                let mut state = self.shared.lock();
                let proxy = state
                    .proxy_consumers
                    .get_mut(&req.object_key)
                    .ok_or_else(|| Raise::System(SystemException::object_not_exist()))?;
                if proxy.connected {
                    return Err(UserExc::AlreadyConnected.into());
                }
                proxy.connected = true;
                proxy.supplier = if supplier.is_nil() { None } else { Some(supplier) };
            }
            (Target::ProxyConsumer, "push") => {
                {
                    let state = self.shared.lock();
                    let proxy = state
                        .proxy_consumers
                        .get(&req.object_key)
                        .ok_or_else(|| Raise::System(SystemException::object_not_exist()))?;
                    if !proxy.connected {
                        return Err(UserExc::Disconnected.into());
                    }
                }
                let event = capture_event(&mut args)?;
                // The lock is taken only to enqueue, and released before this
                // arm returns: the servant never calls out while holding it.
                self.shared.lock().fan_out(event);
                self.shared.wake.notify_all();
            }
            (Target::ProxyConsumer, "disconnect_push_consumer") => {
                let mut state = self.shared.lock();
                if let Some(proxy) = state.proxy_consumers.get_mut(&req.object_key) {
                    proxy.connected = false;
                    proxy.supplier = None;
                }
            }

            // ── ProxyPullSupplier: the pulling consumer's end ──
            (Target::ProxyPullSupplier, "connect_pull_consumer") => {
                // A nil `PullConsumer` is legal here, and the asymmetry with
                // `connect_push_consumer` — which answers `BAD_PARAM` — is
                // real rather than an inconsistency: that reference is the
                // address delivery is sent to, and this one is never dialled
                // at all. It exists only so the proxy could call
                // `disconnect_pull_consumer` back, which the standard makes
                // optional and this channel does not do.
                let consumer = Ior::read_from(&mut args).map_err(|_| marshal())?;
                let mut state = self.shared.lock();
                let proxy = state
                    .proxy_pull_suppliers
                    .get_mut(&req.object_key)
                    .ok_or_else(|| Raise::System(SystemException::object_not_exist()))?;
                if proxy.connected {
                    return Err(UserExc::AlreadyConnected.into());
                }
                proxy.connected = true;
                proxy.consumer = if consumer.is_nil() { None } else { Some(consumer) };
            }
            (Target::ProxyPullSupplier, "pull" | "try_pull") => {
                // Measured, never recomputed: `out` is the real reply body
                // encoder, positioned where the body will land in the message,
                // so `position()` is the `any`'s true CDR offset. The reply's
                // byte order is the request's and not ours to change, which is
                // the half of `relay_check` the push path never exercises.
                let blocking = req.operation == "pull";
                let at = out.position();
                let endian = out.endian();

                let mut state = self.shared.lock();
                let deadline = Instant::now() + state.pull_block;
                let event = loop {
                    let connected = state
                        .proxy_pull_suppliers
                        .get(&req.object_key)
                        .ok_or_else(|| Raise::System(SystemException::object_not_exist()))?
                        .connected;
                    if !connected || state.stopped {
                        // §2.1.1: pulling from a proxy nothing is connected to
                        // is `Disconnected`, and a stopped channel is the same
                        // answer for the same reason — no more events are
                        // coming from here.
                        return Err(UserExc::Disconnected.into());
                    }
                    if let Some(event) = state.take_pull_event(&req.object_key, at, endian) {
                        break Some(event);
                    }
                    if !blocking {
                        break None;
                    }
                    let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                        return Err(pull_timed_out());
                    };
                    // The mutex is released for the duration of this wait, so
                    // a supplier's `push` is served while a `pull` blocks —
                    // which is the only reason a blocking `pull` can ever be
                    // satisfied at all.
                    state = self.shared.wait(&self.shared.wake, state, left.min(PULL_POLL));
                };
                // Released before anything is marshalled: a large `any` must
                // not hold the channel shut while it is written out.
                drop(state);

                if let Some(event) = event {
                    // `relay_check` proved this encodes, at this offset, in
                    // this byte order.
                    typecode::encode_any_at_same_alignment(out, &event.any)
                        .map_err(|_| marshal())?;
                    if !blocking {
                        out.put_bool(true);
                    }
                } else {
                    // `try_pull` with nothing to give: §2.1.1 says the `any`
                    // returned is `tk_null` and `has_event` is false. The
                    // return value precedes the `out` parameter on the wire,
                    // so the boolean is what tells a decoder where the value
                    // ended — an `any` has no length prefix of its own.
                    typecode::encode(out, &TypeCode::Null).map_err(|_| marshal())?;
                    out.put_bool(false);
                }
                self.shared.progress.notify_all();
            }
            (Target::ProxyPullSupplier, "disconnect_pull_supplier") => {
                let mut state = self.shared.lock();
                if let Some(proxy) = state.proxy_pull_suppliers.get_mut(&req.object_key) {
                    let abandoned = proxy.queue.len() as u64;
                    proxy.queue.clear();
                    proxy.connected = false;
                    proxy.consumer = None;
                    state.stats.dropped += abandoned;
                    if abandoned > 0 {
                        eprintln!(
                            "orbweaver: event channel disconnected pull proxy {} with \
                             {abandoned} queued event(s) dropped",
                            String::from_utf8_lossy(&req.object_key)
                        );
                    }
                }
                drop(state);
                // A `pull` blocked on this proxy is waiting on `wake`; it has
                // to learn it was disconnected rather than sit out its
                // deadline. The key stays known and reconnectable, the same
                // choice the push proxy and F6's unbound contexts make.
                self.shared.wake.notify_all();
            }

            // The supplier side of the pull model and `destroy` are declared by
            // `CosEventComm` and `CosEventChannelAdmin` and deliberately not
            // served, so the wire says `NO_IMPLEMENT`: a client can tell that
            // from an oversight, and `BAD_OPERATION` — "no such operation" —
            // could not, which left the difference in this module's header
            // where no caller reads it.
            (_, op) if is_deferred(op) => return Err(SystemException::no_implement().into()),
            // Anything else is a name no interface of this object declares.
            _ => return Err(SystemException::bad_operation().into()),
        }
        Ok(())
    }
}

/// The event-service operations this server knows about and does not serve.
///
/// The reasons are in this module's header, and both were re-argued rather
/// than restated: the **supplier** side of pull would make the channel invoke
/// a specified-to-block `pull` on a reference it does not control, for no
/// supplier that exists; `destroy` is an unauthenticated remote operation that
/// would end the channel for every other client and cannot be undone without
/// restarting the process.
///
/// The **consumer** side of pull left this list once it was measured: it is
/// the same bounded queue drained from the other end, which is what the old
/// reason claimed it could not be. Anything removed from here must gain a
/// served arm in [`EventChannelServer::invoke_operation`] in the same change,
/// or the operation silently degrades from a stated refusal to
/// `BAD_OPERATION`, which says "no such operation" and is a lie.
pub fn is_deferred(op: &str) -> bool {
    matches!(
        op,
        "obtain_pull_consumer" | "connect_pull_supplier" | "disconnect_pull_consumer" | "destroy"
    )
}

/// Reads a `push`'s single `any` argument, verbatim.
///
/// The `any` is the last thing in the body, which is the only way its value
/// length can be known: CDR gives an `any` no length prefix, so the enclosing
/// structure has to say where it ends.
fn capture_event(args: &mut orbweaver_cdr::Decoder<'_>) -> std::result::Result<Event, Raise> {
    let tc = typecode::decode(args).map_err(|_| marshal())?;
    // The request body decoder's origin is the start of the message, so this
    // offset — right after the TypeCode — is the value's true CDR alignment.
    let value_align = args.offset() % 8;
    let len = args.remaining();
    let value = args.get_bytes(len).map_err(|_| marshal())?.to_vec();
    Ok(Event { any: Any { tc, value, endian: args.endian() }, value_align })
}

impl SharedDispatch for EventChannelServer {
    /// One dispatch answers for the channel, both admins and every minted
    /// proxy — the F6 shape, answered from the proxy tables rather than
    /// defaulted.
    fn knows(&self, object_key: &[u8]) -> bool {
        self.route(object_key).is_some()
    }

    fn dispatch_body(
        &self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<DispatchBody, SystemException> {
        match self.invoke_operation(request, out) {
            Ok(()) => Ok(DispatchBody::Return),
            Err(Raise::System(ex)) => Err(ex),
            Err(Raise::User(ex)) => {
                // `handle` wrote nothing before raising (its invariant), so
                // the exception body is the whole buffer.
                ex.write(out);
                Ok(DispatchBody::UserException)
            }
        }
    }

    /// The narrow entry point cannot carry a user exception, so one arriving
    /// here gets the standard mapping: `UNKNOWN`, OMG minor 1.
    fn dispatch(
        &self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        match self.dispatch_body(request, out)? {
            DispatchBody::Return => Ok(()),
            DispatchBody::UserException => Err(SystemException::unknown_user_exception()),
        }
    }
}

/// The `&mut self` shape too, forwarding, so a caller already written against
/// [`crate::server::Server::serve`] keeps working — serialized, as that path
/// always was.
impl Dispatch for EventChannelServer {
    fn knows(&self, object_key: &[u8]) -> bool {
        SharedDispatch::knows(self, object_key)
    }

    fn dispatch_body(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<DispatchBody, SystemException> {
        SharedDispatch::dispatch_body(self, request, out)
    }

    fn dispatch(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        SharedDispatch::dispatch(self, request, out)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// A CosEventComm::PushConsumer of our own
// ─────────────────────────────────────────────────────────────────────────────

/// What a [`PushConsumerServant`] has been told, in arrival order.
#[derive(Debug, Default)]
struct SinkState {
    events: Vec<Any>,
    disconnected: bool,
}

/// Where a [`PushConsumerServant`] puts what it receives.
///
/// Cloning shares the storage, so a test or a spike keeps a view of a sink
/// whose servant has been moved into a serving thread.
///
/// **One lock, not two.** The arrived events and the disconnect flag were
/// separate mutexes, which is two locks a consumer could come to hold at once
/// — the shape [`crate::guarded`] refuses. They are one struct behind one
/// [`Guarded`] now, which is also the truthful model: "what this consumer was
/// told" is one thing.
#[derive(Debug, Clone, Default)]
pub struct EventSink {
    state: Arc<Guarded<SinkState>>,
}

impl EventSink {
    /// An empty sink.
    pub fn new() -> Self {
        EventSink { state: Arc::new(Guarded::new("an event sink", SinkState::default())) }
    }

    /// How many events have arrived.
    pub fn len(&self) -> usize {
        self.state.read(|s| s.events.len())
    }

    /// Whether nothing has arrived.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// A copy of everything received, in arrival order.
    pub fn snapshot(&self) -> Vec<Any> {
        self.state.read(|s| s.events.clone())
    }

    /// Whether `disconnect_push_consumer` has been called on the servant.
    pub fn is_disconnected(&self) -> bool {
        self.state.read(|s| s.disconnected)
    }

    /// Waits for at least `n` events. Returns whether they arrived in time.
    ///
    /// The sleep is not decoration: a spin here would burn the core the
    /// delivery thread needs, and a loop with no sleep at all is the Phase 0
    /// wait-loop failure exactly.
    pub fn wait_for(&self, n: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.len() >= n {
                return true;
            }
            if Instant::now() >= deadline {
                return self.len() >= n;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// A minimal `CosEventComm::PushConsumer` servant: it collects.
///
/// The channel needs something to push *to* — in the spike, in the tests, and
/// in F3/F4 where a control-plane consumer is exactly this plus a handler.
#[derive(Debug)]
pub struct PushConsumerServant {
    key: Vec<u8>,
    sink: EventSink,
}

impl PushConsumerServant {
    /// A consumer answering to `object_key`.
    pub fn new(object_key: Vec<u8>) -> Self {
        Self { key: object_key, sink: EventSink::new() }
    }

    /// A view of what it has received.
    pub fn sink(&self) -> EventSink {
        self.sink.clone()
    }

    /// A publishable reference to this consumer.
    pub fn ior(&self, host: &str, port: u16) -> Ior {
        Ior {
            type_id: PUSH_CONSUMER_ID.to_owned(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: host.to_owned(),
                port,
                object_key: self.key.clone(),
                // §7.10.2.4: a profile with no `TAG_CODE_SETS` declares no
                // `wchar` support, and a conformant client then refuses to
                // marshal a `wstring` to it. See `codeset::server_component`.
                components: vec![codeset::server_component()],
            }],
        }
    }
}

/// A consumer is the simplest sharing shape there is: its whole state is the
/// sink, the sink is already shared by clone, and nothing it does calls out.
/// Two `push`es from two suppliers now land concurrently.
impl SharedDispatch for PushConsumerServant {
    fn knows(&self, object_key: &[u8]) -> bool {
        object_key == self.key
    }

    fn dispatch(
        &self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        let mut args = request.body().map_err(|_| SystemException::marshal())?;
        match request.operation.as_str() {
            "_is_a" => {
                let id = args.get_string().map_err(|_| SystemException::marshal())?;
                out.put_bool(id == PUSH_CONSUMER_ID || id == CORBA_OBJECT_ID);
            }
            "_non_existent" => out.put_bool(false),
            "push" => {
                // The `any` is the whole body, so its value runs to the end —
                // the same reasoning as `capture_event`, from the other side.
                // Decoded *before* the lock is taken, so a large event does
                // not hold the sink shut while it is parsed.
                let event = capture_event(&mut args).map_err(|_| SystemException::marshal())?;
                self.sink.state.write(|s| s.events.push(event.any));
            }
            "disconnect_push_consumer" => self.sink.state.write(|s| s.disconnected = true),
            op if is_deferred(op) => return Err(SystemException::no_implement()),
            _ => return Err(SystemException::bad_operation()),
        }
        Ok(())
    }
}

impl Dispatch for PushConsumerServant {
    fn knows(&self, object_key: &[u8]) -> bool {
        SharedDispatch::knows(self, object_key)
    }

    fn dispatch(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        SharedDispatch::dispatch(self, request, out)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Client side
// ─────────────────────────────────────────────────────────────────────────────

/// The client half of the operations this module serves.
///
/// It exists for the same reason `write_name`/`read_name` are shared between
/// the two halves of CosNaming: one place knows each operation's wire shape,
/// so the client and the server cannot drift apart.
///
/// Reaching a different object on the same channel means dialling that
/// object's own reference: an object key is per-reference, not per-connection.
/// Holding the old [`Connection`] open while the new one is dialled is fine —
/// [`crate::server::Server`] serves its connections concurrently — so whether
/// to drop it is the caller's convenience, not a limit. (It was a limit until
/// the server grew a thread per connection; every caller written before that
/// hangs up between hops, which is still correct.)
pub mod client {
    use super::*;

    fn reference(conn: &mut Connection, operation: &str) -> Result<Ior> {
        let reply = conn.invoke_nullary(operation)?;
        let mut body = reply.body()?;
        Ior::read_from(&mut body)
    }

    /// `EventChannel::for_consumers`.
    pub fn for_consumers(conn: &mut Connection) -> Result<Ior> {
        reference(conn, "for_consumers")
    }

    /// `EventChannel::for_suppliers`.
    pub fn for_suppliers(conn: &mut Connection) -> Result<Ior> {
        reference(conn, "for_suppliers")
    }

    /// `ConsumerAdmin::obtain_push_supplier`.
    pub fn obtain_push_supplier(conn: &mut Connection) -> Result<Ior> {
        reference(conn, "obtain_push_supplier")
    }

    /// `SupplierAdmin::obtain_push_consumer`.
    pub fn obtain_push_consumer(conn: &mut Connection) -> Result<Ior> {
        reference(conn, "obtain_push_consumer")
    }

    /// `ConsumerAdmin::obtain_pull_supplier`.
    pub fn obtain_pull_supplier(conn: &mut Connection) -> Result<Ior> {
        reference(conn, "obtain_pull_supplier")
    }

    /// `ProxyPushSupplier::connect_push_consumer`.
    pub fn connect_push_consumer(conn: &mut Connection, consumer: &Ior) -> Result<()> {
        let consumer = consumer.clone();
        // A marshalling failure poisons the encoder and surfaces from invoke.
        conn.invoke("connect_push_consumer", move |e| {
            let _ = consumer.write_to(e);
        })?;
        Ok(())
    }

    /// `ProxyPushConsumer::connect_push_supplier`. A nil `supplier` is legal.
    pub fn connect_push_supplier(conn: &mut Connection, supplier: &Ior) -> Result<()> {
        let supplier = supplier.clone();
        conn.invoke("connect_push_supplier", move |e| {
            let _ = supplier.write_to(e);
        })?;
        Ok(())
    }

    /// `ProxyPullSupplier::connect_pull_consumer`. A nil `consumer` is legal
    /// and is what a consumer that only intends to pull should send: the
    /// reference is never dialled.
    pub fn connect_pull_consumer(conn: &mut Connection, consumer: &Ior) -> Result<()> {
        let consumer = consumer.clone();
        conn.invoke("connect_pull_consumer", move |e| {
            let _ = consumer.write_to(e);
        })?;
        Ok(())
    }

    /// `ProxyPullSupplier::disconnect_pull_supplier`.
    pub fn disconnect_pull_supplier(conn: &mut Connection) -> Result<()> {
        conn.invoke_nullary("disconnect_pull_supplier")?;
        Ok(())
    }

    /// `PullSupplier::pull` — blocks until an event is available.
    ///
    /// The `any` is the whole reply body, so its value runs to the end. That
    /// is the same reasoning `capture_event` uses from the other side, and it
    /// is the only way an `any`'s length can be known: CDR gives it no prefix.
    ///
    /// A server-side block that expires surfaces as `TIMEOUT` with
    /// `COMPLETED_NO`, which means no event was consumed — calling `pull`
    /// again is safe and is how a caller that really wants to wait forever
    /// waits forever.
    pub fn pull(conn: &mut Connection) -> Result<Any> {
        let reply = conn.invoke_nullary("pull")?;
        let mut body = reply.body()?;
        let tc = typecode::decode(&mut body)?;
        let endian = body.endian();
        let len = body.remaining();
        let value = body.get_bytes(len).map_err(crate::Error::Cdr)?.to_vec();
        Ok(Any { tc, value, endian })
    }

    /// `PullSupplier::try_pull` — `None` when the channel had nothing.
    ///
    /// The reply body is the `any` return value **followed by** the `out
    /// boolean`, in that order (§9.4.2: the return value precedes the out
    /// parameters). So the value ends one octet before the body does, and that
    /// octet is what says so — an `any` with no length prefix at the end of a
    /// body knows where it stops only because the caller knows what follows.
    pub fn try_pull(conn: &mut Connection) -> Result<Option<Any>> {
        let reply = conn.invoke_nullary("try_pull")?;
        let mut body = reply.body()?;
        let tc = typecode::decode(&mut body)?;
        let endian = body.endian();
        let len = body.remaining().checked_sub(1).ok_or(crate::Error::Cdr(
            orbweaver_cdr::Error::Malformed("a try_pull reply with no has_event flag"),
        ))?;
        let value = body.get_bytes(len).map_err(crate::Error::Cdr)?.to_vec();
        let has_event = body.get_bool().map_err(crate::Error::Cdr)?;
        Ok(has_event.then_some(Any { tc, value, endian }))
    }

    /// `ProxyPushSupplier::disconnect_push_supplier`.
    pub fn disconnect_push_supplier(conn: &mut Connection) -> Result<()> {
        conn.invoke_nullary("disconnect_push_supplier")?;
        Ok(())
    }

    /// `ProxyPushConsumer::disconnect_push_consumer`.
    pub fn disconnect_push_consumer(conn: &mut Connection) -> Result<()> {
        conn.invoke_nullary("disconnect_push_consumer")?;
        Ok(())
    }

    /// `ProxyPushConsumer::push` — marshals the `any` in place, so its
    /// internal padding is computed from where it really lands.
    pub fn push<F>(conn: &mut Connection, tc: &TypeCode, write_value: F) -> Result<()>
    where
        F: Fn(&mut Encoder),
    {
        // Fail here, where the error can be returned, rather than inside the
        // encoder closure where it could only be swallowed.
        typecode::encode(&mut Encoder::new(conn.endian()), tc)?;
        let tc = tc.clone();
        conn.invoke("push", move |e| {
            let _ = typecode::encode_any_with(e, &tc, |v| write_value(v));
        })?;
        Ok(())
    }

    /// A nil object reference: empty type id, no profiles (§9.3.6).
    pub fn nil_ref() -> Ior {
        Ior { type_id: String::new(), profiles: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::Server;
    use crate::{DEFAULT_MAX_MESSAGE_SIZE, Error};
    use orbweaver_cdr::Decoder;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};

    const T: Duration = Duration::from_secs(5);
    /// Short enough that a wedged delivery shows up as a test timeout rather
    /// than a five-second stall per attempt.
    const PUSH_T: Duration = Duration::from_millis(500);

    /// A channel served on loopback.
    ///
    /// `Server` serves its connections concurrently, so holding several at
    /// once is allowed; most tests here still dial each sub-object in turn
    /// and drop the previous connection because that is what a client does,
    /// not because the server requires it —
    /// `concurrent_suppliers_and_outbound_delivery_do_not_deadlock` is the
    /// one that deliberately holds them at the same time. Shutdown raises the
    /// stop flag and the serve loop notices it without needing a client to
    /// arrive; `shutdown` still takes the last client so the test's own
    /// ordering stays explicit.
    struct Served {
        channel: Ior,
        handle: ChannelHandle,
        delivery: Option<Delivery>,
        stats: crate::server::ServerStats,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Served {
        /// A channel with its delivery thread already running.
        fn start() -> Self {
            let mut served = Self::start_paused();
            served.begin_delivery();
            served
        }

        /// A channel with **no** delivery thread: pushes queue and stay there,
        /// which is the only way queue accounting can be measured without
        /// racing the drain.
        fn start_paused() -> Self {
            let server = Server::bind("127.0.0.1:0", b"EventChannel".to_vec()).unwrap();
            let port = server.local_addr().unwrap().port();
            let channel =
                Arc::new(EventChannelServer::new("127.0.0.1", port, b"EventChannel".to_vec()));
            let ior = channel.channel_ior();
            let handle = channel.handle();
            let stats = server.stats();
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            // `serve_shared`: the channel answers two calls at once now, which
            // is what makes the delivery/serving overlap tests below test the
            // thing they claim to.
            let thread = std::thread::spawn(move || {
                server.serve_shared(&*channel, move || flag.load(Ordering::SeqCst)).unwrap();
            });
            Served { channel: ior, handle, delivery: None, stats, stop, thread: Some(thread) }
        }

        /// Starts delivery against the already-serving channel's shared state.
        fn begin_delivery(&mut self) {
            let shared = Arc::clone(&self.handle.shared);
            let thread = std::thread::spawn(move || delivery_loop(shared, PUSH_T));
            self.delivery = Some(Delivery { handle: self.handle.clone(), thread: Some(thread) });
        }

        fn dial(&self, ior: &Ior) -> Connection {
            Connection::connect(ior, T).unwrap()
        }

        fn channel_conn(&self) -> Connection {
            self.dial(&self.channel)
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

        /// A `ProxyPushSupplier` with `consumer` attached.
        fn consumer_proxy(&self, consumer: &Ior) -> Ior {
            let mut conn = self.channel_conn();
            let admin = client::for_consumers(&mut conn).unwrap();
            drop(conn);
            let mut conn = self.dial(&admin);
            let proxy = client::obtain_push_supplier(&mut conn).unwrap();
            drop(conn);
            let mut conn = self.dial(&proxy);
            client::connect_push_consumer(&mut conn, consumer).unwrap();
            drop(conn);
            proxy
        }

        fn shutdown(mut self, last_client: Connection) {
            drop(self.delivery.take());
            self.stop.store(true, Ordering::SeqCst);
            drop(last_client);
            self.thread.take().unwrap().join().unwrap();
        }
    }

    /// A collecting consumer on a loopback server of its own.
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
                server.serve_shared(&*servant, || flag.load(Ordering::SeqCst)).unwrap();
            });
            Consumer { ior, sink, stop, thread: Some(thread) }
        }

        /// Stops the servant thread. The accept loop polls the flag, so no
        /// nudge connection is needed to unblock it any more.
        fn shutdown(mut self) {
            self.stop.store(true, Ordering::SeqCst);
            let _ = self.thread.take().unwrap().join();
        }
    }

    /// A reference to a port that was bound and then released: dialling it is
    /// refused immediately, which is what a dead consumer looks like without
    /// waiting out a connect timeout.
    fn dead_consumer_ior() -> Ior {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
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

    fn ulong_value(any: &Any) -> u32 {
        assert_eq!(any.tc, TypeCode::ULong);
        any.value_decoder().get_u32().unwrap()
    }

    /// Admins and proxies are distinct object keys on the one servant, and
    /// `knows` answers for all of them — F6's nesting shape. A key the channel
    /// never minted is not known, which is what makes `OBJECT_NOT_EXIST`
    /// reachable at all rather than a branch nothing takes.
    #[test]
    fn object_keys_route_to_admins_and_proxies() {
        let served = Served::start();
        let mut conn = served.channel_conn();

        let consumers = client::for_consumers(&mut conn).unwrap();
        let suppliers = client::for_suppliers(&mut conn).unwrap();
        assert_eq!(consumers.type_id, CONSUMER_ADMIN_ID);
        assert_eq!(suppliers.type_id, SUPPLIER_ADMIN_ID);
        let ck = consumers.primary().unwrap().object_key.clone();
        let sk = suppliers.primary().unwrap().object_key.clone();
        assert_ne!(ck, b"EventChannel", "an admin is not the channel again");
        assert_ne!(ck, sk, "the two admins are distinct objects");
        drop(conn);

        let mut conn = served.dial(&consumers);
        let pps1 = client::obtain_push_supplier(&mut conn).unwrap();
        let pps2 = client::obtain_push_supplier(&mut conn).unwrap();
        assert_eq!(pps1.type_id, PROXY_PUSH_SUPPLIER_ID);
        assert_ne!(
            pps1.primary().unwrap().object_key,
            pps2.primary().unwrap().object_key,
            "each obtain_push_supplier mints a fresh proxy"
        );
        drop(conn);

        let mut conn = served.dial(&suppliers);
        let ppc = client::obtain_push_consumer(&mut conn).unwrap();
        assert_eq!(ppc.type_id, PROXY_PUSH_CONSUMER_ID);
        drop(conn);

        for reachable in [&served.channel, &consumers, &suppliers, &pps1, &pps2, &ppc] {
            let mut c = served.dial(reachable);
            let reply = c.invoke_nullary("_non_existent").unwrap();
            assert!(!reply.body().unwrap().get_bool().unwrap());
        }

        let stranger = Ior {
            type_id: EVENT_CHANNEL_ID.into(),
            profiles: vec![IiopProfile {
                object_key: b"EventChannel/pps999".to_vec(),
                ..served.channel.primary().unwrap().clone()
            }],
        };
        let mut c = served.dial(&stranger);
        match c.invoke_nullary("_non_existent") {
            Err(Error::SystemException { id, .. }) => {
                assert_eq!(id, crate::server::OBJECT_NOT_EXIST);
            }
            other => panic!("expected OBJECT_NOT_EXIST for an unminted key, got {other:?}"),
        }
        served.shutdown(c);
    }

    /// `_is_a` answers for each object's own interface and for CORBA::Object,
    /// and for nothing else. Every ORB probes before it trusts a narrow.
    #[test]
    fn is_a_answers_per_object() {
        let served = Served::start();
        let mut conn = served.channel_conn();
        let consumers = client::for_consumers(&mut conn).unwrap();
        drop(conn);

        for (ior, own) in [(&served.channel, EVENT_CHANNEL_ID), (&consumers, CONSUMER_ADMIN_ID)] {
            let mut c = served.dial(ior);
            for (id, expected) in
                [(own, true), (CORBA_OBJECT_ID, true), (PROXY_PUSH_CONSUMER_ID, false)]
            {
                let reply = c.invoke("_is_a", move |e| e.put_str(id)).unwrap();
                assert_eq!(reply.body().unwrap().get_bool().unwrap(), expected, "{own} vs {id}");
            }
        }
        let last = served.channel_conn();
        served.shutdown(last);
    }

    /// The connect/disconnect state machine on both proxies: a second connect
    /// is `AlreadyConnected`, a nil consumer is `BAD_PARAM`, and `push` before
    /// connecting or after disconnecting is `Disconnected`.
    #[test]
    fn connect_and_disconnect_state_machine() {
        let served = Served::start();

        // ── ProxyPushConsumer, the supplier's end ──
        let mut conn = served.channel_conn();
        let suppliers = client::for_suppliers(&mut conn).unwrap();
        drop(conn);
        let mut conn = served.dial(&suppliers);
        let ppc = client::obtain_push_consumer(&mut conn).unwrap();
        drop(conn);

        let mut conn = served.dial(&ppc);
        match client::push(&mut conn, &TypeCode::ULong, |e| e.put_u32(1)) {
            Err(Error::UserException { id, .. }) => {
                assert_eq!(id, DISCONNECTED_ID, "push before connect must raise Disconnected");
            }
            other => panic!("expected Disconnected, got {other:?}"),
        }
        client::connect_push_supplier(&mut conn, &client::nil_ref()).unwrap();
        match client::connect_push_supplier(&mut conn, &client::nil_ref()) {
            Err(Error::UserException { id, .. }) => assert_eq!(id, ALREADY_CONNECTED_ID),
            other => panic!("expected AlreadyConnected, got {other:?}"),
        }
        client::push(&mut conn, &TypeCode::ULong, |e| e.put_u32(1)).unwrap();
        client::disconnect_push_consumer(&mut conn).unwrap();
        match client::push(&mut conn, &TypeCode::ULong, |e| e.put_u32(2)) {
            Err(Error::UserException { id, .. }) => assert_eq!(id, DISCONNECTED_ID),
            other => panic!("expected Disconnected after disconnect, got {other:?}"),
        }
        // Reconnectable: the key outlives the connection, as F6's unbound
        // contexts outlive their binding.
        client::connect_push_supplier(&mut conn, &client::nil_ref()).unwrap();
        drop(conn);

        // ── ProxyPushSupplier, the consumer's end ──
        let mut conn = served.channel_conn();
        let consumers = client::for_consumers(&mut conn).unwrap();
        drop(conn);
        let mut conn = served.dial(&consumers);
        let pps = client::obtain_push_supplier(&mut conn).unwrap();
        drop(conn);

        let consumer = dead_consumer_ior();
        let mut conn = served.dial(&pps);
        match client::connect_push_consumer(&mut conn, &client::nil_ref()) {
            Err(Error::SystemException { id, .. }) => {
                assert_eq!(id, "IDL:omg.org/CORBA/BAD_PARAM:1.0", "a nil consumer is BAD_PARAM");
            }
            other => panic!("expected BAD_PARAM, got {other:?}"),
        }
        client::connect_push_consumer(&mut conn, &consumer).unwrap();
        match client::connect_push_consumer(&mut conn, &consumer) {
            Err(Error::UserException { id, .. }) => assert_eq!(id, ALREADY_CONNECTED_ID),
            other => panic!("expected AlreadyConnected, got {other:?}"),
        }
        client::disconnect_push_supplier(&mut conn).unwrap();
        // Idempotent, and reconnectable afterwards.
        client::disconnect_push_supplier(&mut conn).unwrap();
        client::connect_push_consumer(&mut conn, &consumer).unwrap();
        served.shutdown(conn);
    }

    /// The pull model and `destroy` are refused loudly, not half-served.
    /// The deferral list, on the wire and in the predicate, after the pull
    /// model was split in two.
    ///
    /// What is still refused answers `NO_IMPLEMENT` — a decision a client can
    /// read. What is no longer refused answers with a *reference*, which is
    /// the only unambiguous evidence that the arm exists: an operation removed
    /// from [`is_deferred`] without an arm behind it would degrade to
    /// `BAD_OPERATION`, and this test would see the difference.
    #[test]
    fn the_supplier_side_of_pull_and_destroy_answer_no_implement() {
        for op in
            ["obtain_pull_consumer", "connect_pull_supplier", "disconnect_pull_consumer", "destroy"]
        {
            assert!(super::is_deferred(op), "{op} must stay on the deferral list");
        }
        for op in [
            "obtain_pull_supplier",
            "connect_pull_consumer",
            "pull",
            "try_pull",
            "disconnect_pull_supplier",
        ] {
            assert!(!super::is_deferred(op), "{op} is served now and must be off the list");
        }

        let served = Served::start();
        let mut conn = served.channel_conn();
        let consumers = client::for_consumers(&mut conn).unwrap();
        let suppliers = client::for_suppliers(&mut conn).unwrap();

        for op in ["destroy", "obtain_pull_consumer", "disconnect_pull_consumer"] {
            match conn.invoke_nullary(op) {
                Err(Error::SystemException { id, .. }) => {
                    assert_eq!(id, crate::server::NO_IMPLEMENT, "{op}");
                }
                other => panic!("expected NO_IMPLEMENT for {op}, got {other:?}"),
            }
        }
        drop(conn);

        // The ConsumerAdmin mints a pull supplier now; the SupplierAdmin's own
        // pull operation is still a stated refusal, and an operation belonging
        // to neither admin is `BAD_OPERATION` — the distinction that made
        // `NO_IMPLEMENT` worth having.
        let mut c = served.dial(&consumers);
        let pls = client::obtain_pull_supplier(&mut c).unwrap();
        assert_eq!(pls.type_id, PROXY_PULL_SUPPLIER_ID);
        assert_ne!(
            pls.primary().unwrap().object_key,
            served.channel.primary().unwrap().object_key,
            "a pull proxy is its own object"
        );
        drop(c);

        let mut c = served.dial(&suppliers);
        match c.invoke_nullary("obtain_pull_consumer") {
            Err(Error::SystemException { id, .. }) => {
                assert_eq!(id, crate::server::NO_IMPLEMENT);
            }
            other => panic!("expected NO_IMPLEMENT for obtain_pull_consumer, got {other:?}"),
        }
        match c.invoke_nullary("obtain_pull_supplier") {
            Err(Error::SystemException { id, .. }) => {
                assert_eq!(
                    id,
                    crate::server::BAD_OPERATION,
                    "a SupplierAdmin does not declare obtain_pull_supplier"
                );
            }
            other => panic!("expected BAD_OPERATION, got {other:?}"),
        }
        served.shutdown(c);
    }

    /// An `any` survives the whole path — the supplier's client, our servant's
    /// verbatim capture, the delivery thread's relay, our consumer servant —
    /// **in both byte orders**. An encoder that only works native-endian
    /// passes every local test and fails in the field.
    #[test]
    fn any_payloads_relay_in_both_byte_orders() {
        for endian in [Endian::Big, Endian::Little] {
            let served = Served::start();
            let consumer = Consumer::start(b"Consumer");
            served.consumer_proxy(&consumer.ior);

            let mut conn = served.supplier_proxy();
            conn.set_endian(endian);
            for i in 0..4u32 {
                client::push(&mut conn, &TypeCode::ULong, move |e| e.put_u32(0xABC0 + i)).unwrap();
            }
            // A string too: its length prefix is what notices a byte order
            // swapped between capture and relay.
            client::push(&mut conn, &TypeCode::String(0), |e| e.put_str("함정")).unwrap();

            assert!(
                served.handle.wait_until(T, |s| s.delivered == 5),
                "{endian:?}: {:?}",
                served.handle.stats()
            );
            let got = consumer.sink.snapshot();
            assert_eq!(got.len(), 5, "{endian:?}");
            let numbers: Vec<u32> = got[..4].iter().map(ulong_value).collect();
            assert_eq!(numbers, vec![0xABC0, 0xABC1, 0xABC2, 0xABC3], "{endian:?}: order or value");
            assert_eq!(got[4].tc, TypeCode::String(0), "{endian:?}");
            assert_eq!(got[4].value_decoder().get_string().unwrap(), "함정", "{endian:?}");
            assert_eq!(got[4].endian, endian, "{endian:?}: the relay kept the source byte order");

            let stats = served.handle.stats();
            assert_eq!(stats.accepted, 5, "{endian:?}");
            assert_eq!(stats.dropped, 0, "{endian:?}");
            assert_eq!(stats.unrelayable, 0, "{endian:?}");
            assert_eq!(stats.push_failures, 0, "{endian:?}");

            served.shutdown(conn);
            consumer.shutdown();
        }
    }

    /// A sleeping, deadline-bounded rendezvous: `true` when all `n` parties
    /// arrived, `false` when they did not. A spin here would be the harness
    /// rule's wait loop that does not wait.
    fn all_arrived(n: usize, count: &std::sync::atomic::AtomicUsize, within: Duration) -> bool {
        count.fetch_add(1, Ordering::SeqCst);
        let deadline = Instant::now() + within;
        while count.load(Ordering::SeqCst) < n {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        true
    }

    /// A consumer that **stops inside `push`** until it is let go.
    ///
    /// It exists to hold the channel's delivery thread inside an outbound
    /// invocation on purpose, so that the inbound side can be tested while
    /// that call is genuinely in flight. Everything about the hazard depends
    /// on those two things being simultaneous, and a fast consumer cannot
    /// guarantee they ever are.
    struct BlockingConsumer {
        key: Vec<u8>,
        entered: Arc<std::sync::atomic::AtomicUsize>,
        release: Arc<AtomicBool>,
    }

    impl crate::server::SharedDispatch for BlockingConsumer {
        fn knows(&self, object_key: &[u8]) -> bool {
            object_key == self.key
        }

        fn dispatch(
            &self,
            request: &Request,
            out: &mut Encoder,
        ) -> std::result::Result<(), SystemException> {
            let mut args = request.body().map_err(|_| SystemException::marshal())?;
            match request.operation.as_str() {
                "_is_a" => {
                    let id = args.get_string().map_err(|_| SystemException::marshal())?;
                    out.put_bool(id == PUSH_CONSUMER_ID || id == CORBA_OBJECT_ID);
                }
                "_non_existent" => out.put_bool(false),
                "push" => {
                    self.entered.fetch_add(1, Ordering::SeqCst);
                    // Deadline-bounded: a test that forgets to release must
                    // fail, not hang.
                    let until = Instant::now() + T;
                    while !self.release.load(Ordering::SeqCst) && Instant::now() < until {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
                "disconnect_push_consumer" => {}
                _ => return Err(SystemException::bad_operation()),
            }
            Ok(())
        }
    }

    /// **The hazard, pinned.** An inbound `push` must be served while the
    /// channel's own outbound `push` is blocked.
    ///
    /// This is rule 1 of the module docs stated as an experiment rather than
    /// as an intention. The delivery thread is parked inside an outbound
    /// invocation — really inside it, witnessed by the consumer's own counter,
    /// not inferred from a sleep — and while it is parked, S suppliers push
    /// into the channel concurrently. If any lock were held across that
    /// outbound call, every one of them would block until the consumer let go,
    /// and the deadline would fail the test instead of hanging it.
    ///
    /// Concurrent dispatch is what makes this worth re-testing: the servant
    /// side is no longer single-file, so an inbound `push` and an outbound one
    /// are now ordinarily simultaneous rather than rarely so.
    #[test]
    fn an_inbound_push_is_served_while_an_outbound_push_is_blocked() {
        const S: usize = 3;
        let entered = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let release = Arc::new(AtomicBool::new(false));

        let server = Server::bind("127.0.0.1:0", b"BlockingConsumer".to_vec()).unwrap();
        let port = server.local_addr().unwrap().port();
        let consumer_ior = Ior {
            type_id: PUSH_CONSUMER_ID.into(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: "127.0.0.1".into(),
                port,
                object_key: b"BlockingConsumer".to_vec(),
                components: Vec::new(),
            }],
        };
        let servant = Arc::new(BlockingConsumer {
            key: b"BlockingConsumer".to_vec(),
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let consumer_thread = std::thread::spawn(move || {
            server.serve_shared(&*servant, || flag.load(Ordering::SeqCst)).unwrap();
        });

        // A channel whose push timeout is long enough that the blocked
        // consumer is *held*, not timed out from under the test.
        let served = Served::start_paused();
        let shared = Arc::clone(&served.handle.shared);
        let delivery = std::thread::spawn(move || delivery_loop(shared, T));
        served.consumer_proxy(&consumer_ior);

        // One event, which the delivery thread will carry into the consumer
        // and get stuck in.
        served.handle.publish(&TypeCode::ULong, Endian::Big, |e| e.put_u32(1)).unwrap();
        let until = Instant::now() + T;
        while entered.load(Ordering::SeqCst) == 0 && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(entered.load(Ordering::SeqCst), 1, "the outbound push never started");

        // ── the assertion: inbound work proceeds while that call is stuck ──
        std::thread::scope(|scope| {
            for s in 0..S {
                let served = &served;
                scope.spawn(move || {
                    let mut proxy = served.supplier_proxy();
                    client::push(&mut proxy, &TypeCode::ULong, move |e| e.put_u32(100 + s as u32))
                        .expect("an inbound push must not wait for an outbound one");
                });
            }
        });

        release.store(true, Ordering::SeqCst);
        assert!(
            served.handle.wait_until(T, |st| st.accepted > S as u64),
            "the channel did not take every inbound event: {:?}",
            served.handle.stats()
        );

        let last = served.channel_conn();
        // Stop the delivery loop explicitly and join it: a test that leaves a
        // thread pushing into a torn-down fixture is the harness rule about
        // unmeasured things in another costume.
        served.handle.stop();
        delivery.join().unwrap();
        served.shutdown(last);
        stop.store(true, Ordering::SeqCst);
        consumer_thread.join().unwrap();
    }

    /// Rule 1 of the module docs — no lock may be held across an outbound
    /// call — against a server that now serves its connections concurrently.
    ///
    /// Several suppliers hold their own sessions on the channel and push *in*
    /// while the delivery thread pushes *out* to a consumer that is itself a
    /// server in this process. Before this batch the arrangement was
    /// impossible to even set up: one connection was served at a time, so the
    /// suppliers had to take turns. The deadline is what makes a resurrected
    /// deadlock a failed test rather than a hung suite.
    ///
    /// What is proved is that inbound serving and outbound delivery overlap
    /// without deadlocking. Since stream E's second batch the channel is
    /// served through `serve_shared`, so the suppliers' `push` calls really do
    /// enter the servant together rather than taking turns behind the server's
    /// mutex — but this test does not *witness* that, and does not claim to.
    /// `an_inbound_push_is_served_while_an_outbound_push_is_blocked` below is
    /// the one that pins it, by blocking the outbound call on purpose.
    #[test]
    fn concurrent_suppliers_and_outbound_delivery_do_not_deadlock() {
        const S: usize = 4;
        const EACH: u32 = 5;
        let served = Served::start();
        let consumer = Consumer::start(b"ConcurrentConsumer");
        served.consumer_proxy(&consumer.ior);

        let arrived = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for s in 0..S {
                let served = &served;
                let arrived = &arrived;
                scope.spawn(move || {
                    // Three hops, each on its own connection, run by every
                    // supplier at the same time.
                    let mut proxy = served.supplier_proxy();
                    assert!(all_arrived(S, arrived, T), "supplier {s} never overlapped the others");
                    for i in 0..EACH {
                        client::push(&mut proxy, &TypeCode::ULong, move |e| {
                            e.put_u32(s as u32 * 100 + i)
                        })
                        .unwrap();
                    }
                });
            }
        });

        let want = u64::from(EACH) * S as u64;
        assert!(
            served.handle.wait_until(T, |st| st.delivered == want),
            "delivery stalled under concurrency: {:?}",
            served.handle.stats()
        );
        let got = consumer.sink.snapshot();
        assert_eq!(got.len() as u64, want, "every concurrently pushed event must arrive");
        let mut values: Vec<u32> = got.iter().map(ulong_value).collect();
        values.sort_unstable();
        let expected: Vec<u32> =
            (0..S).flat_map(|s| (0..EACH).map(move |i| s as u32 * 100 + i)).collect();
        assert_eq!(values, expected, "an event was lost or duplicated");
        assert!(
            served.stats.peak_active() >= S as u64,
            "the suppliers did not actually overlap: peak was {}",
            served.stats.peak_active()
        );

        let last = served.channel_conn();
        served.shutdown(last);
        consumer.shutdown();
    }

    /// The bounded queue, measured with the drain paused so the accounting
    /// cannot race it: the oldest events go, every discard is counted, and
    /// what survives once delivery starts is the newest.
    #[test]
    fn a_full_queue_drops_the_oldest_counts_it_and_keeps_the_newest() {
        let mut served = Served::start_paused();
        served.handle.set_queue_limit(3);
        let consumer = Consumer::start(b"Newest");
        served.consumer_proxy(&consumer.ior);

        let mut conn = served.supplier_proxy();
        for i in 0..9u32 {
            client::push(&mut conn, &TypeCode::ULong, move |e| e.put_u32(i)).unwrap();
        }
        let stats = served.handle.stats();
        assert_eq!(stats.accepted, 9);
        assert_eq!(stats.queued, 3, "the queue is bounded");
        assert_eq!(stats.dropped, 6, "every discarded event is counted, none silently");
        assert_eq!(stats.delivered, 0, "no delivery thread is running yet");

        served.begin_delivery();
        assert!(served.handle.wait_until(T, |s| s.delivered == 3), "{:?}", served.handle.stats());
        let values: Vec<u32> = consumer.sink.snapshot().iter().map(ulong_value).collect();
        assert_eq!(values, vec![6, 7, 8], "drop-oldest keeps the tail");
        assert_eq!(served.handle.stats().dropped, 6, "draining adds no drops");

        served.shutdown(conn);
        consumer.shutdown();
    }

    /// A dead consumer must not wedge the channel: it is disconnected after
    /// the documented threshold, its backlog is counted as dropped, and the
    /// live consumer beside it receives everything, in order.
    ///
    /// Delivery starts only once both queues are full, so the failure count
    /// and the abandoned backlog are exact rather than a race against how
    /// fast a refused connect returns.
    #[test]
    fn a_dead_consumer_is_disconnected_without_stopping_the_live_one() {
        let mut served = Served::start_paused();
        let live = Consumer::start(b"Live");
        served.consumer_proxy(&live.ior);
        served.consumer_proxy(&dead_consumer_ior());

        let mut conn = served.supplier_proxy();
        for i in 0..6u32 {
            client::push(&mut conn, &TypeCode::ULong, move |e| e.put_u32(i)).unwrap();
        }
        assert_eq!(served.handle.stats().queued, 12, "six events for each of two proxies");

        served.begin_delivery();
        assert!(
            served.handle.wait_until(T, |s| s.delivered == 6 && s.disconnected_for_failure == 1),
            "{:?}",
            served.handle.stats()
        );
        let values: Vec<u32> = live.sink.snapshot().iter().map(ulong_value).collect();
        assert_eq!(values, vec![0, 1, 2, 3, 4, 5], "delivery to the live consumer is in order");

        let stats = served.handle.stats();
        assert_eq!(stats.push_failures, u64::from(MAX_CONSECUTIVE_FAILURES));
        assert_eq!(stats.dropped, 3, "the three still queued when it was cut, counted");
        assert_eq!(stats.consumers_connected, 1, "only the live proxy is still attached");
        assert_eq!(stats.unrelayable, 0);

        served.shutdown(conn);
        live.shutdown();
    }

    /// In-process publishing reaches the same consumers over the same relay,
    /// with no loopback socket on the supplier side — the path F3 and F4 use.
    #[test]
    fn publish_delivers_without_a_supplier_socket() {
        let served = Served::start();
        let consumer = Consumer::start(b"Published");
        served.consumer_proxy(&consumer.ior);

        for i in 0..3u32 {
            served
                .handle
                .publish(&TypeCode::ULong, Endian::Big, move |e| e.put_u32(i * 11))
                .unwrap();
        }
        assert!(served.handle.wait_until(T, |s| s.delivered == 3), "{:?}", served.handle.stats());
        assert!(served.handle.wait_idle(T));
        let values: Vec<u32> = consumer.sink.snapshot().iter().map(ulong_value).collect();
        assert_eq!(values, vec![0, 11, 22]);
        assert_eq!(served.handle.stats().unrelayable, 0);

        let last = served.channel_conn();
        served.shutdown(last);
        consumer.shutdown();
    }

    /// The narrow `dispatch` entry point cannot carry a user exception, so it
    /// maps one to the standard UNKNOWN. Direct call — no server involved.
    #[test]
    fn a_user_exception_through_plain_dispatch_maps_to_unknown() {
        let channel = EventChannelServer::new("127.0.0.1", 1, b"EventChannel".to_vec());
        // A minted but unconnected ProxyPushConsumer: pushing into it raises
        // Disconnected, which this entry point cannot carry.
        let key = channel.mint("ppc");
        channel.shared.lock().proxy_consumers.insert(key.clone(), ProxyConsumer::default());

        let wire =
            crate::encode_request(Version::V1_2, Endian::Little, 7, &key, "push", true, |e| {
                let _ = typecode::encode_any_with(e, &TypeCode::ULong, |v| v.put_u32(1));
            })
            .unwrap();
        let msg = crate::read_message(&mut &wire[..], DEFAULT_MAX_MESSAGE_SIZE).unwrap();
        let req = crate::server::decode_request(msg).unwrap();

        let mut out = Encoder::new(Endian::Little);
        let err = channel.dispatch(&req, &mut out).unwrap_err();
        assert_eq!(err.id, crate::server::UNKNOWN);
        assert_eq!(err.minor, 0x4f4d_0001, "OMGVMCID | 1: unlisted user exception");
    }

    /// The invariant the verbatim relay rests on: a captured `any`'s value
    /// bytes are exactly what an in-place marshalling at the same alignment
    /// produces — checked with a payload whose internal padding depends on
    /// the alignment origin (an octet before an 8-byte value), in both byte
    /// orders.
    #[test]
    fn a_captured_any_matches_an_in_place_marshalling() {
        for endian in [Endian::Big, Endian::Little] {
            let tc = TypeCode::ULongLong;
            let wire = crate::encode_request(Version::V1_2, endian, 1, b"k", "push", true, |e| {
                let _ = typecode::encode_any_with(e, &tc, |v| {
                    v.put_u8(0xEE);
                    v.put_u64(0x0102_0304_0506_0708);
                });
            })
            .unwrap();
            let msg = crate::read_message(&mut &wire[..], DEFAULT_MAX_MESSAGE_SIZE).unwrap();
            let req = crate::server::decode_request(msg).unwrap();
            let mut args = req.body().unwrap();
            let Ok(event) = capture_event(&mut args) else { panic!("{endian:?}: capture failed") };
            // The any starts at an 8-aligned 1.2 body and a simple TypeCode
            // is one u32 kind, so the value began 4 past an 8-boundary.
            assert_eq!(event.value_align, 4, "{endian:?}");

            let mut rebuilt = Encoder::new(endian);
            typecode::encode(&mut rebuilt, &tc).unwrap();
            let tc_len = rebuilt.len();
            rebuilt.put_u8(0xEE);
            rebuilt.put_u64(0x0102_0304_0506_0708);
            let bytes = rebuilt.finish().unwrap();
            assert_eq!(event.any.value, bytes[tc_len..], "{endian:?}: padding differs");

            // Read the value back *at its captured alignment* — from offset
            // zero its internal padding would be misread, which is the whole
            // reason value_align is recorded.
            let mut placed = vec![0u8; event.value_align];
            placed.extend_from_slice(&event.any.value);
            let mut d = Decoder::new(&placed, endian);
            d.seek_to(event.value_align).unwrap();
            assert_eq!(d.get_u8().unwrap(), 0xEE);
            assert_eq!(d.get_u64().unwrap(), 0x0102_0304_0506_0708);
        }
    }
}
