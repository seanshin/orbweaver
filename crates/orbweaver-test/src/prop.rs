//! Property tests from types: seeded CDR round-trips over generated values.
//!
//! The property is one sentence: **for any value of any type, encoding it,
//! decoding the result, and encoding that must produce the same bytes** — in
//! both byte orders, and starting at every alignment phase. It is the DynAny
//! fuzzing seed the component ledger names, built on [`Value`] because the
//! mutation API does not exist yet and the marshalling half does.
//!
//! # Why byte-stability rather than value equality alone
//!
//! Both are checked, and the byte comparison is the one that earns its keep.
//! Value equality catches a decoder that loses information. Byte equality also
//! catches a decoder that *recovers* the value while disagreeing with the
//! encoder about where the padding went — the whole class this project has
//! most often got wrong (§4.4, "alignment origin matters"). A struct whose
//! members straddle an 8-byte boundary can round-trip perfectly by value and
//! still be four bytes short on the wire.
//!
//! This does not contradict the rule in `CLAUDE.md` that says to compare
//! decoded values rather than raw buffers. That rule is about comparing
//! *against a reference ORB*, where padding content is undefined by the
//! specification and omniORB does not zero it. Here both buffers come from our
//! own encoder, so their padding is ours to be consistent about, and an
//! inconsistency is a defect rather than a false alarm.
//!
//! # Alignment phase
//!
//! A value is encoded after `phase` filler octets so that it begins at every
//! offset modulo 8 in turn. This is [`Encoder::continuing_at`]'s hazard made
//! into a test: an encoder that pads correctly only when it starts at zero
//! passes every naive round-trip and mis-encodes every `double` in a GIOP 1.0
//! body. The filler is `0xEE` rather than zero so that padding — which the
//! encoder writes as zeros — stays distinguishable in a hex dump of a failure.
//!
//! # The AnyJSON leg
//!
//! Every value the CDR leg round-trips is also taken across the agent
//! boundary and back: [`anyjson::to_json`], the document rendered to text and
//! parsed again (the text is what actually crosses, and it is the only place
//! the string escaping meets a generated string), [`anyjson::from_json`], then
//! encoded in the same byte order at the same phase — and the bytes must equal
//! the CDR-only leg's. Byte equality is legitimate here for the same reason
//! it is above: both buffers come from our own encoder, so padding is ours to
//! be consistent about, and the JSON mapping is ours too. This leg did not
//! exist until 2026-08-19: the sweep round-tripped CDR only, and the mapping's
//! refusal of every non-empty value under a `TypeCode::Recursive` marker was
//! not something it could have seen at any witness.
//!
//! A type the mapping documents as not crossing (`fixed`, `Principal`, a
//! `void` where a type belongs — the arms `from_json` answers "cannot cross
//! yet") is a **`json/unmapped`** finding, advice, once per type, and the leg
//! is not run for it. That is a distinct class rather than a silent skip so a
//! caller can pin the list; a new member is a finding about the mapping, not
//! about the type.
//!
//! # Seed discipline
//!
//! *A failing case must be reproducible from its seed. That is the entire
//! point of generating values instead of writing them.* 실패한 케이스는 시드
//! 하나로 재현되어야 한다.
//!
//! - Values come from [`Rng`], a 64-bit xorshift with a multiply finaliser
//!   implemented here. Nothing is drawn from the clock, the environment, the
//!   address space or the thread. A failure that cannot be replayed is an
//!   anecdote.
//! - The batch seed is [`DEFAULT_SEED`], a constant. Two runs of
//!   [`roundtrip_property`] with the same arguments produce the same values in
//!   the same order.
//! - Each case gets its **own** derived seed, [`case_seed`], and every finding
//!   reports it. Reproduction is [`roundtrip_case`] with that one number — the
//!   batch does not have to be replayed, and the case count does not have to
//!   match.
//! - [`roundtrip_case`] runs its seed across **both** byte orders and **all
//!   eight** alignment phases, while the batch gives each case one phase. So
//!   the reported seed always reproduces a superset of the failing
//!   combination, and the finding still names the exact endian and phase.
//! - **A seed reproduces a case within a build, not across sampler changes.**
//!   Editing the generator changes what a seed means. That is acceptable and
//!   is why a genuine defect gets its failing [`Value`] pinned into a unit test
//!   as a literal; the seed is for the debugging session, the literal is for
//!   the regression.
//!
//! [`Encoder::continuing_at`]: orbweaver_cdr::Encoder::continuing_at
//! [`anyjson::to_json`]: orbweaver_dynamic::anyjson::to_json
//! [`anyjson::from_json`]: orbweaver_dynamic::anyjson::from_json

use std::collections::BTreeSet;

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_dynamic::Value;
use orbweaver_dynamic::anyjson::{self, LocalReferences};
use orbweaver_dynamic::json::Json;
use orbweaver_forge::{Finding, Severity};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_giop::{IiopProfile, Ior, Version};

use crate::finding;

/// The batch seed. Constant, so a run is a rerun.
pub const DEFAULT_SEED: u64 = 0x0000_C0DE_0000_5EED;

/// How deeply the sampler will nest before it stops growing a value.
///
/// Bounded because a recursive type has no finite expansion (the registry
/// represents the cycle as [`TypeCode::Recursive`]) and because a
/// sequence-of-sequence-of-struct would otherwise grow multiplicatively.
const MAX_DEPTH: u32 = 4;

/// The longest generated sequence, when the bound permits.
const MAX_SEQUENCE: usize = 4;

/// A seeded xorshift64 generator with a multiply finaliser.
///
/// First-party by necessity and by preference: a dependency would need a
/// licence review (`CLAUDE.md`), and thirty lines that we can pin the exact
/// output of is worth more here than a general-purpose crate. The quality bar
/// is "spreads over the input space and never varies between runs", not
/// cryptographic.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// A generator at `seed`. Seed zero is remapped: xorshift is a fixed point
    /// at zero and would return it forever.
    pub fn new(seed: u64) -> Self {
        Self { state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed } }
    }

    /// The next 64 bits.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..n`, or zero when `n` is zero.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next_u64() % n as u64) as usize }
    }

    fn flip(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// One in `n` chance, used to reach boundary values more often than a
    /// uniform draw over 2^64 ever would.
    fn one_in(&mut self, n: usize) -> bool {
        self.below(n) == 0
    }
}

/// The seed for case `index` of a batch rooted at `root`.
///
/// SplitMix64's finaliser, so that adjacent indices produce unrelated seeds —
/// a batch whose cases differ only in their low bits explores one corner.
pub fn case_seed(root: u64, index: u64) -> u64 {
    let mut z = root.wrapping_add(index.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// What a property run measured, as distinct from what it found.
///
/// A report of findings cannot say how much ran: a sweep whose JSON leg was
/// skipped for every value prints the same empty list as one that crossed
/// them all, and until 2026-08-19 that was exactly the state of things — the
/// leg did not exist and nothing said so. These counts are printed by
/// `contract-check` beside the case count so a leg that stops running is a
/// visible number, not a silent one.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Measured {
    /// Values that were round-tripped through CDR, counted once per byte
    /// order — a case that produced a value contributes two.
    pub cdr: usize,
    /// Of those, the ones that were also taken across AnyJSON and back and
    /// re-encoded. Less than `cdr` only where a type is `json/unmapped`.
    pub json: usize,
}

impl Measured {
    /// Adds another run's counts to this one.
    pub fn add(&mut self, other: Measured) {
        self.cdr += other.cdr;
        self.json += other.json;
    }
}

/// Runs `cases` round-trip cases against `tc` from [`DEFAULT_SEED`].
pub fn roundtrip_property(tc: &TypeCode, cases: usize) -> Vec<Finding> {
    roundtrip_property_seeded(tc, cases, DEFAULT_SEED)
}

/// Runs `cases` round-trip cases against `tc` from an explicit root seed.
///
/// Each case gets one alignment phase, cycling `0..8`, so a batch of at least
/// eight cases covers every phase. Fewer than eight is allowed and leaves the
/// remaining phases unmeasured — which is stated here rather than hidden,
/// per the harness rule that an unmeasured check is not a pass.
pub fn roundtrip_property_seeded(tc: &TypeCode, cases: usize, root: u64) -> Vec<Finding> {
    roundtrip_property_measured(tc, cases, root).0
}

/// [`roundtrip_property_seeded`], also returning how much it measured.
pub fn roundtrip_property_measured(
    tc: &TypeCode,
    cases: usize,
    root: u64,
) -> (Vec<Finding>, Measured) {
    let mut out = Vec::new();
    let mut gaps = BTreeSet::new();
    let mut measured = Measured::default();

    if cases == 0 {
        return (out, measured);
    }

    // The mapping's own limit, stated once per type and before any case runs,
    // so it reads the same whether or not the sampler can build the type. A
    // `fixed` member makes a type both unsampled (§4.4) and unmapped, and the
    // two are different facts about two different modules.
    let cross_json = match json_unmapped(tc) {
        Some(reason) => {
            out.push(finding(
                "json/unmapped",
                Severity::Advice,
                format!(
                    "{} is not taken across AnyJSON by the property, so the JSON leg is \
                     unmeasured for it: {reason}",
                    describe(tc)
                ),
                type_id(tc),
                Some(
                    "a documented limit of the mapping rather than a defect; when the mapping \
                     grows the type, remove it from json_unmapped and the leg runs"
                        .into(),
                ),
            ));
            false
        }
        None => true,
    };

    let mut sampler =
        Sampler { rng: Rng::new(root), gaps: BTreeSet::new(), depth: 0, open: Vec::new() };
    if sampler.sample(tc).is_none() {
        out.push(finding(
            "prop/unsupported-type",
            Severity::Advice,
            format!(
                "{} cannot be sampled, so it is not covered by the round-trip property: {}",
                describe(tc),
                why_unsupported(tc)
            ),
            type_id(tc),
            Some(
                "this is a coverage gap rather than a defect; cover the type with a \
                 hand-written case, or note the limit in docs/PLAN.md §4.4"
                    .into(),
            ),
        ));
        return (out, measured);
    }

    for index in 0..cases {
        let seed = case_seed(root, index as u64);
        let phase = index % 8;
        let mut sampler =
            Sampler { rng: Rng::new(seed), gaps: BTreeSet::new(), depth: 0, open: Vec::new() };
        let Some(value) = sampler.sample(tc) else {
            // The root seed sampled this type, so a later seed that cannot is
            // the generator disagreeing with itself — and a case that ran
            // nothing must not count as one that passed. Until 2026-08-19 this
            // was a bare `continue`, and 22 of `corpus/golden/15`'s 32
            // `TreeSeq` cases fell through it with the report still green.
            gaps.insert(format!(
                "case {index} (seed 0x{seed:016x}) produced no value and ran nothing; the \
                 sampler refused a shape its own predicate accepted, so the case is unmeasured"
            ));
            continue;
        };
        gaps.append(&mut sampler.gaps);
        for endian in [Endian::Big, Endian::Little] {
            out.extend(one_case(tc, &value, seed, endian, phase, cross_json, &mut measured));
        }
    }

    for gap in gaps {
        out.push(finding(
            "prop/unmeasured",
            Severity::Advice,
            // Prefixed with the type under test: two registered types can share
            // a gap for the same reason, and two identical lines in a report
            // read as a duplicate rather than as two uncovered types.
            format!("while generating {}: {gap}", type_id(tc)),
            type_id(tc),
            Some(
                "add a hand-written case for the arm the generator cannot reach, so the \
                 coverage gap is visible in a test rather than only here"
                    .into(),
            ),
        ));
    }
    (out, measured)
}

/// Re-runs one case, in both byte orders and at every alignment phase.
///
/// The reproduction entry point: a finding reports a seed, and this takes the
/// seed back to the failure without the batch that found it. The JSON leg
/// runs here too, for the same value, so a `json/*` finding reproduces the
/// same way a `prop/*` one does.
pub fn roundtrip_case(tc: &TypeCode, seed: u64) -> Vec<Finding> {
    let mut sampler =
        Sampler { rng: Rng::new(seed), gaps: BTreeSet::new(), depth: 0, open: Vec::new() };
    let Some(value) = sampler.sample(tc) else { return Vec::new() };
    let cross_json = json_unmapped(tc).is_none();
    let mut measured = Measured::default();
    let mut out = Vec::new();
    for phase in 0..8 {
        for endian in [Endian::Big, Endian::Little] {
            out.extend(one_case(tc, &value, seed, endian, phase, cross_json, &mut measured));
        }
    }
    out
}

/// The sample value a seed produces for a type, for a caller that wants to see
/// it rather than round-trip it.
pub fn sample(tc: &TypeCode, seed: u64) -> Option<Value> {
    Sampler { rng: Rng::new(seed), gaps: BTreeSet::new(), depth: 0, open: Vec::new() }.sample(tc)
}

/// Encode → decode → encode, once, at one byte order and one alignment phase —
/// and, when `cross_json`, the same value out through AnyJSON and back in,
/// encoded again and compared with the first leg's bytes.
fn one_case(
    tc: &TypeCode,
    value: &Value,
    seed: u64,
    endian: Endian,
    phase: usize,
    cross_json: bool,
    measured: &mut Measured,
) -> Vec<Finding> {
    let where_ = format!("seed=0x{seed:016x} endian={endian:?} phase={phase} type={}", type_id(tc));
    let reproduce = format!(
        "reproduce with orbweaver_test::prop::roundtrip_case(&tc, 0x{seed:016x}); the value is \
         orbweaver_test::prop::sample(&tc, 0x{seed:016x})"
    );

    let first = match encode_at_phase(tc, value, endian, phase) {
        Ok(b) => b,
        Err(e) => {
            return vec![finding(
                "prop/encode-error",
                Severity::Error,
                format!("a generated value of {} failed to encode: {e}", describe(tc)),
                where_,
                Some(reproduce),
            )];
        }
    };

    let decoded = match decode_at_phase(tc, &first, endian, phase) {
        Ok(v) => v,
        Err(e) => {
            return vec![finding(
                "prop/decode-error",
                Severity::Error,
                format!(
                    "{} encoded to {} octet(s) that our own decoder rejected: {e}",
                    describe(tc),
                    first.len() - phase
                ),
                where_,
                Some(reproduce),
            )];
        }
    };

    measured.cdr += 1;

    let mut out = Vec::new();
    if decoded != *value {
        out.push(finding(
            "prop/roundtrip-value",
            Severity::Error,
            format!(
                "{} did not survive a round trip: encoded {value:?}, decoded {decoded:?}",
                describe(tc)
            ),
            where_.clone(),
            Some(reproduce.clone()),
        ));
    }
    match encode_at_phase(tc, &decoded, endian, phase) {
        Ok(second) if second != first => out.push(finding(
            "prop/roundtrip-bytes",
            Severity::Error,
            format!(
                "{} is not byte-stable: re-encoding the decoded value produced {} octet(s) \
                 instead of {}, first differing at offset {}",
                describe(tc),
                second.len() - phase,
                first.len() - phase,
                first
                    .iter()
                    .zip(&second)
                    .position(|(a, b)| a != b)
                    .map(|i| i.saturating_sub(phase).to_string())
                    .unwrap_or_else(|| "the end".into()),
            ),
            where_.clone(),
            Some(reproduce.clone()),
        )),
        Ok(_) => {}
        Err(e) => out.push(finding(
            "prop/encode-error",
            Severity::Error,
            format!("{} encoded, decoded, then failed to re-encode: {e}", describe(tc)),
            where_.clone(),
            Some(reproduce.clone()),
        )),
    }

    if cross_json {
        measured.json += 1;
        out.extend(json_leg(tc, value, &first, endian, phase, &where_, &reproduce));
    }
    out
}

/// The AnyJSON leg: `to_json` → text → `from_json` → CDR, against the bytes
/// the CDR-only leg produced for the same value.
///
/// Comparing bytes rather than only values is legitimate here for the reason
/// the module documentation gives: both buffers come from our own encoder,
/// and the mapping in between is ours too, so a difference is a defect in one
/// of the three and never padding a peer left undefined. Value equality is
/// checked as well because it names *what* was lost; the bytes then catch what
/// value equality forgives — `-0.0` compares equal to `0.0`, and does not
/// encode equal.
///
/// The document goes through its text form on purpose. `to_json` hands back a
/// tree, but what crosses the agent boundary is a string, and the escaping in
/// `Json::write` meets a generated string nowhere else in the sweep.
///
/// The classes are the CDR leg's, prefixed `json/`, so a report groups them
/// beside their CDR counterparts and a reader can tell at once which mechanism
/// failed: `to-json-error` (the mapping refused a value the encoder accepted),
/// `text-roundtrip` (the document did not survive its own text),
/// `from-json-error` (the mapping refused what it wrote), `roundtrip-value`,
/// `roundtrip-bytes`, `encode-error`.
#[allow(clippy::too_many_arguments)]
fn json_leg(
    tc: &TypeCode,
    value: &Value,
    first: &[u8],
    endian: Endian,
    phase: usize,
    where_: &str,
    reproduce: &str,
) -> Vec<Finding> {
    let report = |rule: &str, message: String| {
        vec![finding(rule, Severity::Error, message, where_.to_owned(), Some(reproduce.to_owned()))]
    };
    // One table for both directions: a handle `to_json` issues is what
    // `from_json` resolves, exactly as at the MCP boundary within a session.
    let mut refs = LocalReferences::new();

    let doc = match anyjson::to_json(tc, value, &mut refs) {
        Ok(d) => d,
        Err(e) => {
            return report(
                "json/to-json-error",
                format!(
                    "a generated value of {} that CDR encoded was refused by AnyJSON to_json: {e}",
                    describe(tc)
                ),
            );
        }
    };

    let text = doc.to_string();
    let reread = match Json::parse(&text) {
        Ok(j) => j,
        Err(e) => {
            return report(
                "json/text-roundtrip",
                format!(
                    "the AnyJSON document for {} does not parse back from its own text: {e}; \
                     document {}",
                    describe(tc),
                    excerpt(&text)
                ),
            );
        }
    };
    if reread != doc {
        return report(
            "json/text-roundtrip",
            format!(
                "the AnyJSON document for {} changed on the way through its own text; \
                 document {}",
                describe(tc),
                excerpt(&text)
            ),
        );
    }

    let back = match anyjson::from_json(tc, &reread, &refs) {
        Ok(v) => v,
        Err(e) => {
            return report(
                "json/from-json-error",
                format!(
                    "AnyJSON refused the document it wrote for {}: {e}; document {}",
                    describe(tc),
                    excerpt(&text)
                ),
            );
        }
    };

    let mut out = Vec::new();
    if back != *value {
        out.extend(report(
            "json/roundtrip-value",
            format!(
                "{} did not survive AnyJSON: sent {value:?}, got back {back:?}; document {}",
                describe(tc),
                excerpt(&text)
            ),
        ));
    }
    match encode_at_phase(tc, &back, endian, phase) {
        Ok(second) if second != first => out.extend(report(
            "json/roundtrip-bytes",
            format!(
                "{} is not byte-stable across AnyJSON: the value read back from the document \
                 encoded to {} octet(s) instead of {}, first differing at offset {}; document {}",
                describe(tc),
                second.len() - phase,
                first.len() - phase,
                first
                    .iter()
                    .zip(&second)
                    .position(|(a, b)| a != b)
                    .map(|i| i.saturating_sub(phase).to_string())
                    .unwrap_or_else(|| "the end".into()),
                excerpt(&text)
            ),
        )),
        Ok(_) => {}
        Err(e) => out.extend(report(
            "json/encode-error",
            format!(
                "{} crossed AnyJSON and the value that came back failed to encode: {e}; \
                 document {}",
                describe(tc),
                excerpt(&text)
            ),
        )),
    }
    out
}

/// The head of a document, for a message. A finding names a seed that
/// reproduces the whole thing; the excerpt is for reading the report.
fn excerpt(text: &str) -> String {
    const HEAD: usize = 240;
    if text.len() <= HEAD {
        return text.to_owned();
    }
    let cut = (0..=HEAD).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
    format!("{}… ({} bytes)", &text[..cut], text.len())
}

/// Why a type is not taken across AnyJSON, or `None` when it is.
///
/// The list is the mapping's own: `from_json_at` answers "cannot cross yet"
/// for exactly `void`, `null`, `fixed` and `Principal`, and `to_json_at` has
/// no arm for any of them. Kept in one predicate so a test can pin the set of
/// types it names over the corpus, and so growing the mapping is one edit
/// here that makes the leg start running.
fn json_unmapped(tc: &TypeCode) -> Option<String> {
    match tc {
        TypeCode::Alias { aliased, .. } => json_unmapped(aliased),
        TypeCode::Fixed { .. } => Some(
            "`fixed` has no AnyJSON form yet (from_json: \"cannot cross yet\"; the wire does \
             not carry it either, docs/PLAN.md §4.4)"
                .into(),
        ),
        TypeCode::Principal => {
            Some("`Principal` has no AnyJSON form (withdrawn from CORBA)".into())
        }
        TypeCode::Void | TypeCode::Null => {
            Some("`void` where a type belongs has no AnyJSON value form".into())
        }
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
            members.iter().find_map(|m| json_unmapped(&m.tc))
        }
        TypeCode::Union { discriminator, cases, .. } => {
            json_unmapped(discriminator).or_else(|| cases.iter().find_map(|c| json_unmapped(&c.tc)))
        }
        TypeCode::Sequence { element, .. } | TypeCode::Array { element, .. } => {
            json_unmapped(element)
        }
        // A marker names a type that is under construction and has already
        // been asked; `any` and `TypeCode` carry their own type per value and
        // the mapping spells every TypeCode.
        _ => None,
    }
}

/// Encodes after `phase` filler octets, so the value starts off-alignment.
fn encode_at_phase(
    tc: &TypeCode,
    value: &Value,
    endian: Endian,
    phase: usize,
) -> Result<Vec<u8>, String> {
    let mut e = Encoder::new(endian);
    e.put_bytes(&vec![0xEE; phase]);
    orbweaver_dynamic::encode(&mut e, tc, value).map_err(|err| err.to_string())?;
    e.finish().map_err(|err| err.to_string())
}

fn decode_at_phase(
    tc: &TypeCode,
    bytes: &[u8],
    endian: Endian,
    phase: usize,
) -> Result<Value, String> {
    let mut d = Decoder::new(bytes, endian);
    d.get_bytes(phase).map_err(|err| err.to_string())?;
    orbweaver_dynamic::decode(&mut d, tc).map_err(|err| err.to_string())
}

/// The generator.
struct Sampler {
    rng: Rng,
    /// Arms the generator could not reach, phrased for a reader.
    gaps: BTreeSet<String>,
    depth: u32,
    /// Repository ids of the constructed types this sample is currently inside,
    /// with the `TypeCode` each one names.
    ///
    /// This is what lets a recursive arm be generated at all. The registry
    /// represents a cycle as [`TypeCode::Recursive`] holding only an id —
    /// honest, because a recursive type has no finite expansion — so a
    /// generator that reads the `TypeCode` alone can only produce the empty
    /// case, which is what it did: every `TreeSeq` came out empty and the
    /// recursive arm of the marshaller was never executed by anything.
    /// Resolving the id against the enclosing type under way gives a finite
    /// expansion after all, bounded by [`MAX_DEPTH`] rather than by the type.
    open: Vec<(String, TypeCode)>,
}

impl Sampler {
    /// A value of `tc`, or `None` when this project cannot marshal the type at
    /// all.
    fn sample(&mut self, tc: &TypeCode) -> Option<Value> {
        Some(match tc {
            // Pushed like a struct: a cycle can name the typedef rather than
            // the type it wraps — `typedef sequence<Tree> TreeSeq` inside
            // `struct Tree` produces a marker naming TreeSeq — and a sampler
            // that saw through aliases could never resolve that one.
            TypeCode::Alias { id, aliased, .. } => {
                self.open.push((id.clone(), tc.clone()));
                let v = self.sample(aliased);
                self.open.pop();
                v?
            }

            // Void marshals to nothing and decodes to an empty struct, which is
            // a stable round trip and a vacuous one. Included so that a `void`
            // return in a signature does not read as a coverage gap.
            TypeCode::Null | TypeCode::Void => Value::Struct(Vec::new()),

            TypeCode::Boolean => Value::Bool(self.rng.flip()),
            TypeCode::Octet => Value::Octet(self.rng.next_u64() as u8),
            // A CORBA `char` is one octet of a byte-oriented codeset; the wide
            // forms are `wchar`. Printable ASCII keeps failures legible.
            TypeCode::Char => Value::Char(0x20 + (self.rng.below(0x5F) as u8)),
            TypeCode::WChar => Value::WChar(self.wide_char()),

            TypeCode::Short => Value::Short(self.int(i16::MIN as i64, i16::MAX as i64) as i16),
            TypeCode::UShort => Value::UShort(self.uint(u16::MAX as u64) as u16),
            TypeCode::Long => Value::Long(self.int(i32::MIN as i64, i32::MAX as i64) as i32),
            TypeCode::ULong => Value::ULong(self.uint(u32::MAX as u64) as u32),
            TypeCode::LongLong => Value::LongLong(self.int(i64::MIN, i64::MAX)),
            TypeCode::ULongLong => Value::ULongLong(self.uint(u64::MAX)),

            TypeCode::Float => Value::Float(self.float32()),
            TypeCode::Double => Value::Double(self.float64()),
            // Sixteen opaque octets is exactly what the marshaller carries, so
            // that is what is generated; inventing an f128 would test our
            // conversion rather than the wire.
            TypeCode::LongDouble => {
                let mut raw = [0u8; 16];
                for b in raw.iter_mut() {
                    *b = self.rng.next_u64() as u8;
                }
                Value::LongDouble(raw)
            }

            TypeCode::String(bound) => Value::String(self.ascii(*bound)),
            TypeCode::WString(bound) => Value::WString(self.wide_text(*bound)),

            TypeCode::Enum { members, .. } => {
                let i = self.rng.below(members.len());
                Value::Enum(members.get(i)?.clone())
            }

            TypeCode::Struct { id, members, .. } | TypeCode::Except { id, members, .. } => {
                self.open.push((id.clone(), tc.clone()));
                let mut out = Vec::with_capacity(members.len());
                for m in members {
                    self.depth += 1;
                    let v = self.sample(&m.tc);
                    self.depth -= 1;
                    match v {
                        Some(v) => out.push((m.name.clone(), v)),
                        None => {
                            self.open.pop();
                            return None;
                        }
                    }
                }
                self.open.pop();
                Value::Struct(out)
            }

            TypeCode::Union { discriminator, cases, default_index, .. } => {
                self.union(discriminator, cases, *default_index)?
            }

            TypeCode::Sequence { element, bound } => {
                let cap =
                    if *bound == 0 { MAX_SEQUENCE } else { MAX_SEQUENCE.min(*bound as usize) };
                let mut n = self.rng.below(cap + 1);
                // A recursive or unmarshallable element still yields a valid
                // empty sequence, which is a real value and a real gap. Both
                // are recorded: the value goes on the wire, the gap goes in the
                // report.
                //
                // Asked at the elements' depth, not this sequence's: they are
                // sampled one level down, and their members one below that.
                if self.depth >= MAX_DEPTH || !self.can_sample_at(element, self.depth + 1) {
                    if let Some(reason) = self.gap_reason(element) {
                        self.gaps.insert(reason);
                    }
                    n = 0;
                }
                let mut out = Vec::with_capacity(n);
                for _ in 0..n {
                    self.depth += 1;
                    let v = self.sample(element);
                    self.depth -= 1;
                    out.push(v?);
                }
                Value::List(out)
            }

            TypeCode::Array { element, length } => {
                // No escape here: an array's length is in its type, so an
                // unmarshallable element makes the array unmarshallable.
                let mut out = Vec::with_capacity(*length as usize);
                for _ in 0..*length {
                    self.depth += 1;
                    let v = self.sample(element);
                    self.depth -= 1;
                    out.push(v?);
                }
                Value::List(out)
            }

            TypeCode::Any => {
                // The inner type is chosen from a fixed pool rather than
                // generated: an `any` carries a TypeCode on the wire, so the
                // pool is really a list of TypeCode encodings to exercise, and
                // a random one would mostly repeat `long`.
                let pool = [
                    TypeCode::Long,
                    TypeCode::String(0),
                    TypeCode::Boolean,
                    TypeCode::Double,
                    TypeCode::Sequence { element: Box::new(TypeCode::Octet), bound: 0 },
                    TypeCode::WString(0),
                ];
                let inner_tc = pool[self.rng.below(pool.len())].clone();
                self.depth += 1;
                let inner = self.sample(&inner_tc);
                self.depth -= 1;
                Value::Any(Box::new(inner_tc), Box::new(inner?))
            }

            TypeCode::ObjRef { id, .. } => {
                // Both halves matter. Nil is an empty type id with no profiles
                // — an easy thing to encode as an absent field by mistake — and
                // a live reference exercises the profile encapsulation, whose
                // alignment restarts at its own first byte.
                if self.rng.one_in(3) {
                    Value::ObjRef(None)
                } else {
                    Value::ObjRef(Some(self.ior(id)))
                }
            }

            // `tk_TypeCode` as a value in its own right (D008). The pool is
            // deliberately not the `any` pool: what is being exercised here is
            // the TypeCode *encoding* — its own indirections and string
            // encapsulations — rather than a value described by one, so it
            // reaches for the constructed shapes the `any` pool leaves out.
            TypeCode::TypeCode => {
                let pool = [
                    TypeCode::Long,
                    TypeCode::String(0),
                    TypeCode::WString(64),
                    TypeCode::Sequence { element: Box::new(TypeCode::Double), bound: 0 },
                    TypeCode::Array { element: Box::new(TypeCode::Octet), length: 4 },
                    TypeCode::Alias {
                        id: "IDL:prop/Meters:1.0".into(),
                        name: "Meters".into(),
                        aliased: Box::new(TypeCode::Long),
                    },
                    TypeCode::ObjRef { id: "IDL:prop/I:1.0".into(), name: "I".into() },
                    TypeCode::Any,
                ];
                Value::TypeCode(Box::new(pool[self.rng.below(pool.len())].clone()))
            }

            // `fixed` is not marshalled in v1 (§4.4) and `Principal` has no
            // `Value` at all. Saying so beats generating something the encoder
            // will reject and calling it a failure.
            // The recursive arm, resolved against the type it names rather than
            // abandoned. Depth is what terminates this, not the type: at
            // MAX_DEPTH the enclosing sequence has already been forced empty,
            // so the expansion is finite even though the type is not.
            TypeCode::Recursive(id) => {
                let open = self.open.iter().rev().find(|(k, _)| k == id)?.1.clone();
                if self.depth >= MAX_DEPTH {
                    return None;
                }
                self.sample(&open)?
            }

            TypeCode::Fixed { .. } | TypeCode::Principal => return None,
        })
    }

    /// Whether `sample` would succeed here, without consuming randomness.
    fn can_sample(&self, tc: &TypeCode) -> bool {
        self.can_sample_at(tc, self.depth)
    }

    /// Whether `sample` would succeed at `depth`, without consuming randomness.
    ///
    /// Kept separate so that deciding to emit an empty sequence does not shift
    /// every later draw — a predicate that advanced the generator would make
    /// the seed depend on the shape of the type in a way nobody could follow.
    ///
    /// The depth is walked exactly as `sample` walks it — a member or an
    /// element is one level below its container — because the question is
    /// only useful if it is the same question `sample` will answer. It was not:
    /// the recursive arm asked at "one level below the sequence", which is
    /// where the *element* is sampled and not where the element's *members*
    /// are, so `corpus/golden/15`'s `TreeSeq` passed the guard, went one level
    /// deeper than the guard had checked, hit [`MAX_DEPTH`] as a struct
    /// member (which has no empty case to fall back to), and the whole sample
    /// came back `None`. Measured 2026-08-19 over the 32 default seeds:
    /// 22 cases produced no value and were skipped without a finding, and the
    /// 10 that survived were all the empty list. The recursive witness for
    /// that type was the empty list, and the report was green.
    fn can_sample_at(&self, tc: &TypeCode, depth: u32) -> bool {
        match tc {
            TypeCode::Alias { aliased, .. } => self.can_sample_at(aliased, depth),
            TypeCode::Fixed { .. } | TypeCode::Principal => false,
            // Samplable exactly when the type it names is under way, there is
            // depth left to expand it into, and the type it names is itself
            // samplable from here — a cycle with no sequence in it (`struct
            // Loop { Loop me; }`) has no finite value at any depth.
            TypeCode::Recursive(id) => {
                depth < MAX_DEPTH
                    && self
                        .open
                        .iter()
                        .rev()
                        .find(|(k, _)| k == id)
                        .is_some_and(|(_, open)| self.can_sample_at(open, depth))
            }
            TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
                members.iter().all(|m| self.can_sample_at(&m.tc, depth + 1))
            }
            TypeCode::Array { element, .. } => self.can_sample_at(element, depth + 1),
            // A sequence is always samplable: worst case it is empty.
            TypeCode::Sequence { .. } => true,
            TypeCode::Union { discriminator, cases, .. } => {
                self.can_sample_at(discriminator, depth)
                    && cases.iter().all(|c| self.can_sample_at(&c.tc, depth + 1))
            }
            _ => true,
        }
    }

    fn gap_reason(&self, element: &TypeCode) -> Option<String> {
        match element {
            TypeCode::Alias { aliased, .. } => self.gap_reason(aliased),
            // Only a gap when the cycle cannot be resolved at all. A recursive
            // arm that ran and then stopped at MAX_DEPTH left a real tree on
            // the wire; reporting that as unmeasured would be the report
            // lying in the safe direction, which is still lying.
            TypeCode::Recursive(id) if !self.open.iter().any(|(k, _)| k == id) => Some(format!(
                "every generated sequence of {id} is empty because the type is recursive and \
                 the type it names is not under construction here, so the cycle cannot be \
                 resolved; the recursive arm is unmeasured"
            )),
            TypeCode::Recursive(_) => None,
            _ if !self.can_sample(element) => Some(format!(
                "every generated sequence of {} is empty because {}",
                describe(element),
                why_unsupported(element)
            )),
            // Depth, not the type: nothing is missing from the type system.
            _ => None,
        }
    }

    fn union(
        &mut self,
        disc: &TypeCode,
        cases: &[orbweaver_giop::typecode::UnionCase],
        default_index: i32,
    ) -> Option<Value> {
        if cases.is_empty() {
            return None;
        }
        let pick = self.rng.below(cases.len());
        let case = &cases[pick];
        // A default branch is stored with an empty label, because it is
        // selected by *not* matching. Encoding an empty label back into a
        // discriminator is impossible, so the default is reached by searching
        // for a value no explicit label claims.
        let d = if case.label.is_empty() {
            self.unmatched_discriminator(disc, cases)?
        } else {
            value_from_label(disc, &case.label)?
        };
        let selected = if case.label.is_empty() && default_index < 0 {
            // A union with no default whose only branch is unlabelled cannot be
            // built: any discriminator selects nothing.
            return None;
        } else {
            case
        };
        self.depth += 1;
        let inner = self.sample(&selected.tc);
        self.depth -= 1;
        Some(Value::Union { discriminator: Box::new(d), value: Some(Box::new(inner?)) })
    }

    /// A discriminator value that no explicit case label matches.
    ///
    /// Encoded big-endian to compare against the stored labels, which the
    /// registry writes in the discriminator's own wire width — the same
    /// comparison `orbweaver_dynamic::select_case` performs.
    fn unmatched_discriminator(
        &mut self,
        disc: &TypeCode,
        cases: &[orbweaver_giop::typecode::UnionCase],
    ) -> Option<Value> {
        for candidate in 0..128u32 {
            let v = integer_value(disc, candidate as i64)?;
            let mut e = Encoder::new(Endian::Big);
            if orbweaver_dynamic::encode(&mut e, disc, &v).is_err() {
                continue;
            }
            let Ok(label) = e.finish() else { continue };
            if !cases.iter().any(|c| c.label == label) {
                return Some(v);
            }
        }
        None
    }

    /// A boundary value one time in four, otherwise a uniform draw.
    ///
    /// Boundaries are where sign extension and width truncation go wrong, and a
    /// uniform draw over 2^64 reaches `i32::MIN` approximately never.
    fn int(&mut self, min: i64, max: i64) -> i64 {
        if self.rng.one_in(4) {
            let edges = [min, max, 0, -1, 1, min + 1, max - 1];
            return edges[self.rng.below(edges.len())];
        }
        let span = (max as i128) - (min as i128) + 1;
        (min as i128 + (self.rng.next_u64() as i128 % span)) as i64
    }

    fn uint(&mut self, max: u64) -> u64 {
        if self.rng.one_in(4) {
            let edges = [0, max, 1, max - 1];
            return edges[self.rng.below(edges.len())];
        }
        if max == u64::MAX { self.rng.next_u64() } else { self.rng.next_u64() % (max + 1) }
    }

    /// Finite floats only.
    ///
    /// A NaN round-trips bit-for-bit through CDR and compares unequal to
    /// itself, so generating one would report a defect that is not there. The
    /// gap is named rather than hidden: NaN and the infinities are covered by
    /// hand-written cases in `orbweaver-cdr`, not here.
    fn float32(&mut self) -> f32 {
        let edges =
            [0.0f32, -0.0, 1.0, -1.0, f32::MIN, f32::MAX, f32::MIN_POSITIVE, f32::EPSILON, 0.5];
        if self.rng.one_in(3) {
            return edges[self.rng.below(edges.len())];
        }
        let v = f32::from_bits(self.rng.next_u64() as u32);
        if v.is_finite() { v } else { edges[self.rng.below(edges.len())] }
    }

    fn float64(&mut self) -> f64 {
        let edges =
            [0.0f64, -0.0, 1.0, -1.0, f64::MIN, f64::MAX, f64::MIN_POSITIVE, f64::EPSILON, 0.5];
        if self.rng.one_in(3) {
            return edges[self.rng.below(edges.len())];
        }
        let v = f64::from_bits(self.rng.next_u64());
        if v.is_finite() { v } else { edges[self.rng.below(edges.len())] }
    }

    /// Printable ASCII, honouring the bound.
    ///
    /// No NUL: a CDR string is NUL-terminated, so an embedded NUL is outside
    /// the type rather than an interesting case, and generating one would
    /// report a defect against a value IDL cannot express.
    fn ascii(&mut self, bound: u32) -> String {
        let cap = if bound == 0 { 12 } else { 12.min(bound as usize) };
        let n = self.rng.below(cap + 1);
        (0..n).map(|_| (0x20 + self.rng.below(0x5F) as u8) as char).collect()
    }

    /// BMP characters, weighted toward the codesets §8 cares about.
    ///
    /// Korean is in the pool on purpose: the codeset row of the verification
    /// table is about EUC-KR and UTF-16 round-trips, and a generator that only
    /// produced ASCII would report full coverage of `wstring` while measuring
    /// none of it. Restricted to the BMP because a surrogate pair is a
    /// `WideCodec` question rather than a `Value` one.
    fn wide_char(&mut self) -> char {
        let pool = ['A', '한', '글', '€', 'é', 'あ', '中', '\u{7F}', 'ᄀ', 'Ω'];
        pool[self.rng.below(pool.len())]
    }

    fn wide_text(&mut self, bound: u32) -> String {
        let words = ["안녕하세요", "orbweaver", "한글", "διεθνές", "テスト", ""];
        let mut s = String::new();
        let cap = if bound == 0 { usize::MAX } else { bound as usize };
        for _ in 0..=self.rng.below(2) {
            let w = words[self.rng.below(words.len())];
            if s.encode_utf16().count() + w.encode_utf16().count() > cap {
                break;
            }
            s.push_str(w);
        }
        s
    }

    /// A synthetic reference that is structurally a real one.
    ///
    /// §4.7: object references marshal inline, and the profile inside is an
    /// encapsulation whose alignment restarts at its own first byte. A nil-only
    /// generator would never encode one.
    fn ior(&mut self, type_id: &str) -> Ior {
        let key: Vec<u8> = (0..1 + self.rng.below(8)).map(|_| self.rng.next_u64() as u8).collect();
        Ior {
            type_id: type_id.to_owned(),
            profiles: vec![IiopProfile {
                version: Version::V1_2,
                host: "127.0.0.1".into(),
                port: 1024 + self.rng.below(60_000) as u16,
                object_key: key,
                components: Vec::new(),
            }],
        }
    }
}

/// Builds a discriminator value of `disc` from an integer, for label matching.
fn integer_value(disc: &TypeCode, n: i64) -> Option<Value> {
    Some(match disc.resolve_alias() {
        TypeCode::Boolean => Value::Bool(n != 0),
        TypeCode::Char => Value::Char(n as u8),
        TypeCode::Octet => Value::Octet(n as u8),
        TypeCode::Short => Value::Short(n as i16),
        TypeCode::UShort => Value::UShort(n as u16),
        TypeCode::Long => Value::Long(n as i32),
        TypeCode::ULong => Value::ULong(n as u32),
        TypeCode::LongLong => Value::LongLong(n),
        TypeCode::ULongLong => Value::ULongLong(n as u64),
        TypeCode::Enum { members, .. } => Value::Enum(members.get(n as usize)?.clone()),
        _ => return None,
    })
}

/// Reads a stored case label back into a discriminator value.
///
/// Labels are stored big-endian in the discriminator's own width (see
/// `orbweaver_registry::label_bytes`), which is what makes this the inverse of
/// the encoder rather than a second opinion about it.
fn value_from_label(disc: &TypeCode, label: &[u8]) -> Option<Value> {
    let be_i64 = |b: &[u8]| -> i64 {
        let mut v: i64 = 0;
        for x in b {
            v = (v << 8) | *x as i64;
        }
        v
    };
    Some(match disc.resolve_alias() {
        TypeCode::Boolean => Value::Bool(*label.first()? != 0),
        TypeCode::Char => Value::Char(*label.first()?),
        TypeCode::Octet => Value::Octet(*label.first()?),
        TypeCode::Short => Value::Short(i16::from_be_bytes(label.try_into().ok()?)),
        TypeCode::UShort => Value::UShort(u16::from_be_bytes(label.try_into().ok()?)),
        TypeCode::Long => Value::Long(i32::from_be_bytes(label.try_into().ok()?)),
        TypeCode::ULong => Value::ULong(u32::from_be_bytes(label.try_into().ok()?)),
        TypeCode::LongLong => Value::LongLong(i64::from_be_bytes(label.try_into().ok()?)),
        TypeCode::ULongLong => Value::ULongLong(u64::from_be_bytes(label.try_into().ok()?)),
        TypeCode::Enum { members, .. } => Value::Enum(members.get(be_i64(label) as usize)?.clone()),
        _ => return None,
    })
}

/// Why a type has no generated value, phrased as a limit rather than a fault.
fn why_unsupported(tc: &TypeCode) -> &'static str {
    match tc {
        TypeCode::Alias { aliased, .. } => why_unsupported(aliased),
        TypeCode::Fixed { .. } => "`fixed` parses but v1 does not marshal it (docs/PLAN.md §4.4)",
        TypeCode::TypeCode => "a bare `TypeCode` has no `Value` representation in the dynamic path",
        TypeCode::Principal => "`Principal` is withdrawn from CORBA and is not marshalled",
        TypeCode::Recursive(_) => "the type is recursive and has no finite expansion",
        TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => members
            .iter()
            .find(|m| {
                !Sampler { rng: Rng::new(1), gaps: BTreeSet::new(), depth: 0, open: Vec::new() }
                    .can_sample(&m.tc)
            })
            .map(|m| why_unsupported(&m.tc))
            .unwrap_or("a member cannot be sampled"),
        TypeCode::Array { element, .. } => why_unsupported(element),
        TypeCode::Union { .. } => "no branch of the union can be sampled",
        _ => "the sampler has no case for it",
    }
}

/// A short name for messages.
fn describe(tc: &TypeCode) -> String {
    match tc {
        TypeCode::Struct { name, .. }
        | TypeCode::Union { name, .. }
        | TypeCode::Enum { name, .. }
        | TypeCode::Except { name, .. }
        | TypeCode::Alias { name, .. }
        | TypeCode::ObjRef { name, .. } => name.clone(),
        TypeCode::Sequence { element, .. } => format!("sequence<{}>", describe(element)),
        TypeCode::Array { element, length } => format!("{}[{length}]", describe(element)),
        other => format!("{other:?}").split(' ').next().unwrap_or("a type").to_lowercase(),
    }
}

/// The repository id when the type has one, so a finding names something
/// greppable.
fn type_id(tc: &TypeCode) -> String {
    match tc {
        TypeCode::Struct { id, .. }
        | TypeCode::Union { id, .. }
        | TypeCode::Enum { id, .. }
        | TypeCode::Except { id, .. }
        | TypeCode::Alias { id, .. }
        | TypeCode::ObjRef { id, .. } => id.clone(),
        TypeCode::Recursive(id) => id.clone(),
        other => describe(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_giop::typecode::{Member, UnionCase};

    fn tc_struct(name: &str, members: Vec<(&str, TypeCode)>) -> TypeCode {
        TypeCode::Struct {
            id: format!("IDL:m/{name}:1.0"),
            name: name.into(),
            members: members.into_iter().map(|(n, tc)| Member { name: n.into(), tc }).collect(),
        }
    }

    /// The seed discipline, as a test rather than a paragraph.
    #[test]
    fn the_same_seed_produces_the_same_value_every_time() {
        let tc = tc_struct(
            "S",
            vec![("a", TypeCode::Long), ("b", TypeCode::String(0)), ("c", TypeCode::Double)],
        );
        let a = sample(&tc, 0xDEAD_BEEF).expect("sampled");
        let b = sample(&tc, 0xDEAD_BEEF).expect("sampled");
        assert_eq!(a, b, "a seed is a value");
        // And a different seed is a different value, or the seed is decorative.
        let c = sample(&tc, 0xDEAD_BEF0).expect("sampled");
        assert_ne!(a, c);
    }

    #[test]
    fn case_seeds_are_distinct_and_stable() {
        let seeds: Vec<u64> = (0..64).map(|i| case_seed(DEFAULT_SEED, i)).collect();
        let unique: BTreeSet<u64> = seeds.iter().copied().collect();
        assert_eq!(unique.len(), seeds.len(), "adjacent cases must not collide");
        assert_eq!(seeds[0], case_seed(DEFAULT_SEED, 0), "and must not drift within a run");
    }

    /// The whole point: a finding names a seed, and the seed alone reproduces.
    #[test]
    fn a_reported_seed_reproduces_its_case_without_the_batch() {
        let tc = tc_struct("R", vec![("a", TypeCode::Octet), ("b", TypeCode::Double)]);
        let seed = case_seed(DEFAULT_SEED, 5);
        let from_batch = sample(&tc, seed);
        assert!(from_batch.is_some());
        // roundtrip_case must at minimum agree about what value that seed means.
        assert_eq!(from_batch, sample(&tc, seed));
        assert!(roundtrip_case(&tc, seed).is_empty(), "and it must pass");
    }

    #[test]
    fn every_primitive_is_byte_stable_in_both_orders_at_every_phase() {
        for tc in [
            TypeCode::Boolean,
            TypeCode::Octet,
            TypeCode::Char,
            TypeCode::WChar,
            TypeCode::Short,
            TypeCode::UShort,
            TypeCode::Long,
            TypeCode::ULong,
            TypeCode::LongLong,
            TypeCode::ULongLong,
            TypeCode::Float,
            TypeCode::Double,
            TypeCode::LongDouble,
            TypeCode::String(0),
            TypeCode::String(4),
            TypeCode::WString(0),
        ] {
            let findings = roundtrip_property(&tc, 24);
            assert!(findings.is_empty(), "{tc:?}: {findings:?}");
        }
    }

    /// The alignment case: members of different widths force padding, and the
    /// phase decides where it lands.
    #[test]
    fn a_ragged_struct_is_byte_stable_at_every_alignment_phase() {
        let tc = tc_struct(
            "Ragged",
            vec![
                ("a", TypeCode::Octet),
                ("b", TypeCode::Long),
                ("c", TypeCode::Short),
                ("d", TypeCode::Double),
                ("e", TypeCode::Octet),
                ("f", TypeCode::LongDouble),
            ],
        );
        let findings = roundtrip_property(&tc, 32);
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn compound_types_are_byte_stable() {
        let inner = tc_struct("Inner", vec![("x", TypeCode::Short), ("y", TypeCode::Double)]);
        for tc in [
            TypeCode::Sequence { element: Box::new(inner.clone()), bound: 0 },
            TypeCode::Sequence { element: Box::new(TypeCode::Octet), bound: 3 },
            TypeCode::Array { element: Box::new(TypeCode::Long), length: 3 },
            TypeCode::Any,
            TypeCode::Alias {
                id: "IDL:m/A:1.0".into(),
                name: "A".into(),
                aliased: Box::new(inner.clone()),
            },
            TypeCode::Enum {
                id: "IDL:m/E:1.0".into(),
                name: "E".into(),
                members: vec!["RED".into(), "GREEN".into()],
            },
            TypeCode::ObjRef { id: "IDL:m/I:1.0".into(), name: "I".into() },
            TypeCode::Except {
                id: "IDL:m/X:1.0".into(),
                name: "X".into(),
                members: vec![Member { name: "code".into(), tc: TypeCode::Long }],
            },
        ] {
            let findings = roundtrip_property(&tc, 24);
            assert!(findings.is_empty(), "{tc:?}: {findings:?}");
        }
    }

    #[test]
    fn unions_reach_both_labelled_and_default_branches() {
        let tc = TypeCode::Union {
            id: "IDL:m/U:1.0".into(),
            name: "U".into(),
            discriminator: Box::new(TypeCode::Long),
            default_index: 2,
            cases: vec![
                UnionCase {
                    label: 1i32.to_be_bytes().to_vec(),
                    name: "one".into(),
                    tc: TypeCode::Long,
                },
                UnionCase {
                    label: 2i32.to_be_bytes().to_vec(),
                    name: "two".into(),
                    tc: TypeCode::String(0),
                },
                UnionCase { label: Vec::new(), name: "rest".into(), tc: TypeCode::Boolean },
            ],
        };
        let findings = roundtrip_property(&tc, 48);
        assert!(findings.is_empty(), "{findings:?}");

        // And the default branch is actually reached, or the union is only
        // half tested and the report would not say so.
        let reached: BTreeSet<String> = (0..48)
            .filter_map(|i| sample(&tc, case_seed(DEFAULT_SEED, i)))
            .map(|v| match v {
                Value::Union { discriminator, .. } => format!("{discriminator:?}"),
                other => format!("{other:?}"),
            })
            .collect();
        assert!(reached.len() >= 3, "labelled and default branches: {reached:?}");
    }

    /// The recursive arm is now generated, so the property actually exercises
    /// it. Before this, every `TreeSeq` came out empty: the round trip passed
    /// on values that contained no recursion at all, and the marshaller's
    /// recursive path — which turned out not to exist — was reported as a gap
    /// rather than run.
    #[test]
    fn a_resolvable_cycle_is_generated_rather_than_reported() {
        let tree = TypeCode::Struct {
            id: "IDL:m/Tree:1.0".into(),
            name: "Tree".into(),
            members: vec![
                orbweaver_giop::typecode::Member { name: "label".into(), tc: TypeCode::String(0) },
                orbweaver_giop::typecode::Member {
                    name: "kids".into(),
                    tc: TypeCode::Sequence {
                        element: Box::new(TypeCode::Recursive("IDL:m/Tree:1.0".into())),
                        bound: 0,
                    },
                },
            ],
        };
        let findings = roundtrip_property(&tree, 32);
        assert!(findings.is_empty(), "{findings:?}");

        // And at least one of those cases actually had a child, over the fixed
        // batch seed — otherwise this test would pass on the old behaviour.
        let grew = (0..32u64).any(|i| {
            let v = sample(&tree, case_seed(DEFAULT_SEED, i));
            matches!(v, Some(Value::Struct(ref m)) if matches!(&m[1].1, Value::List(k) if !k.is_empty()))
        });
        assert!(grew, "no generated tree had a child; the recursive arm is still unmeasured");
    }

    /// The other spelling of the same cycle — the one `corpus/golden/15`
    /// produces for `TreeSeq`, where the marker is a struct *member* naming
    /// the typedef rather than a sequence element naming the struct. Its
    /// witness was the empty list on every measured case and `None` on the
    /// rest (22 of 32 over the default seeds, skipped without a finding),
    /// because the predicate that decides whether a sequence may be non-empty
    /// asked its question one level higher than the sampler then went. Every
    /// case must now produce a value, none may fall through silently, and at
    /// least one over the batch seed must actually contain a tree.
    #[test]
    fn a_cycle_through_a_typedef_member_is_generated_on_every_case() {
        let tree_seq = TypeCode::Alias {
            id: "IDL:m/TreeSeq:1.0".into(),
            name: "TreeSeq".into(),
            aliased: Box::new(TypeCode::Sequence {
                element: Box::new(tc_struct(
                    "Tree",
                    vec![
                        ("label", TypeCode::String(0)),
                        ("kids", TypeCode::Recursive("IDL:m/TreeSeq:1.0".into())),
                    ],
                )),
                bound: 0,
            }),
        };
        let findings = roundtrip_property(&tree_seq, 32);
        assert!(findings.is_empty(), "{findings:?}");

        let mut produced = 0;
        let mut non_empty = 0;
        for i in 0..32u64 {
            match sample(&tree_seq, case_seed(DEFAULT_SEED, i)) {
                Some(Value::List(items)) => {
                    produced += 1;
                    if !items.is_empty() {
                        non_empty += 1;
                    }
                }
                other => panic!("case {i}: {other:?}"),
            }
        }
        assert_eq!(produced, 32, "a case that produces no value has run nothing");
        assert!(non_empty > 0, "every TreeSeq was empty; the recursive witness measures nothing");
    }

    /// A cycle that cannot be resolved still generates empty sequences and
    /// still *says so*. An unmeasured arm reported as covered is the harness
    /// failure `CLAUDE.md` warns about.
    ///
    /// This test used to assert that of *every* recursive type, which was the
    /// old behaviour and not a property worth having: the marker below names an
    /// id no enclosing type has, so nothing can expand it, whereas a marker
    /// naming its own enclosing type now expands and is covered by
    /// `a_resolvable_cycle_is_generated_rather_than_reported`. The distinction
    /// is the point — one is a limit of the type, the other was a limit of the
    /// generator.
    #[test]
    fn an_unresolvable_cycle_reports_its_unmeasured_arm() {
        let tc = tc_struct(
            "Tree",
            vec![
                ("label", TypeCode::String(0)),
                (
                    "kids",
                    TypeCode::Sequence {
                        element: Box::new(TypeCode::Recursive("IDL:m/Elsewhere:1.0".into())),
                        bound: 0,
                    },
                ),
            ],
        );
        let findings = roundtrip_property(&tc, 16);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "prop/unmeasured");
        assert_eq!(findings[0].severity, Severity::Advice, "a gap is not a defect");
        assert!(findings[0].message.contains("recursive"), "{}", findings[0].message);
    }

    /// `fixed` parses and does not marshal. The property reports a coverage
    /// gap, not a failure — the limit is documented in §4.4 and known.
    #[test]
    fn an_unmarshallable_type_is_a_coverage_gap_rather_than_a_defect() {
        // Two findings since the JSON leg landed — `fixed` is also
        // `json/unmapped`, which the test below pins; this one is about the
        // sampler's half.
        let findings: Vec<Finding> =
            roundtrip_property(&TypeCode::Fixed { digits: 9, scale: 2 }, 8)
                .into_iter()
                .filter(|f| f.rule.starts_with("prop/"))
                .collect();
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "prop/unsupported-type");
        assert_eq!(findings[0].severity, Severity::Advice);
        assert!(findings[0].message.contains("§4.4"), "{}", findings[0].message);
    }

    /// The property must be able to fail, or it is decoration. There is no
    /// injectable encoder, so the check is made against a TypeCode that lies
    /// about the type it describes: a two-member struct decoded as a
    /// three-member one.
    #[test]
    fn the_property_actually_reports_a_mismatch() {
        let honest = tc_struct("S", vec![("a", TypeCode::Long), ("b", TypeCode::Long)]);
        let value = sample(&honest, 7).expect("sampled");
        let lying = tc_struct(
            "S",
            vec![("a", TypeCode::Long), ("b", TypeCode::Long), ("c", TypeCode::Long)],
        );
        let mut e = Encoder::new(Endian::Big);
        orbweaver_dynamic::encode(&mut e, &honest, &value).expect("encodes");
        let bytes = e.finish().expect("finish");
        let mut d = Decoder::new(&bytes, Endian::Big);
        assert!(
            orbweaver_dynamic::decode(&mut d, &lying).is_err(),
            "the decoder must not invent the third member"
        );
        // And the finding for an encode failure carries the seed.
        let mut m = Measured::default();
        let f = one_case(&lying, &value, 0x1234, Endian::Big, 0, true, &mut m);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(m, Measured::default(), "a case that failed to encode measured nothing");
        assert_eq!(f[0].severity, Severity::Error);
        assert!(f[0].source.contains("seed=0x0000000000001234"), "{}", f[0].source);
        assert!(f[0].fix.as_deref().unwrap().contains("roundtrip_case"), "{f:?}");
    }

    /// Zero cases measures nothing and must not look like a pass with content.
    #[test]
    fn zero_cases_produces_nothing() {
        let (findings, measured) = roundtrip_property_measured(&TypeCode::Long, 0, DEFAULT_SEED);
        assert!(findings.is_empty());
        assert_eq!(measured, Measured::default());
    }

    /// The JSON leg runs for every value the CDR leg ran, and says so in the
    /// count. A leg that quietly stopped would leave the findings identical
    /// and this ratio short.
    #[test]
    fn every_value_the_cdr_leg_ran_is_also_taken_across_anyjson() {
        let inner = tc_struct("Inner", vec![("x", TypeCode::Short), ("y", TypeCode::Double)]);
        let tree = TypeCode::Struct {
            id: "IDL:m/Tree:1.0".into(),
            name: "Tree".into(),
            members: vec![
                Member { name: "label".into(), tc: TypeCode::String(0) },
                Member {
                    name: "kids".into(),
                    tc: TypeCode::Sequence {
                        element: Box::new(TypeCode::Recursive("IDL:m/Tree:1.0".into())),
                        bound: 0,
                    },
                },
            ],
        };
        for tc in [
            tc_struct(
                "Ragged",
                vec![
                    ("a", TypeCode::Octet),
                    ("b", TypeCode::LongLong),
                    ("c", TypeCode::Float),
                    ("d", TypeCode::Double),
                    ("e", TypeCode::WChar),
                    ("f", TypeCode::LongDouble),
                    ("g", TypeCode::WString(0)),
                ],
            ),
            TypeCode::Sequence { element: Box::new(inner.clone()), bound: 0 },
            TypeCode::Sequence { element: Box::new(TypeCode::Octet), bound: 3 },
            TypeCode::Array { element: Box::new(TypeCode::Long), length: 3 },
            TypeCode::Any,
            TypeCode::TypeCode,
            TypeCode::Alias {
                id: "IDL:m/A:1.0".into(),
                name: "A".into(),
                aliased: Box::new(inner),
            },
            TypeCode::Enum {
                id: "IDL:m/E:1.0".into(),
                name: "E".into(),
                members: vec!["RED".into(), "GREEN".into()],
            },
            TypeCode::ObjRef { id: "IDL:m/I:1.0".into(), name: "I".into() },
            TypeCode::Union {
                id: "IDL:m/U:1.0".into(),
                name: "U".into(),
                discriminator: Box::new(TypeCode::Long),
                default_index: 2,
                cases: vec![
                    UnionCase {
                        label: 1i32.to_be_bytes().to_vec(),
                        name: "one".into(),
                        tc: TypeCode::Long,
                    },
                    UnionCase {
                        label: 2i32.to_be_bytes().to_vec(),
                        name: "two".into(),
                        tc: TypeCode::String(0),
                    },
                    UnionCase { label: Vec::new(), name: "rest".into(), tc: TypeCode::Boolean },
                ],
            },
            tree,
        ] {
            let (findings, measured) = roundtrip_property_measured(&tc, 24, DEFAULT_SEED);
            assert!(findings.is_empty(), "{tc:?}: {findings:?}");
            assert_eq!(measured.cdr, 48, "{tc:?}: 24 cases × 2 byte orders");
            assert_eq!(measured.json, 48, "{tc:?}: every one of them across AnyJSON too");
        }
    }

    /// A type the mapping cannot carry is a named class, once, and the count
    /// shows the leg did not run — not a pass and not silence.
    #[test]
    fn a_type_anyjson_cannot_carry_is_reported_as_unmapped_and_not_counted() {
        // `fixed`: unsampled by the CDR leg (§4.4) *and* unmapped, and both are
        // said, because they are two facts about two modules.
        let (findings, measured) =
            roundtrip_property_measured(&TypeCode::Fixed { digits: 9, scale: 2 }, 8, DEFAULT_SEED);
        let rules: Vec<&str> = findings.iter().map(|f| f.rule.as_str()).collect();
        assert_eq!(rules, ["json/unmapped", "prop/unsupported-type"], "{findings:?}");
        assert!(findings.iter().all(|f| f.severity == Severity::Advice), "{findings:?}");
        assert!(findings[0].message.contains("cannot cross yet"), "{}", findings[0].message);
        assert_eq!(measured, Measured::default());

        // `void` where a type belongs: the CDR leg runs (void marshals to
        // nothing), the JSON leg does not, and the ratio says so.
        let tc = tc_struct("Odd", vec![("a", TypeCode::Long), ("nothing", TypeCode::Void)]);
        let (findings, measured) = roundtrip_property_measured(&tc, 8, DEFAULT_SEED);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert_eq!(findings[0].rule, "json/unmapped");
        assert_eq!(measured, Measured { cdr: 16, json: 0 });
    }

    /// The JSON leg must be able to fail, or it is decoration — and it must
    /// fail on the *bytes* when the value survives, because that is the class
    /// value equality forgives. There is no injectable mapping, so both arms
    /// are driven with the honest type against a value or bytes that lie.
    #[test]
    fn the_json_leg_actually_reports_a_mismatch() {
        let tc = tc_struct("S", vec![("a", TypeCode::Long), ("b", TypeCode::Double)]);
        let value = sample(&tc, 7).expect("sampled");
        let mut e = Encoder::new(Endian::Big);
        orbweaver_dynamic::encode(&mut e, &tc, &value).expect("encodes");
        let honest = e.finish().expect("finish");

        // The value crosses; the bytes it is compared with are wrong by one
        // octet — what a mapping that flattened `-0.0` to `0.0` would produce.
        let mut lying = honest.clone();
        *lying.last_mut().unwrap() ^= 0x01;
        let f = json_leg(&tc, &value, &lying, Endian::Big, 0, "here", "seed");
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].rule, "json/roundtrip-bytes");
        assert_eq!(f[0].severity, Severity::Error);
        assert!(f[0].message.contains(&format!("offset {}", honest.len() - 1)), "{}", f[0].message);
        assert!(f[0].message.contains("document {"), "{}", f[0].message);
        assert_eq!(f[0].source, "here");
        assert_eq!(f[0].fix.as_deref(), Some("seed"));

        // And a value the mapping writes but will not read back — a member
        // short, the shape of the negative control this leg was landed with —
        // is the class that names the type and the member.
        let short = Value::Struct(vec![("a".into(), Value::Long(1))]);
        let f = json_leg(&tc, &short, &honest, Endian::Big, 0, "here", "seed");
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].rule, "json/from-json-error");
        assert!(f[0].message.contains("S needs a member \"b\""), "{}", f[0].message);
    }

    /// The reproduction entry point runs the JSON leg too, so a `json/*`
    /// finding's seed reproduces the same way a `prop/*` one does — and the
    /// unmapped predicate is honoured there as well.
    #[test]
    fn roundtrip_case_takes_the_json_leg_and_honours_unmapped() {
        let tc = tc_struct("R", vec![("a", TypeCode::Octet), ("b", TypeCode::Double)]);
        assert!(roundtrip_case(&tc, case_seed(DEFAULT_SEED, 3)).is_empty());
        let odd = tc_struct("Odd", vec![("a", TypeCode::Long), ("nothing", TypeCode::Void)]);
        assert!(roundtrip_case(&odd, case_seed(DEFAULT_SEED, 3)).is_empty());
    }
}
