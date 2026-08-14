//! The checked-in generator output must equal what the generator emits now.
//!
//! `tests/emitted/` exists so the wire tests can compile against real generated
//! code. This is what stops it from becoming a fossil: byte equality against a
//! fresh run, for every fixture, in one test — a second fixture with no
//! freshness check is a fossil that looks current because its neighbour is.
//!
//! It deliberately does **not** `mod emitted;`. The check and its bless are the
//! recovery path for a template change, and a recovery path that only compiles
//! once the thing it repairs already compiles is not one: with the fixtures
//! stale, this file still builds and `ORBWEAVER_BLESS=1` still writes them.

/// Every checked-in fixture, and the corpus file it is generated from.
///
/// The path is carried per row rather than assumed: `ir-subset.idl` is under
/// `corpus/services/` and not `corpus/golden/`, because it carries an identity
/// pragma and the golden corpus is deliberately free of them (see the file's
/// own header, and `orbweaver-registry`'s `pragma_corpus.rs`).
const FIXTURES: [(&str, &str); 5] = [
    ("golden/24-skeleton-surface", "f_24_skeleton_surface"),
    ("golden/25-servant-faults", "f_25_servant_faults"),
    ("golden/26-object-identity", "f_26_object_identity"),
    ("golden/27-bounds", "f_27_bounds"),
    ("services/ir-subset", "f_ir_subset"),
];

#[test]
fn the_checked_in_generated_modules_are_current() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut stale = Vec::new();
    for (idl_name, module) in FIXTURES {
        let idl = root.join(format!("../../corpus/{idl_name}.idl"));
        let src = std::fs::read_to_string(&idl).expect("the corpus file");
        let spec = orbweaver_idl::parse(&src).expect("the corpus file parses");
        let mut registry = orbweaver_registry::Registry::new();
        registry.load(&spec).expect("loads");
        let want = orbweaver_gen::emit(&registry, &format!("emitted::{module}")).source;

        let path = root.join(format!("tests/emitted/{module}.rs"));
        if std::env::var_os("ORBWEAVER_BLESS").is_some() {
            std::fs::write(&path, &want).expect("bless");
        }
        if std::fs::read_to_string(&path).unwrap_or_default() != want {
            stale.push(module);
        }
    }
    assert!(
        stale.is_empty(),
        "tests/emitted/ is stale ({}) — re-bless it in the same commit as the template \
         change:\n  ORBWEAVER_BLESS=1 cargo test -p orbweaver-gen",
        stale.join(", ")
    );
}
