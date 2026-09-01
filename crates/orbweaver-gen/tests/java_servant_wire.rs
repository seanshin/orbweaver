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
        return msg;
    }

    @Override
    public int ping() {
        return 42;
    }

    @Override
    public double scale(double v, double by) {
        return v * by;
    }

    @Override
    public echo.spike.Ragged echo_ragged(echo.spike.Ragged v) {
        return v;
    }

    @Override
    public byte[] blob(long size) {
        // `% 251` because that is what `spikes/jacorb/Client.java` checks. It
        // is a fixture agreeing with a peer's fixture, and getting it wrong
        // looked exactly like a size limit: blob(100) passed and blob(40000)
        // and blob(250000) failed, because `(byte) i` and `(byte)(i % 251)`
        // agree until i reaches 251.
        byte[] out = new byte[(int) size];
        for (int i = 0; i < out.length; i++) {
            out[i] = (byte) (i % 251);
        }
        return out;
    }

    @Override
    public int blob_sum(byte[] b) {
        int total = 0;
        for (byte x : b) {
            total += x & 0xff;
        }
        return total;
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
print("ping -> %d" % (echo.ping(),))
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

/// Starts the recording tap in front of `ior` and returns (process, tapped-ior).
///
/// The tap is peer-agnostic — its own header says *"the version and the codeset
/// choice come from the ORBs, and the log is what they did"* — so the same one
/// sits in front of omniORB and JacORB. `minor` republishes the profile at IIOP
/// 1.`minor`, which is how a peer whose outbound version follows the profile is
/// made to speak an older one.
///
/// **Waits for the published file to be non-empty, not merely to exist.** The
/// tap writes it after it binds; an empty file is a path that exists and a
/// listener that does not, which is the shape `spikes/lib/accepting.sh` exists
/// to refuse one layer down.
fn start_tap(
    ior: &orbweaver_giop::Ior,
    dir: &Path,
    log: &Path,
    minor: Option<u8>,
) -> (std::process::Child, PathBuf) {
    let ior_path = dir.join(format!("real-{}.ior", minor.unwrap_or(2)));
    std::fs::write(&ior_path, ior.to_stringified().expect("stringify")).expect("write");
    let tapped = dir.join(format!("tapped-{}.ior", minor.unwrap_or(2)));
    let mut cmd = Command::new("python3");
    cmd.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spikes/jacorb_giop11_tap.py"))
        .arg("--ior")
        .arg(&ior_path)
        .arg("--out")
        .arg(&tapped)
        .arg("--log")
        .arg(log)
        .arg("--op")
        .arg("echo_string");
    if let Some(m) = minor {
        cmd.arg("--minor").arg(m.to_string());
    }
    let child = cmd.stdout(std::process::Stdio::piped()).spawn().expect("the tap starts");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if std::fs::metadata(&tapped).map(|m| m.len() > 0).unwrap_or(false) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        std::fs::metadata(&tapped).map(|m| m.len() > 0).unwrap_or(false),
        "the recording tap never published a tapped IOR"
    );
    (child, tapped)
}

/// Every (version, order) a peer's REQUESTS carry, read off §15.4.1's flag byte.
///
/// The requests and not the replies: in the servant direction the peer is the
/// caller, so its writing is what it sent. Reading the replies here would report
/// our own order as a foreign peer's, which is the one claim `claimed` exists to
/// keep separate — the same split `spikes/lib/tap_orders.sh` keeps in two
/// functions rather than in a flag.
fn peer_request_orders(log: &Path) -> Vec<(String, &'static str)> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let mut out: Vec<(String, &'static str)> = text
        .lines()
        .filter(|l| l.contains("C->S GIOP") && l.contains(" Request "))
        .filter_map(|l| {
            let v = l.split("GIOP ").nth(1)?.get(..3)?.to_owned();
            let order = if l.contains(" BE ") || l.ends_with(" BE") {
                "big"
            } else if l.contains(" LE ") || l.ends_with(" LE") {
                "little"
            } else {
                return None;
            };
            Some((v, order))
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Binds a server holding the Java servant and hands its address to `f`.
///
/// Split out when the JacORB test needed the same three lines the omniORB one
/// had inline: bind, mount, serve on a thread. Two copies of a server setup is
/// two places to get the shutdown wrong, and the shutdown here is the part with
/// a trap in it — the accept loop only sees the stop flag after one more
/// connection, which is why the dial below is not decoration.
fn with_java_servant<F: FnOnce(&orbweaver_giop::Ior, std::net::SocketAddr)>(
    java: &Path,
    classes: &Path,
    f: F,
) {
    let server = Orb::new().server("127.0.0.1:0", ROOT.to_vec()).expect("bind");
    let addr = server.local_addr().expect("addr");
    let ior = server.ior(TYPE_ID, "127.0.0.1").expect("ior");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let classes = classes.to_path_buf();
    let java = java.to_path_buf();
    let serving = std::thread::spawn(move || {
        let child = SeamChild::java(&java, &classes, "Node")
            .expect("the JDK was found and then would not start");
        let mut servant =
            ForeignServant::new(&registry(), TYPE_ID, child).expect("the contract names Echo");
        let _ = server.serve(&mut servant, || flag.load(Ordering::SeqCst));
    });

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&ior, addr)));

    stop.store(true, Ordering::SeqCst);
    // Unblock the accept loop so the thread can see the flag.
    let _ = orbweaver_giop::Connection::connect(&ior, std::time::Duration::from_millis(500));
    let _ = serving.join();
    if let Err(e) = outcome {
        std::panic::resume_unwind(e);
    }
}

/// JacORB drives the Java servant, and the tap reads what JacORB wrote.
///
/// **This is the cell that closes clause 6 for the servant direction.** The
/// `servant × omniorb` cell reports `claimed`: no tap sits between the peers and
/// the little-endian order is inferred from the host. Here a recording tap sits
/// in front of our server, so the order is read off §15.4.1's flag byte of the
/// peer's own REQUESTS — and in the servant direction the requests are the
/// peer's writing, which is the inversion `spikes/lib/tap_orders.sh` keeps in
/// two functions rather than in a flag.
///
/// JacORB is the only peer in this grid that writes big-endian, which is why it
/// is this cell and not another that the servant direction was waiting on.
#[test]
fn jacorb_calls_a_java_servant() {
    let Some((javac, java)) = jdk() else {
        println!(
            "UNMEASURED: no JDK — set ORBWEAVER_JAVA_HOME. JacORB driving a Java servant is              unmeasured, not passing."
        );
        return;
    };
    let jdir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spikes/jacorb");
    if !jdir.join("classes/Client.class").is_file() {
        println!(
            "UNMEASURED: the JacORB fixture is not compiled (spikes/jacorb/classes) — run              spikes/jacorb/setup.sh. Foreign peer x big-endian is unmeasured, not passing."
        );
        return;
    }

    let dir = std::env::temp_dir().join(format!("orbweaver-java-jwire-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a work directory");
    let classes = build(&javac, &dir);

    let jcp = {
        let j = jdir.display().to_string();
        [
            format!("{j}/lib/jacorb.jar"),
            format!("{j}/lib/jacorb-omgapi.jar"),
            format!("{j}/lib/jboss-rmi-api.jar"),
            format!("{j}/lib/slf4j-api-1.7.36.jar"),
            format!("{j}/classes"),
        ]
        .join(":")
    };

    // **1.2 and 1.1.** 1.2 is JacORB's default; 1.1 is reached the way
    // `spikes/jacorb_giop11.sh` reaches it — not by a property but by
    // republishing the profile, because a peer's outbound version follows the
    // profile it dialled. Without the second pass the suite's version line
    // reads `servant: read[1.2] … neither[1.0 1.1]`, and a version nobody read
    // is the same kind of not-a-measurement as an order nobody read.
    let mut readings: Vec<(String, &'static str)> = Vec::new();
    for minor in [None, Some(1u8)] {
        let log = dir.join(format!("tap-{}.log", minor.unwrap_or(2)));
        let mut client_out = String::new();
        with_java_servant(&java, &classes, |ior, _addr| {
            let (mut tap, tapped) = start_tap(ior, &dir, &log, minor);
            let out = Command::new(&java)
                .arg("-cp")
                .arg(&jcp)
                .arg("Client")
                .arg(&tapped)
                .current_dir(&jdir)
                .output()
                .expect("run JacORB's client");
            client_out = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            let _ = tap.kill();
            let _ = tap.wait();
        });
        assert!(
            client_out.contains("ping()") && !client_out.contains("FAIL"),
            "JacORB's client did not complete its calls against the Java servant at IIOP \
             1.{}:\n{client_out}",
            minor.unwrap_or(2)
        );
        let seen = peer_request_orders(&log);
        assert!(
            !seen.is_empty(),
            "the calls completed at IIOP 1.{} and the tap recorded no request, so the byte \
             order was NOT read off the wire. An absent reading cannot count as covered.",
            minor.unwrap_or(2)
        );
        readings.extend(seen);
    }
    readings.sort();
    readings.dedup();

    assert!(
        readings.iter().any(|(_, o)| *o == "big"),
        "JacORB is the only peer in this grid that writes big-endian and the tap read none: \
         {readings:?}"
    );
    assert!(
        readings.iter().any(|(v, _)| v == "1.1") && readings.iter().any(|(v, _)| v == "1.2"),
        "both versions must be read off the wire, and were not: {readings:?}"
    );

    // The cell parses these. Printed by the test rather than recomputed by the
    // shell, because the tap logs live in a directory this test owns.
    for (v, order) in &readings {
        println!("read off the wire at {v} order={order}");
    }
    println!(
        "note {} (version, order) reading(s) from JacORB, off §15.4.1's flag byte of what the \
         PEER wrote — in the servant direction the requests are the peer's writing",
        readings.len()
    );

    let _ = std::fs::remove_dir_all(&dir);
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

    // **The tap, so this cell READS rather than claims.** It reported
    // `claimed giop=1.2 order=little` until 2026-09-01 — a sound inference from
    // omniORB writing its host's native order, and still not a reading. The tap
    // is peer-agnostic and was already in front of JacORB one test over; there
    // was no reason left for the two cells to be different kinds of evidence.
    let log = dir.join("tap-omni.log");
    let (mut tap, tapped) = start_tap(&ior, &dir, &log, None);

    let out = Command::new("python3")
        .arg(&script)
        .arg(&tapped)
        .arg(&idl)
        .current_dir(&dir)
        .output()
        .expect("run omniORB's client");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let _ = tap.kill();
    let _ = tap.wait();

    stop.store(true, Ordering::SeqCst);
    let _ = orbweaver_giop::Connection::connect(&ior, std::time::Duration::from_millis(500));
    let _ = serving.join();

    for wanted in ["add -> 42", "echo_string -> hello", "ping -> 42", "is_a Echo -> True"] {
        assert!(
            stdout.contains(wanted),
            "omniORB's client did not see {wanted:?} from the Java servant.\n\
             stdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }

    let readings = peer_request_orders(&log);
    assert!(
        !readings.is_empty(),
        "omniORB's client completed its calls and the tap recorded no request, so the byte \
         order was NOT read off the wire. An absent reading cannot count as covered."
    );
    assert!(
        readings.iter().any(|(_, o)| *o == "little"),
        "omniORB writes its host's native order and this host is little-endian; the tap read \
         no little-endian request: {readings:?}"
    );
    for (v, order) in &readings {
        println!("read off the wire at {v} order={order}");
    }
    println!(
        "note {} reading(s) from omniORB, off §15.4.1's flag byte of the peer's own requests",
        readings.len()
    );

    let _ = std::fs::remove_dir_all(&dir);
}
