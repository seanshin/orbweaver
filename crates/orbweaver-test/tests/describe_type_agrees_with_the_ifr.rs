//! D024 §5's *"the two must agree"*, measured.
//!
//! `describe_type` has two lives. It is the fourth tool an agent gets at the
//! MCP boundary (`orbweaver_mcp::contract`), answering out of the registry in
//! process; and it is what the Interface Repository's containment walk answers
//! over the wire, as `Contained::describe` (`orbweaver_registry::ifr`, landed
//! 2026-08-25). **Same registry, same question, one local and one remote.**
//! D024 §5: *"If they can disagree, that is a defect and the test that catches
//! it belongs with whichever lands second."* This is that test, and the tool
//! landed second.
//!
//! # What is proved here, and what is proved by construction
//!
//! The three fields both answers derive from a repository id — the unqualified
//! name, the containing module, the version — are **not two implementations
//! held equal by this file.** They are one function,
//! [`orbweaver_registry::ifr::contained_of`], which both halves call. It was a
//! private method on `RepositoryServer` until 2026-08-26, which left the local
//! half no way to reach it and nothing but a test to notice if the two drifted;
//! publishing it made that particular disagreement *impossible* rather than
//! detectable, which CLAUDE.md prefers.
//!
//! So what this file adds is the part a shared function cannot give:
//!
//! * that the local tool and the **live wire reply** carry the same values,
//!   through a real GIOP request, a real dispatch and a real decode — the
//!   difference between "the same function was called" and "the same answer
//!   arrived";
//! * that the **TypeCode** agrees, which is the field neither half derives and
//!   both copy from the registry, in **both byte orders** (the project's rule:
//!   an encoder that only works native-endian passes every local test and fails
//!   in the field);
//! * that the two agree on **which kinds are types at all** — the case that
//!   would otherwise be found by an agent, since `describe` sends an interface,
//!   an exception, a valuetype and a constant down four different description
//!   structs and only one of them is `TypeDescription`.
//!
//! Values are compared **decoded, never as raw buffers** — CDR padding content
//! is undefined and this project has been bitten by byte-for-byte comparison
//! before.
//!
//! # First measurement
//!
//! Recorded because a green test says nothing about whether it ever could have
//! been red. **On the first run of this file the two answers agreed on every
//! field of every case in both byte orders — no defect was found.** That is an
//! honest result and not a strong one: the three derived fields agree by
//! construction (one function), so the only fields that *could* have
//! disagreed were the TypeCode and the choice of description struct.
//!
//! The negative control run to show the file can go red is in the commit
//! message, per D010 §7.2.
//!
//! The `#pragma prefix` case is where the shared function earns its keep — a
//! prefix makes the path segments and the qualified name disagree about how
//! much of the path is prefix, which is precisely what the prefix-blind
//! `split_repository_id` gets wrong — so it is exercised by
//! `a_prefixed_contract_is_where_the_shared_function_earns_its_keep`.
//!
//! *같은 레지스트리, 같은 질문, 하나는 로컬 하나는 원격. 세 필드는 공유 함수라
//! 어긋남이 불가능하고, 이 파일이 더하는 것은 실제 와이어 응답이 같은 값을
//! 가지고 도착한다는 사실이다 — 양쪽 바이트 순서로.*

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_giop::server::{Dispatch, Request, decode_request};
use orbweaver_giop::typecode::{self, TypeCode};
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, Version, encode_request, read_message};
use orbweaver_mcp::policy::Exposure;
use orbweaver_registry::Registry;
use orbweaver_registry::ifr::{self, RepositoryServer, TypeDescription};

const ROOT: &[u8] = b"InterfaceRepository";

/// One contract reaching every shape a type can be, through an interface that
/// is exposed — so the local tool's reachability gate lets all of them through
/// and the two halves are compared on the same set.
///
/// `Ledger` is deliberately reached only through `Account`'s operations, and
/// `Buried` deliberately through nothing at all: the second is the negative
/// control for the local half's own gate.
const IDL: &str = "module bank {
     //@ ai_desc: How much, in the smallest unit
     struct Money { long long units; string currency; };
     enum Kind { DEBIT, CREDIT };
     union Entry switch (long) { case 0: Money amount; default: string note; };
     typedef sequence<Money> Ledger;
     exception Overdrawn { Money shortfall; };
     struct Buried { long nobody_reaches_this; };

     //@ ai_desc: A customer deposit account
     interface Account {
       //@ ai_effect: read_only
       Money balance() raises (Overdrawn);
       //@ ai_effect: read_only
       Ledger history(in Kind kind, in Entry filter);
     };
   };";

const ACCOUNT: &str = "IDL:bank/Account:1.0";

/// Every registered type the fixture reaches, with the description struct
/// `Contained::describe` sends for it.
const TYPES: [&str; 5] = [
    "IDL:bank/Money:1.0",
    "IDL:bank/Kind:1.0",
    "IDL:bank/Entry:1.0",
    "IDL:bank/Ledger:1.0",
    "IDL:bank/Buried:1.0",
];

fn registry() -> Registry {
    let spec = orbweaver_idl::parse(IDL).expect("the fixture parses");
    let mut registry = Registry::new();
    registry.load(&spec).expect("the fixture loads");
    registry
}

/// One GIOP request, framed and decoded exactly as a peer's would be.
///
/// Built through `encode_request` → `read_message` → `decode_request` rather
/// than by hand, because `Request` keeps its body and its offsets private —
/// which is the point: a test that could fabricate one would be testing a
/// shape no peer can send.
fn request(endian: Endian, key: &[u8], operation: &str) -> Request {
    let wire = encode_request(Version::V1_2, endian, 1, key, operation, true, |_| {})
        .expect("the request encodes");
    let mut cursor: &[u8] = &wire;
    let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("the frame reads");
    decode_request(msg).expect("the request decodes")
}

/// Asks the IFR to describe `id` over the wire and returns the reply bytes.
///
/// One dispatch, one buffer: the reply is
/// `Description { DefinitionKind kind; any value; }`, and the `any` is a
/// TypeCode followed by its value **in the same stream** — no encapsulation, no
/// length — so the alignment origin is the encoder's and every read has to stay
/// on one `Decoder`. Splitting the decode across two would restart alignment
/// and is the mistake this helper exists to make impossible.
fn describe_reply(server: &mut RepositoryServer, endian: Endian, id: &str) -> Vec<u8> {
    let key = server.entry_key(id);
    assert!(Dispatch::knows(server, &key), "the IFR must serve {id}");
    let req = request(endian, &key, "describe");
    let mut out = Encoder::new(endian);
    Dispatch::dispatch(server, &req, &mut out)
        .unwrap_or_else(|e| panic!("describe({id}) raised {}", e.id));
    out.finish().expect("the reply finishes")
}

/// The kind and the `any`'s TypeCode — as much of a reply as a non-type needs.
fn describe_head(server: &mut RepositoryServer, endian: Endian, id: &str) -> (u32, TypeCode) {
    let bytes = describe_reply(server, endian, id);
    let mut d = Decoder::new(&bytes, endian);
    let kind = d.get_u32().expect("the most derived kind");
    let any_tc = typecode::decode(&mut d).expect("the any's TypeCode");
    (kind, any_tc)
}

/// The whole `TypeDescription` a `describe` reply carries for one type.
fn wire_type_description(
    server: &mut RepositoryServer,
    endian: Endian,
    id: &str,
) -> (u32, TypeCode, TypeDescription) {
    let bytes = describe_reply(server, endian, id);
    let mut d = Decoder::new(&bytes, endian);
    let kind = d.get_u32().expect("kind");
    let any_tc = typecode::decode(&mut d).expect("the any's TypeCode");
    let desc = TypeDescription::read_from(&mut d).expect("a TypeDescription decodes");
    (kind, any_tc, desc)
}

/// **The agreement.** For every type the fixture declares, in both byte orders,
/// the wire's `TypeDescription` and the tool's JSON carry the same name, the
/// same containing module, the same version and the same TypeCode.
#[test]
fn the_local_answer_and_the_wire_answer_are_the_same_answer() {
    let registry = registry();
    let mut server = RepositoryServer::new("127.0.0.1", 5001, ROOT.to_vec(), registry.clone());

    for endian in [Endian::Big, Endian::Little] {
        for id in TYPES {
            let (_kind, any_tc, wire) = wire_type_description(&mut server, endian, id);
            assert_eq!(
                any_tc,
                ifr::description_tc::type_description(),
                "{id} ({endian:?}): the any must carry a TypeDescription"
            );

            // The local half, through the tool's own rendering rather than
            // through a second hand-written walk of the registry.
            let tc = registry.typecode(id).expect("the fixture registers it");
            let local = orbweaver_mcp::contract::describe_type_json(&registry, id, tc);
            let field = |k: &str| {
                local
                    .get(k)
                    .and_then(orbweaver_dynamic::json::Json::as_str)
                    .unwrap_or_else(|| panic!("{id}: the local answer has no {k:?}: {local}"))
                    .to_owned()
            };

            assert_eq!(field("id"), wire.id, "{id} ({endian:?}): id");
            assert_eq!(field("name"), wire.name, "{id} ({endian:?}): name");
            assert_eq!(field("defined_in"), wire.defined_in, "{id} ({endian:?}): defined_in");
            assert_eq!(field("version"), wire.version, "{id} ({endian:?}): version");

            // The field neither half derives: both copy it from the registry,
            // and the wire round-trips it through CDR in this byte order.
            assert_eq!(
                &wire.tc, tc,
                "{id} ({endian:?}): the TypeCode did not survive the round trip"
            );
        }
    }
}

/// The two halves must agree on **which entries are types at all**.
///
/// `Contained::describe` sends an interface down `InterfaceDescription`, an
/// exception down `ExceptionDescription`, a valuetype down `ValueDescription`
/// and a constant down `ConstantDescription`; only the rest are
/// `TypeDescription`. A local `describe_type` that answered for an interface
/// would be answering a question the wire answers with a different struct —
/// two tools with one name.
///
/// `Registry::typecode` is what keeps them equal, and it is the same predicate
/// the IFR's own branch uses: an `Entry::Interface` has no TypeCode, so the
/// tool has nothing to describe and refuses.
#[test]
fn an_interface_is_not_a_type_to_either_half() {
    let registry = registry();
    let mut server = RepositoryServer::new("127.0.0.1", 5002, ROOT.to_vec(), registry.clone());

    // The wire sends an interface down a different description struct.
    let (_kind, any_tc) = describe_head(&mut server, Endian::Little, ACCOUNT);
    assert_eq!(
        any_tc,
        ifr::description_tc::interface_description(),
        "an interface describes as an InterfaceDescription, not a TypeDescription"
    );

    // And the local half has no TypeCode for it, so `describe_type` refuses
    // rather than inventing one.
    assert!(
        registry.typecode(ACCOUNT).is_none(),
        "an interface entry must not carry a TypeCode, or the two halves would disagree"
    );

    // An exception is a type to the registry and a *different* description to
    // the wire. Recorded rather than asserted equal: this is the one place the
    // two vocabularies genuinely differ, and a future `describe_type` that
    // grew an exception branch has to send ExceptionDescription's fields.
    let (_kind, any_tc) = describe_head(&mut server, Endian::Little, "IDL:bank/Overdrawn:1.0");
    assert_eq!(any_tc, ifr::description_tc::exception_description());
}

/// **Where the shared function earns its keep.**
///
/// Under a `#pragma prefix` the repository id grows path segments the qualified
/// name does not have, so the prefix-blind
/// [`orbweaver_registry::ifr::split_repository_id`] reads part of the prefix as
/// a containing module. [`orbweaver_registry::ifr::contained_of`] asks the
/// registry instead, and is right.
///
/// This is the case where the local half and the wire half *could* have
/// disagreed if the local half had reached for the easy function — and the
/// reason `contained_of` was published rather than reimplemented. The
/// assertion is written against the split as well, so the test states plainly
/// which of the two answers is the wrong one rather than only that they match.
///
/// **The type is declared at file scope on purpose.** A prefixed type inside a
/// module does *not* separate the two functions — the first run of this test
/// used `module bank { struct Money … }` and the `assert_ne!` below caught it,
/// both answering `IDL:acme.example/bank:1.0`. The divergence is at the top
/// level: the qualified name is one segment, so the container is the
/// repository and `defined_in` is empty, while the prefix-blind split sees two
/// path segments and reports the **prefix itself** as a containing module.
#[test]
fn a_prefixed_contract_is_where_the_shared_function_earns_its_keep() {
    let spec = orbweaver_idl::parse(
        "#pragma prefix \"acme.example\"
         struct Money { long long units; };
         interface Vault { Money held(); };",
    )
    .expect("the prefixed fixture parses");
    let mut registry = Registry::new();
    registry.load(&spec).expect("it loads");

    let id = registry
        .ids()
        .find(|id| id.ends_with("/Money:1.0"))
        .expect("the prefixed Money is registered")
        .clone();
    assert!(id.contains("acme.example"), "the fixture must actually carry a prefix: {id}");

    let mut server = RepositoryServer::new("127.0.0.1", 5004, ROOT.to_vec(), registry.clone());
    let (_kind, any_tc, wire) = wire_type_description(&mut server, Endian::Big, &id);
    assert_eq!(any_tc, ifr::description_tc::type_description());

    let tc = registry.typecode(&id).expect("registered");
    let local = orbweaver_mcp::contract::describe_type_json(&registry, &id, tc);
    let field = |k: &str| {
        local.get(k).and_then(orbweaver_dynamic::json::Json::as_str).unwrap_or("").to_owned()
    };
    assert_eq!(field("name"), wire.name, "name under a prefix");
    assert_eq!(field("defined_in"), wire.defined_in, "defined_in under a prefix");
    assert_eq!(field("version"), wire.version, "version under a prefix");

    // And the prefix-blind split is genuinely a different answer here, which is
    // what makes the agreement above worth asserting rather than tautological.
    let (split_name, split_defined_in, _) = ifr::split_repository_id(&id);
    assert_eq!(
        split_name, wire.name,
        "the split gets the name right; it is the container it loses"
    );
    assert_ne!(
        split_defined_in, wire.defined_in,
        "if these are equal the fixture no longer exercises the prefix and this test proves nothing"
    );
}

/// The tool's second gate, which the wire has no equivalent of and must not.
///
/// The IFR serves whatever it holds — it is an Interface Repository, and a peer
/// browsing it is not an agent. The MCP tool is the one that must refuse a type
/// no exposed interface reaches, or it becomes an enumeration tool for an
/// estate nobody exposed. **The two answering differently here is correct**,
/// and it is written down so that a later batch reading the agreement test does
/// not "fix" it.
#[test]
fn the_wire_serves_what_it_holds_and_the_tool_serves_what_is_exposed() {
    let registry = registry();
    let mut server = RepositoryServer::new("127.0.0.1", 5003, ROOT.to_vec(), registry.clone());

    // Reachable from the exposed interface: both answer.
    let open = Exposure::nothing().allow_interface(ACCOUNT);
    assert!(orbweaver_mcp::contract::type_is_reachable(&registry, &open, "IDL:bank/Money:1.0"));

    // Declared, served by the IFR, and reached by no exposed interface: the
    // tool refuses and the repository does not.
    assert!(
        !orbweaver_mcp::contract::type_is_reachable(&registry, &open, "IDL:bank/Buried:1.0"),
        "a type nothing exposed reaches must not be describable through the agent boundary"
    );
    let key = server.entry_key("IDL:bank/Buried:1.0");
    assert!(
        Dispatch::knows(&server, &key),
        "the repository still serves it: browsing an IFR is not the agent boundary"
    );
    let (_kind, any_tc, wire) =
        wire_type_description(&mut server, Endian::Big, "IDL:bank/Buried:1.0");
    assert_eq!(any_tc, ifr::description_tc::type_description());
    assert_eq!(wire.name, "Buried");

    // And with nothing exposed at all, nothing is describable.
    for id in TYPES {
        assert!(!orbweaver_mcp::contract::type_is_reachable(&registry, &Exposure::nothing(), id));
    }
}
