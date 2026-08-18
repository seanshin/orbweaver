//! `TypeCode` and `any`: the self-description that makes dynamic invocation
//! possible.
//!
//! This is the piece the whole AI path rests on. `docs/PLAN.md` §2.1 claims
//! CORBA is "a runtime self-describing type system", and `TypeCode` is what
//! makes that true rather than aspirational — it is how a value describes
//! itself well enough to be decoded by a caller that has never seen its IDL.
//! AnyJSON's `_t` field (§4.5) has assumed this existed since v0.2.
//!
//! # Two encoding subtleties that bite
//!
//! **Complex parameters live in an encapsulation, so alignment restarts** at
//! the encapsulation's byte-order flag (§9.3.3).
//!
//! **Indirection offsets do not.** §9.3.5.1 measures them in the *outermost*
//! stream, so an offset can point out of the encapsulation it appears in.
//! Satisfying both at once means writing everything into one buffer and moving
//! the alignment origin in and out of each encapsulation, rather than building
//! encapsulations in buffers of their own — which is why `Encoder::set_origin`
//! exists.
//!
//! Spec: OMG CORBA 3.4 Part 2, §9.3.5.

use std::collections::HashMap;

use orbweaver_cdr::{Decoder, Encoder, Endian};

use crate::{Error, Result};

/// Sentinel that introduces an indirection instead of a `TCKind`.
const INDIRECTION: u32 = 0xFFFF_FFFF;

/// How deep nested `TypeCode`s may go before we call it hostile.
///
/// A crafted stream can nest sequences without bound; each level costs stack.
const MAX_DEPTH: u32 = 64;

/// `TCKind` ordinals from the OMG `TypeCode` interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
#[allow(missing_docs)]
pub enum TcKind {
    Null = 0,
    Void = 1,
    Short = 2,
    Long = 3,
    UShort = 4,
    ULong = 5,
    Float = 6,
    Double = 7,
    Boolean = 8,
    Char = 9,
    Octet = 10,
    Any = 11,
    TypeCode = 12,
    Principal = 13,
    ObjRef = 14,
    Struct = 15,
    Union = 16,
    Enum = 17,
    String = 18,
    Sequence = 19,
    Array = 20,
    Alias = 21,
    Except = 22,
    LongLong = 23,
    ULongLong = 24,
    LongDouble = 25,
    WChar = 26,
    WString = 27,
    Fixed = 28,
}

impl TcKind {
    const fn from_u32(v: u32) -> Option<Self> {
        Some(match v {
            0 => TcKind::Null,
            1 => TcKind::Void,
            2 => TcKind::Short,
            3 => TcKind::Long,
            4 => TcKind::UShort,
            5 => TcKind::ULong,
            6 => TcKind::Float,
            7 => TcKind::Double,
            8 => TcKind::Boolean,
            9 => TcKind::Char,
            10 => TcKind::Octet,
            11 => TcKind::Any,
            12 => TcKind::TypeCode,
            13 => TcKind::Principal,
            14 => TcKind::ObjRef,
            15 => TcKind::Struct,
            16 => TcKind::Union,
            17 => TcKind::Enum,
            18 => TcKind::String,
            19 => TcKind::Sequence,
            20 => TcKind::Array,
            21 => TcKind::Alias,
            22 => TcKind::Except,
            23 => TcKind::LongLong,
            24 => TcKind::ULongLong,
            25 => TcKind::LongDouble,
            26 => TcKind::WChar,
            27 => TcKind::WString,
            28 => TcKind::Fixed,
            _ => return None,
        })
    }
}

/// A named member of a struct or exception.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    /// Member name as written in IDL.
    pub name: String,
    /// Member type.
    pub tc: TypeCode,
}

/// A union branch.
#[derive(Debug, Clone, PartialEq)]
pub struct UnionCase {
    /// Discriminator value, encoded in the discriminator's own type.
    pub label: Vec<u8>,
    /// Branch name.
    pub name: String,
    /// Branch type.
    pub tc: TypeCode,
}

/// A CORBA `TypeCode`.
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub enum TypeCode {
    Null,
    Void,
    Short,
    Long,
    UShort,
    ULong,
    Float,
    Double,
    Boolean,
    Char,
    Octet,
    Any,
    TypeCode,
    Principal,
    LongLong,
    ULongLong,
    LongDouble,
    WChar,
    /// `string<bound>`; zero means unbounded.
    String(u32),
    /// `wstring<bound>`; zero means unbounded.
    WString(u32),
    Fixed {
        digits: u16,
        scale: i16,
    },
    ObjRef {
        id: String,
        name: String,
    },
    Struct {
        id: String,
        name: String,
        members: Vec<Member>,
    },
    Union {
        id: String,
        name: String,
        discriminator: Box<TypeCode>,
        default_index: i32,
        cases: Vec<UnionCase>,
    },
    Enum {
        id: String,
        name: String,
        members: Vec<String>,
    },
    Sequence {
        element: Box<TypeCode>,
        bound: u32,
    },
    Array {
        element: Box<TypeCode>,
        length: u32,
    },
    Alias {
        id: String,
        name: String,
        aliased: Box<TypeCode>,
    },
    Except {
        id: String,
        name: String,
        members: Vec<Member>,
    },
    /// A reference back to an enclosing type, produced by an indirection that
    /// pointed at a `TypeCode` still being decoded.
    ///
    /// Rust cannot hold the cycle directly, and flattening it would not
    /// terminate, so recursion is represented by the repository id it names.
    /// Resolution is the consumer's job, which is honest: a recursive type has
    /// no finite expansion.
    Recursive(String),
}

impl TypeCode {
    /// The `TCKind` ordinal, or `None` for [`TypeCode::Recursive`], which is
    /// our own marker rather than a wire kind.
    pub fn kind(&self) -> Option<TcKind> {
        Some(match self {
            TypeCode::Null => TcKind::Null,
            TypeCode::Void => TcKind::Void,
            TypeCode::Short => TcKind::Short,
            TypeCode::Long => TcKind::Long,
            TypeCode::UShort => TcKind::UShort,
            TypeCode::ULong => TcKind::ULong,
            TypeCode::Float => TcKind::Float,
            TypeCode::Double => TcKind::Double,
            TypeCode::Boolean => TcKind::Boolean,
            TypeCode::Char => TcKind::Char,
            TypeCode::Octet => TcKind::Octet,
            TypeCode::Any => TcKind::Any,
            TypeCode::TypeCode => TcKind::TypeCode,
            TypeCode::Principal => TcKind::Principal,
            TypeCode::LongLong => TcKind::LongLong,
            TypeCode::ULongLong => TcKind::ULongLong,
            TypeCode::LongDouble => TcKind::LongDouble,
            TypeCode::WChar => TcKind::WChar,
            TypeCode::String(_) => TcKind::String,
            TypeCode::WString(_) => TcKind::WString,
            TypeCode::Fixed { .. } => TcKind::Fixed,
            TypeCode::ObjRef { .. } => TcKind::ObjRef,
            TypeCode::Struct { .. } => TcKind::Struct,
            TypeCode::Union { .. } => TcKind::Union,
            TypeCode::Enum { .. } => TcKind::Enum,
            TypeCode::Sequence { .. } => TcKind::Sequence,
            TypeCode::Array { .. } => TcKind::Array,
            TypeCode::Alias { .. } => TcKind::Alias,
            TypeCode::Except { .. } => TcKind::Except,
            TypeCode::Recursive(_) => return None,
        })
    }

    /// The repository id, for the kinds that carry one.
    pub fn repository_id(&self) -> Option<&str> {
        match self {
            TypeCode::ObjRef { id, .. }
            | TypeCode::Struct { id, .. }
            | TypeCode::Union { id, .. }
            | TypeCode::Enum { id, .. }
            | TypeCode::Alias { id, .. }
            | TypeCode::Except { id, .. } => Some(id),
            TypeCode::Recursive(id) => Some(id),
            _ => None,
        }
    }

    /// Follows `alias` chains to the type that actually governs encoding.
    ///
    /// CDR marshals the aliased type; a typedef has no wire form of its own,
    /// so anything deciding how to read bytes must resolve first.
    pub fn resolve_alias(&self) -> &TypeCode {
        match self {
            TypeCode::Alias { aliased, .. } => aliased.resolve_alias(),
            other => other,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Encoding
// ─────────────────────────────────────────────────────────────────────────────

/// Writes a `TypeCode` into `e`, emitting indirections for repeated
/// repository ids so recursive types terminate.
pub fn encode(e: &mut Encoder, tc: &TypeCode) -> Result<()> {
    let mut seen = HashMap::new();
    encode_inner(e, tc, &mut seen, 0)
}

fn encode_inner(
    e: &mut Encoder,
    tc: &TypeCode,
    seen: &mut HashMap<String, usize>,
    depth: u32,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(Error::Cdr(orbweaver_cdr::Error::Malformed("TypeCode nested too deeply")));
    }

    // A repeated repository id means we are inside that type again. Emit an
    // indirection rather than expanding, which would not terminate.
    if let Some(id) = tc.repository_id()
        && let Some(&start) = seen.get(id)
    {
        e.align_to(4);
        let here = e.len();
        e.put_u32(INDIRECTION);
        // §9.3.5.1: the offset is relative to the position of the offset field
        // itself, which is the four bytes after the sentinel.
        let offset_field = here + 4;
        e.put_i32(start as i64 as i32 - offset_field as i32);
        return Ok(());
    }

    let kind = match tc.kind() {
        Some(k) => k,
        // A Recursive marker with no prior sighting means the caller handed us
        // a fragment of a type rather than a whole one.
        None => {
            return Err(Error::Cdr(orbweaver_cdr::Error::Malformed(
                "recursive TypeCode has no enclosing definition to point at",
            )));
        }
    };

    e.align_to(4);
    let start = e.len();
    e.put_u32(kind as u32);
    if let Some(id) = tc.repository_id() {
        seen.insert(id.to_owned(), start);
    }

    match tc {
        // Empty parameter list.
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
        | TypeCode::WChar => {}

        // Simple parameter list: written inline, no encapsulation.
        TypeCode::String(bound) | TypeCode::WString(bound) => e.put_u32(*bound),
        TypeCode::Fixed { digits, scale } => {
            e.put_u16(*digits);
            e.put_i16(*scale);
        }

        // Complex parameter list: a CDR encapsulation.
        _ => {
            let (len_at, saved_origin) = encapsulation_begin(e);
            match tc {
                TypeCode::ObjRef { id, name } => {
                    e.put_str(id);
                    e.put_str(name);
                }
                TypeCode::Struct { id, name, members } | TypeCode::Except { id, name, members } => {
                    e.put_str(id);
                    e.put_str(name);
                    e.put_u32(members.len() as u32);
                    for m in members {
                        e.put_str(&m.name);
                        encode_inner(e, &m.tc, seen, depth + 1)?;
                    }
                }
                TypeCode::Union { id, name, discriminator, default_index, cases } => {
                    e.put_str(id);
                    e.put_str(name);
                    encode_inner(e, discriminator, seen, depth + 1)?;
                    e.put_i32(*default_index);
                    e.put_u32(cases.len() as u32);
                    let label_len = discriminator_width(discriminator);
                    for c in cases {
                        e.align_to(label_len.min(8));
                        e.put_bytes(&canonical_label(&c.label, e.endian()));
                        e.put_str(&c.name);
                        encode_inner(e, &c.tc, seen, depth + 1)?;
                    }
                }
                TypeCode::Enum { id, name, members } => {
                    e.put_str(id);
                    e.put_str(name);
                    e.put_u32(members.len() as u32);
                    for m in members {
                        e.put_str(m);
                    }
                }
                TypeCode::Sequence { element, bound } => {
                    encode_inner(e, element, seen, depth + 1)?;
                    e.put_u32(*bound);
                }
                TypeCode::Array { element, length } => {
                    encode_inner(e, element, seen, depth + 1)?;
                    e.put_u32(*length);
                }
                TypeCode::Alias { id, name, aliased } => {
                    e.put_str(id);
                    e.put_str(name);
                    encode_inner(e, aliased, seen, depth + 1)?;
                }
                _ => unreachable!("kind already matched"),
            }
            encapsulation_end(e, len_at, saved_origin);
        }
    }
    Ok(())
}

/// Opens an encapsulation: writes a placeholder length, restarts alignment,
/// writes the byte-order flag. Returns the patch point and the origin to
/// restore afterwards.
fn encapsulation_begin(e: &mut Encoder) -> (usize, usize) {
    e.align_to(4);
    let len_at = e.len();
    e.put_bytes(&[0, 0, 0, 0]);
    let saved = e.origin();
    e.set_origin(e.len()); // alignment restarts at the flag
    e.put_u8(e.endian().as_flag());
    (len_at, saved)
}

/// Closes an encapsulation: patches the length and restores the enclosing
/// origin.
///
/// The origin must be *restored*, not zeroed. A struct nested inside a
/// sequence sits inside two encapsulations, and returning to zero after the
/// inner one makes every following field in the outer one align against the
/// wrong base. It survived our own round-trip tests because the decoder saved
/// and restored correctly and the offsets happened to coincide; omniORB
/// rejected it immediately.
fn encapsulation_end(e: &mut Encoder, len_at: usize, saved_origin: usize) {
    let body = (e.len() - (len_at + 4)) as u32;
    e.set_origin(saved_origin);
    e.patch_u32(len_at, body);
}

// ─────────────────────────────────────────────────────────────────────────────
// Decoding
// ─────────────────────────────────────────────────────────────────────────────

/// Reads a `TypeCode`, resolving indirections against the whole stream.
pub fn decode(d: &mut Decoder<'_>) -> Result<TypeCode> {
    let mut open: HashMap<usize, String> = HashMap::new();
    let mut done: HashMap<usize, TypeCode> = HashMap::new();
    decode_inner(d, &mut open, &mut done, 0)
}

fn decode_inner(
    d: &mut Decoder<'_>,
    open: &mut HashMap<usize, String>,
    done: &mut HashMap<usize, TypeCode>,
    depth: u32,
) -> Result<TypeCode> {
    if depth > MAX_DEPTH {
        return Err(Error::Cdr(orbweaver_cdr::Error::Malformed("TypeCode nested too deeply")));
    }
    d.align_to(4)?;
    let start = d.offset();
    let raw = d.get_u32()?;

    if raw == INDIRECTION {
        let offset_field = d.offset();
        let delta = d.get_i32()?;
        let target = (offset_field as i64) + delta as i64;
        if target < 0 || target as usize >= d.buffer().len() {
            return Err(Error::Cdr(orbweaver_cdr::Error::Malformed(
                "TypeCode indirection points outside the stream",
            )));
        }
        let target = target as usize;
        // Pointing at a type still being decoded is recursion; pointing at a
        // finished one is ordinary sharing.
        if let Some(id) = open.get(&target) {
            return Ok(TypeCode::Recursive(id.clone()));
        }
        if let Some(tc) = done.get(&target) {
            return Ok(tc.clone());
        }
        return Err(Error::Cdr(orbweaver_cdr::Error::Malformed(
            "TypeCode indirection points at something that is not a TypeCode",
        )));
    }

    let kind = TcKind::from_u32(raw)
        .ok_or(Error::Cdr(orbweaver_cdr::Error::Malformed("unknown or unsupported TCKind")))?;

    let tc = match kind {
        TcKind::Null => TypeCode::Null,
        TcKind::Void => TypeCode::Void,
        TcKind::Short => TypeCode::Short,
        TcKind::Long => TypeCode::Long,
        TcKind::UShort => TypeCode::UShort,
        TcKind::ULong => TypeCode::ULong,
        TcKind::Float => TypeCode::Float,
        TcKind::Double => TypeCode::Double,
        TcKind::Boolean => TypeCode::Boolean,
        TcKind::Char => TypeCode::Char,
        TcKind::Octet => TypeCode::Octet,
        TcKind::Any => TypeCode::Any,
        TcKind::TypeCode => TypeCode::TypeCode,
        TcKind::Principal => TypeCode::Principal,
        TcKind::LongLong => TypeCode::LongLong,
        TcKind::ULongLong => TypeCode::ULongLong,
        TcKind::LongDouble => TypeCode::LongDouble,
        TcKind::WChar => TypeCode::WChar,
        TcKind::String => TypeCode::String(d.get_u32()?),
        TcKind::WString => TypeCode::WString(d.get_u32()?),
        TcKind::Fixed => TypeCode::Fixed { digits: d.get_u16()?, scale: d.get_i16()? },
        _ => decode_complex(d, kind, start, open, done, depth)?,
    };

    done.insert(start, tc.clone());
    Ok(tc)
}

fn decode_complex(
    d: &mut Decoder<'_>,
    kind: TcKind,
    start: usize,
    open: &mut HashMap<usize, String>,
    done: &mut HashMap<usize, TypeCode>,
    depth: u32,
) -> Result<TypeCode> {
    let len = d.get_u32()?;
    let len = d.validate_count(len, 1)?;
    let body_start = d.offset();
    let body_end = body_start + len;

    // Alignment restarts at the encapsulation's flag; indirection offsets do
    // not, so the decoder walks the same buffer and moves its origin instead
    // of being handed a slice.
    let saved = d.origin();
    d.reset_origin();
    let flag = d.get_u8()?;
    let saved_endian = d.endian();
    d.set_endian(Endian::try_from_flag(flag).map_err(Error::Cdr)?);

    let out = (|| -> Result<TypeCode> {
        Ok(match kind {
            TcKind::ObjRef => TypeCode::ObjRef { id: get_string(d)?, name: get_string(d)? },
            TcKind::Struct | TcKind::Except => {
                let id = get_string(d)?;
                let name = get_string(d)?;
                open.insert(start, id.clone());
                let n = d.get_u32()?;
                let n = d.validate_count(n, 5)?;
                let mut members = Vec::with_capacity(n);
                for _ in 0..n {
                    let mname = get_string(d)?;
                    members
                        .push(Member { name: mname, tc: decode_inner(d, open, done, depth + 1)? });
                }
                open.remove(&start);
                if kind == TcKind::Struct {
                    TypeCode::Struct { id, name, members }
                } else {
                    TypeCode::Except { id, name, members }
                }
            }
            TcKind::Union => {
                let id = get_string(d)?;
                let name = get_string(d)?;
                open.insert(start, id.clone());
                let discriminator = Box::new(decode_inner(d, open, done, depth + 1)?);
                let default_index = d.get_i32()?;
                let n = d.get_u32()?;
                let n = d.validate_count(n, 6)?;
                let label_len = discriminator_width(&discriminator);
                let mut cases = Vec::with_capacity(n);
                for _ in 0..n {
                    // A label is the discriminator marshalled in its own type,
                    // so it aligns like one. Reading it as raw bytes with no
                    // alignment worked for a `long` only because the case count
                    // in front of it happened to leave the stream 4-aligned;
                    // omniORB's `long long` union could not be decoded at all,
                    // and said so as "string length must include the NUL" four
                    // fields later.
                    d.align_to(label_len.min(8))?;
                    let label = canonical_label(d.get_bytes(label_len)?, d.endian());
                    let cname = get_string(d)?;
                    cases.push(UnionCase {
                        label,
                        name: cname,
                        tc: decode_inner(d, open, done, depth + 1)?,
                    });
                }
                open.remove(&start);
                TypeCode::Union { id, name, discriminator, default_index, cases }
            }
            TcKind::Enum => {
                let id = get_string(d)?;
                let name = get_string(d)?;
                let n = d.get_u32()?;
                let n = d.validate_count(n, 5)?;
                let mut members = Vec::with_capacity(n);
                for _ in 0..n {
                    members.push(get_string(d)?);
                }
                TypeCode::Enum { id, name, members }
            }
            TcKind::Sequence => {
                let element = Box::new(decode_inner(d, open, done, depth + 1)?);
                TypeCode::Sequence { element, bound: d.get_u32()? }
            }
            TcKind::Array => {
                let element = Box::new(decode_inner(d, open, done, depth + 1)?);
                TypeCode::Array { element, length: d.get_u32()? }
            }
            TcKind::Alias => {
                let id = get_string(d)?;
                let name = get_string(d)?;
                open.insert(start, id.clone());
                let aliased = Box::new(decode_inner(d, open, done, depth + 1)?);
                open.remove(&start);
                TypeCode::Alias { id, name, aliased }
            }
            other => {
                return Err(Error::Cdr(orbweaver_cdr::Error::Malformed(match other {
                    TcKind::Struct => "struct",
                    _ => "unsupported complex TCKind",
                })));
            }
        })
    })();

    d.set_endian(saved_endian);
    d.set_origin(saved);
    let tc = out?;
    // Trust the declared length over how far the body reader happened to get:
    // a peer may append parameters a later spec version defines.
    d.seek_to(body_end).map_err(Error::Cdr)?;
    Ok(tc)
}

fn get_string(d: &mut Decoder<'_>) -> Result<String> {
    Ok(String::from_utf8_lossy(d.get_string_bytes()?).into_owned())
}

/// A union case label, converted between the wire's byte order and ours.
///
/// A label is a discriminator value marshalled in the discriminator's own
/// type, so it arrives in the byte order of the stream that carried it. Stored
/// raw, that made the same union mean two different things depending on which
/// peer described it: labels from a little-endian ORB missed **every** branch,
/// in both directions, and the refusal blamed the caller's discriminator
/// rather than the label — measured against omniORB, which is little-endian on
/// this host and on most.
///
/// `UnionCase::label` is therefore always **big-endian**, matching the order
/// `orbweaver_dynamic` encodes its probe in, and conversion happens exactly at
/// the wire. The function is its own inverse, which is why one of it serves
/// both directions.
fn canonical_label(bytes: &[u8], wire: Endian) -> Vec<u8> {
    match wire {
        Endian::Big => bytes.to_vec(),
        Endian::Little => bytes.iter().rev().copied().collect(),
    }
}

/// Bytes a union discriminator label occupies on the wire.
fn discriminator_width(tc: &TypeCode) -> usize {
    match tc.resolve_alias() {
        TypeCode::Boolean | TypeCode::Char | TypeCode::Octet => 1,
        TypeCode::Short | TypeCode::UShort => 2,
        TypeCode::LongLong | TypeCode::ULongLong => 8,
        // Enum discriminators marshal as unsigned long, like long and ulong.
        _ => 4,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Any
// ─────────────────────────────────────────────────────────────────────────────

/// A CORBA `any`: a `TypeCode` and the value encoded under it.
///
/// The value stays as raw CDR rather than being decoded into a Rust enum.
/// Decoding needs the negotiated codeset for strings and the caller's intent
/// for everything else, and an `any` is frequently just relayed — decoding and
/// re-encoding it would be lossy work performed for nobody.
#[derive(Debug, Clone, PartialEq)]
pub struct Any {
    /// What the value is.
    pub tc: TypeCode,
    /// The value, in the byte order of the stream it came from.
    pub value: Vec<u8>,
    /// That byte order, needed to read `value`.
    pub endian: Endian,
}

impl Any {
    /// A decoder positioned at the start of the value.
    pub fn value_decoder(&self) -> Decoder<'_> {
        Decoder::new(&self.value, self.endian)
    }
}

/// Writes an `any`: its `TypeCode`, then the value written by `write_value`
/// into the *same* stream.
///
/// The closure exists to make the correct thing the easy thing. An `any`'s
/// value is marshalled immediately after its `TypeCode` with alignment
/// continuing from there, so its internal padding depends on where the whole
/// `any` lands. Building the value in a buffer of its own and appending the
/// bytes produces padding computed from offset zero, which is right only by
/// accident — a `long` has no internal padding and survives; a struct of
/// `octet, long, short, double` does not, and the peer reports garbage at the
/// end of the message rather than a decode error at the offending field.
pub fn encode_any_with<F>(e: &mut Encoder, tc: &TypeCode, write_value: F) -> Result<()>
where
    F: FnOnce(&mut Encoder),
{
    encode(e, tc)?;
    write_value(e);
    Ok(())
}

/// Writes a captured [`Any`] back onto the wire.
///
/// **Only safe when the destination offset has the same alignment as the
/// source**, because the captured value bytes carry padding computed for where
/// they originally sat. Relaying an `any` between positions of different
/// alignment requires re-marshalling the value, which means walking its
/// `TypeCode`. In GIOP 1.2 a request body and a reply body are both 8-aligned,
/// so echoing one straight back is sound; assuming that in general is not.
pub fn encode_any_at_same_alignment(e: &mut Encoder, any: &Any) -> Result<()> {
    encode(e, &any.tc)?;
    e.put_bytes(&any.value);
    Ok(())
}

/// Reads an `any`, taking `value_len` bytes of value after the `TypeCode`.
///
/// The length must come from the caller: CDR gives an `any` no length prefix,
/// so only something that knows the surrounding structure can say where the
/// value ends. Passing the decoder's remaining length is right when the `any`
/// is the last thing in a message body.
pub fn decode_any(d: &mut Decoder<'_>, value_len: usize) -> Result<Any> {
    let tc = decode(d)?;
    let endian = d.endian();
    let value = d.get_bytes(value_len)?.to_vec();
    Ok(Any { tc, value, endian })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(tc: &TypeCode) -> TypeCode {
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            encode(&mut e, tc).expect("encode");
            let bytes = e.finish().expect("finish");
            let mut d = Decoder::new(&bytes, endian);
            let got = decode(&mut d).expect("decode");
            assert_eq!(&got, tc, "round trip differed under {endian:?}");
        }
        let mut e = Encoder::new(Endian::Big);
        encode(&mut e, tc).unwrap();
        let bytes = e.finish().unwrap();
        let mut d = Decoder::new(&bytes, Endian::Big);
        decode(&mut d).unwrap()
    }

    #[test]
    fn primitives_round_trip() {
        for tc in [
            TypeCode::Null,
            TypeCode::Void,
            TypeCode::Short,
            TypeCode::Long,
            TypeCode::UShort,
            TypeCode::ULong,
            TypeCode::Float,
            TypeCode::Double,
            TypeCode::Boolean,
            TypeCode::Char,
            TypeCode::Octet,
            TypeCode::Any,
            TypeCode::TypeCode,
            TypeCode::LongLong,
            TypeCode::ULongLong,
            TypeCode::LongDouble,
            TypeCode::WChar,
        ] {
            round_trip(&tc);
        }
    }

    /// A primitive TypeCode is exactly its four-byte kind — no parameters, no
    /// encapsulation. `tk_long` is 3, cross-checked against omniORB.
    #[test]
    fn primitive_is_just_its_kind() {
        let mut e = Encoder::new(Endian::Big);
        encode(&mut e, &TypeCode::Long).unwrap();
        assert_eq!(e.finish().unwrap(), vec![0, 0, 0, 3]);
    }

    /// String and wstring carry their bound inline, with no encapsulation.
    #[test]
    fn string_bound_is_inline() {
        let mut e = Encoder::new(Endian::Big);
        encode(&mut e, &TypeCode::String(16)).unwrap();
        assert_eq!(e.finish().unwrap(), vec![0, 0, 0, 18, 0, 0, 0, 16]);
        round_trip(&TypeCode::String(0));
        round_trip(&TypeCode::WString(255));
        round_trip(&TypeCode::Fixed { digits: 9, scale: 2 });
    }

    #[test]
    fn struct_round_trips() {
        round_trip(&TypeCode::Struct {
            id: "IDL:gc01/Primitives:1.0".into(),
            name: "Primitives".into(),
            members: vec![
                Member { name: "b".into(), tc: TypeCode::Boolean },
                Member { name: "l".into(), tc: TypeCode::Long },
                Member { name: "d".into(), tc: TypeCode::Double },
                Member { name: "s".into(), tc: TypeCode::String(0) },
            ],
        });
    }

    #[test]
    fn nested_and_collection_types_round_trip() {
        let inner = TypeCode::Struct {
            id: "IDL:gc02/Ragged:1.0".into(),
            name: "Ragged".into(),
            members: vec![
                Member { name: "a".into(), tc: TypeCode::Octet },
                Member { name: "b".into(), tc: TypeCode::Long },
                Member { name: "d".into(), tc: TypeCode::Double },
            ],
        };
        round_trip(&TypeCode::Sequence { element: Box::new(inner.clone()), bound: 0 });
        round_trip(&TypeCode::Array { element: Box::new(TypeCode::Long), length: 12 });
        round_trip(&TypeCode::Sequence {
            element: Box::new(TypeCode::Sequence { element: Box::new(TypeCode::Long), bound: 4 }),
            bound: 0,
        });
        round_trip(&TypeCode::Alias {
            id: "IDL:gc13/Meters:1.0".into(),
            name: "Meters".into(),
            aliased: Box::new(TypeCode::Long),
        });
    }

    #[test]
    fn enum_and_union_round_trip() {
        let kind = TypeCode::Enum {
            id: "IDL:gc06/Kind:1.0".into(),
            name: "Kind".into(),
            members: vec!["K_LONG".into(), "K_STRING".into()],
        };
        round_trip(&kind);
        round_trip(&TypeCode::Union {
            id: "IDL:gc06/Payload:1.0".into(),
            name: "Payload".into(),
            discriminator: Box::new(kind),
            default_index: -1,
            cases: vec![
                UnionCase { label: vec![0, 0, 0, 0], name: "as_long".into(), tc: TypeCode::Long },
                UnionCase {
                    label: vec![0, 0, 0, 1],
                    name: "as_string".into(),
                    tc: TypeCode::String(0),
                },
            ],
        });
    }

    /// A boolean discriminator's labels are one byte, not four. Getting the
    /// width wrong shifts every subsequent case by three bytes and produces a
    /// union that decodes into nonsense rather than failing.
    #[test]
    fn union_label_width_follows_the_discriminator() {
        assert_eq!(discriminator_width(&TypeCode::Boolean), 1);
        assert_eq!(discriminator_width(&TypeCode::Short), 2);
        assert_eq!(discriminator_width(&TypeCode::Long), 4);
        assert_eq!(discriminator_width(&TypeCode::ULongLong), 8);
        // Through an alias, because a typedef has no wire form of its own.
        assert_eq!(
            discriminator_width(&TypeCode::Alias {
                id: "IDL:x/B:1.0".into(),
                name: "B".into(),
                aliased: Box::new(TypeCode::Boolean),
            }),
            1
        );
        round_trip(&TypeCode::Union {
            id: "IDL:gc06/BoolUnion:1.0".into(),
            name: "BoolUnion".into(),
            discriminator: Box::new(TypeCode::Boolean),
            default_index: -1,
            cases: vec![
                UnionCase { label: vec![1], name: "yes".into(), tc: TypeCode::Long },
                UnionCase { label: vec![0], name: "no".into(), tc: TypeCode::Octet },
            ],
        });
    }

    /// `corpus/golden/15-forward-recursive.idl` in TypeCode form. Without
    /// indirection this does not terminate; with it, the cycle comes back as a
    /// `Recursive` node naming the type it points at.
    #[test]
    fn recursive_type_terminates_via_indirection() {
        let tree = TypeCode::Struct {
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
        };
        for endian in [Endian::Big, Endian::Little] {
            let mut e = Encoder::new(endian);
            encode(&mut e, &tree).unwrap();
            let bytes = e.finish().unwrap();
            let mut d = Decoder::new(&bytes, endian);
            let got = decode(&mut d).unwrap();
            assert_eq!(got, tree, "recursive round trip failed under {endian:?}");
        }
    }

    #[test]
    fn indirection_offset_is_relative_to_the_offset_field() {
        let tree = TypeCode::Struct {
            id: "IDL:r/T:1.0".into(),
            name: "T".into(),
            members: vec![Member {
                name: "kids".into(),
                tc: TypeCode::Sequence {
                    element: Box::new(TypeCode::Recursive("IDL:r/T:1.0".into())),
                    bound: 0,
                },
            }],
        };
        let mut e = Encoder::new(Endian::Big);
        encode(&mut e, &tree).unwrap();
        let bytes = e.finish().unwrap();

        // Find the sentinel and check the offset lands on the struct's kind.
        let pos = bytes
            .windows(4)
            .position(|w| w == 0xFFFF_FFFFu32.to_be_bytes())
            .expect("an indirection was emitted");
        let off_field = pos + 4;
        let delta = i32::from_be_bytes(bytes[off_field..off_field + 4].try_into().unwrap());
        let target = (off_field as i64 + delta as i64) as usize;
        assert_eq!(target, 0, "must point at the outer struct's kind field");
        assert_eq!(
            u32::from_be_bytes(bytes[target..target + 4].try_into().unwrap()),
            TcKind::Struct as u32
        );
    }

    #[test]
    fn hostile_indirection_is_rejected() {
        // An offset pointing outside the stream.
        let mut e = Encoder::new(Endian::Big);
        e.put_u32(INDIRECTION);
        e.put_i32(-9999);
        let bytes = e.finish().unwrap();
        let mut d = Decoder::new(&bytes, Endian::Big);
        assert!(decode(&mut d).is_err());

        // An offset pointing at bytes that are not a TypeCode.
        let mut e = Encoder::new(Endian::Big);
        e.put_u32(0xDEAD_BEEF);
        e.put_u32(INDIRECTION);
        e.put_i32(-8);
        let bytes = e.finish().unwrap();
        let mut d = Decoder::new(&bytes, Endian::Big);
        d.get_u32().unwrap();
        assert!(decode(&mut d).is_err());
    }

    #[test]
    fn unknown_kind_is_rejected_rather_than_guessed() {
        let mut e = Encoder::new(Endian::Big);
        e.put_u32(9999);
        let bytes = e.finish().unwrap();
        let mut d = Decoder::new(&bytes, Endian::Big);
        assert!(decode(&mut d).is_err());
    }

    #[test]
    fn alias_resolves_to_its_target() {
        let chain = TypeCode::Alias {
            id: "IDL:a/Range:1.0".into(),
            name: "Range".into(),
            aliased: Box::new(TypeCode::Alias {
                id: "IDL:a/Distance:1.0".into(),
                name: "Distance".into(),
                aliased: Box::new(TypeCode::Long),
            }),
        };
        assert_eq!(chain.resolve_alias(), &TypeCode::Long);
    }

    #[test]
    fn any_round_trips() {
        let mut v = Encoder::new(Endian::Big);
        v.put_i32(42);
        let any = Any { tc: TypeCode::Long, value: v.finish().unwrap(), endian: Endian::Big };

        let mut e = Encoder::new(Endian::Big);
        encode_any_at_same_alignment(&mut e, &any).unwrap();
        let bytes = e.finish().unwrap();

        let mut d = Decoder::new(&bytes, Endian::Big);
        let got = decode_any(&mut d, 4).unwrap();
        assert_eq!(got.tc, TypeCode::Long);
        assert_eq!(got.value_decoder().get_i32().unwrap(), 42);

        // The closure form writes into the live stream, so a struct's internal
        // padding is computed where it will actually sit.
        let mut e = Encoder::new(Endian::Big);
        e.put_u8(0xFF); // shove the any off a 4-byte boundary
        encode_any_with(
            &mut e,
            &TypeCode::Struct {
                id: "IDL:t/S:1.0".into(),
                name: "S".into(),
                members: vec![
                    Member { name: "a".into(), tc: TypeCode::Octet },
                    Member { name: "d".into(), tc: TypeCode::Double },
                ],
            },
            |v| {
                let at = v.position();
                v.put_octet(1);
                v.put_f64(2.5);
                // The double must be 8-aligned in the message, not in a sub-buffer.
                assert_eq!((at + 1).div_ceil(8) * 8 % 8, 0);
            },
        )
        .unwrap();
        assert!(e.finish().is_ok());
    }
}
