# D007 — How a generated Python client reaches the wire

**STATUS: PROPOSED** — drafted 2026-08-14 alongside the Python client target
(`docs/pipeline-runs/2026-08-14-python-target.md`). **Not adopted.** Option A
is what the target was built on because it needs no new dependency and no
approval; this document exists because options B and C do, and because the
choice will be re-opened the moment somebody measures the latency.

**상태: 제안됨** — 2026-08-14 작성, 파이썬 클라이언트 타깃과 함께. **채택 아님.**
A안은 새 의존성도 승인도 필요 없어 그대로 구현했고, 이 문서가 존재하는 이유는
B안과 C안이 둘 다 승인을 필요로 하기 때문이다.

---

## The question / 문제

`orbweaver-gen` now emits Python clients. Python cannot speak GIOP by itself,
and this project's GIOP is Rust. Something has to join them, and every way of
joining them costs something different:

- a **process boundary** costs latency and a second thing to deploy;
- an **FFI boundary** costs a new dependency class and a build system;
- a **re-implementation** costs a second set of alignment bugs.

The licensing boundary removes the option that would otherwise be obvious:
omniORBpy exists, is LGPL, and may only ever be a fixture or an oracle. It is
not a candidate and is not compared below.

파이썬은 GIOP를 못 하고, 이 프로젝트의 GIOP는 러스트다. 둘을 잇는 방법마다 비용이
다르다: 프로세스 경계는 지연과 배포 대상 하나, FFI 경계는 새로운 의존성 종류와
빌드 시스템, 재구현은 정렬 버그 한 벌. omniORBpy는 후보가 아니다 — 라이선스
경계상 픽스처이거나 오라클일 뿐이다.

---

## The options / 선택지

### A. A local bridge process, speaking AnyJSON v1 — *implemented, recommended for v1*

`orbweaver-py-bridge` is started with the contract and the target's IOR, holds
one connection, and exchanges one JSON document per line over its stdin and
stdout. The documents are **AnyJSON v1** (`PLAN.md` §4.5), which this project
already specifies and round-trip tests, so the seam introduced no new format
and no second dialect of one.

| | |
|---|---|
| New dependencies | **none.** CPython's standard library on one side, existing crates on the other |
| Wire code | exists once, in Rust; the bridge calls `orbweaver_dynamic::invoke` |
| Build | `cargo build`; the client needs the binary on `PATH` or in `ORBWEAVER_PY_BRIDGE` |
| Cost | one process per client, one JSON encode/decode per call, and a protocol that is itself a wire and needs its own version discipline |

The honest weakness is not latency, it is **surface**: the bridge is a second
place where a request is described, and a second place can drift from the
first. It is kept small — one file, no policy, no state beyond the reference
table — precisely so the drift has nowhere to hide.

### B. An extension module (PyO3, or a `cdylib` behind `ctypes`/`cffi`)

Python calls into the ORB in-process.

| | |
|---|---|
| New dependencies | **PyO3** (Apache-2.0 OR MIT — compatible) *or* none for a raw `cdylib` behind `ctypes` |
| Wire code | exists once, in Rust |
| Build | a compiled artefact per (OS, arch, CPython ABI): wheels, `maturin` or an equivalent, and a release process this project does not have |
| Cost | the dependency class, and `unsafe` at the boundary |
| Gain | no per-call process hop and no JSON round trip |

**The `unsafe` point matters more than the dependency and is easy to miss.**
`CLAUDE.md` gives a reason for `unsafe_code = "forbid"` — wire parsing is the
classic memory-safety hazard — and PyO3 confines its `unsafe` to its own crate,
so the workspace lint could survive. A hand-rolled `cdylib` with `ctypes` would
not: there the `extern "C"` surface is ours, and the lint would have to be
relaxed in the one crate whose whole job is to be handed foreign bytes.

### C. Pure-Python CDR and GIOP

No seam at all; the generated package speaks IIOP itself.

| | |
|---|---|
| New dependencies | none |
| Wire code | **twice.** A second encoder, a second alignment implementation, a second codeset negotiation |
| Build | nothing |
| Cost | the one this project has already paid for and written down |
| Gain | a Python client that needs nothing installed |

C looks cheapest and is refused on evidence rather than on taste: Phase 3's
`wstring` BOM failure came from re-implementing wire knowledge instead of
reusing it, and `orbweaver-gen` exists on the rule that a generated file
contains no encoding rules. A second implementation would also have to be
verified against two independent ORBs to earn the trust the Rust path has,
which is most of the cost of the Rust path over again.

---

## What is actually duplicated under A / A안에서 실제로 중복되는 것

Not zero, and this document would be dishonest if it claimed zero. The Python
runtime (`_rt.py`) is a **second implementation of §4.5** — the JSON mapping,
no wire. That is unavoidable for a target in any language: something in Python
has to turn Python objects into whatever crosses. What makes it acceptable is
that §4.5 is a written specification with a round-trip acceptance criterion, so
the second implementation is *checkable* against the first, and
`crates/orbweaver-gen/tests/python_target.rs` checks it over the whole golden
corpus in both byte orders.

B has the same duplication in a smaller form — Python objects still have to
become `Value`s. Only C avoids it, by duplicating something far worse instead.

§4.5의 두 번째 구현이 파이썬 쪽에 존재한다. 어떤 언어의 타깃이든 피할 수 없고,
§4.5가 왕복 수용 기준을 가진 명세이기 때문에 **검사 가능한** 중복이다. B안도 더
작은 형태로 같은 중복을 갖는다. C안만 이를 피하지만, 훨씬 나쁜 것을 대신 복제한다.

---

## Recommendation / 권고

**Adopt A for v1. Re-open with B when, and only when, a measurement says the
seam is the bottleneck.** The trigger should be a number: a recorded per-call
cost from a workload that exists, compared against the same calls through the
Rust stub. No such measurement has been taken, and this document does not
pretend one has.

Do not adopt C. If the requirement ever becomes "a Python client with nothing
installed", that is a *packaging* requirement, and B with pre-built wheels
answers it better than C does without buying a second ORB.

**A안 채택, B안은 측정이 병목을 가리킬 때만 재개.** 근거는 느낌이 아니라 숫자여야
하며, 그 숫자는 아직 없다. C안은 채택하지 않는다.

---

## What approval would mean / 승인의 의미

Approving **A** changes nothing that is not already built; it records that the
process seam is deliberate, and that the bridge is **not a security boundary**
(`orbweaver-mcp` is, and a second weaker gate at the bridge would be the §4.7
bypass in a new place).

Approving **B** is the consequential one: it admits a new dependency class (a
Python build toolchain in CI), commits to a release matrix, and requires a
decision about `unsafe_code` at the boundary. None of that should follow from
somebody merging a crate.

A 승인은 이미 만들어진 것에 의도를 기록하는 일이다 — 특히 브리지가 보안 경계가
아니라는 사실을. B 승인은 CI에 파이썬 빌드 툴체인을 들이고, 릴리스 매트릭스를
약속하고, 경계에서의 `unsafe_code`를 결정하는 일이다. 크레이트 하나 머지하는 것으로
따라와서는 안 된다.
