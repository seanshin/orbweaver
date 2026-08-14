"""A generated Python client against a live peer, through orbweaver-py-bridge.

    python3 echo_client.py <package-root> <echo.idl> <echo.ior> <bridge-binary>

The package root is what `gen-python --out` wrote. Nothing here is generated:
this is the *test* of the target, and a test the generator writes for itself
proves nothing — the same rule `gen-corpus`'s oracle main follows.

The peer is the stock omniORB fixture in `spikes/`. It is a wire peer and
nothing else: no part of omniORB is imported, linked or shipped, and this
process does not know omniORB exists — it speaks to `orbweaver-py-bridge`,
which speaks GIOP.

Exit code is the verdict: 0 when every case passed.
"""
import sys

root, idl, ior, bridge = sys.argv[1:5]
sys.path.insert(0, root)

from echo import _rt          # noqa: E402  the generated package's runtime
from echo import spike        # noqa: E402  IDL module `spike`

fails = 0


def case(what, got, want):
    global fails
    if got == want:
        print("  ok   %s -> %r" % (what, got))
    else:
        print("  FAIL %s -> %r, wanted %r" % (what, got, want))
        fails += 1


with _rt.connect(idl, ior, command=[bridge]) as conn:
    echo = spike.Echo(conn)

    case("ping()", echo.ping(), 42)
    case("add(1000000, 337)", echo.add(1000000, 337), 1000337)
    case("echo_string('generated python')",
         echo.echo_string("generated python"), "generated python")
    case("scale(1.5, 4.0)", echo.scale(1.5, 4.0), 6.0)

    # Every alignment rule at once, as a Python object with named members.
    ragged = spike.Ragged(0xAA, -7, 9, 2.5, 0xBB)
    case("echo_ragged(Ragged(...))", echo.echo_ragged(ragged), ragged)

    # `wstring`: the codeset path, which is the one the wire decides and the
    # client never sees.
    case("echo_wstring('정적 스텁')", echo.echo_wstring("정적 스텁"), "정적 스텁")

    # `sequence<octet>` is bytes on this side and base64 across the seam.
    blob = bytes((i % 251) for i in range(64))
    case("blob(64)", echo.blob(64), blob)
    case("blob_sum(blob)", echo.blob_sum(blob), sum(blob) % 2147483647)

    # An `any` carries its own type: (descriptor, value).
    case("echo_any((double, -0.125))",
         echo.echo_any(("double", -0.125)), ("double", -0.125))

    # An object reference crosses as a handle (§4.7): never dialable, and
    # usable as an argument back through the bridge that issued it.
    me = echo.get_self()
    case("get_self() is a handle", isinstance(me, _rt.ObjectRef), True)
    case("same_as(get_self())", echo.same_as(me), True)

    # The refusal path: a handle nobody issued cannot become an address.
    try:
        echo.same_as(_rt.ObjectRef("local-9999"))
        print("  FAIL a forged handle was accepted")
        fails += 1
    except _rt.Error as e:
        print("  ok   a forged handle is refused: %s" % (str(e)[:70],))

print("\npython target: %s" % ("PASS" if fails == 0 else "FAIL — %d case(s)" % fails))
sys.exit(1 if fails else 0)
