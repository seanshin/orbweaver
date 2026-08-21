//! S4 — the validation gate.
//!
//! `docs/PLAN.md` §5 calls this the safety belt of the whole system, and the
//! reason is an asymmetry: **an LLM writes plausible IDL that may be
//! semantically wrong; a deterministic checker rejects wrong IDL every time
//! without exception.** Everything upstream of S4 is allowed to be uncertain
//! because S4 is not. Remove it and the trust model has nothing left holding it
//! up.
//!
//! # Diagnostics are the product
//!
//! §3.3: the self-repair loop is only as good as the messages it feeds on, so
//! error quality is a *tested feature* rather than a nicety. A diagnostic here
//! carries a stable rule name, a source span, and — where the rule permits one
//! — a concrete fix, phrased as an edit rather than as a complaint.
//!
//! Phase 0 measured what this is worth. Twenty generated files produced seven
//! failures sharing one root cause, and one message that names the cause turned
//! 65% into 100% in a single round. The message is the mechanism.
//!
//! # What S4 takes: text, and where the text came from
//!
//! A contract is not only its bytes. A quoted `#include "x.idl"` resolves
//! against the directory the including file lives in, so a gate handed nothing
//! but a `&str` is judging a *smaller* contract than the one on disk — or, when
//! the include is loud rather than silent, refusing a contract that is fine.
//!
//! So the unit S4 takes is a [`Source`]: the text, plus the file it was read
//! from **when there was one**. Both halves are load-bearing. [`Source::from_file`]
//! is what lets `forge-pipeline --only s4` be pointed at a directory of legacy
//! contracts that include each other; [`Source::anonymous`] is what a model
//! writing IDL into a pipe honestly is, and it keeps failing an unresolvable
//! include with the reason — *"this source was supplied as text, not read from
//! a file"*. Guessing a directory for it would make one contract mean different
//! things depending on where the validator ran.
//!
//! # What it does not do
//!
//! It does not call an external compiler. The differential oracles
//! (`spikes/differential.sh`) are a separate check with a separate purpose:
//! they tell us whether *we* are right. This gate tells a generator whether its
//! output is, and has to run in-process, in milliseconds, thousands of times.
//!
//! # The rest of the crate
//!
//! S4 is the gate; the stages around it each own a producer and a gate of their
//! own, so a failure can be attributed to the stage that caused it:
//!
//! | Module | Stage | In → out | Its own gate |
//! |---|---|---|---|
//! | [`ingest`] | S1 | requirement text → [`ingest::Brief`] | is it a brief S2 can work from |
//! | [`synthesize`] | S2 | brief → `.idl` | [`validate`], plus: is anything callable, did the brief survive |
//! | [`annotate`] | S3 | `.idl` → SIDL | is every operation annotated, is every mutating operation scoped |
//! | [`infer`] | S3i | an ingested [`Registry`] → SIDL **proposals** | is every claim marked, evidenced, and refused where the evidence is silent |
//! | [`pipeline`] | — | the §5.1 loop over any one of them, and over all of them | — |
//!
//! S3 and S3i are alternatives, never both: one annotates IDL a model wrote
//! from a brief we hold, the other proposes annotations for an interface a
//! foreign Interface Repository described to us. The second cannot produce
//! facts, only claims, and [`infer`] is mostly the machinery that keeps the
//! difference legible everywhere a claim travels.
//!
//! Each is runnable alone, because a pipeline that only runs end to end can
//! tell you the output is wrong and not which stage was wrong.

#![deny(missing_docs)]

pub mod annotate;
pub mod infer;
pub mod ingest;
pub mod pipeline;
pub mod synthesize;

use std::collections::BTreeMap;
use std::path::Path;

use orbweaver_dynamic::json::Json;
use orbweaver_idl::{Diagnostic, SearchPath};
use orbweaver_registry::Registry;
use orbweaver_registry::diff::{Verdict, diff};

/// How much a finding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth saying; the file is still usable.
    Advice,
    /// The file compiles but something downstream will suffer.
    Warning,
    /// The file is not usable.
    Error,
}

impl Severity {
    /// The lowercase name used in JSON and in messages.
    pub fn label(self) -> &'static str {
        match self {
            Severity::Advice => "advice",
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }
}

/// Which wire S4 judges a contract for — the one decision that changes a
/// finding's severity rather than its text.
///
/// `docs/PLAN.md` §4.4 defers `valuetype`, abstract interfaces and `fixed`:
/// the parser accepts them (it must — `omniidl` does, and agreement with the
/// oracle is what the front end's correctness means), the wire does not carry
/// them, and `orbweaver-gen` skips every item that reaches one. Until this
/// existed the three facts never met: a contract using them passed S4 and was
/// unservable, and the only place that said so was the generator's skip list.
///
/// **Warning by default, error under [`WireGate::V1`].** The default has to be
/// the warning, because S4's acceptance criterion over `corpus/golden/` is the
/// oracle's, and golden 20 and 21 exist precisely to pin that the deferred
/// constructs *parse* — a gate that refused them by default would fail the
/// harness on IDL both oracles accept, or push those two files out of the
/// directory whose sweeps (generation, property, DynAny) are the only place
/// their skips are counted. But the *pipeline* is a different caller: what it
/// gates is a contract a model just wrote for this ORB, and for that caller
/// "valid, with a note" is the wrong answer — the repair loop should be told
/// to model the amount as a string now, not after generation skips it. So
/// [`pipeline::ValidateStage`] gates for `V1` unless told otherwise, and
/// `sidl-validate --wire v1` is the same form at the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WireGate {
    /// The v1 wire: a declaration that reaches a §4.4 construct is refused
    /// ([`Severity::Error`], rule [`orbweaver_idl::DEFERRED_WIRE_RULE`]).
    V1,
    /// The decision is deferred along with the constructs: the same findings
    /// as warnings, and the file passes. The library default.
    #[default]
    Deferred,
}

impl WireGate {
    /// The command-line spelling, `v1` or `deferred`.
    pub fn parse(text: &str) -> Option<WireGate> {
        match text {
            "v1" => Some(WireGate::V1),
            "deferred" => Some(WireGate::Deferred),
            _ => None,
        }
    }

    /// The severity a §4.4 finding takes under this gate.
    pub fn severity(self) -> Severity {
        match self {
            WireGate::V1 => Severity::Error,
            WireGate::Deferred => Severity::Warning,
        }
    }
}

/// One thing wrong, in the form a generator can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Stable identifier for the rule, so tooling can group or suppress.
    pub rule: String,
    /// How much it matters.
    pub severity: Severity,
    /// What is wrong.
    pub message: String,
    /// 1-based line, or 0 when the finding is about the file as a whole.
    pub line: u32,
    /// 1-based column.
    pub column: u32,
    /// The source text the span covers, so a model need not re-derive it.
    pub source: String,
    /// A concrete edit, where the rule admits one.
    pub fix: Option<String>,
}

impl Finding {
    /// The finding rendered with `place` in front of it — and **no second
    /// position**.
    ///
    /// A caller holding nothing but the [`Report`] has only `line:column`, and
    /// that is what [`Display`](std::fmt::Display) writes. A caller that
    /// resolved the includes holds the *file* as well, and its prefix has to
    /// replace this one rather than sit in front of it: `sidl-validate` printed
    /// `{located}: {finding}` and every diagnostic it ever emitted carried its
    /// position twice — `evo-proposed.idl:1:0: 0:0: error: …`, where the second
    /// pair is a line in a splice nobody has. One renderer, one position.
    ///
    /// `place` is whatever the caller can point at: `file:line:column`, or just
    /// the file when the finding is about the file as a whole (`line == 0`) and
    /// there is no line to name.
    ///
    /// *위치는 한 번만 찍는다. 렌더러가 하나이기 때문이다.*
    pub fn rendered_at(&self, place: &str) -> String {
        let mut s = format!("{place}: {}: {} [{}]", self.severity.label(), self.message, self.rule);
        if let Some(fix) = &self.fix {
            s.push_str(&format!("\n    fix: {fix}"));
        }
        s
    }
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.rendered_at(&format!("{}:{}", self.line, self.column)))
    }
}

/// Everything S4 found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// In source order, then by severity.
    pub findings: Vec<Finding>,
}

impl Report {
    /// Whether the file passed.
    pub fn is_ok(&self) -> bool {
        !self.findings.iter().any(|f| f.severity == Severity::Error)
    }

    /// The JSON form, for a caller that will hand it to a model.
    pub fn to_json(&self) -> Json {
        let findings: Vec<Json> = self
            .findings
            .iter()
            .map(|f| {
                let mut m: BTreeMap<String, Json> = BTreeMap::from([
                    ("rule".into(), Json::String(f.rule.clone())),
                    ("severity".into(), Json::String(f.severity.label().into())),
                    ("message".into(), Json::String(f.message.clone())),
                    ("line".into(), Json::Number(f.line.to_string())),
                    ("column".into(), Json::Number(f.column.to_string())),
                    ("source".into(), Json::String(f.source.clone())),
                ]);
                if let Some(fix) = &f.fix {
                    m.insert("fix".into(), Json::String(fix.clone()));
                }
                Json::Object(m)
            })
            .collect();
        Json::Object(BTreeMap::from([
            ("ok".into(), Json::Bool(self.is_ok())),
            ("findings".into(), Json::Array(findings)),
        ]))
    }

    /// The text to hand back to a generator, verbatim.
    ///
    /// Ordered by rule rather than by line, and each rule stated once with its
    /// occurrences listed under it. This is the operating model in miniature:
    /// Phase 0's seven failures were one cause, and a list of seven line
    /// numbers invites seven separate patches while a list of one cause invites
    /// the fix. The grouping is the advice.
    pub fn repair_prompt(&self) -> String {
        if self.is_ok() {
            return "The IDL is valid. No changes are needed.".into();
        }
        let mut by_rule: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
        for f in self.findings.iter().filter(|f| f.severity == Severity::Error) {
            by_rule.entry(&f.rule).or_default().push(f);
        }

        let mut out = String::from(
            "The IDL was rejected. Fix every occurrence of each cause below and return the \
             whole file.\n",
        );
        for (rule, findings) in &by_rule {
            out.push_str(&format!(
                "\n[{rule}] {} occurrence(s)\n  {}\n",
                findings.len(),
                findings[0].message
            ));
            if let Some(fix) = &findings[0].fix {
                out.push_str(&format!("  {fix}\n"));
            }
            for f in findings {
                out.push_str(&format!("    line {}, column {}: {}\n", f.line, f.column, f.source));
            }
        }
        out
    }
}

/// IDL text together with the file it was read from, when it was read from one.
///
/// A quoted `#include "x.idl"` resolves against **the including file's own
/// directory** first; that is the C convention CORBA inherits and what
/// `omniidl -I` implements. Text carries no directory, so an entry point that
/// takes only a `&str` cannot resolve the quoted form at all — and that is the
/// shape of the defect this type exists to close. `forge-pipeline` supplied
/// S4's item as text while holding the path it had just read it from, so the
/// thirteen-file estate was refused thirteen times for an include the caller
/// could have resolved.
///
/// **`origin: None` is an answer, not a gap.** A model that writes IDL into a
/// pipe genuinely has no directory, and the diagnostic says exactly that —
/// *"this source was supplied as text, not read from a file"* — instead of
/// resolving against the process's working directory, which would make one
/// contract mean different things depending on where the validator was invoked.
/// So [`Source::anonymous`] is not a degraded [`Source::from_file`]; it is the
/// truthful description of a different input, and it still fails.
///
/// *텍스트에는 디렉터리가 없다. 출처를 함께 넘기거나, 없다고 정직하게 말한다 —
/// 추측해서 해석하지 않는다.*
#[derive(Debug, Clone, Copy, Default)]
pub struct Source<'a> {
    text: &'a str,
    origin: Option<&'a Path>,
}

impl<'a> Source<'a> {
    /// IDL that came from nowhere on disk: a model wrote it, or a caller built
    /// it. Only an absolute `#include` or the search path can resolve.
    pub fn anonymous(text: &'a str) -> Source<'a> {
        Source { text, origin: None }
    }

    /// IDL and the file it was read from.
    pub fn from_file(text: &'a str, origin: &'a Path) -> Source<'a> {
        Source { text, origin: Some(origin) }
    }

    /// IDL and the file it was read from, where the caller may or may not have
    /// one — the shape a pipeline stage is in, since an item may be either.
    pub fn maybe_from_file(text: &'a str, origin: Option<&'a Path>) -> Source<'a> {
        Source { text, origin }
    }

    /// The IDL itself.
    pub fn text(&self) -> &'a str {
        self.text
    }

    /// The file it was read from, if there was one.
    pub fn origin(&self) -> Option<&'a Path> {
        self.origin
    }
}

/// Runs every in-process check over one file.
///
/// The string entry point: no origin and no search path, so a `#include` here
/// resolves only if it is absolute, and one that does not is **reported** with
/// the reason rather than skipped. Callers that know the path want
/// [`validate_source`]; this one stays because text with no origin is a real
/// input and deserves a real answer.
pub fn validate(src: &str) -> Report {
    validate_source(Source::anonymous(src), &SearchPath::new())
}

/// S4 over a source that carries where it came from.
///
/// Resolves `#include` first — the quoted form against the origin's own
/// directory, the angled form against `search` — and then runs every check over
/// the resolved unit. Positions come back mapped to the file the line was
/// written in, because a line number in the splice is a line number nobody can
/// open.
///
/// With [`Source::anonymous`] this is byte-for-byte the old [`validate`]: same
/// unit, same diagnostics, same refusal for an unresolvable include.
pub fn validate_source(src: Source<'_>, search: &SearchPath) -> Report {
    validate_source_for(src, search, WireGate::Deferred)
}

/// [`validate_source`], judging for a chosen wire — see [`WireGate`].
pub fn validate_source_for(src: Source<'_>, search: &SearchPath, wire: WireGate) -> Report {
    let unit = orbweaver_idl::preprocess(src.text, src.origin, search);
    let mut report = validate_unit_for(&unit, wire);
    locate_findings(&mut report, &unit);
    report
}

/// Where one finding was written, or `None` when it is about the file as a
/// whole.
///
/// The `None` is the whole point of this function existing rather than each
/// caller reaching for [`Unit::locate`](orbweaver_idl::include::Unit::locate)
/// directly. `locate` maps a **span**, and a span's line is 1-based, so it
/// clamps with `.max(1)`; a [`Finding`] is not a span, and `line == 0` is its
/// documented "about the file as a whole" — every `evolution/*` finding is one.
/// Fed to `locate`, that 0 became line 1 of the root, a position nothing was
/// written at. [`locate_findings`] had always skipped line 0 for exactly this
/// reason and `sidl-validate`'s printer, written earlier, had not; two callers
/// of one rule is how they drift, so there is now one.
///
/// *0번 줄은 위치가 아니라 "파일 전체"라는 뜻이다. 스팬으로 넘기면 1번 줄이 된다.*
pub fn written_in<'a>(
    finding: &Finding,
    unit: &'a orbweaver_idl::include::Unit,
) -> Option<orbweaver_idl::include::Location<'a>> {
    if finding.line == 0 {
        return None;
    }
    Some(unit.locate(orbweaver_idl::lex::Span {
        start: 0,
        end: 0,
        line: finding.line,
        column: finding.column,
    }))
}

/// Maps every finding's position back to the file its line was written in.
///
/// A resolved unit is several files spliced together, so an unmapped line
/// number points into a document that exists nowhere. §3.3 hands these
/// diagnostics straight back to a generator, and a confident wrong position is
/// worse than none — so a finding written in an *included* file also says which
/// file, with the include chain that reached it.
///
/// A no-include unit is byte-identical to its input and this is the identity.
///
/// Public because the *binary* has to do exactly this and used not to. Only the
/// human printer of `sidl-validate` mapped anything; `--json` and
/// `--repair-prompt` served the splice's line under the root file's name, so a
/// machine reader — and S4's self-repair loop is one — was told to edit line 8
/// of a file whose line 8 is somebody else's declaration. A caller that
/// resolved the unit itself calls this; the [`validate_source`] family calls it
/// for callers that did not.
pub fn locate_findings(report: &mut Report, unit: &orbweaver_idl::include::Unit) {
    let Some(root) = unit.files.first() else { return };
    for finding in &mut report.findings {
        // Line 0 means "about the file as a whole"; there is no position to map.
        let Some(at) = written_in(finding, unit) else { continue };
        if at.file != root.as_path() {
            let mut chain = String::new();
            for (file, line) in at.chain.iter().rev() {
                chain.push_str(&format!(", included from {}:{}", file.display(), line));
            }
            finding.message = format!(
                "{} (written in {}:{}{chain})",
                finding.message,
                at.file.display(),
                at.line
            );
        }
        finding.line = at.line;
        finding.column = at.column;
    }
}

/// S4 over an already-resolved translation unit.
///
/// The string form is the whole corpus's shape — one self-contained file — and
/// nothing noticed for six phases, because no corpus file has ever had an
/// `#include`. A real estate is a directory, and re-checking a resolved unit's
/// *text* refuses it: the guard directives of thirteen files are still in
/// there, and four `#ifndef` blocks in one string is conditional compilation
/// rather than an include guard. So a caller that has resolved the includes
/// hands the unit in instead of handing the text back.
pub fn validate_unit(unit: &orbweaver_idl::include::Unit) -> Report {
    validate_unit_for(unit, WireGate::Deferred)
}

/// [`validate_unit`], judging for a chosen wire — see [`WireGate`].
pub fn validate_unit_for(unit: &orbweaver_idl::include::Unit, wire: WireGate) -> Report {
    validate_checked(&unit.text, orbweaver_idl::check_unit(unit), wire)
}

fn validate_checked(
    src: &str,
    checked: std::result::Result<orbweaver_idl::ast::Spec, Vec<orbweaver_idl::Diagnostic>>,
    wire: WireGate,
) -> Report {
    let mut findings = Vec::new();

    match checked {
        Err(diags) => {
            // Parsing and semantics have already run; nothing downstream can
            // say anything trustworthy about a file that failed them.
            findings.extend(diags.iter().map(|d| from_diagnostic(src, d)));
        }
        Ok(spec) => {
            let mut registry = Registry::new();
            if let Err(e) = registry.load(&spec) {
                findings.push(Finding {
                    rule: "registry".into(),
                    severity: Severity::Error,
                    message: e.message,
                    line: 0,
                    column: 0,
                    source: String::new(),
                    fix: None,
                });
            }
            findings.extend(explicit_ids(&spec));
            findings.extend(wire_support(src, &spec, wire));
            findings.extend(annotation_advice(src, &spec));
        }
    }

    findings.sort_by(|a, b| {
        b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)).then(a.column.cmp(&b.column))
    });
    Report { findings }
}

/// Compares against a released contract as well, so S4 covers §5.3.
///
/// A file can be perfectly valid and still be a change nobody may ship, and a
/// generator asked to "add a field" will produce exactly that. Reporting it
/// here rather than at release time is the difference between a regenerate and
/// an outage.
pub fn validate_against(src: &str, released: &str) -> Report {
    validate_source_against(Source::anonymous(src), Source::anonymous(released), &SearchPath::new())
}

/// [`validate_against`], judging for a chosen wire — see [`WireGate`].
pub fn validate_against_for(src: &str, released: &str, wire: WireGate) -> Report {
    validate_source_against_for(
        Source::anonymous(src),
        Source::anonymous(released),
        &SearchPath::new(),
        wire,
    )
}

/// Says, in the report, that the §5.3 comparison did not run.
///
/// The failure modes below all used to `return report` — the proposal's own
/// clean verdict, handed back with no mention that the diff never happened.
/// Every caller then read it as "compared, and nothing breaks".
fn never_compared(mut report: Report, why: String) -> Report {
    report.findings.push(Finding {
        rule: RELEASED_UNREADABLE.into(),
        severity: Severity::Error,
        message: format!(
            "the §5.3 comparison against the released contract never ran: {why}. An unmeasured \
             check is a failure, never a pass"
        ),
        line: 0,
        column: 0,
        source: String::new(),
        fix: Some(
            "fix the released contract, or point the comparison at the revision that was \
             actually released"
                .into(),
        ),
    });
    report.findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)));
    report
}

/// The rule a §5.3 comparison reports when the *baseline* is what stopped it.
///
/// A released contract that will not resolve, parse or load is not a clean
/// diff; it is no diff. The old code returned the proposal's report unchanged
/// and every caller read that as "compared, and nothing broke" — an unmeasured
/// check reported as a pass, which is the one thing the harness rules forbid.
pub const RELEASED_UNREADABLE: &str = "evolution/released-unreadable";

/// [`validate_against`] where both sides carry where they came from.
///
/// Both sides, deliberately. A released contract is a path too, and a baseline
/// read as a string loses every name its headers declared — so comparing a
/// resolved proposal against an unresolved baseline reports the whole shared
/// header as *newly added*. Resolving one side only would have been a worse
/// gate than resolving neither.
pub fn validate_source_against(
    proposed: Source<'_>,
    released: Source<'_>,
    search: &SearchPath,
) -> Report {
    validate_source_against_for(proposed, released, search, WireGate::Deferred)
}

/// [`validate_source_against`], judging for a chosen wire — see [`WireGate`].
///
/// The gate applies to the *proposal* only. The released contract is not
/// re-judged: it shipped, and whatever it carries is what it carries.
pub fn validate_source_against_for(
    proposed: Source<'_>,
    released: Source<'_>,
    search: &SearchPath,
    wire: WireGate,
) -> Report {
    let unit = orbweaver_idl::preprocess(proposed.text, proposed.origin, search);
    let released_unit = orbweaver_idl::preprocess(released.text, released.origin, search);
    let mut report = validate_unit_against_for(&unit, &released_unit, wire);
    locate_findings(&mut report, &unit);
    report
}

/// The §5.3 comparison over two **already-resolved** translation units.
///
/// The unit form exists for the same reason [`validate_unit`] does, and it is
/// the same defect one comparison later. A caller that has resolved both sides
/// — `sidl-validate --against` had, twice, for its own error reporting — used
/// to hand the two [`Unit::text`](orbweaver_idl::include::Unit)s back to
/// [`validate_against`], which preprocessed each *splice* a second time. A
/// splice is not a file: it holds the `#ifndef` of every file it contains, and
/// a guard that is not the first directive of the text it sits in is
/// conditional compilation, which this front end refuses on purpose. So a
/// guarded multi-file contract — the ordinary shape of a released contract —
/// was refused with `unsupported-directive` on a header's line 1 and the §5.3
/// comparison never ran at all.
///
/// Positions are **not** mapped here, exactly as [`validate_unit_for`] does not
/// map them: the caller holds the unit that does the mapping and there must not
/// be two conventions for who owns a position. [`validate_source_against_for`]
/// resolves both sides and then maps; a caller passing units maps with
/// [`Unit::locate`](orbweaver_idl::include::Unit::locate) itself.
///
/// The gate applies to the *proposal* only, as in
/// [`validate_source_against_for`].
///
/// *스플라이스는 파일이 아니다. 이미 해석된 쪽은 유닛끼리 비교한다.*
pub fn validate_unit_against(
    proposed: &orbweaver_idl::include::Unit,
    released: &orbweaver_idl::include::Unit,
) -> Report {
    validate_unit_against_for(proposed, released, WireGate::Deferred)
}

/// [`validate_unit_against`], judging for a chosen wire — see [`WireGate`].
pub fn validate_unit_against_for(
    proposed: &orbweaver_idl::include::Unit,
    released: &orbweaver_idl::include::Unit,
    wire: WireGate,
) -> Report {
    let mut report = validate_unit_for(proposed, wire);
    if !report.is_ok() {
        return report;
    }
    let old_spec = match orbweaver_idl::check_unit(released) {
        Ok(spec) => spec,
        Err(diags) => {
            let why = diags
                .first()
                .map(|d| released.render(d))
                .unwrap_or_else(|| "it did not check out".to_owned());
            return never_compared(report, why);
        }
    };
    // `report.is_ok()` above means the proposal already checked out; matched
    // rather than unwrapped so a future divergence is a finding, not a panic.
    let Ok(new_spec) = orbweaver_idl::check_unit(proposed) else {
        return never_compared(report, "the proposal stopped checking out".to_owned());
    };
    let (mut old, mut new) = (Registry::new(), Registry::new());
    if let Err(e) = old.load(&old_spec) {
        return never_compared(report, e.message);
    }
    if let Err(e) = new.load(&new_spec) {
        return never_compared(report, e.message);
    }
    for change in diff(&old, &new) {
        let severity = match change.verdict {
            Verdict::Breaking => Severity::Error,
            Verdict::ConditionallyBreaking => Severity::Warning,
            Verdict::ServerFirst => Severity::Advice,
            Verdict::Compatible => continue,
        };
        report.findings.push(Finding {
            rule: format!("evolution/{}", change.verdict.label().replace(' ', "-")),
            severity,
            message: format!("{}: {} — {}", change.id, change.what, change.why),
            line: 0,
            column: 0,
            source: change.id.clone(),
            fix: match change.verdict {
                Verdict::Breaking => Some(
                    "publish a new version of the interface instead of editing the released \
                     type in place (docs/PLAN.md §5.3)"
                        .into(),
                ),
                _ => None,
            },
        });
    }
    report.findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)));
    report
}

fn from_diagnostic(src: &str, d: &Diagnostic) -> Finding {
    Finding {
        rule: d.rule.to_owned(),
        severity: Severity::Error,
        message: d.message.clone(),
        line: d.span.line,
        column: d.span.column,
        source: src.get(d.span.start..d.span.end).unwrap_or_default().to_owned(),
        fix: fix_for(d, src),
    }
}

/// The concrete edit, for the rules that admit one.
///
/// Only where the fix is unambiguous. A hint that guesses wrong is worse than
/// none: the loop feeds it back verbatim, and a confident wrong instruction is
/// what turns one round into three.
fn fix_for(d: &Diagnostic, src: &str) -> Option<String> {
    let text = src.get(d.span.start..d.span.end).unwrap_or_default();
    match d.rule {
        // The dominant failure of this whole project, measured in Phase 0 and
        // met five times since. The fix is always the same shape, so it is
        // always worth spelling out.
        "identifier-case-clash" | "enclosing-scope-clash" | "inherited-clash" => Some(format!(
            "IDL identifiers collide case-insensitively (CORBA 3.4 §7.2.3). Rename {text:?} \
             to something that does not match a type or an enclosing scope ignoring case — \
             `the_{text}`, `{text}_value` or a domain word all work. Renaming the *type* is \
             usually wrong: it is the one other files refer to."
        )),
        "unknown-name" => Some(format!(
            "{text:?} is not declared anywhere in scope. Declare it, qualify it with its \
             module (`Module::{text}`), or correct the spelling."
        )),
        "not-a-type" => Some(format!(
            "{text:?} names something that is not a type. A constant or an operation cannot \
             be used where a type is expected."
        )),
        "duplicate-declaration" => {
            Some(format!("{text:?} is already declared in this scope; remove one or rename it."))
        }
        "duplicate-union-label" => {
            Some("Each union case label may appear once; remove the repeat.".into())
        }
        "duplicate-union-default" => Some("A union has at most one `default:` branch.".into()),
        // A signature's grammar (`param_type_spec`) is narrower than a
        // declaration's (`type_spec`), and the edit is always the same shape:
        // give the type a name first. `corpus/negative/n13`–`n16`.
        "anonymous-type-in-signature" => Some(format!(
            "`{text}` is a template type. IDL's param_type_spec admits base types, `string`, \
             `wstring` and a scoped name — a `sequence` or a `fixed` reaches an attribute, a \
             parameter or a return only through a name. Declare `typedef {text} <Name>;` \
             outside the interface and write `<Name>` here."
        )),
        // `const_type` is narrower than `type_spec` in both directions at once,
        // and the edit is unambiguous either way: drop the bounds, or name a
        // type that can hold a literal. `corpus/negative/n18`.
        "not-a-const-type" => Some(format!(
            "`{text}` is not a const_type (CORBA 3.4 §7.4.1.4.2 admits the integer types, \
             `char`, `wchar`, `boolean`, the floating-point types, `octet`, `string`, \
             `wstring`, bare `fixed` and a scoped name). A fixed constant is written \
             `const fixed NAME = 9.9d;` — the digits and scale come from the value, so the \
             type takes no `<d,s>`. For anything else, declare the type with a `typedef` and \
             name it here, or give the constant a type that can hold a literal."
        )),
        // A constant's value must be a literal of its own type, with no
        // conversion at all — omniidl converts nothing here, measured across
        // sixteen pairs. The edit is unambiguous because the message already
        // names the class that was wanted and how to write one.
        // `corpus/negative/n19`.
        "const-value-type" => Some(format!(
            "The constant {text:?} has a value of the wrong class. IDL performs no conversion \
             in a constant initialiser: `const double D = 5;` is an error and `5.0` is the \
             fix, as are `9.9d` for a `fixed`, `'a'` for a `char`, `L'a'` for a `wchar`, \
             `\"s\"` for a `string`, `L\"s\"` for a `wstring` and `TRUE`/`FALSE` for a \
             `boolean`. Width is not the axis — `char` and `octet` are both one octet and \
             neither takes the other's literal. Rewrite the literal in the constant's own \
             class, or change the constant's type to the class the value already has."
        )),
        // Range and divide-by-zero. Both edits are the same shape: the value
        // has to change, or the type does. `corpus/negative/n20`.
        "const-value-range" => Some(format!(
            "The constant {text:?} has a value its declared type cannot hold. Widen the \
             type — `short` to `long`, `long` to `long long`, or the signed form to the \
             unsigned one if the value is never negative — or write a value inside the \
             range the message names. Do not truncate it: a constant is part of the \
             contract, and a consumer that reads a wrapped number reads one nobody wrote."
        )),
        // The literal itself is malformed, and unlike most lexical failures the
        // offending text is exactly the thing to edit. `corpus/negative/n22`.
        "fixed-literal" => Some(format!(
            "`{text}` is not a fixed-point literal. CORBA 3.4 §7.2.6.5 spells one as digits, \
             an optional point, more digits and a `d` — there is no exponent production, so \
             write `1000d` rather than `1e3d` (or drop the `d` and let it be a `double`). \
             §7.11.3 caps it at 31 significant digits; a longer one has to lose digits \
             deliberately, because rounding it silently would change the constant's value."
        )),
        "void-in-signature" => Some(format!(
            "`{text}` is a return type and nothing else: `op_type_spec` names it and \
             `param_type_spec` does not. Give the attribute or parameter the type its value \
             actually has."
        )),
        // Escaping is the whole fix and IDL 4 §7.2.3.1 defines it precisely, so
        // this is one of the few parse failures worth a hint.
        "reserved-word" => Some(format!(
            "Prefix it: `_{text}`. IDL reserves the word, and the leading underscore is the \
             escape the specification defines — it is not part of the name and does not appear \
             in the repository id. Renaming works too."
        )),
        // Every other parse failure keeps quiet. The cause is wherever the
        // grammar broke, which is not reliably where the token is, and a
        // confident wrong hint costs a self-repair round.
        _ => None,
    }
}

/// An explicit `#pragma ID` that is not a well-formed repository id.
///
/// A repository id is identity: `_is_a`, the IFR facade, ingestion's matching,
/// an IOR's `type_id` and the exposure allowlist all key on it. `#pragma ID`
/// lets a file set one directly, and nothing checked what it set — we accepted
/// `not an idl id at all` in silence and put it on the wire as the identity of
/// an interface.
///
/// **Warning, not error, and the oracle decides that.** omniidl accepts the
/// same file with `Warning: Repository id of 'I' set to invalid string`, so
/// refusing it would reject IDL a deployed compiler compiles — the divergence
/// this project records rather than creates. Saying nothing, though, is worse
/// than either: the id travels, and the peer that disagrees with it is the one
/// who finds out.
///
/// The form checked is the one the specification gives, `IDL:<path>:<major>.<minor>`,
/// deliberately permissive about the path's first segment because a prefix is
/// a domain name and carries dots. Other schemes (`RMI:`, `DCE:`) are reported
/// as unrecognised rather than malformed: they are real, and we do not
/// implement them.
fn explicit_ids(spec: &orbweaver_idl::ast::Spec) -> Vec<Finding> {
    let mut out = Vec::new();
    for (name, id) in &spec.repository_ids {
        let complaint = if let Some(rest) = id.strip_prefix("IDL:") {
            match rest.rsplit_once(':') {
                None => Some("it has no `:<major>.<minor>` version suffix".to_owned()),
                Some((path, version)) => {
                    let bad_version = version.split_once('.').is_none_or(|(maj, min)| {
                        maj.is_empty()
                            || min.is_empty()
                            || !maj.bytes().all(|b| b.is_ascii_digit())
                            || !min.bytes().all(|b| b.is_ascii_digit())
                    });
                    if bad_version {
                        Some(format!("its version {version:?} is not `<major>.<minor>`"))
                    } else if path.is_empty() {
                        Some("its path is empty".to_owned())
                    } else if path.split('/').any(str::is_empty) {
                        Some("its path has an empty segment".to_owned())
                    } else {
                        None
                    }
                }
            }
        } else if id.starts_with("RMI:") || id.starts_with("DCE:") {
            Some("it uses a scheme this project does not implement".to_owned())
        } else {
            Some("it does not start with `IDL:`".to_owned())
        };

        if let Some(why) = complaint {
            out.push(Finding {
                rule: "id/explicit-malformed".into(),
                severity: Severity::Warning,
                message: format!(
                    "#pragma ID sets the repository id of {name} to {id:?}, but {why}. \
                     A repository id is identity on the wire — `_is_a`, the repository \
                     facade, ingestion and the exposure allowlist all key on it — so a \
                     malformed one disagrees with every peer that derives its own",
                ),
                line: 0,
                column: 0,
                source: name.clone(),
                fix: Some(format!(
                    "set it to `IDL:<path>:<major>.<minor>`, or drop the pragma and let \
                     the id be derived from the scope of {name}"
                )),
            });
        }
    }
    out
}

/// The v1 wire's refusals, from the front end's closure (docs/PLAN.md §4.4).
///
/// The set is computed in `orbweaver-idl` — [`orbweaver_idl::deferred_wire_types`]
/// — because deciding it needs name resolution, and it is the same closure
/// `orbweaver-gen` computes when it skips: a struct with a `fixed` member, the
/// interface returning that struct, the interface inheriting that operation.
/// `orbweaver-gen`'s `deferred_wire_agreement` test holds the two sets equal.
/// This function only chooses the severity, which is [`WireGate`]'s.
///
/// The predecessor of this rule, `wire/valuetype`, named the valuetype and
/// stopped: `fixed` — the construct the golden corpus actually carries in a
/// signature — was reported by nothing at S4, and an interface *returning* a
/// valuetype was reported nowhere at all.
fn wire_support(src: &str, spec: &orbweaver_idl::ast::Spec, wire: WireGate) -> Vec<Finding> {
    orbweaver_idl::deferred_wire_types(spec)
        .iter()
        .map(|d| Finding {
            rule: orbweaver_idl::DEFERRED_WIRE_RULE.into(),
            severity: wire.severity(),
            message: d.message(),
            line: d.span.line,
            column: d.span.column,
            source: src.get(d.span.start..d.span.end).unwrap_or_default().to_owned(),
            fix: Some(d.fix()),
        })
        .collect()
}

/// What a contract needs before an agent can use it (§2.2).
///
/// Advice, never an error: an unannotated interface is perfectly valid CORBA.
/// It is simply unusable by something that has never seen it, which is the
/// whole point of the pipeline, so the gate says so.
fn annotation_advice(src: &str, spec: &orbweaver_idl::ast::Spec) -> Vec<Finding> {
    let mut out = Vec::new();
    walk(&spec.definitions, &mut |def| {
        let orbweaver_idl::ast::Definition::Interface(i) = def else { return };
        // A forward declaration has no body and nothing to annotate.
        let Some(body) = &i.body else { return };
        for op in body.iter().filter_map(|m| match m {
            orbweaver_idl::ast::InterfaceMember::Operation(op) => Some(op),
            _ => None,
        }) {
            if !op.annotations.iter().any(|a| a.key == "ai_desc") {
                out.push(Finding {
                    rule: "sidl/missing-ai_desc".into(),
                    severity: Severity::Advice,
                    message: format!(
                        "{}.{} has no ai_desc, so an agent has only the name to go on",
                        i.name.text, op.name.text
                    ),
                    line: op.name.span.line,
                    column: op.name.span.column,
                    source: src
                        .get(op.name.span.start..op.name.span.end)
                        .unwrap_or_default()
                        .to_owned(),
                    fix: Some(format!(
                        "add `//@ ai_desc: <what {} does, in one sentence>` above it",
                        op.name.text
                    )),
                });
            }
            if !op.annotations.iter().any(|a| a.key == "ai_effect") {
                out.push(Finding {
                    rule: "sidl/missing-ai_effect".into(),
                    severity: Severity::Advice,
                    message: format!(
                        "{}.{} has no ai_effect, so the bridge refuses it: it cannot tell \
                         whether an agent may call this without a human. Annotate it, or set \
                         the exposure's --assume-effect",
                        i.name.text, op.name.text
                    ),
                    line: op.name.span.line,
                    column: op.name.span.column,
                    source: src
                        .get(op.name.span.start..op.name.span.end)
                        .unwrap_or_default()
                        .to_owned(),
                    fix: Some(
                        "add `//@ ai_effect: read_only`, `idempotent` or `destructive`".into(),
                    ),
                });
            }
        }
    });
    out
}

/// Whether `src` carries any SIDL annotation at all.
///
/// The question a run report actually wants when it says how many contracts
/// were annotated. It used to ask the *file name* — `.sidl.idl` meant
/// annotated — which is a fact about a rename, not about the file, and a
/// legacy estate renamed so a gate would find it was duly reported as
/// annotated while containing nothing of the kind. The name is a convention
/// for the pipeline's own artifacts; it is not evidence about a contract
/// somebody else wrote.
///
/// A file that will not parse is not annotated: it has not been shown to carry
/// anything, and reporting it as annotated would be the same lie in a new
/// place. S4 reports the parse failure separately, so nothing is hidden.
pub fn carries_annotations(src: &str) -> bool {
    let Ok(spec) = orbweaver_idl::parse(src) else { return false };
    let mut found = false;
    walk(&spec.definitions, &mut |def| {
        let orbweaver_idl::ast::Definition::Interface(i) = def else { return };
        let Some(body) = &i.body else { return };
        for m in body {
            let annotations = match m {
                orbweaver_idl::ast::InterfaceMember::Operation(op) => &op.annotations,
                orbweaver_idl::ast::InterfaceMember::Attribute(a) => &a.annotations,
                _ => continue,
            };
            if !annotations.is_empty() {
                found = true;
            }
        }
    });
    found
}

fn walk(
    defs: &[orbweaver_idl::ast::Definition],
    f: &mut impl FnMut(&orbweaver_idl::ast::Definition),
) {
    for d in defs {
        f(d);
        if let orbweaver_idl::ast::Definition::Module(m) = d {
            walk(&m.definitions, f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(src: &str) -> Vec<String> {
        validate(src).findings.iter().map(|f| f.rule.clone()).collect()
    }

    /// The question is about the contract, so it is asked of the contract.
    ///
    /// A thirteen-file legacy estate was renamed to `.sidl.idl` because that was
    /// the only name S4 would discover, and the run then reported it as an
    /// annotated file — true about the name, false about every line in it. Both
    /// halves are fixed; this pins the half that stops the *report* from
    /// depending on a rename.
    #[test]
    fn annotation_is_read_from_the_contract_and_not_from_its_name() {
        let bare = "module m { interface I { void f(); }; };";
        assert!(!carries_annotations(bare), "no annotation anywhere");

        let annotated = "module m { interface I {\n //@ ai_desc: does the thing\n void f(); }; };";
        assert!(carries_annotations(annotated));

        // An attribute's annotations count: they are what gates its accessors,
        // so a contract carrying only those is annotated.
        let attr = "module m { interface I {\n //@ ai_effect: read_only\n readonly attribute long n; }; };";
        assert!(carries_annotations(attr));

        // Unparseable is not annotated. It has not been shown to carry
        // anything, and claiming otherwise would be the same lie relocated.
        assert!(!carries_annotations("module m { interface"));
    }

    /// `#pragma ID` is a direct write to identity, and nothing checked it.
    /// omniidl warns on the same file rather than refusing, so we warn: this
    /// is a divergence to record, not one to create.
    #[test]
    fn an_explicit_id_that_is_not_a_repository_id_is_reported() {
        let r = validate(
            "module m { interface I { long ping(); };\n#pragma ID I \"not an idl id\"\n};",
        );
        let f = r.findings.iter().find(|f| f.rule == "id/explicit-malformed").expect("reported");
        assert_eq!(f.severity, Severity::Warning, "the oracle accepts it; we do not refuse");
        assert!(f.message.contains("not an idl id"), "{}", f.message);
        assert!(r.is_ok(), "a warning must not fail the gate");
    }

    /// The forms that are wrong in a way a reader would not spot: a missing
    /// version, a version that is not two numbers, an empty path segment.
    #[test]
    fn the_shape_of_a_repository_id_is_checked_not_just_its_prefix() {
        for bad in ["IDL:m/I", "IDL:m/I:1", "IDL:m/I:x.y", "IDL::1.0", "IDL:m//I:1.0"] {
            let src =
                format!("module m {{ interface I {{ long ping(); }};\n#pragma ID I \"{bad}\"\n}};");
            let r = validate(&src);
            assert!(
                r.findings.iter().any(|f| f.rule == "id/explicit-malformed"),
                "{bad} passed unreported"
            );
        }
    }

    /// A well-formed one — including a dotted prefix segment, which is what a
    /// `#pragma prefix` produces — must be silent, or the rule fires on every
    /// correct file and gets ignored.
    #[test]
    fn a_well_formed_explicit_id_is_silent() {
        let r = validate(
            "module m { interface I { long ping(); };\n#pragma ID I \"IDL:acme.com/m/I:2.3\"\n};",
        );
        assert!(!r.findings.iter().any(|f| f.rule == "id/explicit-malformed"), "{:?}", r.findings);
    }

    #[test]
    fn a_clean_file_passes_with_nothing_to_say_beyond_advice() {
        let r = validate(
            "module m {
               //@ ai_desc: Adds two numbers
               //@ ai_effect: read_only
               interface Calc { long add(in long a, in long b); };
             };",
        );
        assert!(r.is_ok(), "{:?}", r.findings);
        assert_eq!(r.repair_prompt(), "The IDL is valid. No changes are needed.");
    }

    /// The dominant failure. The message must name the identifier and the fix
    /// must be an edit, because this is the one the loop will see most.
    #[test]
    fn the_case_clash_finding_says_exactly_what_to_change() {
        let r = validate(
            "module m { struct Position { double x; }; struct T { Position position; }; };",
        );
        assert!(!r.is_ok());
        let f = r.findings.iter().find(|f| f.rule.contains("clash")).expect("clash reported");
        assert_eq!(f.source, "position", "the span must cover the offending name");
        let fix = f.fix.as_deref().expect("a fix");
        assert!(fix.contains("the_position") || fix.contains("position_value"), "{fix}");
        // And it must steer away from the wrong fix, which is renaming the type
        // every other file refers to.
        assert!(fix.contains("Renaming the *type* is usually wrong"), "{fix}");
    }

    #[test]
    fn every_semantic_rule_reaches_the_report() {
        for (src, rule) in [
            ("module m { struct S { long a; long a; }; };", "duplicate-declaration"),
            ("module m { struct S { Missing a; }; };", "unknown-name"),
            (
                "module m { union U switch (long) { case 1: long a; case 1: long b; }; };",
                "duplicate-union-label",
            ),
        ] {
            assert!(rules(src).iter().any(|r| r == rule), "{src} should report {rule}");
        }
    }

    /// The grouping is the advice: Phase 0's seven failures were one cause, and
    /// a list of seven line numbers invites seven patches.
    #[test]
    fn the_repair_prompt_groups_by_cause_not_by_line() {
        let src = "module m {
             struct Position { double x; };
             struct Value { long v; };
             struct A { Position position; };
             struct B { Value value; };
           };";
        let r = validate(src);
        let prompt = r.repair_prompt();
        assert_eq!(
            prompt.matches("[identifier-case-clash]").count(),
            1,
            "the cause is stated once:\n{prompt}"
        );
        assert!(prompt.contains("2 occurrence(s)"), "{prompt}");
        assert!(prompt.contains("position"), "{prompt}");
        assert!(prompt.contains("value"), "{prompt}");
    }

    #[test]
    fn the_json_form_is_machine_readable_and_round_trips() {
        let r = validate("module m { struct S { Missing a; }; };");
        let j = r.to_json();
        let text = j.to_string();
        assert_eq!(Json::parse(&text).unwrap(), j);
        assert_eq!(j.get("ok"), Some(&Json::Bool(false)));
        let Some(Json::Array(fs)) = j.get("findings") else { panic!("{text}") };
        assert!(fs[0].get("fix").is_some(), "{text}");
        assert!(fs[0].get("line").is_some(), "{text}");
    }

    /// A valuetype compiles and cannot go on this project's wire. Warning by
    /// default: it is legal IDL, and the file may exist for a v2 that can.
    #[test]
    fn a_valuetype_is_a_warning_that_names_the_limit() {
        let r = validate("module m { valuetype V { public long a; }; };");
        assert!(r.is_ok(), "still valid IDL");
        let f = r.findings.iter().find(|f| f.rule == "wire/deferred-type").expect("warned");
        assert_eq!(f.severity, Severity::Warning);
        assert!(f.message.contains("§4.4"));
        assert_eq!(f.source, "V", "the span is the declaration's name");
    }

    /// `corpus/golden/21`'s shape under both gates: the same three findings,
    /// the same text, and only the severity — and so the verdict — differs.
    /// The fix names the edit a generator can make.
    #[test]
    fn fixed_is_a_warning_by_default_and_a_refusal_for_the_v1_wire() {
        let src = "module m { typedef fixed<9,2> Amount; struct Invoice { Amount total; }; \
                   interface Billing { Amount sum(in Amount a, in Amount b); }; };";
        let lax = validate(src);
        assert!(lax.is_ok(), "{:?}", lax.findings);
        let strict = validate_source_for(Source::anonymous(src), &SearchPath::new(), WireGate::V1);
        assert!(!strict.is_ok());
        let pick = |r: &Report| {
            r.findings
                .iter()
                .filter(|f| f.rule == "wire/deferred-type")
                .map(|f| (f.source.clone(), f.message.clone(), f.fix.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(pick(&lax), pick(&strict), "only the severity may differ");
        assert_eq!(
            pick(&strict).iter().map(|(s, _, _)| s.as_str()).collect::<Vec<_>>(),
            ["Amount", "Invoice", "Billing"]
        );
        assert!(
            strict
                .findings
                .iter()
                .all(|f| f.severity == Severity::Error || f.rule != "wire/deferred-type")
        );
        let fix = pick(&strict)[0].2.clone().expect("a fix");
        assert!(fix.contains("string"), "{fix}");
        // The repair prompt groups the three under the one cause.
        let prompt = strict.repair_prompt();
        assert_eq!(prompt.matches("[wire/deferred-type]").count(), 1, "{prompt}");
        assert!(prompt.contains("3 occurrence(s)"), "{prompt}");
    }

    /// The strict form refuses only what reaches a §4.4 construct: a file
    /// with none is identical under both gates.
    #[test]
    fn the_v1_gate_changes_nothing_for_a_contract_the_wire_carries() {
        let src = "module m { struct S { long a; }; interface I { S get(); }; };";
        assert_eq!(
            validate(src).findings,
            validate_source_for(Source::anonymous(src), &SearchPath::new(), WireGate::V1).findings
        );
        assert_eq!(WireGate::parse("v1"), Some(WireGate::V1));
        assert_eq!(WireGate::parse("deferred"), Some(WireGate::Deferred));
        assert_eq!(WireGate::parse("any"), None);
    }

    /// Missing annotations are advice, because unannotated IDL is valid CORBA
    /// and merely useless to an agent.
    #[test]
    fn missing_sidl_annotations_are_advice_and_never_block() {
        let r = validate("module m { interface I { long f(); }; };");
        assert!(r.is_ok());
        assert!(
            rules("module m { interface I { long f(); }; };")
                .iter()
                .any(|x| x == "sidl/missing-ai_desc")
        );
        assert!(r.findings.iter().all(|f| f.severity != Severity::Error));
    }

    /// S4 covers §5.3 too: a file can be valid and still be a change nobody may
    /// ship, and "add a field" is exactly what a generator will do.
    #[test]
    fn a_breaking_change_against_a_released_contract_is_an_error() {
        let released = "module m { struct S { long a; }; };";
        let proposed = "module m { struct S { long a; long b; }; };";
        let r = validate_against(proposed, released);
        assert!(!r.is_ok(), "{:?}", r.findings);
        let f = r.findings.iter().find(|f| f.rule.starts_with("evolution/")).expect("reported");
        assert_eq!(f.severity, Severity::Error);
        assert!(f.fix.as_deref().unwrap().contains("new version"), "{f:?}");
    }

    #[test]
    fn an_additive_change_is_advice_rather_than_a_refusal() {
        let released = "module m { interface I { long a(); }; };";
        let proposed = "module m { interface I { long a(); long b(); }; };";
        let r = validate_against(proposed, released);
        assert!(r.is_ok(), "{:?}", r.findings);
        assert!(r.findings.iter().any(|f| f.rule == "evolution/server-first"));
    }

    /// A file that does not parse must not also be reported against a baseline:
    /// the diff would be nonsense and would bury the real cause.
    #[test]
    fn a_broken_file_reports_only_why_it_is_broken() {
        let r = validate_against(
            "module m { struct S { long a }",
            "module m { struct S { long a; }; };",
        );
        assert!(!r.is_ok());
        assert!(r.findings.iter().all(|f| !f.rule.starts_with("evolution/")), "{:?}", r.findings);
    }
}
