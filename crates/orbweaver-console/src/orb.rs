//! The ORB's read-side administration surface: what is registered, what the
//! seven numbers are and **where each came from**, and what the counters say.
//!
//! D024 §4 asks for `orbctl`, "read first, and write only what a file can say",
//! and §6 puts the read half here because `orbweaver-console` *renders and
//! decides nothing* — which is exactly what the read half of administration is.
//! This module keeps that charter: every fact on these pages was stated by
//! something else, and where nothing stated it the page says so.
//!
//! # There is no wire here, and that is a refusal, not an omission
//!
//! `PoolStats`, `ServerStats` and `ChannelStats` live inside a running process.
//! D024 §7 refuses to propose a wire interface for administration, because a
//! remote admin interface needs the caller model `PLAN-DEFERRED` §11 is waiting
//! on, and an unauthenticated one would be the same power through a side door.
//!
//! So there are exactly two honest inputs, and [`Snapshot`] is both:
//!
//! 1. **In process** — [`Snapshot::live`] takes the structs themselves. A
//!    process that already holds them renders its own state with no file and no
//!    socket.
//! 2. **Out of process** — that same snapshot goes to JSON with
//!    [`Snapshot::to_json`] and comes back with [`Snapshot::read`]. An operator
//!    points the tool at the file the holding process wrote.
//!
//! **Nothing in this workspace writes one yet.** No server, fixture or binary
//! here calls [`Snapshot::live`]; the writer belongs to whichever process wants
//! to be administered, and giving it one is not this crate's change to make.
//! An operator therefore cannot point this at a running server today, and the
//! tool's own `--help` says so — a limit learned from a usage line is a limit;
//! a limit learned from an empty page is a surprise.
//!
//! # What this module does not decide
//!
//! **Which ObjectIds CORBA 3.4 §8.5.2 reserves is not decided here.** That
//! table has one home and it is the ORB's, not the viewer's; a retyped copy
//! would be a second table that agrees with the first until the day it does
//! not. The snapshot's writer states reservedness per id, and a row whose
//! writer said nothing renders [`Reserved::NotStated`] rather than *no*.
//!
//! **The drop split is never re-summed.** `ChannelStats` broke one drop counter
//! into five causes on purpose (D011 §6.1): a clean `stop` and an overloaded
//! consumer moved the same number, so no reading of the total could tell
//! back-pressure from housekeeping. A renderer that added the five back up
//! would undo exactly that. Every cause is a row of its own, the total is
//! labelled as the channel's own report rather than as a sum taken here, and
//! the two are reconciled by [`ChannelStats::split_adds_up`] — the channel's
//! function, called, not re-implemented. When it says the numbers do not
//! reconcile the page says **that**, in place of numbers that do not add up.
//!
//! # 관리 표면의 읽기 절반
//!
//! ORB의 상태는 프로세스 안에 있고, D024 §7은 그것을 위한 와이어 인터페이스를
//! 거부한다 — 원격 관리에는 `PLAN-DEFERRED` §11이 기다리는 호출자 모델이 필요하고,
//! 인증 없는 관리는 옆문으로 들어온 같은 권한이기 때문이다. 그래서 입력은 두
//! 가지뿐이다: 프로세스 안에서 구조체를 직접 받거나([`Snapshot::live`]), 그
//! 프로세스가 써 둔 JSON 스냅샷을 읽거나([`Snapshot::read`]). **오늘 이 워크스페이스에서
//! 스냅샷을 쓰는 것은 없다** — 실행 중인 서버를 이 도구로 가리킬 수 없다는 뜻이고,
//! 그 사실은 `--help`가 말한다.
//!
//! §8.5.2의 예약 ObjectId 표는 여기서 다시 적지 않는다. 표의 집은 ORB이고,
//! 스냅샷을 쓰는 쪽이 각 id의 예약 여부를 진술한다. 진술이 없으면 *아니오*가 아니라
//! *진술되지 않음*으로 그린다. 드롭 분할은 절대 다시 합치지 않는다 — 다섯 원인을
//! 쪼갠 이유가 그 합계였다. 합계는 채널이 보고한 값으로 표시하고, 대조는
//! `split_adds_up()`을 호출해서 한다.

use std::collections::BTreeMap;
use std::time::Duration;

use orbweaver_dynamic::json::Json;
use orbweaver_giop::event_server::ChannelStats;
use orbweaver_giop::pool::PoolStats;
use orbweaver_giop::server::ServerStats;
use orbweaver_giop::{
    DEFAULT_FRAGMENT_THRESHOLD, DEFAULT_MAX_MESSAGE_SIZE, FOLLOW_TIMEOUT, MAX_FORWARD_HOPS,
    MAX_FRAGMENTS,
};

use crate::html::{Markup, page, provenance_footer};

// ---------------------------------------------------------------------------
// services
// ---------------------------------------------------------------------------

/// Whether an ObjectId is one of CORBA 3.4 §8.5.2's sixteen reserved names.
///
/// Three states rather than two. The table is the ORB's and this crate does not
/// hold a copy, so a snapshot whose writer said nothing about an id leaves the
/// question open — and an open question rendered as *no* would be the console
/// reporting a measurement it did not take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reserved {
    /// The writer said this id is one of the sixteen.
    Yes,
    /// The writer said it is not.
    No,
    /// The writer said nothing about it.
    NotStated,
}

impl Reserved {
    /// How it reads to an operator.
    pub fn label(self) -> &'static str {
        match self {
            Reserved::Yes => "reserved by CORBA 3.4 §8.5.2",
            Reserved::No => "not a reserved name",
            Reserved::NotStated => "reservedness not stated by the writer",
        }
    }

    fn short(self) -> &'static str {
        match self {
            Reserved::Yes => "reserved",
            Reserved::No => "local",
            Reserved::NotStated => "not stated",
        }
    }

    fn badge(self) -> &'static str {
        match self {
            Reserved::Yes => "badge b-scope",
            Reserved::No => "badge b-dark",
            Reserved::NotStated => "badge b-unknown",
        }
    }
}

/// One entry of the initial-references table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// The ObjectId `resolve_initial_references` takes.
    pub id: String,
    /// The reference registered under it, stringified — `None` when the id is
    /// known to this ORB and nothing is bound to it.
    pub ior: Option<String>,
    /// Whether the writer called this one of §8.5.2's sixteen.
    pub reserved: Reserved,
}

impl Service {
    /// Whether a reference is registered under this id.
    pub fn registered(&self) -> bool {
        self.ior.is_some()
    }
}

/// What a peer does for each of the three states an ObjectId can be in.
///
/// The distinction is behavioural, not cosmetic: measured against omniORB on
/// 2026-08-25, a reserved id with nothing bound to it answers `NO_RESOURCES`
/// and an id the ORB has never heard of answers `BAD_PARAM`. A page that
/// collapsed the two would be telling an operator that a missing registration
/// and a typo are the same problem.
pub const RESOLUTION_NOTE: &str = "\
An id with a reference registered under it is what resolve_initial_references \
returns. A reserved id with nothing bound to it is a name this ORB knows and \
has not been given — omniORB answers NO_RESOURCES for that one. An id in \
neither list is not a name this ORB knows at all, and omniORB answers \
BAD_PARAM for it (both measured 2026-08-25). Three states, three answers; this \
page keeps them apart and invents no fourth.";

// ---------------------------------------------------------------------------
// config
// ---------------------------------------------------------------------------

/// Where a configured value came from.
///
/// **This is the half an operator actually needs** (D024 §4) and the half a
/// `--config` batch usually forgets: a number without its origin cannot be
/// changed with any confidence, because nobody knows which lever moves it.
///
/// All three variants exist today and exactly one of them is reachable from
/// this workspace's own state: every ORB number is a compile-time constant, so
/// **every row honestly says `compiled default`**. That is the correct output
/// and not a placeholder. D019 step 3 is building the configuration the other
/// two describe; when it lands it supplies a second and third answer to
/// [`Snapshot::config`] and nothing here changes shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// The constant the ORB was built with. Today, all seven.
    CompiledDefault,
    /// A configuration file said so, naming the file and the key — the two
    /// facts `--config`'s own refusals name, for the same reason.
    ConfigFile {
        /// The file the value was read from.
        file: String,
        /// The key inside it.
        key: String,
    },
    /// A command-line flag said so, naming the flag.
    Flag {
        /// The flag as it was given.
        flag: String,
    },
}

impl Source {
    /// How it reads to an operator — always naming the lever, never just the
    /// category.
    pub fn label(&self) -> String {
        match self {
            Source::CompiledDefault => "compiled default".to_owned(),
            Source::ConfigFile { file, key } => format!("configuration file {file}, key {key}"),
            Source::Flag { flag } => format!("flag {flag}"),
        }
    }

    fn badge(&self) -> &'static str {
        match self {
            Source::CompiledDefault => "badge b-dark",
            Source::ConfigFile { .. } => "badge b-idl",
            Source::Flag { .. } => "badge b-scope",
        }
    }
}

/// One ORB number, its value, and where the value came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    /// The key an operator would set.
    pub name: &'static str,
    /// The value, with its unit, as text — a byte count and a millisecond
    /// count are not the same kind of number and rendering them as bare
    /// integers would invite reading one as the other.
    pub value: String,
    /// Where this value came from.
    pub source: Source,
    /// What the number does, in one line, so the page is readable without the
    /// rustdoc beside it.
    pub what: &'static str,
}

/// The seven ORB numbers as this build compiled them.
///
/// Every value is read from the constant that owns it in `orbweaver-giop`.
/// Retyping any of them here would put a second copy of a number in a viewer,
/// and a viewer that disagreed with the ORB about `max_message_size` would be
/// worse than no viewer.
pub fn compiled() -> Vec<Setting> {
    fn s(name: &'static str, value: String, what: &'static str) -> Setting {
        Setting { name, value, source: Source::CompiledDefault, what }
    }
    vec![
        s(
            "max_message_size",
            bytes(DEFAULT_MAX_MESSAGE_SIZE),
            "ceiling on an inbound message body, so four attacker-controlled length bytes cannot \
             ask for a 4 GiB allocation",
        ),
        s(
            "max_forward_hops",
            MAX_FORWARD_HOPS.to_string(),
            "LOCATION_FORWARD hops followed before giving up",
        ),
        s(
            "follow_timeout",
            millis(FOLLOW_TIMEOUT),
            "how long a dial the ORB makes inside a call it was already given waits",
        ),
        s(
            "fragment_threshold",
            bytes(DEFAULT_FRAGMENT_THRESHOLD),
            "body size above which an outbound message leaves here already fragmented",
        ),
        s(
            "max_fragments",
            MAX_FRAGMENTS.to_string(),
            "fragments accepted for one logical message before the peer is called hostile",
        ),
        s(
            "max_connections",
            orbweaver_giop::server::DEFAULT_MAX_CONNECTIONS.to_string(),
            "connections one server serves at once before refusing",
        ),
        s(
            "stop_poll",
            millis(orbweaver_giop::server::STOP_POLL),
            "granularity of shutdown — how long a connection thread waits before re-checking the \
             stop flag",
        ),
    ]
}

fn bytes(n: usize) -> String {
    format!("{n} bytes")
}

fn millis(d: Duration) -> String {
    format!("{} ms", d.as_millis())
}

// ---------------------------------------------------------------------------
// stats
// ---------------------------------------------------------------------------

/// The six numbers [`ServerStats`] answers, taken once.
///
/// A plain copy rather than the live handle, because a snapshot is a reading at
/// a moment and a handle read twice during one render could produce a page
/// whose own columns disagree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServerCounters {
    /// Connections admitted and served since the server was bound.
    pub accepted: u64,
    /// Connections refused because the cap was already reached.
    pub refused: u64,
    /// Connections being served right now.
    pub active: u64,
    /// High-water mark of `active`.
    pub peak_active: u64,
    /// Requests inside the servant — including ones waiting for its lock.
    pub at_servant: u64,
    /// High-water mark of `at_servant`: queue depth, not overlap.
    pub peak_at_servant: u64,
}

impl ServerCounters {
    /// Reads a live handle once.
    pub fn of(stats: &ServerStats) -> Self {
        ServerCounters {
            accepted: stats.accepted(),
            refused: stats.refused(),
            active: stats.active(),
            peak_active: stats.peak_active(),
            at_servant: stats.at_servant(),
            peak_at_servant: stats.peak_at_servant(),
        }
    }
}

/// One event channel's counters, under the name the deployment knows it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    /// What the deployment calls this channel.
    pub name: String,
    /// Its counters.
    pub stats: ChannelStats,
}

/// The five drop causes of a channel, each with its own number.
///
/// Never a sum: the pairs come straight off the five fields, and the total is
/// asked of the channel rather than computed from them.
pub fn drop_causes(stats: &ChannelStats) -> [(&'static str, u64, &'static str); 5] {
    [
        ("dropped_overflow", stats.dropped_overflow, "back-pressure — a bounded queue was full"),
        (
            "unrelayable",
            stats.unrelayable,
            "this channel's own limitation — the any could not be relayed verbatim",
        ),
        (
            "dropped_on_disconnect",
            stats.dropped_on_disconnect,
            "housekeeping — a consumer disconnected itself with a backlog",
        ),
        (
            "dropped_on_failure_disconnect",
            stats.dropped_on_failure_disconnect,
            "housekeeping — the channel cut a proxy that kept failing",
        ),
        (
            "dropped_at_stop",
            stats.dropped_at_stop,
            "housekeeping — stop() ended the channel with events queued",
        ),
    ]
}

/// What the reconciliation between the split and the reported total says.
///
/// The verdict is [`ChannelStats::split_adds_up`]'s, called. A page that
/// re-derived it would be a second answer to a question the channel already
/// answers, and the interesting case is exactly the one where they differ.
pub fn reconciliation(stats: &ChannelStats) -> String {
    if stats.split_adds_up() {
        format!(
            "the five causes account for every drop ({} reported, {} by cause)",
            stats.dropped,
            stats.by_cause()
        )
    } else {
        format!(
            "THE SPLIT DOES NOT RECONCILE: the channel reports {} drops and the five causes \
             account for {}. A discard path was added without naming its cause, which is the \
             failure the split exists to end. Do not read either number as the drop count.",
            stats.dropped,
            stats.by_cause()
        )
    }
}

// ---------------------------------------------------------------------------
// the snapshot
// ---------------------------------------------------------------------------

/// One reading of an ORB's administrable state.
///
/// Every section is optional and absence is rendered as absence: a snapshot
/// whose writer had no event service says nothing about channels, and a page
/// that showed *zero channels* instead would be answering a question nobody
/// asked it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// Where this reading came from — a file name, or the process itself.
    pub origin: String,
    /// The initial-references table, when the writer stated one.
    pub services: Option<Vec<Service>>,
    /// Settings the writer says did **not** come from the compiled default,
    /// keyed by [`Setting::name`]. Empty today from every writer here, because
    /// today there is nothing else for a value to come from.
    pub config: BTreeMap<String, (String, Source)>,
    /// The connection pool's counters, when the writer stated them.
    pub pool: Option<PoolStats>,
    /// The server's counters, when the writer stated them.
    pub server: Option<ServerCounters>,
    /// The event channels, when the writer stated them.
    pub channels: Option<Vec<Channel>>,
    /// What could not be read. **Counted, never skipped**: a section the
    /// console could not parse is a failure, not a silently smaller table.
    pub complaints: Vec<String>,
}

impl Snapshot {
    /// A reading taken in the process that holds the state.
    ///
    /// The one caller that needs no file and no socket. Reservedness comes from
    /// the caller because the caller is the ORB — see the module docs for why
    /// this crate holds no copy of §8.5.2's table.
    pub fn live(
        origin: impl Into<String>,
        services: Option<Vec<Service>>,
        pool: Option<PoolStats>,
        server: Option<&ServerStats>,
        channels: Option<Vec<Channel>>,
    ) -> Self {
        Snapshot {
            origin: origin.into(),
            services,
            config: BTreeMap::new(),
            pool,
            server: server.map(ServerCounters::of),
            channels,
            complaints: Vec::new(),
        }
    }

    /// The seven numbers, resolved: the compiled default unless this snapshot
    /// states another origin for that key.
    ///
    /// Rows come out in [`compiled`]'s order whatever the snapshot says, so two
    /// pages can be read side by side.
    pub fn config(&self) -> Vec<Setting> {
        compiled()
            .into_iter()
            .map(|mut setting| {
                if let Some((value, source)) = self.config.get(setting.name) {
                    setting.value = value.clone();
                    setting.source = source.clone();
                }
                setting
            })
            .collect()
    }

    /// Renders this snapshot as the JSON a holding process writes.
    ///
    /// The reader's other half. `to_json` then [`Snapshot::read`] is a round
    /// trip, and the test that drives it is the only proof available that the
    /// two halves agree about the format.
    pub fn to_json(&self) -> String {
        let mut root: BTreeMap<String, Json> = BTreeMap::new();
        root.insert("origin".into(), Json::String(self.origin.clone()));
        if let Some(services) = &self.services {
            root.insert(
                "services".into(),
                Json::Array(
                    services
                        .iter()
                        .map(|s| {
                            let mut o: BTreeMap<String, Json> = BTreeMap::new();
                            o.insert("id".into(), Json::String(s.id.clone()));
                            if let Some(ior) = &s.ior {
                                o.insert("ior".into(), Json::String(ior.clone()));
                            }
                            match s.reserved {
                                Reserved::Yes => {
                                    o.insert("reserved".into(), Json::Bool(true));
                                }
                                Reserved::No => {
                                    o.insert("reserved".into(), Json::Bool(false));
                                }
                                Reserved::NotStated => {}
                            }
                            Json::Object(o)
                        })
                        .collect(),
                ),
            );
        }
        if !self.config.is_empty() {
            root.insert(
                "config".into(),
                Json::Array(
                    self.config
                        .iter()
                        .map(|(name, (value, source))| {
                            let mut o: BTreeMap<String, Json> = BTreeMap::new();
                            o.insert("name".into(), Json::String(name.clone()));
                            o.insert("value".into(), Json::String(value.clone()));
                            let mut src: BTreeMap<String, Json> = BTreeMap::new();
                            match source {
                                Source::CompiledDefault => {
                                    src.insert("kind".into(), Json::String("compiled".into()));
                                }
                                Source::ConfigFile { file, key } => {
                                    src.insert("kind".into(), Json::String("file".into()));
                                    src.insert("file".into(), Json::String(file.clone()));
                                    src.insert("key".into(), Json::String(key.clone()));
                                }
                                Source::Flag { flag } => {
                                    src.insert("kind".into(), Json::String("flag".into()));
                                    src.insert("flag".into(), Json::String(flag.clone()));
                                }
                            }
                            o.insert("source".into(), Json::Object(src));
                            Json::Object(o)
                        })
                        .collect(),
                ),
            );
        }
        if let Some(pool) = &self.pool {
            root.insert("pool".into(), Json::Object(numbers(&pool_fields(pool))));
        }
        if let Some(server) = &self.server {
            root.insert("server".into(), Json::Object(numbers(&server_fields(server))));
        }
        if let Some(channels) = &self.channels {
            root.insert(
                "channels".into(),
                Json::Array(
                    channels
                        .iter()
                        .map(|c| {
                            let mut o = numbers(&channel_fields(&c.stats));
                            o.insert("name".into(), Json::String(c.name.clone()));
                            Json::Object(o)
                        })
                        .collect(),
                ),
            );
        }
        format!("{}\n", Json::Object(root))
    }

    /// Reads a snapshot a holding process wrote.
    ///
    /// `origin` is the file name, for the page to say what it is showing.
    /// A document that will not parse at all is an error; a *section* that will
    /// not read becomes a complaint and the rest of the page is still rendered,
    /// because an operator with four readable sections and one named failure is
    /// better served than one with a refusal.
    pub fn read(origin: &str, document: &str) -> Result<Snapshot, String> {
        let root = Json::parse(document).map_err(|e| format!("{origin}: {e}"))?;
        if !matches!(root, Json::Object(_)) {
            return Err(format!("{origin}: a snapshot is a JSON object, this was {}", root.kind()));
        }
        let mut snap = Snapshot { origin: origin.to_owned(), ..Snapshot::default() };
        if let Some(Json::String(stated)) = root.get("origin") {
            snap.origin = format!("{origin} (written by {stated})");
        }
        snap.services = read_services(&root, &mut snap.complaints);
        read_config(&root, &mut snap.config, &mut snap.complaints);
        snap.pool = read_pool(&root, &mut snap.complaints);
        snap.server = read_server(&root, &mut snap.complaints);
        snap.channels = read_channels(&root, &mut snap.complaints);
        Ok(snap)
    }
}

fn numbers(fields: &[(&'static str, u64)]) -> BTreeMap<String, Json> {
    fields.iter().map(|(k, v)| ((*k).to_owned(), Json::Number(v.to_string()))).collect()
}

fn pool_fields(p: &PoolStats) -> Vec<(&'static str, u64)> {
    vec![
        ("dialed", p.dialed),
        ("reused", p.reused),
        ("idle_evicted", p.idle_evicted),
        ("faulted_evicted", p.faulted_evicted),
        ("pressure_evicted", p.pressure_evicted),
        ("retried", p.retried),
        ("refused", p.refused),
    ]
}

fn server_fields(s: &ServerCounters) -> Vec<(&'static str, u64)> {
    vec![
        ("accepted", s.accepted),
        ("refused", s.refused),
        ("active", s.active),
        ("peak_active", s.peak_active),
        ("at_servant", s.at_servant),
        ("peak_at_servant", s.peak_at_servant),
    ]
}

/// Every counter of [`ChannelStats`], in the order a reader wants them.
///
/// **Destructured with no `..`, and that is the point.** This list is what the
/// page renders; a counter missing from it is a counter the operator does not
/// see, and nothing about that is red — the page still renders, still adds up,
/// and quietly answers a question with one fewer number than it was asked.
/// Binding every field by name makes the compiler ask the author of the next
/// counter where it goes. That author is in another crate, which is exactly
/// the distance over which this kind of omission survives: `pull_failures`,
/// `pull_suppliers_connected` and `sourced` arrived on 2026-08-25 and only the
/// struct literal below failed to build — this function would have compiled
/// unchanged and shown fifteen of eighteen counters.
fn channel_fields(c: &ChannelStats) -> Vec<(&'static str, u64)> {
    let ChannelStats {
        accepted,
        fanned_out,
        delivered,
        pulled,
        sourced,
        dropped,
        dropped_overflow,
        unrelayable,
        dropped_on_disconnect,
        dropped_on_failure_disconnect,
        dropped_at_stop,
        push_failures,
        pull_failures,
        disconnected_for_failure,
        pull_rounds_cancelled,
        queued,
        consumers_connected,
        pull_consumers_connected,
        pull_suppliers_connected,
    } = c;
    vec![
        ("accepted", *accepted),
        ("fanned_out", *fanned_out),
        ("delivered", *delivered),
        ("pulled", *pulled),
        ("sourced", *sourced),
        ("dropped", *dropped),
        ("dropped_overflow", *dropped_overflow),
        ("unrelayable", *unrelayable),
        ("dropped_on_disconnect", *dropped_on_disconnect),
        ("dropped_on_failure_disconnect", *dropped_on_failure_disconnect),
        ("dropped_at_stop", *dropped_at_stop),
        ("push_failures", *push_failures),
        ("pull_failures", *pull_failures),
        ("disconnected_for_failure", *disconnected_for_failure),
        ("pull_rounds_cancelled", *pull_rounds_cancelled),
        ("queued", *queued as u64),
        ("consumers_connected", *consumers_connected as u64),
        ("pull_consumers_connected", *pull_consumers_connected as u64),
        ("pull_suppliers_connected", *pull_suppliers_connected as u64),
    ]
}

/// Reads one `u64` member, complaining rather than substituting a zero.
///
/// A counter the snapshot did not carry is not a counter that read zero, and
/// this is the whole reason the reader refuses a row instead of filling one in.
fn number(object: &Json, section: &str, key: &str, out: &mut Vec<String>) -> Option<u64> {
    match object.get(key) {
        Some(Json::Number(text)) => match text.parse::<u64>() {
            Ok(n) => Some(n),
            Err(_) => {
                out.push(format!("{section}: {key} is not a whole number: {text}"));
                None
            }
        },
        Some(other) => {
            out.push(format!("{section}: {key} was {}, expected a number", other.kind()));
            None
        }
        None => {
            out.push(format!("{section}: {key} is missing"));
            None
        }
    }
}

fn array<'a>(
    root: &'a Json,
    key: &str,
    out: &mut Vec<String>,
) -> Option<std::slice::Iter<'a, Json>> {
    match root.get(key) {
        None => None,
        Some(Json::Array(items)) => Some(items.iter()),
        Some(other) => {
            out.push(format!("{key}: was {}, expected an array", other.kind()));
            None
        }
    }
}

fn read_services(root: &Json, out: &mut Vec<String>) -> Option<Vec<Service>> {
    let items = array(root, "services", out)?;
    let mut services = Vec::new();
    for (i, item) in items.enumerate() {
        let Some(Json::String(id)) = item.get("id") else {
            out.push(format!("services[{i}]: id is missing or is not a string; row not shown"));
            continue;
        };
        let ior = match item.get("ior") {
            None | Some(Json::Null) => None,
            Some(Json::String(text)) => Some(text.clone()),
            Some(other) => {
                out.push(format!(
                    "services[{i}] ({id}): ior was {}, expected a string; row not shown",
                    other.kind()
                ));
                continue;
            }
        };
        let reserved = match item.get("reserved") {
            None => Reserved::NotStated,
            Some(Json::Bool(true)) => Reserved::Yes,
            Some(Json::Bool(false)) => Reserved::No,
            Some(other) => {
                out.push(format!(
                    "services[{i}] ({id}): reserved was {}, expected true or false",
                    other.kind()
                ));
                Reserved::NotStated
            }
        };
        services.push(Service { id: id.clone(), ior, reserved });
    }
    Some(services)
}

fn read_config(root: &Json, into: &mut BTreeMap<String, (String, Source)>, out: &mut Vec<String>) {
    let Some(items) = array(root, "config", out) else { return };
    let known: Vec<&'static str> = compiled().into_iter().map(|s| s.name).collect();
    for (i, item) in items.enumerate() {
        let Some(Json::String(name)) = item.get("name") else {
            out.push(format!("config[{i}]: name is missing or is not a string; row not shown"));
            continue;
        };
        if !known.contains(&name.as_str()) {
            // Not dropped and not shown as a setting: an unknown key is a fact
            // about the writer, and inventing an eighth ORB number would be the
            // console deciding something.
            out.push(format!("config[{i}]: {name} is not one of the ORB's seven numbers"));
            continue;
        }
        let Some(Json::String(value)) = item.get("value") else {
            out.push(format!("config[{i}] ({name}): value is missing or is not a string"));
            continue;
        };
        let Some(source) = read_source(item, &format!("config[{i}] ({name})"), out) else {
            continue;
        };
        into.insert(name.clone(), (value.clone(), source));
    }
}

fn read_source(item: &Json, at: &str, out: &mut Vec<String>) -> Option<Source> {
    let Some(source) = item.get("source") else {
        // The row is refused rather than defaulted to the compiled constant.
        // A value with an invented provenance is worse than no row: it is the
        // one column an operator came for, answered by a guess.
        out.push(format!(
            "{at}: no source — a value without where it came from is the half an operator needs"
        ));
        return None;
    };
    match source.get("kind").and_then(Json::as_str) {
        Some("compiled") => Some(Source::CompiledDefault),
        Some("file") => {
            match (
                source.get("file").and_then(Json::as_str),
                source.get("key").and_then(Json::as_str),
            ) {
                (Some(file), Some(key)) => {
                    Some(Source::ConfigFile { file: file.to_owned(), key: key.to_owned() })
                }
                _ => {
                    out.push(format!(
                        "{at}: source kind file needs both file and key — which file and which \
                         key is the answer an operator came for"
                    ));
                    None
                }
            }
        }
        Some("flag") => match source.get("flag").and_then(Json::as_str) {
            Some(flag) => Some(Source::Flag { flag: flag.to_owned() }),
            None => {
                out.push(format!("{at}: source kind flag needs the flag it names"));
                None
            }
        },
        Some(other) => {
            out.push(format!("{at}: source kind {other} is not compiled, file or flag"));
            None
        }
        None => {
            out.push(format!("{at}: source is missing its kind"));
            None
        }
    }
}

fn read_pool(root: &Json, out: &mut Vec<String>) -> Option<PoolStats> {
    let object = root.get("pool")?;
    let before = out.len();
    let stats = PoolStats {
        dialed: number(object, "pool", "dialed", out).unwrap_or_default(),
        reused: number(object, "pool", "reused", out).unwrap_or_default(),
        idle_evicted: number(object, "pool", "idle_evicted", out).unwrap_or_default(),
        faulted_evicted: number(object, "pool", "faulted_evicted", out).unwrap_or_default(),
        pressure_evicted: number(object, "pool", "pressure_evicted", out).unwrap_or_default(),
        retried: number(object, "pool", "retried", out).unwrap_or_default(),
        refused: number(object, "pool", "refused", out).unwrap_or_default(),
    };
    refuse_partial(out, before, "pool").then_some(stats)
}

fn read_server(root: &Json, out: &mut Vec<String>) -> Option<ServerCounters> {
    let object = root.get("server")?;
    let before = out.len();
    let stats = ServerCounters {
        accepted: number(object, "server", "accepted", out).unwrap_or_default(),
        refused: number(object, "server", "refused", out).unwrap_or_default(),
        active: number(object, "server", "active", out).unwrap_or_default(),
        peak_active: number(object, "server", "peak_active", out).unwrap_or_default(),
        at_servant: number(object, "server", "at_servant", out).unwrap_or_default(),
        peak_at_servant: number(object, "server", "peak_at_servant", out).unwrap_or_default(),
    };
    refuse_partial(out, before, "server").then_some(stats)
}

fn read_channels(root: &Json, out: &mut Vec<String>) -> Option<Vec<Channel>> {
    let items = array(root, "channels", out)?;
    let mut channels = Vec::new();
    for (i, item) in items.enumerate() {
        let Some(Json::String(name)) = item.get("name") else {
            out.push(format!("channels[{i}]: name is missing or is not a string; row not shown"));
            continue;
        };
        let at = format!("channels[{i}] ({name})");
        let before = out.len();
        let stats = ChannelStats {
            accepted: number(item, &at, "accepted", out).unwrap_or_default(),
            fanned_out: number(item, &at, "fanned_out", out).unwrap_or_default(),
            delivered: number(item, &at, "delivered", out).unwrap_or_default(),
            pulled: number(item, &at, "pulled", out).unwrap_or_default(),
            dropped: number(item, &at, "dropped", out).unwrap_or_default(),
            dropped_overflow: number(item, &at, "dropped_overflow", out).unwrap_or_default(),
            unrelayable: number(item, &at, "unrelayable", out).unwrap_or_default(),
            dropped_on_disconnect: number(item, &at, "dropped_on_disconnect", out)
                .unwrap_or_default(),
            dropped_on_failure_disconnect: number(item, &at, "dropped_on_failure_disconnect", out)
                .unwrap_or_default(),
            dropped_at_stop: number(item, &at, "dropped_at_stop", out).unwrap_or_default(),
            sourced: number(item, &at, "sourced", out).unwrap_or_default(),
            push_failures: number(item, &at, "push_failures", out).unwrap_or_default(),
            pull_failures: number(item, &at, "pull_failures", out).unwrap_or_default(),
            disconnected_for_failure: number(item, &at, "disconnected_for_failure", out)
                .unwrap_or_default(),
            pull_rounds_cancelled: number(item, &at, "pull_rounds_cancelled", out)
                .unwrap_or_default(),
            queued: number(item, &at, "queued", out).unwrap_or_default() as usize,
            consumers_connected: number(item, &at, "consumers_connected", out).unwrap_or_default()
                as usize,
            pull_consumers_connected: number(item, &at, "pull_consumers_connected", out)
                .unwrap_or_default() as usize,
            pull_suppliers_connected: number(item, &at, "pull_suppliers_connected", out)
                .unwrap_or_default() as usize,
        };
        if refuse_partial(out, before, &at) {
            channels.push(Channel { name: name.clone(), stats });
        }
    }
    Some(channels)
}

/// Whether a counter block read cleanly, and the sentence when it did not.
///
/// A block with one unreadable field is refused whole. The alternative is a row
/// of numbers with a silent zero in it, which is a page reporting a measurement
/// nobody took — and it would be the *drop* count that got the zero often
/// enough to matter.
fn refuse_partial(out: &mut Vec<String>, before: usize, section: &str) -> bool {
    if out.len() == before {
        return true;
    }
    out.push(format!("{section}: not shown — a counter block is read whole or not at all"));
    false
}

// ---------------------------------------------------------------------------
// rendering — services
// ---------------------------------------------------------------------------

/// The initial-references table, for a terminal.
pub fn render_services_text(snap: &Snapshot) -> String {
    let mut out = format!("INITIAL REFERENCES\nfrom {}\n\n", snap.origin);
    match &snap.services {
        None => out.push_str("the snapshot states no initial-references table\n"),
        Some(services) if services.is_empty() => {
            out.push_str("the table is stated and empty: no id is registered and none is known\n");
        }
        Some(services) => {
            let (registered, known): (Vec<&Service>, Vec<&Service>) =
                services.iter().partition(|s| s.registered());
            out.push_str(&format!("registered ({})\n", registered.len()));
            for service in &registered {
                out.push_str(&format!(
                    "  {:<24} {}\n    {}\n",
                    service.id,
                    service.reserved.short(),
                    service.ior.as_deref().unwrap_or_default()
                ));
            }
            if registered.is_empty() {
                out.push_str("  none\n");
            }
            out.push_str(&format!("\nknown, nothing registered ({})\n", known.len()));
            for service in &known {
                out.push_str(&format!("  {:<24} {}\n", service.id, service.reserved.short()));
            }
            if known.is_empty() {
                out.push_str("  none\n");
            }
        }
    }
    out.push_str(&format!("\n{RESOLUTION_NOTE}\n"));
    out.push_str(&complaints_text(snap));
    out
}

/// The initial-references table, as one page.
pub fn render_services_html(snap: &Snapshot) -> String {
    let mut body = Markup::labelled("h1", "", "Initial references");
    body.push(Markup::labelled("p", "sub", &format!("from {}", snap.origin)));
    match &snap.services {
        None => body.push(Markup::labelled(
            "p",
            "absent",
            "the snapshot states no initial-references table",
        )),
        Some(services) if services.is_empty() => body.push(Markup::labelled(
            "p",
            "absent",
            "the table is stated and empty: no id is registered and none is known",
        )),
        Some(services) => {
            let registered = services.iter().filter(|s| s.registered()).count();
            let mut summary = Markup::empty();
            for (n, what, class) in [
                (registered, "registered", "stat"),
                (services.len() - registered, "known, nothing registered", "stat warn"),
            ] {
                let mut inner = Markup::labelled("b", "", &n.to_string());
                inner.push(Markup::text(&format!(" {what}")));
                summary.push(Markup::element("div", class, inner));
            }
            body.push(Markup::element("div", "card", Markup::element("div", "summary", summary)));

            let mut head = Markup::empty();
            for column in ["object id", "§8.5.2", "registration", "reference"] {
                head.push(Markup::labelled("th", "", column));
            }
            let mut rows = Markup::element("tr", "", head);
            for service in services {
                let mut cells =
                    Markup::element("td", "", Markup::labelled("span", "id", &service.id));
                cells.push(Markup::element(
                    "td",
                    "",
                    Markup::labelled("span", service.reserved.badge(), service.reserved.short()),
                ));
                match &service.ior {
                    Some(ior) => {
                        cells.push(Markup::element(
                            "td",
                            "",
                            Markup::labelled("span", "badge b-ok", "registered"),
                        ));
                        cells.push(Markup::element(
                            "td",
                            "",
                            Markup::labelled("span", "mono", ior),
                        ));
                    }
                    None => {
                        cells.push(Markup::element(
                            "td",
                            "",
                            Markup::labelled("span", "badge b-unknown", "nothing registered"),
                        ));
                        cells.push(Markup::labelled(
                            "td",
                            "absent",
                            "no reference — a peer answers NO_RESOURCES",
                        ));
                    }
                }
                let class = if service.registered() { "" } else { "row-dry" };
                rows.push(Markup::element("tr", class, cells));
            }
            body.push(Markup::element("div", "scroll", Markup::element("table", "", rows)));
        }
    }
    body.push(Markup::labelled("p", "note", RESOLUTION_NOTE));
    body.push(complaints_markup(snap));
    body.push(provenance_footer());
    page("Initial references — orbweaver-console", body)
}

// ---------------------------------------------------------------------------
// rendering — config
// ---------------------------------------------------------------------------

/// What the provenance column can say today, said once so both renderings and
/// the documentation agree.
pub const PROVENANCE_NOTE: &str = "\
Where a value came from is the half an operator needs: a number without its \
origin cannot be changed, because nobody knows which lever moves it. Today \
every ORB number is a compile-time constant, so every row here honestly reads \
compiled default — that is this build's answer and not a placeholder. The \
column has two further answers ready, a configuration file with its key and a \
flag with its name, and the configuration that will give them is D019 step 3.";

/// The seven numbers, for a terminal.
pub fn render_config_text(snap: &Snapshot) -> String {
    let mut out = format!("ORB CONFIGURATION\nfrom {}\n\n", snap.origin);
    for setting in snap.config() {
        out.push_str(&format!(
            "  {:<20} {:<20} {}\n      {}\n",
            setting.name,
            setting.value,
            setting.source.label(),
            setting.what
        ));
    }
    out.push_str(&format!("\n{PROVENANCE_NOTE}\n"));
    out.push_str(&complaints_text(snap));
    out
}

/// The seven numbers, as one page.
pub fn render_config_html(snap: &Snapshot) -> String {
    let mut body = Markup::labelled("h1", "", "ORB configuration");
    body.push(Markup::labelled("p", "sub", &format!("from {}", snap.origin)));

    let mut head = Markup::empty();
    for column in ["setting", "value", "where it came from", "what it does"] {
        head.push(Markup::labelled("th", "", column));
    }
    let mut rows = Markup::element("tr", "", head);
    for setting in snap.config() {
        let mut cells = Markup::element("td", "", Markup::labelled("span", "mono", setting.name));
        cells.push(Markup::element("td", "", Markup::labelled("span", "mono", &setting.value)));
        let mut origin = Markup::labelled("span", setting.source.badge(), &setting.source.label());
        if let Source::CompiledDefault = setting.source {
            origin.push(Markup::labelled("p", "note", "the constant this tool was built against"));
        }
        cells.push(Markup::element("td", "", origin));
        cells.push(Markup::labelled("td", "note", setting.what));
        rows.push(Markup::element("tr", "", cells));
    }
    body.push(Markup::element("div", "scroll", Markup::element("table", "", rows)));
    body.push(Markup::labelled("p", "note", PROVENANCE_NOTE));
    body.push(complaints_markup(snap));
    body.push(provenance_footer());
    page("ORB configuration — orbweaver-console", body)
}

// ---------------------------------------------------------------------------
// rendering — stats
// ---------------------------------------------------------------------------

/// The three counter blocks, for a terminal.
pub fn render_stats_text(snap: &Snapshot) -> String {
    let mut out = format!("ORB COUNTERS\nfrom {}\n\nCONNECTION POOL\n", snap.origin);
    match &snap.pool {
        None => out.push_str("  the snapshot states no pool counters\n"),
        Some(pool) => {
            for (name, value) in pool_fields(pool) {
                out.push_str(&format!("  {name:<28} {value}\n"));
            }
        }
    }
    out.push_str("\nSERVER\n");
    match &snap.server {
        None => out.push_str("  the snapshot states no server counters\n"),
        Some(server) => {
            for (name, value) in server_fields(server) {
                out.push_str(&format!("  {name:<28} {value}\n"));
            }
        }
    }
    out.push_str("\nEVENT CHANNELS\n");
    match &snap.channels {
        None => out.push_str("  the snapshot states no event channels\n"),
        Some(channels) if channels.is_empty() => {
            out.push_str("  the list is stated and empty: no channel exists\n");
        }
        Some(channels) => {
            for channel in channels {
                out.push_str(&format!("\n  channel {}\n", channel.name));
                for (name, value) in channel_fields(&channel.stats) {
                    if name.starts_with("dropped") || name == "unrelayable" {
                        continue;
                    }
                    out.push_str(&format!("    {name:<30} {value}\n"));
                }
                out.push_str("    drops, by cause\n");
                for (cause, value, what) in drop_causes(&channel.stats) {
                    out.push_str(&format!("      {cause:<32} {value}   {what}\n"));
                }
                out.push_str(&format!(
                    "    reported by the channel            {}\n    {}\n",
                    channel.stats.dropped,
                    reconciliation(&channel.stats)
                ));
            }
        }
    }
    out.push_str(&format!("\n{DROP_NOTE}\n"));
    out.push_str(&complaints_text(snap));
    out
}

/// Why the drop count is five numbers and not one, said once.
pub const DROP_NOTE: &str = "\
The drop count is shown by cause and never as a single total. A clean stop and \
an overloaded consumer moved the same number while it was one counter, so no \
reading of it could tell back-pressure from housekeeping (D011 §6.1) — and \
re-summing the five here would put that back. Exactly one of the five, \
dropped_overflow, means back-pressure. The total beside them is the channel's \
own report, and the reconciliation is the channel's own split_adds_up().";

/// The three counter blocks, as one page.
pub fn render_stats_html(snap: &Snapshot) -> String {
    let mut body = Markup::labelled("h1", "", "ORB counters");
    body.push(Markup::labelled("p", "sub", &format!("from {}", snap.origin)));

    body.push(Markup::labelled("h2", "", "Connection pool"));
    match &snap.pool {
        None => {
            body.push(Markup::labelled("p", "absent", "the snapshot states no pool counters"));
        }
        Some(pool) => body.push(counter_table(&pool_fields(pool))),
    }

    body.push(Markup::labelled("h2", "", "Server"));
    match &snap.server {
        None => {
            body.push(Markup::labelled("p", "absent", "the snapshot states no server counters"));
        }
        Some(server) => body.push(counter_table(&server_fields(server))),
    }

    body.push(Markup::labelled("h2", "", "Event channels"));
    match &snap.channels {
        None => {
            body.push(Markup::labelled("p", "absent", "the snapshot states no event channels"));
        }
        Some(channels) if channels.is_empty() => body.push(Markup::labelled(
            "p",
            "absent",
            "the list is stated and empty: no channel exists",
        )),
        Some(channels) => {
            for channel in channels {
                let mut card = Markup::labelled("p", "id", &channel.name);
                let moved: Vec<(&'static str, u64)> = channel_fields(&channel.stats)
                    .into_iter()
                    .filter(|(name, _)| !name.starts_with("dropped") && *name != "unrelayable")
                    .collect();
                card.push(counter_table(&moved));

                let mut head = Markup::empty();
                for column in ["drop cause", "events", "what it means"] {
                    head.push(Markup::labelled("th", "", column));
                }
                let mut rows = Markup::element("tr", "", head);
                for (cause, value, what) in drop_causes(&channel.stats) {
                    let mut cells =
                        Markup::element("td", "", Markup::labelled("span", "mono", cause));
                    cells.push(Markup::element(
                        "td",
                        "",
                        Markup::labelled("b", "", &value.to_string()),
                    ));
                    cells.push(Markup::labelled("td", "note", what));
                    let class =
                        if cause == "dropped_overflow" && value > 0 { "row-refuse" } else { "" };
                    rows.push(Markup::element("tr", class, cells));
                }
                card.push(Markup::element("div", "scroll", Markup::element("table", "", rows)));

                let reconciles = channel.stats.split_adds_up();
                card.push(Markup::labelled(
                    "p",
                    if reconciles { "note" } else { "badge b-destructive" },
                    &reconciliation(&channel.stats),
                ));
                body.push(Markup::element("div", "card", card));
            }
        }
    }
    body.push(Markup::labelled("p", "note", DROP_NOTE));
    body.push(complaints_markup(snap));
    body.push(provenance_footer());
    page("ORB counters — orbweaver-console", body)
}

fn counter_table(fields: &[(&'static str, u64)]) -> Markup {
    let mut head = Markup::empty();
    for column in ["counter", "value"] {
        head.push(Markup::labelled("th", "", column));
    }
    let mut rows = Markup::element("tr", "", head);
    for (name, value) in fields {
        let mut cells = Markup::element("td", "", Markup::labelled("span", "mono", name));
        cells.push(Markup::element("td", "", Markup::labelled("b", "", &value.to_string())));
        rows.push(Markup::element("tr", "", cells));
    }
    Markup::element("div", "scroll", Markup::element("table", "", rows))
}

// ---------------------------------------------------------------------------
// complaints
// ---------------------------------------------------------------------------

fn complaints_text(snap: &Snapshot) -> String {
    if snap.complaints.is_empty() {
        return String::new();
    }
    let mut out = format!("\nUNREADABLE ({})\n", snap.complaints.len());
    for complaint in &snap.complaints {
        out.push_str(&format!("  {complaint}\n"));
    }
    out
}

fn complaints_markup(snap: &Snapshot) -> Markup {
    if snap.complaints.is_empty() {
        return Markup::empty();
    }
    let mut card = Markup::labelled("h2", "", &format!("Not readable ({})", snap.complaints.len()));
    card.push(Markup::labelled(
        "p",
        "note",
        "Counted rather than skipped: a section this page could not read is a failure, never a \
         silently smaller table.",
    ));
    for complaint in &snap.complaints {
        card.push(Markup::labelled("p", "mono", complaint));
    }
    Markup::element("div", "card", card)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn services() -> Vec<Service> {
        vec![
            Service {
                id: "NameService".into(),
                ior: Some("IOR:0100".into()),
                reserved: Reserved::Yes,
            },
            Service { id: "RootPOA".into(), ior: None, reserved: Reserved::Yes },
            Service {
                id: "OurOwnThing".into(),
                ior: Some("IOR:0200".into()),
                reserved: Reserved::No,
            },
            Service { id: "Unlabelled".into(), ior: None, reserved: Reserved::NotStated },
        ]
    }

    fn channel(stats: ChannelStats) -> Vec<Channel> {
        vec![Channel { name: "alerts".into(), stats }]
    }

    fn reconciling() -> ChannelStats {
        ChannelStats {
            accepted: 10,
            fanned_out: 20,
            delivered: 14,
            dropped: 6,
            dropped_overflow: 3,
            unrelayable: 1,
            dropped_on_disconnect: 1,
            dropped_on_failure_disconnect: 0,
            dropped_at_stop: 1,
            pulled: 0,
            sourced: 0,
            push_failures: 2,
            pull_failures: 0,
            disconnected_for_failure: 0,
            pull_rounds_cancelled: 0,
            queued: 4,
            consumers_connected: 2,
            pull_consumers_connected: 1,
            pull_suppliers_connected: 0,
        }
    }

    fn full() -> Snapshot {
        Snapshot::live(
            "a test",
            Some(services()),
            Some(PoolStats { dialed: 3, reused: 9, refused: 1, ..PoolStats::default() }),
            None,
            Some(channel(reconciling())),
        )
    }

    // -- services ----------------------------------------------------------

    /// Registered and reserved-but-unregistered are different rows with
    /// different words, because a peer gives them different answers.
    #[test]
    fn a_reserved_id_with_nothing_bound_is_not_shown_as_registered() {
        let snap = full();
        let text = render_services_text(&snap);
        assert!(text.contains("registered (2)"), "{text}");
        assert!(text.contains("known, nothing registered (2)"), "{text}");
        let html = render_services_html(&snap);
        assert!(html.contains("nothing registered"), "{html}");
        assert!(html.contains("NO_RESOURCES"), "{html}");
        assert!(html.contains("BAD_PARAM"), "{html}");
    }

    /// The three states of reservedness are three renderings. An id the writer
    /// said nothing about must not read as *not reserved*.
    #[test]
    fn reservedness_the_writer_did_not_state_is_not_rendered_as_no() {
        let html = render_services_html(&full());
        assert!(html.contains("not stated"), "{html}");
        assert_ne!(Reserved::NotStated.short(), Reserved::No.short());
        assert_ne!(Reserved::NotStated.label(), Reserved::No.label());
    }

    /// A table nobody stated is not an empty table.
    #[test]
    fn an_absent_services_table_is_not_an_empty_one() {
        let none = Snapshot { origin: "x".into(), ..Snapshot::default() };
        assert!(render_services_text(&none).contains("states no initial-references table"));
        let empty = Snapshot { services: Some(Vec::new()), ..none.clone() };
        let text = render_services_text(&empty);
        assert!(text.contains("stated and empty"), "{text}");
        assert!(!text.contains("states no initial-references table"), "{text}");
    }

    // -- config ------------------------------------------------------------

    /// Seven numbers, every one of them saying where it came from, and today
    /// every one of them saying the same true thing.
    #[test]
    fn every_setting_names_its_origin_and_today_every_origin_is_the_compiled_default() {
        let settings = compiled();
        assert_eq!(settings.len(), 7, "{settings:?}");
        for setting in &settings {
            assert_eq!(setting.source, Source::CompiledDefault, "{}", setting.name);
        }
        let text = render_config_text(&Snapshot::default());
        for setting in &settings {
            assert!(text.contains(setting.name), "{} missing from {text}", setting.name);
        }
        assert_eq!(text.matches("compiled default").count(), 8, "seven rows and the note: {text}");
    }

    /// The values are the ORB's constants, not a second copy of them.
    #[test]
    fn the_values_come_from_the_constants_that_own_them() {
        let settings = compiled();
        let by_name = |name: &str| {
            settings
                .iter()
                .find(|s| s.name == name)
                .unwrap_or_else(|| panic!("{name}"))
                .value
                .clone()
        };
        assert_eq!(by_name("max_message_size"), format!("{DEFAULT_MAX_MESSAGE_SIZE} bytes"));
        assert_eq!(by_name("max_forward_hops"), MAX_FORWARD_HOPS.to_string());
        assert_eq!(by_name("follow_timeout"), format!("{} ms", FOLLOW_TIMEOUT.as_millis()));
        assert_eq!(by_name("fragment_threshold"), format!("{DEFAULT_FRAGMENT_THRESHOLD} bytes"));
        assert_eq!(by_name("max_fragments"), MAX_FRAGMENTS.to_string());
        assert_eq!(
            by_name("max_connections"),
            orbweaver_giop::server::DEFAULT_MAX_CONNECTIONS.to_string()
        );
        assert_eq!(
            by_name("stop_poll"),
            format!("{} ms", orbweaver_giop::server::STOP_POLL.as_millis())
        );
    }

    /// The other two origins are not hypothetical: a snapshot that states one
    /// renders it, naming the lever. This is the shape D019 step 3 fills in.
    #[test]
    fn a_stated_origin_replaces_the_compiled_default_and_names_the_lever() {
        let document = r#"{"config":[
            {"name":"max_connections","value":"512","source":{"kind":"file","file":"orb.json","key":"max_connections"}},
            {"name":"stop_poll","value":"5 ms","source":{"kind":"flag","flag":"-ORBstopPoll"}}
        ]}"#;
        let snap = Snapshot::read("orb.json", document).expect("reads");
        assert!(snap.complaints.is_empty(), "{:?}", snap.complaints);
        let settings = snap.config();
        assert_eq!(settings.len(), 7, "the seven stay seven");
        let file = settings.iter().find(|s| s.name == "max_connections").expect("row");
        assert_eq!(file.value, "512");
        assert_eq!(file.source.label(), "configuration file orb.json, key max_connections");
        let flag = settings.iter().find(|s| s.name == "stop_poll").expect("row");
        assert_eq!(flag.source.label(), "flag -ORBstopPoll");
        let text = render_config_text(&snap);
        assert!(text.contains("configuration file orb.json, key max_connections"), "{text}");
        assert!(text.contains("flag -ORBstopPoll"), "{text}");
    }

    /// An eighth number is not invented, and a source that names no lever is
    /// refused rather than shown as an origin nobody can act on.
    #[test]
    fn an_unknown_key_and_a_lever_less_source_are_complaints_not_rows() {
        let document = r#"{"config":[
            {"name":"turbo","value":"on","source":{"kind":"compiled"}},
            {"name":"max_fragments","value":"9","source":{"kind":"file","file":"orb.json"}}
        ]}"#;
        let snap = Snapshot::read("orb.json", document).expect("reads");
        assert_eq!(snap.complaints.len(), 2, "{:?}", snap.complaints);
        assert!(snap.complaints[0].contains("not one of the ORB's seven numbers"));
        assert!(snap.complaints[1].contains("which file and which key"));
        let settings = snap.config();
        assert_eq!(settings.len(), 7);
        let untouched = settings.iter().find(|s| s.name == "max_fragments").expect("row");
        assert_eq!(untouched.source, Source::CompiledDefault, "a refused row changes nothing");
    }

    /// A row with a value and **no source at all** is refused and named.
    ///
    /// This one is here because the first version of the reader dropped it in
    /// silence: `read_source` began `let source = item.get("source")?`, and a
    /// `?` on an `Option` in a function whose complaints are a side channel is
    /// a skip nobody sees. Nothing was red — the row simply was not there, and
    /// the page showed the compiled default beside it, which is the console
    /// answering the one question an operator came with by guessing.
    #[test]
    fn a_value_with_no_source_at_all_is_refused_and_named() {
        let document = r#"{"config":[{"name":"max_fragments","value":"9"}]}"#;
        let snap = Snapshot::read("orb.json", document).expect("reads");
        assert_eq!(snap.complaints.len(), 1, "{:?}", snap.complaints);
        assert!(snap.complaints[0].contains("no source"), "{:?}", snap.complaints);
        let row = snap.config().into_iter().find(|s| s.name == "max_fragments").expect("row");
        assert_eq!(row.source, Source::CompiledDefault);
        assert_eq!(row.value, format!("{MAX_FRAGMENTS}"), "the refused value did not land");
        assert!(render_config_text(&snap).contains("UNREADABLE"));
    }

    // -- stats -------------------------------------------------------------

    /// The five causes are five rows, and the total beside them is the
    /// channel's own report rather than a sum taken here.
    #[test]
    fn the_drop_split_is_rendered_by_cause_and_never_as_one_number() {
        let snap = full();
        let text = render_stats_text(&snap);
        for cause in [
            "dropped_overflow",
            "unrelayable",
            "dropped_on_disconnect",
            "dropped_on_failure_disconnect",
            "dropped_at_stop",
        ] {
            assert!(text.contains(cause), "{cause} missing from {text}");
        }
        assert!(text.contains("reported by the channel"), "{text}");
        assert!(text.contains("the five causes account for every drop"), "{text}");
        let html = render_stats_html(&snap);
        for cause in ["dropped_overflow", "dropped_at_stop"] {
            assert!(html.contains(cause), "{cause} missing from {html}");
        }
        assert!(html.contains("back-pressure"), "{html}");
    }

    /// The verdict is the channel's function, so a split that does not add up
    /// makes the page say so instead of showing numbers that disagree.
    #[test]
    fn a_split_that_does_not_reconcile_is_said_rather_than_shown() {
        let mut stats = reconciling();
        stats.dropped += 4; // a discard path that named no cause
        assert!(!stats.split_adds_up());
        let snap = Snapshot::live("a test", None, None, None, Some(channel(stats)));
        let text = render_stats_text(&snap);
        assert!(text.contains("THE SPLIT DOES NOT RECONCILE"), "{text}");
        assert!(text.contains("Do not read either number as the drop count"), "{text}");
        let html = render_stats_html(&snap);
        assert!(html.contains("THE SPLIT DOES NOT RECONCILE"), "{html}");
    }

    /// Absent counters are absent, not zero.
    #[test]
    fn absent_counter_blocks_are_rendered_absent() {
        let snap = Snapshot { origin: "x".into(), ..Snapshot::default() };
        let text = render_stats_text(&snap);
        assert!(text.contains("states no pool counters"), "{text}");
        assert!(text.contains("states no server counters"), "{text}");
        assert!(text.contains("states no event channels"), "{text}");
        let html = render_stats_html(&snap);
        assert!(html.contains("states no pool counters"), "{html}");
    }

    /// A live handle is read once, through its own accessors.
    #[test]
    fn the_server_block_is_the_live_handles_own_answer() {
        let stats = ServerStats::default();
        let read = ServerCounters::of(&stats);
        assert_eq!(read.accepted, stats.accepted());
        assert_eq!(read.peak_at_servant, stats.peak_at_servant());
        let snap = Snapshot::live("in process", None, None, Some(&stats), None);
        assert_eq!(snap.server, Some(read));
    }

    // -- the snapshot ------------------------------------------------------

    /// The two halves of the format agree, which is the only thing that can be
    /// proved about a format with one writer and one reader.
    #[test]
    fn a_snapshot_survives_the_round_trip_it_was_written_for() {
        let before = full();
        let document = before.to_json();
        let after = Snapshot::read("round-trip.json", &document).expect("reads");
        assert!(after.complaints.is_empty(), "{:?}", after.complaints);
        assert_eq!(after.services, before.services);
        assert_eq!(after.pool, before.pool);
        assert_eq!(after.channels, before.channels);
        assert_eq!(
            render_stats_text(&after).replace(&after.origin, ""),
            render_stats_text(&before).replace(&before.origin, "")
        );
    }

    /// A counter block with one unreadable field is refused whole. Half a block
    /// with a zero in it is a page reporting a measurement nobody took.
    #[test]
    fn a_partly_unreadable_counter_block_is_refused_rather_than_zero_filled() {
        let snap = Snapshot::read("x.json", r#"{"pool":{"dialed":3}}"#).expect("reads");
        assert_eq!(snap.pool, None, "not a PoolStats with six zeroes in it");
        assert!(
            snap.complaints.iter().any(|c| c.contains("reused is missing")),
            "{:?}",
            snap.complaints
        );
        assert!(
            snap.complaints.iter().any(|c| c.contains("read whole or not at all")),
            "{:?}",
            snap.complaints
        );
        let text = render_stats_text(&snap);
        assert!(text.contains("UNREADABLE"), "{text}");
        assert!(text.contains("states no pool counters"), "{text}");
    }

    /// A section of the wrong shape is a named complaint, and the rest of the
    /// page still renders.
    #[test]
    fn a_section_of_the_wrong_shape_is_named_and_the_rest_survives() {
        let snap = Snapshot::read("x.json", r#"{"services":7,"pool":{"dialed":1,"reused":1,"idle_evicted":1,"faulted_evicted":1,"pressure_evicted":1,"retried":1,"refused":1}}"#)
            .expect("reads");
        assert_eq!(snap.services, None);
        assert!(snap.pool.is_some(), "the readable section still reads");
        assert!(
            snap.complaints.iter().any(|c| c.contains("expected an array")),
            "{:?}",
            snap.complaints
        );
    }

    /// A document that is not a snapshot at all is an error, not an empty page.
    #[test]
    fn a_document_that_is_not_an_object_is_refused() {
        let err = Snapshot::read("x.json", "[]").expect_err("refused");
        assert!(err.contains("a snapshot is a JSON object"), "{err}");
        let err = Snapshot::read("x.json", "{").expect_err("refused");
        assert!(err.contains("x.json"), "{err}");
    }
}
