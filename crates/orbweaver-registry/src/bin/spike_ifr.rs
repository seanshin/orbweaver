//! Self-consistency proof for the read-only Interface Repository facade: OUR
//! client against OUR server, one process, loopback.
//!
//! Drives the walk a foreign DII client makes — `lookup_id`, the `Contained`
//! accessors, `describe_interface`, `is_a` through two levels of inheritance,
//! `_get_base_interfaces` — and checks that every mutating operation is
//! refused with `NO_PERMISSION`. Self-consistency only: it proves the two
//! halves agree, not that either matches the specification. The independent
//! check is the python oracle below.
//!
//! Usage: `spike-ifr [ior-out-path] [idl-path...] [--hold]`
//!
//! Defaults are `spikes/ifr.ior` over golden 10 and golden 19, loaded into one
//! registry. Golden 10 is the inheritance case, which is what
//! `describe_interface` has to get right; golden 19 is the one with
//! parameters, `raises` and `oneway`, which is what
//! `OperationDescription`/`ParameterDescription` have to get right.
//!
//! # Cross-ORB oracle (measured, 2026-08-13)
//!
//! omniORBpy ships the IR stubs as `omniORB.ir_idl`, so a foreign client can
//! narrow to `CORBA::Repository` with no stubs of ours. Start
//! `spike-ifr spikes/ifr.ior --hold`, then run exactly:
//!
//! ```text
//! python3 -c "import sys, CORBA, omniORB.ir_idl; \
//! orb = CORBA.ORB_init(sys.argv); \
//! r = orb.string_to_object(open('spikes/ifr.ior').read().strip())._narrow(CORBA.Repository); \
//! d = r.lookup_id('IDL:gc10/Both:1.0')._narrow(CORBA.InterfaceDef).describe_interface(); \
//! print(d.name, d.id, [o.name for o in d.operations], [a.name for a in d.attributes], d.base_interfaces)"
//! ```
//!
//! Measured output against this servant (omniORB 4.3.4, omniORBpy, macOS):
//!
//! ```text
//! Both IDL:gc10/Both:1.0 ['touch', 'value'] ['id', 'name'] ['IDL:gc10/Derived:1.0', 'IDL:gc10/Nameable:1.0']
//! ```
//!
//! Substituting `IDL:tms/TrackManager:1.0` for the id exercises the
//! signature-carrying half through the same foreign ORB; see
//! `orbweaver_registry::ifr`'s module documentation for what that run
//! measured. That is the recommended harness snippet: one process holding,
//! two `python3 -c` invocations, both compared against fixed strings.
//!
//! One serving limit the harness must respect, inherited from `Server`:
//! `--hold` is stopped by killing the process, since `destroy` is refused and
//! there is no remote shutdown. The one-connection-at-a-time limit this note
//! used to carry ended with stream E.

use std::time::Duration;

use orbweaver_giop::server::{BAD_OPERATION, MARSHAL, Server};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_giop::{Connection, Error};
use orbweaver_registry::ifr::{
    self, DefinitionKind, FullInterfaceDescription, INTERFACE_DEF_ID, NO_PERMISSION,
    RepositoryServer,
};
use orbweaver_registry::{Registry, Strictness};

const T: Duration = Duration::from_secs(5);
const ROOT: &[u8] = b"InterfaceRepository";

/// The interface golden 10 defines with two bases and an inherited operation.
const SUBJECT: &str = "IDL:gc10/Both:1.0";

/// The interface golden 19 defines with parameters, `raises` and `oneway`.
const SIGNATURES: &str = "IDL:tms/TrackManager:1.0";

/// Loaded together into one registry when no IDL path is given.
const DEFAULT_IDL: [&str; 2] =
    ["corpus/golden/10-inheritance.idl", "corpus/golden/19-realistic-service.idl"];

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let hold = args.iter().any(|a| a == "--hold");
    let positional: Vec<&str> =
        args.iter().filter(|a| !a.starts_with("--")).map(String::as_str).collect();
    let ior_path = positional.first().copied().unwrap_or("spikes/ifr.ior");
    let idl_paths: Vec<&str> =
        if positional.len() > 1 { positional[1..].to_vec() } else { DEFAULT_IDL.to_vec() };

    match run(ior_path, &idl_paths, hold) {
        Ok(()) => {
            println!("\nifr-facade: PASS");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            println!("\nifr-facade: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

type Fallible = Result<(), Box<dyn std::error::Error>>;

fn ok(what: &str) {
    println!("  ok   {what}");
}

fn require(cond: bool, what: &str) -> Fallible {
    if cond { Ok(()) } else { Err(what.into()) }
}

/// Asserts a call failed with a particular system exception. A call that
/// *succeeded* is the interesting failure here: a mutating operation that
/// returned normally means the read-only promise is not being kept.
fn expect_system(result: Result<orbweaver_giop::Reply, Error>, want: &str, what: &str) -> Fallible {
    match result {
        Err(Error::SystemException { id, .. }) if id == want => Ok(()),
        Err(other) => Err(format!("{what}: expected {want}, got {other:?}").into()),
        Ok(_) => Err(format!("{what}: expected {want}, but the call SUCCEEDED").into()),
    }
}

/// `contents(limit_type, exclude_inherited = true)`, counted.
fn count_contents(
    conn: &mut Connection,
    limit: DefinitionKind,
) -> Result<u32, Box<dyn std::error::Error>> {
    let reply = conn.invoke("contents", |e| {
        e.put_u32(limit as u32);
        e.put_bool(true);
    })?;
    Ok(reply.body()?.get_u32()?)
}

fn run(ior_path: &str, idl_paths: &[&str], hold: bool) -> Fallible {
    // `Registry::load` accumulates, so several files become one repository —
    // which is what a repository is. Each file's `#include`s are resolved
    // first: a facade that served an interface with a base it could not find
    // would answer `_is_a` and `describe_interface` short, which is the answer
    // an IFR exists to give correctly.
    let registry: Registry = orbweaver_registry::registry_from_files(
        idl_paths,
        &orbweaver_idl::SearchPath::new(),
        Strictness::Grammar,
    )?;
    for path in idl_paths {
        println!("loaded {path}");
    }
    for u in registry.unresolved() {
        println!("  warning: {u}");
    }
    println!("repository holds {} interfaces", ifr::interface_ids(&registry).len());

    let server = Server::bind("127.0.0.1:0", ROOT.to_vec())?;
    let port = server.local_addr()?.port();
    let facade = RepositoryServer::new("127.0.0.1", port, ROOT.to_vec(), registry);
    let keys = facade.clone();
    let root = facade.root_ior();
    std::fs::write(ior_path, root.to_stringified()?)?;
    println!("listening on 127.0.0.1:{port}");
    println!("IOR written to {ior_path}");
    println!("READY");
    std::thread::spawn(move || {
        let _ = server.serve_shared(&facade, || false);
    });

    // ── lookup_id, both outcomes ──
    let mut repo = Connection::connect(&root, T)?;
    let reply = repo.invoke("lookup_id", |e| e.put_str(SUBJECT))?;
    let found = ifr::decode_object_reference(&mut reply.body()?)?;
    require(!found.is_nil(), "lookup_id returned a nil reference for a registered interface")?;
    require(found.type_id == INTERFACE_DEF_ID, "lookup_id did not advertise InterfaceDef")?;
    require(
        found.primary()?.object_key == keys.entry_key(SUBJECT),
        "lookup_id handed back a key the derivation does not produce",
    )?;
    ok(&format!("lookup_id {SUBJECT} returned an InterfaceDef reference"));

    let reply = repo.invoke("lookup_id", |e| e.put_str("IDL:gc10/Absent:1.0"))?;
    let missing = ifr::decode_object_reference(&mut reply.body()?)?;
    require(missing.is_nil(), "lookup_id of an unregistered id was not nil")?;
    ok("lookup_id of an unregistered id returned a nil reference");

    // ── mutating operations refused, on the Repository ──
    for op in ["create_interface", "create_module", "_set_id", "destroy"] {
        expect_system(repo.invoke(op, |e| e.put_str("x")), NO_PERMISSION, op)?;
    }
    ok("create_interface / create_module / _set_id / destroy all refused NO_PERMISSION");
    // Three refusals, three meanings — and the wire, not a document, is what
    // separates them. This used to check `contents` and `lookup_name` for
    // NO_IMPLEMENT; both are served since 2026-08-25, so what is measured here
    // is that they are *served* and that an operation nobody has decided about
    // is still BAD_OPERATION. A nullary body reaches a `contents` that wants
    // two arguments, so the honest answer to that request is MARSHAL.
    expect_system(repo.invoke_nullary("contents"), MARSHAL, "contents with no arguments")?;
    expect_system(repo.invoke_nullary("no_such_operation"), BAD_OPERATION, "no_such_operation")?;
    ok("an unknown operation is BAD_OPERATION and a malformed one is MARSHAL, not one answer");

    // ── the containment walk (§14.5.4), landed 2026-08-25 ──
    let top = repo.invoke("contents", |e| {
        e.put_u32(DefinitionKind::All as u32);
        e.put_bool(true);
    })?;
    let mut body = top.body()?;
    let count = body.get_u32()?;
    require(count > 0, "the repository's contents are empty")?;
    ok(&format!("Repository::contents(dk_all) returned {count} object(s)"));

    // Everything at the root of this repository is a module, so the filter is
    // measured by what it *excludes*: the same call limited to dk_Interface
    // must come back empty, and limited to dk_Module must come back whole.
    // A `contents` that ignored `limit_type` would pass a "> 0" check.
    let modules = count_contents(&mut repo, DefinitionKind::Module)?;
    let interfaces = count_contents(&mut repo, DefinitionKind::Interface)?;
    let structs = count_contents(&mut repo, DefinitionKind::Struct)?;
    require(
        modules == count && interfaces == 0 && structs == 0,
        "limit_type did not filter: dk_all/dk_Module/dk_Interface/dk_Struct          must be n/n/0/0 at a root holding only modules",
    )?;
    ok(&format!(
        "limit_type filters: dk_all {count}, dk_Module {modules}, dk_Interface {interfaces},          dk_Struct {structs}"
    ));

    let looked = repo.invoke("lookup", |e| {
        e.put_str("::gc10::Both");
    })?;
    let found_by_name = orbweaver_giop::Ior::read_from(&mut looked.body()?)?;
    require(!found_by_name.profiles.is_empty(), "lookup of an absolute name returned nil")?;
    ok("Container::lookup resolved an absolute scoped name");

    let missing = repo.invoke("lookup", |e| {
        e.put_str("::no::Such");
    })?;
    let nil = orbweaver_giop::Ior::read_from(&mut missing.body()?)?;
    require(nil.profiles.is_empty(), "lookup of an absent name did not return nil")?;
    ok("Container::lookup of an absent name is a nil reference, not an exception");

    let undefined = repo.invoke("lookup_name", |e| {
        e.put_str("Both");
        e.put_i32(0);
        e.put_u32(DefinitionKind::All as u32);
        e.put_bool(true);
    });
    expect_system(
        undefined,
        "IDL:omg.org/CORBA/BAD_PARAM:1.0",
        "lookup_name with levels_to_search = 0",
    )?;
    ok("levels_to_search 0 is undefined in the specification and is refused, not guessed");

    // One connection is served at a time: hang up before dialing the
    // InterfaceDef the lookup handed back.
    drop(repo);
    let mut def = Connection::connect(&found, T)?;

    // ── the Contained accessors ──
    let id = def.invoke_nullary("_get_id")?.body()?.get_string()?;
    let name = def.invoke_nullary("_get_name")?.body()?.get_string()?;
    let absolute = def.invoke_nullary("_get_absolute_name")?.body()?.get_string()?;
    let kind = def.invoke_nullary("_get_def_kind")?.body()?.get_u32()?;
    // `_get_version` is here because it was *absent* until 2026-08-14 while
    // `_set_version` was refused NO_PERMISSION — the read and the write the
    // wrong way round on data the id already carries.
    let version = def.invoke_nullary("_get_version")?.body()?.get_string()?;
    require(
        id == SUBJECT
            && name == "Both"
            && absolute == "::gc10::Both"
            && version == "1.0"
            && kind == DefinitionKind::Interface as u32,
        "the Contained accessors disagreed with the repository id",
    )?;
    expect_system(def.invoke("_set_version", |e| e.put_str("2.0")), NO_PERMISSION, "_set_version")?;
    ok(&format!(
        "_get_id/_get_name/_get_absolute_name/_get_version/_get_def_kind → \
         {id} {name} {absolute} {version} dk_Interface, and _set_version still refused"
    ));

    // ── describe_interface, decoded as a DII client would ──
    let reply = def.invoke_nullary("describe_interface")?;
    let d: FullInterfaceDescription = ifr::decode_full_interface_description(&mut reply.body()?)?;
    require(d.id == SUBJECT && d.name == "Both", "describe_interface named the wrong interface")?;
    require(d.defined_in == "IDL:gc10:1.0", "defined_in was not the containing module")?;
    require(d.version == "1.0", "version was not taken from the repository id")?;
    let ops: Vec<&str> = d.operations.iter().map(|o| o.name.as_str()).collect();
    require(ops == ["touch", "value"], &format!("operations were {ops:?}, expected touch, value"))?;
    let attrs: Vec<&str> = d.attributes.iter().map(|a| a.name.as_str()).collect();
    require(attrs == ["id", "name"], &format!("attributes were {attrs:?}, expected id, name"))?;
    require(
        d.base_interfaces == ["IDL:gc10/Derived:1.0", "IDL:gc10/Nameable:1.0"],
        "base_interfaces were not the two direct bases in declaration order",
    )?;
    ok(&format!(
        "describe_interface → {} operations, {} attributes, {} bases, tk_objref",
        d.operations.len(),
        d.attributes.len(),
        d.base_interfaces.len()
    ));

    // Each inherited member names its declarer, which is what makes including
    // inherited members lossless.
    let value = d.operations.iter().find(|o| o.name == "value").ok_or("value is missing")?;
    require(
        value.defined_in == "IDL:gc10/Derived:1.0",
        "the inherited operation did not name its declaring interface",
    )?;
    let id_attr = d.attributes.iter().find(|a| a.name == "id").ok_or("id is missing")?;
    require(
        id_attr.defined_in == "IDL:gc10/Base:1.0" && id_attr.mode == ifr::ATTR_READONLY,
        "the twice-inherited attribute lost its declarer or its readonly mode",
    )?;
    ok("inherited members carry defined_in (value←Derived, id←Base) and their modes");

    // ── is_a through inheritance, and the _is_a it must not be confused with ──
    for (asked, want) in [
        ("IDL:gc10/Base:1.0", true),
        ("IDL:gc10/Derived:1.0", true),
        ("IDL:gc10/Nameable:1.0", true),
        ("IDL:gc10/Base2:1.0", false),
    ] {
        let got = def.invoke("is_a", move |e| e.put_str(asked))?.body()?.get_bool()?;
        require(got == want, &format!("is_a({asked}) was {got}, expected {want}"))?;
    }
    ok("is_a walked two levels of inheritance and both bases, and refused a stranger");

    let self_is_a = def.invoke("_is_a", |e| e.put_str("IDL:gc10/Base:1.0"))?.body()?.get_bool()?;
    require(
        !self_is_a,
        "_is_a must answer for the InterfaceDef reference, not the described type",
    )?;
    let narrows = def.invoke("_is_a", |e| e.put_str(INTERFACE_DEF_ID))?.body()?.get_bool()?;
    require(narrows, "_is_a refused the InterfaceDef narrow a foreign ORB probes with")?;
    ok(
        "_is_a answers for the reference (InterfaceDef yes, gc10::Base no) — not the same question as is_a",
    );

    // ── base references are usable, not decorative ──
    let reply = def.invoke_nullary("_get_base_interfaces")?;
    let bases = ifr::read_interface_def_seq(&mut reply.body()?)?;
    require(bases.len() == 2, "_get_base_interfaces did not return two references")?;
    let derived = bases[0].clone();
    drop(def);
    let mut base_conn = Connection::connect(&derived, T)?;
    let base_id = base_conn.invoke_nullary("_get_id")?.body()?.get_string()?;
    require(
        base_id == "IDL:gc10/Derived:1.0",
        "the first base reference addressed the wrong entry",
    )?;
    ok("a reference from _get_base_interfaces is dialable and answers as its own entry");

    // ── the refusal holds on an InterfaceDef too, not only on the root ──
    expect_system(
        base_conn.invoke("_set_name", |e| e.put_str("Renamed")),
        NO_PERMISSION,
        "_set_name",
    )?;
    expect_system(base_conn.invoke_nullary("destroy"), NO_PERMISSION, "destroy")?;
    ok("_set_name and destroy on an InterfaceDef are refused NO_PERMISSION too");
    drop(base_conn);

    // ── the signature-carrying half: parameters, raises, oneway ──
    let mut tm = Connection::connect(&keys.entry_ior(SIGNATURES), T)?;
    let reply = tm.invoke_nullary("describe_interface")?;
    let d = ifr::decode_full_interface_description(&mut reply.body()?)?;

    let get = d.operations.iter().find(|o| o.name == "get").ok_or("get is missing")?;
    let params: Vec<(&str, u32)> =
        get.parameters.iter().map(|p| (p.name.as_str(), p.mode)).collect();
    require(params == [("id", ifr::PARAM_IN)], &format!("get's parameters were {params:?}"))?;
    let raised: Vec<&str> = get.exceptions.iter().map(|x| x.id.as_str()).collect();
    require(raised == ["IDL:tms/NoSuchTrack:1.0"], &format!("get raises {raised:?}"))?;
    require(
        matches!(&get.exceptions[0].tc, TypeCode::Except { members, .. } if members.len() == 1),
        "the raised exception's TypeCode came back empty, not from the registry",
    )?;
    ok(
        "describe_interface on tms::TrackManager carried in-parameters and a populated raises TypeCode",
    );

    let drop_op = d.operations.iter().find(|o| o.name == "drop").ok_or("drop is missing")?;
    require(drop_op.mode == ifr::OP_ONEWAY, "the oneway operation was reported OP_NORMAL")?;
    let snapshot = d.operations.iter().find(|o| o.name == "snapshot").ok_or("no snapshot")?;
    require(
        matches!(snapshot.result.resolve_alias(), TypeCode::Sequence { .. }),
        "snapshot's sequence<Track> return type did not survive the description",
    )?;
    ok("oneway is OP_ONEWAY and a sequence<struct> return type survives the round trip");
    drop(tm);

    if hold {
        println!(
            "HOLDING — {SUBJECT} is in the repository; point a cross-ORB client at {ior_path}"
        );
        loop {
            std::thread::park();
        }
    }
    Ok(())
}
