//! The ORB object, in the one shape that closes `corbaloc:rir:`.
//!
//! [`crate::naming::ObjectUrl`] has parsed `corbaloc:rir:NameService` since
//! Phase 1 and [`crate::naming::ObjectUrl::to_ior`] has answered `None` for it
//! ever since: **the ORB spoke the language of `resolve_initial_references` and
//! had nothing to resolve against.** `Corbaloc` and `Corbaname` both work
//! because the caller supplied an address; `InitialReference` is exactly the
//! case where no address is given and the ORB is supposed to know. That
//! difference is the whole gap, and this module is the table that closes it.
//!
//! # What the specification fixes, and what it leaves to us
//!
//! CORBA 3.4 Part 1 **§8.5.2 *Obtaining Initial Object References*** defines
//! the mechanism and both of its operations:
//!
//! ```text
//! typedef string ObjectId;
//! typedef sequence <ObjectId> ObjectIdList;
//! ObjectIdList list_initial_services ();
//!
//! exception InvalidName {};
//! Object resolve_initial_references (in ObjectId identifier)
//!     raises (InvalidName);
//! ```
//!
//! Three sentences of that sub clause decide this module's shape:
//!
//! - *"a simplified, local version of the Naming Service that applications can
//!   use to obtain a small, defined set of object references … the naming
//!   context can be **flattened to be a single-level name space**."* That is
//!   why this is a flat `ObjectId -> Ior` map and not a hierarchy, and it is
//!   why it is **not** the naming service: `crate::naming_server` serves
//!   `CosNaming` over the wire and is a servant; this answers before any
//!   servant is reached. They meet at one entry — `NameService` resolves to
//!   the naming server's IOR — and are otherwise different things.
//! - *"`resolve_initial_references` **never returns a nil reference**. Instead,
//!   the non-availability of a particular reference is indicated by throwing an
//!   `InvalidName` exception (even if a nil reference is explicitly configured
//!   for an `ObjectId`)."* So the refusal is [`InvalidName`], never `None` and
//!   never a nil [`Ior`] — and here it names the key that was asked for.
//! - The **reserved `ObjectId`s are enumerated** by the specification, not by
//!   us: see [`RESERVED_OBJECT_IDS`], transcribed from §8.5.2's own sentence
//!   and Table 8.1.
//!
//! `register_initial_reference` is **§16.10.1**, on the same ORB interface; its
//! two `InvalidName` conditions are transcribed onto
//! [`Orb::register_initial_reference`].
//!
//! # Nothing registers itself
//!
//! A fresh [`Orb`] resolves nothing at all — [`Orb::list_initial_services`]
//! answers an empty list — and every name resolves because something
//! registered it. That is deliberate: an ORB that silently manufactured a
//! `NameService` entry would be answering for a service it does not serve, and
//! the refusal is the honest answer until a deployment says otherwise.
//!
//! # The two conversions (D019 step 2)
//!
//! [`Orb::string_to_object`] and [`Orb::object_to_string`] are **CORBA 3.4
//! §8.2.2**, under the names every CORBA programmer looks for. They are not new
//! behaviour: [`Ior::parse`], [`Ior::to_stringified`] and
//! [`crate::naming::ObjectUrl`] already did the work. What they add is the
//! **decision** — §8.2.2 has one `string_to_object` because a caller holding a
//! stringified reference cannot be expected to know which of three forms it
//! has, and until step 1 landed the `rir` case there was no single place that
//! could answer all three. See [`Orb::string_to_object`] for why that is a
//! decision and not a rename.
//!
//! # The four responsibilities, complete
//!
//! D019 §5 proposes four responsibilities for this object and it now has all
//! four: the initial references table (step 1), the two named conversions
//! (step 2), the eight configuration numbers (step 3), and the transport
//! (step 4 — [`Orb::server`] and [`Orb::pool`]). The root POA is handed out by
//! `orbweaver_object::OrbPoa`, an extension trait in that crate, because
//! `orbweaver-object` depends on this crate and not the other way round.
//!
//! **Four, and one thing beyond them.** §5's *"not proposed"* list is still
//! binding in the part that matters: there is no `ORB_init` signature here and
//! no thread policies, and each would still have to earn its place from a
//! scattered fact or a fired trigger, as everything else here did.
//!
//! # The fifth thing, and why it is not `run`
//!
//! [`Orb::shutdown`] exists (D032). D019 §5's refusal named
//! *"`ORB::run`/`shutdown` semantics"* together, and **the half that was
//! refused is `run`** — an event-loop model, a main thread parked in the ORB,
//! which this ORB does not have and does not grow here. D029 §3.1 measured why
//! the other half had to follow anyway: step 4 made [`Orb::server`] and
//! [`Orb::pool`] the only way in, so *"an ORB can hand out N servers and cannot
//! stop one of them"* became a property of the product rather than of a spike.
//!
//! Nothing about the serving model moves. The caller still owns the thread,
//! `serve_shared` still takes the caller's own stop predicate, and the ORB
//! joins nothing — it raises a flag that is OR'd with that predicate, with
//! neither privileged. The bound this buys is on [`Orb::shutdown`]; the
//! argument is D032.
//!
//! *거절된 절반은 `run`이다. 서빙 모델은 움직이지 않는다 — 호출자가 여전히 스레드를
//! 소유하고, ORB는 아무것도 합류시키지 않는다.*
//!
//! *ORB가 `resolve_initial_references`의 언어를 말할 줄 알면서 그것을 대조할
//! 표를 갖고 있지 않았다. 이 모듈이 그 표다 — CORBA 3.4 §8.5.2가 정의한 평평한
//! 단일 계층 이름 공간이고, 없는 이름은 **이름을 대며** 거절한다.*

pub mod config;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::Ior;
use crate::naming::ObjectUrl;

pub use config::{ConfigError, OrbConfig};

/// The `ObjectId`s CORBA 3.4 §8.5.2 reserves, in the order the specification
/// lists them.
///
/// The first ten come from the sentence *"Currently, reserved ObjectIds are
/// RootPOA, POACurrent, InterfaceRepository, NameService, TradingService,
/// SecurityCurrent, TransactionCurrent, DynAnyFactory, ORBPolicyManager,
/// PolicyCurrent, NotificationService, TypedNotificationService, CodecFactory,
/// PICurrent, ComponentHomeFinder and PSS"*; all sixteen appear again in
/// Table 8.1 with the interface each one denotes — `NameService` is a
/// `CosNaming::NamingContext`, `InterfaceRepository` a `CORBA::Repository`,
/// `RootPOA` a `PortableServer::POA`.
///
/// **This list gates nothing.** §8.5.3.1 requires that an ORB *"can be
/// administratively configured to return an arbitrary object reference"*, and
/// §16.10.1's `register_initial_reference` takes any `ObjectId`, so refusing a
/// name for being absent from this list would be non-conformant. It is used for
/// one thing: telling a caller who asked for a reserved name that this ORB
/// serves nothing under it apart from a caller who invented a name. Both are
/// refused; they are refused with different sentences because they need
/// different fixes.
pub const RESERVED_OBJECT_IDS: [&str; 16] = [
    "RootPOA",
    "POACurrent",
    "InterfaceRepository",
    "NameService",
    "TradingService",
    "SecurityCurrent",
    "TransactionCurrent",
    "DynAnyFactory",
    "ORBPolicyManager",
    "PolicyCurrent",
    "NotificationService",
    "TypedNotificationService",
    "CodecFactory",
    "PICurrent",
    "ComponentHomeFinder",
    "PSS",
];

/// Whether `id` is one of the `ObjectId`s CORBA 3.4 §8.5.2 reserves.
///
/// Case-sensitive: §8.5.2 gives the spellings exactly and a peer asking for
/// `nameservice` is asking for a different `ObjectId` than `NameService`.
pub fn is_reserved_object_id(id: &str) -> bool {
    RESERVED_OBJECT_IDS.contains(&id)
}

/// `CORBA::ORB::InvalidName` — the one exception §8.5.2 and §16.10.1 raise.
///
/// # The sentence lives here
///
/// Every layer that has to refuse an initial reference says it in these words,
/// because [`std::fmt::Display`] on this type is the only place the words are
/// written. A layer that retypes the head instead of formatting this value is
/// the `pub(crate)`-escaping class CLAUDE.md records, and it will go false
/// silently.
///
/// Each variant carries the `ObjectId` it is about, so **no refusal is
/// anonymous**: `"resolve_initial_references answered no"` is not a diagnostic,
/// and neither is `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidName {
    /// [`Orb::resolve_initial_reference`] was asked for an `ObjectId` this ORB
    /// has no entry for. §8.5.2: *"the non-availability of a particular
    /// reference is indicated by throwing an `InvalidName` exception."*
    NotRegistered {
        /// The `ObjectId` that was asked for.
        key: String,
        /// Whether §8.5.2 reserves that spelling — the difference between
        /// "you are asking for a service this ORB does not serve" and "you are
        /// asking for a name nobody has ever defined".
        ///
        /// **A peer makes the same distinction**, which is why this field
        /// exists rather than being a nicety. Measured against omniORB
        /// 2026-08-25, with `-ORBInitRef NameService=…` configured:
        /// `corbaloc:rir:/InterfaceRepository` — reserved, unregistered — is
        /// refused `NO_RESOURCES(InitialRefNotFound)`, while
        /// `corbaloc:rir:/NoSuchService` — never reserved — is refused
        /// `BAD_PARAM(BadURIOther)`. Two exceptions, because the two need
        /// different fixes: register the service, or fix the name.
        reserved: bool,
        /// What this ORB *does* answer for, so the refusal is actionable and
        /// not merely correct.
        known: Vec<String>,
    },
    /// [`Orb::register_initial_reference`] was given an empty `ObjectId`.
    /// §16.10.1: *"InvalidName is raised if this operation is called with an
    /// empty string id."*
    EmptyId,
    /// [`Orb::register_initial_reference`] was given an `ObjectId` that is
    /// already registered. §16.10.1: *"this operation is called with an id
    /// that is already registered."*
    AlreadyRegistered {
        /// The `ObjectId` that was already taken.
        key: String,
    },
}

impl std::fmt::Display for InvalidName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvalidName::NotRegistered { key, reserved, known } => {
                write!(f, "no initial reference is registered for {key:?}")?;
                if *reserved {
                    write!(f, " (an ObjectId CORBA 3.4 §8.5.2 reserves)")?;
                } else {
                    write!(f, " (not an ObjectId CORBA 3.4 §8.5.2 reserves)")?;
                }
                if known.is_empty() {
                    write!(f, "; this ORB has registered nothing")
                } else {
                    write!(f, "; this ORB has registered {}", known.join(", "))
                }
            }
            InvalidName::EmptyId => write!(
                f,
                "an initial reference cannot be registered under the empty ObjectId \
                 (CORBA 3.4 §16.10.1)"
            ),
            InvalidName::AlreadyRegistered { key } => write!(
                f,
                "{key:?} is already registered as an initial reference \
                 (CORBA 3.4 §16.10.1); unregister it first"
            ),
        }
    }
}

impl std::error::Error for InvalidName {}

/// The ORB's initial references table: `ObjectId -> Ior`, and the two
/// operations CORBA 3.4 §8.5.2 defines over it.
///
/// A [`BTreeMap`] rather than a `HashMap` because
/// [`Orb::list_initial_services`] should answer in a stable order — a listing
/// whose order changes between runs is a listing a harness cannot compare.
///
/// # Sharing
///
/// Registration takes `&mut self` and resolution takes `&self`, so a consumer
/// that shares the table across threads wraps it the way it wraps everything
/// else. Interior mutability is not built in here because nothing needs it
/// yet, and an `Arc<Mutex<_>>` chosen before there is a caller is a guess.
#[derive(Debug, Clone, Default)]
pub struct Orb {
    initial: BTreeMap<String, Ior>,
    config: OrbConfig,
    /// What this ORB has handed out and can take back (D032).
    ///
    /// **Shared across clones**, unlike the two fields above. That asymmetry is
    /// the deliberate part: a clone that could not stop what the original
    /// handed out would recreate, one level up, exactly the *gives and cannot
    /// take back* asymmetry D029 §3.1 opened this work to close. An `Orb` is
    /// one ORB; a clone is a handle to it for lifecycle purposes.
    handouts: Arc<Handouts>,
}

/// [`Orb`] equality is **equality of configuration** — the initial references
/// table and the eight numbers — and deliberately excludes
/// [`Orb::shutdown`]'s state.
///
/// Written by hand rather than derived, and the reason is worth having in view
/// because the alternative reads better and is wrong. Deriving is not available
/// once the handouts list is a shared, interior-mutable thing; identity by
/// pointer would have made `Orb::new() != Orb::new()`, silently reversing what
/// this comparison has answered since D019 step 1. So: two ORBs configured
/// alike are equal, **including when one of them has been stopped**, because
/// what they were configured to be is not changed by being asked to stop.
///
/// *동등성은 설정의 동등성이다. 멈춤 상태는 여기 포함되지 않는다 — 멈춰 달라는
/// 요청이 그 ORB가 무엇으로 설정되었는지를 바꾸지는 않기 때문이다.*
impl PartialEq for Orb {
    fn eq(&self, other: &Self) -> bool {
        self.initial == other.initial && self.config == other.config
    }
}

impl Eq for Orb {}

/// The transport an [`Orb`] handed out, held weakly so a dropped handout prunes
/// itself.
#[derive(Debug, Default)]
struct Handouts {
    /// Raised once by [`Orb::shutdown`] and never lowered.
    stopped: crate::server::StopFlag,
    servers: Mutex<Vec<ServerHandout>>,
    pools: Mutex<Vec<crate::pool::PoolWatch>>,
}

/// One [`Server`](crate::server::Server) this ORB handed out.
#[derive(Debug)]
struct ServerHandout {
    /// Weak: a dropped `Server` prunes itself, and that is sound rather than
    /// convenient — `Server::serve_shared` takes `&self`, so no serving loop
    /// can outlive the `Server` it serves. A dead weak means *already
    /// stopped*.
    stop: std::sync::Weak<std::sync::atomic::AtomicBool>,
    /// Strong, because [`Orb::wait_until_stopped`] reads it and a cloned
    /// `ServerStats` is six words. It keeps no socket alive.
    stats: crate::server::ServerStats,
}

/// What one [`Orb::shutdown`] did, so a shutdown that stopped nothing is
/// visible rather than silent.
///
/// A count of *zero* servers on an ORB that handed out four is not a bug — it
/// means all four were dropped before the shutdown, which closed their
/// listeners already. It is reported so a caller expecting otherwise finds out
/// here rather than from a port that is still bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shutdown {
    servers: usize,
    pools: usize,
    already_gone: usize,
}

impl Shutdown {
    /// Servers that were still alive and have now been asked to stop.
    pub fn servers(&self) -> usize {
        self.servers
    }

    /// Pools that were still alive and are now closed.
    pub fn pools(&self) -> usize {
        self.pools
    }

    /// Handouts that had already been dropped, so there was nothing to stop.
    pub fn already_gone(&self) -> usize {
        self.already_gone
    }
}

impl Orb {
    /// An ORB that resolves nothing and changes no limit. See *Nothing
    /// registers itself* in the module docs: this is not a stub, it is the
    /// answer until a deployment registers something.
    pub fn new() -> Self {
        Self::default()
    }

    /// An ORB carrying a deployment's configuration (D019 step 3).
    ///
    /// Every `-ORBInitRef` in the configuration is resolved with
    /// [`Orb::string_to_object`] and registered, **all of them or none**: the
    /// table is built to one side and only moved in once every entry has been
    /// read, so a configuration with one bad URL in it leaves no half-populated
    /// ORB behind. That is the property CORBA 3.4 §8.5.1 requires of
    /// `ORB_init`'s argument handling and the one the MCP `--config` batch
    /// proved is worth having.
    ///
    /// The seven-plus-one numbers are **applied**, as of D019 step 4. They
    /// reach a [`Server`](crate::server::Server) through [`Orb::server`] and a
    /// `Connection` through [`Orb::pool`], and those are the only ways to
    /// obtain either — which is the point, and is why step 4 closed the
    /// hand-built path in the same commit that connected the configuration.
    /// Between step 3 and step 4 they were held and not applied, so
    /// `-ORBmaxMessageSize 4096` parsed, validated, and changed nothing a peer
    /// could observe.
    ///
    /// # Errors
    ///
    /// [`ConfigError::InitRefUnreadable`], naming the `ObjectId`, the URL and
    /// why the URL could not be turned into a reference.
    pub fn with_config(config: OrbConfig) -> std::result::Result<Self, ConfigError> {
        let mut orb = Self { initial: BTreeMap::new(), config, handouts: Arc::default() };
        let mut table = BTreeMap::new();
        for (object_id, url) in orb.config.initial_references() {
            let ior =
                orb.string_to_object(url).map_err(|cause| ConfigError::InitRefUnreadable {
                    object_id: object_id.clone(),
                    url: url.clone(),
                    reason: cause.to_string(),
                })?;
            // `OrbConfig::from_orb_args` already refuses a repeated ObjectId,
            // so this cannot collide; inserting into a table of its own is what
            // makes "all of them or none" a property of the code rather than a
            // claim.
            table.insert(object_id.clone(), ior);
        }
        orb.initial = table;
        Ok(orb)
    }

    /// The deployment's answers to the numbers this ORB owns. Defaults to
    /// today's constants for anything unset — see [`OrbConfig`].
    pub fn config(&self) -> &OrbConfig {
        &self.config
    }

    /// `ORB::register_initial_reference` (CORBA 3.4 §16.10.1) — makes `id`
    /// resolve to `obj`.
    ///
    /// # Errors
    ///
    /// The two §16.10.1 names: [`InvalidName::EmptyId`] for an empty `id`, and
    /// [`InvalidName::AlreadyRegistered`] for one already in the table.
    ///
    /// # Two readings of §16.10.1, and which one this takes
    ///
    /// The sub clause says both that the operation *"can be used to replace the
    /// object reference corresponding to any of the OMG specified Ids"* and
    /// that `InvalidName` is raised when it *"is called with an id that is
    /// already registered, including the default names defined by OMG."* Those
    /// cannot both hold. This takes the second — refuse a re-registration —
    /// because it is the one stated as a condition on this operation rather
    /// than as an illustration, and because the first reading's whole subject
    /// is *substituting for a vendor's built-in service*, of which this ORB has
    /// none: nothing is registered until a caller registers it, so there is
    /// nothing here for a substitution to displace. A caller that means to
    /// replace an entry says so with [`Orb::unregister_initial_reference`].
    ///
    /// §16.10.1 also allows an implementation to refuse substitution of
    /// `RootPOA`, `POACurrent`, `DynAnyFactory`, `ORBPolicyManager`,
    /// `PolicyCurrent`, `CodecFactory` and `PICurrent`, and requires that such
    /// a restriction *"shall be clearly documented"*. **This ORB restricts
    /// none of them**, for the same reason: it supplies none of them, so there
    /// is nothing to protect.
    pub fn register_initial_reference(
        &mut self,
        id: &str,
        obj: Ior,
    ) -> std::result::Result<(), InvalidName> {
        if id.is_empty() {
            return Err(InvalidName::EmptyId);
        }
        if self.initial.contains_key(id) {
            return Err(InvalidName::AlreadyRegistered { key: id.to_owned() });
        }
        self.initial.insert(id.to_owned(), obj);
        Ok(())
    }

    /// Removes `id` from the table, answering what was registered under it.
    ///
    /// Not an OMG operation — §8.5.2 and §16.10.1 define no un-registration —
    /// and it exists for one reason the specification does not have to care
    /// about: [`Orb::register_initial_reference`] refuses to overwrite, so
    /// without this a caller that wants to *replace* an entry has no way to say
    /// so, and would either be pushed into a second registration operation with
    /// different rules or into rebuilding the ORB. It is named for what it does
    /// rather than borrowing a CORBA name it is not.
    pub fn unregister_initial_reference(&mut self, id: &str) -> Option<Ior> {
        self.initial.remove(id)
    }

    /// `ORB::resolve_initial_references` (CORBA 3.4 §8.5.2) — the reference
    /// registered under `id`.
    ///
    /// # Errors
    ///
    /// [`InvalidName::NotRegistered`], **naming `id`** and listing what this
    /// ORB does answer for. §8.5.2 is explicit that this is the only way a
    /// non-answer may be reported: *"resolve_initial_references never returns
    /// a nil reference."* Returning `None`, or a nil [`Ior`], would be the
    /// silent refusal the sub clause forbids.
    pub fn resolve_initial_reference(&self, id: &str) -> std::result::Result<Ior, InvalidName> {
        self.initial.get(id).cloned().ok_or_else(|| InvalidName::NotRegistered {
            key: id.to_owned(),
            reserved: is_reserved_object_id(id),
            known: self.list_initial_services(),
        })
    }

    /// `ORB::list_initial_services` (CORBA 3.4 §8.5.2) — every `ObjectId` this
    /// ORB will answer for, in a stable order.
    ///
    /// The half of §8.5.2 that exists so *"an application can determine which
    /// objects have references available via the initial references
    /// mechanism"*. A table that can be resolved but not listed leaves a client
    /// guessing names, which is the position `corbaloc:rir:` was in before this
    /// module.
    pub fn list_initial_services(&self) -> Vec<String> {
        self.initial.keys().cloned().collect()
    }

    /// Turns any parsed object URL into an [`Ior`] — including the one form
    /// [`ObjectUrl::to_ior`] cannot answer.
    ///
    /// This is the *"caller resolves first"* half of the decision written on
    /// [`ObjectUrl::to_ior`]: the two forms that carry an address are handed
    /// straight to the same profile construction they always used, and the
    /// third — `corbaloc:rir:<ObjectId>`, whose answer is not in the URL — is
    /// answered from this table. `type_id` is the repository id to stamp on a
    /// URL-built reference and is ignored for a `rir:` name, because a
    /// registered [`Ior`] already carries the id it was registered with.
    ///
    /// # Errors
    ///
    /// [`InvalidName`], for a `rir:` name this ORB does not answer for. The
    /// two addressed forms cannot fail here.
    pub fn resolve_url(
        &self,
        url: &ObjectUrl,
        type_id: &str,
    ) -> std::result::Result<Ior, InvalidName> {
        match url {
            ObjectUrl::InitialReference(id) => self.resolve_initial_reference(id),
            ObjectUrl::Corbaloc { addresses, object_key }
            | ObjectUrl::Corbaname { addresses, object_key, .. } => {
                Ok(crate::naming::addressed_ior(addresses, object_key, type_id))
            }
        }
    }

    /// `ORB::string_to_object` (CORBA 3.4 §8.2.2.2) — **the one operation that
    /// decides** what a stringified reference is and turns it into one.
    ///
    /// Accepts every form this ORB can read:
    ///
    /// | The string starts | It is read by | Since |
    /// |---|---|---|
    /// | `IOR:<hex>` | [`Ior::parse`] | Phase 1 |
    /// | `corbaloc:`, `corbaname:` | [`ObjectUrl::parse`] then [`Orb::resolve_url`] | Phase 1 |
    /// | `corbaloc:rir:<ObjectId>` | the initial references table | D019 step 1 |
    ///
    /// # Why this is not cosmetic
    ///
    /// Those three rows were three separate entry points in two modules, and
    /// **the caller had to already know which one it was holding** — which is
    /// precisely the knowledge a stringified reference exists to remove. A
    /// configuration file, a command line and a `-ORBInitRef` argument all hand
    /// over a string whose form is the *deployment's* choice, not the
    /// programmer's. §8.2.2 has one operation because a caller cannot have that
    /// knowledge; we had two because we were reading from the emitting side,
    /// where `to_stringified` is a perfectly good *serialiser* name and
    /// `Ior::parse` is a perfectly good *parser* name. Neither is the name of
    /// the thing a caller wants.
    ///
    /// # The type of what comes back
    ///
    /// §8.5.2: *"The application is responsible for narrowing the object
    /// reference returned."* An `IOR:` string carries the repository id it was
    /// written with and that id is kept. A URL carries **no type at all**, so
    /// the reference comes back with an empty `type_id` rather than one this
    /// function invented — an invented id is a claim the caller cannot check
    /// and would carry onto the wire. A caller that knows the type stamps it,
    /// or calls [`Orb::resolve_url`], which takes one.
    ///
    /// # Errors
    ///
    /// [`StringToObjectError`], which keeps the four causes apart because they
    /// need four different fixes, and names the string in every one.
    ///
    /// # Not accepted, deliberately
    ///
    /// `file:`, `ftp:` and `http:` URLs — which some ORBs read as *"fetch a
    /// stringified IOR from there"* — are refused. They are not in §8.2.2 or in
    /// Part 2 §7.6.10's object URL schemes, and each turns a conversion into an
    /// outbound network fetch from a string a peer may have supplied.
    pub fn string_to_object(&self, s: &str) -> std::result::Result<Ior, StringToObjectError> {
        let text = s.trim();
        if text.len() >= 4 && text[..4].eq_ignore_ascii_case("IOR:") {
            // §7.6.9's prefix is case-insensitive, which is why this sniffs the
            // same way `ior_hex_bytes` does rather than with `starts_with`.
            return Ior::parse(text)
                .map_err(|cause| StringToObjectError::Ior { text: text.to_owned(), cause });
        }
        // A `corbaname:` URL with a name in it denotes **the object bound under
        // that name**, not the naming context that holds it (Part 2 §7.6.10.5).
        // Producing the reference it denotes therefore takes an outbound
        // `resolve` call, and a conversion that dials is not what this batch is.
        // Measured against omniORB 2026-08-25, which *does* dial here — it
        // answers `TRANSIENT` for an unreachable naming service — while
        // agreeing with us on the fragment-less form, which denotes the context
        // itself and needs no call.
        //
        // Returning the naming context would be the wrong object, silently, and
        // that is the one answer this operation must never give. Refused by
        // name until the resolving version has its own batch and its own
        // timeout: see [`crate::naming::NamingContext::from_url`], which is how
        // a caller does it today, in two steps that it can see.
        if let Ok(ObjectUrl::Corbaname { name, .. }) = ObjectUrl::parse(text)
            && !name.is_empty()
        {
            return Err(StringToObjectError::NeedsANamingCall {
                text: text.to_owned(),
                name: crate::naming::stringify_name(&name),
            });
        }
        let url = ObjectUrl::parse(text).map_err(|cause| {
            // `ObjectUrl::parse` refuses an unknown scheme with `BadSchemeName`,
            // which is also the answer for a string that is not a reference at
            // all — the only two things it can be, once `IOR:` is ruled out.
            match cause {
                crate::naming::UrlError::BadSchemeName(_) => {
                    StringToObjectError::NotAReferenceString { text: text.to_owned() }
                }
                other => StringToObjectError::Url { text: text.to_owned(), cause: other },
            }
        })?;
        self.resolve_url(&url, "")
            .map_err(|cause| StringToObjectError::Name { text: text.to_owned(), cause })
    }

    /// `ORB::object_to_string` (CORBA 3.4 §8.2.2.1) — the `IOR:<hex>` string
    /// for a reference.
    ///
    /// The conformance sentence §8.2.2 gives is a **round trip**, and it is the
    /// only thing the sub clause promises: *"if obj is a valid reference to an
    /// object, then `string_to_object(object_to_string(obj))` will return a
    /// valid reference to the same object … For all conforming ORBs supporting
    /// IOP, this remains true even if the two operations are performed on
    /// different ORBs."* So this emits the interoperable form and never a URL:
    /// a URL is a *bootstrap* that may name a different object tomorrow, which
    /// is the opposite of what §8.2.2 asks for.
    ///
    /// # Errors
    ///
    /// The reference could not be marshalled — see [`Ior::to_stringified`],
    /// which this delegates to and does not reimplement.
    pub fn object_to_string(&self, obj: &Ior) -> crate::Result<String> {
        obj.to_stringified()
    }

    /// A listening [`Server`] carrying this ORB's configuration — the fourth
    /// responsibility D019 §5 names, and **the only way to obtain one**.
    ///
    /// [`Server::bind`] became `pub(crate)` with this method. That is the whole
    /// mechanism, and it is worth saying why the door is closed rather than
    /// merely signposted: D019 §3 measured an ORB whose eight numbers parsed,
    /// validated and were *held*, while `-ORBmaxMessageSize 4096` changed
    /// nothing a peer could observe — because every object that touches the
    /// wire was constructed somewhere else. A second constructor is not a
    /// convenience next to a configuration path; it is the configuration path's
    /// leak. With one door there is nowhere for the numbers to fail to arrive.
    ///
    /// `host` is deliberately *not* an argument: binding and publishing are
    /// different decisions and [`Server::ior`] keeps them apart, which is the
    /// Phase 0 assumption D failure.
    ///
    /// # Behaviour on an unconfigured ORB
    ///
    /// [`Orb::new`] answers every one of the eight with the constant this crate
    /// compiled before D019, so `Orb::new().server(addr, key)` is
    /// byte-for-byte the old `Server::bind(addr, key)`. That is a property of
    /// [`OrbConfig`]'s accessors rather than a claim: an unset field is never
    /// read as a number at all.
    ///
    /// # Errors
    ///
    /// Whatever binding the listener answered — the address was taken, or is
    /// not one this host can bind — or [`Error::Stopped`](crate::Error::Stopped)
    /// if [`Orb::shutdown`] has been called. See D032 §7 for why a stopped ORB
    /// refuses rather than obliges.
    ///
    /// *문이 하나면 설정이 도착하지 못할 곳이 없다.*
    pub fn server(&self, addr: &str, object_key: Vec<u8>) -> crate::Result<crate::server::Server> {
        // Checked before the listener is bound, so a refusal costs no port.
        if self.handouts.stopped.raised() {
            return Err(crate::Error::Stopped { what: "a server" });
        }
        let mut server = crate::server::Server::bind(addr, object_key)?;
        server.apply_orb_config(&self.config);
        self.handouts
            .servers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(ServerHandout { stop: server.stop_flag().watch(), stats: server.stats() });
        // The window between the check above and the push: a `shutdown` that
        // landed in it raised `stopped` and then walked a list this entry was
        // not yet in, so nothing would ever raise this server's flag. Looking
        // again after the push closes it in both orderings — either shutdown
        // saw the entry, or we see `stopped` here. A server born stopped is
        // the honest outcome; a server that quietly outlived its ORB's
        // shutdown is the bug this whole batch is about.
        if self.handouts.stopped.raised() {
            server.stop_flag().raise();
        }
        Ok(server)
    }

    /// A client-side connection [`Pool`] carrying this ORB's configuration —
    /// the calling half of the same responsibility, and the only way to obtain
    /// one.
    ///
    /// The pool applies the five connection numbers to **every connection it
    /// dials**, at the one moment they can be applied: `Mux::over` takes them
    /// out of the `Connection` wholesale and a `Mux` has no setter. Before this
    /// existed, a pooled call ran on the compiled defaults no matter what the
    /// deployment had configured, and nothing was red — the numbers were on the
    /// ORB and the ORB was not on the path.
    pub fn pool(&self) -> crate::pool::Pool {
        self.pool_with_limits(crate::pool::Limits::default())
    }

    /// [`Orb::pool`] with [`Limits`](crate::pool::Limits) of your own.
    ///
    /// The pool's own five limits are still Rust-only — there is no `-ORB…` key
    /// for them. That is not an oversight and it is recorded in
    /// [`config`]'s module docs: they were left for the step that gave the ORB
    /// the transport, which is this one, and adding the keys is a separate
    /// batch now that there is finally something for such a key to apply
    /// itself to.
    ///
    /// # On a stopped ORB
    ///
    /// The pool is handed out **already closed** (D032 §6): it dials nothing
    /// and every call through it answers
    /// [`Error::Stopped`](crate::Error::Stopped).
    ///
    /// That differs from [`Orb::server`], which refuses outright, and the
    /// difference is the resource rather than a lapse in symmetry. Binding a
    /// listener takes a port, so refusing before the bind is what keeps a
    /// stopped ORB from holding one; constructing a pool takes an `Arc` and
    /// nothing else, and the refusal has a natural home one step later, at the
    /// call that would have dialled. This signature has nowhere to put an error
    /// and 21 call sites that would have to grow one to no purpose.
    pub fn pool_with_limits(&self, limits: crate::pool::Limits) -> crate::pool::Pool {
        let pool = crate::pool::Pool::with_limits_and_config(limits, self.config.clone());
        self.handouts.pools.lock().unwrap_or_else(|p| p.into_inner()).push(pool.watch());
        // Both orderings, exactly as in `server` — see the comment there.
        if self.handouts.stopped.raised() {
            pool.close();
        }
        pool
    }

    /// **Stops every server and pool this ORB handed out** — D029 §3.1's gap,
    /// and the answer to the design question D029 §5 O1 asked in writing first.
    ///
    /// The argument for this shape lives in
    /// `docs/decisions/D032-stopping-what-the-orb-handed-out.md` and is not
    /// repeated here. **The bound is here**, because the bound is this API's
    /// contract and its reader is holding this API.
    ///
    /// # The bound
    ///
    /// > After `shutdown` returns, every serving loop this ORB created will,
    /// > within one [`stop_poll`](OrbConfig::stop_poll), stop accepting and
    /// > stop reading. **At most one further request per already-admitted
    /// > connection** may still be served — the one whose bytes had already
    /// > reached the socket when that connection's thread last looked — and
    /// > that one is answered in full. Nothing is answered after it. Every live
    /// > connection is then told `CloseConnection` (§9.4.10), and no pool this
    /// > ORB created dials again.
    ///
    /// It is a bound and not a guarantee, which is the point: *at most one
    /// further request* is a number a caller can reason about, where *"in-flight
    /// work is finished"* is a sentence nobody can hold to. See
    /// `crates/orbweaver-giop/tests/orb_stops_what_it_handed_out.rs`, which
    /// measures the bound from a peer's own socket rather than from our
    /// counters.
    ///
    /// # What a peer mid-call sees
    ///
    /// Its reply, in full, and then the goodbye. Never a truncated reply, and
    /// never a bare TCP close with a request outstanding — §9.4.7 makes
    /// `CloseConnection` mean *"not processed, safe to re-send elsewhere"*, and
    /// that stays true only because the request after the flag is left
    /// **unread** rather than read and dropped (D032 §3).
    ///
    /// # What a caller holding a `Server` sees
    ///
    /// [`Server::serve_shared`](crate::server::Server::serve_shared) returns
    /// `Ok(())`, indistinguishable from its own `stop` predicate having gone
    /// true — the two are the same event. A caller that needs to tell them
    /// apart asks
    /// [`Server::stop_requested`](crate::server::Server::stop_requested).
    ///
    /// # This does not wait, and that is deliberate
    ///
    /// The ORB does not own the serving threads — the caller does — so it
    /// cannot join them, and pretending to would be the `run()` D019 §5 refused
    /// and D029 §4 forbids. `shutdown` raises flags and returns.
    /// [`Orb::wait_until_stopped`] is the separate, deadline-bounded question.
    ///
    /// # Idempotent, and one-way
    ///
    /// Calling it twice is harmless; there is no un-shutdown. Afterwards
    /// [`Orb::server`] refuses and [`Orb::pool`] hands out a closed pool, so
    /// `shutdown` is a lifecycle rather than a suggestion (D032 §7).
    ///
    /// *한계는 여기에 산다 — 한계는 이 API의 계약이고 그것을 필요로 하는 사람은 이
    /// API를 들고 있기 때문이다. 논증은 D032에 있고 여기서 되풀이하지 않는다.*
    pub fn shutdown(&self) -> Shutdown {
        // Raised **first**, and this ordering is what the race comments in
        // `server` and `pool_with_limits` depend on.
        self.handouts.stopped.raise();

        let mut servers = 0;
        let mut already_gone = 0;
        for handout in self.handouts.servers.lock().unwrap_or_else(|p| p.into_inner()).iter() {
            match handout.stop.upgrade() {
                Some(flag) => {
                    flag.store(true, std::sync::atomic::Ordering::Release);
                    servers += 1;
                }
                // The `Server` was dropped, which closed its listener and —
                // because `serve_shared` borrows it — ended every loop it ran.
                None => already_gone += 1,
            }
        }

        let mut pools = 0;
        for watch in self.handouts.pools.lock().unwrap_or_else(|p| p.into_inner()).iter() {
            if watch.close() {
                pools += 1;
            } else {
                already_gone += 1;
            }
        }

        Shutdown { servers, pools, already_gone }
    }

    /// Whether [`Orb::shutdown`] has been called on this ORB — or on any clone
    /// of it, which is the same ORB.
    pub fn is_shutdown(&self) -> bool {
        self.handouts.stopped.raised()
    }

    /// Waits, up to `deadline`, for every server this ORB handed out to go
    /// quiet. Answers whether they did.
    ///
    /// A **sleeping** poll — the harness rule about wait loops that do not wait
    /// applies to library code that waits, too — at
    /// [`stop_poll`](OrbConfig::stop_poll) granularity, which is the interval
    /// the servers themselves look at their flags on, so polling faster would
    /// only burn a core.
    ///
    /// # What "quiet" means, exactly, and what it does not prove
    ///
    /// Every live server reports
    /// [`ServerStats::active`](crate::server::ServerStats::active) `== 0` — no
    /// connection thread is left — **and**
    /// [`ServerStats::serving`](crate::server::ServerStats::serving) `== 0` —
    /// no accept loop is left either. The second is why `serving` exists:
    /// `active` reaches zero the moment the last connection thread drops its
    /// slot, while the accept loop may still be inside its final `stop_poll`
    /// sleep.
    ///
    /// **It still does not prove `serve_shared` has returned to its caller.**
    /// The counter is decremented on the way out of the loop, and the few
    /// instructions after that — restoring the listener to blocking, returning
    /// through `thread::scope` — are not covered. That is a real gap of
    /// microseconds, not milliseconds, and it is written down rather than
    /// smoothed over (D032 §9).
    ///
    /// Returns `true` immediately if this ORB handed out no live server.
    ///
    /// *잠자는 폴링이다 — 기다리지 않는 대기 루프에 대한 규칙은 기다리는 라이브러리
    /// 코드에도 적용된다. 조용해졌다는 것이 `serve_shared`가 호출자에게 돌아갔음을
    /// 증명하지는 않는다.*
    pub fn wait_until_stopped(&self, deadline: Duration) -> bool {
        let give_up = Instant::now() + deadline;
        loop {
            let quiet = {
                let handouts = self.handouts.servers.lock().unwrap_or_else(|p| p.into_inner());
                handouts.iter().all(|h| {
                    // A dropped `Server` is quiet by construction.
                    h.stop.upgrade().is_none() || (h.stats.active() == 0 && h.stats.serving() == 0)
                })
            };
            if quiet {
                return true;
            }
            let left = give_up.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            std::thread::sleep(self.config.stop_poll().min(left));
        }
    }
}

/// Why [`Orb::string_to_object`] could not produce a reference.
///
/// Four causes, kept apart because they need four different fixes — a string
/// that is not a reference at all, a malformed `IOR:` body, a malformed URL,
/// and a well-formed `corbaloc:rir:` name nothing registered. Collapsing them
/// into one string is how a caller ends up guessing which, and **every variant
/// carries the string it was given**, so no refusal here is anonymous.
///
/// This type replaced `ResolveError` (D019 step 1) rather than joining it:
/// two error types for *"I gave you a string, give me a reference"* is the same
/// duplication [`Orb::string_to_object`] exists to remove, one layer up.
#[derive(Debug)]
pub enum StringToObjectError {
    /// Neither `IOR:` nor any object URL scheme.
    NotAReferenceString {
        /// The string, as given.
        text: String,
    },
    /// An `IOR:` prefix over a body that is not a marshalled reference.
    Ior {
        /// The string, as given.
        text: String,
        /// What the `IOR:<hex>` reader made of it.
        cause: crate::Error,
    },
    /// A recognised URL scheme, malformed after it.
    Url {
        /// The string, as given.
        text: String,
        /// What the URL parser made of it, carrying §7.6.10.3's minor code.
        cause: crate::naming::UrlError,
    },
    /// A well-formed `corbaloc:rir:<ObjectId>` this ORB has no entry for.
    Name {
        /// The string, as given.
        text: String,
        /// The refusal, which names the `ObjectId`.
        cause: InvalidName,
    },
    /// A `corbaname:` URL carrying a name: the object it denotes can only be
    /// had by calling `resolve` on the naming service the URL addresses, and
    /// this operation does not dial. See [`Orb::string_to_object`].
    NeedsANamingCall {
        /// The string, as given.
        text: String,
        /// The stringified name that would have to be resolved.
        name: String,
    },
}

impl StringToObjectError {
    /// The string that was handed to [`Orb::string_to_object`].
    pub fn text(&self) -> &str {
        match self {
            StringToObjectError::NotAReferenceString { text }
            | StringToObjectError::Ior { text, .. }
            | StringToObjectError::Url { text, .. }
            | StringToObjectError::Name { text, .. }
            | StringToObjectError::NeedsANamingCall { text, .. } => text,
        }
    }
}

impl std::fmt::Display for StringToObjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Long strings are the normal case here — a stringified IOR runs to
        // hundreds of hex digits — so the head is enough to identify it and the
        // whole thing would bury the reason.
        let text = self.text();
        let shown: String = if text.chars().count() > 72 {
            format!("{}…", text.chars().take(72).collect::<String>())
        } else {
            text.to_owned()
        };
        match self {
            StringToObjectError::NotAReferenceString { .. } => write!(
                f,
                "{shown:?} is not a stringified object reference: it begins with neither \
                 \"IOR:\" nor a corbaloc:/corbaname: scheme (CORBA 3.4 §8.2.2.2)"
            ),
            StringToObjectError::Ior { cause, .. } => {
                write!(f, "{shown:?} has an \"IOR:\" prefix but does not decode: {cause}")
            }
            StringToObjectError::Url { cause, .. } => {
                write!(f, "{shown:?} is a malformed object URL: {cause}")
            }
            StringToObjectError::Name { cause, .. } => {
                write!(f, "{shown:?} could not be resolved: {cause}")
            }
            StringToObjectError::NeedsANamingCall { name, .. } => write!(
                f,
                "{shown:?} denotes the object bound under {name:?} in the naming service it \
                 addresses (CORBA 3.4 Part 2 §7.6.10.5), and producing it takes a resolve call \
                 this operation does not make; connect with NamingContext::from_url and resolve \
                 the name"
            ),
        }
    }
}

impl std::error::Error for StringToObjectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            StringToObjectError::NotAReferenceString { .. }
            | StringToObjectError::NeedsANamingCall { .. } => None,
            StringToObjectError::Ior { cause, .. } => Some(cause),
            StringToObjectError::Url { cause, .. } => Some(cause),
            StringToObjectError::Name { cause, .. } => Some(cause),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IiopProfile, Version};

    fn ior(key: &[u8]) -> Ior {
        Ior {
            type_id: "IDL:omg.org/CosNaming/NamingContextExt:1.0".into(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: "192.0.2.1".into(),
                port: 2809,
                object_key: key.to_vec(),
                components: Vec::new(),
            }],
        }
    }

    /// The gap this module closes, stated as the round trip it makes possible:
    /// the URL that carries no address is answered from the table.
    #[test]
    fn rir_resolves_through_the_table_that_was_registered() {
        let mut orb = Orb::new();
        orb.register_initial_reference("NameService", ior(b"NameService")).unwrap();
        let url = ObjectUrl::parse("corbaloc:rir:NameService").unwrap();
        assert_eq!(orb.resolve_url(&url, "IDL:x:1.0").unwrap(), ior(b"NameService"));
        // …and the bare `corbaloc:rir:` form, which §7.6.10.3 defaults to
        // `NameService`, reaches the same entry.
        let bare = ObjectUrl::parse("corbaloc:rir:").unwrap();
        assert_eq!(orb.resolve_url(&bare, "IDL:x:1.0").unwrap(), ior(b"NameService"));
    }

    /// Negative control 1, as a test: **empty the table and the refusal still
    /// names `NameService`** — it is not a panic and it is not a silent `None`.
    #[test]
    fn an_empty_table_refuses_nameservice_by_name() {
        let orb = Orb::new();
        let err = orb.resolve_initial_reference("NameService").unwrap_err();
        let said = err.to_string();
        assert!(said.contains("\"NameService\""), "the refusal must name the key: {said}");
        assert!(said.contains("§8.5.2"), "and cite the sub clause that reserves it: {said}");
        assert!(said.contains("registered nothing"), "and say the table is empty: {said}");
        assert_eq!(
            err,
            InvalidName::NotRegistered {
                key: "NameService".into(),
                reserved: true,
                known: Vec::new()
            }
        );
        // Through the URL layer, which is where the gap was.
        let url = ObjectUrl::parse("corbaloc:rir:NameService").unwrap();
        assert!(
            orb.resolve_url(&url, "IDL:x:1.0").unwrap_err().to_string().contains("NameService")
        );
    }

    /// Negative control 2: a name that was never registered is refused *by
    /// that name*, and the sentence tells a reserved id apart from an invented
    /// one because the two need different fixes.
    #[test]
    fn an_unregistered_name_is_refused_by_name() {
        let mut orb = Orb::new();
        orb.register_initial_reference("NameService", ior(b"NameService")).unwrap();

        let reserved = orb.resolve_initial_reference("InterfaceRepository").unwrap_err();
        let said = reserved.to_string();
        assert!(said.contains("\"InterfaceRepository\""), "{said}");
        assert!(said.contains("an ObjectId CORBA 3.4 §8.5.2 reserves"), "{said}");
        assert!(said.contains("this ORB has registered NameService"), "{said}");

        let invented = orb.resolve_initial_reference("NoSuchService").unwrap_err();
        let said = invented.to_string();
        assert!(said.contains("\"NoSuchService\""), "{said}");
        assert!(said.contains("not an ObjectId CORBA 3.4 §8.5.2 reserves"), "{said}");
    }

    /// Negative control 3: the two forms that already worked have not moved.
    ///
    /// **What this can and cannot see.** `resolve_url` and `to_ior` share
    /// [`crate::naming::addressed_ior`], so comparing them is a *routing* pin,
    /// not a *value* pin: it goes red if the ORB ever starts diverting an
    /// addressed URL somewhere else, and it stays green under any change to
    /// the construction itself, because both sides move together. Measured
    /// 2026-08-25 by mutating the port in `addressed_ior` — these eight cases
    /// stayed green and `naming::tests::address_list_becomes_multiple_profiles`,
    /// `naming::tests::url_becomes_a_parsable_ior` and
    /// `naming_server::tests::a_url_from_to_url_resolves_against_the_server_that_made_it`
    /// went red. **The value pins live in `naming.rs` and stay there**; this
    /// one adds the concrete assertion below so it is not purely a tautology,
    /// and otherwise says only what it can say.
    #[test]
    fn addressed_urls_resolve_exactly_as_to_ior_does() {
        let orb = Orb::new();
        // One concrete value, so a reader is not left thinking the loop below
        // pins what the URL means. What it means is pinned in `naming.rs`.
        let one = ObjectUrl::parse("corbaloc:iiop:1.2@10.0.0.1:9999/Echo").unwrap();
        let built = orb.resolve_url(&one, "IDL:spike/Echo:1.0").unwrap();
        assert_eq!(built.type_id, "IDL:spike/Echo:1.0");
        assert_eq!(built.profiles.len(), 1);
        assert_eq!(built.profiles[0].host, "10.0.0.1");
        assert_eq!(built.profiles[0].port, 9999);
        assert_eq!(built.profiles[0].object_key, b"Echo");
        assert_eq!(built.profiles[0].version, Version::V1_2);

        for text in [
            "corbaloc::example.test/NameService",
            "corbaloc:iiop:1.2@10.0.0.1:9999/Echo",
            "corbaloc::a.test:1111,:b.test:2222,iiop:1.1@c.test/Key",
            "corbaloc:iiop:[1080::8:800:200C:417A]:88/Key",
            "corbaloc::h/a%20b%2Fc%00d",
            "corbaname::host:2809/NameService#spike/Echo",
            "corbaname::host",
        ] {
            let url = ObjectUrl::parse(text).unwrap();
            let direct = url.to_ior("IDL:x:1.0").expect("an addressed URL still builds an IOR");
            let through = orb.resolve_url(&url, "IDL:x:1.0").expect("and resolves the same way");
            assert_eq!(direct, through, "{text}");
        }
        // An empty ORB does not change them: they never consult the table.
        assert!(Orb::new().list_initial_services().is_empty());
    }

    /// §8.5.2's other operation. Nothing registers itself, so a fresh ORB lists
    /// nothing; the order is the map's and therefore stable.
    #[test]
    fn list_initial_services_reports_exactly_what_was_registered() {
        let mut orb = Orb::new();
        assert!(orb.list_initial_services().is_empty(), "nothing registers itself");
        orb.register_initial_reference("NameService", ior(b"NS")).unwrap();
        orb.register_initial_reference("InterfaceRepository", ior(b"IFR")).unwrap();
        orb.register_initial_reference("VendorThing", ior(b"V")).unwrap();
        assert_eq!(
            orb.list_initial_services(),
            ["InterfaceRepository", "NameService", "VendorThing"]
        );
    }

    /// §16.10.1's two conditions on the registering half, and the deliberate
    /// reading of the third (a replacement is spelled out, never implied).
    #[test]
    fn registration_refuses_an_empty_id_and_a_taken_one() {
        let mut orb = Orb::new();
        assert_eq!(orb.register_initial_reference("", ior(b"x")), Err(InvalidName::EmptyId));
        assert!(orb.list_initial_services().is_empty(), "a refused registration changed nothing");

        orb.register_initial_reference("NameService", ior(b"first")).unwrap();
        let again = orb.register_initial_reference("NameService", ior(b"second")).unwrap_err();
        assert_eq!(again, InvalidName::AlreadyRegistered { key: "NameService".into() });
        assert!(again.to_string().contains("\"NameService\""), "{again}");
        assert_eq!(
            orb.resolve_initial_reference("NameService").unwrap(),
            ior(b"first"),
            "a refused re-registration must not have overwritten the entry"
        );

        // Replacing is possible, and it has to be said out loud.
        assert_eq!(orb.unregister_initial_reference("NameService"), Some(ior(b"first")));
        orb.register_initial_reference("NameService", ior(b"second")).unwrap();
        assert_eq!(orb.resolve_initial_reference("NameService").unwrap(), ior(b"second"));
    }

    /// §8.5.3.1 requires that an ORB be configurable to return an *arbitrary*
    /// object reference, so the reserved list must not be a whitelist.
    #[test]
    fn a_name_outside_table_8_1_may_still_be_registered() {
        let mut orb = Orb::new();
        orb.register_initial_reference("VendorPrivateThing", ior(b"v")).unwrap();
        assert_eq!(orb.resolve_initial_reference("VendorPrivateThing").unwrap(), ior(b"v"));
        assert!(!is_reserved_object_id("VendorPrivateThing"));
        for id in RESERVED_OBJECT_IDS {
            assert!(is_reserved_object_id(id), "{id} is in the list §8.5.2 gives");
        }
        // Case-sensitive: §8.5.2 gives the spellings exactly.
        assert!(!is_reserved_object_id("nameservice"));
    }

    /// §8.2.2's one operation reads all three forms, and the caller does not
    /// have to know which one it is holding. That is the whole claim of step 2.
    #[test]
    fn string_to_object_decides_between_the_three_forms() {
        let mut orb = Orb::new();
        let registered = ior(b"NS");
        orb.register_initial_reference("NameService", registered.clone()).unwrap();

        // 1. the hex blob — and its repository id survives, because the string
        //    carried one.
        let hex = registered.to_stringified().unwrap();
        let from_hex = orb.string_to_object(&hex).unwrap();
        assert_eq!(from_hex, registered);
        assert_eq!(from_hex.type_id, "IDL:omg.org/CosNaming/NamingContextExt:1.0");

        // 2. an addressed URL — no type in the string, so no type invented.
        let from_url = orb.string_to_object("corbaloc:iiop:1.2@10.0.0.1:9999/Echo").unwrap();
        assert_eq!(from_url.type_id, "", "a URL carries no repository id; §8.5.2 narrows later");
        assert_eq!(from_url.profiles[0].port, 9999);
        assert_eq!(from_url.profiles[0].object_key, b"Echo");
        // A corbaname URL with no name denotes the naming context itself, and
        // needs no call to produce. One that carries a name does — see
        // `a_corbaname_carrying_a_name_is_refused_rather_than_answered_wrongly`.
        assert_eq!(orb.string_to_object("corbaname::host").unwrap().profiles[0].host, "host");

        // 3. the form that carries no address at all, which only works because
        //    step 1 gave the ORB a table.
        assert_eq!(orb.string_to_object("corbaloc:rir:NameService").unwrap(), registered);
        assert_eq!(orb.string_to_object("corbaloc:rir:").unwrap(), registered);

        // Leading and trailing whitespace: a string read from a file has a
        // newline on it, which is how every fixture in this repository holds an
        // IOR.
        assert_eq!(orb.string_to_object(&format!("  {hex}\n")).unwrap(), registered);
    }

    /// §7.6.9: *"the case of a stringified IOR is not significant."* The
    /// sniffing that picks the branch has to agree with the parser it picks.
    #[test]
    fn the_ior_prefix_is_matched_case_insensitively() {
        let orb = Orb::new();
        let hex = ior(b"K").to_stringified().unwrap();
        for spelling in ["IOR:", "ior:", "Ior:", "iOr:"] {
            let restyled = format!("{spelling}{}", &hex[4..]);
            assert_eq!(orb.string_to_object(&restyled).unwrap(), ior(b"K"), "{spelling}");
        }
    }

    /// The four causes are kept apart, and **every refusal names the string**.
    #[test]
    fn string_to_object_keeps_its_four_causes_apart_and_names_the_string() {
        let orb = Orb::new();

        let not_one = orb.string_to_object("hello world").unwrap_err();
        assert!(matches!(not_one, StringToObjectError::NotAReferenceString { .. }));
        assert!(not_one.to_string().contains("hello world"), "{not_one}");
        assert!(not_one.to_string().contains("§8.2.2.2"), "{not_one}");
        assert_eq!(not_one.text(), "hello world");
        // An unknown *scheme* is the same cause: it is not a reference string.
        assert!(matches!(
            orb.string_to_object("http://example.test/x"),
            Err(StringToObjectError::NotAReferenceString { .. })
        ));

        let bad_hex = orb.string_to_object("IOR:zzz").unwrap_err();
        assert!(matches!(bad_hex, StringToObjectError::Ior { .. }));
        assert!(bad_hex.to_string().contains("IOR:zzz"), "{bad_hex}");

        let bad_url = orb.string_to_object("corbaloc::h:notaport/K").unwrap_err();
        assert!(matches!(bad_url, StringToObjectError::Url { .. }));
        let said = bad_url.to_string();
        assert!(said.contains("corbaloc::h:notaport/K"), "{said}");
        assert!(said.contains("BAD_PARAM minor 8"), "the URL layer's own code survives: {said}");

        let missing = orb.string_to_object("corbaloc:rir:TradingService").unwrap_err();
        assert!(matches!(missing, StringToObjectError::Name { .. }));
        let said = missing.to_string();
        assert!(said.contains("corbaloc:rir:TradingService"), "{said}");
        assert!(said.contains("\"TradingService\""), "the InvalidName still names it: {said}");

        // A stringified IOR is hundreds of characters; the head identifies it
        // and the reason must not be buried behind it.
        let long = format!("IOR:{}", "0".repeat(400));
        let said = orb.string_to_object(&long).unwrap_err().to_string();
        assert!(said.contains('…'), "a long string is elided: {said}");
        assert!(said.len() < 300, "and the reason stays visible: {said}");
    }

    /// §8.2.2's conformance sentence, which is the only thing the sub clause
    /// promises: `string_to_object(object_to_string(obj))` is `obj`.
    ///
    /// Run over every shape this crate can hold — nil, one profile, several,
    /// IPv6, an empty type id, a key with non-UTF-8 bytes in it — because the
    /// round trip is a property of the pair and not of one example.
    #[test]
    fn object_to_string_round_trips_through_string_to_object() {
        let orb = Orb::new();
        let profile = |host: &str, port: u16, key: &[u8], v: Version| IiopProfile {
            version: v,
            host: host.into(),
            port,
            object_key: key.to_vec(),
            components: Vec::new(),
        };
        let cases = [
            Ior { type_id: String::new(), profiles: Vec::new() }, // the nil reference
            ior(b"one"),
            Ior {
                type_id: "IDL:spike/Echo:1.0".into(),
                profiles: vec![
                    profile("a.test", 1111, b"A", Version::V1_0),
                    profile("::1", 2222, b"B", Version::V1_1),
                    profile("c.test", 3333, b"C", Version::V1_2),
                ],
            },
            Ior {
                // No repository id, which is exactly what a URL-built
                // reference looks like coming out of `string_to_object`.
                type_id: String::new(),
                profiles: vec![profile("h", 4001, &[0x00, 0xFF, 0x80, b'/'], Version::V1_2)],
            },
        ];
        for obj in cases {
            let s = orb.object_to_string(&obj).unwrap();
            assert!(s.starts_with("IOR:"), "§8.2.2 asks for the interoperable form, got {s:?}");
            assert_eq!(orb.string_to_object(&s).unwrap(), obj, "round trip failed for {s}");
        }
    }

    /// D019 step 3, end to end: a deployment's `-ORBInitRef` arguments become
    /// entries in the table, so `corbaloc:rir:NameService` answers **without a
    /// line of Rust having named the service.**
    #[test]
    fn orb_init_ref_arguments_populate_the_table() {
        let argv = [
            "serve",
            "-ORBInitRef",
            "NameService=corbaloc::h.test:2809/NameService",
            "-ORBInitRef",
            "InterfaceRepository=corbaloc:iiop:1.2@h.test:4001/IFR",
            "--verbose",
        ];
        let (config, rest) = OrbConfig::from_orb_args(&argv).unwrap();
        assert_eq!(rest, ["serve", "--verbose"], "§8.5.1 removes what it recognised");

        let orb = Orb::with_config(config).unwrap();
        assert_eq!(orb.list_initial_services(), ["InterfaceRepository", "NameService"]);

        let ns = orb.string_to_object("corbaloc:rir:NameService").unwrap();
        assert_eq!(ns.profiles[0].host, "h.test");
        assert_eq!(ns.profiles[0].port, 2809);
        assert_eq!(ns.profiles[0].object_key, b"NameService");

        let ifr = orb.resolve_initial_reference("InterfaceRepository").unwrap();
        assert_eq!(ifr.profiles[0].port, 4001);
        assert_eq!(ifr.profiles[0].version, Version::V1_2);

        // A name nobody configured is still refused by name.
        assert!(
            orb.resolve_initial_reference("TradingService")
                .unwrap_err()
                .to_string()
                .contains("\"TradingService\"")
        );
    }

    /// An `IOR:` blob is one of the forms §8.5.3.2 shows, and it goes through
    /// the same `string_to_object` — which is why step 2 came first.
    #[test]
    fn an_init_ref_may_be_a_stringified_ior() {
        let published = ior(b"NS");
        let hex = published.to_stringified().unwrap();
        let (config, _) =
            OrbConfig::from_orb_args(&["-ORBInitRef".to_owned(), format!("NameService={hex}")])
                .unwrap();
        let orb = Orb::with_config(config).unwrap();
        assert_eq!(orb.string_to_object("corbaloc:rir:").unwrap(), published);
    }

    /// Refused whole: the second of two `-ORBInitRef`s cannot be read, and the
    /// **first one is not registered either**. There is no half-configured ORB.
    #[test]
    fn a_configuration_with_one_bad_reference_registers_nothing() {
        let (config, _) = OrbConfig::from_orb_args(&[
            "-ORBInitRef",
            "NameService=corbaloc::good.test/NameService",
            "-ORBInitRef",
            "InterfaceRepository=not a reference at all",
        ])
        .unwrap();
        let err = Orb::with_config(config).unwrap_err();
        assert!(matches!(err, ConfigError::InitRefUnreadable { .. }));
        let said = err.to_string();
        assert!(said.contains("InterfaceRepository"), "names the ObjectId: {said}");
        assert!(said.contains("not a reference at all"), "names the URL: {said}");
        assert!(said.contains("nothing was registered"), "says what it did: {said}");
        assert!(said.contains("§8.2.2.2"), "carries string_to_object's own reason: {said}");
    }

    /// The numbers have one home, and an unconfigured ORB answers exactly the
    /// constants the crate used before any of this existed.
    #[test]
    fn the_configuration_travels_with_the_orb_and_defaults_to_todays_values() {
        assert!(Orb::new().config().is_empty());
        assert_eq!(Orb::new().config().max_message_size(), crate::DEFAULT_MAX_MESSAGE_SIZE);

        let (config, _) =
            OrbConfig::from_orb_args(&["-ORBmaxMessageSize", "4096", "-ORBmaxForwardHops", "2"])
                .unwrap();
        let orb = Orb::with_config(config).unwrap();
        assert_eq!(orb.config().max_message_size(), 4096);
        assert_eq!(orb.config().max_forward_hops(), 2);
        // …and everything not named stayed where it was.
        assert_eq!(orb.config().stop_poll(), crate::server::STOP_POLL);
        assert_eq!(orb.config().max_connections(), crate::server::DEFAULT_MAX_CONNECTIONS);
    }

    /// A `corbaname:` URL carrying a name denotes the object bound under it,
    /// which takes a call. The wrong answer was available and cheap — hand back
    /// the naming context, which is what [`ObjectUrl::to_ior`] builds for this
    /// form — and it is wrong *silently*, which is why it is refused instead.
    #[test]
    fn a_corbaname_carrying_a_name_is_refused_rather_than_answered_wrongly() {
        let orb = Orb::new();
        let err =
            orb.string_to_object("corbaname::h.test:2809/NameService#spike/Echo").unwrap_err();
        assert!(matches!(err, StringToObjectError::NeedsANamingCall { .. }));
        let said = err.to_string();
        assert!(said.contains("spike/Echo"), "the refusal names the name: {said}");
        assert!(said.contains("§7.6.10.5"), "and cites the sub clause: {said}");
        assert!(said.contains("NamingContext::from_url"), "and says what does work: {said}");

        // The two-step path this points at is unchanged and still builds the
        // naming context, which is the right object *for that step*.
        let url = ObjectUrl::parse("corbaname::h.test:2809/NameService#spike/Echo").unwrap();
        let ctx = url.to_ior("IDL:x:1.0").expect("the context is still addressable");
        assert_eq!(ctx.profiles[0].object_key, b"NameService");
        assert_eq!(ctx.profiles[0].port, 2809);

        // …and the fragment-less form is a reference, not a lookup, so it is
        // answered. Measured 2026-08-25: omniORB draws the line in the same
        // place.
        assert!(orb.string_to_object("corbaname::h.test:2809/NameService").is_ok());
    }

    /// `object_to_string` emits the `IOR:` form even for a reference that
    /// arrived as a URL, because §8.2.2's promise is that the string denotes
    /// *the same object* — and a URL is a bootstrap that may name a different
    /// one tomorrow.
    #[test]
    fn a_url_becomes_an_interoperable_string_not_another_url() {
        let orb = Orb::new();
        let from_url = orb.string_to_object("corbaloc:iiop:1.2@10.0.0.1:9999/Echo").unwrap();
        let s = orb.object_to_string(&from_url).unwrap();
        assert!(s.starts_with("IOR:"), "{s}");
        assert_eq!(orb.string_to_object(&s).unwrap(), from_url);
    }
}
