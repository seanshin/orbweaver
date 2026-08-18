//! The measurement: can a generated skeleton express `naming_server.rs`?
//!
//! `docs/COMPONENTS.md` recorded the gap as "no `knows()`/object keys, so one
//! servant per process and `naming_server`'s multi-context shape is not yet
//! generatable; no `LOCATION_FORWARD`". Three of those four clauses were
//! already false when this file was written — object keys, `knows()` and the
//! forward seam landed in `gen: give generated skeletons object keys, minting
//! and a forward seam`, and `tests/object_identity.rs` drives a
//! `LOCATION_FORWARD` over real GIOP. The clause that was still unmeasured is
//! the one this file closes: the *naming* shape specifically.
//!
//! It is a different shape from the IFR facade, which is what
//! `tests/ifr_shape.rs` measured. The facade **derives** its objects from a
//! registry and mints none; a naming server **mints** a context per
//! `bind_new_context`, must answer for every key it has ever handed out, and
//! its operations reach *other* objects — resolving a path walks contexts,
//! binding one writes into a sibling. A generated skeleton that can serve a
//! derived population says nothing about one that has to serve a growing one.
//!
//! # The arrangement
//!
//! `corpus/services/gen-naming-subset.idl` is the contract. One generated
//! `NamingContextExtSkeleton` serves the whole tree, exactly as one
//! `NamingServer` does: the root context under the bare root key, and every
//! minted context under `root ++ "/_ctx" ++ <n>`. That is `NamingServer`'s own
//! key derivation, adopted here with `with_infix("/_ctx")` so the two mint
//! byte-identical references and their replies can be compared as bytes.
//!
//! The servant **body** below is hand-written — that is the claim, bodies are
//! hand-written — and what is not written is any of the dispatch: no
//! `impl Dispatch`, no `match` on an operation name, no key parsing, no
//! `_is_a` id list, no `Ior` assembled from an `IiopProfile`, no exception
//! body written by hand.
//!
//! # Why the comparison is a script and not a set
//!
//! `ifr.rs` is read-only, so its cases are independent and could be a set. A
//! naming server is not: `resolve` answers differently after `bind`, and the
//! minted key of the third context depends on how many mints succeeded before
//! it. So [`script`] is an **ordered** sequence driven into both servants in
//! lockstep, and every step's reply is compared before the next one runs. A
//! divergence therefore shows up at the first step that caused it rather than
//! at some later step that inherited it.
//!
//! # Byte equality is not the whole check
//!
//! Two servants can agree on the wrong bytes. Every structured reply the
//! script produces is also decoded back with `orbweaver-giop`'s **own**
//! readers — `naming::read_name`, `Ior::read_from` — and asserted field by
//! field in [`the_generated_replies_decode_as_the_oracles_own_readers`].

mod emitted;

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_gen::rt::{self, Dispatch, ObjRef, ObjectHome};
use orbweaver_giop::naming::{
    self, NAMING_CONTEXT_EXT_ID, UrlError, parse_stringified_name, stringify_name, write_name,
};
use orbweaver_giop::naming_server::{
    self, ALREADY_BOUND_ID, INVALID_ADDRESS_ID, INVALID_NAME_ID, NAMING_CONTEXT_ID, NOT_FOUND_ID,
    NamingServer,
};
use orbweaver_giop::server::{Request, decode_request};
use orbweaver_giop::{
    DEFAULT_MAX_MESSAGE_SIZE, IiopProfile, Ior, Version, encode_request, read_message,
};

use emitted::f_gen_naming_subset::CosNaming::NamingContext::{
    AlreadyBound, InvalidName, NotFound, NotFoundReason,
};
use emitted::f_gen_naming_subset::CosNaming::NamingContextExt::InvalidAddress;
use emitted::f_gen_naming_subset::CosNaming::{
    Binding, BindingList, BindingType, Name, NameComponent, NamingContextExtFault,
    NamingContextExtRefs, NamingContextExtServant, NamingContextExtSkeleton,
    NamingContextExtTarget,
};

/// The key `NamingServer::new` is rooted at, and what `rt::Server` would be
/// bound with. `b"NameService"` is what every CosNaming client asks for.
const ROOT: &[u8] = b"NameService";
const HOST: &str = "127.0.0.1";
const PORT: u16 = 4242;

/// `NamingServer::mint_context` builds `root ++ "/_ctx" ++ <n>`. Adopting the
/// same infix is what makes a minted reference comparable as bytes: the whole
/// object key inside the IIOP profile has to match, not merely the shape.
const INFIX: &str = "/_ctx";

/// Both byte orders, always. An encoder that only works native-endian passes
/// every local test and fails in the field.
const ORDERS: [Endian; 2] = [Endian::Big, Endian::Little];

// ── The hand-written half: a servant body, and no dispatch ───────────────────

/// What a name is bound to.
#[derive(Debug, Clone)]
enum Bound {
    /// A reference, held verbatim — naming stores references, never dials one.
    Object(ObjRef),
    /// A context served by this same servant, held by its oid.
    Context(String),
}

/// One context's bindings, keyed by `(id, kind)`. A `BTreeMap` so `list` order
/// is deterministic — and so it is the *same* order the oracle produces.
type Bindings = BTreeMap<(String, String), Bound>;

fn slot(c: &NameComponent) -> (String, String) {
    (c.id.clone(), c.kind.clone())
}

fn not_found(why: NotFoundReason, rest_of_name: Name) -> NamingContextExtFault {
    NamingContextExtFault::NotFound(NotFound { why, rest_of_name })
}

fn invalid_name() -> NamingContextExtFault {
    NamingContextExtFault::InvalidName(InvalidName {})
}

fn already_bound() -> NamingContextExtFault {
    NamingContextExtFault::AlreadyBound(AlreadyBound {})
}

/// The reference a resolved binding hands back.
///
/// A context is named by its oid, so its reference comes from `Target::sibling`
/// — one call, and the servant has a publishable reference to another object of
/// its own interface without ever seeing a host, a port or a profile.
fn reference_of(at: &NamingContextExtTarget<'_>, bound: Bound) -> ObjRef {
    match bound {
        Bound::Object(r) => r,
        Bound::Context(oid) => at.sibling(&oid),
    }
}

/// The whole context tree, keyed by oid rather than by object key: the oid is
/// what the skeleton hands over, so this servant never sees a key at all.
#[derive(Debug, Default)]
struct Tree {
    contexts: BTreeMap<String, Bindings>,
    minted: u64,
}

impl Tree {
    /// A tree whose root context is the default object — the empty oid, which
    /// is the bare root key the server was bound with.
    fn new() -> Self {
        let mut t = Self::default();
        t.contexts.insert(String::new(), Bindings::new());
        t
    }

    /// Contexts are never destroyed, so a missing oid means the request
    /// addressed an object this server never minted.
    fn table(&self, oid: &str) -> Result<&Bindings, NamingContextExtFault> {
        self.contexts.get(oid).ok_or_else(|| rt::raise::object_not_exist().did_not_run().into())
    }

    fn table_mut(&mut self, oid: &str) -> Result<&mut Bindings, NamingContextExtFault> {
        self.contexts.get_mut(oid).ok_or_else(|| rt::raise::object_not_exist().did_not_run().into())
    }

    /// A fresh context. The counter advances only on a mint that happens, so
    /// a refused `bind_new_context` must not reach here — that ordering is
    /// what decides the *next* context's oid, and therefore its key.
    fn mint(&mut self) -> String {
        self.minted += 1;
        let oid = self.minted.to_string();
        self.contexts.insert(oid.clone(), Bindings::new());
        oid
    }

    /// Walks every component but the last, context to context.
    ///
    /// The failure carries `rest_of_name` starting at the component that
    /// failed, which is what omniNames reports and what the oracle writes.
    fn walk(
        &self,
        start: &str,
        name: &Name,
    ) -> Result<(String, NameComponent), NamingContextExtFault> {
        let Some((last, path)) = name.split_last() else {
            return Err(invalid_name());
        };
        let mut ctx = start.to_owned();
        for (i, c) in path.iter().enumerate() {
            match self.table(&ctx)?.get(&slot(c)) {
                None => return Err(not_found(NotFoundReason::missing_node, name[i..].to_vec())),
                Some(Bound::Object(_)) => {
                    return Err(not_found(NotFoundReason::not_context, name[i..].to_vec()));
                }
                Some(Bound::Context(k)) => ctx = k.clone(),
            }
        }
        Ok((ctx, last.clone()))
    }

    fn resolve_from(&self, start: &str, name: &Name) -> Result<Bound, NamingContextExtFault> {
        let (ctx, last) = self.walk(start, name)?;
        match self.table(&ctx)?.get(&slot(&last)) {
            None => Err(not_found(NotFoundReason::missing_node, vec![last])),
            Some(bound) => Ok(bound.clone()),
        }
    }

    fn bind_from(
        &mut self,
        start: &str,
        name: &Name,
        to: Bound,
        overwrite: bool,
    ) -> Result<(), NamingContextExtFault> {
        let (ctx, last) = self.walk(start, name)?;
        match self.table_mut(&ctx)?.entry(slot(&last)) {
            Entry::Vacant(v) => {
                v.insert(to);
                Ok(())
            }
            Entry::Occupied(mut o) => {
                if !overwrite {
                    return Err(already_bound());
                }
                if matches!(o.get(), Bound::Context(_)) {
                    // rebind replaces objects only; replacing a context is
                    // rebind_context's job, which is not served.
                    return Err(not_found(NotFoundReason::not_object, vec![last]));
                }
                o.insert(to);
                Ok(())
            }
        }
    }
}

impl NamingContextExtServant for Tree {
    /// One servant answers for the whole tree: the root context and every key
    /// `bind_new_context`/`new_context` minted.
    ///
    /// This is the method the gap statement was about. A default of `true`
    /// would claim `root ++ "/_ctx99"` — a context nobody ever minted — and
    /// answer `resolve` on it as if it were the root.
    fn knows(&self, at: &NamingContextExtTarget<'_>) -> bool {
        self.contexts.contains_key(at.oid())
    }

    fn resolve(
        &mut self,
        at: &NamingContextExtTarget<'_>,
        n: Name,
    ) -> Result<ObjRef, NamingContextExtFault> {
        let bound = self.resolve_from(at.oid(), &n)?;
        Ok(reference_of(at, bound))
    }

    fn resolve_str(
        &mut self,
        at: &NamingContextExtTarget<'_>,
        sn: String,
    ) -> Result<ObjRef, NamingContextExtFault> {
        let name = parse_name(&sn)?;
        let bound = self.resolve_from(at.oid(), &name)?;
        Ok(reference_of(at, bound))
    }

    fn bind(
        &mut self,
        at: &NamingContextExtTarget<'_>,
        n: Name,
        obj: ObjRef,
    ) -> Result<(), NamingContextExtFault> {
        self.bind_from(at.oid(), &n, Bound::Object(obj), false)
    }

    fn rebind(
        &mut self,
        at: &NamingContextExtTarget<'_>,
        n: Name,
        obj: ObjRef,
    ) -> Result<(), NamingContextExtFault> {
        self.bind_from(at.oid(), &n, Bound::Object(obj), true)
    }

    fn unbind(
        &mut self,
        at: &NamingContextExtTarget<'_>,
        n: Name,
    ) -> Result<(), NamingContextExtFault> {
        let (ctx, last) = self.walk(at.oid(), &n)?;
        match self.table_mut(&ctx)?.remove(&slot(&last)) {
            // An unbound context stays reachable by key; destroy is not served.
            Some(_) => Ok(()),
            None => Err(not_found(NotFoundReason::missing_node, vec![last])),
        }
    }

    fn new_context(
        &mut self,
        at: &NamingContextExtTarget<'_>,
    ) -> Result<ObjRef, NamingContextExtFault> {
        let oid = self.mint();
        Ok(at.sibling(&oid))
    }

    fn bind_new_context(
        &mut self,
        at: &NamingContextExtTarget<'_>,
        n: Name,
    ) -> Result<ObjRef, NamingContextExtFault> {
        // Occupancy is checked before minting, or a failed bind leaks an
        // unreachable context — and shifts every later context's oid by one,
        // which the byte comparison would then report at the wrong step.
        let (ctx, last) = self.walk(at.oid(), &n)?;
        if self.table(&ctx)?.contains_key(&slot(&last)) {
            return Err(already_bound());
        }
        let oid = self.mint();
        self.table_mut(&ctx)?.insert(slot(&last), Bound::Context(oid.clone()));
        Ok(at.sibling(&oid))
    }

    fn list(
        &mut self,
        at: &NamingContextExtTarget<'_>,
        how_many: u32,
    ) -> Result<(BindingList, ObjRef), NamingContextExtFault> {
        let table = self.table(at.oid())?;
        let take = (how_many as usize).min(table.len());
        let bindings = table
            .iter()
            .take(take)
            .map(|((id, kind), bound)| Binding {
                binding_name: vec![NameComponent { id: id.clone(), kind: kind.clone() }],
                binding_type: match bound {
                    Bound::Object(_) => BindingType::nobject,
                    Bound::Context(_) => BindingType::ncontext,
                },
            })
            .collect();
        // Always a nil iterator, even when `take` truncated — the oracle's
        // stub, reproduced because this is a comparison and not a rewrite.
        Ok((bindings, ObjRef(None)))
    }

    fn to_name(
        &mut self,
        _at: &NamingContextExtTarget<'_>,
        sn: String,
    ) -> Result<Name, NamingContextExtFault> {
        parse_name(&sn)
    }

    fn to_string(
        &mut self,
        _at: &NamingContextExtTarget<'_>,
        n: Name,
    ) -> Result<String, NamingContextExtFault> {
        if n.is_empty() {
            return Err(invalid_name());
        }
        Ok(stringify_name(&n.iter().map(to_giop).collect::<Vec<_>>()))
    }

    fn to_url(
        &mut self,
        _at: &NamingContextExtTarget<'_>,
        addr: String,
        sn: String,
    ) -> Result<String, NamingContextExtFault> {
        naming::to_url(&addr, &sn).map_err(|e| match e {
            UrlError::BadAddress(_) => NamingContextExtFault::InvalidAddress(InvalidAddress {}),
            UrlError::BadSchemeName(_) | UrlError::BadSchemeSpecificPart(_) => invalid_name(),
            // Our own two halves disagreed: the servant's defect, not a
            // statement about either argument.
            UrlError::Other(_) => rt::raise::internal().did_not_run().into(),
        })
    }

    // ── Declared so that the servant can refuse them by name ────────────────
    //
    // `NO_IMPLEMENT` says *the contract has this and this servant does not
    // implement it*; `BAD_OPERATION` says *no such operation*, which invites a
    // retry against another reference. Only an operation the contract declares
    // can carry the first answer — which is why `gen-naming-subset.idl`
    // declares all three, and why `ir-subset.idl`'s not declaring the IFR's
    // deferrals is a `NOT_COMPARED` entry over there and is not one here.

    fn bind_context(
        &mut self,
        _at: &NamingContextExtTarget<'_>,
        _n: Name,
        _nc: ObjRef,
    ) -> Result<(), NamingContextExtFault> {
        Err(rt::raise::no_implement().did_not_run().into())
    }

    fn rebind_context(
        &mut self,
        _at: &NamingContextExtTarget<'_>,
        _n: Name,
        _nc: ObjRef,
    ) -> Result<(), NamingContextExtFault> {
        Err(rt::raise::no_implement().did_not_run().into())
    }

    fn destroy(&mut self, _at: &NamingContextExtTarget<'_>) -> Result<(), NamingContextExtFault> {
        Err(rt::raise::no_implement().did_not_run().into())
    }
}

/// A stringified name, parsed the way the oracle parses it: through
/// `orbweaver-giop`'s own parser, so the two cannot drift.
fn parse_name(s: &str) -> Result<Name, NamingContextExtFault> {
    let name = parse_stringified_name(s).map_err(|_| invalid_name())?;
    if name.is_empty() {
        return Err(invalid_name());
    }
    Ok(name.iter().map(from_giop).collect())
}

fn to_giop(c: &NameComponent) -> naming::NameComponent {
    naming::NameComponent { id: c.id.clone(), kind: c.kind.clone() }
}

fn from_giop(c: &naming::NameComponent) -> NameComponent {
    NameComponent { id: c.id.clone(), kind: c.kind.clone() }
}

// ── The two servants, and how a request reaches them ─────────────────────────

fn home() -> ObjectHome {
    ObjectHome::new(HOST, PORT, ROOT.to_vec())
}

fn refs() -> NamingContextExtRefs {
    NamingContextExtRefs::with_infix(home(), INFIX)
}

/// The generated half: one skeleton, one servant, the whole tree.
fn generated() -> NamingContextExtSkeleton<Tree> {
    NamingContextExtSkeleton::new(refs(), Tree::new())
}

/// The oracle: `naming_server.rs`, unmodified.
fn hand_written() -> NamingServer {
    NamingServer::new(HOST, PORT, ROOT.to_vec())
}

/// The object key of the context with this oid; the empty oid is the root.
fn ctx_key(oid: &str) -> Vec<u8> {
    refs().key_of(oid)
}

/// A reference of the kind a client binds: something this server did not mint,
/// held verbatim and handed back unchanged.
fn foreign(tag: &str) -> Ior {
    Ior {
        type_id: format!("IDL:spike/{tag}:1.0"),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "192.0.2.1".into(),
            port: 4000,
            object_key: tag.as_bytes().to_vec(),
            components: Vec::new(),
        }],
    }
}

fn nc(id: &str) -> naming::NameComponent {
    naming::NameComponent::new(id)
}

/// What a servant answered: bytes under a status, a system exception, or
/// "not my object".
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Body(rt::DispatchBody, Vec<u8>),
    Raised { id: String, minor: u32, completed: rt::Completion },
    Unknown,
}

type Args = Box<dyn Fn(&mut Encoder)>;

/// One step of the script: which object, which operation, which arguments.
struct Step {
    what: &'static str,
    key: Vec<u8>,
    op: &'static str,
    args: Args,
}

fn step(what: &'static str, key: Vec<u8>, op: &'static str, args: Args) -> Step {
    Step { what, key, op, args }
}

/// No arguments at all.
fn none() -> Args {
    Box::new(|_| {})
}

fn name_args(ids: &'static [&'static str]) -> Args {
    Box::new(move |e| write_name(e, &ids.iter().map(|i| nc(i)).collect::<Vec<_>>()))
}

fn name_and_ref(ids: &'static [&'static str], tag: &'static str) -> Args {
    Box::new(move |e| {
        write_name(e, &ids.iter().map(|i| nc(i)).collect::<Vec<_>>());
        foreign(tag).write_to(e).expect("a reference marshals");
    })
}

fn string_args(ss: &'static [&'static str]) -> Args {
    Box::new(move |e| {
        for s in ss {
            e.put_str(s);
        }
    })
}

fn u32_arg(v: u32) -> Args {
    Box::new(move |e| e.put_u32(v))
}

fn request(endian: Endian, key: &[u8], operation: &str, args: &Args) -> Request {
    let wire = encode_request(Version::V1_2, endian, 1, key, operation, true, |e| args(e))
        .expect("encode request");
    let mut cursor: &[u8] = &wire;
    let msg = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("frame");
    decode_request(msg).expect("decode request")
}

/// Drives one servant exactly as `rt::Server` would: `knows` first, then a body
/// written into an encoder positioned where a real reply body starts —
/// alignment is measured from the GIOP header, so the origin is not zero.
fn ask<D: Dispatch>(d: &mut D, endian: Endian, key: &[u8], op: &str, args: &Args) -> Answer {
    if !d.knows(key) {
        return Answer::Unknown;
    }
    let req = request(endian, key, op, args);
    let mut out = Encoder::continuing_at(endian, 24);
    match d.dispatch_body(&req, &mut out) {
        Ok(kind) => Answer::Body(kind, out.finish().expect("finish")),
        Err(ex) => Answer::Raised { id: ex.id, minor: ex.minor, completed: ex.completed },
    }
}

// ── The script ───────────────────────────────────────────────────────────────

/// Every step both servants must answer identically, in order.
///
/// Ordered because the subject is stateful: `resolve` answers differently after
/// `bind`, and the oid a mint hands out depends on how many mints succeeded
/// before it. The mutations are interleaved with the reads that observe them,
/// so a servant that applied a bind to the wrong context would diverge at the
/// next read rather than at the bind.
fn script() -> Vec<Step> {
    let root = ROOT.to_vec();
    let mut v = Vec::new();

    // ── Probes, on an empty root context ──
    for id in [
        NAMING_CONTEXT_EXT_ID,
        NAMING_CONTEXT_ID,
        rt::OBJECT_ID,
        "IDL:omg.org/CosNaming/BindingIterator:1.0",
    ] {
        // `_is_a` args are a single string, and the id has to outlive the
        // closure, so it is written directly rather than through `string_args`.
        let id = id.to_owned();
        v.push(step(
            "_is_a on the root context",
            root.clone(),
            "_is_a",
            Box::new(move |e| e.put_str(&id)),
        ));
    }
    v.push(step("_non_existent", root.clone(), "_non_existent", none()));
    v.push(step("an operation neither contract has", root.clone(), "frobnicate", none()));

    // Deferred operations, with **well-formed** bodies. See `NOT_COMPARED`:
    // the oracle refuses before it reads the body and a generated skeleton
    // reads it first, so a malformed body under one of these is the one
    // ordering difference between the two.
    v.push(step("deferred bind_context", root.clone(), "bind_context", name_and_ref(&["c"], "C")));
    v.push(step(
        "deferred rebind_context",
        root.clone(),
        "rebind_context",
        name_and_ref(&["c"], "C"),
    ));
    v.push(step("deferred destroy", root.clone(), "destroy", none()));

    // ── The empty tree ──
    v.push(step("list an empty context", root.clone(), "list", u32_arg(10)));
    v.push(step("resolve a name nothing is bound to", root.clone(), "resolve", name_args(&["a"])));
    v.push(step("resolve the empty name", root.clone(), "resolve", name_args(&[])));
    v.push(step("unbind a name nothing is bound to", root.clone(), "unbind", name_args(&["a"])));

    // The string surface, which answers from `crate::naming` alone.
    v.push(step(
        "to_string of a two-component name",
        root.clone(),
        "to_string",
        name_args(&["a", "b"]),
    ));
    v.push(step("to_string of the empty name", root.clone(), "to_string", name_args(&[])));
    v.push(step("to_name of a stringified path", root.clone(), "to_name", string_args(&["a/b"])));
    v.push(step("to_name of the empty string", root.clone(), "to_name", string_args(&[""])));
    v.push(step(
        "to_url over a well-formed address",
        root.clone(),
        "to_url",
        string_args(&["192.0.2.9:2809", "a/b"]),
    ));
    v.push(step(
        "to_url over an address that does not parse",
        root.clone(),
        "to_url",
        string_args(&["not::an::address", "a"]),
    ));
    v.push(step(
        "to_url over a name that does not parse",
        root.clone(),
        "to_url",
        string_args(&["192.0.2.9:2809", ""]),
    ));
    v.push(step("resolve_str a missing path", root.clone(), "resolve_str", string_args(&["a"])));
    v.push(step("resolve_str the empty path", root.clone(), "resolve_str", string_args(&[""])));

    // ── Mutations, and the reads that observe them ──
    v.push(step("bind a reference", root.clone(), "bind", name_and_ref(&["a"], "A")));
    v.push(step("bind the same name again", root.clone(), "bind", name_and_ref(&["a"], "B")));
    v.push(step("resolve the bound name", root.clone(), "resolve", name_args(&["a"])));
    v.push(step("rebind over an object", root.clone(), "rebind", name_and_ref(&["a"], "B")));
    v.push(step("resolve after the rebind", root.clone(), "resolve", name_args(&["a"])));

    // The mint. `ctx` becomes oid "1", so its key is ROOT ++ "/_ctx1".
    v.push(step(
        "bind_new_context mints oid 1",
        root.clone(),
        "bind_new_context",
        name_args(&["ctx"]),
    ));
    v.push(step("resolve the minted context", root.clone(), "resolve", name_args(&["ctx"])));
    v.push(step("list both bindings", root.clone(), "list", u32_arg(10)));
    v.push(step("list truncated to one", root.clone(), "list", u32_arg(1)));
    v.push(step("list nothing", root.clone(), "list", u32_arg(0)));

    // Two-component paths: the walk, which is what a context tree is for.
    v.push(step(
        "bind through a context",
        root.clone(),
        "bind",
        name_and_ref(&["ctx", "deep"], "D"),
    ));
    v.push(step("resolve through a context", root.clone(), "resolve", name_args(&["ctx", "deep"])));
    v.push(step(
        "resolve_str through a context",
        root.clone(),
        "resolve_str",
        string_args(&["ctx/deep"]),
    ));
    v.push(step("walk through an object", root.clone(), "resolve", name_args(&["a", "deep"])));
    v.push(step("walk through nothing", root.clone(), "resolve", name_args(&["nope", "deep"])));
    v.push(step("rebind over a context", root.clone(), "rebind", name_and_ref(&["ctx"], "A")));
    v.push(step(
        "bind_new_context over an occupied name mints nothing",
        root.clone(),
        "bind_new_context",
        name_args(&["ctx"]),
    ));
    // If the refused mint above had advanced the counter, this would be oid 3
    // on one side and oid 2 on the other, and the key inside the reply would
    // differ. That is the whole reason the refusal is checked before the mint.
    v.push(step("new_context mints oid 2, bound to nothing", root.clone(), "new_context", none()));

    // ── Requests addressed to a minted context, not to the root ──
    let ctx1 = ctx_key("1");
    v.push(step("_is_a on a minted context", ctx1.clone(), "_is_a", {
        let id = NAMING_CONTEXT_EXT_ID.to_owned();
        Box::new(move |e| e.put_str(&id))
    }));
    v.push(step("list inside a minted context", ctx1.clone(), "list", u32_arg(10)));
    v.push(step("resolve inside a minted context", ctx1.clone(), "resolve", name_args(&["deep"])));
    v.push(step(
        "bind inside a minted context",
        ctx1.clone(),
        "bind",
        name_and_ref(&["leaf"], "L"),
    ));
    v.push(step(
        "bind_new_context inside a minted context mints oid 3",
        ctx1.clone(),
        "bind_new_context",
        name_args(&["sub"]),
    ));
    v.push(step("list the minted context again", ctx1.clone(), "list", u32_arg(10)));
    v.push(step("unbind inside a minted context", ctx1.clone(), "unbind", name_args(&["deep"])));
    v.push(step("resolve what was just unbound", ctx1.clone(), "resolve", name_args(&["deep"])));
    // A grandchild: oid 3 exists only because the mint above succeeded.
    let ctx3 = ctx_key("3");
    v.push(step("list a grandchild context", ctx3.clone(), "list", u32_arg(10)));
    v.push(step("bind in a grandchild context", ctx3, "bind", name_and_ref(&["g"], "G")));

    // ── Back at the root ──
    v.push(step("unbind at the root", root.clone(), "unbind", name_args(&["a"])));
    v.push(step("resolve what was unbound", root.clone(), "resolve", name_args(&["a"])));
    v.push(step("unbind it twice", root.clone(), "unbind", name_args(&["a"])));
    v.push(step("list the root once more", root.clone(), "list", u32_arg(10)));

    // ── Malformed bodies, on operations the contract declares ──
    // A `Name` whose count says five and whose body carries none. Both
    // servants must answer MARSHAL, which is the check that the generated
    // decoder refuses the same inputs rather than merely the same shapes.
    v.push(step("resolve with a truncated name", root.clone(), "resolve", u32_arg(5)));
    v.push(step("bind with no reference after the name", root.clone(), "bind", name_args(&["a"])));

    // ── Keys neither servant minted ──
    v.push(step("a context oid nobody minted", ctx_key("99"), "list", u32_arg(10)));
    v.push(step("a key from another key space", b"foreign".to_vec(), "list", u32_arg(10)));
    v.push(step(
        "a key under the wrong infix",
        {
            let mut k = ROOT.to_vec();
            k.extend_from_slice(b"/NamingContextExt/1");
            k
        },
        "list",
        u32_arg(10),
    ));

    v
}

// ── The measurement ──────────────────────────────────────────────────────────

/// The claim: a generated skeleton answers `naming_server.rs`'s multi-context
/// shape byte for byte, in both byte orders.
#[test]
fn a_generated_skeleton_answers_as_the_naming_server_does() {
    let steps = script();
    for endian in ORDERS {
        // Fresh servants per byte order: the script mutates, so a second pass
        // over used servants would compare a different tree.
        let mut hand = hand_written();
        let mut from_idl = generated();
        for (i, s) in steps.iter().enumerate() {
            let want = ask(&mut hand, endian, &s.key, s.op, &s.args);
            let got = ask(&mut from_idl, endian, &s.key, s.op, &s.args);
            assert_eq!(
                want,
                got,
                "{endian:?} step {i} ({}): {} on {:?}",
                s.what,
                s.op,
                String::from_utf8_lossy(&s.key)
            );
        }
    }
}

/// The negative control: a comparison of two servants that both answered
/// nothing would pass. This pins what the script actually exercised.
#[test]
fn the_comparison_is_not_vacuous() {
    let steps = script();
    // Pinned, not bounded: a matrix that silently shrinks is a comparison that
    // silently weakens.
    assert_eq!(steps.len(), 59, "the script changed length");

    let mut from_idl = generated();
    let (mut nonempty, mut empty, mut raised, mut user, mut unknown) = (0, 0, 0, 0, 0);
    for s in &steps {
        match ask(&mut from_idl, Endian::Big, &s.key, s.op, &s.args) {
            Answer::Body(rt::DispatchBody::UserException, b) => {
                user += 1;
                assert!(!b.is_empty(), "{}: a user exception body carries its id", s.what);
            }
            Answer::Body(rt::DispatchBody::Return, b) => {
                if b.is_empty() {
                    empty += 1;
                } else {
                    nonempty += 1;
                }
            }
            Answer::Raised { .. } => raised += 1,
            Answer::Unknown => unknown += 1,
        }
    }
    // Measured, then pinned. Each class is a different path through the
    // skeleton, and a script that stopped reaching one of them would still
    // pass the byte comparison — vacuously.
    assert_eq!(nonempty, 25, "replies carrying a value");
    assert_eq!(empty, 7, "void replies: the seven mutations that succeeded");
    assert_eq!(user, 18, "user exceptions: NotFound, AlreadyBound, InvalidName, InvalidAddress");
    assert_eq!(raised, 6, "system exceptions: three deferrals, one BAD_OPERATION, two MARSHAL");
    assert_eq!(unknown, 3, "keys neither servant claims");
    assert_eq!(nonempty + empty + user + raised + unknown, steps.len(), "every step is classed");

    // And the oracle must not be answering trivially either: after the script,
    // `resolve` on a minted context has to be a real reference with this
    // server's address in it.
    let mut hand = hand_written();
    for s in &steps {
        ask(&mut hand, Endian::Big, &s.key, s.op, &s.args);
    }
    let Answer::Body(_, body) = ask(&mut hand, Endian::Big, ROOT, "resolve", &name_args(&["ctx"]))
    else {
        panic!("the oracle must resolve the minted context");
    };
    let mut d = rt::Decoder::new(&body, Endian::Big);
    let ior = Ior::read_from(&mut d).expect("a reference");
    assert_eq!(ior.type_id, NAMING_CONTEXT_EXT_ID);
    assert_eq!(ior.profiles[0].port, PORT);
    assert_eq!(ior.profiles[0].object_key, ctx_key("1"));
}

/// Byte equality is the measurement; this is the check that the bytes both
/// servants agree on are the *right* ones.
///
/// Every structured reply is read back with `orbweaver-giop`'s own decoders —
/// the ones the client half and omniNames have been measured against — rather
/// than with the generated `Cdr` impls that wrote them. Two servants can agree
/// on the wrong bytes; a servant and an independent reader cannot agree on
/// bytes that are not the shape the specification describes.
#[test]
fn the_generated_replies_decode_as_the_oracles_own_readers() {
    for endian in ORDERS {
        let mut from_idl = generated();
        // Drive the whole script so the tree is in the state the assertions
        // below expect, then ask again.
        for s in &script() {
            ask(&mut from_idl, endian, &s.key, s.op, &s.args);
        }

        // A minted context reference, read as an IOR.
        let Answer::Body(_, body) =
            ask(&mut from_idl, endian, ROOT, "resolve", &name_args(&["ctx"]))
        else {
            panic!("{endian:?}: resolve must answer with a body");
        };
        let mut d = rt::Decoder::new(&body, endian);
        let ior = Ior::read_from(&mut d).expect("the oracle's own reader");
        assert_eq!(d.remaining(), 0, "{endian:?}: trailing bytes after the reference");
        assert_eq!(ior.type_id, NAMING_CONTEXT_EXT_ID);
        assert_eq!(ior.profiles[0].host, HOST);
        assert_eq!(ior.profiles[0].port, PORT);
        assert_eq!(ior.profiles[0].object_key, ctx_key("1"), "the key the oracle mints");
        assert_eq!(ior.profiles[0].version, Version::V1_2);

        // A listing, read as `BindingList` + a nil `BindingIterator`.
        let Answer::Body(_, body) = ask(&mut from_idl, endian, ROOT, "list", &u32_arg(10)) else {
            panic!("{endian:?}: list must answer with a body");
        };
        let mut d = rt::Decoder::new(&body, endian);
        let count = d.get_u32().expect("the sequence length");
        let mut listed = Vec::new();
        for _ in 0..count {
            let name = naming::read_name(&mut d).expect("the oracle's own name reader");
            let kind = d.get_u32().expect("the binding type ordinal");
            listed.push((name, kind));
        }
        let iterator = Ior::read_from(&mut d).expect("the iterator slot is still an IOR");
        assert_eq!(d.remaining(), 0, "{endian:?}: trailing bytes after the listing");
        // "a" was unbound by the script; "ctx" is a context.
        assert_eq!(listed.len(), 1, "{endian:?}: {listed:?}");
        assert_eq!(listed[0].0, vec![nc("ctx")]);
        assert_eq!(listed[0].1, 1, "ncontext is ordinal 1");
        assert!(
            iterator.type_id.is_empty() && iterator.profiles.is_empty(),
            "the iterator is nil (§9.3.6), not absent"
        );

        // A `NotFound` body, read as the client half reads it: repository id
        // first, then the reason ordinal, then the remainder of the path.
        let Answer::Body(kind, body) =
            ask(&mut from_idl, endian, ROOT, "resolve", &name_args(&["nope", "deep"]))
        else {
            panic!("{endian:?}: resolve must raise");
        };
        assert_eq!(kind, rt::DispatchBody::UserException);
        let mut d = rt::Decoder::new(&body, endian);
        let id = String::from_utf8_lossy(d.get_string_bytes().expect("the id")).into_owned();
        assert_eq!(id, NOT_FOUND_ID);
        assert_eq!(d.get_u32().expect("why"), naming_server::WHY_MISSING_NODE);
        assert_eq!(
            naming::read_name(&mut d).expect("rest_of_name"),
            vec![nc("nope"), nc("deep")],
            "the remainder starts at the component that failed"
        );
        assert_eq!(d.remaining(), 0, "{endian:?}: trailing bytes after the exception");

        // The three member-less exceptions: the whole body is the id.
        for (op, args, want) in [
            ("resolve", name_args(&[]), INVALID_NAME_ID),
            ("bind", name_and_ref(&["ctx"], "X"), ALREADY_BOUND_ID),
            ("to_url", string_args(&["not::an::address", "a"]), INVALID_ADDRESS_ID),
        ] {
            let Answer::Body(kind, body) = ask(&mut from_idl, endian, ROOT, op, &args) else {
                panic!("{endian:?}: {op} must raise");
            };
            assert_eq!(kind, rt::DispatchBody::UserException, "{op}");
            let mut d = rt::Decoder::new(&body, endian);
            let id = String::from_utf8_lossy(d.get_string_bytes().expect("the id")).into_owned();
            assert_eq!(id, want, "{op}");
            assert_eq!(d.remaining(), 0, "{op}: no members, so nothing after the id");
        }
    }
}

/// The generated key space is the hand-written one, which is what makes a
/// minted reference comparable as bytes at all.
#[test]
fn the_generated_key_space_is_the_naming_servers() {
    let hand = hand_written();
    let generated = refs();
    assert_eq!(generated.root_key(), hand.root_key());
    assert_eq!(generated.ior("").profiles[0].object_key, hand.root_ior().profiles[0].object_key);
    assert_eq!(generated.ior(""), hand.root_ior(), "the root reference, whole");

    // `NamingServer::mint_context` builds `root ++ "/_ctx" ++ <n>`; the
    // generated scheme has to produce the same bytes for the same oid, and be
    // reversible, or a request addressed to a minted context is not ours.
    for n in ["1", "2", "17", "9999"] {
        let mut want = ROOT.to_vec();
        want.extend_from_slice(format!("/_ctx{n}").as_bytes());
        assert_eq!(generated.key_of(n), want, "oid {n}");
        assert_eq!(generated.oid_of(&want), Some(n), "oid {n} round-trips");
    }
    assert_eq!(generated.oid_of(ROOT), Some(""), "the bare root key is the default object");
    assert_eq!(generated.oid_of(b"foreign"), None);
}

/// `knows` is required with no default, and this is what the default would
/// have cost: a context nobody minted, answered as though it existed.
///
/// The gap statement this file closes named exactly this. A `Dispatch::knows`
/// that defaults to `true` claims every key in the process, so a naming server
/// built on one would answer `list` on `NameService/_ctx99` with the *root*
/// context's bindings.
#[test]
fn a_context_nobody_minted_is_refused_by_both() {
    let mut hand = hand_written();
    let mut from_idl = generated();
    for key in [ctx_key("1"), ctx_key("99"), b"foreign".to_vec()] {
        assert_eq!(
            hand.knows(&key),
            Dispatch::knows(&from_idl, &key),
            "{:?}",
            String::from_utf8_lossy(&key)
        );
        assert!(!Dispatch::knows(&from_idl, &key), "nothing has been minted yet");
    }

    // Mint one, and only that one becomes known — on both sides.
    let minted = ask(&mut from_idl, Endian::Big, ROOT, "bind_new_context", &name_args(&["c"]));
    assert!(matches!(minted, Answer::Body(rt::DispatchBody::Return, _)));
    assert_eq!(ask(&mut hand, Endian::Big, ROOT, "bind_new_context", &name_args(&["c"])), minted);
    assert!(Dispatch::knows(&from_idl, &ctx_key("1")));
    assert!(hand.knows(&ctx_key("1")));
    assert!(!Dispatch::knows(&from_idl, &ctx_key("2")));
    assert!(!hand.knows(&ctx_key("2")));
}

// ── What is still not the same, and why ──────────────────────────────────────

/// Where the generated skeleton and `naming_server.rs` answer differently.
///
/// One entry, and it is the ordering difference `ifr_shape.rs` already
/// records: it is a property of *every* generated skeleton, not of this
/// contract, which is what makes it a root cause rather than a case.
/// Re-measured by [`the_named_divergence_still_diverges`], because a list of
/// gaps nobody re-measures is a list of things that were once true.
///
/// **Three clauses of the gap statement this file was written against were
/// already false** — object keys, `knows()` and the forward seam all exist and
/// are measured (`tests/object_identity.rs`, `tests/ifr_shape.rs`). Nothing
/// about the naming shape turned out to be inexpressible: no `NOT_COMPARED`
/// entry here is about identity, minting, key parsing or reference assembly.
const NOT_COMPARED: [(&str, &str); 1] = [(
    "a malformed body under a deferred operation",
    "`naming_server.rs` matches the operation name before it decodes any argument, so a \
     `bind_context` whose body does not parse is NO_IMPLEMENT. A generated skeleton decodes the \
     arguments the contract declares and only then calls the servant, so the same request is \
     MARSHAL — the servant never runs and never gets to defer. Ordering a refusal ahead of \
     decoding is a policy no contract can state; a servant that needs it overrides at the \
     `Dispatch` level. With a well-formed body the two agree (three deferred operations in \
     `script`), which is what makes this an ordering difference and not a missing answer.",
)];

/// The named divergence must still be one, and must be the only one.
#[test]
fn the_named_divergence_still_diverges() {
    assert_eq!(NOT_COMPARED.len(), 1);
    let mut hand = hand_written();
    let mut from_idl = generated();
    let big = Endian::Big;

    // A `Name` claiming five components and carrying none, under an operation
    // both servants defer.
    let malformed = u32_arg(5);
    match (
        ask(&mut hand, big, ROOT, "bind_context", &malformed),
        ask(&mut from_idl, big, ROOT, "bind_context", &malformed),
    ) {
        (Answer::Raised { id: a, .. }, Answer::Raised { id: b, .. }) => {
            assert_eq!(a, rt::NO_IMPLEMENT, "refused before the body is read");
            assert_eq!(b, rt::MARSHAL, "the arguments are decoded before the servant is called");
        }
        other => panic!("both must refuse, differently: {other:?}"),
    }

    // With a well-formed body the two agree — the difference is the ordering
    // and not the answer.
    let well_formed = name_and_ref(&["c"], "C");
    assert_eq!(
        ask(&mut hand, big, ROOT, "bind_context", &well_formed),
        ask(&mut from_idl, big, ROOT, "bind_context", &well_formed),
    );

    // And the deferral is distinguishable from an oversight on both sides,
    // which is the whole reason the contract declares operations it does not
    // serve.
    match ask(&mut from_idl, big, ROOT, "bind_context", &well_formed) {
        Answer::Raised { id, .. } => assert_eq!(id, rt::NO_IMPLEMENT),
        other => panic!("a deferred operation is NO_IMPLEMENT, not {other:?}"),
    }
    match ask(&mut from_idl, big, ROOT, "frobnicate", &none()) {
        Answer::Raised { id, .. } => assert_eq!(id, rt::BAD_OPERATION),
        other => panic!("an undeclared operation is BAD_OPERATION, not {other:?}"),
    }
}

/// The contract really does carry the ids the comparison depends on.
///
/// Two things travel as repository ids here and a look-alike would compare
/// equal to nothing: `_is_a`'s answer, and the first field of every user
/// exception body. `#pragma prefix "omg.org"` is what makes them the real
/// ones, and dropping it is a change no compiler catches.
#[test]
fn the_contract_carries_the_omg_repository_ids() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/services/gen-naming-subset.idl"),
    )
    .expect("the contract");
    let spec = orbweaver_idl::parse(&src).expect("the contract parses");
    let mut r = orbweaver_registry::Registry::new();
    r.load(&spec).expect("loads");

    for id in [NAMING_CONTEXT_ID, NAMING_CONTEXT_EXT_ID] {
        assert!(r.interface(id).is_some(), "{id} must be declared with the OMG id");
    }
    for id in [NOT_FOUND_ID, ALREADY_BOUND_ID, INVALID_NAME_ID, INVALID_ADDRESS_ID] {
        assert!(
            r.get(id).is_some(),
            "{id} must be declared with the OMG id — the exception body carries it first, so a \
             look-alike is a silent divergence the byte comparison would report as a mismatch"
        );
    }
    // The Ext interface inherits the plain one, which is what makes one
    // skeleton answer `_is_a` for both — as `naming_server.rs` does.
    assert!(
        r.ancestors(NAMING_CONTEXT_EXT_ID).iter().any(|a| a == NAMING_CONTEXT_ID),
        "NamingContextExt must derive from NamingContext"
    );
}
