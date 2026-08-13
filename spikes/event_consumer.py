#!/usr/bin/env python3
"""An omniORB push consumer attached to our CosEvent channel.

The independent half of F7: our channel is written from the OMG specification,
so the check that matters is an ORB we did not write narrowing it, connecting
its own servant, and decoding what we push.

Usage: event_consumer.py <ior-file>   (against `spike-events <ior> --hold`)

Prints PASS when at least one event arrives. Our server handles one connection
at a time, so this must be the only client while it runs.
"""

import sys
import time

from omniORB import CORBA
import CosEventChannelAdmin  # noqa: F401  (registers the stubs)
import CosEventComm  # noqa: F401
import CosEventComm__POA


class Consumer(CosEventComm__POA.PushConsumer):
    def __init__(self):
        self.got = []

    def push(self, data):
        self.got.append(data.value(CORBA.TC_ulong))

    def disconnect_push_consumer(self):
        pass


def main(argv):
    ior_path = argv[1] if len(argv) > 1 else "spikes/events.ior"
    orb = CORBA.ORB_init(argv)
    poa = orb.resolve_initial_references("RootPOA")
    poa._get_the_POAManager().activate()
    servant = Consumer()

    channel = orb.string_to_object(open(ior_path).read().strip())
    channel = channel._narrow(CosEventChannelAdmin.EventChannel)
    if channel is None:
        print("FAIL the IOR did not narrow to CosEventChannelAdmin::EventChannel")
        return 1

    proxy = channel.for_consumers().obtain_push_supplier()
    proxy.connect_push_consumer(servant._this())

    # --hold pushes a ulong once a second; wait with a deadline, do not spin.
    deadline = time.time() + 10.0
    while not servant.got and time.time() < deadline:
        time.sleep(0.2)

    print("received:", servant.got)
    if not servant.got:
        print("FAIL no event arrived from the held channel")
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
