//! What rule each rejection files under, for every file in `corpus/negative/`.
//!
//! # Why this exists
//!
//! A rule id is the key a consumer keys a fix hint on. `orbweaver-forge` reads
//! it and hands a generator an edit, so a rejection that files under the wrong
//! rule reaches the wrong sentence — or none. That happened, silently and in
//! the product: a malformed fixed-point literal filed under `parse` and never
//! received the `fixed-literal` hint written for exactly that input, and
//! **nothing was red, because no test asserted what any rejection files
//! under.** That sentence was written in `lex.rs` on 2026-08-24 and stayed true
//! for a day.
//!
//! This is the missing assertion, at the level the defect lives on: not "the
//! lexer classifies this message correctly" but "this file, which exists to be
//! rejected, is rejected under this rule". It is a table on purpose. A table is
//! the only shape in which *adding a file* forces someone to state what it
//! files under, and in which a diagnosis quietly changing rules is a diff.
//!
//! *규칙 이름은 소비자가 힌트를 거는 열쇠다. 어떤 거부가 어떤 규칙으로 접수되는지
//! 주장하는 테스트가 하나도 없었기 때문에, 잘못 접수된 거부는 자기 앞으로 쓰인
//! 힌트를 받지 못한 채 조용히 제품 안에 있었다.*
//!
//! # What this does not check
//!
//! Whether the hint keyed to the rule is *true of this rejection*. It cannot:
//! the hints live in `orbweaver-forge`, which depends on this crate. The
//! measurement is recorded in the corpus files' own headers — n05, n21, n27,
//! n28 and n29 each name the hint they receive and why it was written about
//! something else — and the check belongs beside the table it would check, in
//! `orbweaver-forge`'s corpus tests, against [`orbweaver_idl::rules::ALL`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use orbweaver_idl::include::SearchPath;
use orbweaver_idl::rules;

/// Every file in `corpus/negative/`, and the rules its diagnostics file under,
/// in the order they are reported.
///
/// A repeat is a repeat: `n19` is six diagnostics of one rule and the count is
/// part of what is pinned, because "six constants, six complaints" is the
/// property that made the file worth writing.
const EXPECTED: &[(&str, &[&str])] = &[
    ("inherited-scope-leak.idl", &[rules::UNKNOWN_NAME, rules::UNKNOWN_NAME]),
    ("n01-missing-semicolon.idl", &[rules::PARSE]),
    ("n02-identifier-clash.idl", &[rules::IDENTIFIER_CASE_CLASH]),
    ("n03-scope-clash.idl", &[rules::ENCLOSING_SCOPE_CLASH]),
    ("n04-unknown-type.idl", &[rules::UNKNOWN_NAME]),
    // Files under the same rule as n04 and is not the same diagnosis: the
    // message says `::CORBA::TypeCode` and the hint keyed to the rule says
    // `Module::TypeCode`. See the file's header.
    ("n05-unqualified-typecode.idl", &[rules::UNKNOWN_NAME]),
    ("n06-duplicate-member.idl", &[rules::DUPLICATE_DECLARATION]),
    ("n07-union-dup-label.idl", &[rules::DUPLICATE_UNION_LABEL]),
    ("n08-reserved-word.idl", &[rules::RESERVED_WORD]),
    ("n09-struct-scope-clash.idl", &[rules::ENCLOSING_SCOPE_CLASH]),
    ("n10-operation-name-clash.idl", &[rules::IDENTIFIER_CASE_CLASH]),
    ("n11-keyword-context.idl", &[rules::RESERVED_WORD]),
    ("n12-enum-member-case-clash.idl", &[rules::IDENTIFIER_CASE_CLASH]),
    ("n13-fixed-attribute.idl", &[rules::ANONYMOUS_TYPE_IN_SIGNATURE]),
    ("n14-fixed-parameter.idl", &[rules::ANONYMOUS_TYPE_IN_SIGNATURE]),
    ("n15-fixed-return.idl", &[rules::ANONYMOUS_TYPE_IN_SIGNATURE]),
    ("n16-anonymous-sequence-parameter.idl", &[rules::ANONYMOUS_TYPE_IN_SIGNATURE]),
    ("n17-void-attribute.idl", &[rules::VOID_IN_SIGNATURE]),
    ("n18-const-fixed-bounds.idl", &[rules::NOT_A_CONST_TYPE]),
    ("n19-const-value-class.idl", &[rules::CONST_VALUE_TYPE; 6]),
    ("n20-const-value-range.idl", &[rules::CONST_VALUE_RANGE; 7]),
    // Same rule as n18, different diagnosis, and the span differs with it: n18
    // spans the type and this spans the constant's name, so the hint written to
    // quote `fixed<3,1>` quotes `TOLERANCE`. See the file's header.
    ("n21-const-long-double.idl", &[rules::NOT_A_CONST_TYPE; 2]),
    ("n22-fixed-literal-shape.idl", &[rules::FIXED_LITERAL]),
    ("n23-fixed-literal-digits.idl", &[rules::FIXED_LITERAL]),
    ("n24-inherited-clash.idl", &[rules::INHERITED_CLASH]),
    ("n25-not-a-type.idl", &[rules::NOT_A_TYPE; 2]),
    ("n26-union-two-defaults.idl", &[rules::DUPLICATE_UNION_DEFAULT]),
    ("n27-const-divide-by-zero.idl", &[rules::CONST_VALUE_RANGE]),
    ("n28-const-string-over-bound.idl", &[rules::CONST_VALUE_RANGE]),
    ("n29-const-enumerator-other-enum.idl", &[rules::CONST_VALUE_TYPE]),
    ("n30-exception-as-type.idl", &[rules::NOT_A_TYPE]),
];

/// Rules this corpus does not produce, each with the reason it does not.
///
/// The list is the point. Without it, `every_rule_reaches_this_corpus` would be
/// a test nobody could keep green, and with it a new rule cannot be added
/// without someone writing down where it is measured — which is the gap that
/// left `inherited-clash`, `not-a-type` and `duplicate-union-default` with
/// hints no corpus file had ever executed until 2026-08-25.
const NOT_IN_THIS_CORPUS: &[(&str, &str)] = &[
    (
        rules::UNKNOWN_SCOPED_NAME,
        "no fix hint is keyed to it — the advice is in the message — and \
         `orbweaver-forge`'s corpus test requires a hint for every negative file's first \
         finding, so a file here would go red for the wrong reason. Covered by the unit \
         tests in `sema` and by `spikes/estate/`, where the shape was found",
    ),
    (
        rules::PRAGMA_UNKNOWN_NAME,
        "the oracle's behaviour over a `#pragma` naming an undeclared thing has not been \
         measured, and a corpus/negative file is a claim that omniidl rejects it. Covered \
         by the unit tests in `parse`",
    ),
    (
        rules::WIRE_DEFERRED_TYPE,
        "not a rejection: §4.4's closure is a separate list by default, and the files it \
         names are in `corpus/golden/` because the oracle accepts them",
    ),
    (rules::UNSUPPORTED_DIRECTIVE, "a preprocessor rule; measured by `corpus/include/`"),
    (rules::INCLUDE_MALFORMED, "a preprocessor rule; measured by `corpus/include/`"),
    (rules::INCLUDE_NOT_FOUND, "a preprocessor rule; measured by `corpus/include/`"),
    (rules::INCLUDE_CYCLE, "a preprocessor rule; measured by `corpus/include/`"),
    (rules::INCLUDE_UNGUARDED_REPEAT, "advice rather than a rejection; `corpus/include/`"),
    (
        rules::INCLUDE_UNREADABLE,
        "needs a file that resolves and then cannot be read, which is a permission the \
         corpus cannot carry in git; covered by a unit test in `include`",
    ),
];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/negative")
}

fn rules_of(path: &Path) -> Vec<String> {
    let (_unit, result) = orbweaver_idl::check_file(path, &SearchPath::new())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    match result {
        Ok(_) => Vec::new(),
        Err(ds) => ds.into_iter().map(|d| d.rule.to_owned()).collect(),
    }
}

fn on_disk() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir())
        .unwrap_or_else(|e| panic!("cannot read corpus/negative: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "idl"))
        .map(|p| p.file_name().unwrap_or_default().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(!names.is_empty(), "corpus/negative is empty");
    names
}

/// The corpus's own contract, restated here because this file would otherwise
/// pass by finding no diagnostics and expecting none.
#[test]
fn every_file_is_still_rejected() {
    let mut accepted = Vec::new();
    for name in on_disk() {
        if rules_of(&dir().join(&name)).is_empty() {
            accepted.push(name);
        }
    }
    assert!(accepted.is_empty(), "these must be rejected: {}", accepted.join(", "));
}

/// The table itself.
#[test]
fn every_rejection_files_under_the_rule_the_table_names() {
    let mut wrong = Vec::new();
    for (name, want) in EXPECTED {
        let got = rules_of(&dir().join(name));
        if got != *want {
            wrong.push(format!("{name}\n    table: {want:?}\n    front end: {got:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "a rejection changed the rule it files under, which changes the fix hint it \
         reaches:\n  {}",
        wrong.join("\n  ")
    );
}

/// A file added without a row is a file whose rule nobody stated.
#[test]
fn the_table_and_the_directory_hold_the_same_files() {
    let listed: BTreeSet<&str> = EXPECTED.iter().map(|(n, _)| *n).collect();
    let present: BTreeSet<String> = on_disk().into_iter().collect();
    let missing: Vec<&String> = present.iter().filter(|n| !listed.contains(n.as_str())).collect();
    let stale: Vec<&&str> = listed.iter().filter(|n| !present.contains(**n)).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "add a row naming the rule it files under: {missing:?}; rows for files that are \
         gone: {stale:?}"
    );
}

/// Every rule this front end can produce is either measured by a file here or
/// named, with a reason, in [`NOT_IN_THIS_CORPUS`].
///
/// This is the gate for the class that cost three hints: a rule with a fix hint
/// written for it and no corpus file to execute it. `inherited-clash`,
/// `not-a-type` and `duplicate-union-default` were all in that state, and one
/// of them was hiding a defect — writing the `not-a-type` file is what found
/// that an exception counted as a type here and did not for the oracle.
#[test]
fn every_rule_reaches_this_corpus_or_says_why_not() {
    let produced: BTreeSet<&str> = EXPECTED.iter().flat_map(|(_, rs)| rs.iter().copied()).collect();
    let excused: BTreeSet<&str> = NOT_IN_THIS_CORPUS.iter().map(|(r, _)| *r).collect();

    let unaccounted: Vec<&&str> =
        rules::ALL.iter().filter(|r| !produced.contains(**r) && !excused.contains(**r)).collect();
    assert!(
        unaccounted.is_empty(),
        "no corpus/negative file produces these, and nothing says why: {unaccounted:?}"
    );

    // Both halves, or the list becomes a place to park a rule that has since
    // acquired a file — and a stale exemption reads as coverage.
    let both: Vec<&&str> = excused.iter().filter(|r| produced.contains(**r)).collect();
    assert!(both.is_empty(), "a file produces these now — drop the exemption: {both:?}");

    let unknown: Vec<&&str> =
        produced.iter().chain(excused.iter()).filter(|r| !rules::ALL.contains(r)).collect();
    assert!(unknown.is_empty(), "not rule ids this front end publishes: {unknown:?}");
}
