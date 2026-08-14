//! The §5 pipeline as the stages §5 actually names, driven by the §5.1 batch
//! loop.
//!
//! ```text
//! S1 ingest      requirement text   → brief JSON   gate: ingest::gate
//! S2 synthesize  brief JSON         → .idl         gate: synthesize::gate
//! S3 annotate    .idl               → .sidl.idl    gate: annotate::check_against
//! S4 validate    the annotated file → verdict      gate: validate, and
//!                                                   validate_against where a
//!                                                   contract is registered
//!                                                   (D005 option B)
//! S5 register    valid SIDL         → catalog      register()
//! ```
//!
//! # One shape for every stage
//!
//! A [`Stage`] is a **producer plus its own gate**. That is the whole
//! abstraction, and it is what makes each stage runnable and measurable alone:
//! [`run_batch`] drives any one of them over a whole set, in the §5.1 loop, and
//! returns a [`BatchReport`] whose first-pass rate and round count are about
//! *that stage*. A pipeline that can only run end to end can tell you the
//! output is wrong; it cannot tell you which stage was wrong, and the entire
//! reason S1 and S3 are stages rather than clauses in S2's prompt is
//! attribution.
//!
//! # What the loop enforces structurally
//!
//! - **No oracle peeking mid-pass.** Within a round, the produce loop over
//!   every pending item completes before the gate loop begins. §5.2 imposes
//!   this on `batch-synth` by withholding Bash; here it is control flow.
//! - **Causes, not items.** Failures are grouped per rule per round — each
//!   cause carrying the ids it affected, which is what §5.1 asks the oracle
//!   step to produce — and the text handed back is
//!   [`crate::Report::repair_prompt`], which states each cause once with its
//!   occurrences under it.
//!
//! # Re-running a stage alone
//!
//! Every stage writes its output to a [`Workspace`] and reads its input from
//! the one before it, so `--from s3` is not a special path: it is the ordinary
//! path with the earlier artifacts already on disk. A human who edits
//! `R07.brief.json` and re-runs from S2 is using the same mechanism the full
//! run uses, which is why editing a brief is a supported operation rather than
//! a hope.
//!
//! **모든 단계는 같은 모양이다: 생산자 + 자기 게이트.** 그래서 각 단계를 단독
//! 실행·측정할 수 있고, 실패를 어느 단계 탓인지 말할 수 있다.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use orbweaver_registry::Registry;

use crate::ingest::Brief;
use crate::{Finding, Report, Severity, annotate, ingest, synthesize, validate, validate_against};

/// One stage of the §5 pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StageId {
    /// S1 — requirements in, a [`Brief`] out.
    Ingest,
    /// S2 — a brief in, an IDL draft out.
    Synthesize,
    /// S3 — a draft in, SIDL out.
    Annotate,
    /// S4 — the gate.
    Validate,
    /// S5 — registration.
    Register,
}

impl StageId {
    /// Every stage, in pipeline order.
    pub const ORDER: [StageId; 5] = [
        StageId::Ingest,
        StageId::Synthesize,
        StageId::Annotate,
        StageId::Validate,
        StageId::Register,
    ];

    /// The short name used on the command line: `s1` … `s5`.
    pub fn tag(self) -> &'static str {
        match self {
            StageId::Ingest => "s1",
            StageId::Synthesize => "s2",
            StageId::Annotate => "s3",
            StageId::Validate => "s4",
            StageId::Register => "s5",
        }
    }

    /// The name used in reports: `S1 ingest`.
    pub fn title(self) -> &'static str {
        match self {
            StageId::Ingest => "S1 ingest",
            StageId::Synthesize => "S2 synthesize",
            StageId::Annotate => "S3 annotate",
            StageId::Validate => "S4 validate",
            StageId::Register => "S5 register",
        }
    }

    /// Accepts `s1`, `S1` or the stage's own word (`ingest`).
    pub fn parse(text: &str) -> Option<StageId> {
        let t = text.trim().to_ascii_lowercase();
        StageId::ORDER.into_iter().find(|s| {
            s.tag() == t || s.title().split_whitespace().nth(1).is_some_and(|word| word == t)
        })
    }

    /// Position in the pipeline, 0-based.
    pub fn index(self) -> usize {
        StageId::ORDER.iter().position(|s| *s == self).unwrap_or(0)
    }

    /// The file name suffix this stage's artifact carries, if it writes one.
    ///
    /// S4 and S5 produce verdicts and catalog entries, not files, so they have
    /// no suffix — which is also why they cannot be re-run from an artifact of
    /// their own.
    pub fn artifact_suffix(self) -> Option<&'static str> {
        match self {
            StageId::Ingest => Some(".brief.json"),
            StageId::Synthesize => Some(".idl"),
            StageId::Annotate => Some(".sidl.idl"),
            StageId::Validate | StageId::Register => None,
        }
    }

    /// The constraints this stage's prompt carries, for a wrapper script that
    /// wants them (`forge-pipeline --print-prompt s3`).
    pub fn prompt(self) -> Option<&'static str> {
        match self {
            StageId::Ingest => Some(ingest::S1_PROMPT),
            StageId::Synthesize => Some(synthesize::S2_PROMPT),
            StageId::Annotate => Some(annotate::S3_PROMPT),
            StageId::Validate | StageId::Register => None,
        }
    }
}

impl std::fmt::Display for StageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.title())
    }
}

/// A producer and the deterministic gate that judges what it produced.
///
/// Both halves belong to the stage. A producer with someone else's gate cannot
/// be measured — its failures land in another stage's numbers — and a gate with
/// no producer is a check, not a stage.
pub trait Stage {
    /// Which stage this is.
    fn id(&self) -> StageId;

    /// Which item is about to be produced and gated.
    ///
    /// Called by [`run_batch`] before each item in both phases, so a stage that
    /// needs a **second input artifact** can go and find it. The default does
    /// nothing, because only S3 has one: D005 option C binds the scope a
    /// requirement states to the `ai_authz` S3 emits, and the token lives in
    /// S1's brief, which S3's input — an `.idl` file — cannot carry.
    ///
    /// The item id rather than the artifact itself, because the artifact is on
    /// disk in the workspace and a [`Stage`] that wants it knows where to look;
    /// widening the produce/gate signatures for one stage's second input would
    /// change every implementation of this trait for a case none of them have.
    fn begin_item(&mut self, _id: &str) {}

    /// Turn one input artifact into one output artifact.
    ///
    /// `repair` is the gate's own [`crate::Report::repair_prompt`] for **this
    /// item** in the previous round, `None` on the first attempt. It is the
    /// only feedback channel, deliberately: §3.3 says the diagnostics are the
    /// product, and a producer that needs more than the diagnostics is a report
    /// about the diagnostics.
    ///
    /// An `Err` is a per-item failure carried honestly — an API outage must not
    /// read as "the model wrote invalid IDL" and must never panic the batch.
    fn produce(&mut self, input: &str, repair: Option<&str>) -> Result<String, String>;

    /// Judge one output. `input` is what the stage was given, for the checks
    /// that only make sense as a comparison (did the brief survive; did the
    /// contract change).
    fn gate(&self, input: &str, output: &str) -> Report;
}

/// Where one item ended up when a stage's loop stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemStatus {
    /// The stage's gate accepted it.
    Valid,
    /// Still rejected when the rounds ran out.
    Invalid {
        /// The prompt that would have opened the next round — kept so a
        /// caller (or a human) can pick up exactly where the loop stopped.
        repair_prompt: String,
    },
    /// The producer itself failed; nothing here says anything about the
    /// artifact.
    Error {
        /// The producer's own words, verbatim.
        message: String,
    },
}

/// One item's outcome in one stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemReport {
    /// The item's identifier, as given by the caller.
    pub id: String,
    /// Where it ended up.
    pub status: ItemStatus,
    /// How many rounds this item ran in this stage. 1 means it passed (or was
    /// abandoned) first try; 0 means it was never attempted (`max_rounds` 0).
    pub rounds: usize,
    /// The last artifact the producer emitted, `None` if it never emitted one.
    /// Kept even when invalid — a failed batch you can inspect beats one you
    /// cannot.
    pub output: Option<String>,
}

/// What one batch through **one stage** measured, in the shape §5.1 requires.
///
/// The first-pass rate and the round count are carried separately and printed
/// separately: the first measures the producer, the second measures the gate,
/// and a single "final" number would hide whichever of the two is the problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchReport {
    /// Which stage this is a report about.
    pub stage: StageId,
    /// Per-item outcomes, in the caller's order.
    pub items: Vec<ItemReport>,
    /// How many items were valid after round 1 — measured before any repair,
    /// which is what makes it a statement about the producer.
    pub first_pass_valid: usize,
    /// How many rounds actually ran.
    pub rounds_used: usize,
    /// The limit the caller set, so "ran out" is distinguishable from
    /// "converged".
    pub max_rounds: usize,
    /// Causes seen per round: rule name → **the items it affected**, in batch
    /// order.
    ///
    /// Items rather than a count, because §5.1 says the output of the oracle
    /// step is "a list of causes with their affected items". A count tells you
    /// a cause hit two files and leaves you re-running the batch to find out
    /// which two — measured the hard way on 2026-08-13, when the run record
    /// could not say whether `identifier-case-clash` and `enclosing-scope-clash`
    /// were one item with two rules or two items with one each.
    ///
    /// A producer `Err` appears under the pseudo-rule `producer-error` so it is
    /// counted, never lost — an unmeasured failure is still a failure.
    pub causes: Vec<BTreeMap<String, Vec<String>>>,
}

impl BatchReport {
    /// The fraction of items valid after round 1, before any repair.
    /// An empty batch is vacuously all-valid.
    pub fn first_pass_rate(&self) -> f64 {
        if self.items.is_empty() {
            return 1.0;
        }
        self.first_pass_valid as f64 / self.items.len() as f64
    }

    /// Whether every item ended valid — and nothing a caller should quote
    /// without the two numbers behind it.
    pub fn all_valid(&self) -> bool {
        self.items.iter().all(|i| i.status == ItemStatus::Valid)
    }

    /// The items this stage accepted, in order.
    pub fn valid(&self) -> impl Iterator<Item = &ItemReport> {
        self.items.iter().filter(|i| i.status == ItemStatus::Valid)
    }

    /// The items one rule affected in one round, empty if it did not fire.
    /// `round` is 0-based.
    pub fn affected(&self, round: usize, rule: &str) -> &[String] {
        self.causes.get(round).and_then(|r| r.get(rule)).map(Vec::as_slice).unwrap_or_default()
    }
}

impl std::fmt::Display for BatchReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}: {} item(s)", self.stage.title(), self.items.len())?;
        writeln!(
            f,
            "  first-pass: {}/{} valid ({:.0}%) — after round 1, before any repair",
            self.first_pass_valid,
            self.items.len(),
            self.first_pass_rate() * 100.0
        )?;
        writeln!(f, "  rounds: {} used, {} allowed", self.rounds_used, self.max_rounds)?;
        for (i, round) in self.causes.iter().enumerate() {
            if round.is_empty() {
                writeln!(f, "    round {}: no causes", i + 1)?;
            } else {
                for (rule, ids) in round {
                    writeln!(
                        f,
                        "    round {}: [{rule}] {} item(s): {}",
                        i + 1,
                        ids.len(),
                        ids.join(", ")
                    )?;
                }
            }
        }
        let failed: Vec<&ItemReport> =
            self.items.iter().filter(|i| i.status != ItemStatus::Valid).collect();
        if failed.is_empty() {
            writeln!(f, "  result: all {} item(s) valid", self.items.len())
        } else {
            // The honesty rule: exhausting the rounds with failures is the
            // headline, never a footnote under a final number.
            writeln!(
                f,
                "  result: NOT all valid — {} item(s) still failing after {} round(s):",
                failed.len(),
                self.rounds_used
            )?;
            for item in failed {
                let why = match &item.status {
                    ItemStatus::Invalid { .. } => "rejected by the stage gate",
                    ItemStatus::Error { message } => message.as_str(),
                    ItemStatus::Valid => unreachable!("filtered above"),
                };
                writeln!(f, "    {}: {why}", item.id)?;
            }
            Ok(())
        }
    }
}

/// Runs the §5.1 loop over a set through **one** stage: `(id, input)` pairs.
///
/// Round 1 produces **every** item before any gating — the produce loop
/// completes before the gate loop starts, so there is no code path on which a
/// verdict reaches the producer mid-pass. Then the whole round is gated at
/// once, failures are recorded by rule, and each failed item's next round
/// carries its own `repair_prompt()`. Rounds repeat until a round ends clean or
/// `max_rounds` is spent; either way the report says which.
pub fn run_batch(
    stage: &mut dyn Stage,
    items: &[(String, String)],
    max_rounds: usize,
) -> BatchReport {
    struct State {
        status: ItemStatus,
        repair: Option<String>,
        rounds: usize,
        output: Option<String>,
    }
    let mut states: Vec<State> = items
        .iter()
        .map(|_| State {
            // Overwritten by the first round; survives only when max_rounds
            // is 0, in which case it is the truth.
            status: ItemStatus::Error { message: "never produced: max_rounds is 0".into() },
            repair: None,
            rounds: 0,
            output: None,
        })
        .collect();

    let mut pending: Vec<usize> = (0..items.len()).collect();
    let mut causes: Vec<BTreeMap<String, Vec<String>>> = Vec::new();
    let mut first_pass_valid = 0;
    let mut rounds_used = 0;

    for round in 1..=max_rounds {
        if pending.is_empty() {
            break;
        }
        rounds_used = round;

        // Produce phase: the whole pending set, no gating interleaved.
        // §5.1 rule 1 lives in the shape of this loop.
        let mut produced: Vec<(usize, Result<String, String>)> = Vec::new();
        for &i in &pending {
            let state = &mut states[i];
            state.rounds = round;
            stage.begin_item(&items[i].0);
            produced.push((i, stage.produce(&items[i].1, state.repair.as_deref())));
        }

        // Gate phase: only now does anything get judged.
        let mut round_causes: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut still_failing: Vec<usize> = Vec::new();
        for (i, result) in produced {
            match result {
                Err(message) => {
                    round_causes
                        .entry("producer-error".into())
                        .or_default()
                        .push(items[i].0.clone());
                    states[i].status = ItemStatus::Error { message };
                    still_failing.push(i);
                }
                Ok(text) => {
                    stage.begin_item(&items[i].0);
                    let report = stage.gate(&items[i].1, &text);
                    states[i].output = Some(text);
                    if report.is_ok() {
                        states[i].status = ItemStatus::Valid;
                    } else {
                        // One count per rule per item: seven occurrences of
                        // one clash in one file are one affected item, not
                        // seven findings to tally.
                        let rules: BTreeSet<&str> = report
                            .findings
                            .iter()
                            .filter(|x| x.severity == Severity::Error)
                            .map(|x| x.rule.as_str())
                            .collect();
                        for rule in rules {
                            round_causes
                                .entry(rule.to_owned())
                                .or_default()
                                .push(items[i].0.clone());
                        }
                        let prompt = report.repair_prompt();
                        states[i].repair = Some(prompt.clone());
                        states[i].status = ItemStatus::Invalid { repair_prompt: prompt };
                        still_failing.push(i);
                    }
                }
            }
        }

        if round == 1 {
            first_pass_valid = items.len() - still_failing.len();
        }
        causes.push(round_causes);
        pending = still_failing;
    }

    BatchReport {
        stage: stage.id(),
        items: items
            .iter()
            .zip(states)
            .map(|((id, _), s)| ItemReport {
                id: id.clone(),
                status: s.status,
                rounds: s.rounds,
                output: s.output,
            })
            .collect(),
        first_pass_valid,
        rounds_used,
        max_rounds,
        causes,
    }
}

/// Where the contract a regeneration is compared against comes from — D005
/// option B's "registered", made concrete.
///
/// # What "registered" means here, and why
///
/// D003-B deferred durable storage, so there is no registry of record to read,
/// and PLAN §5.3 already says as much: *"'released' currently means the file
/// `idl-diff` is pointed at rather than a contract read from a registry of
/// record."* Three candidates existed and only one of them is honest today.
///
/// - **The S5 catalog within a run — rejected.** [`register`] builds its
///   [`Registry`] *after* S4, out of the very artifacts S4 has just gated, so
///   diffing an item against it compares a contract with itself and can never
///   report a change. Its own documentation says the rows do not persist. A
///   baseline a run creates cannot constrain that run.
/// - **Nothing until a store exists — rejected.** That is D005's option E taken
///   by omission. The differ, its verdicts and [`crate::validate_against`] are
///   built, tested and proved on the wire; withholding them until storage lands
///   protects nobody, and the harm B answers — a regenerated contract whose
///   repository ids no longer resolve — is happening now.
/// - **A directory of contracts the run is pointed at — adopted.** It is
///   already what `idl-diff` means by "released", so B introduces no second
///   meaning of the word; it is auditable, because every outcome names the file
///   it was compared against ([`DiffOutcome::against`]); and it is the seam the
///   durable store plugs into — when D003-B lands, [`Registered::contract`] is
///   the only body that changes.
///
/// # An absent contract is silence, not a failure
///
/// A first generation has nothing to diff against and must never be refused for
/// it. [`Registered::contract`] answers [`Baseline::None`], the gate falls back
/// to plain [`validate`], and the absence is *counted and printed* rather than
/// passed over silently — which is the difference between "nothing was
/// registered" and "nothing was checked".
///
/// **등록본이 없으면 침묵한다.** 첫 생성은 비교 대상이 없으므로 거부하지 않는다.
/// 다만 비교하지 않았다는 사실 자체는 세어서 보고한다.
#[derive(Debug, Clone)]
pub struct Registered {
    at: Workspace,
}

impl Registered {
    /// The directory of record for this run.
    ///
    /// An item's contract is `<id>.sidl.idl`, or `<id>.idl` where nothing
    /// annotated it — the same resolution [`Workspace::gated_artifact`] uses, so
    /// a previous run's output directory *is* a usable registry of record and
    /// the two halves of "regenerate and compare" need no separate layout.
    pub fn at(dir: impl Into<PathBuf>) -> Registered {
        Registered { at: Workspace::new(dir) }
    }

    /// The directory this reads from.
    pub fn root(&self) -> &Path {
        self.at.root()
    }

    /// What is registered under `id`.
    pub fn contract(&self, id: &str) -> Baseline {
        let Some(path) = self.at.gated_artifact(id) else { return Baseline::None };
        match std::fs::read_to_string(&path) {
            Ok(idl) => Baseline::Contract { path, idl },
            Err(e) => Baseline::Unreadable { path, message: e.to_string() },
        }
    }
}

/// What the registry of record had for one item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Baseline {
    /// Nothing is registered under this id. A first generation, and silence.
    #[default]
    None,
    /// The registered contract and the file it was read from.
    Contract {
        /// Where it came from, so a report can name it.
        path: PathBuf,
        /// The contract itself.
        idl: String,
    },
    /// Something is registered and could not be read. Never silence: an
    /// unmeasured check is a failure, never a pass, and "the baseline was
    /// unreadable" must not look like "there was no baseline".
    Unreadable {
        /// The file that could not be read.
        path: PathBuf,
        /// The I/O error, verbatim.
        message: String,
    },
}

/// What S4's §5.3 comparison did for one item — the record that makes the
/// difference between *checked and clean* and *never checked* visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffOutcome {
    /// The item.
    pub id: String,
    /// The registered contract it was compared against, `None` when the item
    /// has none.
    pub against: Option<PathBuf>,
    /// Whether the comparison actually ran. `false` when there was no baseline,
    /// and `false` when the file itself was rejected — [`crate::validate_against`]
    /// refuses to diff a file that does not parse, because that diff would be
    /// nonsense and would bury the real cause.
    pub compared: bool,
    /// The changes that refused the item, worst first, verbatim.
    pub blocking: Vec<String>,
    /// The reason a superseding run gave, when one downgraded them.
    pub superseded: Option<String>,
}

/// The gate as a stage: S4, whose producer is the identity.
///
/// S4 is a verdict, not a rewrite, so its "producer" hands its input straight
/// through and the loop runs for exactly one round. Modelling it as a [`Stage`]
/// anyway is what lets `--from s4 --to s4` re-gate a directory of files with
/// the same code path, the same report shape and the same numbers as a full
/// run.
///
/// # D005 option B: the §5.3 comparison, wired in
///
/// Given a [`Registered`] directory this gate calls [`crate::validate_against`]
/// rather than [`validate`], so a regeneration over an already-registered
/// contract is diffed against what is registered and an undeclared breaking
/// change is **refused** — the capability D005 records as built, tested and
/// simply never called by the pipeline.
///
/// ## What it cannot see, stated where it is wired
///
/// The differ compares bases, operations, attributes, `TypeCode`s and constant
/// types and values. It never reads `OperationSig::annotations`, so a
/// regeneration that keeps every identifier and changes only `//@ ai_authz`
/// produces **zero changes** and passes this gate — by §5.3's own logic an
/// annotation is not a wire change at all. That is precisely why D005 landed
/// option C first and why B does not subsume it: the two see disjoint halves of
/// the same regeneration, and only C sees the half that fails silently in
/// production.
///
/// The one exception is narrower than it looks. A contract that *also* states
/// its scope as an IDL constant — as the recorded parking contract does, with
/// `const string GATE_OPERATE_PERMISSION` — moves that constant's value, which
/// the differ does compare and calls *conditionally breaking*, which this crate
/// maps to a warning. So even there B does not refuse, and it sees the scope
/// only because the contract's author chose to model it as a constant. That is
/// a property of one contract's style, not a capability of this gate.
///
/// **B는 스코프 표류를 보지 못한다.** 애노테이션은 §5.3의 표에 행이 없고 differ는
/// 읽지도 않는다. C가 먼저 착륙한 이유가 이것이며, B는 C를 대체하지 못한다.
///
/// ## Superseding, and why it is recorded rather than merely allowed
///
/// D005 warns that a full regeneration renames everything, so this gate fires
/// on every id every time, and *an approval that is always given stops being a
/// signal*. Nothing here can prevent that. What it can do is make the approval
/// leave evidence: [`ValidateStage::superseding`] downgrades the refusals under
/// a stated reason and records **which changes** that reason covered
/// ([`DiffOutcome::superseded`], written out by [`record_supersede`]), so a
/// reflex approval is at least an auditable one.
#[derive(Debug, Default)]
pub struct ValidateStage {
    registered: Option<Registered>,
    supersede: Option<String>,
    id: String,
    baseline: Baseline,
    // `Stage::gate` takes `&self` — every other stage's gate is a pure function
    // of its inputs and widening the trait for this one would be the tail
    // wagging the dog. The cell holds the ledger, nothing else.
    ledger: RefCell<Vec<DiffOutcome>>,
}

impl ValidateStage {
    /// S4 with no baseline: [`validate`] alone, exactly as before D005 option B.
    pub fn new() -> ValidateStage {
        ValidateStage::default()
    }

    /// S4 against a registry of record (D005 option B).
    pub fn against(registered: Registered) -> ValidateStage {
        ValidateStage { registered: Some(registered), ..ValidateStage::default() }
    }

    /// Declare the regeneration: breaking changes are downgraded under `reason`
    /// instead of refusing the item, and every one they cover is recorded.
    ///
    /// This is D005's option D as far as one gate can carry it — a regeneration
    /// over a registered contract is an explicit, reasoned act rather than a
    /// repeat. An empty reason is not a declaration and is ignored.
    pub fn superseding(mut self, reason: impl Into<String>) -> ValidateStage {
        let reason = reason.into();
        self.supersede = (!reason.trim().is_empty()).then_some(reason);
        self
    }

    /// Every item this gate has judged, in order, with what it was compared
    /// against — including the items that had nothing to compare against, which
    /// is the number that keeps "clean" and "unchecked" apart.
    pub fn outcomes(&self) -> Vec<DiffOutcome> {
        self.ledger.borrow().clone()
    }

    /// The registry of record this gate reads, if it has one.
    pub fn registered(&self) -> Option<&Registered> {
        self.registered.as_ref()
    }
}

impl Stage for ValidateStage {
    fn id(&self) -> StageId {
        StageId::Validate
    }

    /// Loads what is registered under this item, if anything is.
    fn begin_item(&mut self, id: &str) {
        self.id = id.to_owned();
        self.baseline = match &self.registered {
            Some(registered) => registered.contract(id),
            None => Baseline::None,
        };
    }

    fn produce(&mut self, input: &str, _repair: Option<&str>) -> Result<String, String> {
        Ok(input.to_owned())
    }

    fn gate(&self, _input: &str, output: &str) -> Report {
        let mut outcome = DiffOutcome {
            id: self.id.clone(),
            against: None,
            compared: false,
            blocking: Vec::new(),
            superseded: None,
        };
        let mut report = match &self.baseline {
            Baseline::None => validate(output),
            Baseline::Unreadable { path, message } => {
                outcome.against = Some(path.clone());
                let mut report = validate(output);
                report.findings.push(Finding {
                    rule: "evolution/registered-unreadable".into(),
                    severity: Severity::Error,
                    message: format!(
                        "{} is registered for {} and could not be read ({message}), so this \
                         regeneration was never compared against it — an unmeasured check is a \
                         failure, never a pass",
                        path.display(),
                        self.id
                    ),
                    line: 0,
                    column: 0,
                    source: self.id.clone(),
                    fix: Some(
                        "make the registered contract readable, or point --registered at the \
                         directory that holds it"
                            .into(),
                    ),
                });
                report
            }
            Baseline::Contract { path, idl } => {
                outcome.against = Some(path.clone());
                let report = validate_against(output, idl);
                // `validate_against` refuses to diff a file that does not parse,
                // so the comparison ran exactly when nothing *else* rejected it.
                outcome.compared = !report
                    .findings
                    .iter()
                    .any(|f| f.severity == Severity::Error && !f.rule.starts_with("evolution/"));
                report
            }
        };

        for finding in &mut report.findings {
            if finding.severity != Severity::Error || !finding.rule.starts_with("evolution/") {
                continue;
            }
            outcome.blocking.push(finding.message.clone());
            if let Some(reason) = &self.supersede {
                finding.severity = Severity::Warning;
                finding.message = format!("superseded — {reason}: {}", finding.message);
                finding.fix = None;
                outcome.superseded = Some(reason.clone());
            }
        }
        report.findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.line.cmp(&b.line)));
        self.ledger.borrow_mut().push(outcome);
        report
    }
}

/// The name of the record [`record_supersede`] writes.
pub const SUPERSEDED_FILE: &str = "superseded.tsv";

/// Writes what a superseding run waved through, or `Ok(None)` if it waved
/// nothing through.
///
/// D005 option D asks that a regeneration over a registered contract be *"an
/// explicit `--supersede <id> --reason <text>` that is recorded"*. This is the
/// recording half: one row per change, naming the item, the reason given and
/// the change it covered. A blanket approval stays a blanket approval — the
/// gate cannot make it thoughtful — but it stops being invisible.
pub fn record_supersede(
    outcomes: &[DiffOutcome],
    out_dir: &Path,
) -> Result<Option<PathBuf>, PipelineError> {
    let waved: Vec<&DiffOutcome> = outcomes.iter().filter(|o| o.superseded.is_some()).collect();
    if waved.is_empty() {
        return Ok(None);
    }
    let mut text = String::from(
        "# Breaking changes waved through by an explicit supersede (docs/PLAN.md \u{a7}5.3,\n\
         # docs/decisions/D005-contract-stability.md option D). Each row is a change a\n\
         # deployed peer does not survive, allowed to land under the reason beside it.\n\
         # An approval that is always given stops being a signal; this file is what is\n\
         # left to read when that happens.\n\
         # columns: item\treason\tchange\n",
    );
    for outcome in &waved {
        let reason = outcome.superseded.as_deref().unwrap_or_default();
        let against = outcome.against.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        for change in &outcome.blocking {
            text.push_str(&format!("{}\t{reason}\t{change}\t{against}\n", outcome.id));
        }
    }
    let path = out_dir.join(SUPERSEDED_FILE);
    let io = |e: std::io::Error| PipelineError::Io { path: path.clone(), message: e.to_string() };
    std::fs::create_dir_all(out_dir).map_err(io)?;
    std::fs::write(&path, text).map_err(io)?;
    Ok(Some(path))
}

/// A stage backed by an external command — the seam a model plugs into.
///
/// The contract is the same for every stage, so one wrapper script serves all
/// three:
///
/// ```text
/// <command> <input-file> [<repair-file>]
/// ```
///
/// with `FORGE_STAGE` (`s1`/`s2`/`s3`) and `FORGE_PROMPT` (a file holding that
/// stage's constraints) in the environment. Standard output is the artifact.
/// A non-zero exit is a producer error, recorded as such and never as "the
/// model wrote something invalid".
///
/// The prompt is passed as a file rather than baked into the wrapper because
/// the 2026-08-13 run record's proposed rule applies here: *a first-pass rate
/// is meaningless without the prompt that produced it*, and a prompt that lives
/// in this crate is versioned with the checker that grades it.
pub struct CommandStage {
    stage: StageId,
    command: String,
    scratch: PathBuf,
    briefs: Option<PathBuf>,
    brief: Option<Brief>,
}

impl CommandStage {
    /// A stage that shells out to `command`. `scratch` holds the temporary
    /// input, repair and prompt files.
    pub fn new(stage: StageId, command: impl Into<String>, scratch: impl Into<PathBuf>) -> Self {
        CommandStage {
            stage,
            command: command.into(),
            scratch: scratch.into(),
            briefs: None,
            brief: None,
        }
    }

    /// Also read each item's brief out of `workspace` — S3's second input.
    ///
    /// This is the plumbing D005 option C names as its honest cost. With it,
    /// S3's producer is handed the scopes the requirement states
    /// ([`annotate::stated_scopes_block`], appended to the prompt file the
    /// wrapper already reads) and S3's gate binds them
    /// ([`annotate::check_against_brief`]). Without it — S3 run alone over a
    /// directory of IDL nobody has a brief for — nothing is bound and nothing is
    /// claimed, which is the honest reading of a missing artifact.
    ///
    /// Only S3 reads it. S1 has no brief yet and S2 already receives one.
    pub fn with_briefs(mut self, workspace: &Workspace) -> Self {
        self.briefs = Some(workspace.root().to_path_buf());
        self
    }

    fn write(&self, name: &str, text: &str) -> Result<PathBuf, String> {
        std::fs::create_dir_all(&self.scratch)
            .map_err(|e| format!("{}: {e}", self.scratch.display()))?;
        let path = self.scratch.join(name);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }
}

impl Stage for CommandStage {
    fn id(&self) -> StageId {
        self.stage
    }

    /// Loads this item's brief, when the stage was given somewhere to find one.
    ///
    /// A brief that is absent or unreadable binds nothing and is not an error:
    /// `--from s3` over hand-written IDL is a supported run, and refusing it
    /// would make a second input mandatory for a stage that has always had one.
    fn begin_item(&mut self, id: &str) {
        self.brief = None;
        if self.stage != StageId::Annotate {
            return;
        }
        let Some(dir) = &self.briefs else { return };
        let path =
            dir.join(format!("{id}{}", StageId::Ingest.artifact_suffix().unwrap_or_default()));
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        self.brief = Brief::parse(&text).ok();
    }

    fn produce(&mut self, input: &str, repair: Option<&str>) -> Result<String, String> {
        // S2 is handed the brief's *rendering*, not its JSON: the JSON is the
        // canonical artifact a human edits, and the rendering is what a prompt
        // can use. Prose input (S1 skipped) passes straight through.
        let prepared = match (self.stage, Brief::parse(input)) {
            (StageId::Synthesize, Ok(brief)) => brief.to_prompt(),
            _ => input.to_owned(),
        };

        // One scratch file per stage, rewritten per call: `run_batch` calls
        // producers one at a time, and a name per item would leave a directory
        // of stale inputs beside the artifacts a human is meant to read.
        let pid = std::process::id();
        let input_file = self.write(&format!("{}-{pid}.input", self.stage.tag()), &prepared)?;
        let mut cmd = std::process::Command::new(&self.command);
        cmd.arg(&input_file);
        cmd.env("FORGE_STAGE", self.stage.tag());
        if let Some(prompt) = self.stage.prompt() {
            // The constraints are the crate constant, always. What follows them
            // is per-item *data* the constraints refer to — the scopes the
            // requirement states, which S3 cannot read off its `.idl` input.
            // `--print-prompt s3` still prints the constant alone, because the
            // constant is what a first-pass rate is a measurement of.
            let scopes = self.brief.as_ref().map(Brief::stated_scopes).unwrap_or_default();
            let text = format!("{prompt}\n{}", annotate::stated_scopes_block(&scopes));
            let path = self.write(&format!("{}.prompt.txt", self.stage.tag()), &text)?;
            cmd.env("FORGE_PROMPT", &path);
        }
        if let Some(text) = repair {
            let path = self.write(&format!("{}-{pid}.repair", self.stage.tag()), text)?;
            cmd.arg(&path);
        }

        let output = cmd.output().map_err(|e| format!("cannot run {}: {e}", self.command))?;
        if !output.status.success() {
            return Err(format!(
                "{} exited {}: {}",
                self.command,
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn gate(&self, input: &str, output: &str) -> Report {
        gate_for_with_brief(self.stage, self.brief.as_ref(), input, output)
    }
}

/// The gate belonging to a stage, as a free function.
///
/// Exposed so a caller can check one artifact without constructing a producer:
/// `gate_for(StageId::Annotate, draft, annotated)` is S3's whole verdict on one
/// file — minus the one check that needs a second artifact, for which see
/// [`gate_for_with_brief`].
///
/// S4 here is [`validate`] alone. The §5.3 comparison against a registered
/// contract needs a *third* artifact — the contract — and lives on
/// [`ValidateStage`], which knows where to find one.
pub fn gate_for(stage: StageId, input: &str, output: &str) -> Report {
    gate_for_with_brief(stage, None, input, output)
}

/// [`gate_for`] with the item's brief, where the caller has one.
///
/// S3 is the only stage that reads it, and it reads it for exactly one rule:
/// `s3/authz-not-the-stated-scope`, which binds a scope-shaped token the
/// requirement states to the `//@ ai_authz` this stage emits (D005 option C).
pub fn gate_for_with_brief(
    stage: StageId,
    brief: Option<&Brief>,
    input: &str,
    output: &str,
) -> Report {
    match stage {
        StageId::Ingest => ingest::gate(output),
        StageId::Synthesize => synthesize::gate(input, output),
        StageId::Annotate => annotate::check_against_brief(brief, input, output),
        StageId::Validate | StageId::Register => validate(output),
    }
}

/// The directory the stages hand artifacts to each other through.
///
/// One file per item per stage, named by the item id, so a rerun that skips
/// earlier stages is the ordinary path with the earlier files already present.
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// A workspace rooted at `root`. The directory is created on first write.
    pub fn new(root: impl Into<PathBuf>) -> Workspace {
        Workspace { root: root.into() }
    }

    /// The directory itself.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where a stage's artifact for `id` lives, or `None` for the stages that
    /// produce verdicts rather than files.
    pub fn artifact(&self, stage: StageId, id: &str) -> Option<PathBuf> {
        stage.artifact_suffix().map(|suffix| self.root.join(format!("{id}{suffix}")))
    }

    /// The file S4 gates for `id`: S3's output if it exists, else S2's.
    ///
    /// Which one it was is worth printing, because "S4 passed" means something
    /// different about a file no annotation stage has seen.
    pub fn gated_artifact(&self, id: &str) -> Option<PathBuf> {
        let annotated = self.artifact(StageId::Annotate, id)?;
        if annotated.exists() {
            return Some(annotated);
        }
        let draft = self.artifact(StageId::Synthesize, id)?;
        draft.exists().then_some(draft)
    }

    /// Writes a stage's artifact, canonicalising a brief so the file a human
    /// opens is one they can read.
    pub fn store(&self, stage: StageId, id: &str, text: &str) -> Result<(), PipelineError> {
        let Some(path) = self.artifact(stage, id) else { return Ok(()) };
        let body = match stage {
            StageId::Ingest => match Brief::parse(text) {
                Ok(brief) => brief.to_text(),
                // Not a brief: keep exactly what was produced. A gate failure a
                // human cannot see is a gate failure nobody can fix.
                Err(_) => text.to_owned(),
            },
            _ => text.to_owned(),
        };
        let io =
            |e: std::io::Error| PipelineError::Io { path: path.clone(), message: e.to_string() };
        std::fs::create_dir_all(&self.root).map_err(io)?;
        std::fs::write(&path, body).map_err(io)
    }

    /// Reads the artifact a stage would **consume** for `id` — the previous
    /// stage's output, which is what makes resuming and running through the
    /// same path.
    pub fn load(&self, stage: StageId, id: &str) -> Result<String, PipelineError> {
        let path = match stage {
            StageId::Validate | StageId::Register => self.gated_artifact(id),
            other => input_stage(other).and_then(|previous| self.artifact(previous, id)),
        };
        let Some(path) = path else {
            return Err(PipelineError::MissingInput { stage, id: id.to_owned() });
        };
        std::fs::read_to_string(&path)
            .map_err(|e| PipelineError::Io { path, message: e.to_string() })
    }

    /// The ids whose **input** artifact for `stage` is already on disk.
    ///
    /// This is how `--from s3` finds its work without being told: the drafts
    /// that exist are the items. Returned sorted and deduplicated, so runs are
    /// comparable across machines.
    ///
    /// Discovery has to accept exactly what [`Workspace::load`] will resolve,
    /// or the fallback it documents is unreachable. S4 and S5 load `<id>.sidl.idl`
    /// **or** `<id>.idl` where nothing annotated it; globbing only the annotated
    /// suffix meant an unannotated contract could be loaded but never found, so
    /// `--only s4` over a legacy estate saw nothing and the only way to be
    /// gated at all was to rename the file. That rename is what made the run
    /// report a file with no annotations in it as annotated — one defect
    /// feeding the next.
    pub fn ids_ready_for(&self, stage: StageId) -> Vec<String> {
        let Some(previous) = input_stage(stage) else { return Vec::new() };
        let Some(suffix) = previous.artifact_suffix() else { return Vec::new() };
        let Ok(entries) = std::fs::read_dir(&self.root) else { return Vec::new() };
        let mut ids: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .filter_map(|name| {
                // `.idl` must not swallow `.sidl.idl`: to S3 the draft set and
                // the annotated set are different sets of files, and an item
                // already annotated is not waiting to be annotated again.
                if previous == StageId::Synthesize && name.ends_with(".sidl.idl") {
                    return None;
                }
                // To a gate they are one set: an unannotated contract is a
                // contract, and it is the case the annotation gates exist for.
                if gates_whatever_exists(stage) && name.ends_with(".idl") {
                    return Some(
                        name.strip_suffix(".sidl.idl")
                            .or_else(|| name.strip_suffix(".idl"))
                            .unwrap_or(&name)
                            .to_owned(),
                    );
                }
                name.strip_suffix(suffix).map(str::to_owned)
            })
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }
}

/// The stage whose artifact `stage` consumes, or `None` for S1.
fn input_stage(stage: StageId) -> Option<StageId> {
    match stage {
        StageId::Ingest => None,
        StageId::Synthesize => Some(StageId::Ingest),
        StageId::Annotate => Some(StageId::Synthesize),
        // S4 and S5 gate whatever the last producing stage left; `load` resolves
        // that to the annotated file if there is one.
        StageId::Validate | StageId::Register => Some(StageId::Annotate),
    }
}

/// Whether `stage` gates whatever the producers left, rather than consuming one
/// named producer's output.
///
/// S4 and S5 are the two: they run over a contract whoever wrote it, which is
/// the whole point of being able to point them at an estate nothing in this
/// pipeline produced.
fn gates_whatever_exists(stage: StageId) -> bool {
    matches!(stage, StageId::Validate | StageId::Register)
}

/// Why a run could not start, or could not continue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineError {
    /// Nothing in the range could run: no producer falls inside it and the
    /// gate is outside it. Refused rather than reported as a clean empty run —
    /// an unmeasured check is a failure, never a pass.
    NothingToRun {
        /// The requested first stage.
        first: StageId,
        /// The requested last stage.
        last: StageId,
    },
    /// A stage's input artifact is not on disk. Re-running from a later stage
    /// requires the earlier stage's output to exist.
    MissingInput {
        /// The stage that has nothing to read.
        stage: StageId,
        /// The item.
        id: String,
    },
    /// The range runs backwards.
    BadRange {
        /// The requested first stage.
        first: StageId,
        /// The requested last stage.
        last: StageId,
    },
    /// An artifact could not be read or written.
    Io {
        /// The path that failed.
        path: PathBuf,
        /// The I/O error, verbatim.
        message: String,
    },
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineError::NothingToRun { first, last } => write!(
                f,
                "nothing to run between {} and {}: no producer falls in that range and the gate \
                 is outside it",
                first.title(),
                last.title()
            ),
            PipelineError::MissingInput { stage, id } => write!(
                f,
                "{}: {} has no input artifact in the workspace — run the stage before it, or \
                 point --out at the directory that holds it",
                stage.title(),
                id
            ),
            PipelineError::BadRange { first, last } => {
                write!(f, "{} comes after {}", first.title(), last.title())
            }
            PipelineError::Io { path, message } => write!(f, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for PipelineError {}

/// The producers for a run, and the range to run them over.
///
/// A producing stage inside the range with no producer is **skipped**, and the
/// skip is named in the report ([`PipelineReport::skipped`]). Skipping has to be
/// possible — running S2 and the gate without an annotation pass is the
/// pre-split pipeline and a legitimate thing to ask for — but it must never be
/// silent, because "S4 passed" means something different about a file no
/// annotation stage has seen.
pub struct Pipeline<'a> {
    /// S1's producer.
    pub ingest: Option<&'a mut dyn Stage>,
    /// S2's producer.
    pub synthesize: Option<&'a mut dyn Stage>,
    /// S3's producer.
    pub annotate: Option<&'a mut dyn Stage>,
    /// S4 — the gate itself, which has no producer but does have a
    /// configuration: the registry of record it compares against, and whether
    /// this regeneration was declared (D005 options B and D).
    /// [`ValidateStage::default`] is the pre-D005-B behaviour, [`validate`]
    /// alone. Read [`ValidateStage::outcomes`] after the run for what it
    /// compared and what it did not.
    pub validate: ValidateStage,
    /// The first stage to run.
    pub first: StageId,
    /// The last stage to run. Capped at [`StageId::Validate`]; S5 is
    /// [`register`], which the caller invokes with the S4 report.
    pub last: StageId,
    /// Repair rounds allowed **per stage**.
    pub max_rounds: usize,
}

impl<'a> Pipeline<'a> {
    /// A pipeline with no producers, running S4 alone — the smallest useful
    /// run, and the one a caller re-gating existing files wants.
    pub fn gate_only() -> Pipeline<'a> {
        Pipeline {
            ingest: None,
            synthesize: None,
            annotate: None,
            validate: ValidateStage::default(),
            first: StageId::Validate,
            last: StageId::Validate,
            max_rounds: 1,
        }
    }

    fn producer<'s>(&'s mut self, stage: StageId) -> Option<&'s mut (dyn Stage + 'a)> {
        match stage {
            StageId::Ingest => self.ingest.as_deref_mut(),
            StageId::Synthesize => self.synthesize.as_deref_mut(),
            StageId::Annotate => self.annotate.as_deref_mut(),
            StageId::Validate | StageId::Register => None,
        }
    }
}

/// Every stage's report from one run, in the order the stages ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineReport {
    /// One entry per stage that ran.
    pub stages: Vec<BatchReport>,
    /// Items that never reached the end because an earlier stage rejected them:
    /// `(id, the stage that stopped it)`. Carried explicitly so a later stage's
    /// clean numbers cannot be read as "the batch is fine".
    pub dropped: Vec<(String, StageId)>,
    /// Stages inside the range that had no producer and did not run. Never
    /// silent: a batch with no annotation pass is a different batch.
    pub skipped: Vec<StageId>,
}

impl PipelineReport {
    /// Whether every item passed every stage it reached, and none was dropped.
    pub fn all_valid(&self) -> bool {
        self.dropped.is_empty() && self.stages.iter().all(BatchReport::all_valid)
    }

    /// The last stage's report, which is what S5 registers from.
    pub fn last(&self) -> Option<&BatchReport> {
        self.stages.last()
    }

    /// One stage's report, if that stage ran.
    pub fn stage(&self, stage: StageId) -> Option<&BatchReport> {
        self.stages.iter().find(|s| s.stage == stage)
    }
}

impl std::fmt::Display for PipelineReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for stage in &self.stages {
            write!(f, "{stage}")?;
        }
        for stage in &self.skipped {
            writeln!(
                f,
                "{}: SKIPPED — no producer was supplied, so nothing performed this stage's work",
                stage.title()
            )?;
        }
        if self.dropped.is_empty() {
            return Ok(());
        }
        // The honesty rule, applied across stages: a later stage's 100% is
        // 100% *of what reached it*, and what did not reach it is stated.
        writeln!(f, "dropped before the end of the run:")?;
        for (id, stage) in &self.dropped {
            writeln!(f, "  {id}: rejected at {}", stage.title())?;
        }
        Ok(())
    }
}

/// Runs a range of stages over a set, one whole stage at a time.
///
/// `items` are `(id, input)` for the **first** stage in the range; every later
/// stage reads the previous stage's artifact out of `workspace`, which is
/// exactly what a rerun from a later stage does. An item a stage rejects does
/// not continue — it is recorded in [`PipelineReport::dropped`] so a later
/// stage's clean number cannot be mistaken for a clean batch.
pub fn run_pipeline(
    pipeline: &mut Pipeline<'_>,
    workspace: &Workspace,
    items: &[(String, String)],
) -> Result<PipelineReport, PipelineError> {
    if pipeline.first.index() > pipeline.last.index() {
        return Err(PipelineError::BadRange { first: pipeline.first, last: pipeline.last });
    }
    let (first, last) = (pipeline.first, pipeline.last);
    let in_range = move |s: &StageId| {
        s.index() >= first.index() && s.index() <= last.index() && *s != StageId::Register
    };
    // What can actually run: the producers supplied, plus the gate. A producing
    // stage in the range with no producer is skipped and said so — see
    // `Pipeline`'s documentation for why skipping is allowed and silence is not.
    let mut stages: Vec<StageId> = Vec::new();
    let mut skipped: Vec<StageId> = Vec::new();
    for stage in StageId::ORDER.into_iter().filter(in_range) {
        if stage == StageId::Validate || pipeline.producer(stage).is_some() {
            stages.push(stage);
        } else {
            skipped.push(stage);
        }
    }
    if stages.is_empty() {
        return Err(PipelineError::NothingToRun { first: pipeline.first, last: pipeline.last });
    }

    let mut report = PipelineReport { stages: Vec::new(), dropped: Vec::new(), skipped };
    let mut current: Vec<(String, String)> = items.to_vec();

    for (position, stage) in stages.iter().copied().enumerate() {
        if current.is_empty() {
            break;
        }
        let max_rounds = pipeline.max_rounds;
        let batch = if let Some(producer) = pipeline.producer(stage) {
            run_batch(producer, &current, max_rounds)
        } else {
            // S4 rewrites nothing, so one round is the whole loop. The stage is
            // the caller's, so its ledger — what each item was compared against
            // — outlives the run.
            run_batch(&mut pipeline.validate, &current, 1)
        };

        for item in &batch.items {
            if let Some(text) = &item.output {
                workspace.store(stage, &item.id, text)?;
            }
            if item.status != ItemStatus::Valid {
                report.dropped.push((item.id.clone(), stage));
            }
        }

        // The next stage reads what this one wrote, from disk — the same path a
        // rerun takes, so "run it all" and "resume from here" cannot diverge.
        // "The next stage" is the next one that will *run*: a skipped S3 means
        // S4 reads S2's draft, which `Workspace::load` resolves.
        let survivors: Vec<String> = batch.valid().map(|i| i.id.clone()).collect();
        report.stages.push(batch);
        current = Vec::new();
        if let Some(next) = stages.get(position + 1) {
            for id in survivors {
                let text = workspace.load(*next, &id)?;
                current.push((id, text));
            }
        }
    }

    Ok(report)
}

/// The name of the worksheet [`register`] writes into its output directory.
pub const EXPOSURE_TODO_FILE: &str = "exposure.todo.tsv";

/// What S5 produced: a loaded catalog and the list of what *could* be exposed
/// — none of which *is*.
///
/// `docs/PLAN.md` §7.4 I2: registration feeds the catalog with exposure off by
/// default. Nothing here builds an `Exposure`; a generated interface becomes
/// agent-visible only when an operator constructs one and allowlists the id,
/// which is the same explicit step a hand-written interface requires.
#[derive(Debug)]
pub struct Registration {
    /// Every valid item's IDL, loaded into one in-process catalog.
    pub registry: Registry,
    /// The repository ids an operator could choose to expose — interfaces
    /// only, as [`orbweaver_mcp::exposable_interfaces`] defines the term.
    /// This is a menu, never a grant.
    pub exposable: Vec<String>,
}

/// Why S5 refused to register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// An item the report marks valid carries no artifact, so there is nothing
    /// whose validity could be re-checked — refused rather than trusted.
    MissingIdl {
        /// The item's id.
        id: String,
    },
    /// The S5 re-check rejected an item the report marks valid. S4 already
    /// passed it, but registration is a gate, not a formality: a report is
    /// data anyone can construct, and the catalog trusts no one's word.
    Rejected {
        /// The item's id.
        id: String,
        /// The repair prompt the re-check produced, so the refusal carries
        /// the same actionable text a generator would have received.
        repair_prompt: String,
    },
    /// The registry itself refused the load (for example, two items declaring
    /// conflicting definitions under one repository id).
    Registry {
        /// The item whose load failed.
        id: String,
        /// The registry's own message.
        message: String,
    },
    /// The exposure worksheet could not be written. Not ignorable: a
    /// registration whose worksheet is missing leaves no record of what is
    /// waiting for a human decision.
    Io {
        /// The path that failed.
        path: PathBuf,
        /// The I/O error, verbatim.
        message: String,
    },
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegisterError::MissingIdl { id } => {
                write!(f, "{id}: marked valid but the report carries no IDL for it")
            }
            RegisterError::Rejected { id, repair_prompt } => {
                write!(f, "{id}: the S5 re-check rejected it:\n{repair_prompt}")
            }
            RegisterError::Registry { id, message } => {
                write!(f, "{id}: the registry refused the load: {message}")
            }
            RegisterError::Io { path, message } => write!(f, "{}: {message}", path.display()),
        }
    }
}

impl std::error::Error for RegisterError {}

/// S5 — register a validated batch, with exposure **off** by default.
///
/// Loads every [`ItemStatus::Valid`] item's IDL into one [`Registry`], after
/// re-running [`validate`] on each file. S4 already passed them, but the
/// re-check is deliberate: registration is a gate, not a formality, and a
/// `BatchReport` is plain data whose `Valid` markings this function refuses
/// to take on faith. Any re-check failure aborts the whole registration —
/// a catalog holding "most of a batch" would misreport what was registered.
///
/// The catalog comes back with **nothing exposed**. Alongside it, the
/// function writes [`EXPOSURE_TODO_FILE`] into `out_dir`: one line per
/// exposable interface, every one `exposed=no`, for a human to turn into
/// allowlist entries. §7.4 I2 is the reason the default is no — a projection
/// that exposes by default exposes the day someone adds a file.
///
/// Honesty note: "register" here means an in-process [`Registry`] plus a
/// human-readable worksheet. The durable catalog store (`docs/PLAN.md` §6,
/// PostgreSQL + pgvector) is future work; this function is the seam where it
/// plugs in, and nothing below pretends the rows persist.
pub fn register(report: &BatchReport, out_dir: &Path) -> Result<Registration, RegisterError> {
    let mut registry = Registry::new();
    for item in report.items.iter().filter(|i| i.status == ItemStatus::Valid) {
        let Some(idl) = &item.output else {
            return Err(RegisterError::MissingIdl { id: item.id.clone() });
        };
        let recheck = validate(idl);
        if !recheck.is_ok() {
            return Err(RegisterError::Rejected {
                id: item.id.clone(),
                repair_prompt: recheck.repair_prompt(),
            });
        }
        // `validate` just accepted it, so `check` succeeds; matched anyway so
        // a future divergence between the two is a loud error, not a panic.
        let spec = orbweaver_idl::check(idl).map_err(|diags| RegisterError::Rejected {
            id: item.id.clone(),
            repair_prompt: diags.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("\n"),
        })?;
        registry
            .load(&spec)
            .map_err(|e| RegisterError::Registry { id: item.id.clone(), message: e.message })?;
    }

    let exposable = orbweaver_mcp::exposable_interfaces(&registry);

    let path = out_dir.join(EXPOSURE_TODO_FILE);
    let io = |e: std::io::Error| RegisterError::Io { path: path.clone(), message: e.to_string() };
    std::fs::create_dir_all(out_dir).map_err(io)?;
    let mut worksheet = String::from(
        "# S5 exposure worksheet — docs/PLAN.md \u{a7}7.4 I2.\n\
         # Every interface below is registered and NOT exposed. The default is no\n\
         # because a projection that exposes by default exposes the day someone adds\n\
         # a file; a generated interface becomes agent-visible only when a human\n\
         # turns its exposed=no into an explicit allowlist entry (Exposure::\n\
         # allow_interface or allow_operation), exactly like a hand-written one.\n\
         # Nothing reads this file automatically; it is a worksheet, not a policy.\n\
         # columns: repository-id\texposed\tnote\n",
    );
    for id in &exposable {
        worksheet.push_str(&format!("{id}\texposed=no\tawaiting a human allowlist decision\n"));
    }
    std::fs::write(&path, worksheet).map_err(io)?;

    Ok(Registration { registry, exposable })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_names_parse_both_ways() {
        for stage in StageId::ORDER {
            assert_eq!(StageId::parse(stage.tag()), Some(stage));
            let word = stage.title().split_whitespace().nth(1).expect("a word");
            assert_eq!(StageId::parse(word), Some(stage), "{}", stage.title());
        }
        assert_eq!(StageId::parse("s9"), None);
    }

    #[test]
    fn the_stage_order_is_the_plan_order() {
        let tags: Vec<&str> = StageId::ORDER.iter().map(|s| s.tag()).collect();
        assert_eq!(tags, ["s1", "s2", "s3", "s4", "s5"]);
        assert!(StageId::Ingest.index() < StageId::Annotate.index());
    }

    /// `.idl` must not swallow `.sidl.idl`: the draft set and the annotated set
    /// are different sets of files, and confusing them would re-annotate an
    /// already annotated file.
    #[test]
    fn the_draft_and_annotated_artifacts_do_not_collide() {
        let dir = std::env::temp_dir().join(format!("forge-ws-suffix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ws = Workspace::new(&dir);
        ws.store(StageId::Synthesize, "R01", "module a {};").expect("writes");
        ws.store(StageId::Annotate, "R01", "module a {};").expect("writes");

        assert_eq!(ws.ids_ready_for(StageId::Annotate), vec!["R01".to_owned()]);
        // S4 gates the annotated file when there is one.
        assert!(ws.gated_artifact("R01").unwrap().to_string_lossy().ends_with(".sidl.idl"));
        // …and finds the item exactly once, not once per artifact on disk.
        assert_eq!(ws.ids_ready_for(StageId::Validate), vec!["R01".to_owned()]);
    }

    /// Discovery must accept everything [`Workspace::load`] resolves, or the
    /// fallback that resolution documents is unreachable.
    ///
    /// A legacy estate is a directory of `.idl` written by somebody else. S4
    /// could *load* one and could not *find* one, so the only way to be gated
    /// was to rename the file to `.sidl.idl` — which then made the run report a
    /// contract with no annotations in it as annotated. A gate that can only be
    /// reached by misdescribing its input is not a gate.
    #[test]
    fn a_gate_finds_an_unannotated_contract_nothing_here_produced() {
        let dir = std::env::temp_dir().join(format!("forge-ws-estate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("creates");
        // Not stored through a stage: dropped in, the way an estate arrives.
        std::fs::write(dir.join("ESTATE.idl"), "module a { interface I { void f(); }; };")
            .expect("writes");
        let ws = Workspace::new(&dir);

        assert_eq!(ws.ids_ready_for(StageId::Validate), vec!["ESTATE".to_owned()]);
        assert_eq!(ws.ids_ready_for(StageId::Register), vec!["ESTATE".to_owned()]);
        // S3 is a producer, not a gate: it consumes drafts and this is not one
        // of its drafts to re-annotate — but the difference is deliberate, so
        // both halves are pinned here rather than left to be discovered.
        assert_eq!(ws.ids_ready_for(StageId::Annotate), vec!["ESTATE".to_owned()]);
    }

    #[test]
    fn s4_gates_the_draft_when_no_annotation_stage_ran() {
        let dir = std::env::temp_dir().join(format!("forge-ws-draft-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ws = Workspace::new(&dir);
        ws.store(StageId::Synthesize, "R02", "module a {};").expect("writes");
        let gated = ws.gated_artifact("R02").expect("a draft");
        assert!(gated.to_string_lossy().ends_with("R02.idl"));
        assert!(!gated.to_string_lossy().ends_with(".sidl.idl"));
    }
}
