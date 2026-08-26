//! `bind_context`, `rebind_context` and `destroy` as a client we did not
//! write actually sees them.
//!
//! Our naming client and our naming server were written together and share
//! every assumption, so a green round trip between them is a statement about
//! our agreement with ourselves. This drives the same three operations from
//! omniORB's own Python client — clause (a) of the licensing boundary, a
//! separate process reached over TCP; nothing of omniORB's is linked,
//! vendored or redistributed.
//!
//! # What each row is for
//!
//! Every expected answer below was **measured against omniNames 4.3.4 first**,
//! with the same client, before it was implemented here. Three rows are
//! deliberate divergences from that oracle and are marked where they appear:
//!
//! - `bind_context` with a reference this server does not serve is
//!   `NO_IMPLEMENT`. omniNames stores whatever it is handed, without a type
//!   check, and chains to it at resolve time. This is the surviving half of
//!   the original deferral, and this test is the only place it is visible to
//!   a peer.
//! - `rebind_context` over an *object* binding is `NotFound { not_context }`.
//!   omniNames silently replaces it. `rebind` already made the mirror-image
//!   choice here before `rebind_context` existed.
//! - the elapsed-time row: the refusal must come back promptly. It is the
//!   measurement that says we did not dial. A chaining implementation pointed
//!   at TEST-NET would answer after a TCP connect timeout — measured against
//!   omniNames as a `TRANSIENT` raised tens of seconds later — so the clock
//!   tells the two designs apart in a way no reply body does.
//!
//! # When the fixture is absent
//!
//! omniORBpy is a fixture (`brew install omniorb`), not a dependency, and a
//! machine without it cannot run this. The test then prints a `SKIPPED` line
//! naming what went unmeasured and passes, exactly as the harness's own naming
//! group does — and the harness snippet in this batch's commit message greps
//! for the `measured` line so that absence stays visible where it counts. A
//! silent pass is the failure this file exists to avoid, so the marker is
//! printed either way.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use orbweaver_giop::naming_server::NamingServer;
use orbweaver_giop::orb::Orb;

/// The driver. It prints one `label\tanswer` row per probe and nothing else on
/// stdout, so a mismatch names the operation rather than the diff of a
/// paragraph.
///
/// `_narrow` is the first thing it does, because every ORB probes with `_is_a`
/// before trusting one and a servant that answers it wrongly fails here rather
/// than three operations later with a confusing message.
const DRIVER: &str = r#"
import sys, time
from omniORB import CORBA
import CosNaming
from CosNaming import NameComponent as NC

orb = CORBA.ORB_init([])
root = orb.string_to_object(open(sys.argv[1]).read().strip())._narrow(CosNaming.NamingContextExt)
if root is None:
    print("narrow\tFAILED")
    sys.exit(0)
print("narrow\tOK")

def row(label, fn):
    try:
        fn()
        print("%s\tOK" % label)
    except CosNaming.NamingContext.NotFound as e:
        print("%s\tNotFound:%s" % (label, e.why))
    except CosNaming.NamingContext.AlreadyBound:
        print("%s\tAlreadyBound" % label)
    except CosNaming.NamingContext.NotEmpty:
        print("%s\tNotEmpty" % label)
    except CosNaming.NamingContext.InvalidName:
        print("%s\tInvalidName" % label)
    except CORBA.SystemException as e:
        print("%s\t%s" % (label, e.__class__.__name__))
    except Exception as e:
        print("%s\tUNEXPECTED:%s" % (label, type(e).__name__))

# ── the local idiom the old deferral refused ──
sub = root.new_context()
row("bind_context", lambda: root.bind_context([NC("sub", "")], sub))
row("bind_through_it", lambda: root.bind([NC("sub", ""), NC("leaf", "")], root))
row("resolve_through_it", lambda: root.resolve([NC("sub", ""), NC("leaf", "")]))
row("bind_context_again", lambda: root.bind_context([NC("sub", "")], root.new_context()))
kinds = dict((b.binding_name[0].id, str(b.binding_type)) for b in root.list(50)[0])
print("lists_as\t%s" % kinds.get("sub", "MISSING"))

# ── rebind_context, and the divergence it carries ──
row("rebind_context", lambda: root.rebind_context([NC("sub", "")], root.new_context()))
row("old_contents_gone", lambda: root.resolve([NC("sub", ""), NC("leaf", "")]))
root.bind([NC("anobject", "")], root)
row("rebind_context_over_object", lambda: root.rebind_context([NC("anobject", "")], root.new_context()))

# ── the surviving deferral, and the clock that says we did not dial ──
far = orb.string_to_object("corbaloc:iiop:1.2@192.0.2.1:4000/Echo")
started = time.time()
row("bind_context_foreign", lambda: root.bind_context([NC("far", "")], far))
print("foreign_took_under_5s\t%s" % ("OK" if time.time() - started < 5.0 else "SLOW"))

# ── destroy, and what a peer sees afterwards ──
dead = root.bind_new_context([NC("dead", "")])
row("destroy_empty", lambda: dead.destroy())
row("destroy_twice", lambda: dead.destroy())
row("call_the_destroyed_reference", lambda: dead.list(10))
row("resolve_the_name_still_works", lambda: root.resolve([NC("dead", "")]))
row("resolve_through_the_dangling_binding", lambda: root.resolve([NC("dead", ""), NC("z", "")]))
kinds = dict((b.binding_name[0].id, str(b.binding_type)) for b in root.list(50)[0])
print("dangling_lists_as\t%s" % kinds.get("dead", "MISSING"))
row("unbind_the_dangling_name", lambda: root.unbind([NC("dead", "")]))

full = root.bind_new_context([NC("full", "")])
full.bind([NC("thing", "")], root)
row("destroy_non_empty", lambda: full.destroy())
row("destroy_after_emptying", lambda: (full.unbind([NC("thing", "")]), full.destroy()))
"#;

/// What omniORB's client must report, in order. Each row was measured against
/// omniNames before it was implemented here; the three marked `divergence` are
/// where this servant deliberately answers differently, and the module docs
/// argue each.
const EXPECTED: &[(&str, &str)] = &[
    ("narrow", "OK"),
    ("bind_context", "OK"),
    ("bind_through_it", "OK"),
    ("resolve_through_it", "OK"),
    ("bind_context_again", "AlreadyBound"),
    ("lists_as", "ncontext"),
    ("rebind_context", "OK"),
    ("old_contents_gone", "NotFound:missing_node"),
    // divergence: omniNames replaces the object binding without complaint.
    ("rebind_context_over_object", "NotFound:not_context"),
    // divergence: omniNames accepts any reference and chains to it later.
    ("bind_context_foreign", "NO_IMPLEMENT"),
    ("foreign_took_under_5s", "OK"),
    ("destroy_empty", "OK"),
    ("destroy_twice", "OBJECT_NOT_EXIST"),
    ("call_the_destroyed_reference", "OBJECT_NOT_EXIST"),
    ("resolve_the_name_still_works", "OK"),
    ("resolve_through_the_dangling_binding", "OBJECT_NOT_EXIST"),
    ("dangling_lists_as", "ncontext"),
    ("unbind_the_dangling_name", "OK"),
    ("destroy_non_empty", "NotEmpty"),
    ("destroy_after_emptying", "OK"),
];

/// Whether omniORB's Python client is installed. Absence is reported, never
/// treated as a pass on the property.
fn omniorbpy_present() -> bool {
    Command::new("python3")
        .args(["-c", "import omniORB, CosNaming"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// A scratch directory of our own. No dependency for this: one directory, one
/// name, removed at the end.
fn scratch() -> PathBuf {
    let unique =
        SystemTime::now().duration_since(UNIX_EPOCH).expect("a clock after 1970").as_nanos();
    let dir =
        std::env::temp_dir().join(format!("orbweaver-naming-peer-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

#[test]
fn omniorbs_client_drives_bind_context_and_destroy_against_our_server() {
    if !omniorbpy_present() {
        // The harness greps for the other marker; this one says why it is
        // missing rather than leaving a green line to be misread.
        println!(
            "naming-peer: SKIPPED — omniORBpy is not installed, so bind_context/rebind_context/\
             destroy are unmeasured against an independent client, not passing"
        );
        let _ = std::io::stdout().flush();
        return;
    }

    let dir = scratch();
    let server = Orb::new().server("127.0.0.1:0", b"NameService".to_vec()).expect("bind");
    let port = server.local_addr().expect("local_addr").port();
    let ns = Arc::new(NamingServer::new("127.0.0.1", port, b"NameService".to_vec()));
    let ior = ns.root_ior().to_stringified().expect("a stringified IOR");

    let ior_path = dir.join("names.ior");
    let driver_path = dir.join("driver.py");
    std::fs::write(&ior_path, &ior).expect("write the IOR");
    std::fs::write(&driver_path, DRIVER).expect("write the driver");

    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::spawn(move || {
        server.serve_shared(&*ns, move || flag.load(Ordering::SeqCst)).expect("serve")
    });

    let run = Command::new("python3")
        .arg(&driver_path)
        .arg(&ior_path)
        .output()
        .expect("python3 runs, since omniORB imported a moment ago");
    stop.store(true, Ordering::SeqCst);
    thread.join().expect("the serve thread");

    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        run.status.success(),
        "the peer's client did not finish.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let got: Vec<(&str, &str)> = stdout
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(a, b)| (a.trim(), b.trim()))
        .collect();

    // Compared as a whole rather than row by row: a missing row would
    // otherwise shift every later comparison and report a dozen failures with
    // one cause, which is the diagnosis this project batches to avoid.
    assert_eq!(
        got,
        EXPECTED.to_vec(),
        "omniORB's client saw something other than what omniNames was measured to give.\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );

    println!("naming-peer: measured {} rows against omniORB's own client", got.len());
    let _ = std::io::stdout().flush();
}
