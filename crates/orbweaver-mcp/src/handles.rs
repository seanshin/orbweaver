//! Capability handles: the only form in which a reference crosses to an agent.
//!
//! `docs/PLAN.md` §4.7 states the problem plainly. An IOR is a **bearer
//! address** — host, port and object key, and nothing else. Anything holding
//! one and able to reach the network can call the target directly, which means
//! handing an agent an IOR hands it a way around authorisation, around
//! `destructive` approval, and around the audit log. The guard becomes
//! decorative.
//!
//! So a reference crossing the boundary becomes a name the bridge can look up
//! and the agent cannot dial. Four properties make that true, and each is
//! tested rather than asserted:
//!
//! 1. **Unguessable.** The token is 128 bits from the operating system's
//!    entropy source. Nothing about the target contributes to it, so no amount
//!    of collecting handles reveals an endpoint.
//! 2. **Session-scoped.** A handle issued to one session is not resolvable by
//!    another. Leaking a transcript therefore leaks nothing usable.
//! 3. **Expiring.** A handle outlives its usefulness by minutes, not for the
//!    life of the process.
//! 4. **Typed.** It carries the target's repository id, so an agent can reason
//!    about what it holds without being able to reach it.
//!
//! # Failing closed
//!
//! If the entropy source cannot be read, no handle is issued. The alternative —
//! falling back to a counter, or to the clock — would produce something that
//! looks like a capability and is not, which is worse than an outage because it
//! fails silently. `unsafe_code = "forbid"` rules out calling `getentropy`
//! directly, so the source is `/dev/urandom`; this is a Unix-only assumption
//! and it is stated rather than discovered.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Read;
use std::rc::Rc;
use std::time::{Duration, Instant};

use orbweaver_dynamic::anyjson::References;
use orbweaver_giop::Ior;

/// One session's table, held by everything that answers for that session.
///
/// A [`crate::Bridge`] owns its session's table, and a
/// [`crate::guard::Guarded`] it issues through `connect_static` needs the same
/// one — not a copy — so that a handle declared to the static path's dry run
/// resolves the way the dynamic path would resolve it, and a handle issued or
/// revoked after the guard was assembled is seen by both. A snapshot would
/// have been a second table to drift from the first, which is the shape
/// [`crate::dryrun`] refuses for the policy and refuses here for the same
/// reason. Shared the way [`crate::quota::Quota`] shares its ledger, and
/// crate-private: nothing outside hands a table to a guard, so the confused
/// deputy — one session's connection over another session's handles — stays
/// inexpressible.
pub(crate) type SharedTable = Rc<RefCell<CapabilityTable>>;

/// A [`SharedTable`] for a fresh session — the tests' and the bridge's one
/// constructor for one.
pub(crate) fn shared(session: impl Into<String>) -> SharedTable {
    Rc::new(RefCell::new(CapabilityTable::new(session)))
}

/// How long a handle stays usable.
///
/// Long enough for an agent to describe an interface and then call it, short
/// enough that a transcript captured after the fact is worthless.
pub const DEFAULT_TTL: Duration = Duration::from_secs(15 * 60);

/// How many references one session may hold at once.
///
/// A bound, because an agent in a loop that fetches references and never uses
/// them would otherwise grow the table without limit — the same
/// unbounded-allocation shape the Phase 0 audit found on the wire, arriving
/// through the front door instead.
pub const MAX_HANDLES_PER_SESSION: usize = 4096;

/// Why a handle could not be issued or resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleError {
    /// The operating system's entropy source could not be read.
    NoEntropy(String),
    /// This session already holds the maximum number of references.
    TooMany,
}

impl std::fmt::Display for HandleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandleError::NoEntropy(why) => write!(
                f,
                "cannot issue a capability handle: no entropy source ({why}). \
                 Refusing rather than issuing a guessable one"
            ),
            HandleError::TooMany => {
                write!(f, "this session already holds {MAX_HANDLES_PER_SESSION} references")
            }
        }
    }
}

impl std::error::Error for HandleError {}

/// An opaque, session-scoped name for an object reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Handle(String);

impl Handle {
    /// The token as it crosses the boundary.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone)]
struct Entry {
    ior: Ior,
    session: String,
    issued: Instant,
    /// The repository id the agent is told about.
    type_id: String,
}

/// The bridge's table of live handles.
#[derive(Debug)]
pub struct CapabilityTable {
    entries: BTreeMap<String, Entry>,
    session: String,
    ttl: Duration,
}

impl CapabilityTable {
    /// A table for one session.
    ///
    /// The session name is opaque here; the bridge decides what it means. It is
    /// compared, never parsed.
    pub fn new(session: impl Into<String>) -> Self {
        Self { entries: BTreeMap::new(), session: session.into(), ttl: DEFAULT_TTL }
    }

    /// Overrides the lifetime, for tests and for deployments with a policy.
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// The session this table belongs to.
    pub fn session(&self) -> &str {
        &self.session
    }

    /// Issues a handle for `ior`, reporting why if it cannot.
    ///
    /// Deliberately **not** deduplicating by IOR. Returning the same token for
    /// the same target would let an agent test whether two references it was
    /// given separately point at the same object, which is a fact about the
    /// deployment it was not told.
    pub fn issue_checked(&mut self, ior: &Ior) -> Result<Handle, HandleError> {
        self.expire();
        if self.entries.len() >= MAX_HANDLES_PER_SESSION {
            return Err(HandleError::TooMany);
        }
        let token = format!("cap_{}", hex(&entropy16()?));
        self.entries.insert(
            token.clone(),
            Entry {
                ior: ior.clone(),
                session: self.session.clone(),
                issued: Instant::now(),
                type_id: ior.type_id.clone(),
            },
        );
        Ok(Handle(token))
    }

    /// The repository id a handle names, without resolving it to an address.
    ///
    /// This is what `describe_interface` needs and all it needs: an agent may
    /// reason about the type of what it holds without holding the endpoint.
    pub fn type_of(&self, handle: &str) -> Option<&str> {
        self.live(handle).map(|e| e.type_id.as_str())
    }

    /// Live handles naming an object of `type_id`.
    ///
    /// How an agent obtains its first handle. Without this the bootstrap
    /// existed only on the operator's terminal, so a session could describe an
    /// interface and had no way to reach an instance of it — the tools were
    /// complete and the workflow was not.
    pub fn handles_for(&self, type_id: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|(_, e)| {
                e.type_id == type_id && e.session == self.session && e.issued.elapsed() < self.ttl
            })
            .map(|(k, _)| k.as_str())
            .collect()
    }

    /// Forgets a handle. Idempotent.
    pub fn revoke(&mut self, handle: &str) -> bool {
        self.entries.remove(handle).is_some()
    }

    /// How many handles are live.
    pub fn len(&self) -> usize {
        self.entries.values().filter(|e| e.issued.elapsed() < self.ttl).count()
    }

    /// Whether nothing is live.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn live(&self, handle: &str) -> Option<&Entry> {
        self.entries
            .get(handle)
            .filter(|e| e.session == self.session && e.issued.elapsed() < self.ttl)
    }

    /// Drops expired entries. Called on issue so the table cannot grow on
    /// expiry alone.
    fn expire(&mut self) {
        let ttl = self.ttl;
        self.entries.retain(|_, e| e.issued.elapsed() < ttl);
    }
}

impl References for CapabilityTable {
    /// Infallible by signature, because the mapping cannot report an error
    /// here. A refusal surfaces as the handle being absent from the document,
    /// which the caller then reports; `issue_checked` is the path that explains
    /// why.
    fn issue(&mut self, ior: &Ior) -> String {
        match self.issue_checked(ior) {
            Ok(h) => h.0,
            // An empty name resolves to nothing, so a failure here cannot
            // produce a usable reference by accident.
            Err(_) => String::new(),
        }
    }

    fn resolve(&self, handle: &str) -> Option<Ior> {
        self.live(handle).map(|e| e.ior.clone())
    }
}

/// 128 bits from the operating system.
fn entropy16() -> Result<[u8; 16], HandleError> {
    let mut buf = [0u8; 16];
    let mut f = std::fs::File::open("/dev/urandom")
        .map_err(|e| HandleError::NoEntropy(format!("/dev/urandom: {e}")))?;
    f.read_exact(&mut buf).map_err(|e| HandleError::NoEntropy(format!("/dev/urandom: {e}")))?;
    Ok(buf)
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[usize::from(b >> 4)] as char);
        out.push(HEX[usize::from(b & 0xF)] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_giop::{IiopProfile, Version};

    fn ior(key: &[u8]) -> Ior {
        Ior {
            type_id: "IDL:bank/Account:1.0".into(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: "10.9.8.7".into(),
                port: 4242,
                object_key: key.to_vec(),
                components: Vec::new(),
            }],
        }
    }

    #[test]
    fn a_handle_resolves_within_its_session() {
        let mut t = CapabilityTable::new("s1");
        let h = t.issue_checked(&ior(b"acct-1")).expect("issued");
        assert_eq!(t.resolve(h.as_str()), Some(ior(b"acct-1")));
        assert_eq!(t.type_of(h.as_str()), Some("IDL:bank/Account:1.0"));
    }

    /// The property that makes a leaked transcript harmless.
    #[test]
    fn a_handle_from_another_session_resolves_to_nothing() {
        let mut a = CapabilityTable::new("s1");
        let h = a.issue_checked(&ior(b"acct-1")).unwrap();
        let b = CapabilityTable::new("s2");
        assert_eq!(b.resolve(h.as_str()), None);
        assert_eq!(b.type_of(h.as_str()), None);
    }

    #[test]
    fn a_handle_stops_working_when_it_expires() {
        let mut t = CapabilityTable::new("s1").with_ttl(Duration::from_millis(40));
        let h = t.issue_checked(&ior(b"acct-1")).unwrap();
        assert!(t.resolve(h.as_str()).is_some());
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(t.resolve(h.as_str()), None, "an expired handle must not resolve");
        assert_eq!(t.len(), 0);
    }

    /// The reason handles exist at all: nothing about the address may be
    /// recoverable from the name.
    #[test]
    fn nothing_about_the_target_appears_in_the_token() {
        let mut t = CapabilityTable::new("s1");
        let h = t.issue_checked(&ior(b"very-distinctive-object-key")).unwrap();
        let token = h.as_str();
        for leak in ["10.9.8.7", "4242", "distinctive", "bank", "Account", "IOR"] {
            assert!(!token.contains(leak), "{leak:?} appears in the handle {token}");
        }
        assert_eq!(token.len(), 4 + 32, "cap_ plus 128 bits of hex");
    }

    /// Sequential handles were the placeholder's flaw: one told you the next.
    #[test]
    fn handles_are_not_derivable_from_one_another() {
        let mut t = CapabilityTable::new("s1");
        let tokens: std::collections::BTreeSet<String> =
            (0..256).map(|_| t.issue_checked(&ior(b"k")).unwrap().0).collect();
        assert_eq!(tokens.len(), 256, "a repeat means the source is not random");

        // Every bit position should vary across the sample. A stuck bit would
        // mean the entropy is not reaching the token, which no amount of
        // uniqueness would reveal.
        let raw: Vec<Vec<u8>> =
            tokens.iter().map(|t| t.trim_start_matches("cap_").as_bytes().to_vec()).collect();
        for pos in 0..32 {
            let distinct: std::collections::BTreeSet<u8> = raw.iter().map(|r| r[pos]).collect();
            assert!(distinct.len() > 4, "hex digit {pos} barely varies: {distinct:?}");
        }
    }

    /// Two handles for one target must differ, or an agent can test whether
    /// two references it was given separately point at the same object.
    #[test]
    fn the_same_target_issued_twice_gets_two_different_handles() {
        let mut t = CapabilityTable::new("s1");
        let a = t.issue_checked(&ior(b"same")).unwrap();
        let b = t.issue_checked(&ior(b"same")).unwrap();
        assert_ne!(a, b);
        assert_eq!(t.resolve(a.as_str()), t.resolve(b.as_str()), "both still work");
    }

    #[test]
    fn revocation_takes_effect_immediately() {
        let mut t = CapabilityTable::new("s1");
        let h = t.issue_checked(&ior(b"k")).unwrap();
        assert!(t.revoke(h.as_str()));
        assert_eq!(t.resolve(h.as_str()), None);
        assert!(!t.revoke(h.as_str()), "revoking twice is not an error, just false");
    }

    #[test]
    fn a_forged_token_resolves_to_nothing() {
        let mut t = CapabilityTable::new("s1");
        let real = t.issue_checked(&ior(b"k")).unwrap();
        for forged in [
            "cap_00000000000000000000000000000000",
            "",
            "local-1",
            &format!("{}0", real.as_str()),
            real.as_str().trim_end_matches(|c: char| c.is_ascii_hexdigit()),
        ] {
            assert_eq!(t.resolve(forged), None, "{forged:?} must not resolve");
        }
    }

    #[test]
    fn the_table_is_bounded_and_says_so() {
        let mut t = CapabilityTable::new("s1");
        for _ in 0..MAX_HANDLES_PER_SESSION {
            t.issue_checked(&ior(b"k")).expect("within the bound");
        }
        assert_eq!(t.issue_checked(&ior(b"k")), Err(HandleError::TooMany));
    }

    /// Expiry must reclaim space, not merely hide entries: otherwise a
    /// long-lived session hits the bound on handles nobody can use.
    #[test]
    fn expired_entries_stop_counting_against_the_bound() {
        let mut t = CapabilityTable::new("s1").with_ttl(Duration::from_millis(30));
        for _ in 0..10 {
            t.issue_checked(&ior(b"k")).unwrap();
        }
        assert_eq!(t.len(), 10);
        std::thread::sleep(Duration::from_millis(50));
        t.issue_checked(&ior(b"k")).unwrap();
        assert_eq!(t.len(), 1, "the ten expired entries were reclaimed");
    }
}
