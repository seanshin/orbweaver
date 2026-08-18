//! `sidl-infer` — S3i at the command line: SIDL **proposals** for interfaces
//! nobody here wrote.
//!
//! ```text
//! sidl-infer --idl <file.idl>...                       ingest through the facade, print subjects
//! sidl-infer --repository <ior-file>  --seed <id>...   ingest from a real foreign IR
//! sidl-infer ... --producer <cmd> --out <dir>          run the batch and write the artifacts
//! sidl-infer --print-prompt                            the prompt a measurement used
//! ```
//!
//! Without `--producer` it runs the whole deterministic half and says so: the
//! ingestion, the subjects, the worksheet, and the refusal an exposure decision
//! would meet. The model-facing numbers — first-pass rate, round count and
//! unknown rate — are then printed as **UNMEASURED**, because a batch with no
//! producer measured no producer. Substituting a scripted stand-in would put a
//! number where an absence belongs.
//!
//! **Exit status:** 0 when everything the run attempted was measured; 1 when a
//! stage ended with items still failing; 2 when the run could not happen.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use orbweaver_dynamic::json::Json;
use orbweaver_forge::infer::{
    self, INFERRED_TODO_FILE, InferStage, Proposal, exposure_refusal, subjects, worksheet,
};
use orbweaver_forge::pipeline::{BatchReport, ItemStatus, run_batch};
use orbweaver_giop::Ior;
use orbweaver_giop::server::Server;
use orbweaver_registry::ifr::{RepositoryServer, interface_ids};
use orbweaver_registry::ingest::{Limits, ingest};
use orbweaver_registry::{Entry, Registry, Strictness};

const TIMEOUT: Duration = Duration::from_secs(10);
const ROOT_KEY: &[u8] = b"InterfaceRepository";

fn main() -> std::process::ExitCode {
    match run() {
        Ok(true) => std::process::ExitCode::SUCCESS,
        Ok(false) => std::process::ExitCode::FAILURE,
        Err(e) => {
            eprintln!("sidl-infer: {e}");
            std::process::ExitCode::from(2)
        }
    }
}

struct Args {
    idl: Vec<String>,
    repository: Option<String>,
    seeds: Vec<String>,
    source: Option<String>,
    producer: Option<String>,
    out: Option<PathBuf>,
    max_rounds: usize,
    json: bool,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut a = Args {
        idl: Vec::new(),
        repository: None,
        seeds: Vec::new(),
        source: None,
        producer: None,
        out: None,
        max_rounds: 3,
        json: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--idl" => a.idl.push(value("--idl")?),
            "--repository" => a.repository = Some(value("--repository")?),
            "--seed" => a.seeds.push(value("--seed")?),
            "--source" => a.source = Some(value("--source")?),
            "--producer" => a.producer = Some(value("--producer")?),
            "--out" => a.out = Some(PathBuf::from(value("--out")?)),
            "--max-rounds" => {
                a.max_rounds =
                    value("--max-rounds")?.parse().map_err(|_| "--max-rounds needs a number")?
            }
            "--json" => a.json = true,
            "--print-prompt" => {
                print!("{}", infer::S3I_PROMPT);
                return Ok(None);
            }
            "-h" | "--help" => {
                println!(
                    "usage: sidl-infer [--idl <f.idl>]... [--repository <ior>] [--seed <id>]...\n\
                     \x20                 [--source <label>] [--producer <cmd>] [--out <dir>]\n\
                     \x20                 [--max-rounds N] [--json] [--print-prompt]\n\
                     \n\
                     Proposes SIDL annotations for ingested interfaces. Every proposal is\n\
                     marked inferred, carries its evidence, and is refused by the exposure\n\
                     check until a named human approves it. Without --producer the\n\
                     deterministic half runs and the model-facing numbers print as\n\
                     UNMEASURED."
                );
                return Ok(None);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if a.idl.is_empty() && a.repository.is_none() {
        return Err("nothing to ingest: pass --idl <file>... or --repository <ior-file>".into());
    }
    Ok(Some(a))
}

fn run() -> Result<bool, String> {
    let Some(args) = parse_args()? else { return Ok(true) };

    // ── ingest ──────────────────────────────────────────────────────────────
    let (repository, source, oracle) = match &args.repository {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let ior = Ior::parse(text.trim()).map_err(|e| format!("{path}: {e}"))?;
            let label = args.source.clone().unwrap_or_else(|| format!("file://{path}"));
            (ior, label, "FOREIGN — their repository, our client")
        }
        None => {
            let local = load_idl(&args.idl)?;
            let ior = self_facade(local)?;
            let label = args.source.clone().unwrap_or_else(|| "ifr://self".to_owned());
            (
                ior,
                label,
                "OUR OWN facade over the given IDL — a SELF-CONSISTENCY stand-in for a \
                 foreign IR, not a cross-ORB claim",
            )
        }
    };

    let seeds = if args.seeds.is_empty() {
        let local = load_idl(&args.idl)?;
        interface_ids(&local)
    } else {
        args.seeds.clone()
    };
    if seeds.is_empty() {
        return Err("no seed interfaces: the IDL declares none and --seed named none".into());
    }

    println!("repository: {oracle}");
    println!("provenance label: {source}");
    let mut registry = Registry::new();
    let report = ingest(&mut registry, &repository, &seeds, &source, &Limits::default(), TIMEOUT)
        .map_err(|e| e.to_string())?;
    println!(
        "ingested {} interface(s), {} type(s); {} refusal(s), {} advisory note(s)",
        report.interfaces.len(),
        registry.ids().filter(|id| registry.typecode(id).is_some()).count(),
        report.refused.len(),
        report.advisories.len()
    );
    for refusal in &report.refused {
        println!("  refused {refusal}");
    }

    let subjects = subjects(&registry);
    if subjects.is_empty() {
        return Err(
            "nothing ingested, so there is nothing to annotate — an unmeasured check is a \
                    failure, not a pass"
                .into(),
        );
    }
    println!("\n{} ingested interface(s) to annotate:", subjects.len());
    let mut silent_total = 0usize;
    let mut op_total = 0usize;
    for s in &subjects {
        let silent = s.operations.iter().filter(|o| o.silent).count();
        silent_total += silent;
        op_total += s.operations.len();
        println!(
            "  {} — {} operation(s), {silent} whose verb the checker does not recognise",
            s.id,
            s.operations.len()
        );
    }
    println!(
        "\nEvidence floor (deterministic, no model): {silent_total}/{op_total} operation(s) \
         ({:.0}%) carry a name whose verb the checker's word list does not contain. A word list \
         is evidence of presence and never of absence, so an effect claimed on one of those is \
         kept — and marked `unrecognised-verb`, which is where a reviewer should look first.",
        percent(silent_total, op_total)
    );

    // ── the batch ───────────────────────────────────────────────────────────
    let items: Vec<(String, String)> =
        subjects.iter().map(|s| (s.id.clone(), s.to_json().to_string())).collect();

    let Some(command) = args.producer.clone() else {
        println!(
            "\nNo --producer, so no model ran.\n\
             first-pass rate: UNMEASURED\nround count:     UNMEASURED\nunknown rate:    UNMEASURED\n\
             These are numbers about a producer, and there was no producer. A scripted \
             stand-in would put a figure where an absence belongs."
        );
        print_refusals(&registry, args.out.as_deref())?;
        return Ok(true);
    };

    let scratch = args
        .out
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join(format!("sidl-infer-{}", std::process::id())));
    std::fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
    let mut stage = InferStage::new(command, scratch.join("scratch"));
    let batch = run_batch(&mut stage, &items, args.max_rounds);
    println!("\n{batch}");

    // ── apply, and report the unknown rate ──────────────────────────────────
    let mut annotated = Registry::new();
    let mut proposals: BTreeMap<String, Proposal> = BTreeMap::new();
    for item in &batch.items {
        let write = |name: String, body: &str| {
            std::fs::write(scratch.join(name), body)
                .map_err(|e| format!("{}: {e}", scratch.display()))
        };
        match (&item.output, &item.status) {
            (Some(text), ItemStatus::Valid) => {
                let proposal = Proposal::parse(text).map_err(|e| format!("{}: {e}", item.id))?;
                write(format!("{}.proposal.json", safe(&item.id)), &proposal.to_text())?;
                proposals.insert(item.id.clone(), proposal);
            }
            // A failed batch you can inspect beats one you cannot. The rejected
            // artifact and the prompt that would have opened the next round are
            // both kept, because clustering a batch by root cause means reading
            // what the failures actually said.
            (output, status) => {
                if let Some(text) = output {
                    write(format!("{}.rejected.json", safe(&item.id)), text)?;
                }
                if let ItemStatus::Invalid { repair_prompt } = status {
                    write(format!("{}.repair.txt", safe(&item.id)), repair_prompt)?;
                }
            }
        }
    }
    // Rebuilt rather than patched: `define_ingested` refuses to replace an
    // entry, whatever its provenance, and that refusal is load-bearing.
    for id in registry.ids().cloned().collect::<Vec<_>>() {
        let Some(entry) = registry.get(&id) else { continue };
        let entry = match (entry, proposals.get(&id)) {
            (Entry::Interface(iface), Some(p)) => Entry::Interface(infer::apply(iface, p)),
            (other, _) => other.clone(),
        };
        annotated.define_ingested(id.clone(), entry, &source).map_err(|e| format!("{id}: {e}"))?;
    }

    let (unknown, total) = proposals.values().fold((0usize, 0usize), |(u, t), p| {
        (u + p.inferences.iter().filter(|i| i.is_unknown()).count(), t + p.inferences.len())
    });
    println!(
        "unknown rate: {unknown}/{total} operation(s) ({:.0}%) — the stage declined to guess an \
         effect. Reported beside the first-pass rate, never instead of it.",
        percent(unknown, total)
    );

    print_refusals(&annotated, Some(&scratch))?;

    if args.json {
        println!("{}", summary_json(&batch, unknown, total));
    }
    Ok(batch.all_valid())
}

fn print_refusals(registry: &Registry, out: Option<&std::path::Path>) -> Result<(), String> {
    let blockers = infer::unapproved(registry);
    println!(
        "\nawaiting a human: {} operation(s) across {} interface(s)",
        blockers.len(),
        subjects(registry).len()
    );
    for s in subjects(registry) {
        if let Some(why) = exposure_refusal(registry, &s.id) {
            println!("  EXPOSURE REFUSED — {why}");
        }
    }
    let sheet = worksheet(registry);
    match out {
        Some(dir) => {
            let path = dir.join(INFERRED_TODO_FILE);
            std::fs::write(&path, &sheet).map_err(|e| format!("{}: {e}", path.display()))?;
            println!("worksheet: {}", path.display());
        }
        None => print!("\n{sheet}"),
    }
    Ok(())
}

fn summary_json(batch: &BatchReport, unknown: usize, total: usize) -> Json {
    Json::Object(BTreeMap::from([
        ("stage".to_owned(), Json::String("s3i".to_owned())),
        ("items".to_owned(), Json::Number(batch.items.len().to_string())),
        ("first_pass_valid".to_owned(), Json::Number(batch.first_pass_valid.to_string())),
        ("rounds_used".to_owned(), Json::Number(batch.rounds_used.to_string())),
        ("unknown".to_owned(), Json::Number(unknown.to_string())),
        ("operations".to_owned(), Json::Number(total.to_string())),
        ("all_valid".to_owned(), Json::Bool(batch.all_valid())),
    ]))
}

fn percent(n: usize, d: usize) -> f64 {
    if d == 0 { 0.0 } else { n as f64 * 100.0 / d as f64 }
}

fn safe(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

/// Builds the local registry S3i ingests through, `#include`s resolved.
///
/// `orbweaver_idl::check` is the *string* entry point and cannot resolve a
/// relative `#include`; this is the same call through the path entry point, at
/// the same strictness. The `--idl` paths are what somebody points at their own
/// estate, and an estate is the one shape that is never a single self-contained
/// file — inferring annotations for an interface whose base went missing would
/// propose a contract for less than the interface actually has.
fn load_idl(paths: &[String]) -> Result<Registry, String> {
    orbweaver_registry::registry_from_files(
        paths,
        &orbweaver_idl::SearchPath::new(),
        Strictness::Checked,
    )
    .map_err(|e| e.message)
}

/// Stands the project's own IR facade up on loopback and returns its reference.
///
/// A self-consistency stand-in, labelled as one everywhere it prints: it
/// measures our encoder against our decoder. What it *does* faithfully
/// reproduce is the thing S3i exists for — the entries come back
/// `Origin::Ingested` with an empty annotation map, because the wire carries no
/// annotations, whoever is on the other end of it.
fn self_facade(registry: Registry) -> Result<Ior, String> {
    let server = Server::bind("127.0.0.1:0", ROOT_KEY.to_vec()).map_err(|e| e.to_string())?;
    let port = server.local_addr().map_err(|e| e.to_string())?.port();
    let mut facade = RepositoryServer::new("127.0.0.1", port, ROOT_KEY.to_vec(), registry);
    let root = facade.root_ior();
    std::thread::spawn(move || {
        let _ = server.serve(&mut facade, || false);
    });
    Ok(root)
}
