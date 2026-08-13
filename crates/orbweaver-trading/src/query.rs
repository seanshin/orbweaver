//! Constraint queries over the offer store: the §4.3 subset.
//!
//! ```text
//! query      := comparison ( "AND" comparison )*  order?
//! comparison := field cmp literal
//! cmp        := "==" | "!=" | "<" | "<=" | ">" | ">="
//! order      := "ORDER" "BY" field ( "ASC" | "DESC" )
//! ```
//!
//! e.g. `specialization == 'math' AND latency_p99 < 200 ORDER BY route_freq
//! DESC`. Fields are the [`crate::Offer`] properties by name; string
//! literals are single-quoted; residency literals are the bare enum names
//! (`RESIDENT`, …); keywords are uppercase, as the architecture document
//! writes them. Latencies are milliseconds by the offer contract, so
//! literals are bare numbers — no unit suffixes.
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
//! # Scope
//!
//! Exactly the subset above and nothing else: no `OR`, no parentheses, no
//! unit suffixes, no case-insensitive keywords. Accepting more here would
//! mean accepting queries the (future) IDL surface cannot round-trip.

use std::cmp::Ordering;

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

/// A parsed constraint query: conjoined comparisons plus an optional
/// `ORDER BY`. Build one with [`Query::parse`], run it with
/// [`Query::select`].
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    conjuncts: Vec<Comparison>,
    order: Option<(Field, Direction)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
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

const FIELD_LIST: &str = "id, specialization, cost, latency_p50, latency_p99, load, residency, \
                          mem_footprint, placement_node, route_freq";

impl Field {
    fn from_name(name: &str) -> Option<Field> {
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

    fn name(self) -> &'static str {
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

    fn kind(self) -> Kind {
        match self {
            Field::Id | Field::Specialization | Field::PlacementNode => Kind::Text,
            Field::Cost | Field::LatencyP50 | Field::LatencyP99 | Field::Load => Kind::Float,
            Field::MemFootprint | Field::RouteFreq => Kind::Counter,
            Field::Residency => Kind::State,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
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
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    fn text(self) -> &'static str {
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

impl Comparison {
    /// Whether the offer satisfies this comparison. The literal's variant
    /// always matches the field's kind — the parser refused anything else.
    fn holds(&self, offer: &Offer) -> bool {
        let ord = match &self.literal {
            Literal::Text(s) => text_value(offer, self.field).cmp(s.as_str()),
            Literal::Float(x) => float_value(offer, self.field).total_cmp(x),
            Literal::Counter(n) => counter_value(offer, self.field).cmp(n),
            Literal::State(r) => offer.residency.cmp(r),
        };
        self.op.holds(ord)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Asc,
    Desc,
}

fn text_value(offer: &Offer, field: Field) -> &str {
    match field {
        Field::Id => &offer.id,
        Field::Specialization => &offer.specialization,
        Field::PlacementNode => &offer.placement_node,
        _ => unreachable!("the parser only pairs text literals with text fields"),
    }
}

fn float_value(offer: &Offer, field: Field) -> f64 {
    match field {
        Field::Cost => offer.cost,
        Field::LatencyP50 => offer.latency_p50,
        Field::LatencyP99 => offer.latency_p99,
        Field::Load => offer.load,
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

/// Compares two offers on `field`, for `ORDER BY`. Floats compare by IEEE
/// total order — deterministic even for the values nobody should register.
fn offer_cmp(a: &Offer, b: &Offer, field: Field) -> Ordering {
    match field.kind() {
        Kind::Text => text_value(a, field).cmp(text_value(b, field)),
        Kind::Float => float_value(a, field).total_cmp(&float_value(b, field)),
        Kind::Counter => counter_value(a, field).cmp(&counter_value(b, field)),
        Kind::State => a.residency.cmp(&b.residency),
    }
}

// ---- lexer ----

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Number(String),
    Str(String),
    Op(CmpOp),
    End,
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Ident(s) => format!("identifier {s:?}"),
        Tok::Number(s) => format!("number {s}"),
        Tok::Str(s) => format!("string '{s}'"),
        Tok::Op(op) => format!("'{}'", op.text()),
        Tok::End => "the end of the query".to_owned(),
    }
}

fn lex(text: &str) -> Result<Vec<(usize, Tok)>, ParseError> {
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
                        message: "expected '!=' ('!' alone is not an operator)".to_owned(),
                    });
                }
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
                return Err(ParseError { at: i, message: format!("unexpected character {ch:?}") });
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
}

impl Parser {
    fn bump(&mut self) -> (usize, Tok) {
        let t = self.toks[self.i].clone();
        if self.i + 1 < self.toks.len() {
            self.i += 1;
        }
        t
    }

    fn err<T>(&self, at: usize, message: String) -> Result<T, ParseError> {
        Err(ParseError { at, message })
    }

    fn parse_field(&mut self) -> Result<Field, ParseError> {
        let (at, tok) = self.bump();
        match tok {
            Tok::Ident(name) => match Field::from_name(&name) {
                Some(f) => Ok(f),
                None => self.err(at, format!("unknown field {name:?}: fields are {FIELD_LIST}")),
            },
            other => self.err(at, format!("expected a field name, found {}", describe(&other))),
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
            (Kind::Float, Tok::Number(n)) => match n.parse::<f64>() {
                Ok(x) => Ok(Literal::Float(x)),
                Err(_) => self.err(at, format!("malformed number {n}")),
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
        let mut p = Parser { toks: lex(text)?, i: 0 };
        let mut conjuncts = vec![p.parse_comparison()?];
        let mut order = None;
        loop {
            let (at, tok) = p.bump();
            match tok {
                Tok::Ident(ref s) if s == "AND" => conjuncts.push(p.parse_comparison()?),
                Tok::Ident(ref s) if s == "ORDER" => {
                    order = Some(p.parse_order()?);
                    let (at, tok) = p.bump();
                    if tok != Tok::End {
                        return Err(ParseError {
                            at,
                            message: format!(
                                "expected the end of the query after the ORDER BY clause, \
                                 found {}",
                                describe(&tok)
                            ),
                        });
                    }
                    break;
                }
                Tok::End => break,
                other => {
                    return Err(ParseError {
                        at,
                        message: format!(
                            "expected 'AND', 'ORDER BY' or the end of the query, found {} \
                             (keywords are uppercase)",
                            describe(&other)
                        ),
                    });
                }
            }
        }
        Ok(Query { conjuncts, order })
    }

    /// Whether `offer` satisfies every conjunct.
    pub fn matches(&self, offer: &Offer) -> bool {
        self.conjuncts.iter().all(|c| c.holds(offer))
    }

    /// Every matching offer, in the pinned order: the `ORDER BY` field and
    /// direction with ties broken by ascending id, or plain ascending id
    /// when no ordering was asked for. The same store and query always
    /// return the same list in the same order.
    pub fn select<'a>(&self, store: &'a OfferStore) -> Vec<&'a Offer> {
        let mut out: Vec<&Offer> = store.iter().filter(|o| self.matches(o)).collect();
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
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                specialization: spec.to_owned(),
                cost: 1.0,
                latency_p50: p99 / 2.0,
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
}
