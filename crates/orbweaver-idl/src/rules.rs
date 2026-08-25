//! Every rule id this front end can file a rejection under, and what each one
//! means.
//!
//! # Why a rule id has a home
//!
//! A rule id is not a label on a diagnostic; it is the **key a consumer keys a
//! fix hint on**. `orbweaver-forge::fix_for` matches on it and returns the edit
//! a generator is handed verbatim, so the id decides which sentence the
//! self-repair loop reads. That makes it a fact with two ends, and CLAUDE.md's
//! rule about facts applies: it has one home, and every other layer reaches it
//! from here rather than retyping it.
//!
//! It had no home until 2026-08-25. Twenty-three construction sites spelled
//! their ids as string literals and one consumer retyped fifteen of them, so
//! nothing could go red on a rename, on a typo, or on a hint keyed to a rule
//! nothing produces. The class had already been caught once inside a single
//! function — `LexError::rule` classified by a retyped prefix that one of its
//! own three sites did not carry, and a malformed fixed-point literal lost the
//! hint written for it — and the fix there was the shared constant
//! [`crate::lex::FIXED_LITERAL_SUBJECT`]. This is the same fix one level up.
//!
//! *규칙 이름은 진단에 붙은 꼬리표가 아니라 **소비자가 수정 힌트를 거는 열쇠**다.
//! 그래서 사실이며, 집은 하나다.*
//!
//! # What a rule id promises
//!
//! **One rule, one diagnosis.** A hint is written for the diagnosis, not for
//! the rule's name, so a second diagnosis filed under an existing rule silently
//! inherits a sentence written about something else. Measured 2026-08-25 across
//! `corpus/negative/`: five diagnoses were sharing a rule with a diagnosis they
//! do not resemble, and two of them were losing in the product — `n05` was told
//! to write `Module::TypeCode` by a hint while the message beside it said
//! `::CORBA::TypeCode`, and `n21` was told that "the floating-point types" are
//! admitted as a constant's type by the hint that fired *because* its `long
//! double` was refused.
//!
//! So each constant below documents the **single** diagnosis it names. Adding a
//! diagnosis under an existing id means the hint keyed to that id has to be
//! true of both, which is a claim to check rather than a coincidence to hope
//! for.
//!
//! *한 규칙에 한 진단. 힌트는 규칙 이름이 아니라 진단을 보고 쓰였기 때문이다.*
//!
//! # The span is part of the contract too
//!
//! A consumer slices the source with the diagnostic's span and quotes the
//! result inside the hint, so what a rule's span covers is part of what the
//! rule promises. [`NOT_A_CONST_TYPE`]'s two parser sites span the *type* and
//! its analyser site spans the constant's *name*, which is why the hint written
//! to say "`fixed<3,1>` is not a const_type" prints "`TOLERANCE` is not a
//! const_type" over `corpus/negative/n21`. Where a rule's sites disagree about
//! the span, the doc comment says so.

/// The catch-all for a syntax failure with no unambiguous edit.
///
/// Deliberately hintless. The cause of a missing separator is wherever the
/// grammar noticed, which is not reliably where the edit belongs, and a
/// confident wrong instruction costs a self-repair round.
/// `corpus/negative/n01`.
pub const PARSE: &str = "parse";

/// A fixed-point literal that is not one: an exponent, or more than 31
/// significant digits.
///
/// The one lexical failure with an unambiguous edit — the offending literal is
/// right there and CORBA 3.4 §7.2.6.5 says exactly what is wrong with it.
/// Classified from [`crate::lex::FIXED_LITERAL_SUBJECT`] rather than from a
/// retyped prefix. `corpus/negative/n22`, `n23`.
pub const FIXED_LITERAL: &str = "fixed-literal";

/// An IDL keyword used as an identifier without the `_` escape.
///
/// Case-insensitive, so `Context` collides with `context`.
/// `corpus/negative/n08`, `n11`.
pub const RESERVED_WORD: &str = "reserved-word";

/// A `#pragma` naming something the file does not declare.
pub const PRAGMA_UNKNOWN_NAME: &str = "pragma-unknown-name";

/// A constant's type that `const_type` does not admit.
///
/// **Two diagnoses share this id and the second is the one to move.** The
/// parser's two sites refuse `fixed<d,s>` in a constant's type and span the
/// type (`corpus/negative/n18`); the analyser's site refuses `long double`,
/// which `const_type`'s grammar *does* admit and which has no literal to write,
/// and spans the constant's name (`corpus/negative/n21`). The hint keyed here
/// is written for the first and is false for the second — see the module
/// documentation.
pub const NOT_A_CONST_TYPE: &str = "not-a-const-type";

/// A `sequence` or a `fixed` written directly in a signature.
///
/// `param_type_spec` is narrower than `type_spec`: a template type reaches an
/// attribute, a parameter or a return only through a `typedef`.
/// `corpus/negative/n13`–`n16`.
pub const ANONYMOUS_TYPE_IN_SIGNATURE: &str = "anonymous-type-in-signature";

/// `void` in attribute or parameter position.
///
/// `op_type_spec` names it and `param_type_spec` does not, which is what makes
/// it a return type and nothing else. `corpus/negative/n17`.
pub const VOID_IN_SIGNATURE: &str = "void-in-signature";

/// Two names in one scope differing only in case.
///
/// The dominant failure of this project. `corpus/negative/n02`, `n10`, `n12`.
pub const IDENTIFIER_CASE_CLASH: &str = "identifier-case-clash";

/// A declaration reusing the name of a scope it sits inside, ignoring case.
///
/// `corpus/negative/n03`, `n09`.
pub const ENCLOSING_SCOPE_CLASH: &str = "enclosing-scope-clash";

/// A derived interface redeclaring a name it already inherits.
///
/// `corpus/negative/n24`.
pub const INHERITED_CLASH: &str = "inherited-clash";

/// The same name declared twice in one scope.
///
/// `corpus/negative/n06`.
pub const DUPLICATE_DECLARATION: &str = "duplicate-declaration";

/// A constant whose value its declared type cannot hold.
///
/// **Three diagnoses share this id.** The integer out of range
/// (`corpus/negative/n20`) is the one the hint was written for; a divisor of
/// zero (`n27`) has no value at all rather than one out of range, and a bounded
/// string constant longer than its bound (`n28`) is answered by widening the
/// *bound*, not the type. See the module documentation.
pub const CONST_VALUE_RANGE: &str = "const-value-range";

/// A constant's value written in a class its type does not take.
///
/// **Two diagnoses share this id.** The literal of the wrong class
/// (`corpus/negative/n19`) is the one the hint was written for; an enumerator
/// belonging to another enum (`n29`) is not a literal at all. See the module
/// documentation.
pub const CONST_VALUE_TYPE: &str = "const-value-type";

/// A union with more than one `default:` branch.
///
/// `corpus/negative/n26`.
pub const DUPLICATE_UNION_DEFAULT: &str = "duplicate-union-default";

/// A union case label used twice.
///
/// `corpus/negative/n07`.
pub const DUPLICATE_UNION_LABEL: &str = "duplicate-union-label";

/// A name that resolves, to something that is not a type.
///
/// A module, an operation or an exception in type position.
/// `corpus/negative/n25`, `n30`.
pub const NOT_A_TYPE: &str = "not-a-type";

/// An unqualified name that resolves nowhere.
///
/// **Two diagnoses share this id.** A name nothing declares
/// (`corpus/negative/n04`, `inherited-scope-leak`) is the one the hint was
/// written for — *declare it, or qualify it with its module*. `TypeCode`,
/// `Object` and `ValueBase` (`n05`) are the other: they are predefined, they
/// are in no module of the author's, and the edit is `::CORBA::TypeCode`, which
/// is what the message says and the opposite of what the hint says. See the
/// module documentation.
pub const UNKNOWN_NAME: &str = "unknown-name";

/// A qualified name whose path breaks at a named component.
///
/// Split off from [`UNKNOWN_NAME`] because the generic advice — *qualify it
/// with its module* — is meaningless for a name that is already qualified; it
/// printed `Module::::` about ninety times over the estate. The advice for this
/// case is in the message, where the analyser knows which component failed.
/// **No fix hint is keyed to it**, which is why no `corpus/negative/` file
/// produces one: `orbweaver-forge`'s corpus test requires a hint for every
/// negative file's first finding.
pub const UNKNOWN_SCOPED_NAME: &str = "unknown-scoped-name";

/// A declaration that is, or carries, something the v1 wire cannot marshal.
///
/// Reported as a separate list rather than as an error by default, so oracle
/// agreement over `corpus/golden/` is untouched; `--wire v1` promotes it.
/// See `docs/PLAN.md` §4.4.
pub const WIRE_DEFERRED_TYPE: &str = "wire/deferred-type";

/// A preprocessor directive this front end refuses rather than skips.
///
/// Conditional compilation is the shape that matters: skipping `#if` compiles
/// every arm at once.
pub const UNSUPPORTED_DIRECTIVE: &str = "unsupported-directive";

/// An `#include` whose argument is not a file name.
pub const INCLUDE_MALFORMED: &str = "include-malformed";

/// An `#include` that resolves to no file, listing every path searched.
pub const INCLUDE_NOT_FOUND: &str = "include-not-found";

/// An `#include` closing a cycle.
pub const INCLUDE_CYCLE: &str = "include-cycle";

/// An unguarded file included twice in one unit. Advice, not an error.
pub const INCLUDE_UNGUARDED_REPEAT: &str = "include-unguarded-repeat";

/// An `#include` that resolved to a file that cannot be read.
pub const INCLUDE_UNREADABLE: &str = "include-unreadable";

/// Every rule id above, so a consumer can check its own table against the set
/// this front end actually produces.
///
/// This is what makes "a hint keyed to a rule nothing produces" and "a rule
/// nothing writes a hint for" both answerable questions. Neither was, before
/// the roster existed: `orbweaver-forge` keys fifteen retyped ids and the two
/// halves could only be compared by reading.
pub const ALL: &[&str] = &[
    ANONYMOUS_TYPE_IN_SIGNATURE,
    CONST_VALUE_RANGE,
    CONST_VALUE_TYPE,
    DUPLICATE_DECLARATION,
    DUPLICATE_UNION_DEFAULT,
    DUPLICATE_UNION_LABEL,
    ENCLOSING_SCOPE_CLASH,
    FIXED_LITERAL,
    IDENTIFIER_CASE_CLASH,
    INCLUDE_CYCLE,
    INCLUDE_MALFORMED,
    INCLUDE_NOT_FOUND,
    INCLUDE_UNGUARDED_REPEAT,
    INCLUDE_UNREADABLE,
    INHERITED_CLASH,
    NOT_A_CONST_TYPE,
    NOT_A_TYPE,
    PARSE,
    PRAGMA_UNKNOWN_NAME,
    RESERVED_WORD,
    UNKNOWN_NAME,
    UNKNOWN_SCOPED_NAME,
    UNSUPPORTED_DIRECTIVE,
    VOID_IN_SIGNATURE,
    WIRE_DEFERRED_TYPE,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The roster is a set, and a duplicate would make one of two rules
    /// invisible to a consumer checking coverage against it.
    #[test]
    fn the_roster_has_no_repeats() {
        let mut seen = ALL.to_vec();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "a rule id appears twice in ALL");
    }

    /// A rule id reaches a consumer as a key and a user as text in brackets;
    /// both want the same shape, and a stray space or capital would make the
    /// two halves of a comparison differ for a reason nobody would look for.
    #[test]
    fn every_rule_id_is_lowercase_and_punctuated_the_same_way() {
        for id in ALL {
            assert!(!id.is_empty(), "empty rule id");
            assert!(
                id.bytes().all(|b| b.is_ascii_lowercase() || b == b'-' || b == b'/'),
                "{id}: a rule id is lowercase words joined by '-', with '/' for a namespace"
            );
        }
    }

    /// **The roster is the home, so nothing in this crate may write a rule id
    /// as a literal at a construction site.**
    ///
    /// Without this the roster is one more restatement: a site could keep its
    /// own spelling, drift from the constant, and nothing would be red — which
    /// is exactly the shape of the defect the roster exists to close. The scan
    /// is over this crate's own source because that is where the sites are;
    /// the corresponding check for a *consumer's* table is
    /// `ALL` against its keys, and lives with the consumer.
    ///
    /// *고정의 범위가 사실의 범위보다 좁으면 그 고정은 어긋남 위에서 초록으로
    /// 남는다 — 그래서 생성 지점은 리터럴을 쓰지 못한다.*
    #[test]
    fn no_construction_site_spells_a_rule_id_itself() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        for entry in std::fs::read_dir(&src).expect("src is readable") {
            let path = entry.expect("entry").path();
            if path.extension().is_none_or(|e| e != "rs")
                || path.file_name().is_some_and(|f| f == "rules.rs")
            {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("source is readable");
            for (n, line) in text.lines().enumerate() {
                let code = line.trim_start();
                // Prose may quote a rule id — most of these files explain one —
                // and a test may compare against a literal on purpose: an id
                // that changes *should* break the test that pinned the old one.
                // What is left after those two is a site that raises one.
                if code.starts_with("//") || code.contains("assert") || code.contains("rule ==") {
                    continue;
                }
                if ALL.iter().any(|id| line.contains(&format!("\"{id}\""))) {
                    offenders.push(format!(
                        "{}:{}: {}",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        n + 1,
                        line.trim()
                    ));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "a rule id belongs to `rules`, not to the site that raises it — use the constant:\n  \
             {}",
            offenders.join("\n  ")
        );
    }
}
