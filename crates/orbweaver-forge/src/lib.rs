//! S4 — the validation gate.
//!
//! `docs/PLAN.md` §5 calls this the safety belt of the whole system, and the
//! reason is an asymmetry: **an LLM writes plausible IDL that may be
//! semantically wrong; a deterministic checker rejects wrong IDL every time
//! without exception.** Everything upstream of S4 is allowed to be uncertain
//! because S4 is not. Remove it and the trust model has nothing left holding it
//! up.
//!
//! # Diagnostics are the product
//!
//! §3.3: the self-repair loop is only as good as the messages it feeds on, so
//! error quality is a *tested feature* rather than a nicety. A diagnostic here
//! carries a stable rule name, a source span, and — where the rule permits one
//! — a concrete fix, phrased as an edit rather than as a complaint.
//!
//! Phase 0 measured what this is worth. Twenty generated files produced seven
//! failures sharing one root cause, and one message that names the cause turned
//! 65% into 100% in a single round. The message is the mechanism.
//!
//! # What it does not do
//!
//! It does not call an external compiler. The differential oracles
//! (`spikes/differential.sh`) are a separate check with a separate purpose:
//! they tell us whether *we* are right. This gate tells a generator whether its
//! output is, and has to run in-process, in milliseconds, thousands of times.
//!
//! # The rest of the crate
//!
//! S4 is the gate; the stages around it each own a producer and a gate of their
//! own, so a failure can be attributed to the stage that caused it:
//!
//! | Module | Stage | In → out | Its own gate |
//! |---|---|---|---|
//! | [`ingest`] | S1 | requirement text → [`ingest::Brief`] | is it a brief S2 can work from |
//! | [`synthesize`] | S2 | brief → `.idl` | [`validate`], plus: is anything callable, did the brief survive |
//! | [`annotate`] | S3 | `.idl` → SIDL | is every operation annotated, is every mutating operation scoped |
//! | [`pipeline`] | — | the §5.1 loop over any one of them, and over all of them | — |
//!
//! Each is runnable alone, because a pipeline that only runs end to end can
//! tell you the output is wrong and not which stage was wrong.

#![deny(missing_docs)]

pub mod annotate;
pub mod ingest;
pub mod pipeline;
pub mod synthesize;

use std::collections::BTreeMap;

use orbweaver_dynamic::json::Json;
use orbweaver_idl::Diagnostic;
use orbweaver_registry::Registry;
use orbweaver_registry::diff::{Verdict, diff};

/// How much a finding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth saying; the file is still usable.
    Advice,
    /// The file compiles but something downstream will suffer.
    Warning,
    /// The file is not usable.
    Error,
}

impl Severity {
    /// The lowercase name used in JSON and in messages.
    pub fn label(self) -> &'static str {
        match self {
            Severity::Advice => "advice",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// One thing wrong, in the form a generator can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Stable identifier for the rule, so tooling can group or suppress.
    pub rule: String,
    /// How much it matters.
    pub severity: Severity,
    /// What is wrong.
    pub message: String,
    /// 1-based line, or 0 when the finding is about the file as a whole.
    pub line: u32,
    /// 1-based column.
    pub column: u32,
    /// The source text the span covers, so a model need not re-derive it.
    pub source: String,
    /// A concrete edit, where the rule admits one.
    pub fix: Option<String>,
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}: {}: {} [{}]",
            self.line,
            self.column,
            self.severity.label(),
            self.message,
            self.rule
        )?;
        if let Some(fix) = &self.fix {
            write!(f, "\n    fix: {fix}")?;
        }
        Ok(())
    }
}

/// Everything S4 found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// In source order, then by severity.
    pub findings: Vec<Finding>,
}

impl Report {
    /// Whether the file passed.
    pub fn is_ok(&self) -> bool {
        !self.findings.iter().any(|f| f.severity == Severity::Error)
    }

    /// The JSON form, for a caller that will hand it to a model.
    pub fn to_json(&self) -> Json {
        let findings: Vec<Json> = self
            .findings
            .iter()
            .map(|f| {
                let mut m: BTreeMap<String, Json> = BTreeMap::from([
                    ("rule".into(), Json::String(f.rule.clone())),
                    ("severity".into(), Json::String(f.severity.label().into())),
                    ("message".into(), Json::String(f.message.clone())),
                    ("line".into(), Json::Number(f.line.to_string())),
                    ("column".into(), Json::Number(f.column.to_string())),
                    ("source".into(), Json::String(f.source.clone())),
                ]);
                if let Some(fix) = &f.fix {
                    m.insert("fix".into(), Json::String(fix.clone()));
                }
                Json::Object(m)
            })
            .collect();
        Json::Object(BTreeMap::from([
            ("ok".into(), Json::Bool(self.is_ok())),
            ("findings".into(), Json::Array(findings)),
        ]))
    }

    /// The text to hand back to a generator, verbatim.
    ///
    /// Ordered by rule rather than by line, and each rule stated once with its
    /// occurrences listed under it. This is the operating model in miniature:
    /// Phase 0's seven failures were one cause, and a list of seven line
    /// numbers invites seven separate patches while a list of one cause invites
    /// the fix. The grouping is the advice.
    pub fn repair_prompt(&self) -> String {
        if self.is_ok() {
            return "The IDL is valid. No changes are needed.".into();
        }
        let mut by_rule: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
        for f in self.findings.iter().filter(|f| f.severity == Severity::Error) {
            by_rule.entry(&f.rule).or_default().push(f);
        }

        let mut out = String::from(
            "The IDL was rejected. Fix every occurrence of each cause below and return the \
             whole file.\n",
        );
        for (rule, findings) in &by_rule {
            out.push_str(&format!(
                "\n[{rule}] {} occurrence(s)\n  {}\n",
                findings.len(),
                findings[0].message
            ));
            if let Some(fix) = &findings[0].fix {
                out.push_str(&format!("  {fix}\n"));
            }
            for f in findings {
                out.push_str(&format!("    line {}, column {}: {}\n", f.line, f.column, f.source));
            }
        }
        out
    }
}

/// Runs every in-process check over one file.
pub fn validate(src: &str) -> Report {
    let mut findings = Vec::new();

    match orbweaver_idl::check(src) {
        Err(diags) => {
            // Parsing and semantics have already run; nothing downstream can
            // say anything trustworthy about a file that failed them.
            findings.extend(diags.iter().map(|d| from_diagnostic(src, d)));
        }
        Ok(spec) => {
            let mut registry = Registry::new();
            if let Err(e) = registry.load(&spec) {
                findings.push(Finding {
                    rule: "registry".into(),
                    severity: Severity::Error,
                    message: e.message,
                    line: 0,
                    column: 0,
                    source: String::new(),
                    fix: None,
                });
            }
            findings.extend(wire_support(src, &spec));
            findings.extend(annotation_advice(src, &spec));
        }
    }

    findings.sort_by(|a, b| {
        b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)).then(a.column.cmp(&b.column))
    });
    Report { findings }
}

/// Compares against a released contract as well, so S4 covers §5.3.
///
/// A file can be perfectly valid and still be a change nobody may ship, and a
/// generator asked to "add a field" will produce exactly that. Reporting it
/// here rather than at release time is the difference between a regenerate and
/// an outage.
pub fn validate_against(src: &str, released: &str) -> Report {
    let mut report = validate(src);
    if !report.is_ok() {
        return report;
    }
    let (Ok(new_spec), Ok(old_spec)) = (orbweaver_idl::check(src), orbweaver_idl::check(released))
    else {
        return report;
    };
    let (mut old, mut new) = (Registry::new(), Registry::new());
    if old.load(&old_spec).is_err() || new.load(&new_spec).is_err() {
        return report;
    }
    for change in diff(&old, &new) {
        let severity = match change.verdict {
            Verdict::Breaking => Severity::Error,
            Verdict::ConditionallyBreaking => Severity::Warning,
            Verdict::ServerFirst => Severity::Advice,
            Verdict::Compatible => continue,
        };
        report.findings.push(Finding {
            rule: format!("evolution/{}", change.verdict.label().replace(' ', "-")),
            severity,
            message: format!("{}: {} — {}", change.id, change.what, change.why),
            line: 0,
            column: 0,
            source: change.id.clone(),
            fix: match change.verdict {
                Verdict::Breaking => Some(
                    "publish a new version of the interface instead of editing the released \
                     type in place (docs/PLAN.md §5.3)"
                        .into(),
                ),
                _ => None,
            },
        });
    }
    report.findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)));
    report
}

fn from_diagnostic(src: &str, d: &Diagnostic) -> Finding {
    Finding {
        rule: d.rule.to_owned(),
        severity: Severity::Error,
        message: d.message.clone(),
        line: d.span.line,
        column: d.span.column,
        source: src.get(d.span.start..d.span.end).unwrap_or_default().to_owned(),
        fix: fix_for(d, src),
    }
}

/// The concrete edit, for the rules that admit one.
///
/// Only where the fix is unambiguous. A hint that guesses wrong is worse than
/// none: the loop feeds it back verbatim, and a confident wrong instruction is
/// what turns one round into three.
fn fix_for(d: &Diagnostic, src: &str) -> Option<String> {
    let text = src.get(d.span.start..d.span.end).unwrap_or_default();
    match d.rule {
        // The dominant failure of this whole project, measured in Phase 0 and
        // met five times since. The fix is always the same shape, so it is
        // always worth spelling out.
        "identifier-case-clash" | "enclosing-scope-clash" | "inherited-clash" => Some(format!(
            "IDL identifiers collide case-insensitively (CORBA 3.4 §7.2.3). Rename {text:?} \
             to something that does not match a type or an enclosing scope ignoring case — \
             `the_{text}`, `{text}_value` or a domain word all work. Renaming the *type* is \
             usually wrong: it is the one other files refer to."
        )),
        "unknown-name" => Some(format!(
            "{text:?} is not declared anywhere in scope. Declare it, qualify it with its \
             module (`Module::{text}`), or correct the spelling."
        )),
        "not-a-type" => Some(format!(
            "{text:?} names something that is not a type. A constant or an operation cannot \
             be used where a type is expected."
        )),
        "duplicate-declaration" => {
            Some(format!("{text:?} is already declared in this scope; remove one or rename it."))
        }
        "duplicate-union-label" => {
            Some("Each union case label may appear once; remove the repeat.".into())
        }
        "duplicate-union-default" => Some("A union has at most one `default:` branch.".into()),
        // Escaping is the whole fix and IDL 4 §7.2.3.1 defines it precisely, so
        // this is one of the few parse failures worth a hint.
        "reserved-word" => Some(format!(
            "Prefix it: `_{text}`. IDL reserves the word, and the leading underscore is the \
             escape the specification defines — it is not part of the name and does not appear \
             in the repository id. Renaming works too."
        )),
        // Every other parse failure keeps quiet. The cause is wherever the
        // grammar broke, which is not reliably where the token is, and a
        // confident wrong hint costs a self-repair round.
        _ => None,
    }
}

/// Things that compile and will not work on this project's wire (§4.4).
fn wire_support(src: &str, spec: &orbweaver_idl::ast::Spec) -> Vec<Finding> {
    let mut out = Vec::new();
    walk(&spec.definitions, &mut |def| {
        if let orbweaver_idl::ast::Definition::ValueType(v) = def {
            out.push(Finding {
                rule: "wire/valuetype".into(),
                severity: Severity::Warning,
                message: format!(
                    "valuetype {:?} parses but v1 cannot marshal it (docs/PLAN.md §4.4)",
                    v.name.text
                ),
                line: v.name.span.line,
                column: v.name.span.column,
                source: src.get(v.name.span.start..v.name.span.end).unwrap_or_default().to_owned(),
                fix: Some(
                    "model the data as a struct, or keep the valuetype out of any operation \
                     signature until v2"
                        .into(),
                ),
            });
        }
    });
    out
}

/// What a contract needs before an agent can use it (§2.2).
///
/// Advice, never an error: an unannotated interface is perfectly valid CORBA.
/// It is simply unusable by something that has never seen it, which is the
/// whole point of the pipeline, so the gate says so.
fn annotation_advice(src: &str, spec: &orbweaver_idl::ast::Spec) -> Vec<Finding> {
    let mut out = Vec::new();
    walk(&spec.definitions, &mut |def| {
        let orbweaver_idl::ast::Definition::Interface(i) = def else { return };
        // A forward declaration has no body and nothing to annotate.
        let Some(body) = &i.body else { return };
        for op in body.iter().filter_map(|m| match m {
            orbweaver_idl::ast::InterfaceMember::Operation(op) => Some(op),
            _ => None,
        }) {
            if !op.annotations.iter().any(|a| a.key == "ai_desc") {
                out.push(Finding {
                    rule: "sidl/missing-ai_desc".into(),
                    severity: Severity::Advice,
                    message: format!(
                        "{}.{} has no ai_desc, so an agent has only the name to go on",
                        i.name.text, op.name.text
                    ),
                    line: op.name.span.line,
                    column: op.name.span.column,
                    source: src
                        .get(op.name.span.start..op.name.span.end)
                        .unwrap_or_default()
                        .to_owned(),
                    fix: Some(format!(
                        "add `//@ ai_desc: <what {} does, in one sentence>` above it",
                        op.name.text
                    )),
                });
            }
            if !op.annotations.iter().any(|a| a.key == "ai_effect") {
                out.push(Finding {
                    rule: "sidl/missing-ai_effect".into(),
                    severity: Severity::Advice,
                    message: format!(
                        "{}.{} has no ai_effect, so the bridge must assume it needs approval",
                        i.name.text, op.name.text
                    ),
                    line: op.name.span.line,
                    column: op.name.span.column,
                    source: src
                        .get(op.name.span.start..op.name.span.end)
                        .unwrap_or_default()
                        .to_owned(),
                    fix: Some(
                        "add `//@ ai_effect: read_only`, `idempotent` or `destructive`".into(),
                    ),
                });
            }
        }
    });
    out
}

fn walk(
    defs: &[orbweaver_idl::ast::Definition],
    f: &mut impl FnMut(&orbweaver_idl::ast::Definition),
) {
    for d in defs {
        f(d);
        if let orbweaver_idl::ast::Definition::Module(m) = d {
            walk(&m.definitions, f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(src: &str) -> Vec<String> {
        validate(src).findings.iter().map(|f| f.rule.clone()).collect()
    }

    #[test]
    fn a_clean_file_passes_with_nothing_to_say_beyond_advice() {
        let r = validate(
            "module m {
               //@ ai_desc: Adds two numbers
               //@ ai_effect: read_only
               interface Calc { long add(in long a, in long b); };
             };",
        );
        assert!(r.is_ok(), "{:?}", r.findings);
        assert_eq!(r.repair_prompt(), "The IDL is valid. No changes are needed.");
    }

    /// The dominant failure. The message must name the identifier and the fix
    /// must be an edit, because this is the one the loop will see most.
    #[test]
    fn the_case_clash_finding_says_exactly_what_to_change() {
        let r = validate(
            "module m { struct Position { double x; }; struct T { Position position; }; };",
        );
        assert!(!r.is_ok());
        let f = r.findings.iter().find(|f| f.rule.contains("clash")).expect("clash reported");
        assert_eq!(f.source, "position", "the span must cover the offending name");
        let fix = f.fix.as_deref().expect("a fix");
        assert!(fix.contains("the_position") || fix.contains("position_value"), "{fix}");
        // And it must steer away from the wrong fix, which is renaming the type
        // every other file refers to.
        assert!(fix.contains("Renaming the *type* is usually wrong"), "{fix}");
    }

    #[test]
    fn every_semantic_rule_reaches_the_report() {
        for (src, rule) in [
            ("module m { struct S { long a; long a; }; };", "duplicate-declaration"),
            ("module m { struct S { Missing a; }; };", "unknown-name"),
            (
                "module m { union U switch (long) { case 1: long a; case 1: long b; }; };",
                "duplicate-union-label",
            ),
        ] {
            assert!(rules(src).iter().any(|r| r == rule), "{src} should report {rule}");
        }
    }

    /// The grouping is the advice: Phase 0's seven failures were one cause, and
    /// a list of seven line numbers invites seven patches.
    #[test]
    fn the_repair_prompt_groups_by_cause_not_by_line() {
        let src = "module m {
             struct Position { double x; };
             struct Value { long v; };
             struct A { Position position; };
             struct B { Value value; };
           };";
        let r = validate(src);
        let prompt = r.repair_prompt();
        assert_eq!(
            prompt.matches("[identifier-case-clash]").count(),
            1,
            "the cause is stated once:\n{prompt}"
        );
        assert!(prompt.contains("2 occurrence(s)"), "{prompt}");
        assert!(prompt.contains("position"), "{prompt}");
        assert!(prompt.contains("value"), "{prompt}");
    }

    #[test]
    fn the_json_form_is_machine_readable_and_round_trips() {
        let r = validate("module m { struct S { Missing a; }; };");
        let j = r.to_json();
        let text = j.to_string();
        assert_eq!(Json::parse(&text).unwrap(), j);
        assert_eq!(j.get("ok"), Some(&Json::Bool(false)));
        let Some(Json::Array(fs)) = j.get("findings") else { panic!("{text}") };
        assert!(fs[0].get("fix").is_some(), "{text}");
        assert!(fs[0].get("line").is_some(), "{text}");
    }

    /// A valuetype compiles and cannot go on this project's wire. Warning, not
    /// error: it is legal IDL, and the file may exist for a v2 that can.
    #[test]
    fn a_valuetype_is_a_warning_that_names_the_limit() {
        let r = validate("module m { valuetype V { public long a; }; };");
        assert!(r.is_ok(), "still valid IDL");
        let f = r.findings.iter().find(|f| f.rule == "wire/valuetype").expect("warned");
        assert_eq!(f.severity, Severity::Warning);
        assert!(f.message.contains("§4.4"));
    }

    /// Missing annotations are advice, because unannotated IDL is valid CORBA
    /// and merely useless to an agent.
    #[test]
    fn missing_sidl_annotations_are_advice_and_never_block() {
        let r = validate("module m { interface I { long f(); }; };");
        assert!(r.is_ok());
        assert!(
            rules("module m { interface I { long f(); }; };")
                .iter()
                .any(|x| x == "sidl/missing-ai_desc")
        );
        assert!(r.findings.iter().all(|f| f.severity != Severity::Error));
    }

    /// S4 covers §5.3 too: a file can be valid and still be a change nobody may
    /// ship, and "add a field" is exactly what a generator will do.
    #[test]
    fn a_breaking_change_against_a_released_contract_is_an_error() {
        let released = "module m { struct S { long a; }; };";
        let proposed = "module m { struct S { long a; long b; }; };";
        let r = validate_against(proposed, released);
        assert!(!r.is_ok(), "{:?}", r.findings);
        let f = r.findings.iter().find(|f| f.rule.starts_with("evolution/")).expect("reported");
        assert_eq!(f.severity, Severity::Error);
        assert!(f.fix.as_deref().unwrap().contains("new version"), "{f:?}");
    }

    #[test]
    fn an_additive_change_is_advice_rather_than_a_refusal() {
        let released = "module m { interface I { long a(); }; };";
        let proposed = "module m { interface I { long a(); long b(); }; };";
        let r = validate_against(proposed, released);
        assert!(r.is_ok(), "{:?}", r.findings);
        assert!(r.findings.iter().any(|f| f.rule == "evolution/server-first"));
    }

    /// A file that does not parse must not also be reported against a baseline:
    /// the diff would be nonsense and would bury the real cause.
    #[test]
    fn a_broken_file_reports_only_why_it_is_broken() {
        let r = validate_against(
            "module m { struct S { long a }",
            "module m { struct S { long a; }; };",
        );
        assert!(!r.is_ok());
        assert!(r.findings.iter().all(|f| !f.rule.starts_with("evolution/")), "{:?}", r.findings);
    }
}
