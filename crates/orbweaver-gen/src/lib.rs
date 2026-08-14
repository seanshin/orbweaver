//! Static generation: Rust client stubs from the registry.
//!
//! Stream B of `docs/PLAN.md` §7.3. The dynamic path is the **reference
//! implementation**: it is the one verified against two independent ORBs, so a
//! generated stub is correct exactly when it produces the same bytes the
//! dynamic path produces for the same values. That rule (§8: *static result
//! equals dynamic result*) is the oracle every batch answers to, and it is why
//! generation starts only now — there had to be something trustworthy to be
//! equal to.
//!
//! # What a generated file may contain
//!
//! Names, field order, discriminator literals, operation names — the facts of
//! one contract. **Never encoding rules.** Every marshalling decision is a call
//! into [`rt`], so the wire knowledge exists once; Phase 3 measured what
//! happens when it is duplicated (the `wstring` BOM failure), and a code
//! generator is a machine for duplicating things.
//!
//! # What is skipped, per item
//!
//! A type that reaches `fixed` is emitted as a comment naming §4.4, and
//! everything depending on it is skipped the same way. The generator's report
//! counts skips separately from failures: a deferred wire type is a decision,
//! not a defect.
//!
//! # Both halves of one contract
//!
//! Every interface produces a **client stub** (here) and a **server skeleton**
//! ([`skeleton`]). They are generated from the same [`OpShape`], so the
//! argument list a caller passes and the argument list a servant receives
//! cannot drift apart: there is one place that decides what an operation looks
//! like in Rust, and both sides read it.

#![deny(missing_docs)]

pub mod rt;
pub mod skeleton;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use orbweaver_giop::typecode::{TypeCode, UnionCase};
use orbweaver_registry::{AttributeSig, Entry, OperationSig, ParamDirection, Registry};

/// What one file's generation produced.
#[derive(Debug, Default)]
pub struct Generated {
    /// The Rust source.
    pub source: String,
    /// Items emitted.
    pub emitted: usize,
    /// Items skipped, with the reason (deferred wire types, constants).
    pub skipped: Vec<(String, String)>,
}

/// Rust keywords that need escaping when they appear as IDL identifiers.
const KEYWORDS: &[&str] = &[
    "as", "box", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "fn",
    "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "static", "struct", "trait", "type", "unsafe", "use", "where", "while", "async",
    "await", "self", "super",
];

pub(crate) fn ident(name: &str) -> String {
    // `self`/`super`/`crate` cannot be raw identifiers; suffix those instead.
    match name {
        "self" | "super" | "crate" => format!("{name}_"),
        n if KEYWORDS.contains(&n) => format!("r#{n}"),
        n => n.to_owned(),
    }
}

/// The scope path for a repository id, as Rust module segments.
///
/// **Not** the inversion of `repository_id()`, and it used to be. Under a
/// `#pragma prefix` the leading segments of an id are *identity*, not scope: an
/// id does not say how many of them are prefix, so splitting on `/` invents a
/// module. `IDL:acme.com/p01/Account:1.0` inverted naively emits
/// `pub mod acme.com`, which is not valid Rust — measured, not supposed.
///
/// So the answer comes from the registry, which recorded the qualified name
/// when it loaded the IDL, and the split is only the fallback for an id that
/// was never loaded from IDL at all — an ingested one, where the id genuinely
/// is all anybody knows. `names` is that table.
pub(crate) fn path_of_with(names: &NameTable, id: &str) -> Vec<String> {
    if let Some(qualified) = names.get(id) {
        return qualified.split("::").map(str::to_owned).collect();
    }
    path_of(id)
}

/// The fallback split, for ids with no recorded name.
///
/// Correct exactly when the id carries no prefix, which is why every caller
/// that can reach a registry goes through [`path_of_with`] instead.
pub(crate) fn path_of(id: &str) -> Vec<String> {
    id.trim_start_matches("IDL:")
        .rsplit_once(':')
        .map_or(id, |(p, _)| p)
        .split('/')
        .map(str::to_owned)
        .collect()
}

/// Repository id → qualified IDL name, taken from the registry at emit time.
///
/// Threaded rather than looked up on demand because `rust_type` runs deep in
/// type emission where no registry is in scope, and a *partly* prefix-aware
/// generator is worse than one that is not: the module would be named from the
/// qualified name and the cross-references from the id, so the two would
/// disagree and nothing would compile.
pub(crate) type NameTable = std::collections::BTreeMap<String, String>;

/// Builds the table from everything the registry has a recorded name for.
/// What every emission function needs and neither half can derive alone: the
/// crate root the generated paths hang from, and the qualified names only the
/// registry knows.
pub(crate) struct Cx<'a> {
    pub root: &'a str,
    pub names: NameTable,
}

impl Cx<'_> {
    /// The scope path for an id, registry-recorded where possible.
    pub fn path_of(&self, id: &str) -> Vec<String> {
        path_of_with(&self.names, id)
    }
}

pub(crate) fn name_table(registry: &Registry) -> NameTable {
    registry
        .ids()
        .filter_map(|id| registry.qualified_name(id).map(|q| (id.clone(), q.to_owned())))
        .collect()
}

pub(crate) fn rust_path(id: &str, cx: &Cx<'_>) -> String {
    let segs: Vec<String> = cx.path_of(id).iter().map(|s| ident(s)).collect();
    let root = cx.root;
    format!("crate::{root}::{}", segs.join("::"))
}

/// The Rust type for a `TypeCode`, or the reason there is none.
pub(crate) fn rust_type(tc: &TypeCode, cx: &Cx<'_>) -> Result<String, String> {
    Ok(match tc {
        TypeCode::Boolean => "bool".into(),
        TypeCode::Octet | TypeCode::Char => "u8".into(),
        TypeCode::WChar => "orbweaver_gen::rt::WChar".into(),
        TypeCode::Short => "i16".into(),
        TypeCode::UShort => "u16".into(),
        TypeCode::Long => "i32".into(),
        TypeCode::ULong => "u32".into(),
        TypeCode::LongLong => "i64".into(),
        TypeCode::ULongLong => "u64".into(),
        TypeCode::Float => "f32".into(),
        TypeCode::Double => "f64".into(),
        TypeCode::LongDouble => "orbweaver_gen::rt::LongDouble".into(),
        TypeCode::String(_) => "String".into(),
        TypeCode::WString(_) => "orbweaver_gen::rt::WString".into(),
        TypeCode::Any => "orbweaver_gen::rt::AnyVal".into(),
        TypeCode::Void | TypeCode::Null => "()".into(),
        TypeCode::ObjRef { .. } => "orbweaver_gen::rt::ObjRef".into(),
        TypeCode::Sequence { element, .. } => format!("Vec<{}>", rust_type(element, cx)?),
        TypeCode::Array { element, length } => {
            format!("[{}; {length}]", rust_type(element, cx)?)
        }
        TypeCode::Struct { id, .. }
        | TypeCode::Union { id, .. }
        | TypeCode::Enum { id, .. }
        | TypeCode::Except { id, .. }
        | TypeCode::Alias { id, .. } => rust_path(id, cx),
        TypeCode::Recursive(id) => rust_path(id, cx),
        TypeCode::Fixed { digits, scale } => {
            return Err(format!("fixed<{digits},{scale}> is deferred at wire level (§4.4)"));
        }
        other => return Err(format!("no static mapping for {other:?}")),
    })
}

/// `first`, `second`, … — the wire position of a member, for its doc comment.
///
/// Declaration order *is* the wire order, and the §5.3 differ proved on the
/// wire that swapping two members of the same size is a silent breaking change
/// omniORB decodes without complaint. Saying the position out loud in the
/// generated documentation is cheap; discovering it from a wrong answer is not.
pub(crate) fn nth(i: usize) -> String {
    const NAMES: &[&str] = &[
        "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth",
        "tenth",
    ];
    match NAMES.get(i) {
        Some(name) => (*name).to_owned(),
        None => format!("at position {}", i + 1),
    }
}

/// Writes `text` as a `///` block, one line per line.
///
/// Generated code is held to `#![deny(missing_docs)]` exactly as this crate is
/// — a generator that exempts its own output from the project's rules is a
/// generator that will be told to stop emitting the rules.
///
/// An empty `text` is a paragraph break, and it has to be special-cased
/// because `"".lines()` yields nothing at all: every `doc(s, "")` in this
/// crate silently emitted *nothing*, which is why generated documentation used
/// to run its paragraphs together. Rustdoc merely rendered that badly; clippy
/// calls it what it is — a list item continued by a paragraph that meant to be
/// separate — and generated code is held to the same lints as everything else.
pub(crate) fn doc(out: &mut String, text: &str) {
    if text.is_empty() {
        let _ = writeln!(out, "///");
        return;
    }
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            let _ = writeln!(out, "///");
        } else {
            let _ = writeln!(out, "/// {line}");
        }
    }
}

/// The documentation for one operation: `ai_desc` when the contract has one,
/// and an honest sentence when it does not.
pub(crate) fn op_doc(out: &mut String, wire_name: &str, annotations: &BTreeMap<String, String>) {
    match annotations.get("ai_desc") {
        Some(desc) => {
            doc(out, desc);
            doc(out, "");
            doc(out, &format!("`{wire_name}` on the wire."));
        }
        None => doc(
            out,
            &format!("`{wire_name}`. The contract carries no `ai_desc` for this operation."),
        ),
    }
}

/// What one operation looks like in Rust, decided once for both sides.
///
/// The client stub and the server skeleton read the same shape, so the
/// arguments a caller passes and the arguments a servant receives cannot drift
/// apart — the divergence a two-template generator produces first and notices
/// last.
pub(crate) struct OpShape {
    /// `, name: Type` fragments for a Rust parameter list, in declaration
    /// order: `in` and `inout`, which are what travels in the request.
    pub args: String,
    /// The same parameters as (ident, type), for code that needs them apart.
    pub ins: Vec<(String, String)>,
    /// The Rust return type: `()`, one type, or a tuple.
    pub ret_ty: String,
    /// Everything the reply carries, in reply order (§7.9.1): the declared
    /// result first when it is not `void`, then `out` and `inout` values in
    /// declaration order.
    pub rets: Vec<String>,
}

pub(crate) fn op_shape(sig: &OperationSig, cx: &Cx<'_>) -> Result<OpShape, String> {
    let mut args = String::new();
    let mut ins = Vec::new();
    for p in &sig.params {
        if matches!(p.direction, ParamDirection::In | ParamDirection::InOut) {
            let ty = rust_type(&p.tc, cx)?;
            let _ = write!(args, ", {}: {ty}", ident(&p.name));
            ins.push((ident(&p.name), ty));
        }
    }
    let mut rets: Vec<String> = Vec::new();
    if !matches!(sig.returns, TypeCode::Void) {
        rets.push(rust_type(&sig.returns, cx)?);
    }
    for p in &sig.params {
        if matches!(p.direction, ParamDirection::Out | ParamDirection::InOut) {
            rets.push(rust_type(&p.tc, cx)?);
        }
    }
    let ret_ty = match rets.len() {
        0 => "()".to_owned(),
        1 => rets[0].clone(),
        _ => format!("({})", rets.join(", ")),
    };
    Ok(OpShape { args, ins, ret_ty, rets })
}

/// Every operation and attribute an interface answers for, inherited ones
/// included.
///
/// Inheritance was already resolved for operations and, until this batch, not
/// for attributes — an asymmetry that would have shipped a skeleton refusing
/// `_get_` on an inherited attribute with `BAD_OPERATION`. One helper now
/// answers for both, so the two cannot diverge again.
pub(crate) fn resolved_members(
    registry: &Registry,
    id: &str,
) -> (BTreeMap<String, OperationSig>, BTreeMap<String, AttributeSig>) {
    let mut ops: BTreeMap<String, OperationSig> = BTreeMap::new();
    let mut attrs: BTreeMap<String, AttributeSig> = BTreeMap::new();
    let mut ids = vec![id.to_owned()];
    ids.extend(registry.ancestors(id));
    for iid in &ids {
        if let Some(i) = registry.interface(iid) {
            for (name, sig) in &i.operations {
                ops.entry(name.clone()).or_insert_with(|| sig.clone());
            }
            for (name, a) in &i.attributes {
                attrs.entry(name.clone()).or_insert_with(|| a.clone());
            }
        }
    }
    (ops, attrs)
}

/// The synthetic signature of an attribute's `_get_`.
pub(crate) fn getter_sig(a: &AttributeSig) -> OperationSig {
    OperationSig {
        returns: a.tc.clone(),
        params: Vec::new(),
        raises: Vec::new(),
        oneway: false,
        annotations: a.annotations.clone(),
    }
}

/// The synthetic signature of an attribute's `_set_`.
pub(crate) fn setter_sig(a: &AttributeSig) -> OperationSig {
    OperationSig {
        returns: TypeCode::Void,
        params: vec![orbweaver_registry::ParamSig {
            name: "value".into(),
            direction: ParamDirection::In,
            tc: a.tc.clone(),
            annotations: BTreeMap::new(),
        }],
        raises: Vec::new(),
        oneway: false,
        annotations: a.annotations.clone(),
    }
}

/// The discriminator: how to write it, read it, and spell one of its values.
struct Disc {
    put: &'static str,
    get: &'static str,
    ty: String,
}

fn disc_of(tc: &TypeCode) -> Result<Disc, String> {
    Ok(match tc {
        TypeCode::Long => Disc { put: "put_i32", get: "get_i32", ty: "i32".into() },
        TypeCode::ULong | TypeCode::Enum { .. } => {
            Disc { put: "put_u32", get: "get_u32", ty: "u32".into() }
        }
        TypeCode::Short => Disc { put: "put_i16", get: "get_i16", ty: "i16".into() },
        TypeCode::UShort => Disc { put: "put_u16", get: "get_u16", ty: "u16".into() },
        TypeCode::Boolean => Disc { put: "put_bool", get: "get_bool", ty: "bool".into() },
        TypeCode::Char | TypeCode::Octet => Disc { put: "put_u8", get: "get_u8", ty: "u8".into() },
        other => return Err(format!("unsupported union discriminator {other:?}")),
    })
}

/// A case label as a Rust literal of the discriminator's type.
fn label_literal(label: &[u8], disc: &TypeCode) -> String {
    let wide = |b: &[u8]| {
        let mut v: i64 = 0;
        for x in b {
            v = (v << 8) | i64::from(*x);
        }
        v
    };
    match disc {
        TypeCode::Boolean => if label.last() == Some(&1) { "true" } else { "false" }.into(),
        TypeCode::Long => format!("{}i32", wide(label) as i32),
        TypeCode::Short => format!("{}i16", wide(label) as i16),
        TypeCode::UShort => format!("{}u16", wide(label) as u16),
        TypeCode::Char | TypeCode::Octet => format!("{}u8", wide(label) as u8),
        _ => format!("{}u32", wide(label) as u32),
    }
}

/// Whether every type this one touches has a static mapping.
///
/// Skipping must cascade: a struct whose member is a `fixed` typedef would
/// otherwise emit cleanly and reference a type that was never written, moving
/// the failure from the generator's report to the consumer's compiler — with
/// the §4.4 reason lost on the way.
fn representable(tc: &TypeCode, visiting: &mut Vec<String>) -> Result<(), String> {
    match tc {
        TypeCode::Fixed { digits, scale } => {
            Err(format!("fixed<{digits},{scale}> is deferred at wire level (§4.4)"))
        }
        TypeCode::Sequence { element, .. } | TypeCode::Array { element, .. } => {
            representable(element, visiting)
        }
        TypeCode::Struct { id, members, .. } | TypeCode::Except { id, members, .. } => {
            if visiting.iter().any(|v| v == id) {
                return Ok(()); // recursion via sequence is legal and fine
            }
            visiting.push(id.clone());
            let r = members.iter().try_for_each(|m| {
                representable(&m.tc, visiting).map_err(|why| format!("member {}: {why}", m.name))
            });
            visiting.pop();
            r
        }
        TypeCode::Union { id, cases, .. } => {
            if visiting.iter().any(|v| v == id) {
                return Ok(());
            }
            visiting.push(id.clone());
            let r = cases.iter().try_for_each(|c| {
                representable(&c.tc, visiting).map_err(|why| format!("case {}: {why}", c.name))
            });
            visiting.pop();
            r
        }
        TypeCode::Alias { id, aliased, .. } => {
            if visiting.iter().any(|v| v == id) {
                return Ok(());
            }
            visiting.push(id.clone());
            let r = representable(aliased, visiting);
            visiting.pop();
            r
        }
        _ => Ok(()),
    }
}

fn interface_representable(registry: &Registry, id: &str) -> Result<(), String> {
    if registry.interface(id).is_none() {
        return Ok(());
    }
    // Inherited members too: both halves generate the resolved set, so the
    // representability check has to cover the resolved set or it would clear an
    // interface whose base contributes a deferred type.
    let (operations, attributes) = resolved_members(registry, id);
    for (name, sig) in &operations {
        representable(&sig.returns, &mut Vec::new())
            .map_err(|why| format!("operation {name} returns: {why}"))?;
        for p in &sig.params {
            representable(&p.tc, &mut Vec::new())
                .map_err(|why| format!("operation {name}, parameter {}: {why}", p.name))?;
        }
        // The skeleton marshals raised exceptions, so a `fixed` reachable only
        // through a `raises` clause has to cascade the skip too — otherwise the
        // interface generates and the exception type it names never does.
        for ex in &sig.raises {
            let Some(tc) = registry.typecode(ex) else {
                return Err(format!(
                    "operation {name} raises {ex}, which the registry has no type for"
                ));
            };
            representable(tc, &mut Vec::new())
                .map_err(|why| format!("operation {name} raises {ex}: {why}"))?;
        }
    }
    for (name, a) in &attributes {
        representable(&a.tc, &mut Vec::new()).map_err(|why| format!("attribute {name}: {why}"))?;
    }
    Ok(())
}

/// Generates one loaded registry as a Rust module body.
pub fn emit(registry: &Registry, root: &str) -> Generated {
    // The context is built here, once, because this is the only place that has
    // both halves: the caller's chosen root and the registry's recorded names.
    let cx = &Cx { root, names: name_table(registry) };
    let mut out = Generated::default();

    // Group items under their module path.
    let mut by_module: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
    let skip = |out: &mut Generated, id: &str, why: String| {
        out.skipped.push((id.to_owned(), why));
    };

    for id in registry.ids() {
        let path = cx.path_of(id);
        let (module, _name) = path.split_at(path.len() - 1);
        let code = match registry.get(id) {
            Some(Entry::Type(tc)) => {
                representable(tc, &mut Vec::new()).and_then(|()| emit_type(id, tc, cx))
            }
            // One contract, both halves: the caller's stub and the servant's
            // skeleton are emitted together, from the same resolved members.
            Some(Entry::Interface(_)) => interface_representable(registry, id).and_then(|()| {
                let stub = emit_interface(registry, id, cx)?;
                let skel = skeleton::emit_skeleton(registry, id, cx)?;
                Ok(format!("{stub}\n{skel}"))
            }),
            Some(Entry::Const { .. }) => {
                Err("constants are not generated yet: the registry records the type but not the \
                 value"
                    .to_owned())
            }
            None => continue,
        };
        match code {
            Ok(code) => {
                out.emitted += 1;
                by_module.entry(module.to_vec()).or_default().push(code);
            }
            Err(why) => skip(&mut out, id, why),
        }
    }

    let mut src = String::new();
    let _ = writeln!(src, "//! Generated by orbweaver-gen from the registry.");
    let _ = writeln!(src, "//!");
    let _ = writeln!(src, "//! Names and order only; every marshalling decision is a call into");
    let _ = writeln!(src, "//! `orbweaver_gen::rt`, so the wire knowledge exists once.");
    let _ = writeln!(src, "//!");
    let _ = writeln!(src, "//! Each interface yields a client stub, a servant trait and a");
    let _ = writeln!(src, "//! skeleton that adapts the trait to `orbweaver_gen::rt::Dispatch`.");
    // A formatter between the generator and the reader makes "what did this
    // template change do" unanswerable: the diff fills with rewrapping and the
    // one changed line hides in it. Generated output is reviewed against the
    // generator, so it declares its own layout final. rustc never sees this —
    // nothing sets the `rustfmt` cfg — so it costs the consumer nothing.
    let _ = writeln!(src, "#![cfg_attr(rustfmt, rustfmt::skip)]");
    // The generated file is held to the same two rules as everything under
    // `crates/`: no unsafe, and no undocumented public item. A generator whose
    // output is exempt from the project's rules teaches that the rules are
    // negotiable — and the output is the part a consumer actually reads.
    let _ = writeln!(src, "#![forbid(unsafe_code)]");
    let _ = writeln!(src, "#![deny(missing_docs)]");
    let _ = writeln!(src, "#![allow(non_camel_case_types, non_snake_case, dead_code)]");
    let _ = writeln!(src);
    for (id, why) in &out.skipped {
        let _ = writeln!(src, "// skipped {id}: {why}");
    }
    if !out.skipped.is_empty() {
        let _ = writeln!(src);
    }
    write_modules(&mut src, &by_module, &[], 0);
    out.source = src;
    out
}

fn write_modules(
    src: &mut String,
    by_module: &BTreeMap<Vec<String>, Vec<String>>,
    at: &[String],
    depth: usize,
) {
    let pad = "    ".repeat(depth);
    // Items at exactly this path.
    if let Some(items) = by_module.get(at) {
        for item in items {
            for line in item.lines() {
                let _ = writeln!(src, "{pad}{line}");
            }
            let _ = writeln!(src);
        }
    }
    // Child modules, one level down.
    let mut children: Vec<&String> = by_module
        .keys()
        .filter(|k| k.len() > at.len() && k.starts_with(at))
        .map(|k| &k[at.len()])
        .collect();
    children.sort();
    children.dedup();
    for child in children {
        let _ = writeln!(src, "{pad}/// IDL module `{child}`.");
        let _ = writeln!(src, "{pad}pub mod {} {{", ident(child));
        let _ = writeln!(src, "{pad}    use orbweaver_gen::rt::{{self, Cdr}};");
        let mut next = at.to_vec();
        next.push(child.clone());
        write_modules(src, by_module, &next, depth + 1);
        let _ = writeln!(src, "{pad}}}");
    }
}

fn emit_type(id: &str, tc: &TypeCode, cx: &Cx<'_>) -> Result<String, String> {
    match tc {
        TypeCode::Struct { name, members, .. } | TypeCode::Except { name, members, .. } => {
            let mut s = String::new();
            let kind = if matches!(tc, TypeCode::Except { .. }) { "exception" } else { "struct" };
            let _ = writeln!(s, "/// IDL {kind} `{id}`.");
            let _ = writeln!(s, "#[derive(Debug, Clone, PartialEq)]");
            let _ = writeln!(s, "pub struct {} {{", ident(name));
            for (i, m) in members.iter().enumerate() {
                let _ = writeln!(s, "    /// IDL member `{}`, marshalled {}.", m.name, nth(i));
                let _ = writeln!(s, "    pub {}: {},", ident(&m.name), rust_type(&m.tc, cx)?);
            }
            let _ = writeln!(s, "}}");
            let (ep, dp) = if members.is_empty() { ("_e", "_d") } else { ("e", "d") };
            let _ = writeln!(s, "impl Cdr for {} {{", ident(name));
            let _ = writeln!(
                s,
                "    fn put(&self, {ep}: &mut rt::Encoder) -> Result<(), rt::GiopError> {{"
            );
            for m in members {
                let _ = writeln!(s, "        self.{}.put(e)?;", ident(&m.name));
            }
            let _ = writeln!(s, "        Ok(())");
            let _ = writeln!(s, "    }}");
            let _ = writeln!(
                s,
                "    fn get({dp}: &mut rt::Decoder<'_>) -> Result<Self, rt::GiopError> {{"
            );
            let _ = writeln!(s, "        Ok(Self {{");
            for m in members {
                let _ = writeln!(s, "            {}: Cdr::get(d)?,", ident(&m.name));
            }
            let _ = writeln!(s, "        }})");
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s, "}}");
            Ok(s)
        }
        TypeCode::Enum { name, members, .. } => {
            let mut s = String::new();
            let _ = writeln!(s, "/// IDL enum `{id}`. The ordinal is what travels.");
            let _ = writeln!(s, "#[derive(Debug, Clone, Copy, PartialEq, Eq)]");
            let _ = writeln!(s, "pub enum {} {{", ident(name));
            for (i, m) in members.iter().enumerate() {
                let _ = writeln!(s, "    /// IDL enumerator `{m}`; ordinal {i} on the wire.");
                let _ = writeln!(s, "    {} = {i},", ident(m));
            }
            let _ = writeln!(s, "}}");
            let _ = writeln!(s, "impl Cdr for {} {{", ident(name));
            let _ = writeln!(
                s,
                "    fn put(&self, e: &mut rt::Encoder) -> Result<(), rt::GiopError> {{"
            );
            let _ = writeln!(s, "        e.put_u32(*self as u32);");
            let _ = writeln!(s, "        Ok(())");
            let _ = writeln!(s, "    }}");
            let _ = writeln!(
                s,
                "    fn get(d: &mut rt::Decoder<'_>) -> Result<Self, rt::GiopError> {{"
            );
            let _ = writeln!(s, "        Ok(match d.get_u32()? {{");
            for (i, m) in members.iter().enumerate() {
                let _ = writeln!(s, "            {i} => Self::{},", ident(m));
            }
            let _ = writeln!(
                s,
                "            _ => return Err(rt::GiopError::Decode(\"ordinal outside {name}; \
                 the sender may be built against a newer contract\")),"
            );
            let _ = writeln!(s, "        }})");
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s, "}}");
            Ok(s)
        }
        TypeCode::Union { name, discriminator, cases, default_index, .. } => {
            emit_union(id, name, discriminator, cases, *default_index, cx)
        }
        TypeCode::Alias { name, aliased, .. } => Ok(format!(
            "/// IDL typedef `{id}`.\npub type {} = {};",
            ident(name),
            rust_type(aliased, cx)?
        )),
        // A bare ObjRef entry is a forward-declared interface that was never
        // given a body in this file; the type alias keeps references to it
        // compilable.
        TypeCode::ObjRef { name, .. } => Ok(format!(
            "/// IDL interface `{id}` (reference type).\npub type {}Ref = rt::ObjRef;",
            ident(name)
        )),
        other => Err(match rust_type(other, cx) {
            Err(why) => why,
            Ok(_) => format!("unexpected top-level type {other:?}"),
        }),
    }
}

/// One member of a union, after multi-label expansion is folded back.
struct Branch<'a> {
    member: &'a str,
    tc: &'a TypeCode,
    labels: Vec<String>,
    is_default: bool,
}

fn emit_union(
    id: &str,
    name: &str,
    disc_tc: &TypeCode,
    cases: &[UnionCase],
    default_index: i32,
    cx: &Cx<'_>,
) -> Result<String, String> {
    let disc = disc_of(disc_tc)?;

    // The registry expands `case 2: case 3: T x;` into two cases sharing a
    // member name; fold them back into one branch with two labels, because the
    // Rust variant is the member, and the discriminator that selected it is a
    // fact the variant must then carry.
    let mut branches: Vec<Branch<'_>> = Vec::new();
    for (i, c) in cases.iter().enumerate() {
        let is_default = default_index >= 0 && i == default_index as usize;
        if let Some(b) =
            branches.iter_mut().find(|b| b.member == c.name && !b.is_default && !is_default)
        {
            b.labels.push(label_literal(&c.label, disc_tc));
            continue;
        }
        branches.push(Branch {
            member: &c.name,
            tc: &c.tc,
            labels: if is_default { Vec::new() } else { vec![label_literal(&c.label, disc_tc)] },
            is_default,
        });
    }
    let exhaustive = matches!(disc_tc, TypeCode::Boolean) && branches.len() == 2;
    let has_default = branches.iter().any(|b| b.is_default);

    let mut s = String::new();
    let _ = writeln!(s, "/// IDL union `{id}`. The discriminator travels first.");
    let _ = writeln!(s, "#[derive(Debug, Clone, PartialEq)]");
    let _ = writeln!(s, "pub enum {} {{", ident(name));
    for b in &branches {
        let ty = rust_type(b.tc, cx)?;
        if b.is_default {
            let _ = writeln!(
                s,
                "    /// IDL `default` branch `{}`; `d` is the discriminator",
                b.member
            );
            let _ = writeln!(s, "    /// that selected it, which no label implies.");
        } else if b.labels.len() > 1 {
            let _ = writeln!(
                s,
                "    /// IDL case `{}`, selected by {}; `d` records which.",
                b.member,
                b.labels.join(" or ")
            );
        } else {
            let _ = writeln!(s, "    /// IDL case `{}`, selected by {}.", b.member, b.labels[0]);
        }
        if b.is_default || b.labels.len() > 1 {
            // The discriminator is not implied by the variant, so it is carried.
            let _ = writeln!(s, "    {} {{", ident(b.member));
            let _ = writeln!(s, "        /// The discriminator value that selected this branch.");
            let _ = writeln!(s, "        d: {},", disc.ty);
            let _ = writeln!(s, "        /// The branch value.");
            let _ = writeln!(s, "        v: {ty},");
            let _ = writeln!(s, "    }},");
        } else {
            let _ = writeln!(s, "    {}({}),", ident(b.member), ty);
        }
    }
    if !has_default && !exhaustive {
        let _ = writeln!(s, "    /// A discriminator matching no case: legal, and the value");
        let _ = writeln!(s, "    /// is the discriminator alone.");
        let _ = writeln!(s, "    Unlisted_({}),", disc.ty);
    }
    let _ = writeln!(s, "}}");

    let _ = writeln!(s, "impl Cdr for {} {{", ident(name));
    let _ = writeln!(s, "    fn put(&self, e: &mut rt::Encoder) -> Result<(), rt::GiopError> {{");
    let _ = writeln!(s, "        match self {{");
    for b in &branches {
        if b.is_default || b.labels.len() > 1 {
            let _ = writeln!(s, "            Self::{} {{ d, v }} => {{", ident(b.member));
            let _ = writeln!(s, "                e.{}(*d);", disc.put);
            let _ = writeln!(s, "                v.put(e)?;");
            let _ = writeln!(s, "            }}");
        } else {
            let _ = writeln!(s, "            Self::{}(v) => {{", ident(b.member));
            let _ = writeln!(s, "                e.{}({});", disc.put, b.labels[0]);
            let _ = writeln!(s, "                v.put(e)?;");
            let _ = writeln!(s, "            }}");
        }
    }
    if !has_default && !exhaustive {
        let _ = writeln!(s, "            Self::Unlisted_(d) => e.{}(*d),", disc.put);
    }
    let _ = writeln!(s, "        }}");
    let _ = writeln!(s, "        Ok(())");
    let _ = writeln!(s, "    }}");

    let _ = writeln!(s, "    fn get(d: &mut rt::Decoder<'_>) -> Result<Self, rt::GiopError> {{");
    let _ = writeln!(s, "        let disc = d.{}()?;", disc.get);
    let _ = writeln!(s, "        Ok(match disc {{");
    for b in &branches {
        if b.is_default {
            continue;
        }
        let pat = b.labels.join(" | ");
        if b.labels.len() > 1 {
            let _ = writeln!(
                s,
                "            {pat} => Self::{} {{ d: disc, v: Cdr::get(d)? }},",
                ident(b.member)
            );
        } else {
            let _ = writeln!(s, "            {pat} => Self::{}(Cdr::get(d)?),", ident(b.member));
        }
    }
    if let Some(b) = branches.iter().find(|b| b.is_default) {
        let _ = writeln!(
            s,
            "            _ => Self::{} {{ d: disc, v: Cdr::get(d)? }},",
            ident(b.member)
        );
    } else if !exhaustive {
        let _ = writeln!(s, "            _ => Self::Unlisted_(disc),");
    }
    let _ = writeln!(s, "        }})");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "}}");
    Ok(s)
}

fn emit_interface(registry: &Registry, id: &str, cx: &Cx<'_>) -> Result<String, String> {
    if registry.interface(id).is_none() {
        return Err("not an interface".to_owned());
    }
    let name = cx.path_of(id).last().cloned().unwrap_or_default();

    let mut s = String::new();
    if let Some(desc) = registry.annotations(id).and_then(|a| a.get("ai_desc")) {
        doc(&mut s, desc);
        doc(&mut s, "");
    }
    let _ = writeln!(s, "/// Client stub for `{id}`.");
    let _ = writeln!(s, "///");
    let _ = writeln!(s, "/// Static twin of the dynamic path; §8 requires their bytes to be");
    let _ = writeln!(s, "/// identical, and the harness holds them to it.");
    let _ = writeln!(s, "///");
    let _ = writeln!(s, "/// Generic over the invoker on purpose (PLAN §7.4 I1): over a raw");
    let _ = writeln!(s, "/// `Connection` inside the trust boundary, or over the guarded");
    let _ = writeln!(s, "/// wrapper at it — a stub hard-wired to the transport would be the");
    let _ = writeln!(s, "/// §4.7 bypass in compiled form.");
    let _ = writeln!(s, "pub struct {}Client<C: rt::Invoker> {{", ident(&name));
    let _ = writeln!(s, "    /// What calls travel over.");
    let _ = writeln!(s, "    pub conn: C,");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s, "impl<C: rt::Invoker> {}Client<C> {{", ident(&name));
    let _ = writeln!(s, "    /// A stub over an open invoker.");
    let _ = writeln!(s, "    pub fn new(conn: C) -> Self {{ Self {{ conn }} }}");

    // Operations and attributes, inherited ones included: a stub less capable
    // than the dynamic invoker would fail the oracle before it failed a user.
    let (ops, attrs) = resolved_members(registry, id);
    for (op_name, sig) in &ops {
        emit_operation(&mut s, op_name, op_name, sig, cx)?;
    }
    for (attr, a) in &attrs {
        emit_operation(&mut s, &format!("_get_{attr}"), attr, &getter_sig(a), cx)?;
        if !a.readonly {
            let setter = setter_sig(a);
            emit_operation(&mut s, &format!("_set_{attr}"), &format!("set_{attr}"), &setter, cx)?;
        }
    }
    let _ = writeln!(s, "}}");
    Ok(s)
}

fn emit_operation(
    s: &mut String,
    wire_name: &str,
    rust_name: &str,
    sig: &OperationSig,
    cx: &Cx<'_>,
) -> Result<(), String> {
    // One shape, read by both halves: the parameters here are exactly the
    // parameters the servant trait receives.
    let shape = op_shape(sig, cx)?;
    let (params, ins, ret_ty) = (&shape.args, &shape.ins, &shape.ret_ty);

    let mut docs = String::new();
    op_doc(&mut docs, wire_name, &sig.annotations);
    for line in docs.lines() {
        let _ = writeln!(s, "    {line}");
    }
    let _ = writeln!(
        s,
        "    pub fn {}(&mut self{params}) -> Result<{ret_ty}, rt::GiopError> {{",
        ident(rust_name)
    );
    // Marshal into a probe first, so a bad argument is a local error rather
    // than a half-written message — the same rule the dynamic invoker follows.
    // No arguments, nothing that can fail, no probe.
    if !ins.is_empty() {
        let _ = writeln!(s, "        let mut __probe = rt::Encoder::new(self.conn.endian());");
        for (p, _) in ins {
            let _ = writeln!(s, "        {p}.put(&mut __probe)?;");
        }
        let _ = writeln!(s, "        __probe.finish().map_err(rt::GiopError::Cdr)?;");
    }
    // __e starts with an underscore, so an argless operation whose closure
    // never touches it raises no unused-variable warning — one spelling serves.
    let closure_arg = "|__e|";
    if sig.oneway {
        let _ = writeln!(s, "        self.conn.invoke_oneway(\"{wire_name}\", {closure_arg} {{");
        for (p, _) in ins {
            let _ = writeln!(s, "            let _ = {p}.put(__e);");
        }
        let _ = writeln!(s, "        }})");
        let _ = writeln!(s, "    }}");
        return Ok(());
    }
    let _ = writeln!(s, "        let __reply = self.conn.invoke(\"{wire_name}\", {closure_arg} {{");
    for (p, _) in ins {
        let _ = writeln!(s, "            let _ = {p}.put(__e);");
    }
    let _ = writeln!(s, "        }})?;");
    let mut reads: Vec<String> = Vec::new();
    if !shape.rets.is_empty() {
        let _ = writeln!(s, "        let mut __body = __reply.body()?;");
    }
    for i in 0..shape.rets.len() {
        let _ = writeln!(s, "        let __r{i} = Cdr::get(&mut __body)?;");
        reads.push(format!("__r{i}"));
    }
    match reads.len() {
        0 => {
            let _ = writeln!(s, "        let _ = __reply;");
            let _ = writeln!(s, "        Ok(())");
        }
        1 => {
            let _ = writeln!(s, "        Ok({})", reads[0]);
        }
        _ => {
            let _ = writeln!(s, "        Ok(({}))", reads.join(", "));
        }
    }
    let _ = writeln!(s, "    }}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A prefixed id must not become a module path. `#pragma prefix` puts
    /// identity in the leading segments, and inverting the id emitted
    /// `pub mod acme.com`, which is not valid Rust — measured with gen-corpus
    /// before this was fixed, not supposed.
    ///
    /// The wire string must keep the full id, because that is what the peer
    /// says; only the Rust scope comes from the qualified name.
    #[test]
    fn a_prefixed_id_names_a_module_by_its_scope_not_by_its_identity() {
        let out = generate(
            "#pragma prefix \"acme.com\"\nmodule p01 { interface Account { long balance(); }; };",
        );
        let code = out.source;
        assert!(code.contains("pub mod p01"), "{code}");
        assert!(!code.contains("acme.com {"), "a prefix segment became a module: {code}");
        assert!(!code.contains("mod acme"), "a prefix segment became a module: {code}");
        assert!(
            code.contains("IDL:acme.com/p01/Account:1.0"),
            "the wire id lost its prefix: {code}"
        );
    }

    fn generate(src: &str) -> Generated {
        let spec = orbweaver_idl::parse(src).expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        emit(&r, "g")
    }

    #[test]
    fn a_struct_becomes_a_struct_with_members_in_order() {
        let g = generate("module m { struct P { long px; long py; }; };");
        assert!(g.source.contains("pub struct P {"), "{}", g.source);
        let px = g.source.find("pub px: i32").expect("px");
        let py = g.source.find("pub py: i32").expect("py");
        assert!(px < py, "declaration order is the wire order");
    }

    #[test]
    fn multi_label_union_branches_carry_their_discriminator() {
        let g = generate(
            "module m { union U switch (long) { case 1: long one; case 2: case 3: string s; \
             default: boolean b; }; };",
        );
        assert!(g.source.contains("one(i32)"), "{}", g.source);
        let s_branch = g.source.split("s {").nth(1).expect("the multi-label branch");
        assert!(s_branch.contains("d: i32,") && s_branch.contains("v: String,"), "{s_branch}");
        let b_branch = g.source.split("b {").nth(1).expect("the default branch");
        assert!(b_branch.contains("d: i32,") && b_branch.contains("v: bool,"), "{b_branch}");
        assert!(g.source.contains("2i32 | 3i32 =>"), "{}", g.source);
        assert!(!g.source.contains("Unlisted_"), "a union with a default has no unlisted arm");
    }

    #[test]
    fn a_boolean_union_is_exhaustive_without_an_escape_arm() {
        let g = generate(
            "module m { union B switch (boolean) { case TRUE: long yes; case FALSE: octet no; }; };",
        );
        assert!(!g.source.contains("Unlisted_"), "{}", g.source);
        assert!(g.source.contains("true =>") && g.source.contains("false =>"), "{}", g.source);
    }

    #[test]
    fn fixed_is_skipped_with_the_plan_section_named() {
        let g = generate(
            "module m { typedef fixed<9,2> Amount; struct Invoice { Amount total; }; \
             interface Billing { Amount sum(in Amount a, in Amount b); }; };",
        );
        assert_eq!(g.skipped.len(), 3, "the skip must cascade: {:?}", g.skipped);
        assert!(g.skipped.iter().all(|(_, why)| why.contains("4.4")), "{:?}", g.skipped);
        assert!(!g.source.contains("struct Invoice"), "{}", g.source);
    }

    #[test]
    fn attributes_become_accessors_with_the_underscore_wire_names() {
        let g = generate(
            "module m { interface I { readonly attribute string label; attribute long n; }; };",
        );
        assert!(g.source.contains("invoke(\"_get_label\""), "{}", g.source);
        assert!(g.source.contains("invoke(\"_set_n\""), "{}", g.source);
        assert!(!g.source.contains("_set_label"), "readonly has no setter");
    }

    #[test]
    fn inherited_operations_appear_on_the_derived_stub() {
        let g = generate(
            "module m { interface Base { long ping(); }; interface Derived : Base { void own(); }; };",
        );
        let derived = g.source.split("pub struct DerivedClient").nth(1).expect("stub");
        assert!(derived.contains("fn ping"), "{derived}");
        assert!(derived.contains("fn own"), "{derived}");
    }

    /// IDL escapes a keyword with a leading underscore (`_loop` names the
    /// identifier `loop`); Rust escapes with `r#`. A name legal on one side
    /// and reserved on the other must survive the crossing.
    #[test]
    fn keywords_are_escaped_rather_than_emitted_raw() {
        let g = generate("module m { struct S { long _loop; }; };");
        assert!(g.source.contains("pub r#loop: i32"), "{}", g.source);
        assert!(g.source.contains("self.r#loop.put(e)?"), "{}", g.source);
    }

    /// `corpus/services/` holds whole services rather than type coverage, and
    /// it is not under `corpus/golden/` because the files in it carry identity
    /// pragmas that the golden corpus is deliberately free of. The harness's
    /// out-of-workspace build globs `corpus/golden/*.idl`, so without this
    /// nothing would notice a services contract that stopped generating.
    #[test]
    fn the_services_corpus_generates_with_nothing_skipped() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/services");
        let mut seen = 0usize;
        let mut failures = Vec::new();
        for entry in std::fs::read_dir(&root).expect("corpus/services") {
            let path = entry.expect("entry").path();
            if path.extension().is_none_or(|x| x != "idl") {
                continue;
            }
            seen += 1;
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            let src = std::fs::read_to_string(&path).expect("read");
            let spec = orbweaver_idl::parse(&src).expect("a service contract parses");
            let mut r = Registry::new();
            r.load(&spec).expect("a service contract loads");
            let g = emit(&r, "g");
            if !g.skipped.is_empty() {
                failures.push(format!("{stem}: skipped {:?}", g.skipped));
            }
            if g.emitted == 0 {
                failures.push(format!("{stem}: emitted nothing"));
            }
        }
        assert!(seen > 0, "corpus/services must not be empty");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn the_whole_golden_corpus_generates_with_only_the_deferred_files_skipping() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/golden");
        let mut failures = Vec::new();
        for entry in std::fs::read_dir(&root).expect("corpus") {
            let path = entry.expect("entry").path();
            if path.extension().is_none_or(|x| x != "idl") {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("read");
            let spec = orbweaver_idl::parse(&src).expect("golden parses");
            let mut r = Registry::new();
            if r.load(&spec).is_err() {
                continue;
            }
            let g = emit(&r, "g");
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            let deferred = stem.contains("deferred");
            // Constants are a named non-goal of this batch (the registry keeps
            // the type, not the value); everything else must generate.
            let unexpected: Vec<_> = g
                .skipped
                .iter()
                .filter(|(_, why)| !why.contains("constants are not generated"))
                .collect();
            if !deferred && !unexpected.is_empty() {
                failures.push(format!("{stem}: skipped {unexpected:?}"));
            }
            if g.emitted == 0 && !deferred {
                failures.push(format!("{stem}: emitted nothing"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
