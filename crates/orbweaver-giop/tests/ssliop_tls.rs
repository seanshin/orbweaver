//! The `ssliop` TLS transport against an in-process rustls peer.
//!
//! What these tests do and do not claim: they prove the TLS layer works —
//! establishment, certificate verification (on and effective), GIOP bytes
//! crossing the encrypted transport unchanged, clean refusal of a non-TLS
//! peer — using the self-originated fixtures in `spikes/tls/`. They do **not**
//! prove interop with an SSLIOP-speaking ORB; omniORB's sslTP fixture is a
//! future batch, and until it exists that claim is unmeasured and unmade.
#![cfg(feature = "ssliop")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use orbweaver_cdr::Endian;
use orbweaver_giop::csiv2::options;
use orbweaver_giop::ssliop::{SslComponent, TAG_SSL_SEC_TRANS};
use orbweaver_giop::{
    Connection, Error, IiopProfile, Ior, TaggedComponent, Version, encode_locate_request,
};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spikes/tls").join(name)
}

/// Server side of the tests: the fixture certificate and key.
fn server_config() -> Arc<rustls::ServerConfig> {
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(fixture("server.pem"))
        .expect("read spikes/tls/server.pem")
        .collect::<Result<_, _>>()
        .expect("parse spikes/tls/server.pem");
    let key = PrivateKeyDer::from_pem_file(fixture("server.key")).expect("spikes/tls/server.key");
    Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .expect("fixture cert and key must match"),
    )
}

/// Client side: trust exactly the named CA file, nothing else.
fn client_config(ca: &str) -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in CertificateDer::pem_file_iter(fixture(ca)).expect("read CA fixture") {
        roots.add(cert.expect("parse CA fixture")).expect("add CA to root store");
    }
    Arc::new(rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth())
}

/// An IOR whose single profile is SSL-only: cleartext port 0 (the
/// better-attested deployed convention) and a `TAG_SSL_SEC_TRANS` component
/// carrying the real TLS port.
fn tls_only_ior(host: &str, tls_port: u16) -> Ior {
    let ssl = SslComponent {
        target_supports: options::INTEGRITY | options::CONFIDENTIALITY,
        target_requires: options::INTEGRITY | options::CONFIDENTIALITY,
        port: tls_port,
    };
    Ior {
        type_id: "IDL:Echo:1.0".into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: host.into(),
            port: 0,
            object_key: b"key".to_vec(),
            components: vec![TaggedComponent {
                tag: TAG_SSL_SEC_TRANS,
                data: ssl.encode(Endian::Big).expect("encode SSL component"),
            }],
        }],
    }
}

/// Accepts one TLS connection, reads exactly one GIOP message, and hands the
/// decrypted bytes back over the channel. Then closes cleanly (close_notify),
/// so the client's wait for a reply fails fast instead of timing out.
fn one_shot_tls_server(listener: TcpListener, tx: mpsc::Sender<Vec<u8>>) -> thread::JoinHandle<()> {
    let config = server_config();
    thread::spawn(move || {
        let (tcp, _) = listener.accept().expect("accept");
        tcp.set_read_timeout(Some(Duration::from_secs(10))).expect("server read timeout");
        let conn = rustls::ServerConnection::new(config).expect("server connection");
        let mut tls = rustls::StreamOwned::new(conn, tcp);
        let mut header = [0u8; 12];
        tls.read_exact(&mut header).expect("GIOP header through TLS");
        let len = [header[8], header[9], header[10], header[11]];
        let size =
            if header[6] & 1 == 0 { u32::from_be_bytes(len) } else { u32::from_le_bytes(len) };
        let mut message = header.to_vec();
        message.resize(12 + size as usize, 0);
        tls.read_exact(&mut message[12..]).expect("GIOP body through TLS");
        tx.send(message).expect("hand bytes to the test");
        tls.conn.send_close_notify();
        let _ = tls.flush();
    })
}

/// The core claim: `connect_tls` establishes against the fixture server, and
/// a GIOP LocateRequest arrives byte-identical after TLS decryption — the
/// transport neither reframes nor rewrites what the GIOP layer produced.
#[test]
fn a_locate_request_crosses_tls_byte_identical() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("local addr").port();
    let (tx, rx) = mpsc::channel();
    let server = one_shot_tls_server(listener, tx);

    let ior = tls_only_ior("localhost", port);
    let mut conn = Connection::connect_tls(&ior, Duration::from_secs(5), client_config("ca.pem"))
        .expect("TLS connect to the fixture server");

    // The server captures one message and closes without replying, so the
    // locate itself errs on the missing reply — deliberately. The assertion
    // is about the bytes that crossed the wire, not about a reply no GIOP
    // peer exists to send.
    let version = conn.version();
    let endian = conn.endian();
    let _ = conn.locate();

    let seen = rx.recv_timeout(Duration::from_secs(5)).expect("decrypted bytes from the server");
    let expected =
        encode_locate_request(version, endian, 1, b"key").expect("reference LocateRequest");
    assert_eq!(
        seen, expected,
        "bytes after TLS decryption must equal the encoded LocateRequest exactly"
    );
    server.join().expect("server thread");
}

/// Verification is on and effective: a client trusting only a CA that signed
/// nothing must refuse the fixture server's certificate.
#[test]
fn a_server_signed_by_an_untrusted_ca_is_refused() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("local addr").port();
    let config = server_config();
    let server = thread::spawn(move || {
        let (tcp, _) = listener.accept().expect("accept");
        let conn = rustls::ServerConnection::new(config).expect("server connection");
        let mut tls = rustls::StreamOwned::new(conn, tcp);
        // The handshake fails from the client's alert; any outcome but a
        // panic is fine here — the assertion lives on the client side.
        let _ = tls.read(&mut [0u8; 1]);
    });

    let ior = tls_only_ior("localhost", port);
    let err = Connection::connect_tls(&ior, Duration::from_secs(5), client_config("wrong-ca.pem"))
        .expect_err("a certificate from an untrusted CA must be refused");
    let text = err.to_string();
    assert!(
        text.contains("certificate") || text.contains("Certificate"),
        "the refusal must be attributable to certificate verification, got: {text}"
    );
    server.join().expect("server thread");
}

/// A peer that accepted TCP but never speaks TLS must produce a clean error
/// within the caller's timeout — not a hang, and not a success.
#[test]
fn a_plain_tcp_listener_fails_the_connect_cleanly() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("local addr").port();
    // No accept, no bytes: the ClientHello lands in the backlog and nothing
    // ever answers it.
    let ior = tls_only_ior("localhost", port);
    let started = Instant::now();
    let err = Connection::connect_tls(&ior, Duration::from_millis(500), client_config("ca.pem"))
        .expect_err("a peer that never answers the ClientHello must fail the connect");
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(5), "failed, but only after {elapsed:?}");
    assert!(
        matches!(err, Error::AllEndpointsFailed { tried: 1, .. }),
        "expected the endpoint to be counted as tried and failed, got: {err}"
    );
    drop(listener);
}

/// An IOR that advertises no TLS endpoint is refused outright — never dialed
/// in cleartext. The dual of `connect` never upgrading.
#[test]
fn an_ior_without_ssliop_is_refused_not_downgraded() {
    // A live cleartext listener at the profile address, to prove the refusal
    // is by policy and not by accident of nothing listening.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("local addr").port();
    let ior = Ior {
        type_id: "IDL:Echo:1.0".into(),
        profiles: vec![IiopProfile {
            version: Version::V1_2,
            host: "localhost".into(),
            port,
            object_key: b"key".to_vec(),
            components: vec![],
        }],
    };
    let err = Connection::connect_tls(&ior, Duration::from_millis(500), client_config("ca.pem"))
        .expect_err("no advertisement, no dial");
    assert!(matches!(err, Error::NoTlsEndpoint), "got: {err}");
    drop(listener);
}

/// TLS failover follows the plain `connect` order: a dead first profile moves
/// the dial to the second advertising profile.
#[test]
fn tls_failover_reaches_the_second_advertising_profile() {
    // A port the OS just proved free: bind ephemeral, take the number, drop.
    // The refusal is then attributable to nothing listening, not to a guess
    // about the environment (the spike-failover lesson).
    let dead_port = {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind probe");
        l.local_addr().expect("local addr").port()
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let live_port = listener.local_addr().expect("local addr").port();
    let (tx, rx) = mpsc::channel();
    let server = one_shot_tls_server(listener, tx);

    let mut ior = tls_only_ior("localhost", dead_port);
    ior.profiles.extend(tls_only_ior("localhost", live_port).profiles);
    let mut conn = Connection::connect_tls(&ior, Duration::from_secs(5), client_config("ca.pem"))
        .expect("failover must reach the live second profile");

    // Prove it reached the real server, not just returned Ok.
    let _ = conn.locate();
    let seen = rx.recv_timeout(Duration::from_secs(5)).expect("bytes via the second profile");
    assert_eq!(&seen[0..4], b"GIOP");
    server.join().expect("server thread");
}
