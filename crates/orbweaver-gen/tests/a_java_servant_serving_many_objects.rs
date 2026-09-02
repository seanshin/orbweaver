//! A Java servant serving many objects can tell which one it was addressed to.
//!
//! # Why this is a leak test and not a feature test
//!
//! `ForeignServant::with_home` says what it changes, and the first of the three
//! is *every call document carries `CALL_OBJECT`, so the far side knows which
//! object it is*. The Rust side has put that key in every call document since
//! homes existed. A **Rust** servant reads it through `<I>Target::oid()`; a
//! **Python** one through `own_oid()`; a **Java** one could not read it at all
//! until 2026-09-02, because its `Servant` interface had no member for it and
//! `dispatchCall` never looked.
//!
//! So a Java servant answered **every object of its interface identically**,
//! and a caller holding two references to two objects of one interface could
//! tell — from what the servant could *do*, not from an address — that it was
//! written in Java. That is D029 §6.1's Language row, and it is why this is
//! ranked as a leak rather than as the capability it looks like.
//!
//! It was found by writing `_Rt.seamProtocol()`: a document that must state
//! **what the file reads** could not honestly publish `call.object`, and the
//! absence was the leak wearing a missing key.
//!
//! # What is measured
//!
//! One Java servant under one home, addressed twice with the keys of two
//! different objects. It answers with its own oid, so the two replies must
//! **differ** and must name the oids that were addressed.
//!
//! A servant with **no** home sees `""` — the default object — which is the
//! other half: "" is not an absence a servant author needs a rule about.
//!
//! *많은 객체를 서비스하는 자바 서번트가 자기가 어느 객체로 불렸는지 안다. 러스트는
//! `<I>Target::oid()`로, 파이썬은 `own_oid()`로 읽던 것을 자바는 2026-09-02까지 아예
//! 읽지 못했고, 그래서 **모든 객체에 똑같이 답했다** — 호출자가 참조 둘을 들고
//! 서번트가 **할 수 있는 일**로부터 그것이 자바임을 알 수 있었다. D029 §6.1의 언어
//! 행이며, 기능처럼 보이지만 누출로 순위가 매겨진 이유다.*

use std::path::{Path, PathBuf};
use std::process::Command;

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_gen::pychild::SeamChild;
use orbweaver_gen::rt::ObjectHome;
use orbweaver_gen::seam::{ForeignServant, key_infix_of};
use orbweaver_giop::server::{Dispatch, decode_request};
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, Version, encode_request, read_message};
use orbweaver_registry::{Contract, Registry, Strictness};

const PACKAGE: &str = "echomany";
const TYPE_ID: &str = "IDL:spike/Echo:1.0";
const HOST: &str = "127.0.0.1";
const PORT: u16 = 4242;
const ROOT: &[u8] = b"echo";

/// The two objects, and neither name is a prefix of the other: a scheme that
/// truncated would still tell these apart wrongly rather than not at all.
const FIRST: &str = "alpha";
const SECOND: &str = "bravo";

fn registry() -> Registry {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spikes/echo.idl");
    let contract = Contract::load(&path, &Default::default(), Strictness::Checked)
        .expect("the echo contract must load");
    let mut r = Registry::new();
    r.load(&contract.spec).expect("the contract must build a registry");
    r
}

fn home() -> ObjectHome {
    ObjectHome::new(HOST, PORT, ROOT.to_vec())
}

/// The key for one object, spelled the way the seam's identity spells it.
fn key(oid: &str) -> Vec<u8> {
    home().key_of(&key_infix_of(TYPE_ID), oid)
}

fn jdk() -> Option<(PathBuf, PathBuf)> {
    let home = std::env::var("ORBWEAVER_JAVA_HOME").unwrap_or_else(|_| {
        "/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home".into()
    });
    let javac = Path::new(&home).join("bin/javac");
    let java = Path::new(&home).join("bin/java");
    (javac.is_file() && java.is_file()).then_some((javac, java))
}

/// The Java servant: it answers with the oid it was addressed as.
///
/// `echo_string` ignores its argument on purpose — what is being read back is
/// the servant's own answer to *which object am I*, and mixing the argument in
/// would let a servant that echoed pass without ever consulting it.
const SERVANT: &str = r#"
import echomany._Rt;
import echomany.spike.EchoServant;

public final class Many extends EchoServant {
    @Override
    public String echo_string(String msg) {
        return "oid=" + ownOid();
    }

    public static void main(String[] argv) throws Exception {
        _Rt.serveOnPipes(new Many());
    }
}
"#;

fn build(javac: &Path, dir: &Path) -> PathBuf {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("a work directory");
    let generated = orbweaver_gen::java::emit_java(&registry(), PACKAGE);
    let mut files = Vec::new();
    for (name, text) in &generated.files {
        let at = src.join(name);
        std::fs::create_dir_all(at.parent().expect("a parent")).expect("mkdir");
        std::fs::write(&at, text).expect("write");
        files.push(at);
    }
    let servant = src.join("Many.java");
    std::fs::write(&servant, SERVANT).expect("write the servant");
    files.push(servant);

    let classes = dir.join("classes");
    let mut cmd = Command::new(javac);
    cmd.arg("-nowarn").arg("-encoding").arg("UTF-8").arg("-d").arg(&classes);
    for f in &files {
        cmd.arg(f);
    }
    let built = cmd.output().expect("javac runs");
    assert!(
        built.status.success(),
        "javac refused the servant:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );
    classes
}

/// Sends `echo_string("x")` to `object_key` and returns what came back.
fn ask(servant: &mut ForeignServant<SeamChild>, object_key: &[u8]) -> String {
    let wire =
        encode_request(Version::V1_2, Endian::Big, 1, object_key, "echo_string", true, |e| {
            e.put_str("x");
        })
        .expect("encode");
    let mut cursor: &[u8] = &wire;
    let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("frame");
    let request = decode_request(msg).expect("decode");
    let mut out = Encoder::new(Endian::Big);
    servant.dispatch(&request, &mut out).expect("the Java servant answered");
    let mut body = Decoder::new(out.as_bytes(), Endian::Big);
    body.get_string().expect("a string")
}

const NO_JDK: &str = "SKIPPED  no JDK — set ORBWEAVER_JAVA_HOME. Whether a Java servant can tell \
                      which object it was addressed to is UNMEASURED here, not passing.";

/// The leg: two objects, one Java servant, two different answers.
#[test]
fn a_java_servant_tells_its_objects_apart() {
    let Some((javac, java)) = jdk() else {
        eprintln!("{NO_JDK}");
        return;
    };
    let dir = std::env::temp_dir().join(format!("orbweaver-jmany-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let classes = build(&javac, &dir);

    let child = SeamChild::java(&java, &classes, "Many").expect("the JDK starts");
    let mut servant = ForeignServant::new(&registry(), TYPE_ID, child)
        .expect("the contract names Echo")
        .with_home(home());

    let first = ask(&mut servant, &key(FIRST));
    let second = ask(&mut servant, &key(SECOND));
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(
        first,
        format!("oid={FIRST}"),
        "the servant did not read the oid it was addressed as. Before 2026-09-02 it answered \
         `oid=` for every object, which is what a caller could tell it apart from a Rust or \
         Python servant by"
    );
    assert_eq!(second, format!("oid={SECOND}"), "the second object's oid did not arrive either");
    assert_ne!(
        first, second,
        "one servant answered two different objects identically. That is the leak this file \
         exists for: a caller holding two references could tell, from what the servant can do, \
         that it is not a Rust or a Python one"
    );
}

/// The other half: a servant with no home sees the default object.
///
/// `""` is what a servant serving one object always sees, so there is no
/// absence for a servant author to have a rule about — the same sentence the
/// Python runtime carries. Without this, the assertion above would be equally
/// satisfied by a runtime that invented an oid when it had none.
#[test]
fn a_java_servant_with_no_home_sees_the_default_object() {
    let Some((javac, java)) = jdk() else {
        eprintln!("{NO_JDK}");
        return;
    };
    let dir = std::env::temp_dir().join(format!("orbweaver-jmany-home-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let classes = build(&javac, &dir);

    let child = SeamChild::java(&java, &classes, "Many").expect("the JDK starts");
    // No `with_home`: one object, and the seam says so with "".
    let mut servant =
        ForeignServant::new(&registry(), TYPE_ID, child).expect("the contract names Echo");

    let answer = ask(&mut servant, ROOT);
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(
        answer, "oid=",
        "a servant with no home answered {answer:?}. The default object is \"\", and a runtime \
         that invented something here would make the assertion in the test above pass without \
         the oid having travelled"
    );
}
