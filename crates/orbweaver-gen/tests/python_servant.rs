//! What a caller can tell about a servant's language, measured rather than
//! asserted.
//!
//! `docs/decisions/D029-*.md` §6.1 lists five transparencies and says of the
//! **Language** row that it *"leaks by construction: Python is clients only. A
//! Python servant cannot be dispatched into, so the target's language is
//! visible in whether it can be a target at all."* `orbweaver_gen::pyservant`
//! closes that. This file is how we find out what is left.
//!
//! # The shape of the measurement
//!
//! One contract — `corpus/golden/24-skeleton-surface.idl`, which exists because
//! it holds every hazard a dispatcher has: a oneway, both attribute kinds, a
//! readonly setter to refuse, two user exceptions, out parameters, and a
//! leading `double` so the reply body's alignment depends on where the body
//! sits inside the GIOP message.
//!
//! Two servants for it. One is the generated Rust skeleton with a hand-written
//! `Bench` behind it — the thing an application author writes today. The other
//! is [`PyServant`] with [`Mirror`] behind it, which answers the AnyJSON
//! documents a Python servant would send. **Both are then handed byte-identical
//! requests, and their replies are compared byte for byte**, over three GIOP
//! versions and both byte orders.
//!
//! Comparing bytes rather than decoded values is deliberate here and is not the
//! thing `CLAUDE.md` forbids. That rule is about a *foreign* peer, whose CDR
//! padding the specification leaves undefined and which omniORB does not zero;
//! both encoders here are ours, so any difference in the bytes is a difference
//! a caller could observe, which is exactly what is being hunted.
//!
//! # What the differences are
//!
//! Every one this file found is a test below, named after the difference rather
//! than after the mechanism, and each says whether it is closed or remains.
//! [`every_operation_answers_identically_whichever_language_serves_it`] is the
//! gate: it goes red if a new one appears.

mod emitted;

use std::collections::BTreeMap;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_dynamic::json::Json;
use orbweaver_gen::pyservant::{Answerer, PyServant};
use orbweaver_gen::rt::{Dispatch, DispatchBody, ObjectHome, SystemException};
use orbweaver_giop::server::{Request, decode_request};
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, Version, encode_request, read_message};
use orbweaver_registry::{Contract, Registry, Strictness};

use emitted::f_24_skeleton_surface::gc24::{
    Busy, GaugeFault, GaugeRefs, GaugeServant, GaugeSkeleton, GaugeTarget, Reading, Rejected,
};

const KEY: &[u8] = b"gauge";
const TYPE_ID: &str = "IDL:gc24/Gauge:1.0";
const VERSIONS: [Version; 3] = [Version::V1_0, Version::V1_1, Version::V1_2];
const ORDERS: [Endian; 2] = [Endian::Big, Endian::Little];

// ── The Rust servant: what an application author writes today ────────────────

/// A gauge, in Rust. Copied in shape from `skeleton_wire.rs`'s `Bench` on
/// purpose: this file's whole claim is that the *other* servant answers the
/// same, so the Rust half must be the ordinary one and not a special case.
struct Bench {
    samples: Vec<f64>,
    label: String,
    latest: Reading,
}

impl Default for Bench {
    fn default() -> Self {
        Self {
            samples: Vec::new(),
            label: "unset".into(),
            latest: Reading { at: 0.0, sequence_no: 0, unit: String::new() },
        }
    }
}

impl GaugeServant for Bench {
    fn knows(&self, at: &GaugeTarget<'_>) -> bool {
        at.is_default()
    }

    fn latest(&mut self, _at: &GaugeTarget<'_>) -> Result<Reading, GaugeFault> {
        Ok(self.latest.clone())
    }

    fn label(&mut self, _at: &GaugeTarget<'_>) -> Result<String, GaugeFault> {
        Ok(self.label.clone())
    }

    fn set_label(&mut self, _at: &GaugeTarget<'_>, value: String) -> Result<(), GaugeFault> {
        self.label = value;
        Ok(())
    }

    fn record(
        &mut self,
        _at: &GaugeTarget<'_>,
        sample: f64,
        unit: String,
    ) -> Result<Reading, GaugeFault> {
        if sample < 0.0 {
            return Err(GaugeFault::Rejected(Rejected {
                why: "a sample below zero is not a reading".into(),
                code: 7,
            }));
        }
        if unit.is_empty() {
            return Err(GaugeFault::Busy(Busy {}));
        }
        self.samples.push(sample);
        self.latest =
            Reading { at: sample, sequence_no: self.samples.len() as i32, unit: unit.clone() };
        Ok(self.latest.clone())
    }

    fn scale_all(&mut self, _at: &GaugeTarget<'_>, e: f64) -> Result<i32, GaugeFault> {
        for s in &mut self.samples {
            *s *= e;
        }
        self.latest.at *= e;
        Ok(self.samples.len() as i32)
    }

    fn reset(&mut self, _at: &GaugeTarget<'_>) -> Result<(), GaugeFault> {
        self.samples.clear();
        self.latest = Reading { at: 0.0, sequence_no: 0, unit: String::new() };
        Ok(())
    }

    fn split(&mut self, _at: &GaugeTarget<'_>) -> Result<(f64, String), GaugeFault> {
        Ok((self.latest.at, self.latest.unit.clone()))
    }
}

// ── The Python servant, without Python ───────────────────────────────────────

/// The same gauge, answering the documents a generated Python servant sends.
///
/// This is `_rt.dispatch_call`'s output for a `GaugeServant` subclass whose
/// bodies are `Bench`'s, hand-written so that the comparison is between two
/// independent implementations rather than between one and a rendering of
/// itself. `python_servant_end_to_end.rs`'s omniORB case runs the real Python;
/// this one runs every branch with no interpreter, no socket and no fixture, so
/// it is measurable on a machine that is too busy to start a peer.
#[derive(Default)]
struct Mirror {
    samples: Vec<f64>,
    label: String,
    latest_at: f64,
    latest_seq: i32,
    latest_unit: String,
    /// Set by a test to make this servant answer something the Rust one would
    /// not. The negative control for the whole file: with no way to perturb an
    /// answer, a comparison that always passes is indistinguishable from one
    /// that measures nothing.
    perturb: Option<Perturbation>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Perturbation {
    /// One member of a returned struct differs.
    WrongSequenceNo,
    /// A user exception the operation does declare, carrying different members.
    WrongExceptionMembers,
    /// A user exception the operation does **not** declare.
    UndeclaredException,
    /// A system exception whose completion status was never stated.
    UnstatedCompletion,
    /// An out parameter the servant never returned.
    MissingOutParameter,
}

impl Mirror {
    fn new() -> Self {
        Self { label: "unset".into(), ..Default::default() }
    }

    fn with(perturb: Perturbation) -> Self {
        Self { perturb: Some(perturb), ..Self::new() }
    }

    fn reading(&self) -> Json {
        let seq = match self.perturb {
            Some(Perturbation::WrongSequenceNo) => self.latest_seq + 1,
            _ => self.latest_seq,
        };
        json_object([
            ("at", Json::Number(render_f64(self.latest_at))),
            ("sequence_no", Json::Number(seq.to_string())),
            ("unit", Json::String(self.latest_unit.clone())),
        ])
    }
}

impl Answerer for Mirror {
    fn ask(&mut self, call: &Json) -> Result<Json, String> {
        let op = call.get("op").and_then(Json::as_str).ok_or("a call needs an op")?.to_owned();
        let args = call.get("args").cloned().unwrap_or(json_object([]));
        let arg = |name: &str| args.get(name).cloned().unwrap_or(Json::Null);

        Ok(match op.as_str() {
            "_get_latest" => ok(self.reading(), []),
            "_get_label" => ok(Json::String(self.label.clone()), []),
            "_set_label" => {
                self.label = arg("value").as_str().unwrap_or_default().to_owned();
                ok(Json::Null, [])
            }
            "record" => {
                let sample = as_f64(&arg("sample"))?;
                let unit = arg("unit").as_str().unwrap_or_default().to_owned();
                if sample < 0.0 {
                    let why = match self.perturb {
                        Some(Perturbation::WrongExceptionMembers) => "a different reason",
                        _ => "a sample below zero is not a reading",
                    };
                    return Ok(user_exception(
                        "IDL:gc24/Rejected:1.0",
                        json_object([
                            ("why", Json::String(why.to_owned())),
                            ("code", Json::Number("7".to_owned())),
                        ]),
                    ));
                }
                if unit.is_empty() {
                    if self.perturb == Some(Perturbation::UndeclaredException) {
                        // `NotDeclared` is not in `record`'s raises clause, and
                        // is not even in this contract.
                        return Ok(user_exception("IDL:gc24/NotDeclared:1.0", json_object([])));
                    }
                    return Ok(user_exception("IDL:gc24/Busy:1.0", json_object([])));
                }
                self.samples.push(sample);
                self.latest_at = sample;
                self.latest_seq = self.samples.len() as i32;
                self.latest_unit = unit;
                ok(self.reading(), [])
            }
            "scale_all" => {
                if self.perturb == Some(Perturbation::UnstatedCompletion) {
                    // What `_rt.dispatch_call` refuses to produce. It reaches
                    // the Rust half only if the Python half were bypassed, and
                    // the Rust half refuses it too — the boundary check.
                    return Ok(json_object([(
                        "system_exception",
                        json_object([
                            ("id", Json::String("IDL:omg.org/CORBA/INTERNAL:1.0".into())),
                            ("minor", Json::Number("0".into())),
                        ]),
                    )]));
                }
                let e = as_f64(&arg("e"))?;
                for s in &mut self.samples {
                    *s *= e;
                }
                self.latest_at *= e;
                ok(Json::Number(self.samples.len().to_string()), [])
            }
            "reset" => {
                self.samples.clear();
                self.latest_at = 0.0;
                self.latest_seq = 0;
                self.latest_unit = String::new();
                ok(Json::Null, [])
            }
            "split" => {
                if self.perturb == Some(Perturbation::MissingOutParameter) {
                    return Ok(ok(Json::Null, [("at", Json::Number(render_f64(self.latest_at)))]));
                }
                ok(
                    Json::Null,
                    [
                        ("at", Json::Number(render_f64(self.latest_at))),
                        ("unit", Json::String(self.latest_unit.clone())),
                    ],
                )
            }
            other => return Err(format!("this servant does not implement {other:?}")),
        })
    }
}

// ── Documents ────────────────────────────────────────────────────────────────

fn json_object<const N: usize>(fields: [(&str, Json); N]) -> Json {
    Json::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn ok<const N: usize>(returns: Json, outputs: [(&str, Json); N]) -> Json {
    json_object([("ok", json_object([("returns", returns), ("outputs", json_object(outputs))]))])
}

fn user_exception(id: &str, members: Json) -> Json {
    json_object([(
        "user_exception",
        json_object([("id", Json::String(id.to_owned())), ("members", members)]),
    )])
}

fn as_f64(j: &Json) -> Result<f64, String> {
    match j {
        Json::Number(n) => n.parse::<f64>().map_err(|e| e.to_string()),
        other => Err(format!("expected a number, got {other}")),
    }
}

/// A `double` as AnyJSON writes one.
///
/// `{:?}` and not `{}`: Rust's `Display` for a float drops the fractional part
/// of `1.0`, and `to_json` on the Python side would then be handed an integer
/// where the descriptor says double. The seam is where that kind of thing is
/// found, which is the argument for having one.
fn render_f64(v: f64) -> String {
    format!("{v:?}")
}

// ── Harness ──────────────────────────────────────────────────────────────────

fn registry() -> Registry {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/golden/24-skeleton-surface.idl");
    let contract = Contract::load(&path, &Default::default(), Strictness::Checked)
        .expect("the corpus contract must load");
    let mut registry = Registry::new();
    registry.load(&contract.spec).expect("the contract must build a registry");
    registry
}

fn refs() -> GaugeRefs {
    GaugeRefs::new(ObjectHome::new("127.0.0.1", 0, KEY.to_vec()))
}

/// One decoded `Request`, built straight from our encoder — no socket, so both
/// servants can be handed a message with a chosen version, order and origin.
fn request<F: FnOnce(&mut Encoder)>(
    version: Version,
    endian: Endian,
    operation: &str,
    expect_reply: bool,
    args: F,
) -> Request {
    let wire = encode_request(version, endian, 1, KEY, operation, expect_reply, args)
        .expect("encode request");
    let mut cursor: &[u8] = &wire;
    let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("frame");
    decode_request(msg).expect("decode request")
}

/// What a caller receives: the reply status and the body bytes, or the system
/// exception that replaced them.
type Answer = Result<(DispatchBody, Vec<u8>), SystemException>;

fn answer<D: Dispatch>(servant: &mut D, req: &Request, endian: Endian) -> Answer {
    // 24 is where a GIOP 1.2 request body starts, and the origin is what makes
    // the leading `double` in `Reading` a real test rather than a formality.
    let mut out = Encoder::continuing_at(endian, 24);
    match servant.dispatch_body(req, &mut out) {
        Ok(kind) => Ok((kind, out.finish().expect("finish"))),
        Err(e) => Err(e),
    }
}

fn render(a: &Answer) -> String {
    match a {
        Ok((kind, body)) => format!("{kind:?} {body:02x?}"),
        Err(e) => format!("{} minor={} completed={:?}", e.id, e.minor, e.completed),
    }
}

/// Every call the comparison makes, in order, against one pair of servants.
///
/// A sequence rather than independent calls because both servants hold state:
/// `record` then `_get_latest` measures something `record` alone does not, and
/// a divergence that only shows on the second call is exactly the kind a
/// stateless check misses.
fn script(version: Version, endian: Endian) -> Vec<(String, Request)> {
    let mut calls: Vec<(String, Request)> = Vec::new();
    let mut push = |name: &str, req: Request| calls.push((name.to_owned(), req));

    push("_get_label", request(version, endian, "_get_label", true, |_| {}));
    push(
        "_set_label",
        request(version, endian, "_set_label", true, |e| {
            e.put_str("driven by the comparison");
        }),
    );
    push("_get_label after set", request(version, endian, "_get_label", true, |_| {}));
    push(
        "record",
        request(version, endian, "record", true, |e| {
            e.put_f64(21.5);
            e.put_str("C");
        }),
    );
    push("_get_latest", request(version, endian, "_get_latest", true, |_| {}));
    push(
        "record again",
        request(version, endian, "record", true, |e| {
            e.put_f64(22.5);
            e.put_str("C");
        }),
    );
    push("split", request(version, endian, "split", true, |_| {}));
    push(
        "scale_all",
        request(version, endian, "scale_all", true, |e| {
            e.put_f64(2.0);
        }),
    );
    push("_get_latest after scale", request(version, endian, "_get_latest", true, |_| {}));
    push(
        "record refused",
        request(version, endian, "record", true, |e| {
            e.put_f64(-1.0);
            e.put_str("C");
        }),
    );
    push(
        "record busy",
        request(version, endian, "record", true, |e| {
            e.put_f64(1.0);
            e.put_str("");
        }),
    );
    push("reset (oneway)", request(version, endian, "reset", false, |_| {}));
    push("_get_latest after reset", request(version, endian, "_get_latest", true, |_| {}));
    push(
        "_is_a self",
        request(version, endian, "_is_a", true, |e| {
            e.put_str(TYPE_ID);
        }),
    );
    push(
        "_is_a Object",
        request(version, endian, "_is_a", true, |e| {
            e.put_str("IDL:omg.org/CORBA/Object:1.0");
        }),
    );
    push(
        "_is_a stranger",
        request(version, endian, "_is_a", true, |e| {
            e.put_str("IDL:CosNaming/NamingContext:1.0");
        }),
    );
    push("_non_existent", request(version, endian, "_non_existent", true, |_| {}));
    push("no such operation", request(version, endian, "no_such_thing", true, |_| {}));
    push(
        "_set_ on a readonly attribute",
        request(version, endian, "_set_latest", true, |e| {
            e.put_str("x");
        }),
    );
    calls
}

/// Runs `script` against both servants and returns the calls whose answers
/// differed, as `(call, rust, python)`.
fn differences(perturb: Option<Perturbation>) -> Vec<(String, String, String)> {
    let registry = registry();
    let mut found = Vec::new();
    for version in VERSIONS {
        for endian in ORDERS {
            let mut rust = GaugeSkeleton::new(refs(), Bench::default());
            let mirror = match perturb {
                Some(p) => Mirror::with(p),
                None => Mirror::new(),
            };
            let mut python =
                PyServant::new(&registry, TYPE_ID, mirror).expect("the servant must build");
            for (name, req) in script(version, endian) {
                let a = answer(&mut rust, &req, endian);
                let b = answer(&mut python, &req, endian);
                if render(&a) != render(&b) {
                    found.push((format!("{name} ({version} {endian:?})"), render(&a), render(&b)));
                }
            }
        }
    }
    found
}

// ── The gate ─────────────────────────────────────────────────────────────────

/// **The measurement this batch exists for.** Nineteen calls covering every
/// operation, both attribute kinds, a oneway, both user exceptions, the object
/// probes and two refusals — answered by a Rust servant and a Python one, over
/// three GIOP versions and both byte orders, and compared byte for byte.
///
/// A caller holding only a reference cannot tell which language answered. That
/// is D029 §6.1's Language row, and this is what refutes it if it stops being
/// true: a new difference makes this test name the call, the version, the byte
/// order and both answers.
#[test]
fn every_operation_answers_identically_whichever_language_serves_it() {
    let found = differences(None);
    assert!(
        found.is_empty(),
        "a caller can tell these apart, which is a leak in language transparency \
         (D029 §6.1):\n{}",
        found
            .iter()
            .map(|(call, rust, python)| format!(
                "  {call}\n    rust:   {rust}\n    python: {python}\n"
            ))
            .collect::<String>()
    );
}

/// The negative control for the test above, and it moves a counter.
///
/// A comparison with no way to fail is the class `CLAUDE.md` calls
/// green-while-measuring-nothing, and this project has found five of those in
/// one week — every one by a negative control and none by review. So each
/// perturbation below makes the Python servant answer something the Rust one
/// would not, and this test asserts the comparison **sees it** and says which
/// call.
///
/// The count is asserted per perturbation rather than merely "non-empty":
/// a control that fires on every call would also pass a bare `!is_empty()`
/// while telling us the harness is broken rather than that it works.
#[test]
fn the_comparison_sees_a_python_servant_answering_differently() {
    for (perturb, expect_calls) in [
        // The state-reading calls differ once one member of `Reading` is
        // wrong: the two `record`s and every later `_get_latest`.
        (Perturbation::WrongSequenceNo, vec!["record", "_get_latest"]),
        (Perturbation::WrongExceptionMembers, vec!["record refused"]),
        (Perturbation::UndeclaredException, vec!["record busy"]),
        (Perturbation::UnstatedCompletion, vec!["scale_all"]),
        (Perturbation::MissingOutParameter, vec!["split"]),
    ] {
        let found = differences(Some(perturb));
        assert!(
            !found.is_empty(),
            "{perturb:?} made the Python servant answer differently and the comparison \
             did not see it — the gate above is measuring nothing"
        );
        for wanted in &expect_calls {
            assert!(
                found.iter().any(|(call, _, _)| call.starts_with(wanted)),
                "{perturb:?} should have been seen on {wanted:?}; it was seen on {:?}",
                found.iter().map(|(c, _, _)| c.as_str()).collect::<Vec<_>>()
            );
        }
        // And seen on **every** version and byte order. A control that fired on
        // one combination only would mean the matrix is not being walked —
        // which is the same defect as a comparison that cannot fail, one layer
        // up. (The first draft asserted the count was a multiple of six, which
        // is weaker and was also wrong: the number of calls a perturbation
        // reaches is not constant across the matrix.)
        for version in VERSIONS {
            for endian in ORDERS {
                let cell = format!("({version} {endian:?})");
                assert!(
                    found.iter().any(|(call, _, _)| call.ends_with(&cell)),
                    "{perturb:?} was never seen on {cell}; the matrix is not being walked"
                );
            }
        }
    }
}

// ── The differences that remain, each named ──────────────────────────────────

/// A Python servant refuses an undeclared user exception with the answer §4.11
/// fixes, and a Rust servant cannot reach the state at all.
///
/// This is a **difference in what is possible, not in what a caller sees**: the
/// Rust skeleton's generated `GaugeFault` has no variant for an exception the
/// operation does not declare, so the mistake is a compile error. Python has no
/// such enum, so the mistake is available — and the seam answers `UNKNOWN` with
/// the OMG minor for an unlisted user exception, which is what any ORB does
/// with one. A caller therefore sees a legal CORBA answer either way, and what
/// it cannot see is that one of the two servants could have been stopped
/// earlier.
#[test]
fn an_undeclared_raise_reaches_the_caller_as_unknown_and_not_as_itself() {
    let registry = registry();
    let mut python =
        PyServant::new(&registry, TYPE_ID, Mirror::with(Perturbation::UndeclaredException))
            .expect("servant");
    let req = request(Version::V1_2, Endian::Big, "record", true, |e| {
        e.put_f64(1.0);
        e.put_str("");
    });
    let err = answer(&mut python, &req, Endian::Big).expect_err("an undeclared raise is refused");
    assert_eq!(err.id, "IDL:omg.org/CORBA/UNKNOWN:1.0");
    assert_eq!(err.minor, 0x4f4d_0001, "the OMG minor for an unlisted user exception");
}

/// A system exception whose completion status was never stated is refused at
/// the seam, in both halves.
///
/// Rust's `rt::Raising` has no `Default` and no `From`, and its `#[must_use]`
/// makes a forgotten status a warning, because a generator-chosen COMPLETED_NO
/// on a raise that fired halfway through a mutation is how a well-behaved retry
/// loop corrupts state. Python cannot enforce that at compile time — so
/// `_rt.dispatch_call` refuses to serialise one, and the Rust half refuses to
/// accept one, which is the check that holds even if a hand-written Python
/// servant bypasses the runtime.
#[test]
fn a_completion_status_that_was_never_stated_is_refused_rather_than_defaulted() {
    let registry = registry();
    let mut python =
        PyServant::new(&registry, TYPE_ID, Mirror::with(Perturbation::UnstatedCompletion))
            .expect("servant");
    let req = request(Version::V1_2, Endian::Little, "scale_all", true, |e| {
        e.put_f64(2.0);
    });
    let err = answer(&mut python, &req, Endian::Little).expect_err("refused");
    assert_ne!(
        err.id, "IDL:omg.org/CORBA/INTERNAL:1.0",
        "an unstated completion status must not be honoured as though it had been given"
    );
    assert_eq!(err.id, orbweaver_gen::pyservant::SEAM_FAILURE);
}

/// The object probes never reach Python, and that is what keeps a Python object
/// narrowable.
///
/// `_is_a` is a fact about the contract the registry resolved, not about the
/// implementation. A servant that answered it from its own idea of its type
/// could produce an object an ORB refuses to narrow through a base-typed
/// reference — a caller discovering the target's language by failing to use it.
#[test]
fn the_object_probes_are_answered_without_asking_the_servant() {
    /// Answers nothing at all. Any call reaching it is a call that should not
    /// have crossed.
    struct Silent;
    impl Answerer for Silent {
        fn ask(&mut self, call: &Json) -> Result<Json, String> {
            panic!("this call must never reach the servant: {call}");
        }
    }

    let registry = registry();
    let mut python = PyServant::new(&registry, TYPE_ID, Silent).expect("servant");
    for (probe, arg) in [
        ("_is_a", Some(TYPE_ID)),
        ("_is_a", Some("IDL:omg.org/CORBA/Object:1.0")),
        ("_is_a", Some("IDL:CosNaming/NamingContext:1.0")),
        ("_non_existent", None),
    ] {
        let req = request(Version::V1_2, Endian::Big, probe, true, |e| {
            if let Some(a) = arg {
                e.put_str(a);
            }
        });
        answer(&mut python, &req, Endian::Big).expect("a probe is answered here");
    }
}

/// An operation the contract does not declare is `BAD_OPERATION` from both, and
/// the Python servant is never told about it.
#[test]
fn an_unknown_operation_is_refused_before_it_crosses() {
    struct Silent;
    impl Answerer for Silent {
        fn ask(&mut self, call: &Json) -> Result<Json, String> {
            panic!("an unknown operation must not cross: {call}");
        }
    }

    let registry = registry();
    let mut python = PyServant::new(&registry, TYPE_ID, Silent).expect("servant");
    let req = request(Version::V1_2, Endian::Big, "no_such_thing", true, |_| {});
    let err = answer(&mut python, &req, Endian::Big).expect_err("refused");
    assert_eq!(err.id, "IDL:omg.org/CORBA/BAD_OPERATION:1.0");
}

/// The callable surface is the client's, computed once.
///
/// Not a wire property and not a byte comparison — a claim about the emitter
/// that would otherwise drift silently: a servant that answered a *different*
/// set of names than a client of the same contract can send would be a caller
/// telling the two apart by trying. One function decides both, and this is what
/// says so.
#[test]
fn a_python_servant_answers_exactly_the_names_a_python_client_can_send() {
    let registry = registry();
    let client: Vec<String> =
        orbweaver_gen::python::client_operations(&registry, TYPE_ID).keys().cloned().collect();
    let servant = PyServant::new(&registry, TYPE_ID, Mirror::new()).expect("servant");
    let served: Vec<String> = servant.operations().map(str::to_owned).collect();
    assert_eq!(served, client, "the two halves of one contract must have one surface");
    assert!(
        client.iter().any(|o| o == "_get_latest") && !client.iter().any(|o| o == "_set_latest"),
        "a readonly attribute has a getter and no setter: {client:?}"
    );
}

/// The generated Python servant class exists, carries the operation table, and
/// declares no `_is_a`.
///
/// A text assertion on emitted source, which is what the Rust skeleton's own
/// tests do, and for the same reason: the alternative is running Python, and
/// what is being checked here is what the *generator wrote*.
#[test]
fn the_emitter_writes_a_servant_class_with_the_contracts_operations() {
    let registry = registry();
    let package = orbweaver_gen::python::emit_python(&registry, "gc24_surface");
    let module = package
        .files
        .iter()
        .find(|(path, _)| path.ends_with("gc24/__init__.py"))
        .map(|(_, body)| body.clone())
        .expect("the gc24 module must be emitted");

    assert!(module.contains("class GaugeServant(_rt.Servant):"), "{module}");
    for op in ["record", "scale_all", "split", "reset", "_get_latest", "_get_label", "_set_label"] {
        assert!(
            module.contains(&format!("\"{op}\": _rt.Op(")),
            "the operation table must carry {op}:\n{module}"
        );
    }
    assert!(
        !module.contains("\"_set_latest\""),
        "a readonly attribute must produce no setter:\n{module}"
    );
    assert!(
        !module.contains("def _is_a"),
        "a servant must not answer _is_a; the bridge does, from the resolved chain:\n{module}"
    );
    assert!(
        module.contains("raise _rt.Raise.no_implement().did_not_run()"),
        "an unimplemented operation answers NO_IMPLEMENT, not AttributeError:\n{module}"
    );
    // `reset` is oneway and `record` is not: the table has to say so, because
    // §9.4.1 decides whether a reply may be written at all.
    assert!(module.contains("\"reset\": _rt.Op(\"reset\", ins=(), returns=\"void\", outs=(), raises=(), oneway=True)"), "{module}");
}

/// A oneway's answer is dropped and no reply body is written, whichever
/// language served it.
///
/// The hazard `corpus/golden/24` was added for: an empty `NO_EXCEPTION` reply
/// is not almost-nothing, it is a whole message the peer is not waiting for, so
/// it reads it as the header of the next reply and every later request on that
/// connection is answered with the wrong bytes.
#[test]
fn a_oneway_writes_no_reply_from_either_language() {
    let registry = registry();
    for version in VERSIONS {
        for endian in ORDERS {
            let req = request(version, endian, "reset", false, |_| {});
            let mut rust = GaugeSkeleton::new(refs(), Bench::default());
            let mut python = PyServant::new(&registry, TYPE_ID, Mirror::new()).expect("servant");
            for (who, a) in [
                ("rust", answer(&mut rust, &req, endian)),
                ("python", answer(&mut python, &req, endian)),
            ] {
                let (kind, body) = a.expect("a oneway does not fail here");
                assert_eq!(kind, DispatchBody::Return, "{who} {version} {endian:?}");
                assert!(body.is_empty(), "{who} wrote {body:02x?} for a oneway");
            }
        }
    }
}

/// The seam's own conversion table, in both directions, for every argument the
/// contract has.
///
/// Distinct from the comparison above because it fails *differently*: a
/// conversion that lost a value would usually make the two servants disagree,
/// but a conversion that lost it the same way in both — a descriptor wrong on
/// both sides of the seam — would not. So this asks the mapping directly.
#[test]
fn what_crosses_the_seam_comes_back_as_what_went_in() {
    let registry = registry();
    let mut seen: BTreeMap<String, Json> = BTreeMap::new();
    /// Records what the servant was asked, and answers whatever keeps the
    /// script moving.
    struct Recorder<'a>(&'a mut BTreeMap<String, Json>);
    impl Answerer for Recorder<'_> {
        fn ask(&mut self, call: &Json) -> Result<Json, String> {
            let op = call.get("op").and_then(Json::as_str).unwrap_or("").to_owned();
            self.0.insert(op.clone(), call.get("args").cloned().unwrap_or(Json::Null));
            Ok(match op.as_str() {
                "record" | "_get_latest" => ok(
                    json_object([
                        ("at", Json::Number("21.5".into())),
                        ("sequence_no", Json::Number("1".into())),
                        ("unit", Json::String("C".into())),
                    ]),
                    [],
                ),
                "_get_label" => ok(Json::String("unset".into()), []),
                "scale_all" => ok(Json::Number("1".into()), []),
                _ => ok(Json::Null, []),
            })
        }
    }

    let mut python = PyServant::new(&registry, TYPE_ID, Recorder(&mut seen)).expect("servant");
    for (op, build) in [
        (
            "record",
            Box::new(|e: &mut Encoder| {
                e.put_f64(21.5);
                e.put_str("C");
            }) as Box<dyn FnOnce(&mut Encoder)>,
        ),
        ("scale_all", Box::new(|e: &mut Encoder| e.put_f64(2.5))),
        ("_set_label", Box::new(|e: &mut Encoder| e.put_str("a label"))),
    ] {
        let req = request(Version::V1_2, Endian::Big, op, true, build);
        answer(&mut python, &req, Endian::Big).expect("dispatch");
    }
    drop(python);

    assert_eq!(
        seen.get("record").map(ToString::to_string).as_deref(),
        Some(r#"{"sample":21.5,"unit":"C"}"#),
        "a double and a string cross unchanged"
    );
    assert_eq!(
        seen.get("scale_all").map(ToString::to_string).as_deref(),
        Some(r#"{"e":2.5}"#),
        "the parameter named `e` is the one the corpus file added deliberately"
    );
    assert_eq!(
        seen.get("_set_label").map(ToString::to_string).as_deref(),
        Some(r#"{"value":"a label"}"#),
        "an attribute setter's parameter is named `value` (§7.9.1)"
    );
}
