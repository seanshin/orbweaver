//! The Python target's oracle: the generated code is **executed**, and what it
//! produces is held to the Rust mapping over the whole golden corpus at once.
//!
//! # The criterion
//!
//! §4.5 states its own acceptance criterion — for any value, `CDR → JSON →
//! CDR` must reproduce identical bytes — and this test is that rule with
//! Python in the middle:
//!
//! ```text
//! Value --Rust to_json--> JSON --Python from_json--> a Python object
//!       --Python to_json--> JSON --Rust from_json--> Value --encode--> bytes
//! ```
//!
//! The bytes at the end must equal the bytes the original value encodes to, in
//! **both byte orders**. Comparing the two JSON documents as text would be the
//! mistake `CLAUDE.md` names about CDR padding, one layer up: `2.5` and `2.50`
//! are the same value and different strings, and a float's shortest
//! round-tripping spelling is a property of the language that printed it.
//!
//! # What is executed, and what that proves
//!
//! Two things run under CPython. The **runtime** (`_rt.py`), which is a second
//! implementation of §4.5 and the only part of this target that could disagree
//! with the reference mapping. And the **generated stubs**, driven through
//! `_rt.Loopback`: a stub renders its arguments into a request, reads a reply,
//! and both are compared here. So a template that dropped a parameter, ordered
//! two members wrongly, or lost an `out` value fails this test rather than
//! failing a user.
//!
//! No ORB, no fixture and no network are involved. The live half — a generated
//! Python client calling the omniORB fixture through `orbweaver-py-bridge` —
//! is a harness measurement, recorded in
//! `docs/pipeline-runs/2026-08-14-python-target.md` with its result.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_dynamic::anyjson::{self, LocalReferences};
use orbweaver_dynamic::json::Json;
use orbweaver_dynamic::{Value, encode};
use orbweaver_gen::python::{descriptor, emit_python};
use orbweaver_giop::typecode::Member;
use orbweaver_giop::typecode::TypeCode;
use orbweaver_giop::typecode::ValueMember;
use orbweaver_giop::{IiopProfile, Ior, Version};
use orbweaver_registry::{Entry, ParamDirection, Registry};

fn corpus(dir: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(dir);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "idl"))
        .collect();
    files.sort();
    files
}

fn load(path: &Path) -> Option<Registry> {
    let src = std::fs::read_to_string(path).expect("read");
    let spec = orbweaver_idl::parse(&src).expect("golden parses");
    let mut r = Registry::new();
    r.load(&spec).ok()?;
    Some(r)
}

/// A deterministic sample value for a type, or `None` when the type has none.
///
/// Deterministic rather than random: a round-trip oracle that fails on one seed
/// in a thousand is a flake, and the property this test is after — that two
/// implementations of one mapping agree — does not need search to find, it
/// needs coverage of every shape the corpus declares.
///
/// `open` is the chain of constructed types under construction, innermost
/// last, with the `TypeCode` each one names. It is what lets a
/// [`TypeCode::Recursive`] marker be followed: the registry represents a
/// cycle as the marker and an id, so a witness that read the `TypeCode` alone
/// could only produce the empty case — which is what this one did until
/// 2026-08-19, and why `anyjson`'s refusal of every non-empty value under a
/// marker was never red here.
fn witness(tc: &TypeCode, open: &mut Vec<(String, TypeCode)>) -> Option<Value> {
    Some(match tc {
        TypeCode::Boolean => Value::Bool(true),
        TypeCode::Octet => Value::Octet(0xA7),
        // A `char` is one octet of the codeset, so it stays inside ASCII: the
        // wide types below are where non-ASCII text belongs.
        TypeCode::Char => Value::Char(b'Q'),
        TypeCode::WChar => Value::WChar('한'),
        TypeCode::Short => Value::Short(-31_000),
        TypeCode::UShort => Value::UShort(65_000),
        TypeCode::Long => Value::Long(-2_000_000_111),
        TypeCode::ULong => Value::ULong(4_000_000_222),
        // Past 2^53 on purpose: this is the value that proves the two
        // implementations agree that a 64-bit integer crosses as a string.
        TypeCode::LongLong => Value::LongLong(-9_007_199_254_740_993),
        TypeCode::ULongLong => Value::ULongLong(18_014_398_509_481_985),
        TypeCode::Float => Value::Float(-0.5),
        TypeCode::Double => Value::Double(1.0 / 3.0),
        TypeCode::LongDouble => {
            Value::LongDouble([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16])
        }
        TypeCode::String(bound) => Value::String(bounded_text("orbweaver", *bound)),
        TypeCode::WString(bound) => Value::WString(bounded_text("정적 스텁", *bound)),
        // A constructed type, on purpose, and one no generated package
        // declares: `_t` is then AnyJSON v1.1's structural form and Python has
        // to build the type from the document (D008). It was a bare double
        // until 2026-08-19, which crossed as a name string and proved nothing
        // about the form every `any` with a struct in it actually carries.
        TypeCode::Any => Value::Any(Box::new(described()), Box::new(described_value())),
        TypeCode::ObjRef { .. } => Value::ObjRef(Some(sample_ior())),
        TypeCode::Enum { members, .. } => Value::Enum(members.last()?.clone()),
        TypeCode::Sequence { element, bound } => {
            // A sequence back into a type already re-entered once is where
            // recursion terminates: the marker is followed one level, so the
            // value beneath it is a real one, and below that the empty list is
            // the only finite value. One level, deliberately: the document
            // doubles per level and the property is that the marker crosses,
            // not how far.
            if terminates(element, open) {
                Value::List(Vec::new())
            } else {
                let n = if *bound == 0 { 2 } else { (*bound).min(2) } as usize;
                Value::List((0..n).map(|_| witness(element, open)).collect::<Option<_>>()?)
            }
        }
        TypeCode::Array { element, length } => {
            Value::List((0..*length).map(|_| witness(element, open)).collect::<Option<_>>()?)
        }
        TypeCode::Struct { id, members, .. } | TypeCode::Except { id, members, .. } => {
            open.push((id.clone(), tc.clone()));
            let out = members
                .iter()
                .map(|m| Some((m.name.clone(), witness(&m.tc, open)?)))
                .collect::<Option<Vec<_>>>();
            open.pop();
            Value::Struct(out?)
        }
        TypeCode::Union { id, discriminator, cases, default_index, .. } => {
            open.push((id.clone(), tc.clone()));
            // The *last* case rather than the first: with a `default:` present
            // it is usually the default branch, which is the one a generator
            // gets wrong.
            let (i, case) = cases.iter().enumerate().next_back()?;
            let d = label_value(&case.label, discriminator, i as i32 == *default_index, cases)?;
            let v = witness(&case.tc, open);
            open.pop();
            Value::Union { discriminator: Box::new(d), value: Some(Box::new(v?)) }
        }
        // Recorded like a struct: a cycle can name the typedef rather than the
        // type it wraps — `corpus/golden/15`'s `TreeSeq` — and a witness that
        // saw through aliases could never resolve that marker.
        TypeCode::Alias { id, aliased, .. } => {
            open.push((id.clone(), tc.clone()));
            let v = witness(aliased, open);
            open.pop();
            v?
        }
        // The marker, resolved against the type under construction that it
        // names. Termination is `terminates`' job, one sequence up; a marker
        // reached with nothing to resolve against is a TypeCode fragment, and
        // has no witness.
        TypeCode::Recursive(id) => {
            let target = open.iter().rev().find(|(k, _)| k == id)?.1.clone();
            witness(&target, open)?
        }
        // A TypeCode as a value (D008). Deliberately a constructed one: a
        // primitive would cross as the same name string v1 already used, so it
        // would prove nothing about the structural form the ir-subset
        // descriptions are actually made of.
        TypeCode::TypeCode => Value::TypeCode(Box::new(described())),
        _ => return None,
    })
}

/// The type an `any` carries in this sweep: a struct declared by no corpus
/// file, so that Python meets it only as a document.
///
/// Its members are chosen for what they make the two implementations agree on
/// through one value: a **bound** (`string<12>` is structural where `string`
/// is a name), an anonymous sequence, and a `sequence<any>` holding one `any`
/// per short-named type — every name the Rust side's `short_name` writes, the
/// Python table has to read back, and this is the one place that inverse is
/// held across the language boundary rather than one table apart.
fn described() -> TypeCode {
    TypeCode::Struct {
        id: "IDL:witness/Described:1.0".into(),
        name: "Described".into(),
        members: vec![
            Member { name: "label".into(), tc: TypeCode::String(12) },
            Member {
                name: "points".into(),
                tc: TypeCode::Sequence { element: Box::new(TypeCode::Double), bound: 0 },
            },
            Member {
                name: "each".into(),
                tc: TypeCode::Sequence { element: Box::new(TypeCode::Any), bound: 0 },
            },
        ],
    }
}

/// The TypeCode of a `valuetype` — `tk_value`, 29 — with the three slots the
/// two implementations have to agree about beyond a struct's: the
/// `ValueModifier`, a concrete base (absent is `tk_null` and not `tk_void`),
/// and a per-member visibility.
///
/// §4.4 defers the *value*, not this. It is a TypeCode, a TypeCode is a value
/// the v1 wire carries, and so it crosses inside an `any` like any other —
/// which is exactly the thing the Python runtime had no reading half for.
fn deferred_value() -> TypeCode {
    TypeCode::Value {
        id: "IDL:witness/Priced:1.0".into(),
        name: "Priced".into(),
        modifier: 0,
        base: Some(Box::new(TypeCode::Value {
            id: "IDL:witness/Money:1.0".into(),
            name: "Money".into(),
            modifier: 0,
            base: None,
            members: vec![
                ValueMember { name: "currency".into(), tc: TypeCode::String(3), visibility: 1 },
                ValueMember { name: "amount".into(), tc: TypeCode::LongLong, visibility: 0 },
            ],
        })),
        members: vec![ValueMember { name: "sku".into(), tc: TypeCode::String(0), visibility: 1 }],
    }
}

/// The TypeCode of an abstract interface — `tk_abstract_interface`, 32. An id
/// and a name, like an object reference, and pointedly a different kind: on
/// the wire it is the union of a value and a reference, so spelling it as a
/// reference is the wrong answer rather than the deferred one.
fn deferred_abstract() -> TypeCode {
    TypeCode::AbstractInterface {
        id: "IDL:witness/Describable:1.0".into(),
        name: "Describable".into(),
    }
}

fn described_value() -> Value {
    let any = |tc: TypeCode, v: Value| Value::Any(Box::new(tc), Box::new(v));
    Value::Struct(vec![
        ("label".into(), Value::String("witnessed".into())),
        ("points".into(), Value::List(vec![Value::Double(0.5), Value::Double(-2.0)])),
        (
            "each".into(),
            Value::List(vec![
                any(TypeCode::Boolean, Value::Bool(false)),
                any(TypeCode::Octet, Value::Octet(0x5A)),
                any(TypeCode::Char, Value::Char(b'c')),
                any(TypeCode::WChar, Value::WChar('글')),
                any(TypeCode::Short, Value::Short(-7)),
                any(TypeCode::UShort, Value::UShort(7)),
                any(TypeCode::Long, Value::Long(-70_000)),
                any(TypeCode::ULong, Value::ULong(70_000)),
                any(TypeCode::LongLong, Value::LongLong(-9_007_199_254_740_995)),
                any(TypeCode::ULongLong, Value::ULongLong(18_014_398_509_481_987)),
                any(TypeCode::Float, Value::Float(1.5)),
                any(TypeCode::Double, Value::Double(-0.125)),
                any(TypeCode::LongDouble, Value::LongDouble([0xAB; 16])),
                any(TypeCode::String(0), Value::String("unbounded".into())),
                any(TypeCode::WString(0), Value::WString("넓은".into())),
                any(TypeCode::String(5), Value::String("bound".into())),
                any(TypeCode::WString(2), Value::WString("두자".into())),
                any(
                    TypeCode::Sequence { element: Box::new(TypeCode::Long), bound: 3 },
                    Value::List(vec![Value::Long(1), Value::Long(2)]),
                ),
                any(
                    TypeCode::Array { element: Box::new(TypeCode::Octet), length: 2 },
                    Value::List(vec![Value::Octet(1), Value::Octet(2)]),
                ),
                any(TypeCode::TypeCode, Value::TypeCode(Box::new(TypeCode::Long))),
                // §4.4's two deferrals, as the only thing about them that
                // crosses: their TypeCode. Both go through the whole loop —
                // Rust writes the structural form, Python reads it, relays it
                // and writes it back, Rust decodes it and re-encodes it in
                // both byte orders — so `tk_value` and `tk_abstract_interface`
                // are held to the same criterion as every other type here
                // rather than being tested one implementation at a time.
                any(TypeCode::TypeCode, Value::TypeCode(Box::new(deferred_value()))),
                any(TypeCode::TypeCode, Value::TypeCode(Box::new(deferred_abstract()))),
                any(TypeCode::Any, any(TypeCode::Double, Value::Double(2.5))),
            ]),
        ),
    ])
}

/// Whether a sequence element would re-enter, for the second time, a type
/// already being built — once is the level the witness follows the marker
/// to; twice is where it stops.
fn terminates(element: &TypeCode, open: &[(String, TypeCode)]) -> bool {
    let entered = |id: &str| open.iter().filter(|(k, _)| k == id).count() >= 2;
    match element {
        TypeCode::Recursive(id) => entered(id),
        TypeCode::Struct { id, .. } | TypeCode::Union { id, .. } | TypeCode::Except { id, .. } => {
            entered(id)
        }
        TypeCode::Alias { aliased, .. } => terminates(aliased, open),
        TypeCode::Sequence { element, .. } | TypeCode::Array { element, .. } => {
            terminates(element, open)
        }
        _ => false,
    }
}

fn bounded_text(text: &str, bound: u32) -> String {
    if bound == 0 { text.to_owned() } else { text.chars().take(bound as usize).collect() }
}

fn label_value(
    label: &[u8],
    disc: &TypeCode,
    is_default: bool,
    cases: &[orbweaver_giop::typecode::UnionCase],
) -> Option<Value> {
    let mut wide: i64 = 0;
    for b in label {
        wide = (wide << 8) | i64::from(*b);
    }
    // The default branch has no stored label — it is a case of its own with
    // an empty one, where `default:` was written — so a discriminator that
    // matches nothing else is what selects it, and every corpus union that
    // has one reserves a value outside its labels.
    Some(match disc {
        TypeCode::Boolean => Value::Bool(!is_default && wide != 0),
        TypeCode::Long => Value::Long(if is_default { i32::MIN } else { wide as i32 }),
        TypeCode::ULong => Value::ULong(if is_default { u32::MAX } else { wide as u32 }),
        TypeCode::Short => Value::Short(if is_default { i16::MIN } else { wide as i16 }),
        TypeCode::UShort => Value::UShort(if is_default { u16::MAX } else { wide as u16 }),
        TypeCode::Char => Value::Char(if is_default { 0xFE } else { wide as u8 }),
        TypeCode::Octet => Value::Octet(if is_default { 0xFE } else { wide as u8 }),
        TypeCode::Enum { members, .. } if is_default => {
            // The first enumerator no case names. `golden/29`'s `Tint` is
            // `case RED: .. case GREEN: default: ..` — its default member,
            // now the LAST case with no label, read as ordinal 0 was RED,
            // which selects `warm` and not the branch under witness; the
            // value could not be rendered and the sweep silently lost the
            // union, its any and its call (170 -> 168 values, 137 -> 136
            // calls) until this arm existed.
            let taken = |i: usize| cases.iter().any(|c| c.label == (i as u32).to_be_bytes());
            Value::Enum(
                (0..members.len()).find(|i| !taken(*i)).and_then(|i| members.get(i))?.clone(),
            )
        }
        TypeCode::Enum { members, .. } => {
            Value::Enum(members.get(wide as usize).or_else(|| members.first())?.clone())
        }
        _ => return None,
    })
}

fn sample_ior() -> Ior {
    Ior {
        type_id: "IDL:oracle/Peer:1.0".to_owned(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "127.0.0.1".to_owned(),
            port: 4242,
            object_key: b"witness".to_vec(),
            components: Vec::new(),
        }],
    }
}

/// The §4.5 criterion: the same value, encoded before and after the trip.
fn same_bytes(tc: &TypeCode, before: &Value, after: &Value) -> Result<(), String> {
    for endian in [Endian::Big, Endian::Little] {
        let mut a = Encoder::new(endian);
        encode(&mut a, tc, before).map_err(|e| format!("encoding the original: {e}"))?;
        let mut b = Encoder::new(endian);
        encode(&mut b, tc, after).map_err(|e| format!("encoding what came back: {e}"))?;
        let (a, b) =
            (a.finish().map_err(|e| e.to_string())?, b.finish().map_err(|e| e.to_string())?);
        if a != b {
            return Err(format!("{endian:?}: {a:02x?} != {b:02x?}"));
        }
    }
    Ok(())
}

fn json_obj<const N: usize>(fields: [(&str, Json); N]) -> Json {
    Json::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// The driver: it imports the generated package and does exactly what a caller
/// would, then reports what it saw. Fixed text, never generated — a test the
/// generator writes for itself proves nothing.
const DRIVER: &str = r#"
"""Executes a generated package against a plan and reports what it produced."""
import ast, importlib, json, sys, traceback

plan = json.load(open(sys.argv[1]))
sys.path.insert(0, plan["path"])
package = importlib.import_module(plan["package"])
_rt = importlib.import_module(plan["package"] + "._rt")

def desc(text):
    return ast.literal_eval(text)

out = {"values": [], "calls": [], "errors": []}

# A failed item still occupies its position. Dropping one shifts every later
# item against the wrong expectation, which invents failures — the same class
# of harm as an unmeasured check reported as a pass.
for item in plan["values"]:
    try:
        d = desc(item["desc"])
        value = _rt.from_json(d, item["json"])
        out["values"].append({"id": item["id"], "json": _rt.to_json(d, value),
                              "repr": repr(value)})
    except Exception as e:
        out["values"].append({"id": item["id"], "failed": "%s: %s" % (type(e).__name__, e)})
        out["errors"].append("%s: %s: %s" % (item["id"], type(e).__name__, e))

for item in plan["calls"]:
    try:
        module = package
        for part in item["module"]:
            module = getattr(module, part)
        stub = getattr(module, item["class"])(_rt.Loopback([item["reply"]]))
        args = [_rt.from_json(desc(d), j) for d, j in item["args"]]
        result = getattr(stub, item["method"])(*args)
        n = (1 if item["returns"] else 0) + len(item["outs"])
        if n == 0:
            values = []
        elif n == 1:
            values = [result]
        else:
            values = list(result)
        rendered = []
        if item["returns"]:
            rendered.append(_rt.to_json(desc(item["returns"]), values.pop(0)))
        for (name, d) in item["outs"]:
            rendered.append(_rt.to_json(desc(d), values.pop(0)))
        out["calls"].append({
            "id": item["id"], "method": item["method"],
            "request": stub._invoker.requests[0],
            "rendered": rendered,
        })
    except Exception as e:
        out["calls"].append({"id": item["id"], "method": item["method"],
                             "failed": "%s: %s" % (type(e).__name__, e)})
        out["errors"].append("%s.%s: %s: %s"
                             % (item["id"], item["method"], type(e).__name__, e))

json.dump(out, open(sys.argv[2], "w"))
"#;

struct Outcome {
    files: usize,
    values: usize,
    calls: usize,
    failures: Vec<String>,
}

/// Generates, executes and checks one corpus directory in one pass.
fn run_corpus(dir: &str) -> Outcome {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("python-target/{dir}"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("temp dir");

    let mut out = Outcome { files: 0, values: 0, calls: 0, failures: Vec::new() };

    for path in corpus(dir) {
        let stem = path.file_stem().unwrap().to_string_lossy().replace(['-', '.'], "_");
        let package = if stem.starts_with(|c: char| c.is_ascii_digit()) {
            format!("g{stem}")
        } else {
            stem.clone()
        };
        let Some(registry) = load(&path) else { continue };
        out.files += 1;

        let generated = emit_python(&registry, &package);
        let root = tmp.join(&package);
        for (name, source) in &generated.files {
            let target = root.join(name);
            std::fs::create_dir_all(target.parent().unwrap()).expect("mkdir");
            std::fs::write(&target, source).expect("write");
        }

        // ── the plan ────────────────────────────────────────────────────────
        // Never over what the generator said it skipped. A skip is a decision
        // with a reason attached (§4.4, or a type with no AnyJSON form), and an
        // oracle that demanded the item anyway would report the decision as a
        // defect and bury the ones that are real.
        let skipped: Vec<&str> = generated.skipped.iter().map(|(id, _)| id.as_str()).collect();
        let mut handles = LocalReferences::new();
        let mut values = Vec::new();
        let mut expected: Vec<(String, TypeCode, Value)> = Vec::new();
        let mut calls = Vec::new();
        let mut expected_calls: Vec<CallCheck> = Vec::new();

        for id in registry.ids() {
            if skipped.contains(&id.as_str()) {
                continue;
            }
            match registry.get(id) {
                Some(Entry::Type(tc)) => {
                    if descriptor(tc).is_err() {
                        continue;
                    }
                    let Some(v) = witness(tc, &mut Vec::new()) else { continue };
                    // A forward-declared interface has no `("ref", id)` to be
                    // read through — its descriptor is `("objref", id)` and its
                    // Python name is bound to that — so it is measured only
                    // through the `any` below, where the document names it.
                    if !matches!(tc, TypeCode::ObjRef { .. }) {
                        let Ok(j) = anyjson::to_json(tc, &v, &mut handles) else { continue };
                        values.push(json_obj([
                            ("id", Json::String(id.clone())),
                            ("desc", Json::String(format!("(\"ref\", {id:?})"))),
                            ("json", j),
                        ]));
                        expected.push((id.clone(), tc.clone(), v.clone()));
                    }
                    // The same value inside an `any`, so `_t` is this type's
                    // structural form and Python must rebuild it — name, alias
                    // layers, recursion markers and union labels included —
                    // from the class the package declared, or the encoded
                    // TypeCode comes back as different bytes.
                    let carried = Value::Any(Box::new(tc.clone()), Box::new(v));
                    let Ok(j) = anyjson::to_json(&TypeCode::Any, &carried, &mut handles) else {
                        continue;
                    };
                    values.push(json_obj([
                        ("id", Json::String(format!("{id} in an any"))),
                        ("desc", Json::String("\"any\"".into())),
                        ("json", j),
                    ]));
                    expected.push((format!("{id} in an any"), TypeCode::Any, carried));
                }
                Some(Entry::Interface(_)) => {
                    let module: Vec<String> = registry
                        .qualified_name(id)
                        .map(|q| q.split("::").map(str::to_owned).collect())
                        .unwrap_or_default();
                    let (class, module) = match module.split_last() {
                        Some((c, m)) => (c.clone(), m.to_vec()),
                        None => continue,
                    };
                    for (op, sig) in orbweaver_gen::python::client_operations(&registry, id) {
                        let mut args = Vec::new();
                        let mut arg_checks = Vec::new();
                        let mut ok = true;
                        for p in &sig.params {
                            if !matches!(p.direction, ParamDirection::In | ParamDirection::InOut) {
                                continue;
                            }
                            let (Ok(d), Some(v)) =
                                (descriptor(&p.tc), witness(&p.tc, &mut Vec::new()))
                            else {
                                ok = false;
                                break;
                            };
                            let Ok(j) = anyjson::to_json(&p.tc, &v, &mut handles) else {
                                ok = false;
                                break;
                            };
                            args.push(Json::Array(vec![Json::String(d), j]));
                            arg_checks.push((p.name.clone(), p.tc.clone(), v));
                        }
                        if !ok {
                            continue;
                        }

                        let mut rets: Vec<(String, TypeCode, Value)> = Vec::new();
                        let returns = if matches!(sig.returns, TypeCode::Void) || sig.oneway {
                            Json::Null
                        } else {
                            let (Ok(d), Some(v)) =
                                (descriptor(&sig.returns), witness(&sig.returns, &mut Vec::new()))
                            else {
                                continue;
                            };
                            rets.push(("<return>".to_owned(), sig.returns.clone(), v));
                            Json::String(d)
                        };
                        let mut outs = Vec::new();
                        let mut outputs = BTreeMap::new();
                        if !sig.oneway {
                            for p in &sig.params {
                                if !matches!(
                                    p.direction,
                                    ParamDirection::Out | ParamDirection::InOut
                                ) {
                                    continue;
                                }
                                let (Ok(d), Some(v)) =
                                    (descriptor(&p.tc), witness(&p.tc, &mut Vec::new()))
                                else {
                                    ok = false;
                                    break;
                                };
                                let Ok(j) = anyjson::to_json(&p.tc, &v, &mut handles) else {
                                    ok = false;
                                    break;
                                };
                                outs.push(Json::Array(vec![
                                    Json::String(p.name.clone()),
                                    Json::String(d),
                                ]));
                                outputs.insert(p.name.clone(), j);
                                rets.push((p.name.clone(), p.tc.clone(), v));
                            }
                        }
                        if !ok {
                            continue;
                        }

                        let reply_returns = rets
                            .first()
                            .filter(|_| !matches!(returns, Json::Null))
                            .map(|(_, tc, v)| anyjson::to_json(tc, v, &mut handles).unwrap())
                            .unwrap_or(Json::Null);
                        calls.push(json_obj([
                            ("id", Json::String(id.clone())),
                            ("class", Json::String(class.clone())),
                            (
                                "module",
                                Json::Array(module.iter().cloned().map(Json::String).collect()),
                            ),
                            // The method a caller reaches for and the name that
                            // travels are not the same string when the IDL
                            // identifier is a Python keyword, and 28-target-
                            // keywords is in the corpus so that this is
                            // measured rather than assumed.
                            ("method", Json::String(orbweaver_gen::python::python_name(&op))),
                            ("wire", Json::String(op.clone())),
                            ("args", Json::Array(args)),
                            ("returns", returns),
                            ("outs", Json::Array(outs)),
                            (
                                "reply",
                                json_obj([(
                                    "ok",
                                    json_obj([
                                        ("returns", reply_returns),
                                        ("outputs", Json::Object(outputs)),
                                    ]),
                                )]),
                            ),
                        ]));
                        expected_calls.push(CallCheck {
                            id: id.clone(),
                            method: op.clone(),
                            oneway: sig.oneway,
                            args: arg_checks,
                            rets,
                        });
                    }
                }
                _ => {}
            }
        }

        if values.is_empty() && calls.is_empty() {
            continue;
        }
        let plan = json_obj([
            ("path", Json::String(tmp.display().to_string())),
            ("package", Json::String(package.clone())),
            ("values", Json::Array(values)),
            ("calls", Json::Array(calls)),
        ]);
        let plan_path = tmp.join(format!("{package}.plan.json"));
        let result_path = tmp.join(format!("{package}.result.json"));
        let driver_path = tmp.join("driver.py");
        std::fs::write(&plan_path, plan.to_string()).expect("plan");
        std::fs::write(&driver_path, DRIVER).expect("driver");

        let run = Command::new("python3")
            .arg(&driver_path)
            .arg(&plan_path)
            .arg(&result_path)
            .output()
            .unwrap_or_else(|e| {
                panic!(
                    "python3 could not be run ({e}). An unmeasured check is a failure, \
                     never a pass: this test executes generated Python and cannot report \
                     anything without an interpreter."
                )
            });
        if !run.status.success() {
            out.failures.push(format!(
                "{package}: the driver failed\n{}",
                String::from_utf8_lossy(&run.stderr)
            ));
            continue;
        }
        let result = std::fs::read_to_string(&result_path).expect("result");
        let result = Json::parse(&result).expect("the driver writes JSON");

        for e in array(&result, "errors") {
            out.failures.push(format!("{package}: {}", e.as_str().unwrap_or("?")));
        }

        // ── values ──────────────────────────────────────────────────────────
        let got = array(&result, "values");
        if got.len() != expected.len() {
            out.failures.push(format!(
                "{package}: {} value(s) went in and {} came back",
                expected.len(),
                got.len()
            ));
        }
        for (item, (id, tc, before)) in got.iter().zip(&expected) {
            out.values += 1;
            if item.get("failed").is_some() {
                continue; // already reported, once, through the driver's errors
            }
            let Some(j) = item.get("json") else {
                out.failures.push(format!("{package} {id}: no json came back"));
                continue;
            };
            match anyjson::from_json(tc, j, &handles) {
                Ok(after) => {
                    if let Err(why) = same_bytes(tc, before, &after) {
                        out.failures.push(format!("{package} {id}: {why}"));
                    }
                }
                Err(e) => out.failures.push(format!(
                    "{package} {id}: what Python produced is not a value of this type: {e}\n  {j}"
                )),
            }
        }

        // ── calls ───────────────────────────────────────────────────────────
        let got = array(&result, "calls");
        if got.len() != expected_calls.len() {
            out.failures.push(format!(
                "{package}: {} call(s) went in and {} came back",
                expected_calls.len(),
                got.len()
            ));
        }
        for (item, check) in got.iter().zip(&expected_calls) {
            out.calls += 1;
            if item.get("failed").is_some() {
                continue;
            }
            let request = item.get("request");
            let where_ = format!("{package} {}.{}", check.id, check.method);
            match request.and_then(|r| r.get("op")).and_then(Json::as_str) {
                Some(sent) if sent == check.method => {}
                Some(sent) => out.failures.push(format!(
                    "{where_}: the stub sent the operation name {sent:?}, the contract says \
                     {:?} — an escaped Python method must still name the IDL operation",
                    check.method
                )),
                None => out.failures.push(format!("{where_}: the request named no operation")),
            }
            match request.and_then(|r| r.get("args")) {
                Some(Json::Object(sent)) => {
                    if sent.len() != check.args.len() {
                        out.failures.push(format!(
                            "{where_}: the stub sent {} argument(s), the contract declares {}",
                            sent.len(),
                            check.args.len()
                        ));
                    }
                    for (name, tc, want) in &check.args {
                        let Some(j) = sent.get(name) else {
                            out.failures.push(format!("{where_}: no argument {name:?} was sent"));
                            continue;
                        };
                        match anyjson::from_json(tc, j, &handles) {
                            Ok(got) => {
                                if let Err(why) = same_bytes(tc, want, &got) {
                                    out.failures.push(format!("{where_} argument {name}: {why}"));
                                }
                            }
                            Err(e) => {
                                out.failures.push(format!("{where_} argument {name}: {e}\n  {j}"))
                            }
                        }
                    }
                }
                _ => out.failures.push(format!("{where_}: the request carried no args object")),
            }
            let oneway_sent = request
                .and_then(|r| r.get("oneway"))
                .map(|j| matches!(j, Json::Bool(true)))
                .unwrap_or(false);
            if oneway_sent != check.oneway {
                out.failures.push(format!(
                    "{where_}: oneway is {} in the contract and {oneway_sent} in the request",
                    check.oneway
                ));
            }
            if check.oneway {
                continue;
            }
            let rendered = array(item, "rendered");
            if rendered.len() != check.rets.len() {
                out.failures.push(format!(
                    "{where_}: the reply carries {} value(s), the stub answered {}",
                    check.rets.len(),
                    rendered.len()
                ));
                continue;
            }
            for (j, (name, tc, want)) in rendered.iter().zip(&check.rets) {
                match anyjson::from_json(tc, j, &handles) {
                    Ok(got) => {
                        if let Err(why) = same_bytes(tc, want, &got) {
                            out.failures.push(format!("{where_} reply {name}: {why}"));
                        }
                    }
                    Err(e) => out.failures.push(format!("{where_} reply {name}: {e}\n  {j}")),
                }
            }
        }
    }
    // Printed rather than only asserted: what an oracle measured is the half of
    // its verdict that a passing run otherwise throws away, and a check that
    // quietly stopped measuring anything would still be green.
    println!(
        "{dir}: {} file(s), {} value(s) and {} call(s) crossed to Python and back, \
         {} divergence(s)",
        out.files,
        out.values,
        out.calls,
        out.failures.len()
    );
    out
}

struct CallCheck {
    id: String,
    method: String,
    oneway: bool,
    args: Vec<(String, TypeCode, Value)>,
    rets: Vec<(String, TypeCode, Value)>,
}

fn array<'a>(j: &'a Json, key: &str) -> &'a [Json] {
    match j.get(key) {
        Some(Json::Array(items)) => items,
        _ => &[],
    }
}

#[test]
fn the_golden_corpus_crosses_to_python_and_back_unchanged() {
    let out = run_corpus("corpus/golden");
    assert!(out.files >= 20, "the corpus shrank: {} file(s)", out.files);
    // Pinned at what was measured on 2026-08-19, when every registered type
    // gained its twin inside an `any` (78/132 before, 158/132 after). A floor,
    // not an equality: the corpus growing should raise it, and nothing else
    // should move it — a drop is the oracle quietly measuring less, which is
    // the failure a passing run otherwise throws away.
    assert!(
        out.values >= 158 && out.calls >= 132,
        "the oracle measures less than it did: {} value(s), {} call(s)",
        out.values,
        out.calls
    );
    assert!(
        out.failures.is_empty(),
        "{} of {} value(s) and {} call(s) diverged:\n{}",
        out.failures.len(),
        out.values,
        out.calls,
        out.failures.join("\n")
    );
}

#[test]
fn the_services_corpus_crosses_to_python_and_back_unchanged() {
    let out = run_corpus("corpus/services");
    assert!(out.files > 0, "corpus/services must not be empty");
    // 35/46 before the `any` twins, 70/46 after (2026-08-19); a floor, as above.
    assert!(
        out.values >= 70 && out.calls >= 46,
        "the oracle measures less than it did: {} value(s), {} call(s)",
        out.values,
        out.calls
    );
    assert!(out.failures.is_empty(), "{}", out.failures.join("\n"));
}

/// Runs `script` against a package generated from `idl`, and returns what it
/// printed. The script is executed, not inspected: a stub that raises the
/// wrong class, or does not raise at all, fails here.
fn run_script(name: &str, idl: &str, script: &str) -> String {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("python-target/{name}"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("temp dir");

    let spec = orbweaver_idl::parse(idl).expect("parses");
    let mut registry = Registry::new();
    registry.load(&spec).expect("loads");
    let generated = emit_python(&registry, name);
    for (file, source) in &generated.files {
        let target = tmp.join(name).join(file);
        std::fs::create_dir_all(target.parent().unwrap()).expect("mkdir");
        std::fs::write(&target, source).expect("write");
    }
    let script_path = tmp.join("case.py");
    std::fs::write(&script_path, script).expect("script");

    let run = Command::new("python3")
        .arg(&script_path)
        .arg(&tmp)
        .output()
        .expect("python3 is required: an unmeasured check is a failure, never a pass");
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    assert!(
        run.status.success(),
        "the case failed\n--- stdout ---\n{stdout}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    stdout
}

/// The reply paths the corpus sweep cannot reach: a raised user exception, a
/// system exception, and a `oneway` that has no reply at all.
///
/// The sweep drives every operation with a successful reply, because that is
/// the shape it can synthesise from a `TypeCode`. A failure reply is not a
/// value of any declared type, and it is the half a client meets on its worst
/// day, so it is driven here by hand.
#[test]
fn a_generated_stub_raises_what_the_reply_names() {
    let out = run_script(
        "faults",
        "module f {\n\
           exception Insufficient { long shortfall; string ledger; };\n\
           interface Vault {\n\
             void draw(in long amount) raises (Insufficient);\n\
             oneway void note(in string what);\n\
           };\n\
         };",
        r#"
import sys
sys.path.insert(0, sys.argv[1])
from faults import _rt
from faults import f

# A declared exception comes back as the generated class, with its members.
vault = f.Vault(_rt.Loopback([{"user_exception": {
    "id": "IDL:f/Insufficient:1.0",
    "members": {"shortfall": 25, "ledger": "main"}}}]))
try:
    vault.draw(100)
    raise SystemExit("a raised exception did not reach the caller")
except f.Insufficient as e:
    assert e.shortfall == 25 and e.ledger == "main", repr(e)
    print("user exception:", repr(e))

# A system exception is not a user exception and must not be catchable as one.
vault = f.Vault(_rt.Loopback([{"system_exception": {
    "id": "IDL:omg.org/CORBA/NO_PERMISSION:1.0", "minor": 1330446337, "completed": 1}}]))
try:
    vault.draw(1)
    raise SystemExit("a system exception did not reach the caller")
except f.Insufficient:
    raise SystemExit("a system exception was caught as a user exception")
except _rt.SystemException as e:
    assert e.id.endswith("NO_PERMISSION:1.0"), e.id
    assert e.completed == 1, e.completed
    print("system exception:", e.id, e.minor, e.completed)

# An exception id the caller was never built against cannot be constructed, so
# it becomes UNKNOWN with OMG minor 1 — the standard mapping, and better than
# a plausible wrong class.
vault = f.Vault(_rt.Loopback([{"user_exception": {"id": "IDL:elsewhere/Odd:1.0"}}]))
try:
    vault.draw(1)
    raise SystemExit("an unknown exception did not reach the caller")
except _rt.SystemException as e:
    assert e.id.endswith("UNKNOWN:1.0") and e.minor == 0x4f4d0001, (e.id, e.minor)
    print("unknown user exception:", e.id, hex(e.minor))

# A oneway sends `oneway` and answers None without reading a reply.
loop = _rt.Loopback()
assert f.Vault(loop).note("done") is None
assert loop.requests[0]["oneway"] is True, loop.requests
assert loop.requests[0]["op"] == "note", loop.requests
print("oneway:", loop.requests[0])
"#,
    );
    assert!(out.contains("user exception: Insufficient(shortfall=25, ledger='main')"), "{out}");
    assert!(out.contains("system exception: IDL:omg.org/CORBA/NO_PERMISSION:1.0"), "{out}");
    assert!(out.contains("unknown user exception:"), "{out}");
    assert!(out.contains("oneway:"), "{out}");
}

/// An `any` whose type no generated package declares: the structural form is
/// all Python has, and it must build the type from it and hand back the same
/// bytes (D008 — a self-describing type needs no prior copy at the reader).
///
/// The corpus sweep reaches this path only for a struct; the kinds a peer's
/// document can also carry — an enum, a union with a default, a typedef, a
/// recursion marker, a member whose name is a Python keyword — are driven
/// here. The document is written by the Rust mapping, never by hand, so the
/// spelling Python is held to is the spelling that actually crosses.
#[test]
fn an_any_describing_a_type_the_package_never_declared_is_read_and_reproduced() {
    use orbweaver_giop::typecode::UnionCase;
    let reading_id = "IDL:elsewhere/Reading:1.0";
    let reading = TypeCode::Struct {
        id: reading_id.into(),
        name: "Reading".into(),
        members: vec![
            Member {
                name: "unit".into(),
                tc: TypeCode::Enum {
                    id: "IDL:elsewhere/Unit:1.0".into(),
                    name: "Unit".into(),
                    members: vec!["C".into(), "F".into()],
                },
            },
            Member {
                name: "value".into(),
                tc: TypeCode::Union {
                    id: "IDL:elsewhere/Val:1.0".into(),
                    name: "Val".into(),
                    discriminator: Box::new(TypeCode::Long),
                    default_index: 2,
                    cases: vec![
                        UnionCase {
                            label: 1i32.to_be_bytes().to_vec(),
                            name: "i".into(),
                            tc: TypeCode::Long,
                        },
                        UnionCase {
                            label: 2i32.to_be_bytes().to_vec(),
                            name: "i".into(),
                            tc: TypeCode::Long,
                        },
                        UnionCase { label: Vec::new(), name: "s".into(), tc: TypeCode::String(0) },
                    ],
                },
            },
            Member {
                name: "kids".into(),
                tc: TypeCode::Alias {
                    id: "IDL:elsewhere/Readings:1.0".into(),
                    name: "Readings".into(),
                    aliased: Box::new(TypeCode::Sequence {
                        element: Box::new(TypeCode::Recursive(reading_id.into())),
                        bound: 0,
                    }),
                },
            },
            Member { name: "lambda".into(), tc: TypeCode::Boolean },
            // A branch that is both labelled and `default:` — `case 1: case 2:
            // default: boolean loud;` — is one case per label in the registry
            // with the default a labelless case of its own where `default:`
            // was written, `default_index` on it (omniidl's member list). The
            // synthesised class has to keep the labels on its default branch
            // AND put the default member back in its slot, or the TypeCode it
            // writes back has a different member list from this one
            // (`corpus/golden/29` holds the generated classes to the same).
            Member {
                name: "mode".into(),
                tc: TypeCode::Union {
                    id: "IDL:elsewhere/Mode:1.0".into(),
                    name: "Mode".into(),
                    discriminator: Box::new(TypeCode::Short),
                    default_index: 2,
                    cases: vec![
                        UnionCase {
                            label: 1i16.to_be_bytes().to_vec(),
                            name: "loud".into(),
                            tc: TypeCode::Boolean,
                        },
                        UnionCase {
                            label: 2i16.to_be_bytes().to_vec(),
                            name: "loud".into(),
                            tc: TypeCode::Boolean,
                        },
                        UnionCase { label: Vec::new(), name: "loud".into(), tc: TypeCode::Boolean },
                        UnionCase {
                            label: 3i16.to_be_bytes().to_vec(),
                            name: "level".into(),
                            tc: TypeCode::Octet,
                        },
                    ],
                },
            },
        ],
    };
    // `kids` carries a Reading, so the value *under* the recursion marker
    // crosses and not only the marker. Until 2026-08-19 it had to be empty:
    // `anyjson::to_json` resolved aliases and nothing else, so a non-empty
    // `sequence<Reading>` inside `Reading` failed on the Rust side before
    // Python was reached ("... is not a value of IDL:elsewhere/Reading:1.0"),
    // and this test held Python to the marker in the TypeCode while the value
    // beneath it stayed a gap in `orbweaver-dynamic`. The gap closed with the
    // marker now resolved against the enclosing type on both sides of the
    // mapping, and this is the document that proves it end to end.
    let leaf = Value::Struct(vec![
        ("unit".into(), Value::Enum("C".into())),
        (
            "value".into(),
            Value::Union {
                discriminator: Box::new(Value::Long(2)),
                value: Some(Box::new(Value::Long(22))),
            },
        ),
        ("kids".into(), Value::List(vec![])),
        ("lambda".into(), Value::Bool(false)),
        (
            "mode".into(),
            Value::Union {
                discriminator: Box::new(Value::Short(3)),
                value: Some(Box::new(Value::Octet(200))),
            },
        ),
    ]);
    let root = Value::Struct(vec![
        ("unit".into(), Value::Enum("F".into())),
        (
            "value".into(),
            Value::Union {
                discriminator: Box::new(Value::Long(9)),
                value: Some(Box::new(Value::String("nine".into()))),
            },
        ),
        ("kids".into(), Value::List(vec![leaf])),
        ("lambda".into(), Value::Bool(true)),
        // 5 names no label: the labelled default branch, selected by default.
        (
            "mode".into(),
            Value::Union {
                discriminator: Box::new(Value::Short(5)),
                value: Some(Box::new(Value::Bool(true))),
            },
        ),
    ]);
    let carried = Value::Any(Box::new(reading), Box::new(root));
    let mut handles = LocalReferences::new();
    let doc = anyjson::to_json(&TypeCode::Any, &carried, &mut handles).expect("Rust writes it");

    let script = r#"
import json, sys
sys.path.insert(0, sys.argv[1])
from undeclared import _rt

doc = json.loads(r'''__DOC__''')
desc, v = _rt.from_json("any", doc)
assert desc == ("ref", "IDL:elsewhere/Reading:1.0"), desc
assert type(v).__name__ == "Reading" and isinstance(v, _rt.Struct), v
assert v.unit == _rt.EnumItem("F", 1, "IDL:elsewhere/Unit:1.0"), v.unit
assert v.value._d == 9 and v.value._v == "nine", repr(v.value)
assert v._lambda is True, "a keyword member is escaped as the generator escapes it"
assert len(v.kids) == 1 and type(v.kids[0]).__name__ == "Reading", repr(v.kids)
assert v.kids[0].value._d == 2 and v.kids[0].value._v == 22 and v.kids[0].kids == [], repr(v.kids[0])
assert v.mode._d == 5 and v.mode._v is True and v.mode._branch("loud") is True, repr(v.mode)
assert v.kids[0].mode._d == 3 and v.kids[0].mode._branch("level") == 200, repr(v.kids[0].mode)
print("read:", repr(v))

# The type it described is now a type this package can speak, recursion
# included: a Reading inside a Reading marshals through the synthesised class,
# and the reference mapping reads the result back (below).
Reading = _rt.TYPES["IDL:elsewhere/Reading:1.0"]
Unit = _rt.TYPES["IDL:elsewhere/Unit:1.0"]
Val = _rt.TYPES["IDL:elsewhere/Val:1.0"]
Mode = _rt.TYPES["IDL:elsewhere/Mode:1.0"]
nested = _rt.to_json("any", (desc, Reading(Unit.C, Val(1, 1), [Reading(Unit.F, Val(2, 2), [], False, Mode(3, 7))], False, Mode(2, False))))
assert nested["_v"]["kids"][0]["value"] == {"_d": 2, "_v": 2}, nested
assert Val(3, "three")._d == 3, "the default branch"
# The labelled default keeps its labels: 1 and 2 select it by label, anything
# else by default, and setting the branch picks its first label.
assert Mode._idl_cases[Mode._idl_default][0] == (1, 2), Mode._idl_cases
assert Mode._idl_default_slot == 2, "the default member sits after both labels, where `default:` was written"
assert Mode(1, True)._branch("loud") is True and Mode(9, False)._branch("loud") is False, "by label and by default"
m = Mode(3, 0); m._set_branch("loud", True)
assert m._d == 1 and m._v is True, repr(m)
again = Reading(Unit.C, v.value, [], False, Mode(1, True))
assert _rt.TypeCode.of(("ref", "IDL:elsewhere/Reading:1.0")).form == doc["_t"], "of()"
assert _rt.TypeCode(doc["_t"]).descriptor() == desc, "descriptor()"

open(sys.argv[1] + "/back.json", "w").write(json.dumps(_rt.to_json("any", (desc, v))))
open(sys.argv[1] + "/again.json", "w").write(json.dumps(_rt.to_json("any", (desc, again))))
print("wrote it back")
"#;
    let text = doc.to_string();
    assert!(!text.contains("'''"), "the document cannot be embedded verbatim: {text}");
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join("python-target/undeclared");
    let out = run_script(
        "undeclared",
        "module undeclared_m { interface Nothing {}; };",
        &script.replace("__DOC__", &text),
    );
    assert!(out.contains("read: Reading("), "{out}");
    assert!(out.contains("wrote it back"), "{out}");

    let back = std::fs::read_to_string(tmp.join("back.json")).expect("back");
    let back = Json::parse(&back).expect("json");
    let after = anyjson::from_json(&TypeCode::Any, &back, &handles)
        .unwrap_or_else(|e| panic!("what Python produced is not an any: {e}\n  {back}"));
    same_bytes(&TypeCode::Any, &carried, &after).unwrap_or_else(|why| panic!("{why}\n  {back}"));

    // A value built from the synthesised class, not merely relayed.
    let again = std::fs::read_to_string(tmp.join("again.json")).expect("again");
    let again = Json::parse(&again).expect("json");
    let Value::Any(tc, _) = anyjson::from_json(&TypeCode::Any, &again, &handles)
        .expect("a value of the described type")
    else {
        panic!("not an any");
    };
    let Value::Any(want, _) = &carried else { unreachable!() };
    assert_eq!(&tc, want, "the rebuilt TypeCode is the one that was described");
}

/// §4.4's two deferrals, peer-fed: the **description** is read, synthesised and
/// written back byte-identically, and an **instance** is refused — on both
/// sides of the mapping, with the section named.
///
/// The asymmetry is the point and is deliberate. A `valuetype`'s TypeCode is
/// `tk_value` (29) and an abstract interface's is `tk_abstract_interface` (32);
/// both are values the v1 wire carries, so a document describing one has to be
/// readable by a reader that will never be able to instantiate it. Until this
/// existed the Python runtime had no `_desc_of` arm for either form, so a
/// peer-fed document carrying one was refused rather than read — and a *struct*
/// with a valuetype member was unreadable in its entirety, for a member whose
/// value was never going to be asked for.
///
/// What must never become symmetric: reading the description must not become
/// permission to marshal the value. Both refusals are asserted here, in both
/// directions, in both implementations, and they are held to the same sentence.
#[test]
fn a_peer_fed_deferral_is_described_read_and_written_back_but_never_instantiated() {
    // A recursive valuetype as well as the sweep's two: `valuetype Node {
    // public sequence<Node> kids; };` is where the indirection marker sits
    // inside a kind Python had no class for, and it is the case that would
    // have gone quietly wrong had the synthesised class been registered after
    // its members were read rather than before.
    let node_id = "IDL:elsewhere/Node:1.0";
    let node = TypeCode::Value {
        id: node_id.into(),
        name: "Node".into(),
        modifier: 1,
        base: None,
        members: vec![
            ValueMember { name: "tag".into(), tc: TypeCode::String(0), visibility: 1 },
            ValueMember {
                name: "kids".into(),
                tc: TypeCode::Sequence {
                    element: Box::new(TypeCode::Recursive(node_id.into())),
                    bound: 0,
                },
                visibility: 0,
            },
        ],
    };
    let cases: Vec<TypeCode> = vec![deferred_value(), deferred_abstract(), node];

    let mut handles = LocalReferences::new();
    let carried = Value::List(
        cases
            .iter()
            .map(|tc| {
                Value::Any(
                    Box::new(TypeCode::TypeCode),
                    Box::new(Value::TypeCode(Box::new(tc.clone()))),
                )
            })
            .collect(),
    );
    let carrier = TypeCode::Sequence { element: Box::new(TypeCode::Any), bound: 0 };
    let doc = anyjson::to_json(&carrier, &carried, &mut handles).expect("Rust writes it");

    // ── the Rust half of the refusal ────────────────────────────────────────
    // Stated here rather than only in `orbweaver-dynamic`'s own tests because
    // what is being pinned is that the two implementations refuse the *same*
    // thing — an instance, never a description — and that is a fact about the
    // pair, which neither crate's tests can hold on its own.
    //
    // The sentences are collected rather than only pattern-matched, and handed
    // to the script below to be compared for **equality** with what `_rt.py`
    // raises. Python cannot import a Rust constant, so the only thing holding
    // `_DEFERRED` to `orbweaver_dynamic`'s wording is this comparison; asserting
    // that both merely contain "§4.4" would let the halves drift into two
    // different explanations of the same boundary, which is the state the
    // AnyJSON layer was in until 2026-08-21.
    let mut want = Vec::new();
    for tc in &cases {
        let mut e = Encoder::new(Endian::Little);
        let why = orbweaver_dynamic::encode(&mut e, tc, &Value::Struct(Vec::new()))
            .expect_err("an instance of a deferred type has no encoding");
        assert!(why.message.contains("§4.4"), "Rust encode: {why}");
        let mut d = orbweaver_cdr::Decoder::new(&[0u8; 16], Endian::Little);
        let why = orbweaver_dynamic::decode(&mut d, tc)
            .expect_err("an instance of a deferred type cannot be read");
        assert!(why.message.contains("§4.4"), "Rust decode: {why}");
        let read = why.message.clone();

        // The AnyJSON layer, which is the one `_rt.to_json`/`_rt.from_json` are
        // the second implementation of, in both directions.
        let mut h = LocalReferences::new();
        let out = anyjson::to_json(tc, &Value::Struct(Vec::new()), &mut h)
            .expect_err("an instance of a deferred type has no AnyJSON form");
        let back = anyjson::from_json(tc, &Json::parse("{}").expect("document"), &h)
            .expect_err("an instance of a deferred type has no AnyJSON form");
        assert_eq!(out.message, read, "Rust to_json vs decode");
        assert_eq!(back.message, read, "Rust from_json vs decode");
        want.push(Json::String(read));
    }
    let want = Json::Array(want).to_string();

    let script = r#"
import json, sys
sys.path.insert(0, sys.argv[1])
from deferred import _rt

docs = json.loads(r'''__DOC__''')
want = json.loads(r'''__WANT__''')
priced, describable, node = [_rt.from_json("any", d)[1] for d in docs]

# ── read ────────────────────────────────────────────────────────────────────
# A `tk_value` describes itself down to the modifier, the concrete base and
# each member's visibility, and every one of those is a byte a peer compares.
d = priced.descriptor()
assert d == ("ref", "IDL:witness/Priced:1.0"), d
Priced = _rt.TYPES["IDL:witness/Priced:1.0"]
Money = _rt.TYPES["IDL:witness/Money:1.0"]
assert issubclass(Priced, _rt.ValueType) and issubclass(Money, _rt.ValueType)
assert not issubclass(Priced, _rt.Struct), "a valuetype is not a struct that happens to defer"
assert Priced._idl_base == ("ref", "IDL:witness/Money:1.0"), Priced._idl_base
assert Money._idl_base is None, "no concrete base is tk_null, not a base of type null"
assert Money._idl_members == (("currency", ("string", 3), 1), ("amount", "longlong", 0)), \
    Money._idl_members
assert _rt.NAMES["IDL:witness/Money:1.0"] == "Money"

da = describable.descriptor()
assert da == ("abstract_interface", "IDL:witness/Describable:1.0"), da
assert _rt.NAMES["IDL:witness/Describable:1.0"] == "Describable"

# A valuetype whose member re-enters it: the class has to be registered before
# its members are read, or the marker resolves to nothing.
dn = node.descriptor()
Node = _rt.TYPES["IDL:elsewhere/Node:1.0"]
assert Node._idl_modifier == 1, "the ValueModifier is carried, not assumed"
assert Node._idl_members[1] == ("kids", ("seq", ("ref", "IDL:elsewhere/Node:1.0"), 0), 0), \
    Node._idl_members
print("read:", Priced.__name__, Money.__name__, Node.__name__, da[0])

# ── written back ────────────────────────────────────────────────────────────
for tc, desc in ((priced, d), (describable, da), (node, dn)):
    assert _rt.TypeCode.of(desc).form == tc.form, (desc, _rt.TypeCode.of(desc).form, tc.form)

# ── and never instantiated ──────────────────────────────────────────────────
# Both directions, both kinds, and the two sentences are the same sentence —
# not "the same shape", the same string as the Rust CDR and AnyJSON layers
# produce for these very TypeCodes, which is what `want` carries in.
seen = []
for (desc, what), expected in zip(
        ((d, "valuetype"), (da, "abstract interface"), (dn, "valuetype")), want):
    for call in (lambda: _rt.to_json(desc, object()), lambda: _rt.from_json(desc, {})):
        try:
            call()
            raise SystemExit("a value of a deferred type was marshalled: %r" % (desc,))
        except _rt.MarshalError as e:
            assert "docs/PLAN.md §4.4" in e.message, e.message
            assert e.message.startswith(what + " "), e.message
            assert e.message == expected, (e.message, expected)
            seen.append(e.message)
assert len(set(seen)) == 3, seen
try:
    Priced()
    raise SystemExit("a valuetype was constructed")
except _rt.MarshalError as e:
    assert "§4.4" in e.message, e.message

# The shape a peer actually sends when it sends the thing we cannot read: an
# `any` whose `_t` IS the deferred type. Refused at the value, by name — not
# at the type, which is what makes the description still readable.
for form in (priced.form, describable.form):
    try:
        _rt.from_json("any", {"_t": form, "_v": {}})
        raise SystemExit("an instance arrived and was accepted")
    except _rt.MarshalError as e:
        assert "docs/PLAN.md §4.4" in e.message, e.message
print("refused:", seen[0])

open(sys.argv[1] + "/back.json", "w").write(json.dumps(
    [_rt.to_json("any", ("typecode", _rt.TypeCode.of(x))) for x in (d, da, dn)]))
print("wrote it back")
"#;
    let text = doc.to_string();
    assert!(!text.contains("'''"), "the document cannot be embedded verbatim: {text}");
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR")).join("python-target/deferred");
    assert!(!want.contains("'''"), "the sentences cannot be embedded verbatim: {want}");
    let out = run_script(
        "deferred",
        "module deferred_m { interface Nothing {}; };",
        &script.replace("__DOC__", &text).replace("__WANT__", &want),
    );
    assert!(out.contains("read: Priced Money Node abstract_interface"), "{out}");
    assert!(out.contains("refused:"), "{out}");
    assert!(out.contains("wrote it back"), "{out}");

    // The §4.5 criterion, on what Python rebuilt from the descriptor rather
    // than on what it relayed: the TypeCode has to come back the same value
    // and the same bytes, in both byte orders.
    let back = std::fs::read_to_string(tmp.join("back.json")).expect("back");
    let back = Json::parse(&back).expect("json");
    let after = anyjson::from_json(&carrier, &back, &handles)
        .unwrap_or_else(|e| panic!("what Python produced is not a sequence<any>: {e}\n  {back}"));
    same_bytes(&carrier, &carried, &after).unwrap_or_else(|why| panic!("{why}\n  {back}"));
    let Value::List(items) = &after else { panic!("not a list") };
    for (item, want) in items.iter().zip(&cases) {
        let Value::Any(_, v) = item else { panic!("not an any") };
        let Value::TypeCode(got) = &**v else { panic!("not a TypeCode") };
        assert_eq!(&**got, want, "the TypeCode Python rebuilt is the one it was given");
    }
}

/// The two families this runtime refuses at the **type form** rather than at
/// the value — `fixed` and `native` — each held to the Rust sentence for its
/// own boundary, by equality, across the crate boundary.
///
/// The test above covers the two §4.4 constructs Python has a descriptor for. A
/// `fixed` and a `native` have none, so `_desc_of` is where a peer-fed document
/// carrying one stops, and each was writing its own sentence there:
///
/// ```text
/// fixed   "fixed<9,2> is deferred at wire level (§4.4)"    (a fourth wording
///                                                           of §4.4's, in the
///                                                           layer a peer meets)
/// native  "no AnyJSON value form for a 'native' type"      (names neither the
///                                                           construct nor any
///                                                           boundary)
/// ```
///
/// Both now come from a constant — `_DEFERRED` and `_UNMARSHALLABLE` — and the
/// assertion is **equality** with what the Rust layers raise for the same
/// TypeCode, because Python cannot import a Rust constant and a substring check
/// would let the halves drift into two explanations of one boundary.
///
/// # Why the two constants must not become one
///
/// `native X;` is not deferred: §4.4's three have a wire form the specification
/// defines and this version has not implemented, and a native has none to
/// implement in any version. So the two sentences have to read *differently*,
/// and the two ways a reader is told something false are asserted here in
/// Python as well as in Rust — "yet" promises a version that will never come,
/// and §4.4's deferral claim sends the reader to a plan entry that does not
/// name the construct. Both were live in shipped code on 2026-08-21.
#[test]
fn a_peer_fed_form_with_no_descriptor_is_refused_in_the_rust_layers_words() {
    let cases: Vec<(&str, TypeCode)> = vec![
        ("fixed<9,2>", TypeCode::Fixed { digits: 9, scale: 2 }),
        (
            "native Handle",
            TypeCode::Native { id: "IDL:witness/Handle:1.0".into(), name: "Handle".into() },
        ),
    ];

    let mut forms = Vec::new();
    let mut wants = Vec::new();
    for (what, tc) in &cases {
        // What Rust says, taken from the code rather than typed here. Both
        // AnyJSON directions, and — for the native, which the CDR path also has
        // arms for — both CDR directions too, so a Python string equal to one
        // of them is equal to all of them. `fixed` has no CDR arm at all, which
        // `orbweaver-dynamic`'s `the_cdr_path_does_not_yet_name_the_section_for_fixed`
        // records as a measured gap rather than a wish.
        let mut h = LocalReferences::new();
        let mut said = vec![
            anyjson::to_json(tc, &Value::Struct(Vec::new()), &mut h)
                .expect_err("an instance has no AnyJSON form")
                .message,
            anyjson::from_json(tc, &Json::parse("{}").expect("document"), &h)
                .expect_err("an instance has no AnyJSON form")
                .message,
        ];
        if matches!(tc, TypeCode::Native { .. }) {
            said.push(
                encode(&mut Encoder::new(Endian::Little), tc, &Value::Struct(Vec::new()))
                    .expect_err("an instance has no encoding")
                    .message,
            );
            said.push(
                orbweaver_dynamic::decode(
                    &mut orbweaver_cdr::Decoder::new(&[0u8; 16], Endian::Little),
                    tc,
                )
                .expect_err("an instance cannot be read")
                .message,
            );
        }
        said.dedup();
        assert_eq!(said.len(), 1, "{what}: the Rust layers disagree with each other: {said:?}");
        let want = said.remove(0);
        assert!(want.starts_with(what), "{want}");

        // The form a peer actually sends. `orbweaver_dynamic::tc_to_json`
        // writes both structurally, which is what makes `_desc_of`'s arms
        // reachable at all.
        let form = anyjson::tc_to_json(tc).to_string();
        assert!(!form.contains("'''"), "{form}");
        forms.push(Json::parse(&form).expect("a form"));
        wants.push(Json::String(want));
    }

    // The distinction, asserted on the Rust side before it is handed over: the
    // deferred sentence and the never sentence are not one string, and the
    // never one carries neither falsehood.
    let Json::String(fixed_want) = &wants[0] else { unreachable!() };
    let Json::String(native_want) = &wants[1] else { unreachable!() };
    assert!(
        fixed_want.contains("is not marshalled by the v1 wire (docs/PLAN.md §4.4)"),
        "{fixed_want}"
    );
    assert!(!native_want.contains("yet"), "{native_want}");
    assert!(
        !native_want.contains("is not marshalled by the v1 wire (docs/PLAN.md §4.4)"),
        "a native must not carry §4.4's deferral claim: {native_want}"
    );

    let forms = Json::Array(forms).to_string();
    let wants = Json::Array(wants).to_string();
    assert!(!forms.contains("'''") && !wants.contains("'''"), "{forms} {wants}");

    let script = r#"
import json, sys
sys.path.insert(0, sys.argv[1])
from nodesc import _rt

forms = json.loads(r'''__FORMS__''')
wants = json.loads(r'''__WANTS__''')
assert [f["kind"] for f in forms] == ["fixed", "native"], forms

# Both entry points a peer-fed document can arrive through: a bare type form,
# and the `any` that carries one.
seen = []
for form, want in zip(forms, wants):
    for call in (lambda: _rt._desc_of(form, ""),
                 lambda: _rt.from_json("any", {"_t": form, "_v": {}})):
        try:
            call()
            raise SystemExit("a form with no descriptor was accepted: %r" % (form,))
        except _rt.MarshalError as e:
            assert e.message == want, (e.message, want)
    seen.append(want)

# The §4.4 half says the section; the fourth family's says the opposite, and
# neither may say "yet" about the other's boundary.
assert _rt._DEFERRED % "fixed<9,2>" == seen[0], _rt._DEFERRED
assert _rt._UNMARSHALLABLE % "native Handle" == seen[1], _rt._UNMARSHALLABLE
assert _rt._UNMARSHALLABLE != _rt._DEFERRED, "the two families are one string"
assert "yet" not in seen[1], seen[1]
assert "is not marshalled by the v1 wire" not in seen[1], seen[1]
print("refused:", seen[1])
"#;
    let out = run_script(
        "nodesc",
        "module nodesc_m { interface Nothing {}; };",
        &script.replace("__FORMS__", &forms).replace("__WANTS__", &wants),
    );
    assert!(out.contains("refused: native Handle has no wire form at all"), "{out}");
}

/// A union's Python surface: `_d`/`_v`, the named branch accessors, and the
/// refusal to read a branch that is not the active one.
///
/// The sweep proves a union *marshals* correctly. This proves the API a caller
/// actually touches, which no round trip exercises: a value can survive the
/// trip while the accessor that produced it lies about which branch is live.
#[test]
fn a_generated_union_answers_for_its_active_branch_only() {
    let out = run_script(
        "unions",
        "module u {\n\
           enum Kind { K_NONE, K_TEXT, K_COUNT };\n\
           union Payload switch (Kind) {\n\
             case K_TEXT: string text;\n\
             case K_COUNT: long count;\n\
             default: boolean flag;\n\
           };\n\
         };",
        r#"
import sys
sys.path.insert(0, sys.argv[1])
from unions import _rt
from unions import u

p = u.Payload(u.K_TEXT, "hello")
assert p._d == u.K_TEXT and p._v == "hello"
assert p.text == "hello"
try:
    p.count
    raise SystemExit("an inactive branch answered")
except _rt.Error as e:
    print("inactive branch refused:", e)

# Setting a branch sets the discriminator that selects it: the two cannot be
# left disagreeing, which is the whole reason `_d` is explicit in §4.5.
p.count = 7
assert p._d == u.K_COUNT and p._v == 7, (p._d, p._v)
print("branch set:", repr(p))

# The default branch is selected by any discriminator no case names.
d = u.Payload(u.K_NONE, True)
assert d.flag is True and d._d == u.K_NONE
print("default branch:", repr(d))

# An enumerator is equal by name and owner, not by identity.
assert u.K_TEXT == _rt.EnumItem("K_TEXT", 1, "IDL:u/Kind:1.0")
assert u.K_TEXT != u.K_COUNT
print("enumerators compare by name and owner")
"#,
    );
    assert!(out.contains("inactive branch refused:"), "{out}");
    assert!(out.contains("branch set:"), "{out}");
    assert!(out.contains("default branch:"), "{out}");
}
