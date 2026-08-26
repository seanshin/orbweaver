//! What an interface answers to, in no language in particular.
//!
//! D032 §3's first row: **the contract may not differ per language.** Which
//! names an interface answers to is a fact of the contract — §7.9.1 makes
//! `_get_balance` an operation on the wire — so it belongs to one function that
//! every target and every binding reads.
//!
//! It lived in [`crate::python`] until 2026-08-26, and was correct there for as
//! long as Python was the only second target. It stopped being correct the
//! moment a *seam* read it: `pyservant` computed a foreign servant's callable
//! surface with `python::client_operations`, so a Java servant would have
//! resolved its contract through the Python emitter. Nothing would have been
//! red — the function is language-neutral and always was — which is exactly the
//! shape `CLAUDE.md` records under *"a sentence many layers say is a fact"*:
//! the pin's scope was a module and the fact's scope is the workspace.
//!
//! `python::client_operations` is a re-export, so no caller changed.
//!
//! *인터페이스가 응답하는 이름은 계약의 사실이지 언어의 사실이 아니다. 파이썬
//! 이미터 안에 있던 동안에는 아무것도 붉지 않았고, 그것이 바로 문제였다.*

use std::collections::BTreeMap;

use orbweaver_registry::{OperationSig, Registry};

/// Every name an interface answers to, keyed by the name that travels.
///
/// Operations and attribute accessors in one map, because on the wire they are
/// one thing: §7.9.1 says `_get_balance` is an operation, and an interface that
/// answered `balance` differently depending on whether the caller went through
/// an attribute or an operation would be two contracts. Inherited members are
/// included, which is the same resolved set the Rust stub is built from.
///
/// Four consumers, none of which can derive it safely for itself: the Python
/// client emitter, `orbweaver-py-bridge` (which must route every name a stub
/// can send), the oracle (which drives every method the emitter wrote), and
/// [`crate::seam::ForeignServant`] (which must dispatch exactly the names a
/// client of the same contract can send — one function decides both, in every
/// language).
pub fn callable_operations(registry: &Registry, id: &str) -> BTreeMap<String, OperationSig> {
    let (mut ops, attrs) = crate::resolved_members(registry, id);
    for (attr, a) in &attrs {
        ops.insert(format!("_get_{attr}"), crate::getter_sig(a));
        if !a.readonly {
            ops.insert(format!("_set_{attr}"), crate::setter_sig(a));
        }
    }
    ops
}
