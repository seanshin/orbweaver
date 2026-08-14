//! IOR endpoint rewriting for NAT, containers and load balancers (PLAN R7).
//!
//! An IOR carries addresses. A server puts into it the address it *believes*
//! it has, which inside a container is the container's address and behind a
//! load balancer is nobody's. Phase 0 assumption D measured this: the default
//! publish was a routable-but-local address, and the simulated-container case
//! left a client dialing `10.244.3.17` until the OS gave up. Rewriting is the
//! standard deployment step, not a troubleshooting one.
//!
//! # What this rewrites, and what it refuses to
//!
//! | Field | Disposition | Why |
//! |---|---|---|
//! | Profile `host`/`port` | Rewritten through the map | It is the address, and the address is what is wrong |
//! | `TAG_ALTERNATE_IIOP_ADDRESS` | Rewritten by the same map | [`crate::IiopProfile::endpoints`] dials these too, so a half-rewritten IOR still hangs a client on an internal address |
//! | Every profile, not only the first | All of them | Failover dials every profile in order; an unrewritten one costs a real connect timeout before the good one is reached |
//! | `object_key` | **Never touched** | It is the servant's identity, not a route. A rewriter that alters it turns "the wrong address" into "the wrong object", which fails later and further away |
//! | IIOP `version` | **Never touched** | The version is the peer's capability statement (§9.4.1). Rewriting it makes the client speak a protocol the server never claimed |
//! | `type_id` | **Never touched** | Interface identity |
//! | A profile tag we do not understand | **Preserved byte-for-byte** | §9.7.2. Dropping a profile a client could have used is a worse outcome than not rewriting at all |
//! | An IIOP profile that will not decode | **The whole rewrite fails** | The same answer [`crate::Ior::parse`] gives. A tag we claim to speak and cannot read is a malformed reference, and emitting a half-understood one would hide that |
//!
//! # Why not through [`crate::Ior`]
//!
//! [`crate::Ior`] is the *dialing* view: [`crate::Ior::read_from`] keeps
//! `TAG_INTERNET_IOP` profiles and discards every other tag, because it exists
//! to answer "where do I connect". That is lossy, and re-emitting a parsed
//! [`crate::Ior`] silently drops a `TAG_MULTIPLE_COMPONENTS` or vendor profile
//! that the original carried — a fact `ior_drops_a_profile_it_does_not_speak`
//! measures rather than assumes.
//!
//! Rewriting must not be lossy, so it runs on [`RawIor`], which keeps every
//! profile as `(tag, body)` and only decodes the IIOP ones. A profile whose
//! address needs no change is re-emitted as the *same bytes*, so an empty map
//! is the identity function on the wire — see `empty_map_is_byte_identical`.
//!
//! # Publish time or read time
//!
//! Both are implemented. [`crate::server::Server::ior_mapped`] rewrites at
//! **publish** time (the server publishes what it should have) and
//! [`rewrite_stringified`] rewrites at **read** time (a client repairs what it
//! received). This project prefers publish time; `docs/PHASE6.md` argues why,
//! and the short version is that a foreign ORB client cannot be patched and a
//! reference the server hands out is read by everybody.
//!
//! # A rewriter is a redirector
//!
//! Applied to somebody else's IOR, a map that matches broadly (`*`) points a
//! caller at an address of the map author's choosing. Read-time rewriting is
//! therefore an explicit call a caller makes on a reference it already
//! decided to trust, never a hook inside the decoder.

use crate::{Error, Result, TAG_ALTERNATE_IIOP_ADDRESS, TAG_INTERNET_IOP, TaggedComponent};
use orbweaver_cdr::{Decoder, Encoder, Endian};
use std::fmt;
use std::net::IpAddr;

/// Environment variable a deployment sets to configure publishing.
///
/// Named so a container image can carry the rewrite in its manifest rather
/// than in an operator's head, which is what "templated into every
/// deployment" (PLAN R7) has to mean to be worth anything.
pub const PUBLISH_MAP_ENV: &str = "ORBWEAVER_PUBLISH_MAP";

// ─────────────────────────────────────────────────────────────────────────────
// The map
// ─────────────────────────────────────────────────────────────────────────────

/// Why an endpoint map specification could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapError {
    /// A rule had no `=` separating the internal side from the published one.
    NoSeparator(String),
    /// A host part was empty.
    EmptyHost(String),
    /// A port was not a number in 1..=65535.
    BadPort(String),
    /// An IPv6 literal was written without brackets, so `host:port` cannot be
    /// split unambiguously.
    UnbracketedIpv6(String),
    /// The published side was `*`, which names no address.
    WildcardTarget(String),
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::NoSeparator(s) => {
                write!(f, "rule {s:?} has no '='; expected internal[:port]=published[:port]")
            }
            MapError::EmptyHost(s) => write!(f, "rule {s:?} has an empty host"),
            MapError::BadPort(s) => write!(f, "rule {s:?} has a port outside 1..=65535"),
            MapError::UnbracketedIpv6(s) => {
                write!(f, "rule {s:?}: an IPv6 literal must be bracketed, as [::1]:5555")
            }
            MapError::WildcardTarget(s) => {
                write!(f, "rule {s:?}: '*' is a pattern, so it cannot be the published address")
            }
        }
    }
}

impl std::error::Error for MapError {}

/// One `internal → published` mapping.
///
/// The internal side may leave the port open (matching any port on that host)
/// and the published side may leave it open (keeping whatever port matched).
/// `*` on the internal host matches every host — the container idiom, where
/// the address to be replaced is not known until the container is running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    from_host: String,
    from_port: Option<u16>,
    to_host: String,
    to_port: Option<u16>,
}

impl Rule {
    /// Rewrites `from` to `to` on any port, keeping the port.
    pub fn host(from: &str, to: &str) -> Rule {
        Rule { from_host: from.to_owned(), from_port: None, to_host: to.to_owned(), to_port: None }
    }

    /// Rewrites one exact endpoint to another.
    pub fn endpoint(from_host: &str, from_port: u16, to_host: &str, to_port: u16) -> Rule {
        Rule {
            from_host: from_host.to_owned(),
            from_port: Some(from_port),
            to_host: to_host.to_owned(),
            to_port: Some(to_port),
        }
    }

    /// Matches every host on any port and republishes it at `to`, keeping the
    /// port.
    ///
    /// Correct only where the deployment knows every address in the IOR is its
    /// own — a container publishing its own reference. Applied to a reference
    /// that came from somewhere else it rewrites somebody else's address, which
    /// is redirection rather than translation.
    pub fn any_host(to: &str) -> Rule {
        Rule::host("*", to)
    }

    /// Reads `internal[:port]=published[:port]`.
    ///
    /// IPv6 literals are bracketed: `[fd00::1]:5555=[2001:db8::1]:683`.
    pub fn parse(spec: &str) -> std::result::Result<Rule, MapError> {
        let spec = spec.trim();
        let (from, to) = spec.split_once('=').ok_or_else(|| MapError::NoSeparator(spec.into()))?;
        let (from_host, from_port) = split_host_port(from.trim(), spec)?;
        let (to_host, to_port) = split_host_port(to.trim(), spec)?;
        if to_host == "*" {
            return Err(MapError::WildcardTarget(spec.into()));
        }
        Ok(Rule { from_host, from_port, to_host, to_port })
    }

    /// The published endpoint for `host:port`, or `None` when this rule does
    /// not match it.
    pub fn apply(&self, host: &str, port: u16) -> Option<(String, u16)> {
        if self.from_port.is_some_and(|p| p != port) {
            return None;
        }
        if self.from_host != "*" && !same_host(&self.from_host, host) {
            return None;
        }
        Some((self.to_host.clone(), self.to_port.unwrap_or(port)))
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.from_host)?;
        if let Some(p) = self.from_port {
            write!(f, ":{p}")?;
        }
        write!(f, "={}", self.to_host)?;
        if let Some(p) = self.to_port {
            write!(f, ":{p}")?;
        }
        Ok(())
    }
}

/// Two host strings naming the same host.
///
/// Compared as addresses when both parse as one, so `::1` and `0:0:0:0:0:0:0:1`
/// match; otherwise ASCII-case-insensitively, which is how DNS names compare.
fn same_host(a: &str, b: &str) -> bool {
    match (a.parse::<IpAddr>(), b.parse::<IpAddr>()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a.eq_ignore_ascii_case(b),
    }
}

fn split_host_port(s: &str, whole: &str) -> std::result::Result<(String, Option<u16>), MapError> {
    let (host, port) = if let Some(rest) = s.strip_prefix('[') {
        let (host, tail) = rest.split_once(']').ok_or_else(|| MapError::EmptyHost(whole.into()))?;
        let port = match tail.strip_prefix(':') {
            Some(p) => Some(p),
            None if tail.is_empty() => None,
            None => return Err(MapError::BadPort(whole.into())),
        };
        (host.to_owned(), port)
    } else if s.matches(':').count() > 1 {
        return Err(MapError::UnbracketedIpv6(whole.into()));
    } else {
        match s.split_once(':') {
            Some((h, p)) => (h.to_owned(), Some(p)),
            None => (s.to_owned(), None),
        }
    };
    if host.is_empty() {
        return Err(MapError::EmptyHost(whole.into()));
    }
    let port = match port {
        // Port 0 is a bind-time wildcard, never a destination: an IOR naming
        // it cannot be dialed, so accepting it would produce a rewrite that
        // looks applied and still fails.
        Some(p) => Some(
            p.parse::<u16>()
                .ok()
                .filter(|&p| p != 0)
                .ok_or_else(|| MapError::BadPort(whole.into()))?,
        ),
        None => None,
    };
    Ok((host, port))
}

/// An ordered set of [`Rule`]s, first match winning.
///
/// Order is the whole interface: a rule that maps an address to itself, placed
/// first, is how a deployment protects one endpoint from a broader rule that
/// follows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EndpointMap {
    rules: Vec<Rule>,
    drop_unmapped_alternates: bool,
}

impl EndpointMap {
    /// An empty map, which rewrites nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a rule, returning `self` so maps can be built inline.
    pub fn with(mut self, rule: Rule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Appends a rule.
    pub fn push(&mut self, rule: Rule) {
        self.rules.push(rule);
    }

    /// Reads a comma- or whitespace-separated list of [`Rule`]s.
    ///
    /// `10.244.3.17:5555=203.0.113.9:31000, 10.244.3.18=203.0.113.9`
    pub fn parse(spec: &str) -> std::result::Result<Self, MapError> {
        let mut map = EndpointMap::new();
        for part in spec.split([',', ' ', '\t', '\n']).filter(|p| !p.trim().is_empty()) {
            map.push(Rule::parse(part)?);
        }
        Ok(map)
    }

    /// Reads [`PUBLISH_MAP_ENV`], or `None` when it is unset or empty.
    ///
    /// An unset variable is not an error: a deployment with no NAT in front of
    /// it sets nothing and publishes what it bound. A variable that is set and
    /// unreadable *is* an error, because silently publishing the internal
    /// address is the failure this module exists to prevent.
    pub fn from_env() -> std::result::Result<Option<Self>, MapError> {
        match std::env::var(PUBLISH_MAP_ENV) {
            Ok(v) if v.trim().is_empty() => Ok(None),
            Ok(v) => EndpointMap::parse(&v).map(Some),
            Err(_) => Ok(None),
        }
    }

    /// Whether this map has no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The rules, in match order.
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Drops alternate addresses no rule matched, instead of keeping them.
    ///
    /// Off by default. An alternate is a route the client may use, and
    /// deleting a route is a loss; but an internal alternate left in a
    /// published IOR costs every client a full connect timeout before it
    /// reaches a working endpoint. Turn this on only where the map is known to
    /// name every endpoint of this server. It never removes a profile's own
    /// address — that would delete the profile, and a rewriter that can delete
    /// profiles is the failure mode this module refuses.
    pub fn drop_unmapped_alternates(mut self, yes: bool) -> Self {
        self.drop_unmapped_alternates = yes;
        self
    }

    /// The published endpoint for `host:port` under the first matching rule.
    pub fn apply(&self, host: &str, port: u16) -> Option<(String, u16)> {
        self.rules.iter().find_map(|r| r.apply(host, port))
    }
}

impl fmt::Display for EndpointMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for r in &self.rules {
            if !first {
                f.write_str(",")?;
            }
            write!(f, "{r}")?;
            first = false;
        }
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The lossless IOR
// ─────────────────────────────────────────────────────────────────────────────

/// A tagged profile kept exactly as it arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawProfile {
    /// `IOP::ProfileId`. [`TAG_INTERNET_IOP`] is the one we decode.
    pub tag: u32,
    /// The profile body, undecoded.
    pub body: Vec<u8>,
}

/// An IOR with every profile preserved, whether or not we speak it.
///
/// This is the representation a rewriter needs and [`crate::Ior`] deliberately
/// is not: `Ior` answers "where do I dial", so it keeps IIOP profiles and
/// forgets the rest. Byte order of the outer encapsulation is remembered too,
/// so re-emitting an unmodified IOR reproduces its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawIor {
    /// Repository ID of the most-derived interface.
    pub type_id: String,
    /// Every profile, in order, verbatim.
    pub profiles: Vec<RawProfile>,
    /// Byte order the source encapsulation used.
    pub endian: Endian,
}

impl RawIor {
    /// Parses the `IOR:<hex>` stringified form.
    pub fn parse(s: &str) -> Result<Self> {
        Self::from_encapsulation(&crate::ior_hex_bytes(s)?)
    }

    /// Parses the CDR encapsulation a stringified IOR wraps.
    pub fn from_encapsulation(bytes: &[u8]) -> Result<Self> {
        let mut d = Decoder::encapsulation(bytes)?;
        let endian = d.endian();
        let (type_id, profiles) = Self::read_body(&mut d)?;
        Ok(RawIor { type_id, profiles, endian })
    }

    /// Reads an IOR marshalled inline in an existing stream (§9.3.6),
    /// adopting that stream's byte order.
    pub fn read_from(d: &mut Decoder<'_>) -> Result<Self> {
        let endian = d.endian();
        let (type_id, profiles) = Self::read_body(d)?;
        Ok(RawIor { type_id, profiles, endian })
    }

    fn read_body(d: &mut Decoder<'_>) -> Result<(String, Vec<RawProfile>)> {
        let type_id = String::from_utf8_lossy(d.get_string_bytes()?).into_owned();
        let count = d.get_u32()?;
        let count = d.validate_count(count, 8)?;
        let mut profiles = Vec::with_capacity(count);
        for _ in 0..count {
            let tag = d.get_u32()?;
            let body = d.get_octet_seq()?.to_vec();
            profiles.push(RawProfile { tag, body });
        }
        Ok((type_id, profiles))
    }

    /// Marshals inline into an existing stream (§9.3.6).
    pub fn write_to(&self, e: &mut Encoder) -> Result<()> {
        e.put_str(&self.type_id);
        e.put_u32(self.profiles.len() as u32);
        for p in &self.profiles {
            e.put_u32(p.tag);
            e.put_octet_seq(&p.body);
        }
        Ok(())
    }

    /// Produces the `IOR:<hex>` stringified form, in the byte order this
    /// reference was read in.
    pub fn to_stringified(&self) -> Result<String> {
        let mut e = Encoder::encapsulation(self.endian);
        self.write_to(&mut e)?;
        Ok(crate::hex_ior(&e.finish().map_err(Error::Cdr)?))
    }

    /// The dialing view: the same reference with non-IIOP profiles dropped.
    pub fn to_ior(&self) -> Result<crate::Ior> {
        let mut profiles = Vec::new();
        for p in &self.profiles {
            if p.tag == TAG_INTERNET_IOP {
                profiles.push(crate::parse_iiop_profile(&p.body)?);
            }
        }
        Ok(crate::Ior { type_id: self.type_id.clone(), profiles })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The rewrite
// ─────────────────────────────────────────────────────────────────────────────

/// Where a rewritten endpoint sat in the reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    /// The profile's own `host`/`port`.
    ProfileAddress,
    /// A `TAG_ALTERNATE_IIOP_ADDRESS` component of that profile.
    Alternate,
}

impl fmt::Display for Where {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Where::ProfileAddress => "profile address",
            Where::Alternate => "alternate",
        })
    }
}

/// One endpoint that moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// Index of the profile within the IOR.
    pub profile: usize,
    /// Which field moved.
    pub site: Where,
    /// Address before.
    pub from: (String, u16),
    /// Address after.
    pub to: (String, u16),
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "profile {} {}: {}:{} -> {}:{}",
            self.profile, self.site, self.from.0, self.from.1, self.to.0, self.to.1
        )
    }
}

/// What a rewrite did, so a deployment can assert on it instead of hoping.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RewriteReport {
    /// Profiles in the reference, of every tag.
    pub profiles: usize,
    /// Profiles carrying [`TAG_INTERNET_IOP`].
    pub iiop_profiles: usize,
    /// Profiles of a tag we do not decode, kept byte-for-byte.
    pub foreign_profiles: usize,
    /// Every endpoint that moved.
    pub changed: Vec<Change>,
    /// Endpoints no rule matched, left as they were.
    pub unmapped: Vec<(String, u16)>,
    /// Alternates removed under [`EndpointMap::drop_unmapped_alternates`].
    pub dropped: Vec<(String, u16)>,
    /// Alternate components whose body would not decode; kept verbatim.
    pub malformed_alternates: usize,
}

impl RewriteReport {
    /// Whether anything moved.
    pub fn changed_anything(&self) -> bool {
        !self.changed.is_empty() || !self.dropped.is_empty()
    }
}

impl fmt::Display for RewriteReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} profile(s): {} IIOP, {} preserved unread; {} endpoint(s) rewritten, \
             {} unmapped, {} alternate(s) dropped, {} malformed alternate(s)",
            self.profiles,
            self.iiop_profiles,
            self.foreign_profiles,
            self.changed.len(),
            self.unmapped.len(),
            self.dropped.len(),
            self.malformed_alternates
        )
    }
}

/// Rewrites every endpoint in `ior` that `map` names, preserving everything
/// else — including profiles this implementation does not understand.
pub fn rewrite(ior: &RawIor, map: &EndpointMap) -> Result<(RawIor, RewriteReport)> {
    let mut report = RewriteReport { profiles: ior.profiles.len(), ..Default::default() };
    let mut out = Vec::with_capacity(ior.profiles.len());
    for (index, p) in ior.profiles.iter().enumerate() {
        if p.tag != TAG_INTERNET_IOP {
            // §9.7.2. We cannot know whether this profile carries an address,
            // so the only honest options are "leave it" and "refuse the whole
            // rewrite". Leaving it keeps a route the client may be able to
            // use; dropping it would delete one silently.
            report.foreign_profiles += 1;
            out.push(p.clone());
            continue;
        }
        report.iiop_profiles += 1;
        out.push(rewrite_iiop_profile(index, p, map, &mut report)?);
    }
    Ok((RawIor { type_id: ior.type_id.clone(), profiles: out, endian: ior.endian }, report))
}

/// [`rewrite`] on the stringified form: `IOR:...` in, `IOR:...` out.
///
/// This is the read-time entry point — a client repairing a reference it
/// received and has decided to trust.
pub fn rewrite_stringified(ior: &str, map: &EndpointMap) -> Result<(String, RewriteReport)> {
    let (out, report) = rewrite(&RawIor::parse(ior)?, map)?;
    Ok((out.to_stringified()?, report))
}

fn rewrite_iiop_profile(
    index: usize,
    raw: &RawProfile,
    map: &EndpointMap,
    report: &mut RewriteReport,
) -> Result<RawProfile> {
    let endian = Decoder::encapsulation(&raw.body)?.endian();
    let profile = crate::parse_iiop_profile(&raw.body)?;
    let mut out = profile.clone();
    let mut changed = false;

    match map.apply(&profile.host, profile.port) {
        Some((host, port)) if (host.as_str(), port) != (profile.host.as_str(), profile.port) => {
            report.changed.push(Change {
                profile: index,
                site: Where::ProfileAddress,
                from: (profile.host.clone(), profile.port),
                to: (host.clone(), port),
            });
            out.host = host;
            out.port = port;
            changed = true;
        }
        // Matched a rule that names the address it already has: an identity
        // guard, deliberately not reported as unmapped.
        Some(_) => {}
        None => report.unmapped.push((profile.host.clone(), profile.port)),
    }

    out.components.clear();
    for c in &profile.components {
        if c.tag != TAG_ALTERNATE_IIOP_ADDRESS {
            out.components.push(c.clone());
            continue;
        }
        let Ok((host, port)) = crate::parse_alternate_address(&c.data) else {
            // The same posture `IiopProfile::endpoints` takes: a bad hint is
            // skipped, never fatal, and never silently rewritten into
            // something well-formed that nobody wrote.
            report.malformed_alternates += 1;
            out.components.push(c.clone());
            continue;
        };
        match map.apply(&host, port) {
            Some((h, p)) if (h.as_str(), p) != (host.as_str(), port) => {
                report.changed.push(Change {
                    profile: index,
                    site: Where::Alternate,
                    from: (host, port),
                    to: (h.clone(), p),
                });
                let comp_endian = Decoder::encapsulation(&c.data)?.endian();
                out.components.push(alternate_address(&h, p, comp_endian)?);
                changed = true;
            }
            Some(_) => out.components.push(c.clone()),
            None if map.drop_unmapped_alternates => {
                report.dropped.push((host, port));
                changed = true;
            }
            None => {
                report.unmapped.push((host, port));
                out.components.push(c.clone());
            }
        }
    }

    // Nothing moved: re-emit the original bytes rather than our own encoding
    // of them. A rewriter that reserializes what it did not change turns every
    // encoding difference — component order, a vendor's padding — into a diff
    // nobody asked for, and makes "the map matched nothing" indistinguishable
    // from "the map did something".
    if !changed {
        return Ok(raw.clone());
    }
    Ok(RawProfile { tag: TAG_INTERNET_IOP, body: out.encapsulate(endian)?.finish()? })
}

/// Builds a `TAG_ALTERNATE_IIOP_ADDRESS` component: an encapsulated
/// `string host; unsigned short port;`.
pub fn alternate_address(host: &str, port: u16, endian: Endian) -> Result<TaggedComponent> {
    let mut e = Encoder::encapsulation(endian);
    e.put_str(host);
    e.put_u16(port);
    Ok(TaggedComponent { tag: TAG_ALTERNATE_IIOP_ADDRESS, data: e.finish()? })
}

/// The endpoint a server should publish for the address it bound.
///
/// `None` when the map names no rule for it. Wildcard bind addresses
/// (`0.0.0.0`, `::`) are the case a deployment must not get wrong: they are
/// bindable and unpublishable, so an unmapped wildcard is an error rather than
/// a default — see [`crate::server::Server::ior_mapped`].
pub fn published_address(bound: std::net::SocketAddr, map: &EndpointMap) -> Option<(String, u16)> {
    map.apply(&bound.ip().to_string(), bound.port())
}

/// Whether an address is one no client can dial: the unspecified address.
pub fn is_unpublishable(ip: std::net::IpAddr) -> bool {
    ip.is_unspecified()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IiopProfile, Ior, Version};

    fn profile(host: &str, port: u16, alternates: &[(&str, u16)]) -> IiopProfile {
        IiopProfile {
            version: Version::V1_2,
            host: host.into(),
            port,
            object_key: b"servant-identity".to_vec(),
            components: alternates
                .iter()
                .map(|(h, p)| alternate_address(h, *p, Endian::Little).unwrap())
                .collect(),
        }
    }

    fn raw_of(profiles: &[IiopProfile], extra: &[RawProfile]) -> RawIor {
        let mut out: Vec<RawProfile> = profiles
            .iter()
            .map(|p| RawProfile {
                tag: TAG_INTERNET_IOP,
                body: p.encapsulate(Endian::Little).unwrap().finish().unwrap(),
            })
            .collect();
        out.extend_from_slice(extra);
        RawIor { type_id: "IDL:spike/Echo:1.0".into(), profiles: out, endian: Endian::Little }
    }

    /// A profile tag nothing here decodes. `TAG_MULTIPLE_COMPONENTS` is 1 and
    /// real; the body is opaque on purpose.
    fn foreign() -> RawProfile {
        RawProfile { tag: 1, body: vec![1, 9, 9, 9, 0, 0, 0, 7] }
    }

    // ── the map ─────────────────────────────────────────────────────────────

    #[test]
    fn rules_parse_in_every_shape_the_docs_promise() {
        let m = EndpointMap::parse(
            "10.244.3.17:5555=203.0.113.9:31000, 10.244.3.18=203.0.113.9 *=lb.example",
        )
        .unwrap();
        assert_eq!(m.rules().len(), 3);
        assert_eq!(m.apply("10.244.3.17", 5555), Some(("203.0.113.9".into(), 31000)));
        // Host-only rule keeps the port it matched.
        assert_eq!(m.apply("10.244.3.18", 4242), Some(("203.0.113.9".into(), 4242)));
        // The exact rule does not match another port; the wildcard then does.
        assert_eq!(m.apply("10.244.3.17", 9), Some(("lb.example".into(), 9)));
        assert_eq!(
            m.to_string(),
            "10.244.3.17:5555=203.0.113.9:31000,10.244.3.18=203.0.113.9,*=lb.example"
        );
    }

    #[test]
    fn ipv6_needs_brackets_and_then_works() {
        assert_eq!(
            Rule::parse("fd00::1:5555=192.0.2.1:5555"),
            Err(MapError::UnbracketedIpv6("fd00::1:5555=192.0.2.1:5555".into()))
        );
        let r = Rule::parse("[fd00::1]:5555=[2001:db8::1]:683").unwrap();
        assert_eq!(r.apply("fd00::1", 5555), Some(("2001:db8::1".into(), 683)));
        // Written out long-hand, it is the same address.
        assert_eq!(r.apply("fd00:0:0:0:0:0:0:1", 5555), Some(("2001:db8::1".into(), 683)));
    }

    #[test]
    fn a_rule_that_cannot_mean_anything_is_refused() {
        assert!(matches!(Rule::parse("10.0.0.1"), Err(MapError::NoSeparator(_))));
        assert!(matches!(Rule::parse("=10.0.0.1"), Err(MapError::EmptyHost(_))));
        assert!(matches!(Rule::parse("a:70000=b:1"), Err(MapError::BadPort(_))));
        // Port 0 is a bind wildcard, never a destination.
        assert!(matches!(Rule::parse("a:1=b:0"), Err(MapError::BadPort(_))));
        assert!(matches!(Rule::parse("a=*"), Err(MapError::WildcardTarget(_))));
    }

    #[test]
    fn first_matching_rule_wins_so_an_identity_guard_works() {
        let map = EndpointMap::new()
            .with(Rule::endpoint("10.0.0.1", 683, "10.0.0.1", 683))
            .with(Rule::any_host("203.0.113.9"));
        assert_eq!(map.apply("10.0.0.1", 683), Some(("10.0.0.1".into(), 683)));
        assert_eq!(map.apply("10.0.0.2", 683), Some(("203.0.113.9".into(), 683)));
    }

    // ── losslessness ────────────────────────────────────────────────────────

    /// The measurement behind "rewriting does not go through `Ior`": `Ior`
    /// really does drop what it cannot dial, so a rewriter built on it would
    /// delete a profile every time.
    #[test]
    fn ior_drops_a_profile_it_does_not_speak() {
        let raw = raw_of(&[profile("10.244.3.17", 5555, &[])], &[foreign()]);
        let text = raw.to_stringified().unwrap();
        assert_eq!(RawIor::parse(&text).unwrap().profiles.len(), 2);
        let dialing = Ior::parse(&text).unwrap();
        assert_eq!(dialing.profiles.len(), 1, "Ior keeps only IIOP profiles");
        // And re-emitting the dialing view is where the loss becomes permanent.
        let round = Ior::parse(&dialing.to_stringified().unwrap()).unwrap();
        assert_eq!(round.profiles.len(), 1);
        assert_eq!(RawIor::parse(&dialing.to_stringified().unwrap()).unwrap().profiles.len(), 1);
    }

    #[test]
    fn empty_map_is_byte_identical() {
        let raw = raw_of(&[profile("10.244.3.17", 5555, &[("10.244.3.18", 5555)])], &[foreign()]);
        let before = raw.to_stringified().unwrap();
        let (after, report) = rewrite_stringified(&before, &EndpointMap::new()).unwrap();
        assert_eq!(after, before, "an empty map must be the identity on the wire");
        assert!(!report.changed_anything());
        assert_eq!(report.unmapped.len(), 2, "both endpoints reported as unmapped");
    }

    #[test]
    fn a_profile_we_do_not_understand_survives_verbatim() {
        let raw = raw_of(&[profile("10.244.3.17", 5555, &[])], &[foreign()]);
        let map = EndpointMap::new().with(Rule::any_host("203.0.113.9"));
        let (out, report) = rewrite(&raw, &map).unwrap();
        assert_eq!(report.profiles, 2);
        assert_eq!(report.foreign_profiles, 1);
        assert_eq!(out.profiles[1], foreign(), "tag and body must be untouched");
        assert_eq!(out.profiles.len(), raw.profiles.len(), "profile count is preserved");
    }

    #[test]
    fn identity_and_version_are_never_rewritten() {
        let raw = raw_of(&[profile("10.244.3.17", 5555, &[])], &[]);
        let map = EndpointMap::new().with(Rule::endpoint("10.244.3.17", 5555, "203.0.113.9", 683));
        let (out, _) = rewrite(&raw, &map).unwrap();
        let before = raw.to_ior().unwrap();
        let after = out.to_ior().unwrap();
        assert_eq!(after.type_id, before.type_id);
        assert_eq!(after.profiles[0].object_key, before.profiles[0].object_key);
        assert_eq!(after.profiles[0].version, before.profiles[0].version);
        assert_eq!(after.profiles[0].host, "203.0.113.9");
        assert_eq!(after.profiles[0].port, 683);
    }

    // ── coverage of the whole reference ─────────────────────────────────────

    #[test]
    fn every_profile_is_rewritten_not_only_the_first() {
        let raw =
            raw_of(&[profile("10.244.3.17", 5555, &[]), profile("10.244.3.18", 5556, &[])], &[]);
        let map = EndpointMap::new()
            .with(Rule::endpoint("10.244.3.17", 5555, "203.0.113.9", 31000))
            .with(Rule::endpoint("10.244.3.18", 5556, "203.0.113.9", 31001));
        let (out, report) = rewrite(&raw, &map).unwrap();
        let after = out.to_ior().unwrap();
        assert_eq!(report.changed.len(), 2);
        assert_eq!(after.profiles[0].endpoints(), vec![("203.0.113.9".into(), 31000)]);
        assert_eq!(after.profiles[1].endpoints(), vec![("203.0.113.9".into(), 31001)]);
        assert!(report.unmapped.is_empty());
    }

    #[test]
    fn alternates_are_rewritten_because_the_client_dials_them() {
        let raw = raw_of(
            &[profile("10.244.3.17", 5555, &[("10.244.3.18", 5555), ("10.244.3.19", 5555)])],
            &[],
        );
        let map = EndpointMap::new().with(Rule::any_host("203.0.113.9"));
        let (out, report) = rewrite(&raw, &map).unwrap();
        let after = out.to_ior().unwrap();
        assert_eq!(
            after.profiles[0].endpoints(),
            vec![
                ("203.0.113.9".to_string(), 5555),
                ("203.0.113.9".to_string(), 5555),
                ("203.0.113.9".to_string(), 5555)
            ],
            "every endpoint the failover path derives must be rewritten, in order"
        );
        assert_eq!(report.changed.len(), 3);
        assert_eq!(report.changed[1].site, Where::Alternate);
    }

    #[test]
    fn a_malformed_alternate_is_kept_not_repaired() {
        let mut p = profile("10.244.3.17", 5555, &[]);
        p.components.push(TaggedComponent { tag: TAG_ALTERNATE_IIOP_ADDRESS, data: vec![1, 255] });
        let raw = raw_of(&[p], &[]);
        let map = EndpointMap::new().with(Rule::any_host("203.0.113.9"));
        let (out, report) = rewrite(&raw, &map).unwrap();
        assert_eq!(report.malformed_alternates, 1);
        let after = out.to_ior().unwrap();
        assert_eq!(after.profiles[0].components[0].data, vec![1, 255]);
        assert_eq!(after.profiles[0].host, "203.0.113.9");
    }

    #[test]
    fn unmapped_alternates_are_dropped_only_when_asked_and_profiles_never_are() {
        let raw = raw_of(&[profile("10.244.3.17", 5555, &[("10.9.9.9", 5555)])], &[]);
        let map = EndpointMap::new()
            .with(Rule::endpoint("10.244.3.17", 5555, "203.0.113.9", 31000))
            .drop_unmapped_alternates(true);
        let (out, report) = rewrite(&raw, &map).unwrap();
        assert_eq!(report.dropped, vec![("10.9.9.9".to_string(), 5555)]);
        let after = out.to_ior().unwrap();
        assert_eq!(after.profiles.len(), 1, "the profile itself survives");
        assert_eq!(after.profiles[0].endpoints(), vec![("203.0.113.9".into(), 31000)]);

        // The same map with an unmapped *profile* address keeps the profile.
        let raw = raw_of(&[profile("10.9.9.9", 5555, &[])], &[]);
        let (out, report) = rewrite(&raw, &map).unwrap();
        assert_eq!(out.profiles.len(), 1);
        assert_eq!(report.unmapped, vec![("10.9.9.9".to_string(), 5555)]);
        assert!(report.dropped.is_empty());
    }

    // ── encoding fidelity ───────────────────────────────────────────────────

    #[test]
    fn byte_order_is_preserved_on_both_sides() {
        for endian in [Endian::Big, Endian::Little] {
            let p = IiopProfile {
                version: Version::V1_2,
                host: "10.244.3.17".into(),
                port: 5555,
                object_key: b"k".to_vec(),
                components: vec![alternate_address("10.244.3.18", 5555, endian).unwrap()],
            };
            let raw = RawIor {
                type_id: "IDL:spike/Echo:1.0".into(),
                profiles: vec![RawProfile {
                    tag: TAG_INTERNET_IOP,
                    body: p.encapsulate(endian).unwrap().finish().unwrap(),
                }],
                endian,
            };
            let map = EndpointMap::new().with(Rule::any_host("203.0.113.9"));
            let (out, _) = rewrite(&raw, &map).unwrap();
            assert_eq!(out.endian, endian, "outer encapsulation keeps its byte order");
            let body = &out.profiles[0].body;
            assert_eq!(
                Decoder::encapsulation(body).unwrap().endian(),
                endian,
                "profile encapsulation keeps its byte order"
            );
            let after = out.to_ior().unwrap();
            assert_eq!(
                after.profiles[0].endpoints(),
                vec![("203.0.113.9".to_string(), 5555), ("203.0.113.9".to_string(), 5555)]
            );
        }
    }

    #[test]
    fn a_1_0_profile_rewrites_without_growing_a_component_list() {
        let p = IiopProfile {
            version: Version::V1_0,
            host: "10.244.3.17".into(),
            port: 5555,
            object_key: b"k".to_vec(),
            components: Vec::new(),
        };
        let raw = raw_of(&[p], &[]);
        let map = EndpointMap::new().with(Rule::any_host("203.0.113.9"));
        let (out, _) = rewrite(&raw, &map).unwrap();
        let after = out.to_ior().unwrap();
        assert_eq!(after.profiles[0].version, Version::V1_0);
        assert!(after.profiles[0].components.is_empty());
        // §9.7.2: a 1.0 profile carries no trailing data. Body is exactly the
        // fixed fields, so re-encoding must not have appended a count.
        let mut d = Decoder::encapsulation(&out.profiles[0].body).unwrap();
        d.get_u8().unwrap();
        d.get_u8().unwrap();
        d.get_string_bytes().unwrap();
        d.get_u16().unwrap();
        d.get_octet_seq().unwrap();
        assert!(d.is_empty(), "a 1.0 profile must end after the object key");
    }

    /// A tag we claim to speak and cannot read is a malformed reference, and
    /// the rewrite says so rather than emitting a half-understood one. This is
    /// the one profile-level failure that is *not* preserved-and-ignored, and
    /// it matches what `Ior::parse` already does with the same bytes.
    #[test]
    fn an_undecodable_iiop_profile_fails_the_rewrite() {
        let truncated = RawProfile { tag: TAG_INTERNET_IOP, body: vec![1, 1, 2, 0] };
        let raw = RawIor {
            type_id: "IDL:spike/Echo:1.0".into(),
            profiles: vec![truncated],
            endian: Endian::Little,
        };
        let map = EndpointMap::new().with(Rule::any_host("203.0.113.9"));
        assert!(rewrite(&raw, &map).is_err());
        assert!(Ior::parse(&raw.to_stringified().unwrap()).is_err(), "and Ior agrees");
    }

    #[test]
    fn a_nil_reference_survives_a_rewrite() {
        let raw = RawIor { type_id: String::new(), profiles: Vec::new(), endian: Endian::Little };
        let map = EndpointMap::new().with(Rule::any_host("203.0.113.9"));
        let (out, report) = rewrite(&raw, &map).unwrap();
        assert!(out.to_ior().unwrap().is_nil());
        assert_eq!(report.profiles, 0);
    }

    #[test]
    fn a_rewritten_reference_is_still_a_parseable_ior() {
        let raw = raw_of(&[profile("10.244.3.17", 5555, &[("10.244.3.18", 5555)])], &[foreign()]);
        let map = EndpointMap::new().with(Rule::any_host("127.0.0.1"));
        let (text, _) = rewrite_stringified(&raw.to_stringified().unwrap(), &map).unwrap();
        let ior = Ior::parse(&text).unwrap();
        assert_eq!(ior.primary().unwrap().host, "127.0.0.1");
        assert_eq!(RawIor::parse(&text).unwrap().profiles.len(), 2);
    }

    #[test]
    fn published_address_needs_a_rule_for_a_wildcard_bind() {
        let map = EndpointMap::new().with(Rule::endpoint("0.0.0.0", 5555, "203.0.113.9", 31000));
        let bound: std::net::SocketAddr = "0.0.0.0:5555".parse().unwrap();
        assert!(is_unpublishable(bound.ip()));
        assert_eq!(published_address(bound, &map), Some(("203.0.113.9".into(), 31000)));
        assert_eq!(published_address(bound, &EndpointMap::new()), None);
    }

    // ── the dial actually changes destination ───────────────────────────────

    /// A unit test can show the rewrite moves the connect, and no more than
    /// that: both addresses are on this machine, so it proves nothing about a
    /// NAT boundary. `spikes/nat_rewrite.sh` is where the failing case lives.
    #[test]
    fn the_rewritten_reference_dials_somewhere_the_original_could_not() {
        use crate::Connection;
        use std::time::Duration;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // A second address on this machine with nothing bound to it: the
        // connect is refused rather than timing out, so the test cannot hang.
        let dead = a_free_low_port();

        let raw = raw_of(&[profile("127.0.0.1", dead, &[])], &[]);
        let text = raw.to_stringified().unwrap();
        let before = Ior::parse(&text).unwrap();
        assert!(
            Connection::connect(&before, Duration::from_millis(500)).is_err(),
            "the unrewritten reference must not dial"
        );

        let map = EndpointMap::new().with(Rule::endpoint("127.0.0.1", dead, "127.0.0.1", port));
        let (text, report) = rewrite_stringified(&text, &map).unwrap();
        assert_eq!(report.changed.len(), 1);
        let after = Ior::parse(&text).unwrap();
        let conn = Connection::connect(&after, Duration::from_millis(500));
        assert!(conn.is_ok(), "the rewritten reference must reach the listener");
        drop(listener);
    }

    /// A port the OS just proved free, below the ephemeral floor so a connect
    /// cannot land on itself — the reasoning `spike-failover` records.
    fn a_free_low_port() -> u16 {
        for port in 2048..5000u16 {
            if let Ok(l) = std::net::TcpListener::bind(("127.0.0.1", port)) {
                drop(l);
                return port;
            }
        }
        panic!("no free low port on loopback");
    }
}
