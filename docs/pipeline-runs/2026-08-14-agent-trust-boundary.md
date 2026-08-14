# Silence is not consent — the two estate defects on the agent boundary

> **Measured 2026-08-14** in `crates/orbweaver-mcp/`, against
> `spikes/estate/run.sh` (the thirteen-file legacy estate landed by
> `docs/pipeline-runs/2026-08-14-estate.md`), `spikes/end_to_end.sh`, and
> `cargo test --workspace`. Every number below came out of one of those.
> **2026-08-14 측정.** 아래 수치는 전부 그 세 가지 실행의 출력이다.

The estate pilot found two defects in the MCP bridge, both on the boundary
between an autonomous agent and somebody's production ORB. This batch repairs
both by root cause and codifies both.

**RC-5** — an unannotated contract disabled the approval gate, so a caller
holding no scopes at all was allowed **76 of 76** operations of the estate,
`SystemConsole.SHUTDOWN`, `AuditSink.purge`, `InvoiceService.void_invoice` and
the writable attribute setter `AuditSink._set_enabled` among them.

**RC-8** — `describe_interface` showed an agent **11** operations of an object
the gate judged **13** of and the servant answered **13** of, so the agent
successfully invoked an operation it had never been shown.

에스테이트 파일럿이 찾은 두 결함은 모두 **에이전트와 실제 ORB 사이의 신뢰
경계**에 있다. 이 배치는 두 건을 원인 단위로 고치고 성문화한다.

---

## 1. The batch / 배치

Batch size **9 sites** — every place in `orbweaver-mcp` that either reads an
annotation as a permission or describes a surface by walking declared
operations. The whole set was written in one pass before any oracle ran.

| # | site | cause |
|---|---|---|
| 1 | `policy::destructive_effect` → `stated_effect` | C |
| 2 | `policy::Exposure::check_call` | C |
| 3 | `interceptor::ApprovalInterceptor` | C |
| 4 | `interceptor::Chain::standard` | C |
| 5 | `policy::required_scopes` — **examined, deliberately unchanged** | C |
| 6 | `lib::describe_interface` (operations **and** attributes) | E |
| 7 | `lib::search_interfaces` (haystack **and** the advertised count) | E |
| 8 | `bin/search_bench::interface_text` (the embedding text) | E |
| 9 | `policy::attribute_annotations` (a third ancestor walk) | E |

**First-pass rate: 0/12.** The first oracle run failed twelve tests. That number
is about the *fixtures*, not about the change: all twelve had one cause, below.

**Round count: 3.** Round 1 clustered the twelve; round 2 found two fixtures the
first repair did not reach; round 3 was green in this crate and found one more
in `orbweaver-console` and one in `orbweaver-forge`.

배치 9곳, **1차 통과율 0/12**, **라운드 3회**. 두 수치는 서로 다른 것을 측정하며
따로 보고한다 — 0/12는 픽스처에 대한 측정이고, 12건 전부 같은 원인이다.

---

## 2. Root causes / 근본원인

### RC-a — an annotation-keyed gate read *the contract has nothing to say* as *the contract says yes* · 1 mechanism, 6 call sites, 15 fixtures

`annotations.get("ai_effect")?` returned `None` for a missing key and the `?`
turned that into "no approval required". A **misspelled** effect needed a human
and a **missing** one did not: the safe-direction rule was applied to a typo and
not to an absence.

Three answers now exist where there were two:

```rust
pub enum Effect {
    Harmless,        // read_only / readonly / idempotent / safe — and any `_get_`
    Stated(String),  // anything else somebody wrote, including a typo
    Unstated,        // no ai_effect reaches this operation
}
```

**What "closed" means, decided rather than defaulted.** An `Unstated` operation
is **refused** — not approval-required, and not hidden:

- Not `NeedApproval`, because an approval is a human saying yes to a *specific*
  call and nobody can say yes to a call whose effect nobody has stated. It would
  also have made a legacy estate seventy-six approvals, which is the shape of
  gate people learn to click through — and one `--approve` would then have
  unlocked all seventy-six at once. That last property is pinned by
  `an_approval_in_hand_does_not_unlock_a_silence`.
- Not `NotExposed`, because the operator *did* expose it. Answering "not
  exposed" would send them into the allowlist for a problem that is in the
  contract — the misdirection the estate recorded arriving by another road
  (RC-4).

So the refusal is its own variant, `Denied::EffectUnstated`, and its own
dry-run row, `need_effect`, whose fix lives in a third place: the contract.

**What makes failing closed usable.** Refusing per operation is correct and, on
its own, produces one refusal per silence. `Exposure::assuming_unannotated`
(`--assume-effect <value>` on the server) is the operator's **single**
declaration for an exposure — *for the operations that state nothing, assume
this*. It runs through the same recognition a contract's own value gets, so
`--assume-effect read_only` allows them and `--assume-effect destructive` sends
them to the approval queue, and it never touches an operation whose contract
does state an effect.

**Every place an allow rests on that assumption says so**: `assumed: true` on
the refusal, `effect_stated_by: "exposure"` on the dry-run row,
`unannotated_effect` at the top of every survey, and a startup line naming how
many operations of the exposure carry no `ai_effect` — because the size of the
silence is the fact the decision is about.

**A getter is `Harmless`, and by the grammar rather than by an annotation.**
`_get_x` reads `x`; that is a statement IDL makes in the language it is written
in, not a silence. Refusing every attribute read of every legacy contract for
want of an annotation the language already implies would be a gate nobody could
satisfy without rewriting the contract. What a getter may *leak* is the scope
gate's question, and `required_scopes` already guards both accessors from the
attribute's own `ai_authz`.

애너테이션에 키를 건 게이트가 **부재**를 **허용**으로 읽었다. 오타난 효과는 승인을
요구하고 없는 효과는 요구하지 않았다. 이제 답은 셋이고, 침묵은 **거부**다 —
승인 대기도 아니고 미노출도 아니다. 거부를 쓸 수 있게 만드는 것은 운영자가 노출
단위로 **한 번** 선언하는 `--assume-effect`이며, 그 가정에 기대는 모든 허용은
문서와 거부 메시지에 그 사실을 적는다.

### RC-b — a surface was *described* by walking declared operations while it was *reached* by resolving inheritance · 4 walks, 1 of them correct

RC-8 was not a missing walk in one function. Four functions described an
interface's surface and exactly one of them — `dryrun::operations_of` — had been
taught to resolve inheritance. So the gate judged thirteen operations of an
object the agent was shown eleven of, and the two missing ones were inherited
from a base that ten of the estate's twelve interfaces share.

One fix: `orbweaver_mcp::resolved_operations` and `resolved_attributes`, public
so that nothing inside or outside the crate has to write a fifth walk. Every
row carries `declared_in`, so *where* an operation comes from stays visible —
inheritance is information, and flattening it away would trade one missing fact
for another.

**A flat fixture cannot see this class at all**, which is why it went unfound
until a set of contracts existed to run against. The pin is built on a
three-level hierarchy (`Dispatcher : Routable : Describable`) and the
load-bearing assertion is not about `describe_interface` — it is
`the_described_surface_is_the_surveyed_surface`, an equality between what an
agent is shown and what the gate judges. A test naming only `describe_interface`
would let the next copy of the walk drift the same way.

표면을 **선언 기준**으로 기술하면서 **해석 기준**으로 도달하게 두었다. 걷기가
네 곳에 있었고 그중 하나만 상속을 해석했다. 수정은 하나의 공개 걷기이며, 핵심
성문화는 `describe_interface` 테스트가 아니라 **에이전트가 보는 집합과 게이트가
판단하는 집합의 동일성**이다.

### RC-c — fifteen test fixtures encoded "absence is permission" · all 12 first-pass failures

Every one of the twelve failures was a fixture contract with an unannotated
operation the test expected allowed. Not twelve defects: one belief, written
down fifteen times. The repair is the same repair — the fixtures are contracts,
and a contract that says nothing is now refused — applied across every affected
file in one pass.

Two of them were findings in their own right rather than churn:

- `quota.rs` named a second interface, `IDL:bank/Ledger:1.0`, that **was not in
  the fixture's catalog at all**. The budget cases still passed, because a
  target nothing knew about reached the quota stage regardless. The effect gate
  stops one now, which is how the gap surfaced.
- `dryrun.rs` and `interceptor.rs` keep one operation deliberately unannotated,
  because the crate now has a fourth verdict and a fixture with one of each is
  a better fixture. `a_survey_answers_for_every_operation_at_once` reads
  `allow / need_approval / need_scope / need_effect` — one of each.

픽스처 15곳이 "부재=허용"을 적어 두고 있었다. 12건의 실패는 결함 12개가 아니라
믿음 하나가 열다섯 번 적힌 것이다.

---

## 3. The neighbouring gates, examined / 인접 게이트 점검

The brief asked for the same cause to be looked for in the neighbours before
one was fixed. It was, and one of them is a **deliberate non-change**.

| gate | same cause? | verdict |
|---|---|---|
| `ai_effect` absence | yes | fixed — RC-a |
| `ai_authz` absence | **no** | deliberately unchanged, with the reason in the code and a test |
| `attribute_annotations` / `declares_accessor` | E, not C | rebuilt on the one walk |
| `_set_` on a `readonly` attribute | no | unchanged: it reaches no servant, so there is no call for a contract to have described |
| the quota seat | no | already the right pattern — `Chain::quota` returns `false` and installs nothing rather than defaulting a number only an operator has. This is the precedent RC-a's default follows. |

**Why an absent `ai_authz` is not the same cause**, stated because it looks like
it:

1. **There is nothing to fail closed *to*.** A scope refusal is actionable
   because it names the scope to grant. An absent `ai_authz` names none, so the
   only "closed" available is *refuse everything*, whose fix hint would be "add
   an `ai_authz`" — wrong advice for an operation whose author decided it needs
   none.
2. **The silence is no longer reachable un-vetted.** An operation nobody
   annotated at all is stopped by the effect gate before this question matters.
   What survives to the scope gate is an operation whose author *was* writing
   annotations and chose not to require a scope, which is a decision rather than
   an absence.

**What remains, and is reported rather than fixed:** `//@ ai_effect: read_only`
with no `ai_authz` is readable by anyone the exposure lets in, and on a
balance-reading operation that is a real hole. It is a **contract-quality**
problem, so the instrument is S4's advice and `contract-check`, not a gate that
can only refuse. `an_absent_ai_authz_still_requires_no_scope_and_that_is_deliberate`
puts the reasoning on the record so it is a decision rather than an omission
somebody re-derives as a bug.

`ai_authz`의 부재는 같은 원인이 **아니다** — 닫을 대상이 없고, 미주석 연산은 이미
효과 게이트에서 멈춘다. 남는 구멍(`read_only` + authz 없음)은 게이트가 아니라
계약 품질의 문제이므로 S4 조언이 담당한다.

---

## 4. Measured, before and after / 측정 (전·후)

`./spikes/estate/run.sh --tsv`, the rows the brief asked for quoted verbatim:

| row | before | after |
|---|---:|---:|
| `s7-dryrun allowed` | **76** | **12** |
| `s7-dryrun operations` | 76 | 76 |
| `s7-dryrun bytes` | **7253** | **31957** |
| `s9-agent describe-lists-inherited` | **no** | **yes** |
| `s9-agent rc` | 0 | **1** — see §6 |

Every other row is unchanged: `s1-per-file 2/13`, `s2-oracle 13/13`,
`s3-splice 660 lines / 104 advice`, `s4-identity 49 oracle-ids, spliced-agrees
yes, naive-drift 5`, `s5-register 12 exposable`, `s6-generate 0 skipped / 8384
lines / 12/12 both halves`, `s7-dryrun audit-lines 76`, `s8-serve published yes`.

**The 12 that are still allowed are the attribute getters** — `_get_label`
inherited from `Describable` by ten of the twelve interfaces, plus the audit
sink's. Every one of the 64 operations the estate declares and never described
is now `need_effect`. `AuditSink._set_enabled`, `AuditSink.purge`,
`SystemConsole.SHUTDOWN`, `InvoiceService.void_invoice`, `ShipmentTracker.cancel`
and `EdiGateway.flush_queue` are all among them.

**`spikes/end_to_end.sh`: PASS, and the number the brief said must not move did
not move.** Hop 4 reads *"predicted: two operations allowed, open_entry_gate
needs gate:operate"* — `allow=2 need_scope=1 need_approval=0 not_exposed=0
refuse=0`, byte for byte what it read before. The annotated contract's three
operations all carry an `ai_effect`, so nothing about the annotated path
changed. That is the point of the design: this batch is entirely about what
happens when a contract says **nothing**.

`spikes/end_to_end.sh`는 통과하며, **주석된 경로의 수치는 움직이지 않았다** —
`allow=2 need_scope=1`. 이 배치는 계약이 **아무 말도 하지 않을 때**만을 바꾼다.

### Gates

- `cargo test --workspace --no-fail-fast`: **1154 passed, 1 failed.** The one
  failure is `orbweaver-forge`'s `an_inferred_scope_enforces_nothing_at_the_policy_gate`,
  it is caused by this change, and it is **not fixed here** — see §6.
- `cargo test -p orbweaver-mcp`: 207 + 5 + 6 = **218 passed, 0 failed**
  (11 new).
- `RUSTFLAGS="-D warnings" cargo test -p orbweaver-mcp`: green.
- `cargo fmt --check`: clean. `cargo clippy --workspace --all-targets`: 0
  warnings.
- `unsafe_code = "forbid"` and `#![deny(missing_docs)]` untouched.
- The console catalog renders: `cargo run -p orbweaver-console --bin
  orbweaver-console -- catalog spikes/e2e/recorded/PARKING.sidl.idl --text`.

---

## 5. The unusable gate, re-measured / 규모에서의 사용성 재측정

The estate report's §4 found a gate that is *correct and unusable*: 7,253 bytes
of dry-run report, 76 entries, every one `allow`. Correct, and zero signal,
because the answer did not vary.

**What it becomes: 31,957 bytes, 12 `allow` and 64 `need_effect`.** Both halves
of that are worth stating plainly.

- **The signal is there now.** The summary line an operator actually reads went
  from one uniform number to a two-way split that names the problem, and the
  problem it names is a fact about the estate rather than about the bridge:
  *sixty-four of these operations have never been described by anyone.*
- **The document is four times longer and still not readable line by line.**
  The growth is entirely the `why` string: a refused row carries a reason and an
  allowed row does not, so 64 rows gained ~250 bytes of identical prose. The
  length is now proportional to the size of the problem, which is the right
  relationship — but 64 verbatim copies of one sentence is redundancy, not
  information.

**The finding this leaves, named and not built:** a survey should carry one
reason **per class** and not one per row. `unannotated_effect` is already at the
document level; the per-row `why` for a `need_effect` adds nothing the row's
`would` does not already say. Making that change would put the report at roughly
8 KB with strictly more signal than the 7,253-byte version it replaces. It is a
report-shape change rather than a gate change, it belongs with whoever owns the
survey document's format, and it is recorded here rather than done because this
batch is about the gate.

보고서는 7,253바이트 전부 `allow`에서 **31,957바이트, `allow` 12 / `need_effect`
64**가 되었다. 신호는 생겼고(요약 줄이 문제를 이름 지어 말한다), 길이는 4배가
되었으며 여전히 한 줄씩 읽을 수 있는 문서는 아니다. 증가분은 전부 동일한 `why`
문장 64벌이다. **행 단위가 아니라 부류 단위로 이유를 싣는** 것이 남은 개선이며,
이는 게이트가 아니라 보고서 형식의 변경이므로 기록만 하고 만들지 않았다.

---

## 6. What this change breaks elsewhere, and was deliberately not fixed / 이 변경이 다른 곳에서 깨뜨리는 것

Three, all outside this batch's footprint, all with the same one-line shape, all
reported rather than applied.

### 6.1 `crates/orbweaver-forge/tests/infer.rs` — the workspace's one red test

`an_inferred_scope_enforces_nothing_at_the_policy_gate` asserts `verdict.is_ok()`
for `delete_all` on an **ingested** contract. Ingested entries carry no SIDL, so
the operation states no `ai_effect` and the gate now answers
`Err(EffectUnstated)`.

**The test's thesis still holds and is now provable more strongly.** The point
is that an *inferred* annotation gates nothing: `inferred_authz` is not
`ai_authz`. The refusal is `EffectUnstated`, which is not `MissingScope` and not
`NeedsApproval` — no inference gated anything. The assertion should say that:

```rust
    // An ingested contract carries no SIDL, so the effect gate stops it —
    // which is *not* the inference gating anything. That is the property:
    // the refusal must never be MissingScope or NeedsApproval, because both
    // would mean a model-invented permission name had become enforceable.
    assert!(
        matches!(verdict, Err(Denied::EffectUnstated { .. })),
        "an inferred annotation must not gate anything; if this is MissingScope \
         or NeedsApproval the marks have leaked into ai_* keys: {verdict:?}"
    );
```

(with `orbweaver_mcp::policy::Denied` imported). **Not applied**: `orbweaver-forge`
is another agent's footprint this wave.

### 6.2 `spikes/echo.idl` — `gen-corpus`'s I1 check

`blob_sum` carries `//@ ai_authz: echo:blob` and no `ai_effect`, so
`gen-corpus`'s I1 check *"blob_sum() allowed: the caller holds the echo:blob
scope the contract asks for"* now fails. Measured directly:

```
$ orbweaver-mcp-server --idl spikes/echo.idl --expose IDL:spike/Echo:1.0.blob_sum \
    --as alice --scope echo:blob --dry-run
9 exposed operation(s) carry no ai_effect and will be REFUSED (Echo:1.0.blob,
  Echo:1.0.blob_sum, Echo:1.0.echo_any…)
… "operation":"blob_sum" … "would":"need_effect"
```

The fix is one line — `//@ ai_effect: read_only` above `blob_sum`, which is what
summing octets is. **Not applied**: `spikes/` is outside this batch's footprint.

### 6.3 `spikes/estate/agent.py` — `s9-agent rc` 0 → 1

Four of the driver's agent-session cases assert the old behaviour: `lookup`,
`describe`, `backlog` and `cancel` are expected to reach the servant on an
estate that annotates none of them. They are refused at `safety.approval` now,
which is the change working.

Session A's design intent — *a destructive legacy operation stopped by the
contract, not the guard* — is still expressible and is worth keeping, because it
is a different and real finding. The estate is exactly the input
`--assume-effect` exists for. Adding `--assume-effect read_only` to session A's
server arguments restores that session's meaning (an operator who has decided
what this estate's silences mean) while session B, which measures the guard
refusing, is unaffected. **Not applied**: `spikes/` is outside this footprint.
`s9-agent describe-lists-inherited` already flipped to `yes` under the driver's
existing row, which is RC-8 closing without any driver change at all.

세 곳 모두 이 배치의 발자국 밖이라 **적용하지 않고 보고**한다. 각각 한 줄짜리
수정이며, 위에 정확한 형태를 적어 두었다.

---

## 7. Is `orbweaver-forge`'s advice string now true? / 포지의 조언 문구는 이제 참인가

**Yes — for the first time.** S4 advises, fifty-two times over this estate, that
an operation *"has no `ai_effect`, so the bridge must assume it needs
approval"*. The estate measured that as false: the bridge allowed.

It is now true in substance and **off by one word in form**. The bridge does not
assume the operation needs *approval*; it refuses and says the contract must
state an effect, because an approval is a yes to a described call. The
distinction is the whole of §2's design and the advice should carry it. A
faithful rewording, for whoever owns `orbweaver-forge`:

> has no `ai_effect`, so the bridge refuses it: it cannot tell whether an agent
> may call this without a human. Annotate it, or set the exposure's
> `--assume-effect`.

`orbweaver-forge` is outside this footprint, so this is a recommendation and not
a change.

**이제 참이다** — 다만 한 단어가 다르다. 브릿지는 *승인을 요구*하지 않고 **거부**
하며 계약이 효과를 말하라고 답한다. 승인은 기술된 호출에 대한 동의이기 때문이다.

---

## 8. What was codified / 성문화한 것

A cause that is only fixed comes back. Eleven new tests, each named for the
property rather than for the bug.

**RC-a, the gate:**

- `an_operation_the_contract_says_nothing_about_is_refused` — the estate defect
  in one assertion, with a doc comment saying what going the other way would
  mean: *the bridge has gone back to telling an autonomous agent that an
  operation nobody has described is safe to call against somebody's production
  ORB.*
- `the_refusal_names_the_annotation_that_is_missing` — asserts the message
  contains `ai_effect` and **does not** read as an exposure problem.
- `an_absent_effect_and_an_unrecognised_one_are_different_answers` — the two now
  differ deliberately, and in the direction that costs something.
- `an_approval_in_hand_does_not_unlock_a_silence` — one `--approve` must not
  unlock every operation nobody has described.
- `an_assumption_covers_the_silences_and_only_the_silences`
- `an_assumed_destructive_needs_an_approval_and_says_whose_word_it_is`
- `the_default_posture_on_a_silence_is_refusal`
- `an_unannotated_setter_is_refused_and_its_getter_is_not`
- `an_absent_ai_authz_still_requires_no_scope_and_that_is_deliberate` — the
  non-change, on the record.

**RC-a, the process** (`tests/serving_audit.rs`, spawning the real binary,
because RC-5 was a property of a real process's real output):

- `an_unannotated_operation_is_refused_by_the_process_and_the_silence_is_counted`
- `one_assumption_covers_every_silence_and_the_process_says_it_is_an_assumption`
- `the_dry_run_document_names_the_posture_and_marks_what_rests_on_it`

**RC-b:**

- `describe_lists_inherited_operations_and_says_where_each_comes_from` — built
  on a three-level hierarchy, not a flat interface.
- **`the_described_surface_is_the_surveyed_surface`** — the load-bearing one.
  Equality between what the agent is shown and what the gate judges, so the next
  copy of the walk cannot drift.
- `search_finds_an_interface_by_an_operation_it_inherits`
- `a_derived_declaration_shadows_the_base_it_overrides`
- `a_refusal_does_not_reveal_whether_the_operation_exists` — rewritten from
  `an_unknown_operation_is_reported_undeclared_rather_than_refused`. A declared
  silence and an undeclared name now get byte-identical verdicts, and existence
  is still reported as its own field.

**Structural, so the causes cannot recur by construction:**

- `resolved_operations` / `resolved_attributes` are **public**, so nothing has
  an excuse to write a fifth walk. `dryrun::operations_of` and
  `policy::attribute_annotations` were rebuilt on them; three walks became one.
- `effect_refusal` is the one composition of the effect rule, called by both
  `Exposure::check_call` and `ApprovalInterceptor` — the same discipline
  `required_scopes` already had.
- `Chain::standard` takes the safety stage's posture from the same `Exposure`
  the allowlist comes from, before it moves, so the two cannot be configured
  apart. `ApprovalInterceptor::for_exposure` is the way to build one and its
  `Default` is `Refuse`.
- `Chain::unannotated()` reads the posture **off the stage that will act on
  it**, so a report cannot describe a posture the gate is not taking.

성문화 11건. 핵심은 결함 이름이 아니라 **성질** 이름을 단 두 개다 —
"침묵은 허용이 아니다"와 "에이전트가 보는 표면 = 게이트가 판단하는 표면".

---

## 9. What was not measured / 측정하지 않은 것

Named, because a stage nobody mentions is a gap and a stage named unmeasured is
a result.

- **`spikes/run_checks.sh` was not run as a whole.** The batch was told not to
  edit it and it takes a machine-wide lock while four other agents are building;
  the disk filled twice during this batch (measured: 258 MiB free on a 460 GiB
  volume, with 27 GiB of sibling worktree `target/` directories). The two gates
  it runs that this change could plausibly move — the frozen search baselines —
  were run individually and are the next item.
- **The frozen search benchmarks were re-run and did not move.** This batch
  widens the lexical haystack **and** the stand-in embedding text to the
  resolved surface, which *can* move a search result: an interface now also
  matches on the names and `ai_desc` prose of what it inherits. A haystack only
  grows, so nothing findable yesterday is unfindable today — but the *negative*
  and *injection* classes are exactly the ones a wider haystack breaks, so this
  was measured rather than reasoned about:

  ```
  search-v1: PASS baseline exact=18/18 synonym=0/10 negative=6/6 injection=5/5
  search-v2: PASS baseline exact=28/28 synonym=0/10 negative=6/6 injection=5/5
  ```

  Both identical to the frozen baselines. **The corpus has no inheritance**, so
  the widened haystack had nothing extra to index there — which means this is a
  *negative control that could not have fired*, and it is reported as such
  rather than as evidence that the widening is safe on a corpus that does
  inherit. The estate is the input that exercises it, and stage 9 measured
  search over an inheriting interface finding it (session A,
  `search_interfaces('shipment tracking')`).
- **The vector arm of search is unmeasured**, for the reason `run_checks.sh`
  already documents: embeddings arrive through a process boundary or not at all,
  and with no `VOYAGE_API_KEY` the half is never green and never faked with the
  offline stand-in.
- **No foreign ORB was driven.** Nothing in this batch touches the wire; the
  estate's stage 8 servant and stage 9 agent are ours.
- **The dry-run report's readability at 32 KB is an opinion, not a
  measurement.** Nobody read it end to end. What was measured is its size and
  its distribution of verdicts.
- **Both estate measurements used the driver as it stood at `dd22c3f`.** The
  estate was copied into this worktree from `main` to run it and has been
  removed again; nothing under `spikes/` is in this batch's diff. `main` has
  since moved — a sibling batch repaired the estate's RC-6/RC-7 and rewrote
  `run.sh`'s stage 3/5/6 filenames. The before/after above were taken with the
  **same** driver on both sides, which is what makes them a controlled
  comparison; re-running on the newer driver will produce the same `s7`/`s9`
  rows for different stage-3 plumbing, but that has not been measured here.

`run_checks.sh` 전체는 미실행(락·디스크). 이 변경이 움직일 수 있는 **동결된 검색
벤치마크는 개별 실행했고 수치는 그대로다**(v1 18/18·6/6·5/5, v2 28/28·6/6·5/5).
다만 코퍼스에 상속이 없으므로 이는 **발화할 수 없었던 음성 대조**이며, 그렇게
보고한다. 벡터 경로는 키가 없어 미측정이다 — `run_checks.sh`의 원칙대로 절대
초록으로 만들지 않는다.

---

## 10. Reproducing / 재현

```bash
cargo test -p orbweaver-mcp                 # 218, including the 11 new pins
cargo test --workspace --no-fail-fast       # 1154 pass, 1 fail (forge, §6.1)
./spikes/estate/run.sh --tsv                # s7-dryrun allowed: 12 (was 76)
./spikes/end_to_end.sh                      # allow=2 need_scope=1, unmoved

# the two postures, side by side, on any unannotated contract
orbweaver-mcp-server --idl <legacy>.idl --expose <id> --as ops --dry-run
orbweaver-mcp-server --idl <legacy>.idl --expose <id> --as ops --dry-run \
                     --assume-effect read_only
```

```bash
# the frozen baselines this change could have moved, and did not
cargo run -q -p orbweaver-mcp --bin search-bench -- \
    corpus/queries/search-v1.tsv corpus/golden/*.idl spikes/echo.idl
```

**Still to run before landing:** `./spikes/run_checks.sh`, once the machine has
disk and the lock — and after §6.2's one-line `spikes/echo.idl` annotation,
without which its `gen-corpus` I1 check fails.
