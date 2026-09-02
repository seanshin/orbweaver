//! A reference *arriving* at a **Java** servant, and the servant invoking it.
//!
//! # What this adds that its Python twin does not
//!
//! `a_call_travelling_the_other_way.rs` measures the same property through a
//! Python servant and is the file to read first: the argument for the two
//! assertions, and why neither is sufficient alone, is made there and is not
//! restated here.
//!
//! What this file is for is that **the protocol has three implementations and
//! a property held by one of them is not held by the seam.** Python's runtime
//! and Rust's carried the invoke direction from 2026-08-31; Java's carried none
//! of it until 2026-09-02, and nothing was red — `the_seam_is_one_protocol.rs`
//! had a single row. So this is not a copy of that test for a second language,
//! it is the second measurement the first one's greenness never implied.
//!
//! # What is measured here
//!
//! A Java servant implements `gc16::Registry`. It is handed a reference to a
//! `Target` — a **Rust** servant on a **real socket**, bound by this test — as
//! the `ref` argument of `bind`, and it calls `ping()` on it.
//!
//! Two assertions, and neither is sufficient alone:
//!
//! * the Rust `Target` records that `ping` was invoked **once**. Without this,
//!   a Java servant that returned successfully having done nothing would pass.
//! * the Java servant refuses unless the value it read back is [`PONG`]. So the
//!   answer travelled the other way and arrived intact, not merely that a
//!   nested request was written.
//!
//! # The two controls
//!
//! * the same servant with the nested call removed must leave the target
//!   uncalled — otherwise the assertions above are green in a world where
//!   nothing crossed;
//! * a servant that keeps the reference and invokes it **after** the dispatch
//!   has ended must be refused. *A handle is not a proxy*, and a test that only
//!   proves the success passes in a world where that refusal was deleted.
//!
//! *도착한 참조를 **자바** 서번트가 호출한다. 파이썬 쌍둥이가 있는데도 이 파일이
//! 있는 이유는, 프로토콜에 구현이 셋이고 **한 구현이 가진 성질은 seam의 성질이
//! 아니기 때문**이다 — 자바는 2026-09-02까지 이 방향을 하나도 싣지 않았고 아무것도
//! 빨갛지 않았다. 대조군 둘: 중첩 호출을 뺀 같은 서번트는 대상에 닿지 않아야 하고,
//! 디스패치가 끝난 뒤 참조를 쓰면 거절되어야 한다.*

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_gen::pychild::SeamChild;
use orbweaver_gen::seam::ForeignServant;
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{Dispatch, Request, SystemException, decode_request};
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, Ior, Version, encode_request, read_message};
use orbweaver_registry::{Contract, Registry, Strictness};

const PACKAGE: &str = "gc16jotherway";
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
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/golden/16-object-refs.idl");
    let contract = Contract::load(&path, &Default::default(), Strictness::Checked)
        .expect("the corpus contract must load");
    let mut r = Registry::new();
    r.load(&contract.spec).expect("the contract must build a registry");
    r
}

/// `javac` and `java`, or `None` — the same discipline the other Java tests in
/// this crate use, and deliberately the same environment variable.
fn jdk() -> Option<(PathBuf, PathBuf)> {
    let home = std::env::var("ORBWEAVER_JAVA_HOME").unwrap_or_else(|_| {
        "/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home".into()
    });
    let javac = Path::new(&home).join("bin/javac");
    let java = Path::new(&home).join("bin/java");
    (javac.is_file() && java.is_file()).then_some((javac, java))
}

/// The Rust servant the Java one will be asked to call.
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

/// Which servant to build, and therefore which property is under test.
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    /// Invokes the reference during the dispatch: the leak leg.
    Nested,
    /// Receives the reference and never invokes it: the control that shows the
    /// assertions can fail.
    Inert,
    /// Keeps the reference and invokes it after the dispatch has returned: the
    /// control for *a handle is not a proxy*.
    AfterTheCall,
}

/// The Java servant, and the one line this batch is about: `ref.invoke`.
fn servant_source(shape: Shape) -> String {
    // The nested result is AnyJSON, not a mapped value — a boundary stated in
    // `ObjectRef.invoke`'s javadoc — so the servant reads a `Num` rather than an
    // `int`. Java says that in its type where Python says it in prose.
    let body = match shape {
        Shape::Nested => format!(
            "        Object got = ref.invoke(\"ping\");\n\
             \x20       long v = got instanceof _Rt.Num ? ((_Rt.Num) got).asLong() : -1L;\n\
             \x20       if (v != {PONG}L) {{\n\
             \x20           throw _Rt.Raise.ranToCompletion(\"IDL:omg.org/CORBA/UNKNOWN:1.0\", 1);\n\
             \x20       }}\n"
        ),
        Shape::Inert => "        // receives the reference and never uses it\n".to_owned(),
        Shape::AfterTheCall => "        kept = ref;\n".to_owned(),
    };
    // The after-the-call control needs the dispatch to END before the reference
    // is used, so it invokes from `main` once `serveOnPipes` has returned. If
    // that were allowed, the servant would be writing into a conversation that
    // is over.
    let after = if shape == Shape::AfterTheCall {
        "        if (kept != null) {\n\
         \x20           try {\n\
         \x20               kept.invoke(\"ping\");\n\
         \x20               System.err.println(\"ORBWEAVER-CONTROL-NOT-REFUSED\");\n\
         \x20           } catch (_Rt.ServantError e) {\n\
         \x20               System.err.println(\"ORBWEAVER-CONTROL-REFUSED\");\n\
         \x20           }\n\
         \x20       }\n"
    } else {
        ""
    };
    format!(
        r#"
import {PACKAGE}._Rt;
import {PACKAGE}.gc16.RegistryServant;

public final class Reg extends RegistryServant {{
    static _Rt.ObjectRef kept = null;

    @Override
    public void bind(String name, _Rt.ObjectRef ref) {{
{body}    }}

    public static void main(String[] argv) throws Exception {{
        _Rt.serveOnPipes(new Reg());
{after}    }}
}}
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
fn bind_through(servant: &mut ForeignServant<SeamChild>, target: &Ior) -> Result<(), String> {
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

/// Emits the package, writes the servant, and compiles both.
fn build(javac: &Path, dir: &Path, shape: Shape) -> PathBuf {
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
    let servant = src.join("Reg.java");
    std::fs::write(&servant, servant_source(shape)).expect("write the servant");
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

fn run(shape: Shape) -> Option<(Result<(), String>, u32)> {
    let (javac, java) = jdk()?;
    let dir = std::env::temp_dir().join(format!(
        "orbweaver-jotherway-{}-{}",
        std::process::id(),
        shape as u8
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let classes = build(&javac, &dir, shape);

    let pings = Arc::new(AtomicU32::new(0));
    let counted = pings.clone();
    let mut result = Err("never ran".to_owned());
    with_target(counted, |ior| {
        let child = SeamChild::java(&java, &classes, "Reg").expect("the JDK starts");
        let mut servant =
            ForeignServant::new(&registry(), REGISTRY_ID, child).expect("Registry resolves");
        result = bind_through(&mut servant, ior);
    });
    let _ = std::fs::remove_dir_all(&dir);
    let n = pings.load(Ordering::SeqCst);
    Some((result, n))
}

const NO_JDK: &str = "SKIPPED  no JDK — set ORBWEAVER_JAVA_HOME. Whether a Java servant can \
                      invoke a reference it was handed is UNMEASURED here, not passing.";

/// The leak leg: the far side uses a reference it was handed.
#[test]
fn a_java_servant_invokes_a_reference_it_was_handed() {
    let Some((result, pings)) = run(Shape::Nested) else {
        eprintln!("{NO_JDK}");
        return;
    };
    assert!(
        result.is_ok(),
        "the Java servant refused `bind`, which it only does when the value it read back \
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
#[test]
fn without_the_nested_call_the_java_target_is_never_reached() {
    let Some((result, pings)) = run(Shape::Inert) else {
        eprintln!("{NO_JDK}");
        return;
    };
    assert!(result.is_ok(), "the control servant should answer `bind` without calling: {result:?}");
    assert_eq!(
        pings, 0,
        "the control invoked the target {pings} time(s). It makes no nested call, so a \
         non-zero count means something other than the servant is dialling and the \
         measurement above is not about the seam"
    );
}

/// The second control: a handle is not a proxy.
///
/// The servant keeps the reference and invokes it once `serveOnPipes` has
/// returned — the dispatch is over and the channel is gone. It must be refused.
/// A test that only proves the success would pass in a world where that refusal
/// had been deleted, which is why this one asserts the target stays uncalled.
#[test]
fn a_reference_kept_past_the_dispatch_is_refused() {
    let Some((result, pings)) = run(Shape::AfterTheCall) else {
        eprintln!("{NO_JDK}");
        return;
    };
    assert!(result.is_ok(), "the servant should answer `bind` and keep the reference: {result:?}");
    assert_eq!(
        pings, 0,
        "the reference was invoked {pings} time(s) after its dispatch ended. The channel is \
         installed for one dispatch and cleared afterwards, so a non-zero count here means a \
         servant can write into a conversation that is over"
    );
}
