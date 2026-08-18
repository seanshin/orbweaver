//! The slot D009 batch 1 adds, and the two things it must be true of.
//!
//! Batch 1's oracle is **byte identity**: with no codec attached, every stream
//! writes what it wrote before. That is necessary and not sufficient — a slot
//! that is never consulted would pass it. So both halves are asserted here:
//! nothing changes when it is absent, and the right thing changes when it is
//! present.

use std::sync::Arc;

use orbweaver_cdr::{Decoder, Encoder, Endian, Error, TextCodec};

/// A deliberately silly codeset: ASCII with the case swapped. Not a real one —
/// a real one lives in `orbweaver-giop::codeset` and this crate must not learn
/// it. What it proves is that the octets on the wire came from the codec.
#[derive(Debug)]
struct SwapCase;

impl TextCodec for SwapCase {
    fn encode_narrow(&self, s: &str) -> Result<Vec<u8>, Error> {
        if !s.is_ascii() {
            return Err(Error::Malformed("swapcase carries ASCII only"));
        }
        Ok(s.bytes().map(|b| b ^ 0x20).collect())
    }

    fn decode_narrow(&self, bytes: &[u8]) -> Result<String, Error> {
        String::from_utf8(bytes.iter().map(|b| b ^ 0x20).collect())
            .map_err(|_| Error::Malformed("swapcase produced non-UTF-8"))
    }

    // The wide half is another codeset's business; these test the narrow one,
    // and a codec that refuses what it does not implement is the honest shape.
    fn put_wide(&self, _: &mut Encoder, _: &str) -> Result<(), Error> {
        Err(Error::Malformed("this test codec carries narrow text only"))
    }
    fn get_wide(&self, _: &mut Decoder<'_>) -> Result<String, Error> {
        Err(Error::Malformed("this test codec carries narrow text only"))
    }
    fn put_wide_char(&self, _: &mut Encoder, _: char) -> Result<(), Error> {
        Err(Error::Malformed("this test codec carries narrow text only"))
    }
    fn get_wide_char(&self, _: &mut Decoder<'_>) -> Result<char, Error> {
        Err(Error::Malformed("this test codec carries narrow text only"))
    }
}

fn swap() -> Option<Arc<dyn TextCodec>> {
    Some(Arc::new(SwapCase))
}

/// The batch's own oracle, stated as a test rather than as a claim in a commit
/// message: an absent codec is byte-for-byte what shipped before.
#[test]
fn no_codec_is_exactly_what_shipped_before() {
    for endian in [Endian::Big, Endian::Little] {
        let mut plain = Encoder::new(endian);
        plain.put_str("Orbweaver");
        let mut slotted = Encoder::new(endian).with_codec(None);
        slotted.put_str("Orbweaver");
        assert_eq!(plain.finish().unwrap(), slotted.finish().unwrap(), "{endian:?}");
    }
}

/// And the half byte identity cannot see.
#[test]
fn an_attached_codec_decides_the_octets() {
    let mut e = Encoder::new(Endian::Big).with_codec(swap());
    e.put_str("abc");
    let bytes = e.finish().unwrap();
    // length counts the NUL, and the NUL is the *stream's*, not the codec's.
    assert_eq!(bytes, vec![0, 0, 0, 4, b'A', b'B', b'C', 0]);

    let back = Decoder::new(&bytes, Endian::Big).with_codec(swap()).get_string().unwrap();
    assert_eq!(back, "abc");

    // Read without the codec and the octets are what the codec wrote — proof
    // the conversion happened at the wire and not somewhere in the caller.
    let raw = Decoder::new(&bytes, Endian::Big).get_string().unwrap();
    assert_eq!(raw, "ABC");
}

/// The framing rules stay in one place. A codec supplies octets; it does not
/// get to decide whether the NUL is counted, and it cannot smuggle one in.
#[test]
fn the_framing_rules_are_still_the_streams() {
    #[derive(Debug)]
    struct Nul;
    impl TextCodec for Nul {
        fn encode_narrow(&self, _: &str) -> Result<Vec<u8>, Error> {
            Ok(vec![b'a', 0, b'b'])
        }
        fn decode_narrow(&self, b: &[u8]) -> Result<String, Error> {
            Ok(String::from_utf8_lossy(b).into_owned())
        }

        // The wide half is another codeset's business; these test the narrow one,
        // and a codec that refuses what it does not implement is the honest shape.
        fn put_wide(&self, _: &mut Encoder, _: &str) -> Result<(), Error> {
            Err(Error::Malformed("this test codec carries narrow text only"))
        }
        fn get_wide(&self, _: &mut Decoder<'_>) -> Result<String, Error> {
            Err(Error::Malformed("this test codec carries narrow text only"))
        }
        fn put_wide_char(&self, _: &mut Encoder, _: char) -> Result<(), Error> {
            Err(Error::Malformed("this test codec carries narrow text only"))
        }
        fn get_wide_char(&self, _: &mut Decoder<'_>) -> Result<char, Error> {
            Err(Error::Malformed("this test codec carries narrow text only"))
        }
    }
    let mut e = Encoder::new(Endian::Big).with_codec(Some(Arc::new(Nul)));
    e.put_str("anything");
    assert!(matches!(e.finish(), Err(Error::EmbeddedNul)), "the stream refuses an embedded NUL");
}

/// A codeset that cannot represent the text poisons the encoder rather than
/// writing something else — the same shape every other write failure has.
#[test]
fn text_the_codeset_cannot_carry_is_a_refusal_not_a_substitution() {
    let mut e = Encoder::new(Endian::Big).with_codec(swap());
    e.put_str("한글");
    assert!(matches!(e.finish(), Err(Error::Malformed(_))), "must refuse, not transliterate");
}

/// An encapsulation starts clean. Inheriting would re-encode the repository
/// ids and member names inside every `TypeCode`, which are the contract's own
/// identifiers and not the peer's text.
#[test]
fn an_encapsulation_does_not_inherit_the_codec() {
    let mut outer = Encoder::new(Endian::Big).with_codec(swap());
    outer.put_str("abc");
    let mut inner = Encoder::encapsulation(Endian::Big);
    inner.put_str("abc");
    let inner_bytes = inner.finish().unwrap();
    // byte 0 is the encapsulation's own flag; the string follows, unswapped.
    assert!(inner_bytes.ends_with(&[b'a', b'b', b'c', 0]), "{inner_bytes:?}");
    let _ = outer.finish().unwrap();
}
