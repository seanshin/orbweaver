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

use crate::ast::*;
use crate::lex::{Annotation, LexError, Lexer, Span, Tok, Token};

/// A parse failure, positioned and phrased as something to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// What to do about it.
    pub message: String,
    /// Where.
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}: {}", self.span.line, self.span.column, self.message)
    }
}

impl std::error::Error for ParseError {}

impl From<LexError> for ParseError {
    fn from(e: LexError) -> Self {
        ParseError { message: e.message, span: e.span }
    }
}

/// Result of a parse.
pub type Result<T> = std::result::Result<T, ParseError>;

/// Parses a whole IDL source file.
pub fn parse(src: &str) -> Result<Spec> {
    let tokens = Lexer::new(src).tokenize()?;
    Parser { toks: tokens, i: 0 }.spec()
}

struct Parser {
    toks: Vec<Token>,
    i: usize,
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
        Err(ParseError { message: message.into(), span: self.peek().span })
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
                    });
                }
                Ok(Named { text, span: t.span })
            }
            other => Err(ParseError {
                message: format!("expected an identifier, found {other}"),
                span: t.span,
            }),
        }
    }

    fn take_annotations(&mut self) -> Vec<Annotation> {
        self.peek().annotations.clone()
    }

    // ── top level ───────────────────────────────────────────────────────────

    fn spec(&mut self) -> Result<Spec> {
        let mut definitions = Vec::new();
        while !matches!(self.peek_tok(), Tok::Eof) {
            definitions.push(self.definition()?);
        }
        Ok(Spec { definitions })
    }

    fn definition(&mut self) -> Result<Definition> {
        let ann = self.take_annotations();
        let d = self.definition_inner(ann)?;
        self.expect_punct(";")?;
        Ok(d)
    }

    fn definition_inner(&mut self, ann: Vec<Annotation>) -> Result<Definition> {
        if self.at_kw("module") {
            return Ok(Definition::Module(self.module(ann)?));
        }
        if self.at_kw("interface") || self.at_kw("abstract") || self.at_kw("local") {
            // `abstract`/`local` also lead a valuetype, so look one further.
            if self.at_kw("abstract") && matches!(self.toks.get(self.i + 1).map(|t| &t.tok), Some(Tok::Ident(s)) if s == "valuetype")
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
        let mut definitions = Vec::new();
        while !self.at_punct("}") {
            if matches!(self.peek_tok(), Tok::Eof) {
                return self.err(format!("module {:?} is missing its closing '}}'", name.text));
            }
            definitions.push(self.definition()?);
        }
        self.expect_punct("}")?;
        Ok(Module { name, definitions, annotations })
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
        let mut body = Vec::new();
        while !self.at_punct("}") {
            if matches!(self.peek_tok(), Tok::Eof) {
                return self.err(format!("interface {:?} is missing its closing '}}'", name.text));
            }
            body.push(self.interface_member()?);
        }
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
        let mut members = Vec::new();
        while !self.at_punct("}") {
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
        self.expect_punct("}")?;
        Ok(ValueTypeDef {
            name,
            base,
            supports,
            members: Some(members),
            is_abstract,
            annotations,
        })
    }

    // ── types ───────────────────────────────────────────────────────────────

    fn scoped_name(&mut self) -> Result<ScopedName> {
        let span = self.peek().span;
        let absolute = self.eat_punct("::");
        let mut parts = vec![self.expect_ident()?.text];
        while self.eat_punct("::") {
            parts.push(self.expect_ident()?.text);
        }
        Ok(ScopedName { absolute, parts, span })
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
            self.toks[self.i] =
                Token { tok: Tok::Punct(">"), span, annotations: Vec::new(), escaped: false };
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
            Tok::Ident(s) => {
                let mut parts = vec![s];
                let span = t.span;
                while self.eat_punct("::") {
                    parts.push(self.expect_ident()?.text);
                }
                Ok(ConstExpr::Name(ScopedName { absolute: false, parts, span }))
            }
            Tok::Punct("::") => {
                let mut parts = vec![self.expect_ident()?.text];
                while self.eat_punct("::") {
                    parts.push(self.expect_ident()?.text);
                }
                Ok(ConstExpr::Name(ScopedName { absolute: true, parts, span: t.span }))
            }
            other => Err(ParseError {
                message: format!("expected a constant expression, found {other}"),
                span: t.span,
            }),
        }
    }
}
