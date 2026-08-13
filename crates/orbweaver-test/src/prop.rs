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

use std::collections::BTreeSet;

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_dynamic::Value;
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
    let mut out = Vec::new();
    let mut gaps = BTreeSet::new();

    if cases == 0 {
        return out;
    }

    let mut sampler = Sampler { rng: Rng::new(root), gaps: BTreeSet::new(), depth: 0 };
    if sampler.sample(tc).is_none() {
        return vec![finding(
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
        )];
    }

    for index in 0..cases {
        let seed = case_seed(root, index as u64);
        let phase = index % 8;
        let mut sampler = Sampler { rng: Rng::new(seed), gaps: BTreeSet::new(), depth: 0 };
        let Some(value) = sampler.sample(tc) else { continue };
        gaps.append(&mut sampler.gaps);
        for endian in [Endian::Big, Endian::Little] {
            out.extend(one_case(tc, &value, seed, endian, phase));
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
    out
}

/// Re-runs one case, in both byte orders and at every alignment phase.
///
/// The reproduction entry point: a finding reports a seed, and this takes the
/// seed back to the failure without the batch that found it.
pub fn roundtrip_case(tc: &TypeCode, seed: u64) -> Vec<Finding> {
    let mut sampler = Sampler { rng: Rng::new(seed), gaps: BTreeSet::new(), depth: 0 };
    let Some(value) = sampler.sample(tc) else { return Vec::new() };
    let mut out = Vec::new();
    for phase in 0..8 {
        for endian in [Endian::Big, Endian::Little] {
            out.extend(one_case(tc, &value, seed, endian, phase));
        }
    }
    out
}

/// The sample value a seed produces for a type, for a caller that wants to see
/// it rather than round-trip it.
pub fn sample(tc: &TypeCode, seed: u64) -> Option<Value> {
    Sampler { rng: Rng::new(seed), gaps: BTreeSet::new(), depth: 0 }.sample(tc)
}

/// Encode → decode → encode, once, at one byte order and one alignment phase.
fn one_case(tc: &TypeCode, value: &Value, seed: u64, endian: Endian, phase: usize) -> Vec<Finding> {
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
            where_,
            Some(reproduce),
        )),
        Ok(_) => {}
        Err(e) => out.push(finding(
            "prop/encode-error",
            Severity::Error,
            format!("{} encoded, decoded, then failed to re-encode: {e}", describe(tc)),
            where_,
            Some(reproduce),
        )),
    }
    out
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
}

impl Sampler {
    /// A value of `tc`, or `None` when this project cannot marshal the type at
    /// all.
    fn sample(&mut self, tc: &TypeCode) -> Option<Value> {
        Some(match tc {
            TypeCode::Alias { aliased, .. } => self.sample(aliased)?,

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

            TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
                let mut out = Vec::with_capacity(members.len());
                for m in members {
                    self.depth += 1;
                    let v = self.sample(&m.tc);
                    self.depth -= 1;
                    out.push((m.name.clone(), v?));
                }
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
                if self.depth >= MAX_DEPTH || !self.can_sample(element) {
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

            // Neither is marshalled in v1 (§4.4), and `TypeCode`/`Principal`
            // have no `Value` at all. Saying so beats generating something the
            // encoder will reject and calling it a failure.
            TypeCode::Fixed { .. }
            | TypeCode::TypeCode
            | TypeCode::Principal
            | TypeCode::Recursive(_) => return None,
        })
    }

    /// Whether `sample` would succeed, without consuming randomness.
    ///
    /// Kept separate so that deciding to emit an empty sequence does not shift
    /// every later draw — a predicate that advanced the generator would make
    /// the seed depend on the shape of the type in a way nobody could follow.
    fn can_sample(&self, tc: &TypeCode) -> bool {
        match tc {
            TypeCode::Alias { aliased, .. } => self.can_sample(aliased),
            TypeCode::Fixed { .. }
            | TypeCode::TypeCode
            | TypeCode::Principal
            | TypeCode::Recursive(_) => false,
            TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
                members.iter().all(|m| self.can_sample(&m.tc))
            }
            TypeCode::Array { element, .. } => self.can_sample(element),
            // A sequence is always samplable: worst case it is empty.
            TypeCode::Sequence { .. } => true,
            TypeCode::Union { discriminator, cases, .. } => {
                self.can_sample(discriminator) && cases.iter().all(|c| self.can_sample(&c.tc))
            }
            _ => true,
        }
    }

    fn gap_reason(&self, element: &TypeCode) -> Option<String> {
        match element {
            TypeCode::Alias { aliased, .. } => self.gap_reason(aliased),
            TypeCode::Recursive(id) => Some(format!(
                "every generated sequence of {id} is empty because the type is recursive and \
                 has no finite expansion; the recursive arm is unmeasured"
            )),
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
                !Sampler { rng: Rng::new(1), gaps: BTreeSet::new(), depth: 0 }.can_sample(&m.tc)
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

    /// A recursive type generates empty sequences and *says so*. An unmeasured
    /// arm reported as covered is the harness failure `CLAUDE.md` warns about.
    #[test]
    fn a_recursive_type_reports_its_unmeasured_arm() {
        let tc = tc_struct(
            "Tree",
            vec![
                ("label", TypeCode::String(0)),
                (
                    "kids",
                    TypeCode::Sequence {
                        element: Box::new(TypeCode::Recursive("IDL:m/Tree:1.0".into())),
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
        let findings = roundtrip_property(&TypeCode::Fixed { digits: 9, scale: 2 }, 8);
        assert_eq!(findings.len(), 1);
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
        let f = one_case(&lying, &value, 0x1234, Endian::Big, 0);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].severity, Severity::Error);
        assert!(f[0].source.contains("seed=0x0000000000001234"), "{}", f[0].source);
        assert!(f[0].fix.as_deref().unwrap().contains("roundtrip_case"), "{f:?}");
    }

    /// Zero cases measures nothing and must not look like a pass with content.
    #[test]
    fn zero_cases_produces_nothing() {
        assert!(roundtrip_property(&TypeCode::Long, 0).is_empty());
    }
}
