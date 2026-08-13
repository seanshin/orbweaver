//! SSLIOP: where an IOR says "there is a TLS listener, and this is its port".
//!
//! This is the transport-identity row of `docs/PLAN.md` §4.8 — the layer that
//! answers *which process is connected*, below CSIv2's *on whose behalf*. A
//! target that accepts TLS advertises it with `TAG_SSL_SEC_TRANS` (IOP
//! ComponentId 20) on its IIOP profile. The component body is a CDR
//! encapsulation of the `SSLIOP::SSL` struct from the OMG Security Service
//! specification's SSLIOP chapter:
//!
//! ```idl
//! struct SSL {
//!     Security::AssociationOptions target_supports;  // unsigned short
//!     Security::AssociationOptions target_requires;  // unsigned short
//!     unsigned short               port;
//! };
//! ```
//!
//! The option bits are the same values CSIv2 later adopted as
//! `CSI::AssociationOptions`, so this module reuses [`crate::csiv2::options`]
//! rather than redefining them — one wire fact should have one set of names.
//! SSLIOP predates CSIv2 and only the transport-capable bits
//! ([`NO_PROTECTION`](crate::csiv2::options::NO_PROTECTION) through
//! [`ESTABLISH_TRUST_IN_CLIENT`](crate::csiv2::options::ESTABLISH_TRUST_IN_CLIENT))
//! are meaningful on a transport; the delegation bits belong to layers above.
//!
//! # What this module does not claim
//!
//! It parses and re-encodes the advertisement; the handshake itself lives in
//! `Connection::connect_tls`, behind the off-by-default `ssliop` feature. Per
//! `docs/decisions/D002-tls-dependency.md` (approved 2026-08-13) that feature
//! depends on rustls rather than shipping first-party TLS, because no oracle
//! we can run detects cryptographic mistakes — a default build carries no TLS
//! dependency at all.
//!
//! Honesty about what has been measured: the TLS path has been exercised
//! against an in-process rustls peer only. No SSLIOP-speaking ORB (omniORB's
//! sslTP, JacORB's SSL transport) has been dialed yet — that fixture is a
//! future batch — so today's verified claim is "the TLS layer works and GIOP
//! passes through it unchanged", not peer interop.

use orbweaver_cdr::{Decoder, Encoder, Endian};

use crate::{Error, IiopProfile, Result, TaggedComponent};

/// `IOP::ComponentId` for the `SSLIOP::SSL` component.
pub const TAG_SSL_SEC_TRANS: u32 = 20;

/// A parsed `SSLIOP::SSL` component body.
///
/// `target_supports` and `target_requires` carry
/// [`crate::csiv2::options`] bits. The pair is what decides whether a
/// cleartext caller may proceed at all: a target whose `target_requires`
/// includes more than `NO_PROTECTION` is saying that the cleartext port next
/// to this component is not actually usable for invocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SslComponent {
    /// Association options the target can provide over TLS.
    pub target_supports: u16,
    /// Association options the target insists on.
    pub target_requires: u16,
    /// TCP port of the TLS listener, on the profile's host.
    ///
    /// A port and no host, by design: the SSL listener is a second listener of
    /// the same server, so the host is the profile's. See [`ssl_endpoint`].
    pub port: u16,
}

impl SslComponent {
    /// Parses the body of a `TAG_SSL_SEC_TRANS` component.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut d = Decoder::encapsulation(data)?;
        Ok(SslComponent {
            target_supports: d.get_u16()?,
            target_requires: d.get_u16()?,
            port: d.get_u16()?,
        })
    }

    /// Encodes the component body as the encapsulation an IOR carries.
    ///
    /// Needed the day we emit IORs for a TLS listener of our own; needed today
    /// so the round-trip tests exercise the same encoder a peer would face.
    pub fn encode(&self, endian: Endian) -> Result<Vec<u8>> {
        let mut e = Encoder::encapsulation(endian);
        e.put_u16(self.target_supports);
        e.put_u16(self.target_requires);
        e.put_u16(self.port);
        e.finish().map_err(Error::Cdr)
    }
}

/// Finds a profile's SSLIOP advertisement in its components.
///
/// `None` means the target advertises no TLS endpoint — the common case among
/// the legacy targets §4.8 describes, and not an error. It must not be treated
/// as one: against such a target there is no transport identity to verify,
/// only a fact for the catalogue to record. `Some(Err(_))` is different — a
/// component that *claims* to be an SSLIOP advertisement but cannot be read is
/// worth surfacing, because silently ignoring it downgrades to cleartext.
pub fn advertised(components: &[TaggedComponent]) -> Option<Result<SslComponent>> {
    components.iter().find(|c| c.tag == TAG_SSL_SEC_TRANS).map(|c| SslComponent::parse(&c.data))
}

/// The TLS endpoint a profile advertises, if any.
///
/// The SSL port replaces the profile's cleartext port **at the same host**:
/// the Security Service specification's SSLIOP chapter (§3 in its standalone
/// numbering) defines the component as carrying only a port precisely because
/// the SSL listener is a different port of the same server the profile already
/// names. That is also why a component with its own host field would be
/// suspect — the certificate the handshake verifies is for the profile's host.
///
/// A component port of 0 is treated as "the profile's own port is the TLS
/// port". To be honest about provenance: the specification text this module is
/// written against says the field carries the port the target listens on for
/// SSL, and assigns no meaning to 0 that we can point to; "0 means same port"
/// is deployed-ORB convention, not spec. The *reverse* convention is the
/// better-attested one — an SSL-only target sets the **profile** port to 0 and
/// puts the real port here, which needs no special case in this function.
///
/// A malformed component yields `None` here, because this function answers
/// "where would I dial TLS" and an unreadable advertisement gives no answer.
/// A caller that must refuse to fall back to cleartext when a target required
/// protection cannot use this shortcut; it must look at [`advertised`], where
/// the malformation is an `Err` it can act on.
pub fn ssl_endpoint(profile: &IiopProfile) -> Option<(String, u16)> {
    let ssl = advertised(&profile.components)?.ok()?;
    let port = if ssl.port == 0 { profile.port } else { ssl.port };
    Some((profile.host.clone(), port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Version;
    use crate::csiv2::options;

    fn profile_with(components: Vec<TaggedComponent>) -> IiopProfile {
        IiopProfile {
            version: Version::V1_2,
            host: "legacy.example.com".into(),
            port: 2809,
            object_key: b"key".to_vec(),
            components,
        }
    }

    fn component(ssl: &SslComponent, endian: Endian) -> TaggedComponent {
        TaggedComponent { tag: TAG_SSL_SEC_TRANS, data: ssl.encode(endian).unwrap() }
    }

    #[test]
    fn an_ssl_component_round_trips_in_both_byte_orders() {
        let ssl = SslComponent {
            target_supports: options::INTEGRITY
                | options::CONFIDENTIALITY
                | options::ESTABLISH_TRUST_IN_TARGET,
            target_requires: options::INTEGRITY | options::CONFIDENTIALITY,
            port: 15_001,
        };
        for endian in [Endian::Big, Endian::Little] {
            let bytes = ssl.encode(endian).expect("encodes");
            assert_eq!(SslComponent::parse(&bytes).expect("parses"), ssl, "{endian:?}");
        }
    }

    #[test]
    fn every_truncation_is_refused_without_panicking() {
        for endian in [Endian::Big, Endian::Little] {
            let full = SslComponent {
                target_supports: options::CONFIDENTIALITY,
                target_requires: options::NO_PROTECTION,
                port: 443,
            }
            .encode(endian)
            .unwrap();
            for i in 0..full.len() {
                assert!(
                    SslComponent::parse(&full[..i]).is_err(),
                    "{endian:?}: a {i}-byte prefix of {} bytes must be refused",
                    full.len()
                );
            }
        }
    }

    /// The common case, and not an error: no component at all.
    #[test]
    fn absence_is_none_not_error() {
        assert!(advertised(&[]).is_none());
        assert!(advertised(&[TaggedComponent { tag: 1, data: vec![] }]).is_none());
        assert!(ssl_endpoint(&profile_with(vec![])).is_none());
    }

    /// Unreadable is different from absent: a claim that cannot be read is
    /// surfaced, because ignoring it silently downgrades to cleartext.
    #[test]
    fn a_present_but_unreadable_component_is_an_error_not_absence() {
        let bad = TaggedComponent { tag: TAG_SSL_SEC_TRANS, data: vec![0x00, 0x00] };
        assert!(matches!(advertised(std::slice::from_ref(&bad)), Some(Err(_))));
        // The dial-shortcut cannot express the error and says None; the doc
        // comment sends security-sensitive callers to `advertised` instead.
        assert!(ssl_endpoint(&profile_with(vec![bad])).is_none());
    }

    #[test]
    fn the_ssl_port_replaces_the_cleartext_port_at_the_same_host() {
        let ssl = SslComponent {
            target_supports: options::CONFIDENTIALITY,
            target_requires: 0,
            port: 15_001,
        };
        let profile = profile_with(vec![component(&ssl, Endian::Big)]);
        assert_eq!(ssl_endpoint(&profile), Some(("legacy.example.com".into(), 15_001)));
    }

    /// Port 0 in the component means "the profile's port is the TLS port" —
    /// deployed convention, not spec text we can cite; the module docs say so.
    #[test]
    fn a_component_port_of_zero_falls_back_to_the_profile_port() {
        let ssl = SslComponent { target_supports: 0, target_requires: 0, port: 0 };
        let profile = profile_with(vec![component(&ssl, Endian::Little)]);
        assert_eq!(ssl_endpoint(&profile), Some(("legacy.example.com".into(), 2809)));
    }

    /// The better-attested convention needs no special case: an SSL-only
    /// target zeroes the *profile* port and advertises the real one here.
    #[test]
    fn an_ssl_only_profile_with_cleartext_port_zero_still_yields_the_tls_endpoint() {
        let ssl = SslComponent {
            target_supports: options::CONFIDENTIALITY | options::ESTABLISH_TRUST_IN_CLIENT,
            target_requires: options::CONFIDENTIALITY | options::ESTABLISH_TRUST_IN_CLIENT,
            port: 15_002,
        };
        let mut profile = profile_with(vec![component(&ssl, Endian::Big)]);
        profile.port = 0;
        assert_eq!(ssl_endpoint(&profile), Some(("legacy.example.com".into(), 15_002)));
    }
}
