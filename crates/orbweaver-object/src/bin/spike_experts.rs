//! The MoE control plane end to end, over a real socket.
//!
//! `moe::ExpertRegistry` and `moe::ExpertLoader` (corpus/golden/22) served by
//! [`ExpertService`], called by our own client, with the control loop running
//! *between* the windows — three registrations, a heartbeat, a oneway
//! prefetch, a guarded eviction, and one §6 policy application whose decision
//! list is pinned here rather than merely printed.
//!
//! # Why the control loop is not a second thread
//!
//! The serving windows are a control-flow choice, not a limit of the server.
//! They were close to being one: until stream E's second batch the servant was
//! borrowed mutably for the window, so a control loop could not run beside the
//! wire even in principle. It can now — the service is shared by reference and
//! locks its own state — and the windows stay anyway, because what they buy is
//! legibility: each phase's effects are attributable to that phase. Instead
//! the spike opens a serving
//! window, does the wire work, closes it, and runs the out-of-band control
//! steps with the servant back in hand. That is not a workaround: §5 puts
//! residency transitions at *batch and statistics period*, and "between
//! windows" is exactly when a control loop is supposed to run.
//!
//! The serving windows use `std::thread::scope`, so the serving thread
//! borrows the servant for the length of the window and gives it back at the
//! end — the type system enforces that no control step runs while the wire is
//! open, which is the property the design is claiming.
//!
//! Usage: `spike-experts [registry-ior [loader-ior [router-ior]]] [--hold]`
//!
//! Defaults are `spikes/moe-registry.ior`, `spikes/moe-loader.ior` and
//! `spikes/moe-router.ior`. The three references are published and `READY` is
//! printed **before** the checks run, so a harness can wait on a file the way
//! it does for `spike-names`.
//!
//! With `--hold` the serving window stays open after the checks instead of
//! closing with the last one — the same shape `spike-names`, `spike-events`
//! and `spike-ifr` have, and the thing whose absence made
//! `SERVICES-COVERAGE.md` §9 build a separate holder crate to address this
//! servant from outside. Held state is what the checks left: four experts
//! registered, `expert-code` OFFLOADED, `expert-math` ACTIVE, `expert-vision`
//! RESIDENT and pinned, the first three carrying an out-of-band specialization
//! so `Router::select` has an answerable question to be asked, and
//! `expert-math`/`expert-math-b` measured through the v1.1 path (windows 4
//! and 5) so `ORDER BY latency_p50` has a complete answer. Stopped by killing
//! the process; there is no remote shutdown.
//!
//! # Windows 4 and 5 — the contract's v1.1 half
//!
//! D010 A2: *a latency-ordered router prefers the experts nobody has
//! measured.* Before those windows every offer arrived through v1.0 and has
//! no `latency_p50`; the spike asks the engine for the fastest maths expert
//! and checks that it **refuses** — sets the unmeasured one aside rather than
//! ranking it. Then `register_measured` and `heartbeat_measured` (moe v1.1,
//! `MeasuredCapability`) carry a measurement over the wire, and the same
//! question gets a complete answer, and the answer is the one that was
//! measured fastest. A v1.0 `heartbeat` in between must not erase it.

//! # Where this fixture's population comes from
//!
//! D026 §4, and the corollary is the load-bearing half: *a fixture may still
//! invent a population, and says so.* So this one says so.
//!
//! **From `corpus/state/moe-estate.json`:** the node these experts report
//! (`reported_placement`, deployment `control-plane`) and the capability
//! vocabulary their specializations are drawn from. Those are the two facts
//! `spike_tenants` and `spikes/seed_trading_client.py` also touch, and the
//! ones the seed batch found disagreeing.
//!
//! **Invented here, deliberately:** the four experts themselves — their names,
//! memory footprints, loads, latencies and the v1.1 measurements. No other
//! fixture uses them, and what they exist for is *paths* rather than a world:
//! the footprints are chosen so the §6 policy pass evicts exactly one expert,
//! and the two p50s so the ordered router has a wrong answer available to it.
//! Dragging them into a shared file to satisfy a rule would make the seed the
//! only population, which D026 §3 forbids and which `wire-fuzz` and the
//! property tests exist to prevent.
//!
//! **The cost is invented too, and that is a decision.** `moe::Capability.cost`
//! here is what an expert reports *about itself*; the tenancy seed's
//! `capabilities[].cost` is what a tenant's manifest declares. They are the
//! same split as the two node domains — self-reported versus operator-declared
//! — so joining them would be the merge `corpus/state/README.md` argues
//! against, one field over.
//!
//! The loader is reached the same way `spike_tenants` reaches it, and for the
//! same reason; that fixture's *Where the loader lives* has the argument.
#[allow(dead_code)]
#[path = "../../../orbweaver-test/src/state.rs"]
mod state;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use state::MoeEstate;

use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{BAD_OPERATION, Server};
use orbweaver_giop::{Connection, Error, IiopProfile, Ior, Version};
use orbweaver_object::expert_service::{
    BAD_PARAM, Capability, Constraints, EXPERT_ID, ExpertService, GateSignal, MOE_BASE_KEY,
    MeasuredCapability, NO_IMPLEMENT, NO_PERMISSION, TRANSIENT, residency_from_ordinal,
};
use orbweaver_object::get_reference;
use orbweaver_object::residency::Applied;
use orbweaver_trading::policy::{Decision, LoadingPolicy};
use orbweaver_trading::query::Query;
use orbweaver_trading::{FREQ_SCALE, Residency};

const T: Duration = Duration::from_secs(5);

/// Failures are counted, never inferred from a clean exit: an unmeasured
/// check is a failure, so anything that does not report `ok` increments this.
struct Report {
    failures: u32,
}

impl Report {
    fn check(&mut self, ok: bool, what: &str) {
        if ok {
            println!("  ok    {what}");
        } else {
            self.failures += 1;
            println!("  FAIL  {what}");
        }
    }

    fn eq<T: PartialEq + std::fmt::Debug>(&mut self, got: T, want: T, what: &str) {
        if got == want {
            println!("  ok    {what}");
        } else {
            self.failures += 1;
            println!("  FAIL  {what}: got {got:?}, wanted {want:?}");
        }
    }
}

/// A reference to an expert, as the expert itself would publish one. Nothing
/// dials these — the registry stores them so a router can hand them back.
fn expert_ref(name: &str) -> Ior {
    Ior {
        type_id: EXPERT_ID.to_owned(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "192.0.2.7".into(),
            port: 4242,
            object_key: name.as_bytes().to_vec(),
            components: Vec::new(),
        }],
    }
}

/// A `moe::Capability` as an expert of this deployment would report one.
///
/// `node` comes from the seed's **reported placement** domain — the one this
/// fixture shares with anything else. Everything else here is this fixture's
/// own; see the module docs for which and why.
fn capability(id: &str, mem: u64, load: f32, node: &str) -> Capability {
    Capability {
        id: id.to_owned(),
        cost: 1.5,
        latency_p99_ms: 180.0,
        load,
        // A report, not an instruction: the loader answers OFFLOADED whatever
        // an expert claims here, and the spike checks that it does.
        state: Residency::Resident,
        mem_footprint: mem,
        // Likewise invented, and likewise ignored: the store owns routing
        // history and a heartbeat cannot rewrite it.
        route_freq: 99.0,
        // Self-reported, and nothing validates it — domain B. The value is
        // the seed's so the three fixtures that model placement stop each
        // holding a private spelling of it; the *absence* of a check against
        // the operator's declared estate is the decision, not an omission.
        placement_node: node.to_owned(),
        contract_version: "moe/1.0".into(),
    }
}

fn status(c: &mut Connection, id: &str) -> Result<Residency, Error> {
    let owned = id.to_owned();
    let ordinal = c.invoke("status", move |e| e.put_str(&owned))?.body()?.get_u32()?;
    residency_from_ordinal(ordinal)
        .ok_or(Error::Decode("status answered an ordinal that is not a moe::Residency"))
}

fn exception_id(result: Result<orbweaver_giop::Reply, Error>) -> String {
    match result {
        Err(Error::SystemException { id, .. }) => id,
        Err(other) => format!("<{other}>"),
        Ok(_) => "<no exception>".to_owned(),
    }
}

/// Serves `svc` for the length of `window`, then hands it back.
///
/// The scoped thread used to *borrow the servant mutably* for the window, so
/// the compiler stopped a control-loop step running while the wire was open.
/// Concurrent dispatch (stream E) removed that borrow — the servant is shared
/// by reference now — so the exclusion is no longer free. It is preserved by
/// shape instead: `window` is handed only the report, never the service, so
/// the only thing that can reach `svc` during the window is a wire call.
/// Widening that closure to take the service would be a real change in what
/// this spike measures, not a convenience.
fn serve_window<F>(server: &Server, svc: &ExpertService, probe: &Ior, r: &mut Report, window: F)
where
    F: FnOnce(&mut Report),
{
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let serving = scope.spawn(|| {
            server.serve_shared(svc, || stop.load(Ordering::SeqCst)).expect("the server ran");
        });
        window(r);
        // The flag goes up after the window's last connection has closed, so
        // the serve loop is blocked in accept by now; one throwaway
        // connection is what wakes it to notice.
        stop.store(true, Ordering::SeqCst);
        drop(Connection::connect(probe, T));
        serving.join().expect("the serving thread ended cleanly");
    });
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hold = args.iter().any(|a| a == "--hold");
    let paths: Vec<&str> =
        args.iter().filter(|a| !a.starts_with("--")).map(String::as_str).collect();
    let out = [
        paths.first().copied().unwrap_or("spikes/moe-registry.ior"),
        paths.get(1).copied().unwrap_or("spikes/moe-loader.ior"),
        paths.get(2).copied().unwrap_or("spikes/moe-router.ior"),
    ];

    match run(&out, hold) {
        Ok(0) => {
            println!("\nexpert-service: PASS");
            std::process::ExitCode::SUCCESS
        }
        Ok(failures) => {
            println!("\nexpert-service: FAIL — {failures} check(s) failed");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            println!("\nexpert-service: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// The `ExpertSeq` a `Router::select` reply carries: a length, then that many
/// object references. Decoded here rather than trusted, because "select
/// answered" and "select answered a sequence of experts" are different claims.
fn select(c: &mut Connection, gate: GateSignal, qos: Constraints) -> Result<Vec<Ior>, Error> {
    let reply = c.invoke("select", move |e| {
        gate.write_to(e);
        qos.write_to(e);
    })?;
    let mut body = reply.body()?;
    let n = body.get_u32()?;
    let n = body.validate_count(n, 4)?;
    let mut experts = Vec::with_capacity(n);
    for _ in 0..n {
        experts.push(get_reference(&mut body)?.ok_or(Error::Decode("a nil expert reference"))?);
    }
    Ok(experts)
}

/// The object keys of a selection, which is what a reader can check by eye.
fn names(experts: &[Ior]) -> Vec<String> {
    experts
        .iter()
        .map(|i| {
            i.primary()
                .map(|p| String::from_utf8_lossy(&p.object_key).into_owned())
                .unwrap_or_else(|_| "<no profile>".to_owned())
        })
        .collect()
}

/// The listening server and the servant it serves, built together — the one
/// place either identity is chosen.
///
/// They come back as a pair because the defect they replaced was a pair that
/// disagreed (D028 §1): the server was bound to `b"MoE/registry"` on one line
/// and the servant handed the base `b"MoE"` on the next, from which it derives
/// `MoE/registry` — the same bytes arrived at twice, by two routes, with
/// nothing able to notice. A caller that cannot obtain one half without the
/// other cannot re-open that gap, and the gate below builds this real pair
/// rather than retyping either half of it.
fn plane(policy: LoadingPolicy, cold_below: u64) -> Result<(Server, ExpertService), Error> {
    let server = Orb::new().server("127.0.0.1:0", MOE_BASE_KEY.to_vec())?;
    let port = server.local_addr()?.port();
    let svc = ExpertService::new("127.0.0.1", port, MOE_BASE_KEY, policy, cold_below);
    Ok((server, svc))
}

fn run(out: &[&str; 3], hold: bool) -> Result<u32, Box<dyn std::error::Error>> {
    // §6's knobs, and the cold threshold in FREQ_SCALE units: "fewer than two
    // hits' worth of history left".
    let policy = LoadingPolicy { affinity_weight: 1, low_watermark: 100, high_watermark: 400 };
    let cold_below = 2 * FREQ_SCALE;

    // The shared half of the population, loaded rather than invented: the node
    // this deployment's experts report about themselves, and the vocabulary
    // their specializations come from. The rest is this fixture's own and the
    // module docs say which.
    let estate = MoeEstate::load()?;
    let node = estate
        .reported_placement
        .node_for("control-plane")
        .ok_or("the seed states a reported-placement node for the `control-plane` deployment")?
        .to_owned();
    // The three words this fixture declares out of band, checked against the
    // seed's vocabulary before anything is served. Refused rather than
    // reported as an `ok` line: it costs no output, so it cannot be read as
    // coverage, and a word the vocabulary does not know would otherwise reach
    // the offer store and make `specializations_come_from_the_stated_vocabulary`
    // a claim about the seed only.
    let specializations = ["code", "math", "vision"];
    for s in specializations {
        if !estate.capability_vocabulary.iter().any(|w| w == s) {
            return Err(format!(
                "this fixture declares the specialization `{s}`, which is not in the seed's \
                 capability vocabulary {:?}. One of the two moved without the other.",
                estate.capability_vocabulary
            )
            .into());
        }
    }

    let (server, svc) = plane(policy, cold_below)?;
    let registry_ior = svc.registry_ior();
    let loader_ior = svc.loader_ior();
    let router_ior = svc.router_ior();
    let mut r = Report { failures: 0 };

    println!("serving  {}", server.local_addr()?);
    println!(
        "registry {} key {:?}",
        registry_ior.type_id,
        String::from_utf8_lossy(svc.registry_key())
    );
    println!("loader   {} key {:?}", loader_ior.type_id, String::from_utf8_lossy(svc.loader_key()));
    println!("router   {} key {:?}", router_ior.type_id, String::from_utf8_lossy(svc.router_key()));
    println!(
        "policy   affinity {}, watermarks {}/{}, cold below {cold_below}",
        policy.affinity_weight, policy.low_watermark, policy.high_watermark
    );
    // The one line this migration adds, and the only difference from the
    // pre-migration output. It is here because a seeded value that reaches no
    // output cannot be shown reaching anything: `placement_node` is reported
    // into the offer store and printed by nothing, so changing it in the seed
    // moved no counter and the negative control came back GREEN — a control
    // that cannot fail is not a control. Now it can.
    println!(
        "placement {node} (reported — the seed's `control-plane` deployment, domain B; nothing \
         checks it against a declared estate, by decision)"
    );
    // Published before the checks run, so a harness waiting on a file is
    // waiting on something that exists as early as it can.
    for (path, ior) in out.iter().zip([&registry_ior, &loader_ior, &router_ior]) {
        std::fs::write(path, ior.to_stringified()?)?;
        println!("IOR written to {path}");
    }
    println!("READY");

    // ── window 1: registration and the loading requests ─────────────────────
    println!("\nwindow 1 — the wire is open");
    serve_window(&server, &svc, &registry_ior, &mut r, |r| {
        let mut reg = match Connection::connect(&registry_ior, T) {
            Ok(c) => c,
            Err(e) => {
                r.check(false, &format!("connect to the registry: {e}"));
                return;
            }
        };
        // Three experts, registered with the footprints they start at.
        for (name, mem) in [("expert-code", 30u64), ("expert-math", 200), ("expert-vision", 100)] {
            let reference = expert_ref(name);
            let cap = capability(name, mem, 0.25, &node);
            let ok = reg
                .invoke("register_expert", |e| {
                    reference.write_to(e).expect("an IOR always encodes");
                    cap.write_to(e);
                })
                .is_ok();
            r.check(ok, &format!("register_expert({name}, {mem} bytes)"));
        }
        // Re-announcing an existing expert is heartbeat's job, not register's.
        let reference = expert_ref("expert-code");
        let cap = capability("expert-code", 30, 0.25, &node);
        let got = exception_id(reg.invoke("register_expert", |e| {
            reference.write_to(e).expect("an IOR always encodes");
            cap.write_to(e);
        }));
        r.eq(got.as_str(), BAD_PARAM, "a duplicate register_expert is BAD_PARAM");

        // The heartbeat: expert-code now occupies twice what it registered,
        // and this is the number the policy will decide on later.
        let reference = expert_ref("expert-code");
        let cap = capability("expert-code", 60, 0.9, &node);
        let ok = reg
            .invoke("heartbeat", |e| {
                reference.write_to(e).expect("an IOR always encodes");
                cap.write_to(e);
            })
            .is_ok();
        r.check(ok, "heartbeat(expert-code, 60 bytes, load 0.9)");
        drop(reg);

        let mut ldr = match Connection::connect(&loader_ior, T) {
            Ok(c) => c,
            Err(e) => {
                r.check(false, &format!("connect to the loader: {e}"));
                return;
            }
        };
        // Every expert is OFFLOADED however it described itself.
        for name in ["expert-code", "expert-math", "expert-vision"] {
            match status(&mut ldr, name) {
                Ok(s) => r.eq(s, Residency::Offloaded, &format!("status({name}) before any load")),
                Err(e) => r.check(false, &format!("status({name}): {e}")),
            }
        }

        // The oneway. If the server answered it, the next invoke would read
        // that reply, see the wrong request id and fail as desynchronised —
        // so the status call below is what proves no reply was written.
        for name in ["expert-code", "expert-math", "expert-vision"] {
            let owned = name.to_owned();
            r.check(
                ldr.invoke_oneway("prefetch", move |e| e.put_str(&owned)).is_ok(),
                &format!("prefetch({name}) — oneway, no reply awaited"),
            );
        }
        match status(&mut ldr, "expert-code") {
            Ok(s) => {
                r.eq(s, Residency::Prefetching, "status(expert-code) after the oneway prefetch")
            }
            Err(e) => r.check(false, &format!("the oneway left a reply on the wire: {e}")),
        }

        // Refusals: an id nobody registered gets no plausible default.
        let got = exception_id(ldr.invoke("status", |e| e.put_str("expert-ghost")));
        r.eq(got.as_str(), BAD_PARAM, "status(expert-ghost) is BAD_PARAM, not OFFLOADED");
        let got = exception_id(ldr.invoke("pin", |e| e.put_str("expert-ghost")));
        r.eq(got.as_str(), BAD_PARAM, "pin(expert-ghost) is BAD_PARAM");
    });

    // ── between the windows: what no wire operation may do ──────────────────
    println!("\nbetween windows — the control loop");
    for name in ["expert-code", "expert-math", "expert-vision"] {
        match svc.complete_load(name) {
            Ok(s) => r.eq(s, Residency::Resident, &format!("the copy for {name} finished")),
            Err(e) => r.check(false, &format!("complete_load({name}): {e}")),
        }
    }
    // A call arrives on expert-math: ACTIVE, and inflight for the guard.
    match svc.begin_call("expert-math") {
        Ok(s) => r.eq(s, Residency::Active, "a call began on expert-math"),
        Err(e) => r.check(false, &format!("begin_call: {e}")),
    }
    // Routing telemetry, which is in-process by design (§5: no per-call wire
    // hook). One hit each, so all three are cold (16 < 32) and the eviction
    // refusals below are about the guard's *later* conditions rather than
    // about coldness — §5 reports the first unmet condition, so a hot expert
    // would mask everything after it.
    for name in ["expert-code", "expert-math", "expert-vision"] {
        svc.record_hit(name);
    }
    // The snapshot the guard reads. The loader cannot know it and the store
    // cannot either; it arrives from outside, per window.
    svc.observe_free_memory(50);
    r.eq(
        svc.with_store(|s| s.get("expert-code").map(|o| o.mem_footprint)),
        Some(60),
        "the heartbeat's footprint is in the offer store",
    );
    r.eq(
        svc.with_store(|s| s.get("expert-code").map(|o| o.route_freq)),
        Some(FREQ_SCALE),
        "…and the store's routing counter, not the 99.0 the expert claimed",
    );
    r.check(
        svc.reference_for("expert-math") == Some(expert_ref("expert-math")),
        "the registered Expert reference is held for a router to hand back",
    );

    // ── window 2: the guarded eviction ──────────────────────────────────────
    println!("\nwindow 2 — the wire is open again");
    let loader_ior2 = loader_ior.clone();
    let router_ior2 = router_ior.clone();
    serve_window(&server, &svc, &registry_ior, &mut r, |r| {
        let mut ldr = match Connection::connect(&loader_ior2, T) {
            Ok(c) => c,
            Err(e) => {
                r.check(false, &format!("connect to the loader: {e}"));
                return;
            }
        };
        // expert-math is under pressure (free 50 < 100), cold (16 < 32) and
        // unpinned — the guard's first three conditions hold — but a call is
        // inflight, so eviction is refused and nothing moves.
        let got = exception_id(ldr.invoke("evict", |e| e.put_str("expert-math")));
        r.eq(got.as_str(), TRANSIENT, "evict(expert-math) with a call inflight is TRANSIENT");
        match status(&mut ldr, "expert-math") {
            Ok(s) => r.eq(s, Residency::Active, "…and the status is unchanged"),
            Err(e) => r.check(false, &format!("status: {e}")),
        }

        // A pin, and the refusal it produces: NO_PERMISSION rather than
        // TRANSIENT, because a pin does not lapse when the window closes and
        // a caller told "try again" would retry for ever.
        r.check(ldr.invoke("pin", |e| e.put_str("expert-vision")).is_ok(), "pin(expert-vision)");
        let got = exception_id(ldr.invoke("evict", |e| e.put_str("expert-vision")));
        r.eq(got.as_str(), NO_PERMISSION, "evict(expert-vision) while pinned is NO_PERMISSION");
        match status(&mut ldr, "expert-vision") {
            Ok(s) => r.eq(s, Residency::Resident, "…and that status is unchanged too"),
            Err(e) => r.check(false, &format!("status: {e}")),
        }
        drop(ldr);

        // ── moe::Router::select, on its own object ──
        let mut rtr = match Connection::connect(&router_ior2, T) {
            Ok(c) => c,
            Err(e) => {
                r.check(false, &format!("connect to the router: {e}"));
                return;
            }
        };
        // Unconstrained on the one property the contract cannot carry: every
        // remaining field is answerable, so this is the form of the question
        // that works today. All three have one hit, so they tie on route_freq
        // and the engine's ascending-id tie-break decides.
        let open = Constraints { required: String::new(), max_latency_ms: 1000.0, max_cost: 100.0 };
        match select(&mut rtr, GateSignal { affinity: vec![7; 8], top_k: 2 }, open.clone()) {
            Ok(experts) => r.eq(
                names(&experts),
                vec!["expert-code".to_owned(), "expert-math".to_owned()],
                "select(top_k=2) — route_freq DESC, ties by id, truncated",
            ),
            Err(e) => r.check(false, &format!("select: {e}")),
        }
        // A constraint on a property no offer carries: refused whole. The
        // failure this replaces would have been an ExpertSeq of length 0,
        // which reads as "no expert does maths".
        let math =
            Constraints { required: "math".to_owned(), max_latency_ms: 1000.0, max_cost: 100.0 };
        let got = exception_id(rtr.invoke("select", {
            let math = math.clone();
            move |e| {
                GateSignal { affinity: Vec::new(), top_k: 4 }.write_to(e);
                math.write_to(e);
            }
        }));
        r.eq(
            got.as_str(),
            NO_IMPLEMENT,
            "select(required='math') with no specialization on file is NO_IMPLEMENT, not empty",
        );
        // The other half of the interface, refused with the PLAN-MOE §4.6
        // reason written in the servant — and since 2026-08-18 the wire says
        // which kind of refusal it is. D006 approved the exclusion on
        // 2026-08-14 while this answered `BAD_OPERATION`, "no such operation",
        // so a client could not tell the decision from a servant that had
        // forgotten. A name the interface does not declare still gets
        // `BAD_OPERATION`, which is what makes the pair worth checking.
        let got = exception_id(rtr.invoke("dispatch", |e| e.put_octet_seq(&[])));
        r.eq(
            got.as_str(),
            NO_IMPLEMENT,
            "Router::dispatch is declared and deliberately not served",
        );
        let got = exception_id(rtr.invoke("no_such_operation", |e| e.put_octet_seq(&[])));
        r.eq(got.as_str(), BAD_OPERATION, "a name moe::Router does not declare");
    });

    // ── the policy application, pinned ──────────────────────────────────────
    println!("\nafter window 2 — one §6 batch window");
    let applied = svc.apply_policy(50);
    for a in &applied {
        println!("        {:?} -> {:?}", a.decision, a.outcome);
    }
    r.eq(
        applied,
        vec![Applied {
            decision: Decision::Evict("expert-code".to_owned()),
            outcome: Ok(Residency::Offloaded),
        }],
        "the decision list and its outcomes",
    );
    // Why that list and no other: expert-math is ACTIVE and never a
    // candidate, expert-vision is pinned in both copies, and expert-code —
    // cold, resident, unpinned — releases the 60 bytes the *heartbeat* said
    // it holds, which reaches the low watermark and ends the pass. (That the
    // heartbeat's footprint is what ends it is asserted properly in
    // `the_heartbeat_is_what_changes_the_decision`, where both footprints can
    // be run; here it is one number in one run and is not claimed as more.)
    r.eq(
        svc.with_loader(|l| l.status("expert-code")),
        Some(Residency::Offloaded),
        "expert-code offloaded",
    );
    r.eq(
        svc.with_loader(|l| l.status("expert-math")),
        Some(Residency::Active),
        "expert-math still ACTIVE",
    );
    r.eq(
        svc.with_loader(|l| l.status("expert-vision")),
        Some(Residency::Resident),
        "expert-vision still RESIDENT",
    );
    r.eq(
        svc.with_store(|s| s.get("expert-code").map(|o| o.residency)),
        Some(Residency::Offloaded),
        "the offer store mirrors what the loader actually did",
    );

    // ── out of band again: the property the contract has no member for ──────
    println!("\nafter the policy — out of band, because moe::Capability declares no member");
    // The words are the seed's — checked against its vocabulary before READY —
    // and the expert each belongs to is this fixture's own naming.
    for spec in specializations {
        let name = format!("expert-{spec}");
        r.check(
            svc.declare_specialization(&name, spec),
            &format!("declare_specialization({name}, {spec}) — PLAN-MOE §4.5's gap, from inside"),
        );
    }

    // ── window 3: the same question, now answerable ─────────────────────────
    println!("\nwindow 3 — the wire is open, and the router can answer");
    let router_ior3 = router_ior.clone();
    serve_window(&server, &svc, &registry_ior, &mut r, |r| {
        let mut rtr = match Connection::connect(&router_ior3, T) {
            Ok(c) => c,
            Err(e) => {
                r.check(false, &format!("connect to the router: {e}"));
                return;
            }
        };
        let math =
            Constraints { required: "math".to_owned(), max_latency_ms: 1000.0, max_cost: 100.0 };
        match select(&mut rtr, GateSignal { affinity: Vec::new(), top_k: 4 }, math) {
            Ok(experts) => r.eq(
                names(&experts),
                vec!["expert-math".to_owned()],
                "select(required='math') now answers, and answers only the maths expert",
            ),
            Err(e) => r.check(false, &format!("select: {e}")),
        }
        // A capability nobody claims: an empty sequence is the true answer,
        // and it is a different reply from the refusal in window 2.
        let none =
            Constraints { required: "cooking".to_owned(), max_latency_ms: 1000.0, max_cost: 100.0 };
        match select(&mut rtr, GateSignal { affinity: Vec::new(), top_k: 4 }, none) {
            Ok(experts) => r.eq(
                experts.len(),
                0,
                "select(required='cooking') is an empty sequence — answered, not refused",
            ),
            Err(e) => r.check(false, &format!("select: {e}")),
        }
        // expert-code is OFFLOADED after the policy pass and is still
        // selected: `Constraints` declares no residency member, so `select`
        // does not filter on one, and a caller's cue is `prefetch`.
        let code =
            Constraints { required: "code".to_owned(), max_latency_ms: 1000.0, max_cost: 100.0 };
        match select(&mut rtr, GateSignal { affinity: Vec::new(), top_k: 4 }, code) {
            Ok(experts) => r.eq(
                names(&experts),
                vec!["expert-code".to_owned()],
                "an OFFLOADED expert is still selected — select filters on the declared constraints only",
            ),
            Err(e) => r.check(false, &format!("select: {e}")),
        }
    });

    // ── the latency-ordered router, before anything is measured ─────────────
    // D010 A2's sentence: "a latency-ordered router prefers the experts nobody
    // has measured". Every offer so far arrived through v1.0, so none has a
    // p50; the engine must set them aside as unranked, and a router that reads
    // `is_complete()` refuses — it does not name expert-math "the fastest"
    // because it was the only maths expert on the list.
    println!("\nbefore any measurement — ORDER BY latency_p50 over v1.0 offers");
    let fastest_maths =
        Query::parse("specialization == 'math' ORDER BY latency_p50 ASC").expect("parses");
    let picked = pick_fastest(&svc, &fastest_maths);
    r.eq(
        picked,
        Err(vec!["expert-math".to_owned()]),
        "only unmeasured candidates: the router refuses rather than picking one",
    );

    // ── window 4: the v1.1 path, over the wire ──────────────────────────────
    // `register_measured` carries a MeasuredCapability — the released
    // Capability plus the two members idl-diff refused to let anyone add in
    // place. A second maths expert arrives measured; the first is still not.
    println!("\nwindow 4 — the wire is open; register_measured (moe v1.1)");
    let registry_ior4 = registry_ior.clone();
    serve_window(&server, &svc, &registry_ior, &mut r, |r| {
        let mut reg = match Connection::connect(&registry_ior4, T) {
            Ok(c) => c,
            Err(e) => {
                r.check(false, &format!("connect to the registry: {e}"));
                return;
            }
        };
        let reference = expert_ref("expert-math-b");
        let m = MeasuredCapability {
            base: capability("expert-math-b", 120, 0.3, &node),
            specialization: "math".into(),
            latency_p50_ms: 8.0,
        };
        let ok = reg
            .invoke("register_measured", |e| {
                reference.write_to(e).expect("an IOR always encodes");
                m.write_to(e);
            })
            .is_ok();
        r.check(ok, "register_measured(expert-math-b, math, p50 8.0 ms)");
    });
    r.eq(
        svc.with_store(|s| {
            s.get("expert-math-b").map(|o| (o.specialization.clone(), o.latency_p50))
        }),
        Some((Some("math".to_owned()), Some(8.0))),
        "the two members reached the offer store as values, not placeholders",
    );
    // One measured, one not: the measured one would win — and the router
    // still refuses, because expert-math might outrank it and nobody knows.
    // A partial measurement is not a ranking.
    let picked = pick_fastest(&svc, &fastest_maths);
    r.eq(
        picked,
        Err(vec!["expert-math".to_owned()]),
        "one measured beside one unmeasured: still refused, the unmeasured one is named",
    );

    // ── window 5: the measurement for expert-math arrives by heartbeat ──────
    println!("\nwindow 5 — the wire is open; heartbeat_measured, then a v1.0 heartbeat");
    let registry_ior5 = registry_ior.clone();
    serve_window(&server, &svc, &registry_ior, &mut r, |r| {
        let mut reg = match Connection::connect(&registry_ior5, T) {
            Ok(c) => c,
            Err(e) => {
                r.check(false, &format!("connect to the registry: {e}"));
                return;
            }
        };
        let reference = expert_ref("expert-math");
        let m = MeasuredCapability {
            base: capability("expert-math", 200, 0.25, &node),
            specialization: "math".into(),
            latency_p50_ms: 12.0,
        };
        let ok = reg
            .invoke("heartbeat_measured", |e| {
                reference.write_to(e).expect("an IOR always encodes");
                m.write_to(e);
            })
            .is_ok();
        r.check(ok, "heartbeat_measured(expert-math, math, p50 12.0 ms)");
        // Then the old shape again. It has no member for either fact, so it
        // cannot withdraw them — the measurement must survive this call.
        let reference = expert_ref("expert-math");
        let cap = capability("expert-math", 200, 0.5, &node);
        let ok = reg
            .invoke("heartbeat", |e| {
                reference.write_to(e).expect("an IOR always encodes");
                cap.write_to(e);
            })
            .is_ok();
        r.check(ok, "heartbeat(expert-math) — the v1.0 shape, after the measurement");
    });
    r.eq(
        svc.with_store(|s| s.get("expert-math").map(|o| (o.load, o.latency_p50))),
        Some((0.5, Some(12.0))),
        "the v1.0 heartbeat updated what it carries and kept what it cannot mention",
    );
    // Both measured: the answer is complete, and the fastest is the one that
    // was measured fastest — not the one that registered first.
    let picked = pick_fastest(&svc, &fastest_maths);
    r.eq(
        picked,
        Ok("expert-math-b".to_owned()),
        "both measured: the router picks the faster (8.0 < 12.0), by measurement",
    );
    // The negative control the other way round: a bound the measurements
    // fail is an honest nothing, not a refusal — the answer is complete and
    // empty, and a router may say "no maths expert under 5 ms".
    let too_fast =
        Query::parse("specialization == 'math' AND latency_p50 < 5 ORDER BY latency_p50 ASC")
            .expect("parses");
    r.eq(
        pick_fastest(&svc, &too_fast),
        Ok(String::new()),
        "no maths expert under 5 ms — answered (empty), not refused",
    );

    if hold {
        println!(
            "\nHOLDING — registry/loader/router stay served; point an external client at {}",
            out.join(", ")
        );
        // serve_sites: refusal — HOLDING serves external clients until the
        // driver kills the process ("stopped by killing the process", this
        // file's header): every local check is already done, so no in-process
        // actor is left to raise a stop and a predicate here would be one
        // nobody can call.
        server.serve_shared(&svc, || false)?;
    }

    Ok(r.failures)
}

/// A latency-ordered router in five lines: the head of the ordered answer,
/// **when the answer is complete**. `Err` carries the ids the engine could not
/// judge or place — the experts a lesser router would have ranked by fiat.
/// `Ok("")` is an honest empty answer: everything was judged, nothing
/// qualified.
fn pick_fastest(svc: &ExpertService, q: &Query) -> Result<String, Vec<String>> {
    svc.with_store(|s| {
        let sel = q.select_reporting(s);
        let ranked: Vec<&str> = sel.matched.iter().map(|o| o.id.as_str()).collect();
        let set_aside: Vec<String> =
            sel.unanswerable.iter().chain(sel.unranked.iter()).map(|o| o.id.clone()).collect();
        println!(
            "        ranked {ranked:?}, set aside {set_aside:?}{}",
            sel.gap_note().map(|n| format!(" — {n}")).unwrap_or_default()
        );
        if sel.is_complete() {
            Ok(sel.matched.first().map(|o| o.id.clone()).unwrap_or_default())
        } else {
            Err(set_aside)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_giop::server::SharedDispatch;

    /// **The gate for D028 §1's third finding.**
    ///
    /// The defect was not that `MoE/registry` is a bad key — it is a fine key.
    /// It was that this fixture reached the same bytes by two routes: the
    /// server's own identity, typed here, and the registry face's key, derived
    /// by [`ExpertService::new`] from a base typed on the next line. Nothing
    /// could go red over it, because a `Server`'s key is read only by
    /// `Server::ior` and this fixture publishes the service's references.
    ///
    /// So the check is not "the key is not `MoE/registry`" — that is a
    /// spelling, and a spelling gate goes green the moment somebody picks a
    /// different pair of colliding names. It is *the server's identity is not
    /// one of the three the servant answers for*, asked of the servant itself,
    /// which is the property the two spellings were violating.
    ///
    /// And it asks it of [`plane`] — the real pair the fixture serves — rather
    /// than of two constants retyped here. A gate that retypes the choice it
    /// is checking is green over exactly the change it exists to refuse.
    #[test]
    fn the_servers_identity_is_not_one_of_the_faces_it_serves() {
        let (server, svc) = plane(
            LoadingPolicy { affinity_weight: 1, low_watermark: 100, high_watermark: 400 },
            2 * FREQ_SCALE,
        )
        .expect("the control plane binds a loopback port");
        let identity = server.object_key();
        assert!(
            !SharedDispatch::knows(&svc, identity),
            "the server binds {:?} as its own identity and the servant it serves also answers \
             to that key — the same bytes arrived at twice. The three faces are {:?}, {:?}, {:?}.",
            String::from_utf8_lossy(identity),
            String::from_utf8_lossy(svc.registry_key()),
            String::from_utf8_lossy(svc.loader_key()),
            String::from_utf8_lossy(svc.router_key()),
        );
        // …and the faces really are derived from that identity, so the
        // assertion above is about a collision that can happen rather than
        // about two unrelated byte strings.
        assert!(
            svc.registry_key().starts_with(identity),
            "the faces no longer derive from the key the server binds, so this check has \
             stopped being about a collision: identity {:?}, registry {:?}",
            String::from_utf8_lossy(identity),
            String::from_utf8_lossy(svc.registry_key()),
        );
        assert!(SharedDispatch::knows(&svc, svc.registry_key()));
    }
}
