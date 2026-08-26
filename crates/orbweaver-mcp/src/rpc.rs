//! JSON-RPC 2.0 framing, and the MCP methods on top of it.
//!
//! Everything below this module decides *what* an agent may do. This module is
//! only about getting the question in and the answer out, and it is short on
//! purpose: a transport that makes decisions is a second policy nobody
//! remembers to audit.
//!
//! # The rule that governs stdio
//!
//! **stdout is the protocol.** One JSON object per line, and nothing else may
//! ever be written there — a stray `println!`, a progress bar, a panic message
//! and the session is desynchronised in a way the client reports as malformed
//! JSON rather than as the bug it is. Diagnostics go to stderr. This is stated
//! here because it is the mistake, not a mistake.
//!
//! # Which revision
//!
//! The shapes here follow MCP's `2024-11-05` revision: `initialize`,
//! `tools/list`, `tools/call`, and the `notifications/initialized` that follows
//! the handshake. The protocol version is echoed from the client when it asks
//! for one this server understands and pinned otherwise, so a newer client gets
//! an explicit answer rather than silence.
//!
//! It has **not** been driven by a real MCP client in this repository — the
//! harness drives it over a pipe with hand-written frames. That is a genuine
//! gap and it is stated rather than left to be discovered.

use orbweaver_dynamic::json::Json;

/// The MCP revision these shapes follow.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// The longest line the server will read.
///
/// stdin is untrusted at this boundary like everything else, and a reader with
/// no bound turns one long line into an allocation of that size. 8 MiB is far
/// past any real `tools/call` and far short of a problem.
pub const MAX_LINE: usize = 8 * 1024 * 1024;

/// JSON-RPC 2.0 error codes; the negative range is reserved by the
/// specification and these are its defined values.
pub mod codes {
    /// The line was not JSON.
    pub const PARSE_ERROR: i32 = -32700;
    /// It was JSON, but not a request.
    pub const INVALID_REQUEST: i32 = -32600;
    /// No such method.
    pub const METHOD_NOT_FOUND: i32 = -32601;
    /// The method exists and the params are wrong.
    pub const INVALID_PARAMS: i32 = -32602;
    /// The server broke.
    #[allow(dead_code)]
    pub const INTERNAL_ERROR: i32 = -32603;
}

/// One parsed request.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// Absent for a notification, which takes no reply.
    pub id: Option<Json>,
    /// The method name.
    pub method: String,
    /// The params object, or `Json::Null`.
    pub params: Json,
}

fn obj(pairs: impl IntoIterator<Item = (&'static str, Json)>) -> Json {
    Json::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn s(text: impl Into<String>) -> Json {
    Json::String(text.into())
}

/// Builds a success response.
pub fn result(id: Json, value: Json) -> Json {
    obj([("jsonrpc", s("2.0")), ("id", id), ("result", value)])
}

/// Builds an error response.
pub fn error(id: Json, code: i32, message: impl Into<String>) -> Json {
    obj([
        ("jsonrpc", s("2.0")),
        ("id", id),
        ("error", obj([("code", Json::Number(code.to_string())), ("message", s(message))])),
    ])
}

/// Parses one line into a request.
///
/// Returns an error *response* rather than an error, because a malformed
/// request still gets a well-formed answer — that is the difference between a
/// protocol violation the client can report and a hang it cannot.
pub fn parse_request(line: &str) -> Result<Request, Json> {
    let doc = Json::parse(line)
        .map_err(|e| error(Json::Null, codes::PARSE_ERROR, format!("invalid JSON: {e}")))?;

    // The id is recovered before validating anything else, so an error can be
    // correlated with the request that caused it. A response with a null id is
    // one the client can only log.
    let id = doc.get("id").cloned();

    if doc.get("jsonrpc").and_then(Json::as_str) != Some("2.0") {
        return Err(error(
            id.unwrap_or(Json::Null),
            codes::INVALID_REQUEST,
            "every message must carry \"jsonrpc\": \"2.0\"",
        ));
    }
    let Some(method) = doc.get("method").and_then(Json::as_str) else {
        return Err(error(
            id.unwrap_or(Json::Null),
            codes::INVALID_REQUEST,
            "a request needs a string \"method\"",
        ));
    };
    Ok(Request {
        id,
        method: method.to_owned(),
        params: doc.get("params").cloned().unwrap_or(Json::Null),
    })
}

/// The seven tools, as MCP describes them to a client.
///
/// The descriptions are what an agent reads before choosing, so they say what
/// the tool is *for* rather than what it does mechanically — and
/// `invoke_operation` says plainly that a reference is a handle, because an
/// agent that expects to pass an address will otherwise try.
///
/// # Two groups, and the line between them
///
/// The first three are the **estate** triad: find a contract, read it, call it.
/// Every one of them names something that exists in the catalog.
///
/// The last four are D024 §5's **contract** tools: validate IDL, diff it,
/// describe a type, preview what would be generated. Three of them take IDL
/// text the agent wrote and name nothing in the catalog at all;
/// `describe_type` is the one that reads the catalog, and it is gated twice for
/// that reason (see [`crate::Bridge::describe_type`]).
///
/// **Every one of them returns findings rather than a verdict** — D024 §3, and
/// the reason is stated there: the caller is a generator that will quote the
/// answer back, so `{"ok": false}` throws away the position and the fix hint at
/// the boundary where they matter most. The descriptions say so, because an
/// agent that expects a boolean will write code that reads one.
///
/// **Registration is deliberately not among them** (D024 §5): an agent that can
/// register a contract can change what other agents see, and that is
/// `exposure`'s decision.
pub fn tool_definitions() -> Json {
    let string_prop = |desc: &str| obj([("type", s("string")), ("description", s(desc))]);

    Json::Array(vec![
        obj([
            ("name", s("search_interfaces")),
            (
                "description",
                s("Find CORBA interfaces in the catalog by name, operation name, or what the \
                   contract says they do. Returns ids to pass to describe_interface. Only \
                   interfaces an operator has exposed are visible."),
            ),
            (
                "inputSchema",
                obj([
                    ("type", s("object")),
                    (
                        "properties",
                        obj([
                            (
                                "query",
                                string_prop("Words to match; empty lists everything exposed"),
                            ),
                            (
                                "limit",
                                obj([
                                    ("type", s("integer")),
                                    ("description", s("Maximum results, default 20")),
                                ]),
                            ),
                        ]),
                    ),
                    ("required", Json::Array(vec![s("query")])),
                ]),
            ),
        ]),
        obj([
            ("name", s("describe_interface")),
            (
                "description",
                s("The full contract for one interface: operations, parameter names and types, \
                   exceptions, and the semantic annotations its author wrote. Operations you \
                   are not permitted to call are omitted."),
            ),
            (
                "inputSchema",
                obj([
                    ("type", s("object")),
                    (
                        "properties",
                        obj([("id", string_prop("Repository id, e.g. IDL:bank/Account:1.0"))]),
                    ),
                    ("required", Json::Array(vec![s("id")])),
                ]),
            ),
        ]),
        obj([
            ("name", s("invoke_operation")),
            (
                "description",
                s("Call an operation on an object. `handle` is a capability handle obtained \
                   from a previous result — not an address, and not something to construct. \
                   Arguments are a JSON object keyed by parameter name; 64-bit integers are \
                   strings and binary is base64. Operations marked destructive need approval."),
            ),
            (
                "inputSchema",
                obj([
                    ("type", s("object")),
                    (
                        "properties",
                        obj([
                            ("handle", string_prop("A capability handle from an earlier result")),
                            ("operation", string_prop("Operation name, exactly as declared")),
                            (
                                "arguments",
                                obj([
                                    ("type", s("object")),
                                    ("description", s("in and inout parameters, keyed by name")),
                                ]),
                            ),
                        ]),
                    ),
                    ("required", Json::Array(vec![s("handle"), s("operation")])),
                ]),
            ),
        ]),
        // ---- D024 §5, the contract tools. --------------------------------
        // Every description below says "findings" and none says "valid" or
        // "ok", because an agent that expects a boolean writes code that reads
        // one and then has nothing to repair from.
        obj([
            ("name", s("validate_contract")),
            (
                "description",
                s("Check IDL you have written. Returns findings — each with a rule, a line and \
                   column, and a concrete fix where the rule admits one — plus a repair_prompt \
                   that states every cause once. Never a bare verdict. Constructs this wire \
                   cannot carry are errors here, and say whether that is not-yet or never."),
            ),
            (
                "inputSchema",
                obj([
                    ("type", s("object")),
                    ("properties", obj([("source", string_prop("The IDL text to check"))])),
                    ("required", Json::Array(vec![s("source")])),
                ]),
            ),
        ]),
        obj([
            ("name", s("diff_contract")),
            (
                "description",
                s("Compare a proposed contract against a released one. Returns every change with \
                   the verdict it carries — compatible, server-first, conditionally breaking or \
                   BREAKING — and the reason for that verdict, plus the same findings and \
                   repair_prompt validate_contract returns. Both sides are IDL text."),
            ),
            (
                "inputSchema",
                obj([
                    ("type", s("object")),
                    (
                        "properties",
                        obj([
                            ("released", string_prop("The IDL of the contract already released")),
                            ("proposed", string_prop("The IDL of the contract being proposed")),
                        ]),
                    ),
                    ("required", Json::Array(vec![s("released"), s("proposed")])),
                ]),
            ),
        ]),
        obj([
            ("name", s("describe_type")),
            (
                "description",
                s("What describe_interface does for an interface, for a type. Returns the type's \
                   name, the module it is defined in, and its shape — the members of a struct, \
                   the cases of a union, the enumerators of an enum, what a typedef aliases. Use \
                   it when a member's type is a name you cannot look up. Only types an exposed \
                   interface actually reaches are visible."),
            ),
            (
                "inputSchema",
                obj([
                    ("type", s("object")),
                    (
                        "properties",
                        obj([("id", string_prop("Repository id, e.g. IDL:bank/Money:1.0"))]),
                    ),
                    ("required", Json::Array(vec![s("id")])),
                ]),
            ),
        ]),
        obj([
            ("name", s("preview_generation")),
            (
                "description",
                s("What would be generated for a contract, for both targets, and — the half that \
                   matters — what would be skipped and why. A construct this wire cannot carry is \
                   skipped silently at generation time, so check here before assuming a type you \
                   declared will exist in the client."),
            ),
            (
                "inputSchema",
                obj([
                    ("type", s("object")),
                    ("properties", obj([("source", string_prop("The IDL text to preview"))])),
                    ("required", Json::Array(vec![s("source")])),
                ]),
            ),
        ]),
    ])
}

/// The `initialize` result.
pub fn initialize_result(client_version: Option<&str>, server_name: &str) -> Json {
    obj([
        // Echoing a version we understand, and pinning ours otherwise, so a
        // newer client is told plainly instead of guessing from silence.
        (
            "protocolVersion",
            s(match client_version {
                Some(v) if v == PROTOCOL_VERSION => v,
                _ => PROTOCOL_VERSION,
            }),
        ),
        ("capabilities", obj([("tools", obj([]))])),
        ("serverInfo", obj([("name", s(server_name)), ("version", s(env!("CARGO_PKG_VERSION")))])),
    ])
}

/// Wraps a value as MCP tool content.
pub fn tool_content(value: &Json) -> Json {
    obj([
        ("content", Json::Array(vec![obj([("type", s("text")), ("text", s(value.to_string()))])])),
        ("isError", Json::Bool(false)),
    ])
}

/// Wraps a refusal as MCP tool content.
///
/// A refused call is a **successful** JSON-RPC response carrying `isError`, not
/// a protocol error. The distinction matters: a protocol error says the request
/// was malformed and invites a retry of the same thing, while this says the
/// request was understood and denied, which is what the agent needs to act on.
pub fn tool_error(message: impl Into<String>) -> Json {
    obj([
        ("content", Json::Array(vec![obj([("type", s("text")), ("text", s(message))])])),
        ("isError", Json::Bool(true)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_well_formed_request_parses() {
        let r = parse_request(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
        assert_eq!(r.method, "tools/list");
        assert_eq!(r.id, Some(Json::Number("1".into())));
        assert_eq!(r.params, Json::Null);
    }

    /// A notification takes no reply, and the distinction is the presence of an
    /// id. Answering one desynchronises a client that is not expecting it.
    #[test]
    fn a_notification_has_no_id() {
        let r = parse_request(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap();
        assert_eq!(r.id, None);
    }

    /// An error the client cannot correlate is an error it can only log.
    #[test]
    fn a_malformed_request_still_answers_with_its_id() {
        let e = parse_request(r#"{"jsonrpc":"2.0","id":7}"#).unwrap_err();
        assert_eq!(e.get("id"), Some(&Json::Number("7".into())));
        assert_eq!(
            e.get("error").and_then(|x| x.get("code")),
            Some(&Json::Number(codes::INVALID_REQUEST.to_string()))
        );
    }

    #[test]
    fn broken_json_is_a_parse_error_and_not_a_panic() {
        let e = parse_request("{not json").unwrap_err();
        assert_eq!(
            e.get("error").and_then(|x| x.get("code")),
            Some(&Json::Number(codes::PARSE_ERROR.to_string()))
        );
        assert_eq!(e.get("id"), Some(&Json::Null));
    }

    #[test]
    fn the_wrong_jsonrpc_version_is_refused() {
        assert!(parse_request(r#"{"jsonrpc":"1.0","id":1,"method":"x"}"#).is_err());
        assert!(parse_request(r#"{"id":1,"method":"x"}"#).is_err());
    }

    /// Every frame must be one line, because the stdio transport is
    /// line-delimited and a newline inside a message ends it early.
    #[test]
    fn every_response_is_a_single_line() {
        let messages = [
            result(Json::Number("1".into()), tool_definitions()),
            error(Json::Null, codes::PARSE_ERROR, "bad\nnews\twith\rcontrol characters"),
            result(Json::Number("2".into()), tool_content(&Json::String("multi\nline".into()))),
            result(Json::Number("3".into()), initialize_result(None, "orbweaver")),
        ];
        for m in messages {
            let text = m.to_string();
            assert!(!text.contains('\n'), "a frame contains a newline: {text}");
            assert!(!text.contains('\r'), "a frame contains a carriage return: {text}");
            assert_eq!(Json::parse(&text).unwrap(), m, "and it must re-parse");
        }
    }

    /// **The pin.** It asserted the triad until 2026-08-26 and now asserts the
    /// triad *followed by* D024 §5's four, in order, and nothing else.
    ///
    /// The order is part of the assertion, not an accident of writing: an agent
    /// reads this list top to bottom before choosing, and the estate tools come
    /// first because finding a contract is what most sessions do.
    ///
    /// The negative half is the one that earns its place — **`register_contract`
    /// is named here as a tool that must never appear.** D024 §5 excludes
    /// registration because an agent that can register a contract can change
    /// what other agents see, which is `exposure`'s decision; a list that only
    /// checked what it contained would let a fifth tool arrive without anybody
    /// re-reading that paragraph.
    #[test]
    fn the_tool_list_advertises_the_triad_and_the_four_contract_tools() {
        let Json::Array(tools) = tool_definitions() else { panic!() };
        let names: Vec<&str> = tools.iter().filter_map(|t| t.get("name")?.as_str()).collect();
        assert_eq!(
            names,
            [
                "search_interfaces",
                "describe_interface",
                "invoke_operation",
                "validate_contract",
                "diff_contract",
                "describe_type",
                "preview_generation",
            ]
        );
        for t in &tools {
            assert!(t.get("description").is_some(), "{t}");
            assert!(t.get("inputSchema").is_some(), "{t}");
        }
        for forbidden in ["register_contract", "register", "compile_idl"] {
            assert!(
                !names.contains(&forbidden),
                "{forbidden} is deliberately not a tool; see D024 §5 before adding it"
            );
        }
    }

    /// The four contract tools are exactly the four the gate has a contract
    /// for, computed from that contract rather than retyped here — so a tool
    /// advertised with no operation behind it (which would be refused at every
    /// call by `OperationNotExposed`, confusingly) cannot ship.
    #[test]
    fn every_advertised_contract_tool_is_declared_by_the_gates_own_contract() {
        let Json::Array(tools) = tool_definitions() else { panic!() };
        let names: Vec<&str> = tools.iter().filter_map(|t| t.get("name")?.as_str()).collect();
        let registry = crate::contract::contract_tools_registry();
        for tool in crate::contract::CONTRACT_TOOLS {
            assert!(names.contains(&tool), "{tool} is declared and not advertised");
            assert!(
                registry.resolve_operation(crate::contract::CONTRACT_TOOLS_ID, tool).is_some(),
                "{tool} is advertised and the tool contract does not declare it"
            );
        }
    }

    /// D024 §3: the answer is findings, and the description has to say so
    /// before the agent calls rather than after.
    #[test]
    fn every_contract_tool_promises_findings_rather_than_a_verdict() {
        let Json::Array(tools) = tool_definitions() else { panic!() };
        for tool in crate::contract::CONTRACT_TOOLS {
            let t = tools
                .iter()
                .find(|t| t.get("name").and_then(Json::as_str) == Some(tool))
                .unwrap_or_else(|| panic!("{tool} is advertised"));
            let desc = t.get("description").and_then(Json::as_str).unwrap_or("");
            assert!(
                desc.contains("Returns") || desc.contains("what would be"),
                "{tool} must say what it returns: {desc}"
            );
        }
    }

    /// An agent that thinks a handle is an address will try to build one.
    #[test]
    fn the_invoke_tool_says_a_handle_is_not_an_address() {
        let Json::Array(tools) = tool_definitions() else { panic!() };
        let invoke = tools
            .iter()
            .find(|t| t.get("name").and_then(Json::as_str) == Some("invoke_operation"))
            .unwrap();
        let desc = invoke.get("description").and_then(Json::as_str).unwrap();
        assert!(desc.contains("not an address"), "{desc}");
        assert!(desc.contains("base64"), "the precision rules must be discoverable: {desc}");
    }

    /// A refusal is a successful response carrying isError, not a JSON-RPC
    /// error: the request was understood, and a protocol error would invite a
    /// retry of the same thing.
    #[test]
    fn a_refusal_is_content_rather_than_a_protocol_error() {
        let c = tool_error("not exposed");
        assert_eq!(c.get("isError"), Some(&Json::Bool(true)));
        assert!(c.get("content").is_some());
    }
}
