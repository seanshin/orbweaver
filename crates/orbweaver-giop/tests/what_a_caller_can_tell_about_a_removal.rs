//! D029 §6.1 lifecycle, and D035 §5's option **B**, approved 2026-08-27: what a
//! caller can tell when a target is **removed** under it.
//!
//! # The floor this file names rather than closes
//!
//! A caller of a removed target **can tell it is gone.** There is nowhere else
//! for its request to go, and nothing inside one process can change that,
//! because a caller has to be given one address to send a first packet to.
//! D035 §4 asked whether moving that leak from N addresses to one is *a row
//! that no longer leaks* or *a row that leaks once instead of N times*, and the
//! owner answered **the latter: displacement is not closure**. So the leak is
//! recorded as an irreducible floor of a single-node deployment — the same
//! shape D029 §6.1 already records for the bootstrap address, in its own words
//! *"displaced, not closed — from N channels to one bootstrap"*.
//!
//! **This file therefore does not assert that a removal is invisible.** An
//! assertion like that would be false, and writing it would be the failure mode
//! B exists to avoid: a row that reads *closed* when what happened is that the
//! leak was named.
//!
//! # What is measured is everything above the floor
//!
//! **The first draft of this file measured the wrong thing and the test said
//! so, which is written down rather than quietly corrected.** It asserted that
//! a caller *cannot tell which* target was removed. That claim is empty: the
//! caller chose which reference to dial, so it knows which one it lost before
//! anything is removed. The assertion failed on its first run for a better
//! reason than it was written for.
//!
//! The property that is real, and that a deployment depends on, is
//! **isolation**: removing one target must be invisible to a caller of a
//! *different* one. A caller of B must not be able to tell that A was removed —
//! not from its replies, not from its connection, not at all. That is
//! refutable, and `ORBWEAVER_LEAK_CONTROL=removal_isolation` refutes it.
//!
//! The second thing the first draft got wrong is worth keeping too: a removed
//! target **still answered** on an already-open connection. That is not a bug,
//! it is D034's graceful shutdown at request granularity, measured from a
//! peer's socket in `orb_stops_what_it_handed_out.rs`. So the floor is observed
//! the way a caller holding only a reference actually observes it — by
//! **dialling again** — and a live connection's drain is D034's subject, not
//! this file's.
//!
//! # Why half of this file is an anti-vacuity guard
//!
//! *Cannot tell* passes in every world where nothing happens. This project has
//! measured that directly: the backend leg stayed green when `Dispatch::knows`
//! was made a blanket `false`, because a server that serves nothing answers
//! both keys identically too. So the equality assertion here is worth nothing
//! on its own, and it is paired with a counted companion proving the two
//! targets **could** be told apart while they were alive. If that companion
//! ever goes quiet, the main assertion is comparing two identical nothings and
//! must not be read as a measurement.
//!
//! # What is NOT claimed
//!
//! That a removal is indistinguishable from *any* disappearance. It is not: an
//! ORB that stops through [`Orb::shutdown`] says §9.4.10's goodbye, and a
//! process that is killed leaves a reset. The measured claim is the narrower
//! and honest one — **two targets removed the same way are indistinguishable to
//! a caller** — and the difference between a goodbye and a reset is the second
//! floor, named here rather than discovered later.
//!
//! *제거된 대상의 호출자는 그것이 사라졌음을 **알 수 있다.** 한 프로세스 안에서는
//! 그것을 바꿀 수 없다 — 호출자는 첫 패킷을 보낼 주소 하나를 받아야 하기 때문이다.
//! D035 §4의 질문에 소유자는 **전가는 폐쇄가 아니다**라고 답했고, 그래서 이 구멍은
//! 닫는 대상이 아니라 **이름 붙인 바닥**으로 기록된다. 이 파일은 제거가 보이지
//! 않는다고 주장하지 않는다 — 그것은 거짓이고, B가 피하려는 실패다. 재는 것은 그
//! 바닥 위의 전부다: 호출자는 **무엇을** 잃었는지 알 수 없어야 한다. 그리고 절반은
//! 공허하지 않음을 지키는 대조다 — 아무 일도 일어나지 않는 세계에서도 "알 수 없다"는
//! 통과하기 때문이다.*

use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{Dispatch, Request, SystemException};
use orbweaver_giop::{MsgType, Version, decode_reply, encode_request, read_message};

const KEY_A: &[u8] = b"RemovalProbeA";
const KEY_B: &[u8] = b"RemovalProbeB";
const ANSWER_A: i32 = 11;
const ANSWER_B: i32 = 22;
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// The leaks this file can put back, switched at run time.
///
/// Built in rather than patched in, for the reason
/// `crates/orbweaver-test/tests/what_a_caller_can_tell.rs` records: a control
/// that needs the source edited is a control nobody runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Leak {
    None,
    /// **The leak:** removing one target takes the other down with it, so a
    /// caller of B can tell that A was removed. This is the shape a shared
    /// pool, a shared listener or a process-wide stop would produce.
    RemovalIsolation,
}

impl Leak {
    /// Parsing is its own function so the `dk_peer` test below can exercise it
    /// **without touching the environment**. `set_var` is `unsafe` since Rust
    /// 2024 and this workspace forbids `unsafe` outright, so a control that
    /// needed it would be a control that cannot be written here.
    fn parse(name: &str) -> Self {
        match name {
            "removal_isolation" => Leak::RemovalIsolation,
            "none" => Leak::None,
            other => panic!(
                "ORBWEAVER_LEAK_CONTROL={other} is not a leak this file knows; \
                 spikes/leak_controls.sh and this enum must name the same set"
            ),
        }
    }

    fn from_env() -> Self {
        match std::env::var("ORBWEAVER_LEAK_CONTROL") {
            Ok(v) => Self::parse(&v),
            Err(_) => Leak::None,
        }
    }
}

/// A servant that answers one number, so two of them are distinguishable while
/// they are alive — which is the anti-vacuity guard's whole subject.
struct One(i32);

impl Dispatch for One {
    /// Stated, because D036 made it required.
    ///
    /// `One` is distinguished by the number it answers, not by the key it is
    /// reached at — two of them run on two servers, which is what makes the
    /// removal of one invisible to a caller of the other. Answering for any key
    /// is therefore right here and is now said rather than inherited.
    fn knows(&self, _object_key: &[u8]) -> bool {
        true
    }

    fn dispatch(&mut self, req: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        if req.operation == "ping" {
            out.put_i32(self.0);
            return Ok(());
        }
        Err(SystemException::bad_operation())
    }
}

/// What a caller observed, reduced to what it could *tell* — never to raw
/// bytes, because CDR padding content is undefined by the specification and
/// comparing buffers is this project's recorded way of manufacturing false
/// failures.
#[derive(Debug, PartialEq, Eq)]
enum Observed {
    /// A reply and its decoded body.
    Reply(i32),
    /// §9.4.10's goodbye, and its length so "and no body" is assertable.
    Goodbye(usize),
    /// The socket ended without a GIOP message — a reset or a clean EOF, which
    /// a caller cannot tell apart and neither does this.
    Gone,
    /// Anything else, kept rather than panicked on so a failure prints what
    /// actually arrived.
    Other(String),
}

fn request(id: u32, key: &[u8]) -> Vec<u8> {
    encode_request(Version::V1_2, Endian::Little, id, key, "ping", true, |_| {})
        .expect("our own encoder must produce our own request")
}

/// One observation: write a request, read whatever comes back.
fn observe(peer: &mut TcpStream, id: u32, key: &[u8]) -> Observed {
    peer.set_read_timeout(Some(READ_TIMEOUT)).expect("a read timeout the test can fail on");
    if peer.write_all(&request(id, key)).is_err() {
        // A write to a socket the other end has closed is itself "gone", and a
        // caller cannot tell it from a read that ends the same way.
        return Observed::Gone;
    }
    match read_message(peer, 64 * 1024) {
        Ok(msg) => match msg.msg_type {
            MsgType::Reply => {
                let len = msg.bytes.len();
                match decode_reply(msg) {
                    Ok(r) => match r
                        .body()
                        .map_err(|e| e.to_string())
                        .and_then(|mut b| b.get_i32().map_err(|e| e.to_string()))
                    {
                        Ok(v) => Observed::Reply(v),
                        Err(e) => Observed::Other(format!("undecodable body: {e}")),
                    },
                    Err(e) => Observed::Other(format!("bad reply of {len}B: {e}")),
                }
            }
            MsgType::CloseConnection => Observed::Goodbye(msg.bytes.len()),
            other => Observed::Other(format!("{other:?}")),
        },
        Err(orbweaver_giop::Error::Io(e))
            if matches!(
                e.kind(),
                std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
            ) =>
        {
            Observed::Gone
        }
        Err(e) => Observed::Other(format!("read failed: {e}")),
    }
}

/// One target: an ORB, a server bound on an ephemeral port, and a thread
/// serving it. Removal is `orb.shutdown()`, which is what a deployment does
/// and what D034 measured from a peer's socket.
struct Target {
    orb: Orb,
    addr: std::net::SocketAddr,
    key: &'static [u8],
    serving: Option<std::thread::JoinHandle<()>>,
}

fn start_target(key: &'static [u8], answer: i32) -> Target {
    let orb = Orb::new();
    let server = orb.server("127.0.0.1:0", key.to_vec()).expect("bind");
    let addr = server.local_addr().expect("bound address");
    let serving = std::thread::spawn(move || {
        let mut servant = One(answer);
        // `|| false` deliberately: the shape 17 of this workspace's 63 serve
        // sites use, so the removal under test is the ORB's own flag rather
        // than a stop condition written for the test.
        server.serve(&mut servant, || false).expect("serve");
    });
    Target { orb, addr, key, serving: Some(serving) }
}

impl Target {
    fn connect(&self) -> TcpStream {
        self.connect_or_gone().expect("connect to a target that is supposed to be alive")
    }

    /// Dialling the reference, which is what a caller holding only a reference
    /// does. `None` is a target that is gone — the refusal a caller actually
    /// meets, rather than a panic that would turn the floor into a crash.
    fn connect_or_gone(&self) -> Option<TcpStream> {
        let peer = TcpStream::connect(self.addr).ok()?;
        peer.set_nodelay(true).expect("nodelay");
        Some(peer)
    }

    /// Remove it, the way a deployment removes it.
    fn remove(&mut self) {
        let report = self.orb.shutdown();
        assert!(report.servers() >= 1, "the target's own server was the one stopped");
    }
}

impl Drop for Target {
    fn drop(&mut self) {
        // Whatever the test did, the ORB stops — so a failing assertion leaks
        // no listener and no thread into the rest of the suite. `shutdown` is
        // idempotent and says so, which is why calling it again here is safe.
        let _ = self.orb.shutdown();
        // NOT joined: a build where the flag never reaches the serving loop
        // would hang here, and a hung test is the one diagnostic nobody can
        // read. The thread dies with the process; the assertion is what
        // matters and it has already been made.
        drop(self.serving.take());
    }
}

/// **The anti-vacuity guard, as its own counted test.**
///
/// If this goes quiet, the assertion below is comparing two identical nothings
/// and is not a measurement of anything. It is a separate `#[test]` rather than
/// a line inside the other one precisely so that it is counted, named and
/// visible when it fails.
#[test]
fn the_two_targets_could_be_told_apart_while_they_were_alive() {
    let a = start_target(KEY_A, ANSWER_A);
    let b = start_target(KEY_B, ANSWER_B);

    let mut pa = a.connect();
    let mut pb = b.connect();
    let seen_a = observe(&mut pa, 1, a.key);
    let seen_b = observe(&mut pb, 1, b.key);

    assert_eq!(seen_a, Observed::Reply(ANSWER_A), "target A answers its own number");
    assert_eq!(seen_b, Observed::Reply(ANSWER_B), "target B answers its own number");
    assert_ne!(
        seen_a, seen_b,
        "the two targets must be distinguishable while alive, or the removal test below \
         compares two identical nothings and measures nothing at all"
    );
}

/// **The measurement: removing one target is invisible to a caller of another.**
///
/// A caller of B holds a live connection and has already been answered. A is
/// then removed. B's caller must observe exactly what it observed before — the
/// same reply, from the same connection — and a fresh dial of B must still
/// work, because "invisible" has to cover both the connection it holds and the
/// reference it holds.
#[test]
fn removing_one_target_is_invisible_to_a_caller_of_another() {
    let leak = Leak::from_env();
    let mut a = start_target(KEY_A, ANSWER_A);
    let mut b = start_target(KEY_B, ANSWER_B);

    let mut held = b.connect();
    let before = observe(&mut held, 1, b.key);
    assert_eq!(before, Observed::Reply(ANSWER_B), "B answers before anything is removed");
    // A is alive too, and asserted — otherwise "removing A changed nothing"
    // could be true because A was never there, which is the vacuous green this
    // file's header is about.
    let mut a_peer = a.connect();
    assert_eq!(observe(&mut a_peer, 1, a.key), Observed::Reply(ANSWER_A), "A answers first");

    a.remove();
    if leak == Leak::RemovalIsolation {
        // THE LEAK: A's removal takes B with it. A shared pool, a shared
        // listener or a process-wide stop all produce exactly this.
        b.remove();
    }

    let after = observe(&mut held, 2, b.key);
    assert_eq!(
        after, before,
        "THE CALLER COULD TELL ANOTHER TARGET WAS REMOVED: the connection it already held \
         answered {before:?} before A was removed and {after:?} after. Removing one target \
         must be invisible to a caller of a different one."
    );

    let mut fresh = b.connect_or_gone();
    let redialled = match fresh.as_mut() {
        Some(p) => observe(p, 3, b.key),
        None => Observed::Gone,
    };
    assert_eq!(
        redialled,
        Observed::Reply(ANSWER_B),
        "THE CALLER COULD TELL ANOTHER TARGET WAS REMOVED: a fresh dial of B's own \
         reference answered {redialled:?} after A — and only A — was removed"
    );
}

/// The floor, asserted rather than left to prose.
///
/// This is the half B **names** rather than closes, and naming it in a test is
/// what stops the row from drifting into reading *closed*: if a future change
/// ever let a removed target keep answering a fresh dial, this goes red and the
/// row's wording has to be revisited on purpose rather than by accident.
///
/// Observed by **dialling again**, not down a held connection. A held
/// connection drains gracefully at request granularity — that is D034's bound
/// and `orb_stops_what_it_handed_out.rs` measures it from a peer's socket. A
/// caller holding only a reference is the subject here, and what it does is
/// dial.
#[test]
fn a_caller_of_a_removed_target_can_tell_it_is_gone_and_that_is_the_floor() {
    let mut a = start_target(KEY_A, ANSWER_A);
    let mut peer = a.connect();
    assert_eq!(observe(&mut peer, 1, a.key), Observed::Reply(ANSWER_A), "alive first");

    a.remove();

    let redialled = match a.connect_or_gone().as_mut() {
        Some(p) => observe(p, 2, a.key),
        None => Observed::Gone,
    };
    assert_eq!(
        redialled,
        Observed::Gone,
        "a caller holding only the reference learns the target is gone — this is the leak \
         D035 §5 B records as a floor rather than closing, and it is asserted so that a \
         change which made it stop being true could not pass unnoticed: got {redialled:?}"
    );
}

/// The `dk_peer` check: every control `spikes/leak_controls.sh` can name is one
/// this file knows, so a control table entry cannot silently point at nothing.
#[test]
fn every_named_control_is_a_leak_this_file_knows() {
    // `parse` panics on an unknown name, so this asserts by not panicking.
    // The environment is never written: `set_var` is `unsafe` in Rust 2024 and
    // this workspace forbids `unsafe`, which is why parsing is a function.
    assert_eq!(Leak::parse("none"), Leak::None);
    assert_eq!(Leak::parse("removal_isolation"), Leak::RemovalIsolation);
}
