//! A constant's value, all the way from the source text to the registry.
//!
//! # The gate this is
//!
//! Two components fold constants: `orbweaver_idl::sema`, which must, because
//! `const short S = 40000;` is an error in the language and errors in the
//! language are reported by the thing that reads the language; and
//! `orbweaver_registry`, which must, because every consumer reads a value from
//! it rather than carrying an evaluator of its own.
//!
//! Two folders that agree by construction today will disagree by accident
//! later, and the one that disagrees silently is the one that ships. So the
//! agreement is a test rather than a hope:
//! [`the_front_end_and_the_registry_agree_over_golden`] walks every file in
//! `corpus/golden/` and asserts the two halves of one claim —
//!
//! * every constant the front end **accepts** is one the registry gives a
//!   value to, and
//! * a file the front end **rejects** never reaches the registry at all.
//!
//! The first half is the one with teeth. A constant with no value is skipped
//! by both emitters, silently, so a divergence in that direction is invisible
//! at every layer above it — which is exactly what happened to `fixed`:
//! `const fixed TAX = 9.9d;` validated clean, folded to a rounded `f64`, was
//! refused by `coerce` for want of a decimal variant, and was reported to
//! nobody.
//!
//! *두 개의 폴더가 있으면 언젠가 어긋난다. 어긋남을 조용히 넘기지 않도록
//! 합의를 테스트로 만든다.*

use std::path::{Path, PathBuf};

use orbweaver_registry::{ConstValue, Entry, Registry};

fn corpus(name: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join("corpus").join(name);
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "idl"))
        .collect();
    out.sort();
    assert!(!out.is_empty(), "{} is empty", dir.display());
    out
}

/// Loads a file and returns every constant it declares, id and value.
fn constants(path: &Path) -> Vec<(String, Option<ConstValue>)> {
    let src = std::fs::read_to_string(path).expect("readable");
    let spec =
        orbweaver_idl::parse(&src).unwrap_or_else(|e| panic!("{} must parse: {e}", path.display()));
    let analysis = orbweaver_idl::sema::analyse(&spec);
    assert!(
        analysis.is_ok(),
        "{} is in corpus/golden and must be semantically clean: {:?}",
        path.display(),
        analysis.diagnostics
    );
    let mut reg = Registry::new();
    reg.load(&spec).unwrap_or_else(|e| panic!("{} must load: {e}", path.display()));
    let ids: Vec<String> = reg.ids().cloned().collect();
    ids.into_iter()
        .filter_map(|id| match reg.get(&id) {
            Some(Entry::Const { value, .. }) => Some((id.clone(), value.clone())),
            _ => None,
        })
        .collect()
}

/// Every constant the front end accepts is one the registry can evaluate.
///
/// The exception list is empty on purpose. When it stops being empty, the
/// entry has to say *why* a constant the language accepts has no value — and
/// "the registry has no variant for it" is the answer that cost this project
/// a silently rounded tax rate.
#[test]
fn the_front_end_and_the_registry_agree_over_golden() {
    let mut unevaluated: Vec<String> = Vec::new();
    let mut total = 0usize;
    for path in corpus("golden") {
        for (id, value) in constants(&path) {
            total += 1;
            if value.is_none() {
                unevaluated
                    .push(format!("{} :: {id}", path.file_name().unwrap().to_string_lossy()));
            }
        }
    }
    assert!(total >= 20, "the golden corpus should declare constants; found {total}");
    assert!(
        unevaluated.is_empty(),
        "{} of {total} constant(s) validate clean and have no value in the registry — each is \
         invisible to both emitters, to the console catalogue and to idl-diff's §5.3 \
         comparison: {unevaluated:#?}",
        unevaluated.len()
    );
}

/// The decimals, spelled back. Each is checked against `omniidl -b dump`'s own
/// rendering of the same file, which is where the normal form came from.
#[test]
fn a_fixed_constant_keeps_the_decimal_it_was_written_as() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("corpus/golden/31-const-values.idl");
    let found = constants(&path);
    let get = |name: &str| {
        found
            .iter()
            .find(|(id, _)| id.contains(&format!("/{name}:")))
            .unwrap_or_else(|| panic!("{name} is declared in 31-const-values.idl: {found:#?}"))
            .1
            .clone()
            .unwrap_or_else(|| panic!("{name} has no value"))
    };
    let decimal = |name: &str| get(name).as_decimal().expect("a fixed constant");

    // 9.9 has no `f64`; the nearest is 9.90000000000000035527136788005009.
    assert_eq!(decimal("TAX_RATE"), "9.9");
    assert_eq!(decimal("UNIT_PRICE"), "1.005");
    assert_eq!(decimal("EPSILON"), "-0.001");
    assert_eq!(decimal("ZERO"), "0");
    assert_eq!(decimal("HALF"), "0.5");
    assert_eq!(decimal("ONE"), "1");
    assert_eq!(decimal("WIDEST"), "1234567890123456789012345678901");

    // Trailing fractional zeros are not part of the value, which is the
    // oracle's normalisation and not ours: omniidl dumps `9.90d` as `9.9d`.
    // A differ that reported this pair as a change would report a change that
    // no deployed compiler can see.
    assert_eq!(decimal("SAME_AS_TAX_RATE"), decimal("TAX_RATE"));

    // Decimal arithmetic, exactly. `99999.99 - 0.01` through an `f64` gives
    // 99999.98000000000465661287307739; omniidl dumps `99999.98d`.
    assert_eq!(decimal("DERIVED"), "99999.98");
}

/// The integers that do not fit an `i64`, which is what the lexer used to
/// funnel every integer literal through.
#[test]
fn an_unsigned_long_long_constant_reaches_its_own_maximum() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("corpus/golden/31-const-values.idl");
    let found = constants(&path);
    let get = |name: &str| {
        found
            .iter()
            .find(|(id, _)| id.contains(&format!("/{name}:")))
            .and_then(|(_, v)| v.clone())
            .unwrap_or_else(|| panic!("{name} has no value: {found:#?}"))
    };
    let max = i128::from(u64::MAX);
    assert_eq!(get("ULL_MAX"), ConstValue::Int(max));
    // The same value written in hex. Both spellings were refused by the lexer,
    // in two different messages, for one reason.
    assert_eq!(get("ULL_MAX_HEX"), ConstValue::Int(max));
    assert_eq!(get("LL_MAX"), ConstValue::Int(i128::from(i64::MAX)));
    assert_eq!(get("PERMISSIONS"), ConstValue::Int(0o755));
    assert_eq!(get("S_MIN"), ConstValue::Int(i128::from(i16::MIN)));
    assert_eq!(get("WITHIN_AFTER_FOLDING"), ConstValue::Int(30_000));
}

/// A `fixed` beyond `i128`'s reach folds to nothing rather than to a wrapped
/// number — the negative control for the exactness above.
#[test]
fn a_fixed_expression_that_cannot_be_exact_has_no_value() {
    let src = "module m {
        const fixed WIDE = 1234567890123456789012345678901d;
        const fixed OVERFLOWS = WIDE * WIDE;
        const fixed INEXACT = 1.0d / 3.0d;
    };";
    let spec = orbweaver_idl::parse(src).expect("parses");
    let mut reg = Registry::new();
    reg.load(&spec).expect("loads");
    let value = |name: &str| {
        let id = reg
            .ids()
            .find(|id| id.contains(&format!("/{name}:")))
            .unwrap_or_else(|| panic!("{name} is registered"))
            .clone();
        match reg.get(&id) {
            Some(Entry::Const { value, .. }) => value.clone(),
            _ => panic!("{name} is a constant"),
        }
    };
    assert!(value("WIDE").is_some(), "31 digits is inside the range");
    assert_eq!(value("OVERFLOWS"), None, "62 digits is past i128; nothing is invented");
    assert_eq!(value("INEXACT"), None, "1/3 has no exact decimal and IDL names no rounding");
}
