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
//! # Scope
//!
//! Derives what the wire needs. `#pragma prefix` and explicit `typeid` are not
//! yet honoured, so a repository id is `IDL:` plus the qualified name plus
//! `:1.0` — which is what every fixture in this project publishes, and what a
//! peer must agree with for `_is_a` to mean anything.

#![deny(missing_docs)]

pub mod diff;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use orbweaver_giop::typecode::{Member as TcMember, TypeCode, UnionCase as TcUnionCase};
use orbweaver_idl::ast::*;

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
    },
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

/// The registry.
#[derive(Debug, Clone, Default)]
pub struct Registry {
    entries: BTreeMap<RepositoryId, Entry>,
    /// Qualified IDL name (`spike::Echo`) to repository id.
    by_name: HashMap<String, RepositoryId>,
    /// SIDL annotations attached to a registered entry.
    annotations: BTreeMap<RepositoryId, BTreeMap<String, String>>,
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
        let mut builder = Builder { reg: self, names, in_progress: Vec::new() };
        builder.walk(&[], &spec.definitions);
        Ok(())
    }

    /// Every registered repository id, in sorted order.
    pub fn ids(&self) -> impl Iterator<Item = &RepositoryId> {
        self.entries.keys()
    }

    /// Looks an entry up by repository id.
    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.get(id)
    }

    /// Looks a repository id up by qualified IDL name, e.g. `spike::Echo`.
    pub fn id_of(&self, qualified: &str) -> Option<&RepositoryId> {
        self.by_name.get(qualified)
    }

    /// The `TypeCode` of a registered type.
    pub fn typecode(&self, id: &str) -> Option<&TypeCode> {
        match self.entries.get(id) {
            Some(Entry::Type(tc)) => Some(tc),
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

/// Builds `IDL:a/b/C:1.0` from a qualified path.
pub fn repository_id(path: &[String]) -> RepositoryId {
    format!("IDL:{}:1.0", path.join("/"))
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
    Interface,
    /// Neither is marshalled in v1 (PLAN §4.4).
    ValueType,
    /// Likewise.
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
                        self.defs.insert(key.clone(), DefRef::Interface);
                    }
                    if let Some(body) = &i.body {
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
                    for m in &e.members {
                        let mut ep = path.to_vec();
                        ep.push(m.text.clone());
                        self.paths.insert(qualified(&ep).to_lowercase(), ep);
                    }
                }
                Definition::Typedef(t) => {
                    self.defs.insert(key, DefRef::Typedef(t.clone()));
                }
                Definition::ValueType(_) => {
                    self.defs.insert(key, DefRef::ValueType);
                }
                Definition::Native(_) => {
                    self.defs.insert(key, DefRef::Native);
                }
                Definition::Const(_) => {}
            }
        }
    }

    /// Resolves a reference from `scope` outwards, IDL-style.
    fn resolve(&self, scope: &[String], name: &ScopedName) -> Option<Vec<String>> {
        let tail = name.parts.join("::").to_lowercase();
        if name.absolute {
            return self.paths.get(&tail).cloned();
        }
        // Try the innermost scope first, then each enclosing one.
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
}

impl Builder<'_> {
    fn walk(&mut self, path: &[String], defs: &[Definition]) {
        for d in defs {
            let mut p = path.to_vec();
            p.push(d.name().text.clone());
            let id = repository_id(&p);
            self.reg.by_name.insert(qualified(&p), id.clone());

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
                        self.reg.entries.insert(id, Entry::Interface(entry));
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
                    self.register_annotations(&id, &c.annotations);
                    self.reg.entries.insert(id, Entry::Const { tc });
                }
                other => {
                    if let Some(tc) = self.derive(&p) {
                        self.register_annotations(&id, annotations_of(other));
                        self.reg.entries.insert(id, Entry::Type(tc));
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

    fn interface_entry(&mut self, path: &[String], i: &Interface) -> InterfaceEntry {
        let scope = &path[..path.len() - 1];
        let bases = i
            .bases
            .iter()
            .filter_map(|b| self.names.resolve(scope, b).map(|p| repository_id(&p)))
            .collect();
        let Some(body) = &i.body else {
            return InterfaceEntry { bases, forward_only: true, ..InterfaceEntry::default() };
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
                            .filter_map(|r| self.names.resolve(path, r).map(|p| repository_id(&p)))
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
        InterfaceEntry { bases, operations, attributes, forward_only: false }
    }

    /// Derives the `TypeCode` of the definition at `path`.
    fn derive(&mut self, path: &[String]) -> Option<TypeCode> {
        let key = qualified(path);
        let id = repository_id(path);
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
                let cases = u
                    .cases
                    .iter()
                    .flat_map(|c| {
                        let tc = self.type_of(path, &c.member.ty);
                        let member_name =
                            c.member.names.first().map(|n| n.text.clone()).unwrap_or_default();
                        let labels: Vec<Vec<u8>> = if c.labels.is_empty() {
                            vec![Vec::new()]
                        } else {
                            c.labels.iter().map(|l| label_bytes(l, &disc)).collect()
                        };
                        labels
                            .into_iter()
                            .map(|label| TcUnionCase {
                                label,
                                name: member_name.clone(),
                                tc: tc.clone(),
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect();
                // Indexed against the *expanded* case list. A branch with
                // several labels becomes several cases, so any multi-label
                // branch before the default shifts it — computing the position
                // in the AST list pointed the default at the wrong case, and
                // everything consuming this TypeCode (the dynamic invoker's
                // branch selection, the peer we encode it for, the static
                // generator) inherited the error. Found by the generator's
                // union tests, which is stream B's oracle earning its keep.
                let default_index = {
                    let mut index = -1i32;
                    let mut expanded = 0usize;
                    for c in &u.cases {
                        if c.is_default {
                            index = expanded as i32;
                        }
                        expanded += c.labels.len().max(1);
                    }
                    index
                };
                TypeCode::Union {
                    id: id.clone(),
                    name,
                    discriminator: Box::new(disc),
                    default_index,
                    cases,
                }
            }
            DefRef::Interface => TypeCode::ObjRef { id: id.clone(), name },
            DefRef::ValueType | DefRef::Native => {
                // Neither is marshalled in v1 (PLAN §4.4). Registering the name
                // as an object reference keeps `_is_a` and catalogue lookups
                // working without implying a wire form we do not have.
                TypeCode::ObjRef { id: id.clone(), name }
            }
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
            TypeSpec::ValueBase => TypeCode::ObjRef {
                id: "IDL:omg.org/CORBA/ValueBase:1.0".into(),
                name: "ValueBase".into(),
            },
            TypeSpec::String(b) => TypeCode::String(b.as_deref().and_then(const_u32).unwrap_or(0)),
            TypeSpec::WString(b) => {
                TypeCode::WString(b.as_deref().and_then(const_u32).unwrap_or(0))
            }
            TypeSpec::Fixed { digits, scale } => TypeCode::Fixed {
                digits: const_u32(digits).unwrap_or(0) as u16,
                scale: const_u32(scale).unwrap_or(0) as i16,
            },
            TypeSpec::Sequence { element, bound } => TypeCode::Sequence {
                element: Box::new(self.type_of(scope, element)),
                bound: bound.as_deref().and_then(const_u32).unwrap_or(0),
            },
            TypeSpec::Named(n) => match self.names.resolve(scope, n) {
                Some(p) => self.derive(&p).unwrap_or(TypeCode::Void),
                // An unresolved name is a semantic error the checker already
                // reports; producing `void` here keeps the registry loadable
                // for tooling that wants to show what *did* resolve.
                None => TypeCode::Void,
            },
        }
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
    let v: i64 = match e {
        ConstExpr::Int(v) => *v,
        ConstExpr::Bool(b) => i64::from(*b),
        ConstExpr::Char(c) => *c as i64,
        ConstExpr::Unary { op: "-", operand } => match operand.as_ref() {
            ConstExpr::Int(v) => -*v,
            _ => 0,
        },
        // An enumerator label is its ordinal, which the discriminator's own
        // TypeCode carries.
        ConstExpr::Name(n) => match disc.resolve_alias() {
            TypeCode::Enum { members, .. } => {
                members.iter().position(|m| m == n.last()).unwrap_or(0) as i64
            }
            _ => 0,
        },
        _ => 0,
    };
    match disc.resolve_alias() {
        TypeCode::Boolean | TypeCode::Char | TypeCode::Octet => vec![v as u8],
        TypeCode::Short | TypeCode::UShort => (v as i16).to_be_bytes().to_vec(),
        TypeCode::LongLong | TypeCode::ULongLong => v.to_be_bytes().to_vec(),
        _ => (v as i32).to_be_bytes().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn a_forward_declaration_does_not_erase_a_body() {
        let r = load("module m { interface I { long f(); }; interface I; };");
        let i = r.interface("IDL:m/I:1.0").unwrap();
        assert!(!i.forward_only);
        assert!(i.operations.contains_key("f"));
    }
}
