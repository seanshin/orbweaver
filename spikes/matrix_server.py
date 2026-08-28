#!/usr/bin/env python3
"""Interop peer for the v1 wire-type matrix (spikes/matrix.idl).

TEST FIXTURE. omniORB is LGPL/GPL and is never linked into or shipped with
Orbweaver — the Rust client reaches it over TCP using the published GIOP
specification only. See docs/PLAN.md §10.

Every operation is an echo. That is deliberate: an echo makes the peer's
decoder and encoder both part of the test, so a round-trip failure means our
bytes were wrong rather than the peer's logic being wrong. Anything that
transforms the value would blur that.
"""

import pathlib
import sys

import omniORB
from omniORB import CORBA, PortableServer

HERE = pathlib.Path(__file__).parent
omniORB.importIDL(str(HERE / "matrix.idl"))
import matrix  # noqa: E402
import matrix__POA  # noqa: E402

from orbexit import leave


class TypeMatrix(matrix__POA.TypeMatrix):
    def __init__(self):
        self._calls = 0
        self._fired = 0

    def _count(self):
        self._calls += 1

    # ── echoes ───────────────────────────────────────────────────────────
    def echo_ragged(self, v):
        self._count()
        return v

    def echo_nested(self, v):
        self._count()
        return v

    def echo_payload(self, v):
        self._count()
        return v

    def echo_defaulted(self, v):
        self._count()
        return v

    def echo_bool_switched(self, v):
        self._count()
        return v

    def echo_collections(self, v):
        self._count()
        return v

    def echo_extremes(self, v):
        self._count()
        return v

    def echo_text(self, v):
        self._count()
        return v

    def echo_any(self, v):
        self._count()
        return v

    # ── exceptions ───────────────────────────────────────────────────────
    def raise_simple(self):
        self._count()
        raise matrix.Simple()

    def raise_detailed(self):
        self._count()
        raise matrix.Detailed("deliberate failure from the interop peer", 42)

    # ── oneway ───────────────────────────────────────────────────────────
    def fire_and_forget(self, note):
        # No reply is sent. The client cannot observe this directly, which is
        # why fired_count() exists — it is the only way to prove a oneway
        # actually arrived rather than being silently dropped.
        self._fired += 1

    def fired_count(self):
        self._count()
        return self._fired

    # ── attribute ────────────────────────────────────────────────────────
    def _get_call_count(self):
        return self._calls


def main():
    orb = CORBA.ORB_init(sys.argv, CORBA.ORB_ID)
    poa = orb.resolve_initial_references("RootPOA")
    servant = TypeMatrix()
    ref = servant._this()
    poa._get_the_POAManager().activate()

    out = HERE / "matrix.ior"
    out.write_text(orb.object_to_string(ref))
    print(f"IOR written to {out}", flush=True)
    print("READY", flush=True)
    try:
        orb.run()
    except KeyboardInterrupt:
        orb.shutdown(True)


if __name__ == "__main__":
    leave(main())
