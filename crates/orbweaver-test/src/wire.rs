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

use std::panic::{AssertUnwindSafe, catch_unwind};

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_forge::{Finding, Severity};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_giop::{Ior, Version};

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
    fn label(self) -> &'static str {
        match self {
            Source::Uniform => "uniform",
            Source::Mutated => "mutated",
            Source::Truncated => "truncated",
        }
    }
}

/// One decoder under test, named for the report.
struct Target {
    name: &'static str,
    run: fn(&[u8]),
}

/// The decoders a peer can reach without authenticating: everything that runs
/// before any policy does.
fn targets() -> Vec<Target> {
    vec![
        Target {
            name: "giop::read_message",
            run: |b| {
                let mut cursor = std::io::Cursor::new(b);
                let _ = orbweaver_giop::read_message(&mut cursor, 64 * 1024);
            },
        },
        Target {
            name: "giop::read_one_message",
            run: |b| {
                let mut cursor = std::io::Cursor::new(b);
                let _ = orbweaver_giop::read_one_message(&mut cursor, 64 * 1024);
            },
        },
        Target {
            name: "server::decode_request",
            run: |b| {
                // The server's front door, and the only target here that a
                // peer reaches *before* any policy runs: read_message frames
                // it, this decodes it, and the guard chain has not been
                // consulted yet. A panic here is a refused caller stopping the
                // process it was refused by.
                let mut cursor = std::io::Cursor::new(b);
                if let Ok(msg) = orbweaver_giop::read_message(&mut cursor, 64 * 1024) {
                    let _ = orbweaver_giop::server::decode_request(msg);
                }
            },
        },
        Target {
            name: "giop::decode_reply",
            run: |b| {
                let mut cursor = std::io::Cursor::new(b);
                if let Ok(msg) = orbweaver_giop::read_message(&mut cursor, 64 * 1024) {
                    let _ = orbweaver_giop::decode_reply(msg);
                }
            },
        },
        Target {
            name: "giop::decode_locate_reply",
            run: |b| {
                let mut cursor = std::io::Cursor::new(b);
                if let Ok(msg) = orbweaver_giop::read_message(&mut cursor, 64 * 1024) {
                    let _ = orbweaver_giop::decode_locate_reply(msg);
                }
            },
        },
        Target {
            name: "typecode::decode",
            run: |b| {
                for endian in [Endian::Big, Endian::Little] {
                    let mut d = Decoder::new(b, endian);
                    let _ = orbweaver_giop::typecode::decode(&mut d);
                }
            },
        },
        Target {
            name: "Ior::read_from",
            run: |b| {
                for endian in [Endian::Big, Endian::Little] {
                    let mut d = Decoder::new(b, endian);
                    let _ = Ior::read_from(&mut d);
                }
            },
        },
        Target {
            name: "Ior::parse",
            run: |b| {
                // The stringified form is attacker-controlled too: it arrives
                // in configuration, in a corbaname, and out of a naming
                // service. Non-UTF-8 is discarded rather than lossily
                // converted, because `parse` takes a `&str` and lossy
                // conversion would be this test inventing an input.
                if let Ok(s) = std::str::from_utf8(b) {
                    let _ = Ior::parse(s);
                }
            },
        },
        Target {
            name: "dynamic::decode(recursive struct)",
            run: |b| {
                let tc = recursive_tree();
                for endian in [Endian::Big, Endian::Little] {
                    let mut d = Decoder::new(b, endian);
                    let _ = orbweaver_dynamic::decode(&mut d, &tc);
                }
            },
        },
        Target {
            name: "dynamic::decode(any)",
            run: |b| {
                // `any` is the sharpest of these: the bytes choose the
                // TypeCode, so the sender picks which decoder runs next.
                for endian in [Endian::Big, Endian::Little] {
                    let mut d = Decoder::new(b, endian);
                    let _ = orbweaver_dynamic::decode(&mut d, &TypeCode::Any);
                }
            },
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
    }
    out
}

/// Runs `cases` inputs against every target and reports every panic.
///
/// Silences the panic hook for the duration: a run that finds nothing should
/// print nothing, and a run that finds something reports it as a [`Finding`]
/// with the input attached rather than as a backtrace nobody kept.
pub fn panic_freedom(cases: usize, root: u64) -> Vec<Finding> {
    let seeds = seeds();
    let targets = targets();
    let mut out = Vec::new();

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    for i in 0..cases {
        let seed = case_seed(root, i as u64);
        let mut rng = Rng::new(seed);
        let (source, input) = make_input(&mut rng, &seeds);
        for t in &targets {
            let bytes = input.clone();
            if catch_unwind(AssertUnwindSafe(|| (t.run)(&bytes))).is_err() {
                out.push(finding(
                    "wire/panic",
                    Severity::Error,
                    format!(
                        "{} panicked on {} input of {} byte(s); a peer that can send these \
                         bytes can stop the process",
                        t.name,
                        source.label(),
                        input.len()
                    ),
                    t.name.to_string(),
                    Some(format!(
                        "reproduce with orbweaver_test::wire::run_case({seed:#x}, {:?}); the \
                         bytes are {}",
                        t.name,
                        hex(&input)
                    )),
                ));
            }
        }
    }

    std::panic::set_hook(previous);
    out
}

/// What a run actually reached, so a green result can be read.
///
/// A fuzz that never gets past the first length check is green and worthless,
/// and the exit code cannot tell the two apart. These counts can.
#[derive(Debug, Clone, Copy, Default)]
pub struct Reach {
    /// Inputs drawn uniformly at random.
    pub uniform: usize,
    /// Inputs made by flipping bits in a valid message.
    pub mutated: usize,
    /// Inputs made by cutting a valid message short.
    pub truncated: usize,
    /// How many inputs parsed as a GIOP message — the ones that reached past
    /// the header into the body decoders.
    pub parsed: usize,
}

/// Measures [`Reach`] for the same inputs [`panic_freedom`] would run.
pub fn reach(cases: usize, root: u64) -> Reach {
    let seeds = seeds();
    let mut r = Reach::default();
    for i in 0..cases {
        let mut rng = Rng::new(case_seed(root, i as u64));
        let (source, input) = make_input(&mut rng, &seeds);
        match source {
            Source::Uniform => r.uniform += 1,
            Source::Mutated => r.mutated += 1,
            Source::Truncated => r.truncated += 1,
        }
        let mut cursor = std::io::Cursor::new(&input[..]);
        if orbweaver_giop::read_message(&mut cursor, 64 * 1024).is_ok() {
            r.parsed += 1;
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
    let seeds = seeds();
    let mut rng = Rng::new(seed);
    let (_, input) = make_input(&mut rng, &seeds);
    for t in targets() {
        if t.name == target {
            (t.run)(&input);
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
