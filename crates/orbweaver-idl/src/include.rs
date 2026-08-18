//! `#include` resolution: turning a set of files into one translation unit.
//!
//! Until this module existed the lexer skipped `#include` along with every
//! other `#` directive, which meant our front end accepted *IDL* but not *the
//! IDL people have*. Every fixture in `corpus/` is a single self-contained
//! file, so no corpus case could ever exercise a cross-file reference and the
//! gap was invisible by construction; the thirteen-file estate in
//! `spikes/estate/` found it immediately — 11 of 13 files rejected by us and
//! accepted by `omniidl`, with ~90 diagnostics that were all one cause
//! (`docs/pipeline-runs/2026-08-14-estate.md`, RC-1).
//!
//! # What is implemented, and what is refused
//!
//! This is an **include resolver**, not a C preprocessor. Concretely:
//!
//! | directive | what happens |
//! |---|---|
//! | `#include "x.idl"` | resolved relative to the including file, then along the search path |
//! | `#include <x.idl>` | resolved along the search path only |
//! | `#ifndef G` / `#define G` / `#endif` in the include-guard shape | recognised, and ignored — see below |
//! | `#pragma` | passed through; the lexer owns the identity pragmas |
//! | `# 12 "f.idl"` line marker | passed through and ignored, as before |
//! | `#if`, `#ifdef`, `#else`, `#elif`, `#undef`, `#error`, a `#define` with a replacement | **refused** with a diagnostic |
//!
//! The refusal is the point. Conditional compilation cannot be *skipped*
//! safely: ignoring `#ifdef DEBUG` compiles both arms, which is a silent
//! misparse, and this project's rule is that an unmeasured check is a failure
//! rather than a pass. A file that needs macros must be preprocessed first
//! (`cpp`, or `omniidl -E`) and the result handed to us; the diagnostic says
//! so.
//!
//! # Idempotence without requiring guards
//!
//! **A file is spliced at most once per translation unit, keyed by its
//! canonical path.** Guards are therefore not required, and that is
//! deliberate: the estate measured that real IDL in the wild *has* no guards —
//! six of thirteen files were rejected by `omniidl` for exactly that, and the
//! estate's author had to add them. A front end that only works on IDL someone
//! already fixed is not much use on the IDL that motivated it.
//!
//! Including an IDL file twice can never *add* a declaration, only duplicate
//! one, so once-only loses nothing. It does make us laxer than `omniidl`,
//! which is a direction this project is normally suspicious of — so the
//! re-inclusion of an **unguarded** file raises non-blocking advice naming the
//! file and the guard to add. We accept it and say that a deployed compiler
//! will not. The divergence is recorded in `corpus/divergences.tsv`.
//!
//! # `#pragma prefix` does not cross a file boundary
//!
//! This is the trap the estate found the expensive way (RC-4): a file-scope
//! `#pragma prefix` is in force **to the end of its file**, so concatenating
//! files silently hands one file's prefix to the next one's declarations, and
//! both the right and the wrong answer are well-formed repository ids that
//! nothing warns about. Measured against `omniidl` on 2026-08-14 with two
//! probe files and `-Wbinline`:
//!
//! ```text
//! a.idl: #pragma prefix "aaa"     b.idl (no prefix of its own)
//!        #include "b.idl"           module N { interface J ... };
//!        module M { interface I };
//!
//! omniidl:  IDL:aaa/M/I:1.0        IDL:N/J:1.0     <- the includer's prefix does NOT enter
//! ```
//!
//! and the mirror case, where `b.idl` sets `"bbb"`, leaves the includer's
//! declarations on `"aaa"` — the included file's prefix does not escape either.
//! So the rule is a save/restore across the boundary.
//!
//! # The boundary is a marker, not a `#pragma prefix`
//!
//! It was a `#pragma prefix` pair — `""` on the way in, the includer's own
//! string on the way out, injected only when a prefix was in force. That is
//! correct for an `#include` written at **file scope** and wrong for one
//! written inside a module, which is the shape `corpus/include/inc-scope-*.idl`
//! now covers and no file covered before. Two things go wrong at once, and
//! both are the same cause — `#pragma prefix` *replaces* the id path
//! (`corpus/pragma/p02`), so it cannot express either half of a save/restore
//! once the path has anything in it but the prefix:
//!
//! ```text
//!   #pragma prefix "hub.example"        measured 2026-08-18, in-module #include
//!   module Yard {                       omniidl   JacORB 3.9   us (before)
//!     #include "leaf.idl"    Parcel::Tag  IDL:Parcel/…  IDL:hub.example/Yard/Parcel/…  IDL:Parcel/…
//!     interface Gate {…};    Yard::Gate   IDL:hub.example/Yard/Gate:1.0 (both oracles)  IDL:hub.example/Gate:1.0
//!   };
//! ```
//!
//! The restore dropped `Yard`, because `#pragma prefix "hub.example"` written
//! inside `module Yard` *means* "the id path is now `hub.example`". And the
//! entry was skipped whenever no prefix was in force, which left an unprefixed
//! included file inheriting the includer's module — omniidl resets there too.
//!
//! So the pair is now `#pragma orbweaver include-enter` / `include-leave`,
//! injected **unconditionally**, and the parser treats them as a save/restore
//! of the whole id path. A unit whose files carry no prefix is no longer
//! spliced byte-for-byte, and that is the point: the boundary is a boundary
//! whether or not anybody wrote a pragma near it.
//!
//! `#pragma prefix`로는 파일 경계를 표현할 수 없다 — 접두사는 ID 경로를
//! **대체**하므로, 모듈 안에서 인클루드하면 복원이 감싸던 모듈을 잃는다.
//! 이제 경계는 전용 표식이며 조건 없이 삽입된다.
//!
//! Both oracles agree on the **exit**; they disagree on the **entry** (JacORB
//! never resets, so a leaf's ids depend on who included it). We follow
//! omniidl, and `corpus/include/cases.tsv` records why.
//!
//! # Positions
//!
//! A diagnostic's span points into the spliced text, which is not a file
//! anybody has. [`Unit::locate`] maps it back to the file and line it was
//! written on, plus the chain of `#include`s that reached it, and
//! [`Unit::render`] formats that the way a C compiler does — the included
//! file's own line first, the chain underneath. Reporting the *includer's*
//! line instead would make every diagnostic in a large estate point at the
//! same handful of `#include` lines.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::lex::Span;
use crate::sema::Diagnostic;

/// Where `#include` looks, in order.
///
/// The quoted form `#include "x.idl"` tries the including file's own directory
/// first and then these; the angled form `#include <x.idl>` tries only these.
/// That is the C convention, which CORBA IDL inherits, and it is what
/// `omniidl -I` implements.
///
/// Nothing is implicit. In particular the process's working directory is *not*
/// searched: a validator invoked from a build directory would otherwise resolve
/// differently from the same validator invoked from the source tree, and the
/// difference would show up as a repository id rather than as an error.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchPath {
    dirs: Vec<PathBuf>,
}

impl SearchPath {
    /// An empty search path: only the quoted form, relative to its includer,
    /// will resolve.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a directory, as `-I` does.
    pub fn push(&mut self, dir: impl Into<PathBuf>) -> &mut Self {
        self.dirs.push(dir.into());
        self
    }

    /// The directories, in search order.
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }
}

impl<P: Into<PathBuf>> FromIterator<P> for SearchPath {
    fn from_iter<I: IntoIterator<Item = P>>(iter: I) -> Self {
        SearchPath { dirs: iter.into_iter().map(Into::into).collect() }
    }
}

/// One contiguous run of output lines that came from one place.
#[derive(Debug, Clone, Copy)]
struct Seg {
    /// 1-based first line of the run in the spliced text.
    out_line: u32,
    /// Index into [`Unit::files`].
    file: usize,
    /// 1-based line in that file which `out_line` corresponds to.
    in_line: u32,
    /// Index into `Unit::chains`.
    chain: usize,
    /// Whether the run is text we injected rather than text anybody wrote.
    synthetic: bool,
}

/// Where a span was actually written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location<'a> {
    /// The file the text is in.
    pub file: &'a Path,
    /// 1-based line within that file.
    pub line: u32,
    /// 1-based column, which splicing never changes.
    pub column: u32,
    /// The `#include` chain that reached the file, outermost first, as
    /// (including file, line of the `#include`).
    pub chain: Vec<(&'a Path, u32)>,
    /// Whether the position is inside text the resolver injected — a
    /// file-boundary marker — rather than text somebody wrote. A diagnostic
    /// landing here is a bug in this module, and saying so beats pointing at a
    /// line the reader cannot find.
    pub synthetic: bool,
}

/// A translation unit: one root file, everything it included, and the map back.
#[derive(Debug, Clone)]
pub struct Unit {
    /// The spliced source, ready for [`crate::parse`].
    pub text: String,
    /// Every file that contributed, in the order they were first spliced.
    /// Index 0 is the root.
    pub files: Vec<PathBuf>,
    /// Problems that stop the unit from meaning anything: a `#include` that
    /// resolves to nothing, or a directive we refuse to guess at.
    pub errors: Vec<Diagnostic>,
    /// Problems worth saying and not worth failing over: a cycle, or a
    /// re-inclusion that a C preprocessor would have handled differently.
    pub advice: Vec<Diagnostic>,
    map: Vec<Seg>,
    chains: Vec<Vec<(usize, u32)>>,
}

impl Unit {
    /// A unit over text with no origin and nothing included.
    fn bare(text: String, root: PathBuf) -> Self {
        Unit {
            text,
            files: vec![root],
            errors: Vec::new(),
            advice: Vec::new(),
            map: vec![Seg { out_line: 1, file: 0, in_line: 1, chain: 0, synthetic: false }],
            chains: vec![Vec::new()],
        }
    }

    /// Whether the unit can be parsed at all.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Maps a span in [`Unit::text`] back to where it was written.
    pub fn locate(&self, span: Span) -> Location<'_> {
        let line = span.line.max(1);
        let i = match self.map.binary_search_by_key(&line, |s| s.out_line) {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        };
        let seg = self.map.get(i).copied().unwrap_or(Seg {
            out_line: 1,
            file: 0,
            in_line: 1,
            chain: 0,
            synthetic: false,
        });
        Location {
            file: &self.files[seg.file],
            line: seg.in_line + (line - seg.out_line),
            column: span.column,
            chain: self.chains[seg.chain]
                .iter()
                .map(|&(f, l)| (self.files[f].as_path(), l))
                .collect(),
            synthetic: seg.synthetic,
        }
    }

    /// Formats a diagnostic against the files it was written in.
    ///
    /// One line for the diagnostic and one indented line per `#include` that
    /// reached it, innermost last — the shape a C compiler uses, because the
    /// chain is the only thing that makes a diagnostic in a shared header
    /// actionable.
    pub fn render(&self, d: &Diagnostic) -> String {
        let at = self.locate(d.span);
        let mut s =
            format!("{}:{}:{}: {} [{}]", at.file.display(), at.line, at.column, d.message, d.rule);
        if at.synthetic {
            s.push_str("\n    note: in a file-boundary marker inserted by include resolution");
        }
        for (file, line) in at.chain.iter().rev() {
            s.push_str(&format!("\n    included from {}:{}", file.display(), line));
        }
        s
    }
}

/// Resolves `path` and everything it includes into one translation unit.
///
/// The `Err` is reserved for not being able to read the root file at all;
/// anything wrong *inside* the unit comes back in [`Unit::errors`], because a
/// batch tool needs every problem in the set rather than the first one.
pub fn preprocess_file(path: &Path, search: &SearchPath) -> std::io::Result<Unit> {
    let text = std::fs::read_to_string(path)?;
    Ok(preprocess(&text, Some(path), search))
}

/// Resolves text that may or may not have come from a file.
///
/// `origin` is what `#include "x.idl"` is relative to. Without it the quoted
/// form has nothing to be relative to and only the search path can resolve —
/// which is why a `#include` in a string handed to [`crate::check`] is
/// reported rather than skipped.
pub fn preprocess(text: &str, origin: Option<&Path>, search: &SearchPath) -> Unit {
    let root = origin.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("<input>"));
    // The overwhelmingly common case is a single self-contained file. Detecting
    // it keeps the output byte-identical to the input, which matters: callers
    // that slice the *original* source with a diagnostic's span — the fix hints
    // in `orbweaver-forge` do — would otherwise be reading at an offset.
    let mut ctx = Ctx {
        search,
        out: String::with_capacity(text.len()),
        out_line: 1,
        files: vec![root.clone()],
        seen: HashMap::new(),
        included: HashSet::new(),
        stack: Vec::new(),
        chains: vec![Vec::new()],
        chain_index: HashMap::new(),
        map: Vec::new(),
        errors: Vec::new(),
        advice: Vec::new(),
        guarded: HashSet::new(),
        run: None,
    };
    if let Some(c) = canonical(&root) {
        ctx.seen.insert(c.clone(), 0);
        ctx.included.insert(c);
    }
    ctx.chain_index.insert(Vec::new(), 0);
    let dir = origin.and_then(Path::parent).map(Path::to_path_buf);
    ctx.emit(0, text, dir.as_deref());
    ctx.finish(root)
}

// ── the scanner ──────────────────────────────────────────────────────────────

/// The first word of a directive body and the rest, e.g. `("include", "\"x\"")`.
fn split_directive(body: &str) -> (&str, &str) {
    let body = body.trim();
    match body.find(|c: char| c.is_whitespace()) {
        Some(i) => (&body[..i], body[i..].trim()),
        None => (body, ""),
    }
}

/// Extracts a `#include`'s file name, quoted or angled.
fn include_target(rest: &str) -> Option<(String, bool)> {
    let rest = rest.trim();
    if let Some(inner) = rest.strip_prefix('"') {
        let end = inner.find('"')?;
        return Some((inner[..end].to_owned(), false));
    }
    if let Some(inner) = rest.strip_prefix('<') {
        let end = inner.find('>')?;
        return Some((inner[..end].to_owned(), true));
    }
    None
}

/// Whether a line, given the comment state it starts in, is a directive; and
/// what the comment state is at the end of it.
///
/// The comment tracking is why this is not a regex over `^\s*#`: `/* an
/// example: #include "x.idl" */` spanning two lines is a comment, and treating
/// its second line as a directive would splice a file nobody asked for.
fn directive_body<'a>(line: &'a str, in_block: &mut bool) -> Option<&'a str> {
    let starts_code = !*in_block;
    let mut body = None;
    if starts_code {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix('#') {
            body = Some(rest);
        }
    }
    advance_comment_state(line, in_block);
    body
}

/// Walks one line, updating whether a `/* */` is open at the end of it.
fn advance_comment_state(line: &str, in_block: &mut bool) {
    let b = line.as_bytes();
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        if *in_block {
            if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                *in_block = false;
                i += 2;
                continue;
            }
        } else if in_str {
            if b[i] == b'\\' {
                i += 2;
                continue;
            }
            if b[i] == b'"' {
                in_str = false;
            }
        } else {
            match (b[i], b.get(i + 1)) {
                (b'/', Some(&b'/')) => return,
                (b'/', Some(&b'*')) => {
                    *in_block = true;
                    i += 2;
                    continue;
                }
                (b'"', _) => in_str = true,
                _ => {}
            }
        }
        i += 1;
    }
}

/// Classifies every `#` line of a file, and decides whether it carries a guard.
///
/// Returns the guard's macro name, if the file opens with the
/// `#ifndef G` / `#define G` idiom and uses no other conditional.
fn classify(text: &str) -> (Option<String>, Vec<(u32, String)>) {
    let mut in_block = false;
    let mut directives: Vec<(u32, String, String)> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if let Some(body) = directive_body(line, &mut in_block) {
            let (word, rest) = split_directive(body);
            directives.push((n as u32 + 1, word.to_owned(), rest.to_owned()));
        }
    }

    let conditional = |w: &str| {
        matches!(w, "if" | "ifdef" | "ifndef" | "elif" | "else" | "endif" | "elifdef" | "elifndef")
    };

    // The guard: the file's first two directives are `#ifndef G` and
    // `#define G`, and the only other conditional in the file is the `#endif`
    // that closes it. Anything richer is conditional compilation, and this
    // module refuses conditional compilation rather than guessing an arm.
    let mut guard = None;
    let mut guard_lines: Vec<u32> = Vec::new();
    if directives.len() >= 3
        && directives[0].1 == "ifndef"
        && directives[1].1 == "define"
        && !directives[0].2.is_empty()
        && directives[1].2 == directives[0].2
    {
        let closers: Vec<u32> =
            directives.iter().filter(|(_, w, _)| conditional(w)).map(|(l, _, _)| *l).collect();
        // ifndef + endif, and nothing else conditional in between.
        if closers.len() == 2
            && directives.iter().any(|(l, w, _)| *w == "endif" && *l == closers[1])
        {
            guard = Some(directives[0].2.clone());
            guard_lines = vec![directives[0].0, directives[1].0, closers[1]];
        }
    }

    let mut refused = Vec::new();
    for (line, word, rest) in &directives {
        if guard_lines.contains(line) {
            continue;
        }
        let why = match word.as_str() {
            "include" | "pragma" => continue,
            // `# 12 "f.idl"` — a line marker from something that already ran
            // the preprocessor. Ignored, as it was before this module existed.
            w if w.chars().next().is_some_and(|c| c.is_ascii_digit()) => continue,
            "" => continue,
            "define" if rest.split_whitespace().count() == 1 => continue,
            "define" => format!(
                "`#define {rest}` defines a macro with a replacement, and macro expansion is \
                 not implemented"
            ),
            "undef" => "`#undef` is not implemented".to_owned(),
            "error" => format!("`#error {rest}`"),
            // A lone `#endif` says nothing on its own, and the `#if` it closes
            // is reported already. Refusing both turns one refusal into two
            // and makes a file with three conditionals read like six problems.
            "endif" => continue,
            w if conditional(w) => format!(
                "`#{w}` is conditional compilation, which is not implemented — ignoring it \
                 would compile every arm at once, which is a silent misparse"
            ),
            w => format!("`#{w}` is not a directive this front end understands"),
        };
        refused.push((
            *line,
            format!(
                "{why}. Run the file through a C preprocessor first \
                 (`cpp -P`, or `omniidl -E`) and validate its output; \
                 `#include`, `#pragma` and the `#ifndef`/`#define`/`#endif` include-guard \
                 idiom are the directives handled here."
            ),
        ));
    }
    (guard, refused)
}

// ── the splicer ──────────────────────────────────────────────────────────────

struct Ctx<'a> {
    search: &'a SearchPath,
    out: String,
    out_line: u32,
    files: Vec<PathBuf>,
    seen: HashMap<PathBuf, usize>,
    included: HashSet<PathBuf>,
    stack: Vec<(usize, u32)>,
    chains: Vec<Vec<(usize, u32)>>,
    chain_index: HashMap<Vec<(usize, u32)>, usize>,
    map: Vec<Seg>,
    errors: Vec<Diagnostic>,
    advice: Vec<Diagnostic>,
    guarded: HashSet<PathBuf>,
    /// The run of lines currently being extended, so the map holds one entry
    /// per contiguous stretch rather than one per line.
    run: Option<(usize, u32, usize, bool)>,
}

/// A canonical path, or `None` when the file system will not say.
fn canonical(p: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(p).ok()
}

impl Ctx<'_> {
    fn chain_id(&mut self) -> usize {
        if let Some(&i) = self.chain_index.get(&self.stack) {
            return i;
        }
        let i = self.chains.len();
        self.chains.push(self.stack.clone());
        self.chain_index.insert(self.stack.clone(), i);
        i
    }

    /// A zero-width span at a line of the output.
    ///
    /// Zero-width because a directive line is not in the output as itself —
    /// see [`Ctx::push_directive_stub`] — and a span with a width would slice
    /// text that is not the directive's.
    fn span_at(&self, out_line: u32, column: u32) -> Span {
        Span { start: self.out.len(), end: self.out.len(), line: out_line, column }
    }

    fn note(&mut self, span: Span, message: String, rule: &'static str, fatal: bool) {
        let d = Diagnostic { message, span, rule };
        if fatal { self.errors.push(d) } else { self.advice.push(d) }
    }

    /// Appends one source line, keeping the map's run going where it can.
    fn push_line(&mut self, line: &str, file: usize, in_line: u32, synthetic: bool) {
        let chain = self.chain_id();
        let extend = matches!(self.run, Some((f, next, c, s))
            if f == file && c == chain && s == synthetic && next == in_line);
        if !extend {
            self.map.push(Seg { out_line: self.out_line, file, in_line, chain, synthetic });
        }
        self.run = Some((file, in_line + 1, chain, synthetic));
        self.out.push_str(line);
        if !line.ends_with('\n') {
            self.out.push('\n');
        }
        self.out_line += 1;
    }

    /// Replaces a `#include` line with an inert comment recording it.
    ///
    /// One output line per source line is what keeps [`Unit::locate`] a simple
    /// affine map, and dropping the line instead cost two `#include`s in the
    /// same file the same reported position. The comment rather than the
    /// original text because the point of `-E` is a unit somebody else can
    /// compile, and a resolved unit that still says `#include` would make a
    /// second compiler resolve it a second time.
    fn push_directive_stub(&mut self, directive: &str, file: usize, in_line: u32, note: &str) {
        let text = format!("// [orbweaver-idl] {} — {note}\n", directive.trim());
        self.push_line(&text, file, in_line, false);
    }

    /// Injects a line nobody wrote, attributed to the `#include` that caused it.
    fn push_synthetic(&mut self, line: &str, file: usize, at: u32) {
        self.run = None;
        self.push_line(line, file, at, true);
        self.run = None;
    }

    /// Registers a file and returns its index in [`Unit::files`].
    fn intern(&mut self, path: &Path, canon: Option<&Path>) -> usize {
        if let Some(c) = canon
            && let Some(&i) = self.seen.get(c)
        {
            return i;
        }
        let i = self.files.len();
        self.files.push(path.to_path_buf());
        if let Some(c) = canon {
            self.seen.insert(c.to_path_buf(), i);
        }
        i
    }

    /// Finds the file a `#include` names, or reports where it looked.
    fn resolve(&self, name: &str, angled: bool, dir: Option<&Path>) -> Result<PathBuf, String> {
        let mut tried: Vec<String> = Vec::new();
        let candidate = Path::new(name);
        if candidate.is_absolute() {
            if candidate.is_file() {
                return Ok(candidate.to_path_buf());
            }
            tried.push(candidate.display().to_string());
        } else {
            if !angled {
                match dir {
                    Some(d) => {
                        let p = d.join(name);
                        if p.is_file() {
                            return Ok(p);
                        }
                        tried.push(p.display().to_string());
                    }
                    None => tried.push(
                        "the including file's own directory (unknown: this source was supplied \
                         as text, not read from a file)"
                            .to_owned(),
                    ),
                }
            }
            for d in self.search.dirs() {
                let p = d.join(name);
                if p.is_file() {
                    return Ok(p);
                }
                tried.push(p.display().to_string());
            }
        }
        Err(if tried.is_empty() {
            "Nothing was searched: there is no include path and no including file to be \
             relative to. Pass the directory with `-I`."
                .to_owned()
        } else {
            format!(
                "Searched, in order:\n      {}\n    Add the directory holding it with `-I`.",
                tried.join("\n      ")
            )
        })
    }

    /// Splices one file's text, recursing through its includes.
    fn emit(&mut self, file: usize, text: &str, dir: Option<&Path>) {
        let (_, refused) = classify(text);
        let mut in_block = false;

        for (n, line) in text.split_inclusive('\n').enumerate() {
            let lineno = n as u32 + 1;
            if let Some((_, why)) = refused.iter().find(|(l, _)| *l == lineno) {
                let span = self.span_at(self.out_line, 1);
                self.note(span, why.clone(), "unsupported-directive", true);
            }
            let stripped = line.strip_suffix('\n').unwrap_or(line);
            let Some(body) = directive_body(stripped, &mut in_block) else {
                self.push_line(line, file, lineno, false);
                continue;
            };
            let (word, rest) = split_directive(body);
            // Every other directive — `#pragma` included — is passed straight
            // through. The identity pragmas belong to the lexer, and this
            // module no longer tracks the prefix in force at all: the file
            // boundary is a marker the parser acts on, not a prefix this
            // module recomputes.
            if word != "include" {
                self.push_line(line, file, lineno, false);
                continue;
            }

            let column = stripped.len() as u32 - stripped.trim_start().len() as u32 + 1;
            // The stub goes in first so the directive keeps an output line of
            // its own, which is what makes its diagnostic point at *this*
            // `#include` and not at the previous one.
            let out_line = self.out_line;
            let Some((name, angled)) = include_target(rest) else {
                self.push_directive_stub(stripped, file, lineno, "not a file name");
                let span = self.span_at(out_line, column);
                self.note(
                    span,
                    format!(
                        "`#include{rest}` names no file. Write `#include \"x.idl\"` for a file \
                         beside this one, or `#include <x.idl>` for one on the include path."
                    ),
                    "include-malformed",
                    true,
                );
                continue;
            };
            self.emit_include(&name, angled, dir, file, lineno, column, out_line, stripped);
        }
    }

    /// Handles one `#include`.
    #[allow(clippy::too_many_arguments)]
    fn emit_include(
        &mut self,
        name: &str,
        angled: bool,
        dir: Option<&Path>,
        from: usize,
        at: u32,
        column: u32,
        out_line: u32,
        directive: &str,
    ) {
        let path = match self.resolve(name, angled, dir) {
            Ok(p) => p,
            Err(where_it_looked) => {
                self.push_directive_stub(directive, from, at, "unresolved");
                let span = self.span_at(out_line, column);
                self.note(
                    span,
                    format!("`#include \"{name}\"` resolves to no file. {where_it_looked}"),
                    "include-not-found",
                    true,
                );
                return;
            }
        };
        let canon = canonical(&path);

        // A cycle. Terminates by not re-entering, and says so: an include graph
        // with a loop in it is nearly always a mistake, but a guarded one is
        // legal IDL that `omniidl` compiles, so this is advice and not an error.
        if let Some(c) = &canon
            && let Some(&i) = self.seen.get(c)
            && self.stack.iter().any(|&(f, _)| f == i)
        {
            let mut chain: Vec<String> =
                self.stack.iter().map(|&(f, _)| self.files[f].display().to_string()).collect();
            chain.push(self.files[from].display().to_string());
            chain.push(path.display().to_string());
            self.push_directive_stub(directive, from, at, "cycle, not followed");
            let span = self.span_at(out_line, column);
            self.note(
                span,
                format!(
                    "`#include \"{name}\"` closes a cycle: {}. It is included once and the \
                     cycle is not followed; break the loop or accept that the order of \
                     declarations depends on which file is compiled first.",
                    chain.join(" -> ")
                ),
                "include-cycle",
                false,
            );
            return;
        }

        // Already spliced. Silent when the file carries a guard, because that is
        // exactly what the guard is for; advice when it does not, because a
        // deployed compiler will include it twice and reject the result.
        if let Some(c) = &canon
            && self.included.contains(c)
        {
            self.push_directive_stub(directive, from, at, "already included");
            if !self.guarded.contains(c) {
                let span = self.span_at(out_line, column);
                self.note(
                    span,
                    format!(
                        "`{}` was already included in this unit and has no `#ifndef`/`#define` \
                         include guard. We splice it once; a C preprocessor does not, so \
                         `omniidl` and every deployed compiler will report every declaration \
                         in it as a duplicate. Add a guard.",
                        path.display()
                    ),
                    "include-unguarded-repeat",
                    false,
                );
            }
            return;
        }

        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                self.push_directive_stub(directive, from, at, "unreadable");
                let span = self.span_at(out_line, column);
                self.note(
                    span,
                    format!(
                        "`#include \"{name}\"` found {} but cannot read it: {e}",
                        path.display()
                    ),
                    "include-unreadable",
                    true,
                );
                return;
            }
        };

        self.push_directive_stub(directive, from, at, &format!("resolved to {}", path.display()));
        let idx = self.intern(&path, canon.as_deref());
        if let Some(c) = &canon {
            self.included.insert(c.clone());
        }
        let (guard, _) = classify(&text);
        if guard.is_some()
            && let Some(c) = &canon
        {
            self.guarded.insert(c.clone());
        }

        // Enter the included file at the empty id path, which is the state it
        // would have begun in had it been compiled on its own, and leave it
        // with the includer's path — prefix *and* enclosing modules — put back.
        // Both markers are unconditional, and both are markers rather than
        // `#pragma prefix` lines; the module docs say why neither shortcut
        // survives an `#include` written inside a module.
        self.push_synthetic("#pragma orbweaver include-enter\n", from, at);
        self.stack.push((from, at));
        let sub_dir = path.parent().map(Path::to_path_buf);
        self.emit(idx, &text, sub_dir.as_deref());
        self.stack.pop();
        self.run = None;
        self.push_synthetic("#pragma orbweaver include-leave\n", from, at);
    }

    fn finish(self, root: PathBuf) -> Unit {
        let Ctx { out, files, errors, advice, map, chains, .. } = self;
        let mut u = Unit::bare(out, root);
        u.files = files;
        u.errors = errors;
        u.advice = advice;
        if !map.is_empty() {
            u.map = map;
        }
        u.chains = chains;
        u
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("orbweaver-include-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("temp dir");
        d
    }

    fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, text).expect("write");
        p
    }

    /// The case that motivated the module: a name declared in an included file.
    #[test]
    fn an_included_declaration_resolves() {
        let d = tmp("basic");
        write(&d, "common.idl", "typedef string ISODate;\n");
        let a = write(
            &d,
            "a.idl",
            "#include \"common.idl\"\nmodule M { struct S { ISODate at; }; };\n",
        );
        let u = preprocess_file(&a, &SearchPath::new()).expect("read");
        assert!(u.is_ok(), "{:?}", u.errors);
        assert!(u.text.contains("typedef string ISODate;"));
        assert!(crate::check(&u.text).is_ok(), "{:?}", crate::check(&u.text).err());
    }

    /// Idempotence without guards, which is what real IDL looks like.
    #[test]
    fn the_same_file_through_two_paths_is_spliced_once() {
        let d = tmp("twice");
        write(&d, "c.idl", "typedef string ISODate;\n");
        write(&d, "b.idl", "#include \"c.idl\"\ntypedef sequence<string> Names;\n");
        let a = write(
            &d,
            "a.idl",
            "#include \"c.idl\"\n#include \"b.idl\"\nmodule M { struct S { ISODate at; Names who; }; };\n",
        );
        let u = preprocess_file(&a, &SearchPath::new()).expect("read");
        assert!(u.is_ok(), "{:?}", u.errors);
        assert_eq!(u.text.matches("typedef string ISODate;").count(), 1);
        assert!(crate::check(&u.text).is_ok());
        // Laxer than omniidl, and it says so rather than being quiet about it.
        assert_eq!(u.advice.len(), 1, "{:?}", u.advice);
        assert_eq!(u.advice[0].rule, "include-unguarded-repeat");
    }

    /// A guarded file is skipped without a word, because that is what the
    /// guard means.
    #[test]
    fn a_guarded_repeat_is_silent() {
        let d = tmp("guarded");
        write(&d, "c.idl", "#ifndef C_IDL\n#define C_IDL\ntypedef string ISODate;\n#endif\n");
        write(&d, "b.idl", "#include \"c.idl\"\ntypedef long Count;\n");
        let a = write(&d, "a.idl", "#include \"c.idl\"\n#include \"b.idl\"\n");
        let u = preprocess_file(&a, &SearchPath::new()).expect("read");
        assert!(u.is_ok(), "{:?}", u.errors);
        assert!(u.advice.is_empty(), "{:?}", u.advice);
        assert_eq!(u.text.matches("typedef string ISODate;").count(), 1);
    }

    /// Terminates, and names the loop rather than overflowing the stack.
    #[test]
    fn a_cycle_terminates_and_is_named() {
        let d = tmp("cycle");
        write(&d, "b.idl", "#include \"a.idl\"\ntypedef long B;\n");
        let a = write(&d, "a.idl", "#include \"b.idl\"\ntypedef long A;\n");
        let u = preprocess_file(&a, &SearchPath::new()).expect("read");
        assert!(u.is_ok(), "{:?}", u.errors);
        let cycle: Vec<_> = u.advice.iter().filter(|d| d.rule == "include-cycle").collect();
        assert_eq!(cycle.len(), 1, "{:?}", u.advice);
        let arrows = cycle[0].message.matches(" -> ").count();
        assert_eq!(arrows, 2, "the chain must name every file in the loop: {}", cycle[0].message);
        assert!(cycle[0].message.contains("b.idl"), "{}", cycle[0].message);
    }

    /// A three-file cycle, because a two-file one can be terminated by accident.
    #[test]
    fn a_longer_cycle_also_terminates() {
        let d = tmp("cycle3");
        write(&d, "c.idl", "#include \"a.idl\"\ntypedef long C;\n");
        write(&d, "b.idl", "#include \"c.idl\"\ntypedef long B;\n");
        let a = write(&d, "a.idl", "#include \"b.idl\"\ntypedef long A;\n");
        let u = preprocess_file(&a, &SearchPath::new()).expect("read");
        assert!(u.is_ok(), "{:?}", u.errors);
        assert!(u.advice.iter().any(|d| d.rule == "include-cycle"), "{:?}", u.advice);
        assert!(crate::check(&u.text).is_ok());
    }

    /// A missing include is a diagnostic listing where we looked. Silence here
    /// is the defect this module exists to remove.
    #[test]
    fn a_missing_include_lists_the_paths_searched() {
        let d = tmp("missing");
        let a = write(&d, "a.idl", "#include \"nope.idl\"\n");
        let mut sp = SearchPath::new();
        sp.push(d.join("elsewhere"));
        let u = preprocess_file(&a, &sp).expect("read");
        assert!(!u.is_ok());
        assert_eq!(u.errors[0].rule, "include-not-found");
        assert!(u.errors[0].message.contains("nope.idl"), "{}", u.errors[0].message);
        assert!(u.errors[0].message.contains("elsewhere"), "{}", u.errors[0].message);
    }

    /// The angled form does not look beside the including file.
    #[test]
    fn angled_and_quoted_forms_search_differently() {
        let d = tmp("angled");
        std::fs::create_dir_all(d.join("inc")).expect("mkdir");
        write(&d, "beside.idl", "typedef long Beside;\n");
        write(&d.join("inc"), "onpath.idl", "typedef long OnPath;\n");

        let a = write(&d, "a.idl", "#include <beside.idl>\n");
        let mut sp = SearchPath::new();
        sp.push(d.join("inc"));
        let u = preprocess_file(&a, &sp).expect("read");
        assert_eq!(u.errors.len(), 1, "angled must not find a sibling: {:?}", u.errors);

        let b = write(&d, "b.idl", "#include <onpath.idl>\n#include \"beside.idl\"\n");
        let u = preprocess_file(&b, &sp).expect("read");
        assert!(u.is_ok(), "{:?}", u.errors);
        assert!(u.text.contains("OnPath") && u.text.contains("Beside"));
    }

    /// The prefix rule, in the shape the estate found it: an included file
    /// starts with the empty prefix and the includer gets its own back.
    #[test]
    fn a_file_scope_prefix_does_not_cross_an_include() {
        let d = tmp("prefix");
        write(&d, "plain.idl", "module N { interface J { void ping(); }; };\n");
        let a = write(
            &d,
            "a.idl",
            "#pragma prefix \"aaa\"\n#include \"plain.idl\"\nmodule M { interface I { void ping(); }; };\n",
        );
        let u = preprocess_file(&a, &SearchPath::new()).expect("read");
        assert!(u.is_ok(), "{:?}", u.errors);
        let spec = crate::parse(&u.text).expect("parses");
        assert_eq!(spec.repository_ids.get("M::I").map(String::as_str), Some("IDL:aaa/M/I:1.0"));
        assert_eq!(spec.repository_ids.get("N::J"), None, "the includer's prefix must not enter");
    }

    /// ...and the mirror: an included file's prefix does not escape into the
    /// includer's declarations, which is the drift `amalgamate.py` measured.
    #[test]
    fn an_included_prefix_does_not_escape_into_the_includer() {
        let d = tmp("prefix2");
        write(
            &d,
            "pref.idl",
            "#pragma prefix \"bbb\"\nmodule P { interface K { void ping(); }; };\n",
        );
        let a = write(
            &d,
            "a.idl",
            "#include \"pref.idl\"\nmodule Q { interface L { void ping(); }; };\n",
        );
        let u = preprocess_file(&a, &SearchPath::new()).expect("read");
        assert!(u.is_ok(), "{:?}", u.errors);
        let spec = crate::parse(&u.text).expect("parses");
        assert_eq!(spec.repository_ids.get("P::K").map(String::as_str), Some("IDL:bbb/P/K:1.0"));
        assert_eq!(spec.repository_ids.get("Q::L"), None, "Q::L must keep the empty prefix");
    }

    /// The boundary saves and restores the **whole id path**, not the prefix
    /// part of it.
    ///
    /// The `#include` is inside `module Yard`, which is the shape that separates
    /// the two readings. A restore written as `#pragma prefix "aaa"` gives
    /// `Yard::I` the id `IDL:aaa/I:1.0`, because a prefix *replaces* the path;
    /// both omniidl 4.3.4 and JacORB 3.9 say `IDL:aaa/Yard/I:1.0`.
    #[test]
    fn an_in_module_include_restores_the_enclosing_module_too() {
        let d = tmp("inmodule");
        write(&d, "leaf.idl", "module N { interface J { void ping(); }; };\n");
        let a = write(
            &d,
            "a.idl",
            "#pragma prefix \"aaa\"\nmodule Yard {\n#include \"leaf.idl\"\n\
             interface I { void ping(); };\n};\n",
        );
        let u = preprocess_file(&a, &SearchPath::new()).expect("read");
        assert!(u.is_ok(), "{:?}", u.errors);
        let spec = crate::parse(&u.text).expect("parses");
        assert_eq!(
            spec.repository_ids.get("Yard::I").map(String::as_str),
            Some("IDL:aaa/Yard/I:1.0"),
            "the restore must put back the modules the #include sat inside"
        );
        // And the leaf starts from the empty path, as it would compiled alone —
        // `Yard` is absent from the id even though the declaration is nested
        // inside it, which is what makes the id differ from the plain
        // derivation and therefore get recorded at all.
        assert_eq!(spec.repository_ids.get("Yard::N::J").map(String::as_str), Some("IDL:N/J:1.0"));
    }

    /// The boundary is unconditional: no prefix anywhere and the included
    /// file still starts at the empty id path.
    ///
    /// This is where the two oracles part company. omniidl resets at a file
    /// boundary whether or not a prefix is in play; JacORB 3.9 resets nothing,
    /// which makes a leaf's identity depend on which root reached it. We follow
    /// omniidl — see `corpus/include/inc-scope-control.idl`.
    #[test]
    fn the_boundary_applies_even_when_no_prefix_is_in_play() {
        let d = tmp("noprefix");
        write(&d, "leaf.idl", "module N { interface J { void ping(); }; };\n");
        let a = write(&d, "a.idl", "module Led {\n#include \"leaf.idl\"\n};\n");
        let u = preprocess_file(&a, &SearchPath::new()).expect("read");
        assert!(u.is_ok(), "{:?}", u.errors);
        let spec = crate::parse(&u.text).expect("parses");
        assert_eq!(
            spec.repository_ids.get("Led::N::J").map(String::as_str),
            Some("IDL:N/J:1.0"),
            "an included file must not inherit the module its #include was written in"
        );
    }

    /// Conditional compilation is refused, not skipped: skipping it compiles
    /// both arms.
    #[test]
    fn conditional_compilation_is_refused() {
        let d = tmp("cond");
        let a = write(&d, "a.idl", "#ifdef DEBUG\ntypedef long D;\n#endif\ntypedef long E;\n");
        let u = preprocess_file(&a, &SearchPath::new()).expect("read");
        assert!(!u.is_ok());
        assert_eq!(u.errors[0].rule, "unsupported-directive");
        assert!(u.errors[0].message.contains("cpp -P"), "{}", u.errors[0].message);
    }

    /// A macro with a replacement is refused for the same reason; a guard's
    /// bare `#define` is not.
    #[test]
    fn a_macro_with_a_replacement_is_refused_and_a_guard_is_not() {
        let d = tmp("define");
        let a = write(&d, "a.idl", "#define MAX 10\nconst long L = 1;\n");
        let u = preprocess_file(&a, &SearchPath::new()).expect("read");
        assert!(!u.is_ok(), "a replacement macro must be refused");

        let b = write(&d, "b.idl", "#ifndef B_IDL\n#define B_IDL\nconst long L = 1;\n#endif\n");
        let u = preprocess_file(&b, &SearchPath::new()).expect("read");
        assert!(u.is_ok(), "{:?}", u.errors);
    }

    /// A `#include` written inside a block comment is a comment.
    #[test]
    fn an_include_inside_a_comment_is_not_a_directive() {
        let d = tmp("comment");
        let a = write(&d, "a.idl", "/* example:\n#include \"nope.idl\"\n*/\ntypedef long L;\n");
        let u = preprocess_file(&a, &SearchPath::new()).expect("read");
        assert!(u.is_ok(), "{:?}", u.errors);
    }

    /// A diagnostic in an included file names that file, its own line, and the
    /// chain — not the includer's `#include` line.
    #[test]
    fn a_position_maps_back_through_the_chain() {
        let d = tmp("map");
        write(&d, "deep.idl", "// a comment\nmodule Z { struct S { Nope n; }; };\n");
        write(&d, "mid.idl", "#include \"deep.idl\"\n");
        let a = write(&d, "a.idl", "// one\n// two\n#include \"mid.idl\"\ntypedef long L;\n");
        let u = preprocess_file(&a, &SearchPath::new()).expect("read");
        assert!(u.is_ok(), "{:?}", u.errors);
        let diags = crate::check(&u.text).expect_err("Nope is undeclared");
        let at = u.locate(diags[0].span);
        assert_eq!(at.file.file_name().unwrap(), "deep.idl");
        assert_eq!(at.line, 2, "the included file's own line");
        let chain: Vec<_> =
            at.chain.iter().map(|(f, l)| (f.file_name().unwrap().to_owned(), *l)).collect();
        assert_eq!(chain, vec![("a.idl".into(), 3), ("mid.idl".into(), 1)]);
        let rendered = u.render(&diags[0]);
        assert!(rendered.contains("deep.idl:2:"), "{rendered}");
        assert!(rendered.contains("included from"), "{rendered}");
    }

    /// The identity property the rest of the tree depends on: a file with
    /// nothing to resolve comes out byte-for-byte, so a span still indexes the
    /// original source.
    #[test]
    fn a_self_contained_file_is_unchanged() {
        let src = "#pragma prefix \"acme.com\"\nmodule M {\n  interface I { void ping(); };\n};\n";
        let u = preprocess(src, None, &SearchPath::new());
        assert_eq!(u.text, src);
        assert!(u.is_ok() && u.advice.is_empty());
    }

    /// A `#include` in text with no origin cannot resolve, and says why rather
    /// than being skipped — the silent skip is the whole defect.
    #[test]
    fn a_string_with_an_include_says_it_cannot_resolve_it() {
        let u = preprocess("#include \"common.idl\"\nmodule M {};\n", None, &SearchPath::new());
        assert!(!u.is_ok());
        assert_eq!(u.errors[0].rule, "include-not-found");
        assert!(
            u.errors[0].message.contains("supplied as text"),
            "the reason must name the missing origin: {}",
            u.errors[0].message
        );
        assert!(u.errors[0].message.contains("-I"), "{}", u.errors[0].message);
    }
}
