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
//! [`Chain`]: orbweaver_mcp::interceptor::Chain
//! [`StageOutcome::NotReached`]: orbweaver_mcp::interceptor::StageOutcome

use orbweaver_dynamic::json::Json;
use orbweaver_mcp::dryrun;
use orbweaver_mcp::identity::Caller;
use orbweaver_mcp::interceptor::{CallContext, Chain, STAGE_SCOPES, StageOutcome};
use orbweaver_mcp::policy::{Approval, Denied, Exposure};
use orbweaver_registry::{Origin, Registry};

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
    /// Every interface in the registry, sorted by repository id.
    pub interfaces: Vec<InterfaceRow>,
    /// Ids the exposure allowlists that the catalog does not have — a typo, a
    /// stale id, or an id mis-parsed on the way in. An exposure line that
    /// allowlists nothing is a misconfiguration an operator wants to see before
    /// a deployment rather than after one.
    pub unknown_exposures: Vec<String>,
}

impl Catalog {
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

        interfaces.push(InterfaceRow {
            exposed: flag(entry, "exposed"),
            known: flag(entry, "known"),
            origin: registry.origin(id).unwrap_or(Origin::Idl),
            touches_ingested: registry.touches_ingested(id),
            ai_desc: registry.annotations(id).and_then(|a| a.get("ai_desc")).map(ToOwned::to_owned),
            operations,
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
        unknown_exposures: unknown_exposures.collect(),
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
    let ctx = CallContext { registry, caller: None, target, operation, approval };
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
        body.push(Markup::labelled("p", "absent", "the catalog is empty"));
    }
    body.push(provenance_footer());
    page("Catalog — orbweaver-console", body)
}

fn header_card(catalog: &Catalog) -> Markup {
    let mut stats = Markup::empty();
    stats.push(stat("", catalog.interfaces.len(), "interfaces"));
    stats.push(stat("stop", catalog.exposed_count(), "exposed"));
    stats.push(stat("warn", catalog.ingested_count(), "ingested"));
    stats.push(stat("warn", catalog.touching_ingested_count(), "touching ingested"));
    stats.push(stat("stop", catalog.destructive_count(), "operations need a human"));
    stats.push(stat("", catalog.gated_count(), "operations gated by a scope"));

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
    if let Some(desc) = &iface.ai_desc {
        inner.push(Markup::labelled("p", "desc", desc));
    }

    inner.push(operations_table(&iface.operations));

    let class = match (iface.exposed, iface.touches_ingested) {
        (true, true) => "iface exposed ingested",
        (true, false) => "iface exposed",
        (false, true) => "iface ingested",
        (false, false) => "iface",
    };
    Markup::element("div", class, inner)
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

        let mut effect = match &op.effect {
            Some(effect) => Markup::labelled("span", "badge b-destructive", effect),
            None => Markup::labelled("span", "absent", "—"),
        };
        if let Some(approver) = &op.approver {
            effect.push(Markup::labelled("div", "note", &format!("approver: {approver}")));
        }
        cells.push(Markup::element("td", "", effect));

        cells.push(Markup::element("td", "", optional(op.stage.as_deref(), "—")));
        cells.push(Markup::element("td", "", optional(op.why.as_deref(), "—")));

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

fn optional(value: Option<&str>, dash: &str) -> Markup {
    match value {
        Some(v) => Markup::text(v),
        None => Markup::labelled("span", "absent", dash),
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
        "{} interfaces, {} exposed, {} ingested, {} touching ingested, {} operations need a \
         human, {} gated by a scope\n",
        catalog.interfaces.len(),
        catalog.exposed_count(),
        catalog.ingested_count(),
        catalog.touching_ingested_count(),
        catalog.destructive_count(),
        catalog.gated_count(),
    ));
    for id in &catalog.unknown_exposures {
        out.push_str(&format!("! allowlisted and not in the catalog: {id}\n"));
    }
    for iface in &catalog.interfaces {
        let exposure = if iface.exposed { "EXPOSED" } else { "not exposed" };
        let origin = match iface.source() {
            Some(source) => format!("INGESTED from {source}"),
            None if iface.touches_ingested => "from IDL, INHERITS FROM INGESTED".to_owned(),
            None => "from IDL".to_owned(),
        };
        out.push_str(&format!("\n{} [{exposure}] [{origin}]\n", iface.id));
        if let Some(desc) = &iface.ai_desc {
            out.push_str(&format!("  desc: {desc}\n"));
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
    out
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
          void deposit(in long cents);
          //@ ai_effect: destructive
          //@ ai_approver: the duty risk officer
          void close();
        };
        interface Ledger { long total(); };
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
}
