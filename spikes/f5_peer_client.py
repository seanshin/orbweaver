#!/usr/bin/env python3
"""F5 (CosLifeCycle/Property) driven by a *peer ORB*, not by our client.

`SERVICES-COVERAGE.md` §9 records the honest limit of the 16-of-16 result for
`corpus/golden/23-moe-enterprise.idl`: *"No cross-ORB direction. This is our
client shape, over raw GIOP, against our servers."*  PLAN-SERVICES §1 rule 3
asks every service chapter to name its peer or say none exists; §5 (F5) names
none.  This is that peer.

omniORBpy compiles the contract itself (`omniidl -bpython`) and invokes it as
an ordinary CORBA client: stubs it generated, its own IIOP, its own CDR, its
own exception decoding.  Nothing of omniORB is imported by the project or
linked into it — it is run as a separate-process wire peer and an external
program, clauses (a) and (b) of the licensing boundary.

Usage
-----
    cargo run -q --bin spike-tenants -- spikes/f5a.ior spikes/f5b.ior --hold &
    python3 spikes/f5_peer_client.py spikes/f5a.ior spikes/f5b.ior

Exit code is the verdict.  A missing omniORBpy is reported as BLOCKED and is a
failure, never a pass — an unmeasured check is a failure (CLAUDE.md).

What it does not measure
------------------------
Byte order.  omniORB emits native-endian and this machine is little-endian, so
the big-endian request path is exercised by our own tests and not here.
"""

import os
import re
import subprocess
import sys
import tempfile

# How an omniORB fixture leaves: see spikes/orbexit.py.
from orbexit import leave

IDL = "corpus/golden/23-moe-enterprise.idl"

# The key template `tenant_service.rs`'s module docs publish.  Deriving a
# reference from it is the documented way to reach the objects no operation of
# this contract returns — see the note this script prints at the end.
KEY_EXPERT = "MoE/t/{tenant}/expert/{capability}"
KEY_POLICY = "MoE/t/{tenant}/policy/{domain}"


class Report:
    def __init__(self):
        self.failures = 0
        self.served = set()

    def check(self, ok, what):
        print(f"  {'ok  ' if ok else 'FAIL'}  {what}")
        if not ok:
            self.failures += 1

    def eq(self, got, want, what):
        self.check(got == want, f"{what} (got {got!r}, want {want!r})")

    def op(self, name):
        self.served.add(name)


def blocked(why):
    print(f"f5-peer: BLOCKED — {why}")
    return 2


def main(argv):
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    os.chdir(root)
    acme_ior = argv[0] if argv else "spikes/f5a.ior"
    globex_ior = argv[1] if len(argv) > 1 else "spikes/f5b.ior"

    for p in (acme_ior, globex_ior, IDL):
        if not os.path.exists(p):
            return blocked(f"{p} is not there — start `spike-tenants --hold` first")

    stubs = tempfile.mkdtemp(prefix="f5-peer-stubs-")
    # omniidl derives a python module name from the file name, and
    # `23-moe-enterprise` is not an identifier.  The copy is the contract
    # byte for byte; only its name changes.
    staged = os.path.join(stubs, "f5_moe_enterprise.idl")
    with open(IDL, "rb") as src, open(staged, "wb") as dst:
        dst.write(src.read())
    r = subprocess.run(
        ["omniidl", "-bpython", "-C", stubs, staged],
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        return blocked(f"omniidl could not compile {IDL}: {r.stderr.strip()}")

    sys.path.insert(0, stubs)
    try:
        from omniORB import CORBA  # noqa: E402  (the peer ORB, imported by the peer)
        import moe  # noqa: F401  — omniidl's own stubs for our contract
        import moe.enterprise
    except ImportError as e:
        return blocked(f"omniORBpy is not installed: {e}")

    orb = CORBA.ORB_init(["-ORBnativeCharCodeSet", "ISO-8859-1"], CORBA.ORB_ID)
    rep = Report()

    with open(acme_ior) as f:
        acme_ref = orb.string_to_object(f.read().strip())
    with open(globex_ior) as f:
        globex_ref = orb.string_to_object(f.read().strip())

    # ── narrow: the peer's own _is_a decision ────────────────────────────────
    acme = acme_ref._narrow(moe.enterprise.ModelFactory)
    globex = globex_ref._narrow(moe.enterprise.ModelFactory)
    rep.check(acme is not None, "omniORB narrows our IOR to moe::enterprise::ModelFactory")
    rep.check(globex is not None, "omniORB narrows globex's factory too")
    if acme is None or globex is None:
        print("\nf5-peer: FAIL — nothing to call")
        return 1

    # Where the objects this contract hands back no reference for live. See
    # the note this script prints when it passes: `bind_expert` and
    # `set_policy` take references no operation of golden 23 returns, so a
    # client — ours, the sweep's, or this peer — has to build them from the
    # key template `tenant_service.rs` publishes.
    with open(acme_ior) as f:
        endpoint = endpoint_of(f.read().strip())

    # ── ModelFactory ─────────────────────────────────────────────────────────
    print("\nModelFactory — the CosLifeCycle half")
    # A version is unique within a tenant and a duplicate is BAD_PARAM, so the
    # run stamps its own — the servant this points at may have been held open
    # across several runs, and a fixed string would fail the second time for a
    # reason that has nothing to do with the wire.
    version = f"peer-{os.getpid()}"
    manifest = moe.enterprise.Manifest(
        tenant_id="acme",
        base_model="llama-70b",
        experts=[],
        policy_domain="acme-default",
        version=version,
        residency_region="eu-west",
    )
    model = acme.create(manifest)
    rep.op("create")
    rep.check(model is not None, "create(Manifest) -> a ComposedModel omniORB can hold")

    clone = acme.clone_model(model, version + "-clone")
    rep.op("clone_model")
    rep.check(clone is not None, "clone_model(model, <version>-clone) -> a second reference")

    acme.deploy(model)
    rep.op("deploy")
    rep.check(True, "deploy(model) returned")

    # ── ComposedModel ────────────────────────────────────────────────────────
    print("\nComposedModel — the composition half")
    got = model.get_manifest()
    rep.op("get_manifest")
    rep.eq(got.tenant_id, "acme", "get_manifest().tenant_id decoded by omniORB")
    rep.eq(got.version, version, "get_manifest().version")
    rep.eq(list(got.experts), [], "get_manifest().experts sequence (empty before the bind)")

    expert = orb.string_to_object(
        corbaloc(endpoint, KEY_EXPERT.format(tenant="acme", capability="math"))
    )._narrow(moe.enterprise.EnterpriseExpert)
    rep.check(expert is not None, "the documented key template narrows to EnterpriseExpert")

    model.bind_expert(expert)
    rep.op("bind_expert")
    rep.check(True, "bind_expert(acme/math) returned")

    policy = orb.string_to_object(
        corbaloc(endpoint, KEY_POLICY.format(tenant="acme", domain="acme-default"))
    )._narrow(moe.enterprise.PolicyDomain)
    rep.check(policy is not None, "the key template narrows to PolicyDomain")

    model.set_policy(policy)
    rep.op("set_policy")
    rep.check(True, "set_policy(acme-default) returned")

    act = moe.Activation(data=b"\x01\x02\x03", dtype="f32", shape="[3]")
    ctx = moe.CallContext(request_id="peer-1", trace_id="t-1", step=1)
    out = model.infer(act, ctx)
    rep.op("infer")
    rep.check(len(out.data) > 0, "infer() after a bind returns an Activation")

    # ── PolicyDomain ─────────────────────────────────────────────────────────
    print("\nPolicyDomain — governance as an object")
    rep.eq(policy.authorize("nobody", "math"), False, "authorize('nobody') is default-deny")
    rep.op("authorize")
    rep.eq(policy.check_residency("gpu-eu-1"), True, "check_residency('gpu-eu-1') in region")
    rep.eq(policy.check_residency("gpu-us-1"), False, "check_residency('gpu-us-1') out of region")
    rep.eq(policy.check_residency("gpu-unknown"), False, "an undeclared node is refused")
    rep.op("check_residency")
    policy.audit(ctx, "peer ORB was here")
    rep.op("audit")
    rep.check(True, "audit(ctx, event) returned")

    # ── EnterpriseExpert, and the two it inherits ────────────────────────────
    print("\nEnterpriseExpert — and ::moe::Expert through it")
    rep.eq(expert.get_tenant_id(), "acme", "get_tenant_id()")
    rep.op("get_tenant_id")
    base = expert.base()
    rep.op("base")
    rep.check(base is not None, "base() -> a ::moe::Expert reference")
    rep.check(
        base._is_a("IDL:moe/Expert:1.0") and not base._is_a("IDL:moe/enterprise/EnterpriseExpert:1.0"),
        "the shared base is an Expert and nothing more — the type bounds the crossing",
    )
    delta = expert.adapter_delta()
    rep.op("adapter_delta")
    rep.check(len(delta) > 0, "adapter_delta() -> the tenant's own bytes")

    cap = expert.describe()
    rep.op("describe")
    rep.eq(cap.id, "math", "describe() inherited from ::moe::Expert")
    proc = expert.process(act, ctx)
    rep.op("process")
    rep.check(proc.dtype != "", "process() inherited from ::moe::Expert")

    base_cap = base._narrow(moe.Expert).describe()
    rep.check(base_cap.id != "", "describe() on the shared base too")

    # ── the refusals, decoded by the peer's own exception machinery ──────────
    print("\nRefusals — omniORB decoding our system exceptions")
    try:
        globex.deploy(model)
        rep.check(False, "globex.deploy(acme's model) should have been refused")
    except CORBA.NO_PERMISSION:
        rep.check(True, "a cross-tenant reference argument is NO_PERMISSION to the peer")
    except CORBA.Exception as e:
        rep.check(False, f"cross-tenant deploy raised {e.__class__.__name__}, want NO_PERMISSION")

    try:
        acme.create(
            moe.enterprise.Manifest(
                tenant_id="globex",
                base_model="llama-70b",
                experts=[],
                policy_domain="acme-default",
                version=version + "-x",
                residency_region="eu-west",
            )
        )
        rep.check(False, "create() of another tenant's manifest should have been refused")
    except CORBA.NO_PERMISSION:
        rep.check(True, "create() cannot mint into another tenant")
    except CORBA.Exception as e:
        rep.check(False, f"cross-tenant create raised {e.__class__.__name__}, want NO_PERMISSION")

    # `retire` really destroys — measured by the peer, on a reference it holds.
    acme.retire(model)
    rep.op("retire")
    try:
        model.get_manifest()
        rep.check(False, "a retired model should not answer")
    except CORBA.OBJECT_NOT_EXIST:
        rep.check(True, "after retire() the peer's own reference is OBJECT_NOT_EXIST")
    except CORBA.Exception as e:
        rep.check(False, f"retired model raised {e.__class__.__name__}, want OBJECT_NOT_EXIST")

    # The peer's own _is_a, on a neighbouring interface: "16 served" has to
    # mean five distinct objects, not one with a union of sixteen operations.
    try:
        wrong = orb.string_to_object(
            corbaloc(endpoint, KEY_POLICY.format(tenant="acme", domain="acme-default"))
        )._narrow(moe.enterprise.ModelFactory)
        rep.check(wrong is None, "a PolicyDomain does not narrow to a ModelFactory")
    except CORBA.Exception as e:
        rep.check(False, f"narrow probe raised {e.__class__.__name__}")

    declared = {
        "create", "clone_model", "retire", "deploy",
        "get_manifest", "infer", "bind_expert", "set_policy",
        "authorize", "check_residency", "audit",
        "get_tenant_id", "base", "adapter_delta",
        "describe", "process",
    }
    missing = sorted(declared - rep.served)
    rep.check(
        not missing,
        "every declared operation was called by the peer"
        + ("" if not missing else f": missing {missing}"),
    )
    print(f"\ndeclared {len(declared)} · called by the peer {len(rep.served & declared)}")
    print(
        "note: EnterpriseExpert and PolicyDomain references were built from the\n"
        "      documented key template, because no operation of this contract\n"
        "      returns one — `bind_expert` and `set_policy` take arguments the\n"
        "      wire never hands out."
    )

    orb.shutdown(True)
    if rep.failures:
        print(f"\nf5-peer: FAIL — {rep.failures} check(s) failed")
        return 1
    print("\nf5-peer: PASS")
    return 0


def corbaloc(endpoint, key):
    """A corbaloc URL for one of our object keys.

    The key is passed through as-is: our keys are ASCII and use `/` as their
    own separator, which corbaloc's key-string production allows because
    everything after the first `/` is the key.
    """
    return f"corbaloc:iiop:1.2@{endpoint}/{key}"


def endpoint_of(stringified):
    """host:port out of a stringified IOR.

    Only the endpoint, and only to build the corbaloc URLs for the objects the
    contract returns no reference for.  Every *invocation* below goes through
    omniORB's own stubs and its own CDR — this reads the address and nothing
    else, deliberately, because a second IOR parser in this file would start
    agreeing with ours instead of checking it.
    """
    raw = bytes.fromhex(stringified[4:])
    # Located by scanning for the printable dotted quad rather than by offset
    # arithmetic: the profile's leading fields are exactly what this script
    # must not re-implement.
    m = re.search(rb"((?:\d{1,3}\.){3}\d{1,3})\x00", raw)
    if not m:
        leave("f5-peer: BLOCKED — no IIOP host in the IOR")
    host = m.group(1).decode()
    after = m.end()
    after += (-after) % 2
    port = int.from_bytes(raw[after:after + 2], "little")
    return f"{host}:{port}"


if __name__ == "__main__":
    leave(main(sys.argv[1:]))