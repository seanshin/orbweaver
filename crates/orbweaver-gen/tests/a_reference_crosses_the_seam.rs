//! PROBE — measured before anything is built, so the "before" is evidence.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_dynamic::json::Json;
use orbweaver_gen::pyservant::{Answerer, PyServant};
use orbweaver_gen::rt::{Dispatch, ObjectHome};
use orbweaver_giop::server::{Request, decode_request};
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, Version, encode_request, read_message};
use orbweaver_registry::{Contract, Registry, Strictness};

const TYPE_ID: &str = "IDL:gc16/Registry:1.0";
const ROOT: &[u8] = b"reg";

fn registry() -> Registry {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/golden/16-object-refs.idl");
    let contract = Contract::load(&path, &Default::default(), Strictness::Checked)
        .expect("the corpus contract must load");
    let mut registry = Registry::new();
    registry.load(&contract.spec).expect("the contract must build a registry");
    registry
}

struct Spy {
    seen: Rc<RefCell<Vec<Json>>>,
    answer: Json,
}

impl Answerer for Spy {
    fn ask(&mut self, call: &Json) -> Result<Json, String> {
        self.seen.borrow_mut().push(call.clone());
        Ok(self.answer.clone())
    }
}

fn request(key: &[u8], operation: &str, args: impl FnOnce(&mut Encoder)) -> Request {
    let wire =
        encode_request(Version::V1_2, Endian::Little, 1, key, operation, true, args).expect("enc");
    let mut cursor: &[u8] = &wire;
    let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("frame");
    decode_request(msg).expect("decode")
}

fn ok_returns(j: Json) -> Json {
    Json::Object(BTreeMap::from([(
        "ok".to_owned(),
        Json::Object(BTreeMap::from([("returns".to_owned(), j)])),
    )]))
}

fn objref(handle: &str) -> Json {
    Json::Object(BTreeMap::from([("_ref".to_owned(), Json::String(handle.to_owned()))]))
}

#[test]
fn probe_what_a_foreign_servant_cannot_do() {
    let reg = registry();
    let home = ObjectHome::new("127.0.0.1", 4001, ROOT.to_vec());
    let seen = Rc::new(RefCell::new(Vec::new()));

    // 1. Can a foreign servant return a reference to an object it hosts?
    let mut s = PyServant::new(
        &reg,
        TYPE_ID,
        Spy { seen: seen.clone(), answer: ok_returns(objref("shelf-7")) },
    )
    .expect("servant");
    let req = request(ROOT, "lookup", |e| e.put_str("shelf-7"));
    let mut out = Encoder::continuing_at(Endian::Little, 24);
    match s.dispatch_body(&req, &mut out) {
        Ok(k) => println!("MINT  -> {k:?}"),
        Err(e) => println!("MINT  -> refused {} completed={:?}", e.id, e.completed),
    }

    // 2. Does the call document tell the servant WHICH object was addressed?
    seen.borrow_mut().clear();
    let mut s2 = PyServant::new(
        &reg,
        TYPE_ID,
        Spy {
            seen: seen.clone(),
            answer: ok_returns(Json::Object(BTreeMap::from([("_ref".to_owned(), Json::Null)]))),
        },
    )
    .expect("servant");
    for key in [ROOT.to_vec(), home.key_of("/Registry/", "shelf-7")] {
        let r = request(&key, "lookup", |e| e.put_str("x"));
        let mut o = Encoder::continuing_at(Endian::Little, 24);
        let _ = s2.dispatch_body(&r, &mut o);
    }
    for (n, doc) in seen.borrow().iter().enumerate() {
        println!("OID   -> call {n} = {doc}");
    }

    // 3. Does it answer for keys that are not its own?
    println!("KNOWS a key from another home -> {}", s2.knows(b"somebody-elses-key"));
}
