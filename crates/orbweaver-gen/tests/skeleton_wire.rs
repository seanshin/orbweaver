//! A generated skeleton, hand-implemented and driven over real GIOP.
//!
//! Compiling generated code proves it parses. This drives it: the servant trait
//! for `corpus/golden/24-skeleton-surface.idl` is implemented by hand below —
//! the thing an application author actually writes — and the generated
//! dispatcher serves it to our own client across every GIOP version and both
//! byte orders, and to **omniORB's Python client**, which knows nothing about
//! this project and agrees or does not.
//!
//! The three hazards the corpus file was added for get their own tests, because
//! each of them passes every "does it compile" check and fails on the wire:
//!
//! * [`a_oneway_gets_no_reply_and_the_connection_survives_it`]
//! * [`a_readonly_attribute_refuses_its_setter_with_bad_operation`]
//! * [`the_reply_body_aligns_from_the_message_not_from_the_buffer`]

mod emitted;

use std::io::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_gen::rt::{Cdr, Dispatch, DispatchBody, ObjectHome, Server};
use orbweaver_giop::server::{Request, decode_request};
use orbweaver_giop::{
    Connection, DEFAULT_MAX_MESSAGE_SIZE, Error as GiopError, Ior, Version, encode_request,
    read_message,
};

use emitted::f_24_skeleton_surface::gc24::{
    GaugeClient, GaugeFault, GaugeRefs, GaugeServant, GaugeSkeleton, GaugeTarget, Reading, Rejected,
};

const KEY: &[u8] = b"gauge";
const TYPE_ID: &str = "IDL:gc24/Gauge:1.0";

// ── The hand-written half: what an application author writes ─────────────────

/// A gauge. Nothing here mentions GIOP, an `Encoder`, or an operation name —
/// which is the whole claim a skeleton makes.
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
    /// One object, addressed by the bare root key the server was bound with.
    fn knows(&self, __at: &GaugeTarget<'_>) -> bool {
        __at.is_default()
    }

    fn latest(&mut self, __at: &GaugeTarget<'_>) -> Result<Reading, GaugeFault> {
        Ok(self.latest.clone())
    }

    fn label(&mut self, __at: &GaugeTarget<'_>) -> Result<String, GaugeFault> {
        Ok(self.label.clone())
    }

    fn set_label(&mut self, __at: &GaugeTarget<'_>, value: String) -> Result<(), GaugeFault> {
        self.label = value;
        Ok(())
    }

    fn record(
        &mut self,
        __at: &GaugeTarget<'_>,
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
            return Err(GaugeFault::Busy(emitted::f_24_skeleton_surface::gc24::Busy {}));
        }
        self.samples.push(sample);
        self.latest =
            Reading { at: sample, sequence_no: self.samples.len() as i32, unit: unit.clone() };
        Ok(self.latest.clone())
    }

    fn scale_all(&mut self, __at: &GaugeTarget<'_>, e: f64) -> Result<i32, GaugeFault> {
        for s in &mut self.samples {
            *s *= e;
        }
        self.latest.at *= e;
        Ok(self.samples.len() as i32)
    }

    fn reset(&mut self, __at: &GaugeTarget<'_>) -> Result<(), GaugeFault> {
        self.samples.clear();
        self.latest = Reading { at: 0.0, sequence_no: 0, unit: String::new() };
        Ok(())
    }

    fn split(&mut self, __at: &GaugeTarget<'_>) -> Result<(f64, String), GaugeFault> {
        Ok((self.latest.at, self.latest.unit.clone()))
    }
}

// ── Harness ──────────────────────────────────────────────────────────────────

/// The key scheme for a one-object gauge: the bare root key and nothing else.
///
/// The port is zero because a `Gauge` mints no references — its contract
/// returns no object reference anywhere — so nothing here ever reads it. Where
/// a server exists, `ObjectHome::of` takes the real one.
fn refs() -> GaugeRefs {
    GaugeRefs::new(ObjectHome::new("127.0.0.1", 0, KEY.to_vec()))
}

/// Runs `f` against a live server whose dispatcher is the generated skeleton.
///
/// The stop flag is observed between accepts, so the loop is woken with one
/// throwaway connection rather than left blocked in `accept` — a sleeping,
/// deadline-free wake instead of a spin, per `CLAUDE.md`'s harness rules.
fn with_server<F: FnOnce(&Ior)>(f: F) {
    let server = Server::bind("127.0.0.1:0", KEY.to_vec()).expect("bind");
    let addr = server.local_addr().expect("addr");
    let ior = server.ior(TYPE_ID, "127.0.0.1").expect("ior");
    let home = ObjectHome::of(&server, "127.0.0.1").expect("home");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let t = std::thread::spawn(move || {
        let mut skeleton = GaugeSkeleton::new(GaugeRefs::new(home), Bench::default());
        server.serve(&mut skeleton, || flag.load(Ordering::SeqCst)).expect("serve");
    });

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&ior)));

    stop.store(true, Ordering::SeqCst);
    let _ = std::net::TcpStream::connect(addr); // wake the accept loop
    t.join().expect("the server thread must not panic");
    if let Err(p) = outcome {
        std::panic::resume_unwind(p);
    }
}

fn connect(ior: &Ior, version: Version, endian: Endian) -> Connection {
    let mut conn = Connection::connect(ior, Duration::from_secs(5)).expect("connect");
    conn.cap_version(version);
    conn.set_endian(endian);
    conn
}

/// One decoded `Request`, built straight from our encoder — no socket, so the
/// dispatcher can be handed a message with a chosen version and origin.
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

const VERSIONS: [Version; 3] = [Version::V1_0, Version::V1_1, Version::V1_2];

// ── Hazard 1: oneway ─────────────────────────────────────────────────────────

/// §9.4.1: a oneway gets **no reply at all**. An empty `NO_EXCEPTION` reply is
/// not "almost nothing" — it is a whole message the peer is not waiting for, so
/// it reads it as the header of the next reply and every later request on that
/// connection is answered with the wrong bytes.
///
/// Two halves: the dispatcher writes nothing, and a live connection keeps its
/// framing across a oneway. The second is the one that would have caught it.
#[test]
fn a_oneway_gets_no_reply_and_the_connection_survives_it() {
    for version in VERSIONS {
        for endian in [Endian::Big, Endian::Little] {
            let req = request(version, endian, "reset", false, |_| {});
            let mut skeleton = GaugeSkeleton::new(refs(), Bench::default());
            let mut out = Encoder::continuing_at(endian, 24);
            let kind = skeleton.dispatch_body(&req, &mut out).expect("dispatch");
            assert_eq!(kind, DispatchBody::Return, "{version} {endian:?}");
            assert!(
                out.finish().expect("finish").is_empty(),
                "{version} {endian:?}: a oneway may write no reply body"
            );
        }
    }

    with_server(|ior| {
        for version in VERSIONS {
            for endian in [Endian::Big, Endian::Little] {
                let mut client = GaugeClient::new(connect(ior, version, endian));
                client.set_label("before".into()).expect("set_label");
                client.record(2.5, "C".into()).expect("record");
                client.reset().expect("the oneway itself");
                // If the skeleton had written an empty reply to the oneway, the
                // next twoway would decode that reply instead of its own.
                assert_eq!(client.label().expect("label"), "before", "{version} {endian:?}");
                let after = client.record(1.5, "K".into()).expect("record after the oneway");
                assert_eq!(after.at, 1.5, "{version} {endian:?}");
                assert_eq!(after.sequence_no, 1, "the oneway did clear the samples");
            }
        }
    });
}

// ── Hazard 2: attributes ─────────────────────────────────────────────────────

/// `_get_x`/`_set_x` are operations on the wire, and `readonly` means the
/// setter does not exist — not that it exists and complains. A generator that
/// emits accessors from one template turns a read-only contract into a writable
/// one, and nothing but a wire test notices.
#[test]
fn a_readonly_attribute_refuses_its_setter_with_bad_operation() {
    with_server(|ior| {
        let mut conn = connect(ior, Version::V1_2, Endian::Big);

        let bad = conn
            .invoke("_set_latest", |e| {
                let _ = Reading { at: 1.0, sequence_no: 1, unit: "C".into() }.put(e);
            })
            .expect_err("a readonly attribute has no setter");
        assert!(
            matches!(&bad, GiopError::SystemException { id, .. } if id.contains("BAD_OPERATION")),
            "got {bad}"
        );

        let unknown = conn.invoke_nullary("no_such_operation").expect_err("unknown operation");
        assert!(
            matches!(&unknown, GiopError::SystemException { id, .. }
                if id.contains("BAD_OPERATION")),
            "got {unknown}"
        );

        // And the refusals cost the connection nothing: a BAD_OPERATION reply
        // is a reply, so the framing has to be intact behind it.
        let mut client = GaugeClient::new(conn);
        client.set_label("still here".into()).expect("set_label");
        assert_eq!(client.label().expect("label"), "still here");
        assert_eq!(client.latest().expect("latest").sequence_no, 0);
    });
}

// ── Hazard 3: alignment origin ───────────────────────────────────────────────

/// CDR alignment is measured from the first byte of the GIOP message. The
/// reply body is written into an encoder `continuing_at` the offset the body
/// will occupy, so a skeleton that built its body in a fresh buffer and copied
/// it would restart alignment at zero.
///
/// Honest note: [`Server`] hands over an origin of 24 today (a 12-byte header
/// plus 12 bytes of reply header with an empty service-context list), and 24 is
/// already 8-aligned, so the reply side cannot currently *distinguish* the two.
/// The second half of this test therefore dispatches at an origin of 20, which
/// can: the padding it demands exists only if the origin is honoured. The
/// request side needs no such contrivance — GIOP 1.0 and 1.1 do not align the
/// request body at all.
#[test]
fn the_reply_body_aligns_from_the_message_not_from_the_buffer() {
    let reading = Reading { at: 2.5, sequence_no: 3, unit: "C".into() };

    // The origin Server uses: no padding, the 8-aligned member leads.
    let req = request(Version::V1_2, Endian::Big, "_get_latest", true, |_| {});
    let mut skeleton =
        GaugeSkeleton::new(refs(), Bench { latest: reading.clone(), ..Bench::default() });
    let mut out = Encoder::continuing_at(Endian::Big, 24);
    assert_eq!(skeleton.dispatch_body(&req, &mut out).expect("dispatch"), DispatchBody::Return);
    let body = out.finish().expect("finish");
    assert_eq!(&body[..8], &2.5f64.to_be_bytes(), "an 8-aligned origin needs no padding");

    // An origin that is not 8-aligned: four bytes of padding must appear before
    // the double, and only an encoder that knows where it sits produces them.
    let mut out = Encoder::continuing_at(Endian::Big, 20);
    assert_eq!(skeleton.dispatch_body(&req, &mut out).expect("dispatch"), DispatchBody::Return);
    let body = out.finish().expect("finish");
    assert_eq!(&body[..4], &[0, 0, 0, 0], "20 → 24 is four bytes of padding");
    assert_eq!(&body[4..12], &2.5f64.to_be_bytes());

    // The request side, where the hazard is live rather than latent: under GIOP
    // 1.0 and 1.1 the body is not aligned at all, so the offset of a `double`
    // first argument depends on the object key and operation name lengths.
    for version in [Version::V1_0, Version::V1_1] {
        for endian in [Endian::Big, Endian::Little] {
            let req = request(version, endian, "record", true, |e| {
                e.put_f64(2.5);
                e.put_str("C");
            });
            assert_ne!(
                req.body().expect("body").offset() % 8,
                0,
                "{version}: this case only bites when the body is unaligned"
            );
            let mut skeleton = GaugeSkeleton::new(refs(), Bench::default());
            let mut out = Encoder::continuing_at(endian, 24);
            assert_eq!(
                skeleton.dispatch_body(&req, &mut out).expect("dispatch"),
                DispatchBody::Return
            );
            assert_eq!(skeleton.servant.latest.at, 2.5, "{version} {endian:?}");
            assert_eq!(skeleton.servant.latest.unit, "C", "{version} {endian:?}");
        }
    }
}

// ── The whole surface, over the wire ─────────────────────────────────────────

/// Every operation and accessor, every GIOP version, both byte orders. An
/// encoder that only works native-endian passes every local test and fails in
/// the field, so neither order is the default here.
#[test]
fn the_whole_generated_surface_answers_over_every_version_and_byte_order() {
    with_server(|ior| {
        for version in VERSIONS {
            for endian in [Endian::Big, Endian::Little] {
                let what = format!("{version} {endian:?}");
                let mut client = GaugeClient::new(connect(ior, version, endian));

                client.reset().expect("reset");
                client.set_label("bench".into()).expect("set_label");
                assert_eq!(client.label().expect("label"), "bench", "{what}");

                let r = client.record(2.5, "C".into()).expect("record");
                assert_eq!(r, Reading { at: 2.5, sequence_no: 1, unit: "C".into() }, "{what}");
                assert_eq!(client.latest().expect("latest"), r, "{what}");

                assert_eq!(client.scale_all(4.0).expect("scale_all"), 1, "{what}");
                let (at, unit) = client.split().expect("split");
                assert_eq!((at, unit.as_str()), (10.0, "C"), "{what}");
            }
        }
    });
}

/// A user exception, encoded as the wire expects: `USER_EXCEPTION` status, and
/// a body whose first item is the repository id, followed by the members. The
/// oracle for the shape is our own client, which hands back a reply positioned
/// exactly there.
#[test]
fn raised_user_exceptions_travel_as_user_exception_replies() {
    with_server(|ior| {
        for endian in [Endian::Big, Endian::Little] {
            let mut client = GaugeClient::new(connect(ior, Version::V1_2, endian));

            let err = client.record(-1.0, "C".into()).expect_err("a negative sample is rejected");
            match err {
                GiopError::UserException { id, reply } => {
                    assert_eq!(id, "IDL:gc24/Rejected:1.0", "{endian:?}");
                    let mut body = reply.body().expect("body");
                    assert_eq!(body.get_string().expect("the repository id"), id);
                    assert_eq!(
                        body.get_string().expect("why"),
                        "a sample below zero is not a reading"
                    );
                    assert_eq!(body.get_i32().expect("code"), 7);
                }
                other => panic!("{endian:?}: expected a user exception, got {other}"),
            }

            // A member-less exception is the repository id and nothing else.
            let err = client.record(1.0, String::new()).expect_err("an empty unit is refused");
            match err {
                GiopError::UserException { id, reply } => {
                    assert_eq!(id, "IDL:gc24/Busy:1.0", "{endian:?}");
                    let mut body = reply.body().expect("body");
                    assert_eq!(body.get_string().expect("the repository id"), id);
                    assert!(body.is_empty(), "{endian:?}: Busy has no members");
                }
                other => panic!("{endian:?}: expected a user exception, got {other}"),
            }

            // A raise is an answer, not a fault: the connection carries on.
            assert_eq!(client.record(3.0, "C".into()).expect("after the raise").at, 3.0);
        }
    });
}

/// An ORB probes with `_is_a` before it will narrow, so a skeleton that does
/// not answer it is one that cannot be narrowed to.
#[test]
fn the_object_probes_are_answered_from_the_registrys_inheritance_chain() {
    with_server(|ior| {
        let mut conn = connect(ior, Version::V1_2, Endian::Big);
        for (id, expected) in [
            (TYPE_ID, true),
            ("IDL:omg.org/CORBA/Object:1.0", true),
            ("IDL:omg.org/CosNaming/NamingContext:1.0", false),
        ] {
            let reply = conn.invoke("_is_a", move |e| e.put_str(id)).expect("_is_a");
            assert_eq!(reply.body().expect("body").get_bool().expect("bool"), expected, "{id}");
        }
        let reply = conn.invoke_nullary("_non_existent").expect("_non_existent");
        assert!(!reply.body().expect("body").get_bool().expect("bool"));
    });
}

// ── The claim worth having: omniORB's own client ─────────────────────────────

/// omniORB's Python client, generated by `omniidl -bpython` from the same
/// corpus file, driving our generated skeleton.
///
/// omniORB is a **fixture, never a dependency** (`CLAUDE.md`): it is run as a
/// separate process over TCP and nothing of it is linked, vendored or copied.
/// This is the only test here whose verdict does not come from our own code —
/// every other one has our encoder on both ends.
///
/// When the fixture is absent the test reports what it did not measure and
/// passes; `run_checks.sh` is where an absent fixture is a counted skip.
#[test]
fn omniorb_python_drives_the_generated_skeleton() {
    let Some(dir) = omniidl_python_stubs() else {
        eprintln!(
            "UNMEASURED: omniORB's Python client is absent (omniidl or the omniORB \
             module); the interop half of this test did not run"
        );
        return;
    };

    let script = dir.path().join("drive.py");
    std::fs::write(&script, PYTHON_DRIVER).expect("write the driver");

    let mut output = String::new();
    with_server(|ior| {
        let ior_path = dir.path().join("gauge.ior");
        let mut f = std::fs::File::create(&ior_path).expect("ior file");
        writeln!(f, "{}", ior.to_stringified().expect("stringify")).expect("write ior");
        drop(f);

        let out = std::process::Command::new("python3")
            .arg(&script)
            .arg(&ior_path)
            .current_dir(dir.path())
            .output()
            .expect("run python3");
        output = String::from_utf8_lossy(&out.stdout).into_owned();
        if !out.status.success() {
            panic!(
                "omniORB's client failed:\nstdout:\n{output}\nstderr:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    });

    eprintln!("omniORB python client said:\n{output}");
    for expected in [
        "narrowed to gc24.Gauge",
        "label = unset",
        "label = driven by omniORB",
        "record -> 2.5 1 C",
        "latest -> 2.5 C",
        "scale_all -> 1",
        "split -> 10.0 C",
        "after the oneway, label = driven by omniORB",
        "Rejected a sample below zero is not a reading 7",
        "Busy",
        "is_a NamingContext -> False",
        "non_existent -> False",
        "OK",
    ] {
        assert!(output.contains(expected), "omniORB did not report {expected:?}:\n{output}");
    }
}

/// Runs `omniidl -bpython` over the corpus file, into a temporary directory.
///
/// The IDL is copied to a Python-legal basename first: `omniidl -bpython` names
/// the module after the file, and `24-skeleton-surface_idl` is not an
/// identifier.
fn omniidl_python_stubs() -> Option<TempDir> {
    let importable = std::process::Command::new("python3")
        .args(["-c", "import omniORB"])
        .output()
        .ok()
        .is_some_and(|o| o.status.success());
    if !importable {
        return None;
    }

    let dir = TempDir::new("orbweaver-skel")?;
    let idl = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/golden/24-skeleton-surface.idl");
    let copied = dir.path().join("gc24_surface.idl");
    std::fs::copy(&idl, &copied).ok()?;

    let out = std::process::Command::new("omniidl")
        .args(["-bpython", "-C"])
        .arg(dir.path())
        .arg(&copied)
        .output()
        .ok()?;
    if !out.status.success() {
        eprintln!("omniidl -bpython failed: {}", String::from_utf8_lossy(&out.stderr));
        return None;
    }
    Some(dir)
}

/// A temporary directory that removes itself. Small enough to own rather than
/// to take a dependency for.
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

/// Fixed text, never generated: a driver the generator wrote for itself would
/// prove nothing. Deliberately written the way an omniORB user writes one —
/// attribute access, a raised exception caught by its generated Python class,
/// and a oneway followed by a twoway on the same connection.
const PYTHON_DRIVER: &str = r#"import sys, os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from omniORB import CORBA
import gc24_surface_idl  # noqa: F401  -- registers the gc24 module
import gc24

orb = CORBA.ORB_init(sys.argv, CORBA.ORB_ID)
with open(sys.argv[1]) as f:
    ior = f.read().strip()

obj = orb.string_to_object(ior)
gauge = obj._narrow(gc24.Gauge)
if gauge is None:
    print("NARROW FAILED")
    sys.exit(1)
print("narrowed to gc24.Gauge")

print("label =", gauge.label)
gauge.label = "driven by omniORB"
print("label =", gauge.label)

r = gauge.record(2.5, "C")
print("record ->", r.at, r.sequence_no, r.unit)
print("latest ->", gauge.latest.at, gauge.latest.unit)
print("scale_all ->", gauge.scale_all(4.0))
at, unit = gauge.split()
print("split ->", at, unit)

# oneway: omniORB sends response_expected = false and does not wait. A reply
# here would be read as the header of the next one.
gauge.reset()
gauge.label = "driven by omniORB"
print("after the oneway, label =", gauge.label)

try:
    gauge.record(-1.0, "C")
    print("NO EXCEPTION RAISED")
except gc24.Rejected as ex:
    print("Rejected", ex.why, ex.code)

try:
    gauge.record(1.0, "")
    print("NO EXCEPTION RAISED")
except gc24.Busy:
    print("Busy")

print("is_a NamingContext ->", gauge._is_a("IDL:omg.org/CosNaming/NamingContext:1.0"))
print("non_existent ->", gauge._non_existent())
print("OK")
"#;
