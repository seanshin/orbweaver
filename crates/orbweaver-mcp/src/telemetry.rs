//! D004 tier 1: one span record per decision, through a first-party sink.
//!
//! `docs/decisions/D004-observability.md` is **APPROVED** with three tiers, and
//! this is tier 1: a first-party span record on F4's telemetry stage behind
//! [`TelemetrySink`], JSON lines, **no Cargo dependency**. `cargo tree` stays at
//! two external crates, and the check is part of the batch that added this file.
//!
//! # Why a trait and not a crate type
//!
//! D004's one constraint on F4's signature: *the telemetry stage takes a sink
//! trait, not a crate type.* If [`crate::interceptor::TelemetryInterceptor`]
//! named `tracing::Span` or an OpenTelemetry type, the dependency would stop
//! being reversible and D004 would stop being a decision. Nothing in this
//! module's public surface names a type from outside this workspace — `io::Write`
//! is `std`, which is not a dependency — so tiers 2 and 3 remain a choice
//! somebody else can make later for the cost of one indirection.
//!
//! # The record shape is the decision's, not this file's
//!
//! The nine keys and **their order** are fixed in D004's table, deliberately
//! settled in the decision rather than in whichever crate landed first: the
//! emitter and `orbweaver-console` are separate batches, and a format settled by
//! whoever committed earliest is a format nobody agreed to. [`SpanRecord`] is
//! that table:
//!
//! | key | where it comes from |
//! |---|---|
//! | `ts` | the **caller's** [`Timestamp`]; see below |
//! | `session` | the MCP session id the host gave [`Trace::new`] |
//! | `caller` | `ctx.caller`'s principal, or [`ABSENT`] |
//! | `target` | `ctx.target`, the repository id the capability table resolved |
//! | `operation` | `ctx.operation` |
//! | `decision` | [`Decision`], the audit vocabulary in lower case |
//! | `stage` | the stage that refused, or [`ABSENT`] |
//! | `path` | [`CallPath`], what the promotion policy reasons about |
//! | `outcome` | [`OUTCOME_OK`], [`OUTCOME_REFUSED`], or [`ABSENT`] |
//!
//! It is emitted with [`orbweaver_dynamic::json`]'s writer — its escaper, via
//! `Json::String`'s `Display` — and **not** as a `Json::Object`, because that is
//! a `BTreeMap` and would sort the keys into `caller, decision, operation, …`.
//! Sorted is not the order the decision fixed, so the object is written out key
//! by key here. Values go through the escaper because `target` and `operation`
//! are agent-influenced strings; keys go through it too, so no reader has to
//! trust that a key was chosen safely.
//!
//! # This is not a second audit format
//!
//! [`crate::guard::audit_entry`] stays the one audit format, and a trace line is
//! not one. They answer different questions and both have to stay legible: the
//! audit line is the **decision** — who asked for what, and why it was refused,
//! in a line an operator reads — while the trace record is the **call**, joined
//! to the ledger by `session` and machine-read by whatever renders it. The
//! `decision` column is the seam: it carries the same four tokens the audit line
//! carries, lower-cased, so the two records cannot disagree about what happened.
//! [`Decision::audit_token`] is that correspondence written down rather than
//! remembered, and a test pins every arm of it.
//!
//! # There is no clock here, and that is the feature
//!
//! `ts` comes from the caller. The interceptor chain has no clock, this batch
//! does not add one, and the sink takes the time it is given — which is what
//! makes trace replay deterministic and is the same discipline the residency
//! machine keeps (`PLAN-DEFERRED.md` §3, D004's table). A host with a clock
//! calls [`Trace::stamp`] before each call; a host without one leaves
//! [`Timestamp::unstamped`] and every line reads `"ts":"-"`, which is honest and
//! reproducible rather than plausible and not.
//!
//! D004 also settles the field that is *not* here: **no duration**. Adding one
//! needs a clock, and a duration nobody can reproduce is worse than an absent
//! one. When a batch needs timing it takes the clock as an argument and says so.
//!
//! # A credential cannot reach a line
//!
//! Modelled on [`crate::identity::audit_line`], which took a `&Caller` and an
//! `&Assertion` precisely so that *there is no argument that could carry a
//! password*. The same property here, in three parts that hold together:
//!
//! 1. [`SpanRecord`]'s fields are private and the struct is only ever built in
//!    [`Trace::record`], in this module. There is no public constructor to hand
//!    a secret to.
//! 2. [`Trace::record`]'s only inputs are a [`CallContext`], a [`Decision`], a
//!    stage name and an outcome. `CallContext::caller` is an
//!    [`Option<&Caller>`](crate::identity::Caller), and `Caller` holds a
//!    principal, scopes and an expiry — the credential is *absent from the
//!    type*, which is §4.8's hygiene rule made structural one layer down.
//! 3. A stage **can** see the arguments — [`CallContext::arguments`] carries
//!    them unmapped for [`crate::interceptor::SEAT_SAFETY_CONTENT`] — and a
//!    record still cannot, because [`SpanRecord`] has no field for a value and
//!    [`Trace::record`] has no parameter that could carry one. A refusal
//!    contributes only its *stage name* here; the free prose a stage writes
//!    into [`crate::policy::Denied::Intercepted`] reaches this module not at
//!    all, and the audit ledger only as the stage's name (see
//!    `crate::guard::audit_reason`). The blindness was the old argument for
//!    this line and it no longer holds; the shape of the record is the
//!    argument, and it always was the stronger one.
//!
//! `a_secret_in_a_session_reaches_no_trace_line` runs a real session against a
//! caller carrying secrets in its scopes, an object key and the call arguments,
//! and asserts none of them appear — with the principal asserted *present* in
//! the same test, so that a sink emitting nothing cannot pass it.
//! `crate::interceptor`'s `an_argument_a_content_stage_saw_cannot_reach_the_ledger`
//! is the harder case on the same property: a stage that has the value and is
//! trying to write it down.

use std::io;

use orbweaver_dynamic::json::Json;

use crate::guard::{
    DECISION_ALLOW, DECISION_DRY_RUN_ALLOW, DECISION_DRY_RUN_REFUSE, DECISION_REFUSE, NO_PERMISSION,
};
use crate::interceptor::CallContext;

/// The value of a field the chain has nothing to put in: no caller, no refusing
/// stage, no outcome it was told about, no timestamp the host supplied.
///
/// One marker rather than four, and never an empty string: a reader that cannot
/// tell "absent" from "the empty principal" will eventually be asked to.
pub const ABSENT: &str = "-";

/// `outcome` for a call that completed.
pub const OUTCOME_OK: &str = "ok";

/// `outcome` for a call a **policy** stage refused: the system exception the
/// refusal becomes.
///
/// This is not a guess. [`crate::guard::Guarded`] turns a policy refusal into
/// exactly this repository id, which is what a stub's caller sees; the dynamic
/// path renders the same policy fact as a `ToolError`. The audit line carries
/// the *why*, and this field carries what the caller was told.
///
/// It is not the only refusal outcome: a spent [`crate::quota`] budget that a
/// later window renews is [`crate::guard::TRANSIENT`], because a retry may
/// succeed. [`crate::guard::refusal_id`] is the one place that chooses between
/// them, for the trace and for the exception alike.
pub const OUTCOME_REFUSED: &str = NO_PERMISSION;

/// Which path a call took, which is what the promotion policy reasons about.
///
/// Supplied by the host at [`Trace::new`] rather than inferred, because the
/// chain genuinely does not know: [`crate::interceptor::Chain::standard`] builds
/// the identical stack for [`crate::Bridge`] and for [`crate::guard::Guarded`],
/// and that sameness is the property §7.4 I4 depends on. A stage that guessed
/// would be inventing the one fact this column exists to record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallPath {
    /// A generated stub through [`crate::guard::Guarded`].
    Static,
    /// `invoke_operation` through [`crate::Bridge`].
    Dynamic,
}

impl CallPath {
    /// The token as it appears in the record.
    pub fn as_str(self) -> &'static str {
        match self {
            CallPath::Static => "static",
            CallPath::Dynamic => "dynamic",
        }
    }
}

impl std::fmt::Display for CallPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the gate decided — the audit line's vocabulary, closed to four values.
///
/// A closed enum rather than a `&str` so that a stage cannot invent a fifth
/// decision, and [`Decision::audit_token`] so that the trace token and the audit
/// token are one mapping instead of two conventions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Every gate proceeded and the call was made.
    Allow,
    /// A stage refused and the call did not happen.
    Refuse,
    /// A dry run every gate would have proceeded on. **No call was made.**
    DryRunAllow,
    /// A dry run a stage would have refused. **No call was made.**
    DryRunRefuse,
}

impl Decision {
    /// The token as it appears in the record.
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Refuse => "refuse",
            Decision::DryRunAllow => "dryrun-allow",
            Decision::DryRunRefuse => "dryrun-refuse",
        }
    }

    /// The same decision as the audit line spells it.
    ///
    /// The two records must agree about what happened, so the correspondence is
    /// a function rather than a convention; `the_trace_vocabulary_is_the_audit_
    /// vocabulary_lower_cased` walks every arm.
    pub fn audit_token(self) -> &'static str {
        match self {
            Decision::Allow => DECISION_ALLOW,
            Decision::Refuse => DECISION_REFUSE,
            Decision::DryRunAllow => DECISION_DRY_RUN_ALLOW,
            Decision::DryRunRefuse => DECISION_DRY_RUN_REFUSE,
        }
    }

    /// Whether this describes a call that never happened.
    ///
    /// The same distinction [`crate::guard::is_hypothetical`] draws over the
    /// audit line, and drawn from the same place — a hypothetical must never be
    /// counted as a call.
    pub fn is_hypothetical(self) -> bool {
        crate::guard::is_hypothetical(self.audit_token())
    }
}

impl std::fmt::Display for Decision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The `ts` field: RFC 3339 UTC, **from the caller**.
///
/// A newtype rather than a `String` so that the argument cannot be filled in by
/// accident from something that is not a time, and so that
/// [`Timestamp::unstamped`] is a stated choice rather than an empty string that
/// might be a bug. Nothing here reads a clock; see the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timestamp(String);

impl Timestamp {
    /// The time the host says it is, in RFC 3339 UTC.
    ///
    /// Not validated, and deliberately: a sink that rejected a host's clock
    /// format would be a second opinion about time in a module whose whole
    /// point is that it has none.
    pub fn new(ts: impl Into<String>) -> Self {
        Self(ts.into())
    }

    /// No time at all — [`ABSENT`], for a host with no clock in scope.
    ///
    /// This is the default a harness gets, and it is why two runs of the same
    /// calls produce byte-identical output.
    pub fn unstamped() -> Self {
        Self(ABSENT.to_owned())
    }

    /// The text as it appears in the record.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One line of the trace: D004's table, in D004's order.
///
/// Fields are private and there is no public constructor. The only place one is
/// built is [`Trace::record`], whose arguments cannot carry a credential — see
/// the module docs for the three parts of that argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanRecord<'a> {
    ts: &'a str,
    session: &'a str,
    caller: &'a str,
    target: &'a str,
    operation: &'a str,
    decision: Decision,
    stage: &'a str,
    path: CallPath,
    outcome: &'a str,
}

impl<'a> SpanRecord<'a> {
    /// RFC 3339 UTC as the host supplied it, or [`ABSENT`].
    pub fn ts(&self) -> &'a str {
        self.ts
    }

    /// The MCP session id, which is what joins this line to the audit ledger.
    pub fn session(&self) -> &'a str {
        self.session
    }

    /// The principal, or [`ABSENT`]. Never a credential.
    pub fn caller(&self) -> &'a str {
        self.caller
    }

    /// The repository id, or — for an unresolved handle — the handle as
    /// presented, which is what the audit line has always named for that case.
    pub fn target(&self) -> &'a str {
        self.target
    }

    /// The operation.
    pub fn operation(&self) -> &'a str {
        self.operation
    }

    /// What the gate decided.
    pub fn decision(&self) -> Decision {
        self.decision
    }

    /// The stage that refused, or [`ABSENT`].
    pub fn stage(&self) -> &'a str {
        self.stage
    }

    /// Static or dynamic.
    pub fn path(&self) -> CallPath {
        self.path
    }

    /// [`OUTCOME_OK`], [`OUTCOME_REFUSED`], [`crate::guard::TRANSIENT`] for a
    /// spent quota, or [`ABSENT`].
    pub fn outcome(&self) -> &'a str {
        self.outcome
    }

    /// The record as one JSON object, keys in D004's order, no newline.
    pub fn to_line(&self) -> String {
        // Nine members, each at most a few dozen bytes in the common case.
        let mut out = String::with_capacity(256);
        out.push('{');
        member(&mut out, "ts", self.ts, true);
        member(&mut out, "session", self.session, false);
        member(&mut out, "caller", self.caller, false);
        member(&mut out, "target", self.target, false);
        member(&mut out, "operation", self.operation, false);
        member(&mut out, "decision", self.decision.as_str(), false);
        member(&mut out, "stage", self.stage, false);
        member(&mut out, "path", self.path.as_str(), false);
        member(&mut out, "outcome", self.outcome, false);
        out.push('}');
        out
    }
}

impl std::fmt::Display for SpanRecord<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_line())
    }
}

/// One `"key":"value"` member, both halves through the first-party escaper.
///
/// `Json::String`'s `Display` is `orbweaver_dynamic::json`'s `write_string`,
/// which is RFC 8259 §7 — every control character escaped and nothing else. That
/// is what keeps one record on one line no matter what an agent names an
/// operation.
fn member(out: &mut String, key: &str, value: &str, first: bool) {
    if !first {
        out.push(',');
    }
    out.push_str(&Json::String(key.to_owned()).to_string());
    out.push(':');
    out.push_str(&Json::String(value.to_owned()).to_string());
}

/// Where span records go.
///
/// First-party, and no type from outside this workspace appears in it — that is
/// the property that keeps D004 reversible. An implementation may write, count,
/// forward or drop; it may not fail loudly, because a trace that can break a
/// call path is a worse instrument than no trace. [`JsonLines`] counts its write
/// failures instead, so an operator can still tell.
pub trait TelemetrySink {
    /// Take one record. Called once per decided call, from the telemetry stage's
    /// unwinding.
    fn emit(&mut self, record: &SpanRecord<'_>);
}

/// The no-op sink: takes every record and keeps none.
///
/// The default for a deployment that wants the wiring in place and the output
/// off. Note that a chain with **no** [`Trace`] at all does not even build a
/// record — this type is for the case where a host wants the stage configured
/// and silent, which is a different statement from an absent one.
#[derive(Debug, Default, Clone, Copy)]
pub struct Discard;

impl TelemetrySink for Discard {
    fn emit(&mut self, _record: &SpanRecord<'_>) {}
}

/// JSON lines to any [`io::Write`]: a file, stderr, a pipe, a `Vec<u8>` in a
/// test.
///
/// One object per line, flushed per line. Flushing every line costs a syscall
/// per decision and buys the property a trace is *for*: the lines are on disk
/// when the process is killed, which is exactly when somebody goes looking for
/// them.
#[derive(Debug)]
pub struct JsonLines<W: io::Write> {
    out: W,
    written: u64,
    errors: u64,
}

impl<W: io::Write> JsonLines<W> {
    /// A sink writing to `out`.
    pub fn new(out: W) -> Self {
        Self { out, written: 0, errors: 0 }
    }

    /// How many lines were written.
    pub fn written(&self) -> u64 {
        self.written
    }

    /// How many lines could **not** be written.
    ///
    /// Not zero by construction, and reported rather than hidden: a full disk
    /// must not stop a call, and a harness that reported green over a sink that
    /// wrote nothing would be the "unmeasured check greened as a pass" the
    /// harness rules forbid.
    pub fn errors(&self) -> u64 {
        self.errors
    }

    /// The writer back, for a test that wants to read what it captured.
    pub fn into_inner(self) -> W {
        self.out
    }
}

impl<W: io::Write> TelemetrySink for JsonLines<W> {
    fn emit(&mut self, record: &SpanRecord<'_>) {
        let mut line = record.to_line();
        line.push('\n');
        match self.out.write_all(line.as_bytes()).and_then(|()| self.out.flush()) {
            Ok(()) => self.written += 1,
            Err(_) => self.errors += 1,
        }
    }
}

/// A sink plus the three things a record needs that the chain does not know:
/// the time, the session, and which path this is.
///
/// Installed on a chain with [`crate::interceptor::Chain::trace`], which puts it
/// on the telemetry stage and answers `false` if there is no such stage — D004's
/// "absence is reported, never greened".
pub struct Trace {
    ts: Timestamp,
    session: String,
    path: CallPath,
    sink: Box<dyn TelemetrySink>,
    emitted: u64,
}

impl std::fmt::Debug for Trace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The sink is `dyn` and has no `Debug` bound — requiring one would put a
        // constraint on every implementation for the benefit of this line.
        f.debug_struct("Trace")
            .field("ts", &self.ts)
            .field("session", &self.session)
            .field("path", &self.path)
            .field("emitted", &self.emitted)
            .finish_non_exhaustive()
    }
}

impl Trace {
    /// A trace for one session on one path, stamped with the time **the caller
    /// supplies**.
    ///
    /// `ts` is an argument and not a clock read, and it is required rather than
    /// defaulted so that a host cannot acquire a timestamp without deciding
    /// where it came from. [`Timestamp::unstamped`] is the decision for a host
    /// that has no clock in scope, which is every host in this workspace today.
    pub fn new(
        session: impl Into<String>,
        path: CallPath,
        ts: Timestamp,
        sink: impl TelemetrySink + 'static,
    ) -> Self {
        Self { ts, session: session.into(), path, sink: Box::new(sink), emitted: 0 }
    }

    /// Sets the time the next records carry.
    ///
    /// The only way `ts` ever changes. A host with a clock calls this before a
    /// call; a host without one never calls it and every line reads `"ts":"-"`.
    pub fn stamp(&mut self, ts: Timestamp) {
        self.ts = ts;
    }

    /// The time the next record will carry.
    pub fn ts(&self) -> &Timestamp {
        &self.ts
    }

    /// The session id every record carries.
    pub fn session(&self) -> &str {
        &self.session
    }

    /// The path every record carries.
    pub fn path(&self) -> CallPath {
        self.path
    }

    /// How many records have been handed to the sink.
    ///
    /// What the sink did with them is the sink's business; this is the count the
    /// chain can vouch for.
    pub fn emitted(&self) -> u64 {
        self.emitted
    }

    /// Builds one record and hands it to the sink.
    ///
    /// The **only** place a [`SpanRecord`] is constructed. Its arguments are a
    /// call context, a closed decision, a stage name and an outcome — nothing
    /// that could carry a credential, by the same reasoning that made
    /// [`crate::identity::audit_line`] safe.
    pub fn record(
        &mut self,
        ctx: &CallContext<'_>,
        decision: Decision,
        stage: Option<&str>,
        outcome: &str,
    ) {
        // Destructured so the sink can be borrowed mutably while the timestamp
        // and the session id are borrowed for the record that names them.
        let Trace { ts, session, path, sink, emitted } = self;
        let record = SpanRecord {
            ts: ts.as_str(),
            session: session.as_str(),
            // The principal, exactly as the audit line takes it. `Caller` has no
            // credential field, so this is the whole of what identity can
            // contribute to a line.
            caller: ctx.caller.map_or(ABSENT, |c| c.principal.as_str()),
            target: ctx.target,
            operation: ctx.operation,
            decision,
            stage: stage.unwrap_or(ABSENT),
            path: *path,
            outcome,
        };
        sink.emit(&record);
        *emitted += 1;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use orbweaver_registry::Registry;

    use super::*;
    use crate::identity::Caller;
    use crate::interceptor::Chain;
    use crate::policy::{Approval, Exposure};

    /// A sink that keeps the lines, for asserting on them.
    #[derive(Default)]
    struct Captured(Rc<RefCell<Vec<String>>>);

    impl TelemetrySink for Captured {
        fn emit(&mut self, record: &SpanRecord<'_>) {
            self.0.borrow_mut().push(record.to_line());
        }
    }

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
          //@ ai_effect: idempotent
          void deposit(in long cents);
        };
      };";

    const ACCOUNT: &str = "IDL:bank/Account:1.0";

    fn trace_into(lines: &Rc<RefCell<Vec<String>>>, ts: Timestamp) -> Trace {
        Trace::new("s-1", CallPath::Dynamic, ts, Captured(Rc::clone(lines)))
    }

    /// D004's table, key for key and **in order**, against a line produced by
    /// the real stage. The console is a batch that cannot see this one, so the
    /// contract is asserted as a whole string rather than field by field.
    #[test]
    fn the_line_is_the_decision_s_table_in_the_decision_s_order() {
        let reg = registry(IDL);
        let lines = Rc::new(RefCell::new(Vec::new()));
        let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
        assert!(chain.trace(trace_into(&lines, Timestamp::new("2026-08-14T09:00:00Z"))));

        let alice = Caller::new("alice").with_scope("accounts:write");
        let ctx = CallContext {
            registry: &reg,
            caller: Some(&alice),
            target: ACCOUNT,
            operation: "balance",
            approval: Approval::default(),
            arguments: None,
        };
        chain.run(&ctx).expect("exposed");
        chain.completed(&ctx, true);

        assert_eq!(
            lines.borrow().as_slice(),
            [concat!(
                r#"{"ts":"2026-08-14T09:00:00Z","session":"s-1","caller":"alice","#,
                r#""target":"IDL:bank/Account:1.0","operation":"balance","decision":"allow","#,
                r#""stage":"-","path":"dynamic","outcome":"ok"}"#
            )]
        );

        // And the order is not an accident of the literal above: every key
        // appears exactly once, in this sequence, with nothing else present.
        let line = lines.borrow()[0].clone();
        let keys = [
            "ts",
            "session",
            "caller",
            "target",
            "operation",
            "decision",
            "stage",
            "path",
            "outcome",
        ];
        let mut at = 0;
        for key in keys {
            let needle = format!("\"{key}\":");
            let found =
                line[at..].find(&needle).unwrap_or_else(|| panic!("{key} after {at}: {line}"));
            at += found + needle.len();
        }
        assert_eq!(line.matches("\":").count(), keys.len(), "an unexpected member: {line}");
    }

    /// The seam between the two records: same vocabulary, different case, one
    /// mapping. Two conventions is how the audit format drifted into two in the
    /// first place.
    #[test]
    fn the_trace_vocabulary_is_the_audit_vocabulary_lower_cased() {
        for decision in
            [Decision::Allow, Decision::Refuse, Decision::DryRunAllow, Decision::DryRunRefuse]
        {
            assert_eq!(decision.as_str(), decision.audit_token().to_ascii_lowercase());
        }
        assert!(!Decision::Allow.is_hypothetical());
        assert!(!Decision::Refuse.is_hypothetical());
        assert!(Decision::DryRunAllow.is_hypothetical());
        assert!(Decision::DryRunRefuse.is_hypothetical());
    }

    /// An operation name is agent-influenced text. One record is one line, or
    /// every reader of the trace is parsing a format the emitter can break.
    #[test]
    fn a_hostile_operation_name_cannot_break_the_line() {
        let reg = registry(IDL);
        let lines = Rc::new(RefCell::new(Vec::new()));
        let mut chain = Chain::standard(Exposure::nothing());
        assert!(chain.trace(trace_into(&lines, Timestamp::unstamped())));

        let nasty = "bal\"ance\n{\"decision\":\"allow\"}\r\u{1}";
        let ctx = CallContext {
            registry: &reg,
            caller: None,
            target: ACCOUNT,
            operation: nasty,
            approval: Approval::default(),
            arguments: None,
        };
        chain.run(&ctx).unwrap_err();

        let line = lines.borrow()[0].clone();
        assert_eq!(lines.borrow().len(), 1);
        assert!(!line.contains('\n') && !line.contains('\r'), "{line}");
        let parsed = Json::parse(&line).unwrap_or_else(|e| panic!("{e}: {line}"));
        assert_eq!(parsed.get("operation").and_then(Json::as_str), Some(nasty));
        // The injected object is a *value*, not a second decision.
        assert_eq!(parsed.get("decision").and_then(Json::as_str), Some("refuse"));
    }

    /// A trace that cannot be written must not take the call path down with it,
    /// and must not report success either.
    #[test]
    fn a_sink_that_cannot_write_counts_the_failure_and_the_call_survives() {
        struct FullDisk;
        impl io::Write for FullDisk {
            fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
                Err(io::Error::other("no space left on device"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let reg = registry(IDL);
        let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
        assert!(chain.trace(Trace::new(
            "s-1",
            CallPath::Static,
            Timestamp::unstamped(),
            JsonLines::new(FullDisk),
        )));
        let ctx = CallContext {
            registry: &reg,
            caller: None,
            target: ACCOUNT,
            operation: "balance",
            approval: Approval::default(),
            arguments: None,
        };
        chain.run(&ctx).expect("the gate is unaffected by the sink");
        chain.completed(&ctx, true);
        assert_eq!(chain.trace_mut().expect("installed").emitted(), 1);
        assert_eq!(chain.stats().calls(ACCOUNT, "balance"), 1, "the counters are untouched");
    }

    /// The `static` column is not decoration: a generated stub's guard takes a
    /// trace through the chain it already exposes, and its records say so.
    ///
    /// This is also the whole of the static path's wiring — [`CallPath`] is a
    /// host's statement rather than an inference, so no new constructor was
    /// needed on [`crate::guard::Guarded`] to make it true.
    #[test]
    fn the_static_path_traces_as_static_through_the_guard_s_own_chain() {
        use orbweaver_cdr::{Encoder, Endian};
        use orbweaver_giop::{Error as GiopError, Invoker, Reply};

        /// An invoker that answers nothing; the trace is about the gate.
        struct Dead;
        impl Invoker for Dead {
            fn endian(&self) -> Endian {
                Endian::Big
            }
            fn invoke<F: Fn(&mut Encoder)>(
                &mut self,
                _operation: &str,
                _write_args: F,
            ) -> Result<Reply, GiopError> {
                Err(GiopError::ConnectionClosed)
            }
            fn invoke_oneway<F: Fn(&mut Encoder)>(
                &mut self,
                _operation: &str,
                _write_args: F,
            ) -> Result<(), GiopError> {
                Ok(())
            }
        }

        let reg = registry(IDL);
        let lines = Rc::new(RefCell::new(Vec::new()));
        let mut guarded = crate::guard::Guarded::assemble(
            Dead,
            &reg,
            Exposure::nothing().allow_operation(ACCOUNT, "balance"),
            Some(Caller::new("alice")),
            ACCOUNT.to_owned(),
            Approval::default(),
        );
        assert!(guarded.chain_mut().trace(Trace::new(
            "s-static",
            CallPath::Static,
            Timestamp::new("2026-08-14T09:00:00Z"),
            Captured(Rc::clone(&lines)),
        )));

        // Allowed by the gate, then failed on the wire: still an `allow`, with
        // an outcome the chain was not told the name of.
        let _ = guarded.invoke("balance", |_| {});
        // Refused by the gate, so nothing reached the invoker.
        let _ = guarded.invoke("deposit", |_| {});

        let lines = lines.borrow().clone();
        assert_eq!(lines.len(), 2, "{lines:#?}");
        assert!(lines.iter().all(|l| l.contains("\"path\":\"static\"")), "{lines:#?}");
        assert!(lines[0].contains("\"decision\":\"allow\""), "{}", lines[0]);
        assert!(lines[0].contains("\"outcome\":\"-\""), "{}", lines[0]);
        assert!(lines[1].contains("\"decision\":\"refuse\""), "{}", lines[1]);
        assert!(lines[1].contains("\"stage\":\"authz.exposure\""), "{}", lines[1]);
        assert!(lines[1].contains(&format!("\"outcome\":\"{NO_PERMISSION}\"")), "{}", lines[1]);
    }

    /// The half of D004's `outcome` column that used to be unreachable: an
    /// allowed call that failed now **names what failed it**, and the audit
    /// line for the same call is unchanged to the byte.
    ///
    /// Both halves are asserted together on purpose. The trace and the ledger
    /// answer different questions — what happened to the call, and what the
    /// policy decided — and the failure mode of widening one is that the other
    /// quietly starts carrying transport facts it was never meant to.
    #[test]
    fn a_system_exception_names_itself_in_the_outcome_and_leaves_the_audit_line_alone() {
        use orbweaver_cdr::{Encoder, Endian};
        use orbweaver_giop::{Error as GiopError, Invoker, Reply};

        const BAD_OPERATION: &str = "IDL:omg.org/CORBA/BAD_OPERATION:1.0";

        /// A target whose ORB refuses: the first call raises a system
        /// exception, every later one drops the connection instead — two
        /// failures the chain used to render identically.
        struct Refusing {
            calls: usize,
        }
        impl Invoker for Refusing {
            fn endian(&self) -> Endian {
                Endian::Big
            }
            fn invoke<F: Fn(&mut Encoder)>(
                &mut self,
                _operation: &str,
                _write_args: F,
            ) -> Result<Reply, GiopError> {
                self.calls += 1;
                if self.calls == 1 {
                    return Err(GiopError::SystemException {
                        id: BAD_OPERATION.to_owned(),
                        minor: 0,
                        completed: 1,
                    });
                }
                Err(GiopError::ConnectionClosed)
            }
            fn invoke_oneway<F: Fn(&mut Encoder)>(
                &mut self,
                _operation: &str,
                _write_args: F,
            ) -> Result<(), GiopError> {
                Ok(())
            }
        }

        let reg = registry(IDL);
        let lines = Rc::new(RefCell::new(Vec::new()));
        let mut guarded = crate::guard::Guarded::assemble(
            Refusing { calls: 0 },
            &reg,
            Exposure::nothing().allow_operation(ACCOUNT, "balance"),
            Some(Caller::new("alice")),
            ACCOUNT.to_owned(),
            Approval::default(),
        );
        assert!(guarded.chain_mut().trace(Trace::new(
            "s-raised",
            CallPath::Static,
            Timestamp::new("2026-08-14T09:00:00Z"),
            Captured(Rc::clone(&lines)),
        )));

        let _ = guarded.invoke("balance", |_| {});
        let _ = guarded.invoke("balance", |_| {});

        let lines = lines.borrow().clone();
        assert_eq!(lines.len(), 2, "{lines:#?}");
        // 1. The exception the target raised, in D004's own record, in order.
        assert_eq!(
            lines[0],
            concat!(
                r#"{"ts":"2026-08-14T09:00:00Z","session":"s-raised","caller":"alice","#,
                r#""target":"IDL:bank/Account:1.0","operation":"balance","decision":"allow","#,
                r#""stage":"-","path":"static","outcome":"IDL:omg.org/CORBA/BAD_OPERATION:1.0"}"#
            ),
        );
        // 2. And a failure with nothing to name it is still `-` — which is now
        //    an absence rather than a column nobody plumbed.
        assert!(lines[1].contains(r#""outcome":"-""#), "{}", lines[1]);
        assert!(lines[1].contains(r#""decision":"allow""#), "a failed call was still allowed");

        // The ledger is untouched by any of it: same format, same fields, no
        // transport facts. Both calls are plain ALLOWs and equal as strings.
        assert_eq!(
            guarded.audit(),
            [
                "ALLOW caller=alice target=IDL:bank/Account:1.0 operation=balance",
                "ALLOW caller=alice target=IDL:bank/Account:1.0 operation=balance",
            ]
        );
        // And both count as failures, however well named.
        assert_eq!(guarded.stats().calls(ACCOUNT, "balance"), 2);
        assert_eq!(guarded.stats().failures(ACCOUNT, "balance"), 2);
    }

    /// JSON lines to an `io::Write`, which is what the harness captures.
    #[test]
    fn the_json_lines_sink_writes_one_line_per_record_and_counts_them() {
        let reg = registry(IDL);
        let mut chain = Chain::standard(Exposure::nothing());
        assert!(chain.trace(Trace::new(
            "s-1",
            CallPath::Dynamic,
            Timestamp::new("2026-08-14T09:00:00Z"),
            JsonLines::new(Vec::<u8>::new()),
        )));
        let ctx = CallContext {
            registry: &reg,
            caller: None,
            target: ACCOUNT,
            operation: "balance",
            approval: Approval::default(),
            arguments: None,
        };
        chain.run(&ctx).unwrap_err();
        chain.dry_run(&ctx);
        assert_eq!(chain.trace_mut().expect("installed").emitted(), 2);
    }

    /// The no-op sink is a configured silence, not an absence.
    #[test]
    fn the_discard_sink_keeps_nothing_and_still_counts() {
        let reg = registry(IDL);
        let mut chain = Chain::standard(Exposure::nothing());
        assert!(
            chain.trace(Trace::new("s-1", CallPath::Dynamic, Timestamp::unstamped(), Discard,))
        );
        let ctx = CallContext {
            registry: &reg,
            caller: None,
            target: ACCOUNT,
            operation: "balance",
            approval: Approval::default(),
            arguments: None,
        };
        chain.run(&ctx).unwrap_err();
        assert_eq!(chain.trace_mut().expect("installed").emitted(), 1);
    }

    /// §4.8's hygiene rule, tested the way the transcript-leak tests test it:
    /// run a real session whose caller carries secrets and assert that none of
    /// them appear in any emitted line — with the principal asserted *present*,
    /// so a sink that emitted nothing could not pass.
    #[test]
    fn a_secret_in_a_session_reaches_no_trace_line() {
        use orbweaver_giop::{Connection, IiopProfile, Ior, Version};

        use crate::session::Session;

        let reg: &'static Registry = Box::leak(Box::new(registry(IDL)));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
        let ior = Ior {
            type_id: ACCOUNT.into(),
            // The object key is address material: it is a secret of the same
            // kind a handle exists to hide.
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: "127.0.0.1".into(),
                port: listener.local_addr().expect("bound").port(),
                object_key: b"objectkey-s3cret-cafe".to_vec(),
                components: Vec::new(),
            }],
        };
        let conn = Connection::connect(&ior, std::time::Duration::from_secs(5)).expect("dials");

        let lines = Rc::new(RefCell::new(Vec::new()));
        let exposure = Exposure::nothing().allow_operation(ACCOUNT, "deposit");
        let mut session = Session::new(reg, exposure, conn, "s-leak").on_behalf_of(
            Caller::new("alice@example.com")
                .with_scope("accounts:write")
                .with_scope("accounts:write-s3cret-scope"),
        );
        assert!(
            session.bridge().chain_mut().trace(Trace::new(
                "s-leak",
                CallPath::Dynamic,
                Timestamp::new("2026-08-14T09:00:00Z"),
                Captured(Rc::clone(&lines)),
            )),
            "the trace must land on the telemetry stage"
        );
        let handle = session.bridge().handles().issue_checked(&ior).expect("issued");

        session.handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);
        // Allowed by policy; the argument carries a secret and never reaches a
        // stage, because the chain runs before the arguments are decoded.
        session.handle_line(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"invoke_operation","arguments":{{"handle":"{handle}","operation":"deposit","arguments":{{"cents":"pin-s3cret-4242"}}}}}}}}"#
        ));
        // Refused by policy, same secrets in flight.
        session.handle_line(&format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"invoke_operation","arguments":{{"handle":"{handle}","operation":"balance"}}}}}}"#
        ));

        let emitted = lines.borrow().join("\n");
        assert!(!emitted.is_empty(), "a test that captured nothing proves nothing");
        for secret in [
            "s3cret",
            "pin-",
            "objectkey",
            "cafe",
            "write-s3cret-scope",
            "127.0.0.1",
            handle.as_str(),
        ] {
            assert!(!emitted.contains(secret), "{secret:?} reached a trace line:\n{emitted}");
        }
        // The positive control: the record does name the principal.
        assert!(emitted.contains("\"caller\":\"alice@example.com\""), "{emitted}");
        assert!(emitted.contains("\"decision\":\"allow\""), "{emitted}");
        assert!(emitted.contains("\"decision\":\"refuse\""), "{emitted}");
    }
}
