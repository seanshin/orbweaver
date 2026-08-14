//! §8 in the serving direction: **a generated skeleton's reply bytes must
//! equal the dynamic path's bytes for the same values.**
//!
//! The calling direction has had this oracle since stream B landed
//! (`static-oracle`: generated stub bytes versus DII bytes). The serving
//! direction had nothing. Every skeleton test until now compared a generated
//! encoder against a generated decoder — our own code on both ends of the
//! wire, which agrees with itself by construction and would go on agreeing
//! after both halves were wrong in the same way. This file is the missing
//! half: the dynamic path is the reference implementation, the one verified
//! against two independent ORBs, and a skeleton is correct exactly when its
//! reply equals what the dynamic path would have written for the same values.
//!
//! # What is compared
//!
//! For every operation of a corpus interface, over **three GIOP versions** and
//! **both byte orders** and **two reply origins**:
//!
//! 1. the arguments are marshalled by the *dynamic* encoder into a real GIOP
//!    request, so the skeleton's decode is measured against the reference
//!    encoder rather than against our own;
//! 2. the generated skeleton dispatches it and writes a reply body;
//! 3. the dynamic encoder writes the same values, in §7.9.1's reply order,
//!    into an encoder with the same origin;
//! 4. the two byte strings must be identical, under the same reply status.
//!
//! The second origin (20) is not one any server produces. `Server` hands over
//! 24, which is already 8-aligned, so at 24 alone a skeleton that rebuilt its
//! body in a fresh buffer would be *indistinguishable* from a correct one. At
//! 20 the padding before an 8-aligned member exists only if the origin is
//! honoured, so the comparison can actually fail.
//!
//! # What is not compared, and why
//!
//! Named rather than skipped. Every member of every interface under test is
//! either a case here or on [`NOT_COMPARED`], and
//! [`every_member_is_compared_or_named`] fails if a contract grows a member
//! that is neither. Three further limits are worth stating, because a
//! comparison that passes for a weak reason still passes:
//!
//! * **An empty reply body compares two empty byte strings.** `_set_label` and
//!   `store` return `void`, so their cases prove that the *arguments* arrived
//!   (the servant checks them) and nothing about encoding.
//! * **A `SystemException` reply is not the skeleton's bytes.** It is written
//!   by `orbweaver-giop`'s `encode_system_exception`, so there is no generated
//!   encoding to hold a dynamic one to; `servant_faults.rs` measures those
//!   against omniORB instead.
//! * **The wide-character codec is pinned on both sides**, so a `wstring`
//!   would compare equal here even on a connection where the two paths should
//!   differ. Neither interface under test has one, and
//!   [`no_interface_under_test_carries_a_wide_type`] is what keeps that true.

mod emitted;

use std::collections::BTreeMap;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_dynamic::Value;
use orbweaver_gen::rt::{Dispatch, DispatchBody, ObjectHome};
use orbweaver_giop::server::{Request, decode_request};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, Version, encode_request, read_message};
use orbweaver_registry::{ParamDirection, Registry};

use emitted::f_24_skeleton_surface::gc24::{
    Busy, GaugeFault, GaugeRefs, GaugeServant, GaugeSkeleton, GaugeTarget, Reading, Rejected,
};
use emitted::f_25_servant_faults::fault25::{
    VaultFault, VaultRefs, VaultServant, VaultSkeleton, VaultTarget,
};

const KEY: &[u8] = b"oracle";

/// The key scheme both skeletons under test serve under: one object each, at
/// the bare root key. Neither contract returns an object reference, so the
/// published host and port are never read out of a reply.
fn gauge_refs() -> GaugeRefs {
    GaugeRefs::new(ObjectHome::new("127.0.0.1", 0, KEY.to_vec()))
}

/// The same, for the vault.
fn vault_refs() -> VaultRefs {
    VaultRefs::new(ObjectHome::new("127.0.0.1", 0, KEY.to_vec()))
}

const GAUGE: &str = "IDL:gc24/Gauge:1.0";
const VAULT: &str = "IDL:fault25/Vault:1.0";
const VERSIONS: [Version; 3] = [Version::V1_0, Version::V1_1, Version::V1_2];
/// The origin `Server` hands a dispatch, and one that is not 8-aligned.
const ORIGINS: [usize; 2] = [24, 20];

/// The members with no dynamic counterpart at all, and why there is none.
///
/// A silent skip is the harness failure `CLAUDE.md` names; this list is the
/// alternative, and it is checked for completeness rather than trusted. Only
/// operations that genuinely *cannot* be compared belong here — an operation
/// that compares weakly is compared, and its weakness is in the module docs.
const NOT_COMPARED: [(&str, &str, &str); 2] = [
    (
        GAUGE,
        "reset",
        "oneway (§9.4.1): there is no reply message on either side, so there are no reply \
         bytes to compare. skeleton_wire.rs asserts the absence instead.",
    ),
    (
        VAULT,
        "forget",
        "oneway (§9.4.1), as above. servant_faults.rs also checks that the fault the servant \
         raised was dropped rather than lost.",
    ),
];

// ── The canned servants: fixed answers, and arguments checked on arrival ─────

const SAMPLE: f64 = 2.5;
const UNIT: &str = "C";
const LABEL: &str = "bench";
const NEW_LABEL: &str = "written by the oracle";
const SCALE: f64 = 4.0;
const SCALED: i32 = 3;
const WHY: &str = "a sample below zero is not a reading";
const CODE: i32 = 7;

fn reading() -> Reading {
    // A double first (the alignment pin), then a long, then a string: the
    // reply body whose padding depends on where the body sits.
    Reading { at: -0.125, sequence_no: 9, unit: "kPa".into() }
}

fn reading_value() -> Value {
    Value::Struct(vec![
        ("at".into(), Value::Double(-0.125)),
        ("sequence_no".into(), Value::Long(9)),
        ("unit".into(), Value::String("kPa".into())),
    ])
}

/// A gauge that answers with constants and checks what it was handed.
///
/// The checks are what make the *request* side part of the oracle: the
/// arguments were marshalled by the dynamic encoder, so a decode that
/// disagrees with it fails here rather than silently producing a reply for
/// different values.
struct CannedGauge;

impl GaugeServant for CannedGauge {
    /// One object, addressed by the bare root key the server was bound with.
    fn knows(&self, __at: &GaugeTarget<'_>) -> bool {
        __at.is_default()
    }

    fn latest(&mut self, __at: &GaugeTarget<'_>) -> Result<Reading, GaugeFault> {
        Ok(reading())
    }

    fn label(&mut self, __at: &GaugeTarget<'_>) -> Result<String, GaugeFault> {
        Ok(LABEL.to_owned())
    }

    fn set_label(&mut self, __at: &GaugeTarget<'_>, value: String) -> Result<(), GaugeFault> {
        assert_eq!(value, NEW_LABEL, "the dynamic encoder's string did not arrive intact");
        Ok(())
    }

    fn record(
        &mut self,
        __at: &GaugeTarget<'_>,
        sample: f64,
        unit: String,
    ) -> Result<Reading, GaugeFault> {
        if sample < 0.0 {
            return Err(GaugeFault::Rejected(Rejected { why: WHY.into(), code: CODE }));
        }
        if unit.is_empty() {
            return Err(GaugeFault::Busy(Busy {}));
        }
        assert_eq!(sample, SAMPLE, "a double did not survive the dynamic encoder");
        assert_eq!(unit, UNIT, "a string did not survive the dynamic encoder");
        Ok(reading())
    }

    fn scale_all(&mut self, __at: &GaugeTarget<'_>, e: f64) -> Result<i32, GaugeFault> {
        assert_eq!(e, SCALE);
        Ok(SCALED)
    }

    fn reset(&mut self, __at: &GaugeTarget<'_>) -> Result<(), GaugeFault> {
        Ok(())
    }

    fn split(&mut self, __at: &GaugeTarget<'_>) -> Result<(f64, String), GaugeFault> {
        Ok((-0.125, "kPa".into()))
    }
}

const FETCHED: &str = "first";
const ROTATE_ARG: i32 = 7;
const ROTATED: i32 = 8;
const DEPTH: i32 = 1;

/// A vault with no state at all: the oracle drives one servant through every
/// case, so an answer that depended on call order would not be an oracle.
struct CannedVault;

impl VaultServant for CannedVault {
    /// One object, addressed by the bare root key the server was bound with.
    fn knows(&self, __at: &VaultTarget<'_>) -> bool {
        __at.is_default()
    }

    fn fetch(&mut self, __at: &VaultTarget<'_>, key: String) -> Result<String, VaultFault> {
        assert_eq!(key, "alpha");
        Ok(FETCHED.to_owned())
    }

    fn store(
        &mut self,
        __at: &VaultTarget<'_>,
        key: String,
        text: String,
    ) -> Result<(), VaultFault> {
        assert_eq!((key.as_str(), text.as_str()), ("beta", "second"));
        Ok(())
    }

    fn rotate(&mut self, __at: &VaultTarget<'_>, wanted: i32) -> Result<i32, VaultFault> {
        assert_eq!(wanted, ROTATE_ARG);
        Ok(ROTATED)
    }

    fn forget(&mut self, __at: &VaultTarget<'_>, _key: String) -> Result<(), VaultFault> {
        Ok(())
    }

    fn depth(&mut self, __at: &VaultTarget<'_>) -> Result<i32, VaultFault> {
        Ok(DEPTH)
    }
}

// ── The oracle ───────────────────────────────────────────────────────────────

/// What the servant answers with, in the shape the dynamic side encodes.
enum Answer {
    /// The declared result first when it is not `void`, then `out` and `inout`
    /// values in declaration order (§7.9.1).
    Returns(Vec<Value>),
    /// A user exception: repository id first, then the members (§9.4.3.1).
    Raises(&'static str, Value),
}

/// One comparison, with every typecode already resolved.
struct Case {
    op: String,
    arg_tcs: Vec<TypeCode>,
    args: Vec<Value>,
    reply_tcs: Vec<TypeCode>,
    answer: Answer,
}

/// A case whose shapes come from the contract, which is the point: the dynamic
/// side is driven by the registry, not by a second hand-written description of
/// the same operation.
fn declared(reg: &Registry, iface: &str, op: &str, args: Vec<Value>, answer: Answer) -> Case {
    let (arg_tcs, reply_tcs) = shapes(reg, iface, op);
    assert_eq!(args.len(), arg_tcs.len(), "{iface} {op}: argument count");
    if let Answer::Returns(vs) = &answer {
        assert_eq!(vs.len(), reply_tcs.len(), "{iface} {op}: reply value count");
    }
    Case { op: op.to_owned(), arg_tcs, args, reply_tcs, answer }
}

/// The argument and reply typecodes of `op`, from the contract.
///
/// Attribute accessors are operations on the wire and not in the registry's
/// operation table, so they are synthesised here exactly as the generator
/// synthesises them — `_get_x` returns the attribute, `_set_x` takes it and
/// returns nothing.
fn shapes(reg: &Registry, iface: &str, op: &str) -> (Vec<TypeCode>, Vec<TypeCode>) {
    if let Some(attr) = op.strip_prefix("_get_") {
        let a = attribute(reg, iface, attr);
        return (Vec::new(), vec![a]);
    }
    if let Some(attr) = op.strip_prefix("_set_") {
        let a = attribute(reg, iface, attr);
        return (vec![a], Vec::new());
    }
    let (_, sig) =
        reg.resolve_operation(iface, op).unwrap_or_else(|| panic!("{iface} has no {op}"));
    let mut args = Vec::new();
    let mut reply = Vec::new();
    if !matches!(sig.returns, TypeCode::Void) {
        reply.push(sig.returns.clone());
    }
    for p in &sig.params {
        if matches!(p.direction, ParamDirection::In | ParamDirection::InOut) {
            args.push(p.tc.clone());
        }
        if matches!(p.direction, ParamDirection::Out | ParamDirection::InOut) {
            reply.push(p.tc.clone());
        }
    }
    (args, reply)
}

fn attribute(reg: &Registry, iface: &str, name: &str) -> TypeCode {
    let mut ids = vec![iface.to_owned()];
    ids.extend(reg.ancestors(iface));
    for id in ids {
        if let Some(a) = reg.interface(&id).and_then(|i| i.attributes.get(name)) {
            return a.tc.clone();
        }
    }
    panic!("{iface} has no attribute {name}");
}

/// `_is_a` and `_non_existent` are `CORBA::Object`'s, not the contract's, so
/// the registry has no signature for them. §4.3.x fixes their shapes —
/// `boolean _is_a(in string)` and `boolean _non_existent()` — so the dynamic
/// side can still be driven; the typecodes are supplied here instead of
/// resolved, and that is the only difference.
fn pseudo(op: &str, arg_tcs: Vec<TypeCode>, args: Vec<Value>, reply: Value) -> Case {
    Case {
        op: op.to_owned(),
        arg_tcs,
        args,
        reply_tcs: vec![TypeCode::Boolean],
        answer: Answer::Returns(vec![reply]),
    }
}

/// Builds the GIOP request whose body the *dynamic* encoder wrote.
fn request(version: Version, endian: Endian, case: &Case) -> Request {
    let wire = encode_request(version, endian, 1, KEY, &case.op, true, |e| {
        for (tc, v) in case.arg_tcs.iter().zip(&case.args) {
            orbweaver_dynamic::encode(e, tc, v).expect("the dynamic encoder writes the arguments");
        }
    })
    .expect("encode request");
    let mut cursor: &[u8] = &wire;
    let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("frame");
    decode_request(msg).expect("decode request")
}

/// The reply body the dynamic path would have written, at the same origin.
fn dynamic_reply(registry: &Registry, case: &Case, endian: Endian, origin: usize) -> Vec<u8> {
    let mut e = Encoder::continuing_at(endian, origin);
    match &case.answer {
        Answer::Returns(values) => {
            for (tc, v) in case.reply_tcs.iter().zip(values) {
                orbweaver_dynamic::encode(&mut e, tc, v).expect("dynamic reply");
            }
        }
        Answer::Raises(id, members) => {
            // §9.4.3.1: the repository id leads the body, then the members —
            // and the id is encoded as a `string` by the same encoder, so even
            // that is the dynamic path's bytes rather than ours.
            orbweaver_dynamic::encode(&mut e, &TypeCode::String(0), &Value::String((*id).into()))
                .expect("the repository id");
            let tc = registry.typecode(id).unwrap_or_else(|| panic!("{id} is not in the registry"));
            orbweaver_dynamic::encode(&mut e, tc, members).expect("the exception members");
        }
    }
    e.finish().expect("finish")
}

/// Runs every case against `skeleton`, returning one line per mismatch.
///
/// Collected rather than asserted case by case: a batch is verified as a batch
/// (`CLAUDE.md` §the operating model), and one failing byte order out of
/// twelve is a different diagnosis from all twelve failing.
fn compare(registry: &Registry, skeleton: &mut dyn Dispatch, cases: &[Case]) -> Vec<String> {
    let mut bad = Vec::new();
    for case in cases {
        for version in VERSIONS {
            for endian in [Endian::Big, Endian::Little] {
                for origin in ORIGINS {
                    let req = request(version, endian, case);
                    let mut out = Encoder::continuing_at(endian, origin);
                    let where_ = format!("{} {version} {endian:?} origin {origin}", case.op);

                    let label = match skeleton.dispatch_body(&req, &mut out) {
                        Ok(label) => label,
                        Err(ex) => {
                            bad.push(format!("{where_}: the skeleton raised {}", ex.id));
                            continue;
                        }
                    };
                    let wanted_label = match case.answer {
                        Answer::Returns(_) => DispatchBody::Return,
                        Answer::Raises(..) => DispatchBody::UserException,
                    };
                    if label != wanted_label {
                        bad.push(format!(
                            "{where_}: reply status {label:?}, want {wanted_label:?}"
                        ));
                        continue;
                    }

                    let stat = out.finish().expect("finish");
                    let dyna = dynamic_reply(registry, case, endian, origin);
                    if stat != dyna {
                        bad.push(format!("{where_}: static {stat:02x?} != dynamic {dyna:02x?}"));
                    }
                }
            }
        }
    }
    bad
}

fn registry_of(file: &str) -> Registry {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/golden").join(file);
    let src = std::fs::read_to_string(&path).expect("the corpus file");
    let spec = orbweaver_idl::parse(&src).expect("parses");
    let mut r = Registry::new();
    r.load(&spec).expect("loads");
    r
}

fn gauge_cases(reg: &Registry) -> Vec<Case> {
    vec![
        declared(
            reg,
            GAUGE,
            "record",
            vec![Value::Double(SAMPLE), Value::String(UNIT.into())],
            Answer::Returns(vec![reading_value()]),
        ),
        // The user-exception body, which is a reply the skeleton encodes and
        // the dynamic path can therefore be held to: id, then members.
        declared(
            reg,
            GAUGE,
            "record",
            vec![Value::Double(-1.0), Value::String(UNIT.into())],
            Answer::Raises(
                "IDL:gc24/Rejected:1.0",
                Value::Struct(vec![
                    ("why".into(), Value::String(WHY.into())),
                    ("code".into(), Value::Long(CODE)),
                ]),
            ),
        ),
        // A member-less exception: the id and nothing else.
        declared(
            reg,
            GAUGE,
            "record",
            vec![Value::Double(1.0), Value::String(String::new())],
            Answer::Raises("IDL:gc24/Busy:1.0", Value::Struct(Vec::new())),
        ),
        declared(
            reg,
            GAUGE,
            "scale_all",
            vec![Value::Double(SCALE)],
            Answer::Returns(vec![Value::Long(SCALED)]),
        ),
        // Two `out` parameters and no return: the §7.9.1 order with nothing in
        // front of it.
        declared(
            reg,
            GAUGE,
            "split",
            Vec::new(),
            Answer::Returns(vec![Value::Double(-0.125), Value::String("kPa".into())]),
        ),
        declared(reg, GAUGE, "_get_latest", Vec::new(), Answer::Returns(vec![reading_value()])),
        declared(
            reg,
            GAUGE,
            "_get_label",
            Vec::new(),
            Answer::Returns(vec![Value::String(LABEL.into())]),
        ),
        declared(
            reg,
            GAUGE,
            "_set_label",
            vec![Value::String(NEW_LABEL.into())],
            Answer::Returns(Vec::new()),
        ),
        pseudo(
            "_is_a",
            vec![TypeCode::String(0)],
            vec![Value::String(GAUGE.into())],
            Value::Bool(true),
        ),
        pseudo(
            "_is_a",
            vec![TypeCode::String(0)],
            vec![Value::String("IDL:omg.org/CosNaming/NamingContext:1.0".into())],
            Value::Bool(false),
        ),
        pseudo("_non_existent", Vec::new(), Vec::new(), Value::Bool(false)),
    ]
}

fn vault_cases(reg: &Registry) -> Vec<Case> {
    vec![
        declared(
            reg,
            VAULT,
            "fetch",
            vec![Value::String("alpha".into())],
            Answer::Returns(vec![Value::String(FETCHED.into())]),
        ),
        declared(
            reg,
            VAULT,
            "store",
            vec![Value::String("beta".into()), Value::String("second".into())],
            Answer::Returns(Vec::new()),
        ),
        declared(
            reg,
            VAULT,
            "rotate",
            vec![Value::Long(ROTATE_ARG)],
            Answer::Returns(vec![Value::Long(ROTATED)]),
        ),
        declared(reg, VAULT, "_get_depth", Vec::new(), Answer::Returns(vec![Value::Long(DEPTH)])),
        pseudo(
            "_is_a",
            vec![TypeCode::String(0)],
            vec![Value::String(VAULT.into())],
            Value::Bool(true),
        ),
        pseudo("_non_existent", Vec::new(), Vec::new(), Value::Bool(false)),
    ]
}

// ── The tests ────────────────────────────────────────────────────────────────

#[test]
fn a_gauge_skeletons_replies_are_the_dynamic_paths_bytes() {
    let reg = registry_of("24-skeleton-surface.idl");
    let cases = gauge_cases(&reg);
    let mut skeleton = GaugeSkeleton::new(gauge_refs(), CannedGauge);
    let bad = compare(&reg, &mut skeleton, &cases);
    assert!(
        bad.is_empty(),
        "{} of {} comparisons disagree:\n  {}",
        bad.len(),
        cases.len() * VERSIONS.len() * 2 * ORIGINS.len(),
        bad.join("\n  ")
    );
}

#[test]
fn a_vault_skeletons_replies_are_the_dynamic_paths_bytes() {
    let reg = registry_of("25-servant-faults.idl");
    let cases = vault_cases(&reg);
    let mut skeleton = VaultSkeleton::new(vault_refs(), CannedVault);
    let bad = compare(&reg, &mut skeleton, &cases);
    assert!(
        bad.is_empty(),
        "{} of {} comparisons disagree:\n  {}",
        bad.len(),
        cases.len() * VERSIONS.len() * 2 * ORIGINS.len(),
        bad.join("\n  ")
    );
}

/// The oracle's own coverage, measured rather than claimed.
///
/// Every operation and attribute accessor of both interfaces must be either
/// compared above or on [`NOT_COMPARED`] with a reason. An operation added to
/// either contract and to neither list fails here — which is the difference
/// between an oracle and a set of examples. The coverage is printed too, so a
/// harness reading the output sees what was measured rather than inferring it.
#[test]
fn every_member_is_compared_or_named() {
    let mut missing = Vec::new();
    for (file, iface, cases) in [
        ("24-skeleton-surface.idl", GAUGE, gauge_cases as fn(&Registry) -> Vec<Case>),
        ("25-servant-faults.idl", VAULT, vault_cases as fn(&Registry) -> Vec<Case>),
    ] {
        let reg = registry_of(file);
        let compared: Vec<String> = cases(&reg).iter().map(|c| c.op.clone()).collect();
        let named: Vec<&str> =
            NOT_COMPARED.iter().filter(|(i, ..)| *i == iface).map(|(_, op, _)| *op).collect();

        let mut wire_names: Vec<String> = Vec::new();
        let mut ids = vec![iface.to_owned()];
        ids.extend(reg.ancestors(iface));
        let mut seen: BTreeMap<String, ()> = BTreeMap::new();
        for id in ids {
            let Some(i) = reg.interface(&id) else { continue };
            for op in i.operations.keys() {
                seen.entry(op.clone()).or_default();
            }
            for (attr, a) in &i.attributes {
                seen.entry(format!("_get_{attr}")).or_default();
                if !a.readonly {
                    seen.entry(format!("_set_{attr}")).or_default();
                }
            }
        }
        wire_names.extend(seen.into_keys());
        // The two the skeleton answers on CORBA::Object's behalf are part of
        // its surface too: an ORB that cannot probe cannot narrow.
        wire_names.push("_is_a".into());
        wire_names.push("_non_existent".into());

        for name in &wire_names {
            if !compared.contains(name) && !named.contains(&name.as_str()) {
                missing.push(format!("{iface} {name}"));
            }
        }
        eprintln!(
            "{iface}: {} member(s), {} comparison(s) over {} version(s) × 2 byte order(s) × {} \
             origin(s); not compared: {}",
            wire_names.len(),
            compared.len() * VERSIONS.len() * 2 * ORIGINS.len(),
            VERSIONS.len(),
            ORIGINS.len(),
            if named.is_empty() { "nothing".to_owned() } else { named.join(", ") },
        );
    }
    assert!(
        missing.is_empty(),
        "these members are neither compared nor named as incomparable:\n  {}\n\
         Add a case to the oracle, or a line to NOT_COMPARED saying why there is no dynamic \
         equivalent.",
        missing.join("\n  ")
    );
}

/// The oracle must be able to fail.
///
/// A comparison that cannot distinguish a correct skeleton from a broken one
/// is a decoration. This feeds the dynamic side a value the servant does not
/// answer with and a body built at the wrong origin, and requires both to be
/// caught — the second is the alignment-origin bug the 24-byte origin cannot
/// see, and the reason [`ORIGINS`] has a second entry.
#[test]
fn the_oracle_notices_a_wrong_value_and_a_wrong_origin() {
    let reg = registry_of("24-skeleton-surface.idl");
    let mut skeleton = GaugeSkeleton::new(gauge_refs(), CannedGauge);

    let wrong = Case {
        op: "_get_label".into(),
        arg_tcs: Vec::new(),
        args: Vec::new(),
        reply_tcs: vec![TypeCode::String(0)],
        answer: Answer::Returns(vec![Value::String("not what the servant says".into())]),
    };
    assert!(!compare(&reg, &mut skeleton, &[wrong]).is_empty(), "a wrong value must be caught");

    // The alignment-origin bug, staged: the skeleton writes at origin 20 (as
    // the oracle asks) and the dynamic side is built at 24, which is what a
    // body copied into a fresh buffer amounts to. Only the misaligned origin
    // can tell the difference, and `Reading` starts with a double.
    let case =
        declared(&reg, GAUGE, "_get_latest", Vec::new(), Answer::Returns(vec![reading_value()]));
    let req = request(Version::V1_2, Endian::Big, &case);
    let mut out = Encoder::continuing_at(Endian::Big, 20);
    skeleton.dispatch_body(&req, &mut out).expect("dispatch");
    assert_ne!(
        out.finish().expect("finish"),
        dynamic_reply(&reg, &case, Endian::Big, 24),
        "an origin the skeleton ignored must not compare equal"
    );
}

/// The one place the oracle is blind, said out loud.
///
/// `orbweaver_gen::rt` pins the wide-character codec to GIOP 1.2 + UTF-16 (see
/// its `wide()`), and `orbweaver_dynamic::encode` defaults to the same, so a
/// `wstring` reply would compare equal here on a 1.0 connection where the two
/// *should* differ — the codec belongs to the connection, and threading it
/// through generated signatures is separate work. Neither corpus interface
/// under test has a wide type, so nothing here is affected today; this test
/// exists so that stops being true loudly rather than quietly.
#[test]
fn no_interface_under_test_carries_a_wide_type() {
    fn wide(tc: &TypeCode) -> bool {
        match tc {
            TypeCode::WChar | TypeCode::WString(_) => true,
            TypeCode::Sequence { element, .. } | TypeCode::Array { element, .. } => wide(element),
            TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
                members.iter().any(|m| wide(&m.tc))
            }
            TypeCode::Alias { aliased, .. } => wide(aliased),
            _ => false,
        }
    }
    for (file, iface) in [("24-skeleton-surface.idl", GAUGE), ("25-servant-faults.idl", VAULT)] {
        let reg = registry_of(file);
        let cases = if iface == GAUGE { gauge_cases(&reg) } else { vault_cases(&reg) };
        for case in cases {
            for tc in case.arg_tcs.iter().chain(&case.reply_tcs) {
                assert!(
                    !wide(tc),
                    "{iface} {} carries a wide type; the oracle's fixed 1.2/UTF-16 codec makes \
                     this comparison weaker than it looks — read the doc on this test",
                    case.op
                );
            }
        }
    }
}
