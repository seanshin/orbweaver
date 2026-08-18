//! Several requests in flight on one connection, replies correlated by
//! `request_id`.
//!
//! [`Connection`] sends one request and blocks until its reply arrives. GIOP has carried a `request_id` since 1.0 for exactly the
//! reason that makes this wasteful: the field exists so that replies need not
//! come back in the order the requests went out. This module spends that
//! field.
//!
//! *하나의 연결에 여러 요청을 동시에 띄우고, 응답은 `request_id`로 맞춘다.*
//!
//! # What the specification actually says, version by version
//!
//! The question that decides this module's shape is whether GIOP 1.0 and 1.1
//! permit what 1.2 permits. They are not the same, but the difference is not
//! where it is usually assumed to be.
//!
//! **Correlation is version-independent.** Connection Management (CORBA 2.2
//! §13.5.1, carried forward as CORBA 3.4 §9.5.1 — the text predates GIOP 1.2)
//! says: *"Request IDs must unambiguously associate replies with requests
//! within the scope and lifetime of a connection. Request IDs may be re-used
//! if there is no possibility that the previous request using the ID may still
//! have a pending reply."* A rule about when an id may be re-used is a rule
//! written for a client that has **several** requests pending at once; it says
//! so in the same sentence. The same section adds *"Request IDs must be unique
//! among both Request and LocateRequest messages"*, which is why one counter
//! serves both here and in [`Connection`]. So multiplexing is not a 1.2
//! feature and this module does not pretend it is.
//!
//! **Fragmentation is where the versions part.** A GIOP 1.1 `Fragment` carries
//! no request id — `FragmentHeader` was added in 1.2 (§9.4.9) — so a
//! fragmented 1.1 reply is attributable only by *position*: it belongs to
//! whatever message was last opened on the connection. That attribution is
//! correct only if nothing else was interleaved, and a receiver has no way to
//! check whether it was. A 1.2 receiver checks: the id in the fragment header
//! must match, and [`read_message`] already refuses when it does not. The 1.1 receiver has nothing to check against, so a peer that
//! interleaved would be silently mis-attributed rather than refused, and
//! "produce a plausible wrong value" is the one outcome this codebase treats
//! as worse than failing.
//!
//! **So: this module multiplexes on GIOP 1.2 and refuses below it.** Below 1.2
//! a [`Mux`] still works — it holds a turnstile so that exactly one request is
//! in flight, which is what [`Connection`] does — and [`Mux::send`], the API
//! whose whole point is a second request in flight,
//! returns [`Error::MultiplexingUnsupported`]. The refusal is conservatism and
//! is labelled as such: §13.5.1 would permit it, and what stops us is that
//! *we* cannot verify a 1.1 peer's fragmenting behaviour, not that the peer is
//! forbidden to multiplex. The cost of being wrong in the other direction is
//! concrete: at 1.1 our reader answers a fragmented reply with
//! [`Error::FragmentUnsupported`], and with N requests in flight that one
//! peer's decision fails all N callers instead of one.
//!
//! And the cost is not hypothetical. `spike-mux` asked omniORB 4.3.4 for a
//! 1 MB reply at GIOP 1.1: it **fragmented** it, and the reply was refused
//! exactly as described. At 1.2 the same call reassembles from a real peer's
//! two pieces. That is the version rule, measured on both sides of the line.
//!
//! # What the peers do, measured
//!
//! Both of them answer **out of order**, in their default configurations, with
//! nothing configured at either end (`spike-mux`, 2026-08-14, 12 pipelined
//! calls, alternating a 1 MB `blob` with a `ping`):
//!
//! | peer | in flight | replies that overtook an older request |
//! |---|---|---|
//! | omniORB 4.3.4 | 12 | 4–8 across runs |
//! | JacORB 3.9 | 12 | 6–10 across runs |
//!
//! Worth recording because the expectation was the opposite: omniORB documents
//! one thread per connection as its default (`threadPerConnectionPolicy = 1`),
//! which reads as "answers in order", and this was expected to need thread-pool
//! mode before out-of-order replies could appear at all. It did not. A client
//! that assumed reply order — which is what a connection with one request in
//! flight lets you get away with — would have been wrong against a stock
//! omniORB the first time it pipelined anything.
//!
//! # Who reads the socket
//!
//! **The caller that is waiting, one at a time.** Not a reader thread per
//! connection.
//!
//! A waiter loops: look in the inbox for my reply; if it is not there, try to
//! become *the* reader; if somebody else already is, sleep on the condition
//! variable until either my reply lands or my turn to read comes. Whoever
//! holds the read half reads one whole logical message, files it under its
//! request id, wakes everybody, and lets the read half go.
//!
//! Why not a reader thread — three reasons, in order of how much they cost:
//!
//! 1. **Lifetime.** A thread per connection is a thread per *pooled*
//!    connection: [`crate::pool`] holds up to [`crate::pool::DEFAULT_MAX_TOTAL`]
//!    of them, so the reader-thread design pays 64 parked threads for an idle
//!    process, and every one of them has to be joined on shutdown by a path
//!    that must not itself block. The leader design has no thread to own, so
//!    dropping a [`Mux`] closes a socket and nothing else.
//! 2. **The deadline already belongs to the caller.** A reader thread blocked
//!    in `read` cannot be told "the caller you were reading for gave up"
//!    without a second channel to tell it on. Here the caller *is* the reader,
//!    so its own deadline bounds its own read, and a caller that gives up
//!    simply stops reading.
//! 3. **It matches what this crate is.** Everything here is synchronous
//!    `std::net`; a reader thread would be the first piece of scheduling
//!    machinery in the client path, and it would need its own error routing,
//!    its own shutdown protocol and its own tests before it delivered a single
//!    reply.
//!
//! What it costs, stated rather than hidden: a reply is read only while
//! somebody is waiting for *a* reply. Nobody waiting means nobody reading, so
//! a `CloseConnection` that arrives while the connection is idle is noticed at
//! the next call rather than when it arrives. That is the same latency
//! [`Connection`] has always had, and [`crate::pool`] turns it into a retry
//! rather than an error.
//!
//! # A caller whose reply never comes
//!
//! Two deadlines, and they answer different questions.
//!
//! The **socket** timeout, set when the connection was dialed, bounds a read.
//! When it fires the leader asks the question that decides everything: *did
//! this read consume any bytes?* If it consumed none, nothing is wrong with
//! the connection — the wire was simply quiet — so the leader yields and the
//! mux stays healthy. If it consumed some, a logical message is half-read, the
//! framing can no longer be trusted, and the mux faults for everybody. That
//! distinction is why the read goes through `Counting` instead of straight
//! at the stream: `read_exact` reports failure without saying how much it
//! swallowed, and a mux that treated every quiet second as corruption would
//! destroy healthy connections on a slow service.
//!
//! The **call** deadline is the caller's own, passed to [`Mux::call`], and it
//! is what a follower's condition-variable wait is bounded by. When it expires
//! the caller gets [`Error::Timeout`] carrying its request id, its slot is
//! marked abandoned, and **the connection is left usable** — one caller's
//! patience running out says nothing about the other callers on the wire. The
//! late reply, if it ever comes, is dropped and counted in
//! [`MuxStats::orphaned`]; it cannot be mistaken for anybody else's because
//! ids are never re-used, which is the other half of §13.5.1's rule.
//!
//! No `CancelRequest` is sent on a timeout, deliberately. §9.4.4 makes it
//! advisory — "the target MAY ignore it" — and the measured behaviour recorded
//! on [`Connection::cancel`] is that omniORB 4.3.4
//! *closes the connection* on a 1.0/1.1 `CancelRequest`. Sending one
//! automatically would convert one caller's timeout into every other caller's
//! failure, on a connection they are all sharing. The message stays available
//! as [`Mux::cancel`] for a caller that wants it and knows its peer.
//!
//! # Fragments, and not creating the interleaving the reassembler rejects
//!
//! §9.4.9 lets a request or reply be split, and
//! [`read_message`] reassembles it by reading the leading
//! message and every continuation *in a row*, refusing anything else in
//! between. Multiplexing is precisely the machinery that could break that, on
//! both sides:
//!
//! - **Outbound**: every piece [`fragment_message`]
//!   produces is written under one hold of the write half, so no other
//!   thread's request can land between them. This is required, not tidiness:
//!   §9.4.9 says a client that has fragmented a request header *"may not send
//!   another Request message until after the request ID is sent"*, and holding
//!   the write half across the whole message satisfies that by construction
//!   instead of by counting bytes.
//! - **Inbound**: reassembly happens inside the leader's hold of the read
//!   half, so no second reader can consume a continuation the reassembler is
//!   waiting for. If the *peer* interleaves, [`read_message`] answers
//!   `UnexpectedMessage` exactly as it did before this module existed, and the
//!   mux faults rather than mis-filing anything.
//!
//! **Closed 2026-08-14.** That gap used to be recorded here: a
//! `CloseConnection` arriving *between* the fragments of a reply was refused as
//! an unexpected message, reduced to `Desynchronized` by the fault, and so
//! never retried by [`crate::pool`]. It now arrives as
//! [`Error::InterruptedMidReassembly`], and the fault it makes is the only one
//! that answers per caller: the call whose reply was already coming back is
//! **not** re-sendable — the peer had processed it, whatever §13.5.1 says about
//! requests without replies — while every other caller on the connection gets
//! [`Error::ConnectionClosed`] and its re-send. Reporting one answer for both
//! groups is wrong in one direction or the other whichever answer is chosen,
//! which is why there are two.
//!
//! Still not observed from a peer: this needs a server to shut down inside the
//! window between two fragments, and neither fixture will do it on command. The
//! oracle is a scripted TCP peer built from this crate's own encoders.
//!
//! # Locks
//!
//! Three, and no thread ever holds more than one of the pair that could
//! deadlock:
//!
//! - the **write half** and the **read half** are `Mutex<Stream>`, and they
//!   are not [`Section`]s. They exist to serialize blocking I/O, so "nothing
//!   blocking inside the lock" cannot be the rule for them; what
//!   [`crate::guarded`] forbids is holding *state* across a blocking call, and
//!   these hold no state — they are the socket.
//! - the **inbox** is a `Mutex` plus a `Condvar` and it *is* a [`Section`],
//!   registered exactly the way [`crate::event_server`] registers its own, so
//!   the one tripwire sees it. Nothing blocking happens inside it: messages
//!   are decoded before it is taken and the condvar wait carries the section
//!   through rather than blocking with the mutex held.
//!
//! The one nesting that exists is **read half → inbox**, taken by the leader
//! to file what it just read before it lets the read half go. Filing first is
//! not an optimisation: releasing the read half first would let the caller
//! whose reply is in the leader's hand become the leader itself and block
//! reading a message that will never come. The reverse nesting, inbox → read
//! half, happens only through `try_lock`, so it can never wait and the cycle
//! cannot close. That is the whole deadlock argument, and it is short because
//! the discipline this crate already had made it short.
//!
//! Because a [`Section`] may not be entered twice on one thread,
//! [`Mux::in_flight`], [`Mux::is_usable`] and [`Mux::idle_for`] are answered
//! from atomics rather than from the inbox — [`crate::pool`] asks all three
//! from inside its own lock, and the tripwire would fire if they took one.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::{Duration, Instant};

use orbweaver_cdr::{Encoder, Endian};

use crate::guarded::{Section, assert_nothing_held};
use crate::{
    Connection, Error, Ior, MsgType, RawMessage, Reply, ReplyStatus, Result, ServiceContext,
    Stream, Version, codeset, decode_reply, encode_cancel_request, encode_request_with_contexts,
    fragment_message, read_message,
};

/// How long [`Mux::call`] waits for a reply when the caller does not say.
///
/// Deliberately longer than a dial timeout and shorter than forever: this
/// bounds a *caller*, not the connection, so it has to outlast a slow servant
/// without leaving a thread parked on a service that will never answer.
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a follower sleeps before re-checking whether it can become the
/// reader.
///
/// Leadership changes hands under the inbox lock and the wake-up goes out
/// there, but a follower woken while the outgoing leader still holds the read
/// half would find it busy and go back to sleep. Rather than build a handover
/// protocol for a case that costs microseconds, followers re-poll. The bound
/// is the reason a stranded reader is impossible rather than unlikely — the
/// same reason [`crate::server::STOP_POLL`] exists, and the same rule as the
/// harness's "wait loops must sleep".
const LEADER_POLL: Duration = Duration::from_millis(25);

/// The name the inbox section is reported under when the discipline is
/// violated.
const MUX_LOCK: &str = "a multiplexed connection's inbox";

/// How many abandoned slots may pile up before they are swept.
///
/// A caller that times out leaves its slot behind so the late reply can be
/// recognised and dropped rather than mistaken for somebody else's. That
/// bookkeeping has to be bounded or a long-lived connection with a failing
/// service grows a map nobody ever reads. Swept entries become "an id we do
/// not know", which is already a counted, non-fatal case.
const MAX_ABANDONED: usize = 4096;

/// Process start, so "last used" can live in an atomic instead of behind a
/// lock the pool would have to take from inside its own.
static EPOCH: OnceLock<Instant> = OnceLock::new();

fn now_ms() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

// ─────────────────────────────────────────────────────────────────────────────
// Outcomes
// ─────────────────────────────────────────────────────────────────────────────

/// What a completed call turned out to be.
///
/// A `LOCATION_FORWARD` is handed back rather than followed, because a [`Mux`]
/// is one connection to one endpoint and a forward may point anywhere. Whoever
/// can dial follows it; that is [`crate::pool::Pool`], and for a bare `Mux`
/// it is the caller.
#[derive(Debug)]
pub enum Sent {
    /// The target answered.
    Reply(Box<Reply>),
    /// The target moved; the body carried the reference to retry against.
    Forward(Box<Ior>),
}

/// A failed call, plus the one fact a retry needs.
#[derive(Debug)]
pub struct Failed {
    /// What went wrong.
    pub error: Error,
    /// Whether the peer provably did **not** process this request, so
    /// re-sending it on another connection is safe even when the operation is
    /// not idempotent.
    ///
    /// Two cases set it, and both are the specification's own statements
    /// rather than an optimistic guess:
    ///
    /// - `CloseConnection` (§13.5.1): *"any outstanding messages (i.e.,
    ///   without replies) were received after the server sent the
    ///   CloseConnection message, were not processed, and may be safely resent
    ///   on a new connection."*
    /// - a **failed write**: a GIOP message the peer never finished reading
    ///   cannot be dispatched, and the connection is discarded on the spot, so
    ///   the rest of those bytes will never arrive. The peer sees a truncated
    ///   message and an EOF.
    ///
    /// It is never set for a failure that happened *after* a complete request
    /// went out. That case is genuinely unknown — the servant may well have
    /// run — and reporting it as unsent would silently duplicate calls.
    pub unsent: bool,
}

impl From<Failed> for Error {
    fn from(f: Failed) -> Error {
        f.error
    }
}

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.unsent {
            write!(f, "{} (not processed; safe to re-send)", self.error)
        } else {
            write!(f, "{}", self.error)
        }
    }
}

impl std::error::Error for Failed {}

fn failed(error: Error, unsent: bool) -> Failed {
    Failed { error, unsent }
}

/// A call's outcome, or the reason there will not be one.
pub type Answered = std::result::Result<Sent, Failed>;

// ─────────────────────────────────────────────────────────────────────────────
// Faults
// ─────────────────────────────────────────────────────────────────────────────

/// Why a connection stopped being usable, in a form every waiter can be handed
/// a copy of.
///
/// [`Error`] is not `Clone` — it carries an `std::io::Error` and a decoded
/// reply — and a fault has to be told to N waiting callers at once, so the
/// reason is stored in this reduced form and rebuilt per caller.
#[derive(Debug, Clone)]
enum Fault {
    /// §9.4.7 `CloseConnection`, between messages. Retryable for everybody.
    Closed,
    /// Framing can no longer be trusted.
    Desynchronized,
    /// The transport failed.
    Io(std::io::ErrorKind, String),
    /// The peer sent something that has no place here.
    Unexpected(MsgType),
    /// An orderly control message arrived where the continuation of a
    /// fragmented reply was due — [`Error::InterruptedMidReassembly`].
    ///
    /// The only fault that does not mean the same thing to every waiter, which
    /// is why [`Fault::to_error`] and [`Fault::unsent`] take the caller's
    /// request id. One caller had a reply *in flight*; the others had nothing
    /// back at all, and §13.5.1 covers them and not it.
    Interrupted {
        /// `CloseConnection` or `MessageError`.
        control: MsgType,
        /// What was being reassembled.
        partial: MsgType,
        /// Whose reply it was.
        request_id: u32,
        /// How many pieces of it had arrived.
        received: usize,
    },
}

impl Fault {
    /// The error to hand the caller waiting on `waiter`.
    ///
    /// Per caller rather than per connection, because an interruption is one
    /// event with two meanings: the caller whose reply was cut short needs the
    /// full context — it is the one request that may already have run — while
    /// everybody else simply met a closed connection, and telling them about
    /// somebody else's request id would be noise they cannot act on.
    fn to_error(&self, waiter: u32) -> Error {
        match self {
            Fault::Closed => Error::ConnectionClosed,
            Fault::Desynchronized => Error::Desynchronized,
            Fault::Io(kind, msg) => Error::Io(std::io::Error::new(*kind, msg.clone())),
            Fault::Unexpected(t) => Error::UnexpectedMessage(*t),
            Fault::Interrupted { control, partial, request_id, received }
                if waiter == *request_id =>
            {
                Error::InterruptedMidReassembly {
                    control: *control,
                    partial: *partial,
                    request_id: *request_id,
                    received: *received,
                }
            }
            Fault::Interrupted { control: MsgType::CloseConnection, .. } => Error::ConnectionClosed,
            Fault::Interrupted { control, .. } => Error::UnexpectedMessage(*control),
        }
    }

    /// Whether the caller waiting on `waiter` may re-send.
    fn unsent(&self, waiter: u32) -> bool {
        match self {
            Fault::Closed => true,
            // §13.5.1's promise is about requests *without replies*. The peer
            // had begun this one's reply, so it was processed; every other
            // caller on the connection is still covered. Getting this wrong in
            // the generous direction duplicates a non-idempotent call, which is
            // the one failure mode a retry is supposed to be worth.
            Fault::Interrupted { control: MsgType::CloseConnection, request_id, .. } => {
                waiter != *request_id
            }
            // A `MessageError` says the peer could not parse something we sent
            // and names nothing (§9.4.8 carries no body). With no way to tell
            // which message it means, no request may be called unsent.
            Fault::Interrupted { .. } => false,
            Fault::Desynchronized | Fault::Io(..) | Fault::Unexpected(_) => false,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The inbox
// ─────────────────────────────────────────────────────────────────────────────

/// What has happened to one request id.
#[derive(Debug)]
enum Slot {
    /// Sent; a caller is waiting.
    Waiting,
    /// Answered; the caller has not picked it up yet.
    Ready(Box<Reply>),
    /// The caller gave up. The reply, if it comes, is dropped and counted.
    Abandoned,
}

/// Counters a caller can read back, so a claim about concurrency can be
/// checked instead of asserted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MuxStats {
    /// Requests written that expected a reply.
    pub sent: u64,
    /// Replies handed to a caller.
    pub answered: u64,
    /// Replies that arrived while an **older** request was still outstanding.
    ///
    /// This is the witness that a peer really did answer out of order, and it
    /// is counted where the reply is filed — inside the lock that owns the
    /// outstanding set — rather than from timing. Ids are allocated under the
    /// write half, so a smaller id provably went out first and "older" is a
    /// fact about the wire, not an inference about scheduling.
    pub out_of_order: u64,
    /// Replies that arrived for a request whose caller had already given up.
    pub orphaned: u64,
    /// The most requests that were ever outstanding at one instant.
    ///
    /// Outstanding means written and unanswered — the peer has the bytes — so
    /// unlike a counter taken around a lock this cannot mistake queueing for
    /// concurrency. It is the measurement the concurrent-dispatch batch got
    /// wrong and is written this way because of it.
    pub peak_in_flight: usize,
    /// The most fragments any one reply arrived in.
    pub max_reply_fragments: usize,
}

#[derive(Debug, Default)]
struct Inbox {
    slots: HashMap<u32, Slot>,
    fault: Option<Fault>,
    stats: MuxStats,
    in_flight: usize,
}

/// The inbox, held — and registered as held, so [`crate::guarded`]'s tripwire
/// sees it. Same shape and same reason as `event_server`'s guard.
struct Held<'a> {
    // Declared first so the mutex is released before the section closes.
    inbox: MutexGuard<'a, Inbox>,
    section: Section,
}

impl std::ops::Deref for Held<'_> {
    type Target = Inbox;

    fn deref(&self) -> &Inbox {
        &self.inbox
    }
}

impl std::ops::DerefMut for Held<'_> {
    fn deref_mut(&mut self) -> &mut Inbox {
        &mut self.inbox
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The wire
// ─────────────────────────────────────────────────────────────────────────────

/// The transport, split into halves when it can be.
#[derive(Debug)]
enum Wire {
    /// Two handles on one socket: a reader and a writer proceed together.
    Split {
        /// Held by the leader while it reads one logical message.
        rx: Mutex<Stream>,
        /// Held while one logical message — every fragment of it — is written.
        tx: Mutex<Stream>,
    },
    /// One handle, because the transport would not split. Sending and
    /// receiving take turns, and the turnstile keeps one call in flight so
    /// they never want the half at the same time.
    Whole(Mutex<Stream>),
}

impl Wire {
    fn rx(&self) -> &Mutex<Stream> {
        match self {
            Wire::Split { rx, .. } => rx,
            Wire::Whole(s) => s,
        }
    }

    fn tx(&self) -> &Mutex<Stream> {
        match self {
            Wire::Split { tx, .. } => tx,
            Wire::Whole(s) => s,
        }
    }
}

/// A reader that remembers how much it handed over.
///
/// The whole point: `read_exact` fails without saying whether it consumed
/// anything, and "the socket timed out with nothing read" (healthy, the peer
/// is just quiet) and "the socket timed out mid-message" (fatal, the framing
/// is now unknowable) are the same `ErrorKind`. Guessing either way is wrong —
/// treat every timeout as fatal and a slow service destroys its own
/// connections; treat none as fatal and a half-read message is decoded as the
/// next one.
struct Counting<'a> {
    inner: &'a mut Stream,
    seen: usize,
}

impl Read for Counting<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.seen += n;
        Ok(n)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Mux
// ─────────────────────────────────────────────────────────────────────────────

/// One connection, shared by any number of callers.
///
/// Cheap to clone — a clone is another handle on the same connection, not
/// another connection — and `Send + Sync`, so the natural way to use it is to
/// hand a clone to each thread that has a call to make. Dropping the last
/// clone closes the socket.
#[derive(Debug, Clone)]
pub struct Mux {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    wire: Wire,
    /// Held for a whole call when this connection will not multiplex, which
    /// makes it behave exactly like [`Connection`]: one request in flight.
    turnstile: Mutex<()>,
    inbox: Mutex<Inbox>,
    arrived: Condvar,

    version: Version,
    endian: Endian,
    endpoint: (String, u16),
    default_key: Vec<u8>,
    max_message_size: usize,
    fragment_threshold: usize,
    multiplexes: bool,

    /// Allocated under the write half, so id order is wire order.
    next_id: AtomicU32,
    /// What §7.10.2 produced for `char` data here. Carried across from the
    /// `Connection` this took over, so a `Mux` declares and refuses exactly
    /// what that connection would have.
    char_codeset: crate::CharCodeset,
    /// Whether some caller has undertaken to convert `char` data itself; see
    /// [`Mux::convert_chars`]. Shared, because the undertaking is about the
    /// connection rather than about one caller: everyone writing to this wire
    /// writes under the one `CodeSets` context it sent.
    caller_converts_chars: AtomicBool,
    /// §7.10.2.5 negotiates once per connection, so exactly one request may
    /// carry the `CodeSets` context. Taken under the write half, which is what
    /// makes "the first request written" and "the request carrying the
    /// context" the same request even when two threads send at once.
    codeset_pending: AtomicBool,

    // Lock-free mirrors, so the pool can ask about a connection from inside
    // its own lock without entering a second section. See the module docs.
    in_flight: AtomicUsize,
    faulted: AtomicBool,
    last_used: AtomicU64,
}

/// The narrow-text codec for a negotiated `char` codeset, or `None` for UTF-8.
///
/// A free function rather than a method on two types: `Connection` and this
/// multiplexer hold the same `CharCodeset` and must answer the same way, and
/// two methods with one rule is where the two stop agreeing. `None` means the
/// agreement *is* UTF-8, not that there was none — a failed negotiation is
/// `CharCodeset::Incompatible` and refuses the call.
pub(crate) fn narrow_codec(
    cs: &crate::CharCodeset,
) -> Option<std::sync::Arc<dyn orbweaver_cdr::TextCodec>> {
    let agreed = cs.agreed()?;
    if agreed.id() == codeset::CodeSetId::UTF_8 {
        return None;
    }
    Some(std::sync::Arc::new(agreed))
}

impl Mux {
    /// Takes over an established connection.
    ///
    /// Everything negotiated at dial time — version, codeset, object key,
    /// message ceiling, fragmentation threshold, byte order — comes across
    /// unchanged, so a `Mux` speaks exactly what the [`Connection`] would
    /// have. Whether it will actually put two requests in flight is decided
    /// here and readable from [`Mux::multiplexes`].
    pub fn over(conn: Connection) -> Mux {
        let Connection {
            stream,
            endpoint,
            object_key,
            version,
            endian,
            next_id,
            max_message_size,
            poisoned,
            char_codeset,
            caller_converts_chars,
            codeset_context_pending,
            fragment_threshold,
            ..
        } = conn;

        // Two conditions, both necessary. The version argument is in the
        // module docs; the transport one is that a TLS session is a single
        // piece of mutable state that a reader and a writer cannot both hold.
        let (wire, multiplexes) = match (version.is_1_2_layout(), stream.try_split()) {
            (true, Some(second)) => {
                (Wire::Split { rx: Mutex::new(stream), tx: Mutex::new(second) }, true)
            }
            (_, _) => (Wire::Whole(Mutex::new(stream)), false),
        };

        let mut inbox = Inbox::default();
        if poisoned {
            inbox.fault = Some(Fault::Desynchronized);
        }

        Mux {
            inner: Arc::new(Inner {
                wire,
                turnstile: Mutex::new(()),
                inbox: Mutex::new(inbox),
                arrived: Condvar::new(),
                version,
                endian,
                endpoint,
                default_key: object_key,
                max_message_size,
                fragment_threshold,
                multiplexes,
                next_id: AtomicU32::new(next_id),
                char_codeset,
                caller_converts_chars: AtomicBool::new(caller_converts_chars),
                codeset_pending: AtomicBool::new(codeset_context_pending),
                in_flight: AtomicUsize::new(0),
                faulted: AtomicBool::new(poisoned),
                last_used: AtomicU64::new(now_ms()),
            }),
        }
    }

    /// Dials `ior` and multiplexes over the result.
    pub fn connect(ior: &Ior, timeout: Duration) -> Result<Mux> {
        Ok(Mux::over(Connection::connect(ior, timeout)?))
    }

    /// Whether more than one request may be in flight here.
    ///
    /// False on GIOP 1.0 and 1.1 (see the module docs for the specification
    /// argument) and false on a transport that cannot be split, which today
    /// means TLS. A `Mux` that answers false still works; it serializes.
    pub fn multiplexes(&self) -> bool {
        self.inner.multiplexes
    }

    /// The GIOP version this connection speaks.
    pub fn version(&self) -> Version {
        self.inner.version
    }

    /// Whether two handles are the same connection.
    ///
    /// Identity, not equality: [`crate::pool`] has to find *this* connection
    /// among the ones it holds in order to discard it, and two connections to
    /// one endpoint are interchangeable in every respect except which socket
    /// they are.
    pub fn same_connection(a: &Mux, b: &Mux) -> bool {
        Arc::ptr_eq(&a.inner, &b.inner)
    }

    /// The host and port this connection reached.
    pub fn endpoint(&self) -> (&str, u16) {
        (&self.inner.endpoint.0, self.inner.endpoint.1)
    }

    /// The object key the connection was dialed for, used by [`Mux::call`]
    /// when a caller names no other.
    pub fn object_key(&self) -> &[u8] {
        &self.inner.default_key
    }

    /// The converter for `char` data on this connection (§7.10.2.5's
    /// ISO-8859-1 when nothing was negotiated).
    ///
    /// **Reading it does not make it apply**; see [`Mux::convert_chars`].
    pub fn char_converter(&self) -> codeset::Converter {
        self.inner.char_codeset.agreed().unwrap_or_else(|| {
            codeset::Converter::new(codeset::CodeSetId::ISO_8859_1)
                .expect("ISO-8859-1 is always supported")
        })
    }

    /// Takes responsibility for converting `char` data on this connection, and
    /// returns the converter to do it with.
    ///
    /// [`Connection::convert_chars`] with one difference that follows from a
    /// `Mux` being shared: the undertaking binds every caller on this wire, not
    /// only the one that asked. It has to — there is one connection, one
    /// `CodeSets` context, and one meaning for the octets under it.
    pub fn convert_chars(&self) -> Result<codeset::Converter> {
        let c = self.inner.char_codeset.usable()?;
        self.inner.caller_converts_chars.store(true, Ordering::SeqCst);
        Ok(c)
    }

    /// Whether this connection can still carry calls.
    ///
    /// Lock-free on purpose: [`crate::pool`] asks from inside its own lock.
    pub fn is_usable(&self) -> bool {
        !self.inner.faulted.load(Ordering::SeqCst)
    }

    /// How many requests are on the wire unanswered right now. Lock-free, for
    /// the same reason.
    pub fn in_flight(&self) -> usize {
        self.inner.in_flight.load(Ordering::SeqCst)
    }

    /// How long since a call last started or finished here. Lock-free.
    pub fn idle_for(&self) -> Duration {
        Duration::from_millis(now_ms().saturating_sub(self.inner.last_used.load(Ordering::SeqCst)))
    }

    /// The counters. Takes the inbox, so never call it from inside another
    /// lock section.
    pub fn stats(&self) -> MuxStats {
        self.inner.lock().stats
    }

    /// Invokes `operation` on this connection's own object key.
    pub fn call<F>(&self, operation: &str, write_args: F, timeout: Duration) -> Answered
    where
        F: Fn(&mut Encoder),
    {
        let key = self.inner.default_key.clone();
        self.call_on(&key, operation, write_args, timeout)
    }

    /// Invokes `operation` on an arbitrary object key.
    ///
    /// The key is a parameter and not a property of the connection because
    /// that is what pooling requires: one connection to an endpoint serves
    /// every object behind it, and GIOP has always addressed per message
    /// rather than per connection.
    pub fn call_on<F>(
        &self,
        object_key: &[u8],
        operation: &str,
        write_args: F,
        timeout: Duration,
    ) -> Answered
    where
        F: Fn(&mut Encoder),
    {
        assert_nothing_held("a multiplexed invocation");
        let deadline = Instant::now() + timeout;
        // On a connection that will not multiplex the whole call is exclusive,
        // which reproduces `Connection`'s behaviour exactly. Held across the
        // wait, so the second caller starts its request only once the first is
        // answered.
        let _turn = (!self.inner.multiplexes)
            .then(|| self.inner.turnstile.lock().unwrap_or_else(|e| e.into_inner()));
        let id = self.inner.send(object_key, operation, &write_args, true)?;
        self.inner.wait_for(id, deadline)
    }

    /// Sends a request and returns without waiting, so another may follow it
    /// onto the same connection.
    ///
    /// Refuses with [`Error::MultiplexingUnsupported`] where this
    /// implementation will not put two requests in flight — GIOP below 1.2, or
    /// a transport that cannot be split. Refusing is the point: the alternative
    /// is a `Pending` that silently could not overlap with anything.
    pub fn send<F>(&self, object_key: &[u8], operation: &str, write_args: F) -> Result<Pending>
    where
        F: Fn(&mut Encoder),
    {
        assert_nothing_held("a multiplexed send");
        if !self.inner.multiplexes {
            return Err(Error::MultiplexingUnsupported { version: self.inner.version });
        }
        let id = self.inner.send(object_key, operation, &write_args, true).map_err(|f| f.error)?;
        Ok(Pending { inner: Arc::clone(&self.inner), id, taken: false })
    }

    /// Sends a `oneway`: bytes written, nothing more promised.
    ///
    /// Returns `()` rather than a manufactured [`Sent`], because §9.4.3.2 has
    /// no reply for a result — or a `LOCATION_FORWARD` — to travel in, and a
    /// fabricated empty reply would let a caller decode a body that does not
    /// exist.
    pub fn call_oneway<F>(
        &self,
        object_key: &[u8],
        operation: &str,
        write_args: F,
    ) -> std::result::Result<(), Failed>
    where
        F: Fn(&mut Encoder),
    {
        assert_nothing_held("a multiplexed oneway invocation");
        let _turn = (!self.inner.multiplexes)
            .then(|| self.inner.turnstile.lock().unwrap_or_else(|e| e.into_inner()));
        self.inner.send(object_key, operation, &write_args, false)?;
        Ok(())
    }

    /// Sends a §9.4.4 `CancelRequest` for `request_id`.
    ///
    /// Not sent automatically on a timeout — see the module docs — because a
    /// peer that answers it by closing the connection would fail every other
    /// caller sharing this one. Exposed for a caller that knows its peer.
    pub fn cancel(&self, request_id: u32) -> Result<()> {
        let mut tx = self.inner.wire.tx().lock().unwrap_or_else(|e| e.into_inner());
        let msg = encode_cancel_request(self.inner.version, self.inner.endian, request_id)?;
        tx.write_all(&msg).map_err(|e| self.inner.fault_io(&e))?;
        tx.flush().map_err(|e| self.inner.fault_io(&e))?;
        Ok(())
    }
}

/// A request on the wire whose reply has not been collected.
///
/// Dropping one without waiting abandons it: the slot is released and the
/// reply, if it arrives, is dropped and counted in [`MuxStats::orphaned`].
/// That is the honest behaviour — there is no way to un-send a request — and
/// it is why a dropped `Pending` costs nothing but a wasted reply.
#[derive(Debug)]
pub struct Pending {
    inner: Arc<Inner>,
    id: u32,
    taken: bool,
}

impl Pending {
    /// The `request_id` this call went out with.
    pub fn request_id(&self) -> u32 {
        self.id
    }

    /// Waits for this call's reply, for at most `timeout`.
    pub fn wait(mut self, timeout: Duration) -> Answered {
        assert_nothing_held("waiting for a multiplexed reply");
        self.taken = true;
        self.inner.wait_for(self.id, Instant::now() + timeout)
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.taken {
            self.inner.abandon(self.id);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Inner: sending
// ─────────────────────────────────────────────────────────────────────────────

impl Inner {
    fn lock(&self) -> Held<'_> {
        let section = Section::enter(MUX_LOCK);
        Held { inbox: self.inbox.lock().unwrap_or_else(|e| e.into_inner()), section }
    }

    /// Waits on the arrival condvar, carrying the section through the wait for
    /// the reason `event_server` documents: the mutex is released but the
    /// thread is still inside a section it means to resume.
    fn wait<'a>(&'a self, held: Held<'a>, left: Duration) -> Held<'a> {
        let Held { inbox, section } = held;
        let (inbox, _) = self.arrived.wait_timeout(inbox, left).unwrap_or_else(|e| e.into_inner());
        Held { inbox, section }
    }

    fn touch(&self) {
        self.last_used.store(now_ms(), Ordering::SeqCst);
    }

    /// Records a fault for everybody, from outside the inbox lock.
    fn fault(&self, f: Fault) {
        let mut held = self.lock();
        Inner::fault_locked(&mut held, &self.faulted, f);
        self.arrived.notify_all();
    }

    /// The first fault wins: it is the one that explains the others.
    fn fault_locked(held: &mut Held<'_>, flag: &AtomicBool, f: Fault) {
        flag.store(true, Ordering::SeqCst);
        if held.fault.is_none() {
            held.fault = Some(f);
        }
    }

    fn fault_io(&self, e: &std::io::Error) -> Error {
        self.fault(Fault::Io(e.kind(), e.to_string()));
        Error::Io(std::io::Error::new(e.kind(), e.to_string()))
    }

    /// Writes one request. Returns its id, or why it did not go out.
    ///
    /// The order here is load-bearing three times over: the id is allocated
    /// under the write half so id order is wire order; the slot is registered
    /// *before* the bytes leave, because a peer fast enough to answer during
    /// our own `write_all` would otherwise find no slot and have its reply
    /// dropped as an orphan; and every fragment goes out under one hold, so
    /// nothing can be interleaved into a message the peer is reassembling.
    fn send<F>(
        &self,
        object_key: &[u8],
        operation: &str,
        write_args: &F,
        expect_reply: bool,
    ) -> std::result::Result<u32, Failed>
    where
        F: Fn(&mut Encoder),
    {
        // The `CodeSets` context and the octets under it must describe the same
        // bytes. Checked before the write half is even taken, and marked unsent
        // because nothing has been written: this is a refusal to speak, not a
        // failure while speaking.
        if let Err(e) =
            self.char_codeset.may_send(self.caller_converts_chars.load(Ordering::SeqCst))
        {
            return Err(failed(e, true));
        }

        let mut tx = self.wire.tx().lock().unwrap_or_else(|e| e.into_inner());

        let id = {
            let mut held = self.lock();
            if let Some(f) = held.fault.clone() {
                // Nothing was written, so this one is re-sendable whatever
                // killed the connection. It has no id yet either, and 0 is
                // never allocated as one — so an interruption describes itself
                // to this caller as the close or the report it was, rather than
                // as somebody else's half-received reply.
                return Err(failed(f.to_error(0), true));
            }
            // §13.5.1: ids must be unambiguous within the lifetime of the
            // connection and unique across Request and LocateRequest alike.
            // One counter, never re-used, so a late reply can never be
            // mistaken for a live call's.
            let id = self.next_id.fetch_add(1, Ordering::SeqCst).max(1);
            if expect_reply {
                held.slots.insert(id, Slot::Waiting);
                held.in_flight += 1;
                held.stats.sent += 1;
                held.stats.peak_in_flight = held.stats.peak_in_flight.max(held.in_flight);
                self.in_flight.store(held.in_flight, Ordering::SeqCst);
            }
            id
        };
        self.touch();

        // Taken here rather than at connect time so that "the first request
        // written" and "the request carrying the context" are the same one.
        // Put back if this request never reaches the wire: a connection that
        // silently dropped its only chance to announce a codeset would send
        // every later string under an agreement the peer never heard.
        let took_context = self.codeset_pending.swap(false, Ordering::SeqCst);
        let undo = |slf: &Inner| {
            slf.release(id);
            if took_context {
                slf.codeset_pending.store(true, Ordering::SeqCst);
            }
        };

        let contexts = match (took_context, self.char_codeset.agreed()) {
            (true, Some(c)) => {
                let ctx = codeset::CodeSetContext {
                    char_data: c.id(),
                    wchar_data: codeset::CodeSetId::UTF_16,
                };
                match ctx.encode(self.endian) {
                    Ok(data) => vec![ServiceContext { id: codeset::SERVICE_ID_CODE_SETS, data }],
                    Err(e) => {
                        undo(self);
                        return Err(failed(e, true));
                    }
                }
            }
            _ => Vec::new(),
        };

        let msg = match encode_request_with_contexts(
            self.version,
            self.endian,
            id,
            object_key,
            operation,
            expect_reply,
            &contexts,
            // The agreement this connection reached, carried by the
            // stream rather than remembered by the caller. `None` when the
            // negotiation produced UTF-8 or produced nothing, which keeps
            // every existing call byte-identical.
            narrow_codec(&self.char_codeset),
            write_args,
        ) {
            Ok(m) => m,
            Err(e) => {
                undo(self);
                return Err(failed(e, true));
            }
        };

        let pieces = match fragment_message(msg, self.fragment_threshold) {
            Ok(p) => p,
            Err(e) => {
                undo(self);
                return Err(failed(e, true));
            }
        };
        for piece in pieces {
            if let Err(e) = tx.write_all(&piece) {
                // A half-written message leaves the outbound half unframeable,
                // so the connection dies here — but this request provably did
                // not run: the peer is waiting for bytes that will never come.
                self.release(id);
                return Err(failed(self.fault_io(&e), true));
            }
        }
        if let Err(e) = tx.flush() {
            self.release(id);
            return Err(failed(self.fault_io(&e), true));
        }
        Ok(id)
    }

    /// Drops a slot that never made it onto the wire.
    fn release(&self, id: u32) {
        let mut held = self.lock();
        if held.slots.remove(&id).is_some() {
            held.in_flight = held.in_flight.saturating_sub(1);
            held.stats.sent = held.stats.sent.saturating_sub(1);
            self.in_flight.store(held.in_flight, Ordering::SeqCst);
        }
    }

    /// The caller gave up. The request is still on the wire, so the slot stays
    /// — as a tombstone — until the reply lands or the connection ends: an id
    /// we can still recognise is an id that cannot be confused with a live
    /// call's.
    fn abandon(&self, id: u32) {
        let mut held = self.lock();
        Inner::abandon_locked(&mut held, id);
        self.in_flight.store(held.in_flight, Ordering::SeqCst);
    }

    fn abandon_locked(held: &mut Held<'_>, id: u32) {
        match held.slots.get(&id) {
            // Answered after all, and nobody is coming for it.
            Some(Slot::Ready(_)) => {
                held.slots.remove(&id);
                held.stats.orphaned += 1;
                held.in_flight = held.in_flight.saturating_sub(1);
            }
            Some(Slot::Waiting) => {
                held.slots.insert(id, Slot::Abandoned);
                held.in_flight = held.in_flight.saturating_sub(1);
                Inner::sweep_abandoned(held);
            }
            // Already abandoned, or already collected: in_flight was
            // decremented then and must not be decremented twice.
            Some(Slot::Abandoned) | None => {}
        }
    }

    /// Drops the tombstones once there are more of them than any peer could
    /// still be about to answer.
    fn sweep_abandoned(held: &mut Held<'_>) {
        // The cheap test first: the map can only be over the bound if it is
        // over the bound in total, and this runs on every giving-up caller.
        if held.slots.len() <= MAX_ABANDONED {
            return;
        }
        held.slots.retain(|_, s| !matches!(s, Slot::Abandoned));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Inner: receiving
// ─────────────────────────────────────────────────────────────────────────────

impl Inner {
    /// Waits for `id`, reading the socket whenever nobody else is.
    fn wait_for(&self, id: u32, deadline: Instant) -> Answered {
        let started = Instant::now();
        loop {
            let mut held = self.lock();

            // 1. Is it here, or is the connection gone?
            match Inner::collect(&mut held, id) {
                Collected::Answer(reply) => {
                    self.in_flight.store(held.in_flight, Ordering::SeqCst);
                    drop(held);
                    self.touch();
                    return interpret(*reply);
                }
                Collected::Gone(f) => {
                    self.in_flight.store(held.in_flight, Ordering::SeqCst);
                    drop(held);
                    return Err(f);
                }
                Collected::Waiting => {}
            }

            // 2. Read, if nobody else is. `try_lock` and never `lock`: this is
            //    the inbox → read-half direction, and letting it block is
            //    exactly the cycle the module docs rule out.
            match self.wire.rx().try_lock() {
                Ok(mut rx) => {
                    drop(held); // never read with the inbox held
                    // The reader is a caller, so its own deadline bounds the
                    // read. Without this the socket timeout does, and a caller
                    // that asked for 300ms would wait out the dial timeout
                    // instead — measured, not imagined: the test named for it
                    // failed exactly that way before this line existed.
                    let budget = deadline.saturating_duration_since(Instant::now());
                    let _ = rx.set_read_timeout(budget);
                    let mut counting = Counting { inner: &mut rx, seen: 0 };
                    let outcome = read_message(&mut counting, self.max_message_size);
                    let seen = counting.seen;
                    // File it *before* letting the read half go, so the caller
                    // it belongs to cannot become the next leader and block on
                    // a message that has already arrived.
                    let mut filing = self.lock();
                    self.file(&mut filing, outcome, seen);
                    self.arrived.notify_all();
                    drop(filing);
                    drop(rx);
                }
                Err(TryLockError::Poisoned(g)) => {
                    // A reader panicked mid-message. The stream is whatever it
                    // left behind, which is precisely what desynchronized
                    // means.
                    drop(g);
                    drop(held);
                    self.fault(Fault::Desynchronized);
                }
                Err(TryLockError::WouldBlock) => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if !left.is_zero() {
                        // Bounded, so leadership can never be stranded: see
                        // `LEADER_POLL`.
                        let held = self.wait(held, left.min(LEADER_POLL));
                        drop(held);
                    } else {
                        drop(held);
                    }
                }
            }

            if Instant::now() >= deadline {
                // One last look before giving up: the reply may have landed
                // while this thread was reading.
                let mut held = self.lock();
                if let Collected::Answer(reply) = Inner::collect(&mut held, id) {
                    self.in_flight.store(held.in_flight, Ordering::SeqCst);
                    drop(held);
                    self.touch();
                    return interpret(*reply);
                }
                Inner::abandon_locked(&mut held, id);
                self.in_flight.store(held.in_flight, Ordering::SeqCst);
                drop(held);
                // One caller's patience is not the connection's health: no
                // fault is recorded and everybody else keeps calling.
                return Err(failed(
                    Error::Timeout { request_id: id, waited: started.elapsed() },
                    false,
                ));
            }
        }
    }

    /// Takes `id`'s answer if it is there, or says why there will not be one.
    fn collect(held: &mut Held<'_>, id: u32) -> Collected {
        if let Some(Slot::Ready(_)) = held.slots.get(&id) {
            let Some(Slot::Ready(reply)) = held.slots.remove(&id) else {
                unreachable!("just checked")
            };
            held.in_flight = held.in_flight.saturating_sub(1);
            held.stats.answered += 1;
            return Collected::Answer(reply);
        }
        if let Some(f) = held.fault.clone() {
            if held.slots.remove(&id).is_some() {
                held.in_flight = held.in_flight.saturating_sub(1);
            }
            return Collected::Gone(failed(f.to_error(id), f.unsent(id)));
        }
        if !held.slots.contains_key(&id) {
            // No slot and no fault: already collected, or never registered.
            // Waiting forever for it would be the one unrecoverable answer.
            return Collected::Gone(failed(Error::Desynchronized, false));
        }
        Collected::Waiting
    }

    /// Files one read outcome under its request id, or faults the connection.
    fn file(&self, held: &mut Held<'_>, outcome: Result<RawMessage>, bytes_read: usize) {
        let msg = match outcome {
            Ok(m) => m,
            Err(Error::Io(e)) => {
                if bytes_read == 0
                    && matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    )
                {
                    // Quiet wire, whole framing. Yield and let somebody read
                    // again. This is the case a naive implementation reports
                    // as a dead connection.
                    return;
                }
                Inner::fault_locked(held, &self.faulted, Fault::Io(e.kind(), e.to_string()));
                return;
            }
            Err(Error::InterruptedMidReassembly { control, partial, request_id, received }) => {
                // The peer interrupted a reply with an orderly control message.
                // Reduced to `Desynchronized` — which is what every non-I/O
                // reader error used to become — this told N callers that the
                // connection was corrupt and none of them that it had merely
                // been closed, so `crate::pool` refused to retry a call that
                // §13.5.1 says nobody processed. The fault keeps the shape so
                // each waiter can be told its own truth.
                Inner::fault_locked(
                    held,
                    &self.faulted,
                    Fault::Interrupted { control, partial, request_id, received },
                );
                return;
            }
            Err(e) => {
                // Every waiter is about to be told `Desynchronized`, which is
                // true and useless: the *reason* the framing failed is in `e`
                // and is thrown away by the reduction to a shared fault. It is
                // worth a line, because the reasons are specific and
                // actionable — a peer fragmenting below GIOP 1.2 reads as
                // "desynchronized" to a caller and as `FragmentUnsupported`
                // here, and only one of those tells anybody what to change.
                eprintln!("orbweaver: connection faulted while framing a reply: {e}");
                Inner::fault_locked(held, &self.faulted, Fault::Desynchronized);
                return;
            }
        };

        match msg.msg_type {
            MsgType::Reply => {}
            MsgType::CloseConnection => {
                // §13.5.1: everything outstanding was not processed and may be
                // re-sent on a new connection — which is what makes this the
                // one fault a pooled retry is allowed to hide.
                Inner::fault_locked(held, &self.faulted, Fault::Closed);
                return;
            }
            other => {
                Inner::fault_locked(held, &self.faulted, Fault::Unexpected(other));
                return;
            }
        }

        let fragments = msg.fragments;
        let reply = match decode_reply(msg) {
            Ok(r) => r,
            // A reply we cannot decode cannot be attributed to a caller, so it
            // cannot be failed to one either. Everybody hears about it.
            Err(_) => {
                Inner::fault_locked(held, &self.faulted, Fault::Desynchronized);
                return;
            }
        };

        held.stats.max_reply_fragments = held.stats.max_reply_fragments.max(fragments);
        let id = reply.request_id;
        match held.slots.get(&id) {
            Some(Slot::Waiting) => {
                // Ids are allocated under the write half, so any smaller id
                // still outstanding provably went out earlier: this reply
                // overtook it on the peer's side. Counted here, inside the
                // lock that owns the outstanding set, so it cannot be an
                // artefact of when two threads happened to look.
                if held.slots.keys().any(|&other| other < id) {
                    held.stats.out_of_order += 1;
                }
                held.slots.insert(id, Slot::Ready(Box::new(reply)));
            }
            Some(Slot::Abandoned) => {
                held.slots.remove(&id);
                held.stats.orphaned += 1;
            }
            Some(Slot::Ready(_)) => {
                // Two replies to one id. The peer is not making sense, and the
                // second would overwrite an answer somebody is about to
                // collect.
                Inner::fault_locked(held, &self.faulted, Fault::Desynchronized);
            }
            // A reply for an id we never sent, or one already collected. Not
            // fatal — a late reply to a call that timed out and was swept
            // looks exactly like this — but counted, because a stream of them
            // means the peer and this end disagree about something.
            None => {
                held.stats.orphaned += 1;
            }
        }
    }
}

/// What a look in the inbox found.
enum Collected {
    /// The reply, removed from the inbox.
    Answer(Box<Reply>),
    /// There will not be one.
    Gone(Failed),
    /// Still outstanding.
    Waiting,
}

/// Turns a decoded reply into the outcome its status calls for.
///
/// Deliberately a free function and not
/// [`Connection`]'s method: that one *follows* a
/// `LOCATION_FORWARD` by reconnecting itself, which a shared connection must
/// never do — a forward may point at another host, and the other callers on
/// this connection did not ask to be moved.
fn interpret(reply: Reply) -> Answered {
    match reply.status {
        ReplyStatus::NoException => Ok(Sent::Reply(Box::new(reply))),
        ReplyStatus::SystemException => {
            let mut b = match reply.body() {
                Ok(b) => b,
                Err(e) => return Err(failed(e, false)),
            };
            Err(failed(
                Error::SystemException {
                    id: b.get_string().unwrap_or_else(|_| "<unreadable>".into()),
                    minor: b.get_u32().unwrap_or(0),
                    completed: b.get_u32().unwrap_or(0),
                },
                false,
            ))
        }
        ReplyStatus::UserException => {
            let id = match reply.body() {
                Ok(mut b) => b.get_string().unwrap_or_else(|_| "<unreadable>".into()),
                Err(e) => return Err(failed(e, false)),
            };
            Err(failed(Error::UserException { id, reply: Box::new(reply) }, false))
        }
        ReplyStatus::LocationForward | ReplyStatus::LocationForwardPerm => {
            let mut b = match reply.body() {
                Ok(b) => b,
                Err(e) => return Err(failed(e, false)),
            };
            match Ior::read_from(&mut b) {
                Ok(ior) => Ok(Sent::Forward(Box::new(ior))),
                Err(e) => Err(failed(e, false)),
            }
        }
        ReplyStatus::NeedsAddressingMode => {
            Err(failed(Error::UnexpectedMessage(MsgType::Reply), false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fault is told to every waiter the same way, and only
    /// `CloseConnection` says the request can be re-sent — §13.5.1 is the only
    /// place the specification promises that, so it is the only place this
    /// promises it.
    #[test]
    fn only_close_connection_reports_a_request_as_unsent() {
        assert!(Fault::Closed.unsent(1));
        assert!(!Fault::Desynchronized.unsent(1));
        assert!(!Fault::Io(std::io::ErrorKind::BrokenPipe, "gone".into()).unsent(1));
        assert!(!Fault::Unexpected(MsgType::Request).unsent(1));
        assert!(matches!(Fault::Closed.to_error(1), Error::ConnectionClosed));
        assert!(matches!(Fault::Desynchronized.to_error(1), Error::Desynchronized));
    }

    /// The one fault that is not the same for everybody. A close that cut a
    /// reply in half is retryable for every caller *except* the one whose reply
    /// it cut: that peer had begun answering, so §13.5.1's "was not processed"
    /// is false for exactly that request and true for the rest.
    #[test]
    fn an_interruption_answers_each_caller_about_its_own_request() {
        let f = Fault::Interrupted {
            control: MsgType::CloseConnection,
            partial: MsgType::Reply,
            request_id: 7,
            received: 2,
        };
        assert!(!f.unsent(7), "the half-answered call was processed; re-sending would repeat it");
        assert!(f.unsent(8), "§13.5.1 still covers a caller that got nothing back");
        assert!(matches!(
            f.to_error(7),
            Error::InterruptedMidReassembly { control: MsgType::CloseConnection, received: 2, .. }
        ));
        assert!(matches!(f.to_error(8), Error::ConnectionClosed));
        assert!(f.to_error(7).is_orderly_close() && f.to_error(8).is_orderly_close());

        // A `MessageError` names nothing, so it makes nobody's request unsent.
        let m = Fault::Interrupted {
            control: MsgType::MessageError,
            partial: MsgType::Reply,
            request_id: 7,
            received: 1,
        };
        assert!(!m.unsent(7) && !m.unsent(8));
        assert!(!m.to_error(7).is_orderly_close(), "a report is not a goodbye");
        assert!(matches!(m.to_error(8), Error::UnexpectedMessage(MsgType::MessageError)));
    }

    /// The distinction the whole timeout policy rests on: a read that consumed
    /// nothing has not damaged the framing, and one that consumed something
    /// has. A socket with a read timeout and a silent peer is the first case,
    /// and treating it as the second would destroy healthy connections.
    #[test]
    fn a_quiet_socket_consumes_nothing() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let client = std::net::TcpStream::connect(addr).expect("connect");
        let _server = listener.accept().expect("accept").0; // held, and silent
        client.set_read_timeout(Some(Duration::from_millis(50))).expect("timeout");

        let mut stream = Stream::Plain(client);
        let mut counting = Counting { inner: &mut stream, seen: 0 };
        let err = read_message(&mut counting, crate::DEFAULT_MAX_MESSAGE_SIZE)
            .expect_err("a silent peer cannot produce a message");
        assert!(matches!(err, Error::Io(_)), "expected a socket timeout, got {err}");
        assert_eq!(counting.seen, 0, "nothing was sent, so nothing may be counted as consumed");
    }

    /// The same reader, told half a header: now the framing really is gone and
    /// the count says so.
    #[test]
    fn a_truncated_message_consumes_something() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let client = std::net::TcpStream::connect(addr).expect("connect");
        let mut server = listener.accept().expect("accept").0;
        server.write_all(b"GIOP\x01\x02").expect("half a header");
        server.flush().expect("flush");
        client.set_read_timeout(Some(Duration::from_millis(200))).expect("timeout");

        let mut stream = Stream::Plain(client);
        let mut counting = Counting { inner: &mut stream, seen: 0 };
        let _ = read_message(&mut counting, crate::DEFAULT_MAX_MESSAGE_SIZE);
        assert_eq!(counting.seen, 6, "six bytes went into a message that never completed");
    }
}
