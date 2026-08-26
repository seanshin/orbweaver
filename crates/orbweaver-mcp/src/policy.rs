//! Default-deny exposure: what an agent is allowed to see and call.
//!
//! `docs/PLAN.md` §4.6 and §9.0: **nothing in the registry is reachable through
//! MCP until it is explicitly allowlisted.** The registry is populated from
//! whatever IDL a deployment has, which in a legacy estate is everything —
//! including the operations that move money and the ones that delete things.
//! A projection that exposes by default exposes those on the day someone adds
//! a file.
//!
//! Deny-by-default is also the only rule that stays correct as the catalog
//! grows. An allowlist gets stale in the safe direction; a denylist gets stale
//! in the other one.
//!
//! # Two gates, not one
//!
//! Being *exposed* and being *callable without a human* are different
//! questions. An operation annotated `ai_effect: destructive` may be visible,
//! describable and still refused unless the caller presents an approval. The
//! annotation comes from SIDL (§2.2), so the person who wrote the contract is
//! the one who decides — not the person wiring up the bridge.
//!
//! # Silence is not consent
//!
//! The gate above keys on an annotation, and until the estate pilot
//! (`docs/pipeline-runs/2026-08-14-estate.md`, RC-5) it asked for one with
//! `annotations.get("ai_effect")?` — so a **misspelled** effect needed a human
//! and a **missing** one did not. Measured over a thirteen-file legacy estate
//! that exposed 76 of 76 operations to a caller holding no scopes at all,
//! `SystemConsole.SHUTDOWN` and `AuditSink.purge` among them. That is not a
//! quirk of that estate: an unannotated contract is what every legacy contract
//! is, and the gate was reading *the contract has nothing to say* as *the
//! contract says yes*.
//!
//! [`Effect`] makes the three answers three answers. `Harmless` and `Stated`
//! are what the contract says; [`Effect::Unstated`] is the contract saying
//! nothing, and nothing is not a permission. What happens to a silence is
//! [`Unannotated`], which is a decision an **operator** takes once for an
//! exposure — not one this crate takes for them by defaulting, and not one it
//! extracts 76 times from whoever is clicking the approvals.
//!
//! 침묵은 승인이 아니다. 애너테이션이 **없는** 것과 **오타난** 것은 서로 다른
//! 답이어야 하고, 지금까지는 우연히 반대 방향으로 달랐다.

use std::collections::BTreeSet;

use orbweaver_registry::Registry;

use crate::identity::Caller;

/// What a **host** has decided, as distinct from what a caller claims.
///
/// Never built from the agent's own request. A caller that can assert its own
/// approval has no approval gate, so this arrives from the process that
/// authenticated the human — at present the operator who launched the bridge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Approval {
    /// A human has approved this specific call.
    pub destructive_approved: bool,
}

/// Why a call or a description was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denied {
    /// The interface is not allowlisted.
    InterfaceNotExposed(String),
    /// The interface is exposed but this operation is not.
    OperationNotExposed {
        /// Repository id.
        id: String,
        /// Operation name.
        operation: String,
    },
    /// The contract requires an authorization scope the caller does not hold.
    MissingScope {
        /// Repository id.
        id: String,
        /// Operation name.
        operation: String,
        /// What `ai_authz` asks for.
        required: String,
    },
    /// The caller's credential has outlived its grant ([`crate::token::Expiry`],
    /// §4.8's fourth discomfort).
    ///
    /// Classified as [`crate::dryrun::Would::NeedAuthentication`] rather than
    /// given a row of its own, deliberately: the two say the same thing to the
    /// person reading the report — *sign in again* — and splitting one action
    /// into two rows would make an operator choose between them. What separates
    /// them in the record is the `why`, which names the expiry.
    CredentialExpired {
        /// The principal whose credential lapsed.
        principal: String,
        /// How long ago it lapsed, or `None` when the host has supplied no
        /// instant and the stage therefore **cannot tell**. The two are
        /// different facts and a stage that cannot tell must never render as
        /// *still valid*; see [`crate::token::Unstamped`].
        overdue_secs: Option<u64>,
    },
    /// The contract names an authorization requirement and nobody is
    /// authenticated to satisfy it.
    NotAuthenticated {
        /// Repository id.
        id: String,
        /// Operation name.
        operation: String,
        /// What `ai_authz` asks for.
        required: String,
    },
    /// The operation is exposed but marked destructive and unapproved.
    NeedsApproval {
        /// Repository id.
        id: String,
        /// Operation name.
        operation: String,
        /// What the contract says it does, if it says.
        effect: String,
        /// Whether `effect` came from the **exposure's** [`Unannotated`]
        /// assumption rather than from the contract. An operator reading an
        /// approval request has to know whether the word `destructive` was
        /// written by the person who owns the interface or by the person who
        /// wired up the bridge.
        assumed: bool,
        /// Who the contract says may approve it ([`crate::dryrun::AI_APPROVER`]),
        /// or `None` where it does not say.
        ///
        /// Carried on the refusal rather than only in the dry-run report
        /// because *waiting for whom* is half of what a caller stuck at this
        /// gate needs, and the gate already had it: [`effect_refusal`] holds
        /// the registry at the moment it refuses. Still **data** — nothing here
        /// acts on it (§9.0 R11); it is rendered by [`Denied::remedy`] and
        /// nowhere else.
        approver: Option<String>,
    },
    /// The contract states no `ai_effect` for this operation and the exposure
    /// declares no assumption for the silence.
    ///
    /// **The variant that used to be an `allow`.** It is deliberately not
    /// [`Denied::NeedsApproval`]: an approval is a human saying yes to a
    /// specific call, and nobody can say yes to a call whose effect nobody has
    /// stated. Routing silences into the approval queue turns a legacy estate
    /// into seventy-six approvals, which is the shape of gate people learn to
    /// click through — and one `--approve` would then unlock the whole estate
    /// at once.
    ///
    /// It is also deliberately not [`Denied::InterfaceNotExposed`] or
    /// [`Denied::OperationNotExposed`]: the operator *did* expose it, and
    /// answering "not exposed" would send them hunting through the allowlist
    /// for a problem that is in the contract. The estate pilot recorded that
    /// misdirection reaching production by another road (RC-4), which is why
    /// this refusal names the annotation instead.
    EffectUnstated {
        /// Repository id.
        id: String,
        /// Operation name.
        operation: String,
    },
    /// A stage of [`crate::interceptor::Chain`] outside the built-in gates
    /// refused the call — a deployment's rate limiter, quota or safety filter.
    ///
    /// The variant exists so that a stage nobody here wrote refuses in the
    /// same currency as one that is: the same `Denied`, so it reaches the
    /// caller as the same `ToolError` and the audit log as the same line.
    /// A chain whose extensions had to invent their own refusal type would
    /// have two refusal paths, and only one of them audited.
    Intercepted {
        /// Which stage refused, since the audit line's fixed format has
        /// nowhere else to put it.
        stage: String,
        /// What that stage says about it.
        ///
        /// **The one field of a refusal that this crate did not write, and the
        /// only one that does not reach the audit ledger.** A stage at
        /// [`crate::interceptor::SEAT_SAFETY_CONTENT`] holds the argument
        /// values, so its sentence can hold one too;
        /// `crate::guard::audit_reason` therefore renders an `Intercepted`
        /// into the ledger by its `stage` alone. This text still reaches the
        /// caller, the [`crate::dryrun`] report and every observer stage — the
        /// readers who already have the arguments.
        reason: String,
    },
    /// `describe_type` was asked about a repository id the caller **can
    /// already see** and that is not a type.
    ///
    /// **The variant that exists because a refusal misdirected.** The first
    /// version of `describe_type` answered every undescribable id with
    /// [`Denied::InterfaceNotExposed`], which is right for a type nothing
    /// exposed reaches — a refusal there must not confirm what sits behind the
    /// gate. It is wrong for an **exposed interface**: an agent that asked
    /// `describe_type` for `IDL:bank/Account:1.0` was told *"is not exposed"*
    /// about a contract the operator had just exposed, and would go hunting
    /// through the allowlist for a problem that is in the request. That is the
    /// RC-4 misdirection this file already refuses once, in
    /// [`Denied::EffectUnstated`]'s note — arriving by a third road.
    ///
    /// It leaks nothing: it is answered **only** for an id the exposure
    /// already exposes, so the caller could have learned the same thing from
    /// `describe_interface`. Everything else — a type nothing reaches, an id
    /// nobody declared — keeps the one indistinguishable answer.
    ///
    /// Found by driving the shipped binary rather than by a unit test, which
    /// is the lesson `tests/serving_audit.rs` opens with.
    NotAType {
        /// Repository id.
        id: String,
        /// What the catalog says it is, in IDL's own word.
        kind: String,
    },
    /// A consumption budget is spent ([`crate::quota`], §4.5 #2).
    ///
    /// **The one variant that is not about permission.** Every other refusal
    /// here is a statement about what this caller may do, and re-asking cannot
    /// change it; this one is a statement about what has been *used*, and a
    /// later window can. That difference is why it is a variant of its own
    /// rather than an [`Denied::Intercepted`] with a well-chosen sentence:
    /// [`Denied::is_transient`] has to be answerable by a match, because
    /// [`crate::guard::refusal_id`] turns it into the system exception a stub's
    /// caller reads, and a retry decision taken by grepping prose is not one.
    QuotaExhausted {
        /// What the budget is counted against, rendered in the audit line's own
        /// field spelling — `caller=alice target=… operation=…`.
        budget: String,
        /// What has been spent against it.
        used: u64,
        /// What it allows.
        limit: u64,
        /// The window the host last opened, or `-` for a host that has opened
        /// none. See [`crate::quota::Window`]: there is no clock in this crate.
        window: String,
        /// Whether a later window can change this answer — the operator's
        /// [`crate::quota::Renewal`], not an inference. A stage with no clock
        /// cannot know that time will pass.
        renews: bool,
    },
}

impl Denied {
    /// Whether this refusal is a "not right now" rather than a "you may not".
    ///
    /// True only for [`Denied::QuotaExhausted`] on a budget that renews.
    /// [`crate::guard::refusal_id`] is the one place that turns this into a
    /// repository id, so the answer a stub's caller retries on and the answer
    /// the trace records cannot disagree.
    pub fn is_transient(&self) -> bool {
        matches!(self, Denied::QuotaExhausted { renews: true, .. })
    }

    /// **What would make this call legitimate**, from what the gate already
    /// held when it refused.
    ///
    /// S4 gives an IDL diagnostic a position and a fix hint, and this project
    /// counts diagnostics as a product; a refused *call* used to get a rule id
    /// and a reason and not the one thing an agent needs next. Nothing here is
    /// inferred, discovered or guessed: every clause below is built from fields
    /// this refusal already carries — the repository id the allowlist does not
    /// name, the scope the contract asks for, the annotation the contract does
    /// not carry, the budget's own key. This is `orbweaver-forge`'s
    /// finding-plus-fix shape applied to calls, and it is deliberately the
    /// *same* shape: an agent that meets two refusal formats from one system
    /// learns neither.
    ///
    /// # The rule, and why it is written at the site
    ///
    /// **A remedy is not a way around the refusal.** Default-deny stays
    /// default-deny. Every sentence below names an act belonging to somebody
    /// who is not the caller — an operator widening an allowlist, the host that
    /// issues credentials, a human approving, the author of the contract
    /// annotating it — and none of them names a route the agent can take by
    /// itself. If a remedy would read as *"ask again with X"* where `X` is
    /// something the agent controls and the gate exists to stop, that remedy is
    /// wrong and the refusal is correct as it stands. The next person to extend
    /// this will feel the pull; [`REMEDY_ACTORS`] and [`REMEDY_FORBIDDEN`] are
    /// how the tests hold the line, and they are published here rather than
    /// retyped in a test so that a changed vocabulary changes both at once.
    ///
    /// The one apparent exception is [`Denied::QuotaExhausted`] on a renewing
    /// budget, where waiting *is* the answer. It is not an exception to the
    /// rule: that gate exists to bound a rate and not to bound a permission, so
    /// a later window is the legitimate path and [`Denied::is_transient`]
    /// already says so in the exception a stub's caller reads. The sentence
    /// still names **the host** as what opens the next window, because nothing
    /// about the *request* changes the count.
    ///
    /// # It is a `String` and the match is exhaustive
    ///
    /// Not an `Option`, and no `_ =>` arm. A refusal an agent can receive owes
    /// it a next step, so the compiler is what asks the next variant's author
    /// for one — which is the whole of the codification, since a rule about
    /// diagnostics that lives only in a document is a rule the next variant
    /// will not read.
    ///
    /// *거절은 우회로가 아니다. 모든 구제책은 호출자가 아닌 누군가의 행위를
    /// 이름한다 — 운영자, 호스트, 승인하는 사람, 계약의 저자. 에이전트가 혼자
    /// 갈 수 있는 길은 결코 적지 않는다.*
    pub fn remedy(&self) -> String {
        match self {
            // Names the id and says the allowlist is default-deny by design. It
            // must not say how to get on the allowlist: that is an operator's
            // act, and an agent reading instructions for it is the failure this
            // gate exists to prevent.
            Denied::InterfaceNotExposed(id) => format!(
                "an operator must add {id} to this bridge's exposure allowlist; exposure is \
                 default-deny by design and no request can widen it"
            ),
            Denied::OperationNotExposed { id, operation } => format!(
                "an operator must add {operation:?} to the operations allowed on {id}; the \
                 interface is exposed and this operation is not, and no request can widen it"
            ),
            // The scope is right there in the comparison the stage just made.
            Denied::MissingScope { id, operation, required } => format!(
                "the scope {required:?} is what {id}.{operation} asks for, and the host that \
                 issued this caller's credential is what grants it; a call cannot widen its own \
                 scopes"
            ),
            Denied::NotAuthenticated { id, operation, required } => format!(
                "the host must authenticate a caller before {id}.{operation} can be checked \
                 against anything, and that caller has to hold the scope {required:?}"
            ),
            Denied::CredentialExpired { principal, overdue_secs: Some(_) } => format!(
                "the host must re-authenticate {principal}; an expired credential cannot be \
                 extended from the caller's side"
            ),
            Denied::CredentialExpired { principal, overdue_secs: None } => format!(
                "the host must supply the current instant to the expiry gate before {principal}'s \
                 credential can be judged still valid"
            ),
            Denied::NeedsApproval { id, operation, effect, assumed, approver } => {
                let mut out = format!(
                    "a human must approve this specific call of {id}.{operation} and the approval \
                     reaches this bridge from the host that authenticated them; a caller cannot \
                     assert its own approval"
                );
                if let Some(who) = approver {
                    out.push_str(&format!(", and the contract names {who} as who may approve"));
                }
                // Said only for an assumed effect. The remedy for a *stated*
                // one is the approval and nothing else: telling a caller its
                // contract could say something different would be inviting an
                // edit to get past a gate, which is the shape this whole
                // function refuses.
                if *assumed {
                    out.push_str(&format!(
                        ". {id}.{operation} could also state an ai_effect of its own, so that \
                         {effect} comes from the contract's author rather than from this \
                         exposure's assumption"
                    ));
                }
                out
            }
            // **The sentence S4 writes for the same condition, and now
            // literally the same sentence.** It used to be this crate's own
            // wording, and the comment here said why: "neither crate depends
            // on the other". That stopped being true on 2026-08-26, when the
            // `orbweaver-forge -> orbweaver-mcp` edge was reversed so the
            // boundary could reach the pipeline it exposes — which also put
            // the sentence's owner within reach.
            //
            // The offer stays **two values** where S4 offers three, and
            // `effect::OFFER_GATE` carries the argument at its own site: a
            // remedy is read by the agent that was just refused, and the
            // choice its operator faces is a pole, not a menu. `None` for the
            // flag for the same reason — naming `--assume-effect` here would
            // address a reader who cannot run it.
            Denied::EffectUnstated { id, operation } => format!(
                "{}; until one of those happens {id}.{operation} has nobody's statement to rest on",
                orbweaver_forge::effect::annotate_or_assume(
                    &orbweaver_forge::effect::OFFER_GATE,
                    None
                )
            ),
            // Names the stage and **not one word the stage wrote**, for the
            // reason `crate::guard::audit_reason` gives: that prose is the only
            // part of a refusal this crate did not write, and the seat it comes
            // from is the one that holds argument values.
            Denied::Intercepted { stage, .. } => format!(
                "the {stage} stage was installed by this deployment, so what would satisfy it is \
                 an operator's answer and not this bridge's"
            ),
            // The one remedy that points at another tool, and it is not a way
            // around a gate: nothing was denied here by policy. The contract
            // is what makes this id an interface, and naming the tool that
            // describes one is telling the caller what it asked for exists
            // elsewhere — not how to get past a refusal.
            Denied::NotAType { id, kind } => format!(
                "the contract declares {id} as {kind} and not as a type, so there is no type for \
                 describe_type to describe; describe_interface is what reads an interface"
            ),
            Denied::QuotaExhausted { budget, window, renews: true, .. } => format!(
                "the budget {budget} is spent for window {window:?} and the host is what opens \
                 the next one; nothing about the request changes the count"
            ),
            Denied::QuotaExhausted { budget, renews: false, .. } => format!(
                "an operator must raise or reset the limit on {budget}; this budget does not \
                 renew, so no later window will"
            ),
        }
    }
}

/// The actors a [`Denied::remedy`] is allowed to name, and every remedy names
/// at least one of them.
///
/// The positive half of the rule that keeps a remedy from becoming a way
/// around the refusal: the act that would make the call legitimate belongs to
/// somebody who is **not the caller**, so the sentence has to say who. A
/// remedy that names none of these is either vague or is addressing the agent,
/// and both are defects.
///
/// Published here rather than retyped in a test because a classifier that
/// matches a hand-written substring of a sentence some other function owns
/// drifts the moment the sentence changes for a good reason.
pub const REMEDY_ACTORS: [&str; 4] = ["an operator", "the host", "a human", "the contract"];

/// Phrasings a [`Denied::remedy`] may never contain — the negative half.
///
/// Each is a way of telling the caller to act on **its own request**: to send
/// it again, to send it differently, or to hand itself something. A gate whose
/// refusal ends in one of these is a gate that has explained how to get past
/// it. The second-person forms are here because that is how such a sentence
/// arrives: a remedy addressed to *you* is a remedy the agent is expected to
/// carry out.
///
/// `retry` is on the list even though a renewing budget genuinely does invite
/// one — [`Denied::is_transient`] is where that is said, in the currency a
/// stub's caller acts on, and saying it a second time in prose would put the
/// retry decision back into text somebody has to grep.
pub const REMEDY_FORBIDDEN: [&str; 10] = [
    "you ",
    "your ",
    "yourself",
    "retry",
    "try again",
    "ask again",
    "call again",
    "resend",
    "re-send",
    "request it again",
];

/// The refusal as everything that reads one hears it: **the fact, then what
/// would make the call legitimate.**
///
/// The remedy is a second sentence in the same string rather than a field on
/// the enum, and that is a decision about reach rather than about taste. Every
/// consumer of a refusal in this crate already takes it as prose through this
/// one rendering — [`crate::ToolError`]'s `Display`, which `crate::session`
/// hands `crate::rpc::tool_error` verbatim; the [`crate::dryrun`] report's
/// `why`; the audit
/// ledger by way of `crate::guard::audit_reason`; every observer stage handed a
/// `CallResult::Refused`. A field would have taught exactly the readers that
/// were rewritten to ask for it and silently not the others, which is the
/// familiar shape of a fact with two homes. [`Denied::remedy`] is still
/// separately callable, so a structured consumer that wants the halves apart
/// has them.
///
/// It reaches the ledger too, and that is intended: the remedy names an
/// operator's act, and an operator grepping `REFUSE` lines is the actor most of
/// these sentences are about. It is safe there for the reason
/// `crate::guard::audit_reason` states — a remedy is built from repository ids,
/// operation names, scope names and a budget key, and never from a byte the
/// agent sent. The one variant whose prose could hold a payload,
/// [`Denied::Intercepted`], gets a remedy that names its stage and quotes
/// nothing.
impl std::fmt::Display for Denied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fact(f)?;
        write!(f, ". To make this call legitimate: {}", self.remedy())
    }
}

impl Denied {
    /// The refusal as it stood before it taught anything: what was refused and
    /// why, with no next step. Split out of `Display` so that the two halves
    /// have one writer each and neither can restate the other.
    fn fact(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Denied::InterfaceNotExposed(id) => write!(
                f,
                "{id} is not exposed. Nothing is reachable through this bridge until it is \
                 allowlisted"
            ),
            Denied::OperationNotExposed { id, operation } => {
                write!(f, "{id} is exposed but {operation:?} is not among its allowed operations")
            }
            Denied::NotAType { id, kind } => {
                write!(f, "{id} is {kind}, not a type")
            }
            Denied::MissingScope { id, operation, required } => write!(
                f,
                "{id}.{operation} requires the scope {required:?}, which this caller does not \
                 hold"
            ),
            Denied::CredentialExpired { principal, overdue_secs: Some(secs) } => write!(
                f,
                "the credential for {principal} expired {secs}s ago; a call must not proceed on \
                 an expired context, so this session must re-authenticate"
            ),
            Denied::CredentialExpired { principal, overdue_secs: None } => write!(
                f,
                "the credential for {principal} carries an expiry and the host has supplied no \
                 instant to check it against; a stage that cannot tell must not read as still \
                 valid, so this call is refused until the host stamps the expiry gate"
            ),
            Denied::NotAuthenticated { id, operation, required } => write!(
                f,
                "{id}.{operation} requires the scope {required:?} and this session has no \
                 authenticated caller, so there is nobody to check it against"
            ),
            Denied::NeedsApproval { id, operation, effect, assumed: false, .. } => write!(
                f,
                "{id}.{operation} is marked {effect} and needs an explicit approval before it \
                 can be called"
            ),
            // Who said the word matters to whoever is being asked to approve.
            Denied::NeedsApproval { id, operation, effect, assumed: true, .. } => write!(
                f,
                "{id}.{operation} states no ai_effect and this exposure assumes {effect} for the \
                 operations that state none, so it needs an explicit approval before it can be \
                 called"
            ),
            // Names the annotation, not the allowlist. A refusal that said only
            // "no" would send an operator into a permissions config looking for
            // a problem that is in the contract. The clause that says *which*
            // annotation moved to [`Denied::remedy`] when every refusal grew
            // one: it was this variant's second sentence before it was every
            // variant's, and leaving a copy here would have made it the only
            // refusal that says its remedy twice.
            Denied::EffectUnstated { id, operation } => write!(
                f,
                "{id}.{operation} carries no ai_effect, so the contract does not say whether an \
                 agent may call it without a human, and this bridge will not guess one"
            ),
            Denied::Intercepted { stage, reason } => {
                write!(f, "the {stage} stage refused this call: {reason}")
            }
            // The leading token is load-bearing: it is what separates a
            // consumption refusal from a permission refusal in a log an
            // operator greps, and the closing clause is what tells a stuck
            // agent's owner whether waiting is a strategy.
            Denied::QuotaExhausted { budget, used, limit, window, renews } => write!(
                f,
                "quota exhausted: {budget} has used {used} of {limit} calls in window \
                 {window:?}; {}",
                if *renews {
                    "retry in a later window"
                } else {
                    "this budget does not renew, so retrying will not help"
                }
            ),
        }
    }
}

impl std::error::Error for Denied {}

/// What the contract says an operation does — the input to the approval gate.
///
/// Three answers, because there are three facts. Until the estate pilot there
/// were two, and the third was silently folded into "harmless".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// `ai_effect` names one of the values that need no human: `read_only`,
    /// `readonly`, `idempotent`, `safe`.
    ///
    /// An attribute **getter** is also this, and by the IDL grammar rather than
    /// by an annotation: `_get_x` reads `x`, which is a fact the contract
    /// states in the language it is written in. That is a statement, not a
    /// silence, so it is not [`Effect::Unstated`]. What a getter may *leak* is
    /// the scope gate's question, and [`required_scopes`] guards both accessors
    /// from the attribute's own `ai_authz`.
    Harmless,
    /// `ai_effect` names something else — stated by whoever wrote the contract
    /// and not on the harmless list.
    ///
    /// Both `destructive` and a typo'd `destructve` land here, and both need a
    /// human. A value nobody anticipated is not a reason to let a call through.
    Stated(String),
    /// **No `ai_effect` reaches this operation.** The contract does not say.
    ///
    /// Distinct from [`Effect::Stated`] on purpose, and in the direction that
    /// costs something: a typo gets a human's yes because somebody was writing
    /// annotations and got one wrong, while a silence gets sent back to the
    /// contract because nobody has written anything for a human to say yes to.
    Unstated,
}

/// What an exposure does with an operation whose contract states no
/// `ai_effect` — [`Effect::Unstated`].
///
/// The operator's decision, taken **once** for an exposure. That is the whole
/// design: failing closed per operation is correct and produces one approval
/// per silence, and a gate that asks seventy-six times is a gate somebody
/// automates away. One declaration, recorded in every report and every refusal
/// that rests on it, is a decision that can be reviewed; seventy-six clicks are
/// not.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Unannotated {
    /// Refuse, naming the missing annotation ([`Denied::EffectUnstated`]).
    ///
    /// **The default, and the only safe one.** A bridge that shipped any other
    /// default would be making a safety claim about contracts it has never
    /// seen.
    #[default]
    Refuse,
    /// Read a silence as if the contract had said this.
    ///
    /// The escape hatch for an estate nobody is going to annotate this quarter,
    /// and it is deliberately an *effect value* rather than a boolean, so it
    /// runs through the same recognition the contract's own value does:
    /// `Assume("read_only")` allows the silences, `Assume("destructive")` sends
    /// them to the approval queue. It never touches an operation whose contract
    /// **does** state an effect — an assumption about silences cannot downgrade
    /// something somebody wrote.
    ///
    /// Every document and every refusal that rests on one says so
    /// (`assumed: true`, `effect_stated_by: "exposure"`,
    /// `unannotated_effect` in a [`crate::dryrun::survey`]), because the
    /// difference between "the contract says this is safe" and "we assumed it
    /// was" is the whole of what an operator is signing.
    Assume(String),
}

/// `IDL:module/Iface:1.0[.operation]` — the interface, and the operation if one
/// is named.
///
/// **The one reading of that grammar**, for every surface an operator writes an
/// exposure on: `--expose`, `--dry-run=`, and a configuration file's `expose`
/// list. It lived in the server binary while the command line was the only such
/// surface; a file is a second, and a repository id that meant one thing in a
/// flag and another in a file would be an allowlist entry silently naming a
/// different operation than the operator wrote.
///
/// The operation is split at the last dot. A repository id ends in its
/// *version*, `:1.0`, which has a dot in it, so the trailing part is only an
/// operation when it looks like an IDL identifier: a bare `IDL:spike/Echo:1.0`
/// used to be read as the interface `IDL:spike/Echo:1` with an operation named
/// `0`, which allowlisted an interface nobody had and exposed nothing. The
/// first `--dry-run` report run against a real IDL file said
/// `id: IDL:spike/Echo:1, operation: 0, declared: false`, which is how that was
/// found.
pub fn split_operation(spec: &str) -> (&str, Option<&str>) {
    let identifier = |op: &str| op.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_');
    match spec.rsplit_once('.') {
        Some((id, op)) if identifier(op) && !op.contains(':') => (id, Some(op)),
        _ => (spec, None),
    }
}

/// Which interfaces and operations an agent may reach, and what it does with
/// the operations whose contracts say nothing.
#[derive(Debug, Default, Clone)]
pub struct Exposure {
    /// Keys are repository ids; the value is the set of allowed operations, or
    /// empty for "every operation this interface declares".
    allowed: std::collections::BTreeMap<String, BTreeSet<String>>,
    /// What happens to an [`Effect::Unstated`] operation.
    unannotated: Unannotated,
}

impl Exposure {
    /// An exposure that permits nothing. This is the only sensible starting
    /// point, and it is what `Default` gives.
    pub fn nothing() -> Self {
        Self::default()
    }

    /// Allows every operation of an interface.
    ///
    /// Broader than naming operations, and deliberately still explicit: it
    /// covers operations added *later*, so a contract that grows grows the
    /// exposure with it. Name operations individually where that matters.
    pub fn allow_interface(mut self, id: impl Into<String>) -> Self {
        self.allowed.entry(id.into()).or_default();
        self
    }

    /// Allows one operation of an interface.
    pub fn allow_operation(mut self, id: impl Into<String>, operation: impl Into<String>) -> Self {
        self.allowed.entry(id.into()).or_default().insert(operation.into());
        self
    }

    /// Declares what this exposure assumes for operations whose contract states
    /// no `ai_effect`. See [`Unannotated`]; the default is
    /// [`Unannotated::Refuse`].
    ///
    /// This is the operator's single decision about an unannotated estate, and
    /// it is why failing closed does not cost seventy-six approvals.
    pub fn assuming_unannotated(mut self, policy: Unannotated) -> Self {
        self.unannotated = policy;
        self
    }

    /// The posture this exposure takes on operations that state no `ai_effect`.
    ///
    /// [`crate::interceptor::Chain::standard`] copies it into the approval
    /// stage, and [`crate::dryrun::survey`] renders it, so a report and the
    /// gate it predicts cannot disagree about what a silence means.
    pub fn unannotated(&self) -> &Unannotated {
        &self.unannotated
    }

    /// Whether an interface may be searched or described.
    pub fn exposes(&self, id: &str) -> bool {
        self.allowed.contains_key(id)
    }

    /// Every exposed repository id.
    pub fn interfaces(&self) -> impl Iterator<Item = &String> {
        self.allowed.keys()
    }

    /// The operations named for `id`. Empty means either "every operation this
    /// interface declares" or "this interface is not exposed" — [`exposes`] is
    /// what separates those, and no decision should be taken from this alone.
    ///
    /// It exists for [`crate::dryrun::survey`], which needs the names an
    /// operator *wrote* and not only the ones the contract declares: an
    /// exposure line for an operation that does not exist allowlists nothing,
    /// and a report that enumerated only the contract would never mention it.
    ///
    /// [`exposes`]: Exposure::exposes
    pub fn allowed_operations(&self, id: &str) -> impl Iterator<Item = &String> {
        self.allowed.get(id).into_iter().flatten()
    }

    /// Whether an operation is within the exposed set, ignoring approval.
    pub fn exposes_operation(&self, id: &str, operation: &str) -> bool {
        match self.allowed.get(id) {
            None => false,
            Some(ops) => ops.is_empty() || ops.contains(operation),
        }
    }

    /// The full check a call must pass.
    ///
    /// Order matters for what the caller learns: an operation on an unexposed
    /// interface reports the interface, never "no such operation", because the
    /// second answer would confirm or deny the existence of operations on
    /// something the caller was not permitted to see.
    ///
    /// This is one composition of the rules; [`crate::interceptor::Chain`] is
    /// the other, stage by stage, and it is what a call actually runs through.
    /// Both call the same primitives — nothing is decided twice — and
    /// `the_chain_and_check_call_answer_alike` pins them to the same verdict
    /// case by case. This one stays because a *question* about a call
    /// (`Bridge::check`) must be answerable without auditing and counting an
    /// invocation that never happened.
    pub fn check_call(
        &self,
        registry: &Registry,
        id: &str,
        operation: &str,
        approval: Approval,
        caller: Option<&Caller>,
    ) -> Result<(), Denied> {
        if !self.exposes(id) {
            return Err(Denied::InterfaceNotExposed(id.to_owned()));
        }
        if !self.exposes_operation(id, operation) {
            return Err(Denied::OperationNotExposed {
                id: id.to_owned(),
                operation: operation.to_owned(),
            });
        }
        // The authorization row of §4.8's table. The requirement is written in
        // the contract by whoever owns the interface, so it is checked before
        // the effect gate — an unauthorised caller should not be told which
        // operations would merely have needed an approval.
        for required in required_scopes(registry, id, operation) {
            match caller {
                None => {
                    return Err(Denied::NotAuthenticated {
                        id: id.to_owned(),
                        operation: operation.to_owned(),
                        required,
                    });
                }
                Some(c) if !c.scopes.contains(&required) => {
                    return Err(Denied::MissingScope {
                        id: id.to_owned(),
                        operation: operation.to_owned(),
                        required,
                    });
                }
                Some(_) => {}
            }
        }
        if let Some(why) = effect_refusal(registry, &self.unannotated, id, operation, approval) {
            return Err(why);
        }
        Ok(())
    }
}

/// The effect gate, in **one** place: what the contract states, what the
/// operator assumed for the silences, and the approval in hand.
///
/// [`Exposure::check_call`] and [`crate::interceptor::ApprovalInterceptor`]
/// both call this, for the reason [`required_scopes`] gives — one
/// implementation of the rule, two compositions of it. A second copy is how
/// the dry run and the live gate come to different conclusions.
pub(crate) fn effect_refusal(
    registry: &Registry,
    unannotated: &Unannotated,
    id: &str,
    operation: &str,
    approval: Approval,
) -> Option<Denied> {
    let (effect, assumed) = match stated_effect(registry, id, operation) {
        Effect::Harmless => return None,
        Effect::Stated(effect) => (effect, false),
        // The silence. What happens to it is the operator's declaration, and
        // the default declaration is to refuse and say which annotation is
        // missing.
        Effect::Unstated => match unannotated {
            Unannotated::Refuse => {
                return Some(Denied::EffectUnstated {
                    id: id.to_owned(),
                    operation: operation.to_owned(),
                });
            }
            // Run through the same recognition the contract's own value gets,
            // so `Assume("read_only")` and `//@ ai_effect: read_only` cannot
            // mean different things.
            Unannotated::Assume(assumed) if is_harmless(assumed) => return None,
            Unannotated::Assume(assumed) => (assumed.clone(), true),
        },
    };
    // An approval is a human saying yes to a call whose effect somebody stated.
    // It is reachable here and unreachable above, which is the point: one
    // `--approve` must not unlock every operation nobody has described.
    (!approval.destructive_approved).then(|| Denied::NeedsApproval {
        id: id.to_owned(),
        operation: operation.to_owned(),
        effect,
        assumed,
        // Read through the same constant the dry-run report reads it by, so the
        // approver a report names and the approver a refusal names cannot come
        // to differ. Absent is absent: nothing is guessed when the contract is
        // silent about who may say yes.
        approver: registry
            .resolve_operation(id, operation)
            .and_then(|(_, sig)| sig.annotations.get(crate::dryrun::AI_APPROVER).cloned()),
    })
}

/// The `ai_effect` values that need no human.
///
/// **Published, and one line.** This predicate is the gate's own — it is what
/// [`effect_refusal`] asks before letting a call through — and it had two
/// hand-kept mirrors in other crates (`orbweaver_forge::annotate::
/// UNGATED_EFFECTS` and `orbweaver_test::contract::UNGATED_EFFECTS`), each
/// documented as a mirror and each free to fall behind it. The list now has
/// one home in [`orbweaver_forge::effect::UNGATED`], which every layer that
/// needs it reads, so the three cannot disagree.
///
/// It is `pub` because a classifier outside this crate that wants to know what
/// the gate will let through must be able to **ask** rather than to retype —
/// CLAUDE.md's *a classifier is a sentence too*.
pub fn is_harmless(value: &str) -> bool {
    orbweaver_forge::effect::is_harmless(value)
}

/// The scopes `ai_authz` asks for, comma-separated in the annotation.
///
/// [`crate::interceptor::ScopeInterceptor`] reads the requirement through this
/// same function: one implementation of the rule, two compositions of it.
///
/// # Why an absent `ai_authz` is *not* [`Effect::Unstated`]'s cause
///
/// This gate keys on an annotation too, and an operation with no `ai_authz`
/// requires no scope — which reads like the same fail-open the effect gate had.
/// It was examined with it and deliberately left alone, for two reasons that
/// are about this gate specifically rather than about appetite:
///
/// 1. **There is nothing to fail closed *to*.** A scope refusal is actionable
///    because it names the scope to grant. An absent `ai_authz` names none, so
///    the only "closed" available is *refuse everything*, whose fix hint would
///    be "add an `ai_authz`" — wrong advice for an operation whose author
///    decided it needs none.
/// 2. **The silence is no longer reachable un-vetted.** An operation nobody
///    annotated at all is now stopped by [`effect_refusal`] before this
///    question matters. What survives to here is an operation whose author
///    *was* writing annotations and chose not to require a scope, which is a
///    decision rather than an absence.
///
/// What remains, and is a finding rather than a fix: `//@ ai_effect: read_only`
/// with no `ai_authz` is world-readable by anyone the exposure lets in, and on
/// a balance-reading operation that is a real hole. It is a **contract-quality**
/// problem, so the instrument for it is S4's advice and `contract-check`, not a
/// gate that can only refuse.
///
/// 부재한 `ai_authz`는 같은 원인이 아니다 — 닫을 대상이 없고, 애초에 미주석
/// 연산은 효과 게이트에서 이미 멈춘다.
pub(crate) fn required_scopes(registry: &Registry, id: &str, operation: &str) -> Vec<String> {
    let annotations = match registry.resolve_operation(id, operation) {
        Some((_, sig)) => &sig.annotations,
        // An attribute accessor is an operation on the wire and nothing else
        // in this crate knew it. `resolve_operation` looks only at declared
        // operations, so `_get_balance` had no signature, no annotations and
        // therefore no scopes — while `allow_interface` made it perfectly
        // callable. An `//@ ai_authz` written on an attribute bought nothing.
        None => match attribute_annotations(registry, id, operation) {
            Some(a) => a,
            None => return Vec::new(),
        },
    };
    annotations
        .get("ai_authz")
        .map(|v| v.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default()
}

/// Whether the interface (or an ancestor) declares the attribute this accessor
/// name reaches.
pub(crate) fn declares_accessor(registry: &Registry, id: &str, operation: &str) -> bool {
    attribute_annotations(registry, id, operation).is_some()
        || operation
            .strip_prefix("_set_")
            .is_some_and(|n| attribute_annotations(registry, id, &format!("_get_{n}")).is_some())
}

/// The annotations of the attribute an accessor name reaches, if any.
///
/// `_get_x` and `_set_x` are what an attribute becomes on the wire (§4.4), so
/// the policy has to be able to see them. Ancestors are walked because an
/// inherited attribute is callable on the derived interface — the same reason
/// `resolve_operation` walks them for operations.
///
/// A `_set_` on a `readonly` attribute resolves to nothing here: the servant
/// answers `BAD_OPERATION` and there is no annotation to honour, so treating
/// it as gated would invent a control over a call that cannot happen.
///
/// The ancestor walk is [`crate::resolved_attributes`]'s and not a fourth copy
/// of one — the estate pilot's RC-8 was a walk written out longhand in one
/// place and omitted in another.
fn attribute_annotations<'r>(
    registry: &'r Registry,
    id: &str,
    operation: &str,
) -> Option<&'r std::collections::BTreeMap<String, String>> {
    let (name, is_set) = match operation.strip_prefix("_get_") {
        Some(n) => (n, false),
        None => (operation.strip_prefix("_set_")?, true),
    };
    let (_, _, attr) =
        crate::resolved_attributes(registry, id).into_iter().find(|(n, _, _)| n == name)?;
    if is_set && attr.readonly {
        return None;
    }
    Some(&attr.annotations)
}

/// What the contract says this operation does. **Never a policy decision** —
/// [`effect_refusal`] is the only thing that turns this into a verdict, and
/// [`crate::dryrun`] renders it into a report.
///
/// Reads the resolved surface, so an operation inherited from a base is judged
/// by the annotation its declaring interface carries.
pub(crate) fn stated_effect(registry: &Registry, id: &str, operation: &str) -> Effect {
    let annotations = match registry.resolve_operation(id, operation) {
        Some((_, sig)) => &sig.annotations,
        // An `ai_effect` on an attribute describes **writing** it. Applying it
        // to the getter as well made reading a `destructive` attribute demand a
        // human approval, which is not what the annotation says and is the kind
        // of gate people learn to click through. A scope is the other way round
        // — it guards the value, so it guards both accessors.
        None if operation.starts_with("_set_") => {
            match attribute_annotations(registry, id, operation) {
                Some(a) => a,
                // A `_set_` on a `readonly` attribute, or on nothing at all.
                // Neither reaches a servant, so there is no call for a contract
                // to have described; the exposure gate and argument mapping are
                // what answer it.
                None => return Effect::Unstated,
            }
        }
        // A getter is a read, stated by the grammar rather than by an
        // annotation. See `Effect::Harmless`.
        None if operation.starts_with("_get_") && declares_accessor(registry, id, operation) => {
            return Effect::Harmless;
        }
        None => return Effect::Unstated,
    };
    match annotations.get("ai_effect") {
        None => Effect::Unstated,
        Some(v) if is_harmless(v) => Effect::Harmless,
        Some(v) => Effect::Stated(v.trim().to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The version's dot is not an operation's, and an operation's is.
    #[test]
    fn a_repository_ids_version_is_not_an_operation() {
        assert_eq!(split_operation("IDL:spike/Echo:1.0"), ("IDL:spike/Echo:1.0", None));
        assert_eq!(
            split_operation("IDL:spike/Echo:1.0.echo"),
            ("IDL:spike/Echo:1.0", Some("echo"))
        );
        assert_eq!(
            split_operation("IDL:spike/Echo:1.0._get_balance"),
            ("IDL:spike/Echo:1.0", Some("_get_balance"))
        );
        assert_eq!(split_operation("IDL:a/B:1.0.2"), ("IDL:a/B:1.0.2", None), "not an identifier");
    }

    /// An `ai_authz` written on an attribute used to buy nothing: the accessor
    /// has no operation signature, so the scope check found no annotations and
    /// let the call through — while `allow_interface` made `_get_balance`
    /// perfectly callable. Invisible *and* ungated.
    #[test]
    fn an_attribute_accessor_is_gated_by_the_attributes_own_scope() {
        let r = registry(
            "module m { interface I {
               //@ ai_authz: bank.balance.read
               readonly attribute long balance;
               //@ ai_authz: bank.label.write
               attribute string label;
               long ping();
             }; };",
        );
        assert_eq!(required_scopes(&r, "IDL:m/I:1.0", "_get_balance"), ["bank.balance.read"]);
        assert_eq!(required_scopes(&r, "IDL:m/I:1.0", "_set_label"), ["bank.label.write"]);
        assert_eq!(required_scopes(&r, "IDL:m/I:1.0", "_get_label"), ["bank.label.write"]);
        // A `_set_` on a readonly attribute reaches no annotation: the servant
        // answers BAD_OPERATION, and inventing a control over a call that
        // cannot happen would be a gate nobody can pass or fail.
        assert!(required_scopes(&r, "IDL:m/I:1.0", "_set_balance").is_empty());
        assert!(required_scopes(&r, "IDL:m/I:1.0", "ping").is_empty());
    }

    /// And an inherited attribute is callable on the derived interface, so the
    /// walk has to reach it — the same reason `resolve_operation` walks bases.
    #[test]
    fn an_inherited_attributes_scope_is_found_from_the_derived_interface() {
        let r = registry(
            "module m {
               interface Base {
                 //@ ai_authz: base.reading
                 readonly attribute long reading;
               };
               interface Derived : Base { long ping(); };
             };",
        );
        assert_eq!(required_scopes(&r, "IDL:m/Derived:1.0", "_get_reading"), ["base.reading"]);
    }

    /// A `_set_` is a mutation, and it was never approval-gated for the same
    /// reason it was never scope-gated.
    /// An `ai_effect` on an attribute describes writing it. Applied to the
    /// getter it made *reading* a destructive attribute demand a human
    /// approval — a gate on the wrong operation, and the kind people learn to
    /// click through. A scope is the other way round: it guards the value, so
    /// it guards both accessors.
    #[test]
    fn an_effect_gates_the_setter_and_not_the_getter() {
        let r = registry(
            "module m { interface I {
               //@ ai_effect: destructive
               //@ ai_authz: m.mode
               attribute string mode;
             }; };",
        );
        assert_eq!(
            stated_effect(&r, "IDL:m/I:1.0", "_set_mode"),
            Effect::Stated("destructive".into())
        );
        // A getter is a read by the grammar, so it is `Harmless` and **not**
        // `Unstated`: refusing every attribute read of every legacy contract
        // for want of an annotation IDL already implies would be a gate nobody
        // could satisfy without rewriting the contract.
        assert_eq!(stated_effect(&r, "IDL:m/I:1.0", "_get_mode"), Effect::Harmless);
        assert_eq!(required_scopes(&r, "IDL:m/I:1.0", "_get_mode"), ["m.mode"]);
        assert_eq!(required_scopes(&r, "IDL:m/I:1.0", "_set_mode"), ["m.mode"]);
    }

    #[test]
    fn a_setter_can_require_approval() {
        let r = registry(
            "module m { interface I {
               //@ ai_effect: destructive
               attribute string mode;
             }; };",
        );
        assert_eq!(
            stated_effect(&r, "IDL:m/I:1.0", "_set_mode"),
            Effect::Stated("destructive".into())
        );
    }

    /// A writable attribute nobody annotated is the estate's
    /// `AuditSink._set_enabled` — the operation that turns an audit log off,
    /// measured as `allow` to a caller holding no scopes. The getter beside it
    /// stays allowed, because reading is what `_get_` means.
    #[test]
    fn an_unannotated_setter_is_refused_and_its_getter_is_not() {
        let r = registry("module m { interface Sink { attribute boolean enabled; }; };");
        let e = Exposure::nothing().allow_interface("IDL:m/Sink:1.0");
        assert_eq!(stated_effect(&r, "IDL:m/Sink:1.0", "_set_enabled"), Effect::Unstated);
        assert!(matches!(
            e.check_call(&r, "IDL:m/Sink:1.0", "_set_enabled", Approval::default(), None),
            Err(Denied::EffectUnstated { .. })
        ));
        assert!(
            e.check_call(&r, "IDL:m/Sink:1.0", "_get_enabled", Approval::default(), None).is_ok()
        );
    }

    fn registry(src: &str) -> Registry {
        let spec = orbweaver_idl::parse(src).expect("parses");
        let mut r = Registry::new();
        r.load(&spec).expect("loads");
        r
    }

    const IDL: &str = r#"
        module bank {
          interface Account {
            //@ ai_effect: read_only
            long balance();
            //@ ai_effect: destructive
            void close();
            void touch();
          };
          interface Ledger { long total(); };
        };"#;

    #[test]
    fn nothing_is_exposed_by_default() {
        let r = registry(IDL);
        let e = Exposure::nothing();
        assert!(!e.exposes("IDL:bank/Account:1.0"));
        assert_eq!(
            e.check_call(&r, "IDL:bank/Account:1.0", "balance", Approval::default(), None),
            Err(Denied::InterfaceNotExposed("IDL:bank/Account:1.0".into()))
        );
    }

    #[test]
    fn allowlisting_an_interface_covers_its_operations() {
        let r = registry(IDL);
        let e = Exposure::nothing().allow_interface("IDL:bank/Account:1.0");
        assert!(
            e.check_call(&r, "IDL:bank/Account:1.0", "balance", Approval::default(), None).is_ok()
        );
        // `touch` carries no `ai_effect`, so allowlisting the interface is not
        // enough to call it: an allowlist says *what an agent may reach* and an
        // `ai_effect` says *what it does*, and this gate needs both. It used to
        // pass here, which is the defect.
        assert!(matches!(
            e.check_call(&r, "IDL:bank/Account:1.0", "touch", Approval::default(), None),
            Err(Denied::EffectUnstated { .. })
        ));
        // And still covers nothing else.
        assert!(!e.exposes("IDL:bank/Ledger:1.0"));
    }

    /// **The estate defect, in one assertion.** `annotations.get("ai_effect")?`
    /// read a missing key as permission, so a thirteen-file legacy estate
    /// exposed 76 of 76 operations — `SystemConsole.SHUTDOWN`,
    /// `AuditSink.purge`, `InvoiceService.void_invoice` — to a caller holding
    /// no scopes at all.
    ///
    /// If this test ever reads `is_ok()` again, the bridge has gone back to
    /// telling an autonomous agent that an operation nobody has described is
    /// safe to call against somebody's production ORB.
    #[test]
    fn an_operation_the_contract_says_nothing_about_is_refused() {
        let r = registry("module m { interface Console { void SHUTDOWN(in string reason); }; };");
        let e = Exposure::nothing().allow_interface("IDL:m/Console:1.0");
        let d = e.check_call(&r, "IDL:m/Console:1.0", "SHUTDOWN", Approval::default(), None);
        assert!(matches!(d, Err(Denied::EffectUnstated { .. })), "{d:?}");
    }

    /// The refusal has to send the reader to the **contract**. An operator who
    /// is told only "no" goes looking through a permissions config for a
    /// problem that is an annotation problem — the misdirection the estate
    /// recorded arriving by another road (RC-4).
    #[test]
    fn the_refusal_names_the_annotation_that_is_missing() {
        let why =
            Denied::EffectUnstated { id: "IDL:m/Console:1.0".into(), operation: "SHUTDOWN".into() }
                .to_string();
        assert!(why.contains("ai_effect"), "{why}");
        assert!(why.contains("IDL:m/Console:1.0.SHUTDOWN"), "{why}");
        // And it must not read as an exposure problem.
        assert!(!why.contains("not exposed"), "{why}");
    }

    /// **Absent and unrecognised are different answers, deliberately.** They
    /// already differed before this batch — by accident, and in the wrong
    /// direction: a typo needed a human and a silence did not.
    ///
    /// The direction now: a typo reaches a human, because somebody was writing
    /// annotations and got one wrong and there is a value for a person to read.
    /// A silence goes back to the contract, because there is nothing to say yes
    /// to.
    #[test]
    fn an_absent_effect_and_an_unrecognised_one_are_different_answers() {
        let r = registry(
            "module m { interface I {
               //@ ai_effect: probably_fine
               void typo();
               void silent();
             }; };",
        );
        let e = Exposure::nothing().allow_interface("IDL:m/I:1.0");
        assert!(matches!(
            e.check_call(&r, "IDL:m/I:1.0", "typo", Approval::default(), None),
            Err(Denied::NeedsApproval { assumed: false, .. })
        ));
        assert!(matches!(
            e.check_call(&r, "IDL:m/I:1.0", "silent", Approval::default(), None),
            Err(Denied::EffectUnstated { .. })
        ));
    }

    /// An approval is a human saying yes to a call somebody described. One
    /// `--approve` must not unlock every operation nobody has described — that
    /// is the fail-open default coming back through the approval flag.
    #[test]
    fn an_approval_in_hand_does_not_unlock_a_silence() {
        let r = registry("module m { interface I { void wipe(); }; };");
        let e = Exposure::nothing().allow_interface("IDL:m/I:1.0");
        let approved = Approval { destructive_approved: true };
        assert!(matches!(
            e.check_call(&r, "IDL:m/I:1.0", "wipe", approved, None),
            Err(Denied::EffectUnstated { .. })
        ));
    }

    /// The operator's one declaration, and its limits. It covers the silences
    /// and **only** the silences: an assumption about what nobody wrote cannot
    /// downgrade what somebody did.
    #[test]
    fn an_assumption_covers_the_silences_and_only_the_silences() {
        let r = registry(
            "module m { interface I {
               //@ ai_effect: destructive
               void close();
               void silent();
             }; };",
        );
        let e = Exposure::nothing()
            .allow_interface("IDL:m/I:1.0")
            .assuming_unannotated(Unannotated::Assume("read_only".into()));
        assert!(e.check_call(&r, "IDL:m/I:1.0", "silent", Approval::default(), None).is_ok());
        assert!(matches!(
            e.check_call(&r, "IDL:m/I:1.0", "close", Approval::default(), None),
            Err(Denied::NeedsApproval { assumed: false, .. })
        ));
    }

    /// The other useful setting, and the field that keeps it honest: an
    /// approval request that rests on an assumption says so, because the
    /// operator being asked has to know whether `destructive` is the interface
    /// owner's word or the bridge operator's.
    #[test]
    fn an_assumed_destructive_needs_an_approval_and_says_whose_word_it_is() {
        let r = registry("module m { interface I { void silent(); }; };");
        let e = Exposure::nothing()
            .allow_interface("IDL:m/I:1.0")
            .assuming_unannotated(Unannotated::Assume("destructive".into()));
        let d = e.check_call(&r, "IDL:m/I:1.0", "silent", Approval::default(), None);
        assert!(matches!(d, Err(Denied::NeedsApproval { assumed: true, .. })), "{d:?}");
        let why = d.unwrap_err().to_string();
        assert!(why.contains("states no ai_effect"), "{why}");
        assert!(why.contains("this exposure assumes"), "{why}");
        // And an approval in hand clears it, unlike an unassumed silence: the
        // operator has stated what they are approving.
        assert!(
            e.check_call(
                &r,
                "IDL:m/I:1.0",
                "silent",
                Approval { destructive_approved: true },
                None
            )
            .is_ok()
        );
    }

    /// `Unannotated::Refuse` is the default, and nothing in this crate may
    /// quietly pick another one. A bridge that shipped any other default would
    /// be making a safety claim about contracts it has never seen.
    #[test]
    fn the_default_posture_on_a_silence_is_refusal() {
        assert_eq!(Unannotated::default(), Unannotated::Refuse);
        assert_eq!(Exposure::nothing().unannotated(), &Unannotated::Refuse);
    }

    /// The neighbouring gate, examined with the effect gate and deliberately
    /// **not** changed. See [`required_scopes`]'s docs for why: an absent
    /// `ai_authz` names no scope, so the only "closed" available is *refuse
    /// everything* with a fix hint that would be wrong.
    ///
    /// The pin is here so that the reasoning is a decision on the record rather
    /// than an omission somebody re-derives as a bug.
    #[test]
    fn an_absent_ai_authz_still_requires_no_scope_and_that_is_deliberate() {
        let r = registry("module m { interface I { //@ ai_effect: read_only\n long peek(); }; };");
        assert!(required_scopes(&r, "IDL:m/I:1.0", "peek").is_empty());
        // Reachable only because the author *did* annotate: a contract that
        // says nothing at all never gets this far.
        let e = Exposure::nothing().allow_interface("IDL:m/I:1.0");
        assert!(e.check_call(&r, "IDL:m/I:1.0", "peek", Approval::default(), None).is_ok());
    }

    #[test]
    fn naming_operations_excludes_the_ones_not_named() {
        let r = registry(IDL);
        let e = Exposure::nothing().allow_operation("IDL:bank/Account:1.0", "balance");
        assert!(
            e.check_call(&r, "IDL:bank/Account:1.0", "balance", Approval::default(), None).is_ok()
        );
        assert_eq!(
            e.check_call(&r, "IDL:bank/Account:1.0", "touch", Approval::default(), None),
            Err(Denied::OperationNotExposed {
                id: "IDL:bank/Account:1.0".into(),
                operation: "touch".into()
            })
        );
    }

    /// The second gate. Being visible is not being callable.
    #[test]
    fn a_destructive_operation_needs_an_approval_even_when_exposed() {
        let r = registry(IDL);
        let e = Exposure::nothing().allow_interface("IDL:bank/Account:1.0");
        let denied = e.check_call(&r, "IDL:bank/Account:1.0", "close", Approval::default(), None);
        assert!(matches!(denied, Err(Denied::NeedsApproval { .. })), "{denied:?}");
        assert!(
            e.check_call(
                &r,
                "IDL:bank/Account:1.0",
                "close",
                Approval { destructive_approved: true },
                None
            )
            .is_ok()
        );
    }

    /// An `ai_effect` value nobody anticipated must not be read as permission.
    #[test]
    fn an_unrecognised_effect_is_treated_as_needing_approval() {
        let r = registry("module m { interface I { //@ ai_effect: probably_fine\n void f(); }; };");
        let e = Exposure::nothing().allow_interface("IDL:m/I:1.0");
        assert!(matches!(
            e.check_call(&r, "IDL:m/I:1.0", "f", Approval::default(), None),
            Err(Denied::NeedsApproval { .. })
        ));
    }

    /// The refusal must not become an oracle for what exists behind it.
    #[test]
    fn an_unexposed_interface_reveals_nothing_about_its_operations() {
        let r = registry(IDL);
        let e = Exposure::nothing();
        let real = e.check_call(&r, "IDL:bank/Account:1.0", "balance", Approval::default(), None);
        let invented =
            e.check_call(&r, "IDL:bank/Account:1.0", "no_such_op", Approval::default(), None);
        assert_eq!(real, invented, "the two answers must be indistinguishable");
    }

    /// The authorization row of §4.8's table: `ai_authz` in the contract,
    /// scopes on the caller, matched here.
    #[test]
    fn an_ai_authz_scope_is_enforced_against_the_caller() {
        // Annotated with an effect as well as a scope: the scope gate is what
        // this test is about, and an operation the effect gate would stop for
        // an unrelated reason would not exercise it to the end.
        let r = registry(
            "module bank { interface Account { //@ ai_authz: accounts:write\n \
             //@ ai_effect: idempotent\n void close(); }; };",
        );
        let e = Exposure::nothing().allow_interface("IDL:bank/Account:1.0");

        // Nobody signed in: refused, and the message says why.
        let d = e.check_call(&r, "IDL:bank/Account:1.0", "close", Approval::default(), None);
        assert!(matches!(d, Err(Denied::NotAuthenticated { .. })), "{d:?}");

        // Signed in without the scope: refused.
        let alice = Caller::new("alice").with_scope("accounts:read");
        let d =
            e.check_call(&r, "IDL:bank/Account:1.0", "close", Approval::default(), Some(&alice));
        assert!(matches!(d, Err(Denied::MissingScope { .. })), "{d:?}");

        // With the scope: allowed.
        let admin = Caller::new("root").with_scope("accounts:write");
        assert!(
            e.check_call(&r, "IDL:bank/Account:1.0", "close", Approval::default(), Some(&admin))
                .is_ok()
        );
    }

    /// Several scopes, comma-separated, all required.
    #[test]
    fn every_listed_scope_is_required_not_any() {
        let r = registry(
            "module m { interface I { //@ ai_authz: a:read, b:write\n \
             //@ ai_effect: read_only\n void f(); }; };",
        );
        let e = Exposure::nothing().allow_interface("IDL:m/I:1.0");
        let partial = Caller::new("x").with_scope("a:read");
        assert!(matches!(
            e.check_call(&r, "IDL:m/I:1.0", "f", Approval::default(), Some(&partial)),
            Err(Denied::MissingScope { required, .. }) if required == "b:write"
        ));
        let full = Caller::new("x").with_scope("a:read").with_scope("b:write");
        assert!(e.check_call(&r, "IDL:m/I:1.0", "f", Approval::default(), Some(&full)).is_ok());
    }

    /// The scope gate runs before the effect gate: an unauthorised caller is
    /// not told which operations would merely have needed approval.
    #[test]
    fn the_scope_gate_answers_before_the_approval_gate() {
        let r = registry(
            "module m { interface I { //@ ai_authz: admin\n //@ ai_effect: destructive\n void wipe(); }; };",
        );
        let e = Exposure::nothing().allow_interface("IDL:m/I:1.0");
        let d = e.check_call(&r, "IDL:m/I:1.0", "wipe", Approval::default(), None);
        assert!(matches!(d, Err(Denied::NotAuthenticated { .. })), "{d:?}");
    }

    #[test]
    fn an_operation_inherited_from_a_base_is_checked_like_any_other() {
        let r = registry(
            "module m { interface Base { //@ ai_effect: destructive\n void wipe(); }; \
             interface Derived : Base {}; };",
        );
        let e = Exposure::nothing().allow_interface("IDL:m/Derived:1.0");
        assert!(matches!(
            e.check_call(&r, "IDL:m/Derived:1.0", "wipe", Approval::default(), None),
            Err(Denied::NeedsApproval { .. })
        ));
    }
}
