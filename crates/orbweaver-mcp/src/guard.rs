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
//! A refusal surfaces as CORBA `NO_PERMISSION` — what a native guard would
//! raise, so a stub's caller handles policy the way it already handles the
//! target's own refusals. The *why* goes to the audit log, where §4.8 wants it.

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_giop::{Connection, Error as GiopError, Invoker, Reply};
use orbweaver_registry::Registry;

use crate::identity::Caller;
use crate::policy::{Approval, Exposure};

/// The repository id `NO_PERMISSION` travels under.
pub const NO_PERMISSION: &str = "IDL:omg.org/CORBA/NO_PERMISSION:1.0";

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
    exposure: Exposure,
    caller: Option<Caller>,
    /// The repository id the handle named. From the capability table, never
    /// from the stub: a stub asserting its own interface id would be asserting
    /// its own permissions.
    id: String,
    approval: Approval,
    audit: Vec<String>,
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
        Self { conn, registry, exposure, caller, id, approval, audit: Vec::new() }
    }

    /// Every decision this guard has made, oldest first.
    ///
    /// Lines name the principal and the operation and never credential
    /// material — the same rule as [`crate::identity::audit_line`], and for the
    /// same reason: there is nothing here a line *could* leak, because the
    /// guard never holds a credential.
    pub fn audit(&self) -> &[String] {
        &self.audit
    }

    fn check(&mut self, operation: &str) -> Result<(), GiopError> {
        let caller = self.caller.as_ref().map_or("<nobody>", |c| c.principal.as_str());
        match self.exposure.check_call(
            self.registry,
            &self.id,
            operation,
            self.approval,
            self.caller.as_ref(),
        ) {
            Ok(()) => {
                self.audit.push(format!(
                    "ALLOW caller={caller} target={} operation={operation}",
                    self.id
                ));
                Ok(())
            }
            Err(denied) => {
                self.audit.push(format!(
                    "REFUSE caller={caller} target={} operation={operation} why={denied}",
                    self.id
                ));
                // The shape a native CORBA guard would answer with; the reason
                // stays in the audit log, which is where §4.8 wants it.
                Err(GiopError::SystemException {
                    id: NO_PERMISSION.to_owned(),
                    minor: 0,
                    completed: 0,
                })
            }
        }
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
        self.check(operation)?;
        self.conn.invoke(operation, write_args)
    }

    fn invoke_oneway<F: Fn(&mut Encoder)>(
        &mut self,
        operation: &str,
        write_args: F,
    ) -> Result<(), GiopError> {
        // Checked like a twoway: a oneway that skipped the gate would make
        // "fire and forget" the way around the guard.
        self.check(operation)?;
        self.conn.invoke_oneway(operation, write_args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
