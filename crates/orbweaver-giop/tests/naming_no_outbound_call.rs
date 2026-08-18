//! The naming servant has no outbound call in it, checked rather than
//! asserted in prose.
//!
//! `naming_server`'s module docs have claimed since the concurrency batch that
//! "there is no [`Connection`] anywhere in this module, so the *no lock across
//! an outbound call* rule is satisfied structurally rather than by care". That
//! is the property `bind_context` was re-examined against, and the property a
//! federated `resolve` would have spent: chaining means dialling a peer from
//! inside a servant, and once one call site exists the rule stops being true
//! by construction and starts depending on a tripwire firing in somebody's
//! test run.
//!
//! Documenting a rule does not prevent it — this repository has the receipts,
//! and `guarded`'s own module docs are one of them. So the claim is a test.
//! Its sibling,
//! `naming_server::tests::no_operation_of_this_servant_calls_out_from_inside_
//! the_tree_lock`, measures the *runtime* half by dispatching every operation
//! under [`orbweaver_giop::guarded::complaints_about`]; that one would go red
//! if a dial were added inside a lock section. This one goes red if a dial is
//! added **at all**, which is the earlier and cheaper signal, and the one that
//! catches a dial written carefully outside the lock — correct today, and the
//! first step of turning a structural property back into a conventional one.
//!
//! It is a grep, and a grep is exactly as good as what it excludes. The
//! comments are stripped first, because this module's prose is *about*
//! `Connection` and a check that its own subject's documentation defeats is
//! the "green while measuring nothing" failure with a new hat on.

/// The servant under inspection, at compile time — so a rename or a move
/// fails the build rather than silently checking a file that is no longer
/// there. A path-based read could not tell "the property holds" from "I could
/// not find the file", and would report the first.
const SOURCE: &str = include_str!("../src/naming_server.rs");

/// Every way this workspace reaches a peer. A servant naming any of these has
/// an outbound call in it.
///
/// `Connection` and `Pool` are the two front doors; `invoke` is what every
/// call funnels through, in either. The list is the same set of names
/// [`orbweaver_giop::guarded::assert_nothing_held`] is called from, which is
/// what makes it the right list rather than a plausible one.
const OUTBOUND: &[&str] = &["Connection", "Pool::", "Mux", "invoke", "TcpStream", "connect("];

/// Where the servant ends and its tests begin. The property is about the
/// *servant*; the module's own tests drive it with our client and connect and
/// invoke on every other line, which is what they are for.
const TESTS_BEGIN: &str = "#[cfg(test)]";

/// The servant, with `//` line comments removed — doc comments included,
/// since `//!` and `///` are the ones that discuss the thing being looked for.
///
/// String literals are left alone deliberately. A servant that dialled a peer
/// would not hide the fact in a string, and stripping them would need a real
/// lexer to be correct, which is a second implementation to get wrong.
fn code_only() -> String {
    let servant = SOURCE.split(TESTS_BEGIN).next().expect("split yields at least one part");
    servant
        .lines()
        .map(|line| match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_naming_servant_names_nothing_that_dials_a_peer() {
    let code = code_only();
    let found: Vec<&str> = OUTBOUND.iter().copied().filter(|n| code.contains(n)).collect();
    assert!(
        found.is_empty(),
        "crates/orbweaver-giop/src/naming_server.rs now names {found:?}.\n\
         If that is a federated `bind_context` chaining a resolve over the wire, it is a \
         deliberate trade and the module docs' section on it has to be rewritten before this \
         list is: the servant stops satisfying `no lock across an outbound call` structurally \
         and starts satisfying it by care, which is what `guarded` exists because nobody does."
    );
}

/// The control the grep needs to be worth anything: the same scan over a
/// module that *does* dial must find something.
///
/// Without this the test above passes just as happily on an empty string, a
/// path typo, or a `code_only` that strips too much — every one of which is
/// "an unmeasured check reported as a pass".
#[test]
fn the_same_scan_finds_the_outbound_calls_in_a_module_that_has_them() {
    const DIALS: &str = include_str!("../src/pool.rs");
    let code: String = DIALS
        .lines()
        .map(|line| match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let found: Vec<&str> = OUTBOUND.iter().copied().filter(|n| code.contains(n)).collect();
    assert!(
        !found.is_empty(),
        "the scan found no outbound call in `pool.rs`, which is made of them — \
         the comment stripping or the name list is broken, and the sibling test is \
         measuring nothing"
    );
}

/// The doc comments really do discuss `Connection`, so the stripping is load
/// bearing rather than tidiness. If this ever stops being true the sibling
/// test has quietly become a weaker check than it reads as.
#[test]
fn the_comment_stripping_is_what_makes_the_scan_honest() {
    assert!(
        SOURCE.contains("crate::Connection"),
        "the module no longer explains the property in its own docs"
    );
    assert!(
        !code_only().contains("Connection"),
        "a `Connection` survived comment stripping — it is in the code, not the prose"
    );
}

/// The other half of the same worry: the test cut has to be real, or the scan
/// silently inspects an empty string and passes.
#[test]
fn the_scan_covers_the_servant_and_stops_at_its_tests() {
    assert!(SOURCE.contains(TESTS_BEGIN), "the module's test cut moved; the scan's range is wrong");
    let code = code_only();
    assert!(
        code.contains("impl SharedDispatch for NamingServer"),
        "the scan stopped before the dispatch implementation, so it measured almost nothing"
    );
    assert!(
        !code.contains("fn no_operation_of_this_servant"),
        "the scan ran into the tests, whose clients dial by design"
    );
}
