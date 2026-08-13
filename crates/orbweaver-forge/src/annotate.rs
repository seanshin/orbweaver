//! S3 — annotate: an IDL draft in, SIDL out, **measured on its own**.
//!
//! # Why this is a stage and not a clause in S2's prompt
//!
//! It was a clause in S2's prompt, and the corpus says what that produced.
//! The first run of `contract-check` — the S7 checker in `orbweaver-test`,
//! which reads the same SIDL vocabulary the MCP policy gate and the guard read
//! — over our own `corpus/golden/` returned **20 findings** (commit `0ca5b28`
//! repaired them down to 4, and those 4 are honest coverage gaps rather than
//! contract defects). The dominant class was **operations that change state
//! carrying no `ai_authz` scope**. Those files were written by people and by
//! models who had all been told about `ai_authz` in the same breath as
//! everything else, and the annotation is the part that got dropped, in exactly
//! the way a secondary instruction in a long prompt gets dropped.
//!
//! That is the failure a stage which is "part of another prompt" produces, and
//! it is not a failure a syntax gate can catch: an unannotated operation is
//! *valid CORBA*. S4 says so — `sidl/missing-ai_desc` is Advice there, and
//! correctly, because unannotated IDL compiles. It is only useless. So the
//! missing annotation has to be an error **somewhere**, and the only place it
//! can be an error without S4 refusing legal IDL is in the gate of the stage
//! whose entire job it is.
//!
//! Nor is it a failure a lint can finish. Two of that batch's twenty —
//! `moe::ExpertLoader::evict` among them, which destroys everything routed to
//! an expert — contain no mutating verb, so [`mutating_verb`] would never have
//! found them. A heuristic can catch the operation *named* `delete_account`; it
//! cannot catch the one named `evict`. Only something that reads the contract
//! can, which is why S3 is a model stage with a checker behind it rather than a
//! checker alone.
//!
//! Hence: S3 has its own prompt ([`S3_PROMPT`]), its own gate ([`check`]), its
//! own first-pass rate and its own round count. When the batch report says S3
//! needed two rounds, that is a statement about annotation quality and nothing
//! else — which is the whole argument for splitting stages.
//!
//! # What S3 must always do
//!
//! [`RULES`] is that list, and it is load-bearing in two directions at once:
//! every entry names a constraint written into [`S3_PROMPT`] **and** a rule
//! [`check`] enforces. The test `every_rule_is_a_prompt_constraint_and_a_check`
//! fails if either half is missing, so the corpus finding above cannot come
//! back as a silent regression: it would have to come back as a red test.
//!
//! # S3 annotates; it does not redesign
//!
//! [`check_against`] compares the contract before and after. An annotator that
//! "improves" a signature while it is in there has changed what ships, and the
//! change would be attributed to S2 by every reader of the diff. Structured
//! comments in, nothing else.
//!
//! **S3은 별도 단계다.** 우리 코퍼스를 `contract-check`로 재면 20건이 나왔고
//! 지배적 원인은 **변경 연산에 스코프(`ai_authz`)가 없는 것**이었다. 이는 다른
//! 프롬프트에 얹힌 단계가 만드는 전형적 실패이며, 구문 게이트로는 잡을 수 없다
//! (주석 없는 IDL도 적법하다). 따라서 S3 자신의 게이트에서 오류로 잡는다.

use std::collections::BTreeMap;

use orbweaver_idl::ast::{
    Definition, Interface, InterfaceMember, Operation, Param, ScopedName, Spec, TypeSpec,
};
use orbweaver_registry::{Entry, Registry};

use crate::{Finding, Report, Severity, validate};

/// One thing S3 must always do.
///
/// Both halves are mandatory. A constraint only in the prompt is a request; a
/// rule only in the checker is a surprise. The pair is what makes a confirmed
/// root cause unable to recur silently (CLAUDE.md, codify).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    /// The rule name [`check`] reports under.
    pub id: &'static str,
    /// What it demands, in one line — for a human reading the roster.
    pub demand: &'static str,
    /// A phrase [`S3_PROMPT`] must contain, so prompt and checker cannot drift.
    pub prompt_phrase: &'static str,
}

/// Everything S3 must always do, as prompt constraints and as checks.
pub const RULES: [Rule; 11] = [
    Rule {
        id: "s3/missing-ai_desc",
        demand: "every operation carries an ai_desc",
        prompt_phrase: "ai_desc: one sentence",
    },
    Rule {
        id: "s3/missing-ai_effect",
        demand: "every operation carries an ai_effect",
        prompt_phrase: "ai_effect: read_only, idempotent or destructive",
    },
    Rule {
        id: "s3/effect-unknown",
        demand: "ai_effect uses a value the policy gate recognises",
        prompt_phrase: "treated as needing human approval",
    },
    Rule {
        id: "s3/missing-ai_authz",
        demand: "every operation that changes state, hands out a reference or takes high PII \
                 carries an ai_authz scope",
        prompt_phrase: "MUST carry //@ ai_authz",
    },
    Rule {
        id: "s3/read-only-mutating-name",
        demand: "a read_only claim is not contradicted by the operation's own name",
        prompt_phrase: "do not annotate read_only on an operation whose name says it changes",
    },
    Rule {
        id: "s3/authz-on-interface-only",
        demand: "scopes are written per operation, because that is where the guard reads them",
        prompt_phrase: "per operation, never on the interface",
    },
    Rule {
        id: "s3/unknown-annotation",
        demand: "only the SIDL v1 vocabulary is used",
        prompt_phrase: "The whole vocabulary is",
    },
    Rule {
        id: "s3/pii-level-unknown",
        demand: "ai_pii is none, low or high",
        prompt_phrase: "ai_pii: none, low or high",
    },
    Rule {
        id: "s3/idempotent-not-boolean",
        demand: "ai_idempotent is true or false",
        prompt_phrase: "ai_idempotent: true or false",
    },
    Rule {
        id: "s3/oneway-not-idempotent",
        demand: "a oneway operation is not also declared unsafe to retry",
        prompt_phrase: "never ai_idempotent: false on a oneway",
    },
    Rule {
        id: "s3/contract-changed",
        demand: "the IDL that comes out is the IDL that went in, plus comments",
        prompt_phrase: "Do not change the IDL",
    },
];

/// The annotation constraints, quoted into the S3 prompt verbatim.
///
/// Every [`RULES`] entry's `prompt_phrase` appears below; the test pins it.
pub const S3_PROMPT: &str = "\
You are adding AI metadata to an IDL file that is already correct. Output the
complete file text with the annotations added and NOTHING else: no markdown
fences, no commentary.

Do not change the IDL. Do not rename anything, do not add or remove operations,
parameters, members or types, do not change a type. Your entire output is the
input plus structured comments. A signature you improve here is a change nobody
reviewed.

Annotations are structured comments — '//@ key: value' on the lines directly
above what they describe. Do NOT use IDL 4 '@annotation' syntax; deployed
compilers reject it.

The whole vocabulary is: ai_desc, ai_effect, ai_authz, ai_pii, ai_unit,
ai_idempotent, ai_example, ai_precond. Any other key is kept by the registry
and read by nobody, so a typo is an annotation that exists only in the source.

Every operation gets, at minimum:
  //@ ai_desc: one sentence saying what it does, for a reader who has never
      seen this interface
  //@ ai_effect: read_only, idempotent or destructive — any other value is
      treated as needing human approval, so a typo gates the operation by
      accident rather than by intent

Every operation that changes state, hands out an object reference, or takes a
parameter marked ai_pii high MUST carry //@ ai_authz: <scope> naming the
permission a caller needs. This is the one that gets dropped: a measurement of
this project's own corpus found 20 contract findings and the dominant class was
mutating operations with no scope. An operation with no ai_authz requires no
permission — the guard lets any caller who reaches the bridge call it.

Write ai_authz per operation, never on the interface. The guard reads the
operation's annotations and never the interface's, so an interface-level scope
looks exactly like an enforced one and enforces nothing.

Also:
- do not annotate read_only on an operation whose name says it changes
  something ('delete', 'update', 'transfer', 'create'). The policy gate
  believes the annotation, not the name.
- ai_pii: none, low or high, on parameters carrying personal data.
- ai_unit: the unit of a numeric parameter (KRW, metres, seconds).
- ai_idempotent: true or false, where retry-safety is worth stating — but
  never ai_idempotent: false on a oneway operation. A oneway has no reply, so
  the caller cannot learn whether the call arrived; declaring retry unsafe as
  well leaves a client that lost its connection with no correct move at all.
";

/// Every `ai_*` key SIDL v1 defines (§2.2).
///
/// Mirrored from `orbweaver-test`'s `contract::VOCABULARY`, which is the S7
/// authority. It is duplicated rather than imported because that crate depends
/// on this one and the dependency may not be reversed; the test
/// `the_vocabulary_matches_the_prompt` pins the copy against the prompt, and
/// any drift shows up as a `contract/unknown-annotation` on S3's output the
/// first time it is measured by the real checker.
pub const VOCABULARY: [&str; 8] = [
    "ai_desc",
    "ai_unit",
    "ai_effect",
    "ai_idempotent",
    "ai_pii",
    "ai_example",
    "ai_precond",
    "ai_authz",
];

/// `ai_effect` values the MCP policy gate treats as needing no approval.
///
/// Mirrored from `orbweaver-mcp`'s `policy::destructive_effect` by way of
/// `orbweaver-test`'s `contract::UNGATED_EFFECTS`.
pub const UNGATED_EFFECTS: [&str; 4] = ["read_only", "readonly", "idempotent", "safe"];

/// The subset of [`UNGATED_EFFECTS`] claiming the operation *only reads*.
///
/// Not the same set: `idempotent` is a claim about repetition and an ordinary
/// thing to say about an operation that writes. Conflating the two was
/// `orbweaver-test`'s first false positive and is not repeated here.
pub const READ_ONLY_EFFECTS: [&str; 3] = ["read_only", "readonly", "safe"];

/// The one `ai_effect` value that means "needs a human" on purpose.
pub const GATED_EFFECTS: [&str; 1] = ["destructive"];

/// `ai_pii` levels §2.2 defines.
pub const PII_LEVELS: [&str; 3] = ["none", "low", "high"];

/// Verbs in an operation name that suggest it changes something.
///
/// Mirrored from `orbweaver-test`'s list. Read-side verbs (`get`, `find`,
/// `list`, `query`) are deliberately absent: their absence is what keeps the
/// rule quiet over the read half of a normal interface.
const MUTATING_VERBS: [&str; 26] = [
    "set_",
    "create",
    "delete",
    "remove",
    "update",
    "write",
    "insert",
    "drop",
    "purge",
    "reset",
    "shutdown",
    "commit",
    "transfer",
    "wipe",
    "clear",
    "store",
    "register",
    "unregister",
    "bind",
    "unbind",
    "rebind",
    "activate",
    "deactivate",
    "destroy",
    "kill",
    "revoke",
];

fn finding(
    rule: &str,
    severity: Severity,
    message: String,
    at: &orbweaver_idl::lex::Span,
    source: String,
    fix: &str,
) -> Finding {
    Finding {
        rule: rule.to_owned(),
        severity,
        message,
        line: at.line,
        column: at.column,
        source,
        fix: Some(fix.to_owned()),
    }
}

/// The mutation verb an operation name contains, if any.
fn mutating_verb(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    MUTATING_VERBS.iter().copied().find(|v| lower.contains(v))
}

/// Annotations as a map, last write winning — the same flattening the registry
/// performs, so what is checked here is what a consumer will read.
fn annotations(list: &[orbweaver_idl::lex::Annotation]) -> BTreeMap<&str, &str> {
    list.iter().map(|a| (a.key.as_str(), a.value.trim())).collect()
}

/// S3's gate: the annotated file must still be valid IDL, and it must be
/// annotated.
///
/// [`crate::validate`] runs first and short-circuits: annotation findings over
/// a file that does not parse would be noise on top of the real cause.
pub fn gate(idl: &str) -> Report {
    let report = validate(idl);
    if !report.is_ok() {
        return report;
    }
    let mut merged = report;
    merged.findings.extend(check(idl).findings);
    merged.findings.sort_by(|a, b| {
        b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)).then(a.column.cmp(&b.column))
    });
    merged
}

/// [`gate`] plus the contract-identity check against S3's own input.
///
/// The stage runner uses this; [`gate`] exists for a caller checking a file it
/// has no "before" for — a hand-written SIDL file, say.
pub fn check_against(before: &str, after: &str) -> Report {
    let mut report = gate(after);
    if !report.is_ok() {
        return report;
    }
    report.findings.extend(contract_changes(before, after));
    report
}

/// Every annotation check over one file.
///
/// Assumes the file parses; [`gate`] enforces that. Findings are ordered by
/// severity then position, matching S4.
pub fn check(idl: &str) -> Report {
    let Ok(spec) = orbweaver_idl::check(idl) else { return Report::default() };
    let index = TypeIndex::of(&spec);
    let mut findings = Vec::new();
    let mut interfaces: Vec<&Interface> = Vec::new();
    collect_interfaces(&spec.definitions, &mut interfaces);

    for iface in interfaces {
        let Some(body) = &iface.body else { continue };
        let iface_ann = annotations(&iface.annotations);
        findings.extend(unknown_keys(&iface_ann, &iface.name.span, &iface.name.text));

        let ops: Vec<&Operation> = body
            .iter()
            .filter_map(|m| match m {
                InterfaceMember::Operation(op) => Some(op),
                _ => None,
            })
            .collect();

        // The guard reads ai_authz from the operation and nowhere else
        // (orbweaver-mcp `policy::required_scopes`). A scope on the interface
        // looks in the source exactly like one that is enforced.
        if let Some(scope) = iface_ann.get("ai_authz") {
            let unscoped: Vec<&str> = ops
                .iter()
                .filter(|op| !annotations(&op.annotations).contains_key("ai_authz"))
                .map(|op| op.name.text.as_str())
                .collect();
            if !unscoped.is_empty() {
                findings.push(finding(
                    "s3/authz-on-interface-only",
                    Severity::Error,
                    format!(
                        "interface {} carries ai_authz {scope:?}, but the guard reads ai_authz \
                         per operation and never from the interface, so {} operation(s) are \
                         unscoped in practice: {}",
                        iface.name.text,
                        unscoped.len(),
                        unscoped.join(", ")
                    ),
                    &iface.name.span,
                    iface.name.text.clone(),
                    "repeat `//@ ai_authz: <scope>` above each operation that needs it; \
                     interface-level inheritance is not implemented and an unenforced scope reads \
                     like an enforced one",
                ));
            }
        }

        for op in ops {
            findings.extend(operation_findings(&index, &iface.name.text, op));
        }

        for member in body {
            if let InterfaceMember::Attribute(attr) = member {
                let ann = annotations(&attr.annotations);
                if let Some(first) = attr.names.first() {
                    findings.extend(unknown_keys(&ann, &first.span, &first.text));
                }
            }
        }
    }

    findings.sort_by(|a, b| {
        b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)).then(a.column.cmp(&b.column))
    });
    Report { findings }
}

fn operation_findings(index: &TypeIndex, iface: &str, op: &Operation) -> Vec<Finding> {
    let mut out = Vec::new();
    let at = &op.name.span;
    let name = op.name.text.as_str();
    let where_ = format!("{iface}.{name}");
    let ann = annotations(&op.annotations);
    out.extend(unknown_keys(&ann, at, name));

    if !ann.contains_key("ai_desc") {
        out.push(finding(
            "s3/missing-ai_desc",
            Severity::Error,
            format!("{where_} has no ai_desc, so an agent choosing it has only the name to go on"),
            at,
            name.to_owned(),
            "add `//@ ai_desc: <one sentence, for a reader who has never seen this interface>` \
             above it",
        ));
    }

    let effect = ann.get("ai_effect").copied();
    match effect {
        None => out.push(finding(
            "s3/missing-ai_effect",
            Severity::Error,
            format!(
                "{where_} has no ai_effect, so the MCP policy gate must assume it needs human \
                 approval"
            ),
            at,
            name.to_owned(),
            "add `//@ ai_effect: read_only`, `idempotent` or `destructive`",
        )),
        Some(e) if !UNGATED_EFFECTS.contains(&e) && !GATED_EFFECTS.contains(&e) => {
            out.push(finding(
                "s3/effect-unknown",
                Severity::Error,
                format!(
                    "{where_} declares ai_effect {e:?}, which is not in the vocabulary; the policy \
                     gate treats any unrecognised value as needing approval, so the operation is \
                     gated by accident rather than by intent"
                ),
                at,
                name.to_owned(),
                "use one of read_only, idempotent, safe or destructive",
            ));
        }
        Some(_) => {}
    }

    let reads_only = effect.is_some_and(|e| READ_ONLY_EFFECTS.contains(&e));
    let contradicted = reads_only && mutating_verb(name).is_some();
    if contradicted {
        let verb = mutating_verb(name).unwrap_or_default();
        out.push(finding(
            "s3/read-only-mutating-name",
            Severity::Error,
            format!(
                "{where_} is annotated {:?} but its name contains {verb:?}; the policy gate \
                 believes the annotation and will let an agent call it with no approval",
                effect.unwrap_or_default()
            ),
            at,
            name.to_owned(),
            "if it really does change state, annotate `destructive` and give it a scope; if the \
             name is misleading, rename it — an agent reads the name too",
        ));
    }

    if let Some(v) = ann.get("ai_idempotent") {
        if !v.eq_ignore_ascii_case("true") && !v.eq_ignore_ascii_case("false") {
            out.push(finding(
                "s3/idempotent-not-boolean",
                Severity::Error,
                format!(
                    "{where_} declares ai_idempotent {v:?}; §2.2 types it as a boolean, so \
                     anything else is read as neither true nor false and the claim is lost"
                ),
                at,
                name.to_owned(),
                "write `true` or `false`",
            ));
        } else if v.eq_ignore_ascii_case("false") && op.oneway {
            // Found by the 2026-08-14 batch's oracle rather than by design:
            // `contract-check` reported it on R13 and S3's gate had nothing to
            // say, which is a gate missing a rule its own output can break.
            out.push(finding(
                "s3/oneway-not-idempotent",
                Severity::Error,
                format!(
                    "{where_} is oneway and declares ai_idempotent false; the caller cannot learn \
                     whether the call arrived and cannot safely retry it, so a lost connection \
                     loses the request with no correct move left"
                ),
                at,
                name.to_owned(),
                "annotate `ai_idempotent: true` if a blind retry is in fact safe — for a log or \
                 metric sink it usually is — or say so in ai_desc and drop the claim rather than \
                 asserting the combination that leaves a caller stuck",
            ));
        }
    }

    // The dominant corpus finding, as an error in the gate of the stage that
    // owns it. One finding per operation, against the strongest evidence:
    // three findings saying "add ai_authz" to one operation is the item-by-item
    // report §5.1 exists to avoid.
    let has_authz = ann.get("ai_authz").is_some_and(|s| !s.is_empty());
    if !has_authz && let Some(why) = needs_authz(index, op, effect, contradicted) {
        out.push(finding(
            "s3/missing-ai_authz",
            Severity::Error,
            format!(
                "{where_} {why} and carries no ai_authz; the guard therefore requires no \
                 permission for it, so any caller who reaches the bridge may call it"
            ),
            at,
            name.to_owned(),
            "add `//@ ai_authz: <scope>` naming the permission a caller needs — approval and \
             authorization are different gates and ai_effect only answers the first",
        ));
    }

    for p in &op.params {
        let pann = annotations(&p.annotations);
        out.extend(unknown_keys(&pann, &p.name.span, &p.name.text));
        if let Some(level) = pann.get("ai_pii")
            && !PII_LEVELS.contains(level)
        {
            out.push(finding(
                "s3/pii-level-unknown",
                Severity::Error,
                format!(
                    "{where_} parameter {:?} declares ai_pii {level:?}; §2.2 defines none | low | \
                     high, and a level outside them is read by nobody",
                    p.name.text
                ),
                &p.name.span,
                p.name.text.clone(),
                "use one of none, low, high",
            ));
        }
        if pann.contains_key("ai_unit") && plainly_not_numeric(&p.ty) {
            out.push(finding(
                "s3/unit-on-non-numeric",
                Severity::Warning,
                format!(
                    "{where_} parameter {:?} carries ai_unit and its type is not numeric; a unit \
                     on a non-numeric type either means nothing or means the value is a number in \
                     disguise",
                    p.name.text
                ),
                &p.name.span,
                p.name.text.clone(),
                "give the parameter a numeric type so the unit describes something the wire \
                 carries, or drop the annotation",
            ));
        }
    }
    out
}

/// Why this operation needs a scope, or `None` if it does not.
///
/// Priority order is deliberate — the strongest evidence is reported, and only
/// one reason per operation.
fn needs_authz(
    index: &TypeIndex,
    op: &Operation,
    effect: Option<&str>,
    contradicted: bool,
) -> Option<String> {
    let name = op.name.text.as_str();
    if effect.is_some_and(|e| !UNGATED_EFFECTS.contains(&e)) {
        return Some(format!("declares ai_effect {:?}", effect.unwrap_or_default()));
    }
    // A read_only claim contradicted by the name is already reported; adding
    // "and it needs a scope" tells the author to fix the wrong thing first.
    if !contradicted && let Some(verb) = mutating_verb(name) {
        return Some(format!("has a name containing {verb:?}, so it changes something"));
    }
    if let Some(path) = escaping_reference(index, op) {
        return Some(format!(
            "hands out an object reference ({path}); a reference is a bearer address (§4.7), so \
             this widens what its caller can reach even if it changes nothing itself"
        ));
    }
    if op.params.iter().any(|p| annotations(&p.annotations).get("ai_pii") == Some(&"high")) {
        return Some("takes a parameter marked ai_pii high".to_owned());
    }
    None
}

fn unknown_keys(
    ann: &BTreeMap<&str, &str>,
    at: &orbweaver_idl::lex::Span,
    what: &str,
) -> Vec<Finding> {
    ann.keys()
        .filter(|k| k.starts_with("ai_") && !VOCABULARY.contains(k))
        .map(|k| {
            finding(
                "s3/unknown-annotation",
                Severity::Error,
                format!(
                    "{what} carries {k:?}, which is not in the SIDL v1 vocabulary; the registry \
                     keeps it and no consumer reads it, so the annotation is present in the \
                     source and absent from every decision"
                ),
                at,
                what.to_owned(),
                "use one of ai_desc, ai_unit, ai_effect, ai_idempotent, ai_pii, ai_example, \
                 ai_precond, ai_authz",
            )
        })
        .collect()
}

/// File-scope type names, so an object reference can be told from data.
///
/// Simple names only: within one generated file, a scoped name's last segment
/// identifies it, and the alternative is re-implementing the registry's scope
/// resolution in a checker that runs before the registry exists.
struct TypeIndex {
    interfaces: Vec<String>,
    aliases: BTreeMap<String, TypeSpec>,
}

impl TypeIndex {
    fn of(spec: &Spec) -> TypeIndex {
        let mut index = TypeIndex { interfaces: Vec::new(), aliases: BTreeMap::new() };
        index.walk(&spec.definitions);
        index
    }

    fn walk(&mut self, defs: &[Definition]) {
        for d in defs {
            match d {
                Definition::Module(m) => self.walk(&m.definitions),
                Definition::Interface(i) => {
                    self.interfaces.push(i.name.text.to_ascii_lowercase());
                    for member in i.body.iter().flatten() {
                        if let InterfaceMember::Nested(nested) = member {
                            self.walk(std::slice::from_ref(nested));
                        }
                    }
                }
                Definition::Typedef(t) => {
                    self.aliases.insert(t.name.text.to_ascii_lowercase(), t.ty.clone());
                }
                _ => {}
            }
        }
    }

    /// Whether a type spec is, or contains, a live object reference.
    fn is_reference(&self, ty: &TypeSpec, depth: usize) -> bool {
        const MAX_DEPTH: usize = 6;
        if depth > MAX_DEPTH {
            return false;
        }
        match ty {
            TypeSpec::Object => true,
            TypeSpec::Sequence { element, .. } => self.is_reference(element, depth + 1),
            TypeSpec::Named(sn) => {
                let simple = last(sn).to_ascii_lowercase();
                if self.interfaces.contains(&simple) {
                    return true;
                }
                match self.aliases.get(&simple) {
                    Some(inner) => self.is_reference(&inner.clone(), depth + 1),
                    None => false,
                }
            }
            _ => false,
        }
    }
}

fn last(sn: &ScopedName) -> &str {
    sn.parts.last().map(String::as_str).unwrap_or_default()
}

/// How an operation hands a reference to its caller, if it does.
fn escaping_reference(index: &TypeIndex, op: &Operation) -> Option<String> {
    if index.is_reference(&op.returns, 0) {
        return Some("as its return value".to_owned());
    }
    op.params
        .iter()
        .filter(|p: &&Param| p.direction != orbweaver_idl::ast::Direction::In)
        .find(|p| index.is_reference(&p.ty, 0))
        .map(|p| format!("through the {:?} parameter", p.name.text))
}

/// Types no unit can describe. Only the primitives, so an unresolved name is
/// never guessed at — a wrong hint costs a round.
fn plainly_not_numeric(ty: &TypeSpec) -> bool {
    matches!(
        ty,
        TypeSpec::Boolean
            | TypeSpec::Char
            | TypeSpec::WChar
            | TypeSpec::String(_)
            | TypeSpec::WString(_)
            | TypeSpec::Object
            | TypeSpec::Void
    )
}

/// What S3 changed beyond the comments, if anything.
///
/// The comparison is over the **registry** rather than the syntax tree: the
/// registry is the form both consumers of a contract read, it stores
/// annotations beside the signature rather than inside it, and two files that
/// register identically are the same contract however the whitespace moved.
fn contract_changes(before: &str, after: &str) -> Vec<Finding> {
    let (Some(old), Some(new)) = (contract_shape(before), contract_shape(after)) else {
        // The input did not register — S3 was handed something S2's gate should
        // have caught, and inventing a diff here would blame the wrong stage.
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut say = |what: String, fix: &str| {
        out.push(Finding {
            rule: "s3/contract-changed".to_owned(),
            severity: Severity::Error,
            message: format!(
                "S3 may only add structured comments, and this output changes the contract: \
                 {what}"
            ),
            line: 0,
            column: 0,
            source: String::new(),
            fix: Some(fix.to_owned()),
        });
    };
    for (id, shape) in &old {
        match new.get(id) {
            None => say(
                format!("{id} is gone"),
                "return the input file with annotations added — nothing removed",
            ),
            Some(now) if now != shape => say(
                format!("{id} has a different shape than it did before annotation"),
                "annotate the signature; do not adjust it. A signature changed here is a change \
                 nobody reviewed",
            ),
            Some(_) => {}
        }
    }
    for id in new.keys() {
        if !old.contains_key(id) {
            say(
                format!("{id} was added"),
                "S3 adds comments only; a new declaration belongs to S2, where the gate compares \
                 it against the brief",
            );
        }
    }
    out
}

/// A contract as a comparable map: repository id → everything but annotations.
fn contract_shape(idl: &str) -> Option<BTreeMap<String, String>> {
    let spec = orbweaver_idl::check(idl).ok()?;
    let mut registry = Registry::new();
    registry.load(&spec).ok()?;
    let mut out = BTreeMap::new();
    for id in registry.ids() {
        let text = match registry.get(id) {
            Some(Entry::Interface(i)) => {
                let ops: Vec<String> = i
                    .operations
                    .iter()
                    .map(|(name, sig)| {
                        let params: Vec<String> = sig
                            .params
                            .iter()
                            .map(|p| format!("{:?} {:?} {}", p.direction, p.tc, p.name))
                            .collect();
                        format!(
                            "{name}(oneway={}, {}) -> {:?} raises {:?}",
                            sig.oneway,
                            params.join(", "),
                            sig.returns,
                            sig.raises
                        )
                    })
                    .collect();
                let attrs: Vec<String> = i
                    .attributes
                    .iter()
                    .map(|(name, a)| format!("{name}: readonly={} {:?}", a.readonly, a.tc))
                    .collect();
                format!(
                    "interface bases={:?} forward={} ops=[{}] attrs=[{}]",
                    i.bases,
                    i.forward_only,
                    ops.join("; "),
                    attrs.join("; ")
                )
            }
            Some(Entry::Type(tc)) => format!("type {tc:?}"),
            Some(Entry::Const { tc }) => format!("const {tc:?}"),
            None => continue,
        };
        out.insert(id.clone(), text);
    }
    Some(out)
}

fn collect_interfaces<'a>(defs: &'a [Definition], out: &mut Vec<&'a Interface>) {
    for d in defs {
        match d {
            Definition::Interface(i) => {
                out.push(i);
                for member in i.body.iter().flatten() {
                    if let InterfaceMember::Nested(nested) = member {
                        collect_interfaces(std::slice::from_ref(nested), out);
                    }
                }
            }
            Definition::Module(m) => collect_interfaces(&m.definitions, out),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fully annotated contract: the shape S3 is supposed to produce.
    const ANNOTATED: &str = "module bank {
         //@ ai_desc: Moves money between accounts
         interface Ledger {
           //@ ai_desc: Returns the balance of one account
           //@ ai_effect: read_only
           long long balance(in string the_account);

           //@ ai_desc: Moves an amount between two accounts
           //@ ai_effect: destructive
           //@ ai_idempotent: false
           //@ ai_authz: ledger.transfer
           void transfer(
             //@ ai_pii: high
             in string source,
             //@ ai_unit: KRW
             in long long amount);
         };
       };";

    fn rules(idl: &str) -> Vec<String> {
        check(idl).findings.iter().map(|f| f.rule.clone()).collect()
    }

    #[test]
    fn a_fully_annotated_contract_passes_s3s_own_gate() {
        let r = gate(ANNOTATED);
        assert!(r.is_ok(), "{:?}", r.findings);
    }

    /// The reason this stage exists, as a test: S4 is content and S3 is not.
    #[test]
    fn s4_accepts_what_s3_refuses() {
        let bare = "module bank { interface Ledger { void transfer(in long amount); }; };";
        assert!(validate(bare).is_ok(), "unannotated IDL is valid CORBA and S4 says so");
        let r = gate(bare);
        assert!(!r.is_ok(), "and S3's gate is where it is a failure");
        let names = rules(bare);
        assert!(names.contains(&"s3/missing-ai_desc".to_owned()), "{names:?}");
        assert!(names.contains(&"s3/missing-ai_effect".to_owned()), "{names:?}");
        assert!(names.contains(&"s3/missing-ai_authz".to_owned()), "{names:?}");
    }

    /// The dominant corpus finding, codified as a check. `transfer` changes
    /// money and requires no permission.
    #[test]
    fn a_mutating_operation_with_no_scope_is_the_corpus_finding_as_an_error() {
        let src = "module bank { interface Ledger {
             //@ ai_desc: Moves an amount
             //@ ai_effect: destructive
             void transfer(in long long amount);
           }; };";
        let f = check(src)
            .findings
            .into_iter()
            .find(|f| f.rule == "s3/missing-ai_authz")
            .expect("reported");
        assert_eq!(f.severity, Severity::Error);
        assert!(f.message.contains("destructive"), "{}", f.message);
        assert!(f.message.contains("any caller who reaches the bridge"), "{}", f.message);
        assert!(f.line > 0, "the finding is locatable: {f:?}");
    }

    /// The name alone is evidence enough, with no effect annotation to argue
    /// with — but only one authz finding per operation.
    #[test]
    fn only_one_missing_authz_finding_per_operation() {
        let src = "module m { interface Target { long ping(); };
             interface I {
               //@ ai_desc: Creates a session
               //@ ai_effect: destructive
               Target create_session(
                 //@ ai_pii: high
                 in string user);
             };
           };";
        let authz: Vec<Finding> =
            check(src).findings.into_iter().filter(|f| f.rule.contains("authz")).collect();
        assert_eq!(authz.len(), 1, "{authz:?}");
    }

    #[test]
    fn a_reference_handed_out_without_a_scope_is_reported() {
        let src = "module m { interface Target {
               //@ ai_desc: pings
               //@ ai_effect: read_only
               long ping(); };
             interface Directory {
               //@ ai_desc: Looks a target up by name
               //@ ai_effect: read_only
               Target lookup(in string named);
             };
           };";
        let f = check(src)
            .findings
            .into_iter()
            .find(|f| f.rule == "s3/missing-ai_authz")
            .expect("reported");
        assert!(f.message.contains("bearer address"), "{}", f.message);
    }

    /// A sequence of references hands out as many bearer addresses as it holds
    /// — the more generous case, and the one a shallow check misses.
    #[test]
    fn a_reference_inside_a_sequence_still_escapes() {
        let src = "module m { interface Target {
               //@ ai_desc: pings
               //@ ai_effect: read_only
               long ping(); };
             typedef sequence<Target> TargetSeq;
             interface Directory {
               //@ ai_desc: Lists every target
               //@ ai_effect: read_only
               TargetSeq all();
             };
           };";
        assert!(rules(src).contains(&"s3/missing-ai_authz".to_owned()), "{:?}", rules(src));
    }

    #[test]
    fn a_scope_on_the_interface_is_an_error_because_the_guard_never_reads_it() {
        let src = "module m {
             //@ ai_authz: m.admin
             interface I {
               //@ ai_desc: peeks
               //@ ai_effect: read_only
               long peek();
             };
           };";
        let f = check(src)
            .findings
            .into_iter()
            .find(|f| f.rule == "s3/authz-on-interface-only")
            .expect("reported");
        assert!(f.message.contains("per operation"), "{}", f.message);
    }

    #[test]
    fn a_read_only_claim_contradicted_by_the_name_is_an_error() {
        let src = "module m { interface I {
             //@ ai_desc: Deletes an account
             //@ ai_effect: read_only
             void delete_account(in long id);
           }; };";
        let names = rules(src);
        assert!(names.contains(&"s3/read-only-mutating-name".to_owned()), "{names:?}");
        // …and the authz rule does not pile on: fix the effect first.
        assert!(!names.contains(&"s3/missing-ai_authz".to_owned()), "{names:?}");
    }

    #[test]
    fn a_typo_in_the_vocabulary_is_an_error_because_nobody_reads_it() {
        let src = "module m { interface I {
             //@ ai_descr: peeks
             //@ ai_desc: peeks
             //@ ai_effect: read_only
             long peek();
           }; };";
        assert!(rules(src).contains(&"s3/unknown-annotation".to_owned()), "{:?}", rules(src));
    }

    #[test]
    fn an_effect_outside_the_vocabulary_names_what_the_gate_will_do() {
        let src = "module m { interface I {
             //@ ai_desc: goes
             //@ ai_effect: probably_fine
             //@ ai_authz: m.go
             void go();
           }; };";
        let f = check(src)
            .findings
            .into_iter()
            .find(|f| f.rule == "s3/effect-unknown")
            .expect("reported");
        assert!(f.message.contains("approval"), "{}", f.message);
    }

    #[test]
    fn a_pii_level_outside_the_vocabulary_is_reported_on_the_parameter() {
        let src = "module m { interface I {
             //@ ai_desc: quotes
             //@ ai_effect: read_only
             long quote(
               //@ ai_pii: extreme
               in long who);
           }; };";
        let f = check(src)
            .findings
            .into_iter()
            .find(|f| f.rule == "s3/pii-level-unknown")
            .expect("reported");
        assert_eq!(f.source, "who");
    }

    #[test]
    fn a_unit_on_a_string_is_a_warning_not_a_refusal() {
        let src = "module m { interface I {
             //@ ai_desc: quotes
             //@ ai_effect: read_only
             long quote(
               //@ ai_unit: KRW
               in string amount);
           }; };";
        let r = check(src);
        let f = r.findings.iter().find(|f| f.rule == "s3/unit-on-non-numeric").expect("warned");
        assert_eq!(f.severity, Severity::Warning);
        assert!(r.is_ok(), "a warning does not fail the stage");
    }

    #[test]
    fn ai_idempotent_must_be_a_boolean() {
        let src = "module m { interface I {
             //@ ai_desc: publishes
             //@ ai_effect: destructive
             //@ ai_authz: m.write
             //@ ai_idempotent: mostly
             oneway void publish(in string topic);
           }; };";
        assert!(rules(src).contains(&"s3/idempotent-not-boolean".to_owned()), "{:?}", rules(src));
    }

    /// The 2026-08-14 batch's one root cause, codified. `contract-check`
    /// reported this on R13's oneway log sink and S3's gate had nothing to say:
    /// a gate missing a rule its own output can break.
    ///
    /// The pair matters. `ai_idempotent: true` on a oneway is **not** flagged —
    /// retry-safety is a claim about repetition, and on a call whose delivery
    /// is unconfirmable it is the claim that makes recovery possible. It is the
    /// `false` that leaves a caller with no correct move.
    #[test]
    fn a_oneway_declared_unsafe_to_retry_leaves_the_caller_stuck() {
        let unsafe_retry = "module m { interface I {
             //@ ai_desc: Submits a batch of records
             //@ ai_effect: destructive
             //@ ai_authz: m.write
             //@ ai_idempotent: false
             oneway void submit(in string batch);
           }; };";
        let f = check(unsafe_retry)
            .findings
            .into_iter()
            .find(|f| f.rule == "s3/oneway-not-idempotent")
            .expect("reported");
        assert_eq!(f.severity, Severity::Error);
        assert!(f.message.contains("cannot safely retry"), "{}", f.message);
        assert!(f.fix.as_deref().unwrap().contains("ai_idempotent: true"), "{f:?}");

        let safe_retry = unsafe_retry.replace("ai_idempotent: false", "ai_idempotent: true");
        assert!(check(&safe_retry).is_ok(), "{:?}", check(&safe_retry).findings);
    }

    /// S3 annotates; it does not redesign. Anything else is a change nobody
    /// reviewed, attributed to S2 by every reader of the diff.
    #[test]
    fn changing_the_contract_while_annotating_is_refused() {
        let before = "module m { interface I { long peek(); void go(in long n); }; };";
        let after = "module m { interface I {
             //@ ai_desc: peeks
             //@ ai_effect: read_only
             long peek();
           }; };";
        let r = check_against(before, after);
        let f = r.findings.iter().find(|f| f.rule == "s3/contract-changed").expect("refused");
        assert!(f.message.contains("different shape") || f.message.contains("gone"), "{f:?}");
    }

    #[test]
    fn adding_a_declaration_while_annotating_is_refused() {
        let before = "module m { interface I {
             //@ ai_desc: peeks
             //@ ai_effect: read_only
             long peek(); }; };";
        let after = "module m { struct Extra { long x; };
           interface I {
             //@ ai_desc: peeks
             //@ ai_effect: read_only
             long peek(); }; };";
        let r = check_against(before, after);
        assert!(r.findings.iter().any(|f| f.rule == "s3/contract-changed"), "{:?}", r.findings);
    }

    /// The permitted change: comments, and nothing else.
    #[test]
    fn adding_only_annotations_is_accepted() {
        let before = "module bank {
             interface Ledger {
               long long balance(in string the_account);
               void transfer(in string source, in long long amount);
             };
           };";
        assert!(check_against(before, ANNOTATED).is_ok(), "{:?}", check_against(before, ANNOTATED));
    }

    /// A file that does not parse gets its parse error and nothing else: S3
    /// findings piled on top of a syntax error bury the cause.
    #[test]
    fn a_broken_file_reports_only_why_it_is_broken() {
        let r = gate("module m { interface I { long peek() }; };");
        assert!(!r.is_ok());
        assert!(r.findings.iter().all(|f| !f.rule.starts_with("s3/")), "{:?}", r.findings);
    }

    /// The codify test. Every rule S3 enforces is also a constraint S3's prompt
    /// states, and vice versa — a rule in one and not the other is exactly how
    /// the corpus finding got in.
    #[test]
    fn every_rule_is_a_prompt_constraint_and_a_check() {
        for rule in RULES {
            assert!(
                S3_PROMPT.contains(rule.prompt_phrase),
                "{}: the prompt never says {:?}, so the model is measured against a rule it was \
                 not given",
                rule.id,
                rule.prompt_phrase
            );
            assert!(!rule.demand.is_empty());
        }

        // And each rule fires on a file that breaks it, so the roster is not a
        // list of aspirations.
        let samples: [(&str, &str); 11] = [
            ("s3/missing-ai_desc", "module m { interface I { long peek(); }; };"),
            (
                "s3/missing-ai_effect",
                "module m { interface I { //@ ai_desc: peeks\n long peek(); }; };",
            ),
            (
                "s3/effect-unknown",
                "module m { interface I { //@ ai_desc: x\n //@ ai_effect: maybe\n //@ ai_authz: \
                 m.x\n long peek(); }; };",
            ),
            (
                "s3/missing-ai_authz",
                "module m { interface I { //@ ai_desc: x\n //@ ai_effect: destructive\n void \
                 go(); }; };",
            ),
            (
                "s3/read-only-mutating-name",
                "module m { interface I { //@ ai_desc: x\n //@ ai_effect: read_only\n void \
                 delete_it(); }; };",
            ),
            (
                "s3/authz-on-interface-only",
                "module m { //@ ai_authz: m.admin\n interface I { //@ ai_desc: x\n //@ ai_effect: \
                 read_only\n long peek(); }; };",
            ),
            (
                "s3/unknown-annotation",
                "module m { interface I { //@ ai_desc: x\n //@ ai_effect: read_only\n //@ \
                 ai_wrong: y\n long peek(); }; };",
            ),
            (
                "s3/pii-level-unknown",
                "module m { interface I { //@ ai_desc: x\n //@ ai_effect: read_only\n long \
                 peek(//@ ai_pii: extreme\n in long who); }; };",
            ),
            (
                "s3/idempotent-not-boolean",
                "module m { interface I { //@ ai_desc: x\n //@ ai_effect: read_only\n //@ \
                 ai_idempotent: mostly\n long peek(); }; };",
            ),
            (
                "s3/oneway-not-idempotent",
                "module m { interface I { //@ ai_desc: x\n //@ ai_effect: destructive\n //@ \
                 ai_authz: m.write\n //@ ai_idempotent: false\n oneway void publish(in string \
                 topic); }; };",
            ),
            ("s3/contract-changed", ""),
        ];
        for rule in RULES {
            let (_, sample) = samples.iter().find(|(id, _)| *id == rule.id).unwrap_or_else(|| {
                panic!("{} has no sample; every rule must be shown to fire", rule.id)
            });
            if rule.id == "s3/contract-changed" {
                // Needs a before and an after, so it has its own test above.
                let before = "module m { interface I { long peek(); }; };";
                let after = "module m { interface I { //@ ai_desc: x\n //@ ai_effect: read_only\n \
                             long peek();\n //@ ai_desc: y\n //@ ai_effect: read_only\n long \
                             extra(); }; };";
                assert!(
                    check_against(before, after).findings.iter().any(|f| f.rule == rule.id),
                    "{} never fires",
                    rule.id
                );
                continue;
            }
            assert!(
                check(sample).findings.iter().any(|f| f.rule == rule.id),
                "{} never fires on its own sample:\n{sample}\ngot {:?}",
                rule.id,
                rules(sample)
            );
        }
    }

    /// The mirrored vocabulary is the one the prompt hands the model. Drift
    /// between them would measure the model against a list it never saw.
    #[test]
    fn the_vocabulary_matches_the_prompt() {
        for key in VOCABULARY {
            assert!(S3_PROMPT.contains(key), "the prompt never names {key}");
        }
        assert_eq!(UNGATED_EFFECTS, ["read_only", "readonly", "idempotent", "safe"]);
        assert_eq!(GATED_EFFECTS, ["destructive"]);
        assert!(UNGATED_EFFECTS.iter().all(|u| !GATED_EFFECTS.contains(u)));
    }
}
