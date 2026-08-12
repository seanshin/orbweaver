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
    /// EUC-KR. Conversion is not implemented; see `docs/decisions/D001`.
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

    /// Whether this crate can convert to and from it today.
    pub fn is_supported(self) -> bool {
        matches!(
            self,
            CodeSetId::ISO_8859_1 | CodeSetId::UTF_8 | CodeSetId::ASCII | CodeSetId::UTF_16
        )
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
        }
    }
}

/// What this implementation offers for `char` data.
pub fn client_char_component() -> CodeSetComponent {
    CodeSetComponent {
        native: Some(CodeSetId::UTF_8),
        conversions: vec![CodeSetId::ISO_8859_1, CodeSetId::ASCII],
    }
}

/// What this implementation offers for `wchar` data.
pub fn client_wchar_component() -> CodeSetComponent {
    CodeSetComponent { native: Some(CodeSetId::UTF_16), conversions: vec![] }
}

/// Chooses a transmission codeset, following §7.10.2.6.
///
/// The order matters: preferring the server's native set when we can convert
/// to it avoids forcing conversion work onto the peer, which is what the spec
/// intends by listing that case before the reverse.
pub fn negotiate(
    client: &CodeSetComponent,
    server: &CodeSetComponent,
) -> std::result::Result<CodeSetId, NegotiationError> {
    let client_native = client.native.ok_or(NegotiationError::NoWcharCodeSet)?;

    // 1. Identical native sets: transmit as-is, no conversion by anyone.
    if server.native == Some(client_native) {
        return check_supported(client_native);
    }
    // 2. We can convert to the server's native set.
    if let Some(sn) = server.native
        && client.conversions.contains(&sn)
    {
        return check_supported(sn);
    }
    // 3. The server can convert to ours.
    if server.conversions.contains(&client_native) {
        return check_supported(client_native);
    }
    // 4. Some conversion set in common. Resolved by the *client's* preference
    //    order rather than by numeric id: both are deterministic, but list
    //    order carries intent — we list ISO-8859-1 before ASCII because it is
    //    the superset — whereas the lowest registry number is an accident of
    //    how OSF assigned them.
    if let Some(common) = client
        .conversions
        .iter()
        .find(|c| server.conversions.contains(c))
    {
        return check_supported(*common);
    }
    // 5. §7.10.2.6 allows falling back to a universal set when both sides can
    //    reach it. UTF-8 is the char fallback.
    if client.supports(CodeSetId::UTF_8) && server.supports(CodeSetId::UTF_8) {
        return check_supported(CodeSetId::UTF_8);
    }

    Err(NegotiationError::Incompatible {
        client_native,
        server_native: server.native,
    })
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

    /// The case that matters in practice: omniORB is natively ISO-8859-1 and we
    /// are natively UTF-8, so we must convert rather than assume.
    #[test]
    fn converts_to_the_peers_native_when_we_can() {
        let server = CodeSetComponent {
            native: Some(CodeSetId::ISO_8859_1),
            conversions: vec![CodeSetId::UTF_8],
        };
        let chosen = negotiate(&client_char_component(), &server).unwrap();
        assert_eq!(
            chosen,
            CodeSetId::ISO_8859_1,
            "the peer's native set is preferred so conversion cost stays with us"
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

    /// A common conversion set is resolved by *our* preference order, not by
    /// the peer's and not by registry number. Order carries intent: ISO-8859-1
    /// is listed before ASCII because it is the superset, so it must win even
    /// though the peer lists ASCII first.
    #[test]
    fn common_conversion_set_follows_our_preference_order() {
        let client = CodeSetComponent {
            native: Some(CodeSetId(0x0AAA_0000)),
            conversions: vec![CodeSetId::UTF_8, CodeSetId::ISO_8859_1, CodeSetId::ASCII],
        };
        let server = CodeSetComponent {
            native: Some(CodeSetId(0x0BBB_0000)),
            conversions: vec![CodeSetId::ASCII, CodeSetId::ISO_8859_1],
        };
        let a = negotiate(&client, &server).unwrap();
        assert_eq!(a, negotiate(&client, &server).unwrap(), "must be deterministic");
        assert_eq!(a, CodeSetId::ISO_8859_1, "our order decides, not the peer's");
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

    /// A peer asking for EUC-KR must produce a distinct, actionable error —
    /// not `Incompatible`, because the peers agree and the gap is ours.
    #[test]
    fn euc_kr_is_refused_as_unsupported_not_incompatible() {
        let client = CodeSetComponent {
            native: Some(CodeSetId::EUC_KR),
            conversions: vec![],
        };
        let server = client.clone();
        match negotiate(&client, &server) {
            Err(NegotiationError::Unsupported(id)) => assert_eq!(id, CodeSetId::EUC_KR),
            other => panic!("expected Unsupported(EUC-KR), got {other:?}"),
        }
        assert!(matches!(
            Converter::new(CodeSetId::EUC_KR),
            Err(NegotiationError::Unsupported(_))
        ));
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

    #[test]
    fn codeset_ids_display_usefully() {
        assert_eq!(CodeSetId::ISO_8859_1.to_string(), "ISO-8859-1 (0x00010001)");
        assert_eq!(CodeSetId(0x1234).to_string(), "unregistered (0x00001234)");
    }
}
