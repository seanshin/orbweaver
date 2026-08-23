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
use crate::lex::{FixedLit, Span};

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
    /// The type this symbol names or was declared with, for the kinds that
    /// have one: a `Typedef`'s target, a `Const`'s declared type, and an
    /// `Enumerator`'s enum (as a [`TypeSpec::Named`]).
    ///
    /// Carried so that a constant's type can be followed through `typedef`s
    /// without a second symbol table. `typedef long double D; const D A = 1.0;`
    /// is the shape that needs it: the rule is about `long double`, and the
    /// declaration never says `long double`.
    pub ty: Option<TypeSpec>,
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
    /// Every declaration the v1 wire cannot carry (docs/PLAN.md §4.4), in
    /// source order. **Not** part of [`Analysis::is_ok`]: these files are
    /// valid IDL that the oracles accept, and this pass records what the wire
    /// will do with them, not whether they mean anything. See
    /// [`deferred_wire_types`].
    pub deferred_wire: Vec<DeferredWireUse>,
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
    let mut a =
        Analyser { scopes: vec![Scope::default()], diagnostics: Vec::new(), deferred: Vec::new() };
    a.install_corba_module();
    a.collect_definitions(0, &spec.definitions);
    a.resolve_deferred();
    a.diagnostics.sort_by_key(|d| (d.span.line, d.span.column));
    let deferred_wire = a.deferred_wire_types(spec);
    Analysis { scopes: a.scopes, diagnostics: a.diagnostics, deferred_wire }
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
        self.scopes.push(Scope { name: name.to_owned(), parent: Some(parent), ..Scope::default() });
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
                Symbol {
                    name: name.into(),
                    kind,
                    span: Span::empty(),
                    scope: None,
                    defined: true,
                    ty: None,
                },
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
                ty: None,
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
            Symbol {
                name: name.text.clone(),
                kind,
                span: name.span,
                scope: inner,
                defined,
                ty: None,
            },
        );
    }

    /// Declares a symbol and records the type it names or was declared with.
    ///
    /// Separate from [`Self::declare`] so that the dozen call sites with no
    /// type keep saying nothing about one, rather than passing `None` twelve
    /// times. Setting the field afterwards is safe because `declare` either
    /// inserted under this key or reported a clash and inserted nothing — in
    /// the second case there is an older symbol here whose type is its own,
    /// and overwriting it would attribute the loser's type to the winner.
    fn declare_typed(&mut self, scope: usize, name: &Named, kind: SymbolKind, ty: TypeSpec) {
        let before = self.scopes[scope].symbols.contains_key(&name.text.to_lowercase());
        self.declare(scope, name, kind, None);
        if before {
            return;
        }
        if let Some(sym) = self.scopes[scope].symbols.get_mut(&name.text.to_lowercase()) {
            sym.ty = Some(ty);
        }
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
                        self.declare_with(
                            scope,
                            &i.name,
                            SymbolKind::Interface,
                            Some(inner),
                            false,
                        );
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
                // Each carries the enum it belongs to, which is how
                // `const E A = Y;` is told from `const E A = X;` when `Y`
                // belongs to some other enum in the same scope.
                let owner = TypeSpec::Named(ScopedName {
                    absolute: false,
                    parts: vec![e.name.text.clone()],
                    span: e.name.span,
                });
                for m in &e.members {
                    self.declare_typed(scope, m, SymbolKind::Enumerator, owner.clone());
                }
            }
            Definition::Typedef(t) => {
                self.type_spec(scope, &t.ty);
                self.declare_typed(scope, &t.name, SymbolKind::Typedef, t.ty.clone());
            }
            Definition::Const(c) => {
                self.type_spec(scope, &c.ty);
                self.const_expr(scope, &c.value);
                // Checked before the name is declared: a constant cannot refer
                // to itself, and declaring first would let `const long A = A;`
                // resolve to the declaration being checked.
                self.check_const(scope, c);
                self.declare_typed(scope, &c.name, SymbolKind::Const, c.ty.clone());
            }
            Definition::ValueType(v) => {
                let Some(members) = &v.members else {
                    if !self.scopes[scope].symbols.contains_key(&v.name.text.to_lowercase()) {
                        let inner = self.push_scope(scope, &v.name.text);
                        self.declare_with(
                            scope,
                            &v.name,
                            SymbolKind::ValueType,
                            Some(inner),
                            false,
                        );
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

    // ── a constant's value against its declared type ────────────────────────

    /// Follows a constant's declared type through `typedef`s to the type the
    /// rules are written about. `None` when it does not resolve, or when the
    /// chain is longer than any real one — a `typedef` cycle is a different
    /// diagnostic and must not become a hang here.
    fn base_type(&self, scope: usize, ty: &TypeSpec) -> Option<TypeSpec> {
        let mut cur = ty.clone();
        for _ in 0..32 {
            let TypeSpec::Named(n) = &cur else { return Some(cur) };
            let sym = self.lookup(scope, n)?;
            match sym.kind {
                SymbolKind::Typedef => cur = sym.ty.clone()?,
                // An enum type is a base type: it is what an enumerator fits.
                _ => return Some(cur),
            }
        }
        None
    }

    /// Evaluates a constant expression far enough to type- and range-check it.
    ///
    /// # Why the front end folds at all
    ///
    /// `orbweaver_registry` folds constants too, and its own documentation
    /// argues that folding should happen once. That argument is about
    /// *consumers*: three emitters with three folders will disagree and the
    /// silent one ships. A front end is not a consumer — `const short S =
    /// 40000;` is an error in the language, and an error in the language is
    /// reported by the thing that reads the language. omniidl folds here for
    /// the same reason, and every message this check emits is one of its
    /// messages.
    ///
    /// The two folders are held together by a test rather than by hope:
    /// `registry_agrees_with_the_front_end` walks `corpus/golden/` and asserts
    /// that every constant this pass accepts is one the registry gives a value
    /// to, and every constant it rejects is one the registry leaves `None`.
    ///
    /// `None` here means *"not evaluated"*, never *"invalid"*: an unresolved
    /// name, an operator this does not implement, an overflow. Nothing is
    /// reported from a `None`, because a value that did not fold is not a value
    /// that is wrong.
    fn fold(&self, scope: usize, e: &ConstExpr) -> Option<ConstFold> {
        Some(match e {
            ConstExpr::Int(v) => ConstFold::Int(*v),
            ConstExpr::Float(v) => ConstFold::Float(*v),
            ConstExpr::Fixed(v) => ConstFold::Fixed(*v),
            ConstExpr::Str(s) => ConstFold::Str(s.clone()),
            ConstExpr::WStr(s) => ConstFold::WStr(s.clone()),
            ConstExpr::Char(c) => ConstFold::Char(*c),
            ConstExpr::WChar(c) => ConstFold::WChar(*c),
            ConstExpr::Bool(b) => ConstFold::Bool(*b),
            ConstExpr::Name(n) => {
                let sym = self.lookup(scope, n)?;
                match sym.kind {
                    // An enumerator's value is the enum it belongs to: which
                    // enum is the whole question, and the ordinal is not.
                    SymbolKind::Enumerator => {
                        let TypeSpec::Named(owner) = sym.ty.as_ref()? else { return None };
                        ConstFold::Enum(owner.last().to_lowercase())
                    }
                    // Another constant contributes its *class*, taken from the
                    // type it was declared with. That is enough for every rule
                    // here except the integer range, which is why an integer
                    // named rather than written is deliberately not folded.
                    SymbolKind::Const => {
                        let base = self.base_type(scope, sym.ty.as_ref()?)?;
                        ConstFold::from_type(&base)?
                    }
                    _ => return None,
                }
            }
            ConstExpr::Unary { op, operand } => match (*op, self.fold(scope, operand)?) {
                ("+", v) => v,
                ("-", ConstFold::Int(v)) => ConstFold::Int(v.checked_neg()?),
                ("-", ConstFold::Float(v)) => ConstFold::Float(-v),
                ("-", ConstFold::Fixed(v)) => ConstFold::Fixed(v),
                ("~", ConstFold::Int(v)) => ConstFold::Int(!v),
                _ => return None,
            },
            ConstExpr::Binary { op, left, right } => {
                match (self.fold(scope, left)?, self.fold(scope, right)?) {
                    (ConstFold::Int(a), ConstFold::Int(b)) => ConstFold::Int(match *op {
                        "+" => a.checked_add(b)?,
                        "-" => a.checked_sub(b)?,
                        "*" => a.checked_mul(b)?,
                        "/" => a.checked_div(b)?,
                        "%" => a.checked_rem(b)?,
                        "|" => a | b,
                        "^" => a ^ b,
                        "&" => a & b,
                        "<<" => a.checked_shl(u32::try_from(b).ok()?)?,
                        ">>" => a.checked_shr(u32::try_from(b).ok()?)?,
                        _ => return None,
                    }),
                    // Arithmetic that mixes classes, or that this does not
                    // implement exactly (a `fixed` sum needs decimal
                    // arithmetic), stays unevaluated. The class of the result
                    // is still the class of the operands, which is what the
                    // type check needs.
                    (a, b) if a.class() == b.class() => a,
                    _ => return None,
                }
            }
        })
    }

    /// Whether any `/` or `%` in the expression has a divisor that folds to
    /// zero, anywhere in the tree.
    fn divides_by_zero(&self, scope: usize, e: &ConstExpr) -> bool {
        match e {
            ConstExpr::Unary { operand, .. } => self.divides_by_zero(scope, operand),
            ConstExpr::Binary { op, left, right } => {
                if matches!(*op, "/" | "%")
                    && matches!(self.fold(scope, right), Some(ConstFold::Int(0)))
                {
                    return true;
                }
                self.divides_by_zero(scope, left) || self.divides_by_zero(scope, right)
            }
            _ => false,
        }
    }

    /// Reports a constant whose value does not fit the type it was declared
    /// with. Every message here is omniidl 4.3.4's, measured 2026-08-21.
    ///
    /// # The shape of the bug this closes
    ///
    /// There was no check at all. `orbweaver_registry::coerce` had the range
    /// half of it — and its own doc comment said the out-of-range case "is an
    /// IDL error the checker reports" — but the checker did not report it, so
    /// the rule's only effect was that the registry silently stored no value.
    /// A constant with no value is one an emitter skips, so `const octet O =
    /// 300;` validated clean, emitted nothing, and said nothing about either.
    /// *규칙이 레지스트리 안에 조용한 `None`으로만 있었고 진단은 없었다.*
    ///
    /// Sixteen shapes were measured diverging from the oracle in the lax
    /// direction across `const` and its neighbours; all sixteen are this.
    fn check_const(&mut self, scope: usize, c: &ConstDef) {
        let Some(base) = self.base_type(scope, &c.ty) else { return };
        let span = c.name.span;

        // `long double` is legal by `const_type`'s grammar — `floating_pt_type`
        // admits it — and refused semantically, so it is checked before the
        // value is looked at at all. It reaches here as `const long double`,
        // and equally as a `typedef` nobody reading the declaration would see.
        if base == TypeSpec::LongDouble {
            self.diagnostics.push(Diagnostic {
                message: "'long double' cannot be a constant's type: `const_type` admits it \
                          through `floating_pt_type` and the oracle refuses it anyway \
                          (\"Invalid type for constant: long double\"), because there is no \
                          `long double` literal to write. Use `double`."
                    .to_owned(),
                span,
                rule: "not-a-const-type",
            });
            return;
        }

        // Reported before the fold rather than inferred from its failure: a
        // fold returns `None` for a dozen reasons and "did not evaluate" is
        // not a diagnosis. This one has an answer — there is no value — so it
        // is found by looking for it.
        if self.divides_by_zero(scope, &c.value) {
            self.diagnostics.push(Diagnostic {
                message: format!(
                    "{:?} divides by zero: the expression has no value, so the constant has \
                     none either. Change the divisor.",
                    c.name.text
                ),
                span,
                rule: "const-value-range",
            });
            return;
        }

        let Some(want) = ConstFold::from_type(&base) else { return };
        let Some(got) = self.fold(scope, &c.value) else { return };

        if got.class() != want.class() {
            self.diagnostics.push(Diagnostic {
                message: format!(
                    "cannot interpret {} as {}: a constant's value must be a literal of its own \
                     type — IDL converts nothing here, so `{}` needs {}",
                    got.class().literal_name(),
                    want.class().article_name(),
                    c.name.text,
                    want.class().how_to_write(),
                ),
                span,
                rule: "const-value-type",
            });
            return;
        }

        match (&got, &base) {
            (ConstFold::Int(v), _) => {
                if let Some((lo, hi, name)) = int_range(&base) {
                    if *v < lo || *v > hi {
                        let side = if *v < lo { "small" } else { "large" };
                        self.diagnostics.push(Diagnostic {
                            message: format!(
                                "value {v} is too {side} for {name}: the range is {lo}..={hi}. \
                                 Widen the type, or write a value inside it — a constant is \
                                 part of the contract, and truncating one here would hand \
                                 every consumer a number nobody wrote."
                            ),
                            span,
                            rule: "const-value-range",
                        });
                    }
                }
            }
            (ConstFold::Enum(from), TypeSpec::Named(want_enum))
                if *from != want_enum.last().to_lowercase() =>
            {
                self.diagnostics.push(Diagnostic {
                    message: format!(
                        "enumerator does not belong to enum {:?}: an enumerator initialises a \
                         constant only of its own enum. It belongs to {:?}.",
                        want_enum.last(),
                        from
                    ),
                    span,
                    rule: "const-value-type",
                });
            }
            (ConstFold::Str(s), TypeSpec::String(Some(b)))
            | (ConstFold::WStr(s), TypeSpec::WString(Some(b))) => {
                if let Some(ConstFold::Int(bound)) = self.fold(scope, b) {
                    let len = i128::try_from(s.chars().count()).unwrap_or(i128::MAX);
                    if len > bound {
                        self.diagnostics.push(Diagnostic {
                            message: format!(
                                "string constant is {len} characters and the bound is {bound}: \
                                 a bounded string constant may not exceed its own bound. \
                                 Widen the bound, or shorten the value."
                            ),
                            span,
                            rule: "const-value-range",
                        });
                    }
                }
            }
            _ => {}
        }
    }

    fn check_union_labels(&mut self, u: &UnionDef) {
        let mut seen: HashMap<String, Span> = HashMap::new();
        let mut default_at: Option<Span> = None;
        for c in &u.cases {
            if c.is_default() {
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
            TypeSpec::Fixed { bounds: Some((digits, scale)) } => {
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
                    let (rule, hint) = self.unknown_name_advice(d.scope, &d.name);
                    self.diagnostics.push(Diagnostic {
                        message: format!("{:?} is not declared{hint}", d.name.text()),
                        span: d.name.span,
                        rule,
                    });
                }
            }
        }
    }

    /// The rule to file an unresolved name under, and what to say about it.
    ///
    /// **A qualified name gets its own rule.** The generic advice for an
    /// unknown name is *"declare it, or qualify it with its module"*, which is
    /// right for `AuditStamp` and meaningless for `::MFS::Common::StringList` —
    /// it is already qualified, and the consumer that renders that advice
    /// (`orbweaver-forge`) has no way to tell the two apart from the rule name
    /// alone. It printed *"qualify it with `Module::::`"* ~90 times over the
    /// estate (`docs/pipeline-runs/2026-08-14-estate.md`, RC-2). The wrong
    /// *text* came from a wrong span, fixed in [`crate::parse`]; the wrong
    /// *advice* is this: two different diagnoses were sharing one rule, so
    /// they now do not, and the advice for the qualified case is written here
    /// where the analyser knows which component actually failed.
    fn unknown_name_advice(&self, scope: usize, n: &ScopedName) -> (&'static str, String) {
        // The most common cause of a name that resolves nowhere, once a unit
        // can span files at all, is that the file declaring it was never
        // included. Phrased as a condition, because it is not the only cause.
        const ELSEWHERE: &str = "if it is declared in another file, this translation unit has no \
                                 `#include` that reaches that file";
        if n.parts.len() == 1 && !n.absolute {
            if matches!(n.last(), "TypeCode" | "Object" | "ValueBase") {
                return (
                    "unknown-name",
                    format!(" — write '::CORBA::{}' to reach the predefined one", n.last()),
                );
            }
            return ("unknown-name", format!(" — {ELSEWHERE}"));
        }
        let (failed, container) = self.first_unresolved_part(scope, n);
        let where_it_looked = match container {
            Some(path) if !path.is_empty() => format!("is not declared in {path:?}"),
            Some(_) => "is not declared at global scope".to_owned(),
            None => "is not in scope here".to_owned(),
        };
        ("unknown-scoped-name", format!(" — {:?} {where_it_looked}; {ELSEWHERE}", n.parts[failed]))
    }

    /// Which component of a scoped name stopped resolving, and the qualified
    /// name of the scope it was looked for in (`None` when the first component
    /// was searched outwards from here rather than inside anything).
    fn first_unresolved_part(&self, scope: usize, n: &ScopedName) -> (usize, Option<String>) {
        let Some(first) = n.parts.first() else { return (0, None) };
        let mut sym = if n.absolute {
            match self.lookup_in(0, first) {
                Some(s) => s,
                None => return (0, Some(String::new())),
            }
        } else {
            match self.lookup_upwards(scope, first) {
                Some(s) => s,
                None => return (0, None),
            }
        };
        for (i, part) in n.parts.iter().enumerate().skip(1) {
            let container = self.scope_path(sym.scope);
            match sym.scope.and_then(|sc| self.lookup_in(sc, part)) {
                Some(next) => sym = next,
                None => return (i, Some(container)),
            }
        }
        (n.parts.len() - 1, Some(self.scope_path(sym.scope)))
    }

    /// The `A::B` path of a scope, for a message that says where it looked.
    fn scope_path(&self, scope: Option<usize>) -> String {
        let mut parts = Vec::new();
        let mut cur = scope;
        while let Some(id) = cur {
            if self.scopes[id].name.is_empty() {
                break;
            }
            parts.push(self.scopes[id].name.clone());
            cur = self.scopes[id].parent;
        }
        parts.reverse();
        parts.join("::")
    }

    /// Resolves a possibly-qualified name from `scope`.
    fn lookup(&self, scope: usize, n: &ScopedName) -> Option<Symbol> {
        let start = if n.absolute { 0 } else { scope };
        let first = n.parts.first()?;
        let mut sym =
            if n.absolute { self.lookup_in(0, first)? } else { self.lookup_upwards(start, first)? };
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

// ── what the wire cannot carry (docs/PLAN.md §4.4, and one thing it omits) ───
//
// **This rule's set is four families, and §4.4 names three.** The fourth is
// `native X;`, and the sentence that separates it from the other three is the
// whole reason it is worth writing down: §4.4 *defers* `valuetype`, abstract
// interfaces and `fixed` — each has a wire form the specification defines and
// this version does not implement — while a `native` has no wire form to
// implement, in v1 or in any later version. A native names a type only the
// language mapping knows; there is nothing to marshal.
//
// It was missing here for exactly that reason, inverted: because §4.4 did not
// name it, this rule did not name it, so the generator could not refuse it
// without breaking `deferred_wire_agreement`, so the registry recorded a
// native as `TypeCode::ObjRef` and both emitters emitted an object reference
// — an IOR on the wire where nothing at all should go. The previous batch
// fixed the same wrong answer for the other two and left this one with an
// honest note: *"no rule names it, so a change here would be a claim no gate
// checks."* The fix for that is a rule, which is this paragraph.
//
// The rule's *name* is `wire/deferred-type` and stays that way: renaming it
// would break every consumer and the harness pins, for a word. The name is
// imprecise for a native and the message is not — [`DeferredWireUse::message`]
// says "cannot go on the wire at all" and says why §4.4 does not apply.
//
// *§4.4는 셋을 미룬다. 이 규칙의 집합은 넷이다 — `native`는 미뤄진 것이 아니라
// 애초에 마샬링될 수 없는 것이고, 그 문장이 차이의 전부다.*
//
// The parser accepts `valuetype`, abstract interfaces and `fixed` because a
// conformant front end has to, and `corpus/golden/20` and `21` pin that it
// does. The wire does not carry them, by decision. Between those two facts
// there was, until this pass, nothing: a contract using them checked out
// here, passed S4, and was unservable — the refusal lived in the *generator*
// (`orbweaver-gen` skips such items with the section named), which a caller
// of S4 never sees. This pass is the same closure the generator computes,
// computed at the front end so S4 can say it, and `orbweaver-gen`'s
// `deferred_wire_agreement` test holds the two to the same set.
//
// **A separate list, not a diagnostic.** [`Analysis::is_ok`] is agreement
// with the oracle, and the oracle accepts these files. Filing them under
// `diagnostics` would make `idl-check` disagree with `omniidl` over golden
// files, which is the one thing this crate's contract forbids. Whether a
// consumer treats an entry as a warning or a refusal is that consumer's
// decision (S4 has both forms); this crate only establishes the set.

/// The rule name every consumer files these under.
///
/// `wire/`, not `sidl/`: S4's existing prefix for exactly this class was
/// `wire/valuetype`, and the SIDL prefix names the annotation vocabulary,
/// which this is not about.
pub const DEFERRED_WIRE_RULE: &str = "wire/deferred-type";

/// One declaration the v1 wire cannot carry, and why.
///
/// A declaration is here either because it *is* one of the deferred
/// constructs, or because a value of it would carry one: a struct with a
/// `fixed` member, an interface whose operation returns such a struct, an
/// interface inheriting that operation. The closure follows values, not
/// references — a member typed as a plain `interface` is an object reference
/// on the wire whatever that interface's operations take, so it does not
/// propagate. A `valuetype` or `abstract interface` is passed by value (or by
/// a value/reference union), so those do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredWireUse {
    /// The declaration's qualified IDL name, `gc21::Invoice` — the same
    /// spelling the registry gives it, so a finding here can be matched to a
    /// repository id without a second resolution.
    pub declaration: String,
    /// What kind of declaration it is: `struct`, `interface`, `typedef`, …
    pub kind: &'static str,
    /// The construct at the root of the reason, as written: `fixed<9,2>`,
    /// `valuetype`, `abstract valuetype`, `abstract interface`, `ValueBase`.
    pub construct: String,
    /// The path from the declaration to the construct, in prose, starting
    /// after the declaration's name: *"member "total" is "gc21::Amount", which
    /// is fixed<9,2>"*. For a declaration that is the construct itself, *"it
    /// is a valuetype"*.
    pub reason: String,
    /// Where the declaration's name is written.
    pub span: Span,
}

impl DeferredWireUse {
    /// The finding as prose, in the shape S4's other rules take.
    ///
    /// Two sentences, because the set is two things: three constructs §4.4
    /// *defers*, and one — `native` — it does not mention because there is
    /// nothing to defer. Saying "§4.4 defers natives" would be false, and a
    /// gate that says a false thing gets bypassed.
    pub fn message(&self) -> String {
        if self.family() == "natives" {
            return format!(
                "{} {:?} cannot go on the wire at all: {}, which names a type only a language \
                 mapping knows and has no CDR encoding in any version — so this is not one of \
                 docs/PLAN.md §4.4's three deferrals, there is nothing here to defer; the \
                 generator skips it and the dynamic path cannot marshal it",
                self.kind, self.declaration, self.reason,
            );
        }
        format!(
            "{} {:?} cannot go on the v1 wire: {} — docs/PLAN.md §4.4 defers {}; the \
             generator skips it and the dynamic path cannot marshal it",
            self.kind,
            self.declaration,
            self.reason,
            self.family()
        )
    }

    /// The concrete edit, per construct family.
    pub fn fix(&self) -> String {
        match self.family() {
            "natives" => "declare the type in IDL — a struct, a typedef, or an `interface` if \
                          it is an object — so the contract says what crosses; a `native` \
                          names a type only the language mapping knows, and no wire version \
                          will carry one (omniORB's C++ back end refuses the declaration \
                          outright)"
                .into(),
            "fixed" => "carry the amount as a string, or as scaled integers (`long long` \
                        units plus a scale the contract documents), until §4.4 lands `fixed`; \
                        AnyJSON already carries decimals as strings"
                .into(),
            "abstract interfaces" => "declare it a plain `interface` (a reference on the wire) \
                                       or a `valuetype`, whichever the design meant; v1 \
                                       cannot carry the value-or-reference union an abstract \
                                       interface is"
                .into(),
            _ => "model the state as a struct and pass it by value, or keep the valuetype out \
                  of every operation signature until §4.4 lands valuetypes"
                .into(),
        }
    }

    /// The family the construct belongs to, for the message and the fix.
    ///
    /// Three of the four are §4.4's; `"natives"` is not, and
    /// [`DeferredWireUse::message`] branches on exactly this answer so the
    /// difference is said rather than glossed.
    pub fn family(&self) -> &'static str {
        if self.construct.starts_with("fixed") {
            "fixed"
        } else if self.construct == "abstract interface" {
            "abstract interfaces"
        } else if self.construct == "native" {
            "natives"
        } else {
            "valuetypes"
        }
    }

    /// The same, as a [`Diagnostic`] under [`DEFERRED_WIRE_RULE`].
    pub fn diagnostic(&self) -> Diagnostic {
        Diagnostic { message: self.message(), span: self.span, rule: DEFERRED_WIRE_RULE }
    }
}

/// Every declaration in `spec` the v1 wire cannot carry (docs/PLAN.md §4.4).
///
/// The convenience form of [`Analysis::deferred_wire`] for a caller holding a
/// checked [`Spec`]; the analysis is cheap enough to run twice.
pub fn deferred_wire_types(spec: &Spec) -> Vec<DeferredWireUse> {
    analyse(spec).deferred_wire
}

/// Why a declaration is deferred, before the prose is built.
#[derive(Debug, Clone)]
enum Cause {
    /// The declaration carries the construct itself. `site` is where inside
    /// it (`member "total"`), or `None` when the declaration *is* it.
    Direct { site: Option<String>, construct: String },
    /// The declaration carries `target`, which is deferred.
    Through { site: String, target: String },
}

/// One declaration as this pass sees it: what it is, what it directly is,
/// and what it refers to.
#[derive(Debug)]
struct WireDecl {
    qualified: String,
    kind: &'static str,
    span: Span,
    /// The construct this declaration is or contains, with the site.
    direct: Option<(Option<String>, String)>,
    /// `(site, qualified name of the type referred to, by value)`, in source
    /// order. A reference to an interface is *not* by value — an object
    /// reference is an IOR on the wire whatever the interface's operations
    /// take — so it propagates only if that interface is a §4.4 construct
    /// itself (abstract). Inheritance is by value: the derived interface has
    /// the base's operations.
    refs: Vec<(String, String, bool)>,
}

impl Analyser {
    fn deferred_wire_types(&self, spec: &Spec) -> Vec<DeferredWireUse> {
        let mut decls: Vec<WireDecl> = Vec::new();
        self.wire_definitions(0, &[], &spec.definitions, &mut decls);

        // The closure, to a fixpoint. Source order within a pass, so the
        // reason a declaration is given is the first of its references that
        // was already known to be deferred — deterministic, and usually the
        // shortest chain.
        let mut cause: HashMap<String, Cause> = HashMap::new();
        for d in &decls {
            if let Some((site, construct)) = &d.direct {
                cause
                    .entry(d.qualified.clone())
                    .or_insert(Cause::Direct { site: site.clone(), construct: construct.clone() });
            }
        }
        loop {
            let mut changed = false;
            for d in &decls {
                if cause.contains_key(&d.qualified) {
                    continue;
                }
                let reached = d.refs.iter().find(|(_, target, by_value)| match cause.get(target) {
                    None => false,
                    Some(_) if *by_value => true,
                    // A reference: only an interface that *is* a §4.4
                    // construct — abstract — travels as anything but an IOR.
                    Some(Cause::Direct { site: None, construct }) => {
                        construct == "abstract interface"
                    }
                    Some(_) => false,
                });
                if let Some((site, target, _)) = reached {
                    cause.insert(
                        d.qualified.clone(),
                        Cause::Through { site: site.clone(), target: target.clone() },
                    );
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        let mut out = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for d in &decls {
            let Some(c) = cause.get(&d.qualified) else { continue };
            // A forward declaration and its definition are one declaration.
            if seen.contains(&d.qualified.as_str()) {
                continue;
            }
            seen.push(&d.qualified);
            out.push(DeferredWireUse {
                declaration: d.qualified.clone(),
                kind: d.kind,
                construct: root_construct(&cause, c),
                reason: reason_prose(&cause, c, true),
                span: d.span,
            });
        }
        out
    }

    /// Mirrors [`Analyser::collect_definitions`] over the same scopes, so a
    /// name written here resolves exactly as it did there.
    fn wire_definitions(
        &self,
        scope: usize,
        path: &[String],
        defs: &[Definition],
        out: &mut Vec<WireDecl>,
    ) {
        for d in defs {
            self.wire_definition(scope, path, d, out);
        }
    }

    fn wire_definition(
        &self,
        scope: usize,
        path: &[String],
        d: &Definition,
        out: &mut Vec<WireDecl>,
    ) {
        let name = d.name();
        let mut here = path.to_vec();
        here.push(name.text.clone());
        let qualified = here.join("::");
        // The scope this declaration introduced during collection, if it
        // introduced one; absent only when the file did not check out, in
        // which case the caller has better things to report than this.
        let inner = self.scopes[scope]
            .symbols
            .get(&name.text.to_lowercase())
            .and_then(|s| s.scope)
            .unwrap_or(scope);
        let mut decl =
            WireDecl { qualified, kind: "", span: name.span, direct: None, refs: Vec::new() };
        match d {
            Definition::Module(m) => {
                self.wire_definitions(inner, &here, &m.definitions, out);
                return;
            }
            Definition::Interface(i) => {
                decl.kind = "interface";
                if i.modifier == Some(InterfaceModifier::Abstract) {
                    decl.direct = Some((None, "abstract interface".into()));
                }
                for b in &i.bases {
                    if let Some((target, _)) = self.wire_target(scope, b) {
                        decl.refs.push((format!("base {:?}", b.text()), target, true));
                    }
                }
                let Some(body) = &i.body else {
                    out.push(decl);
                    return;
                };
                for m in body {
                    match m {
                        InterfaceMember::Operation(op) => {
                            self.wire_type(
                                inner,
                                &format!("the return of operation {:?}", op.name.text),
                                &op.returns,
                                &mut decl,
                            );
                            for p in &op.params {
                                self.wire_type(
                                    inner,
                                    &format!(
                                        "parameter {:?} of operation {:?}",
                                        p.name.text, op.name.text
                                    ),
                                    &p.ty,
                                    &mut decl,
                                );
                            }
                            for r in &op.raises {
                                if let Some((target, by_value)) = self.wire_target(inner, r) {
                                    decl.refs.push((
                                        format!(
                                            "the exception operation {:?} raises",
                                            op.name.text
                                        ),
                                        target,
                                        by_value,
                                    ));
                                }
                            }
                        }
                        InterfaceMember::Attribute(a) => {
                            for n in &a.names {
                                self.wire_type(
                                    inner,
                                    &format!("attribute {:?}", n.text),
                                    &a.ty,
                                    &mut decl,
                                );
                            }
                        }
                        InterfaceMember::Nested(n) => self.wire_definition(inner, &here, n, out),
                    }
                }
            }
            Definition::Struct(s) | Definition::Exception(s) => {
                decl.kind = if matches!(d, Definition::Struct(_)) { "struct" } else { "exception" };
                for m in s.members.iter().flatten() {
                    for n in &m.names {
                        self.wire_type(inner, &format!("member {:?}", n.text), &m.ty, &mut decl);
                    }
                }
            }
            Definition::Union(u) => {
                decl.kind = "union";
                self.wire_type(scope, "the discriminator", &u.discriminator, &mut decl);
                for c in &u.cases {
                    for n in &c.member.names {
                        self.wire_type(
                            inner,
                            &format!("case {:?}", n.text),
                            &c.member.ty,
                            &mut decl,
                        );
                    }
                }
            }
            Definition::Typedef(t) => {
                decl.kind = "typedef";
                self.wire_type(scope, "", &t.ty, &mut decl);
            }
            // A constant is **not** in this rule's closure, and this arm used
            // to put it there.
            //
            // The rule answers one question: can a v1 peer be served this
            // contract? A constant is never marshalled — no operation carries
            // one, no TypeCode of one is ever encoded, no peer ever sees it —
            // so `const fixed TAX = 0.08d;` beside operations that all take
            // `double` costs the wire nothing, and refusing the whole file
            // under `--wire v1` for it would be a false refusal that blocks
            // work which would have succeeded. The message it produced said so
            // in as many words and was simply untrue: *const "LIMIT" cannot go
            // on the v1 wire*.
            //
            // What such a constant does cost is one generated binding: both
            // emitters skip it, because the registry has no `ConstValue` for a
            // decimal. That is reported where it happens, in
            // `Generated::skipped`, and its wording is imprecise — see the
            // commit that removed this arm.
            //
            // 상수는 마샬링되지 않으므로 §4.4 폐쇄집합 밖이다. 와이어에 오르지
            // 않는 선언 때문에 파일 전체를 거부하는 것은 거짓 거부다.
            Definition::Const(_) => return,
            Definition::ValueType(v) => {
                decl.kind = "valuetype";
                let construct = if v.is_abstract { "abstract valuetype" } else { "valuetype" };
                decl.direct = Some((None, construct.into()));
                // Its members are not walked: the valuetype is the reason, and
                // whatever it carries is carried by a construct v1 has not met.
            }
            // `native X;` — the fourth family, and the one §4.4 omits. See
            // this section's header for why the omission is the defect rather
            // than the justification.
            //
            // Only a native *written in the contract* reaches here. The
            // predeclared `::CORBA::TypeCode` and `::CORBA::Principal` are
            // `SymbolKind::Native` too, but they are not `Definition`s, so
            // they never become a `WireDecl` and never acquire a cause — a
            // struct member typed `::CORBA::TypeCode` is `tk_TypeCode`, which
            // marshals perfectly well, and flagging it would be a false
            // refusal. Asserted, not left to the shape of the code.
            Definition::Native(_) => {
                decl.kind = "native";
                decl.direct = Some((None, "native".into()));
            }
            Definition::Enum(_) => return,
        }
        out.push(decl);
    }

    /// Records what `t` is or refers to, at `site`, on `decl`.
    fn wire_type(&self, scope: usize, site: &str, t: &TypeSpec, decl: &mut WireDecl) {
        // An empty site means the declaration *is* the type: a typedef.
        let at = || (!site.is_empty()).then(|| site.to_owned());
        // The first direct construct is the one named; a second `fixed`
        // member adds nothing a reader needs.
        let first = decl.direct.is_none();
        match t {
            TypeSpec::Fixed { bounds } if first => {
                let construct = match bounds {
                    Some((digits, scale)) => {
                        format!("fixed<{},{}>", const_text(digits), const_text(scale))
                    }
                    // Only a constant's type writes bare `fixed`, and a
                    // constant is not a wire declaration (see the
                    // `Definition::Const` arm above) — so this is unreachable
                    // through `deferred_wire_types` today and spelled as the
                    // source spells it rather than invented, in case a later
                    // caller does reach it.
                    None => "fixed".to_owned(),
                };
                decl.direct = Some((at(), construct));
            }
            TypeSpec::ValueBase if first => decl.direct = Some((at(), "ValueBase".into())),
            TypeSpec::Sequence { element, .. } => self.wire_type(scope, site, element, decl),
            TypeSpec::Named(n) => {
                if let Some((target, by_value)) = self.wire_target(scope, n) {
                    decl.refs.push((site.to_owned(), target, by_value));
                }
            }
            _ => {}
        }
    }

    /// The qualified name of what `n` refers to, if it is something a value
    /// carries — a struct, union, exception, typedef, valuetype or interface
    /// — and whether the reference is by value (everything but an interface).
    ///
    /// Enumerators, constants and enums are not values that carry anything and
    /// resolve to `None`. `::CORBA::ValueBase`, predeclared as a valuetype,
    /// resolves like any other; a use of it is a §4.4 construct in its own
    /// right (the keyword spelling is [`TypeSpec::ValueBase`]).
    ///
    /// A **native** is by value, and used not to resolve at all — which is how
    /// `struct Session { Handle token; }` was reported servable while its
    /// member had no wire form. Whether it propagates is still decided by the
    /// cause table rather than here: the predeclared `::CORBA::TypeCode` is a
    /// `SymbolKind::Native` with no declaration behind it, so it is named here
    /// and never found in the table, which is correct — it is `tk_TypeCode` on
    /// the wire.
    fn wire_target(&self, scope: usize, n: &ScopedName) -> Option<(String, bool)> {
        let (container, sym) = self.find(scope, n)?;
        let by_value = match sym.kind {
            SymbolKind::Struct
            | SymbolKind::Union
            | SymbolKind::Exception
            | SymbolKind::Typedef
            | SymbolKind::ValueType
            | SymbolKind::Native => true,
            SymbolKind::Interface => false,
            _ => return None,
        };
        let path = self.scope_path(Some(container));
        let qualified = if path.is_empty() { sym.name } else { format!("{path}::{}", sym.name) };
        Some((qualified, by_value))
    }

    /// [`Analyser::lookup`], also returning the scope the symbol was found in
    /// — which is what makes its qualified name computable.
    fn find(&self, scope: usize, n: &ScopedName) -> Option<(usize, Symbol)> {
        let first = n.parts.first()?;
        let (mut container, mut sym) = if n.absolute {
            self.find_in(0, first)?
        } else {
            let mut cur = Some(scope);
            loop {
                let id = cur?;
                if let Some(found) = self.find_in(id, first) {
                    break found;
                }
                cur = self.scopes[id].parent;
            }
        };
        for part in &n.parts[1..] {
            (container, sym) = self.find_in(sym.scope?, part)?;
        }
        Some((container, sym))
    }

    /// [`Analyser::lookup_in`], returning the scope that actually holds the
    /// symbol — an inherited name's home is the base, and its qualified name
    /// says so.
    fn find_in(&self, scope: usize, name: &str) -> Option<(usize, Symbol)> {
        let key = name.to_lowercase();
        if let Some(s) = self.scopes[scope].symbols.get(&key) {
            return Some((scope, s.clone()));
        }
        for &base in &self.scopes[scope].inherited {
            if let Some(found) = self.find_in(base, name) {
                return Some(found);
            }
        }
        None
    }
}

/// The construct at the end of a cause chain.
fn root_construct(causes: &HashMap<String, Cause>, c: &Cause) -> String {
    let mut c = c;
    let mut hops = 0;
    loop {
        match c {
            Cause::Direct { construct, .. } => return construct.clone(),
            Cause::Through { target, .. } => {
                hops += 1;
                match causes.get(target) {
                    // Every `Through` was created pointing at a key already
                    // present, and a chain cannot be longer than the table.
                    Some(next) if hops <= causes.len() => c = next,
                    _ => return "a deferred type".to_owned(),
                }
            }
        }
    }
}

/// The chain as prose. `first` is the declaration's own clause; every later
/// hop is a relative clause about the type the previous one named.
fn reason_prose(causes: &HashMap<String, Cause>, c: &Cause, first: bool) -> String {
    // "it is" / "member "x" is" for the declaration's own clause; "which is" /
    // "whose member "x" is" for every hop after it.
    let lead = |site: &str| match (first, site.is_empty()) {
        (true, true) => "it is".to_owned(),
        (true, false) => format!("{site} is"),
        (false, true) => "which is".to_owned(),
        (false, false) => format!("whose {} is", site.strip_prefix("the ").unwrap_or(site)),
    };
    match c {
        Cause::Direct { site, construct } => {
            let named = match construct.as_str() {
                c if c.starts_with("fixed") || c == "ValueBase" => c.to_owned(),
                c if c.starts_with('a') => format!("an {c}"),
                c => format!("a {c}"),
            };
            format!("{} {named}", lead(site.as_deref().unwrap_or("")))
        }
        Cause::Through { site, target } => {
            let rest = causes
                .get(target)
                .map(|next| reason_prose(causes, next, false))
                .unwrap_or_else(|| "which is deferred".to_owned());
            format!("{} {target:?}, {rest}", lead(site))
        }
    }
}

/// What class of literal a value is, and what class a type wants.
///
/// # Why a class and not a type
///
/// IDL's rule for a constant's value, measured against omniidl 4.3.4 on
/// 2026-08-21 one file per pair, is that **the literal's class must be the
/// declared type's class, with no conversion whatsoever**. Every one of these
/// is refused, and the first four were surprises worth measuring rather than
/// assuming:
///
/// ```text
/// const double  A = 5;      Cannot interpret integer literal as a double
/// const fixed   A = 5;      Cannot interpret integer literal as fixed point
/// const long    A = 'a';    Cannot interpret character literal as an integer
/// const char    A = 65;     Cannot interpret integer literal as a character
/// const long    A = TRUE;   Cannot interpret boolean literal as an integer
/// const wstring A = "hi";   Cannot interpret string literal as a wide string
/// const double  A = 9.9d;   Cannot interpret fixed point literal as a double
/// const long    A = X;      Cannot interpret enumerator as an integer
/// ```
///
/// So width is not the axis — `char` and `octet` are both one byte and neither
/// takes the other's literal. The range check is a *second* rule that applies
/// inside [`Class::Int`] only.
///
/// *리터럴의 종류가 선언된 타입의 종류와 같아야 한다. 변환은 없다.*
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Int,
    Float,
    Fixed,
    Char,
    WChar,
    Str,
    WStr,
    Bool,
    Enum,
}

impl Class {
    /// How omniidl names this class in "cannot interpret _ as …".
    fn literal_name(self) -> &'static str {
        match self {
            Class::Int => "an integer literal",
            Class::Float => "a floating point literal",
            Class::Fixed => "a fixed point literal",
            Class::Char => "a character literal",
            Class::WChar => "a wide character literal",
            Class::Str => "a string literal",
            Class::WStr => "a wide string literal",
            Class::Bool => "a boolean literal",
            Class::Enum => "an enumerator",
        }
    }

    /// How omniidl names it as the target.
    fn article_name(self) -> &'static str {
        match self {
            Class::Int => "an integer",
            Class::Float => "a floating point number",
            Class::Fixed => "fixed point",
            Class::Char => "a character",
            Class::WChar => "a wide character",
            Class::Str => "a string",
            Class::WStr => "a wide string",
            Class::Bool => "a boolean",
            Class::Enum => "an enumerator",
        }
    }

    /// The edit, which is the half a diagnostic is for.
    fn how_to_write(self) -> &'static str {
        match self {
            Class::Int => "a plain integer, as in `42`",
            Class::Float => "a literal with a point or an exponent, as in `1.0`",
            Class::Fixed => "a decimal with a `d` suffix, as in `9.9d`",
            Class::Char => "a character literal, as in `'a'`",
            Class::WChar => "a wide character literal, as in `L'a'`",
            Class::Str => "a string literal, as in `\"text\"`",
            Class::WStr => "a wide string literal, as in `L\"text\"`",
            Class::Bool => "`TRUE` or `FALSE`",
            Class::Enum => "one of that enum's own enumerators",
        }
    }
}

/// A constant's value as the front end evaluates it. See [`Analyser::fold`].
#[derive(Debug, Clone, PartialEq)]
enum ConstFold {
    Int(i128),
    Float(f64),
    Fixed(FixedLit),
    Char(char),
    WChar(char),
    Str(String),
    WStr(String),
    Bool(bool),
    /// The lowercase name of the enum an enumerator belongs to.
    Enum(String),
}

impl ConstFold {
    fn class(&self) -> Class {
        match self {
            ConstFold::Int(_) => Class::Int,
            ConstFold::Float(_) => Class::Float,
            ConstFold::Fixed(_) => Class::Fixed,
            ConstFold::Char(_) => Class::Char,
            ConstFold::WChar(_) => Class::WChar,
            ConstFold::Str(_) => Class::Str,
            ConstFold::WStr(_) => Class::WStr,
            ConstFold::Bool(_) => Class::Bool,
            ConstFold::Enum(_) => Class::Enum,
        }
    }

    /// A placeholder of the class a declared type wants, for comparing classes.
    /// `None` for a type no constant can have — those are `const_type`'s
    /// business and are already refused by the parser.
    fn from_type(t: &TypeSpec) -> Option<Self> {
        Some(match t {
            TypeSpec::Short
            | TypeSpec::UShort
            | TypeSpec::Long
            | TypeSpec::ULong
            | TypeSpec::LongLong
            | TypeSpec::ULongLong
            | TypeSpec::Octet => ConstFold::Int(0),
            TypeSpec::Float | TypeSpec::Double => ConstFold::Float(0.0),
            TypeSpec::Fixed { .. } => ConstFold::Fixed(FixedLit { unscaled: 0, scale: 0 }),
            TypeSpec::Char => ConstFold::Char('\0'),
            TypeSpec::WChar => ConstFold::WChar('\0'),
            TypeSpec::String(_) => ConstFold::Str(String::new()),
            TypeSpec::WString(_) => ConstFold::WStr(String::new()),
            TypeSpec::Boolean => ConstFold::Bool(false),
            // A named type that survived `base_type` is an enum, or something
            // that is not a constant's type at all; the enum arm is the only
            // one with a literal, and a non-enum falls out as a class mismatch
            // against whatever was written.
            TypeSpec::Named(n) => ConstFold::Enum(n.last().to_lowercase()),
            _ => return None,
        })
    }
}

/// The closed range of an integer type, and its name for a message.
fn int_range(t: &TypeSpec) -> Option<(i128, i128, &'static str)> {
    Some(match t {
        TypeSpec::Octet => (0, 0xFF, "octet"),
        TypeSpec::Short => (i128::from(i16::MIN), i128::from(i16::MAX), "short"),
        TypeSpec::UShort => (0, i128::from(u16::MAX), "unsigned short"),
        TypeSpec::Long => (i128::from(i32::MIN), i128::from(i32::MAX), "long"),
        TypeSpec::ULong => (0, i128::from(u32::MAX), "unsigned long"),
        TypeSpec::LongLong => (i128::from(i64::MIN), i128::from(i64::MAX), "long long"),
        TypeSpec::ULongLong => (0, i128::from(u64::MAX), "unsigned long long"),
        _ => return None,
    })
}

/// A constant expression as it was written, for `fixed<9,2>` in a message.
fn const_text(e: &ConstExpr) -> String {
    match e {
        ConstExpr::Int(v) => v.to_string(),
        ConstExpr::Float(v) => v.to_string(),
        ConstExpr::Fixed(v) => v.to_string(),
        ConstExpr::Str(s) => format!("{s:?}"),
        ConstExpr::WStr(s) => format!("L{s:?}"),
        ConstExpr::Char(c) => format!("'{c}'"),
        ConstExpr::WChar(c) => format!("L'{c}'"),
        ConstExpr::Bool(b) => b.to_string(),
        ConstExpr::Name(n) => n.text(),
        ConstExpr::Unary { op, operand } => format!("{op}{}", const_text(operand)),
        ConstExpr::Binary { op, left, right } => {
            format!("{} {op} {}", const_text(left), const_text(right))
        }
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
        let d =
            diags("module m { struct Position { double x; }; struct T { Position position; }; };");
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
        let d = diags(
            "module m { struct Order { long id; }; interface P { void place(in Order order); }; };",
        );
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

    // ── a constant's value against its declared type ────────────────────────

    /// Every literal class against every type that takes one.
    ///
    /// omniidl 4.3.4 is strictly typed here and converts nothing: `char` and
    /// `octet` are both one octet and neither takes the other's literal. All
    /// of these validated clean before this check existed — there was no check
    /// at all — and each row is one file measured on 2026-08-21.
    #[test]
    fn a_constants_value_must_be_a_literal_of_its_own_class() {
        for (src, why) in [
            ("const double A = 5;", "an integer is not a double"),
            ("const float A = 5;", "nor a float"),
            ("const fixed A = 5;", "nor fixed point"),
            ("const fixed A = 9.9;", "a float literal is not a decimal"),
            ("const double A = 9.9d;", "and a decimal is not a double"),
            ("const char A = 65;", "an integer is not a character"),
            ("const long A = 'a';", "nor the reverse"),
            ("const octet A = 'a';", "width is not the axis"),
            ("const boolean A = 1;", "a boolean takes TRUE or FALSE"),
            ("const long A = TRUE;", "and gives nothing back"),
            ("const wchar A = 'a';", "a narrow literal is not a wide one"),
            ("const wstring A = \"hi\";", "in either shape"),
            ("const char A = L'a';", "nor the reverse"),
            ("const string A = L\"hi\";", "in either shape"),
            ("const string A = 'a';", "a character is not a string"),
            ("const char A = \"a\";", "nor the reverse"),
        ] {
            let d = diags(&format!("module m {{ {src} }};"));
            assert_eq!(d.len(), 1, "{src}: {why} — {d:?}");
            assert_eq!(d[0].rule, "const-value-type", "{src}");
        }
    }

    /// The classes that *do* match, so the rule above is a rule and not a ban.
    #[test]
    fn a_matching_literal_class_is_accepted() {
        for src in [
            "const double A = 5.0;",
            "const float A = 1e3;",
            "const fixed A = 9.9d;",
            "const char A = 'a';",
            "const wchar A = L'a';",
            "const string A = \"hi\";",
            "const wstring A = L\"hi\";",
            "const boolean A = TRUE;",
            "const octet A = 7;",
            "const unsigned long long A = 18446744073709551615;",
            "enum E { X }; const E A = X;",
            "typedef long Count; const Count A = 5;",
            "const long A = 5; const long B = A;",
        ] {
            let d = diags(&format!("module m {{ {src} }};"));
            assert!(d.is_empty(), "{src} is legal IDL: {d:?}");
        }
    }

    /// An integer constant must fit the type it was declared with.
    ///
    /// The rule was in `orbweaver_registry::coerce` and reported nothing: its
    /// only effect was that the registry stored no value, which both emitters
    /// then skipped in silence. The registry's own doc comment described this
    /// as "an IDL error the checker reports" while no checker reported it.
    #[test]
    fn an_integer_constant_must_fit_its_declared_type() {
        for (src, edge) in [
            ("const octet A = 256;", "255"),
            ("const octet A = -1;", "0"),
            ("const short A = 40000;", "32767"),
            ("const short A = -32769;", "-32768"),
            ("const unsigned short A = 65536;", "65535"),
            ("const long A = 2147483648;", "2147483647"),
            ("const unsigned long A = 4294967296;", "4294967295"),
            ("const unsigned long A = -1;", "0"),
        ] {
            let d = diags(&format!("module m {{ {src} }};"));
            assert_eq!(d.len(), 1, "{src}: {d:?}");
            assert_eq!(d[0].rule, "const-value-range", "{src}");
            assert!(d[0].message.contains(edge), "{src}: the message names the edge: {d:?}");
        }
        // The check is on the folded result, not on the operands — which is
        // what omniidl does, measured: `40000 - 10000` is a legal short.
        assert!(diags("module m { const short A = 40000 - 10000; };").is_empty());
        // Each type's own boundary is inside it.
        for src in [
            "const octet A = 255;",
            "const short A = -32768;",
            "const long A = -2147483648;",
            "const long long A = -9223372036854775808;",
            "const unsigned long long A = 18446744073709551615;",
        ] {
            assert!(diags(&format!("module m {{ {src} }};")).is_empty(), "{src}");
        }
    }

    /// `long double` is admitted by the grammar and refused by the language,
    /// and the refusal follows a `typedef` because nothing at the declaration
    /// says `long double`.
    #[test]
    fn long_double_is_never_a_constants_type() {
        let d = diags("module m { const long double A = 1.0; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "not-a-const-type");
        let d = diags("module m { typedef long double D; const D A = 1.0; };");
        assert_eq!(d.len(), 1, "through an alias too: {d:?}");
        assert_eq!(d[0].rule, "not-a-const-type");
        // The type itself is fine everywhere a value of it is not written.
        assert!(diags("module m { struct S { long double d; }; };").is_empty());
    }

    /// An enumerator initialises a constant only of its own enum, and a
    /// bounded string constant may not exceed its bound.
    #[test]
    fn an_enumerator_and_a_bound_are_checked_against_the_declared_type() {
        let d = diags("module m { enum E { X }; enum F { Y }; const E A = Y; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "const-value-type");
        assert!(diags("module m { enum E { X, Z }; const E A = Z; };").is_empty());

        let d = diags("module m { const string<2> A = \"abc\"; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "const-value-range");
        assert!(diags("module m { const string<8> A = \"abc\"; };").is_empty());
    }

    /// An expression with no value is reported as one, rather than falling out
    /// of the fold as an unexplained `None`.
    #[test]
    fn a_constant_that_divides_by_zero_is_reported() {
        let d = diags("module m { const long A = 1 / 0; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "const-value-range");
        let d = diags("module m { const long Z = 0; const long A = 8 / Z; };");
        assert_eq!(d.len(), 1, "through a named divisor too: {d:?}");
    }

    /// A constant whose value this pass cannot evaluate is **not** an error.
    ///
    /// `None` from the fold means "not evaluated", never "invalid". A rule
    /// that reported one as the other would reject legal IDL for the crime of
    /// being written in a form the front end has not implemented — which is
    /// the failure mode a checker built on a folder invites.
    #[test]
    fn an_unevaluated_constant_is_not_an_error() {
        for src in [
            // Exact decimal arithmetic the front end leaves to the registry.
            "const fixed A = 1.0d + 2.0d;",
            // A shift whose width this fold does not implement.
            "const long A = 1 << 3;",
        ] {
            let d = diags(&format!("module m {{ {src} }};"));
            assert!(d.is_empty(), "{src} is legal IDL: {d:?}");
        }
    }

    /// An unresolved **qualified** name is a different diagnosis from an
    /// unresolved bare one, and files under a different rule.
    ///
    /// The generic advice for `unknown-name` is *"qualify it with its module"*,
    /// which is meaningless for a name that is already qualified — the estate
    /// printed it ~90 times as *"qualify `::` with `Module::::`"*
    /// (`docs/pipeline-runs/2026-08-14-estate.md`, RC-2). Half of that was a
    /// wrong span, fixed in `parse`; the other half was two diagnoses sharing
    /// one rule, which is this.
    #[test]
    fn an_unresolved_qualified_name_names_the_component_that_failed() {
        let d = diags("module m { struct S { ::Elsewhere::Common::Widget w; }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "unknown-scoped-name");
        assert!(d[0].message.contains("::Elsewhere::Common::Widget"), "{}", d[0].message);
        assert!(d[0].message.contains("\"Elsewhere\""), "{}", d[0].message);
        assert!(d[0].message.contains("#include"), "{}", d[0].message);

        // A name whose *leading* components resolve says which one broke, and
        // where it looked — the reader would otherwise re-check the whole path.
        let d = diags("module outer { module inner { }; struct S { outer::inner::Widget w; }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "unknown-scoped-name");
        assert!(d[0].message.contains("\"Widget\""), "{}", d[0].message);
        assert!(d[0].message.contains("outer::inner"), "{}", d[0].message);
    }

    /// A bare unknown name keeps the old rule, because the old advice is right
    /// for it and the consumers that render it are not ours to change.
    #[test]
    fn an_unresolved_bare_name_keeps_its_rule() {
        let d = diags("module m { struct S { Widget w; }; };");
        assert_eq!(d[0].rule, "unknown-name");
        assert!(d[0].message.contains("#include"), "{}", d[0].message);
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

        let d = diags("module m { union U switch (long) { default: long a; default: long b; }; };");
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].rule, "duplicate-union-default");
    }

    /// A type may be used before it is declared in the same scope, so
    /// resolution has to wait for the whole file.
    #[test]
    fn forward_use_within_a_scope_is_allowed() {
        clean("module m { interface I { S get(); }; struct S { long a; }; };");
        clean(
            "module m { interface Node; typedef sequence<Node> Nodes; interface Node { Nodes kids(); }; };",
        );
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
        let d = diags("module m { struct S { long a; long a; }; struct T { Widget w; }; };");
        assert!(d.len() >= 2);
        assert!(d[0].span.line <= d[1].span.line);
        assert!(d[0].span.column < d[1].span.column || d[0].span.line < d[1].span.line);
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    use crate::parse;

    fn deferred(src: &str) -> Vec<DeferredWireUse> {
        deferred_wire_types(&parse(src).expect("should parse"))
    }

    fn names(src: &str) -> Vec<String> {
        deferred(src).into_iter().map(|d| d.declaration).collect()
    }

    fn golden(file: &str) -> String {
        let path = format!("{}/../../corpus/golden/{file}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// `corpus/golden/21`, the shape the whole rule exists for: the typedef,
    /// the struct that carries it, the interface whose operation carries the
    /// struct — the same three the generator skips.
    #[test]
    fn fixed_cascades_from_the_typedef_to_everything_that_carries_it_by_value() {
        let d = deferred(&golden("21-deferred-fixed.idl"));
        assert_eq!(
            d.iter().map(|d| d.declaration.as_str()).collect::<Vec<_>>(),
            ["gc21::Amount", "gc21::Invoice", "gc21::Billing"]
        );
        assert!(d.iter().all(|d| d.construct == "fixed<9,2>"), "{d:?}");
        assert_eq!(d[0].reason, "it is fixed<9,2>");
        assert_eq!(d[1].reason, "member \"total\" is \"gc21::Amount\", which is fixed<9,2>");
        assert_eq!(
            d[2].reason,
            "the return of operation \"sum\" is \"gc21::Amount\", which is fixed<9,2>"
        );
        assert!(d[1].message().contains("§4.4"), "{}", d[1].message());
        assert!(d[1].fix().contains("string"), "{}", d[1].fix());
        assert_eq!(d[0].diagnostic().rule, DEFERRED_WIRE_RULE);
    }

    /// `corpus/golden/20`: two valuetypes, an abstract interface, and an
    /// interface returning a valuetype. All four, and the reason names which.
    #[test]
    fn valuetypes_and_abstract_interfaces_are_the_construct_themselves() {
        let d = deferred(&golden("20-deferred-valuetype.idl"));
        assert_eq!(
            d.iter().map(|d| (d.declaration.as_str(), d.reason.as_str())).collect::<Vec<_>>(),
            [
                ("gc20::Money", "it is a valuetype"),
                ("gc20::Named", "it is a valuetype"),
                ("gc20::Describable", "it is an abstract interface"),
                (
                    "gc20::Wallet",
                    "the return of operation \"balance\" is \"gc20::Money\", which is a valuetype"
                ),
            ]
        );
        assert_eq!(d[2].family(), "abstract interfaces");
        assert_eq!(d[3].family(), "valuetypes");
    }

    /// The closure follows values and stops at references. An interface that
    /// *uses* `fixed` is deferred; a struct holding a reference to that
    /// interface is not — the reference is an IOR whatever the interface takes
    /// — and neither is an interface that returns such a reference. But an
    /// interface *inheriting* the deferred one is: it has the operation.
    #[test]
    fn references_to_a_deferred_interface_do_not_propagate_but_inheritance_does() {
        let src = "module m {
            typedef fixed<9,2> Amount;
            interface Billing { Amount sum(); };
            struct Holder { Billing b; };
            interface Lookup { Billing find(); };
            interface Sub : Billing { void ping(); };
            struct Abs { long a; };
        };";
        assert_eq!(names(src), ["m::Amount", "m::Billing", "m::Sub"]);
        let d = deferred(src);
        assert_eq!(
            d[2].reason,
            "base \"Billing\" is \"m::Billing\", whose return of operation \"sum\" is \
             \"m::Amount\", which is fixed<9,2>"
        );
    }

    /// An abstract interface *is* the construct, so a member typed as one
    /// propagates — that is the value-or-reference union v1 cannot carry — and
    /// so does the `ValueBase` keyword. `Object` is a reference and does not.
    #[test]
    fn abstract_interface_members_and_valuebase_propagate() {
        let src = "module m {
            abstract interface Describable { string describe(); };
            struct Card { Describable d; };
            struct Anything { ValueBase v; };
            struct Plain { Object o; };
        };";
        assert_eq!(names(src), ["m::Describable", "m::Card", "m::Anything"]);
        let d = deferred(src);
        assert_eq!(
            d[1].reason,
            "member \"d\" is \"m::Describable\", which is an abstract interface"
        );
        assert_eq!(d[2].reason, "member \"v\" is ValueBase");
    }

    /// A `native` is in this closure and the predeclared `::CORBA::TypeCode`
    /// is not, and the second half is the one that had to be asserted rather
    /// than reasoned about.
    ///
    /// Both are `SymbolKind::Native` in this analyser. Only a `native` written
    /// in the contract becomes a `WireDecl` and so acquires a cause, so
    /// `::CORBA::TypeCode` is named as a target and never found in the table —
    /// which is correct, `tk_TypeCode` marshals perfectly well. That is a
    /// property of the cause table rather than of `wire_target`, and a
    /// property nobody wrote down is a property that gets refactored away.
    ///
    /// The family is checked too: a native must not be filed under
    /// `"valuetypes"`, because [`DeferredWireUse::message`] branches on it to
    /// say that §4.4 does not apply.
    #[test]
    fn a_native_propagates_and_the_predeclared_corba_typecode_does_not() {
        let src = "module m {
            native Handle;
            struct Session { Handle token; };
            typedef sequence<Handle> Roster;
            interface Broker { Handle acquire(); };
            struct Described { ::CORBA::TypeCode what; };
            struct Plain { long a; };
        };";
        assert_eq!(names(src), ["m::Handle", "m::Session", "m::Roster", "m::Broker"]);
        let d = deferred(src);
        let got: Vec<(&str, &str, &str)> =
            d.iter().map(|d| (d.declaration.as_str(), d.reason.as_str(), d.family())).collect();
        assert_eq!(
            got,
            [
                ("m::Handle", "it is a native", "natives"),
                ("m::Session", "member \"token\" is \"m::Handle\", which is a native", "natives"),
                ("m::Roster", "it is \"m::Handle\", which is a native", "natives"),
                (
                    "m::Broker",
                    "the return of operation \"acquire\" is \"m::Handle\", which is a native",
                    "natives"
                ),
            ]
        );
        // The sentence, not just the family: "§4.4 defers natives" would be
        // false, and a gate that says a false thing gets bypassed.
        let m = d[0].message();
        assert!(m.contains("cannot go on the wire at all"), "{m}");
        assert!(m.contains("not one of docs/PLAN.md §4.4's three deferrals"), "{m}");
    }

    /// Every site a `fixed` can hide behind, each reported once per
    /// declaration with the site named: a sequence element, a union case, an
    /// attribute, a parameter carrying an exception, a constant, an array
    /// typedef, and two hops of struct nesting.
    #[test]
    fn every_carrying_site_is_found_and_named() {
        let src = "module m {
            typedef sequence<fixed<5,1> > Seq;
            union U switch (long) { case 1: fixed<3,0> f; default: long n; };
            exception Bad { fixed<2,1> why; };
            typedef fixed<4,2> Rate;
            interface I {
              attribute Rate spot;
              void g(inout Rate x);
            };
            interface J { void h(in Bad b); };
            const fixed C = 12.5D;
            const Rate R = 12.5D;
            typedef fixed<7,3> Arr[4];
            struct Deep { Seq s; };
            struct Deeper { Deep d; };
        };";
        let d = deferred(src);
        let got: Vec<(&str, &str)> =
            d.iter().map(|d| (d.declaration.as_str(), d.reason.as_str())).collect();
        assert_eq!(
            got,
            [
                ("m::Seq", "it is fixed<5,1>"),
                ("m::U", "case \"f\" is fixed<3,0>"),
                ("m::Bad", "member \"why\" is fixed<2,1>"),
                ("m::Rate", "it is fixed<4,2>"),
                ("m::I", "attribute \"spot\" is \"m::Rate\", which is fixed<4,2>"),
                (
                    "m::J",
                    "parameter \"b\" of operation \"h\" is \"m::Bad\", whose member \"why\" is \
                     fixed<2,1>"
                ),
                // `m::C` and `m::R` are absent on purpose: a constant is not
                // marshalled, so it is outside this rule's closure however
                // its type is spelled — bare `fixed` or a name that resolves
                // to one. See the `Definition::Const` arm.
                ("m::Arr", "it is fixed<7,3>"),
                ("m::Deep", "member \"s\" is \"m::Seq\", which is fixed<5,1>"),
                (
                    "m::Deeper",
                    "member \"d\" is \"m::Deep\", whose member \"s\" is \"m::Seq\", which is \
                     fixed<5,1>"
                ),
            ]
        );
        assert!(
            !got.iter().any(|(name, _)| *name == "m::C" || *name == "m::R"),
            "a constant is not a wire declaration: {got:?}"
        );
    }

    /// A constant is out of the closure, stated on its own rather than left to
    /// an absence in the list above.
    ///
    /// The rule answers "can a v1 peer be served this contract", and nothing
    /// about a constant reaches a peer: no operation carries one, no TypeCode
    /// of one is encoded. Naming it made `--wire v1` refuse a whole file for a
    /// declaration whose wire cost is zero, under a message that said the
    /// constant could not go on the wire — which is true of no constant at
    /// all. What such a constant does cost is one generated binding, and both
    /// emitters report that as a skip of their own.
    #[test]
    fn a_constant_is_outside_the_closure_and_its_typedef_is_not() {
        assert!(names("const fixed C = 12.5D;").is_empty());
        assert_eq!(
            names("module m { typedef fixed<3,1> Ratio; const Ratio LIMIT = 9.9d; };"),
            ["m::Ratio"],
            "the typedef is a type a signature can use; the constant is not"
        );
        // A file whose only §4.4 construct is a constant is servable, which is
        // the verdict `--wire v1` now gives it.
        assert!(names("module m { const fixed C = 1.5d; interface I { long f(); }; };").is_empty());
    }

    /// The exception reached only through `raises` cascades to the interface
    /// (the skeleton marshals it), and a plain file reports nothing.
    #[test]
    fn raises_cascades_and_a_clean_file_is_empty() {
        let src = "module m {
            exception Bad { fixed<2,1> why; };
            interface I { void f() raises (Bad); };
        };";
        let d = deferred(src);
        assert_eq!(names(src), ["m::Bad", "m::I"]);
        assert_eq!(
            d[1].reason,
            "the exception operation \"f\" raises is \"m::Bad\", whose member \"why\" is fixed<2,1>"
        );
        assert!(
            names("module m { struct S { long a; string b; }; interface I { S get(); }; };")
                .is_empty()
        );
    }

    /// The pass is not a diagnostic: the file still checks out, because the
    /// oracle accepts it and agreement with the oracle is what `is_ok` means.
    #[test]
    fn a_deferred_construct_does_not_make_the_analysis_fail() {
        let a = analyse(&parse("module m { typedef fixed<9,2> Amount; };").unwrap());
        assert!(a.is_ok(), "{:?}", a.diagnostics);
        assert_eq!(a.deferred_wire.len(), 1);
    }

    /// A forward declaration and its definition are one declaration; a
    /// recursive struct does not loop; a nested typedef inside an interface
    /// carries the interface's name and does not by itself defer the interface.
    #[test]
    fn forward_declarations_are_reported_once_recursion_terminates_nesting_is_named() {
        let src = "module m {
            valuetype V;
            valuetype V { public long a; };
            struct Node { sequence<Node> kids; fixed<2,1> f; };
            struct Tree { Node root; };
            interface Holder { typedef fixed<4,0> Ticket; void ping(); };
        };";
        assert_eq!(names(src), ["m::V", "m::Node", "m::Tree", "m::Holder::Ticket"]);
    }
}
