//! The runtime generated code marshals through.
//!
//! Everything a stub emits is a call into this module, so the wire knowledge
//! lives here exactly once — the lesson Phase 3 paid for when `wstring` was
//! re-implemented instead of reused. A generated file contains **names and
//! order**, never encoding rules.

use orbweaver_giop::Version;
use orbweaver_giop::codeset::{CodeSetId, WideCodec};

pub use orbweaver_cdr::{Decoder, Encoder, Endian};
pub use orbweaver_giop::server::{Dispatch, DispatchBody, Request, Server, SystemException};
pub use orbweaver_giop::{Connection, Error as GiopError, Invoker, Ior, Reply};

/// Repository id every CORBA object answers `_is_a` to.
///
/// A generated skeleton answers `_is_a` from the registry's inheritance chain
/// plus this; an ORB probes with it before it will narrow, so a skeleton that
/// does not know it is one that cannot be narrowed to.
pub const OBJECT_ID: &str = "IDL:omg.org/CORBA/Object:1.0";

/// The marshalling contract every generated type implements.
///
/// The error is the GIOP error rather than the CDR one because two of the
/// types below (`AnyVal`, `ObjRef`) legitimately fail above the CDR layer, and
/// a second error type on the trait would push a conversion into every
/// generated member line.
pub trait Cdr: Sized {
    /// Writes `self` at the encoder's current position.
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError>;
    /// Reads one value.
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError>;
}

macro_rules! prim {
    ($t:ty, $put:ident, $get:ident) => {
        impl Cdr for $t {
            fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
                e.$put(*self);
                Ok(())
            }
            fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
                Ok(d.$get()?)
            }
        }
    };
}

prim!(bool, put_bool, get_bool);
prim!(u8, put_u8, get_u8);
prim!(i16, put_i16, get_i16);
prim!(u16, put_u16, get_u16);
prim!(i32, put_i32, get_i32);
prim!(u32, put_u32, get_u32);
prim!(i64, put_i64, get_i64);
prim!(u64, put_u64, get_u64);
prim!(f32, put_f32, get_f32);
prim!(f64, put_f64, get_f64);

/// `long double`: 16 raw octets, no portable Rust equivalent.
///
/// A newtype rather than `[u8; 16]`, because a bare 16-byte array already has
/// meaning under the generic array impl — 16 octets, each aligned to 1 — and a
/// `long double` is one value aligned to 8. Same bytes here, different type,
/// and letting them share an impl would hide that they differ on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LongDouble(pub [u8; 16]);

impl Cdr for LongDouble {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        e.put_long_double(self.0);
        Ok(())
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        Ok(LongDouble(d.get_long_double()?))
    }
}

impl Cdr for String {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        e.put_str(self);
        Ok(())
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        Ok(d.get_string()?)
    }
}

impl<T: Cdr> Cdr for Vec<T> {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        e.put_u32(self.len() as u32);
        for item in self {
            item.put(e)?;
        }
        Ok(())
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        let n = d.get_u32()?;
        // Validated against the bytes actually present, so a four-byte length
        // cannot buy a multi-gigabyte allocation (the Phase 0 audit's worst
        // finding, kept out of a third door).
        let n = d.validate_count(n, 1)?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(T::get(d)?);
        }
        Ok(out)
    }
}

/// IDL arrays: no length prefix, the length is in the type.
impl<T: Cdr, const N: usize> Cdr for [T; N] {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        for item in self {
            item.put(e)?;
        }
        Ok(())
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        let mut out = Vec::with_capacity(N);
        for _ in 0..N {
            out.push(T::get(d)?);
        }
        out.try_into().map_err(|_| GiopError::Decode("array length mismatch"))
    }
}

/// The one codec generated code uses for wide characters.
///
/// GIOP 1.2 with UTF-16 — the same choice as the dynamic path's default, so the
/// two paths produce identical bytes. Threading the connection's negotiated
/// codec through every generated signature is stream-B batch-2 work; until
/// then this is one constant in one place, not a rule copied into stubs.
fn wide() -> WideCodec {
    WideCodec::new(Version::V1_2, CodeSetId::UTF_16).expect("1.2 + UTF-16 is always valid")
}

/// `wstring`, distinct from `String` because the wire encoding differs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WString(pub String);

impl Cdr for WString {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        wide().put_wstring(e, &self.0).map_err(|_| GiopError::Decode("untranslatable wstring"))
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        wide().get_wstring(d).map(WString).map_err(|_| GiopError::Decode("malformed wstring"))
    }
}

/// `wchar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WChar(pub char);

impl Cdr for WChar {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        wide().put_wchar(e, self.0).map_err(|_| GiopError::Decode("wchar outside the BMP"))
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        wide().get_wchar(d).map(WChar).map_err(|_| GiopError::Decode("malformed wchar"))
    }
}

/// `any`: a value carrying its own description.
///
/// Static code keeps the dynamic representation here on purpose — an `any` is
/// dynamic by definition, and inventing a static mirror of `Value` would be a
/// second implementation of the same wire rules.
#[derive(Debug, Clone, PartialEq)]
pub struct AnyVal(pub orbweaver_giop::typecode::TypeCode, pub orbweaver_dynamic::Value);

impl Cdr for AnyVal {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        // The closure form keeps the value aligned against the outer stream;
        // building it detached restarts alignment at zero (the thrice-paid-for
        // Phase 1 lesson).
        orbweaver_giop::typecode::encode_any_with(e, &self.0, |enc| {
            let _ = orbweaver_dynamic::encode(enc, &self.0, &self.1);
        })?;
        // Re-run the validation the closure had to swallow.
        let mut probe = Encoder::new(Endian::Little);
        orbweaver_dynamic::encode(&mut probe, &self.0, &self.1)
            .map_err(|_| GiopError::Decode("any value does not match its TypeCode"))
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        let tc = orbweaver_giop::typecode::decode(d)?;
        let v = orbweaver_dynamic::decode(d, &tc)
            .map_err(|_| GiopError::Decode("any body does not match its TypeCode"))?;
        Ok(AnyVal(tc, v))
    }
}

/// An object reference, inline-marshalled; `None` is nil.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObjRef(pub Option<Ior>);

impl Cdr for ObjRef {
    fn put(&self, e: &mut Encoder) -> Result<(), GiopError> {
        match &self.0 {
            Some(ior) => ior.write_to(e),
            None => {
                // Nil: empty type id, zero profiles — not an absent field.
                e.put_str("");
                e.put_u32(0);
                Ok(())
            }
        }
    }
    fn get(d: &mut Decoder<'_>) -> Result<Self, GiopError> {
        let ior = Ior::read_from(d)?;
        Ok(if ior.type_id.is_empty() && ior.profiles.is_empty() {
            ObjRef(None)
        } else {
            ObjRef(Some(ior))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbweaver_dynamic::Value;
    use orbweaver_giop::typecode::TypeCode;

    /// The §8 criterion at the runtime layer: for every type this module
    /// marshals, the bytes must equal the dynamic path's bytes. Comparing
    /// against our own decoder alone would prove two halves of one file agree.
    fn same_bytes_as_dynamic<T: Cdr>(v: &T, tc: &TypeCode, dv: &Value) {
        for endian in [Endian::Big, Endian::Little] {
            let mut a = Encoder::new(endian);
            v.put(&mut a).expect("static put");
            let mut b = Encoder::new(endian);
            orbweaver_dynamic::encode(&mut b, tc, dv).expect("dynamic encode");
            assert_eq!(a.finish().unwrap(), b.finish().unwrap(), "{endian:?}");
        }
    }

    #[test]
    fn primitives_match_the_dynamic_bytes() {
        same_bytes_as_dynamic(&-7i32, &TypeCode::Long, &Value::Long(-7));
        same_bytes_as_dynamic(&true, &TypeCode::Boolean, &Value::Bool(true));
        same_bytes_as_dynamic(&2.5f64, &TypeCode::Double, &Value::Double(2.5));
        same_bytes_as_dynamic(
            &"hello".to_owned(),
            &TypeCode::String(0),
            &Value::String("hello".into()),
        );
    }

    #[test]
    fn sequences_and_arrays_match_the_dynamic_bytes() {
        let seq = TypeCode::Sequence { element: Box::new(TypeCode::Octet), bound: 0 };
        same_bytes_as_dynamic(
            &vec![1u8, 2, 3],
            &seq,
            &Value::List(vec![Value::Octet(1), Value::Octet(2), Value::Octet(3)]),
        );
        let arr = TypeCode::Array { element: Box::new(TypeCode::Long), length: 2 };
        same_bytes_as_dynamic(
            &[10i32, 20],
            &arr,
            &Value::List(vec![Value::Long(10), Value::Long(20)]),
        );
    }

    #[test]
    fn wide_text_matches_the_dynamic_bytes() {
        same_bytes_as_dynamic(
            &WString("안녕".into()),
            &TypeCode::WString(0),
            &Value::WString("안녕".into()),
        );
        same_bytes_as_dynamic(&WChar('한'), &TypeCode::WChar, &Value::WChar('한'));
    }

    #[test]
    fn any_and_object_references_match_the_dynamic_bytes() {
        same_bytes_as_dynamic(
            &AnyVal(TypeCode::Double, Value::Double(-0.125)),
            &TypeCode::Any,
            &Value::Any(Box::new(TypeCode::Double), Box::new(Value::Double(-0.125))),
        );
        let tc = TypeCode::ObjRef { id: "IDL:m/I:1.0".into(), name: "I".into() };
        same_bytes_as_dynamic(&ObjRef(None), &tc, &Value::ObjRef(None));
    }

    #[test]
    fn a_huge_declared_sequence_length_is_refused_not_allocated() {
        let bytes = [0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0];
        let mut d = Decoder::new(&bytes, Endian::Big);
        assert!(<Vec<f64> as Cdr>::get(&mut d).is_err());
    }
}
