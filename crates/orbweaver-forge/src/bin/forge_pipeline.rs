//! `forge-pipeline` — the §5.1 batch loop over a requirements directory.
//!
//! ```text
//! forge-pipeline --requirements <dir> --generator <command> --out <dir> [--max-rounds N]
//! ```
//!
//! Every `.txt` file in the requirements directory is one item (id = file
//! stem). Per item, the generator command is run as
//!
//! ```text
//! <command> <requirement-file>              round 1
//! <command> <requirement-file> <repair>     later rounds; <repair> is a temp
//!                                           file holding the S4 repair prompt
//! ```
//!
//! and its stdout is the IDL, written to `<out>/<id>.idl` each round
//! (overwritten on repair). Exit status: 0 all items valid, 1 some are not,
//! 2 could not run at all.
//!
//! The model is deliberately an external concern. A shell script wrapping an
//! LLM CLI plugs in later without touching this crate:
//!
//! ```text
//! #!/bin/sh
//! # $1 = requirement file, $2 = repair prompt file (rounds 2+ only)
//! claude -p "Write OMG IDL for this requirement. IDL only, no fences.
//! $(cat "$1")
//! ${2:+$(cat "$2")}"
//! ```
//!
//! Honesty note, per the project rules: no model has been run through this
//! loop in this repository yet. The loop's mechanics are tested with scripted
//! generators; a real generator's first-pass rate is unmeasured.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use orbweaver_forge::pipeline::{Generator, ItemStatus, run_batch};

/// A generator that shells out: `<command> <requirement-file> [<repair-file>]`.
///
/// Items are recognized by their requirement text, which is why `main`
/// refuses duplicate requirement contents up front — two ids sharing one text
/// would be indistinguishable here, and a wrong guess would write one item's
/// IDL over the other's.
struct CommandGenerator {
    command: String,
    out_dir: PathBuf,
    /// requirement text → (id, requirement file path)
    by_text: HashMap<String, (String, PathBuf)>,
}

impl Generator for CommandGenerator {
    fn generate(&mut self, requirement: &str, repair: Option<&str>) -> Result<String, String> {
        let (id, req_path) =
            self.by_text.get(requirement).ok_or("internal: requirement text not in map")?;

        let mut cmd = std::process::Command::new(&self.command);
        cmd.arg(req_path);
        let repair_file = match repair {
            Some(text) => {
                let path = std::env::temp_dir()
                    .join(format!("forge-pipeline-{}-{id}.repair.txt", std::process::id()));
                std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
                cmd.arg(&path);
                Some(path)
            }
            None => None,
        };

        let output = cmd.output().map_err(|e| format!("cannot run {}: {e}", self.command));
        if let Some(path) = repair_file {
            let _ = std::fs::remove_file(path);
        }
        let output = output?;
        if !output.status.success() {
            return Err(format!(
                "generator exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }

        let idl = String::from_utf8_lossy(&output.stdout).into_owned();
        // Written every round, valid or not: a batch that stops half way must
        // leave what it had on disk, not in a lost variable.
        let out = self.out_dir.join(format!("{id}.idl"));
        std::fs::write(&out, &idl).map_err(|e| format!("{}: {e}", out.display()))?;
        Ok(idl)
    }
}

fn usage() -> &'static str {
    "usage: forge-pipeline --requirements <dir> --generator <command> --out <dir> \
     [--max-rounds N]"
}

fn main() -> ExitCode {
    let mut requirements_dir: Option<PathBuf> = None;
    let mut generator_cmd: Option<String> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut max_rounds = 3usize;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value =
            |flag: &str| args.next().ok_or_else(|| format!("{flag} needs a value\n{}", usage()));
        let result = match a.as_str() {
            "--requirements" => value("--requirements").map(|v| requirements_dir = Some(v.into())),
            "--generator" => value("--generator").map(|v| generator_cmd = Some(v)),
            "--out" => value("--out").map(|v| out_dir = Some(v.into())),
            "--max-rounds" => value("--max-rounds").and_then(|v| {
                v.parse().map(|n| max_rounds = n).map_err(|_| format!("--max-rounds: {v:?}"))
            }),
            "-h" | "--help" => {
                println!("{}", usage());
                return ExitCode::SUCCESS;
            }
            other => Err(format!("unknown argument {other:?}\n{}", usage())),
        };
        if let Err(e) = result {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    }
    let (Some(req_dir), Some(command), Some(out_dir)) = (requirements_dir, generator_cmd, out_dir)
    else {
        eprintln!("{}", usage());
        return ExitCode::from(2);
    };

    // One requirement per .txt file, id = stem, in name order so runs are
    // comparable across machines.
    let mut files: Vec<PathBuf> = match std::fs::read_dir(&req_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "txt"))
            .collect(),
        Err(e) => {
            eprintln!("{}: {e}", req_dir.display());
            return ExitCode::from(2);
        }
    };
    files.sort();
    if files.is_empty() {
        eprintln!("{}: no .txt requirement files", req_dir.display());
        return ExitCode::from(2);
    }

    let mut requirements: Vec<(String, String)> = Vec::new();
    let mut by_text: HashMap<String, (String, PathBuf)> = HashMap::new();
    for path in files {
        let id = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                return ExitCode::from(2);
            }
        };
        if let Some((other, _)) = by_text.get(&text) {
            // The loop keys generator calls by requirement text; two ids with
            // one text would be indistinguishable and one would silently
            // shadow the other. Refusing is honest; guessing is not.
            eprintln!("{id} and {other} have identical requirement text; ids must be distinct");
            return ExitCode::from(2);
        }
        by_text.insert(text.clone(), (id.clone(), path));
        requirements.push((id, text));
    }

    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("{}: {e}", out_dir.display());
        return ExitCode::from(2);
    }

    let mut generator = CommandGenerator { command, out_dir, by_text };
    let report = run_batch(&mut generator, &requirements, max_rounds);

    // The §5.1 summary: batch size, first-pass rate and round count stated
    // separately, causes with affected counts, and a plain statement when the
    // rounds ran out — never only a final number.
    print!("{report}");
    for item in report.items.iter() {
        if let ItemStatus::Invalid { repair_prompt } = &item.status {
            println!("=== {}\n{repair_prompt}", item.id);
        }
    }

    if report.all_valid() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
