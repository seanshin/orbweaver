//! OMG Common Data Representation (CDR) encoding and decoding.
//!
//! CDR is positional and alignment-sensitive: every primitive is padded to a
//! multiple of its own size, measured from the start of the enclosing stream.
//! Getting that origin wrong is the classic interoperability bug, so it is
//! explicit here rather than implied.
//!
//! Two origins exist in practice:
//!
//! - A GIOP message aligns everything from the first byte of the 12-byte
//!   message header. Build the header into the same buffer and the origin is
//!   simply zero.
//! - An *encapsulation* (`sequence<octet>` carrying a nested CDR stream, used
//!   by IORs and service contexts) restarts alignment at its own first byte,
//!   which is a byte-order flag.
//!
//! # Reading untrusted bytes
//!
//! Everything a [`Decoder`] reads arrives from the network. Length prefixes,
//! element counts and alignment are all attacker-controlled, so this module
//! treats them as hostile by default: lengths are checked against what remains
//! before anything is consumed, alignment past the end of the buffer is an
//! error rather than a silently out-of-range position, and strings with
//! embedded NULs are rejected because a C peer would truncate where we would
//! not.
//!
//! Spec: OMG CORBA 3.4 Part 2 (Interoperability), section 9.3.

#![deny(missing_docs)]

use std::fmt;
use std::sync::Arc;

/// Byte order of a CDR stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endian {
    /// Most significant byte first.
    Big,
    /// Least significant byte first.
    Little,
}

impl Endian {
    /// The byte order of the machine running this code.
    pub const fn native() -> Self {
        if cfg!(target_endian = "little") { Endian::Little } else { Endian::Big }
    }

    /// CDR encodes little-endian as 1, big-endian as 0.
    pub const fn as_flag(self) -> u8 {
        match self {
            Endian::Big => 0,
            Endian::Little => 1,
        }
    }

    /// Reads an encapsulation's byte-order flag, which CORBA 3.4 §9.3.3
    /// specifies as a `boolean`. Values other than 0 and 1 are malformed and
    /// are rejected rather than reinterpreted — a peer sending 0x37 here is
    /// either broken or probing.
    pub const fn try_from_flag(flag: u8) -> Result<Self> {
        match flag {
            0 => Ok(Endian::Big),
            1 => Ok(Endian::Little),
            _ => Err(Error::Malformed("byte-order flag must be 0 or 1")),
        }
    }
}

/// Something went wrong reading or writing a CDR stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The stream ended before the requested value could be read.
    Truncated {
        /// Bytes required.
        need: usize,
        /// Bytes available.
        have: usize,
    },
    /// A string or sequence declared a length the stream cannot satisfy.
    BadLength(u32),
    /// A string was not valid UTF-8 under the assumed codeset.
    BadUtf8,
    /// A string's terminating NUL was absent.
    MissingNul,
    /// A string contained a NUL before its end. Accepting one lets a peer
    /// present two different values to us and to any C-based ORB, which is an
    /// authorization- and audit-bypass primitive once operations are gated by
    /// name.
    EmbeddedNul,
    /// Alignment padding would move the cursor past the end of the buffer.
    AlignmentOutOfRange {
        /// Where alignment would have landed.
        wanted: usize,
        /// Bytes actually present.
        len: usize,
    },
    /// A discriminator, tag or flag had no valid interpretation.
    Malformed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated { need, have } => {
                write!(f, "truncated CDR stream: need {need} bytes, have {have}")
            }
            Error::BadLength(n) => write!(f, "implausible CDR length prefix: {n}"),
            Error::BadUtf8 => write!(f, "string is not valid UTF-8"),
            Error::MissingNul => write!(f, "string is missing its terminating NUL"),
            Error::EmbeddedNul => write!(f, "string contains an embedded NUL"),
            Error::AlignmentOutOfRange { wanted, len } => {
                write!(f, "alignment would reach offset {wanted} in a {len}-byte buffer")
            }
            Error::Malformed(what) => write!(f, "malformed CDR: {what}"),
        }
    }
}

impl std::error::Error for Error {}

/// Result of a CDR operation.
pub type Result<T> = std::result::Result<T, Error>;

/// Converts a `long double` between the canonical big-endian octets the API
/// deals in and the octets `endian` puts on the wire.
///
/// Its own inverse, so the same function serves both directions and neither
/// can drift from the other. That matters more here than the two lines it
/// saves: the previous code moved the octets through untouched in both
/// directions, which is a convention our encoder and decoder agreed on with
/// each other and with nobody else.
const fn wire_long_double(v: [u8; 16], endian: Endian) -> [u8; 16] {
    match endian {
        Endian::Big => v,
        Endian::Little => {
            let mut out = [0u8; 16];
            let mut i = 0;
            while i < 16 {
                out[i] = v[15 - i];
                i += 1;
            }
            out
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Encoder
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// Transmission codeset (D009)
// ─────────────────────────────────────────────────────────────────────────────

/// Converts narrow text between Rust's `str` and the octets a stream carries.
///
/// # Why this is a trait here and the tables are not
///
/// A CORBA connection negotiates a *transmission codeset* per §13.10, and a
/// stream that does not know its own is a stream whose text is a guess. The
/// tables that answer it are 1300 lines in `orbweaver-giop::codeset`, need a
/// GIOP `Version`, and optionally pull `encoding_rs` for EUC-KR — a
/// BSD-3-Clause obligation disclosed in `NOTICE` under D001. This crate has
/// **zero dependencies** and knows nothing of GIOP, deliberately, and
/// `run_checks.sh` tests the promise that `--no-default-features` drops that
/// obligation. So the stream gets a slot and a trait; the knowledge stays
/// where it already is.
///
/// # What this does *not* cover
///
/// **Framing.** [`Encoder::put_string_bytes`] and
/// [`Decoder::get_string_bytes`] keep the length prefix, the trailing NUL and
/// the embedded-NUL refusal, and those three rules stay in one place. A codec
/// that owned them would be a second opinion about whether the NUL is counted.
///
/// **Wide text.** A `wstring`'s framing itself varies with the GIOP version
/// and the BOM, so it cannot live in a crate that has no version to consult.
/// `orbweaver-giop::codeset::WideCodec` owns that end to end.
///
/// # 한국어
///
/// 연결은 전송 코드셋을 협상하며, 자기 코드셋을 모르는 스트림의 텍스트는
/// 추측이다. 그 표는 GIOP 쪽에 있고 라이선스 의무를 동반하므로, 이 크레이트는
/// **슬롯과 트레이트만** 얻는다. 프레이밍은 이미 한 곳에 있으니 그대로 두고,
/// 넓은 문자열은 버전을 알아야 하므로 여기 오지 않는다.
pub trait TextCodec: Send + Sync + fmt::Debug {
    /// The octets this codeset uses for `s`, or why it cannot represent it.
    fn encode_narrow(&self, s: &str) -> Result<Vec<u8>>;

    /// The text `bytes` carry, or why they are not valid in this codeset.
    fn decode_narrow(&self, bytes: &[u8]) -> Result<String>;

    /// Writes an IDL `wstring` — **the whole field**, framing included.
    ///
    /// The opposite of the narrow half, and the asymmetry is the design: a
    /// `wstring`'s framing itself varies with the GIOP version and the BOM, so
    /// this crate has nothing to keep. It has no version to consult and must
    /// not learn one.
    fn put_wide(&self, e: &mut Encoder, s: &str) -> Result<()>;

    /// Reads an IDL `wstring`, framing included.
    fn get_wide(&self, d: &mut Decoder<'_>) -> Result<String>;

    /// Writes an IDL `wchar`.
    fn put_wide_char(&self, e: &mut Encoder, c: char) -> Result<()>;

    /// Reads an IDL `wchar`.
    fn get_wide_char(&self, d: &mut Decoder<'_>) -> Result<char>;
}

/// Writes CDR-encoded values into a growable buffer.
///
/// Write methods do not return a `Result`, because threading one through every
/// field of a nested struct makes call sites unreadable. Instead the encoder
/// *poisons*: the first failed write records its error and every later write is
/// ignored, and [`Encoder::finish`] surfaces it. A caller cannot get bytes out
/// without confronting the error.
#[derive(Debug, Clone)]

pub struct Encoder {
    /// The transmission codeset for narrow text, or `None` for UTF-8 (D009).
    ///
    /// `Arc`, not `&dyn`: this struct has no lifetime parameter, and giving it
    /// one would change all 145 of its construction sites and `Cdr::put`'s
    /// signature besides — the churn the decision used to reject the
    /// alternative. `None` costs nothing and is exactly the behaviour that
    /// shipped before this field existed.
    codec: Option<Arc<dyn TextCodec>>,
    buf: Vec<u8>,
    endian: Endian,
    /// Offset that counts as position zero for alignment purposes.
    origin: usize,
    /// Bytes notionally already written before this buffer begins.
    virtual_offset: usize,
    poison: Option<Error>,
}

impl Encoder {
    /// A new encoder whose alignment origin is the start of its buffer.
    pub fn new(endian: Endian) -> Self {
        Self {
            codec: None,
            buf: Vec::with_capacity(256),
            endian,
            origin: 0,
            virtual_offset: 0,
            poison: None,
        }
    }

    /// An encoder that aligns as though `offset` bytes already preceded it.
    ///
    /// Needed whenever a fragment of a stream is built in its own buffer and
    /// spliced in later. CDR alignment is measured from the start of the
    /// enclosing message, so a detached buffer that starts counting at zero
    /// pads to the wrong boundaries.
    ///
    /// This is not hypothetical: building a GIOP request body in a plain
    /// `Encoder::new` looked correct against GIOP 1.2, where the body happens
    /// to start 8-aligned, and silently mis-encoded every `double` in a 1.0 or
    /// 1.1 body, where it does not.
    pub fn continuing_at(endian: Endian, offset: usize) -> Self {
        Self {
            codec: None,
            buf: Vec::with_capacity(256),
            endian,
            origin: 0,
            virtual_offset: offset,
            poison: None,
        }
    }

    /// A new encoder for an encapsulation: the byte-order flag is written
    /// first and alignment restarts from it, per CORBA 3.4 §9.3.3.
    pub fn encapsulation(endian: Endian) -> Self {
        let mut e = Self::new(endian);
        e.put_u8(endian.as_flag());
        e
    }

    /// Writes a `wstring` through the stream's codec.
    ///
    /// Unlike [`Encoder::put_str`] there is no default: a `wstring`'s wire
    /// form depends on the GIOP version, this crate does not know it, and
    /// inventing one is how a 1.1 connection gets 1.2's form. A stream with no
    /// codec refuses rather than guessing.
    pub fn put_wstr(&mut self, v: &str) -> Result<()> {
        match self.codec.clone() {
            Some(c) => c.put_wide(self, v),
            None => Err(Error::Malformed(
                "this stream carries no wide codec, and a wstring's form depends on the \
                 GIOP version — attach one rather than assuming 1.2",
            )),
        }
    }

    /// Writes a `wchar`. See [`Encoder::put_wstr`] for why there is no default.
    pub fn put_wchar_text(&mut self, c: char) -> Result<()> {
        match self.codec.clone() {
            Some(codec) => codec.put_wide_char(self, c),
            None => Err(Error::Malformed(
                "this stream carries no wide codec, and a wchar's form depends on the \
                 GIOP version",
            )),
        }
    }

    /// Whether a codec is attached.
    ///
    /// For a caller that has a correct fallback of its own — an encapsulation,
    /// whose wide form is fixed by §9.3.1.6 — and must not turn "nothing
    /// attached" into a refusal.
    pub fn has_codec(&self) -> bool {
        self.codec.is_some()
    }

    /// Attaches the transmission codeset for narrow text (D009).
    ///
    /// **An encapsulation does not inherit it, and that is deliberate.**
    /// [`Encoder::encapsulation`] starts from [`Encoder::new`], so a nested
    /// encapsulation begins with no codec — UTF-8 — and the caller attaches
    /// one only if the thing inside really is negotiated text. The alternative,
    /// inheriting, silently re-encodes every string in every `TypeCode`
    /// encapsulation, whose repository ids and member names are the contract's
    /// own identifiers and are not the peer's text at all. §9.3.1.6 makes the
    /// same distinction the other way for `wchar`, which is always the 1.2
    /// form inside an encapsulation whatever the message says.
    ///
    /// 캡슐화는 코덱을 **물려받지 않는다.** 물려받으면 `TypeCode` 캡슐화 안의
    /// 저장소 id와 멤버 이름 — 피어의 텍스트가 아니라 계약 자신의 식별자 — 까지
    /// 조용히 다시 인코딩된다.
    pub fn with_codec(mut self, codec: Option<Arc<dyn TextCodec>>) -> Self {
        self.codec = codec;
        self
    }

    /// The byte order this encoder writes.
    pub fn endian(&self) -> Endian {
        self.endian
    }

    /// Bytes written so far.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether nothing has been written.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Current offset relative to the alignment origin, including any
    /// virtual prefix from [`Encoder::continuing_at`].
    ///
    /// The origin is an *absolute* offset in the notional stream, which is why
    /// the virtual prefix is added before it is subtracted rather than after.
    /// Adding it afterwards made every encapsulation opened inside a
    /// `continuing_at` buffer align from the enclosing message's offset instead
    /// of from its own first byte — §9.3.1.1 says an encapsulation's octet
    /// index restarts at zero "even if the encapsulation itself is nested in
    /// another encapsulation".
    pub fn position(&self) -> usize {
        (self.virtual_offset + self.buf.len()) - self.origin
    }

    /// The first error encountered, if any.
    pub fn error(&self) -> Option<&Error> {
        self.poison.as_ref()
    }

    fn poison(&mut self, e: Error) {
        if self.poison.is_none() {
            self.poison = Some(e);
        }
    }

    /// Treats the current end of the buffer as the new alignment origin.
    ///
    /// This is the only correct way to open an encapsulation: passing
    /// [`Encoder::len`] to [`Encoder::set_origin`] is a buffer index, and the
    /// origin is an absolute stream offset. The two coincide only when the
    /// encoder was not built with [`Encoder::continuing_at`].
    pub fn reset_origin(&mut self) {
        self.origin = self.virtual_offset + self.buf.len();
    }

    /// The current alignment origin, for saving across a nested encapsulation.
    pub fn origin(&self) -> usize {
        self.origin
    }

    /// Restores an origin previously obtained from [`Encoder::origin`].
    ///
    /// Encapsulations restart alignment at their own first byte, but a
    /// `TypeCode` indirection offset is measured in the *outermost* stream
    /// (CORBA 3.4 §9.3.5.1). Both are satisfiable only by writing everything
    /// into one buffer and moving the origin in and out of each encapsulation,
    /// rather than building encapsulations in buffers of their own.
    ///
    /// Takes an absolute offset in the notional stream — the same space
    /// [`Encoder::origin`] reports — not an index into this buffer.
    pub fn set_origin(&mut self, origin: usize) {
        self.origin = origin;
    }

    /// Pads with zero bytes until the position is a multiple of `align`.
    pub fn align_to(&mut self, align: usize) {
        debug_assert!(align.is_power_of_two());
        let pad = (align - (self.position() % align)) % align;
        self.buf.resize(self.buf.len() + pad, 0);
    }

    /// Consumes the encoder and returns the encoded bytes, or the first error
    /// that occurred while writing them.
    pub fn finish(self) -> Result<Vec<u8>> {
        match self.poison {
            Some(e) => Err(e),
            None => Ok(self.buf),
        }
    }

    /// The bytes written so far, regardless of poison state. For diagnostics.
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    /// Overwrites four bytes at `at` with `value` in this stream's byte order.
    ///
    /// GIOP cannot know its own `message_size` until the body is encoded, so
    /// the field is written as a placeholder and patched afterwards.
    pub fn patch_u32(&mut self, at: usize, value: u32) {
        if self.poison.is_some() || at + 4 > self.buf.len() {
            return;
        }
        let bytes = match self.endian {
            Endian::Big => value.to_be_bytes(),
            Endian::Little => value.to_le_bytes(),
        };
        self.buf[at..at + 4].copy_from_slice(&bytes);
    }

    // ── primitives ──────────────────────────────────────────────────────────

    /// Writes a single byte with no alignment.
    pub fn put_u8(&mut self, v: u8) {
        if self.poison.is_none() {
            self.buf.push(v);
        }
    }

    /// Writes raw bytes with no alignment or length prefix.
    pub fn put_bytes(&mut self, v: &[u8]) {
        if self.poison.is_none() {
            self.buf.extend_from_slice(v);
        }
    }

    /// Writes an IDL `boolean` as one byte.
    pub fn put_bool(&mut self, v: bool) {
        self.put_u8(u8::from(v));
    }

    /// Writes an IDL `octet`.
    pub fn put_octet(&mut self, v: u8) {
        self.put_u8(v);
    }

    /// Writes an IDL `char`.
    pub fn put_char(&mut self, v: u8) {
        self.put_u8(v);
    }

    /// Writes an IDL `short`, aligned to 2.
    pub fn put_i16(&mut self, v: i16) {
        self.put_u16(v as u16);
    }

    /// Writes an IDL `unsigned short`, aligned to 2.
    pub fn put_u16(&mut self, v: u16) {
        if self.poison.is_some() {
            return;
        }
        self.align_to(2);
        match self.endian {
            Endian::Big => self.buf.extend_from_slice(&v.to_be_bytes()),
            Endian::Little => self.buf.extend_from_slice(&v.to_le_bytes()),
        }
    }

    /// Writes an IDL `long`, aligned to 4.
    pub fn put_i32(&mut self, v: i32) {
        self.put_u32(v as u32);
    }

    /// Writes an IDL `unsigned long`, aligned to 4.
    pub fn put_u32(&mut self, v: u32) {
        if self.poison.is_some() {
            return;
        }
        self.align_to(4);
        match self.endian {
            Endian::Big => self.buf.extend_from_slice(&v.to_be_bytes()),
            Endian::Little => self.buf.extend_from_slice(&v.to_le_bytes()),
        }
    }

    /// Writes an IDL `long long`, aligned to 8.
    pub fn put_i64(&mut self, v: i64) {
        self.put_u64(v as u64);
    }

    /// Writes an IDL `unsigned long long`, aligned to 8.
    pub fn put_u64(&mut self, v: u64) {
        if self.poison.is_some() {
            return;
        }
        self.align_to(8);
        match self.endian {
            Endian::Big => self.buf.extend_from_slice(&v.to_be_bytes()),
            Endian::Little => self.buf.extend_from_slice(&v.to_le_bytes()),
        }
    }

    /// Writes an IDL `float`, aligned to 4.
    pub fn put_f32(&mut self, v: f32) {
        self.put_u32(v.to_bits());
    }

    /// Writes an IDL `double`, aligned to 8.
    pub fn put_f64(&mut self, v: f64) {
        self.put_u64(v.to_bits());
    }

    /// Writes an IDL `long double`: 16 octets, 8-aligned.
    ///
    /// Carried as octets because Rust has no stable 128-bit float. Passing them
    /// through is lossless and honest; converting via `f64` would silently
    /// discard precision that the peer took care to send.
    ///
    /// The octets are **big-endian** — sign and exponent first, as CORBA 3.4
    /// Part 2 Figure 9.2 draws them — and are reversed here when the stream is
    /// little-endian. `long double` is a primitive with a size, so §9.3.1 makes
    /// its octet order the stream's business exactly as it does for `double`;
    /// the figure shows the little-endian column as the big-endian one read
    /// bottom to top.
    pub fn put_long_double(&mut self, v: [u8; 16]) {
        if self.poison.is_some() {
            return;
        }
        self.align_to(8);
        self.buf.extend_from_slice(&wire_long_double(v, self.endian));
    }

    /// Writes an IDL `string`: a length that counts the terminating NUL,
    /// then the bytes, then the NUL.
    ///
    /// An embedded NUL poisons the encoder. CORBA 3.4 §9.3.2.7 gives a string
    /// a *single* terminating null, and a C peer would stop at the first one,
    /// so emitting one would let us and the peer disagree about what was sent.
    ///
    /// The bytes are passed through as given. Codeset conversion is the
    /// caller's business, because the transmission codeset is negotiated per
    /// connection and is not knowable here.
    pub fn put_string_bytes(&mut self, bytes: &[u8]) {
        if bytes.contains(&0) {
            self.poison(Error::EmbeddedNul);
            return;
        }
        self.put_u32(bytes.len() as u32 + 1);
        self.put_bytes(bytes);
        self.put_u8(0);
    }

    /// Writes a `str` as an IDL `string`, in the stream's transmission
    /// codeset.
    ///
    /// With no codec attached this is the UTF-8 it has always been. The
    /// framing — the length counting the NUL, the embedded-NUL refusal — stays
    /// here either way; a codec supplies octets, not a field.
    pub fn put_str(&mut self, v: &str) {
        match self.codec.clone() {
            None => self.put_string_bytes(v.as_bytes()),
            Some(c) => match c.encode_narrow(v) {
                Ok(bytes) => self.put_string_bytes(&bytes),
                Err(e) => self.poison(e),
            },
        }
    }

    /// Writes a `sequence<octet>`: a length prefix then the raw bytes.
    pub fn put_octet_seq(&mut self, bytes: &[u8]) {
        self.put_u32(bytes.len() as u32);
        self.put_bytes(bytes);
    }

    /// Writes a nested encapsulation as a `sequence<octet>`, propagating any
    /// error the inner encoder accumulated.
    pub fn put_encapsulation(&mut self, inner: Encoder) {
        match inner.finish() {
            Ok(bytes) => self.put_octet_seq(&bytes),
            Err(e) => self.poison(e),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Decoder
// ─────────────────────────────────────────────────────────────────────────────

/// Reads CDR-encoded values from a byte slice.
#[derive(Debug, Clone)]
pub struct Decoder<'a> {
    /// See [`Encoder::codec`]. `None` is UTF-8, which is what every stream
    /// carried before D009.
    codec: Option<Arc<dyn TextCodec>>,
    buf: &'a [u8],
    pos: usize,
    endian: Endian,
    origin: usize,
}

impl<'a> Decoder<'a> {
    /// A decoder over `buf` whose alignment origin is the start of the slice.
    pub fn new(buf: &'a [u8], endian: Endian) -> Self {
        Self { codec: None, buf, pos: 0, endian, origin: 0 }
    }

    /// A decoder over an encapsulation, reading the leading byte-order flag
    /// and restarting alignment from it.
    pub fn encapsulation(buf: &'a [u8]) -> Result<Self> {
        if buf.is_empty() {
            return Err(Error::Truncated { need: 1, have: 0 });
        }
        let endian = Endian::try_from_flag(buf[0])?;
        Ok(Self { codec: None, buf, pos: 1, endian, origin: 0 })
    }

    /// Whether a codec is attached.
    ///
    /// For a caller that has a correct fallback of its own — an encapsulation,
    /// whose wide form is fixed by §9.3.1.6 — and must not turn "nothing
    /// attached" into a refusal.
    pub fn has_codec(&self) -> bool {
        self.codec.is_some()
    }

    /// Reads a `wstring` through the stream's codec. See
    /// [`Encoder::put_wstr`] for why there is no default.
    pub fn get_wstr(&mut self) -> Result<String> {
        match self.codec.clone() {
            Some(c) => c.get_wide(self),
            None => Err(Error::Malformed(
                "this stream carries no wide codec, and a wstring's form depends on the \
                 GIOP version — attach one rather than assuming 1.2",
            )),
        }
    }

    /// Reads a `wchar` through the stream's codec.
    pub fn get_wchar_text(&mut self) -> Result<char> {
        match self.codec.clone() {
            Some(codec) => codec.get_wide_char(self),
            None => Err(Error::Malformed(
                "this stream carries no wide codec, and a wchar's form depends on the \
                 GIOP version",
            )),
        }
    }

    /// Attaches the transmission codeset for narrow text. See
    /// [`Encoder::with_codec`], including why an encapsulation does not
    /// inherit it.
    pub fn with_codec(mut self, codec: Option<Arc<dyn TextCodec>>) -> Self {
        self.codec = codec;
        self
    }

    /// The byte order this decoder reads.
    pub fn endian(&self) -> Endian {
        self.endian
    }

    /// Switches byte order mid-stream, as GIOP requires after reading the
    /// flags byte of a message header.
    pub fn set_endian(&mut self, endian: Endian) {
        self.endian = endian;
    }

    /// Current absolute offset into the underlying slice.
    pub fn offset(&self) -> usize {
        self.pos
    }

    /// Moves the cursor to an absolute offset, refusing to leave the buffer.
    ///
    /// Existed as `let _ = get_bytes(n)` at one call site, where discarding the
    /// error silently rewound the decoder to offset 0 and handed back the GIOP
    /// magic as payload. Seeking is now explicit and fallible.
    pub fn seek_to(&mut self, offset: usize) -> Result<()> {
        if offset > self.buf.len() {
            return Err(Error::AlignmentOutOfRange { wanted: offset, len: self.buf.len() });
        }
        self.pos = offset;
        Ok(())
    }

    /// Current offset relative to the alignment origin.
    pub fn position(&self) -> usize {
        self.pos - self.origin
    }

    /// The current alignment origin, for saving across a nested encapsulation.
    pub fn origin(&self) -> usize {
        self.origin
    }

    /// Restores a previously saved alignment origin.
    ///
    /// See [`Encoder::set_origin`]: an encapsulation restarts alignment while a
    /// `TypeCode` indirection offset stays absolute, so the reader walks one
    /// buffer and moves the origin rather than slicing sub-decoders out.
    pub fn set_origin(&mut self, origin: usize) {
        self.origin = origin;
    }

    /// Treats the current position as the alignment origin.
    pub fn reset_origin(&mut self) {
        self.origin = self.pos;
    }

    /// The whole underlying buffer, for absolute-offset work.
    pub fn buffer(&self) -> &'a [u8] {
        self.buf
    }

    /// Bytes not yet consumed.
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    /// Whether the stream is fully consumed.
    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    /// Skips forward to the next multiple of `align`, refusing to move past
    /// the end of the buffer.
    pub fn align_to(&mut self, align: usize) -> Result<()> {
        debug_assert!(align.is_power_of_two());
        let pad = (align - (self.position() % align)) % align;
        let wanted = self.pos + pad;
        if wanted > self.buf.len() {
            return Err(Error::AlignmentOutOfRange { wanted, len: self.buf.len() });
        }
        self.pos = wanted;
        Ok(())
    }

    /// Validates a length prefix or element count against what is actually
    /// present, before anything is allocated or looped over.
    ///
    /// `min_element_size` is the smallest number of bytes one element can
    /// occupy, so a count that could not possibly fit is rejected up front
    /// rather than discovered element by element.
    pub fn validate_count(&self, count: u32, min_element_size: usize) -> Result<usize> {
        let count = count as usize;
        let floor = count.saturating_mul(min_element_size.max(1));
        if floor > self.remaining() {
            return Err(Error::BadLength(count as u32));
        }
        Ok(count)
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(Error::Truncated { need: n, have: self.remaining() });
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }

    // ── primitives ──────────────────────────────────────────────────────────

    /// Reads one unaligned byte.
    pub fn get_u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    /// Reads `n` raw bytes with no alignment.
    pub fn get_bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.take(n)
    }

    /// Reads an IDL `boolean`.
    pub fn get_bool(&mut self) -> Result<bool> {
        Ok(self.get_u8()? != 0)
    }

    /// Reads an IDL `unsigned short`.
    pub fn get_u16(&mut self) -> Result<u16> {
        self.align_to(2)?;
        let b = self.take(2)?;
        Ok(match self.endian {
            Endian::Big => u16::from_be_bytes([b[0], b[1]]),
            Endian::Little => u16::from_le_bytes([b[0], b[1]]),
        })
    }

    /// Reads an IDL `short`.
    pub fn get_i16(&mut self) -> Result<i16> {
        Ok(self.get_u16()? as i16)
    }

    /// Reads an IDL `unsigned long`.
    pub fn get_u32(&mut self) -> Result<u32> {
        self.align_to(4)?;
        let b = self.take(4)?;
        Ok(match self.endian {
            Endian::Big => u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
            Endian::Little => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
        })
    }

    /// Reads an IDL `long`.
    pub fn get_i32(&mut self) -> Result<i32> {
        Ok(self.get_u32()? as i32)
    }

    /// Reads an IDL `unsigned long long`.
    pub fn get_u64(&mut self) -> Result<u64> {
        self.align_to(8)?;
        let b = self.take(8)?;
        let a: [u8; 8] = b.try_into().expect("take(8) yields 8 bytes");
        Ok(match self.endian {
            Endian::Big => u64::from_be_bytes(a),
            Endian::Little => u64::from_le_bytes(a),
        })
    }

    /// Reads an IDL `long long`.
    pub fn get_i64(&mut self) -> Result<i64> {
        Ok(self.get_u64()? as i64)
    }

    /// Reads an IDL `float`.
    pub fn get_f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.get_u32()?))
    }

    /// Reads an IDL `double`.
    pub fn get_f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.get_u64()?))
    }

    /// Reads an IDL `long double` as 16 big-endian octets. See
    /// [`Encoder::put_long_double`] for why they stay octets and why
    /// big-endian is the form the caller sees whatever the wire said.
    pub fn get_long_double(&mut self) -> Result<[u8; 16]> {
        self.align_to(8)?;
        let b = self.take(16)?;
        let raw: [u8; 16] = b.try_into().expect("take(16) yields 16 bytes");
        Ok(wire_long_double(raw, self.endian))
    }

    /// Reads an IDL `string` and returns its bytes without the terminating
    /// NUL. No codeset conversion is applied.
    ///
    /// Rejects embedded NULs: a C peer stops at the first one, so accepting a
    /// string that we and the peer would read differently is a truncation
    /// primitive against anything that gates on the value.
    pub fn get_string_bytes(&mut self) -> Result<&'a [u8]> {
        let len = self.get_u32()?;
        if len == 0 {
            return Err(Error::Malformed("string length must include the NUL"));
        }
        if len as usize > self.remaining() {
            return Err(Error::BadLength(len));
        }
        let raw = self.take(len as usize)?;
        match raw.last() {
            Some(0) => {
                let body = &raw[..raw.len() - 1];
                if body.contains(&0) {
                    return Err(Error::EmbeddedNul);
                }
                Ok(body)
            }
            _ => Err(Error::MissingNul),
        }
    }

    /// Reads an IDL `string` in the stream's transmission codeset.
    ///
    /// With no codec attached this is the strict UTF-8 it has always been,
    /// `Error::BadUtf8` and all. One method rather than two: a
    /// `get_string_with_codec` beside a `get_string` is a seam, and the seam
    /// is where the two answers drift apart.
    pub fn get_string(&mut self) -> Result<String> {
        let codec = self.codec.clone();
        let bytes = self.get_string_bytes()?;
        match codec {
            None => std::str::from_utf8(bytes).map(str::to_owned).map_err(|_| Error::BadUtf8),
            Some(c) => c.decode_narrow(bytes),
        }
    }

    /// Reads a `sequence<octet>`.
    pub fn get_octet_seq(&mut self) -> Result<&'a [u8]> {
        let len = self.get_u32()?;
        let n = self.validate_count(len, 1)?;
        self.take(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(e: Encoder) -> Vec<u8> {
        e.finish().expect("encoder was not poisoned")
    }

    /// The alignment hazards from `corpus/golden/02-alignment.idl`, checked as
    /// exact byte offsets rather than just a round-trip.
    #[test]
    fn ragged_struct_alignment() {
        let mut e = Encoder::new(Endian::Big);
        e.put_octet(0xAA); // 0
        e.put_i32(1); // pad 1..4, value at 4
        e.put_i16(2); // 8
        e.put_f64(3.0); // pad 10..16, value at 16
        e.put_octet(0xBB); // 24
        let b = bytes(e);

        assert_eq!(b.len(), 25);
        assert_eq!(&b[1..4], &[0, 0, 0], "octet->long needs 3 pad bytes");
        assert_eq!(&b[4..8], &1i32.to_be_bytes());
        assert_eq!(&b[8..10], &2i16.to_be_bytes());
        assert_eq!(&b[10..16], &[0; 6], "short->double needs 6 pad bytes");
        assert_eq!(&b[16..24], &3.0f64.to_be_bytes());
        assert_eq!(b[24], 0xBB);
    }

    #[test]
    fn ragged_struct_round_trips() {
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            e.put_octet(0xAA);
            e.put_i32(-1);
            e.put_i16(2);
            e.put_f64(3.5);
            e.put_octet(0xBB);
            let raw = bytes(e);

            let mut d = Decoder::new(&raw, endian);
            assert_eq!(d.get_u8().unwrap(), 0xAA);
            assert_eq!(d.get_i32().unwrap(), -1);
            assert_eq!(d.get_i16().unwrap(), 2);
            assert_eq!(d.get_f64().unwrap(), 3.5);
            assert_eq!(d.get_u8().unwrap(), 0xBB);
            assert!(d.is_empty());
        }
    }

    #[test]
    fn string_carries_its_nul() {
        let mut e = Encoder::new(Endian::Little);
        e.put_str("hello");
        let b = bytes(e);
        assert_eq!(&b[0..4], &6u32.to_le_bytes(), "length counts the NUL");
        assert_eq!(&b[4..9], b"hello");
        assert_eq!(b[9], 0);

        let mut d = Decoder::new(&b, Endian::Little);
        assert_eq!(d.get_string().unwrap(), "hello");
    }

    #[test]
    fn empty_string_is_just_a_nul() {
        let mut e = Encoder::new(Endian::Big);
        e.put_str("");
        let b = bytes(e);
        assert_eq!(b, vec![0, 0, 0, 1, 0]);
        let mut d = Decoder::new(&b, Endian::Big);
        assert_eq!(d.get_string().unwrap(), "");
    }

    /// Numeric extremes from `corpus/golden/17-boundaries.idl`.
    #[test]
    fn boundary_values_round_trip() {
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            e.put_i16(i16::MIN);
            e.put_i16(i16::MAX);
            e.put_i32(i32::MIN);
            e.put_i32(i32::MAX);
            e.put_i64(i64::MIN);
            e.put_i64(i64::MAX);
            e.put_u64(u64::MAX);
            let raw = bytes(e);

            let mut d = Decoder::new(&raw, endian);
            assert_eq!(d.get_i16().unwrap(), i16::MIN);
            assert_eq!(d.get_i16().unwrap(), i16::MAX);
            assert_eq!(d.get_i32().unwrap(), i32::MIN);
            assert_eq!(d.get_i32().unwrap(), i32::MAX);
            assert_eq!(d.get_i64().unwrap(), i64::MIN);
            assert_eq!(d.get_i64().unwrap(), i64::MAX);
            assert_eq!(d.get_u64().unwrap(), u64::MAX);
        }
    }

    #[test]
    fn encapsulation_restarts_alignment() {
        let mut inner = Encoder::encapsulation(Endian::Little);
        inner.put_i32(0x11223344);
        let raw = bytes(inner);
        assert_eq!(raw[0], 1, "little-endian flag");
        assert_eq!(&raw[1..4], &[0, 0, 0], "pad to alignment 4");
        assert_eq!(&raw[4..8], &0x11223344u32.to_le_bytes());

        let mut d = Decoder::encapsulation(&raw).unwrap();
        assert_eq!(d.endian(), Endian::Little);
        assert_eq!(d.get_i32().unwrap(), 0x11223344);
    }

    #[test]
    fn truncation_is_an_error_not_a_panic() {
        let raw = [0u8, 0, 0];
        let mut d = Decoder::new(&raw, Endian::Big);
        assert!(matches!(d.get_u32(), Err(Error::Truncated { .. })));
    }

    #[test]
    fn implausible_length_is_rejected() {
        let mut b = Vec::new();
        b.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
        let mut d = Decoder::new(&b, Endian::Big);
        assert!(matches!(d.get_octet_seq(), Err(Error::BadLength(_))));
    }

    /// 16 octets, 8-aligned, and **byte-ordered**. A `long double` that
    /// round-tripped through `f64` would lose the precision the peer sent, so
    /// the octets stay octets — but they are a primitive's octets, and
    /// CORBA 3.4 Part 2 Figure 9.2 draws the little-endian column of a
    /// `long double` as the big-endian column read from octet 15 back to
    /// octet 0: `s e1` sits at index 0 big-endian and at index 15
    /// little-endian.
    ///
    /// This test used to assert byte *transparency* — that the wire bytes were
    /// whatever the caller passed, in either order. That is a convention the
    /// encoder and the decoder shared with each other and with no conformant
    /// peer, and asserting it made the defect look like a requirement.
    /// omniORB 4.3.4 cannot settle it on this host: `cdrMarshal(_tc_longdouble,
    /// …)` raises `NO_IMPLEMENT_Unsupported`, because arm64 macOS has no
    /// 128-bit `long double` to marshal from. The specification is the oracle
    /// here, and the bytes below are built from the figure.
    #[test]
    fn long_double_octets_reverse_for_a_little_endian_stream() {
        let value: [u8; 16] = std::array::from_fn(|i| (i as u8) * 7 + 1);
        let reversed: [u8; 16] = std::array::from_fn(|i| value[15 - i]);
        assert_ne!(value, reversed, "a palindromic fixture would assert nothing");

        for (endian, on_the_wire) in [(Endian::Big, &value), (Endian::Little, &reversed)] {
            let mut e = Encoder::new(endian);
            e.put_octet(0xAA); // force padding before the 8-aligned value
            e.put_long_double(value);
            let raw = bytes(e);
            assert_eq!(raw.len(), 24, "1 byte + 7 pad + 16");
            assert_eq!(&raw[8..24], on_the_wire.as_slice(), "{endian:?} octet order");

            let mut d = Decoder::new(&raw, endian);
            assert_eq!(d.get_u8().unwrap(), 0xAA);
            assert_eq!(d.get_long_double().unwrap(), value, "{endian:?} round trip");
        }
    }

    #[test]
    fn korean_text_survives_as_utf8() {
        // Codeset negotiation is a wire concern; here we only assert the CDR
        // layer is byte-transparent. See spikes/ for the negotiated case.
        let mut e = Encoder::new(Endian::Big);
        e.put_str("함정 전투체계");
        let raw = bytes(e);
        let mut d = Decoder::new(&raw, Endian::Big);
        assert_eq!(d.get_string().unwrap(), "함정 전투체계");
    }

    #[test]
    fn patch_u32_rewrites_in_stream_order() {
        let mut e = Encoder::new(Endian::Little);
        e.put_u32(0);
        e.put_u32(7);
        e.patch_u32(0, 0xDEAD_BEEF);
        let b = bytes(e);
        assert_eq!(&b[0..4], &0xDEAD_BEEFu32.to_le_bytes());
        assert_eq!(&b[4..8], &7u32.to_le_bytes());
    }

    // ── hardening, from the Phase 1 spec audit ──────────────────────────────

    /// Audit HOSTILE #3: alignment past the end used to leave an out-of-range
    /// position that a later discarded error turned into a silent rewind.
    #[test]
    fn alignment_past_the_end_is_an_error() {
        let raw = [1u8, 2, 3];
        let mut d = Decoder::new(&raw, Endian::Big);
        d.get_u8().unwrap();
        assert!(matches!(d.align_to(8), Err(Error::AlignmentOutOfRange { .. })));
        assert_eq!(d.offset(), 1, "a failed alignment must not move the cursor");
    }

    /// Audit HOSTILE #3: seeking is explicit and cannot silently land at 0.
    #[test]
    fn seek_past_the_end_is_an_error() {
        let raw = [1u8, 2, 3];
        let mut d = Decoder::new(&raw, Endian::Big);
        assert!(matches!(d.seek_to(99), Err(Error::AlignmentOutOfRange { .. })));
        assert_eq!(d.offset(), 0);
        assert!(d.seek_to(3).is_ok());
    }

    /// Audit HOSTILE #4: a peer must not be able to show us one string and a
    /// C-based ORB another.
    #[test]
    fn embedded_nul_is_rejected_on_read() {
        let mut e = Encoder::new(Endian::Big);
        e.put_u32(18); // length including the terminator
        e.put_bytes(b"shutdown\0harmless");
        e.put_u8(0);
        let raw = bytes(e);

        let mut d = Decoder::new(&raw, Endian::Big);
        assert!(matches!(d.get_string_bytes(), Err(Error::EmbeddedNul)));
    }

    #[test]
    fn embedded_nul_poisons_the_encoder() {
        let mut e = Encoder::new(Endian::Big);
        e.put_str("shutdown\0harmless");
        assert!(matches!(e.finish(), Err(Error::EmbeddedNul)));
    }

    #[test]
    fn poison_survives_later_writes_and_reaches_finish() {
        let mut e = Encoder::new(Endian::Big);
        e.put_str("bad\0value");
        e.put_i32(1);
        e.put_str("fine");
        assert!(matches!(e.finish(), Err(Error::EmbeddedNul)));
    }

    #[test]
    fn poison_propagates_out_of_an_encapsulation() {
        let mut inner = Encoder::encapsulation(Endian::Big);
        inner.put_str("bad\0value");
        let mut outer = Encoder::new(Endian::Big);
        outer.put_encapsulation(inner);
        assert!(matches!(outer.finish(), Err(Error::EmbeddedNul)));
    }

    /// Audit HOSTILE #6: §9.3.3 makes the flag a boolean, so 0x37 is malformed.
    #[test]
    fn nonboolean_byte_order_flag_is_rejected() {
        assert!(Endian::try_from_flag(0).is_ok());
        assert!(Endian::try_from_flag(1).is_ok());
        assert!(matches!(Endian::try_from_flag(0x37), Err(Error::Malformed(_))));
        let raw = [0x37u8, 0, 0, 0];
        assert!(Decoder::encapsulation(&raw).is_err());
    }

    /// Audit HOSTILE #5: a count is checked against what is present before it
    /// is looped over, so a crafted count cannot drive a long loop.
    #[test]
    fn element_count_is_validated_against_remaining() {
        let raw = [0u8; 16];
        let d = Decoder::new(&raw, Endian::Big);
        assert!(d.validate_count(4, 4).is_ok(), "16 bytes fits 4 x 4");
        assert!(matches!(d.validate_count(5, 4), Err(Error::BadLength(5))));
        assert!(matches!(d.validate_count(u32::MAX, 1), Err(Error::BadLength(_))));
        // A zero-size element still costs at least one byte to distinguish.
        assert!(d.validate_count(16, 0).is_ok());
        assert!(matches!(d.validate_count(17, 0), Err(Error::BadLength(17))));
    }
}
