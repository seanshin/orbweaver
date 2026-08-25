//! `corbaloc:rir:` end to end, with a **foreign ORB on the far end**.
//!
//! Usage: `spike-rir [--peer <corbaloc-url>] [--name <path>] [--empty-table]`
//!
//! (This file is `spike-rir.rs` where its twelve siblings are `spike_*.rs`.
//! Cargo infers a bin target's name from the file stem for any `src/bin/*.rs`
//! no `[[bin]]` claims, so the hyphen is what buys the conventional
//! `spike-rir` command **without** an entry in `Cargo.toml` — which a branch
//! in flight was editing at the time. Normalise it to `spike_rir.rs` plus a
//! `[[bin]]` block whenever both branches have landed.)
//!
//! Defaults: peer `corbaloc::127.0.0.1:2809/NameService` (omniNames, started by
//! the harness), name `spike/Echo` (bound by `spikes/register_name.py`).
//!
//! # Which direction actually measures *our* table
//!
//! `rir` means *resolve initial references*, and CORBA 3.4 §8.5.2 is explicit
//! that the mechanism is **local**: *"a simplified, local version of the Naming
//! Service."* So the ORB that resolves a `corbaloc:rir:` URL is always the ORB
//! being asked, never the one on the other end of the wire. Handing
//! `corbaloc:rir:NameService` to omniORB's Python client therefore measures
//! **omniORB's** initial references table — configured with `-ORBInitRef` — and
//! says nothing at all about ours, however green it comes back.
//!
//! The direction that does measure ours is this one:
//!
//! 1. our [`Orb`] is told `NameService = <the omniNames endpoint>`;
//! 2. our ORB resolves `corbaloc:rir:NameService` **out of its own table**;
//! 3. the reference that comes back is dialled, and the servant that answers is
//!    omniNames — a separate process, a foreign implementation, over TCP;
//! 4. `resolve_str("spike/Echo")` returns a reference to omniORB's echo
//!    servant, and that reference is **called**, because resolving without
//!    calling proves only that bytes were exchanged.
//!
//! Every one of those steps except (1) and (2) already worked. What is new is
//! that a URL carrying **no address at all** reached a foreign servant.
//!
//! # The negative controls this binary can run
//!
//! - `--empty-table` — registers nothing, so step (2) has nothing to answer
//!   with. Expected: a refusal **naming `NameService`**, not a panic and not a
//!   silent `None`. Exit 1.
//! - the unregistered name is asked for on every run (step 5 below): expected a
//!   refusal naming it, and it is a FAIL if the ORB answers.
//! - the addressed forms are re-resolved on every run (step 6) and compared to
//!   what `ObjectUrl::to_ior` alone produces, so the case that already worked
//!   cannot move without this binary going red.

use orbweaver_giop::naming::{NamingContext, ObjectUrl, stringify_name};
use orbweaver_giop::orb::{InvalidName, Orb, OrbConfig};
use orbweaver_giop::{Connection, Ior};
use std::time::Duration;

const T: Duration = Duration::from_secs(5);
const NC_EXT: &str = orbweaver_giop::naming::NAMING_CONTEXT_EXT_ID;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    };
    let peer = flag("--peer").unwrap_or_else(|| "corbaloc::127.0.0.1:2809/NameService".into());
    let name = flag("--name").unwrap_or_else(|| "spike/Echo".into());
    let empty = args.iter().any(|a| a == "--empty-table");

    // D019 step 3: everything an operator would actually type. Anything that is
    // not `-ORB...` comes back and is read by the flags above, which is CORBA
    // 3.4 §8.5.1's *"it will remove from the arg_list ... all strings that match
    // the -ORB<suffix> pattern"* doing its job on a real argument list.
    let (config, rest) = match OrbConfig::from_orb_args(&args) {
        Ok(pair) => pair,
        Err(e) => {
            println!("\nrir: FAIL — {e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    if !config.is_empty() {
        println!(
            "orb args   {} consumed, {} left for the program",
            args.len() - rest.len(),
            rest.len()
        );
    }

    match run(&peer, &name, empty, config) {
        Ok(()) => {
            println!("\nrir: PASS — a URL with no address reached a foreign servant");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            println!("\nrir: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn ok(what: &str) {
    println!("  ok   {what}");
}

fn run(
    peer: &str,
    name: &str,
    empty: bool,
    config: OrbConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let from_args = config.initial_references().iter().any(|(id, _)| id == "NameService");
    // Say which input actually decided the target. Printing `--peer` when an
    // `-ORBInitRef` overrode it would read as though this dialled the default,
    // and a line a harness quotes has to be true.
    if from_args {
        println!("peer       (from -ORBInitRef NameService=…; --peer unused)");
    } else {
        println!("peer       {peer}");
    }
    println!("name       {name}");
    println!("table      {}", if empty { "EMPTY (negative control)" } else { "NameService" });

    // -- 0. D019 step 3: the operator's own configuration --
    // If a `-ORBInitRef` was given, the table is already populated before this
    // binary names a single service, and every number below came off the
    // argument list rather than out of a source file. That is the whole claim
    // of step 3, and the steps that follow cannot tell the difference.
    let configured = config.initial_references().to_vec();
    let mut orb = Orb::with_config(config)?;
    for (object_id, url) in &configured {
        ok(&format!("-ORBInitRef {object_id}={url} registered (CORBA 3.4 §8.5.3.2)"));
    }
    println!(
        "limits     max_message_size={} max_connections={} follow_timeout={:?} stop_poll={:?}",
        orb.config().max_message_size(),
        orb.config().max_connections(),
        orb.config().follow_timeout(),
        orb.config().stop_poll(),
    );

    // ── 1. the address the deployment knows, turned into a reference ──
    // This half is the part that already worked. `--peer` is typed by a human,
    // so it goes through `string_to_object` (§8.2.2.2, D019 step 2) and accepts
    // an `IOR:<hex>` blob just as readily as a URL — the caller does not have
    // to know which it is holding, which is the whole point of that operation.
    // §8.5.3.2 says `-ORBInitRef <ObjectID>=<ObjectURL>` takes any scheme
    // `string_to_object` supports, so this is also the shape a real ORB
    // argument would have.
    let mut ns_ior: Ior = orb.string_to_object(peer)?;
    if ns_ior.type_id.is_empty() {
        // A URL carries no repository id and this one is known: the peer is a
        // naming context. Stamping it here rather than inside the conversion is
        // §8.5.2's *"the application is responsible for narrowing"*.
        ns_ior.type_id = NC_EXT.to_owned();
    }

    if !empty && !orb.list_initial_services().iter().any(|k| k == "NameService") {
        // §16.10.1 refuses a second registration, so this yields to whatever a
        // `-ORBInitRef NameService=...` already put there: the operator's answer
        // wins over the binary's, which is the point of having the argument.
        orb.register_initial_reference("NameService", ns_ior.clone())?;
        ok("registered NameService into the ORB's initial references table");
    }
    println!("  --   list_initial_services -> {:?}", orb.list_initial_services());

    // ── 2. the gap: a URL that carries no address at all ──
    let bootstrap = ObjectUrl::parse("corbaloc:rir:NameService")?;
    assert_eq!(
        bootstrap,
        ObjectUrl::InitialReference("NameService".into()),
        "the parser has read this form since Phase 1; only the answer is new"
    );
    let resolved = match orb.resolve_url(&bootstrap, NC_EXT) {
        Ok(ior) => ior,
        Err(e) => {
            // The negative control's expected path. It must name the key.
            let said = e.to_string();
            if !said.contains("NameService") {
                return Err(format!("the refusal did not name the key it refused: {said}").into());
            }
            return Err(format!("corbaloc:rir:NameService was refused: {said}").into());
        }
    };
    if configured.is_empty() && resolved != ns_ior {
        return Err("the table answered with a reference that is not the one registered".into());
    }
    ok("corbaloc:rir:NameService resolved out of the table — no address in the URL");

    // ── 3–4. the peer: dial what the table answered, and call through it ──
    let mut ctx = NamingContext::connect(&resolved, T)?;
    ok("connected to the foreign naming service through the resolved reference");

    let target = ctx.resolve_str(name)?;
    ok(&format!(
        "resolve_str({name:?}) -> {} ({} profile(s))",
        target.type_id,
        target.profiles.len()
    ));

    // Structured `resolve` too, because a peer may implement only one well.
    let path = orbweaver_giop::naming::parse_stringified_name(name)?;
    let via_struct = ctx.resolve(&path)?;
    if via_struct.primary()?.object_key != target.primary()?.object_key {
        return Err("resolve() and resolve_str() disagreed about the target".into());
    }
    ok(&format!("resolve({}) agreed with resolve_str", stringify_name(&path)));
    drop(ctx);

    let mut conn = Connection::connect(&target, T)?;
    let n = conn.invoke_nullary("ping")?.body()?.get_i32()?;
    if n != 42 {
        return Err(format!("ping() through the bootstrapped reference returned {n}").into());
    }
    ok("ping() through the bootstrapped reference -> 42 (a foreign servant answered)");

    // ── 5. negative control: a name nothing registered ──
    let unknown = ObjectUrl::parse("corbaloc:rir:TradingService")?;
    match orb.resolve_url(&unknown, NC_EXT) {
        Err(InvalidName::NotRegistered { key, .. }) if key == "TradingService" => {
            let said = orb.resolve_initial_reference("TradingService").unwrap_err().to_string();
            if !said.contains("\"TradingService\"") {
                return Err(format!("the refusal did not name the key: {said}").into());
            }
            ok(&format!("corbaloc:rir:TradingService refused by name — {said}"));
        }
        other => {
            return Err(
                format!("an unregistered name must be refused by name, got {other:?}").into()
            );
        }
    }

    // ── 6. negative control: the forms that already worked have not moved ──
    //
    // Fixed URLs rather than `peer`: since step 2 routed `--peer` through
    // `string_to_object`, it may be an `IOR:<hex>` blob, which `ObjectUrl::parse`
    // rightly refuses. Found by running this binary with a hex `--peer` on
    // 2026-08-25 — the check had quietly assumed its input was a URL, which is
    // the very assumption `string_to_object` exists to stop callers making.
    for text in [
        "corbaloc::127.0.0.1:2809/NameService",
        "corbaloc:iiop:1.2@10.0.0.1:9999/Echo",
        "corbaname::127.0.0.1:2809/NameService#spike/Echo",
    ] {
        let url = ObjectUrl::parse(text)?;
        let direct = url.to_ior(NC_EXT).ok_or("an addressed URL stopped building an IOR")?;
        let through = orb.resolve_url(&url, NC_EXT)?;
        if direct != through {
            return Err(
                format!("{text} resolves differently through the ORB than through to_ior").into()
            );
        }
    }
    ok("corbaloc: and corbaname: resolve exactly as to_ior alone does — unchanged");

    // ── 7. step 2: the same peer, named either way, is the same reference ──
    // `--peer` went through `string_to_object`, so whichever form was typed,
    // `object_to_string` of the result must denote the same endpoint. This is
    // §8.2.2's round trip on a reference that came from a live peer rather than
    // from a literal in a test.
    let restringified = orb.object_to_string(&ns_ior)?;
    let reread = orb.string_to_object(&restringified)?;
    if reread.profiles != ns_ior.profiles {
        return Err("object_to_string then string_to_object changed the endpoint".into());
    }
    ok("object_to_string/string_to_object round-tripped the peer's own reference");

    Ok(())
}
