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

use orbweaver_giop::csiv2::{self, GssUpToken, IdentityToken, SecMechList};
use orbweaver_giop::{ServiceContext, TaggedComponent};

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
    /// the only enforcement point, and the catalogue has to say so.
    RecordedOnly {
        /// Why nothing is being asserted.
        reason: &'static str,
    },
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
    /// `advertised` is the target's own `TAG_CSI_SEC_MECH_LIST`, or `None` when
    /// it advertises nothing.
    pub fn decide(
        &self,
        id: &str,
        caller: &Caller,
        advertised: Option<&SecMechList>,
        now: SystemTime,
    ) -> Result<Assertion, Refused> {
        // Expiry is checked first and unconditionally. A lapsed credential is
        // not made acceptable by the target being unable to check it — §4.8's
        // fourth discomfort is that the two clocks disagree, and the safe
        // direction is to stop.
        if !caller.valid_at(now) {
            return Err(Refused::Expired { principal: caller.principal.clone() });
        }

        let Some(sas) = advertised.and_then(SecMechList::identity_assertion) else {
            // Nothing to assert to. Not a refusal: the call proceeds, and the
            // bridge is on the hook for the authorization decision.
            return Ok(Assertion::RecordedOnly {
                reason: "the target advertises no CSIv2 identity assertion, so it cannot \
                         enforce a caller identity and the bridge is the only enforcement point",
            });
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
        Assertion::RecordedOnly { .. } => "asserted nothing; target cannot enforce".to_owned(),
    };
    format!("caller={} target={id} operation={operation} {what}", caller.principal)
}

/// Reads a target's advertisement out of an IOR profile's components.
pub fn advertised_by(components: &[TaggedComponent]) -> Option<SecMechList> {
    csiv2::advertised(components)?.ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_cdr::{Encoder, Endian};

    fn advertising(identity_types: u32) -> SecMechList {
        let mut e = Encoder::encapsulation(Endian::Big);
        e.put_bool(false);
        e.put_u32(1);
        e.put_u16(0);
        e.put_u32(csiv2::TAG_NULL_TAG);
        e.put_octet_seq(&[]);
        e.put_u16(0);
        e.put_u16(0);
        e.put_octet_seq(&[]);
        e.put_octet_seq(&[]);
        e.put_u16(csiv2::options::IDENTITY_ASSERTION);
        e.put_u16(0);
        e.put_u32(0);
        e.put_u32(0);
        e.put_u32(identity_types);
        SecMechList::parse(&e.finish().unwrap()).expect("parses")
    }

    fn alice() -> Caller {
        Caller::new("alice@example.com").with_scope("accounts:read")
    }

    #[test]
    fn impersonation_is_refused_until_it_is_permitted() {
        let target = advertising(2);
        let now = SystemTime::now();
        assert_eq!(
            Delegation::nothing().decide("IDL:bank/Account:1.0", &alice(), Some(&target), now),
            Err(Refused::NotPermitted { id: "IDL:bank/Account:1.0".into() })
        );

        let d = Delegation::nothing()
            .permit("IDL:bank/Account:1.0", "change ticket SEC-114, approved by the data owner")
            .unwrap();
        assert_eq!(
            d.decide("IDL:bank/Account:1.0", &alice(), Some(&target), now),
            Ok(Assertion::Assert(IdentityToken::PrincipalName(b"alice@example.com".to_vec())))
        );
    }

    /// Permission for one interface must not become permission for another.
    #[test]
    fn permission_does_not_spread_to_a_neighbour() {
        let d = Delegation::nothing().permit("IDL:bank/Account:1.0", "ticket").unwrap();
        let target = advertising(2);
        assert!(
            d.decide("IDL:bank/Ledger:1.0", &alice(), Some(&target), SystemTime::now()).is_err()
        );
    }

    /// A decision nobody wrote a reason for is indistinguishable from an
    /// accident six months later.
    #[test]
    fn permitting_without_a_reason_is_refused() {
        assert!(Delegation::nothing().permit("IDL:m/I:1.0", "").is_err());
        assert!(Delegation::nothing().permit("IDL:m/I:1.0", "   ").is_err());
    }

    /// §4.8's second discomfort, named rather than dressed up.
    #[test]
    fn a_target_that_advertises_nothing_gets_nothing_asserted() {
        let outcome = Delegation::nothing()
            .decide("IDL:bank/Account:1.0", &alice(), None, SystemTime::now())
            .expect("the call still proceeds");
        let Assertion::RecordedOnly { reason } = outcome else { panic!("{outcome:?}") };
        assert!(reason.contains("only enforcement point"), "{reason}");
    }

    /// And nothing asserted means nothing on the wire, not "anonymous" — which
    /// would claim a caller who declined to be named.
    #[test]
    fn recorded_only_does_not_send_an_anonymous_claim() {
        let ctx =
            service_context(&Assertion::RecordedOnly { reason: "x" }, None, Endian::Big).unwrap();
        assert!(ctx.is_none(), "a context that establishes nothing should not be sent");
    }

    /// §4.8's fourth discomfort: the two clocks disagree, and a call must not
    /// proceed quietly on an expired context.
    #[test]
    fn an_expired_credential_stops_the_call_even_where_the_target_cannot_check() {
        let past = SystemTime::now() - std::time::Duration::from_secs(60);
        let expired = alice().expiring_at(past);
        let d = Delegation::nothing().permit("IDL:bank/Account:1.0", "ticket").unwrap();
        for advertised in [Some(&advertising(2)), None] {
            assert_eq!(
                d.decide("IDL:bank/Account:1.0", &expired, advertised, SystemTime::now()),
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
        let e = d
            .decide("IDL:bank/Account:1.0", &alice(), Some(&target), SystemTime::now())
            .unwrap_err();
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
            &Assertion::RecordedOnly { reason: "no mechanism" },
        );
        assert!(unenforced.contains("target cannot enforce"), "{unenforced}");
    }
}
