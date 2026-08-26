//! D026 §5 S1 — the seeded naming graph, checked against itself.
//!
//! The wire half is `spikes/seed_naming_client.py`, which reads the same file
//! with omniORB's own `CosNaming` stubs and asks a live `spike-names` whether
//! the stated graph is the graph it is serving. These tests are the half that
//! has to hold before that question is worth asking: a file whose stringified
//! names disagree with its own components would make the wire check fail for a
//! reason that is not about the wire.

use orbweaver_test::state::{NamingGraph, stringify};

#[test]
fn the_naming_graph_loads() {
    let g = NamingGraph::load().expect("naming-graph.json loads");
    assert!(!g.bindings.is_empty(), "the graph binds something");
    assert!(!g.absent.is_empty(), "the graph states at least one absence");
}

/// Every stated stringified name is the stringification of its own
/// components.
///
/// `id.kind` when the kind is non-empty, `id` when it is. This is the check
/// that catches a hand-edited path whose `stringified` was not edited with it
/// — the drift a file with two spellings of one fact always grows.
#[test]
fn a_stringified_name_agrees_with_its_components() {
    let g = NamingGraph::load().unwrap();
    for b in g.bindings.iter().chain(g.absent.iter()) {
        assert_eq!(
            stringify(&b.path),
            b.stringified,
            "the components {:?} stringify to `{}`, but the file says `{}`",
            b.path,
            stringify(&b.path),
            b.stringified
        );
    }
}

/// Every binding's parent context is one the file declares.
///
/// A name bound three components deep needs two contexts to exist first, and
/// a graph that states the leaf without them is not a graph anything can
/// build.
#[test]
fn every_binding_has_its_parent_contexts_declared() {
    let g = NamingGraph::load().unwrap();
    for b in &g.bindings {
        assert!(b.path.len() >= 2, "binding `{}` has no enclosing context", b.stringified);
        let parent: Vec<String> = b.path[..b.path.len() - 1].iter().map(|c| c.id.clone()).collect();
        assert!(
            g.contexts.contains(&parent),
            "binding `{}` needs the context {parent:?}, which the file does not declare \
             (it declares {:?})",
            b.stringified,
            g.contexts
        );
    }
}

/// A name stated absent is not also stated bound.
///
/// Without this the file could say both, and each reader would believe
/// whichever list it consulted first.
#[test]
fn absent_names_are_not_also_bound() {
    let g = NamingGraph::load().unwrap();
    for a in &g.absent {
        assert!(
            !g.bindings.iter().any(|b| b.path == a.path),
            "`{}` is stated both bound and absent",
            a.stringified
        );
    }
}

/// The stated root listing agrees with the contexts the file declares.
///
/// `count` and `names` are two spellings of one fact and both are checked
/// against the third — the declared contexts — rather than against each
/// other.
#[test]
fn the_root_listing_agrees_with_the_declared_contexts() {
    let g = NamingGraph::load().unwrap();
    let mut roots: Vec<String> =
        g.contexts.iter().filter(|c| c.len() == 1).map(|c| c[0].clone()).collect();
    roots.sort();

    let mut stated = g.root_binding_names.clone();
    stated.sort();

    assert_eq!(
        stated, roots,
        "the file says the root holds {stated:?}, but the contexts it declares put {roots:?} there"
    );
    assert_eq!(
        g.root_binding_count as usize,
        roots.len(),
        "the file says the root holds {} binding(s), but names {} of them",
        g.root_binding_count,
        roots.len()
    );
}

/// A component carrying a space keeps it, and the URL form escapes it.
///
/// The one case in the graph whose two spellings differ, which is why it is
/// the one worth pinning: a reader that escapes wrong produces a name the
/// other end cannot resolve, and a graph with only simple names would never
/// execute that path.
#[test]
fn the_url_fragment_escapes_what_the_stringified_form_does_not() {
    let g = NamingGraph::load().unwrap();
    let spaced: Vec<&_> = g.bindings.iter().filter(|b| b.stringified.contains(' ')).collect();
    assert!(!spaced.is_empty(), "the graph states a name with a space in it");
    for b in spaced {
        let frag = b
            .url_fragment
            .as_ref()
            .unwrap_or_else(|| panic!("`{}` has a space but no url_fragment", b.stringified));
        assert!(
            !frag.contains(' '),
            "`{frag}` is offered as a URL fragment but still carries a raw space"
        );
        assert_eq!(
            frag.replace("%20", " "),
            b.stringified,
            "the URL fragment `{frag}` does not unescape to `{}`",
            b.stringified
        );
    }
}
