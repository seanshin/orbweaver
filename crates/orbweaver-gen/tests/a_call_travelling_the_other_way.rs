//! A reference *arriving* at a foreign servant, and the servant **invoking** it.
//!
//! # The sentence this file exists to make false
//!
//! `orbweaver_gen::seam`'s header has said it since the module was written, and
//! `PLAN-FIRST-COMPLETION` §1's L4 named it as the last open leak under D029
//! §6.1's Language row:
//!
//! > a reference *arriving* as an argument is still a handle the far side
//! > cannot invoke, because invoking it would need a call to travel the other
//! > way through `Answerer` and **this protocol has no message for that yet**.
//!
//! D038, approved 2026-08-31, option A: the far side sends `{"invoke": …}`
//! naming a handle, this side dials on its behalf, and the answer comes back as
//! `{"answer": …}` before the reply to the original call. The seam becomes
//! **re-entrant**, which D038 §2 says is a property and not a detail.
//!
//! # What is measured here
//!
//! A Python servant implements `gc16::Registry`. It is handed a reference to a
//! `Target` — a **Rust** servant on a **real socket**, bound by this test — as
//! the `ref` argument of `bind`, and it calls `ping()` on it.
//!
//! Two assertions, and **neither is sufficient alone**, which is the point:
//!
//! * the Rust `Target` records that `ping` was invoked **once**. Without this,
//!   a Python servant that returned successfully having done nothing would pass.
//! * the Python servant refuses unless the value it read back is [`PONG`]. So
//!   the answer travelled the other way and arrived intact, not merely that a
//!   connection was made.
//!
//! A green here means a caller cannot tell, from what the servant can *do* with
//! a reference it was given, whether that servant is written in Rust or in
//! Python — which is the Language row's claim.
//!
//! # What it does not measure
//!
//! The nested call's result reaches Python as AnyJSON rather than as a mapped
//! value, because this side knows only the repository id the reference
//! advertises and a generated stub's descriptors are what map a result. That is
//! stated in `ObjectRef.invoke`'s docstring and is a boundary, not a defect:
//! the marshalling is done where the contract is known, which is the Rust side
//! that resolves the operation in the registry.
//!
//! *도착한 참조를 저쪽이 **호출한다**. 파이썬 서번트가 `bind`로 받은 참조에
//! `ping()`을 걸고, 그 호출은 seam을 거슬러 올라와 이쪽이 대신 다이얼한다.
//! 단언은 둘이고 어느 하나로는 충분하지 않다 — 러스트 쪽이 `ping`을 한 번
//! 받았다는 것과, 파이썬이 읽어 온 값이 맞다는 것.*

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_gen::pychild::PythonChild;
use orbweaver_gen::seam::ForeignServant;
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{Dispatch, Request, SystemException, decode_request};
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, Ior, Version, encode_request, read_message};
use orbweaver_registry::{Contract, Registry, Strictness};

const PACKAGE: &str = "gc16_other_way";
const REGISTRY_ID: &str = "IDL:gc16/Registry:1.0";
const TARGET_ID: &str = "IDL:gc16/Target:1.0";
const ROOT: &[u8] = b"reg";

/// What the Rust `Target` answers `ping()` with.
///
/// Deliberately not 0 and not 1: a default-constructed anything answers 0, and
/// a counter answers 1, so an assertion that cannot tell the servant's value
/// from either is not evidence that the value travelled.
const PONG: i32 = 4242;

fn registry() -> Registry {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/golden/16-object-refs.idl");
    let contract = Contract::load(&path, &Default::default(), Strictness::Checked)
        .expect("the corpus contract must load");
    let mut r = Registry::new();
    r.load(&contract.spec).expect("the contract must build a registry");
    r
}

/// The Rust servant the Python one will be asked to call.
///
/// Hand-written rather than generated, so that what `ping` counts is visible in
/// this file: the count is half the measurement.
struct Target {
    pings: Arc<AtomicU32>,
}

impl Dispatch for Target {
    fn knows(&self, _object_key: &[u8]) -> bool {
        true
    }

    fn dispatch(&mut self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        if request.operation != "ping" {
            return Err(SystemException::bad_operation());
        }
        self.pings.fetch_add(1, Ordering::SeqCst);
        out.put_i32(PONG);
        Ok(())
    }
}

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

/// The Python servant, and the one line this whole batch is about: `ref.invoke`.
///
/// `nested` selects whether it makes the nested call at all — the control below
/// runs the same servant with it off, which is what shows the assertions can
/// fail.
fn program(nested: bool) -> String {
    let body = if nested {
        format!(
            "        got = ref.invoke(\"ping\")\n\
             \x20       if got != {PONG}:\n\
             \x20           raise _rt.Raise.ran_to_completion(\n\
             \x20               \"IDL:omg.org/CORBA/UNKNOWN:1.0\", 1)\n"
        )
    } else {
        // The control: the reference is received and never invoked. A servant
        // that does this is exactly what the seam allowed before D038.
        "        pass\n".to_owned()
    };
    format!(
        r#"
from {PACKAGE} import _rt
from {PACKAGE} import gc16


class Reg(gc16.RegistryServant):
    def lookup(self, name):
        raise _rt.Raise.did_not_run("IDL:omg.org/CORBA/NO_IMPLEMENT:1.0", 0)

    def describe(self, name):
        raise _rt.Raise.did_not_run("IDL:omg.org/CORBA/NO_IMPLEMENT:1.0", 0)

    def bind(self, name, ref):
{body}

_rt.serve_on_pipes(Reg())
"#
    )
}

/// Runs `f` with a live Rust `Target` on a real socket.
fn with_target<F: FnOnce(&Ior)>(pings: Arc<AtomicU32>, f: F) {
    let server = Orb::new().server("127.0.0.1:0", ROOT.to_vec()).expect("bind");
    let ior = server.ior(TARGET_ID, "127.0.0.1").expect("ior");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let t = std::thread::spawn(move || {
        let mut target = Target { pings };
        let _ = server.serve(&mut target, || flag.load(Ordering::SeqCst));
    });
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&ior)));
    stop.store(true, Ordering::SeqCst);
    // Unblock the accept loop so the thread can see the flag.
    let _ = orbweaver_giop::Connection::connect(&ior, std::time::Duration::from_millis(500));
    let _ = t.join();
    if let Err(e) = outcome {
        std::panic::resume_unwind(e);
    }
}

/// Encodes `bind("k", <ior>)` the way a peer would and dispatches it.
fn bind_through(servant: &mut ForeignServant<PythonChild>, target: &Ior) -> Result<(), String> {
    let wire = encode_request(Version::V1_2, Endian::Big, 1, ROOT, "bind", true, |e| {
        e.put_str("k");
        target.write_to(e).expect("an IOR encodes");
    })
    .expect("encode");
    let mut cursor: &[u8] = &wire;
    let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("frame");
    let request = decode_request(msg).expect("decode");
    let mut out = Encoder::new(Endian::Big);
    servant.dispatch(&request, &mut out).map_err(|e| format!("{e:?}"))?;
    let _ = Decoder::new(out.as_bytes(), Endian::Big);
    Ok(())
}

fn run(nested: bool) -> (Result<(), String>, u32) {
    let dir =
        std::env::temp_dir().join(format!("orbweaver-otherway-{}-{}", std::process::id(), nested));
    std::fs::create_dir_all(&dir).expect("a work directory");
    let root = site(&dir);
    let pings = Arc::new(AtomicU32::new(0));
    let counted = pings.clone();
    let mut result = Err("never ran".to_owned());
    with_target(counted, |ior| {
        let child = PythonChild::spawn(&program(nested), &[&root]).expect("python3 starts");
        let mut servant =
            ForeignServant::new(&registry(), REGISTRY_ID, child).expect("Registry resolves");
        result = bind_through(&mut servant, ior);
    });
    let _ = std::fs::remove_dir_all(&dir);
    let n = pings.load(Ordering::SeqCst);
    (result, n)
}

/// The leak leg: the far side uses a reference it was handed.
#[test]
fn a_python_servant_invokes_a_reference_it_was_handed() {
    let (result, pings) = run(true);
    assert!(
        result.is_ok(),
        "the Python servant refused `bind`, which it only does when the value it read back \
         from the nested call was not {PONG}: {result:?}"
    );
    assert_eq!(
        pings, 1,
        "the Rust target was invoked {pings} time(s), not once. A servant that answered \
         without calling would pass the assertion above by doing nothing, which is why the \
         count is asserted beside it"
    );
}

/// The control, and the reason the assertions above are worth anything.
///
/// The same servant with the nested call removed — which is exactly what the
/// seam allowed before D038 — must leave the target uncalled. If this passed,
/// the test above would be green in a world where nothing crossed.
#[test]
fn without_the_nested_call_the_target_is_never_reached() {
    let (result, pings) = run(false);
    assert!(result.is_ok(), "the control servant should answer `bind` without calling: {result:?}");
    assert_eq!(
        pings, 0,
        "the control invoked the target {pings} time(s). It makes no nested call, so a \
         non-zero count means something other than the servant is dialling and the \
         measurement above is not about the seam"
    );
}
