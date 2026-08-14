//! Checks IDL files and reports diagnostics.
//!
//! Replaces `spikes/idl_lint.py`, which approximated the identifier rules with
//! regexes and missed a new shape of them twice. This walks a real scope tree,
//! so the rule is expressed once rather than re-approximated per syntax form —
//! and it also catches what the regex never could: unknown names, duplicate
//! declarations, inherited collisions and repeated union labels.
//!
//! ```text
//! idl-check [-I <dir>]... <file.idl>...
//! idl-check -E [-I <dir>]... <file.idl>      # print the resolved unit
//! ```
//!
//! Each file is a **translation unit**, not a line of text: its `#include`s
//! are resolved, `-I` adds a directory to the search path the way `cc -I` and
//! `omniidl -I` do, and a diagnostic is reported against the file it was
//! written in with the include chain underneath it. Before this the front end
//! skipped `#include` entirely, which made every file of a real estate fail on
//! names that were declared one file away.
//!
//! `-E` writes the spliced unit to stdout and checks nothing, the way `cpp -P`
//! and `omniidl -E` do. It exists so that the tools which take one file of IDL
//! — `repository-ids`, `gen-corpus`, the console — can be pointed at a
//! multi-file estate today without each of them growing an include resolver.
//! The output is not the concatenation of the files: `#pragma prefix` is reset
//! at each file boundary and restored after it, which is the difference between
//! a repository id that is right and one that is merely well-formed.
//!
//! Exit code is the number of files with diagnostics, capped at 255.

use std::path::{Path, PathBuf};

use orbweaver_idl::SearchPath;

fn main() -> std::process::ExitCode {
    let mut search = SearchPath::new();
    let mut files: Vec<PathBuf> = Vec::new();
    let mut preprocess_only = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if let Some(rest) = a.strip_prefix("-I") {
            let dir = if rest.is_empty() { args.next() } else { Some(rest.to_owned()) };
            match dir {
                Some(d) => {
                    search.push(d);
                }
                None => {
                    eprintln!("-I needs a directory");
                    return std::process::ExitCode::from(2);
                }
            }
            continue;
        }
        match a.as_str() {
            "-E" => preprocess_only = true,
            "-h" | "--help" => {
                println!("usage: idl-check [-E] [-I <dir>]... <file.idl>...");
                return std::process::ExitCode::SUCCESS;
            }
            _ => files.push(PathBuf::from(a)),
        }
    }
    if files.is_empty() {
        eprintln!("usage: idl-check [-E] [-I <dir>]... <file.idl>...");
        return std::process::ExitCode::from(2);
    }

    if preprocess_only {
        return emit(&files, &search);
    }

    let mut bad = 0u8;
    for f in &files {
        match check(f, &search) {
            Ok(true) => {}
            Ok(false) => bad = bad.saturating_add(1),
            Err(e) => {
                println!("{}: cannot read: {e}", f.display());
                bad = bad.saturating_add(1);
            }
        }
    }
    std::process::ExitCode::from(bad)
}

/// Writes the resolved units to stdout, and nothing else to it.
///
/// Diagnostics go to stderr so the output stays pipeable. An unresolved
/// `#include` still fails the run rather than emitting a unit with a hole in
/// it: a downstream tool reading a silently incomplete unit is exactly the
/// failure this whole change is about.
fn emit(files: &[PathBuf], search: &SearchPath) -> std::process::ExitCode {
    let mut bad = 0u8;
    for f in files {
        match orbweaver_idl::preprocess_file(f, search) {
            Ok(unit) => {
                for a in &unit.advice {
                    eprintln!("advice: {}", unit.render(a));
                }
                if unit.is_ok() {
                    print!("{}", unit.text);
                } else {
                    for d in &unit.errors {
                        eprintln!("{}", unit.render(d));
                    }
                    bad = bad.saturating_add(1);
                }
            }
            Err(e) => {
                eprintln!("{}: cannot read: {e}", f.display());
                bad = bad.saturating_add(1);
            }
        }
    }
    std::process::ExitCode::from(bad)
}

/// Checks one unit. `Ok(true)` when it is clean of errors.
fn check(path: &Path, search: &SearchPath) -> std::io::Result<bool> {
    let (unit, result) = orbweaver_idl::check_file(path, search)?;
    // Advice never decides the exit code — a gate that blocks on advice is one
    // people route around — but it is printed, because the two things it says
    // (an include cycle, a re-inclusion a deployed compiler would reject) are
    // both invisible in the output otherwise.
    for a in &unit.advice {
        println!("advice: {}", unit.render(a));
    }
    match result {
        Ok(_) => Ok(true),
        Err(diags) => {
            for d in &diags {
                println!("{}", unit.render(d));
            }
            Ok(false)
        }
    }
}
