//! S7 — verification: property tests from types, contract advice from meaning.
//!
//! `docs/PLAN.md` §5 names S7 "Verify: contract tests, interceptors, tracing",
//! and §8's verification table gives the generated-code row a pass criterion of
//! "compile, contract tests, static result equals dynamic result". This crate
//! is the contract-test half of that row, plus the DynAny-shaped fuzzing the
//! component ledger asks for.
//!
//! Two halves, deliberately unequal in what they claim.
//!
//! # [`prop`] — a real gate
//!
//! [`prop::roundtrip_property`] generates deterministic sample [`Value`]s for a
//! `TypeCode` and asserts that encode → decode → encode is byte-stable, in both
//! byte orders and at every alignment phase — and, since 2026-08-19, that the
//! same value taken out through AnyJSON and back in encodes to the same bytes.
//! A failure here is a **defect**: the bytes we put on a wire disagree with the
//! bytes we would put on it after reading our own message back, or after an
//! agent read it and sent it back, and no annotation opinion is involved. These
//! findings are [`Severity::Error`] and the CLI exits non-zero on them.
//!
//! # [`contract`] — advice, and never anything stronger
//!
//! [`contract::contract_findings`] reads the SIDL vocabulary (§2.2) against the
//! types it annotates and reports where the two disagree. **S4 gates syntax and
//! semantics; this crate gates meaning, which is a strictly weaker claim.**
//!
//! The difference is not modesty, it is what the two checks can know. S4 asks
//! questions the language answers: is this identifier declared, does it clash
//! case-insensitively, is this union label a duplicate. Every one of those has
//! exactly one right answer, so refusing the file is honest. This crate asks
//! whether prose a human wrote — `ai_effect: read_only` — is true of code the
//! same human wrote, and it answers by pattern: a verb in an operation name, a
//! shape in a signature, a value outside a known set. Those are evidence, not
//! proof. `purge_cache` may genuinely be read-only against the domain the
//! contract models, and no checker here can know that.
//!
//! So the strongest honest verdict this crate reaches is *a human should look*,
//! and a check that cannot reach further must not block a build. There is a
//! practical edge to that too: a heuristic gate is a gate people route around,
//! and a routed-around gate stops catching the cases it was right about.
//!
//! **S4는 문법과 의미론을 막고, 이 크레이트는 *뜻*을 본다 — 더 약한 주장이다.**
//! 사람이 쓴 주석이 같은 사람이 쓴 코드에 대해 참인지를 패턴으로 판단하므로,
//! 결과는 증거이지 증명이 아니다. 그래서 조언에 머무르며 빌드를 막지 않는다.
//!
//! Severities within that ceiling split on one question — *will any consumer
//! ever act on what the contract says?*
//!
//! - [`Severity::Warning`] — the contract states something **no consumer
//!   reads**, so the author believes a control is in place that is not. An
//!   `ai_authz` scope written where the guard never looks is worse than no
//!   scope at all, because the absence would at least be visible to S4.
//! - [`Severity::Advice`] — a consumer *will* act on it, and it looks wrong.
//!   Acting on it may well be correct; the finding says why it might not be.
//!
//! # What this crate is not
//!
//! It is not the differential oracle. `spikes/differential.sh` and the omniORB
//! fixtures answer "are we right about the wire?" by asking somebody else. The
//! property tests here answer "are we self-consistent?", which is cheaper,
//! runs in-process, and catches a different class — the class where both
//! directions of our own code agree with each other about the wrong thing is
//! precisely what it cannot catch, and the fixtures exist for that.
//!
//! # [`wire`] and [`agent`] — the same question at two boundaries
//!
//! Both ask what a parser does when a stranger picked the input, and they are
//! separate modules because they are separate claims about separate strangers.
//! [`wire`] covers the decoders a **peer** reaches before any policy runs;
//! [`agent`] covers the parsers an **agent** reaches through `tools/call`,
//! which since AnyJSON v1.1 (D008) include one that reads a whole `TypeCode`
//! out of the agent's own document. §9.0's R11/R12 put the two in the same
//! threat model, so neither list is a subset of the other and neither run can
//! stand in for the other's green.
//!
//! **피어와 에이전트는 같은 등급의 비신뢰 입력이지만 도달하는 파서가 다르다.**
//!
//! [`Value`]: orbweaver_dynamic::Value

#![deny(missing_docs)]

pub mod agent;
pub mod contract;
pub mod prop;
pub mod wire;

use orbweaver_forge::{Finding, Report, Severity};

/// Builds a finding with no source span.
///
/// Everything this crate reports comes from the registry or from a generated
/// value, neither of which carries a source position — the registry is built
/// from an AST that has already been dropped. S4's evolution findings take the
/// same shape (line 0, the identifier in `source`), so tooling that already
/// renders those renders these.
fn finding(
    rule: &str,
    severity: Severity,
    message: String,
    source: String,
    fix: Option<String>,
) -> Finding {
    Finding { rule: rule.to_owned(), severity, message, line: 0, column: 0, source, fix }
}

/// Every check this crate has, over one registry.
///
/// The batch unit is the whole registry rather than one interface, for the
/// reason §5.1 gives: a per-item run lets you fix seven symptoms and never see
/// the one cause. [`Report::repair_prompt`] then groups by rule, which is where
/// the cause becomes visible.
///
/// Property tests run over every registered *type*, not over operation
/// signatures: a parameter's type is a registered type, so covering the types
/// covers the signatures without generating the same value once per mention.
pub fn check(registry: &orbweaver_registry::Registry, cases: usize) -> Report {
    check_measured(registry, cases, prop::DEFAULT_SEED).0
}

/// [`check`] from an explicit batch seed, also returning how much the
/// property half measured — the counts `contract-check` prints beside the
/// case count, so a leg that stops running is a number that dropped rather
/// than a finding that never appeared.
pub fn check_measured(
    registry: &orbweaver_registry::Registry,
    cases: usize,
    seed: u64,
) -> (Report, prop::Measured) {
    let mut findings = contract::contract_findings(registry);
    let mut measured = prop::Measured::default();
    for id in registry.ids().cloned().collect::<Vec<_>>() {
        if let Some(tc) = registry.typecode(&id) {
            let (found, m) = prop::roundtrip_property_measured(tc, cases, seed);
            findings.extend(found);
            measured.add(m);
        }
    }
    findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.rule.cmp(&b.rule)));
    (Report { findings }, measured)
}

/// [`check_measured`] plus the one finding that lives on the file rather than
/// in the registry: the SIDL version a contract declares
/// ([`contract::sidl_version_findings`]).
///
/// A separate entry point because a registry has no file to read a marker
/// from — the lexer hands `//@ sidl_version: N` to the first declaration, and
/// when that is a `module` the registry keeps nothing. `contract-check` has
/// the checked source in hand and calls this; a caller with only a registry
/// keeps [`check_measured`], and gets no version verdict, which is honest.
pub fn check_source_measured(
    spec: &orbweaver_idl::ast::Spec,
    registry: &orbweaver_registry::Registry,
    cases: usize,
    seed: u64,
) -> (Report, prop::Measured) {
    let (mut report, measured) = check_measured(registry, cases, seed);
    report.findings.extend(contract::sidl_version_findings(spec));
    report.findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.rule.cmp(&b.rule)));
    (report, measured)
}

/// Whether a report contains a genuine defect, as opposed to advice.
///
/// The exit-code rule in one place: a byte instability is a defect and an
/// annotation smell is not.
pub fn has_defect(report: &Report) -> bool {
    report.findings.iter().any(|f| f.severity == Severity::Error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(src: &str) -> orbweaver_registry::Registry {
        let spec = orbweaver_idl::parse(src).expect("parses");
        let mut r = orbweaver_registry::Registry::new();
        r.load(&spec).expect("loads");
        r
    }

    /// The whole-registry entry point runs both halves and keeps advice out of
    /// the verdict.
    #[test]
    fn check_runs_both_halves_and_only_defects_fail_it() {
        let r = registry(
            "module m {
               struct S { long a; string b; double c; };
               interface I {
                 //@ ai_effect: read_only
                 oneway void poll();
               };
             };",
        );
        let report = check(&r, 8);
        assert!(!has_defect(&report), "no property defect: {:?}", report.findings);
        assert!(
            report.findings.iter().any(|f| f.rule == "contract/read-only-oneway"),
            "the contract half ran too: {:?}",
            report.findings
        );
    }

    /// Advice must never reach `Severity::Error`, whatever it finds. The
    /// module documentation makes this a promise, so it is tested rather than
    /// merely written down.
    #[test]
    fn no_contract_rule_can_ever_produce_an_error() {
        let r = registry(
            "module m {
               interface I {
                 //@ ai_effect: probably_fine
                 //@ ai_idempotent: perhaps
                 //@ ai_nonsense: x
                 void delete_everything(
                   //@ ai_pii: extreme
                   //@ ai_unit: KRW
                   in string who);
               };
             };",
        );
        let findings = contract::contract_findings(&r);
        assert!(!findings.is_empty(), "the sample is full of smells");
        assert!(
            findings.iter().all(|f| f.severity != Severity::Error),
            "meaning is never an error gate: {findings:?}"
        );
    }
}
