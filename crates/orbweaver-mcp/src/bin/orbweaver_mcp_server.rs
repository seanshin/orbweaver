//! `orbweaver-mcp-server` — the CORBA estate as three MCP tools over stdio.
//!
//! ```text
//! orbweaver-mcp-server --idl <file.idl>... --ior <file> \
//!                      [--expose <IDL:module/Iface:1.0[.operation]>]... \
//!                      [--as <principal>] [--scope <scope>]... \
//!                      [--dry-run [<IDL:module/Iface:1.0>]]
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
//! # stdout is the protocol
//!
//! One JSON object per line on stdout and nothing else, ever. Every diagnostic
//! goes to stderr. A single stray `println!` desynchronises the session, and
//! the client reports it as malformed JSON rather than as the bug it is. The
//! `--dry-run` report goes to stdout and is exempt only because it never enters
//! the loop: it prints one JSON object and the process ends.

use std::io::{BufRead, Write};
use std::time::Duration;

use orbweaver_giop::{Connection, Ior};
use orbweaver_mcp::Bridge;
use orbweaver_mcp::identity::Caller;
use orbweaver_mcp::policy::{Approval, Exposure};
use orbweaver_mcp::session::Session;
use orbweaver_registry::Registry;

fn main() -> std::process::ExitCode {
    let mut idls: Vec<String> = Vec::new();
    let mut ior_path: Option<String> = None;
    let mut expose: Vec<String> = Vec::new();
    let mut session_id = "stdio".to_owned();
    let mut principal: Option<String> = None;
    let mut scopes: Vec<String> = Vec::new();
    let mut dry_run = false;
    let mut dry_run_only: Option<String> = None;

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
            "--dry-run" => {
                dry_run = true;
                Ok(())
            }
            "-h" | "--help" => {
                eprintln!(
                    "usage: orbweaver-mcp-server --idl <file.idl>... --ior <file> \
                     [--expose <id[.operation]>]... [--as <principal>] [--scope <scope>]... \
                     [--dry-run[=<id>]]"
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

    let caller = principal
        .map(|p| scopes.iter().fold(Caller::new(p), |c, scope| c.with_scope(scope.clone())));

    if dry_run {
        // No IOR is read, no socket is opened, and no handle is issued: the
        // report is a question about the policy and the policy is in memory.
        // The approval is the one a session starts with — an operator asking
        // "what needs a human?" wants the answer for a session that has not
        // been handed one.
        let mut bridge = Bridge::new(&registry, exposure, session_id);
        if let Some(caller) = caller {
            bridge.set_caller(caller);
        }
        let report = match &dry_run_only {
            Some(id) => bridge.dry_run_interface(id, Approval::default()),
            None => bridge.dry_run_all(Approval::default()),
        };
        println!("{report}");
        // The questions are on the record too, and stderr is where a
        // diagnostic goes; the report on stdout is what gets piped.
        for line in bridge.audit() {
            eprintln!("{line}");
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

    let mut session = Session::new(&registry, exposure, conn, session_id);
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
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("stdin: {e}");
                break;
            }
        };
        if let Some(response) = session.handle_line(&line) {
            // One write, one newline, one flush. A client is waiting on the
            // newline, and a buffered response is indistinguishable from a hang.
            if writeln!(stdout, "{response}").is_err() || stdout.flush().is_err() {
                break;
            }
        }
    }
    std::process::ExitCode::SUCCESS
}
