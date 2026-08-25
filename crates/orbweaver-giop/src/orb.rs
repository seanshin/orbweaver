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
//! # What this module is not, yet
//!
//! D019 §5 proposes four responsibilities for this object. This is the first:
//! the initial references table. The two named conversions
//! (`string_to_object` / `object_to_string`), the seven configuration numbers
//! and the handing out of transport and root POA are separate batches, and the
//! last of those is gated on the §5 shape being approved.
//!
//! *ORB가 `resolve_initial_references`의 언어를 말할 줄 알면서 그것을 대조할
//! 표를 갖고 있지 않았다. 이 모듈이 그 표다 — CORBA 3.4 §8.5.2가 정의한 평평한
//! 단일 계층 이름 공간이고, 없는 이름은 **이름을 대며** 거절한다.*

use std::collections::BTreeMap;

use crate::Ior;
use crate::naming::ObjectUrl;

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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Orb {
    initial: BTreeMap<String, Ior>,
}

impl Orb {
    /// An ORB that resolves nothing. See *Nothing registers itself* in the
    /// module docs: this is not a stub, it is the answer until a deployment
    /// registers something.
    pub fn new() -> Self {
        Self::default()
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

    /// The same, from the URL text — parsing and then resolving.
    ///
    /// # Errors
    ///
    /// [`ResolveError::Url`] if the string is not an object URL at all,
    /// [`ResolveError::Name`] if it is a `rir:` name this ORB does not answer
    /// for.
    pub fn resolve_url_str(
        &self,
        url: &str,
        type_id: &str,
    ) -> std::result::Result<Ior, ResolveError> {
        let parsed = ObjectUrl::parse(url).map_err(ResolveError::Url)?;
        self.resolve_url(&parsed, type_id).map_err(ResolveError::Name)
    }
}

/// Why [`Orb::resolve_url_str`] could not produce a reference: the URL did not
/// parse, or it named an initial reference this ORB does not have.
///
/// Two separate causes because they need different fixes — one is a typo in a
/// URL, the other is a missing registration — and collapsing them into one
/// string is how a caller ends up guessing which.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// The string is not a `corbaloc:`/`corbaname:` URL.
    Url(crate::naming::UrlError),
    /// The URL is `corbaloc:rir:<ObjectId>` and the table has no such entry.
    Name(InvalidName),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::Url(e) => write!(f, "{e}"),
            ResolveError::Name(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ResolveError {}

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

    /// The parse-and-resolve entry keeps the two causes apart, because a typo
    /// in a URL and a missing registration need different fixes.
    #[test]
    fn resolve_url_str_tells_a_bad_url_apart_from_a_missing_name() {
        let mut orb = Orb::new();
        orb.register_initial_reference("NameService", ior(b"NS")).unwrap();
        assert_eq!(
            orb.resolve_url_str("corbaloc:rir:NameService", "IDL:x:1.0").unwrap(),
            ior(b"NS")
        );
        assert!(matches!(
            orb.resolve_url_str("http://x", "IDL:x:1.0"),
            Err(ResolveError::Url(crate::naming::UrlError::BadSchemeName(_)))
        ));
        let missing = orb.resolve_url_str("corbaloc:rir:TradingService", "IDL:x:1.0").unwrap_err();
        assert!(matches!(missing, ResolveError::Name(_)));
        assert!(missing.to_string().contains("\"TradingService\""), "{missing}");
    }
}
