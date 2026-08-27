//! A Python servant, over real GIOP, from a caller that is not it.
//!
//! `python_servant.rs` compares a Python servant against a Rust one with no
//! process in sight, which is what makes every branch measurable on a busy
//! machine. This file is the other half of D030 §3's rule:
//!
//! > *A language is a target when its generated code is measured against a peer
//! > that is not us, in both byte orders, and its refusals say the same
//! > sentences ours do. Anything short of that is an emitter, and is called
//! > one.*
//!
//! So there are two peers here, in increasing order of independence:
//!
//! * [`our_generated_rust_client_calls_a_python_servant`] — the generated Rust
//!   **client** for `corpus/golden/24-skeleton-surface.idl` calling a generated
//!   Python **servant** for the same contract, over three GIOP versions and both
//!   byte orders. Both halves are ours, so this measures the seam and not the
//!   agreement.
//! * [`omniorb_calls_a_python_servant`] — omniORB's Python client, which knows
//!   nothing about this project, calling the same servant. This is the mirror of
//!   `skeleton_wire.rs`'s `omniorb_python_drives_the_generated_skeleton`: same
//!   peer, same contract, same driver shape, opposite language behind the
//!   reference. It is the measurement that decides whether "Python servant" is a
//!   target or an emitter.
//!
//! # The topology, and why it is this way round
//!
//! Python is the **parent**. It starts `orbweaver-py-bridge --serve`, reads the
//! IOR out of the bridge's ready banner, and answers calls the bridge writes to
//! it. That is the mirror of the client direction, where Python starts the same
//! program with `--ior` and writes requests to it — one process, D007's shape
//! unchanged, the initiative reversed. It also matches how a servant author
//! actually works: they run their own program, and the ORB is something it
//! starts, not something that starts it.
//!
//! # When omniORB is absent
//!
//! The peer test prints `UNMEASURED:` and passes, exactly as `skeleton_wire.rs`
//! does — `spikes/run_checks.sh` is where an absent fixture becomes a counted
//! `SKIPPED`, and a cargo test that failed for a missing optional fixture would
//! make the whole workspace unbuildable without omniORB installed. An
//! **unmeasured check is a failure and never a pass**, which is why the word is
//! printed rather than swallowed.

mod emitted;

use std::io::{Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use orbweaver_cdr::Endian;
use orbweaver_gen::rt::ObjectHome;
use orbweaver_giop::orb::Orb;
use orbweaver_giop::{Connection, Ior, Version};

use emitted::f_24_skeleton_surface::gc24::{
    Busy, GaugeClient, GaugeFault, GaugeRefs, GaugeServant, GaugeSkeleton, GaugeTarget, Reading,
    Rejected,
};

const VERSIONS: [Version; 3] = [Version::V1_0, Version::V1_1, Version::V1_2];

/// The generated package's name, and therefore the directory that holds it and
/// the name the servant script imports. One constant because those three have
/// to agree and nothing would compile if they did not — they are strings.
const PACKAGE: &str = "g24_surface";

/// The IDL this whole file is about, as an absolute path.
fn contract() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/golden/24-skeleton-surface.idl")
}

// ── The servant an application author writes ─────────────────────────────────

/// Fixed text, never generated.
///
/// A driver the generator wrote for itself would prove nothing — the same rule
/// `python_target.rs` states about its own. This is what a Python developer
/// writes: subclass the generated servant base, fill in the bodies, hand it to
/// `_rt.serve(...).run(...)`. Nothing here mentions GIOP, CDR, an IOR or a byte
/// order, which is the whole claim the seam makes.
const SERVANT: &str = r#"
import sys

sys.path.insert(0, sys.argv[1])

from g24_surface import _rt
from g24_surface import gc24


class Bench(gc24.GaugeServant):
    """A gauge. The same one `python_servant.rs` implements in Rust."""

    def __init__(self):
        self.samples = []
        self.label = "unset"
        self.latest = gc24.Reading(0.0, 0, "")

    def _get_latest(self):
        return self.latest

    def _get_label(self):
        return self.label

    def _set_label(self, value):
        self.label = value

    def record(self, sample, unit):
        if sample < 0.0:
            raise gc24.Rejected("a sample below zero is not a reading", 7)
        if unit == "":
            raise gc24.Busy()
        self.samples.append(sample)
        self.latest = gc24.Reading(sample, len(self.samples), unit)
        return self.latest

    def scale_all(self, e):
        self.samples = [s * e for s in self.samples]
        self.latest = gc24.Reading(self.latest.at * e, self.latest.sequence_no,
                                   self.latest.unit)
        return len(self.samples)

    def reset(self):
        self.samples = []
        self.latest = gc24.Reading(0.0, 0, "")

    def split(self):
        return (self.latest.at, self.latest.unit)


host = _rt.serve(sys.argv[2], "IDL:gc24/Gauge:1.0")
with open(sys.argv[3], "w") as f:
    f.write(host.ior)
host.run(Bench())
"#;

// ── Harness ──────────────────────────────────────────────────────────────────

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Option<Self> {
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.subsec_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path).ok()?;
        Some(Self(path))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A Python servant process, and the reference it published.
///
/// Killed on drop, by handle rather than by name: `fkill`-style cleanup would
/// reach another batch's fixtures on a machine running more than one thing.
struct Servant {
    child: std::process::Child,
    /// `None` until the servant publishes one. The child is owned from the
    /// moment it is spawned so that *every* exit from the wait below — the
    /// deadline, a panic, the servant dying — still kills it; a version that
    /// waited first and took ownership after would leak a process on exactly
    /// the paths where a leak matters.
    ior: Option<Ior>,
    _dir: TempDir,
}

impl Servant {
    fn ior(&self) -> &Ior {
        self.ior.as_ref().expect("the servant published an IOR before this was called")
    }
}

impl Drop for Servant {
    fn drop(&mut self) {
        // **The servant is not the leaf.** `python_rt.py` starts
        // `orbweaver-py-bridge` as its own child, so killing this handle alone
        // orphans the bridge, which then holds a loopback port until somebody
        // notices. Measured 2026-08-27: **twelve orphans from a single harness
        // run**, every one `ppid=1`, and fifty more from the days before.
        //
        // The comment above this struct reasoned carefully about never leaking
        // *the child* — "every exit from the wait below … still kills it" —
        // and that reasoning is correct one level up from where it needed to
        // be. `child.kill()` is SIGKILL besides, which no handler can catch,
        // so Python never got to run the `close()` it had written for exactly
        // this.
        //
        // So: SIGTERM the whole group first. It reaches the bridge directly,
        // and it gives Python's `atexit` the chance to close its own child
        // politely. `std` cannot signal a group and `unsafe` is forbidden
        // here, so `kill(1)` does it — `--` because the pid is negative.
        let pid = self.child.id();
        let _ =
            std::process::Command::new("kill").args(["-TERM", "--", &format!("-{pid}")]).status();
        // The old behaviour stays as the floor: whatever ignored SIGTERM dies.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Generates the package, writes the servant script, starts it, and waits for
/// the IOR it publishes.
///
/// `None` when `python3` is not on the path — the one absence this returns
/// rather than panics on, because it is the same absence `omniidl_available`
/// reports and both are reported as `UNMEASURED` by the caller.
fn start_servant() -> Option<Servant> {
    let dir = TempDir::new("orbweaver-python-servant")?;

    let mut registry = orbweaver_registry::Registry::new();
    let spec = orbweaver_registry::Contract::load(
        &contract(),
        &Default::default(),
        orbweaver_registry::Strictness::Checked,
    )
    .expect("the corpus contract must load");
    registry.load(&spec.spec).expect("the contract must build a registry");

    let package = orbweaver_gen::python::emit_python(&registry, PACKAGE);
    // `PythonPackage::files` is keyed **relative to the package root**, so the
    // directory holding them has to be named after the package and its
    // *parent* is what goes on `sys.path`. Writing them into a directory
    // called anything else produces a tree Python cannot import, which is what
    // the first run of this file did: `ModuleNotFoundError: No module named
    // 'g24_surface'`, from both tests, with one cause.
    let root = dir.path().join("site");
    let package_dir = root.join(PACKAGE);
    for (relative, body) in &package.files {
        let path = package_dir.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
        std::fs::write(&path, body).expect("write");
    }

    let script = dir.path().join("servant.py");
    std::fs::write(&script, SERVANT).expect("write the servant");
    let ior_path = dir.path().join("gauge.ior");

    // Its own process group, so `Drop` can signal the TREE and not just this
    // handle. `python_rt.py` starts `orbweaver-py-bridge` as a child of this
    // process, so the servant is not the leaf — see the comment on `Drop`.
    // `process_group` is safe std; this workspace forbids `unsafe`.
    use std::os::unix::process::CommandExt as _;
    let child = std::process::Command::new("python3")
        .process_group(0)
        .arg(&script)
        .arg(&root)
        .arg(contract())
        .arg(&ior_path)
        // The bridge the generated runtime starts. Passed explicitly rather
        // than relied on from `PATH`: a test that silently used an installed
        // build would measure something other than this tree.
        .env("ORBWEAVER_PY_BRIDGE", env!("CARGO_BIN_EXE_orbweaver-py-bridge"))
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;

    // A sleeping, deadline-bounded wait. `for i in $(seq 1 500); do [ -f f ] &&
    // break; done` finishes in microseconds and does not wait at all — the
    // phantom failure `CLAUDE.md`'s harness rules open with.
    let mut servant = Servant { child, ior: None, _dir: dir };
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(&ior_path) {
            let text = text.trim();
            if text.starts_with("IOR:") {
                servant.ior = Some(Ior::parse(text).expect("the servant published a parsable IOR"));
                return Some(servant);
            }
        }
        if let Ok(Some(status)) = servant.child.try_wait() {
            let mut why = String::new();
            if let Some(mut e) = servant.child.stderr.take() {
                use std::io::Read as _;
                let _ = e.read_to_string(&mut why);
            }
            panic!("the Python servant exited before publishing an IOR ({status}):\n{why}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the Python servant published no IOR within 30s");
}

fn connect(ior: &Ior, version: Version, endian: Endian) -> Connection {
    let mut conn = Connection::connect(ior, Duration::from_secs(10)).expect("connect");
    conn.cap_version(version);
    conn.set_endian(endian);
    conn
}

/// Whether omniORB's Python bindings are importable.
fn omniorb_available() -> bool {
    std::process::Command::new("python3")
        .args(["-c", "import omniORB"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── Our own client ───────────────────────────────────────────────────────────

/// The generated Rust client for this contract, calling a generated Python
/// servant for the same contract, over every GIOP version and both byte orders.
///
/// Both halves are ours, so what this measures is the **seam** — that a call
/// survives being decoded from CDR, turned into JSON, answered by Python,
/// turned back and re-encoded, with the same values arriving. It is not the
/// independence D030 §3 asks for; [`omniorb_calls_a_python_servant`] is.
#[test]
fn our_generated_rust_client_calls_a_python_servant() {
    let Some(servant) = start_servant() else {
        println!("UNMEASURED: python3 is not available; the Python servant was not started");
        return;
    };

    for version in VERSIONS {
        for endian in [Endian::Big, Endian::Little] {
            let mut client = GaugeClient::new(connect(servant.ior(), version, endian));
            let what = format!("{version} {endian:?}");

            client.set_label(format!("driven at {what}")).expect("set_label");
            assert_eq!(client.label().expect("label"), format!("driven at {what}"), "{what}");

            let reading = client.record(21.5, "C".into()).expect("record");
            assert_eq!(reading.at, 21.5, "{what}");
            assert_eq!(reading.unit, "C", "{what}");
            assert_eq!(reading.sequence_no, 1, "{what}");

            let scaled = client.scale_all(2.0).expect("scale_all");
            assert_eq!(scaled, 1, "{what}");
            assert_eq!(client.latest().expect("latest").at, 43.0, "{what}");

            let (at, unit) = client.split().expect("split");
            assert_eq!((at, unit.as_str()), (43.0, "C"), "{what}");

            // A user exception, raised in Python and travelling as a
            // USER_EXCEPTION reply with its repository id in front. Decoded
            // member by member rather than matched as text: the first version
            // of this assertion searched the error's `Debug` for the message
            // and failed while the servant was answering perfectly, because
            // what `Debug` prints is the undecoded body as a list of numbers.
            match client.record(-1.0, "C".into()).expect_err("a negative sample is rejected") {
                orbweaver_giop::Error::UserException { id, reply } => {
                    assert_eq!(id, "IDL:gc24/Rejected:1.0", "{what}");
                    let mut body = reply.body().expect("body");
                    assert_eq!(body.get_string().expect("the repository id"), id, "{what}");
                    assert_eq!(
                        body.get_string().expect("why"),
                        "a sample below zero is not a reading",
                        "{what}"
                    );
                    assert_eq!(body.get_i32().expect("code"), 7, "{what}");
                }
                other => panic!("{what}: expected a user exception, got {other}"),
            }
            // And the memberless one: the repository id and nothing else, which
            // is the empty-body edge case a servant can get wrong by writing a
            // length or a pad after it.
            match client.record(1.0, String::new()).expect_err("an empty unit is refused") {
                orbweaver_giop::Error::UserException { id, reply } => {
                    assert_eq!(id, "IDL:gc24/Busy:1.0", "{what}");
                    let mut body = reply.body().expect("body");
                    assert_eq!(body.get_string().expect("the repository id"), id, "{what}");
                    assert!(body.is_empty(), "{what}: Busy has no members");
                }
                other => panic!("{what}: expected a user exception, got {other}"),
            }
            // A raise is an answer, not a fault: the connection carries on.
            assert_eq!(client.record(3.0, "C".into()).expect("after the raise").at, 3.0, "{what}");

            // A oneway, and then a call that proves the connection's framing
            // survived it: §9.4.1 gives a oneway no reply, and a servant that
            // wrote an empty one would have the peer read it as the header of
            // this next reply.
            client.reset().expect("reset");
            assert_eq!(client.latest().expect("latest after reset").sequence_no, 0, "{what}");
        }
    }
}

// ── The peer that is not us ──────────────────────────────────────────────────

/// Fixed text, never generated. Written the way an omniORB user writes one:
/// `importIDL`, `string_to_object`, `_narrow`, and ordinary attribute and
/// method syntax. Nothing in it knows the servant is Python.
const OMNIORB_DRIVER: &str = r#"
import sys

import CORBA
import omniORB

omniORB.importIDL(sys.argv[2])
import gc24

orb = CORBA.ORB_init(sys.argv[3:], CORBA.ORB_ID)
gauge = orb.string_to_object(open(sys.argv[1]).read().strip())._narrow(gc24.Gauge)
if gauge is None:
    print("narrow failed")
    raise SystemExit(1)

gauge.label = "driven by omniORB"
print("label = %s" % (gauge.label,))

r = gauge.record(21.5, "C")
print("record -> %r %d %s" % (r.at, r.sequence_no, r.unit))
print("scale_all -> %d" % (gauge.scale_all(2.0),))
print("latest.at -> %r" % (gauge.latest.at,))
at, unit = gauge.split()
print("split -> %r %s" % (at, unit))

try:
    gauge.record(-1.0, "C")
    print("a negative sample was not refused")
except gc24.Rejected as e:
    print("Rejected %s %d" % (e.why, e.code))
try:
    gauge.record(1.0, "")
    print("an empty unit was not refused")
except gc24.Busy:
    print("Busy")

# A readonly attribute. omniORB's generated stub makes this a Python attribute
# error *before* anything reaches the wire, so this line says nothing about the
# servant and is deliberately not asserted on: the wire-level refusal of
# `_set_latest` with BAD_OPERATION is measured in `python_servant.rs`, where
# the request can be built by hand.
try:
    gauge.latest = r
    print("readonly: the client stub allowed the assignment")
except Exception as e:
    print("readonly refused client-side with %s" % (type(e).__name__,))

gauge.reset()
print("after the oneway, sequence_no = %d" % (gauge.latest.sequence_no,))

print("is_a NamingContext -> %s" % (gauge._is_a("IDL:omg.org/CosNaming/NamingContext:1.0"),))
print("is_a Gauge -> %s" % (gauge._is_a("IDL:gc24/Gauge:1.0"),))
print("non_existent -> %s" % (gauge._non_existent(),))
print("OK")
"#;

/// **The measurement D030 §3 asks for.** omniORB's client, which knows nothing
/// about this project, calling a Python servant behind our ORB.
///
/// The mirror of `skeleton_wire.rs`'s `omniorb_python_drives_the_generated_skeleton`
/// — same peer, same contract, same driver shape, and the only difference is the
/// language behind the reference. If this agrees with that one, a caller cannot
/// tell what language it is talking to, which is D029 §6.1's Language row.
///
/// # Byte order: what this measures, and what it does not
///
/// **This test measures one byte order, and that is a gap rather than a
/// choice.** omniORB emits its native order — little-endian on this machine —
/// and `orbweaver-giop`'s server replies in *the request's* order
/// (`server.rs`, `Encoder::continuing_at(req.endian, …)`), so both directions
/// of this exchange are little-endian and nothing here has ever seen a
/// big-endian foreign peer talk to a Python servant.
///
/// D030 §3 asks for both orders against a peer that is not us, so the rule is
/// **not fully met by this file** and saying otherwise would be claiming a
/// measurement nobody took. Both orders *are* measured against our own client
/// in [`our_generated_rust_client_calls_a_python_servant`], which sets the
/// encoder's order explicitly, and byte-for-byte against a Rust servant over
/// both orders in `python_servant.rs`. What is missing is specifically
/// *foreign peer × big-endian*.
///
/// What closes it: **JacORB**, and it is now written —
/// [`jacorb_calls_a_python_servant`], below, runs
/// `spikes/jacorb/GaugeDriver.java` against this same servant and reads the
/// byte order out of the request's flag byte rather than out of the language.
/// This test keeps its own limit stated because it is still true *of this
/// test*: the omniORB leg is little-endian in both directions and always will
/// be on a little-endian host.
#[test]
fn omniorb_calls_a_python_servant() {
    if !omniorb_available() {
        println!(
            "UNMEASURED: omniORB's Python bindings are not importable, so the peer half \
             of D030 §3 was not measured for the servant direction"
        );
        return;
    }
    let Some(servant) = start_servant() else {
        println!("UNMEASURED: python3 is not available; the Python servant was not started");
        return;
    };

    let dir = TempDir::new("orbweaver-omniorb-driver").expect("tmpdir");
    // omniidl needs a Python-legal basename, which `24-skeleton-surface.idl` is
    // not — the same copy `skeleton_wire.rs` makes, for the same reason.
    let idl = dir.path().join("gc24_surface.idl");
    std::fs::copy(contract(), &idl).expect("copy the contract");
    let script = dir.path().join("drive.py");
    std::fs::write(&script, OMNIORB_DRIVER).expect("write the driver");
    let ior_path = dir.path().join("gauge.ior");
    let mut f = std::fs::File::create(&ior_path).expect("create");
    write!(f, "{}", servant.ior().to_stringified().expect("stringify")).expect("write the ior");
    drop(f);

    let out = std::process::Command::new("python3")
        .arg(&script)
        .arg(&ior_path)
        .arg(&idl)
        .current_dir(dir.path())
        .output()
        .expect("run omniORB's client");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    for wanted in [
        "label = driven by omniORB",
        "record -> 21.5 1 C",
        "scale_all -> 1",
        "latest.at -> 43.0",
        "split -> 43.0 C",
        "Rejected a sample below zero is not a reading 7",
        "Busy",
        "after the oneway, sequence_no = 0",
        "is_a NamingContext -> False",
        "is_a Gauge -> True",
        "non_existent -> False",
        "OK",
    ] {
        assert!(
            stdout.contains(wanted),
            "omniORB's client did not report {wanted:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert!(
        !stdout.contains("the client stub allowed the assignment"),
        "a refusal did not happen:\n{stdout}"
    );
    // Nothing about the implementation reached the caller. This is the
    // transparency claim itself, made against the only thing a peer actually
    // has: what it printed. A traceback, a bridge diagnostic or the word
    // Python in an exception's text would all be the target's language
    // arriving at a client that asked about a gauge.
    for leak in ["Traceback", "python", "Python", "bridge", "orbweaver", "_rt"] {
        assert!(
            !stdout.contains(leak),
            "the peer's output mentions {leak:?}, so the servant's implementation \
             reached its caller:\n{stdout}"
        );
    }
}

// ── The other endianness ─────────────────────────────────────────────────────
//
// Everything below exists for one sentence in D030 §3 — *"in both byte
// orders"* — and for the half of it omniORB cannot reach on a little-endian
// host. The order is read out of the request's flag byte, never out of the
// peer's language.

/// The bare root key the Rust servant below is bound with, and the type it
/// serves. Both halves of this comparison serve the same contract; only the
/// key differs, and a key travels in the *request*, which is why the replies
/// can be compared byte for byte at all.
const RUST_KEY: &[u8] = b"gauge";
const TYPE_ID: &str = "IDL:gc24/Gauge:1.0";

/// The one edit the JacORB copy of the contract carries, and why it is safe.
///
/// `org.jacorb.idl.parser` 3.9 emits, for **every** operation, a stub method
/// whose body contains `catch (java.io.IOException e)` — an unprefixed local in
/// the same scope as the operation's own parameters, while every other local it
/// writes is `_`-prefixed. So an IDL parameter named `e` produces Java that does
/// not compile, and `corpus/golden/24-skeleton-surface.idl` has one on purpose:
/// *"`e` is what a hand-written encoder would have called its encoder. The rule
/// needs a case that would break without it."* It broke a third-party emitter
/// too, which is the finding; two errors in `_GaugeStub.java`, and nothing in
/// the package builds.
///
/// **A parameter name is not on the wire.** GIOP marshals an operation's
/// arguments positionally and carries only the operation's name (§15.4.2), so
/// renaming one changes no byte of any request, reply or exception this test
/// compares — which is what makes the workaround a workaround and not a
/// different measurement. The servants on the other side are still generated
/// from the corpus file, unedited.
const JACORB_CANNOT_COMPILE: &str = "long scale_all(in double e);";
const JACORB_CAN_COMPILE: &str = "long scale_all(in double factor);";

/// The jars `spikes/jacorb/setup.sh` fetches. Named here so an incomplete
/// fixture is *absent* rather than a confusing `javac` failure.
const JACORB_JARS: [&str; 5] = [
    "jacorb.jar",
    "jacorb-omgapi.jar",
    "jacorb-idl-compiler.jar",
    "jboss-rmi-api.jar",
    "slf4j-api-1.7.36.jar",
];

// ── A recording relay ────────────────────────────────────────────────────────

/// One GIOP message the tap saw, with the facts read out of its 12-byte header.
#[derive(Clone)]
struct Frame {
    /// True for a message travelling peer → us.
    from_client: bool,
    /// §15.4.1's flag bit 0, inverted: the bit is *set* for little-endian, so
    /// this is the byte order **the peer chose**, taken from the byte it wrote.
    big: bool,
    version: (u8, u8),
    /// §15.4.1 message type: 0 Request, 1 Reply, 3 LocateRequest, 4 LocateReply.
    mtype: u8,
    bytes: Vec<u8>,
}

/// A TCP relay that records every GIOP message crossing it.
///
/// The same instrument `spikes/jacorb_giop11_tap.py` is, in Rust and inside the
/// test that needs it: a peer's byte order and a servant's reply bytes are read
/// off the wire rather than inferred from what came back. It changes nothing in
/// flight.
///
/// **A message is recorded before it is forwarded**, so by the time either peer
/// can act on a message the log already holds it — otherwise the driver could
/// exit and be observed before the tap had filed its last reply, and the
/// comparison would race.
struct Tap {
    addr: SocketAddr,
    frames: Arc<Mutex<Vec<Frame>>>,
    stop: Arc<AtomicBool>,
    accept: Option<std::thread::JoinHandle<()>>,
}

impl Tap {
    fn start(target: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the tap");
        let addr = listener.local_addr().expect("the tap's address");
        let frames = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (f, s) = (frames.clone(), stop.clone());
        let accept = std::thread::spawn(move || {
            for client in listener.incoming() {
                if s.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(client) = client else { break };
                let Ok(server) = TcpStream::connect(target) else { continue };
                let (client_back, server_back) = match (client.try_clone(), server.try_clone()) {
                    (Ok(a), Ok(b)) => (a, b),
                    _ => continue,
                };
                let up = f.clone();
                let down = f.clone();
                std::thread::spawn(move || relay(client, server, true, up));
                std::thread::spawn(move || relay(server_back, client_back, false, down));
            }
        });
        Self { addr, frames, stop, accept: Some(accept) }
    }

    fn frames(&self) -> Vec<Frame> {
        self.frames.lock().expect("the tap's log").clone()
    }

    /// The peer's copy of an IOR: the same reference, dialled through the tap,
    /// and optionally republished at another IIOP version — which is what makes
    /// a peer whose outbound GIOP version follows the profile speak 1.1.
    ///
    /// Every component, `TAG_CODE_SETS` included, is carried over unchanged,
    /// so the peer still negotiates against what the real server advertised.
    fn through(&self, ior: &Ior, version: Option<Version>) -> Ior {
        let mut out = ior.clone();
        for p in &mut out.profiles {
            p.host = self.addr.ip().to_string();
            p.port = self.addr.port();
            if let Some(v) = version {
                p.version = v;
            }
        }
        out
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr); // wake the accept loop
        if let Some(t) = self.accept.take() {
            let _ = t.join();
        }
    }
}

fn relay(mut src: TcpStream, mut dst: TcpStream, from_client: bool, log: Arc<Mutex<Vec<Frame>>>) {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 65536];
    loop {
        let n = match src.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        while buf.len() >= 12 {
            if &buf[..4] != b"GIOP" {
                buf.clear();
                break;
            }
            let big = buf[6] & 1 == 0;
            let raw: [u8; 4] = buf[8..12].try_into().expect("four bytes");
            let size = if big { u32::from_be_bytes(raw) } else { u32::from_le_bytes(raw) } as usize;
            if buf.len() < 12 + size {
                break;
            }
            let bytes: Vec<u8> = buf.drain(..12 + size).collect();
            let frame =
                Frame { from_client, big, version: (bytes[4], bytes[5]), mtype: bytes[7], bytes };
            log.lock().expect("the tap's log").push(frame);
        }
        if dst.write_all(&chunk[..n]).is_err() {
            break;
        }
    }
    let _ = dst.shutdown(std::net::Shutdown::Write);
}

// ── The Rust servant, over the same wire ─────────────────────────────────────

/// A gauge, in Rust — the servant an application author writes today, and the
/// thing the Python one has to be byte-identical to.
///
/// Held to the same shape as `skeleton_wire.rs`'s and `python_servant.rs`'s
/// `Bench` deliberately: the claim is that the *other* servant answers the
/// same, so the Rust half must be the ordinary one and not a special case.
struct Bench {
    samples: Vec<f64>,
    label: String,
    latest: Reading,
    /// The negative control for the byte-identity comparison below, set from
    /// the environment so a harness can make this group red without editing a
    /// source file. A comparison with no way to fail is
    /// green-while-measuring-nothing.
    perturb: bool,
}

impl Default for Bench {
    fn default() -> Self {
        Self {
            samples: Vec::new(),
            label: "unset".into(),
            latest: Reading { at: 0.0, sequence_no: 0, unit: String::new() },
            perturb: std::env::var("ORBWEAVER_JACORB_PERTURB").is_ok(),
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
        let sequence_no = self.samples.len() as i32 + i32::from(self.perturb);
        self.latest = Reading { at: sample, sequence_no, unit: unit.clone() };
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

/// Runs `f` against a live server whose dispatcher is the generated Rust
/// skeleton, handing it the reference and the address to dial.
///
/// The stop flag is observed between accepts and the loop is woken with one
/// throwaway connection — a sleeping, deadline-free wake instead of a spin.
fn with_rust_servant<F: FnOnce(&Ior, SocketAddr)>(f: F) {
    let server = Orb::new().server("127.0.0.1:0", RUST_KEY.to_vec()).expect("bind");
    let addr = server.local_addr().expect("addr");
    let ior = server.ior(TYPE_ID, "127.0.0.1").expect("ior");
    let home = ObjectHome::of(&server, "127.0.0.1").expect("home");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let t = std::thread::spawn(move || {
        let mut skeleton = GaugeSkeleton::new(GaugeRefs::new(home), Bench::default());
        server.serve(&mut skeleton, || flag.load(Ordering::SeqCst)).expect("serve");
    });

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&ior, addr)));

    stop.store(true, Ordering::SeqCst);
    let _ = TcpStream::connect(addr);
    t.join().expect("the server thread must not panic");
    if let Err(p) = outcome {
        std::panic::resume_unwind(p);
    }
}

// ── The fixture ──────────────────────────────────────────────────────────────

/// JacORB's IDL compiler and Java compiler, run once, and the driver they
/// produce.
///
/// A fixture that is **absent** returns `None` and the caller says so; a
/// fixture that is present and will not *start* panics. That split is
/// `CLAUDE.md`'s: an unmeasured check is a failure, never a pass, and the only
/// thing allowed to be silent is a machine that never had the fixture.
struct JacorbFixture {
    java: std::path::PathBuf,
    classpath: String,
    jacorb: std::path::PathBuf,
    dir: TempDir,
}

impl JacorbFixture {
    fn prepare() -> Option<Self> {
        let jacorb = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../spikes/jacorb")
            .canonicalize()
            .ok()?;
        let home = std::env::var("JAVA_HOME_21").unwrap_or_else(|_| {
            "/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home".to_owned()
        });
        let java = std::path::Path::new(&home).join("bin/java");
        let javac = std::path::Path::new(&home).join("bin/javac");
        if !java.is_file() || !javac.is_file() {
            return None;
        }
        let jars: Vec<std::path::PathBuf> =
            JACORB_JARS.iter().map(|j| jacorb.join("lib").join(j)).collect();
        if jars.iter().any(|j| !j.is_file()) {
            return None;
        }
        let driver = jacorb.join("GaugeDriver.java");
        if !driver.is_file() {
            return None;
        }

        let dir = TempDir::new("orbweaver-jacorb-gauge")?;
        let jar_path = jars.iter().map(|j| j.display().to_string()).collect::<Vec<_>>().join(":");

        // The contract, with the one name JacORB's own emitter cannot compile.
        let source = std::fs::read_to_string(contract()).expect("the corpus contract");
        assert_eq!(
            source.matches(JACORB_CANNOT_COMPILE).count(),
            1,
            "the corpus contract no longer contains {JACORB_CANNOT_COMPILE:?}; this copy \
             exists only to rename that parameter and must not silently rename nothing"
        );
        let idl = dir.path().join("gauge24.idl");
        std::fs::write(&idl, source.replace(JACORB_CANNOT_COMPILE, JACORB_CAN_COMPILE))
            .expect("write the JacORB copy of the contract");

        let generated = dir.path().join("gen");
        let classes = dir.path().join("classes");
        std::fs::create_dir_all(&generated).expect("mkdir gen");
        std::fs::create_dir_all(&classes).expect("mkdir classes");

        let idl_out = std::process::Command::new(&java)
            .arg("-cp")
            .arg(&jar_path)
            .arg("org.jacorb.idl.parser")
            .arg("-d")
            .arg(&generated)
            .arg(&idl)
            .output()
            .expect("run JacORB's IDL compiler");
        assert!(
            idl_out.status.success(),
            "JacORB's IDL compiler would not run ({}):\n{}\n{}",
            idl_out.status,
            String::from_utf8_lossy(&idl_out.stdout),
            String::from_utf8_lossy(&idl_out.stderr)
        );

        let mut sources: Vec<std::path::PathBuf> = std::fs::read_dir(generated.join("gc24"))
            .expect("JacORB wrote no gc24 package")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "java"))
            .collect();
        sources.sort();
        sources.push(driver);
        let javac_out = std::process::Command::new(&javac)
            .arg("-nowarn")
            .arg("-cp")
            .arg(&jar_path)
            .arg("-d")
            .arg(&classes)
            .args(&sources)
            .output()
            .expect("run javac");
        assert!(
            javac_out.status.success(),
            "the JacORB driver would not compile ({}):\n{}\n{}",
            javac_out.status,
            String::from_utf8_lossy(&javac_out.stdout),
            String::from_utf8_lossy(&javac_out.stderr)
        );

        let classpath = format!("{jar_path}:{}", classes.display());
        Some(Self { java, classpath, jacorb, dir })
    }

    /// Runs the driver against one reference and returns what it printed.
    fn run(&self, ior: &Ior, tag: &str) -> String {
        let path = self.dir.path().join(format!("{tag}.ior"));
        std::fs::write(&path, ior.to_stringified().expect("stringify")).expect("write the ior");
        let out = std::process::Command::new(&self.java)
            .arg("-cp")
            .arg(&self.classpath)
            .arg("GaugeDriver")
            .arg(&path)
            .current_dir(&self.jacorb)
            .output()
            .expect("run JacORB's client");
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            out.status.success(),
            "JacORB's client exited {} against the {tag} servant\nstdout:\n{stdout}\nstderr:\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        stdout
    }
}

/// What the peer must have printed, whichever language answered it.
///
/// The same list `omniorb_calls_a_python_servant` asserts, in the same words,
/// because the two drivers were written to print the same sentences: a caller
/// that cannot tell the servants apart cannot tell the transcripts apart.
fn assert_transcript(stdout: &str, who: &str) {
    for wanted in [
        "label = driven by JacORB",
        "record -> 21.5 1 C",
        "scale_all -> 1",
        "latest.at -> 43.0",
        "split -> 43.0 C",
        "Rejected a sample below zero is not a reading 7",
        "Busy",
        "after the oneway, sequence_no = 0",
        "is_a NamingContext -> false",
        "is_a Gauge -> true",
        "non_existent -> false",
        "OK",
    ] {
        assert!(
            stdout.contains(wanted),
            "JacORB's client did not report {wanted:?} against the {who} servant\n{stdout}"
        );
    }
    for leak in ["Traceback", "python", "Python", "bridge", "orbweaver", "_rt"] {
        assert!(
            !stdout.contains(leak),
            "the peer's output mentions {leak:?} against the {who} servant, so the \
             implementation reached its caller:\n{stdout}"
        );
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The orders seen in a set of frames, as words, so a failure names what was
/// on the wire rather than a boolean.
fn orders(frames: &[&Frame]) -> Vec<&'static str> {
    let mut seen: Vec<&'static str> =
        frames.iter().map(|f| if f.big { "big" } else { "little" }).collect();
    seen.sort_unstable();
    seen.dedup();
    seen
}

fn versions(frames: &[&Frame]) -> Vec<String> {
    let mut seen: Vec<String> =
        frames.iter().map(|f| format!("{}.{}", f.version.0, f.version.1)).collect();
    seen.sort();
    seen.dedup();
    seen
}

// ── The measurement ──────────────────────────────────────────────────────────

/// **The half of D030 §3 omniORB cannot reach**: a foreign peer that writes
/// big-endian, calling a Python servant behind our ORB — and the same calls,
/// answered by a Rust servant, coming back as the same bytes.
///
/// Three things are asserted, and the first is the one that makes the other two
/// worth anything:
///
/// 1. **The byte order is read off the wire.** Every request JacORB writes has
///    its §15.4.1 flag bit inspected by the tap; "Java is big-endian" is a
///    belief until that byte says so, and the expectation is settable from the
///    environment (`ORBWEAVER_JACORB_EXPECT_ORDER=little`) precisely so the
///    assertion can be made to go red on a wire that has not changed.
/// 2. **The transcript.** JacORB's client gets the same answers omniORB's does,
///    and nothing about the servant's language reaches it.
/// 3. **Byte-identity between our two servants.** Every reply the Python
///    servant sent, against every reply the Rust servant sent for the same
///    driver run, compared byte for byte — which is `python_servant.rs`'s claim
///    made from the other endianness, over a socket, with a foreign peer
///    choosing the order.
///
/// The order is 1, 3, 2 rather than 1, 2, 3, and that is the negative control's
/// doing. `ORBWEAVER_JACORB_PERTURB=1` makes the Rust servant answer one
/// `sequence_no` the Python one would not; **both** the byte comparison and the
/// transcript see it, because this contract's driver prints every value it
/// receives, so a perturbation invisible to the transcript is not reachable
/// through the servant trait at all. Running the bytes first is what makes the
/// control a control *for the bytes* rather than for the printing.
///
/// Comparing raw bytes here is the exception `CLAUDE.md` names, not a breach of
/// it: both replies are written by *our* encoder, so a difference in the bytes
/// is a difference a caller could observe. The foreign peer's own bytes are
/// never compared to anything — only read.
#[test]
fn jacorb_calls_a_python_servant() {
    let Some(fixture) = JacorbFixture::prepare() else {
        println!(
            "UNMEASURED: the JacORB fixture is absent (JDK 21, or spikes/jacorb/lib) — \
             foreign peer × big-endian is unmeasured, not passing; \
             run spikes/jacorb/setup.sh --jars-only"
        );
        return;
    };
    let expect =
        std::env::var("ORBWEAVER_JACORB_EXPECT_ORDER").unwrap_or_else(|_| "big".to_owned());

    // 1.2 is JacORB's default. 1.1 is reached the way `spikes/jacorb_giop11.sh`
    // reaches it: not by a property, but by republishing the profile, because a
    // peer's outbound version follows the profile it dialled.
    for version in [Version::V1_2, Version::V1_1] {
        let Some(servant) = start_servant() else {
            println!("UNMEASURED: python3 is not available; the Python servant was not started");
            return;
        };
        let target: SocketAddr = {
            let p = servant.ior().profiles.first().expect("the servant published a profile");
            format!("{}:{}", p.host, p.port).parse().expect("the servant's address")
        };

        let python_frames;
        let python_out;
        {
            let tap = Tap::start(target);
            python_out = fixture.run(&tap.through(servant.ior(), Some(version)), "python");
            python_frames = tap.frames();
        }
        drop(servant);

        let mut rust_frames = Vec::new();
        let mut rust_out = String::new();
        with_rust_servant(|ior, addr| {
            let tap = Tap::start(addr);
            rust_out = fixture.run(&tap.through(ior, Some(version)), "rust");
            rust_frames = tap.frames();
        });

        // ── what the flag byte said ──────────────────────────────────────────
        let requests: Vec<&Frame> =
            python_frames.iter().filter(|f| f.from_client && f.mtype == 0).collect();
        let replies: Vec<&Frame> =
            python_frames.iter().filter(|f| !f.from_client && f.mtype == 1).collect();
        assert!(
            !requests.is_empty() && !replies.is_empty(),
            "the tap recorded no exchange at {version}; the fixture ran but measured nothing"
        );
        let request_orders = orders(&requests);
        let reply_orders = orders(&replies);
        println!(
            "read off the wire at {version}: {} request(s) from JacORB, flag byte says {}; \
             {} reply(ies) from our server, {}; GIOP {} in, {} out",
            requests.len(),
            request_orders.join(" and "),
            replies.len(),
            reply_orders.join(" and "),
            versions(&requests).join(","),
            versions(&replies).join(","),
        );
        assert_eq!(
            request_orders,
            vec![expect.as_str()],
            "at {version} the peer's requests were {request_orders:?}, and the byte order \
             is what this test exists to measure"
        );
        assert_eq!(
            reply_orders, request_orders,
            "at {version} our server answered in {reply_orders:?} a peer that wrote \
             {request_orders:?}; §15.4.1 lets each message choose, and `server.rs` replies \
             in the request's order"
        );
        let wire_version = format!("{}.{}", version.major, version.minor);
        assert_eq!(
            versions(&requests),
            vec![wire_version.clone()],
            "the profile was republished at IIOP {wire_version} and the peer did not follow it"
        );

        // ── the two servants, byte for byte ──────────────────────────────────
        let python_replies: Vec<&Vec<u8>> = python_frames
            .iter()
            .filter(|f| !f.from_client && f.mtype == 1)
            .map(|f| &f.bytes)
            .collect();
        let rust_replies: Vec<&Vec<u8>> = rust_frames
            .iter()
            .filter(|f| !f.from_client && f.mtype == 1)
            .map(|f| &f.bytes)
            .collect();
        assert_eq!(
            python_replies.len(),
            rust_replies.len(),
            "the two servants answered a different number of times at {version}: \
             python {}, rust {}",
            python_replies.len(),
            rust_replies.len()
        );
        for (i, (p, r)) in python_replies.iter().zip(rust_replies.iter()).enumerate() {
            assert_eq!(
                hex(p),
                hex(r),
                "reply {i} of {} at {version} differs between the two servants, so a \
                 caller can tell which language answered (D029 §6.1's Language row)",
                python_replies.len()
            );
        }
        println!(
            "byte-identical at {version}: {} reply(ies), python servant vs rust servant, \
             answering the same {}-endian peer",
            python_replies.len(),
            request_orders.join("/")
        );

        // Last, because it is the weakest of the three: a transcript is what a
        // peer *decoded*, and the comparison above already refuses anything a
        // peer could decode identically from different bytes. It is still
        // asserted, because it is the only one that sees the servant's
        // implementation leaking into what a caller reads.
        assert_transcript(&python_out, "python");
        assert_transcript(&rust_out, "rust");
    }
}
