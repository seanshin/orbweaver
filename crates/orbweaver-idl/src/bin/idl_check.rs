//! Checks IDL files and reports diagnostics.
//!
//! Replaces `spikes/idl_lint.py`, which approximated the identifier rules with
//! regexes and missed a new shape of them twice. This walks a real scope tree,
//! so the rule is expressed once rather than re-approximated per syntax form —
//! and it also catches what the regex never could: unknown names, duplicate
//! declarations, inherited collisions and repeated union labels.
//!
//! Usage: `idl-check <file.idl>...`
//! Exit code is the number of files with diagnostics.

fn main() -> std::process::ExitCode {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("usage: idl-check <file.idl>...");
        return std::process::ExitCode::from(2);
    }
    let mut bad = 0u8;
    for f in &files {
        let src = match std::fs::read_to_string(f) {
            Ok(s) => s,
            Err(e) => {
                println!("{f}: cannot read: {e}");
                bad = bad.saturating_add(1);
                continue;
            }
        };
        if let Err(diags) = orbweaver_idl::check(&src) {
            bad = bad.saturating_add(1);
            for d in diags {
                println!("{f}:{d}");
            }
        }
    }
    std::process::ExitCode::from(bad)
}
