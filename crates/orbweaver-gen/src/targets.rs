//! The targets this crate emits for, as **data** — so an instrument outside the
//! crate can range over them instead of naming them.
//!
//! # Why this module exists
//!
//! `D032` §4 clause 5 says a binding is a target when *"its keyword escaping is
//! exercised by `corpus/golden/28-target-keywords.idl`"*. Until this module
//! existed that clause **had no instrument**. Both keyword lists were private
//! consts — `python::PY_KEYWORDS` and the four lists in `lib.rs` — so the only
//! thing that exercised them was the fact that the emitters happen to run over
//! the whole golden corpus, which covers a word by accident and would cover a
//! *new* word by accident too, or not at all, with nothing red either way.
//!
//! That is `a pin whose scope is narrower than its fact's` in its keyword form:
//! the fact is *"this word must be escaped, and something must have escaped
//! it"*, and its scope is the workspace, because the corpus file that has to
//! carry the word is not in this crate.
//!
//! # What is published, and what is deliberately not
//!
//! Two things per target, and **both of them are the emitter's own**:
//!
//! * `words()` — every word the emitter will escape, from every list it keeps.
//!   For Rust that is four lists, not one: a raw identifier is the escape for
//!   most keywords, `self`/`Self`/`super`/`crate` cannot be raw at all, `Ok` and
//!   `None` cannot be *bound* under any escape, and a primitive name shadows the
//!   primitive inside the generated module. A check that only knew about
//!   `KEYWORDS` would have said Rust was covered while fifteen of nineteen
//!   positions emitted `pub struct Self {`.
//! * `escape` — the function the emitter itself calls. Not a retyped rule.
//!   `a classifier is a sentence too`: a checker that reimplemented "prefix with
//!   `r#`" would agree with the emitter until the day one of them changed.
//!
//! What is **not** published is any notion of which target is "done". This
//! module answers *which targets have an emitter and what each one escapes*, and
//! nothing else. The acceptance verdict is `spikes/binding_suite.sh`'s, and it
//! is a verdict about what is unmeasured rather than a count of what is.
//!
//! # Adding a target
//!
//! Add a row to [`TARGETS`]. The keyword-coverage check picks it up on the next
//! `cargo test` with no edit of its own — which is the point, because the
//! alternative is a second list of targets that goes stale silently. A word the
//! corpus file does not exercise must then be named in
//! `spikes/bindings/keywords-not-executed.tsv` with its reason; there is no
//! third option, and that is what makes the clause a gate rather than a report.
//!
//! *이 크레이트가 방출하는 대상들을 **데이터**로 공개한다. 각 대상은 자기 예약어
//! 전체와 **이미터 자신이 호출하는** 이스케이프 함수를 내놓는다. 규칙을 다시 적은
//! 검사기는 어긋나는 날까지만 일치한다.*

use orbweaver_registry::Registry;

/// One target's reserved words and the escaping the emitter actually applies.
#[derive(Clone, Copy)]
pub struct Target {
    /// The name the acceptance suite is parameterised by (`--language`).
    ///
    /// This is the one place the spelling lives. `spikes/bindings/<name>.manifest`
    /// is found by it, and a manifest naming something absent here is a failure
    /// rather than an empty run — the `bears_on` lesson, one axis over.
    pub language: &'static str,
    /// Every word this emitter will escape, from every list it keeps.
    pub words: fn() -> Vec<&'static str>,
    /// The emitter's own spelling function, called rather than reproduced.
    pub escape: fn(&str) -> String,
    /// Everything this target emits for a contract, as one string.
    ///
    /// Uniform across targets on purpose, and the *reason* is what makes clause
    /// 5 a measurement rather than a reading. "Exercised" means the escaping
    /// **ran**: if the emitter escaped a word, the escaped spelling is in this
    /// text. Asking the IDL instead — *does the contract declare an identifier
    /// spelled `yield`* — would answer a different and weaker question, because
    /// a contract can declare a name in a position an emitter never escapes,
    /// which is exactly the trap `28-target-keywords.idl`'s own header records:
    /// 1989 probes were needed to find that *position* was the whole question.
    pub emit_text: fn(&Registry) -> String,
}

/// Every target with an emitter in this crate. **The one home for that list.**
pub const TARGETS: &[Target] = &[
    Target {
        language: "rust",
        words: crate::reserved_words,
        escape: crate::rust_name,
        emit_text: emit_rust,
    },
    Target {
        language: "python",
        words: crate::python::reserved_words,
        escape: crate::python::python_name,
        emit_text: emit_python,
    },
    Target {
        language: "java",
        words: crate::java::reserved_words,
        escape: crate::java::java_name,
        emit_text: emit_java,
    },
];

fn emit_rust(r: &Registry) -> String {
    crate::emit(r, "contract").source
}

fn emit_java(r: &Registry) -> String {
    // Every file, for the reason the Python arm gives below, plus one Java has
    // of its own: a Java package **is** a directory, so an IDL module named for
    // a Java keyword is escaped in the directory name, in the `package` line
    // and in every fully qualified reference to something inside it. A check
    // that read one file would report the module position covered off whichever
    // of the three it happened to read.
    //
    // Every file **except the hand-written runtime** — see [`without_runtime`].
    let generated = crate::java::emit_java(r, "contract");
    without_runtime(&generated.files, "contract/_Rt.java")
}

fn emit_python(r: &Registry) -> String {
    // Every file, because a keyword can be escaped in the package `__init__`
    // and nowhere else — an IDL module whose name is a Python keyword is
    // precisely that case, and it is one of the five the corpus file records as
    // measured NOT to survive.
    let generated = crate::python::emit_python(r, "contract");
    without_runtime(&generated.files, "_rt.py")
}

/// Everything the emitter wrote **for this contract**, with the verbatim
/// runtime left out.
///
/// # Why the runtime is excluded, measured 2026-08-26
///
/// `keyword_coverage` asks whether a word's *escaped spelling* appears in what
/// the emitter wrote, and the escape in both of these mappings is a leading
/// underscore — which is also how a hand-written runtime spells its own private
/// names. So a runtime local called `_default` makes `default` read as covered
/// in a contract that never names it, and a runtime docstring mentioning
/// `_lambda` does the same for `lambda`. Both exist: one in `java_rt.java` and
/// one in `python_rt.py`, found by adding a second target and reading why
/// `default` was the one IDL keyword the Java run did not complain about.
///
/// That is the clause-5 defect wearing the instrument's own coat — *"a word
/// covered by accident is what this instrument exists to end"* — and the fix is
/// not to rename the runtime's locals, which would come back the next time
/// somebody wrote one. The runtime is shipped verbatim and is the same bytes
/// for every contract, so it cannot be evidence about any contract.
///
/// *런타임은 계약과 무관하게 같은 바이트로 실려 나가므로 어떤 계약에 대한 증거도 될 수
/// 없다. 이스케이프가 선행 밑줄이라 런타임의 지역 이름이 커버리지로 오독된다.*
fn without_runtime(files: &std::collections::BTreeMap<String, String>, runtime: &str) -> String {
    files
        .iter()
        .filter(|(name, _)| name.as_str() != runtime)
        .map(|(_, text)| text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The target with this name, if this crate emits for it.
pub fn target(language: &str) -> Option<&'static Target> {
    TARGETS.iter().find(|t| t.language == language)
}

/// Whether `needle` occurs in `text` as a whole identifier.
///
/// Not a substring test: `_as` is a substring of `_assert` and both are escaped
/// Python names, so a substring test would report `as` covered by a contract
/// that only ever names `assert`. The boundary is the identifier character set
/// of the *emitted* language, and `#` is deliberately not in it — `r#as` ends at
/// `s`, and the `r#` prefix is what makes it findable at all.
fn whole_word(text: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let b = text.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = text[from..].find(needle) {
        let start = from + rel;
        let end = start + needle.len();
        let before_ok = start == 0 || !ident(b[start - 1] as char);
        let after_ok = end == b.len() || !ident(b[end] as char);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// Which of a target's reserved words this contract put through its escaping,
/// and which it did not.
///
/// Returns `(executed, not_executed)`, both sorted. `executed` means the escaped
/// spelling is present in what the emitter wrote — the escaping *ran on that
/// word*, which is what D033 §3.2 asks for and what finding `yield` missing from
/// the Rust list actually required.
pub fn keyword_coverage(t: &Target, registry: &Registry) -> (Vec<&'static str>, Vec<&'static str>) {
    let text = (t.emit_text)(registry);
    let (mut hit, mut miss) = (Vec::new(), Vec::new());
    for w in (t.words)() {
        if whole_word(&text, &(t.escape)(w)) { hit.push(w) } else { miss.push(w) }
    }
    hit.sort_unstable();
    miss.sort_unstable();
    (hit, miss)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every published word is one the emitter actually changes.
    ///
    /// A word in a list that the escaping does not touch is a list that has
    /// drifted from the function beside it, and it would make the coverage check
    /// demand corpus coverage for a word that needs none. Rust's four lists are
    /// the case that matters: they are consulted in one `match` with an order,
    /// and a word moved between them without the `match` changing shows up here.
    #[test]
    fn every_published_word_is_one_its_own_emitter_escapes() {
        for t in TARGETS {
            for w in (t.words)() {
                let e = (t.escape)(w);
                assert_ne!(
                    e, w,
                    "{}: \"{w}\" is published as a reserved word but the emitter's own \
                     escaping returns it unchanged — the list and the function have drifted",
                    t.language
                );
            }
        }
    }

    /// The names are unique, because the suite looks a target up by one.
    #[test]
    fn no_two_targets_answer_to_the_same_name() {
        let mut seen: Vec<&str> = TARGETS.iter().map(|t| t.language).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(before, seen.len(), "two targets share a --language name: {seen:?}");
    }
}
