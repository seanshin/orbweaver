//! Constraint queries over the offer store: the §4.3 subset, grown toward
//! the OMG Trader Constraint Language (D022 T1).
//!
//! ```text
//! query       := disjunction order?
//! disjunction := conjunction ( "OR" conjunction )*
//! conjunction := negation ( "AND" negation )*
//! negation    := "NOT" negation | primary
//! primary     := "(" disjunction ")" | "EXIST" field | comparison
//! comparison  := field cmp literal
//! cmp         := "==" | "!=" | "<" | "<=" | ">" | ">="
//! order       := "ORDER" "BY" field ( "ASC" | "DESC" )
//! ```
//!
//! **Precedence, written down rather than implied:** `NOT` binds tighter
//! than `AND`, and `AND` binds tighter than `OR`; parentheses override both.
//! So `NOT a == 1 AND b == 2 OR c == 3` groups as
//! `((NOT (a == 1)) AND (b == 2)) OR (c == 3)`, and `NOT` applies to the one
//! primary that follows it — a comparison, an `EXIST`, or a parenthesised
//! expression — never to a trailing `AND`/`OR` chain. `AND` and `OR` are
//! associative in the three-valued logic below, so a chain of either is
//! parsed flat and no grouping question arises within one.
//!
//! e.g. `specialization == 'math' AND latency_p99 < 200 ORDER BY route_freq
//! DESC`. Fields are the [`crate::Offer`] properties by name; string
//! literals are single-quoted; residency literals are the bare enum names
//! (`RESIDENT`, …); keywords are uppercase, as the architecture document
//! writes them. Latencies are milliseconds by the offer contract, so
//! literals are bare numbers — no unit suffixes.
//!
//! # `EXIST`, and why it is not a new notion of "present"
//!
//! `EXIST field` answers whether the offer carries a value for `field` at
//! all. It is wired to `has_value` — the same predicate `ORDER BY` has
//! always used to decide that an offer cannot be *placed* — so there is one
//! notion of "the source could not say" in this crate and `EXIST` is a
//! first-class query over it, not a second one beside it. `EXIST` is the
//! only construct here that is **two-valued**: it answers [`Truth::Yes`] or
//! [`Truth::No`] and never [`Truth::Unknown`], because "does this offer
//! carry a specialization" is a question about the offer we can always
//! answer. That is what makes it the escape hatch: a query whose gapped
//! field is guarded by `EXIST` has no unanswerable offers left, so
//! [`Selection::is_complete`] can be true over a store the plain form could
//! only report a gap for.
//!
//! Only `specialization` and `latency_p50` can answer `EXIST … ` with `No`
//! today — they are the two the v1.0 wire registration cannot carry. Every
//! other field is always present, so `EXIST route_freq` is a constant `Yes`
//! and says so rather than pretending to be a filter.
//!
//! # Why the parser is first-party
//!
//! The same reasoning as `orbweaver-dynamic`'s JSON reader: this grammar is
//! a published contract (§4.3) small enough to own, and it sits where agent
//! output arrives — a generated query is untrusted input, so the limits and
//! the diagnostics should be ours. A recursive-descent parser also makes the
//! S4 lesson cheap to honour: every refusal names the byte position and what
//! was expected, because "did not parse" without a place to fix is exactly
//! the diagnostic quality the negative corpus exists to prevent.
//!
//! *Untrusted* is also why [`MAX_DEPTH`] exists. Parentheses and `NOT` are
//! the first constructs in this grammar that nest, and nesting is how a
//! generated string becomes a stack overflow — which is a crash, not a
//! refusal. Depth is bounded at parse time and the refusal names the limit
//! and the byte position of the token that exceeded it. Chains of `AND` and
//! `OR` do not nest at all: they are flattened into one node, so a query
//! with ten thousand conjuncts costs ten thousand slots and no stack.
//!
//! # Scope
//!
//! Exactly the subset above and nothing else. Against TCL that is still
//! missing `~` (substring), `in` (sequence membership) and arithmetic, and
//! the whole preference expression (`min`/`max`/`with`/`random`/`first`),
//! which is a *different* language and is D022 T2. Also still true: no unit
//! suffixes, and no case-insensitive keywords — TCL spells its own operators
//! lowercase (`and`, `or`, `not`, `exist`), and reconciling that spelling is
//! the wire facade's problem (D022 T4), not this engine's. A lowercase
//! keyword is refused here with the position and the uppercase form to
//! write, never quietly reinterpreted as a field name.

use std::cmp::Ordering;

use crate::preference::{Preference, PreferenceError};
use crate::{Offer, OfferStore, Residency};

/// Why a query could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset where it went wrong.
    pub at: usize,
    /// What was expected there, phrased as something to fix.
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "at byte {}: {}", self.at, self.message)
    }
}

impl std::error::Error for ParseError {}

/// How deeply `(` and `NOT` may nest before a query is refused.
///
/// A bound rather than a taste: this parser's input is generated text, and
/// nesting is the one thing in the grammar that turns a long string into
/// deep recursion. Sixty-four levels is far past anything a constraint
/// wants and far short of any stack. `AND`/`OR` chains are flattened and do
/// not count against it.
pub const MAX_DEPTH: u32 = 64;

/// A parsed constraint query: a three-valued boolean expression over the
/// offer's properties plus an optional `ORDER BY`. Build one with
/// [`Query::parse`], run it with [`Query::select`].
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    expr: Expr,
    order: Option<(Field, Direction)>,
}

/// The expression tree. `And`/`Or` are n-ary because a chain of either is
/// flat in the grammar and associative in the logic, which is also what
/// keeps [`Expr::eval`] from recursing once per conjunct — the depth of
/// this tree is bounded by [`MAX_DEPTH`], its width is not.
#[derive(Debug, Clone, PartialEq)]
enum Expr {
    Compare(Comparison),
    /// `EXIST field` — the two-valued question, see the module docs.
    Exist(Field),
    Not(Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
}

impl Expr {
    fn eval(&self, offer: &Offer) -> Truth {
        match self {
            Expr::Compare(c) => c.holds(offer),
            Expr::Exist(field) => {
                if has_value(offer, *field) {
                    Truth::Yes
                } else {
                    Truth::No
                }
            }
            Expr::Not(inner) => inner.eval(offer).not(),
            Expr::And(parts) => {
                parts.iter().fold(Truth::Yes, |acc, part| acc.and(part.eval(offer)))
            }
            Expr::Or(parts) => parts.iter().fold(Truth::No, |acc, part| acc.or(part.eval(offer))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Field {
    Id,
    Specialization,
    Cost,
    LatencyP50,
    LatencyP99,
    Load,
    Residency,
    MemFootprint,
    PlacementNode,
    RouteFreq,
}

pub(crate) const FIELD_LIST: &str = "id, specialization, cost, latency_p50, latency_p99, load, residency, \
                          mem_footprint, placement_node, route_freq";

impl Field {
    pub(crate) fn from_name(name: &str) -> Option<Field> {
        Some(match name {
            "id" => Field::Id,
            "specialization" => Field::Specialization,
            "cost" => Field::Cost,
            "latency_p50" => Field::LatencyP50,
            "latency_p99" => Field::LatencyP99,
            "load" => Field::Load,
            "residency" => Field::Residency,
            "mem_footprint" => Field::MemFootprint,
            "placement_node" => Field::PlacementNode,
            "route_freq" => Field::RouteFreq,
            _ => return None,
        })
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Field::Id => "id",
            Field::Specialization => "specialization",
            Field::Cost => "cost",
            Field::LatencyP50 => "latency_p50",
            Field::LatencyP99 => "latency_p99",
            Field::Load => "load",
            Field::Residency => "residency",
            Field::MemFootprint => "mem_footprint",
            Field::PlacementNode => "placement_node",
            Field::RouteFreq => "route_freq",
        }
    }

    pub(crate) fn kind(self) -> Kind {
        match self {
            Field::Id | Field::Specialization | Field::PlacementNode => Kind::Text,
            Field::Cost | Field::LatencyP50 | Field::LatencyP99 | Field::Load => Kind::Float,
            Field::MemFootprint | Field::RouteFreq => Kind::Counter,
            Field::Residency => Kind::State,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Text,
    Float,
    Counter,
    State,
}

#[derive(Debug, Clone, PartialEq)]
enum Literal {
    Text(String),
    Float(f64),
    Counter(u64),
    State(Residency),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    pub(crate) fn text(self) -> &'static str {
        match self {
            CmpOp::Eq => "==",
            CmpOp::Ne => "!=",
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
        }
    }

    fn holds(self, ord: Ordering) -> bool {
        match self {
            CmpOp::Eq => ord == Ordering::Equal,
            CmpOp::Ne => ord != Ordering::Equal,
            CmpOp::Lt => ord == Ordering::Less,
            CmpOp::Le => ord != Ordering::Greater,
            CmpOp::Gt => ord == Ordering::Greater,
            CmpOp::Ge => ord != Ordering::Less,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Comparison {
    field: Field,
    op: CmpOp,
    literal: Literal,
}

/// The answer to a comparison, which is not a boolean.
///
/// A field the offer's source could not populate makes the comparison
/// **unanswerable**, and collapsing that into `false` is what made the gap
/// silent: an unanswerable query and an answered-no query produced the same
/// empty result, so an operator could not tell "no expert does maths" from
/// "nobody said what these experts do".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truth {
    /// The offer satisfies the comparison.
    Yes,
    /// The offer does not satisfy it.
    No,
    /// The offer does not carry the field, so nothing can be concluded.
    Unknown,
}

impl Truth {
    /// Conjunction in three values: any `No` decides, otherwise any `Unknown`
    /// does. An offer that is `Unknown` for one conjunct and `No` for another
    /// is genuinely `No` — the missing field would not have saved it.
    ///
    /// | `AND` | Yes | No | Unknown |
    /// |---|---|---|---|
    /// | **Yes** | Yes | No | Unknown |
    /// | **No** | No | No | **No** |
    /// | **Unknown** | Unknown | **No** | Unknown |
    pub fn and(self, other: Truth) -> Truth {
        match (self, other) {
            (Truth::No, _) | (_, Truth::No) => Truth::No,
            (Truth::Unknown, _) | (_, Truth::Unknown) => Truth::Unknown,
            _ => Truth::Yes,
        }
    }

    /// Disjunction in three values, the mirror of [`Truth::and`]: any `Yes`
    /// decides, otherwise any `Unknown` does. `Unknown OR Yes` is **`Yes`** —
    /// the disjunct that answered is enough, and nothing the missing field
    /// could have said would change it.
    ///
    /// | `OR` | Yes | No | Unknown |
    /// |---|---|---|---|
    /// | **Yes** | Yes | Yes | **Yes** |
    /// | **No** | Yes | No | Unknown |
    /// | **Unknown** | **Yes** | Unknown | Unknown |
    pub fn or(self, other: Truth) -> Truth {
        match (self, other) {
            (Truth::Yes, _) | (_, Truth::Yes) => Truth::Yes,
            (Truth::Unknown, _) | (_, Truth::Unknown) => Truth::Unknown,
            _ => Truth::No,
        }
    }

    /// Negation: `Yes` and `No` swap, and **`NOT Unknown` is `Unknown`**.
    ///
    /// This is the one entry in the three tables that is a decision rather
    /// than an obvious reading, and it is the one that makes three-valued
    /// logic differ observably from "a field nobody carries just does not
    /// match". Under that two-valued reading `NOT specialization == 'math'`
    /// **returns** an offer whose specialization nobody ever recorded —
    /// which is precisely the failure [`Truth`] exists to prevent, arriving
    /// through the new operator instead of the old one. Here it stays
    /// `Unknown`, is reported in [`Selection::unanswerable`], and the caller
    /// who genuinely wants "offers that are not recorded as maths" writes
    /// the question it actually is: `NOT EXIST specialization OR
    /// specialization != 'math'`.
    ///
    /// **Chosen, not cited.** These three tables are Kleene's strong
    /// three-valued logic, which is also SQL's for `AND`/`OR`/`NOT`. The OMG
    /// Trader Constraint Language is defined in the *Trading Object Service*
    /// specification, a **separate document** from *CORBA — Part 1:
    /// Interfaces v3.4*; the copy of Part 1 available to this batch contains
    /// no TCL grammar and no statement about a constraint over a property an
    /// offer does not carry (its only mention of trading is the
    /// `TradingService` initial-reference row). So no normative sentence was
    /// read for this behaviour and none is quoted for it. The semantics
    /// above were chosen on the engine's own prior grounds — the MoE
    /// contract's unpopulated fields, PLAN-MOE §4.5 — and, for the
    /// `AND`/`OR` fragment, they agree with the two-valued reading anyway:
    /// with monotone connectives only, an expression is `Yes` here exactly
    /// when it is true with every unknown replaced by false. `NOT` is where
    /// the two part company, which is why that is the sentence to check
    /// against the Trading specification when a copy of it is in hand.
    // Deliberately not `std::ops::Not`: `!truth` is the two-valued spelling,
    // and a reader who sees it will read a two-valued negation — which is
    // the confusion this whole type exists to prevent.
    #[allow(clippy::should_implement_trait)]
    pub fn not(self) -> Truth {
        match self {
            Truth::Yes => Truth::No,
            Truth::No => Truth::Yes,
            Truth::Unknown => Truth::Unknown,
        }
    }
}

impl Comparison {
    /// Whether the offer satisfies this comparison. The literal's variant
    /// always matches the field's kind — the parser refused anything else.
    fn holds(&self, offer: &Offer) -> Truth {
        let ord = match &self.literal {
            Literal::Text(s) => match text_value(offer, self.field) {
                Some(v) => v.cmp(s.as_str()),
                None => return Truth::Unknown,
            },
            Literal::Float(x) => match float_value(offer, self.field) {
                Some(v) => v.total_cmp(x),
                None => return Truth::Unknown,
            },
            Literal::Counter(n) => counter_value(offer, self.field).cmp(n),
            Literal::State(r) => offer.residency.cmp(r),
        };
        if self.op.holds(ord) { Truth::Yes } else { Truth::No }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Asc,
    Desc,
}

fn text_value(offer: &Offer, field: Field) -> Option<&str> {
    match field {
        Field::Id => Some(&offer.id),
        Field::Specialization => offer.specialization.as_deref(),
        Field::PlacementNode => Some(&offer.placement_node),
        _ => unreachable!("the parser only pairs text literals with text fields"),
    }
}

fn float_value(offer: &Offer, field: Field) -> Option<f64> {
    match field {
        Field::Cost => Some(offer.cost),
        Field::LatencyP50 => offer.latency_p50,
        Field::LatencyP99 => Some(offer.latency_p99),
        Field::Load => Some(offer.load),
        _ => unreachable!("the parser only pairs float literals with float fields"),
    }
}

fn counter_value(offer: &Offer, field: Field) -> u64 {
    match field {
        Field::MemFootprint => offer.mem_footprint,
        Field::RouteFreq => offer.route_freq,
        _ => unreachable!("the parser only pairs counter literals with counter fields"),
    }
}

/// Whether `offer` carries a value for `field` at all.
///
/// Two callers, one notion. `ORDER BY` has always had to ask this before it
/// could place an offer; since D022 T1 `EXIST field` asks the same function
/// so that the constraint language and the ordering agree by construction
/// about what "present" means, rather than by two people writing the same
/// predicate twice.
///
/// Counters and residency are always known; the two text/float fields a
/// v1.0 wire registration cannot carry (`specialization`, `latency_p50`)
/// are the only ones that can answer `false`, which is why `EXIST
/// route_freq` is a constant `Yes` and not a filter.
pub(crate) fn has_value(offer: &Offer, field: Field) -> bool {
    match field.kind() {
        Kind::Text => text_value(offer, field).is_some(),
        Kind::Float => float_value(offer, field).is_some(),
        Kind::Counter | Kind::State => true,
    }
}

/// Compares two offers on `field`, for `ORDER BY`. Floats compare by IEEE
/// total order — deterministic even for the values nobody should register.
///
/// Both offers are known to carry the field: [`Query::select_reporting`]
/// puts an offer with no value for the ordering key into
/// [`Selection::unranked`] before anything is sorted, so this never has to
/// decide where an unknown goes. It used to — unknown sorted *after* every
/// known value — and that was the right answer to the wrong question: it kept
/// an unmeasured offer from being *first*, but when every candidate was
/// unmeasured the "fastest" was still one of them, and a router taking the
/// head of the list preferred an expert nobody had timed. An offer that
/// cannot be placed is not in the ordered answer at all.
pub(crate) fn offer_cmp(a: &Offer, b: &Offer, field: Field) -> Ordering {
    match field.kind() {
        Kind::Text => match (text_value(a, field), text_value(b, field)) {
            (Some(x), Some(y)) => x.cmp(y),
            _ => unreachable!("unranked offers are set aside before sorting"),
        },
        Kind::Float => match (float_value(a, field), float_value(b, field)) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            _ => unreachable!("unranked offers are set aside before sorting"),
        },
        Kind::Counter => counter_value(a, field).cmp(&counter_value(b, field)),
        Kind::State => a.residency.cmp(&b.residency),
    }
}

// ---- lexer ----

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Tok {
    Ident(String),
    Number(String),
    Str(String),
    Op(CmpOp),
    Open,
    Close,
    End,
}

pub(crate) fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Ident(s) => format!("identifier {s:?}"),
        Tok::Number(s) => format!("number {s}"),
        Tok::Str(s) => format!("string '{s}'"),
        Tok::Op(op) => format!("'{}'", op.text()),
        Tok::Open => "'('".to_owned(),
        Tok::Close => "')'".to_owned(),
        Tok::End => "the end of the query".to_owned(),
    }
}

/// The keywords, so that a lowercase one is refused as a keyword rather than
/// reported as an unknown field. `not` and `exist` made this necessary:
/// before them, every lowercase keyword happened to appear where the parser
/// was already expecting `AND` or `ORDER`, so the "keywords are uppercase"
/// hint was reached by luck rather than by design.
pub(crate) const KEYWORDS: [&str; 8] = ["AND", "OR", "NOT", "EXIST", "ORDER", "BY", "ASC", "DESC"];

pub(crate) fn lex(text: &str) -> Result<Vec<(usize, Tok)>, ParseError> {
    let bytes = text.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'\'' => {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && bytes[j] != b'\'' {
                    j += 1;
                }
                if j == bytes.len() {
                    return Err(ParseError {
                        at: i,
                        message: "unterminated string: expected a closing '".to_owned(),
                    });
                }
                toks.push((i, Tok::Str(text[start..j].to_owned())));
                i = j + 1;
            }
            b'=' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    toks.push((i, Tok::Op(CmpOp::Eq)));
                    i += 2;
                } else {
                    return Err(ParseError {
                        at: i,
                        message: "expected '==' (a single '=' is not an operator)".to_owned(),
                    });
                }
            }
            b'!' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    toks.push((i, Tok::Op(CmpOp::Ne)));
                    i += 2;
                } else {
                    return Err(ParseError {
                        at: i,
                        message: "expected '!=' ('!' alone is not an operator; negation is \
                                  written 'NOT')"
                            .to_owned(),
                    });
                }
            }
            b'(' => {
                toks.push((i, Tok::Open));
                i += 1;
            }
            b')' => {
                toks.push((i, Tok::Close));
                i += 1;
            }
            b'<' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    toks.push((i, Tok::Op(CmpOp::Le)));
                    i += 2;
                } else {
                    toks.push((i, Tok::Op(CmpOp::Lt)));
                    i += 1;
                }
            }
            b'>' => {
                if bytes.get(i + 1) == Some(&b'=') {
                    toks.push((i, Tok::Op(CmpOp::Ge)));
                    i += 2;
                } else {
                    toks.push((i, Tok::Op(CmpOp::Gt)));
                    i += 1;
                }
            }
            b'0'..=b'9' | b'-' => {
                let start = i;
                if bytes[i] == b'-' {
                    i += 1;
                }
                let digits_start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i == digits_start {
                    return Err(ParseError {
                        at: start,
                        message: "malformed number: expected digits after '-'".to_owned(),
                    });
                }
                if i < bytes.len() && bytes[i] == b'.' {
                    let dot = i;
                    i += 1;
                    let frac_start = i;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    if i == frac_start {
                        return Err(ParseError {
                            at: dot,
                            message: "malformed number: expected digits after '.'".to_owned(),
                        });
                    }
                }
                toks.push((start, Tok::Number(text[start..i].to_owned())));
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                toks.push((start, Tok::Ident(text[start..i].to_owned())));
            }
            _ => {
                let ch = text[i..].chars().next().unwrap_or('?');
                // Named an expectation as of D022 T1: this refusal used to
                // give a position and nothing to fix, which is half the bar
                // the module's own docs set for every other refusal here.
                return Err(ParseError {
                    at: i,
                    message: format!(
                        "unexpected character {ch:?}: expected a field name, a keyword \
                         ({}), a comparison operator (one of == != < <= > >=), a number, \
                         a quoted string, '(' or ')'",
                        KEYWORDS.join(" ")
                    ),
                });
            }
        }
    }
    toks.push((text.len(), Tok::End));
    Ok(toks)
}

// ---- parser ----

struct Parser {
    toks: Vec<(usize, Tok)>,
    i: usize,
    depth: u32,
}

impl Parser {
    fn bump(&mut self) -> (usize, Tok) {
        let t = self.toks[self.i].clone();
        if self.i + 1 < self.toks.len() {
            self.i += 1;
        }
        t
    }

    /// The current token without consuming it. `Tok::End` repeats for ever,
    /// which is what lets every caller peek without a bounds check.
    fn peek(&self) -> (usize, Tok) {
        self.toks[self.i].clone()
    }

    /// Whether the current token is exactly this keyword.
    fn at_keyword(&self, kw: &str) -> bool {
        matches!(&self.toks[self.i].1, Tok::Ident(s) if s == kw)
    }

    fn err<T>(&self, at: usize, message: String) -> Result<T, ParseError> {
        Err(ParseError { at, message })
    }

    /// One nesting level of `(` or `NOT`, refused by position and limit when
    /// it goes past [`MAX_DEPTH`].
    fn enter(&mut self, at: usize, what: &str) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return self.err(
                at,
                format!(
                    "this {what} nests more than {MAX_DEPTH} levels deep: flatten the query \
                     (a nesting limit, so that a generated string cannot become a stack \
                     overflow instead of a refusal)"
                ),
            );
        }
        Ok(())
    }

    fn parse_field(&mut self) -> Result<Field, ParseError> {
        let (at, tok) = self.bump();
        match tok {
            Tok::Ident(name) => match Field::from_name(&name) {
                Some(f) => Ok(f),
                None => {
                    let upper = name.to_ascii_uppercase();
                    if !KEYWORDS.contains(&upper.as_str()) {
                        self.err(at, format!("unknown field {name:?}: fields are {FIELD_LIST}"))
                    } else if upper == name {
                        // Already uppercase: it is a keyword in a place no
                        // keyword belongs, so "keywords are uppercase" would
                        // be advice they have already taken.
                        self.err(
                            at,
                            format!(
                                "expected a field name, found the keyword {name:?}: fields \
                                 are {FIELD_LIST}"
                            ),
                        )
                    } else {
                        self.err(
                            at,
                            format!(
                                "expected a field name, found the keyword {name:?}: keywords \
                                 are uppercase — write '{upper}'"
                            ),
                        )
                    }
                }
            },
            other => self.err(at, format!("expected a field name, found {}", describe(&other))),
        }
    }

    // ---- the expression grammar, loosest binding first ----

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        let mut parts = vec![self.parse_conjunction()?];
        while self.at_keyword("OR") {
            self.bump();
            parts.push(self.parse_conjunction()?);
        }
        Ok(if parts.len() == 1 { parts.pop().expect("just checked") } else { Expr::Or(parts) })
    }

    fn parse_conjunction(&mut self) -> Result<Expr, ParseError> {
        let mut parts = vec![self.parse_negation()?];
        while self.at_keyword("AND") {
            self.bump();
            parts.push(self.parse_negation()?);
        }
        Ok(if parts.len() == 1 { parts.pop().expect("just checked") } else { Expr::And(parts) })
    }

    fn parse_negation(&mut self) -> Result<Expr, ParseError> {
        if self.at_keyword("NOT") {
            let (at, _) = self.bump();
            self.enter(at, "'NOT'")?;
            let inner = self.parse_negation()?;
            self.depth -= 1;
            return Ok(Expr::Not(Box::new(inner)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let (at, tok) = self.peek();
        match tok {
            Tok::Open => {
                self.bump();
                self.enter(at, "'('")?;
                let inner = self.parse_expr()?;
                self.depth -= 1;
                let (close_at, close) = self.bump();
                if close != Tok::Close {
                    return self.err(
                        close_at,
                        format!(
                            "expected ')' to close the '(' at byte {at}, found {}",
                            describe(&close)
                        ),
                    );
                }
                Ok(inner)
            }
            Tok::Ident(ref s) if s == "EXIST" => {
                self.bump();
                Ok(Expr::Exist(self.parse_field()?))
            }
            Tok::Ident(_) => Ok(Expr::Compare(self.parse_comparison()?)),
            other => self.err(
                at,
                format!(
                    "expected a condition here — a field name, 'NOT', 'EXIST' or '(' — \
                     found {}",
                    describe(&other)
                ),
            ),
        }
    }

    fn parse_comparison(&mut self) -> Result<Comparison, ParseError> {
        let field = self.parse_field()?;
        let (at, tok) = self.bump();
        let op = match tok {
            Tok::Op(op) => op,
            other => {
                return self.err(
                    at,
                    format!(
                        "expected a comparison operator (one of == != < <= > >=) after {}, \
                         found {}",
                        field.name(),
                        describe(&other)
                    ),
                );
            }
        };
        let literal = self.parse_literal(field)?;
        Ok(Comparison { field, op, literal })
    }

    fn parse_literal(&mut self, field: Field) -> Result<Literal, ParseError> {
        let (at, tok) = self.bump();
        match (field.kind(), tok) {
            (Kind::Text, Tok::Str(s)) => Ok(Literal::Text(s)),
            (Kind::Text, other) => self.err(
                at,
                format!(
                    "expected a quoted string (like 'math') for {}, found {}",
                    field.name(),
                    describe(&other)
                ),
            ),
            // The overflow arm is not decoration. `f64::from_str` answers
            // `Ok(inf)` for a number too large to represent rather than an
            // error, so before D022 T1 a query with four hundred digits in
            // it parsed to `latency_p99 < inf` — a bound that matches
            // everything, arrived at silently. The counter path next door
            // had always refused its overflow by name; this one now does
            // too.
            (Kind::Float, Tok::Number(n)) => match n.parse::<f64>() {
                Ok(x) if x.is_finite() => Ok(Literal::Float(x)),
                Ok(_) => self.err(
                    at,
                    format!(
                        "number {n} is too large for the 64-bit float {} carries: write a \
                         value it can represent",
                        field.name()
                    ),
                ),
                Err(_) => self.err(
                    at,
                    format!(
                        "malformed number {n}: expected digits, optionally signed and \
                         optionally with one fractional part (like -1.5)"
                    ),
                ),
            },
            (Kind::Float, other) => self.err(
                at,
                format!("expected a number for {}, found {}", field.name(), describe(&other)),
            ),
            (Kind::Counter, Tok::Number(n)) => {
                if n.starts_with('-') || n.contains('.') {
                    return self.err(
                        at,
                        format!(
                            "expected a non-negative integer for {} (counter fields carry no \
                             fractions), found number {n}",
                            field.name()
                        ),
                    );
                }
                match n.parse::<u64>() {
                    Ok(v) => Ok(Literal::Counter(v)),
                    Err(_) => self.err(at, format!("number {n} does not fit in 64 bits")),
                }
            }
            (Kind::Counter, other) => self.err(
                at,
                format!(
                    "expected a non-negative integer for {}, found {}",
                    field.name(),
                    describe(&other)
                ),
            ),
            (Kind::State, Tok::Ident(name)) => match name.as_str() {
                "OFFLOADED" => Ok(Literal::State(Residency::Offloaded)),
                "PREFETCHING" => Ok(Literal::State(Residency::Prefetching)),
                "RESIDENT" => Ok(Literal::State(Residency::Resident)),
                "ACTIVE" => Ok(Literal::State(Residency::Active)),
                _ => self.err(
                    at,
                    format!(
                        "expected a residency (OFFLOADED, PREFETCHING, RESIDENT or ACTIVE), \
                         found identifier {name:?}"
                    ),
                ),
            },
            (Kind::State, other) => self.err(
                at,
                format!(
                    "expected a residency (OFFLOADED, PREFETCHING, RESIDENT or ACTIVE), \
                     found {}",
                    describe(&other)
                ),
            ),
        }
    }

    fn parse_order(&mut self) -> Result<(Field, Direction), ParseError> {
        // "ORDER" is already consumed.
        let (at, tok) = self.bump();
        match tok {
            Tok::Ident(ref s) if s == "BY" => {}
            other => {
                return self
                    .err(at, format!("expected 'BY' after 'ORDER', found {}", describe(&other)));
            }
        }
        let field = self.parse_field()?;
        let (at, tok) = self.bump();
        let direction = match tok {
            Tok::Ident(ref s) if s == "ASC" => Direction::Asc,
            Tok::Ident(ref s) if s == "DESC" => Direction::Desc,
            other => {
                return self.err(
                    at,
                    format!(
                        "expected 'ASC' or 'DESC' after 'ORDER BY {}', found {}",
                        field.name(),
                        describe(&other)
                    ),
                );
            }
        };
        Ok((field, direction))
    }
}

impl Query {
    /// Parses a query. Refusals name the byte position and what was
    /// expected there — see [`ParseError`].
    pub fn parse(text: &str) -> Result<Query, ParseError> {
        Query::parse_inner(text, true)
    }

    /// The constraint language alone, with no `ORDER BY` accepted.
    ///
    /// `WITH <constraint>` inside a [`crate::preference::Preference`] reads
    /// a constraint that is not a whole query: the preference *is* the
    /// ordering there, so an `ORDER BY` inside it would be a second one, and
    /// it is refused at the position of the word rather than ignored.
    pub(crate) fn parse_constraint(text: &str) -> Result<Query, ParseError> {
        Query::parse_inner(text, false)
    }

    /// Whether this query carries an `ORDER BY` clause of its own.
    pub(crate) fn has_order(&self) -> bool {
        self.order.is_some()
    }

    fn parse_inner(text: &str, allow_order: bool) -> Result<Query, ParseError> {
        let mut p = Parser { toks: lex(text)?, i: 0, depth: 0 };
        let expr = p.parse_expr()?;
        let mut order = None;
        let (at, tok) = p.bump();
        match tok {
            Tok::Ident(ref s) if s == "ORDER" && !allow_order => {
                return Err(ParseError {
                    at,
                    message: "'ORDER BY' is not allowed here: this constraint is being read \
                              as part of a preference, and the preference is the ordering"
                        .to_owned(),
                });
            }
            Tok::Ident(ref s) if s == "ORDER" => {
                order = Some(p.parse_order()?);
                let (at, tok) = p.bump();
                if tok != Tok::End {
                    return Err(ParseError {
                        at,
                        message: format!(
                            "expected the end of the query after the ORDER BY clause, found {}",
                            describe(&tok)
                        ),
                    });
                }
            }
            Tok::End => {}
            other => {
                return Err(ParseError {
                    at,
                    message: format!(
                        "expected 'AND', 'OR', 'ORDER BY' or the end of the query, found {} \
                         (keywords are uppercase)",
                        describe(&other)
                    ),
                });
            }
        }
        Ok(Query { expr, order })
    }

    /// Whether `offer` satisfies the constraint, in three values.
    pub fn evaluate(&self, offer: &Offer) -> Truth {
        self.expr.eval(offer)
    }

    /// Whether `offer` definitely satisfies the constraint.
    ///
    /// `Unknown` answers `false` here, which is safe — an offer that might
    /// match is not a match — but a caller that only asks this question cannot
    /// distinguish "no" from "cannot tell", so [`Query::select`] reports the
    /// two separately and the servant path says so out loud.
    pub fn matches(&self, offer: &Offer) -> bool {
        self.evaluate(offer) == Truth::Yes
    }

    /// Every matching offer, in the pinned order: the `ORDER BY` field and
    /// direction with ties broken by ascending id, or plain ascending id
    /// when no ordering was asked for. The same store and query always
    /// return the same list in the same order.
    ///
    /// Lossy by construction: the offers the query could not judge, and the
    /// ones it could not place in the requested order, are not in this list
    /// and nothing here says so. A caller that must tell "no" from "cannot
    /// tell" — every router — uses [`Query::select_reporting`].
    pub fn select<'a>(&self, store: &'a OfferStore) -> Vec<&'a Offer> {
        self.select_reporting(store).matched
    }

    /// [`Query::select`], plus the offers the query could not answer for and
    /// the ones it could not rank.
    ///
    /// The reason this exists rather than a plain `Vec`: an offer registered
    /// over the wire's v1.0 path carries no `specialization` and no
    /// `latency_p50`, because `moe::Capability` has no member for either.
    /// Folding those into "did not match" made the two indistinguishable, and
    /// one of them is a question for whoever registered the expert rather
    /// than an answer about maths. Ordering has the same shape of gap: an
    /// `ORDER BY latency_p50` cannot place an offer that has none, and a list
    /// that quietly put it last — or first, when nothing else was measured —
    /// was a ranking that had not been earned.
    pub fn select_reporting<'a>(&self, store: &'a OfferStore) -> Selection<'a> {
        let mut matched: Vec<&Offer> = Vec::new();
        let mut unanswerable: Vec<&Offer> = Vec::new();
        let mut unranked: Vec<&Offer> = Vec::new();
        for offer in store.iter() {
            match self.evaluate(offer) {
                Truth::Yes => match self.order {
                    Some((field, _)) if !has_value(offer, field) => unranked.push(offer),
                    _ => matched.push(offer),
                },
                Truth::Unknown => unanswerable.push(offer),
                Truth::No => {}
            }
        }
        let mut out = matched;
        if let Some((field, direction)) = self.order {
            out.sort_by(|a, b| {
                let ord = offer_cmp(a, b, field);
                let ord = match direction {
                    Direction::Asc => ord,
                    Direction::Desc => ord.reverse(),
                };
                ord.then_with(|| a.id.cmp(&b.id))
            });
        }
        unanswerable.sort_by(|a, b| a.id.cmp(&b.id));
        unranked.sort_by(|a, b| a.id.cmp(&b.id));
        Selection { matched: out, unanswerable, unranked }
    }

    /// [`Query::select_reporting`], ordered by a
    /// [`Preference`](crate::preference::Preference) instead of by this
    /// query's own `ORDER BY` — D022 T2, the standard's second expression
    /// language.
    ///
    /// The three buckets keep exactly their meaning. **The constraint
    /// decides membership and the preference decides order**, and they ask
    /// different questions of the same offer: an offer the constraint
    /// answered [`Truth::Yes`] for can still be one the preference cannot
    /// place, and it lands in [`Selection::unranked`] — the same bucket, for
    /// the same reason, as an offer with no value for an `ORDER BY` field.
    ///
    /// # Two orderings is one too many
    ///
    /// A query that carries its own `ORDER BY` is **refused** here rather
    /// than having one of the two silently win. `ORDER BY` did not go away —
    /// it is still how our own callers order, and the MoE contract's queries
    /// use it — so a caller holding both has written two answers to one
    /// question, and picking for them is the sort of quiet choice this
    /// workspace records instead of making.
    pub fn select_preferring<'a>(
        &self,
        store: &'a OfferStore,
        preference: &Preference,
    ) -> Result<Selection<'a>, PreferenceError> {
        if self.has_order() {
            return Err(PreferenceError {
                message: format!(
                    "this query carries its own 'ORDER BY' and was also given the preference \
                     {preference}: that is two orderings for one answer. Drop the ORDER BY \
                     to order by the preference, or use select_reporting to order by the \
                     ORDER BY."
                ),
            });
        }
        let mut candidates: Vec<&Offer> = Vec::new();
        let mut unanswerable: Vec<&Offer> = Vec::new();
        let mut unranked: Vec<&Offer> = Vec::new();
        for offer in store.iter() {
            match self.evaluate(offer) {
                Truth::Yes => {
                    if preference.can_place(offer) {
                        candidates.push(offer);
                    } else {
                        unranked.push(offer);
                    }
                }
                Truth::Unknown => unanswerable.push(offer),
                Truth::No => {}
            }
        }
        // Ties break by ascending id in both languages, so the order is
        // pinned whatever the preference says — including `FIRST`, which
        // says nothing and therefore gets the store order.
        candidates.sort_by(|a, b| preference.rank(a, b).then_with(|| a.id.cmp(&b.id)));
        unanswerable.sort_by(|a, b| a.id.cmp(&b.id));
        unranked.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(Selection { matched: candidates, unanswerable, unranked })
    }
}

/// What a query could answer, what it could not, and what it could not
/// place.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection<'a> {
    /// Offers the constraint answered [`Truth::Yes`] for **and** that carry
    /// the `ORDER BY` field, in the pinned order.
    pub matched: Vec<&'a Offer>,
    /// Offers the constraint answered [`Truth::Unknown`] for, in ascending
    /// id. Never silently dropped: an empty `matched` beside a non-empty
    /// `unanswerable` is a different situation from an empty `matched`
    /// alone, and only the second one means "nothing qualifies".
    ///
    /// *Not* "offers missing a field the query names" — that was the whole
    /// criterion while the grammar was a chain of `AND`s, and D022 T1 made
    /// it false in both directions. `specialization == 'math' OR
    /// route_freq > 10` answers `Yes` for an offer with no specialization
    /// and a busy counter, because the disjunct that answered is enough; and
    /// `EXIST specialization` names the field and is never unanswerable at
    /// all. The criterion is, and now only is, what [`Query::evaluate`]
    /// returned.
    pub unanswerable: Vec<&'a Offer>,
    /// Offers the constraint answered `Yes` for but that carry no value for the `ORDER
    /// BY` field, in ascending id — they qualify, and nobody can say where
    /// they rank. Empty whenever the query has no ordering, or orders by a
    /// field every offer carries (`route_freq`, `residency`, …). A router
    /// that takes the head of `matched` as "the fastest" while this is
    /// non-empty is preferring the measured over the unmeasured by fiat, and
    /// [`Selection::is_complete`] is how it declines to.
    pub unranked: Vec<&'a Offer>,
}

impl Selection<'_> {
    /// Whether every offer was judged and placed: `matched` is the whole
    /// answer, and nothing was set aside as unanswerable or unranked.
    ///
    /// The router rule in one predicate — *a sequence of references is a
    /// complete answer or it is a refusal.* An offer nobody could judge, or
    /// nobody could rank, might have outranked the ones that came back, so a
    /// consumer that hands `matched` on as "the experts that qualify, best
    /// first" checks this and refuses when it is false. An empty `matched`
    /// with this `true` is an honest nothing.
    pub fn is_complete(&self) -> bool {
        self.unanswerable.is_empty() && self.unranked.is_empty()
    }

    /// One line an operator can act on, or `None` when everything was
    /// answerable and rankable.
    pub fn gap_note(&self) -> Option<String> {
        if self.is_complete() {
            return None;
        }
        let list =
            |offers: &[&Offer]| offers.iter().map(|o| o.id.as_str()).collect::<Vec<_>>().join(", ");
        let mut parts = Vec::new();
        if !self.unanswerable.is_empty() {
            parts.push(format!(
                "{} offer(s) could not be judged, because a field the constraint had to \
                 read is one their source did not record: {}",
                self.unanswerable.len(),
                list(&self.unanswerable)
            ));
        }
        if !self.unranked.is_empty() {
            parts.push(format!(
                "{} offer(s) qualify but carry no value for the ORDER BY field, so they \
                 cannot be placed: {}",
                self.unranked.len(),
                list(&self.unranked)
            ));
        }
        Some(format!(
            "{}. An expert registered through moe::ExpertRegistry::register_expert (v1.0) \
             has no specialization and no latency_p50, because moe::Capability declares \
             neither; register_measured / heartbeat_measured (v1.1, MeasuredCapability) \
             carry both. A query that must answer over the gap rather than report it \
             guards the field with EXIST (e.g. EXIST specialization AND specialization \
             == 'math'), which is answerable for every offer.",
            parts.join("; ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this three-valued logic exists for, pinned from both sides.
    ///
    /// An offer registered over the wire carries no `latency_p50`, and as a
    /// `0.0` placeholder it did not fail to match `latency_p50 < 20` — it
    /// **matched**, so a router selecting on latency preferred exactly the
    /// experts nobody had measured. The mirror case is `specialization`,
    /// where the empty-string placeholder produced a silent non-match that
    /// read as "no expert does maths".
    #[test]
    fn an_unknown_field_neither_matches_nor_silently_misses() {
        let mut store = OfferStore::new();
        store
            .register(Offer {
                id: "from-the-wire".to_owned(),
                specialization: None,
                cost: 1.0,
                latency_p50: None,
                latency_p99: 50.0,
                load: 0.1,
                residency: Residency::Resident,
                mem_footprint: 100,
                placement_node: "n1".to_owned(),
                route_freq: 0,
            })
            .expect("registers");

        let fast = Query::parse("latency_p50 < 20").expect("parses");
        let sel = fast.select_reporting(&store);
        assert!(sel.matched.is_empty(), "an unmeasured latency is not a fast one");
        assert_eq!(sel.unanswerable.len(), 1, "and it is reported, not dropped");
        assert!(sel.gap_note().expect("a note").contains("from-the-wire"));

        let maths = Query::parse("specialization == 'math'").expect("parses");
        let sel = maths.select_reporting(&store);
        assert!(sel.matched.is_empty());
        assert_eq!(sel.unanswerable.len(), 1, "'nobody said' is not 'no'");

        // A conjunct the offer definitely fails still decides: the missing
        // field would not have saved it.
        let slow = Query::parse("latency_p99 > 1000 AND latency_p50 < 20").expect("parses");
        let sel = slow.select_reporting(&store);
        assert!(sel.matched.is_empty());
        assert!(sel.unanswerable.is_empty(), "No beats Unknown");
    }

    fn maths_offer(id: &str, p50: Option<f64>) -> Offer {
        Offer {
            id: id.to_owned(),
            specialization: Some("math".to_owned()),
            cost: 1.0,
            latency_p50: p50,
            latency_p99: 100.0,
            load: 0.1,
            residency: Residency::Resident,
            mem_footprint: 100,
            placement_node: "n1".to_owned(),
            route_freq: 0,
        }
    }

    /// Ordering must not let an unknown win either. `ORDER BY latency_p50`
    /// used to put the unmeasured offer last; now it does not place it at
    /// all — it qualifies, it is reported as unranked, and the ordered
    /// answer is incomplete until it is measured.
    #[test]
    fn an_unknown_ordering_key_is_unranked_not_last() {
        let mut store = OfferStore::new();
        for (id, p50) in [("known-slow", Some(90.0)), ("unknown", None), ("known-fast", Some(5.0))]
        {
            store.register(maths_offer(id, p50)).expect("registers");
        }
        let q = Query::parse("specialization == 'math' ORDER BY latency_p50 ASC").expect("parses");
        let sel = q.select_reporting(&store);
        assert_eq!(ids(&sel.matched), ["known-fast", "known-slow"]);
        assert_eq!(ids(&sel.unranked), ["unknown"], "it qualifies; nobody can say where");
        assert!(sel.unanswerable.is_empty(), "no conjunct named the missing field");
        assert!(!sel.is_complete(), "a ranking with a hole in it is not a ranking");
        assert!(sel.gap_note().expect("a note").contains("cannot be placed: unknown"));
        // The lossy form drops it, as documented.
        assert_eq!(ids(&q.select(&store)), ["known-fast", "known-slow"]);
        // Descending too: the unmeasured offer is not "the slowest" either.
        let q = Query::parse("specialization == 'math' ORDER BY latency_p50 DESC").expect("parses");
        let sel = q.select_reporting(&store);
        assert_eq!(ids(&sel.matched), ["known-slow", "known-fast"]);
        assert_eq!(ids(&sel.unranked), ["unknown"]);
    }

    /// The case D010 A2 named: when *nothing* is measured, "the fastest" has
    /// no answer, and the engine says so instead of naming whichever offer
    /// sorted first. Then one measurement arrives (the v1.1 path) and the
    /// answer is complete, and it is the measured one — not the one that
    /// registered first.
    #[test]
    fn a_router_ordering_by_latency_cannot_prefer_an_unmeasured_expert() {
        let mut store = OfferStore::new();
        store.register(maths_offer("expert-math", None)).expect("registers");
        let q = Query::parse("specialization == 'math' ORDER BY latency_p50 ASC").expect("parses");
        let sel = q.select_reporting(&store);
        assert!(sel.matched.is_empty(), "nothing is placed");
        assert_eq!(ids(&sel.unranked), ["expert-math"]);
        assert!(!sel.is_complete(), "one unmeasured candidate is a refusal, not a pick");
        // A second maths expert, measured: still incomplete, because the
        // first one might outrank it — nobody knows.
        store.register(maths_offer("expert-math-b", Some(8.0))).expect("registers");
        let sel = q.select_reporting(&store);
        assert_eq!(ids(&sel.matched), ["expert-math-b"]);
        assert_eq!(ids(&sel.unranked), ["expert-math"]);
        assert!(!sel.is_complete());
        // The measurement arrives, as a heartbeat would carry it.
        store.heartbeat(maths_offer("expert-math", Some(12.0))).expect("updates");
        let sel = q.select_reporting(&store);
        assert_eq!(ids(&sel.matched), ["expert-math-b", "expert-math"]);
        assert!(sel.is_complete());
        assert_eq!(sel.gap_note(), None);
        // And an ordering by a field every offer carries never sets anything
        // aside — the wire `Router::select` orders by `route_freq`.
        let q = Query::parse("specialization == 'math' ORDER BY route_freq DESC").expect("parses");
        store.heartbeat(maths_offer("expert-math", None)).expect("updates");
        let sel = q.select_reporting(&store);
        assert_eq!(sel.matched.len(), 2);
        assert!(sel.is_complete());
    }

    fn store() -> OfferStore {
        let mut s = OfferStore::new();
        for (id, spec, p99, load, node, residency, footprint, freq) in [
            ("expert-a", "math", 120.0, 0.5, "node-a", Residency::Resident, 100u64, 48u64),
            ("expert-b", "math", 250.0, 0.4, "node-b", Residency::Offloaded, 200, 48),
            ("expert-c", "code", 80.0, 0.9, "node-a", Residency::Resident, 50, 96),
            ("expert-d", "math", 150.0, 0.7, "node-c", Residency::Prefetching, 300, 16),
        ] {
            s.register(Offer {
                id: id.to_owned(),
                specialization: Some(spec.to_owned()),
                cost: 1.0,
                latency_p50: Some(p99 / 2.0),
                latency_p99: p99,
                load,
                residency,
                mem_footprint: footprint,
                placement_node: node.to_owned(),
                route_freq: freq,
            })
            .expect("fixture registers");
        }
        s
    }

    fn ids(offers: &[&Offer]) -> Vec<String> {
        offers.iter().map(|o| o.id.clone()).collect()
    }

    /// §4.3's own example, adapted to the offer fields: conjunction, float
    /// comparisons, and a descending order.
    #[test]
    fn the_architecture_documents_example_query_selects_and_orders() {
        let s = store();
        let q = Query::parse(
            "specialization == 'math' AND latency_p99 < 200 AND load < 0.8 \
             ORDER BY route_freq DESC",
        )
        .expect("parses");
        // expert-b: p99 250 too slow. expert-c: code. Ties: a and d differ in freq.
        assert_eq!(ids(&q.select(&s)), ["expert-a", "expert-d"]);
    }

    #[test]
    fn every_operator_holds_on_a_counter_field() {
        let s = store();
        for (query, expect) in [
            ("mem_footprint == 200", vec!["expert-b"]),
            ("mem_footprint != 200", vec!["expert-a", "expert-c", "expert-d"]),
            ("mem_footprint < 100", vec!["expert-c"]),
            ("mem_footprint <= 100", vec!["expert-a", "expert-c"]),
            ("mem_footprint > 200", vec!["expert-d"]),
            ("mem_footprint >= 200", vec!["expert-b", "expert-d"]),
        ] {
            let q = Query::parse(query).expect(query);
            assert_eq!(ids(&q.select(&s)), *expect, "{query}");
        }
    }

    #[test]
    fn string_fields_compare_lexicographically() {
        let s = store();
        let q = Query::parse("placement_node < 'node-b'").expect("parses");
        assert_eq!(ids(&q.select(&s)), ["expert-a", "expert-c"]);
        let q = Query::parse("id >= 'expert-c'").expect("parses");
        assert_eq!(ids(&q.select(&s)), ["expert-c", "expert-d"]);
    }

    /// Residency literals are bare enum names, ordered by the loading
    /// progression: `< RESIDENT` means "not yet callable".
    #[test]
    fn residency_compares_by_loading_progression() {
        let s = store();
        let q = Query::parse("residency == RESIDENT").expect("parses");
        assert_eq!(ids(&q.select(&s)), ["expert-a", "expert-c"]);
        let q = Query::parse("residency < RESIDENT").expect("parses");
        assert_eq!(ids(&q.select(&s)), ["expert-b", "expert-d"]);
    }

    /// Without ORDER BY the result order is still pinned: ascending id.
    #[test]
    fn the_default_order_is_ascending_id() {
        let s = store();
        let q = Query::parse("cost == 1.0").expect("parses");
        assert_eq!(ids(&q.select(&s)), ["expert-a", "expert-b", "expert-c", "expert-d"]);
    }

    /// An ORDER BY tie is broken by ascending id, both directions — the
    /// pinned order must not depend on sort stability details.
    #[test]
    fn order_by_ties_break_by_ascending_id() {
        let s = store();
        let q = Query::parse("specialization == 'math' ORDER BY route_freq ASC").expect("parses");
        // d(16), then a and b tie at 48 → id order.
        assert_eq!(ids(&q.select(&s)), ["expert-d", "expert-a", "expert-b"]);
        let q = Query::parse("specialization == 'math' ORDER BY route_freq DESC").expect("parses");
        assert_eq!(ids(&q.select(&s)), ["expert-a", "expert-b", "expert-d"]);
    }

    // ---- diagnostics: every refusal names a position and an expectation ----

    #[test]
    fn an_unknown_field_is_named_and_the_field_list_offered() {
        let err = Query::parse("speciality == 'math'").unwrap_err();
        assert_eq!(err.at, 0);
        assert!(err.message.contains("unknown field \"speciality\""), "{err}");
        assert!(err.message.contains("specialization"), "{err}");
    }

    #[test]
    fn a_single_equals_is_refused_at_its_position() {
        let err = Query::parse("load = 0.8").unwrap_err();
        assert_eq!(err.at, 5);
        assert!(err.message.contains("'=='"), "{err}");
    }

    #[test]
    fn an_unterminated_string_points_at_its_opening_quote() {
        let err = Query::parse("specialization == 'math").unwrap_err();
        assert_eq!(err.at, 18);
        assert!(err.message.contains("closing '"), "{err}");
    }

    #[test]
    fn lowercase_keywords_are_refused_with_the_hint() {
        let err = Query::parse("load < 0.8 and cost < 2").unwrap_err();
        assert_eq!(err.at, 11);
        assert!(err.message.contains("keywords are uppercase"), "{err}");
    }

    #[test]
    fn order_by_requires_an_explicit_direction() {
        let err = Query::parse("load < 0.8 ORDER BY load").unwrap_err();
        assert_eq!(err.at, 24);
        assert!(err.message.contains("expected 'ASC' or 'DESC'"), "{err}");
    }

    #[test]
    fn literal_types_are_checked_against_the_field_at_parse_time() {
        // A quoted string where a float belongs.
        let err = Query::parse("latency_p99 < '200'").unwrap_err();
        assert_eq!(err.at, 14);
        assert!(err.message.contains("expected a number for latency_p99"), "{err}");
        // A fraction where a counter belongs.
        let err = Query::parse("mem_footprint < 1.5").unwrap_err();
        assert_eq!(err.at, 16);
        assert!(err.message.contains("non-negative integer for mem_footprint"), "{err}");
        // A quoted string where a residency belongs.
        let err = Query::parse("residency == 'RESIDENT'").unwrap_err();
        assert_eq!(err.at, 13);
        assert!(err.message.contains("OFFLOADED, PREFETCHING, RESIDENT or ACTIVE"), "{err}");
    }

    #[test]
    fn trailing_tokens_after_order_by_are_refused() {
        let err = Query::parse("load < 0.8 ORDER BY load ASC load").unwrap_err();
        assert_eq!(err.at, 29);
        assert!(err.message.contains("expected the end of the query"), "{err}");
    }

    #[test]
    fn the_ms_suffix_the_prose_uses_is_refused_not_silently_dropped() {
        // §4.3 writes `latency_p99 < 200ms`; the grammar takes bare numbers.
        let err = Query::parse("latency_p99 < 200ms").unwrap_err();
        assert_eq!(err.at, 17);
        assert!(err.message.contains("expected 'AND'"), "{err}");
    }

    /// A float literal too large to represent used to parse to `inf`, and a
    /// bound of `inf` matches everything — a filter that had silently
    /// stopped filtering. The counter field beside it had always refused
    /// its own overflow by name.
    #[test]
    fn a_float_literal_too_large_to_represent_is_refused_not_rounded_to_infinity() {
        let huge = "9".repeat(400);
        let err = Query::parse(&format!("latency_p99 < {huge}")).unwrap_err();
        assert_eq!(err.at, 14);
        assert!(err.message.contains("too large for the 64-bit float latency_p99"), "{err}");
        // The counter path, unchanged, for the comparison.
        let err = Query::parse(&format!("mem_footprint < {huge}")).unwrap_err();
        assert_eq!(err.at, 16);
        assert!(err.message.contains("does not fit in 64 bits"), "{err}");
    }

    // ---- D022 T1: OR, NOT, parentheses and EXIST ----

    /// The offer the whole three-valued design exists for: registered over
    /// the v1.0 wire path, so `specialization` and `latency_p50` are the two
    /// things nobody recorded. Everything else it carries.
    fn gapped_offer() -> Offer {
        Offer {
            id: "from-the-wire".to_owned(),
            specialization: None,
            cost: 1.0,
            latency_p50: None,
            latency_p99: 50.0,
            load: 0.1,
            residency: Residency::Resident,
            mem_footprint: 100,
            placement_node: "n1".to_owned(),
            route_freq: 0,
        }
    }

    /// The truth table as a table, over the offer with two unrecorded
    /// fields — **every** unanswerable case the grammar can produce, not a
    /// sample of them. Kleene's strong three-valued logic; see
    /// [`Truth::not`] for why that is a choice made here rather than a
    /// sentence quoted from the Trading specification.
    #[test]
    fn tcl_expressions_answer_the_three_valued_table_over_an_offer_with_gaps() {
        let offer = gapped_offer();
        for (text, expect) in [
            // Two-valued ground, unchanged from the old grammar.
            ("cost == 1.0", Truth::Yes),
            ("cost == 2.0", Truth::No),
            // A field the source never recorded.
            ("specialization == 'math'", Truth::Unknown),
            ("latency_p50 < 20", Truth::Unknown),
            // NOT: swaps the two, and leaves the third alone.
            ("NOT cost == 1.0", Truth::No),
            ("NOT cost == 2.0", Truth::Yes),
            ("NOT specialization == 'math'", Truth::Unknown),
            ("NOT NOT specialization == 'math'", Truth::Unknown),
            ("NOT latency_p50 < 20", Truth::Unknown),
            // EXIST is the two-valued question: never Unknown, either way.
            ("EXIST specialization", Truth::No),
            ("EXIST latency_p50", Truth::No),
            ("EXIST cost", Truth::Yes),
            ("EXIST id", Truth::Yes),
            ("EXIST route_freq", Truth::Yes),
            ("EXIST residency", Truth::Yes),
            ("EXIST placement_node", Truth::Yes),
            ("NOT EXIST specialization", Truth::Yes),
            ("NOT EXIST cost", Truth::No),
            // OR: a Yes decides, whichever side it is on.
            ("specialization == 'math' OR cost == 1.0", Truth::Yes),
            ("cost == 1.0 OR specialization == 'math'", Truth::Yes),
            // OR: Unknown beside No stays Unknown — the missing field could
            // still have said yes.
            ("specialization == 'math' OR cost == 2.0", Truth::Unknown),
            ("cost == 2.0 OR specialization == 'math'", Truth::Unknown),
            ("specialization == 'math' OR latency_p50 < 20", Truth::Unknown),
            // AND: a No decides, whichever side it is on.
            ("specialization == 'math' AND cost == 2.0", Truth::No),
            ("cost == 2.0 AND specialization == 'math'", Truth::No),
            // AND: Unknown beside Yes stays Unknown.
            ("specialization == 'math' AND cost == 1.0", Truth::Unknown),
            ("specialization == 'math' AND latency_p50 < 20", Truth::Unknown),
            // The guarded forms — the reason EXIST is two-valued.
            ("EXIST specialization AND specialization == 'math'", Truth::No),
            ("EXIST latency_p50 AND latency_p50 < 20", Truth::No),
            ("NOT EXIST specialization OR specialization != 'math'", Truth::Yes),
            ("NOT EXIST specialization AND NOT EXIST latency_p50", Truth::Yes),
            // Precedence: NOT tighter than AND, AND tighter than OR.
            ("NOT cost == 2.0 AND cost == 1.0", Truth::Yes),
            ("NOT residency == RESIDENT OR specialization == 'math'", Truth::Unknown),
            ("cost == 2.0 AND specialization == 'math' OR cost == 1.0", Truth::Yes),
            // Parentheses override each of those three.
            ("NOT (cost == 2.0 AND cost == 1.0)", Truth::Yes),
            ("NOT (cost == 1.0 AND cost == 1.0)", Truth::No),
            ("NOT (residency == RESIDENT OR specialization == 'math')", Truth::No),
            ("cost == 2.0 AND (specialization == 'math' OR cost == 1.0)", Truth::No),
            ("(cost == 2.0 AND specialization == 'math') OR cost == 1.0", Truth::Yes),
            ("((cost == 1.0))", Truth::Yes),
        ] {
            let q = Query::parse(text).unwrap_or_else(|e| panic!("{text:?}: {e}"));
            assert_eq!(q.evaluate(&offer), expect, "{text:?}");
        }
    }

    /// The same table's other half: an offer that recorded everything can
    /// never answer `Unknown`, whatever the query says.
    #[test]
    fn an_offer_that_recorded_everything_is_never_unanswerable() {
        let offer = maths_offer("measured", Some(5.0));
        for (text, expect) in [
            ("EXIST specialization", Truth::Yes),
            ("EXIST latency_p50", Truth::Yes),
            ("NOT EXIST latency_p50", Truth::No),
            ("specialization == 'math'", Truth::Yes),
            ("NOT specialization == 'math'", Truth::No),
            ("specialization == 'code' OR latency_p50 < 10", Truth::Yes),
            ("specialization == 'code' AND latency_p50 < 10", Truth::No),
            ("NOT (specialization == 'code' OR latency_p50 > 10)", Truth::Yes),
        ] {
            let q = Query::parse(text).unwrap_or_else(|e| panic!("{text:?}: {e}"));
            assert_eq!(q.evaluate(&offer), expect, "{text:?}");
        }
    }

    /// `EXIST <field>` is two-valued for every field and every offer — the
    /// property, not four examples of it. If it ever answered `Unknown` it
    /// would be a second notion of "present" beside the one it was wired to,
    /// and a query could no longer close its own gap.
    #[test]
    fn exist_never_answers_unknown_for_any_field_of_any_offer() {
        const FIELDS: [&str; 10] = [
            "id",
            "specialization",
            "cost",
            "latency_p50",
            "latency_p99",
            "load",
            "residency",
            "mem_footprint",
            "placement_node",
            "route_freq",
        ];
        for offer in [gapped_offer(), maths_offer("measured", Some(5.0))] {
            for field in FIELDS {
                let q = Query::parse(&format!("EXIST {field}")).expect(field);
                assert_ne!(q.evaluate(&offer), Truth::Unknown, "EXIST {field}");
                let q = Query::parse(&format!("NOT EXIST {field}")).expect(field);
                assert_ne!(q.evaluate(&offer), Truth::Unknown, "NOT EXIST {field}");
            }
        }
    }

    /// **The finding of this stage.** `AND` and `OR` alone cannot tell
    /// three-valued logic apart from "a field nobody recorded simply does
    /// not match": with only monotone connectives, an expression answers
    /// `Yes` here exactly when it is true with every unknown replaced by
    /// false. `NOT` breaks that, and it breaks it in the dangerous
    /// direction — the two-valued reading *returns* the offer nobody could
    /// judge, which is the original bug arriving through the new operator.
    ///
    /// Recorded as a decision, not a citation: no normative TCL text was
    /// available to this batch (see [`Truth::not`]).
    #[test]
    fn not_is_where_three_valued_logic_stops_agreeing_with_missing_means_false() {
        let mut store = OfferStore::new();
        store.register(gapped_offer()).expect("registers");
        store.register(maths_offer("measured", Some(5.0))).expect("registers");

        // "not maths", the naive spelling. Under "missing means false" the
        // unrecorded offer would come back as an answer. Here it does not:
        // it is reported as unjudgeable, and `matched` holds only the offer
        // that is genuinely not maths — which, here, is none of them.
        let naive = Query::parse("NOT specialization == 'math'").expect("parses");
        let sel = naive.select_reporting(&store);
        assert!(sel.matched.is_empty(), "nobody is *known* not to do maths");
        assert_eq!(ids(&sel.unanswerable), ["from-the-wire"]);
        assert!(!sel.is_complete(), "an answer with an unjudged offer in it is a refusal");

        // The same intent, spelled as the question it actually is. Now every
        // offer is judged and the answer is complete.
        let honest =
            Query::parse("NOT EXIST specialization OR specialization != 'math'").expect("parses");
        let sel = honest.select_reporting(&store);
        assert_eq!(ids(&sel.matched), ["from-the-wire"]);
        assert!(sel.is_complete(), "EXIST closes the gap the naive spelling only reported");

        // And the fragment where the two readings agree, for the contrast:
        // AND/OR over the same gap give the same `matched` either way.
        let monotone = Query::parse("specialization == 'math' OR cost == 1.0").expect("parses");
        let sel = monotone.select_reporting(&store);
        assert_eq!(ids(&sel.matched), ["from-the-wire", "measured"]);
        assert!(sel.is_complete(), "the disjunct that answered was enough for both");
    }

    /// `EXIST` wired to the ordering's own `has_value` rather than to a
    /// second predicate: the gap `select_reporting` reports is exactly the
    /// gap `EXIST` can ask about, so a query can close it.
    #[test]
    fn exist_turns_the_reported_gap_into_a_query_that_can_close_it() {
        let mut store = OfferStore::new();
        store.register(gapped_offer()).expect("registers");
        store.register(maths_offer("measured", Some(5.0))).expect("registers");

        let plain = Query::parse("specialization == 'math'").expect("parses");
        let sel = plain.select_reporting(&store);
        assert_eq!(ids(&sel.matched), ["measured"]);
        assert_eq!(ids(&sel.unanswerable), ["from-the-wire"]);
        assert!(!sel.is_complete());
        // The note now offers the spelling that would close it.
        assert!(sel.gap_note().expect("a note").contains("EXIST specialization"), "{sel:?}");

        let guarded =
            Query::parse("EXIST specialization AND specialization == 'math'").expect("parses");
        let sel = guarded.select_reporting(&store);
        assert_eq!(ids(&sel.matched), ["measured"], "the same answer");
        assert!(sel.unanswerable.is_empty());
        assert!(sel.is_complete(), "and now a complete one");
        assert_eq!(sel.gap_note(), None);

        // The operator's other question — who never told us? — is one query.
        let unrecorded = Query::parse("NOT EXIST specialization").expect("parses");
        let sel = unrecorded.select_reporting(&store);
        assert_eq!(ids(&sel.matched), ["from-the-wire"]);
        assert!(sel.is_complete());
    }

    /// `OR` and parentheses over the store, selecting and ordering — the
    /// grammar's new shape doing the job the old one could only do with two
    /// queries and a merge.
    #[test]
    fn or_and_parentheses_select_and_order_over_the_store() {
        let s = store();
        // Two specializations at once: impossible before, and the reason
        // `OR` was the first thing TCL was missing.
        let q =
            Query::parse("specialization == 'math' OR specialization == 'code'").expect("parses");
        assert_eq!(ids(&q.select(&s)), ["expert-a", "expert-b", "expert-c", "expert-d"]);
        // Parentheses change which offers come back, and the unparenthesised
        // form is pinned beside it so precedence is measured, not assumed.
        let grouped =
            Query::parse("(specialization == 'math' OR specialization == 'code') AND load < 0.6")
                .expect("parses");
        assert_eq!(ids(&grouped.select(&s)), ["expert-a", "expert-b"]);
        let ungrouped =
            Query::parse("specialization == 'math' OR specialization == 'code' AND load < 0.6")
                .expect("parses");
        assert_eq!(
            ids(&ungrouped.select(&s)),
            ["expert-a", "expert-b", "expert-d"],
            "AND binds tighter, so the maths experts are unconditional"
        );
        // NOT over the store, with an ORDER BY still attached.
        let q =
            Query::parse("NOT specialization == 'math' ORDER BY route_freq DESC").expect("parses");
        assert_eq!(ids(&q.select(&s)), ["expert-c"]);
    }

    // ---- diagnostics for the new syntax: position and expectation, both ----

    /// Every malformed shape the four new constructs can produce, each with
    /// the byte position it must name. The bar the module's own docs set:
    /// "did not parse" without a place to fix is the diagnostic quality the
    /// negative corpus exists to prevent.
    #[test]
    fn every_new_syntax_error_names_its_byte_position_and_an_expectation() {
        for (text, at, expected) in [
            // A binary operator with nothing after it.
            ("load < 0.8 OR", 13usize, "expected a condition here"),
            ("load < 0.8 AND", 14, "expected a condition here"),
            ("NOT", 3, "expected a condition here"),
            ("NOT NOT", 7, "expected a condition here"),
            // Parentheses, both ways round, naming the '(' they belong to.
            ("(load < 0.8", 11, "expected ')' to close the '(' at byte 0"),
            ("load < 0.8 AND (cost == 1.0", 27, "expected ')' to close the '(' at byte 15"),
            ("(load < 0.8 AND (cost == 1.0)", 29, "expected ')' to close the '(' at byte 0"),
            ("load < 0.8)", 10, "expected 'AND', 'OR', 'ORDER BY' or the end of the query"),
            ("()", 1, "expected a condition here"),
            ("(load < 0.8 cost == 1.0)", 12, "expected ')' to close the '(' at byte 0"),
            // EXIST wants a field, and says which names are fields.
            ("EXIST", 5, "expected a field name, found the end of the query"),
            ("EXIST 'math'", 6, "expected a field name, found string 'math'"),
            ("EXIST speciality", 6, "unknown field \"speciality\""),
            ("EXIST specialization == 'math'", 21, "expected 'AND', 'OR', 'ORDER BY'"),
            // A lowercase keyword is refused as a keyword, not reported as
            // an unknown field. Before T1 this only worked where the parser
            // already expected 'AND' or 'ORDER'.
            ("load < 0.8 or cost < 2", 11, "keywords are uppercase"),
            ("not load < 0.8", 0, "keywords are uppercase — write 'NOT'"),
            ("exist specialization", 0, "keywords are uppercase — write 'EXIST'"),
            ("load < 0.8 AND exist cost", 15, "keywords are uppercase — write 'EXIST'"),
            ("load < 0.8 AND not cost == 1.0", 15, "keywords are uppercase — write 'NOT'"),
            // …but an *uppercase* keyword where a field belongs is not
            // advised to become uppercase. It is told what fields are.
            ("load < 0.8 AND BY", 15, "found the keyword \"BY\": fields are id, specialization"),
            ("NOT DESC", 4, "found the keyword \"DESC\": fields are id, specialization"),
            // '!' is not negation, and now says what is.
            ("!EXIST specialization", 0, "negation is written 'NOT'"),
            // An unexpected character names what could have been there.
            ("load < 0.8 & cost < 2", 11, "unexpected character '&'"),
            ("load < 0.8 & cost < 2", 11, "expected a field name, a keyword"),
            ("load < 0.8 | cost < 2", 11, "unexpected character '|'"),
        ] {
            let err = Query::parse(text).expect_err(text);
            assert_eq!(err.at, at, "{text:?} → {err}");
            assert!(err.message.contains(expected), "{text:?} → {err}");
        }
    }

    /// Nesting is bounded, and the bound refuses rather than crashing.
    /// Untrusted input plus unbounded recursion is a stack overflow, which
    /// is not a refusal at all — this parser's whole argument for being
    /// first-party is that it refuses with a position.
    #[test]
    fn nesting_past_the_limit_is_refused_by_position_and_limit_not_by_a_stack_overflow() {
        let depth = usize::try_from(MAX_DEPTH).expect("fits");
        // Exactly at the limit: accepted, and still means what it says.
        let ok = format!("{}cost == 1.0{}", "(".repeat(depth), ")".repeat(depth));
        assert_eq!(Query::parse(&ok).expect("at the limit").evaluate(&gapped_offer()), Truth::Yes);
        let ok = format!("{}cost == 2.0", "NOT ".repeat(depth));
        assert!(Query::parse(&ok).is_ok(), "NOT nests to the same limit");

        // One past it, both constructs, each naming the offending token.
        let err = Query::parse(&format!("{}cost == 1.0", "(".repeat(depth + 1))).unwrap_err();
        assert_eq!(err.at, depth, "the '(' that went too deep");
        assert!(err.message.contains(&format!("more than {MAX_DEPTH} levels deep")), "{err}");
        let err = Query::parse(&format!("{}cost == 1.0", "NOT ".repeat(depth + 1))).unwrap_err();
        assert_eq!(err.at, depth * 4);
        assert!(err.message.contains("'NOT' nests more than"), "{err}");

        // A pathological string is a refusal in bounded time, not a crash.
        let err = Query::parse(&"(".repeat(100_000)).unwrap_err();
        assert_eq!(err.at, depth);
    }

    /// Width is *not* bounded, and must not become bounded by accident: a
    /// long `AND`/`OR` chain is flat in the tree, so evaluating it costs a
    /// loop rather than fifty thousand stack frames. If `And`/`Or` ever go
    /// back to being binary and recursive, this is where it shows up.
    #[test]
    fn a_very_wide_and_or_chain_parses_and_evaluates_without_recursing() {
        let wide = vec!["cost == 1.0"; 50_000].join(" AND ");
        let q = Query::parse(&wide).expect("width is not depth");
        assert_eq!(q.evaluate(&gapped_offer()), Truth::Yes);
        let wide = vec!["cost == 2.0"; 50_000].join(" OR ");
        let q = Query::parse(&wide).expect("width is not depth");
        assert_eq!(q.evaluate(&gapped_offer()), Truth::No);
    }
}
