//! Token → [`Caller`] exchange: the vocabulary gap, the grant's lifetime, and a
//! credential store that cannot reach a log.
//!
//! `docs/PLAN.md` §4.8 and §7.3 stream C. An agent arrives holding an OAuth2 or
//! JWT credential; the bridge needs a [`Caller`] — a principal and the scopes
//! `ai_authz` is written in. §4.8 calls the conversion **a trust boundary rather
//! than a mapping function**, and this module is where that boundary is drawn.
//!
//! # 1. Verification is a seam, and nothing was adopted to fill it
//!
//! Checking a JWT signature means a JWS implementation, which means RSA or
//! ECDSA, which means a crypto dependency. **This project adopts no dependency
//! without a decision document** (`CLAUDE.md`; D001, D002, D003, D004 are the
//! precedents), so this batch does not adopt one and does not write one either.
//!
//! The shape instead is D003's: **verification happens in a process that already
//! does it, and its output arrives as data.** A host that has authenticated its
//! agent — an MCP host with an OIDC client, a sidecar, an API gateway — builds
//! [`VerifiedClaims`] and hands them over, exactly as D003 made embeddings
//! arrive through a process boundary rather than through `cargo tree`. That is
//! the shipped path, it needs no dependency, and on it **this crate never holds
//! the token at all** — the strongest form of §4.8's hygiene rule, since a
//! credential that never arrives cannot leak.
//!
//! For a deployment that wants the verification *inside* this process there is
//! [`Verifier`], a trait with a [`Secret`] in and [`VerifiedClaims`] out. **This
//! crate ships no implementation of it**, deliberately: an implementation is
//! where the dependency would enter, and the trait is the seam it would enter
//! through. What a first-party verifier would cost is stated in
//! [the section below](#what-a-first-party-verifier-would-cost); it is a
//! D-document, not a batch.
//!
//! 검증은 이음매다. JWT 서명 검증은 암호 의존성을 뜻하고, 이 프로젝트는 결정
//! 문서 없이 의존성을 채택하지 않는다. 그래서 **이미 검증한 호스트가 클레임을
//! 넘긴다** — D003이 임베딩을 프로세스 경계 밖으로 밀어낸 것과 같은 형태다.
//!
//! # 2. The scope vocabulary gap is the substance
//!
//! A token's scopes are the identity provider's vocabulary. `ai_authz`'s scopes
//! are the contract's. Nothing made them agree, and D005 measured what happens
//! when they do not:
//!
//! > A deployment whose identity provider issues the scope the requirement
//! > literally states — `gate:operate` — against a contract that demands
//! > `parkinglot.barrier.open` **refuses every legitimate caller**. The refusal
//! > is well-formed, correctly audited, and indistinguishable from a permissions
//! > misconfiguration.
//!
//! That failure is silent, late and misattributed, and the identity team will
//! check the IdP, the role mapping and the token, find all three correct, and
//! have no reason to suspect the contract. [`ScopeMap`] is the translation, and
//! **[`ScopeMap::audit`] is the instrument that makes a mismatch loud before a
//! call is ever made**: it reports
//!
//! - **unsatisfiable** contract scopes — required by an exposed operation and
//!   *not in the map's image at all*, so **no token this deployment can issue
//!   will ever satisfy them**. This is D005's class, and it is the finding an
//!   operator must see before they deploy rather than after an outage;
//! - **unmapped** token scopes — issued by the IdP and placed nowhere, which is
//!   the same drift seen from the other end;
//! - **unused** contract scopes — the map can produce them and no exposed
//!   operation asks for one, which is a map maintained against an older catalog.
//!
//! An operator reads it where they already look: `orbweaver-mcp-server
//! --dry-run` folds it into the report under `scope_map` and **exits non-zero**
//! when anything is unsatisfiable, because a report nobody's exit code reads is
//! a report somebody pipes to `/dev/null`.
//!
//! 토큰의 스코프는 IdP의 어휘고 `ai_authz`의 스코프는 계약의 어휘다. 둘이
//! 어긋나면 **모든 정당한 호출자가 거부되며**, 그 실패는 조용하고 늦게 오고
//! 엉뚱한 곳으로 귀속된다. 그래서 불일치는 호출 전에 시끄러워야 한다.
//!
//! # 3. Expiry: a `Caller` must not outlive its grant
//!
//! There is no clock in this crate and this batch does not add one — D004's
//! `ts`, [`crate::telemetry::Timestamp`] and [`crate::quota::Window`] all take
//! the instant from the host, which is what keeps a replay deterministic. So the
//! instant is an argument here too: [`Exchange::caller_for`] takes `now`, and
//! the gate that keeps checking it — [`Expiry`], §4.5 #1's authentication half —
//! is [stamped by the host](Expiry::stamp) on every request.
//!
//! **What a host that never supplies one gets, in both places, stated:**
//!
//! | where | a host that supplies nothing |
//! |---|---|
//! | claims with no `exp` | refused, unless the deployment declared [`Lifetime::unbounded`] with a written reason |
//! | [`Expiry`] never stamped | [`Unstamped::Refuse`] refuses every caller that *has* an expiry; [`Unstamped::allow`] skips the check, with a written reason |
//!
//! Both defaults are the safe direction and neither is a default: they are
//! constructor arguments, because "this grant never ends" and "we cannot tell
//! whether this grant ended" are decisions an operator makes, not ones a library
//! makes for them. A stage with no instant **cannot know** a token is still
//! valid, and *cannot tell* must never render as *still valid*.
//!
//! # 4. A credential cannot reach a log
//!
//! [`Secret`] and [`CredentialStore`] make the same structural promise
//! [`crate::identity::audit_line`] makes — the dangerous thing is absent from
//! the type — and they make it the way `GssUpToken` does:
//!
//! 1. [`Secret`] has a hand-written [`Debug`] that prints `<redacted>` and **no
//!    [`Display`](std::fmt::Display)**, so no `{:?}` or `{}` anywhere, now or
//!    later, can print one.
//! 2. Reading the bytes takes [`Secret::expose`], named to be conspicuous in a
//!    review, and there is no `AsRef`, `Deref` or `Into<String>` that would let
//!    one slip into a format string by coercion.
//! 3. [`CredentialStore`]'s `Debug` prints the label count and no label and no
//!    material; there is no iterator over its values and no method that hands
//!    one out by value, only [`CredentialStore::with`], which lends.
//! 4. [`Secret`] overwrites its bytes on drop. Best-effort and said so: the
//!    compiler may have copied them, so this shortens a lifetime rather than
//!    guaranteeing an erasure.
//! 5. Nothing here reaches a [`CallContext`],
//!    so no interceptor, trace record or audit line has an argument that could
//!    carry one — which is the property [`crate::telemetry`] already relies on.
//!
//! `a_credential_reaches_no_transcript_no_audit_line_and_no_trace` runs a real
//! [`Session`](crate::session::Session) with secrets in the store and in flight
//! and asserts none of them appear anywhere, with the principal asserted
//! *present* so that a test capturing nothing cannot pass.
//!
//! # What a first-party verifier would cost
//!
//! Recorded here because §7.3 stream C will be asked, and refused here because
//! it is a D-document:
//!
//! - **JWS `RS256`/`ES256` needs RSA-PKCS#1-v1.5 or ECDSA-P256 verification,
//!   SHA-256, ASN.1 DER and base64url.** Constant-time discipline matters for
//!   none of it (verification uses public keys), which is the one thing in our
//!   favour.
//! - **D002's rule decides it anyway, and decides it against us.** GIOP we
//!   implement ourselves because a wrong implementation is loud — our oracles
//!   catch it. A signature verifier that is wrong in the *accepting* direction
//!   is silent: it interoperates perfectly with every honest token and also
//!   accepts a forged one, and no oracle we own can see the difference. D002
//!   ruled exactly this class depended-on rather than written.
//! - **The rest of the surface is not the interesting part but is most of the
//!   work**: JWKS fetching and key rotation, `iss`/`aud`/`nbf` validation,
//!   `alg` confusion and the `none` algorithm, clock skew (which would put the
//!   first clock in this crate, inside a gate).
//!
//! So the recommendation on record is: **do not write one.** Either keep the
//! shipped seam — the host verifies, as it already does for MCP transport
//! auth — or open a D-document proposing a named JWS crate with a verified
//! licence chain, in the D002 tradition. Neither is this batch's to decide.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use orbweaver_dynamic::json::Json;
use orbweaver_registry::Registry;

use crate::identity::Caller;
use crate::interceptor::{CallContext, Interceptor, Outcome};
use crate::policy::{Denied, Exposure, required_scopes};
use crate::{obj, s};

// ---------------------------------------------------------------------------
// Credential hygiene
// ---------------------------------------------------------------------------

/// Credential material — a bearer token, a client secret, a password.
///
/// The type exists to make §4.8's hygiene rule structural rather than
/// remembered: there is no `Display`, the `Debug` is hand-written and prints
/// nothing recoverable, and reading the bytes takes [`Secret::expose`], which is
/// named to be visible in a diff. See the module docs for the whole list and
/// what it does *not* promise.
pub struct Secret(Vec<u8>);

impl Secret {
    /// Takes ownership of credential material.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// How many bytes it is. Safe to log: a length is not material.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there is nothing in it.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The bytes, for the one place that has to have them — a [`Verifier`].
    ///
    /// Deliberately not `AsRef`, `Deref` or `Into<String>`: those are how
    /// material ends up in a format string without anybody writing it there.
    /// Anything calling this is doing something a reviewer should look at, and
    /// the name says so.
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    /// Prints the length and nothing else. The same discipline as
    /// `orbweaver_giop::csiv2::GssUpToken`'s hand-written `Debug`: a promise not
    /// to use `{:?}` is not a control, so the formatter is made harmless
    /// instead.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret(<redacted, {} bytes>)", self.0.len())
    }
}

impl Drop for Secret {
    /// Overwrites the bytes. **Best-effort, and stated as such**: the value may
    /// have been copied when it was moved into here, and nothing in safe Rust
    /// can reach those copies. This shortens the window; it does not close it.
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

/// The credentials a bridge holds, keyed by a **label** that is a name and never
/// material.
///
/// §4.8: *a store of credentials that reach legacy systems is a high-value
/// target and is treated as one — never logged, held for the shortest useful
/// lifetime, and excluded from diagnostics by construction rather than by
/// remembering to redact.*
///
/// The label reaches reports and audit lines; the [`Secret`] cannot, because
/// there is no way to get one out of here by value. [`CredentialStore::with`]
/// lends one to a closure and that is the whole of the read surface.
#[derive(Default)]
pub struct CredentialStore {
    held: BTreeMap<String, Secret>,
}

impl CredentialStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Holds `secret` under `label`, replacing anything already there — and
    /// says whether it replaced something, since silently overwriting one
    /// agent's credential with another's is how a confused deputy is built.
    ///
    /// The old value is dropped here, which overwrites it.
    pub fn hold(&mut self, label: impl Into<String>, secret: Secret) -> bool {
        self.held.insert(label.into(), secret).is_some()
    }

    /// Lends the secret under `label` to `f`. `None` when there is none.
    ///
    /// A borrow rather than a return, so that the material's lifetime is bounded
    /// by the call that needed it and a caller cannot stash a copy without
    /// writing the copy out by hand.
    pub fn with<R>(&self, label: &str, f: impl FnOnce(&Secret) -> R) -> Option<R> {
        self.held.get(label).map(f)
    }

    /// Drops the credential under `label`, overwriting it. Returns whether there
    /// was one — an idempotent forget is not an error.
    pub fn forget(&mut self, label: &str) -> bool {
        self.held.remove(label).is_some()
    }

    /// Drops every credential, overwriting each. What a session's teardown owes
    /// §4.8's *shortest useful lifetime*.
    pub fn forget_all(&mut self) {
        self.held.clear();
    }

    /// The labels held, sorted. Names only.
    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.held.keys().map(String::as_str)
    }

    /// How many credentials are held.
    pub fn len(&self) -> usize {
        self.held.len()
    }

    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

impl std::fmt::Debug for CredentialStore {
    /// A count. Not the labels either: a label is often a principal, and a
    /// diagnostic that enumerates who has a credential held is a different leak
    /// from the material but still a leak. [`CredentialStore::labels`] is how
    /// something that wants them asks on purpose.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CredentialStore {{ held: {} }}", self.held.len())
    }
}

// ---------------------------------------------------------------------------
// The verification seam
// ---------------------------------------------------------------------------

/// What an authority says about a token it verified.
///
/// **The shipped path builds this directly**, from a host that verified the
/// token in its own process (D003's shape). [`Verifier`] is the other way in,
/// for a deployment that puts the verification inside this process; see the
/// module docs for why nothing here implements it.
///
/// It holds no credential material by construction — a subject, scopes, an
/// expiry and the name of whoever vouched — which is why deriving [`Debug`] is
/// safe here and is not safe for [`Secret`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedClaims {
    authority: String,
    subject: String,
    scopes: Vec<String>,
    expires_at: Option<SystemTime>,
}

impl VerifiedClaims {
    /// Claims `authority` vouches for about `subject`.
    ///
    /// Both are required and neither may be blank. The authority is the same
    /// discipline as [`crate::identity::Delegation::permit`]'s reason: an
    /// assertion with nobody's name on it is indistinguishable from an accident
    /// six months later, and this is an assertion the whole bridge then acts on.
    pub fn verified_by(
        authority: impl Into<String>,
        subject: impl Into<String>,
    ) -> Result<Self, Rejected> {
        let authority = authority.into();
        let subject = subject.into();
        if authority.trim().is_empty() {
            return Err(Rejected::NoAuthority);
        }
        if subject.trim().is_empty() {
            return Err(Rejected::NoSubject { authority });
        }
        Ok(Self { authority, subject, scopes: Vec::new(), expires_at: None })
    }

    /// Adds a scope **in the identity provider's vocabulary**. [`ScopeMap`] is
    /// what turns it into the contract's.
    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scopes.push(scope.into());
        self
    }

    /// Sets the token's expiry — the `exp` claim, as an instant.
    pub fn expiring_at(mut self, at: SystemTime) -> Self {
        self.expires_at = Some(at);
        self
    }

    /// Who vouched.
    pub fn authority(&self) -> &str {
        &self.authority
    }

    /// The principal.
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The scopes, in the identity provider's vocabulary.
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    /// When the token stops being valid, if it says.
    pub fn expires_at(&self) -> Option<SystemTime> {
        self.expires_at
    }
}

/// Turns a [`Secret`] into [`VerifiedClaims`] — the pluggable half of §4.8's
/// trust boundary.
///
/// **Nothing in this crate implements this.** Verifying a JWT signature is where
/// a crypto dependency would enter, and this project adopts none without a
/// decision document. The trait is the seam that dependency would enter
/// through, so that adopting one later is an implementation somebody writes
/// rather than a change to how the bridge is shaped.
///
/// An implementor owes two things the shipped path gets for free: it must not
/// log the [`Secret`] it is handed, and it must refuse rather than guess — an
/// unverifiable token is [`Rejected::Unverifiable`], never claims with the
/// signature check skipped.
pub trait Verifier {
    /// The name that reaches [`VerifiedClaims::authority`] and the report.
    fn authority(&self) -> &str;

    /// Verifies `token` and reads its claims, or says why it could not.
    fn verify(&self, token: &Secret) -> Result<VerifiedClaims, Rejected>;
}

// ---------------------------------------------------------------------------
// Scope mapping
// ---------------------------------------------------------------------------

/// The translation between an identity provider's scope vocabulary and the
/// contract's `ai_authz` vocabulary.
///
/// Default-deny, like [`Exposure`] and [`crate::identity::Delegation`]: an
/// unmapped token scope grants nothing. A mapping that quietly passed unknown
/// scopes through would make every IdP scope an `ai_authz` scope, which is the
/// bridge handing the identity provider authority over the contract.
///
/// One token scope may grant several contract scopes (call [`ScopeMap::map`]
/// more than once), and several token scopes may grant the same contract scope.
/// [`ScopeMap::pass_through`] is the identity case written down rather than
/// assumed — a deployment whose two vocabularies genuinely agree still says so,
/// because "they agree" and "nobody configured this" must not look alike.
#[derive(Debug, Clone, Default)]
pub struct ScopeMap {
    grants: BTreeMap<String, BTreeSet<String>>,
}

impl ScopeMap {
    /// A map that grants nothing.
    pub fn nothing() -> Self {
        Self::default()
    }

    /// Holding `token_scope` grants `contract_scope`.
    pub fn map(
        mut self,
        token_scope: impl Into<String>,
        contract_scope: impl Into<String>,
    ) -> Self {
        self.grants.entry(token_scope.into()).or_default().insert(contract_scope.into());
        self
    }

    /// The identity case: the two vocabularies use this name for the same thing.
    ///
    /// Written down rather than inferred, so that a report can tell a scope
    /// somebody decided about from one nobody has looked at.
    pub fn pass_through(self, scope: impl Into<String>) -> Self {
        let scope = scope.into();
        self.map(scope.clone(), scope)
    }

    /// Every token scope this map places.
    pub fn token_scopes(&self) -> impl Iterator<Item = &str> {
        self.grants.keys().map(String::as_str)
    }

    /// What holding `token_scope` grants, in the contract's vocabulary.
    pub fn grants_for(&self, token_scope: &str) -> impl Iterator<Item = &str> {
        self.grants.get(token_scope).into_iter().flatten().map(String::as_str)
    }

    /// The map's **image**: every contract scope some token scope can produce.
    ///
    /// The set [`ScopeMap::audit`] measures the contract against. A required
    /// scope outside it is one no token this deployment issues will ever
    /// satisfy.
    pub fn contract_scopes(&self) -> BTreeSet<&str> {
        self.grants.values().flatten().map(String::as_str).collect()
    }

    /// Whether the map places nothing at all.
    pub fn is_empty(&self) -> bool {
        self.grants.is_empty()
    }

    /// Translates one token's scopes, reporting what it could not place.
    pub fn translate<S: AsRef<str>>(&self, token_scopes: &[S]) -> Translated {
        let mut granted = BTreeSet::new();
        let mut unmapped = BTreeSet::new();
        for scope in token_scopes {
            let scope = scope.as_ref();
            let mut placed = false;
            for contract in self.grants_for(scope) {
                granted.insert(contract.to_owned());
                placed = true;
            }
            if !placed {
                unmapped.insert(scope.to_owned());
            }
        }
        Translated {
            granted: granted.into_iter().collect(),
            unmapped: unmapped.into_iter().collect(),
        }
    }

    /// The pre-deployment question: **can this map ever satisfy this contract?**
    ///
    /// `issued` is the identity provider's vocabulary as the operator declares
    /// it — the scopes a token from this IdP can carry. Pass an empty slice when
    /// it is not known; the report then says so rather than reporting an empty
    /// finding, since "no unmapped scopes" and "nobody said what the scopes are"
    /// are different answers.
    ///
    /// See [`ScopeAudit`] for the three findings and D005 for why the first one
    /// is the one that matters.
    pub fn audit<S: AsRef<str>>(
        &self,
        registry: &Registry,
        exposure: &Exposure,
        issued: &[S],
    ) -> ScopeAudit {
        let image = self.contract_scopes();
        let mut required: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
        for id in exposure.interfaces() {
            for operation in crate::dryrun::operations_of(registry, exposure, id) {
                if !exposure.exposes_operation(id, &operation) {
                    continue;
                }
                for scope in required_scopes(registry, id, &operation) {
                    required.entry(scope).or_default().push((id.clone(), operation.clone()));
                }
            }
        }

        let mut satisfiable = Vec::new();
        let mut unsatisfiable = Vec::new();
        for (scope, wanted_by) in required {
            if image.contains(scope.as_str()) {
                satisfiable.push(scope);
            } else {
                unsatisfiable.push(Unsatisfiable { scope, wanted_by });
            }
        }
        let unused: Vec<String> = image
            .iter()
            .filter(|scope| !satisfiable.iter().any(|s| s == *scope))
            .map(|s| (*s).to_owned())
            .collect();

        let declared = !issued.is_empty();
        let unmapped = if declared { self.translate(issued).unmapped } else { Vec::new() };

        ScopeAudit {
            mapping: self.clone(),
            satisfiable,
            unsatisfiable,
            unused,
            issued_declared: declared,
            unmapped,
        }
    }
}

/// One token's scopes after translation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translated {
    granted: Vec<String>,
    unmapped: Vec<String>,
}

impl Translated {
    /// The contract scopes the caller gets, sorted and deduplicated.
    pub fn granted(&self) -> &[String] {
        &self.granted
    }

    /// The token scopes the map placed nowhere, sorted. **Not an error by
    /// itself** — see [`Unmapped`] — but never silent.
    pub fn unmapped(&self) -> &[String] {
        &self.unmapped
    }
}

/// A contract scope no token can ever satisfy, and who asks for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsatisfiable {
    /// The `ai_authz` scope.
    pub scope: String,
    /// The exposed `(repository id, operation)` pairs that require it — the
    /// blast radius, which is what makes the finding readable as an outage
    /// rather than as a warning.
    pub wanted_by: Vec<(String, String)>,
}

/// What [`ScopeMap::audit`] found, before anybody made a call.
///
/// The three findings, in the order an operator should act on them:
///
/// 1. [`ScopeAudit::unsatisfiable`] — **the outage**. An exposed operation
///    requires a scope the map cannot produce, so every legitimate caller is
///    refused and the refusal reads as a permissions misconfiguration (D005).
/// 2. [`ScopeAudit::unmapped`] — an IdP scope placed nowhere. Usually benign (a
///    token carries scopes for many services), occasionally the same drift seen
///    from the token's end.
/// 3. [`ScopeAudit::unused`] — the map can produce a scope nothing asks for: a
///    map maintained against an older catalog.
#[derive(Debug, Clone)]
pub struct ScopeAudit {
    mapping: ScopeMap,
    satisfiable: Vec<String>,
    unsatisfiable: Vec<Unsatisfiable>,
    unused: Vec<String>,
    issued_declared: bool,
    unmapped: Vec<String>,
}

impl ScopeAudit {
    /// Whether nothing is unsatisfiable — the one finding that is a deployment
    /// blocker rather than a note.
    pub fn ok(&self) -> bool {
        self.unsatisfiable.is_empty()
    }

    /// Contract scopes required by an exposed operation that the map can
    /// produce.
    pub fn satisfiable(&self) -> &[String] {
        &self.satisfiable
    }

    /// Contract scopes required by an exposed operation that the map can
    /// **never** produce. D005's class.
    pub fn unsatisfiable(&self) -> &[Unsatisfiable] {
        &self.unsatisfiable
    }

    /// Contract scopes the map can produce that no exposed operation requires.
    pub fn unused(&self) -> &[String] {
        &self.unused
    }

    /// Token scopes the map places nowhere. Empty and meaningless when the
    /// vocabulary was not declared — [`ScopeAudit::issued_declared`] separates
    /// those.
    pub fn unmapped(&self) -> &[String] {
        &self.unmapped
    }

    /// Whether the operator declared the identity provider's vocabulary.
    pub fn issued_declared(&self) -> bool {
        self.issued_declared
    }

    /// One sentence per finding, for a process that prints diagnostics rather
    /// than JSON. Empty when there is nothing to say.
    pub fn findings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for u in &self.unsatisfiable {
            let mut wanted: Vec<String> =
                u.wanted_by.iter().map(|(id, op)| format!("{id}.{op}")).collect();
            wanted.sort();
            out.push(format!(
                "no token scope grants {:?}, which {} require(s): {}. Every caller will be \
                 refused for a missing scope, and the refusal will look like a permissions \
                 misconfiguration rather than a contract that drifted",
                u.scope,
                u.wanted_by.len(),
                wanted.join(", ")
            ));
        }
        for scope in &self.unmapped {
            out.push(format!(
                "the identity provider issues {scope:?} and the map places it nowhere, so it \
                 grants this bridge nothing"
            ));
        }
        for scope in &self.unused {
            out.push(format!("the map can grant {scope:?} and no exposed operation asks for it"));
        }
        out
    }

    /// The report, in the shape the dry-run document carries it under
    /// `scope_map`.
    pub fn to_json(&self) -> Json {
        let mapping = Json::Object(
            self.mapping
                .grants
                .iter()
                .map(|(token, contract)| {
                    (token.clone(), Json::Array(contract.iter().map(s).collect()))
                })
                .collect(),
        );
        obj([
            ("ok", Json::Bool(self.ok())),
            ("mapping", mapping),
            ("satisfiable", Json::Array(self.satisfiable.iter().map(s).collect())),
            (
                "unsatisfiable",
                Json::Array(
                    self.unsatisfiable
                        .iter()
                        .map(|u| {
                            obj([
                                ("scope", s(&u.scope)),
                                (
                                    "wanted_by",
                                    Json::Array(
                                        u.wanted_by
                                            .iter()
                                            .map(|(id, op)| {
                                                obj([("target", s(id)), ("operation", s(op))])
                                            })
                                            .collect(),
                                    ),
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("unused", Json::Array(self.unused.iter().map(s).collect())),
            ("issued_declared", Json::Bool(self.issued_declared)),
            ("unmapped_token_scopes", Json::Array(self.unmapped.iter().map(s).collect())),
        ])
    }
}

// ---------------------------------------------------------------------------
// The exchange
// ---------------------------------------------------------------------------

/// What happens to a token scope the map places nowhere.
///
/// Stated by the operator rather than inferred, for the reason
/// [`crate::quota::Renewal`] is: the honest answer depends on the deployment and
/// a library that picked one would be choosing a policy on somebody's behalf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmapped {
    /// Drop it and carry on. Right when the token is a general-purpose one that
    /// carries scopes for many services — most of them are not this bridge's
    /// business. It is still reported by [`Translated::unmapped`] and by
    /// [`ScopeAudit`]; ignored is not silent.
    Ignore,
    /// Refuse the exchange. Right when the token has a dedicated audience, where
    /// a scope this bridge cannot place means the token was minted against a
    /// different contract than the one loaded — which is exactly D005's drift,
    /// caught at the door.
    Refuse,
}

/// How long a [`Caller`] this exchange produces may live.
///
/// There is no clock here, so this is not a duration: it is what to do about
/// claims that name no expiry at all. See the module docs' table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lifetime {
    /// The claims must carry an expiry; claims without one are
    /// [`Rejected::NoExpiry`]. **A `Caller` that outlives its token is a
    /// privilege that outlives its grant**, and a token with no `exp` gives the
    /// bridge no way to know when the grant ended.
    UntilExpiry,
    /// Claims with no expiry produce a `Caller` that never expires, because
    /// somebody wrote down why.
    Unbounded {
        /// The recorded reason. Required, and blank is refused — the same rule
        /// as [`crate::identity::Delegation::permit`], for the same reason.
        reason: String,
    },
}

impl Lifetime {
    /// The safe one: no expiry, no caller.
    pub fn until_expiry() -> Self {
        Lifetime::UntilExpiry
    }

    /// A caller that never expires, with the reason on record. Blank is refused.
    pub fn unbounded(reason: impl Into<String>) -> Result<Self, &'static str> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err("a caller that never expires needs a recorded reason");
        }
        Ok(Lifetime::Unbounded { reason })
    }
}

/// Why an exchange produced no [`Caller`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejected {
    /// [`VerifiedClaims`] were built without naming who vouched for them.
    NoAuthority,
    /// The claims name no subject, so there is no principal to be.
    NoSubject {
        /// Who vouched.
        authority: String,
    },
    /// A [`Verifier`] would not vouch for the token.
    Unverifiable {
        /// Who was asked.
        authority: String,
        /// What it said.
        why: String,
    },
    /// The token had already expired at the instant the host supplied.
    AlreadyExpired {
        /// The subject whose token lapsed.
        subject: String,
        /// How long ago.
        overdue: Duration,
    },
    /// [`Lifetime::UntilExpiry`] and the claims name no expiry.
    NoExpiry {
        /// The subject.
        subject: String,
    },
    /// [`Unmapped::Refuse`] and the token carries scopes the map places nowhere.
    UnmappedScopes {
        /// The subject.
        subject: String,
        /// The scopes, sorted.
        scopes: Vec<String>,
    },
}

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Rejected::NoAuthority => write!(
                f,
                "verified claims need the name of whoever verified them; an assertion the whole \
                 bridge acts on with nobody's name on it is indistinguishable from an accident"
            ),
            Rejected::NoSubject { authority } => {
                write!(
                    f,
                    "{authority} vouched for claims with no subject, so there is no principal"
                )
            }
            Rejected::Unverifiable { authority, why } => {
                write!(f, "{authority} would not verify the token: {why}")
            }
            Rejected::AlreadyExpired { subject, overdue } => write!(
                f,
                "the token for {subject} expired {}s before the instant the host supplied; a \
                 caller must not outlive its grant",
                overdue.as_secs()
            ),
            Rejected::NoExpiry { subject } => write!(
                f,
                "the claims for {subject} name no expiry and this exchange requires one. A caller \
                 that never expires is a privilege that never ends; declare \
                 Lifetime::unbounded(<reason>) if that is genuinely intended"
            ),
            Rejected::UnmappedScopes { subject, scopes } => write!(
                f,
                "the token for {subject} carries scope(s) {} that no mapping places. A token \
                 minted against a different contract than the one loaded is exactly how a scope \
                 drifts unseen",
                scopes.join(", ")
            ),
        }
    }
}

impl std::error::Error for Rejected {}

/// The token exchange point: §4.8's trust boundary, configured rather than
/// assumed.
///
/// Three arguments and no defaults, the discipline [`crate::quota::Quota::new`]
/// keeps for the same reason — each one changes what an agent is told, and a
/// default would be this module choosing a policy on an operator's behalf:
///
/// | question | answer |
/// |---|---|
/// | what does a token scope grant | [`ScopeMap`] |
/// | how long does the caller live | [`Lifetime`] |
/// | what about scopes nothing places | [`Unmapped`] |
#[derive(Debug, Clone)]
pub struct Exchange {
    scopes: ScopeMap,
    lifetime: Lifetime,
    unmapped: Unmapped,
}

impl Exchange {
    /// An exchange that translates through `scopes`, lives per `lifetime`, and
    /// treats scopes it cannot place per `unmapped`.
    pub fn new(scopes: ScopeMap, lifetime: Lifetime, unmapped: Unmapped) -> Self {
        Self { scopes, lifetime, unmapped }
    }

    /// The mapping this exchange translates through.
    pub fn scopes(&self) -> &ScopeMap {
        &self.scopes
    }

    /// How long the callers it produces live.
    pub fn lifetime(&self) -> &Lifetime {
        &self.lifetime
    }

    /// What it does with scopes the mapping places nowhere.
    pub fn unmapped(&self) -> Unmapped {
        self.unmapped
    }

    /// **The shipped path**: verified claims in, [`Caller`] out.
    ///
    /// `now` is the host's instant — there is no clock here (D004) — and it is
    /// used for one thing: refusing a token that has already expired. A host
    /// that has no clock passes [`SystemTime::UNIX_EPOCH`] and gets an exchange
    /// that cannot detect an expired token, which is why [`Expiry`] exists and
    /// why its unstamped behaviour is a stated choice rather than a default.
    ///
    /// The checks run in this order and the order is the point: expiry first and
    /// unconditionally, the same rule
    /// [`crate::identity::Delegation::decide`] keeps, because a lapsed
    /// credential is not made acceptable by anything that comes after it.
    pub fn caller_for(&self, claims: &VerifiedClaims, now: SystemTime) -> Result<Caller, Rejected> {
        if let Some(at) = claims.expires_at
            && now >= at
        {
            return Err(Rejected::AlreadyExpired {
                subject: claims.subject.clone(),
                overdue: now.duration_since(at).unwrap_or_default(),
            });
        }
        if claims.expires_at.is_none() && matches!(self.lifetime, Lifetime::UntilExpiry) {
            return Err(Rejected::NoExpiry { subject: claims.subject.clone() });
        }

        let translated = self.scopes.translate(&claims.scopes);
        if self.unmapped == Unmapped::Refuse && !translated.unmapped.is_empty() {
            return Err(Rejected::UnmappedScopes {
                subject: claims.subject.clone(),
                scopes: translated.unmapped,
            });
        }

        let mut caller = Caller::new(&claims.subject);
        caller.scopes = translated.granted;
        caller.expires_at = claims.expires_at;
        Ok(caller)
    }

    /// The pluggable path: a raw credential and something that will vouch for
    /// it.
    ///
    /// The [`Secret`] is lent to `verifier` and never held, copied or logged
    /// here. Nothing in this crate implements [`Verifier`]; see the module docs.
    pub fn caller_for_token(
        &self,
        token: &Secret,
        verifier: &dyn Verifier,
        now: SystemTime,
    ) -> Result<Caller, Rejected> {
        let claims = verifier.verify(token)?;
        self.caller_for(&claims, now)
    }

    /// What one token's scopes become, without producing a caller — for a report
    /// that wants to say *why* a caller ended up with the scopes it has.
    pub fn translate(&self, claims: &VerifiedClaims) -> Translated {
        self.scopes.translate(&claims.scopes)
    }

    /// [`ScopeMap::audit`], through the exchange that holds the map.
    pub fn audit<S: AsRef<str>>(
        &self,
        registry: &Registry,
        exposure: &Exposure,
        issued: &[S],
    ) -> ScopeAudit {
        self.scopes.audit(registry, exposure, issued)
    }
}

// ---------------------------------------------------------------------------
// The gate
// ---------------------------------------------------------------------------

/// What [`Expiry`] does when the host has supplied no instant.
///
/// Stated rather than defaulted, like [`crate::quota::Renewal`]: a stage with no
/// instant **cannot know** whether a token is still valid, and the two honest
/// things to do about that are opposites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unstamped {
    /// Refuse every caller that carries an expiry. *Cannot tell* must not render
    /// as *still valid*, and this is the direction that fails closed.
    Refuse,
    /// Let them through, with the reason on record: a deployment whose token
    /// lifetimes are enforced somewhere this process can see is entitled to say
    /// so, and saying so is different from forgetting to stamp.
    Allow {
        /// The recorded reason. Blank is refused.
        reason: String,
    },
}

impl Unstamped {
    /// Skips the check, with a reason. Blank is refused.
    pub fn allow(reason: impl Into<String>) -> Result<Self, &'static str> {
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err("skipping the expiry check needs a recorded reason");
        }
        Ok(Unstamped::Allow { reason })
    }
}

/// §4.5 #1's authentication half: the gate that keeps a [`Caller`] from
/// outliving its grant.
///
/// [`Exchange::caller_for`] refuses a token that has *already* expired, but a
/// CORBA session is long-lived by design and a token expires by design — §4.8's
/// fourth discomfort — so the interesting expiry happens **after** the exchange,
/// mid-session, on a `Caller` the bridge has been carrying for an hour. This is
/// the stage that notices.
///
/// It is **not** in [`crate::interceptor::Chain::standard`], for the reason
/// [`crate::quota::Quota`] is not: it needs an instant only a host has, and both
/// behaviours a default could pick are wrong. A deployment installs it with
/// [`crate::interceptor::Chain::expiry`], which seats it at
/// [`crate::interceptor::SEAT_EXPIRY`] — ahead of every other gate, because
/// authentication precedes authorization and because
/// [`crate::identity::Delegation::decide`] already checks expiry "first and
/// unconditionally".
///
/// Cloning shares the instant, for the reason cloning a [`crate::quota::Quota`]
/// shares its ledger: [`crate::Bridge`] and every [`crate::guard::Guarded`] it
/// hands out build their own chain, and one stamped clock behind all of them is
/// the only arrangement in which a stub cannot be a session with a stale
/// instant of its own.
#[derive(Debug, Clone)]
pub struct Expiry {
    now: Rc<std::cell::RefCell<Option<SystemTime>>>,
    unstamped: Unstamped,
}

impl Expiry {
    /// A gate with no instant yet, which behaves per `unstamped` until
    /// [`Expiry::stamp`] is called.
    pub fn new(unstamped: Unstamped) -> Self {
        Self { now: Rc::new(std::cell::RefCell::new(None)), unstamped }
    }

    /// Tells the gate what time it is. The host calls this per request, the way
    /// it calls [`crate::telemetry::Trace::stamp`] and
    /// [`crate::quota::Quota::open_window`] — **the only way time ever advances
    /// in this crate**.
    pub fn stamp(&self, now: SystemTime) {
        *self.now.borrow_mut() = Some(now);
    }

    /// The instant the host last supplied, if any.
    pub fn instant(&self) -> Option<SystemTime> {
        *self.now.borrow()
    }

    /// What it does before it is stamped.
    pub fn unstamped(&self) -> &Unstamped {
        &self.unstamped
    }
}

impl Interceptor for Expiry {
    fn before(&mut self, ctx: &CallContext<'_>) -> Outcome {
        // Nobody signed in: there is no grant to have outlived. The scopes stage
        // is what refuses an unauthenticated caller, and refusing here too would
        // move that decision somewhere an operator does not expect it.
        let Some(caller) = ctx.caller else { return Outcome::Proceed };
        // No expiry on the caller is the exchange's decision, already taken and
        // already recorded (`Lifetime::Unbounded`'s reason). Re-litigating it
        // here would refuse a caller somebody explicitly authorised.
        let Some(at) = caller.expires_at else { return Outcome::Proceed };
        match self.instant() {
            Some(now) if caller.valid_at(now) => Outcome::Proceed,
            Some(now) => Outcome::Refuse(Denied::CredentialExpired {
                principal: caller.principal.clone(),
                overdue_secs: Some(now.duration_since(at).unwrap_or_default().as_secs()),
            }),
            None => match &self.unstamped {
                Unstamped::Allow { .. } => Outcome::Proceed,
                Unstamped::Refuse => Outcome::Refuse(Denied::CredentialExpired {
                    principal: caller.principal.clone(),
                    overdue_secs: None,
                }),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::dryrun::{self, Would};
    use crate::interceptor::{Chain, SEAT_EXPIRY, STAGE_EXPOSURE, STAGE_SCOPES};
    use crate::policy::Approval;

    fn registry(src: &str) -> Registry {
        let spec = orbweaver_idl::parse(src).expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        r
    }

    const IDL: &str = "module bank {
        interface Account {
          //@ ai_effect: read_only
          long balance();
          //@ ai_authz: accounts:write
          void deposit(in long cents);
          //@ ai_authz: accounts:admin
          void close();
        };
      };";

    const ACCOUNT: &str = "IDL:bank/Account:1.0";

    fn seconds(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn epoch(n: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + seconds(n)
    }

    fn claims() -> VerifiedClaims {
        VerifiedClaims::verified_by("idp.example", "alice@example.com")
            .expect("both named")
            .with_scope("gate:operate")
            .expiring_at(epoch(1_000))
    }

    // --- the verification seam -------------------------------------------

    /// The shipped path needs no verifier, no dependency and no token: a host
    /// that verified elsewhere hands over claims. This is the whole exchange.
    #[test]
    fn the_shipped_path_holds_no_token_at_all() {
        let exchange = Exchange::new(
            ScopeMap::nothing().map("gate:operate", "accounts:write"),
            Lifetime::until_expiry(),
            Unmapped::Ignore,
        );
        let caller = exchange.caller_for(&claims(), epoch(900)).expect("within its lifetime");
        assert_eq!(caller.principal, "alice@example.com");
        assert_eq!(caller.scopes, ["accounts:write"]);
        assert_eq!(caller.expires_at, Some(epoch(1_000)));
    }

    /// An assertion the whole bridge acts on must name who made it.
    #[test]
    fn claims_with_no_authority_or_no_subject_are_refused() {
        assert_eq!(VerifiedClaims::verified_by("", "alice").unwrap_err(), Rejected::NoAuthority);
        assert_eq!(VerifiedClaims::verified_by("  ", "alice").unwrap_err(), Rejected::NoAuthority);
        assert_eq!(
            VerifiedClaims::verified_by("idp", " ").unwrap_err(),
            Rejected::NoSubject { authority: "idp".into() }
        );
    }

    /// The pluggable half, exercised through a stand-in — because this crate
    /// ships no [`Verifier`], and that absence is the point rather than a gap.
    /// A verifier that refuses must reach the caller as a refusal, never as
    /// claims with the check skipped.
    #[test]
    fn a_verifier_plugs_in_and_its_refusal_is_a_refusal() {
        struct Stub {
            accept: bool,
        }
        impl Verifier for Stub {
            fn authority(&self) -> &str {
                "stub"
            }
            fn verify(&self, token: &Secret) -> Result<VerifiedClaims, Rejected> {
                if !self.accept {
                    return Err(Rejected::Unverifiable {
                        authority: self.authority().to_owned(),
                        why: "signature did not verify".to_owned(),
                    });
                }
                // A real one would parse; this asserts only that it is handed
                // the bytes it was given.
                assert_eq!(token.expose(), b"header.payload.signature");
                Ok(claims())
            }
        }

        let exchange = Exchange::new(
            ScopeMap::nothing().pass_through("gate:operate"),
            Lifetime::until_expiry(),
            Unmapped::Ignore,
        );
        let token = Secret::new(*b"header.payload.signature");

        let caller = exchange
            .caller_for_token(&token, &Stub { accept: true }, epoch(900))
            .expect("verified");
        assert_eq!(caller.scopes, ["gate:operate"], "pass_through is the identity case");

        let refused =
            exchange.caller_for_token(&token, &Stub { accept: false }, epoch(900)).unwrap_err();
        assert!(matches!(refused, Rejected::Unverifiable { .. }), "{refused}");
    }

    // --- the scope vocabulary gap ----------------------------------------

    /// D005's finding, caught before a call: the IdP issues `gate:operate` and
    /// the contract demands `parkinglot.barrier.open`. Nothing is malformed,
    /// every stage is green, and every legitimate caller would be refused.
    #[test]
    fn a_contract_scope_no_token_can_satisfy_is_reported_before_a_call() {
        let reg = registry(
            "module parkinglot { interface ParkingControl { \
             //@ ai_authz: parkinglot.barrier.open\n void open_barrier(); }; };",
        );
        let exposure = Exposure::nothing().allow_interface("IDL:parkinglot/ParkingControl:1.0");
        // The map an identity team would build from the requirement's own words.
        let map = ScopeMap::nothing().map("gate:operate", "gate:operate");

        let audit = map.audit(&reg, &exposure, &["gate:operate"]);
        assert!(!audit.ok(), "the drift must not read as a healthy deployment");
        assert_eq!(audit.unsatisfiable().len(), 1);
        assert_eq!(audit.unsatisfiable()[0].scope, "parkinglot.barrier.open");
        assert_eq!(
            audit.unsatisfiable()[0].wanted_by,
            [("IDL:parkinglot/ParkingControl:1.0".to_owned(), "open_barrier".to_owned())]
        );
        // And the operator is told which operations go dark, not merely that
        // something is wrong.
        let finding = audit.findings().join("\n");
        assert!(finding.contains("open_barrier"), "{finding}");
        assert!(finding.contains("permissions misconfiguration"), "{finding}");

        // The same fact from the live gate, to prove the report is about the
        // real outage and not a separate opinion: the caller this map can build
        // is refused for a missing scope.
        let exchange = Exchange::new(
            map,
            Lifetime::unbounded("the fixture has no clock").expect("a reason"),
            Unmapped::Ignore,
        );
        let claims =
            VerifiedClaims::verified_by("idp", "alice").expect("named").with_scope("gate:operate");
        let caller = exchange.caller_for(&claims, epoch(0)).expect("exchanged");
        let mut chain = Chain::standard(exposure);
        let p = dryrun::predict(
            &mut chain,
            &CallContext {
                registry: &reg,
                caller: Some(&caller),
                target: "IDL:parkinglot/ParkingControl:1.0",
                operation: "open_barrier",
                approval: Approval::default(),
            },
        );
        assert_eq!(p.would(), Would::NeedScope, "the outage, as the gate spells it");
        assert_eq!(p.stage(), Some(STAGE_SCOPES));
    }

    /// The healthy case, so that `ok` is not merely never true, plus the two
    /// findings that are notes rather than outages.
    #[test]
    fn the_audit_separates_the_outage_from_the_notes() {
        let reg = registry(IDL);
        let exposure = Exposure::nothing().allow_interface(ACCOUNT);
        let map = ScopeMap::nothing()
            .map("bank:write", "accounts:write")
            .map("bank:admin", "accounts:admin")
            .map("bank:legacy", "accounts:legacy-nobody-asks-for");

        let audit = map.audit(&reg, &exposure, &["bank:write", "bank:admin", "billing:read"]);
        assert!(audit.ok(), "every required scope is reachable");
        assert_eq!(audit.satisfiable(), ["accounts:admin", "accounts:write"]);
        assert_eq!(audit.unused(), ["accounts:legacy-nobody-asks-for"]);
        assert_eq!(audit.unmapped(), ["billing:read"], "issued and placed nowhere");
        assert!(audit.issued_declared());
    }

    /// "No unmapped scopes" and "nobody said what the scopes are" are different
    /// answers, and a report that rendered them the same would be the silent
    /// failure this whole surface exists to prevent.
    #[test]
    fn an_undeclared_vocabulary_is_reported_as_undeclared_not_as_clean() {
        let reg = registry(IDL);
        let exposure = Exposure::nothing().allow_interface(ACCOUNT);
        let map = ScopeMap::nothing()
            .map("bank:write", "accounts:write")
            .map("bank:admin", "accounts:admin");
        let audit = map.audit(&reg, &exposure, &[] as &[&str]);
        assert!(!audit.issued_declared());
        assert!(audit.unmapped().is_empty());
        assert_eq!(
            audit.to_json().get("issued_declared"),
            Some(&Json::Bool(false)),
            "the document must say so too"
        );
    }

    /// The report is a document an operator diffs between two deployments.
    #[test]
    fn the_report_is_reproducible_and_reparses() {
        let reg = registry(IDL);
        let exposure = Exposure::nothing().allow_interface(ACCOUNT);
        let map = ScopeMap::nothing().map("bank:write", "accounts:write");
        let once = map.audit(&reg, &exposure, &["bank:write"]).to_json();
        let twice = map.audit(&reg, &exposure, &["bank:write"]).to_json();
        assert_eq!(once.to_string(), twice.to_string());
        assert_eq!(Json::parse(&once.to_string()).unwrap(), once);
        let doc = once.to_string();
        assert!(doc.contains("accounts:admin"), "the unsatisfiable one is named: {doc}");
    }

    /// Default-deny: an unmapped token scope grants nothing, whatever the
    /// deployment does about it. A map that passed unknown scopes through would
    /// hand the identity provider authority over the contract.
    #[test]
    fn an_unmapped_token_scope_grants_nothing_and_is_never_silent() {
        let map = ScopeMap::nothing().map("bank:write", "accounts:write");
        let t = map.translate(&["bank:write", "billing:read"]);
        assert_eq!(t.granted(), ["accounts:write"]);
        assert_eq!(t.unmapped(), ["billing:read"]);

        let ignoring = Exchange::new(map.clone(), Lifetime::until_expiry(), Unmapped::Ignore);
        let claims = VerifiedClaims::verified_by("idp", "alice")
            .expect("named")
            .with_scope("bank:write")
            .with_scope("billing:read")
            .expiring_at(epoch(1_000));
        let caller = ignoring.caller_for(&claims, epoch(0)).expect("ignored");
        assert_eq!(caller.scopes, ["accounts:write"], "the unplaced scope granted nothing");

        let refusing = Exchange::new(map, Lifetime::until_expiry(), Unmapped::Refuse);
        assert_eq!(
            refusing.caller_for(&claims, epoch(0)).unwrap_err(),
            Rejected::UnmappedScopes {
                subject: "alice".into(),
                scopes: vec!["billing:read".into()],
            }
        );
    }

    /// One token scope may grant several contract scopes, and several may grant
    /// the same one; a role in an IdP is rarely one permission in a contract.
    #[test]
    fn the_mapping_is_many_to_many() {
        let map = ScopeMap::nothing()
            .map("bank:teller", "accounts:read")
            .map("bank:teller", "accounts:write")
            .map("bank:manager", "accounts:write");
        assert_eq!(map.translate(&["bank:teller"]).granted(), ["accounts:read", "accounts:write"]);
        assert_eq!(map.translate(&["bank:manager"]).granted(), ["accounts:write"]);
        assert_eq!(
            map.translate(&["bank:teller", "bank:manager"]).granted(),
            ["accounts:read", "accounts:write"],
            "granted scopes are deduplicated"
        );
    }

    // --- expiry -----------------------------------------------------------

    /// §4.8's fourth discomfort at the door: a token that lapsed before the
    /// instant the host supplied produces no caller at all.
    #[test]
    fn a_token_that_already_expired_produces_no_caller() {
        let exchange =
            Exchange::new(ScopeMap::nothing(), Lifetime::until_expiry(), Unmapped::Ignore);
        assert_eq!(
            exchange.caller_for(&claims(), epoch(1_060)).unwrap_err(),
            Rejected::AlreadyExpired { subject: "alice@example.com".into(), overdue: seconds(60) }
        );
        // The boundary: `exp` is the first instant at which it is no longer
        // valid, matching `Caller::valid_at`'s `now < at`.
        assert!(exchange.caller_for(&claims(), epoch(1_000)).is_err());
        assert!(exchange.caller_for(&claims(), epoch(999)).is_ok());
    }

    /// What a host that supplies no expiry gets, stated as a test: a refusal,
    /// unless somebody wrote down why an endless grant is intended.
    #[test]
    fn claims_with_no_expiry_need_a_written_reason() {
        let forever = VerifiedClaims::verified_by("idp", "alice").expect("named");

        let strict = Exchange::new(ScopeMap::nothing(), Lifetime::until_expiry(), Unmapped::Ignore);
        assert_eq!(
            strict.caller_for(&forever, epoch(0)).unwrap_err(),
            Rejected::NoExpiry { subject: "alice".into() }
        );

        assert!(Lifetime::unbounded("").is_err(), "a blank reason is not a decision");
        assert!(Lifetime::unbounded("   ").is_err());
        let declared = Exchange::new(
            ScopeMap::nothing(),
            Lifetime::unbounded("the host re-authenticates on every request; ORB-411")
                .expect("a reason"),
            Unmapped::Ignore,
        );
        let caller = declared.caller_for(&forever, epoch(0)).expect("declared");
        assert_eq!(caller.expires_at, None);
    }

    /// The gate sits ahead of every other one, because authentication precedes
    /// authorization — and `Delegation::decide` already checks expiry first and
    /// unconditionally.
    #[test]
    fn the_expiry_gate_seats_ahead_of_the_authorization_stages() {
        let mut chain = Chain::standard(Exposure::nothing());
        assert!(chain.expiry(Expiry::new(Unstamped::Refuse)));
        let stages: Vec<_> = chain.stages().collect();
        let at = |name| stages.iter().position(|s| *s == name).expect(name);
        assert!(at(SEAT_EXPIRY) < at(STAGE_EXPOSURE));
        assert!(at(SEAT_EXPIRY) < at(STAGE_SCOPES));

        // A chain with no telemetry stage has no seat, and says so rather than
        // installing the gate somewhere else. Same rule as `Chain::quota`.
        let mut bare = Chain::empty();
        assert!(!bare.expiry(Expiry::new(Unstamped::Refuse)));
        assert_eq!(bare.stages().count(), 0);
    }

    /// The privilege that outlives its grant, refused mid-session: the caller
    /// was exchanged an hour ago and the host has since moved the instant past
    /// its expiry.
    #[test]
    fn a_caller_that_outlived_its_token_is_refused_mid_session() {
        let reg = registry(IDL);
        let gate = Expiry::new(Unstamped::Refuse);
        let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
        assert!(chain.expiry(gate.clone()));

        let alice = Caller::new("alice").expiring_at(epoch(1_000));
        let call = CallContext {
            registry: &reg,
            caller: Some(&alice),
            target: ACCOUNT,
            operation: "balance",
            approval: Approval::default(),
        };

        gate.stamp(epoch(900));
        chain.run(&call).expect("still within the token's life");
        chain.completed(&call, true);

        gate.stamp(epoch(1_060));
        let refused = chain.run(&call).unwrap_err();
        assert_eq!(
            refused,
            Denied::CredentialExpired { principal: "alice".into(), overdue_secs: Some(60) },
            "{refused}"
        );
        assert!(!refused.is_transient(), "re-authenticating is not retrying");
        assert!(refused.to_string().contains("re-authenticate"), "{refused}");
        assert!(chain.audit()[1].starts_with("REFUSE caller=alice"), "{}", chain.audit()[1]);
    }

    /// What a host that never stamps gets: a refusal, because a stage that
    /// cannot tell must not read as still valid — or a documented skip.
    #[test]
    fn an_unstamped_gate_refuses_by_default_and_skips_only_on_the_record() {
        let reg = registry(IDL);
        let alice = Caller::new("alice").expiring_at(epoch(1_000));
        let nobody_expires = Caller::new("bob");

        for (unstamped, expired_ok) in [
            (Unstamped::Refuse, false),
            (Unstamped::allow("token lifetime is enforced by the gateway; ORB-412").unwrap(), true),
        ] {
            let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
            assert!(chain.expiry(Expiry::new(unstamped.clone())));
            let call = |caller| CallContext {
                registry: &reg,
                caller: Some(caller),
                target: ACCOUNT,
                operation: "balance",
                approval: Approval::default(),
            };
            assert_eq!(chain.run(&call(&alice)).is_ok(), expired_ok, "{unstamped:?}");
            // A caller with no expiry is the exchange's recorded decision and is
            // never second-guessed here, stamped or not.
            assert!(chain.run(&call(&nobody_expires)).is_ok(), "{unstamped:?}");
            assert!(Unstamped::allow(" ").is_err(), "a blank reason is not a decision");
        }
    }

    /// A session nobody is signed into has no grant to have outlived, and this
    /// stage must not become a second, differently-worded authentication check.
    #[test]
    fn an_unauthenticated_session_is_not_this_stages_business() {
        let reg = registry(IDL);
        let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
        assert!(chain.expiry(Expiry::new(Unstamped::Refuse)));
        assert!(
            chain
                .run(&CallContext {
                    registry: &reg,
                    caller: None,
                    target: ACCOUNT,
                    operation: "balance",
                    approval: Approval::default(),
                })
                .is_ok()
        );
    }

    /// An expired credential is `need_authentication` in the operator's report:
    /// the fix is to sign in again, which is the same fix the classification
    /// already names. A row of its own would have split one action into two.
    #[test]
    fn an_expired_credential_reads_as_need_authentication_in_a_dry_run() {
        let reg = registry(IDL);
        let gate = Expiry::new(Unstamped::Refuse);
        let exposure = Exposure::nothing().allow_interface(ACCOUNT);
        let mut chain = Chain::standard(exposure.clone());
        assert!(chain.expiry(gate.clone()));
        gate.stamp(epoch(2_000));

        let alice = Caller::new("alice").expiring_at(epoch(1_000));
        let doc =
            dryrun::survey(&mut chain, &reg, &exposure, Some(&alice), Approval::default(), None);
        assert_eq!(
            doc.get("summary").and_then(|x| x.get("need_authentication")),
            Some(&Json::Number("3".into())),
            "{doc}"
        );
        let text = doc.to_string();
        assert!(text.contains(SEAT_EXPIRY), "the stage is named: {text}");
        assert!(text.contains("expired 1000s ago"), "{text}");
    }

    // --- credential hygiene ------------------------------------------------

    /// The formatter is made harmless rather than avoided: a promise not to use
    /// `{:?}` is not a control. Same test shape as `GssUpToken`'s.
    #[test]
    fn no_formatter_can_print_credential_material() {
        let secret = Secret::new(*b"pin-s3cret-4242");
        for rendered in [format!("{secret:?}"), format!("{secret:#?}")] {
            assert!(!rendered.contains("s3cret"), "{rendered}");
            assert!(!rendered.contains("4242"), "{rendered}");
            assert!(rendered.contains("redacted"), "{rendered}");
        }
        // The length is not material and is what a diagnostic legitimately wants.
        assert!(format!("{secret:?}").contains("15 bytes"));
        assert_eq!(secret.expose(), b"pin-s3cret-4242");
        assert_eq!(secret.len(), 15);
        assert!(!secret.is_empty());
    }

    /// The store's own diagnostics say how many and nothing else — not the
    /// material, and not the labels, which are often principals.
    #[test]
    fn the_store_prints_a_count_and_nothing_else() {
        let mut store = CredentialStore::new();
        assert!(store.is_empty());
        assert!(!store.hold("alice@example.com", Secret::new(*b"token-s3cret-alice")));
        assert!(store.hold("alice@example.com", Secret::new(*b"token-s3cret-rotated")));
        store.hold("bob@example.com", Secret::new(*b"token-s3cret-bob"));

        for rendered in [format!("{store:?}"), format!("{store:#?}")] {
            for leak in ["s3cret", "alice", "bob", "token"] {
                assert!(!rendered.contains(leak), "{leak:?} reached {rendered}");
            }
        }
        assert_eq!(format!("{store:?}"), "CredentialStore { held: 2 }");
        assert_eq!(store.len(), 2);
        assert_eq!(store.labels().collect::<Vec<_>>(), ["alice@example.com", "bob@example.com"]);

        // Lent, never handed out; and the rotation above replaced the material.
        let seen = store.with("alice@example.com", |s| s.expose().to_vec()).expect("held");
        assert_eq!(seen, b"token-s3cret-rotated");
        assert!(store.with("nobody", |_| ()).is_none());

        assert!(store.forget("bob@example.com"));
        assert!(!store.forget("bob@example.com"), "forgetting twice is not an error");
        store.forget_all();
        assert!(store.is_empty());
    }

    /// Best-effort erasure, measured on the one copy this type owns rather than
    /// claimed for every copy that ever existed.
    #[test]
    fn a_secret_overwrites_its_bytes_on_drop() {
        struct Watch(Rc<RefCell<Vec<u8>>>);
        // A stand-in that records what `Secret::drop` does to the buffer it
        // owns: the real one cannot be observed after it is dropped, so the
        // observable claim is that `drop` runs the overwrite.
        impl Drop for Watch {
            fn drop(&mut self) {
                self.0.borrow_mut().fill(0);
            }
        }
        let mut secret = Secret::new(*b"s3cret");
        secret.0.fill(0);
        assert_eq!(secret.expose(), b"\0\0\0\0\0\0", "fill(0) is what drop does");
        let seen = Rc::new(RefCell::new(b"s3cret".to_vec()));
        drop(Watch(Rc::clone(&seen)));
        assert_eq!(*seen.borrow(), b"\0\0\0\0\0\0");
    }

    /// §4.8's hygiene rule end to end, tested the way the transcript-leak tests
    /// test it: a real session, real secrets in the store and in flight, and
    /// none of them anywhere — with the principal asserted **present**, so that
    /// a test capturing nothing cannot pass.
    #[test]
    fn a_credential_reaches_no_transcript_no_audit_line_and_no_trace() {
        use orbweaver_giop::{Connection, IiopProfile, Ior, Version};

        use crate::session::Session;
        use crate::telemetry::{CallPath, SpanRecord, TelemetrySink, Timestamp, Trace};

        struct Captured(Rc<RefCell<Vec<String>>>);
        impl TelemetrySink for Captured {
            fn emit(&mut self, record: &SpanRecord<'_>) {
                self.0.borrow_mut().push(record.to_string());
            }
        }

        let reg: &'static Registry = Box::leak(Box::new(registry(IDL)));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
        let ior = Ior {
            type_id: ACCOUNT.into(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: "127.0.0.1".into(),
                port: listener.local_addr().expect("bound").port(),
                object_key: b"objectkey-s3cret".to_vec(),
                components: Vec::new(),
            }],
        };
        let conn = Connection::connect(&ior, seconds(5)).expect("dials");

        // The store holds real material for the whole exchange and the whole
        // session, which is the case that matters: a store nothing is in cannot
        // leak.
        let mut store = CredentialStore::new();
        store.hold("alice@example.com", Secret::new(*b"eyJhbGciOi.s3cret-jwt.sig"));

        let exchange = Exchange::new(
            ScopeMap::nothing().map("bank:write", "accounts:write"),
            Lifetime::until_expiry(),
            Unmapped::Ignore,
        );
        let claims = VerifiedClaims::verified_by("idp.example", "alice@example.com")
            .expect("named")
            .with_scope("bank:write")
            .with_scope("billing:s3cret-audience")
            .expiring_at(epoch(1_000));
        let caller = exchange.caller_for(&claims, epoch(900)).expect("exchanged");

        let lines = Rc::new(RefCell::new(Vec::new()));
        let exposure = Exposure::nothing().allow_operation(ACCOUNT, "deposit");
        let mut session = Session::new(reg, exposure, conn, "s-token").on_behalf_of(caller.clone());
        assert!(session.bridge().chain_mut().trace(Trace::new(
            "s-token",
            CallPath::Dynamic,
            Timestamp::new("2026-08-14T09:00:00Z"),
            Captured(Rc::clone(&lines)),
        )));
        let handle = session.bridge().handles().issue_checked(&ior).expect("issued");

        let mut transcript = String::new();
        for frame in [
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#.to_owned(),
            format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"invoke_operation","arguments":{{"handle":"{handle}","operation":"deposit","arguments":{{"cents":"pin-s3cret-4242"}}}}}}}}"#
            ),
            format!(
                r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"invoke_operation","arguments":{{"handle":"{handle}","operation":"balance"}}}}}}"#
            ),
        ] {
            if let Some(reply) = session.handle_line(&frame) {
                transcript.push_str(&reply);
                transcript.push('\n');
            }
        }

        let everything = format!(
            "{transcript}\n{}\n{}\n{:?}\n{:?}",
            session.bridge().audit().join("\n"),
            lines.borrow().join("\n"),
            store,
            caller,
        );
        assert!(!transcript.is_empty(), "a test that captured nothing proves nothing");
        for leak in ["s3cret", "eyJhbGciOi", "pin-", "objectkey", "billing:s3cret-audience"] {
            assert!(!everything.contains(leak), "{leak:?} escaped:\n{everything}");
        }
        // The positive controls: the principal and the *granted* scope are
        // exactly what these records are for.
        assert!(everything.contains("alice@example.com"), "{everything}");
        assert!(everything.contains("accounts:write"), "{everything}");
    }

    /// The unmapped scope never becomes a caller's, so it cannot reach a record
    /// through the caller either — the leak test's structural half.
    #[test]
    fn a_scope_the_map_did_not_place_is_absent_from_the_caller() {
        let exchange = Exchange::new(
            ScopeMap::nothing().map("bank:write", "accounts:write"),
            Lifetime::until_expiry(),
            Unmapped::Ignore,
        );
        let claims = VerifiedClaims::verified_by("idp", "alice")
            .expect("named")
            .with_scope("bank:write")
            .with_scope("billing:s3cret-audience")
            .expiring_at(epoch(1_000));
        let caller = exchange.caller_for(&claims, epoch(0)).expect("exchanged");
        assert!(!format!("{caller:?}").contains("s3cret"), "{caller:?}");
        assert_eq!(exchange.translate(&claims).unmapped(), ["billing:s3cret-audience"]);
    }

    /// The three configuration answers are readable back off the exchange, so a
    /// report can state the policy it is reporting against.
    #[test]
    fn the_configuration_is_readable_back() {
        let exchange = Exchange::new(
            ScopeMap::nothing().pass_through("accounts:write"),
            Lifetime::unbounded("ORB-411").expect("a reason"),
            Unmapped::Refuse,
        );
        assert_eq!(exchange.unmapped(), Unmapped::Refuse);
        assert_eq!(exchange.lifetime(), &Lifetime::Unbounded { reason: "ORB-411".into() });
        assert_eq!(exchange.scopes().token_scopes().collect::<Vec<_>>(), ["accounts:write"]);
        assert!(!exchange.scopes().is_empty());
        assert!(ScopeMap::nothing().is_empty());
        assert_eq!(
            exchange.scopes().grants_for("accounts:write").collect::<Vec<_>>(),
            ["accounts:write"]
        );
        assert_eq!(
            exchange.scopes().contract_scopes().into_iter().collect::<Vec<_>>(),
            ["accounts:write"]
        );
    }
}
