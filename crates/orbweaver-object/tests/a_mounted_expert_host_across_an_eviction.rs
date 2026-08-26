//! D029 §6.1's **Activation / load** row, measured where a deployment meets
//! it: on [`ExpertHost`] — the servant that mounts
//! [`ExpertLocator`](orbweaver_object::residency::ExpertLocator) — rather than
//! on a fixture written to hold it.
//!
//! `what_a_caller_can_tell_about_load.rs` measures
//! [`MissPolicy::Activate`] in isolation: a test-private servant whose
//! `describe` answers a string and a constant, whose `knows` is left at `true`,
//! and which no deployment could run because it lives in a `tests/` file. This
//! one measures the same property through the type a deployment constructs,
//! serving `moe::Expert`'s **declared** operations, and it finds two things the
//! isolated shape could not.
//!
//! # The two things a mounted host has that a fixture does not
//!
//! **1. A `LocateRequest`.** A real host shares a server with other objects, so
//! its [`SharedDispatch::knows`] answers for its own keys and nothing else —
//! and a `knows` that consulted residency would let a caller learn the load
//! state from the *probe*, one message before spending an invocation, on the
//! one message §9.4.5 guarantees is side-effect-free. That is the same class
//! D029's `LocateRequest` subsection closed for **location** on the same day
//! this row was closed for **load**, and it is not reachable in a fixture whose
//! `knows` is the default `true`. [`the_probe_cannot_tell_either`] measures it
//! and [`a_knows_that_reads_residency_is_the_probe_leak`] is its control — a
//! second servant, in this file, whose `knows` consults the loader, shown
//! telling the caller.
//!
//! **2. `moe::Capability`'s fifth member is `Residency state`.** The contract's
//! own `describe()` *reports the load state*, so the fixture's reasoning —
//! *"the answer depends on the object and on nothing else"* — is false for the
//! declared operation even though the assertion still holds. It holds for a
//! different reason, and [`describe_tells_the_truth_and_the_truth_is_the_same`]
//! pins that reason rather than the coincidence: the demand load happens inside
//! `locate`, **before** the servant runs, so the honest report is RESIDENT both
//! times. D029 says why reporting it at all is not a leak — load state has two
//! contract homes where it is a value a caller *asks for*.
//!
//! # The controls, and what counter-movement each shows
//!
//! Every control is in this file and `cargo test` runs it; none is a commit
//! message.
//!
//! | control | what it moves | what it must show |
//! |---|---|---|
//! | [`the_refusing_policies_are_the_leak_on_a_mounted_host`] | the miss policy back to the two refusing variants | the caller can tell, **naming `OBJECT_NOT_EXIST`**, on `process` and on `describe` |
//! | [`a_knows_that_reads_residency_is_the_probe_leak`] | `knows` to consult the loader | the probe answers `Unknown` after the eviction where the host answers `Here` |
//! | [`a_delegate_that_filtered_by_residency_would_leak`] | `delegate`'s answer to the loaded-only reading | the returned reference changes across the eviction |
//!
//! # What this does not measure
//!
//! - **Time.** A demand-loaded call is slower and a caller with a clock can
//!   tell. In this repository a load is two map writes, so that latency is real
//!   in a deployment and **absent here**. This file compares bytes. Nothing in
//!   it is evidence about latency and a green run must not be read as any.
//! - **One process, loopback, our own client.** A leak visible only to
//!   omniORB's or JacORB's client is invisible here.
//! - **The forward arm.** `ExpertLocator` never forwards; [`ExpertHost`]
//!   answers `INTERNAL` there rather than `OBJECT_NOT_EXIST`, precisely so a
//!   future forwarding locator cannot be mistaken for a regression of this
//!   property. Untested because unreachable.
//!
//! *배포가 실제로 만나는 자리에서 잰다 — 픽스처가 아니라 마운트된
//! [`ExpertHost`]에서, 계약이 선언한 연산으로. 격리된 형태가 볼 수 없던 두 가지가
//! 나온다: (1) `LocateRequest` 프로브 — `knows`가 잔류를 보면 호출자는 호출을
//! 쓰기 한 메시지 전에 적재 상태를 배운다. (2) `moe::Capability`의 다섯째 멤버가
//! `Residency`이므로 계약의 `describe()`는 적재 상태를 **보고한다** — 그래도 답이
//! 같은 이유는 "아무것도 잔류에 의존하지 않아서"가 아니라 **순서** 때문이다.
//! 재지 않는 것은 시간이다.*

use std::collections::BTreeSet;
use std::io::Write;
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use orbweaver_cdr::Encoder;
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{LocateStatus, Request, SharedDispatch, SystemException};
use orbweaver_giop::{Connection, Ior, LocateResult};

use orbweaver_object::expert_host::{ExpertHost, Reported};
use orbweaver_object::expert_service::Capability;
use orbweaver_object::get_reference;
use orbweaver_object::residency::{BatchStats, MissPolicy, Residency};
use orbweaver_object::tenant_service::{Activation, CallContext};

/// Long enough that a busy machine does not produce a false red, short enough
/// that a hung fixture is a failure rather than a hung suite.
const TIMEOUT: Duration = Duration::from_secs(10);

/// The expert a caller dials.
const EXPERT: &str = "expert-a";

/// A second hosted expert, so `delegate` has something to hand back that is
/// not the object the call was addressed to.
const OTHER: &str = "expert-b";

fn reported(node: &str) -> Reported {
    Reported {
        cost: 1.5,
        latency_p99_ms: 12.0,
        load: 0.25,
        mem_footprint: 1 << 30,
        route_freq: 3.0,
        placement_node: node.to_owned(),
        contract_version: "1.0".to_owned(),
    }
}

/// The window that makes an eviction legal: pressure, and the expert cold.
///
/// Built by literal rather than from an offer store on purpose — the guard's
/// question is *may this be evicted*, and a test that could not make the answer
/// yes would be measuring the guard instead of the property.
fn pressure(id: &str) -> BatchStats {
    BatchStats { memory_pressure: true, cold: BTreeSet::from([id.to_owned()]) }
}

// ─────────────────────────────────────────────────────────────────────────────
// The fixture: the production host, on a real socket
// ─────────────────────────────────────────────────────────────────────────────

/// A bound server serving `dispatch`, stopped on drop.
struct Fixture<D: SharedDispatch + 'static> {
    servant: Arc<D>,
    port: u16,
    stop: Arc<AtomicBool>,
    joined: Option<std::thread::JoinHandle<()>>,
}

impl<D: SharedDispatch + Send + 'static> Fixture<D> {
    /// Binds a port, builds the servant with `make` (which needs the port, so
    /// the references it mints are dialable), and serves it.
    fn start(make: impl FnOnce(u16) -> D) -> Fixture<D> {
        let orb = Orb::new();
        let server = orb.server("127.0.0.1:0", b"expert-host".to_vec()).expect("bind");
        let port = server.local_addr().expect("bound address").port();
        let servant = Arc::new(make(port));
        let stop = Arc::new(AtomicBool::new(false));
        let serving = Arc::clone(&servant);
        let serving_stop = Arc::clone(&stop);
        let joined = std::thread::spawn(move || {
            server
                .serve_shared(&*serving, move || serving_stop.load(Ordering::SeqCst))
                .expect("serve");
        });
        Fixture { servant, port, stop, joined: Some(joined) }
    }
}

impl<D: SharedDispatch + 'static> Drop for Fixture<D> {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The accept loop polls the flag; a connect wakes it sooner and costs
        // nothing when it has already gone.
        let _ = TcpStream::connect(("127.0.0.1", self.port)).map(|mut s| s.write_all(&[]));
        if let Some(h) = self.joined.take() {
            let _ = h.join();
        }
    }
}

/// A started [`ExpertHost`] with two experts on it, both RESIDENT.
fn host(port: u16, miss: MissPolicy) -> ExpertHost {
    let h = ExpertHost::new("127.0.0.1", port, "experts").with_miss_policy(miss);
    h.host_expert(EXPERT, reported("node-a")).expect("a fresh id");
    h.host_expert(OTHER, reported("node-b")).expect("a fresh id");
    h
}

fn started(miss: MissPolicy) -> Fixture<ExpertHost> {
    Fixture::start(|port| host(port, miss))
}

/// The reference the caller holds, with this server's live port asserted so a
/// misconfigured host fails here rather than as a connect timeout.
fn reference<D: SharedDispatch>(f: &Fixture<D>, ior: Option<Ior>) -> Ior {
    let ior = ior.expect("the POA was told where it publishes");
    assert_eq!(ior.primary().expect("one profile").port, f.port, "the POA's port");
    ior
}

// ─────────────────────────────────────────────────────────────────────────────
// What one call let the caller see
// ─────────────────────────────────────────────────────────────────────────────

/// Everything one reply carried.
///
/// Compared **whole**. Naming the fields individually in each assertion would
/// let a field added later go uncompared, which is how a byte-identity check
/// quietly narrows.
#[derive(PartialEq, Eq, Debug)]
struct Observation {
    status: String,
    version: String,
    endian: String,
    body: Vec<u8>,
}

fn reply_of(r: orbweaver_giop::Reply) -> Result<Observation, String> {
    let status = format!("{:?}", r.status);
    let version = format!("{:?}", r.version);
    let endian = format!("{:?}", r.endian);
    let mut d = r.body().map_err(|e| e.to_string())?;
    let n = d.remaining();
    let body = d.get_bytes(n).map_err(|e| e.to_string())?.to_vec();
    Ok(Observation { status, version, endian, body })
}

/// `Activation process(in Activation x, in CallContext ctx)` — the operation
/// whose answer the contract ties to the *argument* and to nothing else
/// (PLAN-MOE §5: no data plane, so it comes back unchanged).
fn process(c: &mut Connection) -> Result<Observation, String> {
    let x =
        Activation { data: vec![7, 8, 9, 10], dtype: "f32".to_owned(), shape: "[2,2]".to_owned() };
    let ctx = CallContext { request_id: "r-1".to_owned(), trace_id: "t-1".to_owned(), step: 3 };
    let r = c
        .invoke("process", |e| {
            x.write_to(e);
            ctx.write_to(e);
        })
        .map_err(|e| e.to_string())?;
    reply_of(r)
}

/// `Capability describe()` — the operation that reports residency, by contract.
fn describe(c: &mut Connection) -> Result<Observation, String> {
    reply_of(c.invoke_nullary("describe").map_err(|e| e.to_string())?)
}

/// The `Capability need` argument of `delegate`, asking for `want`.
fn need_for(want: &str) -> Capability {
    Capability {
        id: want.to_owned(),
        cost: 0.0,
        latency_p99_ms: 0.0,
        load: 0.0,
        // The *asked-for* state. Arbitrary: nothing reads it, which is part of
        // what `a_delegate_that_filtered_by_residency_would_leak` is about.
        state: Residency::Resident,
        mem_footprint: 0,
        route_freq: 0.0,
        placement_node: String::new(),
        contract_version: "1.0".to_owned(),
    }
}

/// `Expert delegate(in Capability need)`, asking for `want`.
fn delegate(c: &mut Connection, want: &str) -> Result<Observation, String> {
    let need = need_for(want);
    reply_of(c.invoke("delegate", |e| need.write_to(e)).map_err(|e| e.to_string())?)
}

// ─────────────────────────────────────────────────────────────────────────────
// The scenario
// ─────────────────────────────────────────────────────────────────────────────

/// One caller, one reference, one connection, an eviction in between.
///
/// Returns `Err` carrying **what the caller could tell** — the sentence a
/// control run is required to produce and the sentence a regression would.
fn what_a_caller_could_tell(
    miss: MissPolicy,
    call: fn(&mut Connection) -> Result<Observation, String>,
    what: &str,
) -> Result<(), String> {
    let fixture = started(miss);
    let ior = reference(&fixture, fixture.servant.reference(EXPERT));
    let mut caller = Connection::connect(&ior, TIMEOUT).map_err(|e| format!("connect: {e}"))?;

    assert_eq!(fixture.servant.residency(EXPERT), Some(Residency::Resident), "loaded first");
    let before = call(&mut caller)?;

    fixture.servant.evict(EXPERT, &pressure(EXPERT)).expect("unpinned, idle, cold, under pressure");
    assert_eq!(
        fixture.servant.residency(EXPERT),
        Some(Residency::Offloaded),
        "the eviction took — without it this test passes while measuring nothing"
    );

    let after = call(&mut caller).map_err(|e| {
        format!("the caller could tell: {what} answered {e}, where the first call answered a reply")
    })?;

    if before != after {
        return Err(format!(
            "the caller could tell: {what}'s reply changed across the eviction\n  \
             before: {before:?}\n  after:  {after:?}"
        ));
    }
    Ok(())
}

/// **`process` is byte-identical across an eviction on a mounted host.**
///
/// The operation whose answer the contract does not tie to residency, on the
/// servant a deployment constructs, over one live connection.
#[test]
fn a_caller_cannot_tell_an_evicted_expert_from_a_resident_one() {
    if let Err(what) = what_a_caller_could_tell(MissPolicy::Activate, process, "process") {
        panic!("{what}");
    }
}

/// **`describe` is byte-identical too — and for a reason worth stating.**
///
/// `moe::Capability`'s fifth member is `Residency state`, so this reply *does*
/// report the load state; the reason it does not change is that the demand load
/// happens in `locate` before the servant runs, so the honest report is
/// RESIDENT both times. The expectation comes from
/// [`ExpertHost::describe_state`] rather than from a literal here, so a change
/// of mind about the ordering fails in one place instead of drifting.
#[test]
fn describe_tells_the_truth_and_the_truth_is_the_same() {
    if let Err(what) = what_a_caller_could_tell(MissPolicy::Activate, describe, "describe") {
        panic!("{what}");
    }

    // And the reported word is the one the ordering argument predicts.
    let fixture = started(MissPolicy::Activate);
    let ior = reference(&fixture, fixture.servant.reference(EXPERT));
    let mut caller = Connection::connect(&ior, TIMEOUT).expect("connect");
    let expected =
        ExpertHost::describe_state(MissPolicy::Activate).expect("Activate reaches the servant");

    let read = |c: &mut Connection| {
        let r = c.invoke_nullary("describe").expect("answered");
        let mut d = r.body().expect("a body");
        Capability::read_from(&mut d).expect("nine members").state
    };
    assert_eq!(read(&mut caller), expected, "before the eviction");
    fixture.servant.evict(EXPERT, &pressure(EXPERT)).expect("evicted");
    assert_eq!(read(&mut caller), expected, "after it, because the load ran first");
}

/// The demand load actually happened — the closure is doing work rather than
/// the eviction failing to.
///
/// Separate from the tests above so each fails for exactly one reason. A
/// `reconcile` that quietly stopped deactivating would make the caller unable
/// to tell for the wrong reason, and this is what says so.
#[test]
fn the_second_call_was_served_by_a_demand_load() {
    let fixture = started(MissPolicy::Activate);
    let ior = reference(&fixture, fixture.servant.reference(EXPERT));
    let mut caller = Connection::connect(&ior, TIMEOUT).expect("connect");
    process(&mut caller).expect("the first call is answered");
    fixture.servant.evict(EXPERT, &pressure(EXPERT)).expect("evicted");
    assert_eq!(fixture.servant.residency(EXPERT), Some(Residency::Offloaded), "the eviction took");
    process(&mut caller).expect("the second call is answered");
    assert_eq!(
        fixture.servant.residency(EXPERT),
        Some(Residency::Resident),
        "loaded again, and it was the request that loaded it"
    );
}

/// **The §9.4.5 probe cannot tell either.**
///
/// The message a caller can send *without* invoking anything. If `knows`
/// consulted the loader, this is where the load state would escape — a caller
/// polling `LocateRequest` would read residency for free, and the closure on
/// the request path would buy nothing.
#[test]
fn the_probe_cannot_tell_either() {
    let fixture = started(MissPolicy::Activate);
    let ior = reference(&fixture, fixture.servant.reference(EXPERT));
    let mut caller = Connection::connect(&ior, TIMEOUT).expect("connect");

    let before = caller.locate().expect("a locate reply");
    assert!(matches!(before, LocateResult::Here), "resident: {before:?}");
    fixture.servant.evict(EXPERT, &pressure(EXPERT)).expect("evicted");
    let after = caller.locate().expect("a locate reply");
    assert!(
        matches!(after, LocateResult::Here),
        "the caller could tell from the probe alone: {after:?}"
    );
}

/// **A reference obtained from `delegate` cannot tell either.**
///
/// D029's row makes this the decisive case against filtering anywhere else: a
/// reference handed over by `Expert::delegate` never went through
/// `Router::select`, so no filter there could ever have covered it. Here the
/// delegated expert is evicted *before* the `delegate` call, and the reference
/// that comes back is dialled and answers.
#[test]
fn a_delegated_reference_does_not_reveal_the_delegates_load_state() {
    let fixture = started(MissPolicy::Activate);
    let ior = reference(&fixture, fixture.servant.reference(EXPERT));
    let mut caller = Connection::connect(&ior, TIMEOUT).expect("connect");

    let resident = delegate(&mut caller, OTHER).expect("delegate answers");
    fixture.servant.evict(OTHER, &pressure(OTHER)).expect("the delegate is evicted");
    let offloaded = delegate(&mut caller, OTHER).expect("delegate answers");
    assert_eq!(resident, offloaded, "the caller could tell which experts are loaded from delegate");

    // And the reference it handed back is live: dialled, and invoked, while
    // the expert it names is still OFFLOADED. Decoded from the reply's own
    // decoder rather than from `offloaded.body` — the body's byte order is the
    // connection's, and re-reading it under an assumed one is a bug in the
    // test, not a property of the reply.
    assert_eq!(fixture.servant.residency(OTHER), Some(Residency::Offloaded), "still evicted");
    let reply = caller.invoke("delegate", |e| need_for(OTHER).write_to(e)).expect("delegate");
    let mut d = reply.body().expect("a body");
    let handed = get_reference(&mut d).expect("an inline object reference").expect("not nil");
    let mut second = Connection::connect(&handed, TIMEOUT).expect("connect to the delegate");
    process(&mut second).expect("the delegated, evicted expert answers");
}

// ─────────────────────────────────────────────────────────────────────────────
// The controls. Each moves one thing and requires the leak to reappear.
// ─────────────────────────────────────────────────────────────────────────────

/// **Control 1 — the leak is the refusal.** Under the two refusing policies the
/// caller *can* tell, on both operations, and this requires it to.
///
/// If it ever passes, either the leak was closed elsewhere or this file stopped
/// measuring, and both are things a reader must be told rather than left to
/// infer from a green run above.
#[test]
fn the_refusing_policies_are_the_leak_on_a_mounted_host() {
    for miss in [MissPolicy::Refuse, MissPolicy::RefuseAndPrefetch] {
        for (call, what) in
            [(process as fn(&mut Connection) -> _, "process"), (describe, "describe")]
        {
            let told = what_a_caller_could_tell(miss, call, what).expect_err(&format!(
                "{miss:?} on {what} must leak; if it no longer does, this file is stale"
            ));
            assert!(
                told.contains("OBJECT_NOT_EXIST"),
                "{miss:?} on {what}: the caller must be able to tell, by OBJECT_NOT_EXIST — \
                 got {told}"
            );
        }
    }
}

/// A host whose `knows` reads the loader — the probe leak, put back.
///
/// This is [`ExpertHost`] in every respect except the one decision under test,
/// which is what makes it a control rather than a second implementation:
/// `dispatch` and `locate` are delegated verbatim.
struct KnowsReadsResidency(ExpertHost);

impl SharedDispatch for KnowsReadsResidency {
    /// The counter-movement: residency, consulted one message too early.
    fn knows(&self, object_key: &[u8]) -> bool {
        let hosted = self.0.hosted();
        let key = String::from_utf8_lossy(object_key).into_owned();
        hosted.iter().any(|id| {
            key.ends_with(id) && matches!(self.0.residency(id), Some(Residency::Resident))
        })
    }

    fn locate(&self, object_key: &[u8]) -> LocateStatus {
        if self.knows(object_key) { LocateStatus::ObjectHere } else { LocateStatus::UnknownObject }
    }

    fn dispatch(&self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        self.0.dispatch(request, out)
    }
}

/// **Control 2 — the probe leak, reproduced.**
///
/// The same scenario as [`the_probe_cannot_tell_either`] against a host whose
/// `knows` consults the loader. It must answer `Unknown` after the eviction:
/// that is the caller learning the load state without invoking anything, and
/// the reason [`ExpertHost::knows`] is residency-independent.
#[test]
fn a_knows_that_reads_residency_is_the_probe_leak() {
    let fixture = Fixture::start(|port| KnowsReadsResidency(host(port, MissPolicy::Activate)));
    let ior = reference(&fixture, fixture.servant.0.reference(EXPERT));
    let mut caller = Connection::connect(&ior, TIMEOUT).expect("connect");

    assert!(matches!(caller.locate().expect("probe"), LocateResult::Here), "resident");
    fixture.servant.0.evict(EXPERT, &pressure(EXPERT)).expect("evicted");
    let after = caller.locate().expect("probe");
    assert!(
        matches!(after, LocateResult::Unknown),
        "the control must reproduce the probe leak; got {after:?}"
    );
}

/// **Control 3 — a `delegate` that filtered by residency.**
///
/// Written as the assertion rather than as a second servant, because the
/// filtering `delegate` is a *removal*: the reference the caller gets would be
/// nil for an offloaded expert and non-nil for a resident one. This shows those
/// two replies are distinguishable, which is why
/// [`a_delegated_reference_does_not_reveal_the_delegates_load_state`] is a real
/// assertion and not a tautology about two identical `None`s.
#[test]
fn a_delegate_that_filtered_by_residency_would_leak() {
    let fixture = started(MissPolicy::Activate);
    let ior = reference(&fixture, fixture.servant.reference(EXPERT));
    let mut caller = Connection::connect(&ior, TIMEOUT).expect("connect");

    let hosted = delegate(&mut caller, OTHER).expect("delegate answers");
    let unhosted = delegate(&mut caller, "expert-nobody-hosts").expect("delegate answers");
    assert_ne!(
        hosted, unhosted,
        "a nil reference and a real one must be distinguishable on the wire, or the test that \
         compares two `delegate` replies is comparing nothing"
    );
}

/// An id this host was never given is `OBJECT_NOT_EXIST` under **every**
/// policy, `Activate` included — and its probe says `Unknown`.
///
/// That is not a load state and refusing it leaks nothing: the object genuinely
/// does not exist. Pinned because the obvious way to write `Activate` — load
/// whatever is asked for — would answer for references nobody ever minted, and
/// because it is the case a *failed demand load* is currently conflated with.
#[test]
fn an_unhosted_id_is_refused_under_every_policy() {
    for miss in [MissPolicy::Refuse, MissPolicy::RefuseAndPrefetch, MissPolicy::Activate] {
        let fixture = started(miss);
        let real = reference(&fixture, fixture.servant.reference(EXPERT));
        let profile = real.primary().expect("one profile");
        let mut stranger = real.clone();
        stranger.profiles[0].object_key = b"experts/expert-nobody-hosts".to_vec();
        assert_eq!(stranger.profiles[0].port, profile.port, "the same server");

        let mut caller = Connection::connect(&stranger, TIMEOUT).expect("connect");
        let told = process(&mut caller).expect_err("an unhosted id is refused");
        assert!(
            told.contains("OBJECT_NOT_EXIST"),
            "{miss:?}: an id nobody hosts is OBJECT_NOT_EXIST — got {told}"
        );
        assert!(
            matches!(caller.locate().expect("probe"), LocateResult::Unknown),
            "{miss:?}: and the probe agrees"
        );
    }
}
