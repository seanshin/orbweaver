//! GIOP messages, IIOP transport, and IOR handling.
//!
//! Implements GIOP 1.0, 1.1 and 1.2 on both sides: request/reply and
//! locate/locate-reply, fragmentation in both directions, codeset negotiation,
//! multi-profile failover at connect time and the serving half. Several
//! requests may be in flight on one connection ([`mux`]) and connections are
//! reused per endpoint ([`pool`]); where something is absent this code fails
//! loudly rather than misparsing.
//!
//! Failover is verified here only at the dial level — a refused endpoint moves
//! the client to the next one. Whether a peer that *accepts* on a secondary
//! address actually serves the object is a peer-level question, answered by
//! the harness against real ORBs, not by unit tests.
//!
//! # Two rules that govern everything here
//!
//! **Alignment origin.** A GIOP message aligns from the first byte of its
//! 12-byte header, so the header is built into the same buffer as the body and
//! the CDR origin stays at zero. An encapsulation nested inside (IOR profiles,
//! service contexts) restarts alignment at its own first byte.
//!
//! **The version is the peer's to choose.** An IIOP profile advertises the
//! highest GIOP minor version the server supports, and CORBA 3.4 §9.4.1
//! forbids exceeding it. Header layouts differ between versions in ways that
//! misparse rather than error — `ReplyHeader_1_0` puts `service_context`
//! first, `ReplyHeader_1_2` puts it last — so the version travels with every
//! message rather than being assumed.
//!
//! Spec: OMG CORBA 3.4 Part 2, sections 9.3 (CDR), 9.4 (GIOP), 9.7 (IIOP/IOR).

#![deny(missing_docs)]

pub mod codeset;
pub mod csiv2;
pub mod event_server;
pub mod guarded;
pub mod mux;
pub mod naming;
pub mod naming_server;
pub mod nat;
pub mod pool;
pub mod server;
pub mod ssliop;
pub mod typecode;

use orbweaver_cdr::{Decoder, Encoder, Endian};
use std::fmt;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// The four bytes every GIOP message starts with.
pub const MAGIC: &[u8; 4] = b"GIOP";

/// Fixed size of a GIOP message header.
pub const HEADER_LEN: usize = 12;

/// Profile tag for an IIOP profile inside an IOR.
pub const TAG_INTERNET_IOP: u32 = 0;

/// Component id for an alternate endpoint on an IIOP profile
/// (IOP `TAG_ALTERNATE_IIOP_ADDRESS`, ComponentId 3).
///
/// The component body is a CDR encapsulation of `string host; unsigned short
/// port;` — another way to reach the *same* profile, so it shares the
/// profile's IIOP version and object key.
pub const TAG_ALTERNATE_IIOP_ADDRESS: u32 = 3;

/// Default ceiling on an inbound message body.
///
/// `message_size` is four attacker-controlled bytes that used to drive a
/// `Vec::resize` directly, so `ff ff ff ff` asked for a 4 GiB zeroed
/// allocation and an allocation failure aborts the process. A ceiling is not
/// optional on a network-facing decoder.
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;

/// How many `LOCATION_FORWARD` hops to follow before giving up.
pub const MAX_FORWARD_HOPS: u8 = 8;

/// How long [`Connection::invoke`] waits for a dial it makes itself — to a
/// forwarded-to reference, or back to [`Connection::origin`] on a §9.6
/// restart. The caller chose the first dial's timeout; these dials happen
/// inside a call it has already made.
pub const FOLLOW_TIMEOUT: Duration = Duration::from_secs(10);

/// Body size above which an outbound message is split into fragments.
///
/// Chosen below omniORB's 2 MiB default `giopMaxMsgSize` so a large sequence
/// leaves here already fragmented rather than being rejected whole with
/// `MARSHAL`. Peers advertise no limit, so this is a guess that errs small.
pub const DEFAULT_FRAGMENT_THRESHOLD: usize = 1024 * 1024;

/// Most fragments to accept for one logical message before calling it hostile.
///
/// A peer that never sets the final-fragment bit would otherwise hold the
/// connection open and grow the reassembly buffer without bound.
pub const MAX_FRAGMENTS: usize = 4096;

/// A GIOP protocol version.
///
/// `Hash` because [`pool`] keys connections on it: two references to one
/// endpoint that negotiated different versions may not share a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Major version. Always 1 in practice.
    pub major: u8,
    /// Minor version: 0, 1 or 2.
    pub minor: u8,
}

impl Version {
    /// GIOP 1.0.
    pub const V1_0: Version = Version { major: 1, minor: 0 };
    /// GIOP 1.1.
    pub const V1_1: Version = Version { major: 1, minor: 1 };
    /// GIOP 1.2 — the highest this implementation speaks.
    pub const V1_2: Version = Version { major: 1, minor: 2 };

    /// The highest version we can speak.
    pub const fn max_supported() -> Version {
        Version::V1_2
    }

    /// Whether headers use the 1.2 field order and `TargetAddress`.
    pub const fn is_1_2_layout(self) -> bool {
        self.minor >= 2
    }

    /// Whether a request/reply body is aligned to 8. Only 1.2 does this, and
    /// only when the body is non-empty (§9.4.2.1, §9.4.3.1).
    pub const fn aligns_body(self) -> bool {
        self.minor >= 2
    }

    /// Whether the header carries the three reserved octets. 1.0 does not.
    pub const fn has_reserved_octets(self) -> bool {
        self.minor >= 1
    }

    /// Picks the version to speak to a peer advertising `advertised`.
    ///
    /// §9.4.1: a client must not exceed the minor version published in the
    /// IIOP profile.
    pub fn negotiate(advertised: Version) -> Version {
        if advertised < Version::max_supported() { advertised } else { Version::max_supported() }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GIOP {}.{}", self.major, self.minor)
    }
}

/// GIOP message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MsgType {
    /// Client invokes an operation.
    Request = 0,
    /// Server answers an invocation.
    Reply = 1,
    /// Client abandons a request.
    CancelRequest = 2,
    /// Client asks whether a target is here.
    LocateRequest = 3,
    /// Server answers a locate.
    LocateReply = 4,
    /// Peer is shutting the connection down cleanly.
    CloseConnection = 5,
    /// Peer could not process the message.
    MessageError = 6,
    /// Continuation of an oversized message.
    Fragment = 7,
}

impl MsgType {
    /// Interprets the message-type octet of a GIOP header.
    pub const fn from_octet(v: u8) -> Option<Self> {
        Some(match v {
            0 => MsgType::Request,
            1 => MsgType::Reply,
            2 => MsgType::CancelRequest,
            3 => MsgType::LocateRequest,
            4 => MsgType::LocateReply,
            5 => MsgType::CloseConnection,
            6 => MsgType::MessageError,
            7 => MsgType::Fragment,
            _ => return None,
        })
    }
}

/// Outcome reported by a GIOP `Reply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyStatus {
    /// Operation completed; the body holds the result.
    NoException,
    /// Operation raised a user exception declared in IDL.
    UserException,
    /// Operation raised a system exception.
    SystemException,
    /// Target moved; the body holds a new IOR to retry against.
    LocationForward,
    /// Target moved permanently. GIOP 1.2 only.
    LocationForwardPerm,
    /// Wrong target addressing disposition. GIOP 1.2 only.
    NeedsAddressingMode,
}

impl ReplyStatus {
    /// Interprets a reply status for a given version.
    ///
    /// `ReplyStatusType_1_0` (used by GIOP 1.0 *and* 1.1) has four
    /// enumerators; 4 and 5 were added in 1.2. Accepting them on a 1.1 reply
    /// would be accepting a value the peer cannot have meant.
    pub const fn from_u32(v: u32, version: Version) -> Option<Self> {
        Some(match v {
            0 => ReplyStatus::NoException,
            1 => ReplyStatus::UserException,
            2 => ReplyStatus::SystemException,
            3 => ReplyStatus::LocationForward,
            4 if version.minor >= 2 => ReplyStatus::LocationForwardPerm,
            5 if version.minor >= 2 => ReplyStatus::NeedsAddressingMode,
            _ => return None,
        })
    }
}

/// A redirect: where a request goes instead, and whether the caller may
/// forget the reference it dialled.
///
/// The two are §9.4.3.2's `LOCATION_FORWARD` (status 3) and
/// `LOCATION_FORWARD_PERM` (status 4, GIOP 1.2 and later). Both carry an IOR
/// and both oblige the client to retry there without the application noticing.
/// What differs is what the client may *conclude*: after a temporary forward
/// the reference it dialled is still valid and is where it falls back to when
/// the new one stops answering; after a permanent one the object lives at the
/// new reference and the client may replace what it holds. A servant that has
/// moved an object for good says so with [`Forward::Permanent`] — until this
/// type existed nothing a servant could return said it, and every mover was
/// re-forwarding every caller on every call.
///
/// `From<Ior>` is the temporary case, so a bare reference keeps meaning what
/// it always meant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Forward {
    /// `LOCATION_FORWARD`: retry there; the reference you dialled is still good.
    Temporary(Ior),
    /// `LOCATION_FORWARD_PERM`: it lives there now.
    Permanent(Ior),
}

impl Forward {
    /// The reference to retry against.
    pub fn ior(&self) -> &Ior {
        match self {
            Forward::Temporary(ior) | Forward::Permanent(ior) => ior,
        }
    }

    /// The reference to retry against, owned.
    pub fn into_ior(self) -> Ior {
        match self {
            Forward::Temporary(ior) | Forward::Permanent(ior) => ior,
        }
    }

    /// Whether the caller may replace the reference it dialled.
    pub fn is_permanent(&self) -> bool {
        matches!(self, Forward::Permanent(_))
    }

    /// The reply status this travels under when answering a `version` peer.
    ///
    /// `LOCATION_FORWARD_PERM` is status 4, and `ReplyStatusType_1_0` — the
    /// enumeration GIOP 1.0 and 1.1 both use — stops at 3. A 1.0/1.1 peer
    /// therefore cannot be told *permanent*; it is told `LOCATION_FORWARD`,
    /// which is true (the object is there) and loses only the permission to
    /// forget the old address. Refusing the reply instead would drop a
    /// legitimate redirect on the floor for the crime of being sent to an
    /// older client.
    pub const fn reply_status(&self, version: Version) -> ReplyStatus {
        match self {
            Forward::Permanent(_) if version.is_1_2_layout() => ReplyStatus::LocationForwardPerm,
            Forward::Permanent(_) | Forward::Temporary(_) => ReplyStatus::LocationForward,
        }
    }
}

impl From<Ior> for Forward {
    fn from(ior: Ior) -> Self {
        Forward::Temporary(ior)
    }
}

/// Anything that can go wrong invoking over GIOP.
#[derive(Debug)]
pub enum Error {
    /// Underlying socket failure.
    Io(std::io::Error),
    /// A CDR value could not be read or written.
    Cdr(orbweaver_cdr::Error),
    /// The peer sent something that is not a GIOP message.
    NotGiop([u8; 4]),
    /// The peer spoke a GIOP version this implementation does not handle.
    UnsupportedVersion(Version),
    /// The message-type octet had no valid interpretation.
    UnknownMessageType(u8),
    /// The peer sent a message type we did not expect here.
    UnexpectedMessage(MsgType),
    /// `message_size` exceeded the configured ceiling.
    MessageTooLarge {
        /// Size the peer declared.
        declared: usize,
        /// Ceiling in force.
        limit: usize,
    },
    /// The peer fragmented a message. Reassembly is Phase 1 work still
    /// outstanding; failing here is deliberate, because decoding the first
    /// fragment as a whole message silently truncates the value.
    FragmentUnsupported,
    /// The reply status octet had no valid interpretation for its version.
    BadReplyStatus(u32),
    /// The target raised a CORBA system exception.
    SystemException {
        /// Repository ID, e.g. `IDL:omg.org/CORBA/BAD_OPERATION:1.0`.
        id: String,
        /// Vendor minor code.
        minor: u32,
        /// Whether the operation ran before failing.
        completed: u32,
    },
    /// The target raised a user exception. The undecoded body is retained so
    /// the caller can read the exception's members.
    UserException {
        /// Repository ID of the exception.
        id: String,
        /// The reply, positioned so `body()` starts at the repository ID.
        reply: Box<Reply>,
    },
    /// The IOR string could not be parsed.
    BadIor(&'static str),
    /// Generated or dynamic decode found bytes that do not fit the contract.
    Decode(&'static str),
    /// The IOR carried no IIOP profile to connect to.
    NoIiopProfile,
    /// Every endpoint the IOR named was dialed and none accepted.
    ///
    /// Carries the count *and* the last endpoint's failure, because a caller
    /// debugging a dead service needs the reason — refused and timed out call
    /// for different fixes — not just how many addresses were tried.
    AllEndpointsFailed {
        /// How many host:port endpoints were dialed, counting each profile's
        /// own address and every alternate.
        tried: usize,
        /// Why the last endpoint failed.
        last: Box<Error>,
    },
    /// The peer closed the connection cleanly. Per §9.4.7 the pending request
    /// was not processed and may be safely re-sent on a new connection.
    ConnectionClosed,
    /// A previous failure left unread bytes in the stream, so the connection
    /// can no longer be framed. It must be discarded, not reused.
    Desynchronized,
    /// A fragmented message was cut short by an orderly control message —
    /// `CloseConnection` (§9.4.7) or `MessageError` (§9.4.8) — arriving where
    /// §9.4.9's continuation was due.
    ///
    /// Two rules meet here and neither one alone gives a true answer. §9.4.9
    /// says nothing may interrupt a fragmented message on a connection, which
    /// makes this an interleaved message; §13.5.1 makes `CloseConnection`
    /// something a server may legitimately send at any moment, which is why
    /// [`crate::pool`] re-sends on one. So [`Error::Desynchronized`] and
    /// [`Error::UnexpectedMessage`] both say "the peer is broken and the stream
    /// is corrupt" about what was in fact an orderly goodbye, and a client that
    /// believes them gives up on a call it could simply have re-dialed.
    /// [`Error::ConnectionClosed`] says the opposite and is no better: its
    /// promise is §13.5.1's *"were not processed, and may be safely resent"*,
    /// and a peer that had already begun to send this reply had, demonstrably,
    /// processed the request — re-sending a non-idempotent operation on that
    /// promise runs it twice.
    ///
    /// Hence a variant of its own. It says teardown rather than corruption —
    /// see [`Error::is_orderly_close`] — and it names the one request §13.5.1's
    /// promise does **not** cover. Every *other* request outstanding on the
    /// connection is still covered, which is why [`crate::mux::Failed::unsent`]
    /// is decided per caller rather than per connection.
    InterruptedMidReassembly {
        /// What arrived instead: `CloseConnection` or `MessageError`.
        ///
        /// The difference is what the peer is telling us. A `CloseConnection`
        /// is about the *connection* and leaves every other outstanding
        /// request re-sendable; a `MessageError` is a report about something
        /// **we** sent that the peer could not parse (§9.4.8), names nothing,
        /// and therefore makes no request safe to re-send.
        control: MsgType,
        /// The message type that was being reassembled.
        partial: MsgType,
        /// The request id of that half-received message — the one call that
        /// must not be re-sent on §13.5.1's promise.
        request_id: u32,
        /// How many wire messages of it had arrived, counting the leading one.
        received: usize,
    },
    /// `LOCATION_FORWARD` chain exceeded [`MAX_FORWARD_HOPS`].
    TooManyForwards,
    /// A multiplexed call's own deadline expired before its reply arrived.
    ///
    /// Distinct from [`Error::Io`] with a timeout kind, which is the *socket*
    /// giving up: this is the caller giving up on a connection that may still
    /// be perfectly healthy for everybody else on it. See [`mux`].
    Timeout {
        /// The request that went unanswered, so a `CancelRequest` can name it.
        request_id: u32,
        /// How long this caller actually waited.
        waited: Duration,
    },
    /// More than one request in flight was asked for on a connection where
    /// this implementation will not do it — see [`mux`] for the version
    /// argument and for which transports can be split.
    MultiplexingUnsupported {
        /// The version negotiated for the connection.
        version: Version,
    },
    /// The pool is at its connection bound and nothing in it could be evicted,
    /// so a new endpoint cannot be dialed. Refusing is deliberate; see
    /// [`pool`].
    PoolExhausted {
        /// The bound in force.
        limit: usize,
    },
    /// No profile in the IOR advertised a TLS endpoint (`TAG_SSL_SEC_TRANS`),
    /// so [`Connection::connect_tls`] had nothing to dial. Distinct from
    /// [`Error::AllEndpointsFailed`] on purpose: "the target offers no TLS"
    /// and "the target's TLS endpoints are down" call for different fixes.
    #[cfg(feature = "ssliop")]
    NoTlsEndpoint,
    /// The TLS client could not be set up: rejected configuration or a
    /// profile host that is not a valid server name. Handshake and
    /// certificate failures surface as [`Error::Io`] instead, because rustls
    /// reports them through the socket I/O that carried the handshake.
    #[cfg(feature = "ssliop")]
    Tls(rustls::Error),
    /// §7.10.2.6: the peer published a `TAG_CODE_SETS` component and nothing
    /// in it was acceptable to both sides. `CODESET_INCOMPATIBLE`.
    ///
    /// Distinct from sending nothing and hoping. A profile that publishes no
    /// component at all is not this error — §7.10.2.5 gives that case a
    /// specified default — but a profile that publishes one we cannot agree
    /// with has told us, in advance, that our `string` octets will be read as
    /// something they are not.
    CodesetIncompatible(codeset::NegotiationError),
    /// A `char` transmission codeset was agreed, and nothing on this
    /// connection will convert to it.
    ///
    /// This crate's writers emit UTF-8 (`Encoder::put_str`, and every generated
    /// stub through it). When negotiation lands somewhere else the octets and
    /// the `CodeSets` context stop describing the same thing, so the request is
    /// refused before it reaches the wire. A caller that *does* convert says so
    /// with [`Connection::convert_chars`] and is not refused.
    CodesetNotApplied {
        /// What §7.10.2.6 agreed on and nobody encoded to.
        negotiated: codeset::CodeSetId,
    },
}

impl Error {
    /// Whether the peer tore this connection down in an orderly way — §9.4.7
    /// `CloseConnection`, whether it arrived between messages or between the
    /// fragments of one — rather than leaving a stream that can no longer be
    /// framed.
    ///
    /// Exists so the decision that matters can be taken on a value instead of
    /// on the text of a message. "Teardown" and "corruption" call for opposite
    /// reactions — dial again versus stop and report — and a caller that has to
    /// match on a `Display` string to tell them apart will get it wrong the
    /// first time either string is reworded.
    ///
    /// True does **not** by itself mean the call may be re-sent: an orderly
    /// close that interrupted a reply already in flight is
    /// [`Error::InterruptedMidReassembly`], and for that one request §13.5.1's
    /// promise does not hold. Re-send safety is [`crate::mux::Failed::unsent`],
    /// which knows which caller is asking.
    pub fn is_orderly_close(&self) -> bool {
        matches!(
            self,
            Error::ConnectionClosed
                | Error::InterruptedMidReassembly { control: MsgType::CloseConnection, .. }
        )
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io: {e}"),
            Error::Cdr(e) => write!(f, "cdr: {e}"),
            Error::NotGiop(m) => write!(f, "not a GIOP message, magic was {m:?}"),
            Error::UnsupportedVersion(v) => write!(f, "unsupported {v}"),
            Error::UnknownMessageType(v) => write!(f, "unknown GIOP message type {v}"),
            Error::UnexpectedMessage(t) => write!(f, "unexpected GIOP message: {t:?}"),
            Error::MessageTooLarge { declared, limit } => {
                write!(f, "peer declared a {declared}-byte message, ceiling is {limit}")
            }
            Error::FragmentUnsupported => {
                write!(f, "peer fragmented the message; reassembly is not implemented")
            }
            Error::BadReplyStatus(v) => write!(f, "invalid reply status {v} for this version"),
            Error::SystemException { id, minor, completed } => {
                write!(f, "system exception {id} (minor={minor}, completed={completed})")
            }
            Error::UserException { id, .. } => write!(f, "user exception {id}"),
            Error::BadIor(why) => write!(f, "bad IOR: {why}"),
            Error::Decode(why) => write!(f, "decode: {why}"),
            Error::NoIiopProfile => write!(f, "IOR has no IIOP profile"),
            Error::AllEndpointsFailed { tried, last } => {
                write!(f, "all {tried} endpoint(s) failed; last: {last}")
            }
            Error::ConnectionClosed => {
                write!(f, "peer closed the connection; the request was not processed")
            }
            Error::Desynchronized => {
                write!(f, "connection is desynchronized and must be discarded")
            }
            Error::InterruptedMidReassembly { control, partial, request_id, received } => {
                write!(
                    f,
                    "peer sent {control:?} after {received} piece(s) of a fragmented {partial:?} \
                     for request {request_id}"
                )
            }
            Error::TooManyForwards => write!(f, "too many LOCATION_FORWARD hops"),
            Error::Timeout { request_id, waited } => {
                write!(f, "request {request_id} unanswered after {waited:?}")
            }
            Error::MultiplexingUnsupported { version } => {
                write!(f, "more than one request in flight is not supported on {version}")
            }
            Error::PoolExhausted { limit } => {
                write!(f, "connection pool is at its bound of {limit} and nothing was evictable")
            }
            #[cfg(feature = "ssliop")]
            Error::NoTlsEndpoint => {
                write!(f, "no profile in the IOR advertises a TLS endpoint (TAG_SSL_SEC_TRANS)")
            }
            #[cfg(feature = "ssliop")]
            Error::Tls(e) => write!(f, "tls: {e}"),
            Error::CodesetIncompatible(e) => {
                write!(f, "CODESET_INCOMPATIBLE: {e}")
            }
            Error::CodesetNotApplied { negotiated } => write!(
                f,
                "negotiated char codeset {negotiated}, which nothing on this connection \
                 encodes to; call Connection::convert_chars and write the converted \
                 octets, or talk to a peer that accepts UTF-8"
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<orbweaver_cdr::Error> for Error {
    fn from(e: orbweaver_cdr::Error) -> Self {
        Error::Cdr(e)
    }
}

/// Result of a GIOP operation.
pub type Result<T> = std::result::Result<T, Error>;

// ─────────────────────────────────────────────────────────────────────────────
// IOR
// ─────────────────────────────────────────────────────────────────────────────

/// A component attached to an IIOP profile, kept verbatim.
///
/// §9.7.2 requires that data we do not understand be "ignored, but preserved",
/// because an IOR we re-emit must still round-trip. Discarding components also
/// loses `TAG_SSL_SEC_TRANS`, whose absence makes an SSLIOP profile look like
/// port 0, and `TAG_CODE_SETS`, which codeset negotiation needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedComponent {
    /// Component identifier.
    pub tag: u32,
    /// Undecoded component body.
    pub data: Vec<u8>,
}

/// The parts of an IIOP profile needed to place a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IiopProfile {
    /// IIOP version, which bounds the GIOP version we may speak.
    pub version: Version,
    /// Host to connect to, as advertised by the server.
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// Opaque key identifying the servant behind the endpoint.
    pub object_key: Vec<u8>,
    /// Components, preserved whether or not we understand them.
    pub components: Vec<TaggedComponent>,
}

impl IiopProfile {
    /// Every endpoint this profile names, in the order they must be dialed:
    /// the profile's own host and port first, then each parseable
    /// [`TAG_ALTERNATE_IIOP_ADDRESS`] component in component order.
    ///
    /// A malformed alternate component is skipped rather than failing the
    /// profile. The component is a hint attached to an address that already
    /// works on its own, and a bad hint must not kill a good address — the
    /// same posture §9.7.2 takes toward components in general, which are to
    /// be ignored when not understood, never treated as fatal.
    pub fn endpoints(&self) -> Vec<(String, u16)> {
        let mut out = vec![(self.host.clone(), self.port)];
        for c in &self.components {
            if c.tag == TAG_ALTERNATE_IIOP_ADDRESS
                && let Ok(ep) = parse_alternate_address(&c.data)
            {
                out.push(ep);
            }
        }
        out
    }

    /// Encodes this profile as the encapsulation an IOR carries.
    pub fn encapsulate(&self, endian: Endian) -> Result<Encoder> {
        let mut e = Encoder::encapsulation(endian);
        e.put_u8(self.version.major);
        e.put_u8(self.version.minor);
        e.put_str(&self.host);
        e.put_u16(self.port);
        e.put_octet_seq(&self.object_key);
        // §9.7.2: a 1.0 profile must carry no trailing data at all, so the
        // component list is emitted only from 1.1 onward.
        if self.version.minor >= 1 {
            e.put_u32(self.components.len() as u32);
            for c in &self.components {
                e.put_u32(c.tag);
                e.put_octet_seq(&c.data);
            }
        }
        Ok(e)
    }
}

/// A parsed Interoperable Object Reference — the *dialing* view.
///
/// This type answers "where do I connect", so [`Ior::read_from`] keeps
/// `TAG_INTERNET_IOP` profiles and drops every other tag. That makes it lossy
/// by construction: re-emitting a parsed `Ior` loses a `TAG_MULTIPLE_COMPONENTS`
/// or vendor profile the original carried. Anything that must preserve a
/// reference — endpoint rewriting above all — works on [`nat::RawIor`], which
/// keeps every profile verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ior {
    /// Repository ID of the most-derived interface, empty for a nil reference.
    pub type_id: String,
    /// Every IIOP profile found, in order.
    pub profiles: Vec<IiopProfile>,
}

impl Ior {
    /// Parses the `IOR:<hex>` stringified form.
    ///
    /// §7.6.9 defines the prefix case-insensitively and states that the case
    /// of a stringified IOR is not significant.
    pub fn parse(s: &str) -> Result<Self> {
        Self::from_encapsulation(&ior_hex_bytes(s)?)
    }

    /// Parses the CDR encapsulation that a stringified IOR wraps.
    pub fn from_encapsulation(bytes: &[u8]) -> Result<Self> {
        let mut d = Decoder::encapsulation(bytes)?;
        Self::read_from(&mut d)
    }

    /// Reads an IOR marshalled inline in an existing stream.
    ///
    /// §9.3.6 marshals an object reference inline, not as a standalone
    /// encapsulation, which is how a `LOCATION_FORWARD` body and every
    /// `Object`-typed parameter arrive.
    pub fn read_from(d: &mut Decoder<'_>) -> Result<Self> {
        // A decode failure here must be fatal. Swallowing it with
        // `unwrap_or_default` leaves the cursor mid-string, so the profile
        // count is then read out of string content and the resulting profile
        // carries a host and port taken from arbitrary bytes — a parse that
        // succeeds and dials an endpoint nobody intended.
        let type_id = match d.get_string_bytes() {
            Ok(b) => String::from_utf8_lossy(b).into_owned(),
            Err(e) => return Err(Error::Cdr(e)),
        };
        let count = d.get_u32()?;
        // Each profile costs at least a 4-byte tag plus a 4-byte length.
        let count = d.validate_count(count, 8)?;
        let mut profiles = Vec::new();
        for _ in 0..count {
            let tag = d.get_u32()?;
            let body = d.get_octet_seq()?;
            if tag == TAG_INTERNET_IOP {
                profiles.push(parse_iiop_profile(body)?);
            }
        }
        Ok(Ior { type_id, profiles })
    }

    /// Whether this is the nil reference: no type and no profiles.
    pub fn is_nil(&self) -> bool {
        self.type_id.is_empty() && self.profiles.is_empty()
    }

    /// Marshals this reference inline into an existing stream (§9.3.6).
    pub fn write_to(&self, e: &mut Encoder) -> Result<()> {
        e.put_str(&self.type_id);
        e.put_u32(self.profiles.len() as u32);
        for p in &self.profiles {
            e.put_u32(TAG_INTERNET_IOP);
            e.put_encapsulation(p.encapsulate(e.endian())?);
        }
        Ok(())
    }

    /// Produces the `IOR:<hex>` stringified form.
    ///
    /// Needed to publish a reference at all: a peer cannot call us until it
    /// has one of these. Emission is deliberately little-endian, matching what
    /// every ORB observed here produces, but the parser accepts either.
    pub fn to_stringified(&self) -> Result<String> {
        let mut e = Encoder::encapsulation(Endian::Little);
        self.write_to(&mut e)?;
        Ok(hex_ior(&e.finish().map_err(Error::Cdr)?))
    }

    /// The first IIOP profile, which is the one dialed first.
    pub fn primary(&self) -> Result<&IiopProfile> {
        self.profiles.first().ok_or(Error::NoIiopProfile)
    }
}

/// The bytes behind an `IOR:<hex>` string.
///
/// §7.6.9 defines the prefix case-insensitively and states that the case of a
/// stringified IOR is not significant. Shared with [`nat::RawIor`] so the two
/// views of a reference cannot disagree about what a valid string is.
pub(crate) fn ior_hex_bytes(s: &str) -> Result<Vec<u8>> {
    let s = s.trim();
    let hex = s
        .get(..4)
        .filter(|p| p.eq_ignore_ascii_case("IOR:"))
        .map(|_| &s[4..])
        .ok_or(Error::BadIor("missing 'IOR:' prefix"))?;
    if hex.is_empty() || hex.len() % 2 != 0 {
        return Err(Error::BadIor("hex body has odd or zero length"));
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks(2) {
        let hi = hex_nibble(pair[0]).ok_or(Error::BadIor("non-hex digit"))?;
        let lo = hex_nibble(pair[1]).ok_or(Error::BadIor("non-hex digit"))?;
        bytes.push((hi << 4) | lo);
    }
    Ok(bytes)
}

/// The `IOR:<hex>` string for an encapsulation.
pub(crate) fn hex_ior(bytes: &[u8]) -> String {
    use fmt::Write;
    let mut out = String::with_capacity(4 + bytes.len() * 2);
    out.push_str("IOR:");
    for b in bytes {
        // `write!` into a String cannot fail; the Result is discarded rather
        // than unwrapped so this stays panic-free by construction.
        let _ = write!(out, "{b:02x}");
    }
    out
}

const fn hex_nibble(c: u8) -> Option<u8> {
    Some(match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => return None,
    })
}

fn parse_iiop_profile(body: &[u8]) -> Result<IiopProfile> {
    let mut d = Decoder::encapsulation(body)?;
    let major = d.get_u8()?;
    let minor = d.get_u8()?;
    let host = String::from_utf8_lossy(d.get_string_bytes()?).into_owned();
    let port = d.get_u16()?;
    let object_key = d.get_octet_seq()?.to_vec();

    // IIOP 1.0 profiles carry no components; 1.1 and later do. A 1.0 profile
    // with trailing data is malformed per §9.7.2, but tolerate it rather than
    // reject an otherwise usable reference.
    let mut components = Vec::new();
    if minor >= 1 && !d.is_empty() {
        let count = d.get_u32()?;
        let count = d.validate_count(count, 8)?;
        for _ in 0..count {
            let tag = d.get_u32()?;
            let data = d.get_octet_seq()?.to_vec();
            components.push(TaggedComponent { tag, data });
        }
    }

    Ok(IiopProfile { version: Version { major, minor }, host, port, object_key, components })
}

/// Decodes a `TAG_ALTERNATE_IIOP_ADDRESS` body: a CDR encapsulation of
/// `string host; unsigned short port;`.
fn parse_alternate_address(data: &[u8]) -> Result<(String, u16)> {
    let mut d = Decoder::encapsulation(data)?;
    let host = String::from_utf8_lossy(d.get_string_bytes()?).into_owned();
    let port = d.get_u16()?;
    Ok((host, port))
}

// ─────────────────────────────────────────────────────────────────────────────
// Messages
// ─────────────────────────────────────────────────────────────────────────────

/// A service context attached to a request or reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceContext {
    /// `IOP::ServiceId`, e.g. [`codeset::SERVICE_ID_CODE_SETS`].
    pub id: u32,
    /// Encapsulated body.
    pub data: Vec<u8>,
}

/// Encodes a GIOP `Request` for `version`, whose body is written by
/// `write_body`.
///
/// Header layout is version-conditional. 1.0 and 1.1 put `service_context`
/// first, use a `boolean response_expected`, carry the object key as a raw
/// sequence and end with `requesting_principal`; 1.2 puts `service_context`
/// last and addresses through a `TargetAddress` union. Sending the wrong shape
/// does not produce an error at the peer, it produces a misparse.
pub fn encode_request<F>(
    version: Version,
    endian: Endian,
    request_id: u32,
    object_key: &[u8],
    operation: &str,
    expect_reply: bool,
    write_body: F,
) -> Result<Vec<u8>>
where
    F: FnOnce(&mut Encoder),
{
    encode_request_with_contexts(
        version,
        endian,
        request_id,
        object_key,
        operation,
        expect_reply,
        &[],
        None,
        write_body,
    )
}

/// Encodes a `LocateRequest` for `version`.
///
/// 1.0 and 1.1 carry a bare `object_key`; 1.2 wraps it in a `TargetAddress`
/// union whose `KeyAddr` arm is discriminant 0 — the same asymmetry the
/// server-side decoder handles, expressed from the other end.
pub fn encode_locate_request(
    version: Version,
    endian: Endian,
    request_id: u32,
    object_key: &[u8],
) -> Result<Vec<u8>> {
    let mut e = Encoder::new(endian);
    e.put_bytes(b"GIOP");
    e.put_u8(version.major);
    e.put_u8(version.minor);
    e.put_u8(if endian == Endian::Little { 1 } else { 0 });
    e.put_u8(MsgType::LocateRequest as u8);
    let size_at = e.len();
    e.put_u32(0);
    e.put_u32(request_id);
    if version.is_1_2_layout() {
        e.put_u16(0); // TargetAddress: KeyAddr
    }
    e.put_octet_seq(object_key);
    let size = (e.len() - HEADER_LEN) as u32;
    e.patch_u32(size_at, size);
    e.finish().map_err(Error::Cdr)
}

/// Encodes a `CancelRequest` for `version` (§9.4.4).
///
/// The body is just the `request_id` being abandoned; unlike every other
/// header in this file the layout is identical in 1.0, 1.1 and 1.2. The
/// message is advisory — the target MAY ignore it, and no reply ever
/// correlates with it — so the sender learns nothing about whether the
/// cancellation took effect.
pub fn encode_cancel_request(version: Version, endian: Endian, request_id: u32) -> Result<Vec<u8>> {
    let mut e = Encoder::new(endian);
    e.put_bytes(MAGIC);
    e.put_u8(version.major);
    e.put_u8(version.minor);
    e.put_u8(if endian == Endian::Little { 1 } else { 0 });
    e.put_u8(MsgType::CancelRequest as u8);
    let size_at = e.len();
    e.put_u32(0);
    e.put_u32(request_id);
    let size = (e.len() - HEADER_LEN) as u32;
    e.patch_u32(size_at, size);
    e.finish().map_err(Error::Cdr)
}

/// What a `LocateReply` said about the object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocateResult {
    /// The target does not know the object at all.
    Unknown,
    /// The object is there; invoking will work.
    Here,
    /// The object lives elsewhere; the reply carried its new address.
    Forward(Box<Ior>),
}

/// Decodes a `LocateReply`.
///
/// §9.4.6's asymmetry, from the reading side: a `LocateReply` body follows the
/// header with **no** 8-byte alignment even in GIOP 1.2, unlike a `Reply`.
/// Applying the `Reply` rule here misreads every `OBJECT_FORWARD` body.
pub fn decode_locate_reply(msg: RawMessage) -> Result<(u32, LocateResult)> {
    if msg.msg_type != MsgType::LocateReply {
        return Err(Error::UnexpectedMessage(msg.msg_type));
    }
    let mut d = Decoder::new(&msg.bytes, msg.endian);
    d.seek_to(HEADER_LEN)?;
    let request_id = d.get_u32()?;
    let status = d.get_u32()?;
    let result = match status {
        0 => LocateResult::Unknown,
        1 => LocateResult::Here,
        // 3 is OBJECT_FORWARD_PERM (1.2): the permanence hint changes what a
        // client may cache, not what it must do next, so both carry the IOR.
        2 | 3 => LocateResult::Forward(Box::new(Ior::read_from(&mut d)?)),
        // LOC_SYSTEM_EXCEPTION (1.2): the body is the standard exception shape.
        4 => {
            return Err(Error::SystemException {
                id: d.get_string()?,
                minor: d.get_u32()?,
                completed: d.get_u32()?,
            });
        }
        other => return Err(Error::BadReplyStatus(other)),
    };
    Ok((request_id, result))
}

/// As [`encode_request`], but attaching service contexts and the transmission
/// codeset the connection agreed on.
///
/// Codeset negotiation needs both halves: §7.10.2.5 carries the agreed
/// transmission codesets in a `CodeSets` context, and without one the
/// specified default is ISO-8859-1 regardless of what bytes we actually send —
/// but declaring an agreement and then writing UTF-8 anyway is the *same*
/// defect wearing the other face, and that is what shipped until D009. `codec`
/// is `None` for UTF-8, which is every call this project made before then.
#[allow(clippy::too_many_arguments)]
pub fn encode_request_with_contexts<F>(
    version: Version,
    endian: Endian,
    request_id: u32,
    object_key: &[u8],
    operation: &str,
    expect_reply: bool,
    contexts: &[ServiceContext],
    codec: Option<std::sync::Arc<dyn orbweaver_cdr::TextCodec>>,
    write_body: F,
) -> Result<Vec<u8>>
where
    F: FnOnce(&mut Encoder),
{
    if version.major != 1 || version.minor > 2 {
        return Err(Error::UnsupportedVersion(version));
    }
    // The codec rides the *body*, and the header is not text: `operation` and
    // the object key are identifiers the contract chose, not the peer's prose,
    // and re-encoding them would change the name a servant dispatches on.
    // Attached after the header is written, for exactly that reason.
    let mut e = Encoder::new(endian);

    // ── message header (12 bytes, alignment origin) ──
    e.put_bytes(MAGIC);
    e.put_u8(version.major);
    e.put_u8(version.minor);
    if version.minor == 0 {
        // 1.0 defines this octet as a `boolean byte_order`, not a flags field.
        e.put_bool(endian == Endian::Little);
    } else {
        e.put_u8(endian.as_flag()); // bit 0 byte order, bit 1 more-fragments
    }
    e.put_u8(MsgType::Request as u8);
    let size_at = e.len();
    e.put_bytes(&[0, 0, 0, 0]); // message_size placeholder

    if version.is_1_2_layout() {
        e.put_u32(request_id);
        e.put_u8(if expect_reply { 3 } else { 0 }); // response_flags
        e.put_bytes(&[0, 0, 0]); // reserved
        e.put_u16(0); // TargetAddress discriminator: 0 = KeyAddr
        e.put_octet_seq(object_key);
        e.put_str(operation);
        write_contexts(&mut e, contexts);
    } else {
        write_contexts(&mut e, contexts); // 1.0/1.1 put the list first
        e.put_u32(request_id);
        e.put_bool(expect_reply); // response_expected
        if version.has_reserved_octets() {
            e.put_bytes(&[0, 0, 0]); // 1.1 only
        }
        e.put_octet_seq(object_key);
        e.put_str(operation);
        e.put_octet_seq(&[]); // requesting_principal
    }

    // §9.4.2.1: "There is no padding after the request header when an
    // unfragmented request message body is empty." Measure the body first so
    // the padding is only emitted when something follows it.
    // The body is measured before being emitted, so that padding can be
    // omitted when it turns out to be empty. It must still align as though it
    // sat where it will actually land — CDR counts from the start of the
    // message, not from the start of whatever buffer we built it in.
    let body_start = if version.aligns_body() { e.len().div_ceil(8) * 8 } else { e.len() };
    // The codec goes on the **body** and never on the header. `operation` and
    // the object key are written above with `put_str`, and they are
    // identifiers the contract chose — re-encoding them would change the name
    // a servant dispatches on. That the body is already a separate encoder is
    // what makes this one line instead of an argument threaded everywhere.
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

fn write_contexts(e: &mut Encoder, contexts: &[ServiceContext]) {
    e.put_u32(contexts.len() as u32);
    for c in contexts {
        e.put_u32(c.id);
        e.put_octet_seq(&c.data);
    }
}

/// A decoded GIOP `Reply`, with its body left as raw CDR for the caller.
#[derive(Debug, Clone)]
pub struct Reply {
    /// Correlates with the request that produced it.
    pub request_id: u32,
    /// Outcome reported by the target.
    pub status: ReplyStatus,
    /// Byte order the reply was encoded in.
    pub endian: Endian,
    /// Version the reply was encoded with.
    pub version: Version,
    /// Whole message, retained so body offsets stay meaningful.
    raw: Vec<u8>,
    /// Offset in `raw` where the reply body begins.
    body_at: usize,
    /// The narrow-text codeset this reply's body is in (D009 §7.1).
    ///
    /// A reply is encoded under the agreement its *request* was sent under, so
    /// this is the connection's, attached when the reply is read rather than
    /// re-derived here — a reply that decoded under a different agreement from
    /// the one it answered is the defect D009 is about, wearing the other face.
    codec: Option<std::sync::Arc<dyn orbweaver_cdr::TextCodec>>,
}

impl Reply {
    /// A decoder positioned at the start of the reply body.
    ///
    /// Fallible on purpose. The previous version discarded the seek error,
    /// which left the decoder at offset 0 and returned the GIOP magic as
    /// payload — wrong values, no error, nothing in the logs.
    pub fn body(&self) -> Result<Decoder<'_>> {
        let mut d = Decoder::new(&self.raw, self.endian).with_codec(self.codec.clone());
        d.seek_to(self.body_at)?;
        Ok(d)
    }

    /// Attaches the narrow-text codeset the connection agreed on.
    ///
    /// Crate-private: a reply's codeset is not the reader's choice, it is the
    /// one its request was sent under, and letting a caller set it would make
    /// that a matter of opinion.
    pub(crate) fn set_codec(
        &mut self,
        codec: Option<std::sync::Arc<dyn orbweaver_cdr::TextCodec>>,
    ) {
        self.codec = codec;
    }

    /// The undecoded message, for diagnostics.
    pub fn raw(&self) -> &[u8] {
        &self.raw
    }
}

/// Splits an encoded message into a leading message plus `Fragment`
/// continuations, if it exceeds `threshold`.
///
/// §9.4.9 constrains the leading and intermediate pieces: `message_size + 12`
/// must be divisible by 8, so that the next piece resumes on the alignment the
/// unfragmented stream would have had. Getting that wrong shifts every
/// subsequent field, which is why the split point is rounded rather than taken
/// wherever the threshold happens to fall.
///
/// Returns a single element when no split was needed.
pub fn fragment_message(msg: Vec<u8>, threshold: usize) -> Result<Vec<Vec<u8>>> {
    if msg.len() <= threshold || msg.len() <= HEADER_LEN {
        return Ok(vec![msg]);
    }
    let version = Version { major: msg[4], minor: msg[5] };
    if !version.is_1_2_layout() {
        // 1.1 fragments carry no request id and restart alignment; we do not
        // emit them for the same reason we refuse to read them.
        return Ok(vec![msg]);
    }
    let endian = if msg[6] & 1 == 1 { Endian::Little } else { Endian::Big };
    let msg_type = MsgType::from_octet(msg[7]).ok_or(Error::UnknownMessageType(msg[7]))?;
    if !matches!(
        msg_type,
        MsgType::Request | MsgType::Reply | MsgType::LocateRequest | MsgType::LocateReply
    ) {
        return Ok(vec![msg]);
    }
    let request_id = logical_request_id(&msg, endian, version, msg_type)?;

    // The first piece keeps the original header, so its total length must be a
    // multiple of 8.
    let mut cut = threshold.max(HEADER_LEN + 8);
    cut -= cut % 8;
    if cut >= msg.len() {
        return Ok(vec![msg]);
    }

    let mut out = Vec::new();
    let mut head = msg[..cut].to_vec();
    head[6] |= 0b10; // more fragments
    patch_size(&mut head, endian, (cut - HEADER_LEN) as u32);
    out.push(head);

    let mut pos = cut;
    while pos < msg.len() {
        // Each fragment is a header plus a 4-byte request id plus payload, and
        // the same divisible-by-8 rule applies while more follow.
        let mut take = threshold.saturating_sub(HEADER_LEN + 4).max(8);
        if pos + take < msg.len() {
            let total = HEADER_LEN + 4 + take;
            take -= total % 8;
        }
        let end = (pos + take).min(msg.len());
        let last = end == msg.len();

        let mut frag = Vec::with_capacity(HEADER_LEN + 4 + (end - pos));
        frag.extend_from_slice(MAGIC);
        frag.push(version.major);
        frag.push(version.minor);
        frag.push(endian.as_flag() | if last { 0 } else { 0b10 });
        frag.push(MsgType::Fragment as u8);
        frag.extend_from_slice(&[0, 0, 0, 0]);
        let id_bytes = match endian {
            Endian::Big => request_id.to_be_bytes(),
            Endian::Little => request_id.to_le_bytes(),
        };
        frag.extend_from_slice(&id_bytes);
        frag.extend_from_slice(&msg[pos..end]);
        let size = (frag.len() - HEADER_LEN) as u32;
        patch_size(&mut frag, endian, size);
        out.push(frag);
        pos = end;
    }
    Ok(out)
}

fn patch_size(msg: &mut [u8], endian: Endian, size: u32) {
    let b = match endian {
        Endian::Big => size.to_be_bytes(),
        Endian::Little => size.to_le_bytes(),
    };
    msg[8..12].copy_from_slice(&b);
}

/// A framed but undecoded GIOP message.
#[derive(Debug, Clone)]
pub struct RawMessage {
    /// Message type from the header.
    pub msg_type: MsgType,
    /// Version from the header.
    pub version: Version,
    /// Byte order from the header.
    pub endian: Endian,
    /// Whether the more-fragments bit was set. Always false after reassembly.
    pub more_fragments: bool,
    /// How many wire messages this logical message was reassembled from.
    /// One means the peer did not fragment.
    pub fragments: usize,
    /// Header and body together.
    pub bytes: Vec<u8>,
}

/// Reads one logical GIOP message, reassembling fragments if the peer sent
/// any.
///
/// §9.4.9 lets a peer split `Request`, `Reply`, `LocateRequest` and
/// `LocateReply` across a leading message plus `Fragment` continuations. In
/// GIOP 1.2 alignment does **not** restart per fragment — the pieces form one
/// logical stream — so reassembly is a concatenation of the leading message
/// with each fragment's payload, and the result aligns exactly as an
/// unfragmented message would have.
pub fn read_message(stream: &mut impl Read, max_size: usize) -> Result<RawMessage> {
    let first = read_one_message(stream, max_size)?;
    // A `Fragment` continues something. Arriving first it continues nothing,
    // and handing it back as a message makes every caller responsible for a
    // check none of them make — the server would see a message type it has no
    // arm for, which is a worse place to notice than here. Our own emitter
    // cannot produce this, which is exactly why it went unnoticed: no peer
    // that fragments exists to have shown it to us.
    if first.msg_type == MsgType::Fragment {
        return Err(Error::UnexpectedMessage(MsgType::Fragment));
    }
    if !first.more_fragments {
        return Ok(first);
    }
    if !first.version.is_1_2_layout() {
        // GIOP 1.1 restarts alignment relative to each fragment and carries no
        // request id to correlate them, so concatenation is not reassembly and
        // there is no way to tell whose fragments these are. Refusing beats
        // producing a plausible wrong value.
        //
        // This refusal deliberately wins over anything that follows, including
        // a `CloseConnection` sitting in the very next bytes: nothing more is
        // read, so the rest of the stream stays where it is and the diagnosis
        // stays about the thing we actually cannot do. Preferring the close
        // would be worse than untidy — `FragmentUnsupported` is permanent for
        // this peer and this reply, a close is retryable, and reporting the
        // retryable one would send [`crate::pool`] round again to be told the
        // same thing on a fresh connection.
        return Err(Error::FragmentUnsupported);
    }

    let RawMessage { msg_type, version, endian, mut bytes, .. } = first;
    let mut count = 0usize;
    loop {
        count += 1;
        if count > MAX_FRAGMENTS {
            return Err(Error::MessageTooLarge { declared: count, limit: MAX_FRAGMENTS });
        }
        let next = read_one_message(stream, max_size)?;
        if next.msg_type != MsgType::Fragment {
            // Not every interruption is a broken peer. `CloseConnection` and
            // `MessageError` are the two messages a peer sends *about* the
            // conversation rather than as part of it, and §13.5.1 makes the
            // first one an ordinary event a client is expected to survive. Both
            // arrive here as well-framed whole messages, so the inbound framing
            // is intact and "desynchronized" would be a false diagnosis; what is
            // lost is this one logical message, which can never complete now.
            // Reporting them as an interleaved `Request` would be reported is
            // the difference between a client that re-dials and a client that
            // gives up, so they are told apart here rather than guessed at
            // upstream.
            return Err(match next.msg_type {
                MsgType::CloseConnection | MsgType::MessageError => {
                    Error::InterruptedMidReassembly {
                        control: next.msg_type,
                        partial: msg_type,
                        request_id: logical_request_id(&bytes, endian, version, msg_type)?,
                        // The leading message plus every fragment that arrived
                        // before the interruption.
                        received: count,
                    }
                }
                other => Error::UnexpectedMessage(other),
            });
        }
        // Neither the version nor the byte order may change mid-message.
        //
        // The version is not pedantry about a field: a 1.1 `Fragment` carries
        // no request id, so the four bytes read below would be *body*. Matching
        // them against the leading message's id would then be a coincidence,
        // and mismatching them would report a desynchronised connection when
        // the real fault is a peer switching layouts.
        //
        // Byte order used to be allowed to change, on the grounds that "the
        // payload is opaque bytes either way". It is not opaque: §9.4.9 makes a
        // fragment a continuation of one CDR stream — alignment does not even
        // restart at 1.2 — so its payload is decoded under the *leading*
        // message's flag no matter what its own header said. A peer that flips
        // the flag mid-message therefore gets every multi-octet field after the
        // split silently reversed, with a reassembled header that still claims
        // the original order. §9.4.9 forbids it in as many words — "the byte
        // order and GIOP protocol version of a fragment shall be the same as
        // that of the message it continues" — so it is refused rather than
        // reassembled into a plausible wrong value.
        if next.version != version || next.endian != endian {
            return Err(Error::UnexpectedMessage(MsgType::Fragment));
        }
        // FragmentHeader_1_2 is a single request_id, which must match.
        let mut d = Decoder::new(&next.bytes, next.endian);
        d.seek_to(HEADER_LEN)?;
        let frag_id = d.get_u32()?;
        let own_id = logical_request_id(&bytes, endian, version, msg_type)?;
        if frag_id != own_id {
            return Err(Error::Desynchronized);
        }
        let payload = &next.bytes[HEADER_LEN + 4..];
        if bytes.len() + payload.len() > max_size {
            return Err(Error::MessageTooLarge {
                declared: bytes.len() + payload.len(),
                limit: max_size,
            });
        }
        bytes.extend_from_slice(payload);
        if !next.more_fragments {
            break;
        }
    }

    // Rewrite the header so the reassembled message describes itself: the
    // more-fragments bit is gone and message_size covers everything.
    let size = (bytes.len() - HEADER_LEN) as u32;
    bytes[6] &= !0b10;
    let size_bytes = match endian {
        Endian::Big => size.to_be_bytes(),
        Endian::Little => size.to_le_bytes(),
    };
    bytes[8..12].copy_from_slice(&size_bytes);
    Ok(RawMessage { msg_type, version, endian, more_fragments: false, fragments: count + 1, bytes })
}

/// Reads the `request_id` of a partially-received message, to match fragments
/// against it.
fn logical_request_id(
    bytes: &[u8],
    endian: Endian,
    version: Version,
    msg_type: MsgType,
) -> Result<u32> {
    let mut d = Decoder::new(bytes, endian);
    d.seek_to(HEADER_LEN)?;
    match msg_type {
        // In 1.2 the request id is the first field of all four fragmentable
        // message types.
        MsgType::Request | MsgType::Reply | MsgType::LocateRequest | MsgType::LocateReply => {
            let _ = version;
            Ok(d.get_u32()?)
        }
        other => Err(Error::UnexpectedMessage(other)),
    }
}

/// How much of a declared body is committed before any of it has arrived.
///
/// The bound this buys, in one sentence: **memory committed for an inbound
/// message stays proportional to the bytes the peer has actually delivered,
/// plus at most one chunk.** `message_size` above `max_size` is still refused
/// outright; below it, the ceiling is no longer something a peer can spend by
/// asserting a number, because the buffer only grows as the body arrives.
///
/// 64 KiB is a size, not a limit: large enough that an ordinary message is one
/// or two `read` calls, small enough that a header from a peer that then goes
/// quiet costs 64 KiB rather than 64 MiB.
const BODY_CHUNK: usize = 64 * 1024;

/// Reads exactly one GIOP message, without reassembling anything.
///
/// Rejects a `message_size` above `max_size` before allocating, and commits
/// what is below it only as it is received — see [`BODY_CHUNK`].
pub fn read_one_message(stream: &mut impl Read, max_size: usize) -> Result<RawMessage> {
    let mut header = [0u8; HEADER_LEN];
    stream.read_exact(&mut header)?;
    if &header[0..4] != MAGIC {
        return Err(Error::NotGiop([header[0], header[1], header[2], header[3]]));
    }
    let version = Version { major: header[4], minor: header[5] };
    if version.major != 1 || version.minor > 2 {
        return Err(Error::UnsupportedVersion(version));
    }

    let flags = header[6];
    let endian = if flags & 1 == 1 { Endian::Little } else { Endian::Big };
    let more_fragments = version.minor >= 1 && flags & 0b10 != 0;
    if version.minor >= 1 {
        // §9.4.1: the top six bits must be zero.
        if flags & 0b1111_1100 != 0 {
            return Err(Error::Cdr(orbweaver_cdr::Error::Malformed(
                "reserved bits of the GIOP flags octet must be zero",
            )));
        }
    } else if flags > 1 {
        // 1.0 defines the octet as a boolean.
        return Err(Error::Cdr(orbweaver_cdr::Error::Malformed(
            "GIOP 1.0 byte_order octet must be 0 or 1",
        )));
    }

    let msg_type = MsgType::from_octet(header[7]).ok_or(Error::UnknownMessageType(header[7]))?;

    let size = match endian {
        Endian::Big => u32::from_be_bytes([header[8], header[9], header[10], header[11]]),
        Endian::Little => u32::from_le_bytes([header[8], header[9], header[10], header[11]]),
    } as usize;
    if size > max_size {
        return Err(Error::MessageTooLarge { declared: size, limit: max_size });
    }

    // Twelve bytes must not buy a 64 MiB allocation. `size` is peer-declared
    // and, until the body arrives, entirely unbacked: committing and zeroing
    // it up front let one header — measured, 12 bytes, no body at all — cost
    // the whole ceiling, on every connection, for free. This is the rule the
    // sequence decoder already enforces with `validate_count`, arriving
    // through the reader's door instead of the decoder's, which is exactly why
    // it was missed here: the reader has no buffer to validate against, so it
    // must let the bytes themselves do the validating.
    let mut bytes = Vec::with_capacity(HEADER_LEN + size.min(BODY_CHUNK));
    bytes.extend_from_slice(&header);
    let mut remaining = size;
    while remaining > 0 {
        let chunk = remaining.min(BODY_CHUNK);
        let at = bytes.len();
        bytes.resize(at + chunk, 0);
        stream.read_exact(&mut bytes[at..])?;
        remaining -= chunk;
    }
    Ok(RawMessage { msg_type, version, endian, more_fragments, fragments: 1, bytes })
}

/// Decodes a `Reply` that [`read_message`] already framed.
///
/// The field order is version-conditional: 1.0 and 1.1 marshal
/// `service_context, request_id, reply_status`, while 1.2 marshals
/// `request_id, reply_status, service_context`. Reading a 1.1 reply with the
/// 1.2 order takes the service-context *count* as the request id, which is why
/// this cannot be version-agnostic.
pub fn decode_reply(msg: RawMessage) -> Result<Reply> {
    let RawMessage { version, endian, bytes: raw, .. } = msg;
    let mut d = Decoder::new(&raw, endian);
    d.seek_to(HEADER_LEN)?; // step over the header, keeping alignment

    let (request_id, status_raw);
    if version.is_1_2_layout() {
        request_id = d.get_u32()?;
        status_raw = d.get_u32()?;
        skip_service_contexts(&mut d)?;
    } else {
        skip_service_contexts(&mut d)?;
        request_id = d.get_u32()?;
        status_raw = d.get_u32()?;
    }

    let status =
        ReplyStatus::from_u32(status_raw, version).ok_or(Error::BadReplyStatus(status_raw))?;

    // §9.4.3.1: no padding after the header when the body is empty. Aligning
    // unconditionally can push the cursor past the end of a short message.
    let body_at = if version.aligns_body() && !d.is_empty() {
        d.align_to(8)?;
        d.offset()
    } else {
        d.offset()
    };

    Ok(Reply { request_id, status, endian, version, raw, body_at, codec: None })
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

// ─────────────────────────────────────────────────────────────────────────────
// Invoker
// ─────────────────────────────────────────────────────────────────────────────

/// The call surface a client stub needs, abstracted from the transport.
///
/// Generated stubs are generic over this rather than taking a `Connection`,
/// and the difference is a security boundary, not a convenience: a stub
/// hard-wired to `Connection` can only ever be used *around* the guard, so the
/// static path would recreate in compiled form exactly the bypass §4.7 exists
/// to prevent. With the trait, the same stub runs over a raw connection inside
/// the trust boundary or over `orbweaver-mcp`'s guarded wrapper at it — and the
/// guarded wrapper checks policy per operation, because the operation name is
/// right here in the signature.
pub trait Invoker {
    /// The byte order requests are encoded in.
    fn endian(&self) -> Endian;

    /// Invokes a twoway operation.
    fn invoke<F: Fn(&mut Encoder)>(&mut self, operation: &str, write_args: F) -> Result<Reply>;

    /// Invokes a oneway operation: bytes written, nothing more promised.
    fn invoke_oneway<F: Fn(&mut Encoder)>(&mut self, operation: &str, write_args: F) -> Result<()>;
}

impl Invoker for Connection {
    fn endian(&self) -> Endian {
        Connection::endian(self)
    }
    fn invoke<F: Fn(&mut Encoder)>(&mut self, operation: &str, write_args: F) -> Result<Reply> {
        Connection::invoke(self, operation, write_args)
    }
    fn invoke_oneway<F: Fn(&mut Encoder)>(&mut self, operation: &str, write_args: F) -> Result<()> {
        Connection::invoke_oneway(self, operation, write_args)
    }
}

/// The transport under a [`Connection`]: cleartext TCP, or TLS over it.
///
/// Private, and an enum rather than a boxed trait object on purpose: these
/// are the only transports v1 speaks, and the match keeps the plain arm
/// exactly the code it was before TLS existed — every read/write site above
/// this type is transport-blind and identical in both builds. Poisoning,
/// framing and timeout behaviour all live in [`Connection`] and see only
/// `Read + Write`, so they cannot differ between the arms.
enum Stream {
    /// Cleartext TCP, the only arm a default build compiles.
    Plain(TcpStream),
    /// TLS over TCP, dialed from a `TAG_SSL_SEC_TRANS` advertisement. Boxed
    /// because rustls's connection state is large and this arm must not tax
    /// the plain one's size.
    #[cfg(feature = "ssliop")]
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Stream {
    /// A second handle on the same transport, for a reader and a writer to
    /// hold at once — `None` when the transport cannot be split.
    ///
    /// A `TcpStream` splits: two clones name one kernel socket, and the kernel
    /// already serializes each direction, so one thread may block in `read`
    /// while another writes. A TLS session does not: its record layer, its
    /// sequence numbers and its rekeying live in one `ClientConnection` that
    /// both directions mutate, so "clone the socket" would clone the wrong
    /// half of the state. [`mux`] answers that by not multiplexing over TLS at
    /// all rather than by wrapping the session in a lock that would serialize
    /// it anyway — see that module.
    fn try_split(&self) -> Option<Stream> {
        match self {
            Stream::Plain(s) => s.try_clone().ok().map(Stream::Plain),
            #[cfg(feature = "ssliop")]
            Stream::Tls(_) => None,
        }
    }

    /// Re-arms the socket's read timeout.
    ///
    /// [`mux`] needs it per read rather than per connection: the thread that
    /// happens to be reading is a *caller*, with its own deadline, and a
    /// socket timeout set once at dial time would let it overshoot that
    /// deadline by the difference. The timeout is per `read` call, not per
    /// message, so a message that is still streaming in still completes.
    fn set_read_timeout(&self, t: Duration) -> std::io::Result<()> {
        // Zero means "no timeout" to the kernel, which is the opposite of what
        // a zero budget means here.
        let t = Some(t.max(Duration::from_millis(1)));
        match self {
            Stream::Plain(s) => s.set_read_timeout(t),
            #[cfg(feature = "ssliop")]
            Stream::Tls(s) => s.sock.set_read_timeout(t),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            #[cfg(feature = "ssliop")]
            Stream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            #[cfg(feature = "ssliop")]
            Stream::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            #[cfg(feature = "ssliop")]
            Stream::Tls(s) => s.flush(),
        }
    }
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stream::Plain(s) => f.debug_tuple("Plain").field(s).finish(),
            #[cfg(feature = "ssliop")]
            Stream::Tls(s) => f.debug_tuple("Tls").field(&s.sock).finish(),
        }
    }
}

/// A synchronous, single-connection invoker.
///
/// Deliberately minimal, but no longer optimistic: it negotiates the version
/// from the IOR, follows `LOCATION_FORWARD`, distinguishes a clean
/// `CloseConnection` from a protocol error, and poisons itself rather than
/// reusing a stream whose framing is in doubt.
///
/// **One request at a time**, which is now a choice rather than a limit: this
/// type owns its stream and blocks on the reply, so it is the right thing for
/// a spike, a probe, or any caller that wants a socket to itself. A caller
/// that wants several requests in flight, or connections shared across
/// references, wraps one in [`mux::Mux`] or asks [`pool::Pool`] for one —
/// both of which take a `Connection` and keep everything it negotiated.
///
/// # After a forward: where it is, and where it restarts from
///
/// Following a forward replaces the socket, so a `Connection` that followed
/// one *is* the connection to the forwarded-to endpoint (§9.4.3.2 permits
/// staying there). It also keeps [`Connection::origin`] — the reference it
/// was dialled from — because CORBA 3.4 Part 2 §9.6 says what happens when
/// the forwarded-to endpoint stops answering, and it says it in *shall*:
///
/// > Once a connection based on location-forwarding information is closed,
/// > a client can attempt to reuse the forwarding information it has, but,
/// > if that fails, it shall restart the location process using the
/// > original address specified in the initial object reference.
///
/// and, of `LOCATION_FORWARD_PERM` (§9.6, the same words at §9.4.3.2):
///
/// > ... it also provides an indication to the client that it may replace
/// > the old IOR with the new IOR. ... both the old IOR and the new IOR are
/// > valid, but the new IOR is preferred for future use.
///
/// [`Connection::invoke`] does both, in that order, once per call:
///
/// 1. **A temporary forward stands** ([`Connection::forwarded`] is
///    [`Forward::Temporary`]) **and the attempt failed without the request
///    having run** — the peer sent `CloseConnection` instead of a reply
///    (§13.5.1: outstanding requests *"were not processed, and may be safely
///    resent"*), or the request never left this side in full (a write
///    failure, or a connection already poisoned by an earlier call), so the
///    peer cannot have dispatched it. In every case the *connection* has
///    failed — an encode error or a codeset refusal is this side's and
///    leaves the connection sound, and is not a location event. Then the
///    forwarding information is
///    reused once — the forwarded-to endpoint is redialled and the request
///    re-issued there — and if that dial fails, or the re-issued request
///    fails the same way, the **origin** is redialled and the request
///    re-issued there. After the restart `forwarded()` is `None` (or
///    whatever the origin forwards to next).
/// 2. **A permanent forward** takes up the *may*: the forwarded-to IOR
///    becomes the origin. Nothing falls back past it — a permanent forward
///    said the old address is no longer the preferred one, and this client
///    believes it.
///
/// What does **not** trigger the restart, and why: a failure that arrives
/// after a complete request went out and carries no proof of non-processing
/// — a socket EOF or reset while waiting for the reply, a socket timeout, a
/// `CloseConnection` that interrupted a reply already begun
/// ([`Error::InterruptedMidReassembly`]) — leaves the request's completion
/// **unknown**, and a request that may have executed must not be sent
/// again: re-issuing a non-idempotent operation on a guess runs it twice.
/// Those return the error, poison the connection, and the *next* call on
/// it, whose request has never left, is what restarts (rule 1's third
/// case). A reply-carrying failure — a system or user exception — is an
/// answer, not a location failure, and is never retried anywhere. A
/// forward whose target refuses the very first dial is not a closed
/// forwarding connection either; it fails as it always did.
///
/// omniORB 4.3.4 restarts on that unknown case as well: in
/// `spikes/perm_fallback.sh` its `ping` after the target was killed by PID —
/// no `CloseConnection`, a bare close — is answered by the original on the
/// same call under temporary, and its permanent arm shows what that failure
/// was (`COMM_FAILURE`, `COMPLETED_MAYBE`, surfaced because there it does not
/// retry). That is a wider retry than this one, on a promise the wire did
/// not make; the difference is a call that fails here once, honestly, and
/// recovers on the next.
#[derive(Debug)]
pub struct Connection {
    stream: Stream,
    /// The endpoint actually dialed, which is not always the profile's own
    /// address: failover may have landed on a `TAG_ALTERNATE_IIOP_ADDRESS`.
    /// [`pool`] keys on this, so it has to be what the socket connected to
    /// rather than what the IOR asked for first.
    endpoint: (String, u16),
    object_key: Vec<u8>,
    version: Version,
    endian: Endian,
    next_id: u32,
    max_message_size: usize,
    /// Set once framing can no longer be trusted. A desynchronized stream must
    /// be discarded: the next read would take payload bytes as a GIOP header.
    poisoned: bool,
    /// What §7.10.2 produced for `char` data on this connection.
    char_codeset: CharCodeset,
    /// Set by [`Connection::convert_chars`]: a caller has taken the converter
    /// and undertakes to encode every `string` argument through it. Without
    /// this, an agreement on anything but UTF-8 refuses to send.
    caller_converts_chars: bool,
    /// Whether the `CodeSets` context still needs to go out.
    codeset_context_pending: bool,
    /// Body size above which outbound messages are fragmented.
    fragment_threshold: usize,
    /// Largest number of fragments any one reply arrived in.
    max_reply_fragments: usize,
    /// The redirect most recently followed, if any. See
    /// [`Connection::forwarded`].
    forwarded: Option<Forward>,
    /// The reference this connection stands for: what it was dialled from,
    /// or the last permanent forward. What §9.6's restart dials. See
    /// [`Connection::origin`].
    origin: Ior,
    /// The TLS policy this connection was dialed with, if any. Kept so a
    /// `LOCATION_FORWARD` is followed at the same security level: a
    /// connection whose caller demanded TLS must never chase a redirect back
    /// down to cleartext.
    #[cfg(feature = "ssliop")]
    tls_config: Option<std::sync::Arc<rustls::ClientConfig>>,
}

impl Connection {
    /// Connects to an IOR, trying every endpoint it names until one answers.
    ///
    /// A deployed IOR routinely names several endpoints — a profile per
    /// server address, plus `TAG_ALTERNATE_IIOP_ADDRESS` hints — precisely so
    /// a client survives the first one being down. The dial order is the
    /// IOR's own: each profile's host and port, then that profile's
    /// alternates, then the next profile. A connect failure (refused, timed
    /// out, unresolvable) moves to the next endpoint; only when every one has
    /// failed does this return [`Error::AllEndpointsFailed`], which carries
    /// the count and the last endpoint's reason.
    ///
    /// The GIOP version is still negotiated per profile, because each profile
    /// advertises its own IIOP version — an alternate address inherits its
    /// profile's version, being only another route to the same profile.
    ///
    /// This dials cleartext endpoints only, even when a profile also
    /// advertises `TAG_SSL_SEC_TRANS`: a caller that asked for cleartext gets
    /// cleartext. Upgrading silently would be worse than useless — without a
    /// caller-supplied verification policy there is nothing to verify the
    /// peer against, and an unverified TLS session only *looks* safer.
    /// Dialing the advertised TLS endpoint is `Connection::connect_tls`
    /// (feature `ssliop`), which takes that policy explicitly.
    pub fn connect(ior: &Ior, timeout: Duration) -> Result<Self> {
        let mut tried = 0usize;
        let mut last: Option<Error> = None;
        for p in &ior.profiles {
            for (host, port) in p.endpoints() {
                tried += 1;
                match Self::connect_endpoint(p, &host, port, timeout) {
                    Ok(mut conn) => {
                        // The whole IOR, not the one profile that answered:
                        // a §9.6 restart gets the same failover this dial had.
                        conn.origin = ior.clone();
                        return Ok(conn);
                    }
                    Err(e) => last = Some(e),
                }
            }
        }
        match last {
            Some(e) => Err(Error::AllEndpointsFailed { tried, last: Box::new(e) }),
            // No endpoint was even tried: the IOR had no IIOP profile at all.
            None => Err(Error::NoIiopProfile),
        }
    }

    /// Connects to a specific profile's own address, with no failover.
    pub fn connect_to(p: &IiopProfile, timeout: Duration) -> Result<Self> {
        Self::connect_endpoint(p, &p.host, p.port, timeout)
    }

    /// Connects to the TLS endpoint(s) an IOR advertises, verifying the peer
    /// per `tls_config`.
    ///
    /// The failover order is [`Connection::connect`]'s — profiles in IOR
    /// order — restricted to profiles that advertise `TAG_SSL_SEC_TRANS`,
    /// each dialed at its [`ssliop::ssl_endpoint`]. When no profile
    /// advertises one this returns [`Error::NoTlsEndpoint`] rather than
    /// falling back to cleartext, for the same reason [`Connection::connect`]
    /// never upgrades: the transport the caller asked for is the transport
    /// they get.
    ///
    /// The server name presented for SNI and certificate verification is the
    /// profile's host — the SSLIOP component carries only a port precisely
    /// because the TLS listener is another port of the server the profile
    /// already names. Trust is the caller's to configure: which roots to
    /// accept and whether to present a client certificate are deployment
    /// policy, so they arrive in `tls_config` instead of being decided here.
    ///
    /// A `LOCATION_FORWARD` received over this connection is followed with
    /// the same `tls_config`, never downgraded to cleartext.
    ///
    /// # What this has been measured against
    ///
    /// An in-process rustls peer only (`tests/ssliop_tls.rs`). No
    /// SSLIOP-speaking ORB — omniORB's sslTP, JacORB's SSL transport — has
    /// been exercised yet; that fixture is a future batch. What is verified
    /// today is the TLS layer and GIOP framing pass-through, not peer
    /// interop.
    #[cfg(feature = "ssliop")]
    pub fn connect_tls(
        ior: &Ior,
        timeout: Duration,
        tls_config: std::sync::Arc<rustls::ClientConfig>,
    ) -> Result<Self> {
        let mut tried = 0usize;
        let mut last: Option<Error> = None;
        for p in &ior.profiles {
            let Some((host, port)) = ssliop::ssl_endpoint(p) else { continue };
            tried += 1;
            match Self::connect_tls_endpoint(p, &host, port, timeout, &tls_config) {
                Ok(mut conn) => {
                    conn.origin = ior.clone();
                    return Ok(conn);
                }
                Err(e) => last = Some(e),
            }
        }
        match last {
            Some(e) => Err(Error::AllEndpointsFailed { tried, last: Box::new(e) }),
            None if ior.profiles.is_empty() => Err(Error::NoIiopProfile),
            None => Err(Error::NoTlsEndpoint),
        }
    }

    /// Dials one profile's advertised TLS endpoint and completes the
    /// handshake.
    #[cfg(feature = "ssliop")]
    fn connect_tls_endpoint(
        p: &IiopProfile,
        host: &str,
        port: u16,
        timeout: Duration,
        config: &std::sync::Arc<rustls::ClientConfig>,
    ) -> Result<Self> {
        let mut tcp = dial_configured(host, port, timeout)?;
        let name = rustls::pki_types::ServerName::try_from(host.to_owned()).map_err(|_| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("profile host {host:?} is not a valid TLS server name"),
            ))
        })?;
        let mut tls = rustls::ClientConnection::new(std::sync::Arc::clone(config), name)
            .map_err(Error::Tls)?;
        // Complete the handshake here, so a refusal — wrong CA, a peer that
        // is not speaking TLS at all — fails the *connect*, bounded by the
        // socket timeouts set above, instead of surfacing as a framing error
        // on the first request. rustls reports handshake and certificate
        // failures through the I/O that carried them, hence `Error::Io`.
        while tls.is_handshaking() {
            tls.complete_io(&mut tcp)?;
        }
        let mut conn = Self::from_stream(
            p,
            (host.to_owned(), port),
            Stream::Tls(Box::new(rustls::StreamOwned::new(tls, tcp))),
        );
        conn.tls_config = Some(std::sync::Arc::clone(config));
        Ok(conn)
    }

    /// Connects to one endpoint, taking everything except the address —
    /// version, object key, components — from the profile it belongs to.
    fn connect_endpoint(p: &IiopProfile, host: &str, port: u16, timeout: Duration) -> Result<Self> {
        Ok(Self::from_stream(
            p,
            (host.to_owned(), port),
            Stream::Plain(dial_configured(host, port, timeout)?),
        ))
    }

    /// Builds the connection state over an established transport, taking
    /// everything except the transport — version, object key, codeset
    /// negotiation — from the profile. Shared by the plain and TLS paths so
    /// the two cannot drift apart in anything but the transport itself.
    fn from_stream(p: &IiopProfile, endpoint: (String, u16), stream: Stream) -> Self {
        let char_codeset = negotiated_char_codeset(p);
        let codeset_context_pending = char_codeset.agreed().is_some();

        Self {
            stream,
            endpoint,
            object_key: p.object_key.clone(),
            version: Version::negotiate(p.version),
            endian: Endian::native(),
            next_id: 1,
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
            poisoned: false,
            char_codeset,
            caller_converts_chars: false,
            codeset_context_pending,
            fragment_threshold: DEFAULT_FRAGMENT_THRESHOLD,
            max_reply_fragments: 1,
            forwarded: None,
            // The profile alone until the constructor that has the whole
            // IOR overwrites it — `connect_to` has only the profile, and
            // that is then exactly what it restarts from.
            origin: Ior { type_id: String::new(), profiles: vec![p.clone()] },
            #[cfg(feature = "ssliop")]
            tls_config: None,
        }
    }

    /// The converter for `char`/`string` data on this connection.
    ///
    /// Falls back to ISO-8859-1, which is what §7.10.2.5 specifies when no
    /// context is negotiated. It is `Copy`, so take it before calling
    /// [`Connection::invoke`] and use it inside the closure.
    ///
    /// **Reading this does not make it apply.** It is the negotiated value, for
    /// reporting; a caller that intends to encode through it calls
    /// [`Connection::convert_chars`] instead, which is what tells the
    /// connection that its declaration will be honoured.
    pub fn char_converter(&self) -> codeset::Converter {
        self.char_codeset.agreed().unwrap_or_else(|| {
            codeset::Converter::new(codeset::CodeSetId::ISO_8859_1)
                .expect("ISO-8859-1 is always supported")
        })
    }

    /// The narrow-text codec for this connection's streams, or `None` for
    /// UTF-8.
    ///
    /// `None` is not "we failed to negotiate" — that is
    /// [`CharCodeset::Incompatible`], which refuses the call. This is "the
    /// agreement is UTF-8, so the stream's default is already right", and it
    /// is what keeps every call that existed before D009 byte-identical.
    ///
    /// Unlike [`Connection::char_converter`], **reading this does make it
    /// apply**: the value goes onto the request body's encoder.
    pub(crate) fn narrow_codec(&self) -> Option<std::sync::Arc<dyn orbweaver_cdr::TextCodec>> {
        crate::mux::stream_codec(&self.char_codeset, self.version)
    }

    /// Takes responsibility for converting `char` data, and returns the
    /// converter to do it with.
    ///
    /// Every `string` argument written after this **must** be encoded through
    /// the returned [`codeset::Converter`] and written with
    /// `Encoder::put_string_bytes`; `put_str` writes UTF-8 and would put the
    /// connection back in the state this call is here to leave. That obligation
    /// cannot be checked from here — a closure writing octets is opaque — so
    /// this is an undertaking, and the alternative to undertaking it is not
    /// calling this.
    ///
    /// Without it, a connection that agreed on anything but UTF-8 refuses to
    /// send at all ([`Error::CodesetNotApplied`]). That refusal is the fix: the
    /// behaviour it replaced was to announce ISO-8859-1 in the `CodeSets`
    /// context and then put UTF-8 octets under it, which no peer can detect and
    /// every peer decodes wrongly.
    ///
    /// Fails with [`Error::CodesetIncompatible`] when §7.10.2.6 found nothing
    /// both sides accept: there is no converter to hand out, and pretending
    /// otherwise would hide the one fact the caller needs.
    pub fn convert_chars(&mut self) -> Result<codeset::Converter> {
        let c = self.char_codeset.usable()?;
        self.caller_converts_chars = true;
        Ok(c)
    }

    /// The GIOP version negotiated for this connection.
    pub fn version(&self) -> Version {
        self.version
    }

    /// Caps the version spoken on this connection.
    ///
    /// §9.4.1 forbids exceeding what the peer advertised, so this can only
    /// lower the negotiated version, never raise it. Exists because the
    /// version-conditional paths — `wstring` lengths above all — are otherwise
    /// only reachable against a peer that happens to be old.
    pub fn cap_version(&mut self, max: Version) {
        if max < self.version {
            self.version = max;
        }
    }

    /// Byte order this connection writes. Defaults to native.
    pub fn set_endian(&mut self, endian: Endian) {
        self.endian = endian;
    }

    /// Overrides the inbound message ceiling.
    pub fn set_max_message_size(&mut self, bytes: usize) {
        self.max_message_size = bytes;
    }

    /// Overrides the outbound fragmentation threshold.
    pub fn set_fragment_threshold(&mut self, bytes: usize) {
        self.fragment_threshold = bytes;
    }

    /// The most fragments any single reply on this connection arrived in.
    ///
    /// Exists so a test can prove the peer actually fragmented. "We exercised
    /// reassembly" is only true if something was reassembled, and a peer that
    /// quietly sent whole messages would otherwise produce a passing run that
    /// tested nothing.
    pub fn max_reply_fragments(&self) -> usize {
        self.max_reply_fragments
    }

    /// The redirect this connection most recently followed, or `None` if it
    /// is still talking to the reference it dialled.
    ///
    /// [`Connection::invoke`] follows every forward transparently and — as
    /// §9.4.3.2 permits — keeps talking to the new endpoint afterwards, so
    /// from the returned `Reply` a caller cannot tell whether a redirect
    /// happened, let alone whether the servant said *permanent*. This is how
    /// it can tell. A permanent forward is the servant's leave to replace the
    /// reference the caller holds; a temporary one is not, and a caller that
    /// republishes a temporary target as if it were the object's address is
    /// publishing something the servant did not say.
    ///
    /// Only the last hop of a chain is kept: it is the one this connection is
    /// now on. It goes back to `None` when a §9.6 restart lands the
    /// connection at [`Connection::origin`] again — see the type's docs.
    pub fn forwarded(&self) -> Option<&Forward> {
        self.forwarded.as_ref()
    }

    /// The reference this connection stands for, and the address §9.6's
    /// restart dials: the IOR it was connected from — the whole IOR, so a
    /// restart has the failover the first dial had — or, after a
    /// `LOCATION_FORWARD_PERM`, the forwarded-to IOR, since a permanent
    /// forward is the servant's word that the new address is the one to
    /// keep. A temporary forward never changes it: that is the difference
    /// between the two statuses at this level.
    pub fn origin(&self) -> &Ior {
        &self.origin
    }

    /// The object key extracted from the IOR.
    pub fn object_key(&self) -> &[u8] {
        &self.object_key
    }

    /// The host and port this connection actually reached.
    ///
    /// Not necessarily the first address in the IOR: failover may have moved
    /// on to a later profile or to a `TAG_ALTERNATE_IIOP_ADDRESS`. [`pool`]
    /// keys on the answer, and keying on the *requested* address would file
    /// two connections to one server under two names.
    pub fn endpoint(&self) -> (&str, u16) {
        (&self.endpoint.0, self.endpoint.1)
    }

    /// Whether framing is still trustworthy.
    pub fn is_usable(&self) -> bool {
        !self.poisoned
    }

    fn next_request_id(&mut self) -> u32 {
        let id = self.next_id;
        // §9.4.2.1 forbids reusing an id whose request is still outstanding.
        // Wrapping past zero is astronomically unlikely here but must not
        // panic in a debug build.
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    /// The byte order this connection encodes requests in.
    pub fn endian(&self) -> Endian {
        self.endian
    }

    /// Asks the target whether it knows this connection's object, without
    /// invoking anything (§9.4.5 `LocateRequest`).
    ///
    /// What `_non_existent` answers at the object layer, this answers at the
    /// message layer — and unlike an invocation it cannot have side effects,
    /// which is what makes it safe to use as a probe.
    pub fn locate(&mut self) -> Result<LocateResult> {
        let key = self.object_key.clone();
        self.locate_key(&key)
    }

    /// As [`Connection::locate`], probing an arbitrary object key.
    ///
    /// Exists so a harness can prove the *negative* answer: a locate that can
    /// only ever probe the key it connected with can never show that a bogus
    /// key is refused, and an unmeasured refusal is not a refusal.
    pub fn locate_key(&mut self, object_key: &[u8]) -> Result<LocateResult> {
        if self.poisoned {
            return Err(Error::Desynchronized);
        }
        let id = self.next_request_id();
        let msg = encode_locate_request(self.version, self.endian, id, object_key)?;
        self.stream.write_all(&msg).inspect_err(|_| self.poisoned = true)?;
        self.stream.flush().inspect_err(|_| self.poisoned = true)?;

        let raw = match read_message(&mut self.stream, self.max_message_size) {
            Ok(m) => m,
            Err(e) => {
                self.poisoned = true;
                return Err(e);
            }
        };
        let (reply_id, result) = decode_locate_reply(raw).inspect_err(|_| self.poisoned = true)?;
        if reply_id != id {
            self.poisoned = true;
            return Err(Error::Desynchronized);
        }
        Ok(result)
    }

    /// Sends a raw §9.4.4 `CancelRequest` for `request_id`.
    ///
    /// Advisory by specification: the target MAY ignore it, and no reply
    /// ever correlates with it, so a successful return means only that the
    /// bytes were written — the same promise as [`Connection::invoke_oneway`].
    ///
    /// Honesty note on what this can be used for: this connection holds at
    /// most one request in flight and blocks until its reply arrives, so
    /// from the public API there is never an id mid-flight to cancel. The
    /// property that *is* measurable — and that `spike-cancel` measures
    /// against real ORBs — is what the peer does next: it either ignores
    /// the message (the next invoke on this connection still works) or
    /// refuses it and closes, in which case the next invoke must fail
    /// cleanly on a poisoned connection. A wrapper shaped like
    /// `cancel_last()` would pretend to a capability the invoker does not
    /// have, which is why the raw message is exposed instead.
    ///
    /// Measured peer behaviour: omniORB 4.3.4 ignores a GIOP 1.2
    /// `CancelRequest` but closes the connection on a 1.0 or 1.1 one, even
    /// as the first message on a fresh connection — so below 1.2, expect
    /// the invoke after a cancel to return an error and plan to reconnect.
    pub fn cancel(&mut self, request_id: u32) -> Result<()> {
        if self.poisoned {
            return Err(Error::Desynchronized);
        }
        let msg = encode_cancel_request(self.version, self.endian, request_id)?;
        self.stream.write_all(&msg).inspect_err(|_| self.poisoned = true)?;
        self.stream.flush().inspect_err(|_| self.poisoned = true)?;
        Ok(())
    }

    /// Invokes a `oneway` operation: sends the request and does not wait.
    ///
    /// Not `invoke` with a flag, because the two differ in what the caller may
    /// conclude. A oneway carries no reply, so there is nothing to correlate
    /// and no `LOCATION_FORWARD` to follow — §9.4.3.2's redirect needs a reply
    /// to travel in, which is why `Server` refuses to forward one either. A
    /// successful return here means the bytes were written, and nothing more.
    pub fn invoke_oneway<F>(&mut self, operation: &str, write_args: F) -> Result<()>
    where
        F: Fn(&mut Encoder),
    {
        guarded::assert_nothing_held("a oneway invocation");
        if self.poisoned {
            return Err(Error::Desynchronized);
        }
        let id = self.next_request_id();
        let msg = encode_request(
            self.version,
            self.endian,
            id,
            &self.object_key,
            operation,
            false,
            write_args,
        )?;
        for piece in fragment_message(msg, self.fragment_threshold)? {
            self.stream.write_all(&piece).inspect_err(|_| self.poisoned = true)?;
        }
        self.stream.flush().inspect_err(|_| self.poisoned = true)?;
        Ok(())
    }

    /// Invokes `operation`, writing arguments via `write_args`.
    ///
    /// Follows `LOCATION_FORWARD` transparently, as §9.4.3.2 requires, up to
    /// [`MAX_FORWARD_HOPS`]; and when a temporarily forwarded-to endpoint
    /// has gone, restarts at [`Connection::origin`] as §9.6 requires — under
    /// exactly the conditions the type's docs list, which are the conditions
    /// under which the request provably did not run. A dial made here, for a
    /// forward or a restart, waits [`FOLLOW_TIMEOUT`].
    pub fn invoke<F>(&mut self, operation: &str, write_args: F) -> Result<Reply>
    where
        F: Fn(&mut Encoder),
    {
        // Waiting for a reply is the longest block a servant can take, so this
        // is where holding a lock hurts most. See `crate::guarded`.
        guarded::assert_nothing_held("an invocation");
        let mut fallback = Fallback::Reuse;
        for _ in 0..MAX_FORWARD_HOPS {
            // Before anything is allocated or written: the `CodeSets` context
            // and the octets that follow it must describe the same bytes.
            // Checked per attempt rather than at connect time so the failure
            // names the call that would have carried the mismatch, and so a
            // hop or a restart — each a new connection with its own
            // agreement — is never sent to with the check skipped.
            self.char_codeset.may_send(self.caller_converts_chars)?;
            match self.invoke_once(operation, &write_args) {
                Ok(Outcome::Done(reply)) => return Ok(reply),
                Ok(Outcome::Forwarded(forward)) => self.follow(forward)?,
                Err(AttemptFailed { error, unsent }) => {
                    // §9.6's restart, and only under its conditions: a
                    // temporary forward stands, the *connection* failed (it
                    // is poisoned — an encode error or a codeset refusal is
                    // this side's, not the location's), the request did not
                    // run, and this call has not spent its restart yet.
                    // Everything else is the caller's error to see.
                    let temporary = matches!(self.forwarded, Some(Forward::Temporary(_)));
                    if !unsent || !self.poisoned || !temporary {
                        return Err(error);
                    }
                    match fallback {
                        Fallback::Reuse => {
                            fallback = Fallback::Restart;
                            let via = self.forwarded.as_ref().map(|f| f.ior().clone());
                            let Some(via) = via else { return Err(error) };
                            // "can attempt to reuse the forwarding
                            // information it has": one redial of the
                            // forwarded-to endpoint. Where it is stays as it
                            // was — the same endpoint. "but, if that fails":
                            // straight on to the origin, in this same call.
                            if self.move_to(&via).is_ok() {
                                continue;
                            }
                            fallback = Fallback::Spent;
                            self.restart_at_origin()?;
                        }
                        Fallback::Restart => {
                            fallback = Fallback::Spent;
                            self.restart_at_origin()?;
                        }
                        Fallback::Spent => return Err(error),
                    }
                }
            }
        }
        Err(Error::TooManyForwards)
    }

    /// Moves to a forwarded-to reference and records the forward — the hop
    /// §9.4.3.2 describes. A permanent forward also becomes the origin
    /// (§9.6's *may replace the old IOR*); a temporary one leaves it alone.
    fn follow(&mut self, forward: Forward) -> Result<()> {
        self.move_to(forward.ior())?;
        if forward.is_permanent() {
            self.origin = forward.ior().clone();
        }
        self.forwarded = Some(forward);
        Ok(())
    }

    /// "It shall restart the location process using the original address":
    /// redials [`Connection::origin`]. Where it then is, is the origin, so
    /// no forward stands any more — until the origin says otherwise.
    fn restart_at_origin(&mut self) -> Result<()> {
        let origin = self.origin.clone();
        self.move_to(&origin)?;
        self.forwarded = None;
        Ok(())
    }

    /// Replaces this connection with a fresh one to `ior`, keeping everything
    /// the caller decided — byte order, converter, TLS policy — and the
    /// origin and the standing forward. On a dial failure `self` is left as
    /// it was, poisoned or not.
    fn move_to(&mut self, ior: &Ior) -> Result<()> {
        // A forwarded reference is a full IOR and may itself name several
        // endpoints, so it gets the same failover as the original connect
        // did — over the same transport: a TLS connection follows the
        // forward with the policy it was dialed with, and fails rather than
        // downgrade to cleartext if the new IOR advertises no TLS endpoint.
        #[cfg(feature = "ssliop")]
        let next = match &self.tls_config {
            Some(cfg) => Self::connect_tls(ior, FOLLOW_TIMEOUT, cfg.clone())?,
            None => Self::connect(ior, FOLLOW_TIMEOUT)?,
        };
        #[cfg(not(feature = "ssliop"))]
        let next = Self::connect(ior, FOLLOW_TIMEOUT)?;
        let endian = self.endian;
        let converting = self.caller_converts_chars;
        let origin = self.origin.clone();
        let forwarded = self.forwarded.take();
        // §7.10.2.5 negotiates per *connection*, and a forward is a
        // different connection. A caller that took a converter is
        // still encoding through the one it holds, so the new
        // target agreeing on something else is a mismatch the
        // caller cannot see: it wrote its octets before the
        // redirect existed.
        let before = self.char_codeset.agreed().map(|c| c.id());
        *self = next;
        self.endian = endian;
        self.origin = origin;
        self.forwarded = forwarded;
        if converting {
            let after = self.char_codeset.agreed().map(|c| c.id());
            if after != before {
                return Err(Error::CodesetNotApplied {
                    // §7.10.2.5: absent an agreement the
                    // transmission codeset *is* ISO-8859-1, so this
                    // names what the new connection would transmit
                    // under rather than inventing a placeholder.
                    negotiated: after.unwrap_or(codeset::CodeSetId::ISO_8859_1),
                });
            }
            self.caller_converts_chars = true;
        }
        Ok(())
    }

    /// One attempt at one request on the connection as it stands. The error
    /// says whether the request provably did not run — see [`AttemptFailed`]
    /// — which is what [`Connection::invoke`] needs to know before it may
    /// re-issue it anywhere.
    fn invoke_once<F>(
        &mut self,
        operation: &str,
        write_args: &F,
    ) -> std::result::Result<Outcome, AttemptFailed>
    where
        F: Fn(&mut Encoder),
    {
        if self.poisoned {
            // Nothing was written for *this* request: whatever poisoned the
            // connection was reported to the call it happened to.
            return Err(AttemptFailed { error: Error::Desynchronized, unsent: true });
        }
        let id = self.next_request_id();
        // §7.10.2.5 negotiates per connection, so the context goes on the
        // first request only. Sending it again on a later request would risk
        // MARSHAL minor 9 for conflicting contexts on one connection.
        let contexts = if self.codeset_context_pending {
            match self.char_codeset.agreed() {
                Some(c) => vec![ServiceContext {
                    id: codeset::SERVICE_ID_CODE_SETS,
                    data: codeset::CodeSetContext {
                        char_data: c.id(),
                        wchar_data: codeset::CodeSetId::UTF_16,
                    }
                    .encode(self.endian)
                    .map_err(AttemptFailed::unsent)?,
                }],
                None => Vec::new(),
            }
        } else {
            Vec::new()
        };

        let msg = encode_request_with_contexts(
            self.version,
            self.endian,
            id,
            &self.object_key,
            operation,
            true,
            &contexts,
            // The agreement this connection reached, carried by the
            // stream rather than remembered by the caller. `None` when the
            // negotiation produced UTF-8 or produced nothing, which keeps
            // every existing call byte-identical.
            self.narrow_codec(),
            write_args,
        )
        .map_err(AttemptFailed::unsent)?;
        self.codeset_context_pending = false;
        // A failed write poisons too: a partially-written request leaves the
        // *outbound* half of the stream unframeable, exactly as unread bytes
        // do the inbound half. This was the one send path that did not poison
        // — found when a peer that closes on CancelRequest (omniORB below
        // GIOP 1.2) made the next write the first thing to fail.
        //
        // And a failed write is *unsent*: a GIOP message the peer never
        // finished reading cannot be dispatched, and this connection is not
        // reused, so the rest of it never arrives — the same reasoning as
        // [`mux::Failed::unsent`].
        for piece in
            fragment_message(msg, self.fragment_threshold).map_err(AttemptFailed::unsent)?
        {
            self.stream
                .write_all(&piece)
                .inspect_err(|_| self.poisoned = true)
                .map_err(|e| AttemptFailed::unsent(e.into()))?;
        }
        self.stream
            .flush()
            .inspect_err(|_| self.poisoned = true)
            .map_err(|e| AttemptFailed::unsent(e.into()))?;

        // Exactly one message answers one request. This was written as a loop,
        // which said the opposite — that some messages could be skipped and the
        // read retried — while every branch in fact returned. Nothing here may
        // loop, and multiplexing does not change that: with one outstanding
        // request, a message that is not our reply means our accounting is
        // wrong, and reading past it would compound the error rather than
        // recover from it. The loop that *is* correct — read, file under a
        // request id, look again — needs somewhere to file, which is exactly
        // what [`mux::Mux`] adds and this type deliberately does not have.
        //
        // Any framing failure from here leaves unread bytes behind, so every
        // error path poisons the connection.
        //
        // The reader's reason is passed through untouched, which matters most
        // for the one case that looks like it wants translating:
        // [`Error::InterruptedMidReassembly`] with a `CloseConnection` is a
        // teardown, but it is *not* [`Error::ConnectionClosed`] and must not be
        // rewritten into one on the way out — this request's reply had already
        // begun, so §13.5.1's "was not processed" does not describe it, and a
        // caller re-sending on that promise would run the operation twice. The
        // fact a caller needs is on the value: `is_orderly_close` says re-dial,
        // the variant says do not assume this call went unrun.
        //
        // From here on the whole request is with the peer, so nothing below
        // is *unsent* except the one message that says so: an EOF, a reset or
        // a timeout while waiting is a request whose completion is unknown.
        let raw = match read_message(&mut self.stream, self.max_message_size) {
            Ok(m) => m,
            Err(e) => {
                self.poisoned = true;
                return Err(AttemptFailed::unknown(e));
            }
        };
        self.max_reply_fragments = self.max_reply_fragments.max(raw.fragments);
        match raw.msg_type {
            MsgType::Reply => {
                let mut reply = decode_reply(raw)
                    .inspect_err(|_| self.poisoned = true)
                    .map_err(AttemptFailed::unknown)?;
                // §7.1: one agreement for a call and its answer. The reply is
                // encoded under what this connection negotiated, so it is read
                // under the same thing rather than under whatever a later
                // renegotiation might say.
                reply.set_codec(self.narrow_codec());
                if reply.request_id != id {
                    self.poisoned = true;
                    return Err(AttemptFailed::unknown(Error::Desynchronized));
                }
                self.interpret(reply).map_err(AttemptFailed::unknown)
            }
            MsgType::CloseConnection => {
                // §9.4.7: the request was not processed and is safe to
                // re-send on a fresh connection.
                self.poisoned = true;
                Err(AttemptFailed::unsent(Error::ConnectionClosed))
            }
            other => {
                self.poisoned = true;
                Err(AttemptFailed::unknown(Error::UnexpectedMessage(other)))
            }
        }
    }

    fn interpret(&mut self, reply: Reply) -> Result<Outcome> {
        match reply.status {
            ReplyStatus::NoException => Ok(Outcome::Done(reply)),
            ReplyStatus::SystemException => {
                let mut b = reply.body()?;
                Err(Error::SystemException {
                    id: b.get_string().unwrap_or_else(|_| "<unreadable>".into()),
                    minor: b.get_u32().unwrap_or(0),
                    completed: b.get_u32().unwrap_or(0),
                })
            }
            ReplyStatus::UserException => {
                // Read the id but hand the reply back, so the caller can
                // decode the exception's members. Consuming and dropping the
                // body left callers able to see that a call failed but never
                // why.
                let id = reply.body()?.get_string().unwrap_or_else(|_| "<unreadable>".into());
                Err(Error::UserException { id, reply: Box::new(reply) })
            }
            ReplyStatus::LocationForward => {
                let mut b = reply.body()?;
                Ok(Outcome::Forwarded(Forward::Temporary(Ior::read_from(&mut b)?)))
            }
            ReplyStatus::LocationForwardPerm => {
                let mut b = reply.body()?;
                Ok(Outcome::Forwarded(Forward::Permanent(Ior::read_from(&mut b)?)))
            }
            ReplyStatus::NeedsAddressingMode => {
                // Answering this requires the ProfileAddr and ReferenceAddr
                // target dispositions, which are not implemented. Failing is
                // correct; pretending the body is a return value is not.
                Err(Error::UnexpectedMessage(MsgType::Reply))
            }
        }
    }

    /// Invokes an operation that takes no arguments.
    pub fn invoke_nullary(&mut self, operation: &str) -> Result<Reply> {
        self.invoke(operation, |_| {})
    }
}

enum Outcome {
    Done(Reply),
    Forwarded(Forward),
}

/// One attempt's failure, and the one fact [`Connection::invoke`] must have
/// before it may issue the request again anywhere: whether the peer provably
/// did not run it.
///
/// `unsent` is set only where the specification, not optimism, says so:
/// nothing — or not all of the message — was written, so the peer cannot
/// have dispatched it; or the peer answered `CloseConnection`, which §13.5.1
/// defines as *"were not processed, and may be safely resent"*. It is never
/// set for a failure after a complete request went out with no such
/// message back — that request may have run, and reporting it as unsent
/// would run it twice. The same rule as [`mux::Failed::unsent`], for the
/// same reason.
struct AttemptFailed {
    error: Error,
    unsent: bool,
}

impl AttemptFailed {
    fn unsent(error: Error) -> Self {
        Self { error, unsent: true }
    }
    fn unknown(error: Error) -> Self {
        Self { error, unsent: false }
    }
}

/// How much of §9.6's fallback one call has left. In order: reuse the
/// forwarding information (redial where the forward pointed), restart at the
/// origin, and then nothing — a call that failed unsent at the origin too is
/// reported, not looped.
#[derive(Clone, Copy)]
enum Fallback {
    Reuse,
    Restart,
    Spent,
}

/// What §7.10.2 produced for `char` data against one profile.
///
/// Three outcomes, not two, and the third is the one that used to be missing.
/// A profile that publishes no component and a profile whose component we
/// cannot agree with were both collapsed into "nothing negotiated", which then
/// took §7.10.2.5's *absent context* path — so a peer that had said, in its own
/// IOR, that it reads Latin-1 and nothing else received UTF-8 octets under a
/// specified default of ISO-8859-1 and no way to know.
#[derive(Debug, Clone)]
pub(crate) enum CharCodeset {
    /// The profile publishes no `TAG_CODE_SETS`. §7.10.2.5 then makes the
    /// transmission codeset ISO-8859-1 and no context is sent.
    Unspecified,
    /// A codeset both sides accept, per §7.10.2.6.
    Agreed(codeset::Converter),
    /// The profile publishes a component and nothing in it was mutually
    /// acceptable, or it named a codeset this build cannot convert.
    Incompatible(codeset::NegotiationError),
}

impl CharCodeset {
    /// The agreed converter, if negotiation reached one.
    pub(crate) fn agreed(&self) -> Option<codeset::Converter> {
        match self {
            CharCodeset::Agreed(c) => Some(*c),
            _ => None,
        }
    }

    /// The converter a caller may encode through, or why there is none.
    pub(crate) fn usable(&self) -> Result<codeset::Converter> {
        match self {
            CharCodeset::Agreed(c) => Ok(*c),
            CharCodeset::Incompatible(e) => Err(Error::CodesetIncompatible(e.clone())),
            // Nothing was negotiated, so §7.10.2.5's default stands and there
            // is nothing to convert *to* beyond it.
            CharCodeset::Unspecified => codeset::Converter::new(codeset::CodeSetId::ISO_8859_1)
                .map_err(Error::CodesetIncompatible),
        }
    }

    /// Whether a request may go out with `caller_converts` as it stands.
    ///
    /// The whole point of the type: a declaration and the octets under it are
    /// checked against each other once, in one place, rather than being assumed
    /// to match by two files that never met.
    pub(crate) fn may_send(&self, caller_converts: bool) -> Result<()> {
        match self {
            // No agreement was made, so nothing was declared. §7.10.2.5's
            // default applies and is left as it was; see the module docs on
            // `codeset` for what that default costs above ASCII.
            CharCodeset::Unspecified => Ok(()),
            CharCodeset::Incompatible(e) => Err(Error::CodesetIncompatible(e.clone())),
            // The one codeset this crate's own writers emit.
            CharCodeset::Agreed(c) if c.id() == codeset::CodeSetId::UTF_8 => Ok(()),
            CharCodeset::Agreed(_) if caller_converts => Ok(()),
            CharCodeset::Agreed(c) => Err(Error::CodesetNotApplied { negotiated: c.id() }),
        }
    }
}

/// The `char` codeset a connection to this profile would negotiate.
///
/// §7.10.2.5 makes this a **per-connection** decision, taken once from the
/// profile and then implied by every string on the wire. That is why it is a
/// function of the profile alone and why [`pool`] puts its answer in the pool
/// key: two references to one endpoint that negotiate different codesets
/// cannot share a connection, because the second one's strings would be
/// encoded under the first one's agreement.
fn negotiated_char_codeset(p: &IiopProfile) -> CharCodeset {
    let mut out = CharCodeset::Unspecified;
    for c in &p.components {
        if c.tag != codeset::TAG_CODE_SETS {
            continue;
        }
        // §9.7.2: a component we cannot read is ignored rather than fatal —
        // which for *this* component means falling back to §7.10.2.5's default,
        // the same position an absent component leaves us in.
        let Ok(info) = codeset::CodeSetComponentInfo::parse(&c.data) else { continue };
        out = match codeset::negotiate(&codeset::client_char_component(), &info.for_char)
            .and_then(codeset::Converter::new)
        {
            Ok(conv) => CharCodeset::Agreed(conv),
            Err(e) => CharCodeset::Incompatible(e),
        };
    }
    out
}

/// The `char` converter a connection to this profile would negotiate, or
/// `None` when nothing was agreed. [`pool::Key`] keys on it.
fn negotiated_char_converter(p: &IiopProfile) -> Option<codeset::Converter> {
    negotiated_char_codeset(p).agreed()
}

/// [`dial`], plus the socket options every connection gets: both timeouts,
/// and Nagle off. One function so the plain and TLS paths cannot diverge in
/// socket behaviour — for TLS the timeouts also bound the handshake, which is
/// what turns "the peer never answered the ClientHello" into an error instead
/// of a hang.
fn dial_configured(host: &str, port: u16, timeout: Duration) -> Result<TcpStream> {
    // The single funnel every outbound connection — cleartext and TLS —
    // passes through, which is why the lock tripwire lives here rather than in
    // each `connect*`. A servant dialling from inside its own lock is the
    // cross-process deadlock `event_server` documents; see `crate::guarded`.
    guarded::assert_nothing_held("connecting to a peer");
    let stream = dial(host, port, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.set_nodelay(true)?;
    Ok(stream)
}

/// Resolves and connects with a real timeout.
///
/// `"host:port".parse::<SocketAddr>()` only succeeds for numeric literals, so
/// routing DNS names through a `parse().unwrap_or_else(connect)` fallback
/// silently dropped the timeout for every hostname — and produced
/// `::1:9999` for IPv6 literals, which resolves to nothing at all.
fn dial(host: &str, port: u16, timeout: Duration) -> Result<TcpStream> {
    let addrs = (host, port).to_socket_addrs().map_err(Error::Io)?.collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            format!("no address for {host}:{port}"),
        )));
    }
    let mut last = None;
    for addr in &addrs {
        match TcpStream::connect_timeout(addr, timeout) {
            Ok(s) => return Ok(s),
            Err(e) => last = Some(e),
        }
    }
    Err(Error::Io(last.expect("addrs was non-empty")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(version: Version, endian: Endian, op: &str, expect_reply: bool) -> Vec<u8> {
        encode_request(version, endian, 42, b"key", op, expect_reply, |_| {}).unwrap()
    }

    #[test]
    fn request_header_layout_is_giop_1_2() {
        let msg = req(Version::V1_2, Endian::Big, "ping", true);
        assert_eq!(&msg[0..4], MAGIC);
        assert_eq!(msg[4], 1);
        assert_eq!(msg[5], 2);
        assert_eq!(msg[6], 0, "big-endian flag");
        assert_eq!(msg[7], MsgType::Request as u8);

        let size = u32::from_be_bytes([msg[8], msg[9], msg[10], msg[11]]) as usize;
        assert_eq!(size, msg.len() - HEADER_LEN, "message_size excludes the header");
        assert_eq!(u32::from_be_bytes([msg[12], msg[13], msg[14], msg[15]]), 42);
        assert_eq!(msg[16], 3, "response_flags requests a reply");
    }

    /// Audit CONFIRMED #6: §9.4.2.1 forbids padding after the header when the
    /// body is empty. The Phase 0 encoder emitted four bytes of it.
    #[test]
    fn empty_body_gets_no_alignment_padding() {
        // "ping": header 12 + id 4 + flags 1 + reserved 3 + target 2 + pad 2
        //         + keylen 4 + "key" 3 + pad 1 + oplen 4 + "ping\0" 5 + pad 3
        //         + ctx 4 = 48. A multiple of 8 by coincidence, so it proves
        //         nothing on its own.
        assert_eq!(req(Version::V1_2, Endian::Big, "ping", true).len(), 48);

        // "abc" lands on 44, which is not a multiple of 8. If the encoder were
        // still aligning unconditionally this would be 48.
        let msg = req(Version::V1_2, Endian::Big, "abc", true);
        assert_eq!(msg.len(), 44);
        assert_ne!(msg.len() % 8, 0, "no padding may follow an empty body");
    }

    #[test]
    fn nonempty_body_is_eight_byte_aligned_in_1_2() {
        let msg = encode_request(Version::V1_2, Endian::Little, 1, b"k", "op", true, |e| {
            e.put_f64(1.5);
        })
        .unwrap();
        let at = msg.len() - 8;
        assert_eq!(at % 8, 0, "body must start 8-aligned, started at {at}");
        assert_eq!(&msg[at..], &1.5f64.to_le_bytes());
    }

    /// Audit CONFIRMED #1/#2: 1.0 and 1.1 put service_context first, use a
    /// boolean response_expected, and never align the body.
    #[test]
    fn request_header_layout_is_giop_1_0() {
        let msg = encode_request(Version::V1_0, Endian::Big, 7, b"k", "op", true, |e| {
            e.put_f64(1.5);
        })
        .unwrap();
        assert_eq!(msg[5], 0, "minor version");
        // service_context count comes first in 1.0
        assert_eq!(u32::from_be_bytes([msg[12], msg[13], msg[14], msg[15]]), 0);
        assert_eq!(u32::from_be_bytes([msg[16], msg[17], msg[18], msg[19]]), 7, "request_id");
        assert_eq!(msg[20], 1, "response_expected is a boolean in 1.0");
        // Body is unaligned in 1.0, so the double sits at the very end.
        assert_eq!(&msg[msg.len() - 8..], &1.5f64.to_be_bytes());
    }

    /// The 1.1 `reserved[3]` field is invisible on the wire for a request.
    ///
    /// `response_expected` always ends at offset 21, and the `object_key`
    /// sequence that follows aligns to 4 regardless — so 1.0 pads 21→24 with
    /// zeros and 1.1 writes three explicit zeros into the same span. The two
    /// encodings are byte-identical apart from the version octet.
    ///
    /// Worth pinning: the obvious expectation is that 1.1 is three bytes
    /// longer, and acting on that would mean "fixing" an encoder that is
    /// already correct.
    #[test]
    fn giop_1_1_reserved_octets_land_in_1_0s_padding() {
        let v10 = encode_request(Version::V1_0, Endian::Big, 7, b"k", "op", true, |_| {}).unwrap();
        let v11 = encode_request(Version::V1_1, Endian::Big, 7, b"k", "op", true, |_| {}).unwrap();

        assert_eq!(v11.len(), v10.len(), "alignment absorbs the reserved field");
        assert_eq!(v10[5], 0);
        assert_eq!(v11[5], 1);
        assert_eq!(&v10[6..], &v11[6..], "identical past the version octet");
        assert_eq!(&v11[21..24], &[0, 0, 0], "reserved octets are zero");
    }

    /// The client encoder against the server decoder, across every version
    /// and both byte orders — two halves of one wire rule, checked against
    /// each other before a peer ever is.
    #[test]
    fn locate_requests_round_trip_through_the_server_decoder() {
        for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
            for endian in [Endian::Big, Endian::Little] {
                let msg = encode_locate_request(version, endian, 77, b"the-key").expect("encodes");
                let raw =
                    read_message(&mut msg.as_slice(), DEFAULT_MAX_MESSAGE_SIZE).expect("frames");
                assert_eq!(raw.msg_type, MsgType::LocateRequest);
                let lr = crate::server::decode_locate_request(raw).expect("decodes");
                assert_eq!(lr.request_id, 77, "{version} {endian:?}");
                assert_eq!(lr.object_key, b"the-key", "{version} {endian:?}");
            }
        }
    }

    /// §9.4.4: a CancelRequest is a header plus the abandoned request id and
    /// nothing else, in every version — the one header this file encodes that
    /// is not version-conditional, which is worth pinning precisely because
    /// everything around it is.
    #[test]
    fn cancel_request_is_a_header_plus_the_abandoned_id() {
        for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
            for endian in [Endian::Big, Endian::Little] {
                let msg = encode_cancel_request(version, endian, 9999).expect("encodes");
                assert_eq!(msg.len(), HEADER_LEN + 4, "{version} {endian:?}");
                let raw =
                    read_message(&mut msg.as_slice(), DEFAULT_MAX_MESSAGE_SIZE).expect("frames");
                assert_eq!(raw.msg_type, MsgType::CancelRequest);
                assert_eq!(raw.version, version);
                let mut d = Decoder::new(&raw.bytes, raw.endian);
                d.seek_to(HEADER_LEN).unwrap();
                assert_eq!(d.get_u32().unwrap(), 9999, "{version} {endian:?}");
            }
        }
    }

    #[test]
    fn locate_replies_decode_in_every_version() {
        use crate::server::{LocateStatus, encode_locate_reply};
        for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
            for endian in [Endian::Big, Endian::Little] {
                for (status, want) in [
                    (LocateStatus::UnknownObject, LocateResult::Unknown),
                    (LocateStatus::ObjectHere, LocateResult::Here),
                ] {
                    let msg = encode_locate_reply(version, endian, 5, status).unwrap();
                    let raw = read_message(&mut msg.as_slice(), DEFAULT_MAX_MESSAGE_SIZE)
                        .expect("frames");
                    let (id, got) = decode_locate_reply(raw).expect("decodes");
                    assert_eq!(id, 5);
                    assert_eq!(got, want, "{version} {endian:?}");
                }
            }
        }
    }

    /// The §9.4.6 asymmetry: a 1.2 LocateReply body is NOT 8-aligned, so a
    /// forwarded IOR starts immediately after the status word. A decoder that
    /// borrowed the Reply alignment rule reads the IOR four bytes late.
    #[test]
    fn a_forwarded_locate_reply_carries_its_ior_unaligned() {
        let ior = Ior {
            type_id: "IDL:m/I:1.0".into(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: "h".into(),
                port: 1,
                object_key: b"k".to_vec(),
                components: Vec::new(),
            }],
        };
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            e.put_bytes(b"GIOP");
            e.put_u8(1);
            e.put_u8(2);
            e.put_u8(if endian == Endian::Little { 1 } else { 0 });
            e.put_u8(MsgType::LocateReply as u8);
            let size_at = e.len();
            e.put_u32(0);
            e.put_u32(9); // request_id
            e.put_u32(2); // OBJECT_FORWARD — body follows with no 8-alignment
            ior.write_to(&mut e).unwrap();
            let size = (e.len() - HEADER_LEN) as u32;
            e.patch_u32(size_at, size);
            let msg = e.finish().unwrap();
            let raw = read_message(&mut msg.as_slice(), DEFAULT_MAX_MESSAGE_SIZE).expect("frames");
            let (id, got) = decode_locate_reply(raw).expect("decodes");
            assert_eq!(id, 9);
            assert_eq!(got, LocateResult::Forward(Box::new(ior.clone())), "{endian:?}");
        }
    }

    #[test]
    fn a_locate_system_exception_surfaces_as_one() {
        let mut e = Encoder::new(Endian::Big);
        e.put_bytes(b"GIOP");
        e.put_u8(1);
        e.put_u8(2);
        e.put_u8(0);
        e.put_u8(MsgType::LocateReply as u8);
        let size_at = e.len();
        e.put_u32(0);
        e.put_u32(3); // request_id
        e.put_u32(4); // LOC_SYSTEM_EXCEPTION
        e.put_str("IDL:omg.org/CORBA/OBJECT_NOT_EXIST:1.0");
        e.put_u32(7);
        e.put_u32(1);
        let size = (e.len() - HEADER_LEN) as u32;
        e.patch_u32(size_at, size);
        let msg = e.finish().unwrap();
        let raw = read_message(&mut msg.as_slice(), DEFAULT_MAX_MESSAGE_SIZE).unwrap();
        let err = decode_locate_reply(raw).unwrap_err();
        assert!(
            matches!(&err, Error::SystemException { id, minor: 7, .. }
                if id.contains("OBJECT_NOT_EXIST")),
            "{err}"
        );
    }

    #[test]
    fn oneway_clears_the_reply_flag() {
        let msg = req(Version::V1_2, Endian::Big, "fire", false);
        assert_eq!(msg[16], 0, "oneway must not request a reply");
        let v10 = req(Version::V1_0, Endian::Big, "fire", false);
        assert_eq!(v10[20], 0, "1.0 response_expected must be false");
    }

    #[test]
    fn version_negotiation_never_exceeds_the_peer() {
        assert_eq!(Version::negotiate(Version::V1_0), Version::V1_0);
        assert_eq!(Version::negotiate(Version::V1_1), Version::V1_1);
        assert_eq!(Version::negotiate(Version::V1_2), Version::V1_2);
        assert_eq!(Version::negotiate(Version { major: 1, minor: 9 }), Version::V1_2);
    }

    /// Audit CONFIRMED #14: statuses 4 and 5 arrived in 1.2.
    #[test]
    fn reply_status_is_version_aware() {
        assert_eq!(ReplyStatus::from_u32(3, Version::V1_0), Some(ReplyStatus::LocationForward));
        assert_eq!(ReplyStatus::from_u32(4, Version::V1_1), None);
        assert_eq!(ReplyStatus::from_u32(4, Version::V1_2), Some(ReplyStatus::LocationForwardPerm));
        assert_eq!(ReplyStatus::from_u32(9, Version::V1_2), None);
    }

    fn build_reply(
        version: Version,
        endian: Endian,
        id: u32,
        status: u32,
        contexts: u32,
    ) -> Vec<u8> {
        let mut e = Encoder::new(endian);
        e.put_bytes(MAGIC);
        e.put_u8(version.major);
        e.put_u8(version.minor);
        e.put_u8(endian.as_flag());
        e.put_u8(MsgType::Reply as u8);
        let size_at = e.len();
        e.put_bytes(&[0, 0, 0, 0]);
        if version.is_1_2_layout() {
            e.put_u32(id);
            e.put_u32(status);
            e.put_u32(contexts);
            for _ in 0..contexts {
                e.put_u32(1);
                e.put_octet_seq(&[0xAB, 0xCD]);
            }
        } else {
            e.put_u32(contexts);
            for _ in 0..contexts {
                e.put_u32(1);
                e.put_octet_seq(&[0xAB, 0xCD]);
            }
            e.put_u32(id);
            e.put_u32(status);
        }
        if version.aligns_body() {
            e.align_to(8);
        }
        e.put_f64(2.25);
        let size = (e.len() - HEADER_LEN) as u32;
        e.patch_u32(size_at, size);
        e.finish().unwrap()
    }

    #[test]
    fn reply_body_offset_preserves_alignment_in_1_2() {
        let raw = build_reply(Version::V1_2, Endian::Big, 99, 0, 0);
        let mut cursor: &[u8] = &raw;
        let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
        let reply = decode_reply(msg).unwrap();
        assert_eq!(reply.request_id, 99);
        assert_eq!(reply.status, ReplyStatus::NoException);
        assert_eq!(reply.body().unwrap().get_f64().unwrap(), 2.25);
    }

    /// Audit CONFIRMED #1, the highest-damage finding: a 1.1 reply carrying a
    /// service context used to be read with the 1.2 field order, turning the
    /// context count into the request id and the request id into the status.
    #[test]
    fn giop_1_1_reply_with_service_context_decodes_correctly() {
        let raw = build_reply(Version::V1_1, Endian::Big, 1, 0, 1);
        let mut cursor: &[u8] = &raw;
        let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
        let reply = decode_reply(msg).unwrap();
        assert_eq!(reply.request_id, 1, "must not read the context count as the id");
        assert_eq!(reply.status, ReplyStatus::NoException, "must not read the id as the status");
        assert_eq!(reply.body().unwrap().get_f64().unwrap(), 2.25);
    }

    #[test]
    fn giop_1_0_reply_decodes_with_the_old_field_order() {
        let raw = build_reply(Version::V1_0, Endian::Little, 5, 0, 2);
        let mut cursor: &[u8] = &raw;
        let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
        let reply = decode_reply(msg).unwrap();
        assert_eq!(reply.request_id, 5);
        assert_eq!(reply.body().unwrap().get_f64().unwrap(), 2.25);
    }

    #[test]
    fn non_giop_traffic_is_rejected() {
        let mut junk: &[u8] = b"HTTP/1.1 200 OK\r\n\r\npadding-to-twelve";
        assert!(matches!(
            read_message(&mut junk, DEFAULT_MAX_MESSAGE_SIZE),
            Err(Error::NotGiop(_))
        ));
    }

    /// Audit HOSTILE #1: twelve bytes used to buy a 4 GiB zeroed allocation,
    /// and an allocation failure aborts the process.
    #[test]
    fn oversized_message_is_refused_before_allocating() {
        let mut hostile: &[u8] = &[b'G', b'I', b'O', b'P', 1, 2, 1, 1, 0xff, 0xff, 0xff, 0xff];
        match read_message(&mut hostile, DEFAULT_MAX_MESSAGE_SIZE) {
            Err(Error::MessageTooLarge { declared, limit }) => {
                assert_eq!(declared, 0xffff_ffff);
                assert_eq!(limit, DEFAULT_MAX_MESSAGE_SIZE);
            }
            other => panic!("expected MessageTooLarge, got {other:?}"),
        }
    }

    /// A GIOP 1.2 header with an arbitrary declared `message_size`.
    fn header_declaring(size: u32) -> Vec<u8> {
        let mut h = MAGIC.to_vec();
        h.extend_from_slice(&[1, 2, 0, 0]); // 1.2, big endian, Request
        h.extend_from_slice(&size.to_be_bytes());
        h
    }

    /// A peer that sends a header, then at most `body` bytes, then goes quiet.
    /// It records what it was *asked* for, which is the observable side of
    /// "committed only as it arrives".
    struct Trickle {
        head: Vec<u8>,
        at: usize,
        body: usize,
        largest_request: usize,
        /// Requests for a whole chunk — one per chunk the reader committed.
        chunks_entered: usize,
    }

    impl Read for Trickle {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.at >= self.head.len() {
                self.largest_request = self.largest_request.max(buf.len());
                if buf.len() == BODY_CHUNK {
                    self.chunks_entered += 1;
                }
            }
            if self.at < self.head.len() {
                let n = buf.len().min(self.head.len() - self.at);
                buf[..n].copy_from_slice(&self.head[self.at..self.at + n]);
                self.at += n;
                return Ok(n);
            }
            let n = buf.len().min(self.body);
            buf[..n].fill(0xAA);
            self.body -= n;
            Ok(n) // 0 once the body runs out: the peer stopped talking
        }
    }

    /// Twelve bytes must not buy a 64 MiB allocation — the Phase 0 audit's
    /// worst finding, guarded in the sequence decoder and, until this test,
    /// not in the reader. Measured before the fix with a counting allocator:
    /// a header declaring 64 MiB and **no body at all** committed and zeroed
    /// 67,108,875 bytes. After it: 65,548.
    ///
    /// A test cannot read `Vec` capacity, and `forbid(unsafe_code)` rules out
    /// instrumenting the allocator in-tree, so what is asserted here is the
    /// property that produces the bound — the reader never asks for, and so
    /// never commits, more than one chunk the peer has not delivered.
    #[test]
    fn a_declared_body_is_committed_only_as_it_arrives() {
        let declared = 64 * 1024 * 1024 - 1;
        let mut peer = Trickle {
            head: header_declaring(declared),
            at: 0,
            body: 1024,
            largest_request: 0,
            chunks_entered: 0,
        };
        let r = read_one_message(&mut peer, DEFAULT_MAX_MESSAGE_SIZE);
        assert!(r.is_err(), "a body that never arrives is not a message");
        assert!(
            peer.largest_request <= BODY_CHUNK,
            "asked for {} bytes in one read; that is what gets committed",
            peer.largest_request
        );
        assert_eq!(
            peer.chunks_entered, 1,
            "committed {} chunks for a peer that sent 1024 body bytes of a declared {declared}",
            peer.chunks_entered
        );
    }

    /// The negative control: chunking must not truncate or reorder an honest
    /// message that is larger than one chunk.
    #[test]
    fn a_body_larger_than_one_chunk_still_reads_back_byte_for_byte() {
        let body: Vec<u8> = (0..(3 * BODY_CHUNK + 7)).map(|i| (i % 251) as u8).collect();
        let mut wire = header_declaring(body.len() as u32);
        wire.extend_from_slice(&body);
        let mut cursor: &[u8] = &wire;
        let msg = read_one_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("reads");
        assert_eq!(msg.bytes, wire);
        assert!(cursor.is_empty(), "read past or short of the message boundary");
    }

    /// An exact multiple of the chunk must not read one chunk too many, and a
    /// body of zero must not read at all.
    #[test]
    fn chunk_boundaries_are_exact() {
        for len in [0usize, 1, BODY_CHUNK - 1, BODY_CHUNK, BODY_CHUNK + 1, 2 * BODY_CHUNK] {
            let body = vec![0x5A; len];
            let mut wire = header_declaring(len as u32);
            wire.extend_from_slice(&body);
            wire.extend_from_slice(b"trailing bytes of the next message");
            let mut cursor: &[u8] = &wire;
            let msg = read_one_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("reads");
            assert_eq!(msg.bytes.len(), HEADER_LEN + len, "len {len}");
            assert_eq!(cursor, b"trailing bytes of the next message", "len {len}");
        }
    }

    /// Audit CONFIRMED #4: the more-fragments bit was masked away, so the
    /// first fragment was decoded as a whole message and the value silently
    /// truncated. Batch 8 reassembles instead — but a stream that *promises*
    /// more and then ends must still error rather than hand back the piece it
    /// managed to read.
    #[test]
    fn incomplete_fragment_stream_errors_rather_than_truncating() {
        let mut frag: &[u8] = &[b'G', b'I', b'O', b'P', 1, 2, 0b11, 1, 0, 0, 0, 0];
        match read_message(&mut frag, DEFAULT_MAX_MESSAGE_SIZE) {
            Err(_) => {}
            Ok(m) => panic!("returned a {}-byte message from an unfinished stream", m.bytes.len()),
        }
    }

    /// Fragmenting and reassembling must reproduce the original byte for byte,
    /// including alignment — a split at the wrong offset shifts every field
    /// after it, and the peer reports garbage at the end rather than an error
    /// where the damage is.
    #[test]
    fn fragment_round_trip_reproduces_the_original() {
        for endian in [Endian::Big, Endian::Little] {
            for payload in [64usize, 1000, 5000, 20000] {
                let msg = encode_request(Version::V1_2, endian, 7, b"k", "blob", true, |e| {
                    e.put_u32(payload as u32);
                    for i in 0..payload {
                        e.put_octet((i % 251) as u8);
                    }
                })
                .unwrap();

                let pieces = fragment_message(msg.clone(), 512).unwrap();
                if msg.len() > 512 {
                    assert!(pieces.len() > 1, "{payload} bytes should have split");
                }
                // Every piece but the last must set the more-fragments bit, and
                // 9.4.9 requires their total length to be divisible by 8.
                for p in &pieces[..pieces.len() - 1] {
                    assert_eq!(p[6] & 0b10, 0b10, "non-final piece must say more follow");
                    assert_eq!(p.len() % 8, 0, "non-final piece must be 8-aligned overall");
                }
                assert_eq!(pieces.last().unwrap()[6] & 0b10, 0, "final piece must not");

                let wire: Vec<u8> = pieces.concat();
                let mut cursor: &[u8] = &wire;
                let back = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
                assert!(!back.more_fragments);
                assert_eq!(back.bytes, msg, "{payload} bytes, {endian:?} did not reassemble");
                assert!(cursor.is_empty(), "reassembly must consume every fragment");
            }
        }
    }

    #[test]
    fn small_messages_are_not_fragmented() {
        let msg =
            encode_request(Version::V1_2, Endian::Big, 1, b"k", "ping", true, |_| {}).unwrap();
        assert_eq!(fragment_message(msg.clone(), 4096).unwrap(), vec![msg]);
    }

    /// GIOP 1.1 restarts alignment per fragment and carries no request id, so
    /// concatenation is not reassembly. Refusing is correct; producing a
    /// plausible wrong value would not be.
    #[test]
    fn giop_1_1_fragments_are_refused_rather_than_concatenated() {
        let mut frag: &[u8] = &[b'G', b'I', b'O', b'P', 1, 1, 0b11, 0, 0, 0, 0, 0];
        assert!(matches!(
            read_message(&mut frag, DEFAULT_MAX_MESSAGE_SIZE),
            Err(Error::FragmentUnsupported)
        ));
    }

    /// A peer that never sets the final bit must not grow our buffer forever.
    #[test]
    fn endless_fragments_are_bounded() {
        let mut wire = encode_request(Version::V1_2, Endian::Big, 3, b"k", "op", true, |e| {
            e.put_u32(1);
        })
        .unwrap();
        wire[6] |= 0b10;
        // Append fragments that always claim more follow.
        for _ in 0..(MAX_FRAGMENTS + 2) {
            let mut f = Vec::new();
            f.extend_from_slice(MAGIC);
            f.extend_from_slice(&[1, 2, 0b10, MsgType::Fragment as u8]);
            f.extend_from_slice(&12u32.to_be_bytes());
            f.extend_from_slice(&3u32.to_be_bytes());
            f.extend_from_slice(&[0u8; 8]);
            wire.extend_from_slice(&f);
        }
        let mut cursor: &[u8] = &wire;
        assert!(read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).is_err());
    }

    /// A fragment for a different request means our accounting is wrong.
    #[test]
    fn mismatched_fragment_id_is_rejected() {
        let mut wire = encode_request(Version::V1_2, Endian::Big, 3, b"k", "op", true, |e| {
            e.put_u32(1);
        })
        .unwrap();
        wire[6] |= 0b10;
        let mut f = Vec::new();
        f.extend_from_slice(MAGIC);
        f.extend_from_slice(&[1, 2, 0, MsgType::Fragment as u8]);
        f.extend_from_slice(&12u32.to_be_bytes());
        f.extend_from_slice(&999u32.to_be_bytes()); // wrong request id
        f.extend_from_slice(&[0u8; 8]);
        wire.extend_from_slice(&f);
        let mut cursor: &[u8] = &wire;
        assert!(matches!(
            read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE),
            Err(Error::Desynchronized)
        ));
    }

    #[test]
    fn reserved_flag_bits_must_be_zero() {
        let mut bad: &[u8] = &[b'G', b'I', b'O', b'P', 1, 2, 0b1000_0001, 1, 0, 0, 0, 0];
        assert!(read_message(&mut bad, DEFAULT_MAX_MESSAGE_SIZE).is_err());
    }

    #[test]
    fn unknown_message_type_names_itself() {
        let mut bad: &[u8] = &[b'G', b'I', b'O', b'P', 1, 2, 1, 99, 0, 0, 0, 0];
        match read_message(&mut bad, DEFAULT_MAX_MESSAGE_SIZE) {
            Err(Error::UnknownMessageType(99)) => {}
            other => panic!("expected UnknownMessageType(99), got {other:?}"),
        }
    }

    // ── IOR ──────────────────────────────────────────────────────────────────

    fn sample_ior(minor: u8, with_component: bool) -> Vec<u8> {
        let mut profile = Encoder::encapsulation(Endian::Little);
        profile.put_u8(1);
        profile.put_u8(minor);
        profile.put_str("192.0.2.10");
        profile.put_u16(9999);
        profile.put_octet_seq(b"objkey");
        if minor >= 1 {
            if with_component {
                profile.put_u32(1);
                profile.put_u32(20); // TAG_SSL_SEC_TRANS
                profile.put_octet_seq(&[1, 2, 3, 4]);
            } else {
                profile.put_u32(0);
            }
        }
        let mut ior = Encoder::encapsulation(Endian::Little);
        ior.put_str("IDL:spike/Echo:1.0");
        ior.put_u32(1);
        ior.put_u32(TAG_INTERNET_IOP);
        ior.put_encapsulation(profile);
        ior.finish().unwrap()
    }

    #[test]
    fn ior_round_trips_through_our_own_encoder() {
        let parsed = Ior::from_encapsulation(&sample_ior(2, false)).unwrap();
        assert_eq!(parsed.type_id, "IDL:spike/Echo:1.0");
        let p = parsed.primary().unwrap();
        assert_eq!(p.version, Version::V1_2);
        assert_eq!(p.host, "192.0.2.10");
        assert_eq!(p.port, 9999);
        assert_eq!(p.object_key, b"objkey");
    }

    /// Audit CONFIRMED #11: components used to be discarded, which loses the
    /// real port of an SSLIOP profile and blocks codeset negotiation.
    #[test]
    fn tagged_components_are_preserved() {
        let parsed = Ior::from_encapsulation(&sample_ior(2, true)).unwrap();
        let p = parsed.primary().unwrap();
        assert_eq!(p.components.len(), 1);
        assert_eq!(p.components[0].tag, 20);
        assert_eq!(p.components[0].data, vec![1, 2, 3, 4]);
    }

    #[test]
    fn iiop_1_0_profile_has_no_components() {
        let parsed = Ior::from_encapsulation(&sample_ior(0, false)).unwrap();
        let p = parsed.primary().unwrap();
        assert_eq!(p.version, Version::V1_0);
        assert!(p.components.is_empty());
    }

    /// Audit CONFIRMED #12: §7.6.9 says the case of a stringified IOR is not
    /// significant.
    #[test]
    fn ior_prefix_is_case_insensitive() {
        let hex: String = sample_ior(2, false).iter().map(|b| format!("{b:02x}")).collect();
        for prefix in ["IOR:", "ior:", "Ior:", "iOr:"] {
            assert!(Ior::parse(&format!("{prefix}{hex}")).is_ok(), "rejected {prefix}");
        }
    }

    #[test]
    fn ior_rejects_junk() {
        assert!(matches!(Ior::parse("nope"), Err(Error::BadIor(_))));
        assert!(matches!(Ior::parse("IOR:xyz"), Err(Error::BadIor(_))));
        assert!(matches!(Ior::parse("IOR:abc"), Err(Error::BadIor(_))));
        assert!(matches!(Ior::parse("IOR:"), Err(Error::BadIor(_))));
    }

    /// Audit HOSTILE #2: a truncated IOR used to parse "successfully" into a
    /// profile whose host and port came from string content.
    #[test]
    fn truncated_ior_fails_instead_of_inventing_an_endpoint() {
        let full = sample_ior(2, false);
        for cut in [6, 10, 14, 20, 26] {
            let truncated = &full[..cut.min(full.len())];
            match Ior::from_encapsulation(truncated) {
                Err(_) => {}
                Ok(ior) => panic!(
                    "truncation at {cut} produced a usable IOR with {} profile(s)",
                    ior.profiles.len()
                ),
            }
        }
    }

    // ── multi-profile failover ───────────────────────────────────────────────
    //
    // These tests need no ORB peer: Connection::connect performs no GIOP
    // traffic after the TCP handshake — codeset negotiation reads only the
    // profile's components, and the CodeSets context rides on the *first
    // request* — so a bound-but-never-accepting TcpListener is a sufficient
    // "live" endpoint. What a real peer does once bytes flow is the wire
    // oracle's job at integration, not this file's.

    fn profile_at(host: &str, port: u16, components: Vec<TaggedComponent>) -> IiopProfile {
        IiopProfile {
            version: Version::V1_2,
            host: host.into(),
            port,
            object_key: b"k".to_vec(),
            components,
        }
    }

    fn failover_ior(profiles: Vec<IiopProfile>) -> Ior {
        Ior { type_id: "IDL:spike/Echo:1.0".into(), profiles }
    }

    /// Builds a `TAG_ALTERNATE_IIOP_ADDRESS` body the way a server would:
    /// an encapsulation of `string host; unsigned short port;`.
    fn alternate_component(endian: Endian, host: &str, port: u16) -> TaggedComponent {
        let mut e = Encoder::encapsulation(endian);
        e.put_str(host);
        e.put_u16(port);
        TaggedComponent { tag: TAG_ALTERNATE_IIOP_ADDRESS, data: e.finish().unwrap() }
    }

    /// A loopback port nothing listens on, found by binding and releasing it.
    /// Connecting to it is refused immediately — no timeout to wait out — so
    /// a dead endpoint costs the test microseconds, not seconds.
    fn refused_port() -> u16 {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        port
    }

    fn live_listener() -> (std::net::TcpListener, u16) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        (l, port)
    }

    /// Whether a connection reaches `l`'s accept queue within `within`.
    ///
    /// This must be a wait, not a single probe: `connect_timeout` returning
    /// on the client does **not** mean the server side is acceptable yet.
    /// Measured on macOS loopback, an immediate non-blocking `accept()`
    /// missed up to 25 of 500 freshly-completed connections — the same class
    /// of phantom failure as CLAUDE.md's wait-loops-must-sleep rule, so the
    /// loop sleeps. Negative callers pass a short window; since `connect()`
    /// has already returned by the time they ask, any wrongly-dialed
    /// connection was established before the window even opened.
    fn connection_arrived(l: &std::net::TcpListener, within: Duration) -> bool {
        l.set_nonblocking(true).unwrap();
        let deadline = std::time::Instant::now() + within;
        loop {
            if l.accept().is_ok() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn got_connection(l: &std::net::TcpListener) -> bool {
        connection_arrived(l, Duration::from_secs(2))
    }

    fn no_connection(l: &std::net::TcpListener) -> bool {
        !connection_arrived(l, Duration::from_millis(20))
    }

    #[test]
    fn connect_fails_over_to_the_second_profile() {
        let (listener, live) = live_listener();
        let ior = failover_ior(vec![
            profile_at("127.0.0.1", refused_port(), Vec::new()),
            profile_at("127.0.0.1", live, Vec::new()),
        ]);
        let conn = Connection::connect(&ior, Duration::from_secs(5))
            .expect("a dead first profile must not fail the connect");
        assert!(conn.is_usable());
        assert!(got_connection(&listener), "the connection must have landed on profile 2");
    }

    #[test]
    fn exhausted_endpoints_report_the_count_and_the_last_reason() {
        // Two profiles, the first carrying an alternate: three endpoints, all
        // dead. The count proves every one was dialed; the boxed error keeps
        // the last endpoint's reason, which is what a caller debugs with.
        let ior = failover_ior(vec![
            profile_at(
                "127.0.0.1",
                refused_port(),
                vec![alternate_component(Endian::Little, "127.0.0.1", refused_port())],
            ),
            profile_at("127.0.0.1", refused_port(), Vec::new()),
        ]);
        let err = Connection::connect(&ior, Duration::from_secs(5)).unwrap_err();
        match &err {
            Error::AllEndpointsFailed { tried: 3, last } => {
                assert!(matches!(**last, Error::Io(_)), "last reason must survive: {last}");
            }
            other => panic!("expected AllEndpointsFailed over 3 endpoints, got {other:?}"),
        }
        let msg = err.to_string();
        assert!(msg.contains("3 endpoint"), "the message must state the count: {msg}");
    }

    #[test]
    fn an_ior_without_profiles_still_says_so() {
        // Zero endpoints is a different diagnosis from N dead ones, and
        // "all 0 endpoints failed" would be a lie about having tried.
        let err =
            Connection::connect(&failover_ior(Vec::new()), Duration::from_secs(5)).unwrap_err();
        assert!(matches!(err, Error::NoIiopProfile), "{err}");
    }

    #[test]
    fn alternate_components_parse_in_both_byte_orders() {
        // The encapsulation carries its own byte-order flag, so a big-endian
        // server's component must decode on a little-endian client and vice
        // versa — the classic failure that passes every native-endian test.
        for endian in [Endian::Big, Endian::Little] {
            let p = profile_at(
                "primary.example",
                2809,
                vec![alternate_component(endian, "alternate.example", 2810)],
            );
            assert_eq!(
                p.endpoints(),
                vec![("primary.example".into(), 2809), ("alternate.example".into(), 2810)],
                "{endian:?}"
            );
        }
    }

    #[test]
    fn an_alternate_address_rescues_a_dead_primary_endpoint() {
        for endian in [Endian::Big, Endian::Little] {
            let (listener, live) = live_listener();
            let ior = failover_ior(vec![profile_at(
                "127.0.0.1",
                refused_port(),
                vec![alternate_component(endian, "127.0.0.1", live)],
            )]);
            Connection::connect(&ior, Duration::from_secs(5))
                .unwrap_or_else(|e| panic!("{endian:?} alternate must be dialed: {e}"));
            assert!(got_connection(&listener), "{endian:?}");
        }
    }

    #[test]
    fn a_malformed_alternate_is_skipped_and_the_profile_survives() {
        // Truncate a well-formed component mid-host: it must vanish from the
        // endpoint list without taking the profile's own good address along.
        let mut broken = alternate_component(Endian::Little, "127.0.0.1", 2809);
        broken.data.truncate(6);

        let (listener, live) = live_listener();
        let p = profile_at("127.0.0.1", live, vec![broken]);
        assert_eq!(p.endpoints(), vec![("127.0.0.1".into(), live)]);

        Connection::connect(&failover_ior(vec![p]), Duration::from_secs(5))
            .expect("a bad hint must not kill a good address");
        assert!(got_connection(&listener));
    }

    #[test]
    fn endpoints_are_dialed_in_ior_order() {
        // Within a profile, its own address comes before its alternates …
        let (own, own_port) = live_listener();
        let (alt, alt_port) = live_listener();
        let ior = failover_ior(vec![profile_at(
            "127.0.0.1",
            own_port,
            vec![alternate_component(Endian::Little, "127.0.0.1", alt_port)],
        )]);
        Connection::connect(&ior, Duration::from_secs(5)).unwrap();
        assert!(got_connection(&own), "the profile's own address is dialed first");
        assert!(no_connection(&alt), "the alternate must not be dialed when it is not needed");

        // … and a profile's alternates come before the *next* profile.
        let (alt2, alt2_port) = live_listener();
        let (next, next_port) = live_listener();
        let ior = failover_ior(vec![
            profile_at(
                "127.0.0.1",
                refused_port(),
                vec![alternate_component(Endian::Big, "127.0.0.1", alt2_port)],
            ),
            profile_at("127.0.0.1", next_port, Vec::new()),
        ]);
        Connection::connect(&ior, Duration::from_secs(5)).unwrap();
        assert!(got_connection(&alt2), "profile 1's alternate outranks profile 2");
        assert!(no_connection(&next), "profile 2 must not be dialed when it is not needed");
    }

    /// Failover must not disturb what single-profile callers relied on:
    /// the negotiated version still comes from the profile that answered.
    #[test]
    fn the_answering_profiles_version_is_the_one_negotiated() {
        let (_listener, live) = live_listener();
        let mut old = profile_at("127.0.0.1", live, Vec::new());
        old.version = Version::V1_1;
        let ior = failover_ior(vec![
            profile_at("127.0.0.1", refused_port(), Vec::new()), // advertises 1.2
            old,
        ]);
        let conn = Connection::connect(&ior, Duration::from_secs(5)).unwrap();
        assert_eq!(
            conn.version(),
            Version::V1_1,
            "the version belongs to the profile that answered, not the first one"
        );
    }

    #[test]
    fn ior_read_inline_matches_encapsulated() {
        // A LOCATION_FORWARD body marshals the IOR inline, not encapsulated.
        let mut e = Encoder::new(Endian::Little);
        e.put_str("IDL:spike/Echo:1.0");
        e.put_u32(1);
        e.put_u32(TAG_INTERNET_IOP);
        let mut profile = Encoder::encapsulation(Endian::Little);
        profile.put_u8(1);
        profile.put_u8(2);
        profile.put_str("10.0.0.1");
        profile.put_u16(1234);
        profile.put_octet_seq(b"k");
        profile.put_u32(0);
        e.put_encapsulation(profile);
        let raw = e.finish().unwrap();

        let mut d = Decoder::new(&raw, Endian::Little);
        let ior = Ior::read_from(&mut d).unwrap();
        assert_eq!(ior.primary().unwrap().host, "10.0.0.1");
        assert_eq!(ior.primary().unwrap().port, 1234);
    }
}
