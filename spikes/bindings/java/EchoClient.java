// A generated Java client against a live peer, through orbweaver-py-bridge.
//
//     java -cp <classes> EchoClient <echo.idl> <echo.ior> <bridge-binary>
//
// Nothing here is generated: this is the *test* of the target, and a test the
// generator writes for itself proves nothing — the same rule `gen-corpus`'s
// oracle main and `echo_client.py` follow. It is compiled against what
// `gen-java --package echo spikes/echo.idl` wrote.
//
// The peer is a stock ORB in another process — omniORB's `echo_server.py` or
// JacORB's `Server` — and this process does not know which. It speaks to
// `orbweaver-py-bridge`, which speaks GIOP. **No `org.omg.CORBA` is imported,
// linked or shipped**: JDK 11 removed one (JEP 320), the only one on this
// machine is JacORB's jar, and JacORB is an LGPL fixture rather than a
// dependency. The classpath here is the generated tree and nothing else.
//
// Exit code is the verdict: 0 when every case passed.

import echo._Rt;
import echo.spike.Echo;
import echo.spike.Ragged;

public final class EchoClient {
    private EchoClient() {}

    static int fails = 0;

    static void ok(String _what, Object _got) {
        System.out.println("  ok   " + _what + " -> " + _Rt._show(_got));
    }

    static void check(String _what, Object _got, Object _want) {
        if (_Rt._eq(_got, _want)) {
            ok(_what, _got);
        } else {
            System.out.println("  FAIL " + _what + " -> " + _Rt._show(_got)
                    + ", wanted " + _Rt._show(_want));
            fails++;
        }
    }

    public static void main(String[] _argv) {
        if (_argv.length < 3) {
            System.out.println("usage: EchoClient <echo.idl> <echo.ior> <bridge>");
            System.exit(2);
        }
        // `--no-wide` omits the one case the wire refuses below GIOP 1.2. Off by
        // default, so every cell that does not pass it drives what it always
        // drove. Added 2026-09-03 so the client direction can READ GIOP 1.0.
        boolean _narrow = false;
        for (String _a : _argv) {
            if (_a.equals("--no-wide")) {
                _narrow = true;
            }
        }
        String _idl = _argv[0];
        String _ior = _argv[1];
        String _bridge = _argv[2];

        try (_Rt.Bridge _conn = _Rt.Bridge.connect(_bridge, _idl, _ior)) {
            Echo _echo = new Echo(_conn);

            check("ping()", Integer.valueOf(_echo.ping()), Integer.valueOf(42));
            check("add(1000000, 337)", Integer.valueOf(_echo.add(1000000, 337)),
                    Integer.valueOf(1000337));
            check("echo_string('generated java')", _echo.echo_string("generated java"),
                    "generated java");
            check("scale(1.5, 4.0)", Double.valueOf(_echo.scale(1.5, 4.0)),
                    Double.valueOf(6.0));

            // Every alignment rule at once, as a Java object with named members
            // — one of which is called `e`, which is the name that made
            // JacORB's own generated stub fail to compile.
            Ragged _ragged = new Ragged((byte) 0xAA, -7, (short) 9, 2.5, (byte) 0xBB);
            check("echo_ragged(Ragged(...))", _echo.echo_ragged(_ragged), _ragged);

            // `wstring`: the codeset path, which the wire decides and the
            // client never sees. Skipped under `--no-wide`, and only there:
            // GIOP 1.0 cannot carry it (§9.3.1.6) and refusing is correct, so a
            // 1.0 pass that drove it would measure our own refusal instead of
            // the peer's flag byte.
            if (!_narrow) {
                check("echo_wstring('정적 스텁')", _echo.echo_wstring("정적 스텁"), "정적 스텁");
            }

            // `sequence<octet>` is a byte[] on this side and base64 across the
            // seam.
            byte[] _blob = new byte[64];
            int _sum = 0;
            for (int _i = 0; _i < 64; _i++) {
                _blob[_i] = (byte) (_i % 251);
                _sum += _blob[_i] & 0xFF;
            }
            check("blob(64)", _echo.blob(64L), _blob);
            check("blob_sum(blob)", Integer.valueOf(_echo.blob_sum(_blob)),
                    Integer.valueOf(_sum % 2147483647));

            // An `any` carries its own type. This runtime constructs one whose
            // type has a simple name and relays any other unopened, which is
            // the scope its class comment states.
            _Rt.Any _any = _Rt.Any.of(_Rt.DOUBLE, Double.valueOf(-0.125));
            _Rt.Any _back = _echo.echo_any(_any);
            check("echo_any(any(double, -0.125))", _back.open(), Double.valueOf(-0.125));

            // An object reference crosses as a handle (§4.7): never dialable,
            // and usable as an argument back through the bridge that issued it.
            _Rt.ObjectRef _me = _echo.get_self();
            check("get_self() is a handle", Boolean.valueOf(_me != null), Boolean.TRUE);
            check("same_as(get_self())", Boolean.valueOf(_echo.same_as(_me)), Boolean.TRUE);

            // The refusal path: a handle nobody issued cannot become an
            // address.
            try {
                _echo.same_as(new _Rt.ObjectRef("local-9999"));
                System.out.println("  FAIL a forged handle was accepted");
                fails++;
            } catch (_Rt.Error _e) {
                String _m = String.valueOf(_e.getMessage());
                System.out.println("  ok   a forged handle is refused: "
                        + _m.substring(0, Math.min(70, _m.length())));
            }
        } catch (Throwable _t) {
            System.out.println("  FAIL the client did not complete: " + _t);
            _t.printStackTrace(System.out);
            fails++;
        }

        System.out.println();
        System.out.println("java target: " + (fails == 0 ? "PASS" : "FAIL — " + fails + " case(s)"));
        System.exit(fails == 0 ? 0 : 1);
    }
}
