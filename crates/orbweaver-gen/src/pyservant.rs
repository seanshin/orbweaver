//! The other protocol direction: a request our Rust ORB decoded, carried into
//! a Python servant and back.
//!
//! `orbweaver-gen` emitted Python **clients** for six phases and could not emit
//! a servant, and `COMPONENTS.md` recorded why in one phrase — *"a Python
//! servant needs the bridge to call back into Python, a second protocol
//! direction."* This module is that direction. It is the mirror of
//! [`crate::skeleton`]: where a generated Rust skeleton turns a [`Request`]
//! into a call on a trait, [`PyServant`] turns the same [`Request`] into one
//! JSON line and a reply line back.
//!
//! # Why this is the seam and not a Python ORB
//!
//! `docs/decisions/D030-*.md` §4 refuses a second ORB core, and the refusal has
//! a shape: **the bridge carries a dispatch, not a wire.** Everything below
//! happens on this side of the line — GIOP framing, CDR, alignment, byte order,
//! codeset negotiation, the reply status, the repository id on a user
//! exception. What crosses to Python is an operation name and a bag of already
//! decoded values. A binding that spoke GIOP would be a second ORB wearing a
//! binding's name, and it would owe a second set of alignment bugs.
//!
//! *브리지는 와이어가 아니라 디스패치를 나른다.*
//!
//! # Why the refusals cannot drift
//!
//! A generated Rust skeleton answers an unknown operation with
//! [`SystemException::bad_operation`], an undecodable body with
//! [`SystemException::marshal`] and an unknown key with
//! [`SystemException::object_not_exist`]. So does this module — by **calling
//! those same constructors**, not by reproducing their ids and completion
//! statuses. There is nothing here for a test to hold equal because there is
//! only one spelling, which is what this project means by making a cause
//! impossible rather than fixing it. The sentences that *do* have to be held
//! equal across the seam are the five wire-refusal families, and those live in
//! `orbweaver-dynamic` and are read from Python by
//! `tests/python_servant.rs`.
//!
//! # What a caller can still tell
//!
//! Not nothing, and this module's documentation is the honest place to say so
//! rather than the report that accompanied it. `docs/decisions/D029-*.md` §6.1
//! calls language transparency a leak *by construction* while Python is
//! clients only; closing that leaves a smaller one, and
//! [`PyServant::dispatch_body`]'s own comments name each remaining difference
//! at the point where it arises.

use std::collections::{BTreeMap, BTreeSet};

use orbweaver_cdr::Encoder;
use orbweaver_dynamic::anyjson::{self, LocalReferences};
use orbweaver_dynamic::json::Json;
use orbweaver_giop::codeset::{CodeSetId, WideCodec};
use orbweaver_giop::Version;
use orbweaver_giop::server::{Completion, Dispatch, DispatchBody, Request, SystemException};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_registry::{OperationSig, ParamDirection, Registry};

use crate::rt::{OBJECT_ID, UNKNOWN};

/// The repository id a servant answers when the thing that failed was the seam
/// itself, rather than anything the contract describes.
///
/// `UNKNOWN` and not `INTERNAL`: §4.11 gives `UNKNOWN` to an exception the
/// caller's contract cannot name, which is exactly what a dead bridge child or
/// an undeclared Python exception is from the caller's side. `INTERNAL` would
/// claim we know the servant's invariant broke, and for a seam failure we do
/// not know that.
pub const SEAM_FAILURE: &str = UNKNOWN;

/// What answers a call on the servant's behalf.
///
/// A trait rather than a process, for one reason that is worth the
/// indirection: it lets the whole of [`PyServant`] — argument decoding, the
/// AnyJSON conversion, the reply framing, every refusal — be **executed by a
/// test with no child process, no socket and no fixture**. That is the same
/// argument `_rt.Loopback` makes on the Python side, and it is why the seam's
/// behaviour is measurable on a machine that is too busy to start a peer.
pub trait Answerer {
    /// Puts one call to the servant and waits for its answer.
    ///
    /// `Err` is for the seam breaking — the child is gone, the line was not
    /// JSON — never for the servant refusing, which is a well-formed answer
    /// and comes back as `Ok` carrying `user_exception` or
    /// `system_exception`.
    fn ask(&mut self, call: &Json) -> Result<Json, String>;
}

/// A servant written in Python, dispatched into by our Rust ORB.
///
/// Implements [`Dispatch`], so it goes wherever a generated Rust skeleton goes
/// and the server cannot tell them apart. The `&mut self` shape is deliberate
/// and matches [`crate::skeleton`]'s: `Server::serve` wraps a `Dispatch` in a
/// mutex for the duration of one message, which is also exactly the discipline
/// a single-threaded child process on one pair of pipes needs. A Python
/// servant is therefore *no more* serialized than a generated Rust one — a
/// difference this seam was expected to have and does not.
pub struct PyServant<A: Answerer> {
    id: String,
    /// The resolved callable surface: every operation and attribute accessor
    /// this interface answers to, inherited ones included.
    ///
    /// [`crate::python::client_operations`] computes it, and this is its third
    /// consumer. That reuse is the property, not a convenience: a Python
    /// servant answers exactly the names a Python client of the same contract
    /// can send, because one function decides both.
    ops: BTreeMap<String, OperationSig>,
    /// `_is_a`'s answer set: this interface, its resolved ancestors, and
    /// `CORBA::Object`. The same set [`crate::skeleton`] bakes into a generated
    /// skeleton, computed the same way.
    ancestry: BTreeSet<String>,
    registry: Registry,
    answerer: A,
    handles: LocalReferences,
}

impl<A: Answerer> PyServant<A> {
    /// A servant for `id`, answering through `answerer`.
    ///
    /// Fails when `id` names no interface in `registry`, which is the one
    /// mistake worth refusing at construction: a servant that dispatches
    /// nothing would otherwise answer every call `BAD_OPERATION` and look like
    /// a contract mismatch.
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
            ops: crate::python::client_operations(registry, id),
            ancestry,
            registry: registry.clone(),
            answerer,
            handles: LocalReferences::new(),
        })
    }

    /// The interface this servant answers for.
    pub fn type_id(&self) -> &str {
        &self.id
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
        SystemException {
            id: SEAM_FAILURE.to_owned(),
            minor: 0,
            completed: Completion::Maybe,
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
    /// else here would make a Python servant and a Rust one disagree on a 1.0
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

impl<A: Answerer> Dispatch for PyServant<A> {
    fn dispatch_body(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> Result<DispatchBody, SystemException> {
        let wide = Self::wide(request);

        // Object-lifetime pseudo-operations are answered **here**, never in
        // Python, and that is a transparency decision rather than an
        // optimisation. `_is_a` is what an ORB asks before it will narrow, and
        // its answer is a fact about the contract the registry resolved — not
        // about the servant's implementation. A Python author who forgot to
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
        // this contract cannot read is `MARSHAL` from our side and Python is
        // never told about a call that was never well-formed. The decoder is
        // positioned inside the whole message, so alignment is measured from
        // the GIOP header — the origin rule, honoured by not copying.
        let mut args = request.body().map_err(|_| SystemException::marshal())?;
        let mut body: BTreeMap<String, Json> = BTreeMap::new();
        for p in &sig.params {
            if !matches!(p.direction, ParamDirection::In | ParamDirection::InOut) {
                continue;
            }
            let v = orbweaver_dynamic::decode_named_with(&mut args, &p.tc, &p.name, wide)
                .map_err(|_| SystemException::marshal())?;
            // An object reference among the arguments becomes a **handle** into
            // this process's table (D007's one documented asymmetry: §4.5
            // cannot emit an IOR). A Rust servant is handed a reference it can
            // invoke; a Python servant is handed an opaque token it cannot.
            // That is a real remaining leak and it is recorded rather than
            // papered over — see the report accompanying this module.
            let j = anyjson::to_json(&p.tc, &v, &mut self.handles)
                .map_err(|_| SystemException::marshal())?;
            body.insert(p.name.clone(), j);
        }

        let mut call = BTreeMap::from([
            ("id".to_owned(), Json::String(self.id.clone())),
            ("op".to_owned(), Json::String(request.operation.clone())),
            ("args".to_owned(), Json::Object(body)),
        ]);
        if sig.oneway {
            // Told, not hidden. §9.4.1 gives a oneway no reply to travel in, so
            // a servant that raises has nowhere to put it — and a Python author
            // who cannot see that the call was oneway would write a `raise`
            // believing it reaches somebody.
            call.insert("oneway".to_owned(), Json::Bool(true));
        }
        let call = Json::Object(call);

        let reply = self.answerer.ask(&call).map_err(|_| Self::seam_failure())?;

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
        if let Some(Json::Object(s)) = reply.get("system_exception") {
            return Err(system_exception_from(s).unwrap_or_else(Self::seam_failure));
        }
        if let Some(Json::Object(u)) = reply.get("user_exception") {
            let Some(Json::String(id)) = u.get("id") else {
                return Err(Self::seam_failure());
            };
            // A servant may only raise what its own operation declares. Python
            // has no type system stopping it, so the check that a Rust
            // skeleton gets from its generated error enum is made here — and
            // an undeclared raise is `UNKNOWN` with the OMG minor for exactly
            // that, which is the mapping §4.11 already fixes.
            if !sig.raises.iter().any(|r| r == id) {
                return Err(SystemException::unknown_user_exception());
            }
            let Some(tc) = self.registry.typecode(id) else {
                return Err(SystemException::unknown_user_exception());
            };
            let members = u.get("members").cloned().unwrap_or(Json::Object(BTreeMap::new()));
            let v = anyjson::from_json(tc, &members, &self.handles)
                .map_err(|_| SystemException::marshal())?;
            out.put_str(id);
            orbweaver_dynamic::encode_with(out, tc, &v, wide)
                .map_err(|_| SystemException::marshal())?;
            return Ok(DispatchBody::UserException);
        }
        let Some(Json::Object(ok)) = reply.get("ok") else {
            return Err(Self::seam_failure());
        };

        // The declared result first when it is not void, then the out and inout
        // values in declaration order (§7.9.1) — the same order the Python
        // client's `_rt.call` reads a reply in, because it is the same rule.
        if !matches!(sig.returns, TypeCode::Void) {
            let j = ok.get("returns").cloned().unwrap_or(Json::Null);
            let v = anyjson::from_json(&sig.returns, &j, &self.handles)
                .map_err(|_| SystemException::marshal())?;
            orbweaver_dynamic::encode_named_with(out, &sig.returns, &v, "<return>", wide)
                .map_err(|_| SystemException::marshal())?;
        }
        let outputs = match ok.get("outputs") {
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
            let v = anyjson::from_json(&p.tc, j, &self.handles)
                .map_err(|_| SystemException::marshal())?;
            orbweaver_dynamic::encode_named_with(out, &p.tc, &v, &p.name, wide)
                .map_err(|_| SystemException::marshal())?;
        }
        Ok(DispatchBody::Return)
    }

    fn dispatch(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> Result<(), SystemException> {
        match self.dispatch_body(request, out)? {
            DispatchBody::Return => Ok(()),
            // The narrow entry point cannot carry a user exception, so one
            // arriving here gets the standard mapping — the same answer the
            // generated Rust skeleton gives at the same seam.
            DispatchBody::UserException => Err(SystemException::unknown_user_exception()),
        }
    }
}

/// A `system_exception` document as the type the server replies with.
///
/// The completion status crosses as **the ordinal**, unnamed. `orbweaver-gen`'s
/// runtime has a comment longer than the enum about why: §4.11.4 numbers
/// `COMPLETED_YES` 0, `COMPLETED_NO` 1, `COMPLETED_MAYBE` 2, this project has
/// transposed the first two once already, and naming them in a second language
/// would be a second place to get the same numbering wrong.
fn system_exception_from(s: &BTreeMap<String, Json>) -> Option<SystemException> {
    let Some(Json::String(id)) = s.get("id") else { return None };
    let minor = s.get("minor").and_then(number_u32).unwrap_or(0);
    let completed = match s.get("completed").and_then(number_u32) {
        Some(0) => Completion::Yes,
        Some(1) => Completion::No,
        Some(2) => Completion::Maybe,
        // Absent or unrecognised is refused rather than defaulted. A Rust
        // servant cannot reach a `SystemException` without naming the status —
        // `rt::Raising` has no `Default` and no `From` for exactly this reason —
        // and a Python servant that omits it must not be quietly given one.
        _ => return None,
    };
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
    for family in ["system_exception", "user_exception", "error"] {
        if let Some(v) = reply.get(family) {
            return Some(format!("{family}: {v}"));
        }
    }
    None
}
