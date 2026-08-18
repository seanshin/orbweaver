//! An encapsulation aligns from its own first byte, wherever it lands.
//!
//! §9.3.1.1: "Alignment is defined above as being relative to the beginning of
//! an octet stream. … Such octet streams begin at the start of a GIOP message
//! header and at the beginning of an encapsulation, **even if the encapsulation
//! itself is nested in another encapsulation**."
//!
//! That makes an encapsulation's contents a function of the encapsulation and
//! nothing else. A GIOP body, though, is built in a buffer of its own with
//! [`Encoder::continuing_at`], because CDR counts alignment from the start of
//! the message and a detached buffer starting at zero pads to the wrong
//! boundaries. Those two facts have to hold at the same time, and they did not:
//! `Encoder::position` added the virtual prefix *after* subtracting the origin,
//! so an encapsulation opened inside such a buffer aligned from the enclosing
//! message's offset instead of from its own flag byte.
//!
//! Nothing in the repository had ever encoded a `TypeCode` into a body encoder
//! at an offset that was not a multiple of eight, so nothing was red — and this
//! one did not even round-trip against itself. The reachable case is a GIOP 1.0
//! or 1.1 request or reply, whose header ends 4-aligned rather than 8-aligned,
//! carrying an `any` or a `TypeCode` over a `long long` discriminated union.

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_giop::typecode::{TypeCode, decode, encode};

/// `union W switch (long long) { case 1: long a; case 2: string b;
/// case 3: double c; }` as omniORB 4.3.4 marshalled it — the same recording
/// `union_labels_from_a_peer.rs` holds, kept here so this file states the
/// standard it is measuring against rather than borrowing one.
///
/// Bytes 9..12 are three the peer does not zero.
const LONG_LONG_DISCRIMINATED: &[u8] = &[
    0x10, 0x00, 0x00, 0x00, 0x74, 0x00, 0x00, 0x00, 0x01, 0x1d, 0x03, 0x6d, 0x0e, 0x00, 0x00, 0x00,
    0x49, 0x44, 0x4c, 0x3a, 0x75, 0x74, 0x32, 0x2f, 0x57, 0x3a, 0x31, 0x2e, 0x30, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x57, 0x00, 0x00, 0x00, 0x17, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x61, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x62, 0x00, 0x00, 0x00,
    0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x63, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00,
];

fn peer_union() -> TypeCode {
    decode(&mut Decoder::new(LONG_LONG_DISCRIMINATED, Endian::Little)).expect("decode")
}

/// A `TypeCode` always starts 4-aligned, so those are the offsets a body
/// encoder can really hand it; all four must produce the peer's bytes.
///
/// This is the peer-anchored form of the invariant. Asserting only that the
/// four encodings match *each other* would have been another oracle agreeing
/// with itself; the recording says which one of them is right.
#[test]
fn a_typecode_encodes_the_same_wherever_the_body_lands() {
    let tc = peer_union();
    for offset in [0usize, 4, 8, 12, 16, 20, 24, 28] {
        let mut e = Encoder::continuing_at(Endian::Little, offset);
        encode(&mut e, &tc).expect("encode");
        let ours = e.finish().expect("finish");

        assert_eq!(
            ours.len(),
            LONG_LONG_DISCRIMINATED.len(),
            "offset {offset}: length differs from the peer's"
        );
        for (i, (a, b)) in ours.iter().zip(LONG_LONG_DISCRIMINATED).enumerate() {
            if (9..12).contains(&i) {
                continue; // three octets the peer does not zero
            }
            assert_eq!(a, b, "offset {offset}: byte {i} is {a:#04x}, the peer wrote {b:#04x}");
        }
    }
}

/// The same buffers read back. Before the fix this failed on its own, without
/// any peer: the encoder padded the 8-byte union labels from the wrong base and
/// the decoder — which resets its origin correctly — could not find them.
#[test]
fn a_typecode_written_at_any_offset_reads_back() {
    let tc = peer_union();
    for endian in [Endian::Big, Endian::Little] {
        for offset in [0usize, 4, 8, 12, 16, 20, 24, 28] {
            let mut e = Encoder::continuing_at(endian, offset);
            encode(&mut e, &tc).expect("encode");
            let bytes = e.finish().expect("finish");
            let back = decode(&mut Decoder::new(&bytes, endian)).expect("decode");
            assert_eq!(back, tc, "{endian:?} at offset {offset}");
        }
    }
}

/// The property stated where it lives, rather than only through `TypeCode`:
/// an origin set inside a `continuing_at` buffer must count from itself.
#[test]
fn an_origin_set_inside_a_continuing_buffer_counts_from_itself() {
    for prefix in 0..16usize {
        let mut e = Encoder::continuing_at(Endian::Big, prefix);
        e.reset_origin();
        assert_eq!(e.position(), 0, "prefix {prefix}: a fresh origin is position zero");
        e.put_u8(1);
        e.put_u64(2); // must pad to offset 8 *within the encapsulation*
        assert_eq!(e.finish().expect("finish").len(), 16, "prefix {prefix}");
    }
}
