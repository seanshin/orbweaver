//! The gate as an ordered chain: `docs/PLAN-MOE.md` §3 F4, architecture §4.5.
//!
//! [`crate::guard::Guarded::check`] *was* the chain, written out longhand:
//! exposure, then `ai_authz` scopes, then the destructive-effect approval, then
//! an audit line. Four concerns in one function, in an order nothing named and
//! nothing could extend. §4.5 asks for them as portable interceptors for the
//! reason it gives in one line — *게이팅 정책을 인터셉터로 구현하면 라우팅
//! 로직과 정책을 분리 가능*: routing decides where a call goes, policy decides
//! whether it goes, and a function that does both can be neither reordered nor
//! extended by a deployment that needs one more rule.
//!
//! Nothing here re-decides anything. Each gate calls the same primitive
//! [`crate::policy`] already exports — [`Exposure::exposes`],
//! [`Exposure::exposes_operation`], [`required_scopes`], [`destructive_effect`]
//! — so this module is a *composition* of the rules, not a second copy of them.
//! [`Exposure::check_call`] is the other composition of the same primitives,
//! and `the_chain_and_check_call_answer_alike` pins the two together case by
//! case, because two compositions of one rule set is exactly the shape that
//! drifts.
//!
//! # The stack, and which seats have occupants
//!
//! §4.5's recommended stack, with what actually sits in each seat:
//!
//! | §4.5 | concern | stage | occupant |
//! |------|---------|-------|----------|
//! | 1 | 인증·인가 | [`STAGE_EXPOSURE`], [`STAGE_SCOPES`] | the default-deny allowlist and `ai_authz` |
//! | 2 | 쿼터·레이트 리밋 | [`SEAT_QUOTA`] | **none** |
//! | 3 | 안전 필터 | [`STAGE_APPROVAL`], [`SEAT_SAFETY_CONTENT`] | the destructive-effect approval only |
//! | 4 | 텔레메트리 | [`STAGE_TELEMETRY`] | call counts into [`CallStats`] |
//! | 5 | 감사 로그 | [`STAGE_AUDIT`] | the one audit formatter |
//!
//! The empty seats are named rather than omitted, because **a named empty seat
//! is a plan and an unnamed absence is a gap**:
//!
//! - **[`SEAT_QUOTA`] (§4.5 #2) has no occupant.** There is no rate limiter,
//!   no token budget and no per-tenant quota in this crate. A deployment that
//!   needs one inserts it with
//!   `chain.insert_after(STAGE_SCOPES, "quota.rate_limit", …)` — after
//!   authorization, before safety, which is where §4.5 puts it — and touches
//!   no built-in stage to do it. `a_custom_stage_fills_the_empty_quota_seat`
//!   is that insertion, run.
//! - **[`SEAT_SAFETY_CONTENT`] (§4.5 #3) has no occupant.** [`STAGE_APPROVAL`]
//!   fills the half of the safety seat that reads the *contract*
//!   (`ai_effect: destructive` needs a human). The half that reads the
//!   *arguments* — prompt-injection screening, PII in an `in` parameter, a
//!   payload that is fine to send to one target and not another — is empty.
//!   It is empty for a stated reason: a content filter that inspects arguments
//!   needs the decoded arguments, and this chain deliberately runs before them
//!   (see below).
//! - **Telemetry is half-occupied.** §4.5 asks for 지연·토큰·비용 — latency,
//!   tokens, cost. [`TelemetryInterceptor`] records counts, and nothing else,
//!   for the reason [`crate::promote`] gives at length: there is no clock in
//!   scope, and a count-based history is the one that recommends the same
//!   promotion twice.
//!
//! # Registration order is not acting order
//!
//! A stage that gates acts in [`Interceptor::before`]. A stage that observes
//! acts in [`Interceptor::after`]. The unwinding discipline calls `after` only
//! on the stages that ran, so **an observer that must see every call — including
//! the ones a gate refuses — has to be registered outside every gate.** That is
//! not a reordering of §4.5; it is what §4.5's order costs in an onion. Read
//! the acting order and the numbering comes back:
//!
//! ```text
//! registration:  audit  telemetry  exposure  scopes  [quota]  approval
//! before  (in):    ·        ·         1 ───────1 ─────[2]──────3
//! after  (out):    5 ◀──────4 ◀───────·────────·───────·───────·
//! ```
//!
//! The gates act on the way in, in §4.5's order 1 → 2 → 3. The observers act on
//! the way out, in §4.5's order 4 → 5, with the audit line as the last word
//! about the call — which is what "감사 로그, 마지막" means operationally.
//!
//! # The unwinding discipline
//!
//! [`Chain::run`] calls `before` in registration order and stops at the first
//! [`Outcome::Refuse`]. It then calls `after` in **reverse** order over the
//! stages that ran, the refuser included, and never on a stage whose `before`
//! did not run. This is CORBA's own rule for portable interceptors — an ending
//! interception point runs for the interception points that completed, and for
//! no others — and it is the whole reason the observers sit outermost.
//!
//! # Why the context carries no connection
//!
//! [`CallContext`] carries the contract and the request: the registry, the
//! caller, the target repository id, the operation and the approval. That is
//! everything the ported checks read, and it is deliberately not the
//! connection.
//!
//! A stage handed a `Connection` could dial, send and block, on the hot path of
//! every call, and three things follow from that. A gate that does I/O is no
//! longer a deterministic function of the contract and the request, so it can
//! time out and its answer stops being reproducible — the same property
//! [`crate::promote`] protects for the same reason. It can be made to hang by
//! the very target the caller is being protected *from*. And a stage that can
//! send is a stage that can call: the gate becomes a caller, past its own gate,
//! which is §4.7's bypass rebuilt inside the thing that exists to prevent it.
//!
//! The chain also runs **before** the arguments are decoded, so no stage sees
//! them. That is what leaves [`SEAT_SAFETY_CONTENT`] empty rather than merely
//! unimplemented: filling it is a change to where the chain runs, not just a
//! stage to write.

use orbweaver_registry::Registry;

use crate::guard::audit_entry;
use crate::identity::Caller;
use crate::policy::{Approval, Denied, Exposure, destructive_effect, required_scopes};
use crate::promote::CallStats;

/// §4.5 #1, the allowlist half: is this interface, and this operation on it,
/// exposed at all?
pub const STAGE_EXPOSURE: &str = "authz.exposure";
/// §4.5 #1, the authorization half: does the caller hold what `ai_authz` asks
/// for?
pub const STAGE_SCOPES: &str = "authz.scopes";
/// §4.5 #2, unoccupied. The seat a rate limiter, a token budget or a per-tenant
/// quota goes into, between [`STAGE_SCOPES`] and [`STAGE_APPROVAL`].
pub const SEAT_QUOTA: &str = "quota";
/// §4.5 #3, the contract half of the safety seat: `ai_effect: destructive`
/// needs a human's approval.
pub const STAGE_APPROVAL: &str = "safety.approval";
/// §4.5 #3, unoccupied. The argument-reading half of the safety seat; see the
/// module docs for why it cannot simply be written today.
pub const SEAT_SAFETY_CONTENT: &str = "safety.content";
/// §4.5 #4: counts into [`CallStats`], the first half of §6's feedback loop.
pub const STAGE_TELEMETRY: &str = "telemetry";
/// §4.5 #5: one line per decision, through the one formatter.
pub const STAGE_AUDIT: &str = "audit";

/// Everything a stage may read about the call it is deciding on.
///
/// The contract (`registry`) and the request (`caller`, `target`, `operation`,
/// `approval`) — exactly what the ported checks read, and no connection. The
/// module docs say why that absence is load-bearing rather than an oversight.
#[derive(Debug, Clone, Copy)]
pub struct CallContext<'a> {
    /// The catalog the contract is read from. Read-only, in memory, no I/O:
    /// annotations are looked up here, which is all `ai_authz` and `ai_effect`
    /// ever needed.
    pub registry: &'a Registry,
    /// Who the call is on behalf of, as the *host* authenticated them. `None`
    /// is a session nobody is signed into.
    pub caller: Option<&'a Caller>,
    /// The repository id the capability table resolved. Never what a caller
    /// asserted about itself: a stub that could name its own interface would
    /// be naming its own permissions.
    pub target: &'a str,
    /// The operation being called.
    pub operation: &'a str,
    /// What the *host* has approved, never what the agent claims.
    pub approval: Approval,
}

/// What a stage answers when it is asked to let a call through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// This stage has no objection. The next stage decides.
    Proceed,
    /// This stage refuses. No later stage runs, and the call does not happen.
    Refuse(Denied),
}

/// What became of a call, as the unwinding reports it to the stages that ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallResult<'a> {
    /// Every gate proceeded and the call was made. `ok` is whether it
    /// completed — a mapping error, a refusal from the target and a dropped
    /// connection are all `false`, exactly as [`CallStats::record`] wants them.
    ///
    /// Note that `ok: false` is still an **allowed** call. The audit line says
    /// `ALLOW`, because the policy did allow it; what happened afterwards is
    /// not a policy decision.
    Completed {
        /// Whether the call completed.
        ok: bool,
    },
    /// A stage refused, and the call never reached the target.
    Refused {
        /// The stage that refused, for an observer that wants to attribute it.
        /// The audit line does *not* carry it: the line format is fixed and
        /// shared with the dynamic path (see [`audit_entry`]).
        stage: &'static str,
        /// Why.
        why: &'a Denied,
    },
    /// The call was refused *before* any stage ran, because the capability
    /// table did not resolve the handle. There is no gated target, no counter
    /// to touch, and nothing to promote — only a decision to record. See
    /// [`Chain::unresolved`].
    Unresolved {
        /// Why, already rendered: it comes from a [`crate::ToolError`], not
        /// from a [`Denied`].
        why: &'a str,
    },
}

/// What a stage kept, for the chain's owner to read back.
///
/// This exists because `Any` downcasting does not fit: [`Chain`] holds boxed
/// stages, and reaching a concrete type through a box means `dyn Any`, which
/// means `'static`. One readout with a closed set of shapes costs a stage that
/// records nothing exactly nothing — the default returns [`Record::Nothing`].
pub enum Record<'a> {
    /// This stage keeps nothing a chain owner needs.
    Nothing,
    /// Audit lines, oldest first.
    Lines(&'a [String]),
    /// Promotion counters.
    Counters(&'a CallStats),
}

/// One stage of the chain: the CORBA portable-interceptor shape, adapted to
/// this call path.
///
/// `send_request`/`receive_request` collapse into [`Interceptor::before`] and
/// `send_reply`/`send_exception` into [`Interceptor::after`], because this
/// chain sits on one side of one call — the bridge's — and the four-way split
/// is about which side of the wire the ORB is on.
pub trait Interceptor {
    /// Called on the way in, in registration order. [`Outcome::Refuse`] stops
    /// the call: no later stage runs and nothing reaches the target.
    fn before(&mut self, ctx: &CallContext<'_>) -> Outcome;

    /// Called on the way out, in reverse registration order, on the stages
    /// whose `before` ran — including the one that refused, and never one that
    /// did not run.
    fn after(&mut self, ctx: &CallContext<'_>, result: &CallResult<'_>) {
        let _ = (ctx, result);
    }

    /// What this stage recorded. Default: nothing.
    fn record(&self) -> Record<'_> {
        Record::Nothing
    }
}

/// An ordered, extensible stack of [`Interceptor`]s.
///
/// [`Chain::standard`] builds §4.5's stack. Everything else is insertion:
/// [`Chain::push`] at the innermost end, [`Chain::insert_after`] at a named
/// seat. There is no removal, deliberately — a chain a deployment can subtract
/// the audit stage from is a chain that can be configured into not auditing.
#[derive(Default)]
pub struct Chain {
    stages: Vec<Stage>,
}

struct Stage {
    name: &'static str,
    interceptor: Box<dyn Interceptor>,
}

/// The readout fallback for a chain with no telemetry stage. Reachable only
/// through [`Chain::empty`] plus insertions: [`Chain::standard`] always has one
/// and nothing can remove it.
static NO_COUNTERS: CallStats = CallStats::empty();

impl Chain {
    /// A chain with no stages, which refuses nothing and records nothing.
    pub fn empty() -> Self {
        Self { stages: Vec::new() }
    }

    /// §4.5's 표준 스택 — the recommended order, every stage named.
    ///
    /// Registration order is `audit`, `telemetry`, `exposure`, `scopes`,
    /// `approval`; acting order is §4.5's 1 → 2 → 3 on the way in and 4 → 5 on
    /// the way out. The module docs draw it. The two unoccupied seats,
    /// [`SEAT_QUOTA`] and [`SEAT_SAFETY_CONTENT`], are named there too.
    pub fn standard(exposure: Exposure) -> Self {
        let mut chain = Self::empty();
        // §4.5 #5, outermost so that its `after` is the last word on every
        // call, refused or not.
        chain.push(STAGE_AUDIT, AuditInterceptor::default());
        // §4.5 #4, just inside it: counts every decision the gates make.
        chain.push(STAGE_TELEMETRY, TelemetryInterceptor::default());
        // §4.5 #1, the two halves of authentication and authorization.
        chain.push(STAGE_EXPOSURE, ExposureInterceptor::new(exposure));
        chain.push(STAGE_SCOPES, ScopeInterceptor);
        // §4.5 #2 SEAT_QUOTA sits here, unoccupied.
        // §4.5 #3, the contract half; SEAT_SAFETY_CONTENT's half is unoccupied.
        chain.push(STAGE_APPROVAL, ApprovalInterceptor);
        chain
    }

    /// Appends a stage at the innermost end — the last to gate, the first to
    /// unwind.
    pub fn push(&mut self, name: &'static str, stage: impl Interceptor + 'static) -> &mut Self {
        self.stages.push(Stage { name, interceptor: Box::new(stage) });
        self
    }

    /// Inserts a stage immediately inside `existing`, so it gates after
    /// `existing` and unwinds before it. Returns `false` — inserting nothing —
    /// when no stage is named `existing`.
    ///
    /// This is how §4.5's empty seats get filled without editing a built-in:
    /// `insert_after(STAGE_SCOPES, "quota.rate_limit", …)` puts a limiter in
    /// [`SEAT_QUOTA`], between authorization and safety.
    pub fn insert_after(
        &mut self,
        existing: &str,
        name: &'static str,
        stage: impl Interceptor + 'static,
    ) -> bool {
        let Some(at) = self.stages.iter().position(|s| s.name == existing) else { return false };
        self.stages.insert(at + 1, Stage { name, interceptor: Box::new(stage) });
        true
    }

    /// Every stage's name, in registration order.
    pub fn stages(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.stages.iter().map(|s| s.name)
    }

    /// The gate: `before` in order, stopping at the first refusal, then `after`
    /// in reverse over the stages that ran.
    ///
    /// On `Ok` every stage ran and the caller owes the chain a
    /// [`Chain::completed`] once the call has been made — that is where the
    /// observers act. On `Err` the unwinding has already happened and the
    /// caller owes nothing.
    pub fn run(&mut self, ctx: &CallContext<'_>) -> Result<(), Denied> {
        for i in 0..self.stages.len() {
            let Outcome::Refuse(why) = self.stages[i].interceptor.before(ctx) else { continue };
            let result = CallResult::Refused { stage: self.stages[i].name, why: &why };
            for stage in self.stages[..=i].iter_mut().rev() {
                stage.interceptor.after(ctx, &result);
            }
            return Err(why);
        }
        Ok(())
    }

    /// Unwinds an allowed call: `after` in reverse order over every stage,
    /// since a [`Chain::run`] that returned `Ok` ran all of them.
    pub fn completed(&mut self, ctx: &CallContext<'_>, ok: bool) {
        let result = CallResult::Completed { ok };
        for stage in self.stages.iter_mut().rev() {
            stage.interceptor.after(ctx, &result);
        }
    }

    /// The one decision the chain does not make.
    ///
    /// Resolving a handle is what *produces* the target a [`CallContext`]
    /// needs, so the capability table answers upstream of every stage: a forged
    /// or expired handle is refused before there is anything to gate. The
    /// decision still has to be on record and it has to be the *same* record,
    /// so every stage is notified in reverse order with
    /// [`CallResult::Unresolved`] — the audit stage writes its line, and
    /// telemetry deliberately counts nothing, because there is no resolved
    /// target to count against and an attacker with a made-up handle must not
    /// be able to write into the promotion statistics.
    ///
    /// `ctx.target` here is the handle as presented, which is what the audit
    /// line has always named for this case.
    pub fn unresolved(&mut self, ctx: &CallContext<'_>, why: &str) {
        let result = CallResult::Unresolved { why };
        for stage in self.stages.iter_mut().rev() {
            stage.interceptor.after(ctx, &result);
        }
    }

    /// The audit lines the chain's audit stage kept, oldest first. Empty when
    /// there is no such stage.
    pub fn audit(&self) -> &[String] {
        self.stages
            .iter()
            .find_map(|s| match s.interceptor.record() {
                Record::Lines(lines) => Some(lines),
                _ => None,
            })
            .unwrap_or(&[])
    }

    /// The counters the chain's telemetry stage kept. Empty when there is no
    /// such stage.
    pub fn stats(&self) -> &CallStats {
        self.stages
            .iter()
            .find_map(|s| match s.interceptor.record() {
                Record::Counters(stats) => Some(stats),
                _ => None,
            })
            .unwrap_or(&NO_COUNTERS)
    }
}

/// §4.5 #1, the allowlist half: default-deny exposure, and the per-operation
/// allowlist inside an exposed interface.
///
/// The order of the two questions is the one [`Exposure::check_call`] gives its
/// own paragraph: an operation on an unexposed interface reports the
/// *interface*, never "no such operation", because the second answer confirms
/// what exists behind a gate the caller never got through.
pub struct ExposureInterceptor {
    exposure: Exposure,
}

impl ExposureInterceptor {
    /// The gate for one exposure.
    pub fn new(exposure: Exposure) -> Self {
        Self { exposure }
    }

    /// What this stage will let through.
    pub fn exposure(&self) -> &Exposure {
        &self.exposure
    }
}

impl Interceptor for ExposureInterceptor {
    fn before(&mut self, ctx: &CallContext<'_>) -> Outcome {
        if !self.exposure.exposes(ctx.target) {
            return Outcome::Refuse(Denied::InterfaceNotExposed(ctx.target.to_owned()));
        }
        if !self.exposure.exposes_operation(ctx.target, ctx.operation) {
            return Outcome::Refuse(Denied::OperationNotExposed {
                id: ctx.target.to_owned(),
                operation: ctx.operation.to_owned(),
            });
        }
        Outcome::Proceed
    }
}

/// §4.5 #1, the authorization half: the scopes `ai_authz` asks for, matched
/// against the caller the host authenticated.
///
/// Stateless — the requirement is in the contract and the scopes are on the
/// caller, so there is nothing for this stage to hold.
pub struct ScopeInterceptor;

impl Interceptor for ScopeInterceptor {
    fn before(&mut self, ctx: &CallContext<'_>) -> Outcome {
        for required in required_scopes(ctx.registry, ctx.target, ctx.operation) {
            match ctx.caller {
                None => {
                    return Outcome::Refuse(Denied::NotAuthenticated {
                        id: ctx.target.to_owned(),
                        operation: ctx.operation.to_owned(),
                        required,
                    });
                }
                Some(c) if !c.scopes.contains(&required) => {
                    return Outcome::Refuse(Denied::MissingScope {
                        id: ctx.target.to_owned(),
                        operation: ctx.operation.to_owned(),
                        required,
                    });
                }
                Some(_) => {}
            }
        }
        Outcome::Proceed
    }
}

/// §4.5 #3, the contract half of the safety seat: an operation whose
/// `ai_effect` is not one of the harmless ones needs a human's approval.
///
/// It runs *after* [`ScopeInterceptor`] for the reason [`Exposure::check_call`]
/// states: an unauthorised caller must not be told which operations would
/// merely have needed an approval.
pub struct ApprovalInterceptor;

impl Interceptor for ApprovalInterceptor {
    fn before(&mut self, ctx: &CallContext<'_>) -> Outcome {
        if let Some(effect) = destructive_effect(ctx.registry, ctx.target, ctx.operation)
            && !ctx.approval.destructive_approved
        {
            return Outcome::Refuse(Denied::NeedsApproval {
                id: ctx.target.to_owned(),
                operation: ctx.operation.to_owned(),
                effect,
            });
        }
        Outcome::Proceed
    }
}

/// §4.5 #4: the first half of §6's feedback loop.
///
/// Counts land in a [`CallStats`], which is the same store the promotion policy
/// reads — PLAN-MOE's integration point **IF2**: F4's telemetry and stream B's
/// promotion statistics are one store, not two. A refused call counts as a
/// failure, because a path that is refused is not one to freeze into compiled
/// code; an unresolved handle counts as nothing at all, because it named no
/// path.
#[derive(Debug, Default)]
pub struct TelemetryInterceptor {
    stats: CallStats,
}

impl TelemetryInterceptor {
    /// A stage with an empty history.
    pub fn new() -> Self {
        Self::default()
    }

    /// What it has counted.
    pub fn stats(&self) -> &CallStats {
        &self.stats
    }
}

impl Interceptor for TelemetryInterceptor {
    fn before(&mut self, _ctx: &CallContext<'_>) -> Outcome {
        // Observation only: this stage never refuses. It is registered outside
        // the gates so that its `after` runs whatever they decide.
        Outcome::Proceed
    }

    fn after(&mut self, ctx: &CallContext<'_>, result: &CallResult<'_>) {
        match result {
            CallResult::Completed { ok } => self.stats.record(ctx.target, ctx.operation, *ok),
            CallResult::Refused { .. } => self.stats.record(ctx.target, ctx.operation, false),
            // No target was resolved, so there is nothing to count against —
            // see [`Chain::unresolved`].
            CallResult::Unresolved { .. } => {}
        }
    }

    fn record(&self) -> Record<'_> {
        Record::Counters(&self.stats)
    }
}

/// §4.5 #5: one line per decision, through [`audit_entry`] — **the** formatter.
///
/// Not a second format, on purpose. The static and the dynamic path write
/// string-equal lines for the same (caller, target, operation), which is what
/// lets `crate::promote`'s gate compare captured lines instead of
/// reconstructed ones. This stage writes through the same function it always
/// did; only the place it is called from moved.
///
/// # What that move cost, stated
///
/// The line used to be written the instant the policy answered, before
/// anything touched the wire. It is now written on the way out, because a
/// single audit stage that sees *both* an allowed call and a refusal from a
/// gate ahead of it can only be an outermost observer, and an observer acts in
/// `after`. The line's content and its order are unchanged — every existing
/// test passes untouched — but an `ALLOW` for a call the process dies in the
/// middle of is no longer on record, where before it was. That is one line
/// lost on a crash in exchange for one audit stage instead of an audit call at
/// every point a decision can be made, which is how the format drifted into
/// two in the first place. It is a real cost and not a free refactor.
#[derive(Debug, Default)]
pub struct AuditInterceptor {
    lines: Vec<String>,
}

impl AuditInterceptor {
    /// A stage with an empty log.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every line it has written, oldest first.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }
}

impl Interceptor for AuditInterceptor {
    fn before(&mut self, _ctx: &CallContext<'_>) -> Outcome {
        // Observation only, and outermost: a stage that gated here would be
        // deciding before anything it audits had decided.
        Outcome::Proceed
    }

    fn after(&mut self, ctx: &CallContext<'_>, result: &CallResult<'_>) {
        let line = match result {
            // `ok` is not a policy fact. The gate allowed the call; whether it
            // then failed at argument mapping or on the wire is a different
            // question, and answering it here would make the audit log a
            // transport log.
            CallResult::Completed { .. } => {
                audit_entry("ALLOW", ctx.caller, ctx.target, ctx.operation, None)
            }
            CallResult::Refused { why, .. } => {
                audit_entry("REFUSE", ctx.caller, ctx.target, ctx.operation, Some(&why.to_string()))
            }
            CallResult::Unresolved { why } => {
                audit_entry("REFUSE", ctx.caller, ctx.target, ctx.operation, Some(why))
            }
        };
        self.lines.push(line);
    }

    fn record(&self) -> Record<'_> {
        Record::Lines(&self.lines)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

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
          //@ ai_effect: destructive
          void close();
          void touch();
        };
      };";

    const ACCOUNT: &str = "IDL:bank/Account:1.0";

    fn ctx<'a>(
        reg: &'a Registry,
        caller: Option<&'a Caller>,
        operation: &'a str,
        approval: Approval,
    ) -> CallContext<'a> {
        CallContext { registry: reg, caller, target: ACCOUNT, operation, approval }
    }

    /// PLAN-MOE F4's first oracle: the order is pinned, not incidental.
    #[test]
    fn the_standard_stack_registers_every_named_stage_in_order() {
        let chain = Chain::standard(Exposure::nothing());
        assert_eq!(
            chain.stages().collect::<Vec<_>>(),
            vec![
                STAGE_AUDIT,     // §4.5 #5, outermost so it always unwinds
                STAGE_TELEMETRY, // §4.5 #4
                STAGE_EXPOSURE,  // §4.5 #1
                STAGE_SCOPES,    // §4.5 #1
                // §4.5 #2 SEAT_QUOTA: named, unoccupied
                STAGE_APPROVAL, // §4.5 #3; SEAT_SAFETY_CONTENT unoccupied
            ]
        );
    }

    /// The port's own oracle. Two compositions of one rule set is the shape
    /// that drifts, so every combination that distinguishes the gates is
    /// checked against [`Exposure::check_call`] — same verdict, same `Denied`.
    #[test]
    fn the_chain_and_check_call_answer_alike() {
        let reg = registry(IDL);
        let alice = Caller::new("alice").with_scope("accounts:write");
        let bob = Caller::new("bob");
        let exposures = [
            ("nothing", Exposure::nothing()),
            ("interface", Exposure::nothing().allow_interface(ACCOUNT)),
            ("one operation", Exposure::nothing().allow_operation(ACCOUNT, "balance")),
            ("another interface", Exposure::nothing().allow_interface("IDL:other/Thing:1.0")),
        ];
        let callers = [("nobody", None), ("alice", Some(&alice)), ("bob", Some(&bob))];

        for (ename, exposure) in &exposures {
            for (cname, caller) in &callers {
                for operation in ["balance", "deposit", "close", "touch", "no_such_op"] {
                    for approved in [false, true] {
                        let approval = Approval { destructive_approved: approved };
                        let expected =
                            exposure.check_call(&reg, ACCOUNT, operation, approval, *caller);
                        let mut chain = Chain::standard(exposure.clone());
                        let got = chain.run(&ctx(&reg, *caller, operation, approval));
                        assert_eq!(
                            got.err(),
                            expected.err(),
                            "{ename}/{cname}/{operation}/approved={approved}"
                        );
                    }
                }
            }
        }
    }

    /// The refused call never reaches the observers' *gate*, but always reaches
    /// their unwinding: one REFUSE line, one failure counted.
    #[test]
    fn a_refusal_still_reaches_the_audit_and_the_counters() {
        let reg = registry(IDL);
        let mut chain = Chain::standard(Exposure::nothing());
        let err = chain.run(&ctx(&reg, None, "balance", Approval::default())).unwrap_err();
        assert!(matches!(err, Denied::InterfaceNotExposed(_)), "{err}");
        assert_eq!(chain.audit().len(), 1);
        assert!(chain.audit()[0].starts_with("REFUSE caller=<nobody>"), "{}", chain.audit()[0]);
        assert_eq!(chain.stats().calls(ACCOUNT, "balance"), 1);
        assert_eq!(chain.stats().failures(ACCOUNT, "balance"), 1);
    }

    /// An allowed call that then failed is still an ALLOW: the gate's answer
    /// and the call's fate are different facts.
    #[test]
    fn an_allowed_call_that_failed_is_audited_allow_and_counted_failed() {
        let reg = registry(IDL);
        let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
        let call = ctx(&reg, None, "balance", Approval::default());
        chain.run(&call).expect("exposed");
        chain.completed(&call, false);
        assert_eq!(
            chain.audit(),
            ["ALLOW caller=<nobody> target=IDL:bank/Account:1.0 operation=balance"]
        );
        assert_eq!(chain.stats().failures(ACCOUNT, "balance"), 1);
    }

    /// An unresolved handle is recorded and *not* counted: a made-up handle
    /// must not be able to write into the promotion statistics.
    #[test]
    fn an_unresolved_handle_is_audited_but_never_counted() {
        let reg = registry(IDL);
        let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
        let forged = "cap_00000000000000000000000000000000";
        let call = CallContext {
            registry: &reg,
            caller: None,
            target: forged,
            operation: "balance",
            approval: Approval::default(),
        };
        chain.unresolved(&call, "no live reference is held under that handle");
        assert_eq!(chain.audit().len(), 1);
        assert!(chain.audit()[0].contains(&format!("target={forged}")), "{}", chain.audit()[0]);
        assert_eq!(chain.stats().calls(forged, "balance"), 0);
    }

    // --- extensibility: a stage nobody built in ---

    /// A rate limiter, the occupant [`SEAT_QUOTA`] does not have. Refuses the
    /// third call in a window and counts nothing else.
    struct RateLimiter {
        seen: usize,
        allowed_per_window: usize,
    }

    impl Interceptor for RateLimiter {
        fn before(&mut self, _ctx: &CallContext<'_>) -> Outcome {
            self.seen += 1;
            if self.seen > self.allowed_per_window {
                return Outcome::Refuse(Denied::Intercepted {
                    stage: "quota.rate_limit".to_owned(),
                    reason: format!(
                        "{} calls are allowed per window and this is call {}",
                        self.allowed_per_window, self.seen
                    ),
                });
            }
            Outcome::Proceed
        }
    }

    /// F4's extensibility proof: a deployment fills §4.5's empty quota seat
    /// without touching a single built-in stage, and the built-ins keep
    /// working around it — the limiter's refusal is audited and counted by the
    /// stages that were already there.
    #[test]
    fn a_custom_stage_fills_the_empty_quota_seat() {
        let reg = registry(IDL);
        let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
        assert!(chain.insert_after(
            STAGE_SCOPES,
            "quota.rate_limit",
            RateLimiter { seen: 0, allowed_per_window: 2 }
        ));
        assert_eq!(
            chain.stages().collect::<Vec<_>>(),
            vec![
                STAGE_AUDIT,
                STAGE_TELEMETRY,
                STAGE_EXPOSURE,
                STAGE_SCOPES,
                "quota.rate_limit", // §4.5 #2, occupied by the deployment
                STAGE_APPROVAL,
            ],
            "the limiter sits where §4.5 puts a quota: after authz, before safety"
        );

        let call = ctx(&reg, None, "balance", Approval::default());
        for _ in 0..2 {
            chain.run(&call).expect("within the window");
            chain.completed(&call, true);
        }
        let refused = chain.run(&call).unwrap_err();
        assert_eq!(
            refused,
            Denied::Intercepted {
                stage: "quota.rate_limit".to_owned(),
                reason: "2 calls are allowed per window and this is call 3".to_owned(),
            },
            "{refused}"
        );

        // The built-ins did their jobs around a stage they know nothing about.
        assert_eq!(chain.audit().len(), 3);
        assert!(chain.audit()[2].starts_with("REFUSE "), "{}", chain.audit()[2]);
        assert!(chain.audit()[2].contains("quota.rate_limit"), "{}", chain.audit()[2]);
        assert_eq!(chain.stats().calls(ACCOUNT, "balance"), 3);
        assert_eq!(chain.stats().failures(ACCOUNT, "balance"), 1);
    }

    /// Records what ran and in which phase, so the unwinding discipline can be
    /// read off a list rather than argued about.
    struct Tracer {
        name: &'static str,
        log: Rc<RefCell<Vec<String>>>,
        refuses: bool,
    }

    impl Interceptor for Tracer {
        fn before(&mut self, _ctx: &CallContext<'_>) -> Outcome {
            self.log.borrow_mut().push(format!("before {}", self.name));
            if self.refuses {
                return Outcome::Refuse(Denied::Intercepted {
                    stage: self.name.to_owned(),
                    reason: "no".to_owned(),
                });
            }
            Outcome::Proceed
        }

        fn after(&mut self, _ctx: &CallContext<'_>, result: &CallResult<'_>) {
            let phase = match result {
                CallResult::Completed { .. } => "completed",
                CallResult::Refused { stage, .. } => stage,
                CallResult::Unresolved { .. } => "unresolved",
            };
            self.log.borrow_mut().push(format!("after {} ({phase})", self.name));
        }
    }

    fn tracer(name: &'static str, log: &Rc<RefCell<Vec<String>>>, refuses: bool) -> Tracer {
        Tracer { name, log: Rc::clone(log), refuses }
    }

    #[test]
    fn after_unwinds_in_reverse_over_the_stages_that_ran() {
        let reg = registry(IDL);
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut chain = Chain::empty();
        chain.push("outer", tracer("outer", &log, false));
        chain.push("middle", tracer("middle", &log, false));
        chain.push("inner", tracer("inner", &log, false));

        let call = ctx(&reg, None, "balance", Approval::default());
        chain.run(&call).expect("nothing refuses");
        chain.completed(&call, true);
        assert_eq!(
            log.borrow().as_slice(),
            [
                "before outer",
                "before middle",
                "before inner",
                "after inner (completed)",
                "after middle (completed)",
                "after outer (completed)",
            ]
        );
    }

    /// The other half of the discipline: a stage past the refusal never runs,
    /// and so is never unwound — while the refuser itself is.
    #[test]
    fn a_stage_after_the_refusal_is_neither_gated_nor_unwound() {
        let reg = registry(IDL);
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut chain = Chain::empty();
        chain.push("outer", tracer("outer", &log, false));
        chain.push("refuser", tracer("refuser", &log, true));
        chain.push("inner", tracer("inner", &log, false));

        let call = ctx(&reg, None, "balance", Approval::default());
        chain.run(&call).unwrap_err();
        assert_eq!(
            log.borrow().as_slice(),
            [
                "before outer",
                "before refuser",
                "after refuser (refuser)",
                "after outer (refuser)",
                // and nothing at all from "inner"
            ]
        );
    }

    #[test]
    fn inserting_after_a_stage_that_is_not_there_inserts_nothing() {
        let mut chain = Chain::standard(Exposure::nothing());
        let before: Vec<_> = chain.stages().collect();
        assert!(!chain.insert_after("no.such.stage", "x", ScopeInterceptor));
        assert_eq!(chain.stages().collect::<Vec<_>>(), before);
    }

    #[test]
    fn an_empty_chain_records_nothing_and_refuses_nothing() {
        let reg = registry(IDL);
        let mut chain = Chain::empty();
        let call = ctx(&reg, None, "close", Approval::default());
        assert!(chain.run(&call).is_ok(), "a chain with no gates gates nothing");
        chain.completed(&call, true);
        assert!(chain.audit().is_empty());
        assert_eq!(chain.stats().calls(ACCOUNT, "close"), 0);
    }
}
