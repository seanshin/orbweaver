//! Server skeletons: the half of static generation that answers calls.
//!
//! The client stub in [`crate`] turns a Rust call into a GIOP request. A
//! skeleton is its mirror: it turns a GIOP request back into a Rust call. For
//! each interface it emits
//!
//! * `<I>UserException` — one variant per exception named in a `raises`
//!   clause, and the `write` that puts the body §9.4.3.1 describes: repository
//!   id first, then the members;
//! * `<I>Servant` — a trait with one method per operation and per attribute
//!   accessor, taking **decoded Rust arguments** and returning
//!   `Result<Ret, <I>UserException>`. No `Encoder`, no `Decoder`, no operation
//!   names: a servant that never sees the wire cannot get the wire wrong;
//! * `<I>Skeleton<S>` — the [`rt::Dispatch`](crate::rt::Dispatch)
//!   implementation that decodes, calls the trait, and encodes.
//!
//! Both halves are generated from the same [`OpShape`](crate::OpShape), so the
//! arguments a caller passes and the arguments a servant receives are the same
//! list by construction rather than by review.
//!
//! # The three things a skeleton generator gets wrong
//!
//! **oneway.** §9.4.1: a request with `response_expected` false gets no reply
//! *at all*. An empty `NO_EXCEPTION` reply is not "nearly nothing" — it is a
//! whole extra message, and the peer, which is not waiting for one, reads it as
//! the header of the next reply. Every later request on that connection is then
//! answered with the wrong bytes. A oneway arm here therefore writes nothing
//! into the reply encoder, and drops a raise it has no way to carry.
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
    doc, getter_sig, ident, op_doc, op_shape, path_of, resolved_members, rust_path, setter_sig,
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
fn raises_of(registry: &Registry, ops: &BTreeMap<String, OperationSig>, root: &str) -> Vec<Raise> {
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
        let last = path_of(id).last().cloned().unwrap_or_default();
        *short.entry(last).or_insert(0) += 1;
    }
    ids.iter()
        .map(|id| {
            let path = path_of(id);
            let last = path.last().cloned().unwrap_or_default();
            let variant =
                if short.get(&last).copied().unwrap_or(0) > 1 { path.join("_") } else { last };
            Raise { id: id.clone(), variant: ident(&variant), ty: rust_path(id, root) }
        })
        .filter(|r| registry.get(&r.id).is_some())
        .collect()
}

/// Generates the servant trait, its user-exception type and the dispatcher.
pub(crate) fn emit_skeleton(registry: &Registry, id: &str, root: &str) -> Result<String, String> {
    if registry.interface(id).is_none() {
        return Err("not an interface".to_owned());
    }
    let name = ident(&path_of(id).last().cloned().unwrap_or_default());
    let (ops, attrs) = resolved_members(registry, id);
    let raises = raises_of(registry, &ops, root);

    let mut s = String::new();
    emit_user_exception(&mut s, &name, id, &raises);
    emit_trait(&mut s, registry, &name, id, &ops, &attrs, root)?;
    emit_dispatch(&mut s, registry, &name, id, &ops, &attrs, root)?;
    Ok(s)
}

fn emit_user_exception(s: &mut String, name: &str, id: &str, raises: &[Raise]) {
    let ty = format!("{name}UserException");
    doc(s, &format!("The user exceptions `{id}` may raise."));
    doc(s, "");
    if raises.is_empty() {
        doc(s, "This interface names no `raises` clause, so there is nothing a");
        doc(s, "servant method can raise and this type has no values. A system");
        doc(s, "exception is still possible: the skeleton produces `BAD_OPERATION`");
        doc(s, "for a name this interface does not have and `MARSHAL` for a body");
        doc(s, "that does not decode.");
    } else {
        doc(s, "One variant per exception named in a `raises` clause of this");
        doc(s, "interface or of one it inherits. `write` puts the body §9.4.3.1");
        doc(s, "describes — repository id first, then the members — which is");
        doc(s, "exactly what the client side reads back out of");
        doc(s, "`rt::GiopError::UserException`.");
    }
    let _ = writeln!(s, "#[derive(Debug, Clone, PartialEq)]");
    let _ = writeln!(s, "pub enum {ty} {{");
    for r in raises {
        let _ = writeln!(s, "    /// IDL exception `{}`.", r.id);
        let _ = writeln!(s, "    {}({}),", r.variant, r.ty);
    }
    let _ = writeln!(s, "}}");

    let _ = writeln!(s, "impl {ty} {{");
    let _ = writeln!(s, "    /// The repository id that travels first in the exception body.");
    let _ = writeln!(s, "    pub fn id(&self) -> &'static str {{");
    if raises.is_empty() {
        let _ = writeln!(s, "        match *self {{}}");
    } else {
        let _ = writeln!(s, "        match self {{");
        for r in raises {
            let _ = writeln!(s, "            Self::{}(_) => \"{}\",", r.variant, r.id);
        }
        let _ = writeln!(s, "        }}");
    }
    let _ = writeln!(s, "    }}");
    let _ = writeln!(s, "    /// Writes the exception body: repository id, then the members.");
    let _ = writeln!(
        s,
        "    pub fn write(&self, __out: &mut rt::Encoder) -> Result<(), rt::GiopError> {{"
    );
    if raises.is_empty() {
        let _ = writeln!(s, "        let _ = __out;");
        let _ = writeln!(s, "        match *self {{}}");
    } else {
        let _ = writeln!(s, "        __out.put_str(self.id());");
        let _ = writeln!(s, "        match self {{");
        for r in raises {
            let _ = writeln!(s, "            Self::{}(__v) => __v.put(__out),", r.variant);
        }
        let _ = writeln!(s, "        }}");
    }
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
    root: &str,
) -> Result<(), String> {
    let exc = format!("{name}UserException");
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
    let _ = writeln!(s, "pub trait {name}Servant {{");
    for (wire, rust, sig) in methods(ops, attrs) {
        let shape = op_shape(&sig, root)?;
        let mut docs = String::new();
        op_doc(&mut docs, &wire, &sig.annotations);
        if sig.oneway {
            doc(&mut docs, "");
            doc(&mut docs, "`oneway`: the caller is not waiting, so neither a result nor a raise");
            doc(&mut docs, "can reach it. Returning `Err` here is dropped, deliberately.");
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
    root: &str,
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
        let shape = op_shape(&sig, root)?;
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
                "// verdict has no way back, so it is dropped here.",
            ] {
                let _ = writeln!(s, "                {line}");
            }
            let _ = writeln!(s, "                let _ = {call};");
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
        // an exception body would be neither.
        let _ = writeln!(s, "                    Err(__ex) => {{");
        let _ = writeln!(
            s,
            "                        __ex.write(__out).map_err(|_| \
             rt::SystemException::marshal())?;"
        );
        let _ = writeln!(s, "                        Ok(rt::DispatchBody::UserException)");
        let _ = writeln!(s, "                    }}");
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
        assert!(arm.contains("let _ = self.servant.fire(n)"), "{arm}");
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
        assert!(g.source.contains("pub enum IUserException {"), "{}", g.source);
        assert!(g.source.contains("NotFound(crate::g::m::NotFound),"), "{}", g.source);
        assert!(g.source.contains("Busy(crate::g::m::Busy),"), "{}", g.source);
        assert!(g.source.contains("__out.put_str(self.id());"), "{}", g.source);
        assert!(g.source.contains("Self::NotFound(_) => \"IDL:m/NotFound:1.0\","), "{}", g.source);
        assert!(g.source.contains("Ok(rt::DispatchBody::UserException)"), "{}", g.source);
    }

    #[test]
    fn an_interface_that_raises_nothing_gets_an_empty_exception_type() {
        let g = generate("module m { interface I { long ping(); }; };");
        let decl = g.source.split("pub enum IUserException {").nth(1).expect("the enum");
        assert!(decl.trim_start().starts_with('}'), "no variants may be emitted:\n{decl}");
        assert!(g.source.contains("match *self {}"), "{}", g.source);
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
