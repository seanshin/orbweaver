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
//! ## How the servant is shared
//!
//! **One servant, shared by reference, with a lock of its own or none at
//! all.** [`SharedDispatch`] is `&self`-shaped: [`Server::serve_shared`] hands
//! every connection thread the same `&D` and calls into it without taking
//! anything, so two calls to one servant proceed **concurrently**. What each
//! servant does about that is the servant's decision, argued in its own
//! module: the IFR facade is read-only by policy and locks nothing at all; the
//! naming tree and the tenant graph take an [`RwLock`](crate::guarded::Guarded)
//! whose read half is the whole read-only surface; the event channel already
//! had a mutex it shares with its delivery thread. There is no one answer here
//! because the five servants do not have one sharing shape.
//!
//! That is the limit stream E left in place, and this is where it is lifted.
//! Until this batch, one servant sat behind one mutex taken per message: ten
//! clients could hold sessions, but a servant that blocked for a second
//! blocked all ten for that second. Connections were concurrent; dispatch was
//! not.
//!
//! ## The lock discipline, and why it is not a convention
//!
//! Concurrency inside a servant is where deadlock comes from, and the hazard
//! this workspace already met — `event_server` pushing outbound while it
//! serves inbound — gets *easier* to hit, not harder, when two calls run at
//! once. So the rule is enforced rather than written down:
//! [`crate::guarded`] counts open lock sections per thread, refuses a second
//! one, and the outbound client path refuses to block while one is open. Read
//! that module before adding a lock to a servant.
//!
//! ## The compatibility path, which still serializes
//!
//! [`Dispatch`] — the `&mut self` trait every generated skeleton implements —
//! has not moved. [`Server::serve`] still takes `&mut D` and still wraps it in
//! [`Serialized`], a mutex taken per message, so those servants behave exactly
//! as they did: **dispatch serialized, connections concurrent**. It is a
//! compatibility path and it is honest about being one; a servant that wants
//! the concurrency implements [`SharedDispatch`].
//!
//! One re-entrancy is forbidden by *that* path and not by the other: a
//! `Dispatch` servant that, from inside `dispatch`, calls back into its own
//! server waits for the [`Serialized`] mutex its own caller holds. It does not
//! wedge the server — the inner call is bounded by the client read timeout
//! [`crate::Connection`] sets, fails, and serving continues — but it cannot
//! succeed. A [`SharedDispatch`] servant holding no lock has no such limit,
//! which is asserted rather than assumed. Calling *another* server in the same
//! process, which is what the event channel's delivery loop does, is fine on
//! both paths and is proved by test.
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

/// Repository ID for an operation name no interface of this object declares.
///
/// Not the same as "we do not implement it" — that is [`NO_IMPLEMENT`], and
/// keeping the two apart is what lets a client tell a decision from a gap
/// without reading a document.
pub const BAD_OPERATION: &str = "IDL:omg.org/CORBA/BAD_OPERATION:1.0";
/// Repository id of `CORBA::NO_IMPLEMENT`.
pub const NO_IMPLEMENT: &str = "IDL:omg.org/CORBA/NO_IMPLEMENT:1.0";
/// Repository ID for a malformed or undecodable request body.
pub const MARSHAL: &str = "IDL:omg.org/CORBA/MARSHAL:1.0";
/// Repository ID for an object key we do not recognise.
pub const OBJECT_NOT_EXIST: &str = "IDL:omg.org/CORBA/OBJECT_NOT_EXIST:1.0";
/// Repository ID for a failure with no more precise description — including a
/// user exception reaching a caller that cannot carry one.
pub const UNKNOWN: &str = "IDL:omg.org/CORBA/UNKNOWN:1.0";
/// Repository ID for a servant invariant that did not hold. Never a statement
/// about the caller's request: it says the servant found its own state
/// inconsistent, which is a defect here rather than something to retry.
pub const INTERNAL: &str = "IDL:omg.org/CORBA/INTERNAL:1.0";

/// Whether an operation had run when it failed (§9.4.3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Completion {
    /// The operation ran to completion but the reply was lost.
    ///
    /// Ordinal 0, and the order of this enum is **not** a style choice: the
    /// specification declares `enum completion_status { COMPLETED_YES,
    /// COMPLETED_NO, COMPLETED_MAYBE }`, so YES is zero. We had NO at zero,
    /// which meant a servant reporting "it did not run" reached every foreign
    /// ORB as "it ran" — the two values that decide whether a caller may
    /// safely retry, transposed. MAYBE is 2 either way, which is why nothing
    /// caught it: our own client compared against the same enum and agreed
    /// with itself. Measured against omniORB before the fix
    /// (`CORBA.COMPLETED_YES` is 0) rather than read off the specification
    /// alone.
    Yes = 0,
    /// The operation did not run, so a retry is safe.
    No = 1,
    /// Cannot be determined; a retry may or may not duplicate the effect.
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

    /// A `NO_IMPLEMENT` for an operation this servant knows about and has
    /// decided not to implement.
    ///
    /// The difference from [`Self::bad_operation`] is the whole point and it
    /// is visible on the wire: `BAD_OPERATION` says *no such operation*, which
    /// is what an oversight and a decision both used to say, so the only thing
    /// separating them was a sentence in a document the client cannot read.
    /// `NO_IMPLEMENT` says *the operation exists in the contract and this
    /// servant does not implement it*, on purpose. `orbweaver-registry`'s IFR
    /// facade found this first; `SERVICES-COVERAGE.md` is what made it a rule.
    pub fn no_implement() -> Self {
        Self { id: NO_IMPLEMENT.into(), minor: 0, completed: Completion::No }
    }

    /// A `MARSHAL` for a body we could not decode.
    pub fn marshal() -> Self {
        Self { id: MARSHAL.into(), minor: 0, completed: Completion::No }
    }

    /// An `OBJECT_NOT_EXIST` for an unrecognised object key.
    pub fn object_not_exist() -> Self {
        Self { id: OBJECT_NOT_EXIST.into(), minor: 0, completed: Completion::No }
    }

    /// An `INTERNAL` for a servant invariant that did not hold. `Completion::No`
    /// because a servant that noticed its own inconsistency stopped before
    /// changing anything.
    pub fn internal() -> Self {
        Self { id: INTERNAL.into(), minor: 0, completed: Completion::No }
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
    contexts: Vec<crate::ServiceContext>,
    raw: Vec<u8>,
    body_at: usize,
}

impl Request {
    /// A decoder positioned at the first argument, reading text in the
    /// codeset the **client declared** (D009 §7.1).
    ///
    /// `None` — no `CodeSets` context — is ISO-8859-1 by §7.10.2.5 and not
    /// UTF-8, but this project's streams default to UTF-8 and every peer here
    /// reaches it, so a missing context is left as the stream's default rather
    /// than silently reinterpreting every existing caller's bytes. That gap is
    /// named in `codeset.rs`'s module docs; closing it changes behaviour
    /// against every component-less peer in existence, on a question no peer
    /// here can settle.
    pub fn body(&self) -> Result<Decoder<'_>> {
        let mut d = Decoder::new(&self.raw, self.endian).with_codec(self.narrow_codec());
        d.seek_to(self.body_at)?;
        Ok(d)
    }

    /// The narrow-text codec this request's own `CodeSets` context asks for.
    ///
    /// Derived from the request rather than from the connection, because a
    /// servant answers the caller in front of it: two clients on one
    /// multiplexed connection can have declared different things.
    pub fn narrow_codec(&self) -> Option<std::sync::Arc<dyn orbweaver_cdr::TextCodec>> {
        let cs = self.code_sets()?;
        let id = cs.char_data;
        if id == crate::codeset::CodeSetId::UTF_8 {
            return None;
        }
        crate::codeset::Converter::new(id)
            .ok()
            .map(|c| std::sync::Arc::new(c) as std::sync::Arc<dyn orbweaver_cdr::TextCodec>)
    }

    /// Every `IOP::ServiceContext` the peer attached, in the order it sent
    /// them.
    ///
    /// These were parsed and dropped on the floor until this batch, which made
    /// the codeset half of §7.10.2 a one-way street: we published what we could
    /// read and never looked at what the client said it was sending.
    pub fn service_contexts(&self) -> &[crate::ServiceContext] {
        &self.contexts
    }

    /// The `CodeSets` context (§7.10.2.5), if the peer sent a readable one.
    ///
    /// `None` is not "no opinion". §7.10.2.5 is explicit that *"if no char
    /// transmission code set is specified in the code set service context, then
    /// the char transmission code set is considered to be ISO 8859-1 for
    /// backward compatibility"* — so an absent context is a peer declaring
    /// Latin-1, which is what omniORB does when the reference it dialed carried
    /// no `TAG_CODE_SETS` (measured; see [`crate::codeset::server_component`]).
    /// A servant that cares about text above ASCII must treat `None` as
    /// ISO-8859-1 rather than as UTF-8.
    ///
    /// A malformed body reads as `None` for the reason §9.7.2 gives about
    /// components generally: what cannot be understood is not thereby fatal.
    pub fn code_sets(&self) -> Option<crate::codeset::CodeSetContext> {
        self.contexts
            .iter()
            .find(|c| c.id == crate::codeset::SERVICE_ID_CODE_SETS)
            .and_then(|c| crate::codeset::CodeSetContext::parse(&c.data).ok())
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
    let contexts;

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
        contexts = read_service_contexts(&mut d)?;
    } else {
        contexts = read_service_contexts(&mut d)?;
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

    Ok(Request {
        version,
        endian,
        request_id,
        object_key,
        operation,
        expect_reply,
        contexts,
        raw,
        body_at,
    })
}

fn read_service_contexts(d: &mut Decoder<'_>) -> Result<Vec<crate::ServiceContext>> {
    let n = d.get_u32()?;
    let n = d.validate_count(n, 8)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let id = d.get_u32()?;
        let data = d.get_octet_seq()?.to_vec();
        out.push(crate::ServiceContext { id, data });
    }
    Ok(out)
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
    codec: Option<std::sync::Arc<dyn orbweaver_cdr::TextCodec>>,
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
    // §7.1: the answer goes back in the codeset the question was asked in.
    // On the body only — the reply header carries no text.
    let mut body = Encoder::continuing_at(endian, body_start).with_codec(codec);
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
    let bytes =
        encode_reply(version, endian, request_id, ReplyStatus::LocationForward, None, |b| {
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
    encode_reply(version, endian, request_id, ReplyStatus::SystemException, None, |b| {
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

/// What a servant does with an invocation, one operation at a time.
///
/// The `&mut self` shape is the compatibility path and the one every
/// generated skeleton implements. [`Server::serve`] wraps it in
/// [`Serialized`] and takes that mutex for the duration of one message, so an
/// implementation needs no locking of its own — and a servant that blocks (a
/// long computation, an outbound call) blocks every other client for as long
/// as it blocks.
///
/// **A servant that wants two calls to run at once implements
/// [`SharedDispatch`] instead**, which is `&self`-shaped and takes nothing.
/// The five servants in this workspace all do; see [`crate::guarded`] for the
/// lock discipline that comes with the privilege.
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

/// Forwards to the servant behind a mutable reference, so a `&mut D` can be
/// handed to anything that wants a `Dispatch`.
///
/// [`Server::serve`] needs it: it takes `&mut D` and puts it in a
/// [`Serialized`], which is a `Dispatch` container.
impl<D: Dispatch + ?Sized> Dispatch for &mut D {
    fn dispatch(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        (**self).dispatch(request, out)
    }

    fn dispatch_body(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<DispatchBody, SystemException> {
        (**self).dispatch_body(request, out)
    }

    fn knows(&self, object_key: &[u8]) -> bool {
        (**self).knows(object_key)
    }

    fn forward(&mut self, request: &Request) -> Option<crate::Ior> {
        (**self).forward(request)
    }
}

/// What a servant does with an invocation when **two of them may be running at
/// once**.
///
/// This is [`Dispatch`] with the exclusivity removed: `&self`, `Sync`, and no
/// lock taken on the servant's behalf. It is the trait that lifts the limit
/// stream E left in place — a slow operation no longer delays a concurrent
/// caller, because there is nothing between the two calls to delay them.
///
/// # What an implementation owes
///
/// 1. **Interior mutability, or none.** A servant with no mutable state (the
///    IFR facade, which refuses every write as policy) implements this with no
///    synchronisation at all. Everything else puts its state behind
///    [`crate::guarded::Guarded`] — or, where a `Condvar` is involved, its own
///    mutex joined to the same discipline.
/// 2. **One lock, taken once per request.** Under [`Dispatch`] the server's
///    mutex made a request one indivisible look at the servant. Keeping that —
///    taking the servant's own lock once, at the top, for the whole operation —
///    is what preserves per-request atomicity. Taking it twice inside one
///    operation is both a torn request and a re-entrant lock, and
///    [`crate::guarded`] refuses the second.
/// 3. **Nothing blocking inside the lock.** See [`crate::guarded`]; the
///    outbound client path enforces it.
///
/// # What is no longer true, and matters
///
/// [`SharedDispatch::knows`] and [`SharedDispatch::dispatch_body`] are two
/// separate looks at the servant, where the [`Serialized`] path made them one.
/// A key can therefore be retired between them, so the "unreachable"
/// `OBJECT_NOT_EXIST` arm inside a servant's dispatch is now genuinely
/// reachable — which is why every servant here has one instead of an `expect`.
pub trait SharedDispatch: Sync {
    /// Handles `request`, writing the reply body into `out`.
    ///
    /// Returning `Err` produces a system-exception reply. Returning `Ok` with
    /// nothing written produces an empty `NO_EXCEPTION` reply, which is what a
    /// `void` operation looks like.
    fn dispatch(
        &self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException>;

    /// As [`SharedDispatch::dispatch`], but able to label what it wrote a user
    /// exception. This is the method [`Server`] actually calls.
    fn dispatch_body(
        &self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<DispatchBody, SystemException> {
        self.dispatch(request, out).map(|()| DispatchBody::Return)
    }

    /// Whether this servant answers to `object_key`.
    fn knows(&self, _object_key: &[u8]) -> bool {
        true
    }

    /// A reference to redirect this request to, instead of serving it.
    fn forward(&self, _request: &Request) -> Option<crate::Ior> {
        None
    }

    /// One whole request — the method [`Server`] calls, and the unit of
    /// atomicity.
    ///
    /// The default composes the three above, which is right for a servant
    /// whose `knows` is a key comparison and whose dispatch re-checks what it
    /// addresses (every servant in this workspace does, deliberately: see the
    /// trait docs on the arm that stopped being unreachable). A servant that
    /// needs `knows` and `dispatch` to see the *same* state overrides this and
    /// takes its lock once around both — which is exactly what
    /// [`Serialized`] does, and why the compatibility path still gives one
    /// request one indivisible look at the servant.
    fn serve_one(
        &self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<Served, SystemException> {
        if !self.knows(&request.object_key) {
            return Ok(Served::UnknownObject);
        }
        if let Some(to) = self.forward(request) {
            return Ok(Served::Forward(to));
        }
        self.dispatch_body(request, out).map(Served::Body)
    }
}

/// What a servant did with one request, before the reply status is chosen.
#[derive(Debug)]
pub enum Served {
    /// A body was written; this says which reply status it travels under.
    Body(DispatchBody),
    /// Not answered here: redirect the caller (§9.4.3.2).
    Forward(crate::Ior),
    /// The servant does not answer to this object key.
    UnknownObject,
}

/// A [`Dispatch`] servant made shareable by serializing it — the
/// compatibility path, and the whole of what the previous batch had.
///
/// One mutex, taken per message, around a servant that still sees `&mut self`.
/// Connections overlap; operations do not. It is deliberately *not* joined to
/// [`crate::guarded`]'s discipline: a `Dispatch` servant calling out from
/// inside `dispatch` is holding this mutex by construction, that is the
/// documented shape of the path, and a tripwire that fires on every legitimate
/// use of a compatibility path is a tripwire people learn to ignore.
#[derive(Debug, Default)]
pub struct Serialized<D> {
    servant: Mutex<D>,
}

impl<D: Dispatch> Serialized<D> {
    /// Wraps `servant`, which will answer one call at a time.
    pub fn new(servant: D) -> Self {
        Serialized { servant: Mutex::new(servant) }
    }

    /// The servant back, once nothing is serving it.
    pub fn into_inner(self) -> D {
        self.servant.into_inner().unwrap_or_else(|e| e.into_inner())
    }
}

impl<D: Dispatch + Send> SharedDispatch for Serialized<D> {
    fn dispatch(
        &self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        lock(&self.servant).dispatch(request, out)
    }

    fn dispatch_body(
        &self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<DispatchBody, SystemException> {
        lock(&self.servant).dispatch_body(request, out)
    }

    fn knows(&self, object_key: &[u8]) -> bool {
        lock(&self.servant).knows(object_key)
    }

    fn forward(&self, request: &Request) -> Option<crate::Ior> {
        lock(&self.servant).forward(request)
    }

    /// The lock spans knows/forward/dispatch — one request is one indivisible
    /// look at the servant, which is what this path has always given and what
    /// composing the three separately would have quietly taken away.
    fn serve_one(
        &self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<Served, SystemException> {
        let mut servant = lock(&self.servant);
        if !servant.knows(&request.object_key) {
            return Ok(Served::UnknownObject);
        }
        if let Some(to) = servant.forward(request) {
            return Ok(Served::Forward(to));
        }
        servant.dispatch_body(request, out).map(Served::Body)
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
    dispatching: AtomicU64,
    peak_dispatching: AtomicU64,
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

    /// Requests that have reached the servant and not yet returned.
    ///
    /// **Read the boundary carefully, because it is not what the name might
    /// suggest.** This counts requests inside
    /// [`SharedDispatch::serve_one`] — which, for a servant that takes a lock
    /// of its own, includes the ones *waiting* for that lock. Under
    /// [`Serialized`] all N callers are counted here while exactly one of them
    /// is executing.
    ///
    /// That was measured, not assumed: the first version of this counter was
    /// named for concurrent dispatch and asserted on as proof of it, and the
    /// negative control refuted it by reaching N on a serialized server. A
    /// counter outside the servant's lock cannot tell overlap from queueing.
    /// What it *is* good for is queue depth — how many callers are piled up at
    /// a servant — which is the number a slow servant makes interesting.
    ///
    /// The witness for real overlap has to be **inside** the servant: a
    /// rendezvous that cannot complete unless N calls are executing together,
    /// or a counter the servant itself keeps past its own lock. The tests use
    /// both; see `a_blocking_operation_no_longer_delays_a_concurrent_caller`.
    pub fn at_servant(&self) -> u64 {
        self.counters.dispatching.load(Ordering::Relaxed)
    }

    /// The high-water mark of [`ServerStats::at_servant`] — peak queue depth
    /// at the servant, with the caveat spelled out there.
    pub fn peak_at_servant(&self) -> u64 {
        self.counters.peak_dispatching.load(Ordering::Relaxed)
    }

    /// Counts one request into the servant, and out again on drop — including
    /// the drop an unwinding panic performs.
    fn dispatching_now(&self) -> Dispatching<'_> {
        let n = self.counters.dispatching.fetch_add(1, Ordering::AcqRel) + 1;
        self.counters.peak_dispatching.fetch_max(n, Ordering::Relaxed);
        Dispatching { counters: &self.counters }
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

/// One request inside the servant, counted out on drop.
#[derive(Debug)]
struct Dispatching<'a> {
    counters: &'a Counters,
}

impl Drop for Dispatching<'_> {
    fn drop(&mut self) {
        self.counters.dispatching.fetch_sub(1, Ordering::AcqRel);
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
                // §7.10.2.4: an IIOP profile with no `TAG_CODE_SETS` says the
                // server is ISO-8859-1 for `char` and has **no `wchar`
                // support at all**, so a conformant client refuses to marshal
                // a `wstring` to it rather than calling. Publishing what we
                // actually speak is what makes those operations reachable.
                components: vec![crate::codeset::server_component()],
            }],
        })
    }

    /// Builds a publishable IOR by running the **bound** address through
    /// `map` — endpoint rewriting at publish time (PLAN R7).
    ///
    /// This is the rewrite this project prefers, and the reason is who reads
    /// the result: a reference the server hands out is read by every client,
    /// including foreign ORBs nobody here can patch. See
    /// [`crate::nat`] and `docs/PHASE6.md`.
    ///
    /// Two refusals, both deliberate:
    ///
    /// - A **wildcard bind** (`0.0.0.0`, `::`) that no rule names is an error,
    ///   not a default. `0.0.0.0` is bindable and unpublishable, and an ORB
    ///   that publishes it produces references that fail at every client
    ///   rather than at the one process that could have been configured.
    /// - An address that is not a wildcard and that no rule names is
    ///   published unchanged. A deployment with no NAT in front of it sets no
    ///   map, and must still get a working reference.
    pub fn ior_mapped(&self, type_id: &str, map: &crate::nat::EndpointMap) -> Result<crate::Ior> {
        let bound = self.local_addr()?;
        let (host, port) = match crate::nat::published_address(bound, map) {
            Some(ep) => ep,
            None if crate::nat::is_unpublishable(bound.ip()) => {
                return Err(crate::Error::BadIor(
                    "bound to a wildcard address and no rule publishes it; \
                     an IOR must name an address a client can dial",
                ));
            }
            None => (bound.ip().to_string(), bound.port()),
        };
        self.ior(type_id, &host).map(|mut ior| {
            // `ior` took the bound port; a rule may have moved it.
            if let Some(p) = ior.profiles.first_mut() {
                p.port = port;
            }
            ior
        })
    }

    /// Serves connections concurrently, **serializing dispatch**, until `stop`
    /// returns true.
    ///
    /// The compatibility path: `dispatch` is `&mut self`-shaped, so it goes
    /// behind a [`Serialized`] mutex taken per message and connections overlap
    /// while operations do not. A servant that wants two calls at once
    /// implements [`SharedDispatch`] and is served by
    /// [`Server::serve_shared`]; everything else about the two is identical,
    /// because this one is written in terms of that one.
    pub fn serve<D, S>(&self, dispatch: &mut D, stop: S) -> Result<()>
    where
        D: Dispatch + Send,
        S: Fn() -> bool + Sync,
    {
        self.serve_shared(&Serialized::new(dispatch), stop)
    }

    /// Serves connections concurrently until `stop` returns true, with
    /// **dispatch concurrent too**.
    ///
    /// Each accepted connection gets a thread and every one of them calls
    /// straight into `dispatch`, which takes whatever lock it needs (or none)
    /// for itself — so a servant that blocks no longer delays the callers it
    /// is not blocking on. Over [`Server::max_connections`] a connection is
    /// refused with a `CloseConnection` and counted in
    /// [`ServerStats::refused`]; the overlap inside the servant is counted in
    /// [`ServerStats::peak_dispatching`].
    ///
    /// `stop` is polled by the accept loop and by every connection thread at
    /// [`STOP_POLL`]. A stop that lands mid-connection ends it with an
    /// orderly `CloseConnection` (§9.4.10) rather than a bare TCP close, so
    /// the peer knows its unanswered requests were not processed and may
    /// re-send them elsewhere. `serve_shared` returns only once every
    /// connection thread has ended, so nothing it started outlives it.
    ///
    /// A servant that panics still ends the server: the panic surfaces here
    /// when the scope joins. What does *not* happen is the other connections
    /// dying with it — a poisoned lock is recovered rather than propagated,
    /// because one bad request must not take the service down.
    pub fn serve_shared<D, S>(&self, dispatch: &D, stop: S) -> Result<()>
    where
        D: SharedDispatch,
        S: Fn() -> bool + Sync,
    {
        let servant: &dyn SharedDispatch = dispatch;
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
    pub fn serve_connection<D: Dispatch + Send>(&self, s: TcpStream, d: &mut D) -> Result<()> {
        self.serve_connection_until(s, &Serialized::new(d), &|| false)
    }

    /// As [`Server::serve_connection`], on the shared servant, ending with an
    /// orderly `CloseConnection` when `stop` reports true between messages.
    fn serve_connection_until(
        &self,
        mut s: TcpStream,
        servant: &dyn SharedDispatch,
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
                Err(Error::InterruptedMidReassembly {
                    control: MsgType::CloseConnection, ..
                }) => {
                    // A client that says goodbye between the fragments of its
                    // own request is doing the orderly thing, not a broken one,
                    // and this loop already treats a `CloseConnection` between
                    // whole messages as the end of the conversation. Where the
                    // message lands in the stream cannot be what decides
                    // whether it is a protocol error: answering the goodbye
                    // with a §9.4.8 `MessageError` and logging a fault would
                    // put a phantom failure in the server's own record every
                    // time a peer shut down mid-upload.
                    return Ok(());
                }
                Err(e @ Error::InterruptedMidReassembly { .. }) => {
                    // The other one: a `MessageError` interrupted the request.
                    // The peer is telling us it could not parse something we
                    // sent, so it is already giving up; answering a
                    // `MessageError` with a `MessageError` is the one reply
                    // guaranteed to be useless, and between two ORBs that both
                    // do it, it is a loop.
                    return Err(e);
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
                    let status = if servant.knows(&lr.object_key) {
                        LocateStatus::ObjectHere
                    } else {
                        LocateStatus::UnknownObject
                    };
                    let out = encode_locate_reply(lr.version, lr.endian, lr.request_id, status)?;
                    s.write_all(&out)?;
                }
                MsgType::Request => {
                    let req = decode_request(msg)?;
                    // The servant is entered here and left before the reply is
                    // written, so a slow *socket* holds nobody up. Whether a
                    // slow servant does is now the servant's own answer, not
                    // this loop's: `serve_one` is what takes a lock, if it
                    // takes one at all.
                    let reply = self.handle_request(&req, servant)?;
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
                MsgType::MessageError => {
                    // §9.4.8 is a report about something *we* sent, so there is
                    // nothing here to answer and the conversation is over. It
                    // is still a fault worth returning — a peer that cannot
                    // parse our replies is a real interop failure — but it is
                    // reported without sending a `MessageError` back, for the
                    // same reason as the mid-fragment case above: two ORBs that
                    // both answer one with another never stop.
                    return Err(Error::UnexpectedMessage(MsgType::MessageError));
                }
                other => {
                    let _ = encode_message_error(msg.endian)
                        .and_then(|m| s.write_all(&m).map_err(Error::Io));
                    return Err(Error::UnexpectedMessage(other));
                }
            }
        }
    }

    fn handle_request(&self, req: &Request, d: &dyn SharedDispatch) -> Result<Option<Vec<u8>>> {
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
        // Counted around the servant call and nothing else, so the number
        // excludes the framing either side of it. It does *not* exclude a
        // servant's own lock — see `ServerStats::at_servant` for why that
        // distinction cost a test.
        let served = {
            let _inside = self.stats.dispatching_now();
            d.serve_one(req, &mut out)
        };
        match served {
            Ok(Served::UnknownObject) => {
                self.reply_exception(req, &SystemException::object_not_exist())
            }
            Ok(Served::Forward(to)) => {
                if !req.expect_reply {
                    // A oneway cannot be redirected; there is no reply to carry
                    // the new address, and inventing one would be worse than
                    // serving it.
                    return Ok(None);
                }
                Ok(Some(encode_location_forward(req.version, req.endian, req.request_id, &to)?))
            }
            Ok(Served::Body(kind)) => {
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
                Ok(Some(encode_reply(
                    req.version,
                    req.endian,
                    req.request_id,
                    status,
                    req.narrow_codec(),
                    |e| e.put_bytes(&body),
                )?))
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
                let wire = encode_reply(version, endian, 88, ReplyStatus::NoException, None, |e| {
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
        // The literal ordinal, not `Completion::No as u32`. Comparing the
        // symbol against itself is what let the enum sit transposed —
        // COMPLETED_NO written as 0 — through every local test: both sides of
        // the assertion moved together, and only an ORB we did not write could
        // disagree. §4.11.4 fixes COMPLETED_YES at 0, so NO is 1.
        assert_eq!(b.get_u32().unwrap(), 1, "COMPLETED_NO is §4.11.4's ordinal 1");
        assert_eq!(Completion::Yes as u32, 0);
        assert_eq!(Completion::Maybe as u32, 2);
    }

    /// A 1.2-only status must not be emitted to a 1.0/1.1 peer, which has no
    /// enumerator for it.
    #[test]
    fn post_1_2_status_is_refused_on_older_versions() {
        for version in [Version::V1_0, Version::V1_1] {
            assert!(
                encode_reply(
                    version,
                    Endian::Big,
                    1,
                    ReplyStatus::LocationForwardPerm,
                    None,
                    |_| {}
                )
                .is_err(),
                "{version} has no LOCATION_FORWARD_PERM"
            );
        }
        assert!(
            encode_reply(
                Version::V1_2,
                Endian::Big,
                1,
                ReplyStatus::LocationForwardPerm,
                None,
                |_| {}
            )
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
    ///
    /// Both shapes, so the same servant can be served concurrently
    /// ([`Server::serve_shared`]) or serialized ([`Server::serve`]) and the
    /// tests compare like with like.
    struct Pong;

    impl SharedDispatch for Pong {
        fn dispatch(
            &self,
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

    impl Dispatch for Pong {
        fn dispatch(
            &mut self,
            req: &Request,
            out: &mut Encoder,
        ) -> std::result::Result<(), SystemException> {
            SharedDispatch::dispatch(self, req, out)
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

    /// The serving side of the collision §9.4.9 and §13.5.1 create: a client
    /// that says goodbye between the fragments of its own request is shutting
    /// down, not misbehaving.
    ///
    /// Where the message lands in the stream cannot be what decides whether it
    /// is a protocol error — this loop already ends quietly on a
    /// `CloseConnection` between whole messages. Before the reader told the two
    /// apart, the same goodbye one fragment earlier produced a §9.4.8
    /// `MessageError` aimed at a peer that had stopped listening, and a
    /// serving error in our own record for a peer that did everything right.
    #[test]
    fn a_goodbye_between_the_fragments_of_a_request_ends_the_connection_quietly() {
        let server = Server::bind("127.0.0.1:0", b"k".to_vec()).unwrap();
        let addr = server.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            let (s, _) = server.listener.accept().unwrap();
            server.serve_connection(s, &mut Pong)
        });

        let mut c = TcpStream::connect(addr).unwrap();
        let pieces = crate::fragment_message(ping_wire(Version::V1_2, Endian::Big, 1), 24).unwrap();
        assert!(pieces.len() > 1, "the test needs a genuinely fragmented request");
        c.write_all(&pieces[0]).unwrap();
        c.write_all(&encode_close_connection(Version::V1_2, Endian::Big).unwrap()).unwrap();
        c.flush().unwrap();

        // Nothing may come back — not a reply to a request that never
        // completed, and above all not a MessageError at a peer that has
        // already said goodbye.
        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut back = [0u8; 12];
        match std::io::Read::read(&mut c, &mut back) {
            Ok(0) => {}
            Ok(n) => panic!("the server answered an orderly goodbye with {:?}", &back[..n]),
            Err(e) => panic!("expected a clean EOF, got {e}"),
        }
        t.join().unwrap().expect("an orderly goodbye is not a serving error");
    }

    /// §9.4.8 is a report about something *we* sent, so there is nothing to
    /// answer. Answering it with another `MessageError` is the one reply
    /// guaranteed to be useless — and between two ORBs that both do it, it does
    /// not stop.
    #[test]
    fn a_message_error_is_not_answered_with_another_message_error() {
        let server = Server::bind("127.0.0.1:0", b"k".to_vec()).unwrap();
        let addr = server.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            let (s, _) = server.listener.accept().unwrap();
            server.serve_connection(s, &mut Pong)
        });

        let mut c = TcpStream::connect(addr).unwrap();
        c.write_all(&encode_message_error(Endian::Big).unwrap()).unwrap();
        c.flush().unwrap();

        c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut back = [0u8; 12];
        match std::io::Read::read(&mut c, &mut back) {
            Ok(0) => {}
            Ok(n) => panic!("a MessageError must not be answered, got {:?}", &back[..n]),
            Err(e) => panic!("expected a clean EOF, got {e}"),
        }
        // Still a fault worth reporting: a peer that cannot parse what we send
        // is a real interop failure, it is just not one another header helps.
        match t.join().unwrap() {
            Err(Error::UnexpectedMessage(MsgType::MessageError)) => {}
            other => panic!("expected the report to surface, got {other:?}"),
        }
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

    /// Publish-time rewriting, the R7 mitigation at the point addresses enter
    /// the wire: the bound address goes through the map, the object key does
    /// not.
    #[test]
    fn ior_mapped_publishes_the_mapped_address_and_nothing_else() {
        use crate::nat::{EndpointMap, Rule};
        let server = Server::bind("127.0.0.1:0", b"servant-identity".to_vec()).unwrap();
        let port = server.local_addr().unwrap().port();

        let plain = server.ior_mapped("IDL:spike/Echo:1.0", &EndpointMap::new()).unwrap();
        assert_eq!(plain.profiles[0].host, "127.0.0.1", "no map means publish what was bound");
        assert_eq!(plain.profiles[0].port, port);

        let map = EndpointMap::new().with(Rule::endpoint("127.0.0.1", port, "203.0.113.9", 31000));
        let mapped = server.ior_mapped("IDL:spike/Echo:1.0", &map).unwrap();
        assert_eq!(mapped.profiles[0].host, "203.0.113.9");
        assert_eq!(mapped.profiles[0].port, 31000);
        assert_eq!(
            mapped.profiles[0].object_key, b"servant-identity",
            "identity is not an address"
        );
        assert_eq!(mapped.profiles[0].version, plain.profiles[0].version);
    }

    /// `0.0.0.0` is bindable and unpublishable. Publishing it would produce a
    /// reference that fails at every client instead of at the one process that
    /// could still be configured.
    #[test]
    fn ior_mapped_refuses_to_publish_a_wildcard_bind() {
        use crate::nat::{EndpointMap, Rule};
        let server = Server::bind("0.0.0.0:0", b"k".to_vec()).unwrap();
        let port = server.local_addr().unwrap().port();
        assert!(matches!(
            server.ior_mapped("IDL:spike/Echo:1.0", &EndpointMap::new()),
            Err(crate::Error::BadIor(_))
        ));
        let map = EndpointMap::new().with(Rule::host("0.0.0.0", "203.0.113.9"));
        let ior = server.ior_mapped("IDL:spike/Echo:1.0", &map).unwrap();
        assert_eq!(ior.profiles[0].host, "203.0.113.9");
        assert_eq!(ior.profiles[0].port, port, "a host-only rule keeps the bound port");
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
    /// answering requests *in*. This is that shape on the **serialized** path,
    /// where it is only safe because the mutex belongs to one server: a
    /// servant calling a second server in the same process must complete,
    /// under load, from several clients at once.
    /// `an_outbound_call_with_other_calls_in_flight_does_not_deadlock` is the
    /// same claim on the shared path, and holds the calls open at a gate so
    /// they are provably simultaneous rather than merely concurrent. Both
    /// deadlines are what make a resurrected deadlock a failure instead of a
    /// hung suite.
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

    // ── concurrent dispatch ──────────────────────────────────────────────────
    //
    // The limit stream E left in place, and the tests that say it is gone.
    // Every one of them bounds itself with a deadline, and every one asserts
    // against a rendezvous or the server's own counters — never against a
    // clock. A timing-based overlap test passes on a serialized server that
    // merely happens to be fast, which makes it not a check at all.

    use crate::guarded::Guarded;

    /// A servant whose operation **blocks for as long as it takes** for `n`
    /// calls to be inside it at once.
    ///
    /// This is the measurable block the batch is about. Under concurrent
    /// dispatch the gate opens and every caller returns `true`; under
    /// serialized dispatch the first caller waits out `within` alone, which is
    /// a failed rendezvous rather than a hung test.
    ///
    /// Note where the gate is waited on: **outside** the `Guarded`. Blocking
    /// inside it would be the discipline violation this module's docs forbid,
    /// and `guarded`'s tripwire would say so.
    struct Rendezvous {
        gate: Gate,
        within: Duration,
        /// The witness that counts, kept **by the servant**, past whatever
        /// lock it takes. `ServerStats::peak_at_servant` cannot do this job:
        /// it counts callers waiting for a servant's lock as well as the one
        /// holding it, so it reaches N on a serialized server too. Measured,
        /// not reasoned about — that is how this field came to exist.
        inside: AtomicU64,
        peak_inside: AtomicU64,
        calls: Guarded<u64>,
    }

    impl Rendezvous {
        fn new(n: usize, within: Duration) -> Self {
            Rendezvous {
                gate: Gate::new(n),
                within,
                inside: AtomicU64::new(0),
                peak_inside: AtomicU64::new(0),
                calls: Guarded::new("a test servant", 0),
            }
        }

        fn peak_inside(&self) -> u64 {
            self.peak_inside.load(Ordering::SeqCst)
        }
    }

    impl SharedDispatch for Rendezvous {
        fn dispatch(
            &self,
            req: &Request,
            out: &mut Encoder,
        ) -> std::result::Result<(), SystemException> {
            match req.operation.as_str() {
                "slow" => {
                    let n = self.inside.fetch_add(1, Ordering::SeqCst) + 1;
                    self.peak_inside.fetch_max(n, Ordering::SeqCst);
                    let all_here = self.gate.arrive(self.within);
                    self.calls.write(|c| *c += 1);
                    self.inside.fetch_sub(1, Ordering::SeqCst);
                    out.put_bool(all_here);
                    Ok(())
                }
                _ => Err(SystemException::bad_operation()),
            }
        }
    }

    impl Dispatch for Rendezvous {
        fn dispatch(
            &mut self,
            req: &Request,
            out: &mut Encoder,
        ) -> std::result::Result<(), SystemException> {
            SharedDispatch::dispatch(self, req, out)
        }
    }

    /// A server on loopback serving `servant` **concurrently**, with its
    /// counters readable from the test.
    fn serving_shared<D>(servant: Arc<D>, cap: usize) -> Loopback
    where
        D: SharedDispatch + Send + Sync + 'static,
    {
        let mut server = Server::bind("127.0.0.1:0", b"k".to_vec()).unwrap();
        server.set_max_connections(cap);
        let addr = server.local_addr().unwrap();
        let stats = server.stats();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            server.serve_shared(&*servant, move || flag.load(Ordering::SeqCst)).unwrap();
        });
        Loopback { addr, stats, stop, thread: Some(thread) }
    }

    fn slow_wire(endian: Endian, id: u32) -> Vec<u8> {
        encode_request(Version::V1_2, endian, id, b"k", "slow", true, |_| {}).unwrap()
    }

    /// Fires `n` concurrent `slow` calls and returns how many of them saw the
    /// rendezvous complete.
    ///
    /// Both ways of not overlapping are turned into a *readable* failure. A
    /// serialized server can make a caller miss the gate (it returns `false`
    /// and the count comes up short) or make it miss its socket deadline
    /// entirely, because it spent the whole timeout queued behind somebody
    /// else. The second used to surface as an `unwrap` on a `WouldBlock`,
    /// which is a true failure with a useless message; it says what it means
    /// now. Both are bounded, which is the property that matters: the test
    /// fails, it does not hang.
    fn slow_callers(served: &Loopback, n: usize, endian: Endian) -> usize {
        let clients: Vec<_> = (0..n)
            .map(|i| {
                let mut c = served.client();
                std::thread::spawn(move || {
                    c.write_all(&slow_wire(endian, i as u32)).unwrap();
                    let msg = read_message(&mut c, DEFAULT_MAX_MESSAGE_SIZE).unwrap_or_else(|e| {
                        panic!(
                            "slow caller {i}: no reply within the deadline ({e}) — \
                             the servant was still busy with somebody else, which is \
                             exactly what serialized dispatch looks like from outside"
                        )
                    });
                    let reply = crate::decode_reply(msg).unwrap();
                    assert_eq!(reply.status, ReplyStatus::NoException, "slow caller {i}");
                    reply.body().unwrap().get_bool().unwrap()
                })
            })
            .collect();
        clients
            .into_iter()
            .map(|t| t.join().unwrap_or(false))
            .filter(|overlapped| *overlapped)
            .count()
    }

    /// **The measurement this batch exists for.** A servant whose operation
    /// blocks must no longer delay a concurrent caller.
    ///
    /// N clients each invoke an operation that cannot return until all N are
    /// inside the servant together. Two independent witnesses, neither of them
    /// a clock:
    ///
    /// - the **rendezvous**: every caller reports that all N arrived, which is
    ///   only possible if all N were executing inside `dispatch` at the same
    ///   instant;
    /// - the **servant's own high-water mark**, kept past its lock.
    ///
    /// `ServerStats::peak_at_servant` is deliberately *not* one of them: it
    /// counts callers waiting for a servant's lock too, so it reaches N on a
    /// serialized server as well. That is asserted here as a fact about the
    /// counter rather than left as a trap for the next reader.
    ///
    /// Under the previous design — one servant behind one mutex — the first
    /// caller would hold the mutex while it waited, the others would never
    /// enter, and this fails on the gate's deadline. That is not a
    /// hypothetical: it is
    /// `the_negative_control_serialized_dispatch_still_delays_every_caller`
    /// below, which asserts exactly that outcome.
    #[test]
    fn a_blocking_operation_no_longer_delays_a_concurrent_caller() {
        const N: usize = 5;
        for endian in [Endian::Big, Endian::Little] {
            let servant = Arc::new(Rendezvous::new(N, DEADLINE));
            let served = serving_shared(Arc::clone(&servant), DEFAULT_MAX_CONNECTIONS);
            let overlapped = slow_callers(&served, N, endian);
            assert_eq!(overlapped, N, "{endian:?}: only {overlapped}/{N} callers met at the gate");
            assert_eq!(
                servant.peak_inside(),
                N as u64,
                "{endian:?}: the servant saw at most {} calls executing at once, wanted {N}",
                servant.peak_inside()
            );
            assert_eq!(servant.calls.read(|c| *c), N as u64, "{endian:?}: every call completed");
            assert!(served.stats.peak_at_servant() >= N as u64, "{endian:?}");
            served.shutdown();
        }
    }

    /// **The negative control.** The same servant, the same clients, the same
    /// assertions — served through the path this batch did *not* change.
    ///
    /// [`Server::serve`] wraps a `&mut` servant in [`Serialized`], which is
    /// precisely the design that was here before: one servant, one mutex,
    /// taken per message. So this test is the change reverted, and it must
    /// **fail to overlap** — which it asserts positively, so that the day
    /// somebody makes `Serialized` concurrent this test says so rather than
    /// quietly agreeing.
    ///
    /// What matters as much as the outcome is the *manner* of it: the gate has
    /// a short deadline of its own, so serialization produces a completed test
    /// with a false rendezvous rather than a hung suite. A concurrency test
    /// that hangs when it fails is a test nobody can read.
    #[test]
    fn the_negative_control_serialized_dispatch_still_delays_every_caller() {
        const N: usize = 4;
        // Short, because under serialization every caller waits it out in
        // turn: this bounds the whole test at roughly N × this.
        const GATE: Duration = Duration::from_millis(300);

        let servant = Arc::new(Rendezvous::new(N, GATE));
        let mut serialized = AsDispatch(Arc::clone(&servant));
        let mut server = Server::bind("127.0.0.1:0", b"k".to_vec()).unwrap();
        server.set_max_connections(DEFAULT_MAX_CONNECTIONS);
        let addr = server.local_addr().unwrap();
        let stats = server.stats();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            // `serve`, not `serve_shared`: the compatibility path, which is
            // the old behaviour exactly.
            server.serve(&mut serialized, move || flag.load(Ordering::SeqCst)).unwrap();
        });
        let served = Loopback { addr, stats, stop, thread: Some(thread) };

        let overlapped = slow_callers(&served, N, Endian::Big);
        assert!(
            overlapped < N,
            "serialized dispatch must not let {N} callers meet at the gate, but {overlapped} did"
        );
        assert_eq!(
            servant.peak_inside(),
            1,
            "serialized dispatch must never have two calls executing in the servant"
        );
        // The counter that is *not* the witness, asserted as such: it reaches
        // N here, where only one call is ever executing, because the other
        // N-1 are queued on the `Serialized` mutex inside `serve_one`.
        assert_eq!(
            served.stats.peak_at_servant(),
            N as u64,
            "at_servant counts queued callers, which is exactly why it cannot witness overlap"
        );
        // And every caller was still answered — serialized is slow, not broken.
        assert_eq!(served.stats.accepted(), N as u64);
        served.shutdown();
    }

    /// A [`SharedDispatch`] servant reached through the `&mut self` trait, so
    /// one servant can be served both ways and the negative control compares
    /// like with like.
    struct AsDispatch<D>(Arc<D>);

    impl<D: SharedDispatch> Dispatch for AsDispatch<D> {
        fn dispatch(
            &mut self,
            request: &Request,
            out: &mut Encoder,
        ) -> std::result::Result<(), SystemException> {
            SharedDispatch::dispatch(&*self.0, request, out)
        }

        fn dispatch_body(
            &mut self,
            request: &Request,
            out: &mut Encoder,
        ) -> std::result::Result<DispatchBody, SystemException> {
            SharedDispatch::dispatch_body(&*self.0, request, out)
        }

        fn knows(&self, object_key: &[u8]) -> bool {
            SharedDispatch::knows(&*self.0, object_key)
        }
    }

    /// A [`SharedDispatch`] servant that calls out while holding **no** lock —
    /// the shape `event_server`'s delivery loop has, written as the rule says
    /// to write it: copy what you need out of the lock, close it, then call.
    struct SharedRelay {
        target: Guarded<crate::Ior>,
        gate: Gate,
    }

    impl SharedDispatch for SharedRelay {
        fn dispatch(
            &self,
            req: &Request,
            out: &mut Encoder,
        ) -> std::result::Result<(), SystemException> {
            if req.operation != "relay" {
                return Err(SystemException::bad_operation());
            }
            // The rule, in one line: the target is *copied out* of the lock,
            // and the section is closed before anything dials. Moving the two
            // lines below inside the closure would trip `guarded`'s outbound
            // check rather than deadlocking a deployment.
            let target = self.target.read(|t| t.clone());
            // Every caller waits here until they are all in flight, so the
            // outbound calls genuinely overlap rather than merely happening.
            let overlapped = self.gate.arrive(DEADLINE);
            let relayed = (|| -> Result<i32> {
                let mut conn = crate::Connection::connect(&target, DEADLINE)?;
                let reply = conn.invoke_nullary("ping")?;
                reply.body()?.get_i32().map_err(Error::Cdr)
            })();
            match relayed {
                Ok(v) if overlapped => {
                    out.put_i32(v);
                    Ok(())
                }
                Ok(_) => Err(SystemException::bad_operation()), // never overlapped
                Err(e) => {
                    eprintln!("shared relay failed: {e}");
                    Err(SystemException::unknown_user_exception())
                }
            }
        }
    }

    /// **The hazard the concurrency batch already fought, re-tested against
    /// the thing that makes it easier to hit.**
    ///
    /// `event_server`'s rule 1 is that no lock may be held across an outbound
    /// call. Concurrent dispatch does not weaken the rule; it weakens the
    /// accident that used to hide breaches of it, because the servant side is
    /// no longer single-file. So: a servant that calls *out* while other calls
    /// to the same servant are *in flight*, from several clients at once, with
    /// a deadline on every wait so a resurrected deadlock is a failed test and
    /// not a hung suite.
    ///
    /// The gate inside the servant is what makes "while another call is in
    /// flight" true rather than hoped for: no caller can reach its outbound
    /// call until all of them have.
    #[test]
    fn an_outbound_call_with_other_calls_in_flight_does_not_deadlock() {
        const N: usize = 4;
        let inner = serving_shared(Arc::new(Pong), DEFAULT_MAX_CONNECTIONS);
        let servant = Arc::new(SharedRelay {
            target: Guarded::new("a relay's target", ior_at(inner.addr, b"k")),
            gate: Gate::new(N),
        });
        let outer = serving_shared(Arc::clone(&servant), DEFAULT_MAX_CONNECTIONS);

        let clients: Vec<_> = (0..N)
            .map(|i| {
                let mut c = outer.client();
                std::thread::spawn(move || {
                    let wire =
                        encode_request(Version::V1_2, Endian::Big, 1, b"k", "relay", true, |_| {})
                            .unwrap();
                    c.write_all(&wire).unwrap();
                    let msg = read_message(&mut c, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
                    let reply = crate::decode_reply(msg).unwrap();
                    assert_eq!(
                        reply.status,
                        ReplyStatus::NoException,
                        "relay {i}: an overlapping outbound call must complete"
                    );
                    assert_eq!(reply.body().unwrap().get_i32().unwrap(), 42, "relay {i}");
                })
            })
            .collect();
        for t in clients {
            t.join().unwrap();
        }
        assert!(
            outer.stats.peak_at_servant() >= N as u64,
            "the outbound calls must have overlapped, not queued: peak was {}",
            outer.stats.peak_at_servant()
        );
        outer.shutdown();
        inner.shutdown();
    }

    /// The tripwire is not decoration: the outbound client path really does
    /// refuse to block while a lock section is open.
    ///
    /// Written against a *live* server, so the only reason the connect does
    /// not happen is the discipline — a connect that would have failed anyway
    /// proves nothing. `guarded`'s own tests prove the counter; this one
    /// proves it is wired into [`crate::Connection`].
    #[test]
    fn the_outbound_path_refuses_to_dial_from_inside_a_lock_section() {
        let inner = serving_shared(Arc::new(Pong), DEFAULT_MAX_CONNECTIONS);
        let held = Guarded::new("a servant that should have copied out", ior_at(inner.addr, b"k"));

        // Outside the section the same dial succeeds, which is what makes the
        // refusal below attributable to the lock and to nothing else.
        let ior = held.read(|i| i.clone());
        let mut ok = crate::Connection::connect(&ior, DEADLINE).unwrap();
        assert_eq!(ok.invoke_nullary("ping").unwrap().body().unwrap().get_i32().unwrap(), 42);
        drop(ok);

        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            held.read(|i| crate::Connection::connect(i, DEADLINE).map(|_| ()))
        }));
        assert!(refused.is_err(), "dialling from inside a lock section must be refused");
        assert_eq!(crate::guarded::section_held(), None, "the unwound section must have closed");
        inner.shutdown();
    }

    /// A [`SharedDispatch`] servant calling back into its **own** server.
    struct SharedSelfCaller {
        own: crate::Ior,
    }

    impl SharedDispatch for SharedSelfCaller {
        fn dispatch(
            &self,
            req: &Request,
            out: &mut Encoder,
        ) -> std::result::Result<(), SystemException> {
            match req.operation.as_str() {
                "ping" => {
                    out.put_i32(42);
                    Ok(())
                }
                "call_myself" => {
                    let attempt = (|| -> Result<i32> {
                        let mut conn = crate::Connection::connect(&self.own, DEADLINE)?;
                        conn.invoke_nullary("ping")?.body()?.get_i32().map_err(Error::Cdr)
                    })();
                    match attempt {
                        Ok(v) => {
                            out.put_i32(v);
                            Ok(())
                        }
                        Err(e) => {
                            eprintln!("self-call failed: {e}");
                            Err(SystemException::unknown_user_exception())
                        }
                    }
                }
                _ => Err(SystemException::bad_operation()),
            }
        }
    }

    /// The re-entrancy that [`Serialized`] forbids and [`SharedDispatch`] does
    /// not — asserted, because "it should work now" is not a measurement.
    ///
    /// `a_servant_calling_its_own_server_fails_by_timeout_without_wedging_it`
    /// pins the other half: on the compatibility path the same call fails on
    /// its own timeout, because it waits for a mutex its caller holds. A
    /// servant that holds no lock across the call has no such limit, and the
    /// difference between the two tests is the whole difference between the
    /// two paths.
    #[test]
    fn a_shared_servant_may_call_back_into_its_own_server() {
        let server = Server::bind("127.0.0.1:0", b"k".to_vec()).unwrap();
        let addr = server.local_addr().unwrap();
        let servant = Arc::new(SharedSelfCaller { own: ior_at(addr, b"k") });
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let held = Arc::clone(&servant);
        let thread = std::thread::spawn(move || {
            server.serve_shared(&*held, move || flag.load(Ordering::SeqCst)).unwrap();
        });

        let mut c = TcpStream::connect(addr).unwrap();
        c.set_read_timeout(Some(DEADLINE)).unwrap();
        let wire = encode_request(Version::V1_2, Endian::Big, 1, b"k", "call_myself", true, |_| {})
            .unwrap();
        c.write_all(&wire).unwrap();
        let msg = read_message(&mut c, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
        let reply = crate::decode_reply(msg).unwrap();
        assert_eq!(reply.status, ReplyStatus::NoException, "a re-entrant call must now succeed");
        assert_eq!(reply.body().unwrap().get_i32().unwrap(), 42);

        drop(c);
        stop.store(true, Ordering::SeqCst);
        thread.join().unwrap();
    }

    /// Concurrency must not have cost the ordinary guarantees: `knows`,
    /// oneways, `LocateRequest` and the cap all still behave on the shared
    /// path, in both byte orders.
    struct Keyed;

    impl SharedDispatch for Keyed {
        fn knows(&self, object_key: &[u8]) -> bool {
            object_key == b"k"
        }

        fn dispatch(
            &self,
            req: &Request,
            out: &mut Encoder,
        ) -> std::result::Result<(), SystemException> {
            SharedDispatch::dispatch(&Pong, req, out)
        }
    }

    #[test]
    fn a_shared_servant_still_answers_locate_and_refuses_a_key_it_does_not_know() {
        for endian in [Endian::Big, Endian::Little] {
            let served = serving_shared(Arc::new(Keyed), DEFAULT_MAX_CONNECTIONS);
            let mut c = served.client();

            let mut e = Encoder::new(endian);
            let size_at = message_header(&mut e, Version::V1_2, endian, MsgType::LocateRequest);
            e.put_u32(7);
            e.put_u16(0);
            e.put_octet_seq(b"k");
            let size = (e.len() - HEADER_LEN) as u32;
            e.patch_u32(size_at, size);
            c.write_all(&e.finish().unwrap()).unwrap();
            let msg = read_message(&mut c, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
            assert_eq!(msg.msg_type, MsgType::LocateReply, "{endian:?}");

            let wire =
                encode_request(Version::V1_2, endian, 3, b"other", "ping", true, |_| {}).unwrap();
            c.write_all(&wire).unwrap();
            let msg = read_message(&mut c, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
            let reply = crate::decode_reply(msg).unwrap();
            assert_eq!(reply.status, ReplyStatus::SystemException, "{endian:?}");
            assert_eq!(reply.body().unwrap().get_string().unwrap(), OBJECT_NOT_EXIST);

            drop(c);
            served.shutdown();
        }
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
