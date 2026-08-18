//! CSIv2: what a target accepts, and who a call is made on behalf of.
//!
//! `docs/PLAN.md` §4.8 states the problem this exists for. A bridge
//! authenticates to a legacy target with **its own** credentials, so the target
//! sees `orbweaver` on every call whoever asked. Every audit entry names the
//! same principal and every authorization decision is made about the wrong
//! subject. That is the confused deputy, and an AI bridge — trusted,
//! long-lived, reachable by many callers — is an unusually attractive one.
//!
//! Three separate things travel, and conflating them is the usual mistake:
//!
//! | Layer | Question | Mechanism |
//! | --- | --- | --- |
//! | Transport identity | which process is connected? | mTLS / SSLIOP |
//! | Caller identity | on whose behalf? | CSIv2 SAS identity token |
//! | Authorization attributes | allowed to do what? | scopes, against `@ai_authz` |
//!
//! This module is the second row and the part of the first that appears in an
//! IOR. Spec: CORBA 3.4 Part 2 §10 (`CSI`, `CSIIOP`), and RFC 2743 §3.1 for the
//! token framing GSSUP arrives in.
//!
//! # Credential hygiene is structural here
//!
//! §4.8 requires that credentials are never logged and are excluded from
//! diagnostics *by construction* rather than by remembering to redact.
//! [`GssUpToken`] therefore implements `Debug` by hand and prints no password —
//! there is no way to obtain one through a formatter, so no future `{:?}` in a
//! log line can leak it. A test asserts it, because the property is only worth
//! anything if it cannot regress.
//!
//! # What this module does not claim
//!
//! It encodes and decodes. It has **not** been exercised against a peer that
//! enforces CSIv2: neither project fixture advertises a mechanism, which §4.8
//! anticipates as the common case ("many legacy targets have no authentication
//! at all"). Per-peer interop is a claim to be made per peer, never a feature
//! this crate has.

use orbweaver_cdr::{Decoder, Encoder, Endian};

use crate::{Error, Result, TaggedComponent};

/// `IOP::ServiceId` for `SecurityAttributeService`.
pub const SERVICE_ID_SAS: u32 = 15;

/// `IOP::ComponentId` for the mechanism list a target advertises.
pub const TAG_CSI_SEC_MECH_LIST: u32 = 33;

/// `IOP::ComponentId` for a null transport mechanism.
pub const TAG_NULL_TAG: u32 = 0;

/// `CSI::AssociationOptions` — what a mechanism supports or requires.
///
/// A bitmask rather than an enum because a target advertises several at once,
/// and the pair (`supports`, `requires`) is what decides whether a client can
/// talk to it at all.
pub mod options {
    /// The association may be unprotected.
    pub const NO_PROTECTION: u16 = 1;
    /// Message integrity.
    pub const INTEGRITY: u16 = 2;
    /// Message confidentiality.
    pub const CONFIDENTIALITY: u16 = 4;
    /// Replay detection.
    pub const DETECT_REPLAY: u16 = 8;
    /// Misordering detection.
    pub const DETECT_MISORDERING: u16 = 16;
    /// The client will authenticate the target.
    pub const ESTABLISH_TRUST_IN_TARGET: u16 = 32;
    /// The target will authenticate the client.
    pub const ESTABLISH_TRUST_IN_CLIENT: u16 = 64;
    /// The target will not accept delegated credentials.
    pub const NO_DELEGATION: u16 = 128;
    /// Simple delegation.
    pub const SIMPLE_DELEGATION: u16 = 256;
    /// Composite delegation.
    pub const COMPOSITE_DELEGATION: u16 = 512;
    /// The target accepts an asserted identity — the option that makes a bridge
    /// able to stop being a confused deputy.
    pub const IDENTITY_ASSERTION: u16 = 1024;
    /// Delegation performed by the client.
    pub const DELEGATION_BY_CLIENT: u16 = 2048;
}

/// The OID for GSSUP, `2.23.130.1.1.1`, DER-encoded.
///
/// `2.23.130.1.1.1`: the first two arcs pack into one byte (40 × 2 + 23 = 103),
/// 130 needs two base-128 bytes, and the rest are one each.
pub const GSSUP_OID: &[u8] = &[0x06, 0x06, 0x67, 0x81, 0x02, 0x01, 0x01, 0x01];

/// `CSI::IdentityTokenType`.
///
/// `Absent` is the default, and that is the safe direction: a value that
/// defaulted to a claim would let a forgotten field assert one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum IdentityToken {
    /// `ITTAbsent` — no identity is being asserted.
    ///
    /// Distinct from anonymous: absent says nothing was claimed, anonymous
    /// claims a caller who declined to name themselves. A target may treat them
    /// very differently.
    #[default]
    Absent,
    /// `ITTAnonymous`.
    Anonymous,
    /// `ITTPrincipalName`, a GSS exported name.
    PrincipalName(Vec<u8>),
    /// `ITTX509CertChain`, DER.
    X509CertChain(Vec<u8>),
    /// `ITTDistinguishedName`, DER.
    DistinguishedName(Vec<u8>),
}

impl IdentityToken {
    /// The `CSI::IdentityTokenType` discriminator.
    pub fn token_type(&self) -> u32 {
        match self {
            IdentityToken::Absent => 0,
            IdentityToken::Anonymous => 1,
            IdentityToken::PrincipalName(_) => 2,
            IdentityToken::X509CertChain(_) => 4,
            IdentityToken::DistinguishedName(_) => 8,
        }
    }

    /// A name for an audit entry.
    ///
    /// §4.8: the log records *which* principal was asserted, never the material
    /// that asserted it. A certificate chain is material, so it is described
    /// rather than reproduced.
    pub fn audit_name(&self) -> String {
        match self {
            IdentityToken::Absent => "<none asserted>".into(),
            IdentityToken::Anonymous => "<anonymous>".into(),
            IdentityToken::PrincipalName(n) => String::from_utf8_lossy(n).into_owned(),
            IdentityToken::X509CertChain(c) => format!("<x509 chain, {} bytes>", c.len()),
            IdentityToken::DistinguishedName(d) => format!("<dn, {} bytes>", d.len()),
        }
    }

    fn encode(&self, e: &mut Encoder) {
        e.put_u32(self.token_type());
        match self {
            // A union arm with a boolean member: the value is unused and the
            // arm's presence is the information.
            IdentityToken::Absent | IdentityToken::Anonymous => e.put_bool(true),
            IdentityToken::PrincipalName(b)
            | IdentityToken::X509CertChain(b)
            | IdentityToken::DistinguishedName(b) => e.put_octet_seq(b),
        }
    }

    fn decode(d: &mut Decoder<'_>) -> Result<Self> {
        let kind = d.get_u32()?;
        Ok(match kind {
            0 => {
                d.get_bool()?;
                IdentityToken::Absent
            }
            1 => {
                d.get_bool()?;
                IdentityToken::Anonymous
            }
            2 => IdentityToken::PrincipalName(d.get_octet_seq()?.to_vec()),
            4 => IdentityToken::X509CertChain(d.get_octet_seq()?.to_vec()),
            8 => IdentityToken::DistinguishedName(d.get_octet_seq()?.to_vec()),
            // A type we do not know is not one to guess at: the arm's payload
            // shape depends on it, so reading on would produce a principal name
            // out of whatever bytes followed.
            _ => return Err(Error::BadIor("unknown CSI identity token type")),
        })
    }
}

/// A GSSUP username/password token, in the framing RFC 2743 §3.1 defines.
///
/// `Debug` is written by hand and prints no password. That is the whole point:
/// §4.8 requires credentials to be excluded from diagnostics *by construction*,
/// and a redaction that depends on nobody writing `{:?}` in a log line is not
/// a control.
#[derive(Clone, PartialEq, Eq)]
pub struct GssUpToken {
    /// The principal being authenticated.
    pub username: Vec<u8>,
    /// Their password. Never printed, never logged.
    pub password: Vec<u8>,
    /// The GSS exported name of the target this is for.
    ///
    /// Not decoration: a password captured by one target and replayed at
    /// another is stopped by the target name being part of what was sent.
    pub target_name: Vec<u8>,
}

impl std::fmt::Debug for GssUpToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GssUpToken")
            .field("username", &String::from_utf8_lossy(&self.username))
            .field("password", &"<redacted>")
            .field("target_name", &String::from_utf8_lossy(&self.target_name))
            .finish()
    }
}

impl GssUpToken {
    /// Encodes the mechanism-independent token: `0x60`, a DER length, the OID,
    /// then the CDR-encapsulated `GSSUP::InitialContextToken`.
    pub fn encode(&self, endian: Endian) -> Result<Vec<u8>> {
        let mut inner = Encoder::encapsulation(endian);
        inner.put_octet_seq(&self.username);
        inner.put_octet_seq(&self.password);
        inner.put_octet_seq(&self.target_name);
        let inner = inner.finish().map_err(Error::Cdr)?;

        let body_len = GSSUP_OID.len() + inner.len();
        let mut out = vec![0x60];
        out.extend_from_slice(&der_length(body_len));
        out.extend_from_slice(GSSUP_OID);
        out.extend_from_slice(&inner);
        Ok(out)
    }

    /// Decodes one, rejecting anything that is not GSSUP.
    ///
    /// A token whose OID names another mechanism is not a token to guess at:
    /// its body has a different shape, and reading it as GSSUP would produce a
    /// username from whatever bytes happened to be there.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let mut at = 0usize;
        if bytes.first() != Some(&0x60) {
            return Err(Error::BadIor("not a GSS initial context token"));
        }
        at += 1;
        let (len, used) = parse_der_length(&bytes[at..])?;
        at += used;
        // `at + len` is arithmetic on a length the *peer* chose. `60 88 FF FF
        // FF FF FF FF FF FF` declares `usize::MAX`, and the sum panicked in a
        // debug build while wrapping to a bogus "truncated" in release — two
        // behaviours, the quieter one being the one that ships, and the reason
        // a release-mode fuzzer could not see this at all. The length is
        // refused as a length, before it is added to anything.
        let end = at
            .checked_add(len)
            .ok_or(Error::BadIor("GSS token declares a length no buffer can hold"))?;
        let body = bytes.get(at..end).ok_or(Error::BadIor("GSS token is truncated"))?;
        if !body.starts_with(GSSUP_OID) {
            return Err(Error::BadIor("GSS token does not carry the GSSUP mechanism"));
        }
        let mut d = Decoder::encapsulation(&body[GSSUP_OID.len()..])?;
        Ok(GssUpToken {
            username: d.get_octet_seq()?.to_vec(),
            password: d.get_octet_seq()?.to_vec(),
            target_name: d.get_octet_seq()?.to_vec(),
        })
    }
}

/// `CSI::EstablishContext`, the message a client puts in the SAS context.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EstablishContext {
    /// Correlates a stateful context; 0 for stateless.
    pub client_context_id: u64,
    /// Authorization elements, encoded as they arrived.
    pub authorization_token: Vec<(u32, Vec<u8>)>,
    /// Who the call is on behalf of.
    pub identity_token: IdentityToken,
    /// How the *bridge* authenticated itself.
    pub client_authentication_token: Vec<u8>,
}

/// The `CSI::SASContextBody` union arms this implementation handles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SasContextBody {
    /// `MTEstablishContext` (0).
    Establish(EstablishContext),
    /// `MTCompleteEstablishContext` (1).
    Complete {
        /// Echoed context id.
        client_context_id: u64,
        /// Whether the target kept state.
        stateful: bool,
        /// Final GSS token, usually empty.
        final_context_token: Vec<u8>,
    },
    /// `MTContextError` (4).
    Error {
        /// Echoed context id.
        client_context_id: u64,
        /// GSS major status.
        major_status: i32,
        /// GSS minor status.
        minor_status: i32,
        /// Mechanism-specific detail.
        error_token: Vec<u8>,
    },
}

impl SasContextBody {
    /// Encodes the body as the encapsulation a service context carries.
    pub fn encode(&self, endian: Endian) -> Result<Vec<u8>> {
        let mut e = Encoder::encapsulation(endian);
        match self {
            SasContextBody::Establish(c) => {
                e.put_u32(0);
                e.put_u64(c.client_context_id);
                e.put_u32(c.authorization_token.len() as u32);
                for (kind, data) in &c.authorization_token {
                    e.put_u32(*kind);
                    e.put_octet_seq(data);
                }
                c.identity_token.encode(&mut e);
                e.put_octet_seq(&c.client_authentication_token);
            }
            SasContextBody::Complete { client_context_id, stateful, final_context_token } => {
                e.put_u32(1);
                e.put_u64(*client_context_id);
                e.put_bool(*stateful);
                e.put_octet_seq(final_context_token);
            }
            SasContextBody::Error {
                client_context_id,
                major_status,
                minor_status,
                error_token,
            } => {
                e.put_u32(4);
                e.put_u64(*client_context_id);
                e.put_i32(*major_status);
                e.put_i32(*minor_status);
                e.put_octet_seq(error_token);
            }
        }
        e.finish().map_err(Error::Cdr)
    }

    /// Decodes one.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut d = Decoder::encapsulation(data)?;
        let kind = d.get_u32()?;
        Ok(match kind {
            0 => {
                let client_context_id = d.get_u64()?;
                let count = d.get_u32()?;
                let count = d.validate_count(count, 8)?;
                let mut authorization_token = Vec::with_capacity(count);
                for _ in 0..count {
                    let kind = d.get_u32()?;
                    authorization_token.push((kind, d.get_octet_seq()?.to_vec()));
                }
                SasContextBody::Establish(EstablishContext {
                    client_context_id,
                    authorization_token,
                    identity_token: IdentityToken::decode(&mut d)?,
                    client_authentication_token: d.get_octet_seq()?.to_vec(),
                })
            }
            1 => SasContextBody::Complete {
                client_context_id: d.get_u64()?,
                stateful: d.get_bool()?,
                final_context_token: d.get_octet_seq()?.to_vec(),
            },
            4 => SasContextBody::Error {
                client_context_id: d.get_u64()?,
                major_status: d.get_i32()?,
                minor_status: d.get_i32()?,
                error_token: d.get_octet_seq()?.to_vec(),
            },
            _ => return Err(Error::BadIor("unhandled SAS message type")),
        })
    }
}

/// One `CSIIOP::CompoundSecMech` from a target's advertisement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompoundSecMech {
    /// What the target requires of the association.
    pub target_requires: u16,
    /// The transport mechanism component, if any.
    pub transport: Option<TaggedComponent>,
    /// Client authentication, if the target offers it.
    pub as_context: Option<AsContext>,
    /// Identity assertion, if the target offers it.
    pub sas_context: Option<SasContext>,
}

/// `CSIIOP::AS_ContextSec` — how a client authenticates itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsContext {
    /// Options the target supports.
    pub target_supports: u16,
    /// Options the target requires.
    pub target_requires: u16,
    /// The mechanism OID, DER-encoded.
    pub mechanism: Vec<u8>,
    /// The target's GSS exported name.
    pub target_name: Vec<u8>,
}

impl AsContext {
    /// Whether this mechanism is GSSUP.
    pub fn is_gssup(&self) -> bool {
        self.mechanism == GSSUP_OID
    }
}

/// `CSIIOP::SAS_ContextSec` — how an identity is asserted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SasContext {
    /// Options the target supports.
    pub target_supports: u16,
    /// Options the target requires.
    pub target_requires: u16,
    /// Naming mechanisms the target accepts, DER OIDs.
    pub naming_mechanisms: Vec<Vec<u8>>,
    /// Bitmask of `CSI::IdentityTokenType` values the target accepts.
    pub supported_identity_types: u32,
}

impl SasContext {
    /// Whether the target accepts a given identity token kind.
    pub fn accepts(&self, token: &IdentityToken) -> bool {
        // ITTAbsent is 0 and no bit represents it: a target that supports
        // nothing still accepts a call with nothing asserted.
        match token.token_type() {
            0 => true,
            t => self.supported_identity_types & t != 0,
        }
    }
}

/// What a target advertises: `CSIIOP::CompoundSecMechList`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecMechList {
    /// Whether the target supports stateful contexts.
    pub stateful: bool,
    /// The mechanisms, in the target's order of preference.
    pub mechanisms: Vec<CompoundSecMech>,
}

impl SecMechList {
    /// Parses the body of a `TAG_CSI_SEC_MECH_LIST` component.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut d = Decoder::encapsulation(data)?;
        let stateful = d.get_bool()?;
        let count = d.get_u32()?;
        let count = d.validate_count(count, 8)?;
        let mut mechanisms = Vec::with_capacity(count);
        for _ in 0..count {
            let target_requires = d.get_u16()?;
            let tag = d.get_u32()?;
            let data = d.get_octet_seq()?.to_vec();
            // TAG_NULL_TAG with an empty body means "no transport mechanism",
            // which is different from the component being absent.
            let transport = if tag == TAG_NULL_TAG && data.is_empty() {
                None
            } else {
                Some(TaggedComponent { tag, data })
            };

            let as_context = AsContext {
                target_supports: d.get_u16()?,
                target_requires: d.get_u16()?,
                mechanism: d.get_octet_seq()?.to_vec(),
                target_name: d.get_octet_seq()?.to_vec(),
            };
            // A zeroed AS_ContextSec is how a target says it offers no client
            // authentication; keeping it as `Some` would make "offers nothing"
            // and "offers an unnamed mechanism" indistinguishable.
            let as_context = if as_context.target_supports == 0 { None } else { Some(as_context) };

            let sas_supports = d.get_u16()?;
            let sas_requires = d.get_u16()?;
            let priv_count = d.get_u32()?;
            let priv_count = d.validate_count(priv_count, 8)?;
            for _ in 0..priv_count {
                let _syntax = d.get_u32()?;
                let _name = d.get_octet_seq()?;
            }
            let naming_count = d.get_u32()?;
            let naming_count = d.validate_count(naming_count, 4)?;
            let mut naming_mechanisms = Vec::with_capacity(naming_count);
            for _ in 0..naming_count {
                naming_mechanisms.push(d.get_octet_seq()?.to_vec());
            }
            let supported_identity_types = d.get_u32()?;
            let sas_context = if sas_supports == 0 && supported_identity_types == 0 {
                None
            } else {
                Some(SasContext {
                    target_supports: sas_supports,
                    target_requires: sas_requires,
                    naming_mechanisms,
                    supported_identity_types,
                })
            };

            mechanisms.push(CompoundSecMech {
                target_requires,
                transport,
                as_context,
                sas_context,
            });
        }
        Ok(SecMechList { stateful, mechanisms })
    }

    /// The first mechanism that accepts an asserted identity, if any.
    pub fn identity_assertion(&self) -> Option<&SasContext> {
        self.mechanisms.iter().find_map(|m| {
            m.sas_context.as_ref().filter(|s| s.target_supports & options::IDENTITY_ASSERTION != 0)
        })
    }
}

/// Finds a target's advertisement in an IOR's components.
///
/// `None` means the target advertises nothing — the case §4.8 calls the common
/// one. It is not an error and must not be treated as one: against such a
/// target the bridge cannot delegate, only record, and *that* is what belongs
/// in the catalogue.
pub fn advertised(components: &[TaggedComponent]) -> Option<Result<SecMechList>> {
    components.iter().find(|c| c.tag == TAG_CSI_SEC_MECH_LIST).map(|c| SecMechList::parse(&c.data))
}

/// DER definite-length encoding.
fn der_length(len: usize) -> Vec<u8> {
    if len < 0x80 {
        return vec![len as u8];
    }
    let bytes = len.to_be_bytes();
    let first = bytes.iter().position(|b| *b != 0).unwrap_or(bytes.len() - 1);
    let mut out = vec![0x80 | (bytes.len() - first) as u8];
    out.extend_from_slice(&bytes[first..]);
    out
}

fn parse_der_length(bytes: &[u8]) -> Result<(usize, usize)> {
    let first = *bytes.first().ok_or(Error::BadIor("GSS token has no length"))?;
    if first < 0x80 {
        return Ok((first as usize, 1));
    }
    let n = (first & 0x7F) as usize;
    // A length field longer than a usize cannot be honoured, and an indefinite
    // length (0x80) has no place in a GSS token.
    if n == 0 || n > 8 || bytes.len() < 1 + n {
        return Err(Error::BadIor("GSS token has a malformed length"));
    }
    // Accumulated in `u64` and converted, not accumulated in `usize`: `n` may
    // be 8, so on a 32-bit target `len << 8` would shift the peer's leading
    // bytes out and turn an absurd length into a plausible small one. A length
    // that does not fit the address space is refused, never truncated into one
    // that does.
    let mut len = 0u64;
    for b in &bytes[1..1 + n] {
        len = (len << 8) | *b as u64;
    }
    let len = usize::try_from(len)
        .map_err(|_| Error::BadIor("GSS token declares a length no buffer can hold"))?;
    Ok((len, 1 + n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip_body(body: &SasContextBody) {
        for endian in [Endian::Big, Endian::Little] {
            let bytes = body.encode(endian).expect("encodes");
            assert_eq!(&SasContextBody::parse(&bytes).expect("parses"), body, "{endian:?}");
        }
    }

    #[test]
    fn an_establish_context_round_trips_in_both_byte_orders() {
        round_trip_body(&SasContextBody::Establish(EstablishContext {
            client_context_id: 0x0102_0304_0506_0708,
            authorization_token: vec![(1, b"scope=read".to_vec())],
            identity_token: IdentityToken::PrincipalName(b"alice@example.com".to_vec()),
            client_authentication_token: b"opaque".to_vec(),
        }));
    }

    #[test]
    fn every_identity_token_kind_round_trips() {
        for token in [
            IdentityToken::Absent,
            IdentityToken::Anonymous,
            IdentityToken::PrincipalName(b"svc/bridge".to_vec()),
            IdentityToken::X509CertChain(vec![0x30, 0x82, 0x01]),
            IdentityToken::DistinguishedName(b"CN=alice".to_vec()),
        ] {
            round_trip_body(&SasContextBody::Establish(EstablishContext {
                identity_token: token,
                ..Default::default()
            }));
        }
    }

    #[test]
    fn the_target_replies_round_trip_too() {
        round_trip_body(&SasContextBody::Complete {
            client_context_id: 7,
            stateful: true,
            final_context_token: vec![1, 2, 3],
        });
        round_trip_body(&SasContextBody::Error {
            client_context_id: 7,
            major_status: -1,
            minor_status: 42,
            error_token: vec![],
        });
    }

    /// Absent and anonymous are different claims. A target may accept one and
    /// refuse the other, so collapsing them would make the bridge assert
    /// something it was not told.
    #[test]
    fn absent_is_not_anonymous() {
        assert_ne!(IdentityToken::Absent, IdentityToken::Anonymous);
        assert_ne!(IdentityToken::Absent.token_type(), IdentityToken::Anonymous.token_type());
        let a = SasContextBody::Establish(EstablishContext {
            identity_token: IdentityToken::Absent,
            ..Default::default()
        })
        .encode(Endian::Big)
        .unwrap();
        let b = SasContextBody::Establish(EstablishContext {
            identity_token: IdentityToken::Anonymous,
            ..Default::default()
        })
        .encode(Endian::Big)
        .unwrap();
        assert_ne!(a, b, "they must differ on the wire");
    }

    #[test]
    fn a_gssup_token_round_trips_through_its_rfc_2743_framing() {
        let t = GssUpToken {
            username: b"alice".to_vec(),
            password: b"hunter2".to_vec(),
            target_name: b"bank-service".to_vec(),
        };
        for endian in [Endian::Big, Endian::Little] {
            let bytes = t.encode(endian).unwrap();
            assert_eq!(bytes[0], 0x60, "the mechanism-independent token starts with 0x60");
            assert!(bytes[1..].starts_with(&[]) && bytes.len() > GSSUP_OID.len());
            assert_eq!(GssUpToken::decode(&bytes).unwrap(), t, "{endian:?}");
        }
    }

    /// A token for another mechanism has a differently shaped body, so reading
    /// it as GSSUP would produce a username from whatever bytes were there.
    #[test]
    fn a_token_for_another_mechanism_is_refused_rather_than_reinterpreted() {
        let mut bytes = GssUpToken {
            username: b"alice".to_vec(),
            password: b"x".to_vec(),
            target_name: b"t".to_vec(),
        }
        .encode(Endian::Big)
        .unwrap();
        // Change the last arc of the OID: still a valid DER OID, not GSSUP.
        let oid_at = bytes.iter().position(|b| *b == 0x06).unwrap();
        bytes[oid_at + GSSUP_OID.len() - 1] = 0x02;
        assert!(GssUpToken::decode(&bytes).is_err());
    }

    #[test]
    fn a_truncated_gss_token_is_refused_at_every_length() {
        let full = GssUpToken {
            username: b"alice".to_vec(),
            password: b"hunter2".to_vec(),
            target_name: b"bank".to_vec(),
        }
        .encode(Endian::Big)
        .unwrap();
        for i in 0..full.len() {
            let _ = GssUpToken::decode(&full[..i]);
        }
        assert!(GssUpToken::decode(&full[..full.len() - 1]).is_err());
    }

    /// A DER length the peer chose, added to a cursor. `60 88 FF FF FF FF FF
    /// FF FF FF` declares `usize::MAX`: measured, it panicked with "attempt to
    /// add with overflow" in a debug build and wrapped to a misleading
    /// `GSS token is truncated` in release. The release half is why a
    /// `--release` fuzzer could never report it, so this test asserts the
    /// *same* refusal in both profiles — it runs in both, and every long form
    /// is covered rather than the one byte string that was reported.
    ///
    /// Reachable from any `EstablishContext.client_authentication_token`.
    #[test]
    fn a_gss_length_that_cannot_be_added_to_the_cursor_is_refused_not_panicked() {
        for n in 1..=8usize {
            let mut token = vec![0x60, 0x80 | n as u8];
            token.resize(2 + n, 0xFF);
            match GssUpToken::decode(&token) {
                Err(Error::BadIor(_)) => {}
                other => panic!("{token:02X?} gave {other:?}"),
            }
        }
    }

    /// The negative control for the refusal above: a body long enough to force
    /// the DER long form is still decoded, so the fix refuses lengths that
    /// cannot fit rather than lengths that merely need two bytes.
    #[test]
    fn an_honest_long_form_length_still_decodes() {
        let t = GssUpToken {
            username: vec![b'u'; 300],
            password: b"hunter2".to_vec(),
            target_name: b"bank".to_vec(),
        };
        for endian in [Endian::Big, Endian::Little] {
            let bytes = t.encode(endian).expect("encodes");
            assert!(bytes[1] & 0x80 != 0, "expected the DER long form, got {:02X}", bytes[1]);
            assert_eq!(GssUpToken::decode(&bytes).expect("decodes"), t, "{endian:?}");
        }
    }

    /// §4.8's credential hygiene, made structural. A redaction that depends on
    /// nobody writing `{:?}` is not a control.
    #[test]
    fn a_password_cannot_be_obtained_from_a_formatter() {
        let t = GssUpToken {
            username: b"alice".to_vec(),
            password: b"correct-horse-battery-staple".to_vec(),
            target_name: b"bank".to_vec(),
        };
        for rendered in [format!("{t:?}"), format!("{t:#?}")] {
            assert!(!rendered.contains("correct-horse"), "the password leaked: {rendered}");
            assert!(rendered.contains("<redacted>"), "{rendered}");
            // The username is not a secret and is what an audit entry needs.
            assert!(rendered.contains("alice"), "{rendered}");
        }
    }

    /// The audit rule: which principal, never the material.
    #[test]
    fn an_audit_name_never_reproduces_credential_material() {
        assert_eq!(
            IdentityToken::PrincipalName(b"alice@example.com".to_vec()).audit_name(),
            "alice@example.com"
        );
        let chain = IdentityToken::X509CertChain(vec![0xAB; 900]);
        let name = chain.audit_name();
        assert!(name.contains("900 bytes"), "{name}");
        assert!(!name.contains('\u{AB}'), "{name}");
    }

    fn advertisement(supports: u16, identity_types: u32, as_supports: u16) -> Vec<u8> {
        let mut e = Encoder::encapsulation(Endian::Big);
        e.put_bool(false); // stateful
        e.put_u32(1); // one mechanism
        e.put_u16(options::ESTABLISH_TRUST_IN_CLIENT); // target_requires
        e.put_u32(TAG_NULL_TAG);
        e.put_octet_seq(&[]);
        e.put_u16(as_supports);
        e.put_u16(0);
        e.put_octet_seq(GSSUP_OID);
        e.put_octet_seq(b"bank@example.com");
        e.put_u16(supports);
        e.put_u16(0);
        e.put_u32(0); // no privilege authorities
        e.put_u32(1);
        e.put_octet_seq(GSSUP_OID);
        e.put_u32(identity_types);
        e.finish().unwrap()
    }

    #[test]
    fn a_targets_advertisement_parses() {
        let data =
            advertisement(options::IDENTITY_ASSERTION, 2 | 8, options::ESTABLISH_TRUST_IN_CLIENT);
        let list = SecMechList::parse(&data).expect("parses");
        assert!(!list.stateful);
        assert_eq!(list.mechanisms.len(), 1);
        let m = &list.mechanisms[0];
        assert!(m.transport.is_none(), "TAG_NULL_TAG with no body means no transport mechanism");
        assert!(m.as_context.as_ref().unwrap().is_gssup());
        let sas = list.identity_assertion().expect("supports identity assertion");
        assert!(sas.accepts(&IdentityToken::PrincipalName(b"alice".to_vec())));
        assert!(sas.accepts(&IdentityToken::DistinguishedName(b"CN=x".to_vec())));
        assert!(!sas.accepts(&IdentityToken::X509CertChain(vec![])), "type 4 was not offered");
    }

    /// A target that offers nothing must read as offering nothing, not as
    /// offering an unnamed mechanism.
    #[test]
    fn a_target_that_offers_nothing_says_so_rather_than_looking_configured() {
        let data = advertisement(0, 0, 0);
        let list = SecMechList::parse(&data).unwrap();
        assert!(list.mechanisms[0].as_context.is_none());
        assert!(list.mechanisms[0].sas_context.is_none());
        assert!(list.identity_assertion().is_none());
    }

    /// The common case, and not an error: no component at all.
    #[test]
    fn an_ior_with_no_csiv2_component_is_not_an_error() {
        assert!(advertised(&[]).is_none());
        assert!(advertised(&[TaggedComponent { tag: 1, data: vec![] }]).is_none());
    }

    #[test]
    fn der_lengths_round_trip_across_the_short_form_boundary() {
        for len in [0usize, 1, 127, 128, 255, 256, 65_535, 65_536] {
            let encoded = der_length(len);
            let (back, used) = parse_der_length(&encoded).expect("parses");
            assert_eq!(back, len, "{encoded:?}");
            assert_eq!(used, encoded.len());
        }
    }

    #[test]
    fn a_malformed_der_length_is_refused() {
        for bad in [vec![0x80], vec![0x89, 1, 2, 3], vec![0x82, 1]] {
            assert!(parse_der_length(&bad).is_err(), "{bad:?}");
        }
    }

    /// A declared count must be validated against the bytes present, or a
    /// four-byte field buys a multi-gigabyte allocation — the worst finding of
    /// the Phase 0 audit, arriving through a new door.
    #[test]
    fn a_huge_declared_mechanism_count_is_refused() {
        let mut e = Encoder::encapsulation(Endian::Big);
        e.put_bool(false);
        e.put_u32(0xFFFF_FFFF);
        let data = e.finish().unwrap();
        assert!(SecMechList::parse(&data).is_err());
    }

    #[test]
    fn every_truncation_of_an_advertisement_is_refused_without_panicking() {
        let data = advertisement(options::IDENTITY_ASSERTION, 2, 64);
        for i in 0..data.len() {
            let _ = SecMechList::parse(&data[..i]);
        }
    }
}
