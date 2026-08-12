//! `sidl-validate` — S4 at the command line and in the self-repair loop.
//!
//! ```text
//! sidl-validate <file.idl>...                 human-readable
//! sidl-validate --json <file.idl>...          for tooling
//! sidl-validate --repair-prompt <file.idl>    to hand back to a generator
//! sidl-validate --against <released.idl> <proposed.idl>
//! ```
//!
//! Exit status: 0 clean, 1 rejected, 2 could not run. Advice and warnings do
//! not fail the run — a gate that blocks on advice is one people route around,
//! and then it blocks nothing.

use orbweaver_dynamic::json::Json;
use orbweaver_forge::{Report, Severity, validate, validate_against};

fn main() -> std::process::ExitCode {
    let mut json = false;
    let mut repair = false;
    let mut against: Option<String> = None;
    let mut files: Vec<String> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--json" => json = true,
            "--repair-prompt" => repair = true,
            "--against" => match args.next() {
                Some(f) => against = Some(f),
                None => {
                    eprintln!("--against needs a released .idl file");
                    return std::process::ExitCode::from(2);
                }
            },
            "-h" | "--help" => {
                println!(
                    "usage: sidl-validate [--json | --repair-prompt] \
                     [--against <released.idl>] <file.idl>..."
                );
                return std::process::ExitCode::SUCCESS;
            }
            other => files.push(other.to_owned()),
        }
    }
    if files.is_empty() {
        eprintln!("usage: sidl-validate [--json | --repair-prompt] <file.idl>...");
        return std::process::ExitCode::from(2);
    }

    let baseline = match against.as_deref().map(std::fs::read_to_string) {
        Some(Ok(s)) => Some(s),
        Some(Err(e)) => {
            eprintln!("{}: {e}", against.unwrap_or_default());
            return std::process::ExitCode::from(2);
        }
        None => None,
    };

    let mut rejected = 0usize;
    let mut reports: Vec<(String, Report)> = Vec::new();
    for path in &files {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{path}: {e}");
                return std::process::ExitCode::from(2);
            }
        };
        let report = match &baseline {
            Some(b) => validate_against(&src, b),
            None => validate(&src),
        };
        if !report.is_ok() {
            rejected += 1;
        }
        reports.push((path.clone(), report));
    }

    if json {
        // One document for the whole batch. The pipeline validates a set at a
        // time (§5.1), and a caller that has to concatenate per-file documents
        // will eventually get the concatenation wrong.
        let files: Vec<Json> = reports
            .iter()
            .map(|(path, r)| {
                let Json::Object(mut m) = r.to_json() else { unreachable!("to_json is an object") };
                m.insert("file".into(), Json::String(path.clone()));
                Json::Object(m)
            })
            .collect();
        println!(
            "{}",
            Json::Object(std::collections::BTreeMap::from([
                ("ok".to_owned(), Json::Bool(rejected == 0)),
                ("rejected".to_owned(), Json::Number(rejected.to_string())),
                ("files".to_owned(), Json::Array(files)),
            ]))
        );
    } else if repair {
        for (path, r) in &reports {
            if !r.is_ok() {
                println!("=== {path}\n{}", r.repair_prompt());
            }
        }
    } else {
        for (path, r) in &reports {
            for f in &r.findings {
                println!("{path}:{f}");
            }
        }
        let advice = reports
            .iter()
            .flat_map(|(_, r)| &r.findings)
            .filter(|f| f.severity != Severity::Error)
            .count();
        println!(
            "\n{} file(s): {} rejected, {} non-blocking finding(s)",
            reports.len(),
            rejected,
            advice
        );
    }

    if rejected == 0 { std::process::ExitCode::SUCCESS } else { std::process::ExitCode::FAILURE }
}
