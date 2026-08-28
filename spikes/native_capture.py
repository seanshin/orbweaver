#!/usr/bin/env python3
"""Ask a peer what it does with `native X;` and with `ValueBase`.

Two constructs, two different kinds of answer, and the difference is the point.

**`native`: the measurement is a refusal.** `docs/PLAN.md` §4.4 defers three
constructs and a `native` is not among them, which is why the registry recorded
one as `TypeCode::ObjRef` for six phases — no rule named it, so no gate could
notice. Before choosing a representation this asks omniORB for a `tk_native`
TypeCode by every route it has, and omniORB has none:

  1. `omniidl -b dump` accepts `native Handle;` at module level, inside an
     interface, and accepts a struct member and an operation typed by it. This
     is legal IDL and a conformant front end must parse it.
  2. `omniidl -bcxx` exits 1 on the bare declaration alone:
     "Unsupported IDL construct found in input (native)".
  3. `omniidl -bpython` exits 0 with "Warning: ignoring declaration of native
     Handle" and emits nothing for it. If anything *uses* the type, the module
     it generated cannot be imported at all: the descriptor references
     `omniORB.typeMapping["IDL:.../Handle:1.0"]`, which the ignored declaration
     never registered, and the import raises `KeyError`.
  4. At runtime `CORBA.tk_native._v` is 31, and that is all it is: the ORB has
     `create_value_tc`, `create_abstract_interface_tc` and fourteen more and
     **no `create_native_tc`**, and `tcInternal.createTypeCode((tv_native, id,
     name))` raises `CORBA.INTERNAL`.

  So the ordinal is known and the parameter list is not, and there is no
  recording to hold ourselves to. `TypeCode::Native` therefore has no
  `TcKind`: `kind()` answers `None` and `encode` refuses it by name, exactly
  as `TcKind`'s own doc already said it would for 30, 31 and 33. A peer that
  sends 31 is refused symmetrically.

**`ValueBase`: the measurement is bytes.** It is the abstract base of every
valuetype, and the registry mapped it to `TypeCode::ObjRef` — so
`struct Envelope { ValueBase payload; }` generated as a reference and put an
IOR on the wire where a peer sends a value. omniORB writes the member as
TCKind 29 (`tk_value`) with ValueModifier **VM_NONE**, not VM_ABSTRACT, which
is the field a reasoned answer would have got wrong. Recorded, and compared
outside the padding by a walk of the layout rather than a list of offsets — an
encapsulation's three bytes after the byte-order flag are undefined and
omniORB does not zero them.

Recordings live in:

- `crates/orbweaver-giop/tests/native_typecode_from_a_peer.rs` — the four
  refusals above, verbatim, and what our codec does with kind 31.
- `crates/orbweaver-registry/tests/valuebase_shape_from_a_peer.rs` — the bytes
  omniORB writes for `corpus/golden/32-valuebase.idl`'s `gvb32::Envelope`, and
  the registry's derived TypeCode held to them.

`--emit` prints the live capture as Rust constants instead of comparing, for
the day a recording has to be retaken.

Exit 0: every recording matches the live peer. 1: one does not, or a test file
no longer holds a recording. 2: omniidl or omniORBpy could not be run at all —
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
from orbexit import leave

ROOT = Path(__file__).resolve().parent.parent
NATIVE_TEST = ROOT / "crates/orbweaver-giop/tests/native_typecode_from_a_peer.rs"
VALUEBASE_TEST = ROOT / "crates/orbweaver-registry/tests/valuebase_shape_from_a_peer.rs"

# The corpus files themselves, not a paraphrase: the registry side derives its
# TypeCode from these same paths, and an IDL written here to "match" them is a
# second contract that would drift. The copied-to name is not the corpus name
# because omniidl's Python back end derives a module name from the file stem
# and `32-valuebase` is not a Python identifier.
NATIVE_IDL = (ROOT / "corpus/golden/31-native-type.idl", "native_type.idl")
VALUEBASE_IDL = (ROOT / "corpus/golden/32-valuebase.idl", "valuebase.idl")

# (Rust constant, module attribute, byte order flag for cdrMarshal)
RECORDINGS = [
    ("GVB32_ENVELOPE_LITTLE", "gvb32._tc_Envelope", 1),
    ("GVB32_ENVELOPE_BIG", "gvb32._tc_Envelope", 0),
]

MARSHAL = """
import sys
sys.path.insert(0, %r)
import CORBA, gvb32
from omniORB import cdrMarshal
CORBA.ORB_init(["p"], CORBA.ORB_ID)
for const, attr, endian in %r:
    tc = eval(attr)
    data = cdrMarshal(CORBA._tc_TypeCode, tc, endian)
    print(const, tc.kind()._v, " ".join("{:02x}".format(b) for b in data))
"""

# What the runtime says when asked for a native TypeCode by every route it has.
# `tv_native` is omniORB's own descriptor tag; `create_native_tc` is the ORB
# operation CORBA 2.3 §10.7.2 defines and this ORB does not implement.
NATIVE_RUNTIME = """
import CORBA
from omniORB import tcInternal
orb = CORBA.ORB_init(["p"], CORBA.ORB_ID)
print("tk_native", CORBA.tk_native._v)
print("create_native_tc", hasattr(orb, "create_native_tc"))
print("create_value_tc", hasattr(orb, "create_value_tc"))
print("create_abstract_interface_tc", hasattr(orb, "create_abstract_interface_tc"))
try:
    tcInternal.createTypeCode((tcInternal.tv_native, "IDL:x/H:1.0", "H"))
    print("createTypeCode built")
except Exception as ex:
    print("createTypeCode", type(ex).__name__)
"""


def pad_mask(buf, little=True):
    """The byte offsets in `buf` that are padding, walked as CORBA 3.4 Part 2
    Table 9.2 lays a TypeCode out rather than listed.

    Listing offsets is how the sibling union script was green for a week
    against a fixture that padded differently; the walk is the lesson it left.
    Only the kinds these recordings contain are walked, and anything else
    raises — an unwalked kind must not silently contribute "no padding".
    """
    pad = []

    def align(pos, base, n):
        while (pos - base) % n:
            pad.append(pos)
            pos += 1
        return pos

    def u32(pos, little_here):
        return int.from_bytes(buf[pos:pos + 4], "little" if little_here else "big")

    def string(pos, base, little_here):
        pos = align(pos, base, 4)
        return pos + 4 + u32(pos, little_here)

    def typecode(pos, base, little_here):
        pos = align(pos, base, 4)
        kind = u32(pos, little_here)
        pos += 4
        if kind in (0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 23, 24, 25, 26):
            return pos
        if kind in (18, 27):  # string, wstring: a bound, inline
            return pos + 4
        if kind not in (14, 15, 22, 29, 32):
            raise ValueError("no walk for TCKind %d" % kind)
        pos = align(pos, base, 4)
        length = u32(pos, little_here)
        pos += 4
        # An encapsulation restarts alignment at its own flag and carries its
        # own byte order, which is the whole reason the big-endian recording
        # exists: omniORB writes the body little-endian inside a big-endian
        # stream.
        inner_base = pos
        inner_little = buf[pos] == 1
        pos += 1
        end = inner_base + length
        pos = body(pos, inner_base, inner_little, kind)
        if pos != end:
            raise ValueError("walked to %d, encapsulation ends at %d" % (pos, end))
        return end

    def body(pos, base, little_here, kind):
        pos = string(pos, base, little_here)  # repository id
        pos = string(pos, base, little_here)  # name
        if kind in (14, 32):  # tk_objref, tk_abstract_interface
            return pos
        if kind in (15, 22):  # tk_struct, tk_except
            pos = align(pos, base, 4)
            n = u32(pos, little_here)
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
        n = u32(pos, little_here)
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


def recorded_bytes():
    """The `ValueBase` recordings, read back out of the Rust test file."""
    if not VALUEBASE_TEST.exists():
        print("  FAIL %s does not exist" % VALUEBASE_TEST)
        return None
    text = VALUEBASE_TEST.read_text()
    out = {}
    for const, _, _ in RECORDINGS:
        m = re.search(r"const %s: &\[u8\] = &\[(.*?)\];" % const, text, re.S)
        if not m:
            print("  FAIL %s is not in %s any more" % (const, VALUEBASE_TEST.name))
            return None
        out[const] = [int(x, 16) for x in re.findall(r"0x([0-9a-f]{2})", m.group(1))]
    return out


def recorded_refusals():
    """The strings the native probes are held to, read out of the Rust file.

    They are held as `const` items rather than assertions inside a test body so
    that this script and the test read the *same* text: a refusal paraphrased
    in one of the two places is a recording that has quietly stopped being one.
    """
    if not NATIVE_TEST.exists():
        print("  FAIL %s does not exist" % NATIVE_TEST)
        return None
    text = NATIVE_TEST.read_text()
    out = {}
    for const in ("CXX_REFUSAL", "PYTHON_WARNING", "PYTHON_IMPORT_ERROR"):
        m = re.search(r'const %s: &str = "(.*?)";' % const, text, re.S)
        if not m:
            print("  FAIL %s is not in %s any more" % (const, NATIVE_TEST.name))
            return None
        out[const] = m.group(1)
    return out


def run(args, cwd):
    return subprocess.run(args, cwd=cwd, capture_output=True, text=True)


def probe_native(work):
    """Every route to a native TypeCode, and what omniORB answers."""
    src, as_name = NATIVE_IDL
    shutil.copy(src, work / as_name)
    out = {}

    r = run(["omniidl", "-b", "dump", as_name], work)
    out["dump_exit"] = r.returncode

    r = run(["omniidl", "-bcxx", as_name], work)
    out["cxx_exit"] = r.returncode
    out["cxx_text"] = (r.stdout + r.stderr)

    r = run(["omniidl", "-bpython", as_name], work)
    out["python_exit"] = r.returncode
    out["python_text"] = (r.stdout + r.stderr)

    # The generated package imports only if nothing uses the ignored native.
    # `31-native-type.idl` uses it in five declarations, so this is the case
    # that matters: the module omniidl wrote cannot be loaded at all.
    r = run([sys.executable, "-c",
             "import sys; sys.path.insert(0, '.')\n"
             "import CORBA\n"
             "try:\n"
             "    import gn31\n"
             "    print('IMPORTED')\n"
             "except Exception as ex:\n"
             "    print(type(ex).__name__, ex)\n"], work)
    out["import_text"] = r.stdout.strip()

    r = run([sys.executable, "-c", NATIVE_RUNTIME], work)
    if r.returncode != 0:
        return None
    for line in r.stdout.strip().split("\n"):
        k, v = line.split(" ", 1)
        out["rt_" + k] = v
    return out


def check_native(work, bad):
    rec = recorded_refusals()
    if rec is None:
        return bad + 1
    got = probe_native(work)
    if got is None:
        print("  FAIL the omniORB runtime probe did not run")
        return bad + 1

    def want(label, cond, detail):
        nonlocal bad
        if cond:
            print("  ok   %s" % label)
        else:
            print("  FAIL %s: %s" % (label, detail))
            bad += 1

    want("the front end accepts `native Handle;` and every use of it",
         got["dump_exit"] == 0, "omniidl -b dump exited %d" % got["dump_exit"])
    want("the C++ back end refuses the declaration",
         got["cxx_exit"] == 1 and rec["CXX_REFUSAL"] in got["cxx_text"],
         "exit %d, text %r" % (got["cxx_exit"], got["cxx_text"][:200]))
    want("the Python back end ignores the declaration",
         got["python_exit"] == 0 and rec["PYTHON_WARNING"] in got["python_text"],
         "exit %d, text %r" % (got["python_exit"], got["python_text"][:200]))
    want("what it generated cannot be imported",
         rec["PYTHON_IMPORT_ERROR"] in got["import_text"],
         "the import said %r" % got["import_text"])
    want("the runtime names 31 and cannot build one",
         got.get("rt_tk_native") == "31"
         and got.get("rt_create_native_tc") == "False"
         and got.get("rt_create_value_tc") == "True"
         and got.get("rt_create_abstract_interface_tc") == "True"
         and got.get("rt_createTypeCode") == "INTERNAL",
         "the runtime answered %r" % {k: v for k, v in got.items() if k.startswith("rt_")})
    return bad


def capture_valuebase(work):
    src, as_name = VALUEBASE_IDL
    shutil.copy(src, work / as_name)
    r = run(["omniidl", "-bpython", as_name], work)
    if r.returncode != 0:
        print("  omniidl refused %s:" % src.name, r.stderr.strip().splitlines()[-1:])
        return None
    script = MARSHAL % (str(work), RECORDINGS)
    r = run([sys.executable, "-c", script], work)
    if r.returncode != 0:
        print("  the fixture could not marshal:", r.stderr.strip().splitlines()[-1:])
        return None
    out, kinds = {}, {}
    for line in r.stdout.split("\n"):
        if not line.strip():
            continue
        name, kind, rest = line.split(" ", 2)
        kinds[name] = int(kind)
        out[name] = [int(x, 16) for x in rest.split()]
    return out, kinds


def check_valuebase(cap, kinds, bad):
    rec = recorded_bytes()
    if rec is None:
        return bad + 1
    # `Envelope` is a struct; what this file is about is its member, so the
    # ordinal asserted is the member's, read out of the recording by the same
    # walk that finds the padding.
    for const, _, endian in RECORDINGS:
        a, b = rec[const], cap.get(const)
        if b is None:
            print("  FAIL the fixture wrote nothing for %s" % const)
            bad += 1
            continue
        if len(a) != len(b):
            print("  FAIL %s: recorded %d bytes, the peer now writes %d"
                  % (const, len(a), len(b)))
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
    return bad


def emit(cap, kinds):
    for const, attr, endian in RECORDINGS:
        b = cap[const]
        print("/// `%s`, TCKind %d, %s stream"
              % (attr, kinds[const], "little-endian" if endian else "big-endian"))
        print("const %s: &[u8] = &[" % const)
        for i in range(0, len(b), 16):
            print("    " + " ".join("0x%02x," % x for x in b[i:i + 16]))
        print("];")


def main():
    if shutil.which("omniidl") is None:
        print("  SKIPPED  omniidl is not installed — unmeasured, not passing")
        return 2
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        got = capture_valuebase(work)
        if got is None:
            print("  SKIPPED  omniORBpy could not produce the bytes — unmeasured, not passing")
            return 2
        cap, kinds = got
        if "--emit" in sys.argv:
            emit(cap, kinds)
            return 0
        bad = 0
        print("native — the measurement is a refusal:")
        bad = check_native(work, bad)
        print("ValueBase — the measurement is bytes:")
        bad = check_valuebase(cap, kinds, bad)
    return 1 if bad else 0


if __name__ == "__main__":
    leave(main())
