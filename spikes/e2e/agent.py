#!/usr/bin/env python3
"""An agent-shaped caller, reaching the parking facility through the bridge.

This is the last hop of `spikes/end_to_end.sh`: a process that knows a
repository id and nothing else — no IOR, no host, no port, no stub — and that
has to *discover* the interface, read its contract, and then call it. It speaks
the three tools in the order an agent would: search, describe, invoke.

The guard is in the path for every one of those. Exposure is granted for
exactly one interface and the caller holds one scope, so two operations are
allowed and the third is refused for the scope it does not hold. Both outcomes
are asserted here, because a gate nobody ever sees refuse is indistinguishable
from no gate.

usage: agent.py <ior-file> <idl-file>
"""
import json
import pathlib
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent.parent.parent
SERVER = HERE / "target/debug/orbweaver-mcp-server"

INTERFACE = "IDL:ParkingFacility/ParkingControl:1.0"
PRINCIPAL = "ops-agent"
HELD_SCOPE = "parking:read"

fails = 0
transcript = []


def ok(msg):
    print(f"  ok   {msg}")


def no(msg):
    global fails
    fails += 1
    print(f"  FAIL {msg}")


class Bridge:
    """The MCP server as a subprocess, spoken to one line at a time."""

    def __init__(self, ior, idl):
        self.p = subprocess.Popen(
            [
                str(SERVER),
                "--idl", idl,
                "--ior", ior,
                # Exactly one interface, named by a human. Everything else in
                # the catalog stays invisible.
                "--expose", INTERFACE,
                "--as", PRINCIPAL,
                "--scope", HELD_SCOPE,
            ],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True, bufsize=1,
        )

    def send(self, obj, expect_reply=True):
        self.p.stdin.write(json.dumps(obj) + "\n")
        self.p.stdin.flush()
        if not expect_reply:
            return None
        line = self.p.stdout.readline()
        if not line:
            raise RuntimeError("the bridge closed stdout: " + self.p.stderr.read())
        transcript.append(line.rstrip("\n"))
        return json.loads(line)

    def call(self, name, arguments, rid):
        return self.send({
            "jsonrpc": "2.0", "id": rid, "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        })

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=15)
        return self.p.stderr.read()


def text_of(reply):
    """The tool content, which is JSON inside a JSON string."""
    return reply["result"]["content"][0]["text"]


def main():
    ior, idl = sys.argv[1], sys.argv[2]
    b = Bridge(ior, idl)

    init = b.send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                   "params": {"protocolVersion": "2024-11-05"}})
    if init.get("result", {}).get("protocolVersion"):
        ok(f"initialize -> protocol {init['result']['protocolVersion']}")
    else:
        no(f"the handshake did not answer: {init}")
    b.send({"jsonrpc": "2.0", "method": "notifications/initialized"}, expect_reply=False)

    # ── search: the agent knows a word, not an address ───────────────────────
    found = json.loads(text_of(b.call("search_interfaces", {"query": "parking gate"}, 2)))
    ids = [i["id"] for i in found.get("interfaces", [])]
    if ids == [INTERFACE]:
        ok(f"search_interfaces('parking gate') -> {INTERFACE}")
    else:
        no(f"search_interfaces returned {ids}")
    handles = found["interfaces"][0]["handles"] if found.get("interfaces") else []
    if not handles:
        no("search_interfaces carried no handle; an agent has no way to start")
        print("agent: FAIL")
        sys.exit(1)
    handle = handles[0]
    ok("the result carries a capability handle, so the agent can bootstrap itself")

    # ── describe: the contract, including what the annotations say ───────────
    described = json.loads(text_of(b.call("describe_interface", {"id": INTERFACE}, 3)))
    shown = sorted(o["name"] for o in described["operations"])
    if shown == ["get_floor_occupancy", "get_gate_status", "open_entry_gate"]:
        ok(f"describe_interface -> {shown}")
    else:
        no(f"describe_interface showed {shown}")
    described_text = json.dumps(described)
    if "gate:operate" in described_text:
        ok("the description carries the ai_authz scope the requirement asked for")
    else:
        no("the description does not mention the gate:operate scope")

    # ── invoke: allowed, because nothing in the contract gates it ────────────
    r = b.call("invoke_operation",
               {"handle": handle, "operation": "get_floor_occupancy",
                "arguments": {"level": -1}}, 4)
    if r["result"].get("isError"):
        no(f"get_floor_occupancy was refused: {text_of(r)}")
    else:
        got = json.loads(text_of(r)).get("returns", {})
        if got.get("level") == -1 and got.get("remaining_spaces") == 12:
            ok("invoke_operation(get_floor_occupancy, -1) -> 12 free spaces on B1")
        else:
            no(f"get_floor_occupancy returned {got}")

    r = b.call("invoke_operation",
               {"handle": handle, "operation": "get_gate_status",
                "arguments": {"target_gate": "entry-north"}}, 5)
    if r["result"].get("isError"):
        no(f"get_gate_status was refused: {text_of(r)}")
    else:
        got = json.loads(text_of(r)).get("returns", {})
        if got.get("gate_id") == "entry-north" and "CLOSED" in str(got.get("state")):
            ok(f"invoke_operation(get_gate_status, entry-north) -> {got.get('state')}")
        else:
            no(f"get_gate_status returned {got}")

    # ── the declared exception, as content rather than as a crash ────────────
    r = b.call("invoke_operation",
               {"handle": handle, "operation": "get_floor_occupancy",
                "arguments": {"level": -9}}, 6)
    if r["result"].get("isError") and "UnknownFloor" in text_of(r):
        ok("a floor the facility does not have comes back as UnknownFloor")
    else:
        no(f"the unknown floor produced {r}")

    # ── the gate, visible ────────────────────────────────────────────────────
    r = b.call("invoke_operation",
               {"handle": handle, "operation": "open_entry_gate",
                "arguments": {"vehicle": {"plate_number": "12가 3456"}}}, 7)
    refusal = text_of(r) if r["result"].get("isError") else ""
    if refusal and "gate:operate" in refusal and "does not hold" in refusal:
        ok("open_entry_gate is REFUSED: the caller does not hold gate:operate")
        print(f"       refusal: {refusal}")
    else:
        no(f"the scope gate did not refuse: {r}")

    # And the refusal cost the session nothing.
    r = b.call("invoke_operation",
               {"handle": handle, "operation": "get_gate_status",
                "arguments": {"target_gate": "entry-north"}}, 8)
    if not r["result"].get("isError"):
        ok("the session survives a refusal and keeps answering")
    else:
        no(f"the session did not survive the refusal: {r}")

    stderr = b.close()

    # ── the transport rules the bridge exists to keep ────────────────────────
    if all(json.loads(f) for f in transcript):
        ok(f"{len(transcript)} frames on stdout, every one a single-line JSON object")
    blob = "\n".join(transcript)
    leaked = [n for n in (host_of(ior), "IOR:") if n and len(n) >= 3 and n in blob]
    if leaked:
        no(f"{leaked} reached the agent")
    else:
        ok("no endpoint or stringified IOR ever reached the agent")

    if "root handle: cap_" in stderr:
        ok("the operator's handle line went to stderr, where it is not a frame")
    else:
        no("the operator never saw a root handle on stderr")

    print("agent: PASS" if fails == 0 else f"agent: FAIL — {fails} case(s)")
    sys.exit(1 if fails else 0)


def host_of(ior_path):
    """The endpoint the agent must never see, read out of the IOR directly."""
    out = subprocess.run(
        [str(HERE / "target/debug/spike-dump"), ior_path],
        capture_output=True, text=True,
    ).stdout
    for token in out.split():
        if ":" in token and token.count(".") == 3:
            return token.split(":")[0]
    return ""


if __name__ == "__main__":
    main()
