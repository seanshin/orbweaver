//! The IDL syntax tree, with SIDL annotations attached to what they describe.

use crate::lex::{Annotation, Span};

/// A parsed IDL file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Spec {
    /// Top-level definitions, in source order.
    pub definitions: Vec<Definition>,
}

/// Anything that can appear at file or module scope.
#[derive(Debug, Clone, PartialEq)]
pub enum Definition {
    /// `module X { ... };`
    Module(Module),
    /// `interface X { ... };` or a forward declaration.
    Interface(Interface),
    /// `struct X { ... };` or a forward declaration.
    Struct(StructDef),
    /// `union X switch (T) { ... };`
    Union(UnionDef),
    /// `enum X { ... };`
    Enum(EnumDef),
    /// `exception X { ... };`
    Exception(StructDef),
    /// `typedef T X;`
    Typedef(Typedef),
    /// `const T X = ...;`
    Const(ConstDef),
    /// `valuetype X { ... };` — parsed, not marshalled (PLAN §4.4).
    ValueType(ValueTypeDef),
    /// `native X;`
    Native(Named),
}

impl Definition {
    /// The declared name, for scope and clash checking.
    pub fn name(&self) -> &Named {
        match self {
            Definition::Module(m) => &m.name,
            Definition::Interface(i) => &i.name,
            Definition::Struct(s) | Definition::Exception(s) => &s.name,
            Definition::Union(u) => &u.name,
            Definition::Enum(e) => &e.name,
            Definition::Typedef(t) => &t.name,
            Definition::Const(c) => &c.name,
            Definition::ValueType(v) => &v.name,
            Definition::Native(n) => n,
        }
    }
}

/// A name with the position it was written at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    /// The identifier as written.
    pub text: String,
    /// Where it was written.
    pub span: Span,
}

impl Named {
    /// Whether two names collide under IDL's case-insensitive comparison.
    ///
    /// The single most expensive rule in this project: it caused every failure
    /// in the Phase 0 assumption B benchmark and has since taken four distinct
    /// syntactic shapes.
    pub fn clashes_with(&self, other: &str) -> bool {
        self.text.eq_ignore_ascii_case(other)
    }
}

/// `module X { ... };`
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    /// Module name.
    pub name: Named,
    /// Contents.
    pub definitions: Vec<Definition>,
    /// Annotations written above it.
    pub annotations: Vec<Annotation>,
}

/// `interface X : A, B { ... };`
#[derive(Debug, Clone, PartialEq)]
pub struct Interface {
    /// Interface name.
    pub name: Named,
    /// Base interfaces, as written.
    pub bases: Vec<ScopedName>,
    /// Members, or `None` for a forward declaration.
    pub body: Option<Vec<InterfaceMember>>,
    /// `abstract` or `local` modifier.
    pub modifier: Option<InterfaceModifier>,
    /// Annotations written above it.
    pub annotations: Vec<Annotation>,
}

/// `abstract` / `local` on an interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum InterfaceModifier {
    Abstract,
    Local,
}

/// What can appear inside an interface.
#[derive(Debug, Clone, PartialEq)]
pub enum InterfaceMember {
    /// An operation.
    Operation(Operation),
    /// An attribute.
    Attribute(AttributeDef),
    /// A nested type, constant or exception.
    Nested(Definition),
}

/// An operation declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Operation {
    /// Operation name.
    pub name: Named,
    /// Return type.
    pub returns: TypeSpec,
    /// Parameters in order.
    pub params: Vec<Param>,
    /// Exceptions in the `raises` clause.
    pub raises: Vec<ScopedName>,
    /// Whether it is `oneway`.
    pub oneway: bool,
    /// Annotations written above it.
    pub annotations: Vec<Annotation>,
}

/// Parameter direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Direction {
    In,
    Out,
    InOut,
}

/// One operation parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// Direction.
    pub direction: Direction,
    /// Parameter type.
    pub ty: TypeSpec,
    /// Parameter name.
    pub name: Named,
    /// Annotations written above it.
    pub annotations: Vec<Annotation>,
}

/// `attribute T x;` or `readonly attribute T x;`
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeDef {
    /// Whether it is read-only.
    pub readonly: bool,
    /// Attribute type.
    pub ty: TypeSpec,
    /// Declared names — one declaration may introduce several.
    pub names: Vec<Named>,
    /// Annotations written above it.
    pub annotations: Vec<Annotation>,
}

/// `struct X { ... };` or `exception X { ... };`
#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    /// Type name.
    pub name: Named,
    /// Members, or `None` for a forward declaration.
    pub members: Option<Vec<Member>>,
    /// Annotations written above it.
    pub annotations: Vec<Annotation>,
}

/// A struct, exception or valuetype member.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    /// Member type.
    pub ty: TypeSpec,
    /// Declared names.
    pub names: Vec<Named>,
    /// Annotations written above it.
    pub annotations: Vec<Annotation>,
}

/// `union X switch (T) { ... };`
#[derive(Debug, Clone, PartialEq)]
pub struct UnionDef {
    /// Union name.
    pub name: Named,
    /// Discriminator type.
    pub discriminator: TypeSpec,
    /// Branches.
    pub cases: Vec<UnionCase>,
    /// Annotations written above it.
    pub annotations: Vec<Annotation>,
}

/// One union branch, which may carry several labels.
#[derive(Debug, Clone, PartialEq)]
pub struct UnionCase {
    /// Case labels; empty means `default`.
    pub labels: Vec<ConstExpr>,
    /// Whether this branch is the `default`.
    pub is_default: bool,
    /// Branch member.
    pub member: Member,
}

/// `enum X { A, B };`
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    /// Enum name.
    pub name: Named,
    /// Enumerators in order.
    pub members: Vec<Named>,
    /// Annotations written above it.
    pub annotations: Vec<Annotation>,
}

/// `typedef T X;`
#[derive(Debug, Clone, PartialEq)]
pub struct Typedef {
    /// The aliased type.
    pub ty: TypeSpec,
    /// The new name.
    pub name: Named,
    /// Array dimensions, if the typedef declares an array.
    pub dimensions: Vec<ConstExpr>,
    /// Annotations written above it.
    pub annotations: Vec<Annotation>,
}

/// `const T X = expr;`
#[derive(Debug, Clone, PartialEq)]
pub struct ConstDef {
    /// Constant type.
    pub ty: TypeSpec,
    /// Constant name.
    pub name: Named,
    /// Its value.
    pub value: ConstExpr,
    /// Annotations written above it.
    pub annotations: Vec<Annotation>,
}

/// `valuetype X { ... };`
#[derive(Debug, Clone, PartialEq)]
pub struct ValueTypeDef {
    /// Value type name.
    pub name: Named,
    /// Base value type, if any.
    pub base: Option<ScopedName>,
    /// Interfaces it supports.
    pub supports: Vec<ScopedName>,
    /// Members, or `None` for a forward declaration.
    pub members: Option<Vec<ValueMember>>,
    /// Whether it is `abstract`.
    pub is_abstract: bool,
    /// Annotations written above it.
    pub annotations: Vec<Annotation>,
}

/// A member of a valuetype, which may be public or private.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueMember {
    /// A state member.
    State {
        /// Whether it is `public`.
        public: bool,
        /// The member.
        member: Member,
    },
    /// An operation, attribute or nested definition.
    Other(Box<InterfaceMember>),
}

/// A possibly-qualified name, e.g. `::CORBA::TypeCode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedName {
    /// Whether it began with `::`.
    pub absolute: bool,
    /// The parts, in order.
    pub parts: Vec<String>,
    /// Where it was written.
    pub span: Span,
}

impl ScopedName {
    /// The final component, which is what a clash check compares.
    pub fn last(&self) -> &str {
        self.parts.last().map(String::as_str).unwrap_or("")
    }

    /// The name as written.
    pub fn text(&self) -> String {
        let joined = self.parts.join("::");
        if self.absolute { format!("::{joined}") } else { joined }
    }
}

/// A type as written in a declaration.
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub enum TypeSpec {
    Void,
    Boolean,
    Char,
    WChar,
    Octet,
    Short,
    UShort,
    Long,
    ULong,
    LongLong,
    ULongLong,
    Float,
    Double,
    LongDouble,
    Any,
    Object,
    ValueBase,
    /// `string` or `string<N>`.
    String(Option<Box<ConstExpr>>),
    /// `wstring` or `wstring<N>`.
    WString(Option<Box<ConstExpr>>),
    /// `fixed<digits, scale>`.
    Fixed {
        digits: Box<ConstExpr>,
        scale: Box<ConstExpr>,
    },
    /// `sequence<T>` or `sequence<T, N>`.
    Sequence {
        element: Box<TypeSpec>,
        bound: Option<Box<ConstExpr>>,
    },
    /// A reference to a named type.
    Named(ScopedName),
}

/// A constant expression, kept as a tree rather than folded.
///
/// Folding at parse time would lose the source form, and the registry needs to
/// report what an author wrote as well as what it evaluates to.
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub enum ConstExpr {
    Int(i64),
    Float(f64),
    Str(String),
    Char(char),
    Bool(bool),
    Name(ScopedName),
    Unary { op: &'static str, operand: Box<ConstExpr> },
    Binary { op: &'static str, left: Box<ConstExpr>, right: Box<ConstExpr> },
}
