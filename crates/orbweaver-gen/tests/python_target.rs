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
use orbweaver_giop::typecode::TypeCode;
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
fn witness(tc: &TypeCode, visiting: &mut Vec<String>) -> Option<Value> {
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
        TypeCode::Any => Value::Any(Box::new(TypeCode::Double), Box::new(Value::Double(-0.125))),
        TypeCode::ObjRef { .. } => Value::ObjRef(Some(sample_ior())),
        TypeCode::Enum { members, .. } => Value::Enum(members.last()?.clone()),
        TypeCode::Sequence { element, bound } => {
            // A sequence back into a type still being built is where recursion
            // terminates; an empty one is a legal value and the only finite one.
            if terminates(element, visiting) {
                Value::List(Vec::new())
            } else {
                let n = if *bound == 0 { 2 } else { (*bound).min(2) } as usize;
                Value::List((0..n).map(|_| witness(element, visiting)).collect::<Option<_>>()?)
            }
        }
        TypeCode::Array { element, length } => {
            Value::List((0..*length).map(|_| witness(element, visiting)).collect::<Option<_>>()?)
        }
        TypeCode::Struct { id, members, .. } | TypeCode::Except { id, members, .. } => {
            visiting.push(id.clone());
            let out = members
                .iter()
                .map(|m| Some((m.name.clone(), witness(&m.tc, visiting)?)))
                .collect::<Option<Vec<_>>>();
            visiting.pop();
            Value::Struct(out?)
        }
        TypeCode::Union { id, discriminator, cases, default_index, .. } => {
            visiting.push(id.clone());
            // The *last* case rather than the first: with a `default:` present
            // it is usually the default branch, which is the one a generator
            // gets wrong.
            let (i, case) = cases.iter().enumerate().next_back()?;
            let d = label_value(&case.label, discriminator, i as i32 == *default_index)?;
            let v = witness(&case.tc, visiting);
            visiting.pop();
            Value::Union { discriminator: Box::new(d), value: Some(Box::new(v?)) }
        }
        TypeCode::Alias { aliased, .. } => witness(aliased, visiting)?,
        _ => return None,
    })
}

/// Whether a sequence element would re-enter a type already being built.
fn terminates(element: &TypeCode, visiting: &[String]) -> bool {
    match element {
        TypeCode::Recursive(id) => visiting.iter().any(|v| v == id),
        TypeCode::Struct { id, .. } | TypeCode::Union { id, .. } | TypeCode::Except { id, .. } => {
            visiting.iter().any(|v| v == id)
        }
        TypeCode::Alias { aliased, .. } => terminates(aliased, visiting),
        TypeCode::Sequence { element, .. } | TypeCode::Array { element, .. } => {
            terminates(element, visiting)
        }
        _ => false,
    }
}

fn bounded_text(text: &str, bound: u32) -> String {
    if bound == 0 { text.to_owned() } else { text.chars().take(bound as usize).collect() }
}

fn label_value(label: &[u8], disc: &TypeCode, is_default: bool) -> Option<Value> {
    let mut wide: i64 = 0;
    for b in label {
        wide = (wide << 8) | i64::from(*b);
    }
    // The default branch's stored label is not a case value; a discriminator
    // that matches nothing else is what selects it, and every corpus union
    // that has one reserves a value outside its labels.
    Some(match disc {
        TypeCode::Boolean => Value::Bool(!is_default && wide != 0),
        TypeCode::Long => Value::Long(if is_default { i32::MIN } else { wide as i32 }),
        TypeCode::ULong => Value::ULong(if is_default { u32::MAX } else { wide as u32 }),
        TypeCode::Short => Value::Short(if is_default { i16::MIN } else { wide as i16 }),
        TypeCode::UShort => Value::UShort(if is_default { u16::MAX } else { wide as u16 }),
        TypeCode::Char => Value::Char(if is_default { 0xFE } else { wide as u8 }),
        TypeCode::Octet => Value::Octet(if is_default { 0xFE } else { wide as u8 }),
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
                    if descriptor(tc).is_err() || matches!(tc, TypeCode::ObjRef { .. }) {
                        continue;
                    }
                    let Some(v) = witness(tc, &mut Vec::new()) else { continue };
                    let Ok(j) = anyjson::to_json(tc, &v, &mut handles) else { continue };
                    values.push(json_obj([
                        ("id", Json::String(id.clone())),
                        ("desc", Json::String(format!("(\"ref\", {id:?})"))),
                        ("json", j),
                    ]));
                    expected.push((id.clone(), tc.clone(), v));
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
    assert!(
        out.values > 50 && out.calls > 20,
        "the oracle measured almost nothing: {} value(s), {} call(s)",
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
