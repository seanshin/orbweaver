//! Codeset negotiation, checked where it is actually spent: the octets a peer
//! reads and the component a peer reads them under.
//!
//! Both halves of §7.10.2 had the same shape of defect — a value computed
//! correctly and then consulted by nothing — so they are tested together.
//!
//! - The **client** half agreed a `char` transmission codeset, announced it in
//!   the `CodeSets` service context, and then wrote UTF-8 whatever it had
//!   agreed. That is undetectable from a round trip against ourselves, because
//!   our own reader also ignores the declaration; it is only visible by looking
//!   at the request bytes, which is what these tests do.
//! - The **server** half published no `TAG_CODE_SETS` at all, which §7.10.2.4
//!   makes a positive statement — ISO-8859-1 for `char` and *no `wchar`
//!   support* — so a conformant client refuses to marshal a `wstring` to us and
//!   does it entirely inside itself. Measured against omniORB 4.3.4: before
//!   this batch, `echo_wstring` on our own `spike-server` reference raised
//!   `INV_OBJREF` minor `0x4F4D0001` in the client with nothing on the wire and
//!   nothing in our log; after, the call arrives and Korean round-trips.
//!
//! The peer bodies pinned in [`OMNIORB_COMPONENT`] and [`JACORB_COMPONENT`] were
//! captured from those two ORBs' own published IORs. Clause (b) of the
//! licensing boundary: they were run as separate processes and their output
//! read.

use std::io::Write;
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

use orbweaver_cdr::{Decoder, Endian};
use orbweaver_giop::codeset::{
    self, CodeSetComponent, CodeSetComponentInfo, CodeSetContext, CodeSetId,
};
use orbweaver_giop::mux::Mux;
use orbweaver_giop::server::{Server, decode_request, encode_reply};
use orbweaver_giop::{
    Connection, DEFAULT_MAX_MESSAGE_SIZE, Error, IiopProfile, Ior, ReplyStatus, ServiceContext,
    TaggedComponent, Version, encode_request_with_contexts, read_message,
};

const T: Duration = Duration::from_secs(10);

// ─────────────────────────────────────────────────────────────────────────────
// What the two peers actually publish
// ─────────────────────────────────────────────────────────────────────────────

/// omniORB 4.3.4's `TAG_CODE_SETS` body, read out of its own server IOR.
///
/// `char` native ISO-8859-1 with UTF-8 in the conversion list; `wchar` native
/// UTF-16 with UTF-16 listed again as a conversion.
const OMNIORB_COMPONENT: &[u8] = &[
    0x01, 0x00, 0x00, 0x00, // little-endian encapsulation flag + pad
    0x01, 0x00, 0x01, 0x00, // char native 0x00010001 ISO-8859-1
    0x01, 0x00, 0x00, 0x00, // one conversion
    0x01, 0x00, 0x01, 0x05, // 0x05010001 UTF-8
    0x09, 0x01, 0x01, 0x00, // wchar native 0x00010109 UTF-16
    0x01, 0x00, 0x00, 0x00, // one conversion
    0x09, 0x01, 0x01, 0x00, // 0x00010109 UTF-16
];

/// JacORB 3.9's `TAG_CODE_SETS` body, read out of its own server IOR.
///
/// Big-endian encapsulation, `char` native UTF-8 with ISO-8859-1 and
/// ISO-8859-15 offered; `wchar` native UTF-16 with UTF-8 and UCS-2 offered.
/// Deliberately the other byte order from omniORB's — a component parser that
/// only ever saw little-endian bodies has not been tested.
const JACORB_COMPONENT: &[u8] = &[
    0x00, 0x00, 0x00, 0x00, // big-endian encapsulation flag + pad
    0x05, 0x01, 0x00, 0x01, // char native 0x05010001 UTF-8
    0x00, 0x00, 0x00, 0x02, // two conversions
    0x00, 0x01, 0x00, 0x01, // ISO-8859-1
    0x00, 0x01, 0x00, 0x0f, // ISO-8859-15
    0x00, 0x01, 0x01, 0x09, // wchar native UTF-16
    0x00, 0x00, 0x00, 0x02, // two conversions
    0x05, 0x01, 0x00, 0x01, // UTF-8
    0x00, 0x01, 0x01, 0x00, // UCS-2
];

/// Both peers reach UTF-8, by different routes, and neither needs us to claim a
/// conversion we do not perform.
///
/// This is what makes [`codeset::server_component_info`]'s empty conversion
/// lists usable rather than merely honest: had either peer required us to
/// convert, the narrow declaration would have cost interoperability and the
/// choice would have had to go the other way.
#[test]
fn both_peers_negotiate_utf8_against_what_we_offer() {
    for (who, body) in [("omniORB", OMNIORB_COMPONENT), ("JacORB", JACORB_COMPONENT)] {
        let info = CodeSetComponentInfo::parse(body).expect("a peer's component parses");
        let chosen = codeset::negotiate(&codeset::client_char_component(), &info.for_char)
            .unwrap_or_else(|e| panic!("{who}: {e}"));
        assert_eq!(chosen, CodeSetId::UTF_8, "{who} char");

        // The same question from the serving side: what a peer's *client* would
        // agree with the component we publish.
        let ours = codeset::server_component_info();
        let theirs_as_client = info.for_char.clone();
        let chosen = codeset::negotiate(&theirs_as_client, &ours.for_char)
            .unwrap_or_else(|e| panic!("{who} as client: {e}"));
        assert_eq!(chosen, CodeSetId::UTF_8, "{who} char, they call us");

        let wide = codeset::negotiate(&codeset::client_wchar_component(), &info.for_wchar)
            .unwrap_or_else(|e| panic!("{who} wchar: {e}"));
        assert_eq!(wide, CodeSetId::UTF_16, "{who} wchar");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The server half: publishing TAG_CODE_SETS
// ─────────────────────────────────────────────────────────────────────────────

fn code_sets_of(ior: &Ior) -> CodeSetComponentInfo {
    let p = ior.primary().expect("an IIOP profile");
    let c = p
        .components
        .iter()
        .find(|c| c.tag == codeset::TAG_CODE_SETS)
        .expect("the profile publishes TAG_CODE_SETS");
    CodeSetComponentInfo::parse(&c.data).expect("the published component parses")
}

/// Every reference this crate hands out has to carry the component, not just
/// the one that happened to be looked at.
///
/// Four publish sites, listed rather than sampled: `Server::ior` (which
/// `Server::ior_mapped` routes through), the naming service's contexts, the
/// event channel's, and a push consumer's. A servant reachable only through a
/// reference that omits this is a servant whose `wstring` operations a
/// conformant client will not call.
#[test]
fn every_published_reference_declares_its_codesets() {
    let server = Server::bind("127.0.0.1:0", b"k".to_vec()).expect("bind");
    let mut refs: Vec<(&str, Ior)> =
        vec![("Server::ior", server.ior("IDL:t/T:1.0", "127.0.0.1").expect("ior"))];

    let naming =
        orbweaver_giop::naming_server::NamingServer::new("127.0.0.1", 4001, b"NS".to_vec());
    refs.push(("NamingServer::root_ior", naming.root_ior()));

    let channel =
        orbweaver_giop::event_server::EventChannelServer::new("127.0.0.1", 4002, b"Chan".to_vec());
    refs.push(("EventChannelServer::channel_ior", channel.channel_ior()));

    let consumer = orbweaver_giop::event_server::PushConsumerServant::new(b"Cons".to_vec());
    refs.push(("PushConsumerServant::ior", consumer.ior("127.0.0.1", 4003)));

    for (which, ior) in refs {
        let info = code_sets_of(&ior);
        assert_eq!(info.for_char.native, Some(CodeSetId::UTF_8), "{which} char native");
        assert_eq!(info.for_wchar.native, Some(CodeSetId::UTF_16), "{which} wchar native");
        // Empty on purpose; see `codeset::server_component_info`.
        assert!(info.for_char.conversions.is_empty(), "{which} char conversions");
        assert!(info.for_wchar.conversions.is_empty(), "{which} wchar conversions");

        // And it must survive stringification, which is the only form a peer
        // ever sees.
        let round = Ior::parse(&ior.to_stringified().expect("stringify")).expect("reparse");
        assert_eq!(code_sets_of(&round), info, "{which} through IOR: hex");
    }
}

/// `ior_mapped` publishes a rewritten address and must not lose the component
/// on the way. It builds on `ior`, and that is exactly the kind of thing a
/// later refactor stops doing.
#[test]
fn a_nat_mapped_reference_keeps_its_codesets() {
    let server = Server::bind("127.0.0.1:0", b"k".to_vec()).expect("bind");
    let map = orbweaver_giop::nat::EndpointMap::default();
    let ior = server.ior_mapped("IDL:t/T:1.0", &map).expect("mapped ior");
    assert_eq!(code_sets_of(&ior).for_wchar.native, Some(CodeSetId::UTF_16));
}

/// A `corbaloc:` URL says nothing about the target's codesets, so materialising
/// one must not claim anything either.
///
/// The temptation is to give every `Ior` we build the component for
/// consistency. It would be a fabrication: this reference describes *somebody
/// else's* server, and §7.10.2.4 reads an absent component as that server's
/// statement about itself. Inventing one here would make a peer's client
/// marshal a `wstring` to a target that may not accept one.
#[test]
fn a_corbaloc_url_invents_no_codeset_component() {
    let url = orbweaver_giop::naming::ObjectUrl::parse("corbaloc:iiop:1.2@example.test:4001/Key")
        .expect("a corbaloc parses");
    let ior = url.to_ior("IDL:t/T:1.0").expect("it becomes an IOR");
    assert!(
        ior.primary().expect("profile").components.is_empty(),
        "a URL that carried no codeset information must not grow one"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// The server half: reading the client's declaration
// ─────────────────────────────────────────────────────────────────────────────

/// The `CodeSets` context omniORB now sends us, captured off the wire once the
/// reference it dialed carried `TAG_CODE_SETS`. Before that it sent **no
/// service contexts at all**, which §7.10.2.5 makes a declaration of
/// ISO-8859-1.
const OMNIORB_CONTEXT_TO_US: &[u8] = &[
    0x01, 0x00, 0x00, 0x00, // little-endian encapsulation flag + pad
    0x01, 0x00, 0x01, 0x05, // char  TCS 0x05010001 UTF-8
    0x09, 0x01, 0x01, 0x00, // wchar TCS 0x00010109 UTF-16
];

/// A request's service contexts reach the servant instead of being skipped.
///
/// Both byte orders and both request layouts, because 1.0/1.1 marshal the
/// context list first and 1.2 marshals it last — a reader that only ever ran
/// against one of them has tested one offset.
#[test]
fn a_requests_codeset_context_survives_decoding() {
    for version in [Version::V1_0, Version::V1_1, Version::V1_2] {
        for endian in [Endian::Big, Endian::Little] {
            let ctx = ServiceContext {
                id: codeset::SERVICE_ID_CODE_SETS,
                data: OMNIORB_CONTEXT_TO_US.to_vec(),
            };
            let other = ServiceContext { id: 0xDEAD, data: vec![1, 2, 3, 4] };
            let msg = encode_request_with_contexts(
                version,
                endian,
                7,
                b"key",
                "echo_wstring",
                true,
                &[ctx, other],
                |e| e.put_i32(1),
            )
            .expect("encodes");
            let raw = read_message(&mut msg.as_slice(), DEFAULT_MAX_MESSAGE_SIZE).expect("frames");
            let req = decode_request(raw).expect("decodes");

            assert_eq!(req.service_contexts().len(), 2, "{version} {endian:?}");
            let cs = req.code_sets().expect("the codeset context is there");
            assert_eq!(cs.char_data, CodeSetId::UTF_8, "{version} {endian:?}");
            assert_eq!(cs.wchar_data, CodeSetId::UTF_16, "{version} {endian:?}");
            // Unknown contexts are kept verbatim rather than dropped: §13.7
            // leaves the set open, and a servant that wants to act on one can
            // only do so if it is still there.
            assert_eq!(req.service_contexts()[1].id, 0xDEAD);
            // The body still starts where it started.
            assert_eq!(req.body().expect("body").get_i32().expect("arg"), 1);
        }
    }
}

/// A request with no `CodeSets` context reads as `None`, and the doc on
/// [`orbweaver_giop::server::Request::code_sets`] is what says what `None`
/// means. Pinned so the accessor cannot quietly start inventing a default.
#[test]
fn an_absent_codeset_context_is_reported_as_absent() {
    let msg = encode_request_with_contexts(
        Version::V1_2,
        Endian::Big,
        1,
        b"k",
        "ping",
        true,
        &[],
        |_| {},
    )
    .expect("encodes");
    let raw = read_message(&mut msg.as_slice(), DEFAULT_MAX_MESSAGE_SIZE).expect("frames");
    let req = decode_request(raw).expect("decodes");
    assert!(req.service_contexts().is_empty());
    assert!(req.code_sets().is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// The client half: a declaration nobody honoured
// ─────────────────────────────────────────────────────────────────────────────

fn component(info: &CodeSetComponentInfo) -> TaggedComponent {
    TaggedComponent {
        tag: codeset::TAG_CODE_SETS,
        data: info.encode(Endian::Little).expect("encodes"),
    }
}

fn profile_at(addr: std::net::SocketAddr, components: Vec<TaggedComponent>) -> IiopProfile {
    IiopProfile {
        version: Version::V1_2,
        host: addr.ip().to_string(),
        port: addr.port(),
        object_key: b"k".to_vec(),
        components,
    }
}

/// A peer whose char component is `native`, offering `conversions` and nothing
/// else.
fn char_only(native: CodeSetId, conversions: &[CodeSetId]) -> CodeSetComponentInfo {
    CodeSetComponentInfo {
        for_char: CodeSetComponent { native: Some(native), conversions: conversions.to_vec() },
        for_wchar: CodeSetComponent { native: Some(CodeSetId::UTF_16), conversions: Vec::new() },
    }
}

/// Accepts one connection and reports every byte it received before the peer
/// hung up. A refusal that happens before the write is a refusal this can see.
fn recording() -> (std::net::SocketAddr, mpsc::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().expect("accept");
        s.set_read_timeout(Some(Duration::from_millis(750))).expect("timeout");
        let mut got = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match std::io::Read::read(&mut s, &mut buf) {
                Ok(0) => break,
                Ok(n) => got.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        let _ = tx.send(got);
    });
    (addr, rx)
}

/// The defect, stated as the property it broke: what the `CodeSets` context
/// declares and what the octets are must be the same codeset.
///
/// The peer here offers ISO-8859-1 and no conversions, which §7.10.2.6 case 2
/// resolves to ISO-8859-1 — a codeset this crate's writers do not emit. Before
/// this batch the connection announced ISO-8859-1 and then wrote `put_str`'s
/// UTF-8 under it, which no peer can detect and every peer decodes wrongly.
///
/// Nothing reaches the wire now, so the assertion is on the recording as much
/// as on the error.
#[test]
fn a_codeset_nobody_encodes_to_refuses_to_send() {
    let (addr, recorded) = recording();
    let info = char_only(CodeSetId::ISO_8859_1, &[]);
    let p = profile_at(addr, vec![component(&info)]);

    let mut conn = Connection::connect_to(&p, T).expect("connect");
    // Negotiation itself is unchanged and still lands where §7.10.2.6 says.
    assert_eq!(conn.char_converter().id(), CodeSetId::ISO_8859_1);

    match conn.invoke("echo_string", |e| e.put_str("한글")) {
        Err(Error::CodesetNotApplied { negotiated }) => {
            assert_eq!(negotiated, CodeSetId::ISO_8859_1)
        }
        other => panic!("expected a refusal, got {other:?}"),
    }

    drop(conn);
    let bytes = recorded.recv_timeout(T).expect("the peer thread reports");
    assert!(bytes.is_empty(), "nothing may go out under a declaration nobody honours: {bytes:?}");
}

/// The same peer, and a caller that says it will convert. Now the declaration
/// and the octets agree, and the test reads both off the wire rather than
/// trusting the API.
#[test]
fn a_caller_that_converts_may_send_under_its_own_declaration() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().expect("accept");
        let raw = read_message(&mut s, DEFAULT_MAX_MESSAGE_SIZE).expect("a request arrives");
        let (version, endian) = (raw.version, raw.endian);
        let req = decode_request(raw).expect("decodes");
        let cs = req.code_sets();
        let arg = req.body().expect("body").get_string_bytes().expect("a string").to_vec();
        let out = encode_reply(version, endian, req.request_id, ReplyStatus::NoException, |e| {
            e.put_string_bytes(&arg)
        })
        .expect("reply encodes");
        s.write_all(&out).expect("write");
        s.flush().expect("flush");
        let _ = tx.send((cs, arg));
    });

    let info = char_only(CodeSetId::ISO_8859_1, &[]);
    let p = profile_at(addr, vec![component(&info)]);
    let mut conn = Connection::connect_to(&p, T).expect("connect");

    let cs = conn.convert_chars().expect("a converter for the agreed codeset");
    assert_eq!(cs.id(), CodeSetId::ISO_8859_1);
    let sent = cs.encode("Édouard").expect("Latin-1 can carry this");
    let reply = conn.invoke("echo_string", |e| e.put_string_bytes(&sent)).expect("sends now");
    assert_eq!(
        cs.decode(reply.body().expect("body").get_string_bytes().expect("a string"))
            .expect("decodes"),
        "Édouard"
    );

    let (declared, arg) = rx.recv_timeout(T).expect("the peer thread reports");
    let declared = declared.expect("the CodeSets context was sent");
    assert_eq!(declared.char_data, CodeSetId::ISO_8859_1, "what the context claims");
    // And the octets are Latin-1, not UTF-8. `É` is one byte in the codeset the
    // context named and two in the one `put_str` would have written.
    assert_eq!(arg, vec![0xC9, b'd', b'o', b'u', b'a', b'r', b'd'], "what the octets are");
}

/// A peer that publishes a component nothing can be agreed with is
/// `CODESET_INCOMPATIBLE` (§7.10.2.6), not a silent fall-back to §7.10.2.5's
/// no-context default.
///
/// The two used to be the same code path: any negotiation failure produced
/// `None`, `None` meant "the peer published nothing", and "the peer published
/// nothing" meant send UTF-8 with no context — under a *specified* default of
/// ISO-8859-1. So the one peer that had told us in advance it could not read
/// our octets was the peer we told nothing.
#[test]
fn a_component_we_cannot_agree_with_is_not_the_absent_component_case() {
    let (addr, recorded) = recording();
    // A registry id we implement nothing for, on both sides of the component.
    let info = char_only(CodeSetId(0x0AAA_0000), &[]);
    let p = profile_at(addr, vec![component(&info)]);

    let mut conn = Connection::connect_to(&p, T).expect("connect");
    assert!(matches!(conn.invoke_nullary("ping"), Err(Error::CodesetIncompatible(_))));
    assert!(matches!(conn.convert_chars(), Err(Error::CodesetIncompatible(_))));

    drop(conn);
    assert!(recorded.recv_timeout(T).expect("the peer thread reports").is_empty());
}

/// A profile with no component at all keeps §7.10.2.5's behaviour: no context,
/// and the call goes through. Recorded as characterisation, not as a control —
/// this is the case that was already right, and it is here so that tightening
/// the two above cannot tighten this one by accident.
#[test]
fn a_profile_without_a_component_still_calls() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().expect("accept");
        let raw = read_message(&mut s, DEFAULT_MAX_MESSAGE_SIZE).expect("a request");
        let (version, endian) = (raw.version, raw.endian);
        let req = decode_request(raw).expect("decodes");
        let n = req.service_contexts().len();
        let out = encode_reply(version, endian, req.request_id, ReplyStatus::NoException, |e| {
            e.put_i32(42)
        })
        .expect("encodes");
        s.write_all(&out).expect("write");
        s.flush().expect("flush");
        let _ = tx.send(n);
    });

    let p = profile_at(addr, Vec::new());
    let mut conn = Connection::connect_to(&p, T).expect("connect");
    let reply = conn.invoke_nullary("ping").expect("goes through");
    assert_eq!(reply.body().expect("body").get_i32().expect("a long"), 42);
    assert_eq!(rx.recv_timeout(T).expect("reports"), 0, "no context is sent when none was agreed");
}

/// A `Mux` inherits the refusal, and reports it as unsent.
///
/// It has its own copy of everything the connection negotiated, which is
/// exactly why it needs its own check: a fix applied only to `Connection` would
/// leave every pooled call — which is all of them, above the spike level — going
/// out under the declaration nobody honoured.
#[test]
fn a_mux_refuses_what_the_connection_it_took_over_would_have() {
    let (addr, recorded) = recording();
    let info = char_only(CodeSetId::ISO_8859_1, &[]);
    let p = profile_at(addr, vec![component(&info)]);

    let conn = Connection::connect_to(&p, T).expect("connect");
    let mux = Mux::over(conn);
    let failed = mux.call("echo_string", |e| e.put_str("한글"), T).expect_err("refused");
    assert!(failed.unsent, "nothing was written, so the call is re-sendable elsewhere");
    assert!(matches!(failed.error, Error::CodesetNotApplied { .. }), "{:?}", failed.error);

    // And the opt-in works through the shared handle too.
    assert_eq!(mux.convert_chars().expect("converter").id(), CodeSetId::ISO_8859_1);

    drop(mux);
    assert!(recorded.recv_timeout(T).expect("reports").is_empty());
}

/// What the `CodeSets` context claims, read back out of the bytes rather than
/// out of the API, for the ordinary UTF-8 case both real peers land on.
#[test]
fn the_utf8_case_declares_utf8_and_writes_utf8() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().expect("accept");
        let raw = read_message(&mut s, DEFAULT_MAX_MESSAGE_SIZE).expect("a request");
        let (version, endian) = (raw.version, raw.endian);
        let req = decode_request(raw).expect("decodes");
        let declared = req.code_sets();
        let arg = req.body().expect("body").get_string_bytes().expect("a string").to_vec();
        let out = encode_reply(version, endian, req.request_id, ReplyStatus::NoException, |e| {
            e.put_i32(0)
        })
        .expect("encodes");
        let _ = s.write_all(&out);
        let _ = s.flush();
        let _ = tx.send((declared, arg));
    });

    // omniORB's own component: it converts, and we do not.
    let p = profile_at(
        addr,
        vec![TaggedComponent { tag: codeset::TAG_CODE_SETS, data: OMNIORB_COMPONENT.to_vec() }],
    );
    let mut conn = Connection::connect_to(&p, T).expect("connect");
    conn.invoke("echo_string", |e| e.put_str("한글")).expect("UTF-8 needs no undertaking");

    let (declared, arg) = rx.recv_timeout(T).expect("reports");
    assert_eq!(declared.expect("a context").char_data, CodeSetId::UTF_8);
    assert_eq!(arg, "한글".as_bytes(), "the octets are the codeset that was declared");
}

/// The `CodeSets` context goes on the first request and no other (§7.10.2.5
/// negotiates per connection; a second, conflicting context on one connection
/// is `MARSHAL` minor 9). Characterisation of behaviour that was already right,
/// re-asserted here because this batch moved the code that decides it.
#[test]
fn the_context_goes_out_once() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().expect("accept");
        let mut carried = Vec::new();
        for _ in 0..3 {
            let raw = read_message(&mut s, DEFAULT_MAX_MESSAGE_SIZE).expect("a request");
            let (version, endian) = (raw.version, raw.endian);
            let req = decode_request(raw).expect("decodes");
            carried.push(req.code_sets().is_some());
            let out =
                encode_reply(version, endian, req.request_id, ReplyStatus::NoException, |e| {
                    e.put_i32(0)
                })
                .expect("encodes");
            s.write_all(&out).expect("write");
            s.flush().expect("flush");
        }
        let _ = tx.send(carried);
    });

    let p = profile_at(
        addr,
        vec![TaggedComponent { tag: codeset::TAG_CODE_SETS, data: OMNIORB_COMPONENT.to_vec() }],
    );
    let mut conn = Connection::connect_to(&p, T).expect("connect");
    for _ in 0..3 {
        conn.invoke_nullary("ping").expect("calls");
    }
    assert_eq!(rx.recv_timeout(T).expect("reports"), vec![true, false, false]);
}

/// The component we publish is the one we can parse, byte for byte.
///
/// A publisher and a parser written apart drift; here they are the same two
/// functions a peer sits between.
#[test]
fn what_we_publish_is_what_we_read() {
    let c = codeset::server_component();
    assert_eq!(c.tag, codeset::TAG_CODE_SETS);
    assert_eq!(
        CodeSetComponentInfo::parse(&c.data).expect("parses"),
        codeset::server_component_info()
    );

    // And the encapsulation says which byte order it is in, so a peer reading
    // it does not need ours.
    assert_eq!(Decoder::encapsulation(&c.data).expect("encapsulation").endian(), Endian::Little);

    // Both orders round-trip, because a peer may hand us either.
    for endian in [Endian::Big, Endian::Little] {
        let raw = codeset::server_component_info().encode(endian).expect("encodes");
        assert_eq!(
            CodeSetComponentInfo::parse(&raw).expect("parses"),
            codeset::server_component_info(),
            "{endian:?}"
        );
    }
}

/// A `CodeSetContext` is what the client sends; a `CodeSetComponentInfo` is
/// what the server publishes. They are different shapes and the compiler will
/// not stop you confusing them, so this pins the one distinguishing fact: the
/// context has no counts in it.
#[test]
fn a_context_is_not_a_component() {
    let ctx = CodeSetContext { char_data: CodeSetId::UTF_8, wchar_data: CodeSetId::UTF_16 };
    let raw = ctx.encode(Endian::Little).expect("encodes");
    assert_eq!(raw.len(), 12, "flag, pad, and two ids");
    let comp = codeset::server_component_info().encode(Endian::Little).expect("encodes");
    assert_eq!(comp.len(), 20, "flag, pad, and two (native, count) pairs");
}
