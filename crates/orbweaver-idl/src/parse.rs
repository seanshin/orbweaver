//! Recursive-descent parser for OMG IDL 4.2.
//!
//! Correctness here is defined by agreement with `omniidl`, not by taste: the
//! parser must accept every file in `corpus/golden/` and reject every file in
//! `corpus/negative/`. Where the grammar is ambiguous to a reader, the oracle
//! decides.
//!
//! Diagnostics are a product surface (`docs/PLAN.md` §3.3): every error says
//! what to do, not only what is wrong, because the self-repair loop is only as
//! good as the messages it feeds on.
//!
//! # Repository ids and `#pragma`
//!
//! The parser is where `#pragma prefix`, `#pragma version` and `#pragma ID`
//! are resolved, because all three are *positional*: a prefix is in effect
//! from where it is written to the end of the enclosing scope, and `version`
//! and `ID` name something already declared. Source order is the parser's, so
//! the resolution lives here and the result is a map of overrides on
//! [`Spec::repository_ids`]; the registry formats nothing it is not given.
//!
//! The rules implemented, each measured against `omniidl` by the cases in
//! `corpus/pragma/` (see `corpus/pragma/expected.tsv`):
//!
//! * `#pragma prefix "P"` applies to every declaration after it in the
//!   enclosing scope, and to nested scopes, until the scope closes or another
//!   `prefix` pragma replaces it. It does **not** escape a closing brace, and
//!   it does not retroactively change the enclosing scope's own id.
//! * **A prefix replaces the scope path so far; it is not prepended to it.**
//!   At file scope the two readings agree — `#pragma prefix "acme.com"` above
//!   `module bank` gives `IDL:acme.com/bank/Account:1.0` either way — and
//!   inside a module they do not: a prefix written in the body of `module p02`
//!   gives `IDL:acme.com/Ledger:1.0`, with `p02` **gone from the id**.
//!   Measured, not reasoned: `corpus/pragma/p02-prefix-inside-module.idl` and
//!   `p05-prefix-does-not-escape.idl`, and recorded in
//!   `corpus/divergences.tsv` because the specification's wording reads the
//!   other way. Interop is with deployed compilers, not with a document.
//! * `#pragma prefix ""` resets to no prefix — the scope path becomes empty
//!   rather than gaining a leading `/`.
//! * `#pragma version <name> M.m` replaces the `:1.0` of that item's id.
//! * `#pragma ID <name> "…"` replaces the whole id and wins over both the
//!   prefix and any `version`, whichever order they are written in.
//! * `#pragma orbweaver include-enter` / `include-leave` — **not a
//!   specification pragma**, and written by nobody: the pair
//!   [`crate::include`] injects around a spliced file. It saves the whole
//!   scope path and restarts at the empty one, then puts back exactly what it
//!   saved. It exists because `#pragma prefix` cannot express the restore —
//!   a prefix *replaces* the path, so it can name the includer's prefix but
//!   never the modules the `#include` was written inside, which is wrong for
//!   every `#include` that is not at file scope
//!   (`corpus/include/inc-scope-*.idl`, measured against omniidl and JacORB).
//!
//! ## What is not implemented
//!
//! Stated here rather than discovered later, because a partially-understood
//! identity rule silently produces wrong ids:
//!
//! * **`#pragma prefix` is only honoured at file, module and interface
//!   scope.** A pragma written between struct members, union cases, enum
//!   members or operation parameters is parsed and then dropped — those
//!   positions declare nothing that has a repository id of its own.
//! * **No other `#pragma` form has any effect.** `#pragma sendtop`,
//!   `#pragma inhibit_code_generation`, omniORB's `#pragma hh`/`#pragma
//!   validate_disconnect`, and the IDL-4 `typeid`/`typeprefix` *keywords* are
//!   all ignored. `typeid`/`typeprefix` are reserved words to the lexer, so a
//!   file using them fails at the grammar rather than being silently
//!   mis-identified.
//! * **The pragma names are matched case-sensitively** (`prefix`, `version`,
//!   `ID`) — see [`crate::lex`] for why leniency here is the dangerous
//!   direction.
//! * **`#pragma version` and `#pragma ID` resolve their name lexically from
//!   the current scope outwards**, over what has already been declared. A
//!   forward reference to something declared later in the file is not
//!   resolved and is reported as an error rather than guessed at.
//! * **An id given by `#pragma ID` is not validated.** It is put on the wire
//!   as written, including a non-`IDL:` format such as `RMI:` or `DCE:`; the
//!   registry's own ingestion validator is the thing that has an opinion
//!   about id syntax, and it only guards ids that arrive from a peer.

use std::collections::{BTreeMap, HashMap};

use crate::ast::*;
use crate::lex::{Annotation, LexError, Lexer, Pragma, Span, Tok, Token};

/// A parse failure, positioned and phrased as something to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// What to do about it.
    pub message: String,
    /// Where.
    pub span: Span,
    /// A stable identifier for the failure, matching `sema::Diagnostic::rule`.
    ///
    /// Most parse failures are "the grammar broke here" and share the generic
    /// `parse`, because the cause is rarely where the token is and a confident
    /// wrong hint costs a self-repair round. The ones with an unambiguous fix
    /// get their own name so tooling can offer it (§3.3).
    pub rule: &'static str,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.column, self.message)
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError { message: e.message, span: e.span, rule: "parse" }
    }
}

/// Result of a parse.
pub type Result<T> = std::result::Result<T, ParseError>;

/// One span covering two, keeping the start's line and column.
///
/// Used wherever a node spans more than one token. A node whose span covers
/// only its first token cannot be sliced out of the source, and every consumer
/// that tries gets a *plausible* wrong answer rather than an obvious one —
/// which is exactly how the corrupted fix hints of RC-2 survived.
fn span_over(start: Span, end: Span) -> Span {
    Span { start: start.start, end: end.end.max(start.end), line: start.line, column: start.column }
}

/// Parses a whole IDL source file.
pub fn parse(src: &str) -> Result<Spec> {
    let tokens = Lexer::new(src).tokenize()?;
    Parser {
        toks: tokens,
        i: 0,
        scope: Vec::new(),
        scope_ids: vec![String::new()],
        include_ids: Vec::new(),
        ids: BTreeMap::new(),
        declared: HashMap::new(),
        declared_scope: HashMap::new(),
        versions: HashMap::new(),
        explicit: HashMap::new(),
    }
    .spec()
}

struct Parser {
    toks: Vec<Token>,
    i: usize,
    /// The enclosing module/interface names, outermost first.
    scope: Vec<String>,
    /// The id path every declaration in the current scope hangs off, one entry
    /// per open scope; `""` at file scope with no prefix.
    ///
    /// Not "the prefix": `#pragma prefix` **replaces** this string rather than
    /// being prepended to it, which is what makes `p02::Ledger` come out as
    /// `IDL:acme.com/Ledger:1.0` with the enclosing module gone. Pushing a
    /// derived copy on entry and popping on exit is what stops a prefix
    /// escaping a closing brace.
    scope_ids: Vec<String>,
    /// The id paths saved by `#pragma orbweaver include-enter`, innermost last.
    ///
    /// A translation-unit file boundary saves and restores the whole id path,
    /// not just the prefix part of it. `#pragma prefix` cannot express the
    /// restore — it *replaces* the path, so it can name the includer's prefix
    /// but never the modules the `#include` was written inside — which is why
    /// this is a stack of its own rather than another `Prefix` pragma.
    /// Measured: `corpus/include/inc-scope-*.idl`.
    include_ids: Vec<String>,
    /// Repository ids that differ from the plain derivation, by qualified name.
    ids: BTreeMap<String, String>,
    /// Every qualified name declared so far, keyed by its lowercase form —
    /// what `#pragma version`/`#pragma ID` resolve their argument against.
    /// Lowercase because IDL resolves identifiers ignoring case.
    declared: HashMap<String, Vec<String>>,
    /// The scope id path each name was declared under, by qualified name.
    ///
    /// Kept because `#pragma version` arrives *after* the declaration and has
    /// to rebuild the id from the scope that applied at the declaration, not
    /// the one in effect at the pragma.
    declared_scope: HashMap<String, String>,
    /// Versions set by `#pragma version`, by qualified name.
    versions: HashMap<String, (u32, u32)>,
    /// Names whose id was given outright by `#pragma ID`. Nothing recomputes
    /// those: an explicit id overrides derivation entirely, so neither a
    /// prefix, a later `version`, nor a body following a forward declaration
    /// may quietly rebuild it.
    explicit: HashMap<String, ()>,
}

/// The repository id a path has with no pragma in play.
fn derived_id(path: &[String]) -> String {
    format!("IDL:{}:1.0", path.join("/"))
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.toks[self.i.min(self.toks.len() - 1)]
    }

    fn peek_tok(&self) -> &Tok {
        &self.peek().tok
    }

    fn at_kw(&self, kw: &str) -> bool {
        matches!(self.peek_tok(), Tok::Ident(s) if s == kw)
    }

    fn at_punct(&self, p: &str) -> bool {
        matches!(self.peek_tok(), Tok::Punct(x) if *x == p)
    }

    fn next(&mut self) -> Token {
        let t = self.toks[self.i.min(self.toks.len() - 1)].clone();
        if self.i < self.toks.len() - 1 {
            self.i += 1;
        }
        t
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.at_kw(kw) {
            self.next();
            true
        } else {
            false
        }
    }

    fn eat_punct(&mut self, p: &str) -> bool {
        if self.at_punct(p) {
            self.next();
            true
        } else {
            false
        }
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T> {
        Err(ParseError { message: message.into(), span: self.peek().span, rule: "parse" })
    }

    fn expect_punct(&mut self, p: &str) -> Result<()> {
        if self.eat_punct(p) {
            Ok(())
        } else {
            self.err(format!("expected {p:?}, found {}", self.peek_tok()))
        }
    }

    fn expect_ident(&mut self) -> Result<Named> {
        let t = self.next();
        match t.tok {
            Tok::Ident(text) => {
                // A keyword is only usable as a name when escaped with a
                // leading underscore. Accepting it unescaped lets
                // `long interface;` through, which the oracle rejects.
                if !t.escaped && crate::lex::is_keyword(&text) {
                    return Err(ParseError {
                        message: format!(
                            "{text:?} is a reserved word and cannot be a name here; \
                             write '_{text}' to use it as an identifier, or choose another name"
                        ),
                        span: t.span,
                        rule: "reserved-word",
                    });
                }
                Ok(Named { text, span: t.span })
            }
            other => Err(ParseError {
                message: format!("expected an identifier, found {other}"),
                span: t.span,
                rule: "parse",
            }),
        }
    }

    fn take_annotations(&mut self) -> Vec<Annotation> {
        self.peek().annotations.clone()
    }

    // ── repository ids ──────────────────────────────────────────────────────

    /// Consumes the identity pragmas written just before the current token.
    ///
    /// Called at the top of every definition-list loop, *before* the test for
    /// the closing brace, so the pragmas attached to a scope's `}` are seen
    /// too — that is where a `#pragma ID` naming the last item in a module
    /// lives.
    ///
    /// The pragmas are taken out of the token, not copied, so a loop that
    /// peeks repeatedly applies each one exactly once.
    fn apply_pragmas(&mut self) -> Result<()> {
        let i = self.i.min(self.toks.len() - 1);
        let pragmas = std::mem::take(&mut self.toks[i].pragmas);
        for at in pragmas {
            match at.pragma {
                Pragma::Prefix(p) => {
                    if let Some(current) = self.scope_ids.last_mut() {
                        *current = p.unwrap_or_default();
                    }
                }
                Pragma::Id { name, id } => {
                    let path = self.resolve_declared(&name, at.span, "ID")?;
                    let qual = path.join("::");
                    self.explicit.insert(qual.clone(), ());
                    self.ids.insert(qual, id);
                }
                Pragma::Version { name, major, minor } => {
                    let path = self.resolve_declared(&name, at.span, "version")?;
                    let qual = path.join("::");
                    self.versions.insert(qual.clone(), (major, minor));
                    self.set_id(&qual, &path);
                }
                // A file boundary saves the id path in force and restarts the
                // included file at the empty one, then puts back exactly what
                // it saved — enclosing modules included. Unconditional: it is
                // not "reset the prefix if there is one", because an included
                // file with no prefix anywhere still must not inherit the
                // module its `#include` was written inside
                // (`corpus/include/inc-scope-control.idl`, measured against
                // omniidl).
                Pragma::IncludeEnter => {
                    self.include_ids.push(self.scope_ids.last().cloned().unwrap_or_default());
                    if let Some(current) = self.scope_ids.last_mut() {
                        current.clear();
                    }
                }
                // An unmatched leave restores nothing rather than corrupting
                // the path: the markers are injected in pairs, so seeing one
                // alone means the text was hand-edited.
                Pragma::IncludeLeave => {
                    if let Some(saved) = self.include_ids.pop()
                        && let Some(current) = self.scope_ids.last_mut()
                    {
                        *current = saved;
                    }
                }
                Pragma::Other(_) => {}
            }
        }
        Ok(())
    }

    /// Finds what a `#pragma version`/`#pragma ID` argument names, searching
    /// from the current scope outwards the way IDL resolves any other name.
    fn resolve_declared(&self, name: &str, span: Span, what: &str) -> Result<Vec<String>> {
        let absolute = name.starts_with("::");
        let tail = name.trim_start_matches("::").to_lowercase();
        let miss = || ParseError {
            message: format!(
                "#pragma {what} names {name:?}, which is not declared in this scope; \
                 a pragma must come after the declaration it names"
            ),
            span,
            rule: "pragma-unknown-name",
        };
        if absolute {
            return self.declared.get(&tail).cloned().ok_or_else(miss);
        }
        for cut in (0..=self.scope.len()).rev() {
            let mut key = self.scope[..cut].join("::").to_lowercase();
            if !key.is_empty() {
                key.push_str("::");
            }
            key.push_str(&tail);
            if let Some(p) = self.declared.get(&key) {
                return Ok(p.clone());
            }
        }
        Err(miss())
    }

    /// Records a declaration at the current scope and derives its id.
    fn declare(&mut self, name: &str) {
        let mut path = self.scope.clone();
        path.push(name.to_owned());
        let qual = path.join("::");
        self.declared.insert(qual.to_lowercase(), path.clone());
        // A body following a forward declaration re-declares the same name;
        // the scope in effect is the same either way, and an explicit id is
        // left alone by `set_id`.
        let scope_id = self.scope_ids.last().cloned().unwrap_or_default();
        self.declared_scope.insert(qual.clone(), scope_id);
        self.set_id(&qual, &path);
    }

    /// Rebuilds `qual`'s id from its scope and version, or leaves an explicit
    /// one alone. Only a *difference* from the plain derivation is recorded.
    fn set_id(&mut self, qual: &str, path: &[String]) {
        if self.explicit.contains_key(qual) {
            return;
        }
        let name = path.last().cloned().unwrap_or_default();
        let body = match self.declared_scope.get(qual).map(String::as_str).unwrap_or_default() {
            "" => name,
            scope => format!("{scope}/{name}"),
        };
        let (major, minor) = self.versions.get(qual).copied().unwrap_or((1, 0));
        let id = format!("IDL:{body}:{major}.{minor}");
        if id == derived_id(path) {
            self.ids.remove(qual);
        } else {
            self.ids.insert(qual.to_owned(), id);
        }
    }

    // ── top level ───────────────────────────────────────────────────────────

    fn spec(&mut self) -> Result<Spec> {
        let mut definitions = Vec::new();
        loop {
            self.apply_pragmas()?;
            if matches!(self.peek_tok(), Tok::Eof) {
                break;
            }
            definitions.push(self.definition()?);
        }
        Ok(Spec { definitions, repository_ids: std::mem::take(&mut self.ids) })
    }

    fn definition(&mut self) -> Result<Definition> {
        let ann = self.take_annotations();
        let d = self.definition_inner(ann)?;
        self.expect_punct(";")?;
        // After the body, so the scope stack is back where it was: a prefix
        // set *inside* a module does not change that module's own id.
        self.declare(&d.name().text);
        Ok(d)
    }

    fn definition_inner(&mut self, ann: Vec<Annotation>) -> Result<Definition> {
        if self.at_kw("module") {
            return Ok(Definition::Module(self.module(ann)?));
        }
        if self.at_kw("interface") || self.at_kw("abstract") || self.at_kw("local") {
            // `abstract`/`local` also lead a valuetype, so look one further.
            if self.at_kw("abstract")
                && matches!(self.toks.get(self.i + 1).map(|t| &t.tok), Some(Tok::Ident(s)) if s == "valuetype")
            {
                return Ok(Definition::ValueType(self.valuetype(ann)?));
            }
            return Ok(Definition::Interface(self.interface(ann)?));
        }
        if self.at_kw("valuetype") {
            return Ok(Definition::ValueType(self.valuetype(ann)?));
        }
        if self.at_kw("struct") {
            return Ok(Definition::Struct(self.struct_def(ann, "struct")?));
        }
        if self.at_kw("exception") {
            return Ok(Definition::Exception(self.struct_def(ann, "exception")?));
        }
        if self.at_kw("union") {
            return Ok(Definition::Union(self.union_def(ann)?));
        }
        if self.at_kw("enum") {
            return Ok(Definition::Enum(self.enum_def(ann)?));
        }
        if self.at_kw("typedef") {
            self.next();
            return Ok(Definition::Typedef(self.typedef_body(ann)?));
        }
        if self.at_kw("const") {
            return Ok(Definition::Const(self.const_def(ann)?));
        }
        if self.at_kw("native") {
            self.next();
            return Ok(Definition::Native(self.expect_ident()?));
        }
        self.err(format!(
            "expected a definition (module, interface, struct, union, enum, exception, \
             typedef, const, valuetype or native), found {}",
            self.peek_tok()
        ))
    }

    fn module(&mut self, annotations: Vec<Annotation>) -> Result<Module> {
        self.next(); // module
        let name = self.expect_ident()?;
        self.expect_punct("{")?;
        self.enter(&name.text);
        let mut definitions = Vec::new();
        loop {
            self.apply_pragmas()?;
            if self.at_punct("}") {
                break;
            }
            if matches!(self.peek_tok(), Tok::Eof) {
                return self.err(format!("module {:?} is missing its closing '}}'", name.text));
            }
            definitions.push(self.definition()?);
        }
        self.leave();
        self.expect_punct("}")?;
        Ok(Module { name, definitions, annotations })
    }

    /// Opens a scope: the name joins both the qualified path and the id path,
    /// the latter by value so a `#pragma prefix` inside cannot leak back out.
    fn enter(&mut self, name: &str) {
        let child = match self.scope_ids.last().map(String::as_str).unwrap_or_default() {
            "" => name.to_owned(),
            parent => format!("{parent}/{name}"),
        };
        self.scope.push(name.to_owned());
        self.scope_ids.push(child);
    }

    fn leave(&mut self) {
        self.scope.pop();
        self.scope_ids.pop();
    }

    fn interface(&mut self, annotations: Vec<Annotation>) -> Result<Interface> {
        let modifier = if self.eat_kw("abstract") {
            Some(InterfaceModifier::Abstract)
        } else if self.eat_kw("local") {
            Some(InterfaceModifier::Local)
        } else {
            None
        };
        if !self.eat_kw("interface") {
            return self.err("expected 'interface'");
        }
        let name = self.expect_ident()?;

        // A forward declaration ends here.
        if self.at_punct(";") {
            return Ok(Interface { name, bases: Vec::new(), body: None, modifier, annotations });
        }

        let mut bases = Vec::new();
        if self.eat_punct(":") {
            loop {
                bases.push(self.scoped_name()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        self.expect_punct("{")?;
        self.enter(&name.text);
        let mut body = Vec::new();
        loop {
            self.apply_pragmas()?;
            if self.at_punct("}") {
                break;
            }
            if matches!(self.peek_tok(), Tok::Eof) {
                return self.err(format!("interface {:?} is missing its closing '}}'", name.text));
            }
            body.push(self.interface_member()?);
        }
        self.leave();
        self.expect_punct("}")?;
        Ok(Interface { name, bases, body: Some(body), modifier, annotations })
    }

    fn interface_member(&mut self) -> Result<InterfaceMember> {
        let ann = self.take_annotations();
        if self.at_kw("readonly") || self.at_kw("attribute") {
            let a = self.attribute(ann)?;
            self.expect_punct(";")?;
            return Ok(InterfaceMember::Attribute(a));
        }
        for kw in ["struct", "union", "enum", "exception", "typedef", "const", "native"] {
            if self.at_kw(kw) {
                let d = self.definition_inner(ann)?;
                self.expect_punct(";")?;
                self.declare(&d.name().text);
                return Ok(InterfaceMember::Nested(d));
            }
        }
        let op = self.operation(ann)?;
        self.expect_punct(";")?;
        Ok(InterfaceMember::Operation(op))
    }

    fn attribute(&mut self, annotations: Vec<Annotation>) -> Result<AttributeDef> {
        let readonly = self.eat_kw("readonly");
        if !self.eat_kw("attribute") {
            return self.err("expected 'attribute' after 'readonly'");
        }
        let ty = self.type_spec()?;
        let mut names = vec![self.expect_ident()?];
        while self.eat_punct(",") {
            names.push(self.expect_ident()?);
        }
        Ok(AttributeDef { readonly, ty, names, annotations })
    }

    fn operation(&mut self, annotations: Vec<Annotation>) -> Result<Operation> {
        let oneway = self.eat_kw("oneway");
        let returns = self.type_spec()?;
        let name = self.expect_ident()?;
        self.expect_punct("(")?;
        let mut params = Vec::new();
        if !self.at_punct(")") {
            loop {
                params.push(self.param()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        self.expect_punct(")")?;

        let mut raises = Vec::new();
        if self.eat_kw("raises") {
            self.expect_punct("(")?;
            loop {
                raises.push(self.scoped_name()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.expect_punct(")")?;
        }
        // `context` is accepted and discarded: it is legal grammar with no
        // bearing on the wire form we care about.
        if self.eat_kw("context") {
            self.expect_punct("(")?;
            while !self.at_punct(")") {
                self.next();
            }
            self.expect_punct(")")?;
        }
        Ok(Operation { name, returns, params, raises, oneway, annotations })
    }

    fn param(&mut self) -> Result<Param> {
        let annotations = self.take_annotations();
        let direction = if self.eat_kw("in") {
            Direction::In
        } else if self.eat_kw("out") {
            Direction::Out
        } else if self.eat_kw("inout") {
            Direction::InOut
        } else {
            return self.err(format!(
                "a parameter needs a direction: write 'in', 'out' or 'inout' before {}",
                self.peek_tok()
            ));
        };
        let ty = self.type_spec()?;
        let name = self.expect_ident()?;
        Ok(Param { direction, ty, name, annotations })
    }

    fn struct_def(&mut self, annotations: Vec<Annotation>, kw: &str) -> Result<StructDef> {
        self.next(); // struct / exception
        let name = self.expect_ident()?;
        if self.at_punct(";") {
            return Ok(StructDef { name, members: None, annotations });
        }
        self.expect_punct("{")?;
        let mut members = Vec::new();
        while !self.at_punct("}") {
            if matches!(self.peek_tok(), Tok::Eof) {
                return self.err(format!("{kw} {:?} is missing its closing '}}'", name.text));
            }
            members.push(self.member()?);
        }
        self.expect_punct("}")?;
        Ok(StructDef { name, members: Some(members), annotations })
    }

    fn member(&mut self) -> Result<Member> {
        let annotations = self.take_annotations();
        let ty = self.type_spec()?;
        let mut names = vec![self.declarator()?];
        while self.eat_punct(",") {
            names.push(self.declarator()?);
        }
        self.expect_punct(";")?;
        Ok(Member { ty, names, annotations })
    }

    /// A declarator is a name possibly followed by array dimensions.
    ///
    /// The dimensions are consumed and dropped here; `typedef` keeps them
    /// because that is where an array type is actually introduced.
    fn declarator(&mut self) -> Result<Named> {
        let n = self.expect_ident()?;
        while self.eat_punct("[") {
            self.const_expr()?;
            self.expect_punct("]")?;
        }
        Ok(n)
    }

    fn union_def(&mut self, annotations: Vec<Annotation>) -> Result<UnionDef> {
        self.next(); // union
        let name = self.expect_ident()?;
        if !self.eat_kw("switch") {
            return self.err(format!("union {:?} needs a 'switch (type)' clause", name.text));
        }
        self.expect_punct("(")?;
        let discriminator = self.type_spec()?;
        self.expect_punct(")")?;
        self.expect_punct("{")?;

        let mut cases = Vec::new();
        while !self.at_punct("}") {
            if matches!(self.peek_tok(), Tok::Eof) {
                return self.err(format!("union {:?} is missing its closing '}}'", name.text));
            }
            let mut labels = Vec::new();
            let mut is_default = false;
            // One member may carry several labels, and `default` is one of them.
            loop {
                if self.eat_kw("default") {
                    is_default = true;
                    self.expect_punct(":")?;
                } else if self.eat_kw("case") {
                    labels.push(self.const_expr()?);
                    self.expect_punct(":")?;
                } else {
                    break;
                }
            }
            if labels.is_empty() && !is_default {
                return self.err(format!(
                    "expected 'case' or 'default' inside union {:?}, found {}",
                    name.text,
                    self.peek_tok()
                ));
            }
            let member = self.member()?;
            cases.push(UnionCase { labels, is_default, member });
        }
        self.expect_punct("}")?;
        Ok(UnionDef { name, discriminator, cases, annotations })
    }

    fn enum_def(&mut self, annotations: Vec<Annotation>) -> Result<EnumDef> {
        self.next(); // enum
        let name = self.expect_ident()?;
        self.expect_punct("{")?;
        let mut members = Vec::new();
        loop {
            members.push(self.expect_ident()?);
            if !self.eat_punct(",") {
                break;
            }
            if self.at_punct("}") {
                break; // tolerate a trailing comma
            }
        }
        self.expect_punct("}")?;
        Ok(EnumDef { name, members, annotations })
    }

    fn typedef_body(&mut self, annotations: Vec<Annotation>) -> Result<Typedef> {
        let ty = self.type_spec()?;
        let name = self.expect_ident()?;
        let mut dimensions = Vec::new();
        while self.eat_punct("[") {
            dimensions.push(self.const_expr()?);
            self.expect_punct("]")?;
        }
        Ok(Typedef { ty, name, dimensions, annotations })
    }

    fn const_def(&mut self, annotations: Vec<Annotation>) -> Result<ConstDef> {
        self.next(); // const
        let ty = self.type_spec()?;
        let name = self.expect_ident()?;
        self.expect_punct("=")?;
        let value = self.const_expr()?;
        Ok(ConstDef { ty, name, value, annotations })
    }

    fn valuetype(&mut self, annotations: Vec<Annotation>) -> Result<ValueTypeDef> {
        let is_abstract = self.eat_kw("abstract");
        if !self.eat_kw("valuetype") {
            return self.err("expected 'valuetype'");
        }
        let name = self.expect_ident()?;
        if self.at_punct(";") {
            return Ok(ValueTypeDef {
                name,
                base: None,
                supports: Vec::new(),
                members: None,
                is_abstract,
                annotations,
            });
        }
        let mut base = None;
        let mut supports = Vec::new();
        if self.eat_punct(":") {
            self.eat_kw("truncatable");
            base = Some(self.scoped_name()?);
        }
        if self.eat_kw("supports") {
            loop {
                supports.push(self.scoped_name()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        self.expect_punct("{")?;
        self.enter(&name.text);
        let mut members = Vec::new();
        loop {
            self.apply_pragmas()?;
            if self.at_punct("}") {
                break;
            }
            if matches!(self.peek_tok(), Tok::Eof) {
                return self.err(format!("valuetype {:?} is missing its closing '}}'", name.text));
            }
            if self.at_kw("public") || self.at_kw("private") {
                let public = self.eat_kw("public");
                if !public {
                    self.next(); // private
                }
                members.push(ValueMember::State { public, member: self.member()? });
            } else {
                members.push(ValueMember::Other(Box::new(self.interface_member()?)));
            }
        }
        self.leave();
        self.expect_punct("}")?;
        Ok(ValueTypeDef { name, base, supports, members: Some(members), is_abstract, annotations })
    }

    // ── types ───────────────────────────────────────────────────────────────

    /// A scoped name, spanning **all** of it.
    ///
    /// The span used to be the first token's alone, which is wrong for every
    /// qualified name and catastrophically wrong for an absolute one: for
    /// `::MFS::Common::StringList` it covered the leading `::` and nothing
    /// else, so every consumer that slices the source with the span read the
    /// text `"::"`. `orbweaver-forge` builds its fix hint that way, which is
    /// how the estate got ~90 diagnostics advising the reader to *"qualify `::`
    /// with `Module::::`"* — the error line right and the actionable half
    /// wrong (`docs/pipeline-runs/2026-08-14-estate.md`, RC-2). A span is the
    /// extent of the thing it points at; anything less makes every span-slicing
    /// consumer wrong in its own way, which is why this is fixed here rather
    /// than in the consumer that noticed.
    fn scoped_name(&mut self) -> Result<ScopedName> {
        let start = self.peek().span;
        let absolute = self.eat_punct("::");
        let mut end = self.peek().span;
        let mut parts = vec![self.expect_ident()?.text];
        while self.eat_punct("::") {
            let t = self.expect_ident()?;
            end = t.span;
            parts.push(t.text);
        }
        Ok(ScopedName { absolute, parts, span: span_over(start, end) })
    }

    fn type_spec(&mut self) -> Result<TypeSpec> {
        // `unsigned` and `long` compose, so the integer types are matched as
        // sequences of words rather than single keywords.
        if self.eat_kw("unsigned") {
            if self.eat_kw("short") {
                return Ok(TypeSpec::UShort);
            }
            if self.eat_kw("long") {
                return Ok(if self.eat_kw("long") { TypeSpec::ULongLong } else { TypeSpec::ULong });
            }
            return self.err("'unsigned' must be followed by 'short' or 'long'");
        }
        if self.eat_kw("long") {
            if self.eat_kw("long") {
                return Ok(TypeSpec::LongLong);
            }
            if self.eat_kw("double") {
                return Ok(TypeSpec::LongDouble);
            }
            return Ok(TypeSpec::Long);
        }
        for (kw, ty) in [
            ("void", TypeSpec::Void),
            ("boolean", TypeSpec::Boolean),
            ("char", TypeSpec::Char),
            ("wchar", TypeSpec::WChar),
            ("octet", TypeSpec::Octet),
            ("short", TypeSpec::Short),
            ("float", TypeSpec::Float),
            ("double", TypeSpec::Double),
            ("any", TypeSpec::Any),
            ("Object", TypeSpec::Object),
            ("ValueBase", TypeSpec::ValueBase),
        ] {
            if self.eat_kw(kw) {
                return Ok(ty);
            }
        }
        if self.eat_kw("string") {
            return Ok(TypeSpec::String(self.optional_bound()?));
        }
        if self.eat_kw("wstring") {
            return Ok(TypeSpec::WString(self.optional_bound()?));
        }
        if self.eat_kw("fixed") {
            if self.eat_punct("<") {
                let digits = Box::new(self.const_expr()?);
                self.expect_punct(",")?;
                let scale = Box::new(self.const_expr()?);
                self.close_angle()?;
                return Ok(TypeSpec::Fixed { digits, scale });
            }
            return self.err("'fixed' needs <digits, scale>");
        }
        if self.eat_kw("sequence") {
            self.expect_punct("<")?;
            let element = Box::new(self.type_spec()?);
            let bound = if self.eat_punct(",") { Some(Box::new(self.const_expr()?)) } else { None };
            self.close_angle()?;
            return Ok(TypeSpec::Sequence { element, bound });
        }
        if matches!(self.peek_tok(), Tok::Ident(_)) || self.at_punct("::") {
            return Ok(TypeSpec::Named(self.scoped_name()?));
        }
        self.err(format!("expected a type, found {}", self.peek_tok()))
    }

    fn optional_bound(&mut self) -> Result<Option<Box<ConstExpr>>> {
        if self.eat_punct("<") {
            let b = Box::new(self.const_expr()?);
            self.close_angle()?;
            Ok(Some(b))
        } else {
            Ok(None)
        }
    }

    /// Closes a `<...>`, splitting a `>>` token when nested generics produced
    /// one.
    ///
    /// `sequence<sequence<long>>` lexes its tail as a single shift operator.
    /// Treating that as one closing bracket loses a level and the error
    /// surfaces far from the cause.
    fn close_angle(&mut self) -> Result<()> {
        if self.eat_punct(">") {
            return Ok(());
        }
        if self.at_punct(">>") {
            // Rewrite the token in place so the outer nesting sees its own '>'.
            let span = self.peek().span;
            self.toks[self.i] = Token {
                tok: Tok::Punct(">"),
                span,
                annotations: Vec::new(),
                pragmas: Vec::new(),
                escaped: false,
            };
            return Ok(());
        }
        self.err(format!("expected '>', found {}", self.peek_tok()))
    }

    // ── constant expressions ────────────────────────────────────────────────

    fn const_expr(&mut self) -> Result<ConstExpr> {
        self.or_expr()
    }

    fn binary_level(
        &mut self,
        ops: &[&'static str],
        next: fn(&mut Self) -> Result<ConstExpr>,
    ) -> Result<ConstExpr> {
        let mut left = next(self)?;
        loop {
            let Some(op) = ops.iter().find(|o| self.at_punct(o)) else { return Ok(left) };
            self.next();
            let right = next(self)?;
            left = ConstExpr::Binary { op, left: Box::new(left), right: Box::new(right) };
        }
    }

    fn or_expr(&mut self) -> Result<ConstExpr> {
        self.binary_level(&["|"], Self::xor_expr)
    }
    fn xor_expr(&mut self) -> Result<ConstExpr> {
        self.binary_level(&["^"], Self::and_expr)
    }
    fn and_expr(&mut self) -> Result<ConstExpr> {
        self.binary_level(&["&"], Self::shift_expr)
    }
    fn shift_expr(&mut self) -> Result<ConstExpr> {
        self.binary_level(&["<<", ">>"], Self::add_expr)
    }
    fn add_expr(&mut self) -> Result<ConstExpr> {
        self.binary_level(&["+", "-"], Self::mul_expr)
    }
    fn mul_expr(&mut self) -> Result<ConstExpr> {
        self.binary_level(&["*", "/", "%"], Self::unary_expr)
    }

    fn unary_expr(&mut self) -> Result<ConstExpr> {
        for op in ["-", "+", "~"] {
            if self.at_punct(op) {
                self.next();
                let operand = Box::new(self.unary_expr()?);
                let op: &'static str = ["-", "+", "~"].iter().find(|o| **o == op).unwrap();
                return Ok(ConstExpr::Unary { op, operand });
            }
        }
        self.primary_expr()
    }

    fn primary_expr(&mut self) -> Result<ConstExpr> {
        if self.eat_punct("(") {
            let e = self.const_expr()?;
            self.expect_punct(")")?;
            return Ok(e);
        }
        let t = self.next();
        match t.tok {
            Tok::Int(v) => Ok(ConstExpr::Int(v)),
            Tok::Float(v) => Ok(ConstExpr::Float(v)),
            Tok::Str(v) => Ok(ConstExpr::Str(v)),
            Tok::Char(v) => Ok(ConstExpr::Char(v)),
            Tok::Ident(s) if s == "TRUE" => Ok(ConstExpr::Bool(true)),
            Tok::Ident(s) if s == "FALSE" => Ok(ConstExpr::Bool(false)),
            // Both arms span the whole name, for the reason `scoped_name`
            // spells out: a span that stops at the first token is a span that
            // slices to the wrong text.
            Tok::Ident(s) => {
                let mut parts = vec![s];
                let mut end = t.span;
                while self.eat_punct("::") {
                    let n = self.expect_ident()?;
                    end = n.span;
                    parts.push(n.text);
                }
                Ok(ConstExpr::Name(ScopedName {
                    absolute: false,
                    parts,
                    span: span_over(t.span, end),
                }))
            }
            Tok::Punct("::") => {
                let first = self.expect_ident()?;
                let mut end = first.span;
                let mut parts = vec![first.text];
                while self.eat_punct("::") {
                    let n = self.expect_ident()?;
                    end = n.span;
                    parts.push(n.text);
                }
                Ok(ConstExpr::Name(ScopedName {
                    absolute: true,
                    parts,
                    span: span_over(t.span, end),
                }))
            }
            other => Err(ParseError {
                message: format!("expected a constant expression, found {other}"),
                span: t.span,
                rule: "parse",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The override map, which is empty unless an identity pragma moved
    /// something.
    fn ids(src: &str) -> BTreeMap<String, String> {
        parse(src).expect("parses").repository_ids
    }

    fn id(src: &str, qualified: &str) -> String {
        ids(src).get(qualified).cloned().unwrap_or_else(|| panic!("{qualified} was not overridden"))
    }

    /// The text a scoped name's span slices out of the source.
    ///
    /// This is the property RC-2 turned on. Anything that renders a diagnostic
    /// by slicing the source with the span — `orbweaver-forge`'s fix hints do —
    /// reads exactly this, so it is asserted as the string it must be rather
    /// than as an offset.
    fn spanned_type_of_member(src: &str) -> String {
        let spec = parse(src).expect("parses");
        let mut found = None;
        for d in &spec.definitions {
            if let Definition::Module(m) = d {
                for inner in &m.definitions {
                    if let Definition::Struct(s) = inner
                        && let Some(members) = &s.members
                        && let TypeSpec::Named(n) = &members[0].ty
                    {
                        found = Some(n.span);
                    }
                }
            }
        }
        let span = found.expect("a struct member with a named type");
        src[span.start..span.end].to_owned()
    }

    /// A qualified name's span covers the whole name, not its first token.
    ///
    /// Before this, the span of `::MFS::Common::StringList` was the leading
    /// `::` alone, so every consumer that sliced the source with it read `"::"`
    /// — which is how the estate's fix hints came to advise qualifying `::`
    /// with `Module::::` (`docs/pipeline-runs/2026-08-14-estate.md`, RC-2). The
    /// relative form was wrong in the same way and quieter about it: it sliced
    /// to `MFS`, which looks like a name and is not the one in the message.
    #[test]
    fn a_scoped_names_span_covers_all_of_it() {
        assert_eq!(spanned_type_of_member("module m { struct S { ::a::b::C x; }; };"), "::a::b::C");
        assert_eq!(spanned_type_of_member("module m { struct S { a::b::C x; }; };"), "a::b::C");
        assert_eq!(spanned_type_of_member("module m { struct S { ::C x; }; };"), "::C");
        // The unqualified case was already right and must stay right.
        assert_eq!(spanned_type_of_member("module m { struct S { C x; }; };"), "C");
        // Whitespace inside a scoped name is legal and the span still ends at
        // the last component rather than at the last token it happened to see.
        assert_eq!(spanned_type_of_member("module m { struct S { a :: b x; }; };"), "a :: b");
    }

    /// The same property for a scoped name in a constant expression, which is
    /// parsed by a different function and was wrong in both of its arms.
    #[test]
    fn a_scoped_name_in_a_const_expression_spans_all_of_it() {
        for (src, want) in [
            ("module m { const long L = ::a::B; };", "::a::B"),
            ("module m { const long L = a::B; };", "a::B"),
        ] {
            let spec = parse(src).expect("parses");
            let Definition::Module(md) = &spec.definitions[0] else { panic!("module") };
            let Definition::Const(c) = &md.definitions[0] else { panic!("const") };
            let ConstExpr::Name(n) = &c.value else { panic!("a name, got {:?}", c.value) };
            assert_eq!(&src[n.span.start..n.span.end], want);
        }
    }

    /// The measured defect this batch closed: without the pragma we said
    /// `IDL:bank/Account:1.0` and omniidl said `IDL:acme.com/bank/Account:1.0`,
    /// so against any deployment with a prefix we disagreed about the identity
    /// of every type while looking correct locally.
    #[test]
    fn a_file_scope_prefix_leads_the_id() {
        assert_eq!(
            id(
                "#pragma prefix \"acme.com\"\n\
                 module bank { interface Account { long balance(); }; };",
                "bank::Account"
            ),
            "IDL:acme.com/bank/Account:1.0"
        );
    }

    /// A prefix inside a module **replaces** the scope path rather than
    /// leading it — the enclosing module is gone from the id. omniidl's answer
    /// (`corpus/pragma/p02`), against the specification's wording; the reason
    /// we follow it is in `corpus/divergences.tsv`.
    #[test]
    fn a_prefix_inside_a_module_replaces_the_scope_path() {
        assert_eq!(
            id(
                "module m {\n    #pragma prefix \"acme.com\"\n    interface I { void a(); };\n};",
                "m::I"
            ),
            "IDL:acme.com/I:1.0"
        );
    }

    /// The rule an implementation gets wrong by keeping one variable instead
    /// of a stack. Invisible in a one-module file.
    /// The file-boundary marker saves and restores the whole id path, which is
    /// the one thing `#pragma prefix` cannot be made to do.
    ///
    /// Written here as raw text because that is what `crate::include` injects,
    /// and because the pair has to keep working when somebody reads the output
    /// of `idl-check -E` back in. `Yard::I` is `IDL:aaa/Yard/I:1.0` — with the
    /// module — while a `#pragma prefix "aaa"` in the same position would give
    /// `IDL:aaa/I:1.0`. Both oracles say the former; see
    /// `corpus/include/inc-scope-plain.idl`.
    #[test]
    fn an_include_boundary_saves_and_restores_the_whole_id_path() {
        let out = ids("#pragma prefix \"aaa\"\n\
             module Yard {\n\
             #pragma orbweaver include-enter\n\
             module N { interface J { void f(); }; };\n\
             #pragma orbweaver include-leave\n\
             interface I { void g(); };\n\
             };");
        assert_eq!(out.get("Yard::N::J").map(String::as_str), Some("IDL:N/J:1.0"));
        assert_eq!(out.get("Yard::I").map(String::as_str), Some("IDL:aaa/Yard/I:1.0"));
    }

    /// Nested boundaries restore innermost-first, so a chain of includes two
    /// deep does not put an outer file's path back one level early.
    #[test]
    fn include_boundaries_nest() {
        let out = ids("module A {\n\
             #pragma orbweaver include-enter\n\
             module B {\n\
             #pragma orbweaver include-enter\n\
             interface Deep { void f(); };\n\
             #pragma orbweaver include-leave\n\
             interface Mid { void g(); };\n\
             };\n\
             #pragma orbweaver include-leave\n\
             interface Top { void h(); };\n\
             };");
        // The innermost file starts empty; `B` came from the middle file, which
        // itself started empty, so `Mid` keeps `B` and `Deep` keeps nothing.
        assert_eq!(out.get("A::B::Deep").map(String::as_str), Some("IDL:Deep:1.0"));
        assert_eq!(out.get("A::B::Mid").map(String::as_str), Some("IDL:B/Mid:1.0"));
        // ...and the root gets its own path back, `A` included.
        assert_eq!(out.get("A::Top"), None, "IDL:A/Top:1.0 is the plain derivation");
    }

    /// The `orbweaver` pragma namespace belongs to include resolution, and a
    /// spelling it does not inject is an error rather than a silent no-op.
    ///
    /// Silence is the dangerous direction here for the same reason `#pragma id`
    /// is not accepted as `#pragma ID`: a marker we ignore is a repository id
    /// nobody warns about.
    #[test]
    fn an_unknown_orbweaver_pragma_is_an_error() {
        let e = crate::parse("#pragma orbweaver include-enters\nmodule M { };")
            .expect_err("an unknown marker must not be ignored");
        assert!(e.message.contains("orbweaver"), "{}", e.message);
    }

    #[test]
    fn a_prefix_does_not_escape_its_scope() {
        let out = ids("module a {\n#pragma prefix \"in.example\"\ninterface I { void f(); }; };\n\
             module b { interface J { void g(); }; };");
        assert_eq!(out.get("a::I").unwrap(), "IDL:in.example/I:1.0");
        assert_eq!(out.get("b::J"), None, "b is untouched, so it has no override at all");
    }

    #[test]
    fn nested_scopes_inherit_the_prefix() {
        assert_eq!(
            id(
                "#pragma prefix \"acme.com\"\n\
                 module a { module b { interface I { void f(); }; }; };",
                "a::b::I"
            ),
            "IDL:acme.com/a/b/I:1.0"
        );
    }

    #[test]
    fn a_second_prefix_replaces_the_first_and_an_empty_one_resets() {
        let out = ids("#pragma prefix \"one.example\"\nmodule a { interface I { void f(); }; };\n\
             #pragma prefix \"two.example\"\nmodule b { interface J { void g(); }; };\n\
             #pragma prefix \"\"\nmodule c { interface K { void h(); }; };");
        assert_eq!(out.get("a::I").unwrap(), "IDL:one.example/a/I:1.0");
        assert_eq!(out.get("b::J").unwrap(), "IDL:two.example/b/J:1.0");
        assert_eq!(out.get("c::K"), None, "reset to none, not to an empty leading segment");
    }

    #[test]
    fn version_sets_the_version_part_and_prefix_sets_the_rest() {
        assert_eq!(
            id("module m { interface I { void f(); };\n#pragma version I 2.3\n};", "m::I"),
            "IDL:m/I:2.3"
        );
        assert_eq!(
            id(
                "#pragma prefix \"acme.com\"\n\
                 module m { interface I { void f(); };\n#pragma version I 5.4\n};",
                "m::I"
            ),
            "IDL:acme.com/m/I:5.4"
        );
    }

    /// An explicit id overrides derivation entirely — and in either order, so
    /// a `version` pragma cannot reach inside one and edit it.
    #[test]
    fn an_explicit_id_wins_over_prefix_and_version() {
        assert_eq!(
            id(
                "#pragma prefix \"acme.com\"\n\
                 module m { interface I { void f(); };\n\
                 #pragma ID I \"IDL:other/Thing:7.2\"\n#pragma version I 9.9\n};",
                "m::I"
            ),
            "IDL:other/Thing:7.2"
        );
    }

    /// A body following a forward declaration must not quietly rebuild an id
    /// the author pinned between the two.
    #[test]
    fn a_body_after_a_forward_declaration_does_not_undo_a_pinned_id() {
        assert_eq!(
            id(
                "module m { interface I;\n#pragma ID I \"IDL:pinned/I:1.0\"\n\
                 interface I { void f(); }; };",
                "m::I"
            ),
            "IDL:pinned/I:1.0"
        );
    }

    /// A pragma at the very end of a scope still applies: it rides on the
    /// closing brace, which is the token the definition loop stops at.
    #[test]
    fn a_pragma_before_a_closing_brace_is_seen() {
        assert_eq!(
            id(
                "module m { interface I { void f(); };\n#pragma ID I \"IDL:late/I:1.0\"\n};",
                "m::I"
            ),
            "IDL:late/I:1.0"
        );
    }

    /// Naming something that does not exist is an error with a fix in it, not
    /// a silently derived id. A pragma must follow its declaration.
    #[test]
    fn a_pragma_naming_nothing_is_reported() {
        let e = parse("module m { interface I { void f(); };\n#pragma ID Nope \"IDL:x:1.0\"\n};")
            .unwrap_err();
        assert_eq!(e.rule, "pragma-unknown-name");
        assert!(e.message.contains("after the declaration"), "{}", e.message);
    }

    /// The invariant the whole change rests on: a file with no identity pragma
    /// records nothing, so nothing downstream can move.
    #[test]
    fn a_file_without_pragmas_records_no_overrides() {
        assert!(
            ids("module m { interface I { void f(); }; struct S { long x; }; };").is_empty(),
            "no prefix means no change"
        );
    }
}
