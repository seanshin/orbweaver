//! D029 §5 O0 — a leak test per transparency, for the two that are reachable
//! today.
//!
//! # What makes this different from every group the ledger already counts
//!
//! D031's first ledger wrote its own limit into `run_checks.sh`: *"nothing
//! here CHANGES a hidden property under a live caller. Every group this ledger
//! counts was written for another reason and is being re-read."* That is the
//! difference these tests exist to close. Each one below holds **one live
//! `Connection`**, records what that caller observed, changes the hidden
//! property underneath it — the object moves; the implementation behind the
//! reference is swapped — and asserts the caller's next observation is the
//! same bytes.
//!
//! A test that connects, calls, and compares two *separate* runs measures that
//! two things agree. It does not measure that a caller cannot tell them apart,
//! because no caller was there when the change happened. §6 says transparency
//! is **hunted, not confirmed**, and the hunt needs a caller in the room.
//!
//! *한 개의 살아 있는 연결이 관측을 기록하고, 그 아래에서 숨은 성질을 바꾸고,
//! 다음 관측이 같은 바이트인지 본다. 두 번의 별도 실행을 비교하는 시험은 둘이
//! 일치함을 잴 뿐, 호출자가 구별할 수 없음을 재지 못한다 — 변화가 일어날 때
//! 그 자리에 호출자가 없었기 때문이다.*
//!
//! # The control for a leak test is the leak
//!
//! A leak test that cannot be made red is a group that measures nothing, which
//! is the class this project has found nine times. So the leaks are **built in
//! and switched at run time** rather than described in a commit message:
//! `ORBWEAVER_LEAK_CONTROL=<name>` puts the property back on the wire and the
//! test must go red *naming what the caller could tell*. `spikes/leak_controls.sh`
//! runs every one of them in about a minute and is the replacement for
//! `run_checks.sh`, which no batch is allowed to start.
//!
//! The one control that cannot be switched at run time is named in
//! [`limits_survive_a_move`]'s own documentation, with what it printed.
//!
//! # What these tests do NOT cover
//!
//! Said here rather than left to be discovered, because *"a passing leak test
//! says this caller could not tell, in this way"*:
//!
//! - **One process, loopback, no foreign peer.** The caller is our own
//!   `Connection`. A leak visible only to omniORB's or JacORB's client — a
//!   header field they read and we do not — is invisible here. The harness's
//!   existing `location` groups have the foreign peers and do not have a live
//!   caller across the change; these have the live caller and no foreign peer.
//!   Neither is the other's replacement.
//! - **The reply body, not the reply header.** The comparison is over the
//!   whole body, the status, the version and the byte order. A GIOP header
//!   field that differed would be caught only where it changes one of those.
//! - **Two backends, not N.** `backend_swapped_under_a_live_caller` swaps one
//!   implementation for one other. A third that agreed with neither is not
//!   measured by any number of runs of this test.
//! - **Nothing here is about load, language or lifecycle.** Those three are
//!   counted `SKIPPED` in `spikes/leak_tests.sh`, each naming what it waits on.

use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbweaver_cdr::Encoder;
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{Request, SharedDispatch, SystemException};
use orbweaver_giop::{Connection, Forward, Ior};

/// Long enough that a busy machine does not produce a false red, short enough
/// that a hung fixture is a test failure rather than a hung suite.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The object key both servers answer to. **The same key on both**: a move
/// that changed the key would be a different object, not the same one
/// somewhere else.
const KEY: &[u8] = b"the-account";

/// The bulk reply's size. Comfortably over [`SMALL_LIMIT`] and comfortably
/// under the compiled default, so a reply of this size is refused **only**
/// because the caller lowered its own ceiling.
const BULK: usize = 64 * 1024;

/// The ceiling the caller configures on itself before the move. The number is
/// arbitrary; what matters is that it is the caller's and that a `bulk` reply
/// does not fit under it.
const SMALL_LIMIT: usize = 8 * 1024;

/// Which leak this process is putting back. Read once, from the environment,
/// so a control run needs no source edit and the green run needs no flag.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Leak {
    /// Nothing put back. The only value a normal `cargo test` ever sees.
    None,
    /// The old address stops redirecting and just says the object is gone —
    /// *skip the forward*, which is the leak in its crudest form.
    NoForward,
    /// The servant puts its own listening port in the reply, so the answer
    /// says where it was answered. This is `moe::Router::select`'s leak
    /// (D029 §6.1, location row) reduced to one field.
    Address,
    /// The second implementation answers a byte differently from the first.
    Backend,
}

impl Leak {
    /// Named separately from [`Leak::from_env`] so the arms can be checked
    /// without writing to the environment — `unsafe_code = "forbid"` is a
    /// workspace lint and `std::env::set_var` is unsafe from the 2024 edition
    /// on, which is a good reason not to have a test that mutates process
    /// state other tests are reading anyway.
    fn parse(s: &str) -> Leak {
        match s {
            "" | "none" => Leak::None,
            "no_forward" => Leak::NoForward,
            "address" => Leak::Address,
            "backend" => Leak::Backend,
            other => panic!(
                "ORBWEAVER_LEAK_CONTROL={other:?} is not a leak this file knows: \
                 none, no_forward, address, backend"
            ),
        }
    }

    fn from_env() -> Leak {
        Leak::parse(&std::env::var("ORBWEAVER_LEAK_CONTROL").unwrap_or_default())
    }
}

/// Everything one call let the caller see.
///
/// Compared whole. Naming the fields individually in each assertion would let
/// a field added later go uncompared, which is how a byte-identity check
/// quietly narrows.
#[derive(PartialEq, Eq, Debug)]
struct Observation {
    status: String,
    version: String,
    endian: String,
    body: Vec<u8>,
}

fn observe(c: &mut Connection, op: &str) -> Result<Observation, String> {
    let r = c.invoke_nullary(op).map_err(|e| e.to_string())?;
    let mut d = r.body().map_err(|e| e.to_string())?;
    let n = d.remaining();
    let body = d.get_bytes(n).map_err(|e| e.to_string())?.to_vec();
    Ok(Observation {
        status: format!("{:?}", r.status),
        version: format!("{:?}", r.version),
        endian: format!("{:?}", r.endian),
        body,
    })
}

/// A two-implementation servant that can also be told the object has moved.
///
/// `SharedDispatch` and not `Dispatch` on purpose: the interesting changes
/// happen from the *test* thread while the serving thread is inside a call, so
/// the servant has to be reachable through `&self`.
struct Account {
    /// 0 or 1 — which implementation answers. Flipped under a live caller.
    backend: AtomicUsize,
    /// Where this servant redirects, once the object has moved. `None` is
    /// "still here".
    moved_to: Mutex<Option<Ior>>,
    /// The port this servant listens on. Only ever reaches the wire under
    /// [`Leak::Address`].
    port: u16,
    /// How many calls each implementation answered. Server-side evidence that
    /// the swap took effect — asking the *caller* which backend served it
    /// would be asking it to tell us the thing it must not be able to tell.
    served: [AtomicUsize; 2],
    leak: Leak,
}

impl Account {
    fn new(port: u16, leak: Leak) -> Account {
        Account {
            backend: AtomicUsize::new(0),
            moved_to: Mutex::new(None),
            port,
            served: [AtomicUsize::new(0), AtomicUsize::new(0)],
            leak,
        }
    }

    fn moved(&self) -> bool {
        self.moved_to.lock().expect("moved_to").is_some()
    }
}

/// Implementation zero: computes the answer.
fn computed(out: &mut Encoder) {
    let mut total: i32 = 0;
    for _ in 0..42 {
        total += 101;
    }
    out.put_i32(total);
    let mut who = String::from("acct");
    who.push('-');
    who.push('7');
    out.put_str(&who);
}

/// Implementation one: reads the answer out of a table.
///
/// Deliberately a *different way of arriving at the same bytes*, not the same
/// code called twice. A swap between two aliases of one function measures
/// nothing.
const LEDGER: [(&str, i32); 3] = [("acct-6", 4141), ("acct-7", 4242), ("acct-8", 4343)];

fn tabled(out: &mut Encoder, leak: Leak) {
    let (name, amount) = LEDGER[1];
    out.put_i32(amount);
    if leak == Leak::Backend {
        // The leak: this implementation spells the account differently. A
        // caller comparing bytes sees which one answered.
        out.put_str("acct-07");
    } else {
        out.put_str(name);
    }
}

impl SharedDispatch for Account {
    /// Stated, because D036 made it required. This fixture is the subject of the
    /// location and backend leak tests, whose claims are about what a caller can
    /// tell across a MOVE and a SWAP — not about which keys exist. It answers
    /// for any key, and now says so.
    fn knows(&self, _object_key: &[u8]) -> bool {
        true
    }

    fn dispatch(
        &self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        if self.moved() && self.leak == Leak::NoForward {
            // The leak: the object moved and the old address says so instead
            // of redirecting.
            return Err(SystemException::object_not_exist());
        }
        let which = self.backend.load(Ordering::SeqCst);
        self.served[which].fetch_add(1, Ordering::SeqCst);
        match request.operation.as_str() {
            "balance" => {
                if which == 0 {
                    computed(out);
                } else {
                    tabled(out, self.leak);
                }
                if self.leak == Leak::Address {
                    // The leak: the answer names where it was answered.
                    out.put_u16(self.port);
                }
                Ok(())
            }
            "bulk" => {
                out.put_octet_seq(&vec![0xAB; BULK]);
                Ok(())
            }
            _ => Err(SystemException::bad_operation()),
        }
    }

    fn redirect(&self, _request: &Request) -> Option<Forward> {
        if self.leak == Leak::NoForward {
            return None;
        }
        self.moved_to.lock().expect("moved_to").clone().map(Forward::Temporary)
    }
}

/// A bound server plus the thread serving it, stopped on drop.
struct Fixture {
    ior: Ior,
    port: u16,
    servant: Arc<Account>,
    stop: Arc<AtomicBool>,
    joined: Option<std::thread::JoinHandle<()>>,
}

impl Fixture {
    fn start(leak: Leak) -> Fixture {
        let orb = Orb::new();
        let server = orb.server("127.0.0.1:0", KEY.to_vec()).expect("bind");
        let port = server.local_addr().expect("bound address").port();
        let ior = server.ior("IDL:Account:1.0", "127.0.0.1").expect("ior");
        let servant = Arc::new(Account::new(port, leak));
        let stop = Arc::new(AtomicBool::new(false));
        let serving_servant = Arc::clone(&servant);
        let serving_stop = Arc::clone(&stop);
        let joined = std::thread::spawn(move || {
            server
                .serve_shared(&*serving_servant, move || serving_stop.load(Ordering::SeqCst))
                .expect("serve");
        });
        Fixture { ior, port, servant, stop, joined: Some(joined) }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The accept loop polls the flag at `STOP_POLL`; a connect wakes it
        // sooner and costs nothing when it has already gone.
        let _ = TcpStream::connect(("127.0.0.1", self.port)).map(|mut s| s.write_all(&[]));
        if let Some(h) = self.joined.take() {
            let _ = h.join();
        }
    }
}

// ── Location ────────────────────────────────────────────────────────────────

/// **A caller holding only a reference cannot tell the target moved.**
///
/// One `Connection`, dialled at the old address, kept open across the move.
/// The object is then given a new home, and the same connection asks the same
/// question again. The two observations are compared **whole** — status,
/// version, byte order and every byte of the body.
///
/// That whole-body comparison is also the address assertion and this is worth
/// spelling out, because it is the part that would be easy to write as a
/// weaker test: the two servers are on **different ports**, so any address
/// that reached the reply would differ between them and the comparison would
/// fail. There is no need to go looking for a port in the bytes.
///
/// # The controls
///
/// - `ORBWEAVER_LEAK_CONTROL=no_forward` — the old address answers
///   `OBJECT_NOT_EXIST` instead of redirecting. Red at the second call.
/// - `ORBWEAVER_LEAK_CONTROL=address` — the servant puts its own port in the
///   reply. Red at the comparison, naming the differing bytes.
///
/// # What it does not measure
///
/// That the *forward itself* was invisible to a foreign client — the harness's
/// `LOCATION_FORWARD_PERM` group has omniORB and JacORB for that and has no
/// live caller across the move. And a permanent forward: this is
/// `Forward::Temporary`, because a temporary one is the case where the caller
/// keeps the reference it dialled, which is the harder half to get right.
#[test]
fn a_move_under_a_live_caller_is_invisible() {
    let leak = Leak::from_env();
    let home = Fixture::start(leak);
    let away = Fixture::start(leak);
    assert_ne!(home.port, away.port, "the two homes must be distinguishable");

    let mut caller = Connection::connect(&home.ior, TIMEOUT).expect("connect to the old address");
    let before = observe(&mut caller, "balance").expect("the first answer");
    assert!(caller.forwarded().is_none(), "nothing has moved yet");

    // ── the object moves, underneath a caller that is holding the line ──
    *home.servant.moved_to.lock().expect("moved_to") = Some(away.ior.clone());

    let after = match observe(&mut caller, "balance") {
        Ok(o) => o,
        Err(e) => panic!(
            "THE CALLER COULD TELL THE TARGET MOVED: the same connection asking the same \
             question got {e} where it had got an answer. Before the move it observed \
             {before:?}"
        ),
    };
    assert_eq!(
        after, before,
        "THE CALLER COULD TELL THE TARGET MOVED: the reply changed across the move. \
         The two servants differ only in where they run, so a difference here is \
         location reaching the caller."
    );
    assert!(
        caller.forwarded().is_some(),
        "nothing was forwarded, so this run compared two answers from the same place \
         and measured nothing about a move"
    );
    assert!(
        away.servant.served[0].load(Ordering::SeqCst) >= 1,
        "the new home never served a call, so the move did not happen"
    );
}

/// **A move must not change the caller's own configuration.**
///
/// The recorded instance of this leak, D029 §6.1's location row: until
/// 2026-08-26 `Connection::move_to` restored a hand-written field list and
/// dropped two configured limits on every forward, so *the caller's limits
/// changed when the target moved* — invisibly, because nothing measured a
/// limit **after** a forward. This measures one after a forward.
///
/// The limit is measured **by behaviour, not by a getter**: `orb_limits` is
/// private, and a getter would in any case answer from the field a control
/// could leave alone. A reply larger than the ceiling the caller set is
/// refused; if the ceiling were silently restored to the compiled default
/// across the move, the same call would start succeeding.
///
/// # The control that is not switchable, and what it printed
///
/// This one lives in `orbweaver-giop`, which this batch does not own, so it
/// cannot be an `ORBWEAVER_LEAK_CONTROL` arm. It was run as a temporary edit
/// and reverted: deleting the `self.set_orb_limits(limits);` line from
/// `Connection::move_to` (`crates/orbweaver-giop/src/lib.rs`) makes this test
/// print
///
/// ```text
/// THE CALLER'S OWN LIMIT CHANGED WHEN THE TARGET MOVED: a reply of 65536
/// bytes was refused before the move and accepted after it, so the ceiling the
/// caller set on itself did not survive the forward.
/// ```
///
/// # What it does not measure
///
/// One of the five numbers `orb_limits` carries. `fragment_threshold`,
/// `max_fragments`, `max_forward_hops` and `follow_timeout` ride the same code
/// path and are **not** independently observed here, so a change that dropped
/// only one of those four would pass this test. That is why the fix in
/// `move_to` is a group read (`orb_limits()`) rather than four assignments —
/// the code makes the class impossible and this test measures one member of it.
#[test]
fn limits_survive_a_move() {
    let leak = Leak::from_env();
    let home = Fixture::start(leak);
    let away = Fixture::start(leak);

    let mut caller = Connection::connect(&home.ior, TIMEOUT).expect("connect to the old address");
    caller.set_max_message_size(SMALL_LIMIT);

    // A first ordinary call, so this is a live caller and not a fresh dial.
    observe(&mut caller, "balance").expect("the first answer");

    // ── the object moves ──
    *home.servant.moved_to.lock().expect("moved_to") = Some(away.ior.clone());
    if let Err(e) = observe(&mut caller, "balance") {
        panic!(
            "THE CALLER COULD TELL THE TARGET MOVED: the same connection asking the same \
             question got {e} where it had got an answer, so this run never reached the \
             limit it exists to measure."
        );
    }
    assert!(caller.forwarded().is_some(), "no forward was followed; nothing was measured");

    // The ceiling the caller set on itself, asked after the forward.
    match observe(&mut caller, "bulk") {
        Err(_) => {}
        Ok(_) => panic!(
            "THE CALLER'S OWN LIMIT CHANGED WHEN THE TARGET MOVED: a reply of {BULK} \
             bytes was refused before the move and accepted after it, so the ceiling \
             the caller set on itself did not survive the forward."
        ),
    }

    // The probe means nothing unless the same reply is fine at the default
    // ceiling: otherwise `bulk` might simply be a broken operation and this
    // test would go green over a dropped limit.
    let mut unlimited = Connection::connect(&away.ior, TIMEOUT).expect("connect to the new home");
    observe(&mut unlimited, "bulk")
        .expect("a bulk reply is fine at the default ceiling, so the refusal above was the limit");
}

// ── Backend ─────────────────────────────────────────────────────────────────

/// **A caller holding only a reference cannot tell what implements it.**
///
/// One reference, one object key, one live connection — and the implementation
/// behind it replaced mid-session. The two implementations arrive at the same
/// bytes by different routes: one computes the number and builds the string,
/// the other reads both out of a table. A swap between two aliases of one
/// function would measure nothing.
///
/// # The control
///
/// `ORBWEAVER_LEAK_CONTROL=backend` — the table implementation spells the
/// account `acct-07`. Red at the comparison.
///
/// # What it does not measure
///
/// Two implementations, not N: a third that agreed with neither is not
/// measured by any number of runs of this. And both are Rust in one process —
/// *what it is written in* is the language row and is `SKIPPED`, not this one.
/// The harness's existing `backend` groups compare a generated skeleton with a
/// hand-written servant, which is a stronger pair and has no live caller
/// across the change; this has the live caller and the weaker pair.
#[test]
fn backend_swapped_under_a_live_caller() {
    let leak = Leak::from_env();
    let fx = Fixture::start(leak);

    let mut caller = Connection::connect(&fx.ior, TIMEOUT).expect("connect");
    let before = observe(&mut caller, "balance").expect("the first answer");

    // ── the implementation is replaced, underneath a caller holding the line ──
    fx.servant.backend.store(1, Ordering::SeqCst);

    let after = observe(&mut caller, "balance").expect("the answer from the other implementation");
    assert_eq!(
        after, before,
        "THE CALLER COULD TELL WHICH IMPLEMENTATION ANSWERED: the reply changed when \
         the servant behind the reference was replaced, and the reference did not."
    );

    // Server-side evidence that the swap took effect. Asking the caller which
    // backend served it would be asking it to report the thing it must not be
    // able to tell.
    assert!(
        fx.servant.served[0].load(Ordering::SeqCst) >= 1
            && fx.servant.served[1].load(Ordering::SeqCst) >= 1,
        "both implementations must have served at least one call, or the swap did not \
         happen and this run compared one implementation with itself: {:?} / {:?}",
        fx.servant.served[0].load(Ordering::SeqCst),
        fx.servant.served[1].load(Ordering::SeqCst),
    );
}

/// The controls have to be reachable, or `spikes/leak_controls.sh` is asking
/// for arms that silently do nothing.
///
/// This is the `dk_peer` lesson in miniature: the expected set checked against
/// the owner before any leg runs, so a typo fails as *our* table.
#[test]
fn every_named_control_is_a_leak_this_file_knows() {
    let named = ["none", "no_forward", "address", "backend"];
    let mut seen = std::collections::BTreeSet::new();
    for name in named {
        // Panics, with the list, on a name this file does not know.
        seen.insert(format!("{:?}", Leak::parse(name)));
    }
    assert_eq!(
        seen.len(),
        named.len(),
        "two of {named:?} resolved to the same leak, so one control is not \
         controlling what its name says: {seen:?}"
    );
    assert_eq!(Leak::parse("none"), Leak::None, "the default must put no leak back");
}
