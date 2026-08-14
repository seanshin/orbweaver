# Changelog / 변경 이력

Measurements live in [`docs/COMPONENTS.md`](docs/COMPONENTS.md); this file
records what changed and, where it matters, what it changes on the wire.

측정은 `COMPONENTS.md`에, 여기에는 무엇이 바뀌었는지와 — 중요한 경우 — 그것이
와이어에서 무엇을 바꾸는지를 적는다.

---

## v0.2.0 — 2026-08-14

Phase 4 substantially landed; five CORBA services served on our own POA; the
specification pipeline split into stages that can be measured one at a time.

### ⚠ Wire behaviour changed / 와이어 동작 변경

Four defects, in three groups, that change the bytes we put on the wire or
what we accept from a peer. Two of the three groups were found by a reader we
did not write — a foreign ORB, and the specification itself where no peer
could serve as one.

- **`completion_status` was transposed.** `COMPLETED_YES` is ordinal 0 per
  §4.11.4 (confirmed against omniORB before changing anything); our
  `Completion` had `No = 0, Yes = 1`. **A servant reporting "the operation did
  not run" reached every foreign ORB as "it ran"** — so a call refused before
  it started looked like a mutation that had happened, and a client that could
  have safely re-sent concluded it must not. Every servant uses the symbolic
  names, so the fix corrects the naming, event, IFR, expert and tenant services
  at once. `MAYBE` was 2 either way, which is why only two of the three were
  wrong and why nothing local caught it: our own client compared against the
  same enum and agreed with itself, including the test that asserted the
  encoded byte equalled `Completion::No as u32` and therefore moved with the
  bug. It now asserts the literal ordinal.
  **재시도 안전성을 결정하는 두 값이 뒤바뀌어 있었다.** 우리 클라이언트는 같은
  enum으로 비교하므로 스스로와는 늘 일치했고, 외부 ORB만이 이견을 낼 수 있었다.

- **Recursive types could not be marshalled at all.** Every non-empty recursive
  value was refused with "expected a value of type an indirection", and nothing
  noticed because the only generator that could have produced such a value was
  the one reporting the arm as unmeasured. Markers now resolve against the
  enclosing type the error path is already standing on; nesting is bounded at
  64 in both directions, because on decode the depth is the sender's choice.

- **Two fragment-reception defects**, found against hand-built §9.4.9 streams
  since no available peer emits fragments: a stray leading `Fragment` was
  returned as a message, and a fragment at a different GIOP version was
  accepted as a continuation (in 1.1 the bytes read as a request id are body,
  so a match would have been coincidence).

### Added / 추가

- **The end-to-end path, measured as one path** (`spikes/end_to_end.sh`, in the
  harness): a fresh requirement → S1–S5 → both generated halves → a servant on
  our POA → an agent-shaped caller through the guard, with a scope refusal
  visible in the transcript. **185 hand-written product lines against 778
  generated.** Composing it produced the release's most useful finding — see
  *Known limits*.
- **`#pragma prefix` / `version` / `ID`** — repository ids now match omniidl on
  a 25-id corpus, prefixes and all. Before this, every legacy IDL file (the OMG
  recommends a reverse-DNS prefix) would have given us a different identity for
  every type than the peer had, while looking correct locally.
- **`orbweaver-console`** — catalog, contract diff and D004 traces as
  self-contained HTML, no web framework and no template engine.
- **D004 tier 1 telemetry** — one span record per decision, no clock, and a
  credential structurally unable to reach a line.
- **S3i** — annotations inferred for ingested contracts, which never occupy a
  key a gate reads until a human approves them.
- **CORBA services on our POA**: CosNaming server, CosEvent push channel
  (bounded queue, dead consumers disconnected with **drops counted**),
  Trading wire surface for the MoE control plane, a read-only Interface
  Repository facade, and LifeCycle/tenancy with the tenant in every object key.
- **Remote IFR ingestion** — JacORB 3.9's Interface Repository served us. A
  contract can now be taken off the wire with no IDL file, with provenance
  marked and **contagious upwards**.
- **Server skeletons** (`orbweaver-gen`), driven by omniORB's own python
  client, with a servant fault surface whose `#[must_use] Raising` cannot
  become a `SystemException` without naming the completion status.
- **Server-side static-equals-dynamic oracle** — 204 reply-byte comparisons,
  three GIOP versions × two byte orders × two reply origins.
- **Concurrent connections** — cap 64, refusal spoken as §9.4.7's
  `CloseConnection`. Dispatch remains serialized and the documentation says so.
- **The guard's interceptor chain** (F4) and **dry-run**: an exposure can be
  read before it is deployed, audited under its own `DRYRUN-` token, and unable
  to diverge from the live gate by construction.
- **S1–S3 as distinct pipeline stages**, each a producer plus the gate that
  judges it, each runnable alone.
- **Property and contract testing** (`orbweaver-test`), including a wire fuzz
  measuring panic-freedom over the decoders a peer reaches before any policy
  runs: 0 panics in 50,000 cases × 10 targets.
- **Vector search** behind `search_interfaces` via an external command (D003-A)
  — no new dependency. The synonym class remains **UNMEASURED** without a key.
- Decisions **D003** (approved) and **D004** (approved): both adopted zero
  Cargo dependencies.
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md),
  [`docs/PLAN-SERVICES.md`](docs/PLAN-SERVICES.md),
  [`docs/PLAN-DEFERRED.md`](docs/PLAN-DEFERRED.md),
  [`docs/PLAN-MOE.md`](docs/PLAN-MOE.md).

### Fixed / 수정

- The generator bound its encoder closure as `e`, so an IDL parameter named `e`
  shadowed it into `e.put(e)`. Generated locals are now `__`-prefixed and the
  corpus keeps a parameter named `e` on purpose.
- `--expose IDL:spike/Echo:1.0` split at the version's dot, so the
  bare-interface form allowlisted an interface nobody had. Found by dry-run on
  its first run.
- The registry's union `default_index` was computed against the unexpanded case
  list, and **the existing test asserted the buggy semantics**.
- `Poa::new` took an incarnation from a freed `Box` address, which the
  allocator then reused, so two POAs could share one.
- The offer store could lag the residency machine, so under memory pressure the
  loading policy returned an empty decision list — silently.

### Changed / 변경

- The harness takes a **machine-wide lock** and kills fixtures by process
  group. Two concurrent runs used to destroy each other's peers and report
  failures that were about the scheduling; that cost two diagnoses.
- `PLAN.md` / `PLAN.ko.md` at **v0.7**; the streams are scope and the status
  lives in `COMPONENTS.md`, which is refreshed after every wave.

### Known limits / 알려진 한계

Stated because an absence that is not written down reads as a feature:

- Request multiplexing and connection pooling are not implemented: one
  request at a time per connection, and a new connection per reference.
- A generated skeleton has no object keys, so one servant per process; the
  naming server's multi-context shape is not yet generatable.
- Ingested contracts carry no SIDL, so the guard's gates have nothing to key
  on — a second, independent reason exposure stays off.
- The embedding synonym class, the TAO oracle column and the SSLIOP peer proof
  are **unmeasured**, each for a stated reason, and the harness counts them as
  skips rather than passes.
- **The pipeline is not reproducible across runs, and nothing catches it.**
  Re-running S1–S3 on the same requirement with the same prompts passed every
  gate 1/1 again and produced a different contract: different module and
  operation names, a different parameter type, and an authorization scope that
  drifted from the one the requirement literally states. An identity provider
  issuing the stated scope against such a contract refuses every legitimate
  caller. Recorded in `docs/pipeline-runs/2026-08-14-end-to-end.md`; the fix
  needs a decision about what S2 may choose, so it is named rather than
  patched.

---

## v0.1.0 — 2026-08-13

Phases 0–3.5. A from-scratch MIT ORB interoperating with omniORB 4.3.4 and
JacORB 3.9 in both directions at GIOP 1.0/1.1/1.2; IDL 4.2 front end and type
registry in full oracle agreement; POA and object model; dynamic invocation and
AnyJSON; the MCP triad over stdio with default-deny exposure and capability
handles; the S4 validation gate; CSIv2 wire and delegation policy.
