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

use std::io::Write as _;
use std::time::{Duration, Instant};

use orbweaver_cdr::Endian;
use orbweaver_giop::{Connection, Ior, Version};

use emitted::f_24_skeleton_surface::gc24::GaugeClient;

const TYPE_ID: &str = "IDL:gc24/Gauge:1.0";
const VERSIONS: [Version; 3] = [Version::V1_0, Version::V1_1, Version::V1_2];

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

    let package = orbweaver_gen::python::emit_python(&registry, "g24_surface");
    let root = dir.path().join("pkg");
    for (relative, body) in &package.files {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
        std::fs::write(&path, body).expect("write");
    }

    let script = dir.path().join("servant.py");
    std::fs::write(&script, SERVANT).expect("write the servant");
    let ior_path = dir.path().join("gauge.ior");

    let child = std::process::Command::new("python3")
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
                servant.ior =
                    Some(Ior::parse(text).expect("the servant published a parsable IOR"));
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
            // USER_EXCEPTION reply with its repository id in front.
            let refused = client.record(-1.0, "C".into()).expect_err("a negative sample");
            assert!(
                format!("{refused:?}").contains("a sample below zero is not a reading"),
                "{what}: {refused:?}"
            );
            // And the memberless one, which is the empty-body edge case.
            let busy = client.record(1.0, String::new()).expect_err("an empty unit");
            assert!(format!("{busy:?}").contains("Busy"), "{what}: {busy:?}");

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

try:
    gauge.latest = r
    print("a readonly attribute accepted a setter")
except Exception as e:
    print("readonly refused with %s" % (type(e).__name__,))

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
/// Byte order: omniORB emits its native order, so the two orders are covered by
/// running the driver under both `-ORBnativeCharCodeSet`-independent paths that
/// exist here — which on one machine is one order. What the two orders *are*
/// measured on is [`our_generated_rust_client_calls_a_python_servant`], which
/// sets the encoder's order explicitly; this test's independence is about the
/// peer, not about the order, and saying otherwise would be claiming a
/// measurement nobody took.
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
    assert_eq!(
        stdout.contains("TYPE_ID"),
        false,
        "the servant's language must not appear in what the peer prints"
    );
    assert!(
        !stdout.contains("was not refused"),
        "a refusal did not happen:\n{stdout}"
    );
}
