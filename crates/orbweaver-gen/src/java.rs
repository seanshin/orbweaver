//! The Java **client** target: stubs, the types they carry, and the runtime
//! that speaks AnyJSON v1 to the bridge that owns the wire.
//!
//! # What a Java target is, and what makes this one
//!
//! D030 §3: *"A language is a target when its generated code is measured
//! against a peer that is not us, in both byte orders, and its refusals say the
//! same sentences ours do. Anything short of that is an emitter, and is called
//! one."* This module is the emitter half; the measurement is
//! `spikes/bindings/java.manifest` run by `spikes/binding_suite.sh`, and until
//! that suite accepts a cell nothing here may be called a target.
//!
//! # The three layers, and which one this is
//!
//! D032 §3: the **contract** is the corpus's and is not per-language; the
//! **value representation** is AnyJSON v1 and is not per-language; the
//! **dispatch binding** is the only per-language part, and it is this module
//! plus [`RUNTIME`].
//!
//! **There is no GIOP here and there must never be.** A generated Java client
//! starts `orbweaver-py-bridge` and speaks one JSON document per line to it;
//! the bridge holds the connection, encodes the CDR and negotiates the codeset.
//! A binding that spoke GIOP would be a second ORB wearing a binding's name
//! (D032 §6), and it would need an `org.omg.CORBA` — which JDK 11 removed (JEP
//! 320), so the only one on a machine like this is JacORB's jar, and JacORB is
//! an **LGPL fixture, never a dependency**. Generated Java that imported it
//! would have turned a fixture into a dependency through the back door.
//!
//! # Scope: clients, deliberately
//!
//! D030 §5 L2 scopes the second-language work to clients. A Java **servant**
//! would need the bridge's `--serve` direction to call back into a Java process
//! — the seam `pyservant.rs` already implements for one language — and
//! generalising that seam is its own batch. `emit_java` therefore writes no
//! servant base, and `COMPONENTS.md` records that as a gap rather than leaving
//! it to be discovered.
//!
//! # Why every local this emitter writes begins with `_`
//!
//! D030 §5 L2 predicted the Java hazard would be reserved words, and the
//! counter-example arrived before the target: JacORB 3.9's own stub template
//! writes `catch (java.io.IOException e)` into the same scope as an operation's
//! parameters, so an IDL parameter named `e` makes its generated Java fail to
//! compile — and `e` is not a Java keyword. **The hazard is every identifier the
//! template puts in scope.** An IDL identifier can never *begin* with an
//! underscore (the leading `_` in `_struct` is IDL's own keyword escape and is
//! stripped by the front end), so a template local spelled `_x` cannot collide
//! with any contract name in any position. This emitter binds nothing else:
//! every parameter, temporary and field it introduces is `_`-prefixed, and
//! `corpus/golden/28-target-keywords.idl`'s template-locals section executes the
//! claim by declaring contract members named `e`, `o`, `v` and the rest.
//!
//! *Java 클라이언트 대상. 계약과 값 표현은 언어별이 아니며, 여기 있는 것은 디스패치
//! 바인딩뿐이다. GIOP는 없다 — 와이어는 Rust에 한 번 존재한다. 이 방출기가 스코프에
//! 넣는 모든 이름은 `_`로 시작하므로 계약 식별자와 충돌할 수 없다.*

use std::collections::BTreeMap;
use std::fmt::Write as _;

use orbweaver_giop::typecode::TypeCode;
use orbweaver_registry::{ConstValue, Entry, OperationSig, ParamDirection, Registry};

use crate::{Cx, name_table, resolved_members};

/// The hand-written client runtime, shipped verbatim beside every generated
/// package as `_Rt.java`.
///
/// Public so that a consumer who assembles the package themselves — a build
/// script, a test — writes the same bytes the generator writes, rather than a
/// copy that drifts. The file carries no `package` declaration: [`emit_java`]
/// prepends one, because a class in the unnamed package cannot be imported by
/// a class in a named one and every generated file needs `_Rt`.
pub const RUNTIME: &str = include_str!("java_rt.java");

/// What one file's Java generation produced.
#[derive(Debug, Default)]
pub struct JavaPackage {
    /// Files, keyed by path relative to the source root: `contract/_Rt.java`,
    /// `contract/spike/Echo.java`.
    ///
    /// One public class per file, because that is what `javac` requires, and
    /// one directory per IDL module, because that is what a Java package is.
    pub files: BTreeMap<String, String>,
    /// Items emitted.
    pub emitted: usize,
    /// Items skipped, with the reason.
    pub skipped: Vec<(String, String)>,
}

/// Java's reserved words, plus the names the language reserves in a *position*
/// rather than everywhere.
///
/// Three groups, and the reason they are one list is that this emitter escapes
/// all of them the same way:
///
/// * the 50 keywords of JLS §3.9, plus the three literals `true`, `false` and
///   `null`, which are not keywords and are equally fatal as identifiers;
/// * the **contextual** keywords that are restricted *type* names — `var`,
///   `yield`, `record`, `permits`, `sealed`. A class named `record` is a
///   compile-time error in Java 16 and later even though `record` is a legal
///   variable name, and this emitter writes IDL type names into class names;
/// * `_`, which Java 9 removed as an identifier outright.
///
/// Only words that are legal IDL identifiers can ever reach the emitter, but
/// the list is not filtered by that: `spikes/bindings/keywords-not-executed.tsv`
/// is where a word that cannot reach a contract is named, with `binding-words`
/// computing the reachability class by asking `orbweaver_idl::lex::is_keyword`
/// rather than trusting a typed reason.
const JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "void",
    "volatile",
    "while",
    "true",
    "false",
    "null",
    "var",
    "yield",
    "record",
    "permits",
    "sealed",
    "_",
];

/// Every word this emitter escapes.
///
/// Published for [`crate::targets`], which is what gives D032 §4 clause 5 an
/// instrument: `binding-words --language java` asks this list and this
/// function's twin [`java_name`], never a retyped rule.
pub fn reserved_words() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = JAVA_KEYWORDS.to_vec();
    v.sort_unstable();
    v.dedup();
    v
}

/// The Java spelling of an IDL identifier.
///
/// A **leading** underscore, which is the OMG IDL-to-Java mapping's rule
/// (formal/08-01-11 §1.3.2, *"IDL names that would be Java keywords are
/// prefixed with an underscore"*) and what `org.jacorb.idl.parser` writes:
/// `class` becomes `_class`, the operation `final` becomes `_final`. The same
/// rule as the Python mapping's, arrived at independently by two specifications
/// and confirmed against one running compiler.
///
/// Public because the escaping is part of the mapping, not an implementation
/// detail: a caller looking up a generated method, and the oracle driving one,
/// both need to know the operation `final` is reached as `_final` while the
/// name that travels is still `final`.
pub fn java_name(idl: &str) -> String {
    java_ident(idl)
}

pub(crate) fn java_ident(name: &str) -> String {
    if JAVA_KEYWORDS.contains(&name) { format!("_{name}") } else { name.to_owned() }
}

/// A Java string literal, always double-quoted, ASCII-safe by escaping.
///
/// Non-ASCII is escaped rather than passed through: `javac` reads a source file
/// in the platform's default charset unless told otherwise, and a generated
/// file that only compiles under `-encoding UTF-8` is a file that fails on
/// somebody else's machine for a reason the diagnostic does not name.
fn java_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                for unit in c.encode_utf16(&mut [0u16; 2]).iter() {
                    let _ = write!(out, "\\u{unit:04x}");
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A Javadoc block at `indent`, or nothing when there is nothing to say.
///
/// `*/` inside the text is broken rather than dropped: an `ai_desc` is contract
/// text written by somebody else, and a generator that can be made to emit
/// unparseable Java by a comment terminator in a description has an injection
/// bug.
fn javadoc(out: &mut String, indent: &str, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    let text = text.replace("*/", "*&#47;");
    let _ = writeln!(out, "{indent}/**");
    for line in text.lines() {
        if line.trim().is_empty() {
            let _ = writeln!(out, "{indent} *");
        } else {
            let _ = writeln!(out, "{indent} * {}", line.trim_end());
        }
    }
    let _ = writeln!(out, "{indent} */");
}

fn item_doc(annotations: Option<&BTreeMap<String, String>>, what: &str) -> String {
    match annotations.and_then(|a| a.get("ai_desc")) {
        Some(desc) => format!("{desc}\n\n{what}"),
        None => what.to_owned(),
    }
}

/// The AnyJSON descriptor for a type as a Java expression, or the reason there
/// is none.
///
/// Public because a descriptor is this target's type language, exactly as it is
/// the Python target's: a consumer assembling a call by hand and the oracle
/// that drives generated code from Rust both need the spelling the emitter
/// writes, and a second one would be a second mapping.
///
/// Descriptors name other types by **repository id**, never by Java class. IDL
/// scopes are mutually recursive, and a class literal would make one generated
/// file's static initialiser depend on another's having run.
pub fn descriptor(tc: &TypeCode) -> Result<String, String> {
    Ok(match tc {
        TypeCode::Boolean => "_Rt.BOOLEAN".into(),
        TypeCode::Octet => "_Rt.OCTET".into(),
        TypeCode::Char => "_Rt.CHAR".into(),
        TypeCode::WChar => "_Rt.WCHAR".into(),
        TypeCode::Short => "_Rt.SHORT".into(),
        TypeCode::UShort => "_Rt.USHORT".into(),
        TypeCode::Long => "_Rt.LONG".into(),
        TypeCode::ULong => "_Rt.ULONG".into(),
        TypeCode::LongLong => "_Rt.LONGLONG".into(),
        TypeCode::ULongLong => "_Rt.ULONGLONG".into(),
        TypeCode::Float => "_Rt.FLOAT".into(),
        TypeCode::Double => "_Rt.DOUBLE".into(),
        TypeCode::LongDouble => "_Rt.LONGDOUBLE".into(),
        TypeCode::Any => "_Rt.ANY".into(),
        // `tk_TypeCode`, kind 12: a TypeCode as a value in its own right, which
        // is what `::CORBA::TypeCode describe()` returns and what every
        // Interface Repository description is made of. It had no arm here until
        // the corpus sweep found `IDL:gp34/Described:1.0` refused by Java and
        // carried by the other two targets — one contract of 37, and a
        // divergence rather than a decision.
        TypeCode::TypeCode => "_Rt.TYPECODE".into(),
        TypeCode::Void | TypeCode::Null => "_Rt.VOID".into(),
        TypeCode::String(bound) => format!("new _Rt.Str(false, {bound}L)"),
        TypeCode::WString(bound) => format!("new _Rt.Str(true, {bound}L)"),
        TypeCode::Sequence { element, bound } => {
            format!("new _Rt.Seq({}, {bound}L)", descriptor(element)?)
        }
        TypeCode::Array { element, length } => {
            format!("new _Rt.Arr({}, {length})", descriptor(element)?)
        }
        TypeCode::ObjRef { id, .. } => format!("new _Rt.ObjRef({})", java_str(id)),
        TypeCode::Struct { id, .. }
        | TypeCode::Union { id, .. }
        | TypeCode::Enum { id, .. }
        | TypeCode::Except { id, .. }
        | TypeCode::Alias { id, .. }
        | TypeCode::Recursive(id) => format!("new _Rt.Ref({})", java_str(id)),
        // The five families that cannot cross, refused with the sentence whose
        // home is `orbweaver-dynamic` — asked, never retyped. These are the
        // stopping points `crossable` consults rather than keeping a second
        // list of, which is the defect that arm's Python twin was written to
        // close.
        TypeCode::Fixed { digits, scale } => return Err(crate::deferred_fixed(*digits, *scale)),
        TypeCode::Value { name, id, .. } => return Err(crate::deferred_value(name, id)),
        TypeCode::AbstractInterface { name, id, .. } => {
            return Err(crate::deferred_abstract(name, id));
        }
        TypeCode::Native { name, id, .. } => return Err(crate::unmarshallable_native(name, id)),
        TypeCode::Principal => return Err(crate::withdrawn_principal()),
        // No catch-all, deliberately, and `rustc` enforces it: this match is
        // exhaustive over `TypeCode`, so a thirty-fourth construct is a build
        // error here rather than a type Java silently refuses while the other
        // two targets carry it. That is not hypothetical — `TypeCode::TypeCode`
        // reached a catch-all until the corpus sweep found one contract of 37
        // refused by Java alone.
    })
}

/// Whether every type this one reaches has an AnyJSON form.
///
/// The stopping points are [`descriptor`]'s, asked at every node rather than
/// relisted — the property the Python twin's docstring records at length, and
/// the reason a `Principal` nested two structs deep is skipped along with its
/// container instead of being named by a descriptor pointing at a class the
/// package never declared.
fn crossable(tc: &TypeCode, visiting: &mut Vec<String>) -> Result<(), String> {
    descriptor(tc)?;
    match tc {
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

/// The Java type a value of this IDL type is held as.
///
/// The integer widths are the OMG Java mapping's where Java has a type for
/// them, and one step wider where it does not: `unsigned short` is an `int` and
/// `unsigned long` a `long`, because Java has no unsigned arithmetic and a
/// caller who read 65535 as -1 would have been given a silently wrong value
/// rather than a wrong type. `unsigned long long` is the one that cannot widen
/// — it is a `long` read as unsigned, rendered with `Long.toUnsignedString`,
/// which is exact for every value in the range.
fn java_type(tc: &TypeCode, cx: &Jx<'_>) -> Result<String, String> {
    Ok(match tc {
        TypeCode::Boolean => "boolean".into(),
        TypeCode::Octet => "byte".into(),
        TypeCode::Char | TypeCode::WChar => "char".into(),
        TypeCode::Short => "short".into(),
        TypeCode::UShort | TypeCode::Long => "int".into(),
        TypeCode::ULong | TypeCode::LongLong | TypeCode::ULongLong => "long".into(),
        TypeCode::Float => "float".into(),
        TypeCode::Double => "double".into(),
        TypeCode::LongDouble => "_Rt.LongDouble".into(),
        TypeCode::String(_) | TypeCode::WString(_) => "String".into(),
        TypeCode::Any => "_Rt.Any".into(),
        TypeCode::TypeCode => "_Rt.TypeCodeValue".into(),
        TypeCode::ObjRef { .. } => "_Rt.ObjectRef".into(),
        TypeCode::Void | TypeCode::Null => "void".into(),
        TypeCode::Sequence { element, .. } => {
            if matches!(element.resolve_alias(), TypeCode::Octet) {
                "byte[]".into()
            } else {
                format!("java.util.List<{}>", boxed(element, cx)?)
            }
        }
        TypeCode::Array { element, .. } => format!("java.util.List<{}>", boxed(element, cx)?),
        // A typedef is **transparent to the value**, exactly as it is to CDR:
        // `typedef sequence<octet> Payload` is a `byte[]` and not a `Payload`,
        // and `typedef string Name` is a `String`. The generated class of that
        // name holds the descriptor and registers it; it is not a type anything
        // is an instance of, and emitting it as one produced
        // `class java.lang.String cannot be cast to class contract.gc03.ShortName`
        // in 20 of the corpus's contracts at once — one cause, found by
        // sweeping the whole corpus rather than one file.
        TypeCode::Alias { aliased, .. } => java_type(aliased, cx)?,
        TypeCode::Struct { id, .. }
        | TypeCode::Union { id, .. }
        | TypeCode::Enum { id, .. }
        | TypeCode::Except { id, .. }
        | TypeCode::Recursive(id) => cx.java_path(id),
        other => return Err(descriptor(other).err().unwrap_or_else(|| format!("{other:?}"))),
    })
}

/// The same type as a *reference* type, for a generic parameter.
fn boxed(tc: &TypeCode, cx: &Jx<'_>) -> Result<String, String> {
    Ok(match java_type(tc, cx)?.as_str() {
        "boolean" => "Boolean".into(),
        "byte" => "Byte".into(),
        "char" => "Character".into(),
        "short" => "Short".into(),
        "int" => "Integer".into(),
        "long" => "Long".into(),
        "float" => "Float".into(),
        "double" => "Double".into(),
        other => other.to_owned(),
    })
}

/// `_expr` as an `Object`: a primitive is boxed, everything else is itself.
fn box_expr(tc: &TypeCode, expr: &str, cx: &Jx<'_>) -> Result<String, String> {
    Ok(match java_type(tc, cx)?.as_str() {
        "boolean" => format!("Boolean.valueOf({expr})"),
        "byte" => format!("Byte.valueOf({expr})"),
        "char" => format!("Character.valueOf({expr})"),
        "short" => format!("Short.valueOf({expr})"),
        "int" => format!("Integer.valueOf({expr})"),
        "long" => format!("Long.valueOf({expr})"),
        "float" => format!("Float.valueOf({expr})"),
        "double" => format!("Double.valueOf({expr})"),
        _ => expr.to_owned(),
    })
}

/// An `Object` read back as this type: unboxed where Java needs it.
fn unbox_expr(tc: &TypeCode, expr: &str, cx: &Jx<'_>) -> Result<String, String> {
    let ty = java_type(tc, cx)?;
    Ok(match ty.as_str() {
        "boolean" => format!("((Boolean) {expr}).booleanValue()"),
        "byte" => format!("((Byte) {expr}).byteValue()"),
        "char" => format!("((Character) {expr}).charValue()"),
        "short" => format!("((Short) {expr}).shortValue()"),
        "int" => format!("((Integer) {expr}).intValue()"),
        "long" => format!("((Long) {expr}).longValue()"),
        "float" => format!("((Float) {expr}).floatValue()"),
        "double" => format!("((Double) {expr}).doubleValue()"),
        _ => format!("({ty}) {expr}"),
    })
}

/// The emitter's context: what [`Cx`] carries, plus which IDL scopes are
/// interfaces.
///
/// The second half is needed because **an IDL interface is a scope and a Java
/// class is not**. `interface Root { struct Ticket { … }; }` wants `Root` to be
/// a class *and* a package at once, and `javac` says `class Root clashes with
/// package of same name` — measured over the golden corpus, one contract of 37.
/// The OMG IDL-to-Java mapping answers this with the `Package` suffix
/// (formal/08-01-11 §1.3.2 reserves `Package`, `Helper`, `Holder`, `Operations`,
/// `POA` and `POATie` for exactly this class of collision), and the reason that
/// list exists is the reason this field does.
struct Jx<'a> {
    inner: Cx<'a>,
    /// Qualified IDL names (`gc_inh::Root`) of every interface in the registry.
    interfaces: std::collections::BTreeSet<String>,
}

impl<'a> Jx<'a> {
    fn new(registry: &Registry, root: &'a str) -> Self {
        let names = name_table(registry);
        let interfaces = registry
            .ids()
            .filter(|id| matches!(registry.get(id), Some(Entry::Interface(_))))
            .filter_map(|id| names.get(id).cloned())
            .collect();
        Jx { inner: Cx { root, names }, interfaces }
    }

    fn root(&self) -> &str {
        self.inner.root
    }

    fn path_of(&self, id: &str) -> Vec<String> {
        self.inner.path_of(id)
    }

    /// The Java package segments for the scopes enclosing an item.
    ///
    /// A segment that names an **interface** becomes `<Name>Package`, because
    /// the class of that name is already taken by the interface's own stub.
    fn package_segments(&self, path: &[String]) -> Vec<String> {
        let module = &path[..path.len() - 1];
        let mut out = Vec::with_capacity(module.len());
        for (i, seg) in module.iter().enumerate() {
            let qualified = module[..=i].join("::");
            if self.interfaces.contains(&qualified) {
                out.push(format!("{}Package", java_ident(seg)));
            } else {
                out.push(java_ident(seg));
            }
        }
        out
    }

    /// The package an item lives in, fully qualified.
    fn package_of(&self, path: &[String]) -> String {
        let segs = self.package_segments(path);
        if segs.is_empty() {
            self.root().to_owned()
        } else {
            format!("{}.{}", self.root(), segs.join("."))
        }
    }

    /// The fully qualified Java name of a generated type.
    fn java_path(&self, id: &str) -> String {
        let path = self.path_of(id);
        if path.is_empty() {
            return self.root().to_owned();
        }
        let class = java_ident(path.last().expect("non-empty"));
        format!("{}.{class}", self.package_of(&path))
    }
}

fn file_of(package: &str, class: &str) -> String {
    format!("{}/{class}.java", package.replace('.', "/"))
}

/// The header every generated file carries: the package, and the one import.
fn header(package: &str, root: &str) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "// Generated by orbweaver-gen. Do not edit: regeneration overwrites");
    let _ = writeln!(s, "// this file. Names, member order and descriptors only — every");
    let _ = writeln!(s, "// conversion is a call into `_Rt`, which speaks AnyJSON v1");
    let _ = writeln!(s, "// (docs/PLAN.md §4.5) to the bridge that owns the wire.");
    let _ = writeln!(s, "package {package};");
    let _ = writeln!(s);
    let _ = writeln!(s, "import {root}._Rt;");
    let _ = writeln!(s);
    s
}

/// Generates one loaded registry as a Java source tree.
///
/// `package` is the root package the caller compiles under; it is the one name
/// the generated tree hangs from, and every cross-reference inside it is fully
/// qualified so that no generated file depends on another's import list.
pub fn emit_java(registry: &Registry, package: &str) -> JavaPackage {
    let cx = &Jx::new(registry, package);
    let mut out = JavaPackage::default();
    // Every type that registered itself, so `_Types` can touch each one: Java
    // runs a class's static initialiser on first use, and a descriptor names a
    // type by id rather than by class, so nothing would ever *use* the class
    // whose registration the lookup needs. One explicit list, generated.
    let mut registrations: Vec<String> = Vec::new();
    // Constants, gathered per module: Java has no module-level binding, so a
    // module's constants share one holder class.
    let mut consts: BTreeMap<Vec<String>, Vec<(String, String)>> = BTreeMap::new();

    for id in registry.ids() {
        let path = cx.path_of(id);
        if path.is_empty() {
            continue;
        }
        let name = path.last().cloned().unwrap_or_default();
        let pkg = cx.package_of(&path);
        let emitted = match registry.get(id) {
            Some(Entry::Type(tc)) => {
                crossable(tc, &mut Vec::new()).and_then(|()| emit_type(registry, id, &name, tc, cx))
            }
            Some(Entry::Interface(_)) => interface_crossable(registry, id)
                .and_then(|()| emit_interface(registry, id, &name, cx)),
            Some(Entry::Const { tc, value }) => {
                match emit_const(registry, id, &name, tc, value.as_ref(), &path, cx) {
                    Ok(text) => {
                        consts
                            .entry(path[..path.len() - 1].to_vec())
                            .or_default()
                            .push((id.clone(), text));
                        out.emitted += 1;
                        continue;
                    }
                    Err(why) => Err(why),
                }
            }
            None => continue,
        };
        match emitted {
            Ok((class, body, registers)) => {
                out.emitted += 1;
                let mut text = header(&pkg, package);
                text.push_str(&body);
                out.files.insert(file_of(&pkg, &class), text);
                if registers {
                    registrations.push(format!("{pkg}.{class}._register();"));
                    // The servant base beside the stub, for the interfaces that
                    // got one. `registers` is true exactly for an interface that
                    // emitted a body, which is the same set that can be served —
                    // asking that flag rather than re-deciding is what keeps the
                    // two from drifting into different sets.
                    if let Ok((sclass, sbody)) = emit_servant(registry, id, &name, cx) {
                        let mut stext = header(&pkg, package);
                        stext.push_str(&sbody);
                        out.files.insert(file_of(&pkg, &sclass), stext);
                        out.emitted += 1;
                    }
                }
            }
            Err(why) => {
                // A skipped **interface** still owes its name to the runtime.
                // Every reference to one is a descriptor with no body whose
                // TypeCode names it, and an unnamed TypeCode is a byte the Rust
                // target does not write — the defect the Python twin records,
                // in a target that shares the registry it reads.
                if let Some(Entry::Interface(entry)) = registry.get(id)
                    && !entry.abstract_interface
                {
                    let class = java_ident(&name);
                    let mut text = header(&pkg, package);
                    let _ = writeln!(
                        text,
                        "/** IDL `{id}`, skipped here: {why} */\npublic final class {class} {{\n\
                         \x20   private {class}() {{}}\n\n\
                         \x20   /** The name is registered anyway: a reference to it is still an\n\
                         \x20    * objref whose TypeCode names it, and an unnamed TypeCode is a\n\
                         \x20    * byte the Rust target does not write. */\n\
                         \x20   public static void _register() {{\n\
                         \x20       _Rt._registerName({}, {});\n\
                         \x20   }}\n}}",
                        java_str(id),
                        java_str(&name)
                    );
                    out.files.insert(file_of(&pkg, &class), text);
                    registrations.push(format!("{pkg}.{class}._register();"));
                }
                out.skipped.push((id.clone(), why));
            }
        }
    }

    for (module, items) in &consts {
        let pkg = cx.package_of(&[module.clone(), vec!["_Consts".to_owned()]].concat());
        let mut text = header(&pkg, package);
        let scope = if module.is_empty() {
            format!("the global scope of `{package}`")
        } else {
            format!("IDL module `{}`", module.join("::"))
        };
        javadoc(
            &mut text,
            "",
            &format!(
                "The IDL constants declared in {scope}.\n\n\
                 Java has no binding outside a class, so a module's constants share one\n\
                 holder. The name `_Consts` cannot collide with a contract's: an IDL\n\
                 identifier never begins with an underscore, because the leading `_` in\n\
                 `_struct` is IDL's own keyword escape and the front end strips it."
            ),
        );
        let _ = writeln!(text, "public final class _Consts {{");
        let _ = writeln!(text, "    private _Consts() {{}}");
        for (_, item) in items {
            let _ = writeln!(text);
            text.push_str(item);
        }
        let _ = writeln!(text, "}}");
        out.files.insert(file_of(&pkg, "_Consts"), text);
    }

    // The runtime, under the root package. It ships with no `package` line so
    // that this is the one place the root name is written into it.
    let mut runtime = format!("package {package};\n\n");
    runtime.push_str(RUNTIME);
    out.files.insert(file_of(package, "_Rt"), runtime);
    out.files.insert(file_of(package, "_Types"), types_file(package, &registrations, &out.skipped));
    out
}

/// `_Types._ensure()`: every generated type, registered once.
fn types_file(package: &str, registrations: &[String], skipped: &[(String, String)]) -> String {
    let mut s = header(package, package);
    javadoc(
        &mut s,
        "",
        "Registers every type this package declares.\n\n\
         Java runs a class's static initialiser the first time something uses the\n\
         class, and a descriptor names a type by repository id rather than by class —\n\
         so nothing ever *uses* the class whose registration a lookup needs. This\n\
         holder touches each one explicitly. Generated stubs call it from their\n\
         constructors, so a caller who only builds a stub needs to know nothing about\n\
         it; a caller who reads a document without building a stub calls it directly.",
    );
    let _ = writeln!(s, "public final class _Types {{");
    let _ = writeln!(s, "    private _Types() {{}}");
    let _ = writeln!(s);
    let _ = writeln!(s, "    private static boolean _done = false;");
    let _ = writeln!(s);
    let _ = writeln!(s, "    /** Every type this package declares, registered once. */");
    let _ = writeln!(s, "    public static synchronized void _ensure() {{");
    let _ = writeln!(s, "        if (_done) {{");
    let _ = writeln!(s, "            return;");
    let _ = writeln!(s, "        }}");
    let _ = writeln!(s, "        _done = true;");
    for r in registrations {
        let _ = writeln!(s, "        {r}");
    }
    let _ = writeln!(s, "    }}");
    if !skipped.is_empty() {
        let _ = writeln!(s);
        for (id, why) in skipped {
            let _ = writeln!(s, "    // skipped {id}: {why}");
        }
    }
    let _ = writeln!(s, "}}");
    s
}

/// One type: the class body, and whether it has a `_register` to call.
fn emit_type(
    registry: &Registry,
    id: &str,
    name: &str,
    tc: &TypeCode,
    cx: &Jx<'_>,
) -> Result<(String, String, bool), String> {
    match tc {
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
            let is_exception = matches!(tc, TypeCode::Except { .. });
            let kind = if is_exception { "except" } else { "struct" };
            let class = java_ident(name);
            let mut s = String::new();
            javadoc(
                &mut s,
                "",
                &item_doc(
                    registry.annotations(id),
                    &format!(
                        "IDL {} `{id}`.\n\n\
                         Members are public fields in declaration order, which **is** the wire\n\
                         order: the §5.3 differ proved on the wire that swapping two members of\n\
                         the same size is a silent breaking change a peer decodes without\n\
                         complaint.",
                        if is_exception { "exception" } else { "struct" }
                    ),
                ),
            );
            if is_exception {
                let _ = writeln!(s, "public final class {class} extends _Rt.UserException {{");
                let _ = writeln!(s, "    private static final long serialVersionUID = 1L;");
            } else {
                let _ = writeln!(s, "public final class {class} {{");
            }
            let _ = writeln!(s, "    /** The repository id, as the wire names it. */");
            let _ = writeln!(s, "    public static final String _ID = {};", java_str(id));
            let _ = writeln!(s, "    /** The simple IDL name, as a TypeCode carries it. */");
            let _ = writeln!(s, "    public static final String _NAME = {};", java_str(name));
            for (i, m) in members.iter().enumerate() {
                let _ = writeln!(s);
                let _ = writeln!(s, "    /** Marshalled {}. */", crate::nth(i));
                let _ =
                    writeln!(s, "    public {} {};", java_type(&m.tc, cx)?, java_ident(&m.name));
            }
            let _ = writeln!(s);
            let params: Vec<String> = members
                .iter()
                .map(|m| Ok(format!("{} {}", java_type(&m.tc, cx)?, ctor_local(&m.name))))
                .collect::<Result<_, String>>()?;
            let _ = writeln!(s, "    /** Every member, in declaration order. */");
            let _ = writeln!(s, "    public {class}({}) {{", params.join(", "));
            if is_exception {
                let _ = writeln!(s, "        super(_ID);");
            }
            for m in members {
                let _ =
                    writeln!(s, "        this.{} = {};", java_ident(&m.name), ctor_local(&m.name));
            }
            let _ = writeln!(s, "    }}");
            if is_exception {
                let _ = writeln!(s);
                let _ = writeln!(s, "    @Override");
                let _ = writeln!(s, "    public String _id() {{");
                let _ = writeln!(s, "        return _ID;");
                let _ = writeln!(s, "    }}");
            }

            // `_make` and `_parts`: the two halves the runtime drives a value
            // through, generated rather than reflected. Reflection would read a
            // field by the *IDL* name and find nothing where the name was
            // escaped, which is the class of defect this project has measured
            // three times in three languages.
            let _ = writeln!(s);
            let _ = writeln!(s, "    /** Builds one from its members, in declaration order. */");
            let _ = writeln!(s, "    @SuppressWarnings(\"unchecked\")");
            let _ = writeln!(s, "    public static Object _make(Object[] _parts) {{");
            let args: Vec<String> = members
                .iter()
                .enumerate()
                .map(|(i, m)| unbox_expr(&m.tc, &format!("_parts[{i}]"), cx))
                .collect::<Result<_, String>>()?;
            let _ = writeln!(s, "        return new {class}({});", args.join(", "));
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s);
            let _ = writeln!(s, "    /** Takes one apart, in declaration order. */");
            let _ = writeln!(s, "    public static Object[] _parts(Object _value) {{");
            let _ = writeln!(s, "        {class} _v = ({class}) _value;");
            let boxes: Vec<String> = members
                .iter()
                .map(|m| box_expr(&m.tc, &format!("_v.{}", java_ident(&m.name)), cx))
                .collect::<Result<_, String>>()?;
            let _ = writeln!(s, "        return new Object[] {{{}}};", boxes.join(", "));
            let _ = writeln!(s, "    }}");

            let _ = writeln!(s);
            let _ = writeln!(s, "    /** Registers this type with the runtime. */");
            let _ = writeln!(s, "    public static void _register() {{");
            let _ = writeln!(
                s,
                "        _Rt._registerRecord({}, _ID, _NAME, {class}.class, new _Rt.Member[] {{",
                java_str(kind)
            );
            for m in members {
                let _ = writeln!(
                    s,
                    "            _Rt._member({}, {}),",
                    java_str(&m.name),
                    descriptor(&m.tc)?
                );
            }
            let _ = writeln!(s, "        }}, {class}::_make, {class}::_parts);");
            let _ = writeln!(s, "    }}");

            emit_value_semantics(
                &mut s,
                &class,
                &members.iter().map(|m| m.name.clone()).collect::<Vec<_>>(),
            );
            let _ = writeln!(s, "}}");
            Ok((class, s, true))
        }

        TypeCode::Enum { members, .. } => {
            let class = java_ident(name);
            let mut s = String::new();
            javadoc(
                &mut s,
                "",
                &item_doc(
                    registry.annotations(id),
                    &format!(
                        "IDL enum `{id}`.\n\n\
                         The enumerators cross **by name**: the ordinal is a wire detail, and\n\
                         AnyJSON v1 spells an enumerator as a string. Each constant therefore\n\
                         carries the IDL name it travels under, which differs from the Java one\n\
                         exactly when the enumerator is a Java keyword."
                    ),
                ),
            );
            let _ = writeln!(s, "public enum {class} {{");
            for (i, m) in members.iter().enumerate() {
                let sep = if i + 1 == members.len() { ";" } else { "," };
                let _ = writeln!(s, "    {}({}){sep}", java_ident(m), java_str(m));
            }
            if members.is_empty() {
                let _ = writeln!(s, "    ;");
            }
            let _ = writeln!(s);
            let _ = writeln!(s, "    public static final String _ID = {};", java_str(id));
            let _ = writeln!(s, "    public static final String _NAME = {};", java_str(name));
            let _ = writeln!(s);
            let _ = writeln!(s, "    private final String _idl;");
            let _ = writeln!(s);
            let _ = writeln!(s, "    {class}(String _idl) {{");
            let _ = writeln!(s, "        this._idl = _idl;");
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s);
            let _ = writeln!(s, "    /** The name this enumerator travels under. */");
            let _ = writeln!(s, "    public String _idlName() {{");
            let _ = writeln!(s, "        return _idl;");
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s);
            let _ = writeln!(s, "    /** The enumerator a document names, or a refusal. */");
            let _ = writeln!(s, "    public static {class} _byIdl(String _name) {{");
            let _ = writeln!(s, "        for ({class} _c : values()) {{");
            let _ = writeln!(s, "            if (_c._idl.equals(_name)) {{");
            let _ = writeln!(s, "                return _c;");
            let _ = writeln!(s, "            }}");
            let _ = writeln!(s, "        }}");
            let _ = writeln!(
                s,
                "        throw new _Rt.MarshalError(\"\", _name + \" is not an enumerator of \" \
                 + _NAME);"
            );
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s);
            let _ = writeln!(s, "    public static Object _make(Object[] _parts) {{");
            let _ = writeln!(s, "        return _byIdl((String) _parts[0]);");
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s);
            let _ = writeln!(s, "    public static Object[] _parts(Object _value) {{");
            let _ = writeln!(s, "        return new Object[] {{(({class}) _value)._idlName()}};");
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s);
            let _ = writeln!(s, "    /** Registers this type with the runtime. */");
            let _ = writeln!(s, "    public static void _register() {{");
            let _ =
                writeln!(s, "        _Rt._registerEnum(_ID, _NAME, {class}.class, new String[] {{");
            for m in members {
                let _ = writeln!(s, "            {},", java_str(m));
            }
            let _ = writeln!(s, "        }}, {class}::_make, {class}::_parts);");
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s, "}}");
            Ok((class, s, true))
        }

        TypeCode::Union { discriminator, cases, default_index, .. } => {
            let class = java_ident(name);
            // One branch per member, its labels gathered: the registry expands
            // `case 2: case 3:` into two cases sharing a member, and a class
            // holds the member once. The default's SLOT — how many of the
            // branch's labels precede it — is kept, because a rebuilt member
            // list with the default elsewhere is a different `default_index` on
            // the wire.
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
            let disc_ty = java_type(discriminator, cx)?;
            let mut s = String::new();
            javadoc(
                &mut s,
                "",
                &item_doc(
                    registry.annotations(id),
                    &format!(
                        "IDL union `{id}`.\n\n\
                         `_d` is the discriminator and `_v` the value, which is both the OMG\n\
                         mapping and §4.5's rule: the active branch is a fact about the value,\n\
                         never something to infer from which member happens to be set. The two\n\
                         fields are `_`-prefixed, so a branch named for either of them is\n\
                         impossible rather than escaped."
                    ),
                ),
            );
            let _ = writeln!(s, "public final class {class} {{");
            let _ = writeln!(s, "    public static final String _ID = {};", java_str(id));
            let _ = writeln!(s, "    public static final String _NAME = {};", java_str(name));
            let _ = writeln!(s);
            let _ = writeln!(s, "    /** The discriminator. */");
            let _ = writeln!(s, "    public {disc_ty} _d;");
            let _ = writeln!(s, "    /** The selected branch's value, or null for none. */");
            let _ = writeln!(s, "    public Object _v;");
            let _ = writeln!(s);
            let _ = writeln!(s, "    public {class}({disc_ty} _disc, Object _value) {{");
            let _ = writeln!(s, "        this._d = _disc;");
            let _ = writeln!(s, "        this._v = _value;");
            let _ = writeln!(s, "    }}");
            for (_, member, tc, _) in &branches {
                let jname = java_ident(member);
                let ty = java_type(tc, cx)?;
                let _ = writeln!(s);
                let _ = writeln!(
                    s,
                    "    /** The `{member}` branch's value; the branch must be selected. */"
                );
                let _ = writeln!(s, "    @SuppressWarnings(\"unchecked\")");
                let _ = writeln!(s, "    public {ty} {jname}() {{");
                let _ = writeln!(s, "        return {};", unbox_expr(tc, "_v", cx)?);
                let _ = writeln!(s, "    }}");
                let _ = writeln!(s);
                let _ = writeln!(s, "    /** One of these, with `{member}` selected. */");
                let _ = writeln!(
                    s,
                    "    public static {class} {jname}({disc_ty} _disc, {ty} _value) {{"
                );
                let _ = writeln!(
                    s,
                    "        return new {class}(_disc, {});",
                    box_expr(tc, "_value", cx)?
                );
                let _ = writeln!(s, "    }}");
            }
            let _ = writeln!(s);
            let _ = writeln!(s, "    public static Object _make(Object[] _parts) {{");
            let _ = writeln!(
                s,
                "        return new {class}({}, _parts[1]);",
                unbox_expr(discriminator, "_parts[0]", cx)?
            );
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s);
            let _ = writeln!(s, "    public static Object[] _parts(Object _value) {{");
            let _ = writeln!(s, "        {class} _u = ({class}) _value;");
            let _ = writeln!(
                s,
                "        return new Object[] {{{}, _u._v}};",
                box_expr(discriminator, "_u._d", cx)?
            );
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s);
            let _ = writeln!(s, "    /** Registers this type with the runtime. */");
            let _ = writeln!(s, "    public static void _register() {{");
            let _ = writeln!(
                s,
                "        _Rt._registerUnion(_ID, _NAME, {class}.class, {}, new _Rt.Branch[] {{",
                descriptor(discriminator)?
            );
            for (labels, member, tc, slot) in &branches {
                let _ = writeln!(
                    s,
                    "            _Rt._branch(new Object[] {{{}}}, {}, {}, {}),",
                    labels.join(", "),
                    java_str(member),
                    descriptor(tc)?,
                    slot.map_or(-1i64, |v| v as i64)
                );
            }
            let _ = writeln!(s, "        }}, {class}::_make, {class}::_parts);");
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s, "}}");
            Ok((class, s, true))
        }

        TypeCode::Alias { aliased, .. } => {
            let class = java_ident(name);
            let mut s = String::new();
            javadoc(
                &mut s,
                "",
                &item_doc(
                    registry.annotations(id),
                    &format!(
                        "IDL typedef `{id}`.\n\n\
                         A typedef binds its name to the **descriptor** of the type it aliases,\n\
                         not to a class: aliases are transparent to §4.5 exactly as they are to\n\
                         CDR, so `typedef sequence<octet> Payload` still crosses as base64. The\n\
                         name travels too — an `any` carrying this typedef names it, and a name\n\
                         the runtime does not have is a name it cannot write."
                    ),
                ),
            );
            let _ = writeln!(s, "public final class {class} {{");
            let _ = writeln!(s, "    private {class}() {{}}");
            let _ = writeln!(s);
            let _ = writeln!(s, "    public static final String _ID = {};", java_str(id));
            let _ = writeln!(s, "    public static final String _NAME = {};", java_str(name));
            let _ = writeln!(s, "    /** What this name is an alias for. */");
            let _ =
                writeln!(s, "    public static final _Rt.Desc _DESC = {};", descriptor(aliased)?);
            let _ = writeln!(s);
            let _ = writeln!(s, "    /** The Java type a value of this alias is held as. */");
            let _ = writeln!(s, "    // {}", java_type(aliased, cx)?);
            let _ = writeln!(s, "    public static void _register() {{");
            let _ = writeln!(s, "        _Rt._registerAlias(_ID, _NAME, _DESC);");
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s, "}}");
            Ok((class, s, true))
        }

        // Declared and never given a body: a forward-declared interface, or one
        // of the constructs §4.4 defers, which the registry records the same
        // way. A reference is all the v1 wire carries for it.
        TypeCode::ObjRef { .. } => {
            let class = java_ident(name);
            let mut s = String::new();
            javadoc(
                &mut s,
                "",
                &format!(
                    "IDL `{id}`, declared and not defined in this file.\n\n\
                     A reference is all the v1 wire carries for it (§4.4)."
                ),
            );
            let _ = writeln!(s, "public final class {class} {{");
            let _ = writeln!(s, "    private {class}() {{}}");
            let _ = writeln!(s);
            let _ = writeln!(s, "    public static final String _ID = {};", java_str(id));
            let _ = writeln!(s, "    public static final String _NAME = {};", java_str(name));
            let _ = writeln!(s, "    public static final _Rt.Desc _DESC = {};", descriptor(tc)?);
            let _ = writeln!(s);
            let _ = writeln!(s, "    public static void _register() {{");
            let _ = writeln!(s, "        _Rt._registerName(_ID, _NAME);");
            let _ = writeln!(s, "    }}");
            let _ = writeln!(s, "}}");
            Ok((class, s, true))
        }

        other => Err(match descriptor(other) {
            Err(why) => why,
            Ok(_) => format!("unexpected top-level type {other:?}"),
        }),
    }
}

/// The constructor parameter that carries a member's initial value.
///
/// `_in_` and not `_`: an escaped member name **is** `_` plus the IDL name, so
/// a member called `class` has the field `_class`, and a parameter spelled the
/// same way would make the constructor assign a field to itself — silently, and
/// only for members whose names are Java keywords. Two prefixes are needed
/// because one of them is already the escape. `_in_x` cannot collide with any
/// escaped name, because an escaped name is `_` followed by a Java **keyword**
/// and `in_x` is not one.
///
/// This is the D030 §5 L2 hazard in its second form: the first was a template
/// local colliding with a contract name, and this is a template local colliding
/// with the *escape* for one. Found by writing the emitter and reading what it
/// wrote for `struct Reserved { string class; }`.
fn ctor_local(member: &str) -> String {
    format!("_in_{member}")
}

/// `equals`, `hashCode` and `toString` for a generated record type.
fn emit_value_semantics(s: &mut String, class: &str, members: &[String]) {
    let _ = writeln!(s);
    let _ = writeln!(s, "    @Override");
    let _ = writeln!(s, "    public boolean equals(Object _other) {{");
    let _ = writeln!(s, "        if (!(_other instanceof {class})) {{");
    let _ = writeln!(s, "            return false;");
    let _ = writeln!(s, "        }}");
    let _ = writeln!(s, "        {class} _o = ({class}) _other;");
    if members.is_empty() {
        let _ = writeln!(s, "        return true;");
    } else {
        let terms: Vec<String> = members
            .iter()
            .map(|m| {
                let j = java_ident(m);
                // A primitive autoboxes on the way into `_eq(Object, Object)`,
                // and a `byte[]` is compared by content there rather than by
                // identity — the one comparison Java's `equals` gets wrong for
                // a `sequence<octet>`.
                format!("_Rt._eq(this.{j}, _o.{j})")
            })
            .collect();
        let _ = writeln!(s, "        return {};", terms.join("\n                && "));
    }
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s);
    let _ = writeln!(s, "    @Override");
    let _ = writeln!(s, "    public int hashCode() {{");
    let _ = writeln!(s, "        return java.util.Arrays.deepHashCode(_parts(this));");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s);
    let _ = writeln!(s, "    @Override");
    let _ = writeln!(s, "    public String toString() {{");
    let _ = writeln!(s, "        StringBuilder _b = new StringBuilder(_NAME);");
    let _ = writeln!(s, "        _b.append('(');");
    let _ = writeln!(s, "        Object[] _p = _parts(this);");
    let _ = writeln!(s, "        for (int _i = 0; _i < _p.length; _i++) {{");
    let _ = writeln!(s, "            if (_i > 0) {{");
    let _ = writeln!(s, "                _b.append(\", \");");
    let _ = writeln!(s, "            }}");
    let _ = writeln!(s, "            _b.append(_Rt._show(_p[_i]));");
    let _ = writeln!(s, "        }}");
    let _ = writeln!(s, "        return _b.append(')').toString();");
    let _ = writeln!(s, "    }}");
}

/// A union case label in its AnyJSON form, as a Java expression.
///
/// The label travels as the scalar §4.5 defines for the discriminator's type —
/// a JSON number, a boolean, an enumerator's *name*, or, for the two 64-bit
/// types, a **string**, because those cross as strings and a label that was a
/// number could never match the document it is compared against.
fn json_label(label: &[u8], disc: &TypeCode) -> Result<String, String> {
    let wide = |b: &[u8]| {
        let mut v: u64 = 0;
        for x in b {
            v = (v << 8) | u64::from(*x);
        }
        v
    };
    Ok(match disc {
        TypeCode::Boolean => {
            if label.last() == Some(&1) { "Boolean.TRUE" } else { "Boolean.FALSE" }.into()
        }
        TypeCode::Long => format!("_Rt.Num.of({}L)", wide(label) as i32),
        TypeCode::ULong => format!("_Rt.Num.of({}L)", wide(label) as u32),
        TypeCode::Short => format!("_Rt.Num.of({}L)", wide(label) as i16),
        TypeCode::UShort => format!("_Rt.Num.of({}L)", wide(label) as u16),
        TypeCode::Char | TypeCode::Octet => format!("_Rt.Num.of({}L)", wide(label) as u8),
        TypeCode::LongLong => java_str(&format!("{}", wide(label) as i64)),
        TypeCode::ULongLong => java_str(&format!("{}", wide(label))),
        TypeCode::Enum { members, name, .. } => {
            let ordinal = wide(label) as u32 as usize;
            match members.get(ordinal) {
                Some(m) => java_str(m),
                None => return Err(format!("case label {ordinal} is not an enumerator of {name}")),
            }
        }
        other => return Err(format!("unsupported union discriminator {other:?}")),
    })
}

/// One IDL constant, as a field of its module's `_Consts` holder.
fn emit_const(
    registry: &Registry,
    id: &str,
    name: &str,
    tc: &TypeCode,
    value: Option<&ConstValue>,
    path: &[String],
    cx: &Jx<'_>,
) -> Result<String, String> {
    let Some(value) = value else {
        return Err("the registry could not evaluate its expression, and stores no value \
                    rather than a guess — see orbweaver_registry::ConstValue"
            .to_owned());
    };
    let literal = const_literal(tc, value, cx)?;
    let ty = java_type(tc.resolve_alias(), cx)?;
    let mut s = String::new();
    javadoc(&mut s, "    ", &item_doc(registry.annotations(id), &format!("IDL constant `{id}`.")));
    let _ = writeln!(s, "    public static final {ty} {} = {literal};", java_ident(name));
    let _ = path;
    Ok(s)
}

fn const_literal(tc: &TypeCode, v: &ConstValue, cx: &Jx<'_>) -> Result<String, String> {
    let resolved = tc.resolve_alias();
    Ok(match (&resolved, v) {
        (TypeCode::Boolean, ConstValue::Bool(b)) => {
            if *b {
                "true".to_owned()
            } else {
                "false".to_owned()
            }
        }
        (TypeCode::Char | TypeCode::WChar, ConstValue::Int(i)) => {
            let c = u32::try_from(*i)
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| format!("{i} is not a code point"))?;
            format!("(char) 0x{:04x}", c as u32)
        }
        (TypeCode::Octet, ConstValue::Int(i)) => format!("(byte) {i}"),
        (TypeCode::Short, ConstValue::Int(i)) => format!("(short) {i}"),
        (TypeCode::UShort | TypeCode::Long, ConstValue::Int(i)) => format!("{i}"),
        (TypeCode::ULong | TypeCode::LongLong, ConstValue::Int(i)) => format!("{i}L"),
        (TypeCode::ULongLong, ConstValue::Int(i)) => {
            // Java's `long` is signed, and the top half of `unsigned long long`
            // has no literal that means the value: `Long.parseUnsignedLong`
            // does, exactly, and the field is `static final` so the cost is one
            // call at class initialisation.
            if *i > i128::from(i64::MAX) {
                format!("Long.parseUnsignedLong(\"{i}\")")
            } else {
                format!("{i}L")
            }
        }
        (TypeCode::Float, ConstValue::Float(f)) => format!("{:?}f", *f as f32),
        (TypeCode::Double, ConstValue::Float(f)) => format!("{f:?}d"),
        (TypeCode::String(_) | TypeCode::WString(_), ConstValue::Str(s)) => java_str(s),
        (TypeCode::Enum { id, .. }, ConstValue::Enum { member, .. }) => {
            format!("{}.{}", cx.java_path(id), java_ident(member))
        }
        (TypeCode::LongDouble, _) => {
            return Err("a `long double` constant has no Java literal: the value is 16 \
                        octets of an encoding no literal produces (§4.4)"
                .to_owned());
        }
        (TypeCode::Fixed { .. }, v) => {
            let text = v.as_decimal().unwrap_or_else(|| "the value".to_owned());
            return Err(format!(
                "a `fixed` constant has no Java literal here: {text} is a decimal, and a \
                 `double` literal would change it. `new java.math.BigDecimal({text:?})` would \
                 hold it exactly — the registry has the value; a `fixed` does not cross the \
                 v1 wire, so this emitter does not give the package a type it cannot send"
            ));
        }
        (tc, v) => return Err(format!("no Java literal for {v:?} declared as {tc:?}")),
    })
}

/// One interface, as a client stub.
fn emit_interface(
    registry: &Registry,
    id: &str,
    name: &str,
    cx: &Jx<'_>,
) -> Result<(String, String, bool), String> {
    if registry.interface(id).is_none() {
        return Err("not an interface".to_owned());
    }
    let class = java_ident(name);
    let mut s = String::new();
    javadoc(
        &mut s,
        "",
        &item_doc(
            registry.annotations(id),
            &format!(
                "Client stub for `{id}`.\n\n\
                 Takes an invoker — `_Rt.Bridge` over a real target, `_Rt.Loopback` in a\n\
                 test — and answers for every operation and attribute this interface has,\n\
                 inherited ones included. Inherited members are *flattened* rather than\n\
                 expressed as Java inheritance, which is the same resolved set the Rust and\n\
                 Python stubs carry: one interface cannot answer for two different sets\n\
                 depending on which target generated it.\n\n\
                 A servant base is emitted beside this, as `<Name>Servant` — see it for\n\
                 the serving direction. D030 §5 L2 scoped the second language to clients;\n\
                 the seam stopped being one language's on 2026-08-26 and Java's half of it\n\
                 landed 2026-09-01."
            ),
        ),
    );
    let _ = writeln!(s, "public final class {class} {{");
    let _ = writeln!(s, "    public static final String _ID = {};", java_str(id));
    let _ = writeln!(s, "    public static final String _NAME = {};", java_str(name));
    let _ = writeln!(s);
    let _ = writeln!(s, "    private final _Rt.Invoker _invoker;");
    let _ = writeln!(s);
    let _ = writeln!(s, "    /** A stub over `_invoker`. */");
    let _ = writeln!(s, "    public {class}(_Rt.Invoker _invoker) {{");
    let _ = writeln!(s, "        {}._Types._ensure();", cx.root());
    let _ = writeln!(s, "        this._invoker = _invoker;");
    let _ = writeln!(s, "    }}");

    for (op_name, sig) in crate::python::client_operations(registry, id) {
        emit_operation(&mut s, &op_name, &sig, cx)?;
    }

    let _ = writeln!(s);
    let _ = writeln!(s, "    /** Registers this interface's name with the runtime. */");
    let _ = writeln!(s, "    public static void _register() {{");
    let _ = writeln!(s, "        _Rt._registerName(_ID, _NAME);");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "}}");
    Ok((class, s, true))
}

/// The Java method name for an operation, and whether it is an accessor.
///
/// `_get_balance` is an operation on the wire (§7.9.1) and an attribute in the
/// contract, so the name that travels and the name a caller writes differ here
/// for a reason that is not escaping. An IDL identifier cannot begin with an
/// underscore, so no contract operation can collide with either prefix.
fn method_name(op: &str) -> String {
    if let Some(attr) = op.strip_prefix("_get_") {
        return java_ident(attr);
    }
    if let Some(attr) = op.strip_prefix("_set_") {
        return java_ident(attr);
    }
    java_ident(op)
}

/// The servant base for one interface: `<Name>Servant`.
///
/// The mirror of `python.rs`'s, and named the same way for the same reason:
/// `<Name>Servant` beside `<Name>`, never `POA_<Name>`, because both targets of
/// this project put a servant beside its stub and one spelling for both is one
/// fact. The collision that spelling can have — a contract declaring
/// `interface EchoServant` beside `interface Echo` — is the one the Python
/// target already records.
///
/// # What is generated and what is not
///
/// This writes names, order, descriptors and a `switch`. Every conversion and
/// the shape of every reply live in `_Rt.dispatchCall`, which is the same split
/// the client half makes and states: *the stub contributes no conversion logic
/// at all.* It is why the seam's protocol did not have to change for a third
/// language — `seam::protocol()` publishes it and
/// `tests/the_seam_is_one_protocol.rs` asserts the implementations against it.
///
/// Each method answers `NO_IMPLEMENT` until it is overridden, and deliberately
/// not `BAD_OPERATION`: the operation is in the contract and this servant has
/// not implemented it, which is a different thing from there being no such
/// operation. `_is_a` and `_non_existent` are absent and must stay absent — the
/// bridge answers them from the registry's resolved inheritance chain, so a
/// servant cannot make its object un-narrowable by getting them wrong.
fn emit_servant(
    registry: &Registry,
    id: &str,
    name: &str,
    cx: &Jx<'_>,
) -> Result<(String, String), String> {
    let class = format!("{}Servant", java_ident(name));
    let ops = crate::python::client_operations(registry, id);
    let mut s = String::new();
    javadoc(
        &mut s,
        "",
        &item_doc(
            registry.annotations(id),
            &format!(
                "Servant base for `{id}`.\n\n\
                 Subclass it, write the method bodies, and hand an instance to\n\
                 `_Rt.serveOnPipes(...)` — the far side of `orbweaver_gen::seam`, which\n\
                 mounts it as a plain `Dispatch` in a server the Rust process owns. No\n\
                 listener and no address: a language swapped behind one reference is a\n\
                 language swap, and a caller sent to another endpoint has been *moved*,\n\
                 which is a different row of D029 §6.1.\n\n\
                 Every operation and attribute accessor this interface has is declared\n\
                 below, inherited ones included and flattened — the same resolved set the\n\
                 client stub carries, because one function decides both."
            ),
        ),
    );
    let _ = writeln!(s, "public abstract class {class} implements _Rt.Servant {{");
    let _ = writeln!(s, "    public static final String _ID = {};", java_str(id));
    let _ = writeln!(s, "    public static final String _NAME = {};", java_str(name));
    let _ = writeln!(s);
    let _ = writeln!(s, "    private static final java.util.Map<String, _Rt.Op> _OPS =");
    let _ = writeln!(s, "            new java.util.LinkedHashMap<String, _Rt.Op>();");
    let _ = writeln!(s, "    static {{");
    for (op_name, sig) in &ops {
        let _ = writeln!(
            s,
            "        _OPS.put({}, new _Rt.Op({},",
            java_str(op_name),
            java_str(op_name)
        );
        let _ = writeln!(s, "                new _Rt.Param[] {{");
        for p in &sig.params {
            let is_in = matches!(p.direction, ParamDirection::In | ParamDirection::InOut);
            let is_out = matches!(p.direction, ParamDirection::Out | ParamDirection::InOut);
            let _ = writeln!(
                s,
                "                    new _Rt.Param({}, {}, {}, {}),",
                java_str(&p.name),
                descriptor(&p.tc)?,
                is_in,
                is_out
            );
        }
        let _ = writeln!(s, "                }},");
        let _ = writeln!(s, "                {}, {}));", descriptor(&sig.returns)?, sig.oneway);
    }
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s);
    let _ = writeln!(s, "    /** The repository id this servant answers for. */");
    let _ = writeln!(s, "    @Override");
    let _ = writeln!(s, "    public String _idlId() {{");
    let _ = writeln!(s, "        return _ID;");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s);
    let _ = writeln!(s, "    /** The resolved operation set, inherited members flattened. */");
    let _ = writeln!(s, "    @Override");
    let _ = writeln!(s, "    public java.util.Map<String, _Rt.Op> _idlOperations() {{");
    let _ = writeln!(s, "        return _OPS;");
    let _ = writeln!(s, "    }}");

    // One overridable method per operation, refusing until it is written.
    for (op_name, sig) in &ops {
        let method = method_name(op_name);
        let ins: Vec<_> = sig
            .params
            .iter()
            .filter(|p| matches!(p.direction, ParamDirection::In | ParamDirection::InOut))
            .collect();
        let outs: Vec<_> = sig
            .params
            .iter()
            .filter(|p| matches!(p.direction, ParamDirection::Out | ParamDirection::InOut))
            .collect();
        let returns_void = matches!(sig.returns, TypeCode::Void | TypeCode::Null);
        // A servant answering several values hands back `Object[]` rather than
        // a holder class: it is what `_Rt.dispatchCall` reads, and it is the
        // same tuple the Python servant returns — one rule, read from both ends.
        let multi = usize::from(!returns_void) + outs.len() > 1;
        let return_type = if multi {
            "Object[]".to_owned()
        } else if !returns_void {
            java_type(&sig.returns, cx)?
        } else if outs.len() == 1 {
            java_type(&outs[0].tc, cx)?
        } else {
            "void".to_owned()
        };
        let params: Vec<String> = ins
            .iter()
            .map(|p| Ok(format!("{} {}", java_type(&p.tc, cx)?, java_ident(&p.name))))
            .collect::<Result<_, String>>()?;
        let _ = writeln!(s);
        javadoc(
            &mut s,
            "    ",
            &item_doc(
                Some(&sig.annotations),
                &format!(
                    "Answers `{op_name}` — the name that travels, whatever this method is\n\
                     called. Refuses with `NO_IMPLEMENT` until overridden."
                ),
            ),
        );
        let _ = writeln!(s, "    public {return_type} {method}({}) {{", params.join(", "));
        let _ = writeln!(
            s,
            "        throw _Rt.Raise.didNotRun(\"IDL:omg.org/CORBA/NO_IMPLEMENT:1.0\", 0L);"
        );
        let _ = writeln!(s, "    }}");
    }

    // The generated dispatch: a `switch` and never reflection, so a name that
    // reaches a method has already been found in `_OPS`.
    let _ = writeln!(s);
    let _ = writeln!(s, "    /** Calls one operation by the name that travelled. */");
    let _ = writeln!(s, "    @Override");
    let _ = writeln!(s, "    public Object _invokeOp(String _operation, Object[] _argv) {{");
    for (op_name, sig) in &ops {
        let method = method_name(op_name);
        let ins: Vec<_> = sig
            .params
            .iter()
            .filter(|p| matches!(p.direction, ParamDirection::In | ParamDirection::InOut))
            .collect();
        let outs: Vec<_> = sig
            .params
            .iter()
            .filter(|p| matches!(p.direction, ParamDirection::Out | ParamDirection::InOut))
            .collect();
        let returns_void = matches!(sig.returns, TypeCode::Void | TypeCode::Null);
        let multi = usize::from(!returns_void) + outs.len() > 1;
        let args: Vec<String> = ins
            .iter()
            .enumerate()
            .map(|(i, p)| unbox_expr(&p.tc, &format!("_argv[{i}]"), cx))
            .collect::<Result<_, String>>()?;
        let call = format!("{method}({})", args.join(", "));
        let _ = writeln!(s, "        if (_operation.equals({})) {{", java_str(op_name));
        if multi {
            let _ = writeln!(s, "            return {call};");
        } else if !returns_void {
            let _ = writeln!(s, "            return {};", box_expr(&sig.returns, &call, cx)?);
        } else if outs.len() == 1 {
            let _ = writeln!(s, "            return {};", box_expr(&outs[0].tc, &call, cx)?);
        } else {
            let _ = writeln!(s, "            {call};");
            let _ = writeln!(s, "            return null;");
        }
        let _ = writeln!(s, "        }}");
    }
    let _ = writeln!(
        s,
        "        throw _Rt.Raise.didNotRun(\"IDL:omg.org/CORBA/BAD_OPERATION:1.0\", 0L);"
    );
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "}}");
    Ok((class, s))
}

fn emit_operation(
    s: &mut String,
    op_name: &str,
    sig: &OperationSig,
    cx: &Jx<'_>,
) -> Result<(), String> {
    let method = method_name(op_name);
    let ins: Vec<_> = sig
        .params
        .iter()
        .filter(|p| matches!(p.direction, ParamDirection::In | ParamDirection::InOut))
        .collect();
    let outs: Vec<_> = sig
        .params
        .iter()
        .filter(|p| matches!(p.direction, ParamDirection::Out | ParamDirection::InOut))
        .collect();
    let returns_void = matches!(sig.returns, TypeCode::Void | TypeCode::Null);

    // What the method answers with. One value is itself; several are a
    // generated holder, because Java has no tuple and an `out` parameter has no
    // Java form that is not a Holder class the OMG mapping only needs because
    // it lacks one.
    let result_class = format!("{}Result", upper_first(&method));
    let multi = usize::from(!returns_void) + outs.len() > 1;
    if multi {
        let _ = writeln!(s);
        javadoc(
            s,
            "    ",
            &format!(
                "What `{op_name}` answers with: the declared result first when it is not\n\
                 `void`, then the `out` and `inout` values in declaration order — §7.9.1's\n\
                 order, and the same one the Python target returns as a tuple."
            ),
        );
        let _ = writeln!(s, "    public static final class {result_class} {{");
        if !returns_void {
            let _ = writeln!(s, "        /** The declared result. */");
            let _ = writeln!(s, "        public {} _returns;", java_type(&sig.returns, cx)?);
        }
        for p in &outs {
            let _ =
                writeln!(s, "        /** The `{}` parameter, as the reply carried it. */", p.name);
            let _ =
                writeln!(s, "        public {} {};", java_type(&p.tc, cx)?, java_ident(&p.name));
        }
        let _ = writeln!(s, "    }}");
    }

    let return_type = if multi {
        result_class.clone()
    } else if !returns_void {
        java_type(&sig.returns, cx)?
    } else if outs.len() == 1 {
        java_type(&outs[0].tc, cx)?
    } else {
        "void".to_owned()
    };

    let params: Vec<String> = ins
        .iter()
        .map(|p| Ok(format!("{} {}", java_type(&p.tc, cx)?, java_ident(&p.name))))
        .collect::<Result<_, String>>()?;

    let _ = writeln!(s);
    javadoc(
        s,
        "    ",
        &item_doc(
            Some(&sig.annotations),
            &format!("Calls `{op_name}` — the name that travels, whatever this method is called."),
        ),
    );
    let _ = writeln!(s, "    public {return_type} {method}({}) {{", params.join(", "));
    let answers = multi || !returns_void || outs.len() == 1;
    let _ = writeln!(
        s,
        "        {}_Rt._call(_invoker, _ID, {},",
        if answers { "Object[] _r = " } else { "" },
        java_str(op_name)
    );
    let _ = writeln!(s, "                new _Rt.Arg[] {{");
    for p in &ins {
        let _ = writeln!(
            s,
            "                    _Rt._arg({}, {}, {}),",
            java_str(&p.name),
            descriptor(&p.tc)?,
            box_expr(&p.tc, &java_ident(&p.name), cx)?
        );
    }
    let _ = writeln!(s, "                }},");
    let _ = writeln!(s, "                {}, new _Rt.Out[] {{", descriptor(&sig.returns)?);
    for p in &outs {
        let _ = writeln!(
            s,
            "                    _Rt._out({}, {}),",
            java_str(&p.name),
            descriptor(&p.tc)?
        );
    }
    let _ = writeln!(s, "                }}, {});", sig.oneway);
    if multi {
        let _ = writeln!(s, "        {result_class} _out = new {result_class}();");
        let mut at = 0usize;
        if !returns_void {
            let _ =
                writeln!(s, "        _out._returns = {};", unbox_expr(&sig.returns, "_r[0]", cx)?);
            at += 1;
        }
        for p in &outs {
            let _ = writeln!(
                s,
                "        _out.{} = {};",
                java_ident(&p.name),
                unbox_expr(&p.tc, &format!("_r[{at}]"), cx)?
            );
            at += 1;
        }
        let _ = writeln!(s, "        return _out;");
    } else if !returns_void {
        let _ = writeln!(s, "        return {};", unbox_expr(&sig.returns, "_r[0]", cx)?);
    } else if outs.len() == 1 {
        let _ = writeln!(s, "        return {};", unbox_expr(&outs[0].tc, "_r[0]", cx)?);
    }
    let _ = writeln!(s, "    }}");
    Ok(())
}

fn upper_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
