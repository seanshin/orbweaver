//! Value-driven marshalling: put any CDR value on the wire from its `TypeCode`.
//!
//! Everything so far has marshalled by writing the encoder calls by hand, which
//! works when the types are known at compile time and is exactly what the AI
//! path cannot do. `docs/PLAN.md` §4.6 projects an interface as
//! `search_interfaces` → `describe_interface` → `invoke_operation`, and the
//! last step receives an operation name and a bag of values chosen at runtime.
//! Something has to turn those into bytes using only the registry's description
//! of the type. That is this crate.
//!
//! It is the Dynamic Invocation Interface's marshalling half (§4.4), the layer
//! AnyJSON (§4.5) converts into, and the reason `invoke_operation` can exist at
//! all.
//!
//! # Alignment is not this module's to decide
//!
//! A value is encoded into an `Encoder` the caller has already positioned, so
//! padding lands where the enclosing message says it should. Building a value
//! in a detached buffer that starts at offset zero is the single mistake this
//! project has made most often — three times in Phase 1 alone — and the fix is
//! always `Encoder::continuing_at`, never a change here.
//!
//! # Strictness
//!
//! A `Value` that does not match its `TypeCode` is an error with a message
//! naming the path to the offending member, because a dynamic invoker's
//! diagnostics are the only thing standing between a caller and a silently
//! malformed message. §3.3 calls diagnostics a product; nowhere is that truer
//! than here.

#![deny(missing_docs)]

pub mod anyjson;
pub mod dynany;
pub mod invoke;
pub mod json;

use std::fmt;

use orbweaver_cdr::{Decoder, Encoder};
use orbweaver_giop::Ior;
use orbweaver_giop::Version;
use orbweaver_giop::codeset::{CodeSetId, WideCodec};
use orbweaver_giop::typecode::{TypeCode, UnionCase};

/// A CDR value, shaped to match `TypeCode` one variant at a time.
///
/// Deliberately not a JSON-like value: an `any` carrying a `short` and one
/// carrying a `long` are different on the wire, so a number that has forgotten
/// its width cannot be marshalled. AnyJSON (§4.5) converts *into* this type,
/// and that conversion is where the width is recovered from the `TypeCode`.
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub enum Value {
    Bool(bool),
    Octet(u8),
    Char(u8),
    WChar(char),
    Short(i16),
    UShort(u16),
    Long(i32),
    ULong(u32),
    LongLong(i64),
    ULongLong(u64),
    Float(f32),
    Double(f64),
    /// 16 raw octets: `long double` has no portable Rust equivalent, and
    /// inventing one would lose bits the peer sent us.
    LongDouble([u8; 16]),
    String(String),
    WString(String),
    /// An enumerator, held by name rather than ordinal.
    ///
    /// The ordinal is what travels, but the name is what a caller means, and
    /// §5.3 measured what happens when the two are conflated: reordering an
    /// enum silently changes the meaning of every value already in flight.
    Enum(String),
    /// Members in declaration order. Also used for exceptions.
    Struct(Vec<(String, Value)>),
    /// The discriminator, then the value of the branch it selects.
    Union {
        /// The discriminator value; its type comes from the `TypeCode`.
        discriminator: Box<Value>,
        /// The selected branch's value, or `None` for a union whose selected
        /// branch has no member.
        value: Option<Box<Value>>,
    },
    /// A sequence or an array; the `TypeCode` says which, and an array's length
    /// is checked against it.
    List(Vec<Value>),
    /// A value together with the `TypeCode` describing it.
    Any(Box<TypeCode>, Box<Value>),
    /// An object reference, or nil.
    ObjRef(Option<Ior>),
    /// A `TypeCode` as a value in its own right — `tk_TypeCode`, kind 12.
    ///
    /// Distinct from the `TypeCode` inside [`Value::Any`], which describes the
    /// value beside it. Here the type code *is* the value: it is what
    /// `::CORBA::TypeCode describe(...)` returns and what every Interface
    /// Repository description is made of. Without this variant the static
    /// path carried a type the dynamic path could not, so §8's *static equals
    /// dynamic* oracle was not weaker for those operations, it was
    /// inapplicable — see `docs/decisions/D008-anyjson-self-description.md`.
    TypeCode(Box<TypeCode>),
}

/// Why a value could not be marshalled or unmarshalled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// Where in the value it went wrong, e.g. `order.lines[2].quantity`.
    pub path: String,
    /// What went wrong, phrased as something to fix.
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "at {}: {}", self.path, self.message)
        }
    }
}

impl std::error::Error for Error {}

/// The result of marshalling.
pub type Result<T> = std::result::Result<T, Error>;

/// Tracks where we are inside a value so a diagnostic can say so.
///
/// A borrowed linked list rather than a `String` built as we descend: the happy
/// path allocates nothing, and the path is only rendered when something is
/// already wrong.
#[derive(Clone, Copy)]
pub(crate) struct Path<'a> {
    parent: Option<&'a Path<'a>>,
    step: Step<'a>,
    /// The constructed type this node entered, when it entered one.
    ///
    /// This is how a recursive type is marshalled at all. The registry cannot
    /// hold a cycle, so it represents one as [`TypeCode::Recursive`] carrying
    /// only the repository id of the type it points back at. A marshaller that
    /// looks at the `TypeCode` alone therefore has nothing to encode against —
    /// which is what ours did, refusing every non-empty recursive value with
    /// "expected a value of type an indirection". The path already runs from
    /// the root to the current member, so the enclosing types are exactly what
    /// it is standing on: recording them here turns the marker back into the
    /// type it names, for the cost of one pointer per node.
    ///
    /// The *wire* has no indirection in it. CDR indirections appear when a
    /// `TypeCode` is itself marshalled inside an `any`; a recursive **value**
    /// is plain nested structs, and its depth is decided by the sequence
    /// lengths, not by the type. So resolving the marker and continuing inline
    /// is the whole of it.
    ///
    /// [`anyjson`] walks on the same path for the same reason: it had its own
    /// string path and no `open` chain, and so repeated the refusal above
    /// ("… is not a value of <id>") for three phases after the CDR side was
    /// fixed. One mechanism, shared, is how the two halves stay in agreement.
    open: Option<&'a TypeCode>,
}

#[derive(Clone, Copy)]
enum Step<'a> {
    Root,
    Member(&'a str),
    Index(usize),
}

impl<'a> Path<'a> {
    pub(crate) fn root() -> Self {
        Path { parent: None, step: Step::Root, open: None }
    }

    pub(crate) fn member(&'a self, name: &'a str) -> Self {
        Path { parent: Some(self), step: Step::Member(name), open: None }
    }

    pub(crate) fn index(&'a self, i: usize) -> Self {
        Path { parent: Some(self), step: Step::Index(i), open: None }
    }

    /// A node recording that marshalling is now inside `tc`.
    ///
    /// [`Step::Root`] because this is bookkeeping, not a step a reader took:
    /// it renders as nothing, so error paths read exactly as they did before.
    pub(crate) fn entering(&'a self, tc: &'a TypeCode) -> Self {
        Path { parent: Some(self), step: Step::Root, open: Some(tc) }
    }

    /// The enclosing type `id` names, innermost first.
    fn resolve(&self, id: &str) -> Option<&'a TypeCode> {
        if let Some(tc) = self.open
            && type_id_of(tc) == Some(id)
        {
            return Some(tc);
        }
        self.parent?.resolve(id)
    }

    /// How many constructed types this path is currently inside.
    ///
    /// The bound this feeds is a wire-safety measure, not a style limit: on
    /// decode the nesting depth comes from the byte stream, so a crafted
    /// message could otherwise drive our own recursion until the stack ends.
    /// A parser that a peer can crash is the hazard this project chose Rust
    /// for, and `unsafe_code = "forbid"` does not cover stack exhaustion.
    fn depth(&self) -> usize {
        usize::from(self.open.is_some()) + self.parent.map_or(0, Path::depth)
    }

    pub(crate) fn render(&self) -> String {
        let mut out = match self.parent {
            Some(p) => p.render(),
            None => String::new(),
        };
        match self.step {
            Step::Root => {}
            Step::Member(name) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(name);
            }
            Step::Index(i) => out.push_str(&format!("[{i}]")),
        }
        out
    }

    pub(crate) fn fail<T>(&self, message: impl Into<String>) -> Result<T> {
        Err(Error { path: self.render(), message: message.into() })
    }
}

/// The wide-character codec a value is marshalled with.
///
/// `wstring` is the one part of CDR whose encoding is not determined by the
/// `TypeCode`: it depends on the GIOP version and the negotiated wchar
/// codeset, and Phase 1 established the hard way that peers do **not** infer
/// wide-char byte order from the message byte order — a big-endian client
/// talking to omniORB or JacORB gets byte-swapped text unless a BOM says
/// otherwise. `WideCodec` already encodes all of that.
///
/// The first version of this module re-implemented wstring here instead of
/// using it, and a stock omniORB rejected the result immediately: a leading
/// U+FEFF surviving into the decoded value one way, and `UNKNOWN` from the peer
/// the other. Duplicating knowledge the project had already paid for is how it
/// came back.
fn default_codec() -> WideCodec {
    // GIOP 1.2 and UTF-16: what both fixtures negotiate in practice, and what
    // an encapsulated `any` uses regardless of the connection's version.
    WideCodec::new(Version::V1_2, CodeSetId::UTF_16).expect("1.2 + UTF-16 is always valid")
}

/// Encodes `value` as `tc` into `e`, which the caller has already positioned.
///
/// The encoder's origin matters and is not adjusted here; see the module
/// documentation. Wide strings use [`default_codec`]; call [`encode_with`] to
/// supply the one a connection actually negotiated.
pub fn encode(e: &mut Encoder, tc: &TypeCode, value: &Value) -> Result<()> {
    encode_with(e, tc, value, default_codec())
}

/// Encodes with a specific wide-character codec.
pub fn encode_with(e: &mut Encoder, tc: &TypeCode, value: &Value, wide: WideCodec) -> Result<()> {
    encode_at(e, tc, value, &Path::root(), wide)
}

/// Encodes `value` as `tc` under `name`, so a diagnostic says which value.
///
/// [`encode`] starts its path inside the value: a bounded string that does not
/// fit renders as "string is bounded at 8 but 9 were given", and a member of
/// it as "at tag[2]: …". That is complete for one value and useless for one
/// of several — an operation's arguments, a struct's fields marshalled one by
/// one — where the reader's first question is *which*. This entry point roots
/// the path at `name` ("at key: …", "at key.tag[2]: …"), on the same [`Path`]
/// the marshaller and [`anyjson`] walk — so no caller has to prepend the name
/// to a rendered sentence, and none can end up with it twice.
pub fn encode_named(e: &mut Encoder, tc: &TypeCode, value: &Value, name: &str) -> Result<()> {
    encode_named_with(e, tc, value, name, default_codec())
}

/// [`encode_named`] with a specific wide-character codec.
pub fn encode_named_with(
    e: &mut Encoder,
    tc: &TypeCode,
    value: &Value,
    name: &str,
    wide: WideCodec,
) -> Result<()> {
    let root = Path::root();
    encode_at(e, tc, value, &root.member(name), wide)
}

/// Decodes a value of type `tc` from `d`.
pub fn decode(d: &mut Decoder<'_>, tc: &TypeCode) -> Result<Value> {
    decode_with(d, tc, default_codec())
}

/// Decodes with a specific wide-character codec.
pub fn decode_with(d: &mut Decoder<'_>, tc: &TypeCode, wide: WideCodec) -> Result<Value> {
    decode_at(d, tc, &Path::root(), wide)
}

/// Decodes a value of type `tc` under `name`; the read-side twin of
/// [`encode_named`], for a reply's `out` parameters.
pub fn decode_named(d: &mut Decoder<'_>, tc: &TypeCode, name: &str) -> Result<Value> {
    decode_named_with(d, tc, name, default_codec())
}

/// [`decode_named`] with a specific wide-character codec.
pub fn decode_named_with(
    d: &mut Decoder<'_>,
    tc: &TypeCode,
    name: &str,
    wide: WideCodec,
) -> Result<Value> {
    let root = Path::root();
    decode_at(d, tc, &root.member(name), wide)
}

/// Follows `alias` links to the type that actually governs encoding.
///
/// A typedef changes the name and the repository id and nothing about the
/// bytes, so every match below sees through it rather than each arm
/// remembering to.
fn resolved(tc: &TypeCode) -> &TypeCode {
    let mut t = tc;
    while let TypeCode::Alias { aliased, .. } = t {
        t = aliased;
    }
    t
}

fn wrong_kind<T>(p: &Path<'_>, tc: &TypeCode, value: &Value) -> Result<T> {
    p.fail(format!("expected a value of type {}, got {}", describe(tc), kind_of(value)))
}

/// How a construct `docs/PLAN.md` §4.4 defers is named in a refusal, or `None`
/// for a type the v1 wire does carry.
///
/// The kind word and the type's own name — `valuetype Money`, `abstract
/// interface Describable`, `fixed<9,2>` — which is the spelling the generated
/// Python runtime produces from its own `_DEFERRED` format string
/// (`crates/orbweaver-gen/src/python_rt.py`). Aliases are followed, because a
/// `typedef` renames a construct without making the wire able to carry it.
pub fn deferred_wire_name(tc: &TypeCode) -> Option<String> {
    Some(match resolved(tc) {
        TypeCode::Value { name, .. } => format!("valuetype {name}"),
        TypeCode::AbstractInterface { name, .. } => format!("abstract interface {name}"),
        TypeCode::Fixed { digits, scale } => format!("fixed<{digits},{scale}>"),
        _ => return None,
    })
}

/// The head every §4.4 refusal shares, whichever layer raises it.
///
/// One function rather than five literals: the CDR path, the AnyJSON path and
/// the generated Python runtime each refuse the same three constructs, and a
/// reader who meets one of those refusals has to be able to find the other two
/// by the same words. `deferred_sentences_agree_across_the_layers` pins the
/// Rust pair equal, and `orbweaver-gen`'s `python_target` pins the Python one
/// against this — they are in different crates and Python cannot share code,
/// so the equality is held by a test rather than by hope.
///
/// # Why this is `pub` and not `pub(crate)`
///
/// It was `pub(crate)` until 2026-08-24, and that visibility was the whole
/// defect: a layer in another crate could not call it, so it wrote its own
/// sentence, and `deferred_sentence_agreement` — which lives here — could only
/// pin what was inside this crate. Measured that day: **twelve literals in two
/// other crates for the four facts these functions own** (`orbweaver-gen`'s
/// four skip reasons, `orbweaver-test`'s `json_unmapped` and `why_unsupported`,
/// four each), and one of the twelve had gone false — `prop.rs` quoted
/// `from_json` as answering `"cannot cross yet"` for a `fixed`, which that
/// layer stopped saying on 2026-08-21 when the arm above it landed. Nothing
/// went red, because the pin's scope was a crate and the fact's scope is the
/// workspace. A refusal sentence's home is a function, and `pub(crate)` is how
/// a fact escapes its home.
pub fn deferred_wire_head(what: &str) -> String {
    format!("{what} is not marshalled by the v1 wire (docs/PLAN.md §4.4)")
}

/// The whole sentence a **peer-fed** document or stream is refused with.
///
/// The tail is D008's distinction, said out loud: a *description* of a deferred
/// type crosses (a `TypeCode` is a value, and `tc_to_json` spells `tk_value`,
/// `tk_abstract_interface` and `tk_fixed` structurally), an *instance* does
/// not. A refusal that said only "cannot cross yet" would read as the whole
/// type being unreachable, and a reader would stop sending the description too
/// — which is the one thing D008 decided must keep working.
pub fn deferred_wire_sentence(what: &str) -> String {
    format!(
        "{}; the TypeCode describing it reads, the value behind it does not",
        deferred_wire_head(what)
    )
}

/// How a construct with **no wire form at all** is named in a refusal, or
/// `None` for a construct some version of the wire does carry — including the
/// three [`deferred_wire_name`] answers for.
///
/// The fourth family, and the reason it is a separate function rather than a
/// fourth arm of the one above: `native X;` is not deferred. §4.4's three have
/// a wire form the specification defines and this version has not implemented;
/// a native has none to implement, in v1 or in any later version, because it
/// names a type only a language mapping knows. Aliases are followed for the
/// same reason as above — a `typedef` renames a construct without giving the
/// wire a way to carry it.
pub fn unmarshallable_wire_name(tc: &TypeCode) -> Option<String> {
    Some(match resolved(tc) {
        TypeCode::Native { name, .. } => format!("native {name}"),
        _ => return None,
    })
}

/// The head every "no wire form at all" refusal shares, whichever layer raises
/// it — the counterpart of [`deferred_wire_head`] for the fourth family.
///
/// It exists for the reason that one does and was written for the reason that
/// one was *not*: when `native` landed (2026-08-21) the helper above was not on
/// that branch, so five layers wrote five sentences for one fact and two of
/// them told the reader something false — the AnyJSON read direction said
/// `"IDL:m/Handle:1.0 cannot cross yet"`, and the dynamic navigator's default
/// pointed at `docs/PLAN.md §4.4`. Both invite the reader to wait for a release
/// that will never carry it.
pub fn unmarshallable_wire_head(what: &str) -> String {
    format!(
        "{what} has no wire form at all: it names a type only a language mapping knows, and no \
         version of the wire marshals one"
    )
}

/// The whole sentence a **peer-fed** document or stream is refused with.
///
/// # Why this reads differently from [`deferred_wire_sentence`]
///
/// The two tails are the two different things a reader has to be told, and
/// swapping them would be a lie in either direction.
///
/// §4.4's tail is D008's asymmetry: the *description* crosses and the
/// *instance* does not, so keep sending the TypeCode. A native's tail is the
/// absence of a deferral: there is no wire form waiting to be implemented, so
/// the answer will not change in a later version and the fix is to change the
/// contract. The word **"yet"** must never appear in it, and the section must
/// be named only to say it does not apply — a refusal that read as a §4.4
/// deferral would send the reader to a plan entry that does not name this
/// construct and never will.
///
/// `deferred_sentence_agreement` holds both of those, and holds the four
/// families to one source each; `orbweaver-gen`'s `python_target` holds the
/// generated Python runtime's `_UNMARSHALLABLE` equal to this string across the
/// crate boundary, because Python cannot import a Rust constant.
pub fn unmarshallable_wire_sentence(what: &str) -> String {
    format!(
        "{}; this is not one of docs/PLAN.md §4.4's deferrals — those have a wire form this \
         version has not implemented, and there is none here to implement",
        unmarshallable_wire_head(what)
    )
}

fn describe(tc: &TypeCode) -> String {
    match tc {
        TypeCode::Struct { name, .. }
        | TypeCode::Union { name, .. }
        | TypeCode::Enum { name, .. }
        | TypeCode::Except { name, .. }
        | TypeCode::Alias { name, .. }
        | TypeCode::ObjRef { name, .. }
        | TypeCode::Value { name, .. }
        | TypeCode::AbstractInterface { name, .. }
        | TypeCode::Native { name, .. } => name.clone(),
        TypeCode::Sequence { element, bound } if *bound > 0 => {
            format!("sequence<{}, {bound}>", describe(element))
        }
        TypeCode::Sequence { element, .. } => format!("sequence<{}>", describe(element)),
        TypeCode::Array { element, length } => format!("{}[{length}]", describe(element)),
        TypeCode::String(0) => "string".into(),
        TypeCode::String(n) => format!("string<{n}>"),
        TypeCode::WString(0) => "wstring".into(),
        TypeCode::WString(n) => format!("wstring<{n}>"),
        other => match other.kind() {
            Some(k) => format!("{k:?}").to_lowercase(),
            None => "an indirection".into(),
        },
    }
}

fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Bool(_) => "a boolean",
        Value::Octet(_) => "an octet",
        Value::Char(_) => "a char",
        Value::WChar(_) => "a wchar",
        Value::Short(_) => "a short",
        Value::UShort(_) => "an unsigned short",
        Value::Long(_) => "a long",
        Value::ULong(_) => "an unsigned long",
        Value::LongLong(_) => "a long long",
        Value::ULongLong(_) => "an unsigned long long",
        Value::Float(_) => "a float",
        Value::Double(_) => "a double",
        Value::LongDouble(_) => "a long double",
        Value::String(_) => "a string",
        Value::WString(_) => "a wstring",
        Value::Enum(_) => "an enumerator",
        Value::Struct(_) => "a struct",
        Value::Union { .. } => "a union",
        Value::List(_) => "a list",
        Value::Any(..) => "an any",
        Value::ObjRef(_) => "an object reference",
        Value::TypeCode(_) => "a typecode",
    }
}

fn cdr<T>(p: &Path<'_>, r: std::result::Result<T, orbweaver_cdr::Error>) -> Result<T> {
    r.map_err(|e| Error { path: p.render(), message: e.to_string() })
}

/// The repository id of a constructed type, which is what a
/// [`TypeCode::Recursive`] marker names.
fn type_id_of(tc: &TypeCode) -> Option<&str> {
    match tc {
        TypeCode::Struct { id, .. }
        | TypeCode::Except { id, .. }
        | TypeCode::Union { id, .. }
        | TypeCode::Alias { id, .. }
        | TypeCode::Enum { id, .. }
        | TypeCode::ObjRef { id, .. }
        | TypeCode::Value { id, .. }
        | TypeCode::AbstractInterface { id, .. }
        | TypeCode::Native { id, .. } => Some(id),
        _ => None,
    }
}

/// How deep a chain of constructed types either direction will follow.
///
/// Reached only through a recursive type: nothing else nests this far. On
/// decode the depth is chosen by the sender, so this is the bound that keeps a
/// hostile message from exhausting the stack; on encode it bounds a value we
/// built ourselves, where hitting it means a bug rather than an attack. Both
/// report the same way, because a marshaller that treats its own overflow as
/// impossible is how the first one gets found in production.
pub(crate) const MAX_NESTING: usize = 64;

/// The type a recursive marker names, or an error naming what went wrong.
pub(crate) fn open_recursive<'a>(id: &str, p: &Path<'a>) -> Result<&'a TypeCode> {
    if p.depth() >= MAX_NESTING {
        return p.fail(format!(
            "recursive type {id} nests deeper than {MAX_NESTING} levels; refusing to follow it"
        ));
    }
    match p.resolve(id) {
        Some(tc) => Ok(tc),
        // Reachable when a `Recursive` marker is marshalled outside the type
        // it points at — a TypeCode assembled by hand, or a fragment lifted
        // out of its parent. Saying which id could not be resolved is the
        // difference between a fixable report and a shrug.
        None => p.fail(format!(
            "recursive type {id} is not inside the type it names, so the cycle cannot be \
             resolved; marshal the whole type rather than the fragment"
        )),
    }
}

fn encode_at(
    e: &mut Encoder,
    tc: &TypeCode,
    v: &Value,
    p: &Path<'_>,
    wide: WideCodec,
) -> Result<()> {
    // An alias is transparent to the bytes and *not* transparent to a cycle:
    // the registry's marker for `typedef sequence<Tree> TreeSeq; struct Tree {
    // TreeSeq kids; }` names TreeSeq, not Tree. Recording the alias before
    // seeing through it is what lets that marker resolve; `resolved()` still
    // decides every byte.
    if let TypeCode::Alias { aliased, .. } = tc {
        let here = p.entering(tc);
        return encode_at(e, aliased, v, &here, wide);
    }
    match (resolved(tc), v) {
        (TypeCode::Recursive(id), _) => {
            let target = open_recursive(id, p)?;
            let entered = p.entering(target);
            encode_at(e, target, v, &entered, wide)
        }
        (TypeCode::Null | TypeCode::Void, _) => Ok(()),
        (TypeCode::Boolean, Value::Bool(x)) => {
            e.put_bool(*x);
            Ok(())
        }
        (TypeCode::Octet, Value::Octet(x)) => {
            e.put_octet(*x);
            Ok(())
        }
        (TypeCode::Char, Value::Char(x)) => {
            e.put_char(*x);
            Ok(())
        }
        (TypeCode::Short, Value::Short(x)) => {
            e.put_i16(*x);
            Ok(())
        }
        (TypeCode::UShort, Value::UShort(x)) => {
            e.put_u16(*x);
            Ok(())
        }
        (TypeCode::Long, Value::Long(x)) => {
            e.put_i32(*x);
            Ok(())
        }
        (TypeCode::ULong, Value::ULong(x)) => {
            e.put_u32(*x);
            Ok(())
        }
        (TypeCode::LongLong, Value::LongLong(x)) => {
            e.put_i64(*x);
            Ok(())
        }
        (TypeCode::ULongLong, Value::ULongLong(x)) => {
            e.put_u64(*x);
            Ok(())
        }
        (TypeCode::Float, Value::Float(x)) => {
            e.put_f32(*x);
            Ok(())
        }
        (TypeCode::Double, Value::Double(x)) => {
            e.put_f64(*x);
            Ok(())
        }
        (TypeCode::LongDouble, Value::LongDouble(x)) => {
            e.put_long_double(*x);
            Ok(())
        }

        // A wchar's width and byte order belong to the negotiated codeset, not
        // to this layer; §7.10.2 makes it a property of the connection. Until
        // the dynamic path carries a codec, UTF-16 is what both fixtures agreed
        // on and what `WideCodec` emits by default.
        (TypeCode::WChar, Value::WChar(c)) => wide
            .put_wchar(e, *c)
            .map_err(|err| Error { path: p.render(), message: err.to_string() }),

        (TypeCode::String(bound), Value::String(s)) => {
            check_bound(p, *bound, s.chars().count(), "string")?;
            e.put_str(s);
            Ok(())
        }
        (TypeCode::WString(bound), Value::WString(s)) => {
            check_bound(p, *bound, s.encode_utf16().count(), "wstring")?;
            // Through the codec, never by hand: the length unit is
            // version-dependent and the BOM is what stops a peer reading our
            // units in the wrong order.
            wide.put_wstring(e, s)
                .map_err(|err| Error { path: p.render(), message: err.to_string() })
        }

        (TypeCode::Enum { members, name, .. }, Value::Enum(label)) => {
            match members.iter().position(|m| m == label) {
                Some(i) => {
                    e.put_u32(i as u32);
                    Ok(())
                }
                None => p.fail(format!(
                    "{label:?} is not an enumerator of {name}; it has {}",
                    members.join(", ")
                )),
            }
        }

        (
            TypeCode::Struct { members, name, .. } | TypeCode::Except { members, name, .. },
            Value::Struct(given),
        ) => {
            if given.len() != members.len() {
                return p.fail(format!(
                    "{name} has {} member(s), {} given",
                    members.len(),
                    given.len()
                ));
            }
            // Positional, and checked by name anyway. CDR carries no tags, so a
            // caller that supplies the right values in the wrong order produces
            // a message that decodes without complaint into the wrong fields —
            // the failure §5.3 measured against omniORB. Refusing here is the
            // only place it can still be caught.
            for (m, (gname, gval)) in members.iter().zip(given) {
                if *gname != m.name {
                    return p.fail(format!(
                        "member {} of {name} is {:?}, but {:?} was given; CDR is positional \
                         and carries no tags, so a reordered struct would encode silently",
                        members.iter().position(|x| x.name == m.name).unwrap_or(0),
                        m.name,
                        gname
                    ));
                }
                // `entering` before descending, so a `Recursive` marker
                // anywhere below can find this type again.
                let here = p.entering(resolved(tc));
                encode_at(e, &m.tc, gval, &here.member(&m.name), wide)?;
            }
            Ok(())
        }

        (
            TypeCode::Union { discriminator, cases, name, default_index, .. },
            Value::Union { discriminator: d, value },
        ) => {
            encode_at(e, discriminator, d, &p.member("_d"), wide)?;
            let case = select_case(discriminator, cases, *default_index, d, p, name, wide)?;
            let here = p.entering(resolved(tc));
            match (case, value) {
                (None, None) => Ok(()),
                (None, Some(_)) => p.fail(format!(
                    "the selected branch of {name} has no member, but a value was given"
                )),
                (Some(c), Some(val)) => encode_at(e, &c.tc, val, &here.member(&c.name), wide),
                (Some(c), None) => p.fail(format!("branch {:?} of {name} needs a value", c.name)),
            }
        }

        (TypeCode::Sequence { element, bound }, Value::List(items)) => {
            check_bound(p, *bound, items.len(), "sequence")?;
            e.put_u32(items.len() as u32);
            for (i, item) in items.iter().enumerate() {
                encode_at(e, element, item, &p.index(i), wide)?;
            }
            Ok(())
        }
        (TypeCode::Array { element, length }, Value::List(items)) => {
            if items.len() != *length as usize {
                return p.fail(format!("array has {length} element(s), {} given", items.len()));
            }
            // No length prefix: an array's length is in its type.
            for (i, item) in items.iter().enumerate() {
                encode_at(e, element, item, &p.index(i), wide)?;
            }
            Ok(())
        }

        (TypeCode::Any, Value::Any(inner_tc, inner)) => {
            orbweaver_giop::typecode::encode_any_with(e, inner_tc, |enc| {
                // The closure form exists because a value inside an `any` must
                // keep aligning against the outer stream; building it in a
                // detached buffer restarts alignment at zero and misplaces
                // every padding byte after the first.
                let _ = encode_at(enc, inner_tc, inner, &Path::root(), wide);
            })
            .map_err(|e| Error { path: p.render(), message: e.to_string() })?;
            // Re-run the inner encode's validation, which the closure swallowed
            // so that a failure cannot leave a half-written encapsulation.
            validate(inner_tc, inner, p, wide)
        }

        (TypeCode::ObjRef { .. }, Value::ObjRef(r)) => match r {
            Some(ior) => {
                ior.write_to(e).map_err(|err| Error { path: p.render(), message: err.to_string() })
            }
            None => {
                // A nil reference is an empty type id and no profiles, not an
                // absent field.
                e.put_str("");
                e.put_u32(0);
                Ok(())
            }
        },

        (TypeCode::TypeCode, Value::TypeCode(carried)) => {
            orbweaver_giop::typecode::encode(e, carried)
                .map_err(|err| Error { path: p.render(), message: err.to_string() })
        }

        // §4.4's two remaining deferrals, refused by name.
        //
        // They would already have been refused by `wrong_kind` below — there
        // is no `Value` variant to marshal a valuetype's state from — but the
        // sentence matters: until 2026-08-20 the registry recorded both as
        // `TypeCode::ObjRef`, this arm matched `(ObjRef, ObjRef)` and an IOR
        // went out where the peer sends a value. "Expected a value of type
        // Money, got a struct" would be a true sentence about the wrong
        // problem.
        (TypeCode::Value { .. }, _) => p.fail(format!(
            "{}: its state goes inline behind a value tag, and this path has no encoding for it",
            deferred_wire_head(&deferred_wire_name(tc).expect("a valuetype is deferred"))
        )),
        (TypeCode::AbstractInterface { .. }, _) => p.fail(format!(
            "{}: on the wire it is the union of a value and a reference, and this path has no \
             encoding for either form of it",
            deferred_wire_head(&deferred_wire_name(tc).expect("an abstract interface is deferred"))
        )),
        // The fourth, and the one §4.4 does not name — which is why it was
        // still `TypeCode::ObjRef` here when the other two were fixed, and why
        // this path marshalled an IOR for it. Not deferred: there is no
        // encoding to add later, which is what the tail says.
        //
        // Unlike the two arms above, the write direction does **not** keep a
        // tail of its own. A §4.4 write differs from a §4.4 read — the reader
        // is told the description still crosses, the writer is told this path
        // has no encoding — and for a native the two facts are the same fact,
        // because neither direction will ever be implemented.
        (t, _) if unmarshallable_wire_name(t).is_some() => {
            let what = unmarshallable_wire_name(t).expect("just matched");
            p.fail(unmarshallable_wire_sentence(&what))
        }

        (t, v) => wrong_kind(p, t, v),
    }
}

/// Type-checks without writing, for the `any` path where the writer cannot fail.
fn validate(tc: &TypeCode, v: &Value, p: &Path<'_>, wide: WideCodec) -> Result<()> {
    let mut probe = Encoder::new(orbweaver_cdr::Endian::Little);
    encode_at(&mut probe, tc, v, p, wide)
}

/// Type-checks `v` against `tc` as if it stood inside `open`, outermost first.
///
/// The public entry points start at the root, where a [`TypeCode::Recursive`]
/// marker has nothing to resolve against. [`dynany`] type-checks a value at a
/// *cursor*, which may be several levels inside the type the marker names, so
/// the enclosing types have to be handed back in — they are exactly what the
/// encoder would have been standing on had it walked there itself. Without
/// this, mutating one node of a recursive value would be refused for a reason
/// ("not inside the type it names") that is about the checker rather than
/// about the value.
pub(crate) fn check_within(tc: &TypeCode, v: &Value, open: &[&TypeCode]) -> Result<()> {
    fn go(tc: &TypeCode, v: &Value, open: &[&TypeCode], p: &Path<'_>) -> Result<()> {
        match open.split_first() {
            Some((first, rest)) => {
                let here = p.entering(first);
                go(tc, v, rest, &here)
            }
            None => validate(tc, v, p, default_codec()),
        }
    }
    go(tc, v, open, &Path::root())
}

fn check_bound(p: &Path<'_>, bound: u32, len: usize, what: &str) -> Result<()> {
    if bound > 0 && len > bound as usize {
        return p.fail(format!("{what} is bounded at {bound} but {len} were given"));
    }
    Ok(())
}

/// Finds the branch a discriminator selects, or the default.
/// Which branch a discriminator selects, for callers outside this module.
///
/// Exposed rather than reimplemented: the default-branch rule is subtle enough
/// that two copies would eventually disagree, and a union that JSON and CDR
/// disagree about is silent corruption of exactly the §5.3 kind.
pub(crate) fn select_case_public<'c>(
    disc_tc: &TypeCode,
    cases: &'c [UnionCase],
    default_index: i32,
    d: &Value,
    path: &str,
) -> Result<Option<&'c UnionCase>> {
    let root = Path::root();
    select_case(disc_tc, cases, default_index, d, &root, path, default_codec())
}

fn select_case<'c>(
    disc_tc: &TypeCode,
    cases: &'c [UnionCase],
    default_index: i32,
    d: &Value,
    p: &Path<'_>,
    name: &str,
    wide: WideCodec,
) -> Result<Option<&'c UnionCase>> {
    let mut probe = Encoder::new(orbweaver_cdr::Endian::Big);
    encode_at(&mut probe, disc_tc, d, &p.member("_d"), wide)?;
    let label = probe.finish().map_err(|e| Error { path: p.render(), message: e.to_string() })?;

    if let Some(c) = cases.iter().find(|c| c.label == label) {
        return Ok(Some(c));
    }
    if default_index >= 0 {
        return Ok(cases.get(default_index as usize));
    }
    // A union with no default and no matching case encodes the discriminator
    // and nothing else. That is legal, and it is also how a caller who mistyped
    // a label gets an empty message, so it is worth naming.
    p.fail(format!(
        "no branch of {name} matches the discriminator and it has no default; the value \
         would encode as a discriminator with no member"
    ))
}

fn decode_at(d: &mut Decoder<'_>, tc: &TypeCode, p: &Path<'_>, wide: WideCodec) -> Result<Value> {
    if let TypeCode::Alias { aliased, .. } = tc {
        let here = p.entering(tc);
        return decode_at(d, aliased, &here, wide);
    }
    Ok(match resolved(tc) {
        TypeCode::Recursive(id) => {
            let target = open_recursive(id, p)?;
            let entered = p.entering(target);
            return decode_at(d, target, &entered, wide);
        }
        TypeCode::Null | TypeCode::Void => Value::Struct(Vec::new()),
        TypeCode::Boolean => Value::Bool(cdr(p, d.get_bool())?),
        TypeCode::Octet => Value::Octet(cdr(p, d.get_u8())?),
        TypeCode::Char => Value::Char(cdr(p, d.get_u8())?),
        TypeCode::Short => Value::Short(cdr(p, d.get_i16())?),
        TypeCode::UShort => Value::UShort(cdr(p, d.get_u16())?),
        TypeCode::Long => Value::Long(cdr(p, d.get_i32())?),
        TypeCode::ULong => Value::ULong(cdr(p, d.get_u32())?),
        TypeCode::LongLong => Value::LongLong(cdr(p, d.get_i64())?),
        TypeCode::ULongLong => Value::ULongLong(cdr(p, d.get_u64())?),
        TypeCode::Float => Value::Float(cdr(p, d.get_f32())?),
        TypeCode::Double => Value::Double(cdr(p, d.get_f64())?),
        TypeCode::LongDouble => Value::LongDouble(cdr(p, d.get_long_double())?),
        // Through the codec, like the encoder: GIOP 1.2 prefixes a wchar with
        // an octet count and earlier versions do not, so reading a bare u16
        // here silently disagreed with what encode had just written.
        TypeCode::WChar => Value::WChar(
            wide.get_wchar(d)
                .map_err(|err| Error { path: p.render(), message: err.to_string() })?,
        ),
        TypeCode::String(_) => Value::String(cdr(p, d.get_string())?),
        TypeCode::WString(_) => Value::WString(
            wide.get_wstring(d)
                .map_err(|err| Error { path: p.render(), message: err.to_string() })?,
        ),
        TypeCode::Enum { members, name, .. } => {
            let ord = cdr(p, d.get_u32())? as usize;
            match members.get(ord) {
                Some(m) => Value::Enum(m.clone()),
                // §5.3 calls appending an enumerator conditionally breaking for
                // exactly this reason: the ordinal arrives intact and means
                // nothing here. Saying so beats reporting a generic CDR error.
                None => {
                    return p.fail(format!(
                        "ordinal {ord} is not an enumerator of {name}, which has {}; the \
                         sender may be built against a newer contract",
                        members.len()
                    ));
                }
            }
        }
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
            let here = p.entering(resolved(tc));
            let mut out = Vec::with_capacity(members.len());
            for m in members {
                out.push((m.name.clone(), decode_at(d, &m.tc, &here.member(&m.name), wide)?));
            }
            Value::Struct(out)
        }
        TypeCode::Union { discriminator, cases, default_index, name, .. } => {
            let disc = decode_at(d, discriminator, &p.member("_d"), wide)?;
            let case = select_case(discriminator, cases, *default_index, &disc, p, name, wide)?;
            let here = p.entering(resolved(tc));
            let value = match case {
                Some(c) => Some(Box::new(decode_at(d, &c.tc, &here.member(&c.name), wide)?)),
                None => None,
            };
            Value::Union { discriminator: Box::new(disc), value }
        }
        TypeCode::Sequence { element, bound } => {
            let n = cdr(p, d.get_u32())?;
            check_bound(p, *bound, n as usize, "sequence")?;
            // validate_count refuses a length the remaining buffer cannot hold,
            // which is what stops twelve bytes buying a multi-gigabyte
            // allocation — the worst finding of the Phase 0 spec audit.
            let n = cdr(p, d.validate_count(n, min_width(element)))?;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                out.push(decode_at(d, element, &p.index(i), wide)?);
            }
            Value::List(out)
        }
        TypeCode::Array { element, length } => {
            // The same guard the `Sequence` arm above carries, for the same
            // reason and against a length that used to be beyond a peer's
            // reach. An array's length comes from its TypeCode, and until
            // AnyJSON v1.1 (D008) every TypeCode we decoded against had been
            // compiled here — so the number was ours. It is now a field in a
            // document an agent sends: `agent-fuzz` reached this from a
            // **198-byte** document declaring `array<octet, 4294967295>` as a
            // union discriminator, reserving 206 GB before reading a byte.
            // Lazily backed on macOS/arm64, which is why nothing fell over;
            // uncatchably fatal under a memory limit or on a 32-bit target.
            let n = cdr(p, d.validate_count(*length, min_width(element)))?;
            let mut out = Vec::with_capacity(n);
            for i in 0..*length as usize {
                out.push(decode_at(d, element, &p.index(i), wide)?);
            }
            Value::List(out)
        }
        TypeCode::Any => {
            let inner_tc = orbweaver_giop::typecode::decode(d)
                .map_err(|e| Error { path: p.render(), message: e.to_string() })?;
            let inner = decode_at(d, &inner_tc, p, wide)?;
            Value::Any(Box::new(inner_tc), Box::new(inner))
        }
        TypeCode::ObjRef { .. } => {
            let ior = Ior::read_from(d)
                .map_err(|e| Error { path: p.render(), message: e.to_string() })?;
            if ior.type_id.is_empty() && ior.profiles.is_empty() {
                Value::ObjRef(None)
            } else {
                Value::ObjRef(Some(ior))
            }
        }
        TypeCode::TypeCode => {
            let carried = orbweaver_giop::typecode::decode(d)
                .map_err(|e| Error { path: p.render(), message: e.to_string() })?;
            Value::TypeCode(Box::new(carried))
        }
        // The same two, on the way in. A peer that sends us a value is a peer
        // we have to answer honestly rather than by reading an IOR out of its
        // value tag.
        TypeCode::Value { .. } | TypeCode::AbstractInterface { .. } => {
            let what = deferred_wire_name(tc).expect("§4.4 defers both of these");
            return p.fail(deferred_wire_sentence(&what));
        }
        // "yet" is the word this arm must not use: a native is not waiting on
        // an implementation. Refused by name rather than through `describe`,
        // and from the same source the other four layers read.
        other if unmarshallable_wire_name(other).is_some() => {
            let what = unmarshallable_wire_name(other).expect("just matched");
            return p.fail(unmarshallable_wire_sentence(&what));
        }
        other => return p.fail(format!("cannot decode {} yet", describe(other))),
    })
}

/// The smallest number of octets one element of `tc` can occupy.
///
/// Used only to bound a sequence length against the bytes actually available,
/// so it must never over-estimate: a type whose minimum is unclear counts as
/// one octet, which rejects less and never rejects wrongly.
fn min_width(tc: &TypeCode) -> usize {
    match resolved(tc) {
        TypeCode::Short | TypeCode::UShort | TypeCode::WChar => 2,
        TypeCode::Long | TypeCode::ULong | TypeCode::Float | TypeCode::Enum { .. } => 4,
        TypeCode::LongLong | TypeCode::ULongLong | TypeCode::Double => 8,
        TypeCode::LongDouble => 16,
        // A struct is at least the sum of its members, but an empty struct is
        // zero octets and a sequence of them has no floor at all.
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
            members.iter().map(|m| min_width(&m.tc)).sum::<usize>().max(1)
        }
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_cdr::Endian;
    use orbweaver_giop::typecode::Member;

    fn tc_struct(name: &str, members: Vec<(&str, TypeCode)>) -> TypeCode {
        TypeCode::Struct {
            id: format!("IDL:m/{name}:1.0"),
            name: name.into(),
            members: members.into_iter().map(|(n, tc)| Member { name: n.into(), tc }).collect(),
        }
    }

    /// §4.4's two remaining deferrals are refused, in both directions, with the
    /// section named — and a member of a struct carries the refusal out with it.
    ///
    /// The negative control is what this test is for. Until 2026-08-20 the
    /// registry recorded a `valuetype` as `TypeCode::ObjRef`, the encoder's
    /// `(ObjRef, ObjRef)` arm matched, and an IOR went out where a conformant
    /// peer sends a value inline behind a value tag. Nothing was red: an IOR
    /// is a perfectly good thing to marshal, and both ends of every test we
    /// had were reading the same wrong type.
    #[test]
    fn a_valuetype_and_an_abstract_interface_are_refused_naming_the_section() {
        let money = TypeCode::Value {
            id: "IDL:m/Money:1.0".into(),
            name: "Money".into(),
            modifier: 0,
            base: None,
            members: vec![orbweaver_giop::typecode::ValueMember {
                name: "units".into(),
                tc: TypeCode::Long,
                visibility: 1,
            }],
        };
        let describable =
            TypeCode::AbstractInterface { id: "IDL:m/D:1.0".into(), name: "D".into() };

        for (tc, what) in [(&money, "valuetype Money"), (&describable, "abstract interface D")] {
            for endian in [Endian::Big, Endian::Little] {
                // Every `Value` shape a caller might reach for, including the
                // one the old code accepted.
                for v in [
                    Value::ObjRef(None),
                    Value::Struct(vec![("units".into(), Value::Long(1))]),
                    Value::Long(1),
                ] {
                    let err = encode(&mut Encoder::new(endian), tc, &v)
                        .expect_err("{what} must not marshal");
                    assert!(err.message.contains(what), "{err}");
                    assert!(err.message.contains("§4.4"), "{err}");
                }
                let err = decode(&mut Decoder::new(&[0u8; 16], endian), tc)
                    .expect_err("{what} must not decode");
                assert!(err.message.contains(what), "{err}");
                assert!(err.message.contains("§4.4"), "{err}");
            }
        }

        // And the refusal travels: a struct holding one is refused at the
        // member, with the member's path in front of the reason, rather than
        // written as far as the member and then failed.
        let holder = tc_struct("Holder", vec![("body", money.clone())]);
        let err = encode(
            &mut Encoder::new(Endian::Big),
            &holder,
            &Value::Struct(vec![("body".into(), Value::ObjRef(None))]),
        )
        .expect_err("a struct holding a valuetype must not marshal");
        assert_eq!(err.path, "body", "{err}");
        assert!(err.message.contains("§4.4"), "{err}");
    }

    /// The same, for a `native` — with the sentence that is *not* the same.
    ///
    /// It was `TypeCode::ObjRef` here until 2026-08-21, so `Value::ObjRef(None)`
    /// marshalled and an IOR went out for a type with no wire form. This test
    /// exists because removing the refusal arm left the whole crate green: the
    /// valuetype pair had a test and the native did not, which is the same
    /// asymmetry that let the defect through in the first place.
    ///
    /// The assertion is deliberately *not* only "§4.4": the message must say
    /// the section does not apply. A native is not deferred — there is no wire
    /// form waiting to be implemented — and a refusal that filed it under §4.4
    /// would promise a later release that cannot come.
    ///
    /// The exact wording lives in [`unmarshallable_wire_sentence`] and is held
    /// to every other layer's by `tests/deferred_sentence_agreement.rs`. What
    /// is asserted here is what that file cannot assert: that this arm exists
    /// at all, for every `Value` shape and both byte orders. It is built from
    /// the helper rather than re-typed, so a wording change lands in one place
    /// and this test still fails if the arm is deleted.
    #[test]
    fn a_native_is_refused_and_the_refusal_says_it_is_not_a_deferral() {
        let handle = TypeCode::Native { id: "IDL:m/Handle:1.0".into(), name: "Handle".into() };
        let want = unmarshallable_wire_sentence("native Handle");
        assert!(!want.contains("yet"), "a native is not waiting on an implementation: {want}");
        assert!(
            !want.contains(&deferred_wire_head("native Handle")),
            "a native must not carry §4.4's deferral claim: {want}"
        );
        for endian in [Endian::Big, Endian::Little] {
            // Every `Value` shape a caller might reach for, including the one
            // the old `ObjRef` recording accepted.
            for v in [
                Value::ObjRef(None),
                Value::Struct(vec![("token".into(), Value::Long(1))]),
                Value::Long(1),
            ] {
                let err = encode(&mut Encoder::new(endian), &handle, &v)
                    .expect_err("a native must not marshal");
                assert_eq!(err.message, want, "{err}");
            }
            let err = decode(&mut Decoder::new(&[0u8; 16], endian), &handle)
                .expect_err("a native must not decode");
            assert_eq!(err.message, want, "{err}");
        }

        // And the refusal travels to the member, as the valuetype's does.
        let session = tc_struct("Session", vec![("token", handle.clone())]);
        let err = encode(
            &mut Encoder::new(Endian::Big),
            &session,
            &Value::Struct(vec![("token".into(), Value::ObjRef(None))]),
        )
        .expect_err("a struct holding a native must not marshal");
        assert_eq!(err.path, "token", "{err}");
        assert!(err.message.contains("native Handle"), "{err}");
    }

    /// Both byte orders, every time. An encoder that only works native-endian
    /// passes every local test and fails in the field.
    fn round_trip(tc: &TypeCode, v: &Value) {
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            encode(&mut e, tc, v).unwrap_or_else(|err| panic!("{endian:?} encode: {err}"));
            let bytes = e.finish().expect("finish");
            let mut d = Decoder::new(&bytes, endian);
            let back = decode(&mut d, tc).unwrap_or_else(|err| panic!("{endian:?} decode: {err}"));
            assert_eq!(&back, v, "{endian:?} round trip");
        }
    }

    #[test]
    fn every_primitive_round_trips_in_both_byte_orders() {
        for (tc, v) in [
            (TypeCode::Boolean, Value::Bool(true)),
            (TypeCode::Octet, Value::Octet(0xA5)),
            (TypeCode::Char, Value::Char(b'z')),
            (TypeCode::Short, Value::Short(-30_000)),
            (TypeCode::UShort, Value::UShort(65_535)),
            (TypeCode::Long, Value::Long(-2_000_000_000)),
            (TypeCode::ULong, Value::ULong(4_000_000_000)),
            (TypeCode::LongLong, Value::LongLong(-9_000_000_000_000_000_000)),
            (TypeCode::ULongLong, Value::ULongLong(18_000_000_000_000_000_000)),
            (TypeCode::Float, Value::Float(-0.5)),
            (TypeCode::Double, Value::Double(1.0 / 3.0)),
            (TypeCode::LongDouble, Value::LongDouble([7u8; 16])),
            (TypeCode::String(0), Value::String("hello".into())),
            (TypeCode::WChar, Value::WChar('한')),
            (TypeCode::WString(0), Value::WString("안녕하세요".into())),
        ] {
            round_trip(&tc, &v);
        }
    }

    /// The alignment case that has bitten this project repeatedly: members of
    /// different widths force padding, and the padding has to land in the same
    /// place going out as coming back.
    #[test]
    fn a_ragged_struct_round_trips() {
        let tc = tc_struct(
            "Ragged",
            vec![
                ("a", TypeCode::Octet),
                ("b", TypeCode::Long),
                ("c", TypeCode::Short),
                ("d", TypeCode::Double),
                ("e", TypeCode::Octet),
            ],
        );
        round_trip(
            &tc,
            &Value::Struct(vec![
                ("a".into(), Value::Octet(0xAA)),
                ("b".into(), Value::Long(-7)),
                ("c".into(), Value::Short(9)),
                ("d".into(), Value::Double(2.5)),
                ("e".into(), Value::Octet(0xBB)),
            ]),
        );
    }

    #[test]
    fn nested_sequences_and_arrays_round_trip() {
        let inner = TypeCode::Sequence { element: Box::new(TypeCode::Long), bound: 0 };
        let tc = TypeCode::Array { element: Box::new(inner), length: 3 };
        round_trip(
            &tc,
            &Value::List(vec![
                Value::List(vec![Value::Long(1)]),
                Value::List(vec![]),
                Value::List(vec![Value::Long(2), Value::Long(3)]),
            ]),
        );
    }

    #[test]
    fn an_any_inside_a_struct_keeps_the_outer_alignment() {
        // The regression this guards: a value built for an `any` in a detached
        // buffer restarts alignment at zero, so a double inside it lands on the
        // wrong boundary. The leading octet makes any such slip visible.
        let tc = tc_struct("Holder", vec![("pad", TypeCode::Octet), ("v", TypeCode::Any)]);
        round_trip(
            &tc,
            &Value::Struct(vec![
                ("pad".into(), Value::Octet(1)),
                ("v".into(), Value::Any(Box::new(TypeCode::Double), Box::new(Value::Double(2.5)))),
            ]),
        );
    }

    fn union_tc() -> TypeCode {
        TypeCode::Union {
            id: "IDL:m/U:1.0".into(),
            name: "U".into(),
            discriminator: Box::new(TypeCode::Long),
            default_index: 1,
            cases: vec![
                UnionCase {
                    label: 1i32.to_be_bytes().to_vec(),
                    name: "a".into(),
                    tc: TypeCode::Long,
                },
                UnionCase {
                    label: 2i32.to_be_bytes().to_vec(),
                    name: "b".into(),
                    tc: TypeCode::String(0),
                },
            ],
        }
    }

    #[test]
    fn unions_round_trip_on_both_the_named_branch_and_the_default() {
        round_trip(
            &union_tc(),
            &Value::Union {
                discriminator: Box::new(Value::Long(1)),
                value: Some(Box::new(Value::Long(42))),
            },
        );
        // Discriminator 99 matches no case, so default_index selects branch b.
        round_trip(
            &union_tc(),
            &Value::Union {
                discriminator: Box::new(Value::Long(99)),
                value: Some(Box::new(Value::String("default".into()))),
            },
        );
    }

    #[test]
    fn enums_travel_as_ordinals_but_are_held_by_name() {
        let tc = TypeCode::Enum {
            id: "IDL:m/E:1.0".into(),
            name: "E".into(),
            members: vec!["RED".into(), "GREEN".into(), "BLUE".into()],
        };
        round_trip(&tc, &Value::Enum("BLUE".into()));

        let mut e = Encoder::new(Endian::Big);
        encode(&mut e, &tc, &Value::Enum("GREEN".into())).unwrap();
        assert_eq!(e.finish().unwrap(), vec![0, 0, 0, 1], "the ordinal is what travels");
    }

    /// An ordinal from a newer contract has to say so, because §5.3 predicts
    /// exactly this and a generic CDR error would hide the prediction coming
    /// true.
    #[test]
    fn an_unknown_enumerator_names_the_likely_cause() {
        let tc = TypeCode::Enum {
            id: "IDL:m/E:1.0".into(),
            name: "E".into(),
            members: vec!["RED".into(), "GREEN".into()],
        };
        let bytes = vec![0, 0, 0, 7];
        let err = decode(&mut Decoder::new(&bytes, Endian::Big), &tc).unwrap_err();
        assert!(err.message.contains("newer contract"), "{err}");
    }

    /// The whole point of the type check: CDR is positional, so values supplied
    /// in the wrong order would otherwise encode cleanly and arrive wrong.
    #[test]
    fn members_given_out_of_order_are_refused_not_encoded() {
        let tc = tc_struct("P", vec![("px", TypeCode::Long), ("py", TypeCode::Long)]);
        let err = encode(
            &mut Encoder::new(Endian::Big),
            &tc,
            &Value::Struct(vec![("py".into(), Value::Long(22)), ("px".into(), Value::Long(11))]),
        )
        .unwrap_err();
        assert!(err.message.contains("positional"), "{err}");
    }

    #[test]
    fn a_diagnostic_names_the_path_to_the_offending_value() {
        let line = tc_struct("Line", vec![("qty", TypeCode::Long)]);
        let order = tc_struct(
            "Order",
            vec![("lines", TypeCode::Sequence { element: Box::new(line), bound: 0 })],
        );
        let err = encode(
            &mut Encoder::new(Endian::Big),
            &order,
            &Value::Struct(vec![(
                "lines".into(),
                Value::List(vec![
                    Value::Struct(vec![("qty".into(), Value::Long(1))]),
                    Value::Struct(vec![("qty".into(), Value::String("two".into()))]),
                ]),
            )]),
        )
        .unwrap_err();
        assert_eq!(err.path, "lines[1].qty", "{err}");
        assert!(err.message.contains("got a string"), "{err}");
    }

    #[test]
    fn bounds_are_enforced_in_both_directions() {
        let tc = TypeCode::String(4);
        let err = encode(&mut Encoder::new(Endian::Big), &tc, &Value::String("toolong".into()))
            .unwrap_err();
        assert!(err.message.contains("bounded at 4"), "{err}");

        let seq = TypeCode::Sequence { element: Box::new(TypeCode::Octet), bound: 2 };
        let err = encode(
            &mut Encoder::new(Endian::Big),
            &seq,
            &Value::List(vec![Value::Octet(1), Value::Octet(2), Value::Octet(3)]),
        )
        .unwrap_err();
        assert!(err.message.contains("bounded at 2"), "{err}");
    }

    /// Twelve bytes must not buy a multi-gigabyte allocation. This was the
    /// worst finding of the Phase 0 spec audit and it must not come back
    /// through a new marshaller.
    #[test]
    fn a_huge_declared_sequence_length_is_refused_not_allocated() {
        let tc = TypeCode::Sequence { element: Box::new(TypeCode::Double), bound: 0 };
        let bytes = vec![0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0];
        let err = decode(&mut Decoder::new(&bytes, Endian::Big), &tc).unwrap_err();
        assert!(!err.message.is_empty(), "a 4-billion-element claim must be refused");
    }

    #[test]
    fn a_typedef_changes_the_name_and_not_the_bytes() {
        let alias = TypeCode::Alias {
            id: "IDL:m/Meters:1.0".into(),
            name: "Meters".into(),
            aliased: Box::new(TypeCode::Long),
        };
        round_trip(&alias, &Value::Long(1234));

        let mut a = Encoder::new(Endian::Big);
        encode(&mut a, &alias, &Value::Long(1234)).unwrap();
        let mut b = Encoder::new(Endian::Big);
        encode(&mut b, &TypeCode::Long, &Value::Long(1234)).unwrap();
        assert_eq!(a.finish().unwrap(), b.finish().unwrap());
    }

    #[test]
    fn a_nil_object_reference_round_trips_as_nil() {
        let tc = TypeCode::ObjRef { id: "IDL:m/I:1.0".into(), name: "I".into() };
        round_trip(&tc, &Value::ObjRef(None));
    }

    /// `struct Tree { string label; sequence<Tree> kids; }`, the shape
    /// `corpus/golden/15` has carried since Phase 1.
    fn tree() -> TypeCode {
        TypeCode::Struct {
            id: "IDL:gc15/Tree:1.0".into(),
            name: "Tree".into(),
            members: vec![
                Member { name: "label".into(), tc: TypeCode::String(0) },
                Member {
                    name: "kids".into(),
                    tc: TypeCode::Sequence {
                        element: Box::new(TypeCode::Recursive("IDL:gc15/Tree:1.0".into())),
                        bound: 0,
                    },
                },
            ],
        }
    }

    fn node(label: &str, kids: Vec<Value>) -> Value {
        Value::Struct(vec![
            ("label".into(), Value::String(label.into())),
            ("kids".into(), Value::List(kids)),
        ])
    }

    /// The gap this arm was written for. Until the marker could be resolved,
    /// every non-empty recursive value was refused with "expected a value of
    /// type an indirection" — and nothing noticed, because the generator that
    /// would have produced one could only produce the empty case.
    #[test]
    fn a_recursive_struct_round_trips_at_depth() {
        round_trip(&tree(), &node("root", vec![]));
        round_trip(&tree(), &node("root", vec![node("a", vec![]), node("b", vec![])]));
        round_trip(
            &tree(),
            &node("root", vec![node("a", vec![node("a1", vec![node("a11", vec![])])])]),
        );
    }

    /// The cycle can name the typedef rather than the struct, and that spelling
    /// is what `corpus/golden/15` actually produces when `TreeSeq` is the type
    /// being marshalled.
    #[test]
    fn a_cycle_through_a_typedef_resolves_to_the_alias() {
        let tc = TypeCode::Alias {
            id: "IDL:gc15/TreeSeq:1.0".into(),
            name: "TreeSeq".into(),
            aliased: Box::new(TypeCode::Sequence {
                element: Box::new(TypeCode::Struct {
                    id: "IDL:gc15/Tree:1.0".into(),
                    name: "Tree".into(),
                    members: vec![
                        Member { name: "label".into(), tc: TypeCode::String(0) },
                        Member {
                            name: "kids".into(),
                            tc: TypeCode::Recursive("IDL:gc15/TreeSeq:1.0".into()),
                        },
                    ],
                }),
                bound: 0,
            }),
        };
        let leaf = Value::Struct(vec![
            ("label".into(), Value::String("leaf".into())),
            ("kids".into(), Value::List(vec![])),
        ]);
        let branch = Value::Struct(vec![
            ("label".into(), Value::String("branch".into())),
            ("kids".into(), Value::List(vec![leaf.clone()])),
        ]);
        round_trip(&tc, &Value::List(vec![branch, leaf]));
    }

    /// A marker outside the type it names is a diagnosable mistake, not a
    /// panic and not a silent empty value.
    #[test]
    fn an_unresolvable_marker_says_which_id_it_could_not_find() {
        let tc = TypeCode::Recursive("IDL:gc15/Tree:1.0".into());
        let mut e = Encoder::new(Endian::Big);
        let err = encode(&mut e, &tc, &Value::String("x".into())).expect_err("must refuse");
        assert!(err.message.contains("IDL:gc15/Tree:1.0"), "{err}");
        assert!(err.message.contains("cannot be resolved"), "{err}");
    }

    /// Depth on decode is chosen by the sender, so the bound is a wire-safety
    /// property: a message that nests past it is refused with a message, not
    /// followed until the stack ends.
    #[test]
    fn nesting_past_the_bound_is_refused_rather_than_followed() {
        let tc = tree();
        let mut deep = node("leaf", vec![]);
        for _ in 0..MAX_NESTING + 4 {
            deep = node("x", vec![deep]);
        }
        let mut e = Encoder::new(Endian::Big);
        let err = encode(&mut e, &tc, &deep).expect_err("must refuse");
        assert!(err.message.contains(&MAX_NESTING.to_string()), "{err}");

        // The decode half is the one a peer controls, so it is measured
        // against a stream our own encoder would refuse to produce: a Tree is
        // a string then a sequence count, so nesting is just that pair
        // repeated. Hand-building it is the only way to ask the decoder the
        // question an attacker would.
        let mut hostile = Encoder::new(Endian::Big);
        for _ in 0..MAX_NESTING + 4 {
            hostile.put_string_bytes(b"x");
            hostile.put_u32(1); // one child, one level deeper
        }
        hostile.put_string_bytes(b"leaf");
        hostile.put_u32(0);
        let bytes = hostile.finish().expect("finish");
        let mut d = Decoder::new(&bytes, Endian::Big);
        let err = decode(&mut d, &tc).expect_err("the decoder must refuse it too");
        assert!(err.message.contains(&MAX_NESTING.to_string()), "{err}");

        // And a stream inside the bound still decodes, so the refusal is the
        // depth and not the shape.
        let mut ok = Encoder::new(Endian::Big);
        for _ in 0..8 {
            ok.put_string_bytes(b"x");
            ok.put_u32(1);
        }
        ok.put_string_bytes(b"leaf");
        ok.put_u32(0);
        let bytes = ok.finish().expect("finish");
        let mut d = Decoder::new(&bytes, Endian::Big);
        decode(&mut d, &tc).expect("eight levels is inside the bound");
    }
}
