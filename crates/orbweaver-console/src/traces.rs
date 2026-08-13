//! The trace view: D004's span records, grouped into sessions.
//!
//! # The record shape is not this crate's to choose
//!
//! `docs/decisions/D004-observability.md` fixes one JSON object per line with
//! nine keys — `ts`, `session`, `caller`, `target`, `operation`, `decision`,
//! `stage`, `path`, `outcome` — precisely because the emitter and the console
//! are separate batches, and "a format settled by whoever committed earliest is
//! a format nobody agreed to". This module reads that table and adds nothing to
//! it.
//!
//! **There is no duration.** D004 refuses one: it would need a clock, the
//! residency machine's no-clock discipline is what makes replay deterministic,
//! and a duration nobody can reproduce is worse than an absent one. So no view
//! here computes, infers or displays elapsed time, and a test asserts it.
//!
//! # Absent is a rendering, not a default
//!
//! Every key is optional in [`Field`], with three states rather than two: the
//! key was absent, the key carried a string, or the key carried something that
//! is not a string. The third exists because collapsing it into "absent" would
//! make the page say a field was missing when it was in fact malformed — the
//! console reporting a measurement it did not take, which is the one thing
//! `CLAUDE.md` says a report may never do. Malformed lines are counted and
//! listed for the same reason: a line the console could not read is a failure,
//! never a silently smaller table.
//!
//! # A dry run must never look like a call
//!
//! The `decision` vocabulary is the audit line's own, and
//! [`orbweaver_mcp::guard::is_hypothetical`] is the single place the project
//! decides whether a decision describes a call that happened. This module asks
//! that function rather than matching strings of its own, so the console and
//! `promote::verify_promotion` cannot come to different conclusions about the
//! same line. D004's table writes the tokens in lower case and the constants
//! are upper case, so the token is upper-cased before the question is asked —
//! the one normalisation here, and it is stated rather than hidden.
//!
//! A token that is neither is rendered **unknown** and counted separately. It
//! is not guessed into either bucket: calling an unknown token real would
//! invent a call, and calling it hypothetical would hide one.

use std::collections::BTreeMap;

use orbweaver_dynamic::json::Json;
use orbweaver_mcp::guard::{
    DECISION_ALLOW, DECISION_DRY_RUN_ALLOW, DECISION_DRY_RUN_REFUSE, DECISION_REFUSE,
    is_hypothetical,
};

use crate::html::{Markup, page, provenance_footer};

/// The nine keys D004 fixes, in the order the decision writes them.
pub const KEYS: [&str; 9] =
    ["ts", "session", "caller", "target", "operation", "decision", "stage", "path", "outcome"];

/// One field of a span record, as the line actually carried it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Field {
    /// The key was not in the object.
    #[default]
    Absent,
    /// The key carried a string.
    Text(String),
    /// The key carried something that is not a string; the kind, as the JSON
    /// model names it.
    NotAString(&'static str),
}

impl Field {
    /// Reads one key of an object.
    pub fn read(object: &Json, key: &str) -> Self {
        match object.get(key) {
            None => Field::Absent,
            Some(Json::String(text)) => Field::Text(text.clone()),
            Some(other) => Field::NotAString(other.kind()),
        }
    }

    /// The string, when there is one.
    pub fn text(&self) -> Option<&str> {
        match self {
            Field::Text(text) => Some(text),
            _ => None,
        }
    }

    /// How it reads to an operator. Never a value when there is none.
    pub fn label(&self) -> String {
        match self {
            Field::Absent => "absent".to_owned(),
            Field::Text(text) => text.clone(),
            Field::NotAString(kind) => format!("not a string (was {kind})"),
        }
    }

    fn markup(&self) -> Markup {
        match self {
            Field::Text(text) => Markup::labelled("span", "mono", text),
            Field::Absent => Markup::labelled("span", "absent", "absent"),
            Field::NotAString(kind) => {
                Markup::labelled("span", "badge b-unknown", &format!("not a string: {kind}"))
            }
        }
    }
}

/// What a `decision` token says happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// The policy allowed a call, and the call was made.
    Allow,
    /// The policy refused a call.
    Refuse,
    /// A question. The policy **would** have allowed it; nothing was called.
    DryRunAllow,
    /// A question. The policy would have refused it; nothing was called.
    DryRunRefuse,
    /// A token outside D004's vocabulary, kept verbatim.
    Unknown(String),
    /// No `decision` key at all.
    Absent,
}

impl Decision {
    /// Classifies a token, upper-cased first so D004's lower-case table and
    /// [`orbweaver_mcp::guard`]'s upper-case constants agree.
    pub fn classify(field: &Field) -> Self {
        let Some(raw) = field.text() else {
            return match field {
                Field::NotAString(kind) => Decision::Unknown(format!("not a string ({kind})")),
                _ => Decision::Absent,
            };
        };
        let token = raw.to_ascii_uppercase();
        if token == DECISION_ALLOW {
            Decision::Allow
        } else if token == DECISION_REFUSE {
            Decision::Refuse
        } else if token == DECISION_DRY_RUN_ALLOW {
            Decision::DryRunAllow
        } else if token == DECISION_DRY_RUN_REFUSE {
            Decision::DryRunRefuse
        } else {
            Decision::Unknown(raw.to_owned())
        }
    }

    /// Whether this describes a call that never happened, as
    /// [`orbweaver_mcp::guard::is_hypothetical`] decides it.
    pub fn hypothetical(&self) -> bool {
        match self {
            Decision::DryRunAllow => is_hypothetical(DECISION_DRY_RUN_ALLOW),
            Decision::DryRunRefuse => is_hypothetical(DECISION_DRY_RUN_REFUSE),
            _ => false,
        }
    }

    /// Whether the policy said no. A dry-run refusal counts: it is a refusal of
    /// a question, and an operator hunting refusals wants both.
    pub fn refused(&self) -> bool {
        matches!(self, Decision::Refuse | Decision::DryRunRefuse)
    }

    /// Whether a call was actually attempted.
    pub fn real_call(&self) -> bool {
        matches!(self, Decision::Allow | Decision::Refuse)
    }

    /// How it reads on a page. A dry run says so in words, never only in
    /// colour: colour is not available to every reader and is not available at
    /// all in the text mode.
    pub fn label(&self) -> String {
        match self {
            Decision::Allow => "allow".to_owned(),
            Decision::Refuse => "REFUSED".to_owned(),
            Decision::DryRunAllow => "dry run — would allow, no call made".to_owned(),
            Decision::DryRunRefuse => "dry run — would REFUSE, no call made".to_owned(),
            Decision::Unknown(raw) => format!("unknown decision: {raw}"),
            Decision::Absent => "no decision field".to_owned(),
        }
    }

    /// The same fact, short enough for a table cell. The badge says "dry run"
    /// in the same breath as the verdict, so no width of column can separate
    /// the two — and the row carries `no call was made` underneath as well,
    /// because a badge is not a sentence.
    pub fn badge_label(&self) -> String {
        match self {
            Decision::DryRunAllow => "dry run · would allow".to_owned(),
            Decision::DryRunRefuse => "dry run · would refuse".to_owned(),
            other => other.label(),
        }
    }

    fn badge_class(&self) -> &'static str {
        match self {
            Decision::Allow => "badge b-ok",
            Decision::Refuse => "badge b-destructive",
            Decision::DryRunAllow | Decision::DryRunRefuse => "badge b-dry",
            Decision::Unknown(_) | Decision::Absent => "badge b-unknown",
        }
    }
}

/// One span record.
#[derive(Debug, Clone)]
pub struct Span {
    /// Which line of which file it came from, so a finding is locatable.
    pub source: Origin,
    /// The nine keys, in D004's order.
    pub fields: BTreeMap<&'static str, Field>,
    /// Keys this console does not know. Named, never dropped: a field the
    /// emitter writes and the console silently discards is a fact nobody sees.
    pub extra_keys: Vec<String>,
    /// What the `decision` token says happened.
    pub decision: Decision,
}

impl Span {
    /// One of D004's nine fields.
    pub fn field(&self, key: &str) -> &Field {
        static ABSENT: Field = Field::Absent;
        self.fields.get(key).unwrap_or(&ABSENT)
    }
}

/// Where a line came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// The file, as it was named on the command line.
    pub file: String,
    /// 1-based line number.
    pub line: usize,
}

/// A line that could not be read as a span record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    /// Where it was.
    pub source: Origin,
    /// What was wrong, in the parser's own words.
    pub why: String,
}

/// Every span that named one session, in file order.
///
/// File order, not `ts` order: `ts` comes from the caller, D004 adds no clock,
/// and re-sorting by a timestamp the console did not take would be the console
/// asserting an ordering it cannot vouch for.
#[derive(Debug, Clone)]
pub struct Session {
    /// The `session` value, or `None` for lines that carried no usable one.
    pub id: Option<String>,
    /// Its spans, in the order they were read.
    pub spans: Vec<Span>,
}

impl Session {
    /// How this session is named on the page.
    pub fn label(&self) -> &str {
        self.id.as_deref().unwrap_or("(no session field)")
    }

    /// How many of its spans were refusals.
    pub fn refusals(&self) -> usize {
        self.spans.iter().filter(|s| s.decision.refused()).count()
    }

    /// How many of its spans describe a call that never happened.
    pub fn hypotheticals(&self) -> usize {
        self.spans.iter().filter(|s| s.decision.hypothetical()).count()
    }
}

/// A whole trace log.
#[derive(Debug, Clone, Default)]
pub struct TraceLog {
    /// Sessions, in the order they first appear.
    pub sessions: Vec<Session>,
    /// Lines that could not be read. Counted and listed, never dropped.
    pub unreadable: Vec<Unreadable>,
}

impl TraceLog {
    /// Reads JSON lines. Blank lines are skipped; anything else that is not a
    /// readable object becomes an [`Unreadable`].
    ///
    /// `file` is how the source is named on the page.
    pub fn read(&mut self, file: &str, contents: &str) {
        for (index, line) in contents.lines().enumerate() {
            let source = Origin { file: file.to_owned(), line: index + 1 };
            if line.trim().is_empty() {
                continue;
            }
            match Json::parse(line) {
                Ok(Json::Object(map)) => self.push(Span::from_object(source, &Json::Object(map))),
                Ok(other) => self.unreadable.push(Unreadable {
                    source,
                    why: format!("a span record is a JSON object; this line is {}", other.kind()),
                }),
                Err(e) => self.unreadable.push(Unreadable { source, why: e.to_string() }),
            }
        }
    }

    fn push(&mut self, span: Span) {
        let id = span.field("session").text().map(ToOwned::to_owned);
        match self.sessions.iter_mut().find(|s| s.id == id) {
            Some(session) => session.spans.push(span),
            None => self.sessions.push(Session { id, spans: vec![span] }),
        }
    }

    /// Every span, in the order sessions and then lines appear.
    pub fn spans(&self) -> impl Iterator<Item = &Span> {
        self.sessions.iter().flat_map(|s| s.spans.iter())
    }

    /// How many spans were read.
    pub fn total(&self) -> usize {
        self.spans().count()
    }

    /// How many spans the policy refused, dry runs included.
    pub fn refusals(&self) -> usize {
        self.spans().filter(|s| s.decision.refused()).count()
    }

    /// How many spans describe a call that was never made.
    pub fn hypotheticals(&self) -> usize {
        self.spans().filter(|s| s.decision.hypothetical()).count()
    }

    /// How many spans describe a call that was actually attempted.
    pub fn real_calls(&self) -> usize {
        self.spans().filter(|s| s.decision.real_call()).count()
    }

    /// How many spans carried a `decision` this console does not recognise, or
    /// none at all.
    pub fn unclassified(&self) -> usize {
        self.spans()
            .filter(|s| matches!(s.decision, Decision::Unknown(_) | Decision::Absent))
            .count()
    }

    /// Every unknown key seen, sorted and deduplicated.
    pub fn extra_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> =
            self.spans().flat_map(|s| s.extra_keys.iter().cloned()).collect();
        keys.sort();
        keys.dedup();
        keys
    }
}

impl Span {
    fn from_object(source: Origin, object: &Json) -> Self {
        let fields: BTreeMap<&'static str, Field> =
            KEYS.iter().map(|key| (*key, Field::read(object, key))).collect();
        let extra_keys = match object {
            Json::Object(map) => {
                map.keys().filter(|k| !KEYS.contains(&k.as_str())).cloned().collect()
            }
            _ => Vec::new(),
        };
        let decision = Decision::classify(fields.get("decision").unwrap_or(&Field::Absent));
        Span { source, fields, extra_keys, decision }
    }
}

/// Renders the trace log as one self-contained HTML file.
pub fn render_html(log: &TraceLog) -> String {
    let mut body = Markup::empty();
    body.push(Markup::labelled("h1", "", "Traces"));
    body.push(Markup::labelled(
        "p",
        "sub",
        "D004 span records, grouped into sessions in the order they were written.",
    ));

    let mut stats = Markup::empty();
    stats.push(count_stat("", log.total(), "records"));
    stats.push(count_stat("", log.sessions.len(), "sessions"));
    stats.push(count_stat("stop", log.refusals(), "refusals"));
    stats.push(count_stat("", log.real_calls(), "real calls"));
    stats.push(count_stat("", log.hypotheticals(), "dry runs — no call made"));
    stats.push(count_stat("warn", log.unclassified(), "unclassified decisions"));
    stats.push(count_stat("warn", log.unreadable.len(), "unreadable lines"));

    let mut card = Markup::labelled(
        "p",
        "",
        "A dry run is a question the policy answered and nothing more: no call was made, and no \
         row below that says so describes one. There are no durations here — D004 fixes no \
         duration field, because it would need a clock the interceptor chain does not have.",
    );
    card.push(Markup::element("div", "summary", stats));
    body.push(Markup::element("div", "card", card));

    if !log.unreadable.is_empty() {
        body.push(Markup::labelled("h2", "", "Lines that could not be read"));
        let mut list = Markup::empty();
        for bad in &log.unreadable {
            list.push(Markup::labelled(
                "p",
                "",
                &format!("{}:{} — {}", bad.source.file, bad.source.line, bad.why),
            ));
        }
        body.push(Markup::element("div", "card", list));
    }

    let extra = log.extra_keys();
    if !extra.is_empty() {
        body.push(Markup::element(
            "div",
            "card",
            Markup::labelled(
                "p",
                "note",
                &format!(
                    "These keys are outside D004's table and this console does not render their \
                     values: {}",
                    extra.join(", ")
                ),
            ),
        ));
    }

    body.push(Markup::labelled("h2", "", "Sessions"));
    if log.sessions.is_empty() {
        body.push(Markup::labelled("p", "absent", "no records"));
    }
    for session in &log.sessions {
        body.push(session_card(session));
    }
    body.push(provenance_footer());
    page("Traces — orbweaver-console", body)
}

fn count_stat(kind: &'static str, n: usize, label: &str) -> Markup {
    let class = match kind {
        "stop" => "stat stop",
        "warn" => "stat warn",
        _ => "stat",
    };
    let mut inner = Markup::labelled("b", "", &n.to_string());
    inner.push(Markup::text(&format!(" {label}")));
    Markup::element("div", class, inner)
}

fn session_card(session: &Session) -> Markup {
    let mut inner = Markup::labelled("div", "id", session.label());
    let mut badges = Markup::empty();
    badges.push(Markup::labelled(
        "span",
        "badge b-dark",
        &format!("{} records", session.spans.len()),
    ));
    if session.refusals() > 0 {
        badges.push(Markup::labelled(
            "span",
            "badge b-destructive",
            &format!("{} refused", session.refusals()),
        ));
    }
    if session.hypotheticals() > 0 {
        badges.push(Markup::labelled(
            "span",
            "badge b-dry",
            &format!("{} dry run, no call made", session.hypotheticals()),
        ));
    }
    inner.push(Markup::element("div", "badges", badges));

    let mut head = Markup::empty();
    for column in ["ts", "caller", "target", "operation", "decision", "stage", "path", "outcome"] {
        head.push(Markup::labelled("th", "", column));
    }
    let mut rows = Markup::element("tr", "", head);

    for span in &session.spans {
        let mut cells = Markup::empty();
        cells.push(Markup::element("td", "", span.field("ts").markup()));
        cells.push(Markup::element("td", "", span.field("caller").markup()));
        cells.push(Markup::element("td", "", span.field("target").markup()));
        cells.push(Markup::element("td", "", span.field("operation").markup()));

        let mut decision =
            Markup::labelled("span", span.decision.badge_class(), &span.decision.badge_label());
        if span.decision.hypothetical() {
            decision.push(Markup::labelled("div", "note", "no call was made"));
        }
        cells.push(Markup::element("td", "", decision));

        cells.push(Markup::element("td", "", span.field("stage").markup()));
        cells.push(Markup::element("td", "", span.field("path").markup()));
        cells.push(Markup::element("td", "", span.field("outcome").markup()));

        let class = match (&span.decision, span.decision.hypothetical()) {
            (_, true) => "row-dry",
            (d, false) if d.refused() => "row-refuse",
            _ => "",
        };
        rows.push(Markup::element("tr", class, cells));
    }

    inner.push(Markup::element("div", "scroll", Markup::element("table", "", rows)));
    Markup::element("div", "iface", inner)
}

/// Renders the trace log for a terminal.
pub fn render_text(log: &TraceLog) -> String {
    let mut out = String::from("TRACES\n");
    out.push_str(&format!(
        "{} records in {} sessions: {} refused, {} real calls, {} dry runs (no call made), {} \
         unclassified, {} unreadable lines\n",
        log.total(),
        log.sessions.len(),
        log.refusals(),
        log.real_calls(),
        log.hypotheticals(),
        log.unclassified(),
        log.unreadable.len(),
    ));
    for bad in &log.unreadable {
        out.push_str(&format!("! {}:{} — {}\n", bad.source.file, bad.source.line, bad.why));
    }
    let extra = log.extra_keys();
    if !extra.is_empty() {
        out.push_str(&format!(
            "note: keys outside D004's table, not rendered: {}\n",
            extra.join(", ")
        ));
    }
    for session in &log.sessions {
        out.push_str(&format!("\nsession {}\n", session.label()));
        for span in &session.spans {
            let mark = if span.decision.hypothetical() {
                "[DRY RUN, NO CALL MADE]"
            } else if span.decision.refused() {
                "[REFUSED]"
            } else {
                "[        ]"
            };
            out.push_str(&format!(
                "  {mark} {} {} {}.{} decision={} stage={} path={} outcome={}\n",
                span.field("ts").label(),
                span.field("caller").label(),
                span.field("target").label(),
                span.field("operation").label(),
                span.decision.label(),
                span.field("stage").label(),
                span.field("path").label(),
                span.field("outcome").label(),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Written by hand from D004's table, because the emitter is a parallel
    /// batch: the decision fixed the shape so both sides could be built against
    /// it rather than against each other.
    const FIXTURE: &str = include_str!("../tests/fixtures/spans.jsonl");

    fn log(contents: &str) -> TraceLog {
        let mut log = TraceLog::default();
        log.read("spans.jsonl", contents);
        log
    }

    fn fixture() -> TraceLog {
        log(FIXTURE)
    }

    #[test]
    fn the_fixture_groups_into_sessions_in_file_order() {
        let log = fixture();
        let ids: Vec<&str> = log.sessions.iter().map(Session::label).collect();
        assert_eq!(ids, ["s-7f21", "s-9c04", "(no session field)"]);
        assert_eq!(log.sessions[0].spans.len(), 4);
    }

    #[test]
    fn every_key_d004_fixes_is_read() {
        let log = fixture();
        let first = log.sessions[0].spans.first().expect("a span");
        assert_eq!(first.field("ts").text(), Some("2026-08-14T09:12:03Z"));
        assert_eq!(first.field("caller").text(), Some("alice@example.test"));
        assert_eq!(first.field("target").text(), Some("IDL:bank/Account:1.0"));
        assert_eq!(first.field("operation").text(), Some("balance"));
        assert_eq!(first.field("stage").text(), Some("-"));
        assert_eq!(first.field("path").text(), Some("static"));
        assert_eq!(first.field("outcome").text(), Some("ok"));
        assert_eq!(first.decision, Decision::Allow);
    }

    /// The fixture's last line has a null `session`, no `caller` and no
    /// `stage`. It groups under its own heading and renders three absences
    /// rather than three invented values.
    #[test]
    fn a_record_with_no_usable_session_gets_its_own_group() {
        let log = fixture();
        let orphan = log.sessions.last().expect("a group");
        assert_eq!(orphan.id, None);
        assert_eq!(orphan.spans.len(), 1);
        let span = &orphan.spans[0];
        assert_eq!(*span.field("session"), Field::NotAString("null"));
        assert_eq!(*span.field("caller"), Field::Absent);
        assert_eq!(*span.field("stage"), Field::Absent);
    }

    #[test]
    fn a_refusal_names_its_stage_and_its_system_exception() {
        let log = fixture();
        let refusal = log
            .spans()
            .find(|s| s.field("operation").text() == Some("close"))
            .expect("the refusal");
        assert_eq!(refusal.decision, Decision::Refuse);
        assert!(refusal.decision.refused());
        assert_eq!(refusal.field("stage").text(), Some("safety.approval"));
        assert_eq!(refusal.field("outcome").text(), Some("IDL:omg.org/CORBA/NO_PERMISSION:1.0"));
    }

    /// The distinction the whole view stands on.
    #[test]
    fn a_dry_run_is_never_mistakable_for_a_call() {
        let log = fixture();
        let dry: Vec<&Span> = log.spans().filter(|s| s.decision.hypothetical()).collect();
        assert_eq!(dry.len(), 2, "the fixture has one of each dry-run token");
        for span in dry {
            assert!(!span.decision.real_call());
            assert!(span.decision.label().contains("no call made"));
        }
        assert_eq!(log.real_calls(), 5);

        let html = render_html(&log);
        // The badge names it a dry run beside the verdict, so no column width
        // can show one without the other, and the row says it again in a
        // sentence underneath.
        assert!(html.contains("dry run · would refuse"), "{html}");
        assert!(html.contains("dry run · would allow"), "{html}");
        assert!(html.contains("no call was made"), "{html}");
        let text = render_text(&log);
        assert!(text.contains("[DRY RUN, NO CALL MADE]"), "{text}");
    }

    /// A dry-run refusal is still a refusal an operator is hunting for.
    #[test]
    fn refusals_are_counted_including_the_hypothetical_ones() {
        let log = fixture();
        assert_eq!(log.refusals(), 3, "two refusals and one refused dry run");
        assert!(render_text(&log).contains("[REFUSED]"));
    }

    #[test]
    fn an_absent_field_is_rendered_absent_and_never_as_a_value() {
        let log = log(
            r#"{"ts":"2026-08-14T09:00:00Z","session":"s","decision":"allow","path":"dynamic"}"#,
        );
        let span = log.spans().next().expect("a span");
        assert_eq!(*span.field("caller"), Field::Absent);
        assert_eq!(span.field("caller").label(), "absent");
        assert!(render_html(&log).contains("absent"));
    }

    /// Collapsing this into "absent" would make the page say a field was
    /// missing when it was malformed.
    #[test]
    fn a_field_that_is_not_a_string_is_reported_rather_than_hidden() {
        let log = log(r#"{"session":"s","decision":"allow","outcome":404}"#);
        let span = log.spans().next().expect("a span");
        assert_eq!(*span.field("outcome"), Field::NotAString("a number"));
        assert_ne!(*span.field("outcome"), Field::Absent);
        assert!(span.field("outcome").label().contains("not a string"));
        assert!(render_html(&log).contains("not a string"));
    }

    #[test]
    fn an_unknown_decision_is_neither_a_call_nor_a_dry_run() {
        let log = log(r#"{"session":"s","decision":"maybe"}"#);
        let span = log.spans().next().expect("a span");
        assert_eq!(span.decision, Decision::Unknown("maybe".to_owned()));
        assert!(!span.decision.real_call());
        assert!(!span.decision.hypothetical());
        assert!(!span.decision.refused());
        assert_eq!(log.unclassified(), 1);
        assert!(render_html(&log).contains("unknown decision"));
    }

    #[test]
    fn a_missing_decision_is_unclassified_rather_than_allowed() {
        let log = log(r#"{"session":"s","operation":"x"}"#);
        assert_eq!(log.spans().next().expect("a span").decision, Decision::Absent);
        assert_eq!(log.unclassified(), 1);
        assert_eq!(log.real_calls(), 0);
    }

    /// D004's table is lower case, the audit constants are upper case, and the
    /// two must classify alike.
    #[test]
    fn the_decision_vocabulary_is_read_in_either_case() {
        for (token, expected) in [
            ("allow", Decision::Allow),
            ("ALLOW", Decision::Allow),
            ("dryrun-refuse", Decision::DryRunRefuse),
            ("DRYRUN-REFUSE", Decision::DryRunRefuse),
        ] {
            assert_eq!(Decision::classify(&Field::Text(token.to_owned())), expected, "{token}");
        }
    }

    /// An unmeasured check is a failure, never a pass — so a line the console
    /// cannot read is counted and located, not quietly skipped.
    #[test]
    fn an_unreadable_line_is_counted_and_located() {
        let log = log("{\"session\":\"s\",\"decision\":\"allow\"}\nnot json at all\n[1,2]\n");
        assert_eq!(log.total(), 1);
        assert_eq!(log.unreadable.len(), 2);
        assert_eq!(log.unreadable[0].source.line, 2);
        assert_eq!(log.unreadable[1].source.line, 3);
        assert!(log.unreadable[1].why.contains("an array"));
        assert!(render_html(&log).contains("could not be read"));
    }

    #[test]
    fn a_key_outside_the_table_is_named_rather_than_dropped() {
        let log = log(r#"{"session":"s","decision":"allow","duration_ms":42}"#);
        assert_eq!(log.extra_keys(), vec!["duration_ms".to_owned()]);
        let html = render_html(&log);
        assert!(html.contains("outside D004"), "{html}");
        assert!(html.contains("duration_ms"), "the key is named");
        assert!(!html.contains(">42<"), "its value is not rendered as a measurement");
    }

    /// D004 fixes no duration field and says why. The console invents none.
    #[test]
    fn no_view_here_shows_a_duration() {
        let html = render_html(&fixture());
        for invented in ["elapsed", "latency", "took ", " ms", "µs", "seconds"] {
            assert!(!html.contains(invented), "invented a measurement: {invented}");
        }
    }

    #[test]
    fn blank_lines_are_not_records_and_not_failures() {
        let log = log("\n\n{\"session\":\"s\",\"decision\":\"allow\"}\n\n");
        assert_eq!(log.total(), 1);
        assert!(log.unreadable.is_empty());
    }
}
