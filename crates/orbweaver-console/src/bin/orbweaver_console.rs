//! `orbweaver-console` — the operator's three questions, rendered.
//!
//! ```text
//! orbweaver-console catalog <file.idl>... [-I <dir>]... [--expose <id>]
//!                           [--expose-op <id> <op>] [--caller <principal>]
//!                           [--scope <scope>] [--approved] [--ior <file>]...
//!                           [--html <out.html>] [--text]
//! orbweaver-console diff    <released.idl> <proposed.idl> [-I <dir>]...
//!                           [--approvals <file>] [--html <out.html>] [--text]
//! orbweaver-console traces  <spans.jsonl>... [--html <out.html>] [--text]
//! orbweaver-console services <snapshot.json> [--html <out.html>] [--text]
//! orbweaver-console config  [<snapshot.json>] [--html <out.html>] [--text]
//! orbweaver-console stats   <snapshot.json> [--html <out.html>] [--text]
//! ```
//!
//! # Why the ORB's read commands are subcommands here and not an `orbctl`
//!
//! D024 §4 calls the tool `orbctl` and §6 puts its read half in this crate.
//! The document names **the capability, not the executable**, and everything
//! the read half needs is already in this binary: `Output` and its `--html` /
//! `--text` contract, the usage text, and — the one that decides it — the
//! escaping proof in `tests/escaping.rs`, which asserts structurally that no
//! page carries an element this crate did not write. A second binary would
//! duplicate the first two and would sit *outside* the third until somebody
//! remembered to extend it, which is how a page gets rendered without that
//! guarantee. So: three more subcommands, one binary, one allowlist.
//!
//! The write half — `-ORBInitRef` from a configuration file, D024 §6 item 2 —
//! is **not here**, and neither is anything that ends a channel, deactivates a
//! POA or drops a connection. Those are lifecycle, they are the wire's
//! `destroy` question (`PLAN-DEFERRED` §11), and an admin CLI doing them
//! locally would be the same unauthenticated power through a side door.
//!
//! `-I` is `sidl-validate`'s flag and means the same thing: another directory
//! to resolve `#include` against. The quoted form searches the including file's
//! own directory first, so an estate stored as one tree needs no `-I` at all.
//!
//! `--ior` names a file holding one stringified reference (`IOR:…`), as the
//! fixtures publish them. The page then carries that peer's CSIv2 capability
//! record — whether the target can enforce a caller identity, or the bridge is
//! the only enforcement point (PLAN §4.8) — beside the interface its type
//! names. The reference is read for its tagged components and not dialed.
//!
//! Exit status: 0 rendered, 2 could not run.
//!
//! **There is no exit status for "bad news".** `idl-diff` is the release gate
//! and refuses with a non-zero exit; a viewer that also refused would be a
//! second gate, and a second gate is something a release gets routed around.
//! The console renders and exits 0 whether the news is good or not.
//!
//! `--html` writes one self-contained file; `--text` writes the same facts to
//! stdout. Passing neither is `--text`, because a tool that silently wrote a
//! file somewhere would be a tool nobody could find the output of.

use std::path::PathBuf;
use std::process::ExitCode;

use orbweaver_console::{catalog, contract, load, orb, traces};
use orbweaver_giop::Ior;
use orbweaver_idl::include::SearchPath;
use orbweaver_mcp::identity::Caller;
use orbweaver_mcp::interceptor::Chain;
use orbweaver_mcp::policy::{Approval, Exposure};
use orbweaver_registry::Registry;

const USAGE: &str = "\
usage:
  orbweaver-console catalog <file.idl>... [-I <dir>]... [--expose <id>]
                            [--expose-op <id> <op>] [--caller <principal>]
                            [--scope <scope>] [--approved] [--ior <file>]...
                            [--html <out.html>] [--text]
  orbweaver-console diff    <released.idl> <proposed.idl> [-I <dir>]...
                            [--approvals <file>] [--html <out.html>] [--text]
  orbweaver-console traces  <spans.jsonl>... [--html <out.html>] [--text]
  orbweaver-console services <snapshot.json> [--html <out.html>] [--text]
  orbweaver-console config  [<snapshot.json>] [--html <out.html>] [--text]
  orbweaver-console stats   <snapshot.json> [--html <out.html>] [--text]

services, config and stats are the ORB's read-side administration surface
(D024 §4). THEY CANNOT REACH A RUNNING SERVER. The state they show lives
inside the process that holds it, and D024 §7 refuses a wire interface for it
until the caller model PLAN-DEFERRED §11 is waiting on exists — a remote admin
interface without one is unauthenticated power. So they read a JSON snapshot
the holding process wrote (orbweaver_console::orb::Snapshot), or a process
renders its own state in-process through the library.

NOTHING IN THIS WORKSPACE WRITES A SNAPSHOT YET. Until a server grows one,
`config` with no file is the only one of the three with something to say: the
seven ORB numbers as this build compiled them. That is a real answer and not a
placeholder — every value is read from the constant that owns it.

None of the three writes, registers, deactivates or disconnects anything.

-I adds a directory to resolve #include against, as sidl-validate does. Every
file is read as a translation unit: what it includes is part of what it says.
--ior names a file holding a stringified reference; the page carries what that
peer's IOR says it can enforce (CSIv2 mechanism list, TAG_SSL_SEC_TRANS). It
is read, not dialed.

The console renders what the registry, the differ and the audit already
decided. It takes no decision of its own and exits 0 whether the news is good
or not — idl-diff is the release gate.";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

/// How a view is delivered.
struct Output {
    html: Option<PathBuf>,
    text: bool,
}

impl Output {
    fn deliver(
        &self,
        html: impl FnOnce() -> String,
        text: impl FnOnce() -> String,
    ) -> Result<(), String> {
        if let Some(path) = &self.html {
            let page = html();
            std::fs::write(path, page).map_err(|e| format!("{}: {e}", path.display()))?;
            eprintln!("wrote {}", path.display());
        }
        if self.text || self.html.is_none() {
            print!("{}", text());
        }
        Ok(())
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or_else(|| USAGE.to_owned())?;
    let rest: Vec<String> = args.collect();
    match command.as_str() {
        "catalog" => catalog_command(rest),
        "diff" => diff_command(rest),
        "traces" => traces_command(rest),
        "services" => {
            orb_command(rest, "services", orb::render_services_html, orb::render_services_text)
        }
        "config" => orb_command(rest, "config", orb::render_config_html, orb::render_config_text),
        "stats" => orb_command(rest, "stats", orb::render_stats_html, orb::render_stats_text),
        "-h" | "--help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    }
}

fn catalog_command(args: Vec<String>) -> Result<(), String> {
    let mut files = Vec::new();
    let mut exposure = Exposure::nothing();
    let mut principal: Option<String> = None;
    let mut scopes: Vec<String> = Vec::new();
    let mut approval = Approval::default();
    let mut out = Output { html: None, text: false };
    let mut search = SearchPath::new();
    let mut peers: Vec<(String, Ior)> = Vec::new();

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-I" => {
                search.push(need(&mut it, "-I <dir>")?);
            }
            "--ior" => {
                let path = need(&mut it, "--ior <file>")?;
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
                let ior = Ior::parse(text.trim()).map_err(|e| format!("{path}: {e}"))?;
                peers.push((path, ior));
            }
            "--expose" => exposure = exposure.allow_interface(need(&mut it, "--expose <id>")?),
            "--expose-op" => {
                let id = need(&mut it, "--expose-op <id> <operation>")?;
                let op = need(&mut it, "--expose-op <id> <operation>")?;
                exposure = exposure.allow_operation(id, op);
            }
            "--caller" => principal = Some(need(&mut it, "--caller <principal>")?),
            "--scope" => scopes.push(need(&mut it, "--scope <scope>")?),
            "--approved" => approval.destructive_approved = true,
            "--html" => out.html = Some(PathBuf::from(need(&mut it, "--html <path>")?)),
            "--text" => out.text = true,
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag:?}")),
            file => files.push(file.to_owned()),
        }
    }
    if files.is_empty() {
        return Err(format!("catalog needs at least one IDL file\n\n{USAGE}"));
    }

    let mut registry = Registry::new();
    for file in &files {
        load_into(&mut registry, file, &search)?;
    }

    let caller = principal.map(|p| scopes.iter().fold(Caller::new(p), |c, s| c.with_scope(s)));
    // The chain the page is rendered from is `Chain::standard`: this binary has
    // no deployment's extensions to hand. A deployment with its own stages
    // renders through the library, passing its own chain, which is why
    // `catalog::build` takes one rather than building it.
    let mut chain = Chain::standard(exposure.clone());
    let mut view = catalog::build(&mut chain, &registry, &exposure, caller.as_ref(), approval);
    for (label, ior) in &peers {
        view.attach_peer(label.as_str(), ior);
    }

    out.deliver(|| catalog::render_html(&view), || catalog::render_text(&view))
}

fn diff_command(args: Vec<String>) -> Result<(), String> {
    let mut files = Vec::new();
    let mut out = Output { html: None, text: false };
    let mut search = SearchPath::new();
    let mut approvals: Option<PathBuf> = None;
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-I" => {
                search.push(need(&mut it, "-I <dir>")?);
            }
            "--approvals" => {
                approvals = Some(PathBuf::from(need(&mut it, "--approvals <file>")?));
            }
            "--html" => out.html = Some(PathBuf::from(need(&mut it, "--html <path>")?)),
            "--text" => out.text = true,
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag:?}")),
            file => files.push(file.to_owned()),
        }
    }
    let [released, proposed] = files.as_slice() else {
        return Err(format!("diff needs exactly two IDL files\n\n{USAGE}"));
    };

    // Both sides resolve their own includes. A revision that only changed a
    // shared header would otherwise diff as no change at all, which is the
    // §5.3 verdict an operator would most regret trusting. The same pass
    // fingerprints each unit, which is what an approval on record binds to;
    // the store is `--approvals`, or `<proposed>.approvals.tsv` if it exists.
    let (view, advice) = contract::load(
        std::path::Path::new(released),
        std::path::Path::new(proposed),
        &search,
        approvals.as_deref(),
    )?;
    for note in advice {
        eprintln!("note: {note}");
    }
    out.deliver(|| contract::render_html(&view), || contract::render_text(&view))
}

fn traces_command(args: Vec<String>) -> Result<(), String> {
    let mut files = Vec::new();
    let mut out = Output { html: None, text: false };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--html" => out.html = Some(PathBuf::from(need(&mut it, "--html <path>")?)),
            "--text" => out.text = true,
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag:?}")),
            file => files.push(file.to_owned()),
        }
    }
    if files.is_empty() {
        return Err(format!("traces needs at least one JSON-lines file\n\n{USAGE}"));
    }

    let mut log = traces::TraceLog::default();
    for file in &files {
        let contents = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
        log.read(file, &contents);
    }

    out.deliver(|| traces::render_html(&log), || traces::render_text(&log))
}

/// The three ORB read commands, which differ only in which renderer they call.
///
/// One function rather than three, because the argument handling, the "which
/// snapshot" question and the sentence explaining what cannot be reached are
/// the same for all three — and a sentence three functions say is a sentence
/// that will go stale in two of them.
fn orb_command(
    args: Vec<String>,
    what: &str,
    html: fn(&orb::Snapshot) -> String,
    text: fn(&orb::Snapshot) -> String,
) -> Result<(), String> {
    let mut files: Vec<String> = Vec::new();
    let mut out = Output { html: None, text: false };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--html" => out.html = Some(PathBuf::from(need(&mut it, "--html <path>")?)),
            "--text" => out.text = true,
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag:?}")),
            file => files.push(file.to_owned()),
        }
    }
    let snapshot = match files.as_slice() {
        [] if what == "config" => {
            // The one honest thing to show with no snapshot: the constants this
            // build compiled. `services` and `stats` have no such fallback —
            // an empty registration table would be a claim about a running ORB
            // that this tool has not looked at.
            orb::Snapshot { origin: "this build's compiled defaults".into(), ..Default::default() }
        }
        [] => {
            return Err(format!(
                "{what} needs a snapshot file.\n\nThe ORB state it shows lives inside the process \
                 that holds it, and this tool cannot reach a running server — D024 §7 refuses a \
                 wire interface for administration until the caller model PLAN-DEFERRED §11 is \
                 waiting on exists. Nothing in this workspace writes a snapshot yet; a holding \
                 process writes one with orbweaver_console::orb::Snapshot::live(..).to_json().\
                 \n\n{USAGE}"
            ));
        }
        [file] => {
            let document = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
            orb::Snapshot::read(file, &document)?
        }
        _ => {
            return Err(format!(
                "{what} reads one snapshot; two readings of an ORB merged into one page would be \
                 a page about no moment in particular\n\n{USAGE}"
            ));
        }
    };
    // The unreadable sections go to stderr as well as onto the page: a
    // complaint only a reader of the HTML sees is a complaint a script misses.
    for complaint in &snapshot.complaints {
        eprintln!("note: {complaint}");
    }
    out.deliver(|| html(&snapshot), || text(&snapshot))
}

fn need(args: &mut impl Iterator<Item = String>, what: &str) -> Result<String, String> {
    args.next().filter(|v| !v.is_empty()).ok_or_else(|| format!("{what} needs a value"))
}

/// Loads one root file and everything it includes.
///
/// The resolver's advice — a cycle, a re-inclusion — goes to stderr rather than
/// onto a page: it is a fact about how the estate is stored, not about what an
/// agent may reach, and stdout belongs to the view.
fn load_into(registry: &mut Registry, path: &str, search: &SearchPath) -> Result<(), String> {
    let advice = load::load_into(registry, std::path::Path::new(path), search)?;
    for note in advice {
        eprintln!("note: {note}");
    }
    Ok(())
}
