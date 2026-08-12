//! Contract evolution: what changed between two versions of an interface, and
//! whether deployed callers survive it.
//!
//! # Why protobuf intuition is wrong here
//!
//! CDR encodes **by position, not by tag**. A protobuf field number survives
//! reordering and an unknown field is skipped; a CDR member has neither a tag
//! nor a length, so inserting one shifts every byte after it and the receiver
//! reads the next member's bytes as this one's. Nothing detects that — the
//! values are simply wrong.
//!
//! Anyone whose instincts came from protobuf, JSON or Avro will over-trust IDL
//! evolution, so `docs/PLAN.md` §5.3 writes the rules down and this module
//! enforces them.
//!
//! # What a verdict means
//!
//! Not "safe" or "unsafe" in the abstract, but *what has to happen for nobody
//! to break*: sometimes nothing, sometimes an ordering constraint on the
//! rollout, sometimes a new interface version.

use std::collections::BTreeSet;

use orbweaver_giop::typecode::TypeCode;

use crate::{Entry, OperationSig, Registry, RepositoryId};

/// How much damage a change does to deployed peers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    /// Nobody has to do anything.
    Compatible,
    /// Compatible only if the rollout is ordered correctly.
    ServerFirst,
    /// Legal on the wire, but a receiver that has not been updated will see a
    /// value it has no meaning for.
    ConditionallyBreaking,
    /// Deployed peers misread the data or fail outright.
    Breaking,
}

impl Verdict {
    /// Whether a change gate should refuse this without an explicit approval.
    pub fn blocks_release(self) -> bool {
        matches!(self, Verdict::Breaking | Verdict::ConditionallyBreaking)
    }

    /// A short label for a report.
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Compatible => "compatible",
            Verdict::ServerFirst => "server-first",
            Verdict::ConditionallyBreaking => "conditionally breaking",
            Verdict::Breaking => "BREAKING",
        }
    }
}

/// One difference between two contracts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The repository id the change is about.
    pub id: RepositoryId,
    /// What changed, in a form a person can act on.
    pub what: String,
    /// Why it carries the verdict it does.
    pub why: &'static str,
    /// The verdict.
    pub verdict: Verdict,
}

impl std::fmt::Display for Change {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}: {} — {}", self.verdict.label(), self.id, self.what, self.why)
    }
}

/// Compares two registries and reports every difference.
///
/// Ordered worst-first, because a report read from the top should start with
/// the thing that stops the release.
pub fn diff(old: &Registry, new: &Registry) -> Vec<Change> {
    let mut out = Vec::new();
    let old_ids: BTreeSet<&RepositoryId> = old.ids().collect();
    let new_ids: BTreeSet<&RepositoryId> = new.ids().collect();

    for &id in old_ids.difference(&new_ids) {
        out.push(Change {
            id: id.clone(),
            what: "removed".into(),
            // A rename looks exactly like this pair of changes, and there is no
            // way to tell them apart: the repository id *is* the identity, so a
            // renamed type is a different type to every deployed caller.
            why: "the repository id is the contract identity, so removing or renaming it \
                  makes every existing reference to it unresolvable",
            verdict: Verdict::Breaking,
        });
    }
    for &id in new_ids.difference(&old_ids) {
        out.push(Change {
            id: id.clone(),
            what: "added".into(),
            why: "nothing deployed refers to it yet",
            verdict: Verdict::Compatible,
        });
    }

    for &id in old_ids.intersection(&new_ids) {
        match (old.get(id), new.get(id)) {
            (Some(Entry::Interface(a)), Some(Entry::Interface(b))) => {
                diff_interface(id, a, b, &mut out)
            }
            (Some(Entry::Type(a)), Some(Entry::Type(b))) => diff_type(id, a, b, &mut out),
            (Some(Entry::Const { tc: a }), Some(Entry::Const { tc: b })) if a != b => {
                out.push(Change {
                    id: id.clone(),
                    what: "constant changed type".into(),
                    why: "a constant's type is part of every expression that uses it",
                    verdict: Verdict::Breaking,
                });
            }
            (Some(_), Some(_)) => out.push(Change {
                id: id.clone(),
                what: "changed to a different kind of declaration".into(),
                why: "a name that was one kind of thing and is now another cannot be \
                      compatible with anything that used it",
                verdict: Verdict::Breaking,
            }),
            _ => {}
        }
    }

    out.sort_by(|a, b| b.verdict.cmp(&a.verdict).then(a.id.cmp(&b.id)).then(a.what.cmp(&b.what)));
    out
}

fn diff_interface(
    id: &RepositoryId,
    a: &crate::InterfaceEntry,
    b: &crate::InterfaceEntry,
    out: &mut Vec<Change>,
) {
    if a.bases != b.bases {
        let lost: Vec<_> = a.bases.iter().filter(|x| !b.bases.contains(x)).collect();
        if lost.is_empty() {
            out.push(Change {
                id: id.clone(),
                what: "gained a base interface".into(),
                why: "an added base only widens what the interface answers to",
                verdict: Verdict::Compatible,
            });
        } else {
            out.push(Change {
                id: id.clone(),
                what: format!("lost base interface(s) {lost:?}"),
                why: "callers that narrowed to the removed base can no longer do so, and \
                      inherited operations disappear with it",
                verdict: Verdict::Breaking,
            });
        }
    }

    for (name, _) in a.operations.iter().filter(|(n, _)| !b.operations.contains_key(*n)) {
        out.push(Change {
            id: id.clone(),
            what: format!("operation {name:?} removed"),
            why: "a caller that invokes it receives BAD_OPERATION",
            verdict: Verdict::Breaking,
        });
    }
    for (name, _) in b.operations.iter().filter(|(n, _)| !a.operations.contains_key(*n)) {
        out.push(Change {
            id: id.clone(),
            what: format!("operation {name:?} added"),
            // Not simply compatible: a client built against the new contract
            // and pointed at a server still running the old one gets
            // BAD_OPERATION. The constraint is on rollout order, not on the
            // change itself.
            why: "servers must be updated before clients, or a new client calling an old \
                  server receives BAD_OPERATION",
            verdict: Verdict::ServerFirst,
        });
    }
    for (name, oa) in &a.operations {
        let Some(ob) = b.operations.get(name) else { continue };
        if let Some(what) = operation_difference(oa, ob) {
            out.push(Change {
                id: id.clone(),
                what: format!("operation {name:?} {what}"),
                why: "arguments and results are marshalled by position, so a signature \
                      change makes existing callers write and read the wrong bytes",
                verdict: Verdict::Breaking,
            });
        }
    }

    for (name, _) in a.attributes.iter().filter(|(n, _)| !b.attributes.contains_key(*n)) {
        out.push(Change {
            id: id.clone(),
            what: format!("attribute {name:?} removed"),
            why: "its accessor operations disappear with it",
            verdict: Verdict::Breaking,
        });
    }
    for (name, _) in b.attributes.iter().filter(|(n, _)| !a.attributes.contains_key(*n)) {
        out.push(Change {
            id: id.clone(),
            what: format!("attribute {name:?} added"),
            why: "servers must be updated before clients, or the accessor is unknown",
            verdict: Verdict::ServerFirst,
        });
    }
    for (name, aa) in &a.attributes {
        let Some(ab) = b.attributes.get(name) else { continue };
        if !same_identity(&aa.tc, &ab.tc) {
            out.push(Change {
                id: id.clone(),
                what: format!("attribute {name:?} changed type"),
                why: "the accessor's result is marshalled as the old type by every \
                      deployed caller",
                verdict: Verdict::Breaking,
            });
        } else if aa.readonly != ab.readonly {
            let verdict = if aa.readonly { Verdict::ServerFirst } else { Verdict::Breaking };
            out.push(Change {
                id: id.clone(),
                what: format!(
                    "attribute {name:?} became {}",
                    if ab.readonly { "read-only" } else { "writable" }
                ),
                why: if ab.readonly {
                    "callers that set it will start receiving BAD_OPERATION"
                } else {
                    "adding a setter needs servers first, like any added operation"
                },
                verdict: if ab.readonly { Verdict::Breaking } else { verdict },
            });
        }
    }
}

/// Whether two type *references* denote the same type as a contract, rather
/// than being structurally identical.
///
/// A named type is its repository id and nothing else. Comparing structurally
/// instead makes one edit to a struct resurface on every operation, attribute
/// and member that mentions it: the first run of this differ reported a single
/// swapped pair of members three times, twice as "operation `first` changed the
/// type of parameter 0", which sends a reviewer looking for an edit to `first`
/// that does not exist. The type's own entry already carries that change, so
/// everything referring to it stays quiet and a signature line means what it
/// says — the operation itself was edited.
fn same_identity(a: &TypeCode, b: &TypeCode) -> bool {
    match (a, b) {
        (
            TypeCode::Sequence { element: ea, bound: ba },
            TypeCode::Sequence { element: eb, bound: bb },
        ) => ba == bb && same_identity(ea, eb),
        (
            TypeCode::Array { element: ea, length: la },
            TypeCode::Array { element: eb, length: lb },
        ) => la == lb && same_identity(ea, eb),
        // An unnamed type has no identity apart from its structure, so for
        // primitives and bounded strings this is the ordinary comparison.
        _ => match (a.repository_id(), b.repository_id()) {
            (Some(x), Some(y)) => x == y,
            _ => a == b,
        },
    }
}

fn operation_difference(a: &OperationSig, b: &OperationSig) -> Option<String> {
    if a.oneway != b.oneway {
        // A oneway sends no reply at all, so this changes the message exchange
        // rather than only the payload.
        return Some(format!("changed to {}", if b.oneway { "oneway" } else { "twoway" }));
    }
    if !same_identity(&a.returns, &b.returns) {
        return Some("changed return type".into());
    }
    if a.params.len() != b.params.len() {
        return Some(format!(
            "changed parameter count from {} to {}",
            a.params.len(),
            b.params.len()
        ));
    }
    for (i, (pa, pb)) in a.params.iter().zip(&b.params).enumerate() {
        if !same_identity(&pa.tc, &pb.tc) {
            return Some(format!("changed the type of parameter {i} ({:?})", pa.name));
        }
        if pa.direction != pb.direction {
            return Some(format!("changed the direction of parameter {i} ({:?})", pa.name));
        }
    }
    if a.raises != b.raises {
        // A user exception is a reply the caller must be able to decode; one it
        // has never heard of is undecodable rather than merely unexpected.
        return Some("changed its raises clause".into());
    }
    None
}

fn diff_type(id: &RepositoryId, a: &TypeCode, b: &TypeCode, out: &mut Vec<Change>) {
    if a == b {
        return;
    }
    match (a, b) {
        (
            TypeCode::Struct { members: ma, .. } | TypeCode::Except { members: ma, .. },
            TypeCode::Struct { members: mb, .. } | TypeCode::Except { members: mb, .. },
        ) => {
            let names_a: Vec<&str> = ma.iter().map(|m| m.name.as_str()).collect();
            let names_b: Vec<&str> = mb.iter().map(|m| m.name.as_str()).collect();

            if names_a.len() == names_b.len()
                && names_a.iter().collect::<BTreeSet<_>>()
                    == names_b.iter().collect::<BTreeSet<_>>()
                && names_a != names_b
            {
                out.push(Change {
                    id: id.clone(),
                    what: format!("members reordered: {names_a:?} became {names_b:?}"),
                    why: "CDR marshals members by position and carries no tags, so a \
                          reordered struct is read field-for-field into the wrong members \
                          and nothing detects it",
                    verdict: Verdict::Breaking,
                });
                return;
            }
            for n in names_a.iter().filter(|n| !names_b.contains(n)) {
                out.push(Change {
                    id: id.clone(),
                    what: format!("member {n:?} removed"),
                    why: "positional encoding means every following member shifts",
                    verdict: Verdict::Breaking,
                });
            }
            for n in names_b.iter().filter(|n| !names_a.contains(n)) {
                out.push(Change {
                    id: id.clone(),
                    what: format!("member {n:?} added"),
                    why: "a CDR member has no tag and no length, so an added one is read \
                          as part of whatever followed it",
                    verdict: Verdict::Breaking,
                });
            }
            for (x, y) in ma.iter().zip(mb) {
                if x.name == y.name && !same_identity(&x.tc, &y.tc) {
                    out.push(Change {
                        id: id.clone(),
                        what: format!("member {:?} changed type", x.name),
                        why: "the receiver reads the old type's byte count and alignment",
                        verdict: Verdict::Breaking,
                    });
                }
            }
        }
        (TypeCode::Enum { members: ea, .. }, TypeCode::Enum { members: eb, .. }) => {
            let common = ea.len().min(eb.len());
            if ea[..common] != eb[..common] {
                out.push(Change {
                    id: id.clone(),
                    what: "enumerators reordered or renamed".into(),
                    why: "an enum travels as its ordinal, so reordering silently changes \
                          the meaning of every value already in flight or at rest",
                    verdict: Verdict::Breaking,
                });
            } else if eb.len() > ea.len() {
                out.push(Change {
                    id: id.clone(),
                    what: format!("enumerator(s) appended: {:?}", &eb[common..]),
                    // Appending is wire-legal, and that is exactly the trap: the
                    // value arrives intact and means nothing to the receiver.
                    why: "old receivers accept the ordinal and have no case for it, so this \
                          is safe only once every receiver is updated",
                    verdict: Verdict::ConditionallyBreaking,
                });
            } else if eb.len() < ea.len() {
                out.push(Change {
                    id: id.clone(),
                    what: format!("enumerator(s) removed: {:?}", &ea[common..]),
                    why: "senders may still emit the removed ordinal",
                    verdict: Verdict::Breaking,
                });
            }
        }
        (TypeCode::Union { cases: ca, .. }, TypeCode::Union { cases: cb, .. }) => {
            let removed: Vec<_> =
                ca.iter().filter(|x| !cb.iter().any(|y| y.label == x.label)).collect();
            let added: Vec<_> =
                cb.iter().filter(|y| !ca.iter().any(|x| x.label == y.label)).collect();
            if !removed.is_empty() {
                out.push(Change {
                    id: id.clone(),
                    what: format!(
                        "union case(s) removed: {:?}",
                        removed.iter().map(|c| &c.name).collect::<Vec<_>>()
                    ),
                    why: "a sender may still select the removed discriminator",
                    verdict: Verdict::Breaking,
                });
            }
            if !added.is_empty() {
                out.push(Change {
                    id: id.clone(),
                    what: format!(
                        "union case(s) added: {:?}",
                        added.iter().map(|c| &c.name).collect::<Vec<_>>()
                    ),
                    why: "an old receiver has no branch for the new discriminator",
                    verdict: Verdict::ConditionallyBreaking,
                });
            }
            for (x, y) in ca.iter().zip(cb) {
                if x.label == y.label && !same_identity(&x.tc, &y.tc) {
                    out.push(Change {
                        id: id.clone(),
                        what: format!("union case {:?} changed type", x.name),
                        why: "the receiver reads the old branch's bytes",
                        verdict: Verdict::Breaking,
                    });
                }
            }
        }
        _ => out.push(Change {
            id: id.clone(),
            what: "type definition changed".into(),
            why: "the encoded form differs, and CDR gives a receiver no way to notice",
            verdict: Verdict::Breaking,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg(src: &str) -> Registry {
        let spec = orbweaver_idl::parse(src).expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        r
    }

    fn verdicts(before: &str, after: &str) -> Vec<(Verdict, String)> {
        diff(&reg(before), &reg(after)).into_iter().map(|c| (c.verdict, c.what)).collect()
    }

    fn worst(before: &str, after: &str) -> Verdict {
        diff(&reg(before), &reg(after)).first().map_or(Verdict::Compatible, |c| c.verdict)
    }

    #[test]
    fn an_unchanged_contract_has_no_changes() {
        let src = "module m { struct S { long a; }; interface I { S get(); }; };";
        assert!(diff(&reg(src), &reg(src)).is_empty());
    }

    /// The rule protobuf intuition gets wrong: a CDR member has no tag, so
    /// adding one shifts everything after it.
    #[test]
    fn adding_a_struct_member_is_breaking() {
        let v = verdicts(
            "module m { struct S { long a; }; };",
            "module m { struct S { long a; long b; }; };",
        );
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].0, Verdict::Breaking);
        assert!(v[0].1.contains("added"));
    }

    #[test]
    fn reordering_members_is_breaking_and_says_so_plainly() {
        let c = diff(
            &reg("module m { struct S { long a; string b; }; };"),
            &reg("module m { struct S { string b; long a; }; };"),
        );
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].verdict, Verdict::Breaking);
        assert!(c[0].what.contains("reordered"), "{}", c[0].what);
        assert!(c[0].why.contains("no tags"), "the reason must name the mechanism");
    }

    #[test]
    fn retyping_a_member_is_breaking() {
        assert_eq!(
            worst("module m { struct S { long a; }; };", "module m { struct S { double a; }; };"),
            Verdict::Breaking
        );
    }

    /// Adding an operation is safe for existing callers and unsafe for new ones
    /// pointed at old servers, which is a rollout constraint rather than a
    /// property of the change.
    #[test]
    fn adding_an_operation_requires_servers_first() {
        let v = verdicts(
            "module m { interface I { long a(); }; };",
            "module m { interface I { long a(); long b(); }; };",
        );
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].0, Verdict::ServerFirst);
    }

    #[test]
    fn removing_an_operation_is_breaking() {
        assert_eq!(
            worst(
                "module m { interface I { long a(); long b(); }; };",
                "module m { interface I { long a(); }; };"
            ),
            Verdict::Breaking
        );
    }

    #[test]
    fn changing_a_signature_is_breaking_in_every_direction() {
        let base = "module m { interface I { long f(in long a); }; };";
        for after in [
            "module m { interface I { long f(in long a, in long b); }; };",
            "module m { interface I { double f(in long a); }; };",
            "module m { interface I { long f(in double a); }; };",
            "module m { interface I { long f(out long a); }; };",
            "module m { interface I { oneway void f(in long a); }; };",
        ] {
            assert_eq!(worst(base, after), Verdict::Breaking, "{after}");
        }
    }

    /// A user exception the caller has never heard of is undecodable, not
    /// merely unexpected.
    #[test]
    fn changing_the_raises_clause_is_breaking() {
        assert_eq!(
            worst(
                "module m { exception E {}; interface I { long f(); }; };",
                "module m { exception E {}; interface I { long f() raises (E); }; };"
            ),
            Verdict::Breaking
        );
    }

    /// Appending an enumerator is wire-legal, and that is the trap: the value
    /// arrives intact and means nothing to an old receiver.
    #[test]
    fn appending_an_enumerator_is_conditionally_breaking() {
        let v = verdicts("module m { enum E { A, B }; };", "module m { enum E { A, B, C }; };");
        assert_eq!(v.len(), 1, "{v:?}");
        assert_eq!(v[0].0, Verdict::ConditionallyBreaking);
        assert!(Verdict::ConditionallyBreaking.blocks_release());
    }

    #[test]
    fn reordering_enumerators_is_breaking_because_the_ordinal_travels() {
        let c =
            diff(&reg("module m { enum E { A, B }; };"), &reg("module m { enum E { B, A }; };"));
        assert_eq!(c[0].verdict, Verdict::Breaking);
        assert!(c[0].why.contains("ordinal"));
    }

    #[test]
    fn adding_a_type_or_interface_is_compatible() {
        let v = verdicts(
            "module m { struct S { long a; }; };",
            "module m { struct S { long a; }; struct T { long b; }; interface I { T f(); }; };",
        );
        assert!(v.iter().all(|(x, _)| *x == Verdict::Compatible), "{v:?}");
    }

    /// A rename is indistinguishable from a removal plus an addition, because
    /// the repository id *is* the identity.
    #[test]
    fn renaming_reads_as_a_removal_and_is_breaking() {
        let c = diff(
            &reg("module m { struct S { long a; }; };"),
            &reg("module m { struct Renamed { long a; }; };"),
        );
        assert_eq!(c[0].verdict, Verdict::Breaking);
        assert!(c[0].what.contains("removed"));
        assert!(c.iter().any(|x| x.verdict == Verdict::Compatible && x.what.contains("added")));
    }

    #[test]
    fn union_cases_added_and_removed_differ_in_severity() {
        let added = verdicts(
            "module m { union U switch (long) { case 1: long a; }; };",
            "module m { union U switch (long) { case 1: long a; case 2: long b; }; };",
        );
        assert_eq!(added[0].0, Verdict::ConditionallyBreaking, "{added:?}");
        let removed = verdicts(
            "module m { union U switch (long) { case 1: long a; case 2: long b; }; };",
            "module m { union U switch (long) { case 1: long a; }; };",
        );
        assert_eq!(removed[0].0, Verdict::Breaking, "{removed:?}");
    }

    #[test]
    fn attribute_mutability_changes_are_asymmetric() {
        // Losing a setter breaks callers that used it.
        assert_eq!(
            worst(
                "module m { interface I { attribute long n; }; };",
                "module m { interface I { readonly attribute long n; }; };"
            ),
            Verdict::Breaking
        );
        // Gaining one is an added operation.
        assert_eq!(
            worst(
                "module m { interface I { readonly attribute long n; }; };",
                "module m { interface I { attribute long n; }; };"
            ),
            Verdict::ServerFirst
        );
    }

    #[test]
    fn losing_a_base_interface_is_breaking_but_gaining_one_is_not() {
        let base = "module m { interface A { long f(); }; interface B { long g(); }; ";
        assert_eq!(
            worst(
                &format!("{base} interface D : A, B {{ long h(); }}; }};"),
                &format!("{base} interface D : A {{ long h(); }}; }};")
            ),
            Verdict::Breaking
        );
        assert_eq!(
            worst(
                &format!("{base} interface D : A {{ long h(); }}; }};"),
                &format!("{base} interface D : A, B {{ long h(); }}; }};")
            ),
            Verdict::Compatible
        );
    }

    /// One edit is one finding. The first run of this differ reported a single
    /// swapped pair of members three times — once truthfully, and twice as
    /// "operation changed the type of parameter 0", which points a reviewer at
    /// an operation nobody touched.
    #[test]
    fn editing_a_type_does_not_resurface_on_everything_that_mentions_it() {
        let c = diff(
            &reg("module m { struct S { long a; string b; }; \
                  interface I { S f(in S s, in sequence<S> many); attribute S at; }; };"),
            &reg("module m { struct S { string b; long a; }; \
                  interface I { S f(in S s, in sequence<S> many); attribute S at; }; };"),
        );
        assert_eq!(c.len(), 1, "one edit should be one finding, got {c:?}");
        assert!(c[0].id.contains("/S:"), "reported against the type, not its users");
    }

    /// The suppression must not swallow a genuine retype: the test above would
    /// pass just as well if operation signatures were never compared at all.
    #[test]
    fn a_parameter_pointed_at_a_different_type_is_still_caught() {
        let c = diff(
            &reg("module m { struct S { long a; }; struct T { long a; }; \
                  interface I { void f(in S x); }; };"),
            &reg("module m { struct S { long a; }; struct T { long a; }; \
                  interface I { void f(in T x); }; };"),
        );
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].verdict, Verdict::Breaking);
        assert!(c[0].what.contains("parameter 0"), "{}", c[0].what);
    }

    /// A report is read from the top, so the thing that stops the release has
    /// to be there.
    #[test]
    fn changes_are_ordered_worst_first() {
        let c = diff(
            &reg("module m { struct S { long a; }; interface I { long f(); }; };"),
            &reg("module m { struct S { long a; long b; }; struct New { long x; }; \
                 interface I { long f(); long g(); }; };"),
        );
        assert_eq!(c[0].verdict, Verdict::Breaking);
        assert_eq!(c.last().unwrap().verdict, Verdict::Compatible);
        assert!(c.windows(2).all(|w| w[0].verdict >= w[1].verdict));
    }
}
