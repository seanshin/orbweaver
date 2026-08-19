//! Codeset negotiation: `TAG_CODE_SETS`, the `CodeSets` service context, and
//! the conversion the negotiated result implies.
//!
//! # Why this is not optional
//!
//! CORBA 3.4 §7.10.2.5: *"if no char transmission code set is specified in the
//! code set service context, then the char transmission code set is considered
//! to be ISO 8859-1 for backward compatibility."*
//!
//! So a client that sends no context and puts UTF-8 on the wire has, by
//! specification, declared those bytes to be Latin-1. A peer that actually
//! converts will mangle them. Captured from omniORB 4.3.4 on the wire, its
//! outbound context is `char TCS = 0x00010001` (ISO 8859-1) and
//! `wchar TCS = 0x00010109` (UTF-16) — so Korean text surviving a round trip
//! against omniORB reflects that peer passing bytes through unconverted, not
//! agreement about what they mean.
//!
//! Spec: CORBA 3.4 Part 2, §7.6.6.5 (`TAG_CODE_SETS`), §7.10.2 (negotiation).

use orbweaver_cdr::{Decoder, Encoder, Endian};

use crate::{Error, Result};

/// `IOP::ServiceId` for the codeset context.
pub const SERVICE_ID_CODE_SETS: u32 = 1;

/// `IOP::ComponentId` for the codeset component of an IIOP profile.
pub const TAG_CODE_SETS: u32 = 1;

/// An OSF Character and Code Set Registry identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CodeSetId(pub u32);

impl CodeSetId {
    /// ISO 8859-1:1987 (Latin-1). The specified default when no context is
    /// sent, and omniORB's native char set. Verified on the wire.
    pub const ISO_8859_1: CodeSetId = CodeSetId(0x0001_0001);
    /// ISO/IEC 10646-1:1993 UTF-16. Verified on the wire.
    pub const UTF_16: CodeSetId = CodeSetId(0x0001_0109);
    /// ISO/IEC 10646-1:1993 UCS-2 Level 1.
    pub const UCS_2: CodeSetId = CodeSetId(0x0001_0100);
    /// X/Open UTF-8.
    pub const UTF_8: CodeSetId = CodeSetId(0x0501_0001);
    /// ISO 646:1991 IRV (7-bit ASCII).
    pub const ASCII: CodeSetId = CodeSetId(0x0001_0020);
    /// EUC-KR (Windows-949 / Unified Hangul Code). Conversion requires the
    /// `euc-kr` feature; see `NOTICE` and `docs/decisions/D001`.
    pub const EUC_KR: CodeSetId = CodeSetId(0x0004_0002);

    /// A human-readable name, for diagnostics.
    pub fn name(self) -> &'static str {
        match self {
            CodeSetId::ISO_8859_1 => "ISO-8859-1",
            CodeSetId::UTF_16 => "UTF-16",
            CodeSetId::UCS_2 => "UCS-2",
            CodeSetId::UTF_8 => "UTF-8",
            CodeSetId::ASCII => "US-ASCII",
            CodeSetId::EUC_KR => "EUC-KR",
            _ => "unregistered",
        }
    }

    /// Whether this build can convert to and from it.
    ///
    /// EUC-KR depends on the `euc-kr` feature, which pulls in `encoding_rs`
    /// and its BSD-3-Clause attribution for the WHATWG mapping data. Building
    /// without it removes both the support and the obligation, so this is a
    /// build-time question rather than a fixed list.
    // Written out rather than as `matches!`, because one arm is a build-time
    // question and folding it in hides that EUC-KR is conditional.
    #[allow(clippy::match_like_matches_macro)]
    pub fn is_supported(self) -> bool {
        match self {
            CodeSetId::ISO_8859_1 | CodeSetId::UTF_8 | CodeSetId::ASCII | CodeSetId::UTF_16 => true,
            CodeSetId::EUC_KR => cfg!(feature = "euc-kr"),
            _ => false,
        }
    }
}

impl std::fmt::Display for CodeSetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (0x{:08X})", self.name(), self.0)
    }
}

/// What one side supports for either char or wchar data.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodeSetComponent {
    /// The codeset this side uses internally.
    pub native: Option<CodeSetId>,
    /// Codesets it is willing to convert to and from.
    pub conversions: Vec<CodeSetId>,
}

impl CodeSetComponent {
    /// Whether this side can handle `id` at all.
    pub fn supports(&self, id: CodeSetId) -> bool {
        self.native == Some(id) || self.conversions.contains(&id)
    }

    /// Writes this side's declaration into an open encapsulation.
    ///
    /// A native of `None` is written as codeset 0, which is how
    /// [`read_component`] reads "unspecified" back — the two halves of that
    /// convention live next to each other on purpose.
    fn write(&self, e: &mut Encoder) {
        e.put_u32(self.native.map_or(0, |c| c.0));
        e.put_u32(self.conversions.len() as u32);
        for c in &self.conversions {
            e.put_u32(c.0);
        }
    }
}

/// The contents of a `TAG_CODE_SETS` component: what a server declares.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodeSetComponentInfo {
    /// Support for `char` and `string`.
    pub for_char: CodeSetComponent,
    /// Support for `wchar` and `wstring`.
    pub for_wchar: CodeSetComponent,
}

impl CodeSetComponentInfo {
    /// Parses the encapsulated body of a `TAG_CODE_SETS` component.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut d = Decoder::encapsulation(data)?;
        Ok(CodeSetComponentInfo {
            for_char: read_component(&mut d)?,
            for_wchar: read_component(&mut d)?,
        })
    }

    /// Encodes the component body as the encapsulation an IOR carries.
    pub fn encode(&self, endian: Endian) -> Result<Vec<u8>> {
        let mut e = Encoder::encapsulation(endian);
        self.for_char.write(&mut e);
        self.for_wchar.write(&mut e);
        e.finish().map_err(Error::Cdr)
    }
}

/// What this implementation declares in the `TAG_CODE_SETS` component of every
/// reference it publishes.
///
/// **Both conversion lists are empty, and that is the honest answer rather than
/// an omission.** §7.6.6.5 defines `conversion_code_sets` as the sets this side
/// is *willing to convert to and from*; nothing in this workspace converts a
/// `string` argument on the way out or on the way in — `Encoder::put_str` writes
/// UTF-8 and `Decoder::get_string` reads UTF-8, and a servant sees the result of
/// those. Declaring a conversion we do not perform is the same defect as
/// negotiating one and ignoring it, one layer further out: it invites a peer to
/// send Latin-1 that our servants would then read as malformed UTF-8.
///
/// Measured against both peers before choosing the narrow declaration, because
/// "honest" is worth nothing if it is also unusable:
///
/// - omniORB 4.3.4 publishes `char` native ISO-8859-1 with UTF-8 in its
///   conversion list, so §7.10.2.6 case 3 agrees on **UTF-8** and omniORB
///   converts on its side.
/// - JacORB 3.9 publishes `char` native UTF-8, so case 1 agrees on **UTF-8**
///   with nobody converting.
///
/// Both reach UTF-8 without us claiming a conversion. A peer that can reach
/// neither genuinely cannot exchange `char` data with us, and §7.10.2.6's
/// `CODESET_INCOMPATIBLE` is the right answer rather than mojibake.
///
/// `wchar` is UTF-16 native with no conversions for the same reason: the wide
/// path is UTF-16 end to end (§9.3.1.6, and the byte sequences recorded in
/// `tests/wide_chars_from_a_peer.rs`), and both peers publish UTF-16 native.
///
/// # Batch 4 of D009 §8 asked to grow this list, and could not
///
/// Since batch 3 a servant *does* honour a declared conversion, so the reason
/// this list was empty — "nothing converts" — stopped being true. What kept it
/// empty is the condition the decision attaches: a non-empty list lands only
/// against **a peer advertising ISO-8859-1 without UTF-8 in its conversion
/// list**. `spikes/codeset_peer_probe.py` went looking for one and reports
/// **BLOCKED**: ten configurations were measured, five of omniORB 4.3.4 and
/// five of JacORB 3.9, and every one of them reaches UTF-8. Neither ORB has any
/// option that names its *conversion* list — omniORB offers
/// `nativeCharCodeSet`/`defaultCharCodeSet`, JacORB `jacorb.native_char_codeset`
/// and `jacorb.native_wchar_codeset` — so the list follows the build, and this
/// machine cannot produce the peer.
///
/// `spikes/codeset_advertise_probe.py` then measured what growing it would have
/// cost, by publishing the proposed component to unmodified peers: omniORB kept
/// sending UTF-8, while **JacORB with native `char` ISO-8859-1 moved down to
/// ISO-8859-1** and then truncated Korean text to one octet per character
/// without raising anything. The two implementations resolve §7.10.2.6's open
/// case in opposite directions, and an empty list is what keeps that ambiguity
/// out of reach. Both measurements are pinned in
/// `tests/codesets_on_the_wire.rs`.
///
/// *배치 4는 목록을 늘리려 했고, 늘릴 수 없었다.* UTF-8에 닿지 못하는 피어를
/// 열 가지 구성에서 찾지 못했고, 늘렸을 때의 대가는 측정되었다 — JacORB는 더
/// 좁은 코드셋으로 내려가 한글을 조용히 잘라 보냈다.
pub fn server_component_info() -> CodeSetComponentInfo {
    CodeSetComponentInfo {
        for_char: CodeSetComponent { native: Some(CodeSetId::UTF_8), conversions: Vec::new() },
        for_wchar: CodeSetComponent { native: Some(CodeSetId::UTF_16), conversions: Vec::new() },
    }
}

/// The `TAG_CODE_SETS` component every published reference carries.
///
/// # Why a reference without one is not merely incomplete
///
/// §7.10.2.4 makes an absent codeset component a *statement*: the client is to
/// assume the server's native `char` set is ISO 8859-1 and that **there is no
/// `wchar` support at all**. A conformant client then refuses to marshal a
/// `wchar` or `wstring` argument rather than sending one — measured, not
/// inferred: omniORB 4.3.4 raises `INV_OBJREF` minor `0x4F4D0001`
/// (`OMGVMCID | 1`) from inside the client on `echo_wstring`, having sent
/// nothing, while `echo_string` on the same reference over the same connection
/// succeeds. Our server log recorded one request and no error, which is exactly
/// how a servant with a `wstring` in its interface can be unreachable and
/// silent about it.
///
/// Little-endian by construction, matching [`crate::Ior::to_stringified`]: an
/// encapsulation carries its own byte-order flag, so the choice is free, and
/// making it the same one everywhere keeps published bytes reproducible.
pub fn server_component() -> crate::TaggedComponent {
    crate::TaggedComponent {
        tag: TAG_CODE_SETS,
        // Six `u32`s into a fresh encapsulation. `Encoder::finish` fails only
        // for a poisoned encoder, and the only thing that poisons one is a
        // string with an embedded NUL — there are no strings here.
        data: server_component_info()
            .encode(Endian::Little)
            .expect("a fixed-shape codeset component has nothing that can fail to encode"),
    }
}

fn read_component(d: &mut Decoder<'_>) -> Result<CodeSetComponent> {
    let native = CodeSetId(d.get_u32()?);
    let count = d.get_u32()?;
    let count = d.validate_count(count, 4)?;
    let mut conversions = Vec::with_capacity(count);
    for _ in 0..count {
        conversions.push(CodeSetId(d.get_u32()?));
    }
    // A native of zero means "unspecified" rather than codeset 0x00000000.
    let native = if native.0 == 0 { None } else { Some(native) };
    Ok(CodeSetComponent { native, conversions })
}

/// The `CodeSets` service context: what a client tells the server it is
/// actually transmitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeSetContext {
    /// Transmission codeset for `char` and `string`.
    pub char_data: CodeSetId,
    /// Transmission codeset for `wchar` and `wstring`.
    pub wchar_data: CodeSetId,
}

impl CodeSetContext {
    /// Encodes the context body as a CDR encapsulation.
    pub fn encode(&self, endian: Endian) -> Result<Vec<u8>> {
        let mut e = Encoder::encapsulation(endian);
        e.put_u32(self.char_data.0);
        e.put_u32(self.wchar_data.0);
        e.finish().map_err(Error::Cdr)
    }

    /// Decodes a `CodeSets` service context body.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut d = Decoder::encapsulation(data)?;
        Ok(CodeSetContext {
            char_data: CodeSetId(d.get_u32()?),
            wchar_data: CodeSetId(d.get_u32()?),
        })
    }
}

/// Why negotiation could not produce a usable codeset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegotiationError {
    /// No codeset is acceptable to both sides. Maps to `CODESET_INCOMPATIBLE`.
    Incompatible {
        /// What we offered.
        client_native: CodeSetId,
        /// What the peer declared native, if anything.
        server_native: Option<CodeSetId>,
    },
    /// A codeset was agreed but this crate cannot convert to it.
    ///
    /// Distinct from `Incompatible` on purpose: the peers agree and the gap is
    /// ours, which is a different bug report and a different fix.
    Unsupported(CodeSetId),
    /// The server declared no wchar codeset but wide data is required.
    /// `INV_OBJREF` minor 1 in §7.10.2.6.
    NoWcharCodeSet,
    /// The codeset is supported, but this particular text has no
    /// representation in it. `DATA_CONVERSION` in CORBA terms.
    Untranslatable {
        /// The codeset that cannot carry the text.
        codeset: CodeSetId,
        /// A short excerpt, so the report names the offending data.
        text: String,
    },
    /// Received bytes are not valid in the negotiated codeset.
    Malformed {
        /// The codeset the bytes were supposed to be in.
        codeset: CodeSetId,
    },
}

/// Keeps a diagnostic readable without swallowing the evidence.
///
/// Only the EUC-KR path can reject a whole string; `put_wchar` rejects one
/// character and has nothing to truncate. The cfg keeps the attribution-free
/// build warning-clean, which the harness now enforces rather than assumes.
#[cfg(feature = "euc-kr")]
fn truncate_for_message(s: &str) -> String {
    const MAX: usize = 32;
    if s.chars().count() <= MAX {
        return s.to_owned();
    }
    let head: String = s.chars().take(MAX).collect();
    format!("{head}…")
}

impl std::fmt::Display for NegotiationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NegotiationError::Incompatible { client_native, server_native } => write!(
                f,
                "no common codeset: we use {client_native}, peer declared {}",
                match server_native {
                    Some(s) => s.to_string(),
                    None => "none".into(),
                }
            ),
            NegotiationError::Unsupported(id) => {
                write!(f, "negotiated {id}, which this build cannot convert")
            }
            NegotiationError::NoWcharCodeSet => {
                write!(f, "peer declared no wchar codeset")
            }
            NegotiationError::Untranslatable { codeset, text } => {
                write!(f, "text has no representation in {codeset}: {text:?}")
            }
            NegotiationError::Malformed { codeset } => {
                write!(f, "received bytes are not valid {codeset}")
            }
        }
    }
}

/// What this implementation offers for `char` data.
///
/// Conversion order is preference order (§7.10.2.6 case 4 resolves by it), so
/// EUC-KR sits ahead of the Latin sets: against a Korean peer that offers both,
/// choosing EUC-KR carries the text and choosing ISO-8859-1 destroys it.
///
/// This list is wider than [`server_component_info`]'s because the two sides
/// have different means. A client that agrees on a non-UTF-8 codeset can honour
/// it — [`crate::Connection::convert_chars`] hands the caller the [`Converter`]
/// and the caller encodes each `string` argument through it before writing the
/// octets. There is no servant-side equivalent (a servant is handed a
/// `Decoder`, not a string), so the published component claims nothing.
///
/// Offering a conversion is a promise, so a connection that agrees on one and
/// finds nobody willing to keep it now refuses to send rather than putting UTF-8
/// octets under a declaration that says otherwise. See
/// [`crate::Error::CodesetNotApplied`].
pub fn client_char_component() -> CodeSetComponent {
    let mut conversions = Vec::new();
    if cfg!(feature = "euc-kr") {
        conversions.push(CodeSetId::EUC_KR);
    }
    conversions.push(CodeSetId::ISO_8859_1);
    conversions.push(CodeSetId::ASCII);
    CodeSetComponent { native: Some(CodeSetId::UTF_8), conversions }
}

/// What this implementation offers for `wchar` data.
pub fn client_wchar_component() -> CodeSetComponent {
    CodeSetComponent { native: Some(CodeSetId::UTF_16), conversions: vec![] }
}

/// How much text a codeset can carry, used to break ties the spec leaves open.
///
/// §7.10.2.6 says a match between one side's native set and the other's
/// conversion list is acceptable, but does **not** say which direction wins
/// when both hold. That latitude is not neutral: against omniORB, whose native
/// char set is ISO-8859-1 and whose conversion list includes UTF-8, taking the
/// peer's native would agree on Latin-1 and silently make Korean text
/// unrepresentable, while taking ours agrees on UTF-8 and carries it.
///
/// So among the mutually acceptable candidates we choose the widest
/// repertoire. Picking a narrower codeset is a data-loss decision taken at
/// connection setup, before anyone knows what text will actually flow.
fn repertoire_rank(id: CodeSetId) -> u8 {
    match id {
        CodeSetId::UTF_8 => 5,  // all of Unicode, and compact for ASCII
        CodeSetId::UTF_16 => 4, // all of Unicode
        CodeSetId::EUC_KR => 3, // Korean plus ASCII
        CodeSetId::ISO_8859_1 => 2,
        CodeSetId::ASCII => 1,
        _ => 0,
    }
}

/// Chooses a transmission codeset, following §7.10.2.6.
pub fn negotiate(
    client: &CodeSetComponent,
    server: &CodeSetComponent,
) -> std::result::Result<CodeSetId, NegotiationError> {
    let client_native = client.native.ok_or(NegotiationError::NoWcharCodeSet)?;

    // 1. Identical native sets: transmit as-is, nobody converts.
    if server.native == Some(client_native) {
        return check_supported(client_native);
    }

    // 2-4. Collect every candidate the spec permits, then choose among them by
    //      repertoire rather than by the order they happen to be listed in.
    let mut candidates: Vec<CodeSetId> = Vec::new();
    if let Some(sn) = server.native
        && client.conversions.contains(&sn)
    {
        candidates.push(sn); // we convert to the peer's native
    }
    if server.conversions.contains(&client_native) {
        candidates.push(client_native); // the peer converts to ours
    }
    candidates
        .extend(client.conversions.iter().filter(|c| server.conversions.contains(c)).copied());

    // 5. §7.10.2.6 allows a universal fallback when both sides can reach it.
    if client.supports(CodeSetId::UTF_8) && server.supports(CodeSetId::UTF_8) {
        candidates.push(CodeSetId::UTF_8);
    }

    // Widest repertoire wins; ties break on the lower registry id so the
    // result never depends on iteration order.
    if let Some(best) = candidates
        .iter()
        .filter(|c| c.is_supported())
        .max_by_key(|c| (repertoire_rank(**c), std::cmp::Reverse(c.0)))
    {
        return Ok(*best);
    }
    // Something was agreed but we cannot convert it — report that distinctly.
    if let Some(agreed) = candidates.first() {
        return Err(NegotiationError::Unsupported(*agreed));
    }

    // Before declaring the peers incompatible, check whether the peer asked for
    // something this crate knows about but was built without. That is a
    // different situation and deserves a different report: "rebuild with the
    // feature" rather than "the peer is misconfigured", which would send an
    // operator hunting a problem that does not exist.
    if let Some(missing) = server
        .native
        .into_iter()
        .chain(server.conversions.iter().copied())
        .find(|id| compiled_out(*id))
    {
        return Err(NegotiationError::Unsupported(missing));
    }

    Err(NegotiationError::Incompatible { client_native, server_native: server.native })
}

/// Whether a codeset has an implementation in this crate that the current
/// build excluded, as opposed to one we have never implemented.
fn compiled_out(id: CodeSetId) -> bool {
    match id {
        CodeSetId::EUC_KR => !cfg!(feature = "euc-kr"),
        _ => false,
    }
}

fn check_supported(id: CodeSetId) -> std::result::Result<CodeSetId, NegotiationError> {
    if id.is_supported() { Ok(id) } else { Err(NegotiationError::Unsupported(id)) }
}

/// Converts between Rust strings and a transmission codeset.
///
/// EUC-KR is deliberately absent rather than approximated. Its 17,048-entry
/// table is data we cannot originate, and the licensing question is recorded
/// in `docs/decisions/D001`. Returning `Unsupported` is honest; guessing at
/// Korean text is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Converter {
    id: CodeSetId,
}

impl Converter {
    /// A converter for a negotiated codeset.
    pub fn new(id: CodeSetId) -> std::result::Result<Self, NegotiationError> {
        if id.is_supported() {
            Ok(Converter { id })
        } else {
            Err(NegotiationError::Unsupported(id))
        }
    }

    /// The codeset being converted to.
    pub fn id(self) -> CodeSetId {
        self.id
    }

    /// Encodes a string into transmission bytes.
    pub fn encode(self, s: &str) -> std::result::Result<Vec<u8>, NegotiationError> {
        match self.id {
            CodeSetId::UTF_8 => Ok(s.as_bytes().to_vec()),
            CodeSetId::ISO_8859_1 | CodeSetId::ASCII => {
                let limit = if self.id == CodeSetId::ASCII { 0x7F } else { 0xFF };
                s.chars()
                    .map(|c| {
                        let n = c as u32;
                        if n <= limit {
                            Ok(n as u8)
                        } else {
                            // Silently substituting here is how mojibake gets
                            // into a database and stays there.
                            Err(NegotiationError::Unsupported(self.id))
                        }
                    })
                    .collect()
            }
            CodeSetId::UTF_16 => {
                // §9.3.2.7: UCS-2 and UTF-16 use the message's byte order, so
                // the caller writes these as octets into an already-ordered
                // stream. Big-endian is emitted here and swapped by the writer
                // when the stream is little-endian.
                let mut out = Vec::with_capacity(s.len() * 2);
                for unit in s.encode_utf16() {
                    out.extend_from_slice(&unit.to_be_bytes());
                }
                Ok(out)
            }
            #[cfg(feature = "euc-kr")]
            CodeSetId::EUC_KR => {
                let (bytes, _, unmappable) = encoding_rs::EUC_KR.encode(s);
                // encoding_rs substitutes numeric character references for
                // characters it cannot map. That is correct for HTML and wrong
                // here: a CORBA peer would receive "&#26085;" as literal text.
                if unmappable {
                    return Err(NegotiationError::Untranslatable {
                        codeset: self.id,
                        text: truncate_for_message(s),
                    });
                }
                Ok(bytes.into_owned())
            }
            other => Err(NegotiationError::Unsupported(other)),
        }
    }

    /// Decodes transmission bytes into a string.
    pub fn decode(self, bytes: &[u8]) -> std::result::Result<String, NegotiationError> {
        match self.id {
            CodeSetId::UTF_8 => String::from_utf8(bytes.to_vec())
                .map_err(|_| NegotiationError::Unsupported(self.id)),
            CodeSetId::ISO_8859_1 => Ok(bytes.iter().map(|&b| b as char).collect()),
            CodeSetId::ASCII => {
                if bytes.iter().any(|&b| b > 0x7F) {
                    return Err(NegotiationError::Unsupported(self.id));
                }
                Ok(bytes.iter().map(|&b| b as char).collect())
            }
            CodeSetId::UTF_16 => {
                if bytes.len() % 2 != 0 {
                    return Err(NegotiationError::Unsupported(self.id));
                }
                let units: Vec<u16> =
                    bytes.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                String::from_utf16(&units).map_err(|_| NegotiationError::Unsupported(self.id))
            }
            #[cfg(feature = "euc-kr")]
            CodeSetId::EUC_KR => {
                let (text, _, malformed) = encoding_rs::EUC_KR.decode(bytes);
                // Malformed input becomes U+FFFD. Accepting that would hand the
                // caller a string that looks fine and is not what the peer sent.
                if malformed {
                    return Err(NegotiationError::Malformed { codeset: self.id });
                }
                Ok(text.into_owned())
            }
            other => Err(NegotiationError::Unsupported(other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact bytes omniORB 4.3.4 put on the wire, captured during the
    /// Phase 1 batch. Pinned so a refactor cannot quietly change what we parse.
    const OMNIORB_CONTEXT: &[u8] = &[
        0x01, 0x00, 0x00, 0x00, // little-endian encapsulation flag + pad
        0x01, 0x00, 0x01, 0x00, // char  TCS 0x00010001 ISO-8859-1
        0x09, 0x01, 0x01, 0x00, // wchar TCS 0x00010109 UTF-16
    ];

    #[test]
    fn parses_the_context_omniorb_actually_sends() {
        let ctx = CodeSetContext::parse(OMNIORB_CONTEXT).unwrap();
        assert_eq!(ctx.char_data, CodeSetId::ISO_8859_1);
        assert_eq!(ctx.wchar_data, CodeSetId::UTF_16);
    }

    #[test]
    fn context_round_trips() {
        let ctx = CodeSetContext { char_data: CodeSetId::UTF_8, wchar_data: CodeSetId::UTF_16 };
        for endian in [Endian::Big, Endian::Little] {
            let raw = ctx.encode(endian).unwrap();
            assert_eq!(CodeSetContext::parse(&raw).unwrap(), ctx);
        }
    }

    fn component_info(char_native: CodeSetId, wchar_native: CodeSetId) -> Vec<u8> {
        let mut e = Encoder::encapsulation(Endian::Little);
        e.put_u32(char_native.0);
        e.put_u32(1);
        e.put_u32(CodeSetId::UTF_8.0);
        e.put_u32(wchar_native.0);
        e.put_u32(0);
        e.finish().unwrap()
    }

    #[test]
    fn parses_a_tag_code_sets_component() {
        let raw = component_info(CodeSetId::ISO_8859_1, CodeSetId::UTF_16);
        let info = CodeSetComponentInfo::parse(&raw).unwrap();
        assert_eq!(info.for_char.native, Some(CodeSetId::ISO_8859_1));
        assert_eq!(info.for_char.conversions, vec![CodeSetId::UTF_8]);
        assert_eq!(info.for_wchar.native, Some(CodeSetId::UTF_16));
        assert!(info.for_wchar.conversions.is_empty());
    }

    #[test]
    fn native_of_zero_means_unspecified() {
        let raw = component_info(CodeSetId(0), CodeSetId::UTF_16);
        let info = CodeSetComponentInfo::parse(&raw).unwrap();
        assert_eq!(info.for_char.native, None);
    }

    // ── negotiation ─────────────────────────────────────────────────────────

    #[test]
    fn identical_natives_need_no_conversion() {
        let both = CodeSetComponent { native: Some(CodeSetId::UTF_8), conversions: vec![] };
        assert_eq!(negotiate(&both, &both).unwrap(), CodeSetId::UTF_8);
    }

    /// omniORB as it actually declares itself: native ISO-8859-1, conversion
    /// list including UTF-8. Both directions are permitted by §7.10.2.6, and
    /// the choice decides whether Korean text survives the connection.
    #[test]
    fn omniorb_shaped_peer_negotiates_utf8_not_latin1() {
        let server = CodeSetComponent {
            native: Some(CodeSetId::ISO_8859_1),
            conversions: vec![CodeSetId::UTF_8],
        };
        let chosen = negotiate(&client_char_component(), &server).unwrap();
        assert_eq!(
            chosen,
            CodeSetId::UTF_8,
            "both are legal; the wider repertoire is the one that keeps the data"
        );
        assert!(Converter::new(chosen).unwrap().encode("함정 전투체계").is_ok());
    }

    /// With no wider option available we do take the peer's native set — the
    /// preference is for repertoire, not for UTF-8 as such.
    #[test]
    fn falls_back_to_the_peers_native_when_it_is_the_only_option() {
        let server = CodeSetComponent { native: Some(CodeSetId::ISO_8859_1), conversions: vec![] };
        assert_eq!(negotiate(&client_char_component(), &server).unwrap(), CodeSetId::ISO_8859_1);
    }

    #[test]
    fn falls_back_to_our_native_when_the_peer_converts() {
        let server = CodeSetComponent {
            native: Some(CodeSetId(0x0DEA_D000)),
            conversions: vec![CodeSetId::UTF_8],
        };
        assert_eq!(negotiate(&client_char_component(), &server).unwrap(), CodeSetId::UTF_8);
    }

    /// A common conversion set is resolved by repertoire, and deterministically:
    /// ISO-8859-1 beats ASCII because it is the superset, regardless of the
    /// order either side listed them in.
    #[test]
    fn common_conversion_set_resolves_by_repertoire() {
        let client = CodeSetComponent {
            native: Some(CodeSetId(0x0AAA_0000)),
            conversions: vec![CodeSetId::ASCII, CodeSetId::ISO_8859_1],
        };
        let server = CodeSetComponent {
            native: Some(CodeSetId(0x0BBB_0000)),
            conversions: vec![CodeSetId::ASCII, CodeSetId::ISO_8859_1],
        };
        let a = negotiate(&client, &server).unwrap();
        assert_eq!(a, negotiate(&client, &server).unwrap(), "must be deterministic");
        assert_eq!(a, CodeSetId::ISO_8859_1, "superset wins over subset");
    }

    /// A Korean peer offering EUC-KR and Latin-1 must not be answered with
    /// Latin-1 just because it appears first in someone's list.
    #[cfg(feature = "euc-kr")]
    #[test]
    fn repertoire_beats_list_order_for_korean() {
        let server = CodeSetComponent {
            native: Some(CodeSetId::EUC_KR),
            conversions: vec![CodeSetId::ISO_8859_1, CodeSetId::ASCII],
        };
        assert_eq!(negotiate(&client_char_component(), &server).unwrap(), CodeSetId::EUC_KR);
    }

    #[test]
    fn incompatible_peers_are_reported_not_guessed() {
        let client = CodeSetComponent { native: Some(CodeSetId(0x0AAA_0000)), conversions: vec![] };
        let server = CodeSetComponent { native: Some(CodeSetId(0x0BBB_0000)), conversions: vec![] };
        assert!(matches!(negotiate(&client, &server), Err(NegotiationError::Incompatible { .. })));
    }

    /// EUC-KR support is a build-time property, so this asserts the right
    /// behaviour in both configurations rather than pinning one of them.
    ///
    /// With the feature on, a Korean peer negotiates successfully. With it off,
    /// the result must be `Unsupported` and never `Incompatible`: the two sides
    /// agree and the gap is ours, so the report should send someone to our
    /// build flags, not hunting a peer misconfiguration that does not exist.
    #[test]
    fn euc_kr_availability_follows_the_feature() {
        let peer = CodeSetComponent {
            native: Some(CodeSetId::EUC_KR),
            conversions: vec![CodeSetId::EUC_KR],
        };
        let outcome = negotiate(&client_char_component(), &peer);

        if cfg!(feature = "euc-kr") {
            assert_eq!(outcome.unwrap(), CodeSetId::EUC_KR);
            assert!(Converter::new(CodeSetId::EUC_KR).is_ok());
        } else {
            match outcome {
                Err(NegotiationError::Unsupported(id)) => assert_eq!(id, CodeSetId::EUC_KR),
                other => panic!("expected Unsupported(EUC-KR), got {other:?}"),
            }
            assert!(matches!(
                Converter::new(CodeSetId::EUC_KR),
                Err(NegotiationError::Unsupported(_))
            ));
        }
    }

    // ── conversion ──────────────────────────────────────────────────────────

    #[test]
    fn utf8_is_a_pass_through() {
        let c = Converter::new(CodeSetId::UTF_8).unwrap();
        let s = "함정 전투체계";
        assert_eq!(c.encode(s).unwrap(), s.as_bytes());
        assert_eq!(c.decode(s.as_bytes()).unwrap(), s);
    }

    #[test]
    fn latin1_round_trips_its_own_range() {
        let c = Converter::new(CodeSetId::ISO_8859_1).unwrap();
        let s = "café ±90°";
        let bytes = c.encode(s).unwrap();
        assert_eq!(bytes.len(), s.chars().count(), "one byte per character");
        assert_eq!(c.decode(&bytes).unwrap(), s);
    }

    /// The defect this whole module exists to prevent: sending Korean text as
    /// Latin-1 must fail loudly, because a silent substitution puts mojibake
    /// into a database where nobody notices until much later.
    #[test]
    fn latin1_refuses_korean_instead_of_mangling_it() {
        let c = Converter::new(CodeSetId::ISO_8859_1).unwrap();
        assert!(c.encode("함정 전투체계").is_err());
        let ascii = Converter::new(CodeSetId::ASCII).unwrap();
        assert!(ascii.encode("café").is_err());
    }

    #[test]
    fn utf16_round_trips_including_korean() {
        let c = Converter::new(CodeSetId::UTF_16).unwrap();
        let s = "함정 전투체계";
        let bytes = c.encode(s).unwrap();
        assert_eq!(bytes.len(), s.encode_utf16().count() * 2);
        assert_eq!(c.decode(&bytes).unwrap(), s);
    }

    #[test]
    fn utf16_rejects_an_odd_length() {
        let c = Converter::new(CodeSetId::UTF_16).unwrap();
        assert!(c.decode(&[0x00, 0xD5, 0x48]).is_err());
    }

    // ── EUC-KR (feature `euc-kr`; see NOTICE and docs/decisions/D001) ───────

    /// Cross-checked against Python's independent EUC-KR codec: the bytes are
    /// c7d4 c1a4 20 c0fc c5f5 c3bc b0e8 for "함정 전투체계". Two implementations
    /// agreeing on the exact bytes is worth more than a self-round-trip, which
    /// would pass even if the table were wrong in a self-consistent way.
    #[cfg(feature = "euc-kr")]
    #[test]
    fn euc_kr_matches_an_independent_implementation() {
        let c = Converter::new(CodeSetId::EUC_KR).unwrap();
        let s = "함정 전투체계";
        let expected =
            [0xc7, 0xd4, 0xc1, 0xa4, 0x20, 0xc0, 0xfc, 0xc5, 0xf5, 0xc3, 0xbc, 0xb0, 0xe8];
        assert_eq!(c.encode(s).unwrap(), expected);
        assert_eq!(c.decode(&expected).unwrap(), s);
        assert_eq!(
            expected.len(),
            13,
            "13 bytes in EUC-KR against 19 in UTF-8 — the whole reason peers use it"
        );
    }

    #[cfg(feature = "euc-kr")]
    #[test]
    fn euc_kr_handles_ascii_and_mixed_text() {
        let c = Converter::new(CodeSetId::EUC_KR).unwrap();
        for s in ["", "plain ascii", "IDL 4.2 명세 / spec", "가나다"] {
            assert_eq!(c.decode(&c.encode(s).unwrap()).unwrap(), s, "failed on {s:?}");
        }
    }

    /// encoding_rs substitutes HTML numeric character references for
    /// unmappable characters. Correct for a browser, catastrophic here: the
    /// peer would receive the literal text "&#26085;" instead of a character.
    #[cfg(feature = "euc-kr")]
    #[test]
    fn euc_kr_refuses_unmappable_text_rather_than_substituting() {
        let c = Converter::new(CodeSetId::EUC_KR).unwrap();
        match c.encode("emoji 🛰 does not exist in EUC-KR") {
            Err(NegotiationError::Untranslatable { codeset, text }) => {
                assert_eq!(codeset, CodeSetId::EUC_KR);
                assert!(text.contains("🛰"), "the diagnostic must name the offending data");
            }
            Ok(bytes) => panic!(
                "silently produced {} bytes instead of failing: {:?}",
                bytes.len(),
                String::from_utf8_lossy(&bytes)
            ),
            other => panic!("expected Untranslatable, got {other:?}"),
        }
    }

    #[cfg(feature = "euc-kr")]
    #[test]
    fn euc_kr_rejects_malformed_input_rather_than_replacing_it() {
        let c = Converter::new(CodeSetId::EUC_KR).unwrap();
        // 0xC7 begins a two-byte sequence; 0x20 cannot continue it.
        match c.decode(&[0xC7, 0x20, 0xFF]) {
            Err(NegotiationError::Malformed { codeset }) => {
                assert_eq!(codeset, CodeSetId::EUC_KR)
            }
            Ok(s) => panic!("accepted malformed bytes as {s:?} (U+FFFD smuggled in)"),
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    /// The negotiation this feature exists for: a Korean peer offering both
    /// EUC-KR and Latin-1 must get EUC-KR, because Latin-1 destroys the text.
    #[cfg(feature = "euc-kr")]
    #[test]
    fn korean_peer_negotiates_euc_kr_not_latin1() {
        let server = CodeSetComponent {
            native: Some(CodeSetId::EUC_KR),
            conversions: vec![CodeSetId::ISO_8859_1, CodeSetId::EUC_KR],
        };
        let chosen = negotiate(&client_char_component(), &server).unwrap();
        assert_eq!(chosen, CodeSetId::EUC_KR);
        // And the choice actually carries the text, which is the point.
        assert!(Converter::new(chosen).unwrap().encode("함정 전투체계").is_ok());
        assert!(
            Converter::new(CodeSetId::ISO_8859_1).unwrap().encode("함정 전투체계").is_err(),
            "the alternative would have destroyed it"
        );
    }

    // ── wide characters ─────────────────────────────────────────────────────

    fn wide(v: Version) -> WideCodec {
        WideCodec::new(v, CodeSetId::UTF_16).unwrap()
    }

    /// The length field means different things in 1.1 and 1.2, so the same
    /// string produces different bytes. Reading one with the other's rule
    /// returns a wrong string rather than an error, which is why both forms
    /// are pinned to exact bytes.
    #[test]
    fn wstring_length_means_elements_in_1_1_and_octets_in_1_2() {
        let mut e = Encoder::new(Endian::Big);
        wide(Version::V1_2).put_wstring(&mut e, "ab").unwrap();
        assert_eq!(
            e.finish().unwrap(),
            vec![0, 0, 0, 6, 0xFE, 0xFF, 0, b'a', 0, b'b'],
            "octets including the BOM, no terminator"
        );

        // 1.1 carries no mark and its units follow the message — both
        // measured against JacORB 3.9 (see `unmarked_order` and
        // tests/wide_1_1_from_a_peer.rs), neither what a 1.2 value does.
        for (endian, expected) in [
            (Endian::Big, [0, 0, 0, 3, 0, b'a', 0, b'b', 0, 0]),
            (Endian::Little, [3, 0, 0, 0, b'a', 0, b'b', 0, 0, 0]),
        ] {
            let mut e = Encoder::new(endian);
            wide(Version::V1_1).put_wstring(&mut e, "ab").unwrap();
            assert_eq!(
                e.finish().unwrap(),
                expected.to_vec(),
                "{endian:?}: elements including the terminator, no mark, units in the stream's order"
            );
        }
    }

    /// A peer that wrote the opposite byte order marks it, and the reader must
    /// act on that rather than keep the units as they lie. Getting this wrong
    /// yields plausible CJK text instead of an error: `w` becomes U+7700.
    #[test]
    fn reversed_bom_swaps_the_units() {
        // 0xFFFE then LE-ordered "ab", read from a big-endian stream.
        let raw = [0u8, 0, 0, 6, 0xFF, 0xFE, b'a', 0, b'b', 0];
        let mut d = Decoder::new(&raw, Endian::Big);
        assert_eq!(wide(Version::V1_2).get_wstring(&mut d).unwrap(), "ab");
    }

    /// Renamed from `absent_bom_is_read_in_stream_order`, which was true of
    /// what the code did and false of what §9.3.1.6 says — and which could
    /// never have told the difference, because its one case was a big-endian
    /// stream, where "stream order" and "big-endian" are the same answer.
    ///
    /// The little-endian half is the whole finding: omniORB 4.3.4 returns
    /// U+0077 for a BOM-less `00 77` in a little-endian stream, and reading it
    /// in the stream's order returns U+7700.
    #[test]
    fn absent_bom_means_big_endian_in_either_stream() {
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            e.put_u32(4);
            e.put_bytes(&[0, b'a', 0, b'b']);
            let raw = e.finish().unwrap();
            let mut d = Decoder::new(&raw, endian);
            assert_eq!(wide(Version::V1_2).get_wstring(&mut d).unwrap(), "ab", "{endian:?}");
        }
    }

    #[test]
    fn wstring_round_trips_in_both_versions() {
        for v in [Version::V1_1, Version::V1_2] {
            for endian in [Endian::Big, Endian::Little] {
                for s in ["", "ascii", "함정 전투체계", "mixed 한글 123"] {
                    if s.is_empty() && v.minor < 2 {
                        continue; // 1.1 cannot express a zero-length wstring
                    }
                    let mut e = Encoder::new(endian);
                    wide(v).put_wstring(&mut e, s).unwrap();
                    let raw = e.finish().unwrap();
                    let mut d = Decoder::new(&raw, endian);
                    assert_eq!(wide(v).get_wstring(&mut d).unwrap(), s, "{v} {endian:?} {s:?}");
                }
            }
        }
    }

    /// §9.3.2.7 makes a zero-length wstring legal in 1.2 and impossible in 1.1,
    /// where the count includes a terminator that must be there.
    #[test]
    fn empty_wstring_is_legal_in_1_2_only() {
        let mut e = Encoder::new(Endian::Big);
        wide(Version::V1_2).put_wstring(&mut e, "").unwrap();
        assert_eq!(e.finish().unwrap(), vec![0, 0, 0, 0], "no BOM: nothing to mark");

        let zero = [0u8, 0, 0, 0];
        let mut d = Decoder::new(&zero, Endian::Big);
        assert!(
            wide(Version::V1_1).get_wstring(&mut d).is_err(),
            "1.1 count includes a terminator"
        );
    }

    #[test]
    fn missing_terminator_is_rejected_in_1_1() {
        let raw = [0u8, 0, 0, 2, 0, b'a', 0, b'b']; // says 2 elements, no null
        let mut d = Decoder::new(&raw, Endian::Big);
        assert!(wide(Version::V1_1).get_wstring(&mut d).is_err());
    }

    #[test]
    fn odd_octet_count_is_rejected_in_1_2() {
        let raw = [0u8, 0, 0, 3, 0, b'a', 0];
        let mut d = Decoder::new(&raw, Endian::Big);
        assert!(wide(Version::V1_2).get_wstring(&mut d).is_err());
    }

    /// §9.3.1.6: wchar is illegal in GIOP 1.0, and a peer that sends it anyway
    /// must be met with MARSHAL rather than a guess.
    #[test]
    fn wchar_is_illegal_in_giop_1_0() {
        assert!(!WideCodec::is_legal(Version::V1_0));
        assert!(WideCodec::new(Version::V1_0, CodeSetId::UTF_16).is_err());
        assert!(WideCodec::is_legal(Version::V1_1));
    }

    #[test]
    fn wchar_is_length_prefixed_in_1_2_and_fixed_in_1_1() {
        let mut e = Encoder::new(Endian::Big);
        wide(Version::V1_2).put_wchar(&mut e, '한').unwrap();
        assert_eq!(e.finish().unwrap(), vec![2, 0xD5, 0x5C], "octet count then the unit");

        let mut e = Encoder::new(Endian::Big);
        wide(Version::V1_1).put_wchar(&mut e, '한').unwrap();
        assert_eq!(e.finish().unwrap(), vec![0xD5, 0x5C], "two fixed octets");

        for v in [Version::V1_1, Version::V1_2] {
            let mut e = Encoder::new(Endian::Big);
            wide(v).put_wchar(&mut e, '정').unwrap();
            let raw = e.finish().unwrap();
            let mut d = Decoder::new(&raw, Endian::Big);
            assert_eq!(wide(v).get_wchar(&mut d).unwrap(), '정');
        }
    }

    /// A character outside the BMP is a surrogate pair, which is two UTF-16
    /// units and therefore not one wchar. Emitting half of one would hand the
    /// peer a lone surrogate.
    #[test]
    fn astral_characters_are_refused_rather_than_split() {
        let mut e = Encoder::new(Endian::Big);
        match wide(Version::V1_2).put_wchar(&mut e, '🛰') {
            Err(NegotiationError::Untranslatable { text, .. }) => assert_eq!(text, "🛰"),
            other => panic!("expected Untranslatable, got {other:?}"),
        }
    }

    #[test]
    fn only_unicode_wchar_codesets_are_accepted() {
        assert!(WideCodec::new(Version::V1_2, CodeSetId::UTF_16).is_ok());
        assert!(WideCodec::new(Version::V1_2, CodeSetId::UCS_2).is_ok());
        assert!(WideCodec::new(Version::V1_2, CodeSetId::ISO_8859_1).is_err());
    }

    /// §9.3.1.6 gives UCS-2 a different rule from UTF-16 — "for GIOP 1.1, 1.2,
    /// and 1.3, UCS-2 and UCS-4 should be encoded using the endianess of the
    /// GIOP message" — and the BOM paragraph is scoped to UTF-16 by its own
    /// first clause. `WideCodec` accepted UCS-2 and then wrote it as UTF-16.
    ///
    /// The exact bytes are asserted, not just the round trip: a round trip is
    /// what let a hard-coded big-endian writer and a stream-order reader look
    /// correct for four phases.
    #[test]
    fn ucs2_follows_the_message_and_utf16_does_not() {
        let ucs2 = |v| WideCodec::new(v, CodeSetId::UCS_2).unwrap();
        for (endian, expected) in
            [(Endian::Big, [2, 0xD5, 0x5C]), (Endian::Little, [2, 0x5C, 0xD5])]
        {
            let mut e = Encoder::new(endian);
            ucs2(Version::V1_2).put_wchar(&mut e, '한').unwrap();
            assert_eq!(e.finish().unwrap(), expected.to_vec(), "UCS-2 wchar, {endian:?}");

            // UTF-16 stays big-endian in either stream — measured against
            // omniORB in tests/wide_chars_from_a_peer.rs.
            let mut e = Encoder::new(endian);
            wide(Version::V1_2).put_wchar(&mut e, '한').unwrap();
            assert_eq!(e.finish().unwrap(), vec![2, 0xD5, 0x5C], "UTF-16 wchar, {endian:?}");
        }

        // A UCS-2 `wstring` carries no mark and follows the message; a UTF-16
        // one carries a mark, which is what lets its units follow the message
        // too without the reader having to guess.
        for (endian, unit) in [(Endian::Big, [0xD5, 0x5C]), (Endian::Little, [0x5C, 0xD5])] {
            let mut e = Encoder::new(endian);
            ucs2(Version::V1_2).put_wstring(&mut e, "한").unwrap();
            let raw = e.finish().unwrap();
            assert_eq!(&raw[4..], &unit[..], "UCS-2 wstring, {endian:?}");
            let mut d = Decoder::new(&raw, endian);
            assert_eq!(ucs2(Version::V1_2).get_wstring(&mut d).unwrap(), "한");

            let mut e = Encoder::new(endian);
            wide(Version::V1_2).put_wstring(&mut e, "한").unwrap();
            let raw = e.finish().unwrap();
            assert_eq!(raw.len(), 8, "UTF-16 wstring carries a mark, {endian:?}");
            assert_eq!(&raw[6..], &unit[..], "UTF-16 units follow the mark, {endian:?}");
        }
    }

    #[test]
    fn codeset_ids_display_usefully() {
        assert_eq!(CodeSetId::ISO_8859_1.to_string(), "ISO-8859-1 (0x00010001)");
        assert_eq!(CodeSetId(0x1234).to_string(), "unregistered (0x00001234)");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Wide characters
// ─────────────────────────────────────────────────────────────────────────────

use crate::Version;

/// Byte-order mark, as it reads when the stream's order matches the writer's.
const BOM: u16 = 0xFEFF;

/// Which byte order a UTF-16 `wchar` or `wstring` body is in, and the body
/// with any byte-order mark removed.
///
/// §9.3.1.6 gives three bullets and they are about the *octets*, not about the
/// enclosing stream: `FE FF` is big-endian, `FF FE` is little-endian, and
/// **neither is big-endian**. A UTF-16 wide value therefore states its own
/// order, and the message's byte-order flag has no say in it. The one
/// exception the same section names is UCS-2, which "should be encoded using
/// the endianess of the GIOP message, for backward compatibility" through GIOP
/// 1.3 — so that is what `default` is when the codeset is not UTF-16.
///
/// Measured, omniORB 4.3.4, `cdrUnmarshal(_tc_wstring, …)` with the length
/// prefix in each order: `00 77` decoded to U+0077 and `77 00` to U+7700 in a
/// **little-endian** stream as well as a big-endian one. Reading the units in
/// the stream's order got the little-endian case exactly backwards, and our
/// own round trip could not see it because our writer always emits a BOM.
///
/// The spec also requires the mark itself be removed before the value reaches
/// the caller, which is why this returns the body rather than only the order.
///
/// That measurement is a GIOP 1.2 one, and the third bullet is not what the
/// only 1.1 wide-text peer on this host does — see [`unmarked_order`] for the
/// 1.1 rule and its provenance.
fn wide_order(raw: &[u8], version: Version, tcs: CodeSetId, stream: Endian) -> (Endian, &[u8]) {
    match raw.get(..2) {
        Some([0xFE, 0xFF]) => (Endian::Big, &raw[2..]),
        Some([0xFF, 0xFE]) => (Endian::Little, &raw[2..]),
        _ => (unmarked_order(version, tcs, stream), raw),
    }
}

/// The order a wide value is in when it carries no mark of its own.
///
/// **GIOP 1.2:** big-endian for UTF-16, which is §9.3.1.6's third bullet, and
/// measured against omniORB 4.3.4 in `tests/wide_chars_from_a_peer.rs`. The
/// message's own order for UCS-2, which is the carve-out two paragraphs later:
/// "for GIOP 1.1, 1.2, and 1.3, UCS-2 and UCS-4 should be encoded using the
/// endianess of the GIOP message, for backward compatibility" — and the BOM
/// paragraph above it opens with "if UTF-16 is selected as the TCS-W", so it
/// does not reach here.
///
/// **GIOP 1.1:** the message's own order for UTF-16 as well. The BOM paragraph
/// names no GIOP version, but its bullets are phrased "after the length
/// indication" and a 1.1 `wchar` has none — the section is written around the
/// 1.2 form — so whether its third bullet binds a 1.1 `wstring` is genuinely
/// ambiguous, and the decision is to follow the one 1.1 wide-text peer this
/// host can measure. JacORB 3.9 at GIOP 1.1 (`spikes/jacorb_giop11.sh`,
/// 2026-08-19, wchar=UTF-16) reads an unmarked `wstring` **in the message's
/// order**: given `00 77 00 69 …` in a little-endian message it echoed
/// `77 00 69 00 …` — U+7700 U+6900 — and given the same octets in a big-endian
/// message it echoed them unchanged. It writes only big-endian messages, so its
/// writer's convention in a little-endian message is unmeasurable, and it does
/// not read a mark at 1.1 at all (see [`WideCodec::put_wstring`]). omniORB
/// declines 1.1 wide text outright (`BAD_PARAM` minor 23, spike-interop case
/// 9), so there is no second witness. This also makes a 1.1 `wstring` agree
/// with a 1.1 `wchar`, which has always been written and read in the
/// message's order here.
///
/// Both the reader and the writer go through this, which is the point. The
/// writer used to hard-code big-endian for every codeset while the reader used
/// the stream, and the two only ever agreed because a mark was always present
/// to make them agree.
fn unmarked_order(version: Version, tcs: CodeSetId, stream: Endian) -> Endian {
    if tcs == CodeSetId::UTF_16 && version.minor >= 2 { Endian::Big } else { stream }
}

/// Whether a wide value in `tcs` is written with a leading byte-order mark.
/// Only UTF-16 defines one; a UCS-2 peer would render `U+FEFF` as a
/// zero-width no-break space in the middle of the text.
fn marks_its_order(tcs: CodeSetId) -> bool {
    tcs == CodeSetId::UTF_16
}

/// Assembles UTF-16 code units from octets already known to be in `order`.
fn wide_units(body: &[u8], order: Endian) -> Vec<u16> {
    body.chunks_exact(2)
        .map(|c| match order {
            Endian::Big => u16::from_be_bytes([c[0], c[1]]),
            Endian::Little => u16::from_le_bytes([c[0], c[1]]),
        })
        .collect()
}

/// Encodes and decodes `wchar` and `wstring`, whose wire form changes between
/// GIOP versions in ways that silently corrupt rather than fail.
///
/// | | `wstring` length means | terminator | `wchar` |
/// |---|---|---|---|
/// | 1.0 | — | — | **illegal** (§9.3.1.6) |
/// | 1.1 | wide characters, **including** a terminating null | yes | fixed 2 octets |
/// | 1.2 | **octets**, and zero is legal | no | octet count then octets |
///
/// Reading a 1.2 `wstring` with the 1.1 rule takes an octet count as a
/// character count and then looks for a terminator that is not there. Nothing
/// about that fails loudly; it just returns the wrong string.
#[derive(Debug, Clone, Copy)]
pub struct WideCodec {
    version: Version,
    tcs: CodeSetId,
}

impl WideCodec {
    /// A codec for a negotiated wchar transmission codeset.
    pub fn new(version: Version, tcs: CodeSetId) -> std::result::Result<Self, NegotiationError> {
        if version.minor == 0 {
            // §9.3.1.6 does not merely discourage this: a client meeting wchar
            // data in a GIOP 1.0 message must raise MARSHAL minor 6.
            return Err(NegotiationError::Unsupported(tcs));
        }
        match tcs {
            CodeSetId::UTF_16 | CodeSetId::UCS_2 => Ok(Self { version, tcs }),
            other => Err(NegotiationError::Unsupported(other)),
        }
    }

    /// The negotiated wchar codeset.
    pub fn codeset(self) -> CodeSetId {
        self.tcs
    }

    /// Whether `wchar` may appear at all under this version.
    pub fn is_legal(version: Version) -> bool {
        version.minor >= 1
    }

    fn units(self, s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    /// Writes a `wstring` — at GIOP 1.2 prefixed with a byte-order mark, at
    /// GIOP 1.1 unmarked and in the message's order.
    ///
    /// The 1.2 BOM is not decoration. Both omniORB and JacORB misread a
    /// big-endian UTF-16 `wstring` sent without one — `w` (U+0077) came back
    /// as U+7700 — so peers do **not** infer wide-character order from the
    /// enclosing message's byte order, whatever a reading of §9.3.2.7 might
    /// suggest. Writing an explicit BOM removes the ambiguity instead of
    /// betting on which convention the peer chose, and omniORB emits one
    /// itself.
    ///
    /// What that observation was actually seeing is now measured and named in
    /// [`wide_order`]: a BOM-less UTF-16 value is big-endian by definition, in
    /// either kind of stream. At 1.2 the BOM stays because it is what omniORB
    /// writes and what every reader in the field understands, not because the
    /// reader here needs it any more.
    ///
    /// **At 1.1 no mark is written.** §9.3.1.6 (CORBA 3.4 Part 2) makes the
    /// mark the writer's option, not its duty — "if an ORB decides to use BOM
    /// to indicate endianness, it shall add the BOM …" — and its third bullet
    /// makes the unmarked form well-defined: "if the first two bytes (after
    /// the length indication) are neither, it's big-endian". The paragraph is
    /// not scoped to a GIOP version, so a mark at 1.1 is *permitted*; what is
    /// measured is that the one 1.1 wide-text peer this host has does not
    /// read it as one. JacORB 3.9 at GIOP 1.1 (`spikes/jacorb_giop11.sh`,
    /// 2026-08-19, negotiated char=UTF-8 wchar=UTF-16) writes `count=13`
    /// unmarked big-endian for a twelve-unit text and, given our marked
    /// `count=14`, hands its user `U+FEFF` + text — and echoes the mark back
    /// as the fourteenth unit, which our reader then stripped as a mark, so
    /// the round trip was green while the peer's user saw the wrong value.
    /// The same peer strips the mark at 1.2. omniORBpy cannot unmarshal its
    /// own 1.1 `wchar` (D010 B5) and omniORB declines 1.1 wide text with
    /// `BAD_PARAM` minor 23, so JacORB is the only witness, and the form it
    /// reads correctly is the one the specification defines without the
    /// option — unmarked, counted in wide characters plus the terminator
    /// (§9.3.2.7). Recorded in `tests/wide_1_1_from_a_peer.rs`.
    ///
    /// The unmarked units follow [`unmarked_order`] — the same rule the 1.2
    /// `wchar` writer already applies. At 1.2 that is big-endian whatever the
    /// stream; at 1.1 it is the message's order, which is where the same peer
    /// measurement parted from §9.3.1.6's third bullet: our first unmarked
    /// attempt wrote big-endian units into a little-endian message and JacORB
    /// echoed every unit swapped. A UCS-2 value never carried a mark and keeps
    /// following the message in every version.
    pub fn put_wstring(
        self,
        e: &mut Encoder,
        s: &str,
    ) -> std::result::Result<(), NegotiationError> {
        let units = self.units(s);
        // UCS-2 has no mark defined for it; see `marks_its_order`. A 1.1
        // value is unmarked whatever its codeset; see the doc comment.
        let mark = marks_its_order(self.tcs) && self.version.minor >= 2;
        let total = units.len() + usize::from(mark);
        if self.version.minor >= 2 {
            // Octet count, no terminator. Zero length stays legal and carries
            // no BOM, since there is nothing whose order needs marking.
            if units.is_empty() {
                e.put_u32(0);
                return Ok(());
            }
            e.put_u32((total * 2) as u32);
        } else {
            // Element count including the terminating null.
            e.put_u32(total as u32 + 1);
        }
        if mark {
            // A marked value is written in the stream's order and the mark,
            // written the same way, says which order that was.
            e.put_u16(BOM);
            for u in units {
                e.put_u16(u);
            }
        } else {
            // An unmarked value is in the only order the reader may assume.
            let order = unmarked_order(self.version, self.tcs, e.endian());
            for u in units {
                e.put_bytes(&match order {
                    Endian::Big => u.to_be_bytes(),
                    Endian::Little => u.to_le_bytes(),
                });
            }
        }
        if self.version.minor < 2 {
            e.put_u16(0);
        }
        Ok(())
    }

    /// Reads a `wstring`.
    ///
    /// The units are ordered by [`wide_order`] — the value's own BOM, or
    /// [`unmarked_order`] when it carries none: big-endian at 1.2, and **not**
    /// the enclosing stream's order; the stream's order at 1.1.
    /// The elements are read as octets, which is also what Table 9.1's `wchar`
    /// alignment of 1 for GIOP 1.2 asks for; the count is a 4-aligned
    /// `unsigned long`, so a 1.1 element still lands 2-aligned as it must.
    ///
    /// **A leading mark is removed at 1.1 as well as at 1.2.** This is a
    /// decision, not an accident of sharing [`wide_order`]: §9.3.1.6's "if a
    /// BOM is present at the beginning of a wchar or wstring received in a
    /// GIOP message, the ORB shall remove the BOM before passing the value to
    /// the user" names no GIOP version, so a 1.1 peer that marks — as we did
    /// until this was measured, and as the sentence permits — is read as it
    /// meant to be, in the order it marked. What that accepts: a 1.1 peer
    /// that does not mark and whose user's text genuinely begins with U+FEFF
    /// (JacORB 3.9 is one) loses that first character here, exactly as it
    /// would at any 1.2 reader. What it prevents: handing our user a U+FEFF
    /// from a marking peer, and reading a little-endian-marked value
    /// backwards — the former being the very defect this writer no longer
    /// commits against JacORB. JacORB's echo of our unmarked 13-unit request
    /// is 13 unmarked units and decodes to the original text either way; the
    /// spec sentence is what breaks the tie.
    pub fn get_wstring(self, d: &mut Decoder<'_>) -> std::result::Result<String, NegotiationError> {
        let bad = || NegotiationError::Malformed { codeset: self.tcs };
        let len = d.get_u32().map_err(|_| bad())?;
        let octets = if self.version.minor >= 2 {
            if len % 2 != 0 {
                return Err(bad());
            }
            len as usize
        } else {
            if len == 0 {
                // 1.1 counts the terminator, so zero cannot occur.
                return Err(bad());
            }
            // 1.1 counts wide characters, not octets.
            (len as usize).checked_mul(2).ok_or_else(bad)?
        };
        let raw = d.get_bytes(octets).map_err(|_| bad())?;
        let (order, body) = wide_order(raw, self.version, self.tcs, d.endian());
        let mut units = wide_units(body, order);
        if self.version.minor < 2 {
            match units.pop() {
                Some(0) => {}
                _ => return Err(bad()),
            }
        }
        String::from_utf16(&units).map_err(|_| bad())
    }

    /// Writes a `wchar`.
    ///
    /// Characters outside the Basic Multilingual Plane need a surrogate pair,
    /// which is two UTF-16 units and therefore not one `wchar`. Refusing is
    /// correct; emitting half a pair would hand the peer a lone surrogate.
    ///
    /// The 1.2 unit is big-endian with no mark, and that is **measured**, not
    /// assumed: omniORB 4.3.4 writes `02 00 41` for U+0041 and `02 d5 5c` for
    /// U+D55C in a little-endian stream and a big-endian one alike, which is
    /// §9.3.1.6's "defaults to big endian" showing up on a wire. It is also
    /// unaligned in both — `WCharSeq` of two elements came back as
    /// `… 02 00 77 02 00 41`, one abutting the next, which is Table 9.1's
    /// `wchar` alignment of 1 for GIOP 1.2.
    ///
    /// "Big-endian" is [`unmarked_order`]'s answer for UTF-16, not a constant:
    /// a `wchar` carries no mark, so the order the reader will assume is the
    /// only order it may be written in, and for UCS-2 that order is the
    /// message's. Hard-coding big-endian here while the reader used the stream
    /// is what made the two disagree for UCS-2 and agree for UTF-16 by luck.
    pub fn put_wchar(self, e: &mut Encoder, c: char) -> std::result::Result<(), NegotiationError> {
        let mut buf = [0u16; 2];
        let units = c.encode_utf16(&mut buf);
        if units.len() != 1 {
            return Err(NegotiationError::Untranslatable {
                codeset: self.tcs,
                text: c.to_string(),
            });
        }
        if self.version.minor >= 2 {
            let unit = units[0];
            let octets = match unmarked_order(self.version, self.tcs, e.endian()) {
                Endian::Big => unit.to_be_bytes(),
                Endian::Little => unit.to_le_bytes(),
            };
            e.put_u8(2); // octet count
            e.put_bytes(&octets);
        } else {
            e.put_u16(units[0]);
        }
        Ok(())
    }

    /// Reads a `wchar`.
    ///
    /// A GIOP 1.2 `wchar` is an octet count and then that many octets, and
    /// §9.3.1.6 lets a peer spend two of them on a byte-order mark: "if a BOM
    /// is present at the beginning of a wchar or wstring received in a GIOP
    /// message, the ORB **shall** remove the BOM before passing the value to
    /// the user". Insisting on a count of exactly two refused a legal encoding
    /// we happen not to emit, which is why nothing here ever went red over it.
    ///
    /// GIOP 1.1 keeps the stream's order. It has no length indication, so it
    /// has nowhere to put a mark, and §9.3.1.6 phrases the byte-order bullets
    /// in terms of "the first two bytes *after the length indication*". This
    /// one is **unmeasured**: omniORBpy 4.3.4 marshals a bare `wchar` but
    /// raises `MARSHAL_MessageTooLong` unmarshalling its own output, so no peer
    /// on this host can be asked. It is left as it was rather than changed on a
    /// reading.
    pub fn get_wchar(self, d: &mut Decoder<'_>) -> std::result::Result<char, NegotiationError> {
        let bad = || NegotiationError::Malformed { codeset: self.tcs };
        let unit = if self.version.minor >= 2 {
            let n = d.get_u8().map_err(|_| bad())?;
            let raw = d.get_bytes(n as usize).map_err(|_| bad())?;
            let (order, body) = wide_order(raw, self.version, self.tcs, d.endian());
            // An odd count is refused rather than truncated: `chunks_exact`
            // would drop the trailing octet and hand back a plausible
            // character, having consumed a byte nothing accounts for.
            if body.len() != 2 {
                return Err(bad());
            }
            match wide_units(body, order)[..] {
                [unit] => unit,
                _ => return Err(bad()),
            }
        } else {
            d.get_u16().map_err(|_| bad())?
        };
        char::from_u32(unit as u32).ok_or_else(bad)
    }
}

/// The negotiated `char` codeset, as the CDR stream's own [`TextCodec`].
///
/// D009 batch 2. Before this, a `Converter` existed and exactly one caller in
/// the workspace used it — `spike_interop.rs` — which is why the codeset path
/// always measured green: the one binary exercising it was the one binary
/// honouring it. A stream now carries the agreement, so `put_str`/`get_string`
/// honour it without every caller remembering to.
///
/// The framing stays in `orbweaver-cdr`: this converts octets, and the length
/// prefix, the trailing NUL and the embedded-NUL refusal remain the stream's.
///
/// 협상 결과를 스트림이 지니므로, 모든 호출자가 기억하지 않아도 지켜진다.
/// 프레이밍은 여전히 스트림의 몫이다.
impl Converter {
    /// The octets for `s`, as `orbweaver-cdr` wants them.
    ///
    /// Inherent rather than a `TextCodec` impl: this type knows narrow text
    /// only, and the trait covers both halves. [`Codecs`] is the one
    /// implementor, so there is one place a stream's text decisions are made.
    pub fn encode_narrow(&self, s: &str) -> orbweaver_cdr::Result<Vec<u8>> {
        (*self).encode(s).map_err(|e| match e {
            // Not a substitution: mojibake in a database outlives the call
            // that made it, and this is the one place that decision is taken.
            NegotiationError::Unsupported(id) => orbweaver_cdr::Error::Malformed(match id {
                CodeSetId::ASCII => "text has a character the negotiated ASCII cannot carry",
                CodeSetId::ISO_8859_1 => {
                    "text has a character the negotiated ISO-8859-1 cannot carry"
                }
                _ => "text has a character the negotiated codeset cannot carry",
            }),
            _ => orbweaver_cdr::Error::Malformed("the negotiated codeset could not encode this"),
        })
    }

    /// The text `bytes` carry in this codeset.
    pub fn decode_narrow(&self, bytes: &[u8]) -> orbweaver_cdr::Result<String> {
        (*self).decode(bytes).map_err(|_| {
            orbweaver_cdr::Error::Malformed("octets are not valid in the negotiated codeset")
        })
    }
}

/// Both halves of a connection's text agreement, held together and failing
/// apart.
///
/// TCS-C and TCS-W are separate fields of `CodeSetComponentInfo` and negotiate
/// independently: a peer may agree on `char` and not on `wchar`. A single
/// `Option` over both would take working narrow conversion away for a wide
/// disagreement, which is why this holds two — D009 §7.2 asked the question
/// and this is the answer.
///
/// 두 협상은 독립적이므로 함께 지되 따로 실패한다. 하나의 `Option`으로 묶으면
/// wide 불일치가 멀쩡한 narrow 변환까지 빼앗는다.
#[derive(Debug, Clone, Copy)]
pub struct Codecs {
    narrow: Option<Converter>,
    wide: Option<WideCodec>,
}

impl Codecs {
    /// The agreement for a connection at `version`.
    pub fn new(narrow: Option<Converter>, wide: Option<WideCodec>) -> Self {
        Codecs { narrow, wide }
    }

    /// The 1.2 UTF-16 form an **encapsulation** always uses, whatever the
    /// message's version says (§9.3.1.6), with no narrow conversion.
    ///
    /// This is a rule, not a default: it is the one place a fixed answer is
    /// correct, and naming it that way is what keeps it from being copied to
    /// somewhere a connection's version actually matters.
    pub fn encapsulated() -> Self {
        Codecs { narrow: None, wide: WideCodec::new(Version::V1_2, CodeSetId::UTF_16).ok() }
    }
}

impl orbweaver_cdr::TextCodec for Codecs {
    fn encode_narrow(&self, s: &str) -> orbweaver_cdr::Result<Vec<u8>> {
        match self.narrow {
            Some(c) => c.encode_narrow(s),
            None => Ok(s.as_bytes().to_vec()),
        }
    }

    fn decode_narrow(&self, bytes: &[u8]) -> orbweaver_cdr::Result<String> {
        match self.narrow {
            Some(c) => c.decode_narrow(bytes),
            None => std::str::from_utf8(bytes)
                .map(str::to_owned)
                .map_err(|_| orbweaver_cdr::Error::BadUtf8),
        }
    }

    fn put_wide(&self, e: &mut orbweaver_cdr::Encoder, s: &str) -> orbweaver_cdr::Result<()> {
        let w = self.wide.ok_or(orbweaver_cdr::Error::Malformed(
            "this connection agreed on no wchar codeset; a wstring cannot cross it",
        ))?;
        w.put_wstring(e, s).map_err(|_| {
            orbweaver_cdr::Error::Malformed("text the agreed wchar codeset cannot carry")
        })
    }

    fn get_wide(&self, d: &mut orbweaver_cdr::Decoder<'_>) -> orbweaver_cdr::Result<String> {
        let w = self.wide.ok_or(orbweaver_cdr::Error::Malformed(
            "this connection agreed on no wchar codeset; a wstring cannot cross it",
        ))?;
        w.get_wstring(d).map_err(|_| {
            orbweaver_cdr::Error::Malformed("malformed wstring for the agreed codeset")
        })
    }

    fn put_wide_char(&self, e: &mut orbweaver_cdr::Encoder, c: char) -> orbweaver_cdr::Result<()> {
        let w = self
            .wide
            .ok_or(orbweaver_cdr::Error::Malformed("this connection agreed on no wchar codeset"))?;
        w.put_wchar(e, c)
            .map_err(|_| orbweaver_cdr::Error::Malformed("wchar outside the agreed codeset"))
    }

    fn get_wide_char(&self, d: &mut orbweaver_cdr::Decoder<'_>) -> orbweaver_cdr::Result<char> {
        let w = self
            .wide
            .ok_or(orbweaver_cdr::Error::Malformed("this connection agreed on no wchar codeset"))?;
        w.get_wchar(d).map_err(|_| orbweaver_cdr::Error::Malformed("malformed wchar"))
    }
}
