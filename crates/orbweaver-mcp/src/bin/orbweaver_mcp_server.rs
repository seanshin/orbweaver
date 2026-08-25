//! `orbweaver-mcp-server` — the CORBA estate as three MCP tools over stdio.
//!
//! ```text
//! orbweaver-mcp-server --idl <file.idl>... --ior <file> [-I <dir>]... \
//!                      [--config <policy.json>] \
//!                      [--expose <IDL:module/Iface:1.0[.operation]>]... \
//!                      [--assume-effect <ai_effect value>] \
//!                      [--as <principal>] [--scope <scope>]... \
//!                      [--map-scope <token-scope>=<contract-scope>]... [--token-scope <s>]... \
//!                      [--dry-run[=<IDL:module/Iface:1.0[.operation]>]] [--dry-run-args <json>] \
//!                      [--dry-run-handle <name>=<IOR:...|file>]... \
//!                      [--trace <path>|-] [--trace-ts <rfc3339>] \
//!                      [--quota <calls>] [--quota-scope <everything|caller|interface|operation>] \
//!                      [--audit-capacity <lines>]
//! ```
//!
//! Exposure is **default-deny**: with no `--expose` and no `expose` in a
//! `--config`, the server starts, answers the handshake, and finds nothing.
//! That is the correct behaviour and not a misconfiguration — an operator
//! naming what an agent may reach is the point.
//!
//! # `--config`: the numbers a deployment owns, in a file
//!
//! Every flag below is a thing an operator says at the moment they start the
//! process. Three things they own were sayable **nowhere**: how long a
//! capability lives (`orbweaver_mcp::handles::DEFAULT_TTL` and a builder only
//! tests called), how many references a session may hold, and how many results
//! a search that names no limit returns. `--config <policy.json>` is where
//! those live, along with a file's copy of the exposure, the assumption, the
//! quota, the ledger bound and the dial timeout —
//! `orbweaver_mcp::deployment::Deployment` documents the shape and the reasons.
//!
//! Three properties, each of which is the reason for the next:
//!
//! - **No `--config` means no change.** The parsed value supplies `None` for
//!   every setting and installs nothing, so a deployment that does not use the
//!   flag runs the code it ran before the flag existed. There is no search
//!   path and no file picked up from the working directory: a configuration
//!   this process found on its own could start applying to a deployment nobody
//!   changed.
//! - **A flag beats the file** where both speak, so an invocation that worked
//!   before still means what it meant. `expose` is the exception and is a
//!   *union*: both are the operator naming something explicitly, and neither
//!   can be widened by an absence — a missing `expose`, an empty one, an empty
//!   document and an absent flag all leave the allowlist exactly where the
//!   command line put it.
//! - **A malformed file stops the process**, naming the file, the key and what
//!   was expected — including a key no setting is named by, because a typo an
//!   operator wrote is a setting they believe is in force. Nothing is applied
//!   on the way to finding the fault: the document parses whole or not at all,
//!   and a half-applied policy is worse than none because it looks like one.
//!
//! What is in force is said on stderr, wherever it is in force — including
//! under `--dry-run`, where a report that hid the quota it was predicting
//! against would be the report disagreeing with the run.
//!
//! *배포가 소유하는 수치는 파일에 산다. `--config`가 없으면 아무것도 달라지지
//! 않고, 플래그가 파일을 이기며, 잘못된 파일은 파일·키·기대값을 말하고 프로세스를
//! 멈춘다. 부분 적용은 없다.*
//!
//! # `--idl` takes a path, so it takes a translation unit
//!
//! Each `--idl` file is read with its `#include`s resolved — the including
//! file's own directory first, then any `-I <dir>`, which is `sidl-validate`'s
//! flag meaning `sidl-validate`'s thing. Several `--idl` files that include the
//! same header are safe: the header's declarations are identical whichever root
//! reached them, and a later load of the same id replaces the earlier one.
//!
//! This process is where reading a file as a string cost the most. **It could
//! not serve an estate at all**: point it at thirteen legacy contracts that
//! include each other and every base declared next door was dropped without a
//! word, so the catalog an agent searched, the operations `--expose` could name,
//! and the surface `--dry-run` reported were all smaller than the contracts an
//! operator had handed it — and the report said nothing was missing. Measured
//! on `spikes/estate/`: stripping the `#include` lines drops **27 references**,
//! 8 base interfaces and 19 raised exceptions. `spikes/estate/run.sh` worked
//! around it by amalgamating the estate into one file before this process saw
//! it; that workaround is now a second way of doing it rather than the only one.
//!
//! An exposure decision taken over a partial catalog is the shape of failure
//! this whole boundary exists to prevent, so a reference that will not resolve
//! **refuses the start** and names what is missing. A gate that serves what it
//! could read and stays quiet about what it could not is worse than one that
//! will not start.
//!
//! *`--idl`은 경로를 받으므로 번역 단위를 받는다. 문자열로 읽던 시절 이 프로세스는
//! 레거시 에스테이트를 아예 서비스할 수 없었다 — 상속 기반이 조용히 사라진 채로
//! 더 작은 표면을 서비스했고, 무엇이 빠졌는지 아무 말도 하지 않았다.*
//!
//! # `--assume-effect`: what a contract's silence means, declared once
//!
//! An operation whose contract carries no `//@ ai_effect` is **refused**, and
//! the refusal names the missing annotation. That is a change: it used to be
//! allowed, because the gate asked the annotation map for a key and read
//! `None` as *nothing to worry about*. The estate pilot measured what that is
//! worth on a real legacy set — 76 of 76 operations allowed to a caller holding
//! no scopes, `SHUTDOWN` and `purge` among them.
//!
//! Refusing per operation is correct and, on its own, unusable: seventy-six
//! refusals an operator has to clear one at a time is a gate people automate
//! away. `--assume-effect <value>` is the one declaration that replaces them —
//! *for the operations that state nothing, assume this*. It runs through the
//! same recognition a contract's own value does, so `--assume-effect read_only`
//! allows them and `--assume-effect destructive` sends them to the approval
//! queue. It never touches an operation whose contract **does** state an effect.
//!
//! Whatever is chosen, this process says at startup how many operations of the
//! exposure carry no `ai_effect`, because the size of the silence is the fact
//! the decision is about. Every dry-run document carries `unannotated_effect`
//! at the top and marks each row that rests on the assumption
//! (`effect_stated_by: "exposure"`), so a page of `allow` cannot be mistaken
//! for a page of annotated contracts.
//!
//! 계약이 `ai_effect`를 말하지 않으면 **거부**하고, 무엇이 없는지 이름을 말한다.
//! 연산마다 거부하는 것만으로는 쓸 수 없으므로, 침묵에 대한 가정은
//! `--assume-effect`로 **한 번** 선언한다.
//!
//! # `--dry-run`: what would this exposure let an agent do?
//!
//! Prints the policy report for `--as`'s caller over everything `--expose`
//! names — every operation, what would happen to it, and why not — then exits
//! without serving anything. No target is dialled and `--ior` is not required,
//! because the question is asked before there is a deployment to point at. It
//! is the instrument for signing an exposure off; see `orbweaver_mcp::dryrun`.
//!
//! Three grains, one grammar. `--dry-run` surveys everything `--expose` names;
//! `--dry-run=<id>` surveys one interface, exposed or not; and
//! `--dry-run=<id>.<operation>` — the operation split off exactly as `--expose`
//! splits it — asks about **one operation** and prints that operation's own
//! document (`orbweaver_mcp::dryrun::Prediction::to_json`: `would`, `declared`,
//! the refusing `stage` and `why`, every stage's part) instead of a survey.
//! That is the grain at which values can be declared:
//!
//! ```text
//! --dry-run=IDL:gc27/Ledger:1.0.keep --dry-run-args '{"key":"123456789","entry":{…}}'
//! ```
//!
//! `--dry-run-args <json>` is the AnyJSON object an agent would send as the
//! call's arguments. With it the prediction also says whether the payload would
//! **marshal** against the contract's `TypeCode`s — the library maps and encodes
//! it in both byte orders into a buffer that is dropped — and the row carries
//! `payload: marshals` or `payload: would_not_marshal` with `payload_why`; a
//! payload that would not fit an operation the gate would otherwise allow turns
//! `would` to `marshal` and names `raises` (`MARSHAL`). A `string<8>` given
//! nine characters predicts `allow` without values and `marshal` with them: the
//! value-less question is a policy verdict and says so by saying nothing about
//! a payload. Still no target: nothing is dialed with values any more than
//! without them (`orbweaver_mcp::dryrun`, "Nothing reaches the wire").
//!
//! An object reference in the arguments is `{"_ref": <handle>}` — D008's one
//! notation, the same the agent sends — and a handle resolves against **this
//! run's own capability table**. A `--dry-run` issues none on its own, so a
//! declared reference used to predict `would_not_marshal` from the command
//! line however valid the target; the one instrument an operator has could not
//! ask about `heartbeat(in Expert e, …)` at all. `--dry-run-handle
//! <name>=<IOR:…|file>` (repeatable) is how the run comes to hold one:
//!
//! ```text
//! --dry-run=IDL:moe/ExpertRegistry:1.0.heartbeat \
//! --dry-run-handle expert=/var/run/expert.ior \
//! --dry-run-args '{"e":{"_ref":"expert"},"updated_cap":{…}}'
//! ```
//!
//! The IOR is **parsed and never dialed** — read from the file (or given
//! inline as `IOR:…`), issued into the session's table through the same
//! `issue_checked` the serving path issues its root handle through, so the
//! handle carries the same repository id, the same expiry and the same 128
//! bits from `/dev/urandom` a live `resolve` would have issued. The token
//! does not exist before this process runs, so `--dry-run-args` names the
//! reference by the **name** the flag gave it: every `{"_ref": "<name>"}` in
//! the arguments is rewritten to the token before the library sees the
//! document, and the library sees exactly what an agent would have sent. A
//! `_ref` that names no `--dry-run-handle` is left as written and resolves to
//! nothing — `would_not_marshal`, `at e: no reference is held under handle
//! "expert"` — which is the negative control and the same sentence a forged
//! handle earns from a live call. The document carries `handles:
//! {"<name>": "<token>"}` so a reader can see what was held; the token is
//! session-scoped and the session ends with the process, so the line is
//! worthless to anybody who reads it later, exactly like `root handle:` on
//! stderr. Nothing about the target — host, port, object key — reaches
//! stdout, stderr or the ledger; `dryrun_handle.rs` holds that against a
//! listener nobody connects to.
//!
//! 값과 함께 물으면 예측은 페이로드가 계약의 `TypeCode`에 맞는지도 답한다 —
//! 양쪽 바이트 순서로, 버려지는 버퍼에 인코딩해서. 값 없이 물은 예측은 정책의
//! 답일 뿐이며, 페이로드에 대해 아무 말도 하지 않는 것으로 그렇다고 말한다.
//! 인자 속 객체 참조는 `{"_ref": <핸들>}`이며 이 실행의 테이블에서 해석된다.
//! `--dry-run-handle <이름>=<IOR|파일>`은 IOR을 **파싱만 하고 다이얼하지 않은
//! 채** 라이브 경로와 같은 발급 경로로 테이블에 넣고, `--dry-run-args`의
//! `{"_ref": "<이름>"}`을 발급된 토큰으로 바꾼다. 선언되지 않은 이름은 그대로
//! 남아 `would_not_marshal`이 되며, 그것이 음성 대조군이다.
//!
//! # `--map-scope`: the vocabulary gap, made loud before a call
//!
//! A token's scopes are the identity provider's vocabulary and `ai_authz`'s are
//! the contract's, and D005 measured what happens when they drift apart: a
//! deployment whose IdP issues `gate:operate` against a contract that demands
//! `parkinglot.barrier.open` **refuses every legitimate caller**, and the
//! refusal is indistinguishable from a permissions misconfiguration.
//!
//! `--map-scope <token>=<contract>` declares the translation
//! (`orbweaver_mcp::token::ScopeMap`) and `--token-scope <s>` declares what a
//! token from this IdP carries — which is also what `--as`'s caller holds, after
//! translation, so the report and the run agree about the same caller.
//! `--scope` is unchanged and still names **contract** scopes directly, for a
//! host whose caller is already in the contract's vocabulary.
//!
//! With any `--map-scope` given, `--dry-run`'s document carries a `scope_map`
//! section naming every contract scope no token can ever satisfy, who requires
//! it, every token scope placed nowhere, and every mapping nothing asks for.
//! **A finding exits 3**, because a report whose exit code is always zero is a
//! report a pipeline never reads.
//!
//! # `--trace`: one JSON line per decision
//!
//! D004 tier 1. `--trace <path>` appends span records to a file, `--trace -`
//! writes them to stderr, and without the flag nothing is emitted and nothing is
//! built. The record shape is fixed in `docs/decisions/D004-observability.md`
//! and implemented in `orbweaver_mcp::telemetry`; it is a machine-read trace,
//! not a second audit format — the audit lines still go to stderr as they did.
//!
//! **`--trace-ts` exists because this process has no clock.** The `ts` field
//! comes from the caller, which here is the command line: whatever `--trace-ts`
//! says is what every line of the run carries, and without it every line reads
//! `"ts":"-"`. That is the honest rendering of a process that never reads a
//! clock, and it is what makes two runs of the same session byte-identical —
//! the property the harness diffs against. A server that stamped lines from
//! `SystemTime::now()` would produce a trace nobody could replay, which is the
//! discipline D004's table cites (`PLAN-DEFERRED.md` §3).
//!
//! # `--quota`: a budget for the run, which is the only window this process has
//!
//! §4.5 #2's seat, filled by `orbweaver_mcp::quota`. `--quota <calls>` caps how
//! many calls this session may make, `--quota-scope` says what the cap is
//! counted against (per caller by default), and the refusal names the
//! arithmetic and is distinguishable from a policy refusal in both the audit
//! line and the trace.
//!
//! The budget **does not renew**, and that is a statement about this process
//! rather than a limitation of the mechanism. A renewing quota needs somebody
//! to open the next window, and a window is a label a *host* supplies —
//! `orbweaver_mcp::quota::Window`, on the same no-clock discipline as
//! `--trace-ts`. This process reads no clock and has no window source, so a
//! budget here is a per-run total and its refusals say `NO_PERMISSION`:
//! telling an agent to retry in a window that nobody will ever open would be a
//! lie with a retry loop attached to it. A host that *does* have a clock builds
//! the same `Quota` with `Renewal::Window` and calls `open_window` itself.
//!
//! # The audit ledger goes to stderr, one line per decision
//!
//! §4.8 asks for a record of every decision, and until now this process met
//! that requirement only under `--dry-run`. Serving, the ALLOW/REFUSE lines
//! accumulated in the chain's audit stage and were **discarded when the process
//! exited** — the library kept the ledger and the deployment threw it away, so
//! the requirement was satisfied by a test and not by anything an operator
//! could read. The end-to-end run found it.
//!
//! Now each line is emitted to stderr as it is written, alongside the root
//! handle and every other diagnostic; stdout stays protocol-only. Emission
//! happens once the frame that produced it has been handled and *before* its
//! response is written, and once more after the loop — so neither a client
//! that reacts to a refusal by killing the process, nor a decision made by the
//! last frame before EOF, can lose its line.
//!
//! It is stderr rather than a file because that is where this process already
//! puts everything an operator reads, and redirecting a stream is something a
//! supervisor already knows how to do. A trace goes to `--trace <path>`; the
//! audit ledger goes where the diagnostics go.
//!
//! # `--audit-capacity`: the in-memory ledger is bounded, and says so
//!
//! Emitting to stderr left the *library's* ledger growing for the life of the
//! session, which the batch that landed the emission recorded as a known limit.
//! `orbweaver_mcp::interceptor::AuditInterceptor` now keeps its newest
//! `--audit-capacity` lines (65,536 by default) and spends one slot on an
//! elision marker naming how many it dropped — because an audit ledger that
//! drops lines silently reads exactly like a quiet period, and the two are
//! indistinguishable at the moment somebody is reading the log to tell them
//! apart.
//!
//! **The two streams answer different questions and only one of them is
//! bounded.** stderr is the complete ledger: every line is emitted as it is
//! written, before the frame's response goes out, so it is the record an
//! operator keeps. The in-memory slice is what `Bridge::audit` hands to §7.4
//! I4's promotion oracle, and it is bounded because a process that runs for a
//! week must not be holding every decision it ever made. The marker is what
//! keeps the second honest about being a window onto the first, and
//! `promote::verify_promotion` refuses it by name rather than judging a
//! promotion from a gap.
//!
//! # stdout is the protocol
//!
//! One JSON object per line on stdout and nothing else, ever. Every diagnostic
//! goes to stderr. A single stray `println!` desynchronises the session, and
//! the client reports it as malformed JSON rather than as the bug it is. The
//! `--dry-run` report goes to stdout and is exempt only because it never enters
//! the loop: it prints one JSON object and the process ends.

use std::io::{BufRead, Write};

use orbweaver_dynamic::json::Json;
use orbweaver_giop::{Connection, Ior};
use orbweaver_mcp::Bridge;
use orbweaver_mcp::deployment::{DEFAULT_CONNECT_TIMEOUT, Deployment};
use orbweaver_mcp::identity::Caller;
use orbweaver_mcp::policy::{Approval, Exposure, Unannotated, split_operation};
use orbweaver_mcp::quota::Scope;
use orbweaver_mcp::session::Session;
use orbweaver_mcp::telemetry::{CallPath, JsonLines, Timestamp, Trace};
use orbweaver_mcp::token::ScopeMap;
use orbweaver_registry::{Strictness, registry_from_files, take_include_dirs};

/// D004's trace for this run: `-` is stderr, anything else is a file.
///
/// Opened for **append**. A trace is a ledger and truncating one on start would
/// lose the run somebody is asking about — and the harness appends across
/// several invocations on purpose.
fn trace_for(to: &str, ts: Option<&str>, session: &str) -> Result<Trace, String> {
    let sink: Box<dyn Write> = if to == "-" {
        Box::new(std::io::stderr())
    } else {
        match std::fs::OpenOptions::new().create(true).append(true).open(to) {
            Ok(f) => Box::new(f),
            Err(e) => return Err(format!("{to}: {e}")),
        }
    };
    // No clock is read here or anywhere below. `--trace-ts` or `-`.
    let ts = ts.map_or_else(Timestamp::unstamped, Timestamp::new);
    Ok(Trace::new(session, CallPath::Dynamic, ts, JsonLines::new(sink)))
}

/// The deployment this run is configured by: the file if `--config` named one,
/// and then whatever the command line said on top.
///
/// **A flag beats the file.** An invocation that worked before a configuration
/// file existed has to keep meaning what it meant, and a flag is the more
/// specific instrument — it names one run, where a file names a deployment.
/// `expose` is the exception (a union, folded into the allowlist below): two
/// explicit grants add up, and neither is a default that could widen by
/// accident.
///
/// `--quota-scope` is validated only when `--quota` gave it something to
/// count, which is how it behaved before: a scope with no budget attached
/// installs nothing whatever it says.
fn deployment_for(
    config: Option<&str>,
    quota_limit: Option<u64>,
    quota_scope: &str,
    audit_capacity: Option<usize>,
) -> Result<Deployment, String> {
    let mut deployment = match config {
        None => Deployment::default(),
        Some(path) => Deployment::from_file(path).map_err(|e| e.to_string())?,
    };
    if let Some(limit) = quota_limit {
        let Some(scope) = Scope::parse(quota_scope) else {
            return Err(format!(
                "--quota-scope {quota_scope:?}: expected one of {}",
                Scope::names().join(", ")
            ));
        };
        deployment.set_quota(limit, scope);
    }
    if let Some(capacity) = audit_capacity {
        deployment.set_audit_capacity(capacity);
    }
    Ok(deployment)
}

/// `--dry-run-handle <name>=<IOR:…|file>`: a reference this dry run will
/// hold, **parsed and not dialed**.
///
/// Split at the first `=`, like `--map-scope`: a name is an identifier the
/// arguments will use and cannot contain one; a path can. The value is a
/// stringified IOR when it reads as one and a file holding one otherwise, so
/// the file an ORB writes its reference to is usable as it lies. Refused at
/// parse time — a reference that will not parse is a usage error, not a
/// prediction about a payload nobody described — and nothing here opens a
/// socket: `Ior::parse` is a decoder over hex.
fn dry_run_handle(spec: &str) -> Result<(String, Ior), String> {
    let Some((name, value)) = spec.split_once('=') else {
        return Err(format!("--dry-run-handle {spec:?}: expected <name>=<IOR:...|file>"));
    };
    if name.is_empty() || value.is_empty() {
        return Err(format!(
            "--dry-run-handle {spec:?}: expected <name>=<IOR:...|file>, both non-empty"
        ));
    }
    let text = if value.get(..4).is_some_and(|p| p.eq_ignore_ascii_case("IOR:")) {
        value.to_owned()
    } else {
        std::fs::read_to_string(value)
            .map_err(|e| format!("--dry-run-handle {name}: {value}: {e}"))?
    };
    let ior = Ior::parse(text.trim()).map_err(|e| format!("--dry-run-handle {name}: {e}"))?;
    Ok((name.to_owned(), ior))
}

/// Rewrites every `{"_ref": "<name>"}` in `args` whose name is a
/// `--dry-run-handle` to the token that handle was issued as, and returns the
/// names it found.
///
/// The library is handed the document an agent would have sent — D008's one
/// notation, a token in the `_ref` seat — and knows nothing of names. A
/// `_ref` that names no declared handle is left as written: it resolves to
/// nothing, and the prediction says so in the mapper's own sentence, which is
/// what a forged handle earns from a live call. Only the `_ref` **string**
/// is rewritten, so a struct member that happens to be named like a handle
/// is untouched.
fn name_references(
    args: &mut Json,
    tokens: &std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeSet<String> {
    let mut used = std::collections::BTreeSet::new();
    let mut stack = vec![args];
    while let Some(node) = stack.pop() {
        match node {
            Json::Object(fields) => {
                if let Some(Json::String(name)) = fields.get_mut("_ref")
                    && let Some(token) = tokens.get(name.as_str())
                {
                    used.insert(std::mem::replace(name, token.clone()));
                }
                stack.extend(fields.values_mut());
            }
            Json::Array(items) => stack.extend(items.iter_mut()),
            _ => {}
        }
    }
    used
}

/// Emits every audit line written since `from` to stderr, and returns the new
/// watermark.
///
/// A watermark rather than a drain: the chain's audit stage owns its lines and
/// nothing can take them away from it — `Bridge::audit` is what §7.4 I4's
/// oracle captures, and a stage a process could empty is a stage a process
/// could be configured into emptying before anybody read it.
///
/// The watermark is `Bridge::audit_written` — a **count of lines ever written**
/// — and not an index into `Bridge::audit`. That distinction is the whole of
/// what a bounded ledger costs an emitter: the moment the ledger drops its
/// oldest line, every index into it means a different line than it did, so an
/// index-based watermark starts re-emitting the tail and then skips lines
/// forever after. The count does not move under anybody.
///
/// This stream is the complete one. The in-memory ledger keeps its newest
/// `--audit-capacity` lines and marks what it dropped; stderr has had every
/// line since the process started, because each is emitted as it is written.
/// The one way a line could reach the ledger's ceiling before being emitted is
/// a single frame writing more lines than the whole ceiling, which is reported
/// here rather than passed over — an audit stream with a silent hole in it is
/// not an audit stream.
fn emit_audit(bridge: &Bridge<'_>, from: u64) -> u64 {
    let lines = bridge.audit();
    let dropped = bridge.audit_dropped();
    if dropped > from {
        eprintln!(
            "audit: {} line(s) were dropped from the in-memory ledger before this process \
             emitted them; raise --audit-capacity",
            dropped - from
        );
    }
    // Absolute position → position in the retained slice. The marker holds the
    // first slot once anything has been dropped and is skipped here: it stands
    // for lines this stream already carries, and re-emitting it on every frame
    // would be noise that says nothing new.
    let skip = (from.max(dropped) - dropped) as usize + usize::from(dropped > 0);
    for line in &lines[skip.min(lines.len())..] {
        eprintln!("{line}");
    }
    bridge.audit_written()
}

fn main() -> std::process::ExitCode {
    let mut idls: Vec<String> = Vec::new();
    let mut ior_path: Option<String> = None;
    let mut expose: Vec<String> = Vec::new();
    let mut session_id = "stdio".to_owned();
    let mut principal: Option<String> = None;
    let mut scopes: Vec<String> = Vec::new();
    let mut token_scopes: Vec<String> = Vec::new();
    let mut scope_map = ScopeMap::nothing();
    let mut mapping_given = false;
    let mut dry_run = false;
    let mut dry_run_only: Option<String> = None;
    let mut dry_run_args: Option<Json> = None;
    let mut dry_run_handles: Vec<(String, Ior)> = Vec::new();
    let mut trace_to: Option<String> = None;
    let mut trace_ts: Option<String> = None;
    let mut quota_limit: Option<u64> = None;
    let mut quota_scope = "caller".to_owned();
    let mut audit_capacity: Option<usize> = None;
    let mut assume_effect: Option<String> = None;
    let mut config_path: Option<String> = None;

    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    let search = match take_include_dirs(&mut argv) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    };
    let mut args = argv.into_iter();
    while let Some(a) = args.next() {
        let mut next = |what: &str| match args.next() {
            Some(v) => Ok(v),
            None => Err(format!("{what} needs a value")),
        };
        let taken = match a.as_str() {
            "--idl" => next("--idl").map(|v| idls.push(v)),
            "--ior" => next("--ior").map(|v| ior_path = Some(v)),
            // Named, never discovered. Read below, once, before anything is
            // built from it. Twice is a usage error rather than a merge: two
            // policy files with no stated precedence is a deployment nobody
            // can read off the command line.
            "--config" => next("--config").and_then(|v| match &config_path {
                Some(first) => {
                    Err(format!("--config {first:?} and {v:?}: name one configuration file"))
                }
                None => {
                    config_path = Some(v);
                    Ok(())
                }
            }),
            "--expose" => next("--expose").map(|v| expose.push(v)),
            // An empty value would be an assumption nobody could read back off
            // a report, which is the one thing this flag exists to prevent.
            "--assume-effect" => next("--assume-effect").and_then(|v| match v.trim() {
                "" => Err("--assume-effect \"\": name an ai_effect value, such as read_only \
                           or destructive"
                    .to_owned()),
                _ => {
                    assume_effect = Some(v);
                    Ok(())
                }
            }),
            "--session" => next("--session").map(|v| session_id = v),
            "--as" => next("--as").map(|v| principal = Some(v)),
            "--scope" => next("--scope").map(|v| scopes.push(v)),
            "--token-scope" => next("--token-scope").map(|v| token_scopes.push(v)),
            // `token=contract`, split at the **first** `=` only: a contract
            // scope is free to contain one and an IdP scope is not, so the
            // asymmetric split is the one that cannot mangle a real name.
            "--map-scope" => next("--map-scope").and_then(|v| match v.split_once('=') {
                Some((token, contract)) if !token.is_empty() && !contract.is_empty() => {
                    scope_map = std::mem::take(&mut scope_map).map(token, contract);
                    mapping_given = true;
                    Ok(())
                }
                _ => Err(format!(
                    "--map-scope {v:?}: expected <token-scope>=<contract-scope>, both non-empty"
                )),
            }),
            "--dry-run" => {
                dry_run = true;
                Ok(())
            }
            // The arguments a call would carry, as the AnyJSON object an agent
            // would send. Parsed here so a malformed document is a usage error
            // and not a prediction about a payload nobody described.
            "--dry-run-args" => next("--dry-run-args").and_then(|v| match Json::parse(&v) {
                Ok(Json::Object(fields)) => {
                    dry_run_args = Some(Json::Object(fields));
                    Ok(())
                }
                Ok(other) => Err(format!(
                    "--dry-run-args: the arguments are a JSON object, got {}",
                    other.kind()
                )),
                Err(e) => Err(format!("--dry-run-args: {e}")),
            }),
            // A reference this run holds without dialing it. Parsed here, so
            // a malformed IOR is refused before the registry is read; issued
            // below, once there is a session to issue it into.
            "--dry-run-handle" => {
                next("--dry-run-handle").and_then(|v| dry_run_handle(&v)).and_then(|(name, ior)| {
                    if dry_run_handles.iter().any(|(n, _)| *n == name) {
                        return Err(format!("--dry-run-handle {name}: declared twice"));
                    }
                    dry_run_handles.push((name, ior));
                    Ok(())
                })
            }
            "--trace" => next("--trace").map(|v| trace_to = Some(v)),
            "--trace-ts" => next("--trace-ts").map(|v| trace_ts = Some(v)),
            "--quota" => next("--quota").and_then(|v| match v.parse::<u64>() {
                Ok(n) => {
                    quota_limit = Some(n);
                    Ok(())
                }
                Err(e) => Err(format!("--quota {v:?}: {e}")),
            }),
            "--quota-scope" => next("--quota-scope").map(|v| quota_scope = v),
            "--audit-capacity" => next("--audit-capacity").and_then(|v| match v.parse::<usize>() {
                Ok(0) => {
                    Err("--audit-capacity 0: a ledger that cannot hold a line cannot be one"
                        .to_owned())
                }
                Ok(n) => {
                    audit_capacity = Some(n);
                    Ok(())
                }
                Err(e) => Err(format!("--audit-capacity {v:?}: {e}")),
            }),
            "-h" | "--help" => {
                eprintln!(
                    "usage: orbweaver-mcp-server --idl <file.idl>... --ior <file> \
                     [-I <dir>]... [--config <policy.json>] \
                     [--expose <id[.operation]>]... [--assume-effect <value>] \
                     [--as <principal>] [--scope <scope>]... \
                     [--map-scope <token>=<contract>]... [--token-scope <scope>]... \
                     [--dry-run[=<id[.operation]>]] [--dry-run-args <json>] \
                     [--dry-run-handle <name>=<IOR:...|file>]... \
                     [--trace <path>|-] [--trace-ts <rfc3339>] \
                     [--quota <calls>] [--quota-scope <everything|caller|interface|operation>] \
                     [--audit-capacity <lines>]"
                );
                return std::process::ExitCode::SUCCESS;
            }
            // `--dry-run=<id>` asks about one interface, exposed or not, which
            // is the "what if I allowlisted this?" question; `<id>.<operation>`
            // asks about one operation, which is the grain values are declared
            // at. Which it is is decided below by `split_operation`.
            other => match other.strip_prefix("--dry-run=") {
                Some(id) => {
                    dry_run = true;
                    dry_run_only = Some(id.to_owned());
                    Ok(())
                }
                None => Err(format!("unexpected argument {other:?}")),
            },
        };
        if let Err(e) = taken {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    }

    // A dry run needs no target: it asks what the policy would do, and asking
    // is the whole of it. Requiring an `--ior` would mean an operator could
    // only preview an exposure once the thing it protects was already running.
    if idls.is_empty() || (ior_path.is_none() && !dry_run) {
        eprintln!(
            "usage: orbweaver-mcp-server --idl <file.idl>... --ior <file> [-I <dir>]...   \
             (--ior is not needed with --dry-run; -I resolves #include, as sidl-validate \
             spells it)"
        );
        return std::process::ExitCode::from(2);
    }
    // Three grains, one grammar (`split_operation`): everything, one
    // interface, or one operation — and only the last can take values,
    // because values are an answer about one call. Values with nothing to
    // apply them to are a usage error, refused before anything is read
    // rather than silently surveyed without them: a report that dropped the
    // payload would read as a report about it.
    let one = dry_run_only.as_deref().map(split_operation);
    if dry_run_args.is_some() && !matches!(one, Some((_, Some(_)))) {
        eprintln!(
            "--dry-run-args needs one operation to apply the values to: \
             --dry-run=<IDL:module/Iface:1.0>.<operation>"
        );
        return std::process::ExitCode::from(2);
    }
    // A held reference is named by the values or by nothing. Refused for the
    // same reason as values without an operation: a run that held a reference
    // nothing could name would print a document that looks like the one that
    // asked about it.
    if !dry_run_handles.is_empty() && dry_run_args.is_none() {
        eprintln!(
            "--dry-run-handle needs --dry-run-args to name the reference: \
             --dry-run-args '{{\"<param>\":{{\"_ref\":\"<name>\"}}}}'"
        );
        return std::process::ExitCode::from(2);
    }

    // The gate, not the parser, over the whole translation unit: a catalog
    // built from IDL that S4 rejects would describe operations nobody can
    // call, and a catalog built from a file read as a string describes fewer
    // operations than the contract does — silently, which is worse.
    let registry = match registry_from_files(&idls, &search, Strictness::Checked) {
        Ok(r) => r,
        Err(e) => {
            for line in e.message.lines().take(5) {
                eprintln!("{line}");
            }
            return std::process::ExitCode::from(2);
        }
    };
    // Deliberately **not** also gated on `Registry::unresolved()`, which
    // `idl-diff` refuses on: that list holds every name the registry's own
    // resolver missed, and the resolver does not search an inherited
    // interface's scope. `corpus/services/gen-naming-subset.idl` raises
    // `NotFound` from `NamingContextExt : NamingContext` — legal IDL that both
    // oracles accept, and four `Unresolved` markers here. A server that
    // refused to start on the naming contract it serves would be a gate
    // nobody could run. What made an estate unservable was the include, and
    // `registry_from_files` refuses that above, naming the file.

    // The whole deployment, decided in one place before anything is built from
    // it: the file if one was named, then the flags on top. A malformed file
    // stops the process here — before the exposure exists, before a socket, and
    // with nothing applied — which is what makes "never partially applied" a
    // property of the order rather than a promise.
    let deployment =
        match deployment_for(config_path.as_deref(), quota_limit, &quota_scope, audit_capacity) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{e}");
                return std::process::ExitCode::from(2);
            }
        };

    let mut exposure = Exposure::nothing();
    for spec in &expose {
        exposure = match split_operation(spec) {
            (id, Some(op)) => exposure.allow_operation(id, op),
            (id, None) => exposure.allow_interface(id),
        };
    }
    // The file's grants, added to the command line's through the same two
    // builders and the same reading of the grammar. Only ever additive, and
    // only ever what somebody wrote.
    exposure = deployment.extend_exposure(exposure);
    // The flag wins; the file answers when the flag was silent.
    let assume_effect = assume_effect.or_else(|| deployment.assume_effect().map(str::to_owned));
    if let Some(effect) = &assume_effect {
        exposure = exposure.assuming_unannotated(Unannotated::Assume(effect.clone()));
    }
    if expose.is_empty() && deployment.expose().is_empty() {
        eprintln!(
            "no --expose given: the catalog holds {} interface(s) and the agent will see none",
            orbweaver_mcp::exposable_interfaces(&registry).len()
        );
    }
    // The size of the silence, said out loud whichever way it is being handled.
    // Without this an operator meets the new refusal one operation at a time and
    // reads it as a permissions problem; with it, the first line of the run says
    // how big the contract-annotation problem is. The estate pilot's RC-5 is
    // exactly the case where nobody could see the number.
    let silent = orbweaver_mcp::unannotated_operations(&registry, &exposure);
    if !silent.is_empty() {
        let sample: Vec<String> = silent
            .iter()
            .take(3)
            .map(|(id, op)| format!("{}.{op}", id.rsplit('/').next().unwrap_or(id)))
            .collect();
        match &assume_effect {
            // The operator's audience, so the flag *is* named — this is the
            // one reader who can run it. The sentence itself comes from
            // `orbweaver_forge::effect`, the same home S4's fix hint and the
            // gate's remedy read, so a rewording reaches all four at once.
            None => eprintln!(
                "{} exposed operation(s) are each an {} and will be REFUSED ({}…): {}",
                silent.len(),
                orbweaver_forge::effect::SILENCE,
                sample.join(", "),
                orbweaver_forge::effect::annotate_or_assume(
                    &orbweaver_forge::effect::OFFER_AUTHOR,
                    Some("--assume-effect <value>"),
                )
            ),
            Some(effect) => eprintln!(
                "--assume-effect {effect:?}: {} exposed operation(s) carry no ai_effect and will \
                 be treated as {effect} ({}…). This is an assumption made here, not a statement \
                 in any contract.",
                silent.len(),
                sample.join(", ")
            ),
        }
    }

    // Token scopes reach the caller only through the map, which is the whole
    // point: this process holds the IdP's vocabulary and the contract's, and
    // the only way from one to the other is the translation an operator wrote.
    // Anything the map does not place grants nothing and is named on stderr —
    // ignored is not silent (`orbweaver_mcp::token::Unmapped`).
    let translated = scope_map.translate(&token_scopes);
    for unplaced in translated.unmapped() {
        eprintln!(
            "--token-scope {unplaced:?} is placed by no --map-scope, so it grants this bridge \
             nothing"
        );
    }
    let caller = principal.map(|p| {
        scopes
            .iter()
            .chain(translated.granted())
            .fold(Caller::new(p), |c, scope| c.with_scope(scope.clone()))
    });

    if dry_run {
        // No socket is opened and no target is read: the report is a question
        // about the policy and the policy is in memory. The only references
        // it holds are the ones `--dry-run-handle` declared, parsed and issued
        // below — never dialed.
        // The approval is the one a session starts with — an operator asking
        // "what needs a human?" wants the answer for a session that has not
        // been handed one.
        // D005's class, asked before the survey and answered from the same
        // exposure the survey runs against: is there a contract scope no token
        // this deployment issues can ever satisfy? Computed here because
        // `Bridge::new` takes the exposure by value.
        let scope_audit =
            mapping_given.then(|| scope_map.audit(&registry, &exposure, &token_scopes));
        let mut bridge = Bridge::new(&registry, exposure, session_id.clone());
        if let Some(caller) = caller {
            bridge.set_caller(caller);
        }
        // The report is about the chain this deployment would run, so the
        // ledger bound, the expiry policy and the quota go in before the
        // questions are asked. A dry run spends none of the budget —
        // `orbweaver_mcp::quota` refunds what a question charges — and it is
        // said out loud here for the same reason it is said when serving: a
        // report that hid the quota it was predicting against would be the
        // report disagreeing with the run.
        match deployment.apply(&mut bridge) {
            Ok(said) => said.iter().for_each(|line| eprintln!("{line}")),
            Err(e) => {
                eprintln!("{e}");
                return std::process::ExitCode::from(2);
            }
        }
        // A dry run is traced too, under its own decision tokens: the questions
        // an operator asked before a deployment are exactly what somebody wants
        // to read back afterwards. Nothing is dialled and nothing is counted.
        if let Some(to) = &trace_to {
            match trace_for(to, trace_ts.as_deref(), &session_id) {
                Ok(trace) => {
                    if !bridge.chain_mut().trace(trace) {
                        eprintln!("no telemetry stage to trace: nothing would be emitted");
                        return std::process::ExitCode::from(2);
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
                    return std::process::ExitCode::from(2);
                }
            }
        }
        // The references this run holds, issued into the session's own table
        // through the path the serving branch issues its root handle through
        // — same repository id, same expiry, same entropy — so the library
        // resolves them the way a live call would. Then the values name them:
        // `{"_ref": "<name>"}` becomes `{"_ref": "<token>"}` before the
        // library sees the document, and a name nothing declared is left to
        // resolve to nothing. The mapping goes on stderr where the root handle
        // goes, and into the document under `handles`.
        let mut tokens = std::collections::BTreeMap::new();
        for (name, ior) in &dry_run_handles {
            match bridge.handles().issue_checked(ior) {
                Ok(h) => {
                    eprintln!("dry-run handle {name}: {h} ({}; parsed, not dialed)", ior.type_id);
                    tokens.insert(name.clone(), h.as_str().to_owned());
                }
                Err(e) => {
                    eprintln!("--dry-run-handle {name}: {e}");
                    return std::process::ExitCode::from(2);
                }
            }
        }
        if let Some(args) = &mut dry_run_args {
            let used = name_references(args, &tokens);
            for name in tokens.keys().filter(|n| !used.contains(*n)) {
                eprintln!(
                    "--dry-run-handle {name} is named by nothing in --dry-run-args: the \
                     prediction does not depend on it"
                );
            }
        }
        // One operation prints that operation's own document; with values,
        // the same document with the payload's verdict in it. The other two
        // grains are surveys, as they were. (`--dry-run-args` without an
        // operation was refused above.)
        let mut report = match (one, &dry_run_args) {
            (Some((id, Some(op))), Some(args)) => {
                bridge.dry_run_with(id, op, args, Approval::default())
            }
            (Some((id, Some(op))), None) => bridge.dry_run(id, op, Approval::default()),
            (Some((id, None)), _) => bridge.dry_run_interface(id, Approval::default()),
            (None, _) => bridge.dry_run_all(Approval::default()),
        };
        // Folded into the one document rather than printed beside it: stdout is
        // one JSON object, and a second one would break every pipeline that
        // parses this. Absent entirely when no mapping was configured, so a
        // deployment that does not use the feature sees the document it always
        // saw.
        if let (Some(audit), Json::Object(fields)) = (&scope_audit, &mut report) {
            fields.insert("scope_map".to_owned(), audit.to_json());
        }
        // What was held, by name: absent when nothing was declared, so the
        // document a deployment without the flag reads is the one it read.
        if let (false, Json::Object(fields)) = (tokens.is_empty(), &mut report) {
            fields.insert(
                "handles".to_owned(),
                Json::Object(
                    tokens.iter().map(|(n, t)| (n.clone(), Json::String(t.clone()))).collect(),
                ),
            );
        }
        println!("{report}");
        // The questions are on the record too, and stderr is where a
        // diagnostic goes; the report on stdout is what gets piped.
        for line in bridge.audit() {
            eprintln!("{line}");
        }
        // A finding is worth an exit code. An operator can read `scope_map.ok`
        // out of the document, and a pipeline that never checks is exactly how
        // D005's drift reached production the one time it was measured — so the
        // process says so in the one channel a script cannot ignore.
        if let Some(audit) = &scope_audit {
            for finding in audit.findings() {
                eprintln!("{finding}");
            }
            if !audit.ok() {
                return std::process::ExitCode::from(3);
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    let Some(ior_path) = ior_path else {
        eprintln!("--ior is required to serve");
        return std::process::ExitCode::from(2);
    };
    let text = match std::fs::read_to_string(&ior_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{ior_path}: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let ior = match Ior::parse(text.trim()) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{ior_path}: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    // How long to wait for a target that may be on the other side of a WAN is
    // a fact about where this is deployed, not about the protocol.
    let dial = deployment.connect_timeout().unwrap_or(DEFAULT_CONNECT_TIMEOUT);
    let conn = match Connection::connect(&ior, dial) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cannot reach the target: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let mut session = Session::new(&registry, exposure, conn, session_id.clone());
    // Everything this deployment said that belongs to a live bridge, installed
    // once and **said out loud**: a ledger that drops lines, a budget that
    // refuses, a capability that expires sooner than it used to are all things
    // an operator chose, and a choice they forgot they wrote is one they will
    // debug as a policy failure. A file makes forgetting easier than a flag did.
    match deployment.apply(session.bridge()) {
        Ok(said) => said.iter().for_each(|line| eprintln!("{line}")),
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    }
    if let Some(limit) = deployment.search_limit() {
        session.set_search_limit(limit);
        eprintln!("search: a request naming no limit gets {limit} result(s)");
    }
    if let Some(trace_to) = &trace_to {
        match trace_for(trace_to, trace_ts.as_deref(), &session_id) {
            Ok(trace) => {
                if !session.bridge().chain_mut().trace(trace) {
                    eprintln!("no telemetry stage to trace: nothing would be emitted");
                    return std::process::ExitCode::from(2);
                }
            }
            Err(e) => {
                eprintln!("{e}");
                return std::process::ExitCode::from(2);
            }
        }
    }
    if let Some(caller) = caller {
        // Without this the audit log says `<nobody>` for every call the
        // process makes, and every `ai_authz` scope refuses. `--as` is a host
        // assertion made on the command line, which is the only channel this
        // process has for one.
        session = session.on_behalf_of(caller);
    }

    // The agent needs somewhere to start. A bridge that resolved names for
    // itself would issue this from a naming service; here the target is given
    // on the command line, so the handle for it is issued up front and printed
    // to stderr — never to stdout, where it would be a stray frame.
    match session.bridge().handles().issue_checked(&ior) {
        Ok(h) => eprintln!("root handle: {h}"),
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    }

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    // How much of the ledger has left this process. §4.8's requirement is met
    // by what an operator can read, not by what the chain remembers.
    let mut audited = 0u64;
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("stdin: {e}");
                break;
            }
        };
        let response = session.handle_line(&line);
        // Before the response is written, so a client that reads a refusal and
        // kills the process cannot beat its own audit line out of the door.
        audited = emit_audit(session.bridge(), audited);
        if let Some(response) = response {
            // One write, one newline, one flush. A client is waiting on the
            // newline, and a buffered response is indistinguishable from a hang.
            if writeln!(stdout, "{response}").is_err() || stdout.flush().is_err() {
                break;
            }
        }
    }
    // The loop can leave by `break`, so the last decision is emitted here or
    // not at all.
    emit_audit(session.bridge(), audited);
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Only the `_ref` seat is rewritten, and only when it names a declared
    /// handle: a member that happens to share a name, an array of references,
    /// and a name nobody declared are all left as written. The library then
    /// sees D008's notation with a token in it — or a name it will refuse.
    #[test]
    fn name_references_rewrites_the_ref_seat_and_nothing_else() {
        let tokens: std::collections::BTreeMap<String, String> =
            [("acct", "cap_0a"), ("spare", "cap_0b")]
                .into_iter()
                .map(|(n, t)| (n.to_owned(), t.to_owned()))
                .collect();
        let mut args = Json::parse(
            r#"{"acct":{"_ref":"acct"},"more":[{"_ref":"acct"},{"_ref":"nobody"}],
                "note":"acct","nested":{"inner":{"_ref":"acct","acct":"acct"}}}"#,
        )
        .expect("json");
        let used = name_references(&mut args, &tokens);
        assert_eq!(used.into_iter().collect::<Vec<_>>(), ["acct"], "spare was named by nothing");
        assert_eq!(
            args.to_string(),
            Json::parse(
                r#"{"acct":{"_ref":"cap_0a"},"more":[{"_ref":"cap_0a"},{"_ref":"nobody"}],
                    "note":"acct","nested":{"inner":{"_ref":"cap_0a","acct":"acct"}}}"#
            )
            .expect("json")
            .to_string()
        );
    }

    /// `<name>=<IOR:…|file>`, both halves non-empty, and a reference that
    /// will not parse is refused with the flag and the name in the sentence.
    #[test]
    fn a_dry_run_handle_is_name_equals_reference() {
        for bad in ["expert", "=IOR:00", "expert=", ""] {
            assert!(dry_run_handle(bad).is_err(), "{bad:?}");
        }
        assert!(
            dry_run_handle("expert=IOR:zz").unwrap_err().starts_with("--dry-run-handle expert")
        );
    }
}
