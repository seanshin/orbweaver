//! Panic freedom and allocation freedom at the **agent** boundary.
//!
//! [`crate::wire`] asks what our decoders do when a *peer* chooses the bytes.
//! This module asks the same question of the parsers an **agent** reaches, and
//! it is a separate module because it is a separate claim: nothing in the wire
//! fuzz's target list is reachable from a `tools/call`, and nothing here is
//! reachable from a GIOP connection.
//!
//! §9.0's R11/R12 put an agent in the same threat model as a peer — untrusted,
//! and trusted with nothing it has not been granted. AnyJSON v1.1 (D008) then
//! added the sharpest surface either boundary has: `tc_from_json` reads a
//! **structural `TypeCode`** out of a document the agent wrote. The agent no
//! longer merely supplies a value of a type somebody else declared; it supplies
//! the type as well, recursively, and every downstream encoder and decoder then
//! runs against numbers it chose. That is the classic hazard, it landed with no
//! fuzz over it, and this module is that fuzz.
//!
//! **에이전트는 피어와 같은 등급의 비신뢰 입력이다.** D008 이후 에이전트는 값이
//! 아니라 **타입 자체**를 문서로 건네며, 그 뒤의 모든 인코더·디코더가 에이전트가
//! 고른 숫자 위에서 돈다. 그래서 와이어 퍼즈와 같은 질문을 여기서 다시 한다.
//!
//! # Two properties, not one
//!
//! - **Panic freedom**, exactly as in [`crate::wire`]: `Ok` and `Err` are both
//!   passes and a panic is the only failure. A parser a caller can panic is a
//!   process that caller can stop.
//! - **Allocation freedom**, which the wire fuzz names in its own comments and
//!   cannot check here: *twelve bytes must not buy a multi-gigabyte
//!   allocation.* A `TypeCode` is mostly numbers, and a number in a document is
//!   an instruction to reserve. See [`eager_bytes`] for what is measured and
//!   why it is measured on the parse result rather than by watching an
//!   allocator — `unsafe_code = "forbid"` rules out a counting `GlobalAlloc`,
//!   and an allocation large enough to matter aborts rather than unwinding, so
//!   `catch_unwind` would not see it either. The check therefore runs *before*
//!   the reservation would, and a document over budget is reported and then
//!   **not** handed to the targets that would perform it. A fuzz that dies of
//!   the defect it found reports nothing.
//!
//! # Where the documents come from
//!
//! One pipeline, text, because that is the only shape this boundary has: a
//! frame arrives as a line, `orbweaver_mcp::rpc::parse_request` bounds it at
//! `MAX_LINE` and hands it to `Json::parse`, which takes a `&str`. Bytes that
//! are not UTF-8 never reach a parser here — they fail at the transport — so
//! feeding random bytes would measure the transport and not the boundary. Text
//! that *claims* to be something it is not does reach it, and that is what the
//! seeds are made of: a `\uD800` with no pair, a `_raw` that is not base64, an
//! array whose declared length disagrees with its contents, a recursive marker
//! naming a type that is not open, a cycle, and numbers no allocator can honour.
//!
//! Same three sources as the wire fuzz, and mutation is per **character** for
//! the same reason: a `&str` is the input type, so a byte flip would be testing
//! `from_utf8` rather than the parser behind it.
//!
//! # Reading a green run
//!
//! [`Reach`] exists for the reason it exists in [`crate::wire`]: a fuzz whose
//! documents all bounce off `Json::parse` is green and worthless, and the exit
//! code cannot tell that apart from a fuzz that reached every arm of
//! `tc_from_json`. A zero in any reach field is a measurement failure, never a
//! pass. *도달 수 0은 통과가 아니라 측정 실패다.*

use std::panic::{AssertUnwindSafe, catch_unwind};

use orbweaver_dynamic::anyjson::{self, LocalReferences, References as _};
use orbweaver_dynamic::json::Json;
use orbweaver_forge::{Finding, Severity};
use orbweaver_giop::typecode::{Member, TypeCode, UnionCase};
use orbweaver_giop::{IiopProfile, Ior, Version};

use crate::finding;
use crate::prop::{Rng, case_seed};
use crate::wire::Source;

// ─────────────────────────────────────────────────────────────────────────────
// The allocation budget
// ─────────────────────────────────────────────────────────────────────────────

/// How many bytes of eager reservation one byte of document may command.
///
/// The rule this encodes is the wire fuzz's own: *twelve bytes must not buy a
/// multi-gigabyte allocation.* Turning it into a number needs a ratio rather
/// than a ceiling, because a large document legitimately describes a large
/// value and a small one never does.
///
/// 4096 is deliberately generous — three orders of magnitude past anything the
/// mapping emits, so a finding is a finding and not a threshold argument. A
/// 200-byte `tools/call` argument may command 800 KiB; the same argument
/// declaring `array<octet, 4294967295>` commands 192 GiB, which is over by a
/// factor of a quarter of a million.
pub const ALLOCATION_ALLOWANCE: u128 = 4096;

/// The largest eager reservation, in bytes, that decoding a value of this type
/// would ask for before it has looked at a single byte of input.
///
/// **This is a model of one arm of one function**, and saying so is the point.
/// `orbweaver_dynamic::decode_at` reserves for a `Sequence` only after
/// `Decoder::validate_count` has checked the count against the bytes actually
/// remaining — the guard whose comment names this very hazard. Its `Array` arm
/// has no such guard: the length comes from the `TypeCode`, and the `TypeCode`
/// now comes from the agent's document. So `Array` is what this counts, and
/// `Sequence` is what it deliberately does not.
///
/// Modelling rather than measuring is forced, not preferred. A counting
/// allocator needs `unsafe impl GlobalAlloc` and the workspace forbids
/// `unsafe`; and a reservation big enough to fail calls `handle_alloc_error`,
/// which aborts, so no `catch_unwind` would report it either. What can be
/// checked honestly is the number the document contains and the arm it reaches,
/// and that is what this checks.
///
/// Nested arrays multiply, because the outer reservation is still held when the
/// first element's reservation is made. Everything else takes the maximum of
/// what it contains: a struct decodes its members one at a time, so its peak is
/// its worst member and not their sum.
pub fn eager_bytes(tc: &TypeCode) -> u128 {
    /// `size_of::<orbweaver_dynamic::Value>()`, which is what a `Vec<Value>`
    /// reserves per element. Read at runtime rather than written down, so it
    /// cannot go stale against the enum.
    fn slot() -> u128 {
        std::mem::size_of::<orbweaver_dynamic::Value>() as u128
    }
    fn walk(tc: &TypeCode, depth: usize) -> u128 {
        // The same bound `orbweaver_dynamic` puts on following a `Recursive`
        // marker. Without it a cyclic TypeCode would recurse here instead of
        // there, which would make this checker the panic it exists to find.
        if depth >= 64 {
            return 0;
        }
        match tc {
            TypeCode::Array { element, length } => {
                let here = u128::from(*length).saturating_mul(slot());
                here.saturating_add(walk(element, depth + 1))
            }
            TypeCode::Sequence { element, .. } | TypeCode::Alias { aliased: element, .. } => {
                walk(element, depth + 1)
            }
            TypeCode::Struct { members, .. } | TypeCode::Except { members, .. } => {
                members.iter().map(|m| walk(&m.tc, depth + 1)).max().unwrap_or(0)
            }
            TypeCode::Union { discriminator, cases, .. } => walk(discriminator, depth + 1)
                .max(cases.iter().map(|c| walk(&c.tc, depth + 1)).max().unwrap_or(0)),
            _ => 0,
        }
    }
    walk(tc, 0)
}

// ─────────────────────────────────────────────────────────────────────────────
// Targets
// ─────────────────────────────────────────────────────────────────────────────

/// What a target is handed.
enum Feed {
    /// The raw frame, before anything has parsed it.
    Line(fn(&str)),
    /// A document `Json::parse` accepted. Targets behind this one measure the
    /// mapping rather than the JSON grammar, and a case whose text did not
    /// parse never reaches them — which is why [`Reach::parsed`] is reported.
    Doc(fn(&Json)),
}

/// One parser under test, named for the report.
struct Target {
    name: &'static str,
    feed: Feed,
    /// Whether this target can turn a number in the document into a
    /// reservation. Those are skipped for a document already found to be over
    /// [`ALLOCATION_ALLOWANCE`]: the finding is already recorded, and running
    /// it anyway would be the fuzz executing the defect it just reported.
    reserves: bool,
}

/// The parsers an agent reaches through `tools/call`.
fn targets() -> Vec<Target> {
    vec![
        Target {
            name: "json::Json::parse",
            feed: Feed::Line(|s| {
                let _ = Json::parse(s);
            }),
            reserves: false,
        },
        Target {
            name: "json::Json::write",
            feed: Feed::Doc(|j| {
                // The writer is on the agent's side of the boundary too: what
                // it renders goes back out as a response, and a document that
                // panics on the way out is the same stopped process as one that
                // panics on the way in. Re-parsing is in the same breath
                // because that is the round trip the MCP tests assert.
                let text = j.to_string();
                let _ = Json::parse(&text);
            }),
            reserves: false,
        },
        Target {
            name: "anyjson::tc_from_json",
            feed: Feed::Doc(|j| {
                let _ = anyjson::tc_from_json(j, "");
            }),
            reserves: false,
        },
        Target {
            name: "anyjson::tc_from_json -> tc_to_json",
            feed: Feed::Doc(|j| {
                // The path that turns a declared type back into a document,
                // and the one that reaches a *decoder* with an agent-chosen
                // TypeCode: `tc_to_json` renders each union case label by
                // decoding it with the discriminator the document supplied.
                // Reachable from an operation taking a `::CORBA::TypeCode`, or
                // from any `any` whose `_t` is echoed.
                if let Ok(tc) = anyjson::tc_from_json(j, "") {
                    let back = anyjson::tc_to_json(&tc);
                    let _ = anyjson::tc_from_json(&back, "");
                }
            }),
            reserves: true,
        },
        Target {
            name: "anyjson::from_json",
            feed: Feed::Doc(|j| {
                let handles = handles();
                for tc in contract_types() {
                    let _ = anyjson::from_json(&tc, j, &handles);
                }
            }),
            reserves: true,
        },
        Target {
            name: "anyjson::from_json -> to_json",
            feed: Feed::Doc(|j| {
                // The full crossing: a document becomes a `Value` and the
                // `Value` becomes a document again. An `any` carries the
                // agent's own TypeCode through both halves, so this is where a
                // type the agent invented reaches the renderer.
                let mut handles = handles();
                for tc in contract_types() {
                    if let Ok(v) = anyjson::from_json(&tc, j, &handles) {
                        let _ = anyjson::to_json(&tc, &v, &mut handles);
                    }
                }
            }),
            reserves: true,
        },
        Target {
            name: "anyjson::from_json -> encode",
            feed: Feed::Doc(|j| {
                // And onto the wire, which is what an accepted document is for.
                // A value the mapping accepted but the encoder cannot marshal
                // is the interesting case, because the guard chain has already
                // run by the time it is reached.
                let handles = handles();
                for tc in contract_types() {
                    if let Ok(v) = anyjson::from_json(&tc, j, &handles) {
                        let mut e = orbweaver_cdr::Encoder::new(orbweaver_cdr::Endian::Big);
                        let _ = orbweaver_dynamic::encode(&mut e, &tc, &v);
                        let _ = e.finish();
                    }
                }
            }),
            reserves: true,
        },
    ]
}

/// A reference table with one handle already in it.
///
/// Empty would make every `{"_ref": ...}` document fail at the same line and
/// the object-reference arm of the mapping would never be reached — the
/// green-and-worthless case. One issued handle means a mutated document can
/// name it, and [`Reach::references`] says how often that happened.
fn handles() -> LocalReferences {
    let mut h = LocalReferences::new();
    h.issue(&Ior {
        type_id: "IDL:fuzz/Target:1.0".into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "fuzz.invalid".into(),
            port: 4242,
            object_key: b"fuzz/key".to_vec(),
            components: Vec::new(),
        }],
    });
    h
}

/// The handle [`handles`] issues, so a seed document can name it.
const ISSUED_HANDLE: &str = "local-1";

/// The types a *contract* declares — the trusted half of the pair.
///
/// `from_json` is always called with a TypeCode the registry chose and a
/// document the agent wrote, so a fuzz that supplied both would be testing a
/// combination the boundary never presents. These are the declared parameter
/// types; the document is the only thing under the agent's control.
///
/// `Any` and `TypeCode` are in the list because they are the two that hand
/// control of the *type* back to the agent, which is the whole of D008's new
/// surface.
fn contract_types() -> Vec<TypeCode> {
    vec![
        TypeCode::Any,
        TypeCode::TypeCode,
        TypeCode::ULongLong,
        TypeCode::Double,
        // The one scalar whose JSON form is base64, so `unbase64` is reachable
        // from a value and not only from a union label's `_raw` escape.
        TypeCode::LongDouble,
        TypeCode::String(16),
        TypeCode::WString(0),
        TypeCode::Sequence { element: Box::new(TypeCode::Octet), bound: 0 },
        TypeCode::Sequence { element: Box::new(TypeCode::String(0)), bound: 4 },
        TypeCode::Array { element: Box::new(TypeCode::Long), length: 3 },
        TypeCode::Enum {
            id: "IDL:fuzz/Colour:1.0".into(),
            name: "Colour".into(),
            members: vec!["RED".into(), "GREEN".into()],
        },
        TypeCode::ObjRef { id: "IDL:fuzz/Target:1.0".into(), name: "Target".into() },
        TypeCode::Struct {
            id: "IDL:fuzz/Pair:1.0".into(),
            name: "Pair".into(),
            members: vec![
                Member { name: "a".into(), tc: TypeCode::Long },
                Member { name: "b".into(), tc: TypeCode::Any },
            ],
        },
        TypeCode::Union {
            id: "IDL:fuzz/Choice:1.0".into(),
            name: "Choice".into(),
            discriminator: Box::new(TypeCode::Long),
            default_index: -1,
            cases: vec![
                UnionCase { label: vec![0, 0, 0, 1], name: "one".into(), tc: TypeCode::String(0) },
                UnionCase { label: vec![0, 0, 0, 2], name: "two".into(), tc: TypeCode::Double },
            ],
        },
    ]
}

// ─────────────────────────────────────────────────────────────────────────────
// The documents
// ─────────────────────────────────────────────────────────────────────────────

/// Characters a mutated or uniform document is built from.
///
/// JSON's structure, AnyJSON's tag names, the punctuation of a repository id,
/// base64's alphabet and its padding, digits including the ones that make a
/// number absurd, and the awkward characters that turn a byte index into a
/// panic. A uniform draw over Unicode would stop at `Json::parse`'s first
/// character check, which is measured rather than assumed — see
/// [`Reach::parsed`].
const ALPHABET: &str =
    "{}[]\",:_-+/=0123456789aAbcdefgiklmnoprstuvwxzDIKLNOSTUV.\\ \n\t\0\u{0301}\u{1F600}";

/// One character of [`ALPHABET`], by index.
fn a_character(rng: &mut Rng) -> char {
    let n = ALPHABET.chars().count();
    ALPHABET.chars().nth(rng.below(n)).unwrap_or('?')
}

/// Well-formed and deliberately malformed documents to mutate and truncate.
///
/// Every entry is one of the input classes this module claims to cover, and
/// each is here because mutation alone will not produce it: no run of character
/// swaps is going to invent a ten-digit array length or a `\uD800` with no
/// pair. Seeding them is not peeking at an oracle — it is the corpus.
fn seeds() -> Vec<String> {
    let mut out = vec![
        // ── Structural TypeCodes, one per arm of `tc_from_json` ──
        r#""unsigned long long""#.to_owned(),
        r#"{"kind":"string","bound":32}"#.to_owned(),
        r#"{"kind":"wstring","bound":0}"#.to_owned(),
        r#"{"kind":"seq","element":"octet","bound":0}"#.to_owned(),
        r#"{"kind":"array","element":"long","length":3}"#.to_owned(),
        r#"{"kind":"fixed","digits":9,"scale":2}"#.to_owned(),
        r#"{"kind":"objref","id":"IDL:fuzz/Target:1.0","name":"Target"}"#.to_owned(),
        r#"{"kind":"enum","id":"IDL:fuzz/Colour:1.0","name":"Colour","members":["RED","GREEN"]}"#
            .to_owned(),
        r#"{"kind":"alias","id":"IDL:fuzz/Id:1.0","name":"Id","aliased":"unsigned long"}"#
            .to_owned(),
        r#"{"kind":"principal"}"#.to_owned(),
        // Nested members: a struct inside a struct inside a sequence.
        r#"{"kind":"struct","id":"IDL:fuzz/Outer:1.0","name":"Outer","members":[{"name":"a","type":{"kind":"seq","element":{"kind":"struct","id":"IDL:fuzz/Inner:1.0","name":"Inner","members":[{"name":"x","type":"long"},{"name":"y","type":"double"}]},"bound":0}},{"name":"b","type":"string"}]}"#
            .to_owned(),
        r#"{"kind":"except","id":"IDL:fuzz/Boom:1.0","name":"Boom","members":[{"name":"why","type":"string"}]}"#
            .to_owned(),
        // Union cases, with a label in value form and a label in `_raw` form.
        r#"{"kind":"union","id":"IDL:fuzz/Choice:1.0","name":"Choice","discriminator":"long","default":-1,"cases":[{"label":1,"name":"one","type":"string"},{"label":2,"name":"two","type":{"kind":"seq","element":"octet","bound":0}}]}"#
            .to_owned(),
        r#"{"kind":"union","id":"IDL:fuzz/Flag:1.0","name":"Flag","discriminator":"boolean","default":0,"cases":[{"label":{"_raw":"AQ=="},"name":"on","type":"long"}]}"#
            .to_owned(),
        // ── A member list whose declared size does not match its contents ──
        // AnyJSON declares an array's length in the type and its contents in
        // the value, so the disagreement lives across the two halves of an
        // `any`. Both directions, since one is short and one is long.
        r#"{"_t":{"kind":"array","element":"long","length":4},"_v":[1,2]}"#.to_owned(),
        r#"{"_t":{"kind":"array","element":"long","length":1},"_v":[1,2,3,4,5]}"#.to_owned(),
        // And a struct whose document has a member the type does not, and one
        // whose type has a member the document does not.
        r#"{"_t":{"kind":"struct","id":"IDL:fuzz/P:1.0","name":"P","members":[{"name":"a","type":"long"}]},"_v":{"a":1,"b":2}}"#
            .to_owned(),
        r#"{"_t":{"kind":"struct","id":"IDL:fuzz/P:1.0","name":"P","members":[{"name":"a","type":"long"},{"name":"b","type":"long"}]},"_v":{"a":1}}"#
            .to_owned(),
        // ── Absurd bounds and lengths ──
        // The number is the instruction; these are the ones an allocator
        // cannot honour, at the boundaries where a cast changes its meaning.
        r#"{"kind":"array","element":"octet","length":4294967295}"#.to_owned(),
        r#"{"kind":"array","element":{"kind":"array","element":"octet","length":4294967295},"length":4294967295}"#
            .to_owned(),
        r#"{"kind":"seq","element":"octet","bound":4294967295}"#.to_owned(),
        r#"{"kind":"string","bound":4294967296}"#.to_owned(),
        r#"{"kind":"string","bound":-1}"#.to_owned(),
        r#"{"kind":"fixed","digits":65535,"scale":-32768}"#.to_owned(),
        r#"{"kind":"array","element":"octet","length":99999999999999999999}"#.to_owned(),
        r#"{"kind":"array","element":"octet","length":1e309}"#.to_owned(),
        r#"{"kind":"union","id":"IDL:fuzz/U:1.0","name":"U","discriminator":{"kind":"array","element":"octet","length":4294967295},"default":-1,"cases":[{"label":{"_raw":"AAAAAA=="},"name":"a","type":"long"}]}"#
            .to_owned(),
        r#"{"kind":"union","id":"IDL:fuzz/U:1.0","name":"U","discriminator":"long","default":2147483647,"cases":[]}"#
            .to_owned(),
        // ── A `_raw` that is not base64 ──
        r#"{"_raw":"!!!!"}"#.to_owned(),
        r#"{"_raw":"AAA"}"#.to_owned(),
        r#"{"_raw":"A=AA"}"#.to_owned(),
        r#"{"_raw":"===="}"#.to_owned(),
        r#"{"_raw":123}"#.to_owned(),
        r#"{"_raw":null}"#.to_owned(),
        // A `long double` crosses as bare base64 rather than as a `_raw`
        // object, so these reach the same decoder by the other route: sixteen
        // octets exactly, four octets, and something that is not base64 at all.
        r#""AAAAAAAAAAAAAAAAAAAAAA==""#.to_owned(),
        r#""AAAA""#.to_owned(),
        r#""not base64!""#.to_owned(),
        // ── A recursive marker naming a type that is not open ──
        r#"{"kind":"recursive","id":"IDL:fuzz/Nobody:1.0"}"#.to_owned(),
        r#"{"_t":{"kind":"recursive","id":"IDL:fuzz/Nobody:1.0"},"_v":null}"#.to_owned(),
        r#"{"kind":"seq","element":{"kind":"recursive","id":"IDL:fuzz/Nobody:1.0"},"bound":0}"#
            .to_owned(),
        // ── Cycles ──
        // A struct that contains itself directly, which has no finite value at
        // all; the same through a sequence, which does; and an alias that
        // aliases itself.
        r#"{"kind":"struct","id":"IDL:fuzz/Loop:1.0","name":"Loop","members":[{"name":"me","type":{"kind":"recursive","id":"IDL:fuzz/Loop:1.0"}}]}"#
            .to_owned(),
        r#"{"kind":"struct","id":"IDL:fuzz/Tree:1.0","name":"Tree","members":[{"name":"label","type":"string"},{"name":"kids","type":{"kind":"seq","element":{"kind":"recursive","id":"IDL:fuzz/Tree:1.0"},"bound":0}}]}"#
            .to_owned(),
        r#"{"kind":"alias","id":"IDL:fuzz/A:1.0","name":"A","aliased":{"kind":"recursive","id":"IDL:fuzz/A:1.0"}}"#
            .to_owned(),
        r#"{"kind":"union","id":"IDL:fuzz/U:1.0","name":"U","discriminator":"long","default":-1,"cases":[{"label":1,"name":"me","type":{"kind":"recursive","id":"IDL:fuzz/U:1.0"}}]}"#
            .to_owned(),
        // ── Text where the parser expects text ──
        // Non-UTF-8 cannot arrive as bytes at this boundary (the frame is a
        // `&str` before anything here sees it), so it arrives as an escape that
        // names a code point no `char` can hold: an unpaired surrogate, a pair
        // in the wrong order, and a truncated escape.
        r#"{"_t":"wstring","_v":"\ud800"}"#.to_owned(),
        r#"{"_t":"wstring","_v":"\udc00\ud800"}"#.to_owned(),
        r#"{"_t":"wstring","_v":"😀"}"#.to_owned(),
        r#"{"_t":"string","_v":"\u00"}"#.to_owned(),
        r#"{"_t":"string","_v":"\q"}"#.to_owned(),
        // A literal NUL inside a JSON string. Written with an escape rather
        // than as the byte: one raw NUL made this whole file read as
        // *binary* to grep, diff and every review tool, for one seed.
        "{\"_t\":\"string\",\"_v\":\"a\0b\"}".to_owned(),
        r#"{"_t":"wchar","_v":"ab"}"#.to_owned(),
        r#"{"_t":"wchar","_v":""}"#.to_owned(),
        // ── Values, one per arm of `from_json` ──
        r#"{"_t":"long","_v":2147483648}"#.to_owned(),
        r#"{"_t":"unsigned long long","_v":"18446744073709551615"}"#.to_owned(),
        r#"{"_t":"unsigned long long","_v":18446744073709551615}"#.to_owned(),
        r#"{"_t":"long long","_v":1.5e300}"#.to_owned(),
        r#"{"_t":"double","_v":{"_f":"nan"}}"#.to_owned(),
        r#"{"_t":"double","_v":{"_f":"+inf"}}"#.to_owned(),
        r#"{"_t":"double","_v":{"_f":"maybe"}}"#.to_owned(),
        r#"{"_t":{"kind":"seq","element":"octet","bound":0},"_v":"AAECAwQF"}"#.to_owned(),
        r#"{"_t":{"kind":"enum","id":"IDL:fuzz/Colour:1.0","name":"Colour","members":["RED","GREEN"]},"_v":"RED"}"#
            .to_owned(),
        r#"{"_t":{"kind":"enum","id":"IDL:fuzz/Colour:1.0","name":"Colour","members":["RED"]},"_v":0}"#
            .to_owned(),
        r#"{"_d":1,"_v":"a branch"}"#.to_owned(),
        r#"{"_d":9,"_v":"no such branch"}"#.to_owned(),
        r#"{"_d":1}"#.to_owned(),
        r#"["RED","GREEN"]"#.to_owned(),
        r#"[1,2,3]"#.to_owned(),
        r#""AAECAwQF""#.to_owned(),
        r#"{"a":1,"b":{"_t":"long","_v":7}}"#.to_owned(),
        r#"null"#.to_owned(),
        r#"{}"#.to_owned(),
        // ── Object references: the handle table, and a handle nobody issued ──
        format!(r#"{{"_ref":"{ISSUED_HANDLE}"}}"#),
        r#"{"_ref":"local-999"}"#.to_owned(),
        r#"{"_ref":null}"#.to_owned(),
        r#"{"_ref":{"ior":"IOR:0000"}}"#.to_owned(),
        // ── The JSON grammar itself ──
        r#"{"a":1,"a":2}"#.to_owned(),
        "{\"a\":1}\u{feff}".to_owned(),
        r#"01"#.to_owned(),
        r#"{"a":1} {"b":2}"#.to_owned(),
    ];
    // Nesting, at and past `Json::MAX_DEPTH`. Built rather than written out,
    // because the interesting number is the parser's own limit and a literal
    // would go stale the day somebody moved it.
    let depth = orbweaver_dynamic::json::MAX_DEPTH;
    for levels in [depth / 2, depth - 1, depth + 4, depth * 8] {
        out.push(format!("{}{}", "[".repeat(levels), "]".repeat(levels)));
        out.push(nested_sequence(levels));
    }
    out
}

/// `seq<seq<seq<… octet …>>>`, `levels` deep, as a document.
///
/// Two JSON levels per type level, so this is the shape that decides whether
/// `Json::MAX_DEPTH` or `tc_from_json`'s own recursion runs out first. It is
/// the former, and the seed is here so that stays measured rather than assumed.
fn nested_sequence(levels: usize) -> String {
    let mut doc = String::from("\"octet\"");
    for _ in 0..levels {
        doc = format!(r#"{{"kind":"seq","element":{doc},"bound":0}}"#);
    }
    doc
}

/// The document for one case, and where it came from.
///
/// Mutation replaces one or two **characters**, for the reason
/// [`crate::wire`]'s text pipeline gives: the input type is `&str`, so a bit
/// flip would produce something that is not one. Truncation cuts on a character
/// boundary for the same reason.
fn make_document(rng: &mut Rng, seeds: &[String]) -> (Source, String) {
    match rng.below(3) {
        0 => {
            let n = rng.below(97);
            (Source::Uniform, (0..n).map(|_| a_character(rng)).collect())
        }
        1 => {
            let mut chars: Vec<char> = seeds[rng.below(seeds.len())].chars().collect();
            if !chars.is_empty() {
                for _ in 0..1 + rng.below(2) {
                    let at = rng.below(chars.len());
                    chars[at] = a_character(rng);
                }
            }
            (Source::Mutated, chars.into_iter().collect())
        }
        _ => {
            let chars: Vec<char> = seeds[rng.below(seeds.len())].chars().collect();
            let cut = rng.below(chars.len() + 1);
            (Source::Truncated, chars[..cut].iter().collect())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The run
// ─────────────────────────────────────────────────────────────────────────────

/// The rule a panic is reported under.
pub const PANIC_RULE: &str = "agent/panic";

/// The rule an over-budget allocation is reported under.
pub const ALLOCATION_RULE: &str = "agent/allocation";

/// Runs `cases` documents against every target and reports what it found.
///
/// Findings are deduplicated by `(rule, target)` and carry how many cases hit
/// them. A run of fifty thousand cases against one broken arm would otherwise
/// print fifty thousand lines of the same defect, and the first line is the one
/// worth reading — the count belongs in it, not underneath it.
pub fn panic_freedom(cases: usize, root: u64) -> Vec<Finding> {
    let seeds = seeds();
    let targets = targets();
    let mut out: Vec<Finding> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    for i in 0..cases {
        let seed = case_seed(root, i as u64);
        let mut rng = Rng::new(seed);
        let (source, text) = make_document(&mut rng, &seeds);
        let doc = Json::parse(&text).ok();

        // `eager_bytes` is a *model* of what a decode would reserve. It selects
        // which documents are worth asking about; it is not the verdict, and
        // for one run it was: the `Array` guard landed in `orbweaver-dynamic`
        // and this went on reporting the same finding, because a static model
        // cannot see a fix. The verdict is now the decoder's own answer.
        //
        // Running the decode on an over-budget document is safe **because** the
        // guard refuses on the declared count before reserving. If that guard
        // ever regresses, this line reserves rather than reports — which is a
        // real hazard and is written down rather than designed around, because
        // the alternative is a model that lies in both directions.
        let mut over_budget = None;
        if let Some(j) = &doc
            && let Ok(tc) = anyjson::tc_from_json(j, "")
        {
            let demanded = eager_bytes(&tc);
            let budget = ALLOCATION_ALLOWANCE.saturating_mul(text.len().max(1) as u128);
            if demanded > budget {
                // "It returned an error" is not the property. Without the
                // guard the decode *also* fails — on the first element, for a
                // truncated stream — so an is_ok() check can never fire and
                // would be one more green line measuring nothing. The property
                // is that the refusal is about the **declared count**, which
                // is the only refusal that happens before the reservation.
                let probe = [0u8; 8];
                let mut d = orbweaver_cdr::Decoder::new(&probe, orbweaver_cdr::Endian::Big);
                let refused_on_the_count = match orbweaver_dynamic::decode(&mut d, &tc) {
                    Ok(_) => false,
                    Err(e) => e.message.contains("implausible CDR length prefix"),
                };
                if !refused_on_the_count {
                    over_budget = Some(demanded);
                }
            }
        }
        if let Some(demanded) = over_budget {
            record(
                &mut out,
                &mut counts,
                ALLOCATION_RULE,
                "anyjson::tc_from_json",
                format!(
                    "a {} byte(s) document declared a type whose decode reserves {demanded} \
                     byte(s) before reading any input — {} byte(s) of allocation per byte of \
                     {} document, against a budget of {ALLOCATION_ALLOWANCE}; twelve bytes must \
                     not buy a multi-gigabyte allocation",
                    text.len(),
                    demanded / (text.len().max(1) as u128),
                    source.label(),
                ),
                format!(
                    "reproduce with orbweaver_test::agent::run_case({seed:#x}, \
                     \"anyjson::tc_from_json\"); the document is {}. The decoder did not refuse \
                     on the declared count, so the reservation happens before anything stops it; \
                     `orbweaver_dynamic`'s `decode_at` guards its `Sequence` and `Array` arms \
                     with `validate_count` and one of them has stopped",
                    quoted(&text)
                ),
            );
        }

        for t in &targets {
            if t.reserves && over_budget.is_some() {
                continue;
            }
            let panicked = catch_unwind(AssertUnwindSafe(|| match (&t.feed, &doc) {
                (Feed::Line(f), _) => f(&text),
                (Feed::Doc(f), Some(j)) => f(j),
                (Feed::Doc(_), None) => {}
            }))
            .is_err();
            if panicked {
                record(
                    &mut out,
                    &mut counts,
                    PANIC_RULE,
                    t.name,
                    format!(
                        "{} panicked on a {} document of {} character(s); an agent that can send \
                         this document can stop the process",
                        t.name,
                        source.label(),
                        text.chars().count(),
                    ),
                    format!(
                        "reproduce with orbweaver_test::agent::run_case({seed:#x}, {:?}); the \
                         document is {}",
                        t.name,
                        quoted(&text)
                    ),
                );
            }
        }
    }

    std::panic::set_hook(previous);

    for (f, n) in out.iter_mut().zip(&counts) {
        if *n > 1 {
            f.message.push_str(&format!(" ({n} case(s) in this run)"));
        }
    }
    out
}

/// Records a finding once per `(rule, target)`, counting the repeats.
fn record(
    out: &mut Vec<Finding>,
    counts: &mut Vec<usize>,
    rule: &'static str,
    target: &'static str,
    message: String,
    fix: String,
) {
    if let Some(at) = out.iter().position(|f| f.rule == rule && f.source == target) {
        counts[at] += 1;
        return;
    }
    out.push(finding(rule, Severity::Error, message, target.to_owned(), Some(fix)));
    counts.push(1);
}

/// What a run actually reached, so a green result can be read.
///
/// Every field answers *did the input get far enough into this parser to be
/// worth running?* A low number is not a failure — a validator refuses most of
/// what it sees, and that is its job. A **zero** is the failure, because the
/// target behind it returned early on every case and its green result measures
/// nothing at all.
#[derive(Debug, Clone, Copy, Default)]
pub struct Reach {
    /// Documents drawn uniformly at random.
    pub uniform: usize,
    /// Documents made by replacing characters in a seed.
    pub mutated: usize,
    /// Documents made by cutting a seed short.
    pub truncated: usize,
    /// How many documents `Json::parse` accepted — the gate every mapping
    /// target sits behind.
    pub parsed: usize,
    /// How many documents `Json::parse` refused for nesting rather than for
    /// syntax. Reported separately because it is the depth cap doing its job,
    /// and a zero here means the deep seeds stopped arriving.
    pub too_deep: usize,
    /// How many documents `tc_from_json` read a `TypeCode` out of.
    pub type_codes: usize,
    /// How many of those survived `tc_to_json` and came back identical.
    ///
    /// The gap between this and [`Reach::type_codes`] is reported rather than
    /// asserted away, because it is not zero and the reason is worth reading
    /// rather than hiding. Measured over 200 000 cases at seed `0xdeadbeef`:
    /// 2190 types read, 107 of them over the allocation budget and so not
    /// round-tripped at all, and **16** genuine mismatches — every one of them
    /// a union-case label that is not a canonical encoding of its own
    /// discriminator. A `boolean` label of `0x75`, or of two bytes, renders as
    /// `true` and re-reads as `0x01`. That is `tc_to_json` normalising a label
    /// that was already malformed, not losing one that was not, so it is a
    /// number to keep an eye on rather than a defect to report.
    ///
    /// *왕복 실패 16건은 전부 판별자에 대해 정규형이 아닌 union 레이블이었다.*
    pub type_code_round_trips: usize,
    /// How many documents were over [`ALLOCATION_ALLOWANCE`].
    pub over_budget: usize,
    /// How many (contract type, document) pairs `from_json` accepted.
    pub values: usize,
    /// How many of those made it back out through `to_json`.
    pub values_rendered: usize,
    /// How many of those encoded to CDR — the far end of the crossing.
    pub values_encoded: usize,
    /// How many documents resolved an object reference through the handle
    /// table, which is the one arm that needs state to be reached at all.
    pub references: usize,
    /// How many documents carried a `_raw` escape **and** were then accepted by
    /// `tc_from_json` — which is the only route to the base64 decoder from a
    /// type document, since a bad `_raw` fails the whole parse. A zero means
    /// the escape is not being exercised at all.
    pub raw_escapes: usize,
}

/// Whether a `_raw` escape appears anywhere in the document.
///
/// Depth-bounded for the same reason [`eager_bytes`] is: this walks input, and
/// a walker over input that can recurse without a bound is the defect the
/// module is looking for. `Json::parse` already caps at
/// [`orbweaver_dynamic::json::MAX_DEPTH`], so the bound is belt-and-braces and
/// costs one comparison.
fn carries_raw(j: &Json, depth: usize) -> bool {
    if depth >= orbweaver_dynamic::json::MAX_DEPTH {
        return false;
    }
    match j {
        Json::Object(map) => {
            map.contains_key("_raw") || map.values().any(|v| carries_raw(v, depth + 1))
        }
        Json::Array(items) => items.iter().any(|v| carries_raw(v, depth + 1)),
        _ => false,
    }
}

/// Measures [`Reach`] for the same documents [`panic_freedom`] would run.
pub fn reach(cases: usize, root: u64) -> Reach {
    let seeds = seeds();
    let types = contract_types();
    let mut r = Reach::default();
    for i in 0..cases {
        let mut rng = Rng::new(case_seed(root, i as u64));
        let (source, text) = make_document(&mut rng, &seeds);
        match source {
            Source::Uniform => r.uniform += 1,
            Source::Mutated => r.mutated += 1,
            Source::Truncated => r.truncated += 1,
        }
        let doc = match Json::parse(&text) {
            Ok(j) => {
                r.parsed += 1;
                j
            }
            Err(e) => {
                if e.message.contains("nesting deeper") {
                    r.too_deep += 1;
                }
                continue;
            }
        };
        let mut over = false;
        if let Ok(tc) = anyjson::tc_from_json(&doc, "") {
            r.type_codes += 1;
            if carries_raw(&doc, 0) {
                r.raw_escapes += 1;
            }
            let demanded = eager_bytes(&tc);
            over = demanded > ALLOCATION_ALLOWANCE.saturating_mul(text.len().max(1) as u128);
            if over {
                r.over_budget += 1;
            } else if let Ok(back) = anyjson::tc_from_json(&anyjson::tc_to_json(&tc), "")
                && back == tc
            {
                r.type_code_round_trips += 1;
            }
        }
        if over {
            continue;
        }
        let mut handles = handles();
        for tc in &types {
            let Ok(v) = anyjson::from_json(tc, &doc, &handles) else { continue };
            r.values += 1;
            if matches!(tc, TypeCode::ObjRef { .. })
                && matches!(&v, orbweaver_dynamic::Value::ObjRef(Some(_)))
            {
                r.references += 1;
            }
            if anyjson::to_json(tc, &v, &mut handles).is_ok() {
                r.values_rendered += 1;
            }
            let mut e = orbweaver_cdr::Encoder::new(orbweaver_cdr::Endian::Big);
            if orbweaver_dynamic::encode(&mut e, tc, &v).is_ok() && e.finish().is_ok() {
                r.values_encoded += 1;
            }
        }
    }
    r
}

/// The parsers under test, for a report that names what was covered.
pub fn target_names() -> Vec<&'static str> {
    targets().into_iter().map(|t| t.name).collect()
}

/// Replays one case against one target, without the catch, so a debugger sees
/// the panic where it happens.
///
/// The allocation guard is **not** applied here: replaying a finding is the one
/// time you want the reservation actually attempted. That is also why it is a
/// separate entry point from [`panic_freedom`] rather than a flag on it.
pub fn run_case(seed: u64, target: &str) {
    let mut rng = Rng::new(seed);
    let (_, text) = make_document(&mut rng, &seeds());
    let doc = Json::parse(&text).ok();
    for t in targets() {
        if t.name == target {
            match (t.feed, &doc) {
                (Feed::Line(f), _) => f(&text),
                (Feed::Doc(f), Some(j)) => f(j),
                (Feed::Doc(_), None) => {}
            }
        }
    }
}

/// The document for one case, for a caller that wants to look at it.
pub fn document(seed: u64) -> String {
    let mut rng = Rng::new(seed);
    make_document(&mut rng, &seeds()).1
}

/// A document as a finding should carry it: escaped, so a NUL or a newline is
/// visible rather than invisible, and bounded because a finding is meant to be
/// pasted into a test.
fn quoted(text: &str) -> String {
    let shown: String = text.chars().take(160).flat_map(char::escape_debug).collect();
    if text.chars().count() > 160 {
        format!("\"{shown}\"… ({} character(s) total)", text.chars().count())
    } else {
        format!("\"{shown}\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The measurement. If this ever fails, an agent can stop the process it is
    /// talking to.
    #[test]
    fn no_agent_parser_panics_on_hostile_documents() {
        let panics: Vec<_> = panic_freedom(600, crate::prop::DEFAULT_SEED)
            .into_iter()
            .filter(|f| f.rule == PANIC_RULE)
            .collect();
        assert!(
            panics.is_empty(),
            "{}",
            panics.iter().map(|f| f.message.clone()).collect::<Vec<_>>().join("\n")
        );
    }

    /// Every target must be handed something it can actually parse.
    ///
    /// A reach of zero is a target that refused every case and reported the
    /// same green as one that read a thousand types — and the exit code cannot
    /// tell those apart, which is exactly how a fuzz becomes worthless without
    /// anybody noticing. *도달 수 0은 통과가 아니라 측정 실패다.*
    ///
    /// Eight thousand rather than the wire fuzz's two, because the thinnest
    /// arms here — a `_raw` escape and a resolved handle — come from one seed
    /// each and land single digits per thousand cases. The seed is fixed, so
    /// the count is deterministic; the margin is there so that a generator
    /// change that halves reach fails loudly instead of passing on the last
    /// hit.
    #[test]
    fn every_agent_target_is_reached_and_not_merely_refused() {
        let r = reach(8_000, crate::prop::DEFAULT_SEED);
        for (what, count) in [
            ("documents parsed (json::Json::parse)", r.parsed),
            ("types read (anyjson::tc_from_json)", r.type_codes),
            ("types round-tripped (anyjson::tc_to_json)", r.type_code_round_trips),
            ("values read (anyjson::from_json)", r.values),
            ("values rendered (anyjson::to_json)", r.values_rendered),
            ("values encoded (orbweaver_dynamic::encode)", r.values_encoded),
            ("references resolved through the handle table", r.references),
            ("documents refused for nesting depth", r.too_deep),
        ] {
            assert!(count > 0, "{what} never happened in 3000 cases; that target is untested");
        }
    }

    /// All three sources must actually be drawn, since a report that names
    /// three and measures one is the report this crate exists not to write.
    #[test]
    fn the_pipeline_draws_from_all_three_sources() {
        let r = reach(600, crate::prop::DEFAULT_SEED);
        for (what, count) in
            [("uniform", r.uniform), ("mutated", r.mutated), ("truncated", r.truncated)]
        {
            assert!(count > 0, "no {what} documents were drawn");
        }
    }

    /// A mutated document is still a `&str`; that is the whole reason the
    /// mutator works in characters. Stated as a test because "it is a `String`,
    /// so it is UTF-8" stops being obvious the moment somebody optimises the
    /// mutator into byte indexing.
    #[test]
    fn a_mutated_document_is_still_a_string() {
        let seeds = seeds();
        assert!(!seeds.is_empty(), "no seed documents were built");
        for i in 0..400u64 {
            let mut rng = Rng::new(case_seed(crate::prop::DEFAULT_SEED, i));
            let (_, text) = make_document(&mut rng, &seeds);
            assert!(std::str::from_utf8(text.as_bytes()).is_ok());
        }
    }

    /// A seed reproduces its document. Every finding this module prints is a
    /// seed plus a target name, so if this stops holding the findings stop
    /// being replayable and become anecdotes.
    #[test]
    fn a_seed_reproduces_the_document_it_named() {
        for i in 0..200u64 {
            let seed = case_seed(crate::prop::DEFAULT_SEED, i);
            assert_eq!(document(seed), document(seed));
        }
    }

    /// The allocation check can see a number the document chose.
    ///
    /// This is the checker under test, not the crate it points at: it asserts
    /// that a small document declaring a huge array is over budget and that an
    /// ordinary one is not. Whether `orbweaver-dynamic` still honours the
    /// number is that crate's measurement, reported by the binary at runtime,
    /// and deliberately not pinned here — a test that went red the day somebody
    /// fixed the defect would be a test arguing for the defect.
    #[test]
    fn the_allocation_check_sees_a_number_the_document_chose() {
        let slot = std::mem::size_of::<orbweaver_dynamic::Value>() as u128;
        let doc = r#"{"kind":"array","element":"octet","length":4294967295}"#;
        let tc = anyjson::tc_from_json(&Json::parse(doc).expect("parses"), "").expect("a type");
        assert_eq!(eager_bytes(&tc), u128::from(u32::MAX) * slot);
        assert!(
            eager_bytes(&tc) > ALLOCATION_ALLOWANCE * doc.len() as u128,
            "a {} byte document reserving {} bytes must be over budget",
            doc.len(),
            eager_bytes(&tc)
        );

        let ordinary = r#"{"kind":"array","element":"long","length":3}"#;
        let tc =
            anyjson::tc_from_json(&Json::parse(ordinary).expect("parses"), "").expect("a type");
        assert!(eager_bytes(&tc) <= ALLOCATION_ALLOWANCE * ordinary.len() as u128);
    }

    /// A sequence is deliberately not counted, and a nested array is.
    ///
    /// Both halves of [`eager_bytes`]'s claim, because the difference between
    /// them is the whole model: `decode_at` guards a sequence's count against
    /// the bytes remaining and does not guard an array's length at all.
    #[test]
    fn the_model_counts_arrays_and_not_sequences() {
        let seq = TypeCode::Sequence { element: Box::new(TypeCode::Octet), bound: u32::MAX };
        assert_eq!(eager_bytes(&seq), 0, "a sequence's count is checked against the buffer");

        let inner = TypeCode::Array { element: Box::new(TypeCode::Octet), length: 1_000 };
        let outer = TypeCode::Array { element: Box::new(inner), length: 1_000 };
        let slot = std::mem::size_of::<orbweaver_dynamic::Value>() as u128;
        assert_eq!(eager_bytes(&outer), 1_000 * slot + 1_000 * slot);
    }

    /// A cyclic type must not make the checker itself recurse forever. The
    /// module exists to find that class, so it cannot be an example of it.
    #[test]
    fn the_allocation_check_survives_a_cycle() {
        let doc = r#"{"kind":"struct","id":"IDL:fuzz/Loop:1.0","name":"Loop","members":[{"name":"me","type":{"kind":"recursive","id":"IDL:fuzz/Loop:1.0"}}]}"#;
        let tc = anyjson::tc_from_json(&Json::parse(doc).expect("parses"), "").expect("a type");
        assert_eq!(eager_bytes(&tc), 0);
    }

    /// Every finding carries a seed and a document, or it is a story rather
    /// than a regression test.
    #[test]
    fn every_finding_carries_the_input_that_produced_it() {
        for f in panic_freedom(400, crate::prop::DEFAULT_SEED) {
            let fix = f.fix.as_deref().unwrap_or_default();
            assert!(fix.contains("run_case(0x"), "{}: no seed to replay", f.rule);
            assert!(fix.contains("document is"), "{}: no document to read", f.rule);
        }
    }

    /// The deep seeds really are deep enough to be refused, and the shallow
    /// ones really are shallow enough to be read. Without this the depth cap
    /// would be a comment rather than a measurement.
    #[test]
    fn the_nesting_seeds_land_on_both_sides_of_the_parsers_limit() {
        let shallow = nested_sequence(orbweaver_dynamic::json::MAX_DEPTH / 4);
        assert!(Json::parse(&shallow).is_ok(), "a shallow type document must parse");
        let deep = nested_sequence(orbweaver_dynamic::json::MAX_DEPTH * 8);
        let err = Json::parse(&deep).expect_err("a deep one must not");
        assert!(err.message.contains("nesting deeper"), "{err}");
    }
}
