//! Holds `ExpertService` and `TenantService` open on sockets so an external
//! client can sweep them.
//!
//! HARNESS FIXTURE for `spikes/service_sweep.sh`. It performs no checks and
//! asserts nothing: `spike-experts` and `spike-tenants` already measure those
//! two servants. What this adds is the one thing they do not have — a serving
//! window that outlives the process's own client, which is what `--hold` gives
//! `spike-names`, `spike-events` and `spike-ifr`.
//!
//! Usage: `moe-hold <out-dir>`
//!
//! Writes `<out-dir>/moe-registry.ior`, `moe-loader.ior` and
//! `moe-factory.ior`, prints `READY`, then parks. Stopped by killing it.
//!
//! Only the bootstrap that the two contracts declare *no operation for* is
//! done here — `provision_factory` and `provision_expert`, both documented in
//! `tenant_service` as deliberately out of band. Everything the contracts do
//! declare is left for the sweep to call over the wire, because a servant
//! state set up in-process would not measure the dispatch path.

use std::time::Duration;

use orbweaver_giop::server::Server;
use orbweaver_object::expert_service::ExpertService;
use orbweaver_object::tenant_service::TenantService;
use orbweaver_trading::FREQ_SCALE;
use orbweaver_trading::policy::LoadingPolicy;

/// The tenant the sweep addresses. One is enough: this holder measures an
/// operation surface, not the isolation claim `spike-tenants` measures.
const TENANT: &str = "acme";

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("moe-hold: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::env::args().nth(1).unwrap_or_else(|| "spikes".to_owned());

    // ── moe::ExpertRegistry / moe::ExpertLoader (corpus/golden/22) ───────────
    let expert_server = Server::bind("127.0.0.1:0", b"MoE/registry".to_vec())?;
    let expert_port = expert_server.local_addr()?.port();
    let policy = LoadingPolicy { affinity_weight: 1, low_watermark: 100, high_watermark: 400 };
    let mut experts = ExpertService::new("127.0.0.1", expert_port, b"MoE", policy, 2 * FREQ_SCALE);
    write_ior(&out_dir, "moe-registry.ior", &experts.registry_ior())?;
    write_ior(&out_dir, "moe-loader.ior", &experts.loader_ior())?;

    // ── moe::enterprise (corpus/golden/23) ──────────────────────────────────
    let tenant_server = Server::bind("127.0.0.1:0", b"MoE/enterprise".to_vec())?;
    let tenant_port = tenant_server.local_addr()?.port();
    let mut tenants = TenantService::new("127.0.0.1", tenant_port, "MoE/enterprise");
    let factory = tenants
        .provision_factory(TENANT)
        .ok_or("the tenant id is not key-safe, which is a bug in this fixture")?;
    tenants
        .provision_expert(TENANT, "math", "llama-3", 1.5, b"delta")
        .ok_or("the expert components are not key-safe, which is a bug in this fixture")?;
    tenants.declare_node("gpu-04", "eu-west");
    write_ior(&out_dir, "moe-factory.ior", &factory)?;

    println!("moe-hold: expert service on 127.0.0.1:{expert_port}");
    println!("moe-hold: tenant service on 127.0.0.1:{tenant_port}");
    println!("READY");

    std::thread::spawn(move || {
        let _ = expert_server.serve(&mut experts, || false);
    });
    std::thread::spawn(move || {
        let _ = tenant_server.serve(&mut tenants, || false);
    });

    // Parking is what `--hold` does in the three spike binaries that have it.
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn write_ior(
    dir: &str,
    name: &str,
    ior: &orbweaver_giop::Ior,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::write(format!("{dir}/{name}"), ior.to_stringified()?)?;
    Ok(())
}
