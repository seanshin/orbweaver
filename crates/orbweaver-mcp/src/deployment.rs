//! The numbers a deployment owns, in a file instead of a source file.
//!
//! D015 §2's acceptance sentence ends *"…and with an operator able to say who
//! may call what, how often, and for how long."* Three of those clauses reached
//! no declarative surface at all before this module: the handle TTL was a
//! `const` and a builder nothing but a test called, the quota's two numbers had
//! a seat and a flag but no file, and the allowlist was assembled from `argv`
//! and nowhere else. Every one of them is a **policy only an operator has** —
//! the crate owes them the mechanism, the refusal shape and the arithmetic in
//! the message, and must not choose the number
//! ([`crate::interceptor::SEAT_QUOTA`] says exactly this, and it is the
//! argument for this module rather than against it).
//!
//! *배포가 소유하는 수치는 집이 하나이며, 그 집은 소스 파일이 아니다.*
//!
//! # The rule this module is scoped to
//!
//! **A number or a policy that only a deployment can know has one home, and it
//! is not a source file.** Not "the TTL, the quota and the exposure" — those
//! are three instances. The neighbours were re-measured with them, which is why
//! [`Deployment`] also carries the per-session reference ceiling, the audit
//! ledger's bound, the search result cap and the dial timeout. What stayed in
//! code stayed for a stated reason, recorded beside the sweep in the commit
//! that landed this file: a protocol constant, a repository id, a stage name in
//! a D004 record and a vector threshold for an index this process never builds
//! are facts about a format or a specification, not numbers an operator has.
//!
//! # Absent is not zero, and a default is never restated here
//!
//! Every setting is an [`Option`]. **`Deployment::default()` supplies nothing**
//! — not "15 minutes", not "65,536", not "expose nothing": nothing at all. A
//! deployment that names no configuration file therefore runs the code path it
//! ran before this module existed, because [`Deployment::apply`] installs only
//! what was actually written. That is why "defaults preserve today's behaviour
//! exactly" is a property of the type rather than a claim a test has to keep
//! chasing: there is no second copy of `DEFAULT_TTL` in this file to drift from
//! [`crate::handles::DEFAULT_TTL`]. The one number this module owns is
//! [`DEFAULT_CONNECT_TIMEOUT`], because the dial belongs to the deployment
//! surface and had no home before.
//!
//! *없음은 0이 아니다. 기본값은 여기에 다시 적히지 않는다 — 다시 적은 값은
//! 다음 변경에서 조용히 어긋난다.*
//!
//! # Refused loudly, and never half-applied
//!
//! A malformed configuration is a [`ConfigError`] naming **the file, the key
//! and what was expected**, and the process stops. Nothing is applied on the
//! way to discovering the defect, and that is structural rather than careful:
//! parsing produces a whole [`Deployment`] or an `Err`, and
//! [`Deployment::apply`] never runs on a document that did not parse. A
//! half-applied policy — the exposure taken and the quota dropped — is worse
//! than no policy, because it looks like one.
//!
//! An **unknown key is a refusal too.** `handles.ttl_second` is a setting an
//! operator wrote and this process would otherwise ignore in silence, which is
//! the harness rule about silent skips arriving through a configuration file.
//! The known keys are listed in the message, so the typo answers itself.
//!
//! # Default-deny survives every form of the file
//!
//! Nothing here can widen exposure by absence. `expose` missing, `expose: []`,
//! an empty document `{}`, a file that is not there, a file that does not
//! parse — the first three leave the allowlist exactly as the command line
//! built it (which is [`Exposure::nothing`] when no `--expose` was given) and
//! the last two stop the process. The only way to reach an interface is for
//! somebody to have written its repository id somewhere, and this module adds
//! a second place to write it, not a way to skip writing it.
//!
//! # The shape
//!
//! ```json
//! {
//!   "expose": ["IDL:bank/Account:1.0", "IDL:bank/Ledger:1.0.keep"],
//!   "assume_effect": "read_only",
//!   "handles": { "ttl_seconds": 900, "max_per_session": 4096 },
//!   "quota":   { "limit": 500, "scope": "caller" },
//!   "audit":   { "capacity": 65536 },
//!   "search":  { "default_limit": 20 },
//!   "connect": { "timeout_seconds": 10 }
//! }
//! ```
//!
//! `crates/orbweaver-mcp/deployment.example.json` is a working file in this
//! shape, and it is **checked by a test** rather than left as prose — an
//! example nothing parses is a document that goes stale the first time a key is
//! renamed. Every number in it is deliberately *not* a default, because an
//! example that restated the defaults would be the second copy this module
//! exists to avoid.
//!
//! JSON because this crate already parses it ([`orbweaver_dynamic::json`]),
//! this boundary already speaks it, and every `--dry-run` document an operator
//! reads is one. A second format would have been a dependency to license-check
//! or a parser to write, and neither buys anything the operator does not
//! already have.
//!
//! # Why the file is named and never discovered
//!
//! There is no search path and no `orbweaver.json` picked up from the working
//! directory. A configuration this process finds on its own is a configuration
//! that can start applying to a deployment nobody changed — the same class as
//! a default that moved. The path arrives on the command line, is read once at
//! startup, and a path that will not open is an error rather than a fallback:
//! `--config /etc/orbweaver/typo.json` must not quietly become "no policy".

use std::collections::BTreeMap;
use std::time::Duration;

use orbweaver_dynamic::json::Json;

use crate::Bridge;
use crate::policy::{Exposure, split_operation};
use crate::quota::{Quota, Renewal, Scope};

/// How long this process waits for the target to answer a dial, when the
/// configuration does not say.
///
/// The one default this module owns, because the dial is part of the operator
/// surface and had nowhere else to live. Every other default belongs to the
/// thing it configures and is referenced, never restated:
/// [`crate::handles::DEFAULT_TTL`], [`crate::handles::MAX_HANDLES_PER_SESSION`],
/// [`crate::interceptor::DEFAULT_AUDIT_CAPACITY`],
/// [`crate::session::DEFAULT_SEARCH_LIMIT`].
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Why a configuration was refused.
///
/// Four fields rather than one sentence, because a caller that wants to know
/// *which key* was wrong must not have to match a substring of a message this
/// module owns — the classifier rule. [`ConfigError::key`] is the dotted path
/// and is empty only for a fault in the document as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    file: String,
    key: String,
    expected: String,
    found: String,
}

impl ConfigError {
    /// The file the operator has to open.
    pub fn file(&self) -> &str {
        &self.file
    }

    /// The dotted key, or `""` for the document itself.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// What this key accepts.
    pub fn expected(&self) -> &str {
        &self.expected
    }

    /// What was there instead.
    pub fn found(&self) -> &str {
        &self.found
    }
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.key.is_empty() {
            write!(f, "{}: expected {}; found {}", self.file, self.expected, self.found)
        } else {
            write!(
                f,
                "{}: {}: expected {}; found {}",
                self.file, self.key, self.expected, self.found
            )
        }
    }
}

impl std::error::Error for ConfigError {}

/// Everything a deployment may say, and nothing it did not say.
///
/// See the module docs. Every field is optional and `Default` supplies none of
/// them, so a [`Deployment::default()`] applied to a bridge changes nothing
/// about it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Deployment {
    expose: Vec<String>,
    assume_effect: Option<String>,
    handle_ttl: Option<Duration>,
    max_handles: Option<usize>,
    quota: Option<(u64, Scope)>,
    audit_capacity: Option<usize>,
    search_limit: Option<usize>,
    connect_timeout: Option<Duration>,
}

/// The top-level keys, and the order the "expected one of" list names them in.
const KEYS: &[&str] =
    &["assume_effect", "audit", "connect", "expose", "handles", "quota", "search"];

impl Deployment {
    /// Reads and parses one configuration file.
    ///
    /// A path that will not open is an error naming the path and the reason —
    /// never a silent fall back to the defaults, which is how a policy goes
    /// missing without anybody being told.
    pub fn from_file(path: &str) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|e| ConfigError {
            file: path.to_owned(),
            key: String::new(),
            expected: "a readable configuration file".to_owned(),
            found: e.to_string(),
        })?;
        Self::parse(path, &text)
    }

    /// Parses `text` as the configuration named `file`.
    ///
    /// `file` is carried only so a diagnostic can name it; nothing is read from
    /// disk here, which is what lets a test drive the same parser the process
    /// runs.
    pub fn parse(file: &str, text: &str) -> Result<Self, ConfigError> {
        let doc = Json::parse(text).map_err(|e| ConfigError {
            file: file.to_owned(),
            key: String::new(),
            expected: "a JSON object".to_owned(),
            found: e.to_string(),
        })?;
        let Json::Object(fields) = &doc else {
            return Err(ConfigError {
                file: file.to_owned(),
                key: String::new(),
                expected: "a JSON object".to_owned(),
                found: doc.kind().to_owned(),
            });
        };
        unknown(file, "", fields, KEYS)?;

        let mut out = Self::default();
        if let Some(v) = fields.get("expose") {
            let Json::Array(items) = v else {
                return Err(wrong(file, "expose", "an array of repository ids", v));
            };
            for (i, item) in items.iter().enumerate() {
                let key = format!("expose[{i}]");
                let Json::String(spec) = item else {
                    return Err(wrong(
                        file,
                        &key,
                        "a string, IDL:module/Iface:1.0 or IDL:module/Iface:1.0.operation",
                        item,
                    ));
                };
                if spec.trim().is_empty() {
                    return Err(wrong(file, &key, "a non-empty repository id", item));
                }
                out.expose.push(spec.clone());
            }
        }
        if let Some(v) = fields.get("assume_effect") {
            let Json::String(effect) = v else {
                return Err(wrong(
                    file,
                    "assume_effect",
                    "an ai_effect value, such as \"read_only\" or \"destructive\"",
                    v,
                ));
            };
            if effect.trim().is_empty() {
                return Err(wrong(
                    file,
                    "assume_effect",
                    "an ai_effect value, such as \"read_only\" or \"destructive\"",
                    v,
                ));
            }
            out.assume_effect = Some(effect.clone());
        }
        if let Some(v) = fields.get("handles") {
            let section = object(file, "handles", v)?;
            unknown(file, "handles", section, &["max_per_session", "ttl_seconds"])?;
            out.handle_ttl =
                positive(file, "handles.ttl_seconds", section.get("ttl_seconds"), "seconds")?
                    .map(Duration::from_secs);
            out.max_handles = positive(
                file,
                "handles.max_per_session",
                section.get("max_per_session"),
                "references",
            )?
            .map(count);
        }
        if let Some(v) = fields.get("quota") {
            let section = object(file, "quota", v)?;
            unknown(file, "quota", section, &["limit", "scope"])?;
            // A limit of zero is a budget that refuses everything, and it is
            // accepted here for the one reason `--quota 0` is: it is a thing an
            // operator can mean, said in the one place they can say it. Every
            // other ceiling in this file refuses zero, because a ledger that
            // cannot hold a line and a handle that expires before it is issued
            // are not policies but mistakes.
            let Some(limit) = whole(file, "quota.limit", section.get("limit"), "calls")? else {
                return Err(ConfigError {
                    file: file.to_owned(),
                    key: "quota.limit".to_owned(),
                    expected: "a whole number of calls, because a quota section with no limit \
                               installs no quota and would read as one"
                        .to_owned(),
                    found: "no limit".to_owned(),
                });
            };
            let scope = match section.get("scope") {
                None => Scope::Caller,
                Some(Json::String(name)) => Scope::parse(name).ok_or_else(|| ConfigError {
                    file: file.to_owned(),
                    key: "quota.scope".to_owned(),
                    expected: format!("one of {}", Scope::names().join(", ")),
                    found: format!("{name:?}"),
                })?,
                Some(other) => {
                    return Err(wrong(
                        file,
                        "quota.scope",
                        &format!("one of {}", Scope::names().join(", ")),
                        other,
                    ));
                }
            };
            out.quota = Some((limit, scope));
        }
        if let Some(v) = fields.get("audit") {
            let section = object(file, "audit", v)?;
            unknown(file, "audit", section, &["capacity"])?;
            out.audit_capacity =
                positive(file, "audit.capacity", section.get("capacity"), "lines")?.map(count);
        }
        if let Some(v) = fields.get("search") {
            let section = object(file, "search", v)?;
            unknown(file, "search", section, &["default_limit"])?;
            out.search_limit =
                positive(file, "search.default_limit", section.get("default_limit"), "results")?
                    .map(count);
        }
        if let Some(v) = fields.get("connect") {
            let section = object(file, "connect", v)?;
            unknown(file, "connect", section, &["timeout_seconds"])?;
            out.connect_timeout = positive(
                file,
                "connect.timeout_seconds",
                section.get("timeout_seconds"),
                "seconds",
            )?
            .map(Duration::from_secs);
        }
        Ok(out)
    }

    /// The repository ids and operations this configuration exposes, as written.
    pub fn expose(&self) -> &[String] {
        &self.expose
    }

    /// What this configuration assumes an unannotated operation means.
    pub fn assume_effect(&self) -> Option<&str> {
        self.assume_effect.as_deref()
    }

    /// The handle lifetime, if one was declared.
    pub fn handle_ttl(&self) -> Option<Duration> {
        self.handle_ttl
    }

    /// The per-session reference ceiling, if one was declared.
    pub fn max_handles(&self) -> Option<usize> {
        self.max_handles
    }

    /// The consumption budget, if one was declared.
    ///
    /// [`Renewal::Never`] always: this process reads no clock and has no window
    /// source, so a budget here is a per-run total whose refusals do not invite
    /// a retry — the same reasoning `--quota` states. A host with a clock builds
    /// its own [`Quota`] with [`Renewal::Window`].
    pub fn quota(&self) -> Option<Quota> {
        self.quota.map(|(limit, scope)| Quota::new(limit, scope, Renewal::Never))
    }

    /// The in-memory audit ledger's bound, if one was declared.
    pub fn audit_capacity(&self) -> Option<usize> {
        self.audit_capacity
    }

    /// The `search_interfaces` result cap for a request that names none.
    pub fn search_limit(&self) -> Option<usize> {
        self.search_limit
    }

    /// How long to wait for the target to answer a dial, if it was declared.
    pub fn connect_timeout(&self) -> Option<Duration> {
        self.connect_timeout
    }

    /// Overrides the budget, for a command line that named one.
    ///
    /// A flag is the more specific instrument and wins over the file, so an
    /// invocation that worked before a configuration existed still means what
    /// it meant.
    pub fn set_quota(&mut self, limit: u64, scope: Scope) {
        self.quota = Some((limit, scope));
    }

    /// Overrides the ledger bound, for a command line that named one.
    pub fn set_audit_capacity(&mut self, capacity: usize) {
        self.audit_capacity = Some(capacity);
    }

    /// Adds this configuration's `expose` entries to an allowlist.
    ///
    /// The **same** reading of `IDL:module/Iface:1.0[.operation]` the command
    /// line uses ([`split_operation`]) and the same two builders, so a
    /// repository id cannot mean one thing in a flag and another in a file.
    /// `base` is what `--expose` built, which is [`Exposure::nothing`] when
    /// nothing was given: this only ever adds, and adds only what somebody
    /// wrote.
    pub fn extend_exposure(&self, base: Exposure) -> Exposure {
        self.expose.iter().fold(base, |acc, spec| match split_operation(spec) {
            (id, Some(op)) => acc.allow_operation(id, op),
            (id, None) => acc.allow_interface(id),
        })
    }

    /// Installs everything that belongs to a live bridge — the handle table's
    /// lifetime and ceiling, the ledger's bound, and the quota — and returns
    /// one line per setting for the operator to read on stderr.
    ///
    /// **Said out loud, like the quota already was.** A limit an operator
    /// forgot they wrote is a limit they will debug as a policy failure, and a
    /// file makes forgetting easier than a flag did.
    ///
    /// The `Err` is not a configuration fault: it names a seat that was not
    /// there to fill, which is a fault in the chain this process built and is
    /// reported the way the existing `--quota` path reports it.
    pub fn apply(&self, bridge: &mut Bridge<'_>) -> Result<Vec<String>, String> {
        let mut said = Vec::new();
        if let Some(ttl) = self.handle_ttl {
            bridge.handles().set_ttl(ttl);
            said.push(format!(
                "handles: a capability expires {} second(s) after it is issued",
                ttl.as_secs()
            ));
        }
        if let Some(max) = self.max_handles {
            bridge.handles().set_max_per_session(max);
            said.push(format!("handles: this session may hold {max} reference(s) at once"));
        }
        if let Some(capacity) = self.audit_capacity {
            if !bridge.chain_mut().audit_capacity(capacity) {
                return Err("no audit stage to bound".to_owned());
            }
            said.push(format!(
                "audit ledger: the newest {capacity} lines are kept in memory (stderr has all)"
            ));
        }
        if let Some(quota) = self.quota() {
            let (limit, scope) = (quota.limit(), quota.scope());
            if !bridge.chain_mut().quota(quota) {
                return Err("no authorization stage to put a quota after".to_owned());
            }
            said.push(format!(
                "quota: {limit} calls per {scope}, for this run only (this process opens no windows)"
            ));
        }
        Ok(said)
    }
}

/// `{file}: {key}: expected {expected}; found {kind of what was there}`.
fn wrong(file: &str, key: &str, expected: &str, found: &Json) -> ConfigError {
    ConfigError {
        file: file.to_owned(),
        key: key.to_owned(),
        expected: expected.to_owned(),
        found: found.kind().to_owned(),
    }
}

/// The section, or an error naming the key that is not an object.
fn object<'a>(
    file: &str,
    key: &str,
    v: &'a Json,
) -> Result<&'a BTreeMap<String, Json>, ConfigError> {
    match v {
        Json::Object(fields) => Ok(fields),
        other => Err(wrong(file, key, "an object", other)),
    }
}

/// Refuses a key no setting is named by, listing the ones that are.
///
/// A typo an operator wrote is a setting they believe is in force; ignoring it
/// is the silent skip that hides everything else.
fn unknown(
    file: &str,
    prefix: &str,
    fields: &BTreeMap<String, Json>,
    known: &[&str],
) -> Result<(), ConfigError> {
    for name in fields.keys() {
        if !known.contains(&name.as_str()) {
            let key = if prefix.is_empty() { name.clone() } else { format!("{prefix}.{name}") };
            return Err(ConfigError {
                file: file.to_owned(),
                key,
                expected: format!("one of {}", known.join(", ")),
                found: "no such setting".to_owned(),
            });
        }
    }
    Ok(())
}

/// A whole non-negative number, or `None` when the key is absent.
fn whole(file: &str, key: &str, v: Option<&Json>, unit: &str) -> Result<Option<u64>, ConfigError> {
    match v {
        None => Ok(None),
        Some(Json::Number(n)) => n.parse::<u64>().map(Some).map_err(|_| ConfigError {
            file: file.to_owned(),
            key: key.to_owned(),
            expected: format!("a whole number of {unit}"),
            found: n.clone(),
        }),
        Some(other) => Err(wrong(file, key, &format!("a whole number of {unit}"), other)),
    }
}

/// The same, and greater than zero.
///
/// Zero is refused for every ceiling in this file because it is never a policy:
/// a table whose handles expire before they are issued, a session that may hold
/// no references, a ledger that cannot hold a line and a search that returns
/// nothing are all mistakes wearing a number. The quota's limit is the one
/// exception and says why at its own site.
fn positive(
    file: &str,
    key: &str,
    v: Option<&Json>,
    unit: &str,
) -> Result<Option<u64>, ConfigError> {
    match whole(file, key, v, unit)? {
        Some(0) => Err(ConfigError {
            file: file.to_owned(),
            key: key.to_owned(),
            expected: format!("a whole number of {unit} greater than zero"),
            found: "0".to_owned(),
        }),
        other => Ok(other),
    }
}

/// A count the platform can index with, saturating rather than wrapping.
///
/// A 32-bit host asked for four billion audit lines gets `usize::MAX`, which is
/// the honest reading of "more than this machine can hold" and not a small
/// number arrived at by truncation.
fn count(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Unannotated;

    /// The property the whole module rests on: a deployment that says nothing
    /// is a deployment that changes nothing. Asserted as *absence* rather than
    /// against a list of numbers, because a test that restated the defaults
    /// would be the second copy this file exists to avoid.
    #[test]
    fn an_empty_configuration_supplies_nothing() {
        for text in ["{}", "  {}\n", "{\n}\n"] {
            let d = Deployment::parse("empty.json", text).expect("an empty object is valid");
            assert_eq!(d, Deployment::default(), "{text:?} must mean what no file means");
            assert!(d.expose().is_empty());
            assert_eq!(d.handle_ttl(), None);
            assert_eq!(d.max_handles(), None);
            assert!(d.quota().is_none());
            assert_eq!(d.audit_capacity(), None);
            assert_eq!(d.search_limit(), None);
            assert_eq!(d.connect_timeout(), None);
            assert_eq!(d.assume_effect(), None);
        }
    }

    /// Default-deny, in the two shapes a file can take it away in.
    #[test]
    fn no_form_of_the_file_widens_an_empty_exposure() {
        for text in ["{}", r#"{"expose":[]}"#, r#"{"handles":{"ttl_seconds":1}}"#] {
            let d = Deployment::parse("deny.json", text).expect("valid");
            let e = d.extend_exposure(Exposure::nothing());
            assert_eq!(e.interfaces().count(), 0, "{text:?} exposed something");
            assert!(!e.exposes("IDL:bank/Account:1.0"), "{text:?}");
            assert!(!e.exposes_operation("IDL:bank/Account:1.0", "balance"), "{text:?}");
        }
    }

    #[test]
    fn every_setting_is_read() {
        let d = Deployment::parse(
            "full.json",
            r#"{"expose":["IDL:bank/Account:1.0","IDL:bank/Ledger:1.0.keep"],
                "assume_effect":"read_only",
                "handles":{"ttl_seconds":30,"max_per_session":7},
                "quota":{"limit":5,"scope":"operation"},
                "audit":{"capacity":11},
                "search":{"default_limit":3},
                "connect":{"timeout_seconds":2}}"#,
        )
        .expect("valid");
        assert_eq!(d.expose(), ["IDL:bank/Account:1.0", "IDL:bank/Ledger:1.0.keep"]);
        assert_eq!(d.assume_effect(), Some("read_only"));
        assert_eq!(d.handle_ttl(), Some(Duration::from_secs(30)));
        assert_eq!(d.max_handles(), Some(7));
        let q = d.quota().expect("a quota");
        assert_eq!((q.limit(), q.scope(), q.renewal()), (5, Scope::Operation, Renewal::Never));
        assert_eq!(d.audit_capacity(), Some(11));
        assert_eq!(d.search_limit(), Some(3));
        assert_eq!(d.connect_timeout(), Some(Duration::from_secs(2)));

        let e = d
            .extend_exposure(Exposure::nothing())
            .assuming_unannotated(Unannotated::Assume(d.assume_effect().unwrap().to_owned()));
        assert!(e.exposes_operation("IDL:bank/Account:1.0", "anything"), "interface-wide");
        assert!(e.exposes_operation("IDL:bank/Ledger:1.0", "keep"));
        assert!(!e.exposes_operation("IDL:bank/Ledger:1.0", "purge"), "one operation only");
        assert_eq!(e.unannotated(), &Unannotated::Assume("read_only".to_owned()));
    }

    /// The quota's scope defaults to what `--quota-scope` defaults to, and it
    /// is read through the same [`Scope::parse`] the flag reads, so the two
    /// cannot come to different conclusions about the word "caller".
    #[test]
    fn a_quota_without_a_scope_is_per_caller() {
        let d = Deployment::parse("q.json", r#"{"quota":{"limit":1}}"#).expect("valid");
        assert_eq!(d.quota().expect("a quota").scope(), Scope::Caller);
    }

    /// Every refusal names the file and the key. Asserted through the
    /// accessors, not by matching a substring of a sentence this module owns.
    #[test]
    fn a_malformed_setting_names_the_file_and_the_key() {
        let cases: &[(&str, &str)] = &[
            (r#"{"handles":{"ttl_seconds":"15m"}}"#, "handles.ttl_seconds"),
            (r#"{"handles":{"ttl_seconds":0}}"#, "handles.ttl_seconds"),
            (r#"{"handles":{"ttl_seconds":-5}}"#, "handles.ttl_seconds"),
            (r#"{"handles":{"max_per_session":0}}"#, "handles.max_per_session"),
            (r#"{"handles":{"ttl_second":15}}"#, "handles.ttl_second"),
            (r#"{"handles":[]}"#, "handles"),
            (r#"{"quota":{"limit":1,"scope":"tenant"}}"#, "quota.scope"),
            (r#"{"quota":{"limit":1,"scope":7}}"#, "quota.scope"),
            (r#"{"quota":{"scope":"caller"}}"#, "quota.limit"),
            (r#"{"quota":{"limit":"lots"}}"#, "quota.limit"),
            (r#"{"audit":{"capacity":0}}"#, "audit.capacity"),
            (r#"{"search":{"default_limit":0}}"#, "search.default_limit"),
            (r#"{"connect":{"timeout_seconds":0}}"#, "connect.timeout_seconds"),
            (r#"{"expose":"IDL:bank/Account:1.0"}"#, "expose"),
            (r#"{"expose":[7]}"#, "expose[0]"),
            (r#"{"expose":["IDL:a/B:1.0",""]}"#, "expose[1]"),
            (r#"{"assume_effect":""}"#, "assume_effect"),
            (r#"{"assume_effect":true}"#, "assume_effect"),
            (r#"{"exposure":[]}"#, "exposure"),
        ];
        for (text, key) in cases {
            let e = Deployment::parse("policy.json", text)
                .expect_err(&format!("{text} must be refused"));
            assert_eq!(e.file(), "policy.json", "{text}");
            assert_eq!(e.key(), *key, "{text}");
            assert!(!e.expected().is_empty(), "{text}: a refusal must say what it wanted");
            assert!(e.to_string().contains("policy.json"), "{text}");
            assert!(e.to_string().contains(key), "{text}");
        }
    }

    /// A document that is not an object, and one that is not JSON at all.
    #[test]
    fn a_document_that_is_not_an_object_is_refused() {
        for text in ["[]", "7", "null", "\"expose\"", "{", "{\"expose\":}", ""] {
            let e = Deployment::parse("doc.json", text).expect_err(&format!("{text:?}"));
            assert_eq!(e.file(), "doc.json");
            assert_eq!(e.key(), "", "a fault in the document as a whole has no key");
        }
    }

    /// A path that will not open is an error, not "no policy".
    #[test]
    fn a_missing_file_is_refused_rather_than_defaulted() {
        let e = Deployment::from_file("/nonexistent/orbweaver/policy.json").expect_err("no file");
        assert_eq!(e.file(), "/nonexistent/orbweaver/policy.json");
        assert_eq!(e.key(), "");
    }

    /// Nothing is applied on the way to discovering a defect: the parse
    /// produces a whole value or an error, so a later key cannot be in force
    /// while an earlier one was refused.
    #[test]
    fn a_refused_document_yields_no_partial_value() {
        let e = Deployment::parse(
            "half.json",
            r#"{"handles":{"ttl_seconds":30},"quota":{"limit":1,"scope":"tenant"}}"#,
        );
        assert!(e.is_err(), "a bad scope must refuse the whole document");
    }

    /// The shipped example parses, names every key, and states no default.
    ///
    /// An example nothing reads is a document that goes stale the first time a
    /// key is renamed — and one that restated the defaults would be the second
    /// copy of them, so this asserts it differs from every constant it could
    /// have echoed.
    #[test]
    fn the_shipped_example_parses_and_restates_no_default() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/deployment.example.json");
        let d = Deployment::from_file(path).expect("the shipped example must parse");
        assert!(!d.expose().is_empty(), "an example with no exposure teaches default-deny only");
        assert!(d.assume_effect().is_some());
        assert_ne!(d.handle_ttl(), Some(crate::handles::DEFAULT_TTL));
        assert_ne!(d.max_handles(), Some(crate::handles::MAX_HANDLES_PER_SESSION));
        assert_ne!(d.audit_capacity(), Some(crate::interceptor::DEFAULT_AUDIT_CAPACITY));
        assert_ne!(d.search_limit(), Some(crate::session::DEFAULT_SEARCH_LIMIT));
        assert_ne!(d.connect_timeout(), Some(DEFAULT_CONNECT_TIMEOUT));
        assert!(d.quota().is_some());
    }

    /// A flag is the more specific instrument and wins.
    #[test]
    fn the_command_line_overrides_the_file() {
        let mut d =
            Deployment::parse("o.json", r#"{"quota":{"limit":5},"audit":{"capacity":9}}"#).unwrap();
        d.set_quota(2, Scope::Everything);
        d.set_audit_capacity(3);
        let q = d.quota().expect("a quota");
        assert_eq!((q.limit(), q.scope()), (2, Scope::Everything));
        assert_eq!(d.audit_capacity(), Some(3));
    }
}
