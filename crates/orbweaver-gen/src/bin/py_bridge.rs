//! `orbweaver-py-bridge` — where a generated Python client meets the wire.
//!
//! ```text
//! orbweaver-py-bridge --idl <file.idl> --ior <IOR:… | path/to/file.ior> [-I <dir>]...
//! ```
//!
//! The IDL is read as a translation unit, `#include`s resolved, `-I` as
//! `sidl-validate` spells it. This one is not cosmetic: the registry built here
//! is what turns a JSON document into CDR and a reply back into JSON. An
//! operation declared in an included base would be answered `no such operation`
//! by the bridge while the peer implements it, and a `raises` naming an
//! exception from a header would leave the bridge unable to name what it was
//! handed. *브리지의 레지스트리가 곧 호출 가능한 표면이다.*
//!
//! One JSON document per line in each direction, on stdin and stdout. The
//! documents are **AnyJSON v1** (`docs/PLAN.md` §4.5) — the mapping this
//! project already specifies and round-trip tests — so the seam introduced no
//! new wire format and no second dialect of one.
//!
//! ```text
//! → {"id":"IDL:spike/Echo:1.0","op":"add","args":{"a":40,"b":2}}
//! ← {"ok":{"returns":42,"outputs":{}}}
//! ← {"user_exception":{"id":"IDL:bank/Insufficient:1.0","members":{"shortfall":25}}}
//! ← {"system_exception":{"id":"IDL:omg.org/CORBA/NO_PERMISSION:1.0","minor":0,"completed":1}}
//! ← {"error":{"message":"…"}}
//! ```
//!
//! # Why this exists rather than an extension module
//!
//! Because CDR, GIOP and codeset negotiation exist once, in Rust, and a Python
//! target that re-implemented them would be a second ORB with a second set of
//! alignment bugs. Linking Rust into CPython instead would be a new dependency
//! class and a build-system commitment, which `docs/decisions/` says is not
//! adopted by writing code. `D007-python-wire-seam.md` states the options, and
//! is where that decision's status lives — it is not restated here.
//!
//! # What the bridge is not
//!
//! Not a security boundary. It dials the IOR it is given, on behalf of whoever
//! started it, with no exposure policy and no audit log — the guarded path is
//! `orbweaver-mcp`'s, and putting a second, weaker one here would be the §4.7
//! bypass wearing a different hat. It is a transport adapter for a client that
//! is already inside the trust boundary.
//!
//! References are the one exception, and they are not optional: §4.5 cannot
//! emit an IOR, so an object reference crosses as a **handle** into this
//! process's table and can be passed back as an argument. A handle is not
//! dialable and does not outlive the process.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::time::Duration;

use orbweaver_dynamic::anyjson::{self, LocalReferences};
use orbweaver_dynamic::invoke::{self, InvokeError};
use orbweaver_dynamic::json::Json;
use orbweaver_giop::typecode::TypeCode;
use orbweaver_giop::{Connection, Error as GiopError, Ior};
use orbweaver_idl::include::SearchPath;
use orbweaver_registry::{
    Contract, Entry, OperationSig, ParamDirection, ParamSig, Registry, Strictness,
    take_include_dirs,
};

fn main() -> std::process::ExitCode {
    let mut idl: Option<String> = None;
    let mut ior: Option<String> = None;
    let mut serve_id: Option<String> = None;
    let mut endpoint: Option<String> = None;
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
            "--idl" => idl = args.next(),
            "--ior" => ior = args.next(),
            "--serve" => serve_id = args.next(),
            "--endpoint" => endpoint = args.next(),
            other => {
                eprintln!("unexpected argument {other:?}");
                return std::process::ExitCode::from(2);
            }
        }
    }
    let Some(idl) = idl else {
        eprintln!("{USAGE}");
        return std::process::ExitCode::from(2);
    };
    // The two directions are two modes of one program and never both at once,
    // which is what keeps the pipes carrying a single conversation: in `--ior`
    // Python writes a request and the bridge answers; in `--serve` the bridge
    // writes a call and Python answers. A process that could do both would have
    // two writers on one stdout.
    let outcome = match (ior, serve_id) {
        (Some(_), Some(_)) => {
            eprintln!("--ior and --serve are the two directions and cannot be combined");
            return std::process::ExitCode::from(2);
        }
        (Some(ior), None) => run(&idl, &ior, &search),
        (None, Some(id)) => serve::run(&idl, &id, endpoint.as_deref(), &search),
        (None, None) => {
            eprintln!("{USAGE}");
            return std::process::ExitCode::from(2);
        }
    };
    match outcome {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("orbweaver-py-bridge: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage: orbweaver-py-bridge --idl <file.idl> [-I <dir>]...\n  \
     --ior <IOR:… | file>          call a target from Python (the client direction)\n  \
     --serve <interface-id>        serve a Python object (the servant direction)\n  \
     [--endpoint <host:port>]      where a served object listens; default 127.0.0.1:0";

fn run(idl_path: &str, ior_arg: &str, search: &SearchPath) -> Result<(), String> {
    let contract = Contract::load(std::path::Path::new(idl_path), search, Strictness::Checked)
        .map_err(|e| e.message)?;
    let mut registry = Registry::new();
    registry.load(&contract.spec).map_err(|e| e.to_string())?;
    // Not gated on `Registry::unresolved()`: it also records names whose only
    // problem is that the registry's resolver does not search an inherited
    // interface's scope, so refusing on it would refuse the naming contract
    // this project serves. `Contract::load` already refuses the case that
    // matters here — an `#include` that resolved to nothing.
    let registry = with_attribute_accessors(&registry);

    let text = match std::fs::read_to_string(ior_arg) {
        Ok(t) => t.trim().to_owned(),
        Err(_) => ior_arg.trim().to_owned(),
    };
    let ior = Ior::parse(&text).map_err(|e| e.to_string())?;
    let mut conn = Connection::connect(&ior, Duration::from_secs(10)).map_err(|e| e.to_string())?;

    let mut handles = LocalReferences::new();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    // The banner is a synchronisation point, not decoration: a client that
    // wrote a request before the connect finished would block on a reply that
    // could never come, and "the fixture had not started yet" is the phantom
    // failure this project has paid for most often.
    let _ = writeln!(
        out,
        "{}",
        Json::Object(BTreeMap::from([(
            "ready".to_owned(),
            Json::Object(BTreeMap::from([
                ("type_id".to_owned(), Json::String(ior.type_id.clone())),
                ("idl".to_owned(), Json::String(idl_path.to_owned())),
            ]))
        )]))
    );
    let _ = out.flush();

    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serve(&mut conn, &registry, &mut handles, &line) {
            Ok(reply) => reply,
            Err(message) => object([("error", object([("message", Json::String(message))]))]),
        };
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
    }
    Ok(())
}

fn object<const N: usize>(fields: [(&str, Json); N]) -> Json {
    Json::Object(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// The servant direction: our ORB accepts a call and Python answers it.
///
/// The mirror of everything above, and the same program: one JSON document per
/// line on the same two pipes, with the initiative reversed. What makes this a
/// *seam* rather than a second ORB is that nothing below knows about GIOP —
/// `orbweaver_gen::pyservant::PyServant` decodes the request, resolves the
/// operation, answers the object probes and chooses the reply status, and only
/// then is anything written to Python.
///
/// ```text
/// ← {"ready":{"ior":"IOR:…","type_id":"IDL:gc24/Gauge:1.0","idl":"…"}}
/// ← {"call":{"id":"IDL:gc24/Gauge:1.0","op":"record","args":{"sample":1.5,"unit":"C"}}}
/// → {"ok":{"returns":{"at":1.5,"sequence_no":1,"unit":"C"},"outputs":{}}}
/// ```
///
/// The three reply shapes are the client direction's, unchanged and
/// deliberately so: `ok`, `user_exception`, `system_exception` are the same
/// documents read from the other end, which is why a second direction did not
/// need a second format. *두 방향은 같은 문서를 반대편에서 읽는다.*
mod serve {
    use std::io::{BufRead, Write};
    use std::sync::atomic::{AtomicBool, Ordering};

    use orbweaver_dynamic::json::Json;
    use orbweaver_gen::pyservant::{Answerer, PyServant};
    use orbweaver_giop::orb::Orb;
    use orbweaver_idl::include::SearchPath;
    use orbweaver_registry::{Contract, Registry, Strictness};

    /// The object key a served Python object answers to.
    ///
    /// One servant per bridge process, so one key, and `Dispatch::knows`
    /// accepts everything — which is what its own default documentation calls
    /// right for a single-servant process.
    const KEY: &[u8] = b"pyservant";

    /// Asks the parent process, over the pipes it started us with.
    ///
    /// Owned handles rather than locks so the type is `Send`, which
    /// `Server::serve` requires of a `Dispatch`. Reading and writing are both
    /// line-framed and flushed, because a buffered call is a call the servant
    /// never sees and a deadlock nobody can read.
    struct Parent {
        stdin: std::io::Stdin,
        stdout: std::io::Stdout,
        /// Set when the parent closes its end. The accept loop reads it, so a
        /// servant whose Python side has gone away stops rather than answering
        /// every later caller with a seam failure.
        gone: &'static AtomicBool,
    }

    impl Answerer for Parent {
        fn ask(&mut self, call: &Json) -> Result<Json, String> {
            let document = super::object([("call", call.clone())]);
            {
                let mut out = self.stdout.lock();
                writeln!(out, "{document}").map_err(|e| e.to_string())?;
                out.flush().map_err(|e| e.to_string())?;
            }
            let mut line = String::new();
            loop {
                line.clear();
                let n = self.stdin.lock().read_line(&mut line).map_err(|e| e.to_string())?;
                if n == 0 {
                    self.gone.store(true, Ordering::SeqCst);
                    return Err("the servant closed its end".to_owned());
                }
                if line.trim().is_empty() {
                    continue;
                }
                return Json::parse(line.trim()).map_err(|e| e.to_string());
            }
        }
    }

    pub fn run(
        idl_path: &str,
        interface: &str,
        endpoint: Option<&str>,
        search: &SearchPath,
    ) -> Result<(), String> {
        let contract = Contract::load(std::path::Path::new(idl_path), search, Strictness::Checked)
            .map_err(|e| e.message)?;
        let mut registry = Registry::new();
        registry.load(&contract.spec).map_err(|e| e.to_string())?;
        // The plain registry, not `with_attribute_accessors`. The servant's
        // callable surface comes from `python::client_operations`, which
        // synthesises the accessors itself, and the copy the client direction
        // makes marks every entry as ingested from a remote Interface
        // Repository — a lie that direction can afford and this one has no
        // reason to tell.

        let addr = endpoint.unwrap_or("127.0.0.1:0");
        let server = Orb::new().server(addr, KEY.to_vec()).map_err(|e| e.to_string())?;
        let host = addr.split(':').next().filter(|h| !h.is_empty()).unwrap_or("127.0.0.1");
        let ior = server
            .ior(interface, host)
            .and_then(|i| i.to_stringified())
            .map_err(|e| e.to_string())?;

        static GONE: AtomicBool = AtomicBool::new(false);
        let parent = Parent { stdin: std::io::stdin(), stdout: std::io::stdout(), gone: &GONE };
        let mut servant = PyServant::new(&registry, interface, parent)?;

        // The banner, before a single call can arrive and after the listener
        // exists. A caller told the IOR too early dials a closed port, and
        // "the fixture had not started yet" is the phantom failure this project
        // has paid for most often — the same reason the client direction's
        // banner waits for its connect.
        {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            let _ = writeln!(
                out,
                "{}",
                super::object([(
                    "ready",
                    super::object([
                        ("ior", Json::String(ior.clone())),
                        ("type_id", Json::String(interface.to_owned())),
                        ("idl", Json::String(idl_path.to_owned())),
                    ])
                )])
            );
            let _ = out.flush();
        }

        server.serve(&mut servant, || GONE.load(Ordering::SeqCst)).map_err(|e| e.to_string())
    }
}

/// One request, from the line that carried it to the line that answers it.
fn serve(
    conn: &mut Connection,
    registry: &Registry,
    handles: &mut LocalReferences,
    line: &str,
) -> Result<Json, String> {
    let request = Json::parse(line).map_err(|e| e.to_string())?;
    let id = request.get("id").and_then(Json::as_str).ok_or("a request needs an \"id\"")?;
    let op = request.get("op").and_then(Json::as_str).ok_or("a request needs an \"op\"")?;
    let (_, sig) = registry
        .resolve_operation(id, op)
        .ok_or_else(|| format!("{id} has no operation {op:?}"))?;
    let sig = sig.clone();

    let Some(Json::Object(given)) = request.get("args") else {
        return Err("a request needs an \"args\" object".to_owned());
    };

    // Arguments are converted before anything is sent, so a bad one is a local
    // error rather than a half-written message — the same order the dynamic
    // invoker uses, for the same reason.
    let mut args: BTreeMap<String, orbweaver_dynamic::Value> = BTreeMap::new();
    for p in &sig.params {
        if !matches!(p.direction, ParamDirection::In | ParamDirection::InOut) {
            continue;
        }
        let Some(j) = given.get(&p.name) else {
            return Err(format!("{op} needs an argument {:?}", p.name));
        };
        let v = anyjson::from_json(&p.tc, j, handles)
            .map_err(|e| format!("argument {}: {e}", p.name))?;
        args.insert(p.name.clone(), v);
    }

    match invoke::invoke(conn, registry, id, op, &args) {
        Ok(outcome) => {
            let returns = if matches!(sig.returns, TypeCode::Void) {
                Json::Null
            } else {
                anyjson::to_json(&sig.returns, &outcome.returns, handles)
                    .map_err(|e| format!("the reply's return value: {e}"))?
            };
            let mut outputs = BTreeMap::new();
            for p in &sig.params {
                if !matches!(p.direction, ParamDirection::Out | ParamDirection::InOut) {
                    continue;
                }
                let Some(v) = outcome.outputs.get(&p.name) else { continue };
                outputs.insert(
                    p.name.clone(),
                    anyjson::to_json(&p.tc, v, handles)
                        .map_err(|e| format!("out parameter {}: {e}", p.name))?,
                );
            }
            Ok(object([("ok", object([("returns", returns), ("outputs", Json::Object(outputs))]))]))
        }
        Err(InvokeError::User(u)) => {
            let members = match (&u.members, registry.typecode(&u.id)) {
                (Some(v), Some(tc)) => anyjson::to_json(tc, v, handles)
                    .map_err(|e| format!("the raised {}: {e}", u.id))?,
                // An id the registry never heard of still names a contract the
                // caller was not built against, which is the useful half.
                _ => Json::Null,
            };
            Ok(object([(
                "user_exception",
                object([("id", Json::String(u.id.clone())), ("members", members)]),
            )]))
        }
        Err(InvokeError::Transport(GiopError::SystemException { id, minor, completed })) => {
            Ok(object([(
                "system_exception",
                object([
                    ("id", Json::String(id)),
                    ("minor", Json::Number(minor.to_string())),
                    // The ordinal, passed through unchanged. §4.11.4 numbers
                    // `completion_status` COMPLETED_YES, COMPLETED_NO,
                    // COMPLETED_MAYBE, and this project has already had those
                    // first two transposed once — see
                    // `orbweaver_giop::server::Completion`, where the comment
                    // is longer than the enum. Naming the value here would be
                    // a second place to get the same numbering wrong, so the
                    // bridge reports the number the peer sent.
                    ("completed", Json::Number(completed.to_string())),
                ]),
            )]))
        }
        Err(e) => Err(e.to_string()),
    }
}

/// A registry in which every attribute's `_get_`/`_set_` accessors are ordinary
/// operations.
///
/// # Why a copy rather than a special case at the call site
///
/// `_get_balance` is an operation on the wire (§7.9.1) and is not one in the
/// registry, which records attributes separately. The dynamic invoker resolves
/// operations through the registry, so without this the bridge would need its
/// own marshalling path for accessors — a second place that decides how a call
/// is written, which is exactly what this crate exists to avoid.
///
/// # Why it is not public
///
/// The copy is built with [`Registry::define_ingested`], which marks every
/// entry as having come from a remote Interface Repository. That is a lie the
/// bridge can afford — it consults nothing about provenance — and one that
/// `orbweaver-mcp` cannot, since its exposure gate asks exactly that question.
/// So this stays a private function of one binary rather than a convenience
/// somebody reaches for at the agent boundary.
fn with_attribute_accessors(registry: &Registry) -> Registry {
    const SOURCE: &str = "attribute accessors synthesised by orbweaver-py-bridge";
    let mut out = Registry::new();
    for id in registry.ids() {
        let Some(entry) = registry.get(id) else { continue };
        let entry = match entry {
            Entry::Interface(i) => {
                let mut i = i.clone();
                for (name, a) in i.attributes.clone() {
                    i.operations.insert(
                        format!("_get_{name}"),
                        OperationSig {
                            returns: a.tc.clone(),
                            params: Vec::new(),
                            raises: Vec::new(),
                            oneway: false,
                            annotations: a.annotations.clone(),
                        },
                    );
                    if !a.readonly {
                        i.operations.insert(
                            format!("_set_{name}"),
                            OperationSig {
                                returns: TypeCode::Void,
                                params: vec![ParamSig {
                                    name: "value".into(),
                                    direction: ParamDirection::In,
                                    tc: a.tc.clone(),
                                    annotations: BTreeMap::new(),
                                }],
                                raises: Vec::new(),
                                oneway: false,
                                annotations: a.annotations.clone(),
                            },
                        );
                    }
                }
                Entry::Interface(i)
            }
            other => other.clone(),
        };
        let _ = out.define_ingested(id.clone(), entry, SOURCE);
    }
    out
}
