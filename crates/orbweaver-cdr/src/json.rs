//! A JSON reader and writer, written against RFC 8259.
//!
//! # Why this is first-party
//!
//! The same reasoning that makes the ORB core first-party: JSON is a published
//! specification we can implement ourselves and owe nobody for. `CLAUDE.md`
//! draws the line at *data we cannot originate* — a character mapping table has
//! no specification to implement from, so retyping it produces a derived work.
//! A grammar is not that.
//!
//! There is a second reason, and it is the stronger one. This parser sits at
//! the agent boundary, where the input is untrusted by definition (§9.0). Owning
//! it means the limits are ours to set: nesting depth is capped, so a few
//! kilobytes of `[[[[…` cannot exhaust the stack, and every length is checked
//! against the bytes actually present. A dependency would make those limits
//! somebody else's decision.
//!
//! # Scope
//!
//! Exactly RFC 8259 and nothing else: no comments, no trailing commas, no
//! single quotes, no `NaN` literal. Accepting extensions here would mean
//! accepting input the agent's own JSON writer cannot produce, which can only
//! hide bugs.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// The maximum nesting depth a document may reach.
///
/// A recursive-descent parser turns nesting into stack frames, so this is a
/// memory-safety limit rather than a style preference. 64 is far past anything
/// the AnyJSON mapping produces — a deeply nested IDL type bottoms out long
/// before it — and far short of what would hurt.
pub const MAX_DEPTH: usize = 64;

/// The words the depth refusal opens with.
///
/// Published for the same reason [`crate::IMPLAUSIBLE_LENGTH`] is: the
/// agent-reach sweep in `orbweaver-test` counts how many fed documents were
/// refused for depth rather than for shape, and until 2026-08-24 it counted by
/// matching a retyped copy of this sentence. A rewording here would have zeroed
/// that counter with nothing to notice it — the tests that pin this string pin
/// the parser's own message, not the count.
pub const NESTING_TOO_DEEP: &str = "nesting deeper than";

/// A JSON value.
///
/// Numbers keep their source text rather than becoming `f64`. AnyJSON maps
/// `long long` to a JSON *string* precisely because a JSON number cannot hold
/// it, and a parser that eagerly converted every number to a float would
/// destroy the value before the mapping ever saw it — the corruption the
/// mapping exists to prevent, moved one layer down.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number, as written.
    Number(String),
    /// A string, unescaped.
    String(String),
    /// An array.
    Array(Vec<Json>),
    /// An object. Sorted by key, because AnyJSON never depends on member order
    /// — a struct's order comes from its `TypeCode`, not from the document.
    Object(BTreeMap<String, Json>),
}

/// Why a document could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset where it went wrong.
    pub at: usize,
    /// What was wrong, phrased as something to fix.
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "at byte {}: {}", self.at, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Renders the value as compact JSON.
///
/// `Display` rather than an inherent `to_string`, so `{}` in a message and
/// `.to_string()` in code cannot diverge.
impl std::fmt::Display for Json {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = String::new();
        self.write(&mut out);
        f.write_str(&out)
    }
}

impl Json {
    /// Parses one JSON document.
    ///
    /// Trailing content after the value is an error: two documents in one
    /// string is a caller bug, and silently ignoring the second is how it goes
    /// unnoticed.
    pub fn parse(src: &str) -> Result<Json, ParseError> {
        let mut p = Parser { src: src.as_bytes(), at: 0, depth: 0 };
        p.skip_ws();
        let v = p.value()?;
        p.skip_ws();
        if p.at != p.src.len() {
            return Err(p.err("trailing content after the document"));
        }
        Ok(v)
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Number(n) => out.push_str(n),
            Json::String(s) => write_string(s, out),
            Json::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Object(map) => {
                out.push('{');
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }

    /// The string, if this is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    /// The member of an object, if this is an object with that key.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(m) => m.get(key),
            _ => None,
        }
    }

    /// A short name for the kind, for diagnostics.
    pub fn kind(&self) -> &'static str {
        match self {
            Json::Null => "null",
            Json::Bool(_) => "a boolean",
            Json::Number(_) => "a number",
            Json::String(_) => "a string",
            Json::Array(_) => "an array",
            Json::Object(_) => "an object",
        }
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            // RFC 8259 §7 requires escaping everything below 0x20 and nothing
            // else; U+2028 and friends are legal raw JSON even though some
            // JavaScript parsers historically choked on them.
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    src: &'a [u8],
    at: usize,
    depth: usize,
}

impl<'a> Parser<'a> {
    fn err(&self, message: impl Into<String>) -> ParseError {
        ParseError { at: self.at, message: message.into() }
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.at).copied()
    }

    fn skip_ws(&mut self) {
        // RFC 8259 §2: space, tab, newline, carriage return. Nothing else is
        // whitespace, including the vertical tab and the byte-order mark.
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, b: u8) -> Result<(), ParseError> {
        if self.peek() == Some(b) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.err(format!("expected {:?}", b as char)))
        }
    }

    fn literal(&mut self, word: &str, v: Json) -> Result<Json, ParseError> {
        if self.src[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(v)
        } else {
            Err(self.err(format!("expected {word}")))
        }
    }

    fn value(&mut self) -> Result<Json, ParseError> {
        match self.peek() {
            None => Err(self.err("the document ended where a value was expected")),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(c) => Err(self.err(format!("{:?} cannot start a value", c as char))),
        }
    }

    fn nest<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, ParseError>,
    ) -> Result<T, ParseError> {
        if self.depth >= MAX_DEPTH {
            return Err(self.err(format!("{NESTING_TOO_DEEP} {MAX_DEPTH} levels")));
        }
        self.depth += 1;
        let r = f(self);
        self.depth -= 1;
        r
    }

    fn array(&mut self) -> Result<Json, ParseError> {
        self.nest(|p| {
            p.expect(b'[')?;
            let mut items = Vec::new();
            p.skip_ws();
            if p.peek() == Some(b']') {
                p.at += 1;
                return Ok(Json::Array(items));
            }
            loop {
                p.skip_ws();
                items.push(p.value()?);
                p.skip_ws();
                match p.peek() {
                    Some(b',') => p.at += 1,
                    Some(b']') => {
                        p.at += 1;
                        return Ok(Json::Array(items));
                    }
                    _ => return Err(p.err("expected ',' or ']'")),
                }
            }
        })
    }

    fn object(&mut self) -> Result<Json, ParseError> {
        self.nest(|p| {
            p.expect(b'{')?;
            let mut map = BTreeMap::new();
            p.skip_ws();
            if p.peek() == Some(b'}') {
                p.at += 1;
                return Ok(Json::Object(map));
            }
            loop {
                p.skip_ws();
                let at = p.at;
                let key = p.string()?;
                // RFC 8259 permits duplicate names and says the behaviour is
                // unspecified. Unspecified is not something to guess at when
                // the value decides what goes on a wire.
                if map.contains_key(&key) {
                    return Err(ParseError {
                        at,
                        message: format!("duplicate key {key:?}; which one wins is unspecified"),
                    });
                }
                p.skip_ws();
                p.expect(b':')?;
                p.skip_ws();
                let value = p.value()?;
                map.insert(key, value);
                p.skip_ws();
                match p.peek() {
                    Some(b',') => p.at += 1,
                    Some(b'}') => {
                        p.at += 1;
                        return Ok(Json::Object(map));
                    }
                    _ => return Err(p.err("expected ',' or '}'")),
                }
            }
        })
    }

    fn number(&mut self) -> Result<Json, ParseError> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        // A leading zero may not be followed by more digits: 0 and 0.5 are
        // numbers, 01 is not.
        match self.peek() {
            Some(b'0') => self.at += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.at += 1;
                }
            }
            _ => return Err(self.err("a number needs at least one digit")),
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("a decimal point needs a digit after it"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.err("an exponent needs a digit"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
        }
        // Every byte consumed above is ASCII, so the slice is valid UTF-8.
        Ok(Json::Number(String::from_utf8_lossy(&self.src[start..self.at]).into_owned()))
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(self.err("the document ended inside a string"));
            };
            match b {
                b'"' => {
                    self.at += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.at += 1;
                    self.escape(&mut out)?;
                }
                // RFC 8259 §7: unescaped control characters are not allowed.
                0x00..=0x1F => {
                    return Err(
                        self.err(format!("unescaped control character U+{b:04X} in a string"))
                    );
                }
                _ => {
                    // Multi-byte UTF-8 sequences are copied whole. The source
                    // is a &str, so they are already known valid.
                    let rest = std::str::from_utf8(&self.src[self.at..])
                        .map_err(|_| self.err("invalid UTF-8"))?;
                    let c = rest.chars().next().expect("non-empty");
                    out.push(c);
                    self.at += c.len_utf8();
                }
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), ParseError> {
        let Some(b) = self.peek() else {
            return Err(self.err("the document ended after a backslash"));
        };
        self.at += 1;
        let c = match b {
            b'"' => '"',
            b'\\' => '\\',
            b'/' => '/',
            b'b' => '\u{08}',
            b'f' => '\u{0C}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => return self.unicode_escape(out),
            other => {
                return Err(self.err(format!("\\{} is not an escape", other as char)));
            }
        };
        out.push(c);
        Ok(())
    }

    fn unicode_escape(&mut self, out: &mut String) -> Result<(), ParseError> {
        let hi = self.hex4()?;
        // A high surrogate must be followed by its low half. Pushing a lone
        // surrogate is impossible in Rust anyway, but reporting it is better
        // than substituting U+FFFD and putting a replacement character on a
        // wire where the sender meant something else.
        if (0xD800..0xDC00).contains(&hi) {
            if !self.src[self.at..].starts_with(b"\\u") {
                return Err(self.err("a high surrogate must be followed by \\u"));
            }
            self.at += 2;
            let lo = self.hex4()?;
            if !(0xDC00..0xE000).contains(&lo) {
                return Err(self.err("a high surrogate must be followed by a low surrogate"));
            }
            let c = 0x1_0000 + ((u32::from(hi) - 0xD800) << 10) + (u32::from(lo) - 0xDC00);
            out.push(char::from_u32(c).expect("a valid surrogate pair is a valid scalar"));
            return Ok(());
        }
        if (0xDC00..0xE000).contains(&hi) {
            return Err(self.err("a low surrogate with nothing before it"));
        }
        out.push(char::from_u32(u32::from(hi)).expect("not a surrogate, so a valid scalar"));
        Ok(())
    }

    fn hex4(&mut self) -> Result<u16, ParseError> {
        if self.at + 4 > self.src.len() {
            return Err(self.err("\\u needs four hex digits"));
        }
        let mut v: u16 = 0;
        for i in 0..4 {
            let b = self.src[self.at + i];
            let d = match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                b'A'..=b'F' => b - b'A' + 10,
                _ => return Err(self.err("\\u needs four hex digits")),
            };
            v = v * 16 + u16::from(d);
        }
        self.at += 4;
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(src: &str) {
        let v = Json::parse(src).unwrap_or_else(|e| panic!("{src}: {e}"));
        let out = v.to_string();
        let again = Json::parse(&out).unwrap_or_else(|e| panic!("re-parse {out}: {e}"));
        assert_eq!(v, again, "{src} -> {out}");
    }

    #[test]
    fn documents_survive_a_round_trip() {
        for src in [
            "null",
            "true",
            "-0.5e+10",
            r#""plain""#,
            r#""quote \" backslash \\ newline \n tab \t""#,
            "[]",
            "{}",
            r#"[1,2,{"a":[true,null]}]"#,
            r#"{"_d":2,"_v":"x"}"#,
        ] {
            round_trip(src);
        }
    }

    /// The reason numbers keep their text: AnyJSON puts a 64-bit integer in a
    /// JSON *string*, but a number that big can still arrive, and turning it
    /// into an f64 on the way in would lose it before the mapping could
    /// complain.
    #[test]
    fn a_large_integer_keeps_every_digit() {
        let v = Json::parse("18446744073709551615").unwrap();
        assert_eq!(v, Json::Number("18446744073709551615".into()));
        assert_eq!(v.to_string(), "18446744073709551615");
    }

    #[test]
    fn korean_text_survives_both_directions() {
        let v = Json::parse(r#""안녕하세요""#).unwrap();
        assert_eq!(v.as_str(), Some("안녕하세요"));
        assert_eq!(v.to_string(), "\"안녕하세요\"");
    }

    #[test]
    fn a_surrogate_pair_becomes_one_character() {
        let v = Json::parse(r#""🛰""#).unwrap();
        assert_eq!(v.as_str(), Some("🛰"));
    }

    /// Nesting is a stack-depth question, not a style question: a few kilobytes
    /// of brackets must not be able to end the process.
    #[test]
    fn nesting_is_capped_rather_than_overflowing_the_stack() {
        let deep = "[".repeat(10_000) + &"]".repeat(10_000);
        let err = Json::parse(&deep).expect_err("must be refused");
        assert!(err.message.contains("nesting deeper"), "{err}");
    }

    #[test]
    fn the_grammar_is_rfc_8259_and_nothing_else() {
        for (src, why) in [
            ("[1,2,]", "trailing comma"),
            ("{'a':1}", "single quotes"),
            ("[1] [2]", "two documents"),
            ("01", "leading zero"),
            ("[1,2] // note", "comment"),
            ("NaN", "NaN literal"),
            ("{\"a\":1,\"a\":2}", "duplicate key"),
            ("\"raw\nnewline\"", "unescaped control character"),
            (".5", "no integer part"),
            ("1.", "no digit after the point"),
            ("1e", "no exponent digits"),
        ] {
            assert!(Json::parse(src).is_err(), "{why} must be rejected: {src}");
        }
    }

    /// Truncation is the common shape of hostile input, and every prefix of a
    /// valid document is truncated input. None may panic.
    #[test]
    fn every_truncation_of_a_document_fails_without_panicking() {
        let full = r#"{"a":[1,-2.5e3,"é",{"b":null}],"c":true}"#;
        // Character boundaries only: slicing a &str elsewhere panics in the
        // test rather than in the parser, which proves nothing about either.
        // The document contains an 'é' so that the filter is doing work.
        for i in (0..full.len()).filter(|i| full.is_char_boundary(*i)) {
            let _ = Json::parse(&full[..i]);
        }
    }

    #[test]
    fn a_lone_surrogate_is_reported_rather_than_replaced() {
        assert!(Json::parse(r#""\uD800""#).is_err());
        assert!(Json::parse(r#""\uDC00""#).is_err());
        assert!(Json::parse(r#""\uD800A""#).is_err());
    }

    /// RFC 8259 §7 forbids a raw control character inside a string, so writing
    /// one out unescaped would produce a document this very parser rejects.
    #[test]
    fn control_characters_are_escaped_on_the_way_out() {
        let v = Json::String("a\u{1}b".into());
        assert_eq!(v.to_string(), "\"a\\u0001b\"");
        assert_eq!(Json::parse(&v.to_string()).unwrap(), v, "and it reads back identically");
    }
}
