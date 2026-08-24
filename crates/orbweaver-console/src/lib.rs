//! The operator's console: three questions, answered from the sources of truth
//! that already answer them.
//!
//! `docs/PLAN.md` §6 names the console as catalog browser, contract diff viewer
//! and invocation traces. Those are one person's three questions, and the
//! person asking them is deciding what an AI agent may reach inside a legacy
//! estate:
//!
//! 1. **[`catalog`]** — what interfaces exist, which are exposed, which came
//!    off a foreign wire, which operations a scope gates, which need a human.
//! 2. **[`contract`]** — what a revision changes, and whether deployed peers
//!    survive it.
//! 3. **[`traces`]** — what was actually asked for, by whom, and what the
//!    policy said. Refusals findable at a glance; a dry run never mistakable
//!    for a call.
//!
//! # The rule this crate is built around
//!
//! **The console renders. It does not decide.**
//!
//! Every verdict on every page belongs to something else: exposure and gating
//! to [`orbweaver_mcp::dryrun`] run against the deployment's own interceptor
//! chain, provenance to [`orbweaver_registry::Registry`], evolution to
//! [`orbweaver_registry::diff`], and what happened to the audit's own
//! `decision` vocabulary. Nowhere here is there a second reading of `ai_authz`,
//! a second table of §5.3's rules, or a second definition of what "exposed"
//! means. A console that re-derived any of those would be a second policy: it
//! would agree with the real one right up until it did not, and an operator
//! would have made a deployment decision on the page rather than on the gate.
//!
//! When something is unknowable from those sources, the page says so — a scope
//! behind a closed exposure is *not reached*, not *none*; a missing trace field
//! is *absent*, not `-`; a line that would not parse is *counted*, not skipped.
//!
//! # Two consequences worth stating
//!
//! **No dependency.** No web framework, no template engine, no serialiser: the
//! rendering is `format!` and the JSON is `orbweaver_dynamic::json`, which was
//! written for AnyJSON and already owns its own limits. `cargo tree` stays at
//! the two external crates D004 measured, which is the bar that document held
//! observability to and there is no reason a viewer should clear a lower one.
//!
//! **No timing.** D004 fixes no duration field and explains why: it would need
//! a clock, the no-clock discipline is what makes trace replay deterministic,
//! and a duration nobody can reproduce is worse than an absent one. Nothing
//! here computes one, and tests in [`traces`] and [`catalog`] assert that
//! nothing does.
//!
//! **What is drawn is a translation unit, not a file.** [`load`] resolves
//! `#include` before anything is registered. A corpus of self-contained files
//! cannot tell the two apart; a thirteen-file estate can, and did — the
//! per-file path catalogued 58 of the estate's 76 reachable operations and said
//! nothing about the 18. See [`load`] for the measurement and for why loading
//! stops at syntax rather than running S4's gate.
//!
//! # The untrusted input this crate exists downstream of
//!
//! Since remote IFR ingestion the catalog holds repository ids, interface names
//! and prose a peer chose ([`orbweaver_registry::Origin::Ingested`]). A
//! `<script>` in one of those, rendered as markup, would make the console the
//! delivery vehicle for exactly the input the registry marks as untrusted —
//! §9.0's "tool poisoning via remote metadata", aimed at the one person meant
//! to be catching it. [`html`] answers that with a type rather than a habit:
//! there is no way to put a byte in a page except through
//! [`html::Markup::text`], which escapes, and tag and class names are
//! `&'static str`. `tests/escaping.rs` is the standing proof.
//!
//! # 콘솔은 그린다, 결정하지 않는다
//!
//! 세 화면의 모든 판정은 다른 곳의 것이다: 노출과 게이트는 배포된 인터셉터 체인
//! 위의 `dryrun`, 출처는 `Registry`, 진화 규칙은 `registry::diff`, 무슨 일이
//! 있었는지는 감사 로그의 `decision` 어휘. 여기서 `ai_authz`를 다시 읽거나 §5.3
//! 표를 다시 구현하는 곳은 없다. 두 번째 정책은 진짜 정책과 어긋나는 날까지만
//! 일치한다. 알 수 없는 것은 알 수 없다고 그린다 — 닫힌 노출 뒤의 스코프는
//! *도달하지 않음*이지 *없음*이 아니고, 없는 필드는 *부재*이지 `-`가 아니다.
//! 의존성은 0(웹 프레임워크도 템플릿 엔진도 없다), 시간 측정도 0(D004가 기간
//! 필드를 두지 않는다). 그리고 인제스트된 이름은 외부 와이어에서 온 신뢰할 수
//! 없는 입력이므로, 값이 페이지에 들어가는 문은 이스케이프하는 [`html::Markup::text`]
//! 하나뿐이다.

#![deny(missing_docs)]

pub mod catalog;
pub mod contract;
pub mod declarations;
pub mod html;
pub mod load;
pub mod traces;
