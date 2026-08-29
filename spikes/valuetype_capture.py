#!/usr/bin/env python3
"""Re-capture a peer's `valuetype` and `abstract interface` TypeCodes.

`docs/PLAN.md` §4.4 defers both from the v1 wire, and until 2026-08-20 the
registry recorded both as `TypeCode::ObjRef` — so both emitters emitted an
*object reference* for them and skipped nothing. Nothing here was read off the
specification's table: the question this script answers is what a conformant
peer actually writes, and it is asked of the peer.

Two test files hold what omniORB wrote for the constructs in
`corpus/golden/20-deferred-valuetype.idl` and `corpus/golden/deferred-reach.idl`,
and this regenerates all of them from the live fixture and compares, padding
excluded (an encapsulation's three bytes after the byte-order flag are
undefined and omniORB does not zero them: the same TypeCode came back
`01 38 a3 05` in one run and `01 c5 01 0a` in the next):

- `crates/orbweaver-giop/tests/valuetype_typecode_from_a_peer.rs` — what the
  *codec* is held to: `tk_value` is 29 and `tk_abstract_interface` is 32, with
  the parameter lists the peer wrote.
- `crates/orbweaver-registry/tests/valuetype_shape_from_a_peer.rs` — what the
  *registry's derived TypeCode* is held to: the same IDL, loaded here, must
  produce the shape omniORB describes.

`--emit` prints the live capture as Rust constants instead of comparing, for
the day the recording has to be retaken.

Exit 0: every recording matches the live peer. 1: one does not, or the test
file no longer holds a recording. 2: omniORBpy could not produce the bytes —
unmeasured, and not reported as passing.

Clause (b) of the licensing boundary: omniORB is run as an external program and
its output is read. Nothing is linked or redistributed.
"""
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# How an omniORB fixture leaves: see spikes/orbexit.py.
from orbexit import leave, wrap_child

ROOT = Path(__file__).resolve().parent.parent
CODEC_TEST = ROOT / "crates/orbweaver-giop/tests/valuetype_typecode_from_a_peer.rs"
SHAPE_TEST = ROOT / "crates/orbweaver-registry/tests/valuetype_shape_from_a_peer.rs"

# The corpus files themselves, not a paraphrase of them: the registry side of
# this pair derives its TypeCode from these same paths, and an IDL written here
# to "match" them is a second contract that would drift.
# The copied-to name is not the corpus name: omniidl's Python back end derives
# a module name from the file stem, and `20-deferred-valuetype_idl` is not a
# Python identifier — the generated package imports it and the interpreter
# answers "invalid decimal literal". A stem the back end can name, over the
# corpus file's own bytes.
IDL_FILES = [
    (ROOT / "corpus/golden/20-deferred-valuetype.idl", "deferred_valuetype.idl"),
    (ROOT / "corpus/golden/deferred-reach.idl", "deferred_reach.idl"),
]

# (Rust constant, module attribute, byte order flag for cdrMarshal, test file)
RECORDINGS = [
    ("GC20_MONEY", "gc20._tc_Money", 1, CODEC_TEST),
    ("GC20_MONEY_BIG_ENDIAN_STREAM", "gc20._tc_Money", 0, CODEC_TEST),
    ("GC20_NAMED", "gc20._tc_Named", 1, CODEC_TEST),
    ("GC20_DESCRIBABLE", "gc20._tc_Describable", 1, CODEC_TEST),
    ("GC20_DESCRIBABLE_BIG_ENDIAN_STREAM", "gc20._tc_Describable", 0, CODEC_TEST),
    ("GCDR_MEMO", "gcdr._tc_Memo", 1, CODEC_TEST),
    ("GCDR_TAGGED", "gcdr._tc_Tagged", 1, CODEC_TEST),
    ("GC20_MONEY_LITTLE", "gc20._tc_Money", 1, SHAPE_TEST),
    ("GC20_MONEY_BIG", "gc20._tc_Money", 0, SHAPE_TEST),
    ("GC20_NAMED_LITTLE", "gc20._tc_Named", 1, SHAPE_TEST),
    ("GC20_NAMED_BIG", "gc20._tc_Named", 0, SHAPE_TEST),
    # The abstract interface reaches the registry side through a struct that
    # holds one, not on its own: an interface's entry is an `Entry::Interface`
    # and has no `TypeCode`, so `gcdr::Tagged` is where the registry's answer
    # for an abstract interface is observable at all.
    ("GCDR_TAGGED_LITTLE", "gcdr._tc_Tagged", 1, SHAPE_TEST),
    ("GCDR_TAGGED_BIG", "gcdr._tc_Tagged", 0, SHAPE_TEST),
    ("GCDR_MEMO_LITTLE", "gcdr._tc_Memo", 1, SHAPE_TEST),
    ("GCDR_MEMO_BIG", "gcdr._tc_Memo", 0, SHAPE_TEST),
]

MARSHAL = """
import sys
sys.path.insert(0, %r)
import CORBA, gc20, gcdr
from omniORB import cdrMarshal
CORBA.ORB_init(["p"], CORBA.ORB_ID)
for const, attr, endian in %r:
    tc = eval(attr)
    data = cdrMarshal(CORBA._tc_TypeCode, tc, endian)
    print(const, tc.kind()._v, " ".join("{:02x}".format(b) for b in data))
"""


def pad_mask(buf, little=True):
    """The byte offsets in `buf` that are padding, walked as Table 9.2 lays a
    TypeCode out rather than listed.

    Listing offsets is how the sibling union script was green for a week
    against a fixture that padded differently; the walk is the lesson it left.
    Only the kinds these recordings contain are walked, and anything else
    raises — an unwalked kind must not silently contribute "no padding".
    """
    pad = []

    def u32(pos, base):
        return int.from_bytes(buf[pos:pos + 4], "little" if little else "big"), pos + 4

    def align(pos, base, n):
        while (pos - base) % n:
            pad.append(pos)
            pos += 1
        return pos

    def string(pos, base, little_here):
        pos = align(pos, base, 4)
        n = int.from_bytes(buf[pos:pos + 4], "little" if little_here else "big")
        return pos + 4 + n

    def typecode(pos, base, little_here):
        pos = align(pos, base, 4)
        kind = int.from_bytes(buf[pos:pos + 4], "little" if little_here else "big")
        pos += 4
        if kind in (0, 1, 3, 8, 10, 2, 4, 5, 6, 7, 9, 11, 12, 13, 23, 24, 25, 26):
            return pos
        if kind in (18, 27):  # string, wstring: a bound, inline
            return pos + 4
        if kind not in (14, 15, 22, 29, 32):
            raise ValueError("no walk for TCKind %d" % kind)
        pos = align(pos, base, 4)
        length = int.from_bytes(buf[pos:pos + 4], "little" if little_here else "big")
        pos += 4
        # An encapsulation restarts alignment at its own flag and carries its
        # own byte order, which is the whole reason the big-endian recordings
        # exist: omniORB writes the body little-endian inside a big-endian
        # stream.
        inner_base = pos
        inner_little = buf[pos] == 1
        pos += 1
        end = inner_base + length
        pos = inner_string_body(pos, inner_base, inner_little, kind, end)
        if pos != end:
            raise ValueError("walked to %d, encapsulation ends at %d" % (pos, end))
        return end

    def inner_string_body(pos, base, little_here, kind, end):
        pos = string(pos, base, little_here)  # repository id
        pos = string(pos, base, little_here)  # name
        if kind in (14, 32):  # tk_objref, tk_abstract_interface
            return pos
        if kind in (15, 22):  # tk_struct, tk_except
            pos = align(pos, base, 4)
            n = int.from_bytes(buf[pos:pos + 4], "little" if little_here else "big")
            pos += 4
            for _ in range(n):
                pos = string(pos, base, little_here)
                pos = typecode(pos, base, little_here)
            return pos
        # tk_value
        pos = align(pos, base, 2)
        pos += 2  # ValueModifier
        pos = typecode(pos, base, little_here)  # concrete base, tk_null if none
        pos = align(pos, base, 4)
        n = int.from_bytes(buf[pos:pos + 4], "little" if little_here else "big")
        pos += 4
        for _ in range(n):
            pos = string(pos, base, little_here)
            pos = typecode(pos, base, little_here)
            pos = align(pos, base, 2)
            pos += 2  # Visibility
        return pos

    end = typecode(0, 0, little)
    if end != len(buf):
        raise ValueError("walked %d of %d bytes" % (end, len(buf)))
    return set(pad)


def recorded():
    texts = {}
    out = {}
    for const, _, _, test in RECORDINGS:
        if not test.exists():
            print("  FAIL %s does not exist" % test)
            return None
        text = texts.setdefault(test, test.read_text())
        m = re.search(r"const %s: &\[u8\] = &\[(.*?)\];" % const, text, re.S)
        if not m:
            print("  FAIL %s is not in %s any more" % (const, test.name))
            return None
        out[const] = [int(x, 16) for x in re.findall(r"0x([0-9a-f]{2})", m.group(1))]
    return out


def captured(work):
    for src, as_name in IDL_FILES:
        shutil.copy(src, work / as_name)
    for src, as_name in IDL_FILES:
        r = subprocess.run(["omniidl", "-bpython", as_name], cwd=work,
                           capture_output=True, text=True)
        if r.returncode != 0:
            print("  omniidl refused %s:" % src.name, r.stderr.strip().splitlines()[-1:])
            return None
    script = MARSHAL % (str(work), [(c, a, e) for c, a, e, _ in RECORDINGS])
    r = subprocess.run([sys.executable, "-c", wrap_child(script)], cwd=work,
                       capture_output=True, text=True)
    if r.returncode != 0:
        print("  the fixture could not marshal:", r.stderr.strip().splitlines()[-1:])
        return None
    out = {}
    kinds = {}
    for line in r.stdout.split("\n"):
        if not line.strip():
            continue
        name, kind, rest = line.split(" ", 2)
        kinds[name] = int(kind)
        out[name] = [int(x, 16) for x in rest.split()]
    return out, kinds


def emit(cap, kinds):
    for const, attr, endian, test in RECORDINGS:
        b = cap[const]
        print("/// `%s`, TCKind %d, %s stream -> %s"
              % (attr, kinds[const], "little-endian" if endian else "big-endian", test.name))
        print("const %s: &[u8] = &[" % const)
        for i in range(0, len(b), 16):
            print("    " + " ".join("0x%02x," % x for x in b[i:i + 16]))
        print("];")


def main():
    with tempfile.TemporaryDirectory() as tmp:
        got = captured(Path(tmp))
    if got is None:
        print("  SKIPPED  omniORBpy could not produce the bytes — unmeasured, not passing")
        return 2
    cap, kinds = got
    if "--emit" in sys.argv:
        emit(cap, kinds)
        return 0
    rec = recorded()
    if rec is None:
        return 1
    bad = 0
    # The two ordinals the whole batch rests on, asserted rather than assumed.
    for const, want in (("GC20_MONEY", 29), ("GC20_DESCRIBABLE", 32)):
        if kinds.get(const) != want:
            print("  FAIL the peer now gives %s TCKind %r, not %d"
                  % (const, kinds.get(const), want))
            bad += 1
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
            print("  FAIL %s: the recording does not parse as a TypeCode: %s" % (const, e))
            bad += 1
            continue
        diff = [i for i, (x, y) in enumerate(zip(a, b)) if x != y and i not in pad]
        if diff:
            print("  FAIL %s: the recording and the live peer differ at %r" % (const, diff[:8]))
            bad += 1
        else:
            print("  ok   %s: %d bytes, recording matches the live peer "
                  "(%d padding byte(s) not compared)" % (const, len(a), len(pad)))
    return 1 if bad else 0


if __name__ == "__main__":
    leave(main())
