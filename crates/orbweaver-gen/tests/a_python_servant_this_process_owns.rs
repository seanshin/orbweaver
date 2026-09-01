//! A Python servant mounted as a `Dispatch` in a server **this process owns**.
//!
//! # The sentence this file exists to make false
//!
//! `spikes/leak_tests.sh`'s language leg has been a counted `SKIPPED` since it
//! was written, and it named its own blocker rather than leaving it to be
//! guessed at:
//!
//! > What it waits on is a real Python process reachable from one: the only
//! > route today is `orbweaver-py-bridge --serve`, **which binds its own
//! > listener**, so the Python servant arrives as an endpoint rather than as a
//! > servant and a swap becomes a move.
//!
//! A caller made to dial a different address has been **moved**, and *location*
//! and *language* are different rows of D029 §6.1 — so a leak test that swapped
//! endpoints would be measuring the wrong row and looking green while it did.
//!
//! [`orbweaver_gen::pychild::SeamChild`] is the route that was missing:
//! `python3` as a child of **this** process, answering the seam's documents on
//! its own pipes through `python_rt.serve_on_pipes`, wrapped by
//! `seam::ForeignServant` into a plain `Dispatch`. No listener, no address, no
//! second implementation of the protocol.
//!
//! # What this file measures, and what it does not
//!
//! It measures that the route works: a Python servant answers a call arriving
//! through a `Dispatch` this process holds, and answers it with the Python
//! object's value rather than a default. **It is not the language leak test.**
//! That test — a caller holding one reference across a swap from Rust to
//! Python — is what this unblocks and is not what this is; claiming otherwise
//! would be the *green because nothing happened* shape this project keeps
//! finding.
//!
//! *언어 누출 다리가 막혀 있던 이유는 유일한 경로가 자기 리스너를 바인딩해서
//! 파이썬 서번트가 **엔드포인트로** 도착했기 때문이다 — 주소를 바꿔 다이얼하면
//! 그것은 이동이지 재구현이 아니고, 그 둘은 D029 §6.1의 다른 행이다. 이 파일은
//! **경로가 동작한다**만 잰다. 누출 테스트 자체가 아니다.*

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_gen::pychild::SeamChild;
use orbweaver_gen::seam::ForeignServant;
use orbweaver_giop::server::{Dispatch, decode_request};
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, Version, encode_request, read_message};
use orbweaver_registry::{Contract, Registry, Strictness};

const PACKAGE: &str = "gc26_owned";
const TYPE_ID: &str = "IDL:gc26/Directory:1.0";
/// What the Python servant answers `count()` with. Deliberately not 0: a
/// default-constructed anything would answer 0, and an assertion that cannot
/// tell the servant from a default is not evidence that the servant ran.
const COUNT: i32 = 37;

fn registry() -> Registry {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/golden/26-object-identity.idl");
    let contract = Contract::load(&path, &Default::default(), Strictness::Checked)
        .expect("the corpus contract must load");
    let mut registry = Registry::new();
    registry.load(&contract.spec).expect("the contract must build a registry");
    registry
}

/// Writes the generated package and returns the directory to put on `sys.path`.
///
/// `PythonPackage::files` is keyed relative to the package root, so the
/// directory holding them is named after the package and its **parent** is what
/// goes on the path — the mistake `python_servant_wire.rs` records making once.
fn site(dir: &std::path::Path) -> std::path::PathBuf {
    let package = orbweaver_gen::python::emit_python(&registry(), PACKAGE);
    let root = dir.join("site");
    let package_dir = root.join(PACKAGE);
    for (relative, body) in &package.files {
        let path = package_dir.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
        std::fs::write(&path, body).expect("write");
    }
    root
}

fn program() -> String {
    format!(
        r#"
from {PACKAGE} import _rt
from {PACKAGE} import gc26


class Node(gc26.DirectoryServant):
    def _get_label(self):
        return "python"

    def count(self):
        return {COUNT}


_rt.serve_on_pipes(Node())
"#
    )
}

/// A first-party C peer calls the Python servant.
///
/// The Java sibling of this landed first (`java_servant_wire.rs`), and its
/// bound is this one's: `spikes/bindings/AXES` decided, when the peer landed,
/// that **`independent` refutes coding errors and does NOT satisfy clause 6** —
/// the peer shares no code with `crates/`, which is real evidence, and shares
/// the same reading of the same specification by the same process, which *a
/// convention both ends apply cannot be refuted by a round trip* disposes of.
///
/// Worth having for the reason the Java one is: every other peer that has driven
/// a Python servant here is an ORB, and this is a program that speaks GIOP and
/// nothing else, so the servant's answer crosses a stack with no ORB assumptions.
#[test]
fn a_c_peer_calls_a_python_servant() {
    let peer = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/c_peer");
    if !peer.is_file() {
        println!(
            "UNMEASURED: the C peer is not built (target/c_peer) — run \
             spikes/build_c_peer.sh. Unmeasured, not passing."
        );
        return;
    }
    let dir = std::env::temp_dir().join(format!("orbweaver-pyc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a work directory");
    let root = site(&dir);

    let Ok(child) = SeamChild::python(&program(), &[&root]) else {
        println!("UNMEASURED: python3 did not start; the Python servant was not mounted");
        return;
    };
    let mut servant =
        ForeignServant::new(&registry(), TYPE_ID, child).expect("the contract names Directory");

    // The server this test owns; the Python servant is its `Dispatch`. No
    // second listener, so the servant arrives as a servant and not as an
    // endpoint — the distinction this file exists to keep.
    let server =
        orbweaver_giop::orb::Orb::new().server("127.0.0.1:0", b"dirs".to_vec()).expect("bind");
    let ior = server.ior(TYPE_ID, "127.0.0.1").expect("ior");
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stop.clone();
    let serving = std::thread::spawn(move || {
        let _ = server.serve(&mut servant, || flag.load(std::sync::atomic::Ordering::SeqCst));
    });

    let ior_path = dir.join("dirs.ior");
    std::fs::write(&ior_path, ior.to_stringified().expect("stringify")).expect("write");
    let run = std::process::Command::new(&peer)
        .arg("--role")
        .arg("client")
        .arg("--ior-file")
        .arg(&ior_path)
        .arg("--op")
        .arg("count")
        .arg("--expect")
        .arg("long")
        .output()
        .expect("run the C peer");
    let out =
        format!("{}{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));

    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = orbweaver_giop::Connection::connect(&ior, std::time::Duration::from_millis(500));
    let _ = serving.join();

    assert!(
        out.contains(&COUNT.to_string()),
        "the C peer did not read {COUNT} back from the Python servant's `count()`:\n{out}"
    );
    println!(
        "note a C peer that links no ORB called a Python servant and decoded its answer; \
         `independent` refutes coding errors and closes no clause (spikes/bindings/AXES)"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The route the language row was waiting on, exercised end to end in one
/// process: `python3` as this process's child, wrapped as a `Dispatch`.
#[test]
fn a_python_servant_answers_through_a_dispatch_this_process_holds() {
    let dir = std::env::temp_dir().join(format!("orbweaver-pychild-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a work directory");
    let root = site(&dir);

    let child = match SeamChild::python(&program(), &[&root]) {
        Ok(c) => c,
        Err(why) => panic!("python3 did not start: {why}"),
    };
    let mut servant =
        ForeignServant::new(&registry(), TYPE_ID, child).expect("the contract names Directory");

    // Built the way a peer builds one — encoded and decoded — rather than
    // constructed field by field, so the servant is handed the same shape a
    // socket would hand it.
    let wire = encode_request(Version::V1_2, Endian::Big, 1, b"dirs", "count", true, |_| {})
        .expect("encode the request");
    let mut cursor: &[u8] = &wire;
    let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("frame it");
    let request = decode_request(msg).expect("decode it");

    let mut out = Encoder::new(Endian::Big);
    servant.dispatch(&request, &mut out).expect("the Python servant answered");

    let mut body = Decoder::new(out.as_bytes(), Endian::Big);
    assert_eq!(
        body.get_i32().expect("a long"),
        COUNT,
        "the reply carries the Python object's value, so the child ran rather than a default \
         being marshalled in its place"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
