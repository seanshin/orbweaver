//! The §5.1 batch loop, run mechanically: generate → validate → repair →
//! regenerate.
//!
//! Every part of the loop already existed — the S4 gate ([`validate`]), the
//! repair prompt ([`crate::Report::repair_prompt`], grouped by cause per
//! §3.3), the requirements corpus — but nothing drove them over a set. This
//! module is that driver, and it enforces the two load-bearing rules of §5.1
//! structurally rather than by asking politely:
//!
//! - **No oracle peeking mid-pass.** Within a round, the generate loop over
//!   every pending item completes before the validate loop begins; the
//!   generator is handed no verdict while a pass is in flight. That keeps the
//!   first-pass rate an honest measurement of the generator and keeps shared
//!   causes visible — the same constraint §5.2 imposes on `batch-synth` by
//!   withholding Bash, imposed here by control flow.
//! - **Causes, not items.** Failures are recorded per rule per round, and the
//!   text handed back for the next round is `repair_prompt()`, which states
//!   each cause once with its occurrences under it. §3.3: the self-repair
//!   loop is only as good as the messages it feeds on.
//!
//! The generator is a trait so the loop is testable **without a model API**;
//! the tests drive it with scripted fakes. The real model is an external
//! concern: the `forge-pipeline` binary invokes any command that reads a
//! requirement file and prints IDL, so a shell script wrapping an LLM CLI
//! plugs in later without touching this crate. Per the honesty rules, stated
//! plainly: **no model has been run through this loop in this repository
//! yet.** What is measured here is the loop's mechanics; the first-pass rate
//! of a real generator is not, and nothing below pretends otherwise.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Severity, validate};

/// Something that turns a requirement into IDL text.
///
/// `repair` is the S4 repair prompt produced for **this item** in the previous
/// round, `None` on the first attempt. It is the only feedback channel the
/// loop offers, deliberately: §3.3 says the diagnostics are the product, and a
/// generator that needs more than the diagnostics is a report about the
/// diagnostics.
pub trait Generator {
    /// Produce IDL for `requirement`, or say why that was impossible.
    ///
    /// An `Err` is a per-item failure the report carries honestly — an API
    /// outage must not read as "the model wrote invalid IDL", and must never
    /// panic the batch. The item is retried in later rounds with the same
    /// `repair` context it had, since a transport failure carries no new
    /// information about the IDL.
    fn generate(&mut self, requirement: &str, repair: Option<&str>) -> Result<String, String>;
}

/// Where one item ended up when the loop stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemStatus {
    /// S4 accepted it.
    Valid,
    /// Still rejected when the rounds ran out.
    Invalid {
        /// The prompt that would have opened the next round — kept so a
        /// caller (or a human) can pick up exactly where the loop stopped.
        repair_prompt: String,
    },
    /// The generator itself failed; nothing here says anything about IDL.
    Error {
        /// The generator's own words, verbatim.
        message: String,
    },
}

/// One item's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemReport {
    /// The item's identifier, as given by the caller.
    pub id: String,
    /// Where it ended up.
    pub status: ItemStatus,
    /// How many rounds this item was generated in. 1 means it passed (or was
    /// abandoned) first try; 0 means it was never attempted (`max_rounds` 0).
    pub rounds: usize,
    /// The last IDL the generator produced for it, `None` if it never
    /// produced any. Kept even when invalid — a failed batch you can inspect
    /// beats a failed batch you cannot.
    pub idl: Option<String>,
}

/// What one batch run measured, in the shape §5.1 requires it reported.
///
/// The first-pass rate and the round count are carried **separately** and
/// [`std::fmt::Display`] prints them separately: the first measures the
/// generator, the second measures the oracle, and a single "final" number
/// would hide whichever of the two is the problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchReport {
    /// Per-item outcomes, in the caller's order.
    pub items: Vec<ItemReport>,
    /// How many items were valid after round 1 — measured before any repair,
    /// which is what makes it a statement about the generator.
    pub first_pass_valid: usize,
    /// How many rounds actually ran.
    pub rounds_used: usize,
    /// The limit the caller set, so "ran out" is distinguishable from
    /// "converged".
    pub max_rounds: usize,
    /// Causes seen per round: rule name → number of affected items. A
    /// generator `Err` appears under the pseudo-rule `generator-error` so it
    /// is counted, never lost — an unmeasured failure is still a failure.
    pub causes: Vec<BTreeMap<String, usize>>,
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

    /// Whether every item ended valid — the CLI's exit code, and nothing
    /// a caller should quote without the two numbers behind it.
    pub fn all_valid(&self) -> bool {
        self.items.iter().all(|i| i.status == ItemStatus::Valid)
    }
}

impl std::fmt::Display for BatchReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "batch: {} item(s)", self.items.len())?;
        writeln!(
            f,
            "first-pass: {}/{} valid ({:.0}%) — after round 1, before any repair",
            self.first_pass_valid,
            self.items.len(),
            self.first_pass_rate() * 100.0
        )?;
        writeln!(f, "rounds: {} used, {} allowed", self.rounds_used, self.max_rounds)?;
        for (i, round) in self.causes.iter().enumerate() {
            if round.is_empty() {
                writeln!(f, "  round {}: no causes", i + 1)?;
            } else {
                for (rule, n) in round {
                    writeln!(f, "  round {}: [{rule}] {n} item(s)", i + 1)?;
                }
            }
        }
        let failed: Vec<&ItemReport> =
            self.items.iter().filter(|i| i.status != ItemStatus::Valid).collect();
        if failed.is_empty() {
            writeln!(f, "result: all {} item(s) valid", self.items.len())
        } else {
            // The honesty rule: exhausting the rounds with failures is the
            // headline, never a footnote under a final number.
            writeln!(
                f,
                "result: NOT all valid — {} item(s) still failing after {} round(s):",
                failed.len(),
                self.rounds_used
            )?;
            for item in failed {
                let why = match &item.status {
                    ItemStatus::Invalid { .. } => "rejected by S4",
                    ItemStatus::Error { message } => message.as_str(),
                    ItemStatus::Valid => unreachable!("filtered above"),
                };
                writeln!(f, "  {}: {why}", item.id)?;
            }
            Ok(())
        }
    }
}

/// Runs the §5.1 loop over a requirements set: `(id, requirement text)` pairs.
///
/// Round 1 generates **every** item before any validation — the generate loop
/// completes before the validate loop starts, so there is no code path on
/// which a verdict reaches the generator mid-pass. Then the whole round is
/// validated at once, failures are recorded by rule, and each failed item's
/// next round carries its own `repair_prompt()`. Rounds repeat until a round
/// ends clean or `max_rounds` is spent; either way the report says which.
pub fn run_batch(
    generator: &mut dyn Generator,
    requirements: &[(String, String)],
    max_rounds: usize,
) -> BatchReport {
    struct State {
        status: ItemStatus,
        repair: Option<String>,
        rounds: usize,
        idl: Option<String>,
    }
    let mut states: Vec<State> = requirements
        .iter()
        .map(|_| State {
            // Overwritten by the first round; survives only when max_rounds
            // is 0, in which case it is the truth.
            status: ItemStatus::Error { message: "never generated: max_rounds is 0".into() },
            repair: None,
            rounds: 0,
            idl: None,
        })
        .collect();

    let mut pending: Vec<usize> = (0..requirements.len()).collect();
    let mut causes: Vec<BTreeMap<String, usize>> = Vec::new();
    let mut first_pass_valid = 0;
    let mut rounds_used = 0;

    for round in 1..=max_rounds {
        if pending.is_empty() {
            break;
        }
        rounds_used = round;

        // Generate phase: the whole pending set, no validation interleaved.
        // §5.1 rule 1 lives in the shape of this loop.
        let mut produced: Vec<(usize, Result<String, String>)> = Vec::new();
        for &i in &pending {
            let state = &mut states[i];
            state.rounds = round;
            produced.push((i, generator.generate(&requirements[i].1, state.repair.as_deref())));
        }

        // Oracle phase: only now does anything get validated.
        let mut round_causes: BTreeMap<String, usize> = BTreeMap::new();
        let mut still_failing: Vec<usize> = Vec::new();
        for (i, result) in produced {
            match result {
                Err(message) => {
                    *round_causes.entry("generator-error".into()).or_default() += 1;
                    states[i].status = ItemStatus::Error { message };
                    still_failing.push(i);
                }
                Ok(text) => {
                    let report = validate(&text);
                    states[i].idl = Some(text);
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
                            *round_causes.entry(rule.to_owned()).or_default() += 1;
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
            first_pass_valid = requirements.len() - still_failing.len();
        }
        causes.push(round_causes);
        pending = still_failing;
    }

    BatchReport {
        items: requirements
            .iter()
            .zip(states)
            .map(|((id, _), s)| ItemReport {
                id: id.clone(),
                status: s.status,
                rounds: s.rounds,
                idl: s.idl,
            })
            .collect(),
        first_pass_valid,
        rounds_used,
        max_rounds,
        causes,
    }
}
