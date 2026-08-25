//! A second target language: Python clients, from the same registry.
//!
//! # Why a second target exists at all
//!
//! Not because anybody asked for Python. A single target cannot distinguish
//! *the IDL mapping* from *what happened to be convenient in Rust*, and the
//! only way to find out which is which is to write the mapping twice against
//! the same [`Registry`]. Everything the Rust emitter got right by accident
//! shows up here as something that cannot be expressed the same way, and the
//! run record for this batch lists what that turned out to be.
//!
//! # What crosses, and where the wire is
//!
//! Nowhere near here. A generated Python module renders its arguments as
//! **AnyJSON v1** (`docs/PLAN.md` §4.5 — this project's own normative JSON ↔
//! CDR mapping) and hands them to the `orbweaver-py-bridge` process, which
//! performs the invocation through [`orbweaver_dynamic::invoke`] — the same
//! dynamic path that is the reference implementation for every other client in
//! this workspace. So a second target language did **not** buy a second ORB:
//! CDR, GIOP, alignment, byte order and codeset negotiation still exist
//! exactly once, in Rust.
//!
//! The seam is a process boundary rather than an FFI boundary on purpose, and
//! the alternatives are compared in `docs/decisions/D007-python-wire-seam.md`,
//! left PROPOSED because an extension module is a new dependency class and
//! this project does not adopt one of those by writing code.
//!
//! # What a generated Python file may contain
//!
//! Names, member order, discriminator labels, operation names, and a
//! **descriptor** per type — the facts of one contract. Never a conversion
//! rule: every one of those is a call into `_rt`, the hand-written runtime
//! this module ships beside the package it generates. That is the same rule
//! [`crate::rt`] enforces for Rust, and it is why the Python runtime is a
//! file on disk rather than a template string.
//!
//! # What is deliberately not here
//!
//! **Servants.** This is a client target. A Python servant would need the
//! bridge to accept connections and call *back* into Python, which doubles the
//! protocol and gives the seam a second direction to be wrong in, while the
//! Rust [`crate::skeleton`] already answers for the serving half of every
//! contract. The run record states this as a scope boundary with its reason
//! rather than leaving it to be discovered.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use orbweaver_giop::typecode::TypeCode;
use orbweaver_registry::{ConstValue, Entry, OperationSig, ParamDirection, Registry};

use crate::{Cx, name_table, resolved_members};

/// The hand-written client runtime, shipped verbatim beside every generated
/// package as `_rt.py`.
///
/// Public so that a consumer who assembles the package themselves — a build
/// script, a test — writes the same bytes the generator writes, rather than a
/// copy that drifts.
pub const RUNTIME: &str = include_str!("python_rt.py");

/// What one file's Python generation produced.
#[derive(Debug, Default)]
pub struct PythonPackage {
    /// Files, keyed by path relative to the package root: `__init__.py`,
    /// `_rt.py`, `bank/__init__.py`.
    ///
    /// A package of files rather than one module, because the OMG Python
    /// mapping maps an IDL module to a Python module and IDL modules nest.
    /// Flattening them into one file would put `bank::Account` and
    /// `audit::Account` in one namespace, where the second silently replaces
    /// the first.
    pub files: BTreeMap<String, String>,
    /// Items emitted.
    pub emitted: usize,
    /// Items skipped, with the reason.
    pub skipped: Vec<(String, String)>,
}

/// Python's reserved words, plus the two names a generated method binds.
///
/// `self` is not a Python keyword and *is* a legal IDL identifier, so an
/// operation with a parameter named `self` would rebind the receiver — a
/// silent wrong call rather than a syntax error, which is the worse of the two.
const PY_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield", "self", "cls",
];

/// An IDL identifier as a Python one.
///
/// A **leading** underscore, which is the OMG Python mapping's rule and what
/// `omniidl -bpython` produces: `lambda` becomes `_lambda`, the operation
/// `yield` becomes `_yield`. This emitter used a trailing one for a while, on
/// the reasoning that a leading underscore is Python's own convention for
/// "private" — a reasonable argument that the name oracle simply overruled.
/// The point of a language mapping is that two generators produce the same
/// names, and taste does not enter into it.
///
/// It cannot collide with the runtime's own attributes: those are `_idl_*`,
/// `_rt` and the union's `_d`/`_v`, and no Python keyword spells any of them.
pub(crate) fn py_ident(name: &str) -> String {
    if PY_KEYWORDS.contains(&name) { format!("_{name}") } else { name.to_owned() }
}

/// The Python spelling of an IDL identifier.
///
/// Public because the escaping is part of the mapping, not an implementation
/// detail: a caller looking up a generated method, and the oracle driving one,
/// both need to know that the operation `yield` is reached as `_yield` while
/// the name that travels is still `yield`.
pub fn python_name(idl: &str) -> String {
    py_ident(idl)
}

/// A Python string literal, always double-quoted, ASCII-safe by escaping.
fn py_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A docstring block at `indent`, or nothing when there is nothing to say.
///
/// Triple quotes inside the text are escaped rather than dropped: an `ai_desc`
/// is contract text written by somebody else, and a generator that can be made
/// to emit unparseable Python by a quote in a comment is a generator with an
/// injection bug.
fn docstring(out: &mut String, indent: &str, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    let text = text.replace('\\', "\\\\").replace("\"\"\"", "\\\"\\\"\\\"");
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() == 1 {
        let _ = writeln!(out, "{indent}\"\"\"{}\"\"\"", lines[0].trim_end());
        return;
    }
    let _ = writeln!(out, "{indent}\"\"\"{}", lines[0].trim_end());
    for line in &lines[1..] {
        if line.trim().is_empty() {
            let _ = writeln!(out);
        } else {
            let _ = writeln!(out, "{indent}{}", line.trim_end());
        }
    }
    let _ = writeln!(out, "{indent}\"\"\"");
}

/// The documentation for one item: its `ai_desc` when the contract carries one,
/// then the sentence that says what it is on the wire.
fn item_doc(annotations: Option<&BTreeMap<String, String>>, what: &str) -> String {
    match annotations.and_then(|a| a.get("ai_desc")) {
        Some(desc) => format!("{desc}\n\n{what}"),
        None => what.to_owned(),
    }
}

/// The AnyJSON descriptor for a type, or the reason there is none.
///
/// Public because a descriptor is the Python target's type language: a
/// consumer assembling a call by hand, and the oracle that drives generated
/// code from Rust, both need the same spelling the emitter writes — a second
/// one would be a second mapping.
///
/// Descriptors name other types by **repository id**, never by Python name.
/// IDL scopes are mutually recursive and Python has no forward declaration, so
/// a descriptor that named a class would make a module body depend on
/// definition order; an id needs nothing to exist yet and is resolved at the
/// moment of the call.
pub fn descriptor(tc: &TypeCode) -> Result<String, String> {
    Ok(match tc {
        TypeCode::Boolean => "\"boolean\"".into(),
        TypeCode::Octet => "\"octet\"".into(),
        TypeCode::Char => "\"char\"".into(),
        TypeCode::WChar => "\"wchar\"".into(),
        TypeCode::Short => "\"short\"".into(),
        TypeCode::UShort => "\"ushort\"".into(),
        TypeCode::Long => "\"long\"".into(),
        TypeCode::ULong => "\"ulong\"".into(),
        TypeCode::LongLong => "\"longlong\"".into(),
        TypeCode::ULongLong => "\"ulonglong\"".into(),
        TypeCode::Float => "\"float\"".into(),
        TypeCode::Double => "\"double\"".into(),
        TypeCode::LongDouble => "\"longdouble\"".into(),
        TypeCode::Any => "\"any\"".into(),
        TypeCode::Void | TypeCode::Null => "\"void\"".into(),
        TypeCode::String(bound) => format!("(\"string\", {bound})"),
        TypeCode::WString(bound) => format!("(\"wstring\", {bound})"),
        TypeCode::Sequence { element, bound } => {
            format!("(\"seq\", {}, {bound})", descriptor(element)?)
        }
        TypeCode::Array { element, length } => {
            format!("(\"array\", {}, {length})", descriptor(element)?)
        }
        TypeCode::ObjRef { id, .. } => format!("(\"objref\", {})", py_str(id)),
        TypeCode::Struct { id, .. }
        | TypeCode::Union { id, .. }
        | TypeCode::Enum { id, .. }
        | TypeCode::Except { id, .. }
        | TypeCode::Alias { id, .. }
        | TypeCode::Recursive(id) => format!("(\"ref\", {})", py_str(id)),
        TypeCode::Fixed { digits, scale } => return Err(crate::deferred_fixed(*digits, *scale)),
        // No descriptor, deliberately. `("objref", id)` was available and
        // wrong: the Python runtime would have marshalled an IOR where the
        // peer sends a value, and the bridge would have agreed with it,
        // because both halves were reading the same wrong registry.
        TypeCode::Value { name, id, .. } => return Err(crate::deferred_value(name, id)),
        TypeCode::AbstractInterface { name, id, .. } => {
            return Err(crate::deferred_abstract(name, id));
        }
        // Same argument, one step stronger: `("objref", id)` was what the
        // registry's `ObjRef` produced here, and there is no descriptor a
        // native could honestly have — omniORB's own Python back end ignores
        // the declaration and leaves the type mapping dangling.
        TypeCode::Native { name, id, .. } => return Err(crate::unmarshallable_native(name, id)),
        // D008: a TypeCode is a value, and its AnyJSON form is the structural
        // one. Python holds it as `_rt.TypeCode` — the document, kept whole so
        // relaying it is exact — and `_rt._desc_of` reads that document as a
        // descriptor, which is this function run backwards: `("ref", id)` for
        // anything with a body, synthesised when the package never declared it.
        TypeCode::TypeCode => "\"typecode\"".into(),
        other => return Err(format!("no AnyJSON form for {other:?}")),
    })
}

/// Whether every type this one reaches has an AnyJSON form.
///
/// The same cascade [`crate::representable`] runs for Rust, over a different
/// set of stopping points: what a Python client cannot express is not what a
/// Rust client cannot express, and merging the two checks would hide that.
fn crossable(tc: &TypeCode, visiting: &mut Vec<String>) -> Result<(), String> {
    match tc {
        TypeCode::Fixed { .. }
        | TypeCode::Value { .. }
        | TypeCode::AbstractInterface { .. }
        | TypeCode::Native { .. } => descriptor(tc).map(|_| ()),
        TypeCode::Sequence { element, .. } | TypeCode::Array { element, .. } => {
            crossable(element, visiting)
        }
        TypeCode::Struct { id, members, .. } | TypeCode::Except { id, members, .. } => {
            if visiting.iter().any(|v| v == id) {
                return Ok(());
            }
            visiting.push(id.clone());
            let r = members.iter().try_for_each(|m| {
                crossable(&m.tc, visiting).map_err(|why| format!("member {}: {why}", m.name))
            });
            visiting.pop();
            r
        }
        TypeCode::Union { id, cases, discriminator, .. } => {
            if visiting.iter().any(|v| v == id) {
                return Ok(());
            }
            visiting.push(id.clone());
            let r = crossable(discriminator, visiting).and_then(|()| {
                cases.iter().try_for_each(|c| {
                    crossable(&c.tc, visiting).map_err(|why| format!("case {}: {why}", c.name))
                })
            });
            visiting.pop();
            r
        }
        TypeCode::Alias { id, aliased, .. } => {
            if visiting.iter().any(|v| v == id) {
                return Ok(());
            }
            visiting.push(id.clone());
            let r = crossable(aliased, visiting);
            visiting.pop();
            r
        }
        _ => Ok(()),
    }
}

fn interface_crossable(registry: &Registry, id: &str) -> Result<(), String> {
    let Some(entry) = registry.interface(id) else {
        return Ok(());
    };
    // Same argument as `crate::interface_representable`: the declaration is
    // refused before its members are looked at.
    if entry.abstract_interface {
        return Err(crate::deferred_abstract(&crate::abstract_name(registry, id), id));
    }
    let (operations, attributes) = resolved_members(registry, id);
    for (name, sig) in &operations {
        crossable(&sig.returns, &mut Vec::new())
            .map_err(|why| format!("operation {name} returns: {why}"))?;
        for p in &sig.params {
            crossable(&p.tc, &mut Vec::new())
                .map_err(|why| format!("operation {name}, parameter {}: {why}", p.name))?;
        }
        for ex in &sig.raises {
            let Some(tc) = registry.typecode(ex) else {
                return Err(format!(
                    "operation {name} raises {ex}, which the registry has no type for"
                ));
            };
            crossable(tc, &mut Vec::new())
                .map_err(|why| format!("operation {name} raises {ex}: {why}"))?;
        }
    }
    for (name, a) in &attributes {
        crossable(&a.tc, &mut Vec::new()).map_err(|why| format!("attribute {name}: {why}"))?;
    }
    Ok(())
}

/// Which of the four passes an item belongs to.
///
/// Python executes a module body top to bottom, so anything a *statement*
/// evaluates must already exist. Descriptors defer every type reference to
/// call time, which leaves exactly one thing that cannot be deferred: a
/// constant whose value is an enumerator. Enums therefore come first, and the
/// order is a property of the emitter rather than of the registry's iteration
/// order — which is sorted by repository id and means nothing.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum Pass {
    Enums,
    Types,
    Consts,
    Interfaces,
}

/// Generates one loaded registry as a Python package.
///
/// `package` is the name the caller will import; it is used for the package
/// docstring only, since every internal reference is relative.
pub fn emit_python(registry: &Registry, package: &str) -> PythonPackage {
    let cx = &Cx { root: package, names: name_table(registry) };
    let mut out = PythonPackage::default();
    let mut by_module: BTreeMap<Vec<String>, Vec<(Pass, String, String)>> = BTreeMap::new();

    for id in registry.ids() {
        let path = cx.path_of(id);
        let (module, _name) = path.split_at(path.len() - 1);
        let code = match registry.get(id) {
            Some(Entry::Type(tc)) => crossable(tc, &mut Vec::new())
                .and_then(|()| emit_type(id, tc).map(|s| (pass_of(tc), s))),
            Some(Entry::Interface(_)) => interface_crossable(registry, id)
                .and_then(|()| emit_interface(registry, id, cx).map(|s| (Pass::Interfaces, s))),
            Some(Entry::Const { tc, value }) => {
                emit_const(registry, id, tc, value.as_ref(), module, cx).map(|s| (Pass::Consts, s))
            }
            None => continue,
        };
        match code {
            Ok((pass, code)) => {
                out.emitted += 1;
                by_module.entry(module.to_vec()).or_default().push((pass, id.clone(), code));
            }
            Err(why) => {
                // A skipped **interface** still owes its name to the runtime.
                //
                // Every reference to an interface is `("objref", id)` — a
                // descriptor with no body, whose TypeCode name `_rt` looks up
                // in `NAMES`. That table is filled by the emitted item, so an
                // interface that was skipped left the name absent, and
                // `_form_of` filled the slot with `""`: `struct Counter {
                // Teller window; }` inside an `any` went out as
                // `"IDL:gcdr/Teller:1.0" 00 00 00 01 00` where the Rust
                // emitter wrote `… 00 00 00 07 "Teller\0"`. Two targets, one
                // contract, different bytes — and the skip is the *reason* the
                // reference is all there is, so this is exactly the case that
                // must still name it.
                //
                // Not done for a skipped type: its descriptor is `("ref", id)`
                // and the runtime refuses an id it holds no type for, which is
                // a diagnosis rather than a wrong byte.
                if let Some(Entry::Interface(entry)) = registry.get(id)
                    && !entry.abstract_interface
                {
                    let name = path.last().cloned().unwrap_or_default();
                    let code = format!(
                        "#: IDL `{id}`, skipped here: {why}\n\
                         #: The name is registered anyway — a reference to it is still an\n\
                         #: `objref` whose TypeCode names it, and an unnamed TypeCode is a\n\
                         #: byte the Rust target does not write.\n\
                         _rt.register_name({}, {})\n",
                        py_str(id),
                        py_str(&name)
                    );
                    by_module.entry(module.to_vec()).or_default().push((
                        Pass::Interfaces,
                        id.clone(),
                        code,
                    ));
                }
                out.skipped.push((id.clone(), why));
            }
        }
    }

    // Every module that holds an item, plus every module on the way to one: a
    // package with a hole in the middle is not importable.
    let mut modules: Vec<Vec<String>> = Vec::new();
    for path in by_module.keys() {
        for i in 0..=path.len() {
            let prefix = path[..i].to_vec();
            if !modules.contains(&prefix) {
                modules.push(prefix);
            }
        }
    }
    modules.sort();

    for path in &modules {
        let children: Vec<String> = modules
            .iter()
            .filter(|m| m.len() == path.len() + 1 && m.starts_with(path))
            .map(|m| m[path.len()].clone())
            .collect();
        let items = by_module.get(path).cloned().unwrap_or_default();
        // The directory is the module's *Python* name, because that is what
        // the parent's `from . import …` asks for. It used to be the raw IDL
        // name while the import was escaped, so a module named for a Python
        // keyword wrote `lambda/` and imported `_lambda`: the package did not
        // import at all, for all 37 escaped names, and only in this position —
        // one function, two sites, one of them not calling it.
        let file = if path.is_empty() {
            "__init__.py".to_owned()
        } else {
            let dirs: Vec<String> = path.iter().map(|s| py_ident(s)).collect();
            format!("{}/__init__.py", dirs.join("/"))
        };
        out.files.insert(file, module_file(package, path, &children, &items, &out.skipped));
    }
    if out.files.is_empty() {
        // A file that produced nothing still produces a package: the consumer's
        // import must fail with the skip reasons in front of it, not with
        // "no such module".
        out.files
            .insert("__init__.py".to_owned(), module_file(package, &[], &[], &[], &out.skipped));
    }
    out.files.insert("_rt.py".to_owned(), RUNTIME.to_owned());
    out
}

fn pass_of(tc: &TypeCode) -> Pass {
    if matches!(tc, TypeCode::Enum { .. }) { Pass::Enums } else { Pass::Types }
}

/// One `__init__.py`: header, runtime import, child modules, then the items.
fn module_file(
    package: &str,
    path: &[String],
    children: &[String],
    items: &[(Pass, String, String)],
    skipped: &[(String, String)],
) -> String {
    let mut s = String::new();
    let scope = if path.is_empty() {
        format!("the global scope of `{package}`")
    } else {
        format!("IDL module `{}`", path.join("::"))
    };
    docstring(
        &mut s,
        "",
        &format!(
            "{scope}, generated by orbweaver-gen from the registry.\n\n\
             Names, member order and descriptors only; every conversion is a call into\n\
             `_rt`, which speaks AnyJSON v1 (docs/PLAN.md §4.5) to the bridge that owns\n\
             the wire. Do not edit: regeneration overwrites this file."
        ),
    );
    let _ = writeln!(s);
    // `..` per level, so the package is importable under any name the consumer
    // gives it — a generated absolute import would hard-code ours.
    let _ = writeln!(s, "from {} import _rt", ".".repeat(path.len() + 1));
    if !children.is_empty() {
        let _ = writeln!(s);
        for child in children {
            let _ = writeln!(s, "from . import {}", py_ident(child));
        }
    }
    if path.is_empty() && !skipped.is_empty() {
        let _ = writeln!(s);
        for (id, why) in skipped {
            let _ = writeln!(s, "# skipped {id}: {why}");
        }
    }
    let mut items: Vec<&(Pass, String, String)> = items.iter().collect();
    items.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    for (_, _, code) in items {
        let _ = writeln!(s);
        let _ = writeln!(s);
        let _ = write!(s, "{code}");
    }
    s
}

fn emit_type(id: &str, tc: &TypeCode) -> Result<String, String> {
    match tc {
        TypeCode::Struct { name, members, .. } | TypeCode::Except { name, members, .. } => {
            let is_exception = matches!(tc, TypeCode::Except { .. });
            let base = if is_exception { "_rt.UserException" } else { "_rt.Struct" };
            let kind = if is_exception { "exception" } else { "struct" };
            let mut s = String::new();
            let _ = writeln!(s, "class {}({base}):", py_ident(name));
            docstring(&mut s, "    ", &format!("IDL {kind} `{id}`."));
            let _ = writeln!(s, "    _idl_id = {}", py_str(id));
            let _ = writeln!(s, "    _idl_name = {}", py_str(name));
            // (the name on the wire, the attribute here, the type). The first
            // two differ exactly when the member's IDL name is a Python
            // keyword; a runtime that assumed they were the same would read
            // `lambda` off an object that holds `_lambda`.
            let _ = write!(s, "    _idl_members = (");
            for m in members {
                let _ = write!(
                    s,
                    "({}, {}, {}), ",
                    py_str(&m.name),
                    py_str(&py_ident(&m.name)),
                    descriptor(&m.tc)?
                );
            }
            let _ = writeln!(s, ")");
            let _ = writeln!(s);
            let args: Vec<String> = members.iter().map(|m| py_ident(&m.name)).collect();
            let _ = writeln!(
                s,
                "    def __init__(self{}):",
                args.iter().map(|a| format!(", {a}")).collect::<String>()
            );
            if members.is_empty() {
                let _ = writeln!(s, "        pass");
            }
            for (i, m) in members.iter().enumerate() {
                let _ = writeln!(
                    s,
                    "        self.{} = {}  # marshalled {}",
                    py_ident(&m.name),
                    args[i],
                    crate::nth(i)
                );
            }
            let _ = writeln!(s, "_rt.register({})", py_ident(name));
            Ok(s)
        }

        TypeCode::Enum { name, members, .. } => {
            let mut s = String::new();
            let _ = writeln!(s, "class {}(_rt.Enum):", py_ident(name));
            docstring(
                &mut s,
                "    ",
                &format!(
                    "IDL enum `{id}`.\n\n\
                     The enumerators are objects in this scope, as the OMG Python mapping\n\
                     puts them, and they cross by name: the ordinal is a wire detail."
                ),
            );
            let _ = writeln!(s, "    _idl_id = {}", py_str(id));
            let _ = writeln!(s, "    _idl_name = {}", py_str(name));
            let _ = write!(s, "    _idl_members = (");
            for m in members {
                let _ = write!(s, "{}, ", py_str(m));
            }
            let _ = writeln!(s, ")");
            let _ = writeln!(s, "_rt.register({})", py_ident(name));
            for (i, m) in members.iter().enumerate() {
                let _ = writeln!(
                    s,
                    "{} = _rt.EnumItem({}, {i}, {})",
                    py_ident(m),
                    py_str(m),
                    py_str(id)
                );
            }
            let _ = write!(s, "{}._items = {{", py_ident(name));
            for m in members {
                let _ = write!(s, "{}: {}, ", py_str(m), py_ident(m));
            }
            let _ = writeln!(s, "}}");
            Ok(s)
        }

        TypeCode::Union { name, discriminator, cases, default_index, .. } => {
            let mut s = String::new();
            // One branch per member, its labels gathered: the registry expands
            // `case 2: case 3:` into two cases sharing a member, and a class
            // holds the member once. Member names are unique within a union,
            // so the name is the branch.
            //
            // A branch that is both labelled and `default:` (`case 2: default:
            // string rest;`) is one case per label in the registry, the default
            // a case of its own with no label (as omniidl lists it: `(2,
            // rest)`, then the default `rest`, `default_index` on the latter);
            // here it is one branch that keeps its labels and is the default
            // as well. Until `corpus/golden/29` the default branch was written
            // with no labels regardless, so the TypeCode the runtime rebuilt
            // for an `any` had a labelless default where the registry's had 2,
            // and came back as different bytes. `json_label` has no value to
            // render for the default's empty label.
            //
            // The default's SLOT — how many of the branch's labels precede it
            // — is kept as well, because the runtime rebuilds the registry's
            // member list from this class and the default's member sits where
            // `default:` was written (first for `default: case 5: case 6:`,
            // last for `case 2: default:`); a rebuilt list with the default
            // elsewhere is a different `default_index` on the wire.
            let mut branches: Vec<(Vec<String>, &str, &TypeCode, Option<usize>)> = Vec::new();
            for (i, c) in cases.iter().enumerate() {
                let is_default = *default_index >= 0 && i == *default_index as usize;
                let label = if c.label.is_empty() {
                    None
                } else {
                    Some(json_label(&c.label, discriminator)?)
                };
                if let Some(b) = branches.iter_mut().find(|b| b.1 == c.name) {
                    if is_default {
                        b.3.get_or_insert(b.0.len());
                    }
                    b.0.extend(label);
                    continue;
                }
                let slot = is_default.then_some(0);
                branches.push((label.into_iter().collect(), &c.name, &c.tc, slot));
            }
            let _ = writeln!(s, "class {}(_rt.Union):", py_ident(name));
            docstring(
                &mut s,
                "    ",
                &format!(
                    "IDL union `{id}`.\n\n\
                     `_d` is the discriminator and `_v` the value, which is both the OMG\n\
                     mapping and §4.5's rule: the active branch is a fact about the value,\n\
                     never something to infer from which member happens to be set."
                ),
            );
            let _ = writeln!(s, "    _idl_id = {}", py_str(id));
            let _ = writeln!(s, "    _idl_name = {}", py_str(name));
            let _ = writeln!(s, "    _idl_disc = {}", descriptor(discriminator)?);
            let _ = writeln!(s, "    _idl_cases = (");
            for (labels, member, tc, _) in &branches {
                let _ = writeln!(
                    s,
                    "        (({}), {}, {}),",
                    labels.iter().map(|l| format!("{l}, ")).collect::<String>(),
                    py_str(member),
                    descriptor(tc)?
                );
            }
            let _ = writeln!(s, "    )");
            let default_at =
                branches.iter().position(|b| b.3.is_some()).map_or(-1i64, |i| i as i64);
            let _ = writeln!(s, "    _idl_default = {default_at}");
            let slot = branches.iter().find_map(|b| b.3).unwrap_or(0);
            let _ = writeln!(s, "    _idl_default_slot = {slot}");
            for (_, member, _, _) in &branches {
                let py = py_ident(member);
                let _ = writeln!(s);
                // `_rt.property`, not the builtin by its bare name: a class
                // body is a scope the *contract* writes into, so a branch
                // named `property` bound the name before the next branch's
                // decorator ran and Python answered `'property' object is not
                // callable`. A module-level item named `property` does it from
                // one scope further out. `_rt` is the one name in a generated
                // module that no IDL identifier can spell, since a leading
                // underscore is IDL's escape rather than a character.
                let _ = writeln!(s, "    @_rt.property");
                let _ = writeln!(s, "    def {py}(self):");
                let _ = writeln!(s, "        return self._branch({})", py_str(member));
                let _ = writeln!(s);
                let _ = writeln!(s, "    @{py}.setter");
                let _ = writeln!(s, "    def {py}(self, value):");
                let _ = writeln!(s, "        self._set_branch({}, value)", py_str(member));
            }
            let _ = writeln!(s, "_rt.register({})", py_ident(name));
            Ok(s)
        }

        TypeCode::Alias { name, aliased, .. } => {
            let mut s = String::new();
            docstring(
                &mut s,
                "",
                &format!(
                    "IDL typedef `{id}`.\n\n\
                     A typedef binds its name to the *descriptor* of the type it aliases,\n\
                     not to a class: IDL scopes are mutually recursive and Python has no\n\
                     forward declaration, so a name bound to a class would make this\n\
                     module's body depend on an order the registry does not promise. The\n\
                     aliased type keeps its own name and constructor."
                ),
            );
            let _ = writeln!(s, "{} = {}", py_ident(name), descriptor(aliased)?);
            // The name travels too: the TypeCode of an `any` carrying this
            // typedef names it, and the runtime rebuilds that TypeCode from
            // what is registered — a name it does not have is a name it
            // cannot write, and the id is not a place to derive one from.
            let _ = writeln!(
                s,
                "_rt.register_alias({}, {}, {})",
                py_str(id),
                descriptor(aliased)?,
                py_str(name)
            );
            Ok(s)
        }

        // Declared and never given a body here: a forward-declared interface,
        // or one of the constructs §4.4 defers (a `valuetype`, an abstract
        // interface), which the registry records the same way.
        //
        // The name is the IDL name, with **no `Ref` suffix**. The Rust emitter
        // adds one because Rust needs a distinct name for the alias, and
        // copying that here was a Rust artifact in a Python file — measured by
        // the omniidl name oracle, which calls this `Money` where we called it
        // `MoneyRef`. The descriptor is still a reference, because a reference
        // is the only thing the v1 wire carries for these.
        TypeCode::ObjRef { name, .. } => Ok(format!(
            "#: IDL `{id}`, declared and not defined in this file: a reference is all\n\
             #: the v1 wire carries for it (§4.4).\n\
             {} = (\"objref\", {})\n\
             _rt.register_name({}, {})\n",
            py_ident(name),
            py_str(id),
            py_str(id),
            py_str(name)
        )),

        other => Err(match descriptor(other) {
            Err(why) => why,
            Ok(_) => format!("unexpected top-level type {other:?}"),
        }),
    }
}

/// A union case label in its AnyJSON form.
///
/// The label travels as the scalar §4.5 defines for the discriminator's type —
/// a number, a boolean, or an enumerator's *name* — so that a class body needs
/// nothing to already exist. An enum-valued label written as a Python
/// expression would need its enumerator object, and that is the one reference
/// a descriptor cannot defer.
fn json_label(label: &[u8], disc: &TypeCode) -> Result<String, String> {
    // `u64`, so that an eight-octet label at the top of `unsigned long long`'s
    // range does not sign-extend its own top bit on the way in.
    let wide = |b: &[u8]| {
        let mut v: u64 = 0;
        for x in b {
            v = (v << 8) | u64::from(*x);
        }
        v
    };
    Ok(match disc {
        TypeCode::Boolean => if label.last() == Some(&1) { "True" } else { "False" }.into(),
        TypeCode::Long => format!("{}", wide(label) as i32),
        TypeCode::ULong => format!("{}", wide(label) as u32),
        TypeCode::Short => format!("{}", wide(label) as i16),
        TypeCode::UShort => format!("{}", wide(label) as u16),
        TypeCode::Char | TypeCode::Octet => format!("{}", wide(label) as u8),
        // `integer_type` in `switch_type_spec` reaches `long long`; these were
        // legal and refused until corpus/golden/33-const-values.idl wrote one.
        TypeCode::LongLong => format!("{}", wide(label) as i64),
        TypeCode::ULongLong => format!("{}", wide(label)),
        TypeCode::Enum { members, name, .. } => {
            let ordinal = wide(label) as u32 as usize;
            match members.get(ordinal) {
                Some(m) => py_str(m),
                None => return Err(format!("case label {ordinal} is not an enumerator of {name}")),
            }
        }
        other => return Err(format!("unsupported union discriminator {other:?}")),
    })
}

/// One IDL constant, as a module-level name.
fn emit_const(
    registry: &Registry,
    id: &str,
    tc: &TypeCode,
    value: Option<&ConstValue>,
    module: &[String],
    cx: &Cx<'_>,
) -> Result<String, String> {
    let Some(value) = value else {
        return Err("the registry could not evaluate its expression, and stores no value \
                    rather than a guess — see orbweaver_registry::ConstValue"
            .to_owned());
    };
    let name = cx.path_of(id).last().cloned().unwrap_or_default();
    let literal = const_literal(tc, value, module, cx)?;
    let mut s = String::new();
    docstring(&mut s, "", &item_doc(registry.annotations(id), &format!("IDL constant `{id}`.")));
    let _ = writeln!(s, "{} = {literal}", py_ident(&name));
    Ok(s)
}

fn const_literal(
    tc: &TypeCode,
    v: &ConstValue,
    module: &[String],
    cx: &Cx<'_>,
) -> Result<String, String> {
    let resolved = tc.resolve_alias();
    Ok(match (resolved, v) {
        (TypeCode::Boolean, ConstValue::Bool(b)) => {
            if *b {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        // `char` and `wchar` are one-character strings, which is the OMG
        // mapping; the seam turns a `char` back into the octet it is.
        (TypeCode::Char | TypeCode::WChar, ConstValue::Int(i)) => {
            let c = u32::try_from(*i)
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| format!("{i} is not a code point"))?;
            py_str(&c.to_string())
        }
        (
            TypeCode::Octet
            | TypeCode::Short
            | TypeCode::UShort
            | TypeCode::Long
            | TypeCode::ULong
            | TypeCode::LongLong
            | TypeCode::ULongLong,
            ConstValue::Int(i),
        ) => i.to_string(),
        (TypeCode::Float, ConstValue::Float(f)) => format!("{:?}", *f as f32),
        (TypeCode::Double, ConstValue::Float(f)) => format!("{f:?}"),
        (TypeCode::String(_) | TypeCode::WString(_), ConstValue::Str(s)) => py_str(s),
        (TypeCode::Enum { id, members, .. }, ConstValue::Enum { member, .. }) => {
            let ordinal = members.iter().position(|m| m == member).unwrap_or(0);
            // In this module the enumerator is a name; from another module it
            // is rebuilt from the two facts that identify it, because importing
            // a sibling module would impose an order IDL does not have.
            let owner = cx.path_of(id);
            if owner.len() == module.len() + 1 && owner[..module.len()] == *module {
                py_ident(member)
            } else {
                format!("_rt.EnumItem({}, {ordinal}, {})", py_str(member), py_str(id))
            }
        }
        (TypeCode::LongDouble, _) => {
            return Err("a `long double` constant has no Python literal: the value is 16 \
                        octets of an encoding no literal produces (§4.4)"
                .to_owned());
        }
        // Skipped with the value in hand, not for want of one — see the Rust
        // emitter's arm for what changed underneath this. Python is the target
        // that *could* carry it: `decimal.Decimal("12.5")` is exact. It is not
        // emitted because `_rt` is shipped verbatim and every generated module
        // would carry the import whether or not it has a `fixed` constant;
        // that is a decision about the emitted package, not about the value,
        // and it is recorded rather than taken here.
        (TypeCode::Fixed { .. }, v) => {
            let text = v.as_decimal().unwrap_or_else(|| "the value".to_owned());
            return Err(format!(
                "a `fixed` constant has no Python literal here: {text} is a decimal, and a \
                 `float` literal would change it. `decimal.Decimal({text:?})` would hold it \
                 exactly — the registry has the value; this emitter does not import `decimal`."
            ));
        }
        (tc, v) => return Err(format!("no Python literal for {v:?} declared as {tc:?}")),
    })
}

/// What one operation looks like in Python.
struct PyOp {
    /// The parameter list, `, name` per `in`/`inout` parameter.
    params: String,
    /// `(name, descriptor, value)` triples for the request.
    args: Vec<String>,
    /// The declared result's descriptor.
    returns: String,
    /// `(name, descriptor)` for each `out`/`inout`, in declaration order.
    outs: Vec<String>,
}

fn py_op(sig: &OperationSig) -> Result<PyOp, String> {
    let mut op = PyOp {
        params: String::new(),
        args: Vec::new(),
        returns: descriptor(&sig.returns)?,
        outs: Vec::new(),
    };
    for p in &sig.params {
        if matches!(p.direction, ParamDirection::In | ParamDirection::InOut) {
            let _ = write!(op.params, ", {}", py_ident(&p.name));
            op.args.push(format!(
                "({}, {}, {})",
                py_str(&p.name),
                descriptor(&p.tc)?,
                py_ident(&p.name)
            ));
        }
    }
    for p in &sig.params {
        if matches!(p.direction, ParamDirection::Out | ParamDirection::InOut) {
            op.outs.push(format!("({}, {})", py_str(&p.name), descriptor(&p.tc)?));
        }
    }
    Ok(op)
}

/// Every method a generated stub carries, keyed by the name that travels.
///
/// Operations and attribute accessors in one map, because on the wire they are
/// one thing: §7.9.1 says `_get_balance` is an operation, and an interface that
/// answered `balance` differently depending on whether the caller went through
/// an attribute or an operation would be two contracts. Inherited members are
/// included, which is the same resolved set the Rust stub is built from.
///
/// Public because two consumers need exactly this set and neither can derive
/// it safely: `orbweaver-py-bridge`, which must be able to route every name a
/// stub can send, and the oracle, which drives every method the emitter wrote.
pub fn client_operations(registry: &Registry, id: &str) -> BTreeMap<String, OperationSig> {
    let (mut ops, attrs) = resolved_members(registry, id);
    for (attr, a) in &attrs {
        ops.insert(format!("_get_{attr}"), crate::getter_sig(a));
        if !a.readonly {
            ops.insert(format!("_set_{attr}"), crate::setter_sig(a));
        }
    }
    ops
}

fn emit_interface(registry: &Registry, id: &str, cx: &Cx<'_>) -> Result<String, String> {
    if registry.interface(id).is_none() {
        return Err("not an interface".to_owned());
    }
    let name = cx.path_of(id).last().cloned().unwrap_or_default();
    let mut s = String::new();
    // No base class rather than `(object)`: the base is looked up in the module
    // the contract writes into, and `const long object = 1;` beside an
    // interface made the class statement call an `int` — `TypeError: int
    // expected at most 2 arguments, got 3`, measured 2026-08-25. Python 3 gives
    // the same class either way.
    let _ = writeln!(s, "class {}:", py_ident(&name));
    docstring(
        &mut s,
        "    ",
        &item_doc(
            registry.annotations(id),
            &format!(
                "Client stub for `{id}`.\n\n\
                 Takes an invoker — `_rt.Bridge` over a real target, `_rt.Loopback` in a\n\
                 test — and answers for every operation and attribute this interface has,\n\
                 inherited ones included. Inherited members are *flattened* rather than\n\
                 expressed as Python inheritance, which is the same resolved set the Rust\n\
                 stub carries: one interface cannot answer for two different sets\n\
                 depending on which target generated it."
            ),
        ),
    );
    let _ = writeln!(s, "    _idl_id = {}", py_str(id));
    let _ = writeln!(s, "    _idl_name = {}", py_str(&name));
    let _ = writeln!(s);
    let _ = writeln!(s, "    def __init__(self, invoker):");
    let _ = writeln!(s, "        self._invoker = invoker");

    for (op_name, sig) in client_operations(registry, id) {
        emit_operation(&mut s, &op_name, &op_name, &sig)?;
    }
    let _ = writeln!(s, "_rt.register({})", py_ident(&name));
    Ok(s)
}

fn emit_operation(
    s: &mut String,
    wire_name: &str,
    py_name: &str,
    sig: &OperationSig,
) -> Result<(), String> {
    let op = py_op(sig)?;
    let _ = writeln!(s);
    let _ = writeln!(s, "    def {}(self{}):", py_ident(py_name), op.params);
    let mut what = match sig.annotations.get("ai_desc") {
        Some(desc) => format!("{desc}\n\n`{wire_name}` on the wire."),
        None => format!("`{wire_name}`. The contract carries no `ai_desc` for this operation."),
    };
    if !op.outs.is_empty() {
        what.push_str(
            "\n\nAnswers a tuple: the declared result first when it is not void, then the\n\
             out and inout values in declaration order (§7.9.1).",
        );
    }
    if sig.oneway {
        what.push_str(
            "\n\nOneway: there is no reply at all (§9.4.1), so a failure at the target has\n\
             nowhere to travel and this call answers None the moment the request is sent.",
        );
    }
    docstring(s, "        ", &what);
    let _ = writeln!(s, "        return _rt.call(");
    let _ = writeln!(s, "            self._invoker, self._idl_id, {},", py_str(wire_name));
    let _ = writeln!(
        s,
        "            args=({}),",
        op.args.iter().map(|a| format!("{a}, ")).collect::<String>()
    );
    let _ = writeln!(s, "            returns={},", op.returns);
    if !op.outs.is_empty() {
        let _ = writeln!(
            s,
            "            outs=({}),",
            op.outs.iter().map(|o| format!("{o}, ")).collect::<String>()
        );
    }
    if !sig.raises.is_empty() {
        let ids: String = sig.raises.iter().map(|ex| format!("{}, ", py_str(ex))).collect();
        let _ = writeln!(s, "            raises=({ids}),");
    }
    if sig.oneway {
        let _ = writeln!(s, "            oneway=True,");
    }
    let _ = writeln!(s, "        )");
    Ok(())
}
