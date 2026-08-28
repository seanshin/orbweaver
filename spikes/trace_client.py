#!/usr/bin/env python3
"""Ground truth: make omniORB itself emit a GIOP 1.2 request on the wire."""
import sys, pathlib
from omniORB import CORBA
import omniORB
HERE = pathlib.Path(__file__).parent
omniORB.importIDL(str(HERE / "echo.idl"))
import spike

# How an omniORB fixture leaves: see spikes/orbexit.py.
from orbexit import leave

orb = CORBA.ORB_init(sys.argv, CORBA.ORB_ID)
ref = orb.string_to_object((HERE / "echo.ior").read_text().strip())
echo = ref._narrow(spike.Echo)
print("ping ->", echo.ping(), file=sys.stderr)
leave(0)
