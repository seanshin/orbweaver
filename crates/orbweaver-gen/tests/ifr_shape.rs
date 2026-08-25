//! The measurement: can a generated skeleton express a hand-written servant?
//!
//! `crates/orbweaver-registry/src/ifr.rs` is a hand-written `Dispatch` serving
//! the read-only Interface Repository. It is the best candidate of the five
//! hand-written servants in this workspace — read-only, so there is no state to
//! diverge, and its object-key scheme is already documented and reversible.
//!
//! This file replaces its **dispatch** with generated code and keeps its
//! **body** by hand. That is the claim being tested, stated precisely: bodies
//! are hand-written, dispatch is not. What has to disappear is the
//! `impl Dispatch`, the `match (target, operation)`, the key parsing, the
//! `Ior`/`IiopProfile` assembly and the `_is_a` id list — and it does. What is
//! written below is three servant traits' worth of method bodies, each of which
//! reads a registry and answers.
//!
//! `ifr.rs` itself is **not modified**; it is imported and driven as the
//! oracle. Every case in [`compared`] sends the same GIOP request to both
//! servants, in **both byte orders**, and requires the reply bytes to be
//! identical — the §8 rule ("static result equals dynamic result") applied to
//! a different pair.
//!
//! # `describe_interface`
//!
//! The operation the IR facade exists for — one call and a DII client has
//! every signature it needs — is now part of the comparison, and it is the
//! case with the most in it: a struct of eight members carrying three nested
//! sequences of structs, three enums, a nil object reference and a `TypeCode`
//! per operation, per parameter, per attribute and per raised exception.
//!
//! It reached the comparison in two steps, and both are worth naming because
//! [`NOT_COMPARED`] is where they were recorded. It was excluded first because
//! the registry loaded `::CORBA::TypeCode` as `void`, so generating it would
//! have produced a silently empty reply; then, once that was fixed, only
//! because `corpus/services/ir-subset.idl` did not declare it. The contract now
//! does, with the description structs' members placed against `ifr.rs`'s
//! `write_to` line by line rather than against a recollection of the
//! specification — member order is the whole game when the test is byte
//! equality.
//!
//! # What the generated skeleton cannot express
//!
//! Named rather than skipped: [`NOT_COMPARED`] lists every case where the two
//! answer differently, with the cause, and
//! [`the_divergences_are_the_ones_named_and_they_still_diverge`] fails if one
//! of them quietly starts agreeing. A list of known gaps nobody re-measures is
//! a list of things that were once true — as its former first entry was.
//!
//! # The shape of the arrangement
//!
//! `ifr.rs` is one servant answering as three interfaces depending on the key.
//! Generated, that is three skeletons behind an `rt::Servants`, each with a
//! `knows` that claims a disjoint part of one shared key space —
//! `with_infix("/ifr/")`, the same infix `ifr.rs` uses, so the object keys and
//! therefore the minted references are byte-identical too.

mod emitted;

use std::collections::BTreeSet;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_gen::rt::{self, Dispatch, ObjRef, ObjectHome, Servants};
use orbweaver_giop::server::{Request, decode_request};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, Version, encode_request, read_message};
use orbweaver_registry::{Entry, ParamDirection, Registry, ifr};

use emitted::f_ir_subset::CORBA::InterfaceDef::FullInterfaceDescription;
use emitted::f_ir_subset::CORBA::{
    AttributeDescription, AttributeMode, ContainedFault, ContainedRefs, ContainedServant,
    ContainedSkeleton, ContainedTarget, DefinitionKind, ExceptionDescription, InterfaceDefFault,
    InterfaceDefRefs, InterfaceDefServant, InterfaceDefSkeleton, InterfaceDefTarget,
    OperationDescription, OperationMode, ParameterDescription, ParameterMode, RepositoryFault,
    RepositoryRefs, RepositoryServant, RepositorySkeleton, RepositoryTarget,
};

const ROOT: &[u8] = b"ifr-root";
const HOST: &str = "127.0.0.1";
const PORT: u16 = 4242;

/// The key space `ifr.rs` uses, adopted verbatim so the two servants mint the
/// same object keys and their `lookup_id` replies can be compared as bytes.
const INFIX: &str = "/ifr/";

/// The registry both servants describe.
const SUBJECT: &str = "
module bank {
  //@ ai_desc: An amount, in whole units and hundredths
  struct Money { long units; long cents; };
  //@ ai_desc: Which currency an amount is denominated in
  enum Currency { KRW, USD };
  //@ ai_desc: The withdrawal was refused
  exception Denied { string why; };
  //@ ai_desc: Anything that can hold an account
  interface Party {
    //@ ai_desc: The party's registered identifier
    //@ ai_effect: read_only
    readonly attribute string party_id;
  };
  //@ ai_desc: A bank account
  interface Account : Party {
    //@ ai_desc: What is in the account right now
    //@ ai_effect: read_only
    Money balance();
    //@ ai_desc: Takes an amount out of the account
    //@ ai_effect: destructive
    //@ ai_authz: bank.account.withdraw
    void withdraw(in Money amount) raises (Denied);
  };
  // Five entry shapes added 2026-08-25. Two of them — `Voucher` and `Ledger` —
  // were answered `dk_none` by both `def_kind`s until that day, and `Auditable`
  // was answered `dk_Interface` with nothing looking at whether it was
  // abstract. **None of the five was in this subject**, which is why the two
  // classifiers could carry the same `_ =>` catch-all for five days with 82
  // pinned comparisons green: the comparison only compares what the contract
  // declares, so a shared blind spot is invisible to it by construction.
  // `CEILING` and `Statement` were already answered correctly and are here as
  // the control — the exhaustive rewrite must not move them.
  //@ ai_desc: The largest amount a single withdrawal may take
  const long CEILING = 1000000;
  //@ ai_desc: A run of amounts, oldest first
  typedef sequence<Money> Statement;
  //@ ai_desc: Anything that can describe itself to an auditor
  abstract interface Auditable {
    //@ ai_desc: A one-line description for the audit log
    string audit_line();
  };
  //@ ai_desc: A held amount, carried by value rather than by reference
  valuetype Voucher {
    //@ ai_desc: What the voucher is worth
    public long worth;
  };
  //@ ai_desc: A handle only the local language mapping understands
  native Ledger;
};
";

const ACCOUNT: &str = "IDL:bank/Account:1.0";
const PARTY: &str = "IDL:bank/Party:1.0";
const MONEY: &str = "IDL:bank/Money:1.0";
const CURRENCY: &str = "IDL:bank/Currency:1.0";
const DENIED: &str = "IDL:bank/Denied:1.0";
const ABSENT: &str = "IDL:bank/Nope:1.0";
/// `Entry::Const` — `dk_Constant`.
const CEILING: &str = "IDL:bank/CEILING:1.0";
/// `TypeCode::Alias` — `dk_Alias`.
const STATEMENT: &str = "IDL:bank/Statement:1.0";
/// `Entry::Interface` with `abstract_interface` set — `dk_AbstractInterface`.
const AUDITABLE: &str = "IDL:bank/Auditable:1.0";
/// `TypeCode::Value` — `dk_Value`, 20.
const VOUCHER: &str = "IDL:bank/Voucher:1.0";
/// `TypeCode::Native` — `dk_Native`, 23.
const LEDGER: &str = "IDL:bank/Ledger:1.0";

/// `create_module(in RepositoryId, in Identifier, in VersionSpec)` — three
/// arguments, and writing one made the generated skeleton answer `MARSHAL`
/// where the hand-written one answered `NO_PERMISSION`. That divergence was a
/// malformed request, not a defect; the real ordering difference it exposed is
/// the "malformed body under a refused operation" entry of `NOT_COMPARED`.
const CREATE_ARGS: &[&str] = &["IDL:bank/New:1.0", "New", "1.0"];

fn registry() -> Registry {
    let spec = orbweaver_idl::parse(SUBJECT).expect("the subject IDL parses");
    let mut r = Registry::new();
    r.load(&spec).expect("loads");
    r
}

// ── The hand-written half: three servant bodies, no dispatch ─────────────────

/// The `Contained` triple `ifr.rs` computes — name, containing module, version
/// — reproduced here.
///
/// `split_repository_id` alone is wrong under `#pragma prefix`, which reads
/// every leading segment as an enclosing module; the registry recorded the
/// qualified name when it loaded the IDL and the count of its segments is what
/// says how much of the path is prefix. Both helpers `ifr.rs` uses for the
/// fallback are public, so the fallback is shared rather than re-derived.
///
/// The oracle's own `contained_of` is private, so this is the one piece of
/// `ifr.rs` that is re-implemented rather than called. It is re-implemented
/// *line for line* on purpose: `describe_interface` is compared as bytes, and
/// three of its string members come from here.
fn contained_of(registry: &Registry, id: &str) -> (String, String, String) {
    let split = ifr::split_repository_id(id);
    let Some(qual) = registry.qualified_name(id) else { return split };
    let (_, _, version) = split;
    let Some(path) = id.strip_prefix("IDL:").and_then(|rest| rest.rsplit_once(':').map(|(p, _)| p))
    else {
        return ifr::split_repository_id(id);
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

fn name_of(registry: &Registry, id: &str) -> String {
    contained_of(registry, id).0
}

fn absolute_name_of(registry: &Registry, id: &str) -> String {
    match registry.qualified_name(id) {
        Some(q) => format!("::{q}"),
        None => ifr::absolute_name(id),
    }
}

/// One `ExceptionDescription`, as `ifr.rs` builds it.
///
/// An unregistered `raises` clause means the IDL named an exception we never
/// saw a definition for; an empty `tk_except` is the honest answer and keeps
/// the description decodable.
fn exception_description(registry: &Registry, id: &str) -> ExceptionDescription {
    let (name, defined_in, version) = contained_of(registry, id);
    let tc = registry.typecode(id).cloned().unwrap_or(TypeCode::Except {
        id: id.to_owned(),
        name: name.clone(),
        members: Vec::new(),
    });
    ExceptionDescription {
        name,
        id: id.to_owned(),
        defined_in,
        version,
        r#type: rt::TypeCodeVal(tc),
    }
}

/// `IRObject::_get_def_kind`, against the **generated** enum.
///
/// This is the second home of `ifr::RepositoryServer::def_kind` and was a
/// byte-for-byte duplicate of it — the same five arms and the same
/// `_ => dk_none` catch-all — so when the oracle's arm swallowed
/// `TypeCode::Value`, `Native` and `AbstractInterface`, this one swallowed them
/// too and the comparison stayed green over both. **A duplicated classifier
/// cannot refute the classifier it duplicates.** It is still a duplicate — the
/// point of this file is that a generated skeleton can express the hand-written
/// servant, and calling the private original would measure nothing — but it is
/// now a duplicate that has to be *kept* in step by
/// [`the_two_def_kinds_agree_on_every_entry_the_subject_declares`], which
/// drives both over every entry rather than trusting the eye.
///
/// No `_` arm, for the same reason the original has none: a new `TypeCode`
/// variant must break the build here too.
fn def_kind_of(registry: &Registry, id: &str) -> DefinitionKind {
    match registry.get(id) {
        Some(Entry::Interface(i)) => {
            if i.abstract_interface {
                DefinitionKind::dk_AbstractInterface
            } else {
                DefinitionKind::dk_Interface
            }
        }
        Some(Entry::Const { .. }) => DefinitionKind::dk_Constant,
        Some(Entry::Type(tc)) => match tc {
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
            | TypeCode::WChar => DefinitionKind::dk_Primitive,
            // Unbounded is a PrimitiveDef, bounded is a StringDef (§14.5.15).
            TypeCode::String(0) => DefinitionKind::dk_Primitive,
            TypeCode::String(_) => DefinitionKind::dk_String,
            TypeCode::WString(0) => DefinitionKind::dk_Primitive,
            TypeCode::WString(_) => DefinitionKind::dk_Wstring,
            TypeCode::Fixed { .. } => DefinitionKind::dk_Fixed,
            TypeCode::Sequence { .. } => DefinitionKind::dk_Sequence,
            TypeCode::Array { .. } => DefinitionKind::dk_Array,
            TypeCode::ObjRef { .. } => DefinitionKind::dk_Interface,
            TypeCode::Struct { .. } => DefinitionKind::dk_Struct,
            TypeCode::Union { .. } => DefinitionKind::dk_Union,
            TypeCode::Enum { .. } => DefinitionKind::dk_Enum,
            TypeCode::Alias { .. } => DefinitionKind::dk_Alias,
            TypeCode::Except { .. } => DefinitionKind::dk_Exception,
            TypeCode::Value { .. } => DefinitionKind::dk_Value,
            TypeCode::Native { .. } => DefinitionKind::dk_Native,
            TypeCode::AbstractInterface { .. } => DefinitionKind::dk_AbstractInterface,
            TypeCode::Recursive(_) => DefinitionKind::dk_none,
        },
        None => DefinitionKind::dk_none,
    }
}

/// The `Repository` object: the root key, and nothing else.
struct RepositoryFacade {
    registry: Registry,
    /// For minting an `InterfaceDef` reference to an interface entry.
    defs: InterfaceDefRefs,
    /// For minting the weaker `Contained` reference to everything else, so a
    /// client that narrows locally narrows to something we actually serve.
    others: ContainedRefs,
}

impl RepositoryFacade {
    fn entry_reference(&self, id: &str) -> ObjRef {
        match self.registry.get(id) {
            None => InterfaceDefRefs::nil(),
            Some(Entry::Interface(_)) => self.defs.reference(id),
            Some(_) => self.others.reference(id),
        }
    }
}

impl RepositoryServant for RepositoryFacade {
    fn knows(&self, at: &RepositoryTarget<'_>) -> bool {
        at.is_default()
    }

    fn def_kind(&mut self, _at: &RepositoryTarget<'_>) -> Result<DefinitionKind, RepositoryFault> {
        Ok(DefinitionKind::dk_Repository)
    }

    fn lookup_id(
        &mut self,
        _at: &RepositoryTarget<'_>,
        search_id: String,
    ) -> Result<ObjRef, RepositoryFault> {
        Ok(self.entry_reference(&search_id))
    }

    /// The policy refusal, and the reason this operation is in the contract at
    /// all: `BAD_OPERATION` would say "no such operation", which invites a
    /// retry against another reference. `NO_PERMISSION` says the operation
    /// exists and the answer is no.
    fn create_module(
        &mut self,
        _at: &RepositoryTarget<'_>,
        _new_id: String,
        _new_name: String,
        _new_version: String,
    ) -> Result<ObjRef, RepositoryFault> {
        Err(rt::raise::other(ifr::NO_PERMISSION).did_not_run().into())
    }
}

/// An `InterfaceDef` per interface entry.
struct InterfaceDefFacade {
    registry: Registry,
    defs: InterfaceDefRefs,
}

impl InterfaceDefServant for InterfaceDefFacade {
    /// Every derived key naming a registered *interface*, and no other. This
    /// is what makes the three skeletons disjoint inside one key space.
    fn knows(&self, at: &InterfaceDefTarget<'_>) -> bool {
        !at.is_default() && self.registry.interface(at.oid()).is_some()
    }

    fn def_kind(
        &mut self,
        at: &InterfaceDefTarget<'_>,
    ) -> Result<DefinitionKind, InterfaceDefFault> {
        Ok(def_kind_of(&self.registry, at.oid()))
    }

    fn id(&mut self, at: &InterfaceDefTarget<'_>) -> Result<String, InterfaceDefFault> {
        Ok(at.oid().to_owned())
    }

    fn name(&mut self, at: &InterfaceDefTarget<'_>) -> Result<String, InterfaceDefFault> {
        Ok(name_of(&self.registry, at.oid()))
    }

    fn absolute_name(&mut self, at: &InterfaceDefTarget<'_>) -> Result<String, InterfaceDefFault> {
        Ok(absolute_name_of(&self.registry, at.oid()))
    }

    /// The third element of the `Contained` triple, and the reason the contract
    /// grew an attribute: `_get_version` was absent from `ifr.rs` while
    /// `_set_version` was refused, on data every repository id carries.
    fn version(&mut self, at: &InterfaceDefTarget<'_>) -> Result<String, InterfaceDefFault> {
        Ok(contained_of(&self.registry, at.oid()).2)
    }

    fn base_interfaces(
        &mut self,
        at: &InterfaceDefTarget<'_>,
    ) -> Result<Vec<ObjRef>, InterfaceDefFault> {
        let Some(iface) = self.registry.interface(at.oid()) else {
            return Err(rt::SystemException::bad_operation().into());
        };
        // `sibling` would do here — a base of an interface is another
        // `InterfaceDef` — but going through the same `defs` the repository
        // mints with keeps one answer for "what is a reference to an entry".
        Ok(iface.bases.iter().map(|b| self.defs.reference(b)).collect())
    }

    /// The IR operation, which is a different question from `_is_a`: "does the
    /// interface I describe derive from this id", not "is this reference one".
    fn is_a(
        &mut self,
        at: &InterfaceDefTarget<'_>,
        derived_from: String,
    ) -> Result<bool, InterfaceDefFault> {
        if self.registry.interface(at.oid()).is_none() {
            return Err(rt::SystemException::bad_operation().into());
        }
        Ok(self.registry.is_a(at.oid(), &derived_from))
    }

    /// The operation the facade exists for: one call and a DII client has
    /// every signature it needs.
    ///
    /// Inherited members are included, each naming the interface that declares
    /// it, because the consumer is a client asking "what may I call" and an
    /// inherited operation is callable. Own members come first — the chain is
    /// the interface itself followed by its ancestors — and a name declared in
    /// both a derived interface and a base appears once, as the derived one.
    ///
    /// The order below is not incidental: this reply is compared byte for byte
    /// against `ifr.rs`, so the chain order, the `BTreeMap` order within each
    /// interface, and the first-wins de-duplication are all load-bearing.
    fn describe_interface(
        &mut self,
        at: &InterfaceDefTarget<'_>,
    ) -> Result<FullInterfaceDescription, InterfaceDefFault> {
        let id = at.oid();
        let Some(iface) = self.registry.interface(id) else {
            return Err(rt::SystemException::bad_operation().into());
        };
        let bases = iface.bases.clone();
        let (name, defined_in, version) = contained_of(&self.registry, id);

        let mut chain = vec![id.to_owned()];
        chain.extend(self.registry.ancestors(id));

        let mut operations = Vec::new();
        let mut attributes = Vec::new();
        let mut seen_ops: BTreeSet<String> = BTreeSet::new();
        let mut seen_attrs: BTreeSet<String> = BTreeSet::new();

        for owner in &chain {
            let Some(declarer) = self.registry.interface(owner) else { continue };
            let (_, _, owner_version) = ifr::split_repository_id(owner);
            let owner_path =
                owner.strip_suffix(&format!(":{owner_version}")).unwrap_or(owner).to_owned();

            for (op_name, sig) in &declarer.operations {
                if !seen_ops.insert(op_name.clone()) {
                    continue;
                }
                operations.push(OperationDescription {
                    name: op_name.clone(),
                    id: format!("{owner_path}/{op_name}:{owner_version}"),
                    defined_in: owner.clone(),
                    version: owner_version.clone(),
                    result: rt::TypeCodeVal(sig.returns.clone()),
                    mode: if sig.oneway {
                        OperationMode::OP_ONEWAY
                    } else {
                        OperationMode::OP_NORMAL
                    },
                    // The IDL `context` clause is not parsed, and inventing
                    // identifiers would be worse than reporting none.
                    contexts: Vec::new(),
                    parameters: sig
                        .params
                        .iter()
                        .map(|p| ParameterDescription {
                            name: p.name.clone(),
                            r#type: rt::TypeCodeVal(p.tc.clone()),
                            // `type_def` is an `IDLType` reference in the IDL.
                            // This facade mints no `IDLType` objects — the
                            // TypeCode is the complete answer — so nil is the
                            // truthful "there is no such object here".
                            type_def: ObjectHome::nil(),
                            mode: match p.direction {
                                ParamDirection::In => ParameterMode::PARAM_IN,
                                ParamDirection::Out => ParameterMode::PARAM_OUT,
                                ParamDirection::InOut => ParameterMode::PARAM_INOUT,
                            },
                        })
                        .collect(),
                    exceptions: sig
                        .raises
                        .iter()
                        .map(|x| exception_description(&self.registry, x))
                        .collect(),
                });
            }

            for (attr_name, sig) in &declarer.attributes {
                if !seen_attrs.insert(attr_name.clone()) {
                    continue;
                }
                attributes.push(AttributeDescription {
                    name: attr_name.clone(),
                    id: format!("{owner_path}/{attr_name}:{owner_version}"),
                    defined_in: owner.clone(),
                    version: owner_version.clone(),
                    r#type: rt::TypeCodeVal(sig.tc.clone()),
                    mode: if sig.readonly {
                        AttributeMode::ATTR_READONLY
                    } else {
                        AttributeMode::ATTR_NORMAL
                    },
                });
            }
        }

        Ok(FullInterfaceDescription {
            name: name.clone(),
            id: id.to_owned(),
            defined_in,
            version,
            operations,
            attributes,
            // Direct bases only, in declaration order — the same set
            // `_get_base_interfaces` answers with as references.
            base_interfaces: bases,
            r#type: rt::TypeCodeVal(TypeCode::ObjRef { id: id.to_owned(), name }),
        })
    }

    fn create_module(
        &mut self,
        _at: &InterfaceDefTarget<'_>,
        _new_id: String,
        _new_name: String,
        _new_version: String,
    ) -> Result<ObjRef, InterfaceDefFault> {
        Err(rt::raise::other(ifr::NO_PERMISSION).did_not_run().into())
    }
}

/// A `Contained` per non-interface entry: structs, unions, enums, aliases,
/// exceptions and constants.
struct ContainedFacade {
    registry: Registry,
}

impl ContainedServant for ContainedFacade {
    fn knows(&self, at: &ContainedTarget<'_>) -> bool {
        !at.is_default()
            && self.registry.get(at.oid()).is_some()
            && self.registry.interface(at.oid()).is_none()
    }

    fn def_kind(&mut self, at: &ContainedTarget<'_>) -> Result<DefinitionKind, ContainedFault> {
        Ok(def_kind_of(&self.registry, at.oid()))
    }

    fn id(&mut self, at: &ContainedTarget<'_>) -> Result<String, ContainedFault> {
        Ok(at.oid().to_owned())
    }

    fn name(&mut self, at: &ContainedTarget<'_>) -> Result<String, ContainedFault> {
        Ok(name_of(&self.registry, at.oid()))
    }

    fn absolute_name(&mut self, at: &ContainedTarget<'_>) -> Result<String, ContainedFault> {
        Ok(absolute_name_of(&self.registry, at.oid()))
    }

    fn version(&mut self, at: &ContainedTarget<'_>) -> Result<String, ContainedFault> {
        Ok(contained_of(&self.registry, at.oid()).2)
    }
}

// ── Harness ──────────────────────────────────────────────────────────────────

fn home() -> ObjectHome {
    ObjectHome::new(HOST, PORT, ROOT.to_vec())
}

/// The generated arrangement: three skeletons over one key space.
fn generated() -> Servants {
    let r = registry();
    let defs = InterfaceDefRefs::with_infix(home(), INFIX);
    let others = ContainedRefs::with_infix(home(), INFIX);
    Servants::new()
        .with(RepositorySkeleton::new(
            RepositoryRefs::with_infix(home(), INFIX),
            RepositoryFacade { registry: r.clone(), defs: defs.clone(), others: others.clone() },
        ))
        .with(InterfaceDefSkeleton::new(
            defs.clone(),
            InterfaceDefFacade { registry: r.clone(), defs },
        ))
        .with(ContainedSkeleton::new(others, ContainedFacade { registry: r }))
}

/// The oracle: the hand-written servant, unmodified.
fn hand_written() -> ifr::RepositoryServer {
    ifr::RepositoryServer::new(HOST, PORT, ROOT.to_vec(), registry())
}

fn entry_key(id: &str) -> Vec<u8> {
    hand_written().entry_key(id)
}

/// What a servant answered: bytes under a status, a system exception, or
/// "not my object".
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Body(rt::DispatchBody, Vec<u8>),
    Raised { id: String, minor: u32, completed: rt::Completion },
    Unknown,
}

/// Both byte orders, always. An encoder that only works native-endian passes
/// every local test and fails in the field; a `describe_interface` reply is
/// mostly `TypeCode`s, which are the deepest nesting either servant encodes.
const ORDERS: [Endian; 2] = [Endian::Big, Endian::Little];

fn request(endian: Endian, key: &[u8], operation: &str, args: &[&str]) -> Request {
    let wire = encode_request(Version::V1_2, endian, 1, key, operation, true, |e| {
        for a in args {
            e.put_str(a);
        }
    })
    .expect("encode request");
    let mut cursor: &[u8] = &wire;
    let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("frame");
    decode_request(msg).expect("decode request")
}

/// Drives one servant exactly as `Server` would: `knows` first, then a body
/// written into an encoder at the origin a real reply occupies.
fn ask<D: Dispatch>(
    d: &mut D,
    endian: Endian,
    key: &[u8],
    operation: &str,
    args: &[&str],
) -> Answer {
    if !d.knows(key) {
        return Answer::Unknown;
    }
    let req = request(endian, key, operation, args);
    let mut out = Encoder::continuing_at(endian, 24);
    match d.dispatch_body(&req, &mut out) {
        Ok(kind) => Answer::Body(kind, out.finish().expect("finish")),
        Err(ex) => Answer::Raised { id: ex.id, minor: ex.minor, completed: ex.completed },
    }
}

/// One case: an object key, an operation, and an argument.
struct Case {
    what: &'static str,
    key: Vec<u8>,
    op: &'static str,
    args: Vec<&'static str>,
}

fn case(what: &'static str, key: Vec<u8>, op: &'static str, args: &[&'static str]) -> Case {
    Case { what, key, op, args: args.to_vec() }
}

/// Every case the two servants must answer identically.
fn compared() -> Vec<Case> {
    let mut v = Vec::new();

    // ── the Repository object, at the root key ──
    for id in [
        ifr::REPOSITORY_ID,
        ifr::CONTAINER_ID,
        ifr::IR_OBJECT_ID,
        ifr::OBJECT_ID,
        ifr::INTERFACE_DEF_ID,
        ifr::CONTAINED_ID,
    ] {
        v.push(case("repository _is_a", ROOT.to_vec(), "_is_a", &[id]));
    }
    v.push(case("repository _non_existent", ROOT.to_vec(), "_non_existent", &[]));
    v.push(case("repository _get_def_kind", ROOT.to_vec(), "_get_def_kind", &[]));
    for id in [ACCOUNT, PARTY, MONEY, CURRENCY, DENIED, ABSENT] {
        v.push(case("lookup_id", ROOT.to_vec(), "lookup_id", &[id]));
    }
    v.push(case("repository create_module", ROOT.to_vec(), "create_module", CREATE_ARGS));
    // Not an operation of `Repository`: both must say BAD_OPERATION rather
    // than one of them describing the repository as though it were a type.
    v.push(case("repository describe_interface", ROOT.to_vec(), "describe_interface", &[]));

    // ── an InterfaceDef, at a derived key ──
    // `AUDITABLE` is abstract, which changes only `_get_def_kind`; every other
    // accessor must answer identically to a plain interface's, and that is the
    // half a `def_kind`-only test would not have checked.
    for id in [ACCOUNT, PARTY, AUDITABLE] {
        let key = entry_key(id);
        for want in [
            ifr::INTERFACE_DEF_ID,
            ifr::CONTAINER_ID,
            ifr::CONTAINED_ID,
            ifr::IDL_TYPE_ID,
            ifr::IR_OBJECT_ID,
            ifr::OBJECT_ID,
            ifr::REPOSITORY_ID,
        ] {
            v.push(case("interfacedef _is_a", key.clone(), "_is_a", &[want]));
        }
        v.push(case("interfacedef _non_existent", key.clone(), "_non_existent", &[]));
        for op in [
            "_get_def_kind",
            "_get_id",
            "_get_name",
            "_get_absolute_name",
            "_get_version",
            "_get_base_interfaces",
        ] {
            v.push(case("interfacedef accessor", key.clone(), op, &[]));
        }
        for want in [ACCOUNT, PARTY, ABSENT, ifr::OBJECT_ID] {
            v.push(case("interfacedef is_a", key.clone(), "is_a", &[want]));
        }
        // The whole `FullInterfaceDescription`, in one reply: strings, three
        // nested sequences of structs, an enum per member, a nil object
        // reference and four TypeCodes. `Account` carries a base, an inherited
        // readonly attribute, a struct return, a struct parameter and a
        // `raises`; `Party` carries none of those, which is the other half of
        // the range this one case covers.
        v.push(case("interfacedef describe_interface", key.clone(), "describe_interface", &[]));
        v.push(case("interfacedef create_module", key.clone(), "create_module", CREATE_ARGS));
        v.push(case("interfacedef unknown op", key.clone(), "no_such_operation", &[]));
    }

    // ── a Contained, at a derived key, for every non-interface entry kind ──
    // Five of these eight are new on 2026-08-25 and three of them — `VOUCHER`,
    // `LEDGER` and, through `Entry::Const`, `CEILING` — are the entry shapes
    // whose `_get_def_kind` this batch repaired. `STATEMENT` is here because an
    // alias was already answered correctly and a regression in the exhaustive
    // rewrite would be invisible without it.
    for id in [MONEY, CURRENCY, DENIED, CEILING, STATEMENT, VOUCHER, LEDGER] {
        let key = entry_key(id);
        v.push(case("contained _non_existent", key.clone(), "_non_existent", &[]));
        for op in ["_get_def_kind", "_get_id", "_get_name", "_get_absolute_name", "_get_version"] {
            v.push(case("contained accessor", key.clone(), op, &[]));
        }
        // `describe_interface` belongs to `InterfaceDef`. `ifr.rs` reaches its
        // own `describe_interface` and finds the entry is not an interface;
        // the generated `Contained` skeleton never declared the operation.
        // Two different routes, the same BAD_OPERATION — so this is compared
        // rather than listed as a divergence.
        v.push(case("contained describe_interface", key.clone(), "describe_interface", &[]));
    }

    // ── keys neither servant serves ──
    v.push(case("absent entry", entry_key(ABSENT), "_get_id", &[]));
    v.push(case("foreign key", b"somebody-elses".to_vec(), "_get_id", &[]));
    v.push(case("truncated root", b"ifr-roo".to_vec(), "_get_id", &[]));

    v
}

// ── The comparison ───────────────────────────────────────────────────────────

/// Every case in [`compared`], byte for byte, against `ifr.rs` itself, in both
/// byte orders.
#[test]
fn the_generated_skeleton_answers_what_the_hand_written_servant_answers() {
    let mut hand = hand_written();
    let mut from_idl = generated();
    let mut wrong = Vec::new();
    let mut measured = 0usize;
    for endian in ORDERS {
        for c in compared() {
            measured += 1;
            let want = ask(&mut hand, endian, &c.key, c.op, &c.args);
            let got = ask(&mut from_idl, endian, &c.key, c.op, &c.args);
            if want != got {
                wrong.push(format!(
                    "{} — {} {:?} on {:?} ({endian:?})\n    hand-written: {want:?}\n    \
                     generated:    {got:?}",
                    c.what,
                    c.op,
                    c.args,
                    String::from_utf8_lossy(&c.key)
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of the {measured} comparisons differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// The comparison would pass on two servants that both answered nothing, so
/// this is the negative control: the cases must actually exercise something.
#[test]
fn the_comparison_is_not_vacuous() {
    let cases = compared();
    // Pinned, not bounded: a matrix that silently shrinks is a comparison
    // that silently weakens, and ">= 60" would not have noticed. 82 → 131 on
    // 2026-08-25, when `SUBJECT` gained the five entry shapes whose
    // `_get_def_kind` answered `dk_none` — one more InterfaceDef key (17 cases)
    // and four more Contained keys (8 each) — and the reason the figure moves
    // is worth more than the figure: the two `def_kind`s carried the same
    // catch-all for five days *because the contract declared nothing that
    // reached it*.
    assert_eq!(cases.len(), 131, "the compared matrix changed size");
    let mut from_idl = generated();
    let (mut bodies, mut raised, mut unknown) = (0, 0, 0);
    let mut nonempty = 0;
    for c in &cases {
        match ask(&mut from_idl, Endian::Big, &c.key, c.op, &c.args) {
            Answer::Body(_, b) => {
                bodies += 1;
                if !b.is_empty() {
                    nonempty += 1;
                }
            }
            Answer::Raised { .. } => raised += 1,
            Answer::Unknown => unknown += 1,
        }
    }
    assert!(bodies > 40, "{bodies} bodies");
    assert_eq!(bodies, nonempty, "every compared body carries a value");
    assert!(raised >= 4, "{raised} raises");
    assert_eq!(unknown, 3, "three keys are served by neither");

    // And the oracle must not be answering trivially either: a `lookup_id`
    // reply has to be a real reference with the address in it.
    let mut hand = hand_written();
    let Answer::Body(_, body) = ask(&mut hand, Endian::Big, ROOT, "lookup_id", &[ACCOUNT]) else {
        panic!("lookup_id must answer with a body");
    };
    let mut d = rt::Decoder::new(&body, Endian::Big);
    let ior = orbweaver_giop::Ior::read_from(&mut d).expect("a reference");
    assert_eq!(ior.type_id, ifr::INTERFACE_DEF_ID);
    assert_eq!(ior.profiles[0].port, PORT);
    assert_eq!(ior.profiles[0].object_key, entry_key(ACCOUNT));
}

/// The generated `describe_interface` reply, read back by the **oracle's own
/// decoder**, in both byte orders.
///
/// Byte equality against `ifr.rs` is the measurement; this is the check that
/// the bytes both agree on are the right ones. `ifr::FullInterfaceDescription`
/// is what omniORB's IR client has already been measured against (see that
/// module's cross-ORB note), so decoding generated bytes with it is the
/// closest thing to a foreign reader available without a fixture.
#[test]
fn a_generated_description_decodes_as_the_oracles_own_struct() {
    let mut from_idl = generated();
    for endian in ORDERS {
        let key = entry_key(ACCOUNT);
        let Answer::Body(_, body) = ask(&mut from_idl, endian, &key, "describe_interface", &[])
        else {
            panic!("{endian:?}: describe_interface must answer with a body");
        };
        // The reply was written at the offset a real body occupies, so it is
        // read from there too — alignment is measured from the GIOP header.
        let mut d = rt::Decoder::new(&body, endian);
        let described =
            ifr::decode_full_interface_description(&mut d).expect("the oracle's own reader");
        assert_eq!(d.remaining(), 0, "{endian:?}: trailing bytes");

        assert_eq!(described.name, "Account");
        assert_eq!(described.id, ACCOUNT);
        assert_eq!(described.defined_in, "IDL:bank:1.0");
        assert_eq!(described.version, "1.0");
        assert_eq!(described.base_interfaces, [PARTY], "direct bases, in declaration order");
        assert_eq!(described.tc, TypeCode::ObjRef { id: ACCOUNT.into(), name: "Account".into() });

        let ops: Vec<&str> = described.operations.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(ops, ["balance", "withdraw"]);
        let withdraw = &described.operations[1];
        assert_eq!(withdraw.id, "IDL:bank/Account/withdraw:1.0");
        assert_eq!(withdraw.defined_in, ACCOUNT, "the interface that declares it");
        assert_eq!(withdraw.mode, ifr::OP_NORMAL);
        assert_eq!(withdraw.result, TypeCode::Void);
        assert!(withdraw.contexts.is_empty());
        let params: Vec<(&str, u32)> =
            withdraw.parameters.iter().map(|p| (p.name.as_str(), p.mode)).collect();
        assert_eq!(params, [("amount", ifr::PARAM_IN)]);
        assert!(
            matches!(&withdraw.parameters[0].tc, TypeCode::Struct { id, .. } if id == MONEY),
            "the parameter's TypeCode is the registry's, not a name: {:?}",
            withdraw.parameters[0].tc
        );
        assert_eq!(withdraw.exceptions.len(), 1);
        assert_eq!(withdraw.exceptions[0].id, DENIED);
        assert!(
            matches!(&withdraw.exceptions[0].tc, TypeCode::Except { members, .. }
                if members.len() == 1),
            "the exception carries its members"
        );

        // The inherited attribute, named with its declarer — the half a
        // servant that only walked its own interface would drop.
        let attrs: Vec<(&str, u32)> =
            described.attributes.iter().map(|a| (a.name.as_str(), a.mode)).collect();
        assert_eq!(attrs, [("party_id", ifr::ATTR_READONLY)]);
        assert_eq!(described.attributes[0].defined_in, PARTY);
        assert_eq!(described.attributes[0].id, "IDL:bank/Party/party_id:1.0");
    }
}

/// The two `def_kind`s agree on **every** entry the subject declares, and none
/// of them is `dk_none`.
///
/// [`compared`] drives `_get_def_kind` on a fixed handful of keys, which is why
/// the duplicate classifier below could carry the oracle's catch-all for five
/// days undetected: the two agreed *because they were the same wrong code*, and
/// the entries that would have shown it were not in the contract. This walks
/// the registry instead of a list, so an entry shape added to `SUBJECT` is
/// covered the moment it is declared.
///
/// The second assertion is the defect itself, stated as an invariant:
/// **`dk_none` means "no such definition", so no id the registry holds may
/// answer it** (D016 §5 B1). `TypeCode::Recursive` is the one shape that could,
/// and it cannot be an entry — it names one.
#[test]
fn the_two_def_kinds_agree_on_every_entry_the_subject_declares() {
    let r = registry();
    let mut hand = hand_written();
    let mut from_idl = generated();
    let ids: Vec<String> = r.ids().cloned().collect();
    assert!(ids.len() >= 10, "the subject declares {} entries", ids.len());

    let mut none_answered = Vec::new();
    let mut disagreed = Vec::new();
    for endian in ORDERS {
        for id in &ids {
            let key = entry_key(id);
            let want = ask(&mut hand, endian, &key, "_get_def_kind", &[]);
            let got = ask(&mut from_idl, endian, &key, "_get_def_kind", &[]);
            if want != got {
                disagreed.push(format!("{id} ({endian:?}):\n  hand: {want:?}\n  gen:  {got:?}"));
            }
            if def_kind_of(&r, id) == DefinitionKind::dk_none {
                none_answered.push(id.clone());
            }
        }
    }
    assert!(disagreed.is_empty(), "the two def_kinds differ:\n{}", disagreed.join("\n"));
    assert!(
        none_answered.is_empty(),
        "these registered entries were told they do not exist: {none_answered:?}"
    );
}

/// The contract declares every ordinal the specification does, and the facade
/// answers only ordinals the peer can name.
///
/// The generated enum stopped at `dk_Repository` (17) until 2026-08-25, which
/// is why `def_kind_of` above *could not* have been repaired on its own: there
/// was no `dk_Value` to return. A truncated enum also refuses a conformant
/// sender's ordinal in its generated `get`, which is the same defect pointed
/// the other way.
#[test]
fn the_contracts_definition_kind_is_the_specifications() {
    // `CORBA — Part 1: Interfaces, v3.4` §14.5.1, in declaration order.
    let spec = [
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
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/services/ir-subset.idl"),
    )
    .expect("the contract");
    let parsed = orbweaver_idl::parse(&src).expect("the contract parses");
    let mut r = Registry::new();
    r.load(&parsed).expect("loads");
    let Some(TypeCode::Enum { members, .. }) = r.typecode("IDL:omg.org/CORBA/DefinitionKind:1.0")
    else {
        panic!("the contract must declare DefinitionKind as an enum");
    };
    assert_eq!(members, &spec, "ordinal N is declaration position N; a gap renumbers the wire");

    // What the facade may *answer* is the narrower list, and the boundary is a
    // measurement rather than a preference: omniORB 4.3.4's `omniORB.ir_idl`
    // stubs declare 0..24 and would raise MARSHAL on 25. Nothing in this
    // workspace answers above 24 — the hand-written enum stops there.
    assert_eq!(ifr::DefinitionKind::AbstractInterface as u32, 24);
    assert_eq!(ifr::DefinitionKind::Value as u32, 20);
    assert_eq!(ifr::DefinitionKind::Native as u32, 23);
}

/// The contract really does declare the operation, with the members that once
/// could not be expressed.
///
/// The first entry of `NOT_COMPARED` used to say `describe_interface` was
/// missing because `::CORBA::TypeCode` loaded as `void`. If that regresses,
/// the generated struct's `type` member becomes `()` and this file stops
/// compiling — but a compile failure names no cause, so the cause is measured
/// here instead.
#[test]
fn the_contract_declares_describe_interface_over_real_typecodes() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/services/ir-subset.idl"),
    )
    .expect("the contract");
    let spec = orbweaver_idl::parse(&src).expect("the contract parses");
    let mut r = Registry::new();
    r.load(&spec).expect("loads");

    let sig = r
        .interface(ifr::INTERFACE_DEF_ID)
        .and_then(|i| i.operations.get("describe_interface"))
        .expect("InterfaceDef::describe_interface");
    let TypeCode::Struct { id, members, .. } = &sig.returns else {
        panic!("describe_interface must return a struct, not {:?}", sig.returns);
    };
    assert_eq!(id, "IDL:omg.org/CORBA/InterfaceDef/FullInterfaceDescription:1.0");
    let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "name",
            "id",
            "defined_in",
            "version",
            "operations",
            "attributes",
            "base_interfaces",
            "type"
        ],
        "member order is `ifr.rs`'s write_to order, and the comparison is byte equality"
    );
    assert!(
        matches!(members[7].tc, TypeCode::TypeCode),
        "measured: `::CORBA::TypeCode` must resolve to a TypeCode and not to {:?}. If this is \
         Void again, the silent-empty-reply defect is back and every member declared as a \
         TypeCode marshals nothing at all.",
        members[7].tc
    );
}

/// The generated arrangement reproduces the hand-written key scheme exactly —
/// which is why a minted reference can be compared as bytes at all.
#[test]
fn the_generated_key_space_is_the_hand_written_one() {
    let hand = hand_written();
    let defs = InterfaceDefRefs::with_infix(home(), INFIX);
    for id in [ACCOUNT, PARTY, MONEY, ABSENT] {
        assert_eq!(defs.key_of(id), hand.entry_key(id), "{id}");
        assert_eq!(defs.oid_of(&hand.entry_key(id)), Some(id), "{id}");
    }
    assert_eq!(defs.root_key(), hand.root_key());
    assert_eq!(defs.reference(ACCOUNT), ObjRef(Some(hand.entry_ior(ACCOUNT))));
    assert_eq!(
        ContainedRefs::with_infix(home(), INFIX).reference(MONEY),
        ObjRef(Some(hand.entry_ior(MONEY)))
    );
    assert_eq!(RepositoryRefs::with_infix(home(), INFIX).ior(""), hand.root_ior());
}

/// The three generated skeletons claim disjoint parts of one key space, which
/// is the only thing that makes `rt::Servants` deterministic here.
#[test]
fn the_three_skeletons_claim_disjoint_parts_of_one_key_space() {
    let r = registry();
    let defs = InterfaceDefRefs::with_infix(home(), INFIX);
    let others = ContainedRefs::with_infix(home(), INFIX);
    let repo = RepositorySkeleton::new(
        RepositoryRefs::with_infix(home(), INFIX),
        RepositoryFacade { registry: r.clone(), defs: defs.clone(), others: others.clone() },
    );
    let idef =
        InterfaceDefSkeleton::new(defs.clone(), InterfaceDefFacade { registry: r.clone(), defs });
    let cont = ContainedSkeleton::new(others, ContainedFacade { registry: r });

    let mut keys = vec![ROOT.to_vec()];
    keys.extend([ACCOUNT, PARTY, MONEY, CURRENCY, DENIED, ABSENT].map(entry_key));
    keys.push(b"foreign".to_vec());
    for key in keys {
        let claims = [repo.knows(&key), idef.knows(&key), cont.knows(&key)];
        let n = claims.iter().filter(|c| **c).count();
        assert!(n <= 1, "{:?} is claimed by {claims:?}", String::from_utf8_lossy(&key));
    }
}

// ── What is still hand-written, and why ──────────────────────────────────────

/// Where the generated skeleton and `ifr.rs` answer differently, with the cause.
///
/// Each entry is a `(what, why)` pair, and every one of them is re-measured by
/// [`the_divergences_are_the_ones_named_and_they_still_diverge`].
///
/// **This list got shorter on 2026-08-14, and that is the point of keeping it.**
/// `describe_interface` was its first entry for two successive reasons — first
/// that the registry loaded `::CORBA::TypeCode` as `void`, then, once that was
/// fixed, only that the corpus contract did not declare the operation. Neither
/// is true now: `corpus/services/ir-subset.idl` declares it and the whole
/// `FullInterfaceDescription` it returns, and the generated skeleton answers it
/// byte for byte in both byte orders (`compared`, and
/// [`a_generated_description_decodes_as_the_oracles_own_struct`]). An entry
/// whose stated cause is repaired is work, not a limitation, and the way to
/// tell the two apart is to write the cause down where it can be re-read.
///
/// It also got **one longer** on the same day, which is the other half of the
/// point: `ifr.rs` now answers `NO_IMPLEMENT` for the operations it defers, so
/// that a client can tell a deferral from an oversight without reading a
/// document — and a generated skeleton cannot say anything particular about an
/// operation its contract never declared. A new entry arriving because a
/// servant got *better* is not a regression, and the way to know which it is
/// is to write the cause down.
///
/// Of the five, the first two are one root cause, which is worth saying once:
///
/// **`ifr.rs` varies the identity of an object with the object, inside one key
/// space.** A key under `/ifr/` is an `InterfaceDef` or a `Contained` or a
/// `Contained`-plus-`IDLType` depending on what the registry holds under it,
/// and one hand-written `match` covers all three. A generated skeleton is per
/// interface: it can hold disjoint parts of a key space (that is what this file
/// does) but each part answers one fixed `_is_a` list and serves one fixed
/// operation set. Reproducing `ifr.rs`'s per-entry-kind `_is_a` exactly would
/// need an interface per entry kind in the contract, which the IR IDL does not
/// have.
///
/// The next two are not contract-level absences at all: both are *ordering*
/// and *name-shape* policies that `ifr.rs` applies before it looks at the
/// contract, and IDL has no clause for either. The last one is the deferral
/// answer described above.
const NOT_COMPARED: [(&str, &str); 5] = [
    (
        "_is_a on a non-interface entry",
        "`ifr.rs` answers IDLType true for a struct/enum/alias entry and false for a \
         constant, from one key space. The generated `Contained` skeleton answers one \
         fixed inheritance chain for every key it claims. Same root cause as the next.",
    ),
    (
        "create_module on a non-interface entry",
        "`ifr.rs` refuses every mutating operation with NO_PERMISSION on every key, by \
         matching the operation *name*. A generated skeleton answers BAD_OPERATION for an \
         operation the addressed interface does not declare — and `Contained` does not \
         derive from `Container`. The refusal is expressible (see `create_module` on the \
         Repository and InterfaceDef keys, which is compared and agrees); what is not \
         expressible is refusing an operation the contract never declared.",
    ),
    (
        "_set_ on a readonly attribute",
        "`ifr.rs` refuses `_set_id` with NO_PERMISSION by name-shape. A generated skeleton \
         emits no setter arm for a readonly attribute, so it answers BAD_OPERATION — which \
         is the more defensible answer, since the IR IDL declares no such operation either. \
         Recorded as a divergence rather than a defect on either side.",
    ),
    (
        "a malformed body under a refused operation",
        "`ifr.rs` refuses a mutating operation *before* it touches the argument body, so a \
         `create_module` with the wrong arguments is NO_PERMISSION. A generated skeleton \
         decodes the arguments the contract declares and then calls the servant, so the \
         same request is MARSHAL — the servant never runs and never gets to refuse. This \
         one cost the batch a round: the first comparison sent one string to a \
         three-argument operation and read the difference as a defect. Ordering a refusal \
         ahead of decoding is a policy the contract cannot state; a servant that needs it \
         must override `forward`-style, at the `Dispatch` level, or accept MARSHAL for a \
         request that was malformed anyway.",
    ),
    (
        "a deferred operation",
        "`ifr.rs` answers NO_IMPLEMENT for the IR operations it has decided not to implement — \
         `contents`, `lookup`, `lookup_name`, `describe_contents`, `describe`, \
         `_get_defined_in`, `_get_containing_repository`, `get_canonical_typecode`, \
         `get_primitive`, `_get_type` — so that a client can tell a deferral from an \
         oversight without reading a document. `corpus/services/ir-subset.idl` declares none \
         of them, so the generated skeleton answers BAD_OPERATION: the same root cause as \
         `create_module` on a `Contained`, since what is not expressible is answering \
         *anything particular* about an operation the contract never declared. Both answers \
         are right for what each servant knows.",
    ),
];

/// Every named divergence must still be one.
///
/// A list of known gaps that nobody re-measures is a list of things that were
/// once true — which is exactly what happened to this list's former first
/// entry. Every remaining one is measurable, so every remaining one is driven
/// here and required to disagree.
#[test]
fn the_divergences_are_the_ones_named_and_they_still_diverge() {
    assert_eq!(NOT_COMPARED.len(), 5);
    let mut hand = hand_written();
    let mut from_idl = generated();
    let key = entry_key(ACCOUNT);
    let big = Endian::Big;

    // 0 — a deferred operation. The oracle distinguishes a deferral from an
    // oversight on the wire; the generated skeleton only knows what the
    // contract declares, and the contract declares neither.
    for op in ["contents", "lookup_name", "get_primitive", "_get_type"] {
        match (ask(&mut hand, big, ROOT, op, &["x"]), ask(&mut from_idl, big, ROOT, op, &["x"])) {
            (Answer::Raised { id: a, .. }, Answer::Raised { id: b, .. }) => {
                assert_eq!(a, ifr::NO_IMPLEMENT, "{op}: deferred, and the wire says so");
                assert_eq!(b, rt::BAD_OPERATION, "{op}: the contract does not declare it");
            }
            other => panic!("{op}: both must refuse, differently: {other:?}"),
        }
    }

    // 1 and 2 — a non-interface entry.
    let money = entry_key(MONEY);
    assert_ne!(
        ask(&mut hand, big, &money, "_is_a", &[ifr::IDL_TYPE_ID]),
        ask(&mut from_idl, big, &money, "_is_a", &[ifr::IDL_TYPE_ID]),
    );
    assert_ne!(
        ask(&mut hand, big, &money, "create_module", CREATE_ARGS),
        ask(&mut from_idl, big, &money, "create_module", CREATE_ARGS),
    );

    // 3 — the setter of a readonly attribute.
    let (a, b) = (
        ask(&mut hand, big, &key, "_set_id", &[ABSENT]),
        ask(&mut from_idl, big, &key, "_set_id", &[ABSENT]),
    );
    match (a, b) {
        (Answer::Raised { id: a, .. }, Answer::Raised { id: b, .. }) => {
            assert_eq!(a, ifr::NO_PERMISSION);
            assert_eq!(b, rt::BAD_OPERATION);
        }
        other => panic!("both must refuse, differently: {other:?}"),
    }

    // 4 — a refused operation whose body does not match the contract. The
    // round this batch spent on it is the reason it is written down.
    let short = [ABSENT];
    match (
        ask(&mut hand, big, ROOT, "create_module", &short),
        ask(&mut from_idl, big, ROOT, "create_module", &short),
    ) {
        (Answer::Raised { id: a, .. }, Answer::Raised { id: b, .. }) => {
            assert_eq!(a, ifr::NO_PERMISSION, "refused before the body is read");
            assert_eq!(b, rt::MARSHAL, "the arguments are decoded before the servant is called");
        }
        other => panic!("both must refuse, differently: {other:?}"),
    }
    // With a well-formed body the two agree, which is what makes this an
    // ordering difference rather than a missing refusal.
    assert_eq!(
        ask(&mut hand, big, ROOT, "create_module", CREATE_ARGS),
        ask(&mut from_idl, big, ROOT, "create_module", CREATE_ARGS),
    );

    // And the entry that left: `describe_interface` must not merely be absent
    // from this list, it must actually agree. A gap removed from a list of
    // gaps is worth exactly as much as the measurement that replaced it.
    for endian in ORDERS {
        let want = ask(&mut hand, endian, &key, "describe_interface", &[]);
        assert!(matches!(want, Answer::Body(..)), "the oracle serves it");
        assert_eq!(want, ask(&mut from_idl, endian, &key, "describe_interface", &[]));
    }
}
