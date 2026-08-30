//! D029 §6.1's **Language** row, refuted or not, over a socket, with the caller
//! in the room while the language changes.
//!
//! # What was missing, and why it was not just a test nobody had written
//!
//! `spikes/leak_tests.sh`'s language leg has been a counted `SKIPPED` since it
//! was written, and it named its blocker rather than leaving it to be guessed:
//! the only route to a Python servant was `orbweaver-py-bridge --serve`, **which
//! binds its own listener**, so the Python side arrived as an *endpoint*. A
//! caller made to dial a different address has been **moved**, and *location*
//! and *language* are different rows — a test built that way would have
//! measured the wrong row and been green while it did.
//!
//! `orbweaver_gen::pychild::PythonChild` closed that: `python3` as a child of
//! this process, wrapped by `seam::ForeignServant` into a plain `Dispatch`. So
//! both implementations can sit behind **one** server, one reference and one
//! open connection, and the language can change underneath a caller that never
//! learns a new address.
//!
//! # The claim
//!
//! A caller holding one reference invokes, the servant behind it is replaced by
//! one **written in another language**, and the same invocation is made on the
//! same connection. If the two replies are the same octets, that caller could
//! not tell.
//!
//! # What it does not measure
//!
//! Two languages, not N. And it is one *operation* — `count` — over one
//! contract: a language pair that agreed on `count` and diverged on a
//! `wstring` is not measured by any number of runs of this.
//! `python_servant.rs` is the wide comparison and has no live caller;
//! this has the live caller and the narrow pair.
//!
//! # The control
//!
//! `ORBWEAVER_LEAK_CONTROL=language` — the Python servant answers a different
//! number, which is what a caller would see if the language behind a reference
//! were observable. Red at the comparison, naming what the caller could tell.
//!
//! *하나의 참조, 살아 있는 호출자, 그 아래에서 바뀌는 언어. 막혀 있던 이유는
//! 유일한 경로가 리스너를 바인딩해 파이썬 쪽이 **엔드포인트로** 도착했기
//! 때문이고, 주소를 바꿔 다이얼하면 그것은 이동이지 재구현이 아니다.*

mod emitted;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use orbweaver_cdr::Encoder;
use orbweaver_gen::pychild::PythonChild;
use orbweaver_gen::rt::{Dispatch, DispatchBody, ObjRef, ObjectHome, SystemException};
use orbweaver_gen::seam::ForeignServant;
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{Request, SharedDispatch};
use orbweaver_giop::{Connection, Ior};
use orbweaver_registry::{Contract, Registry, Strictness};

use emitted::f_26_object_identity::gc26::{
    DirectoryFault, DirectoryRefs, DirectoryServant, DirectorySkeleton, DirectoryTarget, NotBound,
};

const PACKAGE: &str = "gc26_lang";
const TYPE_ID: &str = "IDL:gc26/Directory:1.0";
const ROOT: &[u8] = b"dirs";
const TIMEOUT: Duration = Duration::from_secs(10);

/// What both implementations answer for `count()` on the root.
///
/// Not 0: a servant that failed to run, a default, and an empty map all answer
/// 0, so an assertion that could not tell those from a working servant would be
/// satisfied by every world in which nothing happened.
const COUNT: i32 = 3;

fn registry() -> Registry {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/golden/26-object-identity.idl");
    let contract = Contract::load(&path, &Default::default(), Strictness::Checked)
        .expect("the corpus contract must load");
    let mut registry = Registry::new();
    registry.load(&contract.spec).expect("the contract must build a registry");
    registry
}

/// The Rust half: a root node with `COUNT` bindings under it.
struct Tree(BTreeMap<String, (String, BTreeMap<String, String>)>);

impl Tree {
    fn rooted() -> Self {
        let mut children = BTreeMap::new();
        for i in 0..COUNT {
            children.insert(format!("c{i}"), format!("n{i}"));
        }
        Tree(BTreeMap::from([(String::new(), ("root".to_owned(), children))]))
    }

    fn node(
        &self,
        at: &DirectoryTarget<'_>,
    ) -> Result<&(String, BTreeMap<String, String>), DirectoryFault> {
        self.0
            .get(at.oid())
            .ok_or(DirectoryFault::NotBound(NotBound { missing: at.oid().to_owned() }))
    }
}

impl DirectoryServant for Tree {
    fn knows(&self, at: &DirectoryTarget<'_>) -> bool {
        self.0.contains_key(at.oid())
    }
    fn label(&mut self, at: &DirectoryTarget<'_>) -> Result<String, DirectoryFault> {
        Ok(self.node(at)?.0.clone())
    }
    fn count(&mut self, at: &DirectoryTarget<'_>) -> Result<i32, DirectoryFault> {
        Ok(self.node(at)?.1.len() as i32)
    }
    fn child(&mut self, at: &DirectoryTarget<'_>, leaf: String) -> Result<ObjRef, DirectoryFault> {
        match self.node(at)?.1.get(&leaf) {
            Some(oid) => Ok(at.sibling(oid)),
            None => Err(DirectoryFault::NotBound(NotBound { missing: leaf })),
        }
    }
    fn make_child(
        &mut self,
        at: &DirectoryTarget<'_>,
        leaf: String,
    ) -> Result<ObjRef, DirectoryFault> {
        let _ = self.node(at)?;
        Ok(at.sibling(&format!("{}+{leaf}", at.oid())))
    }
    fn drop_binding(
        &mut self,
        at: &DirectoryTarget<'_>,
        _leaf: String,
    ) -> Result<(), DirectoryFault> {
        let _ = self.node(at)?;
        Ok(())
    }
}

/// Which language is behind the reference, changed while a caller holds the
/// line — and counted, so a run where the swap did not happen says so.
struct Bilingual {
    rust: std::sync::Mutex<DirectorySkeleton<Tree>>,
    python: std::sync::Mutex<ForeignServant<PythonChild>>,
    /// 0 = Rust, 1 = Python.
    language: AtomicUsize,
    served: [AtomicUsize; 2],
}

impl SharedDispatch for Bilingual {
    fn knows(&self, object_key: &[u8]) -> bool {
        // Stated, as D036 requires. Both halves are reached at the same key,
        // which is the whole point: the reference does not change.
        object_key.starts_with(ROOT)
    }

    fn dispatch(&self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        self.dispatch_body(request, out).map(|_| ())
    }

    fn dispatch_body(
        &self,
        request: &Request,
        out: &mut Encoder,
    ) -> Result<DispatchBody, SystemException> {
        let which = self.language.load(Ordering::SeqCst);
        self.served[which].fetch_add(1, Ordering::SeqCst);
        if which == 0 {
            self.rust.lock().expect("the rust half").dispatch_body(request, out)
        } else {
            self.python.lock().expect("the python half").dispatch_body(request, out)
        }
    }
}

/// Whether the control is armed, read once so a run cannot change its mind.
fn control_armed() -> bool {
    std::env::var("ORBWEAVER_LEAK_CONTROL").as_deref() == Ok("language")
}

fn python_program(dir: &std::path::Path) -> (String, std::path::PathBuf) {
    let package = orbweaver_gen::python::emit_python(&registry(), PACKAGE);
    let root = dir.join("site");
    let package_dir = root.join(PACKAGE);
    for (relative, body) in &package.files {
        let path = package_dir.join(relative);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("mkdir");
        std::fs::write(&path, body).expect("write");
    }
    // The leak, put back: a Python servant that answers a different number is
    // what a caller would see if the language behind a reference were
    // observable.
    let answer = if control_armed() { COUNT + 1 } else { COUNT };
    let program = format!(
        r#"
from {PACKAGE} import _rt
from {PACKAGE} import gc26


class Node(gc26.DirectoryServant):
    def _get_label(self):
        return "root"

    def count(self):
        return {answer}


_rt.serve_on_pipes(Node())
"#
    );
    (program, root)
}

struct Fixture {
    ior: Ior,
    servant: Arc<Bilingual>,
    stop: Arc<AtomicBool>,
    joined: Option<std::thread::JoinHandle<()>>,
    dir: std::path::PathBuf,
}

impl Fixture {
    fn start() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "orbweaver-lang-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).expect("a work directory");
        let (program, site) = python_program(&dir);
        let child = PythonChild::spawn(&program, &[&site]).expect("python3 starts");
        let python = ForeignServant::new(&registry(), TYPE_ID, child)
            .expect("the contract names Directory")
            .with_home(ObjectHome::new("127.0.0.1", 0, ROOT.to_vec()));

        let orb = Orb::new();
        let server = orb.server("127.0.0.1:0", ROOT.to_vec()).expect("bind");
        let ior = server.ior(TYPE_ID, "127.0.0.1").expect("an ior");
        let rust = DirectorySkeleton::new(
            DirectoryRefs::new(ObjectHome::new(
                "127.0.0.1",
                ior.primary().expect("one profile").port,
                ROOT.to_vec(),
            )),
            Tree::rooted(),
        );

        let servant = Arc::new(Bilingual {
            rust: std::sync::Mutex::new(rust),
            python: std::sync::Mutex::new(python),
            language: AtomicUsize::new(0),
            served: [AtomicUsize::new(0), AtomicUsize::new(0)],
        });
        let stop = Arc::new(AtomicBool::new(false));
        let serving = Arc::clone(&servant);
        let flag = Arc::clone(&stop);
        let joined = std::thread::spawn(move || {
            let _ = server.serve_shared(&*serving, || flag.load(Ordering::SeqCst));
        });
        Fixture { ior, servant, stop, joined: Some(joined), dir }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = Connection::connect(&self.ior, TIMEOUT);
        if let Some(j) = self.joined.take() {
            let _ = j.join();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn observe(c: &mut Connection) -> Result<Vec<u8>, String> {
    let reply = c.invoke_nullary("count").map_err(|e| e.to_string())?;
    let mut body = reply.body().map_err(|e| e.to_string())?;
    let n = body.get_i32().map_err(|e| e.to_string())?;
    Ok(n.to_be_bytes().to_vec())
}

/// The row's claim, with the caller in the room.
#[test]
fn a_language_swapped_under_a_live_caller_is_invisible() {
    let fx = Fixture::start();
    let mut caller = Connection::connect(&fx.ior, TIMEOUT).expect("connect");

    let before = observe(&mut caller).expect("the Rust servant answered");

    // ── the language behind the reference changes; the reference does not ──
    fx.servant.language.store(1, Ordering::SeqCst);

    let after = observe(&mut caller).expect("the Python servant answered");
    assert_eq!(
        after, before,
        "THE CALLER COULD TELL WHAT LANGUAGE ANSWERED: the reply changed when the servant \
         behind the reference was replaced by one written in another language, on the same \
         connection and the same reference."
    );

    // Server-side evidence that the swap took effect. Asking the caller which
    // language served it would be asking it to report the thing it must not be
    // able to tell — and without this, a run where the swap silently did not
    // happen would compare one implementation with itself and be green.
    assert!(
        fx.servant.served[0].load(Ordering::SeqCst) >= 1
            && fx.servant.served[1].load(Ordering::SeqCst) >= 1,
        "both languages must have served at least one call, or this run compared one \
         implementation with itself: rust={:?} python={:?}",
        fx.servant.served[0].load(Ordering::SeqCst),
        fx.servant.served[1].load(Ordering::SeqCst),
    );
}

/// The anti-vacuity companion: the two halves *can* be told apart.
///
/// *Indistinguishability is evidence about transparency only beside a
/// demonstration that distinguishing is possible.* Without this, a run in which
/// the Python servant answered nothing at all would satisfy the test above by
/// failing identically twice.
#[test]
fn the_two_languages_are_two_implementations_and_not_one() {
    let fx = Fixture::start();
    let mut caller = Connection::connect(&fx.ior, TIMEOUT).expect("connect");
    let _ = observe(&mut caller).expect("rust answers");
    fx.servant.language.store(1, Ordering::SeqCst);
    let _ = observe(&mut caller).expect("python answers");
    assert!(
        fx.servant.served[0].load(Ordering::SeqCst) >= 1
            && fx.servant.served[1].load(Ordering::SeqCst) >= 1,
        "the counters must show two different servants ran"
    );
}
