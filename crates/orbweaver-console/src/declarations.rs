//! Everything a contract declares that is not an interface.
//!
//! # The measurement this module exists for
//!
//! Measured 2026-08-24 over `corpus/golden/` — 36 files, every one loaded
//! through [`crate::load::load_into`] and every registry entry counted:
//!
//! ```text
//! 208 entries    57 interfaces    151 the catalogue could not reach (72.6%)
//!   const 39   struct 47   typedef 35   exception 11   union 8   enum 7
//!   valuetype 3   native 1
//! ```
//!
//! Two of those files declare **no interface at all**, so the page an operator
//! opened for them was not merely partial:
//! `orbweaver-console catalog corpus/golden/33-const-values.idl --text`
//! printed `0 interfaces` and stopped, over a file that declares 22 constants
//! and a union, and the HTML page said *"the catalog is empty"* in so many
//! words. That is the failure shape [`crate::load`] already has a paragraph
//! about — *a page that renders a smaller surface than the one that exists is
//! one an operator makes a decision on* — arrived at from the other direction.
//!
//! It became worth fixing on the day it did because the constant batch landed
//! the same morning: a `fixed` constant now holds an **exact decimal** in the
//! registry and [`orbweaver_registry::diff`] compares it, so `idl-diff` will
//! call a `9.9d → 9.91d` edit conditionally breaking — while the one surface a
//! human reads had no row for the value at all.
//!
//! # Rendering, not deciding
//!
//! Same rule as the rest of this crate. A constant's value is
//! [`orbweaver_registry::ConstValue`]'s, spelled by [`const_text`] the way the
//! §5.3 differ spells it — pinned by a test that reads the differ's own change
//! text rather than by a comment, because two renderers of one value drift and
//! the one that drifts silently is the one on the page. A type's shape is read
//! off the [`TypeCode`] the registry derived; nothing here re-derives anything
//! from IDL.
//!
//! # What is deliberately not on the page
//!
//! **Union case labels.** A label is stored as the discriminator's own CDR
//! bytes, and turning those back into `case 18446744073709551615:` needs a
//! decoder. Three already exist and all three are private to their crates
//! (`orbweaver_gen::label_literal`, `orbweaver_dynamic::anyjson::label_json`,
//! `DynUnion::current_label`). A fourth copy here is the duplication the
//! workspace rule about encoding rules forbids, and a page that spells a label
//! *wrong* is worse than one that does not spell it. So a union row states its
//! discriminator type, its branches in wire order, and which index is the
//! default — and says the labels are not spelled, rather than leaving a gap a
//! reader fills in themselves. Promoting one of those three to a shared public
//! helper is what would close it.
//!
//! **Modules.** A module is not a registry entry — [`orbweaver_registry`]
//! walks through one and registers what is inside it — so there is nothing
//! held to draw. Every id on the page already carries its module path.
//!
//! # 계약이 선언한 것을 보여준다
//!
//! 카탈로그는 인터페이스만 그렸다. 골든 코퍼스 208개 엔트리 중 151개(72.6%)가
//! 어떤 읽기 화면에도 닿지 않았고, 두 파일은 "카탈로그가 비어 있다"고 적혔다.
//! 상수의 값은 이제 레지스트리가 정확한 십진수로 들고 있고 §5.3 차이 도구가
//! 그것을 비교하는데, 사람이 읽는 유일한 화면에만 그 값이 없었다.

use orbweaver_giop::typecode::TypeCode;
use orbweaver_registry::{ConstValue, Entry, Origin, Registry};

use crate::html::Markup;

/// A constant's value, or the reason there is none to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// This declaration is not a constant.
    NotAConstant,
    /// The value the registry folded, spelled by [`const_text`].
    Folded(String),
    /// A constant whose expression the registry could not evaluate. Rendered
    /// as its own sentence and never as `0`: `Entry::Const { value: None }`
    /// exists precisely so nothing downstream invents a plausible wrong
    /// number, and a page is downstream.
    Unevaluated,
}

/// One declaration that is not an interface, as the page reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarationRow {
    /// Repository id — identity on the wire, and what an operator allowlists
    /// or diffs against.
    pub id: String,
    /// The IDL keyword this entry was declared with: `const`, `struct`,
    /// `union`, `enum`, `exception`, `typedef`, `valuetype`,
    /// `abstract interface`, `native`.
    pub keyword: &'static str,
    /// The type, spelled as IDL by [`spell`]: a constant's declared type, a
    /// typedef's aliased type, the type's own name for everything else.
    pub declared: String,
    /// The value, for a constant.
    pub value: Value,
    /// Members, cases, enumerators — one already-spelled line each, in
    /// declaration order, which is wire order.
    pub members: Vec<String>,
    /// A sentence about the entry the members do not carry, or `None`.
    pub note: Option<String>,
    /// `ai_desc`, when the contract states one. Constants and types carry
    /// structured comments exactly as interfaces do.
    pub ai_desc: Option<String>,
    /// Where the entry came from. An ingested struct is as much a peer's
    /// description as an ingested interface is, and was as invisible.
    pub origin: Origin,
}

impl DeclarationRow {
    /// Whether a peer described this entry, as opposed to IDL declaring it.
    #[must_use]
    pub fn ingested(&self) -> bool {
        matches!(self.origin, Origin::Ingested(_))
    }

    /// The ingestion source label, when there is one.
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        match &self.origin {
            Origin::Ingested(s) => Some(s.as_str()),
            Origin::Idl => None,
        }
    }
}

/// Every non-interface entry in `registry`, in repository-id order.
///
/// The complement of the interface pass, by construction: this walks the same
/// `registry.ids()` and takes exactly what [`Registry::interface`] does not, so
/// no entry can fall between the two — which is the property
/// `every_entry_in_the_registry_reaches_a_row` asserts over the whole golden
/// corpus rather than over a file somebody remembered.
#[must_use]
pub fn collect(registry: &Registry) -> Vec<DeclarationRow> {
    let mut rows = Vec::new();
    for id in registry.ids() {
        let Some(entry) = registry.get(id) else { continue };
        let ai_desc =
            registry.annotations(id).and_then(|a| a.get("ai_desc")).map(ToOwned::to_owned);
        let origin = registry.origin(id).unwrap_or(Origin::Idl);
        let row = match entry {
            // The interface pass draws these.
            Entry::Interface(_) => continue,
            Entry::Const { tc, value } => DeclarationRow {
                id: id.clone(),
                keyword: "const",
                declared: spell(tc),
                value: match value {
                    Some(v) => Value::Folded(const_text(v)),
                    None => Value::Unevaluated,
                },
                members: Vec::new(),
                note: match value {
                    Some(ConstValue::Enum { id, ordinal, .. }) => {
                        Some(format!("enumerator of {id}, ordinal {ordinal}"))
                    }
                    _ => None,
                },
                ai_desc,
                origin,
            },
            Entry::Type(tc) => {
                let (members, note) = shape(tc);
                DeclarationRow {
                    id: id.clone(),
                    keyword: keyword(tc),
                    declared: declared_type(tc),
                    value: Value::NotAConstant,
                    members,
                    note,
                    ai_desc,
                    origin,
                }
            }
        };
        rows.push(row);
    }
    rows
}

/// The IDL keyword a derived [`TypeCode`] was declared with.
///
/// Every arm is named. A `_` here is how a `TypeCode` variant added later
/// reaches the page as "type" and nobody notices; the compiler is the thing
/// that should notice.
fn keyword(tc: &TypeCode) -> &'static str {
    match tc {
        TypeCode::Struct { .. } => "struct",
        TypeCode::Union { .. } => "union",
        TypeCode::Enum { .. } => "enum",
        TypeCode::Except { .. } => "exception",
        TypeCode::Alias { .. } => "typedef",
        TypeCode::Value { modifier: 2, .. } => "abstract valuetype",
        TypeCode::Value { .. } => "valuetype",
        TypeCode::AbstractInterface { .. } => "abstract interface",
        TypeCode::Native { .. } => "native",
        TypeCode::ObjRef { .. } => "interface",
        // A registry entry that is a bare type rather than a named
        // declaration. Not reachable from IDL today; drawn rather than
        // dropped, because an ingested entry is whatever a peer sent.
        _ => "type",
    }
}

/// What the row's "type" column says: a typedef's aliased type, and every
/// other declaration's own name.
fn declared_type(tc: &TypeCode) -> String {
    match tc {
        TypeCode::Alias { aliased, .. } => spell(aliased),
        other => spell(other),
    }
}

/// The members, cases or enumerators of a declaration, plus a sentence about
/// anything the member list cannot carry.
fn shape(tc: &TypeCode) -> (Vec<String>, Option<String>) {
    match tc {
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => (
            members.iter().map(|m| format!("{} {}", spell(&m.tc), m.name)).collect(),
            (members.is_empty()).then(|| "no members".to_owned()),
        ),
        TypeCode::Enum { members, .. } => (members.clone(), None),
        TypeCode::Value { members, base, .. } => (
            members
                .iter()
                .map(|m| {
                    let visibility = match m.visibility {
                        1 => "public",
                        0 => "private",
                        // A peer's short we do not recognise survives rather
                        // than being normalised to one of two guesses, which
                        // is what the wire field's own documentation asks for.
                        other => return format!("visibility {other} {} {}", spell(&m.tc), m.name),
                    };
                    format!("{visibility} {} {}", spell(&m.tc), m.name)
                })
                .collect(),
            base.as_ref().map(|b| format!("derived from {}", spell(b))),
        ),
        TypeCode::Union { cases, discriminator, default_index, .. } => {
            let members = cases
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let which = if i as i32 == *default_index { "default" } else { "case" };
                    format!("{which} → {} {}", spell(&c.tc), c.name)
                })
                .collect();
            // See the module docs: the labels are bytes, and this crate is not
            // going to become the fourth place that decodes them.
            (
                members,
                Some(format!(
                    "switch ({}) — the case labels are stored as discriminator bytes and are \
                     not spelled here",
                    spell(discriminator)
                )),
            )
        }
        TypeCode::Native { name, .. } => (
            Vec::new(),
            Some(format!(
                "`native {name}` has no CDR encoding at any wire version: nothing marshals it \
                 and both emitters skip it by name"
            )),
        ),
        _ => (Vec::new(), None),
    }
}

/// A [`TypeCode`] spelled as the IDL that would declare it.
///
/// A named type is spelled by **name**, never expanded. That is what makes
/// this terminate on `corpus/golden/15-forward-recursive.idl`, where a struct
/// holds a sequence of itself: the recursion always passes through a named
/// type, and a name is where it stops. The `depth` guard behind it costs
/// nothing and is the difference between a wrong page and a hung console if a
/// peer ever sends a `TypeCode` this reasoning does not hold for.
#[must_use]
pub fn spell(tc: &TypeCode) -> String {
    spell_to(tc, 8)
}

fn spell_to(tc: &TypeCode, depth: u8) -> String {
    if depth == 0 {
        return "…".to_owned();
    }
    match tc {
        TypeCode::Null => "null".to_owned(),
        TypeCode::Void => "void".to_owned(),
        TypeCode::Short => "short".to_owned(),
        TypeCode::Long => "long".to_owned(),
        TypeCode::UShort => "unsigned short".to_owned(),
        TypeCode::ULong => "unsigned long".to_owned(),
        TypeCode::LongLong => "long long".to_owned(),
        TypeCode::ULongLong => "unsigned long long".to_owned(),
        TypeCode::Float => "float".to_owned(),
        TypeCode::Double => "double".to_owned(),
        TypeCode::LongDouble => "long double".to_owned(),
        TypeCode::Boolean => "boolean".to_owned(),
        TypeCode::Char => "char".to_owned(),
        TypeCode::WChar => "wchar".to_owned(),
        TypeCode::Octet => "octet".to_owned(),
        TypeCode::Any => "any".to_owned(),
        // Qualified, because that is the only spelling the compiler accepts —
        // the rule `corpus/negative` has a case for.
        TypeCode::TypeCode => "::CORBA::TypeCode".to_owned(),
        TypeCode::Principal => "Principal".to_owned(),
        TypeCode::String(0) => "string".to_owned(),
        TypeCode::String(bound) => format!("string<{bound}>"),
        TypeCode::WString(0) => "wstring".to_owned(),
        TypeCode::WString(bound) => format!("wstring<{bound}>"),
        // `const fixed TAX_RATE = 9.9d;` declares no digits and no scale, and
        // the registry stores that as `Fixed { digits: 0, scale: 0 }`. Spelled
        // `fixed<0,0>` it read as a *declared* precision of nothing, which is
        // not IDL anybody can write and not what the contract says.
        TypeCode::Fixed { digits: 0, scale: 0 } => "fixed".to_owned(),
        TypeCode::Fixed { digits, scale } => format!("fixed<{digits},{scale}>"),
        TypeCode::Sequence { element, bound: 0 } => {
            format!("sequence<{}>", spell_to(element, depth - 1))
        }
        TypeCode::Sequence { element, bound } => {
            format!("sequence<{}, {bound}>", spell_to(element, depth - 1))
        }
        TypeCode::Array { element, length } => {
            format!("{}[{length}]", spell_to(element, depth - 1))
        }
        // Every named construct stops here. `Alias` included: a typedef used
        // as a member type is spelled with its own name, which is what the
        // contract says and what a reader is looking for.
        TypeCode::ObjRef { name, .. }
        | TypeCode::Struct { name, .. }
        | TypeCode::Union { name, .. }
        | TypeCode::Enum { name, .. }
        | TypeCode::Alias { name, .. }
        | TypeCode::Except { name, .. }
        | TypeCode::Value { name, .. }
        | TypeCode::AbstractInterface { name, .. }
        | TypeCode::Native { name, .. } => name.clone(),
        TypeCode::Recursive(id) => format!("(recursive: {id})"),
    }
}

/// A constant's value, spelled exactly as [`orbweaver_registry::diff`] spells
/// it in a §5.3 verdict.
///
/// **The two must agree**, and a comment saying so is not a mechanism: a
/// release note that reads *"BALANCE changed from 9.9 to 9.91"* beside a
/// catalogue row that reads `Fixed { unscaled: 991, scale: 2 }` is one number
/// in two spellings, and an operator reconciling them by hand is the cost.
/// `the_page_spells_a_value_the_way_the_differ_spells_it` reads the differ's
/// own change text and compares strings, so drift is a red test rather than a
/// discovery.
///
/// A `char` or `wchar` constant is its code point, because that is the value
/// the registry holds and the number the differ prints; the row's type column
/// says which of the two it is.
#[must_use]
pub fn const_text(v: &ConstValue) -> String {
    // `as_decimal` first, for the reason it exists: a `fixed`'s scale is the
    // part that is easy to get wrong, and dividing by a power of ten to print
    // one re-introduces the binary float the type exists to avoid.
    if let Some(decimal) = v.as_decimal() {
        return decimal;
    }
    match v {
        ConstValue::Int(i) => i.to_string(),
        ConstValue::Float(f) => format!("{f}"),
        ConstValue::Bool(b) => b.to_string(),
        ConstValue::Str(s) => format!("{s:?}"),
        ConstValue::Enum { member, .. } => member.clone(),
        ConstValue::Fixed { .. } => unreachable!("as_decimal answers for Fixed"),
    }
}

/// How many of each keyword, in keyword order — the header's tally.
#[must_use]
pub fn tally(rows: &[DeclarationRow]) -> Vec<(&'static str, usize)> {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for row in rows {
        match counts.iter_mut().find(|(k, _)| *k == row.keyword) {
            Some((_, n)) => *n += 1,
            None => counts.push((row.keyword, 1)),
        }
    }
    counts.sort_unstable();
    counts
}

/// The declarations section of the HTML page.
#[must_use]
pub fn block(rows: &[DeclarationRow]) -> Markup {
    if rows.is_empty() {
        return Markup::labelled(
            "p",
            "absent",
            "the loaded contracts declare no constant, type or exception of their own",
        );
    }
    let mut head = Markup::empty();
    for column in ["declaration", "type", "value", "shape", "provenance"] {
        head.push(Markup::labelled("th", "", column));
    }
    let mut table = Markup::element("tr", "", head);

    for row in rows {
        let mut cells = Markup::empty();

        let mut name = Markup::labelled("span", "badge b-scope", row.keyword);
        name.push(Markup::labelled("div", "id", &row.id));
        if let Some(desc) = &row.ai_desc {
            name.push(Markup::labelled("div", "desc", desc));
        }
        cells.push(Markup::element("td", "", name));

        cells.push(Markup::element("td", "", Markup::labelled("span", "mono", &row.declared)));

        cells.push(Markup::element("td", "", value_cell(&row.value)));

        let mut shape = Markup::empty();
        for member in &row.members {
            shape.push(Markup::labelled("div", "mono", member));
        }
        if let Some(note) = &row.note {
            shape.push(Markup::labelled("div", "note", note));
        }
        if shape.is_empty() {
            shape = Markup::labelled("span", "absent", "nothing further declared");
        }
        cells.push(Markup::element("td", "", shape));

        let provenance = match row.source() {
            Some(source) => {
                Markup::labelled("span", "badge b-ingested", &format!("ingested from {source}"))
            }
            None => Markup::labelled("span", "badge b-idl", "from IDL"),
        };
        cells.push(Markup::element("td", "", provenance));

        table.push(Markup::element("tr", "", cells));
    }
    Markup::element("div", "scroll", Markup::element("table", "", table))
}

fn value_cell(value: &Value) -> Markup {
    match value {
        Value::Folded(text) => Markup::labelled("span", "mono", text),
        // The one row where an absence is a finding rather than a shrug: the
        // contract states a constant and the registry has no value for it.
        Value::Unevaluated => Markup::labelled(
            "span",
            "badge b-unknown",
            "the registry could not evaluate this expression",
        ),
        Value::NotAConstant => Markup::labelled("span", "absent", "not a constant"),
    }
}

/// The declarations section for a terminal.
#[must_use]
pub fn render_text(rows: &[DeclarationRow]) -> String {
    let mut out = String::new();
    let counts: Vec<String> =
        tally(rows).iter().map(|(keyword, n)| format!("{keyword}={n}")).collect();
    out.push_str(&format!(
        "\nDECLARATIONS: {} that are not interfaces{}\n",
        rows.len(),
        if counts.is_empty() { String::new() } else { format!(" — {}", counts.join(" ")) }
    ));
    for row in rows {
        let origin = match row.source() {
            Some(source) => format!(" [INGESTED from {source}]"),
            None => String::new(),
        };
        out.push_str(&format!("\n{} {} [{}]{origin}\n", row.keyword, row.id, row.declared));
        match &row.value {
            Value::Folded(text) => out.push_str(&format!("  value: {text}\n")),
            Value::Unevaluated => {
                out.push_str("  value: the registry could not evaluate this expression\n");
            }
            Value::NotAConstant => {}
        }
        if let Some(desc) = &row.ai_desc {
            out.push_str(&format!("  desc: {desc}\n"));
        }
        for member in &row.members {
            out.push_str(&format!("  - {member}\n"));
        }
        if let Some(note) = &row.note {
            out.push_str(&format!("  note: {note}\n"));
        }
    }
    out
}
