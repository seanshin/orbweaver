//! Tokenizer for OMG IDL 4.2, with SIDL structured comments preserved.
//!
//! Comments are normally noise a lexer drops. Here they are not: Phase 0
//! established that deployed IDL compilers reject IDL 4 `@annotation`, so SIDL
//! carries its semantics in `//@ key: value` comments instead
//! (`docs/PHASE0.md`, assumption C). Discarding comments would discard the
//! meaning layer the whole project is built on.
//!
//! # `#pragma` is not a comment either
//!
//! The three identity pragmas — `prefix`, `version` and `ID` — decide what a
//! repository id *is*, and a repository id is identity on the wire. Skipping
//! them, which this lexer did until the pragma batch, made us disagree with
//! every deployment that uses a prefix while looking perfectly correct
//! locally. They are lifted out here into [`Pragma`] and attached to the
//! following token, the same mechanism SIDL annotations use, so the parser
//! sees them in source order without the grammar having to admit `#`.
//!
//! Every other `#` directive is skipped **here**: the preprocessor's
//! `# 12 "file.idl"` line markers, an include guard's `#ifndef`/`#define`, and
//! any `#pragma` whose name we do not recognise ([`Pragma::Other`], kept so a
//! reader can see it was seen and ignored rather than mistaken for a comment).
//!
//! `#include` used to be in that list, and being in it was a defect rather than
//! a simplification: a type declared one file away was simply an unknown name,
//! which rejected 11 of the 13 files of the estate in `spikes/estate/` that
//! `omniidl` accepts. It is resolved before the lexer runs now — see
//! [`crate::include`], which also refuses the conditional directives rather
//! than skipping them, because skipping `#ifdef` compiles every arm at once.
//! By the time a token stream exists, an `#include` line has already been
//! replaced by the file it named.

use std::fmt;

/// Where a token sits in the source, for diagnostics the self-repair loop can
/// act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Byte offset of the first character.
    pub start: usize,
    /// Byte offset one past the last character.
    pub end: usize,
    /// 1-based line number of `start`.
    pub line: u32,
    /// 1-based column of `start`, in characters.
    pub column: u32,
}

impl Span {
    /// A zero-width span, for synthesised nodes.
    pub const fn empty() -> Self {
        Span { start: 0, end: 0, line: 0, column: 0 }
    }
}

/// A SIDL annotation lifted out of a `//@ key: value` comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotation {
    /// The key, without the `ai_` prefix stripped — the vocabulary owns that.
    pub key: String,
    /// Everything after the first colon, trimmed.
    pub value: String,
    /// Where the comment was.
    pub span: Span,
}

/// A `#pragma` that participates in repository-id derivation.
///
/// Only the three the specification gives identity meaning to are modelled.
/// Everything else keeps its text in [`Pragma::Other`] and has no effect —
/// see the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pragma {
    /// `#pragma prefix "P"`. `None` is the `""` form, which resets the prefix
    /// to none rather than prepending an empty segment.
    Prefix(Option<String>),
    /// `#pragma version <name> <major>.<minor>`, naming an already-declared
    /// item in scope.
    Version {
        /// The item named, as written — possibly a scoped name.
        name: String,
        /// Major version.
        major: u32,
        /// Minor version.
        minor: u32,
    },
    /// `#pragma ID <name> "IDL:..."`, an explicit id that overrides derivation.
    Id {
        /// The item named, as written — possibly a scoped name.
        name: String,
        /// The id, verbatim. Not validated here: an id we do not recognise is
        /// still what the author means to put on the wire.
        id: String,
    },
    /// The first line of an included file's text, inserted by include
    /// resolution and written by nobody.
    ///
    /// It marks a **translation-unit file boundary**, which is not the same
    /// instruction as `Prefix(None)` even though the two coincide at file
    /// scope. Crossing into an included file saves the whole id path in force
    /// — prefix *and* enclosing scopes — and starts the file at the empty one,
    /// which is the state it would have had if compiled alone.
    ///
    /// 인클루드 파일의 시작을 나타내는, 리졸버가 삽입한 표식. 파일 스코프에서는
    /// `Prefix(None)`과 결과가 같지만 모듈 안에서는 다르다.
    IncludeEnter,
    /// The matching end of one, which restores what [`Pragma::IncludeEnter`]
    /// saved. There is no `#pragma prefix` spelling for this: a prefix
    /// *replaces* the id path, so it can name the includer's prefix but never
    /// the enclosing modules the includer was inside.
    IncludeLeave,
    /// A `#pragma` we recognise as a pragma and nothing more.
    Other(String),
}

/// A pragma together with where it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PragmaAt {
    /// What it says.
    pub pragma: Pragma,
    /// Where it was written, for diagnostics.
    pub span: Span,
}

/// A fixed-point literal, kept as the decimal it was written as.
///
/// # Why this is not an `f64`
///
/// `9.9` is not representable in binary floating point: the nearest `f64` is
/// 9.9000000000000003552713678800500929355621337890625. For a `double` that is
/// the answer, because a `double` *is* a binary float and the author asked for
/// one. For a `fixed` it is a wrong answer to a question nobody asked — the
/// author wrote a decimal, IDL's `fixed` **is** a decimal, and the only reason
/// to round it is that the lexer reached for the nearest Rust type.
///
/// The loss is silent and it is upstream of everything: the registry, the IFR,
/// the console catalogue, both generators' emitters and `idl-diff`'s §5.3
/// comparison all read what the lexer decided. §4.4 keeps a `fixed` **value**
/// off the wire, so nothing here is a marshalling question; a constant's value
/// is part of what a released contract promises, and a differ that cannot tell
/// `9.9d` from `9.9000000000000004` cannot report that promise changing.
///
/// *`fixed`는 십진수다. 렉서가 f64로 접으면 값은 아무것도 실행되기 전에 이미
/// 틀린다.*
///
/// # The normal form, taken from the oracle
///
/// The value is `unscaled / 10^scale`, with `unscaled` unsigned: a leading `-`
/// is IDL's unary operator, not part of the literal. Both fields are normalised
/// exactly as omniidl 4.3.4 normalises them, measured 2026-08-21 by reading
/// back its own `-b dump`:
///
/// ```text
/// 9.9d      -> 9.9d       0.0d    -> 0d      100.d -> 100d
/// 9.90d     -> 9.9d       00.10d  -> 0.1d    .5d   -> 0.5d
/// 0.10d     -> 0.1d       000…01d -> 1d
/// ```
///
/// So trailing fractional zeros and leading integer zeros are **not** part of
/// the value: `9.9d` and `9.90d` are the same constant, and a differ must not
/// report a change between them. That was worth measuring rather than
/// assuming — the batch that produced this type was briefed the other way
/// round, and the oracle said otherwise on the first query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedLit {
    /// The digits as one integer, with the point removed. Unsigned: sign is
    /// an operator applied to this.
    pub unscaled: u128,
    /// How many of those digits fall to the right of the point.
    pub scale: u16,
}

/// The most significant digits a `fixed` may carry (CORBA 3.4 §7.11.3).
pub const FIXED_MAX_DIGITS: u32 = 31;

impl FixedLit {
    /// Builds one from the literal text with the `d`/`D` suffix already gone,
    /// in the normal form above. `None` when the digits do not fit.
    fn parse(text: &str) -> Option<Self> {
        let (int_part, frac_part) = match text.split_once('.') {
            Some((i, f)) => (i, f),
            None => (text, ""),
        };
        // Trailing fractional zeros are not significant, and dropping them is
        // what makes `9.90d == 9.9d` — the oracle's normalisation, not ours.
        let frac = frac_part.trim_end_matches('0');
        let digits: String = format!("{int_part}{frac}");
        let trimmed = digits.trim_start_matches('0');
        let unscaled: u128 = if trimmed.is_empty() { 0 } else { trimmed.parse().ok()? };
        Some(FixedLit { unscaled, scale: u16::try_from(frac.len()).ok()? })
    }

    /// How many significant digits it carries. `0d` carries one.
    pub fn digits(self) -> u32 {
        let mut n = 1;
        let mut v = self.unscaled / 10;
        while v > 0 {
            n += 1;
            v /= 10;
        }
        n.max(u32::from(self.scale))
    }
}

impl fmt::Display for FixedLit {
    /// The normal form, spelled the way omniidl spells it back.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.scale == 0 {
            return write!(f, "{}d", self.unscaled);
        }
        let s = format!("{:0>width$}", self.unscaled, width = usize::from(self.scale) + 1);
        let split = s.len() - usize::from(self.scale);
        write!(f, "{}.{}d", &s[..split], &s[split..])
    }
}

/// What a token is.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// An identifier, or a keyword — the parser decides which by context,
    /// because IDL keywords are only reserved where the grammar expects them.
    Ident(String),
    /// An integer literal, as the **magnitude** the source spelled.
    ///
    /// Unsigned, and `u64` rather than `i64`, because IDL's integer literals
    /// are unsigned and a leading `-` is the unary operator `const_exp` names.
    /// An `i64` here cannot hold `18446744073709551615`, which is a perfectly
    /// ordinary `unsigned long long` constant and a perfectly ordinary
    /// `unsigned long long` union label — both of which we rejected at the
    /// lexer, in a message about 64 bits, until this was widened.
    Int(u64),
    /// A floating-point literal.
    Float(f64),
    /// A fixed-point literal — `9.9d`. Kept as a decimal; see [`FixedLit`].
    Fixed(FixedLit),
    /// A string literal, with escapes already resolved.
    Str(String),
    /// A wide string literal — `L"…"`.
    WStr(String),
    /// A character literal.
    Char(char),
    /// A wide character literal — `L'…'`.
    WChar(char),
    /// Punctuation or an operator, as written.
    Punct(&'static str),
    /// End of input.
    Eof,
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tok::Ident(s) => write!(f, "{s}"),
            Tok::Int(v) => write!(f, "{v}"),
            Tok::Float(v) => write!(f, "{v}"),
            Tok::Fixed(v) => write!(f, "{v}"),
            Tok::Str(s) => write!(f, "{s:?}"),
            Tok::WStr(s) => write!(f, "L{s:?}"),
            Tok::Char(c) => write!(f, "'{c}'"),
            Tok::WChar(c) => write!(f, "L'{c}'"),
            Tok::Punct(p) => write!(f, "{p}"),
            Tok::Eof => write!(f, "end of file"),
        }
    }
}

/// A token and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// What it is.
    pub tok: Tok,
    /// Where it is.
    pub span: Span,
    /// SIDL annotations from comments immediately preceding this token.
    ///
    /// Attached to the token rather than kept in a side list so that a
    /// declaration keeps the annotations written above it even after the
    /// parser reorders anything.
    pub annotations: Vec<Annotation>,
    /// Identity pragmas written between the previous token and this one.
    ///
    /// Attached the same way annotations are, and for the same reason: the
    /// parser needs them *in source order relative to declarations*, because
    /// `#pragma prefix` is positional. A scope's closing `}` carries the
    /// pragmas written just before it, which is how a `#pragma ID` naming an
    /// item at the very end of a module is still seen.
    pub pragmas: Vec<PragmaAt>,
    /// Whether an identifier was written with a leading `_`.
    ///
    /// The underscore is not part of the name, but it is the *only* thing that
    /// makes a keyword usable as one. Dropping it during lexing leaves the
    /// parser unable to tell `interface` — illegal as a member name — from
    /// `_interface`, which is exactly that name legally escaped.
    pub escaped: bool,
}

/// A lexical error, carrying enough position to point at the offending text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexError {
    /// What went wrong, phrased as something to fix.
    pub message: String,
    /// Where.
    pub span: Span,
}

/// The words a fixed-point literal's diagnostics open with, so that
/// [`LexError::rule`] and every site that raises one agree by construction
/// rather than by a retyped prefix.
pub(crate) const FIXED_LITERAL_SUBJECT: &str = "fixed-point literal";

impl LexError {
    /// The rule a lexical failure files under, for the consumers that key on
    /// one.
    ///
    /// Almost every lexical failure is `parse`: the cause is wherever the
    /// scanner gave up, which is not reliably where the edit belongs, and a
    /// confident wrong fix hint costs a self-repair round. The two exceptions
    /// are the fixed-point literal shapes, where the edit *is* unambiguous —
    /// the literal is right here and the specification says exactly what is
    /// wrong with it.
    ///
    /// Derived from the message rather than carried on the struct so that the
    /// twenty existing construction sites keep saying nothing about a rule
    /// they have no opinion on — but derived from a **constant the sites also
    /// build with**, not from a fragment retyped here.
    ///
    /// It was a retyped fragment until 2026-08-24, and one of the three
    /// fixed-point sites had already fallen outside it: `"malformed
    /// fixed-point literal {text:?}"` does not start with the prefix, so it
    /// filed under `parse` and lost the hint written for it — `orbweaver-forge`
    /// keys `fixed-literal` to a hint whose own comment cites
    /// `corpus/negative/n22` and whose text ("`{text}` is not a fixed-point
    /// literal…") is exactly what a malformed one needs. Nothing was red:
    /// no test asserts what `rule()` returns for any message.
    #[must_use]
    pub fn rule(&self) -> &'static str {
        if self.message.starts_with(FIXED_LITERAL_SUBJECT) { "fixed-literal" } else { "parse" }
    }
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.column, self.message)
    }
}

/// IDL keywords, which are reserved and cannot be used as identifiers unless
/// escaped with a leading underscore.
///
/// Comparison is case-insensitive: IDL treats an identifier differing from a
/// keyword only in case as a collision, for the same reason two identifiers
/// differing only in case collide with each other.
pub const KEYWORDS: &[&str] = &[
    "abstract",
    "alias",
    "any",
    "attribute",
    "bitfield",
    "bitmask",
    "bitset",
    "boolean",
    "case",
    "char",
    "component",
    "connector",
    "const",
    "consumes",
    "context",
    "custom",
    "default",
    "double",
    "emits",
    "enum",
    "eventtype",
    "exception",
    "factory",
    "false",
    "finder",
    "fixed",
    "float",
    "getraises",
    "home",
    "import",
    "in",
    "inout",
    "interface",
    "local",
    "long",
    "manages",
    "map",
    "mirrorport",
    "module",
    "multiple",
    "native",
    "object",
    "octet",
    "oneway",
    "out",
    "port",
    "porttype",
    "primarykey",
    "private",
    "provides",
    "public",
    "publishes",
    "raises",
    "readonly",
    "setraises",
    "sequence",
    "short",
    "string",
    "struct",
    "supports",
    "switch",
    "true",
    "truncatable",
    "typedef",
    "typeid",
    "typename",
    "typeprefix",
    "unsigned",
    "union",
    "uses",
    "valuebase",
    "valuetype",
    "void",
    "wchar",
    "wstring",
];

/// Whether `name` is a reserved word, ignoring case.
pub fn is_keyword(name: &str) -> bool {
    KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(name))
}

/// `s` without the leading word `w`, or `None` if it does not start with it.
///
/// The word must be followed by whitespace or end, so `prefixed` is not
/// `prefix`.
fn strip_word<'s>(s: &'s str, w: &str) -> Option<&'s str> {
    let rest = s.strip_prefix(w)?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(rest.trim_start())
    } else {
        None
    }
}

/// The first whitespace-delimited word and what follows it.
fn next_word(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    Some(match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    })
}

/// A `"…"` literal at the front of `s`, and what follows it.
///
/// No escape processing: repository ids and prefixes are `[A-Za-z0-9._/:-]`
/// in every real file, and inventing an escape rule the oracle may not share
/// would put a wrong id on the wire rather than an error on the screen.
fn quoted(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start();
    let body = s.strip_prefix('"')?;
    let end = body.find('"')?;
    Some((body[..end].to_owned(), body[end + 1..].trim_start()))
}

/// Turns the text after `#pragma` into a [`Pragma`].
///
/// The three identity spellings are matched **case-sensitively, exactly as the
/// specification writes them** — `prefix`, `version`, `ID`. Accepting `#pragma
/// id` as well would be friendly and wrong: if a deployed compiler ignores it,
/// being lenient here means we put an id on the wire that nobody else agrees
/// with, which is the failure mode this whole batch exists to close. An
/// unrecognised spelling becomes [`Pragma::Other`] and has no effect.
fn parse_pragma(rest: &str, span: Span) -> Result<Option<Pragma>, LexError> {
    let bad = |message: String| LexError { message, span };
    // Not a specification pragma: the file-boundary marker `crate::include`
    // injects. It is namespaced so that it cannot collide with anything a
    // deployed compiler defines, and it is matched here rather than falling
    // through to `Other` because the boundary is what decides a repository id.
    if let Some(arg) = strip_word(rest, "orbweaver") {
        match arg.trim() {
            "include-enter" => return Ok(Some(Pragma::IncludeEnter)),
            "include-leave" => return Ok(Some(Pragma::IncludeLeave)),
            other => {
                return Err(bad(format!(
                    "`#pragma orbweaver {other}` is not a marker this front end inserts. The \
                     `orbweaver` pragma namespace belongs to include resolution; write your \
                     own pragma under a different name."
                )));
            }
        }
    }
    if let Some(arg) = strip_word(rest, "prefix") {
        let (text, tail) = quoted(arg).ok_or_else(|| {
            bad("#pragma prefix needs a quoted string, e.g. `#pragma prefix \"acme.com\"`".into())
        })?;
        if !tail.is_empty() {
            return Err(bad(format!(
                "#pragma prefix takes one string; drop the trailing {tail:?}"
            )));
        }
        return Ok(Some(Pragma::Prefix(if text.is_empty() { None } else { Some(text) })));
    }
    if let Some(arg) = strip_word(rest, "ID") {
        let (name, tail) = next_word(arg).ok_or_else(|| {
            bad("#pragma ID needs a name and an id, e.g. `#pragma ID Account \
                 \"IDL:acme.com/bank/Account:1.0\"`"
                .into())
        })?;
        let (id, tail) = quoted(tail).ok_or_else(|| {
            bad(format!(
                "#pragma ID {name} needs a quoted id, e.g. \"IDL:acme.com/bank/{name}:1.0\""
            ))
        })?;
        if !tail.is_empty() {
            return Err(bad(format!("#pragma ID takes a name and one string; drop {tail:?}")));
        }
        return Ok(Some(Pragma::Id { name: name.to_owned(), id }));
    }
    if let Some(arg) = strip_word(rest, "version") {
        let (name, tail) = next_word(arg).ok_or_else(|| {
            bad("#pragma version needs a name and a version, e.g. `#pragma version Account 2.1`"
                .into())
        })?;
        let (v, tail) = next_word(tail).ok_or_else(|| {
            bad(format!("#pragma version {name} needs a <major>.<minor> version, e.g. 2.1"))
        })?;
        if !tail.is_empty() {
            return Err(bad(format!(
                "#pragma version takes a name and one version; drop {tail:?}"
            )));
        }
        let malformed =
            || bad(format!("#pragma version {name} {v:?}: a version is <major>.<minor>, e.g. 2.1"));
        let (major, minor) = v.split_once('.').ok_or_else(malformed)?;
        let major: u32 = major.parse().map_err(|_| malformed())?;
        let minor: u32 = minor.parse().map_err(|_| malformed())?;
        return Ok(Some(Pragma::Version { name: name.to_owned(), major, minor }));
    }
    let rest = rest.trim();
    if rest.is_empty() { Ok(None) } else { Ok(Some(Pragma::Other(rest.to_owned()))) }
}

/// Punctuation recognised by the grammar, longest first so `>>` beats `>`.
const PUNCT: &[&str] = &[
    "::", "<<", ">>", "{", "}", "[", "]", "(", ")", "<", ">", ",", ";", ":", "=", "|", "^", "&",
    "+", "-", "*", "/", "%", "~", "@",
];

/// Turns source into tokens.
pub struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    line: u32,
    col: u32,
    pending: Vec<Annotation>,
    pending_pragmas: Vec<PragmaAt>,
}

impl<'a> Lexer<'a> {
    /// A lexer over `src`.
    pub fn new(src: &'a str) -> Self {
        Self { src, pos: 0, line: 1, col: 1, pending: Vec::new(), pending_pragmas: Vec::new() }
    }

    /// Tokenizes the whole input.
    pub fn tokenize(mut self) -> Result<Vec<Token>, LexError> {
        let mut out = Vec::new();
        loop {
            let t = self.next_token()?;
            let eof = t.tok == Tok::Eof;
            out.push(t);
            if eof {
                return Ok(out);
            }
        }
    }

    fn here(&self, start: usize, sl: u32, sc: u32) -> Span {
        Span { start, end: self.pos, line: sl, column: sc }
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.src[self.pos..].chars().next()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.src[self.pos..].chars().nth(n)
    }

    fn skip_trivia(&mut self) -> Result<(), LexError> {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => {
                    self.bump();
                }
                Some('/') if self.peek_at(1) == Some('/') => self.line_comment(),
                Some('/') if self.peek_at(1) == Some('*') => self.block_comment()?,
                // Preprocessor directives. `crate::include` runs first and has
                // already resolved every `#include` and refused every
                // conditional, so what reaches here is line markers, an
                // include guard's own lines, and `#pragma` — which is lifted
                // out rather than skipped, because it decides repository ids.
                Some('#') if self.at_line_start() => self.directive()?,
                _ => return Ok(()),
            }
        }
    }

    /// Whether nothing but blanks stands between here and the line's start.
    ///
    /// Not `col == 1`: the C preprocessor allows a directive to be indented,
    /// so `    #pragma prefix "acme.com"` inside a module is a legal line that
    /// omniidl honours. Requiring column 1 made such a file a lex error, which
    /// is at least loud — but the file is valid, and rejecting valid IDL is
    /// still being wrong about it.
    fn at_line_start(&self) -> bool {
        self.src[..self.pos]
            .rsplit('\n')
            .next()
            .is_none_or(|line| line.chars().all(|c| c == ' ' || c == '\t'))
    }

    /// Consumes a `#` line, keeping it if it is an identity pragma.
    fn directive(&mut self) -> Result<(), LexError> {
        let (sl, sc, start) = (self.line, self.col, self.pos);
        self.bump(); // '#'
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.bump();
        }
        let span = self.here(start, sl, sc);
        // Everything after the '#', which may be separated from the directive
        // name by whitespace: `#  pragma prefix "x"` is one directive.
        let body = self.src[start + 1..self.pos].trim();
        let Some(rest) = strip_word(body, "pragma") else { return Ok(()) };
        if let Some(pragma) = parse_pragma(rest, span)? {
            self.pending_pragmas.push(PragmaAt { pragma, span });
        }
        Ok(())
    }

    fn line_comment(&mut self) {
        let (sl, sc, start) = (self.line, self.col, self.pos);
        self.bump();
        self.bump(); // "//"
        let text_start = self.pos;
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.bump();
        }
        let body = &self.src[text_start..self.pos];
        // `//@ key: value` is a SIDL annotation; anything else is a comment.
        if let Some(rest) = body.trim_start().strip_prefix('@')
            && let Some((k, v)) = rest.split_once(':')
        {
            self.pending.push(Annotation {
                key: k.trim().to_owned(),
                value: v.trim().to_owned(),
                span: self.here(start, sl, sc),
            });
        }
    }

    fn block_comment(&mut self) -> Result<(), LexError> {
        let (sl, sc, start) = (self.line, self.col, self.pos);
        self.bump();
        self.bump(); // "/*"
        loop {
            match self.peek() {
                None => {
                    return Err(LexError {
                        message: "unterminated /* comment: add a closing */".into(),
                        span: self.here(start, sl, sc),
                    });
                }
                Some('*') if self.peek_at(1) == Some('/') => {
                    self.bump();
                    self.bump();
                    return Ok(());
                }
                _ => {
                    self.bump();
                }
            }
        }
    }

    fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_trivia()?;
        let annotations = std::mem::take(&mut self.pending);
        let pragmas = std::mem::take(&mut self.pending_pragmas);
        let (sl, sc, start) = (self.line, self.col, self.pos);

        let Some(c) = self.peek() else {
            return Ok(Token {
                tok: Tok::Eof,
                span: self.here(start, sl, sc),
                annotations,
                pragmas,
                escaped: false,
            });
        };

        let mut escaped_ident = false;
        // `L'a'` and `L"hi"` — the wide literals. Matched ahead of the
        // identifier rule, which would otherwise read the `L` as a name and
        // leave the quote to be lexed on its own: that is exactly what
        // happened, and the diagnostic was `expected ";", found 'a'`, which
        // names neither `L` nor `wchar`. Without these two forms a `wchar` or
        // `wstring` constant cannot be written at all — while `const wchar C =
        // 'a';` was *accepted*, so the only spelling we took was the one
        // omniidl refuses.
        let tok = if c == 'L' && matches!(self.peek_at(1), Some('\'') | Some('"')) {
            self.bump();
            if self.peek() == Some('"') {
                match self.string(start, sl, sc)? {
                    Tok::Str(s) => Tok::WStr(s),
                    other => other,
                }
            } else {
                match self.character(start, sl, sc)? {
                    Tok::Char(c) => Tok::WChar(c),
                    other => other,
                }
            }
        } else if c == '_' || c.is_ascii_alphabetic() {
            // IDL allows a leading underscore to escape a keyword; the escaped
            // name is the identifier without it.
            escaped_ident = c == '_';
            if escaped_ident {
                self.bump();
            }
            let from = self.pos;
            while let Some(c) = self.peek() {
                if c == '_' || c.is_ascii_alphanumeric() {
                    self.bump();
                } else {
                    break;
                }
            }
            if self.pos == from {
                return Err(LexError {
                    message: "'_' must be followed by an identifier".into(),
                    span: self.here(start, sl, sc),
                });
            }
            Tok::Ident(self.src[from..self.pos].to_owned())
        } else if c.is_ascii_digit()
            || (c == '.' && self.peek_at(1).is_some_and(|d| d.is_ascii_digit()))
        {
            self.number(start, sl, sc)?
        } else if c == '"' {
            self.string(start, sl, sc)?
        } else if c == '\'' {
            self.character(start, sl, sc)?
        } else if c == '.' {
            self.bump();
            Tok::Punct(".")
        } else if let Some(p) = PUNCT.iter().find(|p| self.src[self.pos..].starts_with(**p)) {
            for _ in 0..p.chars().count() {
                self.bump();
            }
            Tok::Punct(p)
        } else {
            self.bump();
            return Err(LexError {
                message: format!("unexpected character {c:?}"),
                span: self.here(start, sl, sc),
            });
        };

        Ok(Token {
            tok,
            span: self.here(start, sl, sc),
            annotations,
            pragmas,
            escaped: escaped_ident,
        })
    }

    /// Lexes a numeric literal **without choosing a Rust type that cannot hold
    /// it**, which is the rule this function got wrong three ways at once.
    ///
    /// Measured against omniidl 4.3.4 on 2026-08-21, the three were one cause:
    /// every literal was funnelled into `i64` or `f64` and whatever those could
    /// not represent was lost or refused.
    ///
    /// | written | was | is |
    /// |---|---|---|
    /// | `9.9d` | `Float(9.900000000000000355…)` | `Fixed(99, scale 1)` |
    /// | `18446744073709551615` | refused, "does not fit in 64 bits" | `Int` |
    /// | `0xFFFFFFFFFFFFFFFF` | refused, "malformed hexadecimal" | `Int` |
    ///
    /// The two refusals are legal `unsigned long long` — omniidl accepts both,
    /// as a constant and as a union case label. See [`Tok::Int`] and
    /// [`FixedLit`] for why the types are what they are.
    fn number(&mut self, start: usize, sl: u32, sc: u32) -> Result<Tok, LexError> {
        let from = self.pos;
        let mut is_float = false;
        let mut has_exponent = false;
        let mut is_fixed = false;

        if self.peek() == Some('0') && matches!(self.peek_at(1), Some('x') | Some('X')) {
            self.bump();
            self.bump();
            let hex_from = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                self.bump();
            }
            let text = &self.src[hex_from..self.pos];
            return u64::from_str_radix(text, 16).map(Tok::Int).map_err(|_| LexError {
                message: format!("malformed hexadecimal literal {text:?}"),
                span: self.here(start, sl, sc),
            });
        }

        while let Some(c) = self.peek() {
            match c {
                '0'..='9' => {
                    self.bump();
                }
                '.' if !is_float => {
                    is_float = true;
                    self.bump();
                }
                'e' | 'E' => {
                    is_float = true;
                    has_exponent = true;
                    self.bump();
                    if matches!(self.peek(), Some('+') | Some('-')) {
                        self.bump();
                    }
                }
                'd' | 'D' => {
                    is_fixed = true;
                    self.bump();
                    break;
                }
                _ => break,
            }
        }
        let text = &self.src[from..self.pos];
        if is_fixed {
            let cleaned = text.trim_end_matches(['d', 'D']);
            if has_exponent {
                // omniidl: "Cannot interpret floating point literal as fixed
                // point". `fixed_pt_literal` has no exponent production, so
                // `1e3d` is a float literal wearing a `d`.
                return Err(LexError {
                    message: format!(
                        "{FIXED_LITERAL_SUBJECT} {text:?} may not carry an exponent: IDL's \
                         `fixed_pt_literal` is digits, a point and more digits. Write the \
                         digits out, or drop the `d` and let it be a `double`."
                    ),
                    span: self.here(start, sl, sc),
                });
            }
            let lit = FixedLit::parse(cleaned).ok_or_else(|| LexError {
                message: format!("{FIXED_LITERAL_SUBJECT} {text:?} is malformed"),
                span: self.here(start, sl, sc),
            })?;
            if lit.digits() > FIXED_MAX_DIGITS {
                return Err(LexError {
                    message: format!(
                        "{FIXED_LITERAL_SUBJECT} {text:?} has {} significant digits: a `fixed` \
                         carries at most {FIXED_MAX_DIGITS} (CORBA 3.4 §7.11.3). Drop digits \
                         until it fits — rounding it here would change the value silently.",
                        lit.digits()
                    ),
                    span: self.here(start, sl, sc),
                });
            }
            return Ok(Tok::Fixed(lit));
        }
        if is_float {
            return text.parse::<f64>().map(Tok::Float).map_err(|_| LexError {
                message: format!("malformed floating-point literal {text:?}"),
                span: self.here(start, sl, sc),
            });
        }
        if text.len() > 1 && text.starts_with('0') {
            return u64::from_str_radix(&text[1..], 8).map(Tok::Int).map_err(|_| LexError {
                message: format!("malformed octal literal {text:?}"),
                span: self.here(start, sl, sc),
            });
        }
        text.parse::<u64>().map(Tok::Int).map_err(|_| LexError {
            message: format!("integer literal {text:?} does not fit in 64 bits"),
            span: self.here(start, sl, sc),
        })
    }

    fn escape(&mut self, start: usize, sl: u32, sc: u32) -> Result<char, LexError> {
        let c = self.bump().ok_or_else(|| LexError {
            message: "unterminated escape sequence".into(),
            span: self.here(start, sl, sc),
        })?;
        Ok(match c {
            'n' => '\n',
            't' => '\t',
            'v' => '\u{0b}',
            'b' => '\u{08}',
            'r' => '\r',
            'f' => '\u{0c}',
            'a' => '\u{07}',
            '\\' => '\\',
            '?' => '?',
            '\'' => '\'',
            '"' => '"',
            'x' => {
                let from = self.pos;
                for _ in 0..2 {
                    if self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                        self.bump();
                    }
                }
                let v =
                    u32::from_str_radix(&self.src[from..self.pos], 16).map_err(|_| LexError {
                        message: "malformed \\x escape: expected two hex digits".into(),
                        span: self.here(start, sl, sc),
                    })?;
                char::from_u32(v).unwrap_or('\u{fffd}')
            }
            '0'..='7' => {
                let from = self.pos - 1;
                for _ in 0..2 {
                    if self.peek().is_some_and(|c| ('0'..='7').contains(&c)) {
                        self.bump();
                    }
                }
                let v =
                    u32::from_str_radix(&self.src[from..self.pos], 8).map_err(|_| LexError {
                        message: "malformed octal escape".into(),
                        span: self.here(start, sl, sc),
                    })?;
                char::from_u32(v).unwrap_or('\u{fffd}')
            }
            other => other,
        })
    }

    fn string(&mut self, start: usize, sl: u32, sc: u32) -> Result<Tok, LexError> {
        self.bump(); // opening quote
        let mut out = String::new();
        loop {
            match self.bump() {
                None | Some('\n') => {
                    return Err(LexError {
                        message: "unterminated string literal".into(),
                        span: self.here(start, sl, sc),
                    });
                }
                Some('"') => return Ok(Tok::Str(out)),
                Some('\\') => out.push(self.escape(start, sl, sc)?),
                Some(c) => out.push(c),
            }
        }
    }

    fn character(&mut self, start: usize, sl: u32, sc: u32) -> Result<Tok, LexError> {
        self.bump(); // opening quote
        let c = match self.bump() {
            Some('\\') => self.escape(start, sl, sc)?,
            Some('\'') => {
                return Err(LexError {
                    message: "empty character literal".into(),
                    span: self.here(start, sl, sc),
                });
            }
            Some(c) => c,
            None => {
                return Err(LexError {
                    message: "unterminated character literal".into(),
                    span: self.here(start, sl, sc),
                });
            }
        };
        if self.bump() != Some('\'') {
            return Err(LexError {
                message: "character literal must hold exactly one character".into(),
                span: self.here(start, sl, sc),
            });
        }
        Ok(Tok::Char(c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        Lexer::new(src).tokenize().unwrap().into_iter().map(|t| t.tok).collect()
    }

    /// Every way a fixed-point literal can be refused files under
    /// `fixed-literal`, so the hint written for it actually reaches it.
    ///
    /// **Three shapes, not two.** `rule()` classifies by the subject the sites
    /// build with, and until 2026-08-24 it classified by a retyped prefix that
    /// one of the three did not carry: `"malformed fixed-point literal …"`
    /// filed under `parse`, so a literal too long for the parse to hold — 40
    /// digits and up, where `u128` gives out before the 31-digit rule is even
    /// reached — got the generic diagnostic and none of the hint
    /// `orbweaver-forge` keys to `fixed-literal`, whose text is exactly what
    /// that literal needs. Nothing was red, because nothing asserted what
    /// `rule()` returns for any message at all.
    ///
    /// *세 가지 모양이며 둘이 아니다. 셋 중 하나가 손으로 옮겨 적은 접두사 밖에
    /// 있었고, 그래서 자기 앞으로 쓰인 힌트를 받지 못했다.*
    #[test]
    fn every_fixed_literal_refusal_files_under_the_fixed_literal_rule() {
        let long = "1".repeat(40);
        for src in [
            "const fixed A = 1e3d;",                              // an exponent
            &format!("const fixed A = {long}d;"),                 // too long to parse at all
            "const fixed A = 12345678901234567890123456789012d;", // 32 significant digits
        ] {
            let err = Lexer::new(src).tokenize().expect_err(&format!("{src} must be refused"));
            assert_eq!(err.rule(), "fixed-literal", "{src}: {}", err.message);
        }

        // And the classification is not "anything that mentions a literal":
        // an ordinary lexical failure still files under `parse`, or the hint
        // would fire on inputs it was not written for.
        let err = Lexer::new("module m { \"unterminated").tokenize().expect_err("must be refused");
        assert_eq!(err.rule(), "parse", "{}", err.message);
    }

    #[test]
    fn lexes_a_declaration() {
        assert_eq!(
            toks("module m { long x; };"),
            vec![
                Tok::Ident("module".into()),
                Tok::Ident("m".into()),
                Tok::Punct("{"),
                Tok::Ident("long".into()),
                Tok::Ident("x".into()),
                Tok::Punct(";"),
                Tok::Punct("}"),
                Tok::Punct(";"),
                Tok::Eof,
            ]
        );
    }

    /// `>>` must not be split, or `sequence<sequence<long>>` closes one bracket.
    #[test]
    fn shift_and_scope_operators_are_single_tokens() {
        assert_eq!(
            toks("a::b >> c"),
            vec![
                Tok::Ident("a".into()),
                Tok::Punct("::"),
                Tok::Ident("b".into()),
                Tok::Punct(">>"),
                Tok::Ident("c".into()),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn numbers_cover_the_idl_forms() {
        assert_eq!(toks("42"), vec![Tok::Int(42), Tok::Eof]);
        assert_eq!(toks("0x1F"), vec![Tok::Int(31), Tok::Eof]);
        assert_eq!(toks("0755"), vec![Tok::Int(493), Tok::Eof], "leading zero is octal");
        assert_eq!(toks("0"), vec![Tok::Int(0), Tok::Eof]);
        assert_eq!(toks("1.5"), vec![Tok::Float(1.5), Tok::Eof]);
        assert_eq!(toks("1e3"), vec![Tok::Float(1000.0), Tok::Eof]);
        assert_eq!(toks(".5"), vec![Tok::Float(0.5), Tok::Eof]);
    }

    /// The magnitudes an `i64` could not hold. Both were refused outright, in
    /// two different messages, and both are ordinary `unsigned long long`.
    #[test]
    fn an_integer_literal_reaches_the_top_of_unsigned_long_long() {
        assert_eq!(toks("18446744073709551615"), vec![Tok::Int(u64::MAX), Tok::Eof]);
        assert_eq!(toks("0xFFFFFFFFFFFFFFFF"), vec![Tok::Int(u64::MAX), Tok::Eof]);
        // One past it is still refused, and by the same rule rather than by a
        // Rust type's edge — the message names the width IDL names.
        assert!(Lexer::new("18446744073709551616").tokenize().is_err());
    }

    /// A fixed literal keeps its decimal, in the normal form the oracle uses.
    #[test]
    fn a_fixed_literal_is_a_decimal_and_not_a_float() {
        let fixed = |src: &str| match toks(src).first() {
            Some(Tok::Fixed(f)) => *f,
            other => panic!("{src} should lex as a fixed literal, got {other:?}"),
        };
        // 9.9 has no `f64`. Kept as 99 with scale 1, so it is 9.9 exactly.
        assert_eq!(fixed("9.9d"), FixedLit { unscaled: 99, scale: 1 });
        assert_eq!(fixed("1.005d"), FixedLit { unscaled: 1005, scale: 3 });
        assert_eq!(fixed("9.9D"), fixed("9.9d"), "the suffix may be either case");

        // Normalisation, taken from `omniidl -b dump` reading its own output
        // back (2026-08-21): trailing fractional zeros and leading integer
        // zeros are not part of the value.
        assert_eq!(fixed("9.90d"), fixed("9.9d"));
        assert_eq!(fixed("0.10d"), fixed("0.1d"));
        assert_eq!(fixed("000000001d"), fixed("1d"));
        assert_eq!(fixed("0.0d"), FixedLit { unscaled: 0, scale: 0 });
        assert_eq!(fixed("100.d"), FixedLit { unscaled: 100, scale: 0 });
        assert_eq!(fixed(".5d"), FixedLit { unscaled: 5, scale: 1 });

        // Spelled back the way omniidl spells it.
        assert_eq!(fixed("9.90d").to_string(), "9.9d");
        assert_eq!(fixed("0.001d").to_string(), "0.001d");
        assert_eq!(fixed("0.0d").to_string(), "0d");

        // The digit cap is the specification's, and 31 is exercised rather
        // than described. omniidl silently truncates a 32nd *fractional*
        // digit; we refuse, and corpus/divergences.tsv says why.
        assert_eq!(fixed("1234567890123456789012345678901d").digits(), 31);
        assert!(Lexer::new("12345678901234567890123456789012d").tokenize().is_err());
        assert!(Lexer::new("0.12345678901234567890123456789012d").tokenize().is_err());

        // `fixed_pt_literal` has no exponent production, so this is a float
        // wearing a suffix. It used to lex as `Float(1000.0)`, because the
        // suffix was stripped before anything looked at it.
        assert!(Lexer::new("1e3d").tokenize().is_err());
    }

    /// `L'a'` and `L"hi"`, which had no spelling at all.
    ///
    /// The `L` used to be read as an identifier, leaving the quote to be lexed
    /// on its own — so the error was `expected ";", found 'a'`, naming neither
    /// `L` nor `wchar`. Meanwhile the unprefixed `const wchar C = 'a';` was
    /// accepted, and omniidl refuses that: the only spelling of a wide
    /// constant this repository could produce was the wrong one.
    #[test]
    fn wide_literals_take_an_l_prefix() {
        assert_eq!(toks("L'a'"), vec![Tok::WChar('a'), Tok::Eof]);
        assert_eq!(toks(r#"L"hi""#), vec![Tok::WStr("hi".into()), Tok::Eof]);
        assert_eq!(toks(r"L'\n'"), vec![Tok::WChar('\n'), Tok::Eof], "escapes still resolve");

        // A code point above U+00FF, kept whole. omniidl reads a wide literal
        // one *byte* at a time and cannot lex this at all (measured
        // 2026-08-21, corpus/divergences.tsv), which is why no golden file
        // carries one and this pin does.
        assert_eq!(toks("L'\u{ac00}'"), vec![Tok::WChar('\u{ac00}'), Tok::Eof]);
        assert_eq!(toks("L\"\u{c6d0}\u{c7a5}\""), vec![Tok::WStr("원장".into()), Tok::Eof]);

        // The prefix is only a prefix when a quote follows it: `L` and `Ledger`
        // are ordinary identifiers, and reading them as literals would be a
        // spectacular way to break every contract with an `L`-initial name.
        assert_eq!(toks("L"), vec![Tok::Ident("L".into()), Tok::Eof]);
        assert_eq!(toks("Ledger"), vec![Tok::Ident("Ledger".into()), Tok::Eof]);
    }

    #[test]
    fn strings_and_chars_resolve_escapes() {
        assert_eq!(toks(r#""a\nb""#), vec![Tok::Str("a\nb".into()), Tok::Eof]);
        assert_eq!(toks(r#""q\"q""#), vec![Tok::Str("q\"q".into()), Tok::Eof]);
        assert_eq!(toks(r"'\t'"), vec![Tok::Char('\t'), Tok::Eof]);
        assert_eq!(toks(r"'\x41'"), vec![Tok::Char('A'), Tok::Eof]);
        assert_eq!(toks(r"'\101'"), vec![Tok::Char('A'), Tok::Eof], "octal escape");
    }

    /// A leading underscore escapes a keyword and is not part of the name.
    /// The underscore is not part of the name but must survive as a flag, or
    /// the parser cannot tell an illegal keyword from a legally escaped one.
    #[test]
    fn underscore_escapes_a_keyword() {
        let out = Lexer::new("_interface interface").tokenize().unwrap();
        assert_eq!(out[0].tok, Tok::Ident("interface".into()));
        assert!(out[0].escaped);
        assert_eq!(out[1].tok, Tok::Ident("interface".into()));
        assert!(!out[1].escaped);
    }

    #[test]
    fn keywords_are_recognised_case_insensitively() {
        assert!(is_keyword("interface"));
        assert!(is_keyword("Interface"), "case must not smuggle a keyword through");
        assert!(is_keyword("TRUE"));
        assert!(!is_keyword("interfaces"));
    }

    #[test]
    fn ordinary_comments_are_dropped() {
        assert_eq!(
            toks("a // trailing\n/* block */ b"),
            vec![Tok::Ident("a".into()), Tok::Ident("b".into()), Tok::Eof]
        );
    }

    /// SIDL lives in comments because deployed compilers reject IDL 4
    /// `@annotation` (Phase 0 assumption C). Dropping comments would drop the
    /// meaning layer.
    #[test]
    fn sidl_annotations_attach_to_the_next_token() {
        let out =
            Lexer::new("//@ ai_desc: transfers funds\n//@ ai_effect: destructive\nvoid execute();")
                .tokenize()
                .unwrap();
        let first = &out[0];
        assert_eq!(first.tok, Tok::Ident("void".into()));
        assert_eq!(first.annotations.len(), 2);
        assert_eq!(first.annotations[0].key, "ai_desc");
        assert_eq!(first.annotations[0].value, "transfers funds");
        assert_eq!(first.annotations[1].value, "destructive");
        // A plain comment must not become an annotation.
        assert!(out[1].annotations.is_empty());
    }

    #[test]
    fn a_value_may_contain_colons() {
        let out = Lexer::new("//@ ai_example: {\"a\": 1}\nlong x;").tokenize().unwrap();
        assert_eq!(out[0].annotations[0].value, "{\"a\": 1}");
    }

    #[test]
    fn preprocessor_lines_do_not_stop_the_lexer() {
        assert_eq!(
            toks("#include <orb.idl>\n#pragma prefix \"x\"\nlong x;"),
            vec![Tok::Ident("long".into()), Tok::Ident("x".into()), Tok::Punct(";"), Tok::Eof]
        );
    }

    /// The identity pragmas ride on the following token, the way annotations
    /// do, because `#pragma prefix` is positional and the parser needs them in
    /// source order without the grammar admitting `#`.
    #[test]
    fn identity_pragmas_attach_to_the_next_token() {
        let out = Lexer::new(
            "#pragma prefix \"acme.com\"\n\
             #pragma version Account 2.3\n\
             #pragma ID Account \"IDL:x/Y:1.0\"\n\
             module m;",
        )
        .tokenize()
        .unwrap();
        assert_eq!(out[0].tok, Tok::Ident("module".into()));
        assert_eq!(
            out[0].pragmas.iter().map(|p| p.pragma.clone()).collect::<Vec<_>>(),
            vec![
                Pragma::Prefix(Some("acme.com".into())),
                Pragma::Version { name: "Account".into(), major: 2, minor: 3 },
                Pragma::Id { name: "Account".into(), id: "IDL:x/Y:1.0".into() },
            ]
        );
    }

    /// `""` is a reset, not an empty leading segment: the difference between
    /// `IDL:m/I:1.0` and `IDL:/m/I:1.0`.
    #[test]
    fn an_empty_prefix_is_a_reset() {
        let out = Lexer::new("#pragma prefix \"\"\nlong x;").tokenize().unwrap();
        assert_eq!(out[0].pragmas[0].pragma, Pragma::Prefix(None));
    }

    /// Whitespace between `#` and the directive, and before the `#`, are both
    /// legal C preprocessor. A pragma indented inside a module is exactly how
    /// a real file writes one, and it must not lose its identity or be
    /// rejected outright.
    #[test]
    fn a_spaced_or_indented_hash_is_still_a_pragma() {
        for src in [
            "#  pragma prefix \"acme.com\"\nlong x;",
            "module m {\n    #pragma prefix \"acme.com\"\nlong x;",
            "\t#pragma prefix \"acme.com\"\nlong x;",
        ] {
            let out = Lexer::new(src).tokenize().unwrap();
            let found = out.iter().flat_map(|t| &t.pragmas).map(|p| &p.pragma).collect::<Vec<_>>();
            assert_eq!(found, vec![&Pragma::Prefix(Some("acme.com".into()))], "{src:?}");
        }
    }

    /// A `#` that is not at the start of a line is not a directive, and the
    /// grammar has no use for it either.
    #[test]
    fn a_hash_mid_line_is_still_an_error() {
        assert!(Lexer::new("long x; #pragma prefix \"a\"").tokenize().is_err());
    }

    /// Anything else keeps its text and has no effect — including `#pragma id`
    /// in the wrong case, which we refuse to guess at (see `parse_pragma`).
    #[test]
    fn unrecognised_pragmas_are_kept_and_inert() {
        let out = Lexer::new("#pragma sendtop\n#pragma id Account \"IDL:x:1.0\"\nlong x;")
            .tokenize()
            .unwrap();
        assert_eq!(out[0].pragmas[0].pragma, Pragma::Other("sendtop".into()));
        assert!(matches!(out[0].pragmas[1].pragma, Pragma::Other(_)));
    }

    /// Line markers the preprocessor emits are not pragmas and never were.
    #[test]
    fn preprocessor_line_markers_are_still_skipped() {
        let out = Lexer::new("# 12 \"bank.idl\"\nlong x;").tokenize().unwrap();
        assert!(out[0].pragmas.is_empty());
    }

    /// A malformed identity pragma is an error, not a shrug: silently ignoring
    /// it puts an id on the wire that the author did not write.
    #[test]
    fn a_malformed_identity_pragma_is_reported() {
        for src in [
            "#pragma prefix acme.com\nlong x;",
            "#pragma prefix\nlong x;",
            "#pragma ID Account\nlong x;",
            "#pragma version Account\nlong x;",
            "#pragma version Account two.three\nlong x;",
        ] {
            let e = Lexer::new(src).tokenize().unwrap_err();
            assert!(e.message.contains("#pragma"), "{src:?} -> {}", e.message);
        }
    }

    #[test]
    fn spans_point_at_the_offending_text() {
        let e = Lexer::new("long x;\nlong `y;").tokenize().unwrap_err();
        assert_eq!(e.span.line, 2);
        assert!(e.message.contains('`'), "the message should name the character");
    }

    #[test]
    fn unterminated_constructs_are_named() {
        for (src, want) in [
            ("/* nope", "unterminated /*"),
            ("\"nope", "unterminated string"),
            ("'ab'", "exactly one character"),
        ] {
            let e = Lexer::new(src).tokenize().unwrap_err();
            assert!(e.message.contains(want), "{src:?} gave {:?}", e.message);
        }
    }
}
