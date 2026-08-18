//! `sidl-validate` — S4 at the command line and in the self-repair loop.
//!
//! ```text
//! sidl-validate [-I <dir>] <file.idl>...      human-readable
//! sidl-validate --json <file.idl>...          for tooling
//! sidl-validate --repair-prompt <file.idl>    to hand back to a generator
//! sidl-validate --against <released.idl> <proposed.idl>
//! ```
//!
//! `#include` is resolved before validation, which is what lets this be
//! pointed at a real estate rather than at one self-contained file. The quoted
//! form resolves against the including file's own directory, so a directory of
//! contracts that include each other needs no `-I` at all. Positions are
//! reported against the file the line was written in, never against the
//! spliced unit, because nobody has that file to open.
//!
//! Exit status: 0 clean, 1 rejected, 2 could not run. Advice and warnings do
//! not fail the run — a gate that blocks on advice is one people route around,
//! and then it blocks nothing.

use orbweaver_dynamic::json::Json;
use orbweaver_forge::{Report, Severity, validate_against, validate_unit};
use orbweaver_idl::include::{SearchPath, Unit, preprocess_file};

fn main() -> std::process::ExitCode {
    let mut json = false;
    let mut repair = false;
    let mut against: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut search = SearchPath::default();

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
            a if a.starts_with("-I") => {
                let dir = if a.len() > 2 {
                    a[2..].to_owned()
                } else {
                    match args.next() {
                        Some(d) => d,
                        None => {
                            eprintln!("-I needs a directory");
                            return std::process::ExitCode::from(2);
                        }
                    }
                };
                search.push(dir);
            }
            "-h" | "--help" => {
                println!(
                    "usage: sidl-validate [--json | --repair-prompt] [-I <dir>]... \
                     [--against <released.idl>] <file.idl>..."
                );
                return std::process::ExitCode::SUCCESS;
            }
            other => files.push(other.to_owned()),
        }
    }
    if files.is_empty() {
        eprintln!("usage: sidl-validate [--json | --repair-prompt] [-I <dir>]... <file.idl>...");
        return std::process::ExitCode::from(2);
    }

    // The released contract is resolved too, and for the same reason the
    // proposal is: `--against` compares two contracts, and comparing a resolved
    // proposal with an unresolved baseline reports every name the baseline's
    // headers declared as newly added. Read as a string it was the one file in
    // this tool that `#include` resolution had not reached.
    let baseline = match against.as_deref() {
        Some(path) => match preprocess_file(std::path::Path::new(path), &search) {
            Ok(u) if u.is_ok() => Some(u.text),
            Ok(u) => {
                for d in &u.errors {
                    eprintln!("{}", u.render(d));
                }
                return std::process::ExitCode::from(2);
            }
            Err(e) => {
                eprintln!("{path}: {e}");
                return std::process::ExitCode::from(2);
            }
        },
        None => None,
    };

    let mut rejected = 0usize;
    // The unit is kept beside its report: a `Finding` carries a line number
    // rather than a span, so the mapping back to the file that line was
    // written in has to survive the loop.
    let mut reports: Vec<(String, Unit, Report)> = Vec::new();
    for path in &files {
        let unit = match preprocess_file(std::path::Path::new(path), &search) {
            Ok(u) => u,
            Err(e) => {
                eprintln!("{path}: {e}");
                return std::process::ExitCode::from(2);
            }
        };
        // An unresolvable include is refused here rather than carried into
        // validation: every name the missing file declares would come back as
        // undeclared, which is ninety diagnostics about one missing -I.
        if !unit.is_ok() {
            for d in &unit.errors {
                println!("{}", unit.render(d));
            }
            rejected += 1;
            continue;
        }
        for a in &unit.advice {
            println!("advice: {}", unit.render(a));
        }
        // The unit, not its text: the guard directives of every included file
        // are still in there, and four `#ifndef` blocks in one string is
        // conditional compilation rather than an include guard.
        let report = match &baseline {
            Some(b) => validate_against(&unit.text, b),
            None => validate_unit(&unit),
        };
        if !report.is_ok() {
            rejected += 1;
        }
        reports.push((path.clone(), unit, report));
    }

    if json {
        // One document for the whole batch. The pipeline validates a set at a
        // time (§5.1), and a caller that has to concatenate per-file documents
        // will eventually get the concatenation wrong.
        let files: Vec<Json> = reports
            .iter()
            .map(|(path, _, r)| {
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
        for (path, _, r) in &reports {
            if !r.is_ok() {
                println!("=== {path}\n{}", r.repair_prompt());
            }
        }
    } else {
        for (_, unit, r) in &reports {
            for f in &r.findings {
                let at = unit.locate(orbweaver_idl::lex::Span {
                    start: 0,
                    end: 0,
                    line: f.line,
                    column: f.column,
                });
                println!("{}:{}:{}: {f}", at.file.display(), at.line, at.column);
            }
        }
        let advice = reports
            .iter()
            .flat_map(|(_, _, r)| &r.findings)
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
