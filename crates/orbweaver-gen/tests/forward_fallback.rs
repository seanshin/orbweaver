//! Fallback-on-failure: what our client does after the server it was
//! forwarded to dies — under `LOCATION_FORWARD` and under
//! `LOCATION_FORWARD_PERM`.
//!
//! 680aa41 measured that a request *count* at the old address cannot tell the
//! two statuses apart: every client measured — ours and omniORB's — sends one
//! request to the old address and then talks to the new one, whichever status
//! it was told. The oracle that can tell them apart is what the client does
//! when the forwarded-to address *stops answering*. CORBA Part 2 §9.6 (Object
//! Location, formal/2012-11-14; the section is the same text in 3.4):
//!
//! > A client shall not make any assumptions about the longevity of object
//! > addresses returned by LOCATION_FORWARD (OBJECT_FORWARD) mechanisms. Once
//! > a connection based on location-forwarding information is closed, a
//! > client can attempt to reuse the forwarding information it has, but, if
//! > that fails, it shall restart the location process using the original
//! > address specified in the initial object reference.
//!
//! and, of the permanent form:
//!
//! > For GIOP version 1.2 and later, the usage of LOCATION_FORWARD_PERM
//! > (OBJECT_FORWARD_PERM) behaves like the usage of LOCATION_FORWARD
//! > (OBJECT_FORWARD), but when used by the server it also provides an
//! > indication to the client that it may replace the old IOR with the new
//! > IOR. When using LOCATION_FORWARD_PERM (OBJECT_FORWARD_PERM), both the
//! > old IOR and the new IOR are valid, but the new IOR is preferred for
//! > future use.
//!
//! So: after a *temporary* forward the client *shall* go back to the original
//! address when the forwarded-to one fails; after a *permanent* one it *may*
//! have replaced the original and so may not. A client that goes back under
//! temporary and not under permanent has distinguished them; the spec permits
//! a client to go back under both.
//!
//! This file measures our two clients in that shape — two live servers at
//! two addresses, the first forwarding to the second, the second then
//! stopped — and pins what each does. Neither assertion below is the spec's
//! *shall*: they are what the code does today, written down so that a change
//! goes red and is made on purpose (see each test's doc comment for how the
//! measurement reads against §9.6). The peer half — omniORB 4.3.4's client in
//! the same shape, against two `spike-server` processes — is
//! `spikes/perm_fallback.sh`, which also runs this file and reads its `cell`
//! lines.
//!
//! What each test is for:
//!
//! * [`connection_does_not_fall_back_under_either_status`] — a bare
//!   [`Connection`] moves to the forwarded endpoint and *is* that socket: it
//!   holds no original address, so when the endpoint dies the next call is an
//!   error under temporary and under permanent alike, and the original sees
//!   nothing;
//! * [`reference_re_asks_the_original_under_either_status`] — a pooled
//!   [`Reference`] sends every call to the reference it holds and follows the
//!   forward each time, so it re-asks the original on every call while the
//!   target is alive and again after it dies — under both statuses. It never
//!   replaces its reference on a permanent forward (spec-permitted; the
//!   caller decides, with `Reference::forwarded`), which is why the pool
//!   cannot distinguish the two either.

mod emitted;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use orbweaver_cdr::Endian;
use orbweaver_gen::rt::{self, Forward, ObjRef, ObjectHome, Server};
use orbweaver_giop::pool::Pool;
use orbweaver_giop::{Connection, Ior, Version};

use emitted::f_26_object_identity::gc26::{
    DirectoryClient, DirectoryFault, DirectoryRefs, DirectoryServant, DirectorySkeleton,
    DirectoryTarget, NotBound,
};

const ROOT: &[u8] = b"dirsvc";
const TYPE_ID: &str = "IDL:gc26/Directory:1.0";

// ── The two servants ─────────────────────────────────────────────────────────

/// The forwarded-to server: one node, `new`, whose label says where it is.
struct Landing;

impl DirectoryServant for Landing {
    fn knows(&self, at: &DirectoryTarget<'_>) -> bool {
        at.oid() == "new"
    }
    fn label(&mut self, _at: &DirectoryTarget<'_>) -> Result<String, DirectoryFault> {
        Ok("relocated".into())
    }
    fn count(&mut self, _at: &DirectoryTarget<'_>) -> Result<i32, DirectoryFault> {
        Ok(0)
    }
    fn child(&mut self, _at: &DirectoryTarget<'_>, leaf: String) -> Result<ObjRef, DirectoryFault> {
        Err(DirectoryFault::NotBound(NotBound { missing: leaf }))
    }
    fn make_child(
        &mut self,
        _at: &DirectoryTarget<'_>,
        _leaf: String,
    ) -> Result<ObjRef, DirectoryFault> {
        Err(rt::raise::no_permission().did_not_run().into())
    }
    fn drop_binding(
        &mut self,
        _at: &DirectoryTarget<'_>,
        _l: String,
    ) -> Result<(), DirectoryFault> {
        Ok(())
    }
}

/// The original address: `old` is forwarded to `target` — temporarily or for
/// good — for as long as `forwarding` is set, and served here, labelled
/// `original`, once it is cleared. The harness clears it when it stops the
/// target, which is what lets a client that comes back be *answered* here
/// rather than forwarded into the dead address again; `asked` counts every
/// request that reached `old`, whichever way it was answered.
struct Relay {
    target: Ior,
    for_good: bool,
    forwarding: Arc<AtomicBool>,
    asked: Arc<AtomicU32>,
}

impl DirectoryServant for Relay {
    fn knows(&self, at: &DirectoryTarget<'_>) -> bool {
        at.oid() == "old"
    }
    /// Counted here because `redirect` is asked once per request that passed
    /// `knows`, before anything else — so this is the count of requests that
    /// reached the original address, whatever the client then did.
    fn redirect(&mut self, at: &DirectoryTarget<'_>) -> Option<Forward> {
        if at.oid() != "old" {
            return None;
        }
        self.asked.fetch_add(1, Ordering::SeqCst);
        if !self.forwarding.load(Ordering::SeqCst) {
            return None;
        }
        let to = self.target.clone();
        Some(if self.for_good { Forward::Permanent(to) } else { Forward::Temporary(to) })
    }
    fn label(&mut self, _at: &DirectoryTarget<'_>) -> Result<String, DirectoryFault> {
        Ok("original".into())
    }
    fn count(&mut self, _at: &DirectoryTarget<'_>) -> Result<i32, DirectoryFault> {
        Ok(0)
    }
    fn child(&mut self, _at: &DirectoryTarget<'_>, leaf: String) -> Result<ObjRef, DirectoryFault> {
        Err(DirectoryFault::NotBound(NotBound { missing: leaf }))
    }
    fn make_child(
        &mut self,
        _at: &DirectoryTarget<'_>,
        _leaf: String,
    ) -> Result<ObjRef, DirectoryFault> {
        Err(rt::raise::no_permission().did_not_run().into())
    }
    fn drop_binding(
        &mut self,
        _at: &DirectoryTarget<'_>,
        _l: String,
    ) -> Result<(), DirectoryFault> {
        Ok(())
    }
}

// ── Harness ──────────────────────────────────────────────────────────────────

/// A server on its own thread, stoppable — and *stopped* means the port is
/// closed: `Server::serve` returns only once every connection thread has
/// ended, and the `Server` (its listener with it) is dropped with the thread.
struct Live {
    ior: Ior,
    refs: DirectoryRefs,
    addr: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Live {
    fn start<S: DirectoryServant + Send + 'static>(servant: S) -> Self {
        let server = Server::bind("127.0.0.1:0", ROOT.to_vec()).expect("bind");
        let addr = server.local_addr().expect("addr");
        let ior = server.ior(TYPE_ID, "127.0.0.1").expect("ior");
        let home = ObjectHome::of(&server, "127.0.0.1").expect("home");
        let refs = DirectoryRefs::new(home.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut skeleton = DirectorySkeleton::new(DirectoryRefs::new(home), servant);
            server.serve(&mut skeleton, || flag.load(Ordering::SeqCst)).expect("serve");
        });
        Self { ior, refs, addr, stop, thread: Some(thread) }
    }

    /// The reference to `oid` at this server.
    fn reference(&self, oid: &str) -> Ior {
        let mut ior = self.ior.clone();
        ior.profiles[0].object_key = self.refs.key_of(oid);
        ior
    }

    /// Stops the server and waits until it — and its port — are gone.
    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(self.addr); // wake the accept loop
        if let Some(t) = self.thread.take() {
            t.join().expect("the server thread must not panic");
        }
        // The listener went with the thread; a dial now is refused, which is
        // the "connection closed" of §9.6 in its most definite form.
        let refused = std::net::TcpStream::connect_timeout(&self.addr, Duration::from_secs(2));
        assert!(refused.is_err(), "the stopped server's port must not accept");
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.stop();
        }
    }
}

/// The two-server shape: `landing` serves `new`; `original` forwards `old`
/// there with the given status until told otherwise. Returns them, the
/// forwarding switch and the request counter at `old`.
fn two_servers(for_good: bool) -> (Live, Live, Arc<AtomicBool>, Arc<AtomicU32>) {
    let landing = Live::start(Landing);
    let forwarding = Arc::new(AtomicBool::new(true));
    let asked = Arc::new(AtomicU32::new(0));
    let original = Live::start(Relay {
        target: landing.reference("new"),
        for_good,
        forwarding: forwarding.clone(),
        asked: asked.clone(),
    });
    (original, landing, forwarding, asked)
}

fn status(for_good: bool) -> &'static str {
    if for_good { "permanent" } else { "temporary" }
}

/// One line per cell, in the shape `spikes/perm_fallback.sh` reads.
fn cell(client: &str, for_good: bool, endian: Endian, reasked: bool, after: u32, second: &str) {
    println!(
        "cell client={client} status={} endian={endian:?} giop=1.2 reasked={} \
         requests_at_original_after_death={after} second_call={second}",
        status(for_good),
        if reasked { "yes" } else { "no" },
    );
}

// ── Connection ───────────────────────────────────────────────────────────────

/// A `Connection` that followed a forward *is* the socket to the forwarded-to
/// endpoint (`Connection::invoke` replaces itself with the new connection),
/// and it holds no original address to go back to. When that endpoint dies,
/// the next call is an error under both statuses, and the original address
/// sees no second request. Against §9.6: the "shall restart the location
/// process using the original address" is not something a bare `Connection`
/// does — the caller that holds the initial reference redials it. That is
/// what this pins; whether `Connection` should keep the initial IOR and
/// redial it itself after a *temporary* forward's target dies is a change to
/// `lib.rs`, reported with this measurement and not made here.
#[test]
fn connection_does_not_fall_back_under_either_status() {
    for for_good in [false, true] {
        for endian in [Endian::Big, Endian::Little] {
            let what = format!("{} {endian:?}", status(for_good));
            let (mut original, mut landing, forwarding, asked) = two_servers(for_good);
            let old = original.reference("old");

            let mut conn = Connection::connect(&old, Duration::from_secs(5)).expect("connect");
            conn.cap_version(Version::V1_2);
            conn.set_endian(endian);
            let mut client = DirectoryClient::new(conn);

            // Followed, answered by the new object, and the status reported.
            assert_eq!(client.label().expect("followed"), "relocated", "{what}");
            let followed = client.conn.forwarded().expect("a forward was followed");
            assert_eq!(followed.is_permanent(), for_good, "{what}");
            assert_eq!(asked.load(Ordering::SeqCst), 1, "{what}: one request at the original");

            // The forwarded-to server dies; the original would now answer.
            landing.stop();
            forwarding.store(false, Ordering::SeqCst);

            let second = client.label();
            let after = asked.load(Ordering::SeqCst) - 1;
            let second_text = match &second {
                Ok(label) => format!("Ok({label})"),
                Err(e) => format!("Err({e})"),
            };
            cell("Connection", for_good, endian, after > 0, after, &second_text);
            assert!(second.is_err(), "{what}: the socket it holds is to the dead endpoint");
            assert_eq!(after, 0, "{what}: the original was not re-asked");
            // And it stays down: a third call is no different, because a
            // Connection with a broken stream has nowhere else to go.
            assert!(client.label().is_err(), "{what}");
            assert_eq!(asked.load(Ordering::SeqCst), 1, "{what}");
            original.stop();
        }
    }
}

// ── Reference (the pool) ─────────────────────────────────────────────────────

/// A pooled `Reference` sends every call to the reference it holds and lets
/// the pool follow the forward each time — so the original is asked on every
/// call, alive or dead, under both statuses: the count at the original is
/// the number of calls made. After the target dies the next call reaches the
/// original and is answered there. Against §9.6: the temporary "shall
/// restart at the original address" holds, in the strong form of never having
/// left it; the permanent "may replace the old IOR" is not taken up (the
/// reference is not replaced — `Reference::forwarded` reports the leave and
/// the caller decides). So this client cannot distinguish the two statuses
/// either, and pays a round trip to the original on every call while the
/// forward stands.
#[test]
fn reference_re_asks_the_original_under_either_status() {
    for for_good in [false, true] {
        for endian in [Endian::Big, Endian::Little] {
            let what = format!("{} {endian:?}", status(for_good));
            let (mut original, mut landing, forwarding, asked) = two_servers(for_good);
            let old = original.reference("old");

            let pool = Pool::new();
            let mut r = pool.reference(old.clone());
            r.set_endian(endian);
            let mut client = DirectoryClient::new(r);

            assert_eq!(client.label().expect("followed"), "relocated", "{what}");
            let followed = client.conn.forwarded().expect("a forward was followed");
            assert_eq!(followed.is_permanent(), for_good, "{what}");
            assert_eq!(asked.load(Ordering::SeqCst), 1, "{what}");

            // Target still alive: the second call goes to the original again
            // and is forwarded again — the pool cached nothing.
            assert_eq!(client.label().expect("followed again"), "relocated", "{what}");
            assert_eq!(asked.load(Ordering::SeqCst), 2, "{what}: re-asked while alive");
            assert!(client.conn.forwarded().is_some(), "{what}");

            landing.stop();
            forwarding.store(false, Ordering::SeqCst);

            let third = client.label();
            let after = asked.load(Ordering::SeqCst) - 2;
            let third_text = match &third {
                Ok(label) => format!("Ok({label})"),
                Err(e) => format!("Err({e})"),
            };
            cell("Reference", for_good, endian, after > 0, after, &third_text);
            assert_eq!(third.expect("answered at the original"), "original", "{what}");
            assert_eq!(after, 1, "{what}: one request at the original after the death");
            assert!(client.conn.forwarded().is_none(), "{what}: answered where it was sent");
            original.stop();
        }
    }
}
