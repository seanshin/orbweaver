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

/// A node the estate declares, and the region it is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeededNode {
    /// The node's name, as a `placement_node` would spell it.
    pub name: String,
    /// The residency region it sits in.
    pub region: String,
}

/// The seeded MoE estate: nodes, tenants, policy domains.
#[derive(Debug, Clone)]
pub struct MoeEstate {
    /// Every node any seeded offer may legally be placed on.
    pub nodes: Vec<SeededNode>,
    /// A node name deliberately **not** declared, for showing a residency
    /// check refuse default-deny.
    pub undeclared_probe: String,
    /// The three capability words the MoE fixtures use.
    pub capability_vocabulary: Vec<String>,
    /// The shared base model every tenant's experts adapt.
    pub base_model: String,
}

impl MoeEstate {
    /// Loads `corpus/state/moe-estate.json`.
    pub fn load() -> Result<MoeEstate> {
        const FILE: &str = "moe-estate.json";
        let doc = load(FILE)?;
        let root = At::new(FILE, &doc);

        let mut nodes = Vec::new();
        for n in root.field("nodes")?.items()? {
            nodes.push(SeededNode {
                name: n.field("name")?.as_ref().string()?,
                region: n.field("region")?.as_ref().string()?,
            });
        }

        Ok(MoeEstate {
            nodes,
            undeclared_probe: root.field("_undeclared_node_probe")?.as_ref().string()?,
            capability_vocabulary: root.field("capability_vocabulary")?.as_ref().strings()?,
            base_model: root.field("base_model")?.as_ref().string()?,
        })
    }

    /// Whether the estate declares this node.
    pub fn declares(&self, node: &str) -> bool {
        self.nodes.iter().any(|n| n.name == node)
    }
}
