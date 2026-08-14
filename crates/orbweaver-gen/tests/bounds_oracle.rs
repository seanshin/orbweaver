//! §8, in the reading that catches a dropped bound: **static and dynamic must
//! refuse alike.**
//!
//! # Why the existing oracle could not have found this
//!
//! §8's rule is *static result equals dynamic result*, and every instrument
//! that enforces it — `static-oracle`, `skeleton_oracle.rs`, `rt.rs`'s
//! `same_bytes_as_dynamic` — compares **bytes**. Bytes only exist for a value
//! both paths agreed to encode. A value that violates a declared bound produces
//! no bytes on the dynamic path at all, so it is not a case the byte comparison
//! can generate: the two paths disagreed about whether the case *exists*, and
//! the oracle measured the empty intersection and reported agreement.
//!
//! That is exactly how the defect survived. `rust_type` mapped
//! `sequence<octet, 6>` to a bare `Vec<u8>`, discarding the bound, and
//! `impl Cdr for Vec<T>` wrote `self.len()` unchecked — so a generated stub
//! sent seven octets where the dynamic path refused them, and a generated
//! skeleton accepted seven where the dynamic one rejected them. Every
//! byte-equality test stayed green throughout, because every value it tried was
//! within the bound. `docs/decisions/D006-plane-rule-tensor.md` §2 measured the
//! divergence by reading the two sources, while arguing about something else.
//!
//! # The strengthened rule this file enforces
//!
//! For every value, conforming **or violating**, over both byte orders:
//!
//! * **encode** — the two paths write the same bytes and then reach the same
//!   verdict. Comparing `Encoder::as_bytes()` after the call is what pins "at
//!   the same point": a static path that checked the bound one member later
//!   would have written that member and fail here even though its verdict
//!   agreed.
//! * **decode** — the two paths consume the same number of octets and then
//!   reach the same verdict. A sequence bound is refused immediately after the
//!   length prefix on both, so a hostile length costs neither path an
//!   allocation.
//!
//! The refusal *messages* are not compared, and cannot be: the two paths return
//! different error types (`rt::GiopError` against `orbweaver_dynamic::Error`),
//! and `GiopError::Decode` carries a `&'static str`, so the declared bound
//! cannot be written into it under the workspace MSRV. What is compared is the
//! verdict, the point, and that the static message names the bound at all.
//!
//! # The asymmetry this file pins rather than fixes
//!
//! **A `string`/`wstring` bound is enforced on encode and not on decode**, on
//! *both* paths. That is the reference implementation's behaviour —
//! `orbweaver_dynamic` calls `check_bound` at `lib.rs:461`/`:466` and its
//! decoder does not — and the static path copies it deliberately. Enforcing on
//! decode here would make generated code refuse a message the dynamic path
//! accepts, which is the same divergence pointing the other way.
//! [`a_string_bound_is_encode_only_on_both_paths`] fails if either side changes
//! alone.

mod emitted;

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_dynamic::Value;
use orbweaver_gen::rt::{self, Cdr, GiopError, ObjectHome, WString};
use orbweaver_giop::typecode::TypeCode;
use orbweaver_giop::{DEFAULT_MAX_MESSAGE_SIZE, Version, encode_request, read_message};
use orbweaver_registry::Registry;

use emitted::f_27_bounds::gc27::{
    Blob, BlobSeq, LedgerClient, LedgerFault, LedgerRefs, LedgerServant, LedgerSkeleton,
    LedgerTarget, Record, Tag, TagSeq, TooBig, WideTag,
};

const BOTH: [Endian; 2] = [Endian::Big, Endian::Little];

/// The contract under test, loaded once per test from the corpus file the
/// fixture was generated from — so the dynamic path reads the same declaration
/// the generator read, rather than one retyped here.
fn registry() -> Registry {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = std::fs::read_to_string(root.join("../../corpus/golden/27-bounds.idl"))
        .expect("corpus/golden/27-bounds.idl");
    let spec = orbweaver_idl::parse(&src).expect("the corpus file parses");
    let mut registry = Registry::new();
    registry.load(&spec).expect("loads");
    registry
}

fn tc(registry: &Registry, id: &str) -> TypeCode {
    registry.typecode(id).unwrap_or_else(|| panic!("{id} is not in the registry")).clone()
}

/// Both paths, one value, both byte orders: same bytes written, same verdict.
///
/// `expect` says which verdict this case is *about*, so a case that stopped
/// exercising its bound — a corpus edit that widened one, say — fails loudly
/// instead of passing as a conforming value nobody meant to write.
fn encode_alike(
    what: &str,
    tc: &TypeCode,
    dynamic_value: &Value,
    put_static: &dyn Fn(&mut Encoder) -> Result<(), GiopError>,
    expect: Verdict,
) {
    for endian in BOTH {
        let mut a = Encoder::new(endian);
        let statik = put_static(&mut a);
        let mut b = Encoder::new(endian);
        let dynamic = orbweaver_dynamic::encode(&mut b, tc, dynamic_value);

        assert_eq!(
            statik.is_err(),
            dynamic.is_err(),
            "{what} ({endian:?}): the paths disagree — static {statik:?}, dynamic {dynamic:?}"
        );
        assert_eq!(
            statik.is_err(),
            expect == Verdict::Refused,
            "{what} ({endian:?}): expected {expect:?}, static said {statik:?}"
        );
        // The point, not just the verdict: a check one member too late writes
        // that member first, and this is where that shows.
        assert_eq!(
            a.as_bytes(),
            b.as_bytes(),
            "{what} ({endian:?}): the paths stopped at different points"
        );
        if expect == Verdict::Refused {
            let message = statik.unwrap_err().to_string();
            assert!(message.contains("bound"), "{what}: {message:?} does not name the bound");
            let message = dynamic.unwrap_err().to_string();
            assert!(message.contains("bounded"), "{what}: {message:?} does not name the bound");
        }
    }
}

/// Both paths, one wire message: same octets consumed, same verdict.
fn decode_alike(
    what: &str,
    tc: &TypeCode,
    bytes: &[u8],
    endian: Endian,
    get_static: &dyn Fn(&mut Decoder<'_>) -> Result<(), GiopError>,
    expect: Verdict,
) {
    let mut a = Decoder::new(bytes, endian);
    let statik = get_static(&mut a);
    let mut b = Decoder::new(bytes, endian);
    let dynamic = orbweaver_dynamic::decode(&mut b, tc);

    assert_eq!(
        statik.is_err(),
        dynamic.is_err(),
        "{what} ({endian:?}): the paths disagree — static {statik:?}, dynamic {dynamic:?}"
    );
    assert_eq!(
        statik.is_err(),
        expect == Verdict::Refused,
        "{what} ({endian:?}): expected {expect:?}, static said {statik:?}"
    );
    assert_eq!(
        a.offset(),
        b.offset(),
        "{what} ({endian:?}): the paths stopped at different offsets — a refusal after the \
         elements costs an allocation a refusal at the length prefix does not"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Accepted,
    Refused,
}

/// The reference encoder, over an *unbounded* twin of the declared type: this
/// is how a message that violates a bound is produced at all. A peer built
/// against a wider contract sends exactly these bytes.
fn wire(twin: &TypeCode, value: &Value, endian: Endian) -> Vec<u8> {
    let mut e = Encoder::new(endian);
    orbweaver_dynamic::encode(&mut e, twin, value).expect("the unbounded twin encodes");
    e.finish().expect("finish")
}

fn octets(n: usize) -> Value {
    Value::List((0..n).map(|i| Value::Octet(i as u8)).collect())
}

fn unbounded_octets() -> TypeCode {
    TypeCode::Sequence { element: Box::new(TypeCode::Octet), bound: 0 }
}

// ── Placement 1: a bounded sequence as a top-level typedef ───────────────────

#[test]
fn a_bounded_sequence_typedef_is_enforced_on_encode_by_both_paths() {
    let r = registry();
    let blob = tc(&r, "IDL:gc27/Blob:1.0");
    encode_alike(
        "Blob at its bound",
        &blob,
        &octets(6),
        &|e| Blob::new((0..6u8).collect()).put(e),
        Verdict::Accepted,
    );
    encode_alike(
        "Blob one octet past its bound",
        &blob,
        &octets(7),
        &|e| Blob::new((0..7u8).collect()).put(e),
        Verdict::Refused,
    );
}

#[test]
fn a_bounded_sequence_typedef_is_enforced_on_decode_by_both_paths() {
    let r = registry();
    let blob = tc(&r, "IDL:gc27/Blob:1.0");
    for endian in BOTH {
        decode_alike(
            "Blob at its bound",
            &blob,
            &wire(&unbounded_octets(), &octets(6), endian),
            endian,
            &|d| Blob::get(d).map(|_| ()),
            Verdict::Accepted,
        );
        decode_alike(
            "Blob one octet past its bound",
            &blob,
            &wire(&unbounded_octets(), &octets(7), endian),
            endian,
            &|d| Blob::get(d).map(|_| ()),
            Verdict::Refused,
        );
    }
}

// ── Placement 2: bounded string and wstring typedefs ─────────────────────────

#[test]
fn a_bounded_string_typedef_is_enforced_on_encode_by_both_paths() {
    let r = registry();
    let tag = tc(&r, "IDL:gc27/Tag:1.0");
    encode_alike(
        "Tag at its bound",
        &tag,
        &Value::String("12345678".into()),
        &|e| Tag::new("12345678".to_owned()).put(e),
        Verdict::Accepted,
    );
    encode_alike(
        "Tag one character past its bound",
        &tag,
        &Value::String("123456789".into()),
        &|e| Tag::new("123456789".to_owned()).put(e),
        Verdict::Refused,
    );
    // Characters, not octets: eight Korean syllables are twenty-four octets and
    // are inside `string<8>` on both paths. A byte-counting implementation
    // would refuse this and agree with nobody.
    encode_alike(
        "Tag of eight non-ASCII characters",
        &tag,
        &Value::String("가나다라마바사아".into()),
        &|e| Tag::new("가나다라마바사아".to_owned()).put(e),
        Verdict::Accepted,
    );
}

#[test]
fn a_bounded_wstring_typedef_is_enforced_on_encode_by_both_paths() {
    let r = registry();
    let wide = tc(&r, "IDL:gc27/WideTag:1.0");
    encode_alike(
        "WideTag at its bound",
        &wide,
        &Value::WString("가나다라".into()),
        &|e| WideTag::new(WString("가나다라".into())).put(e),
        Verdict::Accepted,
    );
    encode_alike(
        "WideTag one code unit past its bound",
        &wide,
        &Value::WString("가나다라마".into()),
        &|e| WideTag::new(WString("가나다라마".into())).put(e),
        Verdict::Refused,
    );
}

/// The measured asymmetry, pinned so neither side can change alone.
///
/// `orbweaver_dynamic` checks a `string`/`wstring` bound on encode and not on
/// decode. The static path copies that, so a nine-character `string<8>` on the
/// wire is accepted by both. This test is not an endorsement of the rule — it
/// is the tripwire that makes changing it a two-crate decision instead of a
/// silent divergence, which is the whole class of defect this file exists for.
#[test]
fn a_string_bound_is_encode_only_on_both_paths() {
    let r = registry();
    let tag = tc(&r, "IDL:gc27/Tag:1.0");
    let wide = tc(&r, "IDL:gc27/WideTag:1.0");
    for endian in BOTH {
        decode_alike(
            "a nine-character Tag arriving from a wider peer",
            &tag,
            &wire(&TypeCode::String(0), &Value::String("123456789".into()), endian),
            endian,
            &|d| Tag::get(d).map(|_| ()),
            Verdict::Accepted,
        );
        decode_alike(
            "a five-unit WideTag arriving from a wider peer",
            &wide,
            &wire(&TypeCode::WString(0), &Value::WString("가나다라마".into()), endian),
            endian,
            &|d| WideTag::get(d).map(|_| ()),
            Verdict::Accepted,
        );
    }
}

// ── Placement 3: a bounded member inside a struct ────────────────────────────

fn record_value(label: &str, payload: usize, wide: &str) -> Value {
    Value::Struct(vec![
        ("label".into(), Value::String(label.into())),
        ("payload".into(), octets(payload)),
        ("wide".into(), Value::WString(wide.into())),
    ])
}

fn record(label: &str, payload: usize, wide: &str) -> Record {
    Record {
        label: Tag::new(label.to_owned()),
        payload: Blob::new((0..payload as u8).collect()),
        wide: WideTag::new(WString(wide.to_owned())),
    }
}

/// A struct whose members are all unbounded, for producing wire bytes a wider
/// peer would send.
fn unbounded_record() -> TypeCode {
    TypeCode::Struct {
        id: "IDL:gc27/Record:1.0".into(),
        name: "Record".into(),
        members: vec![
            orbweaver_giop::typecode::Member { name: "label".into(), tc: TypeCode::String(0) },
            orbweaver_giop::typecode::Member { name: "payload".into(), tc: unbounded_octets() },
            orbweaver_giop::typecode::Member { name: "wide".into(), tc: TypeCode::WString(0) },
        ],
    }
}

#[test]
fn a_bounded_member_inside_a_struct_is_enforced_by_both_paths() {
    let r = registry();
    let rec = tc(&r, "IDL:gc27/Record:1.0");
    encode_alike(
        "Record within every bound",
        &rec,
        &record_value("ok", 6, "가나다라"),
        &|e| record("ok", 6, "가나다라").put(e),
        Verdict::Accepted,
    );
    // The second member is the one that violates, so the label is written by
    // both paths before either refuses — which is what `encode_alike`'s byte
    // comparison is for.
    encode_alike(
        "Record whose second member is past its bound",
        &rec,
        &record_value("ok", 7, "가나다라"),
        &|e| record("ok", 7, "가나다라").put(e),
        Verdict::Refused,
    );
    for endian in BOTH {
        decode_alike(
            "Record whose second member arrives past its bound",
            &rec,
            &wire(&unbounded_record(), &record_value("ok", 7, "가나다라"), endian),
            endian,
            &|d| Record::get(d).map(|_| ()),
            Verdict::Refused,
        );
    }
}

// ── Placement 4: a bounded element inside a sequence ─────────────────────────

#[test]
fn a_bounded_element_inside_a_bounded_sequence_is_enforced_by_both_paths() {
    let r = registry();
    let tags = tc(&r, "IDL:gc27/TagSeq:1.0");
    let three = |a: &str, b: &str, c: &str| {
        Value::List(vec![Value::String(a.into()), Value::String(b.into()), Value::String(c.into())])
    };
    let built = |a: &str, b: &str, c: &str| {
        TagSeq::new(vec![Tag::new(a.to_owned()), Tag::new(b.to_owned()), Tag::new(c.to_owned())])
    };
    encode_alike(
        "TagSeq at both bounds",
        &tags,
        &three("a", "b", "c"),
        &|e| built("a", "b", "c").put(e),
        Verdict::Accepted,
    );
    // The *element* bound, with the outer one satisfied: three tags, the last
    // of them nine characters.
    encode_alike(
        "TagSeq whose third element is past the element bound",
        &tags,
        &three("a", "b", "123456789"),
        &|e| built("a", "b", "123456789").put(e),
        Verdict::Refused,
    );
    // The *outer* bound, with every element inside its own.
    let four = Value::List((0..4).map(|i| Value::String(format!("t{i}"))).collect());
    encode_alike(
        "TagSeq with a fourth element",
        &tags,
        &four,
        &|e| TagSeq::new((0..4).map(|i| Tag::new(format!("t{i}"))).collect()).put(e),
        Verdict::Refused,
    );
    for endian in BOTH {
        let twin = TypeCode::Sequence { element: Box::new(TypeCode::String(0)), bound: 0 };
        decode_alike(
            "TagSeq with a fourth element arriving",
            &tags,
            &wire(&twin, &four, endian),
            endian,
            &|d| TagSeq::get(d).map(|_| ()),
            Verdict::Refused,
        );
    }
}

#[test]
fn a_bounded_element_inside_an_unbounded_sequence_is_enforced_by_both_paths() {
    let r = registry();
    let blobs = tc(&r, "IDL:gc27/BlobSeq:1.0");
    let batch = |sizes: &[usize]| Value::List(sizes.iter().map(|n| octets(*n)).collect());
    let built = |sizes: &[usize]| -> BlobSeq {
        sizes.iter().map(|n| Blob::new((0..*n as u8).collect())).collect()
    };
    encode_alike(
        "BlobSeq of conforming blobs",
        &blobs,
        &batch(&[1, 6, 0]),
        &|e| built(&[1, 6, 0]).put(e),
        Verdict::Accepted,
    );
    encode_alike(
        "BlobSeq whose second blob is past the element bound",
        &blobs,
        &batch(&[1, 7, 0]),
        &|e| built(&[1, 7, 0]).put(e),
        Verdict::Refused,
    );
    for endian in BOTH {
        let twin = TypeCode::Sequence { element: Box::new(unbounded_octets()), bound: 0 };
        decode_alike(
            "BlobSeq whose second blob arrives past the element bound",
            &blobs,
            &wire(&twin, &batch(&[1, 7, 0]), endian),
            endian,
            &|d| BlobSeq::get(d).map(|_| ()),
            Verdict::Refused,
        );
    }
}

// ── Placement 5: a bounded field in an exception ─────────────────────────────

#[test]
fn a_bounded_field_in_an_exception_is_enforced_by_both_paths() {
    let r = registry();
    let too_big = tc(&r, "IDL:gc27/TooBig:1.0");
    let value = |offending: usize| {
        Value::Struct(vec![
            ("reason".into(), Value::String("why".into())),
            ("offending".into(), octets(offending)),
        ])
    };
    let built = |offending: usize| TooBig {
        reason: Tag::new("why".to_owned()),
        offending: Blob::new((0..offending as u8).collect()),
    };
    encode_alike(
        "TooBig within its bounds",
        &too_big,
        &value(6),
        &|e| built(6).put(e),
        Verdict::Accepted,
    );
    encode_alike(
        "TooBig whose offending blob is itself too big",
        &too_big,
        &value(7),
        &|e| built(7).put(e),
        Verdict::Refused,
    );
    for endian in BOTH {
        let twin = TypeCode::Except {
            id: "IDL:gc27/TooBig:1.0".into(),
            name: "TooBig".into(),
            members: vec![
                orbweaver_giop::typecode::Member { name: "reason".into(), tc: TypeCode::String(0) },
                orbweaver_giop::typecode::Member {
                    name: "offending".into(),
                    tc: unbounded_octets(),
                },
            ],
        };
        decode_alike(
            "TooBig arriving with an over-long offending blob",
            &too_big,
            &wire(&twin, &value(7), endian),
            endian,
            &|d| TooBig::get(d).map(|_| ()),
            Verdict::Refused,
        );
    }
}

// ── Placement 6: a bounded parameter, through the generated stub ─────────────

/// An invoker that records whether anything was ever sent, and never answers.
///
/// The point of the recording is the *absence*: a stub that refuses an
/// over-bound argument must refuse it before a request exists, because the
/// alternative — sending it and letting the peer refuse — is exactly the
/// behaviour the dynamic path does not have.
struct Recording {
    endian: Endian,
    sent: Vec<String>,
}

impl rt::Invoker for Recording {
    fn endian(&self) -> Endian {
        self.endian
    }
    fn invoke<F: Fn(&mut Encoder)>(
        &mut self,
        operation: &str,
        _write_args: F,
    ) -> Result<rt::Reply, GiopError> {
        self.sent.push(operation.to_owned());
        Err(GiopError::ConnectionClosed)
    }
    fn invoke_oneway<F: Fn(&mut Encoder)>(
        &mut self,
        operation: &str,
        _write_args: F,
    ) -> Result<(), GiopError> {
        self.sent.push(operation.to_owned());
        Err(GiopError::ConnectionClosed)
    }
}

#[test]
fn a_generated_stub_refuses_an_over_bound_argument_before_it_is_sent() {
    for endian in BOTH {
        let mut client = LedgerClient::new(Recording { endian, sent: Vec::new() });

        let refused = client.set_title(Tag::new("123456789".to_owned()));
        match refused {
            Err(GiopError::Decode(why)) => {
                assert!(why.contains("bound"), "{why:?} does not name the bound");
            }
            other => panic!("a nine-character string<8> was not refused: {other:?}"),
        }
        assert!(client.conn.sent.is_empty(), "the request was sent anyway: {:?}", client.conn.sent);

        // The control: the same call inside the bound does reach the invoker,
        // so the refusal above is the bound and not a stub that never sends.
        let sent = client.set_title(Tag::new("12345678".to_owned()));
        assert!(matches!(sent, Err(GiopError::ConnectionClosed)), "{sent:?}");
        assert_eq!(client.conn.sent, vec!["_set_title".to_owned()]);

        // A bounded member reached through a parameter's *type*, rather than
        // the parameter itself.
        let mut client = LedgerClient::new(Recording { endian, sent: Vec::new() });
        let refused = client.keep(Tag::new("k".to_owned()), record("ok", 7, "가"));
        assert!(matches!(refused, Err(GiopError::Decode(_))), "{refused:?}");
        assert!(client.conn.sent.is_empty(), "{:?}", client.conn.sent);
    }
}

// ── Placement 7: a bounded parameter, through the generated skeleton ─────────

const KEY: &[u8] = b"bounds";

/// A servant whose answers are chosen by the test.
///
/// `Answer::Conforming` makes every reply fit its bounds; the other two make a
/// servant reply with a value its own contract forbids, which is the direction
/// nothing else here measures — a servant is inside the trust boundary and can
/// hand the skeleton anything its Rust types allow.
struct Canned(Answer);

#[derive(Clone, Copy)]
enum Answer {
    Conforming,
    /// A `TagSeq` of four, where the contract bounds it at three.
    OverBoundReturn,
    /// A `TooBig` whose own `Blob` field is over its bound.
    OverBoundException,
}

impl LedgerServant for Canned {
    fn knows(&self, at: &LedgerTarget<'_>) -> bool {
        at.is_default()
    }
    fn digest(&mut self, _at: &LedgerTarget<'_>, _batch: BlobSeq) -> Result<TagSeq, LedgerFault> {
        match self.0 {
            Answer::OverBoundReturn => {
                Ok(TagSeq::new((0..4).map(|i| Tag::new(format!("t{i}"))).collect()))
            }
            _ => Ok(TagSeq::new(vec![Tag::new("d".to_owned())])),
        }
    }
    fn keep(
        &mut self,
        _at: &LedgerTarget<'_>,
        _key: Tag,
        entry: Record,
    ) -> Result<Record, LedgerFault> {
        match self.0 {
            Answer::OverBoundException => Err(LedgerFault::TooBig(TooBig {
                reason: Tag::new("nope".to_owned()),
                offending: Blob::new((0..7u8).collect()),
            })),
            _ => Ok(entry),
        }
    }
    fn title(&mut self, _at: &LedgerTarget<'_>) -> Result<Tag, LedgerFault> {
        Ok(Tag::new("t".to_owned()))
    }
    fn set_title(&mut self, _at: &LedgerTarget<'_>, _value: Tag) -> Result<(), LedgerFault> {
        Ok(())
    }
}

/// Dispatches one request whose arguments were written by the **dynamic**
/// encoder over `twin`, and says whether the skeleton accepted it.
fn dispatch_to(
    servant: Answer,
    operation: &str,
    twin: &TypeCode,
    args: &Value,
    endian: Endian,
) -> Result<(), String> {
    let refs = LedgerRefs::new(ObjectHome::new("127.0.0.1", 0, KEY.to_vec()));
    let mut skeleton = LedgerSkeleton::new(refs, Canned(servant));
    let bytes = encode_request(Version::V1_2, endian, 1, KEY, operation, true, |e| {
        orbweaver_dynamic::encode(e, twin, args).expect("the unbounded twin encodes");
    })
    .expect("encode request");
    let mut cursor: &[u8] = &bytes;
    let message = read_message(&mut cursor, DEFAULT_MAX_MESSAGE_SIZE).expect("frame");
    let request = orbweaver_giop::server::decode_request(message).expect("decode request");
    let mut out = Encoder::continuing_at(endian, 24);
    rt::Dispatch::dispatch_body(&mut skeleton, &request, &mut out)
        .map(|_| ())
        .map_err(|ex| ex.id.clone())
}

fn dispatch(operation: &str, twin: &TypeCode, args: &Value, endian: Endian) -> Result<(), String> {
    dispatch_to(Answer::Conforming, operation, twin, args, endian)
}

#[test]
fn a_generated_skeleton_refuses_an_over_bound_argument_the_dynamic_path_would_reject() {
    let r = registry();
    let blobs = tc(&r, "IDL:gc27/BlobSeq:1.0");
    let twin = TypeCode::Sequence { element: Box::new(unbounded_octets()), bound: 0 };
    for endian in BOTH {
        let conforming = Value::List(vec![octets(6)]);
        let violating = Value::List(vec![octets(7)]);

        assert_eq!(dispatch("digest", &twin, &conforming, endian), Ok(()));

        // The verdict a caller sees, and the one the dynamic path gives for the
        // same body: a refusal. `MARSHAL` is what a generated skeleton turns a
        // decode failure into, so the two agree on the outcome and the skeleton
        // supplies the CORBA name.
        let refused = dispatch("digest", &twin, &violating, endian);
        assert_eq!(refused, Err(rt::MARSHAL.to_owned()), "{endian:?}");

        // ...and the dynamic path really does refuse this body, rather than
        // this being a claim about it.
        let bytes = wire(&twin, &violating, endian);
        let mut d = Decoder::new(&bytes, endian);
        assert!(
            orbweaver_dynamic::decode(&mut d, &blobs).is_err(),
            "the dynamic path accepted what the skeleton refused"
        );
    }
}

/// The reply direction: a *servant* may not exceed its own declared bound
/// either.
///
/// This is the half no wire test can produce, because the value never comes
/// from a peer — a servant is inside the trust boundary and hands the skeleton
/// whatever its Rust types allow. Before the bound was in the type, `TagSeq`
/// was a bare `Vec<Tag>` and four tags marshalled cleanly into a reply that a
/// conformant client is entitled to refuse. Now the skeleton refuses first,
/// which is the same verdict a dynamic servant would have got from
/// `orbweaver_dynamic::encode` for the same value.
#[test]
fn a_generated_skeleton_refuses_an_over_bound_reply_from_its_own_servant() {
    let r = registry();
    let tags = tc(&r, "IDL:gc27/TagSeq:1.0");
    let twin = TypeCode::Sequence { element: Box::new(unbounded_octets()), bound: 0 };
    let batch = Value::List(vec![octets(1)]);
    for endian in BOTH {
        assert_eq!(dispatch("digest", &twin, &batch, endian), Ok(()));
        assert_eq!(
            dispatch_to(Answer::OverBoundReturn, "digest", &twin, &batch, endian),
            Err(rt::MARSHAL.to_owned()),
            "{endian:?}: a four-element TagSeq was replied where the contract bounds it at three"
        );
        // The dynamic path's verdict for that same reply value, so the two are
        // compared rather than the static one asserted alone.
        let four = Value::List((0..4).map(|i| Value::String(format!("t{i}"))).collect());
        let mut e = Encoder::new(endian);
        assert!(
            orbweaver_dynamic::encode(&mut e, &tags, &four).is_err(),
            "the dynamic path would have sent what the skeleton refused"
        );
    }
}

/// And the same for a bounded field of a raised exception.
///
/// A `raises` body is written by the generated fault enum rather than by an
/// operation's reply path, so it is a second, separate site — and one where a
/// dropped bound would have been discovered only by whoever decoded the
/// exception.
#[test]
fn a_generated_skeleton_refuses_an_over_bound_exception_field() {
    let r = registry();
    let too_big = tc(&r, "IDL:gc27/TooBig:1.0");
    let twin = TypeCode::Struct {
        id: "IDL:gc27/keep-args:1.0".into(),
        name: "keep_args".into(),
        members: vec![
            orbweaver_giop::typecode::Member { name: "key".into(), tc: TypeCode::String(0) },
            orbweaver_giop::typecode::Member { name: "entry".into(), tc: unbounded_record() },
        ],
    };
    let args = Value::Struct(vec![
        ("key".into(), Value::String("k".into())),
        ("entry".into(), record_value("ok", 6, "가나다라")),
    ]);
    for endian in BOTH {
        assert_eq!(dispatch("keep", &twin, &args, endian), Ok(()));
        assert_eq!(
            dispatch_to(Answer::OverBoundException, "keep", &twin, &args, endian),
            Err(rt::MARSHAL.to_owned()),
            "{endian:?}: a TooBig with a seven-octet Blob was raised where the bound is six"
        );
        let value = Value::Struct(vec![
            ("reason".into(), Value::String("nope".into())),
            ("offending".into(), octets(7)),
        ]);
        let mut e = Encoder::new(endian);
        assert!(
            orbweaver_dynamic::encode(&mut e, &too_big, &value).is_err(),
            "the dynamic path would have sent what the skeleton refused"
        );
    }
}

// ── The generator's own output, read as text ─────────────────────────────────

/// Every bound in the contract reaches the emitted Rust as a type parameter.
///
/// Reading the source rather than only the behaviour is deliberate: it is the
/// half of the claim that a reader of the generated trait can check, and it is
/// what makes "the bound is in the type" falsifiable rather than a description
/// of an implementation detail.
#[test]
fn every_declared_bound_appears_in_the_emitted_type() {
    let registry = registry();
    let source = orbweaver_gen::emit(&registry, "emitted::f_27_bounds").source;
    for wanted in [
        "pub type Tag = orbweaver_gen::rt::Bounded<String, 8>;",
        "pub type WideTag = orbweaver_gen::rt::Bounded<orbweaver_gen::rt::WString, 4>;",
        "pub type Blob = orbweaver_gen::rt::Bounded<Vec<u8>, 6>;",
    ] {
        assert!(source.contains(wanted), "the emitted module is missing:\n  {wanted}");
    }
    // `TagSeq` is a bounded sequence *of* a bounded string: both numbers have
    // to survive, which a generator that carried only the outermost would fail.
    assert!(
        source.contains("pub type TagSeq = orbweaver_gen::rt::Bounded<Vec<")
            && source.contains("gc27::Tag>, 3>;"),
        "TagSeq lost a bound:\n{source}"
    );
    // And an unbounded declaration keeps its bare type: wrapping everything
    // would pass every test above and change every signature in the workspace.
    assert!(
        source.contains("pub type BlobSeq = Vec<"),
        "an unbounded sequence grew a wrapper:\n{source}"
    );
}
