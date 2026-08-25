#!/usr/bin/env python3
"""An omniORB PullSupplier that OUR CosEvent channel comes and fetches from.

The mirror of `event_consumer.py`, and the peer oracle for the supplier side
of the pull model. `event_consumer.py` measures the direction where our channel
*calls* an ORB we did not write; this one measures the direction where our
channel is a **client** of one — it dials the servant below and invokes
`try_pull` on it, on a schedule it owns.

That is the half no self-test can establish. A convention both ends apply
cannot be refuted by a round trip: our own `PullSupplierServant` and our own
channel were written from the same reading of the same chapter, so if the
reading is wrong they agree anyway. omniORB read the chapter separately.

Usage: event_pull_supplier.py <ior-file>   (against `spike-events <ior> --hold`)

Prints PASS when every event this supplier offered came back through the
channel to a consumer of omniORB's own — both consumer models, because the
supplier side of pull is what unblocked pull/push *and* pull/pull.

Run it twice, against `--source-endian big` and `--source-endian little`: the
channel asks in the order it is told to and a supplier replies in the order it
was asked in, so that flag is what puts both byte orders on the wire.
"""

import sys
import time

from omniORB import CORBA, PortableServer
import CosEventChannelAdmin  # noqa: F401  (registers the stubs)
import CosEventComm  # noqa: F401
import CosEventComm__POA

# Distinct from the --hold ticker's small ulongs, so "did OUR events come
# back?" is answerable rather than "did some event come back?".
BASE = 0xBEE0
COUNT = 5


class Supplier(CosEventComm__POA.PullSupplier):
    """Holds what it was given until the channel comes and asks."""

    def __init__(self, values):
        self.pending = list(values)
        self.pull_calls = 0
        self.try_pull_calls = 0

    def try_pull(self):
        self.try_pull_calls += 1
        if not self.pending:
            return (CORBA.Any(CORBA.TC_null, None), False)
        return (CORBA.Any(CORBA.TC_ulong, self.pending.pop(0)), True)

    def pull(self):
        # Counted, not served in a hurry: the channel is expected never to call
        # this, and a count is how that stops being a claim. `pull` is
        # specified to block until there is something, so blocking is correct
        # here — and if our channel ever calls it, the count says so.
        self.pull_calls += 1
        while not self.pending:
            time.sleep(0.05)
        return CORBA.Any(CORBA.TC_ulong, self.pending.pop(0))

    def disconnect_pull_supplier(self):
        self.pending = []


class Consumer(CosEventComm__POA.PushConsumer):
    def __init__(self):
        self.got = []

    def push(self, data):
        self.got.append(data.value(CORBA.TC_ulong))

    def disconnect_push_consumer(self):
        pass


def ours(values):
    """Only the events this script offered — the ticker's are not evidence."""
    return [v for v in values if v is not None and BASE <= v < BASE + COUNT]


def main(argv):
    ior_path = argv[1] if len(argv) > 1 else "spikes/events.ior"
    orb = CORBA.ORB_init(argv)
    poa = orb.resolve_initial_references("RootPOA")
    poa._get_the_POAManager().activate()

    channel = orb.string_to_object(open(ior_path).read().strip())
    channel = channel._narrow(CosEventChannelAdmin.EventChannel)
    if channel is None:
        print("FAIL the IOR did not narrow to CosEventChannelAdmin::EventChannel")
        return 1

    # ── the consumer side first: a fan-out reaches only what is connected ──
    consumer = Consumer()
    push_proxy = channel.for_consumers().obtain_push_supplier()
    push_proxy.connect_push_consumer(consumer._this())

    pull_proxy = channel.for_consumers().obtain_pull_supplier()
    if pull_proxy is None:
        print("FAIL obtain_pull_supplier returned nil")
        return 1
    pull_proxy.connect_pull_consumer(None)

    # ── the supplier side: the operation this whole check exists for ──
    supplier = Supplier(range(BASE, BASE + COUNT))
    supplier_admin = channel.for_suppliers()
    try:
        proxy = supplier_admin.obtain_pull_consumer()
    except CORBA.NO_IMPLEMENT:
        print("FAIL obtain_pull_consumer answered NO_IMPLEMENT — the channel cannot pull")
        return 1
    if proxy is None:
        print("FAIL obtain_pull_consumer returned nil")
        return 1
    proxy = proxy._narrow(CosEventChannelAdmin.ProxyPullConsumer)
    if proxy is None:
        print("FAIL the reference did not narrow to ProxyPullConsumer")
        return 1
    proxy.connect_pull_supplier(supplier._this())
    # Published for the teardown below. `main` has seven return paths and the
    # channel starts asking at the line above, so every one of them from here
    # on has to stop it asking — a `finally` around one of them would leave the
    # other six aborting. See `stop_being_a_supplier`.
    global _CONNECTED_PROXY  # noqa: PLW0603 — a fixture, and the alternative is six call sites
    _CONNECTED_PROXY = proxy

    # A sleeping, deadline-bounded wait. A loop with no sleep is the Phase 0
    # wait loop that finishes in microseconds and does not wait at all.
    deadline = time.time() + 20.0
    pulled = []
    while time.time() < deadline:
        if len(ours(consumer.got)) >= COUNT and len(pulled) >= COUNT:
            break
        try:
            value, has_event = pull_proxy.try_pull()
        except CORBA.Exception as exc:  # noqa: BLE001 — reported, not hidden
            print("FAIL try_pull on the channel raised", exc)
            return 1
        if has_event:
            try:
                v = value.value(CORBA.TC_ulong)
            except Exception:  # noqa: BLE001 — the ticker sends ulongs too
                v = None
            if v is not None and BASE <= v < BASE + COUNT:
                pulled.append(v)
        time.sleep(0.1)

    pushed = ours(consumer.got)
    want = list(range(BASE, BASE + COUNT))
    print("offered:  ", want)
    print("pushed to omniORB's PushConsumer:", pushed)
    print("pulled by omniORB from the channel:", pulled)
    print("supplier was asked: try_pull=%d pull=%d" % (supplier.try_pull_calls, supplier.pull_calls))

    if supplier.pull_calls != 0:
        print("FAIL the channel called the blocking pull; it must use try_pull")
        return 1
    if pushed != want:
        print("FAIL the push consumer did not receive what the supplier offered, in order")
        return 1
    if pulled != want:
        print("FAIL the pull consumer did not receive what the supplier offered, in order")
        return 1
    print("PASS")
    return 0


def stop_being_a_supplier(proxy):
    """Tell the channel to stop asking, before this interpreter goes away.

    Measured 2026-08-25 on CI (Linux; it had never fired on macOS): the script
    printed `PASS` and every value matched, then exited **134** — SIGABRT. Our
    channel polls `try_pull` every `DEFAULT_SOURCE_POLL`, and a call that lands
    while CPython is tearing down finds module globals already cleared, so
    `CORBA.Any` raises `AttributeError`. omniORB sees a servant method raise
    something that is not a CORBA exception, prints `FATAL: exception not
    rethrown`, and aborts the process.

    **The measurement was sound and the process still died**, which is exactly
    why this group reads the exit code instead of grepping for `PASS`: a group
    that matched the word would have been green over an aborting fixture. The
    repair is the protocol's own: `disconnect_pull_consumer` is what a supplier
    that is going away is supposed to say, and saying it also exercises the
    third of the three operations this fixture exists to measure.
    """
    try:
        proxy.disconnect_pull_consumer()
    except Exception as exc:  # noqa: BLE001 — reported; teardown must not mask a result
        print("note teardown: disconnect_pull_consumer raised", exc)


#: The channel's `ProxyPullConsumer` once connected, or `None`.
_CONNECTED_PROXY = None

if __name__ == "__main__":
    rc = main(sys.argv)
    if _CONNECTED_PROXY is not None:
        stop_being_a_supplier(_CONNECTED_PROXY)
    sys.exit(rc)
