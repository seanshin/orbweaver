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
//! # Severity
//!
//! Never [`Severity::Error`]; see the crate documentation. Within that,
//! [`Severity::Warning`] means the contract states something **no consumer
//! reads**, so the author believes a control exists that does not.
//! [`Severity::Advice`] means a consumer will act on it and it looks wrong.

use std::collections::BTreeMap;

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
fn escaping_reference(registry: &Registry, sig: &OperationSig) -> Option<String> {
    if is_reference(registry, &sig.returns) {
        return Some("as its return value".into());
    }
    sig.params
        .iter()
        .find(|p| p.direction != ParamDirection::In && is_reference(registry, &p.tc))
        .map(|p| format!("through the {:?} parameter", p.name))
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
