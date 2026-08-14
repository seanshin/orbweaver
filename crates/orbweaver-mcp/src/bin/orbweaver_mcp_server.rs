//! `orbweaver-mcp-server` — the CORBA estate as three MCP tools over stdio.
//!
//! ```text
//! orbweaver-mcp-server --idl <file.idl>... --ior <file> \
//!                      [--expose <IDL:module/Iface:1.0[.operation]>]... \
//!                      [--as <principal>] [--scope <scope>]... \
//!                      [--map-scope <token-scope>=<contract-scope>]... [--token-scope <s>]... \
//!                      [--dry-run [<IDL:module/Iface:1.0>]] \
//!                      [--trace <path>|-] [--trace-ts <rfc3339>] \
//!                      [--quota <calls>] [--quota-scope <everything|caller|interface|operation>] \
//!                      [--audit-capacity <lines>]
//! ```
//!
//! Exposure is **default-deny**: with no `--expose`, the server starts, answers
//! the handshake, and finds nothing. That is the correct behaviour and not a
//! misconfiguration — an operator naming what an agent may reach is the point.
//!
//! # `--dry-run`: what would this exposure let an agent do?
//!
//! Prints the policy report for `--as`'s caller over everything `--expose`
//! names — every operation, what would happen to it, and why not — then exits
//! without serving anything. No target is dialled and `--ior` is not required,
//! because the question is asked before there is a deployment to point at. It
//! is the instrument for signing an exposure off; see `orbweaver_mcp::dryrun`.
//!
//! # `--map-scope`: the vocabulary gap, made loud before a call
//!
//! A token's scopes are the identity provider's vocabulary and `ai_authz`'s are
//! the contract's, and D005 measured what happens when they drift apart: a
//! deployment whose IdP issues `gate:operate` against a contract that demands
//! `parkinglot.barrier.open` **refuses every legitimate caller**, and the
//! refusal is indistinguishable from a permissions misconfiguration.
//!
//! `--map-scope <token>=<contract>` declares the translation
//! (`orbweaver_mcp::token::ScopeMap`) and `--token-scope <s>` declares what a
//! token from this IdP carries — which is also what `--as`'s caller holds, after
//! translation, so the report and the run agree about the same caller.
//! `--scope` is unchanged and still names **contract** scopes directly, for a
//! host whose caller is already in the contract's vocabulary.
//!
//! With any `--map-scope` given, `--dry-run`'s document carries a `scope_map`
//! section naming every contract scope no token can ever satisfy, who requires
//! it, every token scope placed nowhere, and every mapping nothing asks for.
//! **A finding exits 3**, because a report whose exit code is always zero is a
//! report a pipeline never reads.
//!
//! # `--trace`: one JSON line per decision
//!
//! D004 tier 1. `--trace <path>` appends span records to a file, `--trace -`
//! writes them to stderr, and without the flag nothing is emitted and nothing is
//! built. The record shape is fixed in `docs/decisions/D004-observability.md`
//! and implemented in `orbweaver_mcp::telemetry`; it is a machine-read trace,
//! not a second audit format — the audit lines still go to stderr as they did.
//!
//! **`--trace-ts` exists because this process has no clock.** The `ts` field
//! comes from the caller, which here is the command line: whatever `--trace-ts`
//! says is what every line of the run carries, and without it every line reads
//! `"ts":"-"`. That is the honest rendering of a process that never reads a
//! clock, and it is what makes two runs of the same session byte-identical —
//! the property the harness diffs against. A server that stamped lines from
//! `SystemTime::now()` would produce a trace nobody could replay, which is the
//! discipline D004's table cites (`PLAN-DEFERRED.md` §3).
//!
//! # `--quota`: a budget for the run, which is the only window this process has
//!
//! §4.5 #2's seat, filled by `orbweaver_mcp::quota`. `--quota <calls>` caps how
//! many calls this session may make, `--quota-scope` says what the cap is
//! counted against (per caller by default), and the refusal names the
//! arithmetic and is distinguishable from a policy refusal in both the audit
//! line and the trace.
//!
//! The budget **does not renew**, and that is a statement about this process
//! rather than a limitation of the mechanism. A renewing quota needs somebody
//! to open the next window, and a window is a label a *host* supplies —
//! `orbweaver_mcp::quota::Window`, on the same no-clock discipline as
//! `--trace-ts`. This process reads no clock and has no window source, so a
//! budget here is a per-run total and its refusals say `NO_PERMISSION`:
//! telling an agent to retry in a window that nobody will ever open would be a
//! lie with a retry loop attached to it. A host that *does* have a clock builds
//! the same `Quota` with `Renewal::Window` and calls `open_window` itself.
//!
//! # The audit ledger goes to stderr, one line per decision
//!
//! §4.8 asks for a record of every decision, and until now this process met
//! that requirement only under `--dry-run`. Serving, the ALLOW/REFUSE lines
//! accumulated in the chain's audit stage and were **discarded when the process
//! exited** — the library kept the ledger and the deployment threw it away, so
//! the requirement was satisfied by a test and not by anything an operator
//! could read. The end-to-end run found it.
//!
//! Now each line is emitted to stderr as it is written, alongside the root
//! handle and every other diagnostic; stdout stays protocol-only. Emission
//! happens once the frame that produced it has been handled and *before* its
//! response is written, and once more after the loop — so neither a client
//! that reacts to a refusal by killing the process, nor a decision made by the
//! last frame before EOF, can lose its line.
//!
//! It is stderr rather than a file because that is where this process already
//! puts everything an operator reads, and redirecting a stream is something a
//! supervisor already knows how to do. A trace goes to `--trace <path>`; the
//! audit ledger goes where the diagnostics go.
//!
//! # `--audit-capacity`: the in-memory ledger is bounded, and says so
//!
//! Emitting to stderr left the *library's* ledger growing for the life of the
//! session, which the batch that landed the emission recorded as a known limit.
//! `orbweaver_mcp::interceptor::AuditInterceptor` now keeps its newest
//! `--audit-capacity` lines (65,536 by default) and spends one slot on an
//! elision marker naming how many it dropped — because an audit ledger that
//! drops lines silently reads exactly like a quiet period, and the two are
//! indistinguishable at the moment somebody is reading the log to tell them
//! apart.
//!
//! **The two streams answer different questions and only one of them is
//! bounded.** stderr is the complete ledger: every line is emitted as it is
//! written, before the frame's response goes out, so it is the record an
//! operator keeps. The in-memory slice is what `Bridge::audit` hands to §7.4
//! I4's promotion oracle, and it is bounded because a process that runs for a
//! week must not be holding every decision it ever made. The marker is what
//! keeps the second honest about being a window onto the first, and
//! `promote::verify_promotion` refuses it by name rather than judging a
//! promotion from a gap.
//!
//! # stdout is the protocol
//!
//! One JSON object per line on stdout and nothing else, ever. Every diagnostic
//! goes to stderr. A single stray `println!` desynchronises the session, and
//! the client reports it as malformed JSON rather than as the bug it is. The
//! `--dry-run` report goes to stdout and is exempt only because it never enters
//! the loop: it prints one JSON object and the process ends.

use std::io::{BufRead, Write};
use std::time::Duration;

use orbweaver_dynamic::json::Json;
use orbweaver_giop::{Connection, Ior};
use orbweaver_mcp::Bridge;
use orbweaver_mcp::identity::Caller;
use orbweaver_mcp::policy::{Approval, Exposure};
use orbweaver_mcp::quota::{Quota, Renewal, Scope};
use orbweaver_mcp::session::Session;
use orbweaver_mcp::telemetry::{CallPath, JsonLines, Timestamp, Trace};
use orbweaver_mcp::token::ScopeMap;
use orbweaver_registry::Registry;

/// D004's trace for this run: `-` is stderr, anything else is a file.
///
/// Opened for **append**. A trace is a ledger and truncating one on start would
/// lose the run somebody is asking about — and the harness appends across
/// several invocations on purpose.
fn trace_for(to: &str, ts: Option<&str>, session: &str) -> Result<Trace, String> {
    let sink: Box<dyn Write> = if to == "-" {
        Box::new(std::io::stderr())
    } else {
        match std::fs::OpenOptions::new().create(true).append(true).open(to) {
            Ok(f) => Box::new(f),
            Err(e) => return Err(format!("{to}: {e}")),
        }
    };
    // No clock is read here or anywhere below. `--trace-ts` or `-`.
    let ts = ts.map_or_else(Timestamp::unstamped, Timestamp::new);
    Ok(Trace::new(session, CallPath::Dynamic, ts, JsonLines::new(sink)))
}

/// §4.5 #2's occupant for this run, or `None` when no `--quota` was given.
///
/// [`Renewal::Never`]: this process has no window source, so the honest shape
/// is a per-run total whose refusals do not invite a retry. See the module
/// docs.
fn quota_for(limit: Option<u64>, scope: &str) -> Result<Option<Quota>, String> {
    let Some(limit) = limit else { return Ok(None) };
    let scope = match scope {
        "everything" => Scope::Everything,
        "caller" => Scope::Caller,
        "interface" => Scope::Interface,
        "operation" => Scope::Operation,
        other => {
            return Err(format!(
                "--quota-scope {other:?}: expected everything, caller, interface or operation"
            ));
        }
    };
    Ok(Some(Quota::new(limit, scope, Renewal::Never)))
}

/// Emits every audit line written since `from` to stderr, and returns the new
/// watermark.
///
/// A watermark rather than a drain: the chain's audit stage owns its lines and
/// nothing can take them away from it — `Bridge::audit` is what §7.4 I4's
/// oracle captures, and a stage a process could empty is a stage a process
/// could be configured into emptying before anybody read it.
///
/// The watermark is `Bridge::audit_written` — a **count of lines ever written**
/// — and not an index into `Bridge::audit`. That distinction is the whole of
/// what a bounded ledger costs an emitter: the moment the ledger drops its
/// oldest line, every index into it means a different line than it did, so an
/// index-based watermark starts re-emitting the tail and then skips lines
/// forever after. The count does not move under anybody.
///
/// This stream is the complete one. The in-memory ledger keeps its newest
/// `--audit-capacity` lines and marks what it dropped; stderr has had every
/// line since the process started, because each is emitted as it is written.
/// The one way a line could reach the ledger's ceiling before being emitted is
/// a single frame writing more lines than the whole ceiling, which is reported
/// here rather than passed over — an audit stream with a silent hole in it is
/// not an audit stream.
fn emit_audit(bridge: &Bridge<'_>, from: u64) -> u64 {
    let lines = bridge.audit();
    let dropped = bridge.audit_dropped();
    if dropped > from {
        eprintln!(
            "audit: {} line(s) were dropped from the in-memory ledger before this process \
             emitted them; raise --audit-capacity",
            dropped - from
        );
    }
    // Absolute position → position in the retained slice. The marker holds the
    // first slot once anything has been dropped and is skipped here: it stands
    // for lines this stream already carries, and re-emitting it on every frame
    // would be noise that says nothing new.
    let skip = (from.max(dropped) - dropped) as usize + usize::from(dropped > 0);
    for line in &lines[skip.min(lines.len())..] {
        eprintln!("{line}");
    }
    bridge.audit_written()
}

fn main() -> std::process::ExitCode {
    let mut idls: Vec<String> = Vec::new();
    let mut ior_path: Option<String> = None;
    let mut expose: Vec<String> = Vec::new();
    let mut session_id = "stdio".to_owned();
    let mut principal: Option<String> = None;
    let mut scopes: Vec<String> = Vec::new();
    let mut token_scopes: Vec<String> = Vec::new();
    let mut scope_map = ScopeMap::nothing();
    let mut mapping_given = false;
    let mut dry_run = false;
    let mut dry_run_only: Option<String> = None;
    let mut trace_to: Option<String> = None;
    let mut trace_ts: Option<String> = None;
    let mut quota_limit: Option<u64> = None;
    let mut quota_scope = "caller".to_owned();
    let mut audit_capacity: Option<usize> = None;

    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut next = |what: &str| match args.next() {
            Some(v) => Ok(v),
            None => Err(format!("{what} needs a value")),
        };
        let taken = match a.as_str() {
            "--idl" => next("--idl").map(|v| idls.push(v)),
            "--ior" => next("--ior").map(|v| ior_path = Some(v)),
            "--expose" => next("--expose").map(|v| expose.push(v)),
            "--session" => next("--session").map(|v| session_id = v),
            "--as" => next("--as").map(|v| principal = Some(v)),
            "--scope" => next("--scope").map(|v| scopes.push(v)),
            "--token-scope" => next("--token-scope").map(|v| token_scopes.push(v)),
            // `token=contract`, split at the **first** `=` only: a contract
            // scope is free to contain one and an IdP scope is not, so the
            // asymmetric split is the one that cannot mangle a real name.
            "--map-scope" => next("--map-scope").and_then(|v| match v.split_once('=') {
                Some((token, contract)) if !token.is_empty() && !contract.is_empty() => {
                    scope_map = std::mem::take(&mut scope_map).map(token, contract);
                    mapping_given = true;
                    Ok(())
                }
                _ => Err(format!(
                    "--map-scope {v:?}: expected <token-scope>=<contract-scope>, both non-empty"
                )),
            }),
            "--dry-run" => {
                dry_run = true;
                Ok(())
            }
            "--trace" => next("--trace").map(|v| trace_to = Some(v)),
            "--trace-ts" => next("--trace-ts").map(|v| trace_ts = Some(v)),
            "--quota" => next("--quota").and_then(|v| match v.parse::<u64>() {
                Ok(n) => {
                    quota_limit = Some(n);
                    Ok(())
                }
                Err(e) => Err(format!("--quota {v:?}: {e}")),
            }),
            "--quota-scope" => next("--quota-scope").map(|v| quota_scope = v),
            "--audit-capacity" => next("--audit-capacity").and_then(|v| match v.parse::<usize>() {
                Ok(0) => {
                    Err("--audit-capacity 0: a ledger that cannot hold a line cannot be one"
                        .to_owned())
                }
                Ok(n) => {
                    audit_capacity = Some(n);
                    Ok(())
                }
                Err(e) => Err(format!("--audit-capacity {v:?}: {e}")),
            }),
            "-h" | "--help" => {
                eprintln!(
                    "usage: orbweaver-mcp-server --idl <file.idl>... --ior <file> \
                     [--expose <id[.operation]>]... [--as <principal>] [--scope <scope>]... \
                     [--map-scope <token>=<contract>]... [--token-scope <scope>]... \
                     [--dry-run[=<id>]] [--trace <path>|-] [--trace-ts <rfc3339>] \
                     [--quota <calls>] [--quota-scope <everything|caller|interface|operation>] \
                     [--audit-capacity <lines>]"
                );
                return std::process::ExitCode::SUCCESS;
            }
            // `--dry-run=<id>` asks about one interface, exposed or not, which
            // is the "what if I allowlisted this?" question.
            other => match other.strip_prefix("--dry-run=") {
                Some(id) => {
                    dry_run = true;
                    dry_run_only = Some(id.to_owned());
                    Ok(())
                }
                None => Err(format!("unexpected argument {other:?}")),
            },
        };
        if let Err(e) = taken {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    }

    // A dry run needs no target: it asks what the policy would do, and asking
    // is the whole of it. Requiring an `--ior` would mean an operator could
    // only preview an exposure once the thing it protects was already running.
    if idls.is_empty() || (ior_path.is_none() && !dry_run) {
        eprintln!(
            "usage: orbweaver-mcp-server --idl <file.idl>... --ior <file>   (--ior is not \
             needed with --dry-run)"
        );
        return std::process::ExitCode::from(2);
    }

    let mut registry = Registry::new();
    for path in &idls {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{path}: {e}");
                return std::process::ExitCode::from(2);
            }
        };
        // The gate, not the parser: a catalog built from IDL that S4 rejects
        // would describe operations nobody can call.
        match orbweaver_idl::check(&src) {
            Ok(spec) => {
                if let Err(e) = registry.load(&spec) {
                    eprintln!("{path}: {e}");
                    return std::process::ExitCode::from(2);
                }
            }
            Err(diags) => {
                for d in diags.iter().take(5) {
                    eprintln!("{path}:{d}");
                }
                return std::process::ExitCode::from(2);
            }
        }
    }

    let mut exposure = Exposure::nothing();
    for spec in &expose {
        // `IDL:m/I:1.0.operation` — the operation is split at the last dot.
        // A repository id ends in its *version*, `:1.0`, which has a dot in
        // it, so the trailing part is only an operation when it looks like an
        // IDL identifier: a bare `IDL:spike/Echo:1.0` used to be read as the
        // interface `IDL:spike/Echo:1` with an operation named `0`, which
        // allowlisted an interface nobody had and exposed nothing. The first
        // `--dry-run` report run against a real IDL file said
        // `id: IDL:spike/Echo:1, operation: 0, declared: false`, which is how
        // this was found.
        let identifier = |op: &str| op.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_');
        match spec.rsplit_once('.') {
            Some((id, op)) if identifier(op) && !op.contains(':') => {
                exposure = exposure.allow_operation(id, op);
            }
            _ => exposure = exposure.allow_interface(spec.clone()),
        }
    }
    if expose.is_empty() {
        eprintln!(
            "no --expose given: the catalog holds {} interface(s) and the agent will see none",
            orbweaver_mcp::exposable_interfaces(&registry).len()
        );
    }

    // Token scopes reach the caller only through the map, which is the whole
    // point: this process holds the IdP's vocabulary and the contract's, and
    // the only way from one to the other is the translation an operator wrote.
    // Anything the map does not place grants nothing and is named on stderr —
    // ignored is not silent (`orbweaver_mcp::token::Unmapped`).
    let translated = scope_map.translate(&token_scopes);
    for unplaced in translated.unmapped() {
        eprintln!(
            "--token-scope {unplaced:?} is placed by no --map-scope, so it grants this bridge \
             nothing"
        );
    }
    let caller = principal.map(|p| {
        scopes
            .iter()
            .chain(translated.granted())
            .fold(Caller::new(p), |c, scope| c.with_scope(scope.clone()))
    });

    let quota = match quota_for(quota_limit, &quota_scope) {
        Ok(q) => q,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    };

    if dry_run {
        // No IOR is read, no socket is opened, and no handle is issued: the
        // report is a question about the policy and the policy is in memory.
        // The approval is the one a session starts with — an operator asking
        // "what needs a human?" wants the answer for a session that has not
        // been handed one.
        // D005's class, asked before the survey and answered from the same
        // exposure the survey runs against: is there a contract scope no token
        // this deployment issues can ever satisfy? Computed here because
        // `Bridge::new` takes the exposure by value.
        let scope_audit =
            mapping_given.then(|| scope_map.audit(&registry, &exposure, &token_scopes));
        let mut bridge = Bridge::new(&registry, exposure, session_id.clone());
        if let Some(caller) = caller {
            bridge.set_caller(caller);
        }
        if let Some(capacity) = audit_capacity
            && !bridge.chain_mut().audit_capacity(capacity)
        {
            eprintln!("no audit stage to bound");
            return std::process::ExitCode::from(2);
        }
        // The report is about the chain this deployment would run, so the
        // quota goes in before the questions are asked. A dry run spends none
        // of it — `orbweaver_mcp::quota` refunds what a question charges.
        if let Some(quota) = &quota
            && !bridge.chain_mut().quota(quota.clone())
        {
            eprintln!("no authorization stage to put a quota after");
            return std::process::ExitCode::from(2);
        }
        // A dry run is traced too, under its own decision tokens: the questions
        // an operator asked before a deployment are exactly what somebody wants
        // to read back afterwards. Nothing is dialled and nothing is counted.
        if let Some(to) = &trace_to {
            match trace_for(to, trace_ts.as_deref(), &session_id) {
                Ok(trace) => {
                    if !bridge.chain_mut().trace(trace) {
                        eprintln!("no telemetry stage to trace: nothing would be emitted");
                        return std::process::ExitCode::from(2);
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
                    return std::process::ExitCode::from(2);
                }
            }
        }
        let mut report = match &dry_run_only {
            Some(id) => bridge.dry_run_interface(id, Approval::default()),
            None => bridge.dry_run_all(Approval::default()),
        };
        // Folded into the one document rather than printed beside it: stdout is
        // one JSON object, and a second one would break every pipeline that
        // parses this. Absent entirely when no mapping was configured, so a
        // deployment that does not use the feature sees the document it always
        // saw.
        if let (Some(audit), Json::Object(fields)) = (&scope_audit, &mut report) {
            fields.insert("scope_map".to_owned(), audit.to_json());
        }
        println!("{report}");
        // The questions are on the record too, and stderr is where a
        // diagnostic goes; the report on stdout is what gets piped.
        for line in bridge.audit() {
            eprintln!("{line}");
        }
        // A finding is worth an exit code. An operator can read `scope_map.ok`
        // out of the document, and a pipeline that never checks is exactly how
        // D005's drift reached production the one time it was measured — so the
        // process says so in the one channel a script cannot ignore.
        if let Some(audit) = &scope_audit {
            for finding in audit.findings() {
                eprintln!("{finding}");
            }
            if !audit.ok() {
                return std::process::ExitCode::from(3);
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    let Some(ior_path) = ior_path else {
        eprintln!("--ior is required to serve");
        return std::process::ExitCode::from(2);
    };
    let text = match std::fs::read_to_string(&ior_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{ior_path}: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let ior = match Ior::parse(text.trim()) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{ior_path}: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let conn = match Connection::connect(&ior, Duration::from_secs(10)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot reach the target: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let mut session = Session::new(&registry, exposure, conn, session_id.clone());
    if let Some(capacity) = audit_capacity {
        if !session.bridge().chain_mut().audit_capacity(capacity) {
            eprintln!("no audit stage to bound");
            return std::process::ExitCode::from(2);
        }
        // Said out loud, like the quota: a ledger that drops lines is something
        // an operator chose, and the choice belongs in the same stream as the
        // lines it will eventually elide.
        eprintln!("audit ledger: the newest {capacity} lines are kept in memory (stderr has all)");
    }
    if let Some(quota) = &quota {
        if !session.bridge().chain_mut().quota(quota.clone()) {
            eprintln!("no authorization stage to put a quota after");
            return std::process::ExitCode::from(2);
        }
        // Said out loud at startup: a limit an operator forgot they set is a
        // limit they will debug as a policy failure.
        eprintln!(
            "quota: {} calls per {}, for this run only (this process opens no windows)",
            quota.limit(),
            quota.scope()
        );
    }
    if let Some(trace_to) = &trace_to {
        match trace_for(trace_to, trace_ts.as_deref(), &session_id) {
            Ok(trace) => {
                if !session.bridge().chain_mut().trace(trace) {
                    eprintln!("no telemetry stage to trace: nothing would be emitted");
                    return std::process::ExitCode::from(2);
                }
            }
            Err(e) => {
                eprintln!("{e}");
                return std::process::ExitCode::from(2);
            }
        }
    }
    if let Some(caller) = caller {
        // Without this the audit log says `<nobody>` for every call the
        // process makes, and every `ai_authz` scope refuses. `--as` is a host
        // assertion made on the command line, which is the only channel this
        // process has for one.
        session = session.on_behalf_of(caller);
    }

    // The agent needs somewhere to start. A bridge that resolved names for
    // itself would issue this from a naming service; here the target is given
    // on the command line, so the handle for it is issued up front and printed
    // to stderr — never to stdout, where it would be a stray frame.
    match session.bridge().handles().issue_checked(&ior) {
        Ok(h) => eprintln!("root handle: {h}"),
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    }

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    // How much of the ledger has left this process. §4.8's requirement is met
    // by what an operator can read, not by what the chain remembers.
    let mut audited = 0u64;
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("stdin: {e}");
                break;
            }
        };
        let response = session.handle_line(&line);
        // Before the response is written, so a client that reads a refusal and
        // kills the process cannot beat its own audit line out of the door.
        audited = emit_audit(session.bridge(), audited);
        if let Some(response) = response {
            // One write, one newline, one flush. A client is waiting on the
            // newline, and a buffered response is indistinguishable from a hang.
            if writeln!(stdout, "{response}").is_err() || stdout.flush().is_err() {
                break;
            }
        }
    }
    // The loop can leave by `break`, so the last decision is emitted here or
    // not at all.
    emit_audit(session.bridge(), audited);
    std::process::ExitCode::SUCCESS
}
