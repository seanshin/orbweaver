//! First-party CosNaming *server* — the other half of [`crate::naming`].
//!
//! The client half shipped in Phase 1 and has only ever spoken to omniNames;
//! the review's finding was the asymmetry: we could resolve against anyone's
//! naming service but could not be one. This servant closes it, in memory,
//! behind the existing [`Server`]/[`Dispatch`] machinery.
//!
//! # What is served, and what honestly is not
//!
//! `resolve`, `bind`, `rebind`, `unbind`, `bind_new_context`, `new_context`,
//! `list`, and the `NamingContextExt` string surface (`resolve_str`,
//! `to_name`, `to_string`, `to_url`) — plus `_is_a`/`_non_existent`, which
//! every ORB probes with before trusting a narrow. Everything else answers
//! `BAD_OPERATION` rather than being half-served:
//!
//! - **`BindingIterator` is stubbed.** `list` returns at most `how_many`
//!   bindings and a **nil** iterator; anything beyond `how_many` is simply
//!   not reported on that call. A nil iterator conventionally means "you
//!   have everything", so a truncated `list` under-reports — a caller that
//!   wants the full set passes a large `how_many`. Real iterators need
//!   servant lifecycle, which is POA work.
//! - **`bind_context`/`rebind_context` are not served.** They would bind a
//!   *foreign* context, and resolving through one means chaining the call
//!   over the wire — not v1 work. [`NamingContext::bind_new_context`] covers
//!   local nesting: every new context is a fresh object key behind this same
//!   dispatch, and [`Dispatch::knows`] answers for all of them.
//! - **`destroy` is not served**; contexts live as long as the process, and
//!   an unbound context stays reachable by its key.
//!
//! # `to_url` and the parser it has to agree with
//!
//! `to_url` was the one `NamingContextExt` operation absent while its client
//! half already shipped: [`crate::naming`] has parsed `corbaname:` URLs since
//! Phase 1, and `to_url` is the operation that *produces* one. It is served
//! by [`crate::naming::to_url`], which builds the URL, parses what it built
//! with that same parser, and refuses to hand back anything the parser reads
//! as a different name — so the two halves cannot disagree about escaping the
//! way an encoder and a decoder written apart always eventually do.
//!
//! This servant does **not** consult its own tree to answer it: §2.5.3.3's
//! `to_url` is a pure string operation over an address the *caller* supplies,
//! and inventing a "did you mean this context?" check would answer a question
//! the operation does not ask.
//!
//! # Exception shapes
//!
//! CosNaming failures are **user** exceptions, so this is the first servant
//! to use [`DispatchBody::UserException`]: the body is the repository id
//! followed by the members — for `NotFound`, the `why` enum then the
//! `rest_of_name` starting at the component that failed. The oracle for the
//! shape is our own client: [`crate::Error::UserException`] hands back a
//! reply whose `body()` starts at that repository id, and the unit tests
//! decode every raised exception through it, both byte orders.
//!
//! # Sharing: one `RwLock` over the whole tree
//!
//! This servant implements [`SharedDispatch`], so two calls into it may run at
//! once. The sharing decision is a per-servant one and this is the argument
//! for *this* servant:
//!
//! - **One lock, not one per context.** A `resolve` walks several contexts and
//!   a `bind_new_context` writes two — the child it mints and the parent it
//!   binds into. Locking per context would mean holding two at once, which is
//!   the lock-ordering problem [`crate::guarded`]'s discipline exists to make
//!   impossible, and it would let a walk observe a half-applied bind. The tree
//!   is one consistency domain, so it gets one lock.
//! - **`RwLock`, not `Mutex`.** A naming service is read-dominated by
//!   construction: `resolve`, `resolve_str`, `list`, `to_name`, `to_string`,
//!   `_is_a` and `knows` are the traffic, and `bind`/`unbind` are
//!   configuration. Those reads now overlap, which is the whole point of the
//!   batch — a slow `list` over a large context no longer delays a `resolve`
//!   on an unrelated one.
//! - **Nothing blocking inside it.** Naming *stores* references and never
//!   dials one: there is no [`crate::Connection`] anywhere in this module, so
//!   the "no lock across an outbound call" rule is satisfied structurally
//!   rather than by care. If a future `bind_context` chains a resolve over the
//!   wire, that call must happen outside the lock and the tripwire in
//!   [`crate::guarded`] will say so.
//!
//! One consequence, stated because it is now reachable: `knows` and the
//! dispatch are two separate looks at the tree, so a context could in
//! principle be unbound between them. Contexts are never destroyed here, so
//! today the window is empty — and `table()` answers `OBJECT_NOT_EXIST`
//! rather than panicking either way.
//!
//! [`NamingContext::bind_new_context`]: crate::naming::NamingContext::bind_new_context
//! [`Server`]: crate::server::Server
//! [`SharedDispatch`]: crate::server::SharedDispatch

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use orbweaver_cdr::Encoder;

use crate::guarded::Guarded;
use crate::naming::{
    NAMING_CONTEXT_EXT_ID, NameComponent, UrlError, parse_stringified_name, read_name,
    stringify_name, to_url, write_name,
};
use crate::server::{Dispatch, DispatchBody, Request, SharedDispatch, SystemException};
use crate::{IiopProfile, Ior, Version};

/// Repository id of the plain (pre-Ext) naming context interface. `_is_a`
/// answers `true` for it and for [`NAMING_CONTEXT_EXT_ID`], so a client may
/// narrow to either.
pub const NAMING_CONTEXT_ID: &str = "IDL:omg.org/CosNaming/NamingContext:1.0";

/// Repository id of `CosNaming::NamingContext::NotFound`.
pub const NOT_FOUND_ID: &str = "IDL:omg.org/CosNaming/NamingContext/NotFound:1.0";

/// Repository id of `CosNaming::NamingContext::AlreadyBound`.
pub const ALREADY_BOUND_ID: &str = "IDL:omg.org/CosNaming/NamingContext/AlreadyBound:1.0";

/// Repository id of `CosNaming::NamingContext::InvalidName`.
pub const INVALID_NAME_ID: &str = "IDL:omg.org/CosNaming/NamingContext/InvalidName:1.0";

/// Repository id of `CosNaming::NamingContextExt::InvalidAddress` — declared
/// by the `Ext` interface alone, and raised only by `to_url`.
pub const INVALID_ADDRESS_ID: &str = "IDL:omg.org/CosNaming/NamingContextExt/InvalidAddress:1.0";

/// `NotFoundReason::missing_node` — the component is not bound at all.
pub const WHY_MISSING_NODE: u32 = 0;
/// `NotFoundReason::not_context` — an intermediate component is bound to an
/// object, so resolution cannot continue through it.
pub const WHY_NOT_CONTEXT: u32 = 1;
/// `NotFoundReason::not_object` — `rebind` found a context where it may only
/// replace an object.
pub const WHY_NOT_OBJECT: u32 = 2;

/// `BindingType::nobject` on the wire.
const BINDING_OBJECT: u32 = 0;
/// `BindingType::ncontext` on the wire.
const BINDING_CONTEXT: u32 = 1;

/// The user exceptions this servant raises.
#[derive(Debug, Clone)]
enum UserExc {
    /// `NotFound { why, rest_of_name }`; `rest` starts at the component that
    /// failed, as omniNames reports it.
    NotFound { why: u32, rest: Vec<NameComponent> },
    /// `AlreadyBound`, no members.
    AlreadyBound,
    /// `InvalidName`, no members.
    InvalidName,
    /// `NamingContextExt::InvalidAddress`, no members.
    InvalidAddress,
}

impl UserExc {
    /// Writes the exception body: repository id first, then the members —
    /// the exact shape the client decodes back out of
    /// [`crate::Error::UserException`].
    fn write(&self, out: &mut Encoder) {
        match self {
            UserExc::NotFound { why, rest } => {
                out.put_str(NOT_FOUND_ID);
                out.put_u32(*why);
                write_name(out, rest);
            }
            UserExc::AlreadyBound => out.put_str(ALREADY_BOUND_ID),
            UserExc::InvalidName => out.put_str(INVALID_NAME_ID),
            UserExc::InvalidAddress => out.put_str(INVALID_ADDRESS_ID),
        }
    }
}

/// A failure a handler raises: a CosNaming user exception, or a system
/// exception for the failures CORBA itself defines.
enum Raise {
    User(UserExc),
    System(SystemException),
}

impl From<UserExc> for Raise {
    fn from(e: UserExc) -> Self {
        Raise::User(e)
    }
}

impl From<SystemException> for Raise {
    fn from(e: SystemException) -> Self {
        Raise::System(e)
    }
}

/// A `MARSHAL` for arguments that did not decode, as a [`Raise`].
fn marshal() -> Raise {
    Raise::System(SystemException::marshal())
}

/// What a name is bound to.
#[derive(Debug, Clone)]
enum Bound {
    /// An object reference, held verbatim — naming stores references, it
    /// never dials them.
    Object(Ior),
    /// A context served by this same dispatch, held by its object key.
    Context(Vec<u8>),
}

/// Bindings of one context, keyed by `(id, kind)`. A `BTreeMap` so `list`
/// order is deterministic.
type Bindings = BTreeMap<(String, String), Bound>;

fn slot(c: &NameComponent) -> (String, String) {
    (c.id.clone(), c.kind.clone())
}

/// The nil object reference: empty type id, no profiles (§9.3.6).
fn nil_ref() -> Ior {
    Ior { type_id: String::new(), profiles: Vec::new() }
}

/// The whole context tree, and the only mutable state this servant has.
///
/// It is one consistency domain behind one lock — see the module docs on
/// sharing for why a lock per context would be both a lock-ordering hazard
/// and a torn `walk`.
#[derive(Debug)]
struct Tree {
    contexts: BTreeMap<Vec<u8>, Bindings>,
    minted: u64,
}

impl Tree {
    /// Creates an unbound context under `root` and returns its fresh object
    /// key.
    fn mint_context(&mut self, root: &[u8]) -> Vec<u8> {
        self.minted += 1;
        let mut key = root.to_vec();
        key.extend_from_slice(format!("/_ctx{}", self.minted).as_bytes());
        self.contexts.insert(key.clone(), Bindings::new());
        key
    }

    /// The binding table of the context behind `key`. Contexts are never
    /// destroyed, so a missing key means the request addressed an object
    /// this server never minted.
    fn table(&self, key: &[u8]) -> Result<&Bindings, Raise> {
        self.contexts.get(key).ok_or_else(|| Raise::System(SystemException::object_not_exist()))
    }

    fn table_mut(&mut self, key: &[u8]) -> Result<&mut Bindings, Raise> {
        self.contexts.get_mut(key).ok_or_else(|| Raise::System(SystemException::object_not_exist()))
    }
}

/// An in-memory CosNaming servant behind [`Server`].
///
/// One instance serves a whole context *tree*: the root context is the
/// object key the [`Server`] was bound with, and every context minted by
/// `bind_new_context`/`new_context` is a further key on the same dispatch —
/// which is why [`SharedDispatch::knows`] is answered from the context table
/// rather than defaulted.
///
/// `host`, `port` and the root key are set at construction and never change,
/// so they live outside the lock: what a reference *points at* is not part of
/// what the tree *holds*, and taking a lock to read a constant would be the
/// serialization this batch removes, reintroduced by habit. They are the
/// caller's to publish correctly (Phase 0 assumption D: the bind address and
/// the publishable address differ behind NAT).
///
/// [`Server`]: crate::server::Server
/// [`SharedDispatch::knows`]: crate::server::SharedDispatch::knows
#[derive(Debug)]
pub struct NamingServer {
    host: String,
    port: u16,
    root: Vec<u8>,
    tree: Guarded<Tree>,
}

impl NamingServer {
    /// A naming server rooted at `root_key`, minting references that point
    /// at `host:port`.
    pub fn new(host: impl Into<String>, port: u16, root_key: Vec<u8>) -> Self {
        let mut contexts = BTreeMap::new();
        contexts.insert(root_key.clone(), Bindings::new());
        Self {
            host: host.into(),
            port,
            root: root_key,
            tree: Guarded::new("the naming tree", Tree { contexts, minted: 0 }),
        }
    }

    /// The root context's object key — what the [`Server`] must be bound
    /// with for the two to describe the same object.
    ///
    /// [`Server`]: crate::server::Server
    pub fn root_key(&self) -> &[u8] {
        &self.root
    }

    /// A publishable reference to the root context, advertising
    /// [`NAMING_CONTEXT_EXT_ID`] — `resolve_str`/`to_name`/`to_string` are
    /// served, so the Ext claim is honest.
    pub fn root_ior(&self) -> Ior {
        self.ior_for(&self.root)
    }

    fn ior_for(&self, key: &[u8]) -> Ior {
        Ior {
            type_id: NAMING_CONTEXT_EXT_ID.to_owned(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: self.host.clone(),
                port: self.port,
                object_key: key.to_vec(),
                components: Vec::new(),
            }],
        }
    }

    /// The reference a resolved binding hands back. A context is named by its
    /// key, so its reference is minted here — outside the lock, from fields
    /// that cannot change.
    fn ior_of(&self, bound: Bound) -> Ior {
        match bound {
            Bound::Object(ior) => ior,
            Bound::Context(key) => self.ior_for(&key),
        }
    }

    /// Dispatches one operation, writing the result body into `out`.
    ///
    /// The lock is taken **once per operation** — the whole of one request is
    /// one look at the tree, which is what the server's own mutex used to
    /// give and what keeps a `walk` from observing a half-applied `bind`. It
    /// is a read for the read-only surface and a write for the four
    /// operations that change something; nothing here calls out, so nothing
    /// blocking happens inside either.
    ///
    /// Invariant every arm keeps: nothing is written into `out` until the
    /// operation can no longer raise a *user* exception, because the buffer
    /// travels whole under a single reply status. (A system exception after
    /// a partial write is fine — the server discards `out` on that path.)
    fn handle(&self, req: &Request, out: &mut Encoder) -> Result<(), Raise> {
        let mut args = req.body().map_err(|_| marshal())?;
        match req.operation.as_str() {
            "_is_a" => {
                let id = args.get_string().map_err(|_| marshal())?;
                out.put_bool(matches!(
                    id.as_str(),
                    NAMING_CONTEXT_EXT_ID | NAMING_CONTEXT_ID | "IDL:omg.org/CORBA/Object:1.0"
                ));
            }
            "_non_existent" => out.put_bool(false),
            "resolve" => {
                let name = read_name(&mut args).map_err(|_| marshal())?;
                let bound = self.tree.read(|t| t.resolve_from(&req.object_key, &name))?;
                self.ior_of(bound).write_to(out).map_err(|_| marshal())?;
            }
            "resolve_str" => {
                let s = args.get_string().map_err(|_| marshal())?;
                let name = parse_stringified_name(&s).map_err(|_| UserExc::InvalidName)?;
                let bound = self.tree.read(|t| t.resolve_from(&req.object_key, &name))?;
                self.ior_of(bound).write_to(out).map_err(|_| marshal())?;
            }
            "bind" | "rebind" => {
                let name = read_name(&mut args).map_err(|_| marshal())?;
                let obj = Ior::read_from(&mut args).map_err(|_| marshal())?;
                let overwrite = req.operation == "rebind";
                self.tree.write(|t| {
                    t.bind_from(&req.object_key, &name, Bound::Object(obj), overwrite)
                })?;
            }
            "unbind" => {
                let name = read_name(&mut args).map_err(|_| marshal())?;
                self.tree.write(|t| t.unbind_from(&req.object_key, &name))?;
            }
            "bind_new_context" => {
                let name = read_name(&mut args).map_err(|_| marshal())?;
                let key = self
                    .tree
                    .write(|t| t.bind_new_context_from(&self.root, &req.object_key, &name))?;
                self.ior_for(&key).write_to(out).map_err(|_| marshal())?;
            }
            "new_context" => {
                let key = self.tree.write(|t| t.mint_context(&self.root));
                self.ior_for(&key).write_to(out).map_err(|_| marshal())?;
            }
            "list" => {
                let how_many = args.get_u32().map_err(|_| marshal())?;
                // Written from inside the read section: copying the bindings
                // out first would double a large context in memory to save a
                // lock that other readers can hold at the same time anyway.
                self.tree.read(|t| {
                    let table = t.table(&req.object_key)?;
                    let take = (how_many as usize).min(table.len());
                    out.put_u32(take as u32);
                    for ((id, kind), bound) in table.iter().take(take) {
                        write_name(out, &[NameComponent { id: id.clone(), kind: kind.clone() }]);
                        out.put_u32(match bound {
                            Bound::Object(_) => BINDING_OBJECT,
                            Bound::Context(_) => BINDING_CONTEXT,
                        });
                    }
                    Ok::<(), Raise>(())
                })?;
                // The stub: always a nil iterator, even when `take` truncated
                // the list — see the module docs for why that is accepted.
                nil_ref().write_to(out).map_err(|_| marshal())?;
            }
            "to_name" => {
                let s = args.get_string().map_err(|_| marshal())?;
                let name = parse_stringified_name(&s).map_err(|_| UserExc::InvalidName)?;
                if name.is_empty() {
                    return Err(UserExc::InvalidName.into());
                }
                write_name(out, &name);
            }
            "to_string" => {
                let name = read_name(&mut args).map_err(|_| marshal())?;
                if name.is_empty() {
                    return Err(UserExc::InvalidName.into());
                }
                out.put_str(&stringify_name(&name));
            }
            // `URLString to_url(in Address addr, in StringName sn)
            //  raises(InvalidAddress, InvalidName)`. The whole answer comes
            // from `crate::naming`, so the URL this hands out and the URL our
            // client parses are one piece of code — see the module docs.
            "to_url" => {
                let address = args.get_string().map_err(|_| marshal())?;
                let name = args.get_string().map_err(|_| marshal())?;
                let url = to_url(&address, &name).map_err(|e| match e {
                    UrlError::BadAddress(_) => Raise::User(UserExc::InvalidAddress),
                    UrlError::BadSchemeName(_) | UrlError::BadSchemeSpecificPart(_) => {
                        Raise::User(UserExc::InvalidName)
                    }
                    // The round-trip check refused what it built: our own two
                    // halves disagree, which is this servant's defect and not
                    // a statement about either argument.
                    UrlError::Other(_) => Raise::System(SystemException::internal()),
                })?;
                out.put_str(&url);
            }
            _ => return Err(SystemException::bad_operation().into()),
        }
        Ok(())
    }
}

impl Tree {
    /// Walks every component but the last, context to context, and returns
    /// the final context's key plus the last component.
    ///
    /// Failures carry `rest_of_name` starting at the component that failed —
    /// `missing_node` when it is not bound, `not_context` when it is bound
    /// to an object that resolution cannot continue through.
    fn walk(
        &self,
        start: &[u8],
        name: &[NameComponent],
    ) -> Result<(Vec<u8>, NameComponent), Raise> {
        let Some((last, path)) = name.split_last() else {
            return Err(UserExc::InvalidName.into());
        };
        let mut ctx = start.to_vec();
        for (i, c) in path.iter().enumerate() {
            match self.table(&ctx)?.get(&slot(c)) {
                None => {
                    return Err(UserExc::NotFound {
                        why: WHY_MISSING_NODE,
                        rest: name[i..].to_vec(),
                    }
                    .into());
                }
                Some(Bound::Object(_)) => {
                    return Err(UserExc::NotFound {
                        why: WHY_NOT_CONTEXT,
                        rest: name[i..].to_vec(),
                    }
                    .into());
                }
                Some(Bound::Context(k)) => ctx = k.clone(),
            }
        }
        Ok((ctx, last.clone()))
    }

    fn resolve_from(&self, start: &[u8], name: &[NameComponent]) -> Result<Bound, Raise> {
        let (ctx, last) = self.walk(start, name)?;
        match self.table(&ctx)?.get(&slot(&last)) {
            None => Err(UserExc::NotFound { why: WHY_MISSING_NODE, rest: vec![last] }.into()),
            Some(bound) => Ok(bound.clone()),
        }
    }

    fn bind_from(
        &mut self,
        start: &[u8],
        name: &[NameComponent],
        to: Bound,
        overwrite: bool,
    ) -> Result<(), Raise> {
        let (ctx, last) = self.walk(start, name)?;
        match self.table_mut(&ctx)?.entry(slot(&last)) {
            Entry::Vacant(v) => {
                v.insert(to);
                Ok(())
            }
            Entry::Occupied(mut o) => {
                if !overwrite {
                    return Err(UserExc::AlreadyBound.into());
                }
                if matches!(o.get(), Bound::Context(_)) {
                    // rebind replaces objects only; replacing a context is
                    // rebind_context's job, which is not served.
                    return Err(UserExc::NotFound { why: WHY_NOT_OBJECT, rest: vec![last] }.into());
                }
                o.insert(to);
                Ok(())
            }
        }
    }

    fn unbind_from(&mut self, start: &[u8], name: &[NameComponent]) -> Result<(), Raise> {
        let (ctx, last) = self.walk(start, name)?;
        match self.table_mut(&ctx)?.remove(&slot(&last)) {
            // An unbound context stays reachable by key; destroy is not served.
            Some(_) => Ok(()),
            None => Err(UserExc::NotFound { why: WHY_MISSING_NODE, rest: vec![last] }.into()),
        }
    }

    fn bind_new_context_from(
        &mut self,
        root: &[u8],
        start: &[u8],
        name: &[NameComponent],
    ) -> Result<Vec<u8>, Raise> {
        // Occupancy is checked before minting, or a failed bind would leak an
        // unreachable context.
        let (ctx, last) = self.walk(start, name)?;
        if self.table(&ctx)?.contains_key(&slot(&last)) {
            return Err(UserExc::AlreadyBound.into());
        }
        let key = self.mint_context(root);
        self.table_mut(&ctx)?.insert(slot(&last), Bound::Context(key.clone()));
        Ok(key)
    }
}

impl SharedDispatch for NamingServer {
    /// One dispatch answers for the whole context tree: the root key and
    /// every key `bind_new_context`/`new_context` minted.
    fn knows(&self, object_key: &[u8]) -> bool {
        self.tree.read(|t| t.contexts.contains_key(object_key))
    }

    fn dispatch_body(
        &self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<DispatchBody, SystemException> {
        match self.handle(request, out) {
            Ok(()) => Ok(DispatchBody::Return),
            Err(Raise::System(ex)) => Err(ex),
            Err(Raise::User(ex)) => {
                // `handle` wrote nothing before raising (its invariant), so
                // the exception body is the whole buffer.
                ex.write(out);
                Ok(DispatchBody::UserException)
            }
        }
    }

    /// The narrow entry point cannot carry a user exception, so one arriving
    /// here gets the standard mapping: `UNKNOWN`, OMG minor 1.
    /// [`Server`](crate::server::Server)
    /// never takes this path — it calls `dispatch_body` — but the trait
    /// requires the method and lying with `NO_EXCEPTION` would be worse.
    fn dispatch(
        &self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        match self.dispatch_body(request, out)? {
            DispatchBody::Return => Ok(()),
            DispatchBody::UserException => Err(SystemException::unknown_user_exception()),
        }
    }
}

/// The `&mut self` shape as well, so a caller with a
/// [`Server::serve`](crate::server::Server::serve) already written keeps
/// working — serialized, as that path always was. Every method forwards to
/// the shared one, so there is exactly one implementation of the naming
/// semantics and no second copy to drift.
impl Dispatch for NamingServer {
    fn knows(&self, object_key: &[u8]) -> bool {
        SharedDispatch::knows(self, object_key)
    }

    fn dispatch_body(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<DispatchBody, SystemException> {
        SharedDispatch::dispatch_body(self, request, out)
    }

    fn dispatch(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        SharedDispatch::dispatch(self, request, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::naming::NamingContext;
    use crate::server::Server;
    use crate::{DEFAULT_MAX_MESSAGE_SIZE, Error};
    use orbweaver_cdr::Endian;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    const T: Duration = Duration::from_secs(5);

    fn nc(id: &str) -> NameComponent {
        NameComponent::new(id)
    }

    fn dummy(key: &[u8]) -> Ior {
        Ior {
            type_id: "IDL:spike/Echo:1.0".into(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: "192.0.2.1".into(),
                port: 4000,
                object_key: key.to_vec(),
                components: Vec::new(),
            }],
        }
    }

    /// A NamingServer served on loopback.
    ///
    /// `Server` serves its connections concurrently, so nothing here has to
    /// take turns; most tests still use one client at a time because that is
    /// what they are testing, and
    /// [`Served::shutdown`] still takes the last client to keep each test's
    /// ordering explicit. The stop flag no longer needs a connection to
    /// arrive before it is noticed — the accept loop polls it.
    struct Served {
        root: Ior,
        stats: crate::server::ServerStats,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Served {
        fn start() -> Self {
            let server = Server::bind("127.0.0.1:0", b"NameService".to_vec()).unwrap();
            let port = server.local_addr().unwrap().port();
            let ns = Arc::new(NamingServer::new("127.0.0.1", port, b"NameService".to_vec()));
            let root = ns.root_ior();
            let stats = server.stats();
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let thread = std::thread::spawn(move || {
                server.serve_shared(&*ns, move || flag.load(Ordering::SeqCst)).unwrap();
            });
            Served { root, stats, stop, thread: Some(thread) }
        }

        fn client(&self) -> NamingContext {
            NamingContext::connect(&self.root, T).unwrap()
        }

        fn shutdown(mut self, last_client: NamingContext) {
            self.stop.store(true, Ordering::SeqCst);
            drop(last_client);
            self.thread.take().unwrap().join().unwrap();
        }
    }

    fn expect_not_found(err: Error, why: u32, rest: &[NameComponent], ctx: &str) {
        match err {
            Error::UserException { id, reply } => {
                assert_eq!(id, NOT_FOUND_ID, "{ctx}");
                // Decode the body exactly as a generated client would:
                // repository id, why, rest_of_name.
                let mut b = reply.body().unwrap();
                assert_eq!(b.get_string().unwrap(), NOT_FOUND_ID, "{ctx}");
                assert_eq!(b.get_u32().unwrap(), why, "{ctx}: wrong why");
                assert_eq!(read_name(&mut b).unwrap(), rest, "{ctx}: wrong rest_of_name");
            }
            other => panic!("{ctx}: expected NotFound, got {other:?}"),
        }
    }

    /// The exception shape, decoded by our own client — the first oracle —
    /// in both byte orders and in both GIOP header layouts (1.0 replies are
    /// not body-aligned, 1.2 replies are).
    #[test]
    fn not_found_carries_why_and_rest_as_the_client_decodes_them() {
        let served = Served::start();
        let mut setup = served.client();
        setup.bind_new_context(&[nc("a")]).unwrap();
        drop(setup);

        for version in [Version::V1_0, Version::V1_2] {
            for endian in [Endian::Big, Endian::Little] {
                let mut ctx = served.client();
                ctx.connection().cap_version(version);
                ctx.connection().set_endian(endian);
                let err = ctx.resolve(&[nc("a"), nc("x"), nc("y")]).unwrap_err();
                expect_not_found(
                    err,
                    WHY_MISSING_NODE,
                    &[nc("x"), nc("y")],
                    &format!("{version} {endian:?}"),
                );
                drop(ctx);
            }
        }
        let last = served.client();
        served.shutdown(last);
    }

    #[test]
    fn bind_refuses_a_taken_name_and_rebind_replaces_it() {
        let served = Served::start();
        let mut ctx = served.client();

        ctx.bind(&[nc("x")], &dummy(b"one")).unwrap();
        match ctx.bind(&[nc("x")], &dummy(b"two")) {
            Err(Error::UserException { id, .. }) => assert_eq!(id, ALREADY_BOUND_ID),
            other => panic!("expected AlreadyBound, got {other:?}"),
        }
        assert_eq!(ctx.resolve(&[nc("x")]).unwrap().primary().unwrap().object_key, b"one");

        ctx.rebind(&[nc("x")], &dummy(b"two")).unwrap();
        assert_eq!(ctx.resolve(&[nc("x")]).unwrap().primary().unwrap().object_key, b"two");
        served.shutdown(ctx);
    }

    /// rebind replaces objects only; a context under the name is `NotFound`
    /// with `why = not_object` (that is `rebind_context`'s job, not served).
    #[test]
    fn rebind_over_a_context_is_not_object() {
        let served = Served::start();
        let mut ctx = served.client();
        ctx.bind_new_context(&[nc("c")]).unwrap();
        let err = ctx.rebind(&[nc("c")], &dummy(b"o")).unwrap_err();
        expect_not_found(err, WHY_NOT_OBJECT, &[nc("c")], "rebind over context");
        served.shutdown(ctx);
    }

    #[test]
    fn unbind_removes_the_binding_and_a_second_unbind_is_missing_node() {
        let served = Served::start();
        let mut ctx = served.client();
        ctx.bind(&[nc("gone")], &dummy(b"g")).unwrap();
        ctx.unbind(&[nc("gone")]).unwrap();
        let err = ctx.resolve(&[nc("gone")]).unwrap_err();
        expect_not_found(err, WHY_MISSING_NODE, &[nc("gone")], "resolve after unbind");
        let err = ctx.unbind(&[nc("gone")]).unwrap_err();
        expect_not_found(err, WHY_MISSING_NODE, &[nc("gone")], "second unbind");
        served.shutdown(ctx);
    }

    /// Resolution through an *object* binding cannot continue: `not_context`,
    /// with `rest_of_name` starting at the offending component.
    #[test]
    fn resolving_through_an_object_is_not_context() {
        let served = Served::start();
        let mut ctx = served.client();
        ctx.bind(&[nc("leafy")], &dummy(b"l")).unwrap();
        let err = ctx.resolve(&[nc("leafy"), nc("below")]).unwrap_err();
        expect_not_found(err, WHY_NOT_CONTEXT, &[nc("leafy"), nc("below")], "through object");
        served.shutdown(ctx);
    }

    /// Nested contexts are distinct object keys on the same dispatch: the
    /// same leaf resolves through the root by full path and through the
    /// child context's own reference by the tail.
    #[test]
    fn nested_contexts_resolve_by_path_and_by_their_own_key() {
        let served = Served::start();
        let mut ctx = served.client();
        ctx.bind_new_context(&[nc("a")]).unwrap();
        ctx.bind_new_context(&[nc("a"), nc("b")]).unwrap();
        ctx.bind(&[nc("a"), nc("b"), nc("o")], &dummy(b"Leaf")).unwrap();

        let full = ctx.resolve(&[nc("a"), nc("b"), nc("o")]).unwrap();
        assert_eq!(full.primary().unwrap().object_key, b"Leaf");

        let child = ctx.resolve(&[nc("a")]).unwrap();
        assert_eq!(child.type_id, NAMING_CONTEXT_EXT_ID);
        assert_ne!(
            child.primary().unwrap().object_key,
            b"NameService",
            "a nested context must be a distinct object, not the root again"
        );

        // One connection at a time: the root client must hang up before the
        // child context's connection can be served.
        drop(ctx);
        let mut sub = NamingContext::connect(&child, T).unwrap();
        let tail = sub.resolve(&[nc("b"), nc("o")]).unwrap();
        assert_eq!(tail.primary().unwrap().object_key, b"Leaf");
        served.shutdown(sub);
    }

    /// The honest stub: at most `how_many` bindings, a nil iterator always,
    /// deterministic order, correct binding types.
    #[test]
    fn list_truncates_to_how_many_and_returns_a_nil_iterator() {
        let served = Served::start();
        let mut ctx = served.client();
        ctx.bind(&[nc("obj")], &dummy(b"o")).unwrap();
        ctx.bind_new_context(&[nc("sub")]).unwrap();
        ctx.bind(&[NameComponent { id: "kinded".into(), kind: "dev".into() }], &dummy(b"k"))
            .unwrap();

        let (page, it) = ctx.list(2).unwrap();
        assert_eq!(page.len(), 2, "truncated to how_many");
        assert!(it.is_nil(), "the stub returns a nil iterator even when it truncates");

        let (all, it) = ctx.list(100).unwrap();
        assert!(it.is_nil());
        let described: Vec<(String, String, bool)> = all
            .iter()
            .map(|b| (b.name[0].id.clone(), b.name[0].kind.clone(), b.is_context))
            .collect();
        assert_eq!(
            described,
            vec![
                ("kinded".into(), "dev".into(), false),
                ("obj".into(), String::new(), false),
                ("sub".into(), String::new(), true),
            ],
            "BTreeMap order, kinds preserved, ncontext flagged"
        );

        let (none, it) = ctx.list(0).unwrap();
        assert!(none.is_empty() && it.is_nil());
        served.shutdown(ctx);
    }

    /// The NamingContextExt surface: resolve_str shares the URL grammar the
    /// client parses, and to_name/to_string are its two directions.
    #[test]
    fn the_ext_string_surface_matches_the_shared_grammar() {
        let served = Served::start();
        let mut ctx = served.client();
        ctx.bind_new_context(&[nc("ctx")]).unwrap();
        let kinded = NameComponent { id: "obj".into(), kind: "dev".into() };
        ctx.bind(&[nc("ctx"), kinded.clone()], &dummy(b"K")).unwrap();

        let ior = ctx.resolve_str("ctx/obj.dev").unwrap();
        assert_eq!(ior.primary().unwrap().object_key, b"K");

        let reply = ctx.connection().invoke("to_name", |e| e.put_str("ctx/obj.dev")).unwrap();
        let mut b = reply.body().unwrap();
        assert_eq!(read_name(&mut b).unwrap(), vec![nc("ctx"), kinded.clone()]);

        let name = vec![nc("ctx"), kinded];
        let reply = ctx.connection().invoke("to_string", |e| write_name(e, &name)).unwrap();
        assert_eq!(reply.body().unwrap().get_string().unwrap(), "ctx/obj.dev");
        served.shutdown(ctx);
    }

    /// The operation the coverage sweep found absent, over the wire, closing
    /// the loop the client half left open: the server produces the URL and
    /// **our own parser reads back the name that went in** — including the
    /// components that need both escape layers.
    #[test]
    fn to_url_over_the_wire_round_trips_through_the_client_parser() {
        let served = Served::start();
        let mut ctx = served.client();
        let cases = [
            vec![nc("spike"), nc("Echo")],
            vec![NameComponent { id: "a/b".into(), kind: "c.d".into() }],
            vec![NameComponent { id: "with space".into(), kind: "함정".into() }],
            vec![NameComponent { id: "100%".into(), kind: "#frag".into() }],
        ];
        for name in &cases {
            let sn = crate::naming::stringify_name(name);
            let url = ctx.to_url("iiop:1.2@127.0.0.1:4001", &sn).unwrap();
            assert!(url.starts_with("corbaname:iiop:1.2@127.0.0.1:4001#"), "{url}");
            match crate::naming::ObjectUrl::parse(&url) {
                Ok(crate::naming::ObjectUrl::Corbaname { name: back, addresses, object_key }) => {
                    assert_eq!(&back, name, "{url}");
                    assert_eq!(addresses[0].port, 4001);
                    assert_eq!(object_key, b"NameService", "no key means NameService");
                }
                other => panic!("{url} parsed as {other:?}"),
            }
        }
        served.shutdown(ctx);
    }

    /// The two exceptions `to_url` declares, told apart by which argument was
    /// wrong — the distinction is the whole reason `InvalidAddress` exists as
    /// a separate exception rather than as another `InvalidName`.
    #[test]
    fn to_url_raises_invalid_address_and_invalid_name_separately() {
        let served = Served::start();
        let mut ctx = served.client();
        for (address, name, want) in [
            ("no-protocol-token", "a", INVALID_ADDRESS_ID),
            ("", "a", INVALID_ADDRESS_ID),
            // Measured divergence from omniNames, reasoned in `naming::to_url`.
            ("rir:", "a", INVALID_ADDRESS_ID),
            (":h", "trailing\\", INVALID_NAME_ID),
        ] {
            match ctx.to_url(address, name) {
                Err(Error::UserException { id, .. }) => {
                    assert_eq!(id, want, "to_url({address:?}, {name:?})");
                }
                other => panic!("to_url({address:?}, {name:?}) gave {other:?}"),
            }
        }
        served.shutdown(ctx);
    }

    /// The URL `to_url` hands out is one a client can act on: resolving the
    /// name it names, through this same server, returns the bound reference.
    /// (Same process, so it is our client both times — the cross-ORB half is
    /// `spike-names --hold` plus the omniORB snippet in its header.)
    #[test]
    fn a_url_from_to_url_resolves_against_the_server_that_made_it() {
        let served = Served::start();
        let mut ctx = served.client();
        ctx.bind_new_context(&[nc("spike")]).unwrap();
        ctx.bind(
            &[nc("spike"), NameComponent { id: "Echo.1".into(), kind: String::new() }],
            &dummy(b"Echo"),
        )
        .unwrap();

        let addr = served.root.primary().unwrap();
        let url = ctx
            .to_url(
                &format!("iiop:1.2@{}:{}", addr.host, addr.port),
                &crate::naming::stringify_name(&[
                    nc("spike"),
                    NameComponent { id: "Echo.1".into(), kind: String::new() },
                ]),
            )
            .unwrap();
        drop(ctx);

        let parsed = crate::naming::ObjectUrl::parse(&url).unwrap();
        let crate::naming::ObjectUrl::Corbaname { name, .. } = &parsed else {
            panic!("{url} is not a corbaname URL")
        };
        let name = name.clone();
        let mut through = NamingContext::from_url(&parsed, T).unwrap();
        assert_eq!(through.resolve(&name).unwrap().primary().unwrap().object_key, b"Echo");
        served.shutdown(through);
    }

    #[test]
    fn empty_names_are_invalid_name() {
        let served = Served::start();
        let mut ctx = served.client();
        for err in [ctx.resolve(&[]).unwrap_err(), ctx.resolve_str("").unwrap_err()] {
            match err {
                Error::UserException { id, .. } => assert_eq!(id, INVALID_NAME_ID),
                other => panic!("expected InvalidName, got {other:?}"),
            }
        }
        served.shutdown(ctx);
    }

    /// Every ORB probes with `_is_a` before trusting a narrow; both naming
    /// interface ids must answer true. An operation outside the served
    /// surface is `BAD_OPERATION`, never a silent empty reply.
    #[test]
    fn is_a_answers_for_both_interfaces_and_unserved_ops_are_bad_operation() {
        let served = Served::start();
        let mut ctx = served.client();
        for (id, expected) in [
            (NAMING_CONTEXT_EXT_ID, true),
            (NAMING_CONTEXT_ID, true),
            ("IDL:spike/Echo:1.0", false),
        ] {
            let reply = ctx.connection().invoke("_is_a", move |e| e.put_str(id)).unwrap();
            assert_eq!(reply.body().unwrap().get_bool().unwrap(), expected, "{id}");
        }
        match ctx.connection().invoke_nullary("destroy") {
            Err(Error::SystemException { id, .. }) => {
                assert_eq!(id, crate::server::BAD_OPERATION);
            }
            other => panic!("expected BAD_OPERATION for destroy, got {other:?}"),
        }
        served.shutdown(ctx);
    }

    /// The limit this servant's docs used to name, gone: several clients hold
    /// naming sessions at the same time and every one of them is answered.
    ///
    /// Each client binds its own name while the others are connected, then
    /// waits at a deadline-bounded rendezvous before resolving what its
    /// neighbours bound — so a server that served one connection at a time
    /// would fail here on the deadline rather than hang. The binds are made
    /// safe by the tree's own `RwLock` — the servant's, not the server's,
    /// since stream E's second batch — which is what
    /// `concurrent_resolvers_overlap_with_a_writer_without_tearing_the_tree`
    /// exercises from the other side.
    #[test]
    fn concurrent_clients_bind_and_resolve_without_taking_turns() {
        const N: usize = 5;
        let served = Served::start();
        let arrived = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for i in 0..N {
                let served = &served;
                let arrived = &arrived;
                scope.spawn(move || {
                    let mut ctx = served.client();
                    let mine = format!("client{i}");
                    ctx.bind(&[nc(&mine)], &dummy(mine.as_bytes())).unwrap();

                    // Everybody is connected and bound before anybody reads.
                    arrived.fetch_add(1, Ordering::SeqCst);
                    let deadline = std::time::Instant::now() + T;
                    while arrived.load(Ordering::SeqCst) < N {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "client {i} waited out the others: the server is still serializing"
                        );
                        std::thread::sleep(Duration::from_millis(2));
                    }

                    for other in 0..N {
                        let name = format!("client{other}");
                        let got = ctx.resolve(&[nc(&name)]).unwrap();
                        assert_eq!(
                            got.primary().unwrap().object_key,
                            name.as_bytes(),
                            "client {i} resolving {name}"
                        );
                    }
                });
            }
        });
        assert!(
            served.stats.peak_active() >= N as u64,
            "the clients did not actually overlap: peak was {}",
            served.stats.peak_active()
        );
        let last = served.client();
        served.shutdown(last);
    }

    /// Readers overlap; a writer excludes them; and the tree is consistent
    /// afterwards either way.
    ///
    /// The naming service is read-dominated, which is why its lock is an
    /// `RwLock` — so this is the test that says the read half is real. N
    /// clients resolve the same name repeatedly while one client rebinds it
    /// underneath them, and every resolve must return **one of the two
    /// bindings**, never a torn one and never a failure. Then the writer's
    /// last value is what everybody sees.
    ///
    /// The deadline lives in the client sockets and in the join: a lock
    /// inversion here would show up as a test that fails on a read timeout,
    /// not as one that hangs.
    #[test]
    fn concurrent_resolvers_overlap_with_a_writer_without_tearing_the_tree() {
        const N: usize = 5;
        const EACH: usize = 20;
        let served = Served::start();
        let mut writer = served.client();
        let first = dummy(b"first");
        let second = dummy(b"second");
        writer.bind(&[nc("target")], &first).unwrap();

        std::thread::scope(|scope| {
            for i in 0..N {
                let served = &served;
                let (first, second) = (&first, &second);
                scope.spawn(move || {
                    let mut c = served.client();
                    for _ in 0..EACH {
                        let got = c.resolve(&[nc("target")]).unwrap();
                        let key = got.primary().unwrap().object_key.clone();
                        assert!(
                            key == first.primary().unwrap().object_key
                                || key == second.primary().unwrap().object_key,
                            "reader {i} saw a binding that was never written: {key:?}"
                        );
                    }
                });
            }
            // The writer runs the whole time the readers do.
            for _ in 0..EACH {
                writer.rebind(&[nc("target")], &second).unwrap();
                writer.rebind(&[nc("target")], &first).unwrap();
            }
            writer.rebind(&[nc("target")], &second).unwrap();
        });

        let settled = writer.resolve(&[nc("target")]).unwrap();
        assert_eq!(
            settled.primary().unwrap().object_key,
            second.primary().unwrap().object_key,
            "the last write must be what the tree settled on"
        );
        assert!(
            served.stats.peak_active() >= N as u64,
            "the readers never overlapped: peak was {}",
            served.stats.peak_active()
        );
        served.shutdown(writer);
    }

    /// The narrow `dispatch` entry point cannot carry a user exception, so it
    /// maps one to the standard UNKNOWN. Direct call — no server involved.
    #[test]
    fn a_user_exception_through_plain_dispatch_maps_to_unknown() {
        let wire = crate::encode_request(
            Version::V1_2,
            Endian::Little,
            7,
            b"NameService",
            "resolve",
            true,
            |e| write_name(e, &[nc("missing")]),
        )
        .unwrap();
        let msg = crate::read_message(&mut &wire[..], DEFAULT_MAX_MESSAGE_SIZE).unwrap();
        let req = crate::server::decode_request(msg).unwrap();

        let ns = NamingServer::new("127.0.0.1", 1, b"NameService".to_vec());
        let mut out = Encoder::new(Endian::Little);
        let err = ns.dispatch(&req, &mut out).unwrap_err();
        assert_eq!(err.id, crate::server::UNKNOWN);
        assert_eq!(err.minor, 0x4f4d_0001, "OMGVMCID | 1: unlisted user exception");
    }
}
