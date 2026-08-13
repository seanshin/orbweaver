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
//! * `<I>Servant` — a trait with one method per operation and per attribute
//!   accessor, taking **decoded Rust arguments** and returning
//!   `Result<Ret, <I>Fault>`. No `Encoder`, no `Decoder`, no operation names:
//!   a servant that never sees the wire cannot get the wire wrong;
//! * `<I>Skeleton<S>` — the [`rt::Dispatch`](crate::rt::Dispatch)
//!   implementation that decodes, calls the trait, and encodes.
//!
//! Both halves are generated from the same [`OpShape`](crate::OpShape), so the
//! arguments a caller passes and the arguments a servant receives are the same
//! list by construction rather than by review.
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
    ids.iter()
        .map(|id| {
            let path = cx.path_of(id);
            let last = path.last().cloned().unwrap_or_default();
            let variant =
                if short.get(&last).copied().unwrap_or(0) > 1 { path.join("_") } else { last };
            Raise { id: id.clone(), variant: ident(&variant), ty: rust_path(id, cx) }
        })
        .filter(|r| registry.get(&r.id).is_some())
        .collect()
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
    emit_trait(&mut s, registry, &name, id, &ops, &attrs, cx)?;
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
    doc(s, "of them. Build one with `rt::raise::*`, which does not produce a");
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
        doc(s, "`rt::GiopError::UserException`.");
    }
    let _ = writeln!(s, "#[derive(Debug, Clone)]");
    let _ = writeln!(s, "pub enum {ty} {{");
    let _ = writeln!(s, "    /// A CORBA system exception, with the completion status the");
    let _ = writeln!(s, "    /// servant chose. Travels as a `SystemException` reply.");
    let _ = writeln!(s, "    System(rt::SystemException),");
    for r in raises {
        let _ = writeln!(s, "    /// IDL exception `{}`.", r.id);
        let _ = writeln!(s, "    {}({}),", r.variant, r.ty);
    }
    let _ = writeln!(s, "}}");

    // `?` and `.into()` on a raise, which is how a servant body reads when the
    // failure comes from a helper rather than from the operation itself.
    let _ = writeln!(s, "impl From<rt::SystemException> for {ty} {{");
    let _ = writeln!(s, "    fn from(__ex: rt::SystemException) -> Self {{");
    let _ = writeln!(s, "        Self::System(__ex)");
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "}}");

    // Hand-written rather than derived: `rt::SystemException` has no
    // `PartialEq`, and a servant's own tests want to compare faults. Comparing
    // the three fields that travel is the whole of what a system exception is.
    let _ = writeln!(s, "impl PartialEq for {ty} {{");
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
    let _ = writeln!(s, "    pub fn user_id(&self) -> Option<&'static str> {{");
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
        "    pub fn write(&self, __out: &mut rt::Encoder) \
         -> Result<rt::DispatchBody, rt::SystemException> {{"
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
            "                __v.put(__out).map_err(|_| rt::SystemException::marshal())?;"
        );
        let _ = writeln!(s, "                Ok(rt::DispatchBody::UserException)");
        let _ = writeln!(s, "            }}");
    }
    let _ = writeln!(s, "        }}");
    let _ = writeln!(s, "    }}");
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
    doc(s, "`rt::Dispatch`.");
    doc(s, "");
    doc(s, &format!("Every method may fail with a `{exc}`: a declared user exception,"));
    doc(s, "or a system exception built with `rt::raise::*` — the vocabulary");
    doc(s, "(`OBJECT_NOT_EXIST`, `NO_PERMISSION`, `BAD_PARAM`, `TRANSIENT`) that");
    doc(s, "no contract declares and every servant needs.");
    let _ = writeln!(s, "pub trait {name}Servant {{");
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
            "    fn {}(&mut self{}) -> Result<{}, {exc}>;",
            ident(&rust),
            shape.args,
            shape.ret_ty
        );
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
    doc(s, "Hand it to `rt::Server::serve` in place of a hand-written");
    doc(s, "`rt::Dispatch`. What it answers, beyond the contract's own");
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
    let _ = writeln!(s, "pub struct {skel}<S: {servant}> {{");
    let _ = writeln!(s, "    /// The implementation invocations are delivered to.");
    let _ = writeln!(s, "    pub servant: S,");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s, "impl<S: {servant}> {skel}<S> {{");
    let _ = writeln!(s, "    /// A skeleton over a servant.");
    let _ = writeln!(s, "    pub fn new(servant: S) -> Self {{ Self {{ servant }} }}");
    let _ = writeln!(s, "}}");

    let _ = writeln!(s, "impl<S: {servant}> rt::Dispatch for {skel}<S> {{");
    let _ = writeln!(s, "    fn dispatch_body(");
    let _ = writeln!(s, "        &mut self,");
    let _ = writeln!(s, "        __req: &rt::Request,");
    let _ = writeln!(s, "        __out: &mut rt::Encoder,");
    let _ = writeln!(s, "    ) -> Result<rt::DispatchBody, rt::SystemException> {{");
    // The decoder is positioned inside the whole message, so alignment is
    // measured from the GIOP header — the origin rule, honoured by not copying.
    let _ = writeln!(
        s,
        "        let mut __args = __req.body().map_err(|_| rt::SystemException::marshal())?;"
    );
    let _ = writeln!(s, "        match __req.operation.as_str() {{");

    for (wire, rust, sig) in methods(ops, attrs) {
        let shape = op_shape(&sig, cx)?;
        let _ = writeln!(s, "            \"{wire}\" => {{");
        for (arg, ty) in &shape.ins {
            let _ = writeln!(
                s,
                "                let {arg}: {ty} = Cdr::get(&mut __args)\n                    \
                 .map_err(|_| rt::SystemException::marshal())?;"
            );
        }
        let call = format!(
            "self.servant.{}({})",
            ident(&rust),
            shape.ins.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>().join(", ")
        );
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
                "                    rt::oneway_fault_dropped(\"{id}\", \"{wire}\", &__f);"
            );
            let _ = writeln!(s, "                }}");
            let _ = writeln!(s, "                Ok(rt::DispatchBody::Return)");
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
                 rt::SystemException::marshal())?;"
            );
        }
        let _ = writeln!(s, "                        Ok(rt::DispatchBody::Return)");
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
        "                let __id: String = Cdr::get(&mut __args)\n                    \
         .map_err(|_| rt::SystemException::marshal())?;"
    );
    let _ = writeln!(s, "                let __answer = matches!(__id.as_str(),");
    for a in &ancestry {
        let _ = writeln!(s, "                    \"{a}\" |");
    }
    let _ = writeln!(s, "                    rt::OBJECT_ID);");
    let _ = writeln!(
        s,
        "                __answer.put(__out).map_err(|_| rt::SystemException::marshal())?;"
    );
    let _ = writeln!(s, "                Ok(rt::DispatchBody::Return)");
    let _ = writeln!(s, "            }}");
    let _ = writeln!(s, "            \"_non_existent\" => {{");
    let _ = writeln!(
        s,
        "                false.put(__out).map_err(|_| rt::SystemException::marshal())?;"
    );
    let _ = writeln!(s, "                Ok(rt::DispatchBody::Return)");
    let _ = writeln!(s, "            }}");
    let _ = writeln!(s, "            _ => Err(rt::SystemException::bad_operation()),");
    let _ = writeln!(s, "        }}");
    let _ = writeln!(s, "    }}");

    // The narrow entry point cannot carry a user exception, so one arriving
    // here gets the standard mapping: UNKNOWN with the OMG minor for an
    // unlisted user exception.
    let _ = writeln!(s, "    fn dispatch(");
    let _ = writeln!(s, "        &mut self,");
    let _ = writeln!(s, "        __req: &rt::Request,");
    let _ = writeln!(s, "        __out: &mut rt::Encoder,");
    let _ = writeln!(s, "    ) -> Result<(), rt::SystemException> {{");
    let _ = writeln!(s, "        match self.dispatch_body(__req, __out)? {{");
    let _ = writeln!(s, "            rt::DispatchBody::Return => Ok(()),");
    let _ = writeln!(s, "            rt::DispatchBody::UserException => {{");
    let _ = writeln!(s, "                Err(rt::SystemException::unknown_user_exception())");
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
        assert!(arm.contains("if let Err(__f) = self.servant.fire(n)"), "{arm}");
        // Dropped, but not in silence: §9.4.1 leaves the fault nowhere to go,
        // and a `let _ =` would make an undebuggable server out of a correct
        // protocol decision.
        assert!(
            arm.contains("rt::oneway_fault_dropped(\"IDL:m/I:1.0\", \"fire\", &__f)"),
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
        assert!(skel.contains("fn label(&mut self) -> Result<String,"), "{skel}");
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
        assert!(!g.source.contains("rt::Decoder::new"), "no second decoder may appear");
        assert!(!g.source.contains("rt::Encoder::new(__req"), "no second encoder may appear");
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
        assert!(g.source.contains("Ok(rt::DispatchBody::UserException)"), "{}", g.source);
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
        assert!(body.contains("System(rt::SystemException),"), "{body}");
        assert_eq!(
            body.matches("    System(").count() + body.matches("(crate::g::").count(),
            1,
            "no user variants may be emitted:\n{body}"
        );
        assert!(!g.source.contains("match *self {}"), "the type must be inhabited:\n{}", g.source);
        assert!(g.source.contains("Result<i32, IFault>"), "{}", g.source);
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
        assert!(g.source.contains("impl From<rt::SystemException> for IFault"), "{}", g.source);
    }

    #[test]
    fn unknown_operations_and_the_object_probes_are_answered() {
        let g = generate(
            "module m { interface Base { long ping(); }; interface Derived : Base { void own(); }; };",
        );
        let d = g.source.split("pub struct DerivedSkeleton").nth(1).expect("a skeleton");
        assert!(d.contains("_ => Err(rt::SystemException::bad_operation()),"), "{d}");
        assert!(d.contains("\"IDL:m/Derived:1.0\""), "{d}");
        assert!(d.contains("\"IDL:m/Base:1.0\""), "_is_a must answer for the base too:\n{d}");
        assert!(d.contains("rt::OBJECT_ID"), "{d}");
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
        assert!(g.source.contains("let e: crate::g::m::T = Cdr::get(&mut __args)"), "{}", g.source);
        assert!(g.source.contains("self.servant.take(e)"), "{}", g.source);
        assert!(g.source.contains("fn take(&mut self, e: crate::g::m::T)"), "{}", g.source);
    }
}
