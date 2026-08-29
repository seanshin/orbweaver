//! D029 §6.1's **Activation / load** row, refuted or not, over a socket.
//!
//! The row's claim is that a caller must not be able to tell *whether the
//! target is loaded right now*. This file holds **one live `Connection`**,
//! records what that caller observed, evicts the target underneath it, and
//! asks the same question again on the same connection. If the second
//! observation is the same bytes, that caller could not tell.
//!
//! It is the shape of `orbweaver-test`'s `what_a_caller_can_tell.rs` and for
//! the reason that file gives: a test that compares two *separate* runs
//! measures that two things agree, not that a caller could not tell them
//! apart, because no caller was in the room when the change happened.
//!
//! # Where the leak was, and where it was not
//!
//! The row named `moe::Router::select`. It is not there — see
//! [`ExpertService::select`](orbweaver_object::expert_service::ExpertService::select)'s
//! own documentation for the four contract arguments, of which the decisive
//! one is that a filter could not close it anyway: `select` answers at T, the
//! caller dials at T+ε, and a reference obtained any other way never went
//! through that operation at all.
//!
//! It is here: two of [`MissPolicy`]'s three variants answer
//! [`Located::Unknown`](orbweaver_object::Located::Unknown) for an OFFLOADED
//! expert, the POA turns that into `OBJECT_NOT_EXIST`, and **that** is the
//! difference a caller reads. Being a POA-level fact it holds for *any*
//! target, which is what §6's criterion asks for.
//!
//! # The control for a leak test is the leak, and it is in the tree
//!
//! [`the_refusing_miss_policies_are_the_leak`] runs the identical scenario
//! under [`MissPolicy::Refuse`] and [`MissPolicy::RefuseAndPrefetch`] and
//! **requires it to fail, naming `OBJECT_NOT_EXIST`**. So the green test is
//! evidence about a leak rather than about a switch that has stopped working:
//! if someone makes the refusing variants stop leaking, the control goes red
//! and says so. No environment variable and no commit-message transcript —
//! `cargo test` runs both directions.
//!
//! # What this does not measure
//!
//! Said here rather than left to be discovered.
//!
//! - **Time.** A demand-loaded call is slower than a resident one and a caller
//!   with a clock can tell. In this repository a load is a state transition
//!   and an opaque blob — two map writes — so the latency
//!   [`MissPolicy`]'s refusal is about is both real in a deployment and
//!   absent here. This file measures *bytes*, and says so rather than letting
//!   a green run imply a timing claim it never made.
//! - **One process, loopback, our own client.** A leak visible only to
//!   omniORB's or JacORB's client is invisible here.
//! - **The POA's forward arm.** `ExpertLocator` never answers
//!   `Located::Forward`, so [`ExpertHost`] implements no `redirect` and a
//!   locator that did forward would hit the `panic!` in `dispatch`'s
//!   `Target::Forward` arm rather than reach the wire. A panic and not a
//!   silent `OBJECT_NOT_EXIST` on purpose: the second would look exactly like
//!   the leak this file measures. Naming it because a future locator that
//!   forwards needs this host changed, not merely reconfigured.
//! - **One expert, one operation.** The reply body is a constant and the
//!   object id. A servant whose *answer* depended on residency would defeat
//!   this test, and nothing here stops one being written.
//!
//! *한 개의 살아 있는 연결이 관측을 기록하고, 그 아래에서 대상을 축출하고, 같은
//! 질문을 다시 던진다. 구멍은 `select`가 아니라 **참조**에 있었다: 거절하는 두
//! 미스 정책이 `OBJECT_NOT_EXIST`를 만들고, 그것이 호출자가 읽는 차이다. 대조군은
//! 커밋 메시지가 아니라 트리에 있으며 `cargo test`가 양방향을 모두 돈다. 재지
//! 않는 것은 **시간**이다 — 이 저장소에서 적재는 맵 쓰기 두 번이므로 거절문이
//! 말하는 지연은 배포에서는 실재하고 여기서는 없다.*

use std::collections::BTreeSet;
use std::io::Write;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use orbweaver_cdr::Encoder;
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{Request, SharedDispatch, SystemException};
use orbweaver_giop::{Connection, IiopProfile, Ior, Version};

use orbweaver_object::residency::{BatchStats, ExpertLoader, MissPolicy};
use orbweaver_object::{Lifespan, ObjectId, OrbPoa, Poa, Target, UnknownIdPolicy};

/// Long enough that a busy machine does not produce a false red, short enough
/// that a hung fixture is a test failure rather than a hung suite.
const TIMEOUT: Duration = Duration::from_secs(10);

/// `moe::Expert`'s repository id, as `expert_service` publishes it.
const EXPERT_ID: &str = "IDL:moe/Expert:1.0";

/// The one expert this file loads and evicts.
const EXPERT: &str = "expert-a";

/// The answer body's constant half. Arbitrary; what matters is that it does
/// not depend on residency, which is the whole claim.
const ANSWER: i32 = 4242;

// ─────────────────────────────────────────────────────────────────────────────
// The host: a POA, a residency machine, and one lock over both
// ─────────────────────────────────────────────────────────────────────────────

/// The POA and the loader, which move together.
///
/// One lock, for `expert_service`'s reason: the activation set and the
/// residency map are two copies of one truth, and a reader that saw one
/// without the other would see exactly the mid-mirror state the choke point
/// exists to prevent.
struct Plane {
    poa: Poa,
    loader: ExpertLoader,
}

/// A servant that routes every request through
/// [`Poa::dispatch_target`](orbweaver_object::Poa::dispatch_target).
///
/// **This is the first thing in the workspace that does.** Before it, the
/// POA's request-processing decision — `USE_SERVANT_MANAGER`, the locator, the
/// activation — was reached only from unit tests calling `dispatch_target`
/// directly. A leak in it could not have been seen by any caller because no
/// caller could reach it.
///
/// `knows` is left at its default `true` **on purpose**: the object's
/// existence is the POA's decision here, not a second one taken in front of
/// it. A `knows` that answered for expert ids would decide the same question
/// one layer earlier and this test would be measuring that instead.
struct ExpertHost {
    plane: Mutex<Plane>,
    miss: MissPolicy,
    /// How many requests were answered from the POA's active map. Server-side
    /// evidence that the demand load happened; asking the *caller* which one
    /// served it would be asking it to tell us the thing it must not be able
    /// to tell.
    served: AtomicUsize,
}

impl ExpertHost {
    /// A host with `EXPERT` registered, loaded, and activated on the POA.
    fn started(port: u16, miss: MissPolicy) -> ExpertHost {
        let orb = Orb::new();
        let poa = orb
            .create_poa("experts", EXPERT_ID)
            // Persistent, so the key carries no incarnation and the reference
            // the client holds is the reference a redeployment would mint.
            .with_lifespan(Lifespan::Persistent)
            // §15.3.8.6's `USE_SERVANT_MANAGER`. Under `Reject` the locator is
            // never consulted and no miss policy could matter.
            .with_unknown_id(UnknownIdPolicy::AskLocator)
            .publish_at("127.0.0.1", port);

        let mut loader = ExpertLoader::new();
        loader.register(EXPERT, Lifespan::Persistent).expect("a fresh id registers");
        loader.request_prefetch(EXPERT).expect("OFFLOADED → PREFETCHING");
        loader.complete_load(EXPERT).expect("PREFETCHING → RESIDENT");

        let mut plane = Plane { poa, loader };
        let done = plane.loader.reconcile(&mut plane.poa);
        assert_eq!(done.activated, vec![ObjectId::from_name(EXPERT)], "resident ⇒ activated");

        ExpertHost { plane: Mutex::new(plane), miss, served: AtomicUsize::new(0) }
    }

    /// The reference a caller holds. Minted by the POA, so it carries the
    /// POA's key and nothing about residency.
    fn reference(&self) -> Ior {
        self.plane
            .lock()
            .expect("plane")
            .poa
            .reference(&ObjectId::from_name(EXPERT))
            .expect("the POA was told where it publishes")
    }

    /// Evicts the expert and brings the POA's activation set back in line —
    /// the hidden change this file makes underneath a live caller.
    ///
    /// The window is built by literal rather than from a store: the guard's
    /// question is "may this be evicted", and a test that could not make the
    /// answer yes would be measuring the guard instead of the leak.
    fn evict(&self) {
        let mut p = self.plane.lock().expect("plane");
        let Plane { poa, loader } = &mut *p;
        let window =
            BatchStats { memory_pressure: true, cold: BTreeSet::from([EXPERT.to_owned()]) };
        loader.evict(EXPERT, &window).expect("an unpinned, idle, cold expert under pressure");
        let done = loader.reconcile(poa);
        assert_eq!(
            done.deactivated,
            vec![ObjectId::from_name(EXPERT)],
            "evicted ⇒ deactivated, or the POA would keep serving it from the active map \
             and this test would pass while measuring nothing"
        );
    }

    /// The residency the machine holds right now — read by the test, never by
    /// the caller.
    fn residency(&self) -> Option<orbweaver_object::residency::Residency> {
        self.plane.lock().expect("plane").loader.status(EXPERT)
    }
}

impl SharedDispatch for ExpertHost {
    /// `true` for every key, and D036 made saying so compulsory.
    ///
    /// The reason is the struct's own and is unchanged: the object's existence
    /// is the POA's decision here, not a second one taken in front of it. A
    /// `knows` that answered for expert ids would decide the same question one
    /// layer earlier and this test would be measuring that instead.
    ///
    /// This is the case D036 §6.3 names: the leak test stays green across that
    /// change **by construction**, because what was inherited is now stated. The
    /// evidence for D036 is the compile error that brought a reader to this
    /// line, not a test going red.
    fn knows(&self, _object_key: &[u8]) -> bool {
        true
    }

    fn dispatch(&self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        let mut p = self.plane.lock().expect("plane");
        let Plane { poa, loader } = &mut *p;
        let mut locator = loader.locator(self.miss);
        match poa.dispatch_target(&request.object_key, Some(&mut locator)) {
            Target::Active(id) => {
                self.served.fetch_add(1, Ordering::SeqCst);
                match request.operation.as_str() {
                    // The answer depends on the object and on nothing else.
                    // Every byte here is the same whether the expert was
                    // already resident or was loaded to answer this call.
                    "describe" => {
                        out.put_str(id.as_str().expect("expert ids are text"));
                        out.put_i32(ANSWER);
                        Ok(())
                    }
                    _ => Err(SystemException::bad_operation()),
                }
            }
            Target::Unknown => Err(SystemException::object_not_exist()),
            // `ExpertLocator` never forwards — placement belongs to the
            // trading service and this locator has no view of another node.
            // A locator that did would need `redirect` on this host; see the
            // module docs.
            Target::Forward(_) => panic!("ExpertLocator does not forward"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The fixture
// ─────────────────────────────────────────────────────────────────────────────

/// A bound server plus the thread serving it, stopped on drop.
struct Fixture {
    host: Arc<ExpertHost>,
    port: u16,
    stop: Arc<AtomicBool>,
    joined: Option<std::thread::JoinHandle<()>>,
}

impl Fixture {
    fn start(miss: MissPolicy) -> Fixture {
        let orb = Orb::new();
        // The bind key is not the key any request here carries: every
        // reference is minted by the POA. `knows` defaults to true, so the
        // POA sees them all.
        let server = orb.server("127.0.0.1:0", b"expert-host".to_vec()).expect("bind");
        let port = server.local_addr().expect("bound address").port();
        let host = Arc::new(ExpertHost::started(port, miss));
        let stop = Arc::new(AtomicBool::new(false));
        let serving_host = Arc::clone(&host);
        let serving_stop = Arc::clone(&stop);
        let joined = std::thread::spawn(move || {
            server
                .serve_shared(&*serving_host, move || serving_stop.load(Ordering::SeqCst))
                .expect("serve");
        });
        Fixture { host, port, stop, joined: Some(joined) }
    }

    /// A reference with this server's live port. The POA was told the port at
    /// construction, so this is what it minted.
    fn reference(&self) -> Ior {
        let ior = self.host.reference();
        assert_eq!(ior.primary().expect("one profile").port, self.port, "the POA's port");
        ior
    }
}

impl Drop for Fixture {
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

fn observe(c: &mut Connection) -> Result<Observation, String> {
    let r = c.invoke_nullary("describe").map_err(|e| e.to_string())?;
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

// ─────────────────────────────────────────────────────────────────────────────
// The scenario, run under each policy
// ─────────────────────────────────────────────────────────────────────────────

/// One caller, one reference, an eviction in between.
///
/// Returns `Err` with **what the caller could tell** — the sentence a control
/// run is required to produce, and the sentence a regression would produce.
fn what_a_caller_could_tell(miss: MissPolicy) -> Result<(), String> {
    use orbweaver_object::residency::Residency;

    let fixture = Fixture::start(miss);
    let mut caller =
        Connection::connect(&fixture.reference(), TIMEOUT).map_err(|e| format!("connect: {e}"))?;

    // Before: the expert is loaded, and the caller has no idea.
    assert_eq!(fixture.host.residency(), Some(Residency::Resident), "loaded before the call");
    let before = observe(&mut caller)?;

    // The hidden change, under the live connection.
    fixture.host.evict();
    assert_eq!(fixture.host.residency(), Some(Residency::Offloaded), "evicted, underneath it");

    // After: the same question, the same connection, the same reference.
    let after = observe(&mut caller).map_err(|e| {
        format!(
            "the caller could tell: the second call answered {e}, where the first answered a reply"
        )
    })?;

    if before != after {
        return Err(format!(
            "the caller could tell: the reply changed across the eviction\n  before: {before:?}\n  after:  {after:?}"
        ));
    }
    Ok(())
}

/// **A caller holding only a reference cannot tell the target was evicted.**
///
/// One `Connection`, kept open across an eviction that happens on the test
/// thread. The two observations are compared **whole** — status, version, byte
/// order and every byte of the body.
///
/// The server-side counter is checked afterwards rather than the caller being
/// asked which path served it: two answers from the POA's active map and one
/// of them reached through a demand load is what makes this a measurement of
/// the closure rather than of an eviction that never took.
#[test]
fn a_caller_cannot_tell_an_evicted_expert_from_a_resident_one() {
    if let Err(what) = what_a_caller_could_tell(MissPolicy::Activate) {
        panic!("{what}");
    }
}

/// The demand load actually happened — the closure is doing work, not the
/// eviction failing to.
///
/// Separate from the test above because that one must fail for exactly one
/// reason. A `reconcile` that quietly stopped deactivating would make the
/// caller unable to tell for the wrong reason, and this is what says so.
#[test]
fn the_second_call_was_served_by_a_demand_load() {
    use orbweaver_object::residency::Residency;

    let fixture = Fixture::start(MissPolicy::Activate);
    let mut caller = Connection::connect(&fixture.reference(), TIMEOUT).expect("connect");
    observe(&mut caller).expect("the first call is answered");
    fixture.host.evict();
    assert_eq!(fixture.host.residency(), Some(Residency::Offloaded), "the eviction took");
    observe(&mut caller).expect("the second call is answered");
    assert_eq!(
        fixture.host.residency(),
        Some(Residency::Resident),
        "the expert is loaded again, and it was the request that loaded it"
    );
    assert_eq!(fixture.host.served.load(Ordering::SeqCst), 2, "both calls reached a servant");
}

/// **The control is the leak.** Under the two refusing policies the caller
/// *can* tell, and this test requires it to.
///
/// If this ever passes — if `Refuse` stops producing `OBJECT_NOT_EXIST` for an
/// evicted expert — then either the leak was closed elsewhere or the fixture
/// stopped measuring, and both are things a reader has to be told about rather
/// than left to infer from a green run of the test above.
#[test]
fn the_refusing_miss_policies_are_the_leak() {
    for miss in [MissPolicy::Refuse, MissPolicy::RefuseAndPrefetch] {
        let told = what_a_caller_could_tell(miss)
            .expect_err(&format!("{miss:?} must leak; if it no longer does, this file is stale"));
        assert!(
            told.contains("OBJECT_NOT_EXIST"),
            "{miss:?}: the caller must be able to tell, by OBJECT_NOT_EXIST — got {told}"
        );
    }
}

/// An id the loader never registered is `OBJECT_NOT_EXIST` under **every**
/// policy, `Activate` included.
///
/// That is not a load state and refusing it leaks nothing: the object
/// genuinely does not exist. Pinned because the obvious way to write
/// `Activate` — load whatever is asked for — would answer for references
/// nobody ever minted, and no assertion in this file would have noticed.
#[test]
fn an_unregistered_id_is_still_unknown_under_every_policy() {
    for miss in [MissPolicy::Refuse, MissPolicy::RefuseAndPrefetch, MissPolicy::Activate] {
        let fixture = Fixture::start(miss);
        let real = fixture.reference();
        let profile = real.primary().expect("one profile");
        let stranger = Ior {
            type_id: real.type_id.clone(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: profile.host.clone(),
                port: profile.port,
                // The same POA, an id it never minted a servant for.
                object_key: b"experts/expert-nobody-registered".to_vec(),
                components: profile.components.clone(),
            }],
        };
        let mut caller = Connection::connect(&stranger, TIMEOUT).expect("connect");
        let answer = observe(&mut caller).expect_err("an unregistered id is refused");
        assert!(
            answer.contains("OBJECT_NOT_EXIST"),
            "{miss:?}: an id nobody registered is OBJECT_NOT_EXIST — got {answer}"
        );
    }
}
