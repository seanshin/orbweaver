//! GIOP messages, IIOP transport, and IOR handling.
//!
//! Implements GIOP 1.0, 1.1 and 1.2 request/reply on the client side, with the
//! version-conditional header layouts each one requires. Fragmentation,
//! `LocateRequest`, codeset negotiation and the serving half are Phase 1 work
//! still outstanding; where they are absent this code fails loudly rather than
//! misparsing.
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
pub mod naming;
pub mod server;
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

/// Default ceiling on an inbound message body.
///
/// `message_size` is four attacker-controlled bytes that used to drive a
/// `Vec::resize` directly, so `ff ff ff ff` asked for a 4 GiB zeroed
/// allocation and an allocation failure aborts the process. A ceiling is not
/// optional on a network-facing decoder.
pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;

/// How many `LOCATION_FORWARD` hops to follow before giving up.
pub const MAX_FORWARD_HOPS: u8 = 8;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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
    /// The IOR carried no IIOP profile to connect to.
    NoIiopProfile,
    /// The peer closed the connection cleanly. Per §9.4.7 the pending request
    /// was not processed and may be safely re-sent on a new connection.
    ConnectionClosed,
    /// A previous failure left unread bytes in the stream, so the connection
    /// can no longer be framed. It must be discarded, not reused.
    Desynchronized,
    /// `LOCATION_FORWARD` chain exceeded [`MAX_FORWARD_HOPS`].
    TooManyForwards,
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
            Error::NoIiopProfile => write!(f, "IOR has no IIOP profile"),
            Error::ConnectionClosed => {
                write!(f, "peer closed the connection; the request was not processed")
            }
            Error::Desynchronized => {
                write!(f, "connection is desynchronized and must be discarded")
            }
            Error::TooManyForwards => write!(f, "too many LOCATION_FORWARD hops"),
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

/// A parsed Interoperable Object Reference.
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
        Self::from_encapsulation(&bytes)
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
        let bytes = e.finish().map_err(Error::Cdr)?;
        let mut out = String::with_capacity(4 + bytes.len() * 2);
        out.push_str("IOR:");
        for b in bytes {
            out.push_str(&format!("{b:02x}"));
        }
        Ok(out)
    }

    /// The first IIOP profile, which is the one dialed first.
    pub fn primary(&self) -> Result<&IiopProfile> {
        self.profiles.first().ok_or(Error::NoIiopProfile)
    }
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

    Ok(IiopProfile {
        version: Version { major, minor },
        host,
        port,
        object_key,
        components,
    })
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
        version, endian, request_id, object_key, operation, expect_reply, &[], write_body,
    )
}

/// As [`encode_request`], but attaching service contexts.
///
/// Codeset negotiation needs this: §7.10.2.5 carries the agreed transmission
/// codesets in a `CodeSets` context, and without one the specified default is
/// ISO-8859-1 regardless of what bytes we actually send.
#[allow(clippy::too_many_arguments)]
pub fn encode_request_with_contexts<F>(
    version: Version,
    endian: Endian,
    request_id: u32,
    object_key: &[u8],
    operation: &str,
    expect_reply: bool,
    contexts: &[ServiceContext],
    write_body: F,
) -> Result<Vec<u8>>
where
    F: FnOnce(&mut Encoder),
{
    if version.major != 1 || version.minor > 2 {
        return Err(Error::UnsupportedVersion(version));
    }
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
}

impl Reply {
    /// A decoder positioned at the start of the reply body.
    ///
    /// Fallible on purpose. The previous version discarded the seek error,
    /// which left the decoder at offset 0 and returned the GIOP magic as
    /// payload — wrong values, no error, nothing in the logs.
    pub fn body(&self) -> Result<Decoder<'_>> {
        let mut d = Decoder::new(&self.raw, self.endian);
        d.seek_to(self.body_at)?;
        Ok(d)
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
    if !first.more_fragments {
        return Ok(first);
    }
    if !first.version.is_1_2_layout() {
        // GIOP 1.1 restarts alignment relative to each fragment and carries no
        // request id to correlate them, so concatenation is not reassembly and
        // there is no way to tell whose fragments these are. Refusing beats
        // producing a plausible wrong value.
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
            return Err(Error::UnexpectedMessage(next.msg_type));
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

/// Reads exactly one GIOP message, without reassembling anything.
///
/// Rejects a `message_size` above `max_size` before allocating.
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

    let mut bytes = Vec::with_capacity(HEADER_LEN + size);
    bytes.extend_from_slice(&header);
    bytes.resize(HEADER_LEN + size, 0);
    stream.read_exact(&mut bytes[HEADER_LEN..])?;
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

    let status = ReplyStatus::from_u32(status_raw, version)
        .ok_or(Error::BadReplyStatus(status_raw))?;

    // §9.4.3.1: no padding after the header when the body is empty. Aligning
    // unconditionally can push the cursor past the end of a short message.
    let body_at = if version.aligns_body() && !d.is_empty() {
        d.align_to(8)?;
        d.offset()
    } else {
        d.offset()
    };

    Ok(Reply { request_id, status, endian, version, raw, body_at })
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

/// A synchronous, single-connection invoker.
///
/// Deliberately minimal, but no longer optimistic: it negotiates the version
/// from the IOR, follows `LOCATION_FORWARD`, distinguishes a clean
/// `CloseConnection` from a protocol error, and poisons itself rather than
/// reusing a stream whose framing is in doubt.
///
/// Connection pooling, request multiplexing and send-side fragmentation remain
/// Phase 1 work.
#[derive(Debug)]
pub struct Connection {
    stream: TcpStream,
    object_key: Vec<u8>,
    version: Version,
    endian: Endian,
    next_id: u32,
    max_message_size: usize,
    /// Set once framing can no longer be trusted. A desynchronized stream must
    /// be discarded: the next read would take payload bytes as a GIOP header.
    poisoned: bool,
    /// Negotiated `char` converter, or `None` when the peer published no
    /// codeset component. §7.10.2.5 then specifies ISO-8859-1 and no context.
    char_converter: Option<codeset::Converter>,
    /// Whether the `CodeSets` context still needs to go out.
    codeset_context_pending: bool,
    /// Body size above which outbound messages are fragmented.
    fragment_threshold: usize,
    /// Largest number of fragments any one reply arrived in.
    max_reply_fragments: usize,
}

impl Connection {
    /// Connects to the endpoint named by an IOR's first IIOP profile,
    /// negotiating the GIOP version down to what that profile advertises.
    pub fn connect(ior: &Ior, timeout: Duration) -> Result<Self> {
        let p = ior.primary()?;
        Self::connect_to(p, timeout)
    }

    /// Connects to a specific profile.
    pub fn connect_to(p: &IiopProfile, timeout: Duration) -> Result<Self> {
        let stream = dial(&p.host, p.port, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.set_nodelay(true)?;
        // Negotiate from TAG_CODE_SETS if the peer published one. Absent it,
        // §7.10.2.5 makes the transmission codeset ISO-8859-1 and forbids
        // pretending otherwise, so no context is sent and strings are Latin-1.
        let mut char_converter = None;
        for c in &p.components {
            if c.tag == codeset::TAG_CODE_SETS
                && let Ok(info) = codeset::CodeSetComponentInfo::parse(&c.data)
                && let Ok(id) = codeset::negotiate(&codeset::client_char_component(), &info.for_char)
                && let Ok(conv) = codeset::Converter::new(id)
            {
                char_converter = Some(conv);
            }
        }
        let codeset_context_pending = char_converter.is_some();

        Ok(Self {
            stream,
            object_key: p.object_key.clone(),
            version: Version::negotiate(p.version),
            endian: Endian::native(),
            next_id: 1,
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
            poisoned: false,
            char_converter,
            codeset_context_pending,
            fragment_threshold: DEFAULT_FRAGMENT_THRESHOLD,
            max_reply_fragments: 1,
        })
    }

    /// The converter for `char`/`string` data on this connection.
    ///
    /// Falls back to ISO-8859-1, which is what §7.10.2.5 specifies when no
    /// context is negotiated. It is `Copy`, so take it before calling
    /// [`Connection::invoke`] and use it inside the closure.
    pub fn char_converter(&self) -> codeset::Converter {
        self.char_converter.unwrap_or_else(|| {
            codeset::Converter::new(codeset::CodeSetId::ISO_8859_1)
                .expect("ISO-8859-1 is always supported")
        })
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

    /// The object key extracted from the IOR.
    pub fn object_key(&self) -> &[u8] {
        &self.object_key
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

    /// Invokes `operation`, writing arguments via `write_args`.
    ///
    /// Follows `LOCATION_FORWARD` transparently, as §9.4.3.2 requires, up to
    /// [`MAX_FORWARD_HOPS`].
    pub fn invoke<F>(&mut self, operation: &str, write_args: F) -> Result<Reply>
    where
        F: Fn(&mut Encoder),
    {
        for _ in 0..MAX_FORWARD_HOPS {
            match self.invoke_once(operation, &write_args)? {
                Outcome::Done(reply) => return Ok(reply),
                Outcome::Forwarded(ior) => {
                    let p = ior.primary()?;
                    let next = Self::connect_to(p, Duration::from_secs(10))?;
                    let endian = self.endian;
                    *self = next;
                    self.endian = endian;
                }
            }
        }
        Err(Error::TooManyForwards)
    }

    fn invoke_once<F>(&mut self, operation: &str, write_args: &F) -> Result<Outcome>
    where
        F: Fn(&mut Encoder),
    {
        if self.poisoned {
            return Err(Error::Desynchronized);
        }
        let id = self.next_request_id();
        // §7.10.2.5 negotiates per connection, so the context goes on the
        // first request only. Sending it again on a later request would risk
        // MARSHAL minor 9 for conflicting contexts on one connection.
        let contexts = if self.codeset_context_pending {
            match self.char_converter {
                Some(c) => vec![ServiceContext {
                    id: codeset::SERVICE_ID_CODE_SETS,
                    data: codeset::CodeSetContext {
                        char_data: c.id(),
                        wchar_data: codeset::CodeSetId::UTF_16,
                    }
                    .encode(self.endian)?,
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
            write_args,
        )?;
        self.codeset_context_pending = false;
        for piece in fragment_message(msg, self.fragment_threshold)? {
            self.stream.write_all(&piece)?;
        }
        self.stream.flush()?;

        loop {
            // Any framing failure from here leaves unread bytes behind.
            let raw = match read_message(&mut self.stream, self.max_message_size) {
                Ok(m) => m,
                Err(e) => {
                    self.poisoned = true;
                    return Err(e);
                }
            };
            self.max_reply_fragments = self.max_reply_fragments.max(raw.fragments);
            match raw.msg_type {
                MsgType::Reply => {
                    let reply = decode_reply(raw).inspect_err(|_| self.poisoned = true)?;
                    if reply.request_id != id {
                        // A reply for a request we are not waiting on means
                        // our accounting is wrong; keeping the connection
                        // would compound it.
                        self.poisoned = true;
                        return Err(Error::Desynchronized);
                    }
                    return self.interpret(reply);
                }
                MsgType::CloseConnection => {
                    // §9.4.7: the request was not processed and is safe to
                    // re-send on a fresh connection.
                    self.poisoned = true;
                    return Err(Error::ConnectionClosed);
                }
                other => {
                    self.poisoned = true;
                    return Err(Error::UnexpectedMessage(other));
                }
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
                let id = reply
                    .body()?
                    .get_string()
                    .unwrap_or_else(|_| "<unreadable>".into());
                Err(Error::UserException { id, reply: Box::new(reply) })
            }
            ReplyStatus::LocationForward | ReplyStatus::LocationForwardPerm => {
                let mut b = reply.body()?;
                let ior = Ior::read_from(&mut b)?;
                Ok(Outcome::Forwarded(ior))
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
    Forwarded(Ior),
}

/// Resolves and connects with a real timeout.
///
/// `"host:port".parse::<SocketAddr>()` only succeeds for numeric literals, so
/// routing DNS names through a `parse().unwrap_or_else(connect)` fallback
/// silently dropped the timeout for every hostname — and produced
/// `::1:9999` for IPv6 literals, which resolves to nothing at all.
fn dial(host: &str, port: u16, timeout: Duration) -> Result<TcpStream> {
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(Error::Io)?
        .collect::<Vec<_>>();
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
        assert_eq!(
            ReplyStatus::from_u32(4, Version::V1_2),
            Some(ReplyStatus::LocationForwardPerm)
        );
        assert_eq!(ReplyStatus::from_u32(9, Version::V1_2), None);
    }

    fn build_reply(version: Version, endian: Endian, id: u32, status: u32, contexts: u32) -> Vec<u8> {
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
        let mut hostile: &[u8] = &[
            b'G', b'I', b'O', b'P', 1, 2, 1, 1, 0xff, 0xff, 0xff, 0xff,
        ];
        match read_message(&mut hostile, DEFAULT_MAX_MESSAGE_SIZE) {
            Err(Error::MessageTooLarge { declared, limit }) => {
                assert_eq!(declared, 0xffff_ffff);
                assert_eq!(limit, DEFAULT_MAX_MESSAGE_SIZE);
            }
            other => panic!("expected MessageTooLarge, got {other:?}"),
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
        let msg = encode_request(Version::V1_2, Endian::Big, 1, b"k", "ping", true, |_| {}).unwrap();
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
