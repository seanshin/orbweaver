//! Dials `spikes/ssliop_peer.py` **through the SSLIOP path** and reports what
//! happened. The driver half of `spikes/ssliop.sh`; D010 §4 B3.
//!
//! # This file is staged, not built
//!
//! It belongs at `crates/orbweaver-giop/src/bin/spike_ssliop.rs`, with
//!
//! ```toml
//! [[bin]]
//! name = "spike-ssliop"
//! path = "src/bin/spike_ssliop.rs"
//! required-features = ["ssliop"]
//! ```
//!
//! in that crate's `Cargo.toml`. It lives under `spikes/` because
//! `crates/orbweaver-giop` was held by three other batches on the day it was
//! written, and a driver nobody can build is still a better deliverable than a
//! merge conflict in the crate under test. `cargo` does not look here, so
//! nothing in the workspace changes by its presence; moving it is a `git mv`
//! plus the four lines above. Until then `spikes/ssliop.sh` exits 3 —
//! *nothing was measured*, which is a failure and never a pass — unless
//! `SPIKE_SSLIOP` names a build of this file.
//!
//! *이 파일은 `crates/orbweaver-giop/src/bin/`에 있어야 하며, 작성 당일 그
//! 크레이트를 다른 배치 셋이 잡고 있었기 때문에 `spikes/`에 대기 중이다.*
//!
//! # What it measures
//!
//! The address is never handed in. `--ior` names a file the peer wrote, and
//! everything downstream comes out of it: [`Ior::parse`] reads the peer's
//! hand-built encapsulation, [`ssliop::advertised`] reads a
//! `TAG_SSL_SEC_TRANS` component this project's encoder did not write, and
//! [`Connection::connect_tls`] dials [`ssliop::ssl_endpoint`]'s answer. That
//! is the difference between measuring SSLIOP and measuring rustls.
//!
//! `--expect` states the outcome the caller's trust configuration and the
//! peer's advertisement together imply, so the negative directions are
//! measured the same way as the positive one rather than by reading prose off
//! a stream.
//!
//! # Exit codes
//!
//! * `0` — the expectation held.
//! * `1` — it did not; the claim was refuted.
//! * `3` — **nothing was measured**: the IOR or the CA file could not be read
//!   at all, so this process has no account of the peer to be right or wrong
//!   about. Counted apart from `1`, because collapsing them points a false
//!   diagnosis at the code under test on a run where nothing happened.

use std::sync::Arc;
use std::time::Duration;

use orbweaver_giop::{Connection, Error, Ior, ssliop};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

const TIMEOUT: Duration = Duration::from_secs(10);

/// What the caller says should happen, given the trust it configured and the
/// advertisement the peer published.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    /// The handshake completes and the call answers.
    Ok,
    /// A TLS endpoint was advertised and dialing it must fail — an untrusted
    /// certificate, a peer not speaking TLS, an address that refuses.
    Refused,
    /// No usable advertisement, so nothing may be dialed at all. The dual of
    /// `connect` never upgrading: `connect_tls` never downgrades.
    NoTlsEndpoint,
}

fn arg(name: &str) -> Option<String> {
    let mut args = std::env::args();
    while let Some(a) = args.next() {
        if a == name {
            return args.next();
        }
    }
    None
}

fn fail(msg: String) -> ! {
    println!("outcome=refuted");
    println!("why={msg}");
    std::process::exit(1)
}

fn unmeasured(msg: String) -> ! {
    println!("outcome=unmeasured");
    println!("why={msg}");
    std::process::exit(3)
}

fn client_config(ca: &str) -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    let iter = match CertificateDer::pem_file_iter(ca) {
        Ok(i) => i,
        Err(e) => unmeasured(format!("the CA file {ca} could not be read: {e}")),
    };
    for cert in iter {
        match cert {
            Ok(c) => {
                if let Err(e) = roots.add(c) {
                    unmeasured(format!("the CA file {ca} is not a usable trust anchor: {e}"));
                }
            }
            Err(e) => unmeasured(format!("the CA file {ca} did not parse: {e}")),
        }
    }
    let builder = rustls::ClientConfig::builder().with_root_certificates(roots);

    // A client identity, when the target's `target_requires` carries
    // ESTABLISH_TRUST_IN_CLIENT. Optional, so every case that ran before this
    // arrived runs unchanged. Added 2026-09-03, the day omniORB's own SSLIOP
    // endpoint was first dialled: it publishes `requires=0x0066` and answered
    // the handshake with `CertificateRequired` — the component read off ITS
    // encoder saying what it wants, and this driver unable to give it. That
    // is a gap only a foreign encoder could have shown, since our own peer
    // never asked.
    match (arg("--client-cert"), arg("--client-key")) {
        (Some(cert), Some(key)) => {
            let chain: Vec<CertificateDer<'static>> = match CertificateDer::pem_file_iter(&cert) {
                Ok(i) => i
                    .map(|c| {
                        c.unwrap_or_else(|e| {
                            unmeasured(format!("the client cert {cert} did not parse: {e}"))
                        })
                    })
                    .collect(),
                Err(e) => unmeasured(format!("the client cert {cert} could not be read: {e}")),
            };
            let key = PrivateKeyDer::from_pem_file(&key)
                .unwrap_or_else(|e| unmeasured(format!("the client key {key} did not parse: {e}")));
            Arc::new(
                builder.with_client_auth_cert(chain, key).unwrap_or_else(|e| {
                    unmeasured(format!("the client identity is not usable: {e}"))
                }),
            )
        }
        (None, None) => Arc::new(builder.with_no_client_auth()),
        _ => unmeasured("--client-cert and --client-key go together".into()),
    }
}

fn main() {
    let ior_path = arg("--ior").unwrap_or_else(|| unmeasured("no --ior was given".into()));
    let ca = arg("--ca").unwrap_or_else(|| unmeasured("no --ca was given".into()));
    let expect = match arg("--expect").as_deref() {
        Some("ok") => Expect::Ok,
        Some("refused") => Expect::Refused,
        Some("no-tls-endpoint") => Expect::NoTlsEndpoint,
        other => unmeasured(format!("--expect must be ok|refused|no-tls-endpoint, got {other:?}")),
    };
    let a: i32 = arg("--a").and_then(|s| s.parse().ok()).unwrap_or(7);
    let b: i32 = arg("--b").and_then(|s| s.parse().ok()).unwrap_or(35);
    let want_reply_endian = arg("--expect-reply-endian");

    let text = match std::fs::read_to_string(&ior_path) {
        Ok(t) => t,
        Err(e) => unmeasured(format!("the IOR file {ior_path} could not be read: {e}")),
    };
    // The peer's octets, through this project's parser. A failure here is a
    // refutation and not an unmeasured run: the peer published something and
    // we could not read it.
    let ior = match Ior::parse(text.trim()) {
        Ok(i) => i,
        Err(e) => fail(format!("the peer's stringified IOR did not parse: {e}")),
    };
    let profile = match ior.primary() {
        Ok(p) => p,
        Err(e) => fail(format!("the peer's IOR carries no IIOP profile: {e}")),
    };
    println!("type_id={}", ior.type_id);
    println!("profile_endpoint={}:{}", profile.host, profile.port);
    println!("object_key_len={}", profile.object_key.len());

    // What the advertisement says, before anything is dialed. Printed whatever
    // the outcome, because for `none` and `unreadable` this *is* the subject.
    match ssliop::advertised(&profile.components) {
        None => println!("ssliop=absent"),
        Some(Err(e)) => println!("ssliop=unreadable ({e})"),
        Some(Ok(ssl)) => println!(
            "ssliop=supports:{:#06x} requires:{:#06x} port:{}",
            ssl.target_supports, ssl.target_requires, ssl.port
        ),
    }
    match ssliop::ssl_endpoint(profile) {
        None => println!("tls_endpoint=none"),
        Some((h, p)) => println!("tls_endpoint={h}:{p}"),
    }

    let config = client_config(&ca);
    let connected = Connection::connect_tls(&ior, TIMEOUT, config);

    match (expect, connected) {
        (Expect::Ok, Err(e)) => fail(format!("the TLS dial was expected to succeed: {e}")),
        (Expect::Refused, Ok(_)) => {
            fail("a dial that had to be refused succeeded instead".to_string())
        }
        (Expect::NoTlsEndpoint, Ok(_)) => fail(
            "an IOR with no usable SSL advertisement was dialed successfully — \
             the cleartext downgrade this exists to refuse"
                .to_string(),
        ),
        (Expect::Refused, Err(e)) => {
            println!("refusal={e}");
            // A refusal that is not attributable to the transport is the same
            // green-over-nothing as no refusal: `NoTlsEndpoint` here would
            // mean the advertisement was never read, not that the peer was
            // rejected.
            if matches!(e, Error::NoTlsEndpoint) {
                fail("refused, but for want of an advertisement rather than by the peer".into());
            }
            println!("outcome=refused");
        }
        (Expect::NoTlsEndpoint, Err(e)) => {
            println!("refusal={e}");
            if !matches!(e, Error::NoTlsEndpoint) {
                fail(format!("expected NoTlsEndpoint, got a different refusal: {e}"));
            }
            println!("outcome=no-tls-endpoint");
        }
        (Expect::Ok, Ok(mut conn)) => {
            let (host, port) = {
                let (h, p) = conn.endpoint();
                (h.to_owned(), p)
            };
            println!("dialed={host}:{port}");
            println!("request_endian={:?}", conn.endian());
            let reply = match conn.invoke("add", |e| {
                e.put_i32(a);
                e.put_i32(b);
            }) {
                Ok(r) => r,
                Err(e) => fail(format!("the call over TLS failed: {e}")),
            };
            println!("reply_endian={:?}", reply.endian);
            println!("reply_status={:?}", reply.status);
            let mut body = match reply.body() {
                Ok(d) => d,
                Err(e) => fail(format!("the reply body would not seek: {e}")),
            };
            let sum = match body.get_i32() {
                Ok(v) => v,
                Err(e) => fail(format!("the reply body would not decode: {e}")),
            };
            println!("sum={sum}");
            if sum != a + b {
                fail(format!("{a} + {b} came back as {sum} through TLS"));
            }
            if let Some(want) = want_reply_endian {
                let got = format!("{:?}", reply.endian).to_lowercase();
                if got != want.to_lowercase() {
                    fail(format!("the peer answered in {got} and was told to answer in {want}"));
                }
            }
            println!("outcome=ok");
        }
    }
}
