#!/usr/bin/env python3
"""An agent-shaped caller against the estate, run twice with two exposures.

`spikes/e2e/agent.py` asks whether the bridge works. This asks a different
question, and it is the one a dozen files make askable: **what does an operator
actually get for the exposure they will actually write?**

So the same agent runs twice against the same live object.

  session A — `--expose <interface>`, the one-flag form. Twelve interfaces and
              seventy-six operations were registered; this names one of them
              and grants the caller no scopes at all.
  session B — `--expose <interface>.<operation>` three times, naming only the
              read-only slice.

Both sessions call the same destructive operation, `cancel`. The difference
between what happens to it in A and in B is the measurement, and it is reported
rather than asserted to be safe: the estate carries no `ai_effect` and no
`ai_authz`, because legacy IDL never does, so the approval stage and the scope
stage have nothing to key on and **A is the whole policy**.

usage: agent.py <ior-file> <idl-file>
"""
import json
import pathlib
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent.parent.parent
SERVER = HERE / "target/debug/orbweaver-mcp-server"

INTERFACE = "IDL:meridian.com/MFS/Tracking/ShipmentTracker:1.0"
PRINCIPAL = "ops-agent"
READ_ONLY_SLICE = ["lookup", "delivered", "backlog"]

fails = 0


def ok(msg):
    print(f"  ok   {msg}")


def no(msg):
    global fails
    fails += 1
    print(f"  FAIL {msg}")


def note(msg):
    print(f"  ..   {msg}")


class Bridge:
    """The MCP server as a subprocess, spoken to one line at a time."""

    def __init__(self, ior, idl, expose, assume_effect=None):
        argv = [str(SERVER), "--idl", idl, "--ior", ior, "--as", PRINCIPAL]
        for e in expose:
            argv += ["--expose", e]
        # An estate annotates nothing, so every operation now states no effect
        # and the bridge refuses it — correctly. `--assume-effect` is the
        # operator saying, once, what this estate's silence means. Without it
        # session A cannot reach the servant at all and its finding (the
        # *contract* stopped the call, not the guard) becomes unmeasurable.
        if assume_effect:
            argv += ["--assume-effect", assume_effect]
        self.frames = []
        self.p = subprocess.Popen(
            argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
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
        self.frames.append(line.rstrip("\n"))
        return json.loads(line)

    def call(self, name, arguments, rid):
        return self.send({
            "jsonrpc": "2.0", "id": rid, "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        })

    def open(self):
        init = self.send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                          "params": {"protocolVersion": "2024-11-05"}})
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"},
                  expect_reply=False)
        return init

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=20)
        return self.p.stderr.read()


def text_of(reply):
    """The tool content, which is JSON inside a JSON string."""
    return reply["result"]["content"][0]["text"]


def handle_for(b, rid):
    """Discover the interface the way an agent must: by searching for a word."""
    found = json.loads(text_of(b.call("search_interfaces", {"query": "shipment tracking"}, rid)))
    for entry in found.get("interfaces", []):
        if entry["id"] == INTERFACE and entry.get("handles"):
            return entry["handles"][0], found
    return None, found


# ── session A: the one-flag exposure an operator writes first ────────────────

def session_a(ior, idl):
    print("\nsession A — --expose <interface>, no scopes held, silence read as read_only")
    b = Bridge(ior, idl, [INTERFACE], assume_effect="read_only")
    init = b.open()
    if init.get("result", {}).get("protocolVersion"):
        ok(f"initialize -> protocol {init['result']['protocolVersion']}")
    else:
        no(f"the handshake did not answer: {init}")

    handle, found = handle_for(b, 2)
    ids = [i["id"] for i in found.get("interfaces", [])]
    if handle:
        ok(f"search_interfaces('shipment tracking') found it among {len(ids)} visible interface(s)")
    else:
        no(f"search_interfaces returned {ids} and no handle; the agent cannot start")
        b.close()
        return

    described = json.loads(text_of(b.call("describe_interface", {"id": INTERFACE}, 3)))
    shown = sorted(o["name"] for o in described["operations"])
    note(f"describe_interface -> {len(shown)} operations: {', '.join(shown)}")
    blob = json.dumps(described)
    # The estate is unannotated, and this is the point rather than a defect in
    # the estate: no legacy IDL carries SIDL. What the agent gets is names.
    if "ai_authz" not in blob and "ai_desc" not in blob:
        ok("the description carries no ai_desc and no ai_authz — the legacy state")
    else:
        no("the estate unexpectedly carries annotations; the measurement below is not about legacy IDL")

    r = b.call("invoke_operation", {"handle": handle, "operation": "lookup",
                                    "arguments": {"reference": 8801}}, 4)
    if r["result"].get("isError"):
        no(f"lookup was refused: {text_of(r)}")
    else:
        got = json.loads(text_of(r)).get("returns", {})
        if got.get("origin") == "LHR2" and got.get("destination") == "BRU":
            ok(f"invoke_operation(lookup, 8801) -> {got.get('origin')} to {got.get('destination')}")
        else:
            no(f"lookup returned {got}")

    # Inherited from Describable, and never declared on ShipmentTracker.
    r = b.call("invoke_operation", {"handle": handle, "operation": "describe",
                                    "arguments": {}}, 5)
    if not r["result"].get("isError") and "consignment" in text_of(r):
        ok("an operation inherited from Describable dispatches through the skeleton")
    else:
        no(f"the inherited operation did not answer: {r}")

    # And now the three views of one interface, compared. `describe_interface`
    # is what an agent reads to learn what it may call; the guard's dry run
    # counts what it would judge; the servant answers what it serves. On an
    # estate where ten of twelve interfaces inherit `Describable`, they differ.
    if "describe" not in shown and not r["result"].get("isError"):
        print("  ..   INHERITANCE GAP: describe_interface listed "
              f"{len(shown)} operations and omitted the inherited ones, "
              "yet `describe` was invoked successfully on the same handle")
        print("  ..   the guard's dry run counts the resolved chain and the "
              "servant serves it; only the agent's own description does not show it")
    elif "describe" in shown:
        ok("describe_interface lists the operations inherited from Describable")

    # THE MEASUREMENT. `cancel` changes state and the caller holds no scope.
    r = b.call("invoke_operation", {"handle": handle, "operation": "cancel",
                                    "arguments": {"reference": 8801,
                                                  "reason": "agent decided to"}}, 6)
    body = text_of(r)
    if r["result"].get("isError") and "NotAuthorised" in body:
        ok("cancel reached the wire and was refused BY THE 2003 SERVANT, not by the guard")
        note("the guard allowed it: no ai_effect means no approval stage, no ai_authz means no scope stage")
        note(f"       the servant's answer: {body[:120]}")
    elif r["result"].get("isError"):
        no(f"cancel was refused, but not by the servant: {body[:200]}")
    else:
        no("cancel SUCCEEDED — a destructive legacy operation ran on one --expose flag")

    stderr = b.close()
    if "root handle: cap_" in stderr:
        ok("the operator's handle line went to stderr, where it is not a frame")
    else:
        no("the operator never saw a root handle on stderr")
    leaked = [n for n in ("IOR:",) if n in "\n".join(b.frames)]
    if leaked:
        no(f"{leaked} reached the agent")
    else:
        ok("no stringified IOR ever reached the agent")


# ── session B: the exposure that actually holds ──────────────────────────────

def session_b(ior, idl):
    print("\nsession B — --expose <interface>.<operation> x3, the read-only slice")
    expose = [f"{INTERFACE}.{op}" for op in READ_ONLY_SLICE]
    note(f"the operator wrote {len(expose)} flags to allow {len(expose)} of 13 operations")
    # Session B needs the same declaration as A, and for the same reason: the
    # slice is read-only by the operator's judgement, not by anything the
    # contract says. Naming three operations does not annotate them. Its
    # finding survives intact — `--assume-effect` states what a silence means,
    # it does not expose anything, so `cancel` is still refused by the
    # allowlist before anything is dialled.
    b = Bridge(ior, idl, expose, assume_effect="read_only")
    b.open()
    handle, _ = handle_for(b, 2)
    if not handle:
        no("per-operation exposure left the interface undiscoverable, so the agent cannot start")
        b.close()
        return
    ok("the interface is still discoverable when only some operations are allowed")

    r = b.call("invoke_operation", {"handle": handle, "operation": "backlog",
                                    "arguments": {}}, 3)
    if not r["result"].get("isError") and json.loads(text_of(r)).get("returns") == 1:
        ok("invoke_operation(backlog) -> 1 undelivered consignment")
    else:
        no(f"backlog returned {r}")

    r = b.call("invoke_operation", {"handle": handle, "operation": "cancel",
                                    "arguments": {"reference": 8801,
                                                  "reason": "agent decided to"}}, 4)
    body = text_of(r) if r["result"].get("isError") else ""
    if body and "NotAuthorised" not in body:
        ok("cancel is refused by the GUARD this time, before anything is dialled")
        note(f"       the guard's answer: {body[:120]}")
    elif body:
        no("cancel still reached the servant; per-operation exposure did not gate it")
    else:
        no("cancel succeeded under a read-only allowlist")

    # A refusal must not become an oracle for what exists behind it.
    r = b.call("invoke_operation", {"handle": handle, "operation": "no_such_operation",
                                    "arguments": {}}, 5)
    invented = text_of(r) if r["result"].get("isError") else ""
    if invented and body and invented.split(":")[0] == body.split(":")[0]:
        ok("an operation that does not exist is refused the same way as one that does")
    else:
        note(f"refusals differ: real={body[:60]!r} invented={invented[:60]!r}")

    b.close()


def main():
    ior, idl = sys.argv[1], sys.argv[2]
    session_a(ior, idl)
    session_b(ior, idl)
    print("\nagent: PASS" if fails == 0 else f"\nagent: FAIL — {fails} case(s)")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
