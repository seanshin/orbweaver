//! Two tenants on one MoE control plane, over a real socket.
//!
//! `moe::enterprise`'s four interfaces (corpus/golden/23) served by
//! [`TenantService`], called by our own client. What this spike is *for* is
//! the isolation claim, not the operation list: every cross-tenant call that
//! the wire surface can refuse is made here and refused here, the shared base
//! crossing is made and shown to reach nothing, a retire is shown to make a
//! reference stop existing while the neighbour's still answers, and a policy
//! is set and shown to change what `authorize` says.
//!
//! # Why two windows
//!
//! Same reason as `spike-experts`: `Server` is single-threaded and handles one
//! connection at a time, so the spike opens a serving window, does the wire
//! work, closes it, and runs the out-of-band steps with the servant back in
//! hand. The steps that must be out of band are the ones the contract declares
//! no operation for — `grant`, and reading another tenant's audit trail — and
//! that is the point rather than a workaround: a wire `grant` would be an
//! authorization surface corpus/golden/23 does not have.
//!
//! The window uses `std::thread::scope`, so the compiler enforces that no
//! out-of-band step runs while the wire is open.
//!
//! Usage: `spike-tenants [factory-ior [globex-factory-ior]] [--hold]`
//!
//! Defaults are `spikes/moe-factory.ior` and `spikes/moe-factory-globex.ior`.
//! Both are published and `READY` is printed **before** the checks run, so a
//! harness can wait on a file the way it does for `spike-names`.
//!
//! With `--hold` the serving window stays open after the checks — the same
//! shape `spike-names`, `spike-events` and `spike-ifr` have, and the thing
//! whose absence made `SERVICES-COVERAGE.md` §9 build a separate holder crate
//! to address this servant from outside. Both tenants' factories are held, so
//! an external client can measure the isolation claim and not only the
//! operation list; acme's 1.0 model has been retired by then, which is what
//! makes a `get_manifest` on a stale reference answer `OBJECT_NOT_EXIST`.
//! Stopped by killing the process.

//! # Where this fixture's population comes from
//!
//! D026 §4: *a fixture states where its population came from, and a population
//! that more than one fixture uses has one home.* Every tenant, region,
//! capability, cost, adapter delta, policy domain, grant and declared node
//! below is **loaded from `corpus/state/moe-estate.json`**, which
//! `spike_experts` and `spikes/seed_trading_client.py` read too. Nothing about
//! the population is retyped here.
//!
//! What is *not* from the seed, and is invented here on purpose: the two model
//! versions (`1.0`, `2.0`), the request and trace ids, and the one inference
//! tensor. Those exercise paths rather than describing a world, and D026 §3 is
//! explicit that a seeded population must not become the only population.
//!
//! ## Where the loader lives, and why `#[path]`
//!
//! The loader's home is `crates/orbweaver-test/src/state.rs` and it is reached
//! by including that file, not by `use orbweaver_test::state`. The reason is
//! the dependency graph and it is not a preference: `orbweaver-test` depends
//! on `orbweaver-giop`, `orbweaver-registry` and `orbweaver-dynamic`, so it
//! sits **above** every crate the five fixtures live in — a fixture cannot
//! name it without a cycle. Cargo has no bin-only dependency either, so a
//! `dev-dependency` (which reaches tests, examples and benches) does not reach
//! a `[[bin]]`.
//!
//! Including the file keeps the population's home singular — one file, two
//! compilations, no second copy to drift — which is the property D026 §4 asks
//! for. It is a workaround for the graph and it is written down as one:
//! `corpus/state/README.md`, *What the migration could not do*, records the
//! structural fix and which fixtures are still blocked by it.
#[allow(dead_code)]
#[path = "../../../orbweaver-test/src/state.rs"]
mod state;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use state::MoeEstate;

use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{BAD_OPERATION, Server};
use orbweaver_giop::{Connection, Error, Ior, Reply};
use orbweaver_object::tenant_service::{
    Activation, BAD_INV_ORDER, BAD_PARAM, CallContext, Capability, ENTERPRISE_EXPERT_ID, Manifest,
    NO_PERMISSION, OBJECT_NOT_EXIST, TenantService,
};
use orbweaver_object::{get_reference, put_reference};

const T: Duration = Duration::from_secs(5);

/// Failures are counted, never inferred from a clean exit: an unmeasured check
/// is a failure, so anything that does not report `ok` increments this.
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

/// A manifest for `tenant`, with the base model and residency region the seed
/// states for it. Only the version and the policy domain are the caller's.
fn manifest(estate: &MoeEstate, tenant: &str, version: &str, domain: &str) -> Manifest {
    let t = estate
        .tenant(tenant)
        .unwrap_or_else(|| panic!("corpus/state/moe-estate.json states the tenant `{tenant}`"));
    Manifest {
        tenant_id: t.id.clone(),
        base_model: estate.base_model.clone(),
        experts: Vec::new(),
        policy_domain: domain.to_owned(),
        version: version.to_owned(),
        residency_region: t.residency_region.clone(),
    }
}

/// A manifest naming a tenant through *another* tenant's factory — the one
/// case that must not be built from the seed's own tenant record, because what
/// it is testing is a manifest whose contents the caller has no right to.
fn foreign_manifest(estate: &MoeEstate, tenant: &str, version: &str, domain: &str) -> Manifest {
    Manifest {
        tenant_id: tenant.to_owned(),
        base_model: estate.base_model.clone(),
        experts: Vec::new(),
        policy_domain: domain.to_owned(),
        version: version.to_owned(),
        residency_region: "eu-west".to_owned(),
    }
}

fn exception_id(result: Result<Reply, Error>) -> String {
    match result {
        Err(Error::SystemException { id, .. }) => id,
        Err(other) => format!("<{other}>"),
        Ok(_) => "<no exception>".to_owned(),
    }
}

/// The single reference a reply body carries.
fn reference_in(reply: Reply) -> Result<Ior, Error> {
    get_reference(&mut reply.body()?)?.ok_or(Error::Decode("a nil reference"))
}

fn manifest_in(reply: Reply) -> Result<Manifest, Error> {
    Manifest::read_from(&mut reply.body()?).map_err(Error::Cdr)
}

fn capability_in(reply: Reply) -> Result<Capability, Error> {
    Capability::read_from(&mut reply.body()?).map_err(Error::Cdr)
}

fn activation_in(reply: Reply) -> Result<Activation, Error> {
    Activation::read_from(&mut reply.body()?).map_err(Error::Cdr)
}

/// The one inference this spike sends, written once so the request and the
/// expected reply cannot drift apart.
fn inference_input() -> Activation {
    Activation { data: vec![1, 2, 3], dtype: "f16".to_owned(), shape: "1x3".to_owned() }
}

fn write_inference(e: &mut orbweaver_cdr::Encoder) {
    inference_input().write_to(e);
    CallContext { request_id: "acme-r1".to_owned(), trace_id: "ta".to_owned(), step: 1 }
        .write_to(e);
}

fn boolean_in(reply: Reply) -> Result<bool, Error> {
    reply.body()?.get_bool().map_err(Error::Cdr)
}

fn string_in(reply: Reply) -> Result<String, Error> {
    reply.body()?.get_string().map_err(Error::Cdr)
}

fn octets_in(reply: Reply) -> Result<Vec<u8>, Error> {
    reply.body()?.get_octet_seq().map(<[u8]>::to_vec).map_err(Error::Cdr)
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
fn serve_window<F>(server: &Server, svc: &TenantService, probe: &Ior, r: &mut Report, window: F)
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
        // the serve loop is blocked in accept by now; one throwaway connection
        // is what wakes it to notice.
        stop.store(true, Ordering::SeqCst);
        drop(Connection::connect(probe, T));
        serving.join().expect("the serving thread ended cleanly");
    });
}

/// Opens a connection, runs `body`, and closes it — `Server` handles one
/// connection at a time, so overlapping two would deadlock the window.
fn on<F>(r: &mut Report, what: &str, target: &Ior, body: F)
where
    F: FnOnce(&mut Report, &mut Connection),
{
    match Connection::connect(target, T) {
        Ok(mut c) => body(r, &mut c),
        Err(e) => r.check(false, &format!("connect to {what}: {e}")),
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hold = args.iter().any(|a| a == "--hold");
    let paths: Vec<&str> =
        args.iter().filter(|a| !a.starts_with("--")).map(String::as_str).collect();
    let out = [
        paths.first().copied().unwrap_or("spikes/moe-factory.ior"),
        paths.get(1).copied().unwrap_or("spikes/moe-factory-globex.ior"),
    ];

    match run(&out, hold) {
        Ok(0) => {
            println!("\ntenant-service: PASS");
            std::process::ExitCode::SUCCESS
        }
        Ok(failures) => {
            println!("\ntenant-service: FAIL — {failures} check(s) failed");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            println!("\ntenant-service: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(out: &[&str; 2], hold: bool) -> Result<u32, Box<dyn std::error::Error>> {
    let server = Orb::new().server("127.0.0.1:0", b"MoE".to_vec())?;
    let port = server.local_addr()?.port();
    let svc = TenantService::new("127.0.0.1", port, "MoE");
    let mut r = Report { failures: 0 };

    // The population, loaded rather than invented. A failure here is a seed
    // failure and says so: an unmeasured check is a failure, and a fixture
    // that quietly fell back to values of its own would be measuring a
    // population no other reader has.
    let estate = MoeEstate::load()?;
    let acme = estate.tenant("acme").ok_or("the seed states the tenant `acme`")?;
    let globex = estate.tenant("globex").ok_or("the seed states the tenant `globex`")?;
    // By role rather than by name: the domain that grants nothing is what
    // default-deny is shown under, and the one that grants something is what
    // `set_policy` swaps to. Both spellings stay in the seed.
    let acme_default = acme.default_domain().ok_or("acme has a domain that grants nothing")?;
    let acme_strict = acme.granting_domain().ok_or("acme has a domain that grants something")?;
    let globex_default = globex.default_domain().ok_or("globex has a default domain")?;
    let grant = acme_strict.grants.first().ok_or("acme's granting domain grants something")?;

    // ── out of band: what the contract declares no operation for ────────────
    // A factory per tenant, the adapters (no weights exist here — PLAN-MOE §5)
    // and the node → region table (a deployment fact no member carries).
    let factory_a = svc.provision_factory(&acme.id).ok_or("acme is a usable tenant id")?;
    let factory_b = svc.provision_factory(&globex.id).ok_or("globex is a usable tenant id")?;
    // One expert per capability the tenant states, in the order the seed
    // states them — a JSON array has an order and this one is load-bearing:
    // the manifest's `experts` sequence comes back in bind order, and the
    // check below compares it against the same list rather than a retyped one.
    let provision = |tenant: &state::SeededTenant| -> Vec<(String, Ior)> {
        tenant
            .capabilities
            .iter()
            .filter_map(|c| {
                svc.provision_expert(
                    &tenant.id,
                    &c.name,
                    &estate.base_model,
                    c.cost as f32,
                    c.adapter_delta.as_bytes(),
                )
                .map(|ior| (c.name.clone(), ior))
            })
            .collect()
    };
    let acme_experts = provision(acme);
    let globex_experts = provision(globex);
    if acme_experts.len() != acme.capabilities.len()
        || globex_experts.len() != globex.capabilities.len()
    {
        return Err("every seeded capability provisions an expert".into());
    }
    // The prose below says "a two-element experts sequence", and prose does
    // not compile. Refused here rather than checked as an `ok` line, so the
    // demand costs no output and cannot be read as coverage: a seed that grew
    // acme a third capability would leave the sentence false and the check
    // green, which is the drift this whole batch is about.
    if acme_experts.len() != 2 {
        return Err(format!(
            "this fixture's window-1 check reads '…and a two-element experts sequence round \
             trips', but the seed states {} capabilities for `{}`. Move the sentence with the \
             seed.",
            acme_experts.len(),
            acme.id
        )
        .into());
    }
    let expert_a = acme_experts[0].1.clone();
    let expert_b = globex_experts[0].1.clone();
    for n in &estate.declared_estate.nodes {
        svc.declare_node(&n.name, &n.region);
    }

    println!("serving  {}", server.local_addr()?);
    println!("factory  acme   {}", String::from_utf8_lossy(&key_of(&factory_a)?));
    println!("factory  globex {}", String::from_utf8_lossy(&key_of(&factory_b)?));
    println!("expert   acme   {}", String::from_utf8_lossy(&key_of(&expert_a)?));
    println!("expert   globex {}", String::from_utf8_lossy(&key_of(&expert_b)?));
    println!(
        "nodes    {}",
        estate
            .declared_estate
            .nodes
            .iter()
            .map(|n| format!("{}={}", n.name, n.region))
            .collect::<Vec<_>>()
            .join(", ")
    );
    // Published before the checks run, so a harness waiting on a file is
    // waiting on something that exists as early as it can.
    for (path, ior) in out.iter().zip([&factory_a, &factory_b]) {
        std::fs::write(path, ior.to_stringified()?)?;
        println!("IOR written to {path}");
    }
    println!("READY");

    // ── window 1: minting, isolation, the base crossing ─────────────────────
    println!("\nwindow 1 — the wire is open");
    let mut models: Vec<Ior> = Vec::new();
    let mut base_ref: Option<Ior> = None;
    serve_window(&server, &svc, &factory_a, &mut r, |r| {
        on(r, "acme's factory", &factory_a, |r, c| {
            match c.invoke("create", |e| {
                manifest(&estate, &acme.id, "1.0", &acme_default.name).write_to(e)
            }) {
                Ok(reply) => match reference_in(reply) {
                    Ok(ior) => {
                        r.check(true, "acme create(1.0) mints a ComposedModel");
                        models.push(ior);
                    }
                    Err(e) => r.check(false, &format!("acme create(1.0): {e}")),
                },
                Err(e) => r.check(false, &format!("acme create(1.0): {e}")),
            }
            // A second acme model, whose only job is to mint the second policy
            // domain the window-2 `set_policy` swaps to.
            let ok = c
                .invoke("create", |e| {
                    manifest(&estate, &acme.id, "2.0", &acme_strict.name).write_to(e)
                })
                .is_ok();
            r.check(ok, &format!("acme create(2.0) mints the {} domain", acme_strict.name));
            // A manifest naming somebody else, through acme's own factory.
            let got = exception_id(c.invoke("create", |e| {
                foreign_manifest(&estate, &globex.id, "9.9", "x").write_to(e)
            }));
            r.eq(
                got.as_str(),
                NO_PERMISSION,
                "acme create() of a globex manifest is NO_PERMISSION",
            );
        });

        on(r, "globex's factory", &factory_b, |r, c| {
            match c
                .invoke("create", |e| {
                    manifest(&estate, &globex.id, "1.0", &globex_default.name).write_to(e)
                })
                .and_then(reference_in)
            {
                Ok(ior) => {
                    r.check(true, "globex create(1.0) mints a ComposedModel");
                    models.push(ior);
                }
                Err(e) => r.check(false, &format!("globex create(1.0): {e}")),
            }
        });
        let (Some(model_a), Some(model_b)) = (models.first().cloned(), models.get(1).cloned())
        else {
            r.check(false, "both models were minted");
            return;
        };

        // globex's factory, handed acme's model: refused before existence is
        // even consulted, so the refusal is not an existence oracle either.
        on(r, "globex's factory", &factory_b, |r, c| {
            let arg = model_a.clone();
            let got =
                exception_id(c.invoke("retire", move |e| put_reference(e, Some(&arg)).unwrap()));
            r.eq(got.as_str(), NO_PERMISSION, "globex retire(acme's model) is NO_PERMISSION");
            let arg = model_a.clone();
            let got =
                exception_id(c.invoke("deploy", move |e| put_reference(e, Some(&arg)).unwrap()));
            r.eq(got.as_str(), NO_PERMISSION, "globex deploy(acme's model) is NO_PERMISSION");
            let arg = model_a.clone();
            let got = exception_id(c.invoke("clone_model", move |e| {
                put_reference(e, Some(&arg)).unwrap();
                e.put_str("stolen");
            }));
            r.eq(got.as_str(), NO_PERMISSION, "globex clone_model(acme's model) is NO_PERMISSION");
        });

        // The manifest as corpus/golden/23 declares it: an empty
        // sequence<::moe::CapabilityId> first, a two-element one after the binds.
        on(r, "acme's model", &model_a, |r, c| {
            match c.invoke_nullary("get_manifest").and_then(manifest_in) {
                Ok(m) => {
                    r.eq(m.tenant_id.as_str(), acme.id.as_str(), "get_manifest().tenant_id");
                    r.eq(
                        m.residency_region.as_str(),
                        acme.residency_region.as_str(),
                        "get_manifest().residency_region",
                    );
                    r.check(m.experts.is_empty(), "…and an empty experts sequence round trips");
                }
                Err(e) => r.check(false, &format!("get_manifest: {e}")),
            }
            for (what, ex) in &acme_experts {
                let arg = ex.clone();
                let ok =
                    c.invoke("bind_expert", move |e| put_reference(e, Some(&arg)).unwrap()).is_ok();
                r.check(ok, &format!("bind_expert(acme/{what})"));
            }
            // acme's model, handed globex's expert.
            let arg = expert_b.clone();
            let got = exception_id(
                c.invoke("bind_expert", move |e| put_reference(e, Some(&arg)).unwrap()),
            );
            r.eq(got.as_str(), NO_PERMISSION, "bind_expert(globex's expert) is NO_PERMISSION");
            match c.invoke_nullary("get_manifest").and_then(manifest_in) {
                // The expectation is the bind list itself, not a retyped copy
                // of it: a fixture that types `["math", "code"]` beside a
                // population it also typed is one author agreeing with
                // themselves, which is the shape D026 §5 S1 exists to remove.
                Ok(m) => r.eq(
                    m.experts,
                    acme_experts.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>(),
                    "…and a two-element experts sequence round trips",
                ),
                Err(e) => r.check(false, &format!("get_manifest: {e}")),
            }
            // infer before deploy is a missing edge, not a silent success.
            let got = exception_id(c.invoke("infer", write_inference));
            r.eq(got.as_str(), BAD_INV_ORDER, "infer before deploy is BAD_INV_ORDER");
        });

        // The base crossing: both tenants' experts hand back one reference.
        let mut from_a = None;
        on(r, "acme's expert", &expert_a, |r, c| {
            match c.invoke_nullary("get_tenant_id").and_then(string_in) {
                Ok(t) => r.eq(t.as_str(), acme.id.as_str(), "get_tenant_id()"),
                Err(e) => r.check(false, &format!("get_tenant_id: {e}")),
            }
            match c.invoke_nullary("adapter_delta").and_then(octets_in) {
                Ok(d) => r.eq(
                    d.as_slice(),
                    acme.capabilities[0].adapter_delta.as_bytes(),
                    "adapter_delta() is the tenant's",
                ),
                Err(e) => r.check(false, &format!("adapter_delta: {e}")),
            }
            match c.invoke_nullary("base").and_then(reference_in) {
                Ok(ior) => {
                    r.eq(ior.type_id.as_str(), "IDL:moe/Expert:1.0", "base() is a ::moe::Expert");
                    from_a = Some(ior);
                }
                Err(e) => r.check(false, &format!("base: {e}")),
            }
        });
        on(r, "globex's expert", &expert_b, |r, c| {
            match c.invoke_nullary("base").and_then(reference_in) {
                Ok(ior) => {
                    r.eq(Some(&ior), from_a.as_ref(), "globex's base() is the same shared object");
                    base_ref = Some(ior);
                }
                Err(e) => r.check(false, &format!("base: {e}")),
            }
        });

        // …and what that shared reference can do: describe itself, and nothing
        // that would reach into a tenant.
        let Some(shared) = base_ref.clone() else {
            r.check(false, "the shared base reference was obtained");
            return;
        };
        on(r, "the shared base", &shared, |r, c| {
            match c.invoke_nullary("describe").and_then(capability_in) {
                Ok(cap) => r.eq(
                    cap,
                    Capability { id: estate.base_model.clone(), cost: 0.0 },
                    "describe() — cost 0.0, because the manifest carries none",
                ),
                Err(e) => r.check(false, &format!("describe: {e}")),
            }
            match c.invoke("_is_a", |e| e.put_str(ENTERPRISE_EXPERT_ID)).and_then(boolean_in) {
                Ok(v) => r.eq(v, false, "the shared base does not narrow to EnterpriseExpert"),
                Err(e) => r.check(false, &format!("_is_a: {e}")),
            }
            let got = exception_id(c.invoke_nullary("get_tenant_id"));
            r.eq(got.as_str(), BAD_OPERATION, "…and has no tenant to give");
        });
        on(r, "acme's model", &model_a, |r, c| {
            let arg = shared.clone();
            let got = exception_id(
                c.invoke("bind_expert", move |e| put_reference(e, Some(&arg)).unwrap()),
            );
            r.eq(got.as_str(), BAD_PARAM, "bind_expert(base()) is refused — it names no tenant");
        });

        // Deploy acme's model and run one inference through it.
        on(r, "acme's factory", &factory_a, |r, c| {
            let arg = model_a.clone();
            let ok = c.invoke("deploy", move |e| put_reference(e, Some(&arg)).unwrap()).is_ok();
            r.check(ok, "deploy(acme's model)");
        });
        on(r, "acme's model", &model_a, |r, c| {
            // PLAN-MOE §5: no data plane exists here, so the tensor comes back
            // unchanged. What is measured is that the round trip marshals, not
            // that anything computed.
            match c.invoke("infer", write_inference).and_then(activation_in) {
                Ok(y) => r.eq(y, inference_input(), "infer() once deployed round trips"),
                Err(e) => r.check(false, &format!("infer: {e}")),
            }
        });
        on(r, "globex's model", &model_b, |r, c| {
            match c.invoke_nullary("get_manifest").and_then(manifest_in) {
                Ok(m) => r.eq(m.tenant_id.as_str(), "globex", "globex's model answers for globex"),
                Err(e) => r.check(false, &format!("get_manifest: {e}")),
            }
        });
    });

    // ── between the windows: what no wire operation may do ──────────────────
    println!("\nbetween windows — out of band, because the contract declares no operation");
    let granted = svc.grant(&acme.id, &acme_strict.name, &grant.subject, &grant.capability);
    r.check(
        granted,
        &format!(
            "grant({}, {} → {}) — no wire operation exists for this",
            acme_strict.name, grant.subject, grant.capability
        ),
    );
    let strict = svc
        .policy_reference(&acme.id, &acme_strict.name)
        .ok_or_else(|| format!("{} exists", acme_strict.name))?;
    let default_a = svc
        .policy_reference(&acme.id, &acme_default.name)
        .ok_or_else(|| format!("{} exists", acme_default.name))?;
    let default_b = svc
        .policy_reference(&globex.id, &globex_default.name)
        .ok_or_else(|| format!("{} exists", globex_default.name))?;
    r.eq(svc.base_crossings(&acme.id), 1, "acme's base crossing was counted");
    r.eq(svc.base_crossings(&globex.id), 1, "globex's base crossing was counted");

    let [model_a, model_b] = models.as_slice() else {
        // An unmeasured check is a failure, never a pass: without both models
        // window 2 measures nothing, so it does not run and says so.
        println!("  FAIL  both models exist for window 2");
        return Ok(r.failures + 1);
    };
    let (model_a, model_b) = (model_a.clone(), model_b.clone());

    // ── window 2: policy, residency, and a real retire ──────────────────────
    println!("\nwindow 2 — the wire is open again");
    serve_window(&server, &svc, &factory_a, &mut r, |r| {
        on(r, &acme_default.name, &default_a, |r, c| {
            match c
                .invoke("authorize", |e| {
                    e.put_str(&grant.subject);
                    e.put_str(&grant.capability);
                })
                .and_then(boolean_in)
            {
                Ok(v) => r.eq(v, false, "authorize() under the default domain: default-deny"),
                Err(e) => r.check(false, &format!("authorize: {e}")),
            }
        });

        on(r, "acme's model", &model_a, |r, c| {
            // The one operation with a scope of its own, because whoever sets
            // the policy lifts every other gate.
            let arg = strict.clone();
            let ok = c.invoke("set_policy", move |e| put_reference(e, Some(&arg)).unwrap()).is_ok();
            r.check(ok, &format!("set_policy({})", acme_strict.name));
            match c.invoke_nullary("get_manifest").and_then(manifest_in) {
                Ok(m) => r.eq(
                    m.policy_domain.as_str(),
                    acme_strict.name.as_str(),
                    "…and the manifest names the domain that was set",
                ),
                Err(e) => r.check(false, &format!("get_manifest: {e}")),
            }
            let arg = default_b.clone();
            let got = exception_id(
                c.invoke("set_policy", move |e| put_reference(e, Some(&arg)).unwrap()),
            );
            r.eq(got.as_str(), NO_PERMISSION, "set_policy(globex's domain) is NO_PERMISSION");
        });

        on(r, &acme_strict.name, &strict, |r, c| {
            match c
                .invoke("authorize", |e| {
                    e.put_str(&grant.subject);
                    e.put_str(&grant.capability);
                })
                .and_then(boolean_in)
            {
                Ok(v) => r.eq(v, true, "authorize() reflects the policy that was set"),
                Err(e) => r.check(false, &format!("authorize: {e}")),
            }
            // The seed names the word nobody is granted rather than this
            // fixture picking one, because the refusal below is a
            // demonstration only while that stays true — and `vision` is
            // exactly the word `spike_experts` registers as a pinned, resident
            // expert. Two worlds, not a contradiction:
            // `corpus/state/README.md`, *The two worlds `vision` lives in*.
            // `the_ungranted_capability_is_granted_by_nobody` is what goes red
            // if a grant ever appears.
            match c
                .invoke("authorize", |e| {
                    e.put_str(&grant.subject);
                    e.put_str(&estate.ungranted_capability);
                })
                .and_then(boolean_in)
            {
                Ok(v) => r.eq(v, false, "…and only for what was granted"),
                Err(e) => r.check(false, &format!("authorize: {e}")),
            }
            // Domain A, exhaustively: every node the operator declared, plus
            // the one deliberately not declared. Both the expectation and the
            // reason are *derived from the estate* rather than typed beside
            // it, so a seed that moved `gpu-us-1` into `eu-west` would change
            // what this fixture expects instead of leaving it asserting the
            // old answer. Nothing here consults domain B: a node an expert
            // reports about itself is not a node the operator declared, and
            // resolving one against the other is the question neither domain
            // can answer.
            let mut residency: Vec<(String, bool, &str)> = estate
                .declared_estate
                .nodes
                .iter()
                .map(|n| {
                    if n.region == acme.residency_region {
                        (n.name.clone(), true, "in the manifest's region")
                    } else {
                        (n.name.clone(), false, "in another region — refused")
                    }
                })
                .collect();
            residency.push((
                estate.declared_estate.undeclared_probe.clone(),
                false,
                "undeclared — refused, default-deny",
            ));
            for (node, want, why) in residency {
                let arg = node.clone();
                match c.invoke("check_residency", move |e| e.put_str(&arg)).and_then(boolean_in) {
                    Ok(v) => r.eq(v, want, &format!("check_residency({node}) — {why}")),
                    Err(e) => r.check(false, &format!("check_residency({node}): {e}")),
                }
            }
            let ok = c
                .invoke("audit", |e| {
                    CallContext { request_id: "acme-r2".into(), trace_id: "ta".into(), step: 2 }
                        .write_to(e);
                    e.put_str("placement approved");
                })
                .is_ok();
            r.check(ok, "audit(ctx, event) appends to acme's trail");
        });

        // The retire, and what it means: the reference stops existing.
        on(r, "acme's factory", &factory_a, |r, c| {
            let arg = model_a.clone();
            let ok = c.invoke("retire", move |e| put_reference(e, Some(&arg)).unwrap()).is_ok();
            r.check(ok, "retire(acme's 1.0)");
            let arg = model_a.clone();
            let got =
                exception_id(c.invoke("retire", move |e| put_reference(e, Some(&arg)).unwrap()));
            r.eq(got.as_str(), OBJECT_NOT_EXIST, "retiring it again says gone, not bad argument");
        });
        on(r, "acme's retired model", &model_a, |r, c| {
            let got = exception_id(c.invoke_nullary("get_manifest"));
            r.eq(got.as_str(), OBJECT_NOT_EXIST, "the retired reference stops existing");
        });
        on(r, "globex's model", &model_b, |r, c| {
            match c.invoke_nullary("get_manifest").and_then(manifest_in) {
                Ok(m) => {
                    r.eq(m.version.as_str(), "1.0", "…while the other tenant's model still answers")
                }
                Err(e) => r.check(false, &format!("get_manifest: {e}")),
            }
        });
    });

    // ── after the windows: the trails, which no wire operation reads ────────
    println!("\nafter the windows — the audit trails");
    let acme = svc.audit_log("acme");
    let globex = svc.audit_log("globex");
    for e in &acme {
        println!("        acme   {:?} {} {}", e.domain, e.request_id, e.event);
    }
    for e in &globex {
        println!("        globex {:?} {} {}", e.domain, e.request_id, e.event);
    }
    r.check(
        acme.iter().any(|e| e.request_id == "acme-r2" && e.event == "placement approved"),
        "acme's trail holds the wire audit line",
    );
    r.check(!acme.iter().any(|e| e.request_id.starts_with("globex")), "…and none of globex's");
    r.check(
        !globex.iter().any(|e| e.request_id.starts_with("acme")),
        "globex's trail holds none of acme's",
    );
    r.check(
        acme.iter().any(|e| e.event.contains("base crossing")),
        "the base crossing is in the trail, not only in a counter",
    );
    r.eq(svc.audit_log("nobody").len(), 0, "a tenant with no calls has no trail");

    if hold {
        println!(
            "\nHOLDING — both tenants' factories stay served; point an external client at {}",
            out.join(", ")
        );
        server.serve_shared(&svc, || false)?;
    }

    Ok(r.failures)
}

fn key_of(ior: &Ior) -> Result<Vec<u8>, Error> {
    Ok(ior.primary()?.object_key.clone())
}
