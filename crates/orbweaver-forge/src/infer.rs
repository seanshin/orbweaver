//! S3i — annotate what we did not write: SIDL **proposals** for ingested
//! interfaces.
//!
//! [`crate::annotate`] is S3 over IDL a model just produced from a brief we
//! hold. This module is S3 over the other input path: an interface described to
//! us by a foreign Interface Repository ([`orbweaver_registry::ingest`]). The
//! gap it closes is stated in that module's own documentation, and it is the
//! last broken link in the ingestion path:
//!
//! > **No SIDL annotations exist on the wire.** The IR carries no `ai_effect`,
//! > `ai_authz` or `ai_desc`, so every ingested operation arrives with an empty
//! > annotation map. The guard's destructive-approval gate and scope checks
//! > have nothing to key on […]
//!
//! An ingested interface therefore cannot be exposed to an agent safely, not
//! because a switch is off but because the gates are keyed on annotations that
//! do not exist. Something has to write them. Nobody who can be trusted to
//! write them is available: the people who wrote the service are, by the
//! premise of ingestion, gone.
//!
//! # The honesty problem is the whole design, not the plumbing
//!
//! An annotation inferred here is **a claim about someone else's service, made
//! by reading names and types.** It is not a fact. `ai_effect` and `ai_authz`
//! are inputs to an authorization decision ([`orbweaver_mcp::policy`]), so
//! treating an inference as a fact means the exposure gate is keyed on a guess.
//!
//! Three rules follow, and they are enforced rather than requested:
//!
//! 1. **An inference never occupies a key a gate reads.** Values land under
//!    [`INFERRED_PREFIX`] — `inferred_effect`, `inferred_authz`,
//!    `inferred_desc` — beside [`MARK_EVIDENCE`] and [`MARK_STATUS`]. The
//!    registry carries unknown keys through to every consumer, so the mark
//!    travels wherever the annotation travels; and `policy::required_scopes`
//!    and `policy::effect_refusal` read `ai_authz` and `ai_effect`, so an
//!    inference enforces exactly nothing until a human moves it. That is not a
//!    weakness of the scheme, it *is* the scheme: [`Provenance`] is answerable
//!    from the annotation map alone, which a two-value `ai_effect` string could
//!    never be.
//!
//! 2. **An inference may propose a value that closes a gate, never one that
//!    opens one.** `destructive` is proposable; `read_only`, `idempotent` and
//!    `safe` are refused ([`RULES`], `si/ungating-claim`). The asymmetry is the
//!    argument: a wrong `destructive` costs a human an approval click, and a
//!    wrong `read_only` **removes** the approval gate on an operation that
//!    moves money — `policy::effect_refusal` returns `None` for it and the call
//!    goes through. Since the evidence is a name, and a name cannot say whether
//!    an operation writes, only one of those two errors may be reachable.
//!
//! 3. **Where the evidence is silent, the output is `unknown`.** Not a
//!    fallback: [`Evidence::is_silent`] is checked by the gate, and an effect
//!    claim over a name like `process`, `execute` or `handle` is an error
//!    (`si/effect-without-evidence`). A stage that never says "I don't know" is
//!    a stage that is guessing, so [`Proposal::unknown_rate`] is reported
//!    beside the first-pass rate rather than buried.
//!
//! # What an inference cannot know, stated plainly
//!
//! The name says nothing about whether the operation writes to a database, and
//! no amount of prompting fixes that. `settle` may post a ledger row or format
//! a string. `get_report` may bill the caller. `ping` may reset a watchdog that
//! fails a cluster over. The IR carries no comments, no source and no
//! behaviour — only identifiers and `TypeCode`s — and the peer that serves it
//! is not even the peer that implements the object
//! (`orbweaver_registry::ingest`, "the IR and the object are different peers").
//! Everything in this module is a triage aid for a human who will go and look;
//! it is never evidence about the service.
//!
//! # Visible, not merely default-off
//!
//! [`worksheet`] emits one row per ingested operation — including the ones no
//! proposal covers — so an un-approved inference is a line an operator reads
//! rather than an absent field nobody notices. [`unapproved`] is the same list
//! as data, and [`exposure_refusal`] is the single question an exposure
//! decision should ask before allowlisting an id.
//!
//! **추론 주석은 남의 서비스에 대한 주장이지 사실이 아니다.** 그래서 게이트가
//! 읽는 키(`ai_effect`, `ai_authz`)를 절대 차지하지 않고, 게이트를 여는 값은
//! 제안조차 할 수 없으며, 근거가 없으면 답은 `unknown`이다. 승인은 사람이 하고,
//! 승인 전 상태는 워크시트의 한 줄로 **보이게** 남는다.

use std::collections::{BTreeMap, BTreeSet};

use orbweaver_dynamic::json::Json;
use orbweaver_giop::typecode::TypeCode;
use orbweaver_registry::{Entry, InterfaceEntry, OperationSig, ParamDirection, Registry};

use crate::pipeline::{Stage, StageId};
use crate::{Finding, Report, Severity};

/// The prefix every inferred value carries.
///
/// Chosen so that no SIDL consumer reads it: `orbweaver-mcp`'s policy gate
/// keys on `ai_effect` and `ai_authz`, `orbweaver-test`'s `contract-check`
/// only judges keys beginning `ai_`, and both therefore see an inferred value
/// as what it is — data attached to the contract that no decision consults.
pub const INFERRED_PREFIX: &str = "inferred_";

/// Where an inference came from: the ingestion source label.
pub const MARK_SOURCE: &str = "inferred_source";

/// What the inference was drawn from, in words, quoting only the subject.
pub const MARK_EVIDENCE: &str = "inferred_evidence";

/// Whether a human has taken responsibility for the inference yet.
///
/// [`UNAPPROVED`] until someone runs [`approve`], after which it records who
/// and when. It is never removed: an annotation that began as a guess stays
/// distinguishable from one an author wrote, for as long as it exists.
pub const MARK_STATUS: &str = "inferred_status";

/// The value [`MARK_STATUS`] carries before anybody has approved anything.
pub const UNAPPROVED: &str = "unapproved";

/// How much of the claim the *checker* can corroborate, computed by
/// [`apply`] from the signature and never taken from the producer.
///
/// Its three values are [`BASIS_DECLINED`], [`BASIS_RECOGNISED`] and
/// [`BASIS_UNRECOGNISED`], and the third exists because of a measured failure:
/// see [`RULES`]' `si/unnamed-verb` entry.
pub const MARK_BASIS: &str = "inferred_basis";

/// [`MARK_BASIS`] when the stage declined to claim an effect.
pub const BASIS_DECLINED: &str = "declined — the stage claimed no effect";

/// [`MARK_BASIS`] when the operation's name contains a verb the checker knows
/// to be mutating.
pub const BASIS_RECOGNISED: &str = "recognised-verb";

/// [`MARK_BASIS`] when an effect is claimed from a word the checker's list does
/// not contain.
///
/// The claim is **kept**, not refused, and the reviewer is told exactly this:
/// the reading is the producer's alone. `moe::ExpertLoader::evict` is the
/// measured case — it destroys everything routed to an expert, and no verb list
/// this project has ever written contained the word.
pub const BASIS_UNRECOGNISED: &str = "unrecognised-verb — the checker's word list does not contain \
                                      this name's verb, so the reading is the producer's alone \
                                      and a human should read the name themselves";

/// The `ai_*` keys an inference is forbidden to occupy, and why: each one is
/// read by a gate.
///
/// `ai_effect` decides whether a call needs a human approval and `ai_authz`
/// decides which scope a caller must hold (`orbweaver_mcp::policy`). `ai_desc`
/// is in the list for a different reason — it is what an agent reads when it
/// chooses an operation, so a fabricated description steers a tool call as
/// surely as a permission does.
pub const GATE_KEYS: [&str; 3] = ["ai_effect", "ai_authz", "ai_desc"];

/// `ai_effect` values that would *remove* a gate, and which may therefore never
/// be inferred.
///
/// Mirrored from [`crate::annotate::UNGATED_EFFECTS`], which mirrors
/// `orbweaver-mcp`'s `policy::is_harmless` — the private predicate
/// `policy::effect_refusal` asks before it returns `None`. The test
/// `the_ungating_set_is_the_policy_gates_ungated_set` pins the local copy
/// against `annotate`'s, and `the_ungating_set_is_what_the_gate_lets_through`
/// pins it against the gate itself, over the crate boundary and through the
/// public API — because `is_harmless` is private, so the only honest way to
/// check what it holds is to run it.
///
/// Both doc comments named a `policy::destructive_effect` until 2026-08-25.
/// That function was **removed on 2026-08-14** (8cbf5ce, "absence is not
/// permission") and split into `effect_refusal` and `is_harmless`. What the
/// sentence *described* stayed true for eleven days, which is why nothing
/// noticed; what it *named* was gone, so a reader following it found nothing.
/// A comment is a sentence about another crate's code and no compiler reads
/// one — hence the behavioural pin below, which does.
pub const UNGATING: [&str; 4] = ["read_only", "readonly", "idempotent", "safe"];

/// The whole `inferred_effect` vocabulary: one gating value, and the honest
/// answer.
pub const EFFECT_VALUES: [&str; 2] = ["destructive", "unknown"];

/// Verbs whose presence in an operation name is evidence that it changes
/// something.
///
/// Mirrored from [`crate::annotate`]'s list, which mirrors `orbweaver-test`'s.
/// A verb here licenses at most a `destructive` proposal; nothing licenses an
/// ungating one.
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

/// Verbs whose presence is evidence the operation is *about* reading.
///
/// Deliberately **not** enough to propose `read_only`: `get_statement` may bill
/// the caller and `describe` may write an audit row. Its only job is to make
/// [`Evidence::is_silent`] false, so the name is at least *about* something —
/// which is the difference between "the model read a name" and "the model read
/// nothing and answered anyway".
const READING_VERBS: [&str; 14] = [
    "get", "find", "list", "query", "fetch", "read", "lookup", "describe", "count", "search",
    "is_", "has_", "peek", "status",
];

// ── evidence: the only facts that exist ──────────────────────────────────────

/// Everything about one ingested operation that is actually knowable here.
///
/// Built from the registry entry alone, deterministically, before any model is
/// asked anything. It exists so that "what the inference was drawn from" is a
/// computed fact rather than a sentence a model wrote about itself — the gate
/// checks a proposal's evidence against this, not against a claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    /// Operation name, as the peer reported it.
    pub operation: String,
    /// The mutating verb the name contains, if any.
    pub mutating_verb: Option<&'static str>,
    /// The reading verb the name contains, if any.
    pub reading_verb: Option<&'static str>,
    /// Whether a reply is expected at all.
    pub oneway: bool,
    /// Rendered return type.
    pub returns: String,
    /// Whether the return value or an `out`/`inout` parameter hands the caller
    /// an object reference — a bearer address (PLAN §4.7), which widens what a
    /// caller can reach whatever the operation itself does.
    pub hands_out_reference: bool,
    /// Rendered parameters, in declaration order.
    pub params: Vec<String>,
    /// Repository ids in the raises clause.
    pub raises: Vec<String>,
}

impl Evidence {
    /// The evidence for one operation of an ingested interface.
    pub fn of(name: &str, sig: &OperationSig) -> Evidence {
        let lower = name.to_ascii_lowercase();
        let hands_out = is_reference(&sig.returns)
            || sig.params.iter().any(|p| p.direction != ParamDirection::In && is_reference(&p.tc));
        Evidence {
            operation: name.to_owned(),
            mutating_verb: MUTATING_VERBS.iter().copied().find(|v| lower.contains(v)),
            reading_verb: READING_VERBS.iter().copied().find(|v| lower.contains(v)),
            oneway: sig.oneway,
            returns: render_type(&sig.returns),
            hands_out_reference: hands_out,
            params: sig
                .params
                .iter()
                .map(|p| format!("{} {} {}", direction(p.direction), render_type(&p.tc), p.name))
                .collect(),
            raises: sig.raises.clone(),
        }
    }

    /// Whether the name says nothing at all about what the operation does.
    ///
    /// This is the trigger for the honest answer. `process`, `execute`,
    /// `handle`, `submit`, `run`, `apply` — every one of them is a real
    /// operation name on a real legacy interface, and every one of them is
    /// compatible with reading a cache and with wiring money. When this is
    /// true, `unknown` is the only `inferred_effect` the gate accepts.
    pub fn is_silent(&self) -> bool {
        self.mutating_verb.is_none() && self.reading_verb.is_none()
    }

    /// The evidence as one line, which is what a model is shown and what a
    /// proposal's own `evidence` text is checked against.
    pub fn to_line(&self) -> String {
        let mut out = format!("{}({}) -> {}", self.operation, self.params.join(", "), self.returns);
        if self.oneway {
            out.push_str(" [oneway]");
        }
        if !self.raises.is_empty() {
            out.push_str(&format!(" raises {}", self.raises.join(", ")));
        }
        if let Some(v) = self.mutating_verb {
            out.push_str(&format!(" [name contains {v:?}]"));
        }
        if let Some(v) = self.reading_verb {
            out.push_str(&format!(" [name contains {v:?}]"));
        }
        if self.hands_out_reference {
            out.push_str(" [hands out an object reference]");
        }
        if self.is_silent() {
            out.push_str(" [the name says nothing about effect]");
        }
        out
    }
}

fn direction(d: ParamDirection) -> &'static str {
    match d {
        ParamDirection::In => "in",
        ParamDirection::Out => "out",
        ParamDirection::InOut => "inout",
    }
}

/// A compact rendering of a `TypeCode`, for a human and for a prompt.
///
/// # Every variant carries a verdict, and the match is exhaustive on purpose
///
/// This function ended in `_ => "<unnamed type>"` until 2026-08-25, under a
/// comment claiming *"everything left is something v1 does not marshal or does
/// not name"*. **Half of that was false, and the false half reached a reader.**
/// Two of the nine variants the catch-all swallowed are marshalled by v1 in
/// both directions:
///
/// - `long double` — `orbweaver_dynamic` has `TypeCode::LongDouble` arms in the
///   value path and in AnyJSON, and `orbweaver_gen`'s `rust_type` maps it to
///   `orbweaver_gen::rt::LongDouble`.
/// - `::CORBA::TypeCode` — and this one is worse, because it was *deliberately
///   rescued*: `orbweaver_registry`'s `is_corba_typecode` exists to stop the
///   predeclared name falling through to `void`, on the argument that a silent
///   `void` is not a cosmetic default — `describe_interface`, the operation the
///   Interface Repository facade exists for, returns one. This renderer then
///   called the type that fix rescued `<unnamed type>`.
///
/// Both feed the operation signature in the **prompt a model and a human
/// read** ([`Evidence::to_line`], [`Subject::to_prompt`]), so
/// `long double price()` and `::CORBA::TypeCode describe()` both arrived as
/// `<unnamed type> op(...)`.
///
/// The half worth keeping is the *reason* for a placeholder: **a prompt that
/// invents a type name teaches the producer to quote one back** — `si/quote-only`
/// checks a proposal against [`subject_haystack`], so an invented name would be
/// quotable text. That reason is true of exactly three variants, the three no
/// declaration can produce, and of nothing else. Each of those three says why
/// *it* has no name rather than sharing one sentence with eight others.
///
/// So there is no catch-all. Exhaustiveness is what makes a thirty-fourth
/// `TypeCode` variant go **red here** rather than silently acquiring
/// `<unnamed type>` — the argument `orbweaver_dynamic::dynany::default_within`
/// makes for the same shape, where the wrong answer was a default value rather
/// than a name.
///
/// # What this function deliberately does not say
///
/// Whether the v1 wire carries the type. That fact has a home — S4 reports it
/// as `wire/deferred-type` and the refusal sentence belongs to
/// `orbweaver_dynamic::deferred_wire_head` and its unmarshallable counterpart —
/// and restating it in a signature line would be one more place for it to drift
/// from. What this function borrows from that home is the four constructs'
/// **spelling**, by calling the functions that own it, so a refusal a reader
/// meets later reads back the same string this prompt showed them.
fn render_type(tc: &TypeCode) -> String {
    match tc {
        // ── the base types, spelled as IDL spells them ───────────────────────
        TypeCode::Void => "void".into(),
        TypeCode::Short => "short".into(),
        TypeCode::Long => "long".into(),
        TypeCode::UShort => "unsigned short".into(),
        TypeCode::ULong => "unsigned long".into(),
        TypeCode::LongLong => "long long".into(),
        TypeCode::ULongLong => "unsigned long long".into(),
        TypeCode::Float => "float".into(),
        TypeCode::Double => "double".into(),
        // `long double`, not `longdouble`: this function renders IDL, and its
        // neighbours two lines up already write `unsigned long` and `long long`
        // with the space. (D016 §4 A2 records seven private namers for this
        // type across the workspace that do not agree with each other, and
        // plans one `TypeCode::idl_name()` owned by `orbweaver-giop` to replace
        // them. Not measured here — this arm is the IDL spelling and says so.)
        TypeCode::LongDouble => "long double".into(),
        TypeCode::Boolean => "boolean".into(),
        TypeCode::Char => "char".into(),
        TypeCode::WChar => "wchar".into(),
        TypeCode::Octet => "octet".into(),
        TypeCode::Any => "any".into(),
        // The qualified spelling CLAUDE.md requires and the front end
        // predeclares the name under; anything shorter is a name the reader
        // cannot write back into a contract.
        TypeCode::TypeCode => "::CORBA::TypeCode".into(),
        TypeCode::String(0) => "string".into(),
        TypeCode::String(n) => format!("string<{n}>"),
        TypeCode::WString(0) => "wstring".into(),
        TypeCode::WString(n) => format!("wstring<{n}>"),
        // ── the constructed types, which carry the name the contract wrote ───
        TypeCode::Sequence { element, .. } => format!("sequence<{}>", render_type(element)),
        TypeCode::Array { element, length } => format!("{}[{length}]", render_type(element)),
        TypeCode::ObjRef { name, .. } if name.is_empty() => "Object".into(),
        TypeCode::ObjRef { name, .. } => name.clone(),
        TypeCode::Struct { name, .. }
        | TypeCode::Union { name, .. }
        | TypeCode::Enum { name, .. }
        | TypeCode::Alias { name, .. }
        | TypeCode::Except { name, .. } => name.clone(),
        // ── the four the v1 wire refuses, spelled by the layer that refuses ──
        //
        // Named, not hidden. A reader who cannot call `balance()` needs to know
        // it returns `valuetype Money (IDL:m/Money:1.0)`, and the string is the
        // one the marshaller and both emitters will refuse it by — the id is in
        // it because two modules can declare `Describable` and a reader of two
        // refusals could not otherwise tell one type from two.
        TypeCode::Fixed { digits, scale } => orbweaver_dynamic::fixed_subject(*digits, *scale),
        TypeCode::Value { name, id, .. } => orbweaver_dynamic::valuetype_subject(name, id),
        TypeCode::AbstractInterface { name, id } => {
            orbweaver_dynamic::abstract_interface_subject(name, id)
        }
        TypeCode::Native { name, id } => orbweaver_dynamic::native_subject(name, id),
        // ── the three no declaration produces: a placeholder, each with the
        //    reason that is true of it ─────────────────────────────────────────
        //
        // `tk_null` is the empty TypeCode — the content type of an `any` nobody
        // filled in, and a valuetype's absent base. No IDL names it, so there is
        // no name to give and nothing for a reader to look up.
        TypeCode::Null => "<tk_null: the empty TypeCode, which no declaration writes>".into(),
        // `Principal` was withdrawn from CORBA; no contract can declare one and
        // no `#include` will bring it back. It can only arrive from a peer, and
        // "go and ask the peer" is the whole of the advice.
        TypeCode::Principal => {
            "<Principal: withdrawn from CORBA, so no contract declares one>".into()
        }
        // An indirection that pointed at a `TypeCode` still being decoded. It is
        // not a type, it is a marker naming one — so it renders the id it names
        // rather than a name, which is the actionable half and all there is.
        TypeCode::Recursive(id) => format!("<recursion back to {id}, which has no finite form>"),
    }
}

/// Whether a type is, or contains, a live object reference.
///
/// Exhaustive for [`render_type`]'s reason and with a sharper consequence: the
/// answer becomes [`Evidence::hands_out_reference`], a caution a reader weighs
/// *against* a call, so a variant that quietly answered `false` **removed** a
/// caution rather than adding one.
///
/// It ended in `_ => false` until 2026-08-25, and that arm was wrong for five
/// variants. A `struct`, `union`, `exception` or `valuetype` whose *member* is
/// an object reference hands one out exactly as a bare return does — `Object
/// get_root()` was marked and `struct Handle { Object it; } get_root()` was
/// not — and an abstract interface is a reference half the time by
/// construction: on the wire it is the union of a value and a reference.
///
/// Two answers are `false` for a stated reason rather than by default:
///
/// - **`any`** carries its content type as a *runtime value*. Whether it holds
///   a reference is not knowable from a signature, and marking every `any`
///   would make the caution mean "somewhere, perhaps" wherever it appeared.
/// - **a recursion marker** names a type this walk is already inside. Every
///   member reachable through it is a member of that enclosing type, which the
///   walk visits directly, so following it would add nothing and would not
///   terminate.
fn is_reference(tc: &TypeCode) -> bool {
    match tc {
        TypeCode::ObjRef { .. } => true,
        // Half of an abstract interface *is* a reference (CORBA 3.4 Part 1
        // §7.9.2), and which half is a per-value decision the signature cannot
        // see. The caution is the honest answer.
        TypeCode::AbstractInterface { .. } => true,
        TypeCode::Sequence { element, .. } | TypeCode::Array { element, .. } => {
            is_reference(element)
        }
        TypeCode::Alias { aliased, .. } => is_reference(aliased),
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
            members.iter().any(|m| is_reference(&m.tc))
        }
        TypeCode::Union { discriminator, cases, .. } => {
            is_reference(discriminator) || cases.iter().any(|c| is_reference(&c.tc))
        }
        // The state of a valuetype goes on the wire inline, members and all, so
        // a reference among them travels with it. That v1 declines to marshal
        // the value is a different question, answered elsewhere and not by
        // pretending the reference is absent.
        TypeCode::Value { base, members, .. } => {
            base.as_deref().is_some_and(is_reference) || members.iter().any(|m| is_reference(&m.tc))
        }
        // A native names a type only a language mapping knows: no CDR encoding
        // at all, so nothing crosses and no address is handed out.
        TypeCode::Native { .. } => false,
        // See the two stated reasons above.
        TypeCode::Any | TypeCode::Recursive(_) => false,
        // Everything below is a leaf carrying bits, not an address.
        TypeCode::Null
        | TypeCode::Void
        | TypeCode::Short
        | TypeCode::Long
        | TypeCode::UShort
        | TypeCode::ULong
        | TypeCode::LongLong
        | TypeCode::ULongLong
        | TypeCode::Float
        | TypeCode::Double
        | TypeCode::LongDouble
        | TypeCode::Boolean
        | TypeCode::Char
        | TypeCode::WChar
        | TypeCode::Octet
        | TypeCode::TypeCode
        | TypeCode::Principal
        | TypeCode::String(_)
        | TypeCode::WString(_)
        | TypeCode::Fixed { .. }
        | TypeCode::Enum { .. } => false,
    }
}

// ── the subject: what a model is allowed to see ──────────────────────────────

/// One operation as the subject presents it: a name, the evidence line, and
/// the one derived fact the gate needs.
///
/// `silent` is carried in the artifact rather than recomputed, so that a gate
/// run over a proposal and a subject on disk decides with exactly the facts the
/// producer was shown. Recomputing it would let a change to [`READING_VERBS`]
/// silently regrade an old batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectOp {
    /// Operation name, as the peer reported it.
    pub name: String,
    /// The one-line rendering the producer was shown.
    pub evidence: String,
    /// Whether the name says nothing about effect; see [`Evidence::is_silent`].
    pub silent: bool,
}

/// One ingested interface, rendered as the evidence a model may read.
///
/// Nothing else is available to it — no source, no comments, no behaviour —
/// and the type exists partly to make that visible: a `Subject` is the whole
/// input, so anything in a proposal that is not derivable from one is
/// fabricated by construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    /// Repository id of the ingested interface.
    pub id: String,
    /// The provenance label the ingestion ran under.
    pub source: String,
    /// One entry per operation in the interface's callable surface.
    pub operations: Vec<SubjectOp>,
}

impl Subject {
    /// The subject as JSON, which is the artifact the batch loop carries.
    pub fn to_json(&self) -> Json {
        Json::Object(BTreeMap::from([
            ("id".into(), Json::String(self.id.clone())),
            ("source".into(), Json::String(self.source.clone())),
            (
                "operations".into(),
                Json::Array(
                    self.operations
                        .iter()
                        .map(|e| {
                            Json::Object(BTreeMap::from([
                                ("name".into(), Json::String(e.name.clone())),
                                ("evidence".into(), Json::String(e.evidence.clone())),
                                ("silent".into(), Json::Bool(e.silent)),
                            ]))
                        })
                        .collect(),
                ),
            ),
        ]))
    }

    /// The subject as prompt text — the whole of what a producer is shown.
    pub fn to_prompt(&self) -> String {
        let mut out = format!(
            "INGESTED INTERFACE {}\nDescribed to us by: {}\nNo IDL file, no comments and no source \
             exist for it. Everything known is below.\n\nOPERATIONS\n",
            self.id, self.source
        );
        for e in &self.operations {
            out.push_str(&format!("  {}\n", e.evidence));
        }
        out
    }

    /// The subject's entry for one operation, if it has it.
    pub fn operation(&self, name: &str) -> Option<&SubjectOp> {
        self.operations.iter().find(|e| e.name == name)
    }

    /// Parses the JSON form back, so the gate can run on artifacts alone.
    pub fn parse(text: &str) -> Result<Subject, String> {
        let j = Json::parse(text).map_err(|e| format!("subject is not JSON: {e}"))?;
        let id = j.get("id").and_then(Json::as_str).ok_or("subject has no id")?.to_owned();
        let source =
            j.get("source").and_then(Json::as_str).ok_or("subject has no source")?.to_owned();
        let Some(Json::Array(ops)) = j.get("operations") else {
            return Err("subject has no operations array".into());
        };
        let operations = ops
            .iter()
            .map(|o| {
                let name =
                    o.get("name").and_then(Json::as_str).ok_or("an operation has no name")?;
                Ok(SubjectOp {
                    name: name.to_owned(),
                    evidence: o.get("evidence").and_then(Json::as_str).unwrap_or("").to_owned(),
                    silent: matches!(o.get("silent"), Some(Json::Bool(true))),
                })
            })
            .collect::<Result<Vec<SubjectOp>, String>>()?;
        Ok(Subject { id, source, operations })
    }
}

/// Every ingested interface in a registry, as subjects.
///
/// The question asked is [`Registry::touches_ingested`] rather than
/// `is_ingested`: an interface whose *base* came off the wire has remote
/// operations in its callable surface, and those need annotating just as much.
pub fn subjects(registry: &Registry) -> Vec<Subject> {
    let mut out = Vec::new();
    for id in registry.ids() {
        if !registry.touches_ingested(id) {
            continue;
        }
        let Some(Entry::Interface(iface)) = registry.get(id) else { continue };
        if iface.forward_only {
            continue;
        }
        let source = match registry.origin(id) {
            Some(orbweaver_registry::Origin::Ingested(s)) => s,
            // Local interface with an ingested base: name the bases' source, so
            // the row says where the untrusted half came from.
            _ => registry
                .ancestors(id)
                .iter()
                .find_map(|a| match registry.origin(a) {
                    Some(orbweaver_registry::Origin::Ingested(s)) => Some(s),
                    _ => None,
                })
                .unwrap_or_else(|| "unknown".into()),
        };
        out.push(Subject {
            id: id.clone(),
            source,
            operations: callable_surface(registry, id)
                .into_iter()
                .map(|(name, sig)| {
                    let e = Evidence::of(&name, &sig);
                    SubjectOp { name, evidence: e.to_line(), silent: e.is_silent() }
                })
                .collect(),
        });
    }
    out
}

/// Every operation callable on `id`, its own and its bases', by name.
fn callable_surface(registry: &Registry, id: &str) -> Vec<(String, OperationSig)> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    let mut chain = vec![id.to_owned()];
    chain.extend(registry.ancestors(id));
    for owner in chain {
        let Some(iface) = registry.interface(&owner) else { continue };
        for (name, sig) in &iface.operations {
            if seen.insert(name.clone()) {
                out.push((name.clone(), sig.clone()));
            }
        }
    }
    out
}

// ── the proposal: what S3i produces ──────────────────────────────────────────

/// One operation's inferred annotation set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inference {
    /// The operation this is about.
    pub operation: String,
    /// A one-sentence description an agent could read — the inference with the
    /// most value and the least authority.
    pub desc: String,
    /// `destructive` or `unknown`. Never an ungating value; see [`UNGATING`].
    pub effect: String,
    /// A proposed scope name, for a human to accept, rename or reject.
    pub authz: Option<String>,
    /// What this was drawn from, quoting only the subject.
    pub evidence: String,
}

impl Inference {
    /// Whether this inference declined to guess.
    pub fn is_unknown(&self) -> bool {
        self.effect == "unknown"
    }

    /// The annotation map this becomes on the registry entry.
    ///
    /// Every key carries [`INFERRED_PREFIX`], plus the two marks. A consumer
    /// that has never heard of this module still sees `inferred_status:
    /// unapproved` beside the value, which is the point of putting the mark in
    /// the data rather than in a side table.
    pub fn annotations(&self, source: &str) -> BTreeMap<String, String> {
        let mut m = BTreeMap::from([
            ("inferred_desc".to_owned(), self.desc.clone()),
            ("inferred_effect".to_owned(), self.effect.clone()),
            (MARK_EVIDENCE.to_owned(), self.evidence.clone()),
            (MARK_STATUS.to_owned(), UNAPPROVED.to_owned()),
            (MARK_SOURCE.to_owned(), source.to_owned()),
        ]);
        if let Some(scope) = &self.authz {
            m.insert("inferred_authz".to_owned(), scope.clone());
        }
        m
    }
}

/// Everything S3i inferred about one ingested interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// The interface this is about.
    pub id: String,
    /// The ingestion source label, carried through so the mark records it.
    pub source: String,
    /// One entry per operation.
    pub inferences: Vec<Inference>,
}

impl Proposal {
    /// The fraction of operations for which the stage declined to guess an
    /// effect.
    ///
    /// Reported beside the first-pass rate and never instead of it. A batch
    /// whose unknown rate is zero is not a batch that understood everything;
    /// it is a batch to be suspicious of, because these names came off a wire
    /// and roughly half of any real interface is named for its noun rather
    /// than its verb.
    pub fn unknown_rate(&self) -> f64 {
        if self.inferences.is_empty() {
            return 0.0;
        }
        self.inferences.iter().filter(|i| i.is_unknown()).count() as f64
            / self.inferences.len() as f64
    }

    /// The inference for one operation.
    pub fn get(&self, operation: &str) -> Option<&Inference> {
        self.inferences.iter().find(|i| i.operation == operation)
    }

    /// The JSON form, which is the artifact on disk.
    pub fn to_json(&self) -> Json {
        Json::Object(BTreeMap::from([
            ("id".into(), Json::String(self.id.clone())),
            ("source".into(), Json::String(self.source.clone())),
            (
                "inferences".into(),
                Json::Array(
                    self.inferences
                        .iter()
                        .map(|i| {
                            let mut m = BTreeMap::from([
                                ("operation".to_owned(), Json::String(i.operation.clone())),
                                ("desc".to_owned(), Json::String(i.desc.clone())),
                                ("effect".to_owned(), Json::String(i.effect.clone())),
                                ("evidence".to_owned(), Json::String(i.evidence.clone())),
                            ]);
                            m.insert(
                                "authz".to_owned(),
                                match &i.authz {
                                    Some(s) => Json::String(s.clone()),
                                    None => Json::Null,
                                },
                            );
                            Json::Object(m)
                        })
                        .collect(),
                ),
            ),
        ]))
    }

    /// The canonical text form a workspace stores.
    pub fn to_text(&self) -> String {
        self.to_json().to_string()
    }

    /// Parses a producer's output.
    ///
    /// Tolerant of a leading or trailing markdown fence, because that is the
    /// one producer habit that would otherwise turn every model failure into
    /// the same uninformative parse error.
    pub fn parse(text: &str) -> Result<Proposal, String> {
        let trimmed = strip_fence(text);
        let j = Json::parse(trimmed).map_err(|e| format!("not JSON: {e}"))?;
        let id = j.get("id").and_then(Json::as_str).ok_or("no \"id\"")?.to_owned();
        let source = j.get("source").and_then(Json::as_str).unwrap_or("").to_owned();
        let Some(Json::Array(items)) = j.get("inferences") else {
            return Err("no \"inferences\" array".into());
        };
        let inferences = items
            .iter()
            .enumerate()
            .map(|(n, o)| {
                let at = |k: &str| -> Result<String, String> {
                    o.get(k)
                        .and_then(Json::as_str)
                        .map(str::to_owned)
                        .ok_or_else(|| format!("$.inferences[{n}] has no string {k:?}"))
                };
                Ok(Inference {
                    operation: at("operation")?,
                    desc: at("desc")?,
                    effect: at("effect")?,
                    authz: match o.get("authz") {
                        Some(Json::String(s)) if !s.trim().is_empty() => Some(s.clone()),
                        _ => None,
                    },
                    evidence: at("evidence")?,
                })
            })
            .collect::<Result<Vec<Inference>, String>>()?;
        Ok(Proposal { id, source, inferences })
    }
}

fn strip_fence(text: &str) -> &str {
    let t = text.trim();
    let Some(rest) = t.strip_prefix("```") else { return t };
    let rest = rest.split_once('\n').map(|(_, r)| r).unwrap_or(rest);
    rest.trim_end().strip_suffix("```").unwrap_or(rest).trim()
}

// ── the rules, in the prompt and in the check ────────────────────────────────

/// One thing S3i must always do.
///
/// The same two-halves discipline [`crate::annotate::Rule`] uses, for the same
/// reason: a constraint only in the prompt is a request, and a rule only in the
/// checker is a surprise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    /// The name [`gate`] reports under.
    pub id: &'static str,
    /// What it demands, in one line.
    pub demand: &'static str,
    /// A phrase [`S3I_PROMPT`] must contain.
    pub prompt_phrase: &'static str,
}

/// Everything S3i must always do, as prompt constraints and as checks.
pub const RULES: [Rule; 10] = [
    Rule {
        id: "si/operation-missing",
        demand: "every operation in the subject gets an inference",
        prompt_phrase: "one entry for EVERY operation",
    },
    Rule {
        id: "si/operation-unknown",
        demand: "no inference names an operation the subject does not have",
        prompt_phrase: "Do not invent operations",
    },
    Rule {
        id: "si/missing-desc",
        demand: "every inference carries a description that is not the name again",
        prompt_phrase: "desc: one sentence",
    },
    Rule {
        id: "si/missing-evidence",
        demand: "every inference carries the evidence it was drawn from",
        prompt_phrase: "evidence: what in the listing above",
    },
    Rule {
        id: "si/evidence-not-in-subject",
        demand: "quoted evidence appears in the subject, so nothing is fabricated",
        prompt_phrase: "Quote only text that appears",
    },
    Rule {
        id: "si/effect-not-in-vocabulary",
        demand: "effect is destructive or unknown",
        prompt_phrase: "effect is exactly one of: destructive, unknown",
    },
    Rule {
        id: "si/ungating-claim",
        demand: "no inference proposes a value that would remove a gate",
        prompt_phrase: "never read_only, idempotent or safe",
    },
    Rule {
        id: "si/unnamed-verb",
        demand: "an effect claimed from a word the checker does not recognise must name that word",
        prompt_phrase: "quote the word you are reading",
    },
    Rule {
        id: "si/missing-authz-proposal",
        demand: "a destructive proposal names the scope a human would have to grant",
        prompt_phrase: "authz: the scope a caller would need",
    },
    Rule {
        id: "si/gate-key-in-proposal",
        demand: "a proposal never writes an ai_* key, because a gate reads those",
        prompt_phrase: "Never write a key beginning ai_",
    },
];

/// The constraints, quoted into the S3i prompt verbatim.
///
/// Every [`RULES`] entry's `prompt_phrase` appears below; the test
/// `every_rule_is_a_prompt_constraint_and_a_check` pins it.
pub const S3I_PROMPT: &str = "\
You are looking at an interface belonging to somebody else's running system. No
IDL file, no comments and no source code exist for it. Everything anyone knows
about it is the listing you are given: operation names, parameter names and
types, return types, oneway markers and raises clauses.

You are producing a PROPOSAL that a human will review. You are not producing
facts, and nothing you write here is enforced by anything until a person
approves it.

Output one JSON object and NOTHING else: no markdown fences, no commentary.

  {\"id\": \"<the repository id you were given>\",
   \"source\": \"<the source label you were given>\",
   \"inferences\": [
     {\"operation\": \"...\", \"desc\": \"...\", \"effect\": \"...\",
      \"authz\": \"...\" or null, \"evidence\": \"...\"}
   ]}

Write one entry for EVERY operation in the listing, and only those.
Do not invent operations, parameters or types: anything not in the listing
does not exist as far as you are concerned.

  desc: one sentence saying what the operation appears to do, for a reader who
    has never seen this interface. Write what the SIGNATURE shows. Do not
    assert persistence, billing, notification or any other side effect — a name
    cannot tell you whether an operation writes to a database.

  effect is exactly one of: destructive, unknown.
    - destructive when the name itself says the operation changes something
      ('delete', 'update', 'transfer', 'shutdown', 'revoke').
    - unknown otherwise. This includes every read-sounding name: 'get_report'
      may bill the caller and 'describe' may write an audit row.
    Write never read_only, idempotent or safe. Those values REMOVE the approval
    gate in the bridge, and a wrong guess there lets an agent call an operation
    that moves money with no human in the loop. A wrong 'destructive' costs
    somebody one approval click; a wrong 'read_only' costs them the control.
    If the name is a word like 'process', 'execute', 'handle' or 'run' that
    says nothing at all, the honest answer is unknown. It is not a failure to
    write it — a stage that never says it does not know is a stage that is
    guessing, and the rate is reported.
    The checker keeps a short list of mutating verbs and it is INCOMPLETE:
    'evict', 'quiesce', 'drain' and 'retire' are not on it, and an operation
    named for one of them may well destroy something. If you read destruction
    in a word the checker does not know, then in the evidence
    quote the word you are reading, in quotation marks: the name 'evict' names
    a removal. The claim is then kept and marked as resting on your reading
    alone, for the human to check. What is refused is claiming an effect from
    a name while pointing at nothing in it.

  authz: the scope a caller would need, as a name a human can accept or rename
    ('tms.tracks.write'). Required whenever effect is destructive. Null
    otherwise. It is a suggestion for a person, never a permission.

  evidence: what in the listing above led you to this, in one line.
    Quote only text that appears in the listing — a parameter name you did not
    see there is a fabrication, and the checker rejects it.

Never write a key beginning ai_ anywhere in your output. Those keys are read by
the authorization gate, and an inferred value in one of them would be
indistinguishable from a contract somebody wrote and reviewed.
";

fn finding(rule: &str, message: String, source: String, fix: &str) -> Finding {
    Finding {
        rule: rule.to_owned(),
        severity: Severity::Error,
        message,
        line: 0,
        column: 0,
        source,
        fix: Some(fix.to_owned()),
    }
}

/// S3i's gate: does this proposal say only what the subject supports?
///
/// `subject` is the subject JSON the stage was handed; `output` is the
/// producer's proposal. Every check is deterministic, and every one of them is
/// about honesty rather than about form — the form checks exist only to get far
/// enough to ask the honesty ones.
pub fn gate(subject: &str, output: &str) -> Report {
    let mut findings = Vec::new();
    let subject = match Subject::parse(subject) {
        Ok(s) => s,
        Err(e) => {
            // The stage was handed something that is not a subject. That is a
            // caller defect, not a producer defect, and blaming the producer
            // for it would put the failure in the wrong column.
            return Report {
                findings: vec![finding(
                    "si/bad-subject",
                    format!("the subject artifact could not be read: {e}"),
                    String::new(),
                    "regenerate the subject with `subjects()`; nothing here is the producer's \
                     fault",
                )],
            };
        }
    };
    let proposal = match Proposal::parse(output) {
        Ok(p) => p,
        Err(e) => {
            return Report {
                findings: vec![finding(
                    "si/unparseable",
                    format!("the proposal could not be read: {e}"),
                    String::new(),
                    "return one JSON object with \"id\", \"source\" and an \"inferences\" array, \
                     and nothing else",
                )],
            };
        }
    };

    if proposal.id != subject.id {
        findings.push(finding(
            "si/operation-unknown",
            format!(
                "the proposal is about {:?} and the subject is {:?}; an annotation attached to the \
                 wrong interface is worse than none",
                proposal.id, subject.id
            ),
            proposal.id.clone(),
            "copy the id from the subject verbatim",
        ));
    }

    let known: BTreeSet<&str> = subject.operations.iter().map(|e| e.name.as_str()).collect();
    let covered: BTreeSet<&str> =
        proposal.inferences.iter().map(|i| i.operation.as_str()).collect();

    for missing in known.difference(&covered) {
        findings.push(finding(
            "si/operation-missing",
            format!(
                "{}.{missing} has no inference, so it would reach a human's review with nothing to \
                 review and no row saying why",
                subject.id
            ),
            (*missing).to_owned(),
            "add an entry for it; `unknown` with the evidence is a complete answer, silence is not",
        ));
    }
    for extra in covered.difference(&known) {
        findings.push(finding(
            "si/operation-unknown",
            format!(
                "the proposal describes {:?}, which is not an operation of {}; this is a claim \
                 about somebody else's service that the description we were given does not support",
                extra, subject.id
            ),
            (*extra).to_owned(),
            "remove it — the listing is the whole of what exists here",
        ));
    }

    let subject_text = subject_haystack(&subject);
    for inf in &proposal.inferences {
        findings.extend(inference_findings(&subject, &subject_text, inf));
    }

    findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.rule.cmp(&b.rule)));
    Report { findings }
}

/// Every character a proposal is allowed to quote.
fn subject_haystack(subject: &Subject) -> String {
    let mut s = format!("{} {} ", subject.id, subject.source);
    for e in &subject.operations {
        s.push_str(&e.name);
        s.push(' ');
        s.push_str(&e.evidence);
        s.push(' ');
    }
    s.to_ascii_lowercase()
}

fn inference_findings(subject: &Subject, haystack: &str, inf: &Inference) -> Vec<Finding> {
    let mut out = Vec::new();
    let where_ = format!("{}.{}", subject.id, inf.operation);

    for key in GATE_KEYS {
        if inf.desc.contains(key) || inf.evidence.contains(key) || inf.effect.contains(key) {
            out.push(finding(
                "si/gate-key-in-proposal",
                format!(
                    "{where_} writes {key:?} into the proposal; that key is read by the \
                     authorization gate, and an inferred value there is indistinguishable from a \
                     contract a person wrote and reviewed"
                ),
                inf.operation.clone(),
                "use the proposal's own fields; the inferred_* keys are what reach the registry, \
                 and a human promotes them with `approve`",
            ));
            break;
        }
    }

    if inf.desc.trim().is_empty() || normalize(&inf.desc) == normalize(&inf.operation) {
        out.push(finding(
            "si/missing-desc",
            format!(
                "{where_} has no description beyond its own name, which leaves an agent choosing \
                 it exactly as blind as the empty annotation map ingestion produced"
            ),
            inf.operation.clone(),
            "write one sentence about what the signature shows, without asserting a side effect \
             the signature cannot show",
        ));
    }

    if inf.evidence.trim().is_empty() {
        out.push(finding(
            "si/missing-evidence",
            format!(
                "{where_} carries no evidence; an inference without its evidence is a verdict, and \
                 the human reviewing it would have to re-derive the reasoning to disagree with it"
            ),
            inf.operation.clone(),
            "say what in the listing led to the claim — the verb in the name, a parameter, the \
             return type",
        ));
    } else {
        for quoted in quoted_terms(&inf.evidence) {
            if !haystack.contains(&quoted.to_ascii_lowercase()) {
                out.push(finding(
                    "si/evidence-not-in-subject",
                    format!(
                        "{where_} cites {quoted:?} as evidence and no such text is in the \
                         description we were given; the inference is resting on something \
                         invented"
                    ),
                    quoted,
                    "quote only operation names, parameter names and types from the listing",
                ));
            }
        }
    }

    let effect = inf.effect.trim();
    if UNGATING.contains(&effect) {
        out.push(finding(
            "si/ungating-claim",
            format!(
                "{where_} proposes ai_effect {effect:?}, which is a value that REMOVES the \
                 approval gate. An inference may propose a value that closes a gate and never one \
                 that opens one: the evidence is a name, and a name cannot say whether the \
                 operation writes"
            ),
            inf.operation.clone(),
            "write `unknown`; a person who knows the service can author `read_only` themselves, \
             and that is a different act from approving a machine's guess",
        ));
    } else if !EFFECT_VALUES.contains(&effect) {
        out.push(finding(
            "si/effect-not-in-vocabulary",
            format!(
                "{where_} proposes effect {effect:?}, which is neither `destructive` nor \
                 `unknown`; a value nobody defined would be carried into the worksheet and mean \
                 whatever its reader assumed"
            ),
            inf.operation.clone(),
            "use `destructive` or `unknown`",
        ));
    }

    // A word list is evidence of *presence* and never of absence. Treating
    // "the checker recognises no verb" as "the name says nothing" asserts
    // exactly the thing this module says an inference cannot know — measured on
    // 2026-08-14, when it refused a correct `destructive` on
    // `moe::ExpertLoader::evict`, an operation whose own IDL comment says it
    // destroys everything routed to an expert. So the claim is kept and
    // [`MARK_BASIS`] records that the checker could not corroborate it; what is
    // refused is a claim that points at nothing in the name at all.
    let silent = subject.operation(&inf.operation).is_some_and(|o| o.silent);
    if silent && effect == "destructive" && !names_a_word_of(&inf.evidence, &inf.operation) {
        out.push(finding(
            "si/unnamed-verb",
            format!(
                "{where_} claims {effect:?} from a name whose verb the checker's word list does \
                 not contain, and the evidence quotes no word of the name, so nothing here says \
                 what the claim is being read from"
            ),
            inf.operation.clone(),
            "quote the word in the operation's own name that you are reading as destructive — \
             \"the name 'evict' names a removal\" — or write `unknown`. Both are honest; a claim \
             pointing at nothing is not",
        ));
    }

    if effect == "destructive" && inf.authz.as_ref().is_none_or(|s| s.trim().is_empty()) {
        out.push(finding(
            "si/missing-authz-proposal",
            format!(
                "{where_} is proposed destructive and names no scope, so the worksheet row a human \
                 reads would say `needs approval` and not say what permission to grant"
            ),
            inf.operation.clone(),
            "propose a scope name a person can accept or rename, such as \
             `<service>.<noun>.write`",
        ));
    }

    out
}

/// The double-quoted or single-quoted terms in an evidence sentence.
fn quoted_terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (quote, _) in [('"', 0), ('\'', 0)] {
        let mut rest = text;
        while let Some(open) = rest.find(quote) {
            let after = &rest[open + quote.len_utf8()..];
            let Some(close) = after.find(quote) else { break };
            let term = after[..close].trim();
            if !term.is_empty() && term.len() <= 64 {
                out.push(term.to_owned());
            }
            rest = &after[close + quote.len_utf8()..];
        }
    }
    out
}

/// Whether the evidence quotes a word that is part of the operation's own name.
///
/// The check the `si/unnamed-verb` rule can actually make: not "is the verb
/// real" — no checker knows that — but "is the claim pointing at something in
/// the name". `evict` quoted out of `evict` passes; a sentence about the
/// parameter list does not.
fn names_a_word_of(evidence: &str, operation: &str) -> bool {
    let op = operation.to_ascii_lowercase();
    quoted_terms(evidence).iter().any(|t| {
        let t = t.to_ascii_lowercase();
        !t.is_empty() && t.len() >= 3 && op.contains(&t)
    })
}

fn normalize(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase()
}

// ── the stage ────────────────────────────────────────────────────────────────

/// S3i as a [`Stage`]: a producer plus the gate that judges it.
///
/// It reports under [`StageId::Annotate`] because it **is** the annotation
/// stage — same position in the pipeline, same output in kind, a different
/// input medium. Giving it a sixth `StageId` would put it in `StageId::ORDER`
/// and therefore in `run_pipeline`'s range, which is wrong: S3 and S3i are
/// alternatives, never both. Run records name which input a batch ran over.
pub struct InferStage {
    command: String,
    scratch: std::path::PathBuf,
}

impl InferStage {
    /// A stage that shells out to `command`, in `CommandStage`'s contract:
    /// `<command> <input-file> [<repair-file>]`, `FORGE_STAGE` and
    /// `FORGE_PROMPT` in the environment, stdout is the artifact.
    pub fn new(command: impl Into<String>, scratch: impl Into<std::path::PathBuf>) -> Self {
        InferStage { command: command.into(), scratch: scratch.into() }
    }

    fn write(&self, name: &str, text: &str) -> Result<std::path::PathBuf, String> {
        std::fs::create_dir_all(&self.scratch)
            .map_err(|e| format!("{}: {e}", self.scratch.display()))?;
        let path = self.scratch.join(name);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }
}

impl Stage for InferStage {
    fn id(&self) -> StageId {
        StageId::Annotate
    }

    fn produce(&mut self, input: &str, repair: Option<&str>) -> Result<String, String> {
        // The producer is handed the subject's *rendering*: the JSON is the
        // canonical artifact, the rendering is what a prompt can use.
        let prepared = match Subject::parse(input) {
            Ok(subject) => subject.to_prompt(),
            // Not a subject: hand it over verbatim. A gate failure a human
            // cannot see is a gate failure nobody can fix.
            Err(_) => input.to_owned(),
        };

        let pid = std::process::id();
        let input_file = self.write(&format!("s3i-{pid}.input"), &prepared)?;
        let prompt_file = self.write("s3i.prompt.txt", S3I_PROMPT)?;
        let mut cmd = std::process::Command::new(&self.command);
        cmd.arg(&input_file);
        cmd.env("FORGE_STAGE", "s3i");
        cmd.env("FORGE_PROMPT", &prompt_file);
        if let Some(text) = repair {
            let path = self.write(&format!("s3i-{pid}.repair"), text)?;
            cmd.arg(&path);
        }
        let output = cmd.output().map_err(|e| format!("cannot run {}: {e}", self.command))?;
        if !output.status.success() {
            return Err(format!(
                "{} exited {}: {}",
                self.command,
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn gate(&self, input: &str, output: &str) -> Report {
        gate(input, output)
    }
}

// ── applying, approving, and refusing ────────────────────────────────────────

/// Attaches a proposal's marks to an interface entry's operations.
///
/// Returns a **new** entry: the registry never lets an ingested description be
/// replaced in place ([`Registry::define_ingested`] refuses), so the annotated
/// entry is what a caller registers, not something it patches afterwards.
///
/// Only the annotation maps differ. `applying_a_proposal_changes_no_signature`
/// pins that, because an annotator that adjusts a signature has changed what a
/// call marshals against a server that agreed to none of it.
/// [`MARK_BASIS`] is computed here, from the signature, and never taken from
/// the producer: it is the checker's own statement about how much of the claim
/// it could corroborate, which is not a thing the claimant may write.
pub fn apply(entry: &InterfaceEntry, proposal: &Proposal) -> InterfaceEntry {
    let mut out = entry.clone();
    for (name, sig) in out.operations.iter_mut() {
        let Some(inf) = proposal.get(name) else { continue };
        let evidence = Evidence::of(name, sig);
        sig.annotations.extend(inf.annotations(&proposal.source));
        sig.annotations.insert(MARK_BASIS.to_owned(), basis(inf, &evidence));
    }
    out
}

/// How much of one claim the checker can corroborate.
fn basis(inf: &Inference, evidence: &Evidence) -> String {
    if inf.is_unknown() {
        return BASIS_DECLINED.to_owned();
    }
    match evidence.mutating_verb {
        Some(verb) => format!("{BASIS_RECOGNISED}: {verb:?}"),
        None => BASIS_UNRECOGNISED.to_owned(),
    }
}

/// A human taking responsibility for an inference.
///
/// Deliberately not `Default`: an approval with an empty approver is the state
/// this whole module exists to make impossible, and a type that can be
/// conjured with `..Default::default()` is one that will be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    /// Who approved it. A person or a role a person is accountable for.
    pub by: String,
    /// When, as text the audit line will carry verbatim.
    pub at: String,
}

/// Why an approval was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApproveError {
    /// The approver or the date is empty.
    NoApprover,
    /// There is nothing marked inferred to approve.
    NothingInferred,
    /// The value being promoted is one an inference may never make; see
    /// [`UNGATING`]. Refused at approval as well as at inference, because the
    /// worksheet is editable and a rule enforced at only one end is a rule with
    /// a way around it.
    Ungating(String),
}

impl std::fmt::Display for ApproveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApproveError::NoApprover => write!(
                f,
                "an approval must name who gave it and when; an anonymous approval is the \
                 un-approved state wearing a different label"
            ),
            ApproveError::NothingInferred => {
                write!(f, "there is no inferred_* value here to promote")
            }
            ApproveError::Ungating(v) => write!(
                f,
                "{v:?} removes the approval gate and may not be promoted from an inference — a \
                 person who knows the service writes that annotation themselves"
            ),
        }
    }
}

impl std::error::Error for ApproveError {}

/// Promotes an operation's inferred values into the keys the gates read.
///
/// This is the **only** transition from "a machine guessed" to "the bridge acts
/// on it", and it is a human act with a name attached. The `inferred_*` keys
/// are kept rather than removed, and [`MARK_STATUS`] records the approver, so
/// an annotation that began as a guess stays distinguishable from one an author
/// wrote for as long as it exists — in the registry, in a description an agent
/// reads, and in an audit line.
pub fn approve(
    annotations: &mut BTreeMap<String, String>,
    approval: &Approval,
) -> Result<(), ApproveError> {
    if approval.by.trim().is_empty() || approval.at.trim().is_empty() {
        return Err(ApproveError::NoApprover);
    }
    let promotable: Vec<(String, String)> = annotations
        .iter()
        .filter_map(|(k, v)| {
            let suffix = k.strip_prefix(INFERRED_PREFIX)?;
            // The marks are metadata about the inference, not values to promote.
            if matches!(suffix, "status" | "evidence" | "source" | "basis") {
                return None;
            }
            Some((format!("ai_{suffix}"), v.clone()))
        })
        .collect();
    if promotable.is_empty() {
        return Err(ApproveError::NothingInferred);
    }
    for (key, value) in &promotable {
        if key == "ai_effect" {
            let v = value.trim();
            if UNGATING.contains(&v) {
                return Err(ApproveError::Ungating(v.to_owned()));
            }
            // `unknown` is not an `ai_effect` the policy gate knows, and that
            // is the correct outcome: an unrecognised value is treated as
            // needing approval, so promoting "we could not tell" leaves the
            // human in the loop rather than removing them from it.
        }
    }
    for (key, value) in promotable {
        annotations.insert(key, value);
    }
    annotations.insert(
        MARK_STATUS.to_owned(),
        format!("approved by {:?} on {}", approval.by.trim(), approval.at.trim()),
    );
    Ok(())
}

/// What an annotation map is, from the map alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provenance {
    /// Written by a person, in IDL that went through S4.
    Authored,
    /// Inferred and not yet approved by anybody.
    InferredUnapproved,
    /// Inferred, and a named human has taken responsibility for it.
    InferredApproved(String),
}

/// The provenance of one operation's annotations.
///
/// Answerable from the map alone, which is the property a bare `ai_effect`
/// string can never have — and the reason the marks are annotations rather than
/// a side table that a `Registry` clone, a JSON round trip or a description
/// handed to an agent would quietly drop.
pub fn provenance(annotations: &BTreeMap<String, String>) -> Provenance {
    match annotations.get(MARK_STATUS).map(String::as_str) {
        None => Provenance::Authored,
        Some(UNAPPROVED) => Provenance::InferredUnapproved,
        Some(other) => Provenance::InferredApproved(other.to_owned()),
    }
}

/// One operation that no human has signed off, and why it matters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocker {
    /// Repository id of the interface.
    pub id: String,
    /// Operation name.
    pub operation: String,
    /// What the state is, in the words an operator should read.
    pub why: String,
    /// The proposal's suggested effect, or `None` where no proposal exists.
    pub proposed_effect: Option<String>,
    /// The proposal's suggested scope, or `None`.
    pub proposed_authz: Option<String>,
    /// How much of the claim the checker could corroborate ([`MARK_BASIS`]),
    /// or `None` where there is no claim. A reviewer reading the worksheet
    /// top to bottom should be able to see which rows the machine is merely
    /// repeating a word list back on and which rows are the producer's own
    /// reading — those are the ones worth a human's time first.
    pub basis: Option<String>,
}

/// Every operation on an ingested surface that a human has not signed off.
///
/// Both states are listed and they are different: an operation with an
/// un-approved proposal, and an operation with **no proposal at all**. The
/// second is the one a "default-off" design loses — nothing is set, so nothing
/// is wrong, so nothing is shown — and it is exactly the state an ingested
/// interface starts in, since the wire carries no annotations.
pub fn unapproved(registry: &Registry) -> Vec<Blocker> {
    let mut out = Vec::new();
    for id in registry.ids() {
        if !registry.touches_ingested(id) {
            continue;
        }
        if registry.interface(id).is_none() {
            continue;
        }
        for (name, sig) in callable_surface(registry, id) {
            let ann = &sig.annotations;
            let proposed_effect = ann.get("inferred_effect").cloned();
            let proposed_authz = ann.get("inferred_authz").cloned();
            let basis = ann.get(MARK_BASIS).cloned();
            let why = match provenance(ann) {
                Provenance::InferredApproved(_) => continue,
                Provenance::Authored => {
                    if ann.contains_key("ai_effect") || ann.contains_key("ai_authz") {
                        // An ingested entry carrying gate keys with no mark is
                        // the one shape this design forbids: it is a remote
                        // description wearing a reviewed contract's clothes.
                        "carries ai_* gate keys with no provenance mark, on an interface that came \
                         off the wire"
                            .to_owned()
                    } else {
                        "no annotation at all — the wire carries none, so the guard has nothing to \
                         key on"
                            .to_owned()
                    }
                }
                Provenance::InferredUnapproved => format!(
                    "inferred ({}), not approved by anybody",
                    proposed_effect.clone().unwrap_or_else(|| "no effect".into())
                ),
            };
            out.push(Blocker {
                id: id.clone(),
                operation: name,
                why,
                proposed_effect,
                proposed_authz,
                basis,
            });
        }
    }
    out
}

/// The name of the worksheet [`worksheet`] is written to, beside S5's.
pub const INFERRED_TODO_FILE: &str = "inferred.todo.tsv";

/// The un-approved state as something an operator reads.
///
/// One row per ingested operation, always — the point being that the absence of
/// an approval is a line rather than a missing field. §7.4 I2's exposure
/// worksheet is the model, and the two are meant to be read together: that one
/// says what is not exposed, this one says what could not be safely exposed
/// even if somebody allowlisted it.
pub fn worksheet(registry: &Registry) -> String {
    let blockers = unapproved(registry);
    let mut out = String::from(
        "# S3i inference worksheet — every ingested operation awaiting a human.\n\
         # An inferred annotation is a claim about somebody else's service, made by\n\
         # reading names and types. Nothing below is enforced by anything: the values\n\
         # live under inferred_* keys, and the bridge's gates read ai_effect and\n\
         # ai_authz. A row leaves this file when a person promotes it with\n\
         # infer::approve, which records who and when and keeps the inferred_* keys\n\
         # so the annotation stays distinguishable from one an author wrote.\n\
         # The inference cannot know whether an operation writes to a database. The\n\
         # name does not say, and no prompt makes it say.\n\
         # A row whose basis is `unrecognised-verb` is the producer's own reading of a\n\
         # word this project's verb list has never contained — `evict` destroys every\n\
         # request routed to an expert, and no list here had the word. Those rows are\n\
         # where a reviewer's attention is worth most.\n\
         # columns: repository-id\toperation\tproposed-effect\tproposed-scope\tapproved\tbasis\tstate\n",
    );
    for b in &blockers {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\tapproved=no\t{}\t{}\n",
            b.id,
            b.operation,
            b.proposed_effect.as_deref().unwrap_or("-"),
            b.proposed_authz.as_deref().unwrap_or("-"),
            b.basis.as_deref().unwrap_or("-"),
            b.why
        ));
    }
    if blockers.is_empty() {
        out.push_str("# nothing ingested, or every ingested operation is approved\n");
    }
    out
}

/// Whether `id` may be handed to an exposure decision, and why not if not.
///
/// The single question an operator's allowlisting step should ask. It refuses
/// on the *interface*, not per call, because exposure is granted per interface
/// and a refusal that arrived at call time would arrive after the tool was
/// already advertised to an agent.
pub fn exposure_refusal(registry: &Registry, id: &str) -> Option<String> {
    if !registry.touches_ingested(id) {
        return None;
    }
    let all = unapproved(registry);
    let mine: Vec<&Blocker> = all.iter().filter(|b| b.id == id).collect();
    if mine.is_empty() {
        return None;
    }
    Some(format!(
        "{id} came off the wire and {} of its operations have no annotation a human has approved \
         ({}). The bridge's gates read ai_effect and ai_authz; an inferred value is not in those \
         keys and enforces nothing, so exposing this interface now would expose it with no effect \
         gate and no scope check at all.",
        mine.len(),
        mine.iter().map(|b| b.operation.as_str()).collect::<Vec<_>>().join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_giop::typecode::{Member, UnionCase, ValueMember};
    use orbweaver_registry::{Entry, InterfaceEntry, OperationSig, ParamSig};

    fn sig(oneway: bool, returns: TypeCode, params: Vec<ParamSig>) -> OperationSig {
        OperationSig { returns, params, raises: Vec::new(), oneway, annotations: BTreeMap::new() }
    }

    fn ingested_registry() -> Registry {
        let mut iface = InterfaceEntry::default();
        iface.operations.insert("delete_track".into(), sig(false, TypeCode::Void, vec![]));
        iface.operations.insert("get_track".into(), sig(false, TypeCode::String(0), vec![]));
        iface.operations.insert("process".into(), sig(false, TypeCode::Long, vec![]));
        let mut r = Registry::new();
        r.define_ingested(
            "IDL:tms/TrackManager:1.0".into(),
            Entry::Interface(iface),
            "ifr://legacy",
        )
        .expect("registers");
        r
    }

    fn subject_json() -> String {
        subjects(&ingested_registry())[0].to_json().to_string()
    }

    fn proposal_json(effect_for_process: &str) -> String {
        format!(
            r#"{{"id":"IDL:tms/TrackManager:1.0","source":"ifr://legacy","inferences":[
              {{"operation":"delete_track","desc":"Removes a track by identifier.",
                "effect":"destructive","authz":"tms.tracks.write",
                "evidence":"the name contains 'delete'"}},
              {{"operation":"get_track","desc":"Returns a track as a string.",
                "effect":"unknown","authz":null,
                "evidence":"the name contains 'get' but that says nothing about writes"}},
              {{"operation":"process","desc":"Does something and returns a long.",
                "effect":"{effect_for_process}","authz":{},
                "evidence":"the name 'process' says nothing"}}
            ]}}"#,
            if effect_for_process == "destructive" { "\"tms.process\"" } else { "null" }
        )
    }

    #[test]
    fn a_faithful_proposal_passes_the_gate() {
        let r = gate(&subject_json(), &proposal_json("unknown"));
        assert!(r.is_ok(), "{:?}", r.findings);
    }

    /// The rule that is the whole point. A name cannot say whether an operation
    /// writes, so an inference may never propose the value that removes the
    /// gate.
    #[test]
    fn an_ungating_effect_may_not_be_inferred() {
        for value in UNGATING {
            let text = proposal_json("unknown")
                .replace("\"effect\":\"unknown\"", &format!("\"effect\":\"{value}\""));
            let r = gate(&subject_json(), &text);
            let f = r
                .findings
                .iter()
                .find(|f| f.rule == "si/ungating-claim")
                .unwrap_or_else(|| panic!("{value} was not refused: {:?}", r.findings));
            assert!(f.message.contains("REMOVES the approval gate"), "{}", f.message);
            assert!(f.message.contains("a name cannot say whether"), "{}", f.message);
        }
    }

    /// A claim from a name the checker's word list does not cover is refused
    /// **only when it points at nothing**. The rule used to refuse it outright;
    /// the 2026-08-14 batch measured what that costs — see the run record and
    /// [`BASIS_UNRECOGNISED`].
    #[test]
    fn a_claim_that_points_at_nothing_in_the_name_is_refused() {
        let text = proposal_json("destructive")
            .replace("the name 'process' says nothing", "the return type is a long");
        let r = gate(&subject_json(), &text);
        let f = r.findings.iter().find(|f| f.rule == "si/unnamed-verb").expect("refused");
        assert!(f.message.contains("quotes no word of the name"), "{}", f.message);
        assert!(f.fix.as_deref().unwrap().contains("evict"), "{f:?}");
    }

    /// …and the same claim, quoting the word it reads, is **kept**. A word list
    /// is evidence of presence and never of absence, and `evict` destroys
    /// everything routed to an expert while appearing on no verb list this
    /// project has ever written.
    #[test]
    fn a_claim_that_names_the_word_it_reads_is_kept_and_marked() {
        let text = proposal_json("destructive")
            .replace("the name 'process' says nothing", "the name 'process' names a run");
        let r = gate(&subject_json(), &text);
        assert!(r.is_ok(), "{:?}", r.findings);

        let registry = ingested_registry();
        let before = registry.interface("IDL:tms/TrackManager:1.0").expect("there");
        let after = apply(before, &Proposal::parse(&text).expect("parses"));
        assert_eq!(after.operations["process"].annotations[MARK_BASIS], BASIS_UNRECOGNISED);
        assert!(
            after.operations["delete_track"].annotations[MARK_BASIS].starts_with(BASIS_RECOGNISED),
            "a recognised verb is marked as such"
        );
        assert_eq!(after.operations["get_track"].annotations[MARK_BASIS], BASIS_DECLINED);
    }

    #[test]
    fn the_unknown_rate_is_computed_and_reported() {
        let p = Proposal::parse(&proposal_json("unknown")).expect("parses");
        // get_track and process both decline to guess; delete_track does not.
        assert!((p.unknown_rate() - 2.0 / 3.0).abs() < 1e-9, "{}", p.unknown_rate());
    }

    #[test]
    fn evidence_that_quotes_something_absent_is_refused() {
        let text = proposal_json("unknown").replace(
            "the name contains 'delete'",
            "the parameter 'authorization_token' is present",
        );
        let r = gate(&subject_json(), &text);
        assert!(
            r.findings.iter().any(|f| f.rule == "si/evidence-not-in-subject"),
            "{:?}",
            r.findings
        );
    }

    #[test]
    fn an_invented_operation_is_refused() {
        let text = proposal_json("unknown")
            .replace("\"operation\":\"process\"", "\"operation\":\"wipe_everything\"");
        let r = gate(&subject_json(), &text);
        let rules: Vec<&str> = r.findings.iter().map(|f| f.rule.as_str()).collect();
        assert!(rules.contains(&"si/operation-unknown"), "{rules:?}");
        assert!(rules.contains(&"si/operation-missing"), "{rules:?}");
    }

    #[test]
    fn an_operation_left_out_is_refused_because_silence_is_not_an_answer() {
        let text = r#"{"id":"IDL:tms/TrackManager:1.0","source":"ifr://legacy","inferences":[
          {"operation":"delete_track","desc":"Removes a track.","effect":"destructive",
           "authz":"tms.tracks.write","evidence":"the name contains 'delete'"}]}"#;
        let r = gate(&subject_json(), text);
        let missing: Vec<&Finding> =
            r.findings.iter().filter(|f| f.rule == "si/operation-missing").collect();
        assert_eq!(missing.len(), 2, "{:?}", r.findings);
        assert!(missing[0].fix.as_deref().unwrap().contains("silence is not"), "{:?}", missing[0]);
    }

    #[test]
    fn a_destructive_proposal_without_a_scope_is_refused() {
        let text = proposal_json("unknown").replace("\"tms.tracks.write\"", "null");
        let r = gate(&subject_json(), &text);
        assert!(
            r.findings.iter().any(|f| f.rule == "si/missing-authz-proposal"),
            "{:?}",
            r.findings
        );
    }

    #[test]
    fn a_proposal_that_writes_a_gate_key_is_refused() {
        let text = proposal_json("unknown")
            .replace("the name contains 'delete'", "ai_effect should be destructive");
        let r = gate(&subject_json(), &text);
        assert!(r.findings.iter().any(|f| f.rule == "si/gate-key-in-proposal"), "{:?}", r.findings);
    }

    /// A producer failure must not read as a dishonest proposal.
    #[test]
    fn unparseable_output_is_its_own_cause() {
        let r = gate(&subject_json(), "I'm sorry, I can't help with that.");
        assert_eq!(r.findings.len(), 1);
        assert_eq!(r.findings[0].rule, "si/unparseable");
    }

    #[test]
    fn a_bad_subject_blames_the_caller_and_not_the_producer() {
        let r = gate("not json", &proposal_json("unknown"));
        assert_eq!(r.findings[0].rule, "si/bad-subject");
        assert!(r.findings[0].fix.as_deref().unwrap().contains("the producer's fault"));
    }

    // ── marking, approval and the refusal ────────────────────────────────────

    #[test]
    fn an_inference_never_lands_in_a_key_a_gate_reads() {
        let p = Proposal::parse(&proposal_json("unknown")).expect("parses");
        for inf in &p.inferences {
            let ann = inf.annotations("ifr://legacy");
            for key in ann.keys() {
                assert!(!key.starts_with("ai_"), "{key} is a key a gate reads");
                assert!(key.starts_with(INFERRED_PREFIX), "{key} carries no mark");
            }
            assert_eq!(ann[MARK_STATUS], UNAPPROVED);
            assert!(!ann[MARK_EVIDENCE].is_empty());
        }
    }

    #[test]
    fn provenance_is_answerable_from_the_annotation_map_alone() {
        let p = Proposal::parse(&proposal_json("unknown")).expect("parses");
        let mut ann = p.get("delete_track").unwrap().annotations("ifr://legacy");
        assert_eq!(provenance(&ann), Provenance::InferredUnapproved);
        assert_eq!(provenance(&BTreeMap::new()), Provenance::Authored);

        approve(&mut ann, &Approval { by: "ops-lead".into(), at: "2026-08-14".into() })
            .expect("approved");
        match provenance(&ann) {
            Provenance::InferredApproved(who) => {
                assert!(who.contains("ops-lead"), "{who}");
                assert!(who.contains("2026-08-14"), "{who}");
            }
            other => panic!("{other:?}"),
        }
        // The promotion happened, and the mark survived it.
        assert_eq!(ann["ai_effect"], "destructive");
        assert_eq!(ann["ai_authz"], "tms.tracks.write");
        assert_eq!(ann["inferred_effect"], "destructive");
        assert!(ann.contains_key(MARK_EVIDENCE), "the evidence travels with the promotion");
    }

    #[test]
    fn an_anonymous_approval_is_refused() {
        let p = Proposal::parse(&proposal_json("unknown")).expect("parses");
        let mut ann = p.get("delete_track").unwrap().annotations("ifr://legacy");
        assert_eq!(
            approve(&mut ann, &Approval { by: "  ".into(), at: "2026-08-14".into() }),
            Err(ApproveError::NoApprover)
        );
        assert_eq!(provenance(&ann), Provenance::InferredUnapproved, "nothing moved");
    }

    /// The asymmetry is enforced at both ends: the worksheet is a text file,
    /// and a rule enforced only at inference time is a rule with a way around
    /// it.
    #[test]
    fn an_ungating_value_cannot_be_promoted_even_by_a_human_through_this_path() {
        let mut ann = BTreeMap::from([
            ("inferred_effect".to_owned(), "read_only".to_owned()),
            (MARK_STATUS.to_owned(), UNAPPROVED.to_owned()),
        ]);
        assert_eq!(
            approve(&mut ann, &Approval { by: "ops-lead".into(), at: "2026-08-14".into() }),
            Err(ApproveError::Ungating("read_only".into()))
        );
        assert!(!ann.contains_key("ai_effect"));
    }

    #[test]
    fn the_worksheet_carries_a_row_for_an_operation_with_no_proposal_at_all() {
        let sheet = worksheet(&ingested_registry());
        for op in ["delete_track", "get_track", "process"] {
            assert!(sheet.contains(op), "{op} has no row:\n{sheet}");
        }
        assert!(sheet.contains("no annotation at all"), "{sheet}");
        assert!(sheet.contains("cannot know whether an operation writes"), "{sheet}");
    }

    #[test]
    fn an_ingested_interface_is_refused_for_exposure_until_a_human_approves() {
        let r = ingested_registry();
        let why = exposure_refusal(&r, "IDL:tms/TrackManager:1.0").expect("refused");
        assert!(why.contains("enforces nothing"), "{why}");
        assert!(exposure_refusal(&r, "IDL:nothing/Here:1.0").is_none(), "not ingested, not ours");
    }

    #[test]
    fn applying_a_proposal_changes_no_signature() {
        let registry = ingested_registry();
        let before = registry.interface("IDL:tms/TrackManager:1.0").expect("there").clone();
        let p = Proposal::parse(&proposal_json("unknown")).expect("parses");
        let after = apply(&before, &p);
        assert_eq!(before.bases, after.bases);
        assert_eq!(before.operations.len(), after.operations.len());
        for (name, sig) in &before.operations {
            let now = &after.operations[name];
            assert_eq!(sig.returns, now.returns, "{name}");
            assert_eq!(sig.params, now.params, "{name}");
            assert_eq!(sig.oneway, now.oneway, "{name}");
            assert_eq!(sig.raises, now.raises, "{name}");
            assert!(now.annotations.len() > sig.annotations.len(), "{name} gained no marks");
        }
    }

    // ── the roster ───────────────────────────────────────────────────────────

    /// The codify test, in [`crate::annotate`]'s shape: every rule is a prompt
    /// constraint and a check, and every rule fires on a sample.
    #[test]
    fn every_rule_is_a_prompt_constraint_and_a_check() {
        let subject = subject_json();
        let samples: [(&str, String); 10] = [
            (
                "si/operation-missing",
                r#"{"id":"IDL:tms/TrackManager:1.0","source":"s","inferences":[]}"#.to_owned(),
            ),
            (
                "si/operation-unknown",
                proposal_json("unknown")
                    .replace("\"operation\":\"process\"", "\"operation\":\"nope\""),
            ),
            (
                "si/missing-desc",
                proposal_json("unknown").replace("Removes a track by identifier.", ""),
            ),
            (
                "si/missing-evidence",
                proposal_json("unknown").replace("the name contains 'delete'", ""),
            ),
            (
                "si/evidence-not-in-subject",
                proposal_json("unknown")
                    .replace("the name contains 'delete'", "the parameter 'nonexistent_thing'"),
            ),
            (
                "si/effect-not-in-vocabulary",
                proposal_json("unknown")
                    .replace("\"effect\":\"destructive\"", "\"effect\":\"probably_fine\""),
            ),
            (
                "si/ungating-claim",
                proposal_json("unknown")
                    .replace("\"effect\":\"destructive\"", "\"effect\":\"read_only\""),
            ),
            (
                "si/unnamed-verb",
                proposal_json("destructive")
                    .replace("the name 'process' says nothing", "the return type is a long"),
            ),
            (
                "si/missing-authz-proposal",
                proposal_json("unknown").replace("\"tms.tracks.write\"", "null"),
            ),
            (
                "si/gate-key-in-proposal",
                proposal_json("unknown").replace("Removes a track by identifier.", "sets ai_authz"),
            ),
        ];
        for rule in RULES {
            assert!(
                S3I_PROMPT.contains(rule.prompt_phrase),
                "{}: the prompt never says {:?}, so the producer is measured against a rule it was \
                 not given",
                rule.id,
                rule.prompt_phrase
            );
            assert!(!rule.demand.is_empty());
            let (_, sample) = samples
                .iter()
                .find(|(id, _)| *id == rule.id)
                .unwrap_or_else(|| panic!("{} has no sample", rule.id));
            let got = gate(&subject, sample);
            assert!(
                got.findings.iter().any(|f| f.rule == rule.id),
                "{} never fires on its own sample; got {:?}",
                rule.id,
                got.findings.iter().map(|f| f.rule.as_str()).collect::<Vec<_>>()
            );
        }
    }

    /// The mirrored constant must stay the policy gate's set, or the asymmetry
    /// argument silently stops covering one of the values that opens a gate.
    #[test]
    fn the_ungating_set_is_the_policy_gates_ungated_set() {
        assert_eq!(UNGATING, crate::annotate::UNGATED_EFFECTS);
        assert!(!UNGATING.contains(&"destructive"), "the one value an inference may propose");
        assert!(EFFECT_VALUES.iter().all(|v| !UNGATING.contains(v)));
    }

    #[test]
    fn a_silent_name_is_silent_and_a_verb_is_not() {
        let e = Evidence::of("process", &sig(false, TypeCode::Long, vec![]));
        assert!(e.is_silent());
        assert!(e.to_line().contains("says nothing about effect"));
        assert!(!Evidence::of("delete_track", &sig(false, TypeCode::Void, vec![])).is_silent());
        assert!(!Evidence::of("get_track", &sig(false, TypeCode::Long, vec![])).is_silent());
    }

    // ── the renderer: one verdict per variant ────────────────────────────────

    /// The number of `TypeCode` variants as of 2026-08-25.
    ///
    /// A floor would prove nothing here: the property is *every* variant has a
    /// verdict, so the figure has to be the count, and [`variant_tag`] is what
    /// keeps it honest — a thirty-fourth variant fails to compile there and in
    /// [`render_type`] itself before this number can go stale.
    const TYPECODE_VARIANTS: usize = 33;

    /// A tag per variant, exhaustively, so the table below cannot silently stop
    /// covering one. No `_` arm, on purpose — this is half the negative control.
    fn variant_tag(tc: &TypeCode) -> &'static str {
        match tc {
            TypeCode::Null => "Null",
            TypeCode::Void => "Void",
            TypeCode::Short => "Short",
            TypeCode::Long => "Long",
            TypeCode::UShort => "UShort",
            TypeCode::ULong => "ULong",
            TypeCode::Float => "Float",
            TypeCode::Double => "Double",
            TypeCode::Boolean => "Boolean",
            TypeCode::Char => "Char",
            TypeCode::Octet => "Octet",
            TypeCode::Any => "Any",
            TypeCode::TypeCode => "TypeCode",
            TypeCode::Principal => "Principal",
            TypeCode::LongLong => "LongLong",
            TypeCode::ULongLong => "ULongLong",
            TypeCode::LongDouble => "LongDouble",
            TypeCode::WChar => "WChar",
            TypeCode::String(_) => "String",
            TypeCode::WString(_) => "WString",
            TypeCode::Fixed { .. } => "Fixed",
            TypeCode::ObjRef { .. } => "ObjRef",
            TypeCode::Struct { .. } => "Struct",
            TypeCode::Union { .. } => "Union",
            TypeCode::Enum { .. } => "Enum",
            TypeCode::Sequence { .. } => "Sequence",
            TypeCode::Array { .. } => "Array",
            TypeCode::Alias { .. } => "Alias",
            TypeCode::Except { .. } => "Except",
            TypeCode::Value { .. } => "Value",
            TypeCode::AbstractInterface { .. } => "AbstractInterface",
            TypeCode::Native { .. } => "Native",
            TypeCode::Recursive(_) => "Recursive",
        }
    }

    fn member(name: &str, tc: TypeCode) -> Member {
        Member { name: name.to_owned(), tc }
    }

    /// One instance of every `TypeCode` variant, beside the string a prompt is
    /// allowed to show for it.
    fn every_variant() -> Vec<(TypeCode, &'static str)> {
        vec![
            (TypeCode::Null, "<tk_null: the empty TypeCode, which no declaration writes>"),
            (TypeCode::Void, "void"),
            (TypeCode::Short, "short"),
            (TypeCode::Long, "long"),
            (TypeCode::UShort, "unsigned short"),
            (TypeCode::ULong, "unsigned long"),
            (TypeCode::Float, "float"),
            (TypeCode::Double, "double"),
            (TypeCode::Boolean, "boolean"),
            (TypeCode::Char, "char"),
            (TypeCode::Octet, "octet"),
            (TypeCode::Any, "any"),
            (TypeCode::TypeCode, "::CORBA::TypeCode"),
            (TypeCode::Principal, "<Principal: withdrawn from CORBA, so no contract declares one>"),
            (TypeCode::LongLong, "long long"),
            (TypeCode::ULongLong, "unsigned long long"),
            (TypeCode::LongDouble, "long double"),
            (TypeCode::WChar, "wchar"),
            (TypeCode::String(0), "string"),
            (TypeCode::WString(64), "wstring<64>"),
            (TypeCode::Fixed { digits: 9, scale: 2 }, "fixed<9,2>"),
            (TypeCode::ObjRef { id: "IDL:m/I:1.0".into(), name: "I".into() }, "I"),
            (
                TypeCode::Struct {
                    id: "IDL:m/Track:1.0".into(),
                    name: "Track".into(),
                    members: vec![member("label", TypeCode::String(0))],
                },
                "Track",
            ),
            (
                TypeCode::Union {
                    id: "IDL:m/U:1.0".into(),
                    name: "U".into(),
                    discriminator: Box::new(TypeCode::Long),
                    default_index: -1,
                    cases: vec![UnionCase {
                        label: vec![0, 0, 0, 1],
                        name: "a".into(),
                        tc: TypeCode::Long,
                    }],
                },
                "U",
            ),
            (
                TypeCode::Enum {
                    id: "IDL:m/E:1.0".into(),
                    name: "E".into(),
                    members: vec!["ONE".into()],
                },
                "E",
            ),
            (TypeCode::Sequence { element: Box::new(TypeCode::Long), bound: 0 }, "sequence<long>"),
            (TypeCode::Array { element: Box::new(TypeCode::Octet), length: 16 }, "octet[16]"),
            (
                TypeCode::Alias {
                    id: "IDL:m/Id:1.0".into(),
                    name: "Id".into(),
                    aliased: Box::new(TypeCode::String(0)),
                },
                "Id",
            ),
            (
                TypeCode::Except {
                    id: "IDL:m/Bad:1.0".into(),
                    name: "Bad".into(),
                    members: Vec::new(),
                },
                "Bad",
            ),
            (
                TypeCode::Value {
                    id: "IDL:m/Money:1.0".into(),
                    name: "Money".into(),
                    modifier: 0,
                    base: None,
                    members: vec![ValueMember {
                        name: "amount".into(),
                        tc: TypeCode::Double,
                        visibility: 1,
                    }],
                },
                "valuetype Money (IDL:m/Money:1.0)",
            ),
            (
                TypeCode::AbstractInterface {
                    id: "IDL:m/Describable:1.0".into(),
                    name: "Describable".into(),
                },
                "abstract interface Describable (IDL:m/Describable:1.0)",
            ),
            (
                TypeCode::Native { id: "IDL:m/Handle:1.0".into(), name: "Handle".into() },
                "native Handle (IDL:m/Handle:1.0)",
            ),
            (
                TypeCode::Recursive("IDL:m/Node:1.0".into()),
                "<recursion back to IDL:m/Node:1.0, which has no finite form>",
            ),
        ]
    }

    /// The batch's whole claim: **every** variant has a verdict a reader can
    /// act on, and the ones that do not have a name say why *they* have none.
    ///
    /// This is what `_ => "<unnamed type>"` made unwritable. `LongDouble` and
    /// `TypeCode` are the two the catch-all was hiding that the v1 wire
    /// marshals in both directions, and `::CORBA::TypeCode` is the type
    /// `orbweaver-registry` was deliberately fixed to stop calling `void`.
    #[test]
    fn every_typecode_variant_is_rendered_by_a_verdict_of_its_own() {
        let table = every_variant();
        let tags: BTreeSet<&str> = table.iter().map(|(tc, _)| variant_tag(tc)).collect();
        assert_eq!(
            tags.len(),
            TYPECODE_VARIANTS,
            "the table must carry one instance of every variant; missing {:?}",
            TYPECODE_VARIANTS - tags.len()
        );
        for (tc, want) in &table {
            assert_eq!(&render_type(tc), want, "{} renders wrong", variant_tag(tc));
        }
    }

    /// The placeholder set is closed, and it is exactly the three variants no
    /// declaration can produce.
    ///
    /// The good half of the retired comment — *a prompt that invents a type
    /// name teaches the producer to quote one back* — is a reason for a
    /// placeholder and was never a reason to hide a supported type behind one.
    #[test]
    fn only_the_three_undeclarable_variants_render_as_a_placeholder() {
        let placeheld: Vec<&str> = every_variant()
            .iter()
            .filter(|(_, want)| want.starts_with('<'))
            .map(|(tc, _)| variant_tag(tc))
            .collect();
        assert_eq!(placeheld, ["Null", "Principal", "Recursive"]);
        assert!(
            !every_variant().iter().any(|(_, want)| want.contains("unnamed type")),
            "no variant may go back to the one sentence that covered nine"
        );
    }

    /// Where it showed: the operation signature line a model and a human read.
    #[test]
    fn the_two_marshalled_types_reach_the_prompt_named() {
        let price = Evidence::of("price", &sig(false, TypeCode::LongDouble, vec![]));
        assert!(price.to_line().starts_with("price() -> long double"), "{}", price.to_line());
        let describe = Evidence::of(
            "describe",
            &sig(
                false,
                TypeCode::TypeCode,
                vec![ParamSig {
                    name: "of".into(),
                    tc: TypeCode::TypeCode,
                    direction: ParamDirection::In,
                    annotations: BTreeMap::new(),
                }],
            ),
        );
        assert!(
            describe
                .to_line()
                .starts_with("describe(in ::CORBA::TypeCode of) -> ::CORBA::TypeCode"),
            "{}",
            describe.to_line()
        );
    }

    /// The bounded forms keep their bound; the unbounded ones keep their bare
    /// keyword. Two arms guarded by a literal `0`, which an exhaustive match
    /// does not check for.
    #[test]
    fn a_bound_is_rendered_and_an_absent_bound_is_not() {
        assert_eq!(render_type(&TypeCode::String(0)), "string");
        assert_eq!(render_type(&TypeCode::String(32)), "string<32>");
        assert_eq!(render_type(&TypeCode::WString(0)), "wstring");
        assert_eq!(render_type(&TypeCode::WString(8)), "wstring<8>");
        assert_eq!(
            render_type(&TypeCode::ObjRef {
                id: "IDL:CORBA/Object:1.0".into(),
                name: String::new()
            }),
            "Object",
            "a peer-built reference with no simple name is still `Object`"
        );
    }

    // ── the caution: a reference inside a constructed type is still one ──────

    /// `is_reference` ended in `_ => false`, so a reference reached the caller
    /// unmarked whenever it travelled inside a struct, a union, an exception or
    /// a valuetype's state — the direction that **removes** a caution.
    #[test]
    fn a_reference_inside_a_constructed_type_is_still_handed_out() {
        let obj = || TypeCode::ObjRef { id: "IDL:m/I:1.0".into(), name: "I".into() };
        let handle = TypeCode::Struct {
            id: "IDL:m/Handle:1.0".into(),
            name: "Handle".into(),
            members: vec![member("id", TypeCode::String(0)), member("it", obj())],
        };
        assert!(Evidence::of("get_root", &sig(false, handle.clone(), vec![])).hands_out_reference);
        // And through one more layer, which is how a real contract carries it.
        assert!(
            Evidence::of(
                "list_roots",
                &sig(false, TypeCode::Sequence { element: Box::new(handle), bound: 0 }, vec![]),
            )
            .hands_out_reference
        );
        // The two stated `false` answers, so neither drifts into a guess.
        assert!(!is_reference(&TypeCode::Any), "an `any`'s content type is a runtime value");
        assert!(
            !is_reference(&TypeCode::Recursive("IDL:m/Node:1.0".into())),
            "the enclosing type's own members are walked directly"
        );
        assert!(
            is_reference(&TypeCode::AbstractInterface {
                id: "IDL:m/D:1.0".into(),
                name: "D".into(),
            }),
            "on the wire it is the union of a value and a reference"
        );
    }

    #[test]
    fn a_proposal_round_trips_through_its_json() {
        let p = Proposal::parse(&proposal_json("unknown")).expect("parses");
        assert_eq!(Proposal::parse(&p.to_text()).expect("re-parses"), p);
    }

    #[test]
    fn a_fenced_proposal_is_still_read() {
        let fenced = format!("```json\n{}\n```", proposal_json("unknown"));
        assert!(Proposal::parse(&fenced).is_ok());
    }
}
