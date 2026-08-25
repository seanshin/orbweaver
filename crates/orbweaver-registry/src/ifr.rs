//! Read-only `CORBA::Repository` facade over the registry.
//!
//! `docs/PLAN.md` §7 has carried an "optional read-only `CORBA::Repository`
//! facade" line since Phase 2; `docs/PLAN-SERVICES.md` §7 gave it a batch
//! shape. This is that batch: the registry — which is already the project's
//! Interface Repository, populated from IDL — put behind the standard IR
//! interfaces so a *foreign* ORB's DII client can ask what our objects
//! support without being handed our IDL first.
//!
//! # Why this lives in `orbweaver-registry` and not `orbweaver-giop`
//!
//! The facade needs both halves: the registry for the facts and
//! [`Dispatch`]/[`Server`] for the wire. `orbweaver-registry` already depends
//! on `orbweaver-giop` (for `TypeCode` and the client), and the dependency
//! must not run the other way, so the servant belongs on the registry side of
//! the edge. `naming_server.rs` is in `orbweaver-giop` because CosNaming
//! needs nothing but GIOP; this one does.
//!
//! # What is served
//!
//! The subset a DII client actually walks, and nothing else:
//!
//! | Interface | Operations |
//! |---|---|
//! | `Repository` | `lookup_id` |
//! | `Contained` | `_get_id`, `_get_name`, `_get_absolute_name`, `_get_version` |
//! | `IRObject` | `_get_def_kind` |
//! | `InterfaceDef` | `describe_interface`, `_get_base_interfaces`, `is_a` |
//! | `CORBA::Object` | `_is_a`, `_non_existent` |
//!
//! `describe_interface` is the operation that matters: one call and a DII
//! client has every signature it needs.
//!
//! `_get_version` is served because the registry has the answer and the write
//! half already refused it properly. It was absent until 2026-08-14, which
//! `docs/SERVICES-COVERAGE.md` §5 caught by driving all 44 declared IR
//! operations over the wire: `_set_version` answered `NO_PERMISSION` — "the
//! operation exists and the answer is no" — while `_get_version` answered
//! `BAD_OPERATION`, "no such operation", on a version this facade parses out
//! of every repository id it handles. A read-only facade with those two
//! backwards is the sharpest kind of wrong: it refuses the write on purpose and
//! denies the read by accident.
//!
//! # Deferred operations answer `NO_IMPLEMENT`, and why that is not pedantry
//!
//! [`is_deferred`] lists the IR operations this facade knows about and has
//! decided not to implement — `Container::contents`/`lookup`/`lookup_name`/
//! `describe_contents`, `Contained::describe`/`_get_defined_in`/
//! `_get_containing_repository`, `Repository::get_canonical_typecode`/
//! `get_primitive`, and `IDLType::_get_type`. Each answers `NO_IMPLEMENT`.
//!
//! They used to answer `BAD_OPERATION`, with the reasons written in this
//! module and in `docs/PLAN-SERVICES.md` §7 — and that is precisely the defect
//! `SERVICES-COVERAGE.md` was written to find: **the wire could not tell a
//! considered deferral from an oversight.** Both answered "no such operation",
//! so the only thing separating a decision from a gap was whether somebody had
//! written a sentence in a document the client cannot read. Twelve of 107
//! declared operations across the five services were in exactly that state.
//!
//! `NO_IMPLEMENT` is the specification's answer for an operation that exists
//! on the interface and has no implementation here, which is what a deferral
//! *is*. The three answers are now three different facts, on the wire, with no
//! document needed to tell them apart:
//!
//! | answer | means |
//! |---|---|
//! | `NO_PERMISSION` | the operation exists, is implementable, and is refused as policy (every mutating operation) |
//! | `NO_IMPLEMENT` | the operation exists in the contract; this facade has not implemented it, on purpose |
//! | `BAD_OPERATION` | there is no such operation on the object addressed — try a different reference |
//!
//! The reasons stay written down, because the wire says *that* an operation is
//! deferred and never *why*: `contents`/`lookup`/`lookup_name`/
//! `describe_contents` enumerate a container and `describe_interface` already
//! carries what a client wanted from them; `describe`, `_get_defined_in` and
//! `_get_containing_repository` likewise (`describe_interface`'s `defined_in`
//! member is the containing module's repository id); `get_canonical_typecode`
//! and `get_primitive` would have to **mint** `TypeCode`s the registry never
//! stored — a canonical form and the primitives table — which is the one thing
//! a facade that only reports must not do; `_get_type` is merely unimplemented,
//! and was unimplementable until the registry stopped loading
//! `::CORBA::TypeCode` as `void`.
//!
//! # Why every mutating operation is refused with `NO_PERMISSION`
//!
//! `create_*`, `destroy`, `move` and every `_set_` accessor answer
//! `NO_PERMISSION`. This is a policy refusal, not a missing feature, and the
//! reason is the whole trust model: **the registry is populated from IDL
//! through S4, never over the wire.** Everything downstream — the guard's
//! `ai_authz` scopes, the approval gate on `ai_effect: destructive`, the diff
//! that decides whether a change is compatible — reads a registry whose
//! provenance is a reviewed IDL file. A writable IFR would be a second
//! ingestion path with none of those gates on it, so it is refused at the
//! servant rather than left to deployment configuration.
//!
//! `BAD_OPERATION` would have been the wrong answer: it says "no such
//! operation", and a client would reasonably retry against a different
//! reference. `NO_PERMISSION` says the operation exists and the answer is no.
//!
//! # Object keys
//!
//! One key per registry entry, derived from the repository id by
//! concatenation:
//!
//! ```text
//! root                      the Repository object itself
//! root ++ "/ifr/" ++ <id>   the Contained/InterfaceDef for <id>
//! ```
//!
//! The derivation is *reversible*, which is the point. A minted-key table
//! would mean references stop working across a restart and would grow without
//! bound as clients look things up; recovering the id from the key means the
//! server holds no per-reference state at all, and a reference a client
//! stored yesterday still resolves. [`Dispatch::knows`] answers by looking
//! the recovered id up in the registry, so a key naming an entry we do not
//! have is `OBJECT_NOT_EXIST` rather than a servant that answers nonsense.
//! Repository ids contain `/` and `:` but the split is on the *first* `/ifr/`
//! after the root, so no id can be ambiguous.
//!
//! # Cross-ORB oracle
//!
//! omniORBpy ships the IR stubs as `omniORB.ir_idl` (measured 2026-08-13:
//! `hasattr(CORBA, "Repository")` is `False` on a bare `import CORBA` and
//! `True` after `import omniORB.ir_idl`), so the narrow works with no
//! generated stubs of ours. Start `spike-ifr spikes/ifr.ior --hold`, then:
//!
//! ```text
//! python3 -c "import sys, CORBA, omniORB.ir_idl; \
//! orb = CORBA.ORB_init(sys.argv); \
//! r = orb.string_to_object(open('spikes/ifr.ior').read().strip())._narrow(CORBA.Repository); \
//! d = r.lookup_id('IDL:gc10/Both:1.0')._narrow(CORBA.InterfaceDef).describe_interface(); \
//! print(d.name, d.id, [o.name for o in d.operations], [a.name for a in d.attributes], d.base_interfaces)"
//! ```
//!
//! Measured output, omniORB 4.3.4 / omniORBpy 4.3.x on macOS, run against
//! this servant on 2026-08-13:
//!
//! ```text
//! Both IDL:gc10/Both:1.0 ['touch', 'value'] ['id', 'name'] ['IDL:gc10/Derived:1.0', 'IDL:gc10/Nameable:1.0']
//! ```
//!
//! That is a foreign ORB decoding our `FullInterfaceDescription` — the second
//! "their client, our server" claim in the project after F6's. The same
//! session, extended over `IDL:tms/TrackManager:1.0` (golden 19), had omniORB
//! print `PARAM_IN`, `OP_ONEWAY`, `ATTR_READONLY`, `dk_Interface` and
//! `dk_Repository` as *named* enumerators — so the ordinals are right, not
//! merely self-consistent — decode a raised `NoSuchTrack` down to its member
//! names, follow `snapshot`'s return `TypeCode` through `tk_alias` →
//! `tk_sequence` → `tk_struct` to `Track`'s six members, and raise
//! `CORBA.NO_PERMISSION` from `create_module`. The refusal is therefore what
//! a foreign client sees, not only what our own client decodes.
//!
//! One serving limit the harness must respect, inherited from [`Server`]:
//! `--hold` is stopped by killing it — `destroy` is refused, so there is no
//! remote shutdown. The one-connection-at-a-time limit this note used to carry
//! is gone, and so is the serialized-dispatch limit that replaced it.
//!
//! # Sharing: no lock at all, and the policy is why
//!
//! This is the one servant in the batch that implements [`SharedDispatch`]
//! with **no synchronisation whatsoever**, and it is not an optimisation — it
//! is what the refusal policy above already bought and nobody had collected.
//! Every mutating operation answers `NO_PERMISSION` because the registry's
//! only ingestion path is reviewed IDL through S4; a servant that refuses
//! every write is a servant with no mutable state; a servant with no mutable
//! state needs no lock. `&self` and a `Sync` [`Registry`] are the whole
//! implementation.
//!
//! So the IFR facade scales with cores rather than with contention: N clients
//! walking `describe_interface` over a large repository — the expensive
//! operation here, since it assembles every inherited operation and attribute
//! — run at once, all the way down, with nothing between them. It is worth
//! noticing which decision paid for that. The write refusal was made for
//! provenance reasons in a different batch, and the concurrency is a
//! consequence of it, not of anything done here.
//!
//! Two things follow that a future editor must keep true. Adding a cache
//! (memoising `describe_interface`, say) would add mutable state and therefore
//! a lock, and would trade this property for a smaller one. And making the
//! registry replaceable at run time would do the same. If either becomes worth
//! it, the state goes behind [`orbweaver_giop::guarded::Guarded`] and this
//! section gets rewritten — not quietly widened.
//!
//! [`Dispatch`]: orbweaver_giop::server::Dispatch
//! [`SharedDispatch`]: orbweaver_giop::server::SharedDispatch
//! [`Server`]: orbweaver_giop::server::Server

use std::collections::{BTreeMap, BTreeSet};

use orbweaver_cdr::{Decoder, Encoder};
use orbweaver_giop::server::{Completion, Dispatch, Request, SharedDispatch, SystemException};
use orbweaver_giop::typecode::{self, Member, TypeCode};
use orbweaver_giop::{IiopProfile, Ior, Result, Version};

use crate::{ConstValue, Entry, ParamDirection, Registry, RepositoryId};

// ── repository ids ───────────────────────────────────────────────────────────

/// `CORBA::Repository` — the object the root key answers as.
pub const REPOSITORY_ID: &str = "IDL:omg.org/CORBA/Repository:1.0";
/// `CORBA::Container`, which `Repository` and `InterfaceDef` both derive from.
pub const CONTAINER_ID: &str = "IDL:omg.org/CORBA/Container:1.0";
/// `CORBA::Contained` — what `lookup_id` is declared to return.
pub const CONTAINED_ID: &str = "IDL:omg.org/CORBA/Contained:1.0";
/// `CORBA::InterfaceDef` — the object an interface entry answers as.
pub const INTERFACE_DEF_ID: &str = "IDL:omg.org/CORBA/InterfaceDef:1.0";
/// `CORBA::ModuleDef` (§14.5.7) — the object a module answers as. A module has
/// no registry entry; see [`Target::Module`].
pub const MODULE_DEF_ID: &str = "IDL:omg.org/CORBA/ModuleDef:1.0";
/// `CORBA::OperationDef` (§14.5.23) — the object an interface's operation
/// answers as, reachable since the containment walk landed.
pub const OPERATION_DEF_ID: &str = "IDL:omg.org/CORBA/OperationDef:1.0";
/// `CORBA::AttributeDef` (§14.5.21) — likewise for an attribute.
pub const ATTRIBUTE_DEF_ID: &str = "IDL:omg.org/CORBA/AttributeDef:1.0";
/// `CORBA::PrimitiveDef` (§14.5.14) — what `Repository::get_primitive` returns.
/// Not `Contained`: a primitive type is unnamed and has no repository id.
pub const PRIMITIVE_DEF_ID: &str = "IDL:omg.org/CORBA/PrimitiveDef:1.0";
/// `CORBA::IDLType`, a base of `InterfaceDef` and of every type definition.
pub const IDL_TYPE_ID: &str = "IDL:omg.org/CORBA/IDLType:1.0";
/// `CORBA::IRObject`, the root of the IR interface hierarchy.
pub const IR_OBJECT_ID: &str = "IDL:omg.org/CORBA/IRObject:1.0";
/// `CORBA::Object`, which every reference answers `_is_a` for.
pub const OBJECT_ID: &str = "IDL:omg.org/CORBA/Object:1.0";
/// The system exception every mutating operation is refused with.
pub const NO_PERMISSION: &str = "IDL:omg.org/CORBA/NO_PERMISSION:1.0";
/// The system exception a **deferred** operation is refused with — declared by
/// the contract, deliberately not implemented here. See the module docs for why
/// this is not `BAD_OPERATION`.
pub const NO_IMPLEMENT: &str = "IDL:omg.org/CORBA/NO_IMPLEMENT:1.0";

/// The infix that separates the root key from the repository id it addresses.
const KEY_INFIX: &str = "/ifr/";

// ── enumerations from the IR IDL ─────────────────────────────────────────────

/// `CORBA::DefinitionKind` ordinals, in the specification's declaration order.
///
/// **Every ordinal here was read back by name from a peer, and the list stops
/// exactly where that measurement stops.** `CORBA — Part 1: Interfaces, v3.4`
/// §14.5.1 "Supporting Type Definitions", in chapter 14 "The Interface
/// Repository", declares 36 enumerators, `dk_none` (0) through `dk_Event` (35);
/// omniORB 4.3.4's own `omniORB.ir_idl` stubs declare **25**, `dk_none` (0)
/// through `dk_AbstractInterface` (24) — the CORBA 3.0 list — measured
/// 2026-08-25 by asking a client we did not write to name each ordinal our
/// facade wrote for a probe contract (clause (a) of the licensing boundary: a
/// separate process over TCP). So 0..24 are named below, and 25..35 are not:
/// answering `dk_LocalInterface` to that peer would raise `MARSHAL` in *its*
/// stub, and an ordinal nothing can name is a claim no measurement backs.
/// `corpus/services/ir-subset.idl` declares the full 36 for the opposite
/// reason — a *decoder* must accept what a conformant sender may write.
///
/// This doc used to say "only the values this facade produces are named", which
/// was the sentence that let `dk_Value` and `dk_Native` stay absent while the
/// registry had held both since 2026-08-20 (74b5662) and 2026-08-21 (22637a8).
/// A list scoped to what a function currently answers cannot tell you the
/// function is answering wrongly.
///
/// Adding an ordinal above 24 owes a peer that can name it, not a count off the
/// specification's list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
#[allow(missing_docs)]
pub enum DefinitionKind {
    None = 0,
    All = 1,
    Attribute = 2,
    Constant = 3,
    Exception = 4,
    Interface = 5,
    Module = 6,
    Operation = 7,
    Typedef = 8,
    Alias = 9,
    Struct = 10,
    Union = 11,
    Enum = 12,
    Primitive = 13,
    String = 14,
    Sequence = 15,
    Array = 16,
    Repository = 17,
    Wstring = 18,
    Fixed = 19,
    Value = 20,
    ValueBox = 21,
    ValueMember = 22,
    Native = 23,
    AbstractInterface = 24,
}

/// `OperationMode::OP_NORMAL`.
pub const OP_NORMAL: u32 = 0;
/// `OperationMode::OP_ONEWAY`.
pub const OP_ONEWAY: u32 = 1;

/// `ParameterMode::PARAM_IN`.
pub const PARAM_IN: u32 = 0;
/// `ParameterMode::PARAM_OUT`.
pub const PARAM_OUT: u32 = 1;
/// `ParameterMode::PARAM_INOUT`.
pub const PARAM_INOUT: u32 = 2;

/// `AttributeMode::ATTR_NORMAL`.
pub const ATTR_NORMAL: u32 = 0;
/// `AttributeMode::ATTR_READONLY`.
pub const ATTR_READONLY: u32 = 1;

// ── the description structs ──────────────────────────────────────────────────

/// `CORBA::ParameterDescription`.
///
/// `type_def` is an `IDLType` object reference in the IDL. This facade mints
/// no `IDLType` objects — the `TypeCode` in `tc` is the complete answer and an
/// `IDLType` reference would only be a second way to ask the same question —
/// so it is always written nil. A client that narrows it gets a nil reference,
/// which is a truthful "there is no such object here".
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterDescription {
    /// Parameter name as written in IDL.
    pub name: String,
    /// The IDL member is `type`, which Rust reserves.
    pub tc: TypeCode,
    /// [`PARAM_IN`], [`PARAM_OUT`] or [`PARAM_INOUT`].
    pub mode: u32,
}

impl ParameterDescription {
    /// Writes the struct in IDL member order.
    pub fn write_to(&self, e: &mut Encoder) -> Result<()> {
        e.put_str(&self.name);
        typecode::encode(e, &self.tc)?;
        nil_ref().write_to(e)?; // type_def
        e.put_u32(self.mode);
        Ok(())
    }

    /// Reads the struct, discarding the nil `type_def` reference.
    pub fn read_from(d: &mut Decoder<'_>) -> Result<Self> {
        let name = d.get_string()?;
        let tc = typecode::decode(d)?;
        let _type_def = Ior::read_from(d)?;
        let mode = d.get_u32()?;
        Ok(Self { name, tc, mode })
    }
}

/// `CORBA::ExceptionDescription`, as it appears in an operation's `exceptions`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExceptionDescription {
    /// Unqualified name.
    pub name: String,
    /// Repository id.
    pub id: RepositoryId,
    /// Repository id of the containing module, empty at file scope.
    pub defined_in: RepositoryId,
    /// Version part of the repository id.
    pub version: String,
    /// The `tk_except` TypeCode.
    pub tc: TypeCode,
}

impl ExceptionDescription {
    /// Writes the struct in IDL member order.
    pub fn write_to(&self, e: &mut Encoder) -> Result<()> {
        e.put_str(&self.name);
        e.put_str(&self.id);
        e.put_str(&self.defined_in);
        e.put_str(&self.version);
        typecode::encode(e, &self.tc)
    }

    /// Reads the struct.
    pub fn read_from(d: &mut Decoder<'_>) -> Result<Self> {
        Ok(Self {
            name: d.get_string()?,
            id: d.get_string()?,
            defined_in: d.get_string()?,
            version: d.get_string()?,
            tc: typecode::decode(d)?,
        })
    }
}

/// `CORBA::OperationDescription`.
#[derive(Debug, Clone, PartialEq)]
pub struct OperationDescription {
    /// Operation name.
    pub name: String,
    /// Repository id of the operation itself, `<interface path>/<name>`.
    pub id: RepositoryId,
    /// Repository id of the interface that **declares** it, which for an
    /// inherited operation is a base rather than the interface described.
    pub defined_in: RepositoryId,
    /// Version part of the repository id.
    pub version: String,
    /// Return type.
    pub result: TypeCode,
    /// [`OP_NORMAL`] or [`OP_ONEWAY`].
    pub mode: u32,
    /// `ContextIdSeq`. Always empty, and inventing identifiers would be worse
    /// than reporting none.
    ///
    /// This said the `context` clause "is not parsed", which reads as a grammar
    /// gap and is not one: `orbweaver_idl::parse` accepts `context (...)` and
    /// *discards* it — the identifiers never reach the AST, so there is nothing
    /// here to report. The consequence is the same and the sentence was not: a
    /// reader checking it finds `eat_kw("context")` in the parser and concludes
    /// this comment is wrong about the whole claim rather than about one word.
    pub contexts: Vec<String>,
    /// Parameters in declaration order.
    pub parameters: Vec<ParameterDescription>,
    /// Exceptions the operation raises.
    pub exceptions: Vec<ExceptionDescription>,
}

impl OperationDescription {
    /// Writes the struct in IDL member order.
    pub fn write_to(&self, e: &mut Encoder) -> Result<()> {
        e.put_str(&self.name);
        e.put_str(&self.id);
        e.put_str(&self.defined_in);
        e.put_str(&self.version);
        typecode::encode(e, &self.result)?;
        e.put_u32(self.mode);
        e.put_u32(self.contexts.len() as u32);
        for c in &self.contexts {
            e.put_str(c);
        }
        e.put_u32(self.parameters.len() as u32);
        for p in &self.parameters {
            p.write_to(e)?;
        }
        e.put_u32(self.exceptions.len() as u32);
        for x in &self.exceptions {
            x.write_to(e)?;
        }
        Ok(())
    }

    /// Reads the struct.
    pub fn read_from(d: &mut Decoder<'_>) -> Result<Self> {
        let name = d.get_string()?;
        let id = d.get_string()?;
        let defined_in = d.get_string()?;
        let version = d.get_string()?;
        let result = typecode::decode(d)?;
        let mode = d.get_u32()?;
        let contexts = read_string_seq(d)?;
        let parameters = read_seq(d, 8, ParameterDescription::read_from)?;
        let exceptions = read_seq(d, 8, ExceptionDescription::read_from)?;
        Ok(Self { name, id, defined_in, version, result, mode, contexts, parameters, exceptions })
    }
}

/// `CORBA::AttributeDescription`.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeDescription {
    /// Attribute name.
    pub name: String,
    /// Repository id of the attribute itself.
    pub id: RepositoryId,
    /// Repository id of the interface that declares it.
    pub defined_in: RepositoryId,
    /// Version part of the repository id.
    pub version: String,
    /// Attribute type.
    pub tc: TypeCode,
    /// [`ATTR_NORMAL`] or [`ATTR_READONLY`].
    pub mode: u32,
}

impl AttributeDescription {
    /// Writes the struct in IDL member order.
    pub fn write_to(&self, e: &mut Encoder) -> Result<()> {
        e.put_str(&self.name);
        e.put_str(&self.id);
        e.put_str(&self.defined_in);
        e.put_str(&self.version);
        typecode::encode(e, &self.tc)?;
        e.put_u32(self.mode);
        Ok(())
    }

    /// Reads the struct.
    pub fn read_from(d: &mut Decoder<'_>) -> Result<Self> {
        Ok(Self {
            name: d.get_string()?,
            id: d.get_string()?,
            defined_in: d.get_string()?,
            version: d.get_string()?,
            tc: typecode::decode(d)?,
            mode: d.get_u32()?,
        })
    }
}

/// `CORBA::InterfaceDef::FullInterfaceDescription` — everything a DII client
/// needs to build calls against an interface, in one reply.
///
/// The member order is the IR IDL's and the CDR layout follows from it; the
/// oracle for both is omniORBpy's generated stub, whose
/// `FullInterfaceDescription.__init__` signature is
/// `(name, id, defined_in, version, operations, attributes, base_interfaces,
/// type)` — read from the installed fixture, never copied into this project.
///
/// # `operations` and `attributes` include inherited members
///
/// The specification is not explicit about whether `describe_interface`
/// reports inherited members. This facade includes them, because the named
/// consumer is a DII client asking "what may I call", and an inherited
/// operation is callable. Nothing is lost by the choice: each description's
/// `defined_in` names the interface that declares it, so a client that wants
/// only the immediate members can filter on `defined_in == id`. A name
/// declared in both a derived interface and a base appears once, as the
/// derived one.
#[derive(Debug, Clone, PartialEq)]
pub struct FullInterfaceDescription {
    /// Unqualified interface name.
    pub name: String,
    /// Repository id.
    pub id: RepositoryId,
    /// Repository id of the containing module, empty at file scope.
    pub defined_in: RepositoryId,
    /// Version part of the repository id.
    pub version: String,
    /// Own operations first, then inherited ones.
    pub operations: Vec<OperationDescription>,
    /// Own attributes first, then inherited ones.
    pub attributes: Vec<AttributeDescription>,
    /// Repository ids of the **direct** bases, in declaration order — the
    /// same set `_get_base_interfaces` returns as references.
    pub base_interfaces: Vec<RepositoryId>,
    /// The interface's own `tk_objref` TypeCode. The IDL member is `type`.
    pub tc: TypeCode,
}

impl FullInterfaceDescription {
    /// Writes the struct in IDL member order.
    pub fn write_to(&self, e: &mut Encoder) -> Result<()> {
        e.put_str(&self.name);
        e.put_str(&self.id);
        e.put_str(&self.defined_in);
        e.put_str(&self.version);
        e.put_u32(self.operations.len() as u32);
        for o in &self.operations {
            o.write_to(e)?;
        }
        e.put_u32(self.attributes.len() as u32);
        for a in &self.attributes {
            a.write_to(e)?;
        }
        e.put_u32(self.base_interfaces.len() as u32);
        for b in &self.base_interfaces {
            e.put_str(b);
        }
        typecode::encode(e, &self.tc)
    }

    /// Reads the struct.
    pub fn read_from(d: &mut Decoder<'_>) -> Result<Self> {
        let name = d.get_string()?;
        let id = d.get_string()?;
        let defined_in = d.get_string()?;
        let version = d.get_string()?;
        let operations = read_seq(d, 8, OperationDescription::read_from)?;
        let attributes = read_seq(d, 8, AttributeDescription::read_from)?;
        let base_interfaces = read_string_seq(d)?;
        let tc = typecode::decode(d)?;
        Ok(Self { name, id, defined_in, version, operations, attributes, base_interfaces, tc })
    }
}

/// Reads a bounded `sequence<T>`.
///
/// `min_element_size` goes through [`Decoder::validate_count`], so a length
/// larger than the remaining buffer could hold is rejected before any
/// allocation — a declared count is attacker-controlled input.
fn read_seq<'b, T, F>(d: &mut Decoder<'b>, min_element_size: usize, mut read: F) -> Result<Vec<T>>
where
    F: FnMut(&mut Decoder<'b>) -> Result<T>,
{
    let declared = d.get_u32()?;
    let n = d.validate_count(declared, min_element_size)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(read(d)?);
    }
    Ok(out)
}

/// Reads a `sequence<string>` — `ContextIdSeq` and `RepositoryIdSeq`.
fn read_string_seq(d: &mut Decoder<'_>) -> Result<Vec<String>> {
    read_seq(d, 4, |d| Ok(d.get_string()?))
}

/// Reads an `InterfaceDefSeq` — what `_get_base_interfaces` returns.
pub fn read_interface_def_seq(d: &mut Decoder<'_>) -> Result<Vec<Ior>> {
    // An IOR is at least a type-id length and a profile count.
    read_seq(d, 8, Ior::read_from)
}

// ── the browse half's descriptions ───────────────────────────────────────────

/// `CORBA::ModuleDescription` (§14.5.7).
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleDescription {
    /// Unqualified name.
    pub name: String,
    /// Repository id.
    pub id: RepositoryId,
    /// Containing module's id, empty at file scope.
    pub defined_in: RepositoryId,
    /// Version part of the repository id.
    pub version: String,
}

impl ModuleDescription {
    /// Writes the struct in IDL member order.
    pub fn write_to(&self, e: &mut Encoder) -> Result<()> {
        e.put_str(&self.name);
        e.put_str(&self.id);
        e.put_str(&self.defined_in);
        e.put_str(&self.version);
        Ok(())
    }

    /// Reads the struct.
    pub fn read_from(d: &mut Decoder<'_>) -> Result<Self> {
        Ok(Self {
            name: d.get_string()?,
            id: d.get_string()?,
            defined_in: d.get_string()?,
            version: d.get_string()?,
        })
    }
}

/// `CORBA::TypeDescription` (§14.5.9) — what every `TypedefDef` describes as.
///
/// A `NativeDef` describes as one too (§14.5.34, `NativeDef : TypedefDef`),
/// which is why this facade needs no separate native description.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDescription {
    /// Unqualified name.
    pub name: String,
    /// Repository id.
    pub id: RepositoryId,
    /// Containing module's id, empty at file scope.
    pub defined_in: RepositoryId,
    /// Version part of the repository id.
    pub version: String,
    /// The IDL member is `type`, which Rust reserves.
    pub tc: TypeCode,
}

impl TypeDescription {
    /// Writes the struct in IDL member order.
    pub fn write_to(&self, e: &mut Encoder) -> Result<()> {
        e.put_str(&self.name);
        e.put_str(&self.id);
        e.put_str(&self.defined_in);
        e.put_str(&self.version);
        typecode::encode(e, &self.tc)
    }

    /// Reads the struct.
    pub fn read_from(d: &mut Decoder<'_>) -> Result<Self> {
        Ok(Self {
            name: d.get_string()?,
            id: d.get_string()?,
            defined_in: d.get_string()?,
            version: d.get_string()?,
            tc: typecode::decode(d)?,
        })
    }
}

/// `CORBA::InterfaceDescription` (§14.5.24).
///
/// **Five members, not six.** It is not [`FullInterfaceDescription`] with
/// something missing: `describe` on an `InterfaceDef` returns *this*, and
/// `describe_interface` returns the full one. The specification gives this
/// struct no `type` member, and adding one to make the two look alike would
/// put a member on the wire that no client's stub declares.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceDescription {
    /// Unqualified name.
    pub name: String,
    /// Repository id.
    pub id: RepositoryId,
    /// Containing module's id, empty at file scope.
    pub defined_in: RepositoryId,
    /// Version part of the repository id.
    pub version: String,
    /// Repository ids of the direct bases, in declaration order.
    pub base_interfaces: Vec<RepositoryId>,
}

impl InterfaceDescription {
    /// Writes the struct in IDL member order.
    pub fn write_to(&self, e: &mut Encoder) -> Result<()> {
        e.put_str(&self.name);
        e.put_str(&self.id);
        e.put_str(&self.defined_in);
        e.put_str(&self.version);
        e.put_u32(self.base_interfaces.len() as u32);
        for b in &self.base_interfaces {
            e.put_str(b);
        }
        Ok(())
    }

    /// Reads the struct.
    pub fn read_from(d: &mut Decoder<'_>) -> Result<Self> {
        Ok(Self {
            name: d.get_string()?,
            id: d.get_string()?,
            defined_in: d.get_string()?,
            version: d.get_string()?,
            base_interfaces: read_string_seq(d)?,
        })
    }
}

/// `CORBA::ValueDescription` (§14.5.31) — ten members, in that order.
///
/// `is_abstract` and `is_custom` sit **between `id` and `defined_in`**, which
/// is the one member order in this file that reads like a mistake and is not.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueDescription {
    /// Unqualified name.
    pub name: String,
    /// Repository id.
    pub id: RepositoryId,
    /// `VM_ABSTRACT`.
    pub is_abstract: bool,
    /// `VM_CUSTOM`.
    pub is_custom: bool,
    /// Containing module's id, empty at file scope.
    pub defined_in: RepositoryId,
    /// Version part of the repository id.
    pub version: String,
    /// Interfaces the value type supports. Empty here: the front end's AST
    /// does not carry a `supports` clause, so there is nothing to report and
    /// nothing is invented.
    pub supported_interfaces: Vec<RepositoryId>,
    /// Abstract base values. Empty here, for the same reason.
    pub abstract_base_values: Vec<RepositoryId>,
    /// `VM_TRUNCATABLE`.
    pub is_truncatable: bool,
    /// The concrete base value's repository id, empty when there is none.
    pub base_value: RepositoryId,
}

impl ValueDescription {
    /// Writes the struct in IDL member order.
    pub fn write_to(&self, e: &mut Encoder) -> Result<()> {
        e.put_str(&self.name);
        e.put_str(&self.id);
        e.put_bool(self.is_abstract);
        e.put_bool(self.is_custom);
        e.put_str(&self.defined_in);
        e.put_str(&self.version);
        e.put_u32(self.supported_interfaces.len() as u32);
        for s in &self.supported_interfaces {
            e.put_str(s);
        }
        e.put_u32(self.abstract_base_values.len() as u32);
        for a in &self.abstract_base_values {
            e.put_str(a);
        }
        e.put_bool(self.is_truncatable);
        e.put_str(&self.base_value);
        Ok(())
    }

    /// Reads the struct.
    pub fn read_from(d: &mut Decoder<'_>) -> Result<Self> {
        Ok(Self {
            name: d.get_string()?,
            id: d.get_string()?,
            is_abstract: d.get_bool()?,
            is_custom: d.get_bool()?,
            defined_in: d.get_string()?,
            version: d.get_string()?,
            supported_interfaces: read_string_seq(d)?,
            abstract_base_values: read_string_seq(d)?,
            is_truncatable: d.get_bool()?,
            base_value: d.get_string()?,
        })
    }
}

// ── the TypeCodes those descriptions travel under ────────────────────────────

/// The `TypeCode`s of the IR's description structs, built once per call.
///
/// `Contained::describe` returns `Description { DefinitionKind kind; any value; }`
/// (§14.5.3), and an `any` is a `TypeCode` followed by the value. So the wire
/// answer is only as good as these: a client extracts the `any` by comparing
/// the arriving `TypeCode` against its own stub's, and a member name or a
/// member *order* that differs makes the extraction fail — or, worse, succeed
/// against the wrong layout.
///
/// The aliases are written out rather than collapsed to `string`. The IR IDL
/// declares `Identifier`, `RepositoryId`, `VersionSpec` and `ScopedName` as
/// named aliases, so that is what a peer's stub holds, and a `TypeCode` that
/// said `tk_string` where the peer says `tk_alias` is equivalent under
/// `equivalent()` and **not equal** under `equal()` — two operations a client
/// may use either of.
pub mod description_tc {
    use super::{Member, TypeCode};

    /// `IDL:omg.org/CORBA/Identifier:1.0`.
    pub fn identifier() -> TypeCode {
        alias("Identifier", TypeCode::String(0))
    }

    /// `IDL:omg.org/CORBA/RepositoryId:1.0`.
    pub fn repository_id() -> TypeCode {
        alias("RepositoryId", TypeCode::String(0))
    }

    /// `IDL:omg.org/CORBA/VersionSpec:1.0`.
    pub fn version_spec() -> TypeCode {
        alias("VersionSpec", TypeCode::String(0))
    }

    /// `IDL:omg.org/CORBA/RepositoryIdSeq:1.0`.
    pub fn repository_id_seq() -> TypeCode {
        alias(
            "RepositoryIdSeq",
            TypeCode::Sequence { element: Box::new(repository_id()), bound: 0 },
        )
    }

    /// `IDL:omg.org/CORBA/ContextIdSeq:1.0`, whose element is itself an alias
    /// of `Identifier` — two levels, as the IDL writes it.
    pub fn context_id_seq() -> TypeCode {
        let ident = alias("ContextIdentifier", identifier());
        alias("ContextIdSeq", TypeCode::Sequence { element: Box::new(ident), bound: 0 })
    }

    /// `IDL:omg.org/CORBA/DefinitionKind:1.0`, all 36 enumerators.
    pub fn definition_kind() -> TypeCode {
        TypeCode::Enum {
            id: "IDL:omg.org/CORBA/DefinitionKind:1.0".into(),
            name: "DefinitionKind".into(),
            members: DEFINITION_KIND_ENUMERATORS.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    /// `IDL:omg.org/CORBA/AttributeMode:1.0`.
    pub fn attribute_mode() -> TypeCode {
        TypeCode::Enum {
            id: "IDL:omg.org/CORBA/AttributeMode:1.0".into(),
            name: "AttributeMode".into(),
            members: vec!["ATTR_NORMAL".into(), "ATTR_READONLY".into()],
        }
    }

    /// `IDL:omg.org/CORBA/OperationMode:1.0`.
    pub fn operation_mode() -> TypeCode {
        TypeCode::Enum {
            id: "IDL:omg.org/CORBA/OperationMode:1.0".into(),
            name: "OperationMode".into(),
            members: vec!["OP_NORMAL".into(), "OP_ONEWAY".into()],
        }
    }

    /// `IDL:omg.org/CORBA/ParameterMode:1.0`.
    pub fn parameter_mode() -> TypeCode {
        TypeCode::Enum {
            id: "IDL:omg.org/CORBA/ParameterMode:1.0".into(),
            name: "ParameterMode".into(),
            members: vec!["PARAM_IN".into(), "PARAM_OUT".into(), "PARAM_INOUT".into()],
        }
    }

    /// `IDL:omg.org/CORBA/ModuleDescription:1.0` (§14.5.7).
    pub fn module_description() -> TypeCode {
        strukt(
            "ModuleDescription",
            vec![
                member("name", identifier()),
                member("id", repository_id()),
                member("defined_in", repository_id()),
                member("version", version_spec()),
            ],
        )
    }

    /// `IDL:omg.org/CORBA/TypeDescription:1.0` (§14.5.9).
    pub fn type_description() -> TypeCode {
        strukt(
            "TypeDescription",
            vec![
                member("name", identifier()),
                member("id", repository_id()),
                member("defined_in", repository_id()),
                member("version", version_spec()),
                member("type", TypeCode::TypeCode),
            ],
        )
    }

    /// `IDL:omg.org/CORBA/ExceptionDescription:1.0` (§14.5.20).
    pub fn exception_description() -> TypeCode {
        strukt(
            "ExceptionDescription",
            vec![
                member("name", identifier()),
                member("id", repository_id()),
                member("defined_in", repository_id()),
                member("version", version_spec()),
                member("type", TypeCode::TypeCode),
            ],
        )
    }

    /// `IDL:omg.org/CORBA/ConstantDescription:1.0` (§14.5.8).
    pub fn constant_description() -> TypeCode {
        strukt(
            "ConstantDescription",
            vec![
                member("name", identifier()),
                member("id", repository_id()),
                member("defined_in", repository_id()),
                member("version", version_spec()),
                member("type", TypeCode::TypeCode),
                member("value", TypeCode::Any),
            ],
        )
    }

    /// `IDL:omg.org/CORBA/InterfaceDescription:1.0` (§14.5.24) — five members.
    pub fn interface_description() -> TypeCode {
        strukt(
            "InterfaceDescription",
            vec![
                member("name", identifier()),
                member("id", repository_id()),
                member("defined_in", repository_id()),
                member("version", version_spec()),
                member("base_interfaces", repository_id_seq()),
            ],
        )
    }

    /// `IDL:omg.org/CORBA/ValueDescription:1.0` (§14.5.31) — ten members, with
    /// the two booleans between `id` and `defined_in`.
    pub fn value_description() -> TypeCode {
        strukt(
            "ValueDescription",
            vec![
                member("name", identifier()),
                member("id", repository_id()),
                member("is_abstract", TypeCode::Boolean),
                member("is_custom", TypeCode::Boolean),
                member("defined_in", repository_id()),
                member("version", version_spec()),
                member("supported_interfaces", repository_id_seq()),
                member("abstract_base_values", repository_id_seq()),
                member("is_truncatable", TypeCode::Boolean),
                member("base_value", repository_id()),
            ],
        )
    }

    /// `IDL:omg.org/CORBA/AttributeDescription:1.0` (§14.5.21).
    pub fn attribute_description() -> TypeCode {
        strukt(
            "AttributeDescription",
            vec![
                member("name", identifier()),
                member("id", repository_id()),
                member("defined_in", repository_id()),
                member("version", version_spec()),
                member("type", TypeCode::TypeCode),
                member("mode", attribute_mode()),
            ],
        )
    }

    /// `IDL:omg.org/CORBA/ParameterDescription:1.0` (§14.5.23).
    pub fn parameter_description() -> TypeCode {
        strukt(
            "ParameterDescription",
            vec![
                member("name", identifier()),
                member("type", TypeCode::TypeCode),
                member(
                    "type_def",
                    TypeCode::ObjRef { id: super::IDL_TYPE_ID.into(), name: "IDLType".into() },
                ),
                member("mode", parameter_mode()),
            ],
        )
    }

    /// `IDL:omg.org/CORBA/OperationDescription:1.0` (§14.5.23) — note that
    /// `contexts` precedes `parameters`, which precedes `exceptions`.
    pub fn operation_description() -> TypeCode {
        let pars = alias(
            "ParDescriptionSeq",
            TypeCode::Sequence { element: Box::new(parameter_description()), bound: 0 },
        );
        let excs = alias(
            "ExcDescriptionSeq",
            TypeCode::Sequence { element: Box::new(exception_description()), bound: 0 },
        );
        strukt(
            "OperationDescription",
            vec![
                member("name", identifier()),
                member("id", repository_id()),
                member("defined_in", repository_id()),
                member("version", version_spec()),
                member("result", TypeCode::TypeCode),
                member("mode", operation_mode()),
                member("contexts", context_id_seq()),
                member("parameters", pars),
                member("exceptions", excs),
            ],
        )
    }

    /// Every `DefinitionKind` enumerator, in declaration order (§14.5.1).
    ///
    /// The list is here and not beside [`super::DefinitionKind`] because the
    /// two answer different questions: that enum is **what this facade may
    /// write**, and stops at the ordinal a peer was measured naming; this is
    /// **what the type is**, and a `TypeCode` for an enum carries every
    /// enumerator or it is a different type.
    const DEFINITION_KIND_ENUMERATORS: [&str; 36] = [
        "dk_none",
        "dk_all",
        "dk_Attribute",
        "dk_Constant",
        "dk_Exception",
        "dk_Interface",
        "dk_Module",
        "dk_Operation",
        "dk_Typedef",
        "dk_Alias",
        "dk_Struct",
        "dk_Union",
        "dk_Enum",
        "dk_Primitive",
        "dk_String",
        "dk_Sequence",
        "dk_Array",
        "dk_Repository",
        "dk_Wstring",
        "dk_Fixed",
        "dk_Value",
        "dk_ValueBox",
        "dk_ValueMember",
        "dk_Native",
        "dk_AbstractInterface",
        "dk_LocalInterface",
        "dk_Component",
        "dk_Home",
        "dk_Factory",
        "dk_Finder",
        "dk_Emits",
        "dk_Publishes",
        "dk_Consumes",
        "dk_Provides",
        "dk_Uses",
        "dk_Event",
    ];

    fn alias(name: &str, aliased: TypeCode) -> TypeCode {
        TypeCode::Alias {
            id: format!("IDL:omg.org/CORBA/{name}:1.0"),
            name: name.to_owned(),
            aliased: Box::new(aliased),
        }
    }

    fn strukt(name: &str, members: Vec<Member>) -> TypeCode {
        TypeCode::Struct {
            id: format!("IDL:omg.org/CORBA/{name}:1.0"),
            name: name.to_owned(),
            members,
        }
    }

    fn member(name: &str, tc: TypeCode) -> Member {
        Member { name: name.to_owned(), tc }
    }
}

// ── repository-id arithmetic ─────────────────────────────────────────────────

/// The three things every IR description derives from a repository id: the
/// unqualified name, the containing module's id, and the version.
///
/// `IDL:a/b/C:1.0` splits into `("C", "IDL:a/b:1.0", "1.0")`. A top-level
/// definition has no containing module, so `defined_in` is empty — the
/// container is the repository itself, which has no repository id of its own.
/// An id in a shape we do not recognise keeps its whole self as the name
/// rather than being silently mangled.
pub fn split_repository_id(id: &str) -> (String, RepositoryId, String) {
    let Some(rest) = id.strip_prefix("IDL:") else {
        return (id.to_owned(), String::new(), "1.0".into());
    };
    let Some((path, version)) = rest.rsplit_once(':') else {
        return (id.to_owned(), String::new(), "1.0".into());
    };
    match path.rsplit_once('/') {
        Some((container, name)) => {
            (name.to_owned(), format!("IDL:{container}:{version}"), version.to_owned())
        }
        None => (path.to_owned(), String::new(), version.to_owned()),
    }
}

/// The `ScopedName` an IR client sees: `IDL:a/b/C:1.0` is `::a::b::C`.
pub fn absolute_name(id: &str) -> String {
    let path = id
        .strip_prefix("IDL:")
        .and_then(|rest| rest.rsplit_once(':').map(|(p, _)| p))
        .unwrap_or(id);
    format!("::{}", path.replace('/', "::"))
}

/// The nil object reference: empty type id, no profiles (§9.3.6).
fn nil_ref() -> Ior {
    Ior { type_id: String::new(), profiles: Vec::new() }
}

/// A `NO_PERMISSION` refusal. See the module docs for why writes are refused
/// rather than unimplemented: the registry's single ingestion path is IDL
/// through S4, and a writable IFR would be a second one with no gates on it.
fn refused() -> SystemException {
    SystemException { id: NO_PERMISSION.into(), minor: 0, completed: Completion::No }
}

/// A `NO_IMPLEMENT` refusal: the operation is in the contract and this facade
/// has decided not to implement it.
fn not_implemented() -> SystemException {
    SystemException { id: NO_IMPLEMENT.into(), minor: 0, completed: Completion::No }
}

/// A `BAD_PARAM` for an argument the specification leaves **undefined**.
///
/// `lookup_name`'s `levels_to_search` is the only one today: §14.5.4.1 says
/// *"use of values of levels_to_search of 0 or of negative numbers other than
/// -1 is undefined"*. Answering something arbitrary for an undefined input is
/// how an undefined input becomes a compatibility promise nobody chose, so
/// this facade refuses instead — with `Completion::No`, because nothing ran.
fn bad_param() -> SystemException {
    SystemException {
        id: "IDL:omg.org/CORBA/BAD_PARAM:1.0".into(),
        minor: 0,
        completed: Completion::No,
    }
}

/// Writes an `any`: its `TypeCode`, then its value, in the same stream.
///
/// An `any` is **not** an encapsulation (§9.3.4), so alignment continues from
/// wherever the enclosing struct left off. Writing one into a fresh encoder and
/// splicing the bytes in would align the value from the wrong origin, which is
/// the class of defect that only shows up against a real peer.
fn put_any<F>(e: &mut Encoder, tc: &TypeCode, body: F) -> Result<()>
where
    F: FnOnce(&mut Encoder) -> Result<()>,
{
    typecode::encode(e, tc)?;
    body(e)
}

/// Writes a folded constant as the value half of an `any` of type `tc`.
///
/// The declared `TypeCode` decides the wire form, not the [`ConstValue`]
/// variant: `ConstValue::Int` carries every integer type plus `char`, `wchar`
/// and `octet` as the code point they denote, and only the declared type says
/// which width and signedness to write. A value whose declared type this
/// facade cannot write — a `fixed`, which the v1 wire does not carry — is
/// reported by returning `false` so the caller can answer honestly rather than
/// emit a guess.
fn put_const_value(e: &mut Encoder, tc: &TypeCode, value: &ConstValue) -> Result<bool> {
    let written = match (tc, value) {
        (TypeCode::Short, ConstValue::Int(n)) => {
            e.put_i16(*n as i16);
            true
        }
        (TypeCode::UShort, ConstValue::Int(n)) => {
            e.put_u16(*n as u16);
            true
        }
        (TypeCode::Long, ConstValue::Int(n)) => {
            e.put_i32(*n as i32);
            true
        }
        (TypeCode::ULong, ConstValue::Int(n)) => {
            e.put_u32(*n as u32);
            true
        }
        (TypeCode::LongLong, ConstValue::Int(n)) => {
            e.put_i64(*n as i64);
            true
        }
        (TypeCode::ULongLong, ConstValue::Int(n)) => {
            e.put_u64(*n as u64);
            true
        }
        (TypeCode::Octet, ConstValue::Int(n)) => {
            e.put_u8(*n as u8);
            true
        }
        (TypeCode::Char, ConstValue::Int(n)) => {
            e.put_u8(*n as u8);
            true
        }
        (TypeCode::WChar, ConstValue::Int(n)) => {
            // A `wchar`'s wire form depends on the GIOP version, so it goes
            // through the stream's codec and is refused — not guessed — when
            // the connection carries none. `put_wchar_text` owns that rule;
            // writing 1.2's form here would be a second, silently wrong copy.
            match u32::try_from(*n).ok().and_then(char::from_u32) {
                Some(ch) if e.has_codec() => {
                    e.put_wchar_text(ch)?;
                    true
                }
                _ => false,
            }
        }
        (TypeCode::Boolean, ConstValue::Bool(b)) => {
            e.put_bool(*b);
            true
        }
        (TypeCode::Float, ConstValue::Float(f)) => {
            e.put_f32(*f as f32);
            true
        }
        (TypeCode::Double, ConstValue::Float(f)) => {
            e.put_f64(*f);
            true
        }
        (TypeCode::String(_), ConstValue::Str(s)) => {
            e.put_str(s);
            true
        }
        (TypeCode::WString(_), ConstValue::Str(s)) if e.has_codec() => {
            e.put_wstr(s)?;
            true
        }
        (TypeCode::Enum { .. }, ConstValue::Enum { ordinal, .. }) => {
            e.put_u32(*ordinal);
            true
        }
        // An alias in front of any of the above is still that type.
        (TypeCode::Alias { aliased, .. }, v) => return put_const_value(e, aliased, v),
        _ => false,
    };
    Ok(written)
}

/// The repository id a `TypeCode` carries, for the named kinds that carry one.
///
/// `sequence`, `array` and the primitives are anonymous and have none — which
/// is exactly the case §14.5.6.1 tells `get_canonical_typecode` to handle by
/// recursion rather than by lookup.
fn typecode_id(tc: &TypeCode) -> Option<&str> {
    match tc {
        TypeCode::ObjRef { id, .. }
        | TypeCode::Struct { id, .. }
        | TypeCode::Union { id, .. }
        | TypeCode::Enum { id, .. }
        | TypeCode::Alias { id, .. }
        | TypeCode::Except { id, .. }
        | TypeCode::Value { id, .. }
        | TypeCode::AbstractInterface { id, .. }
        | TypeCode::Native { id, .. } => Some(id),
        TypeCode::Recursive(id) => Some(id),
        TypeCode::Null
        | TypeCode::Void
        | TypeCode::Short
        | TypeCode::Long
        | TypeCode::UShort
        | TypeCode::ULong
        | TypeCode::Float
        | TypeCode::Double
        | TypeCode::Boolean
        | TypeCode::Char
        | TypeCode::Octet
        | TypeCode::Any
        | TypeCode::TypeCode
        | TypeCode::Principal
        | TypeCode::LongLong
        | TypeCode::ULongLong
        | TypeCode::LongDouble
        | TypeCode::WChar
        | TypeCode::String(_)
        | TypeCode::WString(_)
        | TypeCode::Fixed { .. }
        | TypeCode::Sequence { .. }
        | TypeCode::Array { .. } => None,
    }
}

/// Whether `op` is an IR operation this facade deliberately does not implement.
///
/// An explicit list rather than a shape, and that is the difference from
/// [`is_mutating`]: the mutating surface is defined by a naming convention the
/// specification will keep, while a deferral is a decision somebody made about
/// one named operation. Matching a *shape* here would quietly capture the next
/// revision's new operations as "deferred on purpose", which is the exact
/// confusion this list exists to end. An operation nobody has decided about
/// stays `BAD_OPERATION` — and that, per `docs/SERVICES-COVERAGE.md`, is a
/// finding rather than a state to be comfortable in.
///
/// Every entry has its reason in the module docs; if one is ever implemented,
/// it comes off this list and into the match in [`RepositoryServer::handle`].
///
/// # The list is empty as of 2026-08-25, and that is the interesting part
///
/// It held ten, and the ten were one thing: **the containment walk** —
/// `contents`, `lookup`, `lookup_name`, `describe_contents`, `describe`,
/// `_get_defined_in`, `_get_containing_repository`, `get_canonical_typecode`,
/// `get_primitive`, `_get_type`. The reason recorded on 2026-08-14 was that
/// this registry is a facade over our own contracts and a browse had no
/// consumer. Two things had changed by 2026-08-25 and both were measured:
/// `orbweaver-mcp` browses this same registry through Rust (`search_interfaces`
/// and the console catalogue), and a working IFR *is* a browsable one — a
/// client that can only look an id up already had to know the id. So the
/// trigger fired and the ten landed together, because they are one shape and
/// nine of them are unusable without the tenth.
///
/// The function stays. An empty list is a claim — *this facade defers nothing
/// on purpose right now* — and it is a different claim from having no list at
/// all, which is what `BAD_OPERATION` for everything used to mean before the
/// deferral/oversight split existed.
pub fn is_deferred(_op: &str) -> bool {
    false
}

/// Whether `op` mutates the repository, and so must be refused.
///
/// Matched by shape rather than by an exhaustive list: the IR's mutating
/// surface is every `create_*` factory, every `_set_` attribute writer,
/// `destroy` and `move`. A list would leak a new operation on the next
/// revision of the specification; the shapes will not change.
fn is_mutating(op: &str) -> bool {
    op.starts_with("create_") || op.starts_with("_set_") || matches!(op, "destroy" | "move")
}

// ── the servant ──────────────────────────────────────────────────────────────

/// What object key a request addressed.
///
/// Four of these five are `Contained` and reachable by walking down from the
/// root, which is what `Container::contents` exists to let a client do. Before
/// 2026-08-25 only two existed, because the walk was `NO_IMPLEMENT` and the
/// only way to reach an object was `lookup_id` on an id you already knew.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// The root: the `Repository` itself.
    Repository,
    /// A module: a `Container` **and** a `Contained`, and the only object here
    /// that is not a registry entry. `Registry::load` records a module's id and
    /// qualified name but defines no entry for it, so the set of modules is
    /// derived from the scopes its entries sit in — see
    /// [`RepositoryServer::module_ids`].
    Module(RepositoryId),
    /// A registry entry: an interface, a type or a constant.
    Entry(RepositoryId),
    /// An operation of an interface entry. Not an entry either: operations live
    /// inside [`crate::InterfaceEntry`], and their repository ids are derived
    /// from the declaring interface's by [`member_id`].
    Operation {
        /// The interface that **declares** it, which is what `defined_in` says.
        owner: RepositoryId,
        /// The operation name.
        name: String,
    },
    /// An attribute of an interface entry, on the same terms.
    Attribute {
        /// The interface that declares it.
        owner: RepositoryId,
        /// The attribute name.
        name: String,
    },
    /// A `PrimitiveDef` (§14.5.14), by `PrimitiveKind` ordinal.
    ///
    /// The one object here that is **not `Contained`** — primitive types are
    /// unnamed, so it has no repository id, no name and no container, and
    /// `describe` on it is `BAD_OPERATION` rather than an empty description.
    /// Its key therefore cannot be a repository id, and is not: see
    /// [`PRIMITIVE_KEY_PREFIX`].
    Primitive(u32),
}

/// What a `PrimitiveDef`'s object key carries where every other key carries a
/// repository id.
///
/// A `PrimitiveDef` has none (§14.5.14), so the key space needs something that
/// **cannot** be one. Every repository id begins with a format name and a
/// colon — `IDL:`, `RMI:`, `DCE:` — so a lower-case `pk:` prefix is
/// unambiguous by construction rather than by hoping no id looks like it, and
/// `Self::object_for` tries it before anything that parses an id.
const PRIMITIVE_KEY_PREFIX: &str = "pk:";

/// `PrimitiveKind` ordinals (§14.5.14), in declaration order.
mod pk {
    /// `pk_null` — the one kind §14.5.14 says has no `PrimitiveDef`.
    pub const NULL: u32 = 0;
    /// `pk_value_base`, the last ordinal.
    pub const VALUE_BASE: u32 = 21;
}

impl Target {
    /// The repository id this object is registered under, or `None` for the
    /// root — which has none, because `Repository` derives from `Container`
    /// and not from `Contained` (§14.5.6).
    fn id(&self) -> Option<RepositoryId> {
        match self {
            Target::Repository | Target::Primitive(_) => None,
            Target::Module(id) | Target::Entry(id) => Some(id.clone()),
            Target::Operation { owner, name } | Target::Attribute { owner, name } => {
                Some(member_id(owner, name))
            }
        }
    }

    /// The object key body for this target: a repository id, or a
    /// [`PRIMITIVE_KEY_PREFIX`]-tagged kind for the one object that has none.
    fn key_body(&self) -> Option<String> {
        match self {
            Target::Primitive(kind) => Some(format!("{PRIMITIVE_KEY_PREFIX}{kind}")),
            other => other.id(),
        }
    }
}

/// The `TypeCode` a `PrimitiveKind` denotes (§14.5.14), or `None` for a kind
/// this facade has no `TypeCode` for.
///
/// `pk_null` is `None` because the specification says so — *"there are no
/// PrimitiveDefs with kind pk_null"*. `pk_value_base` is `None` for a reason
/// of ours: `orbweaver_giop::typecode::TypeCode` has no `ValueBase`, and
/// answering with a `tk_value` named `ValueBase` would be a `TypeCode` this
/// workspace cannot decode back.
fn primitive_typecode(kind: u32) -> Option<TypeCode> {
    Some(match kind {
        1 => TypeCode::Void,
        2 => TypeCode::Short,
        3 => TypeCode::Long,
        4 => TypeCode::UShort,
        5 => TypeCode::ULong,
        6 => TypeCode::Float,
        7 => TypeCode::Double,
        8 => TypeCode::Boolean,
        9 => TypeCode::Char,
        10 => TypeCode::Octet,
        11 => TypeCode::Any,
        12 => TypeCode::TypeCode,
        13 => TypeCode::Principal,
        // §14.5.14: "A PrimitiveDef with kind pk_string represents an
        // unbounded string" — a bounded one is a StringDef.
        14 => TypeCode::String(0),
        // "A PrimitiveDef with kind pk_objref represents the IDL type Object."
        15 => TypeCode::ObjRef { id: OBJECT_ID.to_owned(), name: "Object".to_owned() },
        16 => TypeCode::LongLong,
        17 => TypeCode::ULongLong,
        18 => TypeCode::LongDouble,
        19 => TypeCode::WChar,
        20 => TypeCode::WString(0),
        pk::NULL | pk::VALUE_BASE => return None,
        _ => return None,
    })
}

/// The repository id of an operation or attribute of `owner`.
///
/// `IDL:bank/Party:1.0` + `party_id` becomes `IDL:bank/Party/party_id:1.0`.
///
/// One home for the derivation, because two things must agree on it exactly:
/// the ids inside a `FullInterfaceDescription`, and the object key
/// [`RepositoryServer::target`] reverses to reach an `OperationDef` or an
/// `AttributeDef`. A member reachable by a description but not by a key would
/// be a reference a client is handed and cannot dial.
///
/// The version is the **owner's**, taken off its id rather than assumed to be
/// `1.0`, so `#pragma version` and `#pragma ID` reach members too.
pub fn member_id(owner: &str, name: &str) -> RepositoryId {
    let (_, _, version) = split_repository_id(owner);
    let path = owner.strip_suffix(&format!(":{version}")).unwrap_or(owner);
    format!("{path}/{name}:{version}")
}

/// A read-only IR servant over a [`Registry`], behind
/// [`Server`](orbweaver_giop::server::Server).
///
/// One instance answers for the whole repository: the root object key the
/// server was bound with, plus one derived key per registry entry (see the
/// module docs for the derivation). `host` and `port` are what go into the
/// references it mints, and are the caller's to publish correctly — Phase 0
/// assumption D, the bind address and the publishable address differ behind
/// NAT.
#[derive(Debug, Clone)]
pub struct RepositoryServer {
    host: String,
    port: u16,
    root: Vec<u8>,
    registry: Registry,
}

impl RepositoryServer {
    /// A facade over `registry`, rooted at `root_key`, minting references that
    /// point at `host:port`.
    pub fn new(host: impl Into<String>, port: u16, root_key: Vec<u8>, registry: Registry) -> Self {
        Self { host: host.into(), port, root: root_key, registry }
    }

    /// The `Repository` object's key — what the server must be bound with for
    /// the two to describe the same object.
    pub fn root_key(&self) -> &[u8] {
        &self.root
    }

    /// The registry being served.
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// A publishable reference to the `Repository` itself.
    pub fn root_ior(&self) -> Ior {
        self.ior_for(REPOSITORY_ID, self.root.clone())
    }

    /// The object key for the entry registered under `id`, derived rather
    /// than minted (module docs).
    pub fn entry_key(&self, id: &str) -> Vec<u8> {
        let mut key = self.root.clone();
        key.extend_from_slice(KEY_INFIX.as_bytes());
        key.extend_from_slice(id.as_bytes());
        key
    }

    /// The repository id an object key addresses, or `None` for the root key
    /// and for keys this server did not derive.
    pub fn id_from_key<'a>(&self, key: &'a [u8]) -> Option<&'a str> {
        let rest = key.strip_prefix(self.root.as_slice())?;
        let rest = rest.strip_prefix(KEY_INFIX.as_bytes())?;
        std::str::from_utf8(rest).ok()
    }

    /// A reference to the entry registered under `id`, or a nil reference if
    /// it is not registered.
    ///
    /// The type id advertised is `InterfaceDef` for an interface and the
    /// weaker `Contained` for anything else, so a client that narrows locally
    /// narrows to something we actually serve. An **abstract** interface is
    /// still an `InterfaceDef` here even though [`Self::def_kind`] now
    /// distinguishes it: `AbstractInterfaceDef` derives from `InterfaceDef` in
    /// the IR IDL, so the weaker id is the true one to advertise for a facade
    /// that serves the `InterfaceDef` operations and no `AbstractInterfaceDef`
    /// ones.
    ///
    /// Written out rather than left to `Some(_)`: the two non-interface entry
    /// kinds are named so that a third one is a build error here too.
    pub fn entry_ior(&self, id: &str) -> Ior {
        match self.registry.get(id) {
            None => nil_ref(),
            Some(Entry::Interface(_)) => self.ior_for(INTERFACE_DEF_ID, self.entry_key(id)),
            Some(Entry::Type(_) | Entry::Const { .. }) => {
                self.ior_for(CONTAINED_ID, self.entry_key(id))
            }
        }
    }

    /// A reference to any object this facade serves, by target.
    ///
    /// [`Self::entry_ior`] answers for a registry entry and is what `lookup_id`
    /// has always used; this answers for the three object kinds the containment
    /// walk reaches as well, and every reference a walk hands out comes from
    /// here. The type id is the **most derived** interface the facade actually
    /// serves for that object, so a client that narrows locally narrows to
    /// something dialable.
    fn reference_for(&self, target: &Target) -> Ior {
        if let Target::Repository = target {
            return self.root_ior();
        }
        if let Target::Entry(id) = target {
            return self.entry_ior(id);
        }
        // One home for the key body, so a reference handed out by a walk and
        // the key `target()` reverses are the same string by construction.
        let Some(body) = target.key_body() else { return nil_ref() };
        let type_id = match target {
            Target::Module(_) => MODULE_DEF_ID,
            Target::Operation { .. } => OPERATION_DEF_ID,
            Target::Attribute { .. } => ATTRIBUTE_DEF_ID,
            Target::Primitive(_) => PRIMITIVE_DEF_ID,
            Target::Repository | Target::Entry(_) => unreachable!("answered above"),
        };
        self.ior_for(type_id, self.entry_key(&body))
    }

    fn ior_for(&self, type_id: &str, key: Vec<u8>) -> Ior {
        Ior {
            type_id: type_id.to_owned(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: self.host.clone(),
                port: self.port,
                object_key: key,
                // §7.10.2.4: no TAG_CODE_SETS is a declaration of no wchar
                // support, and a conformant client then refuses inside itself
                // without sending anything (measured, omniORB 4.3.4). D009's
                // L2, landed with the rest of its cause rather than one site
                // at a time: the conversion lists stay empty, so this
                // advertises UTF-8 — which we have — and nothing we do not.
                components: vec![orbweaver_giop::codeset::server_component()],
            }],
        }
    }

    fn target(&self, key: &[u8]) -> std::result::Result<Target, SystemException> {
        if key == self.root.as_slice() {
            return Ok(Target::Repository);
        }
        let Some(id) = self.id_from_key(key) else {
            return Err(SystemException::object_not_exist());
        };
        self.object_for(id).ok_or_else(SystemException::object_not_exist)
    }

    /// Which object a repository id names, or `None` if this facade has none.
    ///
    /// Every reference this facade mints is derived from an id, so this is also
    /// the check that decides whether a key is dialable at all. The four cases
    /// are tried cheapest first: an entry is a map lookup, a module is a set
    /// lookup, and only an id matching neither is decomposed as a member.
    fn object_for(&self, id: &str) -> Option<Target> {
        if let Some(kind) = id.strip_prefix(PRIMITIVE_KEY_PREFIX) {
            let kind: u32 = kind.parse().ok()?;
            // A key for a kind with no `PrimitiveDef` is not a key this facade
            // ever minted, so it does not exist rather than answering emptily.
            primitive_typecode(kind)?;
            return Some(Target::Primitive(kind));
        }
        if self.registry.get(id).is_some() {
            return Some(Target::Entry(id.to_owned()));
        }
        if self.module_ids().contains_key(id) {
            return Some(Target::Module(id.to_owned()));
        }
        self.member_for(id)
    }

    /// An operation or attribute id, decomposed back into its declaring
    /// interface and member name.
    ///
    /// The inverse of [`member_id`], and it has to be an inverse rather than a
    /// re-derivation: `IDL:bank/Party/party_id:1.0` is split at the **last**
    /// path segment and the remainder is required to be an interface entry
    /// that actually declares that member. An id whose prefix is not an
    /// interface, or an interface that does not declare the name, is not a
    /// member of anything and gets no reference.
    fn member_for(&self, id: &str) -> Option<Target> {
        let (name, _, version) = split_repository_id(id);
        let path = id.strip_prefix("IDL:")?.rsplit_once(':').map(|(p, _)| p)?;
        let (owner_path, last) = path.rsplit_once('/')?;
        if last != name {
            return None;
        }
        let owner = format!("IDL:{owner_path}:{version}");
        let iface = self.registry.interface(&owner)?;
        if iface.operations.contains_key(&name) {
            Some(Target::Operation { owner, name })
        } else if iface.attributes.contains_key(&name) {
            Some(Target::Attribute { owner, name })
        } else {
            None
        }
    }

    /// Every module the registry's entries sit inside, id to qualified name.
    ///
    /// **Derived, because `Registry::load` defines no entry for a module** —
    /// it records the id and the qualified name and then walks into the body.
    /// A `Container` walk that skipped modules would report every definition
    /// as sitting directly in the repository, which is a different repository
    /// from the one the IDL described.
    ///
    /// The prefix problem is the same one [`Self::contained_of`] solves and is
    /// solved the same way: `#pragma prefix "acme.com"` puts a segment in the
    /// id path that is **not** a module, so the number of enclosing modules
    /// comes from the *qualified name*'s segment count and never from the
    /// path's. `IDL:acme.com/bank/Money:1.0` named `bank::Money` yields one
    /// module, `IDL:acme.com/bank:1.0` — not two, and not `IDL:acme.com:1.0`,
    /// which is a module that does not exist.
    ///
    /// Recomputed per call rather than cached: the registry behind this facade
    /// is shared and a cache would be a second copy of a fact that can change.
    fn module_ids(&self) -> BTreeMap<RepositoryId, String> {
        let mut out = BTreeMap::new();
        for id in self.registry.ids() {
            let Some(qual) = self.registry.qualified_name(id) else { continue };
            let (_, _, version) = split_repository_id(id);
            let Some(path) =
                id.strip_prefix("IDL:").and_then(|r| r.rsplit_once(':')).map(|(p, _)| p)
            else {
                continue;
            };
            let segments: Vec<&str> = path.split('/').collect();
            let scopes: Vec<&str> = qual.split("::").collect();
            // How many leading path segments are `#pragma prefix` and not
            // modules. A negative difference means the two disagree, which is
            // an ingested entry with no recorded scope; skip it rather than
            // invent a module.
            let Some(prefix_len) = segments.len().checked_sub(scopes.len()) else { continue };
            for depth in (prefix_len + 1)..segments.len() {
                let module_id = format!("IDL:{}:{version}", segments[..depth].join("/"));
                // An enclosing scope that *is* an entry is an interface (or an
                // exception, or a struct) with a nested definition in it, not a
                // module. `walk` recurses into interface bodies, so this is
                // reachable from ordinary IDL and would otherwise mint a second
                // object for an id that already has one.
                if self.registry.get(&module_id).is_some() {
                    continue;
                }
                let module_name = scopes[..depth - prefix_len].join("::");
                out.insert(module_id, module_name);
            }
        }
        out
    }

    /// `IRObject::_get_def_kind` for a registry entry.
    ///
    /// # The sentence that was false for five days
    ///
    /// This note read, over a `_ => DefinitionKind::None` catch-all: *"the
    /// registry stores both as an object-reference `TypeCode` because v1
    /// marshals neither (PLAN §4.4), so the facade cannot tell them apart"* —
    /// true when written, **false from 74b5662 (2026-08-20**, a valuetype became
    /// `TypeCode::Value`) **and 22637a8 (2026-08-21**, a native became
    /// `TypeCode::Native`). It asserted the opposite of the code for five days,
    /// and nothing went red because the *answer* was `dk_none` either way: the
    /// catch-all took both new variants the moment they existed and the
    /// registry's new distinction never reached the wire. A rewrite on
    /// 2026-08-25 replaced the false reason with a true one — "this facade does
    /// not name `dk_Value`, so it has no ordinal to answer with" — which was
    /// honest and still left a conformant client told **`dk_none`: no such
    /// definition, for a definition that exists** (D016 §5 B1, D018 §3.1).
    ///
    /// # What it answers now, and what measured it
    ///
    /// The ordinals were read back by name from omniORB 4.3.4's own
    /// `omniORB.ir_idl` client against this servant on 2026-08-25 — see
    /// [`DefinitionKind`], which stops where that measurement stops.
    ///
    /// # Why the match is exhaustive
    ///
    /// **A catch-all over `TypeCode` is how a classifier absorbs a distinction
    /// without mentioning it.** The three variants added in August were not
    /// forgotten; they were *swallowed*, silently, by an arm written before they
    /// existed. So there is no `_` arm here: a new `TypeCode` variant is a build
    /// error in this function, and whoever adds one has to say what an entry of
    /// that shape is. Every arm below is a verdict — an ordinal, or `None` with
    /// the reason true of that variant and no other.
    ///
    /// `TypeCode::Recursive` is the only `None`, and its reason is not a gap:
    /// it is not a definition at all but a **reference back to one being
    /// described**, so there is nothing under this id to have a kind. Resolving
    /// it would re-enter the entry the caller is already inside.
    ///
    /// # An abstract interface answers `dk_AbstractInterface` from `bases`
    ///
    /// It is an [`Entry::Interface`] like any other — `TypeCode::AbstractInterface`
    /// is a shape a *member* has, not one an entry does — so the distinction
    /// comes from [`InterfaceEntry::abstract_interface`], which the registry has
    /// recorded all along and this facade was not reading. An interface ingested
    /// from a peer is `false` there ("not known to be abstract", which is all a
    /// remote IFR can tell us) and so answers `dk_Interface`, which is the
    /// honest answer for what we know.
    ///
    /// [`InterfaceEntry::abstract_interface`]: crate::InterfaceEntry::abstract_interface
    fn def_kind(&self, id: &str) -> DefinitionKind {
        match self.registry.get(id) {
            Some(Entry::Interface(i)) => {
                if i.abstract_interface {
                    DefinitionKind::AbstractInterface
                } else {
                    DefinitionKind::Interface
                }
            }
            Some(Entry::Const { .. }) => DefinitionKind::Constant,
            Some(Entry::Type(tc)) => Self::kind_of_type(tc),
            None => DefinitionKind::None,
        }
    }

    /// The `DefinitionKind` for an [`Entry::Type`]'s `TypeCode`, exhaustively.
    ///
    /// Split out from [`Self::def_kind`] so the exhaustiveness is over one enum
    /// and reads as a table. See that function for why there is no `_` arm.
    ///
    /// The primitive and anonymous-type arms answer the ordinal the
    /// specification gives them (§14.5.14–§14.5.19: `dk_Primitive`,
    /// `dk_String`, `dk_Wstring`, `dk_Sequence`, `dk_Array`, `dk_Fixed`)
    /// rather than `dk_none`. No loader
    /// in this workspace registers one under a repository id — a
    /// `typedef sequence<long> S` is an `Alias` whose *aliased* type is the
    /// sequence — but [`Registry::define_ingested`] is public and takes any
    /// `Entry::Type`, and "nothing constructs this today" is the reason that
    /// produced the defect this function is repairing. A kind that is right if
    /// it ever happens costs one arm; a `dk_none` that is wrong if it ever
    /// happens costs a wire diagnosis.
    ///
    /// [`Registry::define_ingested`]: crate::Registry::define_ingested
    fn kind_of_type(tc: &TypeCode) -> DefinitionKind {
        match tc {
            // §14.5.14 `PrimitiveDef` covers every basic type, `void` and
            // `null` included; `pk_any`, `pk_TypeCode` and `pk_Principal` are
            // `PrimitiveKind`s in the same table.
            TypeCode::Null
            | TypeCode::Void
            | TypeCode::Short
            | TypeCode::Long
            | TypeCode::UShort
            | TypeCode::ULong
            | TypeCode::Float
            | TypeCode::Double
            | TypeCode::Boolean
            | TypeCode::Char
            | TypeCode::Octet
            | TypeCode::Any
            | TypeCode::TypeCode
            | TypeCode::Principal
            | TypeCode::LongLong
            | TypeCode::ULongLong
            | TypeCode::LongDouble
            | TypeCode::WChar => DefinitionKind::Primitive,
            // **The bound decides the kind, and the specification is explicit
            // about it.** §14.5.15: *"A `StringDef` represents an IDL bounded
            // string type. The unbounded string type is represented as a
            // `PrimitiveDef`"* — §14.5.14 lists `pk_string` and `pk_wstring`
            // among the primitive kinds. Zero is unbounded in this `TypeCode`,
            // so `string` is `dk_Primitive` and `string<40>` is `dk_String`.
            // Answering `dk_String` for both would have been the same class of
            // wrong-but-plausible as the catch-all above.
            TypeCode::String(0) => DefinitionKind::Primitive,
            TypeCode::String(_) => DefinitionKind::String,
            TypeCode::WString(0) => DefinitionKind::Primitive,
            TypeCode::WString(_) => DefinitionKind::Wstring,
            TypeCode::Fixed { .. } => DefinitionKind::Fixed,
            TypeCode::Sequence { .. } => DefinitionKind::Sequence,
            TypeCode::Array { .. } => DefinitionKind::Array,
            // An `ObjRef` entry names a specific interface — it carries a
            // repository id — so it is an `InterfaceDef`, the shape
            // `Registry::define_ingested` produces for an interface a peer
            // mentioned but did not describe. The *anonymous* `Object` type is
            // `pk_objref` on a `PrimitiveDef` (§14.5.14) and is never an entry
            // here, which is why this arm does not branch on the id.
            TypeCode::ObjRef { .. } => DefinitionKind::Interface,
            TypeCode::Struct { .. } => DefinitionKind::Struct,
            TypeCode::Union { .. } => DefinitionKind::Union,
            TypeCode::Enum { .. } => DefinitionKind::Enum,
            TypeCode::Alias { .. } => DefinitionKind::Alias,
            TypeCode::Except { .. } => DefinitionKind::Exception,
            // The three the catch-all used to take. Held distinctly by the
            // registry since 2026-08-20/21; answered on the wire since
            // 2026-08-25.
            TypeCode::Value { .. } => DefinitionKind::Value,
            TypeCode::Native { .. } => DefinitionKind::Native,
            TypeCode::AbstractInterface { .. } => DefinitionKind::AbstractInterface,
            // Not a definition: a reference back to one still being described.
            // The only `None` here, and the reason is true of this variant and
            // of nothing else in the enum.
            TypeCode::Recursive(_) => DefinitionKind::None,
        }
    }

    /// The interfaces whose members are visible on `id`: itself first, then
    /// its ancestors in inheritance order.
    fn declaring_chain(&self, id: &str) -> Vec<RepositoryId> {
        let mut chain = vec![id.to_owned()];
        chain.extend(self.registry.ancestors(id));
        chain
    }

    /// The `Contained` triple — name, container, version — for a registered id.
    ///
    /// [`split_repository_id`] alone is wrong once `#pragma prefix` is in
    /// play: it reads every leading path segment as an enclosing module, so
    /// `IDL:acme.com/Toplevel:1.0` comes back contained in a module
    /// `IDL:acme.com:1.0` that does not exist. The registry recorded the
    /// qualified name when it loaded the IDL, and the count of its segments is
    /// exactly what says how much of the path is prefix.
    ///
    /// Entries with no recorded name — anything ingested from a peer — fall
    /// back to the split, which is all the information there is for those.
    fn contained_of(&self, id: &str) -> (String, RepositoryId, String) {
        let split = split_repository_id(id);
        let Some(qual) = self.registry.qualified_name(id) else { return split };
        let (_, _, version) = split;
        let Some(path) =
            id.strip_prefix("IDL:").and_then(|rest| rest.rsplit_once(':').map(|(p, _)| p))
        else {
            return split_repository_id(id);
        };
        let name = qual.rsplit("::").next().unwrap_or(qual).to_owned();
        let segments: Vec<&str> = path.split('/').collect();
        let defined_in = if qual.split("::").count() < 2 || segments.len() < 2 {
            // Top level: the container is the repository, which has no id.
            String::new()
        } else {
            format!("IDL:{}:{version}", segments[..segments.len() - 1].join("/"))
        };
        (name, defined_in, version)
    }

    /// `::bank::Account` — the scoped name, with no prefix in it.
    fn absolute_name_of(&self, id: &str) -> String {
        match self.registry.qualified_name(id) {
            Some(qual) => format!("::{qual}"),
            None => absolute_name(id),
        }
    }

    /// Builds an `ExceptionDescription` from a repository id, taking the
    /// TypeCode from the registry when the exception is registered.
    fn exception_description(&self, id: &str) -> ExceptionDescription {
        let (name, defined_in, version) = self.contained_of(id);
        // An unregistered raises-clause means the IDL referenced an exception
        // we never saw a definition for. Reporting an empty tk_except is
        // honest — the members are genuinely unknown here — and keeps the
        // description decodable.
        let tc = self.registry.typecode(id).cloned().unwrap_or(TypeCode::Except {
            id: id.to_owned(),
            name: name.clone(),
            members: Vec::new(),
        });
        ExceptionDescription { name, id: id.to_owned(), defined_in, version, tc }
    }

    /// Assembles `describe_interface`'s reply for a registered interface.
    fn describe_interface(
        &self,
        id: &str,
    ) -> std::result::Result<FullInterfaceDescription, SystemException> {
        // Not an interface: `describe_interface` is not an operation of the
        // interface this object implements.
        if self.registry.interface(id).is_none() {
            return Err(SystemException::bad_operation());
        }
        let (name, defined_in, version) = self.contained_of(id);
        let chain = self.declaring_chain(id);

        let mut operations = Vec::new();
        let mut attributes = Vec::new();
        let mut seen_ops: BTreeSet<&str> = BTreeSet::new();
        let mut seen_attrs: BTreeSet<&str> = BTreeSet::new();

        for owner in &chain {
            let Some(iface) = self.registry.interface(owner) else { continue };
            let (_, _, owner_version) = split_repository_id(owner);
            let owner_path = owner.strip_suffix(&format!(":{owner_version}")).unwrap_or(owner);

            for (op_name, sig) in &iface.operations {
                if !seen_ops.insert(op_name.as_str()) {
                    continue;
                }
                operations.push(OperationDescription {
                    name: op_name.clone(),
                    id: format!("{owner_path}/{op_name}:{owner_version}"),
                    defined_in: owner.clone(),
                    version: owner_version.clone(),
                    result: sig.returns.clone(),
                    mode: if sig.oneway { OP_ONEWAY } else { OP_NORMAL },
                    contexts: Vec::new(),
                    parameters: sig
                        .params
                        .iter()
                        .map(|p| ParameterDescription {
                            name: p.name.clone(),
                            tc: p.tc.clone(),
                            mode: match p.direction {
                                ParamDirection::In => PARAM_IN,
                                ParamDirection::Out => PARAM_OUT,
                                ParamDirection::InOut => PARAM_INOUT,
                            },
                        })
                        .collect(),
                    exceptions: sig.raises.iter().map(|x| self.exception_description(x)).collect(),
                });
            }

            for (attr_name, sig) in &iface.attributes {
                if !seen_attrs.insert(attr_name.as_str()) {
                    continue;
                }
                attributes.push(AttributeDescription {
                    name: attr_name.clone(),
                    id: format!("{owner_path}/{attr_name}:{owner_version}"),
                    defined_in: owner.clone(),
                    version: owner_version.clone(),
                    tc: sig.tc.clone(),
                    mode: if sig.readonly { ATTR_READONLY } else { ATTR_NORMAL },
                });
            }
        }

        let bases = self.registry.interface(id).map(|i| i.bases.clone()).unwrap_or_default();

        Ok(FullInterfaceDescription {
            name: name.clone(),
            id: id.to_owned(),
            defined_in,
            version,
            operations,
            attributes,
            base_interfaces: bases,
            tc: TypeCode::ObjRef { id: id.to_owned(), name },
        })
    }

    /// Every repository id `_is_a` answers `true` for, given what the
    /// addressed object is.
    ///
    /// A constant is a `Contained` but **not** an `IDLType`: `ConstantDef` has
    /// a type, it is not one. That is the one place this list narrows, and it
    /// is written as its own arm rather than as a `_` — the arm used to be
    /// `_ =>` and covered `None` as well, which cannot occur here because
    /// [`Self::target`] refuses a key with no entry before this is reached. A
    /// catch-all that quietly covers an unreachable case is where the next
    /// `Entry` variant would have landed without a word.
    fn is_a_ids(&self, target: &Target) -> Vec<&'static str> {
        match target {
            Target::Repository => vec![REPOSITORY_ID, CONTAINER_ID, IR_OBJECT_ID, OBJECT_ID],
            // `ModuleDef : Container, Contained` (§14.5.7) — a container, and
            // not an `IDLType`: a module is not a type.
            Target::Module(_) => {
                vec![MODULE_DEF_ID, CONTAINER_ID, CONTAINED_ID, IR_OBJECT_ID, OBJECT_ID]
            }
            // `OperationDef : Contained` and `AttributeDef : Contained`
            // (§14.5.21, §14.5.23). Neither is a `Container` and neither is an
            // `IDLType` — an operation *has* a result type, it is not one.
            Target::Operation { .. } => {
                vec![OPERATION_DEF_ID, CONTAINED_ID, IR_OBJECT_ID, OBJECT_ID]
            }
            Target::Attribute { .. } => {
                vec![ATTRIBUTE_DEF_ID, CONTAINED_ID, IR_OBJECT_ID, OBJECT_ID]
            }
            // `PrimitiveDef : IDLType` and nothing else — **not** `Contained`.
            Target::Primitive(_) => vec![PRIMITIVE_DEF_ID, IDL_TYPE_ID, IR_OBJECT_ID, OBJECT_ID],
            Target::Entry(id) => match self.registry.get(id) {
                Some(Entry::Interface(_)) => vec![
                    INTERFACE_DEF_ID,
                    CONTAINER_ID,
                    CONTAINED_ID,
                    IDL_TYPE_ID,
                    IR_OBJECT_ID,
                    OBJECT_ID,
                ],
                Some(Entry::Type(_)) => vec![CONTAINED_ID, IDL_TYPE_ID, IR_OBJECT_ID, OBJECT_ID],
                Some(Entry::Const { .. }) | None => vec![CONTAINED_ID, IR_OBJECT_ID, OBJECT_ID],
            },
        }
    }

    // ── the browse half (§14.5.4, landed 2026-08-25) ─────────────────────────

    /// `Container::contents(limit_type, exclude_inherited)` (§14.5.4.1).
    ///
    /// The objects **directly contained by or inherited into** `target`:
    ///
    /// - the `Repository` contains every top-level module and every entry
    ///   whose `defined_in` is empty;
    /// - a `ModuleDef` contains the entries and sub-modules one level below it;
    /// - an `InterfaceDef` contains its operations and attributes, plus any
    ///   definition nested inside it.
    ///
    /// # The two parameters are the semantics
    ///
    /// `limit_type` filters by `DefinitionKind`, with `dk_all` meaning
    /// everything — **not** `dk_none`, which is a kind in its own right and
    /// would match nothing here. `exclude_inherited` decides whether an
    /// interface's inherited operations and attributes appear at all; when it
    /// is false they do, and each carries the `defined_in` of the interface
    /// that declares it, so a client can still tell them apart.
    ///
    /// A `contents` that ignored either argument would answer every call
    /// identically and pass any test that only counted the results.
    ///
    /// # Order
    ///
    /// §14.5.4.1 asks for *"the order in which the elements were created in or
    /// moved into the container"* — declaration order. **This facade cannot
    /// give that and does not pretend to**: `Registry` holds entries and
    /// members in `BTreeMap`s, so declaration order is gone by the time the
    /// IDL is loaded. The order here is by repository id, which is stable,
    /// total and reproducible across both byte orders and both servants —
    /// which is what a byte comparison needs — and is a recorded divergence
    /// rather than an accident. Restoring declaration order is a `Registry`
    /// change, not an `ifr.rs` one.
    fn contents(&self, target: &Target, limit_type: u32, exclude_inherited: bool) -> Vec<Target> {
        let mut out = Vec::new();
        match target {
            Target::Repository => {
                for (id, name) in self.module_ids() {
                    if !name.contains("::") {
                        out.push(Target::Module(id));
                    }
                }
                for id in self.registry.ids() {
                    if self.contained_of(id).1.is_empty() {
                        out.push(Target::Entry(id.clone()));
                    }
                }
            }
            Target::Module(module) => {
                for (id, name) in self.module_ids() {
                    if self.module_container_of(&id, &name).as_deref() == Some(module.as_str()) {
                        out.push(Target::Module(id));
                    }
                }
                for id in self.registry.ids() {
                    if self.contained_of(id).1 == *module {
                        out.push(Target::Entry(id.clone()));
                    }
                }
            }
            Target::Entry(id) => {
                // Nested definitions: `walk` recurses into an interface body,
                // and §14.5.10/§14.5.20 make a StructDef and an ExceptionDef
                // containers too.
                for other in self.registry.ids() {
                    if self.contained_of(other).1 == *id {
                        out.push(Target::Entry(other.clone()));
                    }
                }
                if self.registry.interface(id).is_some() {
                    let chain =
                        if exclude_inherited { vec![id.clone()] } else { self.declaring_chain(id) };
                    let mut seen: BTreeSet<String> = BTreeSet::new();
                    for owner in &chain {
                        let Some(iface) = self.registry.interface(owner) else { continue };
                        for name in iface.operations.keys() {
                            if seen.insert(name.clone()) {
                                out.push(Target::Operation {
                                    owner: owner.clone(),
                                    name: name.clone(),
                                });
                            }
                        }
                        for name in iface.attributes.keys() {
                            if seen.insert(name.clone()) {
                                out.push(Target::Attribute {
                                    owner: owner.clone(),
                                    name: name.clone(),
                                });
                            }
                        }
                    }
                }
            }
            // None is a `Container`; an operation contains nothing.
            Target::Operation { .. } | Target::Attribute { .. } | Target::Primitive(_) => {}
        }
        out.retain(|t| self.matches_limit(t, limit_type));
        out.sort_by_key(|t| t.id().unwrap_or_default());
        out
    }

    /// The module or entry a *module* is defined in, or `None` at file scope.
    ///
    /// Not [`Self::contained_of`], which reads a registered entry's recorded
    /// qualified name. A module has no entry, so its container comes from its
    /// own derived name: `a::b::c` is contained in `a::b`, whose id is this
    /// id's path minus its last segment.
    fn module_container_of(&self, id: &str, name: &str) -> Option<RepositoryId> {
        if !name.contains("::") {
            return None;
        }
        let (_, _, version) = split_repository_id(id);
        let path = id.strip_prefix("IDL:")?.rsplit_once(':').map(|(p, _)| p)?;
        let (parent, _) = path.rsplit_once('/')?;
        Some(format!("IDL:{parent}:{version}"))
    }

    /// Whether an object passes a `limit_type` filter.
    ///
    /// `dk_all` (1) is the wildcard and is the only value that is **not** a
    /// kind to compare against — a filter written as `kind == limit` alone
    /// would let `dk_all` match nothing, which is the opposite of what it
    /// means.
    fn matches_limit(&self, target: &Target, limit_type: u32) -> bool {
        limit_type == DefinitionKind::All as u32 || self.kind_of(target) as u32 == limit_type
    }

    /// The `DefinitionKind` of any object this facade serves.
    ///
    /// [`Self::def_kind`] answers for a registry entry; this answers for the
    /// three object kinds that are not entries as well, and is what
    /// `_get_def_kind`, `contents`' filter and `describe`'s `kind` member all
    /// read — one function, so a client cannot be told two different kinds for
    /// the same object by two different operations.
    fn kind_of(&self, target: &Target) -> DefinitionKind {
        match target {
            Target::Repository => DefinitionKind::Repository,
            Target::Module(_) => DefinitionKind::Module,
            Target::Entry(id) => self.def_kind(id),
            Target::Operation { .. } => DefinitionKind::Operation,
            Target::Attribute { .. } => DefinitionKind::Attribute,
            Target::Primitive(_) => DefinitionKind::Primitive,
        }
    }

    /// The three objects this facade serves as `Container`s, and the refusal
    /// for everything else.
    ///
    /// §14.5.10 and §14.5.20 also make a `StructDef` and an `ExceptionDef`
    /// containers — of nested `StructDef`s, `UnionDef`s and `EnumDef`s. This
    /// facade does **not** claim that, and the reason is consistency rather
    /// than effort: [`Self::is_a_ids`] answers `_is_a` false for
    /// `CORBA::Container` on a type entry, and a servant that refused the
    /// narrow and then honoured the operation would be telling a client two
    /// different things about the same reference. Widening it means widening
    /// both, in one commit.
    fn require_container(&self, target: &Target) -> std::result::Result<(), SystemException> {
        let ok = match target {
            Target::Repository | Target::Module(_) => true,
            Target::Entry(id) => self.registry.interface(id).is_some(),
            Target::Operation { .. } | Target::Attribute { .. } | Target::Primitive(_) => false,
        };
        if ok { Ok(()) } else { Err(SystemException::bad_operation()) }
    }

    /// `Contained::_get_absolute_name` (§14.5.3.1) for the objects that are not
    /// registry entries.
    ///
    /// The rule is recursive and the specification states it as such: *"if this
    /// object's `defined_in` attribute references a Repository, the
    /// absolute_name is formed by concatenating `::` and this object's name;
    /// otherwise … the absolute_name of the object referenced by `defined_in`,
    /// `::`, and this object's name."* A module's is its qualified name with a
    /// leading `::`; a member's is its interface's plus its own.
    fn absolute_name_of_target(&self, target: &Target) -> String {
        match target {
            Target::Repository | Target::Primitive(_) => String::new(),
            Target::Module(id) => match self.module_ids().get(id) {
                Some(qualified) => format!("::{qualified}"),
                None => absolute_name(id),
            },
            Target::Entry(id) => self.absolute_name_of(id),
            Target::Operation { owner, name } | Target::Attribute { owner, name } => {
                format!("{}::{name}", self.absolute_name_of(owner))
            }
        }
    }

    /// `Container::lookup(search_name)` (§14.5.4.1): a **scoped** name,
    /// resolved by IDL's name-scoping rules relative to this container.
    ///
    /// This is not `lookup_name` with one level. A leading `::` makes the name
    /// absolute and resolves it against the repository; otherwise the search
    /// starts in this container and walks **outward** through the enclosing
    /// scopes, which is what "IDL's name scoping rules" means and is the
    /// difference a client would otherwise have to reimplement.
    ///
    /// A name that resolves to nothing yields a nil reference rather than an
    /// exception — the specification says so explicitly.
    fn lookup(&self, target: &Target, search_name: &str) -> Option<Target> {
        let absolute = search_name.strip_prefix("::");
        let wanted = absolute.unwrap_or(search_name);
        if wanted.is_empty() {
            return None;
        }
        if absolute.is_some() {
            return self.resolve_qualified(wanted);
        }
        // Relative: this container's scope first, then each enclosing scope,
        // out to the repository.
        let mut scope = self.scope_of(target);
        loop {
            let candidate =
                if scope.is_empty() { wanted.to_owned() } else { format!("{scope}::{wanted}") };
            if let Some(found) = self.resolve_qualified(&candidate) {
                return Some(found);
            }
            match scope.rsplit_once("::") {
                Some((outer, _)) => scope = outer.to_owned(),
                None if scope.is_empty() => return None,
                None => scope = String::new(),
            }
        }
    }

    /// The qualified IDL name of the scope an object *introduces* — `bank` for
    /// module `bank`, `bank::Account` for interface `Account`, and the empty
    /// string for the repository and for anything that scopes nothing.
    fn scope_of(&self, target: &Target) -> String {
        match target {
            Target::Repository => String::new(),
            Target::Module(id) => self.module_ids().get(id).cloned().unwrap_or_default(),
            Target::Entry(id) => self.registry.qualified_name(id).unwrap_or_default().to_owned(),
            Target::Operation { .. } | Target::Attribute { .. } | Target::Primitive(_) => {
                String::new()
            }
        }
    }

    /// A fully qualified IDL name (`bank::Account::balance`) to the object it
    /// names, entries and members alike.
    /// A fully qualified IDL name (`bank::Account::balance`) to the object it
    /// names — entries, modules and members alike.
    ///
    /// The module case is tried second and not last, and the reason is a
    /// measured one: `Registry::load` **removes** a module's qualified name
    /// from `by_name` after walking into it, so `id_of("gc10")` is `None` and a
    /// resolver that only fell back to a member decomposition answered nil for
    /// `lookup("gc10")` — a top-level module in a repository that plainly
    /// contains it. Found by pointing omniORB's IR client at this facade on
    /// 2026-08-25; no in-tree test had asked for a module by name.
    fn resolve_qualified(&self, qualified: &str) -> Option<Target> {
        if let Some(id) = self.registry.id_of(qualified) {
            return self.object_for(id);
        }
        if let Some((id, _)) = self.module_ids().into_iter().find(|(_, n)| n == qualified) {
            return Some(Target::Module(id));
        }
        // Not an entry and not a module: it may be an operation or attribute,
        // whose qualified name is its interface's plus its own.
        let (owner_name, member) = qualified.rsplit_once("::")?;
        let owner = self.registry.id_of(owner_name)?.clone();
        let iface = self.registry.interface(&owner)?;
        if iface.operations.contains_key(member) {
            Some(Target::Operation { owner, name: member.to_owned() })
        } else if iface.attributes.contains_key(member) {
            Some(Target::Attribute { owner, name: member.to_owned() })
        } else {
            None
        }
    }

    /// `Container::lookup_name(search_name, levels_to_search, limit_type,
    /// exclude_inherited)` (§14.5.4.1): a **simple** name, searched for.
    ///
    /// The difference from [`Self::lookup`] is the whole point of there being
    /// two operations. `lookup` takes a scoped name and resolves it outward
    /// through enclosing scopes, returning at most one object. `lookup_name`
    /// takes an unqualified identifier and searches *inward*, returning every
    /// object of that name it finds — which is why it returns a sequence.
    ///
    /// `levels_to_search` is 1 for this container only and -1 for this
    /// container and everything below it. §14.5.4.1 says every other value —
    /// 0, or a negative other than -1 — is **undefined**; this facade reads a
    /// positive n as n levels and refuses 0 and negatives other than -1 with
    /// `BAD_PARAM`, because answering something arbitrary for an undefined
    /// input is how an undefined input becomes a compatibility promise.
    fn lookup_name(
        &self,
        target: &Target,
        search_name: &str,
        levels_to_search: i32,
        limit_type: u32,
        exclude_inherited: bool,
    ) -> std::result::Result<Vec<Target>, SystemException> {
        if levels_to_search == 0 || levels_to_search < -1 {
            return Err(bad_param());
        }
        let mut out = Vec::new();
        self.lookup_name_into(
            target,
            search_name,
            levels_to_search,
            limit_type,
            exclude_inherited,
            &mut out,
        );
        out.sort_by_key(|t| t.id().unwrap_or_default());
        out.dedup();
        Ok(out)
    }

    fn lookup_name_into(
        &self,
        target: &Target,
        search_name: &str,
        levels: i32,
        limit_type: u32,
        exclude_inherited: bool,
        out: &mut Vec<Target>,
    ) {
        for found in self.contents(target, DefinitionKind::All as u32, exclude_inherited) {
            if self.name_of_target(&found) == search_name && self.matches_limit(&found, limit_type)
            {
                out.push(found.clone());
            }
            if levels == -1 || levels > 1 {
                let deeper = if levels == -1 { -1 } else { levels - 1 };
                self.lookup_name_into(
                    &found,
                    search_name,
                    deeper,
                    limit_type,
                    exclude_inherited,
                    out,
                );
            }
        }
    }

    /// The unqualified name of any object this facade serves.
    fn name_of_target(&self, target: &Target) -> String {
        match target {
            Target::Repository | Target::Primitive(_) => String::new(),
            Target::Module(id) => self
                .module_ids()
                .get(id)
                .and_then(|n| n.rsplit("::").next().map(str::to_owned))
                .unwrap_or_else(|| split_repository_id(id).0),
            Target::Entry(id) => self.contained_of(id).0,
            Target::Operation { name, .. } | Target::Attribute { name, .. } => name.clone(),
        }
    }

    /// `Contained::_get_defined_in` (§14.5.3.1): the `Container` this object is
    /// defined in.
    ///
    /// For a member contained *through inheritance* the specification is
    /// explicit — it identifies the `InterfaceDef` the member is inherited
    /// **from**, not the one that inherited it — which is why
    /// [`Target::Operation`] carries its declaring owner rather than the
    /// interface a walk happened to reach it through.
    ///
    /// A top-level definition is defined in the repository itself, so the
    /// answer is the root reference and not a nil one: nil would say "there is
    /// no container", and there always is.
    fn defined_in_of(&self, target: &Target) -> Option<Target> {
        match target {
            Target::Repository => None,
            Target::Module(id) => {
                let name = self.module_ids().get(id).cloned().unwrap_or_default();
                match self.module_container_of(id, &name) {
                    Some(parent) => Some(Target::Module(parent)),
                    None => Some(Target::Repository),
                }
            }
            Target::Entry(id) => match self.contained_of(id).1 {
                container if container.is_empty() => Some(Target::Repository),
                container => self.object_for(&container).or(Some(Target::Repository)),
            },
            Target::Operation { owner, .. } | Target::Attribute { owner, .. } => {
                Some(Target::Entry(owner.clone()))
            }
            // Not `Contained` (§14.5.14): a primitive is owned by the
            // repository but defined in nothing.
            Target::Primitive(_) => None,
        }
    }

    /// `IDLType::_get_type` (§14.5.5): the `TypeCode` describing the type an
    /// object defines, or `BAD_OPERATION` if it does not define one.
    ///
    /// An `InterfaceDef` is an `IDLType` and answers its own `tk_objref`; a
    /// `TypedefDef` answers the type it names. A `ConstantDef` is **not** an
    /// `IDLType` and neither is a `ModuleDef`, an `OperationDef` or an
    /// `AttributeDef` — the same narrowing [`Self::is_a_ids`] already makes,
    /// read here so the two cannot disagree.
    fn type_of(&self, target: &Target) -> std::result::Result<TypeCode, SystemException> {
        match target {
            Target::Entry(id) => match self.registry.get(id) {
                Some(Entry::Interface(_)) => {
                    Ok(TypeCode::ObjRef { id: id.clone(), name: self.contained_of(id).0 })
                }
                Some(Entry::Type(tc)) => Ok(tc.clone()),
                Some(Entry::Const { .. }) | None => Err(SystemException::bad_operation()),
            },
            // `PrimitiveDef : IDLType` (§14.5.14) — the one non-entry object
            // here that has a type, and answering it is what makes
            // `get_primitive`'s reference worth handing out.
            Target::Primitive(kind) => {
                primitive_typecode(*kind).ok_or_else(SystemException::bad_operation)
            }
            Target::Repository
            | Target::Module(_)
            | Target::Operation { .. }
            | Target::Attribute { .. } => Err(SystemException::bad_operation()),
        }
    }

    /// `Contained::describe()` (§14.5.3.1): `Description { kind, value }`,
    /// where `value` is an `any` carrying the description struct for the
    /// object's **most derived** type.
    ///
    /// The specification is unusually emphatic about `kind`: *"The kind field
    /// of the returned Description struct shall give the DefinitionKind for the
    /// most derived type of the object … returning dk_all would be an error."*
    /// So it is [`Self::kind_of`] — the same function `_get_def_kind` and
    /// `contents`' filter read — and never a broader one.
    ///
    /// The `any` is where this operation meets the wire. Its `TypeCode` comes
    /// from [`description_tc`] and its value is written straight after, in the
    /// same stream; a member name or order that differs from a peer's stub
    /// makes the client's extraction fail, or succeed against the wrong layout.
    ///
    /// The repository itself has no `describe`: `Repository` derives from
    /// `Container` and not from `Contained` (§14.5.6), so asking is
    /// `BAD_OPERATION` and not an empty answer.
    fn describe(
        &self,
        target: &Target,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        out.put_u32(self.kind_of(target) as u32);
        self.put_description_any(target, out)
    }

    /// The `any` half of a description — used by `describe` and, with the
    /// object reference in front of it, by `describe_contents`.
    fn put_description_any(
        &self,
        target: &Target,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        let marshal = |_| SystemException::marshal();
        match target {
            // Neither derives from `Contained`, so neither has a `describe`.
            Target::Repository | Target::Primitive(_) => Err(SystemException::bad_operation()),
            Target::Module(id) => {
                let name = self.name_of_target(target);
                let module_name = self.module_ids().get(id).cloned().unwrap_or_default();
                let (_, _, version) = split_repository_id(id);
                let desc = ModuleDescription {
                    name,
                    id: id.clone(),
                    defined_in: self.module_container_of(id, &module_name).unwrap_or_default(),
                    version,
                };
                put_any(out, &description_tc::module_description(), |e| desc.write_to(e))
                    .map_err(marshal)
            }
            Target::Operation { owner, name } => {
                let desc = self.operation_description(owner, name)?;
                put_any(out, &description_tc::operation_description(), |e| desc.write_to(e))
                    .map_err(marshal)
            }
            Target::Attribute { owner, name } => {
                let desc = self.attribute_description(owner, name)?;
                put_any(out, &description_tc::attribute_description(), |e| desc.write_to(e))
                    .map_err(marshal)
            }
            Target::Entry(id) => {
                let (name, defined_in, version) = self.contained_of(id);
                match self.registry.get(id) {
                    None => Err(SystemException::object_not_exist()),
                    // §14.5.24 / §14.5.26: an `InterfaceDef` and an
                    // `AbstractInterfaceDef` both describe as an
                    // `InterfaceDescription`. Five members — not
                    // `FullInterfaceDescription`.
                    Some(Entry::Interface(iface)) => {
                        let desc = InterfaceDescription {
                            name,
                            id: id.clone(),
                            defined_in,
                            version,
                            base_interfaces: iface.bases.clone(),
                        };
                        put_any(out, &description_tc::interface_description(), |e| desc.write_to(e))
                            .map_err(marshal)
                    }
                    Some(Entry::Const { tc, value }) => {
                        let tc = tc.clone();
                        let value = value.clone();
                        put_any(out, &description_tc::constant_description(), |e| {
                            e.put_str(&name);
                            e.put_str(id);
                            e.put_str(&defined_in);
                            e.put_str(&version);
                            typecode::encode(e, &tc)?;
                            // The `value` member is itself an `any`. A constant
                            // the registry could not fold, or one whose type
                            // this wire cannot carry, is written as an `any` of
                            // `tk_void` — which is a value, and says "there is
                            // no value here" — rather than as a guess at one.
                            match &value {
                                Some(v) => {
                                    let mut probe = Encoder::new(e.endian());
                                    let writable = put_const_value(&mut probe, &tc, v)?;
                                    if writable {
                                        put_any(e, &tc, |e| {
                                            put_const_value(e, &tc, v).map(|_| ())
                                        })?;
                                    } else {
                                        typecode::encode(e, &TypeCode::Void)?;
                                    }
                                }
                                None => typecode::encode(e, &TypeCode::Void)?,
                            }
                            Ok(())
                        })
                        .map_err(marshal)
                    }
                    Some(Entry::Type(tc)) => match tc {
                        // §14.5.31: a `ValueDef` describes as a
                        // `ValueDescription`, which is a different struct with
                        // ten members and two booleans in the middle.
                        TypeCode::Value { modifier, base, .. } => {
                            let desc = ValueDescription {
                                name,
                                id: id.clone(),
                                is_abstract: *modifier == 2,
                                is_custom: *modifier == 1,
                                defined_in,
                                version,
                                supported_interfaces: Vec::new(),
                                abstract_base_values: Vec::new(),
                                is_truncatable: *modifier == 3,
                                base_value: base
                                    .as_deref()
                                    .and_then(typecode_id)
                                    .unwrap_or_default()
                                    .to_owned(),
                            };
                            put_any(out, &description_tc::value_description(), |e| desc.write_to(e))
                                .map_err(marshal)
                        }
                        // §14.5.20: an `ExceptionDef` describes as an
                        // `ExceptionDescription` — same members as a
                        // `TypeDescription`, different repository id, so a
                        // client's extraction picks the right one.
                        TypeCode::Except { .. } => {
                            let desc = self.exception_description(id);
                            put_any(out, &description_tc::exception_description(), |e| {
                                desc.write_to(e)
                            })
                            .map_err(marshal)
                        }
                        // Everything else registered as a type is a
                        // `TypedefDef` or a `NativeDef`, and both describe as a
                        // `TypeDescription` (§14.5.9, §14.5.34).
                        other => {
                            let desc = TypeDescription {
                                name,
                                id: id.clone(),
                                defined_in,
                                version,
                                tc: other.clone(),
                            };
                            put_any(out, &description_tc::type_description(), |e| desc.write_to(e))
                                .map_err(marshal)
                        }
                    },
                }
            }
        }
    }

    /// One `OperationDescription`, for an operation reached as an object.
    ///
    /// Built from the same fields `describe_interface` reads, so an operation
    /// described twice — once inside a `FullInterfaceDescription` and once
    /// through its own `OperationDef` — cannot come back as two different
    /// things.
    fn operation_description(
        &self,
        owner: &str,
        name: &str,
    ) -> std::result::Result<OperationDescription, SystemException> {
        let iface = self.registry.interface(owner).ok_or_else(SystemException::object_not_exist)?;
        let sig = iface.operations.get(name).ok_or_else(SystemException::object_not_exist)?;
        let (_, _, version) = split_repository_id(owner);
        Ok(OperationDescription {
            name: name.to_owned(),
            id: member_id(owner, name),
            defined_in: owner.to_owned(),
            version,
            result: sig.returns.clone(),
            mode: if sig.oneway { OP_ONEWAY } else { OP_NORMAL },
            contexts: Vec::new(),
            parameters: sig
                .params
                .iter()
                .map(|p| ParameterDescription {
                    name: p.name.clone(),
                    tc: p.tc.clone(),
                    mode: match p.direction {
                        ParamDirection::In => PARAM_IN,
                        ParamDirection::Out => PARAM_OUT,
                        ParamDirection::InOut => PARAM_INOUT,
                    },
                })
                .collect(),
            exceptions: sig.raises.iter().map(|x| self.exception_description(x)).collect(),
        })
    }

    /// One `AttributeDescription`, on the same terms.
    fn attribute_description(
        &self,
        owner: &str,
        name: &str,
    ) -> std::result::Result<AttributeDescription, SystemException> {
        let iface = self.registry.interface(owner).ok_or_else(SystemException::object_not_exist)?;
        let sig = iface.attributes.get(name).ok_or_else(SystemException::object_not_exist)?;
        let (_, _, version) = split_repository_id(owner);
        Ok(AttributeDescription {
            name: name.to_owned(),
            id: member_id(owner, name),
            defined_in: owner.to_owned(),
            version,
            tc: sig.tc.clone(),
            mode: if sig.readonly { ATTR_READONLY } else { ATTR_NORMAL },
        })
    }

    /// `Repository::get_canonical_typecode(tc)` (§14.5.6.1).
    ///
    /// *"Looks up the `TypeCode` in the Interface Repository and returns an
    /// equivalent `TypeCode` that includes all repository ids, names, and
    /// member_names. If the top level `TypeCode` does not contain a
    /// RepositoryId … or if it contains a RepositoryId that is not found in
    /// the target Repository, then a new `TypeCode` is constructed by
    /// recursively calling `get_canonical_typecode` on each member."*
    ///
    /// Implemented as exactly that sentence: a named `TypeCode` whose id the
    /// registry holds is **replaced** by the registry's own — which is what
    /// fills in the names a peer sent stripped — and everything else is
    /// rebuilt by recursion. A `TypeCode` the repository knows nothing about
    /// comes back canonicalised as deeply as it can be and no deeper, rather
    /// than refused: the operation's contract is "an equivalent TypeCode", and
    /// the input already is one.
    fn canonical_typecode(&self, tc: &TypeCode) -> TypeCode {
        if let Some(id) = typecode_id(tc) {
            if let Some(known) = self.registry.typecode(id) {
                return known.clone();
            }
            // An interface entry has no `Entry::Type`, and a `tk_objref`
            // naming one is already canonical — its id and name are all there
            // is to fill in.
            if self.registry.interface(id).is_some() {
                return tc.clone();
            }
        }
        match tc {
            TypeCode::Sequence { element, bound } => TypeCode::Sequence {
                element: Box::new(self.canonical_typecode(element)),
                bound: *bound,
            },
            TypeCode::Array { element, length } => TypeCode::Array {
                element: Box::new(self.canonical_typecode(element)),
                length: *length,
            },
            TypeCode::Alias { id, name, aliased } => TypeCode::Alias {
                id: id.clone(),
                name: name.clone(),
                aliased: Box::new(self.canonical_typecode(aliased)),
            },
            other => other.clone(),
        }
    }

    fn handle(&self, req: &Request, out: &mut Encoder) -> std::result::Result<(), SystemException> {
        // Refusal comes before target resolution on purpose: `create_module`
        // against a key we never derived must answer NO_PERMISSION, not
        // OBJECT_NOT_EXIST. The second reads as "try a different reference",
        // which is exactly the retry the policy exists to stop.
        if is_mutating(&req.operation) {
            return Err(refused());
        }
        let target = self.target(&req.object_key)?;
        let mut args = req.body().map_err(|_| SystemException::marshal())?;

        match (&target, req.operation.as_str()) {
            (_, "_is_a") => {
                let asked = args.get_string().map_err(|_| SystemException::marshal())?;
                out.put_bool(self.is_a_ids(&target).contains(&asked.as_str()));
            }
            (_, "_non_existent") => out.put_bool(false),
            // `IRObject::_get_def_kind` for every object, from one function —
            // so `describe`'s `kind`, `contents`' filter and this cannot
            // disagree about the same object.
            (_, "_get_def_kind") => out.put_u32(self.kind_of(&target) as u32),

            // ── Container (§14.5.4), served since 2026-08-25 ──
            (Target::Repository | Target::Module(_) | Target::Entry(_), "contents") => {
                let limit = args.get_u32().map_err(|_| SystemException::marshal())?;
                let exclude = args.get_bool().map_err(|_| SystemException::marshal())?;
                self.require_container(&target)?;
                let found = self.contents(&target, limit, exclude);
                out.put_u32(found.len() as u32);
                for t in &found {
                    self.reference_for(t).write_to(out).map_err(|_| SystemException::marshal())?;
                }
            }
            (Target::Repository | Target::Module(_) | Target::Entry(_), "lookup") => {
                let name = args.get_string().map_err(|_| SystemException::marshal())?;
                self.require_container(&target)?;
                let found = self.lookup(&target, &name);
                match found {
                    Some(t) => self.reference_for(&t),
                    None => nil_ref(),
                }
                .write_to(out)
                .map_err(|_| SystemException::marshal())?;
            }
            (Target::Repository | Target::Module(_) | Target::Entry(_), "lookup_name") => {
                let name = args.get_string().map_err(|_| SystemException::marshal())?;
                let levels = args.get_i32().map_err(|_| SystemException::marshal())?;
                let limit = args.get_u32().map_err(|_| SystemException::marshal())?;
                let exclude = args.get_bool().map_err(|_| SystemException::marshal())?;
                self.require_container(&target)?;
                let found = self.lookup_name(&target, &name, levels, limit, exclude)?;
                out.put_u32(found.len() as u32);
                for t in &found {
                    self.reference_for(t).write_to(out).map_err(|_| SystemException::marshal())?;
                }
            }
            (Target::Repository | Target::Module(_) | Target::Entry(_), "describe_contents") => {
                let limit = args.get_u32().map_err(|_| SystemException::marshal())?;
                let exclude = args.get_bool().map_err(|_| SystemException::marshal())?;
                let max = args.get_i32().map_err(|_| SystemException::marshal())?;
                self.require_container(&target)?;
                let mut found = self.contents(&target, limit, exclude);
                // §14.5.4.1: "-1 means return all contained objects". A
                // positive n truncates; 0 is a legal request for nothing.
                if max >= 0 {
                    found.truncate(max as usize);
                }
                out.put_u32(found.len() as u32);
                for t in &found {
                    // `Container::Description` is the `Contained::Description`
                    // with the object reference in front of it (§14.5.4), and
                    // that reference is what makes `describe_contents` worth
                    // one round trip instead of n+1.
                    self.reference_for(t).write_to(out).map_err(|_| SystemException::marshal())?;
                    out.put_u32(self.kind_of(t) as u32);
                    self.put_description_any(t, out)?;
                }
            }

            // ── Contained (§14.5.3) ──
            (_, "describe") => self.describe(&target, out)?,
            (_, "_get_defined_in") => {
                let container =
                    self.defined_in_of(&target).ok_or_else(SystemException::bad_operation)?;
                self.reference_for(&container)
                    .write_to(out)
                    .map_err(|_| SystemException::marshal())?;
            }
            (_, "_get_containing_repository") => {
                // Every object except the root is "eventually reached by
                // recursively following defined_in" (§14.5.3.1), and this
                // facade serves exactly one repository, so the recursion has
                // one answer. The root itself is not `Contained`.
                if matches!(target, Target::Repository) {
                    return Err(SystemException::bad_operation());
                }
                self.root_ior().write_to(out).map_err(|_| SystemException::marshal())?;
            }

            // ── IDLType (§14.5.5) ──
            (_, "_get_type") => {
                let tc = self.type_of(&target)?;
                typecode::encode(out, &tc).map_err(|_| SystemException::marshal())?;
            }

            // ── Repository (§14.5.6) ──
            (Target::Repository, "lookup_id") => {
                let asked = args.get_string().map_err(|_| SystemException::marshal())?;
                self.entry_ior(&asked).write_to(out).map_err(|_| SystemException::marshal())?;
            }
            (Target::Repository, "get_canonical_typecode") => {
                let tc = typecode::decode(&mut args).map_err(|_| SystemException::marshal())?;
                typecode::encode(out, &self.canonical_typecode(&tc))
                    .map_err(|_| SystemException::marshal())?;
            }
            (Target::Repository, "get_primitive") => {
                let kind = args.get_u32().map_err(|_| SystemException::marshal())?;
                // §14.5.14: "there are no PrimitiveDefs with kind pk_null", so
                // a nil reference is the answer and not an exception. A kind
                // outside the enum, or `pk_value_base` — which this workspace
                // has no `TypeCode` for — is `BAD_PARAM`: a nil there would
                // read as "this repository has no such primitive", which is a
                // different and false claim.
                if kind == pk::NULL {
                    nil_ref().write_to(out).map_err(|_| SystemException::marshal())?;
                } else if primitive_typecode(kind).is_some() {
                    self.reference_for(&Target::Primitive(kind))
                        .write_to(out)
                        .map_err(|_| SystemException::marshal())?;
                } else {
                    return Err(bad_param());
                }
            }
            (Target::Primitive(kind), "_get_kind") => out.put_u32(*kind),

            (_, "_get_id") => out.put_str(&target.id().ok_or_else(SystemException::bad_operation)?),
            (_, "_get_name") => out.put_str(&self.name_of_target(&target)),
            (Target::Entry(id), "_get_absolute_name") => out.put_str(&self.absolute_name_of(id)),
            (
                Target::Module(_) | Target::Operation { .. } | Target::Attribute { .. },
                "_get_absolute_name",
            ) => {
                out.put_str(&self.absolute_name_of_target(&target));
            }
            // The read half of `version`. The write half is `_set_version`,
            // refused `NO_PERMISSION` above with every other mutator, and the
            // two answers have to agree that the operation exists at all.
            (_, "_get_version") => {
                let id = target.id().ok_or_else(SystemException::bad_operation)?;
                out.put_str(&split_repository_id(&id).2);
            }

            // ── AttributeDef and OperationDef read attributes (§14.5.21, §14.5.23) ──
            (Target::Attribute { owner, name }, "_get_mode") => {
                out.put_u32(self.attribute_description(owner, name)?.mode);
            }
            (Target::Operation { owner, name }, "_get_mode") => {
                out.put_u32(self.operation_description(owner, name)?.mode);
            }
            (Target::Operation { owner, name }, "_get_result") => {
                let desc = self.operation_description(owner, name)?;
                typecode::encode(out, &desc.result).map_err(|_| SystemException::marshal())?;
            }
            (Target::Operation { owner, name }, "_get_params") => {
                let desc = self.operation_description(owner, name)?;
                out.put_u32(desc.parameters.len() as u32);
                for p in &desc.parameters {
                    p.write_to(out).map_err(|_| SystemException::marshal())?;
                }
            }
            (Target::Operation { owner, name }, "_get_contexts") => {
                let _ = self.operation_description(owner, name)?;
                // The front end's AST carries no `context` clause, so there is
                // nothing to report. An empty sequence is the answer, not a
                // refusal: the operation genuinely declares no contexts.
                out.put_u32(0);
            }
            (Target::Operation { owner, name }, "_get_exceptions") => {
                let desc = self.operation_description(owner, name)?;
                out.put_u32(desc.exceptions.len() as u32);
                for x in &desc.exceptions {
                    // `ExceptionDefSeq`: references, not descriptions.
                    self.entry_ior(&x.id).write_to(out).map_err(|_| SystemException::marshal())?;
                }
            }
            (Target::Entry(id), "describe_interface") => {
                self.describe_interface(id)?
                    .write_to(out)
                    .map_err(|_| SystemException::marshal())?;
            }
            (Target::Entry(id), "_get_base_interfaces") => {
                let Some(iface) = self.registry.interface(id) else {
                    return Err(SystemException::bad_operation());
                };
                let bases = iface.bases.clone();
                out.put_u32(bases.len() as u32);
                for b in &bases {
                    self.entry_ior(b).write_to(out).map_err(|_| SystemException::marshal())?;
                }
            }
            (Target::Entry(id), "is_a") => {
                // `is_a` is the IR operation — "does the interface I describe
                // derive from this repository id" — and is a different
                // question from `_is_a`, which asks what *this reference* is.
                // An InterfaceDef for `gc10::Both` answers `is_a` true for
                // `IDL:gc10/Base:1.0` and `_is_a` false for it.
                if self.registry.interface(id).is_none() {
                    return Err(SystemException::bad_operation());
                }
                let asked = args.get_string().map_err(|_| SystemException::marshal())?;
                out.put_bool(self.registry.is_a(id, &asked));
            }
            // A deferral and an oversight are different facts, so they get
            // different answers. Unlike the mutating refusal this happens
            // *after* target resolution on purpose: a stale reference should
            // hear that its object is gone (`OBJECT_NOT_EXIST`, actionable)
            // rather than a policy statement about an operation it will never
            // reach — and no client retries a deferral elsewhere, which is the
            // retry the earlier ordering exists to stop.
            _ if is_deferred(&req.operation) => return Err(not_implemented()),
            _ => return Err(SystemException::bad_operation()),
        }
        Ok(())
    }
}

impl SharedDispatch for RepositoryServer {
    /// The root key and every key this facade mints a reference for.
    ///
    /// It used to say "every key derived from a registered repository id",
    /// which was the same thing while the only objects were entries. Since the
    /// containment walk landed, a walk hands out references to modules,
    /// operations, attributes and primitives too — and a reference this servant
    /// mints and then does not claim is a reference a client cannot dial.
    /// [`RepositoryServer::object_for`] is the one function that decides, so
    /// the two cannot drift apart.
    fn knows(&self, object_key: &[u8]) -> bool {
        object_key == self.root.as_slice()
            || self.id_from_key(object_key).is_some_and(|id| self.object_for(id).is_some())
    }

    /// No `dispatch_body` override, deliberately.
    ///
    /// F6's naming servant needed one because CosNaming declares user
    /// exceptions; the read-only IR subset declares none — `lookup_id`
    /// returns nil rather than raising, and `describe_interface` and `is_a`
    /// have no `raises` clause. Every refusal here is therefore a *system*
    /// exception, so the trait's default `dispatch_body` is already correct
    /// and an override would add a branch nothing can reach.
    ///
    /// Nor is there a `serve_one` override: `knows` and `dispatch` read state
    /// that cannot change between them, so there is nothing for a lock to make
    /// atomic.
    fn dispatch(
        &self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        self.handle(request, out)
    }
}

/// The `&mut self` shape too, forwarding, so a caller already written against
/// [`Server::serve`](orbweaver_giop::server::Server::serve) keeps working.
impl Dispatch for RepositoryServer {
    fn knows(&self, object_key: &[u8]) -> bool {
        SharedDispatch::knows(self, object_key)
    }

    fn dispatch(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        SharedDispatch::dispatch(self, request, out)
    }
}

/// Convenience for a client: decodes a `FullInterfaceDescription` reply body.
pub fn decode_full_interface_description(d: &mut Decoder<'_>) -> Result<FullInterfaceDescription> {
    FullInterfaceDescription::read_from(d)
}

/// Convenience for a client: decodes a `lookup_id` reply body.
pub fn decode_object_reference(d: &mut Decoder<'_>) -> Result<Ior> {
    Ior::read_from(d)
}

/// A `Registry` built from IDL source, for callers assembling a facade.
///
/// Exists so a spike or a harness need not repeat the parse-then-load dance,
/// and so the failure is one error type rather than two.
pub fn registry_from_idl(source: &str) -> std::result::Result<Registry, crate::RegistryError> {
    let spec = orbweaver_idl::parse(source)
        .map_err(|e| crate::RegistryError { message: e.to_string() })?;
    let mut registry = Registry::new();
    registry.load(&spec)?;
    Ok(registry)
}

/// The interface repository ids in a registry, for a caller that wants to
/// advertise what it serves.
pub fn interface_ids(registry: &Registry) -> Vec<RepositoryId> {
    registry.ids().filter(|id| registry.interface(id).is_some()).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_cdr::Endian;
    use orbweaver_giop::server::{BAD_OPERATION, OBJECT_NOT_EXIST, Server};
    use orbweaver_giop::{Connection, Error};
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    const T: Duration = Duration::from_secs(5);
    const ROOT: &[u8] = b"InterfaceRepository";

    const IDL: &str = "
        module gc10 {
          interface Base      { readonly attribute string id; };
          interface Nameable  { attribute string name; };
          interface Derived : Base { long value(); };
          interface Both : Derived, Nameable { void touch(); };
          struct Payload { long bits; };
          exception Denied { string why; };
          interface Guarded {
            oneway void fire(in string topic);
            long adjust(in long delta, out Payload snapshot) raises (Denied);
          };
        };
    ";

    fn facade(port: u16) -> RepositoryServer {
        let registry = registry_from_idl(IDL).expect("golden-shaped IDL loads");
        RepositoryServer::new("127.0.0.1", port, ROOT.to_vec(), registry)
    }

    /// A facade served on loopback. Tests dial sequentially and shut down
    /// with the last client still open — the F6 pattern. Sequential is now a
    /// choice rather than a constraint: since stream E the server accepts
    /// concurrent connections, and these tests have nothing to learn from
    /// overlapping them — except
    /// `concurrent_clients_walk_the_repository_at_once`, which does.
    struct Served {
        server: RepositoryServer,
        root: Ior,
        stats: orbweaver_giop::server::ServerStats,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Served {
        fn start() -> Self {
            let server = Server::bind("127.0.0.1:0", ROOT.to_vec()).unwrap();
            let port = server.local_addr().unwrap().port();
            let ifr = Arc::new(facade(port));
            let root = ifr.root_ior();
            let handle = (*ifr).clone();
            let stats = server.stats();
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let thread = std::thread::spawn(move || {
                server.serve_shared(&*ifr, || flag.load(Ordering::SeqCst)).unwrap();
            });
            Served { server: handle, root, stats, stop, thread: Some(thread) }
        }

        fn repository(&self) -> Connection {
            Connection::connect(&self.root, T).unwrap()
        }

        fn entry(&self, id: &str) -> Connection {
            Connection::connect(&self.server.entry_ior(id), T).unwrap()
        }

        fn shutdown(mut self, last: Connection) {
            self.stop.store(true, Ordering::SeqCst);
            drop(last);
            self.thread.take().unwrap().join().unwrap();
        }
    }

    /// The facade locks nothing, so N clients walk it at once — including
    /// through `describe_interface`, which is the expensive operation here
    /// because it assembles every inherited operation and attribute.
    ///
    /// Two things are asserted, and the second is the one that needs the
    /// lock-free shape: every client gets **exactly** the answer a
    /// single-threaded client gets (no torn state), and the server's
    /// `peak_at_servant` reaches N. That counter is a weak witness in general
    /// — it counts callers waiting for a servant's lock as well as the one
    /// holding it — but for a servant with **no lock at all** there is nothing
    /// to wait on, so N inside `serve_one` is N executing. This is the one
    /// servant in the batch where that shortcut is sound, which is worth
    /// stating in the place a reader might copy it from.
    #[test]
    fn concurrent_clients_walk_the_repository_at_once() {
        const N: usize = 6;
        const EACH: usize = 3;
        let served = Served::start();
        let want = described("IDL:gc10/Both:1.0");

        std::thread::scope(|scope| {
            for i in 0..N {
                let served = &served;
                let want = &want;
                scope.spawn(move || {
                    for _ in 0..EACH {
                        let mut entry = served.entry("IDL:gc10/Both:1.0");
                        let reply = entry.invoke_nullary("describe_interface").unwrap();
                        let got =
                            decode_full_interface_description(&mut reply.body().unwrap()).unwrap();
                        assert_eq!(&got, want, "client {i} saw a different repository");
                    }
                });
            }
        });

        assert!(
            served.stats.peak_active() >= 2,
            "the clients never overlapped: peak was {}",
            served.stats.peak_active()
        );
        let last = served.repository();
        served.shutdown(last);
    }

    fn described(id: &str) -> FullInterfaceDescription {
        facade(1).describe_interface(id).expect("an interface")
    }

    /// Attribute names to modes, so a test can assert the shape without
    /// pinning the order of the whole description.
    fn attribute_modes(d: &FullInterfaceDescription) -> BTreeMap<&str, u32> {
        d.attributes.iter().map(|a| (a.name.as_str(), a.mode)).collect()
    }

    // ── object keys ─────────────────────────────────────────────────────────

    /// The scheme's contract: derived, reversible, distinct per entry, and
    /// closed against keys we did not derive.
    #[test]
    fn object_keys_derive_reversibly_from_the_repository_id() {
        let ifr = facade(1);
        let both = ifr.entry_key("IDL:gc10/Both:1.0");
        assert_eq!(both, b"InterfaceRepository/ifr/IDL:gc10/Both:1.0".to_vec());
        assert_eq!(ifr.id_from_key(&both), Some("IDL:gc10/Both:1.0"), "reversible");
        assert_ne!(both, ifr.entry_key("IDL:gc10/Base:1.0"), "one key per entry");

        // Deterministic: a second server over the same registry derives the
        // same key, which is why a stored reference survives a restart.
        assert_eq!(both, facade(2).entry_key("IDL:gc10/Both:1.0"));

        assert_eq!(ifr.id_from_key(ROOT), None, "the root is not an entry");
        assert_eq!(ifr.id_from_key(b"Elsewhere/ifr/IDL:gc10/Both:1.0"), None, "wrong root");

        assert!(SharedDispatch::knows(&ifr, ROOT));
        assert!(SharedDispatch::knows(&ifr, &both));
        assert!(
            !SharedDispatch::knows(&ifr, &ifr.entry_key("IDL:gc10/Absent:1.0")),
            "unregistered id"
        );
    }

    // ── CDR round trip ──────────────────────────────────────────────────────

    /// The description a DII client decodes, written and read back in both
    /// byte orders. An encoder that only works native-endian passes every
    /// local test and fails in the field.
    #[test]
    fn full_interface_description_round_trips_in_both_byte_orders() {
        for source in ["IDL:gc10/Both:1.0", "IDL:gc10/Guarded:1.0"] {
            let original = described(source);
            for endian in [Endian::Big, Endian::Little] {
                let mut e = Encoder::new(endian);
                original.write_to(&mut e).unwrap();
                let bytes = e.finish().unwrap();
                let mut d = Decoder::new(&bytes, endian);
                let back = FullInterfaceDescription::read_from(&mut d).unwrap();
                assert_eq!(back, original, "{source} {endian:?}");
                assert_eq!(d.remaining(), 0, "{source} {endian:?}: trailing bytes");
            }
        }
    }

    /// Not the same test at a different offset: a struct written after an odd
    /// number of bytes has every internal alignment shifted, which is the
    /// class of bug a round trip from offset zero cannot see.
    #[test]
    fn the_description_survives_a_shifted_alignment_origin() {
        let original = described("IDL:gc10/Guarded:1.0");
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            e.put_u8(0xAB);
            original.write_to(&mut e).unwrap();
            let bytes = e.finish().unwrap();
            let mut d = Decoder::new(&bytes, endian);
            assert_eq!(d.get_u8().unwrap(), 0xAB);
            assert_eq!(FullInterfaceDescription::read_from(&mut d).unwrap(), original);
        }
    }

    /// The members a DII client builds calls from: directions, oneway,
    /// raises, and the declaring interface of each inherited member.
    #[test]
    fn the_description_carries_signatures_directions_and_raises() {
        let g = described("IDL:gc10/Guarded:1.0");
        assert_eq!(g.name, "Guarded");
        assert_eq!(g.defined_in, "IDL:gc10:1.0", "the containing module");
        assert_eq!(g.version, "1.0");
        assert_eq!(g.tc, TypeCode::ObjRef { id: g.id.clone(), name: "Guarded".into() });
        assert!(g.base_interfaces.is_empty());

        let adjust = g.operations.iter().find(|o| o.name == "adjust").unwrap();
        assert_eq!(adjust.id, "IDL:gc10/Guarded/adjust:1.0");
        assert_eq!(adjust.defined_in, "IDL:gc10/Guarded:1.0");
        assert_eq!(adjust.mode, OP_NORMAL);
        assert_eq!(adjust.result, TypeCode::Long);
        let modes: Vec<(&str, u32)> =
            adjust.parameters.iter().map(|p| (p.name.as_str(), p.mode)).collect();
        assert_eq!(modes, [("delta", PARAM_IN), ("snapshot", PARAM_OUT)]);
        assert_eq!(adjust.exceptions.len(), 1);
        assert_eq!(adjust.exceptions[0].id, "IDL:gc10/Denied:1.0");
        assert_eq!(adjust.exceptions[0].name, "Denied");
        assert!(
            matches!(&adjust.exceptions[0].tc, TypeCode::Except { members, .. } if members.len() == 1),
            "the exception's TypeCode comes from the registry, not a stub"
        );

        let fire = g.operations.iter().find(|o| o.name == "fire").unwrap();
        assert_eq!(fire.mode, OP_ONEWAY);
        assert_eq!(fire.result, TypeCode::Void);
    }

    /// Inherited members are reported, because the consumer is a client
    /// asking what it may call — and each one names its declaring interface,
    /// so nothing is lost by including them.
    #[test]
    fn inherited_operations_and_attributes_are_reported_with_their_declarer() {
        let both = described("IDL:gc10/Both:1.0");
        let ops: Vec<&str> = both.operations.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(ops, ["touch", "value"], "own first, then inherited");
        assert_eq!(both.operations[0].defined_in, "IDL:gc10/Both:1.0");
        assert_eq!(both.operations[1].defined_in, "IDL:gc10/Derived:1.0", "declarer, not Both");

        let attrs = attribute_modes(&both);
        assert_eq!(attrs[&"id"], ATTR_READONLY, "readonly, through two levels");
        assert_eq!(attrs[&"name"], ATTR_NORMAL, "through the second base");
        let id_attr = both.attributes.iter().find(|a| a.name == "id").unwrap();
        assert_eq!(id_attr.defined_in, "IDL:gc10/Base:1.0");
        assert_eq!(id_attr.id, "IDL:gc10/Base/id:1.0");

        assert_eq!(
            both.base_interfaces,
            ["IDL:gc10/Derived:1.0", "IDL:gc10/Nameable:1.0"],
            "direct bases only, in declaration order"
        );
    }

    #[test]
    fn repository_ids_split_into_name_container_and_version() {
        assert_eq!(
            split_repository_id("IDL:a/b/C:1.0"),
            ("C".into(), "IDL:a/b:1.0".into(), "1.0".into())
        );
        assert_eq!(
            split_repository_id("IDL:Top:1.0"),
            ("Top".into(), String::new(), "1.0".into()),
            "a top-level definition has no containing module"
        );
        assert_eq!(absolute_name("IDL:a/b/C:1.0"), "::a::b::C");
        assert_eq!(absolute_name("nonsense"), "::nonsense", "never mangled silently");
    }

    /// Under `#pragma prefix` the id alone stops being enough: splitting it
    /// reads the prefix as an enclosing module. The facade asks the registry
    /// for the name it recorded when it loaded the IDL instead.
    ///
    /// The top-level case is the sharp one — `IDL:acme.com/Solo:1.0` split
    /// naively is "Solo, contained in module `IDL:acme.com:1.0`", and that
    /// module does not exist anywhere.
    #[test]
    fn a_prefix_is_identity_and_never_a_containing_module() {
        let registry = registry_from_idl(
            "#pragma prefix \"acme.com\"\n\
             interface Solo { void a(); };\n\
             module bank { interface Account { long balance(); }; };",
        )
        .expect("prefixed IDL loads");
        let ifr = RepositoryServer::new("127.0.0.1", 1, ROOT.to_vec(), registry);

        let nested = "IDL:acme.com/bank/Account:1.0";
        assert_eq!(
            ifr.contained_of(nested),
            ("Account".into(), "IDL:acme.com/bank:1.0".into(), "1.0".into()),
            "the container is module bank, whose own id also carries the prefix"
        );
        assert_eq!(ifr.absolute_name_of(nested), "::bank::Account", "no prefix in the name");

        let solo = "IDL:acme.com/Solo:1.0";
        assert_eq!(
            ifr.contained_of(solo),
            ("Solo".into(), String::new(), "1.0".into()),
            "top level: the container is the repository, which has no id"
        );
        assert_eq!(ifr.absolute_name_of(solo), "::Solo");

        // Unchanged for everything without a prefix, which is every fixture
        // this project already publishes.
        assert_eq!(
            facade(1).contained_of("IDL:gc10/Both:1.0"),
            split_repository_id("IDL:gc10/Both:1.0")
        );
    }

    /// An id we only know from a peer has no recorded name, so the split is
    /// all there is — and saying so beats inventing one.
    #[test]
    fn an_unrecorded_id_falls_back_to_splitting() {
        let ifr = facade(1);
        assert_eq!(ifr.absolute_name_of("IDL:elsewhere/Thing:1.0"), "::elsewhere::Thing");
        assert_eq!(
            ifr.contained_of("IDL:elsewhere/Thing:1.0"),
            split_repository_id("IDL:elsewhere/Thing:1.0")
        );
    }

    // ── the containment walk (§14.5.4), landed 2026-08-25 ───────────────────

    /// The contract every walk test reads: two modules, one of them nested,
    /// an inheritance chain, and one definition of every entry kind.
    const WALK_IDL: &str = "
        module wk {
          struct Amount { long units; };
          enum Colour { RED, GREEN };
          exception Refused { string why; };
          typedef sequence<long> Longs;
          const long LIMIT = 7;
          valuetype Wallet { public long balance; };
          native Handle;
          abstract interface Auditable { string audit_line(); };
          interface Base {
            readonly attribute string tag;
            void touch();
          };
          interface Derived : Base {
            Amount total() raises (Refused);
          };
          module inner {
            struct Deep { long bits; };
          };
        };
    ";

    fn walk_server() -> RepositoryServer {
        let registry = registry_from_idl(WALK_IDL).expect("the walk contract loads");
        RepositoryServer::new("127.0.0.1", 0, ROOT.to_vec(), registry)
    }

    fn names_of(server: &RepositoryServer, found: &[Target]) -> Vec<String> {
        found.iter().map(|t| server.name_of_target(t)).collect()
    }

    /// The containment hierarchy the IDL described is the one a client walks.
    ///
    /// Modules are the load-bearing part: `Registry::load` records a module's
    /// id and qualified name and then **defines no entry for it**, so a walk
    /// that only knew about entries would report every definition as sitting
    /// directly in the repository — a different repository from the one the
    /// IDL described.
    #[test]
    fn the_walk_reproduces_the_containment_the_idl_described() {
        let s = walk_server();
        let all = DefinitionKind::All as u32;

        let root = s.contents(&Target::Repository, all, true);
        assert_eq!(names_of(&s, &root), ["wk"], "one top-level module, nothing loose beside it");

        let module = Target::Module("IDL:wk:1.0".into());
        let inside = s.contents(&module, all, true);
        assert_eq!(
            names_of(&s, &inside),
            [
                "Amount",
                "Auditable",
                "Base",
                "Colour",
                "Derived",
                "Handle",
                "LIMIT",
                "Longs",
                "Refused",
                "Wallet",
                "inner"
            ],
            "every entry of the module, plus the nested module, ordered by repository id"
        );

        let nested = Target::Module("IDL:wk/inner:1.0".into());
        assert_eq!(names_of(&s, &s.contents(&nested, all, true)), ["Deep"]);

        // An interface contains its members, and nothing contains the members
        // of an interface it merely inherits from unless asked.
        let derived = Target::Entry("IDL:wk/Derived:1.0".into());
        assert_eq!(names_of(&s, &s.contents(&derived, all, true)), ["total"]);
        assert_eq!(
            names_of(&s, &s.contents(&derived, all, false)),
            ["tag", "touch", "total"],
            "exclude_inherited = false brings in Base's operation and attribute. The order is \
             by repository id, so Base's members (IDL:wk/Base/...) precede Derived's \
             (IDL:wk/Derived/...) — see `contents` for why declaration order is not available"
        );
    }

    /// **Both parameters filter, and a test that only counted would not say
    /// so.** A `contents` that ignored `limit_type` would answer every call
    /// identically; each row here is a different answer.
    #[test]
    fn contents_honours_limit_type_and_exclude_inherited() {
        let s = walk_server();
        let module = Target::Module("IDL:wk:1.0".into());
        let by = |k: DefinitionKind| names_of(&s, &s.contents(&module, k as u32, true));

        assert_eq!(by(DefinitionKind::Struct), ["Amount"]);
        assert_eq!(by(DefinitionKind::Enum), ["Colour"]);
        assert_eq!(by(DefinitionKind::Exception), ["Refused"]);
        assert_eq!(by(DefinitionKind::Alias), ["Longs"]);
        assert_eq!(by(DefinitionKind::Constant), ["LIMIT"]);
        assert_eq!(by(DefinitionKind::Value), ["Wallet"]);
        assert_eq!(by(DefinitionKind::Native), ["Handle"]);
        assert_eq!(by(DefinitionKind::AbstractInterface), ["Auditable"]);
        assert_eq!(by(DefinitionKind::Interface), ["Base", "Derived"]);
        assert_eq!(by(DefinitionKind::Module), ["inner"]);
        // `dk_all` is the wildcard, not a kind to compare against — a filter
        // written as `kind == limit` alone would make it match nothing.
        assert_eq!(by(DefinitionKind::All).len(), 11);
        // A kind nothing in this module has.
        assert!(by(DefinitionKind::Union).is_empty());

        // On an interface, the two parameters are independent.
        let derived = Target::Entry("IDL:wk/Derived:1.0".into());
        let ops =
            |excl| names_of(&s, &s.contents(&derived, DefinitionKind::Operation as u32, excl));
        assert_eq!(ops(true), ["total"]);
        assert_eq!(ops(false), ["touch", "total"], "by repository id: Base's before Derived's");
        let attrs =
            |excl| names_of(&s, &s.contents(&derived, DefinitionKind::Attribute as u32, excl));
        assert!(attrs(true).is_empty(), "Derived declares no attribute of its own");
        assert_eq!(attrs(false), ["tag"], "Base's attribute arrives by inheritance");
    }

    /// `lookup` and `lookup_name` are different operations, and this is the
    /// difference.
    ///
    /// `lookup` takes a **scoped** name and resolves it by IDL's scoping rules
    /// — absolute from the repository, otherwise outward through the enclosing
    /// scopes — and returns at most one object. `lookup_name` takes a **simple**
    /// name and searches inward, returning every match, which is why it returns
    /// a sequence.
    #[test]
    fn lookup_resolves_a_scoped_name_and_lookup_name_searches_for_a_simple_one() {
        let s = walk_server();
        let all = DefinitionKind::All as u32;
        let repo = Target::Repository;
        let module = Target::Module("IDL:wk:1.0".into());
        let id_of = |t: Option<Target>| t.and_then(|t| t.id());

        // Absolute, from anywhere.
        assert_eq!(
            id_of(s.lookup(&module, "::wk::Derived")),
            Some("IDL:wk/Derived:1.0".to_owned())
        );
        // Relative, resolving outward: `Amount` is not in `wk::inner`, so the
        // search continues into `wk` and finds it.
        assert_eq!(
            id_of(s.lookup(&Target::Module("IDL:wk/inner:1.0".into()), "Amount")),
            Some("IDL:wk/Amount:1.0".to_owned())
        );
        // A module, by name. `Registry::load` removes a module's qualified
        // name from `by_name`, so this is the leg that answered nil until
        // omniORB's IR client asked for it on 2026-08-25.
        assert_eq!(id_of(s.lookup(&repo, "wk")), Some("IDL:wk:1.0".to_owned()));
        assert_eq!(id_of(s.lookup(&repo, "wk::inner")), Some("IDL:wk/inner:1.0".to_owned()));
        // A member, which is an object of its own.
        assert_eq!(
            id_of(s.lookup(&repo, "::wk::Base::touch")),
            Some("IDL:wk/Base/touch:1.0".to_owned())
        );
        // Absent is nil, not an exception (§14.5.4.1).
        assert!(s.lookup(&repo, "::wk::Nope").is_none());
        assert!(s.lookup(&repo, "").is_none());

        // `lookup_name` searches by simple name. One level from the root finds
        // only the module; all levels finds the definition.
        let ids = |levels| {
            s.lookup_name(&repo, "Amount", levels, all, true)
                .expect("defined levels")
                .into_iter()
                .filter_map(|t| t.id())
                .collect::<Vec<_>>()
        };
        assert!(ids(1).is_empty(), "one level from the root sees only `wk`");
        assert_eq!(ids(-1), ["IDL:wk/Amount:1.0"]);
        assert_eq!(ids(2), ["IDL:wk/Amount:1.0"]);

        // The filter applies to the search too.
        assert!(
            s.lookup_name(&repo, "Amount", -1, DefinitionKind::Interface as u32, true)
                .expect("defined levels")
                .is_empty()
        );

        // §14.5.4.1 leaves 0 and negatives other than -1 **undefined**, and an
        // undefined input answered arbitrarily becomes a promise nobody chose.
        for levels in [0, -2, -17] {
            assert!(
                s.lookup_name(&repo, "Amount", levels, all, true).is_err(),
                "levels_to_search = {levels} is undefined and must be refused"
            );
        }
    }

    /// Every reference a walk hands out is dialable, and the walk goes back up.
    ///
    /// This is the invariant that makes the walk usable at all: `contents`
    /// returns references, and a reference this servant mints and then does not
    /// claim in `knows` is a reference a client is handed and cannot call. It
    /// is measured over the whole tree rather than a sample, because the four
    /// object kinds have four different key derivations.
    #[test]
    fn every_reference_the_walk_hands_out_can_be_dialled_and_leads_back_up() {
        let s = walk_server();
        let all = DefinitionKind::All as u32;
        let mut stack = vec![Target::Repository];
        let mut seen = 0usize;
        while let Some(here) = stack.pop() {
            for child in s.contents(&here, all, false) {
                seen += 1;
                let ior = s.reference_for(&child);
                assert!(!ior.profiles.is_empty(), "{child:?} minted a nil reference");
                let key = &ior.profiles[0].object_key;
                assert!(
                    SharedDispatch::knows(&s, key),
                    "{child:?} minted a key this servant does not claim"
                );
                assert_eq!(
                    s.target(key).expect("dialable"),
                    child,
                    "the key did not reverse to the object it was minted for"
                );
                // §14.5.3.1: `defined_in` identifies the container — and for a
                // member reached through inheritance, the interface it is
                // inherited *from*.
                let up =
                    s.defined_in_of(&child).expect("everything below the root has a container");
                if !matches!(child, Target::Operation { .. } | Target::Attribute { .. }) {
                    assert_eq!(up, here, "{child:?} does not point back at its container");
                }
                if matches!(child, Target::Module(_) | Target::Entry(_)) {
                    stack.push(child);
                }
            }
        }
        assert!(seen >= 16, "the walk visited only {seen} objects");
    }

    /// `describe` carries an `any`, and the `any` decodes as the struct the
    /// specification names for that kind.
    ///
    /// This is where the walk meets the wire. The `kind` member must be the
    /// **most derived** kind (§14.5.3.1 says returning `dk_all` would be an
    /// error), and the `any`'s `TypeCode` must be the one a peer's stub holds
    /// or the client's extraction fails — or, worse, succeeds against the wrong
    /// layout.
    #[test]
    fn describe_carries_the_description_struct_the_specification_names() {
        let s = walk_server();
        let cases: [(Target, DefinitionKind, &str); 7] = [
            (
                Target::Module("IDL:wk:1.0".into()),
                DefinitionKind::Module,
                "IDL:omg.org/CORBA/ModuleDescription:1.0",
            ),
            (
                Target::Entry("IDL:wk/Amount:1.0".into()),
                DefinitionKind::Struct,
                "IDL:omg.org/CORBA/TypeDescription:1.0",
            ),
            (
                Target::Entry("IDL:wk/Refused:1.0".into()),
                DefinitionKind::Exception,
                "IDL:omg.org/CORBA/ExceptionDescription:1.0",
            ),
            (
                Target::Entry("IDL:wk/LIMIT:1.0".into()),
                DefinitionKind::Constant,
                "IDL:omg.org/CORBA/ConstantDescription:1.0",
            ),
            (
                Target::Entry("IDL:wk/Wallet:1.0".into()),
                DefinitionKind::Value,
                "IDL:omg.org/CORBA/ValueDescription:1.0",
            ),
            (
                Target::Entry("IDL:wk/Derived:1.0".into()),
                DefinitionKind::Interface,
                "IDL:omg.org/CORBA/InterfaceDescription:1.0",
            ),
            (
                Target::Operation { owner: "IDL:wk/Base:1.0".into(), name: "touch".into() },
                DefinitionKind::Operation,
                "IDL:omg.org/CORBA/OperationDescription:1.0",
            ),
        ];
        // Both byte orders: an encoder that only works native-endian passes
        // every local test and fails in the field.
        for endian in [Endian::Big, Endian::Little] {
            for (target, want_kind, want_tc_id) in &cases {
                let mut e = Encoder::new(endian);
                s.describe(target, &mut e).expect("describe answers");
                let bytes = e.finish().expect("finish");
                let mut d = Decoder::new(&bytes, endian);
                assert_eq!(
                    d.get_u32().expect("kind"),
                    *want_kind as u32,
                    "{target:?} ({endian:?}): the most derived kind"
                );
                let tc = typecode::decode(&mut d).expect("the any's TypeCode");
                assert_eq!(
                    typecode_id(&tc),
                    Some(*want_tc_id),
                    "{target:?} ({endian:?}): the any's TypeCode"
                );
                // The value half must be there and must be exactly consumed:
                // a description that under- or over-wrote its members would
                // leave the decoder short or long.
                let name = d.get_string().expect("the first member is the name");
                assert!(!name.is_empty(), "{target:?}: an empty name");
            }
        }
        // The repository is not `Contained` (§14.5.6), so it has no describe.
        let mut e = Encoder::new(Endian::Big);
        assert!(s.describe(&Target::Repository, &mut e).is_err());
    }

    /// The description structs decode back into themselves, both byte orders.
    ///
    /// A writer whose reader is its own is only half a measurement — the other
    /// half is omniORB's IR client extracting the same `any`, which
    /// `spikes/walk_peer.py` drives against the live fixture. This pins the
    /// member order the peer agreed with, so a reorder is red here first.
    #[test]
    fn the_new_description_structs_round_trip_in_both_byte_orders() {
        let module = ModuleDescription {
            name: "wk".into(),
            id: "IDL:wk:1.0".into(),
            defined_in: String::new(),
            version: "1.0".into(),
        };
        let typed = TypeDescription {
            name: "Amount".into(),
            id: "IDL:wk/Amount:1.0".into(),
            defined_in: "IDL:wk:1.0".into(),
            version: "1.0".into(),
            tc: TypeCode::Long,
        };
        let iface = InterfaceDescription {
            name: "Derived".into(),
            id: "IDL:wk/Derived:1.0".into(),
            defined_in: "IDL:wk:1.0".into(),
            version: "1.0".into(),
            base_interfaces: vec!["IDL:wk/Base:1.0".into()],
        };
        let value = ValueDescription {
            name: "Wallet".into(),
            id: "IDL:wk/Wallet:1.0".into(),
            is_abstract: false,
            is_custom: false,
            defined_in: "IDL:wk:1.0".into(),
            version: "1.0".into(),
            supported_interfaces: Vec::new(),
            abstract_base_values: Vec::new(),
            is_truncatable: false,
            base_value: String::new(),
        };
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            module.write_to(&mut e).unwrap();
            typed.write_to(&mut e).unwrap();
            iface.write_to(&mut e).unwrap();
            value.write_to(&mut e).unwrap();
            let bytes = e.finish().unwrap();
            let mut d = Decoder::new(&bytes, endian);
            assert_eq!(ModuleDescription::read_from(&mut d).unwrap(), module, "{endian:?}");
            assert_eq!(TypeDescription::read_from(&mut d).unwrap(), typed, "{endian:?}");
            assert_eq!(InterfaceDescription::read_from(&mut d).unwrap(), iface, "{endian:?}");
            assert_eq!(ValueDescription::read_from(&mut d).unwrap(), value, "{endian:?}");
        }
    }

    /// `Repository::get_primitive` and `get_canonical_typecode` (§14.5.6.1).
    #[test]
    fn the_repository_answers_for_primitives_and_canonicalises_a_typecode() {
        let s = walk_server();

        // A `PrimitiveDef` is an `IDLType` and **not** `Contained`: it has a
        // type and no id, no name and no describe.
        let long = Target::Primitive(3);
        assert_eq!(s.type_of(&long).expect("a primitive has a type"), TypeCode::Long);
        assert_eq!(s.kind_of(&long), DefinitionKind::Primitive);
        assert!(long.id().is_none(), "a primitive type is unnamed (§14.5.14)");
        assert!(!s.is_a_ids(&long).contains(&CONTAINED_ID));
        assert!(s.is_a_ids(&long).contains(&IDL_TYPE_ID));

        // §14.5.14: "A PrimitiveDef with kind pk_string represents an
        // unbounded string" — bounded is a `StringDef`, which is a different
        // object and not one this facade mints.
        assert_eq!(s.type_of(&Target::Primitive(14)).unwrap(), TypeCode::String(0));
        assert_eq!(s.type_of(&Target::Primitive(20)).unwrap(), TypeCode::WString(0));
        // "There are no PrimitiveDefs with kind pk_null."
        assert!(primitive_typecode(pk::NULL).is_none());
        // `pk_value_base` is refused for a reason of ours, recorded rather than
        // answered with an invented TypeCode.
        assert!(primitive_typecode(pk::VALUE_BASE).is_none());
        // A key for a kind with no PrimitiveDef is not a key we minted.
        assert!(s.object_for("pk:0").is_none());
        assert!(s.object_for("pk:99").is_none());
        assert_eq!(s.object_for("pk:3"), Some(Target::Primitive(3)));

        // A named TypeCode the repository holds comes back as the
        // repository's own — which is what fills in names a peer sent stripped.
        let stripped = TypeCode::Struct {
            id: "IDL:wk/Amount:1.0".into(),
            name: String::new(),
            members: Vec::new(),
        };
        let canonical = s.canonical_typecode(&stripped);
        let TypeCode::Struct { name, members, .. } = &canonical else {
            panic!("still a struct: {canonical:?}");
        };
        assert_eq!(name, "Amount");
        assert_eq!(members.len(), 1, "the member the repository knows about");

        // An anonymous TypeCode has no id to look up, so §14.5.6.1 says
        // recurse: the element is canonicalised even though the sequence
        // cannot be.
        let seq = TypeCode::Sequence { element: Box::new(stripped.clone()), bound: 0 };
        let TypeCode::Sequence { element, .. } = s.canonical_typecode(&seq) else {
            panic!("still a sequence");
        };
        assert_eq!(*element, canonical);

        // A TypeCode naming nothing this repository holds comes back
        // unchanged rather than refused — it is already "an equivalent
        // TypeCode", which is all the operation promises.
        let foreign =
            TypeCode::Struct { id: "IDL:no/Such:1.0".into(), name: "Such".into(), members: vec![] };
        assert_eq!(s.canonical_typecode(&foreign), foreign);
    }

    /// A `#pragma prefix` segment is not a module, and the walk must not mint
    /// one.
    ///
    /// The same trap [`RepositoryServer::contained_of`] documents, one level
    /// out: reading every leading path segment as an enclosing module makes
    /// `IDL:acme.com/bank/Money:1.0` sit inside a module `IDL:acme.com:1.0`
    /// that does not exist — and a walk would then hand a client a reference
    /// to it.
    #[test]
    fn a_pragma_prefix_segment_is_not_a_module_the_walk_can_reach() {
        let registry = registry_from_idl(
            "#pragma prefix \"acme.com\"
             module bank {
               module ledger { struct Row { long n; }; };
             };",
        )
        .expect("loads");
        let s = RepositoryServer::new("127.0.0.1", 0, ROOT.to_vec(), registry);

        let modules: Vec<String> = s.module_ids().keys().cloned().collect();
        // Ordered by repository id, where `/` (0x2f) sorts before `:` (0x3a).
        assert_eq!(modules, ["IDL:acme.com/bank/ledger:1.0", "IDL:acme.com/bank:1.0"]);
        assert!(s.object_for("IDL:acme.com:1.0").is_none(), "the prefix is not an object");

        let root = s.contents(&Target::Repository, DefinitionKind::All as u32, true);
        assert_eq!(names_of(&s, &root), ["bank"]);
        assert_eq!(
            s.absolute_name_of_target(&Target::Module("IDL:acme.com/bank/ledger:1.0".into())),
            "::bank::ledger",
            "the absolute name is the IDL scope, with no prefix in it"
        );
    }

    /// A `valuetype`, a `native` and an `abstract interface` get the kind they
    /// are, and the ordinals are the ones omniORB named back.
    ///
    /// Both halves are asserted, which is the point. The registry's half so a
    /// regression to `TypeCode::ObjRef` is red in the facade as well as in
    /// `valuetype_shape_from_a_peer.rs`; the facade's half so that losing an
    /// ordinal again — the state this replaced, where all three answered
    /// `dk_none` and a conformant client was told a definition that exists does
    /// not — cannot land quietly.
    ///
    /// This test was called `..._and_still_answer_dk_none` until 2026-08-25 and
    /// asserted exactly that, over a doc comment which had asserted the
    /// opposite for five days. Both are repaired here.
    #[test]
    fn a_valuetype_a_native_and_an_abstract_interface_get_the_kind_they_are() {
        let registry = registry_from_idl(
            "module gk {
               valuetype Money { public long units; };
               native Handle;
               abstract interface Describable { string describe(); };
               interface Plain { void touch(); };
               struct Holder { long bits; };
             };",
        )
        .expect("loads");

        // The registry's half: three distinct answers where there was one.
        assert!(matches!(
            registry.get("IDL:gk/Money:1.0"),
            Some(Entry::Type(TypeCode::Value { .. }))
        ));
        assert!(matches!(
            registry.get("IDL:gk/Handle:1.0"),
            Some(Entry::Type(TypeCode::Native { .. }))
        ));

        // The facade's half. `dk_Value` 20, `dk_Native` 23,
        // `dk_AbstractInterface` 24 — read back by name from omniORB 4.3.4's
        // own IR client, 2026-08-25.
        let server = RepositoryServer::new("127.0.0.1", 0, ROOT.to_vec(), registry);
        assert_eq!(server.def_kind("IDL:gk/Money:1.0"), DefinitionKind::Value);
        assert_eq!(server.def_kind("IDL:gk/Handle:1.0"), DefinitionKind::Native);
        assert_eq!(
            server.def_kind("IDL:gk/Describable:1.0"),
            DefinitionKind::AbstractInterface,
            "an abstract interface is an Entry::Interface; the distinction is \
             InterfaceEntry::abstract_interface, which this facade did not read"
        );
        assert_eq!(server.def_kind("IDL:gk/Plain:1.0"), DefinitionKind::Interface);
        assert_eq!(server.def_kind("IDL:gk/Holder:1.0"), DefinitionKind::Struct);
        assert_eq!(server.def_kind("IDL:gk/Nope:1.0"), DefinitionKind::None, "absent is dk_none");
    }

    /// Every `TypeCode` variant has a verdict, and the verdict is written here
    /// as well as in the function.
    ///
    /// The value of this test is not the arms it agrees with — it is that
    /// adding a variant to `TypeCode` breaks the build in `kind_of_type` (no
    /// `_` arm) *and* leaves this table visibly short. The three variants added
    /// in August were absorbed by a catch-all with nothing red; a table that
    /// must be extended by hand is the record of the decision.
    #[test]
    fn every_typecode_variant_has_a_definition_kind() {
        use DefinitionKind as K;
        let obj = || TypeCode::ObjRef { id: "IDL:m/I:1.0".into(), name: "I".into() };
        let cases: Vec<(TypeCode, K)> = vec![
            (TypeCode::Null, K::Primitive),
            (TypeCode::Void, K::Primitive),
            (TypeCode::Short, K::Primitive),
            (TypeCode::Long, K::Primitive),
            (TypeCode::UShort, K::Primitive),
            (TypeCode::ULong, K::Primitive),
            (TypeCode::Float, K::Primitive),
            (TypeCode::Double, K::Primitive),
            (TypeCode::Boolean, K::Primitive),
            (TypeCode::Char, K::Primitive),
            (TypeCode::Octet, K::Primitive),
            (TypeCode::Any, K::Primitive),
            (TypeCode::TypeCode, K::Primitive),
            (TypeCode::Principal, K::Primitive),
            (TypeCode::LongLong, K::Primitive),
            (TypeCode::ULongLong, K::Primitive),
            (TypeCode::LongDouble, K::Primitive),
            (TypeCode::WChar, K::Primitive),
            // §14.5.15: unbounded is a PrimitiveDef, bounded is a StringDef.
            (TypeCode::String(0), K::Primitive),
            (TypeCode::String(40), K::String),
            (TypeCode::WString(0), K::Primitive),
            (TypeCode::WString(40), K::Wstring),
            (TypeCode::Fixed { digits: 5, scale: 2 }, K::Fixed),
            (obj(), K::Interface),
            (
                TypeCode::Struct { id: "IDL:m/S:1.0".into(), name: "S".into(), members: vec![] },
                K::Struct,
            ),
            (
                TypeCode::Union {
                    id: "IDL:m/U:1.0".into(),
                    name: "U".into(),
                    discriminator: Box::new(TypeCode::Long),
                    default_index: -1,
                    cases: vec![],
                },
                K::Union,
            ),
            (
                TypeCode::Enum { id: "IDL:m/E:1.0".into(), name: "E".into(), members: vec![] },
                K::Enum,
            ),
            (TypeCode::Sequence { element: Box::new(TypeCode::Long), bound: 0 }, K::Sequence),
            (TypeCode::Array { element: Box::new(TypeCode::Long), length: 2 }, K::Array),
            (
                TypeCode::Alias {
                    id: "IDL:m/A:1.0".into(),
                    name: "A".into(),
                    aliased: Box::new(TypeCode::Long),
                },
                K::Alias,
            ),
            (
                TypeCode::Except { id: "IDL:m/X:1.0".into(), name: "X".into(), members: vec![] },
                K::Exception,
            ),
            (
                TypeCode::Value {
                    id: "IDL:m/V:1.0".into(),
                    name: "V".into(),
                    modifier: 0,
                    base: None,
                    members: vec![],
                },
                K::Value,
            ),
            (
                TypeCode::AbstractInterface { id: "IDL:m/D:1.0".into(), name: "D".into() },
                K::AbstractInterface,
            ),
            (TypeCode::Native { id: "IDL:m/N:1.0".into(), name: "N".into() }, K::Native),
            (TypeCode::Recursive("IDL:m/R:1.0".into()), K::None),
        ];
        // 33 variants, 35 rows: `String` and `WString` each get two, because
        // the bound decides the kind (§14.5.15).
        assert_eq!(cases.len(), 35, "TypeCode has 33 variants; every one gets a verdict");
        let mut registry = Registry::new();
        let mut probes = Vec::new();
        for (n, (tc, want)) in cases.into_iter().enumerate() {
            let id = format!("IDL:probe/T{n}:1.0");
            registry
                .define_ingested(id.clone(), Entry::Type(tc.clone()), "the verdict table")
                .expect("registers");
            probes.push((id, tc, want));
        }
        // Collected rather than asserted one at a time: a catch-all takes
        // *several* variants at once, and a table that stops at the first
        // disagreement reports one of them and hides the rest — which is how
        // this class stays looking like a one-variant oversight.
        let server = RepositoryServer::new("127.0.0.1", 0, ROOT.to_vec(), registry);
        let wrong: Vec<String> = probes
            .iter()
            .filter(|(id, _, want)| server.def_kind(id) != *want)
            .map(|(id, tc, want)| {
                format!("  {tc:?}\n    want {want:?}, got {:?}", server.def_kind(id))
            })
            .collect();
        assert!(
            wrong.is_empty(),
            "{} variants get the wrong kind:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    // ── refusal shapes, over the wire ───────────────────────────────────────

    fn expect_system(err: Error, id: &str, ctx: &str) {
        match err {
            Error::SystemException { id: got, .. } => assert_eq!(got, id, "{ctx}"),
            other => panic!("{ctx}: expected {id}, got {other:?}"),
        }
    }

    /// Three refusals, three meanings, and the wire tells them apart without a
    /// document: `NO_PERMISSION` is policy, `NO_IMPLEMENT` is a deferral,
    /// `BAD_OPERATION` is "no such operation — try another reference".
    ///
    /// The middle row is the repair. Every deferred operation used to answer
    /// `BAD_OPERATION`, which is byte-for-byte what an operation nobody had
    /// thought about answers, so `docs/SERVICES-COVERAGE.md` could only
    /// separate them by searching the repository for a written reason.
    ///
    /// **The middle row is empty as of 2026-08-25** — the ten operations that
    /// filled it were the containment walk and they now answer. The row itself
    /// stays, driven from [`is_deferred`], so the day something is deferred
    /// again the mechanism is already measured rather than remembered.
    #[test]
    fn the_three_refusals_are_distinguishable_on_the_wire() {
        let served = Served::start();
        let mut repo = served.repository();
        for op in
            ["create_interface", "create_struct", "_set_id", "_set_version", "destroy", "move"]
        {
            let err = repo.invoke(op, |e| e.put_str("anything")).unwrap_err();
            expect_system(err, NO_PERMISSION, op);
        }
        // Whatever is on the deferral list must answer NO_IMPLEMENT, and the
        // ten that used to be are named so that re-deferring one silently is
        // impossible: each is required *not* to be deferred and to be served.
        const ONCE_DEFERRED: [&str; 10] = [
            "contents",
            "lookup",
            "lookup_name",
            "describe_contents",
            "describe",
            "_get_defined_in",
            "_get_containing_repository",
            "get_canonical_typecode",
            "get_primitive",
            "_get_type",
        ];
        for op in ONCE_DEFERRED {
            assert!(
                !is_deferred(op),
                "{op} is the containment walk and is served since 2026-08-25"
            );
        }
        for op in ["no_such_operation", "no_such_other_operation"] {
            let err = repo.invoke(op, |e| e.put_str("anything")).unwrap_err();
            expect_system(err, BAD_OPERATION, op);
        }
        // `describe_interface` and `is_a` are `InterfaceDef` operations; the
        // repository is not an interface, so they are BAD_OPERATION *here* and
        // answered on an entry key.
        for op in ["describe_interface", "is_a"] {
            let err = repo.invoke(op, |e| e.put_str("anything")).unwrap_err();
            expect_system(err, BAD_OPERATION, op);
        }
        served.shutdown(repo);
    }

    /// The read half of `version`, on the data the facade already parses out of
    /// every repository id.
    ///
    /// Measured backwards on 2026-08-14: `_set_version` answered
    /// `NO_PERMISSION` ("the operation exists and the answer is no") while
    /// `_get_version` answered `BAD_OPERATION` ("no such operation"), which is
    /// the two the wrong way round by this module's own argument. The write
    /// half is still refused — it is in the loop above — and the read half now
    /// answers.
    #[test]
    fn the_version_a_repository_id_carries_is_readable_and_still_not_writable() {
        let served = Served::start();
        let mut def = served.entry("IDL:gc10/Both:1.0");
        let reply = def.invoke_nullary("_get_version").unwrap();
        assert_eq!(reply.body().unwrap().get_string().unwrap(), "1.0");

        let err = def.invoke("_set_version", |e| e.put_str("2.0")).unwrap_err();
        expect_system(err, NO_PERMISSION, "_set_version");
        drop(def);

        // On a non-interface entry too: `version` is a `Contained` accessor,
        // not an `InterfaceDef` one.
        let mut payload = served.entry("IDL:gc10/Payload:1.0");
        let reply = payload.invoke_nullary("_get_version").unwrap();
        assert_eq!(reply.body().unwrap().get_string().unwrap(), "1.0");
        drop(payload);

        // But not on the repository itself, which is a `Container`, not a
        // `Contained` — there is genuinely no such operation there.
        let mut repo = served.repository();
        let err = repo.invoke_nullary("_get_version").unwrap_err();
        expect_system(err, BAD_OPERATION, "_get_version on the Repository");
        served.shutdown(repo);
    }

    /// A version that is not `1.0` comes from the id, not from a default.
    #[test]
    fn the_version_follows_a_pragma_version() {
        let registry = registry_from_idl(
            "module m {\n  interface Aged { void tick(); };\n#pragma version Aged 2.3\n};",
        )
        .expect("versioned IDL loads");
        let ifr = RepositoryServer::new("127.0.0.1", 1, ROOT.to_vec(), registry);
        let id = "IDL:m/Aged:2.3";
        let ids: Vec<&RepositoryId> = ifr.registry().ids().collect();
        assert!(ifr.registry().get(id).is_some(), "the pragma did not set the id: {ids:?}");
        assert_eq!(ifr.contained_of(id).2, "2.3");
    }

    /// A refusal must not depend on the reference being valid, or a client
    /// reads OBJECT_NOT_EXIST as "retry elsewhere" — which is the retry the
    /// policy exists to stop.
    #[test]
    fn a_mutating_call_on_an_unknown_key_is_still_no_permission() {
        let ifr = facade(1);
        let req = request_on(b"InterfaceRepository/ifr/IDL:gc10/Nope:1.0", "create_module");
        let mut out = Encoder::new(Endian::Little);
        assert_eq!(ifr.dispatch(&req, &mut out).unwrap_err().id, NO_PERMISSION);

        let req = request_on(b"InterfaceRepository/ifr/IDL:gc10/Nope:1.0", "_get_id");
        let mut out = Encoder::new(Endian::Little);
        assert_eq!(
            ifr.dispatch(&req, &mut out).unwrap_err().id,
            OBJECT_NOT_EXIST,
            "a read against an unknown key is a different failure"
        );
    }

    fn request_on(key: &[u8], op: &str) -> Request {
        let wire =
            orbweaver_giop::encode_request(Version::V1_2, Endian::Little, 1, key, op, true, |e| {
                e.put_str("x")
            })
            .unwrap();
        let msg =
            orbweaver_giop::read_message(&mut &wire[..], orbweaver_giop::DEFAULT_MAX_MESSAGE_SIZE)
                .unwrap();
        orbweaver_giop::server::decode_request(msg).unwrap()
    }

    /// `describe_interface` and `is_a` belong to `InterfaceDef`; a struct's
    /// reference is only `Contained`, so they are BAD_OPERATION there while
    /// the Contained accessors still work.
    #[test]
    fn non_interface_entries_serve_contained_only() {
        let served = Served::start();
        let mut payload = served.entry("IDL:gc10/Payload:1.0");

        let reply = payload.invoke_nullary("_get_def_kind").unwrap();
        assert_eq!(reply.body().unwrap().get_u32().unwrap(), DefinitionKind::Struct as u32);
        let reply = payload.invoke_nullary("_get_name").unwrap();
        assert_eq!(reply.body().unwrap().get_string().unwrap(), "Payload");
        let reply = payload.invoke_nullary("_get_absolute_name").unwrap();
        assert_eq!(reply.body().unwrap().get_string().unwrap(), "::gc10::Payload");

        for op in ["describe_interface", "_get_base_interfaces"] {
            let err = payload.invoke_nullary(op).unwrap_err();
            expect_system(err, BAD_OPERATION, op);
        }
        let err = payload.invoke("is_a", |e| e.put_str(CONTAINED_ID)).unwrap_err();
        expect_system(err, BAD_OPERATION, "is_a on a struct");
        served.shutdown(payload);
    }

    // ── the client's walk, over the wire ────────────────────────────────────

    /// The whole DII path: lookup_id, the Contained accessors, then
    /// describe_interface decoded exactly as a foreign client would.
    #[test]
    fn a_client_looks_up_an_id_and_describes_the_interface() {
        let served = Served::start();
        let mut repo = served.repository();

        let reply = repo.invoke("lookup_id", |e| e.put_str("IDL:gc10/Both:1.0")).unwrap();
        let found = decode_object_reference(&mut reply.body().unwrap()).unwrap();
        assert!(!found.is_nil());
        assert_eq!(found.type_id, INTERFACE_DEF_ID);

        let reply = repo.invoke("lookup_id", |e| e.put_str("IDL:gc10/Absent:1.0")).unwrap();
        let missing = decode_object_reference(&mut reply.body().unwrap()).unwrap();
        assert!(missing.is_nil(), "an unknown id is a nil reference, not an exception");

        // Hanging up before dialing the InterfaceDef is a habit, not a
        // requirement: this said "one connection at a time", which was the
        // server's limit until stream E lifted it (2814dce) — the module docs
        // and `Served` both record that it is gone, and this line kept
        // asserting it. `concurrent_clients_walk_the_repository_at_once` below
        // dials several at once and is the proof. Left sequential because this
        // test has nothing to learn from overlapping, which is the only reason
        // there is.
        drop(repo);
        let mut def = Connection::connect(&found, T).unwrap();

        let reply = def.invoke_nullary("_get_id").unwrap();
        assert_eq!(reply.body().unwrap().get_string().unwrap(), "IDL:gc10/Both:1.0");
        let reply = def.invoke_nullary("_get_name").unwrap();
        assert_eq!(reply.body().unwrap().get_string().unwrap(), "Both");
        let reply = def.invoke_nullary("_get_absolute_name").unwrap();
        assert_eq!(reply.body().unwrap().get_string().unwrap(), "::gc10::Both");
        let reply = def.invoke_nullary("_get_def_kind").unwrap();
        assert_eq!(reply.body().unwrap().get_u32().unwrap(), DefinitionKind::Interface as u32);

        let reply = def.invoke_nullary("describe_interface").unwrap();
        let d = decode_full_interface_description(&mut reply.body().unwrap()).unwrap();
        assert_eq!(d, described("IDL:gc10/Both:1.0"), "the wire agrees with the local build");

        let reply = def.invoke_nullary("_get_base_interfaces").unwrap();
        let bases = read_interface_def_seq(&mut reply.body().unwrap()).unwrap();
        assert_eq!(bases.len(), 2);
        assert!(bases.iter().all(|b| b.type_id == INTERFACE_DEF_ID));
        assert_eq!(
            bases[0].primary().unwrap().object_key,
            served.server.entry_key("IDL:gc10/Derived:1.0")
        );

        served.shutdown(def);
    }

    /// `is_a` (the IR operation, about the described interface) and `_is_a`
    /// (about the reference itself) ask different questions, and confusing
    /// them is the easy bug: an InterfaceDef for `Both` *describes* something
    /// that derives from `Base`, but is not itself a `Base`.
    #[test]
    fn is_a_walks_inheritance_while_underscore_is_a_answers_for_the_reference() {
        let served = Served::start();
        let mut def = served.entry("IDL:gc10/Both:1.0");

        for (asked, expected) in [
            ("IDL:gc10/Base:1.0", true),
            ("IDL:gc10/Derived:1.0", true),
            ("IDL:gc10/Nameable:1.0", true),
            ("IDL:gc10/Both:1.0", true),
            ("IDL:gc10/Guarded:1.0", false),
        ] {
            let reply = def.invoke("is_a", move |e| e.put_str(asked)).unwrap();
            assert_eq!(reply.body().unwrap().get_bool().unwrap(), expected, "is_a {asked}");
        }

        for (asked, expected) in [
            (INTERFACE_DEF_ID, true),
            (CONTAINED_ID, true),
            (IR_OBJECT_ID, true),
            (OBJECT_ID, true),
            ("IDL:gc10/Base:1.0", false),
            (REPOSITORY_ID, false),
        ] {
            let reply = def.invoke("_is_a", move |e| e.put_str(asked)).unwrap();
            assert_eq!(reply.body().unwrap().get_bool().unwrap(), expected, "_is_a {asked}");
        }
        served.shutdown(def);
    }

    /// The root answers as the Repository, and narrows to it.
    #[test]
    fn the_root_key_is_the_repository_object() {
        let served = Served::start();
        let mut repo = served.repository();
        assert_eq!(served.root.type_id, REPOSITORY_ID);
        let reply = repo.invoke_nullary("_get_def_kind").unwrap();
        assert_eq!(reply.body().unwrap().get_u32().unwrap(), DefinitionKind::Repository as u32);
        let reply = repo.invoke("_is_a", |e| e.put_str(REPOSITORY_ID)).unwrap();
        assert!(reply.body().unwrap().get_bool().unwrap());
        let reply = repo.invoke("_is_a", |e| e.put_str(INTERFACE_DEF_ID)).unwrap();
        assert!(!reply.body().unwrap().get_bool().unwrap(), "the root is not an InterfaceDef");
        served.shutdown(repo);
    }

    #[test]
    fn interface_ids_lists_only_interfaces() {
        let registry = registry_from_idl(IDL).unwrap();
        let ids = interface_ids(&registry);
        assert!(ids.contains(&"IDL:gc10/Both:1.0".to_string()));
        assert!(!ids.contains(&"IDL:gc10/Payload:1.0".to_string()), "a struct is not an interface");
    }
}
