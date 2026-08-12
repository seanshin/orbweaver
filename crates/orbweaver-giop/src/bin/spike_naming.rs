//! Resolves a target through a real CosNaming service and calls it.
//!
//! Everything before this found targets by reading a stringified IOR out of a
//! file. This is how a deployment actually does it: a URL, a naming service,
//! and a reference that came back over the wire.
//!
//! Usage: `spike-naming <corbaname-url>`

use orbweaver_giop::naming::{NamingContext, ObjectUrl};
use orbweaver_giop::{Connection, Ior};
use std::time::Duration;

fn main() -> std::process::ExitCode {
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "corbaname::127.0.0.1:2809/NameService#spike/Echo".into());
    match run(&url) {
        Ok(()) => {
            println!("\nnaming: PASS — resolved and invoked through CosNaming");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            println!("\nnaming: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("url        {url}");
    let parsed = ObjectUrl::parse(url)?;

    let (addresses, key, name) = match &parsed {
        ObjectUrl::Corbaname { addresses, object_key, name } => (addresses, object_key, name),
        other => return Err(format!("expected a corbaname URL, got {other:?}").into()),
    };
    println!(
        "naming svc {}:{} (GIOP {}.{}), key {:?}",
        addresses[0].host,
        addresses[0].port,
        addresses[0].version.major,
        addresses[0].version.minor,
        String::from_utf8_lossy(key)
    );
    println!("path       {}", orbweaver_giop::naming::stringify_name(name));

    let mut ctx = NamingContext::from_url(&parsed, Duration::from_secs(5))?;
    println!("  ok   connected to the naming context");

    // resolve() takes a structured Name; resolve_str() takes the string form.
    // Both are exercised because a peer may implement only one well.
    let ior: Ior = ctx.resolve(name)?;
    println!("  ok   resolve() returned {} ({} profile(s))", ior.type_id, ior.profiles.len());

    let via_str = ctx.resolve_str(&orbweaver_giop::naming::stringify_name(name))?;
    if via_str.primary()?.object_key != ior.primary()?.object_key {
        return Err("resolve() and resolve_str() disagreed about the target".into());
    }
    println!("  ok   resolve_str() agreed with resolve()");

    // The point of resolving is calling, so the reference must actually work.
    let mut conn = Connection::connect(&ior, Duration::from_secs(5))?;
    let n = conn.invoke_nullary("ping")?.body()?.get_i32()?;
    if n != 42 {
        return Err(format!("ping() through the resolved reference returned {n}").into());
    }
    println!("  ok   ping() through the resolved reference -> 42");
    Ok(())
}
