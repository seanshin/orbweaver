//! Contract assertions from annotations: does the SIDL vocabulary make sense
//! against the types it annotates?
//!
//! §2.2 is the argument this module tests. IDL is strict about syntax and
//! silent about meaning, so SIDL adds the meaning as structured comments, and
//! the registry carries them through to the two consumers that act on them:
//! `orbweaver-mcp`'s policy gate and `orbweaver-guard`'s audit path. An
//! annotation is therefore not decoration — it is an input to an authorization
//! decision — and an annotation that is wrong, misspelled or contradicted by
//! the signature it sits on has a consequence somebody will meet at runtime.
//!
//! # The rule for adding a rule
//!
//! **Every check must name the consumer that will act on it and what that
//! consumer will do.** Without that, a check is a style opinion, and style
//! opinions in a security-adjacent report train people to skim it.
//!
//! That test rejected two checks the design started with, and the reasoning is
//! worth keeping because both look obviously right:
//!
//! - **`ai_effect: read_only` with `out` parameters is *not* flagged.** An
//!   `out` parameter is a return channel, not a mutation: `get_totals(out long
//!   hits, out long misses)` is read-only in every sense that matters, and IDL
//!   has no other way to return two values. Flagging it would fire on ordinary,
//!   correct contracts, and a rule that fires on correct code is how a report
//!   gets ignored. `inout` is closer to interesting — the caller's value is
//!   consumed and replaced — but the replacement is still client-side, so it
//!   says nothing about server state either. Neither is evidence.
//!
//! - **`ai_idempotent` on a `oneway` is *not* flagged.** The vocabulary decides
//!   this. §2.2 defines `@annotation ai_idempotent { boolean value; }` with the
//!   comment "safe to retry", which is a property of the *effect* — calling
//!   twice lands the same state — and not a property of the reply. A oneway has
//!   no reply, which makes retry-safety *more* load-bearing rather than
//!   meaningless: a client that reconnects and cannot tell whether its message
//!   arrived has only the idempotence claim to decide on. Flagging it would be
//!   asserting something the vocabulary does not say.
//!
//!   What **is** flagged is the opposite: `ai_idempotent: false` on a `oneway`.
//!   There the contract states that delivery is unconfirmable *and* that retry
//!   is unsafe, which leaves a caller who loses a connection with no correct
//!   move at all. That is a design gap worth naming.
//!
//! **규칙을 추가하는 규칙: 그 검사를 소비하는 주체와 그 주체가 할 행동을
//! 명시할 수 없으면 규칙이 아니다.** 이 기준으로 `read_only` + `out` 검사와
//! `oneway` + `ai_idempotent` 검사를 뺐다 — 전자는 올바른 계약에서 발화하고,
//! 후자는 어휘가 말하지 않는 것을 주장한다.
//!
//! ## Two consumers that did not exist when the rule was written
//!
//! `orbweaver-mcp`'s **quota stage** (§4.5 #2) counts calls against a budget
//! keyed on `(caller, target, operation)`, and `orbweaver-forge`'s **S3i
//! inference** attaches `inferred_*` values that `infer::approve` later promotes
//! into the `ai_*` keys the gates read. Both act on things a contract decides,
//! so both can be named. The same test threw out more candidates than it let
//! through, and those are recorded here for the same reason the two above are —
//! a checker fills up with style opinions one plausible rule at a time.
//!
//! - **A quota refusal on a non-idempotent operation is *not* flagged.** The
//!   refusal reaches a stub as CORBA `TRANSIENT`, which invites a retry, and
//!   inviting a retry of something the contract says is unsafe to retry looks
//!   like a real contradiction. It is not: `Quota::before` refuses **before the
//!   invocation**, so nothing ran and there is nothing to repeat. Idempotence is
//!   a claim about repeating an effect, and a call that was refused had none.
//!
//! - **A `oneway` under a quota is *not* flagged.** The reasoning that a oneway
//!   has no reply to carry the refusal in is wrong here: `guard::Guarded::
//!   invoke_oneway` runs the chain first and returns the refusal to its caller
//!   locally, so the caller does learn. The gate is in the same process as the
//!   stub, not at the other end of the wire.
//!
//! - **An operation with no `ai_authz` is *not* flagged for sharing `<nobody>`'s
//!   budget.** Every unauthenticated session is one principal, which the quota's
//!   own documentation states as its honest limitation — but that is a property
//!   of whether the *host* wired an identity, not of anything the contract says.
//!   A finding that fires on every operation in a deployment that has no
//!   authentication is a finding about the deployment, filed against the IDL.
//!
//! - **`inferred_status: unapproved` is *not* flagged on its own.** It is
//!   exactly what `infer::worksheet` and `infer::exposure_refusal` exist to
//!   report, in more detail and with the ingestion source attached. A second
//!   opinion on the same fact does not make the fact more visible; it makes two
//!   reports that have to agree.
//!
//! - **`inferred_effect` carrying an ungating value is *not* flagged.**
//!   `infer::approve` already refuses it with `ApproveError::Ungating`, at the
//!   only door it can enter through. A check that duplicates an enforced gate
//!   adds a warning before a refusal, which is noise in front of a wall.
//!
//! - **An `inferred_*` mark with nothing promotable beside it is *not*
//!   flagged**, though it would genuinely wedge `approve` on
//!   `NothingInferred` — because `Inference::annotations` always writes
//!   `inferred_desc` and `inferred_effect`, so no producer in the tree can
//!   reach that state. Reporting it would be reporting a shape nobody has
//!   measured, which is the rule about unmeasured checks pointed the other way.
//!
//! # Severity
//!
//! Never [`Severity::Error`]; see the crate documentation. Within that,
//! [`Severity::Warning`] means the contract states something **no consumer
//! reads**, so the author believes a control exists that does not.
//! [`Severity::Advice`] means a consumer will act on it and it looks wrong.

use std::collections::BTreeMap;

use orbweaver_forge::infer::{
    INFERRED_PREFIX, MARK_BASIS, MARK_EVIDENCE, MARK_SOURCE, MARK_STATUS,
};
use orbweaver_forge::{Finding, Severity};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_registry::{Entry, OperationSig, ParamDirection, Registry, RepositoryId};

use crate::finding;

/// Every `ai_*` key SIDL v1 defines (§2.2).
///
/// A key outside this list reaches the registry and is read by nobody, which is
/// what makes a typo worth reporting: the author sees their annotation in the
/// file and the guard sees nothing at all.
pub const VOCABULARY: [&str; 8] = [
    "ai_desc",
    "ai_unit",
    "ai_effect",
    "ai_idempotent",
    "ai_pii",
    "ai_example",
    "ai_precond",
    "ai_authz",
];

/// `ai_effect` values that `orbweaver-mcp` treats as needing no approval.
///
/// Mirrored from `crates/orbweaver-mcp/src/policy.rs::destructive_effect`,
/// which is the authority. Duplicated rather than imported because the
/// classifier there is private and this crate must not reach into a peer's
/// internals; the test `the_effect_vocabulary_matches_the_policy_gate` pins the
/// list so the copy cannot drift silently.
pub const UNGATED_EFFECTS: [&str; 4] = ["read_only", "readonly", "idempotent", "safe"];

/// The subset of [`UNGATED_EFFECTS`] that claims the operation **only reads**.
///
/// Not the same set, and conflating the two produced this crate's first false
/// positive: `//@ ai_effect: idempotent` on a oneway `prefetch` was reported as
/// "an operation that only reads returns its result to nobody", which is not
/// what `idempotent` says. Idempotence is a claim about *repetition* — calling
/// twice lands the same state — and a perfectly ordinary thing to declare about
/// an operation that writes. `read_only` and `safe` are claims about *effect*,
/// and those are the ones a mutating name or a missing reply can contradict.
///
/// 승인 면제(`UNGATED`)와 "읽기만 한다"(`READ_ONLY`)는 다른 집합이다. 둘을 같이
/// 취급한 것이 이 크레이트의 첫 오탐이었다.
pub const READ_ONLY_EFFECTS: [&str; 3] = ["read_only", "readonly", "safe"];

/// The one `ai_effect` value that means "needs a human" on purpose.
pub const GATED_EFFECTS: [&str; 1] = ["destructive"];

/// `ai_pii` levels §2.2 defines.
pub const PII_LEVELS: [&str; 3] = ["none", "low", "high"];

/// Verbs whose presence in an operation name suggests the operation changes
/// something.
///
/// A heuristic, and treated as one: it only ever produces advice, and it is
/// only consulted where the contract has said nothing about the effect. The
/// list is deliberately about *state-changing* verbs — `get`, `find`, `list`
/// and `query` are absent because their absence is what keeps the rule quiet on
/// the read side of a normal interface.
const MUTATING_VERBS: [&str; 26] = [
    "set_",
    "create",
    "delete",
    "remove",
    "update",
    "write",
    "insert",
    "drop",
    "purge",
    "reset",
    "shutdown",
    "commit",
    "transfer",
    "wipe",
    "clear",
    "store",
    "register",
    "unregister",
    "bind",
    "unbind",
    "rebind",
    "activate",
    "deactivate",
    "destroy",
    "kill",
    "revoke",
];

/// Every contract finding for one registry.
///
/// Sorted by repository id and then by rule, so two runs over the same input
/// produce the same report and a diff of two reports is about the contracts
/// rather than about hash order.
pub fn contract_findings(registry: &Registry) -> Vec<Finding> {
    let mut out = Vec::new();
    for id in registry.ids() {
        // A type carries annotations too, and nothing was reading them. The
        // checker's whole premise is that an `ai_*` key reaching the registry
        // and read by nobody is worth reporting — and a typo on a `typedef`
        // was exactly that, silently, including on the one place D006 proposed
        // putting a plane-rule marker.
        if !matches!(registry.get(id), Some(Entry::Interface(_)))
            && let Some(ann) = registry.annotations(id)
        {
            out.extend(unknown_keys(id, "the type", ann));
        }
        let Some(Entry::Interface(iface)) = registry.get(id) else { continue };
        if iface.forward_only {
            continue;
        }
        let iface_ann = registry.annotations(id);
        out.extend(interface_findings(id, iface_ann, iface));
        for (name, sig) in &iface.operations {
            out.extend(operation_findings(registry, id, name, sig));
        }
        for (name, attr) in &iface.attributes {
            out.extend(unknown_keys(id, &format!("attribute {name}"), &attr.annotations));
        }
    }
    out.sort_by(|a, b| a.source.cmp(&b.source).then(a.rule.cmp(&b.rule)));
    out
}

fn interface_findings(
    id: &RepositoryId,
    annotations: Option<&BTreeMap<String, String>>,
    iface: &orbweaver_registry::InterfaceEntry,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let Some(ann) = annotations else { return out };
    out.extend(unknown_keys(id, "the interface", ann));

    // The guard reads `ai_authz` from the *operation* signature and nowhere
    // else (`orbweaver-mcp/src/policy.rs::required_scopes`). A scope written on
    // the interface therefore enforces nothing, while looking in the source
    // exactly like a scope that does — which is worse than no scope, because
    // S4 reports a missing one and reports nothing about this.
    if let Some(scope) = ann.get("ai_authz") {
        let unscoped: Vec<&String> = iface
            .operations
            .iter()
            .filter(|(_, sig)| !sig.annotations.contains_key("ai_authz"))
            .map(|(name, _)| name)
            .collect();
        if !unscoped.is_empty() {
            out.push(finding(
                "contract/authz-on-interface-only",
                Severity::Warning,
                format!(
                    "{id} requires the scope {scope:?} on the interface, but the guard reads \
                     ai_authz per operation and never from the interface, so {} operation(s) \
                     are unscoped in practice: {}",
                    unscoped.len(),
                    unscoped.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                ),
                id.clone(),
                Some(format!(
                    "repeat `//@ ai_authz: {scope}` above each operation that needs it; \
                     interface-level scope inheritance is not implemented and an unenforced \
                     scope reads like an enforced one"
                )),
            ));
        }
    }
    out
}

fn operation_findings(
    registry: &Registry,
    id: &RepositoryId,
    name: &str,
    sig: &OperationSig,
) -> Vec<Finding> {
    let mut out = Vec::new();
    let at = format!("{id}.{name}");
    out.extend(unknown_keys(id, &format!("operation {name}"), &sig.annotations));

    let effect = sig.annotations.get("ai_effect").map(|s| s.trim());
    let authz = sig.annotations.get("ai_authz").map(|s| s.trim()).filter(|s| !s.is_empty());
    let reads_only = effect.is_some_and(|e| READ_ONLY_EFFECTS.contains(&e));

    // ── the effect vocabulary ───────────────────────────────────────────────
    if let Some(e) = effect
        && !UNGATED_EFFECTS.contains(&e)
        && !GATED_EFFECTS.contains(&e)
    {
        out.push(finding(
            "contract/effect-unknown",
            Severity::Advice,
            format!(
                "{at} declares ai_effect {e:?}, which is not in the vocabulary; the MCP policy \
                 gate treats any unrecognised value as needing human approval, so the operation \
                 is gated by accident rather than by intent"
            ),
            at.clone(),
            Some(format!(
                "use one of {} or {}",
                UNGATED_EFFECTS.join(", "),
                GATED_EFFECTS.join(", ")
            )),
        ));
    }

    // ── the claim against the name ──────────────────────────────────────────
    // A read-only claim on an operation named for a mutation is a genuine
    // contradiction between two things the same author wrote: one of them is
    // wrong, and the policy gate believes the annotation.
    if reads_only && let Some(verb) = mutating_verb(name) {
        out.push(finding(
            "contract/read-only-mutating-name",
            Severity::Advice,
            format!(
                "{at} is annotated {:?} but its name contains {verb:?}; the policy gate believes \
                 the annotation and will let an agent call it without approval",
                effect.unwrap_or_default()
            ),
            at.clone(),
            Some(
                "if the operation really does change state, annotate it `destructive`; if the \
                 name is misleading, rename it — an agent reads the name too"
                    .into(),
            ),
        ));
    }

    // ── the claim against the reply ─────────────────────────────────────────
    // A oneway has no reply at all. A read-only oneway therefore reads
    // something and tells nobody, which is either a wrong annotation or an
    // operation with no observable purpose.
    if reads_only && sig.oneway {
        out.push(finding(
            "contract/read-only-oneway",
            Severity::Advice,
            format!(
                "{at} is oneway and annotated {:?}; a oneway has no reply, so an operation that \
                 only reads returns its result to nobody",
                effect.unwrap_or_default()
            ),
            at.clone(),
            Some(
                "either the operation does change something — annotate the real effect — or it \
                 should return its result, which means it cannot be oneway"
                    .into(),
            ),
        ));
    }

    // ── idempotence ─────────────────────────────────────────────────────────
    if let Some(v) = sig.annotations.get("ai_idempotent") {
        let t = v.trim();
        match t.to_ascii_lowercase().as_str() {
            "true" | "false" => {
                // §2.2 types ai_idempotent as a boolean meaning "safe to
                // retry". On a oneway, delivery is unconfirmable; declaring
                // retry unsafe as well leaves a client that lost its connection
                // with no correct move.
                if t.eq_ignore_ascii_case("false") && sig.oneway {
                    out.push(finding(
                        "contract/oneway-not-idempotent",
                        Severity::Advice,
                        format!(
                            "{at} is oneway and declares ai_idempotent false; the caller cannot \
                             learn whether the call arrived and cannot safely retry it, so a \
                             lost connection loses the request with no recovery"
                        ),
                        at.clone(),
                        Some(
                            "give the operation a reply so delivery is observable, or make it \
                             idempotent so a blind retry is safe"
                                .into(),
                        ),
                    ));
                }
            }
            _ => out.push(finding(
                "contract/idempotent-not-boolean",
                Severity::Advice,
                format!(
                    "{at} declares ai_idempotent {t:?}; §2.2 types it as a boolean, so anything \
                     else is read as neither true nor false and the claim is lost"
                ),
                at.clone(),
                Some("write `true` or `false`".into()),
            )),
        }
    }

    // ── authorization ───────────────────────────────────────────────────────
    // Priority order matters: at most one missing-authz finding per operation,
    // reported against the strongest evidence available. Three findings saying
    // "add ai_authz" to one operation is the item-by-item report §5.1 exists to
    // avoid.
    if authz.is_none() {
        if effect.is_some_and(|e| !UNGATED_EFFECTS.contains(&e)) {
            out.push(finding(
                "contract/gated-without-authz",
                Severity::Advice,
                format!(
                    "{at} declares ai_effect {:?} but no ai_authz; approval and authorization are \
                     different gates — the operation needs a human to approve the call and no \
                     scope at all to be allowed to make it",
                    effect.unwrap_or_default()
                ),
                at.clone(),
                Some(
                    "add `//@ ai_authz: <scope>` naming the permission this operation needs".into(),
                ),
            ));
        } else if let Some(verb) = mutating_verb(name)
            && effect.is_none()
        {
            out.push(finding(
                "contract/mutating-name-without-authz",
                Severity::Advice,
                format!(
                    "{at} has neither ai_effect nor ai_authz and its name contains {verb:?}; the \
                     guard requires no scope for it, so any caller who reaches the bridge may \
                     call it"
                ),
                at.clone(),
                Some(
                    "annotate the effect and the scope: `//@ ai_effect: destructive` plus \
                     `//@ ai_authz: <scope>`, or `//@ ai_effect: read_only` if the name is \
                     misleading"
                        .into(),
                ),
            ));
        } else if let Some(what) = escaping_reference(registry, sig) {
            // §4.7 and R14: an object reference is a bearer address. An
            // operation that hands one out widens what its caller can reach,
            // which is an authorization question even when the operation reads
            // nothing.
            out.push(finding(
                "contract/reference-escapes-without-authz",
                Severity::Advice,
                format!(
                    "{at} hands out an object reference ({what}) and requires no scope; a \
                     reference is a bearer address (§4.7, R14), so this operation widens what \
                     its caller can reach even if it changes nothing itself"
                ),
                at.clone(),
                Some(
                    "add `//@ ai_authz: <scope>` for the reference it returns — obtaining a \
                     reference deserves the scope of what the reference can do"
                        .into(),
                ),
            ));
        } else if sig
            .params
            .iter()
            .any(|p| p.annotations.get("ai_pii").map(|s| s.trim()) == Some("high"))
        {
            out.push(finding(
                "contract/pii-without-authz",
                Severity::Advice,
                format!(
                    "{at} takes a parameter annotated ai_pii high and requires no scope; the \
                     data is marked sensitive by the contract and gated by nothing"
                ),
                at.clone(),
                Some("add `//@ ai_authz: <scope>` covering access to this data".into()),
            ));
        }
    }

    // ── inferred values against the keys they will be promoted into ─────────
    out.extend(inference_findings(&at, sig));

    // ── the budget the quota stage will key on ──────────────────────────────
    out.extend(quota_findings(registry, id, &at, name, effect));

    // ── parameter-level vocabulary against parameter types ──────────────────
    for p in &sig.params {
        out.extend(unknown_keys(id, &format!("parameter {} of {name}", p.name), &p.annotations));
        if let Some(level) = p.annotations.get("ai_pii") {
            let t = level.trim();
            if !PII_LEVELS.contains(&t) {
                out.push(finding(
                    "contract/pii-level-unknown",
                    Severity::Advice,
                    format!(
                        "{at} parameter {:?} declares ai_pii {t:?}; §2.2 defines {}",
                        p.name,
                        PII_LEVELS.join(" | ")
                    ),
                    at.clone(),
                    Some(format!("use one of {}", PII_LEVELS.join(", "))),
                ));
            }
        }
        // A unit is a claim about a quantity. On a string it is either
        // meaningless or it means the string carries a number the type does not
        // declare — which is exactly the stringly-typed surface S4 exists to
        // prevent, and an agent asked to send "KRW" in a string will guess a
        // format.
        if let Some(unit) = p.annotations.get("ai_unit")
            && !carries_a_quantity(&p.tc)
        {
            out.push(finding(
                "contract/unit-on-non-numeric",
                Severity::Advice,
                format!(
                    "{at} parameter {:?} is annotated ai_unit {:?} but its type is not numeric; a \
                     unit on a non-numeric type either means nothing or means the value is a \
                     number in disguise",
                    p.name,
                    unit.trim()
                ),
                at.clone(),
                Some(
                    "give the parameter a numeric type so the unit describes something the \
                     wire actually carries, or drop the annotation"
                        .into(),
                ),
            ));
        }
    }
    out
}

/// The four `inferred_*` keys that are metadata *about* an inference rather
/// than a value it proposes.
///
/// Taken from `orbweaver-forge` rather than retyped, because `infer::approve`
/// skips exactly these when it decides what to promote and a second list here
/// would be a second answer to a question with one.
fn is_mark(key: &str) -> bool {
    [MARK_STATUS, MARK_EVIDENCE, MARK_SOURCE, MARK_BASIS].contains(&key)
}

/// What `infer::approve` would do to this operation's annotations.
///
/// The consumer is `orbweaver_forge::infer::approve`, and what it does is
/// literal: for every `inferred_<k>` that is not one of the marks it inserts
/// `ai_<k>` with that value. Both rules below are about the key it would land
/// on rather than about the value, because the value is a human's to judge and
/// the key is not.
fn inference_findings(at: &str, sig: &OperationSig) -> Vec<Finding> {
    let mut out = Vec::new();
    for key in sig.annotations.keys() {
        let Some(suffix) = key.strip_prefix(INFERRED_PREFIX) else { continue };
        if is_mark(key) {
            continue;
        }
        let target = format!("ai_{suffix}");

        // An inference landing on a key an author already filled in. `approve`
        // inserts, so the authored value is replaced and `MARK_STATUS` then
        // reads "approved by …" over the top — the one state `Provenance`
        // cannot tell you about afterwards, because the annotation that was
        // overwritten leaves nothing behind.
        if let Some(authored) = sig.annotations.get(&target) {
            out.push(finding(
                "contract/inference-overwrites-authored-annotation",
                Severity::Advice,
                format!(
                    "{at} carries {key:?} beside an authored {target:?} of {:?}; \
                     infer::approve promotes the inferred value into that key, so approving \
                     this operation replaces something a person wrote with something a machine \
                     read off the name, and afterwards nothing records that the authored value \
                     ever existed",
                    authored.trim()
                ),
                at.to_owned(),
                Some(format!(
                    "decide which one is true before anybody runs approve: drop {key:?} if the \
                     authored value stands, or clear {target:?} so the promotion is a promotion \
                     rather than an overwrite"
                )),
            ));
        }

        // An inference landing on a key that is not in the vocabulary. It
        // promotes cleanly and produces a dead annotation: `contract/
        // unknown-annotation` would then fire on the very key this promotion
        // created.
        if !VOCABULARY.contains(&target.as_str()) {
            out.push(finding(
                "contract/inference-promotes-into-nothing",
                Severity::Warning,
                format!(
                    "{at} carries {key:?}, and infer::approve would promote it to {target:?}, \
                     which is not in the SIDL v1 vocabulary; the promotion would manufacture an \
                     annotation no consumer reads, out of a human approval that was given in the \
                     belief it enabled something"
                ),
                at.to_owned(),
                Some(format!(
                    "name the inference after a key that exists — one of {} — or drop it; an \
                     approval is the scarcest thing in this pipeline and it should not be spent \
                     on a key nothing consults",
                    VOCABULARY.join(", ")
                )),
            ));
        }
    }
    out
}

/// Where the quota stage will find more than one budget for one operation.
///
/// The consumer is `orbweaver_mcp::quota::Quota`, and what it does is key a
/// budget on `(caller, target, operation)` where `target` is **the repository id
/// the call was made through** (`guard::Guarded` passes its own `self.id`), not
/// the id the operation was declared under. An inherited operation is callable
/// through every derived interface, so under [`Scope::Interface`] or
/// [`Scope::Operation`] the same operation on the same object has one budget per
/// id a caller can reach it by — and a caller holding both references gets the
/// limit twice.
///
/// That is the quota's own argument about scopes turned one level outward:
/// "a budget an agent escapes by moving to another operation is not a budget on
/// the agent". Moving to another *interface* over the same object is the same
/// escape.
///
/// Reported only for operations the contract itself marks `destructive`, which
/// is deliberate and is what keeps the rule quiet. Inheritance is ordinary and
/// correct; a report that fired on every inherited operation would be a report
/// about IDL rather than about this deployment, and nobody would read the ones
/// that mattered.
///
/// [`Scope::Interface`]: orbweaver_mcp::quota::Scope::Interface
/// [`Scope::Operation`]: orbweaver_mcp::quota::Scope::Operation
fn quota_findings(
    registry: &Registry,
    id: &RepositoryId,
    at: &str,
    name: &str,
    effect: Option<&str>,
) -> Vec<Finding> {
    if !effect.is_some_and(|e| GATED_EFFECTS.contains(&e)) {
        return Vec::new();
    }
    // Interfaces that inherit this one and do not redeclare the operation, so
    // the call they receive is this declaration.
    let mut heirs: Vec<&RepositoryId> = registry
        .ids()
        .filter(|other| other.as_str() != id.as_str())
        .filter(|other| registry.ancestors(other).iter().any(|a| a == id))
        .filter(|other| registry.interface(other).is_some_and(|i| !i.operations.contains_key(name)))
        .collect();
    if heirs.is_empty() {
        return Vec::new();
    }
    heirs.sort();
    vec![finding(
        "contract/inherited-destructive-splits-the-quota",
        Severity::Advice,
        format!(
            "{at} is destructive and is inherited by {} interface(s) that do not redeclare it \
             ({}); the quota stage keys a budget on the repository id a call was made through, so \
             at Scope::Interface or Scope::Operation this one operation has {} budgets and a \
             caller holding a reference under each id may call it {} times the configured limit",
            heirs.len(),
            heirs.iter().map(|h| h.as_str()).collect::<Vec<_>>().join(", "),
            heirs.len() + 1,
            heirs.len() + 1
        ),
        at.to_owned(),
        Some(
            "count this budget at Scope::Caller, which does not subdivide by interface, or expose \
             exactly one of the ids that reach this operation — a budget an agent escapes by \
             narrowing to the base interface is not a budget on the agent"
                .into(),
        ),
    )]
}

/// `ai_*` keys nobody reads.
fn unknown_keys(id: &RepositoryId, what: &str, ann: &BTreeMap<String, String>) -> Vec<Finding> {
    ann.keys()
        .filter(|k| k.starts_with("ai_") && !VOCABULARY.contains(&k.as_str()))
        .map(|k| {
            finding(
                "contract/unknown-annotation",
                Severity::Warning,
                format!(
                    "{what} of {id} carries {k:?}, which is not in the SIDL v1 vocabulary; the \
                     registry keeps it and no consumer reads it, so the annotation is present in \
                     the source and absent from every decision"
                ),
                format!("{id}.{what}"),
                Some(format!(
                    "use one of {}, or add the key to the vocabulary in docs/PLAN.md §2.2 and to \
                     VOCABULARY in orbweaver-test if it is meant to be real",
                    VOCABULARY.join(", ")
                )),
            )
        })
        .collect()
}

/// The mutation verb an operation name contains, if any.
fn mutating_verb(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    MUTATING_VERBS.iter().copied().find(|v| lower.contains(v))
}

/// How an operation hands an object reference to its caller, if it does.
///
/// The search descends into constructed types. `sequence<Expert>` hands out as
/// many bearer addresses as it has elements, and the caller who receives them
/// can dial every one; a rule that only looked at the outermost `TypeCode`
/// would report `Expert delegate()` and stay silent about `ExpertSeq select()`,
/// which is the more generous of the two. This was found by reading our own
/// corpus against the rule rather than by a failing test: `moe::Router::select`
/// went unreported for exactly that reason.
fn escaping_reference(registry: &Registry, sig: &OperationSig) -> Option<String> {
    if let Some(path) = reference_within(registry, &sig.returns, 0) {
        return Some(match path.as_str() {
            "" => "as its return value".into(),
            p => format!("in its return value, at {p}"),
        });
    }
    sig.params.iter().filter(|p| p.direction != ParamDirection::In).find_map(|p| {
        reference_within(registry, &p.tc, 0).map(|path| match path.as_str() {
            "" => format!("through the {:?} parameter", p.name),
            path => format!("through the {:?} parameter, at {path}", p.name),
        })
    })
}

/// Where inside a type a live object reference sits, as a readable path, or
/// `None` if there is none. An empty path means the type *is* a reference.
///
/// `depth` bounds the walk. A recursive type is represented by
/// [`TypeCode::Recursive`] rather than a cycle, so this cannot loop today, but
/// the bound is kept because the rule is about what a caller receives and a
/// reference twelve levels inside a reply is one the reader of this diagnostic
/// could not act on anyway.
fn reference_within(registry: &Registry, tc: &TypeCode, depth: usize) -> Option<String> {
    const MAX_DEPTH: usize = 6;
    if depth > MAX_DEPTH {
        return None;
    }
    let nest = |inner: Option<String>, step: String| -> Option<String> {
        inner.map(|p| if p.is_empty() { step.clone() } else { format!("{step}.{p}") })
    };
    match tc.resolve_alias() {
        tc @ TypeCode::ObjRef { .. } => is_reference(registry, tc).then(String::new),
        TypeCode::Sequence { element, .. } => {
            nest(reference_within(registry, element, depth + 1), "each element".into())
        }
        TypeCode::Array { element, .. } => {
            nest(reference_within(registry, element, depth + 1), "each element".into())
        }
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => members
            .iter()
            .find_map(|m| nest(reference_within(registry, &m.tc, depth + 1), m.name.clone())),
        TypeCode::Union { cases, .. } => cases
            .iter()
            .find_map(|c| nest(reference_within(registry, &c.tc, depth + 1), c.name.clone())),
        _ => None,
    }
}

/// Whether the type is a **live object reference**, not merely something the
/// registry represents as one.
///
/// `TypeCode::ObjRef` is not enough on its own. The registry deliberately
/// registers a `valuetype` and a `native` as an object reference so that
/// `_is_a` and catalogue lookups keep working without implying a wire form v1
/// does not have — and a valuetype is data that marshals by value, not a bearer
/// address. Asking the registry which entry the repository id names separates
/// the two: an interface is an `Entry::Interface`, a valuetype is an
/// `Entry::Type` whose TypeCode happens to be an `ObjRef`.
///
/// This was the crate's second false positive: `gc20::Wallet::balance()`
/// returns the valuetype `Money` and was reported as handing out a capability.
/// Both false positives came from trusting a representation instead of asking
/// what it stood for.
fn is_reference(registry: &Registry, tc: &TypeCode) -> bool {
    match tc.resolve_alias() {
        TypeCode::ObjRef { id, .. } => {
            // `CORBA::Object` is a real reference and is never a registry
            // entry, because nothing declares it.
            id == "IDL:omg.org/CORBA/Object:1.0" || registry.interface(id).is_some()
        }
        _ => false,
    }
}

/// Whether a unit could describe values of this type.
///
/// Sequences and arrays of a quantity count: a series of measurements shares
/// the unit of its elements.
fn carries_a_quantity(tc: &TypeCode) -> bool {
    match tc.resolve_alias() {
        TypeCode::Short
        | TypeCode::UShort
        | TypeCode::Long
        | TypeCode::ULong
        | TypeCode::LongLong
        | TypeCode::ULongLong
        | TypeCode::Float
        | TypeCode::Double
        | TypeCode::LongDouble
        | TypeCode::Octet
        | TypeCode::Fixed { .. } => true,
        TypeCode::Sequence { element, .. } | TypeCode::Array { element, .. } => {
            carries_a_quantity(element)
        }
        // A struct of quantities may well carry a unit for the whole thing
        // (a Position in metres), so it is not evidence of a mistake.
        TypeCode::Struct { .. } | TypeCode::Except { .. } => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use orbweaver_forge::infer;

    /// A typo on a `typedef` was silent, which contradicted this module's own
    /// premise: an `ai_*` key that reaches the registry and is read by nobody
    /// is exactly what it reports. The walk visited interfaces only, so every
    /// annotation on a type — including the place D006 proposed putting a
    /// plane-rule marker — went unchecked.
    #[test]
    fn an_unknown_annotation_on_a_type_is_reported_too() {
        let r = rules(
            "module m {
               //@ ai_handel: true
               typedef sequence<octet> Blob;
             };",
        );
        assert_eq!(r, ["contract/unknown-annotation"], "{r:?}");
    }

    /// And a known key on a type stays silent, or the rule fires on every
    /// correctly annotated typedef in the corpus.
    #[test]
    fn a_known_annotation_on_a_type_is_silent() {
        let r = rules(
            "module m {
               //@ ai_desc: An opaque handle
               typedef sequence<octet> Blob;
             };",
        );
        assert!(r.is_empty(), "{r:?}");
    }

    use super::*;

    fn registry(src: &str) -> Registry {
        let spec = orbweaver_idl::parse(src).expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        r
    }

    fn rules(src: &str) -> Vec<String> {
        contract_findings(&registry(src)).iter().map(|f| f.rule.clone()).collect()
    }

    /// One ingested interface with one operation, annotated exactly as given.
    ///
    /// Built rather than parsed because `inferred_*` values do not come from
    /// IDL: they are attached to an entry that arrived off the wire, which is
    /// also the only entry an inference is allowed to sit on. `define_ingested`
    /// is the single door into the registry for that, so the fixture goes
    /// through it rather than around it.
    fn ingested(annotations: &[(&str, &str)]) -> Registry {
        let sig = OperationSig {
            returns: TypeCode::Void,
            params: Vec::new(),
            raises: Vec::new(),
            oneway: false,
            annotations: annotations
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        };
        let entry = Entry::Interface(orbweaver_registry::InterfaceEntry {
            bases: Vec::new(),
            operations: BTreeMap::from([("settle".to_owned(), sig)]),
            attributes: BTreeMap::new(),
            forward_only: false,
        });
        let mut r = Registry::new();
        r.define_ingested("IDL:remote/Ledger:1.0".to_owned(), entry, "a foreign IR")
            .expect("defines");
        r
    }

    /// A contract that says nothing wrong produces nothing. A checker that
    /// always has an opinion is one nobody reads.
    #[test]
    fn a_well_annotated_contract_is_silent() {
        let findings = contract_findings(&registry(
            "module m {
               interface Ledger {
                 //@ ai_desc: Returns the balance
                 //@ ai_effect: read_only
                 //@ ai_authz: ledger.read
                 long balance(in long account);

                 //@ ai_desc: Moves money
                 //@ ai_effect: destructive
                 //@ ai_idempotent: false
                 //@ ai_authz: ledger.transfer
                 void transfer(
                   //@ ai_pii: high
                   in long from,
                   //@ ai_unit: KRW
                   in long amount);
               };
             };",
        ));
        assert!(findings.is_empty(), "{findings:?}");
    }

    /// The two combinations the module argues are *not* suspicious. Pinned as a
    /// test so a later "obvious improvement" has to argue with the reasoning
    /// rather than silently reintroduce the noise.
    #[test]
    fn out_parameters_and_oneway_idempotence_are_deliberately_not_flagged() {
        let out_params = rules(
            "module m { interface I {
               //@ ai_effect: read_only
               //@ ai_authz: m.read
               void totals(out long hits, inout long cursor);
             }; };",
        );
        assert!(out_params.is_empty(), "an out parameter is a return channel: {out_params:?}");

        let oneway_idem = rules(
            "module m { interface I {
               //@ ai_effect: destructive
               //@ ai_idempotent: true
               //@ ai_authz: m.write
               oneway void publish(in string topic);
             }; };",
        );
        assert!(
            oneway_idem.is_empty(),
            "retry-safety is about the effect, not the reply: {oneway_idem:?}"
        );
    }

    /// Root cause 1 from the corpus batch, codified. `idempotent` exempts an
    /// operation from approval without claiming it only reads, so neither the
    /// name rule nor the oneway rule may fire on it.
    #[test]
    fn idempotent_is_not_a_read_only_claim() {
        let oneway = rules(
            "module m { interface I {
               //@ ai_effect: idempotent
               //@ ai_authz: m.write
               oneway void prefetch(in string id);
             }; };",
        );
        assert!(oneway.is_empty(), "corpus/golden/22 ExpertLoader.prefetch: {oneway:?}");

        let named = rules(
            "module m { interface I {
               //@ ai_effect: idempotent
               //@ ai_authz: m.write
               void delete_key(in string k);
             }; };",
        );
        assert!(named.is_empty(), "deleting twice really can be idempotent: {named:?}");

        // …while the claim that *is* about reading still fires on both.
        assert_eq!(
            rules(
                "module m { interface I {
                   //@ ai_effect: safe
                   //@ ai_authz: m.read
                   oneway void peek();
                 }; };"
            ),
            ["contract/read-only-oneway"]
        );
    }

    /// Root cause 2 from the corpus batch, codified. The registry represents a
    /// valuetype as an `ObjRef` so that lookups keep working; that is a
    /// representation, not a capability.
    #[test]
    fn a_valuetype_return_is_not_a_reference_handout() {
        let r = rules(
            "module m {
               valuetype Money { public long units; };
               interface Wallet {
                 //@ ai_effect: read_only
                 Money balance();
               };
             };",
        );
        assert!(r.is_empty(), "corpus/golden/20 Wallet.balance: {r:?}");

        // And a real interface return still is one.
        assert_eq!(
            rules(
                "module m { interface Target { long ping(); };
                   interface W {
                     //@ ai_effect: read_only
                     Target session();
                   };
                 };"
            ),
            ["contract/reference-escapes-without-authz"]
        );
    }

    /// A plain `Object` is a reference too, and is never a registry entry
    /// because nothing declares it.
    #[test]
    fn a_bare_object_return_counts_as_a_reference() {
        let r = rules(
            "module m { interface I {
               //@ ai_effect: read_only
               Object anything();
             }; };",
        );
        assert_eq!(r, ["contract/reference-escapes-without-authz"], "{r:?}");
    }

    /// A bearer address inside a sequence is still a bearer address, and the
    /// sequence hands out as many as it holds. `moe::Router::select` went
    /// unreported while `moe::Expert::delegate` was flagged, which is backwards:
    /// the unreported one is the more generous of the two.
    #[test]
    fn a_reference_inside_a_constructed_type_still_escapes() {
        let seq = contract_findings(&registry(
            "module m { interface Target { long ping(); };
               typedef sequence<Target> TargetSeq;
               interface W {
                 //@ ai_effect: read_only
                 TargetSeq all();
               };
             };",
        ));
        assert_eq!(
            seq.iter().map(|f| f.rule.as_str()).collect::<Vec<_>>(),
            ["contract/reference-escapes-without-authz"]
        );
        assert!(
            seq[0].message.contains("at each element"),
            "the path must say where the reference sits: {}",
            seq[0].message
        );

        // Through a struct member, and through an out parameter's struct.
        let nested = contract_findings(&registry(
            "module m { interface Target { long ping(); };
               struct Handle { string note; Target typed; };
               interface W {
                 //@ ai_effect: read_only
                 Handle describe(in string name);
               };
             };",
        ));
        assert!(
            nested[0].message.contains("at typed"),
            "the member name is the path: {}",
            nested[0].message
        );
    }

    /// The walk must not resurrect the valuetype false positive one level down:
    /// a sequence of data is data however deeply it is wrapped.
    #[test]
    fn a_sequence_of_valuetypes_is_not_a_sequence_of_capabilities() {
        let r = rules(
            "module m {
               valuetype Money { public long units; };
               typedef sequence<Money> MoneySeq;
               interface Wallet {
                 //@ ai_effect: read_only
                 MoneySeq history();
               };
             };",
        );
        assert!(r.is_empty(), "{r:?}");
    }

    #[test]
    fn a_read_only_claim_contradicted_by_the_name_is_reported() {
        let r = rules(
            "module m { interface I {
               //@ ai_effect: read_only
               //@ ai_authz: m.read
               void delete_account(in long id);
             }; };",
        );
        assert_eq!(r, ["contract/read-only-mutating-name"], "{r:?}");
    }

    #[test]
    fn a_read_only_oneway_returns_its_result_to_nobody() {
        let r = rules(
            "module m { interface I {
               //@ ai_effect: read_only
               //@ ai_authz: m.read
               oneway void peek();
             }; };",
        );
        assert_eq!(r, ["contract/read-only-oneway"], "{r:?}");
    }

    #[test]
    fn an_unconfirmable_and_unretryable_oneway_is_named() {
        let r = rules(
            "module m { interface I {
               //@ ai_effect: destructive
               //@ ai_idempotent: false
               //@ ai_authz: m.write
               oneway void append(in string line);
             }; };",
        );
        assert_eq!(r, ["contract/oneway-not-idempotent"], "{r:?}");
    }

    /// The MCP gate treats an unrecognised effect as needing approval, so a
    /// typo silently gates an operation. That is the consequence the message
    /// has to name.
    #[test]
    fn an_effect_outside_the_vocabulary_names_what_the_gate_will_do() {
        let f = contract_findings(&registry(
            "module m { interface I {
               //@ ai_effect: probably_fine
               //@ ai_authz: m.x
               void go();
             }; };",
        ));
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].rule, "contract/effect-unknown");
        assert!(f[0].message.contains("approval"), "{}", f[0].message);
    }

    /// Pins the mirrored classification against the peer that owns it. If
    /// `orbweaver-mcp`'s policy learns a new ungated value, this fails and the
    /// copy gets updated rather than drifting.
    #[test]
    fn the_effect_vocabulary_matches_the_policy_gate() {
        // The observable contract of policy.rs::destructive_effect: these four
        // values need no approval, "destructive" does, and anything else is
        // treated as needing one.
        assert_eq!(UNGATED_EFFECTS, ["read_only", "readonly", "idempotent", "safe"]);
        assert_eq!(GATED_EFFECTS, ["destructive"]);
        // And the two sets must not overlap, or a value would be both.
        assert!(UNGATED_EFFECTS.iter().all(|u| !GATED_EFFECTS.contains(u)));
    }

    #[test]
    fn a_misspelled_annotation_is_a_warning_because_nothing_reads_it() {
        let f = contract_findings(&registry(
            "module m { interface I {
               //@ ai_descr: does a thing
               //@ ai_effect: read_only
               //@ ai_authz: m.read
               long peek();
             }; };",
        ));
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].rule, "contract/unknown-annotation");
        assert_eq!(f[0].severity, Severity::Warning);
    }

    /// The guard reads ai_authz per operation. A scope on the interface is a
    /// control the author believes in and the code never consults.
    #[test]
    fn an_interface_level_scope_is_reported_as_unenforced() {
        let f = contract_findings(&registry(
            "module m {
               //@ ai_authz: m.admin
               interface I {
                 //@ ai_effect: read_only
                 long peek();
               };
             };",
        ));
        let g = f.iter().find(|f| f.rule == "contract/authz-on-interface-only").expect("reported");
        assert_eq!(g.severity, Severity::Warning);
        assert!(g.message.contains("per operation"), "{}", g.message);
        assert!(g.fix.as_deref().unwrap().contains("repeat"), "{g:?}");
    }

    #[test]
    fn only_one_missing_authz_finding_per_operation() {
        // Destructive, mutating name, hands out a reference, takes high PII —
        // every trigger at once. The report must still say it once.
        let f = contract_findings(&registry(
            "module m { interface Target { long ping(); };
               interface I {
                 //@ ai_effect: destructive
                 Target create_session(
                   //@ ai_pii: high
                   in string user);
               };
             };",
        ));
        let authz: Vec<&Finding> = f.iter().filter(|f| f.rule.contains("authz")).collect();
        assert_eq!(authz.len(), 1, "{authz:?}");
        assert_eq!(authz[0].rule, "contract/gated-without-authz");
    }

    #[test]
    fn an_unscoped_reference_handout_cites_the_bearer_problem() {
        let f = contract_findings(&registry(
            "module m { interface Target { long ping(); };
               interface Registry_ {
                 //@ ai_effect: read_only
                 Target lookup(in string name);
               };
             };",
        ));
        let g = f
            .iter()
            .find(|f| f.rule == "contract/reference-escapes-without-authz")
            .unwrap_or_else(|| panic!("{f:?}"));
        assert!(g.message.contains("bearer address"), "{}", g.message);
    }

    #[test]
    fn a_reference_returned_through_an_out_parameter_counts_too() {
        let r = rules(
            "module m { interface Target { long ping(); };
               interface I {
                 //@ ai_effect: read_only
                 void lookup(in string name, out Target found);
               };
             };",
        );
        assert_eq!(r, ["contract/reference-escapes-without-authz"], "{r:?}");
    }

    #[test]
    fn parameter_vocabulary_is_checked_against_the_parameter_type() {
        let r = rules(
            "module m { interface I {
               //@ ai_effect: read_only
               //@ ai_authz: m.read
               long quote(
                 //@ ai_unit: KRW
                 in string amount,
                 //@ ai_pii: extreme
                 in long who);
             }; };",
        );
        assert!(r.contains(&"contract/unit-on-non-numeric".to_owned()), "{r:?}");
        assert!(r.contains(&"contract/pii-level-unknown".to_owned()), "{r:?}");
    }

    #[test]
    fn a_unit_on_a_sequence_of_numbers_is_fine() {
        let r = rules(
            "module m { typedef sequence<long> Amounts;
               interface I {
                 //@ ai_effect: read_only
                 //@ ai_authz: m.read
                 long total(
                   //@ ai_unit: KRW
                   in Amounts amounts);
               };
             };",
        );
        assert!(r.is_empty(), "{r:?}");
    }

    /// A forward declaration has no body and nothing to say about.
    #[test]
    fn a_forward_declared_interface_is_skipped() {
        assert!(rules("module m { interface I; };").is_empty());
    }

    // ── the two consumers that appeared after the rule for adding a rule ─────

    /// An inference landing on a key an author already filled in. The finding
    /// has to name `approve` and say that it overwrites, because "these two
    /// disagree" without the consequence is a style opinion.
    #[test]
    fn an_inference_that_would_overwrite_an_authored_annotation_is_named() {
        let f = contract_findings(&ingested(&[
            ("ai_effect", "destructive"),
            ("ai_authz", "ledger.settle"),
            ("inferred_effect", "unknown"),
            (infer::MARK_STATUS, infer::UNAPPROVED),
        ]));
        let g = f
            .iter()
            .find(|f| f.rule == "contract/inference-overwrites-authored-annotation")
            .unwrap_or_else(|| panic!("{f:?}"));
        assert_eq!(g.severity, Severity::Advice);
        assert!(g.message.contains("infer::approve"), "{}", g.message);
        assert!(g.message.contains("destructive"), "the authored value: {}", g.message);
    }

    /// The consumer really does overwrite. Pinned against `infer::approve`
    /// itself rather than against a copy of what it is believed to do — the
    /// premise of the rule is a behaviour, and a behaviour can be run.
    #[test]
    fn approve_really_does_replace_the_authored_value() {
        let mut ann = BTreeMap::from([
            ("ai_effect".to_owned(), "destructive".to_owned()),
            ("inferred_effect".to_owned(), "unknown".to_owned()),
            (infer::MARK_STATUS.to_owned(), infer::UNAPPROVED.to_owned()),
        ]);
        infer::approve(
            &mut ann,
            &infer::Approval { by: "an operator".into(), at: "2026-08-14".into() },
        )
        .expect("promotes");
        assert_eq!(ann["ai_effect"], "unknown", "the authored value was replaced");
    }

    /// An inference named after a key the vocabulary does not have. `approve`
    /// promotes it happily and produces an annotation nothing reads — spending
    /// the one human approval in the pipeline on nothing.
    #[test]
    fn an_inference_promoting_into_a_key_nobody_reads_is_a_warning() {
        let f = contract_findings(&ingested(&[
            ("ai_effect", "destructive"),
            ("ai_authz", "ledger.settle"),
            ("inferred_sensitivity", "high"),
            (infer::MARK_STATUS, infer::UNAPPROVED),
        ]));
        let g = f
            .iter()
            .find(|f| f.rule == "contract/inference-promotes-into-nothing")
            .unwrap_or_else(|| panic!("{f:?}"));
        assert_eq!(g.severity, Severity::Warning, "nothing will read the result");
        assert!(g.message.contains("ai_sensitivity"), "{}", g.message);
    }

    /// The marks are not values. A finding on `inferred_status` would fire on
    /// every single inferred operation in the registry, which is exactly the
    /// noise the rule for adding a rule exists to keep out — and the whole
    /// point of `unapproved` being visible is that the state is normal.
    #[test]
    fn the_inference_marks_are_not_mistaken_for_proposals() {
        let f = contract_findings(&ingested(&[
            ("ai_effect", "destructive"),
            ("ai_authz", "ledger.settle"),
            (infer::MARK_STATUS, infer::UNAPPROVED),
            (infer::MARK_EVIDENCE, "the name contains \"settle\""),
            (infer::MARK_SOURCE, "a foreign IR"),
            (infer::MARK_BASIS, infer::BASIS_UNRECOGNISED),
        ]));
        assert!(f.is_empty(), "a mark is metadata, not a proposal: {f:?}");
    }

    /// And an inference on a key the author left empty is the ordinary, correct
    /// state — that is what S3i is *for*. Neither rule may fire on it.
    #[test]
    fn an_ordinary_unapproved_inference_is_silent() {
        let f = contract_findings(&ingested(&[
            ("ai_effect", "destructive"),
            ("ai_authz", "ledger.settle"),
            ("inferred_desc", "Settles a trade"),
            (infer::MARK_STATUS, infer::UNAPPROVED),
        ]));
        // `inferred_desc` promotes into `ai_desc`, which is in the vocabulary
        // and which the author did not write, so there is nothing to say.
        assert!(f.is_empty(), "{f:?}");
    }

    /// The quota rule: a destructive operation reachable under more than one
    /// repository id has more than one budget.
    #[test]
    fn a_destructive_operation_inherited_by_two_interfaces_has_three_budgets() {
        let f = contract_findings(&registry(
            "module m {
               interface Base {
                 //@ ai_effect: destructive
                 //@ ai_authz: m.write
                 void wipe();
               };
               interface Left : Base { };
               interface Right : Base { };
             };",
        ));
        let g = f
            .iter()
            .find(|f| f.rule == "contract/inherited-destructive-splits-the-quota")
            .unwrap_or_else(|| panic!("{f:?}"));
        assert_eq!(g.severity, Severity::Advice);
        assert!(g.message.contains("3 budgets"), "{}", g.message);
        assert!(g.message.contains("IDL:m/Left:1.0"), "the heirs are named: {}", g.message);
        assert!(g.fix.as_deref().unwrap().contains("Scope::Caller"), "{g:?}");
    }

    /// The premise, pinned against the quota itself: two repository ids really
    /// are two budgets. If `Scope::Interface` ever stops keying on the target,
    /// this fails and the rule goes rather than drifting into a folk belief.
    #[test]
    fn the_quota_really_does_key_a_separate_budget_per_repository_id() {
        use orbweaver_mcp::interceptor::{CallContext, Interceptor, Outcome};
        use orbweaver_mcp::policy::Approval;
        use orbweaver_mcp::quota::{Quota, Renewal, Scope};

        let reg = registry(
            "module m {
               interface Base { void wipe(); };
               interface Left : Base { };
             };",
        );
        let mut quota = Quota::new(1, Scope::Interface, Renewal::Never);
        let call = |target: &'static str| CallContext {
            registry: &reg,
            caller: None,
            target,
            operation: "wipe",
            approval: Approval { destructive_approved: true },
            // This premise is about the quota keying on the target, which no
            // argument affects.
            arguments: None,
        };
        assert!(matches!(quota.before(&call("IDL:m/Base:1.0")), Outcome::Proceed));
        assert!(matches!(quota.before(&call("IDL:m/Base:1.0")), Outcome::Refuse(_)), "the limit");
        assert!(
            matches!(quota.before(&call("IDL:m/Left:1.0")), Outcome::Proceed),
            "the same operation on the same object, under the derived id, is a second budget"
        );
        assert_eq!(quota.budgets(), 2);
    }

    /// The rule stays quiet on ordinary inheritance, which is the whole reason
    /// it is restricted to `destructive`. A report that fires on every derived
    /// interface is a report about IDL.
    #[test]
    fn inheritance_on_its_own_is_not_a_quota_finding() {
        let f = rules(
            "module m {
               interface Base {
                 //@ ai_effect: read_only
                 //@ ai_authz: m.read
                 long peek();
               };
               interface Derived : Base { };
             };",
        );
        assert!(f.is_empty(), "{f:?}");

        // And an heir that redeclares the operation is answering for itself.
        let redeclared = rules(
            "module m {
               interface Base {
                 //@ ai_effect: destructive
                 //@ ai_authz: m.write
                 void wipe();
               };
               interface Derived : Base {
                 //@ ai_effect: destructive
                 //@ ai_authz: m.write
                 void wipe();
               };
             };",
        );
        assert!(
            !redeclared.contains(&"contract/inherited-destructive-splits-the-quota".to_owned()),
            "{redeclared:?}"
        );
    }

    /// Determinism, because a report that reorders itself cannot be diffed.
    #[test]
    fn the_report_is_stable_across_runs() {
        let r = registry(
            "module m { interface I {
               //@ ai_effect: read_only
               oneway void delete_it();
               //@ ai_bogus: x
               void other();
             }; };",
        );
        assert_eq!(contract_findings(&r), contract_findings(&r));
    }
}
