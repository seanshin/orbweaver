//! D024 §5 — the four IDL tools an agent gets, and the gate they run through.
//!
//! An agent could already **find a contract, read it and call it**. It could
//! not validate IDL it wrote, diff a proposal against a released contract, ask
//! what a *type* looks like, or see what would be generated — the whole S1–S5
//! pipeline existed as command-line binaries and reached the agent through none
//! of it. These are those four stages, each wrapping one that already exists.
//!
//! # Findings, never a verdict (D024 §3)
//!
//! The temptation is a `compile_idl` that takes a string and returns
//! `{"ok": false}`. That is the stringly-typed surface this project has refused
//! twice in writing, and it is worst *here*: the caller is a generator that
//! will quote the answer back, so a bare verdict throws away the position, the
//! rule and the fix at the boundary where they matter most. Every tool below
//! returns [`orbweaver_forge::Report`]'s shape — findings with positions, rules
//! and fix hints — and `validate_contract` and `diff_contract` carry
//! `repair_prompt` besides, because that is the string a repair loop actually
//! consumes.
//!
//! # The gate: why there is a pseudo-contract
//!
//! D024 §5 says each tool passes through **the same interceptor chain as
//! `invoke_operation`**, and that this is what makes the change a
//! trust-boundary change rather than a convenience. The chain's
//! [`crate::interceptor::CallContext`] is keyed on a repository id and an
//! operation, and three of these four tools name nothing in the catalog at
//! all — so the honest question is what target and what operation a contract
//! tool *is*.
//!
//! The answer is that the tool surface is **a contract like any other**:
//! [`CONTRACT_TOOLS_IDL`] declares it, [`CONTRACT_TOOLS_ID`] names it, each
//! tool is one of its operations, and every operation carries a real
//! `//@ ai_effect: read_only` written by a real author. Nothing is
//! special-cased. Every stage of the chain gets a genuine answer out of
//! machinery that already existed, and an operator allowlists the tools with
//! the `--expose` they already know:
//!
//! ```text
//! --expose IDL:orbweaver/ContractTools:1.0                    # all four
//! --expose IDL:orbweaver/ContractTools:1.0.validate_contract  # one
//! ```
//!
//! **They are default-deny, like everything else here.** An absence does not
//! widen an allowlist, and D024 calls this a boundary change that is not
//! self-approvable; four tools that switched themselves on would be exactly
//! the widening-by-absence the deployment rules forbid. The refusal an agent
//! meets is `Denied::InterfaceNotExposed`, whose existing remedy already reads
//! correctly for it.
//!
//! # What each stage means for a tool that takes IDL text
//!
//! Argued per stage, because "it runs the chain" is not an argument:
//!
//! | stage | what it means here |
//! |---|---|
//! | `authn.expiry` | **Unchanged and fully applicable.** A credential that has outlived its grant has outlived it for every tool; nothing about IDL text makes it live again. |
//! | `authz.exposure` | **Load-bearing, and the operator's only switch.** Whether this agent may work with IDL at all, and which of the four. For `describe_type` there is a *second* exposure question the chain cannot ask — see below. |
//! | `authz.scopes` | **Applies, and is deliberately unused.** [`CONTRACT_TOOLS_IDL`] declares no `ai_authz`, so the stage runs and finds nothing to require. That is a statement, not a gap: a deployment that wants IDL work to need a scope adds it to the exposure's own contract rather than to a special case here. |
//! | `quota` | **Applies more strongly than to a wire call.** Parsing, diffing and generating are unbounded work driven entirely by agent-supplied text, and this is the only seat that bounds it. A deployment that fills `SEAT_QUOTA` gets these four counted for free. |
//! | `safety.approval` | **Applies, and answers `read_only` from the contract.** Not from a hard-coded exemption for "read-only tools": the four are annotated, so the effect an operator sees is one an author wrote, and `Unannotated` never has to guess. A future tool here that *did* mutate would be refused by this stage until somebody annotated it, which is the correct default. |
//! | `safety.content` | **The seat this fits best, and the reason arguments are passed.** The IDL text *is* the payload, and it is handed to the chain as [`crate::interceptor::CallContext::arguments`] exactly as an agent's call arguments are. A deployment's content rule sees the contract an agent is proposing before any of it is parsed. |
//! | `telemetry` | **Applies unchanged.** An agent's IDL work is counted in the same [`crate::promote::CallStats`] as its calls. |
//! | `audit` | **Applies unchanged, and matters most.** *This agent validated that contract* is precisely the line an operator wants, written by the same formatter as every other decision. |
//!
//! The one place a stage genuinely **cannot** answer is `describe_type`'s
//! second question, and it is not a stage failing — it is a question the chain
//! is not shaped to ask. See [`type_is_reachable`].
//!
//! # Registration is deliberately not here
//!
//! D024 §5: an agent that can register a contract can change what other agents
//! see, and that is `exposure`'s decision. Four read-only tools do not become
//! five by adding a write.
//!
//! *네 도구는 모두 판정이 아니라 진단을 돌려준다. 도구 표면 자체가 계약이므로
//! 체인의 모든 단계가 특수 처리 없이 진짜 답을 낸다. 기본은 거부다.*

use std::collections::BTreeMap;
use std::sync::OnceLock;

use orbweaver_dynamic::json::Json;
use orbweaver_forge::{Report, Severity, Source, WireGate};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_registry::{Entry, Registry};

use crate::policy::{Denied, Exposure};
use crate::{obj, s};

/// The repository id of the contract-tool surface.
///
/// An operator allowlists this to let an agent work with IDL, and may name one
/// operation of it to allow a single tool. It is a real repository id of a real
/// contract ([`CONTRACT_TOOLS_IDL`]) rather than a sentinel string, because a
/// sentinel would need every stage of the chain to know about it.
pub const CONTRACT_TOOLS_ID: &str = "IDL:orbweaver/ContractTools:1.0";

/// The four tool names, in the order [`crate::rpc::tool_definitions`]
/// advertises them.
pub const CONTRACT_TOOLS: [&str; 4] =
    ["validate_contract", "diff_contract", "describe_type", "preview_generation"];

/// The contract-tool surface, **as SIDL**.
///
/// This is not documentation of the tools; it is the contract the gate reads.
/// The `ai_effect` annotations are what `safety.approval` answers from, and
/// they are `read_only` because that is what these four are: they parse text,
/// read the catalog, and return findings. None of them dials anything, none
/// writes anything, and none can register a contract.
///
/// Written in SIDL structured comments rather than IDL 4 `@annotation`, per the
/// project's own rule — deployed compilers reject the latter.
pub const CONTRACT_TOOLS_IDL: &str = r#"
module orbweaver {
  //@ ai_desc: The IDL tools an agent gets: validate a contract, diff it against
  //@ a released one, describe a type, and preview what would be generated.
  interface ContractTools {
    //@ ai_desc: Check IDL and return findings with positions, rules and fixes
    //@ ai_effect: read_only
    string validate_contract(in string source);

    //@ ai_desc: Compare a proposed contract against a released one and report
    //@ every change with the verdict it carries and why
    //@ ai_effect: read_only
    string diff_contract(in string released, in string proposed);

    //@ ai_desc: Describe one type in the catalog, the way describe_interface
    //@ describes an interface
    //@ ai_effect: read_only
    string describe_type(in string target);

    //@ ai_desc: Report what would be generated for a contract and what would be
    //@ skipped, with the reason for every skip
    //@ ai_effect: read_only
    string preview_generation(in string source);
  };
};
"#;

/// The tool surface's own catalog, parsed once.
///
/// A registry of its own rather than an injection into the deployment's:
/// putting four synthetic operations into an operator's catalog would make
/// them show up in `search_interfaces`, in `dry_run_all`, and in every count a
/// report prints — a tool surface that pretended to be part of the estate.
/// The chain reads *this* registry when it gates a contract tool, which is
/// correct: the contract being called is this one.
pub fn contract_tools_registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let spec = orbweaver_idl::parse(CONTRACT_TOOLS_IDL)
            .expect("the contract-tool surface is first-party IDL and must parse");
        let mut registry = Registry::new();
        registry.load(&spec).expect("the contract-tool surface must load");
        registry
    })
}

/// Whether a **type** may be described, given what the exposure allows.
///
/// # The question the chain cannot ask
///
/// `describe_interface` refuses anything the allowlist does not name, and both
/// "not exposed" and "not in the registry" give the same answer so that a
/// refusal cannot become an oracle for what sits behind the gate (§4.6). A
/// *type* has no entry in that allowlist — an operator exposes interfaces,
/// because a bare type is not callable and exposing one would mean nothing.
///
/// So a `describe_type` that answered for anything in the registry would let an
/// agent enumerate the data model of an entire unexposed estate through a tool
/// whose chain run said ALLOW every time. The chain is not wrong; it was asked
/// about the tool, and this is a question about the argument.
///
/// The rule: **a type is describable exactly when an exposed interface reaches
/// it** — through an operation's return, a parameter, a raises clause or an
/// attribute, and transitively through members, cases, elements and aliases.
/// That is the same set an agent could already reconstruct from
/// `describe_interface`, so this tool reveals nothing new; it only saves the
/// agent from having to.
///
/// *체인이 물을 수 없는 질문이다 — 체인은 도구에 대해 물었고 이것은 인자에 대한
/// 질문이다. 노출된 인터페이스가 닿는 타입만 설명된다.*
pub fn type_is_reachable(registry: &Registry, exposure: &Exposure, target: &str) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    for id in exposure.interfaces() {
        // Only interfaces the exposure names *and* the catalog has.
        let Some(Entry::Interface(_)) = registry.get(id) else { continue };
        for (_op, _declared_in, sig) in crate::resolved_operations(registry, id) {
            reach(&sig.returns, &mut seen, 0);
            for p in &sig.params {
                reach(&p.tc, &mut seen, 0);
            }
            // A raises clause holds ids, not TypeCodes: the exception itself is
            // reachable, and so is everything its members name.
            for ex in &sig.raises {
                seen.insert(ex.to_string());
                if let Some(tc) = registry.typecode(ex) {
                    reach(tc, &mut seen, 0);
                }
            }
        }
        for (_attr, _declared_in, sig) in crate::resolved_attributes(registry, id) {
            reach(&sig.tc, &mut seen, 0);
        }
    }
    seen.contains(target)
}

/// Collects every repository id `tc` names, structurally.
///
/// No `Registry` parameter, deliberately: a `TypeCode` in this registry carries
/// its members inline rather than by reference, so the walk needs nothing
/// looked up. The one place a name must be resolved against the catalog is a
/// `raises` clause, which holds ids rather than TypeCodes — and that lookup is
/// done by the caller, where the registry is already in hand.
///
/// `depth` bounds a type the walk cannot otherwise terminate on; the `insert`
/// guard is what actually stops a recursive struct, and it is checked first so
/// that a legitimately deep type is not silently truncated by the limit.
fn reach(tc: &TypeCode, out: &mut std::collections::BTreeSet<String>, depth: usize) {
    if depth > 32 {
        return;
    }
    if let Some(id) = named_id(tc) {
        // A cycle through a recursive struct terminates here rather than in the
        // depth guard.
        if !out.insert(id.to_owned()) {
            return;
        }
    }
    match tc {
        TypeCode::Alias { aliased, .. } => reach(aliased, out, depth + 1),
        TypeCode::Sequence { element, .. } | TypeCode::Array { element, .. } => {
            reach(element, out, depth + 1)
        }
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
            for m in members {
                reach(&m.tc, out, depth + 1);
            }
        }
        TypeCode::Union { discriminator, cases, .. } => {
            reach(discriminator, out, depth + 1);
            for c in cases {
                reach(&c.tc, out, depth + 1);
            }
        }
        _ => {}
    }
}

/// The repository id a TypeCode carries, for the kinds that carry one.
fn named_id(tc: &TypeCode) -> Option<&str> {
    match tc {
        TypeCode::Struct { id, .. }
        | TypeCode::Union { id, .. }
        | TypeCode::Enum { id, .. }
        | TypeCode::Except { id, .. }
        | TypeCode::Alias { id, .. }
        | TypeCode::ObjRef { id, .. }
        | TypeCode::Value { id, .. }
        | TypeCode::AbstractInterface { id, .. }
        | TypeCode::Native { id, .. } => Some(id),
        _ => None,
    }
}

/// What kind of declaration a TypeCode is, in IDL's own words.
///
/// The same vocabulary `Contained::describe` answers with as a
/// `DefinitionKind`, spelled for a reader rather than as a wire enumerator.
pub fn type_keyword(tc: &TypeCode) -> &'static str {
    match tc {
        TypeCode::Struct { .. } => "struct",
        TypeCode::Union { .. } => "union",
        TypeCode::Enum { .. } => "enum",
        TypeCode::Except { .. } => "exception",
        TypeCode::Alias { .. } => "typedef",
        TypeCode::Value { .. } => "valuetype",
        TypeCode::AbstractInterface { .. } => "abstract interface",
        TypeCode::Native { .. } => "native",
        _ => "typedef",
    }
}

/// `describe_type`'s answer for one registered type.
///
/// The first four fields are **computed by
/// [`orbweaver_registry::ifr::contained_of`]**, which is the same function
/// `Contained::describe` fills its `TypeDescription` from. D024 §5 says the
/// local and the remote answer must agree; the way two answers agree is by
/// being one answer, so they are not two implementations held equal by a test
/// but one implementation reached from two places. The test in
/// `orbweaver-test` proves it end to end over the wire, which is the part a
/// shared function cannot prove on its own.
pub fn describe_type_json(registry: &Registry, target: &str, tc: &TypeCode) -> Json {
    let (name, defined_in, version) = orbweaver_registry::ifr::contained_of(registry, target);
    let mut out: BTreeMap<String, Json> = BTreeMap::new();
    out.insert("id".into(), s(target));
    out.insert("name".into(), s(name));
    out.insert("defined_in".into(), s(defined_in));
    out.insert("version".into(), s(version));
    out.insert("kind".into(), s(type_keyword(tc)));
    out.insert("type".into(), s(crate::type_name(tc)));

    // The members, in the shape the kind actually has. A reader that only ever
    // met `describe_interface` gets the same vocabulary here: a name, a type,
    // and the annotations the author wrote.
    match tc {
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
            out.insert(
                "members".into(),
                Json::Array(
                    members
                        .iter()
                        .map(|m| obj([("name", s(&m.name)), ("type", s(crate::type_name(&m.tc)))]))
                        .collect(),
                ),
            );
        }
        TypeCode::Union { discriminator, cases, .. } => {
            out.insert("discriminator".into(), s(crate::type_name(discriminator)));
            out.insert(
                "cases".into(),
                Json::Array(
                    cases
                        .iter()
                        .map(|c| obj([("name", s(&c.name)), ("type", s(crate::type_name(&c.tc)))]))
                        .collect(),
                ),
            );
        }
        TypeCode::Enum { members, .. } => {
            out.insert(
                "enumerators".into(),
                Json::Array(members.iter().map(|m| s(m.as_str())).collect()),
            );
        }
        TypeCode::Alias { aliased, .. } => {
            out.insert("aliases".into(), s(crate::type_name(aliased)));
        }
        _ => {}
    }

    if let Some(ann) = registry.annotations(target) {
        out.insert("annotations".into(), crate::annotations(ann));
    }
    Json::Object(out)
}

/// The wire gate a contract tool judges by, and why it is not the library
/// default.
///
/// `orbweaver_forge::validate` defaults to [`WireGate::Deferred`], which makes
/// a `valuetype` a warning. At **this** boundary the honest answer is
/// [`WireGate::V1`]: an agent asking whether a contract is good is asking
/// whether it can be served and called here, and a construct this wire cannot
/// carry is an error to that question however legal it is as IDL. The §4.4
/// sentences those findings carry already say the difference between *not yet*
/// and *never*.
pub const TOOL_WIRE_GATE: WireGate = WireGate::V1;

/// `validate_contract`.
pub fn validate_contract(source: &str) -> Json {
    let report = orbweaver_forge::validate_source_for(
        // Anonymous, and that is the honest shape: the text came down a pipe
        // from a model, not off a disk, so an unresolvable `#include` fails
        // with the reason rather than against a directory nobody named.
        Source::anonymous(source),
        &orbweaver_idl::SearchPath::default(),
        TOOL_WIRE_GATE,
    );
    with_repair(&report, BTreeMap::new())
}

/// `diff_contract`.
///
/// Two halves in one answer, because D024 §5 asks for the verdict table **and**
/// its reasons and they are different readers' needs. `findings` is the S4
/// shape a repair loop consumes; `changes` is the table an operator reads, with
/// `why` on every row — the `Change::why` the differ itself wrote, never a
/// sentence retyped here.
pub fn diff_contract(released: &str, proposed: &str) -> Json {
    let report = orbweaver_forge::validate_against_for(proposed, released, TOOL_WIRE_GATE);

    let mut extra: BTreeMap<String, Json> = BTreeMap::new();
    match (registry_of(released), registry_of(proposed)) {
        (Ok(old), Ok(new)) => {
            let changes = orbweaver_registry::diff::diff(&old, &new);
            let worst = changes.iter().map(|c| c.verdict).max();
            extra.insert(
                "changes".into(),
                Json::Array(
                    changes
                        .iter()
                        .map(|c| {
                            obj([
                                ("id", s(&c.id)),
                                ("what", s(&c.what)),
                                ("why", s(c.why)),
                                ("verdict", s(c.verdict.label())),
                            ])
                        })
                        .collect(),
                ),
            );
            // Named from the differ's own label so the word an agent reads and
            // the word an operator's gate acts on cannot come apart.
            extra.insert("verdict".into(), s(worst.map(|v| v.label()).unwrap_or("compatible")));
            extra.insert(
                "blocks_release".into(),
                Json::Bool(changes.iter().any(|c| c.verdict.blocks_release())),
            );
        }
        // One side would not load. The findings already say why — S4 ran on the
        // same text — and inventing a verdict over a contract we could not read
        // would be the bare verdict D024 §3 refuses.
        _ => {
            extra.insert("changes".into(), Json::Array(Vec::new()));
        }
    }
    with_repair(&report, extra)
}

/// `preview_generation`.
///
/// **The skipped half is the honest half** and it is why this is a tool rather
/// than a curiosity: an agent that writes a `valuetype` gets a package that
/// silently lacks it, and the reason it lacks it is a sentence
/// `orbweaver-dynamic` already owns. Both targets are reported, because the
/// same contract skips different things in each and a preview of one is a
/// preview of half the answer.
pub fn preview_generation(source: &str) -> Json {
    let report = orbweaver_forge::validate_source_for(
        Source::anonymous(source),
        &orbweaver_idl::SearchPath::default(),
        TOOL_WIRE_GATE,
    );
    let mut extra: BTreeMap<String, Json> = BTreeMap::new();
    match registry_of(source) {
        Ok(registry) => {
            let rust = orbweaver_gen::emit(&registry, "preview");
            let python = orbweaver_gen::python::emit_python(&registry, "preview");
            extra.insert(
                "targets".into(),
                Json::Array(vec![
                    target_json("rust", rust.emitted, vec!["preview.rs".to_owned()], &rust.skipped),
                    target_json(
                        "python",
                        python.emitted,
                        python.files.keys().cloned().collect(),
                        &python.skipped,
                    ),
                ]),
            );
        }
        Err(_) => {
            // Nothing would be generated, and the findings say why. An empty
            // target list with a report full of errors is not ambiguous.
            extra.insert("targets".into(), Json::Array(Vec::new()));
        }
    }
    with_repair(&report, extra)
}

fn target_json(
    name: &str,
    emitted: usize,
    files: Vec<String>,
    skipped: &[(String, String)],
) -> Json {
    obj([
        ("target", s(name)),
        ("emitted", Json::Number(emitted.to_string())),
        ("files", Json::Array(files.into_iter().map(Json::String).collect())),
        (
            "skipped",
            Json::Array(
                skipped.iter().map(|(id, why)| obj([("id", s(id)), ("why", s(why))])).collect(),
            ),
        ),
    ])
}

/// Loads one contract's text into a registry, or gives up.
///
/// The error is discarded on purpose at every call site: S4 has already run
/// over the same text and its findings say what is wrong with positions on
/// them, so a second, worse sentence here would be a competing diagnosis.
fn registry_of(source: &str) -> Result<Registry, ()> {
    let spec = orbweaver_idl::check(source).map_err(|_| ())?;
    let mut registry = Registry::new();
    registry.load(&spec).map_err(|_| ())?;
    Ok(registry)
}

/// Every contract tool's answer has the same head: the report, and the prompt a
/// repair loop feeds on.
///
/// `repair_prompt` is included rather than left for the agent to assemble
/// because it is the one string the self-repair loop actually consumes, and an
/// agent that has to rebuild it from `findings` will rebuild it differently
/// from the pipeline that measured 65% → 100% with it.
fn with_repair(report: &Report, extra: BTreeMap<String, Json>) -> Json {
    let Json::Object(mut out) = report.to_json() else {
        unreachable!("Report::to_json is an object")
    };
    out.insert("repair_prompt".into(), s(report.repair_prompt()));
    out.insert(
        "errors".into(),
        Json::Number(
            report.findings.iter().filter(|f| f.severity == Severity::Error).count().to_string(),
        ),
    );
    out.extend(extra);
    Json::Object(out)
}

/// The refusal a `describe_type` gives for an id it will not describe.
///
/// **Two answers, and the line between them is what the caller can already
/// see.**
///
/// For an id the exposure *does* expose that simply is not a type — an
/// interface, nearly always — the honest answer names the kind, because the
/// caller could have learned it from `describe_interface` and telling it "not
/// exposed" about something the operator just exposed sends it hunting through
/// an allowlist for a problem that is in its request. That is the RC-4
/// misdirection, and it was live here until the shipped binary was driven end
/// to end: `describe_type("IDL:bank/Account:1.0")` answered *"is not
/// exposed"* about an interface passed to `--expose` on the same command line.
///
/// Everything else — a type nothing exposed reaches, an id nobody declared —
/// gets **one** indistinguishable answer, for the reason `describe_interface`
/// gives: a refusal that told those apart would confirm the existence of
/// something behind a gate the caller never got through.
pub fn undescribable(registry: &Registry, exposure: &Exposure, target: &str) -> Denied {
    if exposure.exposes(target) {
        if let Some(kind) = visible_kind(registry, target) {
            return Denied::NotAType { id: target.to_owned(), kind };
        }
    }
    Denied::InterfaceNotExposed(target.to_owned())
}

/// What the catalog says an id is, for an id the caller is already permitted
/// to see. `None` for anything else, so no caller learns a kind it could not
/// have learned another way.
fn visible_kind(registry: &Registry, id: &str) -> Option<String> {
    match registry.get(id)? {
        Entry::Interface(_) => Some("an interface".to_owned()),
        Entry::Const { .. } => Some("a constant".to_owned()),
        Entry::Type(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_surface_is_a_contract_that_parses_and_annotates_every_operation() {
        let registry = contract_tools_registry();
        let iface = registry.interface(CONTRACT_TOOLS_ID).expect("the surface is registered");
        let _ = iface;
        for tool in CONTRACT_TOOLS {
            let (_, sig) = registry
                .resolve_operation(CONTRACT_TOOLS_ID, tool)
                .unwrap_or_else(|| panic!("{tool} is declared"));
            // The approval stage reads this and nothing else; a tool that lost
            // its annotation would be refused rather than silently allowed.
            assert_eq!(
                sig.annotations.get("ai_effect").map(String::as_str),
                Some("read_only"),
                "{tool} must state its effect in the contract"
            );
            assert!(sig.annotations.contains_key("ai_desc"), "{tool} needs an ai_desc");
        }
    }

    /// The surface declares exactly the four D024 §5 names and nothing else —
    /// notably not a registration.
    #[test]
    fn the_surface_declares_exactly_the_four_and_no_registration() {
        let registry = contract_tools_registry();
        let mut names: Vec<String> = crate::resolved_operations(registry, CONTRACT_TOOLS_ID)
            .into_iter()
            .map(|(n, _, _)| n)
            .collect();
        names.sort();
        let mut expected: Vec<String> = CONTRACT_TOOLS.iter().map(|s| (*s).to_owned()).collect();
        expected.sort();
        assert_eq!(names, expected);
    }

    #[test]
    fn validation_returns_findings_with_positions_and_never_a_bare_verdict() {
        // A case-insensitive clash: the dominant generation failure.
        let out = validate_contract("module m { struct Version { unsigned long version; }; };");
        assert_eq!(out.get("ok"), Some(&Json::Bool(false)));
        let Some(Json::Array(findings)) = out.get("findings") else { panic!("{out}") };
        assert!(!findings.is_empty());
        let f = &findings[0];
        assert!(f.get("rule").is_some(), "a finding names its rule: {f}");
        assert!(f.get("line").is_some(), "a finding names its position: {f}");
        assert!(out.get("repair_prompt").is_some(), "the repair loop's string is present");
    }

    #[test]
    fn a_diff_reports_every_change_with_the_reason_the_differ_wrote() {
        let released = "module b { interface A { long f(in long x); }; };";
        let proposed = "module b { interface A { long f(in long x, in long y); }; };";
        let out = diff_contract(released, proposed);
        let Some(Json::Array(changes)) = out.get("changes") else { panic!("{out}") };
        assert!(!changes.is_empty(), "a changed signature is a change: {out}");
        for c in changes {
            assert!(c.get("why").is_some(), "every row carries its reason: {c}");
            assert!(c.get("verdict").is_some(), "{c}");
        }
        assert_eq!(out.get("blocks_release"), Some(&Json::Bool(true)), "{out}");
    }

    /// The half D024 §5 calls honest: what would be skipped, and why.
    #[test]
    fn a_preview_says_what_would_be_skipped_and_why() {
        let out = preview_generation(
            "module w { valuetype Money { public long units; }; \
             interface Wallet { Money balance(); }; };",
        );
        let Some(Json::Array(targets)) = out.get("targets") else { panic!("{out}") };
        assert_eq!(targets.len(), 2, "both targets are previewed: {out}");
        for t in targets {
            let Some(Json::Array(skipped)) = t.get("skipped") else { panic!("{t}") };
            assert!(!skipped.is_empty(), "a valuetype is skipped by both targets: {t}");
            for sk in skipped {
                let why = sk.get("why").and_then(Json::as_str).unwrap_or("");
                // The reason is the one orbweaver-dynamic owns, not a sentence
                // this crate wrote.
                assert!(why.contains("§4.4") || why.contains("no wire form"), "{sk}");
            }
        }
    }

    fn estate() -> Registry {
        let spec = orbweaver_idl::parse(
            "module e {
               struct Secret { long code; };
               struct Visible { long n; };
               interface Open { Visible read(); };
               interface Closed { Secret peek(); };
             };",
        )
        .expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        r
    }

    /// The whole point of the second gate: a type behind an unexposed interface
    /// must not be describable, or `describe_type` is an enumeration tool for
    /// the estate.
    #[test]
    fn a_type_is_reachable_only_through_an_exposed_interface() {
        let r = estate();
        let open = Exposure::nothing().allow_interface("IDL:e/Open:1.0");
        assert!(type_is_reachable(&r, &open, "IDL:e/Visible:1.0"));
        assert!(
            !type_is_reachable(&r, &open, "IDL:e/Secret:1.0"),
            "Secret is only reachable through an interface nobody exposed"
        );
        assert!(!type_is_reachable(&r, &Exposure::nothing(), "IDL:e/Visible:1.0"));
    }

    /// **The misdirection, pinned.** `describe_type` on an exposed *interface*
    /// must not answer "not exposed" — the operator exposed it, and that
    /// answer sends the reader into the allowlist after a problem that is in
    /// the request.
    ///
    /// Found by driving the shipped binary, not by a unit test: the whole
    /// suite was green while `describe_type("IDL:e/Open:1.0")` under
    /// `--expose IDL:e/Open:1.0` said the interface was not exposed.
    #[test]
    fn describe_type_on_an_exposed_interface_does_not_claim_it_is_unexposed() {
        let r = estate();
        let open = Exposure::nothing().allow_interface("IDL:e/Open:1.0");

        let why = undescribable(&r, &open, "IDL:e/Open:1.0");
        assert!(matches!(why, Denied::NotAType { .. }), "{why:?}");
        let shown = why.to_string();
        assert!(!shown.contains("is not exposed"), "the RC-4 misdirection is back: {shown}");
        assert!(shown.contains("an interface"), "{shown}");
        assert!(
            shown.contains("describe_interface"),
            "it must name the tool that reads one: {shown}"
        );
    }

    /// And the oracle-safe half is unchanged: for anything the caller cannot
    /// already see, the two cases stay indistinguishable.
    #[test]
    fn a_hidden_type_and_a_type_that_does_not_exist_answer_alike() {
        let r = estate();
        let open = Exposure::nothing().allow_interface("IDL:e/Open:1.0");

        let hidden = undescribable(&r, &open, "IDL:e/Secret:1.0").to_string();
        let absent = undescribable(&r, &open, "IDL:e/NoSuchThing:1.0").to_string();
        assert_eq!(
            hidden.replace("Secret", "X"),
            absent.replace("NoSuchThing", "X"),
            "a refusal must not say which of the two it was"
        );
        // And an interface the exposure does *not* name stays in that class
        // too, or the kind becomes an oracle of its own.
        let unexposed = undescribable(&r, &open, "IDL:e/Closed:1.0").to_string();
        assert!(!unexposed.contains("an interface"), "{unexposed}");
    }

    #[test]
    fn a_description_carries_the_triple_the_ifr_answers_with() {
        let r = estate();
        let tc = r.typecode("IDL:e/Visible:1.0").expect("registered");
        let d = describe_type_json(&r, "IDL:e/Visible:1.0", tc);
        assert_eq!(d.get("name"), Some(&Json::String("Visible".into())));
        assert_eq!(d.get("defined_in"), Some(&Json::String("IDL:e:1.0".into())));
        assert_eq!(d.get("version"), Some(&Json::String("1.0".into())));
        assert_eq!(d.get("kind"), Some(&Json::String("struct".into())));
        let Some(Json::Array(members)) = d.get("members") else { panic!("{d}") };
        assert_eq!(members.len(), 1);
    }
}
