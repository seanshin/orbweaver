#!/usr/bin/env python3
"""Re-capture a peer's union TypeCode and check the recording still matches.

`crates/orbweaver-giop/tests/union_labels_from_a_peer.rs` holds two byte
sequences omniORB wrote. A recording is only worth what it still describes, so
this regenerates them from the live fixture and compares.

Clause (b) of the licensing boundary: omniORB is run as an external program and
its output is read. Nothing is linked or redistributed.
"""
import re
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

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
        # Bytes 9..12 are padding the peer does not zero; their content is
        # undefined by the specification and differs between runs.
        diff = [i for i, (x, y) in enumerate(zip(a, b)) if x != y and not 9 <= i < 12]
        if diff:
            print("  FAIL %s: the recording and the live peer differ at %r" % (key, diff[:8]))
            bad += 1
        else:
            print("  ok   %s: %d bytes, recording matches the live peer" % (key, len(a)))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
