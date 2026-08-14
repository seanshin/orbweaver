//! `forge-pipeline` — the §5 stages over a requirements set, in the §5.1 loop.
//!
//! ```text
//! forge-pipeline --out <dir>
//!                [--requirements <dir>]
//!                [--ingest <cmd>] [--synthesize <cmd>] [--annotate <cmd>]
//!                [--from s1] [--to s4] [--only s3]
//!                [--max-rounds N] [--register]
//!                [--print-prompt s1|s2|s3]
//! ```
//!
//! # One command contract for every stage
//!
//! ```text
//! <command> <input-file> [<repair-file>]
//! ```
//!
//! with `FORGE_STAGE` (`s1`, `s2` or `s3`) and `FORGE_PROMPT` (a file holding
//! that stage's constraints, straight out of this crate) in the environment.
//! Standard output is the artifact. One wrapper script therefore serves all
//! three stages, and the prompt it uses is versioned with the checker that
//! grades it — the 2026-08-13 run record's proposed rule, made mechanical:
//! *a first-pass rate is meaningless without the prompt that produced it.*
//!
//! ```sh
//! #!/bin/sh
//! # $1 = input file, $2 = repair prompt (rounds 2+ only)
//! claude -p "$(cat "$FORGE_PROMPT")
//!
//! $(cat "$1")
//! ${2:+$(cat "$2")}"
//! ```
//!
//! # Artifacts, and re-running one stage
//!
//! ```text
//! <out>/<id>.brief.json   S1
//! <out>/<id>.idl          S2
//! <out>/<id>.sidl.idl     S3
//! ```
//!
//! `--from s3` reads the drafts already in `--out` and needs no requirements
//! directory; `--only s4` re-gates whatever is there. Editing a brief by hand
//! and re-running `--from s2` is the supported way to correct S1's reading,
//! and it is not a special path — every stage always reads its input from the
//! workspace.
//!
//! Exit status: 0 every item valid at every stage, 1 some are not, 2 could not
//! run at all.

use std::path::PathBuf;
use std::process::ExitCode;

use orbweaver_forge::pipeline::{
    CommandStage, EXPOSURE_TODO_FILE, ItemStatus, Pipeline, StageId, Workspace, register,
    run_pipeline,
};

fn usage() -> String {
    format!(
        "usage: forge-pipeline --out <dir> [--requirements <dir>]\n\
         \x20   [--ingest <cmd>] [--synthesize <cmd>] [--annotate <cmd>]\n\
         \x20   [--from {stages}] [--to {stages}] [--only {stages}]\n\
         \x20   [--max-rounds N] [--register] [--print-prompt s1|s2|s3]\n\
         \n\
         Every producer is invoked as `<cmd> <input-file> [<repair-file>]`, with\n\
         FORGE_STAGE and FORGE_PROMPT in the environment; stdout is the artifact.",
        stages = "s1|s2|s3|s4"
    )
}

#[derive(Default)]
struct Args {
    out: Option<PathBuf>,
    requirements: Option<PathBuf>,
    ingest: Option<String>,
    synthesize: Option<String>,
    annotate: Option<String>,
    from: Option<StageId>,
    to: Option<StageId>,
    max_rounds: Option<usize>,
    do_register: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut a = Args::default();
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        let mut value =
            |flag: &str| argv.next().ok_or_else(|| format!("{flag} needs a value\n{}", usage()));
        let stage = |flag: &str, v: String| {
            StageId::parse(&v).ok_or_else(|| format!("{flag}: {v:?} is not a stage\n{}", usage()))
        };
        match arg.as_str() {
            "--out" => a.out = Some(value("--out")?.into()),
            "--requirements" => a.requirements = Some(value("--requirements")?.into()),
            "--ingest" => a.ingest = Some(value("--ingest")?),
            // `--generator` is what the 2026-08-13 run record invoked; kept so
            // an existing harness keeps working against the split pipeline.
            "--synthesize" | "--generator" => a.synthesize = Some(value("--synthesize")?),
            "--annotate" => a.annotate = Some(value("--annotate")?),
            "--from" => a.from = Some(stage("--from", value("--from")?)?),
            "--to" => a.to = Some(stage("--to", value("--to")?)?),
            "--only" => {
                let s = stage("--only", value("--only")?)?;
                a.from = Some(s);
                a.to = Some(s);
            }
            "--max-rounds" => {
                let v = value("--max-rounds")?;
                a.max_rounds = Some(v.parse().map_err(|_| format!("--max-rounds: {v:?}"))?);
            }
            "--register" => a.do_register = true,
            "--print-prompt" => {
                let v = value("--print-prompt")?;
                let s = stage("--print-prompt", v)?;
                let prompt = s
                    .prompt()
                    .ok_or_else(|| format!("{} has no prompt: it is a check, not a producer", s))?;
                print!("{prompt}");
                return Ok(ExitCode::SUCCESS);
            }
            "-h" | "--help" => {
                println!("{}", usage());
                return Ok(ExitCode::SUCCESS);
            }
            other => return Err(format!("unknown argument {other:?}\n{}", usage())),
        }
    }

    let out_dir = a.out.clone().ok_or_else(usage)?;
    let workspace = Workspace::new(&out_dir);
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;

    // The range: what was asked for, or the smallest one the producers imply.
    // Defaulting to "every stage" would demand an ingest command from every
    // caller who only wanted to re-gate a directory.
    let first = a.from.unwrap_or_else(|| {
        if a.ingest.is_some() {
            StageId::Ingest
        } else if a.synthesize.is_some() {
            StageId::Synthesize
        } else if a.annotate.is_some() {
            StageId::Annotate
        } else {
            StageId::Validate
        }
    });
    let last = a.to.unwrap_or(StageId::Validate).min(StageId::Validate);

    let items = collect_items(first, &a, &workspace)?;
    if items.is_empty() {
        return Err(format!(
            "nothing to run: no item has an input artifact for {}\n{}",
            first.title(),
            usage()
        ));
    }

    let scratch = out_dir.join(".forge");
    let mut ingest = a.ingest.map(|c| CommandStage::new(StageId::Ingest, c, &scratch));
    let mut synthesize = a.synthesize.map(|c| CommandStage::new(StageId::Synthesize, c, &scratch));
    // S3 alone gets a second input: the brief S1 wrote, so the scope the
    // requirement states can be bound to the ai_authz S3 emits (D005 option C).
    // A run that starts at S3 over IDL with no brief beside it binds nothing.
    let mut annotate = a
        .annotate
        .map(|c| CommandStage::new(StageId::Annotate, c, &scratch).with_briefs(&workspace));
    let mut pipeline = Pipeline {
        ingest: ingest.as_mut().map(|s| s as &mut dyn orbweaver_forge::pipeline::Stage),
        synthesize: synthesize.as_mut().map(|s| s as &mut dyn orbweaver_forge::pipeline::Stage),
        annotate: annotate.as_mut().map(|s| s as &mut dyn orbweaver_forge::pipeline::Stage),
        first,
        last,
        max_rounds: a.max_rounds.unwrap_or(3),
    };

    println!(
        "range: {} → {} over {} item(s), {} repair round(s) allowed per stage",
        first.title(),
        last.title(),
        items.len(),
        pipeline.max_rounds
    );
    let report = run_pipeline(&mut pipeline, &workspace, &items).map_err(|e| e.to_string())?;

    // The §5.1 summary, per stage: batch size, first-pass rate and round count
    // stated separately, causes with affected counts, and a plain statement
    // when the rounds ran out — never only a final number.
    print!("{report}");
    for stage in &report.stages {
        for item in &stage.items {
            if let ItemStatus::Invalid { repair_prompt } = &item.status {
                println!("=== {} {}\n{repair_prompt}", stage.stage.tag(), item.id);
            }
        }
    }

    // Attribution, in one line: "S4 passed" says something different about a
    // file no annotation stage has seen.
    if let Some(s4) = report.stage(StageId::Validate) {
        let annotated = s4
            .items
            .iter()
            .filter(|i| {
                workspace
                    .gated_artifact(&i.id)
                    .is_some_and(|p| p.to_string_lossy().ends_with(".sidl.idl"))
            })
            .count();
        println!(
            "S4 gated {annotated} annotated file(s) and {} unannotated draft(s)",
            s4.items.len().saturating_sub(annotated)
        );
    }

    if !report.all_valid() {
        if a.do_register {
            println!("S5: skipped — registration requires every item valid");
        }
        return Ok(ExitCode::FAILURE);
    }
    if a.do_register {
        let Some(gated) = report.stage(StageId::Validate) else {
            return Err("--register needs S4 in the range: nothing has been gated".to_owned());
        };
        match register(gated, &out_dir) {
            Ok(registration) => {
                println!(
                    "S5: registered {} item(s); {} exposable interface(s), every one exposed=no",
                    gated.items.len(),
                    registration.exposable.len()
                );
                println!("S5: allowlist worksheet: {}", out_dir.join(EXPOSURE_TODO_FILE).display());
            }
            Err(e) => {
                eprintln!("S5 register: {e}");
                // A gate refusal means an item was not valid after all (1);
                // an unwritable worksheet means S5 could not run (2).
                return match e {
                    orbweaver_forge::pipeline::RegisterError::Io { .. } => Ok(ExitCode::from(2)),
                    _ => Ok(ExitCode::FAILURE),
                };
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The `(id, input)` pairs for the first stage in the range.
///
/// S1 and a brief-less S2 read the requirements directory; every later stage
/// reads the workspace, which is what makes "re-run from here" need nothing but
/// `--out`.
fn collect_items(
    first: StageId,
    args: &Args,
    workspace: &Workspace,
) -> Result<Vec<(String, String)>, String> {
    let from_requirements = || -> Result<Vec<(String, String)>, String> {
        let dir = args.requirements.as_ref().ok_or_else(|| {
            format!("{} reads natural-language requirements: pass --requirements", first.title())
        })?;
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "txt"))
            .collect();
        // Name order, so runs are comparable across machines.
        files.sort();
        if files.is_empty() {
            return Err(format!("{}: no .txt requirement files", dir.display()));
        }
        files
            .into_iter()
            .map(|path| {
                let id = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                Ok((id, text))
            })
            .collect()
    };

    let from_workspace = |stage: StageId| -> Result<Vec<(String, String)>, String> {
        workspace
            .ids_ready_for(stage)
            .into_iter()
            .map(|id| {
                let text = workspace.load(stage, &id).map_err(|e| e.to_string())?;
                Ok((id, text))
            })
            .collect()
    };

    match first {
        StageId::Ingest => from_requirements(),
        // S2 prefers a brief and falls back to prose. The fallback is the old
        // single-prompt pipeline, and the report says which it was by whether
        // an S1 stage ran — the coverage half of S2's gate is silent on prose.
        StageId::Synthesize => {
            let briefs = from_workspace(StageId::Synthesize)?;
            if briefs.is_empty() { from_requirements() } else { Ok(briefs) }
        }
        other => from_workspace(other),
    }
}
