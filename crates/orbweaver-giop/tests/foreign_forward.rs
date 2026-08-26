//! A **foreign** ORB forwards our client, and our client lands somewhere else.
//!
//! Location transparency was the best-measured of D029 §6.1's five rows and it
//! was measured in one direction only. This ORB **sends** `LOCATION_FORWARD`
//! and `LOCATE_FORWARD` in both orders across GIOP 1.0/1.1/1.2, and it
//! **follows** a forward — but every forward it had ever followed was one it
//! had written itself. CLAUDE.md names that shape exactly:
//!
//! > A convention both ends apply cannot be refuted by a round trip, and a
//! > convention one end applies on read can hide the other end's defect on
//! > write.
//!
//! So this file's peer is omniORB, rigged through **its own** mechanism (a POA
//! with `USE_SERVANT_MANAGER` + `NON_RETAIN` whose `ServantLocator.preinvoke`
//! raises `ForwardRequest`) to answer `LOCATION_FORWARD` naming a **second**
//! omniORB process at a **different port**. Nothing in this repository encodes
//! that reply. `spikes/foreign_forward_peer.py` starts the two peers and
//! `spikes/foreign_forward.sh` owns the verdict.
//!
//! # Why these tests are `#[ignore]`
//!
//! They need two live foreign processes. A test that quietly passes when its
//! fixture is missing is the *green-while-measuring-nothing* class CLAUDE.md
//! has five measured instances of, so this file never takes that option: with
//! the fixture absent these tests do not run at all under `cargo test`, and
//! when the script runs them with `--ignored` a missing environment variable is
//! a **panic**, never a skip. Which runs are absent is counted by the script,
//! as a `SKIPPED` group naming its fixture (D010 §2).
//!
//! # What each case asserts, and why it is three things and not one
//!
//! A reply arriving is not the claim. The claim is that a caller holding only a
//! reference reached a target it was never told the location of, so each case
//! asserts all of:
//!
//! 1. **The answer came from the destination.** The servant reports the address
//!    it actually ran at, so the *result* distinguishes "reached the
//!    destination" from "reached the forwarder" without trusting either
//!    server's log — our own counters are not what the peer saw (D034 §5.1),
//!    and neither are the peer's.
//! 2. **The connection moved.** [`Connection::endpoint`] is the destination's
//!    port after the call, not the one we dialled.
//! 3. **The reference did what its status says.** After a *temporary* forward
//!    [`Connection::origin`] still names the forwarder — §9.4.3.2 keeps the
//!    dialled reference good, and a caller that had to be handed the new
//!    address to make the second call would have learned the target's
//!    location, which is the leak this row exists to refuse. After a
//!    *permanent* one the forwarded-to IOR becomes the origin, which is §9.6's
//!    *may* and which this ORB takes up.
//!
//!    That distinction was not in the first draft: it asserted the origin never
//!    moves, and two GIOP 1.2 cases went red against a peer behaving correctly
//!    and a client behaving as documented, the moment the fixture was pointed
//!    at its permanent mechanism. An assertion that is true of one status is
//!    not a property of forwarding.
//!
//! # Byte order is read, never assumed
//!
//! `sent` is what we chose; `observed` is [`Reply::endian`], which the decoder
//! took off the message's flag byte. Measured 2026-08-26 against omniORB 4.3.4:
//! **the two differ.** A big-endian request comes back little-endian — omniORB
//! replies in its own native order and is entitled to, since §9.3.1 makes the
//! order a per-message property of the sender. A client that assumed a reply
//! matched its request would be wrong on half of these cases, and this project
//! has a measured instance of a probe reporting an order it had assumed, which
//! is why the two are printed side by side rather than collapsed.

use orbweaver_cdr::Endian;
use orbweaver_giop::{Connection, Ior, ReplyStatus, Version};
use std::time::Duration;

/// Long enough that a loaded machine does not look like a broken forward.
const DIAL: Duration = Duration::from_secs(10);

/// Reads a variable the script is required to have set.
///
/// Panics rather than returning an `Option`. A missing fixture must not be
/// able to make this file look green.
fn required(name: &str) -> String {
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => v,
        _ => panic!(
            "{name} is not set. These tests need the two live omniORB peers; \
             run them through spikes/foreign_forward.sh, which starts the \
             fixture and counts a missing one as SKIPPED rather than passing."
        ),
    }
}

/// The forwarder's IOR, and the address the destination is expected at.
///
/// The destination's address comes from the script rather than from the
/// forwarded-to IOR, deliberately: reading it out of the reply would make the
/// assertion "the peer forwarded us where the peer said it would", which is
/// true of any forward at all, including one pointing back at the forwarder.
/// Taking it from the fixture that started the second process makes the
/// assertion "we landed at the OTHER process".
fn fixture() -> (Ior, String, u16) {
    let path = required("OW_FOREIGN_FORWARD_IOR");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read the forwarder IOR at {path}: {e}"));
    let ior = Ior::parse(text.trim())
        .unwrap_or_else(|e| panic!("the forwarder published something we cannot parse: {e}"));
    let host = required("OW_FOREIGN_FORWARD_DEST_HOST");
    let port: u16 = required("OW_FOREIGN_FORWARD_DEST_PORT")
        .parse()
        .expect("OW_FOREIGN_FORWARD_DEST_PORT must be a port number");
    (ior, host, port)
}

/// One case: dial the forwarder at `version`/`endian`, expect to end up at the
/// destination having called it successfully.
fn follows_a_foreign_forward(version: Version, endian: Endian) {
    let (ior, dest_host, dest_port) = fixture();
    let dialled = {
        let p = ior.primary().expect("forwarder IOR has no IIOP profile");
        (p.host.clone(), p.port)
    };
    assert_ne!(
        dialled.1, dest_port,
        "the fixture published the forwarder and the destination at the same \
         port; this leg measures a move to a DIFFERENT address and cannot"
    );

    let mut conn = Connection::connect(&ior, DIAL)
        .unwrap_or_else(|e| panic!("cannot dial the forwarder at {dialled:?}: {e}"));
    conn.cap_version(version);
    conn.set_endian(endian);

    let reply = conn
        .invoke("where_am_i", |e| e.put_str("rust-client"))
        .unwrap_or_else(|e| panic!("GIOP {version:?} {endian:?}: the call did not complete: {e}"));

    assert_eq!(
        reply.status,
        ReplyStatus::NoException,
        "GIOP {version:?} {endian:?}: after following the foreign forward the \
         call still did not succeed"
    );

    let answer = reply
        .body()
        .and_then(|mut d| Ok(d.get_string()?))
        .unwrap_or_else(|e| panic!("GIOP {version:?} {endian:?}: unreadable result: {e}"));

    // (1) The answer came from the destination, in the destination's own words.
    //
    // The servant answers "<tag>@<host>:<port>:<note>" with the address it
    // actually ran at, so the address is asserted and the tag is not. An
    // earlier draft asserted the tag literal and went red six times against a
    // fixture that had forwarded correctly every time — the operator had
    // started the peer under a different `--tag`. A gate that can be reddened
    // by renaming a label is measuring the label.
    let ran_at = format!("@{}:{}:", dest_host, dest_port);
    assert!(
        answer.contains(&ran_at),
        "GIOP {version:?} {endian:?}: the call was answered by {answer:?}, which \
         does not report having run at {ran_at:?}; a forward that lands back on \
         the forwarder is not a move"
    );
    assert!(
        !answer.contains(&format!("@{}:{}:", dialled.0, dialled.1)),
        "GIOP {version:?} {endian:?}: {answer:?} was answered by the FORWARDER \
         itself, at the address we dialled"
    );

    // (2) The connection moved, and (3) the reference did what its status says.
    let permanent = conn
        .forwarded()
        .unwrap_or_else(|| {
            panic!(
                "GIOP {version:?} {endian:?}: the call succeeded but the \
                 connection does not record having been forwarded"
            )
        })
        .is_permanent();
    assert_eq!(
        conn.endpoint().1,
        dest_port,
        "GIOP {version:?} {endian:?}: the connection is not at the destination"
    );

    // Which origin is correct depends on the status, and this assertion used to
    // ignore that. It asserted the origin never moves, which is right for
    // `LOCATION_FORWARD` and wrong for `LOCATION_FORWARD_PERM` — §9.6 says the
    // client *may* replace the old IOR with the new one, and this ORB takes up
    // that permission (see `Connection`'s own documentation). Found by pointing
    // the fixture at its permanent mechanism, which the leg had not been built
    // to exercise: two GIOP 1.2 cases went red against a peer that was behaving
    // correctly and a client that was behaving as documented.
    let origin_port = conn.origin().primary().expect("origin has no profile").port;
    if permanent {
        assert_eq!(
            origin_port, dest_port,
            "GIOP {version:?} {endian:?}: after a PERMANENT forward the \
             forwarded-to IOR becomes the origin (§9.6's *may*, which this ORB \
             takes up); it is still the address we dialled"
        );
    } else {
        assert_eq!(
            origin_port, dialled.1,
            "GIOP {version:?} {endian:?}: a TEMPORARY forward rewrote the \
             caller's own reference; §9.4.3.2 keeps the dialled reference good, \
             so the caller must still hold what it was given"
        );
    }

    // Order: what we chose beside what the peer actually used.
    println!(
        "cell giop={}.{} sent={endian:?} observed={:?} perm={permanent} dialled={}:{} landed={}:{} answer={answer}",
        version.major,
        version.minor,
        reply.endian,
        dialled.0,
        dialled.1,
        conn.endpoint().0,
        conn.endpoint().1,
    );
}

#[test]
#[ignore = "needs the two live omniORB peers; run via spikes/foreign_forward.sh"]
fn giop_1_2_little_endian_follows_a_foreign_forward() {
    follows_a_foreign_forward(Version::V1_2, Endian::Little);
}

#[test]
#[ignore = "needs the two live omniORB peers; run via spikes/foreign_forward.sh"]
fn giop_1_2_big_endian_follows_a_foreign_forward() {
    follows_a_foreign_forward(Version::V1_2, Endian::Big);
}

#[test]
#[ignore = "needs the two live omniORB peers; run via spikes/foreign_forward.sh"]
fn giop_1_1_little_endian_follows_a_foreign_forward() {
    follows_a_foreign_forward(Version::V1_1, Endian::Little);
}

#[test]
#[ignore = "needs the two live omniORB peers; run via spikes/foreign_forward.sh"]
fn giop_1_1_big_endian_follows_a_foreign_forward() {
    follows_a_foreign_forward(Version::V1_1, Endian::Big);
}

#[test]
#[ignore = "needs the two live omniORB peers; run via spikes/foreign_forward.sh"]
fn giop_1_0_little_endian_follows_a_foreign_forward() {
    follows_a_foreign_forward(Version::V1_0, Endian::Little);
}

#[test]
#[ignore = "needs the two live omniORB peers; run via spikes/foreign_forward.sh"]
fn giop_1_0_big_endian_follows_a_foreign_forward() {
    follows_a_foreign_forward(Version::V1_0, Endian::Big);
}

/// A second call on a fresh connection is forwarded again, and lands again.
///
/// `LOCATION_FORWARD` is the *temporary* status: §9.4.3.2 says the reference
/// the caller holds stays good, so a caller that dials it again must be
/// forwarded again rather than being expected to have remembered anything. This
/// asserts the foreign peer really does re-forward — the case where a caller
/// having to cache the new address would be a leak, because caching is the
/// caller learning where the target lives.
#[test]
#[ignore = "needs the two live omniORB peers; run via spikes/foreign_forward.sh"]
fn a_second_dial_of_the_same_reference_is_forwarded_again() {
    let (ior, dest_host, dest_port) = fixture();
    let _ = &dest_host;
    for round in 1..=2 {
        let mut conn = Connection::connect(&ior, DIAL).expect("dial the forwarder");
        let reply = conn
            .invoke("where_am_i", |e| e.put_str("again"))
            .unwrap_or_else(|e| panic!("round {round}: call did not complete: {e}"));
        assert_eq!(reply.status, ReplyStatus::NoException, "round {round}");
        assert!(
            conn.forwarded().is_some(),
            "round {round}: the reference stopped being forwarded; neither \
             status invalidates the reference the caller holds — §9.4.3.2 keeps \
             the dialled one good and §9.6 only lets the CLIENT prefer the new \
             one, which says nothing about what the server answers next time"
        );
        assert_eq!(conn.endpoint().1, dest_port, "round {round}");
        println!("cell round={round} landed={}", conn.endpoint().1);
    }
}
