//! Type registry: the IFR-equivalent store.
//!
//! `docs/PLAN.md` §2.1 rests on CORBA already having "a runtime self-describing
//! type system", and the Interface Repository is the part of that claim this
//! crate makes true. It turns parsed IDL into something queryable at runtime:
//! repository ids, an inheritance graph, operation signatures, and the
//! `TypeCode` for every type.
//!
//! # Why it is first-party rather than a client of a remote IFR
//!
//! Phase 1 risk R2: real deployments frequently run no Interface Repository at
//! all. A registry we populate from IDL works whether or not the target has
//! one, and `_is_a` answered from our own inheritance graph is both faster and
//! available when the target is unreachable (§4.7).
//!
//! When a deployment *does* run one — and has no IDL files left anywhere —
//! [`ingest`] populates a registry by calling it. Those entries are marked:
//! see [`Origin`], [`Registry::is_ingested`] and [`Registry::touches_ingested`].
//! The marking is not decoration. An entry that came off the wire has not been
//! through S4, carries no SIDL annotations for the guard to key on, and was
//! written by a peer we do not control, so every downstream gate needs to be
//! able to tell the two apart.
//!
//! # Scope
//!
//! Derives what the wire needs. A repository id is `IDL:` plus the qualified
//! name plus `:1.0` — unless the IDL says otherwise, which it does through
//! `#pragma prefix`, `#pragma version` and `#pragma ID`. Those are resolved by
//! the front end ([`orbweaver_idl::ast::Spec::repository_ids`], which records
//! only the *differences* from the plain derivation) and this crate does no
//! id arithmetic of its own beyond applying them.
//!
//! **Why the front end and not here.** All three pragmas are positional — a
//! prefix runs from where it is written to the end of its scope — and source
//! order is the parser's to know. `orbweaver_idl::parse`'s module docs state
//! exactly which forms are honoured and which are not; read them before
//! trusting an id.
//!
//! The IDL-4 `typeid` and `typeprefix` *keywords* are still not honoured. They
//! are reserved words to our lexer, so a file using them fails at the grammar
//! rather than being silently mis-identified — which is the safe direction for
//! something that decides identity.

#![deny(missing_docs)]

pub mod approval;
pub mod diff;
pub mod ifr;
pub mod ingest;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use orbweaver_giop::typecode::{
    Member as TcMember, TypeCode, UnionCase as TcUnionCase, ValueMember as TcValueMember,
};
use orbweaver_idl::ast::*;
use orbweaver_idl::include::{SearchPath, Unit, preprocess_file};
use orbweaver_idl::sema::Diagnostic;

/// A CORBA repository identifier, e.g. `IDL:spike/Echo:1.0`.
pub type RepositoryId = String;

/// Why a registry could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryError {
    /// What went wrong, phrased as something to fix.
    pub message: String,
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RegistryError {}

/// What a registered entry is.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// An interface, with its operations and attributes.
    Interface(InterfaceEntry),
    /// A data type, with the `TypeCode` describing it.
    Type(TypeCode),
    /// A constant.
    Const {
        /// Its type.
        tc: TypeCode,
        /// Its value, evaluated against that type, or `None` when the registry
        /// could not evaluate it. See [`ConstValue`] for what "value" means
        /// here and why it is not the expression.
        value: Option<ConstValue>,
    },
}

/// The value of an IDL constant: an **evaluated literal**, not the expression.
///
/// # Why evaluated, and evaluated here
///
/// `const long OFFSET = MAX_RETRIES * 2;` is the whole argument. Storing the
/// expression would mean every consumer — the Rust generator, the MCP bridge,
/// anything that wants to *print* a contract — carries its own constant folder
/// **and** its own copy of IDL's outward scope resolution, because `MAX_RETRIES`
/// is only findable from the name table the registry builds while it loads.
/// That is the duplication `orbweaver-gen`'s "never encoding rules" rule
/// forbids for marshalling, applied to arithmetic: three folders will disagree,
/// and the one that disagrees silently is the one that ships.
///
/// So folding happens once, where the names are, and consumers read a value.
///
/// # What happens to an expression that cannot be folded
///
/// `value` is `None` and **nothing is invented**. The registry never stores a
/// guessed zero, and a consumer that cannot emit without a value must say so
/// the way [`orbweaver_gen`] says it for a deferred wire type — as a skip with
/// a reason, not as a plausible wrong number. It is `None` in exactly three
/// cases: an operand the folder cannot evaluate (an unresolved name, a name
/// that is neither a constant nor an enumerator, a form the folder does not
/// implement), an operation that has no answer, and a result outside the
/// declared type's range (`const octet O = 300`), which is an IDL error the
/// checker reports and this module refuses to launder into a truncated byte.
///
/// The **count** is unchanged and the middle case's examples were stale. They
/// read "division or modulo by zero, an overflowing negation" — the integer
/// folder's three — while the decimal folder landed on 2026-08-21 with two more
/// of exactly that kind: `1.0d / 3.0d`, which has no exact decimal and no IDL
/// rule saying what scale to round it to, so `fixed_op` has no division arm at
/// all; and a decimal product past `i128`, which is checked rather than wrapped.
/// Both are held by `const_values.rs`'s
/// `a_fixed_expression_that_cannot_be_exact_has_no_value`, which has been green
/// over an example list that did not mention either — a test can pin the
/// behaviour and still not pin the sentence describing it.
///
/// A value that folded is exactly what the declared type says: the coercion
/// runs before storage, so a consumer never has to ask whether an `Int` under a
/// `double` means 3 or 3.0.
///
/// [`orbweaver_gen`]: https://docs.rs/orbweaver-gen
#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    /// Every integer type, and `char`/`wchar`/`octet` as the code point they
    /// denote — the declared [`TypeCode`] says which of those it is, so a
    /// second variant would only be a second way to be wrong about it.
    ///
    /// `i128` rather than `i64` for exactly one type: `unsigned long long`'s
    /// upper half. `const unsigned long long M = 18446744073709551615;` is
    /// ordinary IDL that omniidl accepts, and while this was an `i64` the
    /// value could not be represented at any layer — the lexer refused the
    /// literal before the question reached here.
    Int(i128),
    /// A `fixed` constant, as the decimal it was written as.
    ///
    /// Not a [`ConstValue::Float`], for the reason
    /// [`orbweaver_idl::lex::FixedLit`] gives at length: `9.9` has no `f64`,
    /// so folding a `fixed` into one loses the value before any consumer sees
    /// it. `unscaled` carries the sign; the value is `unscaled / 10^scale`.
    Fixed {
        /// The digits as one signed integer, point removed.
        unscaled: i128,
        /// How many of them fall right of the point.
        scale: u16,
    },
    /// `float` and `double`, unrounded: a `float` constant keeps the value as
    /// written, and narrowing to `f32` is the consumer's to do at the point it
    /// emits one. Never infinite and never NaN — neither is expressible in IDL
    /// and an expression that produced one did not fold.
    Float(f64),
    /// `boolean`.
    Bool(bool),
    /// `string` and `wstring`, as the source spelled it.
    Str(String),
    /// A constant whose declared type is an enum.
    Enum {
        /// Repository id of the enum the enumerator belongs to.
        id: RepositoryId,
        /// The enumerator's name.
        member: String,
        /// Its ordinal, which is what travels on the wire.
        ordinal: u32,
    },
}

impl ConstValue {
    /// A `fixed` constant as the decimal it is, sign included and no `d`
    /// suffix — `12.5`, `-0.001`, `0`. `None` for every other variant.
    ///
    /// Rendering lives here rather than in each consumer because the scale is
    /// the part that is easy to get wrong: `Fixed { unscaled: 1, scale: 3 }`
    /// is `0.001`, and a consumer that divides by `10f64.powi(3)` to print it
    /// has re-introduced the binary float this type exists to avoid.
    #[must_use]
    pub fn as_decimal(&self) -> Option<String> {
        let ConstValue::Fixed { unscaled, scale } = self else { return None };
        let sign = if *unscaled < 0 { "-" } else { "" };
        let mag = unscaled.unsigned_abs();
        if *scale == 0 {
            return Some(format!("{sign}{mag}"));
        }
        let s = format!("{:0>width$}", mag, width = usize::from(*scale) + 1);
        let split = s.len() - usize::from(*scale);
        Some(format!("{sign}{}.{}", &s[..split], &s[split..]))
    }
}

/// A registered interface.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InterfaceEntry {
    /// Repository ids of the direct bases, in declaration order.
    pub bases: Vec<RepositoryId>,
    /// Operations, keyed by name.
    pub operations: BTreeMap<String, OperationSig>,
    /// Attributes, keyed by name.
    pub attributes: BTreeMap<String, AttributeSig>,
    /// Whether only a forward declaration was seen.
    pub forward_only: bool,
    /// Whether it was declared `abstract interface`.
    ///
    /// Recorded here and not left to be inferred from the `TypeCode`, because
    /// an interface's own entry is an [`Entry::Interface`] and has no
    /// `TypeCode` to inspect: a consumer asking "may I generate a stub for
    /// this?" reaches the entry, not the reference. `docs/PLAN.md` §4.4 defers
    /// abstract interfaces from the v1 wire, and until this field existed the
    /// generators had no way to tell one from a plain interface and emitted a
    /// stub for `gc20::Describable` that a peer would never answer.
    ///
    /// Not set by [`Registry::define_ingested`]'s callers: the Interface
    /// Repository's `FullInterfaceDescription` does not carry it, so an
    /// ingested interface is `false` — "not known to be abstract", which is
    /// what a remote IFR can honestly tell us.
    pub abstract_interface: bool,
}

/// A reference the registry could not resolve to a registered id, kept rather
/// than dropped.
///
/// # Why a recorded marker and not an error
///
/// [`Registry::load`] is documented as accumulating: call it repeatedly and
/// several files become one repository. It therefore cannot know, at the moment
/// a name fails to resolve, whether the name is genuinely absent or merely not
/// loaded yet — so refusing the load would make a documented usage impossible,
/// and refusing it *sometimes* would be worse than either.
///
/// # Why not silence either
///
/// Silence is what made the estate defect invisible. The base name of nine of
/// the estate's twelve interfaces lived in a file that `#include` had not
/// resolved; the resolution failed; a `filter_map` dropped it; and the registry
/// then said, truthfully as far as it knew, that those interfaces had no
/// ancestry at all. Nothing downstream could tell "declares no base" from
/// "declares a base I could not find", so the console drew 58 of 76 reachable
/// operations and reported no problem.
///
/// So: the load still succeeds, and what it could not resolve is on the record.
/// Deciding what to do about it belongs to the tool — a gate refuses
/// ([`idl-diff`] exits 2 rather than issuing a verdict from a registry it knows
/// is incomplete), a viewer says so beside what it drew.
///
/// *로드는 성공하되, 해석하지 못한 것은 기록된다. 침묵이 결함을 보이지 않게
/// 만들었다. 어떻게 처리할지는 도구가 정한다.*
///
/// [`idl-diff`]: https://docs.rs/orbweaver-registry
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Unresolved {
    /// The qualified name of the definition the reference was written in, e.g.
    /// `Ledger::Journal` or `Ledger::Journal::fetch`.
    pub at: String,
    /// What kind of reference it was.
    pub kind: UnresolvedKind,
    /// The name as the IDL spelled it, e.g. `Recorded` or `::Freight::NotFound`.
    pub name: String,
}

impl std::fmt::Display for Unresolved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self.kind {
            UnresolvedKind::Base => "base interface",
            UnresolvedKind::Raises => "raised exception",
            UnresolvedKind::Type => "type name",
        };
        write!(f, "{}: {what} `{}` is not declared in this unit", self.at, self.name)
    }
}

/// Which kind of reference an [`Unresolved`] records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UnresolvedKind {
    /// A name in an interface's inheritance list. The expensive one: it costs
    /// every operation, attribute and `_is_a` answer the base would have
    /// contributed.
    Base,
    /// A name in an operation's `raises` clause. It costs the caller the
    /// ability to recognise the exception it is handed.
    Raises,
    /// A name used as a type — a member's, a parameter's, a return value's, a
    /// sequence element's. It costs the *bytes*: an unresolved type became
    /// `void` in the `TypeCode`, and `void` marshals nothing where a peer
    /// expects a value.
    ///
    /// Added when the fix for inherited-scope resolution made the question
    /// "what does a marker mean" answerable: measured, `idl-diff` accepted
    /// `corpus/negative/n04-unknown-type.idl` — `struct S { Widget w; };` —
    /// and exited 0, because only bases and `raises` were ever recorded. The
    /// marker was not ambiguous; it was incomplete, which reads the same from
    /// the gate's side. *마커는 모호했던 것이 아니라 불완전했다.*
    Type,
}

/// An operation's shape, which is what a dynamic invoker needs to build a call.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationSig {
    /// Return type.
    pub returns: TypeCode,
    /// Parameters in declaration order.
    pub params: Vec<ParamSig>,
    /// Repository ids of the exceptions it may raise.
    pub raises: Vec<RepositoryId>,
    /// Whether it is `oneway`, which means no reply is expected.
    pub oneway: bool,
    /// SIDL annotations, the semantics an agent reads (§2.2).
    pub annotations: BTreeMap<String, String>,
}

/// One parameter of an operation.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamSig {
    /// Name as written.
    pub name: String,
    /// Direction.
    pub direction: ParamDirection,
    /// Type.
    pub tc: TypeCode,
    /// SIDL annotations on this parameter.
    pub annotations: BTreeMap<String, String>,
}

/// Parameter direction, mirrored here so consumers need not depend on the AST.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum ParamDirection {
    In,
    Out,
    InOut,
}

/// An attribute's shape.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeSig {
    /// Attribute type.
    pub tc: TypeCode,
    /// Whether it is read-only.
    pub readonly: bool,
    /// SIDL annotations.
    pub annotations: BTreeMap<String, String>,
}

/// Where a registered entry came from.
///
/// The distinction is a trust boundary, not bookkeeping. An [`Origin::Idl`]
/// entry was parsed from IDL text this project holds, which means it passed
/// S4's gate and may carry SIDL annotations. An [`Origin::Ingested`] entry was
/// described to us over the wire by a peer we do not control: it passed no
/// gate, carries no annotations, and is exactly the "tool poisoning via remote
/// metadata" vector in PLAN §9.0. Anything deciding what an agent may see or
/// call has to be able to ask which it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Declared in IDL we hold, and therefore through S4.
    Idl,
    /// Described by a remote Interface Repository, named by the source label
    /// the ingestion ran under.
    Ingested(String),
}

/// Why [`Registry::define_ingested`] refused to register an entry.
///
/// Both variants are refusals to *overwrite*. The registry never lets a remote
/// description displace something already registered, because the interesting
/// attack is not a malformed reply — it is a well-formed one that quietly
/// replaces a locally-defined contract with a compatible-looking remote one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefineError {
    /// The repository id is already registered, with this provenance.
    IdInUse(Origin),
    /// The qualified IDL name is already bound to a different repository id —
    /// the same clash one version digit away (`IDL:a/B:1.0` against
    /// `IDL:a/B:2.0`, both of which are `a::B`).
    NameInUse(RepositoryId),
}

impl std::fmt::Display for DefineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DefineError::IdInUse(Origin::Idl) => {
                write!(
                    f,
                    "already defined locally from IDL; a remote description may not replace it"
                )
            }
            DefineError::IdInUse(Origin::Ingested(src)) => {
                write!(f, "already ingested from {src:?}; a second source may not replace it")
            }
            DefineError::NameInUse(id) => write!(f, "its qualified name is already bound to {id}"),
        }
    }
}

impl std::error::Error for DefineError {}

/// The registry.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    entries: BTreeMap<RepositoryId, Entry>,
    /// Qualified IDL name (`spike::Echo`) to repository id.
    by_name: HashMap<String, RepositoryId>,
    /// The inverse, which stops being derivable once a prefix is in play.
    ///
    /// `IDL:acme.com/bank/Account:1.0` is `bank::Account`, not
    /// `acme.com::bank::Account`: the prefix is part of the identity and no
    /// part of the name. Splitting the id cannot tell the two apart — an id
    /// alone does not say how many leading segments are prefix — so the
    /// mapping is recorded when the IDL is loaded instead of recomputed.
    by_id: BTreeMap<RepositoryId, String>,
    /// SIDL annotations attached to a registered entry.
    annotations: BTreeMap<RepositoryId, BTreeMap<String, String>>,
    /// Ids whose provenance is a remote IFR, mapped to the source label.
    /// Absent means the entry came from IDL — the safe default, since a bug
    /// that forgot to mark something would then under-report trust rather
    /// than over-report it.
    ingested: BTreeMap<RepositoryId, String>,
    /// References a load could not resolve. See [`Unresolved`] for why they are
    /// kept instead of dropped, and why keeping them is not the same as
    /// failing the load.
    unresolved: Vec<Unresolved>,
}

impl Registry {
    /// An empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads a parsed specification.
    ///
    /// Call it repeatedly to accumulate several files; later definitions of the
    /// same repository id replace earlier forward declarations.
    pub fn load(&mut self, spec: &Spec) -> Result<(), RegistryError> {
        let mut names = NameTable::default();
        names.collect(&[], &spec.definitions);
        let mut builder = Builder {
            reg: self,
            names,
            in_progress: Vec::new(),
            overrides: spec.repository_ids.clone(),
        };
        builder.walk(&[], &spec.definitions);
        self.unresolved.sort();
        self.unresolved.dedup();
        Ok(())
    }

    /// Everything the loads so far referred to and could not find.
    ///
    /// Empty is the normal answer for a resolved translation unit. A non-empty
    /// answer means this registry describes less than the IDL did, and any tool
    /// about to make a decision from it — a release verdict, a drawn catalog, a
    /// generated stub — should say so rather than present a partial graph as a
    /// whole one. See [`Unresolved`].
    pub fn unresolved(&self) -> &[Unresolved] {
        &self.unresolved
    }

    /// Every registered repository id, in sorted order.
    pub fn ids(&self) -> impl Iterator<Item = &RepositoryId> {
        self.entries.keys()
    }

    /// The catalog entry kinds a deployment might expose, for a bridge that
    /// wants to build an allowlist from the registry rather than by hand.
    ///
    /// Returns interfaces only: a bare type is not callable, and exposing one
    /// would mean nothing.
    ///
    /// # Why this lives here and not at the agent boundary
    ///
    /// It was `orbweaver_mcp::exposable_interfaces` until 2026-08-26, and it
    /// was **the entire source-level reason `orbweaver-forge` depended on
    /// `orbweaver-mcp`** — one call, in S5. That edge pointed the wrong way:
    /// the boundary crate sat *upstream* of the pipeline it exists to expose,
    /// so `orbweaver-mcp` could not call S4, `idl-diff` or `gen` without a
    /// cycle, and D024 §5's four contract tools could not be written where the
    /// tool surface is. It also put the annotate-or-assume sentence out of
    /// reach of its own owner in the other direction.
    ///
    /// The question this answers — *which entries in this catalog could be
    /// exposed at all* — is a property of the catalog and of nothing else. It
    /// reads a `Registry` and returns ids; it knows no `Exposure`, no caller
    /// and no policy. Asking the owner is what CLAUDE.md's *where a fact lives*
    /// rule prescribes, and the owner is the registry.
    ///
    /// `orbweaver_mcp::exposable_interfaces` remains as a re-export, so no
    /// caller had to move with it.
    ///
    /// *이 함수 하나가 forge → mcp 의존의 전부였고, 그 방향이 경계 크레이트를
    /// 파이프라인보다 위에 두어 D024 §5의 네 도구를 쓸 수 없게 만들고 있었다.
    /// 질문 자체가 카탈로그의 속성이므로 집은 여기다.*
    pub fn exposable_interfaces(&self) -> Vec<String> {
        self.ids().filter(|id| matches!(self.get(id), Some(Entry::Interface(_)))).cloned().collect()
    }

    /// Registers `entry` under `id` with a remote provenance, or refuses.
    ///
    /// This is the *only* way an entry enters the registry without IDL behind
    /// it, and it is deliberately a refusal rather than an insert: an id that
    /// is already registered is never replaced, whatever its provenance. The
    /// asymmetry is the point — [`Registry::load`] may overwrite an ingested
    /// entry (local IDL is authoritative and clears the mark), and ingestion
    /// may never overwrite anything.
    pub fn define_ingested(
        &mut self,
        id: RepositoryId,
        entry: Entry,
        source: &str,
    ) -> std::result::Result<(), DefineError> {
        if let Some(origin) = self.origin(&id) {
            return Err(DefineError::IdInUse(origin));
        }
        if let Some(name) = qualified_of_id(&id) {
            if let Some(held) = self.by_name.get(&name)
                && *held != id
            {
                return Err(DefineError::NameInUse(held.clone()));
            }
            self.by_name.insert(name, id.clone());
            // Deliberately not mirrored into `by_id`: the name above is a
            // guess from the id's shape, and `qualified_name` promises the
            // recorded answer or none at all.
        }
        self.ingested.insert(id.clone(), source.to_owned());
        self.entries.insert(id, entry);
        Ok(())
    }

    /// Registers an entry derived from IDL, clearing any ingested mark.
    fn define_local(&mut self, id: RepositoryId, entry: Entry) {
        self.ingested.remove(&id);
        self.entries.insert(id, entry);
    }

    /// Where the entry registered under `id` came from, or `None` if nothing
    /// is registered there.
    pub fn origin(&self, id: &str) -> Option<Origin> {
        if !self.entries.contains_key(id) {
            return None;
        }
        Some(match self.ingested.get(id) {
            Some(source) => Origin::Ingested(source.clone()),
            None => Origin::Idl,
        })
    }

    /// Whether `id` was described to us by a remote Interface Repository.
    pub fn is_ingested(&self, id: &str) -> bool {
        self.ingested.contains_key(id)
    }

    /// Every ingested repository id, in sorted order.
    pub fn ingested_ids(&self) -> impl Iterator<Item = &RepositoryId> {
        self.ingested.keys()
    }

    /// Whether `id` or anything it inherits from came off the wire.
    ///
    /// This, not [`Registry::is_ingested`], is the question an exposure gate
    /// asks. Provenance is contagious upwards through inheritance: a locally
    /// defined interface is unaffected by what derives from it, but an entry
    /// with an ingested ancestor has operations in its callable surface that
    /// a remote peer chose — `resolve_operation` walks bases, so an ingested
    /// base is an ingested part of the contract.
    pub fn touches_ingested(&self, id: &str) -> bool {
        self.is_ingested(id) || self.ancestors(id).iter().any(|a| self.is_ingested(a))
    }

    /// Looks an entry up by repository id.
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.get(id)
    }

    /// Looks a repository id up by qualified IDL name, e.g. `spike::Echo`.
    pub fn id_of(&self, qualified: &str) -> Option<&RepositoryId> {
        self.by_name.get(qualified)
    }

    /// The qualified IDL name behind a repository id, e.g. `bank::Account`.
    ///
    /// Not the same as splitting the id on `/`: under a `#pragma prefix` the
    /// leading segments are identity, not scope. Anything read off the wire
    /// rather than loaded from IDL has no recorded name and answers `None`,
    /// because for those the id genuinely is all we know.
    pub fn qualified_name(&self, id: &str) -> Option<&str> {
        self.by_id.get(id).map(String::as_str)
    }

    /// The `TypeCode` of a registered type.
    pub fn typecode(&self, id: &str) -> Option<&TypeCode> {
        match self.entries.get(id) {
            Some(Entry::Type(tc)) => Some(tc),
            _ => None,
        }
    }

    /// The value of a registered constant, or `None` if `id` is not a constant
    /// or its expression did not fold ([`ConstValue`] says when that happens).
    ///
    /// The two `None`s are deliberately not distinguished here: a caller that
    /// needs to tell "not a constant" from "a constant with no value" is asking
    /// about the entry, and [`Registry::get`] answers that.
    pub fn const_value(&self, id: &str) -> Option<&ConstValue> {
        match self.entries.get(id) {
            Some(Entry::Const { value, .. }) => value.as_ref(),
            _ => None,
        }
    }

    /// The interface registered under `id`.
    pub fn interface(&self, id: &str) -> Option<&InterfaceEntry> {
        match self.entries.get(id) {
            Some(Entry::Interface(i)) => Some(i),
            _ => None,
        }
    }

    /// SIDL annotations attached to `id`.
    pub fn annotations(&self, id: &str) -> Option<&BTreeMap<String, String>> {
        self.annotations.get(id)
    }

    /// Answers `_is_a` from the inheritance graph, without a network call.
    ///
    /// §4.7: this is both faster than asking the target and available when the
    /// target is unreachable. Every interface is also a `CORBA::Object`, which
    /// callers rely on when narrowing.
    pub fn is_a(&self, id: &str, base: &str) -> bool {
        if id == base || base == "IDL:omg.org/CORBA/Object:1.0" {
            return self.entries.contains_key(id) || id == base;
        }
        let mut seen = BTreeSet::new();
        self.is_a_inner(id, base, &mut seen)
    }

    fn is_a_inner(&self, id: &str, base: &str, seen: &mut BTreeSet<String>) -> bool {
        if id == base {
            return true;
        }
        if !seen.insert(id.to_owned()) {
            // Cyclic inheritance is illegal, but a registry loaded from
            // unchecked input must not hang because of it.
            return false;
        }
        let Some(Entry::Interface(i)) = self.entries.get(id) else { return false };
        i.bases.iter().any(|b| self.is_a_inner(b, base, seen))
    }

    /// Every repository id `id` inherits from, transitively, excluding itself.
    pub fn ancestors(&self, id: &str) -> Vec<RepositoryId> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        self.ancestors_inner(id, &mut out, &mut seen);
        out
    }

    fn ancestors_inner(&self, id: &str, out: &mut Vec<RepositoryId>, seen: &mut BTreeSet<String>) {
        let Some(Entry::Interface(i)) = self.entries.get(id) else { return };
        for b in &i.bases {
            if seen.insert(b.clone()) {
                out.push(b.clone());
                self.ancestors_inner(b, out, seen);
            }
        }
    }

    /// Finds an operation on an interface or any of its bases.
    ///
    /// Inherited operations are callable, so a lookup that stopped at the
    /// declaring interface would report a perfectly valid call as unknown.
    pub fn resolve_operation(&self, id: &str, op: &str) -> Option<(&RepositoryId, &OperationSig)> {
        if let Some(Entry::Interface(i)) = self.entries.get(id)
            && let Some((k, sig)) = i.operations.get_key_value(op)
        {
            let _ = k;
            return self.entries.get_key_value(id).map(|(rid, _)| (rid, sig));
        }
        for base in self.ancestors(id) {
            if let Some(Entry::Interface(i)) = self.entries.get(&base)
                && i.operations.contains_key(op)
            {
                return self.entries.get_key_value(&base).and_then(|(rid, e)| match e {
                    Entry::Interface(i) => i.operations.get(op).map(|s| (rid, s)),
                    _ => None,
                });
            }
        }
        None
    }
}

// ── reading a contract from a path ───────────────────────────────────────────

/// How much of the front end a load runs before the registry sees the spec.
///
/// Both levels resolve `#include`. The difference is only whether semantic
/// analysis runs, and it exists so that fixing include resolution did not also
/// change what any tool accepts — a tool that used to take grammatical IDL
/// still does, and one that gated on S4 still gates on S4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strictness {
    /// The grammar only — the strictness [`orbweaver_idl::parse`] gives.
    Grammar,
    /// The grammar and semantic analysis — the strictness
    /// [`orbweaver_idl::check`] gives, and what S4 gates on.
    Checked,
}

/// A contract read from a path, with its `#include`s resolved.
///
/// # Why this exists rather than `read_to_string` plus [`orbweaver_idl::parse`]
///
/// [`orbweaver_idl::parse`] takes a **string**, and a string has no directory,
/// so a relative `#include` in it cannot resolve — the front end says so in its
/// own documentation. Twelve binaries read a *path*, threw the path away, and
/// handed the text to the string entry point anyway. Each of them therefore
/// analysed a translation unit with pieces missing, and because a missing piece
/// mostly shows up as a name that does not resolve, and an unresolved name was
/// dropped rather than reported, the loss was silent: the console drew 58 of
/// the estate's 76 reachable operations and said nothing about the other 18.
///
/// The fix is not a better diagnostic at each of the twelve sites. It is that
/// a path is loaded through one function that resolves before it parses, and
/// that function is this one.
///
/// *경로를 가진 도구는 전부 여기를 지난다. 문자열 진입점은 상대 `#include`를
/// 해석할 수 없으며, 해석되지 않은 이름은 조용히 버려졌다.*
#[derive(Debug, Clone)]
pub struct Contract {
    /// The resolved translation unit — the root file, everything it included,
    /// and the map from a position in the spliced text back to the file and
    /// line somebody actually wrote. Keep it beside the spec: it is the only
    /// thing that can turn a span into a location a reader can open.
    pub unit: Unit,
    /// The parsed specification, over the whole unit.
    pub spec: Spec,
}

impl Contract {
    /// Resolves `path`'s includes and parses the result.
    ///
    /// An unresolvable `#include` is an error here and not a warning. Carrying
    /// on without the included file turns one missing `-I` into a diagnostic
    /// for every name that file declared — 90 of them across the thirteen-file
    /// estate — and, worse, into no diagnostic at all wherever the consumer
    /// drops unresolved names instead of reporting them.
    pub fn load(
        path: &Path,
        search: &SearchPath,
        strictness: Strictness,
    ) -> Result<Self, RegistryError> {
        let unit = preprocess_file(path, search)
            .map_err(|e| RegistryError { message: format!("{}: {e}", path.display()) })?;
        if !unit.is_ok() {
            return Err(RegistryError { message: rendered(&unit, &unit.errors) });
        }
        let spec = orbweaver_idl::parse(&unit.text).map_err(|e| RegistryError {
            message: unit.render(&Diagnostic { message: e.message, span: e.span, rule: e.rule }),
        })?;
        if strictness == Strictness::Checked {
            let analysis = orbweaver_idl::analyse(&spec);
            if !analysis.is_ok() {
                return Err(RegistryError { message: rendered(&unit, &analysis.diagnostics) });
            }
        }
        Ok(Contract { unit, spec })
    }
}

/// Builds one registry from one or more contract files.
///
/// `Registry::load` accumulates, so several files become one repository — but
/// each call resolves names against **that call's** spec alone, so an interface
/// in one file cannot inherit a base declared in another by loading both. That
/// is what `#include` is for, and why resolution belongs to [`Contract::load`]
/// rather than to this loop.
pub fn registry_from_files<P: AsRef<Path>>(
    paths: &[P],
    search: &SearchPath,
    strictness: Strictness,
) -> Result<Registry, RegistryError> {
    let mut registry = Registry::new();
    for path in paths {
        let contract = Contract::load(path.as_ref(), search, strictness)?;
        registry.load(&contract.spec)?;
    }
    Ok(registry)
}

/// Pulls `-I <dir>` and `-I<dir>` out of `args`, leaving everything else in
/// order.
///
/// The C convention `omniidl -I` implements, and the one `sidl-validate`
/// already documented. A tool that reads a path needs it for the same reason
/// `omniidl` does: the quoted form finds a neighbour on its own, the angled
/// form never does.
pub fn take_include_dirs(args: &mut Vec<String>) -> Result<SearchPath, RegistryError> {
    let mut search = SearchPath::new();
    let mut rest = Vec::with_capacity(args.len());
    let mut it = std::mem::take(args).into_iter();
    while let Some(a) = it.next() {
        if let Some(tail) = a.strip_prefix("-I") {
            let dir = if tail.is_empty() {
                it.next().ok_or(RegistryError { message: "-I needs a directory".to_owned() })?
            } else {
                tail.to_owned()
            };
            search.push(dir);
        } else {
            rest.push(a);
        }
    }
    *args = rest;
    Ok(search)
}

/// Every diagnostic rendered against the file it was written in, one per line.
fn rendered(unit: &Unit, diagnostics: &[Diagnostic]) -> String {
    diagnostics.iter().map(|d| unit.render(d)).collect::<Vec<_>>().join("\n")
}

/// Builds `IDL:a/b/C:1.0` from a qualified path.
///
/// The derivation with no pragma in play. When IDL is the source, prefer
/// [`Registry::id_of`] or the override map the front end produces: this
/// function cannot know about a `#pragma prefix` because a path does not carry
/// one.
pub fn repository_id(path: &[String]) -> RepositoryId {
    format!("IDL:{}:1.0", path.join("/"))
}

/// The inverse: `IDL:a/b/C:1.0` becomes `a::b::C`.
///
/// `None` for anything not in the `IDL:` format, because the mapping is only
/// defined for that one — inventing a qualified name for an `RMI:` or a `DCE:`
/// id would put a lookup key in the table that nothing can ever match.
///
/// **Approximate once a prefix is involved**, and unavoidably so: it reads
/// `IDL:acme.com/bank/Account:1.0` as `acme.com::bank::Account`, because an id
/// on its own does not say which leading segments are prefix. Use it only for
/// ids that arrived from a peer, where the id is all there is;
/// [`Registry::qualified_name`] is the exact answer for anything loaded from
/// IDL.
pub fn qualified_of_id(id: &str) -> Option<String> {
    let rest = id.strip_prefix("IDL:")?;
    let (path, _version) = rest.rsplit_once(':')?;
    if path.is_empty() {
        return None;
    }
    Some(path.replace('/', "::"))
}

/// The qualified IDL name, e.g. `spike::Echo`.
fn qualified(path: &[String]) -> String {
    path.join("::")
}

/// First pass: every declared name and the path it sits at.
///
/// Needed because IDL permits use before declaration, so `TypeCode` derivation
/// cannot resolve names while it is still discovering them.
#[derive(Default)]
struct NameTable {
    /// Lowercased qualified name to canonical path.
    paths: HashMap<String, Vec<String>>,
    /// Definitions by canonical path, for deriving a `TypeCode` on demand.
    defs: HashMap<String, DefRef>,
    /// Lowercased qualified *enumerator* name to the enum that declares it,
    /// its spelling and its ordinal.
    ///
    /// Enumerators live in the enclosing scope, so `paths` already finds one —
    /// but from an enumerator's own path there is no way back to its enum, and
    /// a constant folding `RED` needs exactly that.
    enumerators: HashMap<String, (Vec<String>, String, u32)>,
    /// Lowercased qualified interface name to its direct bases, each paired
    /// with the scope the base name was *written* in.
    ///
    /// Needed because an interface's scope is not only what it declares:
    /// CORBA §3.15.2 resolves an unqualified name "while taking into
    /// consideration inheritance relationships among interfaces", so the name
    /// table cannot answer a lookup without the inheritance graph. Recording
    /// the base as written — rather than resolved — is what lets a base
    /// declared later in the file still be found, which is the same reason
    /// this table exists at all.
    bases: HashMap<String, Vec<(Vec<String>, ScopedName)>>,
}

#[derive(Clone)]
enum DefRef {
    Struct(StructDef),
    Union(UnionDef),
    Enum(EnumDef),
    Exception(StructDef),
    Typedef(Typedef),
    /// Matched for its kind only: an interface's TypeCode is an object
    /// reference built from the repository id, with no body to consult.
    ///
    /// `abstract` is carried because it changes the *kind*, not the body: a
    /// plain interface is `tk_objref` and an `abstract interface` is
    /// `tk_abstract_interface` (32, measured from omniORB — see
    /// `orbweaver-giop/tests/valuetype_typecode_from_a_peer.rs`). Their
    /// parameter lists are identical, which is exactly why recording one as
    /// the other cost nothing at the TypeCode and everything at the value.
    Interface {
        is_abstract: bool,
    },
    /// Kept whole, because a `valuetype`'s TypeCode has a body: modifier,
    /// concrete base and state members with their visibility.
    ///
    /// It used to be a bare marker mapped to `TypeCode::ObjRef`. See
    /// [`TypeCode::Value`] for what that cost.
    ValueType(Box<ValueTypeDef>),
    /// `native X;` — recorded as [`TypeCode::Native`], which is neither a
    /// reference nor a deferred construct.
    ///
    /// It was `TypeCode::ObjRef` until 2026-08-21, for the same reason a
    /// `valuetype` was: an object reference is a legal thing to emit, so
    /// nothing was red. The previous batch left it alone with an honest
    /// reason — *"no rule names it, so a change here would be a claim no gate
    /// checks"* — and the fix for that was to make the rule name it, not to
    /// leave the wrong answer in place. `wire/deferred-type` now closes over
    /// natives and `orbweaver-gen`'s `deferred_wire_agreement` test holds the
    /// two sets equal, so this *is* checked.
    ///
    /// A native is not deferred: §4.4's three constructs have a wire form this
    /// project has not implemented, and a native has none to implement in any
    /// version. See [`TypeCode::Native`] for what omniORB does when asked.
    ///
    /// Only a `native X;` written in the contract reaches here. The names the
    /// front end predeclares inside module `CORBA`
    /// ([`orbweaver_idl::sema::PREDECLARED_CORBA`]) are not definitions and
    /// never enter the name table; they are answered by
    /// [`predeclared_corba`], which is the only thing standing between them
    /// and `TypeCode::Void`.
    Native,
}

impl NameTable {
    fn collect(&mut self, path: &[String], defs: &[Definition]) {
        for d in defs {
            let mut p = path.to_vec();
            p.push(d.name().text.clone());
            let key = qualified(&p);
            self.paths.insert(key.to_lowercase(), p.clone());
            match d {
                Definition::Module(m) => self.collect(&p, &m.definitions),
                Definition::Interface(i) => {
                    // A body replaces a forward declaration.
                    if i.body.is_some() || !self.defs.contains_key(&key) {
                        let is_abstract = matches!(i.modifier, Some(InterfaceModifier::Abstract));
                        self.defs.insert(key.clone(), DefRef::Interface { is_abstract });
                    }
                    if let Some(body) = &i.body {
                        // A forward declaration carries no bases, so only a
                        // body may set them — and a body arriving after one
                        // replaces nothing, since the two agree.
                        self.bases.insert(
                            key.to_lowercase(),
                            i.bases.iter().map(|b| (path.to_vec(), b.clone())).collect(),
                        );
                        let nested: Vec<Definition> = body
                            .iter()
                            .filter_map(|m| match m {
                                InterfaceMember::Nested(d) => Some(d.clone()),
                                _ => None,
                            })
                            .collect();
                        self.collect(&p, &nested);
                    }
                }
                Definition::Struct(s) => {
                    if s.members.is_some() || !self.defs.contains_key(&key) {
                        self.defs.insert(key, DefRef::Struct(s.clone()));
                    }
                }
                Definition::Exception(s) => {
                    self.defs.insert(key, DefRef::Exception(s.clone()));
                }
                Definition::Union(u) => {
                    self.defs.insert(key, DefRef::Union(u.clone()));
                }
                Definition::Enum(e) => {
                    self.defs.insert(key, DefRef::Enum(e.clone()));
                    // Enumerators live in the enclosing scope.
                    for (i, m) in e.members.iter().enumerate() {
                        let mut ep = path.to_vec();
                        ep.push(m.text.clone());
                        let ekey = qualified(&ep).to_lowercase();
                        self.enumerators
                            .insert(ekey.clone(), (p.clone(), m.text.clone(), i as u32));
                        self.paths.insert(ekey, ep);
                    }
                }
                Definition::Typedef(t) => {
                    self.defs.insert(key, DefRef::Typedef(t.clone()));
                }
                Definition::ValueType(v) => {
                    // A body replaces a forward declaration, as for interfaces:
                    // `valuetype V;` ahead of `valuetype V { … };` must not
                    // erase the members.
                    if v.members.is_some() || !self.defs.contains_key(&key) {
                        self.defs.insert(key, DefRef::ValueType(Box::new(v.clone())));
                    }
                }
                Definition::Native(_) => {
                    self.defs.insert(key, DefRef::Native);
                }
                Definition::Const(_) => {}
            }
        }
    }

    /// Resolves a reference from `scope` outwards, IDL-style.
    ///
    /// CORBA 2.3 §3.15.2 (§7.19.2 in CORBA 3.4), *Scoping Rules and Name
    /// Resolution*: "A name can be used in an unqualified form within a
    /// particular scope; it will be resolved by successively searching farther
    /// out in enclosing scopes, **while taking into consideration inheritance
    /// relationships among interfaces**." The spec's own worked example fixes
    /// the order, and the order is the whole rule — for `N::Y : M::B` it
    /// searches
    ///
    /// 1. the scope of `N::Y`,
    /// 2. the scope of `N::Y`'s base `M::B` — the inherited scope,
    /// 3. the scope of module `N`,
    /// 4. the global scope,
    ///
    /// so a name declared in the base wins over the same name in the enclosing
    /// module. A base's scope is searched *before* stepping outward, not after.
    ///
    /// This walked lexical scopes only, which is why
    /// `corpus/services/gen-naming-subset.idl` — `NamingContextExt :
    /// NamingContext` raising the `NotFound` its base declares, exactly as OMG
    /// writes it — recorded five [`Unresolved`] markers and made `idl-diff`
    /// exit 2 over a contract omniidl and JacORB both accept. A gate that cries
    /// wolf gets bypassed.
    ///
    /// Inheritance is a graph, so the walk is one too: a base's bases count
    /// (§3.15.1 admits an identifier "inherited into" a scope, without limiting
    /// how far), a diamond contributes one name rather than two because
    /// "[t]wo shadow copies of the same original ... introduce a single name
    /// into the derived interface and don't conflict with each other", and a
    /// cycle terminates — `seen` gives both.
    ///
    /// **Ambiguity is not decided here.** The spec makes the *same* name
    /// inherited from two *different* originals an error that must be qualified
    /// at the use site; diagnosing it is `orbweaver_idl::sema`'s job, and this
    /// follows declaration order the way that checker does. A registry that
    /// refused to resolve would turn a front-end diagnostic into a silent
    /// hole here.
    ///
    /// *상속 범위는 바깥 범위보다 먼저 탐색된다 (CORBA §3.15.2). 모호성 판정은
    /// 프론트엔드의 몫이다.*
    fn resolve(&self, scope: &[String], name: &ScopedName) -> Option<Vec<String>> {
        if name.absolute {
            return self.resolve_rooted(&[], &name.parts);
        }
        // Try the innermost scope first, then each enclosing one.
        for cut in (0..=scope.len()).rev() {
            if let Some(p) = self.resolve_rooted(&scope[..cut], &name.parts) {
                return Some(p);
            }
        }
        None
    }

    /// `parts` read as a qualified name rooted at `scope`, with no widening.
    ///
    /// §3.15.1: a qualified name is resolved "by first resolving the qualifier
    /// `<scoped-name>` to a scope S, and then locating the definition of
    /// `<identifier>` within S. The identifier must be directly defined in S or
    /// (if S is an interface) inherited into S. The `<identifier>` is not
    /// searched for in enclosing scopes." Hence [`Self::lookup`] per component
    /// rather than one lookup of the joined name: `NamingContextExt::NotFound`
    /// names something real even though nothing is declared at that path.
    fn resolve_rooted(&self, scope: &[String], parts: &[String]) -> Option<Vec<String>> {
        let (first, rest) = parts.split_first()?;
        let mut path = self.lookup(scope, first)?;
        for part in rest {
            path = self.lookup(&path, part)?;
        }
        Some(path)
    }

    /// One identifier, in one scope and in every scope that scope inherits.
    fn lookup(&self, scope: &[String], ident: &str) -> Option<Vec<String>> {
        self.lookup_seen(scope, ident, &mut BTreeSet::new())
    }

    fn lookup_seen(
        &self,
        scope: &[String],
        ident: &str,
        seen: &mut BTreeSet<String>,
    ) -> Option<Vec<String>> {
        let here = qualified(scope).to_lowercase();
        let mut key = here.clone();
        if !key.is_empty() {
            key.push_str("::");
        }
        key.push_str(&ident.to_lowercase());
        if let Some(p) = self.paths.get(&key) {
            return Some(p.clone());
        }
        // Only an interface has anything to inherit; for everything else the
        // lookup ends here and this reduces to what it always did.
        if !seen.insert(here.clone()) {
            return None;
        }
        for (at, base) in self.bases.get(&here).into_iter().flatten() {
            // A base's *own* name is never itself an inherited one: the IDL
            // grammar has no interface declaration inside an interface body,
            // so every interface name sits in a module or at file scope.
            // Resolving it lexically is therefore complete, and it is also
            // what keeps this recursion finite — the only way back into
            // `lookup_seen` is through `seen`.
            let Some(base_path) = self.resolve_lexical(at, base) else { continue };
            if let Some(p) = self.lookup_seen(&base_path, ident, seen) {
                return Some(p);
            }
        }
        None
    }

    /// Enclosing scopes only. See [`Self::lookup_seen`] for why a base name
    /// needs no more than this.
    fn resolve_lexical(&self, scope: &[String], name: &ScopedName) -> Option<Vec<String>> {
        let tail = name.parts.join("::").to_lowercase();
        if name.absolute {
            return self.paths.get(&tail).cloned();
        }
        for cut in (0..=scope.len()).rev() {
            let mut key = scope[..cut].join("::").to_lowercase();
            if !key.is_empty() {
                key.push_str("::");
            }
            key.push_str(&tail);
            if let Some(p) = self.paths.get(&key) {
                return Some(p.clone());
            }
        }
        None
    }
}

struct Builder<'a> {
    reg: &'a mut Registry,
    names: NameTable,
    /// Repository ids currently being derived, for recursion detection.
    in_progress: Vec<RepositoryId>,
    /// Ids the front end resolved from identity pragmas, by qualified name.
    /// Empty for every file without one, which is why loading such a file is
    /// byte-for-byte what it was before pragmas existed.
    overrides: BTreeMap<String, String>,
}

impl Builder<'_> {
    /// The repository id of the definition at `path`.
    ///
    /// One place, so that a base interface declared under a *different*
    /// `#pragma prefix` gets its own id rather than the deriving scope's — an
    /// `_is_a` walk built from re-derived ids would agree with itself and with
    /// nobody else.
    fn id_for(&self, path: &[String]) -> RepositoryId {
        match self.overrides.get(&qualified(path)) {
            Some(id) => id.clone(),
            None => repository_id(path),
        }
    }

    /// Records a name that did not resolve, so the gap is queryable rather
    /// than merely absent. See [`Unresolved`].
    fn note_unresolved(&mut self, at: &str, kind: UnresolvedKind, name: &ScopedName) {
        self.reg.unresolved.push(Unresolved { at: at.to_owned(), kind, name: name.text() });
    }

    fn walk(&mut self, path: &[String], defs: &[Definition]) {
        for d in defs {
            let mut p = path.to_vec();
            p.push(d.name().text.clone());
            let id = self.id_for(&p);
            self.reg.by_name.insert(qualified(&p), id.clone());
            self.reg.by_id.insert(id.clone(), qualified(&p));

            match d {
                Definition::Module(m) => {
                    self.reg.by_name.remove(&qualified(&p));
                    self.walk(&p, &m.definitions);
                }
                Definition::Interface(i) => {
                    self.register_annotations(&id, &i.annotations);
                    let entry = self.interface_entry(&p, i);
                    // A body must not be replaced by a later forward
                    // declaration of the same name.
                    let keep = matches!(
                        self.reg.entries.get(&id),
                        Some(Entry::Interface(prev)) if !prev.forward_only
                    ) && entry.forward_only;
                    if !keep {
                        self.reg.define_local(id, Entry::Interface(entry));
                    }
                    if let Some(body) = &i.body {
                        let nested: Vec<Definition> = body
                            .iter()
                            .filter_map(|m| match m {
                                InterfaceMember::Nested(d) => Some(d.clone()),
                                _ => None,
                            })
                            .collect();
                        self.walk(&p, &nested);
                    }
                }
                Definition::Const(c) => {
                    let tc = self.type_of(path, &c.ty);
                    // Folded against the *enclosing* scope, which is where the
                    // constant's own expression resolves names from, and in
                    // source order, which is the order IDL requires a constant
                    // to be declared before it is used.
                    let value = self.const_value(path, &c.value, &tc);
                    self.register_annotations(&id, &c.annotations);
                    self.reg.define_local(id, Entry::Const { tc, value });
                }
                other => {
                    if let Some(tc) = self.derive(&p) {
                        self.register_annotations(&id, annotations_of(other));
                        self.reg.define_local(id, Entry::Type(tc));
                    }
                }
            }
        }
    }

    fn register_annotations(&mut self, id: &str, ann: &[orbweaver_idl::lex::Annotation]) {
        if ann.is_empty() {
            return;
        }
        let map: BTreeMap<String, String> =
            ann.iter().map(|a| (a.key.clone(), a.value.clone())).collect();
        self.reg.annotations.insert(id.to_owned(), map);
    }

    /// A constant's value: folded, then coerced to the declared type.
    ///
    /// `None` rather than a guess, in every case [`ConstValue`] lists.
    fn const_value(&self, scope: &[String], e: &ConstExpr, tc: &TypeCode) -> Option<ConstValue> {
        coerce(self.fold(scope, e)?, tc.resolve_alias())
    }

    /// Evaluates a constant expression, with no reference to the declared type.
    fn fold(&self, scope: &[String], e: &ConstExpr) -> Option<ConstValue> {
        Some(match e {
            ConstExpr::Int(v) => ConstValue::Int(*v),
            ConstExpr::Float(v) => ConstValue::Float(*v),
            ConstExpr::Fixed(v) => {
                ConstValue::Fixed { unscaled: i128::try_from(v.unscaled).ok()?, scale: v.scale }
            }
            ConstExpr::Str(s) | ConstExpr::WStr(s) => ConstValue::Str(s.clone()),
            // A character literal folds to its code point; whether that is a
            // `char`, a `wchar` or an `octet` is the declared type's business.
            ConstExpr::Char(c) | ConstExpr::WChar(c) => ConstValue::Int(*c as i128),
            ConstExpr::Bool(b) => ConstValue::Bool(*b),
            ConstExpr::Name(n) => self.fold_name(scope, n)?,
            ConstExpr::Unary { op, operand } => match (*op, self.fold(scope, operand)?) {
                ("+", v) => v,
                ("-", ConstValue::Int(v)) => ConstValue::Int(v.checked_neg()?),
                ("-", ConstValue::Float(v)) => ConstValue::Float(-v),
                ("-", ConstValue::Fixed { unscaled, scale }) => {
                    ConstValue::Fixed { unscaled: unscaled.checked_neg()?, scale }
                }
                ("~", ConstValue::Int(v)) => ConstValue::Int(!v),
                _ => return None,
            },
            ConstExpr::Binary { op, left, right } => {
                match (self.fold(scope, left)?, self.fold(scope, right)?) {
                    // Integer arithmetic stays integer — `7 / 2` is 3 in IDL as
                    // it is in C, and folding it as a float would make
                    // `const long H = 7 / 2;` refuse to coerce back.
                    (ConstValue::Int(a), ConstValue::Int(b)) => ConstValue::Int(int_op(op, a, b)?),
                    // Decimal arithmetic stays decimal, for the same reason
                    // integer arithmetic stays integer and a stronger one:
                    // routing `99999.99d - 0.01d` through `f64` would give a
                    // value that is not 99999.98, and `coerce` would then have
                    // to decide whether to believe it. `corpus/golden/30`
                    // writes exactly that expression.
                    (
                        ConstValue::Fixed { unscaled: a, scale: sa },
                        ConstValue::Fixed { unscaled: b, scale: sb },
                    ) => fixed_op(op, a, sa, b, sb)?,
                    (a, b) => ConstValue::Float(float_op(op, as_f64(&a)?, as_f64(&b)?)?),
                }
            }
        })
    }

    /// A name in a constant expression: another constant, or an enumerator.
    fn fold_name(&self, scope: &[String], n: &ScopedName) -> Option<ConstValue> {
        let path = self.names.resolve(scope, n)?;
        // A constant already registered — source order is what makes this
        // enough, and IDL's declare-before-use rule is what makes source order
        // enough. A forward reference is illegal IDL and folds to `None`.
        if let Some(Entry::Const { value, .. }) = self.reg.entries.get(&self.id_for(&path)) {
            return value.clone();
        }
        let (enum_path, member, ordinal) =
            self.names.enumerators.get(&qualified(&path).to_lowercase())?;
        Some(ConstValue::Enum {
            id: self.id_for(enum_path),
            member: member.clone(),
            ordinal: *ordinal,
        })
    }

    fn interface_entry(&mut self, path: &[String], i: &Interface) -> InterfaceEntry {
        let scope = &path[..path.len() - 1];
        // A base that does not resolve is recorded, not dropped. Dropping it is
        // what turned "this interface inherits something I cannot find" into
        // "this interface has no ancestry" — see [`Unresolved`].
        let bases = i
            .bases
            .iter()
            .filter_map(|b| match self.names.resolve(scope, b) {
                Some(p) => Some(self.id_for(&p)),
                None => {
                    self.note_unresolved(&qualified(path), UnresolvedKind::Base, b);
                    None
                }
            })
            .collect();
        let abstract_interface = matches!(i.modifier, Some(InterfaceModifier::Abstract));
        let Some(body) = &i.body else {
            return InterfaceEntry {
                bases,
                forward_only: true,
                abstract_interface,
                ..InterfaceEntry::default()
            };
        };
        let mut operations = BTreeMap::new();
        let mut attributes = BTreeMap::new();
        for m in body {
            match m {
                InterfaceMember::Operation(op) => {
                    let sig = OperationSig {
                        returns: self.type_of(path, &op.returns),
                        params: op
                            .params
                            .iter()
                            .map(|p| ParamSig {
                                name: p.name.text.clone(),
                                direction: match p.direction {
                                    Direction::In => ParamDirection::In,
                                    Direction::Out => ParamDirection::Out,
                                    Direction::InOut => ParamDirection::InOut,
                                },
                                tc: self.type_of(path, &p.ty),
                                annotations: to_map(&p.annotations),
                            })
                            .collect(),
                        raises: op
                            .raises
                            .iter()
                            .filter_map(|r| match self.names.resolve(path, r) {
                                Some(p) => Some(self.id_for(&p)),
                                None => {
                                    let at = format!("{}::{}", qualified(path), op.name.text);
                                    self.note_unresolved(&at, UnresolvedKind::Raises, r);
                                    None
                                }
                            })
                            .collect(),
                        oneway: op.oneway,
                        annotations: to_map(&op.annotations),
                    };
                    operations.insert(op.name.text.clone(), sig);
                }
                InterfaceMember::Attribute(a) => {
                    let tc = self.type_of(path, &a.ty);
                    for n in &a.names {
                        attributes.insert(
                            n.text.clone(),
                            AttributeSig {
                                tc: tc.clone(),
                                readonly: a.readonly,
                                annotations: to_map(&a.annotations),
                            },
                        );
                    }
                }
                InterfaceMember::Nested(_) => {}
            }
        }
        InterfaceEntry { bases, operations, attributes, forward_only: false, abstract_interface }
    }

    /// Derives the `TypeCode` of the definition at `path`.
    fn derive(&mut self, path: &[String]) -> Option<TypeCode> {
        let key = qualified(path);
        let id = self.id_for(path);
        // Re-entering a type means recursion; §9.3.5.1 encodes that as an
        // indirection, which we represent by naming the type.
        if self.in_progress.contains(&id) {
            return Some(TypeCode::Recursive(id));
        }
        let def = self.names.defs.get(&key)?.clone();
        let name = path.last()?.clone();
        let scope = &path[..path.len() - 1];
        self.in_progress.push(id.clone());

        let tc = match def {
            DefRef::Struct(s) => TypeCode::Struct {
                id: id.clone(),
                name,
                members: self.members(path, s.members.as_deref().unwrap_or(&[])),
            },
            DefRef::Exception(s) => TypeCode::Except {
                id: id.clone(),
                name,
                members: self.members(path, s.members.as_deref().unwrap_or(&[])),
            },
            DefRef::Enum(e) => TypeCode::Enum {
                id: id.clone(),
                name,
                members: e.members.iter().map(|m| m.text.clone()).collect(),
            },
            DefRef::Typedef(t) => {
                let mut inner = self.type_of(scope, &t.ty);
                // Array dimensions read outermost-first, so they are applied in
                // reverse to nest correctly.
                for d in t.dimensions.iter().rev() {
                    inner = TypeCode::Array {
                        element: Box::new(inner),
                        length: const_u32(d).unwrap_or(0),
                    };
                }
                TypeCode::Alias { id: id.clone(), name, aliased: Box::new(inner) }
            }
            DefRef::Union(u) => {
                let disc = self.type_of(scope, &u.discriminator);
                // One case per label, the `default:` a case of its own — the
                // member list omniidl and JacORB derive from the same IDL
                // (CORBA 3.4 Part 2 Table 9.2: `default_index` names one member
                // of the list), in source order: `case 2: default: string
                // rest;` is `(2, rest)` then the default `rest`; `default:
                // case 5: case 6: short misc;` is the default first. Until
                // 2026-08-19 a labelled default was ONE case here, carrying its
                // label with `default_index` pointing at it — semantically the
                // same union, and both peers selected identically from it, but
                // a different `member_count` and `default_index` on the wire,
                // a `TypeCode` no peer's IDL-derived one equals, and IDL
                // regenerated from our own decoded TypeCode lost the `case 2:`.
                //
                // The default case is the one selected by *not* matching, so
                // it has no label: an empty one, and `default_index` names it.
                // That is the in-memory convention every consumer reads (the
                // generators fold by member name and read the flag off the
                // index; the dynamic invoker matches labels first and falls
                // back to the index; the property sampler searches for a value
                // no label claims). The wire has its own — a slot of the
                // discriminator's width, value ignored (§9.3.5.1.4) — and the
                // TypeCode codec in `orbweaver_giop` translates between the
                // two: zeros out, dropped on the way in.
                let mut cases = Vec::new();
                let mut default_index = -1i32;
                for c in &u.cases {
                    let tc = self.type_of(path, &c.member.ty);
                    let member_name =
                        c.member.names.first().map(|n| n.text.clone()).unwrap_or_default();
                    let mut labels: Vec<Vec<u8>> =
                        c.labels.iter().map(|l| label_bytes(l, &disc)).collect();
                    if let Some(at) = c.default_at {
                        // Indexed against the *expanded* list, which is what
                        // goes on the wire and what every consumer selects
                        // from: a multi-label branch before the default shifts
                        // it, and computing the position over the AST list
                        // once pointed it at the wrong case.
                        let at = at.min(labels.len());
                        default_index = (cases.len() + at) as i32;
                        labels.insert(at, Vec::new());
                    }
                    cases.extend(labels.into_iter().map(|label| TcUnionCase {
                        label,
                        name: member_name.clone(),
                        tc: tc.clone(),
                    }));
                }
                TypeCode::Union {
                    id: id.clone(),
                    name,
                    discriminator: Box::new(disc),
                    default_index,
                    cases,
                }
            }
            DefRef::Interface { is_abstract: false } => TypeCode::ObjRef { id: id.clone(), name },
            DefRef::Interface { is_abstract: true } => {
                TypeCode::AbstractInterface { id: id.clone(), name }
            }
            DefRef::ValueType(v) => {
                // Described, not marshalled. §4.4 defers the *value's* wire
                // form; this is the type's description, and the two are not
                // the same claim — see [`TypeCode::Value`] for why saying
                // `ObjRef` here was a wrong answer rather than a deferred one.
                //
                // The base is resolved through the same name table every other
                // reference uses, so a base declared later in the file is
                // found; a base that does not resolve is recorded as
                // unresolved and dropped from the TypeCode rather than
                // invented, which is the rule [`Unresolved`] states.
                let base = v.base.as_ref().and_then(|b| match self.names.resolve(scope, b) {
                    Some(p) => self.derive(&p).map(Box::new),
                    None => {
                        self.note_unresolved(&qualified(path), UnresolvedKind::Base, b);
                        None
                    }
                });
                // VM_ABSTRACT is 2; VM_NONE is 0. `custom` and `truncatable`
                // are not in the front end's AST, so they are not invented
                // here — a modifier we cannot see is VM_NONE, and a contract
                // using one would be reported by the front end first.
                let modifier = if v.is_abstract { 2 } else { 0 };
                let members = v
                    .members
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .flat_map(|m| match m {
                        ValueMember::State { public, member } => {
                            let tc = self.type_of(path, &member.ty);
                            member
                                .names
                                .iter()
                                .map(|n| TcValueMember {
                                    name: n.text.clone(),
                                    tc: tc.clone(),
                                    // PUBLIC_MEMBER 1, PRIVATE_MEMBER 0.
                                    visibility: i16::from(*public),
                                })
                                .collect::<Vec<_>>()
                        }
                        // Operations, attributes and nested definitions are not
                        // state and do not appear in a tk_value's member list.
                        _ => Vec::new(),
                    })
                    .collect();
                TypeCode::Value { id: id.clone(), name, modifier, base, members }
            }
            // Described, not marshalled — and unlike a `valuetype` there is
            // nothing to marshal at any wire version. See [`DefRef::Native`].
            DefRef::Native => TypeCode::Native { id: id.clone(), name },
        };
        self.in_progress.pop();
        Some(tc)
    }

    fn members(&mut self, path: &[String], members: &[Member]) -> Vec<TcMember> {
        members
            .iter()
            .flat_map(|m| {
                let tc = self.type_of(path, &m.ty);
                m.names
                    .iter()
                    .map(|n| TcMember { name: n.text.clone(), tc: tc.clone() })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn type_of(&mut self, scope: &[String], t: &TypeSpec) -> TypeCode {
        match t {
            TypeSpec::Void => TypeCode::Void,
            TypeSpec::Boolean => TypeCode::Boolean,
            TypeSpec::Char => TypeCode::Char,
            TypeSpec::WChar => TypeCode::WChar,
            TypeSpec::Octet => TypeCode::Octet,
            TypeSpec::Short => TypeCode::Short,
            TypeSpec::UShort => TypeCode::UShort,
            TypeSpec::Long => TypeCode::Long,
            TypeSpec::ULong => TypeCode::ULong,
            TypeSpec::LongLong => TypeCode::LongLong,
            TypeSpec::ULongLong => TypeCode::ULongLong,
            TypeSpec::Float => TypeCode::Float,
            TypeSpec::Double => TypeCode::Double,
            TypeSpec::LongDouble => TypeCode::LongDouble,
            TypeSpec::Any => TypeCode::Any,
            TypeSpec::Object => TypeCode::ObjRef {
                id: "IDL:omg.org/CORBA/Object:1.0".into(),
                name: "Object".into(),
            },
            // `ValueBase` is a **valuetype**, and this said object reference —
            // the same wrong answer `valuetype` and `native` carried, in the
            // one spelling that has no declaration to hang a fix on. It was
            // the last one left: S4's closure already named it
            // (`sema.rs`, `TypeSpec::ValueBase`), and because the registry
            // handed the generators an `ObjRef` they skipped nothing, so
            // `struct Anything { ValueBase v; }` generated as a reference and
            // put an IOR on the wire where a peer sends a value.
            //
            // Measured rather than reasoned (`spikes/native_capture.py`,
            // omniORB 4.3.4, 2026-08-21): for `struct Holder { ValueBase
            // slot; };` omniORB writes the member as TCKind **29**, seventy-two
            // bytes, `IDL:omg.org/CORBA/ValueBase:1.0` / `ValueBase`,
            // ValueModifier `00 00` — **VM_NONE, not VM_ABSTRACT**, which is
            // the one field a reasoned answer would have got wrong — concrete
            // base `tk_null`, and zero state members. This constructs exactly
            // that, so our TypeCode for a `ValueBase` member is byte-for-byte
            // omniORB's.
            TypeSpec::ValueBase => TypeCode::Value {
                id: "IDL:omg.org/CORBA/ValueBase:1.0".into(),
                name: "ValueBase".into(),
                modifier: 0,
                base: None,
                members: Vec::new(),
            },
            TypeSpec::String(b) => TypeCode::String(b.as_deref().and_then(const_u32).unwrap_or(0)),
            TypeSpec::WString(b) => {
                TypeCode::WString(b.as_deref().and_then(const_u32).unwrap_or(0))
            }
            // A declaration writes `fixed<d,s>`; a constant writes bare `fixed`
            // and CORBA 3.4 §7.4.1.4.2 takes its digits and scale from the
            // *value*. `0, 0` is this function's existing marker for a bound it
            // could not evaluate (the `unwrap_or(0)` beside it), and it is what
            // a bare `fixed` gets: this function is handed a `TypeSpec` and the
            // value is folded elsewhere, by `const_value`, so the pair is not
            // computable *here* — recorded rather than invented.
            //
            // Every other reason this comment gave was true when it was written
            // and false by 2026-08-25, which is why it is worth writing down
            // what it said. It said the decimal was unrecoverable — "the lexer
            // folds `9.9d` to `Tok::Float(9.9)`" — and 9a27659 gave the lexer a
            // `Tok::Fixed(FixedLit)` and the AST a `ConstExpr::Fixed`, so the
            // digits survive intact. It said `coerce` had no arm for a fixed so
            // the entry stored no value; `coerce` has had one since the same
            // commit and the entry stores the exact decimal. It said both
            // emitters skip a valueless constant; both now skip it *with the
            // value in the refusal* — `orbweaver-gen` quotes `as_decimal()` and
            // says the value is exact in the registry. And it said no fixed
            // TypeCode is marshalled, which confuses the two claims the
            // `valuetype` arm above is careful to separate: §4.4 defers the
            // *value's* wire form, not the type's description, and
            // `orbweaver_giop::typecode` has always encoded and decoded a
            // `tk_fixed`'s digits and scale.
            //
            // So the pair is no longer unread by construction. Three layers
            // render it — `orbweaver_dynamic::type_name` as `fixed<d,s>`,
            // `anyjson` as two numbers, `orbweaver_gen::deferred_fixed` into a
            // refusal a person reads. Whether a *constant's* `0, 0` reaches any
            // of them is not something this crate can assert from here, and it
            // is not asserted: the const arms of both emitters take the value
            // and ignore this TypeCode. Folding the pair out of the value is a
            // change with a peer to measure against, not a comment.
            //
            // Nothing was red for any of it. A comment is a sentence about what
            // another function does, and no compiler reads one.
            TypeSpec::Fixed { bounds } => TypeCode::Fixed {
                digits: bounds.as_ref().and_then(|(d, _)| const_u32(d)).unwrap_or(0) as u16,
                scale: bounds.as_ref().and_then(|(_, s)| const_u32(s)).unwrap_or(0) as i16,
            },
            TypeSpec::Sequence { element, bound } => TypeCode::Sequence {
                element: Box::new(self.type_of(scope, element)),
                bound: bound.as_deref().and_then(const_u32).unwrap_or(0),
            },
            TypeSpec::Named(n) => match self.names.resolve(scope, n) {
                // `::CORBA::TypeCode` resolves to the predeclared native the
                // front end provides, and a native has no derivation — so it
                // used to fall through to `void`, silently. That is not a
                // cosmetic default: `describe_interface`, the operation the
                // Interface Repository facade exists for, returns a TypeCode,
                // and an operation whose return type quietly became `void`
                // marshals nothing where a peer expects a TypeCode.
                //
                // CLAUDE.md *requires* this spelling, which is what made the
                // gap invisible: the rule reads as support. Found by the
                // generated-servant batch, which could not express
                // `describe_interface` and said so rather than emitting an
                // empty reply.
                Some(p) => self.derive(&p).unwrap_or(TypeCode::Void),
                // The names module `CORBA` predeclares are *predeclared by the
                // front end* for checking and are not among the spec's
                // definitions, so they resolve to nothing and land here.
                None => match predeclared_corba(n) {
                    Some(tc) => tc,
                    // An unresolved name is a semantic error the checker
                    // already reports; producing `void` here keeps the registry
                    // loadable for tooling that wants to show what *did*
                    // resolve — but it is recorded, for the same reason a base
                    // or a `raises` is. `void` where a type belongs marshals
                    // nothing, and a gate reading a graph with a member typed
                    // `void` by accident is deciding from evidence it does not
                    // have.
                    None => {
                        self.note_unresolved(&qualified(scope), UnresolvedKind::Type, n);
                        TypeCode::Void
                    }
                },
            },
        }
    }
}

/// The `TypeCode` for a name the front end predeclares inside module `CORBA`,
/// or `None` if `n` is not one of them.
///
/// These names have no `Definition` behind them, so `derive` has nothing to
/// work from and the caller's only other answer is `TypeCode::Void` — a member
/// that marshals **zero bytes** where the author wrote a type. This function is
/// therefore the whole promise the front end makes when it admits a name:
/// `orbweaver_idl::sema::PREDECLARED_CORBA` is the table, and
/// `every_predeclared_corba_name_reaches_a_typecode_or_is_refused` iterates it
/// against this one so a row added there without a row here goes red.
///
/// It handled `TypeCode` alone until 2026-08-25, when the sweep that rule
/// demanded found `Principal` — predeclared on the very next line of that table
/// — landing on the `void` this function exists to prevent, in the exact shape
/// its own comment describes as "not a cosmetic default". One row of a table
/// had been repaired and its neighbour left, which is what a fix scoped to a
/// keyword rather than to a rule buys.
///
/// `Principal` is answered rather than refused because **the oracle accepts
/// the contract**: `omniidl -b dump` on `corpus/golden/34-corba-principal.idl`
/// prints `CORBA::Principal who;` and exits 0, so refusing the name in the
/// front end would be a divergence from a conformant compiler over a file it
/// parses. The type is withdrawn from CORBA (it left the GIOP request header
/// after 1.0 and CORBA 3.x removed it) and this version marshals no value for
/// it — which is a *wire* answer, and every marshalling layer already gives it
/// by name: `orbweaver_dynamic::dynany` refuses a `TypeCode::Principal`,
/// `orbweaver_test::prop` says it is withdrawn and not marshalled, and both
/// emitters fall to their catch-all and skip the declaration. None of those
/// arms was reachable from a contract until this line existed.
///
/// Matched on the full name rather than on a trailing identifier, so a user's
/// own `TypeCode` or `Principal` declared in their own module keeps whatever
/// they declared it to be — that one resolves, and never reaches this arm.
///
/// `Object` and `ValueBase` are in that table and are deliberately absent here:
/// `object` and `valuebase` are IDL keywords, so `::CORBA::Object` is a parse
/// error and no `TypeSpec::Named` can carry either. Their keyword spellings are
/// `TypeSpec::Object` and `TypeSpec::ValueBase`, answered above. An arm here
/// would be code no contract can execute; the test covers them through the
/// refusal branch instead, and turns red if the lexer ever lets them through.
fn predeclared_corba(n: &orbweaver_idl::ast::ScopedName) -> Option<TypeCode> {
    match n.parts.as_slice() {
        [m, t] if m == "CORBA" && t == "TypeCode" => Some(TypeCode::TypeCode),
        [m, t] if m == "CORBA" && t == "Principal" => Some(TypeCode::Principal),
        _ => None,
    }
}

fn annotations_of(d: &Definition) -> &[orbweaver_idl::lex::Annotation] {
    match d {
        Definition::Struct(s) | Definition::Exception(s) => &s.annotations,
        Definition::Union(u) => &u.annotations,
        Definition::Enum(e) => &e.annotations,
        Definition::Typedef(t) => &t.annotations,
        Definition::ValueType(v) => &v.annotations,
        Definition::Interface(i) => &i.annotations,
        Definition::Module(m) => &m.annotations,
        Definition::Const(c) => &c.annotations,
        Definition::Native(_) => &[],
    }
}

fn to_map(ann: &[orbweaver_idl::lex::Annotation]) -> BTreeMap<String, String> {
    ann.iter().map(|a| (a.key.clone(), a.value.clone())).collect()
}

/// Integer arithmetic, checked. `None` is "there is no answer", never a wrap:
/// a constant that silently wrapped would be a wrong number with a repository
/// id on it.
fn int_op(op: &str, a: i128, b: i128) -> Option<i128> {
    match op {
        "+" => a.checked_add(b),
        "-" => a.checked_sub(b),
        "*" => a.checked_mul(b),
        "/" => a.checked_div(b),
        "%" => a.checked_rem(b),
        "|" => Some(a | b),
        "&" => Some(a & b),
        "^" => Some(a ^ b),
        "<<" => u32::try_from(b).ok().and_then(|s| a.checked_shl(s)),
        ">>" => u32::try_from(b).ok().and_then(|s| a.checked_shr(s)),
        _ => None,
    }
}

/// Decimal arithmetic: `+`, `-` and `*` on two decimals, **exactly**, or `None`.
///
/// This doc comment opened *"Floating arithmetic. The bitwise operators are
/// integer-only in IDL"* until 2026-08-25, which is [`float_op`]'s sentence and
/// not this function's — 9a27659 inserted `fixed_op` directly above `float_op`
/// and the head stayed where it was, so the decimal folder was documented as
/// the binary one and `float_op` was left with no documentation at all. A
/// comment that describes the function *below* it compiles exactly as well as
/// one that describes the function it is attached to.
///
/// Addition and subtraction line the two scales up first; multiplication adds
/// them. Every step is checked, so an expression whose result needs more than
/// an `i128` folds to nothing rather than to a wrapped number.
///
/// Division is deliberately absent. `1.0d / 3.0d` has no exact decimal, and
/// IDL has no rule saying what scale to round it to — so there is no answer
/// this could give that is not invented. It folds to `None`, which
/// [`ConstValue`] already documents as "an operation that has no answer".
/// *나눗셈은 정확한 십진수가 없으므로 접지 않는다.*
fn fixed_op(op: &str, a: i128, sa: u16, b: i128, sb: u16) -> Option<ConstValue> {
    if op == "*" {
        return Some(ConstValue::Fixed { unscaled: a.checked_mul(b)?, scale: sa.checked_add(sb)? });
    }
    let scale = sa.max(sb);
    let lift = |v: i128, from: u16| {
        let steps = u32::from(scale - from);
        v.checked_mul(10i128.checked_pow(steps)?)
    };
    let (a, b) = (lift(a, sa)?, lift(b, sb)?);
    let unscaled = match op {
        "+" => a.checked_add(b)?,
        "-" => a.checked_sub(b)?,
        _ => return None,
    };
    Some(ConstValue::Fixed { unscaled, scale })
}

/// Floating arithmetic. The bitwise operators are integer-only in IDL, so they
/// have no float arm at all rather than a coerced one — and unlike [`fixed_op`]
/// this one *does* divide, because a binary float has an answer for every pair
/// and the question of what to round to does not arise.
fn float_op(op: &str, a: f64, b: f64) -> Option<f64> {
    let v = match op {
        "+" => a + b,
        "-" => a - b,
        "*" => a * b,
        "/" => a / b,
        _ => return None,
    };
    v.is_finite().then_some(v)
}

fn as_f64(v: &ConstValue) -> Option<f64> {
    match v {
        ConstValue::Int(i) => Some(*i as f64),
        ConstValue::Float(f) => Some(*f),
        _ => None,
    }
}

/// Fits a folded value to its declared type, or refuses.
///
/// `tc` is already alias-resolved. Refusing is the point: `const octet O = 300`
/// is an IDL error, and a registry that stored 44 would hand every consumer a
/// number no author wrote.
fn coerce(v: ConstValue, tc: &TypeCode) -> Option<ConstValue> {
    let in_range =
        |i: i128, lo: i128, hi: i128| (lo..=hi).contains(&i).then_some(ConstValue::Int(i));
    match (tc, &v) {
        (TypeCode::Boolean, ConstValue::Bool(_)) => Some(v),
        // A `fixed` constant keeps its decimal. The declared bounds are not
        // checked against it here: `fixed_pt_const_type` is the bare keyword,
        // so a constant's digits and scale come *from the value* and there is
        // nothing to disagree with. A `typedef fixed<d,s>` used as a
        // constant's type is the one shape that has both, and the oracle
        // accepts it without complaint about either.
        (TypeCode::Fixed { .. }, ConstValue::Fixed { .. }) => Some(v),
        (TypeCode::Octet | TypeCode::Char, ConstValue::Int(i)) => in_range(*i, 0, 0xFF),
        // A `wchar` is a code point, so the range is Unicode's and the
        // surrogate half is excluded — `char::from_u32` is the authority, and
        // it is the same call the generator has to make to spell one.
        (TypeCode::WChar, ConstValue::Int(i)) => {
            u32::try_from(*i).ok().and_then(char::from_u32).map(|c| ConstValue::Int(c as i128))
        }
        (TypeCode::Short, ConstValue::Int(i)) => in_range(*i, i16::MIN.into(), i16::MAX.into()),
        (TypeCode::UShort, ConstValue::Int(i)) => in_range(*i, 0, u16::MAX.into()),
        (TypeCode::Long, ConstValue::Int(i)) => in_range(*i, i32::MIN.into(), i32::MAX.into()),
        (TypeCode::ULong, ConstValue::Int(i)) => in_range(*i, 0, u32::MAX.into()),
        (TypeCode::LongLong, ConstValue::Int(i)) => in_range(*i, i64::MIN.into(), i64::MAX.into()),
        // Both bounds, now that there is a type that can hold the upper one.
        // This arm used to check only `>= 0` and explain that the AST's `i64`
        // made the upper half unreachable — which was true, and the reason it
        // was true was a defect one layer up: the lexer refused
        // `18446744073709551615` outright. The literal is legal, so the check
        // is now a check.
        (TypeCode::ULongLong, ConstValue::Int(i)) => in_range(*i, 0, u64::MAX.into()),
        (TypeCode::Float, ConstValue::Int(_) | ConstValue::Float(_)) => {
            let f = as_f64(&v)?;
            ((f as f32).is_finite()).then_some(ConstValue::Float(f))
        }
        (TypeCode::Double, ConstValue::Int(_) | ConstValue::Float(_)) => {
            let f = as_f64(&v)?;
            f.is_finite().then_some(ConstValue::Float(f))
        }
        (TypeCode::String(_) | TypeCode::WString(_), ConstValue::Str(_)) => Some(v),
        // An enumerator only fits the enum that declares it.
        (TypeCode::Enum { id, .. }, ConstValue::Enum { id: from, .. }) if id == from => Some(v),
        _ => None,
    }
}

/// Evaluates the constant forms that appear as bounds and dimensions.
fn const_u32(e: &ConstExpr) -> Option<u32> {
    match e {
        ConstExpr::Int(v) if *v >= 0 => u32::try_from(*v).ok(),
        ConstExpr::Unary { op: "+", operand } => const_u32(operand),
        ConstExpr::Binary { op, left, right } => {
            let (l, r) = (const_u32(left)?, const_u32(right)?);
            Some(match *op {
                "+" => l.checked_add(r)?,
                "-" => l.checked_sub(r)?,
                "*" => l.checked_mul(r)?,
                "/" => l.checked_div(r)?,
                _ => return None,
            })
        }
        _ => None,
    }
}

/// Encodes a union case label in the discriminator's wire width.
///
/// The width follows the discriminator type, not the label's own value: a
/// boolean label is one octet and a long label is four, and using the wrong
/// one shifts every case that follows.
fn label_bytes(e: &ConstExpr, disc: &TypeCode) -> Vec<u8> {
    // `i128` follows the AST, so that an `unsigned long long` label above
    // `i64::MAX` — `case 18446744073709551615:`, which omniidl accepts and
    // which the lexer used to refuse — reaches the width cast below with its
    // value intact rather than saturating to zero on the way in.
    let v: i128 = match e {
        ConstExpr::Int(v) => *v,
        ConstExpr::Bool(b) => i128::from(*b),
        ConstExpr::Char(c) | ConstExpr::WChar(c) => *c as i128,
        ConstExpr::Unary { op: "-", operand } => match operand.as_ref() {
            ConstExpr::Int(v) => -*v,
            _ => 0,
        },
        // An enumerator label is its ordinal, which the discriminator's own
        // TypeCode carries.
        ConstExpr::Name(n) => match disc.resolve_alias() {
            TypeCode::Enum { members, .. } => {
                members.iter().position(|m| m == n.last()).unwrap_or(0) as i128
            }
            _ => 0,
        },
        _ => 0,
    };
    match disc.resolve_alias() {
        TypeCode::Boolean | TypeCode::Char | TypeCode::Octet => vec![v as u8],
        TypeCode::Short | TypeCode::UShort => (v as i16).to_be_bytes().to_vec(),
        TypeCode::LongLong | TypeCode::ULongLong => (v as i64).to_be_bytes().to_vec(),
        _ => (v as i32).to_be_bytes().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `::CORBA::TypeCode` is a TypeCode, not `void`.
    ///
    /// It used to be `void`, because the front end predeclares the name for
    /// checking and the registry resolves against the spec's own definitions,
    /// where it is absent — so it fell through the unresolved arm. Nothing
    /// noticed for months, and CLAUDE.md *requires* this spelling, which is
    /// what made the gap read as support. An operation returning a TypeCode
    /// marshalled nothing at a peer expecting one.
    #[test]
    fn corba_typecode_is_not_void() {
        let spec =
            orbweaver_idl::parse("module m { interface I { ::CORBA::TypeCode describe(); }; };")
                .expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        let Some(Entry::Interface(i)) = r.get("IDL:m/I:1.0") else { panic!("no interface") };
        assert_eq!(i.operations["describe"].returns, TypeCode::TypeCode);
    }

    /// And a user's own `TypeCode`, declared in their own module, keeps what
    /// they declared — the rule matches the full predeclared name, not a
    /// trailing identifier.
    #[test]
    fn a_users_own_typecode_is_still_their_own() {
        let spec = orbweaver_idl::parse(
            "module m { typedef long TypeCode; interface I { TypeCode describe(); }; };",
        )
        .expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        let Some(Entry::Interface(i)) = r.get("IDL:m/I:1.0") else { panic!("no interface") };
        assert!(
            matches!(i.operations["describe"].returns.resolve_alias(), TypeCode::Long),
            "{:?}",
            i.operations["describe"].returns
        );
    }

    /// The rule the `TypeCode` fix above was one row of: **every name the front
    /// end predeclares inside module `CORBA` either reaches a `TypeCode` this
    /// registry means, or is refused.** Never `Void`.
    ///
    /// `corba_typecode_is_not_void` pinned one entry of
    /// [`orbweaver_idl::sema::PREDECLARED_CORBA`] and left its three
    /// neighbours unmeasured, and one of them — `Principal`, the next line of
    /// the same table — was landing on exactly the `void` that test's own
    /// docstring calls out. A member typed `void` marshals zero bytes, so a
    /// peer writing a Principal handed us nothing and we mis-parsed everything
    /// after it; both emitters produced it (`("who", "who", "void")` in Python,
    /// `pub who: ()` in Rust) and no gate was red.
    ///
    /// So the test iterates the table rather than naming an entry, and a row
    /// added there without a row in [`predeclared_corba`] fails here. Two of
    /// the four cannot be spelled as a scoped name at all — `object` and
    /// `valuebase` are [`orbweaver_idl::lex::KEYWORDS`] — and a refusal is an
    /// acceptable answer under the rule, so the assertion is on the *pair*:
    /// parsed implies not-`Void`. The day the lexer admits those spellings this
    /// goes red instead of the wire going quiet.
    ///
    /// *한 줄만 고정한 테스트는 그 옆줄을 재지 않는다. 그래서 표를 순회한다.*
    #[test]
    fn every_predeclared_corba_name_reaches_a_typecode_or_is_refused() {
        let mut verdicts = Vec::new();
        for (name, _) in orbweaver_idl::sema::PREDECLARED_CORBA {
            let src = format!("module m {{ struct Holder {{ ::CORBA::{name} slot; }}; }};");
            let Ok(spec) = orbweaver_idl::check(&src) else {
                verdicts.push((*name, "refused by the front end".to_owned()));
                continue;
            };
            let mut r = Registry::new();
            r.load(&spec).expect("loads");
            let Some(Entry::Type(TypeCode::Struct { members, .. })) = r.get("IDL:m/Holder:1.0")
            else {
                panic!("{name}: no struct");
            };
            let tc = &members[0].tc;
            assert_ne!(
                *tc,
                TypeCode::Void,
                "::CORBA::{name} is admitted by the front end and the registry called it \
                 `void`, so a member of that type marshals zero bytes — give it an arm in \
                 `predeclared_corba`, or refuse the name"
            );
            verdicts.push((*name, format!("{tc:?}")));
        }
        // The verdicts themselves, so the sweep's result is checked rather than
        // described. Two answers and two refusals, and which is which is the
        // part a reader of the table cannot work out from the table.
        assert_eq!(
            verdicts,
            [
                ("TypeCode", "TypeCode".to_owned()),
                ("Object", "refused by the front end".to_owned()),
                ("ValueBase", "refused by the front end".to_owned()),
                ("Principal", "Principal".to_owned()),
            ]
        );
    }

    /// And the two that are refused as scoped names are carried by their
    /// keyword spellings instead, so the refusal above costs a contract
    /// nothing — the negative control for the branch this rule accepts.
    #[test]
    fn the_keyword_spellings_of_the_refused_two_are_not_void() {
        let r = load("module m { struct Held { Object o; }; struct Kept { ValueBase v; }; };");
        let Some(Entry::Type(TypeCode::Struct { members: held, .. })) = r.get("IDL:m/Held:1.0")
        else {
            panic!("no Held")
        };
        let Some(Entry::Type(TypeCode::Struct { members: kept, .. })) = r.get("IDL:m/Kept:1.0")
        else {
            panic!("no Kept")
        };
        assert!(matches!(held[0].tc, TypeCode::ObjRef { .. }), "{:?}", held[0].tc);
        assert!(matches!(kept[0].tc, TypeCode::Value { .. }), "{:?}", kept[0].tc);
    }

    /// A user's own `Principal`, like a user's own `TypeCode`, keeps what they
    /// declared: [`predeclared_corba`] matches the full name.
    #[test]
    fn a_users_own_principal_is_still_their_own() {
        let r = load("module m { typedef string Principal; struct S { Principal who; }; };");
        let Some(Entry::Type(TypeCode::Struct { members, .. })) = r.get("IDL:m/S:1.0") else {
            panic!("no struct")
        };
        assert!(
            matches!(members[0].tc.resolve_alias(), TypeCode::String(0)),
            "{:?}",
            members[0].tc
        );
    }

    fn load(src: &str) -> Registry {
        let spec = orbweaver_idl::parse(src).expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        r
    }

    #[test]
    fn repository_ids_match_what_peers_publish() {
        let r = load("module spike { interface Echo { long ping(); }; };");
        assert_eq!(r.id_of("spike::Echo").unwrap(), "IDL:spike/Echo:1.0");
        assert!(r.interface("IDL:spike/Echo:1.0").is_some());
    }

    #[test]
    fn nested_modules_nest_in_the_id() {
        let r = load("module a { module b { struct C { long x; }; }; };");
        assert_eq!(r.id_of("a::b::C").unwrap(), "IDL:a/b/C:1.0");
    }

    #[test]
    fn struct_typecodes_carry_members_in_order() {
        let r =
            load("module m { struct Ragged { octet a; long b; short c; double d; octet e; }; };");
        match r.typecode("IDL:m/Ragged:1.0").unwrap() {
            TypeCode::Struct { id, name, members } => {
                assert_eq!(id, "IDL:m/Ragged:1.0");
                assert_eq!(name, "Ragged");
                let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
                assert_eq!(names, ["a", "b", "c", "d", "e"], "order is the wire order");
                assert_eq!(members[3].tc, TypeCode::Double);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_declaration_naming_several_members_expands() {
        let r = load("module m { struct S { long a, b, c; }; };");
        match r.typecode("IDL:m/S:1.0").unwrap() {
            TypeCode::Struct { members, .. } => assert_eq!(members.len(), 3),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn typedefs_become_aliases_and_arrays_nest_outermost_first() {
        let r = load("module m { typedef long Matrix[3][4]; };");
        match r.typecode("IDL:m/Matrix:1.0").unwrap() {
            TypeCode::Alias { aliased, .. } => match aliased.as_ref() {
                TypeCode::Array { element, length } => {
                    assert_eq!(*length, 3, "the first dimension is the outer one");
                    match element.as_ref() {
                        TypeCode::Array { element, length } => {
                            assert_eq!(*length, 4);
                            assert_eq!(**element, TypeCode::Long);
                        }
                        other => panic!("{other:?}"),
                    }
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bounded_strings_and_sequences_keep_their_bounds() {
        let r = load(
            "module m { typedef string<16> Name; typedef sequence<long, 4> Four; \
             typedef sequence<long> Any_; };",
        );
        let unalias = |id: &str| match r.typecode(id).unwrap() {
            TypeCode::Alias { aliased, .. } => (**aliased).clone(),
            other => other.clone(),
        };
        assert_eq!(unalias("IDL:m/Name:1.0"), TypeCode::String(16));
        assert_eq!(
            unalias("IDL:m/Four:1.0"),
            TypeCode::Sequence { element: Box::new(TypeCode::Long), bound: 4 }
        );
        assert_eq!(
            unalias("IDL:m/Any_:1.0"),
            TypeCode::Sequence { element: Box::new(TypeCode::Long), bound: 0 },
            "an absent bound is zero, not one"
        );
    }

    /// A union label's width follows the discriminator, not the label value.
    #[test]
    fn union_labels_use_the_discriminator_width() {
        let r = load(
            "module m { union B switch (boolean) { case TRUE: long y; case FALSE: octet n; }; };",
        );
        match r.typecode("IDL:m/B:1.0").unwrap() {
            TypeCode::Union { cases, .. } => {
                assert_eq!(cases[0].label, vec![1u8], "a boolean label is one octet");
                assert_eq!(cases[1].label, vec![0u8]);
            }
            other => panic!("{other:?}"),
        }
        let r = load("module m { union L switch (long) { case 7: long v; }; };");
        match r.typecode("IDL:m/L:1.0").unwrap() {
            TypeCode::Union { cases, .. } => assert_eq!(cases[0].label, 7i32.to_be_bytes()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn enum_labels_resolve_to_their_ordinal() {
        let r = load("module m { enum K { A, B, C }; union U switch (K) { case B: long v; }; };");
        match r.typecode("IDL:m/U:1.0").unwrap() {
            TypeCode::Union { cases, .. } => assert_eq!(cases[0].label, 1i32.to_be_bytes()),
            other => panic!("{other:?}"),
        }
    }

    /// A multi-label branch before the default used to shift the default
    /// index: it was computed against the AST case list while the cases were
    /// expanded, so the default pointed at the wrong branch. The dynamic
    /// invoker selects default branches from this index, so it inherited the
    /// error too.
    #[test]
    fn the_default_index_survives_multi_label_expansion() {
        let r = load(
            "module m { union U switch (long) { case 1: long one; case 2: case 3: string s; \
             default: boolean b; }; };",
        );
        let Some(TypeCode::Union { cases, default_index, .. }) = r.typecode("IDL:m/U:1.0") else {
            panic!("not a union");
        };
        assert_eq!(cases.len(), 4, "1, 2, 3, default");
        assert_eq!(default_index, &3, "the default is the FOURTH expanded case");
        assert_eq!(cases[3].name, "b");
    }

    /// A branch with several labels becomes several cases sharing one member.
    #[test]
    fn multiple_labels_on_one_branch_expand() {
        let r = load(
            "module m { union U switch (long) { case 2: case 3: string both; default: boolean o; }; };",
        );
        match r.typecode("IDL:m/U:1.0").unwrap() {
            TypeCode::Union { cases, default_index, .. } => {
                assert_eq!(cases.len(), 3, "two labels plus the default");
                assert_eq!(cases[0].name, "both");
                assert_eq!(cases[1].name, "both");
                // This assertion used to read `default_index == 1, "index of
                // the default *branch*"` — which pinned the bug. The TypeCode
                // field indexes the expanded case list that goes on the wire
                // (and that the dynamic invoker selects from), not the source
                // branches; a test asserting the wrong semantics is how a wrong
                // implementation survives its own test suite.
                assert_eq!(*default_index, 2, "index into the EXPANDED cases");
                assert_eq!(cases[2].name, "o");
            }
            other => panic!("{other:?}"),
        }
    }

    /// `corpus/golden/15-forward-recursive.idl` in registry form: without
    /// recursion detection this does not terminate.
    #[test]
    fn recursive_types_terminate() {
        let r = load(
            "module m { struct Tree; typedef sequence<Tree> Kids; \
             struct Tree { string label; Kids kids; }; };",
        );
        match r.typecode("IDL:m/Tree:1.0").unwrap() {
            TypeCode::Struct { members, .. } => match &members[1].tc {
                TypeCode::Alias { aliased, .. } => match aliased.as_ref() {
                    TypeCode::Sequence { element, .. } => {
                        assert_eq!(**element, TypeCode::Recursive("IDL:m/Tree:1.0".into()));
                    }
                    other => panic!("{other:?}"),
                },
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    // ── inheritance ─────────────────────────────────────────────────────────

    #[test]
    fn is_a_walks_the_inheritance_graph() {
        let r = load(
            "module m { interface A { long f(); }; interface B : A { long g(); }; \
             interface C : B { long h(); }; interface Z { long q(); }; };",
        );
        assert!(r.is_a("IDL:m/C:1.0", "IDL:m/A:1.0"), "transitively");
        assert!(r.is_a("IDL:m/C:1.0", "IDL:m/C:1.0"), "reflexively");
        assert!(!r.is_a("IDL:m/A:1.0", "IDL:m/C:1.0"), "not upwards");
        assert!(!r.is_a("IDL:m/C:1.0", "IDL:m/Z:1.0"));
        assert!(r.is_a("IDL:m/C:1.0", "IDL:omg.org/CORBA/Object:1.0"), "everything is an Object");
    }

    #[test]
    fn multiple_inheritance_is_followed() {
        let r = load(
            "module m { interface A { long f(); }; interface B { long g(); }; \
             interface D : A, B { long h(); }; };",
        );
        assert!(r.is_a("IDL:m/D:1.0", "IDL:m/A:1.0"));
        assert!(r.is_a("IDL:m/D:1.0", "IDL:m/B:1.0"));
        let mut anc = r.ancestors("IDL:m/D:1.0");
        anc.sort();
        assert_eq!(anc, ["IDL:m/A:1.0", "IDL:m/B:1.0"]);
    }

    /// Inherited operations are callable, so lookup must not stop at the
    /// declaring interface.
    #[test]
    fn operations_resolve_through_inheritance() {
        let r = load("module m { interface A { long f(); }; interface B : A { long g(); }; };");
        let (owner, sig) = r.resolve_operation("IDL:m/B:1.0", "f").expect("inherited");
        assert_eq!(owner, "IDL:m/A:1.0");
        assert_eq!(sig.returns, TypeCode::Long);
        assert!(r.resolve_operation("IDL:m/B:1.0", "nope").is_none());
    }

    #[test]
    fn cyclic_inheritance_does_not_hang() {
        // Illegal IDL, which the checker rejects — but a registry loaded from
        // unchecked input must still terminate.
        let r = load(
            "module m { interface A; interface B : A { long g(); }; interface A : B { long f(); }; };",
        );
        let _ = r.is_a("IDL:m/A:1.0", "IDL:m/Nope:1.0");
        let _ = r.ancestors("IDL:m/A:1.0");
    }

    // ── operations and annotations ──────────────────────────────────────────

    #[test]
    fn operation_signatures_carry_what_an_invoker_needs() {
        let r = load(
            "module m { exception E { long code; }; \
             interface I { oneway void fire(in string topic); \
                           long add(in long a, inout long b, out long c) raises (E); }; };",
        );
        let i = r.interface("IDL:m/I:1.0").unwrap();
        let fire = &i.operations["fire"];
        assert!(fire.oneway);
        assert_eq!(fire.returns, TypeCode::Void);

        let add = &i.operations["add"];
        assert!(!add.oneway);
        assert_eq!(add.params.len(), 3);
        assert_eq!(add.params[0].direction, ParamDirection::In);
        assert_eq!(add.params[1].direction, ParamDirection::InOut);
        assert_eq!(add.params[2].direction, ParamDirection::Out);
        assert_eq!(add.raises, ["IDL:m/E:1.0"]);
    }

    #[test]
    fn attributes_are_registered_with_their_mutability() {
        let r =
            load("module m { interface I { readonly attribute long n; attribute string s; }; };");
        let i = r.interface("IDL:m/I:1.0").unwrap();
        assert!(i.attributes["n"].readonly);
        assert!(!i.attributes["s"].readonly);
    }

    /// The SIDL layer is the point of owning the parser, so the registry has to
    /// carry it through — an annotation that stops at the AST helps nobody.
    #[test]
    fn sidl_annotations_reach_the_registry() {
        let r = load(
            "module bank {\n\
             //@ ai_desc: Transfers funds between accounts.\n\
             interface Transfer {\n\
             //@ ai_effect: destructive\n\
             //@ ai_authz: bank.transfer.write\n\
             void execute(\n\
             //@ ai_unit: KRW\n\
             in long amount);\n\
             };\n\
             };",
        );
        let ann = r.annotations("IDL:bank/Transfer:1.0").expect("interface annotations");
        assert_eq!(ann["ai_desc"], "Transfers funds between accounts.");

        let op = &r.interface("IDL:bank/Transfer:1.0").unwrap().operations["execute"];
        assert_eq!(op.annotations["ai_effect"], "destructive");
        assert_eq!(op.annotations["ai_authz"], "bank.transfer.write");
        assert_eq!(op.params[0].annotations["ai_unit"], "KRW");
    }

    // ── constants ───────────────────────────────────────────────────────────

    /// The registry records the *value*, not only the type. It used to record
    /// only the type, and `orbweaver-gen` skipped every constant because of it
    /// — measured end to end when a contract declared its authorization scope
    /// as a `const string` so a servant could name it, and the servant could
    /// not.
    #[test]
    fn constants_carry_their_evaluated_value() {
        let r = load(
            "module gc14 {\n\
               const long    MAX_RETRIES = 3;\n\
               const double  EPSILON     = 0.0001;\n\
               const string  VERSION     = \"1.2\";\n\
               const boolean STRICT      = TRUE;\n\
               module inner { const long OFFSET = MAX_RETRIES * 2; };\n\
             };",
        );
        assert_eq!(r.const_value("IDL:gc14/MAX_RETRIES:1.0"), Some(&ConstValue::Int(3)));
        assert_eq!(r.const_value("IDL:gc14/EPSILON:1.0"), Some(&ConstValue::Float(0.0001)));
        assert_eq!(r.const_value("IDL:gc14/VERSION:1.0"), Some(&ConstValue::Str("1.2".into())));
        assert_eq!(r.const_value("IDL:gc14/STRICT:1.0"), Some(&ConstValue::Bool(true)));
        // The one that needs both halves: arithmetic, and a name resolved
        // outwards from an inner module.
        assert_eq!(r.const_value("IDL:gc14/inner/OFFSET:1.0"), Some(&ConstValue::Int(6)));
        assert!(matches!(
            r.get("IDL:gc14/VERSION:1.0"),
            Some(Entry::Const { tc: TypeCode::String(0), .. })
        ));
    }

    /// The value is coerced to the declared type before it is stored, so a
    /// consumer never has to ask whether an integer under a `double` meant 3
    /// or 3.0 — and an enumerator constant keeps the enum it belongs to.
    #[test]
    fn a_value_is_stored_as_the_declared_type_says() {
        let r = load(
            "module m {\n\
               enum Colour { RED, GREEN, BLUE };\n\
               const Colour DEFAULT_COLOUR = GREEN;\n\
               const double WHOLE = 3;\n\
               const long   HALVED = 7 / 2;\n\
               const octet  MASK = 0xF0;\n\
               const char   TAB = '\\t';\n\
               const long   SHIFTED = 1 << 4 | 3;\n\
               const long   INVERTED = ~0;\n\
             };",
        );
        assert_eq!(
            r.const_value("IDL:m/DEFAULT_COLOUR:1.0"),
            Some(&ConstValue::Enum {
                id: "IDL:m/Colour:1.0".into(),
                member: "GREEN".into(),
                ordinal: 1
            })
        );
        assert_eq!(r.const_value("IDL:m/WHOLE:1.0"), Some(&ConstValue::Float(3.0)));
        assert_eq!(
            r.const_value("IDL:m/HALVED:1.0"),
            Some(&ConstValue::Int(3)),
            "integer division"
        );
        assert_eq!(r.const_value("IDL:m/MASK:1.0"), Some(&ConstValue::Int(0xF0)));
        assert_eq!(r.const_value("IDL:m/TAB:1.0"), Some(&ConstValue::Int(9)), "a char is its code");
        // The rest of the operator table the parser can produce.
        assert_eq!(r.const_value("IDL:m/SHIFTED:1.0"), Some(&ConstValue::Int(19)), "1 << 4 | 3");
        assert_eq!(r.const_value("IDL:m/INVERTED:1.0"), Some(&ConstValue::Int(-1)));
    }

    /// The two halves of a bare `fixed` constant, asserted together, because
    /// the comment on `type_of`'s `TypeSpec::Fixed` arm asserted both in prose
    /// and had gone false on one of them for four days.
    ///
    /// The *value* is the exact decimal the source wrote — through the lexer,
    /// the AST and `coerce`, none of which had a decimal in them before
    /// 9a27659. The *TypeCode* is still the `0, 0` marker, because
    /// `fixed_pt_const_type` is the bare keyword and digits and scale come from
    /// the value, which `type_of` is not handed.
    ///
    /// One test rather than two, on purpose: closing the second half — folding
    /// the pair out of the folded value — is a change with a peer to measure
    /// against, and it cannot land without this going red and the note beside
    /// the arm being read. That is the whole repair for a comment nothing
    /// compiles.
    #[test]
    fn a_bare_fixed_constant_has_an_exact_value_and_an_unevaluated_typecode() {
        let r = load("module gf { const fixed TAX = 9.9d; };");
        assert_eq!(
            r.const_value("IDL:gf/TAX:1.0"),
            Some(&ConstValue::Fixed { unscaled: 99, scale: 1 }),
            "the decimal survives the lexer, the AST and `coerce`"
        );
        let Some(Entry::Const { tc, .. }) = r.get("IDL:gf/TAX:1.0") else { panic!("no constant") };
        assert_eq!(
            *tc,
            TypeCode::Fixed { digits: 0, scale: 0 },
            "a bare `fixed` has no bounds to read and `type_of` has no value to derive them from"
        );
    }

    /// What the registry does with an expression it cannot evaluate: it stores
    /// no value at all. Not a zero, not an empty string — a `None` a consumer
    /// has to notice.
    #[test]
    fn an_expression_that_does_not_fold_stores_no_value() {
        for (src, why) in [
            ("module m { const long X = NOT_DECLARED; };", "an unresolved name"),
            ("module m { const long X = 1 / 0; };", "no answer exists"),
            ("module m { const octet X = 300; };", "outside the declared type's range"),
            ("module m { const short X = 40000; };", "outside the declared type's range"),
            ("module m { const long X = Y; const long Y = 1; };", "used before it is declared"),
            ("module m { struct S { long a; }; const long X = S; };", "not a constant"),
        ] {
            let r = load(src);
            assert!(
                matches!(r.get("IDL:m/X:1.0"), Some(Entry::Const { value: None, .. })),
                "{why}: {src} stored {:?}",
                r.get("IDL:m/X:1.0")
            );
        }
    }

    #[test]
    fn a_forward_declaration_does_not_erase_a_body() {
        let r = load("module m { interface I { long f(); }; interface I; };");
        let i = r.interface("IDL:m/I:1.0").unwrap();
        assert!(!i.forward_only);
        assert!(i.operations.contains_key("f"));
    }
}
