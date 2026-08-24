//! The catalog view: what exists, what an agent can reach, and where it came
//! from.
//!
//! # This module decides nothing
//!
//! Every verdict on this page is [`orbweaver_mcp::dryrun::survey`]'s, run
//! against the caller's own [`Chain`] — the same walk a real call takes, so a
//! page cannot disagree with the gate that will actually answer. Provenance is
//! [`Registry::origin`] and [`Registry::touches_ingested`]. Nothing here reads
//! `ai_authz` or `ai_effect` and forms an opinion; if it did there would be two
//! exposure policies and only one of them would be the one that runs.
//!
//! # Two passes, and why
//!
//! An operator asks two different questions about one operation:
//!
//! * *what happens to this caller* — the configured caller's own prediction;
//! * *what does the contract require of anybody* — which scope gates it, and
//!   whether it needs a human.
//!
//! The gate only names a scope when it refuses for the want of it, so a caller
//! who already holds `accounts:write` produces a row that says `allow` and
//! names no scope. Inventing the requirement from the annotation would be this
//! module forming the opinion it must not. So the requirement is read from a
//! **second survey with no caller at all**, where the same gate refuses
//! `NotAuthenticated` and states the scope it wanted. The scope on the page is
//! therefore always a scope the gate spoke, never one this module inferred.
//!
//! That pass has a limit, and it is rendered rather than smoothed over: an
//! operation refused at `authz.exposure` never reaches `authz.scopes`, so its
//! requirement is **not reached**, which is not the same as "requires nothing".
//! [`StageOutcome::NotReached`] is the chain's own word for that and the page
//! reads it directly, rather than inferring it from which stage refused — a
//! deployment may put its own stage anywhere, and a guess from a stage name
//! would silently become wrong the day it does.
//!
//! # What a peer can enforce is on the page too
//!
//! PLAN §4.8: CSIv2 support is per-peer, and where a target cannot enforce a
//! caller identity the bridge is the only enforcement point — *and the
//! catalogue has to say so*. An operator who hands the page a peer's reference
//! ([`Catalog::attach_peer`], the binary's `--ior`) gets that peer's record
//! beside the interface its `type_id` names: whether the target enforces
//! identity, whether its transport is secured, and where enforcement happens.
//! The record is [`orbweaver_mcp::identity::PeerCapability`]'s, read off the
//! IOR by the same code the bridge decides with; this module renders its
//! sentences and forms no opinion of its own about a tagged component. A peer
//! whose type is not in the catalog is still rendered — the record is a fact
//! about the IOR, not about the catalog — under its own heading. An interface
//! nobody handed a reference for says so: what its targets can enforce is
//! *unmeasured here*, which is not the same as "bridge only".
//!
//! [`Chain`]: orbweaver_mcp::interceptor::Chain
//! [`StageOutcome::NotReached`]: orbweaver_mcp::interceptor::StageOutcome

use orbweaver_dynamic::json::Json;
use orbweaver_giop::Ior;
use orbweaver_mcp::dryrun::{self, Would};
use orbweaver_mcp::identity::{Caller, PeerCapability};
use orbweaver_mcp::interceptor::{CallContext, Chain, STAGE_SCOPES, StageOutcome};
use orbweaver_mcp::policy::{Approval, Denied, Exposure};
use orbweaver_registry::{Origin, Registry};

use crate::declarations::{self, DeclarationRow};
use crate::html::{Markup, page, provenance_footer};

/// What the contract asks of a caller before this operation is reachable, as
/// the gate stated it to a session nobody is signed into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requires {
    /// The scopes stage ran and had no objection: the contract names no
    /// `ai_authz` this gate would enforce.
    Nothing,
    /// The scopes stage refused and named this scope.
    Scope(String),
    /// The scopes stage never ran, because an earlier stage refused. **Not**
    /// the same as [`Requires::Nothing`].
    NotReached,
    /// This chain has no scopes stage at all, so nothing in it enforces a
    /// scope. Reported rather than greened, per the harness rule that an
    /// unmeasured check is never a pass.
    Unenforced,
}

impl Requires {
    /// How it reads on a page.
    pub fn label(&self) -> &str {
        match self {
            Requires::Nothing => "none stated",
            Requires::Scope(s) => s,
            Requires::NotReached => "not reached — an earlier stage refused",
            Requires::Unenforced => "unenforced — this chain has no scopes stage",
        }
    }
}

/// One operation, with the gate's answer for it.
#[derive(Debug, Clone)]
pub struct OperationRow {
    /// Operation name.
    pub name: String,
    /// [`orbweaver_mcp::dryrun::Would`]'s name for what the chain answered, for
    /// the configured caller. Carried as the gate's own string so this crate
    /// cannot invent a category the gate does not have.
    pub would: String,
    /// Whether the contract declares the operation. `false` beside `allow` is a
    /// real finding: the gates check permission, not existence.
    pub declared: bool,
    /// The stage that refused, when one did.
    pub stage: Option<String>,
    /// Why it refused, in the gate's own words.
    pub why: Option<String>,
    /// What the contract requires of anybody. See the module docs.
    pub requires: Requires,
    /// The `ai_effect` value that makes this need a human, when it does.
    /// Present whether or not an approval is in hand.
    pub effect: Option<String>,
    /// Who may approve, when the contract says.
    pub approver: Option<String>,
}

impl OperationRow {
    /// Whether this operation needs a human before it runs.
    pub fn destructive(&self) -> bool {
        self.effect.is_some()
    }
}

/// One peer's capability record, as the bridge read it off the peer's IOR.
///
/// The sentences are [`PeerCapability`]'s own; this crate carries them and
/// draws them. `label` is what the operator called the reference — a file
/// path, a handle — and `type_id` is what the IOR itself claims to be, which
/// a peer chose and is therefore untrusted text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRow {
    /// The operator's name for the reference.
    pub label: String,
    /// The repository id the IOR names. Untrusted: the peer wrote it.
    pub type_id: String,
    /// Whether the target advertises a mechanism that accepts an asserted
    /// identity — [`PeerCapability::enforces_identity`].
    pub enforces_identity: bool,
    /// Whether the target advertises `TAG_SSL_SEC_TRANS` —
    /// [`PeerCapability::transport_secured`].
    pub transport_secured: bool,
    /// `target` or `bridge only` — [`PeerCapability::enforcement_point`].
    pub enforcement_point: String,
    /// The identity half, in the bridge's words.
    pub identity: String,
    /// The transport half, in the bridge's words.
    pub transport: String,
}

impl PeerRow {
    /// Reads a peer's record off its reference. The IOR itself is not kept.
    pub fn of(label: impl Into<String>, ior: &Ior) -> Self {
        let record = PeerCapability::of_ior(ior);
        Self {
            label: label.into(),
            type_id: ior.type_id.clone(),
            enforces_identity: record.enforces_identity(),
            transport_secured: record.transport_secured(),
            enforcement_point: record.enforcement_point().as_str().to_owned(),
            identity: record.identity_sentence(),
            transport: record.transport_sentence(),
        }
    }

    /// Whether the bridge is this target's only enforcement point.
    pub fn bridge_only(&self) -> bool {
        !self.enforces_identity
    }
}

/// One interface, with its provenance and its exposure.
#[derive(Debug, Clone)]
pub struct InterfaceRow {
    /// Repository id. Untrusted text when [`InterfaceRow::origin`] is
    /// [`Origin::Ingested`] — a peer chose it.
    pub id: String,
    /// Whether the exposure allowlists it, as [`Exposure::exposes`] answers.
    pub exposed: bool,
    /// Whether the catalog knows the id at all.
    pub known: bool,
    /// Where the entry came from.
    pub origin: Origin,
    /// Whether this or anything it inherits from came off the wire.
    pub touches_ingested: bool,
    /// The `ai_desc` prose, when there is any. Untrusted: it is free text an
    /// author or a peer wrote.
    pub ai_desc: Option<String>,
    /// Operations, sorted, inherited included.
    pub operations: Vec<OperationRow>,
    /// The peers an operator handed the page whose IOR names this interface,
    /// in the order they were attached. Empty means *no reference supplied* —
    /// what this interface's targets can enforce is unmeasured on this page,
    /// which the render says in words rather than by leaving a gap.
    pub peers: Vec<PeerRow>,
}

impl InterfaceRow {
    /// Whether the id itself came off the wire, as opposed to an ancestor.
    pub fn ingested(&self) -> bool {
        matches!(self.origin, Origin::Ingested(_))
    }

    /// The ingestion source label, when there is one.
    pub fn source(&self) -> Option<&str> {
        match &self.origin {
            Origin::Ingested(s) => Some(s.as_str()),
            Origin::Idl => None,
        }
    }
}

/// The whole catalog, as one caller sees it.
#[derive(Debug, Clone)]
pub struct Catalog {
    /// The principal the predictions were run for, as the gate rendered it.
    pub caller: String,
    /// The scopes that caller holds.
    pub scopes: Vec<String>,
    /// Whether a host approval was in hand for the predictions.
    pub destructive_approved: bool,
    /// What this exposure declares an operation whose contract states no
    /// `ai_effect` is to be treated as — `"refuse"`, or the effect an operator
    /// assumed for the silences. The survey states it once at the top because
    /// it conditions every row underneath it, and a page of rows read under
    /// the wrong one is a page read wrong. Carried as the gate's own string.
    pub unannotated_effect: String,
    /// The gate's answer for every operation in the catalog, tallied in
    /// [`Would::ALL`] order and named in the gate's own vocabulary.
    ///
    /// Summed from each interface's own `summary`, which the survey computes;
    /// addition is the only thing done to it here. It exists because the
    /// counts beside it are *properties of the contracts* — how many are
    /// exposed, how many are marked destructive — and on an estate that
    /// annotates nothing, every one of those is zero while the gate is
    /// refusing every operation it is asked about. A summary of zeroes reads
    /// as "nothing to worry about" and was, measurably, the opposite.
    pub would_counts: Vec<(String, usize)>,
    /// Every interface in the registry, sorted by repository id.
    pub interfaces: Vec<InterfaceRow>,
    /// Everything else the loaded contracts declare — constants with their
    /// folded values, structs, unions, enums, exceptions, typedefs,
    /// valuetypes, natives — sorted by repository id.
    ///
    /// Here because the page is titled *what exists*, and until 2026-08-24 it
    /// answered that with interfaces alone: 151 of the golden corpus's 208
    /// registry entries reached no reader surface, and two of its files
    /// rendered a page that said "the catalog is empty" over 23 declarations.
    /// See [`crate::declarations`] for the measurement and for the one thing
    /// this list deliberately does not spell.
    pub declarations: Vec<DeclarationRow>,
    /// Ids the exposure allowlists that the catalog does not have — a typo, a
    /// stale id, or an id mis-parsed on the way in. An exposure line that
    /// allowlists nothing is a misconfiguration an operator wants to see before
    /// a deployment rather than after one.
    pub unknown_exposures: Vec<String>,
    /// Peers whose IOR names a type the catalog does not have. Their records
    /// are still on the page: what a target can enforce is a fact about the
    /// IOR, and an operator who supplied the reference wants the answer
    /// whether or not the contract for it has been loaded.
    pub unmatched_peers: Vec<PeerRow>,
}

impl Catalog {
    /// Attaches a peer's capability record to the interface its IOR names, or
    /// to [`Catalog::unmatched_peers`] when the catalog has no such interface.
    ///
    /// The record is read once, here, through
    /// [`orbweaver_mcp::identity::PeerCapability::of_ior`]; the IOR is not
    /// kept and nothing dialable reaches a row.
    pub fn attach_peer(&mut self, label: impl Into<String>, ior: &Ior) {
        let row = PeerRow::of(label, ior);
        match self.interfaces.iter_mut().find(|i| i.id == row.type_id) {
            Some(iface) => iface.peers.push(row),
            None => self.unmatched_peers.push(row),
        }
    }

    /// Every peer record on the page, matched or not.
    pub fn peers(&self) -> impl Iterator<Item = &PeerRow> {
        self.interfaces.iter().flat_map(|i| &i.peers).chain(&self.unmatched_peers)
    }

    /// How many peers were handed to the page.
    pub fn peer_count(&self) -> usize {
        self.peers().count()
    }

    /// How many of them cannot enforce a caller identity — targets for which
    /// the bridge is the only enforcement point.
    pub fn bridge_only_count(&self) -> usize {
        self.peers().filter(|p| p.bridge_only()).count()
    }

    /// How many interfaces the exposure allowlists.
    pub fn exposed_count(&self) -> usize {
        self.interfaces.iter().filter(|i| i.exposed).count()
    }

    /// How many entries came off a foreign wire.
    pub fn ingested_count(&self) -> usize {
        self.interfaces.iter().filter(|i| i.ingested()).count()
    }

    /// How many entries have wire-described provenance anywhere in their
    /// inheritance, themselves included.
    pub fn touching_ingested_count(&self) -> usize {
        self.interfaces.iter().filter(|i| i.touches_ingested).count()
    }

    /// How many operations across the catalog need a human.
    pub fn destructive_count(&self) -> usize {
        self.interfaces.iter().flat_map(|i| &i.operations).filter(|o| o.destructive()).count()
    }

    /// Every operation the gate was asked about.
    pub fn operation_count(&self) -> usize {
        self.would_counts.iter().map(|(_, n)| n).sum()
    }

    /// How the exposure's declaration about unannotated operations reads.
    ///
    /// Two sentences rather than a word, because "refuse" alone has been read
    /// as "the gate refused something" instead of "the gate refuses these".
    pub fn unannotated_sentence(&self) -> String {
        match self.unannotated_effect.as_str() {
            "refuse" => "An operation whose contract states no ai_effect is refused: this \
                         exposure declares no assumption for the silences."
                .to_owned(),
            effect => format!(
                "An operation whose contract states no ai_effect is treated as {effect} — an \
                 operator declared that assumption for this exposure."
            ),
        }
    }

    /// How many operations name a scope the gate enforces.
    pub fn gated_count(&self) -> usize {
        self.interfaces
            .iter()
            .flat_map(|i| &i.operations)
            .filter(|o| matches!(o.requires, Requires::Scope(_)))
            .count()
    }
}

/// Builds the catalog by asking the real gate about every operation.
///
/// `chain` is the deployment's own chain, extensions included, exactly as
/// [`orbweaver_mcp::dryrun::predict`] intends: a preview run against a copy of
/// the policy would be a preview of a policy nobody deployed. A dry run runs
/// each stage's `before`, so a stage that counts attempts counts these — the
/// cost `Chain::dry_run` documents, paid here once per operation.
pub fn build(
    chain: &mut Chain,
    registry: &Registry,
    exposure: &Exposure,
    caller: Option<&Caller>,
    approval: Approval,
) -> Catalog {
    let mut interfaces = Vec::new();
    let mut totals = [0usize; Would::ALL.len()];
    let ids: Vec<String> = registry
        .ids()
        .filter(|id| registry.interface(id).is_some())
        .map(ToOwned::to_owned)
        .collect();

    for id in &ids {
        // The caller's own verdicts. The survey is also what decides *which*
        // operations exist to ask about — the contract's own, inherited
        // included, union whatever the exposure names — so a misconfigured
        // allowlist line surfaces here rather than being enumerated away.
        let mine = dryrun::survey(chain, registry, exposure, caller, approval, Some(id));
        let Some(entry) = first_interface(&mine) else { continue };

        let names: Vec<String> =
            array(entry, "operations").iter().filter_map(|row| string(row, "operation")).collect();
        let requirements: Vec<Requires> =
            names.iter().map(|name| requirement(chain, registry, id, name, approval)).collect();

        let mut operations = Vec::new();
        for (row, requires) in array(entry, "operations").iter().zip(requirements) {
            let Some(name) = string(row, "operation") else { continue };
            operations.push(OperationRow {
                would: string(row, "would").unwrap_or_default(),
                declared: flag(row, "declared"),
                stage: string(row, "stage"),
                why: string(row, "why"),
                effect: string(row, "effect"),
                approver: string(row, "approver"),
                requires,
                name,
            });
        }

        // The interface's own tally, as the survey counted it. Read rather
        // than recounted from the rows above: two counts of the same thing are
        // two things to keep in agreement.
        if let Some(summary) = entry.get("summary") {
            for (slot, would) in totals.iter_mut().zip(Would::ALL) {
                *slot += summary.get(would.name()).and_then(count).unwrap_or(0);
            }
        }

        interfaces.push(InterfaceRow {
            exposed: flag(entry, "exposed"),
            known: flag(entry, "known"),
            origin: registry.origin(id).unwrap_or(Origin::Idl),
            touches_ingested: registry.touches_ingested(id),
            ai_desc: registry.annotations(id).and_then(|a| a.get("ai_desc")).map(ToOwned::to_owned),
            operations,
            peers: Vec::new(),
            id: id.clone(),
        });
    }

    // One whole-estate pass, purely to collect the exposure lines that point at
    // nothing. The gate is the thing that knows.
    let whole = dryrun::survey(chain, registry, exposure, caller, approval, None);
    let unknown_exposures =
        array(&whole, "unknown_interfaces").iter().filter_map(Json::as_str).map(str::to_owned);

    Catalog {
        caller: whole.get("caller").and_then(Json::as_str).unwrap_or("<nobody>").to_owned(),
        scopes: array(&whole, "scopes")
            .iter()
            .filter_map(Json::as_str)
            .map(str::to_owned)
            .collect(),
        destructive_approved: approval.destructive_approved,
        // Stated by the survey whether or not anything is exposed, which is
        // what makes it readable on a page nobody has allowlisted anything on
        // yet — the page an operator opens first.
        unannotated_effect: whole
            .get("unannotated_effect")
            .and_then(Json::as_str)
            .unwrap_or("unstated")
            .to_owned(),
        would_counts: Would::ALL
            .iter()
            .zip(totals)
            .map(|(w, n)| (w.name().to_owned(), n))
            .collect(),
        unknown_exposures: unknown_exposures.collect(),
        unmatched_peers: Vec::new(),
        // The complement of the loop above, taken from the same `ids()`: what
        // the interface pass skipped is exactly what this collects, so an
        // entry cannot fall between them. No gate is asked about a constant —
        // a constant is not callable and there is nothing to predict — so this
        // is read straight off the registry rather than off a survey.
        declarations: declarations::collect(registry),
        interfaces,
    }
}

/// What the contract asks of anybody, read off the scopes stage's own outcome
/// in a run with no caller at all.
///
/// The no-caller run is the trick: `ScopeInterceptor` states the scope it
/// wanted in [`Denied::NotAuthenticated`] whether or not the configured caller
/// holds it, so the name on the page is always one the gate spoke.
fn requirement(
    chain: &mut Chain,
    registry: &Registry,
    target: &str,
    operation: &str,
    approval: Approval,
) -> Requires {
    // The console asks what the gate *would* do; it never sends arguments,
    // so there are none to screen and `None` is the true answer rather than a
    // placeholder.
    let ctx = CallContext { registry, caller: None, target, operation, approval, arguments: None };
    let prediction = dryrun::predict(chain, &ctx);
    let Some((_, outcome)) = prediction.chain().stages().find(|(name, _)| *name == STAGE_SCOPES)
    else {
        return Requires::Unenforced;
    };
    match outcome {
        StageOutcome::Proceeded => Requires::Nothing,
        StageOutcome::NotReached => Requires::NotReached,
        StageOutcome::Refused(
            Denied::NotAuthenticated { required, .. } | Denied::MissingScope { required, .. },
        ) => Requires::Scope(required.clone()),
        // The scopes stage refused for a reason that is not a scope. It ran and
        // it was not the scope that stopped it, so there is none to state.
        StageOutcome::Refused(_) => Requires::Nothing,
    }
}

fn first_interface(doc: &Json) -> Option<&Json> {
    array(doc, "interfaces").first()
}

fn array<'a>(value: &'a Json, key: &str) -> &'a [Json] {
    match value.get(key) {
        Some(Json::Array(items)) => items,
        _ => &[],
    }
}

fn string(value: &Json, key: &str) -> Option<String> {
    value.get(key).and_then(Json::as_str).map(ToOwned::to_owned)
}

fn flag(value: &Json, key: &str) -> bool {
    matches!(value.get(key), Some(Json::Bool(true)))
}

/// A count the survey wrote. A number that will not read back is left out
/// rather than guessed at zero: a zero is a measurement and this would not be
/// one.
fn count(value: &Json) -> Option<usize> {
    match value {
        Json::Number(text) => text.parse().ok(),
        _ => None,
    }
}

/// Renders the catalog as one self-contained HTML file.
pub fn render_html(catalog: &Catalog) -> String {
    let mut body = Markup::empty();
    body.push(Markup::labelled("h1", "", "Catalog"));
    body.push(Markup::labelled(
        "p",
        "sub",
        "What exists, what an agent can reach, and which of it a peer described to us.",
    ));
    body.push(header_card(catalog));

    if !catalog.unknown_exposures.is_empty() {
        let mut inner = Markup::labelled(
            "p",
            "",
            "These ids are allowlisted and are not in the catalog, so the lines allowlist \
             nothing:",
        );
        for id in &catalog.unknown_exposures {
            inner.push(Markup::labelled("p", "id", id));
        }
        body.push(Markup::element("div", "card", inner));
    }

    body.push(Markup::labelled("h2", "", "Interfaces"));
    for iface in &catalog.interfaces {
        body.push(interface_card(iface));
    }
    if catalog.interfaces.is_empty() {
        // Not "the catalog is empty", which is what stood here and which
        // `corpus/golden/33-const-values.idl` made false: that file declares
        // 22 constants and a union and no interface at all, so the page said
        // there was nothing here over 23 things there were.
        body.push(Markup::labelled(
            "p",
            "absent",
            "no interface is declared — nothing on these contracts is callable",
        ));
    }

    body.push(Markup::labelled("h2", "", "Declarations"));
    body.push(Markup::labelled(
        "p",
        "note",
        "What the contracts declare besides interfaces. No gate is asked about any of it: a \
         constant is not callable, so there is no prediction to render and nothing here is an \
         exposure decision. A constant's value is the registry's folded value, spelled the way \
         a §5.3 release note spells it.",
    ));
    body.push(declarations::block(&catalog.declarations));

    if !catalog.unmatched_peers.is_empty() {
        body.push(Markup::labelled("h2", "", "Peers whose type is not in the catalog"));
        let mut inner = Markup::labelled(
            "p",
            "note",
            "These references were supplied and name a type no loaded contract declares. What \
             each target can enforce is a fact about its IOR and is rendered anyway.",
        );
        for peer in &catalog.unmatched_peers {
            inner.push(peer_block(peer, true));
        }
        body.push(Markup::element("div", "card", inner));
    }
    body.push(provenance_footer());
    page("Catalog — orbweaver-console", body)
}

fn header_card(catalog: &Catalog) -> Markup {
    let mut stats = Markup::empty();
    stats.push(stat("", catalog.interfaces.len(), "interfaces"));
    stats.push(stat("", catalog.declarations.len(), "other declarations"));
    stats.push(stat("stop", catalog.exposed_count(), "exposed"));
    stats.push(stat("warn", catalog.ingested_count(), "ingested"));
    stats.push(stat("warn", catalog.touching_ingested_count(), "touching ingested"));
    stats.push(stat("stop", catalog.destructive_count(), "operations need a human"));
    stats.push(stat("", catalog.gated_count(), "operations gated by a scope"));
    stats.push(stat("", catalog.peer_count(), "peer references supplied"));
    // The common case §4.8 predicts, and still worth a colour: every one of
    // these is a target behind which no second check exists.
    let bridge_only = catalog.bridge_only_count();
    stats.push(stat(
        if bridge_only > 0 { "warn" } else { "" },
        bridge_only,
        "targets where the bridge is the only enforcement point",
    ));

    let scopes =
        if catalog.scopes.is_empty() { "no scopes".to_owned() } else { catalog.scopes.join(", ") };
    let approval = if catalog.destructive_approved {
        "a host approval was in hand"
    } else {
        "no host approval"
    };
    let mut inner = Markup::labelled(
        "p",
        "",
        &format!("Predictions run for {} — {scopes}, {approval}.", catalog.caller),
    );
    inner.push(Markup::element("div", "summary", stats));
    inner.push(Markup::labelled("p", "note", &catalog.unannotated_sentence()));

    // The counts above are properties of the contracts; these are the gate's
    // answers. On a contract set that annotates nothing the first row is all
    // zeroes and this one is not, which is the whole reason it is here.
    let mut answers = Markup::empty();
    for (would, n) in &catalog.would_counts {
        let kind = match would.as_str() {
            // Neither of these is a problem to flag: one is the point of an
            // exposure and the other is the posture everything starts in.
            "allow" | "not_exposed" => "",
            _ if *n > 0 => "warn",
            _ => "",
        };
        answers.push(stat(kind, *n, would));
    }
    inner.push(Markup::labelled(
        "p",
        "",
        &format!(
            "What the gate would answer for each of the {} operations it was asked about:",
            catalog.operation_count()
        ),
    ));
    inner.push(Markup::element("div", "summary", answers));
    Markup::element("div", "card", inner)
}

fn stat(kind: &'static str, n: usize, label: &str) -> Markup {
    let class = match kind {
        "stop" => "stat stop",
        "warn" => "stat warn",
        _ => "stat",
    };
    let mut inner = Markup::labelled("b", "", &n.to_string());
    inner.push(Markup::text(&format!(" {label}")));
    Markup::element("div", class, inner)
}

fn interface_card(iface: &InterfaceRow) -> Markup {
    let mut inner = Markup::labelled("div", "id", &iface.id);

    let mut badges = Markup::empty();
    if iface.exposed {
        badges.push(Markup::labelled("span", "badge b-exposed", "exposed"));
    } else {
        badges.push(Markup::labelled("span", "badge b-dark", "not exposed"));
    }
    match iface.source() {
        Some(source) => badges.push(Markup::labelled(
            "span",
            "badge b-ingested",
            &format!("ingested from {source}"),
        )),
        None => badges.push(Markup::labelled("span", "badge b-idl", "from IDL")),
    }
    if iface.touches_ingested && !iface.ingested() {
        badges.push(Markup::labelled("span", "badge b-derived", "inherits from ingested"));
    }
    if !iface.known {
        badges.push(Markup::labelled("span", "badge b-unknown", "not in the catalog"));
    }
    inner.push(Markup::element("div", "badges", badges));

    if iface.touches_ingested {
        inner.push(Markup::labelled(
            "p",
            "note",
            "A peer described this. It passed no S4 gate and carries no SIDL, so ai_effect and \
             ai_authz have nothing to key on here.",
        ));
    }
    match &iface.ai_desc {
        Some(desc) => inner.push(Markup::labelled("p", "desc", desc)),
        // Drawn, not skipped. A card with no prose on it looks like a card
        // whose prose was short; a legacy estate has none anywhere, and an
        // operator deciding what an agent may reach is entitled to know that
        // the contract told them nothing rather than to infer it from a gap.
        None => inner.push(Markup::labelled(
            "p",
            "absent",
            "no ai_desc — the contract says nothing about what this interface is for",
        )),
    }

    inner.push(peers_block(&iface.peers));
    inner.push(operations_table(&iface.operations));

    let class = match (iface.exposed, iface.touches_ingested) {
        (true, true) => "iface exposed ingested",
        (true, false) => "iface exposed",
        (false, true) => "iface ingested",
        (false, false) => "iface",
    };
    Markup::element("div", class, inner)
}

/// The peers attached to one interface, or the sentence that none were.
fn peers_block(peers: &[PeerRow]) -> Markup {
    if peers.is_empty() {
        // Not "bridge only": that is a measurement of a reference, and none
        // was supplied. Unmeasured is its own word here, as everywhere else on
        // the page.
        return Markup::labelled(
            "p",
            "absent",
            "no peer reference supplied — what this interface's targets can enforce is \
             unmeasured here",
        );
    }
    let mut block = Markup::empty();
    for peer in peers {
        block.push(peer_block(peer, false));
    }
    Markup::element("div", "peers", block)
}

/// One peer's record: its label, and the bridge's two sentences about it.
///
/// `with_type` names the IOR's own type id, for a peer drawn away from the
/// interface card that would otherwise name it.
fn peer_block(peer: &PeerRow, with_type: bool) -> Markup {
    let mut inner = Markup::labelled("span", "mono", &peer.label);
    if with_type {
        inner.push(Markup::text(" — "));
        inner.push(Markup::labelled("span", "id", &peer.type_id));
    }
    let class = if peer.bridge_only() { "badge b-unknown" } else { "badge b-ok" };
    inner.push(Markup::labelled(
        "span",
        class,
        &format!("enforced by: {}", peer.enforcement_point),
    ));
    if peer.transport_secured {
        inner.push(Markup::labelled("span", "badge b-scope", "tls advertised"));
    } else {
        inner.push(Markup::labelled("span", "badge b-dark", "cleartext"));
    }
    inner.push(Markup::labelled("div", "note", &format!("identity: {}", peer.identity)));
    inner.push(Markup::labelled("div", "note", &format!("transport: {}", peer.transport)));
    Markup::element("div", "peer", inner)
}

fn operations_table(operations: &[OperationRow]) -> Markup {
    if operations.is_empty() {
        return Markup::labelled("p", "absent", "no operations");
    }
    let mut head = Markup::empty();
    for column in ["operation", "would", "requires", "effect", "stage", "why"] {
        head.push(Markup::labelled("th", "", column));
    }
    let mut rows = Markup::element("tr", "", head);

    for op in operations {
        let mut cells = Markup::empty();

        let mut name = Markup::labelled("span", "mono", &op.name);
        if !op.declared {
            name.push(Markup::labelled("span", "badge b-unknown", "not declared"));
        }
        cells.push(Markup::element("td", "", name));

        cells.push(Markup::element("td", "", would_badge(&op.would)));

        cells.push(Markup::element("td", "", requires_cell(&op.requires)));

        // An em dash where an effect belongs is the page inviting a reader to
        // supply the meaning, and on a contract that annotates nothing every
        // cell in this column is that dash. The words are the same words the
        // text mode has always used, and the same absence D004's trace fields
        // are rendered with: absent is a rendering, never a default.
        let mut effect = match &op.effect {
            Some(effect) => Markup::labelled("span", "badge b-destructive", effect),
            None => Markup::labelled("span", "absent", "none stated"),
        };
        if let Some(approver) = &op.approver {
            effect.push(Markup::labelled("div", "note", &format!("approver: {approver}")));
        }
        cells.push(Markup::element("td", "", effect));

        cells.push(Markup::element("td", "", optional(op.stage.as_deref(), "no stage refused")));
        cells.push(Markup::element("td", "", optional(op.why.as_deref(), "nothing refused it")));

        rows.push(Markup::element("tr", "", cells));
    }
    Markup::element("div", "scroll", Markup::element("table", "", rows))
}

fn would_badge(would: &str) -> Markup {
    let class = match would {
        "allow" => "badge b-ok",
        "not_exposed" => "badge b-dark",
        _ => "badge b-destructive",
    };
    Markup::labelled("span", class, would)
}

fn requires_cell(requires: &Requires) -> Markup {
    match requires {
        Requires::Scope(scope) => Markup::labelled("span", "badge b-scope", scope),
        Requires::Nothing => Markup::labelled("span", "absent", "none stated"),
        Requires::NotReached => Markup::labelled("span", "absent", "not reached"),
        Requires::Unenforced => Markup::labelled("span", "badge b-unknown", "unenforced"),
    }
}

/// A cell whose value may be absent, with the absence in words.
///
/// `absent` is a sentence and never a dash: a dash is a shape a reader gives
/// their own meaning to, and every reader gives it a different one.
fn optional(value: Option<&str>, absent: &str) -> Markup {
    match value {
        Some(v) => Markup::text(v),
        None => Markup::labelled("span", "absent", absent),
    }
}

/// Renders the catalog for a terminal.
pub fn render_text(catalog: &Catalog) -> String {
    let mut out = String::new();
    out.push_str("CATALOG\n");
    out.push_str(&format!(
        "caller={} scopes=[{}] host-approval={}\n",
        catalog.caller,
        catalog.scopes.join(","),
        catalog.destructive_approved
    ));
    out.push_str(&format!(
        "{} interfaces, {} other declarations, {} exposed, {} ingested, {} touching ingested, \
         {} operations need a human, {} gated by a scope\n",
        catalog.interfaces.len(),
        catalog.declarations.len(),
        catalog.exposed_count(),
        catalog.ingested_count(),
        catalog.touching_ingested_count(),
        catalog.destructive_count(),
        catalog.gated_count(),
    ));
    out.push_str(&format!("unannotated-effect={}\n", catalog.unannotated_effect));
    let answers: Vec<String> =
        catalog.would_counts.iter().map(|(would, n)| format!("{would}={n}")).collect();
    out.push_str(&format!(
        "gate over {} operations: {}\n",
        catalog.operation_count(),
        answers.join(" ")
    ));
    for id in &catalog.unknown_exposures {
        out.push_str(&format!("! allowlisted and not in the catalog: {id}\n"));
    }
    out.push_str(&format!(
        "{} peer reference(s) supplied, {} where the bridge is the only enforcement point\n",
        catalog.peer_count(),
        catalog.bridge_only_count()
    ));
    for iface in &catalog.interfaces {
        let exposure = if iface.exposed { "EXPOSED" } else { "not exposed" };
        let origin = match iface.source() {
            Some(source) => format!("INGESTED from {source}"),
            None if iface.touches_ingested => "from IDL, INHERITS FROM INGESTED".to_owned(),
            None => "from IDL".to_owned(),
        };
        out.push_str(&format!("\n{} [{exposure}] [{origin}]\n", iface.id));
        match &iface.ai_desc {
            Some(desc) => out.push_str(&format!("  desc: {desc}\n")),
            None => out.push_str("  desc: absent\n"),
        }
        if iface.peers.is_empty() {
            out.push_str(
                "  peer: none supplied — what its targets can enforce is unmeasured here\n",
            );
        }
        for peer in &iface.peers {
            out.push_str(&peer_text(peer, false));
        }
        if iface.operations.is_empty() {
            out.push_str("  (no operations)\n");
        }
        for op in &iface.operations {
            out.push_str(&format!("  {:<24} would={}", op.name, op.would));
            out.push_str(&format!(" requires={}", op.requires.label()));
            match &op.effect {
                Some(effect) => out.push_str(&format!(" effect={effect}")),
                None => out.push_str(" effect=absent"),
            }
            if !op.declared {
                out.push_str(" NOT-DECLARED");
            }
            if let Some(stage) = &op.stage {
                out.push_str(&format!(" stage={stage}"));
            }
            if let Some(why) = &op.why {
                out.push_str(&format!("\n      why: {why}"));
            }
            out.push('\n');
        }
    }
    out.push_str(&declarations::render_text(&catalog.declarations));
    if !catalog.unmatched_peers.is_empty() {
        out.push_str("\nPEERS WHOSE TYPE IS NOT IN THE CATALOG\n");
        for peer in &catalog.unmatched_peers {
            out.push_str(&peer_text(peer, true));
        }
    }
    out
}

/// One peer's record for a terminal: one line an operator can grep for
/// `enforced-by=`, then the two sentences.
fn peer_text(peer: &PeerRow, with_type: bool) -> String {
    let mut line = format!("  peer: {} enforced-by={}", peer.label, peer.enforcement_point);
    if with_type {
        line.push_str(&format!(" type={}", peer.type_id));
    }
    line.push_str(&format!(
        " transport={}\n      identity: {}\n      transport: {}\n",
        if peer.transport_secured { "tls-advertised" } else { "cleartext" },
        peer.identity,
        peer.transport
    ));
    line
}

#[cfg(test)]
mod tests {
    use orbweaver_registry::{Entry, InterfaceEntry};

    use super::*;

    const IDL: &str = "module bank {
        //@ ai_desc: A customer deposit account
        interface Account {
          //@ ai_effect: read_only
          long balance();
          //@ ai_authz: accounts:write
          //@ ai_effect: idempotent
          void deposit(in long cents);
          //@ ai_effect: destructive
          //@ ai_approver: the duty risk officer
          void close();
        };
        interface Ledger { //@ ai_effect: read_only
          long total(); };
      };";

    const ACCOUNT: &str = "IDL:bank/Account:1.0";
    const LEDGER: &str = "IDL:bank/Ledger:1.0";

    fn registry(src: &str) -> Registry {
        let spec = orbweaver_idl::parse(src).expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        r
    }

    fn catalog(registry: &Registry, exposure: Exposure, caller: Option<&Caller>) -> Catalog {
        let mut chain = Chain::standard(exposure.clone());
        build(&mut chain, registry, &exposure, caller, Approval::default())
    }

    fn interface<'a>(catalog: &'a Catalog, id: &str) -> &'a InterfaceRow {
        catalog.interfaces.iter().find(|i| i.id == id).expect("the interface")
    }

    fn operation<'a>(iface: &'a InterfaceRow, name: &str) -> &'a OperationRow {
        iface.operations.iter().find(|o| o.name == name).expect("the operation")
    }

    #[test]
    fn exposure_on_the_page_is_the_exposures_own_answer() {
        let r = registry(IDL);
        let e = Exposure::nothing().allow_interface(ACCOUNT);
        let c = catalog(&r, e.clone(), None);
        for iface in &c.interfaces {
            assert_eq!(iface.exposed, e.exposes(&iface.id), "{}", iface.id);
        }
        assert_eq!(c.exposed_count(), 1);
        assert!(!interface(&c, LEDGER).exposed);
    }

    #[test]
    fn an_ingested_interface_is_marked_and_names_its_source() {
        let mut r = registry(IDL);
        r.define_ingested(
            "IDL:legacy/Tracker:1.0".to_owned(),
            Entry::Interface(InterfaceEntry::default()),
            "jacorb-ir",
        )
        .expect("registers");
        let c = catalog(&r, Exposure::nothing(), None);
        let row = interface(&c, "IDL:legacy/Tracker:1.0");
        assert!(row.ingested());
        assert_eq!(row.source(), Some("jacorb-ir"));
        assert!(row.touches_ingested);
        assert_eq!(c.ingested_count(), 1);

        let html = render_html(&c);
        assert!(html.contains("ingested from jacorb-ir"), "{html}");
    }

    /// `touches_ingested` is the question an exposure gate asks, so it is the
    /// question the page answers: a locally declared interface whose base came
    /// off the wire has remote-chosen operations in its callable surface and no
    /// mark of its own would say so.
    #[test]
    fn provenance_is_contagious_upwards_on_the_page_too() {
        let mut r = Registry::new();
        r.define_ingested(
            "IDL:remote/Base:1.0".to_owned(),
            Entry::Interface(InterfaceEntry::default()),
            "peer",
        )
        .expect("registers");
        let derived = InterfaceEntry {
            bases: vec!["IDL:remote/Base:1.0".to_owned()],
            ..InterfaceEntry::default()
        };
        r.define_ingested("IDL:local/Derived:1.0".to_owned(), Entry::Interface(derived), "peer")
            .expect("registers");
        // The interesting case is a *local* entry over an ingested base, which
        // `define_ingested` cannot produce; assert the registry's own answer is
        // what the row carries either way.
        let c = catalog(&r, Exposure::nothing(), None);
        for iface in &c.interfaces {
            assert_eq!(iface.touches_ingested, r.touches_ingested(&iface.id), "{}", iface.id);
        }
        assert_eq!(c.touching_ingested_count(), 2);
    }

    #[test]
    fn a_destructive_operation_carries_its_effect_and_its_approver() {
        let r = registry(IDL);
        let c = catalog(&r, Exposure::nothing().allow_interface(ACCOUNT), None);
        let close = operation(interface(&c, ACCOUNT), "close");
        assert_eq!(close.effect.as_deref(), Some("destructive"));
        assert_eq!(close.approver.as_deref(), Some("the duty risk officer"));
        assert!(close.destructive());
        assert_eq!(c.destructive_count(), 1);

        let balance = operation(interface(&c, ACCOUNT), "balance");
        assert_eq!(balance.effect, None, "read_only does not need a human");
    }

    /// The scope on the page is a scope the gate spoke, and it is spoken even
    /// when the configured caller holds it — that is what the second pass is
    /// for.
    #[test]
    fn a_scoped_operation_names_its_scope_even_for_a_caller_who_holds_it() {
        let r = registry(IDL);
        let holder = Caller::new("alice").with_scope("accounts:write");
        let c = catalog(&r, Exposure::nothing().allow_interface(ACCOUNT), Some(&holder));
        let deposit = operation(interface(&c, ACCOUNT), "deposit");
        assert_eq!(deposit.would, "allow", "alice holds it");
        assert_eq!(deposit.requires, Requires::Scope("accounts:write".to_owned()));
        assert_eq!(c.gated_count(), 1);
    }

    #[test]
    fn an_unscoped_operation_states_nothing_rather_than_guessing() {
        let r = registry(IDL);
        let c = catalog(&r, Exposure::nothing().allow_interface(ACCOUNT), None);
        assert_eq!(operation(interface(&c, ACCOUNT), "balance").requires, Requires::Nothing);
    }

    /// A stage that did not run is not a stage that approved.
    #[test]
    fn a_scope_behind_a_closed_exposure_is_not_reached_rather_than_none() {
        let r = registry(IDL);
        let c = catalog(&r, Exposure::nothing(), None);
        let deposit = operation(interface(&c, ACCOUNT), "deposit");
        assert_eq!(deposit.would, "not_exposed");
        assert_eq!(deposit.requires, Requires::NotReached);
        assert_ne!(deposit.requires, Requires::Nothing);
        assert_eq!(c.gated_count(), 0, "an unreached requirement is not a stated one");
    }

    #[test]
    fn an_allowlist_line_that_names_nothing_is_reported() {
        let r = registry(IDL);
        let c = catalog(&r, Exposure::nothing().allow_interface("IDL:typo/Ledgr:1.0"), None);
        assert_eq!(c.unknown_exposures, vec!["IDL:typo/Ledgr:1.0".to_owned()]);
        let html = render_html(&c);
        assert!(html.contains("allowlisted and are not in the catalog"), "{html}");
    }

    #[test]
    fn the_text_mode_carries_the_same_facts() {
        let r = registry(IDL);
        let c = catalog(&r, Exposure::nothing().allow_interface(ACCOUNT), None);
        let text = render_text(&c);
        assert!(text.contains("IDL:bank/Account:1.0 [EXPOSED]"), "{text}");
        assert!(text.contains("IDL:bank/Ledger:1.0 [not exposed]"), "{text}");
        assert!(text.contains("effect=destructive"), "{text}");
        assert!(text.contains("requires=accounts:write"), "{text}");
        assert!(text.contains("effect=absent"), "{text}");
    }

    /// The tally is the survey's, and it covers exactly the rows on the page —
    /// a summary that counted a different set than the table under it would be
    /// two answers to one question.
    #[test]
    fn the_gates_tally_covers_every_row_on_the_page() {
        let r = registry(IDL);
        let c = catalog(&r, Exposure::nothing().allow_interface(ACCOUNT), None);
        let rows: usize = c.interfaces.iter().map(|i| i.operations.len()).sum();
        assert_eq!(c.operation_count(), rows);
        let counts: std::collections::BTreeMap<&str, usize> =
            c.would_counts.iter().map(|(w, n)| (w.as_str(), *n)).collect();
        // Two of Account's three are allowed for nobody: `deposit` wants a
        // scope and `close` wants a human. Ledger is not exposed at all.
        assert_eq!(counts["allow"], 1, "{counts:?}");
        assert_eq!(counts["need_authentication"], 1, "{counts:?}");
        assert_eq!(counts["need_approval"], 1, "{counts:?}");
        assert_eq!(counts["not_exposed"], 1, "{counts:?}");
        assert_eq!(c.unannotated_effect, "refuse", "the default posture is stated, not assumed");
    }

    /// No timing anywhere: D004 fixes no duration field and says why, and a
    /// console that showed one would be showing a number nobody measured.
    #[test]
    fn nothing_on_the_page_is_a_duration() {
        let r = registry(IDL);
        let c = catalog(&r, Exposure::nothing().allow_interface(ACCOUNT), None);
        let html = render_html(&c);
        for invented in ["duration", "elapsed", "latency", "ms<", "µs"] {
            assert!(!html.contains(invented), "invented a measurement: {invented}");
        }
    }

    /// An IOR for `type_id` at a fictional endpoint, carrying `components`.
    fn ior_for(type_id: &str, components: Vec<orbweaver_giop::TaggedComponent>) -> Ior {
        Ior {
            type_id: type_id.to_owned(),
            profiles: vec![orbweaver_giop::IiopProfile {
                version: orbweaver_giop::Version::V1_2,
                host: "target.example.internal".into(),
                port: 2809,
                object_key: b"very-distinctive-object-key".to_vec(),
                components,
            }],
        }
    }

    /// A `TAG_CSI_SEC_MECH_LIST` whose one mechanism accepts an asserted
    /// principal name — the advertisement neither project fixture makes.
    fn identity_asserting_mechanism_list(
        endian: orbweaver_cdr::Endian,
    ) -> orbweaver_giop::TaggedComponent {
        use orbweaver_cdr::Encoder;
        use orbweaver_giop::csiv2::{TAG_CSI_SEC_MECH_LIST, TAG_NULL_TAG, options};
        let mut e = Encoder::encapsulation(endian);
        e.put_bool(false); // stateful
        e.put_u32(1); // one mechanism
        e.put_u16(0); // target_requires
        e.put_u32(TAG_NULL_TAG); // no transport mechanism
        e.put_octet_seq(&[]);
        e.put_u16(0); // AS_ContextSec: none offered
        e.put_u16(0);
        e.put_octet_seq(&[]);
        e.put_octet_seq(&[]);
        e.put_u16(options::IDENTITY_ASSERTION); // SAS_ContextSec supports
        e.put_u16(0);
        e.put_u32(0); // privilege authorities
        e.put_u32(0); // naming mechanisms
        e.put_u32(2); // ITTPrincipalName
        orbweaver_giop::TaggedComponent { tag: TAG_CSI_SEC_MECH_LIST, data: e.finish().unwrap() }
    }

    /// PLAN §4.8: the catalogue says, per peer, whether the target can enforce
    /// a caller identity — and the words are the bridge's, not this crate's.
    #[test]
    fn a_peer_reference_puts_the_targets_capability_record_beside_its_interface() {
        let r = registry(IDL);
        let mut c = catalog(&r, Exposure::nothing().allow_interface(ACCOUNT), None);
        // Nothing attached yet: unmeasured is said in words, not left blank.
        assert!(interface(&c, ACCOUNT).peers.is_empty());
        let before = render_html(&c);
        assert!(before.contains("no peer reference supplied"), "{before}");
        assert!(before.contains("unmeasured here"), "{before}");
        assert!(!before.contains("identity: not enforced"), "not measured, not said");
        assert_eq!(c.peer_count(), 0);

        // The legacy baseline: an IOR advertising nothing.
        c.attach_peer("spikes/echo.ior", &ior_for(ACCOUNT, Vec::new()));
        // And, in both byte orders, one that advertises identity assertion.
        for (label, endian) in [
            ("fabricated-be.ior", orbweaver_cdr::Endian::Big),
            ("fabricated-le.ior", orbweaver_cdr::Endian::Little),
        ] {
            c.attach_peer(label, &ior_for(LEDGER, vec![identity_asserting_mechanism_list(endian)]));
        }

        let account = interface(&c, ACCOUNT);
        assert_eq!(account.peers.len(), 1);
        let bare = &account.peers[0];
        assert_eq!(bare.label, "spikes/echo.ior");
        assert!(bare.bridge_only());
        assert!(!bare.enforces_identity && !bare.transport_secured);
        assert_eq!(bare.enforcement_point, "bridge only");
        // The record's own words, carried rather than rephrased.
        let record = PeerCapability::of_ior(&ior_for(ACCOUNT, Vec::new()));
        assert_eq!(bare.identity, record.identity_sentence());
        assert_eq!(bare.transport, record.transport_sentence());

        let ledger = interface(&c, LEDGER);
        assert_eq!(ledger.peers.len(), 2, "one per byte order");
        for peer in &ledger.peers {
            assert!(peer.enforces_identity, "{}", peer.label);
            assert!(!peer.bridge_only());
            assert_eq!(peer.enforcement_point, "target");
            assert!(peer.identity.starts_with("enforced by the target"), "{}", peer.identity);
        }
        assert_eq!(c.peer_count(), 3);
        assert_eq!(c.bridge_only_count(), 1);
        assert!(c.unmatched_peers.is_empty());

        // The page says it in words, per peer, and counts it up top.
        let html = render_html(&c);
        assert!(html.contains("identity: not enforced by the target — the bridge is the only enforcement point (no CSIv2 mechanism list in the IOR)"), "{html}");
        assert!(html.contains("transport: cleartext — no TAG_SSL_SEC_TRANS in the IOR"), "{html}");
        assert!(html.contains("enforced by: bridge only"), "{html}");
        assert!(html.contains("enforced by: target"), "{html}");
        assert!(
            html.contains("<b>1</b> targets where the bridge is the only enforcement point"),
            "{html}"
        );
        assert!(html.contains("<b>3</b> peer references supplied"), "{html}");
        // And nothing dialable reached it.
        for needle in ["target.example.internal", "2809", "very-distinctive"] {
            assert!(!html.contains(needle), "{needle} on the page");
        }

        let text = render_text(&c);
        assert!(
            text.contains(
                "3 peer reference(s) supplied, 1 where the bridge is the only enforcement point"
            ),
            "{text}"
        );
        assert!(
            text.contains("peer: spikes/echo.ior enforced-by=bridge only transport=cleartext"),
            "{text}"
        );
        assert!(
            text.contains("peer: fabricated-be.ior enforced-by=target transport=cleartext"),
            "{text}"
        );
        assert!(text.contains("identity: not enforced by the target — the bridge is the only enforcement point (no CSIv2 mechanism list in the IOR)"), "{text}");
        for needle in ["target.example.internal", "2809", "very-distinctive"] {
            assert!(!text.contains(needle), "{needle} in the text");
        }
    }

    /// A reference whose type no loaded contract declares is rendered under
    /// its own heading, record and all: the record is a fact about the IOR.
    #[test]
    fn a_peer_whose_type_is_not_in_the_catalog_is_listed_not_dropped() {
        let r = registry(IDL);
        let mut c = catalog(&r, Exposure::nothing(), None);
        c.attach_peer("spikes/jacorb.ior", &ior_for("IDL:spike/Echo:1.0", Vec::new()));
        assert_eq!(c.unmatched_peers.len(), 1);
        assert_eq!(c.unmatched_peers[0].type_id, "IDL:spike/Echo:1.0");
        assert_eq!(c.peer_count(), 1);
        assert_eq!(c.bridge_only_count(), 1);
        for iface in &c.interfaces {
            assert!(iface.peers.is_empty(), "{}", iface.id);
        }
        let html = render_html(&c);
        assert!(html.contains("Peers whose type is not in the catalog"), "{html}");
        assert!(html.contains("spikes/jacorb.ior"), "{html}");
        assert!(html.contains("enforced by: bridge only"), "{html}");
        let text = render_text(&c);
        assert!(text.contains("PEERS WHOSE TYPE IS NOT IN THE CATALOG"), "{text}");
        assert!(
            text.contains(
                "peer: spikes/jacorb.ior enforced-by=bridge only type=IDL:spike/Echo:1.0"
            ),
            "{text}"
        );
    }
}
