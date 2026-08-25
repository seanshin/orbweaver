//! Object-reference acquisition: `corbaloc:`, `corbaname:` and a CosNaming
//! client.
//!
//! Until now a target could only be found by reading a stringified IOR out of
//! a file. That is fine for a spike and is not how anything is deployed: real
//! systems publish into a naming service and hand out a URL. The dynamic
//! invoker cannot look a target up in a catalogue without this.
//!
//! # The defaults are the trap
//!
//! `corbaloc::host/Key` — with an empty protocol token and no port — is legal
//! and extremely common. §7.6.10.3 then fills in **IIOP 1.0** and **port
//! 2809**. Both matter: assuming 1.2 sends a header the peer cannot parse
//! (Batch 1, cause C1), and assuming the wrong port dials nothing at all.
//!
//! Spec: OMG CORBA 3.4 Part 2, §7.6.10 (object URLs), and the CosNaming
//! service definition.

use orbweaver_cdr::{Decoder, Encoder, Endian};

use crate::{Connection, Error, IiopProfile, Ior, Result, Version};
use std::time::Duration;

/// Port assumed when a `corbaloc:` address omits one (§7.6.10.3).
pub const DEFAULT_CORBALOC_PORT: u16 = 2809;

/// Repository id of the naming context interface.
pub const NAMING_CONTEXT_EXT_ID: &str = "IDL:omg.org/CosNaming/NamingContextExt:1.0";

/// One element of a CosNaming path.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NameComponent {
    /// The identifier part.
    pub id: String,
    /// The kind part, frequently empty.
    pub kind: String,
}

impl NameComponent {
    /// A component with an empty kind.
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), kind: String::new() }
    }
}

/// A parsed `corbaloc:` or `corbaname:` URL.
#[derive(Debug, Clone, PartialEq)]
pub enum ObjectUrl {
    /// Direct addressing: one or more endpoints plus an object key.
    Corbaloc {
        /// Endpoints to try, in order.
        addresses: Vec<IiopAddress>,
        /// Object key, already unescaped.
        object_key: Vec<u8>,
    },
    /// `corbaloc:rir:/Name` — resolve through initial references locally.
    InitialReference(String),
    /// `corbaname:` — a naming service address plus a path to resolve in it.
    Corbaname {
        /// Where the naming service lives.
        addresses: Vec<IiopAddress>,
        /// Object key of the naming context, usually `NameService`.
        object_key: Vec<u8>,
        /// The path to resolve inside it.
        name: Vec<NameComponent>,
    },
}

/// One endpoint from an address list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IiopAddress {
    /// GIOP version the peer claims to speak. Defaults to 1.0 (§7.6.10.3).
    pub version: Version,
    /// Host, as written.
    pub host: String,
    /// Port, defaulting to 2809.
    pub port: u16,
}

/// Why a URL could not be parsed.
///
/// The variants mirror the `BAD_PARAM` minor codes §7.6.10.3 assigns, so a
/// diagnostic can name the same failure the specification does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlError {
    /// Not a scheme this parser handles. `BAD_PARAM` minor 7.
    BadSchemeName(String),
    /// The address portion is malformed. `BAD_PARAM` minor 8.
    BadAddress(String),
    /// The whole URL is malformed. `BAD_PARAM` minor 9.
    BadSchemeSpecificPart(String),
    /// Some other structural problem. `BAD_PARAM` minor 10.
    Other(String),
}

impl std::fmt::Display for UrlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (code, what) = match self {
            UrlError::BadSchemeName(s) => (7, s),
            UrlError::BadAddress(s) => (8, s),
            UrlError::BadSchemeSpecificPart(s) => (9, s),
            UrlError::Other(s) => (10, s),
        };
        write!(f, "{what} (BAD_PARAM minor {code})")
    }
}

impl std::error::Error for UrlError {}

impl ObjectUrl {
    /// Parses a `corbaloc:` or `corbaname:` URL.
    pub fn parse(url: &str) -> std::result::Result<Self, UrlError> {
        let url = url.trim();
        if let Some(rest) = strip_ci(url, "corbaloc:") {
            if let Some(name) = strip_ci(rest, "rir:") {
                // §7.6.10.3: an empty key after rir: means NameService.
                let key = name.strip_prefix('/').unwrap_or(name);
                let key = if key.is_empty() { "NameService" } else { key };
                return Ok(ObjectUrl::InitialReference(key.to_owned()));
            }
            let (addr_part, key_part) = split_key(rest);
            let addresses = parse_addresses(addr_part)?;
            return Ok(ObjectUrl::Corbaloc { addresses, object_key: unescape(key_part)? });
        }
        if let Some(rest) = strip_ci(url, "corbaname:") {
            // §7.6.10.5: everything after '#' is a stringified name.
            let (loc, name_part) = match rest.split_once('#') {
                Some((l, n)) => (l, n),
                None => (rest, ""),
            };
            let (addr_part, key_part) = split_key(loc);
            let addresses = parse_addresses(addr_part)?;
            let key =
                if key_part.is_empty() { b"NameService".to_vec() } else { unescape(key_part)? };
            return Ok(ObjectUrl::Corbaname {
                addresses,
                object_key: key,
                name: parse_stringified_name(&unescape_name(name_part)?)?,
            });
        }
        Err(UrlError::BadSchemeName(format!("{url:?} is not corbaloc: or corbaname:")))
    }

    /// Builds an IOR for the cases that address an object directly.
    ///
    /// Every address becomes a profile, so the existing multi-profile handling
    /// covers a comma-separated list without a second mechanism.
    ///
    /// # Why `corbaloc:rir:` is still `None` here, and where it is answered
    ///
    /// The `None` below used to be the end of the road: nothing in the
    /// workspace could turn a well-known name into a reference, so this
    /// function was *the place the case failed*. It is not any more —
    /// [`crate::orb::Orb::resolve_url`] answers all three forms — and the
    /// choice between putting the table here and putting it there was made
    /// deliberately, so here is the reason.
    ///
    /// **The caller resolves first; this function does not gain the table.**
    /// The answer to `corbaloc:rir:NameService` is not in the URL — that is the
    /// entire difference between the three variants, and it is why
    /// [`ObjectUrl::Corbaloc`] and [`ObjectUrl::Corbaname`] work: they carry an
    /// address. Handing a table to this function would make a pure conversion
    /// depend on ORB state that every one of its callers would then have to
    /// obtain and thread through. A table parameter would also put a lookup
    /// behind a name that says *convert*.
    ///
    /// **The call sites, counted 2026-08-25 rather than remembered.** Outside
    /// tests there are exactly two, both in this file, and both already handle
    /// the `None`: [`NamingContext::from_url`] and [`corbaloc_to_ior_string`].
    /// One test `.expect()`s it — `tests/codesets_on_the_wire.rs:185`, on an
    /// addressed URL. D019 §8 attributes six `.unwrap()`s in [`crate::nat`] to
    /// this function; those are on `nat::RawIor::to_ior`, a different function
    /// with a different signature that never sees an [`ObjectUrl`]. Nothing
    /// that passes a `rir:` URL today exists, so nothing can begin to panic on
    /// a case that used to answer `None`.
    ///
    /// So the `Option` stays, and it stops meaning *"unanswerable"*: it means
    /// *"this form's answer belongs to the ORB, ask [`crate::orb::Orb`]"*. This
    /// function remains where the case is **recognised**; it is no longer where
    /// the case **ends**.
    pub fn to_ior(&self, type_id: &str) -> Option<Ior> {
        match self {
            ObjectUrl::Corbaloc { addresses, object_key }
            | ObjectUrl::Corbaname { addresses, object_key, .. } => {
                Some(addressed_ior(addresses, object_key, type_id))
            }
            ObjectUrl::InitialReference(_) => None,
        }
    }
}

/// Builds the multi-profile IOR an addressed URL denotes.
///
/// Shared by [`ObjectUrl::to_ior`] and [`crate::orb::Orb::resolve_url`] so that
/// the two entry points cannot construct different references from the same
/// URL — the ORB's extra form is an extra *case*, never a second conversion.
pub(crate) fn addressed_ior(addresses: &[IiopAddress], object_key: &[u8], type_id: &str) -> Ior {
    Ior {
        type_id: type_id.to_owned(),
        profiles: addresses
            .iter()
            .map(|a| IiopProfile {
                version: a.version,
                host: a.host.clone(),
                port: a.port,
                object_key: object_key.to_vec(),
                components: Vec::new(),
            })
            .collect(),
    }
}

fn strip_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// Splits an address list from the key, on the first `/` that is not inside
/// an IPv6 literal's brackets.
fn split_key(s: &str) -> (&str, &str) {
    let mut in_brackets = false;
    for (i, c) in s.char_indices() {
        match c {
            '[' => in_brackets = true,
            ']' => in_brackets = false,
            '/' if !in_brackets => return (&s[..i], &s[i + 1..]),
            _ => {}
        }
    }
    (s, "")
}

fn parse_addresses(s: &str) -> std::result::Result<Vec<IiopAddress>, UrlError> {
    if s.is_empty() {
        return Err(UrlError::BadAddress("no address in URL".into()));
    }
    s.split(',').map(parse_address).collect()
}

fn parse_address(s: &str) -> std::result::Result<IiopAddress, UrlError> {
    // The protocol token is either "iiop:" or the bare ":" shorthand, and the
    // shorthand is what most deployments actually write.
    let rest = if let Some(r) = strip_ci(s, "iiop:") {
        r
    } else if let Some(r) = s.strip_prefix(':') {
        r
    } else {
        return Err(UrlError::BadAddress(format!("{s:?} has no iiop: or : protocol token")));
    };

    // Optional "major.minor@" version prefix.
    let (version, rest) = match rest.split_once('@') {
        Some((v, r)) => {
            let (maj, min) = v
                .split_once('.')
                .ok_or_else(|| UrlError::BadAddress(format!("malformed version {v:?}")))?;
            let parse = |x: &str| {
                x.parse::<u8>()
                    .map_err(|_| UrlError::BadAddress(format!("malformed version {v:?}")))
            };
            (Version { major: parse(maj)?, minor: parse(min)? }, r)
        }
        // §7.6.10.3: "If the version is absent, 1.0 is assumed."
        None => (Version::V1_0, rest),
    };

    // IPv6 literals are bracketed so their colons cannot be read as a port.
    let (host, port) = if let Some(rest6) = rest.strip_prefix('[') {
        let (h, tail) = rest6
            .split_once(']')
            .ok_or_else(|| UrlError::BadAddress(format!("unterminated IPv6 literal in {s:?}")))?;
        (h.to_owned(), tail.strip_prefix(':').unwrap_or(""))
    } else {
        match rest.split_once(':') {
            Some((h, p)) => (h.to_owned(), p),
            None => (rest.to_owned(), ""),
        }
    };

    if host.is_empty() {
        return Err(UrlError::BadAddress(format!("no host in {s:?}")));
    }
    let port = if port.is_empty() {
        DEFAULT_CORBALOC_PORT
    } else {
        port.parse().map_err(|_| UrlError::BadAddress(format!("malformed port in {s:?}")))?
    };
    Ok(IiopAddress { version, host, port })
}

/// Decodes RFC 2396 `%XX` escapes into raw object-key bytes.
///
/// The key is bytes, not text: it is opaque server state and may hold anything.
fn unescape(s: &str) -> std::result::Result<Vec<u8>, UrlError> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if i + 2 >= b.len() {
                return Err(UrlError::BadSchemeSpecificPart("truncated % escape".into()));
            }
            let hex = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| UrlError::BadSchemeSpecificPart("malformed % escape".into()))?;
            out.push(hex);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Ok(out)
}

/// Parses a stringified CosNaming name: `id.kind/id.kind/...`.
///
/// `\` escapes a literal `.`, `/` or `\`, which is the only way a name
/// component can contain one.
pub fn parse_stringified_name(s: &str) -> std::result::Result<Vec<NameComponent>, UrlError> {
    if s.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut id = String::new();
    let mut kind = String::new();
    let mut in_kind = false;
    let mut escaped = false;

    for c in s.chars() {
        if escaped {
            if in_kind {
                kind.push(c)
            } else {
                id.push(c)
            }
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '.' if !in_kind => in_kind = true,
            '/' => {
                out.push(NameComponent {
                    id: std::mem::take(&mut id),
                    kind: std::mem::take(&mut kind),
                });
                in_kind = false;
            }
            _ => {
                if in_kind {
                    kind.push(c)
                } else {
                    id.push(c)
                }
            }
        }
    }
    if escaped {
        return Err(UrlError::BadSchemeSpecificPart("name ends in a trailing backslash".into()));
    }
    out.push(NameComponent { id, kind });
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// corbaname: URL construction — the inverse of the parser above
// ─────────────────────────────────────────────────────────────────────────────

/// Whether `b` may stand for itself in the stringified-name part of a
/// `corbaname:` URL.
///
/// RFC 2396's `unreserved` set (alphanumerics plus the `mark` characters) and
/// the `reserved` characters a URL path may carry literally. Two of those
/// reserved characters carry meaning for the *name* grammar as well — `/`
/// separates components and `.` separates an id from its kind — and leaving
/// them unescaped is what makes the two layers compose: the name grammar's own
/// `\` escape is what hides a `/` inside a component, and `\` is **not** in
/// this set, so it survives as `%5C`.
///
/// Everything else is escaped: the space and the control characters (no URL
/// carries them), `#` (which would end the address part early), `%` (which
/// would be read as somebody else's escape) and every byte above 0x7F (so a
/// non-ASCII name travels as its UTF-8 bytes rather than as whatever the peer
/// guesses).
fn is_url_name_safe(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || matches!(
            b,
            // unreserved marks
            b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')'
            // reserved, legal in a path and meaningful to the name grammar
            | b';' | b'/' | b':' | b'?' | b'@' | b'&' | b'=' | b'+' | b'$' | b','
        )
}

/// Percent-escapes a stringified name for the fragment of a `corbaname:` URL
/// (§2.5.3.3).
fn escape_name(s: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if is_url_name_safe(b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[usize::from(b >> 4)] as char);
            out.push(HEX[usize::from(b & 0x0f)] as char);
        }
    }
    out
}

/// The inverse of [`escape_name`]: decodes `%XX` back into text.
///
/// The URL layer sits **above** the name grammar, so this runs first and
/// [`parse_stringified_name`] runs on the result. One consequence worth
/// stating: a `%2F` written by some other producer decodes to a `/` and is
/// then read as a component separator. That is the ordering the layering
/// implies, and [`to_url`] never emits one — it leaves `/` literal, because
/// the name grammar's own `\/` is how a component hides a slash.
fn unescape_name(s: &str) -> std::result::Result<String, UrlError> {
    let bytes = unescape(s)?;
    String::from_utf8(bytes).map_err(|_| {
        UrlError::BadSchemeSpecificPart(
            "the escaped stringified name is not valid UTF-8 once decoded".into(),
        )
    })
}

/// `NamingContextExt::to_url` — builds a `corbaname:` URL out of an address
/// and a stringified name (§2.5.3.3).
///
/// `address` is a `corbaloc:` address list *without* the scheme —
/// `:host`, `:host:2809`, `iiop:1.2@host:2809`, optionally with `/ObjectKey`
/// and optionally comma-separated. `name` is a stringified name in the grammar
/// [`stringify_name`] emits and [`parse_stringified_name`] reads.
///
/// # The parser is the specification
///
/// A `to_url` whose output our own parser rejects would be worse than no
/// `to_url` at all, so this does not merely trust its escaping: it parses what
/// it built and refuses to hand back a URL that does not read as the name it
/// was given. The round trip is therefore an invariant of the function and not
/// only of a test.
///
/// # Errors
///
/// Exactly two kinds, so a servant can map them onto the two exceptions the
/// operation declares:
///
/// - [`UrlError::BadAddress`] — `InvalidAddress`. An empty address, one our
///   own `corbaloc:` address parser refuses, or one carrying a byte that
///   cannot stand in a URL (space, control, `#`, non-ASCII).
/// - [`UrlError::BadSchemeSpecificPart`] — `InvalidName`. A name the name
///   grammar refuses.
/// - [`UrlError::Other`] — the round-trip check above failed, which is a
///   defect in this function rather than in either argument.
///
/// # Two places this was measured against omniNames, 2026-08-14
///
/// omniNames' own `to_url` was driven with the same arguments through
/// omniORB's python client and agreed on every URL it produced, bar the
/// **case of the hex digits** — it writes `%5c`, this writes `%5C`. Both are
/// legal (RFC 2396 calls hex digits case-insensitive; RFC 3986 recommends
/// upper), each parser reads the other's, and
/// `a_foreign_escaped_corbaname_decodes_the_same_way` pins that we read
/// theirs.
///
/// It also disagreed twice, and both are decisions rather than accidents:
///
/// - **An empty name.** omniNames returns the bare `corbaname:<addr>` — a URL
///   naming the context itself — and this does too, because our parser reads
///   that URL back as the empty name it was given. An empty *`Name`* is still
///   `InvalidName` everywhere it is a name: `to_name`, `to_string` and
///   `resolve` all refuse it, and none of them is producing a URL.
/// - **`rir:`.** omniNames returns `corbaname:rir:#a`; this refuses with
///   `InvalidAddress`, because [`ObjectUrl::parse`] accepts `rir:` under
///   `corbaloc:` only, so emitting one would hand back a URL our own parser
///   rejects — the single thing this function exists not to do. Accepting it
///   means giving `ObjectUrl::Corbaname` an address-less form, which nothing
///   in this repository can resolve through yet.
pub fn to_url(address: &str, name: &str) -> std::result::Result<String, UrlError> {
    if address.is_empty() {
        return Err(UrlError::BadAddress("no address given".into()));
    }
    if let Some(bad) = address.bytes().find(|&b| b <= b' ' || b == 0x7f || b >= 0x80 || b == b'#') {
        return Err(UrlError::BadAddress(format!(
            "{address:?} carries a byte that cannot stand in a URL: 0x{bad:02X}"
        )));
    }
    // The same reading of the address the parser will make, made now so the
    // failure is `InvalidAddress` rather than a URL nobody can use.
    let (addr_part, _key_part) = split_key(address);
    parse_addresses(addr_part)?;

    let intended = if name.is_empty() { Vec::new() } else { parse_stringified_name(name)? };
    let url = if name.is_empty() {
        format!("corbaname:{address}")
    } else {
        format!("corbaname:{address}#{}", escape_name(name))
    };
    match ObjectUrl::parse(&url) {
        Ok(ObjectUrl::Corbaname { name: read_back, .. }) if read_back == intended => Ok(url),
        other => Err(UrlError::Other(format!(
            "built {url:?} but our own parser read it as {other:?}, not as {intended:?}"
        ))),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CosNaming client
// ─────────────────────────────────────────────────────────────────────────────

/// A client for a CosNaming context.
#[derive(Debug)]
pub struct NamingContext {
    conn: Connection,
}

impl NamingContext {
    /// Connects to a naming context named by an IOR.
    pub fn connect(ior: &Ior, timeout: Duration) -> Result<Self> {
        Ok(Self { conn: Connection::connect(ior, timeout)? })
    }

    /// Connects to the naming context a `corbaloc:`/`corbaname:` URL addresses.
    ///
    /// A `corbaloc:rir:` URL addresses nothing dialable, so it is refused here
    /// rather than guessed at: resolve it through
    /// [`crate::orb::Orb::resolve_url`] first and connect to what comes back.
    pub fn from_url(url: &ObjectUrl, timeout: Duration) -> Result<Self> {
        let ior = url.to_ior(NAMING_CONTEXT_EXT_ID).ok_or(Error::BadIor(
            "corbaloc:rir: names an initial reference; resolve it through the ORB's table first",
        ))?;
        Self::connect(&ior, timeout)
    }

    /// `NamingContext::resolve` — looks a name up and returns its reference.
    pub fn resolve(&mut self, name: &[NameComponent]) -> Result<Ior> {
        let path = name.to_vec();
        let reply = self.conn.invoke("resolve", move |e| write_name(e, &path))?;
        let mut b = reply.body()?;
        Ior::read_from(&mut b)
    }

    /// `NamingContextExt::resolve_str` — the same, from a stringified name.
    ///
    /// Parsing locally and calling `resolve` would also work, and would be one
    /// fewer interface to depend on; this exists because a peer may accept only
    /// the `Ext` form for names its own escaping produced.
    pub fn resolve_str(&mut self, name: &str) -> Result<Ior> {
        let owned = name.to_owned();
        let reply = self.conn.invoke("resolve_str", move |e| e.put_str(&owned))?;
        let mut b = reply.body()?;
        Ior::read_from(&mut b)
    }

    /// `NamingContextExt::to_url` — asks the *peer* to build a `corbaname:`
    /// URL out of `address` and the stringified `name`.
    ///
    /// [`to_url`] does the same thing locally and needs no round trip. This
    /// exists for the direction that cannot be done locally: asking a foreign
    /// naming service to produce the URL form *it* would hand out, which is
    /// the only way to find out whether its escaping and ours agree.
    pub fn to_url(&mut self, address: &str, name: &str) -> Result<String> {
        let (address, name) = (address.to_owned(), name.to_owned());
        let reply = self.conn.invoke("to_url", move |e| {
            e.put_str(&address);
            e.put_str(&name);
        })?;
        Ok(reply.body()?.get_string()?)
    }

    /// `NamingContext::bind` — publishes `obj` under `name`.
    ///
    /// Never overwrites: a taken name raises `AlreadyBound`, surfaced as
    /// [`Error::UserException`]. Overwriting is [`NamingContext::rebind`].
    pub fn bind(&mut self, name: &[NameComponent], obj: &Ior) -> Result<()> {
        let path = name.to_vec();
        let obj = obj.clone();
        self.conn.invoke("bind", move |e| {
            write_name(e, &path);
            // A marshalling failure poisons `e` and surfaces from the invoke.
            let _ = obj.write_to(e);
        })?;
        Ok(())
    }

    /// `NamingContext::rebind` — as [`NamingContext::bind`], but replaces an
    /// existing *object* binding. A name bound to a context raises `NotFound`
    /// with `why = not_object`; replacing contexts is `rebind_context`'s job.
    pub fn rebind(&mut self, name: &[NameComponent], obj: &Ior) -> Result<()> {
        let path = name.to_vec();
        let obj = obj.clone();
        self.conn.invoke("rebind", move |e| {
            write_name(e, &path);
            let _ = obj.write_to(e);
        })?;
        Ok(())
    }

    /// `NamingContext::unbind` — removes the binding under `name`.
    pub fn unbind(&mut self, name: &[NameComponent]) -> Result<()> {
        let path = name.to_vec();
        self.conn.invoke("unbind", move |e| write_name(e, &path))?;
        Ok(())
    }

    /// `NamingContext::bind_new_context` — creates a context, binds it under
    /// `name`, and returns its reference.
    pub fn bind_new_context(&mut self, name: &[NameComponent]) -> Result<Ior> {
        let path = name.to_vec();
        let reply = self.conn.invoke("bind_new_context", move |e| write_name(e, &path))?;
        let mut b = reply.body()?;
        Ior::read_from(&mut b)
    }

    /// `NamingContext::list` — up to `how_many` bindings, plus the
    /// `BindingIterator` reference holding the remainder. A nil iterator
    /// (check with [`Ior::is_nil`]) means the server will report no more.
    pub fn list(&mut self, how_many: u32) -> Result<(Vec<Binding>, Ior)> {
        let reply = self.conn.invoke("list", move |e| e.put_u32(how_many))?;
        let mut b = reply.body()?;
        let n = b.get_u32()?;
        // Each binding costs at least a name length and a binding type.
        let n = b.validate_count(n, 8)?;
        let mut bindings = Vec::with_capacity(n);
        for _ in 0..n {
            let name = read_name(&mut b)?;
            let binding_type = b.get_u32()?;
            bindings.push(Binding { name, is_context: binding_type == 1 });
        }
        let iterator = Ior::read_from(&mut b)?;
        Ok((bindings, iterator))
    }

    /// The underlying connection, for callers that need its knobs.
    pub fn connection(&mut self) -> &mut Connection {
        &mut self.conn
    }
}

/// One entry reported by `NamingContext::list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    /// The binding's name, relative to the listed context.
    pub name: Vec<NameComponent>,
    /// `true` for `BindingType::ncontext`, `false` for `nobject`.
    pub is_context: bool,
}

/// Marshals a CosNaming `Name`: a sequence of `NameComponent`, each an `id`
/// string followed by a `kind` string.
///
/// This and [`read_name`] are the only places that know the wire shape of a
/// `Name` — the client, the server and the spikes all call them, so the two
/// halves cannot drift apart (the Phase 3 `wstring` lesson).
pub fn write_name(e: &mut Encoder, name: &[NameComponent]) {
    e.put_u32(name.len() as u32);
    for c in name {
        e.put_str(&c.id);
        e.put_str(&c.kind);
    }
}

/// Unmarshals a CosNaming `Name` — the inverse of [`write_name`].
pub fn read_name(d: &mut Decoder<'_>) -> Result<Vec<NameComponent>> {
    let n = d.get_u32()?;
    // Each component costs at least two 4-byte string lengths.
    let n = d.validate_count(n, 8)?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let id = String::from_utf8_lossy(d.get_string_bytes()?).into_owned();
        let kind = String::from_utf8_lossy(d.get_string_bytes()?).into_owned();
        out.push(NameComponent { id, kind });
    }
    Ok(out)
}

/// Emits the stringified form of a name, escaping the separators.
pub fn stringify_name(name: &[NameComponent]) -> String {
    fn esc(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            if matches!(c, '.' | '/' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out
    }
    name.iter()
        .map(|c| {
            if c.kind.is_empty() { esc(&c.id) } else { format!("{}.{}", esc(&c.id), esc(&c.kind)) }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Builds a stringified IOR for a `corbaloc:` URL, for tools that want one.
pub fn corbaloc_to_ior_string(url: &str, type_id: &str) -> Result<String> {
    let parsed = ObjectUrl::parse(url).map_err(|_| Error::BadIor("malformed corbaloc URL"))?;
    let ior = parsed
        .to_ior(type_id)
        .ok_or(Error::BadIor("this URL does not address an object directly"))?;
    ior.to_stringified()
}

/// The byte order used when a URL is turned into an IOR.
pub const URL_IOR_ENDIAN: Endian = Endian::Little;

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(url: &str) -> ObjectUrl {
        ObjectUrl::parse(url).unwrap_or_else(|e| panic!("{url} -> {e}"))
    }

    /// The bare `:` shorthand with no port is the common real-world form, and
    /// the defaults it implies are not the ones a modern reader would guess.
    #[test]
    fn defaults_are_giop_1_0_and_port_2809() {
        match loc("corbaloc::example.test/NameService") {
            ObjectUrl::Corbaloc { addresses, object_key } => {
                assert_eq!(addresses.len(), 1);
                assert_eq!(addresses[0].version, Version::V1_0, "§7.6.10.3 assumes 1.0");
                assert_eq!(addresses[0].port, DEFAULT_CORBALOC_PORT);
                assert_eq!(addresses[0].host, "example.test");
                assert_eq!(object_key, b"NameService");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn explicit_protocol_version_and_port_are_honoured() {
        match loc("corbaloc:iiop:1.2@10.0.0.1:9999/Echo") {
            ObjectUrl::Corbaloc { addresses, object_key } => {
                assert_eq!(addresses[0].version, Version::V1_2);
                assert_eq!(addresses[0].host, "10.0.0.1");
                assert_eq!(addresses[0].port, 9999);
                assert_eq!(object_key, b"Echo");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn scheme_is_case_insensitive() {
        assert!(matches!(loc("CORBALOC::h/K"), ObjectUrl::Corbaloc { .. }));
        assert!(matches!(loc("CorbaLoc:IIOP:h/K"), ObjectUrl::Corbaloc { .. }));
    }

    /// A comma-separated list becomes one profile per address, so multi-profile
    /// handling covers failover without a second mechanism.
    #[test]
    fn address_list_becomes_multiple_profiles() {
        let url = loc("corbaloc::a.test:1111,:b.test:2222,iiop:1.1@c.test/Key");
        let ior = url.to_ior("IDL:x:1.0").unwrap();
        assert_eq!(ior.profiles.len(), 3);
        assert_eq!(ior.profiles[0].port, 1111);
        assert_eq!(ior.profiles[1].host, "b.test");
        assert_eq!(ior.profiles[2].version, Version::V1_1);
        assert_eq!(ior.profiles[2].port, DEFAULT_CORBALOC_PORT);
        for p in &ior.profiles {
            assert_eq!(p.object_key, b"Key");
        }
    }

    /// §7.6.10.1 shows bracketed IPv6. Unbracketed, its colons would be read
    /// as a port separator and the host would be truncated to "".
    #[test]
    fn ipv6_literals_are_bracketed() {
        match loc("corbaloc:iiop:[1080::8:800:200C:417A]:88/Key") {
            ObjectUrl::Corbaloc { addresses, .. } => {
                assert_eq!(addresses[0].host, "1080::8:800:200C:417A");
                assert_eq!(addresses[0].port, 88);
            }
            other => panic!("{other:?}"),
        }
        match loc("corbaloc:iiop:[::1]/Key") {
            ObjectUrl::Corbaloc { addresses, .. } => {
                assert_eq!(addresses[0].host, "::1");
                assert_eq!(addresses[0].port, DEFAULT_CORBALOC_PORT);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn object_key_escapes_are_decoded_to_bytes() {
        match loc("corbaloc::h/a%20b%2Fc%00d") {
            ObjectUrl::Corbaloc { object_key, .. } => {
                assert_eq!(object_key, b"a b/c\0d", "the key is bytes, not text");
            }
            other => panic!("{other:?}"),
        }
    }

    /// # Three forms, and a peer that accepts one of them
    ///
    /// Measured against omniORB 2026-08-25: it reads `corbaloc:rir:/NameService`
    /// and refuses both `corbaloc:rir:NameService` and the bare
    /// `corbaloc:rir:` with `BAD_PARAM(BadURIOther)`. §7.6.10.3's grammar puts
    /// the `/` before the key string, so omniORB is reading the grammar
    /// strictly and this parser is the lenient one — the safe direction, since
    /// the leniency only ever widens what we can *read* and [`to_url`] emits no
    /// `rir:` URL at all (it refuses one, for the reason recorded there). The
    /// leniency is deliberate and pinned here rather than left to be
    /// rediscovered as a divergence.
    #[test]
    fn rir_resolves_locally_and_defaults_to_nameservice() {
        assert_eq!(loc("corbaloc:rir:"), ObjectUrl::InitialReference("NameService".into()));
        assert_eq!(loc("corbaloc:rir:/"), ObjectUrl::InitialReference("NameService".into()));
        assert_eq!(
            loc("corbaloc:rir:/InterfaceRepository"),
            ObjectUrl::InitialReference("InterfaceRepository".into())
        );
        // It addresses nothing dialable, so *this* function must not produce
        // an IOR — the answer is the ORB's table, not the URL's contents. See
        // `to_ior`'s docs and `crate::orb::Orb::resolve_url`.
        assert!(loc("corbaloc:rir:").to_ior("IDL:x:1.0").is_none());
    }

    #[test]
    fn corbaname_splits_the_path_from_the_address() {
        match loc("corbaname::host:2809/NameService#spike/Echo") {
            ObjectUrl::Corbaname { addresses, object_key, name } => {
                assert_eq!(addresses[0].host, "host");
                assert_eq!(object_key, b"NameService");
                assert_eq!(name, vec![NameComponent::new("spike"), NameComponent::new("Echo")]);
            }
            other => panic!("{other:?}"),
        }
        // No '#' means the naming context itself.
        match loc("corbaname::host") {
            ObjectUrl::Corbaname { object_key, name, .. } => {
                assert_eq!(object_key, b"NameService");
                assert!(name.is_empty());
            }
            other => panic!("{other:?}"),
        }
    }

    /// The one wire shape shared by client and server. Both byte orders,
    /// because a name encoder that only works native-endian passes every
    /// local test and fails in the field.
    #[test]
    fn names_round_trip_on_the_wire_in_both_byte_orders() {
        let name = [
            NameComponent { id: "a".into(), kind: "config".into() },
            NameComponent::new("plain"),
            NameComponent { id: "함정".into(), kind: String::new() },
        ];
        for endian in [Endian::Big, Endian::Little] {
            for case in [&name[..], &[]] {
                let mut e = Encoder::new(endian);
                write_name(&mut e, case);
                let bytes = e.finish().unwrap();
                let mut d = Decoder::new(&bytes, endian);
                assert_eq!(read_name(&mut d).unwrap(), case, "{endian:?}");
                assert!(d.is_empty(), "{endian:?}: trailing bytes after the name");
            }
        }
    }

    #[test]
    fn stringified_names_round_trip_with_escapes() {
        let name = vec![
            NameComponent { id: "a.b".into(), kind: "c/d".into() },
            NameComponent { id: "plain".into(), kind: String::new() },
            NameComponent { id: "back\\slash".into(), kind: String::new() },
        ];
        let s = stringify_name(&name);
        assert_eq!(parse_stringified_name(&s).unwrap(), name, "round trip failed for {s:?}");
    }

    #[test]
    fn name_kind_is_optional_and_separated_by_a_dot() {
        assert_eq!(
            parse_stringified_name("ctx/obj.kind").unwrap(),
            vec![
                NameComponent::new("ctx"),
                NameComponent { id: "obj".into(), kind: "kind".into() }
            ]
        );
    }

    /// The claim this operation is worth having: **`parse(to_url(a, n))` is
    /// `n`**, for every name that needs escaping and for both of the two
    /// escape layers at once.
    ///
    /// The cases are chosen to hit each layer and their interaction: `/` and
    /// `.` inside a component (the name grammar's `\` escape, which the URL
    /// layer must then carry as `%5C` without disturbing the separators that
    /// mean what they say), a space and a `%` and a `#` (the URL layer's job
    /// alone — a literal one of any of them makes the URL unparsable or
    /// re-readable as somebody else's escape), and non-ASCII (which must
    /// travel as UTF-8 bytes rather than as a guess).
    #[test]
    fn to_url_round_trips_through_our_own_parser() {
        let names = [
            vec![NameComponent::new("spike"), NameComponent::new("Echo")],
            vec![NameComponent { id: "a/b".into(), kind: "c.d".into() }],
            vec![NameComponent { id: "with space".into(), kind: "and space".into() }],
            vec![NameComponent { id: "100%".into(), kind: "#frag".into() }],
            vec![NameComponent { id: "함정".into(), kind: "한글".into() }],
            vec![NameComponent { id: "back\\slash".into(), kind: String::new() }],
            vec![
                NameComponent::new("ctx"),
                NameComponent { id: "obj".into(), kind: "dev".into() },
                NameComponent { id: "trailing.".into(), kind: String::new() },
            ],
        ];
        let addresses = [
            ":example.test",
            ":example.test:2809",
            "iiop:1.2@127.0.0.1:4001",
            "iiop:1.2@127.0.0.1:4001/NameService",
            ":a.test:1111,:b.test:2222",
            "iiop:[::1]:2809",
        ];
        for address in addresses {
            for name in &names {
                let sn = stringify_name(name);
                let url = to_url(address, &sn).unwrap_or_else(|e| panic!("{address} {sn:?}: {e}"));
                match ObjectUrl::parse(&url) {
                    Ok(ObjectUrl::Corbaname { name: back, .. }) => {
                        assert_eq!(&back, name, "{url}");
                    }
                    other => panic!("{url} parsed as {other:?}"),
                }
            }
        }
    }

    /// The escaping is not merely reversible, it is the *right* escaping: the
    /// characters that would break the URL are gone from the text, and the two
    /// that carry name-grammar meaning are still there to carry it.
    #[test]
    fn to_url_escapes_what_a_url_cannot_carry_and_nothing_else() {
        let url = to_url(":h", &stringify_name(&[NameComponent::new("a b#c%d")])).unwrap();
        assert_eq!(url, "corbaname::h#a%20b%23c%25d");

        // `/` and `.` inside a component: the name grammar backslashes them,
        // and the URL layer escapes only the backslash it added.
        let url =
            to_url(":h", &stringify_name(&[NameComponent { id: "a/b".into(), kind: ".".into() }]))
                .unwrap();
        assert_eq!(url, "corbaname::h#a%5C/b.%5C.");

        // …while separators that mean what they say stay literal.
        let url = to_url(":h", "one/two.kind").unwrap();
        assert_eq!(url, "corbaname::h#one/two.kind");

        // Non-ASCII travels as its UTF-8 bytes.
        assert_eq!(to_url(":h", "함").unwrap(), "corbaname::h#%ED%95%A8");
    }

    /// The two failures map onto the two exceptions the operation declares,
    /// and they are told apart by which argument was wrong.
    #[test]
    fn to_url_refuses_bad_addresses_and_bad_names_distinguishably() {
        for bad in ["", "host-with-no-protocol-token", ":", ":h ost", ":h#x", ":호스트"] {
            assert!(
                matches!(to_url(bad, "a"), Err(UrlError::BadAddress(_))),
                "{bad:?} should be InvalidAddress"
            );
        }
        assert!(
            matches!(to_url(":h", "trailing\\"), Err(UrlError::BadSchemeSpecificPart(_))),
            "a trailing backslash should be InvalidName"
        );
        // Measured against omniNames (see `to_url`'s docs): `rir:` is the one
        // address it accepts and we refuse, because our parser reads `rir:`
        // under `corbaloc:` only and this must not emit what it cannot read.
        assert!(matches!(to_url("rir:", "a"), Err(UrlError::BadAddress(_))));
    }

    /// The empty name is not a failure: it is the URL that names the context
    /// itself, it round-trips, and it is byte-for-byte what omniNames answers.
    #[test]
    fn an_empty_name_gives_the_url_for_the_context_itself() {
        let url = to_url(":h", "").unwrap();
        assert_eq!(url, "corbaname::h", "no fragment at all, as omniNames writes it");
        match ObjectUrl::parse(&url) {
            Ok(ObjectUrl::Corbaname { name, object_key, .. }) => {
                assert!(name.is_empty());
                assert_eq!(object_key, b"NameService");
            }
            other => panic!("{other:?}"),
        }
    }

    /// A URL some other producer escaped is decoded by the same rule, so the
    /// parser reads foreign escaping as well as its own.
    #[test]
    fn a_foreign_escaped_corbaname_decodes_the_same_way() {
        match ObjectUrl::parse("corbaname::h:2809/NameService#a%20b/c%2Ed") {
            Ok(ObjectUrl::Corbaname { name, .. }) => assert_eq!(
                name,
                vec![NameComponent::new("a b"), NameComponent { id: "c".into(), kind: "d".into() }],
                "%2E decodes to '.' and is then the kind separator — the URL layer sits above \
                 the name grammar"
            ),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            ObjectUrl::parse("corbaname::h#%FF"),
            Err(UrlError::BadSchemeSpecificPart(_))
        ));
        // omniNames writes its hex in lower case (`%5c`) where this writes
        // `%5C` — measured 2026-08-14, and the only escaping difference the
        // two producers have. Reading theirs is what makes it a difference
        // rather than an incompatibility.
        match ObjectUrl::parse("corbaname::h#a%5c/b") {
            Ok(ObjectUrl::Corbaname { name, .. }) => {
                assert_eq!(name, vec![NameComponent::new("a/b")]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn malformed_urls_name_their_spec_minor_code() {
        assert!(matches!(ObjectUrl::parse("http://x"), Err(UrlError::BadSchemeName(_))));
        assert!(matches!(ObjectUrl::parse("corbaloc:"), Err(UrlError::BadAddress(_))));
        assert!(matches!(ObjectUrl::parse("corbaloc::/K"), Err(UrlError::BadAddress(_))));
        assert!(matches!(ObjectUrl::parse("corbaloc::h:notaport/K"), Err(UrlError::BadAddress(_))));
        assert!(matches!(ObjectUrl::parse("corbaloc:iiop:9@h/K"), Err(UrlError::BadAddress(_))));
        assert!(matches!(
            ObjectUrl::parse("corbaloc::h/%zz"),
            Err(UrlError::BadSchemeSpecificPart(_))
        ));
        assert!(matches!(
            ObjectUrl::parse("corbaloc::h/%4"),
            Err(UrlError::BadSchemeSpecificPart(_))
        ));
        assert_eq!(UrlError::BadSchemeName("x".into()).to_string(), "x (BAD_PARAM minor 7)");
    }

    #[test]
    fn url_becomes_a_parsable_ior() {
        let s =
            corbaloc_to_ior_string("corbaloc:iiop:1.2@127.0.0.1:4001/Echo", "IDL:spike/Echo:1.0")
                .unwrap();
        let back = Ior::parse(&s).unwrap();
        assert_eq!(back.type_id, "IDL:spike/Echo:1.0");
        assert_eq!(back.primary().unwrap().port, 4001);
        assert_eq!(back.primary().unwrap().object_key, b"Echo");
    }
}
