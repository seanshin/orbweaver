//! The guard on the static path: PLAN §7.4 integration point I1.
//!
//! A generated stub is compiled code that calls `invoke("op", …)` directly.
//! Handed a raw `Connection`, it is past the exposure list, past the scope
//! check, past `destructive` approval and past the audit log — §4.7's bypass,
//! recreated in compiled form and now distributed as a build artifact.
//!
//! [`Guarded`] is the same `Invoker` surface with the same checks the dynamic
//! path runs, applied per operation at call time. The stub cannot tell the
//! difference, which is the point: **which side of the trust boundary a stub
//! runs on is decided by what it is handed, not by how it was generated.**
//!
//! A refusal surfaces as a CORBA system exception — what a native guard would
//! raise, so a stub's caller handles policy the way it already handles the
//! target's own refusals. The *why* goes to the audit log, where §4.8 wants it.
//! Which exception is [`refusal_id`]'s single decision: `NO_PERMISSION` for
//! everything the policy decides, `TRANSIENT` for a [`crate::quota`] budget
//! that a later window may renew — because "you may not" and "not right now"
//! are different answers and a retry loop reads the difference off the
//! repository id, not off the audit log.
//!
//! Since F4 the checks themselves live in [`crate::interceptor`]: this file
//! holds the `Invoker` surface and the `NO_PERMISSION` translation, and the
//! four things it used to do inline are §4.5's stack, in order, extensible by a
//! deployment. What `check` does now is build a [`CallContext`] and run the
//! chain.

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_giop::{Connection, Error as GiopError, Invoker, Reply};
use orbweaver_registry::Registry;

use crate::identity::Caller;
use crate::interceptor::{CallContext, Chain};
use crate::policy::{Approval, Denied, Exposure};
use crate::promote::CallStats;

/// The repository id `NO_PERMISSION` travels under.
pub const NO_PERMISSION: &str = "IDL:omg.org/CORBA/NO_PERMISSION:1.0";

/// The repository id `TRANSIENT` travels under: **refused now**.
///
/// The one CORBA refusal that invites a retry, which is exactly why a
/// consumption limit ([`crate::quota`]) must not borrow `NO_PERMISSION`'s
/// spelling. `orbweaver-gen`'s servant runtime already draws the same
/// distinction for a servant's own refusals, so a stub's caller handles a
/// bridge-side budget with the code it already has for a target-side one.
pub const TRANSIENT: &str = "IDL:omg.org/CORBA/TRANSIENT:1.0";

/// `<nobody>`: the caller field of a record about a session nobody is signed
/// into.
///
/// One spelling, so that an audit line, a quota's budget key and an operator's
/// grep pattern all mean the same thing by it.
pub const NOBODY: &str = "<nobody>";

/// Which system exception a refusal reaches a caller as.
///
/// The **one** place the mapping lives, so that the stub's exception, the
/// trace's `outcome` and the dry run's classification cannot come to different
/// conclusions about whether waiting would help. Everything a policy decides is
/// `NO_PERMISSION`; a spent budget that renews is `TRANSIENT`.
pub fn refusal_id(why: &Denied) -> &'static str {
    if why.is_transient() { TRANSIENT } else { NO_PERMISSION }
}

/// The decision field of an audit line for a call the policy allowed.
pub const DECISION_ALLOW: &str = "ALLOW";
/// The decision field of an audit line for a call the policy refused.
pub const DECISION_REFUSE: &str = "REFUSE";
/// The decision field for a **dry run** the policy would have allowed. No call
/// was made; see [`crate::dryrun`].
pub const DECISION_DRY_RUN_ALLOW: &str = "DRYRUN-ALLOW";
/// The decision field for a **dry run** the policy would have refused. No call
/// was made; see [`crate::dryrun`].
pub const DECISION_DRY_RUN_REFUSE: &str = "DRYRUN-REFUSE";

/// The first field of the **elision marker**: not a decision, but the ledger
/// saying that decisions are missing from it.
///
/// A bounded ledger that drops its oldest lines silently is not an audit
/// ledger — the one thing a reader must be able to do is tell a quiet period
/// apart from a hole. [`crate::interceptor::AuditInterceptor`] therefore spends
/// one of its slots on a line that says how many lines are gone, in the first
/// position where they used to be, and every reader of the log meets it:
/// an operator greps `ELIDED`, [`crate::promote::verify_promotion`] refuses it
/// by name rather than judging a promotion from a gap, and
/// [`crate::interceptor::Chain::audit_dropped`] answers the same number without
/// parsing anything.
///
/// It is deliberately **not** in [`audit_entry`]'s shape: it carries no
/// `caller=` and no `operation=`, so a parser written for decision lines
/// rejects it rather than reading it as one.
pub const DECISION_ELIDED: &str = "ELIDED";

/// The elision marker, in the one format: `ELIDED dropped=<n> why=…`.
///
/// `capacity` is named in the prose because the first question a reader has
/// after "how many are gone" is "gone why", and the answer is a ceiling
/// somebody configured rather than a failure.
pub(crate) fn elided_entry(dropped: u64, capacity: usize) -> String {
    format!(
        "{DECISION_ELIDED} dropped={dropped} why=the audit ledger keeps the newest {capacity} \
         lines and the {dropped} oldest have been dropped"
    )
}

/// How many lines an elision marker says are missing, or `None` for a line that
/// is not one.
///
/// The reader's half of [`elided_entry`], public because the marker is meant to
/// be read outside this crate — a console, a harness, an operator's script —
/// and a format with only a writer is a format everybody else reverse-engineers.
pub fn elided_count(line: &str) -> Option<u64> {
    let mut fields = line.split_whitespace();
    if fields.next()? != DECISION_ELIDED {
        return None;
    }
    fields.find_map(|f| f.strip_prefix("dropped=")).and_then(|n| n.parse().ok())
}

/// Whether an audit line's decision field describes a call that never
/// happened.
///
/// The one place the distinction is decided, so that every reader of the log
/// draws it the same way. [`crate::promote::verify_promotion`] uses it to
/// refuse a hypothetical line outright: a promotion gated on a call nobody
/// made would be comparing a prediction against a measurement.
pub fn is_hypothetical(decision: &str) -> bool {
    decision == DECISION_DRY_RUN_ALLOW || decision == DECISION_DRY_RUN_REFUSE
}

/// One audit line, in **the** format:
/// `ALLOW caller=<principal> target=<id> operation=<op>`, with
/// ` why=<reason>` appended on a `REFUSE`. An absent caller is `<nobody>`.
///
/// `decision` is one of [`DECISION_ALLOW`], [`DECISION_REFUSE`] or the dry-run
/// pair — the field that distinguishes a call from a question, in a format
/// that is otherwise identical for both.
///
/// The single place the format lives. Since F4 there is a single place it is
/// *called* from too — [`crate::interceptor::AuditInterceptor`], the §4.5 audit
/// stage, which both [`Guarded`] and [`crate::Bridge`] run. That is what makes
/// the static and dynamic paths' lines for the same call *equal as strings*
/// rather than merely similar — the property §7.4 I4's gate compares, and the
/// reason `crate::promote`'s parser needs no second format. Change it here or
/// nowhere.
///
/// Like [`crate::identity::audit_line`], a line names the principal and the
/// operation and can carry no credential material: nothing here holds one.
pub(crate) fn audit_entry(
    decision: &str,
    caller: Option<&Caller>,
    target: &str,
    operation: &str,
    why: Option<&str>,
) -> String {
    let caller = caller.map_or(NOBODY, |c| c.principal.as_str());
    let mut line = format!("{decision} caller={caller} target={target} operation={operation}");
    if let Some(why) = why {
        line.push_str(" why=");
        line.push_str(why);
    }
    line
}

/// An invoker that checks policy per operation and records every decision.
///
/// Owns its policy context rather than borrowing the bridge, so holding one
/// does not freeze the session that issued it — and so it *cannot* be assembled
/// from one session's connection and another session's policy, which is the
/// confused-deputy pairing R13 names. The only constructor is
/// [`crate::Bridge::connect_static`].
pub struct Guarded<'r, C: Invoker = Connection> {
    conn: C,
    registry: &'r Registry,
    caller: Option<Caller>,
    /// The repository id the handle named. From the capability table, never
    /// from the stub: a stub asserting its own interface id would be asserting
    /// its own permissions.
    id: String,
    approval: Approval,
    /// §4.5's stack, holding the exposure, the audit lines and the counters.
    chain: Chain,
}

/// The refusal a stub sees. The reason is not in it, deliberately: it is in the
/// audit log, which is where §4.8 wants it.
///
/// Two fields are decided here rather than defaulted:
///
/// - the **repository id** comes from [`refusal_id`], so a spent budget arrives
///   as `TRANSIENT` and everything else as `NO_PERMISSION`;
/// - the **completion status** is `COMPLETED_NO`, which is §4.11.4's ordinal
///   **1** — `COMPLETED_YES` is 0 (the transposition `orbweaver-giop` fixed in
///   its own encoder). This gate refuses *before* anything reaches the wire, so
///   the operation provably did not run, and saying so is what makes a retry
///   safe. It used to say 0 here, which told every refused caller its call had
///   completed; with `TRANSIENT` now reachable that would be actively
///   dangerous — an invitation to retry attached to a claim that the call
///   already happened.
fn refusal(why: &Denied) -> GiopError {
    GiopError::SystemException { id: refusal_id(why).to_owned(), minor: 0, completed: 1 }
}

impl<'r, C: Invoker> Guarded<'r, C> {
    pub(crate) fn assemble(
        conn: C,
        registry: &'r Registry,
        exposure: Exposure,
        caller: Option<Caller>,
        id: String,
        approval: Approval,
    ) -> Self {
        Self { conn, registry, caller, id, approval, chain: Chain::standard(exposure) }
    }

    /// Every decision this guard has made, oldest first.
    ///
    /// Lines name the principal and the operation and never credential
    /// material — the same rule as [`crate::identity::audit_line`], and for the
    /// same reason: there is nothing here a line *could* leak, because the
    /// guard never holds a credential.
    ///
    /// Bounded: see [`Chain::audit`]. Once anything has been dropped the first
    /// element is the elision marker ([`elided_count`]), so a reader of this
    /// slice alone can tell a short history from a truncated one.
    pub fn audit(&self) -> &[String] {
        self.chain.audit()
    }

    /// How many audit lines this guard has written since it was assembled,
    /// dropped ones included.
    pub fn audit_written(&self) -> u64 {
        self.chain.audit_written()
    }

    /// How many of them the bounded ledger has dropped.
    pub fn audit_dropped(&self) -> u64 {
        self.chain.audit_dropped()
    }

    /// What the guard's telemetry stage counted.
    ///
    /// These counters are the *static* path's and they live and die with this
    /// `Guarded`. PLAN-MOE **IF2** asks for one store, and
    /// [`crate::promote::CallStats::merge`] is how a deployment gets one —
    /// `bridge.absorb_static(guarded.stats())` before the guard is dropped.
    ///
    /// It is not automatic, and that is a decision rather than an omission:
    /// [`crate::promote::PromotionPolicy::recommend`] answers "which dynamic
    /// path has earned a compiled stub", and a static call is evidence that a
    /// path *already has one*. Merging by default would keep a promoted path
    /// looking hot and have the policy recommend promoting it again. So the two
    /// stores answer two questions, and joining them is the caller's act, taken
    /// when the question is traffic rather than promotion.
    pub fn stats(&self) -> &CallStats {
        self.chain.stats()
    }

    /// The chain this guard runs, for a deployment that adds a stage. The
    /// built-ins are already in it; see [`Chain::insert_after`].
    pub fn chain_mut(&mut self) -> &mut Chain {
        &mut self.chain
    }

    /// What this guard *would* do with `operation`, without invoking it.
    ///
    /// The same chain [`Invoker::invoke`] runs, on a [`CallContext`] built the
    /// same way from the same three fields — and `self.conn` is not read. A
    /// generated stub is compiled code and cannot be asked politely what it
    /// intends; this is how an operator asks the guard instead, before handing
    /// the stub out.
    pub fn dry_run(&mut self, operation: &str) -> crate::dryrun::Prediction {
        let ctx = CallContext {
            registry: self.registry,
            caller: self.caller.as_ref(),
            target: self.id.as_str(),
            operation,
            approval: self.approval,
        };
        crate::dryrun::predict(&mut self.chain, &ctx)
    }
}

impl<'r, C: Invoker> Invoker for Guarded<'r, C> {
    fn endian(&self) -> Endian {
        self.conn.endian()
    }

    fn invoke<F: Fn(&mut Encoder)>(
        &mut self,
        operation: &str,
        write_args: F,
    ) -> Result<Reply, GiopError> {
        // Built by hand rather than by a helper method: the context borrows
        // three fields of `self` while the chain borrows a fourth mutably, and
        // a method returning the context would borrow all of `self`.
        let ctx = CallContext {
            registry: self.registry,
            caller: self.caller.as_ref(),
            target: self.id.as_str(),
            operation,
            approval: self.approval,
        };
        if let Err(why) = self.chain.run(&ctx) {
            return Err(refusal(&why));
        }
        let reply = self.conn.invoke(operation, write_args);
        // The whole result, not `is_ok()`: a system or user exception names
        // itself in the trace's `outcome` column, and anything else is a
        // failure the chain was genuinely not told the name of. See
        // `crate::interceptor::CallOutcome`.
        self.chain.completed(&ctx, &reply);
        reply
    }

    fn invoke_oneway<F: Fn(&mut Encoder)>(
        &mut self,
        operation: &str,
        write_args: F,
    ) -> Result<(), GiopError> {
        // Gated like a twoway: a oneway that skipped the chain would make
        // "fire and forget" the way around the guard.
        let ctx = CallContext {
            registry: self.registry,
            caller: self.caller.as_ref(),
            target: self.id.as_str(),
            operation,
            approval: self.approval,
        };
        if let Err(why) = self.chain.run(&ctx) {
            return Err(refusal(&why));
        }
        let sent = self.conn.invoke_oneway(operation, write_args);
        self.chain.completed(&ctx, &sent);
        sent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interceptor::{Interceptor, Outcome};
    use crate::policy::Denied;

    /// An invoker that records what reached it, for proving what did not.
    struct Recorder {
        reached: Vec<String>,
    }

    impl Invoker for Recorder {
        fn endian(&self) -> Endian {
            Endian::Big
        }
        fn invoke<F: Fn(&mut Encoder)>(
            &mut self,
            operation: &str,
            _write_args: F,
        ) -> Result<Reply, GiopError> {
            self.reached.push(operation.to_owned());
            // No Reply can be built outside the wire, so the fake fails after
            // recording; the tests only care what got this far.
            Err(GiopError::ConnectionClosed)
        }
        fn invoke_oneway<F: Fn(&mut Encoder)>(
            &mut self,
            operation: &str,
            _write_args: F,
        ) -> Result<(), GiopError> {
            self.reached.push(operation.to_owned());
            Ok(())
        }
    }

    fn registry() -> Registry {
        let spec = orbweaver_idl::parse(
            "module bank {
               interface Account {
                 //@ ai_effect: read_only
                 long balance();
                 //@ ai_authz: accounts:write
                 void deposit(in long cents);
                 //@ ai_effect: destructive
                 void close();
               };
             };",
        )
        .expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        r
    }

    fn guarded<'r>(
        reg: &'r Registry,
        exposure: Exposure,
        caller: Option<Caller>,
        approval: Approval,
    ) -> Guarded<'r, Recorder> {
        Guarded::assemble(
            Recorder { reached: Vec::new() },
            reg,
            exposure,
            caller,
            "IDL:bank/Account:1.0".to_owned(),
            approval,
        )
    }

    /// The property I1 exists for: a refused operation never reaches the
    /// transport. Refusing after sending would be logging, not guarding.
    #[test]
    fn a_refused_operation_never_reaches_the_wire() {
        let reg = registry();
        let mut g = guarded(&reg, Exposure::nothing(), None, Approval::default());
        let err = g.invoke("balance", |_| {}).unwrap_err();
        assert!(
            matches!(&err, GiopError::SystemException { id, .. } if id == NO_PERMISSION),
            "{err}"
        );
        assert!(g.conn.reached.is_empty(), "the transport saw: {:?}", g.conn.reached);
    }

    #[test]
    fn an_exposed_operation_passes_through_and_is_recorded() {
        let reg = registry();
        let exposure = Exposure::nothing().allow_operation("IDL:bank/Account:1.0", "balance");
        let mut g = guarded(&reg, exposure, Some(Caller::new("alice")), Approval::default());
        let _ = g.invoke("balance", |_| {});
        assert_eq!(g.conn.reached, vec!["balance"]);
        assert_eq!(g.audit().len(), 1);
        assert!(g.audit()[0].starts_with("ALLOW caller=alice"), "{}", g.audit()[0]);
    }

    /// The C×B seam: `ai_authz` written in the contract binds the static path
    /// exactly as it binds the dynamic one.
    #[test]
    fn a_scope_requirement_binds_the_static_path() {
        let reg = registry();
        let exposure = Exposure::nothing().allow_interface("IDL:bank/Account:1.0");

        let mut without =
            guarded(&reg, exposure.clone(), Some(Caller::new("bob")), Approval::default());
        assert!(without.invoke("deposit", |_| {}).is_err());
        assert!(without.conn.reached.is_empty());
        assert!(without.audit()[0].contains("accounts:write"), "{}", without.audit()[0]);

        let alice = Caller::new("alice").with_scope("accounts:write");
        let mut with = guarded(&reg, exposure, Some(alice), Approval::default());
        let _ = with.invoke("deposit", |_| {});
        assert_eq!(with.conn.reached, vec!["deposit"]);
    }

    #[test]
    fn destructive_needs_the_same_approval_as_the_dynamic_path() {
        let reg = registry();
        let exposure = Exposure::nothing().allow_interface("IDL:bank/Account:1.0");
        let mut g =
            guarded(&reg, exposure.clone(), Some(Caller::new("alice")), Approval::default());
        assert!(g.invoke("close", |_| {}).is_err());
        assert!(g.conn.reached.is_empty());

        let mut approved = guarded(
            &reg,
            exposure,
            Some(Caller::new("alice")),
            Approval { destructive_approved: true },
        );
        let _ = approved.invoke("close", |_| {});
        assert_eq!(approved.conn.reached, vec!["close"]);
    }

    /// A oneway that skipped the gate would make fire-and-forget the way
    /// around the guard.
    #[test]
    fn oneways_are_gated_like_everything_else() {
        let reg = registry();
        let mut g = guarded(&reg, Exposure::nothing(), None, Approval::default());
        assert!(g.invoke_oneway("balance", |_| {}).is_err());
        assert!(g.conn.reached.is_empty());
    }

    /// F4's extensibility claim, at the level a deployment would make it: a
    /// stage the guard knows nothing about goes into §4.5's empty quota seat
    /// on a live `Guarded`, and the property I1 exists for still holds for it
    /// — a refused call does not reach the transport, and it is refused as
    /// `NO_PERMISSION` like any other, through the built-in stages that were
    /// not touched.
    #[test]
    fn a_deployment_can_add_a_stage_the_guard_knows_nothing_about() {
        struct RateLimiter {
            seen: usize,
        }
        impl Interceptor for RateLimiter {
            fn before(&mut self, _ctx: &CallContext<'_>) -> Outcome {
                self.seen += 1;
                if self.seen > 2 {
                    return Outcome::Refuse(Denied::Intercepted {
                        stage: "quota.rate_limit".to_owned(),
                        reason: "2 calls per window".to_owned(),
                    });
                }
                Outcome::Proceed
            }
        }

        let reg = registry();
        let exposure = Exposure::nothing().allow_interface("IDL:bank/Account:1.0");
        let mut g = guarded(&reg, exposure, Some(Caller::new("alice")), Approval::default());
        assert!(g.chain_mut().insert_after(
            crate::interceptor::STAGE_SCOPES,
            "quota.rate_limit",
            RateLimiter { seen: 0 }
        ));

        for _ in 0..2 {
            let _ = g.invoke("balance", |_| {});
        }
        let err = g.invoke("balance", |_| {}).unwrap_err();
        assert!(
            matches!(&err, GiopError::SystemException { id, .. } if id == NO_PERMISSION),
            "{err}"
        );
        assert_eq!(g.conn.reached, vec!["balance", "balance"], "the third never reached the wire");
        assert_eq!(g.audit().len(), 3);
        assert!(g.audit()[2].starts_with("REFUSE caller=alice"), "{}", g.audit()[2]);
        assert!(g.audit()[2].contains("quota.rate_limit"), "{}", g.audit()[2]);
    }

    /// The two refusals a stub can get, told apart by the only thing a stub's
    /// caller can read: the system exception.
    ///
    /// A retry loop cannot see the audit log and must not have to parse a
    /// reason string. `NO_PERMISSION` means stop; `TRANSIENT` means the budget
    /// may renew — and both say `COMPLETED_NO`, because this gate refuses
    /// before anything reaches the wire and a retry is therefore safe.
    #[test]
    fn a_spent_budget_refuses_a_stub_with_transient_and_a_policy_refuses_with_no_permission() {
        use crate::quota::{Quota, Renewal, Scope};

        let reg = registry();
        let exposure = Exposure::nothing().allow_operation("IDL:bank/Account:1.0", "balance");
        let mut g = guarded(&reg, exposure, Some(Caller::new("alice")), Approval::default());
        let quota = Quota::new(1, Scope::Caller, Renewal::Window);
        assert!(g.chain_mut().quota(quota.clone()));

        let _ = g.invoke("balance", |_| {});
        let spent = g.invoke("balance", |_| {}).unwrap_err();
        assert!(
            matches!(&spent, GiopError::SystemException { id, completed, .. }
                if id == TRANSIENT && *completed == 1),
            "a budget that renews invites a retry: {spent}"
        );

        // The same guard, a refusal the policy made: unchanged.
        let denied = g.invoke("close", |_| {}).unwrap_err();
        assert!(
            matches!(&denied, GiopError::SystemException { id, completed, .. }
                if id == NO_PERMISSION && *completed == 1),
            "{denied}"
        );
        assert_eq!(g.conn.reached, vec!["balance"], "neither refusal reached the wire");

        // A new window, and the stub is served again — without the guard being
        // rebuilt or the exposure being touched.
        assert!(quota.open_window(crate::quota::Window::new("the next hour")));
        let _ = g.invoke("balance", |_| {});
        assert_eq!(g.conn.reached, vec!["balance", "balance"]);
    }

    #[test]
    fn every_decision_lands_in_the_audit_in_order() {
        let reg = registry();
        let exposure = Exposure::nothing().allow_operation("IDL:bank/Account:1.0", "balance");
        let mut g = guarded(&reg, exposure, Some(Caller::new("alice")), Approval::default());
        let _ = g.invoke("balance", |_| {});
        let _ = g.invoke("close", |_| {});
        assert_eq!(g.audit().len(), 2);
        assert!(g.audit()[0].starts_with("ALLOW"));
        assert!(g.audit()[1].starts_with("REFUSE"));
        assert!(g.audit()[1].contains("operation=close"));
    }
}
