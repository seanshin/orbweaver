//! The seed loader — `corpus/state/` read as data, once, for every fixture.
//!
//! D026 §5 S1. The corpus is rich in *contracts* and, until this module, held
//! no *runtime state* at all: every offer a trader ranked, every name a naming
//! graph bound, every tenant and node was invented at the fixture that needed
//! it, inline, in Rust. Five fixtures did that with no shared source, and
//! whether "the same" `PolicyDomain` in two of them meant the same thing was a
//! claim nobody had checked. `corpus/state/README.md` records what checking it
//! found.
//!
//! # This is a reader, not the reader
//!
//! Every file under `corpus/state/` has **two** readers that share no code:
//! this one, and a Python one in `spikes/` that populates omniORB using
//! omniORB's own stubs. That is the whole point of the format (D026 §5 S1b) and
//! not an implementation detail — a peer check that hands the same literal to
//! both ends from inside one script has proved that the script is consistent
//! with itself, and part of every agreement it reports is an artifact of one
//! author typing a value twice.
//!
//! So: **do not add a convenience here that the Python half cannot have.** The
//! format is plain JSON with AnyJSON v1's scalar spellings, for the four
//! reasons `corpus/state/README.md` gives, and the parser is
//! [`orbweaver_dynamic::json`] because the workspace has no serde and gains
//! none by this.
//!
//! # A seeded population is not the only population
//!
//! D026 §3. Nothing here retires a fixture's ad-hoc case. `prop.rs` and
//! `wire-fuzz` exist because a fixed population is a fixed set of paths, and a
//! fixture that can *only* run against the blessed seed has swapped one blind
//! spot for another.
//!
//! # Absent is not zero
//!
//! Every accessor here **refuses** a missing or mistyped member rather than
//! defaulting one. `latency_p50: null` means nobody measured it, and the
//! reason `orbweaver-trading` carries `Option<f64>` there at all is that as a
//! placeholder `0.0` it did not merely fail to match — it matched *every*
//! upper bound, so a router selecting on `latency_p50 < 20` preferred exactly
//! the experts nobody had timed. A loader that silently defaults reintroduces
//! that defect one layer further out, where no type can catch it.

use std::path::{Path, PathBuf};

use orbweaver_dynamic::json::Json;
use orbweaver_trading::{Offer, Residency};

/// Why a seed could not be loaded.
///
/// One type with the file and the JSON path in it, because the thing a reader
/// needs first is *which member of which file*, and a bare "expected a string"
/// from four levels down is a worse diagnostic than no diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedError {
    /// The seed file, as named to [`load`].
    pub file: String,
    /// The dotted path to the member at fault, e.g. `offers[3].cost`.
    pub at: String,
    /// What was wrong.
    pub message: String,
}

impl std::fmt::Display for SeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "corpus/state/{}: at {}: {}", self.file, self.at, self.message)
    }
}

impl std::error::Error for SeedError {}

type Result<T> = std::result::Result<T, SeedError>;

/// The directory the seeds live in.
///
/// Resolved from this crate's manifest rather than from the process's working
/// directory: a fixture run from `spikes/` and a test run by `cargo test` have
/// different `cwd`s, and a seed that loads under one and not the other is a
/// fixture that fails for a reason having nothing to do with what it measures.
pub fn state_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/state")
}

/// Reads and parses one seed file by name, e.g. `moe-experts.json`.
pub fn load(file: &str) -> Result<Json> {
    let path = state_dir().join(file);
    let text = std::fs::read_to_string(&path).map_err(|e| SeedError {
        file: file.to_owned(),
        at: "<file>".to_owned(),
        message: format!("cannot read {}: {e}", path.display()),
    })?;
    Json::parse(&text).map_err(|e| SeedError {
        file: file.to_owned(),
        at: "<document>".to_owned(),
        message: e.to_string(),
    })
}

// ---- typed access, refusing rather than defaulting ----

/// A cursor into one seed document, carrying enough context to say where a
/// fault is.
#[derive(Debug, Clone, Copy)]
pub struct At<'a> {
    file: &'a str,
    path: &'a str,
    json: &'a Json,
}

impl<'a> At<'a> {
    /// A cursor at the root of `json`, which came from `file`.
    pub fn new(file: &'a str, json: &'a Json) -> At<'a> {
        At { file, path: "", json }
    }

    fn err(&self, message: impl Into<String>) -> SeedError {
        SeedError {
            file: self.file.to_owned(),
            at: if self.path.is_empty() { "<root>".to_owned() } else { self.path.to_owned() },
            message: message.into(),
        }
    }

    fn child(&self, path: String, json: &'a Json) -> AtOwned<'a> {
        AtOwned { file: self.file, path, json }
    }

    /// The named member, refusing if it is absent.
    pub fn field(&self, name: &str) -> Result<AtOwned<'a>> {
        let j = self
            .json
            .get(name)
            .ok_or_else(|| self.err(format!("no member `{name}` ({} here)", self.json.kind())))?;
        Ok(self.child(join(self.path, name), j))
    }

    /// The array's elements, refusing if this is not an array.
    pub fn items(&self) -> Result<Vec<AtOwned<'a>>> {
        match self.json {
            Json::Array(v) => Ok(v
                .iter()
                .enumerate()
                .map(|(i, j)| self.child(format!("{}[{i}]", self.path), j))
                .collect()),
            other => Err(self.err(format!("expected an array, found {}", other.kind()))),
        }
    }

    /// The string, refusing anything else — `null` included.
    pub fn string(&self) -> Result<String> {
        match self.json {
            Json::String(s) => Ok(s.clone()),
            other => Err(self.err(format!("expected a string, found {}", other.kind()))),
        }
    }

    /// The string, or `None` for an explicit `null`.
    ///
    /// `null` is the *stated* absence of a measurement, which is why this is a
    /// separate method from [`At::string`] rather than a fallback inside it: a
    /// caller has to choose which of the two it means, and a missing member is
    /// an error under both.
    pub fn string_or_null(&self) -> Result<Option<String>> {
        match self.json {
            Json::Null => Ok(None),
            Json::String(s) => Ok(Some(s.clone())),
            other => Err(self.err(format!("expected a string or null, found {}", other.kind()))),
        }
    }

    /// The number as `f64`, refusing anything else.
    pub fn f64(&self) -> Result<f64> {
        match self.json {
            Json::Number(n) => {
                n.parse::<f64>().map_err(|e| self.err(format!("`{n}` is not a number: {e}")))
            }
            other => Err(self.err(format!("expected a number, found {}", other.kind()))),
        }
    }

    /// The number as `f64`, or `None` for an explicit `null`.
    pub fn f64_or_null(&self) -> Result<Option<f64>> {
        match self.json {
            Json::Null => Ok(None),
            _ => self.f64().map(Some),
        }
    }

    /// A 32-bit count, refusing a non-integer or an out-of-range one.
    pub fn u32(&self) -> Result<u32> {
        match self.json {
            Json::Number(n) => {
                n.parse::<u32>().map_err(|e| self.err(format!("`{n}` is not a u32: {e}")))
            }
            other => Err(self.err(format!("expected a number, found {}", other.kind()))),
        }
    }

    /// A 64-bit integer, which AnyJSON v1 spells as a **string**.
    ///
    /// A JSON number is a `double` in every mainstream implementation, so
    /// anything past 2^53 loses digits silently. Accepting a bare number here
    /// would make the seed's spelling optional, and the day a footprint grew
    /// past 2^53 the two readers would disagree — quietly, and only for large
    /// values, which is the worst shape a disagreement can have.
    pub fn u64_string(&self) -> Result<u64> {
        match self.json {
            Json::String(s) => s
                .parse::<u64>()
                .map_err(|e| self.err(format!("`{s}` is not a 64-bit integer: {e}"))),
            Json::Number(n) => Err(self.err(format!(
                "a 64-bit integer crosses as a string in AnyJSON v1; write \"{n}\", not {n}"
            ))),
            other => Err(self.err(format!("expected a quoted integer, found {}", other.kind()))),
        }
    }

    /// A boolean, refusing anything else.
    pub fn bool(&self) -> Result<bool> {
        match self.json {
            Json::Bool(b) => Ok(*b),
            other => Err(self.err(format!("expected a boolean, found {}", other.kind()))),
        }
    }

    /// An array of strings.
    pub fn strings(&self) -> Result<Vec<String>> {
        self.items()?.iter().map(|it| it.as_ref().string()).collect()
    }
}

/// An [`At`] that owns its path string.
#[derive(Debug, Clone)]
pub struct AtOwned<'a> {
    file: &'a str,
    path: String,
    json: &'a Json,
}

impl<'a> AtOwned<'a> {
    /// Borrows this cursor as an [`At`].
    pub fn as_ref(&self) -> At<'_> {
        At { file: self.file, path: &self.path, json: self.json }
    }

    /// The named member, refusing if it is absent.
    pub fn field(&self, name: &str) -> Result<AtOwned<'a>> {
        let j = self.json.get(name).ok_or_else(|| SeedError {
            file: self.file.to_owned(),
            at: self.path.clone(),
            message: format!("no member `{name}` ({} here)", self.json.kind()),
        })?;
        Ok(AtOwned { file: self.file, path: join(&self.path, name), json: j })
    }

    /// The array's elements.
    pub fn items(&self) -> Result<Vec<AtOwned<'a>>> {
        match self.json {
            Json::Array(v) => Ok(v
                .iter()
                .enumerate()
                .map(|(i, j)| AtOwned {
                    file: self.file,
                    path: format!("{}[{i}]", self.path),
                    json: j,
                })
                .collect()),
            other => Err(SeedError {
                file: self.file.to_owned(),
                at: self.path.clone(),
                message: format!("expected an array, found {}", other.kind()),
            }),
        }
    }
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() { name.to_owned() } else { format!("{prefix}.{name}") }
}

// ---- the MoE expert population ----

/// One query the population makes answerable, with the answer it makes
/// correct.
#[derive(Debug, Clone, PartialEq)]
pub struct SeededQuery {
    /// What the entry is for, in words.
    pub name: String,
    /// The constraint, in the trader's constraint language.
    pub constraint: String,
    /// The preference expression, empty for none.
    pub preference: String,
    /// Whether `expect_ids` is an order or a set.
    pub ordered: bool,
    /// The property `ordered` sorts by, when there is one.
    pub order_by: Option<String>,
    /// The ids the wire must answer with.
    pub expect_ids: Vec<String>,
    /// Ids the preference could not place, which must appear **after** every
    /// ranked one rather than being dropped.
    ///
    /// A separate member and not an inference from a `null` property, because
    /// the two layers say different true things: the engine sets an unrankable
    /// offer aside, and the wire — having one `OfferSeq` and no way to spell
    /// "unranked" — appends it last. Stating it makes a reader that knows only
    /// the engine's half wrong out loud instead of quietly.
    pub expect_unranked_last: Vec<String>,
}

/// The seeded MoE expert population.
#[derive(Debug, Clone)]
pub struct MoeExperts {
    /// The service type the offers are registered under.
    pub service_type_name: String,
    /// Its interface repository id.
    pub interface_id: String,
    /// The properties the service type declares, as `(name, kind, mode)`.
    pub properties: Vec<(String, String, String)>,
    /// The offers, in the order the file states them.
    pub offers: Vec<Offer>,
    /// The queries, with their expected answers.
    pub queries: Vec<SeededQuery>,
}

impl MoeExperts {
    /// Loads `corpus/state/moe-experts.json`.
    pub fn load() -> Result<MoeExperts> {
        const FILE: &str = "moe-experts.json";
        let doc = load(FILE)?;
        let root = At::new(FILE, &doc);

        let st = root.field("service_type")?;
        let service_type_name = st.field("name")?.as_ref().string()?;
        let interface_id = st.field("interface_id")?.as_ref().string()?;
        let mut properties = Vec::new();
        for p in st.field("properties")?.items()? {
            properties.push((
                p.field("name")?.as_ref().string()?,
                p.field("kind")?.as_ref().string()?,
                p.field("mode")?.as_ref().string()?,
            ));
        }

        let mut offers = Vec::new();
        for o in root.field("offers")?.items()? {
            offers.push(Offer {
                id: o.field("id")?.as_ref().string()?,
                specialization: o.field("specialization")?.as_ref().string_or_null()?,
                cost: o.field("cost")?.as_ref().f64()?,
                latency_p50: o.field("latency_p50")?.as_ref().f64_or_null()?,
                latency_p99: o.field("latency_p99")?.as_ref().f64()?,
                load: o.field("load")?.as_ref().f64()?,
                residency: residency(&o.field("residency")?)?,
                mem_footprint: o.field("mem_footprint")?.as_ref().u64_string()?,
                placement_node: o.field("placement_node")?.as_ref().string()?,
                route_freq: o.field("route_freq")?.as_ref().u64_string()?,
            });
        }

        let mut queries = Vec::new();
        for q in root.field("queries")?.items()? {
            let ordered = q.field("ordered")?.as_ref().bool()?;
            queries.push(SeededQuery {
                name: q.field("name")?.as_ref().string()?,
                constraint: q.field("constraint")?.as_ref().string()?,
                preference: q.field("preference")?.as_ref().string()?,
                ordered,
                order_by: match q.field("order_by") {
                    Ok(f) => Some(f.as_ref().string()?),
                    Err(_) => None,
                },
                expect_ids: q.field("expect_ids")?.as_ref().strings()?,
                expect_unranked_last: match q.field("expect_unranked_last") {
                    Ok(f) => f.as_ref().strings()?,
                    Err(_) => Vec::new(),
                },
            });
        }

        Ok(MoeExperts { service_type_name, interface_id, properties, offers, queries })
    }

    /// The offer with this id, if the population states one.
    pub fn offer(&self, id: &str) -> Option<&Offer> {
        self.offers.iter().find(|o| o.id == id)
    }
}

fn residency(at: &AtOwned<'_>) -> Result<Residency> {
    // By name, never by ordinal: AnyJSON v1 crosses enumerators by name
    // because the ordinal is a wire detail, and §5.3 measured what happens
    // when meaning is attached to it.
    let s = at.as_ref().string()?;
    match s.as_str() {
        "RESIDENT" => Ok(Residency::Resident),
        "OFFLOADED" => Ok(Residency::Offloaded),
        "PREFETCHING" => Ok(Residency::Prefetching),
        "ACTIVE" => Ok(Residency::Active),
        other => Err(SeedError {
            file: at.file.to_owned(),
            at: at.path.clone(),
            message: format!(
                "`{other}` is not a moe::Residency enumerator \
                 (RESIDENT, OFFLOADED, PREFETCHING, ACTIVE)"
            ),
        }),
    }
}

// ---- the estate the population is placed in ----

/// A node the operator declared, and the region it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeededNode {
    /// The node's name.
    pub name: String,
    /// The residency region it sits in.
    pub region: String,
}

/// **Domain A** — the nodes an operator declared to the tenant control plane.
///
/// Closed, and default-deny: `PolicyDomain::check_residency` refuses a name
/// this does not list, so membership has to be decidable or the refusal is not
/// one. Its authority is the operator, out of band, because
/// `corpus/golden/23`'s contract declares no member for a node's region and
/// guessing one from the name would make `check_residency` answer confidently
/// about nodes nobody described.
#[derive(Debug, Clone)]
pub struct DeclaredEstate {
    /// Every node the operator declared, with its region.
    pub nodes: Vec<SeededNode>,
    /// A node name deliberately **not** declared, for showing a residency
    /// check refuse default-deny.
    pub undeclared_probe: String,
}

impl DeclaredEstate {
    /// Whether the operator declared this node.
    pub fn declares(&self, node: &str) -> bool {
        self.nodes.iter().any(|n| n.name == node)
    }
}

/// **Domain B** — the node names experts report about themselves.
///
/// Open, and unvalidated: `moe::Capability.placement_node` is a string member
/// of the published contract that the expert fills in, stored verbatim and
/// queried by the trader as an opaque value. Nothing may close it — a set
/// whose membership is decided by the thing being admitted is not a closed
/// set, and turning `heartbeat` into admission control would be a change to
/// what the contract *does*, not to what a seed *says*.
///
/// The list is therefore the vocabulary **this seed** uses, so a typo in an
/// offer is catchable. It is not an admission list and adding a name to it
/// grants nothing.
#[derive(Debug, Clone)]
pub struct ReportedPlacement {
    /// The node names this seed's offers report, each with the deployment it
    /// belongs to.
    pub nodes: Vec<(String, String)>,
}

impl ReportedPlacement {
    /// Whether this seed states an offer may report this node name.
    pub fn states(&self, node: &str) -> bool {
        self.nodes.iter().any(|(n, _)| n == node)
    }

    /// The node name this deployment's experts report.
    ///
    /// By deployment, never by position: *"the second one in the list"* is not
    /// a fact this file states, and a fixture that took it that way would
    /// silently start reporting somebody else's node the day an entry was
    /// added above it.
    pub fn node_for(&self, deployment: &str) -> Option<&str> {
        self.nodes.iter().find(|(_, d)| d == deployment).map(|(n, _)| n.as_str())
    }
}

/// One capability a tenant holds, as its manifest describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct SeededCapability {
    /// The capability word, from the estate's vocabulary.
    pub name: String,
    /// What it costs.
    pub cost: f64,
    /// The adapter delta that implements it over the shared base model.
    pub adapter_delta: String,
}

/// One grant inside a policy domain: a subject may use a capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeededGrant {
    /// Who is granted.
    pub subject: String,
    /// What they are granted.
    pub capability: String,
}

/// One policy domain and everything it grants.
#[derive(Debug, Clone, PartialEq)]
pub struct SeededPolicyDomain {
    /// The domain's name, as a manifest's `policy_domain` spells it.
    pub name: String,
    /// What it grants. Empty is the default-deny domain, and is a statement.
    pub grants: Vec<SeededGrant>,
}

/// One tenant of the MoE estate.
#[derive(Debug, Clone, PartialEq)]
pub struct SeededTenant {
    /// The tenant id, as a manifest's `tenant_id` spells it.
    pub id: String,
    /// The region this tenant's models must stay in.
    pub residency_region: String,
    /// The capabilities this tenant's experts provide.
    pub capabilities: Vec<SeededCapability>,
    /// This tenant's policy domains.
    pub policy_domains: Vec<SeededPolicyDomain>,
}

impl SeededTenant {
    /// The named capability, if this tenant holds it.
    pub fn capability(&self, name: &str) -> Option<&SeededCapability> {
        self.capabilities.iter().find(|c| c.name == name)
    }

    /// The named policy domain, if this tenant has one.
    pub fn policy_domain(&self, name: &str) -> Option<&SeededPolicyDomain> {
        self.policy_domains.iter().find(|d| d.name == name)
    }

    /// The tenant's domain that grants **nothing** — what a default-deny
    /// `authorize` is refused under.
    ///
    /// By role, not by name and not by position. A caller that reached for
    /// `policy_domains[0]` would be asserting the file's member order, which
    /// is not a fact the file states — `Json::Object` is a `BTreeMap` here and
    /// a `dict` in the Python reader, and neither may depend on order. A
    /// caller that reached for the literal `"acme-default"` would be retyping
    /// a name the seed owns.
    pub fn default_domain(&self) -> Option<&SeededPolicyDomain> {
        self.policy_domains.iter().find(|d| d.grants.is_empty())
    }

    /// The tenant's domain that grants **something** — what makes a positive
    /// `authorize` answer positive. See [`SeededTenant::default_domain`] for
    /// why this is by role.
    pub fn granting_domain(&self) -> Option<&SeededPolicyDomain> {
        self.policy_domains.iter().find(|d| !d.grants.is_empty())
    }
}

/// The seeded MoE estate: **two** node domains, tenants, policy domains.
#[derive(Debug, Clone)]
pub struct MoeEstate {
    /// Domain A: what the operator declared.
    pub declared_estate: DeclaredEstate,
    /// Domain B: what experts report about themselves.
    pub reported_placement: ReportedPlacement,
    /// The three capability words the MoE fixtures use.
    pub capability_vocabulary: Vec<String>,
    /// The capability that stays in the vocabulary and out of every grant, so
    /// a refusal shown against it is refusing something.
    pub ungranted_capability: String,
    /// The shared base model every tenant's experts adapt.
    pub base_model: String,
    /// The tenants, in the order the file states them.
    pub tenants: Vec<SeededTenant>,
}

impl MoeEstate {
    /// Loads `corpus/state/moe-estate.json`.
    pub fn load() -> Result<MoeEstate> {
        const FILE: &str = "moe-estate.json";
        let doc = load(FILE)?;
        let root = At::new(FILE, &doc);

        let domains = root.field("node_domains")?;

        let declared = domains.field("declared_estate")?;
        let mut nodes = Vec::new();
        for n in declared.field("nodes")?.items()? {
            nodes.push(SeededNode {
                name: n.field("name")?.as_ref().string()?,
                region: n.field("region")?.as_ref().string()?,
            });
        }
        let declared_estate = DeclaredEstate {
            nodes,
            undeclared_probe: declared.field("undeclared_probe")?.as_ref().string()?,
        };

        let reported = domains.field("reported_placement")?;
        let mut reported_nodes = Vec::new();
        for n in reported.field("nodes")?.items()? {
            reported_nodes.push((
                n.field("name")?.as_ref().string()?,
                n.field("deployment")?.as_ref().string()?,
            ));
        }
        let reported_placement = ReportedPlacement { nodes: reported_nodes };

        let mut tenants = Vec::new();
        for t in root.field("tenants")?.items()? {
            let mut capabilities = Vec::new();
            for c in t.field("capabilities")?.items()? {
                capabilities.push(SeededCapability {
                    name: c.field("name")?.as_ref().string()?,
                    cost: c.field("cost")?.as_ref().f64()?,
                    adapter_delta: c.field("adapter_delta")?.as_ref().string()?,
                });
            }
            let mut policy_domains = Vec::new();
            for d in t.field("policy_domains")?.items()? {
                let mut grants = Vec::new();
                for g in d.field("grants")?.items()? {
                    grants.push(SeededGrant {
                        subject: g.field("subject")?.as_ref().string()?,
                        capability: g.field("capability")?.as_ref().string()?,
                    });
                }
                policy_domains
                    .push(SeededPolicyDomain { name: d.field("name")?.as_ref().string()?, grants });
            }
            tenants.push(SeededTenant {
                id: t.field("id")?.as_ref().string()?,
                residency_region: t.field("residency_region")?.as_ref().string()?,
                capabilities,
                policy_domains,
            });
        }

        Ok(MoeEstate {
            declared_estate,
            reported_placement,
            capability_vocabulary: root.field("capability_vocabulary")?.as_ref().strings()?,
            ungranted_capability: root.field("ungranted_capability")?.as_ref().string()?,
            base_model: root.field("base_model")?.as_ref().string()?,
            tenants,
        })
    }

    /// The named tenant, if the estate states one.
    pub fn tenant(&self, id: &str) -> Option<&SeededTenant> {
        self.tenants.iter().find(|t| t.id == id)
    }

    /// Every grant every tenant makes, as `(tenant, domain, subject,
    /// capability)`.
    pub fn grants(&self) -> Vec<(&str, &str, &str, &str)> {
        self.tenants
            .iter()
            .flat_map(|t| {
                t.policy_domains.iter().flat_map(move |d| {
                    d.grants.iter().map(move |g| {
                        (t.id.as_str(), d.name.as_str(), g.subject.as_str(), g.capability.as_str())
                    })
                })
            })
            .collect()
    }
}

// ---- the naming graph ----

/// One `CosNaming::NameComponent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeededNameComponent {
    /// The `id` field.
    pub id: String,
    /// The `kind` field, empty for most components.
    pub kind: String,
}

/// One name bound in the graph, or stated to be absent from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeededBinding {
    /// The compound name, component by component.
    pub path: Vec<SeededNameComponent>,
    /// The object key the bound reference carries, when this is a binding.
    pub object_key: Option<String>,
    /// The `CosNaming` stringified form, e.g. `spike/Echo 2.dev`.
    pub stringified: String,
    /// The URL-escaped form for a `corbaname:` fragment, when stated.
    pub url_fragment: Option<String>,
}

/// The seeded naming graph.
#[derive(Debug, Clone)]
pub struct NamingGraph {
    /// The `type_id` every bound reference carries.
    pub type_id: String,
    /// Contexts that must exist, parents before children.
    pub contexts: Vec<Vec<String>>,
    /// Names that must resolve.
    pub bindings: Vec<SeededBinding>,
    /// Names that must **not** resolve — a stated property of the population,
    /// not the absence of a statement.
    pub absent: Vec<SeededBinding>,
    /// How many bindings `list` over the root must return.
    pub root_binding_count: u32,
    /// Their names.
    pub root_binding_names: Vec<String>,
}

impl NamingGraph {
    /// Loads `corpus/state/naming-graph.json`.
    pub fn load() -> Result<NamingGraph> {
        const FILE: &str = "naming-graph.json";
        let doc = load(FILE)?;
        let root = At::new(FILE, &doc);

        let type_id = root.field("reference_template")?.field("type_id")?.as_ref().string()?;

        let mut contexts = Vec::new();
        for c in root.field("contexts")?.items()? {
            contexts.push(c.field("path")?.as_ref().strings()?);
        }

        let read_names = |field: &str| -> Result<Vec<SeededBinding>> {
            let mut out = Vec::new();
            for b in root.field(field)?.items()? {
                let mut path = Vec::new();
                for c in b.field("path")?.items()? {
                    path.push(SeededNameComponent {
                        id: c.field("id")?.as_ref().string()?,
                        kind: c.field("kind")?.as_ref().string()?,
                    });
                }
                out.push(SeededBinding {
                    path,
                    object_key: match b.field("object_key") {
                        Ok(f) => Some(f.as_ref().string()?),
                        Err(_) => None,
                    },
                    stringified: b.field("stringified")?.as_ref().string()?,
                    url_fragment: match b.field("url_fragment") {
                        Ok(f) => Some(f.as_ref().string()?),
                        Err(_) => None,
                    },
                });
            }
            Ok(out)
        };

        let bindings = read_names("bindings")?;
        let absent = read_names("absent")?;

        let rb = root.field("root_bindings")?;
        Ok(NamingGraph {
            type_id,
            contexts,
            bindings,
            absent,
            root_binding_count: rb.field("count")?.as_ref().u32()?,
            root_binding_names: rb.field("names")?.as_ref().strings()?,
        })
    }
}

/// The `CosNaming` stringified form of a compound name.
///
/// One function, because the Rust gate and the diagnostics both need it and a
/// second copy would drift. `id.kind` when the kind is non-empty, `id` when it
/// is, joined by `/`.
pub fn stringify(path: &[SeededNameComponent]) -> String {
    path.iter()
        .map(|c| if c.kind.is_empty() { c.id.clone() } else { format!("{}.{}", c.id, c.kind) })
        .collect::<Vec<_>>()
        .join("/")
}
