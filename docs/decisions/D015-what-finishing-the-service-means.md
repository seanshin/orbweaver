# D015 — What finishing the service means, and what cannot be finished here

**STATUS: PROPOSED** — drafted 2026-08-25. Every claim below was verified
against the tree on the drafting day, and the verification is quoted beside the
claim. Not self-approvable: §6 recommends **a release and a named pilot**, and
which pilot is not a thing this document can decide.

**상태: 제안** — 2026-08-25 작성. 아래 모든 주장은 작성 당일 트리에 대해 검증했고,
검증 내용을 주장 옆에 적었다. 스스로 승인하지 않는다: §6의 권고는 **릴리스와
지명된 파일럿**이며, 어느 파일럿인지는 이 문서가 정할 수 없다.

---

## 1. Why a completion document, and why it is not another gap list / 왜 완성 문서인가

`COMPONENTS.md` says what each crate is and is missing. `D010` inventoried what
remained and its class-A programme has since been executed end to end. `D014`
sequences the next waves. **None of them answers the question a service has to
answer: what would make this thing usable by somebody who did not build it.**

That question is different in kind from a gap column. A gap column is written
from the inside — "this crate cannot yet do X". A completion question is
written from the outside: *a person is handed this, with a requirement and a
legacy IDL estate. What stops them?* The four sweeps of 2026-08-25 make this
the right moment to ask it, because they establish that **the inside view is
close to exhausted**: twelve deferrals re-measured, no trigger fired; every
class-A row landed; the one class-B fixture that could be built here was built
that day; and the defects that remain in the inside view are documentation
drift, not capability.

`COMPONENTS.md`는 각 크레이트가 무엇이고 무엇이 없는지 말한다. D010의 A급은
전부 실행되었다. **어느 것도 서비스가 답해야 하는 질문에는 답하지 않는다:
만들지 않은 사람이 이것을 쓸 수 있게 하려면 무엇이 필요한가.** 2026-08-25의
네 스윕이 안쪽 관점이 거의 소진되었음을 보인다 — 유예 열둘 재측정, 발화한
방아쇠 없음; A급 전부 착지; 여기서 지을 수 있던 유일한 B급 픽스처는 그날
지어짐. 남은 결함은 능력이 아니라 문서 어긋남이다.

## 2. The acceptance sentence / 합격 문장

**A person who did not build Orbweaver can take a requirement and a legacy IDL
estate, and end up with an agent making a guarded call against a real ORB —
without editing Rust, without a rebuild, and with an operator able to say who
may call what, how often, and for how long.**

Every clause is load-bearing, and each one is a section below: *without editing
Rust* (§3.1 the operator surface), *without a rebuild* (§3.1 too — a policy
that lives in a binary is a rebuild), *who may call what* (exists), *how often*
(§3.1), *for how long* (§3.1), *against a real ORB* (exists, measured against
two), *a legacy estate* (exists — thirteen contracts, `spikes/estate/`), *and
end up with* (§3.2 — the pipeline runs, but nothing outlives the process).

**오르브위버를 만들지 않은 사람이 요구사항과 레거시 IDL 자산을 들고 와서,
Rust를 고치지 않고, 재빌드 없이, 그리고 운영자가 누가·무엇을·얼마나 자주·얼마
동안 호출할 수 있는지 말할 수 있는 상태로, 에이전트가 실제 ORB에 가드된 호출을
하는 데까지 도달한다.**

## 3. What stands between here and that sentence / 그 문장까지 남은 것

Classed by **what each one needs**, which is the only classification that
changes what we do next. The D010 split (A buildable and measurable here · B
buildable, oracle absent · C deferred with a trigger · D a document claim) is
reused deliberately — it is the vocabulary the harness already speaks.

### 3.1 The operator surface — class A, and the largest single gap

**Verified 2026-08-25.** Three numbers a deployment owns reach no configuration
surface at all:

- **Handle expiry.** `crates/orbweaver-mcp/src/handles.rs` declares
  `DEFAULT_TTL` (15 minutes) and a `with_ttl` builder. Grep for `ttl` in
  `src/bin/orbweaver_mcp_server.rs`, `spike_mcp.rs`, `search_bench.rs`:
  **zero hits in all three.** The builder is reachable only from tests. The
  `orbweaver-capability` row of `COMPONENTS.md` has said "expiry policy
  configuration surface" as its entire gap column for weeks; this is that
  sentence, measured.
- **Quota and rate limit.** `interceptor.rs` documents `SEAT_QUOTA` as having
  an occupant (`quota::Quota`) that `Chain::standard` deliberately does not
  install, with the reason written out: *"the only two numbers a stack could
  default to are both wrong."* That reasoning is right and it is the argument
  **for** a configuration surface, not against one: an operator has the two
  numbers, and today has nowhere to put them.
- **Exposure.** `orbweaver_mcp_server.rs:649` builds `Exposure::nothing()` and
  then populates it in code. Default-deny is correct; **the allowlist being a
  Rust expression is not.** A person who did not build this cannot expose an
  operation without a rebuild.

> **Corrected 2026-08-25 by the batch this section commissioned, which was
> told to verify each claim before building and did.** Two of the three above
> are overstated, and the third understated:
> - *how long* — **true, and worse than written.** `with_ttl` is a *consuming*
>   builder while `Bridge` builds its own table and shares it with every
>   `Guarded` it issues, so the one door that existed could not reach the one
>   table that matters. Not merely unwired: unreachable by construction.
> - *how often* — **half true.** `Chain::standard` still installs no quota, but
>   `--quota`/`--quota-scope` have installed one from the command line since
>   the ledger batch. The operator had somewhere to put the number; what was
>   missing was a *file*, not a surface.
> - *who may call what* — **the flag, not the Rust.** `Exposure::nothing()` is
>   populated from `argv`, not from a Rust expression. The gap is real and one
>   word narrower than written: a **restart**, never a rebuild.
>
> Left standing rather than rewritten, because this is a dated claim and the
> correction is the more useful record: a plan drafted from three greps got two
> of three wrong in the direction that flatters the plan — the gap looked
> bigger than it was — and the only reason that is known is that the brief
> required the builder to re-measure rather than to trust it.
>
> *이 절이 발주한 배치가 세 주장 중 둘을 정정했다. 세 번의 grep으로 쓴 계획이
> 계획에 유리한 방향으로 틀렸고, 그것이 드러난 유일한 이유는 브리프가 짓는
> 쪽에게 믿지 말고 다시 재라고 요구했기 때문이다.*

One batch, one shape: a declarative deployment configuration — file or
environment, read once at startup, defaulting to today's behaviour exactly
(15-minute TTL, no quota installed, expose nothing) so that an existing
deployment sees no change. The rule to scope to is **"a number only an operator
has has one home, and it is not a source file"**; the neighbours to re-measure
are every other such number in the crate, not only these three.

*운영자 표면 — 배포가 소유하는 세 수치(핸들 만료, 쿼터, 노출)가 어떤 설정
표면에도 닿지 않는다. 기본값은 오늘의 동작과 정확히 같아야 한다.*

### 3.2 Nothing outlives the process — class C, trigger not fired

`grep` for `postgres`, `sqlx`, `pgvector`, `rusqlite` across every
`Cargo.toml`: **no hits.** The catalog, the capability table, the audit and the
offer store are all in memory. D003 Part B pre-cleared the store's shape and
deferred it behind a trigger — *a pilot that needs durability* — and
`PLAN-DEFERRED` §4 (PSS) and §2 (OTS) both rest on that same deferral.

**This stays deferred, and §6 is why.** The trigger is a named pilot, and
naming one is the user's decision, not this document's. Building a store before
a pilot names its durability requirement is the class-C defect D010 §9.3
withholds authorisation for — and it would be the most expensive instance of it
in the project, because a store chosen wrong is a migration, not a patch.

*프로세스보다 오래 사는 것이 없다 — 유예 유지. 방아쇠는 지명된 파일럿이고,
지명은 사용자의 결정이다. 잘못 고른 저장소는 패치가 아니라 마이그레이션이다.*

### 3.3 Identity is hand-built — class B, oracle absent here

`token.rs` declares `pub trait Verifier` and states in its own docs that
nothing in the crate implements it; the only `impl` is a test stub. CSIv2 is
wire-tested in both byte orders but **no peer here advertises a mechanism
list** — the harness says so as a counted SKIPPED group naming both missing
things, and goes FAIL the day both are present. That is the correct posture and
it does not change until an identity provider exists to point at.

For the acceptance sentence this matters less than it looks: *who may call
what* is answered by exposure and `ai_authz` today, and identity is how you
know **which** caller — which only becomes load-bearing when there is more than
one, i.e. at the same moment a pilot appears. It is therefore §6's dependency,
not a blocker for §3.1.

### 3.4 Deployment is unmeasured — class B, fixtures absent

No docker here, no second host; both land as counted SKIPPED groups naming
their fixture. `spikes/nat_rewrite.sh` measures R7's endpoint rewriting for the
shapes that can be measured on one machine. This is honest and it is also the
clause of §2 that a person who did not build this will hit first — they will
run it somewhere else. **It cannot be closed here**, and §6 treats that as a
release-note obligation rather than a task.

### 3.5 Python is clients only — class A, deliberately unstarted

`python.rs`'s own header: *"This is a client target."* A Python servant needs
the bridge to call back into Python — a second protocol direction. D010 A6 has
said "not until a consumer names it" since 2026-08-19 and no consumer has. It
stays unstarted for the same reason as §3.2, and the same event unblocks both.

### 3.6 What the four sweeps left behind — class A, small, and already moving

Three of these were commissioned the same day and are in flight or landed:
the operator-facing half of diagnostics (every rejection reaching the fix hint
written for it), the second target's reserved words never having been executed,
and the gate scripts that were reporting green over input they never read.
They are listed here only so this document's inventory is complete; their home
is `D014` and the commits, not a second plan.

## 4. The order, and what runs in parallel / 순서와 병행

1. **The operator surface (§3.1)** — the only item in the acceptance sentence
   that is both blocking and buildable here. One crate (`orbweaver-mcp`), no
   decision needed, defaults preserve today's behaviour.
2. **The release cut (§6)** — after the wave in flight lands, because a
   release whose notes are written from a dirty tree is a release that
   misdescribes itself.
3. **A pilot** — the user's to name. It fires §3.2, §3.3 and §3.5 together,
   which is why none of them is scheduled before it.

Parallel is the default where footprints are disjoint; this document adds no
protocol of its own — `D014` §5 holds it, and it holds by reference rather than
by restatement precisely because the last document to restate a neighbour's
inventory carried a stale row forward within a day of writing "this document
does not restate".

## 5. What this document does not claim / 주장하지 않는 것

It does not claim the ORB core is finished — that question belongs to the
specification and the peers, and `COMPONENTS.md` carries it. It does not claim
the acceptance sentence in §2 is the only reasonable one; it is the one this
project's own plan documents imply, and stating it lets the remaining work be
argued about instead of accumulated. And it does not price anything: no
estimate here has been measured, so none is given.

## 6. The recommendation / 권고

**Build §3.1. Cut a release. Then ask for a pilot.**

The release is the load-bearing half. `CHANGELOG.md`'s Unreleased section
carries wire-visible behaviour changes — a Python runtime that reads
descriptions it used to refuse, a constant that keeps the value that was
written, four refusal families that now name themselves the same way in two
languages. A person who did not build this cannot evaluate any of it from a
git log, and every week those notes go unwritten is a week the measurements
behind them get harder to state honestly.

After that, the honest sentence to hand someone is not "it is finished" but:
*here is what it does, here is what it has been measured against, here are the
three things that need your environment before they can be measured at all.*
That sentence is worth more than a completed checklist, and §3.2–§3.4 are the
reason it is the true one.

**§3.1을 짓고, 릴리스를 자르고, 파일럿을 요청한다.** 넘겨줄 정직한 문장은
"완성되었다"가 아니라 *"이것이 하는 일, 무엇에 대해 측정되었는지, 그리고 당신의
환경이 있어야 비로소 측정할 수 있는 세 가지"* 이다.
