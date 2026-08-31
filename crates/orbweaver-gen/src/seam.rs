//! The seam a servant in **any** language is dispatched into, and its protocol
//! as data.
//!
//! `docs/decisions/D032-what-a-language-binding-is.md` §3 decomposes a servant
//! into three layers and says that **exactly one of them may ever be
//! per-language**:
//!
//! | Layer | May differ per language? |
//! |---|---|
//! | the contract — first-party IDL, repository ids, exception shapes | **No** |
//! | the value representation — AnyJSON v1 | **No** |
//! | the dispatch binding — receiving a call in L, returning a reply | **Yes** |
//!
//! This module is the first two rows. [`ForeignServant`] decodes the request,
//! resolves the operation against the contract, converts every value through
//! AnyJSON v1, frames the reply, and answers every refusal by **calling** the
//! constructor that owns it. What is left for a language to supply is one
//! function — [`Answerer::ask`], "put this document, give me the next one" —
//! and a runtime that speaks AnyJSON v1. Adding C or Java costs an emitter and
//! that runtime, and costs **nothing here**.
//!
//! *세 층 중 오직 하나만 언어마다 다를 수 있다. 이 모듈은 나머지 둘이다.*
//!
//! # The seam is not GIOP
//!
//! D032 §6's first refusal, and D030 §4's: **the bridge carries a dispatch, not
//! a wire.** GIOP framing, CDR, alignment, byte order, codeset negotiation, the
//! reply status and the repository id on a user exception all happen on this
//! side of the line. What crosses is an operation name and a bag of already
//! decoded values. A binding that spoke GIOP would be a second ORB wearing a
//! binding's name, and it would owe a second set of alignment bugs.
//!
//! # Why the protocol is a value and not a comment
//!
//! [`protocol()`] returns the seam's whole document shape — every key, every
//! reply family, the completion ordinals, the two forms a reference takes — as
//! one AnyJSON document **built from the same constants this file dispatches
//! with**. Every runtime that implements the far side publishes the same
//! document, built from the constants *it* reads with, and
//! `tests/the_seam_is_one_protocol.rs` asserts they are equal.
//!
//! Before this existed the protocol was a comment in three places
//! (`pyservant.rs`, `py_bridge.rs`, `python_rt.py`) and a set of string
//! literals in each. Three is where `CLAUDE.md`'s *"a sentence many layers say
//! is a fact"* was already measured; a third and fourth language makes it five.
//! The discipline is `orbweaver_giop::server::serve_one_ordering()`'s: the
//! order stopped being a comment two implementations agreed with and became a
//! value both are asserted against.
//!
//! *프로토콜은 주석이 아니라 값이다 — 각 런타임이 자기가 읽는 상수로 같은 문서를
//! 만들고, 게이트가 같은지 본다.*
//!
//! # What a foreign servant can do here that it could not
//!
//! Measured 2026-08-26, before this module existed: a foreign servant could not
//! tell **which object** it had been called on (two calls to two different
//! object keys produced byte-identical call documents), claimed **every** key
//! in the process, and could not **return a reference** to any object it hosts
//! — the reply was refused `MARSHAL`, because AnyJSON resolves a handle only
//! against the table that issued one and nothing had issued a handle for an
//! object about to be named. So `Registry::lookup` from
//! `corpus/golden/16-object-refs.idl` could not be written in a foreign
//! language at all, and D029 §6.1's Language row could not close: a servant
//! that cannot hand out a reference cannot participate in naming, in trading,
//! or in any forward.
//!
//! [`ObjectIdentity`] closes those three. What it does **not** close is named
//! at [`ForeignServant::dispatch_body`] and in `docs/COMPONENTS.md`: a
//! reference *arriving* as an argument is still a handle the far side cannot
//! invoke, because invoking it would need a call to travel the other way
//! through [`Answerer`] and this protocol has no message for that yet.

use std::collections::{BTreeMap, BTreeSet};

use orbweaver_cdr::Encoder;
use orbweaver_dynamic::anyjson::{self, References};
use orbweaver_dynamic::invoke::{self, InvokeError};
use orbweaver_dynamic::json::Json;
use orbweaver_giop::Version;
use orbweaver_giop::codeset::{CodeSetId, WideCodec};
use orbweaver_giop::server::{Completion, Dispatch, DispatchBody, Request, SystemException};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_giop::{Connection, Error as GiopError};
use orbweaver_registry::{OperationSig, ParamDirection, Registry};

use crate::rt::{OBJECT_ID, ObjectHome, UNKNOWN};

// ── The protocol, as constants every layer reads with ────────────────────────

/// The envelope a call travels in when the far side is the **parent** process
/// and the bridge is the child — `{"call": {…}}`.
///
/// Part of the protocol rather than of one bridge, because the direction is a
/// property of the deployment and not of the language: a binding whose runtime
/// starts the bridge reads this key, and one that is started *by* the bridge
/// does not. Both are the same protocol.
pub const ENVELOPE_CALL: &str = "call";

/// Call: the repository id of the interface the servant answers for.
pub const CALL_INTERFACE: &str = "id";
/// Call: the operation name, `_get_`/`_set_` accessors included.
pub const CALL_OPERATION: &str = "op";
/// Call: which object of that interface this call was addressed to.
pub const CALL_OBJECT: &str = "oid";
/// Call: one member per `in`/`inout` parameter, by its IDL name.
pub const CALL_ARGUMENTS: &str = "args";
/// Call: present and `true` only when the operation is `oneway`.
pub const CALL_ONEWAY: &str = "oneway";

/// Reply: the servant answered.
pub const REPLY_OK: &str = "ok";
/// Reply, inside [`REPLY_OK`]: the declared result, absent when `void`.
pub const REPLY_RETURNS: &str = "returns";
/// Reply, inside [`REPLY_OK`]: one member per `out`/`inout` parameter.
pub const REPLY_OUTPUTS: &str = "outputs";
/// Reply: the servant raised something its operation declares.
pub const REPLY_USER_EXCEPTION: &str = "user_exception";
/// Reply: the servant raised a system exception.
pub const REPLY_SYSTEM_EXCEPTION: &str = "system_exception";
/// Reply: the far side broke before the servant was reached.
pub const REPLY_ERROR: &str = "error";

/// In an exception: its repository id.
pub const EXCEPTION_ID: &str = "id";
/// In a user exception: its members, as AnyJSON.
pub const EXCEPTION_MEMBERS: &str = "members";
/// In a system exception: the OMG minor code.
pub const EXCEPTION_MINOR: &str = "minor";
/// In a system exception: §4.11.4's completion status, **as the ordinal**.
///
/// Unnamed on purpose. §4.11.4 numbers `COMPLETED_YES` 0, `COMPLETED_NO` 1 and
/// `COMPLETED_MAYBE` 2; this project has transposed the first two once already,
/// and naming them in a second language would be a second place to get the same
/// numbering wrong. A third and fourth language would be a fourth and fifth.
pub const EXCEPTION_COMPLETED: &str = "completed";

/// The prefix that turns a handle into **a reference to an object this servant
/// hosts**, rather than a lookup of one it was handed.
///
/// `{"_ref": "oid:shelf-7"}` is the far side saying *"a reference to my object
/// `shelf-7`"*. It is minted on this side, by [`ObjectHome`], advertising the
/// repository id **the contract declares for that slot** — never one the far
/// side spelled, because a servant that could name the type could name the
/// wrong one and a caller would narrow against it.
///
/// The prefix cannot collide with an issued handle: those are `local-N`.
///
/// *접두사 하나가 "내가 호스팅하는 객체에 대한 참조"를 뜻한다. 주소는 이쪽에서
/// 만들고, 저장소 id는 계약이 정한다.*
pub const OWN_OBJECT_PREFIX: &str = "oid:";

/// The repository id a servant answers when the thing that failed was the seam
/// itself, rather than anything the contract describes.
///
/// `UNKNOWN` and not `INTERNAL`: §4.11 gives `UNKNOWN` to an exception the
/// caller's contract cannot name, which is exactly what a dead child process or
/// an undeclared foreign exception is from the caller's side. `INTERNAL` would
/// claim we know the servant's invariant broke, and for a seam failure we do
/// not know that.
pub const SEAM_FAILURE: &str = UNKNOWN;

/// `{k: v, …}` as a `Json`, for the reply documents this module frames.
fn json_object<const N: usize>(pairs: [(&str, Json); N]) -> Json {
    Json::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

/// Dial one call over `conn` and frame its answer as a seam reply document.
///
/// **One function, two callers, because it is one sentence.** `orbweaver-py-bridge`
/// has said it since the bridge existed — take a request naming an interface, an
/// operation and arguments; convert the arguments; invoke; frame `ok`,
/// `user_exception` or `system_exception`. D038's nested request needs exactly
/// that sentence said again, for a handle the far side names instead of for a
/// line the bridge read, and this project's rule is that a sentence more than
/// one layer must give belongs to one function reachable from every layer that
/// owes it. Restating it in `ForeignServant` would have been the `pub(crate)`
/// defect with a new coat: two framings of the same reply, drifting the first
/// time a completion status or an exception shape changed.
///
/// `request` carries `id`, `op` and `args`; the caller decides where those came
/// from.
pub fn perform_call(
    conn: &mut Connection,
    registry: &Registry,
    handles: &mut dyn References,
    request: &Json,
) -> Result<Json, String> {
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
            Ok(json_object([(
                "ok",
                json_object([("returns", returns), ("outputs", Json::Object(outputs))]),
            )]))
        }
        Err(InvokeError::User(u)) => {
            let members = match (&u.members, registry.typecode(&u.id)) {
                (Some(v), Some(tc)) => anyjson::to_json(tc, v, handles)
                    .map_err(|e| format!("the raised {}: {e}", u.id))?,
                // An id the registry never heard of still names a contract the
                // caller was not built against, which is the useful half.
                _ => Json::Null,
            };
            Ok(json_object([(
                "user_exception",
                json_object([("id", Json::String(u.id.clone())), ("members", members)]),
            )]))
        }
        Err(InvokeError::Transport(GiopError::SystemException { id, minor, completed })) => {
            Ok(json_object([(
                "system_exception",
                json_object([
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

/// Carry out one nested request from the far side, or refuse it.
///
/// D038 option A. Three things this must not do, and each is one of §3's
/// invariants:
///
/// * **It never tells the far side an address.** The request names a handle;
///   the `Ior` behind it is read here and never travels back. A design that
///   answered `local-3` with a stringified IOR would close L4 by opening the
///   leak §4.7 exists to prevent.
/// * **The handle table is the boundary.** A handle nobody issued is refused —
///   `resolve` returns `None` and that is the end of it — so the far side
///   guessing `local-99` reaches nothing. It cannot name an address it was not
///   handed, because the only names it has are the ones this table minted.
/// * **A refusal is an answer, not an `Err`.** Every path here returns a
///   document, so a foreign servant can catch a failed nested call and go on
///   answering the call it is inside.
///
/// **And the connection is a fresh one, which is §2.1's rule rather than an
/// implementation choice.** The nested call is made on a connection this
/// servant owns and never on the one the request arrived on: that connection is
/// mid-request — its reply has not been written — and writing a second request
/// down it would interleave two conversations on a stream that has no room for
/// two. It is also the deadlock D038 §2.1 names: a server that serves one
/// message at a time cannot answer a nested call that arrives on the socket it
/// is already busy with. Dialling out is what makes the shape reachable at all,
/// and dialling *here* is what keeps it survivable.
fn resolve_nested(refs: &SeamReferences, registry: &Registry, invoke: &Json) -> Json {
    let Some(handle) = invoke.get(INVOKE_HANDLE).and_then(Json::as_str) else {
        return nested_refusal("a nested request needs a \"handle\"");
    };
    let Some(op) = invoke.get(INVOKE_OPERATION).and_then(Json::as_str) else {
        return nested_refusal("a nested request needs an \"op\"");
    };
    let Some(ior) = References::resolve(refs, handle) else {
        return nested_refusal(&format!(
            "no reference has been issued the handle {handle:?} — the handle table is the \
             boundary, and a handle nobody issued names nothing"
        ));
    };
    let args = invoke.get(INVOKE_ARGUMENTS).cloned().unwrap_or(Json::Object(BTreeMap::new()));

    // A connection of this servant's own, per §2.1. Short timeout on purpose:
    // this dial happens while a caller waits on a reply two layers up, so a
    // hung connect must come back as an answer rather than hold that reply.
    let mut conn = match Connection::connect(&ior, std::time::Duration::from_secs(10)) {
        Ok(c) => c,
        Err(e) => return nested_refusal(&format!("dialling {handle:?} failed: {e}")),
    };
    let request = json_object([
        ("id", Json::String(ior.type_id.clone())),
        ("op", Json::String(op.to_owned())),
        ("args", args),
    ]);
    // The table is read-only for a nested call: a reference coming back in the
    // ANSWER would need issuing, and that is the widening §3 does not grant
    // here. `LocalReferences` gives the framing somewhere to put one and keeps
    // it out of the boundary table.
    let mut scratch = orbweaver_dynamic::anyjson::LocalReferences::new();
    match perform_call(&mut conn, registry, &mut scratch, &request) {
        Ok(answer) => answer,
        Err(e) => nested_refusal(&format!("the nested call to {handle:?} failed: {e}")),
    }
}

/// The answer to a nested request that this side will not carry out.
///
/// **A seam failure, not a servant's refusal** — D038 §3's second invariant.
/// The far side naming a handle nobody issued is a wiring mistake inside one
/// process; it never travelled a wire and no peer did anything wrong, so it
/// must not be framed as a `user_exception` the calling contract appears to
/// declare. It comes back as `UNKNOWN` with completion `MAYBE`, which is the
/// same sentence [`ForeignServant::seam_failure`] gives for the same class, and
/// `ask`'s error contract is untouched: this is a well-formed answer, not
/// `Err`. That is §3's third invariant, and it is what lets a foreign servant
/// catch a failed nested call instead of losing the conversation.
pub fn nested_refusal(why: &str) -> Json {
    json_object([(
        REPLY_SYSTEM_EXCEPTION,
        json_object([
            (EXCEPTION_ID, Json::String(SEAM_FAILURE.to_owned())),
            (EXCEPTION_MINOR, Json::Number("0".to_owned())),
            (EXCEPTION_COMPLETED, Json::Number(completion_ordinal(Completion::Maybe).to_string())),
            // Not part of the protocol document and deliberately so: the far
            // side dispatches on the three keys above, and this is for whoever
            // is reading the conversation. A refusal nobody can explain is the
            // one this project pays for twice.
            ("why", Json::String(why.to_owned())),
        ]),
    )])
}

/// What version of this protocol [`protocol()`] describes.
///
/// **2 since 2026-08-31**, when the far side gained a message of its own
/// ([`ENVELOPE_INVOKE`]). A version-1 parent reading a version-2 child would
/// see an `invoke` document where it expected a reply and would fail as though
/// the servant had answered nonsense, so this is not a compatible addition
/// dressed as one: the direction the conversation can travel changed. D038,
/// approved 2026-08-31, option A.
pub const PROTOCOL_VERSION: &str = "2";

/// The envelope the **far side** wraps a nested request in, mid-answer.
///
/// D038 option A. While the far side is answering a call it may need to invoke
/// a reference it was handed — `local-3` in the handle table — and it cannot
/// dial: it has never been told an address, which is §4.7's rule and stays
/// true. So it asks this side to dial on its behalf, by handle.
///
/// **This is what makes the seam re-entrant**, which D038 §2 says is a property
/// and not a detail: while the parent waits for the reply to call *C*, the
/// child sends one of these, and the parent must answer it before the reply to
/// *C* can arrive. The read loop on both sides stops being *read the reply* and
/// becomes *read the next document, which may be a reply or may be a request.*
pub const ENVELOPE_INVOKE: &str = "invoke";

/// The envelope **this side** wraps the answer to a nested request in.
///
/// Distinct from [`ENVELOPE_CALL`] so the far side's reader can tell a fresh
/// call from the answer to the request it is waiting on — without that, a
/// servant that invoked while being called would take the answer to its own
/// nested request as a new call addressed to it.
pub const ENVELOPE_ANSWER: &str = "answer";

/// Which handle the far side is asking us to invoke. A handle, never an
/// address: D038 §3's first invariant.
pub const INVOKE_HANDLE: &str = "handle";
/// The operation to invoke on it.
pub const INVOKE_OPERATION: &str = "op";
/// Its arguments, in AnyJSON v1 as everywhere else in this protocol.
pub const INVOKE_ARGUMENTS: &str = "args";

/// The whole seam, as one document, built from the constants above.
///
/// This is what a runtime in another language is written against, and what its
/// own copy is compared to. It is deliberately readable by a program that is
/// not this emitter: `cargo run -p orbweaver-gen --bin seam-protocol` prints
/// it, so a C or Java runtime's author does not have to read Rust to implement
/// the far side, and their test does not have to trust that they read it right.
pub fn protocol() -> Json {
    fn s(v: &str) -> Json {
        Json::String(v.to_owned())
    }
    fn n(v: u32) -> Json {
        Json::Number(v.to_string())
    }
    fn o(pairs: impl IntoIterator<Item = (&'static str, Json)>) -> Json {
        Json::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }
    // Keys are the constants themselves, so a layer that starts reading a
    // different key changes this document rather than drifting from it.
    o([
        ("version", s(PROTOCOL_VERSION)),
        ("envelope", o([("call", s(ENVELOPE_CALL))])),
        (
            "call",
            o([
                ("interface", s(CALL_INTERFACE)),
                ("operation", s(CALL_OPERATION)),
                ("object", s(CALL_OBJECT)),
                ("arguments", s(CALL_ARGUMENTS)),
                ("oneway", s(CALL_ONEWAY)),
            ]),
        ),
        (
            "reply",
            o([
                ("ok", s(REPLY_OK)),
                ("returns", s(REPLY_RETURNS)),
                ("outputs", s(REPLY_OUTPUTS)),
                ("user_exception", s(REPLY_USER_EXCEPTION)),
                ("system_exception", s(REPLY_SYSTEM_EXCEPTION)),
                ("error", s(REPLY_ERROR)),
            ]),
        ),
        (
            "exception",
            o([
                ("id", s(EXCEPTION_ID)),
                ("members", s(EXCEPTION_MEMBERS)),
                ("minor", s(EXCEPTION_MINOR)),
                ("completed", s(EXCEPTION_COMPLETED)),
            ]),
        ),
        // Not spelled here from a table of three: each ordinal is read back
        // out of the same `Completion` this file dispatches with, so the
        // document cannot disagree with the code that writes the reply.
        (
            "completed",
            o([
                ("yes", n(completion_ordinal(Completion::Yes))),
                ("no", n(completion_ordinal(Completion::No))),
                ("maybe", n(completion_ordinal(Completion::Maybe))),
            ]),
        ),
        ("reference", o([("own_object_prefix", s(OWN_OBJECT_PREFIX))])),
        // D038's nested request: the far side's own message, and the envelope
        // this side answers it in.
        (
            "invoke",
            o([
                ("envelope", s(ENVELOPE_INVOKE)),
                ("answer_envelope", s(ENVELOPE_ANSWER)),
                ("handle", s(INVOKE_HANDLE)),
                ("operation", s(INVOKE_OPERATION)),
                ("arguments", s(INVOKE_ARGUMENTS)),
            ]),
        ),
    ])
}

/// §4.11.4's ordinal for one completion status.
///
/// One `match`, in the crate that frames the reply, so [`protocol()`] and
/// [`system_exception_from`] cannot number them differently.
const fn completion_ordinal(c: Completion) -> u32 {
    match c {
        Completion::Yes => 0,
        Completion::No => 1,
        Completion::Maybe => 2,
    }
}

/// The reverse, refusing anything that is not one of the three.
///
/// Absent or unrecognised is **refused rather than defaulted**. A Rust servant
/// cannot reach a `SystemException` without naming the status — `rt::Raising`
/// has no `Default` and no `From` for exactly this reason — and a foreign
/// servant that omits it must not be quietly given one.
const fn completion_of(ordinal: u32) -> Option<Completion> {
    match ordinal {
        0 => Some(Completion::Yes),
        1 => Some(Completion::No),
        2 => Some(Completion::Maybe),
        _ => None,
    }
}

// ── Object identity: which object, and how to name another ───────────────────

/// What separates a root object key from an object id, for one interface.
///
/// The **one** spelling of the key scheme's infix. `skeleton.rs` writes it into
/// every generated `<I>Refs::KEY_INFIX` by calling this, and [`ObjectIdentity`]
/// derives it at run time by calling this, so a generated Rust servant and a
/// foreign servant for the same contract address the same objects by
/// construction rather than by agreement.
pub fn key_infix(simple_name: &str) -> String {
    format!("/{simple_name}/")
}

/// The same, for an interface named by its repository id.
///
/// Spelled through [`crate::ident`] and [`crate::path_of`] because that is what
/// `emit_skeleton` spells it through; anything else would be a second rule.
pub fn key_infix_of(repository_id: &str) -> String {
    let simple = crate::path_of(repository_id).last().cloned().unwrap_or_default();
    key_infix(&crate::ident(&simple))
}

/// Where a foreign servant's objects live, so that it can be told which one it
/// is and can name another.
///
/// This is [`crate::rt::ObjectHome`] plus the reference table, and it is the
/// non-generated twin of the generated `<I>Refs`/`<I>Target` pair. A Rust
/// servant is handed `oid()`, `reference()` and `sibling()` by its skeleton; a
/// foreign servant is handed the same three facts through the seam's call
/// document and [`OWN_OBJECT_PREFIX`].
#[derive(Debug, Clone)]
pub struct ObjectIdentity {
    home: ObjectHome,
    own_infix: String,
}

impl ObjectIdentity {
    /// The identity scheme for `repository_id`'s objects, rooted at `home`.
    pub fn new(home: ObjectHome, repository_id: &str) -> Self {
        Self { home, own_infix: key_infix_of(repository_id) }
    }

    /// Where these objects are published.
    pub fn home(&self) -> &ObjectHome {
        &self.home
    }

    /// The infix this servant's own objects are keyed under.
    pub fn own_infix(&self) -> &str {
        &self.own_infix
    }

    /// Which object a request's key addresses, or `None` if this home did not
    /// derive it. The empty oid is the default object.
    pub fn oid_of<'a>(&self, object_key: &'a [u8]) -> Option<&'a str> {
        self.home.oid_of(&self.own_infix, object_key)
    }
}

/// The reference table on this side of the seam.
///
/// Two jobs, and the second is the one [`orbweaver_dynamic::anyjson::LocalReferences`]
/// cannot do:
///
/// * a reference **arriving** as an argument is issued a handle (`local-N`),
///   which the far side may pass back and cannot dial — §4.7's bearer-address
///   rule, unchanged;
/// * a handle the far side spells `oid:<oid>` is **minted**, into an address
///   under this servant's own home and under the repository id the *contract*
///   declares for the slot it is going into.
///
/// Minting is pure: nothing is stored, so a reference the far side names
/// survives a restart exactly as a generated Rust servant's does, and a caller
/// cannot make the process grow by asking for names.
#[derive(Debug, Default)]
pub struct SeamReferences {
    by_handle: BTreeMap<String, orbweaver_giop::Ior>,
    next: u64,
    identity: Option<ObjectIdentity>,
}

impl SeamReferences {
    /// A table that can issue handles and cannot mint.
    pub fn new() -> Self {
        Self::default()
    }

    /// A table that can also mint, under `identity`'s home.
    pub fn with_identity(identity: ObjectIdentity) -> Self {
        Self { identity: Some(identity), ..Self::default() }
    }

    /// How many references arrived and were issued handles.
    pub fn len(&self) -> usize {
        self.by_handle.len()
    }

    /// Whether nothing has been issued.
    pub fn is_empty(&self) -> bool {
        self.by_handle.is_empty()
    }
}

impl References for SeamReferences {
    fn issue(&mut self, ior: &orbweaver_giop::Ior) -> String {
        if let Some((h, _)) = self.by_handle.iter().find(|(_, v)| *v == ior) {
            return h.clone();
        }
        self.next += 1;
        let handle = format!("local-{}", self.next);
        self.by_handle.insert(handle.clone(), ior.clone());
        handle
    }

    fn resolve(&self, handle: &str) -> Option<orbweaver_giop::Ior> {
        self.by_handle.get(handle).cloned()
    }

    fn resolve_as(&self, handle: &str, declared_type: &str) -> Option<orbweaver_giop::Ior> {
        if let Some(ior) = self.by_handle.get(handle) {
            return Some(ior.clone());
        }
        let oid = handle.strip_prefix(OWN_OBJECT_PREFIX)?;
        // No identity means no home, which means there is no address to mint
        // under. `None` here reaches the caller as the same refusal a forged
        // handle gets, which is the honest answer: this servant cannot name
        // that object.
        let identity = self.identity.as_ref()?;
        // The infix comes from the DECLARED type, not from this servant's own
        // interface, so a factory answering `Widget create()` mints a key a
        // `Widget` skeleton in this process will accept. The declared type is
        // also the repository id the reference advertises — the far side never
        // gets to choose either.
        let infix = key_infix_of(declared_type);
        Some(identity.home.ior(declared_type, identity.home.key_of(&infix, oid)))
    }
}

// ── The binding ──────────────────────────────────────────────────────────────

/// What answers a call on the servant's behalf.
///
/// A trait rather than a process, for one reason that is worth the
/// indirection: it lets the whole of [`ForeignServant`] — argument decoding,
/// the AnyJSON conversion, the reply framing, every refusal — be **executed by
/// a test with no child process, no socket and no fixture**. Every language's
/// runtime makes the same argument for its own loopback, which is why the
/// seam's behaviour is measurable on a machine too busy to start a peer.
pub trait Answerer {
    /// Puts one call to the servant and waits for its answer.
    ///
    /// `Err` is for the seam breaking — the child is gone, the line was not
    /// JSON — never for the servant refusing, which is a well-formed answer
    /// and comes back as `Ok` carrying [`REPLY_USER_EXCEPTION`] or
    /// [`REPLY_SYSTEM_EXCEPTION`].
    fn ask(&mut self, call: &Json) -> Result<Json, String>;

    /// [`ask`](Self::ask), but `resolve` answers any **nested request** the far
    /// side sends while it is answering.
    ///
    /// D038 option A, approved 2026-08-31. The far side may be holding a
    /// reference it was handed and unable to dial it — it has never been told
    /// an address, which is §4.7's rule and stays true — so mid-answer it sends
    /// `{"invoke": …}` naming a handle, and this side answers `{"answer": …}`
    /// before the reply to the original call can arrive. **The conversation
    /// nests**, which is why this is a protocol property and not a detail.
    ///
    /// The default implementation ignores `resolve` and delegates to `ask`,
    /// which is exactly what a runtime written before this message existed
    /// does: it never sends one, so it never needs one answered. That default
    /// is not a stub — it is the honest behaviour of a version-1 far side, and
    /// it means adding this method broke no implementation.
    fn ask_resolving(
        &mut self,
        call: &Json,
        resolve: &mut dyn FnMut(&Json) -> Json,
    ) -> Result<Json, String> {
        let _ = resolve;
        self.ask(call)
    }
}

/// A servant written in another language, dispatched into by our Rust ORB.
///
/// Implements [`Dispatch`], so it goes wherever a generated Rust skeleton goes
/// and the server cannot tell them apart. The `&mut self` shape is deliberate
/// and matches [`crate::skeleton`]'s: `Server::serve` wraps a `Dispatch` in a
/// mutex for the duration of one message, which is also exactly the discipline
/// a single-threaded child process on one pair of pipes needs. A foreign
/// servant is therefore *no more* serialized than a generated Rust one — a
/// difference this seam was expected to have and does not.
///
/// Nothing about this type names a language. `pyservant::PyServant` is an alias
/// for it, kept because it is what the first binding was called.
pub struct ForeignServant<A: Answerer> {
    id: String,
    /// The resolved callable surface: every operation and attribute accessor
    /// this interface answers to, inherited ones included.
    ///
    /// [`crate::surface::callable_operations`] computes it, and a generated
    /// client of any target reads the same table. That reuse is the property,
    /// not a convenience: a foreign servant answers exactly the names a client
    /// of the same contract can send, because one function decides both.
    ops: BTreeMap<String, OperationSig>,
    /// `_is_a`'s answer set: this interface, its resolved ancestors, and
    /// `CORBA::Object`. The same set [`crate::skeleton`] bakes into a generated
    /// skeleton, computed the same way.
    ancestry: BTreeSet<String>,
    registry: Registry,
    answerer: A,
    refs: SeamReferences,
    identity: Option<ObjectIdentity>,
}

impl<A: Answerer> ForeignServant<A> {
    /// A servant for `id`, answering through `answerer`, serving one object.
    ///
    /// Fails when `id` names no interface in `registry`, which is the one
    /// mistake worth refusing at construction: a servant that dispatches
    /// nothing would otherwise answer every call `BAD_OPERATION` and look like
    /// a contract mismatch.
    ///
    /// Without a home this servant cannot be told which object it is and
    /// cannot name one — [`ForeignServant::with_home`] is what gives it both.
    pub fn new(registry: &Registry, id: &str, answerer: A) -> Result<Self, String> {
        if registry.interface(id).is_none() {
            return Err(format!("{id} names no interface in this contract"));
        }
        let mut ancestry: BTreeSet<String> = BTreeSet::new();
        ancestry.insert(id.to_owned());
        ancestry.extend(registry.ancestors(id));
        ancestry.insert(OBJECT_ID.to_owned());
        Ok(Self {
            id: id.to_owned(),
            ops: crate::surface::callable_operations(registry, id),
            ancestry,
            registry: registry.clone(),
            answerer,
            refs: SeamReferences::new(),
            identity: None,
        })
    }

    /// The same servant, serving **many** objects under `home`.
    ///
    /// Three things change, and they are the three the probe in
    /// `tests/a_reference_crosses_the_seam.rs` measured as missing:
    ///
    /// * every call document carries [`CALL_OBJECT`], so the far side knows
    ///   which object it is — the seam's half of `<I>Target::oid()`;
    /// * [`Dispatch::knows`] answers for this home's keys only, instead of
    ///   claiming every key in the process;
    /// * a reply may name `oid:<oid>` where a reference goes, and it is minted
    ///   — the seam's half of `<I>Refs::reference()` and `<I>Target::sibling()`.
    pub fn with_home(mut self, home: ObjectHome) -> Self {
        let identity = ObjectIdentity::new(home, &self.id);
        self.refs = SeamReferences::with_identity(identity.clone());
        self.identity = Some(identity);
        self
    }

    /// The interface this servant answers for.
    pub fn type_id(&self) -> &str {
        &self.id
    }

    /// Where this servant's objects live, if it was given a home.
    pub fn identity(&self) -> Option<&ObjectIdentity> {
        self.identity.as_ref()
    }

    /// What answers on the servant's behalf.
    ///
    /// Borrowed rather than hidden so a test can read what the far side was
    /// actually put — the call documents are the seam's observable behaviour,
    /// and a test that could only see the reply bytes could not tell a servant
    /// that was *told* which object it is from one that guessed right.
    pub fn answerer(&self) -> &A {
        &self.answerer
    }

    /// Every operation name this servant will dispatch, `_is_a` and
    /// `_non_existent` excluded.
    pub fn operations(&self) -> impl Iterator<Item = &str> {
        self.ops.keys().map(String::as_str)
    }

    /// The seam broke. Completion is `MAYBE` and that is not caution for its
    /// own sake: the call may have been written to the child and run before the
    /// answer was lost, and this project's own rule is that telling a caller
    /// "safe to retry" wrongly is worse than telling it nobody knows.
    fn seam_failure() -> SystemException {
        SystemException { id: SEAM_FAILURE.to_owned(), minor: 0, completed: Completion::Maybe }
    }

    /// Which object this request addresses, as the far side is told it.
    ///
    /// The empty string is the default object. A servant with no home answers
    /// the empty string too — truthfully, because without a home it serves one
    /// object and cannot tell them apart. That is why the key is always
    /// present: a far side that had to branch on its absence would be a far
    /// side with a rule to get wrong.
    fn oid_for<'a>(&self, request: &'a Request) -> &'a str {
        match &self.identity {
            Some(i) => i.oid_of(&request.object_key).unwrap_or(""),
            None => "",
        }
    }

    /// The wide-character codec **this request** implies.
    ///
    /// Not [`orbweaver_dynamic`]'s fixed default, which is GIOP 1.2 and would
    /// write a 1.1 peer's `wstring` in the wrong form: 1.2 counts octets and
    /// 1.1 counts characters, so the same string is a different field.
    /// `tests/wide_follows_the_connection.rs` records that a stub answering
    /// from a constant could not be refuted by our own round trip, because both
    /// ends applied the same constant. A dynamic servant has the same exposure
    /// and avoids it the same way — by asking the request.
    ///
    /// # Why GIOP 1.0 falls back rather than failing
    ///
    /// There is no 1.0 wide form to ask for: `wchar` arrived in GIOP 1.1, so
    /// [`WideCodec::new`] refuses the pair, and an earlier version of this
    /// function turned that refusal into `MARSHAL` — for **every** operation on
    /// a 1.0 connection, including ones with no text in them at all, and
    /// including `_is_a`. The comparison in `tests/python_servant.rs` found it
    /// on its first run: nineteen calls diverging on 1.0 and none on 1.1 or
    /// 1.2, which is what a whole-batch comparison is for.
    ///
    /// The fallback is not a choice this module gets to make freshly. A
    /// generated Rust skeleton marshals through the *stream's* codec, and
    /// [`Request::narrow_codec`] builds that with `WideCodec::new(...).ok()` —
    /// so on 1.0 the stream has no wide codec and `Cdr` falls back to the form
    /// §9.3.1.6 fixes for an encapsulation, which is 1.2's. Answering anything
    /// else here would make a foreign servant and a Rust one disagree on a 1.0
    /// connection, which is precisely the leak this file exists to close. If
    /// that shared fallback is wrong it is wrong in both languages, and it is
    /// one question rather than two.
    fn wide(request: &Request) -> WideCodec {
        WideCodec::new(request.version, CodeSetId::UTF_16).unwrap_or_else(|_| {
            WideCodec::new(Version::V1_2, CodeSetId::UTF_16)
                .expect("1.2 with UTF-16 is always a valid pair")
        })
    }
}

impl<A: Answerer> Dispatch for ForeignServant<A> {
    fn dispatch_body(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> Result<DispatchBody, SystemException> {
        let wide = Self::wide(request);

        // Object-lifetime pseudo-operations are answered **here**, never in the
        // far language, and that is a transparency decision rather than an
        // optimisation. `_is_a` is what an ORB asks before it will narrow, and
        // its answer is a fact about the contract the registry resolved — not
        // about the servant's implementation. A foreign author who forgot to
        // implement it, or implemented it differently, would produce an object
        // that could not be narrowed through a base-typed reference, which is
        // precisely a caller being able to tell what language it is talking to.
        match request.operation.as_str() {
            "_is_a" => {
                let mut args = request.body().map_err(|_| SystemException::marshal())?;
                let asked = args.get_string().map_err(|_| SystemException::marshal())?;
                let answer = self.ancestry.contains(&asked);
                out.put_bool(answer);
                return Ok(DispatchBody::Return);
            }
            "_non_existent" => {
                out.put_bool(false);
                return Ok(DispatchBody::Return);
            }
            _ => {}
        }

        let Some(sig) = self.ops.get(&request.operation).cloned() else {
            return Err(SystemException::bad_operation());
        };

        // Arguments are decoded from CDR before anything crosses, so a body
        // this contract cannot read is `MARSHAL` from our side and the far
        // language is never told about a call that was never well-formed. The
        // decoder is positioned inside the whole message, so alignment is
        // measured from the GIOP header — the origin rule, honoured by not
        // copying.
        let mut args = request.body().map_err(|_| SystemException::marshal())?;
        let mut body: BTreeMap<String, Json> = BTreeMap::new();
        for p in &sig.params {
            if !matches!(p.direction, ParamDirection::In | ParamDirection::InOut) {
                continue;
            }
            let v = orbweaver_dynamic::decode_named_with(&mut args, &p.tc, &p.name, wide)
                .map_err(|_| SystemException::marshal())?;
            // An object reference among the arguments becomes a **handle** into
            // this process's table (§4.7: the seam cannot emit an IOR). The far
            // side may pass it back — as an argument to another call, or as
            // this call's own result — and cannot dial it.
            //
            // **This is the leak that is still open**, and it is the inbound
            // half rather than the outbound one: a Rust servant is handed a
            // reference it can *invoke*, and a foreign servant is not. Closing
            // it needs a call travelling the other way through `Answerer`,
            // which this protocol has no message for — named in the module
            // documentation and in `docs/COMPONENTS.md` rather than left to be
            // discovered.
            let j = anyjson::to_json(&p.tc, &v, &mut self.refs)
                .map_err(|_| SystemException::marshal())?;
            body.insert(p.name.clone(), j);
        }

        let mut call = BTreeMap::from([
            (CALL_INTERFACE.to_owned(), Json::String(self.id.clone())),
            (CALL_OPERATION.to_owned(), Json::String(request.operation.clone())),
            (CALL_OBJECT.to_owned(), Json::String(self.oid_for(request).to_owned())),
            (CALL_ARGUMENTS.to_owned(), Json::Object(body)),
        ]);
        if sig.oneway {
            // Told, not hidden. §9.4.1 gives a oneway no reply to travel in, so
            // a servant that raises has nowhere to put it — and an author who
            // cannot see that the call was oneway would write a raise believing
            // it reaches somebody.
            call.insert(CALL_ONEWAY.to_owned(), Json::Bool(true));
        }
        let call = Json::Object(call);

        // **The seam is re-entrant here and nowhere else.** While the far
        // side answers this call it may send `{"invoke": …}` naming a handle it
        // was given; `resolve_nested` dials on its behalf and the answer goes
        // back before the reply to this call arrives. The borrow is split
        // deliberately: the resolver needs the handle table while the answerer
        // holds the pipe, and both are fields of `self`.
        let Self { answerer, refs, registry, .. } = self;
        let reply = answerer
            .ask_resolving(&call, &mut |invoke| resolve_nested(refs, registry, invoke))
            .map_err(|_| Self::seam_failure())?;

        if sig.oneway {
            // §9.4.1: no reply may be written at all. An empty one is a whole
            // extra message which the peer, not waiting for it, would read as
            // the header of the next reply. The servant's verdict is dropped —
            // and, as in a generated Rust skeleton, dropped visibly.
            if let Some(fault) = oneway_fault(&reply) {
                crate::rt::oneway_fault_dropped(&self.id, &request.operation, &fault);
            }
            return Ok(DispatchBody::Return);
        }

        // Nothing is written into `out` before the label is known: the whole
        // buffer travels under one reply status, so a half-written result
        // followed by an exception body would be neither. The same rule the
        // generated skeleton's comment states, for the same reason.
        if let Some(Json::Object(s)) = reply.get(REPLY_SYSTEM_EXCEPTION) {
            return Err(system_exception_from(s).unwrap_or_else(Self::seam_failure));
        }
        if let Some(Json::Object(u)) = reply.get(REPLY_USER_EXCEPTION) {
            let Some(Json::String(id)) = u.get(EXCEPTION_ID) else {
                return Err(Self::seam_failure());
            };
            // A servant may only raise what its own operation declares. A
            // language with no type system stopping it gets the check a Rust
            // skeleton gets from its generated error enum made here — and an
            // undeclared raise is `UNKNOWN` with the OMG minor for exactly
            // that, which is the mapping §4.11 already fixes.
            if !sig.raises.iter().any(|r| r == id) {
                return Err(SystemException::unknown_user_exception());
            }
            let Some(tc) = self.registry.typecode(id) else {
                return Err(SystemException::unknown_user_exception());
            };
            let members =
                u.get(EXCEPTION_MEMBERS).cloned().unwrap_or(Json::Object(BTreeMap::new()));
            let v = anyjson::from_json(tc, &members, &self.refs)
                .map_err(|_| SystemException::marshal())?;
            out.put_str(id);
            orbweaver_dynamic::encode_with(out, tc, &v, wide)
                .map_err(|_| SystemException::marshal())?;
            return Ok(DispatchBody::UserException);
        }
        let Some(Json::Object(ok)) = reply.get(REPLY_OK) else {
            return Err(Self::seam_failure());
        };

        // The declared result first when it is not void, then the out and inout
        // values in declaration order (§7.9.1) — the same order a generated
        // client of any target reads a reply in, because it is the same rule.
        if !matches!(sig.returns, TypeCode::Void) {
            let j = ok.get(REPLY_RETURNS).cloned().unwrap_or(Json::Null);
            let v = anyjson::from_json(&sig.returns, &j, &self.refs)
                .map_err(|_| SystemException::marshal())?;
            orbweaver_dynamic::encode_named_with(out, &sig.returns, &v, "<return>", wide)
                .map_err(|_| SystemException::marshal())?;
        }
        let outputs = match ok.get(REPLY_OUTPUTS) {
            Some(Json::Object(o)) => o.clone(),
            _ => BTreeMap::new(),
        };
        for p in &sig.params {
            if !matches!(p.direction, ParamDirection::Out | ParamDirection::InOut) {
                continue;
            }
            // A missing out parameter is the servant's failure, not the
            // caller's, and `MARSHAL` is what a Rust skeleton would have been
            // unable to produce at all — the type system would not compile it.
            let Some(j) = outputs.get(&p.name) else {
                return Err(SystemException::marshal());
            };
            let v =
                anyjson::from_json(&p.tc, j, &self.refs).map_err(|_| SystemException::marshal())?;
            orbweaver_dynamic::encode_named_with(out, &p.tc, &v, &p.name, wide)
                .map_err(|_| SystemException::marshal())?;
        }
        Ok(DispatchBody::Return)
    }

    fn dispatch(&mut self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        match self.dispatch_body(request, out)? {
            DispatchBody::Return => Ok(()),
            // The narrow entry point cannot carry a user exception, so one
            // arriving here gets the standard mapping — the same answer the
            // generated Rust skeleton gives at the same seam.
            DispatchBody::UserException => Err(SystemException::unknown_user_exception()),
        }
    }

    /// Which keys this servant answers for.
    ///
    /// With a home, exactly the keys that home derives — the same rule a
    /// generated skeleton's `knows` applies through `<I>Refs::oid_of`. Without
    /// one, the `Dispatch` default: everything, which is right for a
    /// single-servant process and is what every deployment of this seam had
    /// before homes existed.
    fn knows(&self, object_key: &[u8]) -> bool {
        match &self.identity {
            Some(i) => i.oid_of(object_key).is_some(),
            None => true,
        }
    }
}

/// A `system_exception` document as the type the server replies with.
fn system_exception_from(s: &BTreeMap<String, Json>) -> Option<SystemException> {
    let Some(Json::String(id)) = s.get(EXCEPTION_ID) else { return None };
    let minor = s.get(EXCEPTION_MINOR).and_then(number_u32).unwrap_or(0);
    let completed = completion_of(s.get(EXCEPTION_COMPLETED).and_then(number_u32)?)?;
    Some(SystemException { id: id.clone(), minor, completed })
}

fn number_u32(j: &Json) -> Option<u32> {
    match j {
        Json::Number(n) => n.parse::<u32>().ok(),
        _ => None,
    }
}

/// What a oneway's answer said went wrong, when it said anything.
///
/// Rendered as a string rather than passed through as JSON because its only
/// consumer is [`crate::rt::oneway_fault_dropped`], which takes something
/// printable and exists so that a dropped fault is a decision somebody can see
/// rather than a silence.
fn oneway_fault(reply: &Json) -> Option<String> {
    for family in [REPLY_SYSTEM_EXCEPTION, REPLY_USER_EXCEPTION, REPLY_ERROR] {
        if let Some(v) = reply.get(family) {
            return Some(format!("{family}: {v}"));
        }
    }
    None
}
