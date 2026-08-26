//! The numbers a deployment owns, and the argument syntax the specification
//! gives for setting them.
//!
//! D019 §3 measured the gap: seven limits that a network operator changes
//! first — a message ceiling, a fragment threshold and count, a forward-hop
//! limit, a follow timeout, a connection cap and a shutdown poll — and **not
//! one of them had a home a deployment could reach.** Three had a setter
//! somewhere in the API; none had a way in from outside the process. So D015's
//! acceptance sentence, *"without editing Rust, without a rebuild"*, was still
//! false one layer below where that batch had made it true.
//!
//! *배포만이 알 수 있는 수치의 집은 하나이며, 그것은 소스 파일이 아니다.*
//!
//! # The rule, not the seven
//!
//! **A number only a deployment can know has one home, and it is not a source
//! file.** The seven are where the rule was noticed, not its extent. Swept
//! 2026-08-25 across every file in this crate: **twenty-six numbers are in the
//! deployment class**, and the seven are a quarter of them. Every one has a
//! verdict.
//!
//! ## Configured here (8)
//!
//! `DEFAULT_MAX_MESSAGE_SIZE` · `DEFAULT_FRAGMENT_THRESHOLD` · `MAX_FRAGMENTS`
//! · `MAX_FORWARD_HOPS` · `FOLLOW_TIMEOUT` · `DEFAULT_MAX_CONNECTIONS` ·
//! `STOP_POLL` · `DEFAULT_MESSAGE_TIMEOUT`.
//!
//! The eighth is D019 §3's list plus one the sweep found beside it
//! (`server.rs:921`, 30 s, how long a peer that has started a message may
//! stall). It is the same class, on the same struct, three lines down; leaving
//! it out would have been scoping to the list instead of to the rule.
//!
//! ## Already configurable, and still Rust-only (5)
//!
//! `pool::Limits` — `DEFAULT_MAX_TOTAL`, `DEFAULT_MAX_PER_ENDPOINT`,
//! `DEFAULT_SOFT_IN_FLIGHT`, `DEFAULT_MAX_IDLE`, `DEFAULT_CONNECT_TIMEOUT` —
//! is the one cluster that already had a configuration object. It reaches a
//! deployment only through Rust, which is the same gap; this step said a
//! `-ORB…` flag for a pool limit had nothing to apply itself to until the ORB
//! handed out the transport.
//!
//! **D019 step 4 landed and that precondition is now met** — a pool comes from
//! [`Orb::pool`](crate::orb::Orb::pool) and carries an `OrbConfig` — but the
//! five keys were *not* added with it, deliberately: step 4's own rule is that
//! it changes no behaviour by default, and five new keys are five new
//! behaviours to negative-control. They are named here so the next reader sees
//! a decision rather than an omission.
//!
//! ## Configurable in Rust, no way in from outside (5)
//!
//! `event_server`'s `DEFAULT_QUEUE_LIMIT`, `DEFAULT_PULL_BLOCK` and
//! `DEFAULT_PUSH_TIMEOUT` have setters; `MAX_CONSECUTIVE_FAILURES` and
//! `PULL_POLL` have none. All five are the event channel's, and
//! `event_server.rs` is held by a branch in flight — reported, not touched.
//! Same verdict as the pool: they belong to a servant the ORB does not yet
//! construct.
//!
//! ## In the code, with the reason (8)
//!
//! - `BODY_CHUNK` (`lib.rs:1390`, 64 KiB) — the read buffer, which bounds what
//!   a peer that sends a header and then goes silent can commit. A deployment
//!   changing it changes a memory-safety posture, not a policy.
//! - `MAX_DEPTH` (`typecode.rs:36`, 64) — TypeCode nesting. Same: it is a
//!   defence against a hostile stream, and the number a hostile stream would
//!   like is "higher".
//! - `MAX_ABANDONED` (`mux.rs:257`, 4096) and `LEADER_POLL` (`mux.rs:244`,
//!   25 ms) — multiplexer internals, and `mux.rs` is held by a branch in
//!   flight.
//! - `DEFAULT_CALL_TIMEOUT` (`mux.rs:232`, 30 s) — already overridable per
//!   call through `Pool::invoke_with`, which is the right granularity for a
//!   timeout a *caller* owns rather than an operator.
//! - The three unnamed poll literals: `event_server.rs:1187` (50 ms, the
//!   delivery loop's, **not named at all** and not linked to `PULL_POLL`
//!   fourteen lines away, which is the same value), `event_server.rs:1796`
//!   (2 ms, a test-support waiter), and the 1 ms floor at `lib.rs:1588`, which
//!   is not a policy but a guard against the kernel reading a zero read-timeout
//!   as "wait forever".
//!
//! ## What the sweep corrected
//!
//! **D019 §8 names `csiv2.rs:44`'s `15` as a number to sweep. It is the
//! specification's** — `IOP::ServiceId` for `SecurityAttributeService` — and a
//! key for it would be a way to speak CSIv2 into a service context nobody
//! reads. Verdict: **must not move**, and it is listed here so the next reader
//! of §8 does not spend the same half hour.
//!
//! And a gap the sweep found that is not about configuration at all:
//! `DEFAULT_MAX_MESSAGE_SIZE` had a setter on `Connection` and **none on
//! `Server`**, which hard-coded it into `bind`. The serving side's most
//! important resource cap was the one with no way to change it in-process
//! either. **Closed by D019 step 4**, and closed without adding the public
//! setter that was reported here: `Server::bind` became `pub(crate)`, so the
//! field is reached by `Server::apply_orb_config` from the one constructor
//! there now is. A number that only a deployment can know did not need a
//! second Rust door; it needed the door it already had to be connected.
//!
//! # What must not move, and why that is not a detail
//!
//! `MAGIC`, `HEADER_LEN`, every `TAG_*` component and profile id, the service
//! context ids, the TypeCode kind numbers, the CDR alignment rules, the
//! repository ids and `corbaloc:`'s default port 2809 are **the
//! specification's**. They are not this file's business and there is no key
//! for any of them, because *a configuration key for one of those is a way to
//! write a non-conformant ORB from a file* — a deployment could turn this into
//! something that no longer speaks GIOP, from the outside, with no rebuild and
//! no diff. The line this module draws is exactly: a limit is a deployment's,
//! a wire constant is the OMG's.
//!
//! # The syntax is the specification's too
//!
//! CORBA 3.4 §8.5.1 defines how an ORB is configured at initialization: the
//! argument list is scanned for `-ORB<suffix>` parameters, each optionally
//! followed by *"any associated sequential parameter strings"*. Two sentences
//! of that sub clause are implemented here directly:
//!
//! > *"Before ORB_init returns, it will remove from the arg_list parameter all
//! > strings that match the -ORB<suffix> pattern described above and that are
//! > recognized by that ORB implementation, along with any associated
//! > sequential parameter strings."*
//!
//! — so [`OrbConfig::from_orb_args`] returns the surviving arguments, and a
//! program can hand it everything it was given and keep what is its own.
//!
//! > *"If any strings in arg_list that match this pattern are not recognized by
//! > the ORB implementation, ORB_init will raise the BAD_PARAM system exception
//! > instead."*
//!
//! — so an unrecognised `-ORB…` is a **refusal, not a shrug**. That sentence is
//! the *refused whole or applied whole* property the MCP `--config` batch
//! proved, handed to us by the standard: a typo in an operator's argument stops
//! the ORB rather than silently leaving a limit at its default.
//!
//! [`OrbConfig::from_orb_args`] deliberately leaves anything that is not
//! `-ORB…` alone, including `-ORBid` and `-ORBServerId`, which §8.5.1 and
//! §8.5.1.1 define and this ORB does not implement — see
//! [`UNIMPLEMENTED_ORB_ARGUMENTS`], which lists them so that they are refused
//! *by name and with a reason* rather than as unknown noise.
//!
//! # `-ORBInitRef` is not ours to spell
//!
//! §8.5.3.2 fixes the form exactly — `-ORBInitRef <ObjectID>=<ObjectURL>` —
//! with the examples `NameService=IOR:00230021AB...`,
//! `NotificationService=corbaloc::555objs.com/NotificationService` and
//! `TradingService=corbaname::555objs.com#Dev/Trader`. It also says
//! `<ObjectURL>` may be *"any of the URL schemes supported by
//! `CORBA::ORB::string_to_object`"*, which is why D019's step 2 came before
//! this one: the configuration is specified in terms of that operation, and we
//! now have it.
//!
//! And it carries one exclusion that is easy to miss and is implemented here:
//! *"with the exception of the corbaloc URL scheme with the rir protocol (i.e.,
//! corbaloc:rir...)"*. An `-ORBInitRef` whose value is a `rir` URL would tell
//! the table to resolve a name out of itself, so it is refused, naming both the
//! `ObjectID` and the URL.
//!
//! The seven numeric flags have **no OMG names** — §8.5.1's *"All other
//! -ORB<suffix> parameters in the arg_list may be of significance"* is the
//! vendor space, and these spellings are ours. Each duration flag ends in `Ms`
//! and takes milliseconds, so a value cannot be read in the wrong unit by a
//! reader who did not check the documentation.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::server::{DEFAULT_MAX_CONNECTIONS, DEFAULT_MESSAGE_TIMEOUT, STOP_POLL};
use crate::{
    DEFAULT_FRAGMENT_THRESHOLD, DEFAULT_MAX_MESSAGE_SIZE, FOLLOW_TIMEOUT, MAX_FORWARD_HOPS,
    MAX_FRAGMENTS,
};

/// `-ORB<suffix>` arguments CORBA 3.4 defines that this ORB does not
/// implement, each with the reason it is refused rather than ignored.
///
/// §8.5.1 requires that an unrecognised `-ORB…` raise `BAD_PARAM`, and these
/// would qualify — but "unrecognised" is a poor thing to tell an operator who
/// has typed a *standard* argument. They are refused by name, with what they
/// would take, so the message distinguishes *"you made that up"* from *"that is
/// real and this ORB has not got there yet"*.
pub const UNIMPLEMENTED_ORB_ARGUMENTS: &[(&str, &str)] = &[
    // §8.5.1: names the ORB instance. There is one ORB per `Orb` value here,
    // and nothing keyed by id, so accepting this would be accepting an
    // argument that changes nothing.
    ("-ORBid", "this ORB has no id-keyed instance registry (CORBA 3.4 §8.5.1)"),
    (
        "-ORBServerId",
        "server ids belong to an Implementation Repository, which this ORB has \
      not got (CORBA 3.4 §8.5.1.1)",
    ),
    // §8.5.1.2. This is transport, and handing out the transport is D019
    // step 4 — the step that is gated on the §5 shape being approved.
    // §8.5.1.2. The ORB owns the transport since D019 step 4 — `Orb::server`
    // is now the only way to a listener — so the old reason for this refusal
    // ("construct a Server directly") stopped being true and stopped being
    // possible in the same commit. What remains is a real limit and a
    // different one: the argument takes a *list* of endpoints, and a `Server`
    // holds one `TcpListener`. A key that accepted a list and bound its first
    // element would be the worst of the three options.
    (
        "-ORBListenEndpoints",
        "a Server listens on one endpoint and this argument takes a list; \
      pass the address to Orb::server (CORBA 3.4 §8.5.1.2)",
    ),
    // §8.5.3.3. Needs the four-step resolution order of §8.5.3.4, which needs
    // more than one source of initial references; there is one today.
    (
        "-ORBDefaultInitRef",
        "resolution falls back to no second source, so a default would \
      never be reached (CORBA 3.4 §8.5.3.3)",
    ),
];

/// The `-ORBInitRef` argument, spelled as CORBA 3.4 §8.5.3.2 spells it.
pub const INIT_REF_ARG: &str = "-ORBInitRef";

/// Every `-ORB<suffix>` this ORB implements, with the value each one takes.
///
/// Used to drive [`OrbConfig::from_orb_args`] and to write the refusal for an
/// unknown one, so the list of what is accepted and the list a diagnostic
/// quotes cannot drift apart — there is only one list.
const NUMERIC_ARGS: &[(&str, &str)] = &[
    ("-ORBmaxMessageSize", "a size in bytes"),
    ("-ORBfragmentThreshold", "a size in bytes"),
    ("-ORBmaxFragments", "a count"),
    ("-ORBmaxForwardHops", "a count, at most 255"),
    ("-ORBfollowTimeoutMs", "a duration in milliseconds"),
    ("-ORBmaxConnections", "a count"),
    ("-ORBstopPollMs", "a duration in milliseconds"),
    ("-ORBmessageTimeoutMs", "a duration in milliseconds"),
];

/// The deployment's answers to the seven, and the initial references it wants
/// registered.
///
/// # Absent is not zero
///
/// Every setting is an [`Option`], so *"no configuration changes nothing"* is a
/// property of the type rather than a claim a test has to chase. A field left
/// `None` resolves, at the accessor, to exactly the constant this crate used
/// before any of this existed — [`OrbConfig::max_message_size`] answers
/// [`DEFAULT_MAX_MESSAGE_SIZE`], and so on down. There is no code path in which
/// an unset field becomes `0`, because there is no code path in which an unset
/// field is read as a number at all.
///
/// That property is doubled at the boundary: [`OrbConfig::from_orb_args`]
/// **refuses a zero** for every cap and every duration. A `0` message ceiling
/// refuses every message, a `0` poll interval is a busy loop, and a `0` in a
/// configuration file is almost always an absence that has been through a layer
/// which did not have this type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrbConfig {
    max_message_size: Option<usize>,
    fragment_threshold: Option<usize>,
    max_fragments: Option<usize>,
    max_forward_hops: Option<u8>,
    follow_timeout: Option<Duration>,
    max_connections: Option<usize>,
    stop_poll: Option<Duration>,
    message_timeout: Option<Duration>,
    initial_references: Vec<(String, String)>,
}

impl OrbConfig {
    /// A configuration that changes nothing. Every accessor answers today's
    /// constant.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads every `-ORB<suffix>` argument CORBA 3.4 §8.5.1 describes, and
    /// answers the configuration plus **the arguments that were not ours** —
    /// §8.5.1's *"it will remove from the arg_list … all strings that match the
    /// -ORB<suffix> pattern … and that are recognized by that ORB
    /// implementation, along with any associated sequential parameter
    /// strings."*
    ///
    /// # Refused whole or applied whole
    ///
    /// The first problem stops the read and nothing is applied — there is no
    /// partially-configured [`OrbConfig`] to hand back, because the value is
    /// built and returned in one move. §8.5.1 mandates the strict half of this
    /// for an unrecognised `-ORB…`; the rest follows it for consistency, since
    /// an operator who mistyped one number has no reason to believe the other
    /// six landed.
    ///
    /// # Errors
    ///
    /// [`ConfigError`], which names the argument, the value it was given and
    /// what that argument expects — all three, because a diagnostic missing any
    /// one of them sends the reader back to the source.
    pub fn from_orb_args<S: AsRef<str>>(
        args: &[S],
    ) -> std::result::Result<(Self, Vec<String>), ConfigError> {
        let mut config = Self::new();
        let mut rest = Vec::new();
        let mut seen_init_refs: BTreeMap<String, String> = BTreeMap::new();
        let mut i = 0;

        while i < args.len() {
            let arg = args[i].as_ref();
            if !arg.starts_with("-ORB") {
                rest.push(arg.to_owned());
                i += 1;
                continue;
            }

            // A standard argument this ORB has not implemented is refused with
            // its reason, never as unknown noise.
            if let Some((_, why)) = UNIMPLEMENTED_ORB_ARGUMENTS.iter().find(|(n, _)| *n == arg) {
                return Err(ConfigError::Unimplemented {
                    arg: arg.to_owned(),
                    reason: (*why).to_owned(),
                });
            }

            let value = || -> std::result::Result<&str, ConfigError> {
                args.get(i + 1).map(|v| v.as_ref()).ok_or_else(|| ConfigError::MissingValue {
                    arg: arg.to_owned(),
                    expected: expected_for(arg).to_owned(),
                })
            };

            if arg == INIT_REF_ARG {
                let raw = value()?;
                let (object_id, url) = parse_init_ref(arg, raw)?;
                if let Some(previous) = seen_init_refs.insert(object_id.clone(), url.clone()) {
                    return Err(ConfigError::DuplicateInitRef { object_id, previous, url });
                }
                config.initial_references.push((object_id, url));
                i += 2;
                continue;
            }

            if NUMERIC_ARGS.iter().any(|(n, _)| *n == arg) {
                let raw = value()?;
                config.apply_numeric(arg, raw)?;
                i += 2;
                continue;
            }

            // §8.5.1: *"ORB_init will raise the BAD_PARAM system exception."*
            return Err(ConfigError::Unknown {
                arg: arg.to_owned(),
                known: known_argument_names(),
            });
        }

        Ok((config, rest))
    }

    fn apply_numeric(&mut self, arg: &str, raw: &str) -> std::result::Result<(), ConfigError> {
        let expected = expected_for(arg).to_owned();
        let bad = |value: &str| ConfigError::BadValue {
            arg: arg.to_owned(),
            value: value.to_owned(),
            expected: expected.clone(),
        };
        let count = |value: &str| -> std::result::Result<usize, ConfigError> {
            let n: usize = value.parse().map_err(|_| bad(value))?;
            // See "Absent is not zero": a zero cap is an absence that has been
            // through a layer without this type, and applying it would refuse
            // every message, allow no connection, or spin.
            if n == 0 { Err(bad(value)) } else { Ok(n) }
        };

        match arg {
            "-ORBmaxMessageSize" => self.max_message_size = Some(count(raw)?),
            "-ORBfragmentThreshold" => self.fragment_threshold = Some(count(raw)?),
            "-ORBmaxFragments" => self.max_fragments = Some(count(raw)?),
            "-ORBmaxForwardHops" => {
                let n = count(raw)?;
                self.max_forward_hops = Some(u8::try_from(n).map_err(|_| bad(raw))?);
            }
            "-ORBfollowTimeoutMs" => {
                self.follow_timeout = Some(Duration::from_millis(count(raw)? as u64))
            }
            "-ORBmaxConnections" => self.max_connections = Some(count(raw)?),
            "-ORBstopPollMs" => self.stop_poll = Some(Duration::from_millis(count(raw)? as u64)),
            "-ORBmessageTimeoutMs" => {
                self.message_timeout = Some(Duration::from_millis(count(raw)? as u64))
            }
            // `NUMERIC_ARGS` is the only thing that routes here, and this arm
            // exists so that adding a name to it without adding a case fails
            // loudly at the first use rather than silently doing nothing.
            other => {
                return Err(ConfigError::Unknown {
                    arg: other.to_owned(),
                    known: known_argument_names(),
                });
            }
        }
        Ok(())
    }

    /// Ceiling on an inbound message body. Defaults to
    /// [`DEFAULT_MAX_MESSAGE_SIZE`].
    pub fn max_message_size(&self) -> usize {
        self.max_message_size.unwrap_or(DEFAULT_MAX_MESSAGE_SIZE)
    }

    /// Body size above which an outbound message is fragmented. Defaults to
    /// [`DEFAULT_FRAGMENT_THRESHOLD`].
    pub fn fragment_threshold(&self) -> usize {
        self.fragment_threshold.unwrap_or(DEFAULT_FRAGMENT_THRESHOLD)
    }

    /// Most fragments accepted for one logical message. Defaults to
    /// [`MAX_FRAGMENTS`].
    pub fn max_fragments(&self) -> usize {
        self.max_fragments.unwrap_or(MAX_FRAGMENTS)
    }

    /// How many `LOCATION_FORWARD` hops to follow. Defaults to
    /// [`MAX_FORWARD_HOPS`].
    pub fn max_forward_hops(&self) -> u8 {
        self.max_forward_hops.unwrap_or(MAX_FORWARD_HOPS)
    }

    /// How long a dial the ORB makes for itself may take. Defaults to
    /// [`FOLLOW_TIMEOUT`].
    pub fn follow_timeout(&self) -> Duration {
        self.follow_timeout.unwrap_or(FOLLOW_TIMEOUT)
    }

    /// Concurrent connections a server accepts. Defaults to
    /// [`DEFAULT_MAX_CONNECTIONS`].
    pub fn max_connections(&self) -> usize {
        self.max_connections.unwrap_or(DEFAULT_MAX_CONNECTIONS)
    }

    /// Granularity of shutdown, not of service. Defaults to [`STOP_POLL`].
    pub fn stop_poll(&self) -> Duration {
        self.stop_poll.unwrap_or(STOP_POLL)
    }

    /// How long a peer that has started a message may stall before its
    /// connection is dropped. Defaults to [`DEFAULT_MESSAGE_TIMEOUT`].
    ///
    /// Not in D019 §3's list of seven: the sweep found it three lines below
    /// [`STOP_POLL`], on the same struct, in the same class. Scoping to the
    /// list rather than to the rule would have left it out.
    pub fn message_timeout(&self) -> Duration {
        self.message_timeout.unwrap_or(DEFAULT_MESSAGE_TIMEOUT)
    }

    /// The `-ORBInitRef` entries, in the order they were given.
    pub fn initial_references(&self) -> &[(String, String)] {
        &self.initial_references
    }

    /// Whether anything at all was configured. `false` for
    /// [`OrbConfig::new`], and the honest thing for a caller to log.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn expected_for(arg: &str) -> &'static str {
    if arg == INIT_REF_ARG {
        return "<ObjectID>=<ObjectURL> (CORBA 3.4 §8.5.3.2)";
    }
    NUMERIC_ARGS.iter().find(|(n, _)| *n == arg).map(|(_, e)| *e).unwrap_or("a value")
}

fn known_argument_names() -> Vec<String> {
    std::iter::once(INIT_REF_ARG.to_owned())
        .chain(NUMERIC_ARGS.iter().map(|(n, _)| (*n).to_owned()))
        .collect()
}

/// Splits `<ObjectID>=<ObjectURL>` and applies §8.5.3.2's one exclusion.
fn parse_init_ref(arg: &str, raw: &str) -> std::result::Result<(String, String), ConfigError> {
    let (object_id, url) = raw.split_once('=').ok_or_else(|| ConfigError::BadValue {
        arg: arg.to_owned(),
        value: raw.to_owned(),
        expected: expected_for(arg).to_owned(),
    })?;
    if object_id.is_empty() {
        // The same condition §16.10.1 puts on `register_initial_reference`,
        // caught here so the message can name the argument it came from.
        return Err(ConfigError::BadValue {
            arg: arg.to_owned(),
            value: raw.to_owned(),
            expected: "a non-empty <ObjectID> before the '='".to_owned(),
        });
    }
    // §8.5.3.2: every scheme `string_to_object` supports, *"with the exception
    // of the corbaloc URL scheme with the rir protocol"*. An initial reference
    // that resolves through the initial references table is circular.
    let head = url.get(..12).unwrap_or(url);
    if head.len() >= 12 && head.eq_ignore_ascii_case("corbaloc:rir") {
        return Err(ConfigError::RirInInitRef {
            object_id: object_id.to_owned(),
            url: url.to_owned(),
        });
    }
    Ok((object_id.to_owned(), url.to_owned()))
}

/// Why a configuration was refused. Every variant names the argument; the ones
/// about a value name the value **and** what was expected of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// An `-ORB<suffix>` this ORB does not recognise. CORBA 3.4 §8.5.1:
    /// *"ORB_init will raise the BAD_PARAM system exception."*
    Unknown {
        /// The argument as written.
        arg: String,
        /// What this ORB does accept, so the reader can spot a typo.
        known: Vec<String>,
    },
    /// A standard `-ORB<suffix>` this ORB has not implemented. Told apart from
    /// [`ConfigError::Unknown`] because the fixes differ: one is a typo, the
    /// other is a capability.
    Unimplemented {
        /// The argument as written.
        arg: String,
        /// Why it is not accepted, from [`UNIMPLEMENTED_ORB_ARGUMENTS`].
        reason: String,
    },
    /// The argument takes a following value and the list ended.
    MissingValue {
        /// The argument as written.
        arg: String,
        /// What it takes.
        expected: String,
    },
    /// The value could not be read, or was a zero where zero is never meant.
    BadValue {
        /// The argument as written.
        arg: String,
        /// The value as written.
        value: String,
        /// What it takes.
        expected: String,
    },
    /// `-ORBInitRef <ObjectID>=corbaloc:rir:…`, which §8.5.3.2 excludes.
    RirInInitRef {
        /// The `ObjectID` that was to be registered.
        object_id: String,
        /// The offending URL.
        url: String,
    },
    /// An `-ORBInitRef` value that parsed as an argument and could not be
    /// turned into a reference. Raised by [`crate::orb::Orb::with_config`],
    /// which is where the URL is actually read — §8.5.3.2 fixes the argument's
    /// *shape*, and only `string_to_object` can judge its *value*.
    InitRefUnreadable {
        /// The `ObjectID` that was to be registered.
        object_id: String,
        /// The URL as written.
        url: String,
        /// What `string_to_object` said, which names the string too.
        reason: String,
    },
    /// The same `ObjectID` was given twice. §16.10.1 already refuses a second
    /// registration; catching it here names the argument instead of the table.
    DuplicateInitRef {
        /// The `ObjectID` given twice.
        object_id: String,
        /// The URL from the first occurrence.
        previous: String,
        /// The URL from the second.
        url: String,
    },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Unknown { arg, known } => write!(
                f,
                "{arg} is not an ORB argument this ORB recognises, so the whole configuration \
                 is refused (CORBA 3.4 §8.5.1); it accepts {}",
                known.join(", ")
            ),
            ConfigError::Unimplemented { arg, reason } => {
                write!(f, "{arg} is a standard ORB argument this ORB does not implement: {reason}")
            }
            ConfigError::MissingValue { arg, expected } => {
                write!(f, "{arg} takes {expected}, and the argument list ended")
            }
            ConfigError::BadValue { arg, value, expected } => {
                write!(f, "{arg} takes {expected}, and was given {value:?}")
            }
            ConfigError::RirInInitRef { object_id, url } => write!(
                f,
                "-ORBInitRef {object_id}={url} names a corbaloc:rir URL, which CORBA 3.4 \
                 §8.5.3.2 excludes: an initial reference cannot be resolved out of the initial \
                 references table it is being registered into"
            ),
            ConfigError::InitRefUnreadable { object_id, url, reason } => write!(
                f,
                "-ORBInitRef {object_id}={url} was not readable as an object reference, so the \
                 whole configuration is refused and nothing was registered: {reason}"
            ),
            ConfigError::DuplicateInitRef { object_id, previous, url } => write!(
                f,
                "-ORBInitRef gives {object_id} twice, as {previous:?} and as {url:?}; \
                 CORBA 3.4 §16.10.1 refuses a second registration, so say which one is meant"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(|x| x.to_owned()).collect()
    }

    /// The property that makes every other one cheap: **absent is not zero.**
    /// A configuration nobody touched answers exactly the constants this crate
    /// used before this module existed.
    #[test]
    fn an_empty_configuration_answers_todays_constants() {
        let c = OrbConfig::new();
        assert_eq!(c.max_message_size(), DEFAULT_MAX_MESSAGE_SIZE);
        assert_eq!(c.fragment_threshold(), DEFAULT_FRAGMENT_THRESHOLD);
        assert_eq!(c.max_fragments(), MAX_FRAGMENTS);
        assert_eq!(c.max_forward_hops(), MAX_FORWARD_HOPS);
        assert_eq!(c.follow_timeout(), FOLLOW_TIMEOUT);
        assert_eq!(c.max_connections(), DEFAULT_MAX_CONNECTIONS);
        assert_eq!(c.stop_poll(), STOP_POLL);
        assert_eq!(c.message_timeout(), DEFAULT_MESSAGE_TIMEOUT);
        assert!(c.initial_references().is_empty());
        assert!(c.is_empty());

        // …and so does one parsed from an argument list with nothing of ours
        // in it, which is the case a real program actually hits.
        let (parsed, rest) = OrbConfig::from_orb_args(&args("--verbose -x 3")).unwrap();
        assert_eq!(parsed, c);
        assert_eq!(rest, args("--verbose -x 3"), "§8.5.1 removes only what it recognised");
    }

    /// All seven, set at once, each landing on its own accessor. A value that
    /// went to the wrong field would pass a one-at-a-time test.
    #[test]
    fn every_deployment_number_in_this_crates_class_has_a_home() {
        let (c, rest) = OrbConfig::from_orb_args(&args(
            "-ORBmaxMessageSize 1000 -ORBfragmentThreshold 2000 -ORBmaxFragments 3000 \
             -ORBmaxForwardHops 4 -ORBfollowTimeoutMs 5000 -ORBmaxConnections 6 \
             -ORBstopPollMs 7 -ORBmessageTimeoutMs 8000",
        ))
        .unwrap();
        assert_eq!(c.max_message_size(), 1000);
        assert_eq!(c.fragment_threshold(), 2000);
        assert_eq!(c.max_fragments(), 3000);
        assert_eq!(c.max_forward_hops(), 4);
        assert_eq!(c.follow_timeout(), Duration::from_secs(5));
        assert_eq!(c.max_connections(), 6);
        assert_eq!(c.stop_poll(), Duration::from_millis(7));
        assert_eq!(c.message_timeout(), Duration::from_secs(8));
        assert!(rest.is_empty(), "every one of them was consumed");
        assert!(!c.is_empty());
    }

    /// §8.5.1's own sentence: an ORB argument the implementation recognises is
    /// removed *"along with any associated sequential parameter strings"*, and
    /// everything else survives in order.
    #[test]
    fn recognised_arguments_are_removed_and_the_rest_survive_in_order() {
        let (c, rest) = OrbConfig::from_orb_args(&args(
            "serve --port 9000 -ORBmaxConnections 12 --verbose \
             -ORBInitRef NameService=corbaloc::h.test:2809/NameService last",
        ))
        .unwrap();
        assert_eq!(rest, args("serve --port 9000 --verbose last"));
        assert_eq!(c.max_connections(), 12);
        assert_eq!(
            c.initial_references(),
            [("NameService".to_owned(), "corbaloc::h.test:2809/NameService".to_owned())]
        );
    }

    /// §8.5.3.2's spelling and its three examples, verbatim from the sub
    /// clause. All three schemes it shows must read.
    #[test]
    fn init_ref_takes_the_forms_the_specification_prints() {
        let (c, _) = OrbConfig::from_orb_args(&args(
            "-ORBInitRef NameService=IOR:00230021AB \
             -ORBInitRef NotificationService=corbaloc::555objs.com/NotificationService \
             -ORBInitRef TradingService=corbaname::555objs.com#Dev/Trader",
        ))
        .unwrap();
        assert_eq!(
            c.initial_references(),
            [
                ("NameService".to_owned(), "IOR:00230021AB".to_owned()),
                (
                    "NotificationService".to_owned(),
                    "corbaloc::555objs.com/NotificationService".to_owned()
                ),
                ("TradingService".to_owned(), "corbaname::555objs.com#Dev/Trader".to_owned()),
            ],
            "the reading is deferred: §8.5.3.2 fixes the argument's shape, not its value's"
        );
    }

    /// The exclusion §8.5.3.2 carries and that is easy to miss.
    #[test]
    fn init_ref_refuses_a_rir_url_as_the_specification_requires() {
        for url in ["corbaloc:rir:NameService", "corbaloc:rir:/NameService", "CORBALOC:RIR:"] {
            let err = OrbConfig::from_orb_args(&args(&format!("-ORBInitRef NameService={url}")))
                .unwrap_err();
            assert!(
                matches!(err, ConfigError::RirInInitRef { .. }),
                "{url} should be refused: {err}"
            );
            let said = err.to_string();
            assert!(said.contains("NameService"), "{said}");
            assert!(said.contains(url), "{said}");
            assert!(said.contains("§8.5.3.2"), "{said}");
        }
        // A `corbaloc:` that is not `rir` is untouched by the exclusion.
        assert!(
            OrbConfig::from_orb_args(&args("-ORBInitRef X=corbaloc::h/K")).is_ok(),
            "only the rir protocol is excluded"
        );
    }

    /// Every refusal names the argument, the value and the expectation — all
    /// three, because a message missing one sends the reader to the source.
    #[test]
    fn every_refusal_names_the_argument_the_value_and_the_expectation() {
        let unknown = OrbConfig::from_orb_args(&args("-ORBmaxMessages 5")).unwrap_err();
        assert!(matches!(unknown, ConfigError::Unknown { .. }));
        let said = unknown.to_string();
        assert!(said.contains("-ORBmaxMessages"), "{said}");
        assert!(said.contains("§8.5.1"), "{said}");
        assert!(said.contains("-ORBmaxMessageSize"), "the near miss is listed: {said}");

        let missing = OrbConfig::from_orb_args(&args("-ORBmaxConnections")).unwrap_err();
        assert!(matches!(missing, ConfigError::MissingValue { .. }));
        let said = missing.to_string();
        assert!(said.contains("-ORBmaxConnections") && said.contains("a count"), "{said}");

        let bad = OrbConfig::from_orb_args(&args("-ORBstopPollMs soon")).unwrap_err();
        let said = bad.to_string();
        assert!(said.contains("-ORBstopPollMs"), "{said}");
        assert!(said.contains("\"soon\""), "the value: {said}");
        assert!(said.contains("milliseconds"), "the expectation, with its unit: {said}");

        let dup = OrbConfig::from_orb_args(&args(
            "-ORBInitRef N=corbaloc::a/K -ORBInitRef N=corbaloc::b/K",
        ))
        .unwrap_err();
        assert!(matches!(dup, ConfigError::DuplicateInitRef { .. }));
        let said = dup.to_string();
        assert!(
            said.contains('N') && said.contains("corbaloc::a/K") && said.contains("corbaloc::b/K"),
            "{said}"
        );
        assert!(said.contains("§16.10.1"), "{said}");

        let shapeless = OrbConfig::from_orb_args(&args("-ORBInitRef NameService")).unwrap_err();
        assert!(shapeless.to_string().contains("§8.5.3.2"), "{shapeless}");
        let empty_id = OrbConfig::from_orb_args(&args("-ORBInitRef =corbaloc::h/K")).unwrap_err();
        assert!(empty_id.to_string().contains("non-empty"), "{empty_id}");
    }

    /// A standard argument we have not implemented is refused **with its
    /// reason**, not as unknown noise: the two need different fixes.
    #[test]
    fn a_standard_argument_we_lack_is_told_apart_from_a_typo() {
        for (arg, _) in UNIMPLEMENTED_ORB_ARGUMENTS {
            let err = OrbConfig::from_orb_args(&args(&format!("{arg} whatever"))).unwrap_err();
            assert!(matches!(err, ConfigError::Unimplemented { .. }), "{arg}: {err}");
            let said = err.to_string();
            assert!(said.contains(arg), "{said}");
            assert!(said.contains("CORBA 3.4 §"), "the reason cites its sub clause: {said}");
        }
        // …and an invention is still just unknown.
        assert!(matches!(
            OrbConfig::from_orb_args(&args("-ORBmagicalThinking 1")),
            Err(ConfigError::Unknown { .. })
        ));
    }

    /// Zero is the shape an absence takes after passing through a layer that
    /// does not have [`Option`]. Applying it would refuse every message, allow
    /// no connection, or spin the shutdown poll.
    #[test]
    fn a_zero_is_refused_wherever_zero_is_never_meant() {
        for arg in NUMERIC_ARGS.iter().map(|(n, _)| *n) {
            let err = OrbConfig::from_orb_args(&args(&format!("{arg} 0"))).unwrap_err();
            assert!(matches!(err, ConfigError::BadValue { .. }), "{arg} 0 was accepted");
            assert!(err.to_string().contains(arg), "{err}");
        }
        // A hop count above what the field can hold is the same class.
        assert!(matches!(
            OrbConfig::from_orb_args(&args("-ORBmaxForwardHops 256")),
            Err(ConfigError::BadValue { .. })
        ));
        assert!(OrbConfig::from_orb_args(&args("-ORBmaxForwardHops 255")).is_ok());
    }

    /// Refused whole, never in part: the failure is on the *seventh* argument
    /// and the six good ones before it are not returned to anybody.
    #[test]
    fn a_configuration_is_refused_whole_and_never_applied_in_part() {
        let bad = args(
            "-ORBmaxMessageSize 1000 -ORBfragmentThreshold 2000 -ORBmaxFragments 3000 \
             -ORBmaxForwardHops 4 -ORBfollowTimeoutMs 5000 -ORBmaxConnections 6 \
             -ORBstopPollMs zero",
        );
        assert!(OrbConfig::from_orb_args(&bad).is_err());
        // The type is what enforces it: there is no half-built value to leak,
        // because the value is returned only on the success path.
        let (good, _) = OrbConfig::from_orb_args(&bad[..bad.len() - 2]).unwrap();
        assert_eq!(good.max_message_size(), 1000, "the same six read fine on their own");
        assert_eq!(good.stop_poll(), STOP_POLL, "and the seventh stayed at its default");
    }

    /// `-ORB` is a prefix, so an argument that merely starts with it and is not
    /// one of ours must be refused rather than silently passed through as a
    /// program argument — that is §8.5.1's rule and the whole reason the
    /// unknown case is an error.
    ///
    /// # The bare `-ORB`
    ///
    /// §8.5.1 writes the pattern as `-ORB<suffix>`, and a bare `-ORB` is that
    /// pattern with an empty suffix. This test first asserted it should survive
    /// as a program argument and **the code refused it**; the code is right and
    /// the test was the guess. An operator who typed `-ORB` has mistyped an ORB
    /// argument — no program sensibly claims that spelling for itself — and
    /// §8.5.1's answer to a matching-but-unrecognised argument is `BAD_PARAM`.
    /// Passing it through would hand a mangled ORB argument to the program as
    /// if it had asked for it.
    #[test]
    fn an_orb_prefixed_argument_is_never_passed_through_silently() {
        assert!(matches!(
            OrbConfig::from_orb_args(&args("-ORBtypo 1")),
            Err(ConfigError::Unknown { .. })
        ));
        assert!(
            matches!(OrbConfig::from_orb_args(&args("-ORB")), Err(ConfigError::Unknown { .. })),
            "a bare -ORB is the pattern with an empty suffix, not a program argument"
        );
        // Something that only *looks* similar does not match the pattern and
        // must survive: the match is case-sensitive and anchored at the start.
        let (_, rest) = OrbConfig::from_orb_args(&args("-orbfoo --ORBbar ORB")).unwrap();
        assert_eq!(
            rest,
            args("-orbfoo --ORBbar ORB"),
            "case-sensitive and prefix-anchored, as §8.5.1 writes it"
        );
    }
}
