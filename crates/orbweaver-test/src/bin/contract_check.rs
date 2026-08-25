//! `contract-check` — S7 at the command line.
//!
//! ```text
//! contract-check <file.idl>...              human-readable, in S4's format
//! contract-check --json <file.idl>...       one document for the whole batch
//! contract-check --cases N <file.idl>...    round-trip cases per type (default 32)
//! contract-check --seed 0x... <file.idl>...  an explicit batch seed
//! ```
//!
//! **Exit status: 0 unless a property test fails.** A byte instability is a
//! defect in code we wrote; an annotation smell is an opinion about prose
//! somebody else wrote, and the two must not share an exit code. 2 means the
//! run could not happen — a file that will not open or will not parse. A file
//! that does not compile is S4's business, and `sidl-validate` says so much
//! better than this could.
//!
//! The batch is the whole argument list, per §5.1: one report over every file,
//! grouped by rule, so a shared cause shows up as one cause with many sites
//! instead of many findings.

use std::collections::BTreeMap;

use orbweaver_dynamic::json::Json;
use orbweaver_forge::{Report, Severity};
use orbweaver_registry::Registry;
use orbweaver_test::prop::{DEFAULT_SEED, Measured};
use orbweaver_test::{check_source_measured, deferred_wire_gaps, has_defect};

const DEFAULT_CASES: usize = 32;

fn main() -> std::process::ExitCode {
    let mut json = false;
    let mut cases = DEFAULT_CASES;
    let mut seed: Option<u64> = None;
    let mut files: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--json" => json = true,
            "--cases" => match args.next().and_then(|v| v.parse().ok()) {
                Some(n) => cases = n,
                None => {
                    eprintln!("--cases needs a number");
                    return std::process::ExitCode::from(2);
                }
            },
            "--seed" => match args.next().as_deref().and_then(parse_seed) {
                Some(s) => seed = Some(s),
                None => {
                    eprintln!("--seed needs a number, decimal or 0x-prefixed hex");
                    return std::process::ExitCode::from(2);
                }
            },
            "-h" | "--help" => {
                println!(
                    "usage: contract-check [--json] [--cases N] [--seed S] <file.idl>...\n\
                     \n\
                     Property tests over every registered type (a failure is a defect) and\n\
                     contract advice over every SIDL annotation (never a failure).\n\
                     Exit 0 unless a property test fails."
                );
                return std::process::ExitCode::SUCCESS;
            }
            other => files.push(other.to_owned()),
        }
    }
    if files.is_empty() {
        eprintln!("usage: contract-check [--json] [--cases N] [--seed S] <file.idl>...");
        return std::process::ExitCode::from(2);
    }

    let mut reports: Vec<(String, Report, usize)> = Vec::new();
    let mut measured = Measured::default();
    // The v1 wire's refusals, S4's closure (docs/PLAN.md §4.4): every
    // declaration that is or carries a valuetype, an abstract interface or a
    // fixed. Counted here so the number of unservable declarations over a
    // corpus is on the line the harness reads, beside how many of them the
    // property half could not measure for that reason. The two differ, and
    // the difference is a fact worth printing: an interface has no TypeCode
    // to sample, and a valuetype the registry records as an object reference
    // *is* sampled — as a reference, which is not its wire form.
    let mut deferred_wire: usize = 0;
    for path in &files {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{path}: {e}");
                return std::process::ExitCode::from(2);
            }
        };
        // `check` rather than `parse`: an opinion about the meaning of a file
        // that is not yet semantically valid is worthless, and S4 has already
        // said everything useful about it.
        let spec = match orbweaver_idl::check(&src) {
            Ok(s) => s,
            Err(diags) => {
                // Not our verdict to give. S4 owns "this file is wrong", and
                // saying it twice in two formats is how two tools disagree.
                eprintln!(
                    "{path}: rejected by S4 ({} diagnostic(s)); run sidl-validate for the details",
                    diags.len()
                );
                return std::process::ExitCode::from(2);
            }
        };
        let mut registry = Registry::new();
        if let Err(e) = registry.load(&spec) {
            eprintln!("{path}: {e}");
            return std::process::ExitCode::from(2);
        }
        let types = registry.ids().filter(|id| registry.typecode(id).is_some()).count();
        // Both halves read the source: the SIDL version marker lands on a
        // module the registry keeps nothing for, and §4.4's closure follows
        // declarations the registry has already flattened into TypeCodes.
        deferred_wire += orbweaver_idl::deferred_wire_types(&spec).len();
        let (report, m) =
            check_source_measured(&spec, &registry, cases, seed.unwrap_or(DEFAULT_SEED));
        measured.add(m);
        reports.push((path.clone(), report, types));
    }

    let defects: usize = reports
        .iter()
        .flat_map(|(_, r, _)| &r.findings)
        .filter(|f| f.severity == Severity::Error)
        .count();
    let deferred_unmeasured: usize =
        reports.iter().map(|(_, r, _)| deferred_wire_gaps(r).len()).sum();

    if json {
        let files: Vec<Json> = reports
            .iter()
            .map(|(path, r, types)| {
                let Json::Object(mut m) = r.to_json() else { unreachable!("to_json is an object") };
                m.insert("file".into(), Json::String(path.clone()));
                m.insert("types".into(), Json::Number(types.to_string()));
                m.insert("ok".into(), Json::Bool(!has_defect(r)));
                Json::Object(m)
            })
            .collect();
        println!(
            "{}",
            Json::Object(BTreeMap::from([
                ("ok".to_owned(), Json::Bool(defects == 0)),
                ("defects".to_owned(), Json::Number(defects.to_string())),
                ("cases".to_owned(), Json::Number(cases.to_string())),
                // How much ran, not only what was found: a leg that silently
                // stopped running would otherwise print the same document.
                ("cdr_roundtrips".to_owned(), Json::Number(measured.cdr.to_string())),
                ("anyjson_crossings".to_owned(), Json::Number(measured.json.to_string())),
                // What the wire refuses (S4's §4.4 closure) and how many of
                // those the property half could not measure for that reason.
                ("deferred_wire".to_owned(), Json::Number(deferred_wire.to_string())),
                (
                    "deferred_wire_unmeasured".to_owned(),
                    Json::Number(deferred_unmeasured.to_string()),
                ),
                ("files".to_owned(), Json::Array(files)),
            ]))
        );
    } else {
        for (path, r, _) in &reports {
            for f in &r.findings {
                println!("{path}:{f}");
            }
        }
        let types: usize = reports.iter().map(|(_, _, t)| t).sum();
        let advice = reports
            .iter()
            .flat_map(|(_, r, _)| &r.findings)
            .filter(|f| f.severity != Severity::Error)
            .count();
        // The two counts are what ran, printed beside what was found. Until
        // 2026-08-19 the line said "× 2 byte orders" over a sweep that never
        // touched AnyJSON, and read exactly the same as one that did; the
        // ratio is here so that a JSON leg that stops running is a number
        // dropping to zero on the line the harness already reads.
        println!(
            "\n{} file(s), {types} type(s) × {cases} case(s) × 2 byte orders: {defects} \
             property defect(s), {} of {} CDR round trip(s) also taken across AnyJSON, {advice} \
             contract finding(s), {deferred_wire} declaration(s) the wire cannot carry (§4.4's \
             three, natives, and what CORBA withdrew) of which {deferred_unmeasured} unmeasured \
             by the property",
            reports.len(),
            measured.json,
            measured.cdr
        );
        if defects == 0 && advice > 0 {
            println!(
                "advice does not fail the run: S4 gates syntax and semantics, this gates meaning"
            );
        }
    }

    if defects == 0 { std::process::ExitCode::SUCCESS } else { std::process::ExitCode::FAILURE }
}

fn parse_seed(s: &str) -> Option<u64> {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}
