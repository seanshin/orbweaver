//! The gate over `corpus/include/`, the first multi-file corpus case.
//!
//! `tests/corpus.rs` reads one file at a time, which is all the other corpus
//! directories are. This one only means anything as a set: half its files are
//! leaves, the interesting cases are what happens *between* files, and the
//! manifest (`corpus/include/cases.tsv`) says which files are roots and what
//! each one is filed under.
//!
//! It needs no oracle installed. The `omniidl` column of the manifest is a
//! measurement taken when the directory was written; what runs here is our
//! half, so a regression fails on a laptop with no `omniorb` on it.

use std::path::{Path, PathBuf};

use orbweaver_idl::{SearchPath, check_unit, preprocess_file};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join("corpus").join("include")
}

/// One manifest row.
struct Case {
    root: String,
    search_self: bool,
    verdict: String,
    rule: String,
}

fn cases() -> Vec<Case> {
    let path = dir().join("cases.tsv");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        assert!(f.len() >= 5, "cases.tsv row needs 6 tab-separated fields: {line:?}");
        out.push(Case {
            root: f[0].to_owned(),
            search_self: f[1] == "self",
            verdict: f[2].to_owned(),
            rule: f[3].to_owned(),
        });
    }
    assert!(!out.is_empty(), "cases.tsv lists no cases");
    out
}

/// Every root gets the verdict the manifest files it under, with the rule the
/// manifest names.
///
/// The whole set is run before anything is asserted, and the failures are
/// reported together — a gate that stops at the first one turns a batch into a
/// sequence of single repairs, which is the working model this project spends
/// most of its time not doing.
#[test]
fn every_case_gets_the_verdict_the_manifest_files_it_under() {
    let mut failures: Vec<String> = Vec::new();
    for c in cases() {
        let mut search = SearchPath::new();
        if c.search_self {
            search.push(dir());
        }
        let path = dir().join(&c.root);
        let unit = match preprocess_file(&path, &search) {
            Ok(u) => u,
            Err(e) => {
                failures.push(format!("{}: cannot read: {e}", c.root));
                continue;
            }
        };
        let result = check_unit(&unit);
        let got = match (&result, unit.advice.is_empty()) {
            (Err(_), _) => "reject",
            (Ok(_), true) => "accept",
            (Ok(_), false) => "accept+advice",
        };
        let label = if c.search_self { format!("{} (-I self)", c.root) } else { c.root.clone() };
        if got != c.verdict {
            let detail = match &result {
                Err(ds) => ds.iter().map(|d| unit.render(d)).collect::<Vec<_>>().join("; "),
                Ok(_) => unit.advice.iter().map(|d| unit.render(d)).collect::<Vec<_>>().join("; "),
            };
            failures.push(format!("{label}: filed as {}, got {got} — {detail}", c.verdict));
            continue;
        }
        if c.rule != "-" {
            let seen: Vec<&str> = result
                .as_ref()
                .err()
                .map(|ds| ds.iter().map(|d| d.rule).collect())
                .unwrap_or_else(|| unit.advice.iter().map(|d| d.rule).collect());
            if !seen.contains(&c.rule.as_str()) {
                failures.push(format!("{label}: expected rule {:?}, got {seen:?}", c.rule));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "corpus/include disagrees with its manifest:\n  {}",
        failures.join("\n  ")
    );
}

/// The identity half, which is the cause the estate paid for.
///
/// A file-scope `#pragma prefix` is in force to the end of **its file**.
/// `common.idl` sets one and `types.idl` reopens the same module without one,
/// so the two files' declarations land on different repository ids. Both
/// spellings are well-formed ids for a plausible module, nothing errors either
/// way, and the only thing that ever disagrees is a peer — which is why this is
/// pinned as exact strings rather than as a property.
#[test]
fn a_file_scope_prefix_stops_at_its_own_file() {
    let unit = preprocess_file(&dir().join("types.idl"), &SearchPath::new()).expect("read");
    let spec = check_unit(&unit).expect("types.idl must be clean");
    let id = |n: &str| spec.repository_ids.get(n).map(String::as_str);

    // Declared in common.idl, which sets the prefix.
    assert_eq!(id("Freight::StampText"), Some("IDL:meridian.example/Freight/StampText:1.0"));
    assert_eq!(id("Freight::AuditMark"), Some("IDL:meridian.example/Freight/AuditMark:1.0"));
    // Declared in types.idl, which does not — same module, one file later.
    // `repository_ids` records only ids that *differ* from the plain
    // derivation, so the unprefixed ones are absent by construction.
    assert_eq!(id("Freight::StampList"), None, "the includer must not inherit the prefix");
    assert_eq!(id("Freight::Urgency"), None, "the includer must not inherit the prefix");
}

/// And the mirror, on the guarded chain: `guarded-base.idl` sets a prefix,
/// `guarded-mid.idl` does not, and `guarded-top.idl` reaches both.
#[test]
fn the_prefix_boundary_holds_through_a_two_level_chain() {
    let unit = preprocess_file(&dir().join("guarded-top.idl"), &SearchPath::new()).expect("read");
    let spec = check_unit(&unit).expect("guarded-top.idl must be clean");
    assert_eq!(
        spec.repository_ids.get("Vault::KeyId").map(String::as_str),
        Some("IDL:vault.example/Vault/KeyId:1.0")
    );
    assert_eq!(spec.repository_ids.get("Vault::KeyIds"), None);
    assert_eq!(spec.repository_ids.get("Vault::Locker"), None);
}

/// A guarded file reached twice is spliced once and says nothing about it;
/// an unguarded one is spliced once and does. The pair is the point — an
/// assertion on only the second would pass just as well if we advised on every
/// repeat, which would make the advice worthless.
#[test]
fn the_repeat_advice_fires_only_where_the_guard_is_missing() {
    let guarded =
        preprocess_file(&dir().join("guarded-top.idl"), &SearchPath::new()).expect("read");
    assert!(guarded.advice.is_empty(), "a guarded repeat must be silent: {:?}", guarded.advice);

    let unguarded = preprocess_file(&dir().join("service.idl"), &SearchPath::new()).expect("read");
    let advice: Vec<&str> = unguarded.advice.iter().map(|d| d.rule).collect();
    assert_eq!(advice, vec!["include-unguarded-repeat"]);
    assert!(
        unguarded.advice[0].message.contains("common.idl"),
        "the advice must name the file to guard: {}",
        unguarded.advice[0].message
    );
}

/// A diagnostic inside an included file names that file and its own line, with
/// the chain underneath. Reporting the includer's line instead would point
/// every diagnostic in an estate at the same handful of `#include`s.
#[test]
fn a_diagnostic_is_rendered_against_the_file_it_was_written_in() {
    let unit = preprocess_file(&dir().join("orphan.idl"), &SearchPath::new()).expect("read");
    let err = check_unit(&unit).expect_err("orphan.idl has no such include");
    let rendered = unit.render(&err[0]);
    assert!(rendered.contains("orphan.idl:11:"), "{rendered}");
    assert!(rendered.contains("not-here.idl"), "{rendered}");
    // Every path searched, so the reader can tell a typo from a missing -I.
    assert!(rendered.contains("Searched, in order"), "{rendered}");
}

/// The whole directory, resolved, must not depend on which root you start
/// from for the ids it shares. `qualified.idl` and `types.idl` both pull in
/// `common.idl`; a prefix that leaked in one direction and not the other would
/// show up here and nowhere else.
#[test]
fn a_shared_file_keeps_its_identity_whichever_root_reaches_it() {
    let mut seen: Vec<(String, Option<String>)> = Vec::new();
    for root in ["types.idl", "qualified.idl", "service.idl"] {
        let unit = preprocess_file(&dir().join(root), &SearchPath::new()).expect("read");
        let spec = check_unit(&unit).unwrap_or_else(|d| panic!("{root}: {d:?}"));
        seen.push((root.to_owned(), spec.repository_ids.get("Freight::AuditMark").cloned()));
    }
    let first = seen[0].1.clone();
    assert_eq!(first.as_deref(), Some("IDL:meridian.example/Freight/AuditMark:1.0"), "{seen:?}");
    for (root, id) in &seen {
        assert_eq!(id, &first, "{root} gives a shared declaration a different identity");
    }
}

/// Every repository id the `inc-*` roots produce, as `omniidl` gave them.
///
/// Thirty-two ids over eight files whose only distinguishing feature is that
/// the `#include` is **not at file scope**. That shape existed nowhere in this
/// directory until 2026-08-18, and it is the shape where "reset the prefix"
/// and "reset the id path" stop being the same instruction: seven of these
/// thirty-two disagreed with both oracles when they were first measured, and
/// all seven were the one cause — the boundary was a `#pragma prefix`, which
/// *replaces* the id path, so the restore could name the includer's prefix but
/// never the modules the `#include` sat inside.
///
/// Written out as exact strings rather than derived, for the reason the rest of
/// this file gives: every wrong answer here is a well-formed id for a plausible
/// module, nothing errors either way, and the only thing that ever disagrees is
/// a peer. A table that computed the expectation would compute it with the same
/// rule it is supposed to be checking.
///
/// The values are `omniidl -bpython -Wbinline` on the root, measured
/// 2026-08-18. JacORB 3.9 agrees on all of them except the four leaf ids of
/// `inc-scope-control.idl` and the three of `inc-two-scopes.idl`, where it
/// resets nothing at the boundary; `cases.tsv` records that divergence and why
/// we follow omniidl.
#[test]
fn an_include_inside_a_module_gets_the_ids_omniidl_gives_it() {
    // (root, qualified name, id) — grouped by root, in declaration order.
    let expected: &[(&str, &str, &str)] = &[
        ("inc-leaf-plain.idl", "Parcel::TagNumber", "IDL:Parcel/TagNumber:1.0"),
        ("inc-leaf-plain.idl", "Parcel::Waybill", "IDL:Parcel/Waybill:1.0"),
        ("inc-leaf-plain.idl", "Parcel::Scanner", "IDL:Parcel/Scanner:1.0"),
        ("inc-leaf-prefixed.idl", "Seal::BadgeCode", "IDL:leaf.example/Seal/BadgeCode:1.0"),
        ("inc-leaf-prefixed.idl", "Seal::Stamper", "IDL:leaf.example/Seal/Stamper:1.0"),
        // A file-scope prefix, the include inside `module Yard`. The leaf keeps
        // the id it has alone; `Gate`, after the include, keeps `Yard`.
        ("inc-scope-plain.idl", "Yard::BayNumber", "IDL:hub.example/Yard/BayNumber:1.0"),
        ("inc-scope-plain.idl", "Yard::Parcel::TagNumber", "IDL:Parcel/TagNumber:1.0"),
        ("inc-scope-plain.idl", "Yard::Parcel::Waybill", "IDL:Parcel/Waybill:1.0"),
        ("inc-scope-plain.idl", "Yard::Parcel::Scanner", "IDL:Parcel/Scanner:1.0"),
        ("inc-scope-plain.idl", "Yard::Gate", "IDL:hub.example/Yard/Gate:1.0"),
        // Two prefixes at the splice point, and neither reaches the other.
        ("inc-scope-prefixed.idl", "Wharf::BerthNumber", "IDL:hub.example/Wharf/BerthNumber:1.0"),
        ("inc-scope-prefixed.idl", "Wharf::Seal::BadgeCode", "IDL:leaf.example/Seal/BadgeCode:1.0"),
        ("inc-scope-prefixed.idl", "Wharf::Seal::Stamper", "IDL:leaf.example/Seal/Stamper:1.0"),
        ("inc-scope-prefixed.idl", "Wharf::Crane", "IDL:hub.example/Wharf/Crane:1.0"),
        // No prefix in the includer at all: the restore still has to put
        // `Siding` back, which is what a `#pragma prefix ""` could not do.
        ("inc-scope-bare.idl", "Siding::TrackNumber", "IDL:Siding/TrackNumber:1.0"),
        ("inc-scope-bare.idl", "Siding::Seal::BadgeCode", "IDL:leaf.example/Seal/BadgeCode:1.0"),
        ("inc-scope-bare.idl", "Siding::Seal::Stamper", "IDL:leaf.example/Seal/Stamper:1.0"),
        ("inc-scope-bare.idl", "Siding::Shunt", "IDL:Siding/Shunt:1.0"),
        // The control, and the one place the two oracles disagree: omniidl
        // resets the id path at a file boundary whether or not a prefix is in
        // play, JacORB resets nothing. We follow omniidl.
        ("inc-scope-control.idl", "Ledger::EntryNumber", "IDL:Ledger/EntryNumber:1.0"),
        ("inc-scope-control.idl", "Ledger::Parcel::TagNumber", "IDL:Parcel/TagNumber:1.0"),
        ("inc-scope-control.idl", "Ledger::Parcel::Waybill", "IDL:Parcel/Waybill:1.0"),
        ("inc-scope-control.idl", "Ledger::Parcel::Scanner", "IDL:Parcel/Scanner:1.0"),
        ("inc-scope-control.idl", "Ledger::Journal", "IDL:Ledger/Journal:1.0"),
        // Two prefix scopes, one guarded leaf: it lands in the first and the
        // second does not contain it.
        ("inc-two-scopes.idl", "Alpha::Parcel::TagNumber", "IDL:Parcel/TagNumber:1.0"),
        ("inc-two-scopes.idl", "Alpha::Parcel::Waybill", "IDL:Parcel/Waybill:1.0"),
        ("inc-two-scopes.idl", "Alpha::Parcel::Scanner", "IDL:Parcel/Scanner:1.0"),
        ("inc-two-scopes.idl", "Alpha::Reader", "IDL:alpha.example/Reader:1.0"),
        ("inc-two-scopes.idl", "Beta::Writer", "IDL:beta.example/Writer:1.0"),
        // Depth two, where a restore-to-empty stops being indistinguishable
        // from a restore-to-what-was-saved.
        (
            "inc-nested-scope.idl",
            "Outer::Inner::Seal::BadgeCode",
            "IDL:leaf.example/Seal/BadgeCode:1.0",
        ),
        (
            "inc-nested-scope.idl",
            "Outer::Inner::Seal::Stamper",
            "IDL:leaf.example/Seal/Stamper:1.0",
        ),
        ("inc-nested-scope.idl", "Outer::Inner::Ticket", "IDL:hub.example/Outer/Inner/Ticket:1.0"),
        ("inc-nested-scope.idl", "Outer::Docket", "IDL:hub.example/Outer/Docket:1.0"),
    ];

    let mut failures: Vec<String> = Vec::new();
    let mut root = "";
    let mut spec = None;
    for (r, name, want) in expected {
        if *r != root {
            root = r;
            let unit = preprocess_file(&dir().join(root), &SearchPath::new()).expect("read");
            spec = Some(check_unit(&unit).unwrap_or_else(|d| {
                panic!(
                    "{root} must be clean: {:?}",
                    d.iter().map(|x| unit.render(x)).collect::<Vec<_>>()
                )
            }));
        }
        let s = spec.as_ref().expect("a root was loaded");
        // `repository_ids` records only a *difference* from the plain
        // derivation, so an id equal to the module path is absent by design.
        let got = s
            .repository_ids
            .get(*name)
            .cloned()
            .unwrap_or_else(|| format!("IDL:{}:1.0", name.replace("::", "/")));
        if got != *want {
            failures.push(format!("{root}: {name} is {got}, omniidl says {want}"));
        }
    }
    assert!(
        failures.is_empty(),
        "the in-module include cases disagree with the oracle:\n  {}",
        failures.join("\n  ")
    );
    assert_eq!(expected.len(), 32, "the measurement was 32 ids; adjust the count with the table");
}
