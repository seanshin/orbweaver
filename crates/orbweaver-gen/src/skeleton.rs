//! Server skeletons: the half of static generation that answers calls.
//!
//! The client stub in [`crate`] turns a Rust call into a GIOP request. A
//! skeleton is its mirror: it turns a GIOP request back into a Rust call. For
//! each interface it emits
//!
//! * `<I>Fault` — everything the servant can fail with: a `System` variant
//!   carrying a [`rt::SystemException`](crate::rt::SystemException), plus one
//!   variant per exception named in a `raises` clause. Its `write` puts the
//!   body §9.4.3.1 describes — repository id first, then the members — and
//!   says which reply status the bytes travel under;
//! * `<I>Refs` — the object-key scheme and the reference factory: `key_of`,
//!   `oid_of`, `reference`, `nil`. Reversible by construction, so no table of
//!   minted keys exists to be lost across a restart;
//! * `<I>Target<'a>` — *which* object a call was addressed to, plus the means
//!   to name its neighbours;
//! * `<I>Servant` — a trait with one method per operation and per attribute
//!   accessor, each taking an `<I>Target` and then **decoded Rust arguments**,
//!   returning `Result<Ret, <I>Fault>`. No `Encoder`, no `Decoder`, no
//!   operation names: a servant that never sees the wire cannot get the wire
//!   wrong. Plus `knows` (required) and `forward` / `redirect` (defaulted);
//! * `<I>Object` and `<I>Objects<S>` — the other shape: one value per object,
//!   behind a map, implemented in terms of `<I>Servant`;
//! * `<I>Skeleton<S>` — the [`rt::Dispatch`](crate::rt::Dispatch)
//!   implementation that decodes the key, decodes the arguments, calls the
//!   trait, and encodes.
//!
//! Both halves are generated from the same [`OpShape`](crate::OpShape), so the
//! arguments a caller passes and the arguments a servant receives are the same
//! list by construction rather than by review.
//!
//! # Why a skeleton has an object-key scheme
//!
//! Because a servant that answers for exactly one object is not a servant
//! anybody deploys. Every hand-written `Dispatch` in this workspace holds many
//! objects — a naming context per key, an `InterfaceDef` per repository id, a
//! factory per tenant, two singletons for two interfaces — and all four build
//! their keys the same way: a root key the server was bound with, plus a
//! derived suffix. That derivation is generated here; which keys *exist* is
//! not, and cannot be, because a contract says what an object does and never
//! which ones there are. So `knows` is a required method with no default: the
//! `true` that [`rt::Dispatch`](crate::rt::Dispatch) defaults to is exactly
//! what made a generated server claim every key in its process.
//!
//! # Why identity is an argument and not a `&mut self` per object
//!
//! Both shapes are emitted; the argument-taking one is the trait and the
//! map-shaped one is the adapter, in that order, because the five hand-written
//! servants were read before the choice was made and **none** of them keeps a
//! value per object. The IR facade derives its objects from a registry and
//! stores nothing per reference — materialising one value per entry would
//! recreate the minted-key table its own documentation rejects. The naming and
//! tenant servants resolve the key on every call because their operations reach
//! *other* objects: resolving a path walks contexts, cloning a model writes a
//! sibling. A map-shaped trait can express none of that, and can be expressed
//! *by* the other in ten lines per operation — so it is the one generated on
//! top.
//!
//! # Why the error type is not the user exceptions alone
//!
//! It was, for one batch, and the consequence was that an interface with no
//! `raises` clause got an **uninhabited** error type: a servant for it could
//! not fail. That is wrong for every real servant. The hand-written servants
//! in this workspace — naming, event, IFR, expert, tenant — answer unknown
//! keys with `OBJECT_NOT_EXIST`, refusals with `NO_PERMISSION`, bad arguments
//! with `BAD_PARAM` and a minor code, and "not right now" with `TRANSIENT`,
//! and *none* of that vocabulary is declarable in IDL. A generated skeleton
//! that cannot express it is one that can never replace a hand-written one.
//!
//! The completion status a system exception carries is the servant's decision
//! and not the generator's: see [`rt::Raising`](crate::rt::Raising) for why
//! there is no default for it.
//!
//! # The three things a skeleton generator gets wrong
//!
//! **oneway.** §9.4.1: a request with `response_expected` false gets no reply
//! *at all*. An empty `NO_EXCEPTION` reply is not "nearly nothing" — it is a
//! whole extra message, and the peer, which is not waiting for one, reads it as
//! the header of the next reply. Every later request on that connection is then
//! answered with the wrong bytes. A oneway arm here therefore writes nothing
//! into the reply encoder, and drops a fault it has no way to carry — through
//! [`rt::oneway_fault_dropped`](crate::rt::oneway_fault_dropped), so the drop
//! is a logged decision rather than a `let _ =` nobody can debug.
//!
//! **Attributes.** `readonly attribute T x` is *one* operation, `_get_x`. A
//! generator that emits accessors from a single template gives it a `_set_x`
//! too, and a contract that says read-only starts accepting writes. The
//! readonly case emits no setter arm, so `_set_x` falls to `BAD_OPERATION`
//! along with every other name the interface does not have.
//!
//! **Alignment origin.** CDR alignment is measured from the start of the GIOP
//! message, not from the start of the body. Both directions are affected:
//! arguments are decoded through [`Request::body`](crate::rt::Request::body),
//! whose decoder is positioned inside the whole message, and the reply is
//! written into the encoder [`Server`](crate::rt::Server) hands over, which was
//! created with `continuing_at` the offset the body will occupy. Copying either
//! into a fresh buffer restarts alignment at zero. This is not hypothetical for
//! requests: GIOP 1.0 and 1.1 do not align the request body at all, so its
//! offset depends on the object key and operation name lengths, and a `double`
//! first argument lands at a different padding under every key.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use orbweaver_registry::{OperationSig, Registry};

use crate::{
    Cx, doc, getter_sig, ident, op_doc, op_shape, resolved_members, rust_path, setter_sig,
};

/// One exception an interface can raise, as the generated enum sees it.
struct Raise {
    /// Repository id, which travels first in the exception body.
    id: String,
    /// Rust variant name.
    variant: String,
    /// Rust path of the generated exception struct.
    ty: String,
}

/// Every exception reachable through a `raises` clause of this interface or a
/// base of it, de-duplicated and given non-clashing variant names.
fn raises_of(registry: &Registry, ops: &BTreeMap<String, OperationSig>, cx: &Cx<'_>) -> Vec<Raise> {
    let mut ids: Vec<String> = Vec::new();
    for sig in ops.values() {
        for ex in &sig.raises {
            if !ids.contains(ex) {
                ids.push(ex.clone());
            }
        }
    }
    ids.sort();
    // Two exceptions in different modules can share a last segment, which would
    // give the enum two variants of the same name. Only the colliding ones pay
    // for it, so the common case still reads as the IDL does.
    let mut short: BTreeMap<String, usize> = BTreeMap::new();
    for id in &ids {
        let last = cx.path_of(id).last().cloned().unwrap_or_default();
        *short.entry(last).or_insert(0) += 1;
    }
    // `System` is not the contract's — the fault type mints it, for the
    // vocabulary IDL cannot declare — and it lands in the same enum. An
    // exception whose last segment is `System` declared the variant twice and
    // the crate did not compile (`E0428`, measured 2026-08-25). The *derived*
    // variant is the one that moves, exactly as it already does when two
    // exceptions share a last segment: only the colliding one pays, and
    // `<I>Fault::System` is the variant every servant in this workspace
    // matches on and the documentation above promises is always there.
    let mut taken: Vec<String> = vec!["System".to_owned()];
    let mut out = Vec::new();
    for id in &ids {
        if registry.get(id).is_none() {
            continue;
        }
        let path = cx.path_of(id);
        let last = path.last().cloned().unwrap_or_default();
        let variant =
            if short.get(&last).copied().unwrap_or(0) > 1 { path.join("_") } else { last };
        let mut variant = ident(&variant);
        while taken.contains(&variant) {
            variant.push('_');
        }
        taken.push(variant.clone());
        out.push(Raise { id: id.clone(), variant, ty: rust_path(id, cx) });
    }
    out
}

/// Generates the servant trait, its fault type and the dispatcher.
pub(crate) fn emit_skeleton(registry: &Registry, id: &str, cx: &Cx<'_>) -> Result<String, String> {
    if registry.interface(id).is_none() {
        return Err("not an interface".to_owned());
    }
    let name = ident(&cx.path_of(id).last().cloned().unwrap_or_default());
    let (ops, attrs) = resolved_members(registry, id);
    let raises = raises_of(registry, &ops, cx);

    let mut s = String::new();
    emit_fault(&mut s, &name, id, &raises);
    emit_refs(&mut s, &name, id);
    emit_target(&mut s, &name, id);
    emit_trait(&mut s, registry, &name, id, &ops, &attrs, cx)?;
    emit_object_trait(&mut s, &name, id, &ops, &attrs, cx)?;
    emit_object_map(&mut s, &name, id, &ops, &attrs, cx)?;
    emit_dispatch(&mut s, registry, &name, id, &ops, &attrs, cx)?;
    Ok(s)
}

/// The servant's error type: a system exception, or one of the declared user
/// exceptions.
fn emit_fault(s: &mut String, name: &str, id: &str, raises: &[Raise]) {
    let ty = format!("{name}Fault");
    doc(s, &format!("Everything a servant for `{id}` can fail with."));
    doc(s, "");
    doc(s, "`System` is always here, whatever the contract declares: an unknown");
    doc(s, "key is `OBJECT_NOT_EXIST`, a refused call is `NO_PERMISSION`, a");
    doc(s, "temporary refusal is `TRANSIENT`, and IDL has no way to declare any");
    doc(s, "of them. Build one with `orbweaver_gen::rt::raise::*`, which does not produce a");
    doc(s, "`SystemException` until the completion status is stated — the field");
    doc(s, "that tells the caller whether a retry is safe, and the one thing a");
    doc(s, "generator cannot know for a servant.");
    doc(s, "");
    if raises.is_empty() {
        doc(s, "This interface names no `raises` clause, so `System` is the only");
        doc(s, "variant. It is still an inhabited type: a servant that cannot fail");
        doc(s, "is not a servant anybody can write.");
    } else {
        doc(s, "The remaining variants are the exceptions named in a `raises`");
        doc(s, "clause of this interface or of one it inherits. `write` puts the");
        doc(s, "body §9.4.3.1 describes — repository id first, then the members —");
        doc(s, "which is exactly what the client side reads back out of");
        doc(s, "`orbweaver_gen::rt::GiopError::UserException`.");
    }
    let _ = writeln!(s, "#[derive(Debug, Clone)]");
    let _ = writeln!(s, "pub enum {ty} {{");
    let _ = writeln!(s, "    /// A CORBA system exception, with the completion status the");
    let _ = writeln!(s, "    /// servant chose. Travels as a `SystemException` reply.");
    let _ = writeln!(s, "    System(__rt::SystemException),");
    for r in raises {
        let _ = writeln!(s, "    /// IDL exception `{}`.", r.id);
        let _ = writeln!(s, "    {}({}),", r.variant, r.ty);
    }
    let _ = writeln!(s, "}}");

    // `?` and `.into()` on a raise, which is how a servant body reads when the
    // failure comes from a helper rather than from the operation itself.
    let _ = writeln!(s, "impl ::std::convert::From<__rt::SystemException> for {ty} {{");
    let _ = writeln!(s, "    fn from(__ex: __rt::SystemException) -> Self {{");
    let _ = writeln!(s, "        Self::System(__ex)");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "}}");

    // Hand-written rather than derived: `rt::SystemException` has no
    // `PartialEq`, and a servant's own tests want to compare faults. Comparing
    // the three fields that travel is the whole of what a system exception is.
    let _ = writeln!(s, "impl ::std::cmp::PartialEq for {ty} {{");
    let _ = writeln!(s, "    fn eq(&self, __other: &Self) -> bool {{");
    let _ = writeln!(s, "        match (self, __other) {{");
    let _ = writeln!(s, "            (Self::System(__a), Self::System(__b)) => {{");
    let _ = writeln!(s, "                __a.id == __b.id");
    let _ = writeln!(s, "                    && __a.minor == __b.minor");
    let _ = writeln!(s, "                    && __a.completed == __b.completed");
    let _ = writeln!(s, "            }}");
    for r in raises {
        let _ = writeln!(
            s,
            "            (Self::{v}(__a), Self::{v}(__b)) => __a == __b,",
            v = r.variant
        );
    }
    if !raises.is_empty() {
        let _ = writeln!(s, "            _ => false,");
    }
    let _ = writeln!(s, "        }}");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "}}");

    let _ = writeln!(s, "impl {ty} {{");
    let _ = writeln!(s, "    /// The repository id of the user exception this fault carries, or");
    let _ = writeln!(s, "    /// `None` for a system exception, which carries its own.");
    let _ = writeln!(s, "    pub fn user_id(&self) -> ::std::option::Option<&'static str> {{");
    let _ = writeln!(s, "        match self {{");
    let _ = writeln!(s, "            Self::System(_) => None,");
    for r in raises {
        let _ = writeln!(s, "            Self::{}(_) => Some(\"{}\"),", r.variant, r.id);
    }
    let _ = writeln!(s, "        }}");
    let _ = writeln!(s, "    }}");
    for line in [
        "Writes this fault into the reply body and says which reply status the",
        "bytes travel under.",
        "",
        "A system exception writes nothing and comes back as `Err`: it is not a",
        "body under a status, it *replaces* the reply (§9.4.3.1), so the",
        "dispatcher hands it to the server to encode instead. That is also why",
        "nothing may be written before the fault is known — the whole buffer",
        "travels under one status.",
    ] {
        if line.is_empty() {
            let _ = writeln!(s, "    ///");
        } else {
            let _ = writeln!(s, "    /// {line}");
        }
    }
    let _ = writeln!(
        s,
        "    pub fn write(&self, __out: &mut __rt::Encoder) \
         -> ::std::result::Result<__rt::DispatchBody, __rt::SystemException> {{"
    );
    if raises.is_empty() {
        let _ = writeln!(s, "        let _ = __out;");
    }
    let _ = writeln!(s, "        match self {{");
    let _ = writeln!(s, "            Self::System(__ex) => Err(__ex.clone()),");
    for r in raises {
        let _ = writeln!(s, "            Self::{}(__v) => {{", r.variant);
        let _ = writeln!(s, "                __out.put_str(\"{}\");", r.id);
        let _ = writeln!(
            s,
            "                __v.put(__out).map_err(|_| __rt::SystemException::marshal())?;"
        );
        let _ = writeln!(s, "                Ok(__rt::DispatchBody::UserException)");
        let _ = writeln!(s, "            }}");
    }
    let _ = writeln!(s, "        }}");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "}}");
}

/// The object-key scheme and reference factory for one interface.
///
/// This is the half of "many objects, one process" the generator *can* decide:
/// how a key is built and taken apart. Which keys exist is the servant's, and
/// it is asked through `knows`.
fn emit_refs(s: &mut String, name: &str, id: &str) {
    let ty = format!("{name}Refs");
    doc(s, &format!("The object-key scheme for `{id}`, and its reference factory."));
    doc(s, "");
    doc(s, "# The keys");
    doc(s, "");
    doc(s, "```text");
    doc(s, "root                          the default object — oid \"\"");
    doc(s, &format!("root ++ \"/{name}/\" ++ <oid>    the object named <oid>"));
    doc(s, "```");
    doc(s, "");
    doc(s, "`root` is the key the `orbweaver_gen::rt::Server` was bound with. The derivation is");
    doc(s, "reversible, so no table of minted keys exists: a reference survives a");
    doc(s, "restart, and a client cannot make the process grow by looking things");
    doc(s, "up. The oid is the *whole* remainder after the prefix, so it may");
    doc(s, "contain `/`, `:`, or the infix itself — there is nothing to escape.");
    doc(s, "");
    doc(s, "The infix is the interface name, so two generated skeletons sharing a");
    doc(s, "root cannot read each other's keys. Use `with_infix` when they are");
    doc(s, "*meant* to share one key space — an `InterfaceDef` and a `Contained`");
    doc(s, "over the same repository entry are two interfaces over one object.");
    doc(s, "");
    doc(s, "# Minting");
    doc(s, "");
    doc(s, "`reference` and `ior` answer with a real IIOP profile, built by");
    doc(s, "`orbweaver_gen::rt::ObjectHome` — the servant never assembles one, which is why the");
    doc(s, "GIOP version and profile shape are still decided in exactly one");
    doc(s, "place. `nil` is the nil reference (§9.3.6), the truthful answer when");
    doc(s, "there is no such object.");
    let _ = writeln!(s, "#[derive(Debug, Clone, PartialEq, Eq)]");
    let _ = writeln!(s, "pub struct {ty} {{");
    let _ = writeln!(s, "    home: __rt::ObjectHome,");
    let _ = writeln!(s, "    infix: ::std::string::String,");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s, "impl {ty} {{");
    let _ = writeln!(s, "    /// The repository id minted references advertise.");
    let _ = writeln!(s, "    pub const TYPE_ID: &'static str = \"{id}\";");
    let _ = writeln!(s, "    /// What separates the root key from the oid.");
    // Spelled by `seam::key_infix` and not by a format string here, because a
    // foreign servant derives the same infix at run time and the two must
    // address the same objects by construction rather than by agreement.
    let _ = writeln!(
        s,
        "    pub const KEY_INFIX: &'static str = \"{}\";",
        crate::seam::key_infix(name)
    );
    let _ = writeln!(s, "    /// The scheme rooted at `home`, with the generated infix.");
    let _ = writeln!(s, "    pub fn new(__home: __rt::ObjectHome) -> Self {{");
    let _ = writeln!(s, "        Self {{ home: __home, infix: Self::KEY_INFIX.to_owned() }}");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "    /// The same, with a key space this interface shares with another.");
    let _ = writeln!(s, "    ///");
    let _ = writeln!(s, "    /// Two skeletons given the same infix must disagree about `knows`");
    let _ = writeln!(
        s,
        "    /// for every key, or `orbweaver_gen::rt::Servants` will hand a request to"
    );
    let _ = writeln!(s, "    /// whichever was added first. Sharing a key space is a claim that");
    let _ = writeln!(s, "    /// the two answer for disjoint sets of objects.");
    let _ = writeln!(
        s,
        "    pub fn with_infix(__home: __rt::ObjectHome, \
         __infix: impl ::std::convert::Into<::std::string::String>) -> Self {{"
    );
    let _ = writeln!(s, "        Self {{ home: __home, infix: __infix.into() }}");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "    /// Where these objects are published.");
    let _ = writeln!(s, "    pub fn home(&self) -> &__rt::ObjectHome {{ &self.home }}");
    let _ = writeln!(s, "    /// The infix in use, generated or overridden.");
    let _ = writeln!(s, "    pub fn infix(&self) -> &str {{ &self.infix }}");
    let _ =
        writeln!(s, "    /// The default object's key, which is also every other key's prefix.");
    let _ = writeln!(s, "    pub fn root_key(&self) -> &[u8] {{ self.home.root_key() }}");
    let _ = writeln!(s, "    /// The object key for `oid`; the empty oid is the root key.");
    let _ = writeln!(s, "    pub fn key_of(&self, __oid: &str) -> ::std::vec::Vec<u8> {{");
    let _ = writeln!(s, "        self.home.key_of(&self.infix, __oid)");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "    /// The oid a key addresses, or `None` if it is not one of ours.");
    let _ = writeln!(
        s,
        "    pub fn oid_of<'a>(&self, __key: &'a [u8]) \
         -> ::std::option::Option<&'a str> {{"
    );
    let _ = writeln!(s, "        self.home.oid_of(&self.infix, __key)");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "    /// A reference to `oid`, in the form an operation returns.");
    let _ = writeln!(s, "    pub fn reference(&self, __oid: &str) -> __rt::ObjRef {{");
    let _ = writeln!(s, "        self.home.reference(Self::TYPE_ID, self.key_of(__oid))");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "    /// The same reference, in the form `LOCATION_FORWARD` carries.");
    let _ = writeln!(s, "    pub fn ior(&self, __oid: &str) -> __rt::Ior {{");
    let _ = writeln!(s, "        self.home.ior(Self::TYPE_ID, self.key_of(__oid))");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "    /// The nil reference: there is no such object.");
    let _ = writeln!(s, "    pub fn nil() -> __rt::ObjRef {{ __rt::ObjectHome::nil() }}");
    let _ = writeln!(s, "}}");
}

/// The identity of the object a call was addressed to.
fn emit_target(s: &mut String, name: &str, id: &str) {
    let ty = format!("{name}Target");
    let refs = format!("{name}Refs");
    doc(s, &format!("Which `{id}` a call is addressed to."));
    doc(s, "");
    doc(s, "Every method of the servant trait takes one of these. It carries two");
    doc(s, "things a servant cannot get any other way: **who it is** (`oid`), and");
    doc(s, "**how to name its neighbours** (`sibling`) — so an operation that");
    doc(s, "answers with a reference to another object of this interface can do");
    doc(s, "it in one call rather than by assembling an IIOP profile.");
    doc(s, "");
    doc(s, "The oid borrows the request's own key bytes, so learning who you are");
    doc(s, "allocates nothing.");
    let _ = writeln!(s, "#[derive(Debug, Clone, Copy)]");
    let _ = writeln!(s, "pub struct {ty}<'a> {{");
    let _ = writeln!(s, "    oid: &'a str,");
    let _ = writeln!(s, "    refs: &'a {refs},");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s, "impl<'a> {ty}<'a> {{");
    let _ = writeln!(s, "    /// The identity `oid` under `refs`.");
    let _ = writeln!(s, "    pub fn new(__oid: &'a str, __refs: &'a {refs}) -> Self {{");
    let _ = writeln!(s, "        Self {{ oid: __oid, refs: __refs }}");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "    /// Which object this is. The empty oid is the default object,");
    let _ = writeln!(s, "    /// addressed by the bare root key the server was bound with.");
    let _ = writeln!(s, "    pub fn oid(&self) -> &'a str {{ self.oid }}");
    let _ = writeln!(s, "    /// Whether this is the default object.");
    let _ = writeln!(s, "    pub fn is_default(&self) -> bool {{ self.oid.is_empty() }}");
    let _ = writeln!(s, "    /// The key scheme, for minting references into other key spaces.");
    let _ = writeln!(s, "    pub fn refs(&self) -> &'a {refs} {{ self.refs }}");
    let _ = writeln!(s, "    /// The raw object key this call arrived on.");
    let _ = writeln!(
        s,
        "    pub fn key(&self) -> ::std::vec::Vec<u8> {{ self.refs.key_of(self.oid) }}"
    );
    let _ =
        writeln!(s, "    /// A reference to *this* object, for handing back one's own address.");
    let _ = writeln!(
        s,
        "    pub fn reference(&self) -> __rt::ObjRef {{ self.refs.reference(self.oid) }}"
    );
    let _ = writeln!(s, "    /// A reference to another object of this interface.");
    let _ = writeln!(
        s,
        "    pub fn sibling(&self, __oid: &str) -> __rt::ObjRef {{ self.refs.reference(__oid) }}"
    );
    let _ = writeln!(s, "}}");
}

/// Every method the trait carries: (wire name, Rust name, signature).
fn methods(
    ops: &BTreeMap<String, OperationSig>,
    attrs: &BTreeMap<String, orbweaver_registry::AttributeSig>,
) -> Vec<(String, String, OperationSig)> {
    let mut out: Vec<(String, String, OperationSig)> =
        ops.iter().map(|(op, sig)| (op.clone(), op.clone(), sig.clone())).collect();
    for (attr, a) in attrs {
        out.push((format!("_get_{attr}"), attr.clone(), getter_sig(a)));
        if !a.readonly {
            // A readonly attribute gets no setter — not a setter that fails.
            out.push((format!("_set_{attr}"), format!("set_{attr}"), setter_sig(a)));
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn emit_trait(
    s: &mut String,
    registry: &Registry,
    name: &str,
    id: &str,
    ops: &BTreeMap<String, OperationSig>,
    attrs: &BTreeMap<String, orbweaver_registry::AttributeSig>,
    cx: &Cx<'_>,
) -> Result<(), String> {
    let exc = format!("{name}Fault");
    if let Some(desc) = registry.annotations(id).and_then(|a| a.get("ai_desc")) {
        doc(s, desc);
        doc(s, "");
    }
    doc(s, &format!("What a servant for `{id}` must implement."));
    doc(s, "");
    doc(s, "One method per operation and per attribute accessor, taking decoded");
    doc(s, "Rust arguments. Nothing here mentions GIOP: the wire is entirely the");
    doc(s, &format!("business of `{name}Skeleton`, which adapts this trait to"));
    doc(s, "`orbweaver_gen::rt::Dispatch`.");
    doc(s, "");
    doc(s, &format!("Every method may fail with a `{exc}`: a declared user exception,"));
    doc(s, "or a system exception built with `orbweaver_gen::rt::raise::*` — the vocabulary");
    doc(s, "(`OBJECT_NOT_EXIST`, `NO_PERMISSION`, `BAD_PARAM`, `TRANSIENT`) that");
    doc(s, "no contract declares and every servant needs.");
    doc(s, "");
    doc(s, "# Why every method carries an identity");
    doc(s, "");
    doc(s, &format!("The first argument is a `{name}Target`: *which* object of this"));
    doc(s, "interface the call was addressed to. One servant therefore answers for");
    doc(s, "many objects, which is what a process needs to hold a naming context");
    doc(s, "per key, an `InterfaceDef` per repository id, or a factory per tenant.");
    doc(s, "");
    doc(s, "The alternative — a map from key to servant, so each object gets its");
    doc(s, "own `&mut self` and no argument — is generated too, as");
    doc(s, &format!("`{name}Objects`, in terms of this trait. It is the *adapter* and not"));
    doc(s, "the trait because the five hand-written servants in this workspace are");
    doc(s, "measured, and none of them keeps one value per object: the IR facade");
    doc(s, "derives its objects from the registry and stores nothing per");
    doc(s, "reference, and the naming and tenant servants resolve the key on every");
    doc(s, "call because their operations reach *other* objects — resolving a path,");
    doc(s, "cloning one model into another. A map-shaped trait can express none of");
    doc(s, "those; this one expresses the map in ten lines per operation.");
    let _ = writeln!(s, "pub trait {name}Servant {{");
    // `knows` is required, and that is the whole point of this batch. A default
    // `true` is what `rt::Dispatch` has, and it is what made every generated
    // server single-object: it silently claims every key in the process.
    for line in [
        "Whether this servant answers for the object `__at` names.",
        "",
        "**Required, with no default.** `orbweaver_gen::rt::Dispatch::knows` defaults to",
        "accepting everything, which is right for a process with one object and",
        "is exactly what stopped a generated skeleton from ever holding more",
        "than one: an unknown key was served as if it were the only object.",
        "There is no answer the generator can give here — the contract says",
        "what an object *does*, never which ones exist — so the question is",
        "asked rather than guessed.",
        "",
        "A single-object servant writes `__at.is_default()`. A `false` here",
        "becomes `OBJECT_NOT_EXIST` for a request and `UNKNOWN_OBJECT` for a",
        "`LocateRequest`, and is also how `orbweaver_gen::rt::Servants` picks between",
        "skeletons sharing one key space.",
    ] {
        if line.is_empty() {
            let _ = writeln!(s, "    ///");
        } else {
            let _ = writeln!(s, "    /// {line}");
        }
    }
    let _ = writeln!(s, "    fn knows(&self, __at: &{name}Target<'_>) -> bool;");
    for line in [
        "Where this request should be sent instead, if anywhere.",
        "",
        "Returning `Some` produces a `LOCATION_FORWARD` (§9.4.3.2), which the",
        "client retries transparently against the new reference.",
        "",
        "# Why the generator defaults this and cannot fill it in",
        "",
        "Everything mechanical about a forward *is* generated: the object key is",
        "decoded, the identity is handed over, and a returned reference becomes",
        "a `LOCATION_FORWARD` reply instead of an invocation. What cannot be",
        "generated is the policy — *when* to redirect and *where to*. IDL",
        "describes operations, types and inheritance; it has no vocabulary for",
        "object location, migration, replication or load, so there is no clause",
        "in any contract from which a forward target could be derived. A",
        "generator that invented one would be inventing deployment.",
        "",
        "So the default is `None` — served here — and it is a default rather",
        "than a silence: this method exists, is documented, and is the one place",
        "to override. Note the one limit the runtime imposes and this cannot",
        "lift: a `oneway` is never forwarded, because there is no reply to carry",
        "the address. This hook is the *temporary* forward; an object that has",
        "moved for good says so through `redirect`, below.",
    ] {
        if line.is_empty() {
            let _ = writeln!(s, "    ///");
        } else {
            let _ = writeln!(s, "    /// {line}");
        }
    }
    let _ = writeln!(
        s,
        "    fn forward(&mut self, __at: &{name}Target<'_>) -> ::std::option::Option<__rt::Ior> {{"
    );
    let _ = writeln!(s, "        let _ = __at;");
    let _ = writeln!(s, "        None");
    let _ = writeln!(s, "    }}");
    for line in [
        "Where this request should be sent instead, and whether for good.",
        "",
        "The method the runtime actually asks. `orbweaver_gen::rt::Forward::Temporary` is a",
        "`LOCATION_FORWARD`; `orbweaver_gen::rt::Forward::Permanent` is a",
        "`LOCATION_FORWARD_PERM` (§9.4.3.2, GIOP 1.2 — a 1.0/1.1 peer is told",
        "the temporary form, its reply-status enumeration having no other),",
        "and is the one thing a servant can say that lets a client replace the",
        "reference it holds rather than keep the old one as a fallback.",
        "",
        "Defaults to `forward` wrapped as temporary, so a servant that",
        "overrides only `forward` behaves as it always did. Override this one",
        "to say *permanent*; there is no reason to override both.",
    ] {
        if line.is_empty() {
            let _ = writeln!(s, "    ///");
        } else {
            let _ = writeln!(s, "    /// {line}");
        }
    }
    let _ = writeln!(
        s,
        "    fn redirect(&mut self, __at: &{name}Target<'_>) -> ::std::option::Option<__rt::Forward> {{"
    );
    let _ = writeln!(s, "        self.forward(__at).map(__rt::Forward::Temporary)");
    let _ = writeln!(s, "    }}");
    for (wire, rust, sig) in methods(ops, attrs) {
        let shape = op_shape(&sig, cx)?;
        let mut docs = String::new();
        op_doc(&mut docs, &wire, &sig.annotations);
        if sig.oneway {
            doc(&mut docs, "");
            doc(&mut docs, "`oneway`: the caller is not waiting, so neither a result nor a fault");
            doc(&mut docs, "can reach it. Returning `Err` here is dropped — deliberately, and");
            doc(&mut docs, "logged, because a oneway that fails invisibly is undebuggable.");
        }
        for line in docs.lines() {
            let _ = writeln!(s, "    {line}");
        }
        let _ = writeln!(
            s,
            "    fn {}(&mut self, __at: &{name}Target<'_>{}) -> ::std::result::Result<{}, {exc}>;",
            ident(&rust),
            shape.args,
            shape.ret_ty
        );
    }
    let _ = writeln!(s, "}}");
    Ok(())
}

/// The per-object half: one value per object, addressed through a map.
fn emit_object_trait(
    s: &mut String,
    name: &str,
    id: &str,
    ops: &BTreeMap<String, OperationSig>,
    attrs: &BTreeMap<String, orbweaver_registry::AttributeSig>,
    cx: &Cx<'_>,
) -> Result<(), String> {
    let exc = format!("{name}Fault");
    doc(s, &format!("One object of `{id}`, for a servant that keeps a value per object."));
    doc(s, "");
    doc(s, "The same operations as the servant trait with the identity removed:");
    doc(s, "`&mut self` *is* the object. This is the shape most people reach for");
    doc(s, "first, and it is the right one whenever objects are created, held and");
    doc(s, "destroyed rather than derived — an active session, a subscription, a");
    doc(s, "connection.");
    doc(s, "");
    doc(s, &format!("Put a collection of these behind `{name}Objects` to get a"));
    doc(s, &format!("`{name}Servant`. What it cannot express is an operation that reaches"));
    doc(s, "another object — resolving a path, cloning one model into another — or");
    doc(s, "a population too large or too derived to materialise; for those,");
    doc(s, "implement the servant trait directly.");
    let _ = writeln!(s, "pub trait {name}Object {{");
    for (wire, rust, sig) in methods(ops, attrs) {
        let shape = op_shape(&sig, cx)?;
        let mut docs = String::new();
        op_doc(&mut docs, &wire, &sig.annotations);
        for line in docs.lines() {
            let _ = writeln!(s, "    {line}");
        }
        let _ = writeln!(
            s,
            "    fn {}(&mut self{}) -> ::std::result::Result<{}, {exc}>;",
            ident(&rust),
            shape.args,
            shape.ret_ty
        );
    }
    let _ = writeln!(s, "}}");
    Ok(())
}

/// The adapter: a map of per-object values, serving the identity-taking trait.
fn emit_object_map(
    s: &mut String,
    name: &str,
    id: &str,
    ops: &BTreeMap<String, OperationSig>,
    attrs: &BTreeMap<String, orbweaver_registry::AttributeSig>,
    cx: &Cx<'_>,
) -> Result<(), String> {
    let exc = format!("{name}Fault");
    let obj = format!("{name}Object");
    let map = format!("{name}Objects");
    doc(s, &format!("A map from oid to one `{obj}`, serving `{id}`."));
    doc(s, "");
    doc(s, &format!("The bridge between the two shapes: it implements `{name}Servant` by"));
    doc(s, "looking the identity up and calling the object it found. An oid with");
    doc(s, "no object is `OBJECT_NOT_EXIST` with `COMPLETED_NO` — nothing ran, so");
    doc(s, "a client may re-send elsewhere — and `knows` answers from the same map,");
    doc(s, "so a `LocateRequest` and an invocation cannot disagree.");
    doc(s, "");
    doc(s, "The default object is the entry under the empty oid, which is what the");
    doc(s, "bare root key addresses. A one-object process inserts exactly that.");
    let _ = writeln!(s, "#[derive(Debug, Clone)]");
    let _ = writeln!(s, "pub struct {map}<S: {obj}> {{");
    let _ = writeln!(s, "    objects: ::std::collections::BTreeMap<::std::string::String, S>,");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s, "impl<S: {obj}> ::std::default::Default for {map}<S> {{");
    let _ = writeln!(s, "    fn default() -> Self {{");
    let _ = writeln!(s, "        Self {{ objects: ::std::collections::BTreeMap::new() }}");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s, "impl<S: {obj}> {map}<S> {{");
    let _ = writeln!(s, "    /// An empty map, which knows no object at all.");
    // Fully qualified, and it has to be: `_default()` is a legal IDL operation
    // name — `default` is reserved, and the specification's own escape is the
    // leading underscore, which the mapping then drops — so `{obj}` may itself
    // carry a `default` method. `Self::default()` is then ambiguous and the
    // generated crate does not compile (E0034). Measured on a probe interface;
    // same class as the reserved-word rule, which is why the fix is in the
    // template and not in the contract.
    let _ =
        writeln!(s, "    pub fn new() -> Self {{ <Self as ::std::default::Default>::default() }}");
    let _ = writeln!(s, "    /// Adds or replaces the object under `oid`, answering what it");
    let _ = writeln!(s, "    /// displaced. The empty oid is the default object.");
    let _ = writeln!(
        s,
        "    pub fn insert(&mut self, __oid: impl ::std::convert::Into<::std::string::String>, \
         __object: S) -> ::std::option::Option<S> {{"
    );
    let _ = writeln!(s, "        self.objects.insert(__oid.into(), __object)");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "    /// Removes an object, after which its key is `OBJECT_NOT_EXIST`.");
    let _ = writeln!(s, "    pub fn remove(&mut self, __oid: &str) -> ::std::option::Option<S> {{");
    let _ = writeln!(s, "        self.objects.remove(__oid)");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "    /// The object under `oid`, if there is one.");
    let _ = writeln!(
        s,
        "    pub fn get(&self, __oid: &str) -> ::std::option::Option<&S> \
             {{ self.objects.get(__oid) }}"
    );
    let _ = writeln!(s, "    /// The object under `oid`, mutably.");
    let _ = writeln!(
        s,
        "    pub fn get_mut(&mut self, __oid: &str) -> ::std::option::Option<&mut S> {{"
    );
    let _ = writeln!(s, "        self.objects.get_mut(__oid)");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "    /// Every oid held, in sorted order.");
    let _ = writeln!(s, "    pub fn oids(&self) -> impl ::std::iter::Iterator<Item = &str> {{");
    let _ = writeln!(s, "        self.objects.keys().map(::std::string::String::as_str)");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "    /// How many objects are held.");
    let _ = writeln!(s, "    pub fn len(&self) -> usize {{ self.objects.len() }}");
    let _ = writeln!(s, "    /// Whether no object is held.");
    let _ = writeln!(s, "    pub fn is_empty(&self) -> bool {{ self.objects.is_empty() }}");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s, "impl<S: {obj}> {name}Servant for {map}<S> {{");
    let _ = writeln!(s, "    fn knows(&self, __at: &{name}Target<'_>) -> bool {{");
    let _ = writeln!(s, "        self.objects.contains_key(__at.oid())");
    let _ = writeln!(s, "    }}");
    for (_wire, rust, sig) in methods(ops, attrs) {
        let shape = op_shape(&sig, cx)?;
        let args = shape.ins.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>().join(", ");
        let _ = writeln!(
            s,
            "    fn {}(&mut self, __at: &{name}Target<'_>{}) -> ::std::result::Result<{}, {exc}> {{",
            ident(&rust),
            shape.args,
            shape.ret_ty
        );
        let _ = writeln!(s, "        match self.objects.get_mut(__at.oid()) {{");
        let _ = writeln!(s, "            Some(__o) => __o.{}({args}),", ident(&rust));
        let _ = writeln!(
            s,
            "            None => Err({exc}::System(__rt::raise::object_not_exist().did_not_run())),"
        );
        let _ = writeln!(s, "        }}");
        let _ = writeln!(s, "    }}");
    }
    let _ = writeln!(s, "}}");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_dispatch(
    s: &mut String,
    registry: &Registry,
    name: &str,
    id: &str,
    ops: &BTreeMap<String, OperationSig>,
    attrs: &BTreeMap<String, orbweaver_registry::AttributeSig>,
    cx: &Cx<'_>,
) -> Result<(), String> {
    let servant = format!("{name}Servant");
    let skel = format!("{name}Skeleton");

    doc(s, &format!("Serves `{id}`: decodes the request, calls the servant, encodes the reply."));
    doc(s, "");
    doc(s, "Hand it to `orbweaver_gen::rt::Server::serve` in place of a hand-written");
    doc(s, "`orbweaver_gen::rt::Dispatch`. What it answers, beyond the contract's own");
    doc(s, "operations:");
    doc(s, "");
    doc(s, "* `_is_a`, from the inheritance chain the registry resolved, because");
    doc(s, "  an ORB probes with it before it will narrow;");
    doc(s, "* `_non_existent`, with `false`;");
    doc(s, "* every other name with `BAD_OPERATION`, including the `_set_` of a");
    doc(s, "  readonly attribute;");
    doc(s, "* a body that does not decode with `MARSHAL`.");
    doc(s, "");
    doc(s, &format!("A `{name}Fault::System` the servant raises is passed through"));
    doc(s, "unchanged — repository id, minor code and completion status — so the");
    doc(s, "servant's answer about whether a retry is safe is the one the client");
    doc(s, "receives.");
    doc(s, "");
    doc(s, "# Many objects");
    doc(s, "");
    doc(s, &format!("The skeleton owns a `{name}Refs`, so it takes each request's object"));
    doc(s, "key apart before it looks at the operation and hands the servant the");
    doc(s, "identity it resolved. A key that does not parse under the scheme is");
    doc(s, "not ours; a key that parses but names nothing the servant `knows` is");
    doc(s, "`OBJECT_NOT_EXIST`. Both answers come from generated code, and neither");
    doc(s, "was possible while a skeleton had no key scheme at all.");
    let _ = writeln!(s, "pub struct {skel}<S: {servant}> {{");
    let _ = writeln!(s, "    /// The implementation invocations are delivered to.");
    let _ = writeln!(s, "    pub servant: S,");
    let _ = writeln!(s, "    refs: {name}Refs,");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s, "impl<S: {servant}> {skel}<S> {{");
    let _ = writeln!(s, "    /// A skeleton serving `servant`'s objects under `refs`.");
    let _ = writeln!(s, "    ///");
    let _ = writeln!(
        s,
        "    /// `refs` must be rooted at the key the `orbweaver_gen::rt::Server` was bound"
    );
    let _ = writeln!(s, "    /// with, or nothing this skeleton parses will match what arrives.");
    let _ = writeln!(s, "    pub fn new(__refs: {name}Refs, __servant: S) -> Self {{");
    let _ = writeln!(s, "        Self {{ servant: __servant, refs: __refs }}");
    let _ = writeln!(s, "    }}");
    let _ =
        writeln!(s, "    /// The key scheme in force, for minting references to these objects.");
    let _ = writeln!(s, "    pub fn refs(&self) -> &{name}Refs {{ &self.refs }}");
    let _ = writeln!(s, "}}");

    let _ = writeln!(s, "impl<S: {servant}> __rt::Dispatch for {skel}<S> {{");
    // Two gates, in the order `Server` applies them: is this key ours at all
    // (the scheme decides), and does the servant hold that object (only it
    // knows). A skeleton that answered the first alone would serve every
    // well-formed key as if the object existed.
    let _ = writeln!(s, "    fn knows(&self, __object_key: &[u8]) -> bool {{");
    let _ = writeln!(s, "        match self.refs.oid_of(__object_key) {{");
    let _ = writeln!(
        s,
        "            Some(__oid) => self.servant.knows(&{name}Target::new(__oid, &self.refs)),"
    );
    let _ = writeln!(s, "            None => false,");
    let _ = writeln!(s, "        }}");
    let _ = writeln!(s, "    }}");
    // The forward seam. Everything here is derivable; the decision inside
    // `servant.forward` / `servant.redirect` is not — see the trait method's
    // documentation. `redirect` is what `rt::Server` asks; `forward` is
    // generated too so a skeleton driven directly answers the same as one
    // driven through the server.
    let _ = writeln!(
        s,
        "    fn forward(&mut self, __req: &__rt::Request) -> ::std::option::Option<__rt::Ior> {{"
    );
    let _ = writeln!(s, "        let __oid = self.refs.oid_of(&__req.object_key)?;");
    let _ = writeln!(s, "        let __at = {name}Target::new(__oid, &self.refs);");
    let _ = writeln!(s, "        self.servant.forward(&__at)");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(
        s,
        "    fn redirect(&mut self, __req: &__rt::Request) -> ::std::option::Option<__rt::Forward> {{"
    );
    let _ = writeln!(s, "        let __oid = self.refs.oid_of(&__req.object_key)?;");
    let _ = writeln!(s, "        let __at = {name}Target::new(__oid, &self.refs);");
    let _ = writeln!(s, "        self.servant.redirect(&__at)");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "    fn dispatch_body(");
    let _ = writeln!(s, "        &mut self,");
    let _ = writeln!(s, "        __req: &__rt::Request,");
    let _ = writeln!(s, "        __out: &mut __rt::Encoder,");
    let _ =
        writeln!(s, "    ) -> ::std::result::Result<__rt::DispatchBody, __rt::SystemException> {{");
    // `Server` refuses an unknown key before it gets here, so this is the
    // belt-and-braces answer for a skeleton driven directly.
    let _ = writeln!(s, "        let __oid = self");
    let _ = writeln!(s, "            .refs");
    let _ = writeln!(s, "            .oid_of(&__req.object_key)");
    let _ = writeln!(s, "            .ok_or_else(__rt::SystemException::object_not_exist)?;");
    let _ = writeln!(s, "        let __at = {name}Target::new(__oid, &self.refs);");
    // The decoder is positioned inside the whole message, so alignment is
    // measured from the GIOP header — the origin rule, honoured by not copying.
    let _ = writeln!(
        s,
        "        let mut __args = __req.body().map_err(|_| __rt::SystemException::marshal())?;"
    );
    let _ = writeln!(s, "        match __req.operation.as_str() {{");

    for (wire, rust, sig) in methods(ops, attrs) {
        let shape = op_shape(&sig, cx)?;
        let _ = writeln!(s, "            \"{wire}\" => {{");
        for (arg, ty) in &shape.ins {
            let _ = writeln!(
                s,
                "                let {arg}: {ty} = __Cdr::get(&mut __args)\n                    \
                 .map_err(|_| __rt::SystemException::marshal())?;"
            );
        }
        let mut passed = vec!["&__at".to_owned()];
        passed.extend(shape.ins.iter().map(|(a, _)| a.clone()));
        let call = format!("self.servant.{}({})", ident(&rust), passed.join(", "));
        if sig.oneway {
            for line in [
                "// oneway (§9.4.1): no reply may be written, at all. An empty one",
                "// is a whole extra message, which the peer — not waiting for it —",
                "// would read as the header of the next reply. The servant's",
                "// verdict has no way back, so it is dropped — and logged, so the",
                "// drop is a decision somebody can see rather than a silence.",
            ] {
                let _ = writeln!(s, "                {line}");
            }
            let _ = writeln!(s, "                if let Err(__f) = {call} {{");
            let _ = writeln!(
                s,
                "                    __rt::oneway_fault_dropped(\"{id}\", \"{wire}\", &__f);"
            );
            let _ = writeln!(s, "                }}");
            let _ = writeln!(s, "                Ok(__rt::DispatchBody::Return)");
            let _ = writeln!(s, "            }}");
            continue;
        }
        let binding = match shape.rets.len() {
            0 => "()".to_owned(),
            1 => "__r0".to_owned(),
            n => format!("({})", (0..n).map(|i| format!("__r{i}")).collect::<Vec<_>>().join(", ")),
        };
        let _ = writeln!(s, "                match {call} {{");
        let _ = writeln!(s, "                    Ok({binding}) => {{");
        for i in 0..shape.rets.len() {
            let _ = writeln!(
                s,
                "                        __r{i}.put(__out).map_err(|_| \
                 __rt::SystemException::marshal())?;"
            );
        }
        let _ = writeln!(s, "                        Ok(__rt::DispatchBody::Return)");
        let _ = writeln!(s, "                    }}");
        // Nothing was written before the label was known: the whole buffer
        // travels under one reply status, so a half-written result followed by
        // an exception body would be neither. `write` decides that status —
        // a user exception body under `USER_EXCEPTION`, or nothing at all and
        // an `Err` the server turns into a `SystemException` reply.
        let _ = writeln!(s, "                    Err(__f) => __f.write(__out),");
        let _ = writeln!(s, "                }}");
        let _ = writeln!(s, "            }}");
    }

    // `_is_a` from the resolved inheritance chain: an ORB asks before it
    // narrows, and a skeleton that answers only its own id cannot be narrowed
    // to through a base-typed reference.
    let mut ancestry: Vec<String> = vec![id.to_owned()];
    ancestry.extend(registry.ancestors(id));
    ancestry.sort();
    ancestry.dedup();
    let _ = writeln!(s, "            \"_is_a\" => {{");
    let _ = writeln!(
        s,
        "                let __id: ::std::string::String = __Cdr::get(&mut __args)\n                    \
         .map_err(|_| __rt::SystemException::marshal())?;"
    );
    let _ = writeln!(s, "                let __answer = matches!(__id.as_str(),");
    for a in &ancestry {
        let _ = writeln!(s, "                    \"{a}\" |");
    }
    let _ = writeln!(s, "                    __rt::OBJECT_ID);");
    let _ = writeln!(
        s,
        "                __answer.put(__out).map_err(|_| __rt::SystemException::marshal())?;"
    );
    let _ = writeln!(s, "                Ok(__rt::DispatchBody::Return)");
    let _ = writeln!(s, "            }}");
    let _ = writeln!(s, "            \"_non_existent\" => {{");
    let _ = writeln!(
        s,
        "                false.put(__out).map_err(|_| __rt::SystemException::marshal())?;"
    );
    let _ = writeln!(s, "                Ok(__rt::DispatchBody::Return)");
    let _ = writeln!(s, "            }}");
    let _ = writeln!(s, "            _ => Err(__rt::SystemException::bad_operation()),");
    let _ = writeln!(s, "        }}");
    let _ = writeln!(s, "    }}");

    // The narrow entry point cannot carry a user exception, so one arriving
    // here gets the standard mapping: UNKNOWN with the OMG minor for an
    // unlisted user exception.
    let _ = writeln!(s, "    fn dispatch(");
    let _ = writeln!(s, "        &mut self,");
    let _ = writeln!(s, "        __req: &__rt::Request,");
    let _ = writeln!(s, "        __out: &mut __rt::Encoder,");
    let _ = writeln!(s, "    ) -> ::std::result::Result<(), __rt::SystemException> {{");
    let _ = writeln!(s, "        match self.dispatch_body(__req, __out)? {{");
    let _ = writeln!(s, "            __rt::DispatchBody::Return => Ok(()),");
    let _ = writeln!(s, "            __rt::DispatchBody::UserException => {{");
    let _ = writeln!(s, "                Err(__rt::SystemException::unknown_user_exception())");
    let _ = writeln!(s, "            }}");
    let _ = writeln!(s, "        }}");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "}}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{Generated, emit};
    use orbweaver_registry::Registry;

    fn generate(src: &str) -> Generated {
        let spec = orbweaver_idl::parse(src).expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        emit(&r, "g")
    }

    /// The framing hazard, at the generator level: a oneway arm must contain no
    /// write into the reply encoder. Checked as *absence*, because a oneway
    /// that writes an empty body still writes a message.
    #[test]
    fn a_oneway_arm_writes_nothing_into_the_reply() {
        let g =
            generate("module m { interface I { oneway void fire(in long n); long ping(); }; };");
        let arm = g
            .source
            .split("\"fire\" => {")
            .nth(1)
            .expect("a fire arm")
            .split("\"ping\" => {")
            .next()
            .expect("bounded by the next arm");
        assert!(!arm.contains("put(__out)"), "a oneway may write no reply:\n{arm}");
        assert!(arm.contains("if let Err(__f) = self.servant.fire(&__at, n)"), "{arm}");
        // Dropped, but not in silence: §9.4.1 leaves the fault nowhere to go,
        // and a `let _ =` would make an undebuggable server out of a correct
        // protocol decision.
        assert!(
            arm.contains("__rt::oneway_fault_dropped(\"IDL:m/I:1.0\", \"fire\", &__f)"),
            "a dropped oneway fault must be reported:\n{arm}"
        );
        // The twoway next to it must still write one, or the check above would
        // pass on a generator that never writes replies at all.
        let ping = g.source.split("\"ping\" => {").nth(1).expect("a ping arm");
        assert!(ping.contains("__r0.put(__out)"), "{ping}");
    }

    #[test]
    fn a_readonly_attribute_gets_a_getter_arm_and_no_setter_arm() {
        let g = generate(
            "module m { interface I { readonly attribute string label; attribute long n; }; };",
        );
        let skel = g.source.split("pub trait IServant").nth(1).expect("a servant trait");
        assert!(skel.contains("\"_get_label\" => {"), "{skel}");
        assert!(skel.contains("\"_get_n\" => {"), "{skel}");
        assert!(skel.contains("\"_set_n\" => {"), "{skel}");
        assert!(!skel.contains("\"_set_label\""), "readonly must have no setter arm:\n{skel}");
        assert!(
            skel.contains("fn label(&mut self) -> ::std::result::Result<::std::string::String,"),
            "{skel}"
        );
        assert!(!skel.contains("fn set_label"), "{skel}");
    }

    /// Arguments come out of the request's own decoder, which is positioned
    /// inside the whole message. Copying the body into a fresh buffer would
    /// restart alignment at zero and misplace every 8-aligned member under
    /// GIOP 1.0/1.1, where the body is not aligned at all.
    #[test]
    fn arguments_are_decoded_from_the_message_positioned_decoder() {
        let g = generate("module m { interface I { void take(in double d); }; };");
        assert!(g.source.contains("let mut __args = __req.body()"), "{}", g.source);
        assert!(!g.source.contains("__rt::Decoder::new"), "no second decoder may appear");
        assert!(!g.source.contains("__rt::Encoder::new(__req"), "no second encoder may appear");
    }

    #[test]
    fn raised_exceptions_get_a_variant_and_the_repository_id_travels_first() {
        let g = generate(
            "module m { exception NotFound { string key; }; exception Busy {}; \
             interface I { string get(in string key) raises (NotFound, Busy); }; };",
        );
        assert!(g.source.contains("pub enum IFault {"), "{}", g.source);
        assert!(g.source.contains("NotFound(crate::g::m::NotFound),"), "{}", g.source);
        assert!(g.source.contains("Busy(crate::g::m::Busy),"), "{}", g.source);
        assert!(g.source.contains("__out.put_str(\"IDL:m/NotFound:1.0\");"), "{}", g.source);
        assert!(
            g.source.contains("Self::NotFound(_) => Some(\"IDL:m/NotFound:1.0\"),"),
            "{}",
            g.source
        );
        assert!(g.source.contains("Ok(__rt::DispatchBody::UserException)"), "{}", g.source);
    }

    /// The gap this batch closed. An interface with no `raises` clause used to
    /// get an *uninhabited* error type, so its servant could not fail at all —
    /// no `OBJECT_NOT_EXIST` for an unknown key, no `NO_PERMISSION` for a
    /// refusal. The `System` variant is unconditional for exactly that reason.
    #[test]
    fn an_interface_that_raises_nothing_can_still_fail_with_a_system_exception() {
        let g = generate("module m { interface I { long ping(); }; };");
        let decl = g.source.split("pub enum IFault {").nth(1).expect("the enum");
        let body = decl.split('}').next().expect("the variant list");
        assert!(body.contains("System(__rt::SystemException),"), "{body}");
        assert_eq!(
            body.matches("    System(").count() + body.matches("(crate::g::").count(),
            1,
            "no user variants may be emitted:\n{body}"
        );
        assert!(!g.source.contains("match *self {}"), "the type must be inhabited:\n{}", g.source);
        assert!(g.source.contains("::std::result::Result<i32, IFault>"), "{}", g.source);
        // And the fault still knows how to become a reply: a system exception
        // replaces the reply rather than filling one in.
        assert!(g.source.contains("Self::System(__ex) => Err(__ex.clone()),"), "{}", g.source);
    }

    /// A servant hands `write` whatever it failed with and gets back the reply
    /// status those bytes travel under — one line at the call site, so the two
    /// cases cannot be labelled differently by two arms of the generator.
    #[test]
    fn a_fault_decides_the_reply_status_and_the_arm_is_one_line() {
        let g =
            generate("module m { exception Busy {}; interface I { long f() raises (Busy); }; };");
        let arm = g.source.split("\"f\" => {").nth(1).expect("the f arm");
        assert!(arm.contains("Err(__f) => __f.write(__out),"), "{arm}");
        assert!(
            g.source.contains("impl ::std::convert::From<__rt::SystemException> for IFault"),
            "{}",
            g.source
        );
    }

    #[test]
    fn unknown_operations_and_the_object_probes_are_answered() {
        let g = generate(
            "module m { interface Base { long ping(); }; interface Derived : Base { void own(); }; };",
        );
        let d = g.source.split("pub struct DerivedSkeleton").nth(1).expect("a skeleton");
        assert!(d.contains("_ => Err(__rt::SystemException::bad_operation()),"), "{d}");
        assert!(d.contains("\"IDL:m/Derived:1.0\""), "{d}");
        assert!(d.contains("\"IDL:m/Base:1.0\""), "_is_a must answer for the base too:\n{d}");
        assert!(d.contains("__rt::OBJECT_ID"), "{d}");
    }

    /// Inherited operations *and* inherited attributes: the asymmetry that the
    /// shared resolver removed. A skeleton missing an inherited `_get_` would
    /// refuse it with `BAD_OPERATION` — a wrong answer, not a missing feature.
    #[test]
    fn inherited_attributes_reach_both_halves() {
        let g = generate(
            "module m { interface Base { readonly attribute long size; }; \
             interface Derived : Base { void own(); }; };",
        );
        let d = g.source.split("pub struct DerivedClient").nth(1).expect("a stub");
        assert!(d.contains("invoke(\"_get_size\""), "the stub must inherit the accessor:\n{d}");
        let sk = g.source.split("pub trait DerivedServant").nth(1).expect("a servant trait");
        assert!(sk.contains("\"_get_size\" => {"), "the skeleton must serve it:\n{sk}");
    }

    /// `corpus/golden/22-moe-control-plane.idl` has `register_expert(in Expert
    /// e, ...)`. Every local the generator emits is `__`-prefixed so no IDL
    /// identifier can shadow one; `e` is the pin, because `e` is what a
    /// hand-written encoder would have called its encoder.
    #[test]
    fn an_argument_named_e_does_not_collide_with_a_generated_local() {
        let g = generate("module m { struct T { long v; }; interface I { long take(in T e); }; };");
        assert!(
            g.source.contains("let e: crate::g::m::T = __Cdr::get(&mut __args)"),
            "{}",
            g.source
        );
        assert!(g.source.contains("self.servant.take(&__at, e)"), "{}", g.source);
        assert!(
            g.source.contains("fn take(&mut self, __at: &ITarget<'_>, e: crate::g::m::T)"),
            "{}",
            g.source
        );
    }

    /// The gap this batch closed, at the generator level. A skeleton with no
    /// key scheme answers every object key identically, so one process serves
    /// one object — which is not what a single hand-written servant in this
    /// workspace does.
    #[test]
    fn a_skeleton_derives_its_object_keys_reversibly_from_the_root() {
        let g = generate("module m { interface I { long ping(); }; };");
        assert!(g.source.contains("pub const KEY_INFIX: &'static str = \"/I/\";"), "{}", g.source);
        assert!(
            g.source.contains("pub const TYPE_ID: &'static str = \"IDL:m/I:1.0\";"),
            "{}",
            g.source
        );
        // Both directions, and both through `rt` — the key scheme is a fact of
        // the interface, the byte-level derivation is not.
        assert!(g.source.contains("self.home.key_of(&self.infix, __oid)"), "{}", g.source);
        assert!(g.source.contains("self.home.oid_of(&self.infix, __key)"), "{}", g.source);
        // No table of minted keys anywhere in the emitted scheme.
        let refs = g.source.split("pub struct IRefs").nth(1).expect("the refs type");
        let refs = refs.split("pub struct ITarget").next().expect("bounded by the target type");
        assert!(!refs.contains("BTreeMap"), "keys must be derived, not minted:\n{refs}");
    }

    /// `knows` is required rather than defaulted, and the skeleton asks *both*
    /// gates: does the key parse under our scheme, and does the servant hold
    /// that object. A skeleton answering the first alone would serve every
    /// well-formed key as though the object existed.
    #[test]
    fn knows_is_asked_of_the_servant_and_has_no_default() {
        let g = generate("module m { interface I { long ping(); }; };");
        let t = g.source.split("pub trait IServant").nth(1).expect("the trait");
        let t = t.split("pub trait IObject").next().expect("bounded by the object trait");
        assert!(t.contains("fn knows(&self, __at: &ITarget<'_>) -> bool;"), "{t}");
        assert!(!t.contains("fn knows(&self, __at: &ITarget<'_>) -> bool {"), "no default:\n{t}");
        let d = g.source.split("impl<S: IServant> __rt::Dispatch").nth(1).expect("the dispatch");
        assert!(d.contains("Some(__oid) => self.servant.knows(&ITarget::new(__oid, &self.refs))"));
        assert!(d.contains("None => false,"), "{d}");
    }

    /// The `LOCATION_FORWARD` seam. Everything mechanical is generated — the
    /// key is decoded and the identity handed over; only the decision is the
    /// servant's, because no IDL clause describes object location.
    #[test]
    fn the_forward_seam_is_generated_and_defaulted_rather_than_silent() {
        let g = generate("module m { interface I { long ping(); }; };");
        let t = g.source.split("pub trait IServant").nth(1).expect("the trait");
        assert!(
            t.contains(
                "fn forward(&mut self, __at: &ITarget<'_>) -> ::std::option::Option<__rt::Ior> {"
            ),
            "{t}"
        );
        // The permanent form is a second, defaulted hook — not a changed
        // return type — so every servant that already implements `forward`
        // keeps compiling and keeps meaning temporary.
        assert!(
            t.contains("fn redirect(&mut self, __at: &ITarget<'_>) -> ::std::option::Option<__rt::Forward> {"),
            "{t}"
        );
        assert!(t.contains("self.forward(__at).map(__rt::Forward::Temporary)"), "{t}");
        assert!(!t.contains("LOCATION_FORWARD_PERM` is not reachable"), "{t}");
        let d = g.source.split("impl<S: IServant> __rt::Dispatch").nth(1).expect("the dispatch");
        assert!(
            d.contains(
                "fn forward(&mut self, __req: &__rt::Request) -> ::std::option::Option<__rt::Ior> {"
            ),
            "{d}"
        );
        assert!(d.contains("self.servant.forward(&__at)"), "{d}");
        assert!(
            d.contains("fn redirect(&mut self, __req: &__rt::Request) -> ::std::option::Option<__rt::Forward> {"),
            "{d}"
        );
        assert!(d.contains("self.servant.redirect(&__at)"), "{d}");
    }

    /// An operation returning an object reference must be answerable without
    /// the servant assembling an IIOP profile: the identity it was called with
    /// can name its neighbours.
    #[test]
    fn a_servant_can_mint_a_reference_without_building_an_ior() {
        let g = generate("module m { interface I { I child(in string leaf); }; };");
        assert!(
            g.source.contains("pub fn sibling(&self, __oid: &str) -> __rt::ObjRef"),
            "{}",
            g.source
        );
        assert!(
            g.source.contains("pub fn reference(&self, __oid: &str) -> __rt::ObjRef"),
            "{}",
            g.source
        );
        assert!(g.source.contains("pub fn nil() -> __rt::ObjRef"), "{}", g.source);
        // The profile is assembled in `rt`, exactly once, and never emitted.
        assert!(!g.source.contains("IiopProfile"), "the wire shape must stay in rt:\n{}", g.source);
        assert!(g.source.contains("self.home.reference(Self::TYPE_ID"), "{}", g.source);
    }

    /// The map shape, generated in terms of the identity shape. An absent oid
    /// is `OBJECT_NOT_EXIST` with `COMPLETED_NO` — nothing ran.
    #[test]
    fn the_per_object_map_is_generated_in_terms_of_the_identity_trait() {
        let g = generate("module m { interface I { long ping(); }; };");
        assert!(g.source.contains("pub trait IObject {"), "{}", g.source);
        assert!(
            g.source.contains("fn ping(&mut self) -> ::std::result::Result<i32, IFault>;"),
            "{}",
            g.source
        );
        let m =
            g.source.split("impl<S: IObject> IServant for IObjects<S>").nth(1).expect("adapter");
        assert!(m.contains("self.objects.contains_key(__at.oid())"), "{m}");
        assert!(m.contains("Some(__o) => __o.ping(),"), "{m}");
        assert!(
            m.contains("Err(IFault::System(__rt::raise::object_not_exist().did_not_run()))"),
            "{m}"
        );
    }
}
