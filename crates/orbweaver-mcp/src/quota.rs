//! §4.5 #2's occupant: a consumption budget, counted **without a clock**.
//!
//! `docs/CORBAMoEArchitecture.md` §4.5 puts 쿼터·레이트 리밋 second in the
//! standard stack, between authorization and safety.
//! [`crate::interceptor::SEAT_QUOTA`] has named that place since F4 and held
//! nothing. This is what goes in it.
//!
//! # The three questions an operator has to be able to answer
//!
//! A limiter nobody can reason about is a limiter that gets switched off the
//! first time it refuses something. So the configuration is three explicit
//! arguments and no defaults: **how many**, **counted against what**, and
//! **what happens at the limit**.
//!
//! | question | answer |
//! |---|---|
//! | how many | [`Quota::new`]'s `limit`, in calls |
//! | against what | [`Scope`] — the whole bridge, a caller, a caller's interface, a caller's operation |
//! | at the limit | [`Renewal`] — a later window may allow it, or nothing will |
//!
//! Every [`Scope`] but [`Scope::Everything`] nests inside the caller, because a
//! budget an agent escapes by moving to another operation is not a budget on
//! the agent. **The honest limitation that follows: a session nobody is signed
//! into is one principal.** Every unauthenticated session shares
//! `<nobody>`'s budget, which is the safe direction — a deployment that has
//! not wired `--as` gets one shared cap rather than one cap each — but it does
//! mean a quota is only as fine-grained as the host's authentication.
//!
//! # There is no clock here either, and the window is the argument
//!
//! D004 settled it for the trace and the same discipline holds here: **nothing
//! in this crate reads a clock**, so a stage cannot know that a second has
//! passed, and a "per minute" limiter that measures its own minute would be the
//! first clock in the crate — arriving, of all places, inside a gate, where a
//! non-reproducible answer is worst.
//!
//! So the window is *named by the host*, exactly as [`crate::telemetry::Trace`]
//! takes its `ts`: [`Quota::open_window`] takes a [`Window`] label, and the
//! budgets reset when the label **changes**. A host with a clock passes
//! `Window::new("2026-08-14T09:00")` and gets an hourly quota; a host with a
//! request loop passes a batch number and gets a per-batch quota; a host that
//! never calls it at all gets **one window for the life of the process**, which
//! is a total budget and is what a count-without-a-clock limiter degenerates
//! to. All three are the same code and all three replay identically, because
//! the only time in the mechanism is time somebody handed it.
//!
//! This is also why [`Renewal`] has to be stated rather than inferred: with no
//! clock, *the stage cannot know whether a window will ever be opened again*.
//! Only the host knows, so the host declares it, and the refusal is exactly as
//! truthful as that declaration. A [`Renewal::Window`] quota on a host that
//! never opens a second window tells agents to retry forever — that is a
//! misconfiguration this module cannot detect and does not pretend to.
//!
//! # The refusal is a different answer, and says so in both records
//!
//! "You may not" and "not right now" are different answers, and an operator
//! debugging a stuck agent has to be able to tell them apart:
//!
//! | | policy refusal | quota refusal |
//! |---|---|---|
//! | `Denied` | five contract-shaped variants | [`Denied::QuotaExhausted`] |
//! | audit line | `REFUSE … why=<the contract's reason>` | `REFUSE … why=quota exhausted: …` |
//! | trace `outcome` | [`crate::guard::NO_PERMISSION`] | [`crate::guard::TRANSIENT`] when it renews |
//! | trace `stage` | `authz.*` / `safety.approval` | [`crate::interceptor::SEAT_QUOTA`] |
//! | a stub sees | `NO_PERMISSION` | `TRANSIENT`, `COMPLETED_NO` |
//! | a dry run says | `need_scope`, `need_approval`, … | [`crate::dryrun::Would::Exhausted`] |
//!
//! `TRANSIENT` is not invented here: `orbweaver-gen`'s servant runtime already
//! spells the same distinction the same way — *refused now; a retry may well
//! succeed* — so a stub's caller handles a bridge-side budget with the code it
//! already has for a servant-side one. A [`Renewal::Never`] budget renders
//! `NO_PERMISSION` instead, because inviting a retry that cannot succeed is the
//! one thing worse than refusing.
//!
//! # What is charged, and what is not
//!
//! A unit is spent by **an attempt that got past authorization** — not by a
//! completion. A call the approval stage then refuses has still spent its unit,
//! deliberately: otherwise an agent has an unlimited supply of attempts at
//! precisely the operations it is least allowed to make, and the budget stops
//! being a budget for the calls that matter most.
//!
//! A [`crate::interceptor::Chain::dry_run`] spends nothing. The chain's own
//! documentation names this as the price of asking the real chain — *"a stage
//! that mutates state in `before` — a deployment's rate limiter counting the
//! attempt, say — is mutated by a question"* — and for a first-party occupant
//! that price is too high: an operator's [`crate::dryrun::survey`] of a few
//! hundred operations would exhaust the budget by asking about it. So the
//! charge is refunded in [`Interceptor::considered`], which the chain calls in
//! place of `after` on exactly the stages that ran. The gate's *answer* is
//! unchanged — a dry run of an exhausted budget still reads exhausted — only
//! the ledger is left where it was. **The cost the chain documents is one a
//! stage can pay off; it is not one the chain forces on it.**
//!
//! # One budget, two paths
//!
//! [`Quota`] is a handle to a shared ledger and cloning it shares that ledger.
//! That is not a convenience: [`crate::Bridge`] and every
//! [`crate::guard::Guarded`] it hands out build their **own**
//! [`crate::interceptor::Chain`], so a quota installed per chain would be one
//! budget per stub — and a stub with its own budget is §4.7's bypass wearing a
//! limit. Install `quota.clone()` on each chain and there is one ledger behind
//! all of them. (This is the shared-store shape `Guarded::stats` says it has
//! not got for PLAN-MOE **IF2**; here it was cheap enough to do, because a
//! budget that does not add up is not a budget at all.)
//!
//! The ledger is [`Rc<RefCell<…>>`] and therefore single-threaded, like every
//! other chain stage — [`crate::interceptor::Interceptor`] is not `Send` and a
//! session is one thread's.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::interceptor::{CallContext, CallResult, DryRun, Interceptor, Outcome};
use crate::policy::Denied;

/// What a budget is counted against.
///
/// Every variant but [`Scope::Everything`] keys on the caller first, so a
/// finer scope is a subdivision of the caller's budget and never an escape
/// from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// One budget for everything this ledger gates, whoever calls whatever.
    Everything,
    /// One budget per caller principal.
    Caller,
    /// One budget per (caller, interface).
    Interface,
    /// One budget per (caller, interface, operation).
    Operation,
}

impl Scope {
    /// The name this appears under in a report.
    pub fn name(self) -> &'static str {
        match self {
            Scope::Everything => "everything",
            Scope::Caller => "caller",
            Scope::Interface => "interface",
            Scope::Operation => "operation",
        }
    }
}

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// What happens at the limit — and therefore what the caller is told.
///
/// Stated by the operator rather than inferred, because with no clock the stage
/// cannot know whether another window will ever be opened. See the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renewal {
    /// The host opens windows with [`Quota::open_window`]; a refusal is a "not
    /// right now" and reaches a stub as CORBA `TRANSIENT`.
    Window,
    /// A lifetime budget: [`Quota::open_window`] does nothing and a refusal is
    /// final, reaching a stub as `NO_PERMISSION` like any other refusal.
    Never,
}

impl Renewal {
    /// Whether a later window can change this answer.
    pub fn renews(self) -> bool {
        matches!(self, Renewal::Window)
    }

    /// The name this appears under in a report.
    pub fn name(self) -> &'static str {
        match self {
            Renewal::Window => "window",
            Renewal::Never => "never",
        }
    }
}

impl std::fmt::Display for Renewal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// The window a budget is being spent in — **a label the host supplies**.
///
/// A newtype rather than a `String` for the reason [`crate::telemetry::Timestamp`]
/// is one: the argument cannot be filled in by accident from something that is
/// not a window, and [`Window::unopened`] is a stated choice rather than an
/// empty string that might be a bug. Two labels are the same window when they
/// are equal, and that is the whole of the comparison — nothing here parses a
/// label, orders two of them, or knows what an hour is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Window(String);

impl Window {
    /// The window the host says it is — an hour, a batch number, a shift.
    pub fn new(label: impl Into<String>) -> Self {
        Self(label.into())
    }

    /// No window was ever opened: the process's whole life is one window.
    ///
    /// The default, and what a host with no clock and no batch counter gets.
    /// Renders as `-`, the same marker D004's records use for a field there is
    /// nothing to put in.
    pub fn unopened() -> Self {
        Self(crate::telemetry::ABSENT.to_owned())
    }

    /// The label as it appears in a refusal.
    pub fn as_str(&self) -> &str {
        if self.0.is_empty() { crate::telemetry::ABSENT } else { &self.0 }
    }
}

impl std::fmt::Display for Window {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One budget's key: (caller, target, operation), with the parts this scope
/// does not key on left empty.
///
/// A tuple rather than a formatted string so that two dimensions cannot be
/// made to collide by an operation name chosen to contain the separator.
type Key = (String, String, String);

#[derive(Debug)]
struct Ledger {
    window: Window,
    used: BTreeMap<Key, u64>,
    /// The key charged by the `before` that is still in flight, for the refund
    /// a dry run gets. At most one call is in flight at a time: the chain is
    /// single-threaded and `before` is always followed by `after` or
    /// `considered` on the same stage.
    in_flight: Option<Key>,
    refusals: u64,
    windows: u64,
}

/// §4.5 #2: a consumption budget, and the stage that enforces it.
///
/// Built with [`Quota::new`], installed at [`crate::interceptor::SEAT_QUOTA`]
/// with [`crate::interceptor::Chain::quota`], and read back through the same
/// handle — every method here takes `&self`, so a host keeps its own clone and
/// never has to reach back through the chain to open a window or read a
/// counter.
///
/// Cloning shares the ledger. See the module docs for why that is the point
/// rather than a convenience.
#[derive(Debug, Clone)]
pub struct Quota {
    limit: u64,
    scope: Scope,
    renewal: Renewal,
    ledger: Rc<RefCell<Ledger>>,
}

impl Quota {
    /// A budget of `limit` calls per window, counted against `scope`, which at
    /// the limit answers according to `renewal`.
    ///
    /// Three arguments and no defaults, deliberately: each one changes what an
    /// agent is told, and a default would be this module choosing a policy on
    /// an operator's behalf. A `limit` of zero is legal and refuses everything
    /// — a way to close a path without editing the exposure.
    pub fn new(limit: u64, scope: Scope, renewal: Renewal) -> Self {
        Self {
            limit,
            scope,
            renewal,
            ledger: Rc::new(RefCell::new(Ledger {
                window: Window::unopened(),
                used: BTreeMap::new(),
                in_flight: None,
                refusals: 0,
                windows: 0,
            })),
        }
    }

    /// Calls per window.
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// What the budget is counted against.
    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// What happens at the limit.
    pub fn renewal(&self) -> Renewal {
        self.renewal
    }

    /// The window currently being spent in.
    pub fn window(&self) -> Window {
        self.ledger.borrow().window.clone()
    }

    /// Opens `window`, resetting every budget — **if the label changed and this
    /// quota renews at all**.
    ///
    /// Returns whether anything was reset, so a host cannot quietly believe it
    /// renewed a [`Renewal::Never`] budget: absence is reported, never greened.
    /// Re-opening the same label is idempotent and returns `false`, which is
    /// what lets a host call this on every request with whatever label its
    /// clock currently renders without needing to remember the last one.
    pub fn open_window(&self, window: Window) -> bool {
        if !self.renewal.renews() {
            return false;
        }
        let mut ledger = self.ledger.borrow_mut();
        if ledger.window == window {
            return false;
        }
        ledger.window = window;
        ledger.used.clear();
        ledger.in_flight = None;
        ledger.windows += 1;
        true
    }

    /// What has been spent, in this window, against the budget this call would
    /// be charged to.
    ///
    /// The arguments are the same three a [`CallContext`] carries, so an
    /// operator asks the question in the terms the refusal answers it in.
    /// Parts the [`Scope`] does not key on are ignored.
    pub fn used_for(&self, caller: Option<&str>, target: &str, operation: &str) -> u64 {
        let key = self.key(caller, target, operation);
        self.ledger.borrow().used.get(&key).copied().unwrap_or(0)
    }

    /// How many distinct budgets have been touched in this window.
    ///
    /// One for [`Scope::Everything`]; for the others it is how many callers,
    /// interfaces or operations have been seen — the size of the thing an
    /// operator is about to be surprised by.
    pub fn budgets(&self) -> usize {
        self.ledger.borrow().used.len()
    }

    /// How many calls this quota has refused, over every window.
    ///
    /// Calls only. A [`crate::interceptor::Chain::dry_run`] that *would* be
    /// refused does not count here, for the same reason it does not count in
    /// [`crate::promote::CallStats`]: a question is not a call, and an operator
    /// surveying an exhausted budget must not manufacture the very number they
    /// are investigating.
    pub fn refusals(&self) -> u64 {
        self.ledger.borrow().refusals
    }

    /// How many windows have been opened since this quota was built.
    pub fn windows_opened(&self) -> u64 {
        self.ledger.borrow().windows
    }

    /// The budget key for a call, per this quota's [`Scope`].
    fn key(&self, caller: Option<&str>, target: &str, operation: &str) -> Key {
        let who = || caller.unwrap_or(crate::guard::NOBODY).to_owned();
        match self.scope {
            Scope::Everything => (String::new(), String::new(), String::new()),
            Scope::Caller => (who(), String::new(), String::new()),
            Scope::Interface => (who(), target.to_owned(), String::new()),
            Scope::Operation => (who(), target.to_owned(), operation.to_owned()),
        }
    }

    /// The key as a refusal names it, in the audit line's own field spelling so
    /// that one grep pattern reads both records.
    fn render(&self, key: &Key) -> String {
        match self.scope {
            Scope::Everything => "everything this bridge gates".to_owned(),
            Scope::Caller => format!("caller={}", key.0),
            Scope::Interface => format!("caller={} target={}", key.0, key.1),
            Scope::Operation => {
                format!("caller={} target={} operation={}", key.0, key.1, key.2)
            }
        }
    }
}

impl Interceptor for Quota {
    fn before(&mut self, ctx: &CallContext<'_>) -> Outcome {
        let key = self.key(ctx.caller.map(|c| c.principal.as_str()), ctx.target, ctx.operation);
        let mut ledger = self.ledger.borrow_mut();
        let used = ledger.used.get(&key).copied().unwrap_or(0);
        if used >= self.limit {
            return Outcome::Refuse(Denied::QuotaExhausted {
                budget: self.render(&key),
                used,
                limit: self.limit,
                window: ledger.window.as_str().to_owned(),
                renews: self.renewal.renews(),
            });
        }
        ledger.used.insert(key.clone(), used + 1);
        ledger.in_flight = Some(key);
        Outcome::Proceed
    }

    /// A call was decided: the charge stands, and a refusal this stage made is
    /// counted once it is known to be about a call rather than a question.
    fn after(&mut self, _ctx: &CallContext<'_>, result: &CallResult<'_>) {
        let mut ledger = self.ledger.borrow_mut();
        ledger.in_flight = None;
        if let CallResult::Refused { why, .. } = result
            && matches!(why, Denied::QuotaExhausted { .. })
        {
            ledger.refusals += 1;
        }
    }

    /// A question was asked: the charge is refunded. See the module docs — the
    /// answer is unchanged, only the ledger is.
    fn considered(&mut self, _ctx: &CallContext<'_>, _dry: &DryRun) {
        let mut ledger = self.ledger.borrow_mut();
        let Some(key) = ledger.in_flight.take() else { return };
        if let Some(used) = ledger.used.get_mut(&key) {
            *used = used.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use orbweaver_registry::Registry;

    use super::*;
    use crate::dryrun::{self, Would};
    use crate::guard::{NO_PERMISSION, TRANSIENT};
    use crate::identity::Caller;
    use crate::interceptor::{Chain, SEAT_QUOTA, STAGE_APPROVAL, STAGE_SCOPES};
    use crate::policy::{Approval, Exposure};
    use crate::telemetry::{CallPath, SpanRecord, TelemetrySink, Timestamp, Trace};

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
          //@ ai_effect: idempotent
          void touch();
          //@ ai_effect: destructive
          void close();
        };
        // The second interface is declared here rather than only named by
        // `OTHER`. It used to be absent from the catalog and the budget cases
        // still passed, because a target nothing knew about reached the quota
        // stage regardless — the effect gate now stops one, which is how the
        // gap in the fixture surfaced.
        interface Ledger {
          //@ ai_effect: read_only
          long balance();
        };
      };";

    const ACCOUNT: &str = "IDL:bank/Account:1.0";
    const OTHER: &str = "IDL:bank/Ledger:1.0";

    fn ctx<'a>(
        reg: &'a Registry,
        caller: Option<&'a Caller>,
        target: &'a str,
        operation: &'a str,
    ) -> CallContext<'a> {
        CallContext {
            registry: reg,
            caller,
            target,
            operation,
            approval: Approval { destructive_approved: true },
            arguments: None,
        }
    }

    fn open(exposure: &Exposure, quota: &Quota) -> Chain {
        let mut chain = Chain::standard(exposure.clone());
        assert!(chain.quota(quota.clone()), "the standard stack has a scopes stage to sit after");
        chain
    }

    /// The seat is where §4.5 puts it: after authorization, before safety.
    #[test]
    fn the_quota_sits_between_authorization_and_safety() {
        let quota = Quota::new(1, Scope::Caller, Renewal::Window);
        let chain = open(&Exposure::nothing(), &quota);
        let stages: Vec<_> = chain.stages().collect();
        let at = |name| stages.iter().position(|s| *s == name).expect(name);
        assert_eq!(at(SEAT_QUOTA), at(STAGE_SCOPES) + 1);
        assert!(at(SEAT_QUOTA) < at(STAGE_APPROVAL));
    }

    /// A chain with no authorization stage has no seat to put a quota in, and
    /// says so rather than silently installing a limiter somewhere else.
    #[test]
    fn a_chain_without_the_seat_refuses_the_quota() {
        let mut chain = Chain::empty();
        assert!(!chain.quota(Quota::new(1, Scope::Caller, Renewal::Window)));
        assert_eq!(chain.stages().count(), 0);
    }

    /// The limit, counted and enforced — and the refusal carries the numbers an
    /// operator needs rather than the word "refused".
    #[test]
    fn the_limit_is_enforced_and_the_refusal_states_the_arithmetic() {
        let reg = registry(IDL);
        let quota = Quota::new(2, Scope::Caller, Renewal::Window);
        let mut chain = open(&Exposure::nothing().allow_interface(ACCOUNT), &quota);
        let alice = Caller::new("alice");
        let call = ctx(&reg, Some(&alice), ACCOUNT, "balance");

        for _ in 0..2 {
            chain.run(&call).expect("within the budget");
            chain.completed(&call, true);
        }
        let refused = chain.run(&call).unwrap_err();
        assert_eq!(
            refused,
            Denied::QuotaExhausted {
                budget: "caller=alice".to_owned(),
                used: 2,
                limit: 2,
                window: "-".to_owned(),
                renews: true,
            },
            "{refused}"
        );
        assert_eq!(quota.used_for(Some("alice"), ACCOUNT, "balance"), 2, "a refusal charges none");
        assert_eq!(quota.refusals(), 1);
    }

    /// The scope is what an operator configures, so every variant is pinned:
    /// what shares a budget with what.
    #[test]
    fn each_scope_keys_the_budget_where_it_says_it_does() {
        let reg = registry(IDL);
        let alice = Caller::new("alice");
        let bob = Caller::new("bob");
        // (scope, how many of the four calls below get through with limit 1)
        let cases = [
            (Scope::Everything, 1),
            (Scope::Caller, 2),    // alice and bob
            (Scope::Interface, 3), // alice×Account, alice×Ledger, bob×Account
            (Scope::Operation, 4), // and alice's two operations are two budgets
        ];
        for (scope, expected) in cases {
            let quota = Quota::new(1, scope, Renewal::Window);
            let exposure = Exposure::nothing().allow_interface(ACCOUNT).allow_interface(OTHER);
            let mut chain = open(&exposure, &quota);
            let calls = [
                (Some(&alice), ACCOUNT, "balance"),
                (Some(&alice), ACCOUNT, "touch"),
                (Some(&alice), OTHER, "balance"),
                (Some(&bob), ACCOUNT, "balance"),
            ];
            let allowed = calls
                .into_iter()
                .filter(|(caller, target, operation)| {
                    let call = ctx(&reg, *caller, target, operation);
                    let ok = chain.run(&call).is_ok();
                    if ok {
                        chain.completed(&call, true);
                    }
                    ok
                })
                .count();
            assert_eq!(allowed, expected, "{scope}");
        }
    }

    /// A session nobody is signed into is one principal, stated as a test
    /// because it is the limitation an operator has to know about.
    #[test]
    fn every_unauthenticated_session_shares_one_budget() {
        let reg = registry(IDL);
        let quota = Quota::new(1, Scope::Caller, Renewal::Window);
        let mut chain = open(&Exposure::nothing().allow_interface(ACCOUNT), &quota);
        let call = ctx(&reg, None, ACCOUNT, "balance");
        chain.run(&call).expect("the first");
        chain.completed(&call, true);
        assert!(chain.run(&call).is_err(), "the second is <nobody>'s too");
        assert_eq!(quota.used_for(None, ACCOUNT, "balance"), 1);
    }

    /// The window is a label and nothing else: opening a new one resets the
    /// budgets, re-opening the same one does not, and no clock is consulted to
    /// decide either.
    #[test]
    fn a_new_window_label_resets_the_budgets_and_the_same_label_does_not() {
        let reg = registry(IDL);
        let quota = Quota::new(1, Scope::Caller, Renewal::Window);
        let mut chain = open(&Exposure::nothing().allow_interface(ACCOUNT), &quota);
        let call = ctx(&reg, None, ACCOUNT, "balance");

        chain.run(&call).expect("the first");
        chain.completed(&call, true);
        assert!(chain.run(&call).is_err());

        assert!(!quota.open_window(Window::unopened()), "the same window is not a new one");
        assert!(chain.run(&call).is_err(), "and nothing was reset");

        assert!(quota.open_window(Window::new("2026-08-14T10:00")));
        chain.run(&call).expect("a new window is a new budget");
        chain.completed(&call, true);
        assert!(!quota.open_window(Window::new("2026-08-14T10:00")), "idempotent");
        assert!(chain.run(&call).is_err());
        assert_eq!(quota.windows_opened(), 1);
        assert_eq!(quota.window(), Window::new("2026-08-14T10:00"));
    }

    /// A lifetime budget does not renew, and saying so is the point: a host
    /// that thinks it renewed one is told it did not.
    #[test]
    fn a_budget_that_never_renews_cannot_be_reopened() {
        let reg = registry(IDL);
        let quota = Quota::new(1, Scope::Everything, Renewal::Never);
        let mut chain = open(&Exposure::nothing().allow_interface(ACCOUNT), &quota);
        let call = ctx(&reg, None, ACCOUNT, "balance");
        chain.run(&call).expect("the first");
        chain.completed(&call, true);

        assert!(!quota.open_window(Window::new("a new hour")), "Never means never");
        let refused = chain.run(&call).unwrap_err();
        assert!(
            matches!(refused, Denied::QuotaExhausted { renews: false, .. }),
            "and the caller is not invited to retry: {refused}"
        );
        assert!(!refused.is_transient());
        assert!(refused.to_string().contains("does not renew"), "{refused}");
    }

    /// The whole reason the seat needed a first-party occupant rather than a
    /// worked example: the two refusals are different answers, and an operator
    /// can tell them apart in **both** records without parsing prose.
    #[test]
    fn a_quota_refusal_is_distinguishable_from_a_policy_refusal_in_the_audit_and_the_trace() {
        #[derive(Default)]
        struct Captured(Rc<RefCell<Vec<(String, String)>>>);
        impl TelemetrySink for Captured {
            fn emit(&mut self, record: &SpanRecord<'_>) {
                self.0.borrow_mut().push((record.stage().to_owned(), record.outcome().to_owned()));
            }
        }

        let reg = registry(IDL);
        let quota = Quota::new(1, Scope::Caller, Renewal::Window);
        let mut chain = open(&Exposure::nothing().allow_operation(ACCOUNT, "balance"), &quota);
        let seen = Rc::new(RefCell::new(Vec::new()));
        assert!(chain.trace(Trace::new(
            "s-1",
            CallPath::Dynamic,
            Timestamp::new("2026-08-14T09:00:00Z"),
            Captured(Rc::clone(&seen)),
        )));

        let allowed = ctx(&reg, None, ACCOUNT, "balance");
        chain.run(&allowed).expect("the first is within the budget");
        chain.completed(&allowed, true);
        // "not right now": the same call, over the limit.
        chain.run(&allowed).unwrap_err();
        // "you may not": a call the exposure never allowed, and the budget is
        // not what stopped it.
        chain.run(&ctx(&reg, None, ACCOUNT, "close")).unwrap_err();

        let audit = chain.audit();
        assert_eq!(audit.len(), 3);
        assert!(audit[1].contains("why=quota exhausted:"), "{}", audit[1]);
        assert!(audit[1].contains("retry in a later window"), "{}", audit[1]);
        assert!(!audit[2].contains("quota"), "{}", audit[2]);

        assert_eq!(
            seen.borrow().as_slice(),
            [
                ("-".to_owned(), "ok".to_owned()),
                (SEAT_QUOTA.to_owned(), TRANSIENT.to_owned()),
                ("authz.exposure".to_owned(), NO_PERMISSION.to_owned()),
            ]
        );
    }

    /// The refund, measured on the instrument that made it necessary: a survey
    /// of a whole interface asks about every operation and spends nothing —
    /// while still answering exhausted when the budget genuinely is.
    #[test]
    fn a_survey_asks_about_every_operation_and_spends_none_of_the_budget() {
        let reg = registry(IDL);
        let quota = Quota::new(2, Scope::Caller, Renewal::Window);
        let exposure = Exposure::nothing().allow_interface(ACCOUNT);
        let mut chain = open(&exposure, &quota);

        for _ in 0..10 {
            dryrun::survey(&mut chain, &reg, &exposure, None, Approval::default(), None);
        }
        assert_eq!(quota.used_for(None, ACCOUNT, "balance"), 0, "a question is not a call");
        assert_eq!(quota.refusals(), 0, "and an unasked refusal is not a refusal");

        // The budget still works afterwards, and the survey then reports it.
        let call = ctx(&reg, None, ACCOUNT, "balance");
        for _ in 0..2 {
            chain.run(&call).expect("within the budget");
            chain.completed(&call, true);
        }
        let p = dryrun::predict(&mut chain, &call);
        assert_eq!(p.would(), Would::Exhausted);
        assert_eq!(p.stage(), Some(SEAT_QUOTA));
        assert_eq!(quota.used_for(None, ACCOUNT, "balance"), 2, "and asking cost nothing");
    }

    /// An attempt that got past authorization is charged even when a later
    /// stage refuses it. Otherwise the operations an agent is least allowed to
    /// make are the ones it may attempt without limit.
    #[test]
    fn a_call_the_approval_stage_refuses_has_still_spent_its_unit() {
        let reg = registry(IDL);
        let quota = Quota::new(5, Scope::Caller, Renewal::Window);
        let mut chain = open(&Exposure::nothing().allow_interface(ACCOUNT), &quota);
        let unapproved = CallContext {
            registry: &reg,
            caller: None,
            target: ACCOUNT,
            operation: "close",
            approval: Approval::default(),
            arguments: None,
        };
        for _ in 0..3 {
            assert!(chain.run(&unapproved).is_err(), "destructive and unapproved");
        }
        assert_eq!(quota.used_for(None, ACCOUNT, "close"), 3);
        assert_eq!(quota.refusals(), 0, "the approval stage refused these, not the quota");
    }

    /// One ledger behind two chains: the bridge's and a stub's guard. A budget
    /// a stub gets a fresh copy of is not a budget.
    #[test]
    fn a_cloned_quota_is_one_budget_across_two_chains() {
        let reg = registry(IDL);
        let quota = Quota::new(2, Scope::Interface, Renewal::Window);
        let exposure = Exposure::nothing().allow_interface(ACCOUNT);
        let mut dynamic = open(&exposure, &quota);
        let mut r#static = open(&exposure, &quota);
        let call = ctx(&reg, None, ACCOUNT, "balance");

        dynamic.run(&call).expect("the bridge's first");
        dynamic.completed(&call, true);
        r#static.run(&call).expect("the stub's first, second overall");
        r#static.completed(&call, true);

        assert!(r#static.run(&call).is_err(), "the stub spends the same budget");
        assert!(dynamic.run(&call).is_err(), "and so does the bridge");
        assert_eq!(quota.used_for(None, ACCOUNT, "balance"), 2);
        assert_eq!(quota.budgets(), 1, "one interface, one budget, two chains");
    }

    /// A budget of zero closes a path without touching the exposure.
    #[test]
    fn a_limit_of_zero_refuses_the_first_call() {
        let reg = registry(IDL);
        let quota = Quota::new(0, Scope::Everything, Renewal::Never);
        let mut chain = open(&Exposure::nothing().allow_interface(ACCOUNT), &quota);
        let refused = chain.run(&ctx(&reg, None, ACCOUNT, "balance")).unwrap_err();
        assert!(matches!(refused, Denied::QuotaExhausted { used: 0, limit: 0, .. }), "{refused}");
    }
}
