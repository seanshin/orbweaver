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
//! # The scope the requirement stated, bound to the scope this stage emits
//!
//! S3 invents `ai_authz` from the IDL, and the IDL does not carry the
//! requirement's own words. A run recorded on 2026-08-14
//! (`docs/pipeline-runs/2026-08-14-end-to-end.md`, Cause A) re-ran S1–S3 over an
//! unchanged requirement with unchanged prompts and produced a contract that
//! passed every gate 1/1 while asking for `parkinglot.barrier.open` where the
//! requirement says `gate:operate`. Nothing in this project could see it: the
//! deployment fails **closed** against every legitimate caller, and the evidence
//! points at the identity provider.
//!
//! `docs/decisions/D005-contract-stability.md` (approved) answers it with option
//! C, which is [`check_against_brief`] and [`stated_scope_findings`]: a
//! scope-shaped token the requirement states, recorded by S1 in
//! [`crate::ingest::Brief`], must appear verbatim in this stage's output. String
//! equality, no model. What it does *not* buy is in
//! [`crate::ingest::scope_shaped`]'s documentation and in the decision — it binds
//! one *stated* token, and a requirement that states none is untouched.
//!
//! **S3은 별도 단계다.** 우리 코퍼스를 `contract-check`로 재면 20건이 나왔고
//! 지배적 원인은 **변경 연산에 스코프(`ai_authz`)가 없는 것**이었다. 이는 다른
//! 프롬프트에 얹힌 단계가 만드는 전형적 실패이며, 구문 게이트로는 잡을 수 없다
//! (주석 없는 IDL도 적법하다). 따라서 S3 자신의 게이트에서 오류로 잡는다.

use std::collections::BTreeMap;

use orbweaver_idl::ast::{
    Definition, Interface, InterfaceMember, Operation, Param, ScopedName, Spec, TypeSpec,
    ValueMember,
};
use orbweaver_idl::lex::Annotation;
use orbweaver_registry::{Entry, Registry};

use crate::ingest::Brief;
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
pub const RULES: [Rule; 12] = [
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
    Rule {
        id: "s3/authz-not-the-stated-scope",
        demand: "a scope-shaped token the requirement states survives verbatim into ai_authz",
        prompt_phrase: "copy each token verbatim into ai_authz",
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

When a SCOPES THE REQUIREMENT STATES block accompanies the input, its tokens
are quoted from the requirement itself:
copy each token verbatim into ai_authz on the operation it names.
Do not normalise it, do not reword it, do not compose a tidier one from the
interface's own vocabulary. A scope is a string an identity provider issues, so a
contract that asks for a scope the requirement never stated refuses every
legitimate caller — and the refusal is well-formed, correctly audited, and
indistinguishable from a permissions misconfiguration, so the people who debug
it will check the identity provider and find it correct.

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

/// The key a contract's author states a **precondition** under: what must be
/// true before the call, in the author's own words.
///
/// Named as a constant because it now has a reader — [`crate::infer::Subject`]
/// renders it into the prompt a producer is shown — and CLAUDE.md's rule about
/// a sentence many layers say applies to a key name exactly as it applies to a
/// refusal: the moment two layers spell it, one of them can misspell it in
/// silence. Until 2026-08-25 the key had no reader at all and the literal in
/// [`VOCABULARY`] was the whole of its existence.
pub const AI_PRECOND: &str = "ai_precond";

/// The key a contract's author states a **worked example** under.
///
/// Same reason as [`AI_PRECOND`] for being a constant, and one more of its
/// own: D025 §7 forbids inferring into this slot, so every value that ever
/// appears under it was typed by a person. A key that may only ever hold
/// authored text is a key whose name must not drift from the checker that
/// admits it.
pub const AI_EXAMPLE: &str = "ai_example";

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
    AI_EXAMPLE,
    AI_PRECOND,
    "ai_authz",
];

/// The SIDL version [`VOCABULARY`] is the vocabulary of.
///
/// Until 2026-08-19 "v1" was a doc comment: nothing a consumer could read,
/// nothing a contract could declare, so a v2 key added tomorrow would be
/// indistinguishable from a v1 typo to every reader in the tree. This is the
/// number a contract is compared against when it declares one with
/// [`SIDL_VERSION_KEY`], and it moves when the vocabulary does — never
/// separately. Mirrored in `orbweaver-test`'s `contract::SIDL_VERSION` beside
/// that crate's copy of the vocabulary, for the reason [`VOCABULARY`] gives;
/// the two copies are pinned equal by `orbweaver-test`'s
/// `the_mirror_matches_the_s7_authority`, the one test that can see both.
pub const SIDL_VERSION: &str = "1";

/// The structured comment a contract declares its SIDL version with:
/// `//@ sidl_version: 1`.
///
/// Optional. A contract that declares none is read as v1, because every
/// contract written before the key existed is v1 and a finding on all of them
/// would report the calendar rather than the file. Not an `ai_*` key: it says
/// nothing about an operation, and putting it in the vocabulary would make it
/// a thing an agent reads. To omniidl and every other compiler it is a comment.
pub const SIDL_VERSION_KEY: &str = "sidl_version";

/// The `sidl_version` a file declares, wherever the comment sits.
///
/// The lexer hands a structured comment to the declaration that follows it,
/// so a marker written at the top of a file lands on the first `module` — a
/// place the registry keeps no annotations for — and a marker under a
/// `#pragma prefix` lands one declaration later. Reading it from the syntax
/// tree rather than the registry is what lets it be written where a person
/// would write it. Every carrier is walked in source order and the first
/// marker wins; a file that declares two disagreeing ones is reported on the
/// first, which is the one a reader meets first too.
pub fn declared_sidl_version(spec: &Spec) -> Option<&Annotation> {
    fn in_list(list: &[Annotation]) -> Option<&Annotation> {
        list.iter().find(|a| a.key == SIDL_VERSION_KEY)
    }
    fn in_members(members: &[InterfaceMember]) -> Option<&Annotation> {
        members.iter().find_map(|m| match m {
            InterfaceMember::Operation(op) => in_list(&op.annotations)
                .or_else(|| op.params.iter().find_map(|p| in_list(&p.annotations))),
            InterfaceMember::Attribute(a) => in_list(&a.annotations),
            InterfaceMember::Nested(d) => in_defs(std::slice::from_ref(d)),
        })
    }
    fn in_defs(defs: &[Definition]) -> Option<&Annotation> {
        defs.iter().find_map(|d| match d {
            Definition::Module(m) => in_list(&m.annotations).or_else(|| in_defs(&m.definitions)),
            Definition::Interface(i) => {
                in_list(&i.annotations).or_else(|| i.body.as_deref().and_then(in_members))
            }
            Definition::Struct(s) | Definition::Exception(s) => in_list(&s.annotations)
                .or_else(|| s.members.iter().flatten().find_map(|m| in_list(&m.annotations))),
            Definition::Union(u) => in_list(&u.annotations)
                .or_else(|| u.cases.iter().find_map(|c| in_list(&c.member.annotations))),
            Definition::Enum(e) => in_list(&e.annotations),
            Definition::Typedef(t) => in_list(&t.annotations),
            Definition::Const(c) => in_list(&c.annotations),
            Definition::ValueType(v) => in_list(&v.annotations).or_else(|| {
                v.members.iter().flatten().find_map(|m| match m {
                    ValueMember::State { member, .. } => in_list(&member.annotations),
                    ValueMember::Other(other) => in_members(std::slice::from_ref(other)),
                })
            }),
            Definition::Native(_) => None,
        })
    }
    in_defs(&spec.definitions)
}

/// Whether a declared `sidl_version` is one this reader knows.
///
/// [`Severity::Warning`], and outside [`RULES`] on purpose. S3 does not write
/// the marker — a marker comes in with the file, from a person or an earlier
/// tool — so it is not something the prompt can demand, and the roster test
/// would have nothing of the stage's own to show firing. What a foreign version
/// means is what `s3/unknown-annotation` means one key at a time: the file
/// states something this checker does not read, so a pass from it is a pass
/// over the part it understood. Warning is where every "read by nobody"
/// finding sits, here and in `contract-check`; an Error would refuse a file
/// for being newer than the tool, which is the tool's problem to report and
/// not the contract's to fail on.
pub fn sidl_version_findings(spec: &Spec) -> Vec<Finding> {
    let Some(marker) = declared_sidl_version(spec) else { return Vec::new() };
    let declared = marker.value.trim();
    if declared == SIDL_VERSION {
        return Vec::new();
    }
    let relation = match (declared.parse::<u32>(), SIDL_VERSION.parse::<u32>()) {
        (Ok(theirs), Ok(ours)) if theirs > ours => "later than",
        (Ok(_), Ok(_)) => "not",
        _ => "not a version number, so not",
    };
    vec![finding(
        "s3/unknown-sidl-version",
        Severity::Warning,
        format!(
            "the file declares sidl_version {declared:?}, which is {relation} the SIDL \
             v{SIDL_VERSION} this gate reads; any key that version adds is checked by nothing \
             here, so a pass from this gate covers the part of the contract it understood"
        ),
        &marker.span,
        format!("{}: {}", marker.key, marker.value),
        &format!(
            "write `//@ {SIDL_VERSION_KEY}: {SIDL_VERSION}` if the file is a v{SIDL_VERSION} \
             contract, or upgrade the checker before trusting its verdict on this file"
        ),
    )]
}

/// `ai_effect` values the MCP policy gate treats as needing no approval.
///
/// **No longer a mirror.** It used to carry its own copy of the list and a
/// doc comment admitting where the list came from — *"mirrored from
/// `orbweaver-mcp`'s `policy::is_harmless` … by way of `orbweaver-test`'s
/// `contract::UNGATED_EFFECTS`"* — three hand-kept copies of one predicate,
/// which is the shape that goes quiet when the owner changes for a good
/// reason. Since 2026-08-26 the vocabulary has one home in
/// [`crate::effect::UNGATED`] and `policy::is_harmless` reads it too, so this
/// name is an alias and the agreement is not a thing that can drift.
/// See [`crate::infer::UNGATING`], which is pinned against the gate's own
/// behaviour rather than against this sentence.
pub const UNGATED_EFFECTS: [&str; 4] = crate::effect::UNGATED;

/// The subset of [`UNGATED_EFFECTS`] claiming the operation *only reads*.
///
/// Not the same set: `idempotent` is a claim about repetition and an ordinary
/// thing to say about an operation that writes. Conflating the two was
/// `orbweaver-test`'s first false positive and is not repeated here.
pub const READ_ONLY_EFFECTS: [&str; 3] = ["read_only", "readonly", "safe"];

/// The one `ai_effect` value that means "needs a human" on purpose.
///
/// An alias for [`crate::effect::GATED`], for the reason
/// [`UNGATED_EFFECTS`] gives.
pub const GATED_EFFECTS: [&str; 1] = crate::effect::GATED;

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

/// [`check_against`] plus the binding between the requirement's own scope
/// tokens and the `ai_authz` this stage emits — D005 option C.
///
/// The brief is optional because S3 is runnable alone over an IDL file nobody
/// has a brief for, and a missing brief must mean *no binding* rather than *no
/// stated scope*. When one is supplied, [`Brief::stated_scopes`] decides what is
/// bound and [`stated_scope_findings`] checks it by string equality, with no
/// model anywhere in the path.
///
/// # The obligation that rides with it
///
/// D005 attaches one to whichever change lands its options, and states it as a
/// warning rather than a caveat: **stabilising regeneration converts the only
/// signal this project has ever produced that a reading was a *choice* rather
/// than a fact into silence.** Two runs disagreeing is how anyone learned that
/// S1 asked ten questions and S2 answered them alone. The compensating
/// instrument cannot be a green check.
///
/// What a crate can do about that is small and is done here: while the brief is
/// in hand — S3 is the last stage that holds it, because S4 and S5 see only IDL
/// — every unanswered `open_question` is carried into this report as
/// `s3/unanswered-question`, at [`Severity::Advice`], so it is in front of
/// whoever reads the stage report before registration. It blocks nothing and
/// proves nothing. The instrument is the person; this only makes the questions
/// impossible not to see.
pub fn check_against_brief(brief: Option<&Brief>, before: &str, after: &str) -> Report {
    let mut report = check_against(before, after);
    if !report.is_ok() {
        return report;
    }
    if let Some(brief) = brief {
        report.findings.extend(stated_scope_findings(&brief.stated_scopes(), after));
        report.findings.extend(brief.open_questions.iter().map(|q| Finding {
            rule: "s3/unanswered-question".to_owned(),
            severity: Severity::Advice,
            message: format!(
                "S1 could not settle this from the requirement and nothing since has: {q}. The \
                 contract answers it anyway — read this before the contract is registered"
            ),
            line: 0,
            column: 0,
            source: q.clone(),
            fix: None,
        }));
    }
    report
}

/// The scopes block S3's producer is handed alongside the IDL.
///
/// Empty when the brief binds nothing, which is most requirements — a prompt
/// that always carries a heading and no content teaches a reader to skip the
/// heading.
pub fn stated_scopes_block(scopes: &BTreeMap<String, String>) -> String {
    if scopes.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "SCOPES THE REQUIREMENT STATES — quoted from the requirement, to be copied verbatim into \
         ai_authz:\n",
    );
    for (operation, token) in scopes {
        out.push_str(&format!("  {operation}: {token}\n"));
    }
    out.push_str(
        "If the operation was renamed before it reached you, the token still belongs on whichever \
         operation now does that work. The token itself never changes.\n\n",
    );
    out
}

/// Whether every scope the requirement states survived into an `ai_authz`.
///
/// `scopes` is [`Brief::stated_scopes`]: operation name → the literal token.
/// Two ways to fire, one rule, at most one finding per stated token:
///
/// - **absent** — the token is nowhere in the file. This is the measured drift
///   (`gate:operate` → `parkinglot.barrier.open`) and it is caught whatever the
///   operations were renamed to, because the token is compared against the
///   file's whole set of scopes rather than against one operation.
/// - **misplaced** — an operation whose name still matches the brief's carries a
///   *different* scope. This is the case D005 calls decisive: a regeneration
///   that keeps every identifier and changes only `//@ ai_authz` passes all
///   eight hops of the end-to-end run today.
///
/// Silent on a file that does not parse: [`gate`] has already said why, and a
/// scope finding stacked on a syntax error buries the cause.
pub fn stated_scope_findings(scopes: &BTreeMap<String, String>, idl: &str) -> Vec<Finding> {
    if scopes.is_empty() {
        return Vec::new();
    }
    let Ok(spec) = orbweaver_idl::check(idl) else { return Vec::new() };
    let mut interfaces: Vec<&Interface> = Vec::new();
    collect_interfaces(&spec.definitions, &mut interfaces);

    struct Op<'a> {
        iface: &'a str,
        name: &'a str,
        scope: Option<&'a str>,
        at: &'a orbweaver_idl::lex::Span,
    }
    let mut ops: Vec<Op<'_>> = Vec::new();
    for iface in &interfaces {
        for member in iface.body.iter().flatten() {
            if let InterfaceMember::Operation(op) = member {
                let ann = annotations(&op.annotations);
                ops.push(Op {
                    iface: iface.name.text.as_str(),
                    name: op.name.text.as_str(),
                    scope: ann.get("ai_authz").copied().filter(|s| !s.is_empty()),
                    at: &op.name.span,
                });
            }
        }
    }
    let present: BTreeMap<&str, &str> =
        ops.iter().filter_map(|o| o.scope.map(|s| (s, o.name))).collect();

    // Grouped by token, so a scope two operations share is one cause and not
    // two findings (§5.1: causes, not items).
    let mut by_token: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (operation, token) in scopes {
        by_token.entry(token.as_str()).or_default().push(operation.as_str());
    }

    let mut out = Vec::new();
    for (token, brief_ops) in by_token {
        if !present.contains_key(token) {
            let found: Vec<String> = present.keys().map(|s| format!("{s:?}")).collect();
            let found = if found.is_empty() {
                "no operation in this file carries a scope at all".to_owned()
            } else {
                format!("the scopes this file does ask for are {}", found.join(", "))
            };
            out.push(Finding {
                rule: "s3/authz-not-the-stated-scope".to_owned(),
                severity: Severity::Error,
                message: format!(
                    "the requirement states the scope {token:?} and S1 recorded it on {}, but no \
                     operation carries `//@ ai_authz: {token}` — {found}. An identity provider \
                     issuing the scope the requirement states refuses every caller of a contract \
                     that asks for a different one, and the refusal is well-formed, correctly \
                     audited and indistinguishable from a permissions misconfiguration",
                    brief_ops.join(", ")
                ),
                line: 0,
                column: 0,
                source: token.to_owned(),
                fix: Some(format!(
                    "write `//@ ai_authz: {token}` verbatim above the operation that does this \
                     work. If the token itself is wrong, correct it in the brief and re-run from \
                     S2 — the brief is the artifact of record and an edit there is a decision \
                     somebody made, while a scope composed here is one nobody did"
                )),
            });
            continue;
        }
        for brief_op in brief_ops {
            let Some(op) = ops.iter().find(|o| o.name.eq_ignore_ascii_case(brief_op)) else {
                continue;
            };
            let Some(scope) = op.scope else { continue };
            if scope == token {
                continue;
            }
            out.push(Finding {
                rule: "s3/authz-not-the-stated-scope".to_owned(),
                severity: Severity::Error,
                message: format!(
                    "{}.{} carries ai_authz {scope:?}, and the requirement states {token:?} for \
                     the operation of that name; the contract and the identity provider would \
                     disagree about this one operation while every other check passes",
                    op.iface, op.name
                ),
                line: op.at.line,
                column: op.at.column,
                source: op.name.to_owned(),
                fix: Some(format!("annotate `//@ ai_authz: {token}` on {}", op.name)),
            });
        }
    }
    out
}

/// Every annotation check over one file.
///
/// Assumes the file parses; [`gate`] enforces that. Findings are ordered by
/// severity then position, matching S4.
pub fn check(idl: &str) -> Report {
    let Ok(spec) = orbweaver_idl::check(idl) else { return Report::default() };
    let index = TypeIndex::of(&spec);
    let mut findings = sidl_version_findings(&spec);
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
                "{where_} is an {}, so the MCP policy gate must assume it needs human approval",
                crate::effect::SILENCE
            ),
            at,
            name.to_owned(),
            // S3 writes for the author of the contract, exactly as S4 does:
            // the same sentence, the same three values, one home
            // (`crate::effect`). These two hints were byte-identical strings
            // maintained in two files, which is how they stayed equal by luck.
            &crate::effect::annotate_or_assume(
                &crate::effect::OFFER_AUTHOR,
                Some("--assume-effect <value>"),
            ),
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
                // The one site that enumerates rather than recommends: the
                // contract already said something, and the useful answer is
                // the whole vocabulary rather than a shortlist. That is why
                // `OFFER_ALL` exists and why this hint names `safe`.
                &crate::effect::annotate(&crate::effect::OFFER_ALL),
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
                    "{what} carries {k:?}, which is not in the SIDL v{SIDL_VERSION} vocabulary; the \
                     registry keeps it and no consumer reads it, so the annotation is present in \
                     the source and absent from every decision"
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
            // The value is part of the shape, not part of the annotations: an
            // S3 pass that changed a constant's value while adding comments
            // would be a change nobody reviewed, which is the whole thing this
            // comparison exists to catch.
            Some(Entry::Const { tc, value }) => format!("const {tc:?} = {value:?}"),
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

    /// A brief whose requirement states one scope-shaped token literally, on
    /// one operation — the shape of the 2026-08-14 parking requirement.
    fn scoped_brief(operation: &str, token: &str) -> Brief {
        Brief {
            requirement: format!("차단기 개방은 {token} 권한을 가진 운영자만 할 수 있다."),
            summary: "gate control".into(),
            operations: vec![crate::ingest::OperationSketch {
                name: operation.into(),
                effect: crate::ingest::Effect::Destructive,
                authz: Some(token.into()),
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    const GATE_IDL: &str = "module m { interface Gate {
             //@ ai_desc: Reads the gate state
             //@ ai_effect: read_only
             long peek();
             //@ ai_desc: Opens the entry gate
             //@ ai_effect: destructive
             //@ ai_authz: SCOPE
             void open_entry_gate(in long plate);
           }; };";

    /// D005's measured drift: the requirement says `gate:operate`, the contract
    /// asks for something else, every other gate passes.
    #[test]
    fn a_stated_scope_no_operation_carries_is_refused() {
        let brief = scoped_brief("open_entry_gate", "gate:operate");
        let after = GATE_IDL.replace("SCOPE", "parkinglot.barrier.open");
        let before = strip_annotations(&after);

        // Every other check is content — which is the whole finding.
        assert!(check_against(&before, &after).is_ok(), "{:?}", check_against(&before, &after));

        let r = check_against_brief(Some(&brief), &before, &after);
        let f =
            r.findings.iter().find(|f| f.rule == "s3/authz-not-the-stated-scope").expect("refused");
        assert_eq!(f.severity, Severity::Error);
        assert!(f.message.contains("gate:operate"), "{}", f.message);
        assert!(f.message.contains("parkinglot.barrier.open"), "{}", f.message);
        assert!(f.fix.as_deref().unwrap().contains("//@ ai_authz: gate:operate"), "{f:?}");
    }

    /// The token is in the file, on the wrong operation. D005 calls this the
    /// decisive case: names kept, only the scope moved, all eight hops green.
    #[test]
    fn a_stated_scope_on_the_wrong_operation_is_refused() {
        let brief = scoped_brief("open_entry_gate", "gate:operate");
        let after = "module m { interface Gate {
             //@ ai_desc: Reads the gate state
             //@ ai_effect: destructive
             //@ ai_authz: gate:operate
             long peek();
             //@ ai_desc: Opens the entry gate
             //@ ai_effect: destructive
             //@ ai_authz: parkinglot.barrier.open
             void open_entry_gate(in long plate);
           }; };";
        let before = strip_annotations(after);
        let f = check_against_brief(Some(&brief), &before, after)
            .findings
            .into_iter()
            .find(|f| f.rule == "s3/authz-not-the-stated-scope")
            .expect("refused");
        assert!(f.message.contains("Gate.open_entry_gate"), "{}", f.message);
        assert!(f.line > 0, "the finding is locatable: {f:?}");
    }

    /// The rule must be silent on correct output, or it is a rule people route
    /// around.
    #[test]
    fn a_contract_that_keeps_the_stated_scope_is_silent() {
        let brief = scoped_brief("open_entry_gate", "gate:operate");
        let after = GATE_IDL.replace("SCOPE", "gate:operate");
        let before = strip_annotations(&after);
        let r = check_against_brief(Some(&brief), &before, &after);
        assert!(r.is_ok(), "{:?}", r.findings);

        // …and still silent when S2 renamed the operation, as long as the token
        // survived: the rename is S2's right and the token is what binds.
        let renamed = after.replace("open_entry_gate", "open_entry_barrier");
        let renamed_before = strip_annotations(&renamed);
        assert!(check_against_brief(Some(&brief), &renamed_before, &renamed).is_ok());
    }

    /// Three ways the binding declines to fire, each one deliberate.
    #[test]
    fn the_binding_declines_rather_than_guesses() {
        let after = GATE_IDL.replace("SCOPE", "parkinglot.barrier.open");
        let before = strip_annotations(&after);

        // No brief: S3 run alone over IDL nobody has a brief for.
        assert!(check_against_brief(None, &before, &after).is_ok());

        // A brief whose token is not scope-shaped: prose, not a scope.
        let mut prose = scoped_brief("open_entry_gate", "gate:operate");
        prose.operations[0].authz = Some("운영자 권한".into());
        prose.requirement = "차단기 개방은 운영자 권한이 필요하다.".into();
        assert!(prose.stated_scopes().is_empty());
        assert!(check_against_brief(Some(&prose), &before, &after).is_ok());

        // A scope-shaped token S1 composed rather than read: the requirement
        // never says it, so nothing binds it.
        let mut invented = scoped_brief("open_entry_gate", "gate:operate");
        invented.requirement = "차단기 개방은 운영자만 할 수 있다.".into();
        assert!(invented.stated_scopes().is_empty(), "the requirement never states the token");
        assert!(check_against_brief(Some(&invented), &before, &after).is_ok());
    }

    /// The block is per-item data, and empty when there is nothing to say: a
    /// heading with no content under it teaches a reader to skip the heading.
    #[test]
    fn the_scopes_block_names_the_operation_and_the_token() {
        assert_eq!(stated_scopes_block(&BTreeMap::new()), "");
        let block =
            stated_scopes_block(&scoped_brief("open_entry_gate", "gate:operate").stated_scopes());
        assert!(block.contains("open_entry_gate: gate:operate"), "{block}");
        assert!(block.contains("SCOPES THE REQUIREMENT STATES"), "{block}");
    }

    /// A "before" for the contract-identity check: the same file without its
    /// structured comments, which is what S2 would have handed S3.
    fn strip_annotations(idl: &str) -> String {
        idl.lines().filter(|l| !l.trim_start().starts_with("//@")).collect::<Vec<_>>().join("\n")
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
        let samples: [(&str, &str); 12] = [
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
            ("s3/authz-not-the-stated-scope", ""),
        ];
        for rule in RULES {
            let (_, sample) = samples.iter().find(|(id, _)| *id == rule.id).unwrap_or_else(|| {
                panic!("{} has no sample; every rule must be shown to fire", rule.id)
            });
            if rule.id == "s3/authz-not-the-stated-scope" {
                // Needs the item's brief as well as the file, so it has its own
                // tests above; shown to fire here too, because a roster entry
                // nobody demonstrates is an aspiration.
                let brief = scoped_brief("open_entry_gate", "gate:operate");
                let after = GATE_IDL.replace("SCOPE", "parkinglot.barrier.open");
                let before = strip_annotations(&after);
                assert!(
                    check_against_brief(Some(&brief), &before, &after)
                        .findings
                        .iter()
                        .any(|f| f.rule == rule.id),
                    "{} never fires",
                    rule.id
                );
                continue;
            }
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
        // The version is a number a contract can be compared against, and it
        // is the version the vocabulary above is documented as. The
        // cross-crate half — that `orbweaver-test`'s copy says the same —
        // lives in that crate, the one that can see both.
        assert_eq!(SIDL_VERSION, "1", "the vocabulary above is SIDL v1's; move both together");
        assert!(SIDL_VERSION.parse::<u32>().is_ok(), "a version a file can declare as a number");
        assert!(!SIDL_VERSION_KEY.starts_with("ai_"), "the marker is not vocabulary");
        assert!(!VOCABULARY.contains(&SIDL_VERSION_KEY));
    }

    /// A contract that says which SIDL it was written to, and says this one,
    /// is silent; one that says none is silent too, because every contract
    /// written before the marker existed is v1.
    #[test]
    fn a_declared_or_undeclared_v1_is_silent() {
        let declared = format!("//@ sidl_version: 1\n{ANNOTATED}");
        assert!(gate(&declared).is_ok(), "{:?}", gate(&declared).findings);
        let spec = orbweaver_idl::check(&declared).expect("checks");
        assert_eq!(declared_sidl_version(&spec).map(|a| a.value.as_str()), Some("1"));
        assert!(declared_sidl_version(&orbweaver_idl::check(ANNOTATED).expect("checks")).is_none());
        // Written where a person might also write it: under the pragma, and
        // inside the module rather than above it.
        let under_pragma = format!("#pragma prefix \"x\"\n//@ sidl_version: 1\n{ANNOTATED}");
        assert!(gate(&under_pragma).is_ok(), "{:?}", gate(&under_pragma).findings);
        let inside = ANNOTATED.replacen("module bank {", "module bank {\n//@ sidl_version: 1", 1);
        assert!(gate(&inside).is_ok(), "{:?}", gate(&inside).findings);
    }

    /// A version this reader does not know is a Warning that says which way
    /// it is unknown, and does not fail the gate: newer than the tool is the
    /// tool's problem to report, not the contract's to fail on.
    #[test]
    fn a_later_sidl_version_is_a_warning_naming_what_is_unchecked() {
        let v2 = format!("//@ sidl_version: 2\n{ANNOTATED}");
        let r = gate(&v2);
        assert!(r.is_ok(), "a warning is not a failure: {:?}", r.findings);
        let f: Vec<&Finding> =
            r.findings.iter().filter(|f| f.rule == "s3/unknown-sidl-version").collect();
        assert_eq!(f.len(), 1, "{:?}", r.findings);
        assert_eq!(f[0].severity, Severity::Warning);
        assert!(f[0].message.contains("later than"), "{}", f[0].message);
        assert_eq!(f[0].line, 1, "the finding points at the marker");
        assert_eq!(f[0].source, "sidl_version: 2");

        let words = format!("//@ sidl_version: two\n{ANNOTATED}");
        let f = check(&words).findings;
        assert!(
            f.iter().any(|f| f.rule == "s3/unknown-sidl-version"
                && f.message.contains("not a version number")),
            "{f:?}"
        );
        // And it stays quiet on the rest of a well-annotated file: one finding.
        assert_eq!(f.len(), 1, "{f:?}");
    }
}
