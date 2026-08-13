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
use crate::policy::{Approval, Exposure};
use crate::promote::CallStats;

/// The repository id `NO_PERMISSION` travels under.
pub const NO_PERMISSION: &str = "IDL:omg.org/CORBA/NO_PERMISSION:1.0";

/// One audit line, in **the** format:
/// `ALLOW caller=<principal> target=<id> operation=<op>`, with
/// ` why=<reason>` appended on a `REFUSE`. An absent caller is `<nobody>`.
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
    let caller = caller.map_or("<nobody>", |c| c.principal.as_str());
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
fn no_permission() -> GiopError {
    GiopError::SystemException { id: NO_PERMISSION.to_owned(), minor: 0, completed: 0 }
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
    pub fn audit(&self) -> &[String] {
        self.chain.audit()
    }

    /// What the guard's telemetry stage counted.
    ///
    /// Honest shortfall: these counters are the *static* path's, they live and
    /// die with this `Guarded`, and nothing merges them back into the session's
    /// (`Bridge::stats`). PLAN-MOE **IF2** asks for one store; one store across
    /// two objects with independent lifetimes needs a shared one, which needs
    /// either interior mutability or a store passed in at assembly. Neither is
    /// here yet, and pretending the numbers add up would be worse than saying
    /// they do not.
    pub fn stats(&self) -> &CallStats {
        self.chain.stats()
    }

    /// The chain this guard runs, for a deployment that adds a stage. The
    /// built-ins are already in it; see [`Chain::insert_after`].
    pub fn chain_mut(&mut self) -> &mut Chain {
        &mut self.chain
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
        if self.chain.run(&ctx).is_err() {
            return Err(no_permission());
        }
        let reply = self.conn.invoke(operation, write_args);
        self.chain.completed(&ctx, reply.is_ok());
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
        if self.chain.run(&ctx).is_err() {
            return Err(no_permission());
        }
        let sent = self.conn.invoke_oneway(operation, write_args);
        self.chain.completed(&ctx, sent.is_ok());
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
