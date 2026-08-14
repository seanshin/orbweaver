//! A generated servant that fails, driven over real GIOP.
//!
//! `corpus/golden/25-servant-faults.idl` declares not one `raises` clause, and
//! every operation in it fails: an absent key is `OBJECT_NOT_EXIST`, a refused
//! caller is `NO_PERMISSION`, an empty key is `BAD_PARAM` with a minor code,
//! and a rotation already under way is `TRANSIENT`. None of that is declarable
//! in IDL, which is the whole point — when the servant trait's error type was
//! the user exceptions alone, this contract produced an **uninhabited** error
//! type and a servant that could not fail at all.
//!
//! Three claims, in ascending order of how much they are worth:
//!
//! * [`the_four_system_exceptions_reach_our_own_client`] — id and minor code
//!   survive the round trip, every version, both byte orders;
//! * [`the_completion_status_is_the_servants_and_not_the_generators`] — the
//!   negative control for the whole design. Two calls on the same servant
//!   answer `COMPLETED_NO` and `COMPLETED_MAYBE`, so no constant the generator
//!   picked could produce both;
//! * [`omniorb_python_sees_the_faults_by_class`] — a foreign ORB, which knows
//!   nothing about this project, catching them as `CORBA.NO_PERMISSION` and
//!   friends.

mod emitted;

use std::io::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_gen::rt::{self, Completion, Dispatch, DispatchBody, ObjectHome, Server};
use orbweaver_giop::server::{Request, decode_request};
use orbweaver_giop::{
    Connection, DEFAULT_MAX_MESSAGE_SIZE, Error as GiopError, Ior, Version, encode_request,
    read_message,
};

use emitted::f_25_servant_faults::fault25::{
    VaultClient, VaultFault, VaultRefs, VaultServant, VaultSkeleton, VaultTarget,
};

const KEY: &[u8] = b"vault";
const TYPE_ID: &str = "IDL:fault25/Vault:1.0";
const VERSIONS: [Version; 3] = [Version::V1_0, Version::V1_1, Version::V1_2];

/// The minor code `store` attaches to a `BAD_PARAM`, in OMG's own space so it
/// means the same thing to a peer that never saw this contract.
const EMPTY_KEY_MINOR: u32 = rt::OMG_VMCID | 11;

// ── The hand-written half ────────────────────────────────────────────────────

/// A vault whose every failure is a system exception.
///
/// Nothing here mentions GIOP, and nothing here *could* have said any of this
/// in IDL. Note what the raises look like: `rt::raise::no_permission()` is not
/// a `SystemException` yet — it becomes one only at `.did_not_run()`, which is
/// how the completion status stays a decision.
#[derive(Default)]
struct Store {
    entries: Vec<(String, String)>,
    /// Set while a rotation is under way; the flag that makes `rotate` refuse.
    rotating: bool,
    /// Cleared to make every `store` a refusal, standing in for a caller whose
    /// scope the servant does not accept.
    may_write: bool,
    /// What the last dropped oneway fault was, so the test can see that the
    /// servant really did fail where nothing could be reported.
    forgot: Option<String>,
}

impl Store {
    fn open() -> Self {
        Self { entries: vec![("alpha".into(), "first".into())], may_write: true, ..Self::default() }
    }
}

impl VaultServant for Store {
    /// One object, addressed by the bare root key the server was bound with.
    fn knows(&self, __at: &VaultTarget<'_>) -> bool {
        __at.is_default()
    }

    fn fetch(&mut self, __at: &VaultTarget<'_>, key: String) -> Result<String, VaultFault> {
        match self.entries.iter().find(|(k, _)| *k == key) {
            Some((_, v)) => Ok(v.clone()),
            // The key names nothing here. Nothing ran, so a client may safely
            // ask somewhere else.
            None => Err(rt::raise::object_not_exist().did_not_run().into()),
        }
    }

    fn store(
        &mut self,
        __at: &VaultTarget<'_>,
        key: String,
        text: String,
    ) -> Result<(), VaultFault> {
        if !self.may_write {
            // Refused, and a retry will not change that — which is exactly
            // what distinguishes NO_PERMISSION from TRANSIENT.
            return Err(rt::raise::no_permission().did_not_run().into());
        }
        if key.is_empty() {
            // Which argument was wrong is invisible without a minor code.
            return Err(rt::raise::bad_param().minor(EMPTY_KEY_MINOR).did_not_run().into());
        }
        self.entries.push((key, text));
        Ok(())
    }

    fn rotate(&mut self, __at: &VaultTarget<'_>, wanted: i32) -> Result<i32, VaultFault> {
        if self.rotating {
            // The refusal lands *after* the generation counter moved, so
            // COMPLETED_NO would be a lie and COMPLETED_MAYBE is the truth.
            // This is the one place in the file where the completion status is
            // not `No`, and the reason the choice cannot be a generator's.
            return Err(rt::raise::transient().may_have_run().into());
        }
        self.rotating = true;
        Ok(wanted + 1)
    }

    fn forget(&mut self, __at: &VaultTarget<'_>, key: String) -> Result<(), VaultFault> {
        self.forgot = Some(key.clone());
        if self.entries.iter().any(|(k, _)| *k == key) {
            self.entries.retain(|(k, _)| *k != key);
            return Ok(());
        }
        // §9.4.1 gives this nowhere to go. The skeleton logs it and drops it.
        Err(rt::raise::object_not_exist().did_not_run().into())
    }

    fn depth(&mut self, __at: &VaultTarget<'_>) -> Result<i32, VaultFault> {
        Ok(self.entries.len() as i32)
    }
}

// ── Harness ──────────────────────────────────────────────────────────────────

/// The key scheme for a one-object vault: the bare root key and nothing else.
fn refs() -> VaultRefs {
    VaultRefs::new(ObjectHome::new("127.0.0.1", 0, KEY.to_vec()))
}

/// Runs `f` against a live server whose dispatcher is the generated skeleton.
fn with_server<F: FnOnce(&Ior)>(may_write: bool, f: F) {
    let server = Server::bind("127.0.0.1:0", KEY.to_vec()).expect("bind");
    let addr = server.local_addr().expect("addr");
    let ior = server.ior(TYPE_ID, "127.0.0.1").expect("ior");
    let home = ObjectHome::of(&server, "127.0.0.1").expect("home");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let t = std::thread::spawn(move || {
        let mut skeleton =
            VaultSkeleton::new(VaultRefs::new(home), Store { may_write, ..Store::open() });
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

/// The (id, minor, completed) a call failed with, or `None` if it succeeded.
fn system_fault<T>(r: Result<T, GiopError>) -> Option<(String, u32, u32)> {
    match r {
        Err(GiopError::SystemException { id, minor, completed }) => Some((id, minor, completed)),
        _ => None,
    }
}

/// One decoded `Request`, built straight from our encoder.
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

// ── The claims ───────────────────────────────────────────────────────────────

/// Every one of the four, over every version and both byte orders. What is
/// being checked is that the *servant's* exception arrives — id and minor code
/// intact — rather than the `UNKNOWN` a lost fault would become.
#[test]
fn the_four_system_exceptions_reach_our_own_client() {
    // A server per case: `rotate` is a state machine, and a servant shared
    // across the whole loop would answer the second iteration's first
    // rotation with the first iteration's leftover state.
    for version in VERSIONS {
        for endian in [Endian::Big, Endian::Little] {
            with_server(true, |ior| {
                let what = format!("{version} {endian:?}");
                let mut client = VaultClient::new(connect(ior, version, endian));

                assert_eq!(client.fetch("alpha".into()).expect("a key that is there"), "first");

                let absent = system_fault(client.fetch("nope".into())).expect("a system fault");
                let no = Completion::No as u32;
                assert_eq!(absent, (rt::OBJECT_NOT_EXIST.to_owned(), 0, no), "{what}");

                let empty = system_fault(client.store(String::new(), "x".into())).expect("fault");
                assert_eq!(
                    empty,
                    (rt::BAD_PARAM.to_owned(), EMPTY_KEY_MINOR, no),
                    "{what}: the minor code is the only thing naming the argument"
                );

                assert_eq!(client.rotate(1).expect("the first rotation"), 2, "{what}");
                let busy = system_fault(client.rotate(2)).expect("a system fault");
                assert_eq!(busy, (rt::TRANSIENT.to_owned(), 0, Completion::Maybe as u32), "{what}");

                // A raise is an answer, not a fault of the connection.
                assert!(client.depth().is_ok(), "{what}: the connection must survive a raise");
            });
        }
    }

    // The refusal that needs a differently-configured servant.
    with_server(false, |ior| {
        let mut client = VaultClient::new(connect(ior, Version::V1_2, Endian::Big));
        let refused = system_fault(client.store("k".into(), "v".into())).expect("a system fault");
        assert_eq!(refused, (rt::NO_PERMISSION.to_owned(), 0, Completion::No as u32));
    });
}

/// The negative control for the whole design.
///
/// If the generator picked the completion status, every raise from one servant
/// would carry the same one. These two come from the same servant over the
/// same connection and differ, which no constant can do. The distinction is
/// load-bearing: `COMPLETED_NO` tells a client its call never ran and a retry
/// is safe, and a retry loop that believes that about a mutation which already
/// half-happened is how state gets corrupted.
#[test]
fn the_completion_status_is_the_servants_and_not_the_generators() {
    for endian in [Endian::Big, Endian::Little] {
        with_server(true, |ior| {
            let mut client = VaultClient::new(connect(ior, Version::V1_2, endian));

            let (_, _, absent) = system_fault(client.fetch("nope".into())).expect("fault");
            assert_eq!(absent, Completion::No as u32, "{endian:?}: nothing ran");

            client.rotate(1).expect("the first rotation");
            let (_, _, busy) = system_fault(client.rotate(2)).expect("fault");
            assert_eq!(busy, Completion::Maybe as u32, "{endian:?}: the counter had moved");

            assert_ne!(absent, busy, "{endian:?}: one servant, two answers");
        });
    }
}

/// A system exception replaces the reply; it is not a body under a status. So
/// the dispatcher must write **nothing** into the reply encoder and hand the
/// exception back for the server to encode — a half-written body followed by a
/// system exception would be neither one thing nor the other.
#[test]
fn a_raising_servant_writes_no_reply_body() {
    for version in VERSIONS {
        for endian in [Endian::Big, Endian::Little] {
            let req = request(version, endian, "fetch", true, |e| e.put_str("nope"));
            let mut skeleton = VaultSkeleton::new(refs(), Store::open());
            let mut out = Encoder::continuing_at(endian, 24);
            let ex = skeleton.dispatch_body(&req, &mut out).expect_err("the servant raised");
            assert_eq!(ex.id, rt::OBJECT_NOT_EXIST, "{version} {endian:?}");
            assert!(
                out.finish().expect("finish").is_empty(),
                "{version} {endian:?}: a system exception writes no body"
            );
        }
    }
}

/// §9.4.1 leaves a oneway's fault nowhere to go. It is dropped — but the
/// servant did run and did fail, and the connection must be undisturbed.
#[test]
fn a_oneway_fault_is_dropped_and_the_connection_survives_it() {
    for endian in [Endian::Big, Endian::Little] {
        let req = request(Version::V1_2, endian, "forget", false, |e| e.put_str("nope"));
        let mut skeleton = VaultSkeleton::new(refs(), Store::open());
        let mut out = Encoder::continuing_at(endian, 24);
        let kind = skeleton.dispatch_body(&req, &mut out).expect("a oneway never fails outward");
        assert_eq!(kind, DispatchBody::Return, "{endian:?}");
        assert!(out.finish().expect("finish").is_empty(), "{endian:?}: no reply, at all");
        // The servant genuinely failed; only the report had nowhere to go.
        assert_eq!(skeleton.servant.forgot.as_deref(), Some("nope"), "{endian:?}");
    }

    with_server(true, |ior| {
        let mut client = VaultClient::new(connect(ior, Version::V1_2, Endian::Big));
        client.forget("nope".into()).expect("the oneway itself cannot fail");
        // If the skeleton had answered the oneway, this reply would be that
        // one and the count would be wrong.
        assert_eq!(client.depth().expect("depth"), 1);
    });
}

// ── The claim worth having: omniORB's own client ─────────────────────────────

/// omniORB's Python client, generated by `omniidl -bpython` from the same
/// corpus file, catching a Rust servant's raises **by CORBA class**.
///
/// omniORB is a fixture, never a dependency (`CLAUDE.md`): a separate process
/// over TCP, nothing linked, vendored or copied. This is the only test in this
/// file whose verdict does not come from our own code.
///
/// When the fixture is absent the test reports what it did not measure and
/// passes; `run_checks.sh` is where an absent fixture is a counted skip.
#[test]
fn omniorb_python_sees_the_faults_by_class() {
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
    with_server(true, |ior| {
        let ior_path = dir.path().join("vault.ior");
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
        "narrowed to fault25.Vault",
        "fetch(alpha) -> first",
        "CORBA.OBJECT_NOT_EXIST minor 0",
        // 0x4f4d0000 | 11: OMG's vendor space, so the code means the same
        // thing to a peer that has never seen this contract.
        "CORBA.BAD_PARAM minor 1330446347",
        "rotate(1) -> 2",
        "CORBA.TRANSIENT minor 0",
        "depth = 1",
        "OK",
    ] {
        assert!(output.contains(expected), "omniORB did not report {expected:?}:\n{output}");
    }
    assert_completion_is_read_as_measured(&output);
}

/// What omniORB makes of the completion status we sent.
///
/// §4.11.4 declares `enum completion_status { COMPLETED_YES, COMPLETED_NO,
/// COMPLETED_MAYBE }`, so COMPLETED_YES is ordinal 0 and COMPLETED_NO is 1.
/// This test was written while `orbweaver_giop::server::Completion` numbered
/// them the other way — `No = 0, Yes = 1`, a transposition of exactly the two
/// values that decide whether a caller may retry — and it pinned that as
/// *measured* rather than as correct, because the defect was in another crate.
/// The renumbering has since landed, so these expectations now read the way a
/// foreign ORB should see them, and this test is what catches the
/// transposition coming back.
///
/// MAYBE is 2 either way, which is why only two of the three were ever wrong,
/// and why nothing local caught it: our client compared against the same enum
/// and agreed with itself. It took an ORB we did not write to disagree.
///
/// 재시도 안전성을 결정하는 두 값이 뒤바뀌어 있었다. 우리 클라이언트는 같은
/// enum으로 비교하므로 스스로와는 늘 일치했고, 외부 ORB만이 이견을 낼 수 있었다.
fn assert_completion_is_read_as_measured(output: &str) {
    for (what, reads_as) in [
        // The servant said did_not_run() for both, and that is now what a
        // foreign ORB reads.
        ("CORBA.OBJECT_NOT_EXIST", "COMPLETED_NO"),
        ("CORBA.BAD_PARAM", "COMPLETED_NO"),
        ("CORBA.TRANSIENT", "COMPLETED_MAYBE"),
    ] {
        let line = output
            .lines()
            .find(|l| l.starts_with(what))
            .unwrap_or_else(|| panic!("no {what} line:\n{output}"));
        assert!(
            line.ends_with(reads_as),
            "omniORB read the completion status of {what} as {line:?}, not {reads_as:?}. \
             `Completion` follows §4.11.4 (COMPLETED_YES = 0, COMPLETED_NO = 1, MAYBE = 2); a \
             change that reorders it transposes retry safety and fails here, which is this \
             test's whole job."
        );
    }
}

/// The refusal, which needs the other servant configuration, so it gets its
/// own short run rather than a mode switch on the wire.
#[test]
fn omniorb_python_sees_a_refusal_as_no_permission() {
    let Some(dir) = omniidl_python_stubs() else {
        eprintln!("UNMEASURED: omniORB's Python client is absent; NO_PERMISSION was not measured");
        return;
    };
    let script = dir.path().join("refused.py");
    std::fs::write(&script, PYTHON_REFUSED).expect("write the driver");

    let mut output = String::new();
    with_server(false, |ior| {
        let ior_path = dir.path().join("refused.ior");
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
    assert!(
        output.contains("CORBA.NO_PERMISSION minor 0"),
        "omniORB did not report the refusal by class:\n{output}"
    );
    // did_not_run() now reaches omniORB as COMPLETED_NO, which is what makes a
    // refused call safely re-sendable by a client we did not write.
    assert!(
        output.contains("CORBA.NO_PERMISSION minor 0 completed COMPLETED_NO"),
        "the completion status omniORB read has changed:\n{output}"
    );
    assert!(output.contains("OK"), "{output}");
}

/// Runs `omniidl -bpython` over the corpus file, into a temporary directory.
fn omniidl_python_stubs() -> Option<TempDir> {
    let importable = std::process::Command::new("python3")
        .args(["-c", "import omniORB"])
        .output()
        .ok()
        .is_some_and(|o| o.status.success());
    if !importable {
        return None;
    }

    let dir = TempDir::new("orbweaver-faults")?;
    let idl = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/golden/25-servant-faults.idl");
    // `omniidl -bpython` names the module after the file, and
    // `25-servant-faults_idl` is not a Python identifier.
    let copied = dir.path().join("fault25_surface.idl");
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

/// A temporary directory that removes itself.
///
/// The name carries a counter as well as the clock: two tests in this file
/// want stubs at once, cargo runs them on two threads, and a name built from
/// the pid and the sub-second clock alone collided — the first test to finish
/// deleted the directory the second was still writing into. A single-source
/// unique suffix is cheaper than diagnosing that twice.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(prefix: &str) -> Option<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.subsec_nanos();
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("{prefix}-{}-{nanos}-{n}", std::process::id()));
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

/// Fixed text, never generated. Written the way an omniORB user writes one:
/// `except CORBA.NO_PERMISSION`, by class, with the minor code and completion
/// status read off the exception omniORB constructed from our bytes.
const PYTHON_DRIVER: &str = r#"import sys, os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from omniORB import CORBA
import fault25_surface_idl  # noqa: F401  -- registers the fault25 module
import fault25

COMPLETION = {
    CORBA.COMPLETED_NO: "COMPLETED_NO",
    CORBA.COMPLETED_YES: "COMPLETED_YES",
    CORBA.COMPLETED_MAYBE: "COMPLETED_MAYBE",
}

def report(name, ex):
    print("%s minor %d completed %s" % (name, ex.minor, COMPLETION.get(ex.completed, "?")))

orb = CORBA.ORB_init(sys.argv, CORBA.ORB_ID)
with open(sys.argv[1]) as f:
    ior = f.read().strip()

obj = orb.string_to_object(ior)
vault = obj._narrow(fault25.Vault)
if vault is None:
    print("NARROW FAILED")
    sys.exit(1)
print("narrowed to fault25.Vault")

print("fetch(alpha) ->", vault.fetch("alpha"))

try:
    vault.fetch("nope")
    print("NO EXCEPTION RAISED")
except CORBA.OBJECT_NOT_EXIST as ex:
    report("CORBA.OBJECT_NOT_EXIST", ex)

try:
    vault.store("", "x")
    print("NO EXCEPTION RAISED")
except CORBA.BAD_PARAM as ex:
    report("CORBA.BAD_PARAM", ex)

print("rotate(1) ->", vault.rotate(1))
try:
    vault.rotate(2)
    print("NO EXCEPTION RAISED")
except CORBA.TRANSIENT as ex:
    report("CORBA.TRANSIENT", ex)

# A oneway whose servant fails: omniORB does not wait, and the next twoway
# must be answered by itself rather than by a stray reply.
vault.forget("nope")
print("depth =", vault.depth)
print("OK")
"#;

/// The refusal, against a servant that accepts no writes.
const PYTHON_REFUSED: &str = r#"import sys, os

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from omniORB import CORBA
import fault25_surface_idl  # noqa: F401
import fault25

COMPLETION = {
    CORBA.COMPLETED_NO: "COMPLETED_NO",
    CORBA.COMPLETED_YES: "COMPLETED_YES",
    CORBA.COMPLETED_MAYBE: "COMPLETED_MAYBE",
}

orb = CORBA.ORB_init(sys.argv, CORBA.ORB_ID)
with open(sys.argv[1]) as f:
    ior = f.read().strip()

vault = orb.string_to_object(ior)._narrow(fault25.Vault)
try:
    vault.store("k", "v")
    print("NO EXCEPTION RAISED")
except CORBA.NO_PERMISSION as ex:
    print("CORBA.NO_PERMISSION minor %d completed %s" % (ex.minor, COMPLETION[ex.completed]))
print("OK")
"#;
