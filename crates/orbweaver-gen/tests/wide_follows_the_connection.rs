//! A `wstring` from a stub takes its form from the connection, not from a
//! constant (D009 batch 3).
//!
//! `Cdr::put(&self, e: &mut Encoder)` has no connection to ask, so `WString`
//! answered with GIOP 1.2's form always. On a 1.1 connection that is the wrong
//! wire form, and nothing in this repository could see it: our own round trip
//! used the same constant at both ends, which is the shape the union-label
//! batch named — a convention both ends apply cannot be refuted by a round
//! trip.

use std::sync::Arc;

use orbweaver_cdr::{Decoder, Encoder, Endian, TextCodec};
use orbweaver_gen::rt::{Cdr, WString};
use orbweaver_giop::Version;
use orbweaver_giop::codeset::{CodeSetId, Codecs, WideCodec};

fn codec(v: Version) -> Arc<dyn TextCodec> {
    Arc::new(Codecs::new(None, Some(WideCodec::new(v, CodeSetId::UTF_16).expect("codec"))))
}

/// 1.2 counts octets; 1.1 counts characters. The same string is therefore a
/// different field, and a stub that cannot tell writes the wrong one.
#[test]
fn a_wstring_is_written_in_the_connections_form_not_a_constant() {
    let s = WString("wA".to_owned());

    let mut at_1_2 = Encoder::new(Endian::Big).with_codec(Some(codec(Version::V1_2)));
    s.put(&mut at_1_2).expect("1.2");
    let bytes_1_2 = at_1_2.finish().expect("finish");

    let mut at_1_1 = Encoder::new(Endian::Big).with_codec(Some(codec(Version::V1_1)));
    s.put(&mut at_1_1).expect("1.1");
    let bytes_1_1 = at_1_1.finish().expect("finish");

    assert_ne!(
        bytes_1_2, bytes_1_1,
        "the two GIOP versions must not produce the same wstring field; if they do, \
         the stub is still answering from a constant"
    );

    // And each reads back under its own form.
    for (v, bytes) in [(Version::V1_2, &bytes_1_2), (Version::V1_1, &bytes_1_1)] {
        let mut d = Decoder::new(bytes, Endian::Big).with_codec(Some(codec(v)));
        assert_eq!(WString::get(&mut d).expect("read back"), s, "{v:?}");
    }
}

/// A stream with nothing attached keeps the encapsulation rule: §9.3.1.6 makes
/// a `wchar` inside an encapsulation the 1.2 form whatever the message says,
/// and that is the one place a fixed answer is correct.
#[test]
fn a_stream_with_no_codec_still_uses_the_encapsulation_form() {
    let s = WString("wA".to_owned());

    let mut bare = Encoder::new(Endian::Big);
    s.put(&mut bare).expect("no codec");
    let bare_bytes = bare.finish().expect("finish");

    let mut at_1_2 = Encoder::new(Endian::Big).with_codec(Some(codec(Version::V1_2)));
    s.put(&mut at_1_2).expect("1.2");

    assert_eq!(
        bare_bytes,
        at_1_2.finish().expect("finish"),
        "an unattached stream must still write the 1.2 form the specification fixes \
         for an encapsulation"
    );
}
