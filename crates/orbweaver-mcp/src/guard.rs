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
//! A refusal surfaces as a CORBA system exception — what a native guard would
//! raise, so a stub's caller handles policy the way it already handles the
//! target's own refusals. The *why* goes to the audit log, where §4.8 wants it.
//! Which exception is [`refusal_id`]'s single decision: `NO_PERMISSION` for
//! everything the policy decides, `TRANSIENT` for a [`crate::quota`] budget
//! that a later window may renew — because "you may not" and "not right now"
//! are different answers and a retry loop reads the difference off the
//! repository id, not off the audit log.
//!
//! Since F4 the checks themselves live in [`crate::interceptor`]: this file
//! holds the `Invoker` surface and the `NO_PERMISSION` translation, and the
//! four things it used to do inline are §4.5's stack, in order, extensible by a
//! deployment. What `check` does now is build a [`CallContext`] and run the
//! chain.
//!
//! Since D010 A3 the context carries the **payload** too. A stub hands its
//! arguments over as a closure that writes bytes; the guard runs that closure
//! into a buffer of its own, reads the bytes back through the contract, and
//! hands the chain the same AnyJSON document the dynamic path hands it — so a
//! stage at the content seat sees a static call as it sees a dynamic one, and
//! a stub compiled before this existed is covered by it. Two rules come with
//! that and `Guarded::view` states both: nothing is sent before the chain has
//! answered (the buffer is local, and the stub already probes into one of its
//! own), and a payload the guard cannot read against the contract is refused
//! — `MARSHAL`, or `BAD_OPERATION` for a name the contract lacks — after the
//! gate and before the wire, rather than forwarded past a seat that saw
//! nothing.

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_dynamic::anyjson::{self, LocalReferences};
use orbweaver_dynamic::json::Json;
use orbweaver_giop::server::{BAD_OPERATION, MARSHAL};
use orbweaver_giop::{Connection, Error as GiopError, Invoker, Reply};
use orbweaver_registry::Registry;

use crate::identity::Caller;
use crate::interceptor::{CallContext, Chain};
use crate::policy::{Approval, Denied, Exposure};
use crate::promote::CallStats;

/// The repository id `NO_PERMISSION` travels under.
pub const NO_PERMISSION: &str = "IDL:omg.org/CORBA/NO_PERMISSION:1.0";

/// The repository id `TRANSIENT` travels under: **refused now**.
///
/// The one CORBA refusal that invites a retry, which is exactly why a
/// consumption limit ([`crate::quota`]) must not borrow `NO_PERMISSION`'s
/// spelling. `orbweaver-gen`'s servant runtime already draws the same
/// distinction for a servant's own refusals, so a stub's caller handles a
/// bridge-side budget with the code it already has for a target-side one.
pub const TRANSIENT: &str = "IDL:omg.org/CORBA/TRANSIENT:1.0";

/// `<nobody>`: the caller field of a record about a session nobody is signed
/// into.
///
/// One spelling, so that an audit line, a quota's budget key and an operator's
/// grep pattern all mean the same thing by it.
pub const NOBODY: &str = "<nobody>";

/// Which system exception a refusal reaches a caller as.
///
/// The **one** place the mapping lives, so that the stub's exception, the
/// trace's `outcome` and the dry run's classification cannot come to different
/// conclusions about whether waiting would help. Everything a policy decides is
/// `NO_PERMISSION`; a spent budget that renews is `TRANSIENT`.
pub fn refusal_id(why: &Denied) -> &'static str {
    if why.is_transient() { TRANSIENT } else { NO_PERMISSION }
}

/// The decision field of an audit line for a call the policy allowed.
pub const DECISION_ALLOW: &str = "ALLOW";
/// The decision field of an audit line for a call the policy refused.
pub const DECISION_REFUSE: &str = "REFUSE";
/// The decision field for a **dry run** the policy would have allowed. No call
/// was made; see [`crate::dryrun`].
pub const DECISION_DRY_RUN_ALLOW: &str = "DRYRUN-ALLOW";
/// The decision field for a **dry run** the policy would have refused. No call
/// was made; see [`crate::dryrun`].
pub const DECISION_DRY_RUN_REFUSE: &str = "DRYRUN-REFUSE";

/// The first field of the **elision marker**: not a decision, but the ledger
/// saying that decisions are missing from it.
///
/// A bounded ledger that drops its oldest lines silently is not an audit
/// ledger — the one thing a reader must be able to do is tell a quiet period
/// apart from a hole. [`crate::interceptor::AuditInterceptor`] therefore spends
/// one of its slots on a line that says how many lines are gone, in the first
/// position where they used to be, and every reader of the log meets it:
/// an operator greps `ELIDED`, [`crate::promote::verify_promotion`] refuses it
/// by name rather than judging a promotion from a gap, and
/// [`crate::interceptor::Chain::audit_dropped`] answers the same number without
/// parsing anything.
///
/// It is deliberately **not** in [`audit_entry`]'s shape: it carries no
/// `caller=` and no `operation=`, so a parser written for decision lines
/// rejects it rather than reading it as one.
pub const DECISION_ELIDED: &str = "ELIDED";

/// The elision marker, in the one format: `ELIDED dropped=<n> why=…`.
///
/// `capacity` is named in the prose because the first question a reader has
/// after "how many are gone" is "gone why", and the answer is a ceiling
/// somebody configured rather than a failure.
pub(crate) fn elided_entry(dropped: u64, capacity: usize) -> String {
    format!(
        "{DECISION_ELIDED} dropped={dropped} why=the audit ledger keeps the newest {capacity} \
         lines and the {dropped} oldest have been dropped"
    )
}

/// How many lines an elision marker says are missing, or `None` for a line that
/// is not one.
///
/// The reader's half of [`elided_entry`], public because the marker is meant to
/// be read outside this crate — a console, a harness, an operator's script —
/// and a format with only a writer is a format everybody else reverse-engineers.
pub fn elided_count(line: &str) -> Option<u64> {
    let mut fields = line.split_whitespace();
    if fields.next()? != DECISION_ELIDED {
        return None;
    }
    fields.find_map(|f| f.strip_prefix("dropped=")).and_then(|n| n.parse().ok())
}

/// Whether an audit line's decision field describes a call that never
/// happened.
///
/// The one place the distinction is decided, so that every reader of the log
/// draws it the same way. [`crate::promote::verify_promotion`] uses it to
/// refuse a hypothetical line outright: a promotion gated on a call nobody
/// made would be comparing a prediction against a measurement.
pub fn is_hypothetical(decision: &str) -> bool {
    decision == DECISION_DRY_RUN_ALLOW || decision == DECISION_DRY_RUN_REFUSE
}

/// One audit line, in **the** format:
/// `ALLOW caller=<principal> target=<id> operation=<op>`, with
/// ` why=<reason>` appended on a `REFUSE`. An absent caller is `<nobody>`.
///
/// `decision` is one of [`DECISION_ALLOW`], [`DECISION_REFUSE`] or the dry-run
/// pair — the field that distinguishes a call from a question, in a format
/// that is otherwise identical for both.
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
/// `why` is the one field that is prose rather than an identifier, which is why
/// it comes from [`audit_reason`] and not from a [`Denied`]'s `Display`.
pub(crate) fn audit_entry(
    decision: &str,
    caller: Option<&Caller>,
    target: &str,
    operation: &str,
    why: Option<AuditReason<'_>>,
) -> String {
    let caller = caller.map_or(NOBODY, |c| c.principal.as_str());
    let mut line = format!("{decision} caller={caller} target={target} operation={operation}");
    if let Some(why) = why {
        line.push_str(" why=");
        line.push_str(&why.0);
    }
    line
}

/// Prose that has been judged fit for the ledger.
///
/// A newtype rather than a `&str` because the rule below is not one a reviewer
/// should have to remember at each new call site, and because the grep that
/// was going to enforce it **caught its own explanatory comment and missed a
/// real violation** — a check that is green and measures nothing. There are
/// exactly two constructors: [`audit_reason`], which decides what a refusal
/// may say, and [`AuditReason::already_rendered`], for prose that never came
/// from a [`Denied`] at all. A `Denied` cannot reach this type by `Display`,
/// so it cannot reach the ledger, and the compiler is what says so.
///
/// 문자열 대신 타입인 이유는, 이 규칙을 새 호출 지점마다 사람이 기억해서는
/// 안 되기 때문이다 — 그리고 이를 지키려던 grep이 **자기 설명 주석을 잡고 진짜
/// 위반은 놓쳤기** 때문이다. 초록이면서 아무것도 재지 않는 검사였다.
#[derive(Debug, Clone)]
pub(crate) struct AuditReason<'a>(std::borrow::Cow<'a, str>);

impl<'a> AuditReason<'a> {
    /// For prose that is already a rendered message and never held a payload —
    /// a [`crate::ToolError`], which the caller's own request produced.
    pub(crate) fn already_rendered(text: &'a str) -> Self {
        AuditReason(std::borrow::Cow::Borrowed(text))
    }
}

/// A refusal as the **ledger** states it, which is not always as the caller
/// hears it.
///
/// Every refusal this crate types renders here exactly as it does anywhere
/// else: its fields are the contract and the request — repository ids,
/// operation names, scope names, a budget's arithmetic — and none of them can
/// hold a byte the agent sent. [`Denied::Intercepted`] is the exception, and it
/// is the reason this function exists rather than a call to `to_string`.
///
/// `Intercepted::reason` is free prose written by a stage a deployment
/// installed. That was inert while no stage could see an argument value; it
/// stopped being inert when [`crate::interceptor::CallContext`] grew an
/// `arguments` field, because the seat that field exists for —
/// [`crate::interceptor::SEAT_SAFETY_CONTENT`] — is precisely the stage that
/// holds the values, and a content filter's most natural sentence is *"`cents`
/// looked like a credential: `pin-…`"*. Rendering that into the ledger would
/// put the payload in the one artifact of this crate that is written to disk,
/// retained, and grepped by people who are not the caller. **A gate that has to
/// see a secret must not thereby be a gate that publishes one.**
///
/// So the ledger takes the stage's *name* and drops its prose. Nothing is lost
/// that identifies the decision: the line still says who was refused, on what,
/// for which operation, and by which stage, which is what a reader correlates
/// on. The full sentence still reaches [`Denied`]'s `Display` — the caller's
/// error, the [`crate::dryrun`] report, and the `CallResult::Refused { why }`
/// every observer stage is handed — so the diagnosis goes to whoever already
/// holds the arguments, and only to them.
///
/// 감사 원장은 스테이지의 **이름**만 싣고 산문은 버린다. 내용 필터는 인자 값을
/// 보는 유일한 스테이지이므로, 그 산문을 그대로 기록하면 게이트가 곧 유출
/// 경로가 된다. 전체 문장은 호출자에게만 간다 — 인자를 이미 가진 쪽이다.
pub(crate) fn audit_reason(why: &Denied) -> String {
    match why {
        // The stage, and not one word it wrote.
        Denied::Intercepted { stage, .. } => format!("the {stage} stage refused this call"),
        typed => typed.to_string(),
    }
}

/// [`audit_reason`]'s output, admitted to the ledger. The only path from a
/// [`Denied`] to an audit line.
pub(crate) fn ledger_reason(why: &Denied) -> AuditReason<'static> {
    AuditReason(std::borrow::Cow::Owned(audit_reason(why)))
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
///
/// Two fields are decided here rather than defaulted:
///
/// - the **repository id** comes from [`refusal_id`], so a spent budget arrives
///   as `TRANSIENT` and everything else as `NO_PERMISSION`;
/// - the **completion status** is `COMPLETED_NO`, which is §4.11.4's ordinal
///   **1** — `COMPLETED_YES` is 0 (the transposition `orbweaver-giop` fixed in
///   its own encoder). This gate refuses *before* anything reaches the wire, so
///   the operation provably did not run, and saying so is what makes a retry
///   safe. It used to say 0 here, which told every refused caller its call had
///   completed; with `TRANSIENT` now reachable that would be actively
///   dangerous — an invitation to retry attached to a claim that the call
///   already happened.
fn refusal(why: &Denied) -> GiopError {
    GiopError::SystemException { id: refusal_id(why).to_owned(), minor: 0, completed: 1 }
}

/// A payload the guard could not read back against the contract, refused
/// before the wire: `MARSHAL` when the bytes do not fit the declared
/// parameters, `BAD_OPERATION` when the contract declares no such operation.
/// `COMPLETED_NO` for the same reason [`refusal`] says it: nothing was sent.
fn unreadable(id: &'static str) -> GiopError {
    GiopError::SystemException { id: id.to_owned(), minor: 0, completed: 1 }
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
    ///
    /// Bounded: see [`Chain::audit`]. Once anything has been dropped the first
    /// element is the elision marker ([`elided_count`]), so a reader of this
    /// slice alone can tell a short history from a truncated one.
    pub fn audit(&self) -> &[String] {
        self.chain.audit()
    }

    /// How many audit lines this guard has written since it was assembled,
    /// dropped ones included.
    pub fn audit_written(&self) -> u64 {
        self.chain.audit_written()
    }

    /// How many of them the bounded ledger has dropped.
    pub fn audit_dropped(&self) -> u64 {
        self.chain.audit_dropped()
    }

    /// What the guard's telemetry stage counted.
    ///
    /// These counters are the *static* path's and they live and die with this
    /// `Guarded`. PLAN-MOE **IF2** asks for one store, and
    /// [`crate::promote::CallStats::merge`] is how a deployment gets one —
    /// `bridge.absorb_static(guarded.stats())` before the guard is dropped.
    ///
    /// It is not automatic, and that is a decision rather than an omission:
    /// [`crate::promote::PromotionPolicy::recommend`] answers "which dynamic
    /// path has earned a compiled stub", and a static call is evidence that a
    /// path *already has one*. Merging by default would keep a promoted path
    /// looking hot and have the policy recommend promoting it again. So the two
    /// stores answer two questions, and joining them is the caller's act, taken
    /// when the question is traffic rather than promotion.
    pub fn stats(&self) -> &CallStats {
        self.chain.stats()
    }

    /// The chain this guard runs, for a deployment that adds a stage. The
    /// built-ins are already in it; see [`Chain::insert_after`].
    pub fn chain_mut(&mut self) -> &mut Chain {
        &mut self.chain
    }

    /// What this guard *would* do with `operation`, without invoking it.
    ///
    /// The same chain [`Invoker::invoke`] runs, on a [`CallContext`] built the
    /// same way from the same three fields — and `self.conn` is not read. A
    /// generated stub is compiled code and cannot be asked politely what it
    /// intends; this is how an operator asks the guard instead, before handing
    /// the stub out.
    pub fn dry_run(&mut self, operation: &str) -> crate::dryrun::Prediction {
        let ctx = CallContext {
            registry: self.registry,
            caller: self.caller.as_ref(),
            target: self.id.as_str(),
            operation,
            approval: self.approval,
            arguments: None,
        };
        crate::dryrun::predict(&mut self.chain, &ctx)
    }

    /// [`Guarded::dry_run`] with the values the stub's caller *would* pass,
    /// as the AnyJSON document a content stage would see.
    ///
    /// The static twin of [`crate::Bridge::dry_run_with`]: the chain is
    /// walked with the declared values in the context, and the payload is
    /// mapped and encoded against the contract's `TypeCode`s — both byte
    /// orders, into a buffer that goes nowhere — so the report can say
    /// [`crate::dryrun::Would::Marshal`] for a `string<8>` given nine
    /// characters. `self.conn` is not read; there is no table, so a declared
    /// object reference resolves to nothing and is predicted as such.
    pub fn dry_run_with(&mut self, operation: &str, args: &Json) -> crate::dryrun::Prediction {
        let ctx = CallContext {
            registry: self.registry,
            caller: self.caller.as_ref(),
            target: self.id.as_str(),
            operation,
            approval: self.approval,
            arguments: Some(args),
        };
        crate::dryrun::predict_with(&mut self.chain, &ctx, &LocalReferences::new())
    }

    /// What the stub wrote, read back as the document a content stage sees.
    ///
    /// # How a closure becomes data
    ///
    /// A generated stub hands the wire a closure that *writes* its arguments;
    /// there is no value to pass a stage. So the guard runs the closure once
    /// itself, into a local encoder, and decodes the bytes back through the
    /// contract — [`crate::parameters`], the same list the dynamic path
    /// maps onto — into the same AnyJSON shape the dynamic path hands the
    /// seat: `{"cents": 4242}`. A stage cannot tell which path a payload
    /// came down, which is the property. Every stub compiled before this
    /// existed gains it, because nothing about the stub changed: *which side
    /// of the trust boundary a stub runs on is decided by what it is handed*,
    /// and this is part of what it is handed.
    ///
    /// # This is not sending
    ///
    /// The encoder here is a `Vec` the guard owns and drops. Nothing reaches
    /// `self.conn` until the chain has answered — the same rule the stub
    /// itself already keeps, marshalling into a probe before it calls
    /// `invoke` so a bad argument is a local error and not a half-written
    /// message. So a static call is now encoded **twice before the gate**,
    /// both times into buffers nobody sends, and **zero times to the wire**
    /// before the gate. Stated because D010 A3's second constraint asks for it
    /// to be: refusal still precedes anything sent.
    ///
    /// # A payload the guard cannot read is a payload it does not forward
    ///
    /// The bytes fit the contract or they do not. If they do not — the name
    /// is not declared, the bytes stop short, the bytes run long — the guard
    /// answers `MARSHAL` (or `BAD_OPERATION` for an undeclared name) **after
    /// the chain has run and allowed**, and sends nothing. After, because the
    /// gate answers first for the same reason it always did: a mapping error
    /// answered ahead of a policy refusal is an oracle for the shape of
    /// operations the caller may not call. Sends nothing, because the
    /// alternative is a content stage handed `None` while bytes go out, which
    /// is the blindness this exists to end, reachable by any stub whose
    /// contract has drifted from the guard's catalog. The dynamic path already
    /// refuses an undeclared operation before the wire; this is the same rule
    /// on the other path.
    ///
    /// The wide-character form: the closure writes into a codec-less encoder,
    /// so `wstring` takes the runtime's default (GIOP 1.2, UTF-16) and is
    /// decoded with the same default. The *sent* bytes are still whatever
    /// `self.conn` negotiates — this is a view of the values, not of the
    /// message.
    ///
    /// 게이트가 값을 보려면 스텁이 쓴 바이트를 읽어야 한다. 읽은 것은 로컬
    /// 버퍼이고, 체인이 답하기 전에는 아무것도 보내지 않는다. 읽을 수 없는
    /// 페이로드는 보내지 않는다.
    fn view<F: Fn(&mut Encoder)>(
        &self,
        operation: &str,
        write_args: &F,
    ) -> Result<Json, &'static str> {
        let Some(params) = crate::parameters(self.registry, &self.id, operation) else {
            return Err(BAD_OPERATION);
        };
        let endian = self.conn.endian();
        let mut e = Encoder::new(endian);
        write_args(&mut e);
        let bytes = e.finish().map_err(|_| MARSHAL)?;
        let mut d = Decoder::new(&bytes, endian);
        // Local and per-call: an object reference in the payload becomes a
        // name the stage can see and nobody can dial, and the table it names
        // it in dies with this call.
        let mut refs = LocalReferences::new();
        let mut fields = std::collections::BTreeMap::new();
        for p in params.iter().filter(|p| crate::carried_in(p)) {
            let value = orbweaver_dynamic::decode(&mut d, &p.tc).map_err(|_| MARSHAL)?;
            let json = anyjson::to_json(&p.tc, &value, &mut refs).map_err(|_| MARSHAL)?;
            fields.insert(p.name.clone(), json);
        }
        if d.remaining() != 0 {
            // More than the contract declares: a stub built against another
            // contract, or a hand-written closure. Either way not readable.
            return Err(MARSHAL);
        }
        Ok(Json::Object(fields))
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
        // The payload as data, before the chain and off the wire. `Err` here
        // is not yet an answer: the gate speaks first, and a caller the gate
        // refuses hears the refusal whatever the payload looked like.
        let view = self.view(operation, &write_args);
        // Built by hand rather than by a helper method: the context borrows
        // three fields of `self` while the chain borrows a fourth mutably, and
        // a method returning the context would borrow all of `self`.
        let ctx = CallContext {
            registry: self.registry,
            caller: self.caller.as_ref(),
            target: self.id.as_str(),
            operation,
            approval: self.approval,
            arguments: view.as_ref().ok(),
        };
        if let Err(why) = self.chain.run(&ctx) {
            return Err(refusal(&why));
        }
        let reply = match &view {
            Ok(_) => self.conn.invoke(operation, write_args),
            Err(id) => Err(unreadable(id)),
        };
        // The whole result, not `is_ok()`: a system or user exception names
        // itself in the trace's `outcome` column, and anything else is a
        // failure the chain was genuinely not told the name of. See
        // `crate::interceptor::CallOutcome`.
        self.chain.completed(&ctx, &reply);
        reply
    }

    fn invoke_oneway<F: Fn(&mut Encoder)>(
        &mut self,
        operation: &str,
        write_args: F,
    ) -> Result<(), GiopError> {
        // Gated like a twoway: a oneway that skipped the chain would make
        // "fire and forget" the way around the guard — and read like one, so
        // "fire and forget" is not the way around the content seat either.
        let view = self.view(operation, &write_args);
        let ctx = CallContext {
            registry: self.registry,
            caller: self.caller.as_ref(),
            target: self.id.as_str(),
            operation,
            approval: self.approval,
            arguments: view.as_ref().ok(),
        };
        if let Err(why) = self.chain.run(&ctx) {
            return Err(refusal(&why));
        }
        let sent = match &view {
            Ok(_) => self.conn.invoke_oneway(operation, write_args),
            Err(id) => Err(unreadable(id)),
        };
        self.chain.completed(&ctx, &sent);
        sent
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::interceptor::{Interceptor, Outcome};
    use crate::policy::Denied;

    /// An invoker that records what reached it, for proving what did not.
    struct Recorder {
        reached: Vec<String>,
        /// What the closure wrote when the transport ran it — the bytes the
        /// wire would have carried, for proving the guard forwards the
        /// stub's own closure and not a re-encoding of its reading of it.
        bytes: Vec<Vec<u8>>,
    }

    impl Recorder {
        fn new() -> Self {
            Recorder { reached: Vec::new(), bytes: Vec::new() }
        }
        fn record<F: Fn(&mut Encoder)>(&mut self, operation: &str, write_args: &F) {
            self.reached.push(operation.to_owned());
            let mut e = Encoder::new(Endian::Big);
            write_args(&mut e);
            self.bytes.push(e.finish().unwrap_or_default());
        }
    }

    impl Invoker for Recorder {
        fn endian(&self) -> Endian {
            Endian::Big
        }
        fn invoke<F: Fn(&mut Encoder)>(
            &mut self,
            operation: &str,
            write_args: F,
        ) -> Result<Reply, GiopError> {
            self.record(operation, &write_args);
            // No Reply can be built outside the wire, so the fake fails after
            // recording; the tests only care what got this far.
            Err(GiopError::ConnectionClosed)
        }
        fn invoke_oneway<F: Fn(&mut Encoder)>(
            &mut self,
            operation: &str,
            write_args: F,
        ) -> Result<(), GiopError> {
            self.record(operation, &write_args);
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
                 //@ ai_effect: idempotent
                 void deposit(in long cents);
                 //@ ai_effect: destructive
                 void close();
                 // A string parameter, so a static call can carry the exact
                 // text a content filter exists to catch — the static twin
                 // of the dynamic leak test's `cents` marker.
                 //@ ai_authz: accounts:write
                 //@ ai_effect: idempotent
                 void memo(in string text);
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
            Recorder::new(),
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
        assert!(without.invoke("deposit", |e| e.put_i32(1)).is_err());
        assert!(without.conn.reached.is_empty());
        assert!(without.audit()[0].contains("accounts:write"), "{}", without.audit()[0]);

        let alice = Caller::new("alice").with_scope("accounts:write");
        let mut with = guarded(&reg, exposure, Some(alice), Approval::default());
        let _ = with.invoke("deposit", |e| e.put_i32(1));
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

    /// The two refusals a stub can get, told apart by the only thing a stub's
    /// caller can read: the system exception.
    ///
    /// A retry loop cannot see the audit log and must not have to parse a
    /// reason string. `NO_PERMISSION` means stop; `TRANSIENT` means the budget
    /// may renew — and both say `COMPLETED_NO`, because this gate refuses
    /// before anything reaches the wire and a retry is therefore safe.
    #[test]
    fn a_spent_budget_refuses_a_stub_with_transient_and_a_policy_refuses_with_no_permission() {
        use crate::quota::{Quota, Renewal, Scope};

        let reg = registry();
        let exposure = Exposure::nothing().allow_operation("IDL:bank/Account:1.0", "balance");
        let mut g = guarded(&reg, exposure, Some(Caller::new("alice")), Approval::default());
        let quota = Quota::new(1, Scope::Caller, Renewal::Window);
        assert!(g.chain_mut().quota(quota.clone()));

        let _ = g.invoke("balance", |_| {});
        let spent = g.invoke("balance", |_| {}).unwrap_err();
        assert!(
            matches!(&spent, GiopError::SystemException { id, completed, .. }
                if id == TRANSIENT && *completed == 1),
            "a budget that renews invites a retry: {spent}"
        );

        // The same guard, a refusal the policy made: unchanged.
        let denied = g.invoke("close", |_| {}).unwrap_err();
        assert!(
            matches!(&denied, GiopError::SystemException { id, completed, .. }
                if id == NO_PERMISSION && *completed == 1),
            "{denied}"
        );
        assert_eq!(g.conn.reached, vec!["balance"], "neither refusal reached the wire");

        // A new window, and the stub is served again — without the guard being
        // rebuilt or the exposure being touched.
        assert!(quota.open_window(crate::quota::Window::new("the next hour")));
        let _ = g.invoke("balance", |_| {});
        assert_eq!(g.conn.reached, vec!["balance", "balance"]);
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

    /// A stage at the content seat that keeps what it was handed and lets
    /// the call through — for asking what the seat sees, not what it does.
    struct Sees(Rc<RefCell<Vec<Option<String>>>>);

    impl Interceptor for Sees {
        fn before(&mut self, ctx: &CallContext<'_>) -> Outcome {
            self.0.borrow_mut().push(ctx.arguments.map(ToString::to_string));
            Outcome::Proceed
        }
    }

    type Seen = Rc<RefCell<Vec<Option<String>>>>;

    fn seeing<'r>(reg: &'r Registry, exposure: Exposure) -> (Guarded<'r, Recorder>, Seen) {
        let seen: Seen = Rc::new(RefCell::new(Vec::new()));
        let alice = Caller::new("alice").with_scope("accounts:write");
        let mut g = guarded(reg, exposure, Some(alice), Approval::default());
        assert!(g.chain_mut().insert_after(
            crate::interceptor::STAGE_APPROVAL,
            crate::interceptor::SEAT_SAFETY_CONTENT,
            Sees(Rc::clone(&seen))
        ));
        (g, seen)
    }

    /// **The static path's payload reaches the content seat, in the dynamic
    /// path's shape.** A stub-shaped closure writes a `long`; the stage sees
    /// `{"cents":4242}` — the AnyJSON document a dynamic call for the same
    /// operation carries — and the transport then receives the stub's own
    /// closure, byte for byte, not a re-encoding of the guard's reading.
    /// Twoway and oneway alike, and an argument-less operation as `{}`.
    #[test]
    fn a_static_calls_payload_reaches_the_content_seat_as_the_dynamic_paths_document() {
        let reg = registry();
        let (mut g, seen) =
            seeing(&reg, Exposure::nothing().allow_interface("IDL:bank/Account:1.0"));

        let _ = g.invoke("deposit", |e| e.put_i32(4242));
        let _ = g.invoke_oneway("memo", |e| e.put_str("hello"));
        let _ = g.invoke("balance", |_| {});

        assert_eq!(
            *seen.borrow(),
            vec![
                Some(r#"{"cents":4242}"#.to_owned()),
                Some(r#"{"text":"hello"}"#.to_owned()),
                Some("{}".to_owned()),
            ],
            "the seat sees a static call as it sees a dynamic one"
        );
        // And what went to the transport is what the stub wrote.
        assert_eq!(g.conn.reached, vec!["deposit", "memo", "balance"]);
        assert_eq!(g.conn.bytes[0], 4242i32.to_be_bytes().to_vec());
        let mut e = Encoder::new(Endian::Big);
        e.put_str("hello");
        assert_eq!(g.conn.bytes[1], e.finish().expect("encodes"));
        assert!(g.conn.bytes[2].is_empty());
    }

    /// A payload the guard cannot read is a payload it does not forward —
    /// and the gate still answers first.
    ///
    /// Three unreadable payloads on an allowed operation: bytes that stop
    /// short of the contract, bytes that run past it, and a name the contract
    /// does not declare. Each is refused as a system exception with
    /// `COMPLETED_NO`, none reaches the transport, and each was audited as
    /// the `ALLOW` it was — the gate had no objection; the guard did. Then
    /// the same short payload on an operation the caller may *not* call: the
    /// answer is the policy's `NO_PERMISSION`, so a caller cannot learn an
    /// operation's shape from the difference.
    #[test]
    fn an_unreadable_payload_is_refused_after_the_gate_and_reaches_no_wire() {
        let reg = registry();
        // The undeclared name has no `ai_effect` to read, so without an
        // assumption the *policy* refuses it first — correctly, and as
        // `NO_PERMISSION`. The assumption is what lets the guard's own
        // answer be observed.
        let exposure = Exposure::nothing()
            .allow_interface("IDL:bank/Account:1.0")
            .assuming_unannotated(crate::policy::Unannotated::Assume("read_only".into()));
        let (mut g, seen) = seeing(&reg, exposure);

        let short = g.invoke("deposit", |_| {}).unwrap_err();
        let long = g
            .invoke("deposit", |e| {
                e.put_i32(1);
                e.put_i32(2);
            })
            .unwrap_err();
        let undeclared = g.invoke("no_such_op", |_| {}).unwrap_err();
        for (what, err, id) in [
            ("short", &short, MARSHAL),
            ("long", &long, MARSHAL),
            ("undeclared", &undeclared, BAD_OPERATION),
        ] {
            assert!(
                matches!(err, GiopError::SystemException { id: got, completed: 1, .. } if got == id),
                "{what}: {err}"
            );
        }
        assert!(g.conn.reached.is_empty(), "the transport saw: {:?}", g.conn.reached);
        // The seat was reached each time and handed nothing — which is
        // harmless only because nothing was sent.
        assert_eq!(*seen.borrow(), vec![None, None, None]);
        assert_eq!(g.audit().len(), 3);
        assert!(g.audit().iter().all(|l| l.starts_with("ALLOW caller=alice")), "{:?}", g.audit());

        // Policy first: the same short payload where the policy says no.
        let mut bob = guarded(
            &reg,
            Exposure::nothing().allow_interface("IDL:bank/Account:1.0"),
            Some(Caller::new("bob")),
            Approval::default(),
        );
        let err = bob.invoke("deposit", |_| {}).unwrap_err();
        assert!(
            matches!(&err, GiopError::SystemException { id, .. } if id == NO_PERMISSION),
            "a caller the gate refuses hears the refusal, not the shape: {err}"
        );
    }

    /// The dry run's static twin, with values: a `string<8>` given nine
    /// characters predicts `marshal` where the argument-less question
    /// predicts `allow` — and the invoker is never touched, because a
    /// prediction is synthesised and not a call with the wire unplugged.
    /// The `TypeCode`s are the bounds fixture's, `corpus/golden/27-bounds.idl`,
    /// through an attribute setter, so the accessor arm of
    /// [`crate::parameters`] is what is exercised.
    #[test]
    fn a_static_dry_run_with_values_predicts_marshalling_and_touches_no_wire() {
        use crate::dryrun::{Marshalling, Would};
        use crate::policy::Unannotated;

        struct Detonator;
        impl Invoker for Detonator {
            fn endian(&self) -> Endian {
                Endian::Little
            }
            fn invoke<F: Fn(&mut Encoder)>(&mut self, op: &str, _: F) -> Result<Reply, GiopError> {
                panic!("a dry run reached the invoker: {op}");
            }
            fn invoke_oneway<F: Fn(&mut Encoder)>(
                &mut self,
                op: &str,
                _: F,
            ) -> Result<(), GiopError> {
                panic!("a dry run reached the invoker: {op}");
            }
        }

        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../corpus/golden/27-bounds.idl"
        ))
        .expect("the bounds fixture");
        let spec = orbweaver_idl::parse(&src).expect("parses");
        let mut reg = Registry::new();
        reg.load(&spec).expect("loads");
        const LEDGER: &str = "IDL:gc27/Ledger:1.0";

        // The fixture is unannotated, so the exposure has to say what a
        // silence means before the built-in stack will allow anything.
        let exposure = Exposure::nothing()
            .allow_interface(LEDGER)
            .assuming_unannotated(Unannotated::Assume("read_only".into()));
        let mut g: Guarded<'_, Detonator> = Guarded::assemble(
            Detonator,
            &reg,
            exposure,
            Some(Caller::new("alice")),
            LEDGER.to_owned(),
            Approval::default(),
        );

        let nine = Json::parse(r#"{"value":"123456789"}"#).expect("json");
        let eight = Json::parse(r#"{"value":"12345678"}"#).expect("json");

        let before = g.dry_run("_set_title");
        assert_eq!(before.would(), Would::Allow, "{}", before.to_json());
        assert_eq!(before.marshalling(), None, "no values, no payload verdict");

        let over = g.dry_run_with("_set_title", &nine);
        assert_eq!(over.would(), Would::Marshal, "{}", over.to_json());
        assert!(
            matches!(over.marshalling(), Some(Marshalling::WouldNot(why)) if why.contains("bounded at 8")),
            "{}",
            over.to_json()
        );
        assert!(over.verdict().is_ok(), "the gate allowed; the payload did not fit");
        assert!(over.to_json().to_string().contains(MARSHAL), "{}", over.to_json());

        let within = g.dry_run_with("_set_title", &eight);
        assert_eq!(within.would(), Would::Allow, "{}", within.to_json());
        assert_eq!(within.marshalling(), Some(&Marshalling::Marshals));
        // Every prediction was audited as the question it was.
        assert!(g.audit().iter().all(|l| l.starts_with("DRYRUN-")), "{:?}", g.audit());
    }

    /// A content filter that **tries** to write down what it saw — the same
    /// stage `interceptor::an_argument_a_content_stage_saw_cannot_reach_the_ledger`
    /// installs on the dynamic path, installed here on the static one. It
    /// also keeps what it saw, so the test can ask the seat rather than infer.
    struct WouldLeak(Rc<RefCell<Option<String>>>);

    impl Interceptor for WouldLeak {
        fn before(&mut self, ctx: &CallContext<'_>) -> Outcome {
            let seen = ctx.arguments.map_or_else(|| "<none>".to_owned(), ToString::to_string);
            *self.0.borrow_mut() = Some(seen.clone());
            Outcome::Refuse(Denied::Intercepted {
                stage: crate::interceptor::SEAT_SAFETY_CONTENT.to_owned(),
                reason: format!("this looked like a credential: {seen}"),
            })
        }
    }

    /// **The leak test, on the static path** — D010 A3's first constraint.
    ///
    /// The dynamic path's test drives a real session whose argument carries a
    /// marker and proves the marker reaches the content stage and reaches
    /// neither the ledger nor the trace. This is the same test over a
    /// generated-stub-shaped call: a closure that writes the marker as the
    /// `string` argument of `memo`, through a [`Guarded`] with the same
    /// stage at the same seat and a trace attached.
    ///
    /// The three positive controls are the same three, and the first is the
    /// one that was red before the guard could see a static call's payload:
    /// the stage must have seen the marker. A stage handed `arguments: None`
    /// refuses with `<none>`, leaks nothing, and *proves* nothing — the seat
    /// was blind, not safe.
    #[test]
    fn an_argument_a_content_stage_saw_on_the_static_path_cannot_reach_the_ledger() {
        use crate::interceptor::{SEAT_SAFETY_CONTENT, STAGE_APPROVAL};
        use crate::telemetry::{CallPath, SpanRecord, TelemetrySink, Timestamp, Trace};

        struct Captured(Rc<RefCell<Vec<String>>>);
        impl TelemetrySink for Captured {
            fn emit(&mut self, record: &SpanRecord<'_>) {
                self.0.borrow_mut().push(record.to_line());
            }
        }

        const MARKER: &str = "pin-s3cret-4242";

        let reg = registry();
        let lines = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::new(RefCell::new(None));
        let exposure = Exposure::nothing().allow_operation("IDL:bank/Account:1.0", "memo");
        let alice = Caller::new("alice").with_scope("accounts:write");
        let mut g = guarded(&reg, exposure, Some(alice), Approval::default());
        assert!(g.chain_mut().trace(Trace::new(
            "s-static-ledger",
            CallPath::Static,
            Timestamp::new("2026-08-19T09:00:00Z"),
            Captured(Rc::clone(&lines)),
        )));
        assert!(g.chain_mut().insert_after(
            STAGE_APPROVAL,
            SEAT_SAFETY_CONTENT,
            WouldLeak(Rc::clone(&seen))
        ));

        // The call a generated stub makes: the argument is *written*, as
        // bytes, by a closure. Nothing hands the guard a value.
        let refused = g.invoke("memo", |e| e.put_str(MARKER)).unwrap_err();

        // The stub's caller hears a system exception and not the sentence:
        // that is I1's contract, and the sentence goes to the observers.
        assert!(
            matches!(&refused, GiopError::SystemException { id, .. } if id == NO_PERMISSION),
            "{refused}"
        );
        assert!(g.conn.reached.is_empty(), "the transport saw: {:?}", g.conn.reached);

        // The stage did see it. This is the assertion that was red while the
        // static path handed the seat `arguments: None`: a stage that saw
        // `<none>` leaked nothing because it had nothing, which is blindness
        // and not safety.
        let seen = seen.borrow().clone().expect("the content stage ran");
        assert!(seen.contains(MARKER), "the stage must have seen the marker; it saw: {seen}");
        assert!(seen.contains("\"text\""), "the value arrives under its parameter name: {seen}");

        let audit = g.audit().join("\n");
        let emitted = lines.borrow().join("\n");
        assert!(!audit.is_empty(), "a ledger that wrote nothing proves nothing");
        assert!(!emitted.is_empty(), "a trace that emitted nothing proves nothing");
        // The decision is on the record and attributed…
        assert!(audit.contains("REFUSE caller=alice"), "{audit}");
        assert!(audit.contains(SEAT_SAFETY_CONTENT), "the ledger must name the stage: {audit}");
        assert!(emitted.contains(SEAT_SAFETY_CONTENT), "the trace must name the stage: {emitted}");
        // …and the payload is not.
        for line in [&audit, &emitted] {
            assert!(!line.contains(MARKER), "an argument value reached a record:\n{line}");
            assert!(!line.contains("looked like a credential"), "stage prose was copied:\n{line}");
        }
    }
}
