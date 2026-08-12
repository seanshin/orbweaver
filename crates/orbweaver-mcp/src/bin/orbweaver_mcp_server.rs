//! `orbweaver-mcp-server` — the CORBA estate as three MCP tools over stdio.
//!
//! ```text
//! orbweaver-mcp-server --idl <file.idl>... --ior <file> \
//!                      [--expose <IDL:module/Iface:1.0[.operation]>]...
//! ```
//!
//! Exposure is **default-deny**: with no `--expose`, the server starts, answers
//! the handshake, and finds nothing. That is the correct behaviour and not a
//! misconfiguration — an operator naming what an agent may reach is the point.
//!
//! # stdout is the protocol
//!
//! One JSON object per line on stdout and nothing else, ever. Every diagnostic
//! goes to stderr. A single stray `println!` desynchronises the session, and
//! the client reports it as malformed JSON rather than as the bug it is.

use std::io::{BufRead, Write};
use std::time::Duration;

use orbweaver_giop::{Connection, Ior};
use orbweaver_mcp::policy::Exposure;
use orbweaver_mcp::session::Session;
use orbweaver_registry::Registry;

fn main() -> std::process::ExitCode {
    let mut idls: Vec<String> = Vec::new();
    let mut ior_path: Option<String> = None;
    let mut expose: Vec<String> = Vec::new();
    let mut session_id = "stdio".to_owned();

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
            "-h" | "--help" => {
                eprintln!(
                    "usage: orbweaver-mcp-server --idl <file.idl>... --ior <file> \
                     [--expose <id[.operation]>]..."
                );
                return std::process::ExitCode::SUCCESS;
            }
            other => Err(format!("unexpected argument {other:?}")),
        };
        if let Err(e) = taken {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    }

    let (Some(ior_path), false) = (ior_path, idls.is_empty()) else {
        eprintln!("usage: orbweaver-mcp-server --idl <file.idl>... --ior <file>");
        return std::process::ExitCode::from(2);
    };

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
        // `IDL:m/I:1.0.operation` — the repository id already contains colons
        // and slashes, so the operation is split at the last dot, which a
        // repository id never contains after the version.
        match spec.rsplit_once('.') {
            Some((id, op)) if !op.is_empty() && !op.contains(':') => {
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
