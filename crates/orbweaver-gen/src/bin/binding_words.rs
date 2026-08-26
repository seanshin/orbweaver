//! `binding-words --language <L> --idl <file>... [--allow <tsv>]`
//!
//! D032 §4 clause 5 — *"its keyword escaping is exercised by
//! `28-target-keywords.idl`"* — as an instrument, for one language at a time,
//! so `spikes/binding_suite.sh` can run it as that language's clause-5 cell.
//!
//! # What it decides
//!
//! For every word target `L`'s emitter reserves, the emitted code either
//! contains the escaped spelling — the escaping **ran** on that word — or it
//! does not. A word it did not run on must be named in the `--allow` file with
//! a reason. There is no third outcome, and that is what turns a clause that
//! read as satisfied-by-glob into one that goes red.
//!
//! # Why an allow file rather than a floor
//!
//! `A floor is not a figure`: pinning "at least 34 of 37 words covered" would
//! prove nothing about *which* three, would stay green when a covered word was
//! swapped for an uncovered one, and would have to be edited upward by hand.
//! Naming the exceptions instead makes the gate exact — adding a word to an
//! emitter's list without covering it goes red on the next run, and the file
//! says why each survivor is there and since when.
//!
//! The corpus file's own header already lists five classes measured **not** to
//! survive both emitters. That prose is a fact with no home: nothing reads it,
//! so nothing notices when it stops being true. The allow file is that fact's
//! home, and this binary is what compiles it.
//!
//! # Exit
//!
//! `0` every word accounted for · `1` a word is neither executed nor allowed,
//! or an allowed word is *no longer* uncovered · `2` bad usage or unknown
//! language.
//!
//! A stale allow row is a failure, not a shrug, for the reason
//! `differential.sh`'s staleness loop exists: an exception nobody removed is an
//! exception that stops describing the tree, and the next reader believes it.
//!
//! *절 5를 하나의 언어에 대해 재는 도구. 커버되지 않은 단어는 이유와 함께 허용
//! 파일에 이름이 있어야 하며, 더 이상 필요 없어진 허용 행은 실패다 — 아무도 지우지
//! 않은 예외는 트리를 설명하지 않게 된 뒤에도 읽히기 때문이다.*

use orbweaver_gen::targets;
use orbweaver_registry::Registry;

fn main() -> std::process::ExitCode {
    let mut language = String::new();
    let mut idl: Vec<String> = Vec::new();
    let mut allow: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--language" => language = args.next().unwrap_or_default(),
            "--allow" => allow = args.next(),
            "--idl" => idl.extend(args.by_ref()),
            other => {
                eprintln!("binding-words: unexpected argument {other:?}");
                return usage();
            }
        }
    }
    if language.is_empty() || idl.is_empty() {
        return usage();
    }

    // An unknown language is a failure and never an empty pass. The `bears_on`
    // lesson: a tag naming something the owning document does not have is a
    // FAILURE naming the group and the bad name, because the alternative is a
    // typo that measures nothing and says `ok`.
    let Some(t) = targets::target(&language) else {
        eprintln!(
            "binding-words: no target named {language:?}. This crate emits for: {}",
            targets::TARGETS.iter().map(|t| t.language).collect::<Vec<_>>().join(", ")
        );
        return std::process::ExitCode::from(2);
    };

    let mut registry = Registry::new();
    for path in &idl {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("binding-words: {path}: {e}");
                return std::process::ExitCode::from(2);
            }
        };
        let spec = match orbweaver_idl::parse(&src) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("binding-words: {path}: {e:?}");
                return std::process::ExitCode::from(2);
            }
        };
        if let Err(e) = registry.load(&spec) {
            eprintln!("binding-words: {path}: {e:?}");
            return std::process::ExitCode::from(2);
        }
    }

    let (hit, miss) = targets::keyword_coverage(t, &registry);

    // The reachability class, COMPUTED by asking the front end rather than
    // typed into the allow file's reason. A word that is also an IDL keyword can
    // only reach a contract through IDL's own leading-underscore escape
    // (`_struct` declares an identifier named `struct`), so it is harder to
    // cover but not impossible — the distinction a hand-written reason would
    // get wrong, and `a classifier is a sentence too` is why it is asked of
    // `orbweaver_idl::lex::is_keyword` instead.
    let class = |w: &str| {
        if orbweaver_idl::lex::is_keyword(w) {
            " (also an IDL keyword: reachable only through IDL's own `_` escape)"
        } else {
            ""
        }
    };

    // Allowed = word -> reason, for this language only. A row for another
    // language is not this run's business and is neither used nor complained
    // about; the file is shared so that one reader sees every exception at once.
    let mut allowed: Vec<(String, String)> = Vec::new();
    if let Some(path) = &allow {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        for line in text.lines() {
            let line = line.trim_end();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 3 {
                eprintln!("binding-words: {path}: not three tab-separated fields: {line:?}");
                return std::process::ExitCode::from(2);
            }
            if f[0] == language {
                allowed.push((f[1].to_owned(), f[2..].join(" ")));
            }
        }
    }

    println!(
        "note\t{language}: {} reserved word(s), {} executed by the contract set",
        hit.len() + miss.len(),
        hit.len()
    );

    let mut failures = 0usize;
    for w in &miss {
        match allowed.iter().find(|(a, _)| a == w) {
            Some((_, why)) => println!("note\t{language}: \"{w}\" not executed, allowed: {why}"),
            None => {
                println!(
                    "FAIL\t{language}: \"{w}\" is reserved and its escaped spelling \
                     \"{}\"{} appears nowhere in what the emitter wrote for the contract set. \
                     Either give it a home in the corpus or name it in the allow file with a reason.",
                    (t.escape)(w),
                    class(w)
                );
                failures += 1;
            }
        }
    }
    // The staleness half. An exception that is no longer needed is removed, not
    // left to describe a tree it has stopped describing.
    for (w, _) in &allowed {
        if hit.iter().any(|h| h == w) {
            println!(
                "FAIL\t{language}: \"{w}\" is named in the allow file but the contract set \
                 DOES exercise it now — delete the row rather than leave a stale exception"
            );
            failures += 1;
        } else if !miss.iter().any(|m| m == w) {
            println!(
                "FAIL\t{language}: \"{w}\" is named in the allow file but is not one of \
                 this target's reserved words at all — the emitter's list changed under it"
            );
            failures += 1;
        }
    }

    if failures == 0 { std::process::ExitCode::SUCCESS } else { std::process::ExitCode::from(1) }
}

fn usage() -> std::process::ExitCode {
    eprintln!("usage: binding-words --language <L> [--allow <tsv>] --idl <file.idl>...");
    std::process::ExitCode::from(2)
}
