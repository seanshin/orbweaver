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
//! Usage: `spike-experts`

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use orbweaver_giop::server::Server;
use orbweaver_giop::{Connection, Error, IiopProfile, Ior, Version};
use orbweaver_object::expert_service::{
    BAD_PARAM, Capability, EXPERT_ID, ExpertService, NO_PERMISSION, TRANSIENT,
    residency_from_ordinal,
};
use orbweaver_object::residency::Applied;
use orbweaver_trading::policy::{Decision, LoadingPolicy};
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

fn capability(id: &str, mem: u64, load: f32) -> Capability {
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
        placement_node: "gpu-04".into(),
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
    match run() {
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

fn run() -> Result<u32, Box<dyn std::error::Error>> {
    // §6's knobs, and the cold threshold in FREQ_SCALE units: "fewer than two
    // hits' worth of history left".
    let policy = LoadingPolicy { affinity_weight: 1, low_watermark: 100, high_watermark: 400 };
    let cold_below = 2 * FREQ_SCALE;

    let server = Server::bind("127.0.0.1:0", b"MoE/registry".to_vec())?;
    let port = server.local_addr()?.port();
    let svc = ExpertService::new("127.0.0.1", port, b"MoE", policy, cold_below);
    let registry_ior = svc.registry_ior();
    let loader_ior = svc.loader_ior();
    let mut r = Report { failures: 0 };

    println!("serving  {}", server.local_addr()?);
    println!(
        "registry {} key {:?}",
        registry_ior.type_id,
        String::from_utf8_lossy(svc.registry_key())
    );
    println!("loader   {} key {:?}", loader_ior.type_id, String::from_utf8_lossy(svc.loader_key()));
    println!(
        "policy   affinity {}, watermarks {}/{}, cold below {cold_below}",
        policy.affinity_weight, policy.low_watermark, policy.high_watermark
    );

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
            let cap = capability(name, mem, 0.25);
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
        let cap = capability("expert-code", 30, 0.25);
        let got = exception_id(reg.invoke("register_expert", |e| {
            reference.write_to(e).expect("an IOR always encodes");
            cap.write_to(e);
        }));
        r.eq(got.as_str(), BAD_PARAM, "a duplicate register_expert is BAD_PARAM");

        // The heartbeat: expert-code now occupies twice what it registered,
        // and this is the number the policy will decide on later.
        let reference = expert_ref("expert-code");
        let cap = capability("expert-code", 60, 0.9);
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

    Ok(r.failures)
}
