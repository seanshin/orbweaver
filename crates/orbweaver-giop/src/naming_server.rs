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
//! `to_name`, `to_string`) — plus `_is_a`/`_non_existent`, which every ORB
//! probes with before trusting a narrow. Everything else answers
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
//! [`NamingContext::bind_new_context`]: crate::naming::NamingContext::bind_new_context
//! [`Server`]: crate::server::Server

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use orbweaver_cdr::Encoder;

use crate::naming::{
    NAMING_CONTEXT_EXT_ID, NameComponent, parse_stringified_name, read_name, stringify_name,
    write_name,
};
use crate::server::{Dispatch, DispatchBody, Request, SystemException};
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

/// An in-memory CosNaming servant behind [`Server`].
///
/// One instance serves a whole context *tree*: the root context is the
/// object key the [`Server`] was bound with, and every context minted by
/// `bind_new_context`/`new_context` is a further key on the same dispatch —
/// which is why [`Dispatch::knows`] is answered from the context table
/// rather than defaulted.
///
/// `host` and `port` are what go into minted context references, and are the
/// caller's to publish correctly (Phase 0 assumption D: the bind address and
/// the publishable address differ behind NAT).
///
/// [`Server`]: crate::server::Server
#[derive(Debug)]
pub struct NamingServer {
    host: String,
    port: u16,
    root: Vec<u8>,
    contexts: BTreeMap<Vec<u8>, Bindings>,
    minted: u64,
}

impl NamingServer {
    /// A naming server rooted at `root_key`, minting references that point
    /// at `host:port`.
    pub fn new(host: impl Into<String>, port: u16, root_key: Vec<u8>) -> Self {
        let mut contexts = BTreeMap::new();
        contexts.insert(root_key.clone(), Bindings::new());
        Self { host: host.into(), port, root: root_key, contexts, minted: 0 }
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

    /// Creates an unbound context and returns its fresh object key.
    fn mint_context(&mut self) -> Vec<u8> {
        self.minted += 1;
        let mut key = self.root.clone();
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

    fn resolve_from(&self, start: &[u8], name: &[NameComponent]) -> Result<Ior, Raise> {
        let (ctx, last) = self.walk(start, name)?;
        match self.table(&ctx)?.get(&slot(&last)) {
            None => Err(UserExc::NotFound { why: WHY_MISSING_NODE, rest: vec![last] }.into()),
            Some(Bound::Object(ior)) => Ok(ior.clone()),
            Some(Bound::Context(k)) => Ok(self.ior_for(k)),
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
        start: &[u8],
        name: &[NameComponent],
    ) -> Result<Ior, Raise> {
        // Occupancy is checked before minting, or a failed bind would leak an
        // unreachable context.
        let (ctx, last) = self.walk(start, name)?;
        if self.table(&ctx)?.contains_key(&slot(&last)) {
            return Err(UserExc::AlreadyBound.into());
        }
        let key = self.mint_context();
        self.table_mut(&ctx)?.insert(slot(&last), Bound::Context(key.clone()));
        Ok(self.ior_for(&key))
    }

    /// Dispatches one operation, writing the result body into `out`.
    ///
    /// Invariant every arm keeps: nothing is written into `out` until the
    /// operation can no longer raise a *user* exception, because the buffer
    /// travels whole under a single reply status. (A system exception after
    /// a partial write is fine — the server discards `out` on that path.)
    fn handle(&mut self, req: &Request, out: &mut Encoder) -> Result<(), Raise> {
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
                let ior = self.resolve_from(&req.object_key, &name)?;
                ior.write_to(out).map_err(|_| marshal())?;
            }
            "resolve_str" => {
                let s = args.get_string().map_err(|_| marshal())?;
                let name = parse_stringified_name(&s).map_err(|_| UserExc::InvalidName)?;
                let ior = self.resolve_from(&req.object_key, &name)?;
                ior.write_to(out).map_err(|_| marshal())?;
            }
            "bind" | "rebind" => {
                let name = read_name(&mut args).map_err(|_| marshal())?;
                let obj = Ior::read_from(&mut args).map_err(|_| marshal())?;
                let overwrite = req.operation == "rebind";
                self.bind_from(&req.object_key, &name, Bound::Object(obj), overwrite)?;
            }
            "unbind" => {
                let name = read_name(&mut args).map_err(|_| marshal())?;
                self.unbind_from(&req.object_key, &name)?;
            }
            "bind_new_context" => {
                let name = read_name(&mut args).map_err(|_| marshal())?;
                let ior = self.bind_new_context_from(&req.object_key, &name)?;
                ior.write_to(out).map_err(|_| marshal())?;
            }
            "new_context" => {
                let key = self.mint_context();
                self.ior_for(&key).write_to(out).map_err(|_| marshal())?;
            }
            "list" => {
                let how_many = args.get_u32().map_err(|_| marshal())?;
                let table = self.table(&req.object_key)?;
                let take = (how_many as usize).min(table.len());
                out.put_u32(take as u32);
                for ((id, kind), bound) in table.iter().take(take) {
                    write_name(out, &[NameComponent { id: id.clone(), kind: kind.clone() }]);
                    out.put_u32(match bound {
                        Bound::Object(_) => BINDING_OBJECT,
                        Bound::Context(_) => BINDING_CONTEXT,
                    });
                }
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
            _ => return Err(SystemException::bad_operation().into()),
        }
        Ok(())
    }
}

impl Dispatch for NamingServer {
    /// One dispatch answers for the whole context tree: the root key and
    /// every key `bind_new_context`/`new_context` minted.
    fn knows(&self, object_key: &[u8]) -> bool {
        self.contexts.contains_key(object_key)
    }

    fn dispatch_body(
        &mut self,
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
    /// never takes this path — it calls [`Dispatch::dispatch_body`] — but
    /// the trait requires the method and lying with `NO_EXCEPTION` would be
    /// worse.
    fn dispatch(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> std::result::Result<(), SystemException> {
        match self.dispatch_body(request, out)? {
            DispatchBody::Return => Ok(()),
            DispatchBody::UserException => Err(SystemException::unknown_user_exception()),
        }
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

    /// A NamingServer served on loopback. `Server` handles one connection at
    /// a time, so tests use clients strictly sequentially and must call
    /// [`Served::shutdown`] with the *last* client still open: the stop flag
    /// is raised before that client drops, so the serve loop is guaranteed
    /// to observe it after the connection ends instead of blocking in accept.
    struct Served {
        root: Ior,
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Served {
        fn start() -> Self {
            let server = Server::bind("127.0.0.1:0", b"NameService".to_vec()).unwrap();
            let port = server.local_addr().unwrap().port();
            let mut ns = NamingServer::new("127.0.0.1", port, b"NameService".to_vec());
            let root = ns.root_ior();
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let thread = std::thread::spawn(move || {
                server.serve(&mut ns, || flag.load(Ordering::SeqCst)).unwrap();
            });
            Served { root, stop, thread: Some(thread) }
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

        let mut ns = NamingServer::new("127.0.0.1", 1, b"NameService".to_vec());
        let mut out = Encoder::new(Endian::Little);
        let err = ns.dispatch(&req, &mut out).unwrap_err();
        assert_eq!(err.id, crate::server::UNKNOWN);
        assert_eq!(err.minor, 0x4f4d_0001, "OMGVMCID | 1: unlisted user exception");
    }
}
