//! `spikes/wide.idl` on the wire, from our own stack — both seats.
//!
//! Commit 382baa9 measured the GIOP 1.1 `wchar` against JacORB in both
//! directions, but the seat on our side was a hand-built Python peer
//! (`spikes/jacorb_wchar11.py`): `spike-server` and `spike-interop` know only
//! `spikes/echo.idl`, so the live Rust server and client were never on the
//! wire for this contract, and the codec was held to the exchanged octets by
//! tests alone. This binary closes that: one process that either **serves**
//! `IDL:spike/Wide:1.0` through [`Server`] and a hand-written [`Dispatch`], or
//! **calls** a `Wide` reference through [`Connection`], in either byte order,
//! at whatever GIOP version the profile advertises. Driven by
//! `spikes/wide_rust.sh`, which puts the octets on record through the tap
//! (`spikes/jacorb_giop11_tap.py`) and checks them against
//! `crates/orbweaver-giop/tests/wide_1_1_from_a_peer.rs`.
//!
//! The dispatch is hand-written rather than generated, deliberately stated:
//! a generated skeleton for `wide.idl` would have to be checked in beside this
//! file with nothing to re-bless it, and `orbweaver-gen`'s `rt::WChar` reaches
//! the very same `WideCodec::put_wchar`/`get_wchar` through the stream codec
//! that [`Server`] and [`Connection`] attach — so what is on the wire here is
//! the code path both shapes share, and the generated shape stays a
//! recommendation in the batch report.
//!
//! ```text
//! spike-wide serve <ior-out> [host] [port]
//! spike-wide call  <ior-file> be|le [--text <s>]... [<unit-hex>]...
//! ```
//!
//! `serve` publishes an IIOP 1.2 IOR with our `TAG_CODE_SETS`; the profile
//! version a peer dials is the tap's to rewrite, as `spikes/jacorb_giop11.sh`
//! already does — no `--iiop-minor` flag was added, so the version on the wire
//! is always something the tap's headers can be asked about. Every served
//! call is logged with the version, byte order and the value **our reader
//! decoded**, which is a measurement of our decoder that does not depend on
//! what the peer's user reports.
//!
//! `call` dials the profile as advertised, sends `echo_wchar` for each unit
//! and `echo_wstring` for each `--text`, in the byte order asked, and prints
//! the decoded reply per call. A unit that is not a character (a lone
//! surrogate) is reported as behaviour: our writer cannot ask for it. Under
//! GIOP 1.0, where `wchar` is illegal, the codec's refusal is reported and a
//! raw two-octet `echo_wchar` is sent anyway to see the server refuse it too.
//!
//! *우리 스택을 wide.idl의 양쪽 자리에 앉힌다. 서버는 디코드한 값을 로그에 남기고,
//! 클라이언트는 프로파일이 광고한 버전으로 두 바이트 순서 모두를 보낸다.*

use std::time::Duration;

use orbweaver_cdr::{Encoder, Endian};
use orbweaver_giop::codeset::{CodeSetId, WideCodec};
use orbweaver_giop::server::{Completion, Dispatch, MARSHAL, Request, Server, SystemException};
use orbweaver_giop::{Connection, Error, Ior};
use orbweaver_object::ObjectOps;
use orbweaver_registry::{Registry, Strictness};

/// Matches `spikes/wide.idl`.
const TYPE_ID: &str = "IDL:spike/Wide:1.0";
/// The key JacORB's recorded request carries
/// (`JACORB_REQUEST_HAN` in `tests/wide_1_1_from_a_peer.rs`), so a live
/// request from the same client can be compared octet for octet.
const OBJECT_KEY: &[u8] = b"OrbweaverWide";

const OK: &str = "ok  ";
const NO: &str = "FAIL";

/// OMG's vendor id in a minor code (CORBA §7.1.1); `OMGVMCID | 6` is
/// "wchar/wstring in a GIOP 1.0 message" (§9.3.1.6).
const OMGVMCID: u32 = 0x4F4D_0000;

/// The OMG-defined part of a minor code, if it is one — as `spike-interop`
/// prints it, so "minor 6" reads as the specification's number.
fn omg_minor(minor: u32) -> Option<u32> {
    if minor & 0xFFFF_F000 == OMGVMCID { Some(minor & 0xFFF) } else { None }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("serve") => match serve(&args[1..]) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("spike-wide serve: {e}");
                std::process::ExitCode::from(2)
            }
        },
        Some("call") => call(&args[1..]),
        _ => {
            eprintln!(
                "usage: spike-wide serve <ior-out> [host] [port]\n       \
                 spike-wide call <ior-file> be|le [--text <s>]... [<unit-hex>]..."
            );
            std::process::ExitCode::from(2)
        }
    }
}

// ── the serving seat ─────────────────────────────────────────────────────────

struct Wide {
    self_ref: Ior,
    registry: Registry,
    calls: u32,
    /// Versions and orders actually seen, so the log says which were on the
    /// wire rather than which were intended.
    seen: std::collections::BTreeSet<(u8, u8, bool)>,
}

impl Wide {
    /// The wide codec for this request: the version it arrived in, and the
    /// wchar codeset its `CodeSets` context declared (UTF-16 when it declared
    /// nothing, which is what our `TAG_CODE_SETS` offers).
    ///
    /// Under GIOP 1.0 there is no such codec: §9.3.1.6 says wchar data in a
    /// 1.0 message is `MARSHAL` with OMG minor 6, so that is the exception —
    /// the servant's to raise, because the framing layer does not know an
    /// argument is a wchar.
    fn codec(req: &Request) -> Result<WideCodec, SystemException> {
        let tcs = req.code_sets().map(|c| c.wchar_data).unwrap_or(CodeSetId::UTF_16);
        WideCodec::new(req.version, tcs).map_err(|_| SystemException {
            id: MARSHAL.into(),
            minor: OMGVMCID | 6,
            completed: Completion::No,
        })
    }
}

impl Dispatch for Wide {
    fn dispatch(&mut self, req: &Request, out: &mut Encoder) -> Result<(), SystemException> {
        self.calls += 1;
        if self.seen.insert((req.version.major, req.version.minor, req.endian == Endian::Little)) {
            println!("first request at {} ({:?})", req.version, req.endian);
        }
        if ObjectOps::handles(&req.operation) {
            println!(
                "served {} #{} at {} ({:?})",
                req.operation, self.calls, req.version, req.endian
            );
            return ObjectOps {
                registry: &self.registry,
                type_id: TYPE_ID,
                reference: Some(&self.self_ref),
            }
            .dispatch(req, out);
        }
        let mut args = req.body().map_err(|_| SystemException::marshal())?;
        match req.operation.as_str() {
            "echo_wchar" => {
                let w = Self::codec(req).inspect_err(|_| {
                    println!(
                        "refused echo_wchar #{} at {} ({:?}): wchar is not legal at this version -> MARSHAL",
                        self.calls, req.version, req.endian
                    );
                })?;
                let c = w.get_wchar(&mut args).map_err(|e| {
                    println!(
                        "refused echo_wchar #{} at {} ({:?}): {e} -> MARSHAL",
                        self.calls, req.version, req.endian
                    );
                    SystemException::marshal()
                })?;
                println!(
                    "served echo_wchar #{} at {} ({:?}) {}: decoded U+{:04X}",
                    self.calls,
                    req.version,
                    req.endian,
                    w.codeset(),
                    c as u32
                );
                w.put_wchar(out, c).map_err(|_| SystemException::marshal())?;
            }
            "echo_wstring" => {
                let w = Self::codec(req).inspect_err(|_| {
                    println!(
                        "refused echo_wstring #{} at {} ({:?}): wchar is not legal at this version -> MARSHAL",
                        self.calls, req.version, req.endian
                    );
                })?;
                let s = w.get_wstring(&mut args).map_err(|e| {
                    println!(
                        "refused echo_wstring #{} at {} ({:?}): {e} -> MARSHAL",
                        self.calls, req.version, req.endian
                    );
                    SystemException::marshal()
                })?;
                println!(
                    "served echo_wstring #{} at {} ({:?}) {}: decoded {} unit(s) {}",
                    self.calls,
                    req.version,
                    req.endian,
                    w.codeset(),
                    s.encode_utf16().count(),
                    units_of(&s)
                );
                w.put_wstring(out, &s).map_err(|_| SystemException::marshal())?;
            }
            _ => return Err(SystemException::bad_operation()),
        }
        Ok(())
    }
}

/// `U+XXXX` per UTF-16 unit — the same view JacORB's user has of a string.
fn units_of(s: &str) -> String {
    s.encode_utf16().map(|u| format!("U+{u:04X}")).collect::<Vec<_>>().join(" ")
}

fn serve(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let out_path = args.first().cloned().unwrap_or_else(|| "spikes/wide.ior".into());
    let host = args.get(1).cloned().unwrap_or_else(|| "127.0.0.1".into());
    let port = args.get(2).cloned().unwrap_or_else(|| "0".into());

    let server = Server::bind(&format!("127.0.0.1:{port}"), OBJECT_KEY.to_vec())?;
    let bound = server.local_addr()?;
    let ior = server.ior(TYPE_ID, &host)?;
    std::fs::write(&out_path, ior.to_stringified()?)?;
    println!("listening on {bound}, publishing {host}:{}", bound.port());
    println!("IOR written to {out_path}");
    println!("READY");

    // `_is_a` is answered from the contract itself, as spike-server does.
    let registry: Registry = orbweaver_registry::registry_from_files(
        &["spikes/wide.idl"],
        &orbweaver_idl::SearchPath::new(),
        Strictness::Grammar,
    )?;
    let mut wide = Wide { self_ref: ior, registry, calls: 0, seen: Default::default() };
    server.serve(&mut wide, || false)?;
    Ok(())
}

// ── the calling seat ─────────────────────────────────────────────────────────

fn call(args: &[String]) -> std::process::ExitCode {
    let mut it = args.iter();
    let (Some(path), Some(order)) = (it.next(), it.next()) else {
        eprintln!("usage: spike-wide call <ior-file> be|le [--text <s>]... [<unit-hex>]...");
        return std::process::ExitCode::from(2);
    };
    let endian = match order.as_str() {
        "be" => Endian::Big,
        "le" => Endian::Little,
        other => {
            eprintln!("byte order must be be or le, not {other}");
            return std::process::ExitCode::from(2);
        }
    };
    let mut units: Vec<u32> = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    while let Some(a) = it.next() {
        if a == "--text" {
            match it.next() {
                Some(t) => texts.push(t.clone()),
                None => {
                    eprintln!("--text needs a value");
                    return std::process::ExitCode::from(2);
                }
            }
        } else {
            match u32::from_str_radix(a, 16) {
                Ok(u) => units.push(u),
                Err(_) => {
                    eprintln!("{a}: not a hex code unit");
                    return std::process::ExitCode::from(2);
                }
            }
        }
    }
    let ior_text = match std::fs::read_to_string(path) {
        Ok(s) => s.trim().to_owned(),
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    match run(&ior_text, endian, &units, &texts) {
        Ok(0) => {
            println!("\nwide: PASS");
            std::process::ExitCode::SUCCESS
        }
        Ok(n) => {
            println!("\nwide: FAIL — {n} case(s)");
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            println!("\nwide: FAIL — {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(ior_text: &str, endian: Endian, units: &[u32], texts: &[String]) -> Result<u32, Error> {
    let ior = Ior::parse(ior_text)?;
    let p = ior.primary()?;
    println!("target");
    println!("  type_id    {}", ior.type_id);
    println!("  endpoint   {}:{}  (IIOP {}.{})", p.host, p.port, p.version.major, p.version.minor);
    println!("  object_key {} bytes", p.object_key.len());

    let mut conn = Connection::connect(&ior, Duration::from_secs(5))?;
    conn.set_endian(endian);
    let version = conn.version();
    let label = match endian {
        Endian::Big => "big-endian",
        Endian::Little => "little-endian",
    };
    // The version is the profile's, never asked for here; the tap's headers
    // are what the script asserts against, this line is what it correlates.
    println!("── {label} client, {version} ──");

    let mut fails = 0u32;
    let codec = WideCodec::new(version, CodeSetId::UTF_16);

    for &u in units {
        let label = format!("echo_wchar[U+{u:04X}]");
        let Some(c) = char::from_u32(u) else {
            // Behaviour, not a verdict: a lone surrogate is not a `char`, so
            // our writer has nothing to encode. What our *reader* does with
            // one a peer sends is pinned in tests/wide_1_1_from_a_peer.rs.
            println!(
                "  info {label}: not a character — our writer cannot ask for it (JacORB's char can)"
            );
            continue;
        };
        let w = match codec {
            Ok(w) => w,
            Err(_) => {
                // GIOP 1.0: wchar is illegal (§9.3.1.6, MARSHAL minor 6). Our
                // codec refuses before the wire; the two octets are then sent
                // raw so the server's refusal is on the wire as well.
                println!(
                    "  info {label}: wchar is illegal under {version}; our codec refuses to write it"
                );
                match conn.invoke("echo_wchar", |e| e.put_u16(u as u16)) {
                    Err(Error::SystemException { id, minor, .. }) if id.contains("MARSHAL") => {
                        let minor = match omg_minor(minor) {
                            Some(m) => format!("OMG minor {m}"),
                            None => format!("minor {minor}"),
                        };
                        println!(
                            "  {OK} {label} sent as two raw octets under {version} -> the server refused it: MARSHAL ({minor})"
                        );
                    }
                    Err(e) => {
                        println!("  {NO} {label} raw under {version}: expected MARSHAL, got {e}");
                        fails += 1;
                    }
                    Ok(r) => {
                        println!(
                            "  {NO} {label} raw under {version}: the server answered (status {:?}) instead of refusing",
                            r.status
                        );
                        fails += 1;
                    }
                }
                continue;
            }
        };
        match conn.invoke("echo_wchar", move |e| {
            // A `char` that is not one UTF-16 unit is refused by the writer;
            // every unit this program can be given is in the BMP.
            let _ = w.put_wchar(e, c);
        }) {
            Ok(r) => {
                let id = r.request_id;
                let (rv, re) = (r.version, r.endian);
                match r.body().and_then(|mut b| {
                    let got = w
                        .get_wchar(&mut b)
                        .map_err(|_| Error::Cdr(orbweaver_cdr::Error::Malformed("wchar")))?;
                    Ok((got, b.remaining()))
                }) {
                    Ok((got, 0)) if got == c => {
                        println!(
                            "  {OK} {label} -> U+{:04X}  (reply id={id} {rv} {re:?})",
                            got as u32
                        )
                    }
                    Ok((got, rest)) => {
                        println!(
                            "  {NO} {label} -> U+{:04X}, {rest} octet(s) left  (reply id={id} {rv} {re:?})",
                            got as u32
                        );
                        fails += 1;
                    }
                    Err(e) => {
                        println!("  {NO} {label}: reply would not decode: {e}  (reply id={id})");
                        fails += 1;
                    }
                }
            }
            Err(e) => {
                println!("  {NO} {label}: {e}");
                fails += 1;
            }
        }
    }

    for text in texts {
        let n = text.encode_utf16().count();
        let label = format!("echo_wstring[{n} units {text:?}]");
        let w = match codec {
            Ok(w) => w,
            Err(_) => {
                println!(
                    "  info {label}: wstring is illegal under {version}; our codec refuses to write it — not sent"
                );
                continue;
            }
        };
        let sent = text.clone();
        match conn.invoke("echo_wstring", move |e| {
            let _ = w.put_wstring(e, &sent);
        }) {
            Ok(r) => {
                let id = r.request_id;
                let (rv, re) = (r.version, r.endian);
                match r.body().and_then(|mut b| {
                    let got = w
                        .get_wstring(&mut b)
                        .map_err(|_| Error::Cdr(orbweaver_cdr::Error::Malformed("wstring")))?;
                    Ok((got, b.remaining()))
                }) {
                    Ok((got, 0)) if got == *text => {
                        println!(
                            "  {OK} {label} -> the same {n} units  (reply id={id} {rv} {re:?})"
                        )
                    }
                    Ok((got, rest)) => {
                        println!(
                            "  {NO} {label} -> {} ({rest} octet(s) left)  (reply id={id} {rv} {re:?})",
                            units_of(&got)
                        );
                        fails += 1;
                    }
                    Err(e) => {
                        println!("  {NO} {label}: reply would not decode: {e}  (reply id={id})");
                        fails += 1;
                    }
                }
            }
            Err(e) => {
                println!("  {NO} {label}: {e}");
                fails += 1;
            }
        }
    }
    Ok(fails)
}
