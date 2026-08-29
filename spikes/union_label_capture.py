#!/usr/bin/env python3
"""Re-capture a peer's union TypeCode and check the recording still matches.

`crates/orbweaver-giop/tests/union_labels_from_a_peer.rs` holds two byte
sequences omniORB wrote. A recording is only worth what it still describes, so
this regenerates them from the live fixture and compares.

The comparison skips every padding byte, and it finds them by walking the
encapsulation (`pad_mask` below) rather than by listing offsets. The first
version listed `9..12` — the three bytes after the byte-order flag, the only
padding the local omniORB happened to leave non-zero — and was green here for
a week while CI's omniORB, built from source on Linux, left different garbage
after the repository-id string and before every 8-aligned label. Ten runs red
on bytes the specification says nothing about. CLAUDE.md's wire rule ("compare
decoded values, never raw buffers") applies to the harness's own scripts too.

Clause (b) of the licensing boundary: omniORB is run as an external program and
its output is read. Nothing is linked or redistributed.
"""
import re
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

# How an omniORB fixture leaves: see spikes/orbexit.py.
from orbexit import leave, wrap_child

ROOT = Path(__file__).resolve().parent.parent
TEST = ROOT / "crates/orbweaver-giop/tests/union_labels_from_a_peer.rs"

IDL = """
module ut {
  union U switch (long) { case 1: long as_long; case 2: string as_text; };
};
module ut2 {
  union W switch (long long) {
    case 1: long a; case 2: string b; case 3: double c;
  };
};
"""

MARSHAL = """
import sys
sys.path.insert(0, %r)
import CORBA, ut, ut2
from omniORB import cdrMarshal
CORBA.ORB_init(["p"], CORBA.ORB_ID)
for name, tc, v in [("U", ut._tc_U, ut.U(as_long=7)),
                    ("W", ut2._tc_W, ut2.W(a=5))]:
    data = cdrMarshal(CORBA._tc_any, CORBA.Any(tc, v))
    body = data[4:]                       # a byte-order flag and three pad bytes
    n = 8 + struct.unpack_from("<I", body, 4)[0]
    print(name, " ".join("{:02x}".format(b) for b in body[:n]))
"""


# ── the padding mask ────────────────────────────────────────────────────────
#
# A minimal CDR walk over a TypeCode as CORBA 3.4 §9.4.2 lays it out, recording
# every byte that is alignment padding. Simple kinds are a bare `kind`;
# `tk_string`/`tk_wstring` add a bound; the constructed kinds carry an
# encapsulation whose alignment restarts at its own first byte (the origin rule
# in CLAUDE.md). Only what the two recordings need is implemented, and anything
# else raises rather than guessing — a mask that silently treats a byte it does
# not understand as padding would be the gate this script exists to not be.

TK_SIMPLE = {0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 23, 24, 25, 26}
TK_STRING, TK_WSTRING = 18, 27
TK_STRUCT, TK_UNION, TK_ENUM, TK_SEQUENCE, TK_ALIAS, TK_EXCEPT = 15, 16, 17, 19, 21, 22
LABEL_WIDTH = {2: 2, 3: 4, 4: 2, 5: 4, 8: 1, 9: 1, 17: 4, 23: 8, 24: 8}


class _Walk:
    def __init__(self, buf, base, little):
        self.buf, self.base, self.little, self.pos, self.pad = buf, base, little, 0, set()

    def align(self, n):
        while self.pos % n:
            self.pad.add(self.base + self.pos)
            self.pos += 1

    def take(self, n):
        if self.pos + n > len(self.buf):
            raise ValueError("ran off the end at %d" % (self.base + self.pos))
        v = bytes(self.buf[self.pos:self.pos + n])
        self.pos += n
        return v

    def u(self, n):
        self.align(n)
        return int.from_bytes(self.take(n), "little" if self.little else "big")

    def string(self):
        n = self.u(4)
        self.take(n)

    def typecode(self):
        kind = self.u(4)
        if kind in TK_SIMPLE:
            return kind
        if kind in (TK_STRING, TK_WSTRING):
            self.u(4)
            return kind
        if kind not in (TK_STRUCT, TK_UNION, TK_ENUM, TK_SEQUENCE, TK_ALIAS, TK_EXCEPT):
            raise ValueError("TypeCode kind %d is not walked here" % kind)
        n = self.u(4)
        start = self.pos
        inner = _Walk(self.buf[start:start + n], self.base + start, self.little)
        inner.encapsulated(kind)
        self.pad |= inner.pad
        self.pos = start + n
        return kind

    def encapsulated(self, kind):
        flag = self.take(1)[0]
        self.little = flag == 1
        self.align(4)                    # the three bytes after the flag
        if kind == TK_SEQUENCE:
            self.typecode(); self.u(4); return
        self.string()                    # repository id
        self.string()                    # name
        if kind == TK_ALIAS:
            self.typecode(); return
        if kind == TK_ENUM:
            for _ in range(self.u(4)):
                self.string()
            return
        if kind in (TK_STRUCT, TK_EXCEPT):
            for _ in range(self.u(4)):
                self.string(); self.typecode()
            return
        disc = self.typecode()
        if disc not in LABEL_WIDTH:
            raise ValueError("union discriminator kind %d has no label width here" % disc)
        self.u(4)                        # default index
        for _ in range(self.u(4)):
            self.u(LABEL_WIDTH[disc])    # the label, aligned to its own width
            self.string()
            self.typecode()


def pad_mask(buf, little=True):
    """Offsets in `buf` (a TypeCode starting at its kind) that are padding.

    `little` is the byte order of the *stream* the kind and encapsulation
    length sit in; the encapsulation carries its own flag. omniORB writes the
    body little-endian on this host whichever order the stream is, so a
    big-endian capture is the one case where the two differ.
    """
    w = _Walk(buf, 0, little)
    w.typecode()
    if w.pos != len(buf):
        raise ValueError("walked %d of %d bytes" % (w.pos, len(buf)))
    return w.pad


def recorded():
    text = TEST.read_text()
    out = {}
    for const, key in [("LONG_DISCRIMINATED", "U"), ("LONG_LONG_DISCRIMINATED", "W")]:
        m = re.search(r"const %s: &\[u8\] = &\[(.*?)\];" % const, text, re.S)
        if not m:
            print("  FAIL %s is not in %s any more" % (const, TEST.name))
            return None
        out[key] = [int(x, 16) for x in re.findall(r"0x([0-9a-f]{2})", m.group(1))]
    return out


def captured(work):
    (work / "u.idl").write_text(IDL)
    subprocess.run(["omniidl", "-bpython", "u.idl"], cwd=work, check=True,
                   stdout=subprocess.DEVNULL)
    script = "import struct\n" + MARSHAL % str(work)
    r = subprocess.run([sys.executable, "-c", wrap_child(script)], cwd=work,
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


def main():
    rec = recorded()
    if rec is None:
        return 1
    with tempfile.TemporaryDirectory() as tmp:
        cap = captured(Path(tmp))
    if cap is None:
        print("  SKIPPED  omniORBpy could not produce the bytes — unmeasured, not passing")
        return 2
    bad = 0
    for key in sorted(rec):
        a, b = rec[key], cap.get(key)
        if b is None:
            print("  FAIL the fixture wrote nothing for %s" % key)
            bad += 1
            continue
        if len(a) != len(b):
            print("  FAIL %s: recorded %d bytes, the peer now writes %d" % (key, len(a), len(b)))
            bad += 1
            continue
        # Padding content is undefined by the specification and the peer does
        # not zero it; every other byte must match. The mask is derived from
        # the layout, never from a list of offsets someone saw differ once.
        try:
            pad = pad_mask(a)
        except ValueError as e:
            print("  FAIL %s: the recording does not parse as a union TypeCode: %s" % (key, e))
            bad += 1
            continue
        diff = [i for i, (x, y) in enumerate(zip(a, b)) if x != y and i not in pad]
        if diff:
            print("  FAIL %s: the recording and the live peer differ at %r" % (key, diff[:8]))
            bad += 1
        else:
            print("  ok   %s: %d bytes, recording matches the live peer (%d padding byte(s) not compared)"
                  % (key, len(a), len(pad)))
    return 1 if bad else 0


if __name__ == "__main__":
    leave(main())
