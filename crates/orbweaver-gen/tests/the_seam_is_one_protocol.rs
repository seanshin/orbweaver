//! The seam's protocol is one value, and every binding is asserted against it.
//!
//! `orbweaver_giop::server::serve_one_ordering()` established the discipline
//! this file applies to the servant seam: **an agreement between two
//! implementations is a value, not a comment.** Before it, the order the serve
//! loop asks its three questions in was a comment beside each of two
//! implementations, and the two agreed because somebody had checked.
//!
//! The seam was in exactly that state on 2026-08-26. Its document shape lived
//! in three comments — `pyservant.rs`'s module header, `py_bridge.rs`'s two
//! banner blocks, `python_rt.py`'s prose — and in **hand-typed string literals
//! at every site that read a key**, in two languages. `CLAUDE.md` records what
//! that costs at three layers (*"a sentence many layers say is a fact"*, twelve
//! literals in two crates, one of them already false). A third and fourth
//! language does not add one copy, it adds one per direction per language.
//!
//! So: [`orbweaver_gen::seam::protocol()`] is assembled from the constants the
//! Rust dispatcher reads with, `_rt.seam_protocol()` from the constants the
//! Python runtime reads with, and this file asserts they are equal. **A binding
//! in a third language adds a function and a row here, and nothing else.** That
//! is what "adding C or Java costs an emitter and a small runtime, and costs
//! nothing in the seam's definition" has to mean to be measurable.
//!
//! # Why equality and not a shared symbol
//!
//! Because Python cannot import a Rust constant. Where a constant *can* be
//! shared this project shares it and records that there is then nothing left to
//! test — the drift becomes impossible rather than detectable. Across a
//! language boundary it cannot be, which is the same position the five
//! wire-refusal families are in, held the same way.

use std::collections::BTreeMap;
use std::process::Command;

use orbweaver_dynamic::json::Json;
use orbweaver_gen::seam;

/// Every binding's published protocol, by the name the suite knows it as.
///
/// A row per language. `spikes/bindings/*.manifest` is where a binding's
/// *cells* are enrolled; this is where its **protocol** is, and the two are
/// different questions: a binding can have no peer to be driven against and
/// still owe the same document.
///
/// # The extension rule, which was aspirational until 2026-09-02
///
/// The header above has said since this file was written that *a binding in a
/// third language adds a function and a row here, and nothing else*. **It was
/// not true.** `theirs()` hardcoded `python3` and wrote `python::RUNTIME` to
/// `_rt.py`, and a row's second field was a Python `-c` snippet — so Java, whose
/// servant half landed 2026-09-01, could not be added and was not. It published
/// no document at all, read its envelope key as `get("call")`, and nothing here
/// could go red about any of it. The rule is true now; that is what `Runner` is.
struct Binding {
    language: &'static str,
    runner: Runner,
    /// Differences from [`seam::protocol()`] this binding is **known** to have,
    /// each named exactly as `differences` renders it.
    ///
    /// **Exact, not a floor.** A floor would prove nothing about the count and
    /// would let a difference rot in place; this list must match what the
    /// comparison finds, so a difference that *disappears* fails here too and
    /// has to be struck. Every entry carries its reason in the comment beside
    /// it, and an entry still present when the work it names is done is a
    /// failure rather than a smaller number.
    pinned: &'static [&'static str],
}

/// How one binding's runtime is made to print its document.
enum Runner {
    /// A Python `-c` snippet, run with the runtime written beside it.
    Python(&'static str),
    /// `javac` the runtime, then run its `main`. Needs a JDK; see the test.
    Java,
}

const BINDINGS: [Binding; 2] = [
    Binding {
        language: "python",
        // Printed by the runtime rather than parsed out of it: what is being
        // compared is what the module *computes*, which is the only thing that
        // tracks what it reads with.
        runner: Runner::Python(
            "import json, sys; sys.path.insert(0, sys.argv[1]); import _rt; \
             print(json.dumps(_rt.seam_protocol(), sort_keys=True))",
        ),
        pinned: &[],
    },
    Binding {
        language: "java",
        runner: Runner::Java,
        // **Empty, and it emptied in one day.** Three lines stood here on
        // 2026-09-02 and all three are struck, each by the work that made it
        // false rather than by anyone editing this list first: the absent
        // `invoke` section and `version: "2" vs "1"` went with
        // PLAN-SEAM-JAVA.md S3-S4, and `call.object` went with the leak it
        // turned out to name.
        //
        // Each time, **the test refused to let a struck line linger**: with the
        // work done and this list unedited it failed, naming the difference as
        // pinned-but-no-longer-found. That is what an exact pin buys over a
        // floor, and it was not hypothetical — it happened three times.
        pinned: &[],
    },
];

/// The Rust side, canonicalised so two documents can be compared as text.
fn ours() -> Json {
    seam::protocol()
}

/// One binding's document, or the reason it could not be obtained.
///
/// An interpreter that will not run is a **failure**, never a skip disguised as
/// a pass: this file's whole claim is that the far side agrees, and it cannot
/// report anything without asking the far side.
fn theirs(binding: &Binding) -> Option<Json> {
    let dir = std::env::temp_dir().join(format!(
        "orbweaver-seam-{}-{}",
        binding.language,
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("a directory to write the runtime into");

    let out = match binding.runner {
        Runner::Python(runner) => {
            std::fs::write(dir.join("_rt.py"), orbweaver_gen::python::RUNTIME)
                .expect("the runtime");
            Command::new("python3").arg("-c").arg(runner).arg(&dir).output().unwrap_or_else(|e| {
                panic!(
                    "python3 could not be run ({e}). An unmeasured check is a failure, never \
                     a pass: this test exists to ask the other implementation what protocol \
                     it speaks, and cannot answer without it."
                )
            })
        }
        Runner::Java => {
            // A JDK may genuinely be absent, where python3 may not, so this is
            // the one runner that can decline. It declines the way the Java
            // tests in this crate already do — and `PLAN-SEAM-JAVA.md` §5.3
            // records that a skip here is only honest beside a counted
            // `SKIPPED` in the harness, because on its own it is the
            // skip-disguised-as-a-pass this file's header refuses.
            let (javac, java) = jdk()?;
            std::fs::write(dir.join("_Rt.java"), orbweaver_gen::java::RUNTIME)
                .expect("the runtime");
            let built = Command::new(&javac)
                .arg("-d")
                .arg(&dir)
                .arg(dir.join("_Rt.java"))
                .output()
                .expect("javac runs once it has been found");
            assert!(
                built.status.success(),
                "the Java runtime did not compile:\n{}",
                String::from_utf8_lossy(&built.stderr)
            );
            Command::new(&java).arg("-cp").arg(&dir).arg("_Rt").output().expect("java runs")
        }
    };
    assert!(
        out.status.success(),
        "the {} runtime could not publish its protocol:\n{}",
        binding.language,
        String::from_utf8_lossy(&out.stderr)
    );
    Some(Json::parse(String::from_utf8_lossy(&out.stdout).trim()).expect("the runtime prints JSON"))
}

/// `javac` and `java`, or `None` — the same discipline the Java tests in this
/// crate use, and deliberately the same environment variable.
fn jdk() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let home = std::env::var("ORBWEAVER_JAVA_HOME").unwrap_or_else(|_| {
        "/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home".to_owned()
    });
    let javac = std::path::Path::new(&home).join("bin/javac");
    let java = std::path::Path::new(&home).join("bin/java");
    (javac.is_file() && java.is_file()).then_some((javac, java))
}

/// **The gate.** Every binding speaks the protocol the ORB dispatches with.
#[test]
fn every_binding_publishes_the_protocol_the_orb_dispatches_with() {
    let ours = ours();
    let mut asked = 0;
    for binding in BINDINGS {
        let Some(theirs) = theirs(&binding) else {
            println!(
                "SKIPPED  {} — no JDK, set ORBWEAVER_JAVA_HOME. Its agreement with the \
                 protocol the ORB dispatches with is UNMEASURED, not passing.",
                binding.language
            );
            continue;
        };
        asked += 1;
        let found = differences(&ours, &theirs, "");
        let expected: Vec<String> = binding.pinned.iter().map(|d| (*d).to_owned()).collect();
        // Equality against the pin, in both directions on purpose: a NEW
        // divergence fails, and so does a pinned one that has gone away without
        // being struck from the list. A pin nobody has to maintain is a floor,
        // and a floor here would let a difference rot while reading green.
        assert_eq!(
            found, expected,
            "the {} runtime and orbweaver_gen::seam do not differ in the way this file \
             says they do.\n  found:    {found:#?}\n  pinned:   {expected:#?}\n\
             ours:   {ours}\ntheirs: {theirs}",
            binding.language
        );
    }
    assert!(asked > 0, "no binding could be asked; this gate measured nothing");
}

/// The control: a language-shaped assumption reintroduced, and the count moving.
///
/// `CLAUDE.md`'s rule is that a gate tested only against a tree with no defect
/// in it is *the green-while-measuring-nothing class with better manners*. So
/// the comparison is run against a document with one key renamed, one ordinal
/// transposed, and one section removed — the three shapes a second binding
/// would actually get wrong — and each must be **named** rather than merely
/// counted, because a diff that says "not equal" is not a diff a binding author
/// can act on.
#[test]
fn the_comparison_names_what_diverged() {
    let ours = ours();

    // 1. A key renamed: the shape a binding gets wrong by reading its own
    //    language's idiom instead of the protocol's word.
    let renamed = edit(&ours, "call", |call| {
        let mut c = call.clone();
        c.insert("operation".to_owned(), Json::String("operation".to_owned()));
        c
    });
    assert_eq!(differences(&ours, &renamed, ""), vec!["call.operation: \"op\" vs \"operation\""]);

    // 2. §4.11.4's first two ordinals transposed — the exact mistake this
    //    project has already made once, in a language that had them named.
    let transposed = edit(&ours, "completed", |c| {
        let mut c = c.clone();
        c.insert("yes".to_owned(), Json::Number("1".to_owned()));
        c.insert("no".to_owned(), Json::Number("0".to_owned()));
        c
    });
    assert_eq!(
        differences(&ours, &transposed, ""),
        vec!["completed.no: 1 vs 0", "completed.yes: 0 vs 1",]
    );

    // 3. A section absent: a binding that simply has not implemented one —
    //    which must read as a divergence and not as agreement by omission.
    let Json::Object(mut without) = ours.clone() else { panic!("an object") };
    without.remove("reference");
    assert_eq!(
        differences(&ours, &Json::Object(without), ""),
        vec!["reference: {\"own_object_prefix\":\"oid:\"} vs absent",]
    );
}

/// A binding's document is not allowed to be empty, and neither is the roster.
///
/// The other half of the control above: a comparison over nothing compares
/// nothing. If `BINDINGS` were ever emptied — or `protocol()` reduced to `{}` —
/// the gate would pass while measuring exactly no bindings.
#[test]
fn the_roster_and_the_document_are_not_empty() {
    assert!(!BINDINGS.is_empty(), "a gate over no bindings measures nothing");
    let Json::Object(top) = ours() else { panic!("the protocol is an object") };
    assert!(top.len() >= 6, "sections: {:?}", top.keys().collect::<Vec<_>>());
    for (section, body) in &top {
        if let Json::Object(keys) = body {
            assert!(!keys.is_empty(), "section {section:?} names nothing");
        }
    }
}

/// The seam's definition names no language.
///
/// The mechanical half of neutrality, and the one a second binding's author can
/// check in a second: `seam.rs` and `surface.rs` are the seam's definition, and
/// a reference from either into a language's emitter is D032 §3's third row
/// reaching into the first two. This is exactly the rule
/// `spikes/bindings/AXES` states for `binding_suite.sh` — *"a needed special
/// case would be a finding about the seam, never a `case` arm"* — applied to
/// the seam itself.
#[test]
fn the_seams_definition_does_not_reach_into_a_language() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    for file in ["src/seam.rs", "src/surface.rs"] {
        let src = std::fs::read_to_string(root.join(file)).expect(file);
        for (n, line) in src.lines().enumerate() {
            // Documentation may name a language — the history is *why* this
            // rule exists and deleting it would be the worse trade. Code may
            // not.
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            for reach in ["crate::python", "crate::java", "python::", "java::", "pyservant"] {
                if code.contains(reach) {
                    found.push(format!("{file}:{}: {reach} in {}", n + 1, code.trim()));
                }
            }
        }
    }
    assert_eq!(found, Vec::<String>::new(), "the seam's definition reaches into a language");
}

// ── Comparing two documents so the answer is actionable ──────────────────────

/// Every place two protocol documents differ, by path, deepest first.
///
/// A path and both values rather than a boolean, because the consumer of this
/// answer is somebody writing the third binding, and "not equal" gives them
/// nothing to change.
fn differences(ours: &Json, theirs: &Json, at: &str) -> Vec<String> {
    let mut out = Vec::new();
    match (ours, theirs) {
        (Json::Object(a), Json::Object(b)) => {
            let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            for k in keys {
                let path = if at.is_empty() { k.clone() } else { format!("{at}.{k}") };
                match (a.get(k), b.get(k)) {
                    (Some(x), Some(y)) => out.extend(differences(x, y, &path)),
                    (Some(x), None) => out.push(format!("{path}: {x} vs absent")),
                    (None, Some(y)) => out.push(format!("{path}: absent vs {y}")),
                    (None, None) => unreachable!("the key came from one of the two"),
                }
            }
        }
        // A number the far side wrote as JSON and one we wrote as a decimal
        // string are the same ordinal; comparing them as text would fail on the
        // spelling rather than on the fact.
        (Json::Number(a), Json::Number(b)) if a == b => {}
        (a, b) if a == b => {}
        (a, b) => out.push(format!("{at}: {a} vs {b}")),
    }
    out
}

/// One section of a protocol document, replaced.
fn edit(
    doc: &Json,
    section: &str,
    f: impl FnOnce(&BTreeMap<String, Json>) -> BTreeMap<String, Json>,
) -> Json {
    let Json::Object(top) = doc else { panic!("an object") };
    let Some(Json::Object(body)) = top.get(section) else { panic!("no section {section}") };
    let mut top = top.clone();
    top.insert(section.to_owned(), Json::Object(f(body)));
    Json::Object(top)
}
