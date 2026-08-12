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
        CodeSetId::UTF_8 => 5,   // all of Unicode, and compact for ASCII
        CodeSetId::UTF_16 => 4,  // all of Unicode
        CodeSetId::EUC_KR => 3,  // Korean plus ASCII
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
    candidates.extend(
        client
            .conversions
            .iter()
            .filter(|c| server.conversions.contains(c))
            .copied(),
    );

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

    Err(NegotiationError::Incompatible {
        client_native,
        server_native: server.native,
    })
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
        if id.is_supported() { Ok(Converter { id }) } else { Err(NegotiationError::Unsupported(id)) }
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
            CodeSetId::UTF_8 => {
                String::from_utf8(bytes.to_vec()).map_err(|_| NegotiationError::Unsupported(self.id))
            }
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
                let units: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
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
        let server = CodeSetComponent {
            native: Some(CodeSetId::ISO_8859_1),
            conversions: vec![],
        };
        assert_eq!(
            negotiate(&client_char_component(), &server).unwrap(),
            CodeSetId::ISO_8859_1
        );
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
        let client = CodeSetComponent {
            native: Some(CodeSetId(0x0AAA_0000)),
            conversions: vec![],
        };
        let server = CodeSetComponent {
            native: Some(CodeSetId(0x0BBB_0000)),
            conversions: vec![],
        };
        assert!(matches!(
            negotiate(&client, &server),
            Err(NegotiationError::Incompatible { .. })
        ));
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
        let expected = [
            0xc7, 0xd4, 0xc1, 0xa4, 0x20, 0xc0, 0xfc, 0xc5, 0xf5, 0xc3, 0xbc, 0xb0, 0xe8,
        ];
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

    #[test]
    fn codeset_ids_display_usefully() {
        assert_eq!(CodeSetId::ISO_8859_1.to_string(), "ISO-8859-1 (0x00010001)");
        assert_eq!(CodeSetId(0x1234).to_string(), "unregistered (0x00001234)");
    }
}
