//! First-party CosEvent channel: `CosEventChannelAdmin` plus the
//! `CosEventComm` push pair, and — since the two deferrals below were
//! re-measured — **both sides of the pull model** (PLAN-SERVICES §4), which is
//! all four of CosEvent's models.
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
//! | `ProxyPullConsumer` | `connect_pull_supplier`, `disconnect_pull_consumer` |
//!
//! plus `_is_a`/`_non_existent` on every one of them, because every ORB probes
//! before it trusts a narrow. Each admin and each proxy is a **distinct object
//! key on this one servant**, exactly as F6's nested contexts are — see
//! [`Dispatch::knows`], which is answered from the proxy tables.
//!
//! # The four models, and the one shape that carried two of them
//!
//! CosEvent's models are a 2×2 — each side is either pushed to or pulled from,
//! and this channel is the other half of both — selected by which pair of
//! `obtain_*` operations a client calls:
//!
//! | Supplier side | Consumer side | Operations |
//! |---|---|---|
//! | push | push | `obtain_push_consumer` + `obtain_push_supplier` |
//! | push | pull | `obtain_push_consumer` + `obtain_pull_supplier` |
//! | pull | push | `obtain_pull_consumer` + `obtain_push_supplier` |
//! | pull | pull | `obtain_pull_consumer` + `obtain_pull_supplier` |
//!
//! Two of the four were blocked by exactly one shape — the channel acting as a
//! *pull consumer of a supplier* — and closing it closed both at once. It was
//! never four pieces of work; the [source loop](source_loop) is the whole of
//! it, and every model above is walked over real sockets by
//! `tests/event_pull_supplier_model.rs`.
//!
//! # What is refused, and why
//!
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
//!    counted in the same [`ChannelStats::dropped_overflow`] and logged per
//!    event. The
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
//!    `pull` alike; the refused event is discarded and counted in
//!    [`ChannelStats::unrelayable`], which is both the refusal counter and
//!    this cause's share of [`ChannelStats::dropped`], and the
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
//! # Pulling *from* a supplier: the outbound direction, and its three answers
//!
//! A `ProxyPullConsumer` is the channel's inbound half from a supplier that
//! must be **asked**. It is the only place this module is a client of an
//! interface it does not itself serve, so it answers three questions a reader
//! will ask, and the answers are the reason the deferral's own v1 sketch —
//! *"one thread per connected supplier"* — was not followed.
//!
//! 1. **`try_pull`, never `pull`.** `CosEventComm::PullSupplier::pull` is
//!    specified to block until the supplier has something; `try_pull` answers
//!    at once. A channel that blocked in `pull` would hold a thread on
//!    somebody else's clock with no bound it owns, and — because the round is
//!    shared — one silent supplier would starve every other supplier of the
//!    channel's attention. That is rule 2 of the fan-out section below seen in
//!    a mirror: the failure this module is built around avoiding is one slow
//!    peer wedging the channel for everybody else. So the channel spends the
//!    other side of that trade instead, an invented interval
//!    ([`DEFAULT_SOURCE_POLL`], moved by [`ChannelHandle::set_source_poll`]):
//!    latency and wasted invocations, both bounded and both this channel's own
//!    to pay. `pull_calls` on [`EventSource`] is how a test *measures* that the
//!    channel never calls `pull` rather than taking this paragraph's word.
//!
//!    One thread, round-robin from [`ChannelState::source_cursor`], rather
//!    than the sketch's one per supplier: with nothing to block on there is
//!    nothing for the extra threads to do, and a fixed thread count is a bound
//!    the channel owns where a per-connection count is one a client sets. A
//!    round that yields an event continues immediately, so a backlog drains at
//!    socket speed; only a **barren** round — every connected supplier asked,
//!    none had anything — sleeps out the interval.
//! 2. **An unreachable or slow supplier is governed by
//!    [`MAX_CONSECUTIVE_FAILURES`], the same rule and the same counter as a
//!    dead consumer.** Slowness is bounded by the same outbound timeout the
//!    push direction uses ([`DEFAULT_PUSH_TIMEOUT`], the socket read timeout),
//!    and after three consecutive failed `try_pull`s the proxy is released as
//!    though `disconnect_pull_consumer` had been called, counted in
//!    [`ChannelStats::disconnected_for_failure`]. Failed *invocations* are
//!    counted in [`ChannelStats::pull_failures`] and not in
//!    [`ChannelStats::push_failures`]: one number says what the channel could
//!    not send, the other what it could not fetch, and summing them would hide
//!    which half is broken. **No drop cause joins the split for this**, because
//!    there is nothing to drop: a `ProxyPullConsumer` holds no queue at all —
//!    an event it fetches goes straight into [`ChannelState::fan_out`] and is
//!    thereafter accounted for exactly like a pushed one. A supplier that
//!    answers the user exception `Disconnected` is released immediately and is
//!    **not** counted as a failure; it did not fail, it said it was finished.
//! 3. **A supplier connection does not survive [`ChannelHandle::stop`], and
//!    the connectedness does.** The source thread returns on the stop flag and
//!    its sockets close with it, exactly as the delivery thread's do. What
//!    stays is the proxy's `connected` flag and its supplier reference, which
//!    is the same choice the push proxies make and for the same reason: the
//!    object key stays known and reconnectable. Nothing is discarded on this
//!    account and so [`ChannelStats::split_adds_up`] cannot be disturbed by
//!    it — `stop`'s [`DropCause::Stop`] tally counts proxy *queues*, and this
//!    proxy has none.
//! 4. **A disconnect stops the asking, and "stops" carries a stated bound
//!    rather than a hope.** `disconnect_pull_consumer` clears the proxy's flag
//!    under the state lock and returns. It does **not** wait for the source
//!    thread, and that was decided rather than overlooked: the thread it would
//!    wait for is blocked in an outbound call to the very supplier whose
//!    process is free to be the one calling the disconnect, so a disconnect
//!    that waited would be rule 1 of the fan-out section seen in a mirror — a
//!    servant held for an outbound timeout by the peer it is answering, with
//!    no timeout of its own to break it. A disconnect that can cost
//!    [`DEFAULT_PUSH_TIMEOUT`] is a worse property than a stray call. What is
//!    guaranteed instead is this, stated exactly because a stray call has
//!    already cost one CI failure at each end of it:
//!
//!    > The source loop's **commit point** is
//!    > [`ChannelState::source_still_wanted`], taken under the state lock with
//!    > no I/O between it and the request going out.
//!    > `disconnect_pull_consumer` and [`ChannelHandle::stop`] take that same
//!    > lock. So once either of them has returned, the only `try_pull` that
//!    > can still reach that supplier is one whose commit point had **already
//!    > been passed**; every later round is cancelled where it stands, costs
//!    > nothing, and is counted in [`ChannelStats::pull_rounds_cancelled`].
//!    > One source thread runs one round at a time, so that is **at most one
//!    > further call, landing within the outbound timeout** — never a stream,
//!    > and never a second one.
//!
//!    A caller in this process waits that one round out with
//!    [`ChannelHandle::wait_source_idle`]. A caller over the wire — a supplier
//!    tearing itself down, which is the case that matters — has the time bound
//!    and nothing else, which is why `spikes/event_pull_supplier.py` waits it
//!    out before it lets its ORB go.
//!
//!    Both halves of what this replaced were measured on CI Linux under five
//!    concurrent whole-suite runs and neither has ever reproduced on macOS
//!    (20 serial runs, 5 concurrent, and a 200 µs source poll): a Rust test
//!    sampled a counter an instant before the late call landed, and the Python
//!    fixture printed `PASS` and then aborted, because the late call reached a
//!    servant whose interpreter had already begun clearing its module globals.
//!    One window, two costumes — which is why the repair is one predicate and
//!    not two patches.
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
//!    **oldest** event is dropped, counted in
//!    [`ChannelStats::dropped_overflow`] and logged. Never silently — the
//!    harness rule about unmeasured checks applies to discarded data too.
//!    Slowness is bounded by the push timeout, which is the socket read
//!    timeout on the outbound connection.
//! 3. **Repeated failure disconnects.** After
//!    [`MAX_CONSECUTIVE_FAILURES`] consecutive failed pushes the proxy is
//!    disconnected as though `disconnect_push_supplier` had been called: its
//!    consumer reference is released, its queued events are dropped (counted
//!    in [`ChannelStats::dropped_on_failure_disconnect`]), and a line is
//!    logged. Three, not one: a single failure is a transport
//!    hiccup, and a consumer that has restarted deserves the two retries a
//!    fresh connect gets. The proxy object key stays alive and reconnectable,
//!    the same choice F6 made for unbound contexts.
//!
//! # Counting a discard: five causes, and what a rate can be taken over
//!
//! Every discarded event is counted in [`ChannelStats::dropped`], and until
//! this batch that was all anyone could learn about it. Five different things
//! moved that one number:
//!
//! | Counter | Cause | Class |
//! |---|---|---|
//! | [`ChannelStats::dropped_overflow`] | a bounded queue was full | **back-pressure** |
//! | [`ChannelStats::unrelayable`] | `relay_check` refused the `any` | our own limitation |
//! | [`ChannelStats::dropped_on_disconnect`] | a consumer hung up | housekeeping |
//! | [`ChannelStats::dropped_on_failure_disconnect`] | a failing proxy was cut | a dead consumer |
//! | [`ChannelStats::dropped_at_stop`] | [`ChannelHandle::stop`] | housekeeping |
//!
//! Back-pressure means a producer faster than a consumer, or a bound sized for
//! a slower one; a dead consumer is not the same thing as a slow one, and
//! neither is a shutdown.
//!
//! So a tidy shutdown moved the same counter as an overloaded consumer, and
//! `PLAN-DEFERRED.md` §1's un-defer trigger for CosNotification — *"F7 reports
//! a measured drop rate caused by unwanted fan-out"* — had no instrument that
//! could answer it in **either** direction. A trigger with no instrument is
//! worse than an unmeasured one, because it reads as measured. Splitting the
//! counter is D011 §6.1's finding, fixed.
//!
//! ## What a drop rate can be taken over
//!
//! A rate needs a denominator and there are two, which answer different
//! questions:
//!
//! - [`ChannelStats::accepted`] — what suppliers handed in. One per `push`,
//!   whatever it fans out to. `dropped / accepted` can exceed 1 and means
//!   nothing on its own.
//! - [`ChannelStats::fanned_out`] — the per-proxy copies those events became,
//!   one per connected proxy per accepted event. This is the denominator the
//!   per-proxy bound is spent against, so `dropped_overflow / fanned_out` is
//!   the honest **back-pressure drop rate**, and `fanned_out / accepted` is
//!   the fan-out multiplication itself.
//!
//! ## What these numbers cannot say
//!
//! Two limits, stated because the trigger above is phrased in terms of both.
//!
//! 1. **They are channel-wide, not per consumer.** Every counter here lives in
//!    one [`ChannelStats`]; the queues are per proxy but the accounting is
//!    not. "Which consumer is dropping" is a question this shape cannot
//!    answer, and answering it means per-proxy counters and a way to publish
//!    them — a design change, not a counter. Nothing here guesses at it by
//!    dividing by [`ChannelStats::consumers_connected`], which would be a
//!    fabricated attribution that happens to have a plausible magnitude.
//! 2. **"Unwanted" is not observable in this servant at all.** `CosEventComm`
//!    has no subscription predicate: a connected consumer receives everything
//!    the channel accepts, so there is nothing anywhere in this module that
//!    records what a consumer *wanted*. A drop can be attributed to
//!    back-pressure — that is what the split buys — but never to fan-out
//!    being unwanted, because wanting is not represented. That capability is
//!    precisely what `CosNotification`'s filters would add, which makes §1's
//!    trigger circular as written: it asks this channel to measure the thing
//!    the deferred chapter exists to introduce.
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
/// Repository id of `CosEventChannelAdmin::ProxyPullConsumer` — the object a
/// *supplier* hands its `PullSupplier` to so the channel will come and ask.
pub const PROXY_PULL_CONSUMER_ID: &str = "IDL:omg.org/CosEventChannelAdmin/ProxyPullConsumer:1.0";
/// Repository id of `CosEventComm::PullSupplier`, the reference a supplier
/// hands us and the interface [`PullSupplierServant`] implements.
///
/// Unlike a `PullConsumer` this one **is** dialled — it is the whole of the
/// pull supplier model — so a nil one is `BAD_PARAM`, the same answer and for
/// the same reason as a nil `PushConsumer`.
pub const PULL_SUPPLIER_ID: &str = "IDL:omg.org/CosEventComm/PullSupplier:1.0";
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

/// Consecutive failed outbound calls after which a proxy releases its peer.
///
/// One failure is a hiccup and two cover a peer that restarted between them,
/// since every attempt after a failure redials. Three consecutive failures is
/// a peer that is not coming back on this reference.
///
/// **One rule, both directions.** It governs a `ProxyPushSupplier` whose
/// consumer will not take a `push` and a `ProxyPullConsumer` whose supplier
/// will not answer a `try_pull`, and both land in
/// [`ChannelStats::disconnected_for_failure`]: "this channel gave up on a
/// peer" is one fact, and splitting it by direction would make the number
/// nobody wants two numbers nobody reads.
pub const MAX_CONSECUTIVE_FAILURES: u32 = 3;

/// Connect and reply timeout for an outbound `push`, and for the outbound
/// `try_pull` the source loop makes.
///
/// This is what bounds a *slow* peer: the thread that made the call can be
/// held for at most this long by any one of them, and the servant thread is
/// never held at all.
pub const DEFAULT_PUSH_TIMEOUT: Duration = Duration::from_secs(2);

/// How long the source loop sleeps after a **barren** round — every connected
/// supplier asked, none of them holding an event.
///
/// The interval a channel that polls has to invent, and the price of choosing
/// `try_pull` over a blocking `pull`: latency for an event that arrives just
/// after a round, and one wasted invocation per idle supplier per interval. A
/// round that yields anything does not sleep at all, so this bounds idle cost
/// rather than throughput. Moved by [`ChannelHandle::set_source_poll`].
pub const DEFAULT_SOURCE_POLL: Duration = Duration::from_millis(100);

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

/// A `ProxyPullConsumer`: the channel's inbound half from one supplier that
/// has to be **asked** rather than one that calls.
///
/// The mirror of a [`ProxySupplier`], and deliberately the same three fields:
/// a peer reference this channel dials, and a consecutive-failure count that
/// [`MAX_CONSECUTIVE_FAILURES`] cuts. What it does **not** have is a queue —
/// an event fetched here goes straight into [`ChannelState::fan_out`], so
/// there is never a backlog belonging to this proxy to abandon, and no drop
/// cause joins the split on its account.
#[derive(Debug, Default)]
struct ProxyPullConsumer {
    /// Whether `connect_pull_supplier` has been called.
    ///
    /// Its own flag rather than `supplier.is_some()` for consistency with
    /// every other proxy here, even though a nil supplier is refused: the
    /// supplier reference can be cleared by a failure disconnect, and
    /// "connected" and "has an address" are still two different facts.
    connected: bool,
    /// The supplier's `PullSupplier` reference. Dialled — that is the whole
    /// point of this proxy — so a nil one was refused at `connect`.
    supplier: Option<Ior>,
    consecutive_failures: u32,
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
    ///
    /// One per `push` or [`ChannelHandle::publish`], whatever it fans out to.
    /// It is therefore *not* the denominator of a per-consumer drop rate —
    /// `fanned_out` is. See the module docs, "What a drop rate can be taken
    /// over".
    pub accepted: u64,
    /// Queue slots filled by fan-out: one per connected proxy per accepted
    /// event, push and pull alike.
    ///
    /// The denominator `dropped_overflow` needs. `accepted` counts what
    /// suppliers handed in; this counts the copies the channel made of them,
    /// which is what the per-proxy bound is spent on, and
    /// `fanned_out / accepted` is the multiplication itself.
    pub fanned_out: u64,
    /// Successful outbound `push` invocations.
    pub delivered: u64,
    /// **Every** queued event this channel discarded, whatever the reason:
    /// the total of the five per-cause counters ([`ChannelStats::by_cause`] is
    /// that sum, [`ChannelStats::split_adds_up`] the assertion).
    ///
    /// Kept as the total, because a total is what "did this channel lose
    /// anything?" wants. It cannot answer *why*, and it was being asked to: a
    /// clean [`ChannelHandle::stop`] and an overloaded consumer moved the same
    /// number, so no reading of it could tell back-pressure from housekeeping
    /// (D011 §6.1).
    pub dropped: u64,
    /// Discarded because a proxy's bounded queue was full — the oldest went.
    ///
    /// The only cause that means **back-pressure**: a producer faster than a
    /// consumer, or a bound sized for a slower producer. The other four are
    /// housekeeping or this channel's own limitation.
    pub dropped_overflow: u64,
    /// Discarded because a consumer disconnected *itself*
    /// (`disconnect_push_supplier`, `disconnect_pull_supplier`) with a backlog
    /// still queued. Housekeeping: the consumer asked.
    pub dropped_on_disconnect: u64,
    /// Discarded because *this channel* cut a proxy that had failed
    /// [`MAX_CONSECUTIVE_FAILURES`] times in a row, abandoning its backlog.
    ///
    /// The consumer's fault or the network's, not the producer's rate — which
    /// is why it is not `dropped_overflow`, even though both mean "a consumer
    /// was not keeping up".
    pub dropped_on_failure_disconnect: u64,
    /// Discarded because [`ChannelHandle::stop`] ended the channel with events
    /// still queued. Housekeeping, and the one that must never be read as a
    /// symptom of anything.
    pub dropped_at_stop: u64,
    /// Events the channel **fetched** from a supplier with `try_pull`.
    ///
    /// The inbound counterpart of `delivered`: `accepted` counts everything
    /// the channel took in however it arrived, and this says how much of that
    /// the channel had to go and ask for. `accepted - sourced` is therefore
    /// what suppliers pushed in (plus [`ChannelHandle::publish`]), which is
    /// the split a channel serving both supplier models needs and which
    /// `accepted` alone cannot give.
    pub sourced: u64,
    /// Outbound `push` invocations that failed.
    pub push_failures: u64,
    /// Outbound `try_pull` invocations that failed.
    ///
    /// Separate from `push_failures` for the reason `pulled` is separate from
    /// `delivered`: one number is what this channel could not send and the
    /// other what it could not fetch, and one sum would hide which half is
    /// broken. Both are cut by the same [`MAX_CONSECUTIVE_FAILURES`] and both
    /// land in the same `disconnected_for_failure` when they are.
    pub pull_failures: u64,
    /// Proxies disconnected for reaching [`MAX_CONSECUTIVE_FAILURES`] —
    /// consumers this channel could not push to and suppliers it could not
    /// pull from alike.
    pub disconnected_for_failure: u64,
    /// Source rounds taken and then **not issued**, because the proxy was
    /// disconnected or the channel stopped before the round reached its commit
    /// point ([`ChannelState::source_still_wanted`]).
    ///
    /// Not a failure and not an `Empty` answer: nobody failed and nobody was
    /// asked. It is here rather than nowhere because a thrown-away action is
    /// the same class as a thrown-away event — the module's rule about never
    /// discarding anything silently, applied to the one thing this channel
    /// discards that is not an event. No drop cause joins the split for it,
    /// for the reason a `ProxyPullConsumer` adds none anywhere: there is no
    /// queue here and so nothing was lost.
    pub pull_rounds_cancelled: u64,
    /// Events refused because the destination's CDR alignment or byte order
    /// differs from where the `any` was captured. See the module docs.
    ///
    /// The fifth drop cause as well as its own counter: a refused event is
    /// always discarded — on the pull path by
    /// [`ChannelState::take_pull_event`], on the push path by the delivery
    /// thread, which had already taken it out of a queue. So it counts in
    /// `dropped` too, and there is no separate `dropped_unrelayable` because
    /// this number is already exactly that.
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
    /// Pull-consumer proxies with a supplier reference attached — the
    /// suppliers this channel goes and asks.
    pub pull_suppliers_connected: usize,
}

/// Why an event was discarded.
///
/// Five causes, and the point of naming them is that they answer different
/// questions: one is back-pressure, one is this channel's own limitation, and
/// three are housekeeping. [`ChannelStats::dropped`] summed all five, and
/// `PLAN-DEFERRED.md` §1's un-defer trigger — *"F7 reports a measured drop
/// rate caused by unwanted fan-out"* — asks about exactly one of them, so the
/// one number could not be that trigger's instrument in either direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropCause {
    /// A bounded queue was full and the oldest event went.
    Overflow,
    /// The `any` could not be relayed to this destination verbatim.
    Unrelayable,
    /// A consumer disconnected itself and left a backlog.
    Disconnect,
    /// This channel cut a repeatedly-failing proxy and left its backlog.
    FailureDisconnect,
    /// [`ChannelHandle::stop`] ended the channel with events still queued.
    Stop,
}

impl ChannelStats {
    /// The sum of the five per-cause counters. Equals `dropped`, always.
    pub fn by_cause(&self) -> u64 {
        self.dropped_overflow
            + self.unrelayable
            + self.dropped_on_disconnect
            + self.dropped_on_failure_disconnect
            + self.dropped_at_stop
    }

    /// Whether the split accounts for every drop.
    ///
    /// A caller asserting this is asserting that no discard path was added
    /// without naming its cause — the failure mode the split exists to end,
    /// arriving a second time. Every test here that drives a drop checks it.
    pub fn split_adds_up(&self) -> bool {
        self.by_cause() == self.dropped
    }

    /// The **only** way anything in this module discards an event.
    ///
    /// One call moves the total and exactly one cause, so the two cannot
    /// disagree — the reason this is a method and not five `+=` pairs spread
    /// over six call sites, which is the shape the counter had when it lost
    /// the ability to say what happened.
    fn discard(&mut self, cause: DropCause, n: u64) {
        self.dropped += n;
        match cause {
            DropCause::Overflow => self.dropped_overflow += n,
            DropCause::Unrelayable => self.unrelayable += n,
            DropCause::Disconnect => self.dropped_on_disconnect += n,
            DropCause::FailureDisconnect => self.dropped_on_failure_disconnect += n,
            DropCause::Stop => self.dropped_at_stop += n,
        }
    }
}

/// Everything both the servant thread and the delivery thread touch.
#[derive(Debug)]
struct ChannelState {
    proxy_suppliers: BTreeMap<Vec<u8>, ProxySupplier>,
    proxy_pull_suppliers: BTreeMap<Vec<u8>, ProxyPullSupplier>,
    proxy_consumers: BTreeMap<Vec<u8>, ProxyConsumer>,
    proxy_pull_consumers: BTreeMap<Vec<u8>, ProxyPullConsumer>,
    minted: u64,
    queue_limit: usize,
    /// How long a `pull` blocks before it raises `TIMEOUT`.
    pull_block: Duration,
    /// How long the source loop sleeps after a barren round.
    source_poll: Duration,
    /// Byte order the source loop writes its outbound `try_pull` requests in.
    ///
    /// A reply's byte order is the server's to choose and every ORB this
    /// workspace has measured answers in the request's, so in practice this is
    /// the order pulled events are **captured** in — and a captured event can
    /// only be relayed into a stream of its own order ([`relay_check`]). It is
    /// therefore a policy knob and not a formality: a channel pulling for
    /// consumers that ask big-endian should ask big-endian too.
    source_endian: Endian,
    stopped: bool,
    /// Round-robin cursor, so one busy proxy cannot starve the others.
    cursor: Vec<u8>,
    /// The same, for the source loop's round over connected suppliers.
    source_cursor: Vec<u8>,
    /// The proxy whose round the source thread has **taken** and not yet
    /// recorded, if any.
    ///
    /// Not an idleness counter — see [`ChannelState::idle`], which this
    /// deliberately stays out of. It answers the one question the disconnect
    /// property in the module docs leaves open: *could a round already past
    /// its commit point still be on the wire?* [`ChannelHandle::wait_source_idle`]
    /// is that question asked with a deadline.
    source_in_flight: Option<Vec<u8>>,
    /// A test seam on the source loop; see [`ChannelHandle::set_source_gate`].
    /// `None` in every production configuration.
    source_gate: Option<SourceGate>,
    /// Jobs taken out of a queue but not yet recorded. Part of "idle".
    in_flight: usize,
    stats: ChannelStats,
}

/// The callback behind a [`SourceGate`], taking the proxy key of the round
/// that has been taken.
type SourceGateFn = dyn Fn(&[u8]) + Send + Sync;

/// A callback the source thread runs after taking a round and before
/// committing to it. See [`ChannelHandle::set_source_gate`].
#[derive(Clone)]
struct SourceGate(Arc<SourceGateFn>);

impl std::fmt::Debug for SourceGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SourceGate(..)")
    }
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

/// One `try_pull` attempt against one supplier, copied out from under the lock.
struct SourceJob {
    proxy: Vec<u8>,
    supplier: Ior,
}

/// What one `try_pull` attempt did.
enum SourceOutcome {
    /// The supplier had an event and handed it over.
    Got(Event),
    /// The supplier answered, with nothing. Not a failure — it is the answer
    /// `try_pull` exists to give, and it resets the failure count.
    Empty,
    /// The supplier raised `CosEventComm::Disconnected`: it is finished, which
    /// is a statement rather than a fault. Released without a failure counted.
    SupplierDisconnected(String),
    /// The call was never made: at the commit point the channel no longer
    /// wanted it, because the proxy had been disconnected or the channel
    /// stopped while the round was being set up. Nobody was asked and nobody
    /// failed, so no failure count and no `Empty` reset — see
    /// [`ChannelStats::pull_rounds_cancelled`].
    Cancelled,
    /// The call did not complete.
    Failed(String),
}

impl ChannelState {
    fn new() -> Self {
        Self {
            proxy_suppliers: BTreeMap::new(),
            proxy_pull_suppliers: BTreeMap::new(),
            proxy_consumers: BTreeMap::new(),
            proxy_pull_consumers: BTreeMap::new(),
            minted: 0,
            queue_limit: DEFAULT_QUEUE_LIMIT,
            pull_block: DEFAULT_PULL_BLOCK,
            source_poll: DEFAULT_SOURCE_POLL,
            source_endian: Endian::native(),
            stopped: false,
            cursor: Vec::new(),
            source_cursor: Vec::new(),
            source_in_flight: None,
            source_gate: None,
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
        self.stats.pull_suppliers_connected =
            self.proxy_pull_consumers.values().filter(|p| p.supplier.is_some()).count();
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
            self.stats.fanned_out += 1;
            while proxy.queue.len() > limit {
                proxy.queue.pop_front();
                self.stats.discard(DropCause::Overflow, 1);
                // Loud, per event. Control-plane granularity means a healthy
                // channel prints none of these at all, so a stream of them is
                // the signal, not noise.
                eprintln!(
                    "orbweaver: event channel dropped the oldest event for proxy {} \
                     (queue limit {limit}, {} overflow drop(s) in total)",
                    String::from_utf8_lossy(key),
                    self.stats.dropped_overflow
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
            self.stats.fanned_out += 1;
            while proxy.queue.len() > limit {
                proxy.queue.pop_front();
                self.stats.discard(DropCause::Overflow, 1);
                eprintln!(
                    "orbweaver: event channel dropped the oldest event for pull proxy {} \
                     (queue limit {limit}, {} overflow drop(s) in total)",
                    String::from_utf8_lossy(key),
                    self.stats.dropped_overflow
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
                    self.stats.discard(DropCause::Unrelayable, 1);
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

    /// The next supplier to ask, round-robin from [`ChannelState::source_cursor`],
    /// together with how many suppliers are connected — which is how long a
    /// *round* is, and therefore how many fruitless visits mean the round was
    /// barren rather than that this one supplier is quiet.
    ///
    /// Nothing is taken *out* of anything here — a `ProxyPullConsumer` holds
    /// no queue, and a visit that fails loses nothing because nothing had been
    /// removed — so there is no `in_flight` counterpart in the delivery
    /// thread's sense. [`ChannelState::source_in_flight`] is still recorded,
    /// for the different question stated on it: not "is there work
    /// outstanding" but "could a round already past its commit point still be
    /// on the wire".
    fn take_next_source(&mut self) -> Option<(SourceJob, usize)> {
        let keys: Vec<Vec<u8>> = self
            .proxy_pull_consumers
            .iter()
            .filter(|(_, p)| p.connected && p.supplier.is_some())
            .map(|(k, _)| k.clone())
            .collect();
        if keys.is_empty() {
            return None;
        }
        let start =
            keys.iter().position(|k| k.as_slice() > self.source_cursor.as_slice()).unwrap_or(0);
        let key = keys[start].clone();
        let supplier = self.proxy_pull_consumers.get(&key)?.supplier.clone()?;
        self.source_cursor = key.clone();
        self.source_in_flight = Some(key.clone());
        Some((SourceJob { proxy: key, supplier }, keys.len()))
    }

    /// **The commit point**: whether the round already taken for `proxy_key`
    /// may still go out.
    ///
    /// The module docs state the property this predicate is; the short form is
    /// that it is taken under the same lock `disconnect_pull_consumer` and
    /// [`ChannelHandle::stop`] take, and that nothing between it and the
    /// outbound request does any I/O. One predicate for both of them, because
    /// "the channel no longer wants this round" is one fact — a second check
    /// spelled out at each caller is how the two would drift apart.
    ///
    /// The supplier reference is compared and not only the flag: a client that
    /// disconnected and reconnected to a *different* supplier while this round
    /// was being set up would otherwise be asked a question meant for the old
    /// one.
    fn source_still_wanted(&self, proxy_key: &[u8], supplier: &Ior) -> bool {
        !self.stopped
            && self
                .proxy_pull_consumers
                .get(proxy_key)
                .is_some_and(|p| p.connected && p.supplier.as_ref() == Some(supplier))
    }

    /// Every supplier the source loop is currently entitled to dial, so the
    /// loop can close the sockets of the ones it is not.
    fn connected_source_keys(&self) -> Vec<Vec<u8>> {
        self.proxy_pull_consumers
            .iter()
            .filter(|(_, p)| p.connected && p.supplier.is_some())
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Records one `try_pull` attempt. Returns whether the proxy is still
    /// connected afterwards, which is the source loop's cue to drop its socket.
    fn record_source(&mut self, proxy_key: &[u8], outcome: SourceOutcome) -> bool {
        // The taken round is over, whatever it did — before the early return
        // below, or a proxy forgotten mid-call would leave this set forever.
        self.source_in_flight = None;
        let name = String::from_utf8_lossy(proxy_key).into_owned();
        if !self.proxy_pull_consumers.contains_key(proxy_key) {
            return false; // disconnected and forgotten while the call was out
        }
        match outcome {
            SourceOutcome::Got(event) => {
                if let Some(proxy) = self.proxy_pull_consumers.get_mut(proxy_key) {
                    proxy.consecutive_failures = 0;
                }
                self.stats.sourced += 1;
                // From here on a fetched event is indistinguishable from a
                // pushed one — same fan-out, same bound, same drop causes.
                // That sameness is the whole of why this direction needed one
                // loop and not a second accounting.
                self.fan_out(event);
            }
            SourceOutcome::Empty => {
                if let Some(proxy) = self.proxy_pull_consumers.get_mut(proxy_key) {
                    proxy.consecutive_failures = 0;
                }
            }
            SourceOutcome::Cancelled => {
                // Nothing went out, so nothing here moves but the count of
                // rounds thrown away: not a failure (nobody failed), not
                // `Empty` (nobody answered, so there is no failure streak to
                // reset), and no event to drop.
                self.stats.pull_rounds_cancelled += 1;
            }
            SourceOutcome::SupplierDisconnected(why) => {
                if let Some(proxy) = self.proxy_pull_consumers.get_mut(proxy_key) {
                    proxy.connected = false;
                    proxy.supplier = None;
                    proxy.consecutive_failures = 0;
                }
                eprintln!(
                    "orbweaver: event channel released the supplier on {name}: it answered \
                     {why}; the proxy stays reconnectable"
                );
            }
            SourceOutcome::Failed(why) => {
                self.stats.pull_failures += 1;
                let Some(proxy) = self.proxy_pull_consumers.get_mut(proxy_key) else {
                    return false;
                };
                proxy.consecutive_failures += 1;
                let n = proxy.consecutive_failures;
                eprintln!(
                    "orbweaver: event channel try_pull from {name} failed \
                     ({n}/{MAX_CONSECUTIVE_FAILURES}): {why}"
                );
                if n >= MAX_CONSECUTIVE_FAILURES {
                    proxy.connected = false;
                    proxy.supplier = None;
                    proxy.consecutive_failures = 0;
                    self.stats.disconnected_for_failure += 1;
                    // No `discard` call, and that is not an omission: this
                    // proxy never held an event, so there is no backlog to
                    // abandon and no cause to add to the split.
                    eprintln!(
                        "orbweaver: event channel stopped pulling from {name} after \
                         {MAX_CONSECUTIVE_FAILURES} consecutive failures; no events were \
                         queued here, so nothing was dropped"
                    );
                }
            }
        }
        self.proxy_pull_consumers.get(proxy_key).is_some_and(|p| p.connected)
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
                //
                // It *is* a discard, though, and until the split it was the
                // one that never said so: the delivery thread had already
                // taken the event out of the queue, so refusing it here lost
                // it while `dropped` stood still. The pull path counted the
                // same refusal in both numbers. Now both paths do.
                self.stats.discard(DropCause::Unrelayable, 1);
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
                    self.stats.discard(DropCause::FailureDisconnect, abandoned as u64);
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

    /// Overrides how long the source loop sleeps after a barren round.
    ///
    /// See [`DEFAULT_SOURCE_POLL`]: this is the interval a polling channel has
    /// to invent, and the only cost of having chosen `try_pull` over a
    /// blocking `pull`. It bounds idle chatter, not throughput — a round that
    /// finds an event never reaches the sleep.
    pub fn set_source_poll(&self, poll: Duration) {
        self.shared.lock().source_poll = poll.max(Duration::from_millis(1));
    }

    /// Overrides the byte order the source loop asks in.
    ///
    /// A pulled event is captured in the byte order its supplier replied in,
    /// and every ORB measured here replies in the request's, so this is in
    /// practice the order the events entering by this route can be relayed
    /// in — [`relay_check`] refuses the other one and counts it in
    /// [`ChannelStats::unrelayable`]. Defaults to native, which is what every
    /// outbound [`Connection`] in this workspace defaults to.
    pub fn set_source_endian(&self, endian: Endian) {
        self.shared.lock().source_endian = endian;
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

    /// Blocks until the source thread has finished the round it had taken, or
    /// `timeout` elapses. Returns whether it finished.
    ///
    /// This is the escape clause of the disconnect property in the module docs
    /// made observable. After `disconnect_pull_consumer` — or [`stop`] — a
    /// round already past its commit point may still be on the wire, at most
    /// one; a caller that needs "and now nobody is being asked at all" waits
    /// here for it. It is deliberately **not** part of [`wait_idle`], which is
    /// the delivery thread's question and answers about queues.
    ///
    /// A caller over the wire cannot reach this and has the time bound
    /// instead: one outbound timeout.
    ///
    /// [`stop`]: ChannelHandle::stop
    /// [`wait_idle`]: ChannelHandle::wait_idle
    pub fn wait_source_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = self.shared.lock();
        loop {
            if state.source_in_flight.is_none() {
                return true;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            state = self.shared.wait(&self.shared.progress, state, left);
        }
    }

    /// Installs a callback the source thread runs after **taking** a round and
    /// before committing to it, with the proxy key and with no lock held.
    ///
    /// A test seam, and it exists for the reason [`EventSource::pull_calls`]
    /// does: a property of this loop that is otherwise observable only by
    /// luck. The property is the disconnect bound above, and the window it
    /// concerns is a few instructions wide — five concurrent whole-suite runs
    /// on CI Linux hit it, twenty serial runs on macOS never did. A callback
    /// that blocks here holds the round open and makes the ordering happen
    /// every time, which is what lets the bound be tested rather than hoped
    /// for.
    ///
    /// Blocking in it holds the source thread and nothing else — no lock is
    /// held and the servant threads are untouched — but it does hold
    /// [`Delivery`]'s join, so a callback with no deadline of its own turns a
    /// failing test into a hanging one.
    pub fn set_source_gate<F>(&self, gate: F)
    where
        F: Fn(&[u8]) + Send + Sync + 'static,
    {
        self.shared.lock().source_gate = Some(SourceGate(Arc::new(gate)));
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

    /// Stops the delivery thread, the source thread and the channel with them.
    /// Queued events are not delivered; they are counted as dropped, because
    /// pretending a stopped channel delivered them would be exactly the silent
    /// truncation this module refuses. Pull queues are emptied and counted the
    /// same way, and a `pull` blocked on a stopped channel is woken and
    /// answered `Disconnected` rather than left to time out.
    ///
    /// **A connected supplier is not a queue and so is not a drop.** The
    /// source thread's sockets close with it and its `ProxyPullConsumer`s keep
    /// their `connected` flag and their supplier references, exactly as the
    /// push proxies keep their consumers; the tally below counts proxy queues,
    /// which those proxies do not have, so [`ChannelStats::split_adds_up`]
    /// cannot be disturbed by this direction at all.
    ///
    /// **The asking stops on the same terms a disconnect's does.** Raising
    /// `stopped` under this lock fails the source loop's commit point, so
    /// every round not already past it is cancelled rather than issued; at
    /// most one further `try_pull` can reach a supplier, and
    /// [`ChannelHandle::wait_source_idle`] waits it out. Module docs, point 4.
    pub fn stop(&self) {
        let mut state = self.shared.lock();
        state.stopped = true;
        let abandoned: usize = state.proxy_suppliers.values().map(|p| p.queue.len()).sum::<usize>()
            + state.proxy_pull_suppliers.values().map(|p| p.queue.len()).sum::<usize>();
        if abandoned > 0 {
            state.stats.discard(DropCause::Stop, abandoned as u64);
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

/// The channel's two outbound threads, joined on drop.
///
/// Held by whoever started it; dropping it stops the channel, so a spike that
/// forgets to stop cannot leave a thread pushing into — or pulling from — a
/// torn-down fixture.
///
/// **Two threads, not one**, and the reason is the two timeouts they spend.
/// The delivery thread can be held for a push timeout by a slow *consumer* and
/// the source thread for the same by a slow *supplier*; sharing one thread
/// would make each direction's worst case the other's as well, so a dead
/// consumer would delay every fetch and a silent supplier would delay every
/// delivery. Each keeps its own connection map for the reason the delivery
/// thread always did: a socket the servant thread could reach is a socket the
/// servant thread could block on.
#[derive(Debug)]
pub struct Delivery {
    inner: Arc<Inner>,
}

impl Delivery {
    /// A handle to the channel created with the server, for callers that only
    /// kept this.
    pub fn handle(&self) -> ChannelHandle {
        self.handle_named(&self.inner.default_name).expect("the default channel is never removed")
    }

    /// A handle to the channel named `name`, or `None` if there is none.
    pub fn handle_named(&self, name: &str) -> Option<ChannelHandle> {
        let reg = self.inner.registry.lock().unwrap_or_else(|e| e.into_inner());
        reg.channels.get(name).map(|c| ChannelHandle { shared: Arc::clone(&c.shared) })
    }
}

impl Drop for Delivery {
    /// Stops **every** channel and joins every thread this server started.
    ///
    /// Every channel, because a server is what was started and a server is
    /// what is being stopped; leaving one channel's threads running after the
    /// `Delivery` that owns them is gone is exactly the "spike that forgets to
    /// stop leaves a thread pushing into a torn-down fixture" this type exists
    /// to prevent, and it would be harder to see with several channels rather
    /// than easier.
    fn drop(&mut self) {
        let (channels, threads) = {
            let mut reg = self.inner.registry.lock().unwrap_or_else(|e| e.into_inner());
            reg.running = None;
            reg.started.clear();
            let channels: Vec<Arc<ChannelObjects>> =
                reg.channels.values().map(Arc::clone).collect();
            (channels, std::mem::take(&mut reg.threads))
        };
        // Outside the registry lock: `stop` takes each channel's own lock, and
        // a thread being joined may still be taking it too.
        for objects in channels {
            ChannelHandle { shared: Arc::clone(&objects.shared) }.stop();
        }
        for t in threads {
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

/// One outbound `try_pull`, with **no lock held** — the same rule the delivery
/// path keeps, for the same reason: the supplier we are asking is free to be a
/// consumer of this same channel.
///
/// `try_pull` and never `pull`. See the module docs; the short form is that
/// `pull` is specified to block and this is a shared round, so one silent
/// supplier would be every other supplier's outage.
fn source_pull(
    shared: &Shared,
    conns: &mut HashMap<Vec<u8>, (Ior, Connection)>,
    job: &SourceJob,
    timeout: Duration,
    endian: Endian,
) -> SourceOutcome {
    let fresh = match conns.get(&job.proxy) {
        // A reconnect to a different supplier must not reuse the old socket.
        Some((ior, conn)) => *ior != job.supplier || !conn.is_usable(),
        None => true,
    };
    if fresh {
        conns.remove(&job.proxy);
        match Connection::connect(&job.supplier, timeout) {
            Ok(conn) => {
                conns.insert(job.proxy.clone(), (job.supplier.clone(), conn));
            }
            Err(e) => return SourceOutcome::Failed(format!("connect: {e}")),
        }
    }
    let (_, conn) = conns.get_mut(&job.proxy).expect("inserted just above");
    conn.set_endian(endian);

    // ── The commit point, and the last instant it can be taken: the connect
    // above may have cost a whole timeout, and a disconnect that landed during
    // it must not be answered with a question. The lock is taken and released
    // here — held across the invoke below it would be rule 1, and `guarded`
    // would say so — so what remains open is the gap between this line and the
    // request, which contains no I/O. That gap is the module docs' "at most
    // one further call", and it is the whole of it. ──
    let wanted = shared.lock().source_still_wanted(&job.proxy, &job.supplier);
    if !wanted {
        return SourceOutcome::Cancelled;
    }

    let reply = match conn.invoke_nullary("try_pull") {
        Ok(reply) => reply,
        // A supplier that says `Disconnected` has not failed; it has finished,
        // and the standard gives it that word for exactly this. Counting it as
        // a failure would spend two of three retries on a peer that already
        // answered.
        Err(crate::Error::UserException { ref id, .. }) if id == DISCONNECTED_ID => {
            return SourceOutcome::SupplierDisconnected(id.clone());
        }
        Err(e) => {
            conns.remove(&job.proxy); // a failed invoke may have poisoned it
            return SourceOutcome::Failed(e.to_string());
        }
    };
    let Ok(mut body) = reply.body() else {
        conns.remove(&job.proxy);
        return SourceOutcome::Failed("the try_pull reply body could not be reached".into());
    };
    // §9.4.2: the return value precedes the `out` parameter, so the `any` runs
    // to one octet before the end of the body and that octet is `has_event`.
    // Exactly the shape `client::try_pull` reads, and the reason the value's
    // length is knowable at all — CDR gives an `any` no length prefix.
    let event = match capture_event(&mut body, 1) {
        Ok(event) => event,
        Err(_) => {
            conns.remove(&job.proxy);
            return SourceOutcome::Failed("the try_pull reply did not decode".into());
        }
    };
    match body.get_bool() {
        Ok(true) => SourceOutcome::Got(event),
        Ok(false) => SourceOutcome::Empty,
        Err(_) => {
            conns.remove(&job.proxy);
            SourceOutcome::Failed("the try_pull reply had no has_event flag".into())
        }
    }
}

/// The source thread: the channel as a *client* of its suppliers.
///
/// One round-robin round over every connected supplier. A visit that produces
/// an event continues straight to the next supplier, so a backlog drains at
/// socket speed; only when a whole round has produced nothing does the loop
/// sleep out [`ChannelState::source_poll`] — on the condvar, so a `stop` or a
/// fresh connect ends the sleep early rather than waiting it out.
fn source_loop(shared: Arc<Shared>, timeout: Duration) {
    // The source thread's own connections, never behind the mutex — the rule
    // the delivery thread keeps, and for the same reason.
    let mut conns: HashMap<Vec<u8>, (Ior, Connection)> = HashMap::new();
    // Consecutive visits that produced no event. When it reaches the length of
    // a round, every connected supplier has been asked and none had anything.
    let mut barren = 0usize;
    loop {
        let (job, round, endian, gate) = {
            let mut state = shared.lock();
            loop {
                if state.stopped {
                    return;
                }
                if let Some((job, round)) = state.take_next_source() {
                    let gate = state.source_gate.clone();
                    break (job, round, state.source_endian, gate);
                }
                // Nothing is connected. Sleep on the condvar so a connect
                // arriving in a moment is served in a moment.
                let poll = state.source_poll;
                conns.clear();
                state = shared.wait(&shared.wake, state, poll);
            }
        };
        // ── no lock is held from here on. See the module docs. ──
        // A round has been taken and nothing has gone out yet, which is the
        // only instant from which the commit point inside `source_pull` can be
        // observed to do anything. Production installs no gate.
        if let Some(gate) = gate {
            (gate.0)(&job.proxy);
        }
        let outcome = source_pull(&shared, &mut conns, &job, timeout, endian);
        let got = matches!(outcome, SourceOutcome::Got(_));
        {
            let mut state = shared.lock();
            state.record_source(&job.proxy, outcome);
            // Close the socket of every supplier this channel is no longer
            // entitled to dial — the one just released by a failure or by a
            // `Disconnected`, and any a client disconnected while the round
            // was elsewhere.
            let live = state.connected_source_keys();
            conns.retain(|key, _| live.iter().any(|k| k == key));
        }
        // Every recorded round, not only a fruitful one: `progress` is what
        // `wait_source_idle` sleeps on, and the round a disconnect has to
        // outlast is exactly a round that fetched nothing.
        shared.progress.notify_all();
        if got {
            barren = 0;
            // A fetched event is a queued event: wake the delivery thread and
            // any consumer blocked in `pull`, exactly as an inbound `push`
            // does.
            shared.wake.notify_all();
        } else {
            barren += 1;
            if barren >= round {
                barren = 0;
                let state = shared.lock();
                if state.stopped {
                    return;
                }
                let poll = state.source_poll;
                drop(shared.wait(&shared.wake, state, poll));
            }
        }
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
    ProxyPullConsumer,
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
            Target::ProxyPullConsumer => PROXY_PULL_CONSUMER_ID,
        }
    }
}

/// One channel's objects: its own key space and its own [`Shared`] state.
///
/// A channel is exactly this much — a name, three fixed keys and the state
/// behind them. Everything that makes a channel work (the bounded queues, the
/// drop split, the two outbound threads) already lived behind one `Arc<Shared>`
/// per channel, which is why a server holding several needs no new machinery:
/// it needs a **map** and a rule about keys.
#[derive(Debug)]
struct ChannelObjects {
    name: String,
    base: Vec<u8>,
    consumer_admin: Vec<u8>,
    supplier_admin: Vec<u8>,
    shared: Arc<Shared>,
}

impl ChannelObjects {
    fn new(name: String, base: Vec<u8>) -> Self {
        let mut consumer_admin = base.clone();
        consumer_admin.extend_from_slice(CONSUMER_ADMIN_SUFFIX);
        let mut supplier_admin = base.clone();
        supplier_admin.extend_from_slice(SUPPLIER_ADMIN_SUFFIX);
        ChannelObjects {
            name,
            base,
            consumer_admin,
            supplier_admin,
            shared: Arc::new(Shared {
                state: Mutex::new(ChannelState::new()),
                wake: Condvar::new(),
                progress: Condvar::new(),
            }),
        }
    }
}

/// Why a channel could not be created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelError {
    /// The name cannot be an object-key segment. See
    /// [`is_channel_name_safe`] for the rule and the reason.
    UnsafeName {
        /// The name as given.
        name: String,
        /// Which clause of the rule it broke.
        why: &'static str,
    },
    /// A channel of this name already exists on this server.
    Duplicate {
        /// The name as given.
        name: String,
    },
}

impl std::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelError::UnsafeName { name, why } => {
                write!(f, "the channel name {name:?} cannot be an object-key segment: {why}")
            }
            ChannelError::Duplicate { name } => {
                write!(f, "this server already serves a channel named {name:?}")
            }
        }
    }
}

impl std::error::Error for ChannelError {}

/// The object-key segments this module mints for itself, which a channel name
/// may therefore not be.
const RESERVED_SEGMENTS: [&str; 2] = ["consumerAdmin", "supplierAdmin"];
/// The proxy tags [`EventChannelServer::mint_on`] prefixes a number with.
const PROXY_TAGS: [&str; 4] = ["pps", "pls", "ppc", "plc"];

/// Whether `name` may be a channel name on an [`EventChannelServer`].
///
/// # Why a rule is needed at all
///
/// Every object this servant answers for is addressed by a key, and with one
/// channel per server the keys could be built with no thought: `base`, then
/// `base + "/consumerAdmin"`, then `base + "/pps1"` and so on. With several
/// channels in one server the name enters the key, and **two names that mint
/// the same key are two channels that are one channel** — a supplier pushing
/// into one would be fanned out to the other's consumers, silently, with every
/// counter agreeing.
///
/// # The rule, and why it is enough
///
/// A name must be non-empty, contain no `/`, and not be a segment this module
/// mints for itself — `consumerAdmin`, `supplierAdmin`, or one of the four
/// proxy tags followed by digits.
///
/// That is sufficient, and here is the whole argument. Every key of the
/// channel named *N* is either `prefix(N)` or `prefix(N) + "/" + s` where `s`
/// contains no `/`; `prefix` is the server's `base_key` for the channel
/// created with the server and `base_key + "/" + N` for every other. Two
/// distinct names give two distinct prefixes, since the name is the whole of
/// what follows `base_key + "/"` and contains no `/` to blur the boundary. A
/// created channel's prefix can equal another channel's *minted* key only if
/// the name equals a minted segment, which the reserved clause forbids; and it
/// can equal the server-created channel's prefix only if the name is empty,
/// which the first clause forbids. So the key spaces are disjoint.
///
/// The reserved clause is the one that is easy to leave out and impossible to
/// notice missing: without it a channel named `consumerAdmin` would answer to
/// the *first* channel's `ConsumerAdmin` key, and which of the two a request
/// reached would depend on map iteration order.
pub fn is_channel_name_safe(name: &str) -> bool {
    why_unsafe(name).is_none()
}

/// The clause `name` breaks, or `None` if it breaks none.
fn why_unsafe(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("a name must not be empty");
    }
    if name.contains('/') {
        return Some("a name must not contain '/', which separates key segments");
    }
    if RESERVED_SEGMENTS.contains(&name) {
        return Some("that is an admin key this module mints for every channel");
    }
    for tag in PROXY_TAGS {
        if let Some(rest) = name.strip_prefix(tag)
            && !rest.is_empty()
            && rest.bytes().all(|b| b.is_ascii_digit())
        {
            return Some("that is a proxy key this module mints");
        }
    }
    None
}

/// Everything an [`EventChannelServer`] holds, behind one `Arc` so a
/// [`Delivery`] can outlive the borrow that started it.
#[derive(Debug)]
struct Inner {
    host: String,
    port: u16,
    /// The channel created with the server: the one `channel_ior`, `handle`
    /// and `channel_key` answer for, and the one whose keys are `base_key`
    /// verbatim so a server built the old way is byte-identical.
    default_name: String,
    /// That channel's key, kept out of the map as well as in it.
    ///
    /// Not a duplicate to be tidied away: it is what lets `channel_key` keep
    /// returning a `&[u8]` — the signature every existing caller was written
    /// against — where a lookup behind the registry mutex could only return an
    /// owned copy. It is written once, at construction, and the default
    /// channel is never removed, so the two cannot disagree.
    default_base: Vec<u8>,
    registry: Mutex<Registry>,
}

/// The channels, and the threads serving them.
#[derive(Debug)]
struct Registry {
    channels: BTreeMap<String, Arc<ChannelObjects>>,
    /// `Some(timeout)` once delivery has been started, so a channel created
    /// afterwards starts its own threads rather than sitting inert — the
    /// failure a reader would never see, because a channel with no threads
    /// accepts and queues exactly like one whose consumers are all slow.
    running: Option<Duration>,
    /// Channels whose two outbound threads are running.
    started: std::collections::BTreeSet<String>,
    /// Every thread started for this server, joined by [`Delivery`]'s `Drop`.
    threads: Vec<std::thread::JoinHandle<()>>,
}

/// Starts one channel's two outbound threads and records them for joining.
///
/// Named rather than inlined because it is called from two places that must
/// not diverge — `start_delivery_with` and `create_channel` — and a channel
/// that got only one of the two threads would behave exactly like a channel
/// whose peers were all slow.
fn spawn_outbound(reg: &mut Registry, objects: &Arc<ChannelObjects>, timeout: Duration) {
    let shared = Arc::clone(&objects.shared);
    let delivery = std::thread::Builder::new()
        .name(format!("orbweaver-event-delivery/{}", objects.name))
        .spawn(move || delivery_loop(shared, timeout))
        .expect("spawning the event delivery thread");
    let shared = Arc::clone(&objects.shared);
    let source = std::thread::Builder::new()
        .name(format!("orbweaver-event-source/{}", objects.name))
        .spawn(move || source_loop(shared, timeout))
        .expect("spawning the event source thread");
    reg.threads.push(delivery);
    reg.threads.push(source);
    reg.started.insert(objects.name.clone());
}

/// An in-memory CosEvent channel server behind [`crate::server::Server`].
///
/// One instance serves **one or more channels**, each with its own
/// `EventChannel`, both admins and every proxy either admin mints, each as its
/// own object key on this one dispatch — the F6 shape, once per channel.
/// `host` and `port` are what go into minted references and are the caller's
/// to publish correctly (Phase 0 assumption D: the bind address and the
/// publishable address differ behind NAT).
///
/// # Several channels, and no new wire surface
///
/// `CosEventChannelAdmin` declares **no factory** — the factory in the
/// standard is `CosNotifyChannelAdmin::EventChannelFactory`, which belongs to
/// CosNotification and is deferred (`PLAN-DEFERRED` §1, D021 §3). So creation
/// is a Rust API and a deployment decision, exactly as `Poa` creation is:
/// [`EventChannelServer::create_channel`]. Inventing an Orbweaver-specific
/// factory interface would be a fifth wire surface nobody asked for.
///
/// **A server built the old way is a server with one channel**, whose keys are
/// the `base_key` it was given, byte for byte — absent is not zero, the rule
/// the MCP `--config` batch proved and D020 Stage A applies. Every reference
/// it publishes and every key it answers to is unchanged, which is what makes
/// this compatible rather than merely similar.
///
/// # Where a fact about a channel lives
///
/// Each channel keeps its own [`ChannelStats`], because each has its own
/// queues and its own peers; there is no shared counter and no cross-channel
/// arithmetic anywhere in the servant. [`EventChannelServer::total_stats`]
/// exists for the one question a *process* is asked — "did anything here lose
/// an event?" — and it is a sum, so it can say that and cannot say which
/// channel, the same shape as this module's existing "channel-wide, not per
/// consumer" limit one level up.
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
    inner: Arc<Inner>,
}

impl EventChannelServer {
    /// A server with **one** channel, rooted at `base_key` and minting
    /// references that point at `host:port`. No outbound thread runs until
    /// [`EventChannelServer::start_delivery`] is called — a channel with no
    /// delivery thread accepts and queues, which is what the queue-accounting
    /// tests need.
    ///
    /// The channel's name is `base_key` read as text, and its keys are
    /// `base_key` verbatim: a caller who never asks for a second channel
    /// cannot tell this version from the one before it, on the wire or in the
    /// API.
    pub fn new(host: impl Into<String>, port: u16, base_key: Vec<u8>) -> Self {
        let name = String::from_utf8_lossy(&base_key).into_owned();
        let objects = Arc::new(ChannelObjects::new(name.clone(), base_key.clone()));
        let mut channels = BTreeMap::new();
        channels.insert(name.clone(), objects);
        EventChannelServer {
            inner: Arc::new(Inner {
                host: host.into(),
                port,
                default_name: name,
                default_base: base_key,
                registry: Mutex::new(Registry {
                    channels,
                    running: None,
                    started: std::collections::BTreeSet::new(),
                    threads: Vec::new(),
                }),
            }),
        }
    }

    /// Adds a channel named `name`, with its own admins, its own proxies and
    /// its own [`ChannelStats`].
    ///
    /// The name is checked by [`is_channel_name_safe`], whose documentation
    /// carries the argument for why the rule is what it is. A rejected name is
    /// an error and never a coerced-into-safety name: silently renaming a
    /// caller's channel would publish references under a name the caller does
    /// not know it has.
    ///
    /// If [`EventChannelServer::start_delivery`] has already been called, the
    /// new channel's two outbound threads start here, so a channel created at
    /// any moment behaves like one created before the server started serving.
    pub fn create_channel(&self, name: &str) -> std::result::Result<ChannelHandle, ChannelError> {
        if let Some(why) = why_unsafe(name) {
            return Err(ChannelError::UnsafeName { name: name.to_owned(), why });
        }
        let mut base = self.inner.default_base.clone();
        base.push(b'/');
        base.extend_from_slice(name.as_bytes());
        let objects = Arc::new(ChannelObjects::new(name.to_owned(), base));
        let handle = ChannelHandle { shared: Arc::clone(&objects.shared) };

        let mut reg = self.inner.registry.lock().unwrap_or_else(|e| e.into_inner());
        if reg.channels.contains_key(name) {
            return Err(ChannelError::Duplicate { name: name.to_owned() });
        }
        reg.channels.insert(name.to_owned(), Arc::clone(&objects));
        if let Some(timeout) = reg.running {
            spawn_outbound(&mut reg, &objects, timeout);
        }
        Ok(handle)
    }

    /// Every channel this server holds, in name order.
    pub fn channel_names(&self) -> Vec<String> {
        let reg = self.inner.registry.lock().unwrap_or_else(|e| e.into_inner());
        reg.channels.keys().cloned().collect()
    }

    /// Starts both outbound threads for every channel, with
    /// [`DEFAULT_PUSH_TIMEOUT`].
    pub fn start_delivery(&self) -> Delivery {
        self.start_delivery_with(DEFAULT_PUSH_TIMEOUT)
    }

    /// Starts both outbound threads for every channel, bounding each outbound
    /// call by `timeout`.
    ///
    /// The source thread runs whether or not a supplier is ever connected: it
    /// costs one condvar wake per [`ChannelState::source_poll`] while nothing
    /// is attached, which is the same idle cost the delivery thread has always
    /// had, and making it conditional would mean a `connect_pull_supplier`
    /// arriving at a channel with no one to answer it.
    ///
    /// Two threads **per channel**, not two per server. Channels are the unit
    /// a slow peer can wedge — the queues, the timeouts and the failure counts
    /// are all per channel — so sharing one delivery thread between them would
    /// make one channel's dead consumer every other channel's latency, which
    /// is the failure this module is built around avoiding, one level up.
    pub fn start_delivery_with(&self, timeout: Duration) -> Delivery {
        let mut reg = self.inner.registry.lock().unwrap_or_else(|e| e.into_inner());
        reg.running = Some(timeout);
        let all: Vec<Arc<ChannelObjects>> = reg.channels.values().map(Arc::clone).collect();
        for objects in all {
            if !reg.started.contains(&objects.name) {
                spawn_outbound(&mut reg, &objects, timeout);
            }
        }
        drop(reg);
        Delivery { inner: Arc::clone(&self.inner) }
    }

    /// A handle to the channel created with the server, usable after the
    /// servant has been moved into a serving thread.
    pub fn handle(&self) -> ChannelHandle {
        ChannelHandle { shared: Arc::clone(&self.default_objects().shared) }
    }

    /// A handle to the channel named `name`, or `None` if there is none.
    pub fn handle_named(&self, name: &str) -> Option<ChannelHandle> {
        let reg = self.inner.registry.lock().unwrap_or_else(|e| e.into_inner());
        reg.channels.get(name).map(|c| ChannelHandle { shared: Arc::clone(&c.shared) })
    }

    /// The sum of every channel's counters.
    ///
    /// The one question a *process* is asked — "did anything here lose an
    /// event?" — and the honest limit is the same one the per-channel numbers
    /// already have one level down: it cannot say **which** channel, and
    /// nothing here divides by the channel count to guess. Every counter is
    /// additive and [`ChannelStats::split_adds_up`] is a linear identity, so
    /// it holds of the sum exactly when it holds of every part; a sum that
    /// failed it would mean a channel that had.
    ///
    /// The gauges (`queued`, the three `*_connected`) are sums too, and mean
    /// what a sum of gauges means: how many there are in this process now.
    pub fn total_stats(&self) -> ChannelStats {
        let all: Vec<Arc<ChannelObjects>> = {
            let reg = self.inner.registry.lock().unwrap_or_else(|e| e.into_inner());
            reg.channels.values().map(Arc::clone).collect()
        };
        let mut total = ChannelStats::default();
        for objects in all {
            let s = objects.shared.lock().stats();
            total.accepted += s.accepted;
            total.fanned_out += s.fanned_out;
            total.delivered += s.delivered;
            total.dropped += s.dropped;
            total.dropped_overflow += s.dropped_overflow;
            total.dropped_on_disconnect += s.dropped_on_disconnect;
            total.dropped_on_failure_disconnect += s.dropped_on_failure_disconnect;
            total.dropped_at_stop += s.dropped_at_stop;
            total.sourced += s.sourced;
            total.push_failures += s.push_failures;
            total.pull_failures += s.pull_failures;
            total.disconnected_for_failure += s.disconnected_for_failure;
            total.pull_rounds_cancelled += s.pull_rounds_cancelled;
            total.unrelayable += s.unrelayable;
            total.pulled += s.pulled;
            total.queued += s.queued;
            total.consumers_connected += s.consumers_connected;
            total.pull_consumers_connected += s.pull_consumers_connected;
            total.pull_suppliers_connected += s.pull_suppliers_connected;
        }
        total
    }

    /// The object key of the channel created with the server — what
    /// [`crate::server::Server`] must be bound with for the two to describe
    /// the same object.
    ///
    /// Only that channel's, because only that one's key is the caller's to
    /// know in advance; every other channel's is derived and is reached
    /// through [`EventChannelServer::channel_ior_named`]. The server's
    /// `knows` answers for all of them either way.
    pub fn channel_key(&self) -> &[u8] {
        &self.inner.default_base
    }

    /// A publishable reference to the channel created with the server.
    pub fn channel_ior(&self) -> Ior {
        self.ior_for(&self.inner.default_base, EVENT_CHANNEL_ID)
    }

    /// A publishable reference to the channel named `name`, or `None` if
    /// there is none. This is what E3 will bind into a naming context.
    pub fn channel_ior_named(&self, name: &str) -> Option<Ior> {
        let base = {
            let reg = self.inner.registry.lock().unwrap_or_else(|e| e.into_inner());
            reg.channels.get(name)?.base.clone()
        };
        Some(self.ior_for(&base, EVENT_CHANNEL_ID))
    }

    fn ior_for(&self, key: &[u8], type_id: &str) -> Ior {
        Ior {
            type_id: type_id.to_owned(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: self.inner.host.clone(),
                port: self.inner.port,
                object_key: key.to_vec(),
                // §7.10.2.4: a profile with no `TAG_CODE_SETS` declares no
                // `wchar` support, and a conformant client then refuses to
                // marshal a `wstring` to it. See `codeset::server_component`.
                components: vec![codeset::server_component()],
            }],
        }
    }

    /// Which channel's which object a key names.
    ///
    /// The channel list is copied out from under the registry lock before any
    /// channel state is touched, so this never holds two locks at once — the
    /// discipline [`crate::guarded`] enforces for outbound calls, kept here
    /// for the ordinary reason as well.
    ///
    /// Membership is **exact**, never a prefix match: a minted key is in
    /// exactly one channel's tables, and the fixed keys are compared whole.
    /// A prefix match would have made [`is_channel_name_safe`] a suggestion,
    /// since `base/x/pps1` begins with `base` too.
    fn route(&self, key: &[u8]) -> Option<(Arc<ChannelObjects>, Target)> {
        let all: Vec<Arc<ChannelObjects>> = {
            let reg = self.inner.registry.lock().unwrap_or_else(|e| e.into_inner());
            reg.channels.values().map(Arc::clone).collect()
        };
        for objects in all {
            if key == objects.base {
                return Some((objects, Target::Channel));
            }
            if key == objects.consumer_admin {
                return Some((objects, Target::ConsumerAdmin));
            }
            if key == objects.supplier_admin {
                return Some((objects, Target::SupplierAdmin));
            }
            let found = {
                let state = objects.shared.lock();
                if state.proxy_suppliers.contains_key(key) {
                    Some(Target::ProxySupplier)
                } else if state.proxy_consumers.contains_key(key) {
                    Some(Target::ProxyConsumer)
                } else if state.proxy_pull_suppliers.contains_key(key) {
                    Some(Target::ProxyPullSupplier)
                } else if state.proxy_pull_consumers.contains_key(key) {
                    Some(Target::ProxyPullConsumer)
                } else {
                    None
                }
            };
            if let Some(target) = found {
                return Some((objects, target));
            }
        }
        None
    }

    /// Mints a proxy key **inside one channel's key space**, from that
    /// channel's own counter.
    ///
    /// Per channel and not per server, so the numbering restarts for each and
    /// two channels both mint `pps1` — under different prefixes, which is the
    /// whole reason the prefix rule has to hold.
    /// The objects of the channel created with the server.
    fn default_objects(&self) -> Arc<ChannelObjects> {
        let reg = self.inner.registry.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(
            reg.channels
                .get(&self.inner.default_name)
                .expect("the default channel is never removed"),
        )
    }

    fn mint_on(&self, objects: &ChannelObjects, tag: &str) -> Vec<u8> {
        let mut state = objects.shared.lock();
        state.minted += 1;
        let mut key = objects.base.clone();
        key.extend_from_slice(format!("/{tag}{}", state.minted).as_bytes());
        key
    }

    /// Dispatches one operation, writing the result body into `out`.
    ///
    /// Invariant every arm keeps, inherited from F6: nothing is written into
    /// `out` until the operation can no longer raise a *user* exception,
    /// because the buffer travels whole under a single reply status.
    fn invoke_operation(&self, req: &Request, out: &mut Encoder) -> std::result::Result<(), Raise> {
        // Which channel, and which of its objects. Every arm below reaches
        // `chan` and never a field of `self`: the servant holds no channel
        // state of its own any more, which is what makes "an operation on one
        // channel cannot touch another" structural rather than careful.
        let (chan, target) = self
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
                let ior = self.ior_for(&chan.consumer_admin, CONSUMER_ADMIN_ID);
                ior.write_to(out).map_err(|_| marshal())?;
            }
            (Target::Channel, "for_suppliers") => {
                let ior = self.ior_for(&chan.supplier_admin, SUPPLIER_ADMIN_ID);
                ior.write_to(out).map_err(|_| marshal())?;
            }

            // ── ConsumerAdmin ──
            (Target::ConsumerAdmin, "obtain_push_supplier") => {
                let key = self.mint_on(&chan, "pps");
                chan.shared.lock().proxy_suppliers.insert(key.clone(), ProxySupplier::default());
                self.ior_for(&key, PROXY_PUSH_SUPPLIER_ID).write_to(out).map_err(|_| marshal())?;
            }

            (Target::ConsumerAdmin, "obtain_pull_supplier") => {
                let key = self.mint_on(&chan, "pls");
                chan.shared
                    .lock()
                    .proxy_pull_suppliers
                    .insert(key.clone(), ProxyPullSupplier::default());
                self.ior_for(&key, PROXY_PULL_SUPPLIER_ID).write_to(out).map_err(|_| marshal())?;
            }

            // ── SupplierAdmin ──
            (Target::SupplierAdmin, "obtain_push_consumer") => {
                let key = self.mint_on(&chan, "ppc");
                chan.shared.lock().proxy_consumers.insert(key.clone(), ProxyConsumer::default());
                self.ior_for(&key, PROXY_PUSH_CONSUMER_ID).write_to(out).map_err(|_| marshal())?;
            }

            (Target::SupplierAdmin, "obtain_pull_consumer") => {
                let key = self.mint_on(&chan, "plc");
                chan.shared
                    .lock()
                    .proxy_pull_consumers
                    .insert(key.clone(), ProxyPullConsumer::default());
                self.ior_for(&key, PROXY_PULL_CONSUMER_ID).write_to(out).map_err(|_| marshal())?;
            }

            // ── ProxyPushSupplier: the consumer's end ──
            (Target::ProxySupplier, "connect_push_consumer") => {
                let consumer = Ior::read_from(&mut args).map_err(|_| marshal())?;
                if consumer.is_nil() {
                    // §2.3.6: a nil PushConsumer is BAD_PARAM. Accepting one
                    // would queue events for a reference nothing can dial.
                    return Err(bad_param());
                }
                let mut state = chan.shared.lock();
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
                let mut state = chan.shared.lock();
                if let Some(proxy) = state.proxy_suppliers.get_mut(&req.object_key) {
                    let abandoned = proxy.queue.len() as u64;
                    proxy.queue.clear();
                    proxy.consumer = None;
                    proxy.consecutive_failures = 0;
                    state.stats.discard(DropCause::Disconnect, abandoned);
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
                let mut state = chan.shared.lock();
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
                    let state = chan.shared.lock();
                    let proxy = state
                        .proxy_consumers
                        .get(&req.object_key)
                        .ok_or_else(|| Raise::System(SystemException::object_not_exist()))?;
                    if !proxy.connected {
                        return Err(UserExc::Disconnected.into());
                    }
                }
                let event = capture_event(&mut args, 0)?;
                // The lock is taken only to enqueue, and released before this
                // arm returns: the servant never calls out while holding it.
                chan.shared.lock().fan_out(event);
                chan.shared.wake.notify_all();
            }
            (Target::ProxyConsumer, "disconnect_push_consumer") => {
                let mut state = chan.shared.lock();
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
                let mut state = chan.shared.lock();
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

                let mut state = chan.shared.lock();
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
                    state = chan.shared.wait(&chan.shared.wake, state, left.min(PULL_POLL));
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
                chan.shared.progress.notify_all();
            }
            (Target::ProxyPullSupplier, "disconnect_pull_supplier") => {
                let mut state = chan.shared.lock();
                if let Some(proxy) = state.proxy_pull_suppliers.get_mut(&req.object_key) {
                    let abandoned = proxy.queue.len() as u64;
                    proxy.queue.clear();
                    proxy.connected = false;
                    proxy.consumer = None;
                    state.stats.discard(DropCause::Disconnect, abandoned);
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
                chan.shared.wake.notify_all();
            }

            // ── ProxyPullConsumer: the pulled supplier's end ──
            (Target::ProxyPullConsumer, "connect_pull_supplier") => {
                let supplier = Ior::read_from(&mut args).map_err(|_| marshal())?;
                if supplier.is_nil() {
                    // The mirror of `connect_push_consumer`, and the same
                    // answer for the same reason: this reference is the
                    // address the channel will dial, and the whole of what
                    // this proxy does is dial it. §2.3.6's `BAD_PARAM` for a
                    // nil `PushConsumer` is about a reference nothing can
                    // reach, which is exactly what a nil `PullSupplier` is
                    // here — unlike a nil `PullConsumer`, which is legal
                    // because it is never dialled at all.
                    return Err(bad_param());
                }
                let mut state = chan.shared.lock();
                let proxy = state
                    .proxy_pull_consumers
                    .get_mut(&req.object_key)
                    .ok_or_else(|| Raise::System(SystemException::object_not_exist()))?;
                if proxy.connected {
                    return Err(UserExc::AlreadyConnected.into());
                }
                proxy.connected = true;
                proxy.supplier = Some(supplier);
                proxy.consecutive_failures = 0;
                drop(state);
                // The source thread may be asleep between rounds; a supplier
                // that has just connected should not wait one out.
                chan.shared.wake.notify_all();
            }
            (Target::ProxyPullConsumer, "disconnect_pull_consumer") => {
                let mut state = chan.shared.lock();
                if let Some(proxy) = state.proxy_pull_consumers.get_mut(&req.object_key) {
                    proxy.connected = false;
                    proxy.supplier = None;
                    proxy.consecutive_failures = 0;
                }
                // Nothing is discarded and nothing is counted: this proxy
                // never held a queue. The key stays known and reconnectable,
                // and a second disconnect is not an error — the same two
                // choices every other proxy here makes.
                //
                // Clearing the flag under this lock is also what stops the
                // asking, and the module docs' point 4 states exactly how far
                // that goes: the source loop's commit point reads the same
                // flag under the same lock, so every round not already past it
                // is cancelled. This does **not** wait for a round that is —
                // a disconnect that could cost an outbound timeout is a worse
                // property than the one stray call, and the peer it would wait
                // on is free to be the caller.
                //
                // §2.3 says the proxy calls `disconnect_pull_supplier` back on
                // the supplier. This channel does not, for the reason it does
                // not call `disconnect_pull_consumer` back either: a servant
                // that makes an outbound call is the shape [`crate::guarded`]
                // exists to police, and the two outbound threads are the only
                // places this module dials anybody. The supplier learns on its
                // next visit that has stopped coming.
            }

            // `destroy` is declared by `CosEventChannelAdmin` and deliberately
            // not served, so the wire says `NO_IMPLEMENT`: a client can tell that
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
/// One is left: `destroy` is an unauthenticated remote operation that would
/// end the channel for every other client and cannot be undone without
/// restarting the process, and this servant has no notion of who is calling.
/// Its deferral turns on a **caller model**, not on anything the pull work
/// touched.
///
/// Both halves of pull have left this list, each when it was measured rather
/// than when it was argued. The **consumer** side went first (2026-08-18): it
/// is the same bounded queue drained from the other end, which is what its
/// reason claimed it could not be. The **supplier** side went second
/// (2026-08-25): its reason was a blocking `pull` holding a thread on somebody
/// else's clock, and `try_pull` on a round the channel owns is not that. What
/// is left of the old sentence is a real cost and it is paid where it is
/// visible — an invented interval, [`DEFAULT_SOURCE_POLL`].
///
/// Anything removed from here must gain a served arm in
/// [`EventChannelServer::invoke_operation`] in the same change, or the
/// operation silently degrades from a stated refusal to `BAD_OPERATION`, which
/// says "no such operation" and is a lie.
pub fn is_deferred(op: &str) -> bool {
    matches!(op, "destroy")
}

/// Reads an `any` verbatim out of a body that ends `trailing` octets after it.
///
/// The `any` runs to the end of what encloses it, which is the only way its
/// value length can be known: CDR gives an `any` no length prefix, so the
/// enclosing structure has to say where it ends. `trailing` is how that is
/// said — 0 for a `push` argument, which is the whole body, and 1 for a
/// `try_pull` reply, where the `out boolean` follows the return value.
///
/// One reader for both, because the alignment rule is the fragile half and it
/// is identical: a decoder over a whole GIOP message has the message start as
/// its origin, so the offset right after the `TypeCode` is the value's true
/// CDR alignment — for a request body and a reply body alike.
fn capture_event(
    args: &mut orbweaver_cdr::Decoder<'_>,
    trailing: usize,
) -> std::result::Result<Event, Raise> {
    let tc = typecode::decode(args).map_err(|_| marshal())?;
    let value_align = args.offset() % 8;
    let len = args.remaining().checked_sub(trailing).ok_or_else(marshal)?;
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
                let event = capture_event(&mut args, 0).map_err(|_| SystemException::marshal())?;
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
// A CosEventComm::PullSupplier of our own
// ─────────────────────────────────────────────────────────────────────────────

/// What a [`PullSupplierServant`] is holding, and what it has been asked for.
#[derive(Debug)]
struct SourceState {
    /// Offered events, oldest first. Unbounded on purpose: this is a supplier,
    /// and a supplier that silently discarded what it was given would make
    /// every count downstream a guess. A fixture that offers more than it can
    /// hold is a fixture with a bug, and it should show as memory rather than
    /// as missing events.
    queue: VecDeque<Event>,
    /// How many times the channel called the **blocking** `pull`.
    ///
    /// The reason this servant counts at all. "The channel uses `try_pull`"
    /// is a design decision in the module docs, and a decision no test can
    /// observe is a sentence; this makes it a number, and
    /// `the_channel_asks_with_try_pull_and_never_blocks_in_pull` reads it.
    pull_calls: u64,
    /// How many times the channel called `try_pull`.
    try_pull_calls: u64,
    disconnected: bool,
}

/// A view of what a [`PullSupplierServant`] holds and has been asked.
///
/// Cloning shares the storage, so a test or a spike keeps a view of a supplier
/// whose servant has been moved into a serving thread — the same shape
/// [`EventSink`] has on the push side, which this is the mirror of.
#[derive(Debug, Clone)]
pub struct EventSource {
    state: Arc<Guarded<SourceState>>,
}

impl Default for EventSource {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSource {
    /// A supplier holding nothing.
    pub fn new() -> Self {
        EventSource {
            state: Arc::new(Guarded::new(
                "an event source",
                SourceState {
                    queue: VecDeque::new(),
                    pull_calls: 0,
                    try_pull_calls: 0,
                    disconnected: false,
                },
            )),
        }
    }

    /// Makes an event available for the next `pull`/`try_pull`.
    ///
    /// The value is marshalled at an 8-aligned `any` start, which is where a
    /// GIOP 1.2 reply body begins, so the bytes carry the padding the reply
    /// will need — the same reasoning and the same construction as
    /// [`ChannelHandle::publish`], from the other side of the wire.
    ///
    /// `endian` is the byte order the event is captured in, and a reply may
    /// only carry it in that order ([`relay_check`]). Offer in the order the
    /// puller asks in — for the channel, [`ChannelHandle::set_source_endian`].
    pub fn offer<F>(&self, tc: &TypeCode, endian: Endian, write_value: F) -> Result<()>
    where
        F: FnOnce(&mut Encoder),
    {
        let mut e = Encoder::new(endian);
        typecode::encode(&mut e, tc)?;
        let tc_len = e.len();
        write_value(&mut e);
        let bytes = e.finish()?;
        let any = Any { tc: tc.clone(), value: bytes[tc_len..].to_vec(), endian };
        self.state.write(|s| s.queue.push_back(Event { any, value_align: tc_len % 8 }));
        Ok(())
    }

    /// Events offered and not yet taken.
    pub fn pending(&self) -> usize {
        self.state.read(|s| s.queue.len())
    }

    /// How many times the blocking `pull` was invoked on this supplier.
    pub fn pull_calls(&self) -> u64 {
        self.state.read(|s| s.pull_calls)
    }

    /// How many times `try_pull` was invoked on this supplier.
    pub fn try_pull_calls(&self) -> u64 {
        self.state.read(|s| s.try_pull_calls)
    }

    /// Makes this supplier answer `Disconnected` from now on, as a supplier
    /// that has finished does.
    pub fn disconnect(&self) {
        self.state.write(|s| s.disconnected = true);
    }

    /// Whether `disconnect_pull_supplier` has been called on the servant.
    pub fn is_disconnected(&self) -> bool {
        self.state.read(|s| s.disconnected)
    }

    /// Waits until everything offered has been taken. Returns whether it was.
    ///
    /// The sleep is not decoration — a spin here is the Phase 0 wait loop that
    /// does not wait, and it would burn the core the source thread needs.
    pub fn wait_until_drained(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if self.pending() == 0 {
                return true;
            }
            if Instant::now() >= deadline {
                return self.pending() == 0;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// A minimal `CosEventComm::PullSupplier` servant: it holds what it was
/// offered until somebody comes and asks.
///
/// **This is the object `PLAN-DEFERRED` §10's trigger named** — *"a named
/// `PullSupplier` in this workspace, something that **is** one"*. It exists
/// for the reason [`PushConsumerServant`] does: the channel needs something to
/// pull *from*, in the tests and in the spike, and a fixture written in a test
/// module is a fixture no other crate and no spike can reach.
#[derive(Debug)]
pub struct PullSupplierServant {
    key: Vec<u8>,
    source: EventSource,
}

impl PullSupplierServant {
    /// A supplier answering to `object_key`.
    pub fn new(object_key: Vec<u8>) -> Self {
        Self { key: object_key, source: EventSource::new() }
    }

    /// A view of what it holds and what it has been asked.
    pub fn source(&self) -> EventSource {
        self.source.clone()
    }

    /// A publishable reference to this supplier.
    pub fn ior(&self, host: &str, port: u16) -> Ior {
        Ior {
            type_id: PULL_SUPPLIER_ID.to_owned(),
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

    /// Writes one held event into `out`, or `None` if there is none to write.
    ///
    /// The alignment rule is the channel's, applied from the supplying side:
    /// an `any` captured at one offset in one byte order may only be written
    /// at that offset in that byte order, and [`relay_check`] is the single
    /// predicate that says so for every path in this module.
    fn hand_over(&self, out: &mut Encoder) -> std::result::Result<bool, Raise> {
        let at = out.position();
        let endian = out.endian();
        let taken = self.source.state.write(|s| {
            while let Some(event) = s.queue.pop_front() {
                if relay_check(&event, at, endian).is_ok() {
                    return Some(event);
                }
                eprintln!(
                    "orbweaver: pull supplier cannot hand back an event captured \
                     {:?}-endian into a {endian:?}-endian reply; it was dropped",
                    event.any.endian
                );
            }
            None
        });
        match taken {
            Some(event) => {
                typecode::encode_any_at_same_alignment(out, &event.any).map_err(|_| marshal())?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn invoke_operation(&self, req: &Request, out: &mut Encoder) -> std::result::Result<(), Raise> {
        if req.object_key != self.key {
            return Err(Raise::System(SystemException::object_not_exist()));
        }
        let mut args = req.body().map_err(|_| marshal())?;
        match req.operation.as_str() {
            "_is_a" => {
                let id = args.get_string().map_err(|_| marshal())?;
                out.put_bool(id == PULL_SUPPLIER_ID || id == CORBA_OBJECT_ID);
            }
            "_non_existent" => out.put_bool(false),
            "try_pull" => {
                if self.source.state.write(|s| {
                    s.try_pull_calls += 1;
                    s.disconnected
                }) {
                    return Err(UserExc::Disconnected.into());
                }
                let has_event = self.hand_over(out)?;
                if !has_event {
                    // §2.1.1: nothing to give is a `tk_null` `any` and a false
                    // `has_event`, not an exception.
                    typecode::encode(out, &TypeCode::Null).map_err(|_| marshal())?;
                }
                out.put_bool(has_event);
            }
            "pull" => {
                if self.source.state.write(|s| {
                    s.pull_calls += 1;
                    s.disconnected
                }) {
                    return Err(UserExc::Disconnected.into());
                }
                // `pull` blocks until there is something, bounded and reported
                // as `TIMEOUT`/`COMPLETED_NO` on expiry — the same contract
                // this module's own `ProxyPullSupplier` gives, and for the same
                // reason: an unbounded block is a serving thread a vanished
                // client keeps for the life of the process. Sleeping, never
                // spinning, and nothing is held across the sleep.
                let deadline = Instant::now() + DEFAULT_PULL_BLOCK;
                loop {
                    if self.hand_over(out)? {
                        break;
                    }
                    if Instant::now() >= deadline {
                        return Err(pull_timed_out());
                    }
                    std::thread::sleep(PULL_POLL);
                }
            }
            "disconnect_pull_supplier" => self.source.state.write(|s| s.disconnected = true),
            op if is_deferred(op) => return Err(SystemException::no_implement().into()),
            _ => return Err(SystemException::bad_operation().into()),
        }
        Ok(())
    }
}

/// A supplier's whole state is the queue it was offered, and nothing it does
/// calls out, so two pulls landing at once is the ordinary case.
impl SharedDispatch for PullSupplierServant {
    fn knows(&self, object_key: &[u8]) -> bool {
        object_key == self.key
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
                ex.write(out);
                Ok(DispatchBody::UserException)
            }
        }
    }

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

impl Dispatch for PullSupplierServant {
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

    /// `SupplierAdmin::obtain_pull_consumer` — the proxy a supplier hands its
    /// own `PullSupplier` to so the channel will come and ask for events.
    pub fn obtain_pull_consumer(conn: &mut Connection) -> Result<Ior> {
        reference(conn, "obtain_pull_consumer")
    }

    /// `ProxyPullConsumer::connect_pull_supplier`. A nil `supplier` is
    /// `BAD_PARAM`: this reference is dialled, which is the whole of what the
    /// proxy does.
    pub fn connect_pull_supplier(conn: &mut Connection, supplier: &Ior) -> Result<()> {
        let supplier = supplier.clone();
        conn.invoke("connect_pull_supplier", move |e| {
            let _ = supplier.write_to(e);
        })?;
        Ok(())
    }

    /// `ProxyPullConsumer::disconnect_pull_consumer`.
    pub fn disconnect_pull_consumer(conn: &mut Connection) -> Result<()> {
        conn.invoke_nullary("disconnect_pull_consumer")?;
        Ok(())
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
        servant: Arc<EventChannelServer>,
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
            let servant = Arc::clone(&channel);
            // `serve_shared`: the channel answers two calls at once now, which
            // is what makes the delivery/serving overlap tests below test the
            // thing they claim to.
            let thread = std::thread::spawn(move || {
                server.serve_shared(&*servant, move || flag.load(Ordering::SeqCst)).unwrap();
            });
            Served {
                servant: channel,
                channel: ior,
                handle,
                delivery: None,
                stats,
                stop,
                thread: Some(thread),
            }
        }

        /// Starts both outbound threads against the already-serving server,
        /// through its own public entry point rather than by reaching into its
        /// internals — so a test cannot start a shape the product cannot.
        fn begin_delivery(&mut self) {
            self.delivery = Some(self.servant.start_delivery_with(PUSH_T));
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

    /// Every drop cause, so a new one cannot be added without this array
    /// noticing that [`assert_only_cause`] does not check it.
    const EVERY_CAUSE: [DropCause; 5] = [
        DropCause::Overflow,
        DropCause::Unrelayable,
        DropCause::Disconnect,
        DropCause::FailureDisconnect,
        DropCause::Stop,
    ];

    /// Asserts that **one** drop cause moved, by `n`, that no other did, and
    /// that the per-cause counters account for the whole of `dropped`.
    ///
    /// This is the assertion the old single counter could not make. Sum the
    /// five back into one number and every caller of this function fails
    /// naming the cause it could not tell apart from the others — which is
    /// the negative control recorded in this batch's commit message.
    fn assert_only_cause(stats: &ChannelStats, cause: DropCause, n: u64) {
        for c in EVERY_CAUSE {
            let got = match c {
                DropCause::Overflow => stats.dropped_overflow,
                DropCause::Unrelayable => stats.unrelayable,
                DropCause::Disconnect => stats.dropped_on_disconnect,
                DropCause::FailureDisconnect => stats.dropped_on_failure_disconnect,
                DropCause::Stop => stats.dropped_at_stop,
            };
            let want = if c == cause { n } else { 0 };
            assert_eq!(got, want, "drop cause {c:?} should be {want}: {stats:?}");
        }
        assert_eq!(stats.dropped, n, "the total is the one cause that moved: {stats:?}");
        assert!(stats.split_adds_up(), "the split must account for every drop: {stats:?}");
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

    /// The deferral list, on the wire and in the predicate, now that both
    /// halves of the pull model have left it and only `destroy` remains.
    ///
    /// What is still refused answers `NO_IMPLEMENT` — a decision a client can
    /// read. What is no longer refused answers with a *reference*, which is
    /// the only unambiguous evidence that the arm exists: an operation removed
    /// from [`is_deferred`] without an arm behind it would degrade to
    /// `BAD_OPERATION`, and this test would see the difference.
    ///
    /// `destroy`'s deferral is untouched by this batch on purpose. It turns on
    /// a caller model reaching this servant (`PLAN-DEFERRED` §11) and not on
    /// how many models the channel serves, so serving all four leaves it
    /// exactly as refused as it was.
    #[test]
    fn only_destroy_answers_no_implement() {
        assert!(super::is_deferred("destroy"), "destroy must stay on the deferral list");
        for op in [
            "obtain_pull_supplier",
            "connect_pull_consumer",
            "pull",
            "try_pull",
            "disconnect_pull_supplier",
            "obtain_pull_consumer",
            "connect_pull_supplier",
            "disconnect_pull_consumer",
        ] {
            assert!(!super::is_deferred(op), "{op} is served now and must be off the list");
        }

        let served = Served::start();
        let mut conn = served.channel_conn();
        let consumers = client::for_consumers(&mut conn).unwrap();
        let suppliers = client::for_suppliers(&mut conn).unwrap();

        match conn.invoke_nullary("destroy") {
            Err(Error::SystemException { id, .. }) => {
                assert_eq!(id, crate::server::NO_IMPLEMENT, "destroy");
            }
            other => panic!("expected NO_IMPLEMENT for destroy, got {other:?}"),
        }
        // An operation the *channel* does not declare is `BAD_OPERATION` even
        // though some other object on this servant declares it — the routing
        // is per object, which is what makes `NO_IMPLEMENT` mean something.
        for op in ["obtain_pull_consumer", "disconnect_pull_consumer"] {
            match conn.invoke_nullary(op) {
                Err(Error::SystemException { id, .. }) => {
                    assert_eq!(id, crate::server::BAD_OPERATION, "{op} on the channel itself");
                }
                other => panic!("expected BAD_OPERATION for {op}, got {other:?}"),
            }
        }
        drop(conn);

        let mut c = served.dial(&consumers);
        let pls = client::obtain_pull_supplier(&mut c).unwrap();
        assert_eq!(pls.type_id, PROXY_PULL_SUPPLIER_ID);
        assert_ne!(
            pls.primary().unwrap().object_key,
            served.channel.primary().unwrap().object_key,
            "a pull proxy is its own object"
        );
        drop(c);

        // The SupplierAdmin mints a pull *consumer* now — a reference, which
        // is the evidence the arm exists — and an operation belonging to
        // neither admin is still `BAD_OPERATION`.
        let mut c = served.dial(&suppliers);
        let plc = client::obtain_pull_consumer(&mut c).unwrap();
        assert_eq!(plc.type_id, PROXY_PULL_CONSUMER_ID);
        assert_ne!(
            plc.primary().unwrap().object_key,
            pls.primary().unwrap().object_key,
            "the two pull proxies are distinct objects"
        );
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
        assert_eq!(stats.fanned_out, 9, "one connected proxy, so one copy of each");
        assert_eq!(stats.queued, 3, "the queue is bounded");
        assert_eq!(stats.dropped, 6, "every discarded event is counted, none silently");
        assert_eq!(stats.delivered, 0, "no delivery thread is running yet");
        // The cause, not just the count. Overflow is the one drop cause that
        // means back-pressure, and it is the only one this test drove.
        assert_only_cause(&stats, DropCause::Overflow, 6);

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
        assert_eq!(stats.fanned_out, 12, "two proxies, six events each");
        // A cut consumer is not an overloaded one: nothing here overflowed,
        // and a reader asking about back-pressure must not be handed these.
        assert_only_cause(&stats, DropCause::FailureDisconnect, 3);

        served.shutdown(conn);
        live.shutdown();
    }

    /// A consumer that hangs up on purpose is **housekeeping**, and after the
    /// split it says so: its abandoned backlog counts under its own cause and
    /// leaves the back-pressure counter alone.
    ///
    /// Delivery is paused, so the backlog is exactly what was pushed and the
    /// count is not a race against a drain.
    #[test]
    fn a_consumers_own_disconnect_abandons_its_backlog_under_its_own_cause() {
        let served = Served::start_paused();
        let proxy = served.consumer_proxy(&dead_consumer_ior());

        let conn = served.supplier_proxy();
        let mut supplier = conn;
        for i in 0..4u32 {
            client::push(&mut supplier, &TypeCode::ULong, move |e| e.put_u32(i)).unwrap();
        }
        assert_eq!(served.handle.stats().queued, 4);

        let mut c = served.dial(&proxy);
        client::disconnect_push_supplier(&mut c).unwrap();
        drop(c);

        let stats = served.handle.stats();
        assert_eq!(stats.queued, 0, "the backlog goes with the connection");
        assert_eq!(stats.consumers_connected, 0);
        assert_only_cause(&stats, DropCause::Disconnect, 4);

        served.shutdown(supplier);
    }

    /// A tidy shutdown is not a symptom. `stop` abandoning a backlog counts
    /// under its own cause, so an operator reading `dropped_overflow` after a
    /// restart sees zero rather than the size of the queue at the moment
    /// somebody pressed the button — which is what the single counter showed
    /// and what made `PLAN-DEFERRED.md` §1's trigger unanswerable.
    #[test]
    fn stopping_the_channel_counts_its_backlog_at_stop_and_never_as_pressure() {
        let served = Served::start_paused();
        served.consumer_proxy(&dead_consumer_ior());

        let mut supplier = served.supplier_proxy();
        for i in 0..5u32 {
            client::push(&mut supplier, &TypeCode::ULong, move |e| e.put_u32(i)).unwrap();
        }
        assert_eq!(served.handle.stats().queued, 5);

        served.handle.stop();
        let stats = served.handle.stats();
        assert_eq!(stats.queued, 0, "a stopped channel keeps nothing");
        assert_only_cause(&stats, DropCause::Stop, 5);

        // Idempotent, and it must not re-count: `Delivery::drop` calls `stop`
        // too, so a second call is the ordinary case rather than an odd one.
        served.handle.stop();
        assert_only_cause(&served.handle.stats(), DropCause::Stop, 5);

        served.shutdown(supplier);
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
        let objects = channel.default_objects();
        let key = channel.mint_on(&objects, "ppc");
        objects.shared.lock().proxy_consumers.insert(key.clone(), ProxyConsumer::default());

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
            let Ok(event) = capture_event(&mut args, 0) else {
                panic!("{endian:?}: capture failed")
            };
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
