//! S2 — synthesize: a brief in, an IDL draft out.
//!
//! Unchanged in spirit from the single-prompt pipeline that came before, with
//! one difference that is the whole point of the split: **S2 is handed a
//! [`Brief`] rather than prose.** It no longer decides what the requirement
//! means; it decides how what the requirement means becomes IDL — which types
//! carry which shapes, which operations live on which interface, what the
//! module is called. Those are decisions a compiler can judge, which is why
//! this is the stage the S4 gate sits closest to.
//!
//! # S2's own gate
//!
//! [`gate`] is [`crate::validate`] plus two questions only this stage can ask,
//! because only this stage has both the brief and the IDL in front of it:
//!
//! - **Is anything callable?** A file of types with no interface satisfies
//!   every syntax rule and answers no requirement.
//! - **Did the brief survive?** An operation or entity S1 recorded and S2
//!   dropped is invisible to every syntactic check — the IDL is *valid*, it is
//!   simply not the contract that was asked for. Reported as a warning, never
//!   an error: renaming `조회` to `find_by_email` is correct behaviour and a
//!   rule that fires on correct behaviour is a rule people learn to skim.
//!
//! **S2는 요구사항을 해석하지 않는다. 브리프를 IDL로 옮긴다.** 이 단계의
//! 게이트는 S4의 검사에 "호출 가능한 것이 있는가"와 "브리프가 살아남았는가"를
//! 더한 것이다.

use orbweaver_idl::ast::{Definition, Interface, InterfaceMember};

use crate::ingest::Brief;
use crate::{Finding, Report, Severity, validate};

/// The synthesis constraints, quoted into the S2 prompt verbatim.
///
/// These mirror CLAUDE.md's "IDL rules the compiler enforces". They are stated
/// here rather than only in a shell wrapper so that the prompt a measurement
/// used is a versioned artifact of this crate: per the 2026-08-13 run record,
/// *a first-pass rate is meaningless without the prompt that produced it*.
pub const S2_PROMPT: &str = "\
You are writing OMG IDL 4.2 for a CORBA system from the STRUCTURED BRIEF below.
Output the complete IDL file text and NOTHING else: no markdown fences, no
commentary, no explanation before or after.

Rules this project's compiler enforces:
- Identifier clashes are case-insensitive. A member, parameter or operation may
  not share a name with a type or an enclosing scope, ignoring case.
  'Position position', 'Value value', 'module inventory { interface Inventory }'
  and 'struct Version { unsigned long version; }' are all illegal. This is
  natural naming in every other language, which is exactly why it is the
  dominant generation failure.
- TypeCode, if used, must be qualified as ::CORBA::TypeCode.
- Do not use valuetype, abstract interfaces, or fixed — this wire does not
  support them.
- Declare at least one interface: the brief's operations are what a caller
  calls.
- Cover the brief. Every operation and entity in it should be reachable in the
  IDL; if you deliberately merge or rename one, keep the meaning.
- Do NOT write //@ ai_* annotations. A separate annotation stage runs after you
  and is measured separately; annotations written here are that stage's work
  done where nobody checks it.

Write the types the brief's shapes describe — the shapes are in plain words on
purpose, and choosing the IDL type is your decision, not the brief's.
";

fn finding(rule: &str, severity: Severity, message: String, source: String, fix: &str) -> Finding {
    Finding {
        rule: rule.to_owned(),
        severity,
        message,
        line: 0,
        column: 0,
        source,
        fix: Some(fix.to_owned()),
    }
}

/// S2's gate: S4's checks, plus what only the brief can tell us.
///
/// `input` is S2's input artifact — the brief JSON when S1 ran, or the raw
/// requirement text when it did not. Prose input simply skips the coverage
/// half, and the report says so by not carrying those findings.
pub fn gate(input: &str, idl: &str) -> Report {
    let mut report = validate(idl);
    // A file that does not parse has nothing to compare against a brief, and
    // the parse error is the only finding worth reading.
    if !report.is_ok() {
        return report;
    }
    let Ok(spec) = orbweaver_idl::check(idl) else { return report };

    let mut interfaces: Vec<&Interface> = Vec::new();
    collect_interfaces(&spec.definitions, &mut interfaces);
    if interfaces.iter().all(|i| i.body.is_none()) {
        report.findings.push(finding(
            "s2/no-interface",
            Severity::Error,
            "the draft declares no interface with a body, so nothing in it can be called".into(),
            String::new(),
            "put the brief's operations on an interface — types alone answer no requirement",
        ));
    }

    if let Ok(brief) = Brief::parse(input) {
        report.findings.extend(coverage(&brief, &spec, &interfaces));
    }

    report.findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)));
    report
}

fn collect_interfaces<'a>(defs: &'a [Definition], out: &mut Vec<&'a Interface>) {
    for d in defs {
        match d {
            Definition::Interface(i) => out.push(i),
            Definition::Module(m) => collect_interfaces(&m.definitions, out),
            _ => {}
        }
    }
}

/// Names an IDL identifier could plausibly be, compared the way IDL compares
/// them: case-insensitively, and ignoring the underscores a renaming adds.
fn normalized(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()
}

/// Whether a brief name is one an IDL identifier could be compared with.
///
/// A brief may legitimately name an operation in Korean; the IDL will not, and
/// reporting every such rename would make this rule fire on correct output,
/// which is how a report gets ignored.
fn comparable(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ')
}

fn coverage(
    brief: &Brief,
    spec: &orbweaver_idl::ast::Spec,
    interfaces: &[&Interface],
) -> Vec<Finding> {
    let mut op_names: Vec<String> = Vec::new();
    for i in interfaces {
        for member in i.body.iter().flatten() {
            match member {
                InterfaceMember::Operation(op) => op_names.push(normalized(&op.name.text)),
                InterfaceMember::Attribute(a) => {
                    op_names.extend(a.names.iter().map(|n| normalized(&n.text)))
                }
                InterfaceMember::Nested(_) => {}
            }
        }
    }
    let mut type_names: Vec<String> = Vec::new();
    collect_type_names(&spec.definitions, &mut type_names);

    let mut out = Vec::new();
    for op in brief.operations.iter().filter(|o| comparable(&o.name)) {
        let want = normalized(&op.name);
        if !want.is_empty() && !op_names.iter().any(|got| got.contains(&want) || want.contains(got))
        {
            out.push(finding(
                "s2/operation-missing",
                Severity::Warning,
                format!(
                    "the brief asks for {:?} and no operation of that name is in the draft; the \
                     IDL is valid and is not the contract that was requested",
                    op.name
                ),
                op.name.clone(),
                "add the operation, or — if it was deliberately merged into another — say so in \
                 the brief so the next reader is not comparing two documents by hand",
            ));
        }
    }
    for e in brief.entities.iter().filter(|e| comparable(&e.name)) {
        let want = normalized(&e.name);
        if !want.is_empty()
            && !type_names.iter().any(|got| got.contains(&want) || want.contains(got))
        {
            out.push(finding(
                "s2/entity-missing",
                Severity::Warning,
                format!(
                    "the brief describes the entity {:?} and no type of that name is in the \
                     draft; its fields are unreachable",
                    e.name
                ),
                e.name.clone(),
                "declare it, or fold its fields into a type that carries them and rename it in \
                 the brief",
            ));
        }
    }
    out
}

fn collect_type_names(defs: &[Definition], out: &mut Vec<String>) {
    for d in defs {
        match d {
            Definition::Module(m) => {
                out.push(normalized(&m.name.text));
                collect_type_names(&m.definitions, out);
            }
            Definition::Interface(i) => {
                out.push(normalized(&i.name.text));
                for member in i.body.iter().flatten() {
                    if let InterfaceMember::Nested(nested) = member {
                        collect_type_names(std::slice::from_ref(nested), out);
                    }
                }
            }
            other => out.push(normalized(&other.name().text)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::{Entity, Field, OperationSketch};

    fn brief() -> Brief {
        Brief {
            requirement: "재고 수량을 증감한다".into(),
            summary: "inventory levels".into(),
            entities: vec![Entity {
                name: "Item".into(),
                description: "a stocked item".into(),
                fields: vec![Field {
                    name: "sku".into(),
                    shape: "code".into(),
                    ..Field::default()
                }],
            }],
            operations: vec![OperationSketch {
                name: "adjust".into(),
                ..OperationSketch::default()
            }],
            constraints: vec![],
            open_questions: vec![],
        }
    }

    const COVERING: &str = "module inv {
         struct Item { string sku; };
         interface Levels { long adjust(in string the_sku, in long delta); };
       };";

    #[test]
    fn a_draft_that_covers_its_brief_is_clean() {
        let r = gate(&brief().to_json().to_string(), COVERING);
        assert!(r.is_ok(), "{:?}", r.findings);
        assert!(
            r.findings.iter().all(|f| f.severity == Severity::Advice),
            "only the missing-annotation advice, which S3 answers: {:?}",
            r.findings
        );
    }

    /// Types with no interface satisfy every syntax rule and answer no
    /// requirement.
    #[test]
    fn a_draft_with_nothing_callable_is_rejected() {
        let r =
            gate(&brief().to_json().to_string(), "module inv { struct Item { string sku; }; };");
        assert!(!r.is_ok());
        assert_eq!(r.findings[0].rule, "s2/no-interface");
    }

    /// A dropped operation is invisible to every syntactic check: the file is
    /// valid and is not what was asked for.
    #[test]
    fn a_dropped_operation_is_a_warning_that_names_it() {
        let r = gate(
            &brief().to_json().to_string(),
            "module inv { struct Item { string sku; }; interface Levels { long total(); }; };",
        );
        assert!(r.is_ok(), "valid IDL, wrong contract — a warning, not a refusal");
        let f = r.findings.iter().find(|f| f.rule == "s2/operation-missing").expect("warned");
        assert!(f.message.contains("adjust"), "{}", f.message);
    }

    #[test]
    fn a_dropped_entity_is_a_warning_that_names_it() {
        let r = gate(
            &brief().to_json().to_string(),
            "module inv { interface Levels { long adjust(in string the_sku); }; };",
        );
        let f = r.findings.iter().find(|f| f.rule == "s2/entity-missing").expect("warned");
        assert!(f.message.contains("Item"), "{}", f.message);
    }

    /// The rule must stay quiet when the brief names things in the language the
    /// requirements are written in: an IDL identifier cannot be `조회`, so a
    /// rename there is correct behaviour and not a finding.
    #[test]
    fn a_korean_operation_name_is_not_compared_against_an_ascii_identifier() {
        let mut b = brief();
        b.operations[0].name = "조회".into();
        b.entities[0].name = "품목".into();
        let r = gate(&b.to_json().to_string(), COVERING);
        assert!(
            r.findings.iter().all(|f| !f.rule.starts_with("s2/")),
            "no coverage complaint is possible here: {:?}",
            r.findings
        );
    }

    /// Prose input (S1 skipped) simply has no coverage half — and the syntactic
    /// half still runs.
    #[test]
    fn prose_input_keeps_the_syntactic_gate_and_drops_the_coverage_gate() {
        let r = gate("재고 수량을 증감한다", COVERING);
        assert!(r.is_ok());
        assert!(r.findings.iter().all(|f| !f.rule.starts_with("s2/")), "{:?}", r.findings);

        let broken = gate("재고 수량을 증감한다", "module m { struct S { long a };");
        assert!(!broken.is_ok(), "S4's checks still apply without a brief");
    }

    /// S4's own findings are not lost when S2's are added.
    #[test]
    fn the_dominant_root_cause_still_reaches_the_report_through_s2s_gate() {
        let r = gate(
            &brief().to_json().to_string(),
            "module m { struct Position { double x; }; interface I { void go(in Position position); }; };",
        );
        assert!(!r.is_ok());
        assert!(r.findings.iter().any(|f| f.rule.contains("clash")), "{:?}", r.findings);
    }

    /// The codify rule for this stage: the prompt must forbid what S3 owns, or
    /// the split is undone by the prompt on the first run.
    #[test]
    fn the_prompt_hands_annotations_to_s3_and_names_the_dominant_cause() {
        assert!(S2_PROMPT.contains("Do NOT write //@ ai_*"), "{S2_PROMPT}");
        assert!(S2_PROMPT.contains("case-insensitive"), "{S2_PROMPT}");
        assert!(S2_PROMPT.contains("Declare at least one interface"), "{S2_PROMPT}");
    }
}
