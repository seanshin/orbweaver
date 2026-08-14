//! The runtime generated code marshals through.
//!
//! Everything a stub emits is a call into this module, so the wire knowledge
//! lives here exactly once — the lesson Phase 3 paid for when `wstring` was
//! re-implemented instead of reused. A generated file contains **names and
//! order**, never encoding rules.

use orbweaver_giop::codeset::{CodeSetId, WideCodec};

pub use orbweaver_cdr::{Decoder, Encoder, Endian};
pub use orbweaver_giop::server::{
    BAD_OPERATION, Completion, Dispatch, DispatchBody, MARSHAL, OBJECT_NOT_EXIST, Request, Server,
    SystemException, UNKNOWN,
};
pub use orbweaver_giop::{
    Connection, Error as GiopError, IiopProfile, Invoker, Ior, Reply, Version,
};

/// Repository id every CORBA object answers `_is_a` to.
///
/// A generated skeleton answers `_is_a` from the registry's inheritance chain
/// plus this; an ORB probes with it before it will narrow, so a skeleton that
/// does not know it is one that cannot be narrowed to.
pub const OBJECT_ID: &str = "IDL:omg.org/CORBA/Object:1.0";

/// Repository id: the servant refused the call. Not "try again later" — a
/// refusal that a retry cannot change.
pub const NO_PERMISSION: &str = "IDL:omg.org/CORBA/NO_PERMISSION:1.0";
/// Repository id: an argument was outside what the operation accepts, where
/// the contract has no `raises` clause to say so more precisely.
pub const BAD_PARAM: &str = "IDL:omg.org/CORBA/BAD_PARAM:1.0";
/// Repository id: refused *now*. §9 makes this the one refusal that invites a
/// retry, which is why it is worth distinguishing from `NO_PERMISSION`.
pub const TRANSIENT: &str = "IDL:omg.org/CORBA/TRANSIENT:1.0";
/// Repository id: the call is legal but not in this state — evicting what was
/// never loaded, using a session that was closed.
pub const BAD_INV_ORDER: &str = "IDL:omg.org/CORBA/BAD_INV_ORDER:1.0";
/// Repository id: the operation exists in the contract and this servant does
/// not implement it. Distinct from `BAD_OPERATION`, which means the name is
/// not in the contract at all.
pub const NO_IMPLEMENT: &str = "IDL:omg.org/CORBA/NO_IMPLEMENT:1.0";
/// Repository id: the servant broke. The honest answer when the failure is
/// ours and the caller can do nothing about it.
pub const INTERNAL: &str = "IDL:omg.org/CORBA/INTERNAL:1.0";

/// OMG's vendor minor code space (§4.11.3). A minor code in it means the same
/// thing to every ORB; a minor code outside it means whatever its vendor says.
pub const OMG_VMCID: u32 = 0x4f4d_0000;

/// A system exception under construction: it knows what went wrong and still
/// needs the one thing only the servant knows.
///
/// # Why this is not a constructor
///
/// A `SystemException` carries a completion status (§9.4.3.2), and it is the
/// field a client's retry logic reads: `COMPLETED_NO` says the operation did
/// not run and re-sending is safe, `COMPLETED_MAYBE` says nobody can tell.
/// Whether a refusal happened before or after the state changed is knowledge
/// the *servant* has and the generator does not, so there is no default that
/// is right — a generator-chosen `COMPLETED_NO` on a raise that fired halfway
/// through a mutation is how a well-behaved retry loop corrupts state.
///
/// So this type exists and `SystemException` is not reachable from it without
/// naming the status: [`Raising::did_not_run`], [`Raising::ran_to_completion`]
/// or [`Raising::may_have_run`]. There is no `From`, no `Default`, and the
/// `#[must_use]` makes a forgotten one a warning rather than a silent
/// discard.
///
/// ```text
/// return Err(rt::raise::no_permission().did_not_run().into());
/// return Err(rt::raise::bad_param().omg_minor(3).did_not_run().into());
/// ```
///
/// # What a foreign ORB currently reads
///
/// Measured, not assumed, and currently wrong one layer down. §4.11.4 declares
/// `enum completion_status { COMPLETED_YES, COMPLETED_NO, COMPLETED_MAYBE }`,
/// so `COMPLETED_YES` is ordinal 0. [`Completion`] numbers it `No = 0,
/// Yes = 1`, which transposes the first two, and omniORB's Python client reads
/// a `did_not_run()` raise from a generated skeleton as `CORBA.COMPLETED_YES`
/// — the exact inversion of "safe to re-send". `MAYBE` is 2 either way.
///
/// The defect is in `orbweaver-giop`, not here, and is left to that crate's
/// owner rather than fixed in passing; `tests/servant_faults.rs` pins the
/// behaviour as measured so the fix cannot land silently. Until then a
/// servant's *choice* is carried faithfully by everything in this crate, and
/// two of its three values are renumbered on the way out.
#[derive(Debug, Clone)]
#[must_use = "a raise is not a system exception until its completion status is stated; \
              call .did_not_run(), .ran_to_completion() or .may_have_run()"]
pub struct Raising {
    id: String,
    minor: u32,
}

impl Raising {
    /// Attaches a vendor minor code.
    pub fn minor(mut self, minor: u32) -> Self {
        self.minor = minor;
        self
    }

    /// Attaches a minor code in OMG's own space ([`OMG_VMCID`]), which is the
    /// half of the code that makes it portable between ORBs.
    pub fn omg_minor(self, minor: u16) -> Self {
        let m = OMG_VMCID | u32::from(minor);
        self.minor(m)
    }

    /// `COMPLETED_NO`: the operation did not run. Nothing was touched and the
    /// client may re-send this call somewhere else without repeating an
    /// effect. Correct for a refusal decided before any state changed.
    pub fn did_not_run(self) -> SystemException {
        self.with_completion(Completion::No)
    }

    /// `COMPLETED_YES`: the operation ran to completion and only the answer is
    /// lost. A retry would repeat whatever it did.
    pub fn ran_to_completion(self) -> SystemException {
        self.with_completion(Completion::Yes)
    }

    /// `COMPLETED_MAYBE`: it cannot be determined. The right answer whenever a
    /// failure lands in the middle of a mutation — it is worse for the client
    /// to be told "safe to retry" wrongly than to be told nobody knows.
    pub fn may_have_run(self) -> SystemException {
        self.with_completion(Completion::Maybe)
    }

    /// The same decision made from a value, for a servant whose completion
    /// status is itself computed.
    pub fn with_completion(self, completed: Completion) -> SystemException {
        SystemException { id: self.id, minor: self.minor, completed }
    }
}

/// The system exceptions a servant raises.
///
/// A `raises` clause gives an interface its *user* exceptions; this is the
/// other half, the vocabulary every servant needs and no contract declares —
/// an unknown key is `OBJECT_NOT_EXIST`, a refused call is `NO_PERMISSION`, a
/// temporary refusal is `TRANSIENT`. Each returns a [`Raising`], which becomes
/// a [`SystemException`] only once the completion status is stated; see that
/// type for why there is no default.
pub mod raise {
    use super::{Raising, SystemException};

    fn at(id: &str) -> Raising {
        Raising { id: id.to_owned(), minor: 0 }
    }

    /// The object key names nothing this servant holds.
    pub fn object_not_exist() -> Raising {
        at(super::OBJECT_NOT_EXIST)
    }

    /// The caller may not make this call, and retrying will not change that.
    pub fn no_permission() -> Raising {
        at(super::NO_PERMISSION)
    }

    /// An argument is outside what the operation accepts. Usually worth a
    /// minor code, since the caller cannot see which argument otherwise.
    pub fn bad_param() -> Raising {
        at(super::BAD_PARAM)
    }

    /// Refused now; a retry may well succeed.
    pub fn transient() -> Raising {
        at(super::TRANSIENT)
    }

    /// Legal call, wrong state.
    pub fn bad_inv_order() -> Raising {
        at(super::BAD_INV_ORDER)
    }

    /// In the contract, not in this servant.
    pub fn no_implement() -> Raising {
        at(super::NO_IMPLEMENT)
    }

    /// The servant broke, and the caller can do nothing about it.
    pub fn internal() -> Raising {
        at(super::INTERNAL)
    }

    /// Any other standard system exception, by repository id.
    ///
    /// The set above is what the servants in this workspace actually raise,
    /// not the whole of §4.11; this is the door for the rest of it rather
    /// than a reason to grow the list to thirty constructors.
    pub fn other(id: impl Into<String>) -> Raising {
        Raising { id: id.into(), minor: 0 }
    }

    /// The standard mapping for a user exception reaching a caller that cannot
    /// carry one: `UNKNOWN` with OMG minor 1, completion `YES` because the
    /// operation did run — it raised.
    pub fn unknown_user_exception() -> SystemException {
        SystemException::unknown_user_exception()
    }
}

/// Reports a fault a `oneway` had nowhere to put.
///
/// §9.4.1: a request with `response_expected` false gets no reply *at all*, so
/// a servant's `Err` on a oneway has no message to travel in. Dropping it is
/// what the specification requires; dropping it **silently** is not, and a
/// server whose oneway operations fail invisibly is a server nobody can debug.
/// The generated oneway arm therefore calls this instead of `let _ =`, which
/// is the difference between a decision and an omission.
pub fn oneway_fault_dropped(interface: &str, operation: &str, fault: &dyn std::fmt::Debug) {
    eprintln!(
        "orbweaver: {interface}::{operation} is oneway (§9.4.1): no reply may be sent, so the \
         servant's {fault:?} was dropped"
    );
}

/// Where a generated skeleton's objects live, and the root of their key space.
///
/// # Why this is in the runtime and not in the generated file
///
/// Five hand-written servants in this workspace mint references, and all five
/// contain the *same* function under different names — `naming_server::ior_for`,
/// `tenant_service::ior_for`, `expert_service::ior_for`, `ifr::ior_for` — each
/// assembling `Ior { type_id, profiles: vec![IiopProfile { version, host, port,
/// object_key, components }] }` by hand. That is a wire decision (§9.3.6 and
/// §7.6.2: which profile, which GIOP version goes in it), and the rule this
/// crate is built on is that wire decisions exist once. A code generator is a
/// machine for duplicating things, so emitting a sixth copy per interface would
/// be the worst place of all to put it.
///
/// # Host and port are not the bind address
///
/// Phase 0 assumption D: behind NAT or in a container, the address a server
/// binds and the address a client can dial differ, and publishing the bind
/// address is a service nobody can reach. `host`/`port` are therefore what goes
/// *into* the profile and are the caller's to get right; [`ObjectHome::of`] is
/// the convenience for the common case where they are the same, and it takes
/// `host` separately even so.
///
/// # The key scheme
///
/// One root key, plus one derived key per object:
///
/// ```text
/// root                          the default object — oid ""
/// root ++ <infix> ++ <oid>      the object named <oid>, oid in UTF-8
/// ```
///
/// The derivation is **reversible**, which is the point: the server holds no
/// per-reference table, so a reference a client stored yesterday still resolves
/// after a restart, and a process cannot be made to grow by looking things up.
/// [`ObjectHome::oid_of`] recovers the oid by stripping a fixed prefix, so the
/// oid is the *whole* remainder and may contain the infix, `/`, `:` or anything
/// else UTF-8 can carry — there is nothing to escape and nothing to be
/// ambiguous about. `ifr.rs` derives its keys exactly this way and says the
/// same thing about why.
///
/// Two interfaces served from one root are separated by their infixes, which a
/// generated skeleton takes from the interface name. Because the infix is
/// matched at a fixed offset, no oid under one infix can be read as a key under
/// another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectHome {
    host: String,
    port: u16,
    root: Vec<u8>,
}

impl ObjectHome {
    /// A home publishing at `host:port`, rooted at `root_key`.
    ///
    /// `root_key` must be the key the [`Server`] was bound with, or the
    /// references minted here address an object that server will not accept.
    pub fn new(host: impl Into<String>, port: u16, root_key: Vec<u8>) -> Self {
        Self { host: host.into(), port, root: root_key }
    }

    /// The home for a bound [`Server`], publishing under `host`.
    ///
    /// Reads the port back from the listener, which is the only way to get it
    /// right after a port-zero bind, and takes the root key from the server so
    /// the two cannot disagree.
    pub fn of(server: &Server, host: impl Into<String>) -> Result<Self, GiopError> {
        Ok(Self {
            host: host.into(),
            port: server.local_addr()?.port(),
            root: server.object_key().to_vec(),
        })
    }

    /// The host that goes into minted profiles.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The port that goes into minted profiles.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The root object key: the default object's key, and the prefix of every
    /// other.
    pub fn root_key(&self) -> &[u8] {
        &self.root
    }

    /// The object key for `oid` under `infix`. The empty oid is the root key.
    pub fn key_of(&self, infix: &str, oid: &str) -> Vec<u8> {
        if oid.is_empty() {
            return self.root.clone();
        }
        let mut key = self.root.clone();
        key.extend_from_slice(infix.as_bytes());
        key.extend_from_slice(oid.as_bytes());
        key
    }

    /// The oid a key addresses, or `None` if this home did not derive it.
    ///
    /// Borrowed from the key rather than copied: the caller already owns the
    /// bytes, and a servant answering per call should not allocate to learn
    /// who it is.
    pub fn oid_of<'a>(&self, infix: &str, key: &'a [u8]) -> Option<&'a str> {
        if key == self.root.as_slice() {
            return Some("");
        }
        let rest = key.strip_prefix(self.root.as_slice())?;
        let rest = rest.strip_prefix(infix.as_bytes())?;
        std::str::from_utf8(rest).ok()
    }

    /// A reference to `key`, advertising `type_id`.
    pub fn ior(&self, type_id: &str, key: Vec<u8>) -> Ior {
        Ior {
            type_id: type_id.to_owned(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: self.host.clone(),
                port: self.port,
                object_key: key,
                components: Vec::new(),
            }],
        }
    }

    /// The same reference as [`ObjectHome::ior`], in the form generated code
    /// marshals.
    pub fn reference(&self, type_id: &str, key: Vec<u8>) -> ObjRef {
        ObjRef(Some(self.ior(type_id, key)))
    }

    /// The nil reference (§9.3.6): empty type id, no profiles.
    ///
    /// A truthful "there is no such object", and distinct from an absent
    /// field — it still occupies its place in the reply body.
    pub fn nil() -> ObjRef {
        ObjRef(None)
    }
}

/// Several [`Dispatch`]es behind one [`Server`], chosen by object key.
///
/// A generated skeleton serves one interface. A process usually serves more
/// than one: `ifr.rs` answers as `Repository` under its root key and as
/// `InterfaceDef` under every derived key, and `tenant_service.rs` answers as
/// five different interfaces depending on the key's shape. Without something
/// like this, "the generator can serve many objects" would still mean "of one
/// interface", and every multi-interface service would stay hand-written.
///
/// The routing rule is [`Dispatch::knows`], asked in insertion order, first
/// match wins. That makes `knows` load-bearing rather than advisory, which is
/// why a generated servant must implement it and cannot inherit a `true`.
/// Overlapping members are a configuration mistake this type cannot detect —
/// it can only be predictable about which one wins.
#[derive(Default)]
pub struct Servants {
    entries: Vec<Box<dyn Dispatch + Send>>,
}

impl std::fmt::Debug for Servants {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Servants").field("len", &self.entries.len()).finish()
    }
}

impl Servants {
    /// An empty multiplexer, which knows no key at all.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a dispatcher, after every one already added.
    pub fn with(mut self, dispatch: impl Dispatch + Send + 'static) -> Self {
        self.push(dispatch);
        self
    }

    /// Adds a dispatcher, after every one already added.
    pub fn push(&mut self, dispatch: impl Dispatch + Send + 'static) {
        self.entries.push(Box::new(dispatch));
    }

    /// How many dispatchers are behind this one.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no dispatcher has been added, in which case every key is
    /// `OBJECT_NOT_EXIST`.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The index of the first member that knows `object_key`.
    ///
    /// An index rather than a borrow: returning `&mut dyn Dispatch` out of the
    /// search makes the borrow live for the whole function, and the caller
    /// still needs `self` afterwards.
    fn route(&self, object_key: &[u8]) -> Option<usize> {
        self.entries.iter().position(|d| d.knows(object_key))
    }
}

impl Dispatch for Servants {
    fn knows(&self, object_key: &[u8]) -> bool {
        self.entries.iter().any(|d| d.knows(object_key))
    }

    fn forward(&mut self, request: &Request) -> Option<Ior> {
        let at = self.route(&request.object_key)?;
        self.entries[at].forward(request)
    }

    fn dispatch_body(
        &mut self,
        request: &Request,
        out: &mut Encoder,
    ) -> Result<DispatchBody, SystemException> {
        match self.route(&request.object_key) {
            Some(at) => self.entries[at].dispatch_body(request, out),
            // `Server` asked `knows` first, so this is only reachable if a
            // member changed its mind between the two calls. Answering the
            // same thing the server would have is the honest recovery.
            None => Err(SystemException::object_not_exist()),
        }
    }

    fn dispatch(&mut self, request: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        match self.dispatch_body(request, out)? {
            DispatchBody::Return => Ok(()),
            DispatchBody::UserException => Err(SystemException::unknown_user_exception()),
        }
    }
}

/// The marshalling contract every generated type implements.
///
/// The error is the GIOP error rather than the CDR one because two of the
/// types below (`AnyVal`, `ObjRef`) legitimately fail above the CDR layer, and
/// a second error type on the trait would push a conversion into every
/// generated member line.
pub trait Cdr: Sized {
    /// Writes `self` at the encoder's current position.
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError>;
    /// Reads one value.
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError>;
}

macro_rules! prim {
    ($t:ty, $put:ident, $get:ident) => {
        impl Cdr for $t {
            fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
                e.$put(*self);
                Ok(())
            }
            fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
                Ok(d.$get()?)
            }
        }
    };
}

prim!(bool, put_bool, get_bool);
prim!(u8, put_u8, get_u8);
prim!(i16, put_i16, get_i16);
prim!(u16, put_u16, get_u16);
prim!(i32, put_i32, get_i32);
prim!(u32, put_u32, get_u32);
prim!(i64, put_i64, get_i64);
prim!(u64, put_u64, get_u64);
prim!(f32, put_f32, get_f32);
prim!(f64, put_f64, get_f64);

/// `long double`: 16 raw octets, no portable Rust equivalent.
///
/// A newtype rather than `[u8; 16]`, because a bare 16-byte array already has
/// meaning under the generic array impl — 16 octets, each aligned to 1 — and a
/// `long double` is one value aligned to 8. Same bytes here, different type,
/// and letting them share an impl would hide that they differ on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LongDouble(pub [u8; 16]);

impl Cdr for LongDouble {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        e.put_long_double(self.0);
        Ok(())
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        Ok(LongDouble(d.get_long_double()?))
    }
}

impl Cdr for String {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        e.put_str(self);
        Ok(())
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        Ok(d.get_string()?)
    }
}

impl<T: Cdr> Cdr for Vec<T> {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        e.put_u32(self.len() as u32);
        for item in self {
            item.put(e)?;
        }
        Ok(())
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        let n = d.get_u32()?;
        // Validated against the bytes actually present, so a four-byte length
        // cannot buy a multi-gigabyte allocation (the Phase 0 audit's worst
        // finding, kept out of a third door).
        let n = d.validate_count(n, 1)?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(T::get(d)?);
        }
        Ok(out)
    }
}

/// IDL arrays: no length prefix, the length is in the type.
impl<T: Cdr, const N: usize> Cdr for [T; N] {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        for item in self {
            item.put(e)?;
        }
        Ok(())
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        let mut out = Vec::with_capacity(N);
        for _ in 0..N {
            out.push(T::get(d)?);
        }
        out.try_into().map_err(|_| GiopError::Decode("array length mismatch"))
    }
}

/// The one codec generated code uses for wide characters.
///
/// GIOP 1.2 with UTF-16 — the same choice as the dynamic path's default, so the
/// two paths produce identical bytes. Threading the connection's negotiated
/// codec through every generated signature is stream-B batch-2 work; until
/// then this is one constant in one place, not a rule copied into stubs.
fn wide() -> WideCodec {
    WideCodec::new(Version::V1_2, CodeSetId::UTF_16).expect("1.2 + UTF-16 is always valid")
}

/// `wstring`, distinct from `String` because the wire encoding differs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WString(pub String);

impl Cdr for WString {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        wide().put_wstring(e, &self.0).map_err(|_| GiopError::Decode("untranslatable wstring"))
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        wide().get_wstring(d).map(WString).map_err(|_| GiopError::Decode("malformed wstring"))
    }
}

/// `wchar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WChar(pub char);

impl Cdr for WChar {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        wide().put_wchar(e, self.0).map_err(|_| GiopError::Decode("wchar outside the BMP"))
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        wide().get_wchar(d).map(WChar).map_err(|_| GiopError::Decode("malformed wchar"))
    }
}

/// `any`: a value carrying its own description.
///
/// Static code keeps the dynamic representation here on purpose — an `any` is
/// dynamic by definition, and inventing a static mirror of `Value` would be a
/// second implementation of the same wire rules.
#[derive(Debug, Clone, PartialEq)]
pub struct AnyVal(pub orbweaver_giop::typecode::TypeCode, pub orbweaver_dynamic::Value);

impl Cdr for AnyVal {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        // The closure form keeps the value aligned against the outer stream;
        // building it detached restarts alignment at zero (the thrice-paid-for
        // Phase 1 lesson).
        orbweaver_giop::typecode::encode_any_with(e, &self.0, |enc| {
            let _ = orbweaver_dynamic::encode(enc, &self.0, &self.1);
        })?;
        // Re-run the validation the closure had to swallow.
        let mut probe = Encoder::new(Endian::Little);
        orbweaver_dynamic::encode(&mut probe, &self.0, &self.1)
            .map_err(|_| GiopError::Decode("any value does not match its TypeCode"))
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        let tc = orbweaver_giop::typecode::decode(d)?;
        let v = orbweaver_dynamic::decode(d, &tc)
            .map_err(|_| GiopError::Decode("any body does not match its TypeCode"))?;
        Ok(AnyVal(tc, v))
    }
}

/// A `TypeCode` as a value, which is what `::CORBA::TypeCode` declares.
///
/// The whole implementation is `giop`'s own codec, deliberately: a `TypeCode`
/// inside an `any` and a `TypeCode` as a parameter are the same bytes, and a
/// second encoding of the same rules is how the two drift apart.
///
/// This exists because the registry used to load `::CORBA::TypeCode` as
/// `void`. The generator then emitted a stub that marshalled **nothing** where
/// a peer expected a TypeCode, and it did so silently, because a `void` return
/// is a perfectly ordinary thing to generate.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeCodeVal(pub orbweaver_giop::typecode::TypeCode);

impl Cdr for TypeCodeVal {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        orbweaver_giop::typecode::encode(e, &self.0)
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        Ok(TypeCodeVal(orbweaver_giop::typecode::decode(d)?))
    }
}

/// An object reference, inline-marshalled; `None` is nil.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObjRef(pub Option<Ior>);

impl Cdr for ObjRef {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        match &self.0 {
            Some(ior) => ior.write_to(e),
            None => {
                // Nil: empty type id, zero profiles — not an absent field.
                e.put_str("");
                e.put_u32(0);
                Ok(())
            }
        }
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        let ior = Ior::read_from(d)?;
        Ok(if ior.type_id.is_empty() && ior.profiles.is_empty() {
            ObjRef(None)
        } else {
            ObjRef(Some(ior))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_dynamic::Value;
    use orbweaver_giop::typecode::TypeCode;

    /// The §8 criterion at the runtime layer: for every type this module
    /// marshals, the bytes must equal the dynamic path's bytes. Comparing
    /// against our own decoder alone would prove two halves of one file agree.
    fn same_bytes_as_dynamic<T: Cdr>(v: &T, tc: &TypeCode, dv: &Value) {
        for endian in [Endian::Big, Endian::Little] {
            let mut a = Encoder::new(endian);
            v.put(&mut a).expect("static put");
            let mut b = Encoder::new(endian);
            orbweaver_dynamic::encode(&mut b, tc, dv).expect("dynamic encode");
            assert_eq!(a.finish().unwrap(), b.finish().unwrap(), "{endian:?}");
        }
    }

    #[test]
    fn primitives_match_the_dynamic_bytes() {
        same_bytes_as_dynamic(&-7i32, &TypeCode::Long, &Value::Long(-7));
        same_bytes_as_dynamic(&true, &TypeCode::Boolean, &Value::Bool(true));
        same_bytes_as_dynamic(&2.5f64, &TypeCode::Double, &Value::Double(2.5));
        same_bytes_as_dynamic(
            &"hello".to_owned(),
            &TypeCode::String(0),
            &Value::String("hello".into()),
        );
    }

    #[test]
    fn sequences_and_arrays_match_the_dynamic_bytes() {
        let seq = TypeCode::Sequence { element: Box::new(TypeCode::Octet), bound: 0 };
        same_bytes_as_dynamic(
            &vec![1u8, 2, 3],
            &seq,
            &Value::List(vec![Value::Octet(1), Value::Octet(2), Value::Octet(3)]),
        );
        let arr = TypeCode::Array { element: Box::new(TypeCode::Long), length: 2 };
        same_bytes_as_dynamic(
            &[10i32, 20],
            &arr,
            &Value::List(vec![Value::Long(10), Value::Long(20)]),
        );
    }

    #[test]
    fn wide_text_matches_the_dynamic_bytes() {
        same_bytes_as_dynamic(
            &WString("안녕".into()),
            &TypeCode::WString(0),
            &Value::WString("안녕".into()),
        );
        same_bytes_as_dynamic(&WChar('한'), &TypeCode::WChar, &Value::WChar('한'));
    }

    #[test]
    fn any_and_object_references_match_the_dynamic_bytes() {
        same_bytes_as_dynamic(
            &AnyVal(TypeCode::Double, Value::Double(-0.125)),
            &TypeCode::Any,
            &Value::Any(Box::new(TypeCode::Double), Box::new(Value::Double(-0.125))),
        );
        let tc = TypeCode::ObjRef { id: "IDL:m/I:1.0".into(), name: "I".into() };
        same_bytes_as_dynamic(&ObjRef(None), &tc, &Value::ObjRef(None));
    }

    /// The completion status is the servant's decision, and the three ways of
    /// making it must reach the exception unchanged — this is the field a
    /// client's retry logic reads, so a builder that quietly normalised it
    /// would be worse than no builder.
    #[test]
    fn a_raise_carries_the_completion_status_the_servant_chose() {
        assert_eq!(raise::no_permission().did_not_run().completed, Completion::No);
        assert_eq!(raise::transient().may_have_run().completed, Completion::Maybe);
        assert_eq!(raise::internal().ran_to_completion().completed, Completion::Yes);
        assert_eq!(
            raise::object_not_exist().with_completion(Completion::Maybe).completed,
            Completion::Maybe
        );
    }

    #[test]
    fn a_raise_carries_its_repository_id_and_minor_code() {
        let ex = raise::bad_param().omg_minor(3).did_not_run();
        assert_eq!(ex.id, BAD_PARAM);
        assert_eq!(ex.minor, 0x4f4d_0003, "the OMG vendor space is the high half");
        let ex = raise::other("IDL:omg.org/CORBA/NO_RESOURCES:1.0").minor(42).may_have_run();
        assert_eq!(ex.id, "IDL:omg.org/CORBA/NO_RESOURCES:1.0");
        assert_eq!(ex.minor, 42);
    }

    #[test]
    fn a_huge_declared_sequence_length_is_refused_not_allocated() {
        let bytes = [0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0];
        let mut d = Decoder::new(&bytes, Endian::Big);
        assert!(<Vec<f64> as Cdr>::get(&mut d).is_err());
    }
}
