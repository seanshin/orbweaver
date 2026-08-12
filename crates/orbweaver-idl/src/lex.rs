//! Tokenizer for OMG IDL 4.2, with SIDL structured comments preserved.
//!
//! Comments are normally noise a lexer drops. Here they are not: Phase 0
//! established that deployed IDL compilers reject IDL 4 `@annotation`, so SIDL
//! carries its semantics in `//@ key: value` comments instead
//! (`docs/PHASE0.md`, assumption C). Discarding comments would discard the
//! meaning layer the whole project is built on.

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

/// What a token is.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// An identifier, or a keyword — the parser decides which by context,
    /// because IDL keywords are only reserved where the grammar expects them.
    Ident(String),
    /// An integer literal.
    Int(i64),
    /// A floating-point literal.
    Float(f64),
    /// A string literal, with escapes already resolved.
    Str(String),
    /// A character literal.
    Char(char),
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
            Tok::Str(s) => write!(f, "{s:?}"),
            Tok::Char(c) => write!(f, "'{c}'"),
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
}

impl<'a> Lexer<'a> {
    /// A lexer over `src`.
    pub fn new(src: &'a str) -> Self {
        Self { src, pos: 0, line: 1, col: 1, pending: Vec::new() }
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
                // Preprocessor directives. Full preprocessing is a separate
                // problem; a line beginning with # is skipped so that files
                // carrying #include or #pragma still tokenize rather than
                // failing on the first line.
                Some('#') if self.col == 1 => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                _ => return Ok(()),
            }
        }
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
        let (sl, sc, start) = (self.line, self.col, self.pos);

        let Some(c) = self.peek() else {
            return Ok(Token {
                tok: Tok::Eof,
                span: self.here(start, sl, sc),
                annotations,
                escaped: false,
            });
        };

        let mut escaped_ident = false;
        let tok = if c == '_' || c.is_ascii_alphabetic() {
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

        Ok(Token { tok, span: self.here(start, sl, sc), annotations, escaped: escaped_ident })
    }

    fn number(&mut self, start: usize, sl: u32, sc: u32) -> Result<Tok, LexError> {
        let from = self.pos;
        let mut is_float = false;

        if self.peek() == Some('0') && matches!(self.peek_at(1), Some('x') | Some('X')) {
            self.bump();
            self.bump();
            let hex_from = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                self.bump();
            }
            let text = &self.src[hex_from..self.pos];
            return i64::from_str_radix(text, 16).map(Tok::Int).map_err(|_| LexError {
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
                    self.bump();
                    if matches!(self.peek(), Some('+') | Some('-')) {
                        self.bump();
                    }
                }
                'd' | 'D' => {
                    // A fixed-point literal suffix. Treated as a float for now;
                    // `fixed` is parser-only in v1 (PLAN §4.4).
                    is_float = true;
                    self.bump();
                    break;
                }
                _ => break,
            }
        }
        let text = &self.src[from..self.pos];
        if is_float {
            let cleaned = text.trim_end_matches(['d', 'D']);
            cleaned.parse::<f64>().map(Tok::Float).map_err(|_| LexError {
                message: format!("malformed floating-point literal {text:?}"),
                span: self.here(start, sl, sc),
            })
        } else if text.len() > 1 && text.starts_with('0') {
            i64::from_str_radix(&text[1..], 8).map(Tok::Int).map_err(|_| LexError {
                message: format!("malformed octal literal {text:?}"),
                span: self.here(start, sl, sc),
            })
        } else {
            text.parse::<i64>().map(Tok::Int).map_err(|_| LexError {
                message: format!("integer literal {text:?} does not fit in 64 bits"),
                span: self.here(start, sl, sc),
            })
        }
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
