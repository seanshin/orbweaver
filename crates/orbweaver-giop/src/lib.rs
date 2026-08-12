//! GIOP messages, IIOP transport, and IOR handling.
//!
//! Scope is deliberately the Phase 0 spike surface: GIOP 1.2 `Request` and
//! `Reply` over TCP, enough IOR parsing to find a target, and a synchronous
//! invoker. Fragmentation, `LocateRequest`, GIOP 1.0/1.1 compatibility and
//! codeset negotiation are Phase 1 work.
//!
//! The alignment rule that governs everything here: a GIOP message aligns
//! from the first byte of its 12-byte header, so the header is built into the
//! same buffer as the body and the CDR origin stays at zero. Encapsulations
//! nested inside (IOR profiles, service contexts) restart alignment at their
//! own first byte.
//!
//! Spec: OMG CORBA 3.4 Part 2, sections 9.4 (GIOP) and 9.7 (IIOP/IOR).

#![deny(missing_docs)]

use orbweaver_cdr::{Decoder, Encoder, Endian};
use std::fmt;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// The four bytes every GIOP message starts with.
pub const MAGIC: &[u8; 4] = b"GIOP";

/// Fixed size of a GIOP message header.
pub const HEADER_LEN: usize = 12;

/// Profile tag for an IIOP profile inside an IOR.
pub const TAG_INTERNET_IOP: u32 = 0;

/// GIOP message types used by this crate.
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
    /// Server is shutting down.
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

/// Outcome reported by a GIOP 1.2 `Reply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyStatus {
    /// Operation completed; the body holds the result.
    NoException,
    /// Operation raised a user exception declared in IDL.
    UserException,
    /// Operation raised a system exception.
    SystemException,
    /// Target moved; the body holds a new IOR.
    LocationForward,
    /// Target moved for this request only.
    LocationForwardPerm,
    /// Target address disposition was wrong; retry differently.
    NeedsAddressingMode,
}

impl ReplyStatus {
    const fn from_u32(v: u32) -> Option<Self> {
        Some(match v {
            0 => ReplyStatus::NoException,
            1 => ReplyStatus::UserException,
            2 => ReplyStatus::SystemException,
            3 => ReplyStatus::LocationForward,
            4 => ReplyStatus::LocationForwardPerm,
            5 => ReplyStatus::NeedsAddressingMode,
            _ => return None,
        })
    }
}

/// Anything that can go wrong invoking over GIOP.
#[derive(Debug)]
pub enum Error {
    /// Underlying socket failure.
    Io(std::io::Error),
    /// A CDR value could not be read.
    Cdr(orbweaver_cdr::Error),
    /// The peer sent something that is not a GIOP message.
    NotGiop([u8; 4]),
    /// The peer spoke a GIOP version this spike does not handle.
    UnsupportedVersion(u8, u8),
    /// The peer sent a message type we did not expect here.
    UnexpectedMessage(MsgType),
    /// The reply status octet had no valid interpretation.
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
    /// The target raised a user exception; the body is left to the caller.
    UserException {
        /// Repository ID of the exception.
        id: String,
    },
    /// The IOR string could not be parsed.
    BadIor(&'static str),
    /// The IOR carried no IIOP profile to connect to.
    NoIiopProfile,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io: {e}"),
            Error::Cdr(e) => write!(f, "cdr: {e}"),
            Error::NotGiop(m) => write!(f, "not a GIOP message, magic was {m:?}"),
            Error::UnsupportedVersion(a, b) => write!(f, "unsupported GIOP {a}.{b}"),
            Error::UnexpectedMessage(t) => write!(f, "unexpected GIOP message: {t:?}"),
            Error::BadReplyStatus(v) => write!(f, "unknown reply status {v}"),
            Error::SystemException { id, minor, completed } => {
                write!(f, "system exception {id} (minor={minor}, completed={completed})")
            }
            Error::UserException { id } => write!(f, "user exception {id}"),
            Error::BadIor(why) => write!(f, "bad IOR: {why}"),
            Error::NoIiopProfile => write!(f, "IOR has no IIOP profile"),
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

/// The parts of an IIOP profile this spike needs to place a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IiopProfile {
    /// IIOP major version.
    pub major: u8,
    /// IIOP minor version.
    pub minor: u8,
    /// Host to connect to, as advertised by the server.
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// Opaque key identifying the servant behind the endpoint.
    pub object_key: Vec<u8>,
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
    pub fn parse(s: &str) -> Result<Self> {
        let hex = s.strip_prefix("IOR:").ok_or(Error::BadIor("missing 'IOR:' prefix"))?;
        if hex.len() % 2 != 0 {
            return Err(Error::BadIor("hex body has odd length"));
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
        let type_id = d.get_string().unwrap_or_default();
        let count = d.get_u32()?;
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

    /// The first IIOP profile, which is the one this spike dials.
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
    let host = d.get_string()?;
    let port = d.get_u16()?;
    let object_key = d.get_octet_seq()?.to_vec();
    // IIOP >= 1.1 appends tagged components; the spike does not need them.
    Ok(IiopProfile { major, minor, host, port, object_key })
}

// ─────────────────────────────────────────────────────────────────────────────
// Messages
// ─────────────────────────────────────────────────────────────────────────────

/// Encodes a GIOP 1.2 `Request` whose body is written by `write_body`.
///
/// The `message_size` field cannot be known until the body exists, so it is
/// written as a placeholder and patched once the length is final.
pub fn encode_request<F>(
    endian: Endian,
    request_id: u32,
    object_key: &[u8],
    operation: &str,
    expect_reply: bool,
    write_body: F,
) -> Vec<u8>
where
    F: FnOnce(&mut Encoder),
{
    let mut e = Encoder::new(endian);

    // ── message header (12 bytes, alignment origin) ──
    e.put_bytes(MAGIC);
    e.put_u8(1); // GIOP major
    e.put_u8(2); // GIOP minor
    e.put_u8(endian.as_flag()); // bit 0 byte order, bit 1 fragment
    e.put_u8(MsgType::Request as u8);
    let size_at = e.len();
    e.put_bytes(&[0, 0, 0, 0]); // message_size placeholder

    // ── RequestHeader_1_2 ──
    e.put_u32(request_id);
    e.put_u8(if expect_reply { 3 } else { 0 }); // response_flags
    e.put_bytes(&[0, 0, 0]); // reserved
    e.put_u16(0); // TargetAddress discriminator: 0 = ObjectKey
    e.put_octet_seq(object_key);
    e.put_str(operation);
    e.put_u32(0); // empty ServiceContextList

    // GIOP 1.2 aligns the request body to 8 from the start of the message.
    e.align_to(8);
    write_body(&mut e);

    let size = (e.len() - HEADER_LEN) as u32;
    e.patch_u32(size_at, size);
    e.finish()
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
    /// Whole message, retained so body offsets stay meaningful.
    raw: Vec<u8>,
    /// Offset in `raw` where the reply body begins.
    body_at: usize,
}

impl Reply {
    /// A decoder positioned at the start of the reply body.
    ///
    /// Alignment is preserved by decoding over the whole message, so any
    /// 8-byte-aligned value in the body lands where the peer put it.
    pub fn body(&self) -> Decoder<'_> {
        let mut d = Decoder::new(&self.raw, self.endian);
        let _ = d.get_bytes(self.body_at);
        d
    }
}

/// Reads one whole GIOP message from `stream`.
pub fn read_message(stream: &mut impl Read) -> Result<(MsgType, Endian, Vec<u8>)> {
    let mut header = [0u8; HEADER_LEN];
    stream.read_exact(&mut header)?;
    if &header[0..4] != MAGIC {
        return Err(Error::NotGiop([header[0], header[1], header[2], header[3]]));
    }
    let (major, minor) = (header[4], header[5]);
    if major != 1 || minor > 2 {
        return Err(Error::UnsupportedVersion(major, minor));
    }
    let endian = Endian::from_flag(header[6]);
    let msg_type = MsgType::from_octet(header[7]).ok_or(Error::UnexpectedMessage(MsgType::MessageError))?;

    let size = match endian {
        Endian::Big => u32::from_be_bytes([header[8], header[9], header[10], header[11]]),
        Endian::Little => u32::from_le_bytes([header[8], header[9], header[10], header[11]]),
    } as usize;

    let mut raw = Vec::with_capacity(HEADER_LEN + size);
    raw.extend_from_slice(&header);
    raw.resize(HEADER_LEN + size, 0);
    stream.read_exact(&mut raw[HEADER_LEN..])?;
    Ok((msg_type, endian, raw))
}

/// Decodes a GIOP 1.2 `Reply` message that `read_message` already framed.
pub fn decode_reply(endian: Endian, raw: Vec<u8>) -> Result<Reply> {
    let mut d = Decoder::new(&raw, endian);
    d.get_bytes(HEADER_LEN)?; // step over the message header, keeping alignment

    let request_id = d.get_u32()?;
    let status_raw = d.get_u32()?;
    let status = ReplyStatus::from_u32(status_raw).ok_or(Error::BadReplyStatus(status_raw))?;

    // ServiceContextList
    let n = d.get_u32()?;
    for _ in 0..n {
        let _id = d.get_u32()?;
        let _data = d.get_octet_seq()?;
    }

    d.align_to(8); // GIOP 1.2 body alignment
    let body_at = d.offset();
    Ok(Reply { request_id, status, endian, raw, body_at })
}

// ─────────────────────────────────────────────────────────────────────────────
// Invoker
// ─────────────────────────────────────────────────────────────────────────────

/// A synchronous one-connection-per-target invoker.
///
/// Deliberately minimal: it exists to answer whether a from-scratch GIOP
/// implementation can talk to a stock ORB. Connection pooling, pipelining and
/// reconnection belong to Phase 1.
#[derive(Debug)]
pub struct Connection {
    stream: TcpStream,
    object_key: Vec<u8>,
    endian: Endian,
    next_id: u32,
}

impl Connection {
    /// Connects to the endpoint named by an IOR's first IIOP profile.
    pub fn connect(ior: &Ior, timeout: Duration) -> Result<Self> {
        let p = ior.primary()?;
        // Resolve through the advertised host; NAT and container rewriting is
        // exactly the failure mode Phase 0 assumption D probes.
        let addr = format!("{}:{}", p.host, p.port);
        let sock = addr
            .parse()
            .map(|a| TcpStream::connect_timeout(&a, timeout))
            .unwrap_or_else(|_| TcpStream::connect(&addr))?;
        sock.set_read_timeout(Some(timeout))?;
        sock.set_write_timeout(Some(timeout))?;
        sock.set_nodelay(true)?;
        Ok(Self {
            stream: sock,
            object_key: p.object_key.clone(),
            endian: Endian::native(),
            next_id: 1,
        })
    }

    /// Byte order this connection writes. Defaults to native.
    pub fn set_endian(&mut self, endian: Endian) {
        self.endian = endian;
    }

    /// The object key extracted from the IOR.
    pub fn object_key(&self) -> &[u8] {
        &self.object_key
    }

    /// Invokes `operation`, writing arguments via `write_args`, and returns
    /// the reply for the caller to decode.
    ///
    /// A `SystemException` reply is surfaced as an error, since no caller can
    /// meaningfully read the body of one without the repository ID first.
    pub fn invoke<F>(&mut self, operation: &str, write_args: F) -> Result<Reply>
    where
        F: FnOnce(&mut Encoder),
    {
        let id = self.next_id;
        self.next_id += 1;

        let msg = encode_request(self.endian, id, &self.object_key, operation, true, write_args);
        self.stream.write_all(&msg)?;
        self.stream.flush()?;

        loop {
            let (ty, endian, raw) = read_message(&mut self.stream)?;
            match ty {
                MsgType::Reply => {
                    let reply = decode_reply(endian, raw)?;
                    if reply.request_id != id {
                        continue; // not ours; keep reading
                    }
                    return match reply.status {
                        ReplyStatus::NoException => Ok(reply),
                        ReplyStatus::SystemException => {
                            let mut b = reply.body();
                            Err(Error::SystemException {
                                id: b.get_string().unwrap_or_else(|_| "<unreadable>".into()),
                                minor: b.get_u32().unwrap_or(0),
                                completed: b.get_u32().unwrap_or(0),
                            })
                        }
                        ReplyStatus::UserException => {
                            let mut b = reply.body();
                            Err(Error::UserException {
                                id: b.get_string().unwrap_or_else(|_| "<unreadable>".into()),
                            })
                        }
                        _ => Ok(reply), // location forwards are the caller's problem for now
                    };
                }
                MsgType::CloseConnection => {
                    return Err(Error::UnexpectedMessage(MsgType::CloseConnection));
                }
                other => return Err(Error::UnexpectedMessage(other)),
            }
        }
    }

    /// Invokes an operation that takes no arguments.
    pub fn invoke_nullary(&mut self, operation: &str) -> Result<Reply> {
        self.invoke(operation, |_| {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_header_layout_is_giop_1_2() {
        let msg = encode_request(Endian::Big, 42, b"key", "ping", true, |_| {});

        assert_eq!(&msg[0..4], MAGIC);
        assert_eq!(msg[4], 1, "GIOP major");
        assert_eq!(msg[5], 2, "GIOP minor");
        assert_eq!(msg[6], 0, "big-endian flag");
        assert_eq!(msg[7], MsgType::Request as u8);

        let size = u32::from_be_bytes([msg[8], msg[9], msg[10], msg[11]]) as usize;
        assert_eq!(size, msg.len() - HEADER_LEN, "message_size excludes the header");

        // request_id sits immediately after the header, already 4-aligned.
        assert_eq!(u32::from_be_bytes([msg[12], msg[13], msg[14], msg[15]]), 42);
        assert_eq!(msg[16], 3, "response_flags requests a reply");
    }

    #[test]
    fn request_body_is_eight_byte_aligned() {
        // GIOP 1.2 requires the body to start on an 8-byte boundary measured
        // from the first byte of the message.
        let mut body_at = None;
        let msg = encode_request(Endian::Little, 1, b"k", "op", true, |e| {
            body_at = Some(e.len());
            e.put_f64(1.5);
        });
        let at = body_at.expect("body closure ran");
        assert_eq!(at % 8, 0, "body must start 8-aligned, started at {at}");
        assert_eq!(&msg[at..at + 8], &1.5f64.to_le_bytes());
    }

    #[test]
    fn oneway_clears_the_reply_flag() {
        let msg = encode_request(Endian::Big, 7, b"k", "fire", false, |_| {});
        assert_eq!(msg[16], 0, "oneway must not request a reply");
    }

    #[test]
    fn ior_round_trips_through_our_own_encoder() {
        // Build an IOR the way a server would, then read it back.
        let mut profile = Encoder::encapsulation(Endian::Little);
        profile.put_u8(1); // IIOP major
        profile.put_u8(2); // IIOP minor
        profile.put_str("192.0.2.10");
        profile.put_u16(9999);
        profile.put_octet_seq(b"objkey");
        profile.put_u32(0); // no tagged components

        let mut ior = Encoder::encapsulation(Endian::Little);
        ior.put_str("IDL:spike/Echo:1.0");
        ior.put_u32(1); // one profile
        ior.put_u32(TAG_INTERNET_IOP);
        ior.put_encapsulation(profile);

        let parsed = Ior::from_encapsulation(&ior.finish()).unwrap();
        assert_eq!(parsed.type_id, "IDL:spike/Echo:1.0");
        let p = parsed.primary().unwrap();
        assert_eq!((p.major, p.minor), (1, 2));
        assert_eq!(p.host, "192.0.2.10");
        assert_eq!(p.port, 9999);
        assert_eq!(p.object_key, b"objkey");
    }

    #[test]
    fn ior_rejects_junk() {
        assert!(matches!(Ior::parse("nope"), Err(Error::BadIor(_))));
        assert!(matches!(Ior::parse("IOR:xyz"), Err(Error::BadIor(_))));
        assert!(matches!(Ior::parse("IOR:abc"), Err(Error::BadIor(_))));
    }

    #[test]
    fn reply_body_offset_preserves_alignment() {
        // Hand-build a Reply with a double in the body and confirm the decoder
        // finds it at the alignment the sender used.
        let endian = Endian::Big;
        let mut e = Encoder::new(endian);
        e.put_bytes(MAGIC);
        e.put_u8(1);
        e.put_u8(2);
        e.put_u8(endian.as_flag());
        e.put_u8(MsgType::Reply as u8);
        let size_at = e.len();
        e.put_bytes(&[0, 0, 0, 0]);
        e.put_u32(99); // request_id
        e.put_u32(0); // NO_EXCEPTION
        e.put_u32(0); // empty service contexts
        e.align_to(8);
        e.put_f64(2.25);
        let size = (e.len() - HEADER_LEN) as u32;
        e.patch_u32(size_at, size);
        let raw = e.finish();

        let reply = decode_reply(endian, raw).unwrap();
        assert_eq!(reply.request_id, 99);
        assert_eq!(reply.status, ReplyStatus::NoException);
        assert_eq!(reply.body().get_f64().unwrap(), 2.25);
    }

    #[test]
    fn non_giop_traffic_is_rejected() {
        let mut junk: &[u8] = b"HTTP/1.1 200 OK\r\n\r\npadding-to-twelve";
        assert!(matches!(read_message(&mut junk), Err(Error::NotGiop(_))));
    }
}
