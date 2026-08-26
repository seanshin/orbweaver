//! Connections reused per endpoint, instead of one dialed per reference.
//!
//! A CORBA client holds references, not connections: resolving four names out
//! of one naming service yields four IORs that all point at the same
//! host and port. Dialing per reference pays a TCP handshake, a codeset
//! negotiation and a file descriptor for each of them, and — before
//! [`crate::mux`] — serialized nothing, because each had its own socket. This
//! module keeps one connection per endpoint and lets [`crate::mux`] carry
//! everybody's calls over it.
//!
//! *레퍼런스마다 새로 연결하지 않고, 엔드포인트마다 연결을 재사용한다.*
//!
//! # The key
//!
//! `(host, port, GIOP version, negotiated char codeset)` — the endpoint plus
//! **everything that is agreed once per connection and then implied by every
//! message on it**.
//!
//! Host and port alone are not enough, and the two extra fields are not
//! caution:
//!
//! - **Version.** §9.4.1 forbids exceeding the minor version a profile
//!   advertises, and profiles on one server do differ — an object published by
//!   an older POA can carry a 1.0 profile beside a 1.2 one. A pooled 1.2
//!   connection handed to a 1.0 reference would speak a version that
//!   reference's profile never offered. It also decides whether the connection
//!   multiplexes at all ([`crate::mux`]), so two versions on one connection
//!   would mean two different concurrency rules on one socket.
//! - **Codeset.** §7.10.2.5 negotiates per *connection* and the agreement
//!   travels in a service context on the first request only. Two references
//!   whose profiles publish different `TAG_CODE_SETS` therefore cannot share a
//!   connection: the second one's strings would go out encoded under the
//!   first's agreement, with no way for the peer to tell. This is the failure
//!   that would not show up in any test with ASCII-only fixtures, which is
//!   exactly why it is in the key rather than in a comment.
//!
//! The **object key is not** in it, and that is the point of pooling: GIOP
//! addresses per message, so one connection serves every object behind an
//! endpoint. [`crate::mux::Mux::call_on`] takes the key per call for this
//! reason.
//!
//! TLS connections are **not pooled**. `Connection::connect_tls` (feature
//! `ssliop`) takes a `rustls::ClientConfig` that says which roots to trust and
//! which client certificate to present; that is a trust decision, and two
//! configs that differ are two different decisions. A cache would have to key on it, an
//! `Arc` pointer is not an identity a cache may use, and a pool that silently
//! handed one caller's TLS session to another caller's trust policy would be a
//! security bug wearing a performance feature's clothes.
//!
//! # When a connection is discarded
//!
//! - **It faulted.** Anything that makes framing unknowable takes the
//!   connection out on the spot; a `Mux` answers [`Mux::is_usable`] for
//!   exactly this question and the pool asks before handing one out.
//! - **It went idle.** [`Limits::max_idle`], default
//!   [`DEFAULT_MAX_IDLE`]. Peers reap idle connections themselves — omniORB
//!   scans outgoing connections on `outConScanPeriod` — and discovering their
//!   decision mid-request costs a failed call, while making our own decision
//!   first costs a dial. Idle means *no calls in flight and nothing since*,
//!   both read from the mux's atomics.
//! - **It was closed under us.** See below.
//! - **The bound was reached and it was the least recently used idle one.**
//!
//! # `CloseConnection`, and why a caller must not see it
//!
//! §13.5.1: *"If a client receives a CloseConnection message from the server,
//! it should assume that any outstanding messages (i.e., without replies) were
//! received after the server sent the CloseConnection message, were not
//! processed, and may be safely resent on a new connection."*
//!
//! That is a promise about **processing**, not about idempotence, so it
//! applies to every operation and not only the safe ones. A pooled connection
//! is one the caller never asked for and cannot see, so a server closing it —
//! which every ORB does to idle connections — must not surface as their error.
//! [`Pool::invoke`] therefore discards the connection and re-sends **once** on
//! a fresh one.
//!
//! Once, not until it works. A server that answers every request with
//! `CloseConnection` is refusing, and a retry loop against it is a hot loop
//! that never reports the refusal. The second failure is the caller's.
//!
//! One close does **not** get the retry, and the exception is the whole reason
//! the flag is per caller: a `CloseConnection` that arrives between the
//! *fragments* of a reply (§9.4.9) leaves one request whose answer had already
//! started coming back. §13.5.1's promise is about outstanding messages
//! *without replies*, so it does not reach that one — the peer had processed
//! it — and re-sending it here would run a non-idempotent operation twice to
//! hide an error the caller would rather have seen. It surfaces as
//! [`Error::InterruptedMidReassembly`]; the other callers on that connection
//! still get their re-send.
//!
//! The same one retry covers a **failed write**, and for the same
//! specification-grade reason rather than optimism: a GIOP message the peer
//! never finished reading cannot have been dispatched, and the connection is
//! discarded, so the rest of those bytes will never arrive. That is exactly
//! what [`crate::mux::Failed::unsent`] reports, and the pool retries on that
//! flag rather than on a list of error kinds it would have to keep in sync.
//!
//! # The bound
//!
//! An unbounded pool is a file-descriptor leak with a nicer name, so:
//!
//! - [`Limits::max_per_endpoint`] connections to any one endpoint. Reaching it
//!   is not an error — with multiplexing, another caller simply shares an
//!   existing connection, which is the whole point.
//! - [`Limits::max_total`] connections overall, defaulting to
//!   [`DEFAULT_MAX_TOTAL`], the same number [`crate::server::DEFAULT_MAX_CONNECTIONS`]
//!   bounds the serving side with. When a *new* endpoint needs a connection
//!   and the pool is full, idle connections are evicted to make room; if none
//!   are idle the dial is **refused** with [`Error::PoolExhausted`].
//!
//! Refusing rather than queueing is the same choice [`crate::server`] made at
//! its own cap, and for the same reason: a caller told "no capacity" can shed
//! load or back off, while a caller queued behind an unknown number of others
//! cannot tell a slow peer from a leak. Dialing anyway would make the bound a
//! suggestion, which is the leak this bound exists to prevent.
//!
//! # Why there is no checkout, and no lease to leak
//!
//! Because connections multiplex, the pool is a **cache**, not a checkout
//! desk: several callers hold the same [`Mux`] at once and none of them owns
//! it. There is nothing to return, so there is no return-on-drop path, no
//! "leased but never returned" state, and no way for a panicking caller to
//! lose a connection. On a connection that cannot multiplex the mux's own
//! turnstile serializes the callers, so the shape of the API does not change
//! with the version — only the concurrency does.
//!
//! # The lock rule
//!
//! [`Connection::connect`] asserts that no lock section is held, because a
//! dial can block for the whole connect timeout and a lock held across it is
//! the cross-process deadlock [`crate::guarded`] exists to prevent. A pool is
//! the obvious place to violate that — "look up the endpoint, and dial while
//! I am in here if I do not find one" is the natural way to write it, and it
//! is wrong.
//!
//! So the pool's own state is a [`Guarded`], and dialing inside it does not
//! merely break a convention: [`crate::guarded::assert_nothing_held`] fires
//! inside `connect` and the test that would have introduced it panics. The
//! shape that keeps it true is three steps — **look, then dial, then file**:
//!
//! 1. under the lock, look for a usable connection; if there is none, *reserve*
//!    a slot against the bound and release the lock;
//! 2. **outside** the lock, dial;
//! 3. under the lock again, file the new connection and release the
//!    reservation.
//!
//! The reservation is what makes step 2 safe to do without the lock: it counts
//! against [`Limits::max_total`] while nothing is in the map yet, so two
//! threads dialing at once cannot both discover room that only one of them
//! could have. Evicted connections are moved out of the map and dropped
//! **after** the lock is released, because closing a socket is a syscall and
//! the rule is that nothing which can block happens inside a section.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use orbweaver_cdr::{Encoder, Endian};

use crate::guarded::Guarded;
use crate::mux::{Failed, Mux, Sent};
use crate::{
    Connection, Error, Forward, Invoker, Ior, Reply, Result, Version, codeset,
    negotiated_char_converter,
};

/// Default ceiling on connections held across every endpoint.
///
/// The same number [`crate::server::DEFAULT_MAX_CONNECTIONS`] uses for the
/// serving side, deliberately: a process that will accept 64 connections and
/// hold 64 more has one number to reason about, not two.
pub const DEFAULT_MAX_TOTAL: usize = 64;

/// Default ceiling on connections to any one endpoint.
///
/// More than one only helps where a connection cannot multiplex — GIOP below
/// 1.2, or TLS — and there it is the difference between concurrency and a
/// queue. On a multiplexing connection the pool stops at one until
/// [`Limits::soft_in_flight`] says otherwise.
pub const DEFAULT_MAX_PER_ENDPOINT: usize = 4;

/// Default number of calls that may pile onto one connection before the pool
/// prefers to dial another.
///
/// Not a limit — going over it is fine and happens whenever the endpoint is at
/// [`Limits::max_per_endpoint`]. It is the point at which spending a file
/// descriptor is likely cheaper than sharing a socket, and it is a guess:
/// nothing here has been tuned against a peer under load, and saying so is
/// cheaper than pretending the number is measured.
pub const DEFAULT_SOFT_IN_FLIGHT: usize = 8;

/// Default idle period after which a pooled connection is dropped.
///
/// Shorter than the peers' own idle scans (omniORB's `outConScanPeriod`
/// defaults to two minutes) so that *we* decide, deterministically, rather
/// than finding out mid-request that the peer decided. The cost of being early
/// is a dial; the cost of being late is a failed call and a retry.
pub const DEFAULT_MAX_IDLE: Duration = Duration::from_secs(20);

/// How long a pooled dial waits for the socket.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// What a pool will and will not hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Connections across all endpoints. The file-descriptor bound.
    pub max_total: usize,
    /// Connections to any one endpoint.
    pub max_per_endpoint: usize,
    /// Calls on one connection before another is preferred.
    pub soft_in_flight: usize,
    /// Idle period after which a connection is dropped.
    pub max_idle: Duration,
    /// Timeout for a pooled dial.
    pub connect_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_total: DEFAULT_MAX_TOTAL,
            max_per_endpoint: DEFAULT_MAX_PER_ENDPOINT,
            soft_in_flight: DEFAULT_SOFT_IN_FLIGHT,
            max_idle: DEFAULT_MAX_IDLE,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
        }
    }
}

/// What makes two references able to share a connection.
///
/// See the module docs for why each field is here. `Hash + Eq` because it is a
/// map key, and `Clone` because the pool answers with it — a caller that has
/// acquired a connection often wants to say which one it got.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Key {
    /// Host as the profile spells it. Two spellings of one address ("localhost"
    /// and "127.0.0.1") are deliberately *not* unified: resolving them here
    /// would mean a DNS lookup inside the pool's lock, and the cost of being
    /// wrong is one extra connection rather than a wrong one.
    pub host: String,
    /// TCP port.
    pub port: u16,
    /// The version this connection speaks, already negotiated down from the
    /// profile's advertisement.
    pub version: Version,
    /// The negotiated `char` codeset, or `None` for §7.10.2.5's default.
    pub codeset: Option<codeset::CodeSetId>,
}

impl Key {
    /// The key a connection dialed for this profile would land under.
    pub fn of(profile: &crate::IiopProfile, host: &str, port: u16) -> Key {
        Key {
            host: host.to_owned(),
            port,
            version: Version::negotiate(profile.version),
            codeset: negotiated_char_converter(profile).map(|c| c.id()),
        }
    }
}

/// Counters, so a claim about reuse can be checked rather than asserted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PoolStats {
    /// Connections dialed. The number reuse is supposed to hold down.
    pub dialed: u64,
    /// Times an existing connection was handed out instead of dialing.
    pub reused: u64,
    /// Connections dropped because they had gone idle.
    pub idle_evicted: u64,
    /// Connections dropped because they had faulted.
    pub faulted_evicted: u64,
    /// Connections dropped to make room under [`Limits::max_total`].
    pub pressure_evicted: u64,
    /// Calls re-sent on a fresh connection after the peer proved it had not
    /// processed them.
    pub retried: u64,
    /// Dials refused because the pool was full and nothing was evictable.
    pub refused: u64,
}

#[derive(Debug, Default)]
struct State {
    live: HashMap<Key, Vec<Mux>>,
    /// Dials in progress, counted against the bound while nothing is in the
    /// map yet. Without this two threads both find room for the last slot.
    reserved: usize,
    stats: PoolStats,
}

impl State {
    fn total(&self) -> usize {
        self.live.values().map(Vec::len).sum::<usize>() + self.reserved
    }
}

/// A cache of connections, keyed by [`Key`].
///
/// Cheap to clone; a clone shares the same connections. `Send + Sync`, so one
/// pool serves every thread in a process.
/// # Why this is not `Default`
///
/// It was, and a derived `Default` on a `pub` struct is a **public
/// constructor** — one that hands back a pool configured with nothing. Closing
/// [`Pool::new`] while leaving `Pool::default()` standing would have left the
/// door D019 step 4 exists to close, with a different name on it. A pool comes
/// from [`Orb::pool`](crate::orb::Orb::pool).
///
/// *파생된 `Default`도 공개 생성자다. 이름만 다른 같은 문이다.*
#[derive(Debug, Clone)]
pub struct Pool {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    limits: Limits,
    /// The deployment's numbers, applied to every connection this pool dials.
    /// Defaults answer the compiled constants, so an unconfigured pool behaves
    /// exactly as it did before D019 step 4.
    config: crate::orb::OrbConfig,
    state: Guarded<State>,
    /// Raised by [`Pool::close`] and by
    /// [`Orb::shutdown`](crate::orb::Orb::shutdown) (D034 §6). Deliberately the
    /// same [`StopFlag`](crate::server::StopFlag) type the server side uses:
    /// one flag, raised once, never lowered, with no I/O between raising it and
    /// the next `acquire` seeing it.
    closed: crate::server::StopFlag,
}

/// A non-owning view of a [`Pool`], for the ORB's list of handouts.
///
/// Weak rather than a clone for the same reason the server side is weak: an ORB
/// that kept a strong reference to every pool it ever handed out would keep
/// their sockets alive after the last real holder let go, which is a leak
/// dressed as a lifecycle.
#[derive(Debug)]
pub(crate) struct PoolWatch {
    inner: std::sync::Weak<Inner>,
}

impl PoolWatch {
    /// Closes the pool if it is still alive; answers whether it was.
    ///
    /// A dead `Weak` means every holder has let go, which closed the sockets
    /// already — so there is nothing for a shutdown to do and nothing it
    /// failed to do.
    pub(crate) fn close(&self) -> bool {
        match self.inner.upgrade() {
            Some(inner) => {
                Pool { inner }.close();
                true
            }
            None => false,
        }
    }
}

impl Pool {
    /// A pool with limits and a deployment's configuration (D019 step 4).
    ///
    /// `pub(crate)`, and the only constructor: a [`Pool`] comes from
    /// [`Orb::pool`](crate::orb::Orb::pool). See [`Server::bind`] for why the
    /// second door is closed rather than merely documented — a pool built
    /// beside the ORB dialled every connection with the compiled defaults, and
    /// that was true no matter what the operator had configured.
    ///
    /// [`Server::bind`]: crate::server::Server::bind
    pub(crate) fn with_limits_and_config(limits: Limits, config: crate::orb::OrbConfig) -> Pool {
        Pool {
            inner: Arc::new(Inner {
                limits,
                config,
                state: Guarded::new("the connection pool", State::default()),
                closed: crate::server::StopFlag::default(),
            }),
        }
    }

    /// A non-owning view of this pool, for the ORB's list of handouts.
    pub(crate) fn watch(&self) -> PoolWatch {
        PoolWatch { inner: Arc::downgrade(&self.inner) }
    }

    /// Stops this pool: **no further connection is dialled, and every pooled
    /// connection is dropped** (D034 §6).
    ///
    /// Idempotent, and cloning does not escape it — a `Pool` clone shares the
    /// same `Inner`, so closing through one clone closes all of them.
    ///
    /// # What this does not do
    ///
    /// **A call already in flight on a [`Mux`] a caller holds is not aborted.**
    /// The caller owns that call; aborting it is the immediate shutdown D034 §4
    /// refuses, one layer down, and it has the same problem — there is nothing
    /// honest to put on the wire for a call that was started and abandoned. It
    /// completes, or it fails on the timeout it already had. What close
    /// guarantees is narrower and sayable: **nobody new gets a connection, and
    /// nothing new is dialled.**
    ///
    /// # Where the sockets close
    ///
    /// Outside the [`Guarded`] section, via [`Pool::clear`], for the reason
    /// that module exists: `guarded::assert_nothing_held` is called by
    /// `Connection::connect` and by every `invoke`, and closing is a syscall.
    /// A close that ran inside a section would be the exact violation
    /// [`crate::guarded`] was written to catch.
    ///
    /// *닫힌 뒤에는 아무도 새 연결을 얻지 못하고 새로 걸지도 않는다. 이미 진행 중인
    /// 호출은 중단하지 않는다 — 시작해 놓고 버린 호출에 대해 와이어에 정직하게 쓸
    /// 말이 없기 때문이다.*
    pub fn close(&self) {
        // Raised before the map is emptied, never after: between the two, an
        // `acquire` that had already passed the check could otherwise file a
        // freshly dialled connection into a map we had just emptied, and that
        // connection would outlive the close with nothing left to drop it.
        self.inner.closed.raise();
        self.clear();
    }

    /// Whether [`Pool::close`] — or the [`Orb`](crate::orb::Orb) that handed
    /// this pool out — has stopped it.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.raised()
    }

    /// The limits in force.
    pub fn limits(&self) -> Limits {
        self.inner.limits
    }

    /// The counters.
    pub fn stats(&self) -> PoolStats {
        self.inner.state.read(|s| s.stats)
    }

    /// How many connections are held right now.
    pub fn size(&self) -> usize {
        self.inner.state.read(|s| s.total())
    }

    /// Drops every connection, whether idle or not.
    ///
    /// The sockets are closed after the lock is released, for the same reason
    /// eviction defers them: closing is a syscall and nothing that can block
    /// belongs inside a section.
    pub fn clear(&self) {
        let dropped = self.inner.state.write(|s| std::mem::take(&mut s.live));
        drop(dropped);
    }

    /// A connection able to carry calls for `ior`, dialing one if the pool has
    /// none.
    ///
    /// Answers with the object key as well, because the connection is shared
    /// and the key is what makes a call address *this* reference.
    pub fn acquire(&self, ior: &Ior) -> Result<(Mux, Vec<u8>)> {
        let object_key = ior.primary()?.object_key.clone();
        Ok((self.acquire_connection(ior)?, object_key))
    }

    fn acquire_connection(&self, ior: &Ior) -> Result<Mux> {
        // The one choke point (D034 §6): `acquire`, `invoke`, `invoke_with`,
        // `invoke_tracking`, `invoke_oneway` and every `Reference` call funnel
        // through here, so refusing here is refusing all of them. Checked
        // before anything is dialled and before the section is entered.
        if self.inner.closed.raised() {
            return Err(Error::Stopped { what: "a pooled connection" });
        }
        // Every endpoint this IOR names, in the order `Connection::connect`
        // would dial them, so a pooled hit on a later endpoint counts as a hit
        // rather than sending us to dial the first one again.
        let mut keys = Vec::new();
        for p in &ior.profiles {
            for (host, port) in p.endpoints() {
                keys.push(Key::of(p, &host, port));
            }
        }
        if keys.is_empty() {
            return Err(Error::NoIiopProfile);
        }

        // ── step 1: look, and reserve if there is nothing to look at ──
        let limits = self.inner.limits;
        let (found, evicted) = self.inner.state.write(|s| {
            let evicted = sweep(s, limits);
            for key in &keys {
                if let Some(m) = pick(s, key, limits) {
                    s.stats.reused += 1;
                    return (Some(Ok(m)), evicted);
                }
            }
            // Nothing usable. Make room, then take a slot for the dial.
            if s.total() >= limits.max_total {
                let want = s.total() + 1 - limits.max_total;
                let mut freed = evicted;
                freed.extend(evict_idle(s, want));
                if s.total() >= limits.max_total {
                    s.stats.refused += 1;
                    return (Some(Err(Error::PoolExhausted { limit: limits.max_total })), freed);
                }
                s.reserved += 1;
                return (None, freed);
            }
            s.reserved += 1;
            (None, evicted)
        });
        // Sockets close here, outside the section.
        drop(evicted);
        if let Some(outcome) = found {
            return outcome;
        }

        // ── step 2: dial with nothing held. `connect` asserts exactly that ──
        // The deployment's numbers go on before the connection is used or
        // wrapped: `Mux::over` moves them out of the `Connection` wholesale and
        // there is no setter on a `Mux`, so this is the only moment they can be
        // applied. Before D019 step 4 nothing applied them here at all, and a
        // pooled connection therefore ran on the compiled defaults however the
        // ORB had been configured.
        let dialed = Connection::connect(ior, limits.connect_timeout).map(|mut conn| {
            conn.apply_orb_config(&self.inner.config);
            conn
        });

        // ── step 3: file it, or give the reservation back ──
        match dialed {
            Ok(conn) => {
                let (host, port) = conn.endpoint();
                // Keyed on what was actually reached: failover may have landed
                // on a later profile or an alternate address.
                let key = match ior
                    .profiles
                    .iter()
                    .find(|p| p.endpoints().iter().any(|(h, o)| h == host && *o == port))
                {
                    Some(p) => Key::of(p, host, port),
                    // Cannot happen — the connection came from these profiles
                    // — but inventing a key from the primary profile would
                    // file it under an agreement it does not have, so refuse
                    // to pool it instead. It still serves this call.
                    None => {
                        self.inner.state.write(|s| s.reserved = s.reserved.saturating_sub(1));
                        return Ok(Mux::over(conn));
                    }
                };
                let mux = Mux::over(conn);
                self.inner.state.write(|s| {
                    s.reserved = s.reserved.saturating_sub(1);
                    s.stats.dialed += 1;
                    s.live.entry(key).or_default().push(mux.clone());
                });
                Ok(mux)
            }
            Err(e) => {
                self.inner.state.write(|s| s.reserved = s.reserved.saturating_sub(1));
                Err(e)
            }
        }
    }

    /// Drops one connection from the pool. Other holders keep using it until
    /// they let go; what this guarantees is that nobody *new* gets it.
    pub fn discard(&self, mux: &Mux) {
        let dropped = self.inner.state.write(|s| {
            let mut dropped = Vec::new();
            for muxes in s.live.values_mut() {
                let mut i = 0;
                while i < muxes.len() {
                    if Mux::same_connection(&muxes[i], mux) {
                        dropped.push(muxes.remove(i));
                    } else {
                        i += 1;
                    }
                }
            }
            s.live.retain(|_, v| !v.is_empty());
            dropped
        });
        drop(dropped);
    }

    /// Invokes `operation` on `ior`, over a pooled connection.
    ///
    /// Follows `LOCATION_FORWARD` and `LOCATION_FORWARD_PERM` alike, up to
    /// [`MAX_FORWARD_HOPS`], acquiring a connection for each new reference — a
    /// forward may point at a different host, so it is a new pool lookup and
    /// not a redirect of this one. Which of the two was followed is
    /// [`Pool::invoke_tracking`]'s to report. Hides exactly one class of
    /// failure from the caller: a request the peer proved it had not
    /// processed, which is re-sent once on a fresh connection.
    pub fn invoke<F>(&self, ior: &Ior, operation: &str, write_args: F) -> Result<Reply>
    where
        F: Fn(&mut Encoder),
    {
        self.invoke_with(ior, operation, write_args, crate::mux::DEFAULT_CALL_TIMEOUT)
    }

    /// As [`Pool::invoke`], with the caller's own deadline per attempt.
    pub fn invoke_with<F>(
        &self,
        ior: &Ior,
        operation: &str,
        write_args: F,
        timeout: Duration,
    ) -> Result<Reply>
    where
        F: Fn(&mut Encoder),
    {
        self.invoke_tracking(ior, operation, write_args, timeout).map(|(reply, _)| reply)
    }

    /// As [`Pool::invoke_with`], and says which redirect the call followed.
    ///
    /// The pool follows a `LOCATION_FORWARD` and a `LOCATION_FORWARD_PERM`
    /// identically — both are §9.4.3.2's "retry there" — so from the `Reply`
    /// alone a caller cannot tell that a redirect happened, let alone whether
    /// the servant said *permanent*. This is how it can tell: the second half
    /// is the last hop the call took, `None` when it was answered where it was
    /// sent. A [`Forward::Permanent`] is the servant's leave to replace the
    /// reference the caller holds; a [`Forward::Temporary`] is not, and a
    /// caller that republishes a temporary target as the object's address is
    /// publishing something the servant did not say. The pool itself replaces
    /// nothing: it holds connections, not references, and the next call is
    /// sent to whatever `ior` the caller passes.
    ///
    /// Only the last hop of a chain is reported, as [`Connection::forwarded`]
    /// reports it: it is the one that answered. What the *rest* of the chain
    /// meant for the reference is not this — [`Reference`] is told the whole
    /// chain, hop by hop, and this is the summary for everybody else.
    pub fn invoke_tracking<F>(
        &self,
        ior: &Ior,
        operation: &str,
        write_args: F,
        timeout: Duration,
    ) -> Result<(Reply, Option<Forward>)>
    where
        F: Fn(&mut Encoder),
    {
        self.attempt(ior, operation, &write_args, timeout)
            .map(|(reply, chain)| (reply, chain.last))
            .map_err(|f| f.error)
    }

    /// [`Pool::invoke_tracking`], keeping the one fact a caller that holds
    /// the *original* reference needs when the call fails: whether the
    /// request provably did not run ([`Failed::unsent`]), so it may be
    /// issued again somewhere else — which is what [`Reference`] does under
    /// §9.6 when a temporarily forwarded-to endpoint has gone.
    ///
    /// A dial that fails is `unsent` too: nothing was written. The retry
    /// this pool spends on §13.5.1's promise is spent in here, so a failure
    /// that comes out `unsent` has already been tried twice on this target.
    fn attempt<F>(
        &self,
        ior: &Ior,
        operation: &str,
        write_args: &F,
        timeout: Duration,
    ) -> std::result::Result<(Reply, Chain), Failed>
    where
        F: Fn(&mut Encoder),
    {
        let mut target = ior.clone();
        let mut chain = Chain::default();
        let mut retried = false;
        for _ in 0..self.inner.config.max_forward_hops() {
            let (mux, key) =
                self.acquire(&target).map_err(|error| Failed { error, unsent: true })?;
            match mux.call_on(&key, operation, write_args, timeout) {
                Ok(Sent::Reply(reply)) => return Ok((*reply, chain)),
                Ok(Sent::Forward(next)) => {
                    target = next.ior().clone();
                    chain.hop(*next);
                }
                Err(Failed { error, unsent }) => {
                    // The connection is out either way: a fault means it can
                    // no longer be framed, and an `unsent` failure means the
                    // peer has gone or is going.
                    if !mux.is_usable() {
                        self.discard(&mux);
                    }
                    if unsent && !retried {
                        // §13.5.1's promise, spent exactly once. See the
                        // module docs for why it is not a loop.
                        retried = true;
                        self.inner.state.write(|s| s.stats.retried += 1);
                        continue;
                    }
                    return Err(Failed { error, unsent });
                }
            }
        }
        Err(Failed { error: Error::TooManyForwards, unsent: false })
    }

    /// Invokes a `oneway` over a pooled connection.
    ///
    /// The same one retry applies and means slightly less here: a oneway that
    /// was written has no reply to prove anything, so all this can say is that
    /// the bytes left. That is [`crate::Connection::invoke_oneway`]'s promise
    /// too.
    pub fn invoke_oneway<F>(&self, ior: &Ior, operation: &str, write_args: F) -> Result<()>
    where
        F: Fn(&mut Encoder),
    {
        let mut retried = false;
        loop {
            let (mux, key) = self.acquire(ior)?;
            match mux.call_oneway(&key, operation, &write_args) {
                Ok(()) => return Ok(()),
                Err(Failed { error, unsent }) => {
                    if !mux.is_usable() {
                        self.discard(&mux);
                    }
                    if unsent && !retried {
                        retried = true;
                        self.inner.state.write(|s| s.stats.retried += 1);
                        continue;
                    }
                    return Err(error);
                }
            }
        }
    }

    /// A reference bound to this pool, for code that wants an [`Invoker`].
    ///
    /// Each call makes a **new** object reference: two of them for the same
    /// IOR are two references that happen to name one object, and a permanent
    /// forward taken by one says nothing about the other. Cloning is the
    /// other operation — a clone is another handle on the *same* reference
    /// and shares where the object is. See [`Reference`].
    pub fn reference(&self, ior: Ior) -> Reference {
        Reference {
            pool: self.clone(),
            moved: Arc::new(Guarded::new(ADDRESS, ior.clone())),
            ior,
            endian: Endian::native(),
            via: None,
            forwarded: None,
        }
    }
}

/// What the lock discipline calls a reference's shared address, when it has
/// something to say about it. Named for the fact, not the type: a complaint
/// that says "Guarded" names nothing a reader can act on.
const ADDRESS: &str = "an object reference's address";

/// Drops faulted and idle connections. Returns them for the caller to drop
/// outside the lock.
fn sweep(s: &mut State, limits: Limits) -> Vec<Mux> {
    let mut dropped = Vec::new();
    for muxes in s.live.values_mut() {
        let mut i = 0;
        while i < muxes.len() {
            let m = &muxes[i];
            // Both questions are answered from atomics — taking the mux's own
            // lock in here would be a second section and the tripwire would
            // fire. See `mux`'s module docs.
            let faulted = !m.is_usable();
            let idle = m.in_flight() == 0 && m.idle_for() > limits.max_idle;
            if faulted {
                s.stats.faulted_evicted += 1;
                dropped.push(muxes.remove(i));
            } else if idle {
                s.stats.idle_evicted += 1;
                dropped.push(muxes.remove(i));
            } else {
                i += 1;
            }
        }
    }
    s.live.retain(|_, v| !v.is_empty());
    dropped
}

/// Drops up to `want` idle connections to make room, least recently used
/// first. Busy connections are never taken: a caller is on them.
fn evict_idle(s: &mut State, want: usize) -> Vec<Mux> {
    let mut dropped = Vec::new();
    for _ in 0..want {
        let mut oldest: Option<(Key, usize, Duration)> = None;
        for (key, muxes) in s.live.iter() {
            for (i, m) in muxes.iter().enumerate() {
                if m.in_flight() != 0 {
                    continue;
                }
                let idle = m.idle_for();
                if oldest.as_ref().is_none_or(|(_, _, best)| idle > *best) {
                    oldest = Some((key.clone(), i, idle));
                }
            }
        }
        match oldest {
            Some((key, i, _)) => {
                if let Some(muxes) = s.live.get_mut(&key) {
                    dropped.push(muxes.remove(i));
                    s.stats.pressure_evicted += 1;
                }
            }
            None => break,
        }
    }
    s.live.retain(|_, v| !v.is_empty());
    dropped
}

/// Picks a connection for `key`, or `None` if one should be dialed.
///
/// Prefers the least loaded, and prefers dialing over piling calls onto a busy
/// connection until the endpoint is at its own bound — at which point sharing
/// is the answer, because that is what multiplexing is for.
fn pick(s: &State, key: &Key, limits: Limits) -> Option<Mux> {
    let muxes = s.live.get(key)?;
    let best = muxes
        .iter()
        .filter(|m| m.is_usable())
        .min_by_key(|m| m.in_flight())
        .cloned()
        .filter(|m| m.multiplexes() || m.in_flight() == 0)?;
    if best.in_flight() >= limits.soft_in_flight && muxes.len() < limits.max_per_endpoint {
        return None; // room to spread the load; dial instead
    }
    Some(best)
}

/// What one call's forward chain left behind, hop by hop.
///
/// [`Pool::attempt`] follows every hop of a chain, and the hops are not all
/// the same kind: a servant may forward permanently to another one that then
/// forwards temporarily, and the two hops mean different things to the
/// reference that made the call. §9.6's *may replace the old IOR* belongs to
/// the permanent hop; a temporary hop that comes **after** it is forwarding
/// information about *that* reference, not about the one the caller started
/// with.
///
/// Reporting only the last hop — which is all [`Pool::invoke_tracking`]
/// answers with, because it is the hop that answered — loses exactly that
/// distinction, and the loss is not free: a `permanent → temporary` chain
/// left `Reference::ior` untouched and cached the temporary target against
/// it, so §9.6's restart went back through a hop the servant had already
/// told the client to stop using. Within the spec, and one hop more than
/// needed. [`Connection`] never had it, because it applies each hop as it
/// takes it (`Connection::follow`); this is the same rule, accumulated
/// across a chain the pool walks in one call.
#[derive(Debug, Clone, Default)]
struct Chain {
    /// The last permanent hop: what the reference re-points to.
    repoint: Option<Ior>,
    /// The forwarding information in force at the end of the chain — the
    /// last temporary hop, and only while no permanent hop came after it.
    via: Option<Ior>,
    /// The last hop of any kind: the one that answered, and what
    /// [`Reference::forwarded`] reports.
    last: Option<Forward>,
}

impl Chain {
    /// Takes one hop, by the rule `Connection::follow` applies to one.
    fn hop(&mut self, forward: Forward) {
        match &forward {
            Forward::Temporary(to) => self.via = Some(to.clone()),
            Forward::Permanent(to) => {
                self.repoint = Some(to.clone());
                // Forwarding information cached before a permanent hop is
                // information about an address the servant has just
                // superseded; keeping it would make the restart aim there.
                self.via = None;
            }
        }
        self.last = Some(forward);
    }
}

/// A reference plus the pool that will carry its calls, so generated stubs —
/// which are generic over [`Invoker`] — run over pooled, multiplexed
/// connections without knowing it.
///
/// # Forwards, and §9.6
///
/// The pool holds connections, not references, so what a redirect means for
/// *the reference* is decided here. Before this was decided, every call went
/// to [`Reference::ior`] and was forwarded afresh — two round trips per call
/// for as long as the forward stood, measured at two requests at the
/// original for two calls (`crates/orbweaver-gen/tests/forward_fallback.rs`,
/// before). Now the reference does what CORBA 3.4 Part 2 §9.6 describes,
/// and what [`Connection`] does one layer down:
///
/// - a **temporary** forward is cached as forwarding information: the next
///   call goes straight to where the forward pointed. When that fails and
///   the request provably did not run — the dial failed, or the peer said
///   `CloseConnection` on both the pool's tries (see [`Failed::unsent`]) —
///   the cache is dropped and the call is issued again at `ior`: *"if that
///   fails, it shall restart the location process using the original
///   address specified in the initial object reference"*. A failure whose
///   completion is unknown is returned as it is and drops nothing; the
///   caller decides.
/// - a **permanent** forward re-points the reference: `ior` becomes the
///   forwarded-to IOR — the *may* of §9.6, taken up as
///   [`Connection::origin`] takes it up.
///
/// Both rules are applied **per hop**, over the whole chain one call
/// followed, and not to the last hop alone: a chain that goes
/// permanent-then-temporary re-points `ior` at the permanent hop *and*
/// caches the temporary one relative to it, so a restart returns to the
/// reference as it now stands rather than through a hop the servant has
/// already superseded. That is the hop this saves, and it is the difference
/// between the two orderings: temporary-then-permanent ends re-pointed with
/// nothing cached, because a permanent hop clears the forwarding
/// information it replaces.
///
/// # A clone is another handle, not another reference
///
/// **Where the object is, is shared by every clone**; the routing state a
/// handle built up on the way there is not.
///
/// A permanent forward is the servant saying *the object moved*, and that is
/// a fact about the object rather than about whichever handle happened to
/// make the call that heard it. Every language mapping already reads it that
/// way — C++'s `_duplicate` refcounts one proxy and Java's `_duplicate`
/// shares one `Delegate`, so a permanent forward taken through a duplicate
/// is seen by all of them — and `Clone` is this crate's `_duplicate`. Two
/// [`Pool::reference`] calls, on the other hand, are two references.
///
/// Before this was shared, a clone re-pointed only itself, and §9.6 made the
/// disagreement silent: the old address stays valid, so a stale clone's
/// calls are answered — one forward per call, forever, with nothing to go
/// red. The pattern that pays it is the one this API asks for, not an
/// exotic one: [`Invoker::invoke`] takes `&mut self`, so a reference used
/// from more than one caller has to be cloned, and a template that is cloned
/// per call never makes a call itself and so never re-points. Measured at
/// three calls off one template: three requests at the original before, one
/// after (`tests/forward_clone.rs`).
///
/// What stays per handle, and why:
///
/// - **the temporary forwarding cache** (`via`). §9.6 keeps the original
///   address authoritative for a temporary hop, so the cache is routing
///   state and not the object's address; a handle that lacks it pays one
///   forward and then has it, which is self-correcting in a way a stale
///   permanent address is not.
/// - **[`Reference::forwarded`]**, which answers "what did *my* last call
///   follow". Sharing it would let one thread's call rewrite another
///   thread's answer to a question about its own call.
/// - **the byte order** ([`Reference::set_endian`]), which is advisory and
///   about how this handle encodes.
///
/// The cost is one shared-mode `Guarded` read per two-way call and per
/// [`Reference::ior`], and an exclusive one only when a call's chain
/// actually contained a permanent hop. It is never held across the wire —
/// [`crate::guarded`]'s tripwire fires if it is, which is how that sentence
/// is checked rather than asserted.
#[derive(Debug)]
pub struct Reference {
    pool: Pool,
    /// Where the object is: shared by every clone of this reference, written
    /// only by a permanent hop. See the type's docs.
    moved: Arc<Guarded<Ior>>,
    /// `moved` as this handle last read it, refreshed at every point it is
    /// used or shown — [`Reference::refresh`]. A copy and not the cell
    /// itself because [`Reference::ior`] lends it out, and a borrow may not
    /// escape a lock section.
    ior: Ior,
    endian: Endian,
    /// The forwarding information a temporary forward left: where the next
    /// call goes instead of `ior`, until it fails unsent. See the type's
    /// docs.
    via: Option<Ior>,
    /// The redirect in force. See [`Reference::forwarded`].
    forwarded: Option<Forward>,
}

/// A clone is another handle on the same reference: it shares where the
/// object is, and starts from this handle's routing state — read as of now,
/// so a clone taken off a handle that has not called since the object moved
/// is not born stale.
impl Clone for Reference {
    fn clone(&self) -> Reference {
        let mut copy = Reference {
            pool: self.pool.clone(),
            moved: Arc::clone(&self.moved),
            ior: self.ior.clone(),
            endian: self.endian,
            via: self.via.clone(),
            forwarded: self.forwarded.clone(),
        };
        copy.refresh();
        copy
    }
}

impl Reference {
    /// The reference being called: what it was created with, or the last
    /// permanent forward — including one taken by a clone.
    ///
    /// `&mut self` because the answer is read out of state shared with every
    /// clone and this handle's copy of it is refreshed here. An accessor that
    /// took `&self` could only answer with what this handle last saw, and a
    /// handle that has not called since another clone was re-pointed would
    /// then name an address its own next call is not going to use.
    pub fn ior(&mut self) -> &Ior {
        self.refresh();
        &self.ior
    }

    /// Takes up a re-pointing another clone made.
    ///
    /// The shared read is the whole of the lock cost on the call path, and it
    /// closes before anything blocks: `Guarded::read` returns the value out of
    /// the closure rather than lending a guard, so there is no shape in which
    /// this is held across a dial.
    fn refresh(&mut self) {
        let at = self.moved.read(Ior::clone);
        if at != self.ior {
            self.ior = at;
            // Forwarding information cached against the address the object
            // has just left is information about a superseded address — the
            // rule `Chain::hop` applies within one call, applied across
            // handles.
            self.via = None;
            self.forwarded = None;
        }
    }

    /// Sets the byte order requests are encoded in. Advisory: the connection
    /// was dialed with its own, and this only affects connections dialed
    /// afterwards.
    pub fn set_endian(&mut self, endian: Endian) {
        self.endian = endian;
    }

    /// The redirect in force, and whether the servant said it was for good.
    ///
    /// The pooled counterpart of [`Connection::forwarded`], and it now reads
    /// the same way: where the reference's calls *are* going. `None` until a
    /// call is redirected; [`Forward::Temporary`] while that forwarding
    /// information is cached — the calls it carries are answered where they
    /// are sent and it stays — and back to `None` when a §9.6 restart lands
    /// a call at [`Reference::ior`] again; [`Forward::Permanent`] once the
    /// reference has been re-pointed, until the next redirect. A oneway
    /// carries no reply and cannot be forwarded, and a call whose failure
    /// drops nothing leaves this as it was.
    pub fn forwarded(&self) -> Option<&Forward> {
        self.forwarded.as_ref()
    }

    /// Records what a two-way call's chain came to. An empty chain —
    /// answered where it was sent — changes nothing: a cached temporary
    /// forward stays cached, a re-pointed reference stays re-pointed.
    ///
    /// The order is the chain's own: re-point first, then cache, because a
    /// temporary hop that survived the accumulation is one that came after
    /// the permanent one and is forwarding information *for the re-pointed
    /// reference*.
    fn note(&mut self, chain: Chain) {
        if let Some(to) = chain.repoint {
            // The object moved, so every clone is told, not only this handle.
            self.moved.write(|at| *at = to.clone());
            self.ior = to;
            self.via = None;
        }
        if let Some(to) = chain.via {
            self.via = Some(to);
        }
        if chain.last.is_some() {
            self.forwarded = chain.last;
        }
    }
}

impl Invoker for Reference {
    fn endian(&self) -> Endian {
        self.endian
    }

    fn invoke<F: Fn(&mut Encoder)>(&mut self, operation: &str, write_args: F) -> Result<Reply> {
        let timeout = crate::mux::DEFAULT_CALL_TIMEOUT;
        // Before anything is addressed: another clone may have been told the
        // object moved, and a call sent to the address it left is the forward
        // this reference exists to stop paying.
        self.refresh();
        if let Some(via) = self.via.clone() {
            match self.pool.attempt(&via, operation, &write_args, timeout) {
                Ok((reply, chain)) => {
                    self.note(chain);
                    return Ok(reply);
                }
                Err(Failed { error, unsent: false }) => return Err(error),
                Err(Failed { unsent: true, .. }) => {
                    // The forwarding information failed and the request did
                    // not run: §9.6's restart, in this same call, at the
                    // reference as it now stands — which is the last
                    // permanent hop when the chain had one, and the address
                    // the caller started from when it did not. The dropped
                    // error is the target's refusal; whatever the reference
                    // says next is the answer.
                    self.via = None;
                    self.forwarded = None;
                }
            }
        }
        let (reply, chain) =
            self.pool.attempt(&self.ior, operation, &write_args, timeout).map_err(|f| f.error)?;
        self.note(chain);
        Ok(reply)
    }

    fn invoke_oneway<F: Fn(&mut Encoder)>(&mut self, operation: &str, write_args: F) -> Result<()> {
        // To the reference itself, never the cache: a oneway cannot be
        // forwarded, so a oneway sent to a temporary target would be a
        // request the servant never redirected there. "The reference itself"
        // is the shared address, which is why this refreshes too — a oneway
        // is the one call that gets no chance to be redirected.
        self.refresh();
        self.pool.invoke_oneway(&self.ior, operation, write_args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IiopProfile, TaggedComponent};

    /// A `TAG_CODE_SETS` body advertising UTF-8 for `char`, which is what a
    /// client here negotiates to — so a profile carrying it and one without it
    /// really do end up on different connections.
    fn code_sets_component() -> Vec<u8> {
        let mut e = Encoder::encapsulation(Endian::Little);
        e.put_u32(codeset::CodeSetId::UTF_8.0); // native char
        e.put_u32(1);
        e.put_u32(codeset::CodeSetId::UTF_8.0); // conversion
        e.put_u32(codeset::CodeSetId::UTF_16.0); // native wchar
        e.put_u32(0);
        e.finish().expect("encapsulation encodes")
    }

    fn profile(host: &str, port: u16, minor: u8) -> IiopProfile {
        IiopProfile {
            version: Version { major: 1, minor },
            host: host.into(),
            port,
            object_key: b"key".to_vec(),
            components: Vec::new(),
        }
    }

    /// The key is the endpoint *plus what was negotiated once*. Two profiles
    /// that differ only in version, or only in published codeset, must not
    /// share a connection.
    #[test]
    fn the_key_separates_versions_and_codesets() {
        let a = profile("h", 1, 2);
        let b = profile("h", 1, 0);
        assert_ne!(Key::of(&a, "h", 1), Key::of(&b, "h", 1), "version is part of the key");

        let mut with_codeset = profile("h", 1, 2);
        with_codeset
            .components
            .push(TaggedComponent { tag: codeset::TAG_CODE_SETS, data: code_sets_component() });
        assert_ne!(
            Key::of(&a, "h", 1),
            Key::of(&with_codeset, "h", 1),
            "a negotiated codeset is per connection, so it is part of the key"
        );
        assert_eq!(Key::of(&a, "h", 1), Key::of(&profile("h", 1, 2), "h", 1));
    }

    /// The object key is deliberately *not* in it: one connection serves every
    /// object behind an endpoint, which is the whole reason pooling pays.
    #[test]
    fn the_object_key_is_not_part_of_the_key() {
        let mut a = profile("h", 1, 2);
        let mut b = profile("h", 1, 2);
        a.object_key = b"one".to_vec();
        b.object_key = b"another".to_vec();
        assert_eq!(Key::of(&a, "h", 1), Key::of(&b, "h", 1));
    }

    /// An empty IOR has no endpoint to key on, and must say so rather than
    /// dialing something.
    #[test]
    fn an_ior_without_a_profile_is_refused() {
        let pool = crate::orb::Orb::new().pool();
        let nil = Ior { type_id: String::new(), profiles: Vec::new() };
        assert!(matches!(pool.acquire(&nil), Err(Error::NoIiopProfile)));
        assert_eq!(pool.size(), 0);
    }
}
