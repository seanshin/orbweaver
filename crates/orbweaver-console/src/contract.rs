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
//! takes `--approve <reason>` so the decision travels with the diff. The
//! console renders the same verdicts for someone deciding *whether* to ask for
//! that approval. It exits zero on a breaking change, deliberately: a viewer
//! that also refused would be a second gate a release could be routed around.

use orbweaver_registry::Registry;
use orbweaver_registry::diff::{Change, Verdict, diff};

use crate::html::{Markup, page, provenance_footer};

/// One contract revision compared against another.
#[derive(Debug, Clone)]
pub struct ContractDiff {
    /// How the released side is named on the page — a path, usually.
    pub released: String,
    /// How the proposed side is named.
    pub proposed: String,
    /// Every difference, worst first, exactly as the differ ordered them.
    pub changes: Vec<Change>,
}

impl ContractDiff {
    /// Compares two registries.
    pub fn new(
        released: impl Into<String>,
        proposed: impl Into<String>,
        old: &Registry,
        new: &Registry,
    ) -> Self {
        Self { released: released.into(), proposed: proposed.into(), changes: diff(old, new) }
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
    let mut card = Markup::labelled(
        "p",
        "",
        &match view.blocking() {
            0 => "Nothing here needs an approval at the release gate.".to_owned(),
            n => format!(
                "{n} change(s) would need an explicit --approve at the release gate. A released \
                 type is not editable in place; publish a new version of the interface."
            ),
        },
    );
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
        let mut rows = Markup::element("tr", "", head);
        for change in &view.changes {
            let badge =
                if change.verdict.blocks_release() { "badge b-destructive" } else { "badge b-ok" };
            let mut cells =
                Markup::element("td", "", Markup::labelled("span", badge, change.verdict.label()));
            cells.push(Markup::element("td", "", Markup::labelled("span", "mono", &change.id)));
            cells.push(Markup::labelled("td", "", &change.what));
            cells.push(Markup::labelled("td", "note", change.why));
            let class = if change.verdict.blocks_release() { "row-refuse" } else { "" };
            rows.push(Markup::element("tr", class, cells));
        }
        body.push(Markup::element("div", "scroll", Markup::element("table", "", rows)));
    }

    body.push(Markup::labelled(
        "p",
        "note",
        "The release gate is idl-diff, which refuses with a non-zero exit and records an approval \
         and its reason. This page renders the same verdicts and refuses nothing.",
    ));
    body.push(provenance_footer());
    page("Contract diff — orbweaver-console", body)
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
    for change in &view.changes {
        out.push_str(&format!("{change}\n"));
    }
    out.push_str(&match view.blocking() {
        0 => "\nnothing here needs an approval at the release gate\n".to_owned(),
        n => format!("\n{n} change(s) would need an explicit --approve at the release gate\n"),
    });
    out
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
}
