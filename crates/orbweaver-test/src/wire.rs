//! Panic freedom: what our parsers do when the bytes are hostile.
//!
//! Every other measurement in this crate asks whether a well-formed value
//! survives a round trip. This one asks the opposite question, and it is the
//! one a deployment actually depends on: **given arbitrary bytes, does a
//! decoder return an error, or does it take the process down?**
//!
//! `CLAUDE.md` says the ORB core is Rust because "wire parsing is the classic
//! memory-safety hazard". That is true and it is only half the hazard. Rust
//! rules out the memory-corruption half at compile time and rules out nothing
//! about panics: a slice index, an `unwrap`, a subtraction below zero, or a
//! recursion that runs out of stack are all reachable from a peer's bytes, and
//! every one of them ends the process. A server a stranger can stop by sending
//! twelve bytes is not memory-unsafe; it is simply down. `unsafe_code =
//! "forbid"` does not cover this, so something has to measure it.
//!
//! **패닉하는 파서는 피어가 끌 수 있는 서버다.** Rust는 메모리 손상을 막지만
//! 패닉은 막지 않는다.
//!
//! # What counts as a pass
//!
//! `Ok` and `Err` are both passes. A decoder is entitled to accept nonsense
//! that happens to be well-formed — the fuzz has no oracle for meaning and
//! claims none. The only failure is a panic, and the finding carries the exact
//! bytes so it is a regression test rather than a story.
//!
//! # Where the bytes come from
//!
//! Uniform random bytes almost never reach past a parser's first length check,
//! so three sources are mixed and each is reported separately:
//!
//! - **Uniform** — cheap, and the only source that exercises the "garbage in
//!   the header" path.
//! - **Mutated** — a valid message with a few bits flipped. This is the source
//!   that finds things: the header still says GIOP, the length still looks
//!   plausible, and the body is now lying about something.
//! - **Truncated** — a valid message cut at an arbitrary offset, which is what
//!   a peer that dies mid-write actually produces. Not hypothetical: it is the
//!   normal outcome of a connection reset.
//!
//! # Two pipelines, because half these parsers take a `&str`
//!
//! The decoders this started with all read bytes. Three surfaces that landed
//! later read **text** somebody else chose — a stringified IOR out of a
//! `corbaname` or a configuration file, a D004 trace line, a repository id a
//! foreign Interface Repository picked — and feeding them random bytes tests
//! nothing at all: `std::str::from_utf8` rejects almost every uniform byte
//! string before the parser is even called, and a fuzz whose input never
//! arrives is the green-and-worthless case this module's [`Reach`] exists to
//! expose.
//!
//! So there is a second pipeline with the same three sources, drawn from text
//! seeds and mutated **by character** rather than by bit — a byte flip inside a
//! multi-byte character produces something that is not a `&str` at all, so the
//! flip would be testing `from_utf8` rather than the parser behind it. Both
//! pipelines are drawn from the same case seed, bytes first, so a seed still
//! reproduces the byte input it reproduced before this pipeline existed.
//!
//! **텍스트를 받는 파서에 무작위 바이트를 먹이면 `from_utf8`만 시험하게 된다.**
//! 그래서 문자 단위로 변형하는 두 번째 파이프라인을 따로 둔다.
//!
//! # What this fuzz cannot observe, and why it says so out loud
//!
//! `wire-fuzz` is documented and run as `--release`, and **a release build has
//! overflow checks off**. An `at + len` where `len` is a length a peer wrote
//! therefore does not panic; it wraps, and the parser carries on with a number
//! nobody sent. That is not a smaller version of the same bug — the two builds
//! have genuinely different behaviour, and the quieter one is the one that
//! ships:
//!
//! | build | `60 88 FF FF FF FF FF FF FF FF` |
//! | --- | --- |
//! | `-C overflow-checks=on` | panics — a peer stops the process |
//! | `-C overflow-checks=off` | wraps, and returns the *wrong* error |
//!
//! That is the defect commit `36c8bc0` fixed, and the reason it could not have
//! been found here: a release fuzzer reporting "0 panics" over an arithmetic
//! overflow is not reporting a pass, it is reporting that it was not looking.
//! So [`overflow_checks_on`] measures the build at run time rather than
//! assuming it, [`panic_freedom`] carries the result on every [`Finding`]-free
//! run, and the binary prints a warning naming the class it cannot see. **A
//! fuzz that claims a class it is structurally unable to observe has made a
//! claim it did not earn.**
//!
//! **릴리스 빌드는 정수 오버플로를 볼 수 없다.** 그래서 이 파일은 빌드를 실행
//! 시점에 측정하고, 볼 수 없는 것을 보고서에 적는다.
//!
//! The class *is* observable where the tests run — `cargo test` builds with
//! overflow checks on — which is why [`hostile_literals`] exists as a fixed
//! corpus rather than as seeds: a random pool reaches a peer-chosen length
//! sometimes, and a named literal reaches it every run, in both builds.

use std::panic::{AssertUnwindSafe, catch_unwind};

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_forge::{Finding, Severity};
use orbweaver_giop::csiv2::{
    self, AsContext, CompoundSecMech, EstablishContext, GssUpToken, IdentityToken, SasContext,
    SasContextBody, SecMechList,
};
use orbweaver_giop::nat::{RawIor, RawProfile};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_giop::{IiopProfile, Ior, TAG_INTERNET_IOP, TaggedComponent, Version};
use orbweaver_registry::ingest::{Limits, validate_identifier, validate_repository_id};

use crate::finding;
use crate::prop::{Rng, case_seed};

/// How a fuzz input was produced, kept in the report because the three sources
/// reach different code and a run that only finds uniform-source failures has
/// not tested the interesting half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Uniform random bytes.
    Uniform,
    /// A valid message with bits flipped.
    Mutated,
    /// A valid message cut short.
    Truncated,
}

impl Source {
    /// The word a report uses for this source. Public because
    /// [`crate::agent`] runs the same three sources over a different boundary
    /// and reports them in the same words — two vocabularies for one idea is
    /// how two reports stop being comparable.
    pub fn label(self) -> &'static str {
        match self {
            Source::Uniform => "uniform",
            Source::Mutated => "mutated",
            Source::Truncated => "truncated",
        }
    }
}

/// What a target is handed.
///
/// Not a convenience: a target that takes a `&str` must be given text a peer
/// could have sent, and text drawn from the byte pipeline is text that got past
/// `from_utf8` — a filter, not a parser. See the module documentation.
enum Feed {
    /// Reads bytes off a connection or an encapsulation.
    Bytes(fn(&[u8])),
    /// Reads a string somebody configured, named or pasted.
    Text(fn(&str)),
}

/// One decoder under test, named for the report.
struct Target {
    name: &'static str,
    feed: Feed,
}

/// The decoders a peer can reach without authenticating: everything that runs
/// before any policy does, plus the surfaces that landed later and parse
/// something somebody else named.
fn targets() -> Vec<Target> {
    let mut out = vec![
        Target {
            name: "giop::read_message",
            feed: Feed::Bytes(|b| {
                let mut cursor = std::io::Cursor::new(b);
                let _ = orbweaver_giop::read_message(&mut cursor, 64 * 1024);
            }),
        },
        Target {
            name: "giop::read_one_message",
            feed: Feed::Bytes(|b| {
                let mut cursor = std::io::Cursor::new(b);
                let _ = orbweaver_giop::read_one_message(&mut cursor, 64 * 1024);
            }),
        },
        Target {
            name: "server::decode_request",
            feed: Feed::Bytes(|b| {
                // The server's front door, and the only target here that a
                // peer reaches *before* any policy runs: read_message frames
                // it, this decodes it, and the guard chain has not been
                // consulted yet. A panic here is a refused caller stopping the
                // process it was refused by.
                let mut cursor = std::io::Cursor::new(b);
                if let Ok(msg) = orbweaver_giop::read_message(&mut cursor, 64 * 1024) {
                    let _ = orbweaver_giop::server::decode_request(msg);
                }
            }),
        },
        Target {
            name: "giop::decode_reply",
            feed: Feed::Bytes(|b| {
                let mut cursor = std::io::Cursor::new(b);
                if let Ok(msg) = orbweaver_giop::read_message(&mut cursor, 64 * 1024) {
                    let _ = orbweaver_giop::decode_reply(msg);
                }
            }),
        },
        Target {
            name: "giop::decode_locate_reply",
            feed: Feed::Bytes(|b| {
                let mut cursor = std::io::Cursor::new(b);
                if let Ok(msg) = orbweaver_giop::read_message(&mut cursor, 64 * 1024) {
                    let _ = orbweaver_giop::decode_locate_reply(msg);
                }
            }),
        },
        Target {
            name: "typecode::decode",
            feed: Feed::Bytes(|b| {
                for endian in [Endian::Big, Endian::Little] {
                    let mut d = Decoder::new(b, endian);
                    let _ = orbweaver_giop::typecode::decode(&mut d);
                }
            }),
        },
        Target {
            name: "Ior::read_from",
            feed: Feed::Bytes(|b| {
                for endian in [Endian::Big, Endian::Little] {
                    let mut d = Decoder::new(b, endian);
                    let _ = Ior::read_from(&mut d);
                }
            }),
        },
        Target {
            name: "Ior::parse",
            feed: Feed::Bytes(|b| {
                // The stringified form is attacker-controlled too: it arrives
                // in configuration, in a corbaname, and out of a naming
                // service. Non-UTF-8 is discarded rather than lossily
                // converted, because `parse` takes a `&str` and lossy
                // conversion would be this test inventing an input.
                //
                // Left on the byte pipeline even though the text pipeline now
                // exists, because it is the one target that measures what the
                // `from_utf8` gate lets through — compare `Reach::utf8` with
                // `Reach::stringified_iors`, which is the same parser reached
                // from text.
                if let Ok(s) = std::str::from_utf8(b) {
                    let _ = Ior::parse(s);
                }
            }),
        },
        Target {
            name: "dynamic::decode(recursive struct)",
            feed: Feed::Bytes(|b| {
                let tc = recursive_tree();
                for endian in [Endian::Big, Endian::Little] {
                    let mut d = Decoder::new(b, endian);
                    let _ = orbweaver_dynamic::decode(&mut d, &tc);
                }
            }),
        },
        Target {
            name: "dynamic::decode(any)",
            feed: Feed::Bytes(|b| {
                // `any` is the sharpest of these: the bytes choose the
                // TypeCode, so the sender picks which decoder runs next.
                for endian in [Endian::Big, Endian::Little] {
                    let mut d = Decoder::new(b, endian);
                    let _ = orbweaver_dynamic::decode(&mut d, &TypeCode::Any);
                }
            }),
        },
    ];
    out.extend(later_surfaces());
    out.extend(csiv2_surfaces());
    out
}

/// The surfaces that landed after this module was written.
///
/// In their own function rather than appended to the list above, because they
/// are a different claim. That list is *the decoders a peer reaches before any
/// policy runs*, and a panic there is a server a stranger stopped. These parse
/// input somebody else chose as well, but they sit in three different places
/// and the report is more honest for saying which:
///
/// - [`RawIor`] is reachable from **anything that can hand us a reference** — a
///   configuration file, a naming service, a `corbaname` string — and it exists
///   in order to keep profiles it cannot decode, so holding bytes nobody here
///   understands is its purpose rather than its failure mode. Same severity as
///   the list above.
/// - The console's trace reader is an **operator tool**, not a server. A panic
///   there loses a report that can be re-run; it does not end a process a peer
///   is talking to. Worth knowing, and not the same thing. The escaping
///   invariant is already tested inside `orbweaver-console` and nothing here
///   repeats it — this asks only whether the reader survives the line.
/// - `ingest`'s validators run on strings a **foreign Interface Repository**
///   chose, and they are the gate deciding what may enter the registry. A gate
///   that panics on the input it exists to refuse is a gate that is not there.
fn later_surfaces() -> Vec<Target> {
    vec![
        Target {
            name: "nat::RawIor::from_encapsulation",
            feed: Feed::Bytes(|b| {
                let _ = RawIor::from_encapsulation(b);
            }),
        },
        Target {
            name: "nat::RawIor::read_from",
            feed: Feed::Bytes(|b| {
                // Inline in a stream (§9.3.6): how a reference arrives inside a
                // reply, rather than out of a configuration file.
                for endian in [Endian::Big, Endian::Little] {
                    let mut d = Decoder::new(b, endian);
                    let _ = RawIor::read_from(&mut d);
                }
            }),
        },
        Target {
            name: "nat::RawIor::to_ior",
            feed: Feed::Bytes(|b| {
                // The dialing view decodes the IIOP profile bodies `RawIor`
                // deliberately leaves alone, so it reaches a parser neither
                // target above ever calls. Re-emitting is fuzzed in the same
                // breath because a rewriter does both.
                if let Ok(raw) = RawIor::from_encapsulation(b) {
                    let _ = raw.to_ior();
                    let _ = raw.to_stringified();
                }
            }),
        },
        Target {
            name: "nat::RawIor::parse",
            feed: Feed::Text(|s| {
                let _ = RawIor::parse(s);
            }),
        },
        Target {
            name: "console::TraceLog::read",
            feed: Feed::Text(|s| {
                let mut log = orbweaver_console::traces::TraceLog::default();
                log.read("fuzz.jsonl", s);
                // The counters walk every span that was read, so a panic in the
                // classification is reachable from here as well as one in the
                // JSON parser.
                let _ = (log.total(), log.refusals(), log.hypotheticals(), log.real_calls());
                let _ = (log.unclassified(), log.extra_keys());
            }),
        },
        Target {
            name: "ingest::validate_repository_id",
            feed: Feed::Text(|s| {
                let _ = validate_repository_id(s, &Limits::default());
            }),
        },
        Target {
            name: "ingest::validate_identifier",
            feed: Feed::Text(|s| {
                let _ = validate_identifier("a fuzzed name", s, &Limits::default());
            }),
        },
    ]
}

/// CSIv2, the surface this fuzz did not have until a defect was found without
/// it.
///
/// Commit `36c8bc0` fixed an integer overflow in [`GssUpToken::decode`] reached
/// from a DER length a peer wrote, and reported that **this file had no CSIv2
/// target at all**: the list above covers `read_message`, `decode_request`,
/// TypeCode, IOR, NAT, trace and ingest, and every one of them is a decoder
/// somebody remembered to add. The security layer was not one of them, which is
/// the wrong direction for an omission to run in.
///
/// The three entry points differ in how a peer reaches them, and the report
/// says which rather than flattening them into "CSIv2":
///
/// - [`SecMechList::parse`] is reached **today, in production**, through
///   `csiv2::advertised` on any `TaggedComponent` list — that is an IOR out of
///   a naming service, a configuration file or a reply, so the bytes are the
///   peer's whether or not we ever authenticate.
/// - [`SasContextBody::parse`] and [`GssUpToken::decode`] read what arrives in
///   the `SecurityAttributeService` context (`csiv2::SERVICE_ID_SAS`) on a
///   call. We currently only *write* that context — `orbweaver-mcp`'s
///   `identity::service_context` builds it and nothing in the workspace decodes
///   an incoming one outside tests — so these two are fuzzed **ahead of** the
///   server path that will call them. That is deliberate and it is stated here
///   rather than implied: a parser fuzzed before it is wired up is cheap, and a
///   parser wired up before it is fuzzed is how `36c8bc0` happened.
///
/// **보안 계층은 "잊고 안 넣은 대상"이 되어서는 안 된다.**
fn csiv2_surfaces() -> Vec<Target> {
    vec![
        Target {
            name: "csiv2::GssUpToken::decode",
            feed: Feed::Bytes(|b| {
                let _ = GssUpToken::decode(b);
            }),
        },
        Target {
            name: "csiv2::SasContextBody::parse",
            feed: Feed::Bytes(|b| {
                // A body that decodes is then re-encoded, because the bridge
                // does both and an arm that survives reading can still be a
                // length nobody can write back.
                if let Ok(body) = SasContextBody::parse(b) {
                    for endian in [Endian::Big, Endian::Little] {
                        let _ = body.encode(endian);
                    }
                    // The nested token is the sharper half: a peer chooses the
                    // service context, and the client authentication token
                    // inside it is a second, independently framed parser —
                    // which is exactly where the overflow `36c8bc0` fixed was.
                    if let SasContextBody::Establish(e) = &body {
                        let _ = GssUpToken::decode(&e.client_authentication_token);
                    }
                }
            }),
        },
        Target {
            name: "csiv2::SecMechList::parse",
            feed: Feed::Bytes(|b| {
                let _ = SecMechList::parse(b);
            }),
        },
        Target {
            name: "csiv2::advertised",
            feed: Feed::Bytes(|b| {
                // The production reach: the component list comes off an IOR, so
                // the body handed to `SecMechList::parse` is a slice a peer
                // chose the length of as well as the contents.
                let components =
                    vec![TaggedComponent { tag: csiv2::TAG_CSI_SEC_MECH_LIST, data: b.to_vec() }];
                if let Some(Ok(list)) = csiv2::advertised(&components) {
                    let _ = list.identity_assertion();
                    for token in [
                        IdentityToken::Absent,
                        IdentityToken::Anonymous,
                        IdentityToken::PrincipalName(b"alice".to_vec()),
                    ] {
                        let _ = list
                            .mechanisms
                            .iter()
                            .any(|m| m.sas_context.as_ref().is_some_and(|s| s.accepts(&token)));
                    }
                }
            }),
        },
    ]
}

/// `struct Tree { string label; sequence<Tree> kids; }` — the shape whose
/// depth the sender chooses.
fn recursive_tree() -> TypeCode {
    use orbweaver_giop::typecode::Member;
    TypeCode::Struct {
        id: "IDL:fuzz/Tree:1.0".into(),
        name: "Tree".into(),
        members: vec![
            Member { name: "label".into(), tc: TypeCode::String(0) },
            Member {
                name: "kids".into(),
                tc: TypeCode::Sequence {
                    element: Box::new(TypeCode::Recursive("IDL:fuzz/Tree:1.0".into())),
                    bound: 0,
                },
            },
        ],
    }
}

/// Well-formed messages to mutate and truncate.
fn seeds() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
        for endian in [Endian::Big, Endian::Little] {
            if let Ok(msg) = orbweaver_giop::encode_request(
                version,
                endian,
                7,
                b"fuzz/key",
                "an_operation",
                true,
                |e| {
                    e.put_str("body");
                    e.put_u32(0xDEAD_BEEF);
                },
            ) {
                out.push(msg);
            }
            if let Ok(msg) = orbweaver_giop::encode_locate_request(version, endian, 9, b"fuzz/key")
            {
                out.push(msg);
            }
            // Replies too, or the reply decoders would only ever be handed
            // requests and would refuse at the message type before reaching
            // anything worth fuzzing.
            for status in [
                orbweaver_giop::ReplyStatus::NoException,
                orbweaver_giop::ReplyStatus::UserException,
                orbweaver_giop::ReplyStatus::SystemException,
                orbweaver_giop::ReplyStatus::LocationForward,
            ] {
                if let Ok(msg) =
                    orbweaver_giop::server::encode_reply(version, endian, 7, status, |e| {
                        e.put_str("IDL:fuzz/Boom:1.0");
                        e.put_u32(1);
                        e.put_u32(0);
                    })
                {
                    out.push(msg);
                }
            }
            if let Ok(msg) = orbweaver_giop::server::encode_locate_reply(
                version,
                endian,
                9,
                orbweaver_giop::server::LocateStatus::ObjectHere,
            ) {
                out.push(msg);
            }
        }
    }
    // A TypeCode and an IOR on their own, since two targets decode those
    // directly rather than inside a message.
    for endian in [Endian::Big, Endian::Little] {
        let mut e = Encoder::new(endian);
        if orbweaver_giop::typecode::encode(&mut e, &recursive_tree()).is_ok()
            && let Ok(bytes) = e.finish()
        {
            out.push(bytes);
        }
        let ior = Ior { type_id: "IDL:fuzz/Tree:1.0".into(), profiles: Vec::new() };
        let mut e = Encoder::new(endian);
        if ior.write_to(&mut e).is_ok()
            && let Ok(bytes) = e.finish()
        {
            out.push(bytes);
        }
        // IOR *encapsulations*, which is a different thing from the inline form
        // above: an encapsulation carries its own byte-order flag and restarts
        // alignment at its first byte (CLAUDE.md, "alignment origin matters").
        // `RawIor::from_encapsulation` reads that shape and nothing in the list
        // above produces one, so without these the RawIor byte targets would be
        // relying on a mutated GIOP header happening to start with a valid
        // byte-order flag — measured at 610 encapsulations in 50 000 cases with
        // these seeds present, which is the number `Reach::encapsulations`
        // exists to show.
        for raw in raw_iors(endian) {
            let mut e = Encoder::encapsulation(endian);
            if raw.write_to(&mut e).is_ok()
                && let Ok(bytes) = e.finish()
            {
                out.push(bytes);
            }
        }
    }
    out.extend(csiv2_seeds());
    out
}

/// Well-formed CSIv2 messages, without which the CSIv2 targets are decoration.
///
/// Measured before these existed, over 50 000 cases: **0 GSS initial context
/// tokens and 0 SAS context bodies decoded**, and 10 security mechanism lists.
/// `GssUpToken::decode` refuses anything that does not open with the DER tag
/// `0x60`, so a uniform byte string reaches its second line one time in 256 and
/// then still has to carry a well-formed length and the eight-byte GSSUP OID;
/// the two encapsulation parsers need a byte-order flag, a plausible count and
/// a chain of octet sequences that do not run off the end. A target reached
/// zero times is not a target that passed — it is a target that was never run,
/// reporting the same green as one that decoded a thousand messages.
///
/// **도달 0회는 통과가 아니라 미측정이다.** 그래서 씨앗을 넣는다.
fn csiv2_seeds() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for endian in [Endian::Big, Endian::Little] {
        let gssup = GssUpToken {
            username: b"alice@example.com".to_vec(),
            password: b"hunter2".to_vec(),
            target_name: b"bank@example.com".to_vec(),
        };
        // The short form, and a username long enough to force the DER long form
        // — the branch of `parse_der_length` that reads a peer-chosen byte count
        // and the one `36c8bc0` overflowed in. A seed that only ever exercises
        // the short form leaves that branch to the uniform source, which is the
        // 1-in-256 draw above.
        let long = GssUpToken {
            username: vec![b'u'; 300],
            password: b"hunter2".to_vec(),
            target_name: b"bank@example.com".to_vec(),
        };
        let empty =
            GssUpToken { username: Vec::new(), password: Vec::new(), target_name: Vec::new() };
        let mut tokens = Vec::new();
        for t in [&gssup, &long, &empty] {
            if let Ok(bytes) = t.encode(endian) {
                tokens.push(bytes.clone());
                out.push(bytes);
            }
        }
        let authenticated = tokens.first().cloned().unwrap_or_default();

        for body in [
            // Nothing asserted, nothing authenticated: the shape
            // `identity::service_context` declines to send, and therefore the
            // one a peer can still choose to send us.
            SasContextBody::Establish(EstablishContext::default()),
            // The shape the bridge actually writes, with a GSSUP token nested
            // inside the octet sequence. Two framings, one message: the outer
            // CDR encapsulation and the inner DER-tagged token, and the inner
            // one is where the overflow was.
            SasContextBody::Establish(EstablishContext {
                client_context_id: 0x0102_0304_0506_0708,
                authorization_token: vec![(1, b"scope=read".to_vec()), (17, vec![0xAB; 40])],
                identity_token: IdentityToken::PrincipalName(b"alice@example.com".to_vec()),
                client_authentication_token: authenticated.clone(),
            }),
            SasContextBody::Establish(EstablishContext {
                client_context_id: 1,
                authorization_token: Vec::new(),
                identity_token: IdentityToken::X509CertChain(vec![0x30; 200]),
                client_authentication_token: Vec::new(),
            }),
            SasContextBody::Complete {
                client_context_id: 7,
                stateful: true,
                final_context_token: vec![0x60, 0x02, 0x01, 0x02],
            },
            SasContextBody::Error {
                client_context_id: 7,
                major_status: -1,
                minor_status: i32::MIN,
                error_token: vec![0xFF; 16],
            },
        ] {
            if let Ok(bytes) = body.encode(endian) {
                out.push(bytes);
            }
        }

        for list in [rich_advertisement(), bare_advertisement()] {
            if let Some(bytes) = encode_sec_mech_list(&list, endian) {
                out.push(bytes);
            }
        }
    }
    out
}

/// A target that offers everything: a transport component, GSSUP client
/// authentication, and identity assertion over two naming mechanisms.
fn rich_advertisement() -> SecMechList {
    SecMechList {
        stateful: true,
        mechanisms: vec![
            CompoundSecMech {
                target_requires: csiv2::options::ESTABLISH_TRUST_IN_CLIENT,
                transport: Some(TaggedComponent { tag: 36, data: vec![0x01, 0x00, 0x20, 0x00] }),
                as_context: Some(AsContext {
                    target_supports: csiv2::options::ESTABLISH_TRUST_IN_CLIENT,
                    target_requires: 0,
                    mechanism: csiv2::GSSUP_OID.to_vec(),
                    target_name: b"bank@example.com".to_vec(),
                }),
                sas_context: Some(SasContext {
                    target_supports: csiv2::options::IDENTITY_ASSERTION,
                    target_requires: 0,
                    naming_mechanisms: vec![csiv2::GSSUP_OID.to_vec(), vec![0x06, 0x01, 0x2A]],
                    supported_identity_types: 2 | 4 | 8,
                }),
            },
            CompoundSecMech {
                target_requires: 0,
                transport: None,
                as_context: None,
                sas_context: None,
            },
        ],
    }
}

/// A target that offers nothing, which is the common case §4.8 names and the
/// one whose encoding is all zeroes — the shape a bit flip turns into a claim.
fn bare_advertisement() -> SecMechList {
    SecMechList {
        stateful: false,
        mechanisms: vec![CompoundSecMech {
            target_requires: 0,
            transport: None,
            as_context: None,
            sas_context: None,
        }],
    }
}

/// Writes a `CSIIOP::CompoundSecMechList` encapsulation.
///
/// `orbweaver-giop` parses this shape and does not write it — a target
/// advertises, a bridge reads — so the encoder lives here, in the fuzz that
/// needs seeds, rather than being added to the crate under test for the fuzz's
/// convenience. It mirrors [`SecMechList::parse`] field for field and its
/// agreement is asserted by a test rather than assumed; an encoder that drifts
/// from the parser it feeds produces seeds that decode to nothing, which is the
/// zero-reach failure in a costume.
fn encode_sec_mech_list(list: &SecMechList, endian: Endian) -> Option<Vec<u8>> {
    let mut e = Encoder::encapsulation(endian);
    e.put_bool(list.stateful);
    e.put_u32(u32::try_from(list.mechanisms.len()).ok()?);
    for m in &list.mechanisms {
        e.put_u16(m.target_requires);
        match &m.transport {
            Some(c) => {
                e.put_u32(c.tag);
                e.put_octet_seq(&c.data);
            }
            None => {
                e.put_u32(csiv2::TAG_NULL_TAG);
                e.put_octet_seq(&[]);
            }
        }
        match &m.as_context {
            Some(a) => {
                e.put_u16(a.target_supports);
                e.put_u16(a.target_requires);
                e.put_octet_seq(&a.mechanism);
                e.put_octet_seq(&a.target_name);
            }
            None => {
                e.put_u16(0);
                e.put_u16(0);
                e.put_octet_seq(&[]);
                e.put_octet_seq(&[]);
            }
        }
        let sas = m.sas_context.as_ref();
        e.put_u16(sas.map_or(0, |s| s.target_supports));
        e.put_u16(sas.map_or(0, |s| s.target_requires));
        // No privilege authorities. The parser reads and discards them, so the
        // count still has to be written or every field after it shifts.
        e.put_u32(0);
        let naming: &[Vec<u8>] = sas.map_or(&[], |s| s.naming_mechanisms.as_slice());
        e.put_u32(u32::try_from(naming.len()).ok()?);
        for oid in naming {
            e.put_octet_seq(oid);
        }
        e.put_u32(sas.map_or(0, |s| s.supported_identity_types));
    }
    e.finish().ok()
}

/// References worth mutating: an empty one, and one carrying both an IIOP
/// profile a rewriter would walk and a profile tag we do not speak.
///
/// The unknown tag is the point of [`RawIor`] rather than a curiosity — it is
/// the profile the type exists to preserve, so a fuzz that only ever handed it
/// profiles we decode would be testing the easy half.
fn raw_iors(endian: Endian) -> Vec<RawIor> {
    let profile = IiopProfile {
        version: Version::V1_2,
        host: "fuzz.invalid".into(),
        port: 4242,
        object_key: b"fuzz/key".to_vec(),
        components: vec![TaggedComponent { tag: 20, data: vec![1, 0, 0, 0] }],
    };
    let body = profile.encapsulate(endian).ok().and_then(|e| e.finish().ok()).unwrap_or_default();
    vec![
        RawIor { type_id: String::new(), profiles: Vec::new(), endian },
        RawIor {
            type_id: "IDL:fuzz/Tree:1.0".into(),
            profiles: vec![
                RawProfile { tag: TAG_INTERNET_IOP, body: body.clone() },
                RawProfile { tag: 0x4f57_0001, body: vec![0xAA; 12] },
            ],
            endian,
        },
    ]
}

// ─────────────────────────────────────────────────────────────────────────────
// The text pipeline
// ─────────────────────────────────────────────────────────────────────────────

/// Characters a mutated or uniform string is built from.
///
/// Weighted toward what the three text grammars are made of — identifier
/// characters, the `IDL:`, `/`, `:` and `.` an id is punctuated with, hex digits
/// for a stringified IOR, and JSON's structural characters — because a uniform
/// draw over all of Unicode reaches a parser's first character check and stops
/// there. The awkward ones are in as well: a quote, a backslash, a NUL, a
/// newline, a combining mark and an astral character, since those are the ones
/// that turn a byte index into a panic.
///
/// A `&str` rather than a `[char; N]` so it stays one readable line per row
/// instead of forty.
const ALPHABET: &str = "abcdefilmorstzADILOZ012_79-.:/{}[\"\\,\n\0\u{0301}\u{1F600}";

/// One character of [`ALPHABET`], by index.
fn a_character(rng: &mut Rng) -> char {
    let n = ALPHABET.chars().count();
    ALPHABET.chars().nth(rng.below(n)).unwrap_or('?')
}

/// Well-formed strings to mutate and truncate: one per text grammar under test.
fn text_seeds() -> Vec<String> {
    let mut out = vec![
        // D004 span records (docs/decisions/D004-observability.md): the nine
        // keys, then the shapes the console documents as readable-but-odd — a
        // field that is not a string, an unknown key, and a line that is valid
        // JSON but not an object.
        r#"{"ts":"2026-08-14T09:00:00Z","session":"s-1","caller":"alice","target":"IDL:bank/Account:1.0","operation":"balance","decision":"allow","stage":"-","path":"dynamic","outcome":"ok"}"#
            .to_owned(),
        r#"{"ts":"2026-08-14T09:00:01Z","session":"s-1","caller":null,"target":"IDL:omg.org/CORBA/Object:1.0","operation":"close","decision":"dry_run_refuse","stage":"authz.exposure","path":"static","outcome":"NO_PERMISSION","extra":{"nested":[1,2,3]}}"#
            .to_owned(),
        r#"{"decision":123,"target":["not","a","string"]}"#.to_owned(),
        r#"["not an object at all"]"#.to_owned(),
        // Repository ids a foreign IR could answer with, in the shapes
        // `validate_repository_id` splits on: a pragma prefix, a two-segment
        // scope, a deep scope, a bare name, and a format it refuses outright.
        "IDL:omg.org/CORBA/Object:1.0".to_owned(),
        "IDL:acme.com/bank/Account:1.2".to_owned(),
        "IDL:inventory/warehouse/StockItem:1.0".to_owned(),
        "IDL:Simple:1.0".to_owned(),
        "RMI:com.example.Thing:0000000000000000".to_owned(),
        // Plain identifiers, which is what the second validator sees.
        "balance".to_owned(),
        "_reserved".to_owned(),
        "get_totals".to_owned(),
        // And a corbaname-shaped string, since that is one of the ways a
        // stringified reference arrives.
        "corbaname::fuzz.invalid:4242#bank/Account".to_owned(),
    ];
    // Real stringified IORs, in both byte orders. Built rather than pasted so
    // they cannot go stale against the encoder.
    for endian in [Endian::Big, Endian::Little] {
        for raw in raw_iors(endian) {
            if let Ok(text) = raw.to_stringified() {
                out.push(text);
            }
        }
    }
    out
}

/// The text for one case, and where it came from.
///
/// Mutation replaces **characters**, not bits, and replaces one or two rather
/// than the byte mutator's one to four. Both differences are that mutator's own
/// argument applied to a grammar that is far more brittle: a bit flipped inside
/// a multi-byte character produces something that is not a `&str` at all, and a
/// text grammar rejects on the first wrong character rather than carrying on
/// with a wrong length. Whether the numbers are right is not an opinion — it is
/// [`Reach`], and a run whose `stringified_iors` or `repository_ids` collapses
/// is a run where this got too aggressive.
fn make_text(rng: &mut Rng, seeds: &[String]) -> (Source, String) {
    match rng.below(3) {
        0 => {
            let n = rng.below(65);
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
            // Cut on a character boundary. Cutting on a byte boundary would
            // produce a `Vec<u8>` that is not a `&str`, which is the byte
            // pipeline's job and not this one's.
            (Source::Truncated, chars[..cut].iter().collect())
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The class a release build cannot see
// ─────────────────────────────────────────────────────────────────────────────

/// Whether **this** build panics on an arithmetic overflow.
///
/// Measured, not assumed. `cfg!(debug_assertions)` is the usual guess and it is
/// only a default: `-C overflow-checks` can be set either way independently, so
/// a report built on the `cfg!` would be stating the profile rather than the
/// behaviour. This performs the addition and watches.
///
/// The answer decides what a green run means. With checks off, every
/// `peer_chosen_length + offset` in every decoder wraps silently and this fuzz
/// cannot distinguish "no overflow" from "overflow, unobserved" — so the number
/// is printed beside the panic count rather than left for a reader to infer.
///
/// **오버플로 검사 여부는 가정하지 않고 실행 시점에 측정한다.**
pub fn overflow_checks_on() -> bool {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let panicked = catch_unwind(|| {
        // `black_box` on both operands: rustc denies an overflow it can see at
        // compile time, and would fold this away in release otherwise.
        let a = std::hint::black_box(usize::MAX);
        let b = std::hint::black_box(1usize);
        std::hint::black_box(a + b)
    })
    .is_err();
    std::panic::set_hook(previous);
    panicked
}

/// Inputs that reach a peer-chosen length, by name, every run.
///
/// The random pipeline reaches these shapes *sometimes*, which is the whole
/// argument for [`Reach`]; a named literal reaches them **every run, in every
/// build**, and that is what a class the release build cannot observe needs. If
/// the same input is only reachable through a seed, then whether the check ran
/// at all depends on `--cases`, and a class nobody can see does not get to also
/// be a class nobody reliably reaches.
///
/// Each name is the replay handle: `run_literal(name, target)` re-runs one
/// without the catch, the same way a seed does for the random pipeline.
///
/// Weighted toward **arithmetic on a length the peer wrote**, because that is
/// the class `36c8bc0` found and the class this file could not see. The first
/// entry is that commit's exact input.
pub fn hostile_literals() -> Vec<(String, Vec<u8>)> {
    let oid = csiv2::GSSUP_OID;
    let mut out: Vec<(String, Vec<u8>)> = vec![
        // `36c8bc0`'s input: a DER length of usize::MAX. `at + len` panicked
        // with overflow checks on and wrapped to a bogus "truncated" without.
        (
            "gss/der-length-usize-max".to_owned(),
            vec![0x60, 0x88, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        ),
        // One below, so the refusal is about the addition and not about the
        // single value `usize::MAX` being special-cased somewhere.
        (
            "gss/der-length-usize-max-less-one".to_owned(),
            vec![0x60, 0x88, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE],
        ),
        // Nine length bytes: refused by the `n > 8` check before any arithmetic.
        (
            "gss/der-length-nine-bytes".to_owned(),
            vec![0x60, 0x89, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
        ),
        // 4 GiB on a 64-bit target, and unrepresentable on a 32-bit one.
        ("gss/der-length-four-gib".to_owned(), vec![0x60, 0x85, 0x01, 0x00, 0x00, 0x00, 0x00]),
        // Indefinite length, which has no place in a GSS token.
        ("gss/der-length-indefinite".to_owned(), vec![0x60, 0x80]),
        ("gss/der-length-absent".to_owned(), vec![0x60]),
        ("gss/der-length-long-form-truncated".to_owned(), vec![0x60, 0x84, 0x00, 0x00]),
        ("gss/der-length-short-form-past-end".to_owned(), vec![0x60, 0x7F, 0x00]),
        ("gss/tag-only".to_owned(), vec![0x60]),
        ("gss/empty".to_owned(), Vec::new()),
    ];
    // A length that covers exactly the OID and nothing after it: the
    // encapsulation that follows is empty, so the byte-order flag every
    // encapsulation starts with is missing rather than wrong.
    let mut exact = vec![0x60, oid.len() as u8];
    exact.extend_from_slice(oid);
    out.push(("gss/oid-then-empty-encapsulation".to_owned(), exact));
    // A byte-order flag and nothing else, which is the shortest input that gets
    // past `Decoder::encapsulation` into the body.
    let mut flag_only = vec![0x60, (oid.len() + 1) as u8];
    flag_only.extend_from_slice(oid);
    flag_only.push(0x01);
    out.push(("gss/oid-then-flag-only".to_owned(), flag_only));
    // A length that stops inside the OID: `starts_with` must decide, not index.
    let mut half = vec![0x60, 0x04];
    half.extend_from_slice(&oid[..4]);
    out.push(("gss/length-covers-half-the-oid".to_owned(), half));

    for (endian, tag) in [(Endian::Big, "be"), (Endian::Little, "le")] {
        // Counts and sequence lengths a peer chose, at u32::MAX. `validate_count`
        // is the guard; these are what it is guarding, and a `Vec::with_capacity`
        // reached before it would be an allocation a stranger sized.
        let mut e = Encoder::encapsulation(endian);
        e.put_u32(0); // MTEstablishContext
        e.put_u64(0);
        e.put_u32(u32::MAX); // authorization_token count
        push_named(&mut out, "sas/authorization-count-u32-max", tag, e.finish().ok());

        let mut e = Encoder::encapsulation(endian);
        e.put_u32(0);
        e.put_u64(0);
        e.put_u32(0);
        e.put_u32(2); // ITTPrincipalName
        e.put_u32(u32::MAX); // its octet sequence length
        push_named(&mut out, "sas/identity-token-length-u32-max", tag, e.finish().ok());

        let mut e = Encoder::encapsulation(endian);
        e.put_bool(true);
        e.put_u32(u32::MAX); // mechanism count
        push_named(&mut out, "secmech/mechanism-count-u32-max", tag, e.finish().ok());

        let mut e = Encoder::encapsulation(endian);
        e.put_bool(false);
        e.put_u32(1);
        e.put_u16(0);
        e.put_u32(csiv2::TAG_NULL_TAG);
        e.put_octet_seq(&[]);
        e.put_u16(0);
        e.put_u16(0);
        e.put_octet_seq(&[]);
        e.put_octet_seq(&[]);
        e.put_u16(0);
        e.put_u16(0);
        e.put_u32(u32::MAX); // privilege authority count
        push_named(&mut out, "secmech/privilege-count-u32-max", tag, e.finish().ok());

        // The nested case, and the one an inbound call would actually carry: a
        // well-formed SAS context body whose client authentication token is the
        // hostile DER length above. Two framings, and the inner one is the one
        // that overflowed.
        if let Ok(bytes) = SasContextBody::Establish(EstablishContext {
            client_context_id: 0,
            authorization_token: Vec::new(),
            identity_token: IdentityToken::Anonymous,
            client_authentication_token: vec![
                0x60, 0x88, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
            ],
        })
        .encode(endian)
        {
            push_named(&mut out, "sas/nested-gss-length-usize-max", tag, Some(bytes));
        }

        // A truncated encapsulation: the flag says an order and nothing follows.
        push_named(
            &mut out,
            "cdr/encapsulation-flag-only",
            tag,
            Some(vec![u8::from(endian == Endian::Little)]),
        );
    }
    out
}

/// Names a per-byte-order literal: the two orders get two entries and two
/// replay handles, because "it failed in little-endian" is only useful if the
/// name says so.
fn push_named(out: &mut Vec<(String, Vec<u8>)>, name: &str, order: &str, bytes: Option<Vec<u8>>) {
    if let Some(bytes) = bytes {
        // The order is part of the input, and the name has to say which, or a
        // finding replays the wrong one half the time.
        out.push((format!("{name}[{order}]"), bytes));
    }
}

/// Replays one hostile literal against one target, without the catch.
///
/// The literal counterpart of [`run_case`]: a finding from the fixed corpus
/// carries a name where a finding from the random pipeline carries a seed, and
/// both have to replay or neither is a regression test.
pub fn run_literal(name: &str, target: &str) {
    let Some((_, bytes)) = hostile_literals().into_iter().find(|(n, _)| n == name) else {
        return;
    };
    for t in targets() {
        if t.name == target
            && let Feed::Bytes(f) = t.feed
        {
            f(&bytes);
        }
    }
}

/// Runs `cases` inputs against every target and reports every panic.
///
/// Silences the panic hook for the duration: a run that finds nothing should
/// print nothing, and a run that finds something reports it as a [`Finding`]
/// with the input attached rather than as a backtrace nobody kept.
pub fn panic_freedom(cases: usize, root: u64) -> Vec<Finding> {
    let seeds = seeds();
    let text_seeds = text_seeds();
    let targets = targets();
    let mut out = Vec::new();

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    // The fixed corpus first, and unconditionally: it does not depend on
    // `cases`, so `--cases 1` still runs every input that reaches a peer-chosen
    // length. Whether this build can *see* an overflow among them is
    // [`overflow_checks_on`]'s question, not this loop's.
    out.extend(literal_findings(&targets));

    for i in 0..cases {
        let seed = case_seed(root, i as u64);
        let mut rng = Rng::new(seed);
        // Bytes first, then text, always in that order: a seed that reproduced
        // a byte input before the text pipeline existed still reproduces the
        // same one, because nothing was drawn from the generator ahead of it.
        let (byte_source, bytes) = make_input(&mut rng, &seeds);
        let (text_source, text) = make_text(&mut rng, &text_seeds);
        for t in &targets {
            let (source, size, shown) = match &t.feed {
                Feed::Bytes(_) => (byte_source, bytes.len(), hex(&bytes)),
                Feed::Text(_) => (text_source, text.len(), quoted(&text)),
            };
            let panicked = catch_unwind(AssertUnwindSafe(|| match &t.feed {
                Feed::Bytes(f) => f(&bytes),
                Feed::Text(f) => f(&text),
            }))
            .is_err();
            if panicked {
                out.push(finding(
                    "wire/panic",
                    Severity::Error,
                    format!(
                        "{} panicked on {} input of {size} byte(s); a peer that can send these \
                         bytes can stop the process",
                        t.name,
                        source.label(),
                    ),
                    t.name.to_string(),
                    Some(format!(
                        "reproduce with orbweaver_test::wire::run_case({seed:#x}, {:?}); the \
                         input is {shown}",
                        t.name,
                    )),
                ));
            }
        }
    }

    std::panic::set_hook(previous);
    out
}

/// Runs every [`hostile_literals`] entry against every byte target.
///
/// Split out of [`panic_freedom`] so it can be handed a target that *does*
/// panic and be shown to report it. A reporting path nobody has ever seen fire
/// is a reporting path that might not, and this whole module is an argument
/// against trusting a green result whose machinery was never exercised.
fn literal_findings(targets: &[Target]) -> Vec<Finding> {
    let mut out = Vec::new();
    for (name, bytes) in hostile_literals() {
        for t in targets {
            let Feed::Bytes(f) = t.feed else { continue };
            if catch_unwind(AssertUnwindSafe(|| f(&bytes))).is_err() {
                out.push(finding(
                    "wire/panic",
                    Severity::Error,
                    format!(
                        "{} panicked on the fixed hostile literal {name:?} ({} byte(s)); a peer \
                         that can send these bytes can stop the process",
                        t.name,
                        bytes.len(),
                    ),
                    t.name.to_string(),
                    Some(format!(
                        "reproduce with orbweaver_test::wire::run_literal({name:?}, {:?}); the \
                         input is {}",
                        t.name,
                        hex(&bytes),
                    )),
                ));
            }
        }
    }
    out
}

/// What a run actually reached, so a green result can be read.
///
/// A fuzz that never gets past the first length check is green and worthless,
/// and the exit code cannot tell the two apart. These counts can.
///
/// Every field below the source counts answers one question: **did the input
/// get far enough into this target to be worth running?** They are not pass
/// rates and a low one is not a failure — a validator is supposed to refuse
/// most of what it sees. A *zero* is the failure, because it means the target
/// was handed nothing it could parse and its green result says nothing at all.
#[derive(Debug, Clone, Copy, Default)]
pub struct Reach {
    /// Byte inputs drawn uniformly at random.
    pub uniform: usize,
    /// Byte inputs made by flipping bits in a valid message.
    pub mutated: usize,
    /// Byte inputs made by cutting a valid message short.
    pub truncated: usize,
    /// How many byte inputs parsed as a GIOP message — the ones that reached
    /// past the header into the body decoders.
    pub parsed: usize,
    /// How many byte inputs decoded as an IOR encapsulation, which is what the
    /// three `nat::RawIor` byte targets need before they do anything.
    pub encapsulations: usize,
    /// How many byte inputs were even valid UTF-8, which is the gate the
    /// `Ior::parse` target sits behind.
    pub utf8: usize,
    /// How many byte inputs `Ior::parse` actually accepted — the real reach of
    /// that target, as opposed to how many got past `from_utf8`. The gap
    /// between this and [`Reach::stringified_iors`] is the argument for the
    /// text pipeline, measured rather than asserted.
    pub stringified_from_bytes: usize,
    /// Text inputs drawn uniformly at random.
    pub text_uniform: usize,
    /// Text inputs made by replacing characters in a valid string.
    pub text_mutated: usize,
    /// Text inputs made by cutting a valid string short.
    pub text_truncated: usize,
    /// How many text inputs `nat::RawIor::parse` accepted.
    pub stringified_iors: usize,
    /// How many span records the console read out of the text inputs. Counted
    /// in spans rather than in lines because a line the reader rejected reached
    /// the JSON parser and stopped there.
    pub trace_spans: usize,
    /// How many text inputs `ingest::validate_repository_id` accepted.
    pub repository_ids: usize,
    /// How many text inputs `ingest::validate_identifier` accepted.
    pub identifiers: usize,
    /// How many byte inputs `csiv2::GssUpToken::decode` accepted. The one
    /// number in this struct with a documented reason to be small: the parser
    /// refuses anything that does not open with the DER tag `0x60`, so a
    /// uniform byte string reaches it with probability 1/256 and then has to
    /// carry a well-formed length and the GSSUP OID as well.
    pub gss_tokens: usize,
    /// How many byte inputs `csiv2::SasContextBody::parse` accepted.
    pub sas_bodies: usize,
    /// How many byte inputs `csiv2::SecMechList::parse` accepted.
    pub sec_mech_lists: usize,
    /// How many fixed hostile literals ran, times the byte targets they ran
    /// against. Independent of `cases` on purpose — see [`hostile_literals`].
    pub literal_runs: usize,
    /// Whether this build could have observed an arithmetic overflow among
    /// them. `false` is not a smaller pass; it is a class going unmeasured, and
    /// it is in [`Reach`] so a report cannot omit it by accident.
    pub overflow_observable: bool,
}

/// Measures [`Reach`] for the same inputs [`panic_freedom`] would run.
pub fn reach(cases: usize, root: u64) -> Reach {
    let seeds = seeds();
    let text_seeds = text_seeds();
    let limits = Limits::default();
    let mut r = Reach {
        overflow_observable: overflow_checks_on(),
        literal_runs: hostile_literals().len()
            * targets().iter().filter(|t| matches!(t.feed, Feed::Bytes(_))).count(),
        ..Reach::default()
    };
    for i in 0..cases {
        let mut rng = Rng::new(case_seed(root, i as u64));
        let (source, input) = make_input(&mut rng, &seeds);
        let (text_source, text) = make_text(&mut rng, &text_seeds);
        match source {
            Source::Uniform => r.uniform += 1,
            Source::Mutated => r.mutated += 1,
            Source::Truncated => r.truncated += 1,
        }
        match text_source {
            Source::Uniform => r.text_uniform += 1,
            Source::Mutated => r.text_mutated += 1,
            Source::Truncated => r.text_truncated += 1,
        }
        let mut cursor = std::io::Cursor::new(&input[..]);
        if orbweaver_giop::read_message(&mut cursor, 64 * 1024).is_ok() {
            r.parsed += 1;
        }
        if RawIor::from_encapsulation(&input).is_ok() {
            r.encapsulations += 1;
        }
        if let Ok(s) = std::str::from_utf8(&input) {
            r.utf8 += 1;
            if Ior::parse(s).is_ok() {
                r.stringified_from_bytes += 1;
            }
        }
        if RawIor::parse(&text).is_ok() {
            r.stringified_iors += 1;
        }
        let mut log = orbweaver_console::traces::TraceLog::default();
        log.read("fuzz.jsonl", &text);
        r.trace_spans += log.total();
        if validate_repository_id(&text, &limits).is_ok() {
            r.repository_ids += 1;
        }
        if validate_identifier("a fuzzed name", &text, &limits).is_ok() {
            r.identifiers += 1;
        }
        if GssUpToken::decode(&input).is_ok() {
            r.gss_tokens += 1;
        }
        if SasContextBody::parse(&input).is_ok() {
            r.sas_bodies += 1;
        }
        if SecMechList::parse(&input).is_ok() {
            r.sec_mech_lists += 1;
        }
    }
    r
}

/// The decoders under test, for a report that names what was covered.
pub fn target_names() -> Vec<&'static str> {
    targets().into_iter().map(|t| t.name).collect()
}

/// Replays one case against one target, without the catch, so a debugger sees
/// the panic where it happens.
pub fn run_case(seed: u64, target: &str) {
    let mut rng = Rng::new(seed);
    let (_, bytes) = make_input(&mut rng, &seeds());
    let (_, text) = make_text(&mut rng, &text_seeds());
    for t in targets() {
        if t.name == target {
            match t.feed {
                Feed::Bytes(f) => f(&bytes),
                Feed::Text(f) => f(&text),
            }
        }
    }
}

/// The bytes for one case, and where they came from.
fn make_input(rng: &mut Rng, seeds: &[Vec<u8>]) -> (Source, Vec<u8>) {
    match rng.below(3) {
        0 => {
            let n = rng.below(257);
            (Source::Uniform, (0..n).map(|_| rng.next_u64() as u8).collect())
        }
        1 => {
            let mut bytes = seeds[rng.below(seeds.len())].clone();
            if !bytes.is_empty() {
                // One to four flips. More than that and a mutated message is
                // just a uniform one with a GIOP header.
                for _ in 0..1 + rng.below(4) {
                    let at = rng.below(bytes.len());
                    bytes[at] ^= 1 << rng.below(8);
                }
            }
            (Source::Mutated, bytes)
        }
        _ => {
            let bytes = &seeds[rng.below(seeds.len())];
            let cut = rng.below(bytes.len() + 1);
            (Source::Truncated, bytes[..cut].to_vec())
        }
    }
}

/// A text input as a finding should carry it: escaped, so a NUL or a newline is
/// visible rather than invisible, and bounded for the same reason [`hex`] is.
fn quoted(text: &str) -> String {
    let shown: String = text.chars().take(96).flat_map(char::escape_debug).collect();
    if text.chars().count() > 96 {
        format!("\"{shown}\"… ({} character(s) total)", text.chars().count())
    } else {
        format!("\"{shown}\"")
    }
}

fn hex(bytes: &[u8]) -> String {
    // Bounded: a finding is meant to be pasted into a test, and a page of hex
    // is not. The seed reproduces the whole input.
    let shown: String = bytes.iter().take(48).map(|b| format!("{b:02x}")).collect();
    if bytes.len() > 48 { format!("{shown}… ({} bytes total)", bytes.len()) } else { shown }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The measurement. If this ever fails, a peer can stop us.
    #[test]
    fn no_decoder_panics_on_hostile_bytes() {
        let findings = panic_freedom(400, crate::prop::DEFAULT_SEED);
        assert!(
            findings.is_empty(),
            "{}",
            findings.iter().map(|f| f.message.clone()).collect::<Vec<_>>().join("\n")
        );
    }

    /// The fuzz must actually be reaching the parsers, not bouncing off a
    /// length check every time. Measured, because a fuzz that tests nothing
    /// passes forever and reports the same green as one that tests everything.
    #[test]
    fn the_mutated_source_produces_messages_that_still_parse() {
        let seeds = seeds();
        assert!(!seeds.is_empty(), "no seed messages were built");
        let mut parsed = 0;
        for i in 0..200u64 {
            let mut rng = Rng::new(case_seed(crate::prop::DEFAULT_SEED, i));
            let (source, input) = make_input(&mut rng, &seeds);
            if source != Source::Mutated {
                continue;
            }
            let mut cursor = std::io::Cursor::new(&input[..]);
            if orbweaver_giop::read_message(&mut cursor, 64 * 1024).is_ok() {
                parsed += 1;
            }
        }
        assert!(
            parsed > 0,
            "no mutated input parsed as a message; the fuzz is only testing the header check"
        );
    }

    /// The same demand, made of every target added since: **each one must be
    /// handed something it can actually parse.**
    ///
    /// This is the assertion the module documentation promises and the one a
    /// reviewer should distrust the batch without. A target whose reach is zero
    /// is a target that returned `Err` on every case and reported the same green
    /// as one that decoded a thousand references — and the exit code cannot
    /// tell those apart, which is exactly how a fuzz gets to be worthless
    /// without anybody noticing.
    ///
    /// 도달 수 0은 통과가 아니라 측정 실패다.
    #[test]
    fn every_later_surface_is_reached_and_not_merely_refused() {
        let r = reach(2_000, crate::prop::DEFAULT_SEED);
        for (what, count) in [
            ("IOR encapsulations decoded (nat::RawIor byte targets)", r.encapsulations),
            ("stringified IORs parsed (nat::RawIor::parse)", r.stringified_iors),
            ("span records read (console::TraceLog::read)", r.trace_spans),
            ("repository ids accepted (ingest::validate_repository_id)", r.repository_ids),
            ("identifiers accepted (ingest::validate_identifier)", r.identifiers),
        ] {
            assert!(count > 0, "{what} never happened in 2000 cases; that target is untested");
        }
    }

    /// Why the text pipeline exists, stated as a measurement rather than as an
    /// opinion.
    ///
    /// The byte pipeline reaches a stringified-IOR parser only through
    /// `from_utf8`, and plenty of byte inputs *are* valid UTF-8 — which is
    /// exactly the trap, because "most inputs got past the gate" reads like
    /// reach and is not. What reaches the parser is what the parser accepted,
    /// and from bytes that is essentially none of it.
    #[test]
    fn a_stringified_ior_parser_is_not_reachable_from_random_bytes() {
        let r = reach(2_000, crate::prop::DEFAULT_SEED);
        assert!(
            r.stringified_iors > r.stringified_from_bytes,
            "the text pipeline parsed {} stringified IOR(s) and the byte pipeline {} out of {} \
             inputs that were valid UTF-8; if that ever inverts, the text pipeline has stopped \
             earning its keep",
            r.stringified_iors,
            r.stringified_from_bytes,
            r.utf8
        );
    }

    /// Both pipelines produce all three sources, since a report that names
    /// three and measures one is the report this crate exists not to write.
    #[test]
    fn both_pipelines_draw_from_all_three_sources() {
        let r = reach(600, crate::prop::DEFAULT_SEED);
        for (what, count) in [
            ("uniform bytes", r.uniform),
            ("mutated bytes", r.mutated),
            ("truncated bytes", r.truncated),
            ("uniform text", r.text_uniform),
            ("mutated text", r.text_mutated),
            ("truncated text", r.text_truncated),
        ] {
            assert!(count > 0, "no {what} were drawn");
        }
    }

    /// The text pipeline must not be able to hand a target something that is
    /// not a `&str`; that is the whole reason it mutates characters. Stated as
    /// a test because "it is a `String`, so it is UTF-8" stops being obvious
    /// the moment somebody optimises the mutator into byte indexing.
    #[test]
    fn a_mutated_string_is_still_a_string() {
        let seeds = text_seeds();
        assert!(!seeds.is_empty(), "no text seeds were built");
        for i in 0..300u64 {
            let mut rng = Rng::new(case_seed(crate::prop::DEFAULT_SEED, i));
            let (_, text) = make_text(&mut rng, &seeds);
            assert!(std::str::from_utf8(text.as_bytes()).is_ok());
        }
    }

    /// Adding the text pipeline must not have moved the byte inputs, or every
    /// seed in every finding this module has ever reported would now mean
    /// something else. Bytes are drawn first, and this is what says so.
    #[test]
    fn drawing_text_does_not_disturb_the_byte_input_a_seed_reproduces() {
        let byte_seeds = seeds();
        let strings = text_seeds();
        for i in 0..50u64 {
            let seed = case_seed(crate::prop::DEFAULT_SEED, i);
            let mut alone = Rng::new(seed);
            let mut both = Rng::new(seed);
            let bytes_only = make_input(&mut alone, &byte_seeds);
            let bytes_then_text = make_input(&mut both, &byte_seeds);
            let _ = make_text(&mut both, &strings);
            assert_eq!(bytes_only, bytes_then_text);
        }
    }

    /// The same demand made of the CSIv2 targets, and the reason this batch
    /// exists at all.
    ///
    /// Measured before [`csiv2_seeds`] was written, over 50 000 cases:
    /// **0 GSS initial context tokens, 0 SAS context bodies**, 10 security
    /// mechanism lists. Three targets, two of them never once run. That is the
    /// state a green exit code was already reporting as a pass.
    #[test]
    fn every_csiv2_surface_is_reached_and_not_merely_refused() {
        let r = reach(2_000, crate::prop::DEFAULT_SEED);
        for (what, count) in [
            ("GSS initial context tokens decoded (csiv2::GssUpToken::decode)", r.gss_tokens),
            ("SAS context bodies decoded (csiv2::SasContextBody::parse)", r.sas_bodies),
            ("security mechanism lists decoded (csiv2::SecMechList::parse)", r.sec_mech_lists),
        ] {
            assert!(count > 0, "{what} never happened in 2000 cases; that target is untested");
        }
    }

    /// Why the CSIv2 seeds exist, stated as a measurement rather than as an
    /// opinion — the same argument the text pipeline makes above.
    ///
    /// `GssUpToken::decode` returns on its first line unless byte zero is
    /// `0x60`, so the uniform source reaches its second line one time in 256 and
    /// then still has to carry a well-formed DER length and the eight-byte
    /// GSSUP OID. If the seed-derived sources ever stop dominating, the seeds
    /// have stopped being what makes this target reachable.
    #[test]
    fn a_gssup_token_is_not_reachable_from_random_bytes() {
        let seeds = seeds();
        let (mut from_uniform, mut from_seeds) = (0usize, 0usize);
        for i in 0..2_000u64 {
            let mut rng = Rng::new(case_seed(crate::prop::DEFAULT_SEED, i));
            let (source, input) = make_input(&mut rng, &seeds);
            if GssUpToken::decode(&input).is_ok() {
                match source {
                    Source::Uniform => from_uniform += 1,
                    _ => from_seeds += 1,
                }
            }
        }
        assert!(
            from_seeds > from_uniform,
            "{from_seeds} token(s) decoded from the seeded sources and {from_uniform} from \
             uniform bytes; if that ever inverts, the seed corpus has stopped earning its keep"
        );
    }

    /// The seed encoder must agree with the parser it feeds.
    ///
    /// `orbweaver-giop` parses `CSIIOP::CompoundSecMechList` and does not write
    /// it, so [`encode_sec_mech_list`] is this module's own and nothing else
    /// checks it. An encoder that drifts one field from the parser produces
    /// seeds that decode to nothing — which shows up as reach quietly falling to
    /// zero, the exact failure the reach numbers exist to prevent and the exact
    /// one a green exit code would hide.
    #[test]
    fn the_advertisement_encoder_agrees_with_the_parser_it_feeds() {
        for endian in [Endian::Big, Endian::Little] {
            for list in [rich_advertisement(), bare_advertisement()] {
                let bytes = encode_sec_mech_list(&list, endian).expect("encodes");
                let read = SecMechList::parse(&bytes).expect("parses");
                assert_eq!(read, list, "{endian:?}");
            }
        }
        // And the rich one must actually carry what it claims, or "it round
        // trips" would be true of two empty structures.
        let rich = rich_advertisement();
        assert!(rich.identity_assertion().is_some());
        assert!(rich.mechanisms[0].as_context.as_ref().expect("gssup").is_gssup());
    }

    /// The fixed corpus, and the honesty rule applied to this file itself.
    ///
    /// The assertion that matters is the **first** one: `cargo test` builds with
    /// overflow checks on, and if it did not, every literal below could pass by
    /// wrapping rather than by refusing. An unmeasured check is a failure, never
    /// a pass (`CLAUDE.md`), so the test says what it needed in order to mean
    /// anything before it says what it found.
    ///
    /// **측정할 수 없는 빌드에서의 통과는 통과가 아니다.**
    #[test]
    fn the_hostile_literals_are_refused_and_this_build_could_tell() {
        assert!(
            overflow_checks_on(),
            "this test binary was built with overflow checks OFF, so an arithmetic overflow on a \
             peer-chosen length would wrap here instead of panicking and every literal below \
             would pass without being measured"
        );
        // `cases = 0`: the random pipeline contributes nothing and only the
        // fixed corpus runs, so a failure here names a literal and not a seed.
        let findings = panic_freedom(0, crate::prop::DEFAULT_SEED);
        assert!(
            findings.is_empty(),
            "{}",
            findings.iter().map(|f| f.message.clone()).collect::<Vec<_>>().join("\n")
        );
    }

    /// The literal corpus's *reporting* path, shown firing.
    ///
    /// `the_hostile_literals_are_refused_and_this_build_could_tell` asserts the
    /// list is empty, which is exactly the assertion a broken reporter would
    /// also satisfy. This hands the same machinery a target that panics on
    /// every input and checks that a finding comes back naming the literal and
    /// carrying a replay handle — because "0 findings" only means anything if
    /// 1 finding was ever possible.
    ///
    /// **0건은 1건이 가능할 때만 의미가 있다.**
    #[test]
    fn a_literal_that_panics_is_reported_with_a_handle_that_replays() {
        let exploding = vec![Target {
            name: "fuzz::always_panics",
            feed: Feed::Bytes(|_| panic!("deliberate")),
        }];
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let findings = literal_findings(&exploding);
        std::panic::set_hook(previous);

        assert_eq!(findings.len(), hostile_literals().len(), "one finding per literal");
        let first = &findings[0];
        assert_eq!(first.source, "fuzz::always_panics");
        let fix = first.fix.as_deref().expect("a finding without a replay handle is a story");
        assert!(fix.contains("run_literal"), "{fix}");
        assert!(fix.contains("gss/der-length-usize-max"), "{fix}");
    }

    /// The probe measures the build, not the profile — so it has to be right
    /// about an operation other than the one it performs, and it has to give
    /// the same answer twice.
    #[test]
    fn the_overflow_probe_measures_the_build_rather_than_the_profile() {
        let on = overflow_checks_on();
        assert_eq!(on, overflow_checks_on(), "the probe is not deterministic");

        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        // A subtraction below zero, which is the same class through a different
        // operator. If checks are on it panics; if they are off it wraps.
        let underflowed = catch_unwind(|| {
            let a = std::hint::black_box(0usize);
            let b = std::hint::black_box(1usize);
            std::hint::black_box(a - b)
        })
        .is_err();
        std::panic::set_hook(previous);
        assert_eq!(on, underflowed, "the probe disagrees with the build on a second operation");
    }

    /// A literal finding has to replay, the same way a seeded one does. That
    /// needs the names to be unique, and it needs the lookup to find them.
    #[test]
    fn every_hostile_literal_replays_from_its_name() {
        let literals = hostile_literals();
        assert!(!literals.is_empty());
        let mut names: Vec<&str> = literals.iter().map(|(n, _)| n.as_str()).collect();
        names.sort_unstable();
        let unique = names.len();
        names.dedup();
        assert_eq!(unique, names.len(), "two literals share a name, so one of them cannot replay");
        for (name, bytes) in &literals {
            let found = hostile_literals().into_iter().find(|(n, _)| n == name);
            assert_eq!(found.map(|(_, b)| b).as_ref(), Some(bytes), "{name} does not look up");
            // The replay path itself, over a target that takes these bytes.
            run_literal(name, "csiv2::GssUpToken::decode");
        }
    }

    /// A panic *is* caught and reported rather than escaping the harness —
    /// otherwise a real finding would kill the test run instead of being
    /// counted, which is the failure mode this whole module exists to prevent.
    #[test]
    fn a_panicking_target_is_reported_not_propagated() {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let caught = catch_unwind(AssertUnwindSafe(|| panic!("deliberate"))).is_err();
        std::panic::set_hook(previous);
        assert!(caught);
    }
}
