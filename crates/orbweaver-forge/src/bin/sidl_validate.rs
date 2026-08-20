//! `sidl-validate` — S4 at the command line and in the self-repair loop.
//!
//! ```text
//! sidl-validate [-I <dir>] <file.idl>...      human-readable
//! sidl-validate --json <file.idl>...          for tooling
//! sidl-validate --repair-prompt <file.idl>    to hand back to a generator
//! sidl-validate --against <released.idl> <proposed.idl>
//! sidl-validate --wire v1 <file.idl>...        refuse what the v1 wire cannot carry
//! ```
//!
//! `#include` is resolved before validation, which is what lets this be
//! pointed at a real estate rather than at one self-contained file. The quoted
//! form resolves against the including file's own directory, so a directory of
//! contracts that include each other needs no `-I` at all. Positions are
//! reported against the file the line was written in, never against the
//! spliced unit, because nobody has that file to open.
//!
//! **All three output forms, not one.** That sentence was true only of the
//! human form: `--json` and `--repair-prompt` served the splice's line under
//! the root file's name, and `--repair-prompt` is read by a model. See
//! [`located_json`] for the JSON contract and what it means for a reader.
//! A position is printed **once**, by [`Finding::rendered_at`] — this printer
//! used to prefix the located position in front of `Display`'s own, so every
//! diagnostic carried two, the second of them a line in the splice
//! (`evo-proposed.idl:1:0: 0:0: error: …`). A finding about the file as a
//! whole — every `evolution/*` finding is one, and it carries `line == 0` —
//! prints as the file with **no** line, rather than as line 1, which is where
//! feeding a 0 to a span-mapper puts it.
//!
//! `--against` resolves **both** contracts and compares the two resolved
//! units. It used to resolve both and then hand the two splices back to a
//! string entry point, which preprocessed each of them a second time — and a
//! splice carries the `#ifndef` of every header inside it, which the second
//! pass reads as conditional compilation. So the §5.3 comparison over a
//! guarded multi-file contract never ran; it was refused with
//! `unsupported-directive` at a header's line 1. *비교는 스플라이스가 아니라
//! 유닛끼리 한다.*
//!
//! Exit status: 0 clean, 1 rejected, 2 could not run. Advice and warnings do
//! not fail the run — a gate that blocks on advice is one people route around,
//! and then it blocks nothing.
//!
//! `--wire v1` is the one flag that moves a finding between those classes:
//! `valuetype`, abstract interfaces and `fixed` are warnings by default (the
//! oracle accepts them and so must the default form over `corpus/golden/`) and
//! refusals under it, with the §4.4 reason and the edit. `forge-pipeline`'s S4
//! stage uses the refusing form; see `orbweaver_forge::WireGate` for why the
//! two defaults differ.

use orbweaver_dynamic::json::Json;
use orbweaver_forge::{
    Report, Severity, WireGate, locate_findings, validate_unit_against_for, validate_unit_for,
    written_in,
};
use orbweaver_idl::include::{SearchPath, Unit, preprocess_file};

/// The report as a machine reader gets it: every position mapped to the file it
/// was written in, and the file named beside the line when that is not the root.
///
/// **The contract this settles.** `--json` used to emit the *splice's* line
/// under the root file's name — `{"file": ".../root.idl", …, "line": 8}` for an
/// operation written at `00-audit.idl:5`. Both halves were present and the pair
/// was false, which is the worst of the three available answers.
///
/// `line` and `column` now mean what every other surface in this project
/// already means by them: the position **in the file the text was written in**.
/// That is not a new convention — [`orbweaver_forge::locate_findings`] has
/// mapped the library's reports since `#include` landed, and the human printer
/// of this binary has mapped its own since. The JSON path was the one place
/// holding the third convention, and this removes it rather than documenting it.
///
/// Since one report can now carry lines from several files, and the
/// report-level `file` can only name one, a finding written **elsewhere** names
/// its own file in a per-finding `"file"`. It is emitted *only* when it differs
/// from the report's — so a self-contained contract's document is byte-for-byte
/// what it was, and absent means "the file this report is about". Every
/// consumer of a single-file contract is untouched; a consumer of a multi-file
/// one was reading a false position before and now reads a true one.
///
/// *줄 번호는 스플라이스가 아니라 작성된 파일의 줄이다. 그 파일이 루트와 다르면
/// 소견마다 파일을 함께 적는다 — 같을 때는 적지 않으므로 단일 파일 출력은 그대로다.*
fn located_json(path: &str, unit: &Unit, report: &Report) -> Json {
    // Taken before the mapping, because mapping is what overwrites the line
    // this reads.
    let places: Vec<Option<String>> = report
        .findings
        .iter()
        .map(|f| {
            written_in(f, unit)
                .filter(|at| Some(at.file) != unit.files.first().map(|r| r.as_path()))
                .map(|at| at.file.display().to_string())
        })
        .collect();

    let mut mapped = report.clone();
    locate_findings(&mut mapped, unit);
    let Json::Object(mut m) = mapped.to_json() else { unreachable!("to_json is an object") };
    if let Some(Json::Array(findings)) = m.get_mut("findings") {
        for (finding, place) in findings.iter_mut().zip(&places) {
            if let (Json::Object(o), Some(file)) = (finding, place) {
                o.insert("file".into(), Json::String(file.clone()));
            }
        }
    }
    m.insert("file".into(), Json::String(path.to_owned()));
    Json::Object(m)
}

fn main() -> std::process::ExitCode {
    let mut json = false;
    let mut repair = false;
    let mut against: Option<String> = None;
    let mut wire = WireGate::Deferred;
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
            "--wire" => match args.next().as_deref().and_then(WireGate::parse) {
                Some(w) => wire = w,
                None => {
                    eprintln!("--wire needs `v1` or `deferred`");
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
                     [--against <released.idl>] [--wire v1|deferred] <file.idl>...\n\
                     \n\
                     --wire v1 refuses a contract that reaches a valuetype, an abstract\n\
                     interface or a fixed (docs/PLAN.md §4.4); by default those warn."
                );
                return std::process::ExitCode::SUCCESS;
            }
            other => files.push(other.to_owned()),
        }
    }
    if files.is_empty() {
        eprintln!(
            "usage: sidl-validate [--json | --repair-prompt] [-I <dir>]... [--wire v1|deferred] \
             <file.idl>..."
        );
        return std::process::ExitCode::from(2);
    }

    // The released contract is resolved too, and for the same reason the
    // proposal is: `--against` compares two contracts, and comparing a resolved
    // proposal with an unresolved baseline reports every name the baseline's
    // headers declared as newly added. Read as a string it was the one file in
    // this tool that `#include` resolution had not reached.
    //
    // The `Unit` is kept, not its `.text`: handing the splice back to a
    // string entry point preprocesses it a second time, and the second pass
    // sees the `#ifndef` of every header the first pass spliced in. A guard
    // that is not the first directive of the text it sits in is conditional
    // compilation, which this front end refuses — so every guarded
    // multi-file contract was refused here and the §5.3 comparison never ran.
    let baseline = match against.as_deref() {
        Some(path) => match preprocess_file(std::path::Path::new(path), &search) {
            Ok(u) if u.is_ok() => Some(u),
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
        // The unit, not its text — on both sides of the comparison. The guard
        // directives of every included file are still in the splice, and four
        // `#ifndef` blocks in one string is conditional compilation rather than
        // an include guard. Neither entry point maps positions, which is what
        // lets the one printer below do it for both.
        let report = match &baseline {
            Some(b) => validate_unit_against_for(&unit, b, wire),
            None => validate_unit_for(&unit, wire),
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
        let files: Vec<Json> =
            reports.iter().map(|(path, unit, r)| located_json(path, unit, r)).collect();
        println!(
            "{}",
            Json::Object(std::collections::BTreeMap::from([
                ("ok".to_owned(), Json::Bool(rejected == 0)),
                ("rejected".to_owned(), Json::Number(rejected.to_string())),
                ("files".to_owned(), Json::Array(files)),
            ]))
        );
    } else if repair {
        // Mapped, for the same reason `--json` is, and with more at stake: this
        // text is read by a *model*, and `forge-pipeline` hands its equivalent
        // to the producer as argv[2] (`spikes/e2e/producer.sh`). A splice line
        // under the root's name sends the repair to the wrong file with full
        // confidence, and nothing goes red. The library path was already right
        // (`validate_source_for` maps); this is the command catching up.
        for (path, unit, r) in &reports {
            if !r.is_ok() {
                let mut mapped = r.clone();
                locate_findings(&mut mapped, unit);
                println!("=== {path}\n{}", mapped.repair_prompt());
            }
        }
    } else {
        for (_, unit, r) in &reports {
            for f in &r.findings {
                // `written_in` rather than `Unit::locate`, because a finding is
                // not a span: `line == 0` is "about the file as a whole" and
                // `locate` clamps it to 1, inventing a position in the root.
                // And `rendered_at` rather than `{f}`, because `Display` writes
                // its own `line:column` — prefixing the located one printed
                // every position twice.
                let place = match written_in(f, unit) {
                    Some(at) => format!("{}:{}:{}", at.file.display(), at.line, at.column),
                    None => unit.files[0].display().to_string(),
                };
                println!("{}", f.rendered_at(&place));
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
