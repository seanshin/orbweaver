//! A whole agent session, end to end, against an ORB we did not write.
//!
//! Search a catalog, read a contract, make a call, receive an object reference,
//! call *through* that reference — with nothing generated, and with every
//! message in and out being JSON.
//!
//! The load-bearing assertion is not that the calls work. It is that **no
//! endpoint ever appears in anything the agent sees**. §4.7 makes an IOR a
//! bearer address: an agent holding one can reach the target directly, past
//! authorisation, past `destructive` approval, past the audit log. So this
//! spike records every byte of JSON the session produces and then searches the
//! transcript for the host, the port and the object key. A leak is a failure
//! even when every call succeeded — especially then, because that is the shape
//! it would ship in.
//!
//! Usage: `spike-mcp <ior-file> <idl-file> <interface-id>`

use std::time::Duration;

use orbweaver_dynamic::json::Json;
use orbweaver_giop::{Connection, Ior};
use orbweaver_mcp::Bridge;
use orbweaver_mcp::handles::CapabilityTable;
use orbweaver_mcp::policy::{Approval, Exposure};
use orbweaver_registry::Registry;

const OK: &str = "ok  ";
const NO: &str = "FAIL";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [ior_path, idl_path, interface_id] = args.as_slice() else {
        eprintln!("usage: spike-mcp <ior-file> <idl-file> <interface-id>");
        return std::process::ExitCode::from(2);
    };
    match run(ior_path, idl_path, interface_id) {
        Ok(0) => {
            println!("\nMCP bridge: PASS — an agent session with no address in it");
            std::process::ExitCode::SUCCESS
        }
        Ok(n) => {
            println!("\nMCP bridge: FAIL — {n} case(s)");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            println!("  {NO} {e}");
            println!("\nMCP bridge: FAIL");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Everything the agent was shown, kept so it can be searched afterwards.
#[derive(Default)]
struct Transcript(Vec<String>);

impl Transcript {
    fn record(&mut self, j: &Json) -> String {
        let text = j.to_string();
        self.0.push(text.clone());
        text
    }

    fn note(&mut self, text: impl Into<String>) {
        self.0.push(text.into());
    }

    fn contains(&self, needle: &str) -> bool {
        self.0.iter().any(|t| t.contains(needle))
    }
}

fn run(ior_path: &str, idl_path: &str, interface_id: &str) -> Result<u32, String> {
    let src = std::fs::read_to_string(idl_path).map_err(|e| format!("{idl_path}: {e}"))?;
    let spec = orbweaver_idl::parse(&src).map_err(|e| format!("{idl_path}: {e}"))?;
    let mut registry = Registry::new();
    registry.load(&spec).map_err(|e| e.to_string())?;

    let text = std::fs::read_to_string(ior_path).map_err(|e| format!("{ior_path}: {e}"))?;
    let ior = Ior::parse(text.trim()).map_err(|e| e.to_string())?;
    let profile = ior.primary().map_err(|e| e.to_string())?.clone();
    let mut conn = Connection::connect(&ior, Duration::from_secs(5)).map_err(|e| e.to_string())?;

    let mut fails = 0u32;
    let mut t = Transcript::default();

    // ── The default. Nothing is reachable until somebody says so. ───────────
    println!("── before anything is allowlisted ──");
    let closed = Bridge::new(&registry, Exposure::nothing(), "session-0");
    let empty = closed.search("", 10);
    if empty.get("matched") == Some(&Json::Number("0".into())) {
        println!(
            "  {OK} search finds nothing in a catalog of {} interface(s)",
            registry.ids().count()
        );
    } else {
        println!("  {NO} a default-deny bridge returned {empty}");
        fails += 1;
    }
    if closed.describe(interface_id).is_err() {
        println!("  {OK} describe refuses an interface nobody exposed");
    } else {
        println!("  {NO} describe answered for an unexposed interface");
        fails += 1;
    }

    // ── An operator allowlists part of one interface. ───────────────────────
    let exposure = Exposure::nothing()
        .allow_operation(interface_id, "ping")
        .allow_operation(interface_id, "add")
        .allow_operation(interface_id, "get_self")
        .allow_operation(interface_id, "same_as");

    println!("\n── the agent's session ──");
    let mut bridge = Bridge::new(&registry, exposure.clone(), "session-1")
        .with_handles(CapabilityTable::new("session-1"));
    let hits = bridge.search("echo", 10);
    let hits_text = t.record(&hits);
    if hits.get("matched") == Some(&Json::Number("1".into())) {
        println!("  {OK} search_interfaces(\"echo\") -> 1 interface");
    } else {
        println!("  {NO} search_interfaces returned {hits_text}");
        fails += 1;
    }

    match bridge.describe(interface_id) {
        Ok(d) => {
            let text = t.record(&d);
            let hidden = ["echo_string", "blob", "echo_wstring"];
            let leaked: Vec<&str> = hidden.iter().copied().filter(|o| text.contains(o)).collect();
            if leaked.is_empty() {
                println!("  {OK} describe_interface shows 4 operations and hides the rest");
            } else {
                println!("  {NO} unexposed operations appeared: {leaked:?}");
                fails += 1;
            }
            if text.contains("cheapest liveness check") {
                println!("  {OK} the contract's own prose reached the agent");
            } else {
                println!("  {NO} SIDL annotations did not survive: {text}");
                fails += 1;
            }
        }
        Err(e) => {
            println!("  {NO} describe_interface: {e}");
            fails += 1;
        }
    }

    // The agent is handed one capability to start from, exactly as a bridge
    // would after resolving a name on its behalf.
    let root = match bridge.handles().issue_checked(&ior) {
        Ok(h) => h,
        Err(e) => return Err(e.to_string()),
    };
    t.note(root.to_string());
    println!("  {OK} the agent holds {root}");

    let mut call = |bridge: &mut Bridge<'_>,
                    t: &mut Transcript,
                    handle: &str,
                    op: &str,
                    args: &str,
                    check: &dyn Fn(&Json) -> bool| {
        let parsed = Json::parse(args).expect("the spike's own JSON is valid");
        match bridge.invoke(&mut conn, handle, op, &parsed, Approval::default()) {
            Ok(result) => {
                let text = t.record(&result);
                if check(&result) {
                    println!("  {OK} invoke_operation({op}) -> {text}");
                    Some(result)
                } else {
                    println!("  {NO} invoke_operation({op}) -> {text}");
                    fails += 1;
                    None
                }
            }
            Err(e) => {
                println!("  {NO} invoke_operation({op}): {e}");
                fails += 1;
                None
            }
        }
    };

    call(&mut bridge, &mut t, root.as_str(), "ping", "{}", &|r| {
        r.get("returns") == Some(&Json::Number("42".into()))
    });
    call(&mut bridge, &mut t, root.as_str(), "add", r#"{"a":40,"b":2}"#, &|r| {
        r.get("returns") == Some(&Json::Number("42".into()))
    });

    // A reference comes back. It must arrive as a handle, and it must work.
    let derived = call(&mut bridge, &mut t, root.as_str(), "get_self", "{}", &|r| {
        r.get("returns").and_then(|v| v.get("_ref")).and_then(Json::as_str).is_some()
    });
    if let Some(r) = derived {
        let handle = r
            .get("returns")
            .and_then(|v| v.get("_ref"))
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_owned();
        // Passing a handle back in is the whole point of §4.7: the agent can
        // name a reference it was given and still cannot dial it.
        call(
            &mut bridge,
            &mut t,
            root.as_str(),
            "same_as",
            &format!(r#"{{"other":{{"_ref":"{handle}"}}}}"#),
            &|r| r.get("returns") == Some(&Json::Bool(true)),
        );
    }

    // ── Refusals ────────────────────────────────────────────────────────────
    println!("\n── what the bridge refuses ──");
    for (what, op, args, expect) in [
        ("an operation nobody exposed", "echo_string", r#"{"msg":"hi"}"#, "not among its allowed"),
        (
            "an argument the contract has no name for",
            "add",
            r#"{"a":1,"b":2,"c":3}"#,
            "no parameter",
        ),
        ("an argument left out", "add", r#"{"a":1}"#, "needs an argument"),
    ] {
        let parsed = Json::parse(args).unwrap();
        match bridge.invoke(&mut conn, root.as_str(), op, &parsed, Approval::default()) {
            Err(e) if e.to_string().contains(expect) => println!("  {OK} {what}: {e}"),
            Err(e) => {
                println!("  {NO} {what}: expected {expect:?}, got {e}");
                fails += 1;
            }
            Ok(v) => {
                println!("  {NO} {what} was allowed: {v}");
                fails += 1;
            }
        }
    }

    // A destructive operation is refused even when it is exposed. Checked
    // against a contract that declares one, because the fixture honestly has
    // none — the gate is a local decision and needs no peer.
    let destructive_src =
        "module vault { interface Store { //@ ai_effect: destructive\n void erase(); }; };";
    let dspec = orbweaver_idl::parse(destructive_src).map_err(|e| e.to_string())?;
    let mut dreg = Registry::new();
    dreg.load(&dspec).map_err(|e| e.to_string())?;
    let dexp = Exposure::nothing().allow_interface("IDL:vault/Store:1.0");
    match dexp.check_call(&dreg, "IDL:vault/Store:1.0", "erase", Approval::default(), None) {
        Err(e) => println!("  {OK} a destructive operation, exposed: {e}"),
        Ok(()) => {
            println!("  {NO} a destructive operation was callable without approval");
            fails += 1;
        }
    }
    if dexp
        .check_call(
            &dreg,
            "IDL:vault/Store:1.0",
            "erase",
            Approval { destructive_approved: true },
            None,
        )
        .is_ok()
    {
        println!("  {OK} and callable once a human has approved it");
    } else {
        println!("  {NO} approval did not unblock it");
        fails += 1;
    }

    // A handle from another session must be worthless.
    let other = CapabilityTable::new("session-2");
    if other.resolve_public(root.as_str()).is_none() {
        println!("  {OK} the handle is worthless in another session");
    } else {
        println!("  {NO} a handle crossed a session boundary");
        fails += 1;
    }

    // ── The assertion this spike exists for ─────────────────────────────────
    println!("\n── the transcript, searched for anything dialable ──");
    let key_hex: String = profile.object_key.iter().map(|b| format!("{b:02x}")).collect();
    let key_text = String::from_utf8_lossy(&profile.object_key).into_owned();
    let leaks: Vec<(&str, String)> = vec![
        ("the host", profile.host.clone()),
        ("the port", profile.port.to_string()),
        ("the object key", key_hex),
        ("the object key as text", key_text),
        ("a stringified IOR", "IOR:".to_owned()),
    ];
    let mut leaked = false;
    for (what, needle) in leaks {
        // A short or empty needle would match everything or nothing; either way
        // it would not be a measurement.
        if needle.len() < 3 {
            continue;
        }
        if t.contains(&needle) {
            println!("  {NO} {what} ({needle}) appears in what the agent was shown");
            leaked = true;
        }
    }
    if leaked {
        fails += 1;
    } else {
        println!("  {OK} {} JSON message(s) contain no host, port, object key or IOR", t.0.len());
    }

    Ok(fails)
}

/// A session's own view of a handle, for the cross-session check.
trait ResolvePublic {
    fn resolve_public(&self, handle: &str) -> Option<Ior>;
}

impl ResolvePublic for CapabilityTable {
    fn resolve_public(&self, handle: &str) -> Option<Ior> {
        orbweaver_dynamic::anyjson::References::resolve(self, handle)
    }
}
