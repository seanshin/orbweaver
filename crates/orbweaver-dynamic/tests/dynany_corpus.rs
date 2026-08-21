//! The DynAny oracle: walk every golden-corpus type, rebuild it, compare CDR.
//!
//! §8's discipline applied to the mutation API. A navigator that reads
//! plausibly and writes plausibly is worth nothing; the question is whether a
//! value taken apart component by component and put back together component by
//! component is *the same value on the wire*. So for every named type in the
//! corpus this test:
//!
//! 1. builds a filled value **without touching [`DynAny`]**,
//! 2. walks that value with `DynAny` navigation alone, copying each component
//!    into a second `DynAny` that started at its default and is written only
//!    through [`DynAny::set`], [`DynAny::set_length`] and
//!    [`DynAny::set_discriminator`], and
//! 3. requires the two to encode to **identical bytes**, in both byte orders
//!    and at every alignment phase.
//!
//! # Why step 1 does not use `DynAny`
//!
//! It did, and the oracle was worthless: `next()` was broken by hand to skip
//! every second component and **the test still passed**, because the same
//! broken `next()` had also decided which components got filled. A producer
//! and a consumer that share the defect agree about the result. The sampler
//! below is therefore an independent second opinion about what the value
//! should be — the same reason `spikes/differential.sh` runs two front ends.
//!
//! *생성기와 검증기가 같은 코드를 쓰면 결함이 상쇄된다. 표본 생성기는 DynAny를
//! 전혀 쓰지 않는다 — 그래서 오라클이 된다.*
//!
//! Byte equality rather than value equality, for the reason `prop.rs` gives:
//! value equality catches a walk that loses information, byte equality also
//! catches one that recovers the value while disagreeing about where the
//! padding went. This does not contradict `CLAUDE.md`'s "compare decoded
//! values, never raw buffers" — that rule is about comparing against a
//! reference ORB, where padding content is undefined. Both buffers here are
//! ours.
//!
//! # What it counts
//!
//! Types it could not handle are **counted and named**, not skipped silently:
//! an unmeasured check is a failure, never a pass.

use std::path::{Path, PathBuf};

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_dynamic::dynany::{DynAny, Label};
use orbweaver_dynamic::{Value, encode};
use orbweaver_giop::typecode::{TypeCode, UnionCase};
use orbweaver_registry::Registry;

/// How deep the sampler will nest before it stops growing sequences. A
/// recursive type has no finite expansion, so the depth has to come from
/// somewhere that is not the type.
const MAX_DEPTH: usize = 6;

/// How many elements a sequence gets, bound permitting. Two rather than one:
/// a walk that confuses "the element" with "the sequence" passes at length 1.
const ELEMENTS: usize = 2;

fn corpus(dir: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(dir);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "idl"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no IDL in {}", root.display());
    files
}

fn registry_of(path: &Path) -> Registry {
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let spec = orbweaver_idl::parse(&src)
        .unwrap_or_else(|d| panic!("{} does not parse: {d:?}", path.display()));
    let mut r = Registry::new();
    r.load(&spec).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    r
}

// ---------------------------------------------------------------------------
// The sampler. Knows nothing about DynAny.
// ---------------------------------------------------------------------------

/// Why a type could not be sampled. Carried rather than dropped, so the gap
/// shows up in the count with a reason attached.
struct Uncovered(String);

/// A leaf value that differs from its neighbours, so a walk that swaps two
/// components is a byte difference rather than a coincidence.
fn leaf(tc: &TypeCode, tick: &mut u64) -> Result<Value, Uncovered> {
    *tick += 1;
    let t = *tick;
    Ok(match tc {
        TypeCode::Null | TypeCode::Void => Value::Struct(Vec::new()),
        TypeCode::Boolean => Value::Bool(t % 2 == 1),
        TypeCode::Octet => Value::Octet((t % 251) as u8),
        TypeCode::Char => Value::Char(b'a' + (t % 26) as u8),
        TypeCode::WChar => Value::WChar(char::from(b'A' + (t % 26) as u8)),
        TypeCode::Short => Value::Short(t as i16),
        TypeCode::UShort => Value::UShort(t as u16),
        TypeCode::Long => Value::Long(t as i32),
        TypeCode::ULong => Value::ULong(t as u32),
        TypeCode::LongLong => Value::LongLong(t as i64),
        TypeCode::ULongLong => Value::ULongLong(t),
        TypeCode::Float => Value::Float(t as f32 * 0.5),
        TypeCode::Double => Value::Double(t as f64 * 0.25),
        TypeCode::LongDouble => Value::LongDouble([(t % 251) as u8; 16]),
        TypeCode::String(bound) => Value::String(bounded(&format!("s{t}"), *bound)),
        TypeCode::WString(bound) => Value::WString(bounded(&format!("w{t}"), *bound)),
        TypeCode::ObjRef { .. } => Value::ObjRef(None),
        // A TypeCode as a value in its own right (D008). Something structural
        // rather than a primitive, so the walk carries a shape and not a tag.
        TypeCode::TypeCode => Value::TypeCode(Box::new(TypeCode::Sequence {
            element: Box::new(TypeCode::Octet),
            bound: (t % 8) as u32,
        })),
        TypeCode::Enum { members, name, .. } => {
            match members.get(t as usize % members.len().max(1)) {
                Some(m) => Value::Enum(m.clone()),
                None => return Err(Uncovered(format!("enum {name} has no enumerators"))),
            }
        }
        // An `any`'s contained TypeCode is part of its value, so the sampler
        // chooses one. `DynAny` can navigate into it afterwards; it could not
        // have invented it.
        TypeCode::Any => {
            Value::Any(Box::new(TypeCode::LongLong), Box::new(Value::LongLong(t as i64)))
        }
        TypeCode::Fixed { .. } => {
            return Err(Uncovered("`fixed` has no Value variant (docs/PLAN.md §4.4)".into()));
        }
        // The other two of §4.4's three, uncovered for the same reason and now
        // *named* for the same reason. They used to arrive here as
        // `TypeCode::ObjRef` and be sampled as `Value::ObjRef(None)` — walked,
        // counted as covered, and covered nothing: the sampler was answering
        // about a reference where the type is a value.
        TypeCode::Value { .. } => {
            return Err(Uncovered("a `valuetype` has no Value variant (docs/PLAN.md §4.4)".into()));
        }
        TypeCode::AbstractInterface { .. } => {
            return Err(Uncovered(
                "an abstract interface has no Value variant (docs/PLAN.md §4.4)".into(),
            ));
        }
        // The fourth, and the only one of the four that is not §4.4's: a
        // `native` has no Value variant and never will, because there is no
        // wire form to give it one. It arrived here as `TypeCode::ObjRef` and
        // was sampled as `Value::ObjRef(None)` until 2026-08-21 — walked,
        // counted as covered, covering nothing at all.
        TypeCode::Native { .. } => {
            return Err(Uncovered(
                "a `native` has no Value variant and no wire form to give it one — not a \
                 docs/PLAN.md §4.4 deferral"
                    .into(),
            ));
        }
        TypeCode::Principal => return Err(Uncovered("`Principal` has no Value variant".into())),
        other => return Err(Uncovered(format!("no leaf value for {other:?}"))),
    })
}

fn bounded(s: &str, bound: u32) -> String {
    if bound == 0 { s.to_string() } else { s.chars().take(bound as usize).collect() }
}

fn bounded_len(bound: u32) -> usize {
    if bound == 0 { usize::MAX } else { bound as usize }
}

/// Follows aliases and recursion markers, recording what it passed so a marker
/// below resolves. The library does the same thing; doing it again here is the
/// price of the sampler being independent.
fn resolve(tc: &TypeCode, open: &mut Vec<TypeCode>) -> Result<TypeCode, Uncovered> {
    let mut t = tc.clone();
    loop {
        match &t {
            TypeCode::Alias { aliased, .. } => {
                let next = (**aliased).clone();
                open.push(t);
                t = next;
            }
            TypeCode::Recursive(id) => {
                match open.iter().rev().find(|o| o.repository_id() == Some(id.as_str())) {
                    Some(target) => t = target.clone(),
                    None => {
                        return Err(Uncovered(format!("recursive marker {id} does not resolve")));
                    }
                }
            }
            _ => return Ok(t),
        }
    }
}

/// The branch a discriminator selects, decided exactly as the encoder decides
/// it: the label is the discriminator encoded big-endian.
fn select<'c>(
    disc: &TypeCode,
    cases: &'c [UnionCase],
    default_index: i32,
    d: &Value,
) -> Option<&'c UnionCase> {
    let mut e = Encoder::new(Endian::Big);
    encode(&mut e, disc, d).ok()?;
    let label = e.finish().ok()?;
    if let Some(c) = cases.iter().find(|c| c.label == label) {
        return Some(c);
    }
    if default_index >= 0 {
        return cases.get(default_index as usize);
    }
    None
}

fn sample(
    tc: &TypeCode,
    tick: &mut u64,
    open: &mut Vec<TypeCode>,
    depth: usize,
) -> Result<Value, Uncovered> {
    if depth > MAX_DEPTH + 8 {
        return Err(Uncovered("the sampler nested further than it should have".into()));
    }
    let mark = open.len();
    let t = resolve(tc, open)?;
    let out = match &t {
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
            open.push(t.clone());
            let mut out = Vec::with_capacity(members.len());
            for m in members {
                out.push((m.name.clone(), sample(&m.tc, tick, open, depth + 1)?));
            }
            Value::Struct(out)
        }
        TypeCode::Sequence { element, bound } => {
            let n = if depth >= MAX_DEPTH { 0 } else { ELEMENTS.min(bounded_len(*bound)) };
            open.push(t.clone());
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                out.push(sample(element, tick, open, depth + 1)?);
            }
            Value::List(out)
        }
        TypeCode::Array { element, length } => {
            open.push(t.clone());
            let mut out = Vec::with_capacity(*length as usize);
            for _ in 0..*length {
                out.push(sample(element, tick, open, depth + 1)?);
            }
            Value::List(out)
        }
        TypeCode::Union { discriminator, cases, default_index, name, .. } => {
            open.push(t.clone());
            // Start at a different case for each union, so the corpus does not
            // measure branch zero seventy-six times.
            let start = if cases.is_empty() { 0 } else { *tick as usize % cases.len() };
            let mut chosen = None;
            for k in 0..cases.len() {
                let c = &cases[(start + k) % cases.len()];
                let Ok(d) = orbweaver_dynamic::decode(
                    &mut Decoder::new(&c.label, Endian::Big),
                    discriminator,
                ) else {
                    continue;
                };
                if let Some(sel) = select(discriminator, cases, *default_index, &d) {
                    chosen = Some((d, sel.tc.clone()));
                    break;
                }
            }
            let Some((d, branch_tc)) = chosen else {
                return Err(Uncovered(format!("no discriminator selects a branch of {name}")));
            };
            let branch = sample(&branch_tc, tick, open, depth + 1)?;
            Value::Union { discriminator: Box::new(d), value: Some(Box::new(branch)) }
        }
        other => leaf(other, tick)?,
    };
    open.truncate(mark);
    Ok(out)
}

// ---------------------------------------------------------------------------
// The walk. Knows nothing except DynAny.
// ---------------------------------------------------------------------------

/// Copies the focused node of `src` into the focused node of `dst`, using only
/// navigation on one and mutation on the other.
fn copy(src: &mut DynAny, dst: &mut DynAny) -> orbweaver_dynamic::Result<()> {
    let tc = src.current_type()?;
    match &tc {
        TypeCode::Union { .. } => {
            src.enter()?;
            let disc = src.current_value()?.clone();
            src.leave()?;
            dst.set_discriminator(disc)?;
            if src.component_count()? < 2 {
                return Ok(());
            }
            src.enter()?;
            src.seek(1)?;
            dst.enter()?;
            dst.seek(1)?;
            copy(src, dst)?;
            src.leave()?;
            dst.leave()
        }
        TypeCode::Sequence { .. } => {
            let n = src.component_count()?;
            dst.set_length(n)?;
            walk_children(src, dst, n)
        }
        TypeCode::Struct { .. } | TypeCode::Except { .. } | TypeCode::Array { .. } => {
            let n = src.component_count()?;
            walk_children(src, dst, n)
        }
        TypeCode::Any => {
            // The contained TypeCode is part of the value, so it has to be set
            // whole before there is anything to navigate into. The descent
            // afterwards is not redundant: it is the only thing that proves an
            // `any`'s interior is reachable at all.
            dst.set(src.current_value()?.clone())?;
            src.enter()?;
            dst.enter()?;
            assert_eq!(src.current_label()?, Some(Label::Contained));
            let inner = src.current_value()?.clone();
            dst.set(inner)?;
            src.leave()?;
            dst.leave()
        }
        _ => dst.set(src.current_value()?.clone()),
    }
}

fn walk_children(src: &mut DynAny, dst: &mut DynAny, n: usize) -> orbweaver_dynamic::Result<()> {
    if n == 0 {
        return Ok(());
    }
    src.enter()?;
    dst.enter()?;
    loop {
        copy(src, dst)?;
        let more = src.next()?;
        assert_eq!(more, dst.next()?, "the two walks disagreed about where they were");
        if !more {
            break;
        }
    }
    src.leave()?;
    dst.leave()
}

/// Encoded after `phase` filler octets, so the value begins at every offset
/// modulo 8 in turn. `0xEE` rather than zero so padding stays visible.
fn bytes_at(tc: &TypeCode, v: &Value, endian: Endian, phase: usize) -> Vec<u8> {
    let mut e = Encoder::new(endian);
    for _ in 0..phase {
        e.put_octet(0xEE);
    }
    encode(&mut e, tc, v).unwrap_or_else(|err| panic!("encode: {err}"));
    e.finish().expect("finish")
}

struct Report {
    walked: usize,
    /// Of the walked types, how many the walk can be wrong about.
    ///
    /// A type with no settable leaf — a bare interface reference, an empty
    /// struct — is reassembled correctly by a `copy` that does nothing at all,
    /// because its default already *is* the value. Those are counted
    /// separately rather than folded into the headline number, since a walk
    /// that cannot fail is not evidence that the walk is right.
    distinguishable: usize,
    unsupported: Vec<String>,
}

fn walk_corpus(dir: &str) -> Report {
    let mut walked = 0usize;
    let mut distinguishable = 0usize;
    let mut unsupported = Vec::new();

    for path in corpus(dir) {
        let reg = registry_of(&path);
        let file = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        for id in reg.ids().cloned().collect::<Vec<_>>() {
            let Some(tc) = reg.typecode(&id) else { continue };

            let mut tick = 0u64;
            let value = match sample(tc, &mut tick, &mut Vec::new(), 0) {
                Ok(v) => v,
                Err(Uncovered(why)) => {
                    unsupported.push(format!("{file}: {id}: {why}"));
                    continue;
                }
            };
            let mut src = DynAny::new(tc.clone(), value)
                .unwrap_or_else(|e| panic!("{file}: {id}: the sampled value is invalid: {e}"));

            let mut dst = match DynAny::empty(tc.clone()) {
                Ok(d) => d,
                Err(e) => {
                    unsupported.push(format!("{file}: {id}: no starting value: {e}"));
                    continue;
                }
            };
            if src.value() != dst.value() {
                distinguishable += 1;
            }

            copy(&mut src, &mut dst)
                .unwrap_or_else(|e| panic!("{file}: {id}: the walk failed: {e}"));
            src.reset();
            dst.reset();

            for endian in [Endian::Big, Endian::Little] {
                for phase in 0..8 {
                    let a = bytes_at(tc, src.value(), endian, phase);
                    let b = bytes_at(tc, dst.value(), endian, phase);
                    assert_eq!(
                        a, b,
                        "{file}: {id}: reassembled value differs on the wire \
                         ({endian:?}, phase {phase})"
                    );
                }
            }
            // And the reassembled value is the value, not merely bytes that
            // happen to match: a difference here with matching bytes would be
            // a defect in `Value`'s equality, which is worth knowing about.
            assert_eq!(src.value(), dst.value(), "{file}: {id}: value differs");
            walked += 1;
        }
    }
    // Printed, not only asserted: the count is the measurement, and a number
    // that exists only inside a passing assertion is a number nobody reads.
    println!(
        "{dir}: {walked} type(s) walked ({distinguishable} distinguishable from their \
         default), {} uncovered",
        unsupported.len()
    );
    for gap in &unsupported {
        println!("  uncovered: {gap}");
    }
    Report { walked, distinguishable, unsupported }
}

/// The measurement. Every named type in `corpus/golden/`, taken apart with
/// navigation and put back together with mutation.
#[test]
fn every_golden_type_survives_a_dynany_walk() {
    let r = walk_corpus("corpus/golden");

    // A floor rather than an equality: the corpus grows with the change that
    // motivates it, and a test that must be edited for every addition gets
    // edited without being read.
    assert!(r.walked >= 60, "only {} type(s) walked; the corpus holds far more", r.walked);
    // The walk has to be capable of being wrong about most of what it walks,
    // or a green result means nothing.
    assert!(
        r.distinguishable * 2 > r.walked,
        "only {} of {} walked types differ from their default, so most of this test cannot \
         fail",
        r.distinguishable,
        r.walked
    );

    // The uncovered types are the four the wire cannot carry, and every one of
    // them must say so. `fixed` used to be the only entry here, not because it
    // was the only such type but because the other three arrived as
    // `TypeCode::ObjRef` and were walked as references — the gap was invisible
    // to the gap report. Anything uncovered for a reason that does not cite
    // §4.4 is a new gap and has to be looked at; a `native` cites it in order
    // to say it does not apply, which is the sentence that separates "not
    // implemented yet" from "there is nothing to implement".
    for gap in &r.unsupported {
        assert!(
            gap.contains("§4.4"),
            "a type is uncovered for a reason that is not the known one:\n  {}",
            r.unsupported.join("\n  ")
        );
    }
    for what in ["`fixed`", "`valuetype`", "abstract interface", "`native`"] {
        assert!(
            r.unsupported.iter().any(|g| g.contains(what)),
            "corpus/golden must still report {what} as uncovered:\n  {}",
            r.unsupported.join("\n  ")
        );
    }
}

/// The service contracts as well: they are the ones an agent actually reaches,
/// and they carry identity pragmas that change the repository ids the walk
/// resolves recursion markers by.
#[test]
fn every_service_type_survives_a_dynany_walk() {
    let r = walk_corpus("corpus/services");
    assert!(r.walked > 0, "no service type walked");
    assert!(r.unsupported.is_empty(), "uncovered service type:\n  {}", r.unsupported.join("\n  "));
}
