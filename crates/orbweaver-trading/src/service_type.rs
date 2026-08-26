//! A service type, minimally: a name, an interface repository id, and a
//! property schema **checked at registration**. `docs/decisions/D022` T3.
//!
//! # What a service type is here, and what it deliberately is not
//!
//! `CosTrading::Lookup::query`'s first parameter is a `ServiceTypeName`, so
//! `query` cannot be answered at all without one. D022 §6 T3 says to build
//! the *type* and not the repository: **there is no
//! `CosTradingRepos::ServiceTypeRepository` servant here and T3 must not add
//! one** (§7). Enumerating types is a second service and earns its place when
//! a client asks to enumerate them; answering `query` needs only that a name
//! resolves to a schema.
//!
//! *서비스 타입은 `query`의 첫 인자이므로 없으면 답할 수 없다. 저장소는 두 번째
//! 서비스이며, 열거를 요구하는 클라이언트가 나타날 때 자리를 얻는다.*
//!
//! # Why this is a side table and not a field on [`Offer`]
//!
//! [`Offer`] is ten fixed struct fields with native Rust types — there is no
//! property bag — and `orbweaver-object`'s expert and tenant services build
//! `Offer` literals. Adding an eleventh field would edit every one of those
//! literals in a crate this batch does not hold, for no gain: a service type
//! is a property *of the registration*, not of the offer's value. So the type
//! lives beside the store in [`TypedOfferStore`], which wraps [`OfferStore`]
//! and adds exactly one map — offer id to service type name. Every existing
//! caller of [`OfferStore`] is untouched and keeps working typeless.
//!
//! # The property schema, and what it can actually check
//!
//! The ten property names are a **closed set** — the [`Offer`] fields, the
//! same names [`crate::query`] and [`crate::preference`] parse — so a schema
//! selects a subset of them rather than declaring new ones. That makes the
//! schema's job narrower than the specification's and worth stating plainly:
//!
//! - **The declared kind is a check, not a definition.** A property's kind is
//!   already fixed by which field it is, so declaring `cost` as `Text` cannot
//!   change anything; it is refused at declaration
//!   ([`RefusalKind::PropertyTypeMismatch`]). This is the whole value of
//!   writing the kind down — a schema that disagrees with the engine is
//!   caught when it is written, not when a query runs.
//! - **`Mandatory` is the mode with teeth**, and only for the two fields that
//!   can be absent. `specialization` and `latency_p50` are `Option`, and a
//!   v1.0 wire registration cannot carry either (see [`Offer`]); every other
//!   field always has a value, so `Mandatory` on it is satisfied by
//!   construction. The predicate is the same `has_value` that `EXIST` and
//!   `ORDER BY` use, so a mandatory property and an `EXIST` are the same
//!   question asked at two different times.
//! - **`Readonly` is checked on heartbeat**, which is the only path that
//!   changes a registered offer. A heartbeat that moves a readonly property is
//!   refused naming the property and both values.
//!
//! # Refused by name / 이름을 붙여 거부하는 것
//!
//! - **Super types.** The specification's service types form an inheritance
//!   graph, and a `query` on a type matches offers of its subtypes. That is a
//!   graph walk, a masking rule and an `incarnation` number; none of it is
//!   needed to answer a query against a named type, and D022 §6 says
//!   *minimally*. [`ServiceType::with_super_types`] refuses, naming them, so
//!   the gap is a sentence a caller reads rather than a silent absence.
//! - **A repository id's well-formedness.** `interface_id` is stored as
//!   given. The check for `IDL:…:M.N` has a home —
//!   `orbweaver_registry::ingest::validate_repository_id` — and this crate
//!   deliberately takes no dependencies, while `orbweaver-registry` depends on
//!   `orbweaver-giop`, which is where D022 T4's servant lands. Depending on it
//!   from here would close a cycle; retyping the check here would be the
//!   escaped-fact defect `CLAUDE.md` names. So the field is opaque and this
//!   sentence says so, which is the honest third option.

use std::collections::BTreeMap;
use std::fmt;

use crate::query::{Field, Kind, Query, Selection, has_value};
use crate::{Offer, OfferStore};

/// Which class of refusal a [`Refusal`] is.
///
/// This exists so that a caller — most importantly D022 T4's wire servant,
/// which must choose between ten `CosTrading` exceptions — classifies by a
/// value this module publishes rather than by matching a substring of a
/// message this module owns. `CLAUDE.md`: *a classifier is a sentence too*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalKind {
    /// The service type name is not a legal scoped identifier.
    IllegalServiceType,
    /// No service type of that name has been declared.
    UnknownServiceType,
    /// A service type of that name is already declared.
    DuplicateServiceType,
    /// A property name that is not one of the ten [`Offer`] properties.
    IllegalPropertyName,
    /// The same property named twice in one schema.
    DuplicatePropertyName,
    /// A declared kind that disagrees with the property's actual kind.
    PropertyTypeMismatch,
    /// An offer registered without a property its type declares mandatory.
    MissingMandatoryProperty,
    /// A heartbeat that moves a property the type declares readonly.
    ReadonlyPropertyModified,
    /// A service type declaring super types, which T3 does not carry.
    UnsupportedSuperTypes,
    /// The underlying [`OfferStore`] refused — a duplicate or unknown id.
    Store,
    /// A constraint that did not parse. [`crate::lookup`].
    IllegalConstraint,
    /// A preference that did not parse, or that fought the constraint's own
    /// ordering. [`crate::lookup`].
    IllegalPreference,
    /// An import policy this trader does not implement. [`crate::lookup`].
    IllegalPolicyName,
    /// A query whose answer does not fit the caller's `how_many` or this
    /// trader's own bound, and which therefore cannot be answered without the
    /// `OfferIterator` this trader does not create. [`crate::lookup`].
    DoesNotFit,
}

/// A refusal from the typed layer, carrying the class as a value and the
/// sentence as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// Which class of refusal this is.
    pub kind: RefusalKind,
    /// The sentence. Always names the offending thing.
    pub message: String,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Refusal {}

impl Refusal {
    pub(crate) fn new(kind: RefusalKind, message: String) -> Refusal {
        Refusal { kind, message }
    }
}

/// Every property an [`Offer`] carries, in the engine's own field order.
///
/// The order is [`Offer`]'s declaration order, and it is the order a wire
/// answer projects properties in, so that two calls asking for `all` come back
/// in the same order on every ORB and every run.
pub const ALL_PROPERTIES: [&str; 10] = [
    "id",
    "specialization",
    "cost",
    "latency_p50",
    "latency_p99",
    "load",
    "residency",
    "mem_footprint",
    "placement_node",
    "route_freq",
];

/// Resolves a property name to the engine's own spelling of it, or `None` if
/// an offer does not carry it.
///
/// Goes through `Field::from_name`, so this list cannot drift from the one the
/// constraint and preference parsers accept: a property this answers `Some`
/// for is a property a constraint can name, by construction rather than by
/// two lists agreeing.
pub fn property_name(name: &str) -> Option<&'static str> {
    Field::from_name(name).map(Field::name)
}

/// The kind of value a property carries, as a schema declares it.
///
/// Mirrors the private `query::Kind` — the engine's own classification of the
/// ten [`Offer`] fields — and exists because that one is `pub(crate)` and a
/// schema is written from outside. The mapping is total and checked in both
/// directions by [`ServiceType::declare`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyKind {
    /// A string: `id`, `specialization`, `placement_node`.
    Text,
    /// A 64-bit float: `cost`, `latency_p50`, `latency_p99`, `load`.
    Float,
    /// A non-negative integer: `mem_footprint`, `route_freq`.
    Counter,
    /// A [`crate::Residency`] state.
    State,
}

impl PropertyKind {
    fn of(field: Field) -> PropertyKind {
        match field.kind() {
            Kind::Text => PropertyKind::Text,
            Kind::Float => PropertyKind::Float,
            Kind::Counter => PropertyKind::Counter,
            Kind::State => PropertyKind::State,
        }
    }

    /// The name this kind is written by in a refusal.
    pub fn name(self) -> &'static str {
        match self {
            PropertyKind::Text => "text",
            PropertyKind::Float => "float",
            PropertyKind::Counter => "counter",
            PropertyKind::State => "state",
        }
    }
}

/// How a property may be used, the four modes the specification names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyMode {
    /// May be absent, may change on heartbeat.
    Normal,
    /// May be absent; may not change once registered.
    Readonly,
    /// Must be present at registration; may change on heartbeat.
    Mandatory,
    /// Must be present at registration and may not change.
    MandatoryReadonly,
}

impl PropertyMode {
    /// Whether an offer must carry a value for this property to register.
    pub fn is_mandatory(self) -> bool {
        matches!(self, PropertyMode::Mandatory | PropertyMode::MandatoryReadonly)
    }

    /// Whether a heartbeat may move this property.
    pub fn is_readonly(self) -> bool {
        matches!(self, PropertyMode::Readonly | PropertyMode::MandatoryReadonly)
    }
}

/// One row of a service type's property schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertySchema {
    /// One of the ten [`Offer`] property names.
    pub name: String,
    /// The kind the schema expects. Checked against the engine's own.
    pub kind: PropertyKind,
    /// How the property may be used.
    pub mode: PropertyMode,
}

impl PropertySchema {
    /// A schema row, by property name.
    pub fn new(name: impl Into<String>, kind: PropertyKind, mode: PropertyMode) -> PropertySchema {
        PropertySchema { name: name.into(), kind, mode }
    }
}

/// A service type: a name, the repository id of the interface an offer of
/// this type implements, and a property schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceType {
    name: String,
    interface_id: String,
    properties: Vec<PropertySchema>,
}

impl ServiceType {
    /// Declares a service type, refusing everything §"Refused by name" and the
    /// property-schema rules above describe.
    ///
    /// The name must be a scoped identifier — one or more IDL identifiers
    /// joined by `::`, each starting with a letter — because the
    /// specification says `ServiceTypeName` has *"similar structure to
    /// IR::Identifier"*. A leading `::` is refused rather than tolerated: a
    /// service type name is not a scoped name being resolved, so there is no
    /// root to be absolute against.
    pub fn declare(
        name: impl Into<String>,
        interface_id: impl Into<String>,
        properties: Vec<PropertySchema>,
    ) -> Result<ServiceType, Refusal> {
        let name = name.into();
        let interface_id = interface_id.into();
        check_service_type_name(&name)?;

        if interface_id.is_empty() {
            return Err(Refusal::new(
                RefusalKind::IllegalServiceType,
                format!(
                    "service type {name:?} was declared with an empty interface repository id: \
                     a service type names the interface its offers implement, and that name is \
                     the key (a repository id such as \"IDL:moe/Expert:1.0\")"
                ),
            ));
        }

        let mut seen: BTreeMap<&str, ()> = BTreeMap::new();
        for prop in &properties {
            let Some(field) = Field::from_name(&prop.name) else {
                return Err(Refusal::new(
                    RefusalKind::IllegalPropertyName,
                    format!(
                        "service type {name:?} declares a property {:?}, which is not one of the \
                         offer properties: {}",
                        prop.name,
                        crate::query::FIELD_LIST
                    ),
                ));
            };
            if seen.insert(field.name(), ()).is_some() {
                return Err(Refusal::new(
                    RefusalKind::DuplicatePropertyName,
                    format!(
                        "service type {name:?} declares the property {:?} twice: a schema says \
                         one thing about each property",
                        prop.name
                    ),
                ));
            }
            let actual = PropertyKind::of(field);
            if actual != prop.kind {
                return Err(Refusal::new(
                    RefusalKind::PropertyTypeMismatch,
                    format!(
                        "service type {name:?} declares the property {:?} as {}, but an offer \
                         carries it as {}: the kind is fixed by which property it is, so a schema \
                         that disagrees with the engine is refused where it is written",
                        prop.name,
                        prop.kind.name(),
                        actual.name()
                    ),
                ));
            }
        }

        Ok(ServiceType { name, interface_id, properties })
    }

    /// The refusal for a service type with super types, which T3 does not
    /// carry. Exists so the absence is a sentence rather than a silence: see
    /// the module's §"Refused by name".
    pub fn with_super_types(
        name: impl Into<String>,
        _interface_id: impl Into<String>,
        super_types: &[String],
        _properties: Vec<PropertySchema>,
    ) -> Result<ServiceType, Refusal> {
        let name = name.into();
        Err(Refusal::new(
            RefusalKind::UnsupportedSuperTypes,
            format!(
                "service type {name:?} declares the super types [{}]: this trader carries no \
                 service type inheritance, so a query on a super type would silently not match \
                 offers of this type — declare each type with its own full property schema, or \
                 name a client that needs subtype matching and it earns the graph",
                super_types.join(", ")
            ),
        ))
    }

    /// The type's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The repository id of the interface an offer of this type implements.
    /// Stored as given — see the module's §"Refused by name".
    pub fn interface_id(&self) -> &str {
        &self.interface_id
    }

    /// The property schema, in declaration order.
    pub fn properties(&self) -> &[PropertySchema] {
        &self.properties
    }
}

/// Checks a `ServiceTypeName`. Split out so the check has one home: both
/// [`ServiceType::declare`] and every lookup path call it, and a name that is
/// illegal is a different refusal from a name that is merely unknown.
fn check_service_type_name(name: &str) -> Result<(), Refusal> {
    let illegal = |why: &str| {
        Err(Refusal::new(
            RefusalKind::IllegalServiceType,
            format!(
                "{name:?} is not a legal service type name: {why}. A service type name is one or \
                 more identifiers joined by \"::\", each beginning with a letter and continuing \
                 with letters, digits or underscores — for example \"moe::Expert\""
            ),
        ))
    };
    if name.is_empty() {
        return illegal("it is empty");
    }
    if name.starts_with("::") {
        return illegal(
            "it begins with \"::\", but a service type name is not resolved against a root",
        );
    }
    for part in name.split("::") {
        if part.is_empty() {
            return illegal("it has an empty component");
        }
        let mut chars = part.chars();
        let first = chars.next().expect("a non-empty component has a first character");
        if !first.is_ascii_alphabetic() {
            return illegal("a component begins with something that is not a letter");
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return illegal(
                "a component carries something that is not a letter, digit or underscore",
            );
        }
    }
    Ok(())
}

/// An [`OfferStore`] whose registrations carry a service type, and whose
/// selections can be narrowed to one.
///
/// This is the shape D022 T4's `Lookup::query` needs: `query`'s first
/// parameter is a type name, so an untyped store cannot answer it. Every
/// method that takes a type name refuses an illegal one and an unknown one
/// differently, because the specification's exceptions do.
#[derive(Debug, Default)]
pub struct TypedOfferStore {
    types: BTreeMap<String, ServiceType>,
    store: OfferStore,
    /// Offer id to service type name. The side table the module header
    /// explains.
    typed: BTreeMap<String, String>,
}

impl TypedOfferStore {
    /// An empty store with no declared types.
    pub fn new() -> TypedOfferStore {
        TypedOfferStore::default()
    }

    /// Declares a service type. Refuses a name already declared — a schema
    /// that changes under registered offers is how an offer stops satisfying
    /// the type it registered against.
    pub fn declare(&mut self, service_type: ServiceType) -> Result<(), Refusal> {
        if self.types.contains_key(service_type.name()) {
            return Err(Refusal::new(
                RefusalKind::DuplicateServiceType,
                format!(
                    "service type {:?} is already declared: a schema that changed under the \
                     offers registered against it would leave them unchecked",
                    service_type.name()
                ),
            ));
        }
        self.types.insert(service_type.name().to_owned(), service_type);
        Ok(())
    }

    /// The declared types, by ascending name.
    pub fn types(&self) -> impl Iterator<Item = &ServiceType> {
        self.types.values()
    }

    /// Resolves a type name, refusing an illegal name and an unknown name
    /// with different [`RefusalKind`]s.
    pub fn service_type(&self, name: &str) -> Result<&ServiceType, Refusal> {
        check_service_type_name(name)?;
        self.types.get(name).ok_or_else(|| {
            let known: Vec<&str> = self.types.keys().map(String::as_str).collect();
            let known = if known.is_empty() {
                "no service type has been declared".to_owned()
            } else {
                format!("the declared types are {}", known.join(", "))
            };
            Refusal::new(
                RefusalKind::UnknownServiceType,
                format!("no service type {name:?} is declared here: {known}"),
            )
        })
    }

    /// Registers an offer against a declared type, checking the schema.
    ///
    /// Order of refusal is deliberate: the type is resolved first, then the
    /// schema is checked, and only then is the offer handed to the store. An
    /// offer that fails the schema never reaches the store, so a failed
    /// registration leaves nothing behind.
    pub fn register(&mut self, type_name: &str, offer: Offer) -> Result<(), Refusal> {
        let service_type = self.service_type(type_name)?;
        for prop in &service_type.properties {
            if !prop.mode.is_mandatory() {
                continue;
            }
            let field = Field::from_name(&prop.name)
                .expect("a declared schema's property names were checked at declaration");
            if !has_value(&offer, field) {
                return Err(Refusal::new(
                    RefusalKind::MissingMandatoryProperty,
                    format!(
                        "offer {:?} cannot register as {type_name:?}: the type declares {:?} \
                         mandatory and this offer carries no value for it",
                        offer.id, prop.name
                    ),
                ));
            }
        }
        let id = offer.id.clone();
        self.store.register(offer).map_err(|e| Refusal::new(RefusalKind::Store, e.message))?;
        self.typed.insert(id, type_name.to_owned());
        Ok(())
    }

    /// Updates a registered offer, refusing a move of any property its type
    /// declares readonly.
    pub fn heartbeat(&mut self, update: Offer) -> Result<(), Refusal> {
        if let Some(type_name) = self.typed.get(&update.id) {
            let service_type = self
                .types
                .get(type_name)
                .expect("a recorded type name was declared, and types are never removed");
            let current = self
                .store
                .get(&update.id)
                .expect("a typed offer is in the store until it is deregistered");
            for prop in &service_type.properties {
                let field = Field::from_name(&prop.name)
                    .expect("a declared schema's property names were checked at declaration");
                if prop.mode.is_readonly() && !same_value(current, &update, field) {
                    return Err(Refusal::new(
                        RefusalKind::ReadonlyPropertyModified,
                        format!(
                            "offer {:?} is registered as {type_name:?}, which declares {:?} \
                             readonly, and this heartbeat moves it from {} to {}",
                            update.id,
                            prop.name,
                            describe_value(current, field),
                            describe_value(&update, field)
                        ),
                    ));
                }
                if prop.mode.is_mandatory() && !has_value(&update, field) {
                    return Err(Refusal::new(
                        RefusalKind::MissingMandatoryProperty,
                        format!(
                            "offer {:?} is registered as {type_name:?}, which declares {:?} \
                             mandatory, and this heartbeat drops it",
                            update.id, prop.name
                        ),
                    ));
                }
            }
        }
        self.store.heartbeat(update).map_err(|e| Refusal::new(RefusalKind::Store, e.message))
    }

    /// Removes an offer and its type record.
    pub fn deregister(&mut self, id: &str) -> bool {
        self.typed.remove(id);
        self.store.deregister(id)
    }

    /// The service type an offer registered against, if it registered through
    /// this layer.
    pub fn type_of(&self, id: &str) -> Option<&str> {
        self.typed.get(id).map(String::as_str)
    }

    /// The underlying store, for the untyped operations (`pin`, `add_hit`,
    /// `decay_all`, the loading policy) that a service type says nothing
    /// about.
    pub fn store(&self) -> &OfferStore {
        &self.store
    }

    /// The underlying store, mutably. Registration deliberately does not go
    /// through here: [`OfferStore::register`] reached this way would put an
    /// untyped offer in a typed store, which every selection would then skip.
    pub fn store_mut(&mut self) -> &mut OfferStore {
        &mut self.store
    }

    /// Narrows a [`Selection`] to the offers registered against `type_name`.
    ///
    /// The constraint decides membership and the type decides the population,
    /// and doing it in this order rather than filtering first is what keeps
    /// [`Selection::unanswerable`] meaning what it means: an offer of another
    /// type is not an offer the query could not answer about.
    pub fn narrow<'a>(&'a self, type_name: &str, mut selection: Selection<'a>) -> Selection<'a> {
        let of_type = |o: &&Offer| self.typed.get(&o.id).map(String::as_str) == Some(type_name);
        selection.matched.retain(of_type);
        selection.unanswerable.retain(of_type);
        selection.unranked.retain(of_type);
        selection
    }

    /// Runs a constraint over the offers of one type.
    pub fn select<'a>(&'a self, type_name: &str, query: &Query) -> Result<Selection<'a>, Refusal> {
        self.service_type(type_name)?;
        Ok(self.narrow(type_name, query.select_reporting(&self.store)))
    }
}

/// Whether two offers carry the same value for `field`. Used only by the
/// readonly check, which is why an absent value on both sides counts as
/// unchanged.
fn same_value(a: &Offer, b: &Offer, field: Field) -> bool {
    match field {
        Field::Id => a.id == b.id,
        Field::Specialization => a.specialization == b.specialization,
        Field::Cost => a.cost.total_cmp(&b.cost).is_eq(),
        Field::LatencyP50 => match (a.latency_p50, b.latency_p50) {
            (Some(x), Some(y)) => x.total_cmp(&y).is_eq(),
            (None, None) => true,
            _ => false,
        },
        Field::LatencyP99 => a.latency_p99.total_cmp(&b.latency_p99).is_eq(),
        Field::Load => a.load.total_cmp(&b.load).is_eq(),
        Field::Residency => a.residency == b.residency,
        Field::MemFootprint => a.mem_footprint == b.mem_footprint,
        Field::PlacementNode => a.placement_node == b.placement_node,
        Field::RouteFreq => a.route_freq == b.route_freq,
    }
}

/// Renders a property's value for a refusal. `None` reads as `absent` rather
/// than as an empty string, because an empty `specialization` is a value.
fn describe_value(offer: &Offer, field: Field) -> String {
    match field {
        Field::Id => format!("{:?}", offer.id),
        Field::Specialization => match &offer.specialization {
            Some(s) => format!("{s:?}"),
            None => "absent".to_owned(),
        },
        Field::Cost => offer.cost.to_string(),
        Field::LatencyP50 => match offer.latency_p50 {
            Some(x) => x.to_string(),
            None => "absent".to_owned(),
        },
        Field::LatencyP99 => offer.latency_p99.to_string(),
        Field::Load => offer.load.to_string(),
        Field::Residency => format!("{:?}", offer.residency),
        Field::MemFootprint => offer.mem_footprint.to_string(),
        Field::PlacementNode => format!("{:?}", offer.placement_node),
        Field::RouteFreq => offer.route_freq.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Residency;

    fn offer(id: &str) -> Offer {
        Offer {
            id: id.to_owned(),
            specialization: Some("math".to_owned()),
            cost: 1.0,
            latency_p50: Some(10.0),
            latency_p99: 20.0,
            load: 0.5,
            residency: Residency::Resident,
            mem_footprint: 1024,
            placement_node: "node-a".to_owned(),
            route_freq: 0,
        }
    }

    fn expert_type() -> ServiceType {
        ServiceType::declare(
            "moe::Expert",
            "IDL:moe/Expert:1.0",
            vec![
                PropertySchema::new("specialization", PropertyKind::Text, PropertyMode::Mandatory),
                PropertySchema::new("cost", PropertyKind::Float, PropertyMode::Normal),
                PropertySchema::new(
                    "placement_node",
                    PropertyKind::Text,
                    PropertyMode::MandatoryReadonly,
                ),
            ],
        )
        .expect("the fixture type is legal")
    }

    /// `ALL_PROPERTIES` is the one hand-written list in this module, and this
    /// is what stops it drifting from the parser's. Both halves matter: every
    /// name in it must resolve, and every name the parser knows must be in it
    /// — a list that is merely a *subset* would silently stop projecting a
    /// property the day one was added.
    #[test]
    fn the_projected_property_list_is_exactly_the_list_the_parser_accepts() {
        for name in ALL_PROPERTIES {
            assert_eq!(property_name(name), Some(name), "{name} does not resolve");
        }
        let from_parser: Vec<&str> = crate::query::FIELD_LIST.split(',').map(str::trim).collect();
        assert_eq!(
            from_parser.len(),
            ALL_PROPERTIES.len(),
            "the parser knows {from_parser:?}, projection knows {ALL_PROPERTIES:?}"
        );
        for name in &from_parser {
            assert!(ALL_PROPERTIES.contains(name), "{name} is parseable but never projected");
        }
    }

    #[test]
    fn a_legal_type_declares_and_keeps_its_schema_in_order() {
        let t = expert_type();
        assert_eq!(t.name(), "moe::Expert");
        assert_eq!(t.interface_id(), "IDL:moe/Expert:1.0");
        let names: Vec<&str> = t.properties().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["specialization", "cost", "placement_node"]);
    }

    #[test]
    fn a_name_that_is_not_a_scoped_identifier_is_refused_as_illegal_not_unknown() {
        for bad in ["", "::moe::Expert", "moe::", "1expert", "moe::Ex-pert", "moe:::Expert"] {
            let e = ServiceType::declare(bad, "IDL:x:1.0", vec![]).unwrap_err();
            assert_eq!(e.kind, RefusalKind::IllegalServiceType, "{bad:?} -> {}", e.message);
            assert!(e.message.contains("moe::Expert"), "the shape is shown: {}", e.message);
        }
        assert!(ServiceType::declare("Expert", "IDL:x:1.0", vec![]).is_ok());
        assert!(ServiceType::declare("a::b::C_9", "IDL:x:1.0", vec![]).is_ok());
    }

    #[test]
    fn a_property_the_engine_does_not_have_is_refused_and_the_ten_are_listed() {
        let e = ServiceType::declare(
            "moe::Expert",
            "IDL:x:1.0",
            vec![PropertySchema::new("throughput", PropertyKind::Float, PropertyMode::Normal)],
        )
        .unwrap_err();
        assert_eq!(e.kind, RefusalKind::IllegalPropertyName);
        assert!(e.message.contains("throughput"), "{}", e.message);
        assert!(e.message.contains("latency_p99"), "the closed set is shown: {}", e.message);
    }

    #[test]
    fn a_schema_that_disagrees_with_the_engine_about_a_kind_is_refused_where_it_is_written() {
        let e = ServiceType::declare(
            "moe::Expert",
            "IDL:x:1.0",
            vec![PropertySchema::new("cost", PropertyKind::Text, PropertyMode::Normal)],
        )
        .unwrap_err();
        assert_eq!(e.kind, RefusalKind::PropertyTypeMismatch);
        assert!(e.message.contains("as text"), "{}", e.message);
        assert!(e.message.contains("as float"), "{}", e.message);
    }

    #[test]
    fn the_same_property_twice_is_refused() {
        let e = ServiceType::declare(
            "moe::Expert",
            "IDL:x:1.0",
            vec![
                PropertySchema::new("cost", PropertyKind::Float, PropertyMode::Normal),
                PropertySchema::new("cost", PropertyKind::Float, PropertyMode::Readonly),
            ],
        )
        .unwrap_err();
        assert_eq!(e.kind, RefusalKind::DuplicatePropertyName);
    }

    #[test]
    fn an_empty_interface_id_is_refused_because_the_repository_id_is_the_key() {
        let e = ServiceType::declare("moe::Expert", "", vec![]).unwrap_err();
        assert_eq!(e.kind, RefusalKind::IllegalServiceType);
        assert!(e.message.contains("IDL:moe/Expert:1.0"), "{}", e.message);
    }

    #[test]
    fn super_types_are_refused_by_name_rather_than_silently_dropped() {
        let e = ServiceType::with_super_types(
            "moe::FastExpert",
            "IDL:moe/FastExpert:1.0",
            &["moe::Expert".to_owned()],
            vec![],
        )
        .unwrap_err();
        assert_eq!(e.kind, RefusalKind::UnsupportedSuperTypes);
        assert!(e.message.contains("moe::Expert"), "the super type is named: {}", e.message);
        assert!(e.message.contains("subtype matching"), "{}", e.message);
    }

    #[test]
    fn an_unknown_type_and_an_illegal_type_are_different_refusals() {
        let mut s = TypedOfferStore::new();
        assert_eq!(
            s.service_type("moe::Expert").unwrap_err().kind,
            RefusalKind::UnknownServiceType
        );
        assert!(
            s.service_type("moe::Expert")
                .unwrap_err()
                .message
                .contains("no service type has been declared")
        );
        assert_eq!(s.service_type("1bad").unwrap_err().kind, RefusalKind::IllegalServiceType);
        s.declare(expert_type()).unwrap();
        let said = s.service_type("moe::Other").unwrap_err().message;
        assert!(said.contains("the declared types are moe::Expert"), "{said}");
    }

    #[test]
    fn declaring_the_same_type_twice_is_refused() {
        let mut s = TypedOfferStore::new();
        s.declare(expert_type()).unwrap();
        assert_eq!(s.declare(expert_type()).unwrap_err().kind, RefusalKind::DuplicateServiceType);
    }

    #[test]
    fn a_mandatory_property_the_offer_does_not_carry_refuses_the_registration() {
        let mut s = TypedOfferStore::new();
        s.declare(expert_type()).unwrap();
        let mut o = offer("e1");
        o.specialization = None;
        let e = s.register("moe::Expert", o).unwrap_err();
        assert_eq!(e.kind, RefusalKind::MissingMandatoryProperty);
        assert!(e.message.contains("specialization"), "{}", e.message);
        // and nothing was left behind
        assert!(s.store().is_empty(), "a refused registration stores nothing");
        assert_eq!(s.type_of("e1"), None);
    }

    #[test]
    fn a_registration_against_an_undeclared_type_is_refused_before_the_store_is_touched() {
        let mut s = TypedOfferStore::new();
        let e = s.register("moe::Expert", offer("e1")).unwrap_err();
        assert_eq!(e.kind, RefusalKind::UnknownServiceType);
        assert!(s.store().is_empty());
    }

    #[test]
    fn a_heartbeat_that_moves_a_readonly_property_is_refused_with_both_values() {
        let mut s = TypedOfferStore::new();
        s.declare(expert_type()).unwrap();
        s.register("moe::Expert", offer("e1")).unwrap();

        let mut moved = offer("e1");
        moved.placement_node = "node-b".to_owned();
        let e = s.heartbeat(moved).unwrap_err();
        assert_eq!(e.kind, RefusalKind::ReadonlyPropertyModified);
        assert!(e.message.contains("node-a"), "{}", e.message);
        assert!(e.message.contains("node-b"), "{}", e.message);
        assert_eq!(s.store().get("e1").unwrap().placement_node, "node-a");

        // A normal property moves freely.
        let mut cheaper = offer("e1");
        cheaper.cost = 0.25;
        s.heartbeat(cheaper).unwrap();
        assert_eq!(s.store().get("e1").unwrap().cost, 0.25);
    }

    #[test]
    fn a_heartbeat_that_drops_a_mandatory_property_is_refused() {
        let mut s = TypedOfferStore::new();
        s.declare(expert_type()).unwrap();
        s.register("moe::Expert", offer("e1")).unwrap();
        let mut dropped = offer("e1");
        dropped.specialization = None;
        let e = s.heartbeat(dropped).unwrap_err();
        assert_eq!(e.kind, RefusalKind::MissingMandatoryProperty);
    }

    #[test]
    fn a_duplicate_offer_id_is_the_stores_own_refusal_carried_through() {
        let mut s = TypedOfferStore::new();
        s.declare(expert_type()).unwrap();
        s.register("moe::Expert", offer("e1")).unwrap();
        let e = s.register("moe::Expert", offer("e1")).unwrap_err();
        assert_eq!(e.kind, RefusalKind::Store);
        assert!(e.message.contains("already registered"), "{}", e.message);
    }

    #[test]
    fn selection_is_narrowed_to_one_type_and_the_other_types_offers_are_not_unanswerable() {
        let mut s = TypedOfferStore::new();
        s.declare(expert_type()).unwrap();
        s.declare(ServiceType::declare("moe::Router", "IDL:moe/Router:1.0", vec![]).unwrap())
            .unwrap();

        s.register("moe::Expert", offer("e1")).unwrap();
        let mut gapped = offer("r1");
        gapped.specialization = None;
        s.register("moe::Router", gapped).unwrap();

        let q = Query::parse("specialization == 'math'").unwrap();
        let sel = s.select("moe::Expert", &q).unwrap();
        assert_eq!(sel.matched.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["e1"]);
        assert!(
            sel.unanswerable.is_empty(),
            "r1 is another type, not an offer this query could not answer about: {:?}",
            sel.unanswerable
        );

        let sel = s.select("moe::Router", &q).unwrap();
        assert!(sel.matched.is_empty());
        assert_eq!(sel.unanswerable.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["r1"]);
    }

    #[test]
    fn deregistering_forgets_the_type_too() {
        let mut s = TypedOfferStore::new();
        s.declare(expert_type()).unwrap();
        s.register("moe::Expert", offer("e1")).unwrap();
        assert_eq!(s.type_of("e1"), Some("moe::Expert"));
        assert!(s.deregister("e1"));
        assert_eq!(s.type_of("e1"), None);
        // and the id is free again
        s.register("moe::Expert", offer("e1")).unwrap();
    }
}
