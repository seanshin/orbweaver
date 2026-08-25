# D022 — Opening the trading service, and what `Lookup::query` drags in

**STATUS: PROPOSED** — drafted 2026-08-25 on a request to plan the direction so
the trading service can be opened. Every measurement below was read that day
from `crates/orbweaver-trading/`, `docs/PLAN-SERVICES.md` §3 and
`SERVICES-COVERAGE.md` §8. Not self-approvable: §2 turns on whether a deferral
has been named out of, and §6 adds a wire surface.

**상태: 제안** — 2026-08-25, 트레이딩 서비스가 개시될 수 있도록 방향을 기획하라는
요청에서 작성.

---

## 1. What is already open, measured / 이미 열려 있는 것

`PLAN-SERVICES` §3 marks trading **✅ engine and project-contract wire both
landed**, and the measurement agrees:

- **Engine** — `orbweaver-trading`, 2,086 lines, **40 tests today**: `Offer`,
  `OfferStore`, `Query`, `Selection`, **`Truth` (three-valued)**,
  `LoadingPolicy`, `Decision`, `Residency`, and `replay` with `TraceEvent` /
  `SnapshotDecisions` / `PolicyReport` for deterministic trace replay.
- **Wire** — landed as the **project** contract, not the standard one:
  `moe::ExpertRegistry` + `moe::ExpertLoader` from `corpus/golden/22`, served
  on our POA. MoE enterprise is 16 of 16 in `SERVICES-COVERAGE` §8.
- **Standard `CosTrading`** — appears **zero times** in the coverage document.

So "opening the trading service" does not mean building a trader. It means one
thing precisely: **`CosTrading::Lookup::query` answering a client that is not
ours.**

## 2. The deferral, and whether it has been named out of / 유예와 지명

`PLAN-SERVICES` §3 records the deferral with its reason, which is unusually
well argued and should be read rather than summarised:

> OMG CosTrading is enormous (five interfaces, federated links, proxy offers,
> dynamic properties). Nothing in stream F consumes more than
> property-constrained lookup over registered offers … the standard
> `CosTrading::Lookup::query` facade is still deferred **until a foreign
> trading client is named** — the IFR-facade rule (§7) applied to Trading.
> Deferral is recorded here so it is a decision, not a drift.

**A request to open the service is that naming**, in the same way D021 §2 reads
the event request. This document proceeds on that reading and says so plainly
rather than burying it; if the reading is wrong, §7 says what changes.

One structural note, and it is a real finding: **this deferral's trigger lives
in the wrong document.** `PLAN-DEFERRED` §7 covers *federated naming and
trading links* — a different exclusion with a different trigger — and the
**facade** deferral has no chapter there at all. A reader of `PLAN-DEFERRED`
alone does not learn the facade is deferred; a reader of `PLAN-SERVICES` alone
does not learn the federation is. Two trading exclusions, two documents, and
neither names the other.

*유예의 방아쇠가 잘못된 문서에 산다. `PLAN-DEFERRED` §7은 **연합**을 다루고,
**파사드** 유예는 거기에 장이 없다. 트레이딩 제외가 둘인데 문서도 둘이고, 어느
쪽도 상대를 이름하지 않는다.*

## 3. What `Lookup::query` drags in / `query`가 끌고 오는 것

The operation is one line in IDL and four dependencies in practice:

```idl
void query(in ServiceTypeName type, in Constraint constr, in Preference pref,
           in PolicySeq policies, in SpecifiedProps desired_props,
           in unsigned long how_many,
           out OfferSeq offers, out OfferIterator offer_itr,
           out PolicyNameSeq limits_applied) raises (…8 exceptions…);
```

| Dependency | What it is | Where we stand |
|---|---|---|
| **`ServiceTypeName` + the type repository** | `CosTradingRepos::ServiceTypeRepository`: a service type is a name, an interface repository id, and a property schema | **absent** — our offers carry properties with no declared schema and no type name |
| **`Constraint` (TCL)** | the OMG Trader Constraint Language | **a documented subset** — see §4 |
| **`Preference`** | a *separate* expression language: `min`/`max`/`with`/`random`/`first` | **absent as a language** — we have `ORDER BY field ASC\|DESC` |
| **`OfferIterator`** | a POA-hosted object per query, with a lifecycle | **the hazard this project already refused once** — see §5 |
| `PolicySeq`, `desired_props`, `limits_applied`, and the three attribute interfaces `TraderComponents` / `SupportAttributes` / `ImportAttributes` | cardinality bounds, projection, and what the trader admits about itself | absent, and mostly small |

## 4. The constraint language, item by item / 제약 언어

Our grammar documents its own scope, and the honesty is the useful part:

```
query      := comparison ( "AND" comparison )*  order?
comparison := field cmp literal
cmp        := "==" | "!=" | "<" | "<=" | ">" | ">="
order      := "ORDER" "BY" field ( "ASC" | "DESC" )
```
> **Scope.** Exactly the subset above and nothing else: no `OR`, no
> parentheses, no unit suffixes, no case-insensitive keywords.

Against TCL that is missing: `or`, `not`, parentheses, **`exist <prop>`**,
`~` (substring), `in` (sequence membership), arithmetic, and the whole
preference expression. The comparison operators match exactly.

**`exist` is the interesting one**, because we are further along than it looks:
`Truth` is already three-valued and `Selection::is_complete` / `gap_note`
already report when a property was unanswerable rather than false. TCL's
`exist` is precisely a first-class query over that state. The three-valued
matcher was built for the MoE contract's unpopulated fields (PLAN-MOE §4.5) and
turns out to be the piece TCL needs — **that is the one place our engine is
ahead of the subset, not behind it.**

## 5. The iterator, and the precedent that decides it / 반복자와 선례

`OfferIterator` is a POA-hosted object per query with a lifecycle — which is
**exactly** what `COMPONENTS.md` records as deliberately not built for
`DynAny`: *"it needs a POA-hosted object per component with a lifecycle, which
is the reference-outliving-its-value hazard the local design removes."*

The specification makes the escape legal and it should be taken: when the
number of matching offers is at most `how_many`, **all of them are returned in
`offers` and `offer_itr` is nil.** A first landing that answers `query` with a
bounded `how_many` and a nil iterator is conformant for every query that fits,
and refuses — by name, with the bound — for one that does not.

**That is the shape of the whole proposal**: not a smaller trader, but a
trader that answers exactly the queries it can answer completely and refuses
the rest with the reason, which is the posture every other service in this
workspace already takes.

*명세가 그 탈출구를 합법으로 만든다: 결과가 `how_many`에 들어가면 반복자는
nil이다. 이것이 제안 전체의 모양이다 — 더 작은 트레이더가 아니라, **완전히
답할 수 있는 질의에만 답하고 나머지는 이유와 함께 거부하는** 트레이더.*

## 6. What is proposed / 제안

Four stages. `orbweaver-trading` is the only service footprint free today, so
stage T1 can start immediately; T2–T4 each name what they wait on.

### T1 — the constraint language reaches TCL's shape (engine only)

`or`, `not`, parentheses and **`exist`**, in `orbweaver-trading::query`. No
wire surface, no new crate, and every existing query still parses — the grammar
grows, it does not change. `exist` is wired to the `Truth`/`gap_note` machinery
that already exists rather than to a new one.

**Oracle.** The existing 40 tests plus a table of TCL expressions with their
expected `Truth` including the unanswerable cases; the negative control is a
malformed expression still naming its byte position, which the parser's own
S4-style diagnostics already do.

**Why first.** One crate, free footprint, no decision needed, and it is the
dependency every later stage has. Also the cheapest place to discover that our
`Truth` is ahead of TCL rather than behind it — or that it is not.

### T2 — the preference expression

`min`/`max`/`with`/`random`/`first` as a second parsed expression, replacing
`ORDER BY` as the wire-facing form while keeping `ORDER BY` for our own
callers. Engine only. Waits on T1.

### T3 — a service type, minimally

`ServiceTypeName` plus the property schema, checked at registration. **Not the
full `ServiceTypeRepository` interface** — the *type* is what `query` needs;
the repository is another servant and earns its place when a client asks to
enumerate types. Our repository ids already exist and are the natural key.
Waits on T1; touches `orbweaver-trading` and possibly `orbweaver-registry`.

### T4 — `CosTrading::Lookup::query` on the wire, iterator nil

The servant, on our POA, answering `query` for results that fit `how_many` and
refusing beyond it by name. `TraderComponents` / `SupportAttributes` /
`ImportAttributes` answer what they must about a trader with no links and no
proxies. **Waits on T1–T3 and on a free `orbweaver-object` footprint.**

**Oracle — and it is the point of opening the service at all:** omniORB's
client resolving `TradingService` and calling `query`. That is a foreign
trading client, which is the thing the deferral was waiting to be named. If
omniORB ships no trader client, say so and the oracle becomes a hand-written
peer, as `spikes/half_reply_peer.py` is for GIOP.

## 7. What must not happen / 해서는 안 되는 것

- **No federated links, no proxy offers, no dynamic properties.** Those are
  `PLAN-DEFERRED` §7's and stay deferred; §7's trigger (more than one
  naming/trading domain behind one MCP face) is unchanged by any of this and
  was re-measured 2026-08-25 as **not fired**.
- **No `OfferIterator` object** until a query that cannot fit is named. §5's
  escape is conformant; building the lifecycle is the DynAny hazard.
- **No `ServiceTypeRepository` servant** in T3. The type is the dependency; the
  repository is a second service.
- **If §2's reading is wrong**, T1 and T2 proceed anyway — they are engine work
  that improves the MoE contract's own queries — and T3/T4 stop. The deferral's
  trigger is then **re-dated with today's reading written down**, and moved to
  `PLAN-DEFERRED` where §2 says it belongs.

## 8. On registering `TradingService` / `TradingService` 등록에 대하여

§8.5.2's reserved-ObjectId table (verified 2026-08-25) maps `TradingService` →
`CosTrading::Lookup`, and the specification even gives the configuration
example `-ORBInitRef TradingService=corbaname::555objs.com#Dev/Trader`. So
D019 step 1's initial-references table has a slot for this service, and until
T4 lands **that slot must refuse by name** — which is exactly where a client
meets the facade deferral, and the best possible place for it to meet one.
