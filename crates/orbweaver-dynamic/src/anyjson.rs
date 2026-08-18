//! AnyJSON v1 — the normative JSON ↔ CDR mapping of `docs/PLAN.md` §4.5.
//!
//! An agent speaks JSON and a CORBA target speaks CDR. Something has to sit
//! between them, and if that something is approximate the result is not an
//! error but *wrong data delivered confidently* — a 64-bit account number
//! rounded, an octet sequence mangled by a text codec, a union whose active
//! branch was inferred. §4.5 is therefore a specification rather than a
//! convention, and this module implements it.
//!
//! # The rules that are not obvious
//!
//! - **64-bit integers cross as strings.** A JSON number is a `double` in
//!   every mainstream implementation, so anything past 2^53 loses digits
//!   silently. A string does not.
//! - **`octet` sequences cross as base64**, not as arrays of numbers: a
//!   megabyte of binary becomes an array of a million JSON numbers otherwise,
//!   and no amount of care makes that acceptable.
//! - **Enumerators cross by name.** The ordinal is a wire detail; §5.3 measured
//!   what happens when meaning is attached to it.
//! - **A union carries its discriminator explicitly** as `_d`, because the
//!   active branch is a fact about the value and not something to infer from
//!   which member happens to be present.
//! - **NaN and the infinities have no JSON encoding**, so they cross as
//!   `{"_f": "nan" | "+inf" | "-inf"}`. Writing them as `null` would make a
//!   missing value and a NaN indistinguishable.
//! - **An object reference crosses as a handle**, never as a raw IOR. §4.7 and
//!   §4.8: an IOR is a bearer address, and handing one to an agent hands it a
//!   credential. The mapping physically cannot emit one.
//!
//! # Round-tripping is the acceptance criterion
//!
//! For any value, `CDR → JSON → CDR` must reproduce identical bytes (§8). That
//! is what these tests check, over every constructed type the corpus exercises.

use std::collections::BTreeMap;

use orbweaver_giop::typecode::{Member, TypeCode, UnionCase};

use crate::json::Json;
use crate::{Error, Result, Value};

/// Where an object reference is parked while its name crosses the boundary.
///
/// §4.7: an IOR is a bearer address. Anything holding one can dial the target
/// directly, bypassing authorisation, approval and the audit log, so the
/// mapping must be *incapable* of emitting one. It emits a name instead, and
/// this is what turns a name back into an address.
///
/// A trait rather than a type, because the real implementation belongs at the
/// MCP boundary where sessions and expiry live (`orbweaver-mcp`), and this
/// crate sits below it. What the mapping needs is only these two operations.
pub trait References {
    /// Issues a handle naming `ior`, or returns the one it already has.
    fn issue(&mut self, ior: &orbweaver_giop::Ior) -> String;

    /// The reference a handle names, if this table issued it and it is still
    /// valid. Returning `None` is the whole point: a handle nobody issued
    /// cannot be turned into an address by guessing.
    fn resolve(&self, handle: &str) -> Option<orbweaver_giop::Ior>;
}

/// A reference table with no session and no expiry, for tests and for the
/// static path where the caller is already inside the trust boundary.
///
/// Not for the agent boundary: the handles are sequential, so one is guessable
/// from another. `orbweaver_mcp::CapabilityTable` is the one to use there, and
/// it is a different type precisely so this one cannot be reached for by
/// accident.
#[derive(Debug, Default)]
pub struct LocalReferences {
    by_handle: BTreeMap<String, orbweaver_giop::Ior>,
    next: u64,
}

impl LocalReferences {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many references are outstanding.
    pub fn len(&self) -> usize {
        self.by_handle.len()
    }

    /// Whether nothing has been issued.
    pub fn is_empty(&self) -> bool {
        self.by_handle.is_empty()
    }
}

impl References for LocalReferences {
    fn issue(&mut self, ior: &orbweaver_giop::Ior) -> String {
        if let Some((h, _)) = self.by_handle.iter().find(|(_, v)| *v == ior) {
            return h.clone();
        }
        self.next += 1;
        let handle = format!("local-{}", self.next);
        self.by_handle.insert(handle.clone(), ior.clone());
        handle
    }

    fn resolve(&self, handle: &str) -> Option<orbweaver_giop::Ior> {
        self.by_handle.get(handle).cloned()
    }
}

fn fail<T>(path: &str, message: impl Into<String>) -> Result<T> {
    Err(Error { path: path.to_owned(), message: message.into() })
}

fn member(path: &str, name: &str) -> String {
    if path.is_empty() { name.to_owned() } else { format!("{path}.{name}") }
}

fn index(path: &str, i: usize) -> String {
    format!("{path}[{i}]")
}

/// Converts a CDR value to its AnyJSON form.
pub fn to_json(tc: &TypeCode, v: &Value, handles: &mut dyn References) -> Result<Json> {
    to_json_at(tc, v, handles, "")
}

/// Converts an AnyJSON document to the CDR value `tc` describes.
pub fn from_json(tc: &TypeCode, j: &Json, handles: &dyn References) -> Result<Value> {
    from_json_at(tc, j, handles, "")
}

fn resolved(tc: &TypeCode) -> &TypeCode {
    let mut t = tc;
    while let TypeCode::Alias { aliased, .. } = t {
        t = aliased;
    }
    t
}

/// Whether a sequence of this element type crosses as base64.
fn is_binary(tc: &TypeCode) -> bool {
    matches!(resolved(tc), TypeCode::Octet)
}

fn number(n: impl std::fmt::Display) -> Json {
    Json::Number(n.to_string())
}

/// Floats that JSON cannot spell.
fn float_json(x: f64) -> Json {
    if x.is_nan() {
        return special_float("nan");
    }
    if x.is_infinite() {
        return special_float(if x > 0.0 { "+inf" } else { "-inf" });
    }
    // `{}` on an f64 prints `2` for 2.0, which re-reads as an integer. That is
    // harmless here because the TypeCode decides the type on the way back, and
    // it keeps the document readable.
    Json::Number(format!("{x:?}"))
}

fn special_float(tag: &str) -> Json {
    Json::Object(BTreeMap::from([("_f".to_owned(), Json::String(tag.to_owned()))]))
}

fn to_json_at(tc: &TypeCode, v: &Value, h: &mut dyn References, p: &str) -> Result<Json> {
    Ok(match (resolved(tc), v) {
        (TypeCode::Boolean, Value::Bool(x)) => Json::Bool(*x),
        (TypeCode::Octet, Value::Octet(x)) => number(x),
        (TypeCode::Short, Value::Short(x)) => number(x),
        (TypeCode::UShort, Value::UShort(x)) => number(x),
        (TypeCode::Long, Value::Long(x)) => number(x),
        (TypeCode::ULong, Value::ULong(x)) => number(x),

        // The precision rule. 2^53 is where a JSON number stops being able to
        // hold an integer exactly, and every 64-bit type can exceed it.
        (TypeCode::LongLong, Value::LongLong(x)) => Json::String(x.to_string()),
        (TypeCode::ULongLong, Value::ULongLong(x)) => Json::String(x.to_string()),

        (TypeCode::Float, Value::Float(x)) => float_json(f64::from(*x)),
        (TypeCode::Double, Value::Double(x)) => float_json(*x),
        (TypeCode::LongDouble, Value::LongDouble(b)) => Json::String(base64(b)),

        // A char is one octet of the negotiated codeset, not a Unicode scalar;
        // sending it as a JSON string would claim a text meaning the wire does
        // not give it.
        (TypeCode::Char, Value::Char(x)) => number(x),
        (TypeCode::WChar, Value::WChar(c)) => Json::String(c.to_string()),
        (TypeCode::String(_), Value::String(s)) | (TypeCode::WString(_), Value::WString(s)) => {
            Json::String(s.clone())
        }

        (TypeCode::Enum { .. }, Value::Enum(name)) => Json::String(name.clone()),

        (
            TypeCode::Struct { members, .. } | TypeCode::Except { members, .. },
            Value::Struct(given),
        ) => {
            let mut out = BTreeMap::new();
            for (m, (name, val)) in members.iter().zip(given) {
                out.insert(m.name.clone(), to_json_at(&m.tc, val, h, &member(p, name))?);
            }
            Json::Object(out)
        }

        (
            TypeCode::Union { discriminator, cases, default_index, .. },
            Value::Union { discriminator: d, value },
        ) => {
            let mut out = BTreeMap::new();
            out.insert("_d".to_owned(), to_json_at(discriminator, d, h, &member(p, "_d"))?);
            if let Some(val) = value {
                let case = crate::select_case_public(discriminator, cases, *default_index, d, p)?;
                let tc = case.map(|c| &c.tc).ok_or_else(|| Error {
                    path: p.to_owned(),
                    message: "a union with a value but no selected branch".into(),
                })?;
                out.insert("_v".to_owned(), to_json_at(tc, val, h, &member(p, "_v"))?);
            }
            Json::Object(out)
        }

        (TypeCode::Sequence { element, .. }, Value::List(items)) if is_binary(element) => {
            let bytes: Vec<u8> = items
                .iter()
                .map(|x| match x {
                    Value::Octet(b) => Ok(*b),
                    other => fail(p, format!("expected an octet, got {other:?}")),
                })
                .collect::<Result<_>>()?;
            Json::String(base64(&bytes))
        }
        (
            TypeCode::Sequence { element, .. } | TypeCode::Array { element, .. },
            Value::List(items),
        ) => Json::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, x)| to_json_at(element, x, h, &index(p, i)))
                .collect::<Result<_>>()?,
        ),

        (TypeCode::Any, Value::Any(inner_tc, inner)) => Json::Object(BTreeMap::from([
            ("_t".to_owned(), tc_to_json(inner_tc)),
            ("_v".to_owned(), to_json_at(inner_tc, inner, h, &member(p, "_v"))?),
        ])),

        // A TypeCode standing on its own. The same structural form as `_t`,
        // in the value position: what `describe()` returns and what every
        // Interface Repository description is made of.
        (TypeCode::TypeCode, Value::TypeCode(carried)) => tc_to_json(carried),

        // Never an IOR. §4.8's confused deputy starts with a bearer address
        // reaching something that should not have had one.
        (TypeCode::ObjRef { id, .. }, Value::ObjRef(r)) => match r {
            None => Json::Object(BTreeMap::from([("_ref".to_owned(), Json::Null)])),
            Some(ior) => Json::Object(BTreeMap::from([
                ("_ref".to_owned(), Json::String(h.issue(ior))),
                (
                    "_type".to_owned(),
                    Json::String(if ior.type_id.is_empty() {
                        id.clone()
                    } else {
                        ior.type_id.clone()
                    }),
                ),
            ])),
        },

        (t, v) => return fail(p, format!("{v:?} is not a value of {}", type_name(t))),
    })
}

fn from_json_at(tc: &TypeCode, j: &Json, h: &dyn References, p: &str) -> Result<Value> {
    let t = resolved(tc);
    Ok(match t {
        TypeCode::Boolean => match j {
            Json::Bool(b) => Value::Bool(*b),
            other => return wrong(p, "a boolean", other),
        },
        TypeCode::Octet => Value::Octet(int(j, p, "an octet")?),
        TypeCode::Char => Value::Char(int(j, p, "a char")?),
        TypeCode::Short => Value::Short(int(j, p, "a short")?),
        TypeCode::UShort => Value::UShort(int(j, p, "an unsigned short")?),
        TypeCode::Long => Value::Long(int(j, p, "a long")?),
        TypeCode::ULong => Value::ULong(int(j, p, "an unsigned long")?),

        // Accepts a string, which is what the mapping emits, and also a number,
        // because an agent that has not read the spec will send one. Accepting
        // it is safe only when it survives the trip exactly; otherwise saying so
        // beats delivering a rounded account number.
        TypeCode::LongLong => Value::LongLong(wide_int(j, p, "a long long")?),
        TypeCode::ULongLong => Value::ULongLong(wide_int(j, p, "an unsigned long long")?),

        TypeCode::Float => Value::Float(float(j, p)? as f32),
        TypeCode::Double => Value::Double(float(j, p)?),
        TypeCode::LongDouble => {
            let bytes = unbase64(j, p)?;
            let arr: [u8; 16] = bytes.try_into().map_err(|_| Error {
                path: p.into(),
                message: "a long double is 16 octets".into(),
            })?;
            Value::LongDouble(arr)
        }

        TypeCode::WChar => match j.as_str().and_then(|s| {
            let mut it = s.chars();
            it.next().filter(|_| it.next().is_none())
        }) {
            Some(c) => Value::WChar(c),
            None => return fail(p, "a wchar is a string of exactly one character"),
        },
        TypeCode::String(_) => match j {
            Json::String(s) => Value::String(s.clone()),
            other => return wrong(p, "a string", other),
        },
        TypeCode::WString(_) => match j {
            Json::String(s) => Value::WString(s.clone()),
            other => return wrong(p, "a string", other),
        },

        TypeCode::Enum { members, name, .. } => match j.as_str() {
            Some(s) if members.iter().any(|m| m == s) => Value::Enum(s.to_owned()),
            Some(s) => {
                return fail(
                    p,
                    format!("{s:?} is not an enumerator of {name}; it has {}", members.join(", ")),
                );
            }
            // An ordinal would work on the wire and mean the wrong thing after
            // the next release, which is exactly what §5.3 calls conditionally
            // breaking. Names cost nothing and cannot drift.
            None => return fail(p, format!("an enumerator of {name} is named, not numbered")),
        },

        TypeCode::Struct { members, name, .. } | TypeCode::Except { members, name, .. } => {
            let Json::Object(map) = j else { return wrong(p, "an object", j) };
            let extra: Vec<&str> = map
                .keys()
                .map(String::as_str)
                .filter(|k| !members.iter().any(|m| m.name == *k))
                .collect();
            if !extra.is_empty() {
                // Not ignored: an unknown member is either a typo or a caller
                // built against a different contract, and both are worth
                // knowing before the bytes go out.
                return fail(p, format!("{name} has no member(s) {}", extra.join(", ")));
            }
            let mut out = Vec::with_capacity(members.len());
            for m in members {
                let Some(v) = map.get(&m.name) else {
                    return fail(p, format!("{name} needs a member {:?}", m.name));
                };
                out.push((m.name.clone(), from_json_at(&m.tc, v, h, &member(p, &m.name))?));
            }
            Value::Struct(out)
        }

        TypeCode::Union { discriminator, cases, default_index, name, .. } => {
            let Some(dj) = j.get("_d") else {
                return fail(p, format!("a {name} needs an explicit discriminator in \"_d\""));
            };
            let d = from_json_at(discriminator, dj, h, &member(p, "_d"))?;
            let case = crate::select_case_public(discriminator, cases, *default_index, &d, p)?;
            let value = match (case, j.get("_v")) {
                (Some(c), Some(vj)) => {
                    Some(Box::new(from_json_at(&c.tc, vj, h, &member(p, "_v"))?))
                }
                (Some(c), None) => {
                    return fail(p, format!("branch {:?} of {name} needs a \"_v\"", c.name));
                }
                (None, Some(_)) => {
                    return fail(p, format!("the selected branch of {name} has no member"));
                }
                (None, None) => None,
            };
            Value::Union { discriminator: Box::new(d), value }
        }

        TypeCode::Sequence { element, .. } if is_binary(element) => {
            Value::List(unbase64(j, p)?.into_iter().map(Value::Octet).collect())
        }
        TypeCode::Sequence { element, .. } => match j {
            Json::Array(items) => Value::List(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, x)| from_json_at(element, x, h, &index(p, i)))
                    .collect::<Result<_>>()?,
            ),
            other => return wrong(p, "an array", other),
        },
        TypeCode::Array { element, length } => match j {
            Json::Array(items) if items.len() == *length as usize => Value::List(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, x)| from_json_at(element, x, h, &index(p, i)))
                    .collect::<Result<_>>()?,
            ),
            Json::Array(items) => {
                return fail(p, format!("this array has {length} elements, {} given", items.len()));
            }
            other => return wrong(p, "an array", other),
        },

        TypeCode::Any => {
            let (Some(tj), Some(vj)) = (j.get("_t"), j.get("_v")) else {
                return fail(p, "an any is {\"_t\": <type>, \"_v\": <value>}");
            };
            let inner = tc_from_json(tj, &member(p, "_t"))?;
            let val = from_json_at(&inner, vj, h, &member(p, "_v"))?;
            Value::Any(Box::new(inner), Box::new(val))
        }

        TypeCode::TypeCode => Value::TypeCode(Box::new(tc_from_json(j, p)?)),

        TypeCode::ObjRef { .. } => match j.get("_ref") {
            Some(Json::Null) => Value::ObjRef(None),
            Some(Json::String(handle)) => match h.resolve(handle) {
                Some(ior) => Value::ObjRef(Some(ior)),
                // A handle we never issued is the whole point of handles: it
                // cannot be turned into an address by guessing.
                None => return fail(p, format!("no reference is held under handle {handle:?}")),
            },
            _ => return fail(p, "an object reference is {\"_ref\": <handle>} or {\"_ref\": null}"),
        },

        other => return fail(p, format!("{} cannot cross yet", type_name(other))),
    })
}

fn wrong<T>(p: &str, want: &str, got: &Json) -> Result<T> {
    fail(p, format!("expected {want}, got {}", got.kind()))
}

fn int<T>(j: &Json, p: &str, want: &str) -> Result<T>
where
    T: std::str::FromStr,
{
    match j {
        Json::Number(n) => n
            .parse::<T>()
            .map_err(|_| Error { path: p.to_owned(), message: format!("{n} is not {want}") }),
        other => wrong(p, want, other),
    }
}

/// A 64-bit integer, from the string the mapping emits or a number if one
/// arrives — but only when the number survives the trip exactly.
fn wide_int<T>(j: &Json, p: &str, want: &str) -> Result<T>
where
    T: std::str::FromStr + std::fmt::Display,
{
    let text = match j {
        Json::String(s) => s.clone(),
        Json::Number(n) => n.clone(),
        other => return wrong(p, want, other),
    };
    // The precision advice, not a generic parse error. A value that arrived in
    // exponent form or with a fractional part has *already* been through a
    // double somewhere upstream, so the digits are gone and the caller needs to
    // know why rather than being told the text is malformed.
    let looks_inexact = matches!(j, Json::Number(_))
        && (text.contains(['e', 'E', '.']) || {
            match text.parse::<T>() {
                Ok(v) => v.to_string() != text,
                Err(_) => false,
            }
        });
    if looks_inexact {
        return fail(
            p,
            format!(
                "{text} cannot be carried as a JSON number without losing digits; \
                 send {want} as a string"
            ),
        );
    }
    text.parse().map_err(|_| Error { path: p.to_owned(), message: format!("{text} is not {want}") })
}

fn float(j: &Json, p: &str) -> Result<f64> {
    match j {
        Json::Number(n) => n
            .parse()
            .map_err(|_| Error { path: p.to_owned(), message: format!("{n} is not a number") }),
        Json::Object(_) => match j.get("_f").and_then(Json::as_str) {
            Some("nan") => Ok(f64::NAN),
            Some("+inf") => Ok(f64::INFINITY),
            Some("-inf") => Ok(f64::NEG_INFINITY),
            _ => fail(p, "the only special floats are {\"_f\": \"nan\" | \"+inf\" | \"-inf\"}"),
        },
        other => wrong(p, "a number", other),
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

// ── The structural TypeCode form (AnyJSON v1.1, D008) ───────────────────────
//
// A type describes itself in the document, so neither end needs a shared
// registry to read the other's `any` — the property CDR gets by carrying the
// whole TypeCode inside the encapsulation, and the one a repository id cannot
// have. Additive: every type v1 could spell keeps its v1 spelling, so a v1
// document still parses and still reproduces the same CDR. The object form
// appears only where v1 said nothing at all, or where its name lost something
// the wire keeps — `string<5>` and `string` are the same word to v1 and
// different TypeCode bytes to a peer.

/// The v1 name of a type whose whole identity fits in one, or `None`.
fn short_name(tc: &TypeCode) -> Option<&'static str> {
    Some(match tc {
        TypeCode::Boolean => "boolean",
        TypeCode::Octet => "octet",
        TypeCode::Char => "char",
        TypeCode::WChar => "wchar",
        TypeCode::Short => "short",
        TypeCode::UShort => "unsigned short",
        TypeCode::Long => "long",
        TypeCode::ULong => "unsigned long",
        TypeCode::LongLong => "long long",
        TypeCode::ULongLong => "unsigned long long",
        TypeCode::Float => "float",
        TypeCode::Double => "double",
        TypeCode::LongDouble => "long double",
        TypeCode::String(0) => "string",
        TypeCode::WString(0) => "wstring",
        TypeCode::Any => "any",
        TypeCode::TypeCode => "typecode",
        TypeCode::Void => "void",
        TypeCode::Null => "null",
        _ => return None,
    })
}

fn obj(pairs: impl IntoIterator<Item = (&'static str, Json)>) -> Json {
    Json::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn named(kind: &'static str, id: &str, name: &str, rest: Vec<(&'static str, Json)>) -> Json {
    let mut pairs = vec![
        ("kind", Json::String(kind.to_owned())),
        ("id", Json::String(id.to_owned())),
        ("name", Json::String(name.to_owned())),
    ];
    pairs.extend(rest);
    obj(pairs)
}

fn members_json(ms: &[Member]) -> Json {
    Json::Array(
        ms.iter()
            .map(|m| obj([("name", Json::String(m.name.clone())), ("type", tc_to_json(&m.tc))]))
            .collect(),
    )
}

/// A `TypeCode` as an AnyJSON document.
pub fn tc_to_json(tc: &TypeCode) -> Json {
    if let Some(short) = short_name(tc) {
        return Json::String(short.to_owned());
    }
    match tc {
        TypeCode::String(bound) => {
            obj([("kind", Json::String("string".into())), ("bound", number(bound))])
        }
        TypeCode::WString(bound) => {
            obj([("kind", Json::String("wstring".into())), ("bound", number(bound))])
        }
        TypeCode::Sequence { element, bound } => obj([
            ("kind", Json::String("seq".into())),
            ("element", tc_to_json(element)),
            ("bound", number(bound)),
        ]),
        TypeCode::Array { element, length } => obj([
            ("kind", Json::String("array".into())),
            ("element", tc_to_json(element)),
            ("length", number(length)),
        ]),
        TypeCode::Fixed { digits, scale } => obj([
            ("kind", Json::String("fixed".into())),
            ("digits", number(digits)),
            ("scale", number(scale)),
        ]),
        TypeCode::ObjRef { id, name } => named("objref", id, name, vec![]),
        TypeCode::Struct { id, name, members } => {
            named("struct", id, name, vec![("members", members_json(members))])
        }
        TypeCode::Except { id, name, members } => {
            named("except", id, name, vec![("members", members_json(members))])
        }
        TypeCode::Enum { id, name, members } => named(
            "enum",
            id,
            name,
            vec![(
                "members",
                Json::Array(members.iter().map(|m| Json::String(m.clone())).collect()),
            )],
        ),
        TypeCode::Alias { id, name, aliased } => {
            named("alias", id, name, vec![("aliased", tc_to_json(aliased))])
        }
        TypeCode::Union { id, name, discriminator, default_index, cases } => named(
            "union",
            id,
            name,
            vec![
                ("discriminator", tc_to_json(discriminator)),
                ("default", number(default_index)),
                (
                    "cases",
                    Json::Array(
                        cases
                            .iter()
                            .map(|c| {
                                obj([
                                    // The label stays **base64 of the raw
                                    // bytes**, and this is the one place the
                                    // mapping is deliberately not readable.
                                    //
                                    // A value would be better and was written
                                    // first: `"label": 1` is language-neutral,
                                    // and the Python runtime represents union
                                    // labels as values, not bytes. It was
                                    // reverted on a measurement. A label is
                                    // stored in *the byte order of the stream
                                    // it was decoded from* (`typecode.rs`
                                    // reads it with `get_bytes` and writes it
                                    // with `put_bytes`, neither of which knows
                                    // the endianness), and the TypeCode does
                                    // not record which that was — so turning
                                    // the bytes into a number means guessing.
                                    // Base64 is exact without guessing, and it
                                    // is honest about carrying something this
                                    // mapping cannot yet interpret. When the
                                    // wire defect is fixed this becomes a
                                    // value; until then a Python client can
                                    // relay a union TypeCode and not read one.
                                    ("label", Json::String(base64(&c.label))),
                                    ("name", Json::String(c.name.clone())),
                                    ("type", tc_to_json(&c.tc)),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ],
        ),
        TypeCode::Recursive(id) => {
            obj([("kind", Json::String("recursive".into())), ("id", Json::String(id.clone()))])
        }
        TypeCode::Principal => obj([("kind", Json::String("principal".into()))]),
        // Every remaining variant has a short name and returned above.
        other => Json::String(short_name(other).unwrap_or("void").to_owned()),
    }
}

fn field<'a>(j: &'a Json, key: &str, p: &str) -> Result<&'a Json> {
    j.get(key).ok_or_else(|| Error {
        path: p.to_owned(),
        message: format!("a type object needs a {key:?} field"),
    })
}

fn text(j: &Json, key: &str, p: &str) -> Result<String> {
    match field(j, key, p)? {
        Json::String(s) => Ok(s.clone()),
        other => wrong(p, &format!("a string for {key:?}"), other),
    }
}

fn num<T: std::str::FromStr>(j: &Json, key: &str, p: &str) -> Result<T> {
    int(field(j, key, p)?, p, key)
}

/// A `TypeCode` read back out of an AnyJSON document.
pub fn tc_from_json(j: &Json, p: &str) -> Result<TypeCode> {
    if let Json::String(name) = j {
        return named_type(name).ok_or_else(|| Error {
            path: p.to_owned(),
            message: format!("unknown type name {name:?}"),
        });
    }
    let Json::Object(_) = j else {
        return wrong(p, "a type name or a type object", j);
    };
    let kind = text(j, "kind", p)?;
    Ok(match kind.as_str() {
        "string" => TypeCode::String(num(j, "bound", p)?),
        "wstring" => TypeCode::WString(num(j, "bound", p)?),
        "seq" => TypeCode::Sequence {
            element: Box::new(tc_from_json(field(j, "element", p)?, &member(p, "element"))?),
            bound: num(j, "bound", p)?,
        },
        "array" => TypeCode::Array {
            element: Box::new(tc_from_json(field(j, "element", p)?, &member(p, "element"))?),
            length: num(j, "length", p)?,
        },
        "fixed" => TypeCode::Fixed { digits: num(j, "digits", p)?, scale: num(j, "scale", p)? },
        "objref" => TypeCode::ObjRef { id: text(j, "id", p)?, name: text(j, "name", p)? },
        "struct" => TypeCode::Struct {
            id: text(j, "id", p)?,
            name: text(j, "name", p)?,
            members: members_from_json(field(j, "members", p)?, p)?,
        },
        "except" => TypeCode::Except {
            id: text(j, "id", p)?,
            name: text(j, "name", p)?,
            members: members_from_json(field(j, "members", p)?, p)?,
        },
        "enum" => TypeCode::Enum {
            id: text(j, "id", p)?,
            name: text(j, "name", p)?,
            members: match field(j, "members", p)? {
                Json::Array(items) => items
                    .iter()
                    .map(|i| match i {
                        Json::String(s) => Ok(s.clone()),
                        other => wrong(p, "an enumerator name", other),
                    })
                    .collect::<Result<_>>()?,
                other => return wrong(p, "an array of enumerator names", other),
            },
        },
        "alias" => TypeCode::Alias {
            id: text(j, "id", p)?,
            name: text(j, "name", p)?,
            aliased: Box::new(tc_from_json(field(j, "aliased", p)?, &member(p, "aliased"))?),
        },
        "union" => TypeCode::Union {
            id: text(j, "id", p)?,
            name: text(j, "name", p)?,
            discriminator: Box::new(tc_from_json(
                field(j, "discriminator", p)?,
                &member(p, "discriminator"),
            )?),
            default_index: num(j, "default", p)?,
            cases: match field(j, "cases", p)? {
                Json::Array(items) => items
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let at = index(&member(p, "cases"), i);
                        Ok(UnionCase {
                            label: unbase64(field(c, "label", &at)?, &at)?,
                            name: text(c, "name", &at)?,
                            tc: tc_from_json(field(c, "type", &at)?, &at)?,
                        })
                    })
                    .collect::<Result<_>>()?,
                other => return wrong(p, "an array of union cases", other),
            },
        },
        "recursive" => TypeCode::Recursive(text(j, "id", p)?),
        "principal" => TypeCode::Principal,
        other => return fail(p, format!("unknown type kind {other:?}")),
    })
}

fn members_from_json(j: &Json, p: &str) -> Result<Vec<Member>> {
    match j {
        Json::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let at = index(&member(p, "members"), i);
                Ok(Member {
                    name: text(m, "name", &at)?,
                    tc: tc_from_json(field(m, "type", &at)?, &at)?,
                })
            })
            .collect(),
        other => wrong(p, "an array of members", other),
    }
}

fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

fn unbase64(j: &Json, p: &str) -> Result<Vec<u8>> {
    let Some(s) = j.as_str() else { return wrong(p, "a base64 string", j) };
    let bytes = s.as_bytes();
    if bytes.len() % 4 != 0 {
        return fail(p, "base64 length must be a multiple of 4");
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let mut n = 0u32;
        let mut pad = 0;
        for (i, &c) in chunk.iter().enumerate() {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                // Padding is only legal in the last two positions, and only at
                // the end. Accepting it anywhere would let two documents decode
                // to the same bytes, which is a signature-forgery shape.
                b'=' if i >= 2 => {
                    pad += 1;
                    0
                }
                _ => return fail(p, format!("{:?} is not a base64 character", c as char)),
            };
            n = (n << 6) | u32::from(v);
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

/// The `_t` tag for an `any`.
///
/// Only primitives for now, and the decoder says so rather than guessing: a
/// constructed type needs a repository id looked up in the registry, and the
/// registry is not a parameter here yet.
fn type_name(tc: &TypeCode) -> String {
    match resolved(tc) {
        TypeCode::Boolean => "boolean".into(),
        TypeCode::Octet => "octet".into(),
        TypeCode::Char => "char".into(),
        TypeCode::WChar => "wchar".into(),
        TypeCode::Short => "short".into(),
        TypeCode::UShort => "unsigned short".into(),
        TypeCode::Long => "long".into(),
        TypeCode::ULong => "unsigned long".into(),
        TypeCode::LongLong => "long long".into(),
        TypeCode::ULongLong => "unsigned long long".into(),
        TypeCode::Float => "float".into(),
        TypeCode::Double => "double".into(),
        TypeCode::LongDouble => "long double".into(),
        TypeCode::String(_) => "string".into(),
        TypeCode::WString(_) => "wstring".into(),
        other => other.repository_id().unwrap_or("<anonymous>").to_owned(),
    }
}

/// The inverse of [`short_name`]. Every name that one produces this one must
/// accept, or the mapping writes a document it cannot read — the exact defect
/// D008 was drafted from, reintroduced one table apart. Held to it by
/// `short_name_and_named_type_are_inverses`.
fn named_type(name: &str) -> Option<TypeCode> {
    Some(match name {
        "boolean" => TypeCode::Boolean,
        "octet" => TypeCode::Octet,
        "char" => TypeCode::Char,
        "wchar" => TypeCode::WChar,
        "short" => TypeCode::Short,
        "unsigned short" => TypeCode::UShort,
        "long" => TypeCode::Long,
        "unsigned long" => TypeCode::ULong,
        "long long" => TypeCode::LongLong,
        "unsigned long long" => TypeCode::ULongLong,
        "float" => TypeCode::Float,
        "double" => TypeCode::Double,
        "long double" => TypeCode::LongDouble,
        "string" => TypeCode::String(0),
        "wstring" => TypeCode::WString(0),
        "any" => TypeCode::Any,
        "typecode" => TypeCode::TypeCode,
        "void" => TypeCode::Void,
        "null" => TypeCode::Null,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_cdr::{Decoder, Encoder, Endian};
    use orbweaver_giop::typecode::{Member, UnionCase};

    /// The acceptance criterion from §8: the bytes must come back identical.
    /// Comparing `Value`s would miss an encoder that agrees with a decoder and
    /// disagrees with CDR.
    fn bytes_survive(tc: &TypeCode, v: &Value) {
        let mut h = LocalReferences::new();
        let j = to_json(tc, v, &mut h).unwrap_or_else(|e| panic!("to_json: {e}"));
        let text = j.to_string();
        let reparsed = Json::parse(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
        let back = from_json(tc, &reparsed, &h).unwrap_or_else(|e| panic!("from_json {text}: {e}"));

        for endian in [Endian::Big, Endian::Little] {
            let mut a = Encoder::new(endian);
            crate::encode(&mut a, tc, v).expect("encode original");
            let mut b = Encoder::new(endian);
            crate::encode(&mut b, tc, &back).expect("encode round-tripped");
            assert_eq!(
                a.finish().unwrap(),
                b.finish().unwrap(),
                "{endian:?}: {text} did not reproduce the same CDR"
            );
        }
        // And the decoded value agrees, which catches a mapping that is
        // self-consistently wrong about a field CDR happens not to distinguish.
        let mut e = Encoder::new(Endian::Big);
        crate::encode(&mut e, tc, &back).unwrap();
        let bytes = e.finish().unwrap();
        let decoded = crate::decode(&mut Decoder::new(&bytes, Endian::Big), tc).unwrap();
        assert_eq!(&decoded, &back, "decode disagreed for {text}");
    }

    #[test]
    fn primitives_reproduce_identical_cdr() {
        for (tc, v) in [
            (TypeCode::Boolean, Value::Bool(true)),
            (TypeCode::Octet, Value::Octet(255)),
            (TypeCode::Short, Value::Short(-32_768)),
            (TypeCode::UShort, Value::UShort(65_535)),
            (TypeCode::Long, Value::Long(i32::MIN)),
            (TypeCode::ULong, Value::ULong(u32::MAX)),
            (TypeCode::Double, Value::Double(-0.125)),
            (TypeCode::Float, Value::Float(0.5)),
            (TypeCode::String(0), Value::String("hello".into())),
            (TypeCode::WString(0), Value::WString("안녕".into())),
            (TypeCode::WChar, Value::WChar('한')),
            (TypeCode::LongDouble, Value::LongDouble([3u8; 16])),
        ] {
            bytes_survive(&tc, &v);
        }
    }

    /// The rule the mapping exists for. A JSON number is a double everywhere
    /// that matters, so this value would come back changed.
    #[test]
    fn a_64_bit_integer_crosses_as_a_string_and_keeps_every_digit() {
        let v = Value::ULongLong(18_446_744_073_709_551_615);
        bytes_survive(&TypeCode::ULongLong, &v);

        let mut h = LocalReferences::new();
        let j = to_json(&TypeCode::ULongLong, &v, &mut h).unwrap();
        assert_eq!(j, Json::String("18446744073709551615".into()));

        bytes_survive(&TypeCode::LongLong, &Value::LongLong(i64::MIN));
    }

    /// An agent that sends a number instead is accepted when nothing is lost
    /// and refused, with the reason, when something would be.
    #[test]
    fn a_64_bit_integer_sent_as_a_number_is_checked_rather_than_rounded() {
        let h = LocalReferences::new();
        let small = Json::parse("42").unwrap();
        assert_eq!(from_json(&TypeCode::LongLong, &small, &h).unwrap(), Value::LongLong(42));

        let big = Json::parse("9007199254740993").unwrap(); // 2^53 + 1
        let ok = from_json(&TypeCode::LongLong, &big, &h).unwrap();
        assert_eq!(ok, Value::LongLong(9_007_199_254_740_993), "exact text still parses");

        let rounded = Json::parse("1.8446744073709552e19").unwrap();
        let err = from_json(&TypeCode::ULongLong, &rounded, &h).unwrap_err();
        assert!(err.message.contains("as a string"), "{err}");
    }

    #[test]
    fn an_octet_sequence_crosses_as_base64_not_as_a_million_numbers() {
        let tc = TypeCode::Sequence { element: Box::new(TypeCode::Octet), bound: 0 };
        let v = Value::List((0u16..=255).map(|b| Value::Octet(b as u8)).collect());
        bytes_survive(&tc, &v);

        let mut h = LocalReferences::new();
        let j = to_json(&tc, &Value::List(vec![Value::Octet(0xFF), Value::Octet(0x00)]), &mut h)
            .unwrap();
        assert_eq!(j, Json::String("/wA=".into()));
    }

    /// The two name tables must be inverses. They were not: `short_name`
    /// emitted `any`, `typecode`, `void` and `null`, and `named_type` knew
    /// fifteen primitives, so the mapping wrote four names it could not read —
    /// which is the defect D008 exists for, reintroduced one table apart and
    /// two hours later. Enumerated rather than spot-checked, because a spot
    /// check is what missed it.
    #[test]
    fn short_name_and_named_type_are_inverses() {
        for tc in [
            TypeCode::Boolean,
            TypeCode::Octet,
            TypeCode::Char,
            TypeCode::WChar,
            TypeCode::Short,
            TypeCode::UShort,
            TypeCode::Long,
            TypeCode::ULong,
            TypeCode::LongLong,
            TypeCode::ULongLong,
            TypeCode::Float,
            TypeCode::Double,
            TypeCode::LongDouble,
            TypeCode::String(0),
            TypeCode::WString(0),
            TypeCode::Any,
            TypeCode::TypeCode,
            TypeCode::Void,
            TypeCode::Null,
        ] {
            let name = short_name(&tc).unwrap_or_else(|| panic!("{tc:?} has no short name"));
            let back = named_type(name)
                .unwrap_or_else(|| panic!("short_name emits {name:?} and named_type refuses it"));
            assert_eq!(back, tc, "{name:?}");
        }
    }

    #[test]
    fn base64_round_trips_at_every_padding_length() {
        for n in 0..8usize {
            let bytes: Vec<u8> = (0..n).map(|i| (i * 37) as u8).collect();
            let text = base64(&bytes);
            let back = unbase64(&Json::String(text.clone()), "").unwrap();
            assert_eq!(back, bytes, "{text}");
        }
    }

    /// Padding in the wrong place would let two documents mean the same bytes.
    #[test]
    fn malformed_base64_is_refused() {
        for bad in ["=AAA", "A===", "AA", "A!AA", "AAAAA"] {
            assert!(unbase64(&Json::String(bad.into()), "").is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn nan_and_the_infinities_survive() {
        let mut h = LocalReferences::new();
        for (x, tag) in [(f64::NAN, "nan"), (f64::INFINITY, "+inf"), (f64::NEG_INFINITY, "-inf")] {
            let j = to_json(&TypeCode::Double, &Value::Double(x), &mut h).unwrap();
            assert_eq!(j.get("_f").and_then(Json::as_str), Some(tag));
            let back = from_json(&TypeCode::Double, &j, &h).unwrap();
            match back {
                Value::Double(y) if x.is_nan() => assert!(y.is_nan()),
                Value::Double(y) => assert_eq!(y, x),
                other => panic!("{other:?}"),
            }
        }
    }

    fn point() -> TypeCode {
        TypeCode::Struct {
            id: "IDL:m/Point:1.0".into(),
            name: "Point".into(),
            members: vec![
                Member { name: "px".into(), tc: TypeCode::Long },
                Member { name: "py".into(), tc: TypeCode::Long },
            ],
        }
    }

    #[test]
    fn a_struct_crosses_as_an_object_and_comes_back_in_declaration_order() {
        let v = Value::Struct(vec![("px".into(), Value::Long(11)), ("py".into(), Value::Long(22))]);
        bytes_survive(&point(), &v);

        // JSON objects are unordered, so the members must come back in the
        // order the TypeCode gives, not the order the document listed them.
        let j = Json::parse(r#"{"py":22,"px":11}"#).unwrap();
        assert_eq!(from_json(&point(), &j, &LocalReferences::new()).unwrap(), v);
    }

    #[test]
    fn an_unknown_member_is_refused_rather_than_ignored() {
        let j = Json::parse(r#"{"px":1,"py":2,"pz":3}"#).unwrap();
        let err = from_json(&point(), &j, &LocalReferences::new()).unwrap_err();
        assert!(err.message.contains("pz"), "{err}");
    }

    #[test]
    fn a_missing_member_names_itself() {
        let j = Json::parse(r#"{"px":1}"#).unwrap();
        let err = from_json(&point(), &j, &LocalReferences::new()).unwrap_err();
        assert!(err.message.contains("\"py\""), "{err}");
    }

    #[test]
    fn a_nested_diagnostic_names_the_path() {
        let tc = TypeCode::Sequence { element: Box::new(point()), bound: 0 };
        let j = Json::parse(r#"[{"px":1,"py":2},{"px":1,"py":"two"}]"#).unwrap();
        let err = from_json(&tc, &j, &LocalReferences::new()).unwrap_err();
        assert_eq!(err.path, "[1].py", "{err}");
    }

    #[test]
    fn an_enumerator_crosses_by_name_and_an_ordinal_is_refused() {
        let tc = TypeCode::Enum {
            id: "IDL:m/E:1.0".into(),
            name: "E".into(),
            members: vec!["RED".into(), "GREEN".into()],
        };
        bytes_survive(&tc, &Value::Enum("GREEN".into()));

        let err = from_json(&tc, &Json::parse("1").unwrap(), &LocalReferences::new()).unwrap_err();
        assert!(err.message.contains("named, not numbered"), "{err}");
    }

    fn union_tc() -> TypeCode {
        TypeCode::Union {
            id: "IDL:m/U:1.0".into(),
            name: "U".into(),
            discriminator: Box::new(TypeCode::Long),
            default_index: -1,
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
    fn a_union_carries_its_discriminator_explicitly() {
        let v = Value::Union {
            discriminator: Box::new(Value::Long(2)),
            value: Some(Box::new(Value::String("chosen".into()))),
        };
        bytes_survive(&union_tc(), &v);

        let mut h = LocalReferences::new();
        let j = to_json(&union_tc(), &v, &mut h).unwrap();
        assert_eq!(j.get("_d"), Some(&Json::Number("2".into())));
        assert_eq!(j.get("_v"), Some(&Json::String("chosen".into())));

        // Without _d there is nothing to infer from: two branches could hold a
        // string, and guessing would pick one silently.
        let err = from_json(&union_tc(), &Json::parse(r#"{"_v":"x"}"#).unwrap(), &h).unwrap_err();
        assert!(err.message.contains("_d"), "{err}");
    }

    #[test]
    fn an_any_carries_its_own_type() {
        let tc = TypeCode::Any;
        let v = Value::Any(Box::new(TypeCode::Double), Box::new(Value::Double(2.5)));
        bytes_survive(&tc, &v);

        let mut h = LocalReferences::new();
        let j = to_json(&tc, &v, &mut h).unwrap();
        assert_eq!(j.get("_t").and_then(Json::as_str), Some("double"));
    }

    /// The security rule, and the one that cannot be relaxed later: §4.7 makes
    /// an IOR a bearer address, so the mapping must be incapable of emitting
    /// one.
    #[test]
    fn an_object_reference_crosses_as_a_handle_and_never_as_an_address() {
        let ior = orbweaver_giop::Ior {
            type_id: "IDL:m/I:1.0".into(),
            profiles: vec![orbweaver_giop::IiopProfile {
                version: orbweaver_giop::Version::V1_2,
                host: "10.0.0.7".into(),
                port: 4242,
                object_key: b"secret-key".to_vec(),
                components: Vec::new(),
            }],
        };
        let tc = TypeCode::ObjRef { id: "IDL:m/I:1.0".into(), name: "I".into() };

        let mut h = LocalReferences::new();
        let j = to_json(&tc, &Value::ObjRef(Some(ior.clone())), &mut h).unwrap();
        let text = j.to_string();
        assert!(!text.contains("10.0.0.7"), "the host leaked into JSON: {text}");
        assert!(!text.contains("secret-key"), "the object key leaked into JSON: {text}");
        assert!(!text.contains("IOR:"), "a stringified IOR leaked: {text}");
        assert_eq!(j.get("_type").and_then(Json::as_str), Some("IDL:m/I:1.0"));

        assert_eq!(from_json(&tc, &j, &h).unwrap(), Value::ObjRef(Some(ior)));

        // A handle nobody issued cannot be turned into an address by guessing.
        let forged = Json::parse(r#"{"_ref":"obj-0000000000000001"}"#).unwrap();
        assert!(from_json(&tc, &forged, &LocalReferences::new()).is_err());
    }

    #[test]
    fn a_nil_reference_is_distinct_from_an_absent_one() {
        let tc = TypeCode::ObjRef { id: "IDL:m/I:1.0".into(), name: "I".into() };
        bytes_survive(&tc, &Value::ObjRef(None));

        let mut h = LocalReferences::new();
        let j = to_json(&tc, &Value::ObjRef(None), &mut h).unwrap();
        assert_eq!(j, Json::parse(r#"{"_ref":null}"#).unwrap());
        assert!(from_json(&tc, &Json::parse("{}").unwrap(), &h).is_err(), "absent is not nil");
    }

    /// The whole point of §8's criterion: a deeply mixed value, byte-identical
    /// after a trip through text.
    #[test]
    fn a_realistic_nested_value_reproduces_identical_cdr() {
        let line = TypeCode::Struct {
            id: "IDL:m/Line:1.0".into(),
            name: "Line".into(),
            members: vec![
                Member { name: "sku".into(), tc: TypeCode::String(0) },
                Member { name: "qty".into(), tc: TypeCode::Long },
                Member { name: "cents".into(), tc: TypeCode::LongLong },
                Member {
                    name: "raw".into(),
                    tc: TypeCode::Sequence { element: Box::new(TypeCode::Octet), bound: 0 },
                },
            ],
        };
        let order = TypeCode::Struct {
            id: "IDL:m/Order:1.0".into(),
            name: "Order".into(),
            members: vec![
                Member { name: "flag".into(), tc: TypeCode::Octet },
                Member {
                    name: "lines".into(),
                    tc: TypeCode::Sequence { element: Box::new(line), bound: 0 },
                },
                Member { name: "rate".into(), tc: TypeCode::Double },
            ],
        };
        let mk = |sku: &str, qty: i32, cents: i64, raw: &[u8]| {
            Value::Struct(vec![
                ("sku".into(), Value::String(sku.into())),
                ("qty".into(), Value::Long(qty)),
                ("cents".into(), Value::LongLong(cents)),
                ("raw".into(), Value::List(raw.iter().map(|b| Value::Octet(*b)).collect())),
            ])
        };
        bytes_survive(
            &order,
            &Value::Struct(vec![
                ("flag".into(), Value::Octet(1)),
                (
                    "lines".into(),
                    Value::List(vec![
                        mk("사과", 3, 9_007_199_254_740_993, &[0, 1, 2]),
                        mk("", 0, i64::MIN, &[]),
                    ]),
                ),
                ("rate".into(), Value::Double(1.0 / 3.0)),
            ]),
        );
    }
}
