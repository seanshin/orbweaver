//! A **checked-in generated stub**, through the guard, seen by the content
//! seat — PLAN §7.4 I1 in the reading D010 A3 adds.
//!
//! `bounds_oracle.rs` holds the static and dynamic paths to the same
//! *marshalling* verdict. This file holds them to the same *visibility*: a
//! stage at `orbweaver_mcp::interceptor::SEAT_SAFETY_CONTENT` is handed a
//! static call's payload as the AnyJSON document a dynamic call for the same
//! operation would carry. Nothing about the stub changed to make that true —
//! the fixture is `emitted/f_27_bounds.rs` exactly as it was blessed — which is
//! the property the guard's module docs claim: *which side of the trust
//! boundary a stub runs on is decided by what it is handed*.
//!
//! # The asymmetry this file pins rather than fixes
//!
//! A stub marshals into a probe **before** it calls the invoker, so an
//! argument past its declared bound is refused by the stub, locally, and the
//! guard never hears of the call: no chain, no audit line, no seat. Nothing is
//! sent, so this is not a bypass — but it is a call a content stage never
//! judged and a ledger never recorded, and a deployment counting refusals must
//! know that. `a_stubs_over_bound_argument_is_refused_before_the_guard_hears_of_it`
//! fails the day either the stub or the guard changes that order, so the
//! change is a decision and not a drift. The dry run with values is the
//! instrument that *does* see it: `Guarded::dry_run_with` predicts
//! `Would::Marshal` for the same nine characters.
//!
//! No wire is needed. The `Connection` is dialed to a listener that never
//! answers, and every call here is refused by the content stage — the point is
//! what the stage was handed before it refused.

mod emitted;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use emitted::f_27_bounds::gc27::{Blob, LedgerClient, Record, Tag, WideTag};
use orbweaver_dynamic::json::Json;
use orbweaver_gen::rt::{GiopError, WString};
use orbweaver_giop::{Connection, IiopProfile, Ior, Version};
use orbweaver_mcp::Bridge;
use orbweaver_mcp::dryrun::Would;
use orbweaver_mcp::guard::{Guarded, NO_PERMISSION};
use orbweaver_mcp::identity::Caller;
use orbweaver_mcp::interceptor::{
    CallContext, Interceptor, Outcome, SEAT_SAFETY_CONTENT, STAGE_APPROVAL,
};
use orbweaver_mcp::policy::{Approval, Denied, Exposure, Unannotated};
use orbweaver_registry::Registry;

const LEDGER: &str = "IDL:gc27/Ledger:1.0";

fn registry() -> Registry {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(root.join("../../corpus/golden/27-bounds.idl"))
        .expect("corpus/golden/27-bounds.idl");
    let spec = orbweaver_idl::parse(&src).expect("parses");
    let mut r = Registry::new();
    r.load(&spec).expect("loads");
    r
}

/// A listener nobody accepts on: `connect` completes (the kernel queues it),
/// and nothing this file does needs a byte back.
fn dummy_target() -> (std::net::TcpListener, Ior) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
    let ior = Ior {
        type_id: LEDGER.into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "127.0.0.1".into(),
            port: listener.local_addr().expect("bound").port(),
            object_key: b"ledger-1".to_vec(),
            components: Vec::new(),
        }],
    };
    (listener, ior)
}

/// A content stage that keeps what it was handed and refuses, so that no call
/// in this file goes further than the seat.
struct SeesAndRefuses(Rc<RefCell<Vec<Option<String>>>>);

impl Interceptor for SeesAndRefuses {
    fn before(&mut self, ctx: &CallContext<'_>) -> Outcome {
        self.0.borrow_mut().push(ctx.arguments.map(ToString::to_string));
        Outcome::Refuse(Denied::Intercepted {
            stage: SEAT_SAFETY_CONTENT.to_owned(),
            reason: "this file refuses everything it sees".to_owned(),
        })
    }
}

type Seen = Rc<RefCell<Vec<Option<String>>>>;

/// The fixture: the checked-in `LedgerClient` over a real `Guarded<Connection>`
/// from `Bridge::connect_static`, with a seeing-and-refusing content stage.
fn guarded_client<'r>(
    registry: &'r Registry,
    ior: &Ior,
) -> (LedgerClient<Guarded<'r, Connection>>, Seen) {
    let exposure = Exposure::nothing()
        .allow_interface(LEDGER)
        .assuming_unannotated(Unannotated::Assume("read_only".into()));
    let mut bridge =
        Bridge::new(registry, exposure, "s-guarded-stub").on_behalf_of(Caller::new("alice"));
    let handle = bridge.handles().issue_checked(ior).expect("issued");
    let mut guarded = bridge
        .connect_static(handle.as_str(), Approval::default(), Duration::from_secs(5))
        .expect("dials the dummy target");
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    assert!(guarded.chain_mut().insert_after(
        STAGE_APPROVAL,
        SEAT_SAFETY_CONTENT,
        SeesAndRefuses(Rc::clone(&seen))
    ));
    (LedgerClient::new(guarded), seen)
}

fn refused_by_policy(err: &GiopError) -> bool {
    matches!(err, GiopError::SystemException { id, .. } if id == NO_PERMISSION)
}

/// The generated stub's payload — an attribute setter's `string<8>` and an
/// operation's `(Tag, Record)` — reaches the content seat as the document the
/// dynamic path would carry: parameter names from the contract, a `struct` as
/// an object, an octet sequence as base64, a `wstring` as text.
#[test]
fn a_checked_in_stubs_payload_reaches_the_content_seat_as_anyjson() {
    let registry = registry();
    let (_listener, ior) = dummy_target();
    let (mut client, seen) = guarded_client(&registry, &ior);

    let refused = client.set_title(Tag::new("12345678".into())).unwrap_err();
    assert!(refused_by_policy(&refused), "{refused}");

    let record = Record {
        label: Tag::new("lbl".into()),
        payload: Blob::new(vec![1, 2, 3]),
        wide: WideTag::new(WString("ab".into())),
    };
    let refused = client.keep(Tag::new("k".into()), record).unwrap_err();
    assert!(refused_by_policy(&refused), "{refused}");

    let seen = seen.borrow().clone();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert_eq!(seen[0].as_deref(), Some(r#"{"value":"12345678"}"#));
    let keep = seen[1].clone().expect("the seat was handed the payload");
    let doc = Json::parse(&keep).expect("AnyJSON");
    assert_eq!(doc.get("key").and_then(Json::as_str), Some("k"), "{keep}");
    let entry = doc.get("entry").expect("the struct argument");
    assert_eq!(entry.get("label").and_then(Json::as_str), Some("lbl"), "{keep}");
    assert_eq!(entry.get("payload").and_then(Json::as_str), Some("AQID"), "octets as base64");
    assert_eq!(entry.get("wide").and_then(Json::as_str), Some("ab"), "{keep}");

    // Refused at the seat: the ledger names the stage and none of its prose,
    // and every line is a REFUSE for the principal the bridge ran as.
    let audit = client.conn.audit().join("\n");
    assert!(audit.contains("REFUSE caller=alice"), "{audit}");
    assert!(audit.contains(SEAT_SAFETY_CONTENT), "{audit}");
    assert!(!audit.contains("refuses everything"), "stage prose reached the ledger: {audit}");
    assert!(!audit.contains("12345678"), "a payload reached the ledger: {audit}");
}

/// The asymmetry, pinned: nine characters into a `string<8>` is refused by the
/// stub's own probe, before the guard, so the seat is not reached and nothing
/// is audited — and the dry run with values is what predicts it.
#[test]
fn a_stubs_over_bound_argument_is_refused_before_the_guard_hears_of_it() {
    let registry = registry();
    let (_listener, ior) = dummy_target();
    let (mut client, seen) = guarded_client(&registry, &ior);

    let err = client.set_title(Tag::new("123456789".into())).unwrap_err();
    assert!(
        matches!(&err, GiopError::Decode(msg) if msg.contains("bound")),
        "the stub's probe refuses locally, as bounds_oracle.rs pins: {err}"
    );
    assert!(seen.borrow().is_empty(), "the seat was reached: {:?}", seen.borrow());
    assert!(client.conn.audit().is_empty(), "the guard heard of it: {:?}", client.conn.audit());

    // The instrument that does see it: the same guard, asked, with the values.
    let nine = Json::parse(r#"{"value":"123456789"}"#).expect("json");
    let prediction = client.conn.dry_run_with("_set_title", &nine);
    // The content stage refuses everything, so the row is its refusal — the
    // gate answers first — and the payload's half rides along under its own
    // name.
    assert_eq!(prediction.would(), Would::Refuse, "{}", prediction.to_json());
    let row = prediction.to_json().to_string();
    assert!(row.contains("would_not_marshal") && row.contains("bounded at 8"), "{row}");
    // Without the content stage the same question is the marshalling verdict.
    let exposure = Exposure::nothing()
        .allow_interface(LEDGER)
        .assuming_unannotated(Unannotated::Assume("read_only".into()));
    let mut bridge = Bridge::new(&registry, exposure, "s-plain").on_behalf_of(Caller::new("alice"));
    let handle = bridge.handles().issue_checked(&ior).expect("issued");
    let mut plain = bridge
        .connect_static(handle.as_str(), Approval::default(), Duration::from_secs(5))
        .expect("dials");
    assert_eq!(plain.dry_run("_set_title").would(), Would::Allow);
    assert_eq!(plain.dry_run_with("_set_title", &nine).would(), Would::Marshal);
    // And every line either guard wrote about the questions is a DRYRUN line.
    assert!(plain.audit().iter().all(|l| l.starts_with("DRYRUN-")), "{:?}", plain.audit());
}
