//! D025 §6 P2: **a refusal an agent receives says what would make the call
//! legitimate.**
//!
//! S4 gives an IDL diagnostic a position and a fix hint, and this project
//! counts diagnostics as a product. A refused *call* got a rule id and a reason
//! and not the one thing an agent needs next — and the guard chain knew, having
//! refused for a stated reason. Nothing here is inferred: every remedy is built
//! from fields the refusal already carried at the moment it was raised.
//!
//! # What is measured
//!
//! Every refusal the interceptor chain can hand an agent, **driven through a
//! real [`Chain`]** rather than constructed by hand, and for each one three
//! assertions:
//!
//! 1. the remedy is present in what the caller is handed — the `Display` string
//!    `rpc::tool_error` puts in front of the agent, not a field a rewritten
//!    reader would have had to ask for;
//! 2. it is **specific**: it names the repository id, the scope, the
//!    annotation, the stage or the budget the gate was actually looking at;
//! 3. it **does not name a route the agent can take alone** — it names an
//!    actor from [`policy::REMEDY_ACTORS`] and contains none of
//!    [`policy::REMEDY_FORBIDDEN`].
//!
//! Assertion 3 is the one that keeps the batch honest, and it runs over every
//! refusal in the table rather than over the ones somebody remembered. The two
//! vocabularies are read from `policy` rather than retyped here, for the reason
//! `CLAUDE.md` gives about classifiers: a check that matches a hand-written
//! substring of a sentence another function owns goes green over the drift.
//!
//! # What is deliberately not measured
//!
//! That a remedy is *useful*. This file can hold a sentence to naming the right
//! nouns and to not offering the agent a way round; whether the operator's act
//! it names is the one that would actually unblock the call is a judgement no
//! assertion makes. Said here rather than left to be assumed.
//!
//! *거절은 다음 단계를 말한다 — 그러나 결코 에이전트가 혼자 갈 수 있는 길은
//! 아니다. 세 번째 단정이 이 배치를 정직하게 지킨다.*

use std::time::{Duration, SystemTime};

use orbweaver_mcp::identity::Caller;
use orbweaver_mcp::interceptor::{
    CallContext, Chain, Interceptor, Outcome, SEAT_SAFETY_CONTENT, STAGE_APPROVAL,
};
use orbweaver_mcp::policy::{
    Approval, Denied, Exposure, REMEDY_ACTORS, REMEDY_FORBIDDEN, Unannotated,
};
use orbweaver_mcp::quota::{Quota, Renewal, Scope};
use orbweaver_mcp::token::{Expiry, Unstamped};
use orbweaver_registry::Registry;

const ACCOUNT: &str = "IDL:bank/Account:1.0";

/// The fixture. `close` names an approver so that the *by whom* half of an
/// approval refusal has something true to say; `touch` says nothing at all,
/// which is what every legacy contract is.
const IDL: &str = "module bank {
    interface Account {
      //@ ai_effect: read_only
      long balance();
      //@ ai_authz: accounts:write
      //@ ai_effect: idempotent
      void deposit(in long cents);
      //@ ai_effect: destructive
      //@ ai_approver: the account's owner
      void close();
      void touch();
    };
  };";

fn registry() -> Registry {
    let spec = orbweaver_idl::parse(IDL).expect("parses");
    let mut r = Registry::new();
    r.load(&spec).expect("loads");
    r
}

fn ctx<'a>(
    reg: &'a Registry,
    caller: Option<&'a Caller>,
    operation: &'a str,
    approval: Approval,
) -> CallContext<'a> {
    CallContext { registry: reg, caller, target: ACCOUNT, operation, approval, arguments: None }
}

/// A deployment's own stage, of the kind [`Denied::Intercepted`] exists for.
/// Its sentence holds an argument value on purpose: the remedy must quote none
/// of it.
struct Screen;

impl Interceptor for Screen {
    fn before(&mut self, _ctx: &CallContext<'_>) -> Outcome {
        Outcome::Refuse(Denied::Intercepted {
            stage: SEAT_SAFETY_CONTENT.to_owned(),
            reason: "`cents` looked like a credential: pin-4417".to_owned(),
        })
    }
}

/// The three assertions, applied to one refusal as the caller receives it.
///
/// `must_name` is the specificity half: the facts the gate held when it
/// refused, which the remedy has to hand back. `may_not_name` is per-refusal
/// leakage — what this particular sentence must not repeat.
#[track_caller]
fn teaches(why: &Denied, must_name: &[&str], may_not_name: &[&str]) {
    let remedy = why.remedy();
    let shown = why.to_string();

    // 1 — it reaches the caller, in the one string every reader of a refusal
    // takes. A remedy only a rewritten reader can see is a remedy most readers
    // do not get.
    assert!(!remedy.trim().is_empty(), "a refusal with no next step: {why:?}");
    assert!(shown.contains(&remedy), "the remedy does not reach the caller: {shown}");

    // 2 — specific: the id, the scope, the annotation, the stage, the budget.
    for fact in must_name {
        assert!(remedy.contains(fact), "the remedy does not name {fact:?}: {remedy}");
    }
    for leak in may_not_name {
        assert!(!remedy.contains(leak), "the remedy repeats {leak:?}: {remedy}");
    }

    // 3 — the act belongs to somebody who is not the caller, and the sentence
    // offers no route the caller could take by itself. **This is the assertion
    // that keeps default-deny default-deny**, and it runs on every row.
    assert!(
        REMEDY_ACTORS.iter().any(|a| remedy.contains(a)),
        "the remedy names no actor from {REMEDY_ACTORS:?}, so it is either vague or addressed to \
         the agent: {remedy}"
    );
    let lower = remedy.to_lowercase();
    for route in REMEDY_FORBIDDEN {
        assert!(
            !lower.contains(route),
            "the remedy offers the agent a route of its own ({route:?}): {remedy}"
        );
    }
}

// ── §4.5 #1, the allowlist half ─────────────────────────────────────────────

/// Exposure is default-deny **by an operator's decision**, so the remedy names
/// the id and the operator and stops. An agent told how to get on an allowlist
/// is the failure this gate exists to prevent.
#[test]
fn an_unexposed_interface_names_the_id_and_an_operators_act() {
    let reg = registry();
    let mut chain = Chain::standard(Exposure::nothing());
    let why = chain.run(&ctx(&reg, None, "balance", Approval::default())).unwrap_err();
    assert!(matches!(why, Denied::InterfaceNotExposed(_)), "{why:?}");
    teaches(&why, &[ACCOUNT, "allowlist", "default-deny"], &[]);
}

/// The interface is exposed and the operation is not, so the remedy names the
/// operation — the one fact that separates this from the row above.
#[test]
fn an_unexposed_operation_names_the_operation_and_its_interface() {
    let reg = registry();
    let mut chain = Chain::standard(Exposure::nothing().allow_operation(ACCOUNT, "balance"));
    let why = chain.run(&ctx(&reg, None, "close", Approval::default())).unwrap_err();
    assert!(matches!(why, Denied::OperationNotExposed { .. }), "{why:?}");
    teaches(&why, &[ACCOUNT, "close"], &[]);
}

// ── §4.5 #1, the authorization half ─────────────────────────────────────────

/// The scope was **in the comparison the stage had just made**. Naming it costs
/// nothing and is the whole of what the caller's owner needs; naming a way to
/// obtain it would be naming a way past the gate, so the sentence hands the
/// grant to the host that issued the credential.
#[test]
fn a_missing_scope_names_the_scope_the_contract_asked_for() {
    let reg = registry();
    let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
    let caller = Caller::new("alice").with_scope("accounts:read");
    let why = chain.run(&ctx(&reg, Some(&caller), "deposit", Approval::default())).unwrap_err();
    assert!(matches!(why, Denied::MissingScope { .. }), "{why:?}");
    teaches(&why, &["accounts:write", ACCOUNT, "deposit"], &[]);
}

/// Nobody is signed in, so there is nothing to check the requirement against.
/// The remedy names the scope too — an unauthenticated caller's owner has the
/// same question as an underprivileged one's.
#[test]
fn an_unauthenticated_call_names_the_scope_and_the_host_that_would_sign_it_in() {
    let reg = registry();
    let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
    let why = chain.run(&ctx(&reg, None, "deposit", Approval::default())).unwrap_err();
    assert!(matches!(why, Denied::NotAuthenticated { .. }), "{why:?}");
    teaches(&why, &["accounts:write", ACCOUNT, "deposit"], &[]);
}

/// A lapsed credential cannot be extended from the caller's side, and saying so
/// is the point: the sentence would otherwise read as an invitation to send the
/// same token again.
#[test]
fn an_expired_credential_names_the_principal_and_the_host() {
    let reg = registry();
    let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
    let expiry = Expiry::new(Unstamped::Refuse);
    assert!(chain.expiry(expiry.clone()));
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    expiry.stamp(now);
    let caller = Caller::new("alice").expiring_at(now - Duration::from_secs(60));
    let why = chain.run(&ctx(&reg, Some(&caller), "balance", Approval::default())).unwrap_err();
    assert!(matches!(why, Denied::CredentialExpired { overdue_secs: Some(_), .. }), "{why:?}");
    teaches(&why, &["alice", "re-authenticate"], &[]);
}

/// The gate **cannot tell**, which is a different fact from *expired*, and its
/// remedy is a different act: the host has to supply an instant. The caller is
/// not the party that can, and the sentence must not suggest it is.
#[test]
fn an_unstamped_expiry_gate_names_the_instant_the_host_owes_it() {
    let reg = registry();
    let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
    assert!(chain.expiry(Expiry::new(Unstamped::Refuse)));
    let caller =
        Caller::new("alice").expiring_at(SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000));
    let why = chain.run(&ctx(&reg, Some(&caller), "balance", Approval::default())).unwrap_err();
    assert!(matches!(why, Denied::CredentialExpired { overdue_secs: None, .. }), "{why:?}");
    teaches(&why, &["alice", "instant"], &[]);
}

// ── §4.5 #3, the safety seat ────────────────────────────────────────────────

/// *What is being waited on, and by whom.* The `by whom` is `ai_approver`,
/// which the gate could already read and only the dry-run report was reading.
/// A caller cannot assert its own approval and the sentence says so.
#[test]
fn a_destructive_operation_names_the_human_and_the_approver_the_contract_named() {
    let reg = registry();
    let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
    let why = chain.run(&ctx(&reg, None, "close", Approval::default())).unwrap_err();
    assert!(matches!(why, Denied::NeedsApproval { assumed: false, .. }), "{why:?}");
    teaches(&why, &[ACCOUNT, "close", "the account's owner", "cannot assert its own"], &[]);
}

/// A contract that says nothing about who approves gets no invented approver.
/// The remedy is the human and the host, and the absence is an absence.
#[test]
fn an_approval_with_no_named_approver_invents_nobody() {
    let reg = registry();
    let exposure = Exposure::nothing()
        .allow_interface(ACCOUNT)
        .assuming_unannotated(Unannotated::Assume("destructive".to_owned()));
    let mut chain = Chain::standard(exposure);
    let why = chain.run(&ctx(&reg, None, "touch", Approval::default())).unwrap_err();
    assert!(matches!(why, Denied::NeedsApproval { assumed: true, .. }), "{why:?}");
    teaches(&why, &[ACCOUNT, "touch", "assumption"], &["names"]);
}

/// The remedy S4 writes for the same condition. It names the **annotation**,
/// because a refusal that said only "no" sends an operator into a permissions
/// config after a problem that is in the contract — and it is now said once,
/// by `remedy`, where it used to be this variant's own second sentence.
#[test]
fn an_unstated_effect_names_the_annotation_that_would_settle_it() {
    let reg = registry();
    let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
    let why = chain.run(&ctx(&reg, None, "touch", Approval::default())).unwrap_err();
    assert!(matches!(why, Denied::EffectUnstated { .. }), "{why:?}");
    teaches(&why, &[ACCOUNT, "touch", "ai_effect: read_only", "ai_effect: destructive"], &[]);
    // Said once. The fact used to live in the `Display` arm as well, and two
    // copies of one sentence is how a sentence goes false in one of them.
    let shown = why.to_string();
    assert_eq!(shown.matches("//@ ai_effect: read_only").count(), 1, "{shown}");
}

// ── the seats a deployment fills ────────────────────────────────────────────

/// A stage nobody here wrote. The remedy names the **stage** and not one word
/// the stage said: that prose is the only part of a refusal this crate did not
/// write, and the seat it comes from is the one that holds argument values.
#[test]
fn an_intercepted_call_names_the_stage_and_quotes_nothing_it_said() {
    let reg = registry();
    let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
    assert!(chain.insert_after(STAGE_APPROVAL, SEAT_SAFETY_CONTENT, Screen));
    let why = chain.run(&ctx(&reg, None, "balance", Approval::default())).unwrap_err();
    assert!(matches!(why, Denied::Intercepted { .. }), "{why:?}");
    teaches(&why, &[SEAT_SAFETY_CONTENT], &["pin-4417", "credential"]);
}

/// The one refusal that is not about permission. Waiting **is** the legitimate
/// path here — the gate bounds a rate, not a permission, which is what
/// `is_transient` says in the currency a stub's caller acts on — so the remedy
/// names the host that opens the next window and says the request itself
/// changes nothing. It still may not tell the agent to retry: that decision
/// belongs to the exception id and not to prose somebody has to grep.
#[test]
fn a_spent_renewing_budget_names_the_budget_and_the_host_that_opens_the_window() {
    let reg = registry();
    let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
    assert!(chain.quota(Quota::new(1, Scope::Caller, Renewal::Window)));
    let caller = Caller::new("alice");
    let call = ctx(&reg, Some(&caller), "balance", Approval::default());
    chain.run(&call).expect("the first is within the budget");
    chain.completed(&call, true);
    let why = chain.run(&call).unwrap_err();
    assert!(matches!(why, Denied::QuotaExhausted { renews: true, .. }), "{why:?}");
    assert!(why.is_transient(), "a renewing budget is a not-right-now: {why:?}");
    teaches(&why, &["caller=alice", "window"], &[]);
}

/// A lifetime budget: no later window will help, so the remedy names an
/// operator raising the limit rather than a wait that would never end.
#[test]
fn a_spent_lifetime_budget_names_an_operator_and_not_a_wait() {
    let reg = registry();
    let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
    assert!(chain.quota(Quota::new(0, Scope::Operation, Renewal::Never)));
    let caller = Caller::new("alice");
    let why = chain.run(&ctx(&reg, Some(&caller), "balance", Approval::default())).unwrap_err();
    assert!(matches!(why, Denied::QuotaExhausted { renews: false, .. }), "{why:?}");
    assert!(!why.is_transient(), "a lifetime budget is final: {why:?}");
    // It may say *window* — "no later window will" is the informative half —
    // but it must not hand this caller the renewing budget's answer, which is
    // to wait for the host to open one.
    teaches(&why, &["caller=alice", "balance", "does not renew"], &["opens"]);
}

// ── the properties that hold across the whole table ─────────────────────────

/// **The message changed and the verdict did not.** A remedy that made a
/// refusal into an allow would be the worst possible outcome of this batch, so
/// it is asserted rather than assumed: every row above is still an `Err`, and
/// the one call that is genuinely permitted still proceeds.
#[test]
fn a_refusal_that_teaches_still_refuses() {
    let reg = registry();
    let mut chain = Chain::standard(Exposure::nothing().allow_interface(ACCOUNT));
    let caller = Caller::new("alice").with_scope("accounts:write");

    // Permitted: annotated read_only, exposed, no scope asked for.
    chain.run(&ctx(&reg, Some(&caller), "balance", Approval::default())).expect("allowed");

    // Each of these was refused before this batch and is refused after it.
    for (operation, approval) in [
        ("close", Approval::default()),
        ("touch", Approval::default()),
        ("nonexistent", Approval::default()),
    ] {
        assert!(
            chain.run(&ctx(&reg, Some(&caller), operation, approval)).is_err(),
            "{operation} stopped being refused"
        );
    }

    // And an approval is still what unlocks a destructive call — the remedy
    // describes the same gate that was always there.
    chain
        .run(&ctx(&reg, Some(&caller), "close", Approval { destructive_approved: true }))
        .expect("a human's yes still opens it");
}

/// The rule, over every variant at once rather than over the ones with a test
/// above: a refusal an agent can receive owes it a next step, that step names
/// somebody who is not the agent, and it offers no route of the agent's own.
///
/// The table is built by hand here **on purpose**. The chain tests above prove
/// each sentence is what a caller actually receives; this one proves the
/// property holds for every shape the type can take, including the field
/// combinations no chain in this file produces.
#[test]
fn every_refusal_the_type_can_take_names_an_actor_and_no_route() {
    let every = [
        Denied::InterfaceNotExposed(ACCOUNT.to_owned()),
        Denied::OperationNotExposed { id: ACCOUNT.into(), operation: "close".into() },
        Denied::MissingScope {
            id: ACCOUNT.into(),
            operation: "deposit".into(),
            required: "accounts:write".into(),
        },
        Denied::NotAuthenticated {
            id: ACCOUNT.into(),
            operation: "deposit".into(),
            required: "accounts:write".into(),
        },
        Denied::CredentialExpired { principal: "alice".into(), overdue_secs: Some(60) },
        Denied::CredentialExpired { principal: "alice".into(), overdue_secs: None },
        Denied::NeedsApproval {
            id: ACCOUNT.into(),
            operation: "close".into(),
            effect: "destructive".into(),
            assumed: false,
            approver: None,
        },
        Denied::NeedsApproval {
            id: ACCOUNT.into(),
            operation: "close".into(),
            effect: "destructive".into(),
            assumed: false,
            approver: Some("the account's owner".into()),
        },
        Denied::NeedsApproval {
            id: ACCOUNT.into(),
            operation: "touch".into(),
            effect: "destructive".into(),
            assumed: true,
            approver: None,
        },
        Denied::EffectUnstated { id: ACCOUNT.into(), operation: "touch".into() },
        Denied::Intercepted { stage: "quota.rate_limit".into(), reason: "pin-4417".into() },
        Denied::QuotaExhausted {
            budget: "caller=alice".into(),
            used: 3,
            limit: 3,
            window: "w1".into(),
            renews: true,
        },
        Denied::QuotaExhausted {
            budget: "caller=alice".into(),
            used: 3,
            limit: 3,
            window: "-".into(),
            renews: false,
        },
    ];
    for why in &every {
        teaches(why, &[], &[]);
    }
}
