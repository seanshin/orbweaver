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
//! stopped — and asserts what each does. Both now distinguish the two
//! statuses: they restart at the original under temporary and stay under
//! permanent, which is what omniORB 4.3.4 was measured to do (the peer half —
//! omniORB's client in the same shape, against two `spike-server` processes —
//! is `spikes/perm_fallback.sh`, which also runs this file and reads its
//! `cell` lines and judges them the same way). Before this landed, neither
//! did: `Connection` held no original address and was `Err` under both;
//! `Reference` re-asked the original on every call under both.
//!
//! What each test is for:
//!
//! * [`connection_restarts_at_the_origin_after_a_temporary_forward_only`] —
//!   a bare [`Connection`] moves to the forwarded endpoint and keeps
//!   [`Connection::origin`]. When the endpoint dies, the next call — whose
//!   `CloseConnection` says it was not processed — is redialled at the
//!   origin and answered there under temporary; under permanent the origin
//!   *is* the forwarded-to address, so the call is an error and the old
//!   address sees nothing;
//! * [`reference_caches_a_forward_and_restarts_at_the_original_after_a_temporary_one`]
//!   — a pooled [`Reference`] caches a temporary forward's target and sends
//!   the next call straight there (one request at the original for two
//!   calls, not two); when the target dies it drops the cache and restarts
//!   at the original. A permanent forward re-points the reference, so the
//!   death is an error and the old address sees nothing.

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
/// endpoint and keeps the reference it was dialled from as its origin. When
/// the endpoint dies, the next call meets the `CloseConnection` the stopping
/// server sent — §13.5.1: not processed, safe to re-send — and under a
/// *temporary* forward `Connection::invoke` reuses the forwarding information
/// (a redial, refused: the port is closed) and then restarts at the origin,
/// where the request is answered; `forwarded()` is `None` again. Under a
/// *permanent* forward the origin was replaced by the forwarded-to IOR, so
/// there is nowhere to fall back to: the call is an error, the old address
/// sees no second request, and a third call is no different.
#[test]
fn connection_restarts_at_the_origin_after_a_temporary_forward_only() {
    for for_good in [false, true] {
        for endian in [Endian::Big, Endian::Little] {
            let what = format!("{} {endian:?}", status(for_good));
            let (mut original, mut landing, forwarding, asked) = two_servers(for_good);
            let old = original.reference("old");
            let new = landing.reference("new");

            let mut conn = Connection::connect(&old, Duration::from_secs(5)).expect("connect");
            conn.cap_version(Version::V1_2);
            conn.set_endian(endian);
            assert_eq!(conn.origin(), &old, "{what}: dialled from the old reference");
            let mut client = DirectoryClient::new(conn);

            // Followed, answered by the new object, and the status reported.
            assert_eq!(client.label().expect("followed"), "relocated", "{what}");
            let followed = client.conn.forwarded().expect("a forward was followed");
            assert_eq!(followed.is_permanent(), for_good, "{what}");
            assert_eq!(asked.load(Ordering::SeqCst), 1, "{what}: one request at the original");
            // A temporary forward leaves the origin alone; a permanent one
            // is the servant's leave to replace it, taken.
            assert_eq!(client.conn.origin(), if for_good { &new } else { &old }, "{what}");
            assert_eq!(client.conn.endian(), endian, "{what}: byte order survives the hop");

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
            if for_good {
                assert!(second.is_err(), "{what}: the origin is the dead address now");
                assert_eq!(after, 0, "{what}: the old address was not re-asked");
                assert!(client.conn.forwarded().is_some_and(Forward::is_permanent), "{what}");
                // And it stays down: nothing to restart from.
                assert!(client.label().is_err(), "{what}");
                assert_eq!(asked.load(Ordering::SeqCst), 1, "{what}");
            } else {
                assert_eq!(second.expect("restarted at the origin"), "original", "{what}");
                assert_eq!(after, 1, "{what}: one request at the original after the death");
                assert!(client.conn.forwarded().is_none(), "{what}: at the origin again");
                assert_eq!(client.conn.endian(), endian, "{what}: byte order survives the restart");
                // Now *at* the origin: the next call goes there directly.
                assert_eq!(client.label().expect("served at the origin"), "original", "{what}");
                assert_eq!(asked.load(Ordering::SeqCst), 3, "{what}");
            }
            original.stop();
        }
    }
}

// ── Reference (the pool) ─────────────────────────────────────────────────────

/// A pooled `Reference` caches a temporary forward's target: the second call
/// goes straight there and the original is not asked again (it was, before
/// this landed — two round trips per call for as long as the forward stood).
/// When the target dies, the call fails unsent — the pool's own retry finds
/// the port refusing — so the cache is dropped and the call restarts at the
/// original, which answers it: `forwarded()` is `None` again. A permanent
/// forward re-points the reference — `ior()` is the new address — so after
/// the death there is nowhere to go back to and the call is an error; the old
/// address sees nothing.
#[test]
fn reference_caches_a_forward_and_restarts_at_the_original_after_a_temporary_one() {
    for for_good in [false, true] {
        for endian in [Endian::Big, Endian::Little] {
            let what = format!("{} {endian:?}", status(for_good));
            let (mut original, mut landing, forwarding, asked) = two_servers(for_good);
            let old = original.reference("old");
            let new = landing.reference("new");

            let pool = Pool::new();
            let mut r = pool.reference(old.clone());
            r.set_endian(endian);
            let mut client = DirectoryClient::new(r);

            assert_eq!(client.label().expect("followed"), "relocated", "{what}");
            let followed = client.conn.forwarded().expect("a forward was followed");
            assert_eq!(followed.is_permanent(), for_good, "{what}");
            assert_eq!(asked.load(Ordering::SeqCst), 1, "{what}");
            assert_eq!(client.conn.ior(), if for_good { &new } else { &old }, "{what}");

            // Target still alive: the second call goes straight to it — the
            // forward is cached (temporary) or the reference re-pointed
            // (permanent) — and the original is not asked again.
            assert_eq!(client.label().expect("answered at the target"), "relocated", "{what}");
            assert_eq!(asked.load(Ordering::SeqCst), 1, "{what}: not re-asked while alive");
            assert_eq!(
                client.conn.forwarded().map(Forward::is_permanent),
                Some(for_good),
                "{what}: the redirect in force is still reported"
            );

            landing.stop();
            forwarding.store(false, Ordering::SeqCst);

            let third = client.label();
            let after = asked.load(Ordering::SeqCst) - 1;
            let third_text = match &third {
                Ok(label) => format!("Ok({label})"),
                Err(e) => format!("Err({e})"),
            };
            cell("Reference", for_good, endian, after > 0, after, &third_text);
            if for_good {
                assert!(third.is_err(), "{what}: the reference is the dead address now");
                assert_eq!(after, 0, "{what}: the old address was not re-asked");
                assert_eq!(client.conn.ior(), &new, "{what}: still re-pointed");
                assert!(client.label().is_err(), "{what}");
                assert_eq!(asked.load(Ordering::SeqCst), 1, "{what}");
            } else {
                assert_eq!(third.expect("restarted at the original"), "original", "{what}");
                assert_eq!(after, 1, "{what}: one request at the original after the death");
                assert!(client.conn.forwarded().is_none(), "{what}: the cache is dropped");
                assert_eq!(client.conn.ior(), &old, "{what}");
                assert_eq!(client.label().expect("served at the original"), "original", "{what}");
                assert_eq!(asked.load(Ordering::SeqCst), 3, "{what}");
            }
            original.stop();
        }
    }
}
