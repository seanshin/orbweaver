//! Calling an operation nobody generated a stub for.
//!
//! `docs/PLAN.md` §4.6 projects an interface to an agent as three tools, and
//! the third is `invoke_operation`: a name and a bag of arguments, chosen at
//! runtime. Nothing was compiled for it. This module is what makes that call
//! possible — the registry says what the operation looks like, `super::encode`
//! turns values into bytes, and `Connection` carries them.
//!
//! This is CORBA's Dynamic Invocation Interface in the shape this project
//! needs. It is deliberately not a general DII: there is no `Request` object to
//! populate field by field and no deferred synchronous mode, because neither
//! serves the agent path and both are surface to get wrong.
//!
//! # What it refuses
//!
//! An unknown operation, a missing argument, an argument nobody declared, and
//! any value whose shape does not match its `TypeCode`. All four are mistakes
//! an agent will make, and all four produce a message naming the operation and
//! the argument rather than a marshalling error from somewhere deep in a
//! buffer. §3.3 calls diagnostics a product; for a caller that is guessing,
//! they are the product.

use std::collections::BTreeMap;

use orbweaver_cdr::Encoder;
use orbweaver_giop::{Connection, Error as GiopError};
use orbweaver_registry::{OperationSig, ParamDirection, Registry};

use orbweaver_giop::codeset::WideCodec;

use crate::{Error, Result, Value, decode_named_with, decode_with, encode_named_with};

/// What came back from a call.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// The return value; `Value::Struct(vec![])` stands for `void`.
    pub returns: Value,
    /// `out` and `inout` arguments, by name.
    ///
    /// Separate from `returns` because CORBA operations routinely answer
    /// through both, and flattening them into one value would lose which is
    /// which.
    pub outputs: BTreeMap<String, Value>,
}

/// A user exception the target raised, decoded into a value.
#[derive(Debug, Clone, PartialEq)]
pub struct UserException {
    /// The repository id the target sent.
    pub id: String,
    /// Its members, or `None` when the registry has never heard of it.
    ///
    /// An id we cannot decode is still worth reporting: it names a contract the
    /// caller was not built against, which is the useful half of the message.
    pub members: Option<Value>,
}

/// Why a dynamic call did not complete.
#[derive(Debug)]
#[allow(missing_docs)]
pub enum InvokeError {
    /// The arguments or the reply did not match the contract.
    Marshalling(Error),
    /// The target raised a declared exception.
    User(UserException),
    /// Anything the transport or the target's ORB reported.
    Transport(GiopError),
}

impl std::fmt::Display for InvokeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvokeError::Marshalling(e) => write!(f, "{e}"),
            InvokeError::User(u) => match &u.members {
                Some(v) => write!(f, "user exception {}: {v:?}", u.id),
                None => write!(f, "user exception {} (not in the registry)", u.id),
            },
            InvokeError::Transport(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for InvokeError {}

impl From<Error> for InvokeError {
    fn from(e: Error) -> Self {
        InvokeError::Marshalling(e)
    }
}

impl From<GiopError> for InvokeError {
    fn from(e: GiopError) -> Self {
        InvokeError::Transport(e)
    }
}

fn bad(message: impl Into<String>) -> InvokeError {
    InvokeError::Marshalling(Error { path: String::new(), message: message.into() })
}

/// Invokes `operation` on `interface_id` over `conn`.
///
/// `args` is keyed by parameter name and must contain exactly the `in` and
/// `inout` parameters — no more, and no fewer.
pub fn invoke(
    conn: &mut Connection,
    registry: &Registry,
    interface_id: &str,
    operation: &str,
    args: &BTreeMap<String, Value>,
) -> std::result::Result<Outcome, InvokeError> {
    let (_owner, sig) = registry.resolve_operation(interface_id, operation).ok_or_else(|| {
        bad(match registry.interface(interface_id) {
            Some(_) => format!(
                "{interface_id} has no operation {operation:?}{}",
                nearby(registry, interface_id, operation)
            ),
            None => format!("{interface_id} is not a registered interface"),
        })
    })?;

    check_arguments(operation, sig, args)?;

    // The codec comes from the connection, not from a constant. `wstring` is
    // the one part of CDR whose encoding depends on the GIOP version rather
    // than the TypeCode, and Phase 1 established that a peer will not infer
    // wide-char byte order from the message byte order. Defaulting to 1.2 here
    // would put a length in octets on a 1.1 connection that expects characters.
    let wide = wide_codec(conn);

    // Marshal once, before sending, so a bad argument is a local error instead
    // of a half-written message and a desynchronised connection. `invoke` takes
    // a closure it may call more than once (a LOCATION_FORWARD is retried), so
    // the bytes have to be reproducible anyway.
    let mut probe = Encoder::new(conn.endian());
    write_args(&mut probe, sig, args, wide)?;

    if sig.oneway {
        // A oneway has no reply to decode and no out parameters to fill; §5.3
        // treats changing one into a twoway as breaking for this reason.
        conn.invoke_oneway(operation, |e| {
            let _ = write_args(e, sig, args, wide);
        })?;
        return Ok(Outcome { returns: Value::Struct(Vec::new()), outputs: BTreeMap::new() });
    }

    let reply = match conn.invoke(operation, |e| {
        // Already validated above, so a failure here cannot happen; ignoring it
        // is safe only because of that, and only here.
        let _ = write_args(e, sig, args, wide);
    }) {
        Ok(reply) => reply,
        Err(GiopError::UserException { id, reply }) => {
            return Err(InvokeError::User(decode_user_exception(registry, &id, &reply, wide)));
        }
        Err(e) => return Err(e.into()),
    };

    let mut body = reply.body()?;
    // Order is fixed by §7.9.1: the return value, then the out and inout
    // parameters in the order they were declared. Not alphabetical, and not the
    // order the caller happened to supply them in.
    let returns = decode_with(&mut body, &sig.returns, wide)?;
    let mut outputs = BTreeMap::new();
    for p in &sig.params {
        if matches!(p.direction, ParamDirection::Out | ParamDirection::InOut) {
            outputs.insert(p.name.clone(), decode_named_with(&mut body, &p.tc, &p.name, wide)?);
        }
    }
    Ok(Outcome { returns, outputs })
}

/// Names the argument mistakes precisely, because a caller that is guessing
/// gets nothing from "marshalling failed".
fn check_arguments(
    operation: &str,
    sig: &OperationSig,
    args: &BTreeMap<String, Value>,
) -> std::result::Result<(), InvokeError> {
    let mut wanted: Vec<&str> = Vec::new();
    for p in &sig.params {
        if matches!(p.direction, ParamDirection::In | ParamDirection::InOut) {
            wanted.push(&p.name);
        }
    }
    let missing: Vec<&str> = wanted.iter().copied().filter(|n| !args.contains_key(*n)).collect();
    let extra: Vec<&str> =
        args.keys().map(String::as_str).filter(|n| !wanted.contains(n)).collect();

    if !missing.is_empty() || !extra.is_empty() {
        let mut parts = Vec::new();
        if !missing.is_empty() {
            parts.push(format!("missing {}", missing.join(", ")));
        }
        if !extra.is_empty() {
            // An out parameter passed as an argument is the likeliest cause and
            // the least obvious, so it is worth calling out by name.
            let outs: Vec<&str> = extra
                .iter()
                .copied()
                .filter(|n| {
                    sig.params
                        .iter()
                        .any(|p| p.name == *n && matches!(p.direction, ParamDirection::Out))
                })
                .collect();
            if outs.is_empty() {
                parts.push(format!("unknown {}", extra.join(", ")));
            } else {
                parts.push(format!(
                    "{} {} an out parameter and is not passed in",
                    outs.join(", "),
                    if outs.len() == 1 { "is" } else { "are" }
                ));
            }
        }
        return Err(bad(format!(
            "{operation} takes ({}); {}",
            wanted.join(", "),
            parts.join("; ")
        )));
    }
    Ok(())
}

fn write_args(
    e: &mut Encoder,
    sig: &OperationSig,
    args: &BTreeMap<String, Value>,
    wide: WideCodec,
) -> Result<()> {
    for p in &sig.params {
        if matches!(p.direction, ParamDirection::In | ParamDirection::InOut) {
            let v = args.get(&p.name).expect("checked by check_arguments");
            // Rooted at the parameter's name: "at key.tag[2]: string is
            // bounded at 8 but 9 were given" says which argument to fix, where
            // the value's own path alone would say "at tag[2]" — or, for a
            // bound the whole argument breaks, nothing at all.
            encode_named_with(e, &p.tc, v, &p.name, wide)?;
        }
    }
    Ok(())
}

/// The wide-character codec for this connection's negotiated version.
///
/// **This used to refuse on GIOP 1.0, and refusing broke every call on such a
/// connection** — `ping()` included, and `_is_a` with it. The comment that
/// stood here said so and then did it anyway: *"saying so at that point is more
/// useful than refusing every call on a 1.0 connection — but the invoker cannot
/// know yet, so it reports the version."* §9.3.1.6 forbids carrying `wchar` or
/// `wstring` **data**; it does not forbid a `long ping()`.
///
/// The servant side had the identical defect and fixed it before this one was
/// noticed — nineteen calls diverging at 1.0, found by
/// `orbweaver-gen/tests/python_servant.rs`. This side went on refusing because
/// **no cell in the binding grid could reach 1.0 to see it**: the grid read
/// `client: neither[1.0]` and the note beside it blamed the driver's wide text,
/// which was the symptom.
///
/// The answer is [`orbweaver_giop::codeset::wide_for_version`] and is not
/// restated here. Both sides call it, because a caller and a servant are not
/// free to disagree about what a 1.0 connection carries.
fn wide_codec(conn: &Connection) -> WideCodec {
    orbweaver_giop::codeset::wide_for_version(conn.version())
}

fn decode_user_exception(
    registry: &Registry,
    id: &str,
    reply: &orbweaver_giop::Reply,
    wide: WideCodec,
) -> UserException {
    let members = (|| {
        let tc = registry.typecode(id)?;
        let mut body = reply.body().ok()?;
        // The id was already read off the front of the body by decode_reply,
        // and the members follow it; re-reading from the start means skipping
        // it again rather than assuming a fresh decoder starts after it.
        body.get_string().ok()?;
        decode_with(&mut body, tc, wide).ok()
    })();
    UserException { id: id.to_owned(), members }
}

/// A hint when an operation name is wrong, which for a guessing caller it
/// frequently is. Case is the first thing to try: IDL forbids two names that
/// differ only in case, so a case-insensitive match is unambiguous.
fn nearby(registry: &Registry, interface_id: &str, operation: &str) -> String {
    let Some(iface) = registry.interface(interface_id) else { return String::new() };
    let mut names: Vec<&String> = iface.operations.keys().collect();
    if let Some(hit) = names.iter().find(|n| n.eq_ignore_ascii_case(operation)) {
        return format!("; did you mean {hit:?}?");
    }
    names.sort();
    if names.is_empty() {
        return String::new();
    }
    format!("; it has {}", names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_giop::codeset::CodeSetId;
    use orbweaver_registry::Registry;

    fn registry(src: &str) -> Registry {
        let spec = orbweaver_idl::parse(src).expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        r
    }

    const IDL: &str = "
        module m {
          struct Point { long px; long py; };
          exception Refused { string why; };
          interface Calc {
            long add(in long a, in long b);
            void split(in Point p, out long x, out long y);
            long risky(in long n) raises (Refused);
          };
        };";

    /// The argument checks run before anything touches a connection, so they
    /// are testable without a peer — which is the point of doing them there.
    fn args_error(operation: &str, args: &[(&str, Value)]) -> String {
        let r = registry(IDL);
        let (_, sig) = r.resolve_operation("IDL:m/Calc:1.0", operation).expect("operation");
        let map: BTreeMap<String, Value> =
            args.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect();
        match check_arguments(operation, sig, &map) {
            Err(e) => e.to_string(),
            Ok(()) => panic!("expected {operation} to reject these arguments"),
        }
    }

    #[test]
    fn a_missing_argument_names_it_and_the_full_signature() {
        let msg = args_error("add", &[("a", Value::Long(1))]);
        assert!(msg.contains("missing b"), "{msg}");
        assert!(msg.contains("add takes (a, b)"), "{msg}");
    }

    #[test]
    fn an_undeclared_argument_is_refused() {
        let msg = args_error(
            "add",
            &[("a", Value::Long(1)), ("b", Value::Long(2)), ("c", Value::Long(3))],
        );
        assert!(msg.contains("unknown c"), "{msg}");
    }

    /// Passing an `out` parameter as an argument is the mistake a caller
    /// reading a signature is most likely to make, and "unknown x" would not
    /// explain it.
    #[test]
    fn an_out_parameter_supplied_as_an_argument_says_why_it_is_wrong() {
        let msg = args_error(
            "split",
            &[("p", Value::Struct(vec![])), ("x", Value::Long(0)), ("y", Value::Long(0))],
        );
        assert!(msg.contains("out parameter"), "{msg}");
        assert!(msg.contains("x, y are"), "{msg}");
    }

    #[test]
    fn correct_arguments_pass_the_check() {
        let r = registry(IDL);
        let (_, sig) = r.resolve_operation("IDL:m/Calc:1.0", "add").unwrap();
        let args =
            BTreeMap::from([("a".to_owned(), Value::Long(1)), ("b".to_owned(), Value::Long(2))]);
        check_arguments("add", sig, &args).expect("should be accepted");
    }

    /// Arguments go on the wire in declaration order, never in the order the
    /// caller supplied them. `BTreeMap` sorts by name, so a signature whose
    /// declaration order differs from alphabetical order is the case that
    /// catches a marshaller reading the map instead of the signature.
    #[test]
    fn arguments_are_marshalled_in_declaration_order_not_alphabetical() {
        let r = registry("module m { interface I { void f(in long zebra, in long apple); }; };");
        let (_, sig) = r.resolve_operation("IDL:m/I:1.0", "f").unwrap();
        let args = BTreeMap::from([
            ("apple".to_owned(), Value::Long(2)),
            ("zebra".to_owned(), Value::Long(1)),
        ]);
        let mut e = Encoder::new(orbweaver_cdr::Endian::Big);
        let wide = WideCodec::new(orbweaver_giop::Version::V1_2, CodeSetId::UTF_16).unwrap();
        write_args(&mut e, sig, &args, wide).expect("encodes");
        assert_eq!(
            e.finish().unwrap(),
            vec![0, 0, 0, 1, 0, 0, 0, 2],
            "zebra is declared first and must travel first"
        );
    }

    /// The marshalling sentence must say *which* argument does not fit. A
    /// caller sending nine characters into a `string<8>` in the second
    /// position was told "string is bounded at 8 but 9 were given" and left to
    /// guess between the two; the dry-run prediction in `orbweaver-mcp` had to
    /// prepend the name itself. Both byte orders, because the encoder is
    /// entered per order and a path kept on one side only would pass here.
    #[test]
    fn a_marshalling_error_names_the_argument_and_the_path_inside_it() {
        let r = registry(
            "module m {
               struct Tagged { sequence<string<8> > tag; };
               interface I {
                 void f(in string<8> a, in string<8> key);
                 void g(in long a, in Tagged key);
               };
             };",
        );
        let wide = WideCodec::new(orbweaver_giop::Version::V1_2, CodeSetId::UTF_16).unwrap();
        for endian in [orbweaver_cdr::Endian::Big, orbweaver_cdr::Endian::Little] {
            let (_, sig) = r.resolve_operation("IDL:m/I:1.0", "f").unwrap();
            let args = BTreeMap::from([
                ("a".to_owned(), Value::String("fits".into())),
                ("key".to_owned(), Value::String("nine char".into())),
            ]);
            let mut e = Encoder::new(endian);
            let msg = write_args(&mut e, sig, &args, wide).expect_err("9 > 8").to_string();
            assert_eq!(msg, "at key: string is bounded at 8 but 9 were given", "{endian:?}");

            let (_, sig) = r.resolve_operation("IDL:m/I:1.0", "g").unwrap();
            let tagged = Value::Struct(vec![(
                "tag".to_owned(),
                Value::List(vec![
                    Value::String("ok".into()),
                    Value::String("ok".into()),
                    Value::String("nine char".into()),
                ]),
            )]);
            let args =
                BTreeMap::from([("a".to_owned(), Value::Long(1)), ("key".to_owned(), tagged)]);
            let mut e = Encoder::new(endian);
            let msg = write_args(&mut e, sig, &args, wide).expect_err("9 > 8").to_string();
            assert_eq!(msg, "at key.tag[2]: string is bounded at 8 but 9 were given", "{endian:?}");
        }
    }

    #[test]
    fn an_unknown_operation_suggests_one_that_differs_only_in_case() {
        let r = registry(IDL);
        let hint = nearby(&r, "IDL:m/Calc:1.0", "Add");
        assert!(hint.contains("did you mean \"add\""), "{hint}");
    }

    #[test]
    fn an_unknown_operation_otherwise_lists_what_there_is() {
        let r = registry(IDL);
        let hint = nearby(&r, "IDL:m/Calc:1.0", "multiply");
        assert!(hint.contains("add"), "{hint}");
        assert!(hint.contains("risky"), "{hint}");
    }

    /// Inherited operations must be callable, or a dynamic invoker is less
    /// capable than a generated stub.
    #[test]
    fn an_operation_inherited_from_a_base_resolves() {
        let r =
            registry("module m { interface Base { long ping(); }; interface Derived : Base {}; };");
        assert!(r.resolve_operation("IDL:m/Derived:1.0", "ping").is_some());
    }
}
