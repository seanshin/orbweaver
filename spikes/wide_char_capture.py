#!/usr/bin/env python3
"""Re-capture a peer's wide-character bytes and check the recording still matches.

`crates/orbweaver-giop/tests/wide_chars_from_a_peer.rs` holds five byte
sequences omniORB wrote and a table of six bodies it read. A recording is only
worth what it still describes, so this regenerates both from the live fixture
and compares.

Clause (b) of the licensing boundary: omniORB is run as an external program and
its output is read. Nothing is linked or redistributed.
"""
import re
import subprocess
import sys
from pathlib import Path

# How an omniORB fixture leaves: see spikes/orbexit.py.
from orbexit import leave, wrap_child

ROOT = Path(__file__).resolve().parent.parent
TEST = ROOT / "crates/orbweaver-giop/tests/wide_chars_from_a_peer.rs"

# What the peer writes: (rust const name, TypeCode, value, little-endian?)
WRITES = [
    ("WCHAR_SEQ_BIG", "CORBA._tc_WCharSeq", "[u'w', u'A', u'\\ud55c']", False),
    ("WCHAR_SEQ_LITTLE", "CORBA._tc_WCharSeq", "[u'w', u'A', u'\\ud55c']", True),
    ("WSTRING_BIG", "CORBA._tc_wstring", "u'wA'", False),
    ("WSTRING_LITTLE", "CORBA._tc_wstring", "u'wA'", True),
    ("WSTRING_EMPTY", "CORBA._tc_wstring", "u''", False),
]

# What the peer reads: a wstring body, and the code point it must come back as
# in *both* stream orders. This is the half omniORB's own writer can never
# produce, because it always emits a byte-order mark.
READS = [
    ("0077", "0077"),
    ("7700", "7700"),
    ("feff0077", "0077"),
    ("fffe7700", "0077"),
    ("feff7700", "7700"),
    ("fffe0077", "7700"),
]

SCRIPT = r"""
import CORBA
from omniORB import cdrMarshal, cdrUnmarshal
CORBA.ORB_init(["p"], CORBA.ORB_ID)
for name, tc, value, little in %(writes)r:
    b = cdrMarshal(eval(tc), eval(value), little)
    print("W", name, "".join("%%02x" %% x for x in b))
for body_hex, _ in %(reads)r:
    body = bytes.fromhex(body_hex)
    for little in (False, True):
        n = len(body).to_bytes(4, "little" if little else "big")
        v = cdrUnmarshal(CORBA._tc_wstring, n + body, little)
        print("R", body_hex, "LE" if little else "BE", "%%04x" %% ord(v))
"""


def recorded():
    text = TEST.read_text()
    out = {}
    for name, _, _, _ in WRITES:
        m = re.search(r"const %s: &\[u8\] = *\n? *&\[(.*?)\];" % name, text, re.S)
        if not m:
            print("  FAIL %s is not in %s any more" % (name, TEST.name))
            return None
        out[name] = "".join(re.findall(r"0x([0-9a-f]{2})", m.group(1)))
    return out


def main():
    rec = recorded()
    if rec is None:
        return 1
    script = SCRIPT % {"writes": WRITES, "reads": READS}
    r = subprocess.run([sys.executable, "-c", wrap_child(script)], capture_output=True, text=True)
    if r.returncode != 0:
        tail = r.stderr.strip().splitlines()[-1:]
        print("  the fixture could not marshal:", tail)
        print("  SKIPPED  omniORBpy could not produce the bytes — unmeasured, not passing")
        return 2

    bad = 0
    reads = dict((h, cp) for h, cp in READS)
    for line in r.stdout.split("\n"):
        f = line.split()
        if not f:
            continue
        if f[0] == "W":
            name, got = f[1], f[2]
            if rec[name] != got:
                print("  FAIL %s: recorded %s, the peer now writes %s" % (name, rec[name], got))
                bad += 1
            else:
                print("  ok   %s writes %s" % (name, got))
        elif f[0] == "R":
            body, order, got = f[1], f[2], f[3]
            if reads[body] != got:
                print("  FAIL peer reads %s (%s) as U+%s, the test claims U+%s"
                      % (body, order, got, reads[body]))
                bad += 1
            else:
                print("  ok   %s (%s) reads U+%s" % (body, order, got))
    return 1 if bad else 0


if __name__ == "__main__":
    leave(main())
