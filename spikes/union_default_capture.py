#!/usr/bin/env python3
"""Re-capture a peer's `default:`-bearing union TypeCodes and check the recording.

`crates/orbweaver-giop/tests/union_default_label_from_a_peer.rs` holds byte
sequences omniORB wrote for unions with a `default:` branch — one per
discriminator kind that gives a label a different width, plus the two corpus
unions the defect was found on. This regenerates them from the live fixture
and compares, padding excluded (`pad_mask` from `union_label_capture.py`, the
sibling that recorded the labelled cases and learned not to list offsets).

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

ROOT = Path(__file__).resolve().parent.parent
TEST = ROOT / "crates/orbweaver-giop/tests/union_default_label_from_a_peer.rs"

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
};
"""

# (Rust constant, module attribute, byte order flag for cdrMarshal)
RECORDINGS = [
    ("LONG_DEFAULT", "udef._tc_DL", 1),
    ("LONG_DEFAULT_BIG_ENDIAN_STREAM", "udef._tc_DL", 0),
    ("SHORT_DEFAULT", "udef._tc_DS", 1),
    ("BOOLEAN_DEFAULT", "udef._tc_DB", 1),
    ("CHAR_DEFAULT", "udef._tc_DC", 1),
    ("ENUM_DEFAULT", "udef._tc_DE", 1),
    ("LONG_LONG_DEFAULT", "udef._tc_DLL", 1),
    ("GOLDEN_06_WITH_DEFAULT", "gc06._tc_WithDefault", 1),
    ("GOLDEN_29_CODED", "gc29._tc_Coded", 1),
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
    text = TEST.read_text()
    out = {}
    for const, _, _ in RECORDINGS:
        m = re.search(r"const %s: &\[u8\] = &\[(.*?)\];" % const, text, re.S)
        if not m:
            print("  FAIL %s is not in %s any more" % (const, TEST.name))
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
    script = MARSHAL % (str(work), RECORDINGS)
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
    for const, attr, endian in RECORDINGS:
        b = cap[const]
        print("const %s: &[u8] = &[  // %s, %s stream" % (const, attr, "little-endian" if endian else "big-endian"))
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
    for const, _, endian in RECORDINGS:
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
    sys.exit(main())
