//! The diff view: what changed between two revisions of a contract, and
//! whether deployed peers survive it.
//!
//! # There is one differ
//!
//! `docs/PLAN.md` §5.3's rules live in [`orbweaver_registry::diff`] and are
//! enforced by `idl-diff`. This module calls that function and renders its
//! answers. A second implementation of the table would be a second answer to
//! "is this breaking?", and the whole point of the rule is that the answer is
//! not negotiable — CDR encodes by position, so a reordered struct member
//! reaches a deployed peer as the next member's value with nothing raised.
//!
//! # This view is not the gate
//!
//! `idl-diff` exits non-zero and refuses; that is the release gate, and it
//! takes `--approve <reason> --approver <name>` so the decision travels with
//! the diff — as a row in an approval store beside the proposed contract
//! ([`orbweaver_registry::approval`]). The console renders the same verdicts
//! for someone deciding *whether* to ask for that approval, and when a store
//! is there it renders what the store says: who approved which finding, why,
//! when, and whether that approval still applies to these bytes. It exits zero
//! on a breaking change, deliberately: a viewer that also refused would be a
//! second gate a release could be routed around, and it writes nothing to the
//! store for the same reason — the page shows a decision, it does not take one.

use std::path::Path;

use orbweaver_idl::include::SearchPath;
use orbweaver_registry::approval::{self, Approval, Store};
use orbweaver_registry::diff::{Change, Verdict, diff};
use orbweaver_registry::{Contract, Registry, Strictness};

use crate::html::{Markup, page, provenance_footer};

/// What the approval store says about one change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Coverage {
    /// The change does not block a release, so no approval is needed.
    NotNeeded,
    /// Blocking, and no row on record is about it.
    None,
    /// Blocking, and this row approves it for exactly these bytes.
    Approved(Approval),
    /// Blocking, and the only row about it was given for other bytes — the
    /// contract has been edited since. Not applied; `idl-diff` refuses.
    Stale(Approval),
}

/// The store the page read: where it was, how many rows it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRead {
    /// The path, as the page names it.
    pub path: String,
    /// Rows on record, whether or not they are about this diff.
    pub rows: usize,
}

/// One contract revision compared against another.
#[derive(Debug, Clone)]
pub struct ContractDiff {
    /// How the released side is named on the page — a path, usually.
    pub released: String,
    /// How the proposed side is named.
    pub proposed: String,
    /// Every difference, worst first, exactly as the differ ordered them.
    pub changes: Vec<Change>,
    /// The approval store consulted, if there was one to consult.
    pub store: Option<StoreRead>,
    /// What the store says about each change, index for index with
    /// `changes`. All [`Coverage::NotNeeded`] or [`Coverage::None`] when no
    /// store was read.
    pub coverage: Vec<Coverage>,
}

impl ContractDiff {
    /// Compares two registries.
    pub fn new(
        released: impl Into<String>,
        proposed: impl Into<String>,
        old: &Registry,
        new: &Registry,
    ) -> Self {
        let changes = diff(old, new);
        let coverage = changes
            .iter()
            .map(|c| if c.verdict.blocks_release() { Coverage::None } else { Coverage::NotNeeded })
            .collect();
        Self {
            released: released.into(),
            proposed: proposed.into(),
            changes,
            store: None,
            coverage,
        }
    }

    /// Reads `store` against this diff: `released_sha256` and `proposed_sha256`
    /// are the two units' fingerprints ([`approval::fingerprint`]), and a row
    /// applies only when both match — the same rule `idl-diff` refuses by.
    pub fn with_store(
        mut self,
        store: &Store,
        released_sha256: &str,
        proposed_sha256: &str,
    ) -> Self {
        self.coverage = self
            .changes
            .iter()
            .map(|c| {
                if !c.verdict.blocks_release() {
                    Coverage::NotNeeded
                } else if let Some(a) = store.covering(released_sha256, proposed_sha256, c) {
                    Coverage::Approved(a.clone())
                } else if let Some(a) = store.stale_for(released_sha256, proposed_sha256, c) {
                    Coverage::Stale(a.clone())
                } else {
                    Coverage::None
                }
            })
            .collect();
        self.store =
            Some(StoreRead { path: store.path.display().to_string(), rows: store.approvals.len() });
        self
    }

    /// How many blocking changes carry an approval that applies.
    pub fn approved(&self) -> usize {
        self.coverage.iter().filter(|c| matches!(c, Coverage::Approved(_))).count()
    }

    /// How many blocking changes carry only an approval for other bytes.
    pub fn stale(&self) -> usize {
        self.coverage.iter().filter(|c| matches!(c, Coverage::Stale(_))).count()
    }

    /// How many changes would need an explicit approval at the release gate.
    ///
    /// [`Verdict::blocks_release`]'s answer, not a second reading of it.
    pub fn blocking(&self) -> usize {
        self.changes.iter().filter(|c| c.verdict.blocks_release()).count()
    }

    /// How many changes carry `verdict`.
    pub fn count(&self, verdict: Verdict) -> usize {
        self.changes.iter().filter(|c| c.verdict == verdict).count()
    }
}

/// Loads both contracts as translation units, diffs them, and reads the
/// approval store — `store` if given, else `<proposed>.approvals.tsv` if it
/// exists. Returns the view and the resolver's advice about either unit.
///
/// One pass per side: the unit that is parsed is the unit that is
/// fingerprinted, so the bytes the page says an approval binds to are the
/// bytes it drew. A store that is there and malformed is an error, as it is
/// for `idl-diff`: a page that silently rendered "none on record" over a
/// refused store would show the operator less than the gate knows.
pub fn load(
    released: &Path,
    proposed: &Path,
    search: &SearchPath,
    store: Option<&Path>,
) -> Result<(ContractDiff, Vec<String>), String> {
    let mut advice = Vec::new();
    let mut load_one = |path: &Path| -> Result<(Registry, String), String> {
        let contract = Contract::load(path, search, Strictness::Grammar).map_err(|e| e.message)?;
        advice.extend(contract.unit.advice.iter().map(|d| contract.unit.render(d)));
        let mut registry = Registry::new();
        registry.load(&contract.spec).map_err(|e| format!("{}: {e}", path.display()))?;
        let sha = approval::fingerprint(&contract.unit.files).map_err(|e| e.to_string())?;
        Ok((registry, sha))
    };
    let (old, released_sha) = load_one(released)?;
    let (new, proposed_sha) = load_one(proposed)?;
    let view = ContractDiff::new(
        released.display().to_string(),
        proposed.display().to_string(),
        &old,
        &new,
    );
    let store_path = store.map(Path::to_owned).unwrap_or_else(|| approval::default_store(proposed));
    let view = match approval::read_store(&store_path).map_err(|e| e.to_string())? {
        Some(store) => view.with_store(&store, &released_sha, &proposed_sha),
        None => view,
    };
    Ok((view, advice))
}

const VERDICTS: [Verdict; 4] =
    [Verdict::Breaking, Verdict::ConditionallyBreaking, Verdict::ServerFirst, Verdict::Compatible];

/// Renders the diff as one self-contained HTML file.
pub fn render_html(view: &ContractDiff) -> String {
    let mut body = Markup::empty();
    body.push(Markup::labelled("h1", "", "Contract diff"));
    body.push(Markup::labelled("p", "sub", &format!("{} → {}", view.released, view.proposed)));

    let mut stats = Markup::empty();
    for verdict in VERDICTS {
        let class = if verdict.blocks_release() { "stat stop" } else { "stat" };
        let mut inner = Markup::labelled("b", "", &view.count(verdict).to_string());
        inner.push(Markup::text(&format!(" {}", verdict.label())));
        stats.push(Markup::element("div", class, inner));
    }
    let mut card = Markup::labelled("p", "", &gate_words(view, true));
    if let Some(store) = &view.store {
        card.push(Markup::labelled("p", "note", &store_summary(view, store)));
    }
    card.push(Markup::element("div", "summary", stats));
    body.push(Markup::element("div", "card", card));

    body.push(Markup::labelled("h2", "", "Changes"));
    if view.changes.is_empty() {
        body.push(Markup::labelled("p", "absent", "no difference between the two contracts"));
    } else {
        let mut head = Markup::empty();
        for column in ["verdict", "repository id", "what changed", "why that verdict"] {
            head.push(Markup::labelled("th", "", column));
        }
        if view.store.is_some() {
            head.push(Markup::labelled("th", "", "approval on record"));
        }
        let mut rows = Markup::element("tr", "", head);
        for (change, coverage) in view.changes.iter().zip(&view.coverage) {
            let badge =
                if change.verdict.blocks_release() { "badge b-destructive" } else { "badge b-ok" };
            let mut cells =
                Markup::element("td", "", Markup::labelled("span", badge, change.verdict.label()));
            cells.push(Markup::element("td", "", Markup::labelled("span", "mono", &change.id)));
            cells.push(Markup::labelled("td", "", &change.what));
            cells.push(Markup::labelled("td", "note", change.why));
            if view.store.is_some() {
                let class = match coverage {
                    Coverage::Approved(_) => "",
                    _ => "note",
                };
                cells.push(Markup::labelled("td", class, &coverage_words(coverage)));
            }
            // An approved row still breaks a peer; the row keeps its colour and
            // the approval column says who accepted that. Nothing here decides.
            let class = if change.verdict.blocks_release() { "row-refuse" } else { "" };
            rows.push(Markup::element("tr", class, cells));
        }
        body.push(Markup::element("div", "scroll", Markup::element("table", "", rows)));
    }

    body.push(Markup::labelled(
        "p",
        "note",
        "The release gate is idl-diff, which refuses with a non-zero exit and records an approval \
         — who, why, for which bytes — in the store beside the proposed contract. This page \
         renders the same verdicts and the same store, refuses nothing and writes nothing.",
    ));
    body.push(provenance_footer());
    page("Contract diff — orbweaver-console", body)
}

/// One sentence about the store: where it is and how much of it applies here.
fn store_summary(view: &ContractDiff, store: &StoreRead) -> String {
    let mut s = format!(
        "Approvals read from {}: {} row(s) on record, {} apply to this diff",
        store.path,
        store.rows,
        view.approved()
    );
    match view.stale() {
        0 => {}
        1 => {
            s.push_str(", 1 was given for a different revision of the contract and is not applied")
        }
        n => s.push_str(&format!(
            ", {n} were given for a different revision of the contract and are not applied"
        )),
    }
    s.push('.');
    s
}

/// The approval column, in words. `Approved` says who, why and when; `Stale`
/// says the same and that it no longer applies; the rest say why the cell is
/// otherwise empty rather than leaving a blank an operator could read either way.
fn coverage_words(coverage: &Coverage) -> String {
    match coverage {
        Coverage::NotNeeded => "not needed".to_owned(),
        Coverage::None => "none on record".to_owned(),
        Coverage::Approved(a) => {
            format!("approved by {}: {} ({})", a.approver, a.reason, a.approved_at)
        }
        Coverage::Stale(a) => format!(
            "on record for a different revision — not applied (approved by {} on {}: {})",
            a.approver, a.approved_at, a.reason
        ),
    }
}

/// Renders the diff for a terminal.
pub fn render_text(view: &ContractDiff) -> String {
    let mut out = format!("CONTRACT DIFF\n{} -> {}\n", view.released, view.proposed);
    for verdict in VERDICTS {
        out.push_str(&format!("  {:<24} {}\n", verdict.label(), view.count(verdict)));
    }
    out.push('\n');
    if view.changes.is_empty() {
        out.push_str("no difference between the two contracts\n");
    }
    for (change, coverage) in view.changes.iter().zip(&view.coverage) {
        out.push_str(&format!("{change}\n"));
        match coverage {
            Coverage::Approved(a) => out.push_str(&format!(
                "    [approved by {}: {}] {}\n",
                a.approver, a.reason, a.approved_at
            )),
            Coverage::Stale(a) => out.push_str(&format!(
                "    [approval by {} on {} was for a different revision — not applied]\n",
                a.approver, a.approved_at
            )),
            Coverage::NotNeeded | Coverage::None => {}
        }
    }
    if let Some(store) = &view.store {
        out.push_str(&format!("\n{}\n", store_summary(view, store)));
    }
    out.push_str(&format!("\n{}\n", gate_words(view, false)));
    out
}

/// What the release gate would want, in words — and, when a store was read,
/// how much of it is already on record. The gate's own answer is `idl-diff`'s
/// exit code; this only restates its rule.
fn gate_words(view: &ContractDiff, long: bool) -> String {
    let n = view.blocking();
    if n == 0 {
        return if long {
            "Nothing here needs an approval at the release gate.".to_owned()
        } else {
            "nothing here needs an approval at the release gate".to_owned()
        };
    }
    let mut s = format!("{n} change(s) would need an explicit --approve at the release gate");
    if view.store.is_some() {
        s.push_str(&format!("; {} of them carry one on record for these bytes", view.approved()));
    }
    if long {
        s.push_str(
            ". A released type is not editable in place; publish a new version of the interface.",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(src: &str) -> Registry {
        let spec = orbweaver_idl::parse(src).expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        r
    }

    const RELEASED: &str = "module pay {
        struct Amount { long cents; string currency; };
        interface Teller { Amount quote(); };
      };";

    /// The members are swapped. §5.3's central claim, verified on the wire in
    /// Phase 2: a deployed peer reads the other member's value and raises
    /// nothing.
    const SWAPPED: &str = "module pay {
        struct Amount { string currency; long cents; };
        interface Teller { Amount quote(); };
      };";

    const ADDED: &str = "module pay {
        struct Amount { long cents; string currency; };
        interface Teller { Amount quote(); void refresh(); };
      };";

    #[test]
    fn a_reordered_member_is_breaking_and_blocks() {
        let view =
            ContractDiff::new("released", "proposed", &registry(RELEASED), &registry(SWAPPED));
        assert!(view.blocking() > 0, "{:?}", view.changes);
        assert!(view.changes.iter().any(|c| c.verdict == Verdict::Breaking));
        let html = render_html(&view);
        assert!(html.contains("BREAKING"), "{html}");
        assert!(html.contains("--approve"), "{html}");
    }

    #[test]
    fn an_added_operation_is_server_first_and_does_not_block() {
        let view = ContractDiff::new("released", "proposed", &registry(RELEASED), &registry(ADDED));
        assert_eq!(view.blocking(), 0, "{:?}", view.changes);
        assert_eq!(view.count(Verdict::ServerFirst), 1, "{:?}", view.changes);
        let text = render_text(&view);
        assert!(text.contains("server-first"), "{text}");
        assert!(text.contains("nothing here needs an approval"), "{text}");
    }

    #[test]
    fn an_identical_contract_produces_no_change() {
        let view =
            ContractDiff::new("released", "proposed", &registry(RELEASED), &registry(RELEASED));
        assert!(view.changes.is_empty());
        assert!(render_html(&view).contains("no difference"));
    }

    /// The verdict on the page is the differ's, counted through
    /// `blocks_release` rather than through a second reading of the table.
    #[test]
    fn the_blocking_count_is_the_verdicts_own_answer() {
        let view = ContractDiff::new("a", "b", &registry(RELEASED), &registry(SWAPPED));
        let expected = view.changes.iter().filter(|c| c.verdict.blocks_release()).count();
        assert_eq!(view.blocking(), expected);
    }

    fn row_for(change: &Change, released_sha: &str, proposed_sha: &str, reason: &str) -> Approval {
        Approval {
            released: "released".into(),
            proposed: "proposed".into(),
            released_sha256: released_sha.into(),
            proposed_sha256: proposed_sha.into(),
            id: change.id.clone(),
            verdict: change.verdict.label().to_owned(),
            what: change.what.clone(),
            reason: reason.into(),
            approver: "reviewer".into(),
            approved_at: "2026-08-19T00:00:00Z".into(),
        }
    }

    fn store_with(rows: Vec<Approval>) -> Store {
        Store { path: std::path::PathBuf::from("proposed.idl.approvals.tsv"), approvals: rows }
    }

    /// Who, why, when, for which finding — as words, on both renderings.
    #[test]
    fn an_approval_on_record_is_rendered_with_who_why_and_when() {
        let view =
            ContractDiff::new("released", "proposed", &registry(RELEASED), &registry(SWAPPED));
        assert!(view.store.is_none());
        assert_eq!(view.approved(), 0);
        let (r, p) = (approval::sha256_hex(b"released"), approval::sha256_hex(b"proposed"));
        let breaking = view.changes.iter().find(|c| c.verdict == Verdict::Breaking).unwrap();
        let store = store_with(vec![row_for(breaking, &r, &p, "v2 rollout, peers rebuilt")]);
        let view = view.with_store(&store, &r, &p);
        assert_eq!(view.approved(), 1, "{:?}", view.coverage);
        assert_eq!(view.stale(), 0);
        let text = render_text(&view);
        assert!(
            text.contains("[approved by reviewer: v2 rollout, peers rebuilt] 2026-08-19T00:00:00Z"),
            "{text}"
        );
        assert!(text.contains("1 row(s) on record, 1 apply"), "{text}");
        let html = render_html(&view);
        assert!(html.contains("approval on record"), "{html}");
        assert!(
            html.contains("approved by reviewer: v2 rollout, peers rebuilt (2026-08-19T00:00:00Z)"),
            "{html}"
        );
        assert!(html.contains("proposed.idl.approvals.tsv"), "{html}");
        // The store said nothing about the rest; the page says so rather than
        // leaving the cell blank.
        if view.blocking() > 1 {
            assert!(html.contains("none on record"), "{html}");
        }
    }

    /// The same row against different bytes: shown, and shown as not applying.
    #[test]
    fn an_approval_for_other_bytes_is_shown_and_not_applied() {
        let view =
            ContractDiff::new("released", "proposed", &registry(RELEASED), &registry(SWAPPED));
        let (r, p) = (approval::sha256_hex(b"released"), approval::sha256_hex(b"proposed"));
        let breaking = view.changes.iter().find(|c| c.verdict == Verdict::Breaking).unwrap();
        let store = store_with(vec![row_for(breaking, &r, &p, "was fine then")]);
        let edited = approval::sha256_hex(b"proposed, then edited");
        let view = view.with_store(&store, &r, &edited);
        assert_eq!(view.approved(), 0, "{:?}", view.coverage);
        assert_eq!(view.stale(), 1);
        let text = render_text(&view);
        assert!(text.contains("was for a different revision"), "{text}");
        assert!(
            text.contains("0 apply to this diff, 1 was given for a different revision"),
            "{text}"
        );
        let html = render_html(&view);
        assert!(html.contains("not applied"), "{html}");
        assert!(html.contains("was fine then"), "{html}");
    }

    /// A reason is text a person typed. It goes through `Markup::text` like
    /// every other value, so a reason that is markup renders as its characters.
    #[test]
    fn a_reason_is_text_not_markup() {
        let view =
            ContractDiff::new("released", "proposed", &registry(RELEASED), &registry(SWAPPED));
        let (r, p) = (approval::sha256_hex(b"r"), approval::sha256_hex(b"p"));
        let breaking = view.changes.iter().find(|c| c.verdict == Verdict::Breaking).unwrap();
        let store = store_with(vec![row_for(breaking, &r, &p, "<script>alert(1)</script>")]);
        let html = render_html(&view.with_store(&store, &r, &p));
        assert!(!html.contains("<script>alert"), "{html}");
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"), "{html}");
    }

    /// `load` end to end: no store beside the proposed file, then one written
    /// as `idl-diff --approve` writes it, then one with the approver blanked.
    #[test]
    fn load_reads_the_default_store_beside_the_proposed_file_and_refuses_a_nameless_one() {
        let dir =
            std::env::temp_dir().join(format!("orbweaver-console-diff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch");
        let released = dir.join("released.idl");
        let proposed = dir.join("proposed.idl");
        std::fs::write(&released, RELEASED).expect("released");
        std::fs::write(&proposed, SWAPPED).expect("proposed");
        let store_path = approval::default_store(&proposed);
        let _ = std::fs::remove_file(&store_path);

        let (view, _) = load(&released, &proposed, &SearchPath::new(), None).expect("loads");
        assert!(view.store.is_none(), "no store, none read");
        assert!(view.blocking() > 0);

        let (r, p) = (
            approval::fingerprint(&[&released]).unwrap(),
            approval::fingerprint(&[&proposed]).unwrap(),
        );
        let rows: Vec<Approval> = view
            .changes
            .iter()
            .filter(|c| c.verdict.blocks_release())
            .map(|c| row_for(c, &r, &p, "checked against every peer"))
            .collect();
        approval::append(&store_path, &rows).expect("store written");
        let (view, _) = load(&released, &proposed, &SearchPath::new(), None).expect("loads");
        assert_eq!(view.approved(), view.blocking(), "{:?}", view.coverage);
        assert!(render_text(&view).contains("[approved by reviewer: checked against every peer]"));

        // Edit one byte of the proposed file: every row is now for other bytes.
        std::fs::write(&proposed, format!("{SWAPPED}\n")).expect("edit");
        let (view, _) = load(&released, &proposed, &SearchPath::new(), None).expect("loads");
        assert_eq!(view.approved(), 0, "{:?}", view.coverage);
        assert_eq!(view.stale(), view.blocking());

        // Blank the approver: the store is refused, and so is the page.
        let text = std::fs::read_to_string(&store_path).unwrap().replace("\treviewer\t", "\t\t");
        std::fs::write(&store_path, text).unwrap();
        let err = load(&released, &proposed, &SearchPath::new(), None).expect_err("refused");
        assert!(err.contains("approver is blank"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
