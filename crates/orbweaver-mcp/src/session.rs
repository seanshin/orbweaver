//! Dispatch: one JSON-RPC request in, one response out.
//!
//! Separated from the stdio loop so the whole protocol can be driven by tests
//! without a process, a pipe or a timeout. The loop that owns stdin and stdout
//! is thirty lines and has nothing in it worth testing; this has all of it.

use orbweaver_dynamic::json::Json;
use orbweaver_giop::Connection;
use orbweaver_registry::Registry;

use crate::policy::{Approval, Exposure};
use crate::rpc::{self, codes};
use crate::{Bridge, ToolError};

/// Default result cap for `search_interfaces`, for a deployment that has not
/// said.
///
/// The agent may ask for a different one per request; this is what it gets when
/// it asks for none. How many results are useful is a fact about the size of
/// the estate and the agent's context window, both of which a deployment knows
/// and this crate does not — so it is settable
/// ([`Session::set_search_limit`]) and stated here once.
pub const DEFAULT_SEARCH_LIMIT: usize = 20;

/// An MCP session.
///
/// The connection is optional, and not for tidiness: the handshake, the tool
/// list, `search` and `describe` are all answerable from the catalog alone, and
/// a session that cannot be built without a live target is a session whose
/// dispatch can only be tested by re-implementing it. The first version of this
/// file did exactly that, and the test proved that two copies of the logic
/// agreed with each other.
pub struct Session<'a> {
    bridge: Bridge<'a>,
    conn: Option<Connection>,
    /// Set once `initialize` has been answered.
    ready: bool,
    /// What `search_interfaces` caps at when the request names no `limit`.
    search_limit: usize,
}

impl<'a> Session<'a> {
    /// A session over one connection to one target.
    pub fn new(
        registry: &'a Registry,
        exposure: Exposure,
        conn: Connection,
        session_id: impl Into<String>,
    ) -> Self {
        Self::catalog_only(registry, exposure, session_id).with_target(conn)
    }

    /// A session that can search and describe but not call.
    pub fn catalog_only(
        registry: &'a Registry,
        exposure: Exposure,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            bridge: Bridge::new(registry, exposure, session_id),
            conn: None,
            ready: false,
            search_limit: DEFAULT_SEARCH_LIMIT,
        }
    }

    /// Overrides the result cap a `search_interfaces` that names none gets.
    ///
    /// A deployment's number, not the crate's: see [`DEFAULT_SEARCH_LIMIT`].
    pub fn set_search_limit(&mut self, limit: usize) {
        self.search_limit = limit;
    }

    /// The cap in force for a request that names none.
    pub fn search_limit(&self) -> usize {
        self.search_limit
    }

    /// Attaches a target.
    pub fn with_target(mut self, conn: Connection) -> Self {
        self.conn = Some(conn);
        self
    }

    /// Attaches the caller the host authenticated, so the session's audit
    /// lines and `ai_authz` checks are about somebody.
    ///
    /// From the host, never from the agent's own request — the same rule
    /// [`Bridge::on_behalf_of`] states, and the reason this is not a
    /// JSON-RPC parameter.
    pub fn on_behalf_of(mut self, caller: crate::identity::Caller) -> Self {
        self.bridge.set_caller(caller);
        self
    }

    /// The bridge, for a host that wants to seed the capability table.
    pub fn bridge(&mut self) -> &mut Bridge<'a> {
        &mut self.bridge
    }

    /// Handles one line. Returns the line to write back, or `None` for a
    /// notification.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        if line.trim().is_empty() {
            return None;
        }
        if line.len() > rpc::MAX_LINE {
            return Some(
                rpc::error(
                    Json::Null,
                    codes::INVALID_REQUEST,
                    format!("a frame may not exceed {} bytes", rpc::MAX_LINE),
                )
                .to_string(),
            );
        }
        let request = match rpc::parse_request(line) {
            Ok(r) => r,
            Err(response) => return Some(response.to_string()),
        };
        let Some(id) = request.id.clone() else {
            // Notifications are acted on where they matter and never answered.
            self.notify(&request.method);
            return None;
        };
        Some(self.dispatch(id, &request.method, &request.params).to_string())
    }

    fn notify(&mut self, method: &str) {
        if method == "notifications/initialized" {
            self.ready = true;
        }
    }

    fn dispatch(&mut self, id: Json, method: &str, params: &Json) -> Json {
        match method {
            "initialize" => {
                self.ready = true;
                rpc::result(
                    id,
                    rpc::initialize_result(
                        params.get("protocolVersion").and_then(Json::as_str),
                        "orbweaver",
                    ),
                )
            }
            "ping" => rpc::result(id, Json::Object(Default::default())),
            "tools/list" => {
                // Answering before initialize would let a client skip the
                // handshake and then be surprised by a capability it never
                // negotiated.
                if !self.ready {
                    return rpc::error(id, codes::INVALID_REQUEST, "initialize first");
                }
                rpc::result(
                    id,
                    Json::Object(std::collections::BTreeMap::from([(
                        "tools".to_owned(),
                        rpc::tool_definitions(),
                    )])),
                )
            }
            "tools/call" => {
                if !self.ready {
                    return rpc::error(id, codes::INVALID_REQUEST, "initialize first");
                }
                self.call_tool(id, params)
            }
            other => rpc::error(id, codes::METHOD_NOT_FOUND, format!("no method {other:?}")),
        }
    }

    fn call_tool(&mut self, id: Json, params: &Json) -> Json {
        let Some(name) = params.get("name").and_then(Json::as_str) else {
            return rpc::error(id, codes::INVALID_PARAMS, "tools/call needs a \"name\"");
        };
        let args = params.get("arguments").cloned().unwrap_or(Json::Object(Default::default()));

        match name {
            "search_interfaces" => {
                let query = args.get("query").and_then(Json::as_str).unwrap_or("");
                let limit = args
                    .get("limit")
                    .and_then(|j| match j {
                        Json::Number(n) => n.parse::<usize>().ok(),
                        _ => None,
                    })
                    .unwrap_or(self.search_limit);
                rpc::result(id, rpc::tool_content(&self.bridge.search(query, limit)))
            }
            "describe_interface" => {
                let Some(target) = args.get("id").and_then(Json::as_str) else {
                    return rpc::result(id, rpc::tool_error("describe_interface needs an \"id\""));
                };
                match self.bridge.describe(target) {
                    Ok(d) => rpc::result(id, rpc::tool_content(&d)),
                    Err(e) => rpc::result(id, rpc::tool_error(e.to_string())),
                }
            }
            "invoke_operation" => {
                let (Some(handle), Some(op)) = (
                    args.get("handle").and_then(Json::as_str),
                    args.get("operation").and_then(Json::as_str),
                ) else {
                    return rpc::result(
                        id,
                        rpc::tool_error("invoke_operation needs a \"handle\" and an \"operation\""),
                    );
                };
                let call_args =
                    args.get("arguments").cloned().unwrap_or(Json::Object(Default::default()));
                // Approval never comes from the agent's own request: a caller
                // that can assert its own approval has no approval gate. It is
                // a host decision, and the host does not have a channel for it
                // yet, so the answer here is always "not approved".
                let approval = Approval::default();
                // The handle and the policy are checked before the connection
                // is looked at, so a forged handle gets the same answer whether
                // or not a target is attached. With a target the check runs
                // inside `Bridge::invoke` — the same check, and the point that
                // now writes the audit line; without one it runs here so the
                // refusal still answers first.
                let Some(conn) = self.conn.as_mut() else {
                    return match self.bridge.check(handle, op, approval) {
                        Err(e) => rpc::result(id, rpc::tool_error(e.to_string())),
                        Ok(_) => {
                            rpc::result(id, rpc::tool_error("this session has no target to call"))
                        }
                    };
                };
                match self.bridge.invoke(conn, handle, op, &call_args, approval) {
                    Ok(v) => rpc::result(id, rpc::tool_content(&v)),
                    Err(ToolError::Invoke(e)) => {
                        rpc::result(id, rpc::tool_error(format!("the target refused: {e}")))
                    }
                    Err(e) => rpc::result(id, rpc::tool_error(e.to_string())),
                }
            }
            // ---- D024 §5, the contract tools. ----------------------------
            //
            // Each is a missing-argument check and then one `Bridge` call.
            // The gate is *inside* that call and not here, deliberately: a
            // transport that decided anything would be a second policy nobody
            // remembers to audit, which is the rule this module opens with.
            //
            // A missing argument answers before the gate runs — the same order
            // `invoke_operation` uses, and for the same reason: a request that
            // does not say what it wants has not asked a question the policy
            // can have an opinion about.
            "validate_contract" => {
                let Some(source) = args.get("source").and_then(Json::as_str) else {
                    return rpc::result(
                        id,
                        rpc::tool_error("validate_contract needs a \"source\""),
                    );
                };
                Self::answer(id, self.bridge.validate_contract(source, &args))
            }
            "diff_contract" => {
                let (Some(released), Some(proposed)) = (
                    args.get("released").and_then(Json::as_str),
                    args.get("proposed").and_then(Json::as_str),
                ) else {
                    return rpc::result(
                        id,
                        rpc::tool_error("diff_contract needs a \"released\" and a \"proposed\""),
                    );
                };
                Self::answer(id, self.bridge.diff_contract(released, proposed, &args))
            }
            "describe_type" => {
                let Some(target) = args.get("id").and_then(Json::as_str) else {
                    return rpc::result(id, rpc::tool_error("describe_type needs an \"id\""));
                };
                Self::answer(id, self.bridge.describe_type(target, &args))
            }
            "preview_generation" => {
                let Some(source) = args.get("source").and_then(Json::as_str) else {
                    return rpc::result(
                        id,
                        rpc::tool_error("preview_generation needs a \"source\""),
                    );
                };
                Self::answer(id, self.bridge.preview_generation(source, &args))
            }
            other => rpc::result(id, rpc::tool_error(format!("no tool named {other:?}"))),
        }
    }

    /// One rendering for every contract tool's answer.
    ///
    /// A refusal is content carrying `isError`, never a JSON-RPC error, for the
    /// reason [`rpc::tool_error`] states: the request was understood and
    /// denied, and a protocol error would invite a retry of the same thing.
    /// `Denied`'s `Display` is handed over verbatim, so the refusal an agent
    /// reads here is the fact *and* the remedy, the same as for a call.
    fn answer(id: Json, result: Result<Json, crate::policy::Denied>) -> Json {
        match result {
            Ok(v) => rpc::result(id, rpc::tool_content(&v)),
            Err(e) => rpc::result(id, rpc::tool_error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Registry {
        let spec = orbweaver_idl::parse(
            "module bank {
               //@ ai_desc: A customer deposit account
               interface Account {
                 //@ ai_effect: read_only
                 long long balance();
               };
             };",
        )
        .expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        r
    }

    /// Drives the real `Session`, not a re-implementation of it.
    fn run(exposure: Exposure, lines: &[&str]) -> Vec<Option<String>> {
        let reg = registry();
        // The registry has to outlive the session, so it is leaked for the
        // duration of the test rather than threaded through a lifetime that
        // exists only to satisfy this helper.
        let reg: &'static Registry = Box::leak(Box::new(reg));
        let mut session = Session::catalog_only(reg, exposure, "test");
        lines.iter().map(|l| session.handle_line(l)).collect()
    }

    fn parsed(line: &Option<String>) -> Json {
        Json::parse(line.as_ref().expect("a response")).expect("valid JSON")
    }

    #[test]
    fn the_handshake_answers_and_the_notification_does_not() {
        let out = run(
            Exposure::nothing(),
            &[
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            ],
        );
        assert_eq!(
            parsed(&out[0]).get("result").and_then(|r| r.get("protocolVersion")),
            Some(&Json::String(rpc::PROTOCOL_VERSION.into()))
        );
        assert_eq!(out[1], None, "a notification must not be answered");
        let Some(Json::Array(tools)) =
            parsed(&out[2]).get("result").and_then(|r| r.get("tools")).cloned()
        else {
            panic!("{:?}", out[2])
        };
        // The triad plus D024 §5's four contract tools. The list itself is
        // pinned by name and order in `rpc::tests`; this asserts only that the
        // handshake hands over the same number the definitions carry, computed
        // rather than retyped.
        let Json::Array(defined) = rpc::tool_definitions() else { panic!() };
        assert_eq!(tools.len(), defined.len());
        assert_eq!(tools.len(), 7);
    }

    #[test]
    fn tools_are_not_listed_before_the_handshake() {
        let out = run(Exposure::nothing(), &[r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#]);
        assert!(parsed(&out[0]).get("error").is_some(), "{:?}", out[0]);
    }

    #[test]
    fn an_unknown_method_is_a_protocol_error() {
        let out = run(
            Exposure::nothing(),
            &[
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"resources/list"}"#,
            ],
        );
        assert_eq!(
            parsed(&out[1]).get("error").and_then(|e| e.get("code")),
            Some(&Json::Number(codes::METHOD_NOT_FOUND.to_string()))
        );
    }

    /// Default-deny reaches the transport too, or the policy is decorative at
    /// the only place an agent touches it.
    #[test]
    fn an_unexposed_catalog_is_invisible_through_the_tools() {
        let out = run(
            Exposure::nothing(),
            &[
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"search_interfaces","arguments":{"query":""}}}"#,
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"describe_interface","arguments":{"id":"IDL:bank/Account:1.0"}}}"#,
            ],
        );
        let search = parsed(&out[1]).to_string();
        assert!(!search.contains("Account"), "{search}");
        let describe = parsed(&out[2]);
        assert_eq!(
            describe.get("result").and_then(|r| r.get("isError")),
            Some(&Json::Bool(true)),
            "{describe}"
        );
    }

    #[test]
    fn an_exposed_interface_is_searchable_and_describable() {
        let out = run(
            Exposure::nothing().allow_interface("IDL:bank/Account:1.0"),
            &[
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"describe_interface","arguments":{"id":"IDL:bank/Account:1.0"}}}"#,
            ],
        );
        let d = parsed(&out[1]);
        assert_eq!(d.get("result").and_then(|r| r.get("isError")), Some(&Json::Bool(false)));
        assert!(d.to_string().contains("balance"), "{d}");
    }

    /// A refusal is a successful response with isError, never a protocol error:
    /// the request was understood.
    #[test]
    fn a_denied_tool_call_is_content_rather_than_a_protocol_error() {
        let out = run(
            Exposure::nothing(),
            &[
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"invoke_operation","arguments":{"handle":"cap_deadbeef","operation":"balance"}}}"#,
            ],
        );
        let r = parsed(&out[1]);
        assert!(r.get("error").is_none(), "{r}");
        assert_eq!(r.get("result").and_then(|x| x.get("isError")), Some(&Json::Bool(true)));
    }

    /// A forged handle must not resolve, and the answer must not say whether
    /// the target exists.
    #[test]
    fn a_forged_handle_is_refused_at_the_transport() {
        let out = run(
            Exposure::nothing().allow_interface("IDL:bank/Account:1.0"),
            &[
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"invoke_operation","arguments":{"handle":"cap_00000000000000000000000000000000","operation":"balance"}}}"#,
            ],
        );
        let text = parsed(&out[1]).to_string();
        assert!(text.contains("no live reference"), "{text}");
    }

    /// Every line the server can emit must be one frame, including the ones
    /// produced from hostile input.
    #[test]
    fn no_response_ever_spans_two_lines() {
        let out = run(
            Exposure::nothing().allow_interface("IDL:bank/Account:1.0"),
            &[
                "not json at all",
                "",
                r#"{"jsonrpc":"1.0","id":1,"method":"initialize"}"#,
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
                r#"{"jsonrpc":"2.0","id":"a
b","method":"nope"}"#,
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"describe_interface","arguments":{"id":"IDL:bank/Account:1.0"}}}"#,
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call"}"#,
            ],
        );
        for line in out.into_iter().flatten() {
            assert!(!line.contains('\n'), "{line}");
            assert!(!line.contains('\r'), "{line}");
            assert!(Json::parse(&line).is_ok(), "{line}");
        }
    }

    /// A blank line is not a frame and must not become an error response, or a
    /// client that writes a trailing newline gets an answer to nothing.
    #[test]
    fn a_blank_line_is_ignored() {
        assert_eq!(run(Exposure::nothing(), &["", "   "]), vec![None, None]);
    }

    /// The dispatch path leaves audit lines: `tools/call` funnels through
    /// `Bridge::invoke`, which records every policy decision. A session
    /// nobody is signed into logs `caller=<nobody>`, and a refusal carries
    /// its why — no reconstruction, these are the lines the bridge wrote.
    #[test]
    fn a_tools_call_leaves_audit_lines_and_an_unsigned_session_is_nobody() {
        use orbweaver_giop::{Connection, IiopProfile, Ior, Version};

        let reg: &'static Registry = Box::leak(Box::new(registry_with_deposit()));
        // A real Connection to a listener that answers nothing; the calls
        // below are chosen so nothing ever touches the wire (a missing
        // argument fails after the policy gate, a refusal fails at it).
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
        let ior = Ior {
            type_id: "IDL:bank/Account:1.0".into(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: "127.0.0.1".into(),
                port: listener.local_addr().expect("has an address").port(),
                object_key: b"acct-1".to_vec(),
                components: Vec::new(),
            }],
        };
        let conn = Connection::connect(&ior, std::time::Duration::from_secs(5)).expect("dials");

        let exposure = Exposure::nothing().allow_operation("IDL:bank/Account:1.0", "deposit");
        let mut session = Session::new(reg, exposure, conn, "test");
        let h = session.bridge().handles().issue_checked(&ior).expect("issued");

        session.handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);
        // Allowed by policy, then refused at argument mapping: the ALLOW
        // line records the decision regardless.
        session.handle_line(&format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"invoke_operation","arguments":{{"handle":"{h}","operation":"deposit"}}}}}}"#
        ));
        // Not among the allowed operations: refused, with the why on record.
        session.handle_line(&format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"invoke_operation","arguments":{{"handle":"{h}","operation":"balance"}}}}}}"#
        ));

        let audit = session.bridge().audit().to_vec();
        assert_eq!(
            audit[0], "ALLOW caller=<nobody> target=IDL:bank/Account:1.0 operation=deposit",
            "{audit:?}"
        );
        assert!(
            audit[1].starts_with(
                "REFUSE caller=<nobody> target=IDL:bank/Account:1.0 operation=balance why="
            ),
            "{}",
            audit[1]
        );
        assert_eq!(audit.len(), 2);
    }

    fn registry_with_deposit() -> Registry {
        let spec = orbweaver_idl::parse(
            "module bank {
               interface Account {
                 //@ ai_effect: read_only
                 long balance();
                 //@ ai_effect: idempotent
                 void deposit(in long cents);
               };
             };",
        )
        .expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        r
    }
}
