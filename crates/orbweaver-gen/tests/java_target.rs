//! The Java target's oracle: the generated code is **executed**, and what it
//! produces is held to the Rust mapping over the whole golden corpus at once.
//!
//! # The criterion
//!
//! §4.5 states its own acceptance criterion — for any value, `CDR → JSON → CDR`
//! must reproduce identical bytes — and this test is that rule with Java in the
//! middle:
//!
//! ```text
//! Value --Rust to_json--> JSON --Java _fromJson--> a Java object
//!       --Java _toJson--> JSON --Rust from_json--> Value --encode--> bytes
//! ```
//!
//! The bytes at the end must equal the bytes the original value encodes to, in
//! **both byte orders**. Comparing the two JSON documents as text would be the
//! mistake `CLAUDE.md` names about CDR padding, one layer up: `2.5` and `2.50`
//! are the same value and different strings.
//!
//! # What is executed, and what that proves
//!
//! Two things run under a JDK. The **runtime** (`_Rt.java`), which is a third
//! implementation of §4.5 and the only part of this target that could disagree
//! with the reference mapping. And the **generated stubs**, driven through
//! `_Rt.Loopback` by name: a stub renders its arguments into a request and
//! reads a reply, and both are compared here. So a template that dropped a
//! parameter, ordered two members wrongly, or lost an `out` value fails this
//! test rather than failing a user.
//!
//! No ORB, no fixture and no network are involved, and **that is why this cell
//! can never satisfy D032 §4 clause 6**: both ends are ours. The live legs are
//! `spikes/bindings/java/client-omniorb.sh` and `client-jacorb.sh`, and the
//! suite's verdict — not this file — is what says whether Java is a target.
//!
//! # When there is no JDK
//!
//! Every test here prints `UNMEASURED` and returns, rather than failing. That
//! is not a softening of *an unmeasured check is a failure, never a pass*: it
//! is where the sentence is enforced. `spikes/bindings/java/client-self.sh`
//! checks for the JDK itself — absent is exit 2, a counted SKIPPED naming the
//! fixture; present but printing `UNMEASURED` is exit 1, a failure — so the
//! verdict lives in the suite, and `cargo test --workspace` on a machine with
//! no JDK stays green without claiming anything.
//!
//! *생성된 Java를 **실행**해 Rust 매핑과 대조한다. 양쪽 끝이 모두 우리 것이므로 이
//! 셀은 절 6을 결코 충족할 수 없다 — 살아 있는 다리는 스위트의 다른 칸이다.*

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_dynamic::anyjson::{self, LocalReferences};
use orbweaver_dynamic::json::Json;
use orbweaver_dynamic::{Value, encode};
use orbweaver_gen::java::{emit_java, java_name};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_giop::{IiopProfile, Ior, Version};
use orbweaver_registry::{Entry, ParamDirection, Registry};

// ── the JDK, and what its absence means ─────────────────────────────────────

/// Where `javac` and `java` are, or `None`.
///
/// `ORBWEAVER_JAVA_HOME` first, then the JDK the JacORB fixtures already
/// hard-code (`spikes/jacorb/setup.sh`), then `JAVA_HOME`, then `PATH`. The
/// third and fourth are last because on this machine neither answers: JDK 11
/// removed CORBA (JEP 320), Homebrew's `openjdk@21` is keg-only, and nothing is
/// on `PATH` at all.
fn jdk() -> Option<(PathBuf, PathBuf)> {
    let candidates = [
        std::env::var("ORBWEAVER_JAVA_HOME").ok(),
        Some("/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home".to_owned()),
        std::env::var("JAVA_HOME").ok(),
    ];
    for home in candidates.into_iter().flatten() {
        let javac = Path::new(&home).join("bin/javac");
        let java = Path::new(&home).join("bin/java");
        if javac.is_file() && java.is_file() {
            return Some((javac, java));
        }
    }
    let which = Command::new("sh").arg("-c").arg("command -v javac").output().ok()?;
    if which.status.success() && !which.stdout.is_empty() {
        return Some((PathBuf::from("javac"), PathBuf::from("java")));
    }
    None
}

/// The one sentence a test prints when it could not measure.
///
/// The runner greps for it, so it is spelled once here.
fn unmeasured(what: &str) {
    println!(
        "UNMEASURED  {what}: no JDK found. Set ORBWEAVER_JAVA_HOME, or install one \
         (spikes/jacorb/setup.sh names the same JDK 21 this looks for)."
    );
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn tmp(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("java-target/{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Generates a registry as Java under `dir`, adds the driver, and compiles.
///
/// The package is always `contract`, because the driver imports `contract._Rt`
/// and a driver parameterised by package name would be a second place that
/// knows the layout.
fn build(javac: &Path, registry: &Registry, dir: &Path) -> Result<(PathBuf, Vec<String>), String> {
    let generated = emit_java(registry, "contract");
    let src = dir.join("src");
    let mut files = Vec::new();
    for (name, text) in &generated.files {
        let target = src.join(name);
        std::fs::create_dir_all(target.parent().expect("a file has a parent")).expect("mkdir");
        std::fs::write(&target, text).expect("write");
        files.push(target);
    }
    let driver = src.join("java_sweep.java");
    std::fs::write(&driver, include_str!("java_sweep.java")).expect("driver");
    files.push(driver);

    let classes = dir.join("classes");
    let mut cmd = Command::new(javac);
    cmd.arg("-nowarn").arg("-encoding").arg("UTF-8").arg("-d").arg(&classes);
    for f in &files {
        cmd.arg(f);
    }
    let out = cmd.output().map_err(|e| format!("javac: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "javac refused what the emitter wrote:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    // The ids the emitter refused, so a caller does not drive a stub that was
    // deliberately not written. A skipped interface leaves only a name-holder
    // class with a private constructor, and calling it is the *test's* mistake
    // rather than the emitter's.
    Ok((classes, generated.skipped.iter().map(|(id, _)| id.clone()).collect()))
}

/// The driver process, one request per line.
struct Driver {
    child: Child,
    out: BufReader<std::process::ChildStdout>,
}

impl Driver {
    fn start(java: &Path, classes: &Path) -> Driver {
        let mut child = Command::new(java)
            .arg("-cp")
            .arg(classes)
            .arg("java_sweep")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the JDK was found and then would not start");
        let out = BufReader::new(child.stdout.take().expect("stdout"));
        Driver { child, out }
    }

    fn ask(&mut self, fields: &[&str]) -> Json {
        let line = fields.join("\t");
        let stdin = self.child.stdin.as_mut().expect("stdin");
        writeln!(stdin, "{line}").expect("write to the driver");
        stdin.flush().expect("flush");
        let mut answer = String::new();
        let n = self.out.read_line(&mut answer).expect("read from the driver");
        assert!(n > 0, "the driver closed its output after: {line}");
        Json::parse(answer.trim()).unwrap_or_else(|e| panic!("the driver answered {answer:?}: {e}"))
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ── witnesses ───────────────────────────────────────────────────────────────

/// A deterministic sample value for a type, or `None` when this test has none.
///
/// Deterministic rather than random, for the reason the Python target's twin
/// gives: the property is that three implementations of one mapping agree, and
/// that does not need search to find — it needs coverage of every shape the
/// corpus declares.
///
/// **Recursive types answer `None` here and are counted.** The Python sweep
/// follows a `TypeCode::Recursive` marker one level by carrying the chain of
/// types under construction; this one does not, and the number of types that
/// costs is printed rather than left to be assumed.
fn witness(tc: &TypeCode, depth: usize) -> Option<Value> {
    if depth > 6 {
        return None;
    }
    Some(match tc {
        TypeCode::Boolean => Value::Bool(true),
        TypeCode::Octet => Value::Octet(0xA7),
        TypeCode::Char => Value::Char(b'Q'),
        TypeCode::WChar => Value::WChar('한'),
        TypeCode::Short => Value::Short(-31_000),
        TypeCode::UShort => Value::UShort(65_000),
        TypeCode::Long => Value::Long(-2_000_000_111),
        TypeCode::ULong => Value::ULong(4_000_000_222),
        // Past 2^53 on purpose: this is the value that proves the mapping's
        // rule that a 64-bit integer crosses as a string is implemented on
        // both sides.
        TypeCode::LongLong => Value::LongLong(-9_007_199_254_740_993),
        TypeCode::ULongLong => Value::ULongLong(18_014_398_509_481_985),
        TypeCode::Float => Value::Float(-0.5),
        TypeCode::Double => Value::Double(1.0 / 3.0),
        TypeCode::LongDouble => {
            Value::LongDouble([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16])
        }
        TypeCode::String(bound) => Value::String(bounded("orbweaver", *bound)),
        TypeCode::WString(bound) => Value::WString(bounded("정적 스텁", *bound)),
        // A constructed type inside the `any`, on purpose: `_t` is then AnyJSON
        // v1.1's structural form, and this target **relays** rather than
        // rebuilding it — which is exactly the claim worth measuring, since a
        // relay that lost a byte would show up here as different CDR.
        TypeCode::Any => Value::Any(Box::new(described()), Box::new(described_value())),
        TypeCode::TypeCode => Value::TypeCode(Box::new(described())),
        TypeCode::ObjRef { .. } => Value::ObjRef(Some(sample_ior())),
        TypeCode::Enum { members, .. } => Value::Enum(members.last()?.clone()),
        TypeCode::Sequence { element, .. } => Value::List(vec![witness(element, depth + 1)?]),
        TypeCode::Array { element, length } => {
            let one = witness(element, depth + 1)?;
            Value::List(vec![one; *length as usize])
        }
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
            // Declaration order, because that IS the wire order.
            let mut fields = Vec::new();
            for m in members {
                fields.push((m.name.clone(), witness(&m.tc, depth + 1)?));
            }
            Value::Struct(fields)
        }
        TypeCode::Union { discriminator, cases, .. } => {
            // The first **labelled** branch, never the default. A default
            // branch's label is empty, and reading an empty label as the number
            // zero selects whichever branch is labelled `0` — so the witness
            // carried that branch's value under this one's discriminator, and
            // `octet` met a `string`. One contract of 37 has that shape
            // (`corpus/golden/29`), which is exactly the kind of thing a
            // whole-corpus pass finds and a per-file one does not.
            let case = cases.iter().find(|c| !c.label.is_empty())?;
            let disc = label_value(&case.label, discriminator)?;
            let value = witness(&case.tc, depth + 1)?;
            Value::Union { discriminator: Box::new(disc), value: Some(Box::new(value)) }
        }
        TypeCode::Alias { aliased, .. } => witness(aliased, depth)?,
        _ => return None,
    })
}

fn bounded(text: &str, bound: u32) -> String {
    if bound == 0 {
        return text.to_owned();
    }
    text.chars().take(bound as usize).collect()
}

/// The discriminator value a case label denotes.
fn label_value(label: &[u8], disc: &TypeCode) -> Option<Value> {
    let wide = {
        let mut v: u64 = 0;
        for x in label {
            v = (v << 8) | u64::from(*x);
        }
        v
    };
    Some(match disc {
        TypeCode::Boolean => Value::Bool(label.last() == Some(&1)),
        TypeCode::Long => Value::Long(wide as i32),
        TypeCode::ULong => Value::ULong(wide as u32),
        TypeCode::Short => Value::Short(wide as i16),
        TypeCode::UShort => Value::UShort(wide as u16),
        TypeCode::Char => Value::Char(wide as u8),
        TypeCode::Octet => Value::Octet(wide as u8),
        TypeCode::LongLong => Value::LongLong(wide as i64),
        TypeCode::ULongLong => Value::ULongLong(wide),
        TypeCode::Enum { members, .. } => Value::Enum(members.get(wide as usize)?.clone()),
        // A labelless default branch: the discriminator is not any label, and
        // the lowest value of the type will do for a witness only when it is
        // not a label either. Skipped rather than guessed.
        _ => return None,
    })
}

fn described() -> TypeCode {
    TypeCode::Struct {
        id: "IDL:jw/Point:1.0".to_owned(),
        name: "Point".to_owned(),
        members: vec![
            orbweaver_giop::typecode::Member { name: "x".into(), tc: TypeCode::Long },
            orbweaver_giop::typecode::Member { name: "y".into(), tc: TypeCode::Double },
        ],
    }
}

fn described_value() -> Value {
    Value::Struct(vec![("x".to_owned(), Value::Long(7)), ("y".to_owned(), Value::Double(-0.125))])
}

fn sample_ior() -> Ior {
    Ior {
        type_id: "IDL:jw/Target:1.0".to_owned(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "127.0.0.1".to_owned(),
            port: 4242,
            object_key: b"jw".to_vec(),
            components: Vec::new(),
        }],
    }
}

/// The bytes a value encodes to, in one order.
fn bytes(tc: &TypeCode, value: &Value, endian: Endian) -> Option<Vec<u8>> {
    let mut e = Encoder::new(endian);
    encode(&mut e, tc, value).ok()?;
    e.finish().ok()
}

/// Whether a value survives Java: `to_json`, across, back, and the same CDR.
///
/// Returns the reason it did not, so a caller can cluster the failures rather
/// than stopping at the first.
fn crosses(driver: &mut Driver, tc: &TypeCode, value: &Value) -> Result<(), String> {
    // **One** table for both directions. §4.5 cannot emit an IOR, so a
    // reference crosses as a handle into the table that issued it, and a second
    // table has never heard of `local-1` — which is a fact about this test's
    // scaffolding and not about Java, and it cost four contracts' worth of
    // false failures until the sweep clustered them together.
    let mut refs = LocalReferences::new();
    let form = anyjson::tc_to_json(tc);
    let doc = anyjson::to_json(tc, value, &mut refs).map_err(|e| format!("to_json: {e}"))?;
    let answer = driver.ask(&["value", &form.to_string(), &doc.to_string()]);
    if let Some(Json::String(e)) = answer.get("error") {
        return Err(format!("the Java runtime refused it: {e}"));
    }
    let Some(back) = answer.get("value") else {
        return Err(format!("the driver answered {answer}"));
    };
    let decoded = anyjson::from_json(tc, back, &refs)
        .map_err(|e| format!("what Java wrote does not read back: {e} — {back}"))?;
    for endian in [Endian::Little, Endian::Big] {
        let want = bytes(tc, value, endian);
        let got = bytes(tc, &decoded, endian);
        if want != got {
            return Err(format!(
                "different CDR at {endian:?} after the round trip: {want:?} vs {got:?}"
            ));
        }
    }
    Ok(())
}

// ── clause 3: the refusals say the same sentences ───────────────────────────

/// The Java runtime's refusal sentences are **equal** to the published heads,
/// and a peer-fed document reaches them.
///
/// Equal rather than similar, and computed rather than retyped: the expected
/// text comes from calling the function in `orbweaver-dynamic` that owns it, so
/// a wording change fails here the moment it lands rather than at the next
/// reading. That is the pin `D030 §3` asks for, and the reason it exists is
/// measured: *the generated Python runtime once wrote its own fourth wording
/// for `fixed`, measured by nothing until it was broken on purpose.* A third
/// target triples that exposure, and Java cannot import a Rust constant either.
#[test]
fn a_peer_fed_form_reads_as_a_description_and_refuses_as_a_value() {
    let Some((javac, java)) = jdk() else {
        return unmeasured("clause 3, the refusal sentences");
    };
    let dir = tmp("refusals");
    let spec = orbweaver_idl::parse("module jr { struct Only { long v; }; };").expect("idl");
    let mut registry = Registry::new();
    registry.load(&spec).expect("loads");
    let (classes, _) = build(&javac, &registry, &dir).expect("the driver compiles");
    let mut driver = Driver::start(&java, &classes);

    // The three sentence functions, called on both sides of the boundary.
    let subject = "a witness";
    let said = driver.ask(&["words", subject]);
    for (key, want) in [
        ("deferred", orbweaver_dynamic::deferred_wire_sentence(subject)),
        ("unmarshallable", orbweaver_dynamic::unmarshallable_wire_sentence(subject)),
        ("withdrawn", orbweaver_dynamic::withdrawn_wire_sentence(subject)),
        ("principal_subject", orbweaver_dynamic::principal_subject()),
    ] {
        let got = said.get(key).and_then(Json::as_str).unwrap_or("").to_owned();
        assert_eq!(
            got, want,
            "the Java runtime's {key} sentence has drifted from the function that owns it \
             in orbweaver-dynamic"
        );
    }

    // And the same sentences reached the way a peer reaches them: an `any`
    // whose `_t` describes a construct whose value cannot cross. D008's
    // asymmetry is what makes this reachable — the description crossed.
    let cases: Vec<(&str, TypeCode, String)> = vec![
        (
            "fixed",
            TypeCode::Fixed { digits: 9, scale: 2 },
            orbweaver_dynamic::deferred_wire_sentence(&orbweaver_dynamic::fixed_subject(9, 2)),
        ),
        (
            "native",
            TypeCode::Native { id: "IDL:jr/Handle:1.0".into(), name: "Handle".into() },
            orbweaver_dynamic::unmarshallable_wire_sentence(&orbweaver_dynamic::native_subject(
                "Handle",
                "IDL:jr/Handle:1.0",
            )),
        ),
        (
            "principal",
            TypeCode::Principal,
            orbweaver_dynamic::withdrawn_wire_sentence(&orbweaver_dynamic::principal_subject()),
        ),
        (
            "abstract interface",
            TypeCode::AbstractInterface {
                id: "IDL:jr/Describable:1.0".into(),
                name: "Describable".into(),
            },
            orbweaver_dynamic::deferred_wire_sentence(
                &orbweaver_dynamic::abstract_interface_subject(
                    "Describable",
                    "IDL:jr/Describable:1.0",
                ),
            ),
        ),
    ];
    for (what, tc, want) in cases {
        let form = anyjson::tc_to_json(&tc);
        let answer = driver.ask(&["open", &form.to_string(), "0"]);
        let refused = answer.get("refused").and_then(Json::as_str).unwrap_or("").to_owned();
        assert!(
            refused.contains(&want),
            "a peer-fed {what} was answered {answer}\n  expected the published sentence: {want}"
        );
    }
}

/// The **emitter's** refusals are the same sentences, for the same five
/// families — the half a caller meets before any document arrives.
#[test]
fn the_emitter_refuses_a_deferred_construct_in_the_published_words() {
    let spec = orbweaver_idl::parse(
        "module je {\n\
           native Handle;\n\
           struct WithFixed { fixed<9,2> amount; };\n\
           struct WithNative { Handle h; };\n\
           struct WithPrincipal { ::CORBA::Principal who; };\n\
         };",
    )
    .expect("idl");
    let mut registry = Registry::new();
    registry.load(&spec).expect("loads");
    let generated = emit_java(&registry, "contract");
    let reasons: String =
        generated.skipped.iter().map(|(id, why)| format!("{id}: {why}\n")).collect();
    for want in [
        orbweaver_dynamic::deferred_wire_head(&orbweaver_dynamic::fixed_subject(9, 2)),
        orbweaver_dynamic::unmarshallable_wire_head(&orbweaver_dynamic::native_subject(
            "Handle",
            "IDL:je/Handle:1.0",
        )),
        orbweaver_dynamic::withdrawn_wire_head(&orbweaver_dynamic::principal_subject()),
    ] {
        assert!(
            reasons.contains(&want),
            "the Java emitter skipped something without the published head.\n\
             expected to find: {want}\n\
             what it said instead:\n{reasons}"
        );
    }
}

// ── clause 4: exceptions and the names a caller reaches ─────────────────────

/// A generated stub raises **what the reply named**, as the class the emitter
/// wrote, with its members decoded.
#[test]
fn a_generated_stub_raises_what_the_reply_names() {
    let Some((javac, java)) = jdk() else {
        return unmeasured("clause 4, an exception arrives as itself");
    };
    let dir = tmp("faults");
    let spec = orbweaver_idl::parse(
        "module jf {\n\
           exception Insufficient { long shortfall; string ledger; };\n\
           interface Teller { void withdraw(in long amount) raises (Insufficient); };\n\
         };",
    )
    .expect("idl");
    let mut registry = Registry::new();
    registry.load(&spec).expect("loads");
    let (classes, _) = build(&javac, &registry, &dir).expect("compiles");
    let mut driver = Driver::start(&java, &classes);

    let reply = "{\"user_exception\":{\"id\":\"IDL:jf/Insufficient:1.0\",\
                 \"members\":{\"shortfall\":25,\"ledger\":\"main\"}}}";
    let answer = driver.ask(&[
        "call",
        "contract.jf.Teller",
        "withdraw",
        "[{\"t\":\"long\",\"v\":100}]",
        reply,
        "void",
    ]);
    assert_eq!(
        answer.get("id").and_then(Json::as_str),
        Some("IDL:jf/Insufficient:1.0"),
        "the stub did not raise the exception the reply named: {answer}"
    );
    assert_eq!(
        answer.get("raised").and_then(Json::as_str),
        Some("contract.jf.Insufficient"),
        "the exception arrived as something other than its own generated class: {answer}"
    );
    let members = answer.get("members").expect("the members came back");
    assert_eq!(
        members.get("shortfall").map(ToString::to_string).unwrap_or_default(),
        "25",
        "the exception's members did not decode: {answer}"
    );

    // A system exception is not a user one, and an id the package never heard
    // of is neither: §4.11.4's ordinal is what a caller reads, and this is the
    // one path a runtime writes itself.
    let unknown = "{\"user_exception\":{\"id\":\"IDL:jf/Nobody:1.0\",\"members\":{}}}";
    let answer = driver.ask(&[
        "call",
        "contract.jf.Teller",
        "withdraw",
        "[{\"t\":\"long\",\"v\":1}]",
        unknown,
        "void",
    ]);
    assert_eq!(
        answer.get("raised").and_then(Json::as_str),
        Some("contract._Rt$SystemException"),
        "an undecodable user exception should arrive as a system exception: {answer}"
    );
}

/// Every name the emitted tree writes, and every name a caller reaches one by,
/// goes through [`java_name`] — measured by *executing* the package.
///
/// The three positions that differ in Java and in no other target: an IDL
/// module is a **package**, which is a directory and a `package` line and every
/// qualified reference; an interface that contains a type is a class **and** a
/// package, which Java forbids, so its nested scope is `<Name>Package`; and a
/// contextual keyword is legal as a variable and fatal as a class name.
#[test]
fn a_stub_is_looked_up_by_the_name_the_emitter_gave_it() {
    let Some((javac, java)) = jdk() else {
        return unmeasured("clause 4, the names the emitter gave");
    };
    let dir = tmp("names");
    let spec = orbweaver_idl::parse(
        "module _package {\n\
           interface _class {\n\
             struct Ticket { long id; };\n\
             Ticket issue(in long _final);\n\
           };\n\
         };",
    )
    .expect("idl");
    let mut registry = Registry::new();
    registry.load(&spec).expect("loads");
    let (classes, _) = build(&javac, &registry, &dir).expect("compiles");
    let mut driver = Driver::start(&java, &classes);

    assert_eq!(java_name("package"), "_package");
    assert_eq!(java_name("class"), "_class");
    assert_eq!(java_name("final"), "_final");

    let reply = "{\"ok\":{\"returns\":{\"id\":7},\"outputs\":{}}}";
    let answer = driver.ask(&[
        "call",
        // The module is `_package`, the interface `_class`, and the nested
        // struct lives in `_classPackage` — none of which a caller could guess
        // from the IDL without the mapping.
        "contract._package._class",
        "issue",
        "[{\"t\":\"long\",\"v\":3}]",
        reply,
        "{\"kind\":\"struct\",\"id\":\"IDL:package/class/Ticket:1.0\",\"name\":\"Ticket\",\
          \"members\":[{\"name\":\"id\",\"type\":\"long\"}]}",
    ]);
    assert!(answer.get("error").is_none(), "the stub could not be reached: {answer}");
    assert_eq!(
        answer.get("request").and_then(|r| r.get("op")).and_then(Json::as_str),
        Some("issue"),
        "the operation name that travels is the IDL one: {answer}"
    );
    assert_eq!(
        answer.get("returned").and_then(|r| r.get("id")).map(ToString::to_string),
        Some("7".to_owned()),
        "the nested struct did not come back through _classPackage: {answer}"
    );
}

// ── clause 1, self: the whole corpus at once ────────────────────────────────

/// Every value and every call the golden corpus declares, through Java and
/// back, held to the Rust mapping in both byte orders.
///
/// This is the batch: one pass over the whole corpus, and the failures are
/// clustered by cause rather than reported one at a time, because *"item-by-item
/// work would have produced seven separate patches and never surfaced the
/// rule."*
#[test]
fn the_java_mapping_agrees_with_the_rust_one_over_the_golden_corpus() {
    let Some((javac, java)) = jdk() else {
        return unmeasured("clause 1, the cross-implementation sweep");
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(root().join("corpus/golden"))
        .expect("corpus/golden")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "idl"))
        .collect();
    files.sort();

    let mut values = 0usize;
    let mut calls = 0usize;
    let mut no_witness = 0usize;
    let mut refused = 0usize;
    let mut not_driven = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for path in &files {
        let stem = path.file_stem().expect("stem").to_string_lossy().replace(['-', '.'], "_");
        let src = std::fs::read_to_string(path).expect("read");
        let spec = orbweaver_idl::parse(&src).expect("golden parses");
        let mut registry = Registry::new();
        if registry.load(&spec).is_err() {
            continue;
        }
        let dir = tmp(&format!("sweep/{stem}"));
        let (classes, skipped) = match build(&javac, &registry, &dir) {
            Ok(built) => built,
            Err(why) => {
                failures.push(format!("{}: {why}", path.display()));
                continue;
            }
        };
        let mut driver = Driver::start(&java, &classes);

        // Values.
        for id in registry.ids() {
            if skipped.contains(id) {
                // Refused by the emitter with a published sentence, which is a
                // different fact from "this sweep has no witness" and is
                // counted separately: one is a decision, the other a limit of
                // the instrument.
                refused += 1;
                continue;
            }
            let Some(Entry::Type(tc)) = registry.get(id) else { continue };
            if !matches!(
                tc,
                TypeCode::Struct { .. }
                    | TypeCode::Union { .. }
                    | TypeCode::Enum { .. }
                    | TypeCode::Except { .. }
                    | TypeCode::Alias { .. }
            ) {
                continue;
            }
            let Some(value) = witness(tc, 0) else {
                no_witness += 1;
                continue;
            };
            match crosses(&mut driver, tc, &value) {
                Ok(()) => values += 1,
                Err(why) => failures.push(format!("{id}: {why}")),
            }
        }

        // Calls: every operation of every interface the emitter kept.
        for id in registry.ids() {
            let Some(Entry::Interface(entry)) = registry.get(id) else { continue };
            if entry.abstract_interface || skipped.contains(id) {
                continue;
            }
            let path_segs = registry.qualified_name(id).unwrap_or_default().to_owned();
            let mut segs: Vec<String> = path_segs.split("::").map(java_name).collect::<Vec<_>>();
            let Some(class_name) = segs.pop() else { continue };
            let class = if segs.is_empty() {
                format!("contract.{class_name}")
            } else {
                format!("contract.{}.{class_name}", segs.join("."))
            };
            for (op, sig) in orbweaver_gen::python::client_operations(&registry, id) {
                // One table for the whole call — arguments, reply and the
                // comparison — for the reason `crosses` keeps one: a handle is
                // only meaningful to the table that issued it.
                let mut refs = LocalReferences::new();
                let mut args = Vec::new();
                let mut ok = true;
                for p in &sig.params {
                    if !matches!(p.direction, ParamDirection::In | ParamDirection::InOut) {
                        continue;
                    }
                    let Some(value) = witness(&p.tc, 0) else {
                        ok = false;
                        break;
                    };
                    let Ok(doc) = anyjson::to_json(&p.tc, &value, &mut refs) else {
                        ok = false;
                        break;
                    };
                    args.push((p.name.clone(), p.tc.clone(), value, doc));
                }
                let outs: Vec<_> = sig
                    .params
                    .iter()
                    .filter(|p| matches!(p.direction, ParamDirection::Out | ParamDirection::InOut))
                    .collect();
                if !ok {
                    not_driven += 1;
                    continue;
                }
                // The reply the Loopback answers with, and the one value this
                // sweep compares on the way back. A multi-value operation's
                // holder is not read here, and the count says so.
                let returns_void = matches!(sig.returns, TypeCode::Void | TypeCode::Null);
                let single: Option<(TypeCode, Value)> = if !returns_void && outs.is_empty() {
                    witness(&sig.returns, 0).map(|v| (sig.returns.clone(), v))
                } else if returns_void && outs.len() == 1 {
                    witness(&outs[0].tc, 0).map(|v| (outs[0].tc.clone(), v))
                } else {
                    None
                };
                if !returns_void && single.is_none() && outs.is_empty() {
                    not_driven += 1;
                    continue;
                }

                let mut ok_body = BTreeMap::new();
                let returns_json = if returns_void {
                    Json::Null
                } else {
                    match witness(&sig.returns, 0)
                        .and_then(|v| anyjson::to_json(&sig.returns, &v, &mut refs).ok())
                    {
                        Some(j) => j,
                        None => {
                            not_driven += 1;
                            continue;
                        }
                    }
                };
                ok_body.insert("returns".to_owned(), returns_json);
                let mut outputs = BTreeMap::new();
                let mut missing_out = false;
                for p in &outs {
                    match witness(&p.tc, 0)
                        .and_then(|v| anyjson::to_json(&p.tc, &v, &mut refs).ok())
                    {
                        Some(j) => {
                            outputs.insert(p.name.clone(), j);
                        }
                        None => missing_out = true,
                    }
                }
                if missing_out {
                    not_driven += 1;
                    continue;
                }
                ok_body.insert("outputs".to_owned(), Json::Object(outputs));
                let reply =
                    Json::Object(BTreeMap::from([("ok".to_owned(), Json::Object(ok_body))]))
                        .to_string();

                let arg_docs = Json::Array(
                    args.iter()
                        .map(|(_, tc, _, doc)| {
                            Json::Object(BTreeMap::from([
                                ("t".to_owned(), anyjson::tc_to_json(tc)),
                                ("v".to_owned(), doc.clone()),
                            ]))
                        })
                        .collect(),
                )
                .to_string();
                let returns_form = match &single {
                    Some((tc, _)) => anyjson::tc_to_json(tc).to_string(),
                    None => "void".to_owned(),
                };
                if single.is_none() {
                    not_driven += 1;
                }

                let method = if let Some(attr) = op.strip_prefix("_get_") {
                    java_name(attr)
                } else if let Some(attr) = op.strip_prefix("_set_") {
                    java_name(attr)
                } else {
                    java_name(&op)
                };
                let answer =
                    driver.ask(&["call", &class, &method, &arg_docs, &reply, &returns_form]);
                if let Some(Json::String(e)) = answer.get("error") {
                    failures.push(format!("{id}::{op}: the driver could not call it: {e}"));
                    continue;
                }
                let Some(request) = answer.get("request") else {
                    failures.push(format!("{id}::{op}: no request was recorded: {answer}"));
                    continue;
                };
                if request.get("id").and_then(Json::as_str) != Some(id.as_str()) {
                    failures.push(format!("{id}::{op}: the stub sent id {request}"));
                    continue;
                }
                if request.get("op").and_then(Json::as_str) != Some(op.as_str()) {
                    failures.push(format!("{id}::{op}: the stub sent op {request}"));
                    continue;
                }
                let mut bad = false;
                for (name, tc, value, _) in &args {
                    let Some(sent) = request.get("args").and_then(|a| a.get(name)) else {
                        failures.push(format!("{id}::{op}: the request has no {name}"));
                        bad = true;
                        break;
                    };
                    match anyjson::from_json(tc, sent, &refs) {
                        Ok(back) => {
                            for endian in [Endian::Little, Endian::Big] {
                                if bytes(tc, value, endian) != bytes(tc, &back, endian) {
                                    failures.push(format!(
                                        "{id}::{op}: argument {name} came out as different CDR"
                                    ));
                                    bad = true;
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            failures.push(format!("{id}::{op}: argument {name} unreadable: {e}"));
                            bad = true;
                        }
                    }
                    if bad {
                        break;
                    }
                }
                if bad {
                    continue;
                }
                if let Some((tc, value)) = &single {
                    match answer.get("returned") {
                        Some(back) => match anyjson::from_json(tc, back, &refs) {
                            Ok(decoded) => {
                                for endian in [Endian::Little, Endian::Big] {
                                    if bytes(tc, value, endian) != bytes(tc, &decoded, endian) {
                                        failures.push(format!(
                                            "{id}::{op}: the reply's value came back as \
                                                 different CDR"
                                        ));
                                        bad = true;
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                failures.push(format!("{id}::{op}: the reply's value: {e}"));
                                bad = true;
                            }
                        },
                        None => {
                            failures.push(format!("{id}::{op}: nothing came back: {answer}"));
                            bad = true;
                        }
                    }
                }
                if !bad {
                    calls += 1;
                }
            }
        }
    }

    // The counts are printed, never pinned. `A floor is not a figure`: a
    // `>= N` here would prove nothing about which values crossed, and would
    // stay green while one was swapped for another.
    println!(
        "java target: {values} value(s) and {calls} call(s) crossed, both byte orders, over \
         {} golden contract(s)",
        files.len()
    );
    println!(
        "java target: not measured here — {refused} item(s) the emitter refused with a \
         published sentence, {no_witness} type(s) this sweep has no witness for (recursive \
         types, and unions with no labelled branch), {not_driven} operation(s) whose \
         arguments or whose multi-value result this driver does not build"
    );
    assert!(
        failures.is_empty(),
        "the Java mapping and the Rust one disagree in {} place(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(values > 0 && calls > 0, "the sweep measured nothing at all, which is a failure");
}
