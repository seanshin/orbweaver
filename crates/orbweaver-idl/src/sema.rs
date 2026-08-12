//! Semantic analysis: scopes, name resolution, and the identifier rules that
//! cost this project more than anything else.
//!
//! The parser accepts anything shaped like IDL. This pass decides whether it
//! *means* anything, and its acceptance criterion is the same as the parser's:
//! agreement with the oracle across `corpus/golden/` and `corpus/negative/`.
//!
//! # The rule that keeps costing us
//!
//! IDL compares identifiers **ignoring case**, and it does so in two directions
//! that read as one rule and behave as two:
//!
//! 1. **Within a scope, an identifier has one spelling.** Once `Position` has
//!    been used there, declaring `position` in the same scope is an error —
//!    which is why `struct Track { Position position; }` does not compile.
//! 2. **A declaration may not take the name of an enclosing scope.** Hence
//!    `module inventory { interface Inventory ... }` and
//!    `struct Version { unsigned long version; }`.
//!
//! Both are natural naming in every other language, which is exactly why they
//! accounted for every failure in the Phase 0 assumption B benchmark and have
//! since taken four distinct syntactic shapes. `spikes/idl_lint.py` was the
//! interim home for a regex approximation of them; this is the real one.

use std::collections::HashMap;

use crate::ast::*;
use crate::lex::Span;

/// A semantic problem, phrased as something to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// What is wrong and what to do about it.
    pub message: String,
    /// Where.
    pub span: Span,
    /// A stable identifier for the rule, so tooling can group or suppress.
    pub rule: &'static str,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {} [{}]", self.span.line, self.span.column, self.message, self.rule)
    }
}

/// What an identifier names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum SymbolKind {
    Module,
    Interface,
    ValueType,
    Struct,
    Union,
    Enum,
    Exception,
    Typedef,
    Native,
    Const,
    Enumerator,
    Member,
    Operation,
    Attribute,
    Parameter,
}

impl SymbolKind {
    /// Whether the symbol can be referred to as a type.
    fn is_type(self) -> bool {
        matches!(
            self,
            SymbolKind::Interface
                | SymbolKind::ValueType
                | SymbolKind::Struct
                | SymbolKind::Union
                | SymbolKind::Enum
                | SymbolKind::Exception
                | SymbolKind::Typedef
                | SymbolKind::Native
        )
    }
}

/// One resolved declaration.
#[derive(Debug, Clone)]
pub struct Symbol {
    /// Name as written.
    pub name: String,
    /// What it is.
    pub kind: SymbolKind,
    /// Where it was declared.
    pub span: Span,
    /// Index of the scope it introduces, for the kinds that introduce one.
    pub scope: Option<usize>,
    /// Whether a body has been seen, as opposed to a forward declaration.
    ///
    /// Without this, "a symbol of the same kind already exists" reads as
    /// "this is the definition of that forward declaration" — which silently
    /// merged `struct A` and `struct a` in a reopened module instead of
    /// reporting the clash the oracle reports.
    pub defined: bool,
}

/// A lexical scope.
#[derive(Debug, Default)]
struct Scope {
    /// Name of the scope itself, empty for the global scope.
    name: String,
    parent: Option<usize>,
    /// Declared symbols, keyed by lowercase name.
    symbols: HashMap<String, Symbol>,
    /// Every identifier spelling seen in this scope, keyed by lowercase form.
    ///
    /// Records *uses* as well as declarations, because IDL fixes an
    /// identifier's spelling per scope on first appearance either way.
    spellings: HashMap<String, (String, Span)>,
    /// Scopes whose symbols are also visible here, for interface inheritance.
    inherited: Vec<usize>,
}

/// The result of analysing a specification.
#[derive(Debug)]
pub struct Analysis {
    scopes: Vec<Scope>,
    /// Everything wrong with the input, in source order.
    pub diagnostics: Vec<Diagnostic>,
}

impl Analysis {
    /// Whether the specification is semantically valid.
    pub fn is_ok(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Number of scopes, including the global one and the predefined `CORBA`.
    pub fn scope_count(&self) -> usize {
        self.scopes.len()
    }
}

/// Analyses a parsed specification.
pub fn analyse(spec: &Spec) -> Analysis {
    let mut a = Analyser {
        scopes: vec![Scope::default()],
        diagnostics: Vec::new(),
        deferred: Vec::new(),
    };
    a.install_corba_module();
    a.collect_definitions(0, &spec.definitions);
    a.resolve_deferred();
    a.diagnostics.sort_by_key(|d| (d.span.line, d.span.column));
    Analysis { scopes: a.scopes, diagnostics: a.diagnostics }
}

/// A name reference held back until every declaration is known.
///
/// IDL allows a type to be used before it is declared within the same scope,
/// so resolution cannot happen during collection without producing false
/// "not found" errors on perfectly ordinary files.
struct Deferred {
    scope: usize,
    name: ScopedName,
    want_type: bool,
}

struct Analyser {
    scopes: Vec<Scope>,
    diagnostics: Vec<Diagnostic>,
    deferred: Vec<Deferred>,
}

impl Analyser {
    fn push_scope(&mut self, parent: usize, name: &str) -> usize {
        let id = self.scopes.len();
        self.scopes.push(Scope {
            name: name.to_owned(),
            parent: Some(parent),
            ..Scope::default()
        });
        id
    }

    /// Predeclares the `CORBA` module so `::CORBA::TypeCode` resolves.
    ///
    /// Without it, `TypeCode` is simply an unknown name — which is correct, and
    /// is what `corpus/negative/n05` pins — but the qualified spelling that
    /// *is* legal has to resolve to something.
    fn install_corba_module(&mut self) {
        let corba = self.push_scope(0, "CORBA");
        for (name, kind) in [
            ("TypeCode", SymbolKind::Native),
            ("Object", SymbolKind::Interface),
            ("ValueBase", SymbolKind::ValueType),
            ("Principal", SymbolKind::Native),
        ] {
            self.scopes[corba].symbols.insert(
                name.to_lowercase(),
                Symbol { name: name.into(), kind, span: Span::empty(), scope: None, defined: true },
            );
        }
        self.scopes[0].symbols.insert(
            "corba".into(),
            Symbol {
                name: "CORBA".into(),
                kind: SymbolKind::Module,
                span: Span::empty(),
                scope: Some(corba),
                defined: true,
            },
        );
    }

    /// Records an identifier's spelling in a scope and reports a case conflict.
    ///
    /// This is rule 1: within a scope an identifier has exactly one spelling,
    /// whether it appears as a declaration or as a reference.
    fn note_spelling(&mut self, scope: usize, text: &str, span: Span) {
        let key = text.to_lowercase();
        match self.scopes[scope].spellings.get(&key) {
            Some((first, _)) if first != text => {
                let first = first.clone();
                self.diagnostics.push(Diagnostic {
                    message: format!(
                        "{text:?} clashes with {first:?} in the same scope — IDL compares \
                         identifiers ignoring case. Rename one of them; the usual fix is to \
                         change the member or parameter rather than the type, since the type \
                         name is what callers depend on"
                    ),
                    span,
                    rule: "identifier-case-clash",
                });
            }
            Some(_) => {}
            None => {
                self.scopes[scope].spellings.insert(key, (text.to_owned(), span));
            }
        }
    }

    /// Reports rule 2: a declaration may not take an enclosing scope's name.
    fn check_enclosing(&mut self, scope: usize, name: &Named) {
        let mut cur = Some(scope);
        while let Some(id) = cur {
            let sname = self.scopes[id].name.clone();
            if !sname.is_empty() && name.clashes_with(&sname) {
                self.diagnostics.push(Diagnostic {
                    message: format!(
                        "{:?} clashes with its enclosing scope {sname:?} — IDL compares \
                         identifiers ignoring case, so a declaration cannot reuse the name of \
                         a scope it sits inside. Rename the inner one; the outer path is what \
                         callers import",
                        name.text
                    ),
                    span: name.span,
                    rule: "enclosing-scope-clash",
                });
                return;
            }
            cur = self.scopes[id].parent;
        }
    }

    fn declare(&mut self, scope: usize, name: &Named, kind: SymbolKind, inner: Option<usize>) {
        self.declare_with(scope, name, kind, inner, true)
    }

    fn declare_with(
        &mut self,
        scope: usize,
        name: &Named,
        kind: SymbolKind,
        inner: Option<usize>,
        defined: bool,
    ) {
        self.check_enclosing(scope, name);
        let key = name.text.to_lowercase();
        // An inherited name is visible here, so declaring over it collides —
        // omniidl reports "clashes with inherited operation".
        let inherited: Vec<usize> = self.scopes[scope].inherited.clone();
        for base in inherited {
            if let Some(prev) = self.lookup_in(base, &name.text) {
                self.diagnostics.push(Diagnostic {
                    message: format!(
                        "{:?} clashes with the inherited {:?} {:?} — a derived interface cannot \
                         redeclare a name it already has",
                        name.text, prev.kind, prev.name
                    ),
                    span: name.span,
                    rule: "inherited-clash",
                });
                return;
            }
        }
        if let Some(prev) = self.scopes[scope].symbols.get(&key) {
            let prev_line = prev.span.line;
            self.diagnostics.push(Diagnostic {
                message: format!(
                    "{:?} is already declared in this scope{} — IDL compares identifiers \
                     ignoring case",
                    name.text,
                    if prev_line > 0 { format!(" at line {prev_line}") } else { String::new() }
                ),
                span: name.span,
                rule: "duplicate-declaration",
            });
            return;
        }
        self.note_spelling(scope, &name.text, name.span);
        self.scopes[scope].symbols.insert(
            key,
            Symbol { name: name.text.clone(), kind, span: name.span, scope: inner, defined },
        );
    }

    /// Whether an existing symbol is the forward declaration `name` completes.
    ///
    /// Requires the spelling to match exactly: a differently-cased name is a
    /// clash, not a completion.
    fn completes_forward(&self, scope: usize, name: &Named, kind: SymbolKind) -> Option<usize> {
        let prev = self.scopes[scope].symbols.get(&name.text.to_lowercase())?;
        (prev.kind == kind && !prev.defined && prev.name == name.text).then_some(prev.scope)?
    }

    fn mark_defined(&mut self, scope: usize, name: &Named) {
        if let Some(s) = self.scopes[scope].symbols.get_mut(&name.text.to_lowercase()) {
            s.defined = true;
        }
    }

    // ── collection ──────────────────────────────────────────────────────────

    fn collect_definitions(&mut self, scope: usize, defs: &[Definition]) {
        for d in defs {
            self.collect_definition(scope, d);
        }
    }

    fn collect_definition(&mut self, scope: usize, d: &Definition) {
        match d {
            Definition::Module(m) => {
                // A reopened module continues the scope it already has, which
                // is legal and appears in the golden corpus.
                let inner = match self.scopes[scope].symbols.get(&m.name.text.to_lowercase()) {
                    // Reopening is legal; reopening under a different spelling
                    // is the clash the oracle reports.
                    Some(s) if s.kind == SymbolKind::Module && s.name == m.name.text => {
                        s.scope.expect("module has a scope")
                    }
                    _ => {
                        let inner = self.push_scope(scope, &m.name.text);
                        self.declare(scope, &m.name, SymbolKind::Module, Some(inner));
                        inner
                    }
                };
                self.collect_definitions(inner, &m.definitions);
            }
            Definition::Interface(i) => {
                let Some(body) = &i.body else {
                    // A forward declaration introduces the name only.
                    if !self.scopes[scope].symbols.contains_key(&i.name.text.to_lowercase()) {
                        let inner = self.push_scope(scope, &i.name.text);
                        self.declare_with(scope, &i.name, SymbolKind::Interface, Some(inner), false);
                    }
                    return;
                };
                let inner = match self.completes_forward(scope, &i.name, SymbolKind::Interface) {
                    Some(existing) => {
                        self.mark_defined(scope, &i.name);
                        existing
                    }
                    None => {
                        let inner = self.push_scope(scope, &i.name.text);
                        self.declare(scope, &i.name, SymbolKind::Interface, Some(inner));
                        inner
                    }
                };
                for b in &i.bases {
                    self.reference(scope, b, true);
                    // Base scopes contribute names, so an inherited operation
                    // still collides with a locally declared one.
                    if let Some(sym) = self.lookup(scope, b)
                        && let Some(s) = sym.scope
                    {
                        self.scopes[inner].inherited.push(s);
                    }
                }
                for m in body {
                    self.collect_interface_member(inner, m);
                }
            }
            Definition::Struct(s) | Definition::Exception(s) => {
                let kind = if matches!(d, Definition::Struct(_)) {
                    SymbolKind::Struct
                } else {
                    SymbolKind::Exception
                };
                let Some(members) = &s.members else {
                    if !self.scopes[scope].symbols.contains_key(&s.name.text.to_lowercase()) {
                        let inner = self.push_scope(scope, &s.name.text);
                        self.declare_with(scope, &s.name, kind, Some(inner), false);
                    }
                    return;
                };
                let inner = match self.completes_forward(scope, &s.name, kind) {
                    Some(existing) => {
                        self.mark_defined(scope, &s.name);
                        existing
                    }
                    None => {
                        let inner = self.push_scope(scope, &s.name.text);
                        self.declare(scope, &s.name, kind, Some(inner));
                        inner
                    }
                };
                for m in members {
                    self.collect_member(inner, m);
                }
            }
            Definition::Union(u) => {
                let inner = self.push_scope(scope, &u.name.text);
                self.declare(scope, &u.name, SymbolKind::Union, Some(inner));
                self.type_spec(scope, &u.discriminator);
                self.check_union_labels(u);
                for c in &u.cases {
                    self.collect_member(inner, &c.member);
                }
            }
            Definition::Enum(e) => {
                self.declare(scope, &e.name, SymbolKind::Enum, None);
                // Enumerators live in the *enclosing* scope, not inside the
                // enum, so two enums in one module cannot share a member name.
                for m in &e.members {
                    self.declare(scope, m, SymbolKind::Enumerator, None);
                }
            }
            Definition::Typedef(t) => {
                self.type_spec(scope, &t.ty);
                self.declare(scope, &t.name, SymbolKind::Typedef, None);
            }
            Definition::Const(c) => {
                self.type_spec(scope, &c.ty);
                self.const_expr(scope, &c.value);
                self.declare(scope, &c.name, SymbolKind::Const, None);
            }
            Definition::ValueType(v) => {
                let Some(members) = &v.members else {
                    if !self.scopes[scope].symbols.contains_key(&v.name.text.to_lowercase()) {
                        let inner = self.push_scope(scope, &v.name.text);
                        self.declare_with(scope, &v.name, SymbolKind::ValueType, Some(inner), false);
                    }
                    return;
                };
                let inner = match self.completes_forward(scope, &v.name, SymbolKind::ValueType) {
                    Some(existing) => {
                        self.mark_defined(scope, &v.name);
                        existing
                    }
                    None => {
                        let inner = self.push_scope(scope, &v.name.text);
                        self.declare(scope, &v.name, SymbolKind::ValueType, Some(inner));
                        inner
                    }
                };
                if let Some(b) = &v.base {
                    self.reference(scope, b, true);
                }
                for s in &v.supports {
                    self.reference(scope, s, true);
                }
                for m in members {
                    match m {
                        ValueMember::State { member, .. } => self.collect_member(inner, member),
                        ValueMember::Other(o) => self.collect_interface_member(inner, o),
                    }
                }
            }
            Definition::Native(n) => self.declare(scope, n, SymbolKind::Native, None),
        }
    }

    fn collect_interface_member(&mut self, scope: usize, m: &InterfaceMember) {
        match m {
            InterfaceMember::Operation(op) => {
                // Return type and raises belong to the interface scope: the
                // oracle rejects `Position position()` and
                // `long position(); Position get();` alike.
                self.type_spec(scope, &op.returns);
                self.declare(scope, &op.name, SymbolKind::Operation, None);
                for r in &op.raises {
                    self.reference(scope, r, true);
                }

                // Parameters get a scope of their own. Establishing that took
                // three oracle queries, because the boundary is narrower than
                // it looks: `void v(in Token token)` is rejected while
                // `Token issue(in string token)` is accepted, so a parameter
                // collides with types named in *its own list* and not with the
                // return type or with anything another operation mentions.
                let params = self.push_scope(scope, "");
                for p in &op.params {
                    self.type_spec(params, &p.ty);
                    self.note_spelling(params, &p.name.text, p.name.span);
                }
            }
            InterfaceMember::Attribute(a) => {
                self.type_spec(scope, &a.ty);
                for n in &a.names {
                    self.declare(scope, n, SymbolKind::Attribute, None);
                }
            }
            InterfaceMember::Nested(d) => self.collect_definition(scope, d),
        }
    }

    fn collect_member(&mut self, scope: usize, m: &Member) {
        self.type_spec(scope, &m.ty);
        for n in &m.names {
            self.declare(scope, n, SymbolKind::Member, None);
        }
    }

    fn check_union_labels(&mut self, u: &UnionDef) {
        let mut seen: HashMap<String, Span> = HashMap::new();
        let mut default_at: Option<Span> = None;
        for c in &u.cases {
            if c.is_default {
                if let Some(prev) = default_at {
                    self.diagnostics.push(Diagnostic {
                        message: format!(
                            "union {:?} has more than one 'default' (the first is at line {})",
                            u.name.text, prev.line
                        ),
                        span: c.member.names.first().map_or(u.name.span, |n| n.span),
                        rule: "duplicate-union-default",
                    });
                } else {
                    default_at = Some(c.member.names.first().map_or(u.name.span, |n| n.span));
                }
            }
            for l in &c.labels {
                let key = label_key(l);
                let span = c.member.names.first().map_or(u.name.span, |n| n.span);
                if let Some(prev) = seen.get(&key) {
                    self.diagnostics.push(Diagnostic {
                        message: format!(
                            "union {:?} repeats the case label {key} (first used at line {}) — \
                             each label may select only one branch",
                            u.name.text, prev.line
                        ),
                        span,
                        rule: "duplicate-union-label",
                    });
                } else {
                    seen.insert(key, span);
                }
            }
        }
    }

    // ── references ──────────────────────────────────────────────────────────

    fn type_spec(&mut self, scope: usize, t: &TypeSpec) {
        match t {
            TypeSpec::Named(n) => self.reference(scope, n, true),
            TypeSpec::Sequence { element, bound } => {
                self.type_spec(scope, element);
                if let Some(b) = bound {
                    self.const_expr(scope, b);
                }
            }
            TypeSpec::String(Some(b)) | TypeSpec::WString(Some(b)) => self.const_expr(scope, b),
            TypeSpec::Fixed { digits, scale } => {
                self.const_expr(scope, digits);
                self.const_expr(scope, scale);
            }
            _ => {}
        }
    }

    fn const_expr(&mut self, scope: usize, e: &ConstExpr) {
        match e {
            ConstExpr::Name(n) => self.reference(scope, n, false),
            ConstExpr::Unary { operand, .. } => self.const_expr(scope, operand),
            ConstExpr::Binary { left, right, .. } => {
                self.const_expr(scope, left);
                self.const_expr(scope, right);
            }
            _ => {}
        }
    }

    /// Records a use of a name, deferring the lookup itself.
    fn reference(&mut self, scope: usize, n: &ScopedName, want_type: bool) {
        // Only the first component participates in this scope's spelling rule;
        // the rest are resolved inside whatever it names.
        if let Some(first) = n.parts.first() {
            self.note_spelling(scope, first, n.span);
        }
        self.deferred.push(Deferred { scope, name: n.clone(), want_type });
    }

    fn resolve_deferred(&mut self) {
        let pending = std::mem::take(&mut self.deferred);
        for d in pending {
            match self.lookup(d.scope, &d.name) {
                Some(sym) => {
                    // A reference that resolved to a differently-spelled symbol
                    // is the case clash we already reported. Emitting a second
                    // diagnostic from the same cause makes the self-repair loop
                    // chase the consequence instead of the cause.
                    if sym.name != *d.name.last() {
                        continue;
                    }
                    if d.want_type && !sym.kind.is_type() {
                        self.diagnostics.push(Diagnostic {
                            message: format!(
                                "{:?} is not a type — it names a {:?}",
                                d.name.text(),
                                sym.kind
                            ),
                            span: d.name.span,
                            rule: "not-a-type",
                        });
                    }
                }
                None => {
                    let hint = if d.name.parts.len() == 1
                        && matches!(d.name.last(), "TypeCode" | "Object" | "ValueBase")
                    {
                        format!(" — write '::CORBA::{}' to reach the predefined one", d.name.last())
                    } else {
                        String::new()
                    };
                    self.diagnostics.push(Diagnostic {
                        message: format!("{:?} is not declared{hint}", d.name.text()),
                        span: d.name.span,
                        rule: "unknown-name",
                    });
                }
            }
        }
    }

    /// Resolves a possibly-qualified name from `scope`.
    fn lookup(&self, scope: usize, n: &ScopedName) -> Option<Symbol> {
        let start = if n.absolute { 0 } else { scope };
        let first = n.parts.first()?;
        let mut sym = if n.absolute {
            self.lookup_in(0, first)?
        } else {
            self.lookup_upwards(start, first)?
        };
        for part in &n.parts[1..] {
            sym = self.lookup_in(sym.scope?, part)?;
        }
        Some(sym)
    }

    fn lookup_upwards(&self, scope: usize, name: &str) -> Option<Symbol> {
        let mut cur = Some(scope);
        while let Some(id) = cur {
            if let Some(s) = self.lookup_in(id, name) {
                return Some(s);
            }
            cur = self.scopes[id].parent;
        }
        None
    }

    /// Looks in one scope and the scopes it inherits from.
    fn lookup_in(&self, scope: usize, name: &str) -> Option<Symbol> {
        let key = name.to_lowercase();
        if let Some(s) = self.scopes[scope].symbols.get(&key) {
            return Some(s.clone());
        }
        for &base in &self.scopes[scope].inherited {
            if let Some(s) = self.lookup_in(base, name) {
                return Some(s);
            }
        }
        None
    }
}

/// A comparable form of a case label, so `1` and `0x1` collide.
fn label_key(e: &ConstExpr) -> String {
    match e {
        ConstExpr::Int(v) => format!("{v}"),
        ConstExpr::Char(c) => format!("'{c}'"),
        ConstExpr::Bool(b) => format!("{b}"),
        ConstExpr::Name(n) => n.last().to_owned(),
        ConstExpr::Unary { op, operand } => format!("{op}{}", label_key(operand)),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn diags(src: &str) -> Vec<Diagnostic> {
        analyse(&parse(src).expect("should parse")).diagnostics
    }

    fn clean(src: &str) {
        let d = diags(src);
        assert!(d.is_empty(), "expected no diagnostics, got: {d:?}");
    }

    #[test]
    fn a_plain_module_is_clean() {
        clean("module m { struct S { long a; long b; }; interface I { S get(); }; };");
    }

    /// Rule 1, and the single most expensive rule in the project.
    #[test]
    fn member_clashing_with_a_used_type_is_reported() {
        let d = diags("module m { struct Position { double x; }; struct T { Position position; }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "identifier-case-clash");
        assert!(d[0].message.contains("ignoring case"));
    }

    #[test]
    fn operation_name_clashing_with_a_type_is_reported() {
        let d = diags("module m { typedef sequence<octet> Blob; interface S { Blob blob(); }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "identifier-case-clash");
    }

    #[test]
    fn parameter_clashing_with_a_type_is_reported() {
        let d = diags("module m { struct Order { long id; }; interface P { void place(in Order order); }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "identifier-case-clash");
    }

    /// Rule 2, in both of the shapes it takes.
    #[test]
    fn declaration_matching_an_enclosing_scope_is_reported() {
        let d = diags("module inventory { interface Inventory { long n(); }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "enclosing-scope-clash");

        let d = diags("module m { struct Version { unsigned long version; }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "enclosing-scope-clash");
    }

    #[test]
    fn duplicate_members_are_reported() {
        let d = diags("module m { struct S { long a; long a; }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "duplicate-declaration");
    }

    #[test]
    fn unknown_types_are_reported() {
        let d = diags("module m { struct S { Widget w; }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "unknown-name");
    }

    /// `TypeCode` is not in the global scope, and the message says where it is.
    #[test]
    fn unqualified_typecode_is_reported_with_the_qualified_form() {
        let d = diags("module m { interface I { TypeCode describe(); }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "unknown-name");
        assert!(d[0].message.contains("::CORBA::TypeCode"), "{}", d[0].message);
        clean("module m { interface I { ::CORBA::TypeCode describe(); }; };");
    }

    #[test]
    fn duplicate_union_labels_are_reported() {
        let d = diags("module m { union U switch (long) { case 1: long a; case 1: long b; }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "duplicate-union-label");

        let d = diags(
            "module m { union U switch (long) { default: long a; default: long b; }; };",
        );
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "duplicate-union-default");
    }

    /// A type may be used before it is declared in the same scope, so
    /// resolution has to wait for the whole file.
    #[test]
    fn forward_use_within_a_scope_is_allowed() {
        clean("module m { interface I { S get(); }; struct S { long a; }; };");
        clean("module m { interface Node; typedef sequence<Node> Nodes; interface Node { Nodes kids(); }; };");
    }

    #[test]
    fn reopened_modules_share_a_scope() {
        clean("module m { struct A { long x; }; }; module m { interface I { A get(); }; };");
        let d = diags("module m { struct A { long x; }; }; module m { struct a { long y; }; };");
        assert_eq!(d.len(), 1, "reopening must not hide a clash: {d:?}");
    }

    /// Enumerators sit in the enclosing scope, not inside the enum, so two
    /// enums in one module cannot share a member name.
    #[test]
    fn enumerators_occupy_the_enclosing_scope() {
        let d = diags("module m { enum A { X, Y }; enum B { X, Z }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "duplicate-declaration");
        // Confirmed against the oracle: a member named `a` clashes with the
        // enum `A` used as its type, exactly as `Position position` does.
        let d = diags("module m { enum A { X, Y }; struct S { A a; }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "identifier-case-clash");
        clean("module m { enum A { X, Y }; struct S { A value; }; };");
    }

    #[test]
    fn inherited_names_are_visible_and_collide() {
        clean("module m { interface B { long f(); }; interface D : B { long g(); }; };");
        let d = diags("module m { interface B { long f(); }; interface D : B { long f(); }; };");
        assert_eq!(d.len(), 1, "an inherited operation must still collide: {d:?}");
        assert_eq!(d[0].rule, "inherited-clash");
    }

    /// The oracle reports this as an identifier clash rather than a type
    /// error, because `K` and `k` collide before the kind is ever considered.
    /// Reporting both would send the self-repair loop after the consequence.
    #[test]
    fn a_constant_used_as_a_type_reports_the_clash_the_oracle_reports() {
        let d = diags("module m { const long K = 1; struct S { K k; }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "identifier-case-clash");
    }

    #[test]
    fn a_non_type_used_as_a_type_is_reported() {
        let d = diags("module m { const long K = 1; struct S { K value; }; };");
        assert!(d.iter().any(|x| x.rule == "not-a-type"), "{d:?}");
    }

    #[test]
    fn diagnostics_are_ordered_by_position() {
        let d = diags(
            "module m { struct S { long a; long a; }; struct T { Widget w; }; };",
        );
        assert!(d.len() >= 2);
        assert!(d[0].span.line <= d[1].span.line);
        assert!(d[0].span.column < d[1].span.column || d[0].span.line < d[1].span.line);
    }
}
