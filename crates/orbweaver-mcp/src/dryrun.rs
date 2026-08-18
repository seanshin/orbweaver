//! Dry run: what the gate **would** do, asked without doing it.
//!
//! Before an operator exposes a legacy interface to an agent, they need to know
//! which operations that agent could reach, which would be refused and by which
//! stage, and which would need a human's approval. Until now the only
//! instrument for that question was to make the call — which is the wrong
//! instrument for a decision taken *before* deployment. It dials a production
//! target, it moves whatever the operation moves, and it answers for one
//! operation when the question is about a few hundred.
//!
//! [`predict`] answers one operation and [`survey`] answers the whole exposure.
//! [`crate::Bridge::dry_run`], [`crate::Bridge::dry_run_interface`] and
//! [`crate::Bridge::dry_run_all`] are the same three questions on a session,
//! and [`crate::guard::Guarded::dry_run`] is the static path's.
//!
//! # It is the real chain, or it is worthless
//!
//! Nothing here re-decides anything. [`predict`] hands a synthesized
//! [`CallContext`] to [`Chain::dry_run`], which walks the same gates through
//! the same private `Chain::walk` that [`Chain::run`] walks — one function,
//! two entry points, differing only in what they do with the answer once they
//! have it. **A dry run that answered from a second copy of the policy would
//! eventually disagree with the live gate, and an operator who trusted it would
//! get the surprise in production instead.** So the possibility is removed
//! rather than tested against: there is no second copy to drift.
//! `a_dry_run_and_the_live_gate_answer_alike` walks callers × operations ×
//! exposures and pins the verdict *and the refusing stage* to the live path
//! anyway, in the spirit of `the_chain_and_check_call_answer_alike`, because
//! the property is worth a regression test even when it is structural.
//!
//! Everything an operator reads off the result — "not exposed" rather than
//! "refused", the missing scope's name, the effect that needs approving — is
//! derived from the [`Denied`] the chain produced, never decided here. The
//! derivation is a `match` on the variant and it is the whole of
//! [`Would::of`].
//!
//! # Nothing reaches the wire
//!
//! No [`orbweaver_giop::Invoker`], no `Connection`, no capability handle
//! resolved, no argument marshalled. Two of those are structural rather than
//! careful: [`crate::Bridge::dry_run`] takes no connection parameter to be
//! passed one, and it is keyed by **repository id rather than by handle** —
//! before a deployment there is no handle to hold, and resolving one is the
//! step that produces an address. `a_dry_run_never_touches_the_invoker` makes
//! the remaining half a test: a [`crate::guard::Guarded`] whose invoker panics
//! on contact dry-runs its whole interface and completes.
//!
//! Telemetry is untouched for a reason of its own. [`crate::promote::CallStats`]
//! drives promotion (§7.3 stream B); a hypothetical counted as a call would
//! recommend compiling a stub for a path nobody ever invoked, and an operator's
//! survey of a thousand operations would make every one of them look hot.
//! [`crate::interceptor::TelemetryInterceptor`]'s `considered` is an explicit
//! no-op saying so, and the counters are asserted untouched.
//!
//! # The audit decision, and the case against it
//!
//! **A dry run is audited**, through the one formatter, under its own decision
//! token: `DRYRUN-ALLOW` and `DRYRUN-REFUSE`
//! ([`crate::guard::DECISION_DRY_RUN_ALLOW`]).
//!
//! The case *for* silence is real and worth stating: a dry run makes no call,
//! and a log full of questions is a log an operator has to filter before they
//! can read the answers. A bulk [`survey`] writes one line per operation, so a
//! pre-deployment survey of a large interface is the noisiest thing in the
//! file.
//!
//! It loses to the case against silence. An unaudited dry run is a
//! **reconnaissance instrument**: whoever can call it can map the entire policy
//! — which operations exist, which scope names are missing from a caller, which
//! operations are destructive — one question at a time, and leave nothing
//! behind. That is precisely the enumeration §4.8's log exists to record, and
//! the argument that "it made no call" is exactly the argument an attacker
//! would make. §9.0 asks the audit to record *agent actions*; asking the policy
//! what it would permit is an action.
//!
//! The other tempting answer — audit it as an ordinary `ALLOW` — is worse than
//! either. It would corrupt the record: a reader could no longer tell a call
//! that happened from one that was contemplated, and
//! [`crate::promote::verify_promotion`] would compare a prediction against a
//! measurement. So the line is the same line with a different first field, and
//! `verify_promotion` refuses a hypothetical one by name
//! ([`crate::promote::PromotionRegression::HypotheticalAudit`]). One format,
//! one formatter, two decisions that cannot be confused.
//!
//! # What this is not — §9.0's dry-run honesty clause
//!
//! PLAN §9.0: *a true server-side dry run needs target cooperation that legacy
//! will not provide; the guard's dry run is a client-side gate — it validates,
//! marshals and shows what would be sent, without sending it. Documentation
//! must not oversell this.*
//!
//! This does **less** than that clause allows, and the shortfall is structural
//! rather than unfinished. It answers the *policy* question only: it does not
//! marshal arguments and cannot show what bytes would go out, because the chain
//! runs before the arguments are decoded and a dry run is asked *before a call
//! exists*, so there is no payload to marshal. A dry run that reported the
//! encoded request would have to run after argument mapping, which is a change
//! to *where* the chain runs. Until then: this says who could call what and why
//! not, and says nothing whatever about the payload.
//!
//! The same fact bounds what a prediction is worth once a deployment fills
//! [`crate::interceptor::SEAT_SAFETY_CONTENT`]. Every context synthesized here
//! carries `arguments: None`, so a content stage is *reached* and has nothing
//! to judge: a report can say the stage ran and had no objection, and that is
//! not a promise about a payload nobody has sent yet. An operator reading
//! `Would::Allow` for an operation behind a content filter is reading a policy
//! verdict, not a safety one. Saying so is cheaper than a dry run that guesses.
//!
//! 예측은 **아직 보내지 않은 페이로드**에 대해 아무것도 약속하지 않는다.
//! 내용 필터가 있는 운영에서 `Would::Allow`는 정책의 답이지 안전의 답이 아니다.

use orbweaver_registry::Registry;

use orbweaver_dynamic::json::Json;

use crate::identity::Caller;
use crate::interceptor::{CallContext, Chain, DryRun, StageOutcome};
use crate::policy::{Approval, Denied, Effect, Exposure, Unannotated, stated_effect};
use crate::{obj, s};

/// The annotation a contract may use to name who can approve a destructive
/// operation.
///
/// SIDL v1 defines no such key — `ai_effect` says a human is needed and stops
/// there. This is read opportunistically so that a deployment which *has*
/// written one gets it back in the report, and reports nothing rather than
/// guessing when it is absent. Like every annotation it is **data**: it is
/// rendered into the document and nothing here acts on it (§9.0 R11).
pub const AI_APPROVER: &str = "ai_approver";

/// The caller field's rendering for a session nobody is signed into — the same
/// spelling the audit line uses, so the two can be grepped with one pattern.
const NOBODY: &str = "<nobody>";

/// What would happen, in the terms an operator makes a decision in.
///
/// Derived from the chain's [`Denied`] by [`Would::of`] and by nothing else.
/// The variants exist because the differences between them are the differences
/// an operator acts on: an operation that is *hidden* is a line to add to the
/// exposure, one that is missing a *scope* is a line to add to a role, and one
/// that needs an *approval* is a person to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Would {
    /// Every gate proceeds. The call would be attempted.
    Allow,
    /// The exposure hides the interface or the operation. Not a refusal of
    /// something the caller could otherwise have: it is not on the menu, which
    /// is a different answer and a different fix.
    NotExposed,
    /// Nobody is signed in to hold a scope the contract requires — or the
    /// caller who was signed in has a credential that has since expired
    /// ([`crate::token::Expiry`]). One row, because both have the same fix:
    /// authenticate again. The row's `why` says which it is.
    NeedAuthentication,
    /// The contract requires a scope this caller does not hold.
    NeedScope,
    /// The contract marks the operation as needing a human, and no approval is
    /// in hand.
    NeedApproval,
    /// **The contract does not say what the operation does.** No `ai_effect`
    /// reaches it and the exposure declares no assumption for the silences.
    ///
    /// A row of its own because its fix is a third place: not the allowlist
    /// (`not_exposed`), not a role (`need_scope`), not a person
    /// (`need_approval`) — the **contract**, or one operator declaration about
    /// what a silence means for this exposure. Folding it into `need_approval`
    /// would have sent every one of a legacy estate's silences to a human who
    /// has nothing to read before saying yes.
    NeedEffect,
    /// A consumption budget is spent ([`crate::quota`]). **The one row that is
    /// not about permission**: nothing is missing from the caller and nothing
    /// has to be added to a role — the answer is about what has been used, and
    /// the row's `why` says whether a later window changes it.
    Exhausted,
    /// A stage outside the built-in gates refused — a deployment's own safety
    /// filter, router or rule.
    Refuse,
}

impl Would {
    /// Every variant, in the order a summary lists them. Fixed so that two
    /// surveys of the same exposure diff cleanly.
    pub const ALL: [Would; 8] = [
        Would::Allow,
        Would::NotExposed,
        Would::NeedAuthentication,
        Would::NeedScope,
        Would::NeedApproval,
        Would::NeedEffect,
        Would::Exhausted,
        Would::Refuse,
    ];

    /// The classification of a chain's answer. A `match` on the variant, so
    /// that this is a translation of the gate's decision and never a second
    /// one.
    pub fn of(refusal: Option<&Denied>) -> Self {
        match refusal {
            None => Would::Allow,
            Some(Denied::InterfaceNotExposed(_) | Denied::OperationNotExposed { .. }) => {
                Would::NotExposed
            }
            Some(Denied::NotAuthenticated { .. } | Denied::CredentialExpired { .. }) => {
                Would::NeedAuthentication
            }
            Some(Denied::MissingScope { .. }) => Would::NeedScope,
            Some(Denied::NeedsApproval { .. }) => Would::NeedApproval,
            Some(Denied::EffectUnstated { .. }) => Would::NeedEffect,
            Some(Denied::QuotaExhausted { .. }) => Would::Exhausted,
            Some(Denied::Intercepted { .. }) => Would::Refuse,
        }
    }

    /// The name this appears under in a document.
    pub fn name(self) -> &'static str {
        match self {
            Would::Allow => "allow",
            Would::NotExposed => "not_exposed",
            Would::NeedAuthentication => "need_authentication",
            Would::NeedScope => "need_scope",
            Would::NeedApproval => "need_approval",
            Would::NeedEffect => "need_effect",
            Would::Exhausted => "exhausted",
            Would::Refuse => "refuse",
        }
    }
}

/// What one operation would do, and what an operator can do about it.
#[derive(Debug, Clone)]
pub struct Prediction {
    target: String,
    operation: String,
    caller: String,
    would: Would,
    dry: DryRun,
    /// The scope the contract asks for and the caller has not got. The
    /// actionable part of a scope refusal is its *name*.
    scope: Option<String>,
    /// The `ai_effect` value that makes this operation need a human, if it
    /// does — reported whether or not an approval is already in hand, since
    /// "allowed because you passed an approval" is not the same finding as
    /// "harmless".
    effect: Option<String>,
    /// Whether `effect` was written by the **operator** (as this exposure's
    /// [`crate::policy::Unannotated`] assumption) rather than by the contract.
    ///
    /// The field an operator signing an exposure off reads before anything
    /// else: a page of `allow` rows resting on an assumption and a page resting
    /// on annotations are the same page otherwise, and only one of them is a
    /// statement about the software.
    effect_assumed: bool,
    /// Who may approve, if the contract says. See [`AI_APPROVER`].
    approver: Option<String>,
    /// Whether the contract declares this operation at all.
    declared: bool,
}

impl Prediction {
    /// The classification.
    pub fn would(&self) -> Would {
        self.would
    }

    /// The verdict in the live gate's own currency, for comparing the two.
    pub fn verdict(&self) -> Result<(), Denied> {
        self.dry.verdict()
    }

    /// Which stage refused, if one did.
    pub fn stage(&self) -> Option<&'static str> {
        self.dry.refusal().map(|(stage, _)| stage)
    }

    /// Every stage's part, including the ones that never ran.
    pub fn chain(&self) -> &DryRun {
        &self.dry
    }

    /// Whether the contract declares the operation.
    ///
    /// A `false` here alongside [`Would::Allow`] is a real finding and not a
    /// contradiction: the gates check *permission*, not *existence*, so an
    /// undeclared name on a wholesale-exposed interface passes every one of
    /// them and then fails at argument mapping. The report says both facts
    /// rather than resolving them into a verdict the gate did not give.
    pub fn declared(&self) -> bool {
        self.declared
    }

    /// The full document, per-stage detail included.
    pub fn to_json(&self) -> Json {
        let mut fields = vec![("target", s(&self.target)), ("caller", s(&self.caller))];
        fields.extend(self.row_fields());
        fields.push((
            "stages",
            Json::Array(
                self.dry
                    .stages()
                    .map(|(name, outcome)| {
                        let mut f = vec![("stage", s(name)), ("outcome", s(outcome_name(outcome)))];
                        if let StageOutcome::Refused(why) = outcome {
                            f.push(("why", s(why.to_string())));
                        }
                        obj(f)
                    })
                    .collect(),
            ),
        ));
        obj(fields)
    }

    /// One row of a [`survey`]: the same facts without the ones the enclosing
    /// document already states, and without the per-stage detail — a survey of
    /// a large estate is read as a table, and the stage that refused is the
    /// part of the walk that survives being read that way. Ask [`predict`]
    /// again for the row you care about to see the whole walk.
    fn to_row(&self) -> Json {
        obj(self.row_fields())
    }

    fn row_fields(&self) -> Vec<(&'static str, Json)> {
        let mut f = vec![
            ("operation", s(&self.operation)),
            ("would", s(self.would.name())),
            ("declared", Json::Bool(self.declared)),
        ];
        if let Some((stage, why)) = self.dry.refusal() {
            f.push(("stage", s(stage)));
            f.push(("why", s(why.to_string())));
        }
        if let Some(scope) = &self.scope {
            f.push(("scope", s(scope)));
        }
        if let Some(effect) = &self.effect {
            f.push(("effect", s(effect)));
            // Only when it is not the contract's own word. A deployment on an
            // annotated contract sees the document it always saw.
            if self.effect_assumed {
                f.push(("effect_stated_by", s("exposure")));
            }
        }
        if let Some(approver) = &self.approver {
            f.push(("approver", s(approver)));
        }
        f
    }
}

fn outcome_name(outcome: &StageOutcome) -> &'static str {
    match outcome {
        StageOutcome::Proceeded => "proceeded",
        StageOutcome::Refused(_) => "refused",
        StageOutcome::NotReached => "not_reached",
    }
}

/// Runs `chain` against `ctx` without calling anything, and reads the answer.
///
/// The chain is the caller's own — the one its calls go through, extensions
/// included — so a deployment that has filled [`crate::interceptor::SEAT_QUOTA`]
/// sees its own stage in the answer.
pub fn predict(chain: &mut Chain, ctx: &CallContext<'_>) -> Prediction {
    // Read off the chain's own approval stage rather than from a copy, so a
    // report cannot describe a posture the gate is not taking.
    let assumption = chain.unannotated().cloned();
    let dry = chain.dry_run(ctx);
    let refusal = dry.refusal().map(|(_, why)| why.clone());
    let would = Would::of(refusal.as_ref());
    let scope = match &refusal {
        Some(Denied::MissingScope { required, .. } | Denied::NotAuthenticated { required, .. }) => {
            Some(required.clone())
        }
        _ => None,
    };
    // What the row names as the effect, and who said it. An `Unstated` that the
    // chain refused needs no value here — its `why` names the annotation that is
    // missing, which is the actionable half. An `Unstated` the chain *allowed*
    // needs one badly: that row is indistinguishable from a genuinely
    // `read_only` one otherwise, and the difference is the whole of what an
    // operator is signing.
    let (effect, effect_assumed) =
        match (stated_effect(ctx.registry, ctx.target, ctx.operation), &assumption) {
            (Effect::Stated(e), _) => (Some(e), false),
            (Effect::Unstated, Some(Unannotated::Assume(a))) => (Some(a.clone()), true),
            _ => (None, false),
        };
    let approver = ctx
        .registry
        .resolve_operation(ctx.target, ctx.operation)
        .and_then(|(_, sig)| sig.annotations.get(AI_APPROVER).cloned());
    Prediction {
        target: ctx.target.to_owned(),
        operation: ctx.operation.to_owned(),
        caller: ctx.caller.map_or(NOBODY, |c| c.principal.as_str()).to_owned(),
        would,
        // An attribute accessor is declared — as an attribute. Reporting it
        // undeclared would tell an operator the catalog does not know the
        // name, when the catalog is precisely where it came from.
        declared: ctx.registry.resolve_operation(ctx.target, ctx.operation).is_some()
            || crate::policy::declares_accessor(ctx.registry, ctx.target, ctx.operation),
        dry,
        scope,
        effect,
        effect_assumed,
        approver,
    }
}

/// The question an operator actually asks: for this caller and this exposure,
/// here is every operation and what would happen to it.
///
/// `only` narrows to one repository id; `None` covers everything the exposure
/// names. An interface named by `only` that the exposure does *not* cover is
/// still surveyed — every row comes back [`Would::NotExposed`] from the real
/// chain, which is the answer to "what if I pointed an agent at this?".
///
/// The operation set for an interface is what the contract declares, its
/// inherited operations included, **union** whatever the exposure names for it.
/// The union is what surfaces a misconfiguration: an exposure line for an
/// operation the contract does not have appears with `declared: false` rather
/// than silently allowlisting nothing.
///
/// Attributes are absent because they are not callable through this bridge:
/// `Registry::resolve_operation` synthesizes no `_get_`/`_set_` operations, so
/// there is nothing for the gate to be asked about. That is a gap in the
/// bridge, stated here rather than papered over by a row the gate never sees.
pub fn survey(
    chain: &mut Chain,
    registry: &Registry,
    exposure: &Exposure,
    caller: Option<&Caller>,
    approval: Approval,
    only: Option<&str>,
) -> Json {
    let mut ids: Vec<String> = exposure
        .interfaces()
        .filter(|id| only.is_none_or(|want| want == id.as_str()))
        .cloned()
        .collect();
    if let Some(want) = only
        && !ids.iter().any(|id| id == want)
    {
        ids.push(want.to_owned());
    }
    ids.sort();
    ids.dedup();

    let mut interfaces = Vec::new();
    let mut unknown = Vec::new();
    let mut totals = [0usize; Would::ALL.len()];

    for id in ids {
        let known = registry.interface(&id).is_some();
        let operations = operations_of(registry, exposure, &id);
        if !known && operations.is_empty() {
            // Named by an operator and unknown to the catalog: a configuration
            // error worth finding before a deployment rather than after one.
            // An interface that *is* in the catalog and declares nothing keeps
            // its entry, empty — "exposed and has no operations" is a true
            // statement about it, and "not in the catalog" would not be.
            unknown.push(s(&id));
            continue;
        }
        let mut counts = [0usize; Would::ALL.len()];
        let mut rows = Vec::new();
        for operation in &operations {
            let ctx = CallContext {
                registry,
                caller,
                target: id.as_str(),
                operation: operation.as_str(),
                approval,
                arguments: None,
            };
            let prediction = predict(chain, &ctx);
            let at = Would::ALL.iter().position(|w| *w == prediction.would()).unwrap_or_default();
            counts[at] += 1;
            totals[at] += 1;
            rows.push(prediction.to_row());
        }
        interfaces.push(obj([
            ("id", s(&id)),
            ("exposed", Json::Bool(exposure.exposes(&id))),
            // `exposed: true, known: false` is an exposure line pointing at an
            // interface the catalog does not have — a typo, a stale id, or an
            // id that was mis-parsed on the way in. It reaches this far
            // (rather than `unknown_interfaces`) whenever the line also names
            // operations, which is exactly the case that looks most like it is
            // working.
            ("known", Json::Bool(known)),
            ("operations", Json::Array(rows)),
            ("summary", summary(&counts)),
        ]));
    }

    obj([
        ("caller", s(caller.map_or(NOBODY, |c| c.principal.as_str()))),
        (
            "scopes",
            Json::Array(caller.map(|c| c.scopes.as_slice()).unwrap_or(&[]).iter().map(s).collect()),
        ),
        ("approval", obj([("destructive_approved", Json::Bool(approval.destructive_approved))])),
        // Stated once, at the top, because it conditions every row below it.
        // `refuse` is the default posture; anything else is an operator's
        // declaration about the operations whose contracts say nothing, and a
        // page of `allow` rows means a different thing under each.
        (
            "unannotated_effect",
            match exposure.unannotated() {
                Unannotated::Refuse => s("refuse"),
                Unannotated::Assume(effect) => s(effect),
            },
        ),
        ("interfaces", Json::Array(interfaces)),
        ("unknown_interfaces", Json::Array(unknown)),
        ("summary", summary(&totals)),
    ])
}

/// Every operation name the gate could be asked about for `id`: the resolved
/// surface ([`crate::resolved_operations`]), union what the exposure names.
/// Sorted, because a report an operator diffs must not reorder itself.
///
/// The ancestor walk used to be written out here, and *only* here — which is
/// how the dry run came to judge thirteen operations of an object
/// `describe_interface` showed eleven of (RC-8). It is now the crate's one
/// walk, and `the_described_surface_is_the_surveyed_surface` holds this
/// function and `describe_interface` to the same set.
pub(crate) fn operations_of(registry: &Registry, exposure: &Exposure, id: &str) -> Vec<String> {
    let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    names.extend(crate::resolved_operations(registry, id).into_iter().map(|(name, _, _)| name));
    // Attribute accessors are operations on the wire, and an exposure that
    // names the interface makes them callable. Leaving them out of the survey
    // meant an operator previewing a deployment saw `ping` and not
    // `_get_balance`, while an agent could call both — which is exactly the
    // surprise the dry run exists to remove.
    for (attr, _, sig) in crate::resolved_attributes(registry, id) {
        names.insert(format!("_get_{attr}"));
        if !sig.readonly {
            names.insert(format!("_set_{attr}"));
        }
    }
    names.extend(exposure.allowed_operations(id).cloned());
    names.into_iter().collect()
}

fn summary(counts: &[usize; Would::ALL.len()]) -> Json {
    Json::Object(
        Would::ALL
            .iter()
            .zip(counts)
            .map(|(w, n)| (w.name().to_owned(), Json::Number(n.to_string())))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use orbweaver_cdr::{Encoder, Endian};
    use orbweaver_giop::{Error as GiopError, Invoker, Reply};

    use super::*;
    use crate::guard::{DECISION_DRY_RUN_ALLOW, DECISION_DRY_RUN_REFUSE, Guarded};
    use crate::interceptor::{Interceptor, Outcome, STAGE_APPROVAL, STAGE_EXPOSURE, STAGE_SCOPES};

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
          //@ ai_effect: destructive
          //@ ai_approver: the duty risk officer
          void close();
          // Deliberately unannotated, and the fixture is better for it: one
          // operation whose contract says nothing gives the survey a fourth
          // verdict, so the document under test is one an operator could
          // actually read something off.
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
        CallContext { registry: reg, caller, target: ACCOUNT, operation, approval, arguments: None }
    }

    /// The property the whole feature stands on, and the one an operator is
    /// trusting when they read a report instead of making a call: the dry run
    /// and the live gate give the same verdict, refuse at the same stage, and
    /// refuse for the same reason — over the cross product of exposures,
    /// callers, operations and approvals.
    ///
    /// Structurally they cannot differ (one `Chain::walk`, two entry points),
    /// so this is the regression test for that structure, and it compares
    /// against **both** live compositions: `Chain::run`, which a call runs, and
    /// `Exposure::check_call`, which `Bridge::check` answers from.
    ///
    /// The live side's refusing stage is captured from the live run itself —
    /// a probe stage registered just inside the audit stage, which therefore
    /// sees the [`crate::interceptor::CallResult::Refused`] every gate produces
    /// — rather than read off a second dry run, which would only prove that a
    /// dry run agrees with itself.
    #[test]
    fn a_dry_run_and_the_live_gate_answer_alike() {
        use std::cell::RefCell;
        use std::rc::Rc;

        use crate::interceptor::{CallResult, Record, STAGE_AUDIT};

        struct Probe(Rc<RefCell<Option<&'static str>>>);
        impl Interceptor for Probe {
            fn before(&mut self, _ctx: &CallContext<'_>) -> Outcome {
                Outcome::Proceed
            }
            fn after(&mut self, _ctx: &CallContext<'_>, result: &CallResult<'_>) {
                if let CallResult::Refused { stage, .. } = result {
                    *self.0.borrow_mut() = Some(stage);
                }
            }
            fn record(&self) -> Record<'_> {
                Record::Nothing
            }
        }

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
                        let case = format!("{ename}/{cname}/{operation}/approved={approved}");
                        let call = ctx(&reg, *caller, operation, approval);

                        // The live gate, run for real, with a probe watching
                        // which stage refuses.
                        let refuser = Rc::new(RefCell::new(None));
                        let mut live = Chain::standard(exposure.clone());
                        assert!(live.insert_after(
                            STAGE_AUDIT,
                            "test.probe",
                            Probe(Rc::clone(&refuser))
                        ));
                        let expected = live.run(&call);
                        let expected_stage = *refuser.borrow();

                        // The dry run, on its own chain.
                        let mut chain = Chain::standard(exposure.clone());
                        let got = predict(&mut chain, &call);

                        assert_eq!(got.verdict().err(), expected.clone().err(), "{case}");
                        assert_eq!(
                            got.verdict().err(),
                            exposure.check_call(&reg, ACCOUNT, operation, approval, *caller).err(),
                            "{case}: and the deterministic composition too"
                        );
                        assert_eq!(got.stage(), expected_stage, "{case}: refused by which stage");
                        assert_eq!(
                            got.would() == Would::Allow,
                            expected.is_ok(),
                            "{case}: the classification must not disagree with the verdict"
                        );
                    }
                }
            }
        }
    }

    /// Which stage refused is the actionable half of the answer, so the
    /// mapping from a refusal to a stage is pinned rather than left to the
    /// cross product to imply.
    #[test]
    fn each_refusal_names_the_stage_that_made_it() {
        let reg = registry(IDL);
        let bob = Caller::new("bob");
        let cases = [
            (Exposure::nothing(), None, "balance", STAGE_EXPOSURE, Would::NotExposed),
            (
                Exposure::nothing().allow_operation(ACCOUNT, "balance"),
                None,
                "touch",
                STAGE_EXPOSURE,
                Would::NotExposed,
            ),
            (
                Exposure::nothing().allow_interface(ACCOUNT),
                None,
                "deposit",
                STAGE_SCOPES,
                Would::NeedAuthentication,
            ),
            (
                Exposure::nothing().allow_interface(ACCOUNT),
                Some(&bob),
                "deposit",
                STAGE_SCOPES,
                Would::NeedScope,
            ),
            (
                Exposure::nothing().allow_interface(ACCOUNT),
                None,
                "close",
                STAGE_APPROVAL,
                Would::NeedApproval,
            ),
        ];
        for (exposure, caller, operation, stage, would) in cases {
            let mut chain = Chain::standard(exposure);
            let p = predict(&mut chain, &ctx(&reg, caller, operation, Approval::default()));
            assert_eq!(p.stage(), Some(stage), "{operation}");
            assert_eq!(p.would(), would, "{operation}");
        }
    }

    /// The stages past a refusal must not read as approvals: an operation
    /// stopped at the exposure has said nothing about its scopes.
    #[test]
    fn a_stage_the_refusal_short_circuited_reports_not_reached() {
        let reg = registry(IDL);
        let mut chain = Chain::standard(Exposure::nothing());
        let p = predict(&mut chain, &ctx(&reg, None, "deposit", Approval::default()));
        let seen: Vec<(&str, &str)> =
            p.chain().stages().map(|(n, o)| (n, outcome_name(o))).collect();
        assert_eq!(
            seen,
            vec![
                ("audit", "proceeded"),
                ("telemetry", "proceeded"),
                ("authz.exposure", "refused"),
                ("authz.scopes", "not_reached"),
                ("safety.approval", "not_reached"),
            ]
        );
        // And the document says the same thing, since that is what is read.
        let doc = p.to_json().to_string();
        assert!(doc.contains("not_reached"), "{doc}");
    }

    /// The actionable part of a scope refusal is the scope's *name*: an
    /// operator reading "refused" learns nothing they can act on, and one
    /// reading `accounts:write` has the line to add to a role.
    #[test]
    fn a_missing_scope_is_reported_by_name() {
        let reg = registry(IDL);
        let bob = Caller::new("bob");
        let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
        let p = predict(&mut chain, &ctx(&reg, Some(&bob), "deposit", Approval::default()));
        assert_eq!(p.to_json().get("scope").and_then(Json::as_str), Some("accounts:write"));
    }

    /// A destructive operation reports the effect and, when the contract names
    /// one, the approver. Both are data from the catalog; nothing acts on them.
    #[test]
    fn a_destructive_operation_reports_the_approval_and_who_gives_it() {
        let reg = registry(IDL);
        let exposure = Exposure::nothing().allow_interface(ACCOUNT);
        let mut chain = Chain::standard(exposure.clone());
        let p = predict(&mut chain, &ctx(&reg, None, "close", Approval::default()));
        let doc = p.to_json();
        assert_eq!(doc.get("would").and_then(Json::as_str), Some("need_approval"));
        assert_eq!(doc.get("effect").and_then(Json::as_str), Some("destructive"));
        assert_eq!(doc.get("approver").and_then(Json::as_str), Some("the duty risk officer"));

        // With the approval in hand it is allowed — and still reported as
        // destructive, because "allowed because you held an approval" is not
        // the same finding as "harmless".
        let mut chain = Chain::standard(exposure);
        let approved =
            predict(&mut chain, &ctx(&reg, None, "close", Approval { destructive_approved: true }));
        let doc = approved.to_json();
        assert_eq!(doc.get("would").and_then(Json::as_str), Some("allow"));
        assert_eq!(doc.get("effect").and_then(Json::as_str), Some("destructive"));
    }

    /// Hidden and refused are different answers with different fixes, and the
    /// document must not blur them.
    #[test]
    fn an_operation_the_exposure_hides_reads_as_not_exposed() {
        let reg = registry(IDL);
        let mut chain = Chain::standard(Exposure::nothing().allow_operation(ACCOUNT, "balance"));
        let p = predict(&mut chain, &ctx(&reg, None, "touch", Approval::default()));
        assert_eq!(p.would(), Would::NotExposed);
        let doc = p.to_json();
        assert_eq!(doc.get("would").and_then(Json::as_str), Some("not_exposed"));
        assert_eq!(doc.get("stage").and_then(Json::as_str), Some(STAGE_EXPOSURE));
    }

    /// An operation the contract does not declare. The gates still check
    /// *permission* rather than existence — `declared: false` is the report's
    /// answer to "does this exist", and it is a separate field from the
    /// verdict, so the two facts are never resolved into one.
    ///
    /// The verdict is `need_effect`, because a contract that does not declare
    /// an operation is maximally silent about what it does. That answer is
    /// **byte-identical to the one a declared-but-unannotated operation gets**,
    /// which is the property this test exists to hold: a refusal must never
    /// become an oracle for what exists behind it. It used to be `allow` for
    /// both, which had the same non-oracle property and the wrong default.
    #[test]
    fn a_refusal_does_not_reveal_whether_the_operation_exists() {
        let reg = registry(IDL);
        let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
        let invented = predict(&mut chain, &ctx(&reg, None, "no_such_op", Approval::default()));
        // `touch` is declared and carries no `ai_effect`; `no_such_op` is not
        // declared at all. The gate must not be able to tell a caller which.
        let real = predict(&mut chain, &ctx(&reg, None, "touch", Approval::default()));
        assert_eq!(invented.would(), Would::NeedEffect);
        assert_eq!(real.would(), invented.would(), "the verdicts must be indistinguishable");
        assert_eq!(real.stage(), invented.stage());

        // Existence is still reported, as its own field, to the operator
        // reading the document — never as the verdict.
        assert!(!invented.declared());
        assert!(real.declared());
        assert_eq!(invented.to_json().get("declared"), Some(&Json::Bool(false)));

        // Named operation by operation, the same unknown name is hidden
        // instead — and then it is the exposure that answers, not the catalog.
        let mut chain = Chain::standard(Exposure::nothing().allow_operation(ACCOUNT, "balance"));
        let p = predict(&mut chain, &ctx(&reg, None, "no_such_op", Approval::default()));
        assert_eq!(p.would(), Would::NotExposed);
        assert!(!p.declared());
    }

    /// A deployment's own stage answers in the survey like a built-in one.
    #[test]
    fn a_stage_nobody_here_wrote_appears_in_the_answer() {
        struct Closed;
        impl Interceptor for Closed {
            fn before(&mut self, _ctx: &CallContext<'_>) -> Outcome {
                Outcome::Refuse(Denied::Intercepted {
                    stage: "quota.rate_limit".to_owned(),
                    reason: "the window is exhausted".to_owned(),
                })
            }
        }
        let reg = registry(IDL);
        let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
        assert!(chain.insert_after(STAGE_SCOPES, "quota.rate_limit", Closed));
        let p = predict(&mut chain, &ctx(&reg, None, "balance", Approval::default()));
        assert_eq!(p.would(), Would::Refuse);
        assert_eq!(p.stage(), Some("quota.rate_limit"));
        assert!(p.to_json().to_string().contains("window is exhausted"), "{}", p.to_json());
    }

    // --- the audit and the counters ------------------------------------------

    /// The audit decision, run: a dry run is on the record, it is
    /// distinguishable from a real decision by its first field, and it is
    /// written by the one formatter — every other field is in the same place.
    #[test]
    fn a_dry_run_is_audited_and_cannot_be_read_as_a_call() {
        let reg = registry(IDL);
        let exposure = Exposure::nothing().allow_interface(ACCOUNT);
        let mut chain = Chain::standard(exposure);
        let alice = Caller::new("alice");
        predict(&mut chain, &ctx(&reg, Some(&alice), "balance", Approval::default()));
        predict(&mut chain, &ctx(&reg, Some(&alice), "close", Approval::default()));

        assert_eq!(chain.audit().len(), 2);
        assert_eq!(
            chain.audit()[0],
            format!("{DECISION_DRY_RUN_ALLOW} caller=alice target={ACCOUNT} operation=balance")
        );
        assert!(
            chain.audit()[1].starts_with(&format!(
                "{DECISION_DRY_RUN_REFUSE} caller=alice target={ACCOUNT} operation=close why="
            )),
            "{}",
            chain.audit()[1]
        );
        // The distinguishing token is the decision and nothing else: strip it
        // and what remains is the body a real call would have written, field
        // for field. One format, two decisions.
        for line in chain.audit() {
            let (decision, rest) = line.split_once(' ').expect("a decision and a body");
            assert!(crate::guard::is_hypothetical(decision), "{line}");
            assert!(rest.starts_with(&format!("caller=alice target={ACCOUNT} ")), "{line}");
        }
    }

    /// The promotion statistics are the promotion policy's only input. A
    /// question must not be able to write into them.
    #[test]
    fn a_dry_run_leaves_the_promotion_counters_untouched() {
        let reg = registry(IDL);
        let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
        for _ in 0..25 {
            for operation in ["balance", "deposit", "close", "touch"] {
                predict(&mut chain, &ctx(&reg, None, operation, Approval::default()));
            }
        }
        for operation in ["balance", "deposit", "close", "touch"] {
            assert_eq!(chain.stats().calls(ACCOUNT, operation), 0, "{operation}");
            assert_eq!(chain.stats().failures(ACCOUNT, operation), 0, "{operation}");
        }
        assert_eq!(chain.audit().len(), 100, "and every question is still on record");
    }

    // --- nothing reaches the wire --------------------------------------------

    /// An invoker that fails the test by being used at all.
    struct Detonator;

    impl Invoker for Detonator {
        fn endian(&self) -> Endian {
            Endian::Big
        }
        fn invoke<F: Fn(&mut Encoder)>(
            &mut self,
            operation: &str,
            _write_args: F,
        ) -> Result<Reply, GiopError> {
            panic!("a dry run reached the wire: invoke({operation})");
        }
        fn invoke_oneway<F: Fn(&mut Encoder)>(
            &mut self,
            operation: &str,
            _write_args: F,
        ) -> Result<(), GiopError> {
            panic!("a dry run reached the wire: invoke_oneway({operation})");
        }
    }

    /// The property stated as a test rather than as an intention: a guard whose
    /// transport detonates on contact answers for every operation of its
    /// interface, including the ones it would allow.
    #[test]
    fn a_dry_run_never_touches_the_invoker() {
        let reg = registry(IDL);
        let mut g: Guarded<'_, Detonator> = Guarded::assemble(
            Detonator,
            &reg,
            Exposure::nothing().allow_interface(ACCOUNT),
            Some(Caller::new("alice").with_scope("accounts:write")),
            ACCOUNT.to_owned(),
            Approval::default(),
        );
        let mut allowed = 0;
        for operation in ["balance", "deposit", "close", "touch", "no_such_op"] {
            if g.dry_run(operation).would() == Would::Allow {
                allowed += 1;
            }
        }
        // `balance` and `deposit`: the two the contract describes and this
        // caller may have. `close` needs a human, and `touch` and the invented
        // name are silences the contract never described.
        assert_eq!(allowed, 2, "balance and deposit");
        assert_eq!(g.stats().calls(ACCOUNT, "balance"), 0);
        assert_eq!(g.audit().len(), 5, "five questions, five lines, no calls");
    }

    // --- the survey ----------------------------------------------------------

    #[test]
    fn a_survey_answers_for_every_operation_at_once() {
        let reg = registry(IDL);
        let exposure = Exposure::nothing().allow_interface(ACCOUNT);
        let bob = Caller::new("bob");
        let mut chain = Chain::standard(exposure.clone());
        let doc = survey(&mut chain, &reg, &exposure, Some(&bob), Approval::default(), None);

        assert_eq!(doc.get("caller").and_then(Json::as_str), Some("bob"));
        let Some(Json::Array(interfaces)) = doc.get("interfaces") else { panic!("{doc}") };
        assert_eq!(interfaces.len(), 1);
        let Some(Json::Array(ops)) = interfaces[0].get("operations") else { panic!("{doc}") };
        let seen: Vec<(&str, &str)> = ops
            .iter()
            .map(|o| {
                (
                    o.get("operation").and_then(Json::as_str).unwrap_or(""),
                    o.get("would").and_then(Json::as_str).unwrap_or(""),
                )
            })
            .collect();
        // One of each verdict the contract can produce, which is what makes
        // this document readable: an operator sees four different answers and
        // four different things to do about them. A survey whose every row says
        // the same word carries no signal whatever it says — the estate pilot
        // measured 7,253 bytes of `allow` and called it the unusable gate.
        assert_eq!(
            seen,
            vec![
                ("balance", "allow"),
                ("close", "need_approval"),
                ("deposit", "need_scope"),
                ("touch", "need_effect"),
            ]
        );
        assert_eq!(
            doc.get("summary").and_then(|x| x.get("allow")),
            Some(&Json::Number("1".into()))
        );
        assert_eq!(
            doc.get("summary").and_then(|x| x.get("need_effect")),
            Some(&Json::Number("1".into()))
        );
        // The posture every row above was judged under, stated once at the top.
        assert_eq!(doc.get("unannotated_effect").and_then(Json::as_str), Some("refuse"));
        assert_eq!(
            doc.get("summary").and_then(|x| x.get("need_scope")),
            Some(&Json::Number("1".into()))
        );
        // Every classification is a key whether or not it occurred, so two
        // surveys of the same estate diff cleanly.
        for w in Would::ALL {
            assert!(doc.get("summary").and_then(|x| x.get(w.name())).is_some(), "{}", w.name());
        }
    }

    /// Inherited operations are callable, so a survey that stopped at the
    /// declaring interface would under-report what an agent could reach.
    #[test]
    fn a_survey_covers_inherited_operations() {
        let reg = registry(
            "module m { interface Base { //@ ai_effect: destructive\n void wipe(); }; \
             interface Derived : Base { void ping(); }; };",
        );
        let exposure = Exposure::nothing().allow_interface("IDL:m/Derived:1.0");
        let mut chain = Chain::standard(exposure.clone());
        let doc = survey(&mut chain, &reg, &exposure, None, Approval::default(), None);
        let text = doc.to_string();
        assert!(text.contains("wipe"), "{text}");
        assert!(text.contains("need_approval"), "{text}");
    }

    /// Two configuration errors an operator wants before a deployment, not
    /// after one: an exposure naming an interface the catalog has never heard
    /// of, and one naming an operation the contract does not declare.
    #[test]
    fn a_survey_finds_an_exposure_that_names_something_that_does_not_exist() {
        let reg = registry(IDL);
        let exposure = Exposure::nothing()
            .allow_interface("IDL:ghost/Missing:1.0")
            .allow_operation(ACCOUNT, "balance")
            .allow_operation(ACCOUNT, "frobnicate");
        let mut chain = Chain::standard(exposure.clone());
        let doc = survey(&mut chain, &reg, &exposure, None, Approval::default(), None);

        assert_eq!(
            doc.get("unknown_interfaces"),
            Some(&Json::Array(vec![s("IDL:ghost/Missing:1.0")]))
        );
        let Some(Json::Array(interfaces)) = doc.get("interfaces") else { panic!("{doc}") };
        assert_eq!(interfaces[0].get("known"), Some(&Json::Bool(true)));
        let Some(Json::Array(ops)) = interfaces[0].get("operations") else { panic!("{doc}") };
        let frob = ops
            .iter()
            .find(|o| o.get("operation").and_then(Json::as_str) == Some("frobnicate"))
            .expect("the exposure named it, so the survey must account for it");
        assert_eq!(frob.get("declared"), Some(&Json::Bool(false)));
    }

    /// "What if I pointed an agent at this one?" — an interface outside the
    /// exposure is still surveyable, and every row is the real chain's
    /// not-exposed answer rather than a blank page.
    #[test]
    fn an_unexposed_interface_can_still_be_surveyed_and_reads_as_hidden() {
        let reg = registry(IDL);
        let exposure = Exposure::nothing();
        let mut chain = Chain::standard(exposure.clone());
        let doc = survey(&mut chain, &reg, &exposure, None, Approval::default(), Some(ACCOUNT));
        let Some(Json::Array(interfaces)) = doc.get("interfaces") else { panic!("{doc}") };
        assert_eq!(interfaces.len(), 1);
        assert_eq!(interfaces[0].get("exposed"), Some(&Json::Bool(false)));
        assert_eq!(
            doc.get("summary").and_then(|x| x.get("not_exposed")),
            Some(&Json::Number("4".into())),
            "{doc}"
        );
    }

    /// The shape that produced the first real finding this report made: an
    /// exposure naming operations on an interface the catalog has never heard
    /// of reaches the report as a normal-looking entry, because it *has*
    /// operations to list. `known: false` is what makes it readable as the
    /// mistake it is. (The finding was `orbweaver-mcp-server --expose
    /// IDL:spike/Echo:1.0` splitting at the version's dot.)
    #[test]
    fn an_exposure_on_an_interface_the_catalog_lacks_is_marked_unknown() {
        let reg = registry(IDL);
        let exposure = Exposure::nothing().allow_operation("IDL:bank/Account:1", "0");
        let mut chain = Chain::standard(exposure.clone());
        let doc = survey(&mut chain, &reg, &exposure, None, Approval::default(), None);
        let Some(Json::Array(interfaces)) = doc.get("interfaces") else { panic!("{doc}") };
        assert_eq!(interfaces[0].get("exposed"), Some(&Json::Bool(true)));
        assert_eq!(interfaces[0].get("known"), Some(&Json::Bool(false)), "{doc}");
    }

    /// An interface the catalog *has* and that declares nothing keeps its
    /// entry, empty. Reporting it as unknown would be a false statement about
    /// the catalog, and "exposed, and there is nothing behind it" is the
    /// finding an operator wants.
    #[test]
    fn an_interface_that_declares_nothing_is_still_in_the_catalog() {
        let reg = registry("module m { interface Hollow {}; };");
        let exposure = Exposure::nothing().allow_interface("IDL:m/Hollow:1.0");
        let mut chain = Chain::standard(exposure.clone());
        let doc = survey(&mut chain, &reg, &exposure, None, Approval::default(), None);
        assert_eq!(doc.get("unknown_interfaces"), Some(&Json::Array(vec![])), "{doc}");
        let Some(Json::Array(interfaces)) = doc.get("interfaces") else { panic!("{doc}") };
        assert_eq!(interfaces[0].get("known"), Some(&Json::Bool(true)));
        assert_eq!(interfaces[0].get("operations"), Some(&Json::Array(vec![])), "{doc}");
    }

    /// A survey is a document an operator diffs between two exposures, so it
    /// must be byte-stable for the same inputs.
    #[test]
    fn a_survey_is_reproducible_and_reparses() {
        let reg = registry(IDL);
        let exposure = Exposure::nothing().allow_interface(ACCOUNT);
        let once = {
            let mut chain = Chain::standard(exposure.clone());
            survey(&mut chain, &reg, &exposure, None, Approval::default(), None)
        };
        let twice = {
            let mut chain = Chain::standard(exposure.clone());
            survey(&mut chain, &reg, &exposure, None, Approval::default(), None)
        };
        assert_eq!(once.to_string(), twice.to_string());
        assert_eq!(Json::parse(&once.to_string()).unwrap(), once);
    }
}
