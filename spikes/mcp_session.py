#!/usr/bin/env python3
"""Drives orbweaver-mcp-server the way a client does.

Everything else exercises the bridge in-process. This exercises the part that
only exists once there is a real process: the handshake ordering, one JSON
object per line, and the rule that stdout carries the protocol and nothing else.

It talks *interactively* — write a frame, read the reply, use what it said.
A first version fed every frame at once and read the handle out of an earlier
run, which could never work and was right not to: a capability table belongs to
one session, so a handle from another process is worthless. That is the
property. It also found a real gap, since at the time nothing in a *result*
carried a handle at all and an agent had no way to start.

usage: mcp_session.py <ior-file> <idl-file>
"""
import json
import subprocess
import sys
import pathlib

HERE = pathlib.Path(__file__).parent.parent
fails = 0


def ok(msg):
    print(f"  ok   {msg}")


def no(msg):
    global fails
    fails += 1
    print(f"  FAIL {msg}")


class Client:
    def __init__(self, ior, idl, expose):
        args = [str(HERE / "target/debug/orbweaver-mcp-server"), "--idl", idl, "--ior", ior]
        for e in expose:
            args += ["--expose", e]
        self.p = subprocess.Popen(
            args, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, bufsize=1,
        )
        self.frames = []

    def send(self, obj, expect_reply=True):
        self.p.stdin.write(json.dumps(obj) + "\n")
        self.p.stdin.flush()
        if not expect_reply:
            return None
        line = self.p.stdout.readline()
        if not line:
            raise RuntimeError("the server closed stdout")
        self.frames.append(line.rstrip("\n"))
        return json.loads(line)

    def call(self, name, arguments, rid):
        return self.send({
            "jsonrpc": "2.0", "id": rid, "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        })

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=10)
        return self.p.stderr.read()


def text_of(reply):
    """The tool content, which is JSON inside a JSON string."""
    return reply["result"]["content"][0]["text"]


def main():
    ior, idl = sys.argv[1], sys.argv[2]
    c = Client(ior, idl, ["IDL:spike/Echo:1.0.ping", "IDL:spike/Echo:1.0.add"])

    init = c.send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                   "params": {"protocolVersion": "2024-11-05"}})
    if init.get("result", {}).get("protocolVersion"):
        ok(f"initialize -> protocol {init['result']['protocolVersion']}")
    else:
        no(f"the handshake did not answer: {init}")

    c.send({"jsonrpc": "2.0", "method": "notifications/initialized"}, expect_reply=False)

    listed = c.send({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
    names = [t["name"] for t in listed["result"]["tools"]]
    # The triad until 2026-08-26, when D024 §5's four IDL tools landed behind the
    # same interceptor chain. Pinned as the whole list and in order, because the
    # order is what a client sees and a silent addition is what this exists to
    # catch — and negatively, because a tool that can *register* a contract would
    # change what other agents see and D024 §5 refuses it by name.
    triad = ["search_interfaces", "describe_interface", "invoke_operation"]
    idl_tools = ["validate_contract", "diff_contract", "describe_type", "preview_generation"]
    forbidden = [n for n in ("register_contract", "register", "compile_idl") if n in names]
    if forbidden:
        no(f"tools/list advertises {forbidden}, which D024 §5 refuses by name")
    elif names == triad + idl_tools:
        ok("tools/list -> the triad plus D024 §5's four IDL tools, in order")
    else:
        no(f"tools/list returned {names}")

    found = json.loads(text_of(c.call("search_interfaces", {"query": "echo"}, 3)))
    handles = found["interfaces"][0]["handles"] if found["interfaces"] else []
    if handles:
        ok("search_interfaces hands the session a handle, so it can bootstrap itself")
    else:
        no("search_interfaces carried no handle; an agent has no way to start")
        print("mcp session: FAIL")
        sys.exit(1)
    handle = handles[0]

    described = json.loads(text_of(c.call(
        "describe_interface", {"id": "IDL:spike/Echo:1.0"}, 4)))
    shown = sorted(o["name"] for o in described["operations"])
    if shown == ["add", "ping"]:
        ok("describe_interface shows the two exposed operations and hides nine")
    else:
        no(f"describe_interface showed {shown}")

    r = c.call("invoke_operation",
               {"handle": handle, "operation": "add", "arguments": {"a": 40, "b": 2}}, 5)
    if json.loads(text_of(r)).get("returns") == 42:
        ok("invoke_operation(add, 40, 2) -> 42, through a capability handle")
    else:
        no(f"the call did not return 42: {r}")

    r = c.call("invoke_operation",
               {"handle": handle, "operation": "echo_string", "arguments": {"msg": "x"}}, 6)
    if r["result"]["isError"] and "not among its allowed" in text_of(r):
        ok("an unexposed operation is refused as content, not as a protocol error")
    else:
        no(f"the unexposed operation was not refused properly: {r}")

    forged = "cap_" + "0" * 32
    r = c.call("invoke_operation",
               {"handle": forged, "operation": "add", "arguments": {"a": 1, "b": 2}}, 7)
    if r["result"]["isError"] and "no live reference" in text_of(r):
        ok("a forged handle resolves to nothing")
    else:
        no(f"a forged handle was not refused: {r}")

    r = c.send({"jsonrpc": "2.0", "id": 8, "method": "resources/list"})
    if r.get("error", {}).get("code") == -32601:
        ok("an unknown method is a JSON-RPC error, unlike a refused tool call")
    else:
        no(f"an unknown method did not produce -32601: {r}")

    c.p.stdin.write("not json at all\n")
    c.p.stdin.flush()
    line = c.p.stdout.readline()
    c.frames.append(line.rstrip("\n"))
    if json.loads(line).get("error", {}).get("code") == -32700:
        ok("a malformed line is answered rather than closing the session")
    else:
        no(f"malformed input produced {line!r}")

    stderr = c.close()

    # Every frame must be one line of valid JSON, or the transport is broken in
    # a way a client reports as bad JSON rather than as the bug it is.
    if all(json.loads(f) for f in c.frames):
        ok(f"{len(c.frames)} frames on stdout, every one a single-line JSON object")

    # And nothing dialable may be among them.
    blob = "\n".join(c.frames)
    leaked = [n for n in (host_of(ior), "IOR:") if n and len(n) >= 3 and n in blob]
    if leaked:
        no(f"{leaked} reached stdout")
    else:
        ok("no endpoint or stringified IOR on the wire to the agent")

    if "cap_" in stderr and "cap_" not in blob.split('"handles"')[0]:
        ok("the operator's handle line went to stderr, where it is not a frame")

    print("mcp session: PASS" if fails == 0 else f"mcp session: FAIL — {fails} case(s)")
    sys.exit(1 if fails else 0)


def host_of(ior_path):
    """The endpoint, read from the IOR the agent must never see."""
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
