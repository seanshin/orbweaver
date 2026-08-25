//! `string_to_object` and `object_to_string` measured against an ORB that
//! implements the same two operations and shares none of our assumptions.
//!
//! This is the oracle D019 step 2 needs, and the reason it can exist at all is
//! that **omniORB has the very same pair under the very same names** — CORBA
//! 3.4 §8.2.2 defines them, so a peer is not merely a second opinion here, it
//! is a second implementation of the identical contract. Clause (a) of the
//! licensing boundary: a separate process reached by running `python3`,
//! nothing linked, vendored or redistributed.
//!
//! # The sentence being measured
//!
//! §8.2.2 promises exactly one thing, and it is a cross-ORB promise:
//!
//! > *"For all conforming ORBs, if obj is a valid reference to an object, then
//! > `string_to_object(object_to_string(obj))` will return a valid reference to
//! > the same object … For all conforming ORBs supporting IOP, this remains
//! > true **even if the two operations are performed on different ORBs**."*
//!
//! So the round trip is run **through** the peer: we stringify, omniORB reads
//! and restringifies, and we read the result back. A convention both halves of
//! this crate share cannot be refuted by our own round trip; it can be refuted
//! by that one.
//!
//! # Why no server, no port and no fixture process
//!
//! These two operations touch no socket. The whole probe is a conversion, so
//! there is nothing to bind, nothing to wait for and nothing that can collide
//! with another run of the harness — which also means a failure here is about
//! the conversion and never about scheduling.
//!
//! # What is compared, and what deliberately is not
//!
//! The **dialable facts**: the repository id, and per profile the GIOP version,
//! host, port and object key. Not the raw bytes and not the tagged components —
//! CLAUDE.md's rule is to compare decoded values, and an ORB is free to add
//! components of its own to a reference it re-emits. A byte comparison here
//! would report a difference of policy as a defect.
//!
//! # When the fixture is absent
//!
//! omniORBpy is a fixture (`brew install omniorb`), not a dependency. Without
//! it this prints a `SKIPPED` line naming what went unmeasured and passes,
//! exactly as the harness's naming groups do. The marker is printed either way
//! so that absence stays visible; a silent pass is the failure this file exists
//! to avoid.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use orbweaver_giop::orb::Orb;
use orbweaver_giop::{IiopProfile, Ior, Version};

/// The driver. Reads `label\tstring` lines and prints `label\tanswer`, where
/// the answer is the peer's `object_to_string` of what its own
/// `string_to_object` made of the input, or `REFUSED:<class>` — never a
/// traceback, so a refusal is data rather than a crash to be parsed.
const DRIVER: &str = r#"
import sys
from omniORB import CORBA

orb = CORBA.ORB_init([])
for line in open(sys.argv[1]):
    line = line.rstrip("\n")
    if not line:
        continue
    label, _, text = line.partition("\t")
    try:
        obj = orb.string_to_object(text)
    except CORBA.SystemException as e:
        print("%s\tREFUSED:%s" % (label, e.__class__.__name__))
        continue
    except Exception as e:
        print("%s\tREFUSED:%s" % (label, type(e).__name__))
        continue
    if obj is None:
        # A nil reference is a legal answer, not a failure: it is what the
        # nil IOR denotes. Restringify it the same way as any other.
        print("%s\t%s" % (label, orb.object_to_string(None)))
        continue
    print("%s\t%s" % (label, orb.object_to_string(obj)))
"#;

/// The dialable facts of a reference: what survives a trip through another ORB
/// and what a caller would actually use. See the module docs for why the
/// tagged components are not in here.
#[derive(Debug, PartialEq, Eq)]
struct Dialable {
    type_id: String,
    profiles: Vec<(u8, u8, String, u16, Vec<u8>)>,
}

fn dialable(ior: &Ior) -> Dialable {
    Dialable {
        type_id: ior.type_id.clone(),
        profiles: ior
            .profiles
            .iter()
            .map(|p| {
                (p.version.major, p.version.minor, p.host.clone(), p.port, p.object_key.clone())
            })
            .collect(),
    }
}

/// Every `(host, port, key)` a caller would dial, in dialing order, across
/// every profile **and every `TAG_ALTERNATE_IIOP_ADDRESS` on it**.
///
/// # Why the URL comparison uses this and not the profile list
///
/// Measured 2026-08-25, and it is the one thing in this file a test written
/// from our side alone could never have found. Given
/// `corbaloc::a.test:1111,:b.test:2222/Key`, the two ORBs build **structurally
/// different references that name the same two endpoints in the same order**:
///
/// - we emit **one profile per address** (`naming::addressed_ior`);
/// - omniORB emits **one profile carrying the first address, and folds every
///   later address into a `TAG_ALTERNATE_IIOP_ADDRESS` component** (IOP
///   ComponentId 3) on it.
///
/// Both readings are legal and neither is a defect: §7.6.10.1 says the object
/// may be contacted at any address in the list and does not prescribe how a
/// reference records that, and this crate has read alternates since the
/// failover work ([`IiopProfile::endpoints`]). Comparing profile lists would
/// report that difference of representation as a failure; comparing the dialing
/// order is what the URL actually promises. The `nil` reference has no
/// endpoints at all, which is why the round-trip rows use [`dialable`].
fn endpoints(ior: &Ior) -> Vec<(String, u16, Vec<u8>)> {
    ior.profiles
        .iter()
        .flat_map(|p| p.endpoints().into_iter().map(|(h, port)| (h, port, p.object_key.clone())))
        .collect()
}

fn profile(host: &str, port: u16, key: &[u8], version: Version) -> IiopProfile {
    IiopProfile { version, host: host.into(), port, object_key: key.to_vec(), components: vec![] }
}

fn omniorbpy_present() -> bool {
    Command::new("python3")
        .args(["-c", "import omniORB"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn scratch() -> PathBuf {
    let unique =
        SystemTime::now().duration_since(UNIX_EPOCH).expect("a clock after 1970").as_nanos();
    let dir =
        std::env::temp_dir().join(format!("orbweaver-strobj-peer-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// What the peer must answer for each string we cannot resolve. Measured
/// against omniORB 4.3.4 on 2026-08-25 before being written here.
///
/// The point of these rows is not that the *spelling* of the refusal matches —
/// it cannot, since omniORB raises CORBA system exceptions and we return a
/// typed Rust error — but that **the peer refuses exactly the strings we refuse
/// and accepts exactly the ones we accept.** A string one ORB reads and the
/// other rejects is an interoperability defect in whichever is wrong, and it is
/// invisible to any test written from our side alone.
/// The three classes here were **guessed as `BAD_PARAM` and corrected by the
/// run**, which is the reason the row carries the class at all: two of the five
/// are something else, and the difference is informative rather than noise.
const EXPECTED_REFUSALS: &[(&str, &str)] = &[
    ("bad_text", "REFUSED:BAD_PARAM"),
    // Not `BAD_PARAM`: the prefix is well-formed and the *body* is not a
    // marshalled reference, so omniORB reports the decode rather than the
    // argument. Our own `StringToObjectError` splits the same two causes
    // (`NotAReferenceString` vs `Ior`) for the same reason.
    ("bad_hex", "REFUSED:MARSHAL"),
    ("bad_port", "REFUSED:BAD_PARAM"),
    ("bad_scheme", "REFUSED:BAD_PARAM"),
    // `corbaloc:rir:` resolves against the *asking* ORB's own table (§8.5.2 is
    // explicit that the mechanism is local), and this driver configures no
    // `-ORBInitRef`, so the peer has nothing under `TradingService` either.
    // Both ORBs refuse; only one of them is consulting our table.
    //
    // `NO_RESOURCES`, not `BAD_PARAM`, and that is the same distinction
    // `InvalidName::NotRegistered { reserved }` makes: omniORB answers
    // `NO_RESOURCES(InitialRefNotFound)` for a **reserved** ObjectId nothing
    // registered and `BAD_PARAM(BadURIOther)` for an invented one. A second
    // implementation drawing the same line is why that field is not a nicety.
    ("rir_unregistered", "REFUSED:NO_RESOURCES"),
];

#[test]
fn omniorb_reads_and_rewrites_every_reference_string_we_produce() {
    if !omniorbpy_present() {
        println!(
            "strobj-peer: SKIPPED — omniORBpy is not installed, so string_to_object and \
             object_to_string are unmeasured against an independent implementation of the \
             same two operations, not passing"
        );
        let _ = std::io::stdout().flush();
        return;
    }

    let orb = Orb::new();

    // ── the references whose round trip §8.2.2 promises ──
    let references: Vec<(&str, Ior)> = vec![
        ("rt_nil", Ior { type_id: String::new(), profiles: vec![] }),
        (
            "rt_one",
            Ior {
                type_id: "IDL:spike/Echo:1.0".into(),
                profiles: vec![profile("192.0.2.1", 4001, b"Echo", Version::V1_2)],
            },
        ),
        (
            "rt_multi",
            Ior {
                type_id: "IDL:omg.org/CosNaming/NamingContextExt:1.0".into(),
                profiles: vec![
                    profile("a.test", 1111, b"A", Version::V1_0),
                    profile("::1", 2222, b"B", Version::V1_1),
                    profile("c.test", 3333, b"C", Version::V1_2),
                ],
            },
        ),
        (
            // An object key is opaque bytes, not text. A peer that treats it as
            // a string mangles this one, and nothing else in this file would
            // notice.
            "rt_binary_key",
            Ior {
                type_id: "IDL:spike/Echo:1.0".into(),
                profiles: vec![profile("h.test", 4002, &[0x00, 0xFF, 0x80, b'/'], Version::V1_2)],
            },
        ),
    ];

    // ── URL forms: both ORBs read the same URL, and must build the same
    //    reference out of it. `url_defaults` is the one that matters most —
    //    §7.6.10.3 fills in GIOP 1.0 and port 2809, which is the trap this
    //    crate's own docs open with. ──
    let urls: Vec<(&str, &str)> = vec![
        ("url_explicit", "corbaloc:iiop:1.2@192.0.2.1:4000/Echo"),
        ("url_defaults", "corbaloc::192.0.2.1/Echo"),
        ("url_ipv6", "corbaloc:iiop:[::1]:88/Key"),
        ("url_escaped_key", "corbaloc::h.test/a%20b%2Fc"),
        // No fragment: the URL denotes the naming context itself, which needs
        // no call. Measured 2026-08-25 — this is the corbaname form the two
        // ORBs agree on exactly. The form with a name in it is
        // `CORBANAME_WITH_NAME` below, and it is where they part.
        ("url_corbaname_context", "corbaname::h.test:2809/NameService"),
    ];

    // The multi-address form gets its own row: the two ORBs agree on the
    // endpoints and disagree on how a reference records them, and the
    // disagreement costs us something real. See `a_second_corbaloc_address`
    // below, which is where it is pinned.
    const MULTI: &str = "corbaloc::a.test:1111,:b.test:2222/Key";

    // The corbaname form that carries a name. Both ORBs decline to answer it
    // from the string alone; they decline differently, and the difference is
    // the point. Pinned in `a_corbaname_with_a_name_in_it` below.
    const CORBANAME_WITH_NAME: &str = "corbaname::h.test:2809/NameService#spike/Echo";

    let refusals: Vec<(&str, &str)> = vec![
        ("bad_text", "hello world"),
        ("bad_hex", "IOR:zzz"),
        ("bad_port", "corbaloc::h:notaport/K"),
        ("bad_scheme", "http://example.test/x"),
        ("rir_unregistered", "corbaloc:rir:/TradingService"),
    ];

    // ── build the input the peer will read ──
    let mut input = String::new();
    for (label, obj) in &references {
        let s = orb.object_to_string(obj).expect("our object_to_string");
        assert!(s.starts_with("IOR:"), "§8.2.2 asks for the interoperable form: {s}");
        input.push_str(&format!("{label}\t{s}\n"));
    }
    for (label, url) in &urls {
        input.push_str(&format!("{label}\t{url}\n"));
    }
    input.push_str(&format!("url_multi\t{MULTI}\n"));
    input.push_str(&format!("corbaname_with_name\t{CORBANAME_WITH_NAME}\n"));
    for (label, text) in &refusals {
        input.push_str(&format!("{label}\t{text}\n"));
    }

    let dir = scratch();
    let input_path = dir.join("input.tsv");
    let driver_path = dir.join("driver.py");
    std::fs::write(&input_path, &input).expect("write the input");
    std::fs::write(&driver_path, DRIVER).expect("write the driver");

    let run = Command::new("python3")
        .arg(&driver_path)
        .arg(&input_path)
        .output()
        .expect("python3 runs, since omniORB imported a moment ago");

    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        run.status.success(),
        "the peer's driver did not finish.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    let answers: Vec<(&str, &str)> = stdout
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(a, b)| (a.trim(), b.trim()))
        .collect();
    let answer = |want: &str| -> &str {
        answers
            .iter()
            .find(|(l, _)| *l == want)
            .map(|(_, v)| *v)
            .unwrap_or_else(|| panic!("the peer said nothing about {want}.\nstdout:\n{stdout}"))
    };

    let mut measured = 0usize;

    // ── §8.2.2's cross-ORB round trip ──
    for (label, obj) in &references {
        let back = answer(label);
        assert!(
            back.starts_with("IOR:"),
            "{label}: the peer could not read a reference we wrote — {back}\n\
             stdout:\n{stdout}\nstderr:\n{stderr}"
        );
        let reread = orb
            .string_to_object(back)
            .unwrap_or_else(|e| panic!("{label}: we could not read the peer's rewrite: {e}"));
        assert_eq!(
            dialable(&reread),
            dialable(obj),
            "{label}: string_to_object(object_to_string(obj)) across two ORBs is not obj"
        );
        measured += 1;
    }

    // ── the URL branch, against a peer's reading of the same URL ──
    for (label, url) in &urls {
        let back = answer(label);
        assert!(
            back.starts_with("IOR:"),
            "{label}: the peer refused a URL we accept ({url}) — {back}"
        );
        let theirs = orb
            .string_to_object(back)
            .unwrap_or_else(|e| panic!("{label}: we could not read the peer's rewrite: {e}"));
        let ours = orb
            .string_to_object(url)
            .unwrap_or_else(|e| panic!("{label}: we refused a URL the peer accepts ({url}): {e}"));
        // The repository id is deliberately excluded from this one comparison:
        // a URL carries no type, we leave it empty (§8.5.2 narrows later), and
        // an ORB is free to fill in something of its own. What a caller dials
        // must agree exactly — see `endpoints` for why that, and not the
        // profile list, is the comparison a URL actually promises.
        assert_eq!(
            endpoints(&theirs),
            endpoints(&ours),
            "{label}: the two ORBs read {url} as different endpoints, in this order"
        );
        measured += 1;
    }

    // ── a second corbaloc address: agreement on the endpoints, disagreement
    //    on how a reference records them, and a capability we lose by it ──
    //
    // **Found by this oracle on 2026-08-25 and by nothing else.** Given
    // `corbaloc::a.test:1111,:b.test:2222/Key`:
    //
    // - we build **two profiles**, so `endpoints()` yields both addresses;
    // - omniORB builds **one profile at IIOP 1.0** and appends a
    //   `TAG_ALTERNATE_IIOP_ADDRESS` component (ComponentId 3) carrying
    //   `b.test:2222`;
    // - and `parse_iiop_profile` (`lib.rs:807`) reads a component sequence only
    //   `if minor >= 1`, because §7.6.2 gives `ProfileBody_1_0` no components
    //   field. Its own comment anticipated trailing data and chose to tolerate
    //   it without reading it.
    //
    // So **we silently drop omniORB's alternate address, and failover to
    // `b.test` never happens** for any reference omniORB produced this way.
    // That is a wire-behaviour finding, not a `string_to_object` one: fixing it
    // means deciding whether to read components after a 1.0 profile body, which
    // is a change to how every reference from every peer is parsed and belongs
    // in its own batch with its own peer measurement. It is pinned here rather
    // than fixed, so that the day it *is* fixed this assertion goes red and the
    // divergence record has to be updated instead of quietly rotting.
    {
        let back = answer("url_multi");
        let theirs = orb.string_to_object(back).expect("we can read the peer's rewrite");
        let ours = orb.string_to_object(MULTI).expect("we accept the multi-address form");

        assert_eq!(
            endpoints(&ours),
            vec![
                ("a.test".to_string(), 1111, b"Key".to_vec()),
                ("b.test".to_string(), 2222, b"Key".to_vec()),
            ],
            "we build one profile per address"
        );
        assert_eq!(
            endpoints(&theirs),
            vec![("a.test".to_string(), 1111, b"Key".to_vec())],
            "measured: omniORB's second address is on a component we do not read"
        );
        // The two halves of the cause, pinned separately so a change to either
        // one names itself rather than showing up as a count.
        assert_eq!(
            theirs.profiles.len(),
            1,
            "omniORB folds the address list into one profile, not several"
        );
        assert_eq!(
            theirs.profiles[0].version,
            Version::V1_0,
            "…at IIOP 1.0, which is why our component reader skips what follows"
        );
        assert!(
            theirs.profiles[0].components.is_empty(),
            "…and so the alternate never reaches `IiopProfile::endpoints`"
        );
        // Both ORBs do agree on where to dial *first*, which is the part that
        // makes this a lost fallback rather than a broken reference.
        assert_eq!(endpoints(&ours)[0], endpoints(&theirs)[0]);
        measured += 1;
    }

    // ── a corbaname URL carrying a name: what neither ORB will do from the
    //    string alone, and the one place the peer proved us wrong ──
    //
    // **Found by this oracle on 2026-08-25.** Part 2 §7.6.10.5: such a URL
    // denotes the object bound under that name, *not* the naming context that
    // holds it. Producing it takes an outbound `resolve`.
    //
    // - omniORB **dials**. Against an unreachable naming service it answers
    //   `TRANSIENT` — a network failure, from inside a conversion.
    // - we, before this batch, would have handed back the **naming context**:
    //   `ObjectUrl::to_ior` ignores the `name` field entirely, because its two
    //   callers (`NamingContext::from_url`, `corbaloc_to_ior_string`) go on to
    //   resolve the name themselves in a second step. Routing that through
    //   `string_to_object` unchanged would have returned *the wrong object,
    //   silently*, which is the one answer this operation must never give.
    //
    // So `string_to_object` refuses it, naming the name it would have to
    // resolve and pointing at the two-step path that does the work. Dialling
    // inside a conversion is a real behaviour change with a timeout to choose
    // and belongs in its own batch; answering wrongly is not an option and
    // answering honestly is free.
    {
        assert_eq!(
            answer("corbaname_with_name"),
            "REFUSED:TRANSIENT",
            "measured: omniORB resolves a corbaname URL inside string_to_object"
        );
        let ours = orb.string_to_object(CORBANAME_WITH_NAME).unwrap_err();
        let said = ours.to_string();
        assert!(said.contains("spike/Echo"), "our refusal names the name it would resolve: {said}");
        assert!(said.contains("§7.6.10.5"), "and cites the sub clause: {said}");
        // The neighbouring form, which is a reference and not a lookup, is
        // still answered — and the peer agrees, which the `urls` loop above
        // has already asserted.
        assert!(orb.string_to_object("corbaname::h.test:2809/NameService").is_ok());
        measured += 1;
    }

    // ── the strings both ORBs must refuse ──
    let got_refusals: Vec<(&str, &str)> =
        EXPECTED_REFUSALS.iter().map(|(l, _)| (*l, answer(l))).collect();
    assert_eq!(
        got_refusals,
        EXPECTED_REFUSALS.to_vec(),
        "a string one ORB reads and the other refuses is an interoperability defect.\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    for (label, text) in &refusals {
        assert!(
            orb.string_to_object(text).is_err(),
            "{label}: the peer refuses {text:?} and we do not"
        );
        measured += 1;
    }

    println!(
        "strobj-peer: measured {measured} reference strings against omniORB's own \
         string_to_object/object_to_string"
    );
    let _ = std::io::stdout().flush();
}
