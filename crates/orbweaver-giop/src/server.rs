//! The serving half: decoding requests, encoding replies, and answering the
//! messages a peer sends before it will talk to us at all.
//!
//! `docs/PLAN.md` §7 commits to GIOP 1.0/1.1 compatibility *in both
//! directions*. The client half shipped first, which left the asymmetry that
//! we could call existing systems but they could not call us.
//!
//! # What a peer does before its first call
//!
//! Captured from omniORB 4.3.4: it sends a `LocateRequest` and **waits for the
//! `LocateReply`** before sending any `Request`. A server that treats
//! `LocateRequest` as an unexpected message therefore never receives a single
//! invocation — it hangs at the handshake rather than failing somewhere
//! informative.
//!
//! # Two layout traps
//!
//! `LocateReply` bodies are marshalled **immediately** after the header, with
//! no 8-byte alignment — the opposite of `Reply` in GIOP 1.2 (§9.4.6 against
//! §9.4.3.1). And 1.0/1.1 put `service_context` *first* in both request and
//! reply headers, so a version-blind encoder produces a message the peer
//! misparses rather than rejects.
//!
//! # Serving more than one client at a time
//!
//! [`Server`] used to accept one connection, serve it to completion, and only
//! then accept the next. Every service built on it — the naming server, the
//! event channel, the IFR facade, the MoE expert service — carried a sentence
//! telling its harness that the foreign client had to be the *only* client
//! while it ran, and a deployment where two agents hold sessions at once was
//! simply impossible. [`Server::serve`] now spawns a thread per accepted
//! connection, inside a [`std::thread::scope`] so nothing outlives the call.
//!
//! ## How the servant is shared, and what that does not buy
//!
//! **One servant, behind one mutex.** [`Dispatch`] is `&mut self`-shaped, and
//! the servants in this workspace hold the *authoritative* state of the
//! service they front — the naming tree, the channel's proxy tables, the
//! repository. Giving each connection its own copy would fork that state, so
//! the servant stays single and every connection thread takes the same lock
//! for the duration of one message. The alternative — requiring
//! `Dispatch: Sync` with interior mutability per servant — buys more, and
//! costs a rewrite of every servant in three crates that this footprint may
//! not touch; it stays available as a later batch, because nothing here
//! depends on the lock being where it is.
//!
//! What the mutex buys is exactly one thing: **connections are concurrent**.
//! Ten clients may be connected, mid-session, holding their own sockets and
//! their own GIOP state, and each is answered.
//!
//! What it does **not** buy, said plainly because the difference is easy to
//! oversell: **dispatch is still serialized**. A servant that blocks for a
//! second inside `dispatch` blocks every other client for that second. That
//! is a different limit from the one being removed — "only one client may be
//! connected" is gone; "only one operation runs at a time" is not — and a
//! slow servant is still a slow service. The lock is taken per message, not
//! per connection, so an *idle* connection costs nobody anything.
//!
//! One re-entrancy is forbidden by that choice: a servant that, from inside
//! `dispatch`, calls back into **its own** server waits for a lock its own
//! caller holds. It does not wedge the server — the inner call is bounded by
//! the client read timeout [`crate::Connection`] sets, fails, and serving
//! continues — but it cannot succeed. Calling *another* server in the same
//! process, which is what the event channel's delivery loop does, is fine and
//! is proved by test; see `event_server`'s rule that no lock may be held
//! across an outbound call, which this module's lock obeys by being released
//! before the reply is even written.
//!
//! ## The cap
//!
//! Thread-per-connection with no bound is a one-line resource exhaustion, so
//! at most [`Server::max_connections`] connections are served at once
//! ([`DEFAULT_MAX_CONNECTIONS`] by default). Over the cap a connection is
//! **accepted, told `CloseConnection`, and closed** — refused, not queued.
//! §9.4.7 makes that goodbye mean "your requests were not processed, re-send
//! them elsewhere", which is the true statement and one a client can act on;
//! queueing would leave it blocked with no way to know why, and never
//! accepting at all would leave it stuck in the listen backlog looking
//! identical to a hung server. Every refusal is counted in
//! [`ServerStats::refused`] and logged with the cap that caused it — the
//! harness rule about unmeasured things applies to dropped clients too.
//!
//! ## Shutdown
//!
//! `stop` is polled by the accept loop and by every connection thread, so a
//! raised flag ends the whole server within [`STOP_POLL`] rather than when
//! the next message happens to arrive — the honest limit the old loop
//! documented. Threads waiting for a peer's next message wake to check it;
//! each ends with an orderly `CloseConnection`. `serve` returns only after
//! every connection thread has finished, so a returned `serve` means no
//! thread is left behind. A peer that starts a message and then stalls is
//! bounded too, by [`Server::set_message_timeout`].

use orbweaver_cdr::{Decoder, Encoder, Endian};
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{
    DEFAULT_MAX_MESSAGE_SIZE, Error, HEADER_LEN, MAGIC, MsgType, RawMessage, ReplyStatus, Result,
    Version, fragment_message, read_message,
};

/// Repository ID of the exception raised for an operation we do not implement.
pub const BAD_OPERATION: &str = "IDL:omg.org/CORBA/BAD_OPERATION:1.0";
/// Repository ID for a malformed or undecodable request body.
pub const MARSHAL: &str = "IDL:omg.org/CORBA/MARSHAL:1.0";
/// Repository ID for an object key we do not recognise.
pub const OBJECT_NOT_EXIST: &str = "IDL:omg.org/CORBA/OBJECT_NOT_EXIST:1.0";
/// Repository ID for a failure with no more precise description — including a
/// user exception reaching a caller that cannot carry one.
pub const UNKNOWN: &str = "IDL:omg.org/CORBA/UNKNOWN:1.0";

/// Whether an operation had run when it failed (§9.4.3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Completion {
    /// The operation did not run.
    No = 0,
    /// The operation ran to completion but the reply was lost.
    Yes = 1,
    /// Cannot be determined.
    Maybe = 2,
}

/// A CORBA system exception to return in place of a result.
#[derive(Debug, Clone)]
pub struct SystemException {
    /// Repository ID, e.g. [`BAD_OPERATION`].
    pub id: String,
    /// Vendor minor code.
    pub minor: u32,
    /// Whether the operation ran.
    pub completed: Completion,
}

impl SystemException {
    /// A `BAD_OPERATION` for an operation name we do not serve.
    pub fn bad_operation() -> Self {
        Self { id: BAD_OPERATION.into(), minor: 0, completed: Completion::No }
    }

    /// A `MARSHAL` for a body we could not decode.
    pub fn marshal() -> Self {
        Self { id: MARSHAL.into(), minor: 0, completed: Completion::No }
    }

    /// An `OBJECT_NOT_EXIST` for an unrecognised object key.
    pub fn object_not_exist() -> Self {
        Self { id: OBJECT_NOT_EXIST.into(), minor: 0, completed: Completion::No }
    }

    /// The standard mapping for a user exception that reached a caller unable
    /// to carry one: `UNKNOWN` with the OMG minor for "unlisted user
    /// exception" (OMGVMCID | 1). The operation did run — it raised — so
    /// completion is `Yes`.
    pub fn unknown_user_exception() -> Self {
        Self { id: UNKNOWN.into(), minor: 0x4f4d_0001, completed: Completion::Yes }
    }
}

/// A decoded GIOP `Request`, with the argument body left as raw CDR.
#[derive(Debug, Clone)]
pub struct Request {
    /// Version the peer used, which the reply must match.
    pub version: Version,
    /// Byte order the peer used.
    pub endian: Endian,
    /// Correlates the reply.
    pub request_id: u32,
    /// Object key the peer addressed.
    pub object_key: Vec<u8>,
    /// Operation name.
    pub operation: String,
    /// Whether the peer expects a reply. False for `oneway`.
    pub expect_reply: bool,
    raw: Vec<u8>,
    body_at: usize,
}

impl Request {
    /// A decoder positioned at the first argument.
    pub fn body(&self) -> Result<Decoder<'_>> {
        let mut d = Decoder::new(&self.raw, self.endian);
        d.seek_to(self.body_at)?;
        Ok(d)
    }
}

/// Decodes a `Request` that [`read_message`] already framed.
///
/// 1.0 and 1.1 marshal `service_context` first, use a `boolean
/// response_expected`, carry the object key as a bare sequence and end with
/// `requesting_principal`. 1.2 marshals `service_context` last and addresses
/// through a `TargetAddress` union.
pub fn decode_request(msg: RawMessage) -> Result<Request> {
    let RawMessage { version, endian, bytes: raw, .. } = msg;
    let mut d = Decoder::new(&raw, endian);
    d.seek_to(HEADER_LEN)?;

    let request_id;
    let expect_reply;
    let object_key;
    let operation;

    if version.is_1_2_layout() {
        request_id = d.get_u32()?;
        let flags = d.get_u8()?;
        d.get_bytes(3)?; // reserved
        expect_reply = flags & 0x3 != 0;
        let disposition = d.get_u16()?;
        if disposition != 0 {
            // ProfileAddr and ReferenceAddr require answering with
            // NEEDS_ADDRESSING_MODE, which we cannot yet produce. Refusing is
            // correct; guessing at the address would not be.
            return Err(Error::UnexpectedMessage(MsgType::Request));
        }
        object_key = d.get_octet_seq()?.to_vec();
        operation = String::from_utf8_lossy(d.get_string_bytes()?).into_owned();
        skip_service_contexts(&mut d)?;
    } else {
        skip_service_contexts(&mut d)?;
        request_id = d.get_u32()?;
        expect_reply = d.get_bool()?;
        if version.has_reserved_octets() {
            d.get_bytes(3)?;
        }
        object_key = d.get_octet_seq()?.to_vec();
        operation = String::from_utf8_lossy(d.get_string_bytes()?).into_owned();
        let _requesting_principal = d.get_octet_seq()?;
    }

    // §9.4.2.1: no padding after the header when the body is empty.
    let body_at = if version.aligns_body() && !d.is_empty() {
        d.align_to(8)?;
        d.offset()
    } else {
        d.offset()
    };

    Ok(Request { version, endian, request_id, object_key, operation, expect_reply, raw, body_at })
}

fn skip_service_contexts(d: &mut Decoder<'_>) -> Result<()> {
    let n = d.get_u32()?;
    let n = d.validate_count(n, 8)?;
    for _ in 0..n {
        let _id = d.get_u32()?;
        let _data = d.get_octet_seq()?;
    }
    Ok(())
}

fn message_header(e: &mut Encoder, version: Version, endian: Endian, ty: MsgType) -> usize {
    e.put_bytes(MAGIC);
    e.put_u8(version.major);
    e.put_u8(version.minor);
    if version.minor == 0 {
        e.put_bool(endian == Endian::Little);
    } else {
        e.put_u8(endian.as_flag());
    }
    e.put_u8(ty as u8);
    let at = e.len();
    e.put_bytes(&[0, 0, 0, 0]);
    at
}

/// Encodes a `Reply` whose body is written by `write_body`.
pub fn encode_reply<F>(
    version: Version,
    endian: Endian,
    request_id: u32,
    status: ReplyStatus,
    write_body: F,
) -> Result<Vec<u8>>
where
    F: FnOnce(&mut Encoder),
{
    let status_code: u32 = match status {
        ReplyStatus::NoException => 0,
        ReplyStatus::UserException => 1,
        ReplyStatus::SystemException => 2,
        ReplyStatus::LocationForward => 3,
        ReplyStatus::LocationForwardPerm => 4,
        ReplyStatus::NeedsAddressingMode => 5,
    };
    if status_code > 3 && !version.is_1_2_layout() {
        // ReplyStatusType_1_0 has four enumerators; emitting 4 or 5 to a 1.1
        // peer would send a value it cannot interpret.
        return Err(Error::BadReplyStatus(status_code));
    }

    let mut e = Encoder::new(endian);
    let size_at = message_header(&mut e, version, endian, MsgType::Reply);

    if version.is_1_2_layout() {
        e.put_u32(request_id);
        e.put_u32(status_code);
        e.put_u32(0); // empty ServiceContextList
    } else {
        e.put_u32(0);
        e.put_u32(request_id);
        e.put_u32(status_code);
    }

    // See encode_request: the body must align from where it will land in the
    // message, not from the start of its own buffer.
    let body_start = if version.aligns_body() { e.len().div_ceil(8) * 8 } else { e.len() };
    let mut body = Encoder::continuing_at(endian, body_start);
    write_body(&mut body);
    let body_bytes = body.finish().map_err(Error::Cdr)?;
    if !body_bytes.is_empty() {
        if version.aligns_body() {
            e.align_to(8);
        }
        e.put_bytes(&body_bytes);
    }

    let size = (e.len() - HEADER_LEN) as u32;
    e.patch_u32(size_at, size);
    e.finish().map_err(Error::Cdr)
}

/// Encodes a `LOCATION_FORWARD` reply, whose body is the new reference.
///
/// §9.4.3.2 requires the client to retry against it *transparently*, so this is
/// how a `ServantLocator` moves a caller without the caller noticing. Phase 1
/// taught us to follow one of these; until now we could not send one.
pub fn encode_location_forward(
    version: Version,
    endian: Endian,
    request_id: u32,
    to: &crate::Ior,
) -> Result<Vec<u8>> {
    let mut err = None;
    let bytes = encode_reply(version, endian, request_id, ReplyStatus::LocationForward, |b| {
        // The IOR is marshalled inline here, not as an encapsulation (§9.3.6).
        if let Err(e) = to.write_to(b) {
            err = Some(e);
        }
    })?;
    match err {
        Some(e) => Err(e),
        None => Ok(bytes),
    }
}

/// Encodes a `Reply` carrying a system exception.
pub fn encode_system_exception(
    version: Version,
    endian: Endian,
    request_id: u32,
    ex: &SystemException,
) -> Result<Vec<u8>> {
    encode_reply(version, endian, request_id, ReplyStatus::SystemException, |b| {
        b.put_str(&ex.id);
        b.put_u32(ex.minor);
        b.put_u32(ex.completed as u32);
    })
}

/// Outcome of a `LocateRequest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum LocateStatus {
    /// We do not know this object.
    UnknownObject = 0,
    /// The object is here; go ahead and invoke.
    ObjectHere = 1,
    /// The object moved; the body holds an IOR.
    ObjectForward = 2,
}

/// A decoded `LocateRequest`.
#[derive(Debug, Clone)]
pub struct LocateRequest {
    /// Correlates the `LocateReply`.
    pub request_id: u32,
    /// Object key being probed.
    pub object_key: Vec<u8>,
    /// Version the peer used.
    pub version: Version,
    /// Byte order the peer used.
    pub endian: Endian,
}

/// Decodes a `LocateRequest`.
///
/// 1.0 and 1.1 carry a bare `object_key`; 1.2 uses `TargetAddress`.
pub fn decode_locate_request(msg: RawMessage) -> Result<LocateRequest> {
    let RawMessage { version, endian, bytes: raw, .. } = msg;
    let mut d = Decoder::new(&raw, endian);
    d.seek_to(HEADER_LEN)?;
    let request_id = d.get_u32()?;
    if version.is_1_2_layout() {
        let disposition = d.get_u16()?;
        if disposition != 0 {
            return Err(Error::UnexpectedMessage(MsgType::LocateRequest));
        }
    }
    let object_key = d.get_octet_seq()?.to_vec();
    Ok(LocateRequest { request_id, object_key, version, endian })
}

/// Encodes a `LocateReply`.
///
/// Note the asymmetry with [`encode_reply`]: §9.4.6 marshals a `LocateReply`
/// body immediately after the header with **no** 8-byte alignment, unlike a
/// `Reply` in GIOP 1.2. Applying the `Reply` rule here shifts every byte of an
/// `OBJECT_FORWARD` body.
pub fn encode_locate_reply(
    version: Version,
    endian: Endian,
    request_id: u32,
    status: LocateStatus,
) -> Result<Vec<u8>> {
    let mut e = Encoder::new(endian);
    let size_at = message_header(&mut e, version, endian, MsgType::LocateReply);
    e.put_u32(request_id);
    e.put_u32(status as u32);
    let size = (e.len() - HEADER_LEN) as u32;
    e.patch_u32(size_at, size);
    e.finish().map_err(Error::Cdr)
}

/// Encodes a `MessageError`.
///
/// §9.4.8 requires this in response to a message whose version or type we do
/// not know, or whose header is malformed. Returning a local error and saying
/// nothing leaves the peer waiting for a reply that will never come.
pub fn encode_message_error(endian: Endian) -> Result<Vec<u8>> {
    let mut e = Encoder::new(endian);
    // §9.4.1: a server meeting an unsupported minor version answers with the
    // highest version it does support.
    let size_at = message_header(&mut e, Version::max_supported(), endian, MsgType::MessageError);
    e.patch_u32(size_at, 0);
    e.finish().map_err(Error::Cdr)
}

/// Encodes a `CloseConnection`.
///
/// §9.4.10: no body at all. Sent by a server shutting down in an orderly
/// way; §9.4.7 then entitles the client to conclude that its unanswered
/// requests were never processed and to re-send them elsewhere — which a
/// bare TCP close does not, leaving the client to guess about completion.
/// (GIOP 1.2 also permits a *client* to send this; our client just closes
/// its socket instead, which 1.2 equally allows.)
pub fn encode_close_connection(version: Version, endian: Endian) -> Result<Vec<u8>> {
    let mut e = Encoder::new(endian);
    let size_at = message_header(&mut e, version, endian, MsgType::CloseConnection);
    e.patch_u32(size_at, 0);
    e.finish().map_err(Error::Cdr)
}

/// What the bytes a dispatch wrote into `out` are, which decides the reply
/// status they travel under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchBody {
    /// A result; the reply goes out `NO_EXCEPTION`.
    Return,
    /// A user exception — repository id first, then the members — going out
    /// `USER_EXCEPTION` (§9.4.3.1). The body shape is exactly what the client
    /// side hands back through [`crate::Error::UserException`], whose
    /// `reply.body()` starts at that repository id.
    UserException,
}

/// What a servant does with an invocation.
///
/// One servant answers every connection: [`Server::serve`] holds it behind a
/// mutex and takes that mutex for the duration of one message, so an
/// implementation still sees `&mut self` and still runs one operation at a
/// time. Two consequences worth stating where they will be read: a servant
/// needs no locking of its own, and a servant that blocks — a long
/// computation, an outbound call — blocks every other client for as long as
/// it blocks. See the module docs.
pub trait Dispatch {
    /// Handles `request`, writing the reply body into `out`.
    ///
    /// Returning `Err` produces a system-exception reply. Returning `Ok` with
    /// nothing written produces an empty `NO_EXCEPTION` reply, which is what a
    /// `void` operation looks like.
    fn dispatch(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException>;

    /// As [`Dispatch::dispatch`], but able to label what it wrote a user
    /// exception. This is the method [`Server`] actually calls; the default
    /// delegates to `dispatch`, so a servant with no user exceptions
    /// implements only that and nothing changes for it.
    ///
    /// An override must not write into `out` before it knows which label the
    /// bytes get — the whole buffer travels under a single reply status.
    fn dispatch_body(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<DispatchBody, SystemException> {
        self.dispatch(request, out).map(|()| DispatchBody::Return)
    }

    /// Whether this servant answers to `object_key`. Defaults to accepting
    /// everything, which is right for a single-servant process.
    fn knows(&self, _object_key: &[u8]) -> bool {
        true
    }

    /// A reference to redirect this request to, instead of serving it.
    ///
    /// Returning `Some` produces a `LOCATION_FORWARD`, which §9.4.3.2 requires
    /// the client to retry against transparently. It lives here rather than in
    /// `dispatch` because a forward *replaces* the reply rather than filling
    /// one in.
    fn forward(&mut self, _request: &Request) -> Option<crate::Ior> {
        None
    }
}

/// How many connections one [`Server`] serves at once before refusing.
///
/// Sixty-four is a bound, not a capacity estimate: it is far above the
/// handful of agents and fixtures anything here has ever held open, and far
/// below the point where a thread per connection costs real memory. Raise it
/// with [`Server::set_max_connections`] when the deployment knows better.
pub const DEFAULT_MAX_CONNECTIONS: usize = 64;

/// How long a connection thread waits for its peer's next message before
/// re-checking the stop flag, and how long the accept loop sleeps between
/// polls. This is the granularity of shutdown, not of service: a message that
/// arrives is served the moment it arrives.
pub const STOP_POLL: Duration = Duration::from_millis(50);

/// How long a peer that has started a message may stall before its connection
/// is dropped. It bounds *within* a message; an idle connection between
/// messages is not affected and may idle forever.
pub const DEFAULT_MESSAGE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
struct Counters {
    accepted: AtomicU64,
    refused: AtomicU64,
    active: AtomicU64,
    peak: AtomicU64,
}

/// A live view of one [`Server`]'s connection counters.
///
/// Cloning shares the counters, so a caller can hold this after moving the
/// server into its serving thread — which is how every fixture here is
/// arranged. The point is the cap being *observable*: a refused client that
/// is only logged is invisible to a harness, and an unmeasured refusal is a
/// failure by the same rule that makes an unmeasured check one.
#[derive(Debug, Clone, Default)]
pub struct ServerStats {
    counters: Arc<Counters>,
}

impl ServerStats {
    /// Connections admitted and served since the server was bound.
    pub fn accepted(&self) -> u64 {
        self.counters.accepted.load(Ordering::Relaxed)
    }

    /// Connections refused because the cap was already reached.
    pub fn refused(&self) -> u64 {
        self.counters.refused.load(Ordering::Relaxed)
    }

    /// Connections being served right now.
    pub fn active(&self) -> u64 {
        self.counters.active.load(Ordering::Relaxed)
    }

    /// The high-water mark of [`ServerStats::active`] — the measured overlap.
    pub fn peak_active(&self) -> u64 {
        self.counters.peak.load(Ordering::Relaxed)
    }

    /// Takes a slot if the cap allows, counting the outcome either way.
    fn admit(&self, cap: usize) -> Option<Slot> {
        let cap = cap as u64;
        let mut active = self.counters.active.load(Ordering::Acquire);
        loop {
            if active >= cap {
                self.counters.refused.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            match self.counters.active.compare_exchange_weak(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.counters.accepted.fetch_add(1, Ordering::Relaxed);
                    self.counters.peak.fetch_max(active + 1, Ordering::Relaxed);
                    return Some(Slot { stats: self.clone() });
                }
                Err(observed) => active = observed,
            }
        }
    }
}

/// One admitted connection's slot, returned to the cap on drop — including
/// the drop that unwinding a panicking servant performs, which is why this is
/// a guard and not a pair of counter calls.
#[derive(Debug)]
struct Slot {
    stats: ServerStats,
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.stats.counters.active.fetch_sub(1, Ordering::AcqRel);
    }
}

/// What waiting for a peer's next message ended in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Waiting {
    /// Bytes are there; read the message.
    Ready,
    /// The peer hung up.
    PeerGone,
    /// The stop flag went up while we waited.
    Stopped,
}

/// Takes the shared servant, recovering a poisoned lock rather than
/// propagating it.
///
/// A servant that panicked mid-dispatch poisons the mutex. Refusing to serve
/// anyone afterwards would turn one bad request into a dead service for every
/// other client — the opposite of the rule the accept loop has always
/// followed. The panic itself is not swallowed: it still ends the connection
/// that caused it and still surfaces from [`Server::serve`] when the scope
/// joins.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A GIOP server that serves its connections concurrently, one thread each,
/// bounded by [`Server::max_connections`].
///
/// A POA and servant lifecycle remain later work; what is here is one servant
/// (see [`Dispatch`]) answering every client that dials it. Read the module
/// docs for what the sharing does and does not buy before assuming the second.
#[derive(Debug)]
pub struct Server {
    listener: TcpListener,
    object_key: Vec<u8>,
    max_message_size: usize,
    fragment_threshold: usize,
    max_connections: usize,
    message_timeout: Duration,
    stats: ServerStats,
}

impl Server {
    /// Binds to `addr` and adopts `object_key` as the servant's identity.
    pub fn bind(addr: &str, object_key: Vec<u8>) -> Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(addr)?,
            object_key,
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
            fragment_threshold: crate::DEFAULT_FRAGMENT_THRESHOLD,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            message_timeout: DEFAULT_MESSAGE_TIMEOUT,
            stats: ServerStats::default(),
        })
    }

    /// A handle on this server's connection counters, clonable and readable
    /// while the server is serving.
    pub fn stats(&self) -> ServerStats {
        self.stats.clone()
    }

    /// How many connections are served at once before further ones are
    /// refused with a `CloseConnection`.
    pub fn max_connections(&self) -> usize {
        self.max_connections
    }

    /// Overrides the cap. Zero would refuse everything, so it is clamped to
    /// one: a server that serves nobody is a configuration mistake, not a
    /// policy anyone means to express.
    pub fn set_max_connections(&mut self, n: usize) {
        self.max_connections = n.max(1);
    }

    /// Overrides how long a peer may stall *inside* a message before its
    /// connection is dropped. See [`DEFAULT_MESSAGE_TIMEOUT`].
    pub fn set_message_timeout(&mut self, timeout: Duration) {
        self.message_timeout = timeout;
    }

    /// The address actually bound, after any port-zero assignment.
    pub fn local_addr(&self) -> Result<std::net::SocketAddr> {
        Ok(self.listener.local_addr()?)
    }

    /// Overrides the outbound fragmentation threshold.
    pub fn set_fragment_threshold(&mut self, bytes: usize) {
        self.fragment_threshold = bytes;
    }

    /// The servant's object key.
    pub fn object_key(&self) -> &[u8] {
        &self.object_key
    }

    /// Builds a publishable IOR for this server.
    ///
    /// `host` is what goes into the profile, which is deliberately a separate
    /// argument from the bind address: behind NAT or in a container the two
    /// differ, and publishing the bind address is the failure Phase 0
    /// assumption D reproduced.
    pub fn ior(&self, type_id: &str, host: &str) -> Result<crate::Ior> {
        let port = self.local_addr()?.port();
        Ok(crate::Ior {
            type_id: type_id.to_owned(),
            profiles: vec![crate::IiopProfile {
                version: Version::V1_2,
                host: host.to_owned(),
                port,
                object_key: self.object_key.clone(),
                components: Vec::new(),
            }],
        })
    }

    /// Serves connections concurrently until `stop` returns true.
    ///
    /// Each accepted connection gets a thread; `dispatch` is shared between
    /// them behind a mutex taken per message, so connections overlap and
    /// operations do not (module docs). Over [`Server::max_connections`] a
    /// connection is refused with a `CloseConnection` and counted in
    /// [`ServerStats::refused`].
    ///
    /// `stop` is polled by the accept loop and by every connection thread at
    /// [`STOP_POLL`]. A stop that lands mid-connection ends it with an
    /// orderly `CloseConnection` (§9.4.10) rather than a bare TCP close, so
    /// the peer knows its unanswered requests were not processed and may
    /// re-send them elsewhere. `serve` returns only once every connection
    /// thread has ended, so nothing it started outlives it.
    ///
    /// A servant that panics still ends the server, exactly as it did when
    /// there was one loop: the panic surfaces from `serve` when the scope
    /// joins. What does *not* happen is the other connections dying with it —
    /// a poisoned servant mutex is recovered rather than propagated, because
    /// one bad request must not take the service down.
    pub fn serve<D, S>(&self, dispatch: &mut D, stop: S) -> Result<()>
    where
        D: Dispatch + Send,
        S: Fn() -> bool + Sync,
    {
        let servant = Mutex::new(dispatch);
        let stop = &stop;
        let outcome = std::thread::scope(|scope| -> Result<()> {
            // Polled rather than blocking, so a raised flag is noticed
            // without a client having to arrive to unblock the accept. The
            // poll sleeps: a spin here is the harness rule's wait loop that
            // does not wait, in server form.
            self.listener.set_nonblocking(true)?;
            loop {
                if stop() {
                    return Ok(());
                }
                match self.listener.accept() {
                    Ok((stream, peer)) => {
                        // macOS hands the accepted socket the listener's
                        // non-blocking flag; read_message needs a blocking
                        // one, and inheritance is per-platform, so this is
                        // set rather than assumed. A failure here costs this
                        // one connection and is *not* returned: leaving the
                        // loop with live threads and no raised flag would
                        // hang the scope's join for as long as they last.
                        if let Err(e) = stream.set_nonblocking(false) {
                            eprintln!(
                                "orbweaver: dropping {peer}: not restorable to blocking: {e}"
                            );
                            continue;
                        }
                        match self.stats.admit(self.max_connections) {
                            Some(slot) => {
                                let servant = &servant;
                                scope.spawn(move || {
                                    let _slot = slot;
                                    // One bad client must not take the
                                    // server down.
                                    if let Err(e) =
                                        self.serve_connection_until(stream, servant, &stop)
                                    {
                                        eprintln!("orbweaver: connection ended: {e}");
                                    }
                                });
                            }
                            None => self.refuse(stream, peer),
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(STOP_POLL);
                    }
                    Err(e) => eprintln!("orbweaver: accept failed: {e}"),
                }
            }
        });
        // Hand the listener back the way it was found, so a second serve on
        // the same server — or a direct accept in a test — is unsurprised.
        let _ = self.listener.set_nonblocking(false);
        outcome
    }

    /// Turns a connection away because the cap is full.
    ///
    /// §9.4.7: the goodbye means "not processed, safe to re-send elsewhere",
    /// which is exactly true here and is more than a TCP reset would say.
    fn refuse(&self, mut s: TcpStream, peer: SocketAddr) {
        eprintln!(
            "orbweaver: refusing {peer}: {} connections already served (cap {}), {} refused so far",
            self.stats.active(),
            self.max_connections,
            self.stats.refused(),
        );
        if let Ok(bye) = encode_close_connection(Version::max_supported(), Endian::native()) {
            let _ = s.write_all(&bye);
        }
        let _ = s.shutdown(std::net::Shutdown::Both);
    }

    /// Handles one connection to completion, on this thread.
    ///
    /// The servant is exclusively this connection's for the call, which is
    /// what makes this usable for a hand-rolled accept loop; [`Server::serve`]
    /// shares one servant across many of these instead.
    pub fn serve_connection<D: Dispatch>(&self, s: TcpStream, d: &mut D) -> Result<()> {
        let servant = Mutex::new(d);
        self.serve_connection_until(s, &servant, &|| false)
    }

    /// As [`Server::serve_connection`], taking the shared servant per message
    /// and ending with an orderly `CloseConnection` when `stop` reports true
    /// between messages.
    fn serve_connection_until<D: Dispatch>(
        &self,
        mut s: TcpStream,
        servant: &Mutex<&mut D>,
        stop: &dyn Fn() -> bool,
    ) -> Result<()> {
        s.set_nodelay(true)?;
        // The version and byte order to stamp on a CloseConnection we send:
        // whatever the peer last spoke, defaulting to our best before its
        // first message. (The body is empty, so the endian flag is the only
        // byte it affects.)
        let mut wire_version = Version::max_supported();
        let mut wire_endian = Endian::native();
        loop {
            if stop() {
                let out = encode_close_connection(wire_version, wire_endian)?;
                s.write_all(&out)?;
                return Ok(());
            }
            match self.await_message(&s, stop)? {
                Waiting::Ready => {}
                Waiting::PeerGone => return Ok(()),
                Waiting::Stopped => {
                    let out = encode_close_connection(wire_version, wire_endian)?;
                    s.write_all(&out)?;
                    return Ok(());
                }
            }
            let msg = match read_message(&mut s, self.max_message_size) {
                Ok(m) => m,
                Err(Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
                    ) =>
                {
                    return Ok(()); // peer hung up; not an error
                }
                Err(e) => {
                    // §9.4.8: tell the peer rather than leaving it waiting.
                    let _ = encode_message_error(Endian::native())
                        .and_then(|m| s.write_all(&m).map_err(Error::Io));
                    return Err(e);
                }
            };

            wire_version = msg.version;
            wire_endian = msg.endian;
            match msg.msg_type {
                MsgType::LocateRequest => {
                    let lr = decode_locate_request(msg)?;
                    let status = if lock(servant).knows(&lr.object_key) {
                        LocateStatus::ObjectHere
                    } else {
                        LocateStatus::UnknownObject
                    };
                    let out = encode_locate_reply(lr.version, lr.endian, lr.request_id, status)?;
                    s.write_all(&out)?;
                }
                MsgType::Request => {
                    let req = decode_request(msg)?;
                    // The lock spans knows/forward/dispatch — one request is
                    // one indivisible look at the servant — and is released
                    // before the reply is written, so a slow *socket* holds
                    // nobody up. Only a slow servant does.
                    let reply = {
                        let mut servant = lock(servant);
                        self.handle_request(&req, &mut **servant)?
                    };
                    if let Some(bytes) = reply {
                        for piece in fragment_message(bytes, self.fragment_threshold)? {
                            s.write_all(&piece)?;
                        }
                    }
                }
                MsgType::CancelRequest => {
                    // §9.4.4 makes cancellation advisory and permits ignoring
                    // it, and requests here are handled inline before the next
                    // read, so there is never a queued request to abandon:
                    // consuming the message IS the correct handling, not a
                    // stub. Log-worthy, never an error — a malformed one is
                    // ignored too, since ignoring is what we would do with a
                    // well-formed one.
                    let mut d = Decoder::new(&msg.bytes, msg.endian);
                    if d.seek_to(HEADER_LEN).is_ok()
                        && let Ok(id) = d.get_u32()
                    {
                        eprintln!(
                            "orbweaver: peer cancelled request {id}; nothing is queued, ignoring"
                        );
                    }
                }
                MsgType::CloseConnection => return Ok(()),
                other => {
                    let _ = encode_message_error(msg.endian)
                        .and_then(|m| s.write_all(&m).map_err(Error::Io));
                    return Err(Error::UnexpectedMessage(other));
                }
            }
        }
    }

    fn handle_request<D: Dispatch>(&self, req: &Request, d: &mut D) -> Result<Option<Vec<u8>>> {
        if !d.knows(&req.object_key) {
            return self.reply_exception(req, &SystemException::object_not_exist());
        }
        if let Some(to) = d.forward(req) {
            if !req.expect_reply {
                // A oneway cannot be redirected; there is no reply to carry the
                // new address, and inventing one would be worse than serving it.
                return Ok(None);
            }
            return Ok(Some(encode_location_forward(
                req.version,
                req.endian,
                req.request_id,
                &to,
            )?));
        }

        // Servants write into a detached buffer, so it too must know where it
        // will land. A 1.0 reply body starts immediately after the header.
        //
        // GIOP 1.0/1.1 put the service context list before `request_id` and
        // `reply_status`; 1.2 puts it after. Both therefore measure
        // 4 + 4 + 4 = 12 bytes here — but only because the list we emit is
        // empty. This was written as a version branch with the same value in
        // both arms, which read as though the difference had been accounted
        // for; it has not. Emitting any reply service context makes the two
        // layouts differ and this constant wrong.
        let reply_header_len = HEADER_LEN + 12;
        let body_start = if req.version.aligns_body() {
            reply_header_len.div_ceil(8) * 8
        } else {
            reply_header_len
        };
        let mut out = Encoder::continuing_at(req.endian, body_start);
        match d.dispatch_body(req, &mut out) {
            Ok(kind) => {
                if !req.expect_reply {
                    // A oneway can carry neither a result nor a raised user
                    // exception; dropping both is what the spec requires.
                    return Ok(None);
                }
                let status = match kind {
                    DispatchBody::Return => ReplyStatus::NoException,
                    DispatchBody::UserException => ReplyStatus::UserException,
                };
                let body = out.finish().map_err(Error::Cdr)?;
                Ok(Some(encode_reply(req.version, req.endian, req.request_id, status, |e| {
                    e.put_bytes(&body)
                })?))
            }
            Err(ex) => self.reply_exception(req, &ex),
        }
    }

    /// Waits for the peer's next message, waking often enough to notice
    /// `stop`.
    ///
    /// The wait is a one-byte `peek` under a short read timeout, not a timed
    /// `read`: a timeout that fired in the middle of `read_message` would
    /// have consumed bytes it cannot put back, and the connection's framing
    /// would be gone. Peeking leaves every byte where it was, so the message
    /// is then read with the timeout that bounds a *stalled* peer instead.
    fn await_message(&self, s: &TcpStream, stop: &dyn Fn() -> bool) -> Result<Waiting> {
        s.set_read_timeout(Some(STOP_POLL))?;
        let mut probe = [0u8; 1];
        let waiting = loop {
            match s.peek(&mut probe) {
                Ok(0) => break Waiting::PeerGone,
                Ok(_) => break Waiting::Ready,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    // The poll expired with nothing to read. Platforms
                    // disagree about which of the two kinds that is.
                    if stop() {
                        break Waiting::Stopped;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
                    ) =>
                {
                    break Waiting::PeerGone;
                }
                Err(e) => return Err(Error::Io(e)),
            }
        };
        // Whatever comes next is read with the stall bound, not the poll one.
        s.set_read_timeout(Some(self.message_timeout))?;
        Ok(waiting)
    }

    fn reply_exception(&self, req: &Request, ex: &SystemException) -> Result<Option<Vec<u8>>> {
        if !req.expect_reply {
            // A oneway cannot carry a failure back. Dropping it is what the
            // specification requires, not something to paper over.
            return Ok(None);
        }
        Ok(Some(encode_system_exception(req.version, req.endian, req.request_id, ex)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode_request;

    /// Every version must survive our own encoder feeding our own decoder.
    /// This is weaker evidence than an interop run, but it catches a whole
    /// version's layout being transposed.
    #[test]
    fn request_round_trips_in_every_version() {
        for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
            for endian in [Endian::Big, Endian::Little] {
                let wire = encode_request(version, endian, 77, b"objkey", "compute", true, |e| {
                    e.put_i32(-5);
                    e.put_f64(2.5);
                })
                .unwrap();

                let mut cursor: &[u8] = &wire;
                let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
                let req = decode_request(msg).unwrap();

                assert_eq!(req.request_id, 77, "{version} {endian:?}");
                assert_eq!(req.object_key, b"objkey");
                assert_eq!(req.operation, "compute");
                assert!(req.expect_reply);
                let mut b = req.body().unwrap();
                assert_eq!(b.get_i32().unwrap(), -5, "{version} {endian:?}");
                assert_eq!(b.get_f64().unwrap(), 2.5, "{version} {endian:?}");
            }
        }
    }

    #[test]
    fn oneway_request_is_recognised_as_such() {
        for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
            let wire =
                encode_request(version, Endian::Big, 1, b"k", "fire", false, |_| {}).unwrap();
            let mut cursor: &[u8] = &wire;
            let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
            assert!(!decode_request(msg).unwrap().expect_reply, "{version}");
        }
    }

    #[test]
    fn reply_round_trips_in_every_version() {
        for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
            for endian in [Endian::Big, Endian::Little] {
                let wire = encode_reply(version, endian, 88, ReplyStatus::NoException, |e| {
                    e.put_f64(1.25)
                })
                .unwrap();
                let mut cursor: &[u8] = &wire;
                let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
                let reply = crate::decode_reply(msg).unwrap();
                assert_eq!(reply.request_id, 88, "{version} {endian:?}");
                assert_eq!(reply.status, ReplyStatus::NoException);
                assert_eq!(reply.body().unwrap().get_f64().unwrap(), 1.25, "{version} {endian:?}");
            }
        }
    }

    #[test]
    fn system_exception_round_trips() {
        let ex = SystemException::bad_operation();
        let wire = encode_system_exception(Version::V1_2, Endian::Big, 9, &ex).unwrap();
        let mut cursor: &[u8] = &wire;
        let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
        let reply = crate::decode_reply(msg).unwrap();
        assert_eq!(reply.status, ReplyStatus::SystemException);
        let mut b = reply.body().unwrap();
        assert_eq!(b.get_string().unwrap(), BAD_OPERATION);
        assert_eq!(b.get_u32().unwrap(), 0);
        assert_eq!(b.get_u32().unwrap(), Completion::No as u32);
    }

    /// A 1.2-only status must not be emitted to a 1.0/1.1 peer, which has no
    /// enumerator for it.
    #[test]
    fn post_1_2_status_is_refused_on_older_versions() {
        for version in [Version::V1_0, Version::V1_1] {
            assert!(
                encode_reply(version, Endian::Big, 1, ReplyStatus::LocationForwardPerm, |_| {})
                    .is_err(),
                "{version} has no LOCATION_FORWARD_PERM"
            );
        }
        assert!(
            encode_reply(Version::V1_2, Endian::Big, 1, ReplyStatus::LocationForwardPerm, |_| {})
                .is_ok()
        );
    }

    /// §9.4.6: a LocateReply body follows the header with no alignment, unlike
    /// a Reply in 1.2. The header alone must therefore be exactly 8 bytes.
    #[test]
    fn locate_reply_is_not_body_aligned() {
        let wire =
            encode_locate_reply(Version::V1_2, Endian::Big, 3, LocateStatus::ObjectHere).unwrap();
        assert_eq!(wire.len(), HEADER_LEN + 8, "no padding may follow the locate header");
        assert_eq!(u32::from_be_bytes([wire[8], wire[9], wire[10], wire[11]]), 8);
        assert_eq!(u32::from_be_bytes([wire[12], wire[13], wire[14], wire[15]]), 3);
        assert_eq!(u32::from_be_bytes([wire[16], wire[17], wire[18], wire[19]]), 1);
    }

    #[test]
    fn locate_request_round_trips_in_every_version() {
        for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
            let mut e = Encoder::new(Endian::Little);
            let size_at = message_header(&mut e, version, Endian::Little, MsgType::LocateRequest);
            e.put_u32(4);
            if version.is_1_2_layout() {
                e.put_u16(0);
            }
            e.put_octet_seq(b"probe");
            let size = (e.len() - HEADER_LEN) as u32;
            e.patch_u32(size_at, size);
            let wire = e.finish().unwrap();

            let mut cursor: &[u8] = &wire;
            let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
            let lr = decode_locate_request(msg).unwrap();
            assert_eq!(lr.request_id, 4, "{version}");
            assert_eq!(lr.object_key, b"probe", "{version}");
        }
    }

    #[test]
    fn message_error_is_a_bare_header() {
        let wire = encode_message_error(Endian::Big).unwrap();
        assert_eq!(wire.len(), HEADER_LEN);
        assert_eq!(wire[7], MsgType::MessageError as u8);
        assert_eq!(&wire[8..12], &[0, 0, 0, 0], "no body");
    }

    #[test]
    fn close_connection_is_a_bare_header() {
        for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
            let wire = encode_close_connection(version, Endian::Big).unwrap();
            assert_eq!(wire.len(), HEADER_LEN, "{version}: §9.4.10 allows no body");
            assert_eq!(wire[7], MsgType::CloseConnection as u8);
            assert_eq!(&wire[8..12], &[0, 0, 0, 0], "message_size must be zero");
        }
    }

    /// A servant for the loopback tests: answers `ping` with 42.
    struct Pong;
    impl Dispatch for Pong {
        fn dispatch(
            &mut self,
            req: &Request,
            out: &mut Encoder,
        ) -> std::result::Result<(), SystemException> {
            match req.operation.as_str() {
                "ping" => {
                    out.put_i32(42);
                    Ok(())
                }
                _ => Err(SystemException::bad_operation()),
            }
        }
    }

    fn ping_wire(version: Version, endian: Endian, id: u32) -> Vec<u8> {
        encode_request(version, endian, id, b"k", "ping", true, |_| {}).unwrap()
    }

    fn expect_pong(s: &mut TcpStream, id: u32, why: &str) {
        let msg = read_message(s, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
        let reply = crate::decode_reply(msg).unwrap();
        assert_eq!(reply.request_id, id, "{why}");
        assert_eq!(reply.status, ReplyStatus::NoException, "{why}");
        assert_eq!(reply.body().unwrap().get_i32().unwrap(), 42, "{why}");
    }

    /// §9.4.4 permits ignoring a CancelRequest; what must not happen is the
    /// stream losing its framing over one. A cancel between two requests —
    /// for an id that was never issued, matching what a client can actually
    /// send — must leave the following request answered as if it were not
    /// there.
    #[test]
    fn a_cancel_request_mid_stream_does_not_disturb_the_following_request() {
        for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
            for endian in [Endian::Big, Endian::Little] {
                let server = Server::bind("127.0.0.1:0", b"k".to_vec()).unwrap();
                let addr = server.local_addr().unwrap();
                let t = std::thread::spawn(move || {
                    let (s, _) = server.listener.accept().unwrap();
                    server.serve_connection(s, &mut Pong).unwrap();
                });

                let mut c = TcpStream::connect(addr).unwrap();
                c.write_all(&ping_wire(version, endian, 1)).unwrap();
                expect_pong(&mut c, 1, "before the cancel");

                c.write_all(&crate::encode_cancel_request(version, endian, 9999).unwrap()).unwrap();
                c.write_all(&ping_wire(version, endian, 2)).unwrap();
                expect_pong(&mut c, 2, "the request after a cancel must be undisturbed");

                drop(c); // hang up; the server thread must end cleanly
                t.join().unwrap();
            }
        }
    }

    /// The serving half of an orderly shutdown: when the stop flag is raised
    /// mid-connection, the peer's last sight of us is a CloseConnection, not
    /// a bare TCP close it must guess about.
    #[test]
    fn a_stopped_server_says_goodbye_with_close_connection() {
        for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
            let server = Server::bind("127.0.0.1:0", b"k".to_vec()).unwrap();
            let addr = server.local_addr().unwrap();
            let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let served = flag.clone();
            let t = std::thread::spawn(move || {
                server
                    .serve(&mut Pong, || served.load(std::sync::atomic::Ordering::SeqCst))
                    .unwrap();
            });

            let mut c = TcpStream::connect(addr).unwrap();
            c.write_all(&ping_wire(version, Endian::Big, 1)).unwrap();
            expect_pong(&mut c, 1, "before the stop");

            // Raise the flag, then send one more request. This test first
            // claimed the outcome was deterministic ("the server answers this
            // one and then says goodbye") and CI refuted it: the server checks
            // the flag BETWEEN messages, so when the flag is observed while
            // request 2 is still in flight, the goodbye legitimately precedes
            // the answer — and §9.4.7's whole point is that this is orderly:
            // the request was not processed and is safe to re-send. Both
            // orders are legal; what must never happen is anything that is
            // neither a Reply to 2 nor a CloseConnection.
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            c.write_all(&ping_wire(version, Endian::Big, 2)).unwrap();

            let first = read_message(&mut c, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
            let bye = match first.msg_type {
                // The flag was seen after request 2: answered, then goodbye.
                MsgType::Reply => {
                    let reply = crate::decode_reply(first).unwrap();
                    assert_eq!(reply.request_id, 2, "{version}");
                    assert_eq!(reply.body().unwrap().get_i32().unwrap(), 42, "{version}");
                    read_message(&mut c, DEFAULT_MAX_MESSAGE_SIZE).unwrap()
                }
                // The flag was seen first: request 2 goes unprocessed, which
                // is exactly the safe-to-retry case the message exists for.
                MsgType::CloseConnection => first,
                other => panic!("{version}: neither a reply nor a goodbye: {other:?}"),
            };
            assert_eq!(bye.msg_type, MsgType::CloseConnection, "{version}");
            assert_eq!(bye.bytes.len(), HEADER_LEN, "{version}: no body");
            t.join().unwrap();
        }
    }

    /// The two halves of §9.4.7 against each other: our server's close bytes
    /// through our client's handling. The client must classify them as a
    /// clean close — request not processed, safe to re-send — and refuse to
    /// reuse the connection.
    #[test]
    fn our_close_bytes_read_as_safe_to_retry_by_our_own_client() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            // Read the client's request, then answer with the orderly
            // shutdown bytes instead of a Reply.
            let req = read_message(&mut s, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
            let out = encode_close_connection(req.version, req.endian).unwrap();
            s.write_all(&out).unwrap();
            // Hold the socket open until the client hangs up, so the close
            // bytes cannot be raced away by a TCP reset.
            let _ = read_message(&mut s, DEFAULT_MAX_MESSAGE_SIZE);
        });

        let ior = crate::Ior {
            type_id: "IDL:spike/Echo:1.0".into(),
            profiles: vec![crate::IiopProfile {
                version: Version::V1_2,
                host: "127.0.0.1".into(),
                port,
                object_key: b"k".to_vec(),
                components: Vec::new(),
            }],
        };
        let mut conn = crate::Connection::connect(&ior, std::time::Duration::from_secs(5)).unwrap();
        let err = conn.invoke_nullary("ping").unwrap_err();
        assert!(matches!(err, Error::ConnectionClosed), "got {err}");
        assert!(!conn.is_usable(), "a cleanly closed connection must not be reused");
        drop(conn);
        t.join().unwrap();
    }

    // ── concurrency ──────────────────────────────────────────────────────────
    //
    // Every test below bounds itself twice: the clients' sockets carry a read
    // timeout, and every rendezvous carries a deadline. Serialization must
    // make these tests FAIL, and a failure that arrives as a hang is a test
    // nobody can read.

    use std::sync::Condvar;
    use std::sync::atomic::{AtomicBool, AtomicU64};

    /// The deadline every wait in these tests answers to. Generous enough to
    /// survive a loaded CI box, short enough that a genuine deadlock is a
    /// failed test rather than a killed job.
    const DEADLINE: Duration = Duration::from_secs(10);

    /// A rendezvous for `n` parties, with a deadline.
    ///
    /// `arrive` returns false rather than blocking forever when the others do
    /// not turn up, which is what turns "the server serialized us" from a
    /// hung test into a failing one.
    struct Gate {
        n: usize,
        arrived: Mutex<usize>,
        wake: Condvar,
    }

    impl Gate {
        fn new(n: usize) -> Self {
            Gate { n, arrived: Mutex::new(0), wake: Condvar::new() }
        }

        fn arrive(&self, within: Duration) -> bool {
            let mut arrived = lock(&self.arrived);
            *arrived += 1;
            if *arrived >= self.n {
                self.wake.notify_all();
                return true;
            }
            let (_guard, timed_out) = self
                .wake
                .wait_timeout_while(arrived, within, |a| *a < self.n)
                .unwrap_or_else(|e| e.into_inner());
            !timed_out.timed_out()
        }
    }

    /// A server on loopback, serving `servant`, with its counters readable
    /// after the server itself has moved into the serving thread.
    struct Loopback {
        addr: std::net::SocketAddr,
        stats: ServerStats,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    fn serving<D: Dispatch + Send + 'static>(mut servant: D, cap: usize) -> Loopback {
        let mut server = Server::bind("127.0.0.1:0", b"k".to_vec()).unwrap();
        server.set_max_connections(cap);
        let addr = server.local_addr().unwrap();
        let stats = server.stats();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            server.serve(&mut servant, move || flag.load(Ordering::SeqCst)).unwrap();
        });
        Loopback { addr, stats, stop, thread: Some(thread) }
    }

    impl Loopback {
        /// A client socket that times out rather than waiting forever.
        fn client(&self) -> TcpStream {
            let c = TcpStream::connect(self.addr).unwrap();
            c.set_read_timeout(Some(DEADLINE)).unwrap();
            c
        }

        /// Waits, sleeping, for the counters to say `want`.
        fn wait_until(&self, want: impl Fn(&ServerStats) -> bool) -> bool {
            let deadline = std::time::Instant::now() + DEADLINE;
            while std::time::Instant::now() < deadline {
                if want(&self.stats) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            want(&self.stats)
        }

        fn shutdown(mut self) {
            self.stop.store(true, Ordering::SeqCst);
            self.thread.take().unwrap().join().unwrap();
        }
    }

    fn ior_at(addr: std::net::SocketAddr, key: &[u8]) -> crate::Ior {
        crate::Ior {
            type_id: "IDL:spike/Echo:1.0".into(),
            profiles: vec![crate::IiopProfile {
                version: Version::V1_2,
                host: addr.ip().to_string(),
                port: addr.port(),
                object_key: key.to_vec(),
                components: Vec::new(),
            }],
        }
    }

    /// The limit this batch exists to remove: N clients, each mid-session at
    /// the same time.
    ///
    /// Each client completes a full request/reply, then waits at a gate every
    /// one of them must reach, then completes a second. Under a
    /// one-connection-at-a-time server the second client's *first* reply
    /// never arrives while the first client holds its socket, so the gate
    /// times out and the read times out — the test fails, twice over, rather
    /// than hanging.
    ///
    /// The overlap is also read off the server itself: the high-water mark of
    /// live connections must reach N. Note what is *not* claimed — the
    /// servant still runs one dispatch at a time, so this measures concurrent
    /// *sessions*, which is the limit that was removed.
    #[test]
    fn n_clients_hold_sessions_at_once_in_both_byte_orders() {
        const N: usize = 6;
        for endian in [Endian::Big, Endian::Little] {
            let served = serving(Pong, DEFAULT_MAX_CONNECTIONS);
            let gate = Arc::new(Gate::new(N));
            let clients: Vec<_> = (0..N)
                .map(|i| {
                    let mut c = served.client();
                    let gate = Arc::clone(&gate);
                    std::thread::spawn(move || {
                        let why = format!("client {i} {endian:?}");
                        c.write_all(&ping_wire(Version::V1_2, endian, 1)).unwrap();
                        expect_pong(&mut c, 1, &why);
                        let all_here = gate.arrive(DEADLINE);
                        c.write_all(&ping_wire(Version::V1_2, endian, 2)).unwrap();
                        expect_pong(&mut c, 2, &why);
                        all_here
                    })
                })
                .collect();
            for (i, t) in clients.into_iter().enumerate() {
                assert!(t.join().unwrap(), "client {i} ({endian:?}) waited out the gate alone");
            }
            assert!(
                served.stats.peak_active() >= N as u64,
                "{endian:?}: peak concurrency was {}, wanted {N}",
                served.stats.peak_active()
            );
            assert_eq!(served.stats.refused(), 0, "{endian:?}: nothing should have been refused");
            served.shutdown();
        }
    }

    /// The cap is a bound, and a bound nobody can see is not one. Over it, a
    /// connection is refused with §9.4.7's goodbye — "not processed, re-send
    /// elsewhere" — counted, and the clients already inside keep working.
    #[test]
    fn over_the_cap_a_connection_is_refused_with_a_goodbye_and_counted() {
        let served = serving(Pong, 2);
        let mut a = served.client();
        a.write_all(&ping_wire(Version::V1_2, Endian::Big, 1)).unwrap();
        expect_pong(&mut a, 1, "first client under the cap");
        let mut b = served.client();
        b.write_all(&ping_wire(Version::V1_2, Endian::Big, 1)).unwrap();
        expect_pong(&mut b, 1, "second client under the cap");
        assert!(served.wait_until(|s| s.active() == 2), "both clients should be counted live");

        // The third only reads: a refusal that raced the peer's own write
        // could be answered by a reset instead, and the point being measured
        // here is the goodbye.
        let mut over = served.client();
        let msg = read_message(&mut over, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
        assert_eq!(msg.msg_type, MsgType::CloseConnection, "over the cap must be told, not queued");
        assert_eq!(served.stats.refused(), 1);
        assert_eq!(served.stats.accepted(), 2, "a refused connection is not an accepted one");

        // The cap turned somebody away; it did not disturb anybody.
        a.write_all(&ping_wire(Version::V1_2, Endian::Big, 2)).unwrap();
        expect_pong(&mut a, 2, "after a refusal the admitted clients continue");
        b.write_all(&ping_wire(Version::V1_2, Endian::Big, 2)).unwrap();
        expect_pong(&mut b, 2, "after a refusal the admitted clients continue");

        // A slot freed is a slot reusable.
        drop(a);
        assert!(served.wait_until(|s| s.active() == 1), "the dropped client must free its slot");
        let mut c = served.client();
        c.write_all(&ping_wire(Version::V1_2, Endian::Big, 3)).unwrap();
        expect_pong(&mut c, 3, "a freed slot admits the next client");
        assert_eq!(served.stats.refused(), 1, "no second refusal");
        drop((b, c, over));
        served.shutdown();
    }

    /// A servant counting oneways, plus a `ping` to fence them: the reply to
    /// the ping proves the oneway ahead of it on the same connection was
    /// consumed.
    struct Counting {
        hits: Arc<AtomicU64>,
    }

    impl Dispatch for Counting {
        fn dispatch(
            &mut self,
            req: &Request,
            out: &mut Encoder,
        ) -> std::result::Result<(), SystemException> {
            match req.operation.as_str() {
                "bump" => {
                    self.hits.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
                "ping" => {
                    out.put_i32(42);
                    Ok(())
                }
                _ => Err(SystemException::bad_operation()),
            }
        }
    }

    /// Oneways under concurrency: no reply is owed, so nothing correlates
    /// them, which is exactly why they are worth counting. Every one sent by
    /// every client must land, in both byte orders.
    #[test]
    fn oneways_from_concurrent_clients_all_land() {
        const N: usize = 5;
        const EACH: usize = 4;
        for endian in [Endian::Big, Endian::Little] {
            let hits = Arc::new(AtomicU64::new(0));
            let served = serving(Counting { hits: Arc::clone(&hits) }, DEFAULT_MAX_CONNECTIONS);
            let gate = Arc::new(Gate::new(N));
            let clients: Vec<_> = (0..N)
                .map(|i| {
                    let mut c = served.client();
                    let gate = Arc::clone(&gate);
                    std::thread::spawn(move || {
                        let why = format!("oneway client {i} {endian:?}");
                        assert!(gate.arrive(DEADLINE), "{why}: clients did not overlap");
                        for id in 0..EACH as u32 {
                            let fire = encode_request(
                                Version::V1_2,
                                endian,
                                id,
                                b"k",
                                "bump",
                                false,
                                |_| {},
                            )
                            .unwrap();
                            c.write_all(&fire).unwrap();
                        }
                        // The fence: its reply cannot precede the oneways
                        // queued ahead of it on this connection.
                        c.write_all(&ping_wire(Version::V1_2, endian, 99)).unwrap();
                        expect_pong(&mut c, 99, &why);
                    })
                })
                .collect();
            for t in clients {
                t.join().unwrap();
            }
            assert_eq!(
                hits.load(Ordering::SeqCst),
                (N * EACH) as u64,
                "{endian:?}: a oneway went missing under concurrency"
            );
            served.shutdown();
        }
    }

    /// A client that writes half a request and vanishes is the ordinary
    /// failure of a killed process. It must cost exactly its own connection.
    #[test]
    fn a_client_that_vanishes_mid_request_does_not_disturb_the_others() {
        let served = serving(Pong, DEFAULT_MAX_CONNECTIONS);
        let mut steady = served.client();
        steady.write_all(&ping_wire(Version::V1_2, Endian::Big, 1)).unwrap();
        expect_pong(&mut steady, 1, "before the casualty");

        // Half a header, then gone — the server is mid-message when the
        // socket dies.
        let truncated = &ping_wire(Version::V1_2, Endian::Big, 7)[..6];
        let mut doomed = served.client();
        doomed.write_all(truncated).unwrap();
        drop(doomed);

        steady.write_all(&ping_wire(Version::V1_2, Endian::Big, 2)).unwrap();
        expect_pong(&mut steady, 2, "a neighbour's half-request must not be felt");

        let mut fresh = served.client();
        fresh.write_all(&ping_wire(Version::V1_2, Endian::Little, 3)).unwrap();
        expect_pong(&mut fresh, 3, "the server still admits new clients afterwards");
        assert!(served.wait_until(|s| s.active() == 2), "the dead connection must be reaped");
        served.shutdown();
    }

    /// A servant that makes an outbound call from inside `dispatch` — the
    /// event channel's shape, and the hazard concurrency could have
    /// resurrected.
    struct Relay {
        target: crate::Ior,
    }

    impl Dispatch for Relay {
        fn dispatch(
            &mut self,
            req: &Request,
            out: &mut Encoder,
        ) -> std::result::Result<(), SystemException> {
            if req.operation != "relay" {
                return Err(SystemException::bad_operation());
            }
            let relayed = (|| -> Result<i32> {
                let mut conn = crate::Connection::connect(&self.target, DEADLINE)?;
                let reply = conn.invoke_nullary("ping")?;
                reply.body()?.get_i32().map_err(Error::Cdr)
            })();
            match relayed {
                Ok(v) => {
                    out.put_i32(v);
                    Ok(())
                }
                Err(e) => {
                    eprintln!("relay failed: {e}");
                    Err(SystemException::unknown_user_exception())
                }
            }
        }
    }

    /// `event_server`'s rule 1 — no lock may be held across an outbound call —
    /// against the new concurrency.
    ///
    /// The delivery side of a channel invokes *out* while the serving side is
    /// answering requests *in*. With one servant behind one mutex, that is
    /// only safe because the mutex belongs to one server: a servant calling a
    /// second server in the same process must complete, under load, from
    /// several clients at once. That is what this asserts; the deadline is
    /// what makes a resurrected deadlock a failure instead of a hung suite.
    #[test]
    fn an_outbound_call_from_inside_dispatch_does_not_deadlock() {
        const N: usize = 4;
        let inner = serving(Pong, DEFAULT_MAX_CONNECTIONS);
        let outer = serving(Relay { target: ior_at(inner.addr, b"k") }, DEFAULT_MAX_CONNECTIONS);
        let gate = Arc::new(Gate::new(N));
        let clients: Vec<_> = (0..N)
            .map(|i| {
                let mut c = outer.client();
                let gate = Arc::clone(&gate);
                std::thread::spawn(move || {
                    assert!(gate.arrive(DEADLINE), "relay client {i} never overlapped");
                    let wire =
                        encode_request(Version::V1_2, Endian::Big, 1, b"k", "relay", true, |_| {})
                            .unwrap();
                    c.write_all(&wire).unwrap();
                    let msg = read_message(&mut c, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
                    let reply = crate::decode_reply(msg).unwrap();
                    assert_eq!(reply.status, ReplyStatus::NoException, "relay {i}");
                    assert_eq!(reply.body().unwrap().get_i32().unwrap(), 42, "relay {i}");
                })
            })
            .collect();
        for t in clients {
            t.join().unwrap();
        }
        outer.shutdown();
        inner.shutdown();
    }

    /// A servant whose outbound call comes back to its own server.
    struct SelfCaller {
        own: crate::Ior,
        timeout: Duration,
    }

    impl Dispatch for SelfCaller {
        fn dispatch(
            &mut self,
            req: &Request,
            out: &mut Encoder,
        ) -> std::result::Result<(), SystemException> {
            match req.operation.as_str() {
                "ping" => {
                    out.put_i32(42);
                    Ok(())
                }
                "call_myself" => {
                    let attempt = (|| -> Result<()> {
                        let mut conn = crate::Connection::connect(&self.own, self.timeout)?;
                        conn.invoke_nullary("ping")?;
                        Ok(())
                    })();
                    // The re-entrant call cannot succeed: this dispatch holds
                    // the servant lock its own request would need.
                    out.put_bool(attempt.is_err());
                    Ok(())
                }
                _ => Err(SystemException::bad_operation()),
            }
        }
    }

    /// The re-entrancy one-servant-behind-one-mutex forbids, stated as a test
    /// so it cannot be quietly believed to work.
    ///
    /// A servant calling back into its *own* server from inside `dispatch`
    /// waits for a lock its own caller holds. What must be true is that this
    /// fails on the caller's timeout and leaves the server serving — a
    /// bounded failure, not a wedged process. The channel's outbound pushes
    /// go to *other* servers, which is the case above and is unaffected.
    #[test]
    fn a_servant_calling_its_own_server_fails_by_timeout_without_wedging_it() {
        let server = Server::bind("127.0.0.1:0", b"k".to_vec()).unwrap();
        let addr = server.local_addr().unwrap();
        let stats = server.stats();
        let mut servant =
            SelfCaller { own: ior_at(addr, b"k"), timeout: Duration::from_millis(300) };
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            server.serve(&mut servant, move || flag.load(Ordering::SeqCst)).unwrap();
        });

        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(DEADLINE)).unwrap();
        let wire = encode_request(Version::V1_2, Endian::Big, 1, b"k", "call_myself", true, |_| {})
            .unwrap();
        c.write_all(&wire).unwrap();
        let msg = read_message(&mut c, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
        let reply = crate::decode_reply(msg).unwrap();
        assert_eq!(reply.status, ReplyStatus::NoException);
        assert!(
            reply.body().unwrap().get_bool().unwrap(),
            "a re-entrant self-call must fail, not succeed"
        );

        // And the server is still a server.
        let mut after = TcpStream::connect(addr).unwrap();
        after.set_read_timeout(Some(DEADLINE)).unwrap();
        after.write_all(&ping_wire(Version::V1_2, Endian::Big, 2)).unwrap();
        expect_pong(&mut after, 2, "the server survives a refused re-entrant call");
        assert!(stats.accepted() >= 2);

        drop((c, after));
        stop.store(true, Ordering::SeqCst);
        thread.join().unwrap();
    }

    /// Shutdown must not depend on the clients cooperating.
    ///
    /// A connection that is open and idle used to hold the serving thread
    /// inside a blocking read until its peer said something. `serve` now
    /// returns — having said goodbye — while that client is still connected,
    /// and it returns only after every thread it spawned has ended.
    #[test]
    fn a_stopped_server_ends_its_threads_while_a_client_is_still_connected() {
        let mut served = serving(Pong, DEFAULT_MAX_CONNECTIONS);
        let mut idle = served.client();
        idle.write_all(&ping_wire(Version::V1_2, Endian::Big, 1)).unwrap();
        expect_pong(&mut idle, 1, "before the stop");

        served.stop.store(true, Ordering::SeqCst);
        let thread = served.thread.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            thread.join().unwrap();
            let _ = tx.send(());
        });
        assert!(
            rx.recv_timeout(DEADLINE).is_ok(),
            "serve must return with an idle client still connected"
        );

        let bye = read_message(&mut idle, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
        assert_eq!(bye.msg_type, MsgType::CloseConnection, "the idle client is told, not dropped");
        assert_eq!(served.stats.active(), 0, "no connection thread outlived serve");
    }

    #[test]
    fn ior_emission_round_trips_through_our_parser() {
        let ior = crate::Ior {
            type_id: "IDL:spike/Echo:1.0".into(),
            profiles: vec![crate::IiopProfile {
                version: Version::V1_2,
                host: "127.0.0.1".into(),
                port: 4001,
                object_key: b"servant".to_vec(),
                components: Vec::new(),
            }],
        };
        let s = ior.to_stringified().unwrap();
        assert!(s.starts_with("IOR:"));
        assert_eq!(crate::Ior::parse(&s).unwrap(), ior);
    }
}
