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

/// What a caller has been authorised to do beyond the default.
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
    /// The operation is exposed but marked destructive and unapproved.
    NeedsApproval {
        /// Repository id.
        id: String,
        /// Operation name.
        operation: String,
        /// What the contract says it does, if it says.
        effect: String,
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
            Denied::NeedsApproval { id, operation, effect } => write!(
                f,
                "{id}.{operation} is marked {effect} and needs an explicit approval before it \
                 can be called"
            ),
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
    pub fn check_call(
        &self,
        registry: &Registry,
        id: &str,
        operation: &str,
        approval: Approval,
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

/// The `ai_effect` value, when it is one that needs a human.
///
/// `idempotent` and `read_only` do not. Anything else that is written there is
/// treated as needing approval: a value nobody anticipated is not a reason to
/// let a call through, and the failure direction has to be the safe one.
fn destructive_effect(registry: &Registry, id: &str, operation: &str) -> Option<String> {
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
            e.check_call(&r, "IDL:bank/Account:1.0", "balance", Approval::default()),
            Err(Denied::InterfaceNotExposed("IDL:bank/Account:1.0".into()))
        );
    }

    #[test]
    fn allowlisting_an_interface_covers_its_operations() {
        let r = registry(IDL);
        let e = Exposure::nothing().allow_interface("IDL:bank/Account:1.0");
        assert!(e.check_call(&r, "IDL:bank/Account:1.0", "balance", Approval::default()).is_ok());
        assert!(e.check_call(&r, "IDL:bank/Account:1.0", "touch", Approval::default()).is_ok());
        // And still covers nothing else.
        assert!(!e.exposes("IDL:bank/Ledger:1.0"));
    }

    #[test]
    fn naming_operations_excludes_the_ones_not_named() {
        let r = registry(IDL);
        let e = Exposure::nothing().allow_operation("IDL:bank/Account:1.0", "balance");
        assert!(e.check_call(&r, "IDL:bank/Account:1.0", "balance", Approval::default()).is_ok());
        assert_eq!(
            e.check_call(&r, "IDL:bank/Account:1.0", "touch", Approval::default()),
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
        let denied = e.check_call(&r, "IDL:bank/Account:1.0", "close", Approval::default());
        assert!(matches!(denied, Err(Denied::NeedsApproval { .. })), "{denied:?}");
        assert!(
            e.check_call(
                &r,
                "IDL:bank/Account:1.0",
                "close",
                Approval { destructive_approved: true }
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
            e.check_call(&r, "IDL:m/I:1.0", "f", Approval::default()),
            Err(Denied::NeedsApproval { .. })
        ));
    }

    /// The refusal must not become an oracle for what exists behind it.
    #[test]
    fn an_unexposed_interface_reveals_nothing_about_its_operations() {
        let r = registry(IDL);
        let e = Exposure::nothing();
        let real = e.check_call(&r, "IDL:bank/Account:1.0", "balance", Approval::default());
        let invented = e.check_call(&r, "IDL:bank/Account:1.0", "no_such_op", Approval::default());
        assert_eq!(real, invented, "the two answers must be indistinguishable");
    }

    #[test]
    fn an_operation_inherited_from_a_base_is_checked_like_any_other() {
        let r = registry(
            "module m { interface Base { //@ ai_effect: destructive\n void wipe(); }; \
             interface Derived : Base {}; };",
        );
        let e = Exposure::nothing().allow_interface("IDL:m/Derived:1.0");
        assert!(matches!(
            e.check_call(&r, "IDL:m/Derived:1.0", "wipe", Approval::default()),
            Err(Denied::NeedsApproval { .. })
        ));
    }
}
