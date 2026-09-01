//! A foreign peer drives a **Java** servant behind our ORB.
//!
//! # Why this comes before the `servant × self` cell
//!
//! `spikes/bindings/java.manifest` refused the cheap one first, and said why:
//!
//! > This one would be the cheapest of the three and is still not supplied,
//! > because **a self cell that existed while the foreign ones did not would
//! > report a seam we had never run against anybody else.**
//!
//! That is an ordering the manifest chose and this file honours: a foreign peer
//! first. Our own client on both ends can agree on a convention neither end
//! would keep against a stranger — the argument this repository makes about
//! captures, one row up.
//!
//! # The route, and why it is not the Python one
//!
//! The Python servant cells reach their servant through
//! `orbweaver-py-bridge --serve`, **which binds its own listener**. There is no
//! `orbweaver-java-bridge` and this does not want one: since
//! `SeamChild::java`, a Java servant mounts as a plain `Dispatch` in a server
//! **this test binds**, which is strictly better for what the language row
//! measures. The servant arrives as a servant rather than as an endpoint, so a
//! caller is not sent anywhere — and a caller sent elsewhere has been *moved*,
//! which is a different row of D029 §6.1.
//!
//! # What is measured and what is claimed
//!
//! omniORB's own Python client dials the IOR and calls. The exchange is
//! little-endian because omniORB writes its host's native order and our server
//! replies in the request's — **a sound inference and still not a reading**, so
//! the cell reports `claimed` and not `observed`, exactly as the Python
//! `servant × omniorb` cell does and for the same reason. The JacORB row is
//! where a flag byte gets read; it is not written yet and is not claimed here.
//!
//! *매니페스트가 싼 것을 먼저 하기를 거절했고 이유를 적었다 — 외래 피어에게 한 번도
//! 돌려보지 않은 seam을 self 칸이 보고하게 된다. 그 순서를 지킨다. 경로는 파이썬의
//! 것이 아니다: 자바 서번트는 **이 테스트가 바인드한** 서버에 평범한 `Dispatch`로
//! 올라가므로, 서번트가 엔드포인트가 아니라 서번트로 도착한다.*

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use orbweaver_gen::pychild::SeamChild;
use orbweaver_gen::seam::ForeignServant;
use orbweaver_giop::orb::Orb;
use orbweaver_registry::{Contract, Registry, Strictness};

const PACKAGE: &str = "echo";
const TYPE_ID: &str = "IDL:spike/Echo:1.0";
const ROOT: &[u8] = b"echo";

fn contract() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spikes/echo.idl")
}

fn registry() -> Registry {
    let spec = Contract::load(&contract(), &Default::default(), Strictness::Checked)
        .expect("the echo contract must load");
    let mut r = Registry::new();
    r.load(&spec.spec).expect("the contract must build a registry");
    r
}

fn jdk() -> Option<(PathBuf, PathBuf)> {
    let home = std::env::var("ORBWEAVER_JAVA_HOME").unwrap_or_else(|_| {
        "/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home".into()
    });
    let javac = Path::new(&home).join("bin/javac");
    let java = Path::new(&home).join("bin/java");
    (javac.is_file() && java.is_file()).then_some((javac, java))
}

fn omniorb_available() -> bool {
    Command::new("python3")
        .args(["-c", "import omniORB"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The Java servant. Answers with values a default could not produce.
const SERVANT: &str = r#"
import echo._Rt;
import echo.spike.EchoServant;

public final class Node extends EchoServant {
    @Override
    public int add(int a, int b) {
        return a + b;
    }

    @Override
    public String echo_string(String msg) {
        return "java:" + msg;
    }

    public static void main(String[] argv) throws Exception {
        _Rt.serveOnPipes(new Node());
    }
}
"#;

/// omniORB's own client. It imports the contract through `omniidl` and narrows,
/// so a servant that answered the right bytes under the wrong interface would
/// fail here rather than pass.
const OMNIORB_DRIVER: &str = r#"
import sys

import CORBA
import omniORB

omniORB.importIDL(sys.argv[2])
import spike

orb = CORBA.ORB_init(sys.argv[3:], CORBA.ORB_ID)
echo = orb.string_to_object(open(sys.argv[1]).read().strip())._narrow(spike.Echo)
if echo is None:
    print("narrow failed")
    raise SystemExit(1)

print("add -> %d" % (echo.add(40, 2),))
print("echo_string -> %s" % (echo.echo_string("hello"),))
print("is_a Echo -> %s" % (echo._is_a("IDL:spike/Echo:1.0"),))
"#;

/// Compiles the emitter's Java plus the servant, and returns the classes dir.
fn build(javac: &Path, dir: &Path) -> PathBuf {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("mkdir");
    let generated = orbweaver_gen::java::emit_java(&registry(), PACKAGE);
    let mut files = Vec::new();
    for (name, text) in &generated.files {
        let at = src.join(name);
        std::fs::create_dir_all(at.parent().expect("a parent")).expect("mkdir");
        std::fs::write(&at, text).expect("write");
        files.push(at);
    }
    let node = src.join("Node.java");
    std::fs::write(&node, SERVANT).expect("write the servant");
    files.push(node);

    let classes = dir.join("classes");
    let mut cmd = Command::new(javac);
    cmd.arg("-nowarn").arg("-encoding").arg("UTF-8").arg("-d").arg(&classes);
    for f in &files {
        cmd.arg(f);
    }
    let out = cmd.output().expect("javac runs");
    assert!(
        out.status.success(),
        "javac refused what the emitter wrote:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    classes
}

#[test]
fn omniorb_calls_a_java_servant() {
    let Some((javac, java)) = jdk() else {
        println!(
            "UNMEASURED: no JDK — set ORBWEAVER_JAVA_HOME. A foreign peer driving a Java \
             servant is unmeasured, not passing."
        );
        return;
    };
    if !omniorb_available() {
        println!(
            "UNMEASURED: omniORB's Python bindings are not importable, so the peer half of \
             D030 §3 was not measured for the Java servant direction"
        );
        return;
    }

    let dir = std::env::temp_dir().join(format!("orbweaver-java-wire-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a work directory");
    let classes = build(&javac, &dir);

    // The server this test owns. The Java servant is its `Dispatch`; there is
    // no second listener anywhere in this picture.
    let server = Orb::new().server("127.0.0.1:0", ROOT.to_vec()).expect("bind");
    let ior = server.ior(TYPE_ID, "127.0.0.1").expect("ior");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let classes_for_thread = classes.clone();
    let java_for_thread = java.clone();
    let serving = std::thread::spawn(move || {
        let child = SeamChild::java(&java_for_thread, &classes_for_thread, "Node")
            .expect("the JDK was found and then would not start");
        let mut servant =
            ForeignServant::new(&registry(), TYPE_ID, child).expect("the contract names Echo");
        let _ = server.serve(&mut servant, || flag.load(Ordering::SeqCst));
    });

    let idl = dir.join("echo.idl");
    std::fs::copy(contract(), &idl).expect("copy the contract");
    let script = dir.join("drive.py");
    std::fs::write(&script, OMNIORB_DRIVER).expect("write the driver");
    let ior_path = dir.join("echo.ior");
    let mut f = std::fs::File::create(&ior_path).expect("create");
    write!(f, "{}", ior.to_stringified().expect("stringify")).expect("write the ior");
    drop(f);

    let out = Command::new("python3")
        .arg(&script)
        .arg(&ior_path)
        .arg(&idl)
        .current_dir(&dir)
        .output()
        .expect("run omniORB's client");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    stop.store(true, Ordering::SeqCst);
    let _ = orbweaver_giop::Connection::connect(&ior, std::time::Duration::from_millis(500));
    let _ = serving.join();

    for wanted in ["add -> 42", "echo_string -> java:hello", "is_a Echo -> True"] {
        assert!(
            stdout.contains(wanted),
            "omniORB's client did not see {wanted:?} from the Java servant.\n\
             stdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    let _ = std::fs::remove_dir_all(&dir);
}
