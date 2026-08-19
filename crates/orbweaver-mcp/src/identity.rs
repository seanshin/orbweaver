//! The token exchange point, which is a trust boundary rather than a mapping.
//!
//! `docs/PLAN.md` §4.8. An agent arrives holding an OAuth2 or JWT credential; a
//! legacy target understands GSSUP or a CSIv2 identity token. Something has to
//! convert one into the other, and **whatever performs that conversion is
//! asserting to the target that a claim the target cannot itself verify is
//! true.** That is not a lookup table. It is the moment the bridge's own
//! trustworthiness becomes the security of every system behind it.
//!
//! Four things §4.8 says will be uncomfortable, and what this module does about
//! each:
//!
//! 1. **CSIv2 interop across vendors is poor.** So delegation is configured per
//!    target and never inferred. A target that advertises nothing gets nothing
//!    asserted.
//! 2. **Many legacy targets have no authentication at all.** Against those the
//!    bridge cannot delegate; it can only *record*. [`Assertion::RecordedOnly`]
//!    is that case, named rather than dressed up — asserting an identity a
//!    target ignores is theatre, and calling it a control would be worse than
//!    leaving it out.
//! 3. **Delegation done wrong is privilege escalation.** Impersonation is
//!    default-deny, enabled per interface with a recorded reason, and never
//!    inherited from the agent having been trusted enough to connect.
//! 4. **Token lifetime and connection lifetime disagree.** CORBA connections are
//!    long-lived by design and tokens expire by design, so a call on an expired
//!    context is refused rather than allowed to proceed quietly.

use std::collections::BTreeMap;
use std::time::SystemTime;

use orbweaver_dynamic::json::Json;
use orbweaver_giop::csiv2::{self, GssUpToken, IdentityToken, SecMechList};
use orbweaver_giop::{Ior, ServiceContext, TaggedComponent, ssliop};

/// Who a call is being made on behalf of, as the bridge received it.
///
/// Deliberately not the credential: the material an agent authenticated with
/// has no business travelling further, and this type is what the rest of the
/// bridge passes around. §4.8's hygiene rule is easier to keep when the
/// dangerous thing is simply absent from the type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    /// The principal, as the bridge's own authentication established it.
    pub principal: String,
    /// Authorization scopes, for matching against `@ai_authz`.
    pub scopes: Vec<String>,
    /// When the caller's credential stops being valid.
    pub expires_at: Option<SystemTime>,
}

impl Caller {
    /// A caller with no scopes and no expiry.
    pub fn new(principal: impl Into<String>) -> Self {
        Self { principal: principal.into(), scopes: Vec::new(), expires_at: None }
    }

    /// Adds a scope.
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scopes.push(scope.into());
        self
    }

    /// Sets an expiry.
    pub fn expiring_at(mut self, at: SystemTime) -> Self {
        self.expires_at = Some(at);
        self
    }

    /// Whether the credential is still valid at `now`.
    pub fn valid_at(&self, now: SystemTime) -> bool {
        self.expires_at.is_none_or(|at| now < at)
    }
}

/// What the bridge will do about identity for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assertion {
    /// The target accepts an asserted identity and one will be sent.
    Assert(IdentityToken),
    /// The target cannot enforce anything, so the bridge records the caller and
    /// asserts nothing.
    ///
    /// This is the honest answer for most legacy targets and is **not** a
    /// degraded form of `Assert`. Where the target cannot enforce, the bridge is
    /// the only enforcement point, and the catalogue says so: the `why` here is
    /// [`PeerCapability::unenforced`]'s answer for the same target, so this arm
    /// and the catalogue's record cannot disagree about a peer — one
    /// classification of the IOR, read twice.
    RecordedOnly {
        /// Why the target cannot enforce a caller identity, as its IOR says.
        why: Unenforced,
    },
}

/// Why a target cannot enforce a caller identity, read off its IOR.
///
/// Three ways an IOR fails to advertise identity assertion, kept apart because
/// they call for different remedies: nothing advertised is the legacy-estate
/// baseline §4.8 predicts, an unreadable advertisement is a peer worth a
/// diagnosis, and a list without identity assertion is a peer that
/// authenticates the *bridge* and still cannot tell whose call it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unenforced {
    /// The IOR carries no `TAG_CSI_SEC_MECH_LIST` at all.
    NoMechanismList,
    /// The IOR carries one and it does not parse.
    MechanismListUnreadable,
    /// The IOR carries a readable list and no mechanism in it supports
    /// `IDENTITY_ASSERTION`.
    NoIdentityAssertion {
        /// How many mechanisms the list names.
        mechanisms: usize,
    },
}

impl Unenforced {
    /// The reason in words, for an audit line or a catalogue page.
    pub fn reason(&self) -> String {
        match self {
            Unenforced::NoMechanismList => "the target advertises no CSIv2 mechanism list, so it \
                                            cannot enforce a caller identity and the bridge is \
                                            the only enforcement point"
                .to_owned(),
            Unenforced::MechanismListUnreadable => "the target's CSIv2 mechanism list does not \
                                                    parse, so no identity can be asserted to it \
                                                    and the bridge is the only enforcement point"
                .to_owned(),
            Unenforced::NoIdentityAssertion { mechanisms } => format!(
                "the target advertises {mechanisms} CSIv2 mechanism(s) and none accepts an \
                 asserted identity, so it cannot enforce a caller identity and the bridge is \
                 the only enforcement point"
            ),
        }
    }

    /// A short token for a record or a grep: `no-mechanism-list`,
    /// `mechanism-list-unreadable`, `no-identity-assertion`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Unenforced::NoMechanismList => "no-mechanism-list",
            Unenforced::MechanismListUnreadable => "mechanism-list-unreadable",
            Unenforced::NoIdentityAssertion { .. } => "no-identity-assertion",
        }
    }
}

/// Where a caller identity is enforced for one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementPoint {
    /// The target accepts an asserted identity, so it can refuse a call on the
    /// strength of who the caller is.
    Target,
    /// The target cannot, so every authorization decision about a caller is
    /// made here, in the bridge, and nowhere behind it.
    BridgeOnly,
}

impl EnforcementPoint {
    /// The token as a record carries it: `target` or `bridge only`.
    pub fn as_str(&self) -> &'static str {
        match self {
            EnforcementPoint::Target => "target",
            EnforcementPoint::BridgeOnly => "bridge only",
        }
    }
}

/// What one target can enforce, read off the IOR the bridge holds for it.
///
/// PLAN §4.8: *CSIv2 support is per-peer, never a feature flag*, and where a
/// target cannot enforce a caller identity the bridge is the only enforcement
/// point and the catalogue has to say so. This is the record that says so. It
/// is derived from the IOR alone — the `TAG_CSI_SEC_MECH_LIST` and
/// `TAG_SSL_SEC_TRANS` components of the primary profile, through the same
/// parsers the wire uses — so it can be produced for a peer nobody has dialed
/// yet; and it is what [`Delegation::decide`] reads, so a catalogue page and an
/// audit line describe the same peer in the same words.
///
/// Measured 2026-08-19 on both project fixtures: omniORB 4.3.4 and JacORB 3.9
/// both produce `enforces_identity: false, transport_secured: false,
/// enforcement point: bridge only` — the common case §4.8 anticipates, stated
/// per peer rather than assumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerCapability {
    /// The target's `TAG_CSI_SEC_MECH_LIST`, if it advertises one and it
    /// parses. `None` covers both absence and an unreadable list;
    /// [`PeerCapability::unenforced`] tells them apart.
    pub mechanisms: Option<SecMechList>,
    /// Whether a `TAG_CSI_SEC_MECH_LIST` was present and did not parse.
    pub mechanisms_unreadable: bool,
    /// The target's `TAG_SSL_SEC_TRANS`, if it advertises a TLS listener and
    /// the component parses.
    pub transport: Option<ssliop::SslComponent>,
    /// Whether a `TAG_SSL_SEC_TRANS` was present and did not parse — an
    /// advertisement that *claims* TLS and cannot be read is worth a line,
    /// since silently ignoring it downgrades to cleartext.
    pub transport_unreadable: bool,
}

impl PeerCapability {
    /// The record for a target that advertises nothing at all — the legacy
    /// baseline, and what a nil or profile-less reference is treated as.
    pub fn advertising_nothing() -> Self {
        Self {
            mechanisms: None,
            mechanisms_unreadable: false,
            transport: None,
            transport_unreadable: false,
        }
    }

    /// Reads the record off an IOR's primary profile.
    ///
    /// The primary profile is the one the bridge dials first and the one whose
    /// components a call is made against; a reference with no IIOP profile at
    /// all advertises nothing this record could carry.
    pub fn of_ior(ior: &Ior) -> Self {
        match ior.primary() {
            Ok(profile) => Self::of_components(&profile.components),
            Err(_) => Self::advertising_nothing(),
        }
    }

    /// Reads the record off a profile's tagged components.
    pub fn of_components(components: &[TaggedComponent]) -> Self {
        let (mechanisms, mechanisms_unreadable) = match csiv2::advertised(components) {
            None => (None, false),
            Some(Ok(list)) => (Some(list), false),
            Some(Err(_)) => (None, true),
        };
        let (transport, transport_unreadable) = match ssliop::advertised(components) {
            None => (None, false),
            Some(Ok(ssl)) => (Some(ssl), false),
            Some(Err(_)) => (None, true),
        };
        Self { mechanisms, mechanisms_unreadable, transport, transport_unreadable }
    }

    /// The first mechanism that accepts an asserted identity, if any — what
    /// [`Delegation::decide`] asserts to.
    pub fn identity_assertion(&self) -> Option<&csiv2::SasContext> {
        self.mechanisms.as_ref().and_then(SecMechList::identity_assertion)
    }

    /// Why the target cannot enforce a caller identity, or `None` when it can.
    ///
    /// The one classification [`Assertion::RecordedOnly`], the audit line and
    /// the catalogue all read.
    pub fn unenforced(&self) -> Option<Unenforced> {
        if self.identity_assertion().is_some() {
            return None;
        }
        Some(match &self.mechanisms {
            Some(list) => Unenforced::NoIdentityAssertion { mechanisms: list.mechanisms.len() },
            None if self.mechanisms_unreadable => Unenforced::MechanismListUnreadable,
            None => Unenforced::NoMechanismList,
        })
    }

    /// Whether the target advertises a mechanism that accepts an asserted
    /// identity — whether it can enforce *who* is calling.
    pub fn enforces_identity(&self) -> bool {
        self.unenforced().is_none()
    }

    /// Whether the target advertises a TLS listener (`TAG_SSL_SEC_TRANS`).
    ///
    /// Advertised, not verified: this record is read off the IOR, and whether
    /// the listener answers is measured only by dialing it (D002).
    pub fn transport_secured(&self) -> bool {
        self.transport.is_some()
    }

    /// Where a caller identity is enforced for this target.
    pub fn enforcement_point(&self) -> EnforcementPoint {
        if self.enforces_identity() {
            EnforcementPoint::Target
        } else {
            EnforcementPoint::BridgeOnly
        }
    }

    /// The identity half of the record, in words.
    pub fn identity_sentence(&self) -> String {
        match (self.unenforced(), self.identity_assertion()) {
            (None, Some(sas)) => format!(
                "enforced by the target — it accepts an asserted identity (token types {:#x}); \
                 the bridge asserts, the target decides",
                sas.supported_identity_types
            ),
            (why, _) => {
                let because = match why {
                    Some(Unenforced::NoMechanismList) | None => {
                        "no CSIv2 mechanism list in the IOR".to_owned()
                    }
                    Some(Unenforced::MechanismListUnreadable) => {
                        "the IOR's CSIv2 mechanism list does not parse".to_owned()
                    }
                    Some(Unenforced::NoIdentityAssertion { mechanisms }) => format!(
                        "{mechanisms} CSIv2 mechanism(s) advertised, none accepts an asserted \
                         identity"
                    ),
                };
                format!(
                    "not enforced by the target — the bridge is the only enforcement point \
                     ({because})"
                )
            }
        }
    }

    /// The transport half of the record, in words.
    pub fn transport_sentence(&self) -> String {
        match &self.transport {
            Some(ssl) => format!(
                "TLS advertised (TAG_SSL_SEC_TRANS, port {}, requires {:#06x}) — advertised, not \
                 dialed",
                ssl.port, ssl.target_requires
            ),
            None if self.transport_unreadable => {
                "TAG_SSL_SEC_TRANS present and unreadable — treated as cleartext, which is a \
                 downgrade worth a look"
                    .to_owned()
            }
            None => "cleartext — no TAG_SSL_SEC_TRANS in the IOR".to_owned(),
        }
    }

    /// The record as a JSON object, for a tool result or a catalogue export.
    ///
    /// Booleans, counts and tokens only — no host, port or object key — so a
    /// record can sit in an agent-facing document without carrying anything
    /// dialable.
    pub fn to_json(&self) -> Json {
        let why = match self.unenforced() {
            Some(why) => Json::String(why.as_str().to_owned()),
            None => Json::Null,
        };
        let mechanisms = self.mechanisms.as_ref().map_or(0, |l| l.mechanisms.len());
        Json::Object(
            [
                ("enforces_identity", Json::Bool(self.enforces_identity())),
                ("transport_secured", Json::Bool(self.transport_secured())),
                ("enforcement_point", Json::String(self.enforcement_point().as_str().to_owned())),
                ("unenforced_because", why),
                ("mechanisms", Json::Number(mechanisms.to_string())),
                ("mechanism_list_unreadable", Json::Bool(self.mechanisms_unreadable)),
                ("transport_unreadable", Json::Bool(self.transport_unreadable)),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        )
    }
}

/// Why the bridge would not assert an identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Impersonation is not enabled for this interface.
    NotPermitted {
        /// The repository id asked about.
        id: String,
    },
    /// The caller's credential has expired.
    Expired {
        /// The principal whose credential lapsed.
        principal: String,
    },
    /// The target does not accept the kind of identity we would assert.
    TokenTypeRefused {
        /// What we would have sent.
        offered: u32,
        /// What the target accepts.
        accepted: u32,
    },
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::NotPermitted { id } => write!(
                f,
                "impersonation is not enabled for {id}. It is default-deny and enabled per \
                 interface with a recorded decision, never inherited from the caller having \
                 been trusted enough to connect"
            ),
            Refused::Expired { principal } => write!(
                f,
                "the credential for {principal} has expired; a call must not proceed on an \
                 expired context"
            ),
            Refused::TokenTypeRefused { offered, accepted } => write!(
                f,
                "the target accepts identity token types {accepted:#x} and we would assert \
                 {offered:#x}"
            ),
        }
    }
}

impl std::error::Error for Refused {}

/// Per-interface delegation decisions.
///
/// Default-deny, like [`crate::policy::Exposure`], and for a sharper reason:
/// getting exposure wrong shows an agent something it should not see, while
/// getting this wrong makes a target act on an identity nobody authorised.
#[derive(Debug, Clone, Default)]
pub struct Delegation {
    /// Repository id to the reason it was permitted.
    permitted: BTreeMap<String, String>,
}

impl Delegation {
    /// Permits nothing.
    pub fn nothing() -> Self {
        Self::default()
    }

    /// Permits impersonation when calling `id`, with the reason on record.
    ///
    /// The reason is required rather than optional, and empty is refused: a
    /// decision with no recorded reason is indistinguishable from an accident
    /// six months later, and this is the decision where that matters most.
    pub fn permit(
        mut self,
        id: impl Into<String>,
        reason: impl Into<String>,
    ) -> Result<Self, &'static str> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err("permitting impersonation needs a recorded reason");
        }
        self.permitted.insert(id.into(), reason);
        Ok(self)
    }

    /// The recorded reason, if impersonation is permitted for `id`.
    pub fn reason(&self, id: &str) -> Option<&str> {
        self.permitted.get(id).map(String::as_str)
    }

    /// What to do about identity for a call to `id` on this target.
    ///
    /// `peer` is the target's own record, read off its IOR
    /// ([`PeerCapability::of_ior`]); a target that advertises nothing is
    /// [`PeerCapability::advertising_nothing`].
    pub fn decide(
        &self,
        id: &str,
        caller: &Caller,
        peer: &PeerCapability,
        now: SystemTime,
    ) -> Result<Assertion, Refused> {
        // Expiry is checked first and unconditionally. A lapsed credential is
        // not made acceptable by the target being unable to check it — §4.8's
        // fourth discomfort is that the two clocks disagree, and the safe
        // direction is to stop.
        if !caller.valid_at(now) {
            return Err(Refused::Expired { principal: caller.principal.clone() });
        }

        let Some(sas) = peer.identity_assertion() else {
            // Nothing to assert to. Not a refusal: the call proceeds, and the
            // bridge is on the hook for the authorization decision. The `why`
            // is the record's own, so the catalogue and this decision cannot
            // describe the same peer differently.
            let why = peer.unenforced().expect("no identity assertion means unenforced");
            return Ok(Assertion::RecordedOnly { why });
        };

        if self.reason(id).is_none() {
            return Err(Refused::NotPermitted { id: id.to_owned() });
        }

        let token = IdentityToken::PrincipalName(caller.principal.as_bytes().to_vec());
        if !sas.accepts(&token) {
            return Err(Refused::TokenTypeRefused {
                offered: token.token_type(),
                accepted: sas.supported_identity_types,
            });
        }
        Ok(Assertion::Assert(token))
    }
}

/// Builds the `SecurityAttributeService` context for a call.
///
/// `authenticate_as` is how the *bridge* authenticates itself, which is a
/// different question from whose behalf the call is on. Both travel in the same
/// message and mean different things (§4.8's first two rows).
pub fn service_context(
    assertion: &Assertion,
    authenticate_as: Option<&GssUpToken>,
    endian: orbweaver_cdr::Endian,
) -> Result<Option<ServiceContext>, orbweaver_giop::Error> {
    let identity_token = match assertion {
        Assertion::Assert(t) => t.clone(),
        // Nothing asserted means nothing asserted, on the wire too. Sending
        // ITTAnonymous instead would claim a caller who declined to be named,
        // which is a different and untrue statement.
        Assertion::RecordedOnly { .. } => IdentityToken::Absent,
    };
    let client_authentication_token = match authenticate_as {
        Some(t) => t.encode(endian)?,
        None => Vec::new(),
    };
    if matches!(identity_token, IdentityToken::Absent) && client_authentication_token.is_empty() {
        // An EstablishContext that establishes nothing is noise on every
        // message; a target that reads it learns exactly what its absence says.
        return Ok(None);
    }
    let body = csiv2::SasContextBody::Establish(csiv2::EstablishContext {
        client_context_id: 0,
        authorization_token: Vec::new(),
        identity_token,
        client_authentication_token,
    })
    .encode(endian)?;
    Ok(Some(ServiceContext { id: csiv2::SERVICE_ID_SAS, data: body }))
}

/// One line for the audit log.
///
/// §4.8: the entry records **which** principal was asserted, never the material
/// that asserted it. Taking a `&Caller` and an `&Assertion` rather than a
/// credential is what makes that structural — there is no argument here that
/// could carry a password.
pub fn audit_line(caller: &Caller, id: &str, operation: &str, assertion: &Assertion) -> String {
    let what = match assertion {
        Assertion::Assert(t) => format!("asserted {}", t.audit_name()),
        Assertion::RecordedOnly { why } => {
            format!("asserted nothing; target cannot enforce ({})", why.as_str())
        }
    };
    format!("caller={} target={id} operation={operation} {what}", caller.principal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_cdr::{Encoder, Endian};

    /// A peer whose IOR carries one mechanism accepting `identity_types`.
    fn advertising(identity_types: u32) -> PeerCapability {
        PeerCapability::of_components(&[TaggedComponent {
            tag: csiv2::TAG_CSI_SEC_MECH_LIST,
            data: mech_list(csiv2::options::IDENTITY_ASSERTION, identity_types, Endian::Big),
        }])
    }

    /// The body of a `TAG_CSI_SEC_MECH_LIST`: one mechanism, no transport, no
    /// client authentication, a SAS context with `sas_supports` and
    /// `identity_types`.
    fn mech_list(sas_supports: u16, identity_types: u32, endian: Endian) -> Vec<u8> {
        let mut e = Encoder::encapsulation(endian);
        e.put_bool(false);
        e.put_u32(1);
        e.put_u16(0);
        e.put_u32(csiv2::TAG_NULL_TAG);
        e.put_octet_seq(&[]);
        e.put_u16(0);
        e.put_u16(0);
        e.put_octet_seq(&[]);
        e.put_octet_seq(&[]);
        e.put_u16(sas_supports);
        e.put_u16(0);
        e.put_u32(0);
        e.put_u32(0);
        e.put_u32(identity_types);
        e.finish().unwrap()
    }

    fn nothing() -> PeerCapability {
        PeerCapability::advertising_nothing()
    }

    fn alice() -> Caller {
        Caller::new("alice@example.com").with_scope("accounts:read")
    }

    #[test]
    fn impersonation_is_refused_until_it_is_permitted() {
        let target = advertising(2);
        let now = SystemTime::now();
        assert_eq!(
            Delegation::nothing().decide("IDL:bank/Account:1.0", &alice(), &target, now),
            Err(Refused::NotPermitted { id: "IDL:bank/Account:1.0".into() })
        );

        let d = Delegation::nothing()
            .permit("IDL:bank/Account:1.0", "change ticket SEC-114, approved by the data owner")
            .unwrap();
        assert_eq!(
            d.decide("IDL:bank/Account:1.0", &alice(), &target, now),
            Ok(Assertion::Assert(IdentityToken::PrincipalName(b"alice@example.com".to_vec())))
        );
    }

    /// Permission for one interface must not become permission for another.
    #[test]
    fn permission_does_not_spread_to_a_neighbour() {
        let d = Delegation::nothing().permit("IDL:bank/Account:1.0", "ticket").unwrap();
        let target = advertising(2);
        assert!(d.decide("IDL:bank/Ledger:1.0", &alice(), &target, SystemTime::now()).is_err());
    }

    /// A decision nobody wrote a reason for is indistinguishable from an
    /// accident six months later.
    #[test]
    fn permitting_without_a_reason_is_refused() {
        assert!(Delegation::nothing().permit("IDL:m/I:1.0", "").is_err());
        assert!(Delegation::nothing().permit("IDL:m/I:1.0", "   ").is_err());
    }

    /// §4.8's second discomfort, named rather than dressed up — and named by
    /// the record, so the decision and the catalogue say the same thing.
    #[test]
    fn a_target_that_advertises_nothing_gets_nothing_asserted() {
        let peer = nothing();
        let outcome = Delegation::nothing()
            .decide("IDL:bank/Account:1.0", &alice(), &peer, SystemTime::now())
            .expect("the call still proceeds");
        let Assertion::RecordedOnly { why } = outcome else { panic!("{outcome:?}") };
        assert_eq!(why, Unenforced::NoMechanismList);
        assert_eq!(Some(why), peer.unenforced(), "the assertion is the record's own answer");
        assert!(why.reason().contains("only enforcement point"), "{}", why.reason());
        assert_eq!(peer.enforcement_point(), EnforcementPoint::BridgeOnly);
        assert!(!peer.enforces_identity());
        assert!(!peer.transport_secured());
    }

    /// And nothing asserted means nothing on the wire, not "anonymous" — which
    /// would claim a caller who declined to be named.
    #[test]
    fn recorded_only_does_not_send_an_anonymous_claim() {
        let ctx = service_context(
            &Assertion::RecordedOnly { why: Unenforced::NoMechanismList },
            None,
            Endian::Big,
        )
        .unwrap();
        assert!(ctx.is_none(), "a context that establishes nothing should not be sent");
    }

    /// The record, over every way an IOR can advertise — both byte orders for
    /// each tagged component's encapsulation, since a peer chooses its own.
    #[test]
    fn the_per_peer_record_reads_what_the_ior_advertises_in_both_byte_orders() {
        for endian in [Endian::Big, Endian::Little] {
            // A mechanism list with identity assertion: the target enforces.
            let enforcing = PeerCapability::of_components(&[TaggedComponent {
                tag: csiv2::TAG_CSI_SEC_MECH_LIST,
                data: mech_list(csiv2::options::IDENTITY_ASSERTION, 2, endian),
            }]);
            assert!(enforcing.enforces_identity(), "{endian:?}");
            assert_eq!(enforcing.unenforced(), None);
            assert_eq!(enforcing.enforcement_point(), EnforcementPoint::Target);
            assert!(!enforcing.transport_secured());
            assert!(enforcing.identity_sentence().starts_with("enforced by the target"));

            // A mechanism list without identity assertion: it authenticates the
            // bridge and still cannot say whose call it is.
            let bridge_auth_only = PeerCapability::of_components(&[TaggedComponent {
                tag: csiv2::TAG_CSI_SEC_MECH_LIST,
                data: mech_list(0, 0, endian),
            }]);
            assert_eq!(
                bridge_auth_only.unenforced(),
                Some(Unenforced::NoIdentityAssertion { mechanisms: 1 })
            );
            assert_eq!(bridge_auth_only.enforcement_point(), EnforcementPoint::BridgeOnly);

            // No list at all: the legacy baseline.
            let bare = PeerCapability::of_components(&[]);
            assert_eq!(bare.unenforced(), Some(Unenforced::NoMechanismList));
            assert_eq!(bare, nothing());
            assert!(bare.identity_sentence().contains("bridge is the only enforcement point"));
            assert!(bare.identity_sentence().contains("no CSIv2 mechanism list"));

            // TAG_SSL_SEC_TRANS: transport secured, identity still not enforced.
            let ssl = ssliop::SslComponent { target_supports: 6, target_requires: 6, port: 4443 }
                .encode(endian)
                .unwrap();
            let tls = PeerCapability::of_components(&[TaggedComponent {
                tag: ssliop::TAG_SSL_SEC_TRANS,
                data: ssl,
            }]);
            assert!(tls.transport_secured(), "{endian:?}");
            assert!(!tls.enforces_identity());
            assert!(tls.transport_sentence().contains("port 4443"), "{}", tls.transport_sentence());
            assert!(!tls.transport_sentence().contains("cleartext"));
        }
    }

    /// An advertisement that claims to be a mechanism list and does not parse
    /// is its own class, not silently "advertises nothing".
    #[test]
    fn an_unreadable_advertisement_is_named_not_treated_as_absent() {
        let peer = PeerCapability::of_components(&[
            TaggedComponent { tag: csiv2::TAG_CSI_SEC_MECH_LIST, data: vec![0x00, 0x01] },
            TaggedComponent { tag: ssliop::TAG_SSL_SEC_TRANS, data: vec![0x00] },
        ]);
        assert_eq!(peer.unenforced(), Some(Unenforced::MechanismListUnreadable));
        assert!(peer.mechanisms_unreadable && peer.transport_unreadable);
        assert!(!peer.transport_secured());
        assert!(peer.transport_sentence().contains("unreadable"));
        let json = peer.to_json();
        assert_eq!(json.get("enforcement_point").and_then(Json::as_str), Some("bridge only"));
        assert_eq!(
            json.get("unenforced_because").and_then(Json::as_str),
            Some("mechanism-list-unreadable")
        );
    }

    /// The record is read off an IOR the way the bridge holds one — its primary
    /// profile — and carries nothing dialable in its JSON.
    #[test]
    fn the_record_comes_off_an_ior_and_its_json_carries_nothing_dialable() {
        use orbweaver_giop::{IiopProfile, Version};
        let profile = IiopProfile {
            version: Version { major: 1, minor: 2 },
            host: "target.example.internal".into(),
            port: 2809,
            object_key: b"very-distinctive-object-key".to_vec(),
            components: vec![TaggedComponent {
                tag: csiv2::TAG_CSI_SEC_MECH_LIST,
                data: mech_list(csiv2::options::IDENTITY_ASSERTION, 2, Endian::Little),
            }],
        };
        let ior = Ior { type_id: "IDL:bank/Account:1.0".into(), profiles: vec![profile] };
        // Through the stringified form and back, as a file on disk would be.
        let ior = Ior::parse(&ior.to_stringified().unwrap()).unwrap();
        let peer = PeerCapability::of_ior(&ior);
        assert!(peer.enforces_identity());
        assert_eq!(peer.enforcement_point(), EnforcementPoint::Target);
        let text = peer.to_json().to_string();
        assert_eq!(peer.to_json().get("enforces_identity"), Some(&Json::Bool(true)));
        for needle in ["target.example.internal", "2809", "very-distinctive", "IOR:"] {
            assert!(!text.contains(needle), "{needle:?} in {text}");
        }
        // No profile at all advertises nothing.
        let nil = Ior { type_id: String::new(), profiles: vec![] };
        assert_eq!(PeerCapability::of_ior(&nil), nothing());
    }

    /// §4.8's fourth discomfort: the two clocks disagree, and a call must not
    /// proceed quietly on an expired context.
    #[test]
    fn an_expired_credential_stops_the_call_even_where_the_target_cannot_check() {
        let past = SystemTime::now() - std::time::Duration::from_secs(60);
        let expired = alice().expiring_at(past);
        let d = Delegation::nothing().permit("IDL:bank/Account:1.0", "ticket").unwrap();
        for peer in [advertising(2), nothing()] {
            assert_eq!(
                d.decide("IDL:bank/Account:1.0", &expired, &peer, SystemTime::now()),
                Err(Refused::Expired { principal: "alice@example.com".into() }),
                "expiry must be checked before anything else"
            );
        }
    }

    #[test]
    fn a_target_that_will_not_take_a_principal_name_says_so() {
        // Accepts only ITTX509CertChain.
        let target = advertising(4);
        let d = Delegation::nothing().permit("IDL:bank/Account:1.0", "ticket").unwrap();
        let e = d.decide("IDL:bank/Account:1.0", &alice(), &target, SystemTime::now()).unwrap_err();
        assert!(matches!(e, Refused::TokenTypeRefused { offered: 2, accepted: 4 }), "{e}");
    }

    #[test]
    fn an_asserted_identity_reaches_the_service_context() {
        let ctx = service_context(
            &Assertion::Assert(IdentityToken::PrincipalName(b"alice".to_vec())),
            None,
            Endian::Little,
        )
        .unwrap()
        .expect("a context");
        assert_eq!(ctx.id, csiv2::SERVICE_ID_SAS);
        let body = csiv2::SasContextBody::parse(&ctx.data).unwrap();
        let csiv2::SasContextBody::Establish(e) = body else { panic!("{body:?}") };
        assert_eq!(e.identity_token, IdentityToken::PrincipalName(b"alice".to_vec()));
    }

    /// The bridge's own credential and the caller's identity are different
    /// things and both travel.
    #[test]
    fn the_bridges_own_credential_travels_separately_from_the_asserted_identity() {
        let gssup = GssUpToken {
            username: b"orbweaver".to_vec(),
            password: b"s3cret".to_vec(),
            target_name: b"bank".to_vec(),
        };
        let ctx = service_context(
            &Assertion::Assert(IdentityToken::PrincipalName(b"alice".to_vec())),
            Some(&gssup),
            Endian::Big,
        )
        .unwrap()
        .expect("a context");
        let csiv2::SasContextBody::Establish(e) = csiv2::SasContextBody::parse(&ctx.data).unwrap()
        else {
            panic!()
        };
        assert_eq!(e.identity_token.audit_name(), "alice");
        let back = GssUpToken::decode(&e.client_authentication_token).unwrap();
        assert_eq!(back.username, b"orbweaver");
    }

    /// The audit rule, made structural: there is no argument here that could
    /// carry a password.
    #[test]
    fn an_audit_line_names_the_principal_and_can_carry_nothing_else() {
        let line = audit_line(
            &alice(),
            "IDL:bank/Account:1.0",
            "close",
            &Assertion::Assert(IdentityToken::PrincipalName(b"alice@example.com".to_vec())),
        );
        assert!(line.contains("caller=alice@example.com"));
        assert!(line.contains("operation=close"));
        assert!(line.contains("asserted alice@example.com"));

        let unenforced = audit_line(
            &alice(),
            "IDL:bank/Account:1.0",
            "close",
            &Assertion::RecordedOnly { why: Unenforced::NoMechanismList },
        );
        assert!(unenforced.contains("target cannot enforce (no-mechanism-list)"), "{unenforced}");
    }
}
