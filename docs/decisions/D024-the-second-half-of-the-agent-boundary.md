# D024 — Administering the ORB, and the second half of the agent boundary

**STATUS: PROPOSED** — drafted 2026-08-25 on two requests: an administration
tool for the ORB's management and registration, and an interface that lets an
AI work with IDL. Both were measured that day before anything here was written.
Not self-approvable: §5 adds tools an agent can call, which is a trust-boundary
change.

**상태: 제안** — 2026-08-25, ORB 관리·등록 도구와 AI가 IDL을 다룰 수 있는
인터페이스, 두 요청에서 작성.

---

## 1. The two gaps, measured / 두 갭

**The MCP boundary exposes exactly three tools** (`rpc.rs::tool_definitions`,
pinned by a test that asserts the list):

```
search_interfaces · describe_interface · invoke_operation
```

An agent can **find a contract, read it, and call it.** It cannot validate IDL
it wrote, diff a proposal against a released contract, generate either half,
register a contract, or ask what a *type* looks like — only an interface. The
whole S1–S5 pipeline exists and reaches the agent through **none** of it: the
forge's stages are command-line binaries, and the boundary that decides what an
agent may do has no opinion about them because they never arrive there.

**The ORB has no administration surface at all.** Measured: no binary in the
workspace is named `*admin*`, `*ctl*` or `*manage*`. The console has three
read-only commands (`catalog`, `diff`, `traces`) and states its charter as
*renders, decides nothing*. The state that would be administered exists and is
already structured — `PoolStats`, `ServerStats`, `ChannelStats`, and as of
today `Orb::list_initial_services` — and nothing reaches it from outside the
process that holds it.

*에이전트는 계약을 **찾고·읽고·부를** 수 있다. **쓰고·검증하고·비교할** 수는
없다. 그리고 ORB에는 관리 표면이 아예 없다.*

## 2. Why these are one document / 왜 한 문서인가

They are the same shape: **read state that already exists, apply a declarative
change, through something that decides.** The project has already built that
shape twice and both are the templates —

- the MCP boundary's interceptor chain (exposure → scopes → quota seat →
  approval, with audit and telemetry), and
- the deployment configuration landed 2026-08-25 (`--config <policy.json>`),
  whose three properties are the ones to copy verbatim: *absent is not zero*,
  *default-deny cannot be widened by an absence*, *refused whole or applied
  whole with the file, the key and the expectation named.*

Doing them separately would produce two administration models for one process.

## 3. What an IDL tool surface must not become / IDL 도구가 되면 안 되는 것

The temptation is a `compile_idl` tool that takes a string and returns a
verdict. That is the stringly-typed surface this project has refused twice
already, in writing:

- `PLAN-SERVICES` §5, on why `ModelFactory` drops GenericFactory's genericity:
  *"an untyped `create(key, criteria)` is exactly the stringly-typed surface S4
  exists to prevent."*
- The S4 diagnostics rule: every refusal names a position and a fix. A tool
  returning `{"ok": false}` throws that away at the boundary where it matters
  most, because the caller is a generator that will quote the answer back.

**So every IDL tool returns what `sidl-validate` returns** — findings with
positions, rules and fix hints — and never a bare verdict. The forge already
has `Report::to_json` and `repair_prompt`; they are the shape, not a new one.

*문자열 하나를 받아 판정을 돌려주는 도구가 되면 안 된다. 그것이 S4가 막으려고
존재하는 표면이고, 답을 되받아 인용할 상대가 생성기라 특히 그렇다.*

## 4. The ORB administration tool / ORB 관리 도구

`orbweaver-orbctl`, a CLI. **Read first, and write only what a file can say.**

**Read** — each already has a source of truth and none needs new state:
- `services` — `Orb::list_initial_services`, with each id's registered IOR and
  whether it is one of §8.5.2's sixteen reserved names.
- `config` — the seven ORB numbers and where each value came from: the
  compiled default, the configuration file, or a flag. **"Where it came from"
  is the half an operator actually needs** and the half a `--config` batch
  usually forgets.
- `pool` / `server` / `channels` — the three existing stats structs, rendered.

**Write** — declaratively, never as a mutation API:
- initial references from a configuration file, using **`-ORBInitRef
  <ObjectId>=<URL>`**, which is the specification's own flag (§8.5.3, with
  `-ORBDefaultInitRef` beside it and a four-step resolution order in §8.5.3.4
  putting `register_initial_reference` first). **The names and the syntax are
  OMG's; do not invent either.**

**What it must not do.** No operation that ends a channel, deactivates a POA or
drops a connection — those are the wire's `destroy` question (`PLAN-DEFERRED`
§11, deferred because the servant has no caller model), and an admin CLI that
does them locally would be the same unauthenticated power through a side door.
`orbctl` administers **configuration and registration**; it does not administer
**lifecycle**.

## 5. The IDL tools an agent gets / 에이전트가 받는 IDL 도구

Four, each wrapping a pipeline stage that already exists, each returning
findings rather than verdicts, and each passing through the same interceptor
chain as `invoke_operation` — this is the part that makes it a boundary change
rather than a convenience:

| Tool | Wraps | Returns |
|---|---|---|
| `validate_contract` | S4 `sidl-validate` | `Report::to_json` — findings with position, rule, fix hint |
| `diff_contract` | `idl-diff` (§5.3) | the verdict table **and** its reasons; breaking / conditionally breaking / compatible |
| `describe_type` | the registry | what `describe_interface` does for an interface, for a **type** — the gap an agent hits first, since a struct member's type is a name it cannot look up |
| `preview_generation` | `gen` | what would be emitted, and **what would be skipped and why** — the skip reasons are the honest half and they already carry the §4.4 sentences |

**Registration is deliberately not here.** An agent that can register a
contract can change what other agents see; that is `exposure`'s decision and it
is off by default by design (`PLAN.md` §7.4 I2). If registration is ever
exposed it needs its own approval stage, and this document does not propose it.

**`describe_type` is the one with a second life.** It is also what the IFR's
containment walk answers over the wire (`Contained::describe`), being built in
another batch today. **The two must agree** — the same registry, the same
answer, one local and one remote. If they can disagree, that is a defect and
the test that catches it belongs with whichever lands second.

## 6. Order, and what is free today / 순서

1. **`orbctl` read commands** — `orbweaver-console` is the only crate free
   right now and it is the natural home for rendering: it already renders and
   decides nothing, which is exactly the read half. Its charter stays intact.
2. **`orbctl` write** (`-ORBInitRef` from a file) — waits on D019 step 3, which
   is building the configuration shape the flag feeds.
3. **The four IDL tools** — `orbweaver-mcp`, which has a branch waiting to
   land. After it.
4. **`describe_type` reconciled with `Contained::describe`** — after both.

## 7. What this document does not claim / 주장하지 않는 것

It does not claim an agent should be able to change the catalogue: §5 says
registration is out and why. It does not propose a wire interface for
administration — `orbctl` is a local CLI over a configuration file, and a
remote admin interface would need the caller model `PLAN-DEFERRED` §11 is
waiting on. And it does not claim the four IDL tools are the right four; they
are the four stages that already exist as binaries, which is the cheapest
honest starting set and the one whose behaviour is already measured.
