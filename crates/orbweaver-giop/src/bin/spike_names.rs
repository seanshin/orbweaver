//! Self-consistency proof for the first-party CosNaming server: OUR client
//! against OUR server, one process, loopback.
//!
//! The client half has resolved against omniNames since Phase 1; this
//! exercises the same client against `NamingServer` — bind, resolve,
//! resolve_str, AlreadyBound, unbind then NotFound, nested contexts through
//! two levels by path and by the child's own object key, and the `list`
//! stub. Self-consistency only: it proves the two halves agree, not that
//! either matches the specification.
//!
//! Usage: `spike-names [ior-out-path] [--hold]`
//!
//! Default IOR path is `spikes/names.ior`. With `--hold` the server keeps
//! running after the checks, `spike/Echo` left bound, so an external client
//! can be pointed at it.
//!
//! # Cross-ORB oracle (integration's job, not run here)
//!
//! The independent check is an omniORB *client* resolving against this
//! server. Start `spike-names spikes/names.ior --hold`, then:
//!
//! ```text
//! python3 -c "import sys; from omniORB import CORBA; import CosNaming; \
//! orb = CORBA.ORB_init(sys.argv); \
//! nc = orb.string_to_object(open('spikes/names.ior').read().strip())._narrow(CosNaming.NamingContextExt); \
//! print(orb.object_to_string(nc.resolve_str('spike/Echo')))"
//! ```
//!
//! PASS is a printed `IOR:` string whose object key is `Echo` (the dummy
//! bound below — host 192.0.2.1 is TEST-NET, deliberately undialable:
//! naming stores references, it does not call them). Two serving limits the
//! harness must respect: one connection is served at a time, so the python
//! client must run while no other client holds a connection; and the
//! `--hold` process is stopped by killing it — there is no remote shutdown.

use orbweaver_giop::Error;
use orbweaver_giop::naming::{NameComponent, NamingContext, read_name};
use orbweaver_giop::naming_server::{
    ALREADY_BOUND_ID, NOT_FOUND_ID, NamingServer, WHY_MISSING_NODE,
};
use orbweaver_giop::server::Server;
use orbweaver_giop::{IiopProfile, Ior, Version};
use std::time::Duration;

const T: Duration = Duration::from_secs(5);

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hold = args.iter().any(|a| a == "--hold");
    let out_path = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "spikes/names.ior".into());

    match run(&out_path, hold) {
        Ok(()) => {
            println!("\nnaming-server: PASS");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            println!("\nnaming-server: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn ok(what: &str) {
    println!("  ok   {what}");
}

fn require(cond: bool, what: &str) -> Result<(), Box<dyn std::error::Error>> {
    if cond { Ok(()) } else { Err(what.into()) }
}

fn n(id: &str) -> NameComponent {
    NameComponent::new(id)
}

/// A reference to bind. TEST-NET host on purpose: resolving must work
/// without the target being dialable.
fn dummy(key: &[u8]) -> Ior {
    Ior {
        type_id: "IDL:spike/Echo:1.0".into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "192.0.2.1".into(),
            port: 4000,
            object_key: key.to_vec(),
            components: Vec::new(),
        }],
    }
}

fn run(out_path: &str, hold: bool) -> Result<(), Box<dyn std::error::Error>> {
    let server = Server::bind("127.0.0.1:0", b"NameService".to_vec())?;
    let port = server.local_addr()?.port();
    let mut ns = NamingServer::new("127.0.0.1", port, b"NameService".to_vec());
    let root = ns.root_ior();
    std::fs::write(out_path, root.to_stringified()?)?;
    println!("listening on 127.0.0.1:{port}");
    println!("IOR written to {out_path}");
    println!("READY");
    std::thread::spawn(move || {
        let _ = server.serve(&mut ns, || false);
    });

    let mut ctx = NamingContext::connect(&root, T)?;

    // ── bind and resolve, structured and stringified ──
    ctx.bind_new_context(&[n("spike")])?;
    ok("bind_new_context spike");
    let echo = dummy(b"Echo");
    ctx.bind(&[n("spike"), n("Echo")], &echo)?;
    ok("bind spike/Echo");

    let got = ctx.resolve(&[n("spike"), n("Echo")])?;
    require(
        got.type_id == echo.type_id && got.primary()?.object_key == b"Echo",
        "resolve returned a different reference than was bound",
    )?;
    ok("resolve spike/Echo returned the bound reference");

    let via_str = ctx.resolve_str("spike/Echo")?;
    require(
        via_str.primary()?.object_key == got.primary()?.object_key,
        "resolve_str disagreed with resolve",
    )?;
    ok("resolve_str agreed with resolve");

    // ── AlreadyBound ──
    match ctx.bind(&[n("spike"), n("Echo")], &echo) {
        Err(Error::UserException { id, .. }) if id == ALREADY_BOUND_ID => {
            ok("second bind of spike/Echo raised AlreadyBound");
        }
        other => return Err(format!("expected AlreadyBound, got {other:?}").into()),
    }

    // ── NotFound after unbind, members decoded as a client would ──
    ctx.bind(&[n("spike"), n("Gone")], &dummy(b"Gone"))?;
    ctx.unbind(&[n("spike"), n("Gone")])?;
    ok("bind then unbind spike/Gone");
    match ctx.resolve(&[n("spike"), n("Gone")]) {
        Err(Error::UserException { id, reply }) if id == NOT_FOUND_ID => {
            let mut b = reply.body()?;
            let _repo_id = b.get_string()?;
            let why = b.get_u32()?;
            let rest = read_name(&mut b)?;
            require(
                why == WHY_MISSING_NODE && rest == vec![n("Gone")],
                "NotFound members were not {missing_node, [Gone]}",
            )?;
            ok("resolve after unbind raised NotFound{missing_node, rest=[Gone]}");
        }
        other => return Err(format!("expected NotFound, got {other:?}").into()),
    }

    // ── nested contexts, two levels, by path and by the child's own key ──
    ctx.bind_new_context(&[n("depth1")])?;
    ctx.bind_new_context(&[n("depth1"), n("depth2")])?;
    ctx.bind(&[n("depth1"), n("depth2"), n("leaf")], &dummy(b"Leaf"))?;
    let through = ctx.resolve(&[n("depth1"), n("depth2"), n("leaf")])?;
    require(through.primary()?.object_key == b"Leaf", "resolve through two levels failed")?;
    ok("resolve depth1/depth2/leaf through the root context");

    let child = ctx.resolve(&[n("depth1")])?;
    require(
        child.primary()?.object_key != b"NameService",
        "a nested context must have its own object key",
    )?;
    // One connection is served at a time: hang up before dialing the child.
    drop(ctx);
    let mut sub = NamingContext::connect(&child, T)?;
    let tail = sub.resolve(&[n("depth2"), n("leaf")])?;
    require(tail.primary()?.object_key == b"Leaf", "resolve via the child's key failed")?;
    ok("resolve depth2/leaf via the nested context's own object key");
    drop(sub);

    // ── the list stub: up to how_many, nil iterator, honestly truncated ──
    let mut ctx = NamingContext::connect(&root, T)?;
    let (page, it) = ctx.list(1)?;
    require(page.len() == 1 && it.is_nil(), "list(1) was not one binding plus a nil iterator")?;
    ok("list(1) returned one binding and a nil iterator (stub: remainder dropped)");
    let (all, it) = ctx.list(100)?;
    require(
        all.len() == 2 && all.iter().all(|b| b.is_context) && it.is_nil(),
        "list(100) was not exactly the two root contexts",
    )?;
    ok("list(100) returned both root bindings, both ncontext");
    drop(ctx);

    if hold {
        println!("HOLDING — spike/Echo stays bound; point a cross-ORB client at {out_path}");
        loop {
            std::thread::park();
        }
    }
    Ok(())
}
