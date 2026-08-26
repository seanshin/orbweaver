//! F5's declared-vs-served count, **computed from the contract** and measured
//! over a socket.
//!
//! `SERVICES-COVERAGE.md` produces this number for five services by running
//! `omniidl -b dump` as an external program and parsing its text. That is the
//! right oracle and it has one property this file exists to complement: it
//! needs an omniORB installation, so on a machine without one the number is
//! `BLOCKED` and F5's "16 of 16" is a sentence in a document rather than a
//! thing that goes red.
//!
//! This is the same measurement with our own front end reading the contract:
//! `corpus/golden/23-moe-enterprise.idl` is parsed, every interface's declared
//! operations are collected *including the ones it inherits*, and each is put
//! on the wire against the object that claims that interface. Nothing here is
//! typed in — an operation added to the contract and forgotten in the servant
//! fails this test on the next `cargo test`, which is the only moment anyone
//! would look.
//!
//! # The rule it enforces
//!
//! **A declared operation never answers `BAD_OPERATION`.** It may answer a
//! reply, a user exception, `MARSHAL` (the probe body is degenerate on
//! purpose), or `NO_IMPLEMENT` when a servant has decided not to implement it
//! — that is a decision the wire carries. `BAD_OPERATION` means *no such
//! operation on this interface*, and from an object that claims the interface
//! it is not a decision, it is an omission nobody wrote down.
//!
//! The converse is enforced too: an operation a neighbouring interface
//! declares **must** be `BAD_OPERATION` on an object that does not claim that
//! interface. Without it, "16 served" could be one object with a union of
//! sixteen operations rather than five distinct ones.
//!
//! # The probe body is sixty-four zero bytes
//!
//! An *empty* body makes `Request::body()` fail and every servant maps that to
//! `MARSHAL`, so every operation would look present. Sixty-four zeros decode
//! as empty strings, empty sequences and nil references, which every operation
//! of this contract refuses on its merits — `create` on an empty tenant id,
//! `bind_expert` on a nil reference — so nothing here mutates the service into
//! a state a later probe would misread. `SERVICES-COVERAGE.md` §2 argues the
//! same choice at greater length; this is a second implementation of it, not a
//! quotation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::BAD_OPERATION;
use orbweaver_giop::{Connection, Error, Ior, Reply};
use orbweaver_idl::ast::{Definition, InterfaceMember, Spec};
use orbweaver_object::get_reference;
use orbweaver_object::tenant_service::{Manifest, TenantService};

const T: Duration = Duration::from_secs(5);
const CONTRACT: &str = "corpus/golden/23-moe-enterprise.idl";

// ─────────────────────────────────────────────────────────────────────────────
// The contract, read rather than restated
// ─────────────────────────────────────────────────────────────────────────────

/// Every interface the contract declares: repository id → its own operations,
/// plus the repository ids of its bases.
struct Contract {
    own: BTreeMap<String, BTreeSet<String>>,
    bases: BTreeMap<String, Vec<String>>,
}

impl Contract {
    /// The operations an object claiming `id` must answer: its own and, by
    /// §11.3.7's inheritance, every base's, transitively.
    fn reachable(&self, id: &str) -> BTreeSet<String> {
        let mut out = self.own.get(id).cloned().unwrap_or_default();
        for base in self.bases.get(id).into_iter().flatten() {
            out.extend(self.reachable(base));
        }
        out
    }

    /// Every operation name anywhere in the contract — the pool the negative
    /// control draws from.
    fn all(&self) -> BTreeSet<String> {
        self.own.values().flatten().cloned().collect()
    }

    fn declared_total(&self) -> usize {
        self.own.values().map(BTreeSet::len).sum()
    }
}

fn repository_id(scope: &[String]) -> String {
    format!("IDL:{}:1.0", scope.join("/"))
}

/// Walks the spec, collecting interfaces with the module path they sit in.
///
/// Bases are resolved the way the language scopes them: an absolutely-scoped
/// name is taken as written, and a relative one is looked for from the
/// innermost enclosing scope outwards. `corpus/golden/23` writes
/// `::moe::Expert` absolutely, so the relative arm is unexercised by this
/// contract and is here so a future one does not silently resolve to nothing.
fn collect(spec: &Spec) -> Contract {
    let mut c = Contract { own: BTreeMap::new(), bases: BTreeMap::new() };
    let mut names: Vec<Vec<String>> = Vec::new();
    walk(&spec.definitions, &mut Vec::new(), &mut c, &mut names);
    // Second pass: bases, now that every interface's scope is known.
    let mut resolved = BTreeMap::new();
    walk_bases(&spec.definitions, &mut Vec::new(), &names, &mut resolved);
    c.bases = resolved;
    c
}

fn walk(
    defs: &[Definition],
    scope: &mut Vec<String>,
    c: &mut Contract,
    names: &mut Vec<Vec<String>>,
) {
    for d in defs {
        match d {
            Definition::Module(m) => {
                scope.push(m.name.text.clone());
                walk(&m.definitions, scope, c, names);
                scope.pop();
            }
            Definition::Interface(i) => {
                scope.push(i.name.text.clone());
                names.push(scope.clone());
                let ops = i
                    .body
                    .iter()
                    .flatten()
                    .filter_map(|m| match m {
                        InterfaceMember::Operation(o) => Some(o.name.text.clone()),
                        _ => None,
                    })
                    .collect();
                c.own.insert(repository_id(scope), ops);
                scope.pop();
            }
            _ => {}
        }
    }
}

fn walk_bases(
    defs: &[Definition],
    scope: &mut Vec<String>,
    names: &[Vec<String>],
    out: &mut BTreeMap<String, Vec<String>>,
) {
    for d in defs {
        match d {
            Definition::Module(m) => {
                scope.push(m.name.text.clone());
                walk_bases(&m.definitions, scope, names, out);
                scope.pop();
            }
            Definition::Interface(i) => {
                scope.push(i.name.text.clone());
                let mut bases = Vec::new();
                for b in &i.bases {
                    let target = if b.absolute {
                        Some(b.parts.clone())
                    } else {
                        // Innermost enclosing scope outwards, as the language
                        // resolves a relative name.
                        (0..scope.len())
                            .rev()
                            .map(|n| {
                                let mut candidate = scope[..n].to_vec();
                                candidate.extend(b.parts.iter().cloned());
                                candidate
                            })
                            .find(|candidate| names.contains(candidate))
                    };
                    let target = target
                        .unwrap_or_else(|| panic!("base {} of {} resolves", b.text(), i.name.text));
                    bases.push(repository_id(&target));
                }
                out.insert(repository_id(scope), bases);
                scope.pop();
            }
            _ => {}
        }
    }
}

fn contract() -> Contract {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(CONTRACT);
    let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let spec = orbweaver_idl::check(&src).unwrap_or_else(|d| {
        let why: Vec<String> = d.iter().map(ToString::to_string).collect();
        panic!("{CONTRACT} must parse: {}", why.join("; "));
    });
    collect(&spec)
}

// ─────────────────────────────────────────────────────────────────────────────
// The wire
// ─────────────────────────────────────────────────────────────────────────────

/// The answer to one probe, as the id of the system exception it raised, or
/// `<reply>` when the operation answered at all.
///
/// A user exception is an answer too — the operation resolved and ran — so it
/// is not distinguished here; the only distinction this file makes is
/// `BAD_OPERATION` against everything else.
fn answer(result: Result<Reply, Error>) -> String {
    match result {
        Err(Error::SystemException { id, .. }) => id,
        Err(other) => format!("<{other}>"),
        Ok(_) => "<reply>".to_owned(),
    }
}

/// One probe on one object, on its own connection.
///
/// A connection per probe rather than one for the whole sweep: `Server` serves
/// one connection at a time, and the objects live behind the same endpoint, so
/// holding two open would deadlock the window rather than measure it.
fn probe(target: &Ior, operation: &str) -> String {
    match Connection::connect(target, T) {
        Ok(mut c) => answer(c.invoke(operation, |e| e.put_bytes(&[0u8; 64]))),
        // Not a panic: this runs inside the serving scope, and a failure that
        // never sets the stop flag hangs the join rather than failing the run.
        // A connect that did not happen is an unmeasured check, so it is
        // reported as a failure with its own text and never as `BAD_OPERATION`.
        Err(e) => format!("{UNMEASURED} for {operation}: {e}"),
    }
}

/// The prefix a probe carries when it never reached the servant. Kept distinct
/// from every exception id so an unmeasured probe cannot be read as a served
/// operation on the way past.
const UNMEASURED: &str = "<no connection>";

fn manifest() -> Manifest {
    Manifest {
        tenant_id: "acme".to_owned(),
        base_model: "llama-70b".to_owned(),
        experts: Vec::new(),
        policy_domain: "acme-default".to_owned(),
        version: "1.0".to_owned(),
        residency_region: "eu-west".to_owned(),
    }
}

/// What the sweep calls an object: what it is, what interface it claims, and
/// where it is.
struct Addressed {
    what: &'static str,
    claims: &'static str,
    ior: Ior,
}

#[test]
fn every_declared_operation_of_the_f5_contract_answers() {
    let c = contract();
    assert_eq!(
        c.declared_total(),
        16,
        "corpus/golden/23 declares sixteen operations; if this changed, the servant and \
         SERVICES-COVERAGE.md both have to move with it"
    );

    let server = Orb::new().server("127.0.0.1:0", b"MoE".to_vec()).expect("bound");
    let port = server.local_addr().expect("an address").port();
    let svc = TenantService::new("127.0.0.1", port, "MoE");

    // Out of band, exactly as `spike-tenants` does it: the contract declares
    // no operation that mints a factory, an adapter or a node → region row.
    let factory = svc.provision_factory("acme").expect("acme is a usable tenant id");
    svc.provision_expert("acme", "math", "llama-70b", 1.5, b"acme-delta").expect("acme/math");
    svc.declare_node("gpu-eu-1", "eu-west");
    let expert = svc.expert_reference("acme", "math").expect("the expert reference");
    let base = svc.shared_base_reference("llama-70b").expect("the shared base");

    let stop = AtomicBool::new(false);
    let mut wrong: Vec<String> = Vec::new();
    let mut served = 0usize;

    // Nothing inside the serving scope may panic: the serve loop only stops
    // when `stop` goes up at the end of the window, so a panic before that
    // would hang the scope's join instead of failing the test. Every step in
    // there records into `wrong` and returns; the assertions are outside.
    std::thread::scope(|scope| {
        let serving = scope.spawn(|| {
            let _ = server.serve_shared(&svc, || stop.load(Ordering::SeqCst));
        });

        // The one object no reference is handed out for by any other means:
        // `create` is the only way to hold a `ComposedModel`, which is the
        // CosLifeCycle factory shape working as intended.
        let model = Connection::connect(&factory, T)
            .and_then(|mut conn| conn.invoke("create", |e| manifest().write_to(e)))
            .and_then(|reply| get_reference(&mut reply.body()?))
            .map_err(|e| format!("create on the factory: {e}"))
            .and_then(|r| r.ok_or_else(|| "create returned a nil reference".to_owned()));
        let policy = svc
            .policy_reference("acme", "acme-default")
            .ok_or_else(|| "create should have minted the acme-default domain".to_owned());

        let objects: Vec<Addressed> = match (model, policy) {
            (Ok(model), Ok(policy)) => vec![
                Addressed {
                    what: "factory",
                    claims: "IDL:moe/enterprise/ModelFactory:1.0",
                    ior: factory.clone(),
                },
                Addressed {
                    what: "model",
                    claims: "IDL:moe/enterprise/ComposedModel:1.0",
                    ior: model,
                },
                Addressed {
                    what: "policy",
                    claims: "IDL:moe/enterprise/PolicyDomain:1.0",
                    ior: policy,
                },
                Addressed {
                    what: "expert",
                    claims: "IDL:moe/enterprise/EnterpriseExpert:1.0",
                    ior: expert,
                },
                Addressed { what: "shared base", claims: "IDL:moe/Expert:1.0", ior: base },
            ],
            (model, policy) => {
                wrong.extend(model.err().into_iter().chain(policy.err()));
                Vec::new()
            }
        };

        let every = c.all();
        for o in &objects {
            let reachable = c.reachable(o.claims);
            if reachable.is_empty() {
                wrong.push(format!(
                    "{} claims {} and the contract declares no such interface",
                    o.what, o.claims
                ));
                continue;
            }
            for op in &reachable {
                let got = probe(&o.ior, op);
                if got == BAD_OPERATION {
                    wrong.push(format!(
                        "{}: {op} is declared by {} and answered BAD_OPERATION — a servant that \
                         means to refuse it says NO_IMPLEMENT",
                        o.what, o.claims
                    ));
                } else if got.starts_with(UNMEASURED) {
                    wrong.push(format!("{}: {got}", o.what));
                } else {
                    served += 1;
                }
            }
            // The negative control: a neighbour's operation must not resolve
            // here, or the five objects are one object.
            for op in every.difference(&reachable) {
                let got = probe(&o.ior, op);
                if got != BAD_OPERATION {
                    wrong.push(format!(
                        "{}: {op} is not declared by {} and answered {got} — this object serves \
                         a union of interfaces rather than its own",
                        o.what, o.claims
                    ));
                }
            }
        }

        // The flag goes up after the last probe's connection has closed, so
        // the serve loop is blocked in accept by now; one throwaway connection
        // is what wakes it to notice.
        stop.store(true, Ordering::SeqCst);
        drop(Connection::connect(&factory, T));
        let _ = serving.join();
    });

    assert!(wrong.is_empty(), "F5 declared-vs-served:\n  {}", wrong.join("\n  "));
    // Sixteen probes answered, one per declared operation, on the object that
    // claims the interface declaring it — `EnterpriseExpert` inherits two, and
    // they are counted once each against `::moe::Expert`'s own object and once
    // more against the expert, which is why this is not sixteen.
    assert_eq!(served, 18, "sixteen declared operations, two of them inherited and served twice");
}
