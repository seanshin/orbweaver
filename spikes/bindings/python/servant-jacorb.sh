#!/usr/bin/env bash
# Cell: servant × jacorb. A translator, not a measurement.
#
# `spikes/jacorb_python_servant.sh` is left EXACTLY as it landed — this script
# runs it and turns what it printed into the suite's observation vocabulary.
# That is deliberate and it is the migration's oracle: B3 requires today's
# groups to "produce byte-identical results as an instance", and the cheapest
# way to guarantee that is for the instrument to be the same bytes, invoked the
# same way, with the translation living outside it.
#
# The order is READ, not claimed: `python_servant_wire.rs`'s tap takes it off
# GIOP §15.4.1's flag bit of every request JacORB actually wrote. This script
# only reads that back out of the line the test printed, and if the line is not
# there it says so rather than assuming what it would have said.
#
# Exit: 0 ok · 1 red · 2 the fixture is absent — passed through unchanged from
# the script below, so a missing JDK is unmeasured here exactly as it is there.
#
# *번역기이지 측정이 아니다. 아래 스크립트는 착지한 그대로 두고, 그것이 인쇄한 것을
# 스위트의 어휘로 옮긴다 — 이것이 이관의 오라클이다.*
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT" || exit 1

out=$(./spikes/jacorb_python_servant.sh "$@" 2>&1); rc=$?

if [ "$rc" -eq 2 ]; then
  grep -E "SKIPPED" <<<"$out"
  exit 2
fi
if [ "$rc" -ne 0 ]; then
  printf '%s\n' "$out"
  exit 1
fi

# `read off the wire at GIOP 1.2: 11 request(s) from JacORB, flag byte says big; …`
# Captured to a variable and matched with a herestring throughout: nothing is
# piped into an early-exit matcher anywhere in this file.
wire=$(grep "read off the wire at" <<<"$out")
if [ -z "$wire" ]; then
  echo "FAIL	the script passed but printed no \"read off the wire\" line, so this cell"
  echo "FAIL	has no byte-order reading to report and would otherwise count as covered"
  exit 1
fi

n=0
while IFS= read -r line; do
  [ -n "$line" ] || continue
  v=$(sed -n 's/.*read off the wire at GIOP \([0-9][0-9]*\.[0-9][0-9]*\):.*/\1/p' <<<"$line")
  o=$(sed -n 's/.*flag byte says \([a-z][a-z]*\).*/\1/p' <<<"$line")
  if [ -z "$v" ] || [ -z "$o" ]; then
    echo "FAIL	could not read a version and an order out of: $line"
    exit 1
  fi
  printf 'observed\tgiop=%s\torder=%s\n' "$v" "$o"
  n=$((n+1))
done <<<"$wire"

same=$(grep -c "byte-identical at" <<<"$out")
printf 'note\t%s version(s) with the order read off the flag byte; %s byte-identity result(s) against a Rust servant\n' "$n" "$same"
exit 0
