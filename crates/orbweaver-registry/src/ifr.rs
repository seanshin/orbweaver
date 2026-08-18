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

use std::collections::BTreeSet;

use orbweaver_cdr::{Decoder, Encoder};
use orbweaver_giop::server::{Completion, Dispatch, Request, SharedDispatch, SystemException};
use orbweaver_giop::typecode::{self, TypeCode};
use orbweaver_giop::{IiopProfile, Ior, Result, Version};

use crate::{Entry, ParamDirection, Registry, RepositoryId};

// ── repository ids ───────────────────────────────────────────────────────────

/// `CORBA::Repository` — the object the root key answers as.
pub const REPOSITORY_ID: &str = "IDL:omg.org/CORBA/Repository:1.0";
/// `CORBA::Container`, which `Repository` and `InterfaceDef` both derive from.
pub const CONTAINER_ID: &str = "IDL:omg.org/CORBA/Container:1.0";
/// `CORBA::Contained` — what `lookup_id` is declared to return.
pub const CONTAINED_ID: &str = "IDL:omg.org/CORBA/Contained:1.0";
/// `CORBA::InterfaceDef` — the object an interface entry answers as.
pub const INTERFACE_DEF_ID: &str = "IDL:omg.org/CORBA/InterfaceDef:1.0";
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
/// Only the values this facade can produce are named; the rest of the enum
/// (components, homes, events) describes IDL constructs the registry does not
/// hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
#[allow(missing_docs)]
pub enum DefinitionKind {
    None = 0,
    Constant = 3,
    Exception = 4,
    Interface = 5,
    Alias = 9,
    Struct = 10,
    Union = 11,
    Enum = 12,
    Repository = 17,
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
    /// `ContextIdSeq`. Always empty: the IDL `context` clause is not parsed,
    /// and inventing identifiers would be worse than reporting none.
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
pub fn is_deferred(op: &str) -> bool {
    matches!(
        op,
        // Container
        "contents"
            | "lookup"
            | "lookup_name"
            | "describe_contents"
            // Contained
            | "describe"
            | "_get_defined_in"
            | "_get_containing_repository"
            // Repository
            | "get_canonical_typecode"
            | "get_primitive"
            // IDLType
            | "_get_type"
    )
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
enum Target<'a> {
    /// The root: the `Repository` itself.
    Repository,
    /// A registry entry, by repository id.
    Entry(&'a str),
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
    /// narrows to something we actually serve.
    pub fn entry_ior(&self, id: &str) -> Ior {
        match self.registry.get(id) {
            None => nil_ref(),
            Some(Entry::Interface(_)) => self.ior_for(INTERFACE_DEF_ID, self.entry_key(id)),
            Some(_) => self.ior_for(CONTAINED_ID, self.entry_key(id)),
        }
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

    fn target<'a>(&self, key: &'a [u8]) -> std::result::Result<Target<'a>, SystemException> {
        if key == self.root.as_slice() {
            return Ok(Target::Repository);
        }
        match self.id_from_key(key) {
            Some(id) if self.registry.get(id).is_some() => Ok(Target::Entry(id)),
            _ => Err(SystemException::object_not_exist()),
        }
    }

    /// `IRObject::_get_def_kind` for a registry entry.
    ///
    /// `valuetype` and `native` are the honest gap: the registry stores both
    /// as an object-reference `TypeCode` because v1 marshals neither
    /// (PLAN §4.4), so the facade cannot tell them apart and reports
    /// `dk_none` rather than guessing one of them.
    fn def_kind(&self, id: &str) -> DefinitionKind {
        match self.registry.get(id) {
            Some(Entry::Interface(_)) => DefinitionKind::Interface,
            Some(Entry::Const { .. }) => DefinitionKind::Constant,
            Some(Entry::Type(tc)) => match tc {
                TypeCode::Struct { .. } => DefinitionKind::Struct,
                TypeCode::Union { .. } => DefinitionKind::Union,
                TypeCode::Enum { .. } => DefinitionKind::Enum,
                TypeCode::Alias { .. } => DefinitionKind::Alias,
                TypeCode::Except { .. } => DefinitionKind::Exception,
                _ => DefinitionKind::None,
            },
            None => DefinitionKind::None,
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
    fn is_a_ids(&self, target: &Target<'_>) -> Vec<&'static str> {
        match target {
            Target::Repository => vec![REPOSITORY_ID, CONTAINER_ID, IR_OBJECT_ID, OBJECT_ID],
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
                _ => vec![CONTAINED_ID, IR_OBJECT_ID, OBJECT_ID],
            },
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
            (Target::Repository, "_get_def_kind") => out.put_u32(DefinitionKind::Repository as u32),
            (Target::Repository, "lookup_id") => {
                let asked = args.get_string().map_err(|_| SystemException::marshal())?;
                self.entry_ior(&asked).write_to(out).map_err(|_| SystemException::marshal())?;
            }
            (Target::Entry(id), "_get_def_kind") => out.put_u32(self.def_kind(id) as u32),
            (Target::Entry(id), "_get_id") => out.put_str(id),
            (Target::Entry(id), "_get_name") => out.put_str(&self.contained_of(id).0),
            (Target::Entry(id), "_get_absolute_name") => out.put_str(&self.absolute_name_of(id)),
            // The read half of `version`. The write half is `_set_version`,
            // refused `NO_PERMISSION` above with every other mutator, and the
            // two answers have to agree that the operation exists at all.
            (Target::Entry(id), "_get_version") => out.put_str(&self.contained_of(id).2),
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
    /// The root key and every key derived from a registered repository id.
    fn knows(&self, object_key: &[u8]) -> bool {
        object_key == self.root.as_slice()
            || self.id_from_key(object_key).is_some_and(|id| self.registry.get(id).is_some())
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
        for op in [
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
        ] {
            let err = repo.invoke(op, |e| e.put_str("anything")).unwrap_err();
            expect_system(err, NO_IMPLEMENT, op);
            assert!(is_deferred(op), "{op} answers NO_IMPLEMENT but is not on the list");
        }
        for op in ["no_such_operation", "describe_interface", "is_a"] {
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

        // One connection at a time: hang up before dialing the InterfaceDef.
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
