//! One home for the plane rule — the consumer census D006 asked for,
//! **computed from the contracts** and measured over a socket.
//!
//! `docs/decisions/D006-plane-rule-tensor.md` is APPROVED with option E:
//! `Expert::process` and `Router::dispatch` excluded, because exclusion is
//! *"the only option that removes the opportunity instead of labelling it."*
//! The recommendation rests on one falsifiable claim D006 calls **the consumer
//! census** — *"if the count is zero, E is right; if it is nonzero, E is wrong
//! today and the recommendation inverts"* — recorded as zero on 2026-08-14
//! and never re-run.
//!
//! It is nonzero. [`orbweaver_object::plane`] is the finding and its reasoning;
//! this file is the part that cannot go stale.
//!
//! # What is computed and what is written by hand
//!
//! **Computed:** the membership of the census. Both MoE contracts are parsed
//! with our own front end, every typedef and struct is resolved, and every
//! operation whose return or parameters reach `moe::Tensor` through any depth
//! of typedef, struct or sequence is collected with the direction it crosses
//! in. Nothing about *which* operations carry a `Tensor` is typed in, so a
//! contract edit that puts a `Tensor` behind one more struct member joins the
//! census on the next `cargo test` rather than on the next reading.
//!
//! **Written by hand:** only the status and the reason, and the status is then
//! checked against what the servants actually answer over a socket. A table
//! saying `Served` for an operation that refuses, or `Refused` for one that
//! answers, fails here.
//!
//! # Why the direction is part of the key
//!
//! D006 corrected `PLAN-MOE` §4.6 on exactly this: §4.6 filed `Router::select`
//! as pure control plane because its *return* is references-only, and D006
//! found a `Tensor` on its input side in `GateSignal::affinity`. A census
//! keyed on the operation alone would have recorded the same thing §4.6 did
//! and been wrong in the same way.
//!
//! # The negative controls
//!
//! Three, and each moves a counter rather than printing a sentence:
//!
//! 1. Delete any row from `plane::TENSOR_BEARING` → the computed set has a
//!    member the table does not, and [`the_census_is_computed_from_the_contracts`]
//!    names it.
//! 2. Flip `dispatch` to `Served` in the table → [`the_recorded_status_is_what_the_wire_answers`]
//!    reads `NO_IMPLEMENT` off the socket and disagrees with the table.
//! 3. Make `Router::dispatch` answer `BAD_OPERATION` instead of `NO_IMPLEMENT`
//!    → the same test fails on the polarity, which is the failure this project
//!    has already had twice in prose and never in a test.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{BAD_OPERATION, NO_IMPLEMENT};
use orbweaver_giop::{Connection, Error, Ior, Reply};
use orbweaver_idl::ast::{Definition, InterfaceMember, Spec, TypeSpec};
use orbweaver_object::expert_service::ExpertService;
use orbweaver_object::plane::{PlaneStatus, TENSOR_BEARING};
use orbweaver_object::tenant_service::TenantService;
use orbweaver_trading::policy::LoadingPolicy;

const T: Duration = Duration::from_secs(5);

/// Both contracts that declare a `moe::Tensor`. Listed rather than globbed:
/// a corpus file that starts declaring one has to be looked at, not swept up.
const CONTRACTS: [&str; 2] =
    ["corpus/golden/22-moe-control-plane.idl", "corpus/golden/23-moe-enterprise.idl"];

/// The typedef the whole rule is about, as the contracts spell it.
const TENSOR: &str = "moe::Tensor";

// ─────────────────────────────────────────────────────────────────────────────
// Reading the contract: which types reach a `Tensor`
// ─────────────────────────────────────────────────────────────────────────────

/// Every named type declaration in one contract, by its fully-scoped name.
#[derive(Default)]
struct Types {
    /// `typedef T X;` — the aliased type.
    aliases: BTreeMap<String, TypeSpec>,
    /// `struct X { ... };` — every member's type.
    structs: BTreeMap<String, Vec<TypeSpec>>,
    /// Every declared name, for resolving a relative reference.
    declared: BTreeSet<String>,
}

fn scoped(parts: &[String]) -> String {
    parts.join("::")
}

fn collect_types(defs: &[Definition], scope: &mut Vec<String>, t: &mut Types) {
    for d in defs {
        match d {
            Definition::Module(m) => {
                scope.push(m.name.text.clone());
                collect_types(&m.definitions, scope, t);
                scope.pop();
            }
            Definition::Typedef(td) => {
                scope.push(td.name.text.clone());
                let name = scoped(scope);
                t.declared.insert(name.clone());
                t.aliases.insert(name, td.ty.clone());
                scope.pop();
            }
            Definition::Struct(s) => {
                scope.push(s.name.text.clone());
                let name = scoped(scope);
                t.declared.insert(name.clone());
                let members = s
                    .members
                    .iter()
                    .flatten()
                    .flat_map(|m| m.names.iter().map(|_| m.ty.clone()))
                    .collect();
                t.structs.insert(name, members);
                scope.pop();
            }
            Definition::Enum(e) => {
                scope.push(e.name.text.clone());
                t.declared.insert(scoped(scope));
                scope.pop();
            }
            Definition::Interface(i) => {
                // An interface is a name a type reference can resolve to, and
                // it never reaches a `Tensor` itself: a reference crosses as
                // an IOR. Its *body* can declare types, though, and IDL allows
                // it, so the nested definitions are walked in the interface's
                // own scope. Neither MoE contract uses this today; it is here
                // so a later one does not quietly fall outside the census.
                scope.push(i.name.text.clone());
                t.declared.insert(scoped(scope));
                let nested: Vec<Definition> = i
                    .body
                    .iter()
                    .flatten()
                    .filter_map(|m| match m {
                        InterfaceMember::Nested(d) => Some(d.clone()),
                        _ => None,
                    })
                    .collect();
                collect_types(&nested, scope, t);
                scope.pop();
            }
            _ => {}
        }
    }
}

/// Resolves a type reference the way the language scopes it: an absolute name
/// as written, a relative one from the innermost enclosing scope outwards.
fn resolve(t: &Types, name: &orbweaver_idl::ast::ScopedName, scope: &[String]) -> Option<String> {
    if name.absolute {
        let full = scoped(&name.parts);
        return t.declared.contains(&full).then_some(full);
    }
    (0..=scope.len()).rev().find_map(|n| {
        let mut candidate = scope[..n].to_vec();
        candidate.extend(name.parts.iter().cloned());
        let full = scoped(&candidate);
        t.declared.contains(&full).then_some(full)
    })
}

/// The fixpoint: every named type that transitively reaches `moe::Tensor`.
///
/// A fixpoint rather than one pass because the contracts nest — `Activation`
/// reaches a `Tensor` through a member, and anything holding an `Activation`
/// reaches one through `Activation`. Iterating to a fixed point costs nothing
/// on a contract this size and means depth is never a thing to get right.
fn reaching(t: &Types) -> BTreeSet<String> {
    let mut reach: BTreeSet<String> = BTreeSet::new();
    if t.declared.contains(TENSOR) {
        reach.insert(TENSOR.to_owned());
    }
    loop {
        let before = reach.len();
        for (name, ty) in &t.aliases {
            if reaches(t, ty, &scope_of(name), &reach) {
                reach.insert(name.clone());
            }
        }
        for (name, members) in &t.structs {
            if members.iter().any(|m| reaches(t, m, &scope_of(name), &reach)) {
                reach.insert(name.clone());
            }
        }
        if reach.len() == before {
            return reach;
        }
    }
}

/// The enclosing scope of a fully-scoped name — `moe::Activation` sits in
/// `["moe"]`, which is where a relative member type resolves from.
fn scope_of(full: &str) -> Vec<String> {
    let mut parts: Vec<String> = full.split("::").map(str::to_owned).collect();
    parts.pop();
    parts
}

/// Does this type expression reach a `Tensor`?
fn reaches(t: &Types, ty: &TypeSpec, scope: &[String], reach: &BTreeSet<String>) -> bool {
    match ty {
        TypeSpec::Named(n) => match resolve(t, n, scope) {
            Some(full) => full == TENSOR || reach.contains(&full),
            None => false,
        },
        TypeSpec::Sequence { element, .. } => reaches(t, element, scope, reach),
        _ => false,
    }
}

/// One position in the census: an operation, the interface that declares it,
/// and the direction its `Tensor` crosses in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Crossing {
    interface: String,
    operation: String,
    direction: String,
}

fn census_of(spec: &Spec) -> BTreeSet<Crossing> {
    let mut t = Types::default();
    collect_types(&spec.definitions, &mut Vec::new(), &mut t);
    let reach = reaching(&t);
    let mut out = BTreeSet::new();
    walk_interfaces(&spec.definitions, &mut Vec::new(), &t, &reach, &mut out);
    out
}

fn walk_interfaces(
    defs: &[Definition],
    scope: &mut Vec<String>,
    t: &Types,
    reach: &BTreeSet<String>,
    out: &mut BTreeSet<Crossing>,
) {
    for d in defs {
        match d {
            Definition::Module(m) => {
                scope.push(m.name.text.clone());
                walk_interfaces(&m.definitions, scope, t, reach, out);
                scope.pop();
            }
            Definition::Interface(i) => {
                scope.push(i.name.text.clone());
                let rid = format!("IDL:{}:1.0", scope.join("/"));
                for member in i.body.iter().flatten() {
                    let InterfaceMember::Operation(op) = member else { continue };
                    let returns = reaches(t, &op.returns, scope, reach);
                    let takes = op.params.iter().any(|p| reaches(t, &p.ty, scope, reach));
                    let direction = match (takes, returns) {
                        (true, true) => "in and out",
                        (true, false) => "in",
                        (false, true) => "out",
                        (false, false) => continue,
                    };
                    out.insert(Crossing {
                        interface: rid.clone(),
                        operation: op.name.text.clone(),
                        direction: direction.to_owned(),
                    });
                }
                scope.pop();
            }
            _ => {}
        }
    }
}

fn parse(path: &str) -> Spec {
    let full = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(path);
    let src = std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("{}: {e}", full.display()));
    orbweaver_idl::check(&src).unwrap_or_else(|d| {
        let why: Vec<String> = d.iter().map(ToString::to_string).collect();
        panic!("{path} must parse: {}", why.join("; "));
    })
}

/// The census over every contract that declares a `Tensor`, merged. An
/// operation declared identically in both files — `moe::Expert::process` is —
/// is one position, because it is one operation on the wire.
fn computed_census() -> BTreeSet<Crossing> {
    CONTRACTS.iter().flat_map(|c| census_of(&parse(c))).collect()
}

/// The hand-written table, in the computed table's shape.
fn recorded_census() -> BTreeSet<Crossing> {
    TENSOR_BEARING
        .iter()
        .map(|t| Crossing {
            interface: t.interface.to_owned(),
            operation: t.operation.to_owned(),
            direction: t.direction.to_string(),
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// The census
// ─────────────────────────────────────────────────────────────────────────────

/// The membership of `plane::TENSOR_BEARING` is what the contracts say, not
/// what somebody remembered.
#[test]
fn the_census_is_computed_from_the_contracts() {
    let computed = computed_census();
    let recorded = recorded_census();

    let unrecorded: Vec<&Crossing> = computed.difference(&recorded).collect();
    assert!(
        unrecorded.is_empty(),
        "these operations carry a `moe::Tensor` and `plane::TENSOR_BEARING` does not record \
         them — every crossing needs a status and a reason (D006):\n  {}",
        unrecorded
            .iter()
            .map(|c| format!("{}::{} ({})", c.interface, c.operation, c.direction))
            .collect::<Vec<_>>()
            .join("\n  ")
    );

    let phantom: Vec<&Crossing> = recorded.difference(&computed).collect();
    assert!(
        phantom.is_empty(),
        "`plane::TENSOR_BEARING` records crossings the contracts no longer declare; if an \
         operation stopped carrying a `Tensor` the row comes out and D006 is amended:\n  {}",
        phantom
            .iter()
            .map(|c| format!("{}::{} ({})", c.interface, c.operation, c.direction))
            .collect::<Vec<_>>()
            .join("\n  ")
    );

    // The direction is part of the key above, so this is not a second
    // assertion — it is the one distinction D006 had to correct §4.6 on,
    // named so a reader of a failure knows why it is in the key at all.
    assert_eq!(
        computed.iter().find(|c| c.operation == "select").map(|c| c.direction.as_str()),
        Some("in"),
        "`Router::select` carries a `Tensor` on its input side only, through \
         `GateSignal::affinity` — this is the finding that corrected PLAN-MOE §4.6"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// The wire
// ─────────────────────────────────────────────────────────────────────────────

/// What one probe answered: the system exception id, or `<reply>`.
fn answer(result: Result<Reply, Error>) -> String {
    match result {
        Err(Error::SystemException { id, .. }) => id,
        Err(other) => format!("<{other}>"),
        Ok(_) => "<reply>".to_owned(),
    }
}

/// Sixty-four zero bytes, for the reason `f5_declared_vs_served.rs` argues at
/// length: an empty body makes every servant answer `MARSHAL`, so every
/// operation would look present. Zeros decode as empty sequences and strings.
fn probe(target: &Ior, operation: &str) -> String {
    match Connection::connect(target, T) {
        Ok(mut c) => answer(c.invoke(operation, |e| e.put_bytes(&[0u8; 64]))),
        Err(e) => format!("{UNMEASURED} for {operation}: {e}"),
    }
}

/// A probe that never reached the servant is an unmeasured check, which is a
/// failure and never a pass.
const UNMEASURED: &str = "<no connection>";

/// Whether an answer means the operation was dispatched.
///
/// `NO_IMPLEMENT` is a decision and `BAD_OPERATION` is an omission; everything
/// else — a reply, a `MARSHAL` on the degenerate body, a refusal on the
/// arguments' merits — means the servant took the call.
fn was_served(got: &str) -> bool {
    got != NO_IMPLEMENT && got != BAD_OPERATION && !got.starts_with(UNMEASURED)
}

/// The status column is a measurement, not a claim: each row is put on the
/// wire against the object that claims its interface.
///
/// `EnterpriseExpert::adapter_delta` and `ComposedModel::infer` are probed on
/// `TenantService`; `Router::select` and `Router::dispatch` on
/// `ExpertService`; `Expert::process` on the shared base, which is the object
/// that claims `IDL:moe/Expert:1.0` and nothing more.
#[test]
fn the_recorded_status_is_what_the_wire_answers() {
    let tenants = Orb::new().server("127.0.0.1:0", b"MoE".to_vec()).expect("bound");
    let tenant_port = tenants.local_addr().expect("an address").port();
    let tsvc = TenantService::new("127.0.0.1", tenant_port, "MoE");
    tsvc.provision_expert("acme", "math", "llama-70b", 1.5, b"acme-delta").expect("acme/math");
    let expert = tsvc.expert_reference("acme", "math").expect("the expert reference");
    let base = tsvc.shared_base_reference("llama-70b").expect("the shared base");

    let experts = Orb::new().server("127.0.0.1:0", b"Experts".to_vec()).expect("bound");
    let expert_port = experts.local_addr().expect("an address").port();
    let esvc = ExpertService::new(
        "127.0.0.1",
        expert_port,
        b"Experts",
        LoadingPolicy { affinity_weight: 1, low_watermark: 100, high_watermark: 400 },
        0,
    );
    let router = esvc.router_ior();

    let stop = AtomicBool::new(false);
    let mut wrong: Vec<String> = Vec::new();

    // Nothing inside the serving scope panics: the loops only stop when `stop`
    // goes up, so a panic before that hangs the join instead of failing.
    std::thread::scope(|scope| {
        let serving_tenants = scope.spawn(|| {
            let _ = tenants.serve_shared(&tsvc, || stop.load(Ordering::SeqCst));
        });
        let serving_experts = scope.spawn(|| {
            let _ = experts.serve_shared(&esvc, || stop.load(Ordering::SeqCst));
        });

        for row in TENSOR_BEARING {
            let target = match row.interface {
                "IDL:moe/Router:1.0" => &router,
                "IDL:moe/Expert:1.0" => &base,
                "IDL:moe/enterprise/EnterpriseExpert:1.0" => &expert,
                // `ComposedModel` is minted by `ModelFactory::create` and this
                // test does not stand a factory up; it is left to
                // `f5_declared_vs_served.rs`, which probes every declared
                // operation of that contract including `infer`. Recorded here
                // rather than silently skipped.
                "IDL:moe/enterprise/ComposedModel:1.0" => continue,
                other => {
                    wrong.push(format!("no object in this test claims {other}"));
                    continue;
                }
            };
            let got = probe(target, row.operation);
            if got.starts_with(UNMEASURED) {
                wrong.push(got);
                continue;
            }
            let observed =
                if was_served(&got) { PlaneStatus::Served } else { PlaneStatus::Refused };
            if observed != row.status {
                wrong.push(format!(
                    "{}::{} is recorded {} and the wire answered {got} ({observed}) — \
                     `plane::TENSOR_BEARING` is the home of this fact and it disagrees with \
                     the servant",
                    row.interface, row.operation, row.status
                ));
            }
            // The polarity, separately: a refusal that means *decided* says
            // `NO_IMPLEMENT`, and `BAD_OPERATION` is what an oversight says.
            // This project has written the wrong one down twice in prose.
            if row.status == PlaneStatus::Refused && got != NO_IMPLEMENT {
                wrong.push(format!(
                    "{}::{} refused with {got}; a declared operation a servant decided not to \
                     implement answers NO_IMPLEMENT, never BAD_OPERATION",
                    row.interface, row.operation
                ));
            }
        }

        stop.store(true, Ordering::SeqCst);
        drop(Connection::connect(&router, T));
        drop(Connection::connect(&base, T));
        let _ = serving_experts.join();
        let _ = serving_tenants.join();
    });

    assert!(wrong.is_empty(), "the plane rule's table against the wire:\n  {}", wrong.join("\n  "));
}
