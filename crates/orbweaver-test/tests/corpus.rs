//! The property, measured over the whole golden corpus at once.
//!
//! §5.1: work the whole set at once and verify the whole set at once. The
//! corpus exists precisely so that "does our marshalling hold" is one question
//! with one answer rather than twenty-three. This test is the codified form of
//! the batch run that landed this crate — the run is repeatable in CI instead
//! of being a number in a commit message.
//!
//! The contract half is *counted* rather than asserted empty: it is advice, the
//! corpus is deliberately full of unannotated interfaces (most of it exists to
//! exercise the type system, not SIDL), and a test that demanded silence would
//! have to be satisfied by annotating files for the test's benefit.

use std::path::{Path, PathBuf};

use orbweaver_dynamic::Value;
use orbweaver_forge::Severity;
use orbweaver_registry::Registry;
use orbweaver_test::{contract, prop};

fn corpus(dir: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(dir);
    let mut files: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", root.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "idl"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no IDL in {}", root.display());
    files
}

fn registry_of(path: &Path) -> Registry {
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let spec = orbweaver_idl::parse(&src)
        .unwrap_or_else(|d| panic!("{} does not parse: {d:?}", path.display()));
    let mut r = Registry::new();
    r.load(&spec).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    r
}

/// Every type in the golden corpus, both byte orders, every alignment phase.
#[test]
fn every_golden_type_is_byte_stable() {
    let mut defects = Vec::new();
    let mut types = 0usize;
    for path in corpus("corpus/golden") {
        let reg = registry_of(&path);
        for id in reg.ids().cloned().collect::<Vec<_>>() {
            let Some(tc) = reg.typecode(&id) else { continue };
            types += 1;
            for f in prop::roundtrip_property(tc, 16) {
                if f.severity == Severity::Error {
                    defects.push(format!("{}: {f}", path.display()));
                }
            }
        }
    }
    // 66 named types across 23 files when this landed. A floor rather than an
    // equality: the corpus grows with the change that motivated it, and a test
    // that has to be edited for every addition gets edited without being read.
    assert!(types >= 60, "the corpus should cover far more than {types} types");
    assert!(defects.is_empty(), "byte instability:\n  {}", defects.join("\n  "));
}

/// The gaps are part of the measurement. An unmeasured arm reported as covered
/// is the harness failure `CLAUDE.md` names; this keeps the list visible and
/// pins its size so a new gap has to be acknowledged.
#[test]
fn the_coverage_gaps_over_the_corpus_are_the_known_ones() {
    let mut gaps = Vec::new();
    // The `prop/unmeasured` reasons, kept beside the one-line form: two very
    // different gaps file under that rule — a recursive arm that could not be
    // resolved, and a sequence whose element the wire cannot carry at all —
    // and only the message tells them apart. Counting the rule alone made the
    // first indistinguishable from the second, which is how a new gap class
    // would have arrived reading as a regression in the old one.
    let mut unmeasured: Vec<(String, String)> = Vec::new();
    for path in corpus("corpus/golden") {
        let reg = registry_of(&path);
        for id in reg.ids().cloned().collect::<Vec<_>>() {
            let Some(tc) = reg.typecode(&id) else { continue };
            for f in prop::roundtrip_property(tc, 8) {
                if f.severity != Severity::Error {
                    if f.rule == "prop/unmeasured" {
                        unmeasured.push((f.source.clone(), f.message.clone()));
                    }
                    gaps.push(format!("{}: {} {}", path.display(), f.rule, f.source));
                }
            }
        }
    }
    let recursive = unmeasured.iter().filter(|(_, m)| m.contains("recursive")).count();
    let unsupported = gaps.iter().filter(|g| g.contains("prop/unsupported-type")).count();
    let unmapped: Vec<&str> = gaps
        .iter()
        .filter(|g| g.contains("json/unmapped"))
        .map(|g| g.rsplit(' ').next().unwrap_or(g))
        .collect();
    // The recursive gap closed: markers now resolve against the enclosing type
    // the path is standing on, so `corpus/golden/15`'s trees are generated with
    // children and round-trip like anything else. It is asserted at zero rather
    // than removed, because the class can come back — a cycle whose marker
    // names a type that is not enclosing is still unresolvable, and this is
    // where that would show up.
    assert_eq!(
        recursive,
        0,
        "a recursive arm is unmeasured again:\n  {}",
        unmeasured.iter().map(|(s, m)| format!("{s}: {m}")).collect::<Vec<_>>().join("\n  ")
    );
    // The other `prop/unmeasured` class, pinned by id: a `sequence<T>` whose
    // element the wire cannot carry has exactly one value — the empty one — so
    // every case the sampler draws for it is the same case, and saying so is
    // the whole point of the rule. Both entries arrived on 2026-08-21 with the
    // corpus files for `native` and `ValueBase`; before them the corpus had no
    // sequence of an unmarshallable element, so this arm of `gap_reason` had
    // never run over the corpus at all.
    let empty_sequences: Vec<&str> = unmeasured
        .iter()
        .filter(|(_, m)| !m.contains("recursive"))
        .map(|(s, _)| s.as_str())
        .collect();
    assert_eq!(
        empty_sequences,
        ["IDL:gn31/Roster:1.0", "IDL:gvb32/Cargo:1.0"],
        "the set of sequences that can only be empty changed:\n  {}",
        unmeasured.iter().map(|(s, m)| format!("{s}: {m}")).collect::<Vec<_>>().join("\n  ")
    );
    assert!(
        unsupported > 0,
        "corpus/golden/21-deferred-fixed.idl must report `fixed` as uncovered (§4.4)"
    );
    // The types AnyJSON does not carry, pinned by id rather than counted. The
    // JSON leg is skipped for exactly these and for nothing else; a new id here
    // is a finding about the mapping (something stopped crossing) or about
    // the corpus (a new use of a type the mapping never carried), and either
    // deserves to be read rather than absorbed. `fixed` is on this list and on
    // `prop/unsupported-type` above because those are two facts about two
    // modules: the wire does not carry it (§4.4) and neither does the mapping.
    //
    // `corpus/golden/deferred-reach.idl` (2026-08-19) added five: every way a
    // declaration reaches `fixed` without naming it — an exception, a union
    // whose branches are all `fixed`, an array typedef and the struct holding
    // it, an attribute's typedef. Acknowledged here, as this list asks.
    //
    // 2026-08-20 added five more, and they are the interesting half. The
    // valuetype side used to be *absent* from this list, and not because it
    // crossed: the registry recorded a `valuetype` and an abstract interface
    // as `TypeCode::ObjRef`, so this leg ran for them — as a reference. A
    // measurement of the wrong wire form counted as coverage. The registry now
    // records `TypeCode::Value` and `TypeCode::AbstractInterface`, the mapping
    // has no form for either, and the four valuetypes plus the struct holding
    // an abstract interface say so here instead of passing quietly.
    //
    // 2026-08-21 added eight, and they are the same story twice more. Five
    // from `31-native-type`: a `native` had been recorded as
    // `TypeCode::ObjRef`, so this leg ran for it — as a reference — and a
    // measurement of a wire form the type does not have counted as coverage.
    // Three from `32-valuebase`: `ValueBase` is a valuetype and was recorded
    // as a reference for exactly as long as the keyword had been parsed.
    // Note which ids are *absent*: `gn31::Desk` and `gvb32::Depot` hold
    // references to unservable interfaces, and a reference crosses. Two more
    // are absent for a different reason and were on this list for half a day:
    // `gn31::Roster` is a `sequence<Handle>` and `gvb32::Cargo` a
    // `sequence<ValueBase>`, and a sequence whose element cannot be sampled
    // has exactly one value — the empty one — which AnyJSON carries in both
    // directions. Propagating the element's limit through the sequence made
    // the leg report itself unmeasured for a type whose every existing value
    // does cross, and cost 128 round trips the CDR leg had taken.
    assert_eq!(
        unmapped,
        [
            "IDL:gc20/Money:1.0",
            "IDL:gc20/Named:1.0",
            "IDL:gc21/Amount:1.0",
            "IDL:gc21/Invoice:1.0",
            "IDL:gn31/Booking:1.0",
            "IDL:gn31/Handle:1.0",
            "IDL:gn31/Session:1.0",
            "IDL:gn31/Slot:1.0",
            "IDL:gvb32/Envelope:1.0",
            "IDL:gvb32/Manifest:1.0",
            "IDL:gcdr/Column:1.0",
            "IDL:gcdr/Ledger:1.0",
            "IDL:gcdr/Memo:1.0",
            "IDL:gcdr/Note:1.0",
            "IDL:gcdr/Overdrawn:1.0",
            "IDL:gcdr/Payment:1.0",
            "IDL:gcdr/Rate:1.0",
            "IDL:gcdr/Tagged:1.0",
        ],
        "the set of types the JSON leg does not run for changed:\n  {}",
        gaps.iter()
            .filter(|g| g.contains("json/unmapped"))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n  ")
    );
    // Everything non-Error is one of these three, or a new gap class has
    // appeared and deserves a look rather than a silent pass.
    assert_eq!(
        unmeasured.len() + unsupported + unmapped.len(),
        gaps.len(),
        "unexpected gap class:\n  {}",
        gaps.join("\n  ")
    );
}

/// Every value the CDR leg round-trips is also taken across AnyJSON and back,
/// and the count proves it ran. The property sweep round-tripped CDR only
/// until 2026-08-19, so the mapping's refusal of every non-empty value under a
/// recursion marker (fixed in commit 1b6b4c8) was a defect this test could not
/// have seen at any witness; now it would be a `json/*` error naming the type.
#[test]
fn every_golden_value_also_crosses_anyjson() {
    let mut defects = Vec::new();
    let mut total = prop::Measured::default();
    // CDR round trips taken for a type the mapping does not carry. It used to
    // be structurally impossible for this to be non-zero — a type AnyJSON
    // refused was one the sampler could not sample either, so it contributed
    // nothing to either leg. `typedef sequence<Handle>` broke the coincidence
    // on 2026-08-21: the sequence has exactly one value, the empty one, which
    // marshals to a length of zero perfectly well, so the CDR leg runs
    // sixteen times in each byte order while the JSON leg correctly does not.
    let mut unmapped_cdr = 0usize;
    for path in corpus("corpus/golden") {
        let reg = registry_of(&path);
        for id in reg.ids().cloned().collect::<Vec<_>>() {
            let Some(tc) = reg.typecode(&id) else { continue };
            let (findings, measured) =
                prop::roundtrip_property_measured(tc, 16, prop::DEFAULT_SEED);
            if findings.iter().any(|f| f.rule == "json/unmapped") {
                assert_eq!(
                    measured.json,
                    0,
                    "{}: {id} is json/unmapped and the JSON leg ran {} time(s) anyway",
                    path.display(),
                    measured.json
                );
                unmapped_cdr += measured.cdr;
            } else {
                total.add(measured);
            }
            for f in findings {
                if f.severity == Severity::Error && f.rule.starts_with("json/") {
                    defects.push(format!("{}: {f}", path.display()));
                }
            }
        }
    }
    assert!(defects.is_empty(), "AnyJSON is not a round trip:\n  {}", defects.join("\n  "));
    // The ratio, not only the absence of findings: for every type the mapping
    // *does* carry, each CDR round trip must have had a JSON leg too.
    assert!(total.cdr >= 60 * 16 * 2, "the corpus should measure far more than {}", total.cdr);
    assert_eq!(
        total.json,
        total.cdr,
        "{} CDR round trip(s) were not taken across AnyJSON and no json/unmapped finding \
         accounts for them",
        total.cdr - total.json
    );
    // And the ones a `json/unmapped` finding does account for: **none**, since
    // 2026-08-21. Every type the mapping refuses is also a type the sampler
    // cannot build a value for, so a refused type contributes no CDR round
    // trip either — the two limits coincide, and where they stopped coinciding
    // (a sequence of an unsamplable element, whose one value is empty) the
    // fix was to let the leg run rather than to widen this number. Pinned at
    // zero rather than deleted, because "some CDR round trips have no JSON
    // leg" is exactly the sentence a regression in the mapping would produce,
    // and it must be read rather than absorbed by an inequality.
    assert_eq!(unmapped_cdr, 0, "the CDR-only set changed size");
}

/// The contract half runs clean over the corpus in the sense that matters:
/// it produces findings, never a crash, and never an error-severity verdict.
#[test]
fn contract_advice_over_the_corpus_never_reaches_error() {
    let mut total = 0usize;
    for dir in ["corpus/golden", "corpus/annotations", "corpus/requirements/generated"] {
        for path in corpus(dir) {
            let src = std::fs::read_to_string(&path).expect("read");
            let Ok(spec) = orbweaver_idl::parse(&src) else { continue };
            let mut reg = Registry::new();
            if reg.load(&spec).is_err() {
                continue;
            }
            for f in contract::contract_findings(&reg) {
                assert_ne!(f.severity, Severity::Error, "{}: {f}", path.display());
                total += 1;
            }
        }
    }
    assert!(total > 0, "checks that find nothing on our own corpus are not worth having");
}

/// The recursive witnesses of `corpus/golden/15`, measured rather than
/// assumed. A green property over a recursive type proves nothing if every
/// value it generated was the empty list — and for `TreeSeq` that is what it
/// was: over the 32 default seeds, 22 cases sampled to `None` and were skipped
/// without a finding, and the 10 that ran were all empty (2026-08-19). This
/// pins the two shapes the file produces — the marker as a sequence element
/// naming the struct (`Tree`) and as a struct member naming the typedef
/// (`TreeSeq`) — to a witness that follows the marker on the batch seed.
#[test]
fn golden_15s_recursive_witnesses_are_not_the_empty_list() {
    fn depth(v: &Value) -> usize {
        match v {
            Value::Struct(ms) => 1 + ms.iter().map(|(_, v)| depth(v)).max().unwrap_or(0),
            Value::List(items) => items.iter().map(depth).max().unwrap_or(0),
            _ => 0,
        }
    }
    let path = corpus("corpus/golden")
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("15-")))
        .expect("corpus/golden/15");
    let reg = registry_of(&path);
    let cases = 32u64;
    for (id, min_depth) in [("IDL:gc15/Tree:1.0", 2), ("IDL:gc15/TreeSeq:1.0", 1)] {
        let tc = reg.typecode(id).unwrap_or_else(|| panic!("{id} is in the registry"));
        let mut produced = 0;
        let mut followed = 0;
        for i in 0..cases {
            let Some(v) = prop::sample(tc, prop::case_seed(prop::DEFAULT_SEED, i)) else {
                panic!("{id}: case {i} produced no value, so it ran nothing");
            };
            produced += 1;
            if depth(&v) >= min_depth {
                followed += 1;
            }
        }
        assert_eq!(produced, cases, "{id}: every case must produce a value");
        assert!(
            followed > 0,
            "{id}: no case over the batch seed reached depth {min_depth}; the recursive \
             marker was never followed and the property measured nothing about recursion"
        );
        // And the values that followed the marker cross AnyJSON and come back
        // to the same bytes — the property 1b6b4c8 fixed the mapping for,
        // measured here by the sweep's own reproduction entry point, which
        // takes the JSON leg in both byte orders at every phase.
        for i in 0..cases {
            let seed = prop::case_seed(prop::DEFAULT_SEED, i);
            let v = prop::sample(tc, seed).expect("valued above");
            if depth(&v) < min_depth {
                continue;
            }
            let findings = prop::roundtrip_case(tc, seed);
            assert!(
                findings.is_empty(),
                "{id}: a value that followed the recursive marker did not cross AnyJSON: \
                 {findings:?}"
            );
        }
    }
}
