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

use orbweaver_cdr::{Decoder, Encoder, Endian};
use std::io::Write;
use std::net::{TcpListener, TcpStream};

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

/// What a servant does with an invocation.
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

    /// Whether this servant answers to `object_key`. Defaults to accepting
    /// everything, which is right for a single-servant process.
    fn knows(&self, _object_key: &[u8]) -> bool {
        true
    }
}

/// A single-threaded, one-connection-at-a-time GIOP server.
///
/// Enough to prove the serving half interoperates. Concurrency, a POA and
/// servant lifecycle are later work.
#[derive(Debug)]
pub struct Server {
    listener: TcpListener,
    object_key: Vec<u8>,
    max_message_size: usize,
    fragment_threshold: usize,
}

impl Server {
    /// Binds to `addr` and adopts `object_key` as the servant's identity.
    pub fn bind(addr: &str, object_key: Vec<u8>) -> Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(addr)?,
            object_key,
            max_message_size: DEFAULT_MAX_MESSAGE_SIZE,
            fragment_threshold: crate::DEFAULT_FRAGMENT_THRESHOLD,
        })
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

    /// Serves connections until `stop` returns true between accepts.
    pub fn serve<D, S>(&self, dispatch: &mut D, mut stop: S) -> Result<()>
    where
        D: Dispatch,
        S: FnMut() -> bool,
    {
        for incoming in self.listener.incoming() {
            if stop() {
                return Ok(());
            }
            match incoming {
                Ok(stream) => {
                    // One bad client must not take the server down.
                    if let Err(e) = self.serve_connection(stream, dispatch) {
                        eprintln!("orbweaver: connection ended: {e}");
                    }
                }
                Err(e) => eprintln!("orbweaver: accept failed: {e}"),
            }
            if stop() {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Handles one connection to completion.
    pub fn serve_connection<D: Dispatch>(&self, mut s: TcpStream, d: &mut D) -> Result<()> {
        s.set_nodelay(true)?;
        loop {
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

            match msg.msg_type {
                MsgType::LocateRequest => {
                    let lr = decode_locate_request(msg)?;
                    let status = if d.knows(&lr.object_key) {
                        LocateStatus::ObjectHere
                    } else {
                        LocateStatus::UnknownObject
                    };
                    let out = encode_locate_reply(lr.version, lr.endian, lr.request_id, status)?;
                    s.write_all(&out)?;
                }
                MsgType::Request => {
                    let req = decode_request(msg)?;
                    let reply = self.handle_request(&req, d)?;
                    if let Some(bytes) = reply {
                        for piece in fragment_message(bytes, self.fragment_threshold)? {
                            s.write_all(&piece)?;
                        }
                    }
                }
                MsgType::CancelRequest => {
                    // Nothing is queued, since requests are handled inline.
                    // Reading it and moving on is the correct no-op.
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

        // Servants write into a detached buffer, so it too must know where it
        // will land. A 1.0 reply body starts immediately after the header.
        let reply_header_len = if req.version.is_1_2_layout() { HEADER_LEN + 12 } else { HEADER_LEN + 12 };
        let body_start = if req.version.aligns_body() {
            reply_header_len.div_ceil(8) * 8
        } else {
            reply_header_len
        };
        let mut out = Encoder::continuing_at(req.endian, body_start);
        match d.dispatch(req, &mut out) {
            Ok(()) => {
                if !req.expect_reply {
                    return Ok(None); // oneway: no reply at all
                }
                let body = out.finish().map_err(Error::Cdr)?;
                Ok(Some(encode_reply(
                    req.version,
                    req.endian,
                    req.request_id,
                    ReplyStatus::NoException,
                    |e| e.put_bytes(&body),
                )?))
            }
            Err(ex) => self.reply_exception(req, &ex),
        }
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
                let wire =
                    encode_request(version, endian, 77, b"objkey", "compute", true, |e| {
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
            let wire = encode_request(version, Endian::Big, 1, b"k", "fire", false, |_| {}).unwrap();
            let mut cursor: &[u8] = &wire;
            let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).unwrap();
            assert!(!decode_request(msg).unwrap().expect_reply, "{version}");
        }
    }

    #[test]
    fn reply_round_trips_in_every_version() {
        for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
            for endian in [Endian::Big, Endian::Little] {
                let wire =
                    encode_reply(version, endian, 88, ReplyStatus::NoException, |e| e.put_f64(1.25))
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
