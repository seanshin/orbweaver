//! D019 step 4: the configuration is **live**, not merely held.
//!
//! # Why this file exists, and why the tests that came before it were not
//! enough
//!
//! Step 3 gave the eight numbers a home and tested it thoroughly: that
//! `-ORBmaxMessageSize 4096` parses, that it is refused when it is zero, that
//! it round-trips through [`OrbConfig`], that an unset field answers the
//! compiled constant. Every one of those tests passed, and **on 2026-08-26
//! every call site of `OrbConfig`'s eight getters was a unit test or a spike
//! printing them.** Nothing in `Pool`, `Server` or the encoder asked the ORB
//! for anything, so `-ORBmaxMessageSize 4096` changed nothing a peer could
//! observe — and the whole suite was green, because *held* was all it ever
//! asserted.
//!
//! That is the gap this file measures, so every test here asserts a
//! **difference in behaviour a peer can see**, and each carries its own
//! control: the same exchange through an unconfigured [`Orb`], which must come
//! out the way it always did. A test with no control could pass because the
//! configuration works or because the exchange never worked; the two are told
//! apart here rather than assumed.
//!
//! *설정이 **들려 있는지**가 아니라 **작동하는지**를 잰다. 이전 테스트는 전부
//! 통과했고, 그러면서도 와이어에서는 아무것도 바뀌지 않았다.*

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_giop::orb::{Orb, OrbConfig};
use orbweaver_giop::server::{Dispatch, Request, SystemException};
use orbweaver_giop::{MsgType, Version, encode_request, read_message};

const KEY: &[u8] = b"OrbConfigProbe";
const DEADLINE: Duration = Duration::from_secs(5);

/// Answers every call with nothing, which is a `void` operation's reply.
struct Silent;

impl Dispatch for Silent {
    /// Stated, because D036 made it required. This fixture is about ORB config
    /// reaching the wire, not about key selection: it answers for any key.
    fn knows(&self, _object_key: &[u8]) -> bool {
        true
    }

    fn dispatch(&mut self, _req: &Request, _out: &mut Encoder) -> Result<(), SystemException> {
        Ok(())
    }
}

/// A server built from `args` — the deployment's `-ORB…` command line — served
/// on a thread, with the address it actually bound.
struct Served {
    addr: std::net::SocketAddr,
    ior: orbweaver_giop::Ior,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Served {
    /// `args` is exactly what an operator would type. Going through
    /// [`OrbConfig::from_orb_args`] rather than a setter is deliberate: the
    /// claim under test is that the *command line* reaches the wire, and a test
    /// that built the config in Rust would skip the half that was broken.
    fn with(args: &[&str]) -> Served {
        Served::serving(args, Silent)
    }

    /// As [`Served::with`], with a dispatch of your own — so the *client* half
    /// of the configuration can be measured against a server that replies with
    /// something worth capping.
    fn serving<D: Dispatch + Send + 'static>(args: &[&str], mut d: D) -> Served {
        let (config, rest) = OrbConfig::from_orb_args(args).expect("the arguments are ours");
        assert!(rest.is_empty(), "every argument here is an -ORB one");
        let orb = Orb::with_config(config).expect("no -ORBInitRef to resolve");
        let server = orb.server("127.0.0.1:0", KEY.to_vec()).expect("bind");
        let addr = server.local_addr().expect("bound");
        let ior = server.ior("IDL:probe:1.0", "127.0.0.1").expect("ior");
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            let _ = server.serve(&mut d, || flag.load(Ordering::Relaxed));
        });
        Served { addr, ior, stop, thread: Some(thread) }
    }

    fn client(&self) -> TcpStream {
        let s = TcpStream::connect(self.addr).expect("connect");
        s.set_read_timeout(Some(DEADLINE)).expect("read timeout");
        s
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Unblock the accept loop, which is sleeping on its own poll interval.
        let _ = TcpStream::connect(self.addr);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A `Request` whose body is `body_len` octets of filler, so the message this
/// produces is comfortably larger than `body_len` and can be sized against a
/// configured ceiling.
fn request_of(body_len: usize) -> Vec<u8> {
    encode_request(Version::V1_2, Endian::Big, 1, KEY, "probe", true, |e| {
        e.put_octet_seq(&vec![0x5A; body_len]);
    })
    .expect("encodes")
}

/// What the peer said back, or `None` if it hung up without saying anything —
/// which is itself an answer and has to be distinguishable from a reply.
fn answer(c: &mut TcpStream) -> Option<MsgType> {
    read_message(c, 64 * 1024 * 1024).ok().map(|m| m.msg_type)
}

// ── max_message_size ─────────────────────────────────────────────────────────

/// **The number D019 names, and the one the batch brief calls its payoff.**
///
/// A body over the configured ceiling must be refused by the serving side, and
/// the same body under an unconfigured ORB must be answered. Both halves are
/// asserted in one test so that neither can pass alone: if the refusal were
/// really "this server never answers anything", the control would catch it.
#[test]
fn orb_max_message_size_refuses_a_body_the_default_would_have_answered() {
    let big = request_of(8192);

    // The control first, so a failure reads in the right order: this exchange
    // works, and works for a reason that has nothing to do with a ceiling.
    let plain = Served::with(&[]);
    let mut c = plain.client();
    c.write_all(&big).expect("write");
    assert_eq!(
        answer(&mut c),
        Some(MsgType::Reply),
        "an unconfigured ORB must answer an 8 KiB request exactly as it always did"
    );
    drop(plain);

    // The same bytes, against an ORB told 4096.
    let capped = Served::with(&["-ORBmaxMessageSize", "4096"]);
    let mut c = capped.client();
    c.write_all(&big).expect("write");
    let refused = answer(&mut c);
    assert_ne!(
        refused,
        Some(MsgType::Reply),
        "-ORBmaxMessageSize 4096 must not answer an 8 KiB request; it answered {refused:?}"
    );
}

/// The other half of the same number: a body *under* the configured ceiling is
/// still answered. A cap that refused everything would pass the test above.
#[test]
fn orb_max_message_size_still_answers_under_the_ceiling() {
    let capped = Served::with(&["-ORBmaxMessageSize", "4096"]);
    let mut c = capped.client();
    c.write_all(&request_of(64)).expect("write");
    assert_eq!(answer(&mut c), Some(MsgType::Reply), "a small request must survive a 4096 ceiling");
}

// ── max_connections ──────────────────────────────────────────────────────────

/// `-ORBmaxConnections 1` must turn the second connection away with §9.4.7's
/// goodbye, where the default (64) admits both.
#[test]
fn orb_max_connections_refuses_the_second_connection() {
    let capped = Served::with(&["-ORBmaxConnections", "1"]);
    let mut first = capped.client();
    first.write_all(&request_of(8)).expect("write");
    assert_eq!(answer(&mut first), Some(MsgType::Reply), "the first client is under the cap");

    // The second only reads: a refusal that raced our own write could come
    // back as a reset instead, and the goodbye is what is being measured.
    let mut second = capped.client();
    assert_eq!(
        answer(&mut second),
        Some(MsgType::CloseConnection),
        "over -ORBmaxConnections 1 the second connection must be told, not queued"
    );
    drop(capped);

    // Control: the same two connections against an unconfigured ORB.
    let plain = Served::with(&[]);
    let mut a = plain.client();
    a.write_all(&request_of(8)).expect("write");
    assert_eq!(answer(&mut a), Some(MsgType::Reply), "control: first");
    let mut b = plain.client();
    b.write_all(&request_of(8)).expect("write");
    assert_eq!(
        answer(&mut b),
        Some(MsgType::Reply),
        "control: an unconfigured ORB admits 64, so the second is served"
    );
}

// ── the client half: the pool applies it too ─────────────────────────────────

/// Answers every call with a large body, so a *client*'s ceiling has something
/// to refuse.
struct Loud;

impl Dispatch for Loud {
    /// Stated, because D036 made it required. Same as `Silent` above: this
    /// fixture is about message size on the wire, not about key selection.
    fn knows(&self, _object_key: &[u8]) -> bool {
        true
    }

    fn dispatch(&mut self, _req: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        out.put_octet_seq(&vec![0x5A; 8192]);
        Ok(())
    }
}

/// **The gap the constant sweep found, measured from the calling side.**
///
/// `Pool::acquire` dialled a `Connection` and handed it straight to `Mux::over`
/// without touching either setter, and `Mux` has none — so a pooled call ran on
/// the compiled defaults however the ORB had been configured, and no test could
/// see it because no test configured a pool. Here a client ORB told
/// `-ORBmaxMessageSize 4096` must refuse an 8 KiB *reply* that an unconfigured
/// client accepts, against one and the same server.
///
/// The server is unconfigured in both halves, which is what makes this a
/// measurement of the client: the only thing that differs between the two calls
/// is which ORB the pool came from.
#[test]
fn orb_max_message_size_is_applied_to_a_pooled_connection() {
    let served = Served::serving(&[], Loud);

    // Control: an unconfigured client accepts the 8 KiB reply.
    let plain = Orb::new().pool();
    assert!(
        plain.invoke(&served.ior, "probe", |_| {}).is_ok(),
        "an unconfigured client must accept an 8 KiB reply as it always did"
    );

    // The same call, from a client ORB with a ceiling under the reply size.
    let (config, _) = OrbConfig::from_orb_args(&["-ORBmaxMessageSize", "4096"]).expect("ours");
    let capped = Orb::with_config(config).expect("no init refs").pool();
    let refused = capped.invoke(&served.ior, "probe", |_| {});
    assert!(
        refused.is_err(),
        "-ORBmaxMessageSize 4096 must reach the pooled connection and refuse an 8 KiB reply"
    );
}

// ── max_fragments ────────────────────────────────────────────────────────────

/// `-ORBmaxFragments` bounds how many `Fragment` continuations the reassembler
/// will accept for one logical message — the half of the reassembly bound that
/// was *not* configurable before D019 step 4, because `read_message` took the
/// size ceiling as a parameter and read the fragment ceiling off a constant.
///
/// The same well-formed fragmented request is sent twice: an ORB told to accept
/// two fragments must refuse it, and an unconfigured one (4096) must answer it.
#[test]
fn orb_max_fragments_refuses_a_message_the_default_would_have_reassembled() {
    // Sixteen fragments' worth, at a threshold far under the compiled 1 MiB.
    let pieces = orbweaver_giop::fragment_message(request_of(2048), 128).expect("fragments");
    assert!(pieces.len() > 3, "the probe needs more fragments than the cap under test");

    // Control: the default ceiling reassembles this and answers.
    let plain = Served::with(&[]);
    let mut c = plain.client();
    for p in &pieces {
        c.write_all(p).expect("write");
    }
    assert_eq!(
        answer(&mut c),
        Some(MsgType::Reply),
        "an unconfigured ORB reassembles {} fragments as it always did",
        pieces.len()
    );
    drop(plain);

    let capped = Served::with(&["-ORBmaxFragments", "2"]);
    let mut c = capped.client();
    for p in &pieces {
        // A refused reassembly closes the connection under us, so a write that
        // fails here is the refusal arriving, not a test failure.
        if c.write_all(p).is_err() {
            break;
        }
    }
    let refused = answer(&mut c);
    assert_ne!(
        refused,
        Some(MsgType::Reply),
        "-ORBmaxFragments 2 must not reassemble {} fragments; it answered {refused:?}",
        pieces.len()
    );
}

// ── message_timeout ──────────────────────────────────────────────────────────

/// `-ORBmessageTimeoutMs` bounds how long a peer may stall **inside** a
/// message. A client that sends a header promising a body and then says nothing
/// must be given up on at the configured deadline rather than the compiled 30 s.
///
/// # What is asserted, and what deliberately is not
///
/// The assertion is **timing**, not the shape of the answer: the server ends
/// the stalled conversation the way it ends any unreadable one, and which of
/// its two goodbyes it picks is not what this number changes. What the number
/// changes is *when*, so the test measures when.
///
/// The bound is loose on purpose — 5 s against a configured 300 ms and a
/// compiled 30 s. A tight bound would be measuring the scheduler.
///
/// **This is the one test in this file with no control**, and the reason is
/// arithmetic rather than principle: the control is the same exchange against
/// an unconfigured ORB, and it takes 30 s to finish. The default is what the
/// 5 s bound is stated against, so the control is in the number rather than
/// in a second server.
///
/// # The client's own read timeout is part of the measurement
///
/// It is set to 20 s here rather than left at this file's 5 s default, and the
/// negative control is why. With the shared 5 s timeout the client gave up
/// before either the configured 300 ms or the compiled 30 s could be told
/// apart, so `read` returned an error at 5 s and the assertion below passed —
/// **the test stayed green with `Server::apply_orb_config` stubbed out to a
/// no-op.** A deadline that fires before the thing under test is a deadline
/// that measures itself. It now sits between the two numbers, so only the
/// configured one can satisfy the bound.
#[test]
fn orb_message_timeout_gives_up_on_a_peer_that_stalls_mid_message() {
    let quick = Served::with(&["-ORBmessageTimeoutMs", "300"]);
    let mut c = quick.client();
    c.set_read_timeout(Some(Duration::from_secs(20))).expect("read timeout");
    // A well-formed 1.2 header announcing a body that never comes.
    let mut header = request_of(64);
    header.truncate(16);
    c.write_all(&header).expect("write");

    let started = Instant::now();
    let mut sink = [0u8; 64];
    let read = c.read(&mut sink);
    let waited = started.elapsed();

    assert!(
        read.is_err() || matches!(read, Ok(n) if n == 0 || n >= 12),
        "expected the stalled connection to end, got {read:?}"
    );
    assert!(
        waited < Duration::from_secs(5),
        "-ORBmessageTimeoutMs 300 must fire long before the compiled 30 s default; waited {waited:?}"
    );
}
