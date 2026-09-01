//! A Java servant mounted as a `Dispatch` in a server **this process owns**.
//!
//! # What this closes
//!
//! `spikes/bindings/java.manifest` carried three `waits` rows —
//! `servant × self`, `servant × omniorb`, `servant × jacorb` — and all three
//! named the same blocker: a Java servant needs the bridge's serving direction.
//! Half of that landed on 2026-09-01 (`_Rt.dispatchCall` and the generated
//! `<Name>Servant`), and `spikes/java_servant_half.sh` measured it with no
//! process in sight while saying plainly that **it was not a cell**.
//!
//! This is the other half: `java` as a child of *this* process, answering seam
//! documents on its own pipes through `_Rt.serveOnPipes`, wrapped by
//! `seam::ForeignServant` into a plain `Dispatch`. No listener, no address —
//! which is what keeps a language swap a language swap. A caller sent to a
//! different endpoint has been **moved**, and *location* and *language* are
//! different rows of D029 §6.1.
//!
//! # One child type, not one per language
//!
//! `SeamChild` was `PythonChild` until this test needed it. Only the command is
//! language-specific; the document framing, D038's re-entrancy loop, the process
//! group and the `Drop` that reaps a tree are the seam's. A `JavaChild` beside a
//! `PythonChild` would have been a second copy of `read_answer` — two loops that
//! must stay in step with one protocol. **The rename touched the constructor and
//! nothing else**, which is the evidence that the split was in the right place.
//!
//! # What it measures, and what it does not
//!
//! It measures that the route works end to end: a request built the way a peer
//! builds one — encoded and decoded — reaches a Java object and comes back with
//! that object's value. It is **not** the whole `servant × self` cell as the
//! suite counts it, and the manifest row stays a `waits` until the cell script
//! that drives the acceptance grid exists; claiming otherwise would be the
//! *green because nothing happened* shape one level up.
//!
//! *`java`를 이 프로세스의 자식으로 띄우고 `ForeignServant`로 감싸 평범한
//! `Dispatch`로 만든다. 리스너도 주소도 없다 — 그래야 언어 교체가 이동이 아니다.
//! 자식 타입은 하나다: 언어마다 다른 것은 명령뿐이고, 따로 두면 중첩 요청 루프가
//! 두 벌이 된다.*

use std::path::{Path, PathBuf};
use std::process::Command;

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_gen::pychild::SeamChild;
use orbweaver_gen::seam::ForeignServant;
use orbweaver_giop::server::{Dispatch, decode_request};
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, Version, encode_request, read_message};
use orbweaver_registry::{Contract, Registry, Strictness};

const PACKAGE: &str = "echo";
const TYPE_ID: &str = "IDL:spike/Echo:1.0";
/// What the Java servant answers `add` with is `a + b`; these are chosen so the
/// sum is not a value a default-constructed anything would produce.
const A: i32 = 40;
const B: i32 = 2;

fn registry() -> Registry {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spikes/echo.idl");
    let contract = Contract::load(&path, &Default::default(), Strictness::Checked)
        .expect("the echo contract must load");
    let mut r = Registry::new();
    r.load(&contract.spec).expect("the contract must build a registry");
    r
}

/// `javac` and `java`, or `None` — the same discipline `java_target.rs` uses:
/// an absent JDK is a skip a reader can see, never a silent pass.
fn jdk() -> Option<(PathBuf, PathBuf)> {
    let home = std::env::var("ORBWEAVER_JAVA_HOME").unwrap_or_else(|_| {
        "/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home".into()
    });
    let javac = Path::new(&home).join("bin/javac");
    let java = Path::new(&home).join("bin/java");
    (javac.is_file() && java.is_file()).then_some((javac, java))
}

/// The Java servant: a subclass that implements one operation.
const SERVANT: &str = r#"
import echo._Rt;
import echo.spike.EchoServant;

public final class Node extends EchoServant {
    @Override
    public int add(int a, int b) {
        return a + b;
    }

    public static void main(String[] argv) throws Exception {
        _Rt.serveOnPipes(new Node());
    }
}
"#;

#[test]
fn a_java_servant_answers_through_a_dispatch_this_process_holds() {
    let Some((javac, java)) = jdk() else {
        eprintln!(
            "SKIPPED  no JDK — set ORBWEAVER_JAVA_HOME. A Java servant behind a Dispatch is \
             UNMEASURED here, not passing."
        );
        return;
    };

    let dir = std::env::temp_dir().join(format!("orbweaver-javachild-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
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
    let servant = src.join("Node.java");
    std::fs::write(&servant, SERVANT).expect("write the servant");
    files.push(servant);

    let classes = dir.join("classes");
    let mut cmd = Command::new(&javac);
    cmd.arg("-nowarn").arg("-encoding").arg("UTF-8").arg("-d").arg(&classes);
    for f in &files {
        cmd.arg(f);
    }
    let built = cmd.output().expect("javac runs");
    assert!(
        built.status.success(),
        "javac refused the servant base the emitter wrote:\n{}",
        String::from_utf8_lossy(&built.stderr)
    );

    let child = SeamChild::java(&java, &classes, "Node")
        .expect("the JDK was found and then would not start");
    let mut servant =
        ForeignServant::new(&registry(), TYPE_ID, child).expect("the contract names Echo");

    // Built the way a peer builds one — encoded and decoded — so the servant is
    // handed the same shape a socket would hand it.
    let wire = encode_request(Version::V1_2, Endian::Big, 1, b"echo", "add", true, |e| {
        e.put_i32(A);
        e.put_i32(B);
    })
    .expect("encode the request");
    let mut cursor: &[u8] = &wire;
    let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("frame it");
    let request = decode_request(msg).expect("decode it");

    let mut out = Encoder::new(Endian::Big);
    servant.dispatch(&request, &mut out).expect("the Java servant answered");

    let mut body = Decoder::new(out.as_bytes(), Endian::Big);
    assert_eq!(
        body.get_i32().expect("a long"),
        A + B,
        "the reply carries the Java object's answer, so the child ran rather than a default \
         being marshalled in its place"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
