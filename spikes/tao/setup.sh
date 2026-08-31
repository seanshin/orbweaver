#!/usr/bin/env bash
# Builds the TAO IDL compiler, the differential's third front end.
#
# **A fixture builder, not a gate** — nothing in `run_checks.sh` or `ci.yml`
# runs this, deliberately, and the cost that decides it is measured below.
# Where the fixture is absent the `tao_idl` column is a counted `SKIPPED`
# saying it is unmeasured, which is what it honestly is; where it has been
# built, `differential.sh` finds it here without being told. Whether CI
# should pay to retire that SKIPPED is the owner's call and is asked in
# `docs/PLAN-FIRST-COMPLETION.md` §2.
#
# *게이트가 아니라 픽스처 빌더다. 없으면 계수된 SKIPPED로 남고, 빌드해 두면
# differential이 알아서 찾는다.*
#
# TEST FIXTURE, on exactly the terms omniORB and JacORB are here on. ACE+TAO is
# DOC-licensed; none of it is linked into Orbweaver, nothing is vendored into
# this repository, and no artifact built here is ever published. `tao_idl` is an
# external program whose text output and exit status we read — the same relation
# `omniidl` has. See CLAUDE.md's licensing boundary and docs/PLAN.md §10.
#
# WHY IT IS BUILT AND NOT INSTALLED. Homebrew's `ace` formula downloads the
# ACE+TAO tarball and then builds only `ace/` — there is no `tao_idl` in it, and
# `brew search tao` finds no formula that has one. So the fixture is the
# upstream source, built here, into a directory this repository ignores.
#
# WHAT IT IS FOR (D035 §8 step 2, the owner's amended order). Two things move
# for one fixture: it is what could refute candidate C, and it retires the
# differential's standing `SKIPPED tao_idl absent — its column is unmeasured,
# not passing`. The owner's terms include the outcome where it does not stand
# up: **that is a result, to be recorded rather than worked around.** It stood
# up on 2026-08-31, on macOS 15 / arm64 / Apple clang, in about three minutes.
#
# 테스트 픽스처. omniORB·JacORB와 정확히 같은 조건이다 — DOC 라이선스이며 링크하지
# 않고, 저장소에 넣지 않고, 여기서 만든 산출물을 배포하지 않는다. Homebrew의 `ace`
# 포뮬러는 `ace/`만 빌드하므로 `tao_idl`이 없어서 소스에서 빌드한다.
# THE COST BEHIND THAT REFUSAL, measured rather than asserted.
# `spikes/cited_and_run.py` asks every cited executable for a runner, and this
# one refuses rather than defers. The reason is a cost the owner has just been
# handed a brief about: `ci.yml`'s differential job takes **57 seconds** today
# (measured 2026-08-31, run 33345049823), and this build takes **about three
# minutes on twelve cores** and produces **532 MB**. Wiring it in multiplies
# that job's minutes by roughly four on every push, on a repository whose CI
# volume was the subject of that morning's change — and caching a DOC-licensed
# build tree is a licensing judgement this file will not make on its own.
#
# So the fixture is **built on demand and found automatically once built**:
# `differential.sh` looks for it here as well as on PATH, and where it is absent
# the `tao_idl` column is a counted `SKIPPED` saying the column is unmeasured —
# which is what it honestly is. Whether CI should pay those minutes to retire
# that SKIPPED is the owner's call, and is named in
# `docs/PLAN-FIRST-COMPLETION.md` §2 rather than left in this header.
#
# *게이트가 돌리지 않으며, 그것은 유예가 아니라 결정이다. differential 잡은 오늘
# 57초이고 이 빌드는 12코어에서 약 3분·532 MB다. 매 푸시마다 그 잡을 네 배로
# 만드는 것과, DOC 라이선스 빌드 트리를 캐시하는 것은 이 파일이 혼자 내릴 판단이
# 아니다. 없으면 계수된 SKIPPED로 남는다 — 그것이 정직한 상태다.*
set -euo pipefail
cd "$(dirname "$0")"

VER=8.0.7
ROOT_DIR=$(pwd)
ACE=$ROOT_DIR/ACE_wrappers
JOBS=$( (nproc 2>/dev/null || sysctl -n hw.ncpu) | tr -d ' ' )

if [ -x "$ACE/bin/tao_idl" ]; then
  echo "tao_idl already built: $ACE/bin/tao_idl"
  "$ACE/bin/tao_idl" -V 2>&1 | sed -n '2,3p'
  exit 0
fi

TARBALL=ACE+TAO-$VER.tar.bz2
URL="https://github.com/DOCGroup/ACE_TAO/releases/download/ACE%2BTAO-${VER//./_}/ACE+TAO-$VER.tar.bz2"
# `--retry` for the same transient 5xx class the JacORB fetch already covers.
[ -s "$TARBALL" ] || curl -fL --retry 3 --retry-delay 2 --max-time 600 -o "$TARBALL" "$URL"
[ -d "$ACE" ] || tar xjf "$TARBALL"

case "$(uname -s)" in
  Darwin) OS=macosx ;;
  Linux)  OS=linux ;;
  *) echo "no ACE platform config for $(uname -s) — tao_idl not built"; exit 2 ;;
esac
ln -sf "config-$OS.h" "$ACE/ace/config.h"
ln -sf "platform_$OS.GNU" "$ACE/include/makeinclude/platform_macros.GNU"

export ACE_ROOT="$ACE" TAO_ROOT="$ACE/TAO"
export LD_LIBRARY_PATH="$ACE/lib:${LD_LIBRARY_PATH:-}"
export DYLD_LIBRARY_PATH="$ACE/lib:${DYLD_LIBRARY_PATH:-}"

# Only the three pieces the IDL compiler needs. Building all of TAO would take
# far longer and produce an ORB we have no use for: the fixture is a front end,
# not a wire peer. If a TAO wire peer is ever wanted, that is a separate build
# and a separate decision, because it is a second ORB in the tree.
make -C "$ACE/ace" -f GNUmakefile.ACE debug=0 shared_libs=1 static_libs=0 -j"$JOBS" >/dev/null
# ace_gperf, or every tao_idl run prints four lines about perfect hashing that a
# reader learns to scroll past — the same reason the SIGKILL arm replaced
# `abort()` in the shutdown fixture.
make -C "$ACE/apps/gperf/src" debug=0 -j"$JOBS" >/dev/null
make -C "$ACE/TAO/TAO_IDL" debug=0 shared_libs=1 static_libs=0 -j"$JOBS" >/dev/null

[ -x "$ACE/bin/tao_idl" ] || { echo "the build finished and produced no tao_idl"; exit 1; }
echo "tao fixture ready: $ACE/bin/tao_idl"
"$ACE/bin/tao_idl" -V 2>&1 | sed -n '2,3p'
cat <<EOF

To let spikes/differential.sh see it:
  export ACE_ROOT=$ACE
  export DYLD_LIBRARY_PATH=\$ACE_ROOT/lib:\${DYLD_LIBRARY_PATH:-}
  export LD_LIBRARY_PATH=\$ACE_ROOT/lib:\${LD_LIBRARY_PATH:-}
  export PATH=\$ACE_ROOT/bin:\$PATH
EOF
