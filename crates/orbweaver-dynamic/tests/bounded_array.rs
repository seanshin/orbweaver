//! An array's length used to be ours; since D008 it is a number in a document
//! an agent sends.
//!
//! `agent-fuzz` reached `TypeCode::Array { length: 4_294_967_295 }` from a
//! **198-byte** document — `array<octet, 4294967295>` as a union discriminator
//! — and the decoder reserved 206 GB before reading a byte. It was refused a
//! moment later for a truncated stream, which is why nothing looked wrong: the
//! reservation had already happened and was thrown away. Lazily backed on
//! macOS/arm64; uncatchably fatal under a memory limit or on a 32-bit target.
//!
//! The `Sequence` arm fourteen lines above had carried this guard since Phase
//! 0, with a comment naming the rule. The `Array` arm did not need it while
//! every TypeCode it decoded against had been compiled here.

use orbweaver_cdr::{Decoder, Endian};
use orbweaver_dynamic::{Value, decode};
use orbweaver_giop::typecode::TypeCode;

fn array(length: u32) -> TypeCode {
    TypeCode::Array { element: Box::new(TypeCode::Octet), length }
}

/// The refusal must name the length, and must arrive **before** the buffer is
/// sized — which is what a "need N bytes, have M" message from the *first
/// element* would not tell us.
#[test]
fn a_declared_array_length_no_buffer_could_hold_is_refused_first() {
    let err = decode(&mut Decoder::new(&[0u8; 8], Endian::Big), &array(4_294_967_295))
        .expect_err("4 billion octets cannot come out of eight bytes");
    assert!(
        err.message.contains("4294967295") || err.message.contains("count"),
        "the refusal should be about the declared count, not about the first element: {err}"
    );
}

/// And the guard must not refuse an array a buffer *can* hold.
#[test]
fn an_array_the_buffer_can_hold_still_decodes() {
    let bytes = [1u8, 2, 3, 4];
    let v = decode(&mut Decoder::new(&bytes, Endian::Big), &array(4)).expect("four octets");
    assert_eq!(
        v,
        Value::List(vec![Value::Octet(1), Value::Octet(2), Value::Octet(3), Value::Octet(4)])
    );
}

/// A zero-length array is legal and is not a truncation.
#[test]
fn an_empty_array_is_not_a_truncation() {
    let v = decode(&mut Decoder::new(&[], Endian::Big), &array(0)).expect("empty");
    assert_eq!(v, Value::List(Vec::new()));
}
