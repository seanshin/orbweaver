# SSLIOP peer fixture — status / SSLIOP 피어 픽스처 — 현황

`crates/orbweaver-giop/src/ssliop.rs` and `tests/ssliop_tls.rs` state the open
claim exactly: the TLS path is verified against an in-process rustls peer
only; **no SSLIOP-speaking ORB has been dialed**. This batch set out to build
that peer — a stock omniORB python server over `sslTP`, the SSL twin of
`spikes/echo_server.py`. The probe below blocked it. Per the honesty rules,
a blocked measurement is reported as blocked, not worked around with a
fixture that fakes the claim.

`ssliop.rs`와 `tests/ssliop_tls.rs`가 미해결 주장을 정확히 명시하고 있다: TLS
경로는 인프로세스 rustls 피어로만 검증되었고 **SSLIOP를 말하는 ORB와는 아직
통신한 적이 없다**. 이번 배치는 그 피어 — `spikes/echo_server.py`의 SSL 쌍둥이인
omniORB `sslTP` 서버 — 를 만들려 했으나 아래 조사에서 막혔다. 정직성 규칙에
따라, 막힌 측정은 막혔다고 보고한다. 주장을 흉내 내는 픽스처로 우회하지 않는다.

## What was probed / 무엇을 조사했나

Probed 2026-08-13 on the development machine (macOS, Homebrew `omniorb 4.3.4`,
Python 3.14.6, `omniORB.coreVersion()` = 4.3.4):

```
$ python3 -c "from omniORB import sslTP; print(sslTP)"
ImportError: cannot import name 'sslTP' from 'omniORB'
  (/opt/homebrew/lib/python3.14/site-packages/omniORB/__init__.py)
```

개발 머신(macOS, Homebrew `omniorb 4.3.4`, Python 3.14.6)에서 2026-08-13에
조사했다. 위 한 줄 임포트가 `ImportError`로 실패한다.

## Where the gap is, exactly / 정확히 어디가 비어 있나

The block is one build flag wide, not a missing product:

- Homebrew's `omniorb` formula **does** depend on `openssl@3` and **does**
  ship the C++ SSL transport: `libomnisslTP4.dylib`, the `sslContext.h` /
  `sslEndpoint.h` header family, and `share/idl/omniORB/COS/SSLIOP.idl` are
  all present under `/opt/homebrew/Cellar/omniorb/4.3.4/`.
- The **bundled omniORBpy python bindings were built without the SSL
  transport**: `find` over the whole keg finds no `omniORB/sslTP.py` and no
  `_omnisslTP*.so`. The shipped python extensions are `_omnipy`,
  `_omnicodesets`, `_omniConnMgmt`, `_omniZIOP`, `_omniidl` only.

So the C++ half of the peer exists on this machine today; the python binding
half does not, and `sslTP` is a python-side module.

막힌 폭은 빌드 플래그 하나다. Homebrew `omniorb`는 `openssl@3`에 의존하고 C++
SSL 트랜스포트(`libomnisslTP4.dylib`, `sslContext.h` 계열 헤더, `SSLIOP.idl`)를
**모두 포함**한다. 그러나 함께 들어 있는 omniORBpy 파이썬 바인딩은 SSL
트랜스포트 없이 빌드되었다: keg 전체를 `find`해도 `omniORB/sslTP.py`도
`_omnisslTP*.so`도 없다. `sslTP`는 파이썬 쪽 모듈이므로 피어의 C++ 절반은
있고 파이썬 절반이 없는 상태다.

## Verified today vs. unmeasured / 오늘 검증된 것과 미측정인 것

**Verified** (in-process rustls peer, `crates/orbweaver-giop/tests/ssliop_tls.rs`,
using the self-originated fixtures in this directory): TLS establishment,
certificate verification on and effective (`wrong-ca.pem` refused), GIOP bytes
crossing the encrypted transport unchanged, clean refusal of a non-TLS peer.

**Measured 2026-08-25** (`spikes/ssliop.sh`, **21 of 21 cases**, exit 0):
`Connection::connect_tls` against an **out-of-process** peer whose TLS is
OpenSSL and not rustls, and `ssliop::advertised` / `ssl_endpoint` over a
`TAG_SSL_SEC_TRANS` component **this project's encoder did not write** — both
IOR and component byte orders independently, including a little-endian
component inside a big-endian IOR, a shape a deployment produces and our
encoder never does. The dialled address is never handed in: it comes out of
`ssl_endpoint` and is cross-checked against the port the peer says it listens
on, so the call cannot pass by being told the answer. Five refusals, among them
the two downgrade directions, one of them evidenced from the far end — the peer
observed a TLS ClientHello arriving in cleartext, which is how "the client did
not downgrade" is proved rather than asserted.

The peer is `spikes/ssliop_peer.py`: stdlib `ssl`, **no ORB imported**, every
GIOP and IOR octet built by hand. That is deliberate and it is the finding —
SSLIOP is unmodified GIOP over TLS plus a component, so peer proof needs a peer
that speaks IIOP over TLS, not another ORB's SSLIOP stack. See D010 §4 B3.

**Still unmeasured:** a `TAG_SSL_SEC_TRANS` component produced by **omniORB's
or JacORB's own encoder**. That is a claim about their encoder and only they can
make it. The unblock paths below are now what that one residue needs; they are
no longer what SSLIOP peer proof needs.

**검증됨**(인프로세스 rustls 피어): TLS 수립, 인증서 검증이 켜져 있고 실제로
작동함(`wrong-ca.pem` 거부), GIOP 바이트의 무손상 통과, 비-TLS 피어의 깨끗한
거부.

**2026-08-25 측정**(`spikes/ssliop.sh`, **21/21**, exit 0): TLS가 rustls가 아닌
OpenSSL인 **외부 프로세스** 피어에 대한 `connect_tls`, 그리고 **우리 인코더가
쓰지 않은** `TAG_SSL_SEC_TRANS` 컴포넌트의 파싱 — IOR과 컴포넌트 바이트 순서를
각각 양쪽으로, 빅엔디언 IOR 안의 리틀엔디언 컴포넌트(배포가 만들고 우리 인코더는
만들지 않는 모양)를 포함해서. 다이얼할 주소는 넘겨주지 않는다: `ssl_endpoint`가
뱉은 것을 쓰고 피어가 말하는 포트와 교차 확인하므로, 답을 들어서 통과할 수 없다.
거부 다섯 중 평문 다운그레이드 거부는 **반대편에서** 증거를 잡았다 — 피어가 TLS
ClientHello가 평문으로 도착하는 것을 관측했다.

피어는 `spikes/ssliop_peer.py`이며 stdlib `ssl`을 쓰고 **ORB를 하나도 임포트하지
않는다**. 그것이 의도이자 발견이다 — SSLIOP는 TLS 위의 변경 없는 GIOP에 컴포넌트
하나이므로, 피어 증명에 필요한 것은 다른 ORB의 SSLIOP 스택이 아니라 IIOP over
TLS를 말하는 피어다(D010 §4 B3).

**미측정으로 남는 것:** **omniORB나 JacORB 자신의 인코더**가 만든
`TAG_SSL_SEC_TRANS` 컴포넌트. 그들의 인코더에 대한 주장이므로 그들만이 할 수
있다. 아래 경로들은 이제 그 하나의 잔여를 위한 것이지, 피어 증명을 위한 것이
아니다.

## What installing the peer would take / 피어 설치에 필요한 것

In likely order of preference. omniORB/omniORBpy are LGPL/GPL: build and run
locally or inside CI only, never vendor, commit, or publish (licensing
boundary in `CLAUDE.md`).

1. **Build omniORBpy from source against the brew keg.** The keg already has
   the SSL-enabled C++ libraries, so only the binding needs building:
   configure omniORBpy 4.3.x with omniORB at `/opt/homebrew/opt/omniorb` and
   OpenSSL at `/opt/homebrew/opt/openssl@3`, install into a venv or a
   `PYTHONPATH` prefix. Success criterion is the probe one-liner importing.
2. **A Linux CI container** with a distro omniORBpy. Whether any given distro
   build enables `sslTP` is itself unmeasured — run the same probe one-liner
   in the container before relying on it.
3. **A JacORB SSL peer** (`spikes/jacorb/` already holds an IIOP fixture pair)
   — a second, independent SSLIOP implementation, at the cost of JSSE
   keystore setup on top of the PEM fixtures here.

선호 순서대로: (1) brew keg에는 SSL이 켜진 C++ 라이브러리가 이미 있으므로
omniORBpy만 소스 빌드해 venv에 설치한다 — 성공 기준은 위 임포트 한 줄.
(2) 리눅스 CI 컨테이너의 배포판 omniORBpy — 해당 빌드가 `sslTP`를 켰는지는
그 자체로 미측정이므로 의존하기 전에 같은 임포트로 조사한다. (3) JacORB SSL
피어 — 독립된 두 번째 구현체이나 JSSE 키스토어 설정 비용이 붙는다.
omniORB/omniORBpy는 LGPL/GPL이므로 로컬·CI 안에서 빌드/실행만 하고 절대
벤더링·커밋·배포하지 않는다.

## The fixture and its oracle, once `sslTP` imports / `sslTP`가 임포트되면

`spikes/echo_server_ssl.py` — the same `Echo` servant as `echo_server.py`,
differing only in transport setup, all before `CORBA.ORB_init`:

- `sslTP.certificate_authority_file(str(HERE / "tls" / "ca.pem"))` and the
  server credential from `tls/server.pem` + `tls/server.key`. Confirm the
  exact credential API against the built version: newer omniORBpy exposes a
  separate certificate setter alongside `key_file`; older ones want the chain
  and key concatenated into one PEM (`cat server.pem server.key`) passed to
  `key_file`. Do not guess — read the built module.
- `ORB_init` argv includes `-ORBendPoint giop:ssl::<port>` so the published
  profile advertises the SSL endpoint; write the IOR to `spikes/echo_ssl.ior`
  and print `READY`, same contract as `echo_server.py`.

The deterministic oracle already exists: `cargo run -q --bin spike-dump --
spikes/echo_ssl.ior` prints an `ssliop` line for every IOR — today
`ssliop  no TAG_SSL_SEC_TRANS`, and for a correct fixture
`ssliop  supports=... requires=... port=...` followed by
`ssliop  TLS endpoint would be <host>:<port>`. That line is the fixture's
acceptance check before any TLS handshake is attempted.

What integration wires into `run_checks.sh` (a later batch; this file only
specifies it): start the fixture and record its PID — stop it only by that
captured PID, never by pattern; wait for `echo_ssl.ior` with a sleeping,
deadline-bounded loop; capture the `spike-dump` output to a variable and match
the `ssliop` line (never pipe into `grep -q`); then the wire proof —
`Connection::connect_tls` with a rustls config trusting `tls/ca.pem`, one
`ping`/`add` round-trip, plus the negative control that a config trusting only
`tls/wrong-ca.pem` is refused. If the fixture will not start, the group
fails — an unmeasured check is a failure, never a pass.

픽스처는 `echo_server.py`와 동일한 서번트에 트랜스포트 설정만 다르다:
`ORB_init` 전에 `sslTP` 자격 증명(CA는 `tls/ca.pem`, 서버 자격은
`tls/server.pem`+`server.key` — 정확한 API는 빌드된 모듈을 읽고 확인하되,
구버전은 체인과 키를 한 PEM으로 이어 붙여 `key_file`에 넘긴다)을 등록하고
`-ORBendPoint giop:ssl::<port>`로 초기화하여 `echo_ssl.ior`를 쓴다. 결정적
오라클은 이미 있다: `spike-dump`가 모든 IOR에 대해 `ssliop` 줄을 출력하며,
올바른 픽스처라면 `supports=... requires=... port=...`와 TLS 엔드포인트 줄이
나와야 한다. 통합(이후 배치)은 하네스 규칙 그대로 — 캡처한 PID로만 기동/종료,
잠들며 기다리는 마감 있는 대기 루프, 변수로 캡처 후 매칭 — `connect_tls`
왕복과 `wrong-ca.pem` 부정 통제까지 잇는다. 픽스처가 뜨지 않으면 그 그룹은
실패다. 측정되지 않은 검사는 통과가 아니다.
