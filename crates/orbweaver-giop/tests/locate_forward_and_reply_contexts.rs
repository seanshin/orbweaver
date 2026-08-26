//! Two things §9.4.6 and §9.4.3.1 require that this ORB could not put on the
//! wire, measured from both ends and in both byte orders.
//!
//! **`OBJECT_FORWARD` on a `LocateReply`.** `LocateStatus::ObjectForward` was a
//! name with no body behind it: the encoder wrote a request id and a status
//! word and stopped, so the one status whose entire purpose is to carry an IOR
//! could be *named* and could not be *served*. Worse than absent — the serve
//! loop asked `knows()`, a boolean, so a servant that had moved an object had
//! no way to say so and the client was told `UNKNOWN_OBJECT`: not "elsewhere"
//! but "nowhere". Measured on this workspace before the change, with a servant
//! whose object had moved: `Connection::locate()` answered `Ok(Unknown)`.
//! A caller that probes before spending a request got a wrong answer for its
//! trouble.
//!
//! **A `ServiceContextList` on a `Reply`.** The reply encoder wrote a hard `0`
//! where §9.4.3.1 puts a list, and the reply decoder walked the peer's list
//! only to move its cursor past it. So this ORB could neither send one nor
//! observe one, in the one direction where it is the sender. §9.7.2's rule is
//! *ignored, but preserved*, which this codebase already applies to a
//! `TaggedComponent` in an IOR; the reply header is the same rule and the same
//! `IOP` list, and it was the copy that lost the data.
//!
//! **What is deliberately not here.** Nothing attaches a context to an outgoing
//! reply, and there is no hook, chain or registry for doing so. *Who may put
//! something in that list* is Portable Interceptors, which is `PLAN-DEFERRED`
//! §21 with its own reason and its own trigger. These tests measure that the
//! wire can carry the shape and that an inbound one survives — not a policy for
//! filling it.
//!
//! **What was written down rather than closed, and is now closed.**
//! `Dispatch::serve_one` used to ask `knows` before `redirect`, so a moved
//! object was still `OBJECT_NOT_EXIST` on the *request* path unless its servant
//! kept answering `knows` — a lie it had to tell to be forwarded at all. The
//! characterisation test that pinned it, `a_moved_object_is_still_refused_on_
//! the_request_path`, went red as designed when the order was changed later the
//! same day and is now `a_moved_object_is_forwarded_on_the_request_path_too`,
//! asserting the opposite. The argument for the order lives in
//! `orbweaver_giop::server::serve_one_ordering`, and what a name-keyed redirect
//! still cannot do is `tests/forward_for_a_name.rs`.
//!
//! Every value assertion here compares **decoded values**. The one place a
//! buffer is compared byte for byte is `an_empty_context_list_is_the_zero_that_
//! was_there`, and it is comparing this encoder against a hand-built copy of
//! *its own previous output*, which is the property under test — not against a
//! foreign ORB, whose padding content the specification leaves undefined.

use orbweaver_cdr::{Decoder, Encoder, Endian};
use orbweaver_giop::orb::Orb;
use orbweaver_giop::server::{
    Dispatch, LocateStatus, Request, SystemException, encode_locate_reply, encode_reply,
    encode_reply_with_contexts, reply_body_start,
};
use orbweaver_giop::{
    Connection, DEFAULT_MAX_MESSAGE_SIZE, Forward, IiopProfile, Ior, LocateResult, ReplyStatus,
    ServiceContext, Version, decode_locate_reply, decode_reply, read_message,
};
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Every wait answers to this. A test that can hang is not a test.
const T: Duration = Duration::from_secs(10);

/// The GIOP message header: the alignment origin, and 12 bytes long.
const HEADER_LEN: usize = 12;

const EVERY_VERSION: [Version; 3] = [Version::V1_0, Version::V1_1, Version::V1_2];
const BOTH_ORDERS: [Endian; 2] = [Endian::Big, Endian::Little];

fn ior_at(host: &str, port: u16, key: &[u8]) -> Ior {
    Ior {
        type_id: "IDL:test/Moved:1.0".into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: host.into(),
            port,
            object_key: key.to_vec(),
            components: Vec::new(),
        }],
    }
}

/// Dials `addr` in `endian`, with a read deadline so a wedged peer fails the
/// test instead of hanging it.
///
/// The byte order is set on the connection rather than negotiated: GIOP lets
/// each side encode in its own, so both must be exercised from the sending end.
fn dial(addr: SocketAddr, key: &[u8], endian: Endian) -> Connection {
    let profile = IiopProfile {
        version: Version::V1_2,
        host: addr.ip().to_string(),
        port: addr.port(),
        object_key: key.to_vec(),
        components: Vec::new(),
    };
    let mut conn = Connection::connect_to(&profile, T).expect("connects");
    conn.set_endian(endian);
    conn
}

// ─────────────────────────────────────────────────────────────────────────────
// Gap 1 — a LocateReply that can carry where the object went
// ─────────────────────────────────────────────────────────────────────────────

/// The round trip that could not previously be written down: encode an
/// `OBJECT_FORWARD`, decode it, get the reference back.
///
/// Both byte orders and all three versions, because an encoder that only works
/// native-endian passes every local test and fails in the field.
#[test]
fn an_object_forward_locate_reply_carries_the_reference_in_both_orders() {
    let to = ior_at("far.example", 9009, b"moved-key");
    for version in EVERY_VERSION {
        for endian in BOTH_ORDERS {
            let status = LocateStatus::ObjectForward(Forward::Temporary(to.clone()));
            let wire = encode_locate_reply(version, endian, 41, &status).expect("encodes");
            let raw = read_message(&mut wire.as_slice(), DEFAULT_MAX_MESSAGE_SIZE).expect("frames");
            let (id, got) = decode_locate_reply(raw).expect("decodes");

            assert_eq!(id, 41, "{version} {endian:?}");
            // Decoded values, never the buffer.
            assert_eq!(
                got,
                LocateResult::Forward(Box::new(Forward::Temporary(to.clone()))),
                "{version} {endian:?}: the reference must survive the round trip"
            );
        }
    }
}

/// A permanent move is a permanent move, and below GIOP 1.2 it is told as a
/// plain `OBJECT_FORWARD` rather than not told at all.
///
/// `LocateStatusType` gained `OBJECT_FORWARD_PERM` (3) in 1.2; the 1.0/1.1
/// enumeration stops at 2. Downgrading loses the permission to forget the old
/// reference and keeps the true part — the object is over there. The same
/// argument, and the same version test, as `Forward::reply_status`.
#[test]
fn a_permanent_locate_forward_downgrades_below_1_2_rather_than_vanishing() {
    let to = ior_at("far.example", 9009, b"moved-key");
    let status = LocateStatus::ObjectForward(Forward::Permanent(to.clone()));

    assert_eq!(status.code(Version::V1_0), 2, "1.0 LocateStatusType stops at OBJECT_FORWARD");
    assert_eq!(status.code(Version::V1_1), 2, "1.1 likewise");
    assert_eq!(status.code(Version::V1_2), 3, "1.2 has OBJECT_FORWARD_PERM");

    for endian in BOTH_ORDERS {
        // 1.2 keeps the permanence.
        let wire = encode_locate_reply(Version::V1_2, endian, 7, &status).expect("encodes");
        let raw = read_message(&mut wire.as_slice(), DEFAULT_MAX_MESSAGE_SIZE).expect("frames");
        let (_, got) = decode_locate_reply(raw).expect("decodes");
        assert_eq!(
            got,
            LocateResult::Forward(Box::new(Forward::Permanent(to.clone()))),
            "{endian:?}: 1.2 carries the permanence"
        );

        // 1.1 keeps the address and loses only the permission to forget.
        let wire = encode_locate_reply(Version::V1_1, endian, 7, &status).expect("encodes");
        let raw = read_message(&mut wire.as_slice(), DEFAULT_MAX_MESSAGE_SIZE).expect("frames");
        let (_, got) = decode_locate_reply(raw).expect("decodes");
        assert_eq!(
            got,
            LocateResult::Forward(Box::new(Forward::Temporary(to.clone()))),
            "{endian:?}: 1.1 still learns where to go"
        );
    }
}

/// **Alignment origin.** §9.4.6's asymmetry: a `LocateReply` body follows the
/// header with no 8-byte alignment even in GIOP 1.2, unlike a `Reply`. So the
/// IOR starts at offset 20 — 12 header + 4 request id + 4 status — and a single
/// pad run inserted there shifts every byte of the reference.
///
/// Asserted two ways that fail independently: the body decodes when read from
/// exactly 20, and the framed message length is exactly 8 plus the IOR, leaving
/// no room for padding to hide in.
#[test]
fn the_forward_body_starts_at_offset_20_with_no_padding() {
    let to = ior_at("far.example", 9009, b"moved-key");
    for version in EVERY_VERSION {
        for endian in BOTH_ORDERS {
            let status = LocateStatus::ObjectForward(Forward::Temporary(to.clone()));
            let wire = encode_locate_reply(version, endian, 41, &status).expect("encodes");

            // Read the IOR straight out of offset 20, keeping the message's own
            // alignment origin — which is what a decoder that got this right
            // does, and what one that borrowed the `Reply` rule cannot.
            let mut d = Decoder::new(&wire, endian);
            d.seek_to(HEADER_LEN + 8).expect("offset 20 is inside the message");
            let read_back = Ior::read_from(&mut d).expect("the IOR is right there");
            assert_eq!(read_back, to, "{version} {endian:?}");

            // Nothing follows it, so no padding was inserted before it either.
            let mut alone = Encoder::new(endian);
            to.write_to(&mut alone).expect("the IOR encodes on its own");
            let ior_len = alone.finish().expect("finishes").len();
            assert_eq!(
                wire.len(),
                HEADER_LEN + 8 + ior_len,
                "{version} {endian:?}: header + id + status + IOR, and nothing between"
            );
        }
    }
}

/// A body-less status stays body-less, in every version and order.
///
/// The control for the test above: if `encode_locate_reply` had started
/// emitting a body unconditionally, the round trip would still pass and only
/// the length would say so.
#[test]
fn here_and_unknown_carry_no_body() {
    for version in EVERY_VERSION {
        for endian in BOTH_ORDERS {
            for (status, want) in [
                (LocateStatus::ObjectHere, LocateResult::Here),
                (LocateStatus::UnknownObject, LocateResult::Unknown),
            ] {
                let wire = encode_locate_reply(version, endian, 3, &status).expect("encodes");
                assert_eq!(
                    wire.len(),
                    HEADER_LEN + 8,
                    "{version} {endian:?}: no body may follow this status"
                );
                let raw =
                    read_message(&mut wire.as_slice(), DEFAULT_MAX_MESSAGE_SIZE).expect("frames");
                let (_, got) = decode_locate_reply(raw).expect("decodes");
                assert_eq!(got, want, "{version} {endian:?}");
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Gap 1, end to end — a client that probes a moved object over a real socket
// ─────────────────────────────────────────────────────────────────────────────

/// A servant whose object has moved: it does not answer to the key any more,
/// and it knows where the key went.
struct Moved {
    to: Ior,
}

impl Dispatch for Moved {
    fn dispatch(&mut self, _request: &Request, _out: &mut Encoder) -> Result<(), SystemException> {
        // Unreachable: `redirect` answers `Some` for every request, so
        // `serve_one` forwards before it gets here. Kept honest rather than
        // `unreachable!()` — this arm reaching the wire is precisely the
        // regression `a_moved_object_is_forwarded_on_the_request_path_too`
        // exists to catch, and it should fail as a wrong answer rather than as
        // a panic in a serving thread.
        Err(SystemException::object_not_exist())
    }

    fn knows(&self, _object_key: &[u8]) -> bool {
        // The truth: this servant hosts nothing. Before the 2026-08-26 reorder
        // a moving servant had to lie here — answer `true` for a key it no
        // longer serves — to get its own `redirect` consulted at all.
        false
    }

    fn redirect(&mut self, _request: &Request) -> Option<Forward> {
        Some(Forward::Permanent(self.to.clone()))
    }

    fn locate(&self, _object_key: &[u8]) -> LocateStatus {
        LocateStatus::ObjectForward(Forward::Permanent(self.to.clone()))
    }
}

/// A servant that overrides `locate` with nothing, to pin that the default
/// answer did not move underneath every existing servant in the workspace.
///
/// It *does* override `knows`, and must: `Dispatch::knows` defaults to `true`
/// for every key, so a servant that overrode neither would answer `ObjectHere`
/// to any key at all and could not distinguish a working default from a
/// `locate` that had stopped consulting `knows`. Discovered by this test's own
/// negative half going red on the first run.
struct Stationary;

impl Dispatch for Stationary {
    fn dispatch(&mut self, _request: &Request, _out: &mut Encoder) -> Result<(), SystemException> {
        Ok(())
    }

    fn knows(&self, object_key: &[u8]) -> bool {
        object_key == b"probe-key"
    }
}

/// Runs `servant` on a loopback port until the returned flag is set.
fn serving<D: Dispatch + Send + 'static>(
    servant: D,
) -> (SocketAddr, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
    let server =
        Orb::new().server("127.0.0.1:0", b"probe-key".to_vec()).expect("binds a loopback port");
    let addr = server.local_addr().expect("has an address");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    let mut servant = servant;
    let thread = std::thread::spawn(move || {
        let _ = server.serve(&mut servant, move || flag.load(Ordering::SeqCst));
    });
    (addr, stop, thread)
}

fn shut_down(stop: &Arc<AtomicBool>, addr: SocketAddr, thread: std::thread::JoinHandle<()>) {
    stop.store(true, Ordering::SeqCst);
    // The accept loop checks the flag between connections, so give it one.
    let _ = TcpStream::connect(addr);
    let _ = thread.join();
}

/// **The location-transparency claim, over a socket.** A client holding a
/// reference to an object that has moved asks `LocateRequest` — the probe that
/// exists precisely so the caller does not have to spend an invocation — and is
/// told where the object went.
///
/// Before, this same exchange answered `UNKNOWN_OBJECT`: the caller was told
/// its reference named nothing, and the only way to discover otherwise was to
/// send the request anyway and read the `LOCATION_FORWARD`.
#[test]
fn a_client_probing_a_moved_object_is_told_where_it_went() {
    let destination = ior_at("elsewhere.example", 7007, b"new-key");
    let (addr, stop, thread) = serving(Moved { to: destination.clone() });

    for endian in BOTH_ORDERS {
        let mut conn = dial(addr, b"probe-key", endian);

        match conn.locate().expect("the probe is answered") {
            LocateResult::Forward(to) => {
                assert_eq!(
                    *to,
                    Forward::Permanent(destination.clone()),
                    "{endian:?}: the probe must carry the new reference, and that it is for good"
                );
            }
            other => panic!("{endian:?}: expected a forward, got {other:?}"),
        }
    }

    shut_down(&stop, addr, thread);
}

/// **The second half of the same root cause, closed 2026-08-26.** This test was
/// `a_moved_object_is_still_refused_on_the_request_path` and asserted
/// `OBJECT_NOT_EXIST`; it was a characterisation test whose own panic message
/// said that a forward here *is the fix*. This is that fix, so it now asserts
/// the opposite, and it is no longer a characterisation of anything.
///
/// The servant is unchanged — the identical `Moved`, whose `knows` is `false`
/// and whose `redirect` was already offering a `Forward` nobody asked for. What
/// changed is [`serve_one_ordering`]: `redirect` is asked first. A caller that
/// probes and a caller that simply invokes are now told the same thing.
///
/// **The destination is a live second server on purpose.** Asserting only that
/// a `LOCATION_FORWARD` came back would measure the message and not the
/// property; what location transparency claims is that the caller *gets its
/// answer*, having said nothing about where. So the test requires the reply,
/// and then requires that the connection ended up somewhere other than where it
/// dialled — a forward the client did not follow would satisfy the first half
/// alone.
#[test]
fn a_moved_object_is_forwarded_on_the_request_path_too() {
    let (dest_addr, dest_stop, dest_thread) = serving(Stationary);
    let destination = ior_at("127.0.0.1", dest_addr.port(), b"probe-key");
    let (addr, stop, thread) = serving(Moved { to: destination.clone() });

    for endian in BOTH_ORDERS {
        let mut conn = dial(addr, b"probe-key", endian);
        let reply = conn.invoke("ping", |_: &mut Encoder| {}).unwrap_or_else(|e| {
            panic!(
                "{endian:?}: the caller must be served through the forward, not told \
                 the object does not exist. Got {e:?}"
            )
        });
        assert_eq!(
            reply.status,
            ReplyStatus::NoException,
            "{endian:?}: the reply must come from the destination servant"
        );
        assert_eq!(
            conn.forwarded(),
            Some(&Forward::Permanent(destination.clone())),
            "{endian:?}: the client must have followed a permanent forward to get there"
        );
    }

    shut_down(&stop, addr, thread);
    shut_down(&dest_stop, dest_addr, dest_thread);
}

/// The negative control for the hook: a servant that does not override
/// `locate` answers exactly what this server answered before the method
/// existed. If the default had been changed to forward, or to consult
/// `redirect`, this goes red.
#[test]
fn a_servant_that_overrides_nothing_still_answers_object_here() {
    let (addr, stop, thread) = serving(Stationary);

    for endian in BOTH_ORDERS {
        let mut conn = dial(addr, b"probe-key", endian);
        assert_eq!(
            conn.locate().expect("answered"),
            LocateResult::Here,
            "{endian:?}: the default answer must not have moved"
        );
        // And the key it does *not* know is still refused, not forwarded.
        assert_eq!(
            conn.locate_key(b"no-such-key").expect("answered"),
            LocateResult::Unknown,
            "{endian:?}: an unknown key must still be UNKNOWN_OBJECT"
        );
    }

    shut_down(&stop, addr, thread);
}

// ─────────────────────────────────────────────────────────────────────────────
// Gap 2 — a Reply that can carry a ServiceContextList, and one that survives
// ─────────────────────────────────────────────────────────────────────────────

/// Hand-builds the reply this encoder used to produce: the 12-byte header, then
/// the version-conditional trio with a **hard-written zero** where the context
/// list goes.
///
/// This is the old code, retyped, so that "byte-identical" is a measurement
/// rather than a claim.
fn legacy_reply(version: Version, endian: Endian, request_id: u32, status: u32) -> Vec<u8> {
    let mut e = Encoder::new(endian);
    e.put_bytes(b"GIOP");
    e.put_u8(version.major);
    e.put_u8(version.minor);
    if version.minor == 0 {
        e.put_bool(endian == Endian::Little);
    } else {
        e.put_u8(endian.as_flag());
    }
    e.put_u8(1); // MsgType::Reply
    let size_at = e.len();
    e.put_bytes(&[0, 0, 0, 0]);
    if version.is_1_2_layout() {
        e.put_u32(request_id);
        e.put_u32(status);
        e.put_u32(0); // empty ServiceContextList
    } else {
        e.put_u32(0);
        e.put_u32(request_id);
        e.put_u32(status);
    }
    let size = (e.len() - HEADER_LEN) as u32;
    e.patch_u32(size_at, size);
    e.finish().expect("finishes")
}

/// **Do not change what a `Reply` means for existing callers.** The empty
/// context list is now *written* rather than hard-coded, and the bytes must be
/// the same bytes — asserted, not assumed, per the batch's own rule.
///
/// A buffer comparison is correct here and nowhere else in this file: both
/// sides are this codebase, the region compared contains no padding, and the
/// property under test *is* the byte identity.
#[test]
fn an_empty_context_list_is_the_zero_that_was_there() {
    for version in EVERY_VERSION {
        for endian in BOTH_ORDERS {
            let now = encode_reply(version, endian, 12, ReplyStatus::NoException, None, |_| {})
                .expect("encodes");
            assert_eq!(
                now,
                legacy_reply(version, endian, 12, 0),
                "{version} {endian:?}: writing the empty list must be the zero that was there"
            );
        }
    }
}

/// The list travels, in every version's layout and both byte orders.
///
/// 1.0 and 1.1 marshal `service_context, request_id, reply_status`; 1.2 marshals
/// `request_id, reply_status, service_context`. Putting the list in the wrong
/// place does not produce an error at the peer, it produces a misparse — which
/// is why this varies the version rather than trusting one.
#[test]
fn a_reply_carries_its_service_contexts_in_every_layout_and_order() {
    let contexts = vec![
        ServiceContext { id: 1, data: vec![0xde, 0xad, 0xbe, 0xef] },
        // A context id nothing in this ORB understands, with an odd length so
        // the entry after it depends on the alignment being right. §9.7.2 is
        // about exactly this one: unknown, and kept anyway.
        ServiceContext { id: 0x4f42_5745, data: vec![1, 2, 3, 4, 5, 6, 7] },
        ServiceContext { id: 42, data: Vec::new() },
    ];

    for version in EVERY_VERSION {
        for endian in BOTH_ORDERS {
            let wire = encode_reply_with_contexts(
                version,
                endian,
                77,
                ReplyStatus::NoException,
                &contexts,
                None,
                |e: &mut Encoder| e.put_i32(-4242),
            )
            .expect("encodes");

            let raw = read_message(&mut wire.as_slice(), DEFAULT_MAX_MESSAGE_SIZE).expect("frames");
            let reply = decode_reply(raw).expect("decodes");

            assert_eq!(reply.request_id, 77, "{version} {endian:?}");
            assert_eq!(
                reply.service_contexts, contexts,
                "{version} {endian:?}: every context, in order, byte for byte"
            );
            // The body still lands where it belongs: a context list of the
            // wrong length shifts it, and reading the value back is what
            // catches that.
            assert_eq!(
                reply.body().expect("body").get_i32().expect("an i32"),
                -4242,
                "{version} {endian:?}: contexts must not have moved the body"
            );
        }
    }
}

/// **The offset had two homes, and one of them was a literal.**
///
/// A servant composes its reply body in a detached buffer, so `handle_request`
/// has to tell that buffer where in the finished message it will land — CDR
/// aligns from the first byte of the 12-byte header, not from the buffer's own
/// start. That offset was written twice: computed inside the reply encoder from
/// its own bytes, and retyped in the dispatch path as `HEADER_LEN + 12` with a
/// comment saying it was right only while the context list stayed empty.
///
/// `reply_body_start` is now the single home, and this pins it against the
/// encoder for a list that is *not* empty — the case where the retyped literal
/// would have been wrong, and where nothing in the workspace would have gone
/// red. The check is a decoded value, not a length: the body is read back
/// through a raw decoder seeked to exactly the offset the function reports, so
/// a wrong offset misreads rather than merely mismeasures.
#[test]
fn reply_body_start_agrees_with_the_encoder() {
    let cases: [Vec<ServiceContext>; 3] = [
        Vec::new(),
        vec![ServiceContext { id: 7, data: vec![1, 2, 3] }],
        vec![
            ServiceContext { id: 0x4f42_5745, data: vec![1, 2, 3, 4, 5] },
            ServiceContext { id: 9, data: b"a longer opaque body".to_vec() },
        ],
    ];

    for contexts in &cases {
        for version in EVERY_VERSION {
            for endian in BOTH_ORDERS {
                let wire = encode_reply_with_contexts(
                    version,
                    endian,
                    5,
                    ReplyStatus::NoException,
                    contexts,
                    None,
                    |e: &mut Encoder| e.put_i64(0x0102_0304_0506_0708),
                )
                .expect("encodes");

                let at = reply_body_start(version, endian, contexts);
                let mut d = Decoder::new(&wire, endian);
                d.seek_to(at).expect("the reported offset is inside the message");
                assert_eq!(
                    d.get_i64().expect("an i64 starts exactly there"),
                    0x0102_0304_0506_0708,
                    "{version} {endian:?} with {} contexts: reply_body_start must be the \
                     offset the encoder used",
                    contexts.len()
                );
            }
        }
    }
}

/// A reply with no contexts decodes to no contexts, rather than to a phantom.
#[test]
fn a_reply_without_contexts_decodes_to_an_empty_list() {
    for version in EVERY_VERSION {
        for endian in BOTH_ORDERS {
            let wire = encode_reply(version, endian, 5, ReplyStatus::NoException, None, |_| {})
                .expect("encodes");
            let raw = read_message(&mut wire.as_slice(), DEFAULT_MAX_MESSAGE_SIZE).expect("frames");
            let reply = decode_reply(raw).expect("decodes");
            assert!(reply.service_contexts.is_empty(), "{version} {endian:?}");
        }
    }
}

/// **The preservation, over a socket.** A peer attaches contexts to a reply;
/// the client that reads that reply can still see them.
///
/// This is the test the negative control drops a context out of. It is the
/// whole of gap 2's client half: before, `decode_reply` walked the list to move
/// its cursor and kept nothing, so no test in this workspace could have gone
/// red about a peer's contexts — there was nowhere for them to be observed.
#[test]
fn contexts_a_peer_attached_survive_the_trip_through_the_client() {
    let sent = vec![
        ServiceContext { id: 0x0000_0001, data: vec![9, 9, 9] },
        ServiceContext { id: 0x1234_5678, data: b"opaque".to_vec() },
    ];

    for endian in BOTH_ORDERS {
        let listener = TcpListener::bind("127.0.0.1:0").expect("binds");
        let addr = listener.local_addr().expect("has an address");
        let script = sent.clone();
        let peer = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().expect("accepts");
            s.set_read_timeout(Some(T)).expect("deadline");
            let msg = read_message(&mut s, DEFAULT_MAX_MESSAGE_SIZE).expect("a request arrives");
            let req = orbweaver_giop::server::decode_request(msg).expect("decodes");
            let reply = encode_reply_with_contexts(
                req.version,
                endian,
                req.request_id,
                ReplyStatus::NoException,
                &script,
                None,
                |e: &mut Encoder| e.put_i32(1),
            )
            .expect("encodes");
            s.write_all(&reply).expect("reply goes out");
            s.flush().expect("flush");
        });

        let mut conn = dial(addr, b"probe-key", endian);
        let reply = conn.invoke("ping", |_: &mut Encoder| {}).expect("a reply comes back");
        assert_eq!(
            reply.service_contexts, sent,
            "{endian:?}: the peer's contexts must reach the caller, unread and unchanged"
        );
        // And the body is still readable behind them, which is the half a
        // misplaced list breaks silently.
        assert_eq!(
            reply.body().expect("body").get_i32().expect("an i32"),
            1,
            "{endian:?}: the contexts must not have shifted the body"
        );

        peer.join().expect("the peer finished its script");
    }
}
