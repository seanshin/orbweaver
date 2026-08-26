#!/usr/bin/env python3
"""An omniORB consumer that finds our event channel by NAME, never by address.

D021 §3 settles registration as CosNaming; D029 §6.1's Location row says the
caller must not be able to tell where the target runs. `channel_found_by_name.rs`
measures that with our client at both ends, which is a self-test — a convention
both ends apply cannot be refuted by a round trip. This is the half that can
refute it: an ORB we did not write, resolving the name out of our naming server
and reaching a channel whose address it was never told.

Usage:
    cargo run -q -p orbweaver-giop --bin spike-channel-by-name -- \\
        /tmp/channel-names.ior --hold &
    python3 spikes/event_by_name_client.py /tmp/channel-names.ior

The argument is the NAMING server's IOR. That is the whole point: the channel's
IOR is never written to disk and never passed here. The only other thing this
client is given is the channel's name, and it builds that name with the mapping
the Rust side documents on `CHANNEL_BINDING_KIND` --- one component,
`{id: <channel>, kind: "EventChannel"}`.

Prints PASS when an event arrives over a reference this client resolved.
"""

import sys
import time

from omniORB import CORBA
import CosNaming
import CosEventChannelAdmin  # noqa: F401  (registers the stubs)
import CosEventComm  # noqa: F401
import CosEventComm__POA

# The mapping, retyped here on purpose and for exactly one reason: this is the
# foreign half. If it computed the name by calling our code it would not be
# measuring whether a peer can construct the name from the documentation --- it
# would be measuring that our function equals itself. A drift between this
# literal and `CHANNEL_BINDING_KIND` is a real finding about the documentation,
# which is why the fixture prints the name it published and this prints the name
# it asked for.
CHANNEL_BINDING_KIND = "EventChannel"


class Consumer(CosEventComm__POA.PushConsumer):
    def __init__(self):
        self.got = []

    def push(self, data):
        self.got.append(data.value(CORBA.TC_ulong))

    def disconnect_push_consumer(self):
        pass


def main(argv):
    ns_path = argv[1] if len(argv) > 1 else "spikes/channel-names.ior"
    channel_name = argv[2] if len(argv) > 2 else "alerts"

    orb = CORBA.ORB_init(argv)
    poa = orb.resolve_initial_references("RootPOA")
    poa._get_the_POAManager().activate()
    servant = Consumer()

    # ── step 1: the naming context. The only address this client is given. ──
    root = orb.string_to_object(open(ns_path).read().strip())
    root = root._narrow(CosNaming.NamingContextExt)
    if root is None:
        print("FAIL the IOR did not narrow to CosNaming::NamingContextExt")
        return 1
    print("narrowed to CosNaming::NamingContextExt")

    # ── step 2: resolve the name. Nothing here carries a host or a port. ──
    name = [CosNaming.NameComponent(channel_name, CHANNEL_BINDING_KIND)]
    print("resolving %s.%s" % (channel_name, CHANNEL_BINDING_KIND))
    try:
        ref = root.resolve(name)
    except CosNaming.NamingContext.NotFound as e:
        print("FAIL the channel's name is not bound:", e)
        return 1

    channel = ref._narrow(CosEventChannelAdmin.EventChannel)
    if channel is None:
        print("FAIL what the name resolved to did not narrow to "
              "CosEventChannelAdmin::EventChannel")
        return 1
    print("narrowed to CosEventChannelAdmin::EventChannel")

    # What the peer learned about the location, and only now --- printed so the
    # report can say what a client can still tell. See the Rust test's `Observed`,
    # which deliberately has no address field.
    print("the address this client learned only by resolving:",
          orb.object_to_string(ref)[:24] + "...")

    # ── step 3: attach and receive ──
    proxy = channel.for_consumers().obtain_push_supplier()
    proxy.connect_push_consumer(servant._this())

    # --hold pushes a ulong once a second; wait with a deadline, do not spin.
    deadline = time.time() + 10.0
    while not servant.got and time.time() < deadline:
        time.sleep(0.2)

    print("received:", servant.got)
    if not servant.got:
        print("FAIL no event arrived over a reference this client resolved")
        return 1

    # ── the control: the leak, in the peer's own terms. ──
    #
    # A name that was never published must not resolve. Without this, a naming
    # server that answered anything for any name would satisfy everything above,
    # because the reference it handed back is the one this client then dialled.
    try:
        root.resolve([CosNaming.NameComponent("no-such-channel",
                                              CHANNEL_BINDING_KIND)])
        print("FAIL the control did not move: an unpublished name resolved, so "
              "the resolve above is evidence of nothing")
        return 1
    except CosNaming.NamingContext.NotFound:
        print("  ok   an unpublished name does not resolve")

    print("PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
