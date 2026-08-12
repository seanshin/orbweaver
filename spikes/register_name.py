#!/usr/bin/env python3
"""Binds the omniORB echo fixture into omniNames under spike/Echo.

TEST FIXTURE. See docs/PLAN.md section 10.
"""
import pathlib, sys, omniORB
from omniORB import CORBA, PortableServer
import CosNaming

HERE = pathlib.Path(__file__).parent
omniORB.importIDL(str(HERE / "echo.idl"))
import spike, spike__POA  # noqa: E402


class Echo(spike__POA.Echo):
    def ping(self): return 42
    def add(self, a, b): return a + b
    def echo_string(self, m): return m
    def scale(self, v, by): return v * by
    def echo_ragged(self, v): return v
    def echo_any(self, v): return v


def main():
    orb = CORBA.ORB_init(sys.argv, CORBA.ORB_ID)
    poa = orb.resolve_initial_references("RootPOA")
    ref = Echo()._this()
    poa._get_the_POAManager().activate()

    root = orb.string_to_object("corbaloc::127.0.0.1:2809/NameService")._narrow(
        CosNaming.NamingContext)
    if root is None:
        print("could not reach the naming service", file=sys.stderr); sys.exit(1)

    ctx_name = [CosNaming.NameComponent("spike", "")]
    try:
        ctx = root.bind_new_context(ctx_name)
    except CosNaming.NamingContext.AlreadyBound:
        ctx = root.resolve(ctx_name)._narrow(CosNaming.NamingContext)
    obj_name = [CosNaming.NameComponent("Echo", "")]
    try:
        ctx.bind(obj_name, ref)
    except CosNaming.NamingContext.AlreadyBound:
        ctx.rebind(obj_name, ref)

    print("bound spike/Echo into the naming service", flush=True)
    print("READY", flush=True)
    orb.run()


if __name__ == "__main__":
    main()
