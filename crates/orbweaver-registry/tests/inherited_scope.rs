//! Name resolution across interface inheritance, and what its absence cost.
//!
//! CORBA 2.3 §3.15.2 (§7.19.2 in CORBA 3.4), *Scoping Rules and Name
//! Resolution*: "A name can be used in an unqualified form within a particular
//! scope; it will be resolved by successively searching farther out in
//! enclosing scopes, **while taking into consideration inheritance
//! relationships among interfaces**." The spec's worked example fixes the
//! order — the interface's own scope, then its bases' scopes, then the
//! enclosing module, then global — and the order is load-bearing: a base's
//! declaration beats an enclosing module's declaration of the same name.
//!
//! The registry walked lexical scopes only. The one contract in the corpus that
//! inherits a name rather than declaring it —
//! `corpus/services/gen-naming-subset.idl`, where `NamingContextExt :
//! NamingContext` raises the `NotFound` its base declares, exactly as OMG
//! writes it — recorded five [`Unresolved`] markers, and the §5.3 release gate
//! exited 2 over a contract omniidl and JacORB both accept. A gate that cries
//! wolf gets bypassed, which makes that worse than the defect the exit-2
//! behaviour was added for, not smaller.
//!
//! *상속 범위는 바깥 범위보다 먼저 탐색된다. 게이트가 늑대를 외치면 우회된다.*
//!
//! [`Unresolved`]: orbweaver_registry::Unresolved

use std::path::{Path, PathBuf};

use orbweaver_giop::typecode::TypeCode;
use orbweaver_idl::SearchPath;
use orbweaver_registry::{Registry, Strictness, UnresolvedKind, registry_from_files};

fn corpus(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join("corpus").join(rel)
}

fn loaded(rel: &str) -> Registry {
    registry_from_files(&[corpus(rel)], &SearchPath::new(), Strictness::Grammar)
        .unwrap_or_else(|e| panic!("{rel} must load: {e}"))
}

fn from_str(src: &str) -> Registry {
    let spec = orbweaver_idl::parse(src).expect("parses");
    let mut reg = Registry::new();
    reg.load(&spec).expect("loads");
    reg
}

/// The reproduction, as a test: the contract the gate refused.
#[test]
fn the_contract_both_oracles_accept_has_nothing_unresolved() {
    let reg = loaded("services/gen-naming-subset.idl");
    assert!(
        reg.unresolved().is_empty(),
        "omniidl and JacORB both accept this file; the gate refused it over: {:?}",
        reg.unresolved()
    );

    // Not merely "no longer an error": the inherited exception has to reach the
    // signature, because a `NotFound` body carries its repository id as the
    // first field and a caller that never learned the id recognises nothing.
    let ext = reg.id_of("CosNaming::NamingContextExt").expect("registered").clone();
    let (_, sig) = reg.resolve_operation(&ext, "resolve_str").expect("declared");
    assert!(
        sig.raises.contains(&"IDL:omg.org/CosNaming/NamingContext/NotFound:1.0".to_string()),
        "the id follows the *declaring* scope, not the using one: {:?}",
        sig.raises
    );
    assert!(
        sig.raises.contains(&"IDL:omg.org/CosNaming/NamingContext/InvalidName:1.0".to_string()),
        "{:?}",
        sig.raises
    );
}

/// The spec's own example, in the corpus: a base's declaration beats the
/// enclosing module's. `gc_inh::Ticket` is a `long`; `Root::Ticket` is a
/// `string`; inside `Middle : Root`, `Ticket` is the string. omniidl and JacORB
/// were both asked, and both print the base's.
#[test]
fn a_base_scope_is_searched_before_the_enclosing_one() {
    let reg = loaded("golden/inherited-scope.idl");
    assert!(reg.unresolved().is_empty(), "{:?}", reg.unresolved());

    let middle = reg.id_of("gc_inh::Middle").expect("registered").clone();
    let (_, sig) = reg.resolve_operation(&middle, "reissue").expect("declared");
    let TypeCode::Alias { id, .. } = &sig.returns else {
        panic!("expected the typedef, got {:?}", sig.returns);
    };
    assert_eq!(id, "IDL:gc_inh/Root/Ticket:1.0", "the base's Ticket, not the module's");
    assert_eq!(
        sig.returns.resolve_alias(),
        &TypeCode::String(0),
        "gc_inh::Ticket is a long; picking it here would marshal four bytes for a string"
    );
}

/// Inheritance is a graph: a base's base counts, and a diamond contributes one
/// name rather than two — §3.15.2, "[t]wo shadow copies of the same original
/// ... introduce a single name into the derived interface and don't conflict
/// with each other."
#[test]
fn a_base_of_a_base_counts_and_a_diamond_resolves_once() {
    let reg = loaded("golden/inherited-scope.idl");
    let denied = reg.id_of("gc_inh::Root::Denied").expect("registered").clone();

    for (iface, op) in [("gc_inh::Leaf", "confirm"), ("gc_inh::Joined", "settle")] {
        let id = reg.id_of(iface).unwrap_or_else(|| panic!("{iface} registered")).clone();
        let (_, sig) =
            reg.resolve_operation(&id, op).unwrap_or_else(|| panic!("{iface}::{op} declared"));
        assert_eq!(sig.raises, vec![denied.clone()], "{iface}::{op} raises exactly one Denied");
    }
}

/// The negative control, and the reason it is a file rather than a sentence: a
/// resolver that "fixed" inheritance by searching every interface in the unit
/// passes every positive case above. A sibling is not a base.
#[test]
fn an_inherited_scope_does_not_leak_to_a_sibling() {
    let reg = loaded("negative/inherited-scope-leak.idl");
    let noted = reg.unresolved();
    assert_eq!(noted.len(), 2, "both references must stay unresolved, got {noted:?}");
    assert!(
        noted.iter().any(|u| u.kind == UnresolvedKind::Type && u.name == "Ticket"),
        "{noted:?}"
    );
    assert!(
        noted.iter().any(|u| u.kind == UnresolvedKind::Raises && u.name == "Denied"),
        "{noted:?}"
    );
}

/// A name no base declares is still unresolved, so the marker still marks.
#[test]
fn a_name_no_base_declares_is_still_recorded() {
    let reg = from_str(
        "module m { interface Base { exception Known {}; }; \
         interface Derived : Base { void f() raises (Known, Absent); }; };",
    );
    let noted = reg.unresolved();
    assert_eq!(noted.len(), 1, "only the undeclared one, got {noted:?}");
    assert_eq!(noted[0].name, "Absent");
    assert_eq!(noted[0].at, "m::Derived::f");
}

/// A cycle in the inheritance graph must terminate rather than recurse. Illegal
/// IDL — the front end says so — but `Registry::load` is documented as
/// accumulating whatever it is handed, so it has to survive being handed this.
#[test]
fn a_cycle_in_the_inheritance_graph_terminates() {
    let reg = from_str(
        "module m { interface A : B { void f() raises (Nowhere); }; \
         interface B : A { void g(); }; };",
    );
    let noted = reg.unresolved();
    assert!(
        noted.iter().any(|u| u.name == "Nowhere"),
        "the search has to end, and end saying no: {noted:?}"
    );
}

/// What the marker means now — one fact, and every reference that has it.
///
/// It used to record bases and `raises` only, so `struct S { Widget w; };` —
/// `corpus/negative/n04-unknown-type.idl` — loaded silently with `w` typed
/// `void` and `idl-diff` exited 0 on it. Measured, not assumed: that was the
/// state before this change.
#[test]
fn an_unresolvable_type_name_is_recorded_like_a_base_or_a_raises() {
    let reg = loaded("negative/n04-unknown-type.idl");
    let noted = reg.unresolved();
    assert_eq!(noted.len(), 1, "{noted:?}");
    assert_eq!(noted[0].kind, UnresolvedKind::Type);
    assert_eq!(noted[0].name, "Widget");
    assert_eq!(noted[0].at, "n04::S");
    assert!(noted[0].to_string().contains("not declared in this unit"), "{}", noted[0]);
}

/// `::CORBA::TypeCode` is predeclared by the front end rather than declared in
/// the unit, and it is the spelling `CLAUDE.md` requires. It must not become a
/// marker, or the gate refuses every contract that returns one.
#[test]
fn the_predeclared_typecode_is_not_a_gap() {
    let reg = from_str("module m { interface I { ::CORBA::TypeCode describe(); }; };");
    assert!(reg.unresolved().is_empty(), "{:?}", reg.unresolved());
    let id = reg.id_of("m::I").expect("registered").clone();
    let (_, sig) = reg.resolve_operation(&id, "describe").expect("declared");
    assert_eq!(sig.returns, TypeCode::TypeCode);
}
