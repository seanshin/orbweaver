#!/usr/bin/env python3
"""What does each peer actually advertise for `char`?  (D009 §8, batch 4)

D009 conditions a non-empty `char` conversion list in
`codeset::server_component_info()` on producing **a peer advertising
ISO-8859-1 without UTF-8 in its conversion list**. This probe is that
condition, executed rather than reasoned about: it starts each installed
reference ORB under every configuration that could plausibly move its
`TAG_CODE_SETS` declaration, decodes the component out of the IOR the peer
produced, and prints the `char` native set with its conversion list.

TEST FIXTURE ONLY. omniORB is LGPL/GPL and JacORB is LGPL. Nothing here is
linked into Orbweaver: the peers are separate processes, and `catior` is an
external program whose text output we read as a cross-check. See docs/PLAN.md
§10 and CLAUDE.md's licensing boundary.

    python3 spikes/codeset_peer_probe.py             # sweep, table, verdict
    python3 spikes/codeset_peer_probe.py --ior IOR:… # decode one IOR and stop

The exit status is about the **premise**, not about the capability:

  0  every peer measured can reach UTF-8, so an empty conversion list is
     still the honest declaration and D009 batch 4 stays BLOCKED;
  1  a peer advertises ISO-8859-1 without UTF-8 — the empty list now costs a
     real peer, and batch 4 is unblocked;
  2  something could not be measured (no peer installed, a peer that would not
     start). An unmeasured check is a failure, never a pass.

*피어가 실제로 무엇을 광고하는지 재는 탐침. UTF-8에 닿지 못하는 피어를 만들어
내지 못하면 변환 목록은 비어 있는 채로 남고, 그 사실 자체가 이 배치의 결과다.*
"""

import os
import pathlib
import shutil
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent

# OSF registry ids, from CORBA 3.4 Part 2 §7.10.2 and the OSF registry.
NAMES = {
    0x00010001: "ISO-8859-1",
    0x0001000F: "ISO-8859-15",
    0x00010020: "US-ASCII",
    0x00010100: "UCS-2",
    0x00010109: "UTF-16",
    0x00030010: "UCS-4",
    0x00040002: "EUC-KR",
    0x05010001: "UTF-8",
    0x10020025: "windows-1252",
}
TAG_CODE_SETS = 1
UTF_8 = 0x05010001
ISO_8859_1 = 0x00010001


def cs_name(cs):
    return NAMES.get(cs, "unregistered")


class Reader:
    """Just enough CDR to walk an IOR.

    Self-contained on purpose: reading a peer's advertisement with that peer's
    own library would measure the library's agreement with itself. `catior`
    still gets to disagree with this — see `cross_check`.

    Every reader here is over a CDR **encapsulation**, whose first byte is the
    byte-order flag and whose alignment origin is that same byte — so `buf`
    includes the flag and `pos` starts at 1. Dropping the flag and starting at
    zero is the off-by-one that makes the first `u32` read three pad bytes and
    one real one; it does not fail loudly, it returns a plausible number.
    """

    def __init__(self, buf):
        self.buf, self.little, self.pos = buf, buf[0] == 1, 1

    def align(self, n):
        pad = self.pos % n
        if pad:
            self.pos += n - pad

    def octets(self, n):
        b = self.buf[self.pos : self.pos + n]
        if len(b) != n:
            raise ValueError("truncated IOR")
        self.pos += n
        return b

    def u32(self):
        self.align(4)
        return int.from_bytes(self.octets(4), "little" if self.little else "big")

    def u16(self):
        self.align(2)
        return int.from_bytes(self.octets(2), "little" if self.little else "big")

    def string(self):
        return self.octets(self.u32())[:-1].decode("latin-1")

    def sequence(self):
        return self.octets(self.u32())

    def encapsulation(self):
        # An encapsulation restarts alignment at its own first byte, which is
        # why this returns a new reader over the body rather than seeking.
        return Reader(self.sequence())


def parse_ior(text):
    """-> [((char_native, [conv]), (wchar_native, [conv])), …], one per profile."""
    text = text.strip()
    if not text.startswith("IOR:"):
        raise ValueError("not a stringified IOR")
    d = Reader(bytes.fromhex(text[4:]))
    d.string()  # type id
    found = []
    for _ in range(d.u32()):  # profiles
        tag = d.u32()
        prof = d.encapsulation()
        if tag != 0:  # TAG_INTERNET_IOP
            continue
        prof.octets(2)  # IIOP version major, minor
        prof.string()  # host
        prof.u16()  # port
        prof.sequence()  # object key
        if prof.pos >= len(prof.buf):
            continue  # IIOP 1.0: no component sequence at all
        for _ in range(prof.u32()):
            ctag = prof.u32()
            body = prof.encapsulation()
            if ctag != TAG_CODE_SETS:
                continue
            comps = []
            for _ in range(2):  # for_char, then for_wchar
                native = body.u32()
                comps.append((native, [body.u32() for _ in range(body.u32())]))
            found.append(tuple(comps))
    return found


def describe(component):
    native, conv = component
    convs = ", ".join(f"{cs_name(c)} (0x{c:08X})" for c in conv) or "(empty)"
    return f"native {cs_name(native)} (0x{native:08X}); conversions: {convs}"


# ── omniORB 4.3.4 ───────────────────────────────────────────────────────────

OMNIORB_SERVER = r"""
import sys
from omniORB import CORBA
import omniORB
omniORB.importIDL(sys.argv[1])
import spike__POA
class Echo(spike__POA.Echo):
    def echo_string(self, m): return m
orb = CORBA.ORB_init(sys.argv[2:], CORBA.ORB_ID)
orb.resolve_initial_references("RootPOA")
print(orb.object_to_string(Echo()._this()))
"""

# Every code-set knob omniORB 4.3.4 has, taken from the option strings in its
# own library (`strings libomniORB4.dylib | grep -i codeset`):
# nativeCharCodeSet, defaultCharCodeSet and their wide twins. **No option
# names the conversion list** — that absence is the finding, not a gap in
# this table.
OMNIORB_CASES = [
    ("omniORB 4.3.4 default", []),
    ("omniORB 4.3.4 -ORBnativeCharCodeSet ISO-8859-1", ["-ORBnativeCharCodeSet", "ISO-8859-1"]),
    ("omniORB 4.3.4 -ORBnativeCharCodeSet UTF-8", ["-ORBnativeCharCodeSet", "UTF-8"]),
    ("omniORB 4.3.4 -ORBdefaultCharCodeSet ISO-8859-1", ["-ORBdefaultCharCodeSet", "ISO-8859-1"]),
    (
        "omniORB 4.3.4 -ORBnativeCharCodeSet+-ORBdefaultCharCodeSet ISO-8859-1",
        ["-ORBnativeCharCodeSet", "ISO-8859-1", "-ORBdefaultCharCodeSet", "ISO-8859-1"],
    ),
]


def run_omniorb(args):
    cmd = [sys.executable, "-c", OMNIORB_SERVER, str(HERE / "echo.idl"), *args]
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=180, cwd=str(HERE))
    if p.returncode != 0:
        raise RuntimeError((p.stderr or p.stdout).strip().replace("\n", " ")[-300:])
    return p.stdout.strip().splitlines()[-1]


# ── JacORB 3.9 ──────────────────────────────────────────────────────────────

# JacORB's code-set properties, read off the class files in its own jar
# (`grep -ao 'jacorb\.[a-z_.]*codeset[a-z_.]*'`): `jacorb.codeset`,
# `jacorb.native_char_codeset`, `jacorb.native_wchar_codeset`. Again, none of
# them names the conversion list.
JACORB_CASES = [
    ("JacORB 3.9 default", []),
    (
        "JacORB 3.9 -Djacorb.native_char_codeset=ISO8859_1",
        ["-Djacorb.native_char_codeset=ISO8859_1"],
    ),
    (
        "JacORB 3.9 -Djacorb.native_char_codeset=ISO8859_15",
        ["-Djacorb.native_char_codeset=ISO8859_15"],
    ),
    (
        "JacORB 3.9 -Djacorb.native_char_codeset=US-ASCII",
        ["-Djacorb.native_char_codeset=US-ASCII"],
    ),
    (
        "JacORB 3.9 -Djacorb.native_char_codeset=ISO8859_1 -Djacorb.codeset=on",
        ["-Djacorb.native_char_codeset=ISO8859_1", "-Djacorb.codeset=on"],
    ),
    # The one setting that does stop an ORB advertising UTF-8 — by publishing
    # nothing at all. §7.10.2.4 reads an absent component as ISO-8859-1 native
    # with no wchar support, which is a statement about *this ORB as a server*
    # and says nothing about what its client half can transmit. It is therefore
    # reported and does not unblock anything; see `classify`.
    ("JacORB 3.9 -Djacorb.codeset=off", ["-Djacorb.codeset=off"]),
]

JAVA_HOME_21 = os.environ.get(
    "JAVA_HOME_21", "/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home"
)


def jacorb_classpath():
    lib = HERE / "jacorb" / "lib"
    jars = sorted(str(j) for j in lib.glob("*.jar")) if lib.is_dir() else []
    return ":".join(jars) if jars else None


def java_tool(name):
    p = pathlib.Path(JAVA_HOME_21) / "bin" / name
    return str(p) if p.is_file() else (shutil.which(name) or "")


def run_jacorb(args):
    cp = jacorb_classpath()
    if not cp:
        raise RuntimeError("no jars in spikes/jacorb/lib — run spikes/jacorb/setup.sh --jars-only")
    javac, java = java_tool("javac"), java_tool("java")
    if not javac or not java:
        raise RuntimeError(f"no JDK 21 at {JAVA_HOME_21} (brew install openjdk@21)")
    out = HERE / "jacorb" / "classes"
    out.mkdir(parents=True, exist_ok=True)
    src = HERE / "jacorb" / "IorPrinter.java"
    c = subprocess.run(
        [javac, "-nowarn", "-cp", cp, "-d", str(out), str(src)],
        capture_output=True,
        text=True,
        timeout=300,
    )
    if c.returncode != 0:
        raise RuntimeError((c.stderr or c.stdout).strip().replace("\n", " ")[-300:])
    p = subprocess.run(
        [java, "-cp", f"{cp}:{out}", *args, "IorPrinter"],
        capture_output=True,
        text=True,
        timeout=300,
    )
    iors = [l for l in p.stdout.splitlines() if l.startswith("IOR:")]
    if not iors:
        raise RuntimeError((p.stdout + p.stderr).strip().replace("\n", " ")[-300:])
    return iors[-1]


# ── cross-check ─────────────────────────────────────────────────────────────


def cross_check(ior):
    """omniORB's `catior`, read as text: a second reader of the same octets.

    If it and `parse_ior` disagree, the row above is not a measurement of
    anything, so a disagreement is reported rather than smoothed over.
    """
    if not shutil.which("catior"):
        return []
    p = subprocess.run(["catior", "-o", ior], capture_output=True, text=True, timeout=60)
    return [l.strip() for l in p.stdout.splitlines() if "code set" in l.lower()]


def main():
    argv = sys.argv[1:]
    if argv and argv[0] == "--ior":
        for char, wchar in parse_ior(argv[1]):
            print(f"  char : {describe(char)}")
            print(f"  wchar: {describe(wchar)}")
        return 0

    peers, measured, unmeasured, unblocking, absent = [], [], [], [], []
    if shutil.which("omniidl"):
        peers += [(l, run_omniorb, a) for l, a in OMNIORB_CASES]
    else:
        unmeasured.append(("omniORB 4.3.4", "not installed (brew install omniorb)"))
    if jacorb_classpath():
        peers += [(l, run_jacorb, a) for l, a in JACORB_CASES]
    else:
        unmeasured.append(("JacORB 3.9", "fixture absent (spikes/jacorb/setup.sh --jars-only)"))

    for label, runner, args in peers:
        print(f"\n== {label}")
        try:
            ior = runner(args)
        except Exception as exc:  # a peer that will not start is a failure
            print(f"  UNMEASURED: {exc}")
            unmeasured.append((label, str(exc)))
            continue
        comps = parse_ior(ior)
        if not comps:
            # Measured, not unmeasured: the peer ran and published a profile
            # with no codeset component, which §7.10.2.4 makes a positive
            # statement rather than a silence — ISO-8859-1 for `char` and no
            # `wchar` support at all. It is not the peer D009 asks for: it is
            # this ORB declining to advertise as a *server*, while the same
            # build's client half still transmits UTF-8 to us (measured in
            # spikes/codeset_advertise_probe.py).
            print(
                "  no TAG_CODE_SETS component at all — §7.10.2.4 reads that as "
                "ISO-8859-1 char and NO wchar support. An absent component is "
                "not an advertisement, and it constrains only this ORB's "
                "server side, so it does not satisfy D009 §8 row 4."
            )
            absent.append(label)
            continue
        char, wchar = comps[0]
        print(f"  char : {describe(char)}")
        print(f"  wchar: {describe(wchar)}")
        for line in cross_check(ior):
            print(f"  catior: {line}")
        reaches = char[0] == UTF_8 or UTF_8 in char[1]
        print(f"  reaches UTF-8: {reaches}")
        measured.append((label, char, reaches))
        if char[0] == ISO_8859_1 and UTF_8 not in char[1]:
            unblocking.append(label)

    print("\n-- summary")
    print(f"configurations measured: {len(measured)}")
    for label, char, reaches in measured:
        mark = "utf-8 reachable" if reaches else "UTF-8 UNREACHABLE"
        print(f"  {mark}  {label}\n      {describe(char)}")
    if absent:
        print(f"published no component at all: {len(absent)}")
        for label in absent:
            print(f"  {label} — §7.10.2.4 ISO-8859-1 char, no wchar; not an advertisement")
    if unmeasured:
        print(f"unmeasured: {len(unmeasured)}")
        for label, why in unmeasured:
            print(f"  {label}: {why}")

    if not measured:
        print("\nVERDICT: nothing measured. An unmeasured check is a failure, never a pass.")
        return 2
    if unblocking:
        print(
            "\nVERDICT: a peer advertises ISO-8859-1 without UTF-8 — "
            + ", ".join(unblocking)
            + ". D009 §8 batch 4 is UNBLOCKED: grow server_component_info()'s "
            "char conversion list and measure the round trip, octets asserted."
        )
        return 1
    print(
        "\nVERDICT: every measured peer reaches UTF-8, so the empty `char` "
        "conversion list in codeset::server_component_info() is still the "
        "honest declaration. D009 §8 batch 4 stays BLOCKED. What growing it "
        "would do to these peers is a separate measurement, and it is not "
        "nothing: spikes/codeset_advertise_probe.py finds JacORB moving down "
        "to ISO-8859-1 as soon as the conversion is offered, while omniORB "
        "keeps sending UTF-8."
    )
    return 2 if unmeasured else 0


if __name__ == "__main__":
    sys.exit(main())
