//! The acceptance criterion for `#pragma prefix`/`version`/`ID`: agree with
//! `omniidl`, id for id.
//!
//! `corpus/pragma/expected.tsv` holds what omniidl derived, not what we think
//! it should have derived — including the two rows where omniidl contradicts
//! the specification's wording (`corpus/divergences.tsv`). A repository id is
//! identity on the wire, so "our reading of §14.7.5.1" is not a defence
//! against a peer that answers `_is_a` with a different string.
//!
//! The regeneration command is in `expected.tsv`'s own header, which is where
//! someone staring at a failure will look.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use orbweaver_registry::Registry;

fn corpus_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join("corpus").join(name)
}

fn load(path: &Path) -> Registry {
    let src = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let spec =
        orbweaver_idl::parse(&src).unwrap_or_else(|e| panic!("{} must parse: {e}", path.display()));
    let mut reg = Registry::new();
    reg.load(&spec).unwrap_or_else(|e| panic!("{} must load: {e}", path.display()));
    reg
}

/// `file -> [(qualified, id)]`, in the order the file lists them.
fn expectations() -> BTreeMap<String, Vec<(String, String)>> {
    let path = corpus_dir("pragma").join("expected.tsv");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let mut out: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut cols = line.split('\t');
        let (Some(file), Some(qualified), Some(id)) = (cols.next(), cols.next(), cols.next())
        else {
            panic!("malformed row in {}: {line:?}", path.display());
        };
        out.entry(file.to_owned()).or_default().push((qualified.to_owned(), id.to_owned()));
    }
    assert!(!out.is_empty(), "{} lists no cases", path.display());
    out
}

/// Every id in the case set, in one pass, reported as one list of failures.
///
/// Deliberately not `assert` per row: a failure here is a rule that is wrong,
/// and seeing every case the rule broke is what tells you which rule it is.
/// Stopping at the first would have hidden that both prefix-scope failures in
/// this batch's first round had a single cause.
#[test]
fn pragma_ids_match_omniidl() {
    let mut failures = Vec::new();
    for (file, rows) in expectations() {
        let reg = load(&corpus_dir("pragma").join(&file));
        for (qualified, want) in rows {
            match reg.id_of(&qualified) {
                Some(got) if *got == want => {}
                Some(got) => failures.push(format!("{file}: {qualified} is {got}, omniidl {want}")),
                None => failures.push(format!("{file}: {qualified} is not registered at all")),
            }
        }
    }
    assert!(failures.is_empty(), "omniidl disagrees with us:\n  {}", failures.join("\n  "));
}

/// The inverse direction: nothing extra, and every id resolves back to the
/// name it came from.
///
/// A prefix makes the id-to-name mapping unrecoverable by splitting on `/`
/// (`IDL:acme.com/p01/Account:1.0` is `p01::Account`, not
/// `acme.com::p01::Account`), so the registry records it. If that record were
/// missing the IFR facade would report a containing module that does not
/// exist.
#[test]
fn every_prefixed_id_maps_back_to_its_qualified_name() {
    for (file, rows) in expectations() {
        let reg = load(&corpus_dir("pragma").join(&file));
        for (qualified, id) in rows {
            assert_eq!(
                reg.qualified_name(&id),
                Some(qualified.as_str()),
                "{file}: {id} must map back to {qualified}"
            );
        }
    }
}

/// **No pragma means no change.** The guard on every id this project already
/// publishes.
///
/// The front end records only *differences* from the plain derivation, so an
/// empty override map is a proof rather than a sample: a file with no identity
/// pragma cannot have had an id altered, whatever the pragma code does. A
/// regression here would silently rename every existing type, which is the one
/// failure mode that would be worse than not implementing pragmas at all.
#[test]
fn the_existing_corpus_has_no_pragma_derived_ids() {
    let mut offenders = Vec::new();
    for dir in ["golden", "negative", "annotations", "requirements/generated"] {
        let root = corpus_dir(dir);
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().is_none_or(|x| x != "idl") {
                continue;
            }
            let src = std::fs::read_to_string(&path).unwrap();
            // Negatives are supposed to fail; only the ones that parse have
            // ids to be wrong about.
            let Ok(spec) = orbweaver_idl::parse(&src) else { continue };
            if !spec.repository_ids.is_empty() {
                offenders.push(format!("{}: {:?}", path.display(), spec.repository_ids));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these carry no identity pragma, so their ids must be untouched:\n  {}",
        offenders.join("\n  ")
    );
}

/// Every id we now derive is one ingestion would also accept from a peer.
///
/// The two directions have to agree or prefixes become a one-way door: we
/// would publish `IDL:acme.com/bank/Account:1.0` and then refuse the identical
/// string when the deployment's own IFR described it back to us. The validator
/// already admitted a dotted first segment — this pins that it stays admitted,
/// and that nothing else about a pragma-derived id trips it.
#[test]
fn every_derived_id_survives_the_ingestion_validator() {
    let limits = orbweaver_registry::ingest::Limits::default();
    let mut refused = Vec::new();
    for (file, rows) in expectations() {
        for (_, id) in rows {
            if let Err(reason) = orbweaver_registry::ingest::validate_repository_id(&id, &limits) {
                refused.push(format!("{file}: {id} — {reason:?}"));
            }
        }
    }
    assert!(
        refused.is_empty(),
        "we would refuse ids we ourselves derive:\n  {}",
        refused.join("\n  ")
    );
}

/// An inheritance edge crossing a prefix boundary keeps the base's own id.
///
/// The case the whole change turns on: ids that moved in the registry but not
/// in `_is_a`'s walk would be worse than no prefix support, because the
/// disagreement would then be with ourselves as well as with the peer.
#[test]
fn is_a_follows_a_base_declared_under_a_different_prefix() {
    let reg = load(&corpus_dir("pragma").join("p10-inheritance-across-prefixes.idl"));
    let leaf = "IDL:derived.example/p10derived/Leaf:1.0";
    let root = "IDL:base.example/p10base/Root:1.0";
    assert!(reg.is_a(leaf, root), "the base's id is the base's, not the deriving scope's");
    assert_eq!(reg.ancestors(leaf), [root]);
    let (owner, _) = reg.resolve_operation(leaf, "a").expect("inherited across prefixes");
    assert_eq!(owner, root);
}

/// A `raises` clause names the exception by the same id the exception has.
///
/// Two derivations of the same id are two chances to disagree; a caught
/// exception whose id does not match the one in the operation's raises list is
/// unmatchable, and locally consistent enough to pass every test that does not
/// check exactly this.
#[test]
fn a_raises_edge_uses_the_prefixed_exception_id() {
    let reg = load(&corpus_dir("pragma").join("p16-prefix-exception-and-raises.idl"));
    let doer = reg.interface("IDL:acme.com/p16/Doer:1.0").expect("Doer is registered");
    assert_eq!(doer.operations["go"].raises, ["IDL:acme.com/p16/Refused:1.0"]);
    assert!(reg.typecode("IDL:acme.com/p16/Refused:1.0").is_some(), "and it is registered there");
}
