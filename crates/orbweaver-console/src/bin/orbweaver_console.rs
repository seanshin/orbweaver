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
//! ```
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

use orbweaver_console::{catalog, contract, load, traces};
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
