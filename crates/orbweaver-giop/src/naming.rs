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
                name: parse_stringified_name(name_part)?,
            });
        }
        Err(UrlError::BadSchemeName(format!("{url:?} is not corbaloc: or corbaname:")))
    }

    /// Builds an IOR for the cases that address an object directly.
    ///
    /// Every address becomes a profile, so the existing multi-profile handling
    /// covers a comma-separated list without a second mechanism.
    pub fn to_ior(&self, type_id: &str) -> Option<Ior> {
        let (addresses, object_key) = match self {
            ObjectUrl::Corbaloc { addresses, object_key } => (addresses, object_key),
            ObjectUrl::Corbaname { addresses, object_key, .. } => (addresses, object_key),
            ObjectUrl::InitialReference(_) => return None,
        };
        Some(Ior {
            type_id: type_id.to_owned(),
            profiles: addresses
                .iter()
                .map(|a| IiopProfile {
                    version: a.version,
                    host: a.host.clone(),
                    port: a.port,
                    object_key: object_key.clone(),
                    components: Vec::new(),
                })
                .collect(),
        })
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
    pub fn from_url(url: &ObjectUrl, timeout: Duration) -> Result<Self> {
        let ior = url
            .to_ior(NAMING_CONTEXT_EXT_ID)
            .ok_or(Error::BadIor("corbaloc:rir: needs a local resolver, not a connection"))?;
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

    #[test]
    fn rir_resolves_locally_and_defaults_to_nameservice() {
        assert_eq!(loc("corbaloc:rir:"), ObjectUrl::InitialReference("NameService".into()));
        assert_eq!(loc("corbaloc:rir:/"), ObjectUrl::InitialReference("NameService".into()));
        assert_eq!(
            loc("corbaloc:rir:/InterfaceRepository"),
            ObjectUrl::InitialReference("InterfaceRepository".into())
        );
        // It addresses nothing dialable, so it must not produce an IOR.
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
