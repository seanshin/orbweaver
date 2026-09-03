#!/usr/bin/env bash
# Builds omniORBpy with its SSL transport, so `spikes/echo_server_ssl.py` can run.
#
# **A fixture builder, not a gate** — on exactly the terms `spikes/tao/setup.sh`
# is one. Nothing in `run_checks.sh` or `ci.yml` runs this; where the fixture is
# absent the SSLIOP group stays a counted `SKIPPED` saying it is unmeasured,
# and where it has been built the harness finds it here without being told.
#
# TEST FIXTURE. omniORB and omniORBpy are LGPL/GPL: built and run here only,
# never linked into Orbweaver, never vendored, never committed, never published.
# The build lands under `spikes/tls/omniORBpy/`, which this repository ignores.
# See CLAUDE.md's licensing boundary.
#
# WHY IT IS BUILT AND NOT INSTALLED. Homebrew's `omniorb` ships the C++ SSL
# transport (`libomnisslTP`) but its bundled omniORBpy was built without the
# `sslTP` python module — `spikes/tls/PEER-STATUS.md` measured that on
# 2026-08-13 and named this exact path as unblock option 1. The keg already has
# everything the binding needs; only the binding is missing.
#
# WHAT IT NEEDS. `pkg-config` (`brew install pkgconf`), which is what stopped the
# first configure on 2026-09-03; the brew `omniorb` and `openssl@3` kegs.
#
# SUCCESS CRITERION, the one PEER-STATUS.md fixed before any of this existed:
#
#   PYTHONPATH=<site-packages> python3 -c "from omniORB import sslTP"
#
# *픽스처 빌더이지 게이트가 아니다. 없으면 SSLIOP 그룹은 계수된 SKIPPED로 남고,
# 빌드해 두면 하네스가 알아서 찾는다. Homebrew의 omniORBpy는 `sslTP` 없이 빌드되어
# 있고 keg에는 C++ SSL 트랜스포트가 이미 있으므로, 바인딩만 소스 빌드한다.
# LGPL/GPL — 여기서 빌드·실행만 하고 절대 벤더링·커밋·배포하지 않는다.*
set -euo pipefail
cd "$(dirname "$0")"

VER=4.3.4   # matches the brew omniorb core exactly; a mismatch is a different bug
HERE=$(pwd)
BUILD=$HERE/omniORBpy
PREFIX=$BUILD/install
SITE=$PREFIX/lib/python3.$(python3 -c 'import sys; print(sys.version_info.minor)')/site-packages

if PYTHONPATH="$SITE" python3 -c "from omniORB import sslTP" 2>/dev/null; then
  echo "sslTP already imports from $SITE"
  exit 0
fi

command -v pkg-config >/dev/null 2>&1 || {
  echo "pkg-config is absent (brew install pkgconf); omniORBpy's configure needs it"
  exit 2
}
OMNI=/opt/homebrew/opt/omniorb
SSL=/opt/homebrew/opt/openssl@3
[ -d "$OMNI" ] && [ -d "$SSL" ] || {
  echo "the brew omniorb and openssl@3 kegs are needed at $OMNI and $SSL"
  exit 2
}

mkdir -p "$BUILD" && cd "$BUILD"
TARBALL=omniORBpy-$VER.tar.bz2
URL="https://sourceforge.net/projects/omniorb/files/omniORBpy/omniORBpy-$VER/$TARBALL/download"
[ -s "$TARBALL" ] || curl -fL --retry 3 --retry-delay 2 --max-time 600 -o "$TARBALL" "$URL"
[ -d "omniORBpy-$VER" ] || tar xjf "$TARBALL"

cd "omniORBpy-$VER"
./configure --with-omniorb="$OMNI" --with-openssl="$SSL" --prefix="$PREFIX" \
  >"$BUILD/configure.log" 2>&1 || { tail -20 "$BUILD/configure.log"; exit 1; }
make -j"$( (nproc 2>/dev/null || sysctl -n hw.ncpu) | tr -d ' ')" \
  >"$BUILD/make.log" 2>&1 || { grep -iE "error" "$BUILD/make.log" | head -20; exit 1; }
make install >"$BUILD/install.log" 2>&1 || { tail -20 "$BUILD/install.log"; exit 1; }

# The success criterion, run rather than inferred from `make` exiting 0.
if PYTHONPATH="$SITE" python3 -c "from omniORB import sslTP"; then
  echo "sslTP imports from $SITE"
  echo "run the fixture with: PYTHONPATH=$SITE python3 spikes/echo_server_ssl.py"
else
  echo "built, but sslTP does not import — the build is not the fixture"
  exit 1
fi
