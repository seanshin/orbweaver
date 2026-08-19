//! The promotion engine: PLAN §7.3 stream B, integration point I4.
//!
//! A dynamic call path that runs hot is a candidate for a static stub — that
//! is the point of stream B. This module holds the two halves of the decision:
//! the *recommendation* ([`CallStats`] + [`PromotionPolicy`]), which says
//! which paths have earned a stub, and the *regression gate*
//! ([`verify_promotion`]), which says whether a candidate stub may actually
//! replace the dynamic path it was promoted from.
//!
//! The gate is why this module exists. §7.4 I4: *a promoted static path
//! carries the same `Caller` assertion behaviour as the dynamic path it
//! replaced; "promotion dropped the identity context" is the regression
//! gate's first test.* A promotion that keeps the answer and loses the caller
//! is the §4.8 confused deputy reappearing through an optimization — the
//! target would see the right bytes attributed to the wrong subject, which is
//! R13's exact shape. So the identity comparison runs **before** the result
//! comparison, and losing the caller is its own named error,
//! [`PromotionRegression::IdentityDropped`], even when the results agree
//! byte for byte.
//!
//! # Why the statistics are count-based
//!
//! [`CallStats`] counts calls and failures and holds no latencies and no
//! timestamps, because there is no clock in scope — deliberately. Promotion
//! criteria that depend on wall-clock make the recommendation
//! non-reproducible: the same call history would recommend different
//! candidates on different runs, and reproducibility is what lets the gate be
//! tested. A count-based recommendation over the same history is the same
//! recommendation every time.
//!
//! # What belongs to the integration step, not to this module
//!
//! Honestly stated, per the operating model:
//!
//! - **Wiring into the bridge.** Landed since this note was written:
//!   `Bridge::invoke` records every call into its [`CallStats`] — since F4
//!   through [`crate::interceptor::TelemetryInterceptor`], the §4.5 stage that
//!   owns the counters, so that the chain's telemetry and this module's
//!   promotion statistics are one store and not two (PLAN-MOE **IF2**) — and it
//!   writes its audit lines through the same formatter the guard uses — so
//!   the dynamic line this gate parses can be *captured* from
//!   `Bridge::audit()` rather than reconstructed from session state. The
//!   gen-corpus oracle's I4 section still reconstructs; flipping it to
//!   capture is a one-line change in `orbweaver-gen` owned by a later batch.
//! - **The live static-vs-dynamic comparison.** [`verify_promotion`] compares
//!   two outcomes it is handed; producing those outcomes by running the
//!   generated stub and the dynamic invoker against a real peer, both byte
//!   orders, is the §8 oracle and belongs to the `gen-corpus` oracle binary,
//!   not here. This module is the deterministic judgement, not the
//!   measurement.

use std::collections::BTreeMap;

use orbweaver_dynamic::invoke::Outcome;

/// Per-(repository id, operation) call counters, feeding [`PromotionPolicy`].
///
/// Counts only — see the module docs for why there is no clock here.
#[derive(Debug, Default)]
pub struct CallStats {
    counts: BTreeMap<(String, String), Counts>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Counts {
    calls: u64,
    failures: u64,
}

impl CallStats {
    /// An empty history.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty history, in a constant.
    ///
    /// [`crate::interceptor::Chain::stats`] hands out a reference and has to
    /// have something to point at when a chain carries no telemetry stage.
    /// Returning `Option` there would push the same empty case onto every
    /// caller of `Bridge::stats`, which reads a history for a session that has
    /// made no calls in exactly the same way.
    pub const fn empty() -> Self {
        Self { counts: BTreeMap::new() }
    }

    /// Records one call of `operation` on the interface `id` names.
    ///
    /// `ok` is whether the call completed; a refusal, a marshalling error and
    /// a transport error all count as failures alike, because a path that
    /// fails for any reason is not one to freeze into compiled code yet.
    pub fn record(&mut self, id: &str, operation: &str, ok: bool) {
        let c = self.counts.entry((id.to_owned(), operation.to_owned())).or_default();
        c.calls += 1;
        if !ok {
            c.failures += 1;
        }
    }

    /// Adds another history into this one.
    ///
    /// Exists so that PLAN-MOE's **IF2** — one store, not two — is reachable by
    /// a deployment that wants a single operational view across the dynamic
    /// session and the static guards it handed out. It is deliberately **not**
    /// done automatically, and the reason is the signal rather than the
    /// plumbing: [`PromotionPolicy::recommend`] answers "which dynamic path has
    /// earned a compiled stub", and a static call is evidence that a path
    /// *already has one*. Folding static counts in by default would keep a
    /// promoted path looking hot and have the policy recommend promoting it
    /// again — the store would be one, and the number in it would mean two
    /// things at once.
    ///
    /// So merging is the caller's explicit act, taken when the question is
    /// "how much traffic does this operation carry" rather than "what should be
    /// promoted next".
    ///
    /// **IF2는 이 함수로 도달 가능하지만 자동이 아니다.** 정적 호출은 그 경로가
    /// *이미* 스텁을 가졌다는 증거이므로, 기본으로 합치면 승격된 경로가 계속
    /// 뜨거워 보이고 정책이 같은 것을 또 승격하라고 말한다.
    pub fn merge(&mut self, other: &CallStats) {
        for ((id, operation), c) in &other.counts {
            let mine = self.counts.entry((id.clone(), operation.clone())).or_default();
            mine.calls += c.calls;
            mine.failures += c.failures;
        }
    }

    /// How many calls of `operation` on `id` have been recorded.
    pub fn calls(&self, id: &str, operation: &str) -> u64 {
        self.get(id, operation).calls
    }

    /// How many of those calls failed.
    pub fn failures(&self, id: &str, operation: &str) -> u64 {
        self.get(id, operation).failures
    }

    fn get(&self, id: &str, operation: &str) -> Counts {
        self.counts.get(&(id.to_owned(), operation.to_owned())).copied().unwrap_or_default()
    }
}

/// When a dynamic path has earned a static stub.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PromotionPolicy {
    /// A path must have been called at least this often. Below it there is
    /// not enough evidence that the path is hot or that its contract is
    /// exercised.
    pub min_calls: u64,
    /// The highest acceptable failure rate, inclusive: `0.0` means "no
    /// recorded failure is tolerated". A path that fails is not a candidate —
    /// promoting it would compile the failure in.
    pub max_failure_rate: f64,
}

/// One (interface, operation) pair the policy recommends promoting.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// The repository id of the interface.
    pub id: String,
    /// The operation on it.
    pub operation: String,
    /// How many calls were recorded.
    pub calls: u64,
    /// The recorded failure rate, `failures / calls`.
    pub failure_rate: f64,
}

impl PromotionPolicy {
    /// Every path that crossed `min_calls` with a failure rate within
    /// `max_failure_rate`, hottest first, ties broken by (id, operation).
    ///
    /// The ordering is pinned because the recommendation is an input to a
    /// batch: "generate stubs for the top N" must name the same N for the
    /// same history, or the batch is not reproducible.
    pub fn recommend(&self, stats: &CallStats) -> Vec<Candidate> {
        let mut out: Vec<Candidate> = stats
            .counts
            .iter()
            .filter(|(_, c)| c.calls >= self.min_calls)
            .map(|((id, operation), c)| Candidate {
                id: id.clone(),
                operation: operation.clone(),
                calls: c.calls,
                failure_rate: c.failures as f64 / c.calls as f64,
            })
            .filter(|cand| cand.failure_rate <= self.max_failure_rate)
            .collect();
        out.sort_by(|a, b| {
            b.calls
                .cmp(&a.calls)
                .then_with(|| a.id.cmp(&b.id))
                .then_with(|| a.operation.cmp(&b.operation))
        });
        out
    }
}

/// Why a promotion was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromotionRegression {
    /// Promotion dropped the identity context: the static path made the call
    /// as a different principal than the dynamic path it is replacing. This
    /// is checked before the results are compared, because a promotion that
    /// keeps the answer and loses the caller is the §4.8 confused deputy
    /// (R13) reappearing through an optimization.
    IdentityDropped {
        /// The operation whose promotion was refused.
        operation: String,
        /// Who the dynamic path ran as.
        dynamic_caller: String,
        /// Who the static path ran as.
        static_caller: String,
    },
    /// The two audit lines are not about the same operation, so the
    /// comparison would be meaningless whatever it found.
    OperationMismatch {
        /// The operation the dynamic audit line names.
        dynamic_operation: String,
        /// The operation the static audit line names.
        static_operation: String,
    },
    /// The static path produced a different result than the dynamic path for
    /// the same call. The dynamic path is the reference implementation
    /// (PHASE4), so a divergence is always the stub's bug.
    ResultMismatch {
        /// The operation whose results diverged.
        operation: String,
    },
    /// An audit line could not be parsed, so identity preservation could not
    /// be verified. Unverifiable is a refusal, never a pass — the same rule
    /// the harness lives by.
    MalformedAudit {
        /// The line that did not parse.
        line: String,
    },
    /// An audit line describes a call that never happened — a
    /// [`crate::dryrun`] prediction, not a measurement. Comparing a promotion
    /// against one would be gating a real change on a hypothesis, so the token
    /// that distinguishes the two is checked rather than ignored.
    HypotheticalAudit {
        /// The dry-run line that was offered as evidence.
        line: String,
    },
    /// An audit line is the ledger's **elision marker**: it stands where lines
    /// used to be, so this position in the history is a hole rather than a
    /// decision.
    ///
    /// The bounded ledger ([`crate::interceptor::AuditInterceptor`]) is what
    /// makes this reachable, and refusing it is the whole reason the marker is
    /// in-band. The gate reads what `Bridge::audit` recorded; a gate that
    /// parsed a gap would either fail to parse it — and report a *malformed*
    /// line, which is a different and misleading complaint — or, if the marker
    /// had been written in the decision format, read a caller and an operation
    /// off it and judge a promotion against lines nobody kept. Unverifiable is
    /// a refusal, and a truncated history is unverifiable *by name*.
    TruncatedAudit {
        /// The elision marker that was offered as evidence.
        line: String,
        /// How many lines it says are missing.
        dropped: u64,
    },
}

impl std::fmt::Display for PromotionRegression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PromotionRegression::IdentityDropped { operation, dynamic_caller, static_caller } => {
                write!(
                    f,
                    "promotion dropped the identity context: {operation} ran dynamically as \
                     {dynamic_caller} but statically as {static_caller}"
                )
            }
            PromotionRegression::OperationMismatch { dynamic_operation, static_operation } => {
                write!(
                    f,
                    "the audit lines name different operations: dynamic {dynamic_operation}, \
                     static {static_operation}"
                )
            }
            PromotionRegression::ResultMismatch { operation } => {
                write!(f, "the static path answered {operation} differently than the dynamic path")
            }
            PromotionRegression::MalformedAudit { line } => {
                write!(f, "cannot verify identity preservation: unparseable audit line {line:?}")
            }
            PromotionRegression::HypotheticalAudit { line } => write!(
                f,
                "that audit line is a dry run and not a call: {line:?}. A promotion is gated on \
                 what the two paths did, never on what they would have done"
            ),
            PromotionRegression::TruncatedAudit { line, dropped } => write!(
                f,
                "that audit line is the ledger's elision marker for {dropped} dropped line(s), \
                 not a decision: {line:?}. The history it stands for is gone, so there is \
                 nothing here to gate a promotion on"
            ),
        }
    }
}

impl std::error::Error for PromotionRegression {}

/// The caller and operation an audit line attests to, in the format
/// [`crate::guard::Guarded`] and the dynamic path both write:
/// `ALLOW caller=<principal> target=<id> operation=<op>`.
struct AuditContext<'a> {
    /// The line's first field: what was decided, and — since the dry run
    /// landed — whether anything was actually done about it.
    decision: &'a str,
    caller: &'a str,
    operation: &'a str,
}

/// Parses one of the guard's audit lines. This reads the format guard.rs
/// already writes rather than defining its own: the gate must compare what
/// was actually recorded, and a second format would let the two drift.
fn parse_audit(line: &str) -> Option<AuditContext<'_>> {
    let mut caller = None;
    let mut operation = None;
    for field in line.split_whitespace() {
        if let Some(v) = field.strip_prefix("caller=") {
            caller = Some(v);
        } else if let Some(v) = field.strip_prefix("operation=") {
            operation = Some(v);
        }
    }
    Some(AuditContext {
        decision: line.split_whitespace().next()?,
        caller: caller?,
        operation: operation?,
    })
}

/// The regression gate: may this static path replace the dynamic path it was
/// promoted from? §7.4 integration point I4.
///
/// `dynamic_audit` and `static_audit` are the audit lines the two paths wrote
/// for the compared call, in the guard's format. The checks, in order:
///
/// 1. Neither line may be the ledger's elision marker
///    ([`PromotionRegression::TruncatedAudit`]), because a marker is a hole in
///    the history and not a decision — checked first, so a truncated ledger is
///    refused for what it is rather than reported as a malformed line.
///    Both lines must then parse — an unverifiable line refuses. A line from
///    a [`crate::dryrun`] parses and is still refused, by
///    [`PromotionRegression::HypotheticalAudit`]: it records what a path
///    *would* have done, and a promotion decided on that is decided on nothing
///    that happened.
/// 2. **The identity context must match.** Caller first: a differing caller
///    is [`PromotionRegression::IdentityDropped`], and it fires even when the
///    results are identical, because a right answer under the wrong principal
///    is the confused deputy, not a passing test. Then the operation fields.
/// 3. The outcomes must be equal, compared as decoded
///    [`orbweaver_dynamic::Value`]s ([`Outcome`] equality) — never as raw
///    buffers, per the wire rules: CDR padding content is unspecified.
pub fn verify_promotion(
    dynamic_outcome: &Outcome,
    static_outcome: &Outcome,
    dynamic_audit: &str,
    static_audit: &str,
) -> Result<(), PromotionRegression> {
    // A gap is not evidence, and it is checked before anything is parsed: the
    // marker deliberately does not carry a `caller=` or an `operation=`, so a
    // gate that reached the parser first would refuse it as *malformed* and
    // send whoever reads the error looking for a formatting bug instead of for
    // a ledger that overflowed.
    for line in [dynamic_audit, static_audit] {
        if let Some(dropped) = crate::guard::elided_count(line) {
            return Err(PromotionRegression::TruncatedAudit { line: line.to_owned(), dropped });
        }
    }
    let dynamic = parse_audit(dynamic_audit)
        .ok_or_else(|| PromotionRegression::MalformedAudit { line: dynamic_audit.to_owned() })?;
    let promoted = parse_audit(static_audit)
        .ok_or_else(|| PromotionRegression::MalformedAudit { line: static_audit.to_owned() })?;

    // A prediction is not evidence. Checked before anything is compared,
    // because comparing two hypotheticals would "pass" perfectly well.
    for (line, parsed) in [(dynamic_audit, &dynamic), (static_audit, &promoted)] {
        if crate::guard::is_hypothetical(parsed.decision) {
            return Err(PromotionRegression::HypotheticalAudit { line: line.to_owned() });
        }
    }

    // Identity before results: I4's first test, see the module docs.
    if dynamic.caller != promoted.caller {
        return Err(PromotionRegression::IdentityDropped {
            operation: dynamic.operation.to_owned(),
            dynamic_caller: dynamic.caller.to_owned(),
            static_caller: promoted.caller.to_owned(),
        });
    }
    if dynamic.operation != promoted.operation {
        return Err(PromotionRegression::OperationMismatch {
            dynamic_operation: dynamic.operation.to_owned(),
            static_operation: promoted.operation.to_owned(),
        });
    }
    if dynamic_outcome != static_outcome {
        return Err(PromotionRegression::ResultMismatch {
            operation: dynamic.operation.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    /// IF2 is reachable, and deliberately not automatic. Merging is what a
    /// deployment does when the question is traffic; leaving the stores apart
    /// is what keeps the promotion signal meaning one thing.
    #[test]
    fn merging_is_available_and_would_distort_the_promotion_signal() {
        let mut dynamic = CallStats::new();
        for _ in 0..3 {
            dynamic.record("IDL:m/I:1.0", "hot", true);
        }
        let mut static_side = CallStats::new();
        for _ in 0..40 {
            static_side.record("IDL:m/I:1.0", "hot", true);
        }

        // Apart: a policy wanting ten calls does not see a promoted path as a
        // candidate, which is the answer promotion needs.
        let policy = PromotionPolicy { min_calls: 10, max_failure_rate: 0.5 };
        assert!(policy.recommend(&dynamic).is_empty());

        // Folded: the same history now recommends promoting a path that is
        // already static — the number means two things at once, which is why
        // this is the caller's explicit act and not the default.
        let mut joined = CallStats::new();
        joined.merge(&dynamic);
        joined.merge(&static_side);
        assert_eq!(joined.calls("IDL:m/I:1.0", "hot"), 43);
        assert_eq!(policy.recommend(&joined).len(), 1);
    }

    use orbweaver_cdr::{Encoder, Endian};
    use orbweaver_giop::{Error as GiopError, Invoker, Reply};
    use orbweaver_registry::Registry;

    use super::*;
    use crate::guard::Guarded;
    use crate::identity::Caller;
    use crate::policy::{Approval, Exposure};

    fn outcome(returns: orbweaver_dynamic::Value) -> Outcome {
        Outcome { returns, outputs: BTreeMap::new() }
    }

    fn stats(entries: &[(&str, &str, u64, u64)]) -> CallStats {
        let mut s = CallStats::new();
        for (id, op, ok, failed) in entries {
            for _ in 0..*ok {
                s.record(id, op, true);
            }
            for _ in 0..*failed {
                s.record(id, op, false);
            }
        }
        s
    }

    #[test]
    fn below_min_calls_nothing_is_recommended() {
        let s = stats(&[("IDL:bank/Account:1.0", "balance", 9, 0)]);
        let policy = PromotionPolicy { min_calls: 10, max_failure_rate: 0.0 };
        assert!(policy.recommend(&s).is_empty());
    }

    #[test]
    fn a_failing_path_is_excluded_however_hot() {
        let s = stats(&[
            ("IDL:bank/Account:1.0", "balance", 100, 0),
            // Hotter, but 20% failures against a 5% bar.
            ("IDL:bank/Account:1.0", "deposit", 160, 40),
        ]);
        let policy = PromotionPolicy { min_calls: 10, max_failure_rate: 0.05 };
        let got = policy.recommend(&s);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0].operation, "balance");
        assert_eq!(got[0].calls, 100);
        assert_eq!(got[0].failure_rate, 0.0);
    }

    /// Pinned because the recommendation is a batch input: the same history
    /// must name the same candidates in the same order every run.
    #[test]
    fn ordering_is_hottest_first_with_ties_by_name() {
        let s = stats(&[
            ("IDL:z/Z:1.0", "aa", 50, 0),
            ("IDL:a/A:1.0", "zz", 50, 0),
            ("IDL:a/A:1.0", "mm", 200, 0),
        ]);
        let policy = PromotionPolicy { min_calls: 1, max_failure_rate: 0.0 };
        let names: Vec<(String, String)> =
            policy.recommend(&s).into_iter().map(|c| (c.id, c.operation)).collect();
        assert_eq!(
            names,
            vec![
                ("IDL:a/A:1.0".to_owned(), "mm".to_owned()),
                ("IDL:a/A:1.0".to_owned(), "zz".to_owned()),
                ("IDL:z/Z:1.0".to_owned(), "aa".to_owned()),
            ]
        );
    }

    #[test]
    fn identical_outcomes_and_audits_pass_the_gate() {
        let out = outcome(orbweaver_dynamic::Value::Long(42));
        verify_promotion(
            &out,
            &out.clone(),
            "ALLOW caller=alice target=IDL:bank/Account:1.0 operation=balance",
            "ALLOW caller=alice target=IDL:bank/Account:1.0 operation=balance",
        )
        .expect("an identical promotion must pass");
    }

    #[test]
    fn a_differing_value_is_a_result_mismatch_naming_the_operation() {
        let dynamic = outcome(orbweaver_dynamic::Value::Long(42));
        let promoted = outcome(orbweaver_dynamic::Value::Long(43));
        let audit = "ALLOW caller=alice target=IDL:bank/Account:1.0 operation=balance";
        let err = verify_promotion(&dynamic, &promoted, audit, audit).unwrap_err();
        assert_eq!(
            err,
            PromotionRegression::ResultMismatch { operation: "balance".to_owned() },
            "{err}"
        );
    }

    /// I4's first test: identical answers, different principal. The right
    /// result under the wrong caller must refuse — this is the confused
    /// deputy, and it is exactly the case a result-only gate waves through.
    #[test]
    fn a_dropped_caller_refuses_even_when_the_results_match_exactly() {
        let out = outcome(orbweaver_dynamic::Value::Long(42));
        let err = verify_promotion(
            &out,
            &out.clone(),
            "ALLOW caller=alice target=IDL:bank/Account:1.0 operation=balance",
            "ALLOW caller=<nobody> target=IDL:bank/Account:1.0 operation=balance",
        )
        .unwrap_err();
        assert_eq!(
            err,
            PromotionRegression::IdentityDropped {
                operation: "balance".to_owned(),
                dynamic_caller: "alice".to_owned(),
                static_caller: "<nobody>".to_owned(),
            },
            "{err}"
        );
    }

    #[test]
    fn an_unparseable_audit_line_refuses_rather_than_passes() {
        let out = outcome(orbweaver_dynamic::Value::Long(1));
        let err =
            verify_promotion(&out, &out.clone(), "not an audit line", "also not one").unwrap_err();
        assert!(matches!(err, PromotionRegression::MalformedAudit { .. }), "{err}");
    }

    /// The hazard the dry run introduced, closed. A dry-run line is
    /// well-formed, names a caller and an operation, and would sail through a
    /// gate that only looked at those — while attesting to a call nobody made.
    /// The line here is one a real `Guarded::dry_run` wrote.
    #[test]
    fn a_dry_run_line_is_refused_as_evidence_however_well_formed_it_is() {
        let reg = registry();
        let exposure = Exposure::nothing().allow_interface("IDL:bank/Account:1.0");
        let mut g: Guarded<'_, Recorder> = Guarded::assemble(
            Recorder { reached: Vec::new() },
            &reg,
            exposure,
            Some(Caller::new("alice")),
            "IDL:bank/Account:1.0".to_owned(),
            Approval::default(),
            crate::handles::shared("s-test"),
        );
        g.dry_run("balance");
        let hypothetical = g.audit().last().expect("the dry run was recorded").clone();
        assert!(hypothetical.contains("caller=alice"), "{hypothetical}");

        let real = static_audit_line(&reg, Some(Caller::new("alice")));
        let out = outcome(orbweaver_dynamic::Value::Long(42));
        let err = verify_promotion(&out, &out.clone(), &hypothetical, &real).unwrap_err();
        assert_eq!(
            err,
            PromotionRegression::HypotheticalAudit { line: hypothetical.clone() },
            "{err}"
        );
        // And in the other position too: neither side may be a prediction.
        let err = verify_promotion(&out, &out.clone(), &real, &hypothetical).unwrap_err();
        assert_eq!(err, PromotionRegression::HypotheticalAudit { line: hypothetical }, "{err}");
    }

    /// The hazard the *bounded* ledger introduces, closed the same way the dry
    /// run's was: the gate must refuse a gap by name rather than conclude
    /// anything from it.
    ///
    /// The marker here is one a real bounded ledger wrote, not a string this
    /// test invented — the same discipline as the dry-run case above, and for
    /// the same reason: a gate tested against a hand-written approximation of a
    /// format is a gate tested against the wrong thing.
    #[test]
    fn the_ledgers_elision_marker_is_refused_rather_than_judged_as_a_gap() {
        let reg = registry();
        let exposure = Exposure::nothing().allow_interface("IDL:bank/Account:1.0");
        let mut g: Guarded<'_, Recorder> = Guarded::assemble(
            Recorder { reached: Vec::new() },
            &reg,
            exposure,
            Some(Caller::new("alice")),
            "IDL:bank/Account:1.0".to_owned(),
            Approval::default(),
            crate::handles::shared("s-test"),
        );
        // A ledger with room for the marker and two lines, overflowed.
        assert!(g.chain_mut().audit_capacity(3));
        for _ in 0..6 {
            let _ = g.invoke("balance", |_| {});
        }
        assert_eq!(g.audit_written(), 6);
        assert_eq!(g.audit_dropped(), 4);
        let marker = g.audit()[0].clone();
        let real = g.audit().last().expect("a decision survives").clone();
        assert!(real.starts_with("ALLOW caller=alice"), "{real}");

        let out = outcome(orbweaver_dynamic::Value::Long(42));
        // Offered where the static line goes...
        let err = verify_promotion(&out, &out.clone(), &real, &marker).unwrap_err();
        assert_eq!(
            err,
            PromotionRegression::TruncatedAudit { line: marker.clone(), dropped: 4 },
            "{err}"
        );
        // ...and where the dynamic one goes. Neither side may be a hole.
        let err = verify_promotion(&out, &out.clone(), &marker, &real).unwrap_err();
        assert_eq!(err, PromotionRegression::TruncatedAudit { line: marker, dropped: 4 }, "{err}");

        // The refusal is *specific*: a truncated ledger is not reported as a
        // formatting bug, which is what sends an operator looking in the wrong
        // place. And the surviving lines still gate normally, so a bounded
        // ledger costs the gate nothing it did not have to lose.
        verify_promotion(&out, &out.clone(), &real, &real)
            .expect("the lines the ledger kept are still evidence");
    }

    // --- the end-to-end shape: a real Guarded static path's audit line ---

    /// An invoker that records what reached it; guard.rs's Recorder pattern,
    /// rebuilt here because test modules do not share fakes.
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
            // No Reply can be built outside the wire; the tests only care
            // what got this far and what the guard recorded about it.
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
               };
             };",
        )
        .expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        r
    }

    fn static_audit_line(reg: &Registry, caller: Option<Caller>) -> String {
        let exposure = Exposure::nothing().allow_interface("IDL:bank/Account:1.0");
        let mut g: Guarded<'_, Recorder> = Guarded::assemble(
            Recorder { reached: Vec::new() },
            reg,
            exposure,
            caller,
            "IDL:bank/Account:1.0".to_owned(),
            Approval::default(),
            crate::handles::shared("s-test"),
        );
        let _ = g.invoke("balance", |_| {});
        g.audit().last().expect("the guard recorded the decision").clone()
    }

    /// End to end in shape: the static path is a real `Guarded`, and the
    /// audit line the gate reads is the one the guard actually wrote — not a
    /// string this test invented about the static side.
    #[test]
    fn a_guarded_static_path_that_kept_the_caller_passes_and_one_that_lost_it_refuses() {
        let reg = registry();
        let out = outcome(orbweaver_dynamic::Value::LongLong(1_000));
        let dynamic_audit = "ALLOW caller=alice target=IDL:bank/Account:1.0 operation=balance";

        // Promotion kept the caller: the guard was assembled on behalf of the
        // same principal the dynamic path ran as.
        let kept = static_audit_line(&reg, Some(Caller::new("alice")));
        verify_promotion(&out, &out.clone(), dynamic_audit, &kept)
            .expect("same caller, same result: the promotion must pass");

        // The same promotion rebuilt without a caller — the optimization that
        // forgot on_behalf_of. The result is identical; the gate must still
        // refuse, and must say why by name.
        let dropped = static_audit_line(&reg, None);
        let err = verify_promotion(&out, &out.clone(), dynamic_audit, &dropped).unwrap_err();
        assert!(
            matches!(
                &err,
                PromotionRegression::IdentityDropped { static_caller, .. }
                    if static_caller == "<nobody>"
            ),
            "{err}"
        );
    }

    /// The capture seam, closed at this end: the dynamic line fed to the gate
    /// is one a real `Bridge` *emitted* — taken from `Bridge::audit()`, not a
    /// string reconstructed from session state — and it parses and gates
    /// against a real `Guarded`'s line for the same call. This is what lets
    /// the gen-corpus oracle's I4 section replace reconstruction with capture.
    #[test]
    fn a_bridge_emitted_dynamic_line_parses_and_gates_unchanged() {
        use orbweaver_giop::{Connection, IiopProfile, Ior, Version};

        use crate::Bridge;

        let reg = registry();
        let exposure = Exposure::nothing().allow_interface("IDL:bank/Account:1.0");

        // A real Connection to a listener that answers nothing. The call is
        // given an argument `balance()` does not take, so it passes the
        // policy gate — writing the ALLOW line — and fails at mapping before
        // anything touches the wire.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
        let ior = Ior {
            type_id: "IDL:bank/Account:1.0".into(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: "127.0.0.1".into(),
                port: listener.local_addr().expect("has an address").port(),
                object_key: b"acct-1".to_vec(),
                components: Vec::new(),
            }],
        };
        let mut conn = Connection::connect(&ior, std::time::Duration::from_secs(5)).expect("dials");

        let mut bridge =
            Bridge::new(&reg, exposure, "i4-capture").on_behalf_of(Caller::new("alice"));
        let handle = bridge.handles().issue_checked(&ior).expect("issued");
        let args = orbweaver_dynamic::json::Json::Object(BTreeMap::from([(
            "bogus".to_owned(),
            orbweaver_dynamic::json::Json::Number("1".into()),
        )]));
        let _ = bridge.invoke(&mut conn, handle.as_str(), "balance", &args, Approval::default());
        let dynamic_audit =
            bridge.audit().last().cloned().expect("the bridge recorded its decision");

        let static_audit = static_audit_line(&reg, Some(Caller::new("alice")));
        let out = outcome(orbweaver_dynamic::Value::Long(42));
        verify_promotion(&out, &out.clone(), &dynamic_audit, &static_audit)
            .expect("a captured dynamic line must parse and gate exactly like the format it is");
    }
}
