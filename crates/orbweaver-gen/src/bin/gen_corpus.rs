//! `gen-corpus` — generate a standalone crate of stubs from a set of IDL files.
//!
//! ```text
//! gen-corpus --out <dir> --workspace <path-to-orbweaver> [-I <dir>]... <file.idl>...
//! ```
//!
//! `-I` is `sidl-validate`'s flag and means the same thing: another directory
//! to resolve `#include` against. The quoted form searches the including file's
//! own directory first, so an estate stored as one tree needs no `-I` at all.
//!
//! # Each file is a translation unit, not a string
//!
//! A stub is the caller's half of a contract, so it has to be generated from
//! everything the contract says — including the part a header says. Read as a
//! string, an interface whose base is declared next door arrives with no
//! ancestry, and the client stub is emitted **without the inherited
//! operations**: a method the peer would have answered simply does not exist in
//! the generated crate, and nothing anywhere says one is missing. The identity
//! half is worse, because it is silent in the other direction — a
//! `#pragma prefix` in a shared header is part of every repository id after it,
//! and a stub built without it sends `_is_a` and `GIOP::Request` type ids no
//! deployed object answers to.
//!
//! *스텁은 계약의 절반이므로 번역 단위 전체에서 생성되어야 한다. 문자열로 읽으면
//! 상속된 연산이 조용히 빠지고, 헤더의 `#pragma prefix`가 빠진 저장소 ID는 어떤
//! 객체도 응답하지 않는 ID다.*
//!
//! The output is a crate **outside** the workspace, because that is what a
//! consumer of generated code is: the harness compiling it proves the stubs
//! stand on the published crate surface alone, not on being inside the tree.
//! Each interface contributes both halves — the client stub and the server
//! skeleton — and every generated module declares `#![forbid(unsafe_code)]`
//! and `#![deny(missing_docs)]`, so compiling this crate is also where those
//! two rules are proved to hold for generated code.
//!
//! The crate also contains `src/main.rs`, the stream-B oracle for the echo
//! fixture: byte-equality between the static and dynamic marshalling of the
//! same values, then live calls through the generated stub. That file is a
//! fixed template, not generated — it is the *test* of the generator, and a
//! test the generator writes for itself proves nothing.

use std::fmt::Write as _;
use std::path::Path;

use orbweaver_registry::{Contract, Registry, Strictness, take_include_dirs};

fn main() -> std::process::ExitCode {
    let mut out_dir: Option<String> = None;
    let mut workspace: Option<String> = None;
    let mut files: Vec<String> = Vec::new();

    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    let search = match take_include_dirs(&mut argv) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    };
    let mut args = argv.into_iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out_dir = args.next(),
            "--workspace" => workspace = args.next(),
            other => files.push(other.to_owned()),
        }
    }
    let (Some(out_dir), Some(workspace), false) = (out_dir, workspace, files.is_empty()) else {
        eprintln!(
            "usage: gen-corpus --out <dir> --workspace <orbweaver root> [-I <dir>]... <file.idl>..."
        );
        return std::process::ExitCode::from(2);
    };

    let src_dir = Path::new(&out_dir).join("src");
    if let Err(e) = std::fs::create_dir_all(&src_dir) {
        eprintln!("{out_dir}: {e}");
        return std::process::ExitCode::from(2);
    }

    let mut lib = String::new();
    let mut emitted = 0usize;
    let mut skipped: Vec<(String, String)> = Vec::new();
    let mut failed = 0usize;

    for path in &files {
        let stem = Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().replace(['-', '.'], "_"))
            .unwrap_or_default();
        let module = format!("f_{stem}");

        // A path this process cannot open at all is "could not run" and keeps
        // exit 2, because `Contract::load` folds an unreadable root and a
        // rejected contract into one error and a mistyped filename must not
        // read as a defective contract.
        if let Err(e) = std::fs::File::open(path) {
            eprintln!("{path}: {e}");
            return std::process::ExitCode::from(2);
        }
        // The gate first, over the whole translation unit: generating from IDL
        // that S4 rejects would produce stubs describing calls nobody can make,
        // and generating from a file read as a string would produce stubs
        // missing every operation an included header declares.
        let contract = match Contract::load(Path::new(path), &search, Strictness::Checked) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{path}: rejected by the front end:");
                for line in e.message.lines().take(3) {
                    eprintln!("  {line}");
                }
                failed += 1;
                continue;
            }
        };
        let mut registry = Registry::new();
        if let Err(e) = registry.load(&contract.spec) {
            eprintln!("{path}: {e}");
            failed += 1;
            continue;
        }
        // Deliberately **not** gated on `Registry::unresolved()`, which
        // `idl-diff` refuses on. It reports names the registry could not
        // resolve, and its resolver does not search an inherited interface's
        // scope: `corpus/services/gen-naming-subset.idl` raises `NotFound`
        // from `NamingContextExt : NamingContext`, which is legal IDL both
        // oracles accept and four `Unresolved` markers here. Gating on it
        // would refuse a contract this generator emits correctly today. The
        // include class is already covered — `Contract::load` refuses an
        // `#include` that resolves to nothing, with the file name.
        let generated = orbweaver_gen::emit(&registry, &module);
        emitted += generated.emitted;
        skipped.extend(generated.skipped.iter().map(|(id, why)| (id.clone(), why.clone())));

        let file_name = format!("{module}.rs");
        if let Err(e) = std::fs::write(src_dir.join(&file_name), &generated.source) {
            eprintln!("{file_name}: {e}");
            return std::process::ExitCode::from(2);
        }
        let _ = writeln!(lib, "pub mod {module};");
    }

    let cargo_toml = format!(
        r#"# Generated by gen-corpus. A deliberately standalone crate: compiling it
# proves the stubs stand on the published crate surface alone.
[package]
name = "orbweaver-genout"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]

[dependencies]
orbweaver-cdr = {{ path = "{ws}/crates/orbweaver-cdr" }}
orbweaver-giop = {{ path = "{ws}/crates/orbweaver-giop", default-features = false }}
orbweaver-dynamic = {{ path = "{ws}/crates/orbweaver-dynamic" }}
orbweaver-registry = {{ path = "{ws}/crates/orbweaver-registry" }}
orbweaver-idl = {{ path = "{ws}/crates/orbweaver-idl" }}
orbweaver-gen = {{ path = "{ws}/crates/orbweaver-gen" }}
orbweaver-mcp = {{ path = "{ws}/crates/orbweaver-mcp" }}

[[bin]]
name = "static-oracle"
path = "src/main.rs"
"#,
        ws = workspace
    );

    let ok = std::fs::write(Path::new(&out_dir).join("Cargo.toml"), cargo_toml).is_ok()
        && std::fs::write(src_dir.join("lib.rs"), &lib).is_ok()
        && std::fs::write(src_dir.join("main.rs"), ORACLE_MAIN).is_ok();
    if !ok {
        eprintln!("could not write the generated crate");
        return std::process::ExitCode::from(2);
    }

    println!("generated {emitted} item(s) from {} file(s) into {out_dir}", files.len());
    for (id, why) in &skipped {
        println!("skipped {id}: {why}");
    }
    if failed > 0 {
        println!("{failed} file(s) failed");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

/// The stream-B oracle. Fixed text, never generated: a test the generator
/// writes for itself proves nothing.
const ORACLE_MAIN: &str = r####"//! Stream B's oracle: static equals dynamic, then the wire.
//!
//! Usage: static-oracle <ior-file> <echo.idl>

mod f_echo;

use std::time::Duration;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_dynamic::Value;
use orbweaver_gen::rt::{AnyVal, Cdr, ObjRef, WString};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_giop::{Connection, Ior};
use orbweaver_idl::include::SearchPath;
use orbweaver_registry::{Contract, Registry, Strictness};

use f_echo::spike::{EchoClient, Ragged};

const OK: &str = "ok  ";
const NO: &str = "FAIL";

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [ior_path, idl_path] = args.as_slice() else {
        eprintln!("usage: static-oracle <ior-file> <echo.idl>");
        return std::process::ExitCode::from(2);
    };
    match run(ior_path, idl_path) {
        Ok(0) => {
            println!("\nstatic generation: PASS — static equals dynamic, and the wire agrees");
            std::process::ExitCode::SUCCESS
        }
        Ok(n) => {
            println!("\nstatic generation: FAIL — {n} case(s)");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            println!("  {NO} {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn ragged() -> Ragged {
    Ragged { a: 0xAA, b: -7, c: 9, d: 2.5, e: 0xBB }
}

fn ragged_value() -> Value {
    Value::Struct(vec![
        ("a".into(), Value::Octet(0xAA)),
        ("b".into(), Value::Long(-7)),
        ("c".into(), Value::Short(9)),
        ("d".into(), Value::Double(2.5)),
        ("e".into(), Value::Octet(0xBB)),
    ])
}

fn run(ior_path: &str, idl_path: &str) -> Result<u32, String> {
    let mut fails = 0u32;

    // ── The §8 rule, byte for byte ───────────────────────────────────────────
    // The dynamic path is the reference implementation: it is the one verified
    // against two independent ORBs. A static stub is correct exactly when its
    // bytes equal the dynamic bytes for the same value.
    // The same translation unit `gen-corpus` generated the stubs from. If this
    // read the file as a string while the generator resolved its includes, the
    // two sides of the byte comparison would be built from different contracts
    // and the oracle would be comparing the wrong two things.
    let path = std::path::Path::new(idl_path);
    let contract = Contract::load(path, &SearchPath::new(), Strictness::Grammar)
        .map_err(|e| e.message)?;
    let mut registry = Registry::new();
    registry.load(&contract.spec).map_err(|e| e.to_string())?;
    let ragged_tc = registry.typecode("IDL:spike/Ragged:1.0").ok_or("Ragged missing")?;

    println!("── static bytes versus dynamic bytes ──");
    let cases: Vec<(&str, Box<dyn Fn(&mut Encoder) -> bool>, TypeCode, Value)> = vec![
        (
            "Ragged (every alignment rule at once)",
            Box::new(|e| ragged().put(e).is_ok()),
            ragged_tc.clone(),
            ragged_value(),
        ),
        (
            "wstring with Korean text",
            Box::new(|e| WString("동적 호출".into()).put(e).is_ok()),
            TypeCode::WString(0),
            Value::WString("동적 호출".into()),
        ),
        (
            "any carrying a double",
            Box::new(|e| AnyVal(TypeCode::Double, Value::Double(-0.125)).put(e).is_ok()),
            TypeCode::Any,
            Value::Any(Box::new(TypeCode::Double), Box::new(Value::Double(-0.125))),
        ),
        (
            "sequence<octet>",
            Box::new(|e| (0..64u8).collect::<Vec<u8>>().put(e).is_ok()),
            TypeCode::Sequence { element: Box::new(TypeCode::Octet), bound: 0 },
            Value::List((0..64u8).map(Value::Octet).collect()),
        ),
    ];
    for (what, put_static, tc, dv) in &cases {
        for endian in [Endian::Big, Endian::Little] {
            let mut a = Encoder::new(endian);
            if !put_static(&mut a) {
                println!("  {NO} {what}: static put failed");
                fails += 1;
                continue;
            }
            let mut b = Encoder::new(endian);
            orbweaver_dynamic::encode(&mut b, tc, dv).map_err(|e| e.to_string())?;
            let (sa, sb) = (a.finish().map_err(|e| e.to_string())?, b.finish().map_err(|e| e.to_string())?);
            if sa == sb {
                println!("  {OK} {what} ({endian:?}): {} identical byte(s)", sa.len());
            } else {
                println!("  {NO} {what} ({endian:?}): static {sa:02x?} != dynamic {sb:02x?}");
                fails += 1;
            }
        }
    }

    // ── The wire, through the generated stub ───────────────────────────────
    let text = std::fs::read_to_string(ior_path).map_err(|e| format!("{ior_path}: {e}"))?;
    let ior = Ior::parse(text.trim()).map_err(|e| e.to_string())?;

    for endian in [Endian::Big, Endian::Little] {
        println!("── the generated stub against a stock ORB ({endian:?}) ──");
        let mut conn = Connection::connect(&ior, Duration::from_secs(5)).map_err(|e| e.to_string())?;
        conn.set_endian(endian);
        let mut client = EchoClient::new(conn);

        let mut case = |what: &str, pass: bool| {
            if pass {
                println!("  {OK} {what}");
            } else {
                println!("  {NO} {what}");
                fails += 1;
            }
        };
        case("ping() -> 42", client.ping().map(|v| v == 42).unwrap_or(false));
        case(
            "add(1000000, 337) -> 1000337",
            client.add(1_000_000, 337).map(|v| v == 1_000_337).unwrap_or(false),
        );
        case(
            "echo_string round trip",
            client
                .echo_string("static stub".into())
                .map(|v| v == "static stub")
                .unwrap_or(false),
        );
        case(
            "scale(1.5, 4.0) -> 6.0",
            client.scale(1.5, 4.0).map(|v| (v - 6.0).abs() < 1e-9).unwrap_or(false),
        );
        case(
            "echo_ragged round trip",
            client.echo_ragged(ragged()).map(|v| v == ragged()).unwrap_or(false),
        );
        case(
            "echo_wstring round trip (Korean)",
            client
                .echo_wstring(WString("정적 스텁".into()))
                .map(|v| v == WString("정적 스텁".into()))
                .unwrap_or(false),
        );
        case(
            "blob_sum(0..64) -> 2016",
            client.blob_sum((0..64u8).collect()).map(|v| v == 2016).unwrap_or(false),
        );
        case(
            "echo_any round trip",
            client
                .echo_any(AnyVal(TypeCode::Double, Value::Double(-0.125)))
                .map(|v| v == AnyVal(TypeCode::Double, Value::Double(-0.125)))
                .unwrap_or(false),
        );
        let self_ref = client.get_self();
        case("get_self() -> a non-nil reference", matches!(&self_ref, Ok(ObjRef(Some(_)))));
        if let Ok(r) = self_ref {
            case("same_as(that reference) -> true", client.same_as(r).unwrap_or(false));
        }
    }

    // ── I1: the same stub, on the other side of the trust boundary ─────────
    // PLAN §7.4: a stub that bypasses the guard recreates the §4.7 bypass in
    // compiled form. Here the identical generated code runs over the guarded
    // invoker, and the checks the dynamic path runs bind it per operation.
    println!("── the same generated stub, through the guard (I1) ──");
    use orbweaver_mcp::Bridge;
    use orbweaver_mcp::identity::Caller;
    use orbweaver_mcp::policy::{Approval, Exposure};

    let exposure = Exposure::nothing()
        .allow_operation("IDL:spike/Echo:1.0", "ping")
        .allow_operation("IDL:spike/Echo:1.0", "add")
        .allow_operation("IDL:spike/Echo:1.0", "blob_sum");
    let mut check = |what: &str, pass: bool| {
        if pass {
            println!("  {OK} {what}");
        } else {
            println!("  {NO} {what}");
            fails += 1;
        }
    };

    let mut bridge = Bridge::new(&registry, exposure.clone(), "static-session")
        .on_behalf_of(Caller::new("alice@example.com").with_scope("echo:blob"));
    let handle = bridge
        .handles()
        .issue_checked(&ior)
        .map_err(|e| e.to_string())?;
    let guarded = bridge
        .connect_static(handle.as_str(), Approval::default(), Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let mut gclient = EchoClient::new(guarded);

    check("ping() through the guard -> 42", gclient.ping().map(|v| v == 42).unwrap_or(false));
    check(
        "blob_sum() allowed: the caller holds the echo:blob scope the contract asks for",
        gclient.blob_sum((0..64u8).collect()).map(|v| v == 2016).unwrap_or(false),
    );
    let refused = gclient.echo_string("x".into());
    check(
        "echo_string() refused as NO_PERMISSION: not among the allowed operations",
        matches!(&refused, Err(orbweaver_giop::Error::SystemException { id, .. })
            if id.contains("NO_PERMISSION")),
    );
    check(
        "and the connection still works after the refusal",
        gclient.add(40, 2).map(|v| v == 42).unwrap_or(false),
    );

    // A caller without the scope, same exposure: the contract's ai_authz line
    // binds the static path exactly as it binds the dynamic one.
    let mut bob = Bridge::new(&registry, exposure, "bob-session")
        .on_behalf_of(Caller::new("bob@example.com"));
    let bh = bob.handles().issue_checked(&ior).map_err(|e| e.to_string())?;
    let bg = bob
        .connect_static(bh.as_str(), Approval::default(), Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let mut bclient = EchoClient::new(bg);
    check(
        "the same call without the scope is refused before it is sent",
        bclient.blob_sum(vec![1, 2, 3]).is_err(),
    );

    // The audit trail, and the leak check over it: which principal, which
    // operation, and nothing dialable.
    let audit = gclient.conn.audit().join("\n") + "\n" + &bclient.conn.audit().join("\n");
    check(
        "the audit names principals and operations",
        audit.contains("ALLOW caller=alice@example.com")
            && audit.contains("operation=blob_sum")
            && audit.contains("REFUSE caller=bob@example.com"),
    );
    let profile = ior.primary().map_err(|e| e.to_string())?;
    let key_text = String::from_utf8_lossy(&profile.object_key).into_owned();
    let mut leaked = false;
    for needle in [profile.host.as_str(), key_text.as_str(), "IOR:"] {
        if needle.len() >= 3 && audit.contains(needle) {
            println!("  {NO} {needle:?} appears in the audit log");
            leaked = true;
        }
    }
    if leaked {
        fails += 1;
    } else {
        println!("  {OK} the audit log contains no host, object key or IOR");
    }

    // ── I4: promotion respects identity, against the live peer ─────────────
    // PLAN §7.4 I4: a promoted static path carries the same Caller assertion
    // behaviour as the dynamic path it replaced. verify_promotion is already
    // unit-verified with fakes; this section is its live half — both paths'
    // real outcomes against the same stock ORB, and the static path's real
    // Guarded audit line, fed through the gate.
    println!("── promotion respects identity (I4) ──");
    use std::collections::BTreeMap;

    use orbweaver_dynamic::invoke::Outcome;
    use orbweaver_mcp::promote::{self, CallStats, PromotionPolicy, PromotionRegression};

    let mut check = |what: &str, pass: bool| {
        if pass {
            println!("  {OK} {what}");
        } else {
            println!("  {NO} {what}");
            fails += 1;
        }
    };

    // ping/add carry no ai_authz, so the no-caller session below still
    // succeeds at the wire — which is what makes the negative control real:
    // the wire answers 42 either way, and only the gate can tell the caller
    // was lost.
    let i4_exposure = Exposure::nothing()
        .allow_operation("IDL:spike/Echo:1.0", "ping")
        .allow_operation("IDL:spike/Echo:1.0", "add");

    // Bridge::stats() fills only through Bridge::invoke — the JSON tool path.
    // Every I4 call bypasses it (invoke::invoke on a raw connection, and
    // Guarded stubs from connect_static), so the traffic is recorded into a
    // local CallStats; wiring the static path into the bridge's own stats
    // belongs to lib.rs's owner, not to this oracle.
    let mut stats = CallStats::new();

    // The dynamic path: the reference implementation, add(40, 2) through the
    // same invoke() the bridge's JSON path uses, over a fresh plain connection.
    let mut dconn =
        Connection::connect(&ior, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    let mut dargs = BTreeMap::new();
    dargs.insert("a".to_owned(), Value::Long(40));
    dargs.insert("b".to_owned(), Value::Long(2));
    let dynamic_outcome = orbweaver_dynamic::invoke::invoke(
        &mut dconn,
        &registry,
        "IDL:spike/Echo:1.0",
        "add",
        &dargs,
    );
    stats.record("IDL:spike/Echo:1.0", "add", dynamic_outcome.is_ok());
    let dynamic_outcome = dynamic_outcome.map_err(|e| e.to_string())?;

    // The dynamic session this promotion would replace, on behalf of alice.
    // The audit line is CAPTURED, not reconstructed: since the bridge started
    // emitting real ALLOW lines, the same call runs once through the bridge's
    // own JSON path and the line is taken from Bridge::audit() — closing the
    // seam this section used to carry as an honesty note.
    let mut dynamic_bridge = Bridge::new(&registry, i4_exposure.clone(), "i4-dynamic")
        .on_behalf_of(Caller::new("alice@example.com"));
    let dhandle = dynamic_bridge.handles().issue_checked(&ior).map_err(|e| e.to_string())?;
    let dyn_json = orbweaver_dynamic::json::Json::parse(r#"{"a":40,"b":2}"#).expect("static json");
    dynamic_bridge
        .invoke(&mut dconn, dhandle.as_str(), "add", &dyn_json, Approval::default())
        .map_err(|e| e.to_string())?;
    let dynamic_audit = dynamic_bridge
        .audit()
        .last()
        .cloned()
        .ok_or("the dynamic bridge wrote no audit line")?;

    // The static path: the same operation through the generated stub over
    // connect_static, in a session on behalf of the same principal. Its audit
    // line is the one the guard actually wrote, taken via .audit().
    let mut alice = Bridge::new(&registry, i4_exposure.clone(), "i4-static")
        .on_behalf_of(Caller::new("alice@example.com"));
    let ah = alice.handles().issue_checked(&ior).map_err(|e| e.to_string())?;
    let ag = alice
        .connect_static(ah.as_str(), Approval::default(), Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let mut aclient = EchoClient::new(ag);
    let kept = aclient.add(40, 2);
    stats.record("IDL:spike/Echo:1.0", "add", kept.is_ok());
    let kept = kept.map_err(|e| e.to_string())?;
    // The stub answers a bare i32; the gate compares Outcomes, so lift it
    // into the dynamic shape: returns Value::Long, no out or inout outputs.
    let static_outcome = Outcome { returns: Value::Long(kept), outputs: BTreeMap::new() };
    let static_audit =
        aclient.conn.audit().last().cloned().ok_or("the guard wrote no audit line")?;

    let verdict = promote::verify_promotion(
        &dynamic_outcome,
        &static_outcome,
        &dynamic_audit,
        &static_audit,
    );
    check("I4: a live promotion passes the gate when identity is preserved", verdict.is_ok());
    if let Err(e) = &verdict {
        println!("       {e}");
    }

    // The same promotion rebuilt without a caller — the optimization that
    // forgot on_behalf_of. The wire still answers 42; the gate must refuse
    // anyway, and must say why by name.
    let mut nobody = Bridge::new(&registry, i4_exposure, "i4-nobody");
    let nh = nobody.handles().issue_checked(&ior).map_err(|e| e.to_string())?;
    let ng = nobody
        .connect_static(nh.as_str(), Approval::default(), Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    let mut nclient = EchoClient::new(ng);
    let lost = nclient.add(40, 2);
    stats.record("IDL:spike/Echo:1.0", "add", lost.is_ok());
    let lost = lost.map_err(|e| e.to_string())?;
    let lost_outcome = Outcome { returns: Value::Long(lost), outputs: BTreeMap::new() };
    let lost_audit =
        nclient.conn.audit().last().cloned().ok_or("the guard wrote no audit line")?;
    let refused = promote::verify_promotion(
        &dynamic_outcome,
        &lost_outcome,
        &dynamic_audit,
        &lost_audit,
    );
    check(
        "I4: the gate refuses a promotion that lost the caller, results identical",
        lost_outcome == dynamic_outcome
            && matches!(&refused,
                Err(PromotionRegression::IdentityDropped { static_caller, .. })
                    if static_caller == "<nobody>"),
    );

    // And the recommendation plumbing sees the same real traffic: three add
    // calls were recorded around the wire calls above.
    let policy = PromotionPolicy { min_calls: 3, max_failure_rate: 0.0 };
    check(
        "I4: after 3 recorded live calls the policy recommends (IDL:spike/Echo:1.0, add)",
        policy
            .recommend(&stats)
            .iter()
            .any(|c| c.id == "IDL:spike/Echo:1.0" && c.operation == "add"),
    );

    Ok(fails)
}
"####;
