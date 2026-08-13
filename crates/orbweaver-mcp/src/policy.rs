//! Default-deny exposure: what an agent is allowed to see and call.
//!
//! `docs/PLAN.md` §4.6 and §9.0: **nothing in the registry is reachable through
//! MCP until it is explicitly allowlisted.** The registry is populated from
//! whatever IDL a deployment has, which in a legacy estate is everything —
//! including the operations that move money and the ones that delete things.
//! A projection that exposes by default exposes those on the day someone adds
//! a file.
//!
//! Deny-by-default is also the only rule that stays correct as the catalog
//! grows. An allowlist gets stale in the safe direction; a denylist gets stale
//! in the other one.
//!
//! # Two gates, not one
//!
//! Being *exposed* and being *callable without a human* are different
//! questions. An operation annotated `ai_effect: destructive` may be visible,
//! describable and still refused unless the caller presents an approval. The
//! annotation comes from SIDL (§2.2), so the person who wrote the contract is
//! the one who decides — not the person wiring up the bridge.

use std::collections::BTreeSet;

use orbweaver_registry::Registry;

use crate::identity::Caller;

/// What a **host** has decided, as distinct from what a caller claims.
///
/// Never built from the agent's own request. A caller that can assert its own
/// approval has no approval gate, so this arrives from the process that
/// authenticated the human — at present the operator who launched the bridge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Approval {
    /// A human has approved this specific call.
    pub destructive_approved: bool,
}

/// Why a call or a description was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denied {
    /// The interface is not allowlisted.
    InterfaceNotExposed(String),
    /// The interface is exposed but this operation is not.
    OperationNotExposed {
        /// Repository id.
        id: String,
        /// Operation name.
        operation: String,
    },
    /// The contract requires an authorization scope the caller does not hold.
    MissingScope {
        /// Repository id.
        id: String,
        /// Operation name.
        operation: String,
        /// What `ai_authz` asks for.
        required: String,
    },
    /// The contract names an authorization requirement and nobody is
    /// authenticated to satisfy it.
    NotAuthenticated {
        /// Repository id.
        id: String,
        /// Operation name.
        operation: String,
        /// What `ai_authz` asks for.
        required: String,
    },
    /// The operation is exposed but marked destructive and unapproved.
    NeedsApproval {
        /// Repository id.
        id: String,
        /// Operation name.
        operation: String,
        /// What the contract says it does, if it says.
        effect: String,
    },
    /// A stage of [`crate::interceptor::Chain`] outside the built-in gates
    /// refused the call — a deployment's rate limiter, quota or safety filter.
    ///
    /// The variant exists so that a stage nobody here wrote refuses in the
    /// same currency as one that is: the same `Denied`, so it reaches the
    /// caller as the same `ToolError` and the audit log as the same line.
    /// A chain whose extensions had to invent their own refusal type would
    /// have two refusal paths, and only one of them audited.
    Intercepted {
        /// Which stage refused, since the audit line's fixed format has
        /// nowhere else to put it.
        stage: String,
        /// What that stage says about it.
        reason: String,
    },
}

impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Denied::InterfaceNotExposed(id) => write!(
                f,
                "{id} is not exposed. Nothing is reachable through this bridge until it is \
                 allowlisted"
            ),
            Denied::OperationNotExposed { id, operation } => {
                write!(f, "{id} is exposed but {operation:?} is not among its allowed operations")
            }
            Denied::MissingScope { id, operation, required } => write!(
                f,
                "{id}.{operation} requires the scope {required:?}, which this caller does not \
                 hold"
            ),
            Denied::NotAuthenticated { id, operation, required } => write!(
                f,
                "{id}.{operation} requires the scope {required:?} and this session has no \
                 authenticated caller, so there is nobody to check it against"
            ),
            Denied::NeedsApproval { id, operation, effect } => write!(
                f,
                "{id}.{operation} is marked {effect} and needs an explicit approval before it \
                 can be called"
            ),
            Denied::Intercepted { stage, reason } => {
                write!(f, "the {stage} stage refused this call: {reason}")
            }
        }
    }
}

impl std::error::Error for Denied {}

/// Which interfaces and operations an agent may reach.
#[derive(Debug, Default, Clone)]
pub struct Exposure {
    /// Keys are repository ids; the value is the set of allowed operations, or
    /// empty for "every operation this interface declares".
    allowed: std::collections::BTreeMap<String, BTreeSet<String>>,
}

impl Exposure {
    /// An exposure that permits nothing. This is the only sensible starting
    /// point, and it is what `Default` gives.
    pub fn nothing() -> Self {
        Self::default()
    }

    /// Allows every operation of an interface.
    ///
    /// Broader than naming operations, and deliberately still explicit: it
    /// covers operations added *later*, so a contract that grows grows the
    /// exposure with it. Name operations individually where that matters.
    pub fn allow_interface(mut self, id: impl Into<String>) -> Self {
        self.allowed.entry(id.into()).or_default();
        self
    }

    /// Allows one operation of an interface.
    pub fn allow_operation(mut self, id: impl Into<String>, operation: impl Into<String>) -> Self {
        self.allowed.entry(id.into()).or_default().insert(operation.into());
        self
    }

    /// Whether an interface may be searched or described.
    pub fn exposes(&self, id: &str) -> bool {
        self.allowed.contains_key(id)
    }

    /// Every exposed repository id.
    pub fn interfaces(&self) -> impl Iterator<Item = &String> {
        self.allowed.keys()
    }

    /// The operations named for `id`. Empty means either "every operation this
    /// interface declares" or "this interface is not exposed" — [`exposes`] is
    /// what separates those, and no decision should be taken from this alone.
    ///
    /// It exists for [`crate::dryrun::survey`], which needs the names an
    /// operator *wrote* and not only the ones the contract declares: an
    /// exposure line for an operation that does not exist allowlists nothing,
    /// and a report that enumerated only the contract would never mention it.
    ///
    /// [`exposes`]: Exposure::exposes
    pub fn allowed_operations(&self, id: &str) -> impl Iterator<Item = &String> {
        self.allowed.get(id).into_iter().flatten()
    }

    /// Whether an operation is within the exposed set, ignoring approval.
    pub fn exposes_operation(&self, id: &str, operation: &str) -> bool {
        match self.allowed.get(id) {
            None => false,
            Some(ops) => ops.is_empty() || ops.contains(operation),
        }
    }

    /// The full check a call must pass.
    ///
    /// Order matters for what the caller learns: an operation on an unexposed
    /// interface reports the interface, never "no such operation", because the
    /// second answer would confirm or deny the existence of operations on
    /// something the caller was not permitted to see.
    ///
    /// This is one composition of the rules; [`crate::interceptor::Chain`] is
    /// the other, stage by stage, and it is what a call actually runs through.
    /// Both call the same primitives — nothing is decided twice — and
    /// `the_chain_and_check_call_answer_alike` pins them to the same verdict
    /// case by case. This one stays because a *question* about a call
    /// (`Bridge::check`) must be answerable without auditing and counting an
    /// invocation that never happened.
    pub fn check_call(
        &self,
        registry: &Registry,
        id: &str,
        operation: &str,
        approval: Approval,
        caller: Option<&Caller>,
    ) -> Result<(), Denied> {
        if !self.exposes(id) {
            return Err(Denied::InterfaceNotExposed(id.to_owned()));
        }
        if !self.exposes_operation(id, operation) {
            return Err(Denied::OperationNotExposed {
                id: id.to_owned(),
                operation: operation.to_owned(),
            });
        }
        // The authorization row of §4.8's table. The requirement is written in
        // the contract by whoever owns the interface, so it is checked before
        // the effect gate — an unauthorised caller should not be told which
        // operations would merely have needed an approval.
        for required in required_scopes(registry, id, operation) {
            match caller {
                None => {
                    return Err(Denied::NotAuthenticated {
                        id: id.to_owned(),
                        operation: operation.to_owned(),
                        required,
                    });
                }
                Some(c) if !c.scopes.contains(&required) => {
                    return Err(Denied::MissingScope {
                        id: id.to_owned(),
                        operation: operation.to_owned(),
                        required,
                    });
                }
                Some(_) => {}
            }
        }
        if let Some(effect) = destructive_effect(registry, id, operation)
            && !approval.destructive_approved
        {
            return Err(Denied::NeedsApproval {
                id: id.to_owned(),
                operation: operation.to_owned(),
                effect,
            });
        }
        Ok(())
    }
}

/// The scopes `ai_authz` asks for, comma-separated in the annotation.
///
/// An operation with no `ai_authz` requires none. That is not a loophole — it
/// is what an unannotated legacy contract looks like, and S4 already reports
/// the absence as advice so it is visible rather than silent.
///
/// [`crate::interceptor::ScopeInterceptor`] reads the requirement through this
/// same function: one implementation of the rule, two compositions of it.
pub(crate) fn required_scopes(registry: &Registry, id: &str, operation: &str) -> Vec<String> {
    let Some((_, sig)) = registry.resolve_operation(id, operation) else { return Vec::new() };
    sig.annotations
        .get("ai_authz")
        .map(|v| v.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

/// The `ai_effect` value, when it is one that needs a human.
///
/// `idempotent` and `read_only` do not. Anything else that is written there is
/// treated as needing approval: a value nobody anticipated is not a reason to
/// let a call through, and the failure direction has to be the safe one.
///
/// [`crate::interceptor::ApprovalInterceptor`] reads it through this same
/// function, for the same reason [`required_scopes`] gives.
pub(crate) fn destructive_effect(registry: &Registry, id: &str, operation: &str) -> Option<String> {
    let (_, sig) = registry.resolve_operation(id, operation)?;
    let effect = sig.annotations.get("ai_effect")?;
    match effect.trim() {
        "read_only" | "readonly" | "idempotent" | "safe" => None,
        other => Some(other.to_owned()),
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

    const IDL: &str = r#"
        module bank {
          interface Account {
            //@ ai_effect: read_only
            long balance();
            //@ ai_effect: destructive
            void close();
            void touch();
          };
          interface Ledger { long total(); };
        };"#;

    #[test]
    fn nothing_is_exposed_by_default() {
        let r = registry(IDL);
        let e = Exposure::nothing();
        assert!(!e.exposes("IDL:bank/Account:1.0"));
        assert_eq!(
            e.check_call(&r, "IDL:bank/Account:1.0", "balance", Approval::default(), None),
            Err(Denied::InterfaceNotExposed("IDL:bank/Account:1.0".into()))
        );
    }

    #[test]
    fn allowlisting_an_interface_covers_its_operations() {
        let r = registry(IDL);
        let e = Exposure::nothing().allow_interface("IDL:bank/Account:1.0");
        assert!(
            e.check_call(&r, "IDL:bank/Account:1.0", "balance", Approval::default(), None).is_ok()
        );
        assert!(
            e.check_call(&r, "IDL:bank/Account:1.0", "touch", Approval::default(), None).is_ok()
        );
        // And still covers nothing else.
        assert!(!e.exposes("IDL:bank/Ledger:1.0"));
    }

    #[test]
    fn naming_operations_excludes_the_ones_not_named() {
        let r = registry(IDL);
        let e = Exposure::nothing().allow_operation("IDL:bank/Account:1.0", "balance");
        assert!(
            e.check_call(&r, "IDL:bank/Account:1.0", "balance", Approval::default(), None).is_ok()
        );
        assert_eq!(
            e.check_call(&r, "IDL:bank/Account:1.0", "touch", Approval::default(), None),
            Err(Denied::OperationNotExposed {
                id: "IDL:bank/Account:1.0".into(),
                operation: "touch".into()
            })
        );
    }

    /// The second gate. Being visible is not being callable.
    #[test]
    fn a_destructive_operation_needs_an_approval_even_when_exposed() {
        let r = registry(IDL);
        let e = Exposure::nothing().allow_interface("IDL:bank/Account:1.0");
        let denied = e.check_call(&r, "IDL:bank/Account:1.0", "close", Approval::default(), None);
        assert!(matches!(denied, Err(Denied::NeedsApproval { .. })), "{denied:?}");
        assert!(
            e.check_call(
                &r,
                "IDL:bank/Account:1.0",
                "close",
                Approval { destructive_approved: true },
                None
            )
            .is_ok()
        );
    }

    /// An `ai_effect` value nobody anticipated must not be read as permission.
    #[test]
    fn an_unrecognised_effect_is_treated_as_needing_approval() {
        let r = registry("module m { interface I { //@ ai_effect: probably_fine\n void f(); }; };");
        let e = Exposure::nothing().allow_interface("IDL:m/I:1.0");
        assert!(matches!(
            e.check_call(&r, "IDL:m/I:1.0", "f", Approval::default(), None),
            Err(Denied::NeedsApproval { .. })
        ));
    }

    /// The refusal must not become an oracle for what exists behind it.
    #[test]
    fn an_unexposed_interface_reveals_nothing_about_its_operations() {
        let r = registry(IDL);
        let e = Exposure::nothing();
        let real = e.check_call(&r, "IDL:bank/Account:1.0", "balance", Approval::default(), None);
        let invented =
            e.check_call(&r, "IDL:bank/Account:1.0", "no_such_op", Approval::default(), None);
        assert_eq!(real, invented, "the two answers must be indistinguishable");
    }

    /// The authorization row of §4.8's table: `ai_authz` in the contract,
    /// scopes on the caller, matched here.
    #[test]
    fn an_ai_authz_scope_is_enforced_against_the_caller() {
        let r = registry(
            "module bank { interface Account { //@ ai_authz: accounts:write\n void close(); }; };",
        );
        let e = Exposure::nothing().allow_interface("IDL:bank/Account:1.0");

        // Nobody signed in: refused, and the message says why.
        let d = e.check_call(&r, "IDL:bank/Account:1.0", "close", Approval::default(), None);
        assert!(matches!(d, Err(Denied::NotAuthenticated { .. })), "{d:?}");

        // Signed in without the scope: refused.
        let alice = Caller::new("alice").with_scope("accounts:read");
        let d =
            e.check_call(&r, "IDL:bank/Account:1.0", "close", Approval::default(), Some(&alice));
        assert!(matches!(d, Err(Denied::MissingScope { .. })), "{d:?}");

        // With the scope: allowed.
        let admin = Caller::new("root").with_scope("accounts:write");
        assert!(
            e.check_call(&r, "IDL:bank/Account:1.0", "close", Approval::default(), Some(&admin))
                .is_ok()
        );
    }

    /// Several scopes, comma-separated, all required.
    #[test]
    fn every_listed_scope_is_required_not_any() {
        let r =
            registry("module m { interface I { //@ ai_authz: a:read, b:write\n void f(); }; };");
        let e = Exposure::nothing().allow_interface("IDL:m/I:1.0");
        let partial = Caller::new("x").with_scope("a:read");
        assert!(matches!(
            e.check_call(&r, "IDL:m/I:1.0", "f", Approval::default(), Some(&partial)),
            Err(Denied::MissingScope { required, .. }) if required == "b:write"
        ));
        let full = Caller::new("x").with_scope("a:read").with_scope("b:write");
        assert!(e.check_call(&r, "IDL:m/I:1.0", "f", Approval::default(), Some(&full)).is_ok());
    }

    /// The scope gate runs before the effect gate: an unauthorised caller is
    /// not told which operations would merely have needed approval.
    #[test]
    fn the_scope_gate_answers_before_the_approval_gate() {
        let r = registry(
            "module m { interface I { //@ ai_authz: admin\n //@ ai_effect: destructive\n void wipe(); }; };",
        );
        let e = Exposure::nothing().allow_interface("IDL:m/I:1.0");
        let d = e.check_call(&r, "IDL:m/I:1.0", "wipe", Approval::default(), None);
        assert!(matches!(d, Err(Denied::NotAuthenticated { .. })), "{d:?}");
    }

    #[test]
    fn an_operation_inherited_from_a_base_is_checked_like_any_other() {
        let r = registry(
            "module m { interface Base { //@ ai_effect: destructive\n void wipe(); }; \
             interface Derived : Base {}; };",
        );
        let e = Exposure::nothing().allow_interface("IDL:m/Derived:1.0");
        assert!(matches!(
            e.check_call(&r, "IDL:m/Derived:1.0", "wipe", Approval::default(), None),
            Err(Denied::NeedsApproval { .. })
        ));
    }
}
