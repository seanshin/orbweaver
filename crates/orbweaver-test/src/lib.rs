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
pub mod state;
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

/// The part of a head that does not depend on the subject, taken by calling the
/// head with a sentinel and removing it.
///
/// A classifier is a sentence too. Asking `orbweaver-dynamic` what its heads
/// say — rather than writing a fragment of them here — is the difference
/// between a count that follows the wording and a count that quietly stops
/// following it.
fn head_marker(head: fn(&str) -> String) -> String {
    const SENTINEL: &str = "\u{0}";
    head(SENTINEL).replace(SENTINEL, "")
}

/// The types the property half could not measure **because the wire does not
/// carry them** — the `prop/unsupported-type` and `json/unmapped` findings
/// whose sentence reads one of `orbweaver-dynamic`'s two heads, counted once
/// per type.
///
/// **Three families, and the classifier has to know that.** It filtered on the
/// literal `"§4.4"` until 2026-08-24, so the four `native` declarations were in
/// the set only because their sentence mentioned the section in order to say it
/// does *not* apply. The day that wording moved into the shared head — which
/// names no section, because a native is not deferred — the count went 18 to 14
/// with nothing else changed, and the label above it still read "§4.4 and
/// natives". A classifier is a sentence too: it asks the crate that owns the
/// heads what they say, rather than keeping a fragment of one.
///
/// The third marker landed 2026-08-26 with the `Principal` family, and it is
/// the case that shows asking-the-owner is not the whole rule: this function
/// asked correctly and still could not see a `Principal`, because until that
/// day no head owned that sentence and `prop.rs` wrote two of its own. **A
/// classifier that asks the owner is exactly as complete as the owner's list
/// is** — so a family arriving without a published head is invisible here, and
/// the thing that catches *that* is a fixture-shaped assertion
/// (`tests/one_home_for_a_wire_refusal.rs`), not a better filter.
///
/// Advice, still, and deliberately: the sweep cannot round-trip a `fixed`, so
/// there is no defect to report and no severity to raise. What was missing was
/// the *number*. Over `corpus/golden/` these findings were two advice lines
/// among a hundred, indistinguishable in the summary from an unannotated
/// operation, and the count of types the wire cannot serve was visible
/// nowhere a harness could pin. `contract-check` prints this beside the
/// closure S4 computes ([`orbweaver_idl::deferred_wire_types`]); the two are
/// not the same number, and the difference is itself a measurement — see the
/// binary.
pub fn deferred_wire_gaps(report: &Report) -> std::collections::BTreeSet<String> {
    let deferred = head_marker(orbweaver_dynamic::deferred_wire_head);
    let native = head_marker(orbweaver_dynamic::unmarshallable_wire_head);
    let withdrawn = head_marker(orbweaver_dynamic::withdrawn_wire_head);
    report
        .findings
        .iter()
        .filter(|f| {
            matches!(f.rule.as_str(), "prop/unsupported-type" | "json/unmapped")
                && (f.message.contains(&deferred)
                    || f.message.contains(&native)
                    || f.message.contains(&withdrawn))
        })
        .map(|f| f.source.clone())
        .collect()
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

    /// `corpus/golden/21`'s shape: the property cannot sample `fixed`, and the
    /// two advice findings that say so are counted once per type — the typedef
    /// and the struct carrying it — while the interface, which has no
    /// `TypeCode`, is S4's to count. A file the wire carries counts zero.
    #[test]
    fn the_types_the_wire_defers_are_counted_once_each_and_stay_advice() {
        let r = registry(
            "module m { typedef fixed<9,2> Amount; struct Invoice { Amount total; }; \
             interface Billing { Amount sum(in Amount a); }; };",
        );
        let report = check(&r, 4);
        assert!(!has_defect(&report), "{:?}", report.findings);
        let gaps = deferred_wire_gaps(&report);
        assert_eq!(gaps.len(), 2, "{gaps:?}");
        assert!(gaps.iter().all(|id| id.contains("Amount") || id.contains("Invoice")), "{gaps:?}");
        // Four findings feed two entries: json/unmapped and prop/unsupported-type
        // for each of the two types.
        assert_eq!(report.findings.iter().filter(|f| f.message.contains("§4.4")).count(), 4);

        let clean = registry("module m { struct S { long a; }; };");
        assert!(deferred_wire_gaps(&check(&clean, 4)).is_empty());
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
