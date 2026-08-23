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

use orbweaver_giop::typecode::{TypeCode, UnionCase};

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
            (Some(Entry::Const { tc: a, value: va }), Some(Entry::Const { tc: b, value: vb })) => {
                if a != b {
                    out.push(Change {
                        id: id.clone(),
                        what: "constant changed type".into(),
                        why: "a constant's type is part of every expression that uses it",
                        verdict: Verdict::Breaking,
                    });
                }
                // The value became visible when the registry started folding
                // it, and it is a real compatibility question: a constant is
                // *compiled into* callers, so a peer built against the old
                // contract keeps sending the old value forever. Nothing on the
                // wire is malformed — an authorization scope constant that
                // changed spelling is a legal string the far side has no
                // meaning for, which is what this verdict says.
                if va != vb {
                    out.push(Change {
                        id: id.clone(),
                        what: format!(
                            "constant value changed from {} to {}",
                            render_const(va.as_ref()),
                            render_const(vb.as_ref())
                        ),
                        why: "a constant is compiled into its callers, so peers built against \
                              the old contract keep using the old value until they are rebuilt",
                        verdict: Verdict::ConditionallyBreaking,
                    });
                }
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

/// A constant's value as a release note reads it.
///
/// `{:?}` on a [`crate::ConstValue::Fixed`] prints its two fields —
/// `Fixed { unscaled: 991, scale: 2 }` — which is the internal shape and not
/// the number anybody wrote. A §5.3 verdict is read by a person deciding
/// whether to publish, so it says `9.91`.
fn render_const(v: Option<&crate::ConstValue>) -> String {
    match v {
        None => "nothing the registry could evaluate".to_owned(),
        Some(v) => match v.as_decimal() {
            Some(d) => d,
            None => match v {
                crate::ConstValue::Int(i) => i.to_string(),
                crate::ConstValue::Float(f) => format!("{f}"),
                crate::ConstValue::Bool(b) => b.to_string(),
                crate::ConstValue::Str(s) => format!("{s:?}"),
                crate::ConstValue::Enum { member, .. } => member.clone(),
                crate::ConstValue::Fixed { .. } => unreachable!("as_decimal answers for Fixed"),
            },
        },
    }
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
        (
            TypeCode::Union { discriminator: da, default_index: dia, cases: ca, .. },
            TypeCode::Union { discriminator: db, default_index: dib, cases: cb, .. },
        ) => diff_union(id, (da, *dia, ca), (db, *dib, cb), out),
        // A bound is not an encoding. `sequence<octet>` and `sequence<octet, 64>`
        // put the same bytes on the wire — a length and the elements — so the
        // catch-all's sentence about a receiver having no way to notice is
        // false here, and it is false in the direction that matters: a bound
        // change fails *loudly*, with a refusal, where the class that sentence
        // describes is §5.3's measured horror of a peer returning the wrong
        // member and raising nothing. Still Breaking — tightening refuses a
        // sender that was conformant yesterday, loosening refuses a receiver
        // that is conformant today — but a diagnostic that misdirects an
        // operator toward the silent class costs more than it saves.
        (a, b)
            if bounds_of(a.resolve_alias()).is_some()
                && bounds_of(a.resolve_alias()) != bounds_of(b.resolve_alias())
                && same_shape(a.resolve_alias(), b.resolve_alias()) =>
        {
            out.push(Change {
                id: id.clone(),
                what: format!(
                    "bound changed from {} to {}",
                    render_bound(bounds_of(a.resolve_alias())),
                    render_bound(bounds_of(b.resolve_alias()))
                ),
                why: "the bytes are unchanged; what changes is what a conformant peer may send \
                      or accept, so the failure is a refusal rather than a silent misread",
                verdict: Verdict::Breaking,
            });
        }
        _ => out.push(Change {
            id: id.clone(),
            what: "type definition changed".into(),
            why: "the encoded form differs, and CDR gives a receiver no way to notice",
            verdict: Verdict::Breaking,
        }),
    }
}

/// A union compared by the roles its members play on the wire, not by their
/// positions in the `TypeCode`'s member list.
///
/// A union value is a discriminator followed by the one member that
/// discriminator selects — a labelled case by its label, the default member
/// by *not* matching any label. The member list's order and length are not
/// part of that encoding: `case 2: default: string rest;` was two members in
/// this registry until f8daa21 (the default folded onto its label,
/// `default_index` on it) and is three today (omniidl's list, the default a
/// member of its own), and every discriminator selects the same branch from
/// either. Compared by position those two lists differ, and compared by label
/// the default's empty label reads as a discriminator value nobody has, so
/// the folded shape against the expanded one was "case added — conditionally
/// breaking" one way and "case removed — BREAKING" the other, and a default
/// whose type changed while a case was inserted ahead of it was missed
/// outright (measured 2026-08-19; the tests below hold both). An IFR-ingested
/// peer `TypeCode` is always the expanded shape and a frozen or hand-built one
/// need not be, which is where a gate reasoning by position would be wrong
/// about what it claims.
///
/// So: every case with a non-empty label is keyed by that label — one present
/// in one list and not the other is a branch removed or added; the default is
/// the case `default_index` names, compared with the other side's default
/// wherever that one sits, by that role alone. A default's label, when it has
/// one, is a real label of the folded shape and counts in the first set (on
/// the wire the slot's value is ignored, §9.3.5.1.4, and the decoder drops
/// it, so an ingested default never has one). A case with an empty label that
/// is not the default cannot come from the registry or the wire decoder and
/// is in neither set.
///
/// The verdicts for a default that appears or disappears follow from what a
/// peer without one does with a discriminator no label claims. It does not
/// fail: omniORB's generated stub reads the discriminator, sets its default
/// flag and reads no member (`omniidl -bcxx`, measured 2026-08-19), and its
/// `_default()` writes such a discriminator with no member after it — legal by
/// CORBA 3.4 Part 2 §9.3.2.6, "followed by the representation of any selected
/// member". So a peer with the default and a peer without it disagree on
/// whether a member follows that discriminator, and whichever is receiving
/// reads the following bytes as the wrong thing and raises nothing: §5.3's
/// silent class. Adding a default is that mechanism for every unlabelled
/// discriminator at once, which is what adding a case is for one — the same
/// verdict, conditionally breaking; removing one is a removed branch a
/// deployed sender may still select — BREAKING, like a removed case.
fn diff_union(
    id: &RepositoryId,
    (da, dia, ca): (&TypeCode, i32, &[UnionCase]),
    (db, dib, cb): (&TypeCode, i32, &[UnionCase]),
    out: &mut Vec<Change>,
) {
    if !same_identity(da, db) {
        out.push(Change {
            id: id.clone(),
            what: "discriminator changed type".into(),
            why: "the discriminator is the first thing in every value of the union and is \
                  read at the old type's width, so the branch after it is misread too",
            verdict: Verdict::Breaking,
        });
        // The labels are encoded in the discriminator's width; comparing
        // them across two widths would report every case removed and added.
        return;
    }

    fn labelled(cases: &[UnionCase]) -> impl Iterator<Item = &UnionCase> {
        cases.iter().filter(|c| !c.label.is_empty())
    }
    fn default_of(cases: &[UnionCase], index: i32) -> Option<&UnionCase> {
        usize::try_from(index).ok().and_then(|i| cases.get(i))
    }

    let removed: Vec<_> =
        labelled(ca).filter(|x| !labelled(cb).any(|y| y.label == x.label)).collect();
    let added: Vec<_> =
        labelled(cb).filter(|y| !labelled(ca).any(|x| x.label == y.label)).collect();
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
    for x in labelled(ca) {
        let Some(y) = labelled(cb).find(|y| y.label == x.label) else { continue };
        member_difference(id, "union case", x, y, out);
    }

    match (default_of(ca, dia), default_of(cb, dib)) {
        (None, None) => {}
        (Some(x), Some(y)) => member_difference(id, "default member", x, y, out),
        (Some(x), None) => out.push(Change {
            id: id.clone(),
            what: format!("default member {:?} removed", x.name),
            why: "a sender may still select a discriminator no label claims, and a receiver \
                  without the default reads no member where the sender wrote one — the \
                  member's bytes are read as whatever follows the union, and nothing is raised",
            verdict: Verdict::Breaking,
        }),
        (None, Some(y)) => out.push(Change {
            id: id.clone(),
            what: format!("default member {:?} added", y.name),
            why: "an old receiver reads no member after a discriminator no label claims, so \
                  the default's bytes are read as whatever follows the union and nothing is \
                  raised; safe only once every receiver is updated, like an added case",
            verdict: Verdict::ConditionallyBreaking,
        }),
    }
}

/// One union member against its counterpart on the other side — the same
/// labelled case, or the two defaults — by type and then by name.
fn member_difference(
    id: &RepositoryId,
    role: &str,
    x: &UnionCase,
    y: &UnionCase,
    out: &mut Vec<Change>,
) {
    if !same_identity(&x.tc, &y.tc) {
        out.push(Change {
            id: id.clone(),
            what: format!("{role} {:?} changed type", x.name),
            why: "the receiver reads the old branch's bytes",
            verdict: Verdict::Breaking,
        });
    } else if x.name != y.name {
        out.push(Change {
            id: id.clone(),
            what: format!("{role} {:?} renamed to {:?}", x.name, y.name),
            // Reported so a reviewer knows the differ looked, and compatible
            // because nothing deployed can tell: a value is selected by its
            // discriminator, `TypeCode::equivalent` ignores member names, and
            // only regenerated stubs see the new one.
            why: "a member's name does not travel — values are selected by discriminator and \
                  TypeCode equivalence ignores member names — so only regenerated stubs see it",
            verdict: Verdict::Compatible,
        });
    }
}

/// Whether two types differ only in a way a bound can explain — a sequence of
/// the same element type, or the same string kind. A `sequence<octet>` becoming
/// a `sequence<long>` is not a bound change however the bounds compare.
fn same_shape(a: &TypeCode, b: &TypeCode) -> bool {
    match (a, b) {
        (TypeCode::Sequence { element: ea, .. }, TypeCode::Sequence { element: eb, .. }) => {
            ea == eb
        }
        (TypeCode::String(_), TypeCode::String(_)) => true,
        (TypeCode::WString(_), TypeCode::WString(_)) => true,
        _ => false,
    }
}

/// The declared bound of a bounded type, or `None` for one that has no bound
/// to change. Zero means unbounded in `TypeCode`, and is rendered as such.
fn bounds_of(tc: &TypeCode) -> Option<u32> {
    match tc {
        TypeCode::Sequence { bound, .. } | TypeCode::String(bound) | TypeCode::WString(bound) => {
            Some(*bound)
        }
        _ => None,
    }
}

fn render_bound(b: Option<u32>) -> String {
    match b {
        Some(0) | None => "unbounded".to_owned(),
        Some(n) => n.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bound change is Breaking and **not** the silent class. The bytes are
    /// identical — a length and the elements — so the catch-all's sentence
    /// about a receiver having no way to notice is false here, and false in
    /// the direction that misleads: this failure is a refusal, while the class
    /// that sentence describes is §5.3's measured case of a peer returning the
    /// wrong member and raising nothing.
    #[test]
    fn a_bound_change_is_breaking_but_not_silent() {
        let out = diff(
            &reg("module m { typedef sequence<octet> Blob; };"),
            &reg("module m { typedef sequence<octet, 64> Blob; };"),
        );
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].verdict, Verdict::Breaking);
        assert!(out[0].what.contains("unbounded to 64"), "{}", out[0].what);
        assert!(out[0].why.contains("refusal"), "{}", out[0].why);
        assert!(!out[0].why.contains("no way to notice"), "{}", out[0].why);
    }

    /// Loosening is breaking too, and for the other party: a receiver bounded
    /// at 64 refuses what a newly unbounded sender may now send.
    #[test]
    fn loosening_a_bound_is_breaking_for_the_receiver() {
        let out = diff(
            &reg("module m { typedef string<32> Name; };"),
            &reg("module m { typedef string Name; };"),
        );
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].what.contains("32 to unbounded"), "{}", out[0].what);
    }

    /// And a change a bound cannot explain keeps the silent-class message,
    /// because that one is true of it.
    #[test]
    fn changing_the_element_type_is_still_the_silent_class() {
        let out = diff(
            &reg("module m { typedef sequence<octet> Blob; };"),
            &reg("module m { typedef sequence<long> Blob; };"),
        );
        assert_eq!(out.len(), 1, "{out:?}");
        assert!(out[0].why.contains("no way to notice"), "{}", out[0].why);
    }

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

    /// A union `TypeCode` built by hand, in whichever member-list convention
    /// the test needs. `None` is the default member's empty label.
    fn union_tc(default_index: i32, cases: &[(Option<i32>, &str, TypeCode)]) -> TypeCode {
        TypeCode::Union {
            id: "IDL:m/U:1.0".into(),
            name: "U".into(),
            discriminator: Box::new(TypeCode::Long),
            default_index,
            cases: cases
                .iter()
                .map(|(label, name, tc)| orbweaver_giop::typecode::UnionCase {
                    label: label.map(|v| v.to_be_bytes().to_vec()).unwrap_or_default(),
                    name: (*name).to_owned(),
                    tc: tc.clone(),
                })
                .collect(),
        }
    }

    /// A registry holding one frozen `TypeCode` with no IDL behind it — the
    /// shape an IFR-ingested peer type, or a `TypeCode` built by an older
    /// registry, arrives in.
    fn frozen(tc: TypeCode) -> Registry {
        let mut r = Registry::new();
        r.define_ingested("IDL:m/U:1.0".into(), Entry::Type(tc), "frozen").expect("registers");
        r
    }

    /// `union U switch (long) { case 1: long one; case 2: default: string
    /// rest; }` has two member lists: the one the registry built until
    /// f8daa21 — the `default:` folded onto its label, `default_index` on it —
    /// and omniidl's, one member per label with the default a member of its
    /// own. Same value encoding for every discriminator (measured, 3a061d8),
    /// so the §5.3 gate must call them the same union. It called them "case
    /// added — conditionally breaking" one way and "case removed — BREAKING"
    /// the other, because it compared the default's empty label as if it were
    /// a discriminator value.
    #[test]
    fn a_folded_and_an_expanded_default_are_the_same_union() {
        let folded = union_tc(
            1,
            &[(Some(1), "one", TypeCode::Long), (Some(2), "rest", TypeCode::String(0))],
        );
        let expanded = union_tc(
            2,
            &[
                (Some(1), "one", TypeCode::Long),
                (Some(2), "rest", TypeCode::String(0)),
                (None, "rest", TypeCode::String(0)),
            ],
        );
        let forward = diff(&frozen(folded.clone()), &frozen(expanded.clone()));
        assert!(forward.is_empty(), "folded -> expanded: {forward:#?}");
        let back = diff(&frozen(expanded), &frozen(folded.clone()));
        assert!(back.is_empty(), "expanded -> folded: {back:#?}");
        // And against the shape the registry derives from the IDL today.
        let from_idl = reg(
            "module m { union U switch (long) { case 1: long one; case 2: default: string rest; }; };",
        );
        let against_idl = diff(&frozen(folded), &from_idl);
        assert!(against_idl.is_empty(), "frozen folded -> IDL: {against_idl:#?}");
    }

    /// Removing the default is a removed branch for every discriminator no
    /// label claims: an old sender may still select one, and the new receiver
    /// reads no member where the sender wrote one.
    #[test]
    fn removing_the_default_is_breaking_and_is_reported_as_the_default() {
        let c = diff(
            &reg("module m { union U switch (long) { case 1: long a; default: string b; }; };"),
            &reg("module m { union U switch (long) { case 1: long a; }; };"),
        );
        assert_eq!(c.len(), 1, "{c:#?}");
        assert_eq!(c[0].verdict, Verdict::Breaking);
        assert!(c[0].what.contains("default member \"b\" removed"), "{}", c[0].what);
    }

    /// Adding one is the same mechanism as adding a case, for every
    /// discriminator no label claims at once: an old receiver reads no member
    /// after such a discriminator (omniORB's generated stub sets its default
    /// flag and reads nothing — measured 2026-08-19, `omniidl -bcxx`), so the
    /// new default's bytes are consumed as whatever follows the union, and
    /// nothing is raised. The verdict is the one "case added" carries and the
    /// message says which role changed.
    #[test]
    fn adding_a_default_is_conditionally_breaking_like_an_added_case() {
        let c = diff(
            &reg("module m { union U switch (long) { case 1: long a; }; };"),
            &reg("module m { union U switch (long) { case 1: long a; default: string b; }; };"),
        );
        assert_eq!(c.len(), 1, "{c:#?}");
        assert_eq!(c[0].verdict, Verdict::ConditionallyBreaking);
        assert!(c[0].what.contains("default member \"b\" added"), "{}", c[0].what);
        assert!(c[0].why.contains("no member"), "the reason must name the mechanism: {}", c[0].why);
    }

    /// The default member's type is compared by role, wherever the member sits.
    /// Positional comparison missed this once anything before it shifted:
    /// here a case is inserted ahead of it and its type changes at the same
    /// time.
    #[test]
    fn retyping_the_default_is_breaking_wherever_it_sits() {
        let c = diff(
            &reg("module m { union U switch (long) { case 1: long a; default: string b; }; };"),
            &reg(
                "module m { union U switch (long) { case 1: long a; case 2: long c; default: long b; }; };",
            ),
        );
        assert!(
            c.iter().any(|x| x.verdict == Verdict::Breaking
                && x.what.contains("default member \"b\" changed type")),
            "{c:#?}"
        );
        assert!(
            c.iter()
                .any(|x| x.verdict == Verdict::ConditionallyBreaking && x.what.contains("added")),
            "{c:#?}"
        );
        // And the same edit with the default moved to the front, where the
        // registry's expanded list puts it first.
        let c = diff(
            &reg("module m { union U switch (long) { case 1: long a; default: string b; }; };"),
            &reg("module m { union U switch (long) { default: long b; case 1: long a; }; };"),
        );
        assert_eq!(c.len(), 1, "{c:#?}");
        assert_eq!(c[0].verdict, Verdict::Breaking);
        assert!(c[0].what.contains("default member \"b\" changed type"), "{}", c[0].what);
    }

    /// A member's name does not travel: values are selected by discriminator
    /// and `TypeCode::equivalent` ignores member names, so a deployed peer is
    /// unaffected and only regenerated stubs see it. Reported, so a reviewer
    /// knows the differ looked, and compatible.
    #[test]
    fn renaming_a_union_member_is_compatible_because_names_do_not_travel() {
        let c = diff(
            &reg("module m { union U switch (long) { case 1: long a; default: string b; }; };"),
            &reg("module m { union U switch (long) { case 1: long x; default: string y; }; };"),
        );
        assert_eq!(c.len(), 2, "{c:#?}");
        assert!(c.iter().all(|x| x.verdict == Verdict::Compatible), "{c:#?}");
        assert!(c.iter().any(|x| x.what.contains("case \"a\" renamed to \"x\"")), "{c:#?}");
        assert!(
            c.iter().any(|x| x.what.contains("default member \"b\" renamed to \"y\"")),
            "{c:#?}"
        );
    }

    /// The discriminator is the first thing in every value of the union.
    #[test]
    fn changing_the_discriminator_is_breaking_and_says_so() {
        let c = diff(
            &reg("module m { union U switch (long) { case 1: long a; default: string b; }; };"),
            &reg("module m { union U switch (short) { case 1: long a; default: string b; }; };"),
        );
        assert_eq!(c.len(), 1, "{c:#?}");
        assert_eq!(c[0].verdict, Verdict::Breaking);
        assert!(c[0].what.contains("discriminator"), "{}", c[0].what);
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
            // The second parameter used to read `in sequence<S> many`, which is
            // not legal IDL: a signature takes `param_type_spec`, which has no
            // `sequence_type`, so a sequence reaches a parameter only through a
            // typedef (omniidl: "Syntax error in operation parameters";
            // `corpus/negative/n16`). It is a second plain `S` here rather than
            // `typedef sequence<S> Many;` because the typedef is a *second
            // changed declaration* — the differ reports it in its own right,
            // which is a separate question from this one and is recorded rather
            // than absorbed by loosening the count.
            &reg("module m { struct S { long a; string b; }; \
                  interface I { S f(in S s, in S other); attribute S at; }; };"),
            &reg("module m { struct S { string b; long a; }; \
                  interface I { S f(in S s, in S other); attribute S at; }; };"),
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

    /// §5.3 can tell one decimal from another, and cannot be made to cry wolf
    /// about two spellings of the same one.
    ///
    /// This comparison was blind before 2026-08-21. The lexer folded `9.9d` to
    /// an `f64` and the registry had no `ConstValue` for a decimal, so *every*
    /// `fixed` constant was `None` on both sides of every diff — a released
    /// tax rate could change and the release gate would print "no change".
    /// The pair below is the one that makes the blindness visible rather than
    /// arguable: `9.9d` against `9.91d` must break, and `9.9d` against `9.90d`
    /// must not, and a `None`-on-both-sides comparison gets the first wrong
    /// while a naive text comparison gets the second wrong.
    #[test]
    fn a_fixed_constants_value_is_compared_as_a_decimal() {
        let changed = diff(
            &reg("module m { const fixed RATE = 9.9d; };"),
            &reg("module m { const fixed RATE = 9.91d; };"),
        );
        assert_eq!(changed.len(), 1, "{changed:?}");
        assert_eq!(changed[0].verdict, Verdict::ConditionallyBreaking);
        assert!(
            changed[0].what.contains("9.9") && changed[0].what.contains("9.91"),
            "the verdict names both values as decimals: {}",
            changed[0].what
        );

        // Trailing fractional zeros are not part of the value — omniidl dumps
        // `9.90d` as `9.9d` — so this pair is not a change and reporting one
        // would block a release over a reformat.
        let same = diff(
            &reg("module m { const fixed RATE = 9.9d; };"),
            &reg("module m { const fixed RATE = 9.90d; };"),
        );
        assert!(same.is_empty(), "9.9d and 9.90d are the same constant: {same:?}");

        // And the value a binary float would have rounded it to is a change,
        // which is the whole argument for keeping the decimal.
        let rounded = diff(
            &reg("module m { const fixed RATE = 9.9d; };"),
            &reg("module m { const fixed RATE = 9.9000000000000004d; };"),
        );
        assert_eq!(rounded.len(), 1, "{rounded:?}");
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
