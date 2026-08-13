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

#![deny(missing_docs)]

pub mod rt;

use std::collections::BTreeMap;
use std::fmt::Write as _;

use orbweaver_giop::typecode::{TypeCode, UnionCase};
use orbweaver_registry::{Entry, OperationSig, ParamDirection, Registry};

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

fn ident(name: &str) -> String {
    // `self`/`super`/`crate` cannot be raw identifiers; suffix those instead.
    match name {
        "self" | "super" | "crate" => format!("{name}_"),
        n if KEYWORDS.contains(&n) => format!("r#{n}"),
        n => n.to_owned(),
    }
}

/// `IDL:a/b/C:1.0` → `["a", "b", "C"]`.
///
/// The inversion of `repository_id()`, faithful because `#pragma prefix` is not
/// yet honoured anywhere in this project — stated in the registry's own docs.
fn path_of(id: &str) -> Vec<String> {
    id.trim_start_matches("IDL:")
        .rsplit_once(':')
        .map_or(id, |(p, _)| p)
        .split('/')
        .map(str::to_owned)
        .collect()
}

fn rust_path(id: &str, root: &str) -> String {
    let segs: Vec<String> = path_of(id).iter().map(|s| ident(s)).collect();
    format!("crate::{root}::{}", segs.join("::"))
}

/// The Rust type for a `TypeCode`, or the reason there is none.
fn rust_type(tc: &TypeCode, root: &str) -> Result<String, String> {
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
        TypeCode::Sequence { element, .. } => format!("Vec<{}>", rust_type(element, root)?),
        TypeCode::Array { element, length } => {
            format!("[{}; {length}]", rust_type(element, root)?)
        }
        TypeCode::Struct { id, .. }
        | TypeCode::Union { id, .. }
        | TypeCode::Enum { id, .. }
        | TypeCode::Except { id, .. }
        | TypeCode::Alias { id, .. } => rust_path(id, root),
        TypeCode::Recursive(id) => rust_path(id, root),
        TypeCode::Fixed { digits, scale } => {
            return Err(format!("fixed<{digits},{scale}> is deferred at wire level (§4.4)"));
        }
        other => return Err(format!("no static mapping for {other:?}")),
    })
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
    let Some(iface) = registry.interface(id) else { return Ok(()) };
    for (name, sig) in &iface.operations {
        representable(&sig.returns, &mut Vec::new())
            .map_err(|why| format!("operation {name} returns: {why}"))?;
        for p in &sig.params {
            representable(&p.tc, &mut Vec::new())
                .map_err(|why| format!("operation {name}, parameter {}: {why}", p.name))?;
        }
    }
    for (name, a) in &iface.attributes {
        representable(&a.tc, &mut Vec::new()).map_err(|why| format!("attribute {name}: {why}"))?;
    }
    Ok(())
}

/// Generates one loaded registry as a Rust module body.
pub fn emit(registry: &Registry, root: &str) -> Generated {
    let mut out = Generated::default();

    // Group items under their module path.
    let mut by_module: BTreeMap<Vec<String>, Vec<String>> = BTreeMap::new();
    let skip = |out: &mut Generated, id: &str, why: String| {
        out.skipped.push((id.to_owned(), why));
    };

    for id in registry.ids() {
        let path = path_of(id);
        let (module, _name) = path.split_at(path.len() - 1);
        let code = match registry.get(id) {
            Some(Entry::Type(tc)) => {
                representable(tc, &mut Vec::new()).and_then(|()| emit_type(id, tc, root))
            }
            Some(Entry::Interface(_)) => interface_representable(registry, id)
                .and_then(|()| emit_interface(registry, id, root)),
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
    let _ = writeln!(src, "// Generated by orbweaver-gen. Names and order only; every");
    let _ = writeln!(src, "// marshalling decision is a call into orbweaver_gen::rt.");
    for (id, why) in &out.skipped {
        let _ = writeln!(src, "// skipped {id}: {why}");
    }
    let _ = writeln!(src, "#![allow(non_camel_case_types, non_snake_case, dead_code)]");
    let _ = writeln!(src);
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
        let _ = writeln!(src, "{pad}pub mod {} {{", ident(child));
        let _ = writeln!(src, "{pad}    use orbweaver_gen::rt::{{self, Cdr}};");
        let mut next = at.to_vec();
        next.push(child.clone());
        write_modules(src, by_module, &next, depth + 1);
        let _ = writeln!(src, "{pad}}}");
    }
}

fn emit_type(id: &str, tc: &TypeCode, root: &str) -> Result<String, String> {
    match tc {
        TypeCode::Struct { name, members, .. } | TypeCode::Except { name, members, .. } => {
            let mut s = String::new();
            let kind = if matches!(tc, TypeCode::Except { .. }) { "exception" } else { "struct" };
            let _ = writeln!(s, "/// IDL {kind} `{id}`.");
            let _ = writeln!(s, "#[derive(Debug, Clone, PartialEq)]");
            let _ = writeln!(s, "pub struct {} {{", ident(name));
            for m in members {
                let _ = writeln!(s, "    pub {}: {},", ident(&m.name), rust_type(&m.tc, root)?);
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
            emit_union(id, name, discriminator, cases, *default_index, root)
        }
        TypeCode::Alias { name, aliased, .. } => Ok(format!(
            "/// IDL typedef `{id}`.\npub type {} = {};",
            ident(name),
            rust_type(aliased, root)?
        )),
        // A bare ObjRef entry is a forward-declared interface that was never
        // given a body in this file; the type alias keeps references to it
        // compilable.
        TypeCode::ObjRef { name, .. } => Ok(format!(
            "/// IDL interface `{id}` (reference type).\npub type {}Ref = rt::ObjRef;",
            ident(name)
        )),
        other => Err(match rust_type(other, root) {
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
    root: &str,
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
        let ty = rust_type(b.tc, root)?;
        if b.is_default || b.labels.len() > 1 {
            // The discriminator is not implied by the variant, so it is carried.
            let _ = writeln!(s, "    {} {{ d: {}, v: {} }},", ident(b.member), disc.ty, ty);
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

fn emit_interface(registry: &Registry, id: &str, root: &str) -> Result<String, String> {
    let iface = registry.interface(id).ok_or("not an interface")?;
    let name = path_of(id).last().cloned().unwrap_or_default();

    let mut s = String::new();
    let _ = writeln!(s, "/// Client stub for `{id}`.");
    let _ = writeln!(s, "///");
    let _ = writeln!(s, "/// Static twin of the dynamic path; §8 requires their bytes to be");
    let _ = writeln!(s, "/// identical, and the harness holds them to it.");
    let _ = writeln!(s, "pub struct {}Client {{", ident(&name));
    let _ = writeln!(s, "    /// The connection calls travel over.");
    let _ = writeln!(s, "    pub conn: rt::Connection,");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s, "impl {}Client {{", ident(&name));
    let _ = writeln!(s, "    /// A stub over an open connection.");
    let _ = writeln!(s, "    pub fn new(conn: rt::Connection) -> Self {{ Self {{ conn }} }}");

    // Operations, inherited ones included: a stub less capable than the
    // dynamic invoker would fail the oracle before it failed a user.
    let mut ops: BTreeMap<String, OperationSig> = BTreeMap::new();
    let mut ids = vec![id.to_owned()];
    ids.extend(registry.ancestors(id));
    for iid in &ids {
        if let Some(i) = registry.interface(iid) {
            for (op_name, sig) in &i.operations {
                ops.entry(op_name.clone()).or_insert_with(|| sig.clone());
            }
        }
    }
    for (op_name, sig) in &ops {
        emit_operation(&mut s, op_name, op_name, sig, root)?;
    }
    for (attr, a) in &iface.attributes {
        let getter = OperationSig {
            returns: a.tc.clone(),
            params: Vec::new(),
            raises: Vec::new(),
            oneway: false,
            annotations: BTreeMap::new(),
        };
        emit_operation(&mut s, &format!("_get_{attr}"), attr, &getter, root)?;
        if !a.readonly {
            let setter = OperationSig {
                returns: TypeCode::Void,
                params: vec![orbweaver_registry::ParamSig {
                    name: "value".into(),
                    direction: ParamDirection::In,
                    tc: a.tc.clone(),
                    annotations: BTreeMap::new(),
                }],
                raises: Vec::new(),
                oneway: false,
                annotations: BTreeMap::new(),
            };
            emit_operation(&mut s, &format!("_set_{attr}"), &format!("set_{attr}"), &setter, root)?;
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
    root: &str,
) -> Result<(), String> {
    let mut params = String::new();
    let mut ins: Vec<&str> = Vec::new();
    for p in &sig.params {
        match p.direction {
            ParamDirection::In | ParamDirection::InOut => {
                let _ = write!(params, ", {}: {}", ident(&p.name), rust_type(&p.tc, root)?);
                ins.push(&p.name);
            }
            ParamDirection::Out => {}
        }
    }
    // The return: the declared result, then out/inout values in declaration
    // order (§7.9.1's reply layout, surfaced as a tuple).
    let mut rets: Vec<String> = Vec::new();
    if !matches!(sig.returns, TypeCode::Void) {
        rets.push(rust_type(&sig.returns, root)?);
    }
    let mut out_reads: Vec<String> = Vec::new();
    for p in &sig.params {
        if matches!(p.direction, ParamDirection::Out | ParamDirection::InOut) {
            rets.push(rust_type(&p.tc, root)?);
            out_reads.push(p.name.clone());
        }
    }
    let ret_ty = match rets.len() {
        0 => "()".to_owned(),
        1 => rets[0].clone(),
        _ => format!("({})", rets.join(", ")),
    };

    let _ = writeln!(s, "    /// `{wire_name}`.");
    let _ = writeln!(
        s,
        "    pub fn {}(&mut self{params}) -> Result<{ret_ty}, rt::GiopError> {{",
        ident(rust_name)
    );
    // Marshal into a probe first, so a bad argument is a local error rather
    // than a half-written message — the same rule the dynamic invoker follows.
    // No arguments, nothing that can fail, no probe.
    if !ins.is_empty() {
        let _ = writeln!(s, "        let mut probe = rt::Encoder::new(self.conn.endian());");
        for p in &ins {
            let _ = writeln!(s, "        {}.put(&mut probe)?;", ident(p));
        }
        let _ = writeln!(s, "        probe.finish().map_err(rt::GiopError::Cdr)?;");
    }
    let closure_arg = if ins.is_empty() { "|_e|" } else { "|e|" };
    if sig.oneway {
        let _ = writeln!(s, "        self.conn.invoke_oneway(\"{wire_name}\", {closure_arg} {{");
        for p in &ins {
            let _ = writeln!(s, "            let _ = {}.put(e);", ident(p));
        }
        let _ = writeln!(s, "        }})");
        let _ = writeln!(s, "    }}");
        return Ok(());
    }
    let _ = writeln!(s, "        let reply = self.conn.invoke(\"{wire_name}\", {closure_arg} {{");
    for p in &ins {
        let _ = writeln!(s, "            let _ = {}.put(e);", ident(p));
    }
    let _ = writeln!(s, "        }})?;");
    let mut reads: Vec<String> = Vec::new();
    if !matches!(sig.returns, TypeCode::Void) || !out_reads.is_empty() {
        let _ = writeln!(s, "        let mut body = reply.body()?;");
    }
    if !matches!(sig.returns, TypeCode::Void) {
        let _ = writeln!(s, "        let ret_ = Cdr::get(&mut body)?;");
        reads.push("ret_".into());
    }
    for (i, name) in out_reads.iter().enumerate() {
        let _ = writeln!(s, "        let out_{i} = Cdr::get(&mut body)?; // out {name}");
        reads.push(format!("out_{i}"));
    }
    match reads.len() {
        0 => {
            let _ = writeln!(s, "        let _ = reply;");
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
        assert!(g.source.contains("s { d: i32, v: String }"), "{}", g.source);
        assert!(g.source.contains("b { d: i32, v: bool }"), "{}", g.source);
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
