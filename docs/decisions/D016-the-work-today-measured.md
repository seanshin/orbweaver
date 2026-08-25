# D016 — The work 2026-08-25 measured, arranged so it can be done at once

**STATUS: PROPOSED** — drafted 2026-08-25 from that day's five surveys and
twelve batches. Every item carries the measurement that found it and the file
it lives in; nothing here is a gap column retyped. Not self-approvable: §6
asks for a decision the user owns.

**상태: 제안** — 2026-08-25의 스윕 다섯과 배치 열둘에서 기안. 모든 항목은
그것을 찾아낸 측정과 사는 파일을 함께 적는다. 갭 열을 옮겨 적은 것은 없다.

---

## 1. Why this document is arranged by footprint / 왜 footprint 축인가

D014 sequenced waves by value. That was right for five batches and wrong for
twelve: on 2026-08-25 the work stopped being the constraint and **landing
became it** — serial merges at one harness run each, and a run went 16 → 55
minutes under agent CPU contention. A plan that orders by value produces a
queue; a plan that orders by **footprint** produces a schedule, because two
batches that share no file can be measured at the same time and two that do
cannot, whatever their value.

So the axis here is the file set. Severity is recorded per item and decides
the order *within* a footprint, never across.

D014는 가치 순으로 웨이브를 배열했다. 배치 다섯에는 맞았고 열둘에는 틀렸다 —
그날 작업이 아니라 **착지가 제약이 되었다.** 가치로 정렬한 계획은 대기열을
낳고, **footprint**로 정렬한 계획은 일정을 낳는다. 파일을 공유하지 않는 두
배치는 동시에 잴 수 있고, 공유하는 두 배치는 가치와 무관하게 그럴 수 없다.

## 2. The rule every item below shares / 모든 항목이 공유하는 규칙

Eleven of the fourteen items are the same defect wearing different coats, and
it is the one CLAUDE.md's "Where a fact lives" section already names three
times: **a fact with more than one home, and no compiler behind any of them.**
A refusal sentence, a rule id, a spelling, a keyword list, a width, a
repository id — each has one truth and several places that state it, and the
places drift in the direction nobody is looking.

What makes this list different from the same list a month ago is that every
instance below was found by **running the layers rather than reading them**,
and four were **already losing in the product** when they were found. Scope
each batch to *the fact*, not to the instance it was handed; that instruction
has found the real rule every time it was given, and this document exists
because it was given five times in one day.

*열넷 중 열하나가 같은 결함이다: 집이 둘 이상인 사실, 그리고 그중 어느 것도
뒤에 컴파일러가 없다. 각 배치는 건네받은 사례가 아니라 **사실**로 범위를
잡는다.*

## 3. Parallel now — disjoint footprints, no decision needed / 지금 병행 가능

Each of these is one crate, one batch, and blocks nothing.

| # | Footprint | What, and the measurement | Severity |
|---|---|---|---|
| P1 | `orbweaver-mcp` | **`type_name` is a third home and it reaches the agent.** `lib.rs` ~1174 copies `dynamic::describe` without the arms that matter: a `native` parameter is described to the model as **`<recursive>`**, a `Money` return as `value`, an abstract interface as `abstractinterface` — a spelling that exists nowhere else. `describe_interface` puts it in every parameter, return and attribute row. Two independent batches pointed here on the same day; `tests/ifr_reaches_the_agent.rs` covers no valuetype, native or abstract interface. | **high — a model reads it** |
| P2 | `orbweaver-forge` | **`infer::render_type` calls two supported types unnamed.** `infer.rs` ~308-342's `_ => "<unnamed type>"` swallows `LongDouble` and `TypeCode`, both of which v1 marshals — the second was *deliberately fixed* in the registry to stop being `void`, and this renderer then calls it unnamed. It feeds the operation signature in the **prompt a model and a human read**. Its comment claims the catch-all set is "something v1 does not marshal or does not name"; half of that is false. | **high — a model reads it** |
| P3 | `orbweaver-registry` | **`ingest::harvest` drops what a peer described.** `ingest.rs` ~1101-1134's `_ => {}` justifies only `ObjRef` and silently also drops `Value`, `AbstractInterface`, `Native` and `Recursive` — and does not recurse, so a `struct` reachable only through a valuetype's state is lost from an ingested registry. Contradicts `TypeCode::Value`'s own doc ("an IFR can hold it, the catalogue can draw it"). Whether to *register* a valuetype is a judgement; whether to **walk its members** is not. | medium |
| P4 | `orbweaver-registry` (after P3) | **`diff::diff_type`'s catch-all gives a false reason.** `diff.rs` ~663 says "the encoded form differs, and CDR gives a receiver no way to notice" — but a changed `valuetype`/`fixed`/`native` fails **loudly**; the marshaller refuses them by name. The verdict (Breaking) is right and the sentence an operator reads is wrong. The arm above it exists for exactly this argument and did not extend it. | medium |
| P5 | `spikes/` + `orbweaver-giop` tests | **Seven port-recycle sites report a stolen port as a defect.** `giop/src/lib.rs` `refused_port()`, `ssliop_tls.rs` `dead_port`, `spike_events.rs` and `event_server.rs` `dead_consumer_ior()`, `spike_failover.rs` and `nat.rs` `a_free_low_port()` (a **retyped fact in two crates**), `gen/tests/forward_fallback.rs`. Plus `run_checks.sh`'s three `start_*` helpers, whose `fixture_died()` prints the diagnosis and whose *counter* still calls a dead fixture a refuted claim. The survey's honest conclusion: **eight places, not sixty** — the ~60 in-process bind/connect sites have no mechanism anyone can name. | medium |
| P6 | `orbweaver-gen` tests + `spikes/` | **The Rust↔shell "unmeasured" channel runs on markers, not exit codes.** `servant_faults.rs`, `skeleton_wire.rs`, `object_identity.rs` `eprintln!("UNMEASURED: …")` matched by `run_checks.sh`. CLAUDE.md's own fifth-gate story is a probe that grepped its marker out of a traceback echoing the source line: **probes use exit codes, not markers** — and these markers are source literals in the files whose output is grepped. | medium |

## 4. Atomic across crates — cannot be split, must not be parallel / 크레이트 횡단 원자

Each of these is **one commit spanning two or more crates**. Landing half turns
the other half red, which is why they are listed apart rather than as three
smaller batches. They are the reason "one batch, one crate" is a default and
not a law.

- **A1. The negative corpus's rule splits** (`orbweaver-idl` + `orbweaver-forge`).
  Five diagnoses share a rule id with a diagnosis they do not resemble, and
  two hand a generator a hint that **contradicts the message beside it** —
  n05 is told `Module::TypeCode` while its own message says
  `::CORBA::TypeCode`; n21 is told the floating-point types *are* admitted.
  `forge/tests/corpus.rs` requires a hint for every negative file's first
  finding, so an idl-side split landing alone turns those files red. Split the
  rules and add the hint arms **in one commit**, then the check the batch could
  not land: every rule the front end produces is keyed to a hint or listed as
  deliberately hintless, and no hint is keyed to a rule nothing produces.
- **A2. `TypeCode::idl_name()`, the one home for naming a type**
  (`orbweaver-giop` owns it; `mcp`, `dynamic`, `forge`, `test`, `registry` and
  two spike binaries call it). Seven private namers exist and **every pair
  differs** — measured on `LongDouble`, which nobody disputes: `longdouble` in
  three, `long double` in one. P1 and P2 above are the two that reach a user;
  do them first as local fixes, then collapse all seven here. Also collapses
  four private homes of "the repository id of a TypeCode" into
  `giop::repository_id`.
- **A3. A fifth refusal family for `Principal`** (`orbweaver-dynamic` publishes
  the head; both emitters use it; `deferred_wire_agreement.rs` holds S4's set
  and the emitters' skips equal). Left deliberately open by the Principal
  batch rather than half-done: today S4 does not warn on a declaration
  reaching `Principal` the way it does for a native, and both emitters refuse
  it through generic catch-alls rather than a sentence with a home.
- **A4. The union discriminator, front end first** (`orbweaver-idl`, then
  `giop` + `registry` + `gen`). Nothing restricts `switch_type_spec`, so
  `union U switch (double)` reaches three separate `_ => 4` answers to "how
  wide is a label" — `giop::discriminator_width`, `registry::label_bytes`,
  `dynamic::select_case` — two of which resolve aliases and one of which the
  encoder contradicts for `WChar`. And `gen::disc_of` refuses an aliased
  discriminator while `gen::label_literal` would emit `u32` for one: **the two
  must be repaired in the same commit or an aliased `long` starts emitting
  `5u32` for an `i32` variant.** Fix the front end and the three catch-alls
  become provably unreachable and can say so.

## 5. Needs a peer, or a decision / 피어 또는 결정이 필요한 것

- **B1. `ifr::def_kind` answers `dk_none` for definitions that exist**
  (`registry/src/ifr.rs`, and a byte-for-byte **second home** at
  `gen/tests/ifr_shape.rs` written against the generated enum — repairing one
  leaves the omniORB oracle answering `dk_none` forever). The ordinals
  `dk_Value=20`, `dk_Native=23`, `dk_AbstractInterface=24` are a **wire
  claim**, and every other ordinal in that enum was measured against a peer
  that printed ours back as named enumerators. Do not reason them off the
  specification's declaration order and ship them.
- **B2. One `unmeasured` exit code.** Three conventions collide today:
  `embed.sh` uses 3, `codeset_peer_probe.py` uses 2, and every spike binary
  uses 2 for usage errors — while `orbweaver_mcp_server` uses 3 for a real
  product verdict. The mid-reply batch introduced a fourth reading of 3.
  Pick one, say which, and write it where a script author will meet it. A
  decision record, not a patch.
- **B3. The release cut** (D015 §6). `CHANGELOG.md`'s Unreleased section
  carries wire-visible behaviour changes and today added a dozen more. A
  person who did not build this cannot evaluate any of it from a git log.
- **B4. A named pilot** (D015 §3.2, §3.3, §3.5). The trigger for the durable
  store, for identity through a real provider, and for the Python servant
  direction — one event unblocks all three, and naming it is the user's.

## 6. What this document asks the user / 사용자에게 묻는 것

Two things, neither of which an agent may decide:

1. **D010's status.** It is PROPOSED and its entire class-A programme has been
   executed, with every commit verified; nothing in class C was built, so no
   honesty rule was broken. Approve in retrospect, supersede by D014/D016, or
   keep as a record — but a PROPOSED decision whose proposal is finished
   describes nothing.
2. **A pilot** (B4), or an explicit "not yet", which is also an answer and
   makes three deferrals correct rather than pending.

## 7. What is deliberately not here / 여기 없는 것

The ~60 in-process bind/connect test sites the socket survey classified
HAZARD-latent: the mechanism is hazard-shaped and **nobody can name a way a
refusal reaches them**, so sweeping them would be work justified by a
possibility rather than a measurement. Extract the helper (P5) and adopt it as
those tests are touched.

Also not here: anything from a gap column that was not re-verified on
2026-08-25. D014 §4 borrowed one such row and it was void when written — in a
document whose §1 promised it quotes a gap row only with the verification that
it is still true. That is the whole reason this document cites a measurement
per line.

*의도적으로 빼둔 것: 소켓 스윕이 잠재-위험으로 분류한 in-process 사이트 약
60곳. 위험의 모양은 있으나 **거절이 도달하는 경로를 아무도 이름하지 못한다** —
측정이 아니라 가능성에 근거한 작업이 된다.*
