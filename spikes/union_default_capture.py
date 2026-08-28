#!/usr/bin/env python3
"""Re-capture a peer's `default:`-bearing union TypeCodes and check the recording.

Two test files hold byte sequences omniORB wrote for unions with a `default:`
branch, and this regenerates all of them from the live fixture and compares,
padding excluded (`pad_mask` from `union_label_capture.py`, the sibling that
recorded the labelled cases and learned not to list offsets):

- `crates/orbweaver-giop/tests/union_default_label_from_a_peer.rs` — one per
  discriminator kind that gives a label a different width, plus the two corpus
  unions the default-label defect was found on. What the *codec* is held to.
- `crates/orbweaver-registry/tests/union_shape_from_a_peer.rs` — the four
  corpus unions with a default (`golden/06` WithDefault, `golden/29` Coded,
  Spread, Tint), each in a little-endian and a big-endian stream. What the
  *registry's derived member list* is held to: one member per label with the
  default a member of its own, in source order.

`--emit` prints the live capture as Rust constants instead of comparing, for
the day the recording has to be retaken.

Exit 0: every recording matches the live peer. 1: one does not, or the test
file no longer holds a recording. 2: omniORBpy could not produce the bytes —
unmeasured, and not reported as passing.

Clause (b) of the licensing boundary: omniORB is run as an external program and
its output is read. Nothing is linked or redistributed.
"""
import re
import subprocess
import sys
import tempfile
from pathlib import Path

from union_label_capture import pad_mask

# How an omniORB fixture leaves: see spikes/orbexit.py.
from orbexit import leave

ROOT = Path(__file__).resolve().parent.parent
CODEC_TEST = ROOT / "crates/orbweaver-giop/tests/union_default_label_from_a_peer.rs"
SHAPE_TEST = ROOT / "crates/orbweaver-registry/tests/union_shape_from_a_peer.rs"

# `udef`, not `dl`: `module dl { union DL ... }` is the case-insensitive clash
# omniidl rejects, and it was the first thing this script's author wrote.
IDL = """
module udef {
  union DL  switch (long)      { case 1: long a;   default: string b; };
  union DS  switch (short)     { case 1: short a;  default: string b; };
  union DB  switch (boolean)   { case TRUE: long yes; default: octet no; };
  union DC  switch (char)      { case 'a': long a; default: string b; };
  enum Hue { RED, GREEN, BLUE };
  union DE  switch (Hue)       { case RED: octet warm; default: string named; };
  union DLL switch (long long) { case 1: long a;   default: string b; };
};
module gc06 {
  union WithDefault switch (long) {
    case 1: long one;
    case 2:
    case 3: string two_or_three;
    default: boolean other;
  };
};
module gc29 {
  union Coded switch (long) {
    case 1: long one;
    case 2:
    default: string rest;
  };
  union Spread switch (short) {
    default:
    case 5:
    case 6: short misc;
    case 7: long seven;
  };
  enum Hue { RED, GREEN, BLUE };
  union Tint switch (Hue) {
    case RED: octet warm;
    case GREEN:
    default: string named;
  };
};
"""

# (Rust constant, module attribute, byte order flag for cdrMarshal, test file)
RECORDINGS = [
    ("LONG_DEFAULT", "udef._tc_DL", 1, CODEC_TEST),
    ("LONG_DEFAULT_BIG_ENDIAN_STREAM", "udef._tc_DL", 0, CODEC_TEST),
    ("SHORT_DEFAULT", "udef._tc_DS", 1, CODEC_TEST),
    ("BOOLEAN_DEFAULT", "udef._tc_DB", 1, CODEC_TEST),
    ("CHAR_DEFAULT", "udef._tc_DC", 1, CODEC_TEST),
    ("ENUM_DEFAULT", "udef._tc_DE", 1, CODEC_TEST),
    ("LONG_LONG_DEFAULT", "udef._tc_DLL", 1, CODEC_TEST),
    ("GOLDEN_06_WITH_DEFAULT", "gc06._tc_WithDefault", 1, CODEC_TEST),
    ("GOLDEN_29_CODED", "gc29._tc_Coded", 1, CODEC_TEST),
    ("GOLDEN_06_WITH_DEFAULT_LITTLE", "gc06._tc_WithDefault", 1, SHAPE_TEST),
    ("GOLDEN_06_WITH_DEFAULT_BIG", "gc06._tc_WithDefault", 0, SHAPE_TEST),
    ("GOLDEN_29_CODED_LITTLE", "gc29._tc_Coded", 1, SHAPE_TEST),
    ("GOLDEN_29_CODED_BIG", "gc29._tc_Coded", 0, SHAPE_TEST),
    ("GOLDEN_29_SPREAD_LITTLE", "gc29._tc_Spread", 1, SHAPE_TEST),
    ("GOLDEN_29_SPREAD_BIG", "gc29._tc_Spread", 0, SHAPE_TEST),
    ("GOLDEN_29_TINT_LITTLE", "gc29._tc_Tint", 1, SHAPE_TEST),
    ("GOLDEN_29_TINT_BIG", "gc29._tc_Tint", 0, SHAPE_TEST),
]

MARSHAL = """
import sys
sys.path.insert(0, %r)
import CORBA, udef, gc06, gc29
from omniORB import cdrMarshal
CORBA.ORB_init(["p"], CORBA.ORB_ID)
for const, attr, endian in %r:
    tc = eval(attr)
    data = cdrMarshal(CORBA._tc_TypeCode, tc, endian)
    print(const, " ".join("{:02x}".format(b) for b in data))
"""


def recorded():
    texts = {}
    out = {}
    for const, _, _, test in RECORDINGS:
        text = texts.setdefault(test, test.read_text())
        m = re.search(r"const %s: &\[u8\] = &\[(.*?)\];" % const, text, re.S)
        if not m:
            print("  FAIL %s is not in %s any more" % (const, test.name))
            return None
        out[const] = [int(x, 16) for x in re.findall(r"0x([0-9a-f]{2})", m.group(1))]
    return out


def captured(work):
    (work / "u.idl").write_text(IDL)
    r = subprocess.run(["omniidl", "-bpython", "u.idl"], cwd=work,
                       capture_output=True, text=True)
    if r.returncode != 0:
        print("  omniidl refused the IDL:", r.stderr.strip().splitlines()[-1:])
        return None
    script = MARSHAL % (str(work), [(c, a, e) for c, a, e, _ in RECORDINGS])
    r = subprocess.run([sys.executable, "-c", script], cwd=work,
                       capture_output=True, text=True)
    if r.returncode != 0:
        print("  the fixture could not marshal:", r.stderr.strip().splitlines()[-1:])
        return None
    out = {}
    for line in r.stdout.split("\n"):
        if not line.strip():
            continue
        name, rest = line.split(" ", 1)
        out[name] = [int(x, 16) for x in rest.split()]
    return out


def emit(cap):
    for const, attr, endian, test in RECORDINGS:
        b = cap[const]
        print("const %s: &[u8] = &[  // %s, %s stream -> %s"
              % (const, attr, "little-endian" if endian else "big-endian", test.name))
        for i in range(0, len(b), 16):
            print("    " + " ".join("0x%02x," % x for x in b[i:i + 16]))
        print("];")


def main():
    with tempfile.TemporaryDirectory() as tmp:
        cap = captured(Path(tmp))
    if cap is None:
        print("  SKIPPED  omniORBpy could not produce the bytes — unmeasured, not passing")
        return 2
    if "--emit" in sys.argv:
        emit(cap)
        return 0
    rec = recorded()
    if rec is None:
        return 1
    bad = 0
    for const, _, endian, _ in RECORDINGS:
        a, b = rec[const], cap.get(const)
        if b is None:
            print("  FAIL the fixture wrote nothing for %s" % const)
            bad += 1
            continue
        if len(a) != len(b):
            print("  FAIL %s: recorded %d bytes, the peer now writes %d" % (const, len(a), len(b)))
            bad += 1
            continue
        try:
            pad = pad_mask(a, little=bool(endian))
        except ValueError as e:
            print("  FAIL %s: the recording does not parse as a union TypeCode: %s" % (const, e))
            bad += 1
            continue
        diff = [i for i, (x, y) in enumerate(zip(a, b)) if x != y and i not in pad]
        if diff:
            print("  FAIL %s: the recording and the live peer differ at %r" % (const, diff[:8]))
            bad += 1
        else:
            print("  ok   %s: %d bytes, recording matches the live peer (%d padding byte(s) not compared)"
                  % (const, len(a), len(pad)))
    return 1 if bad else 0


if __name__ == "__main__":
    leave(main())
