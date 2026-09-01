import cpeer._Rt;
import cpeer.CPeerEcho;

/**
 * Calls `add` on the C peer's server role, through a generated Java client.
 *
 * Deliberately tiny: the cell it serves measures that generated code can reach a
 * program that speaks GIOP and links no ORB. Every conversion and the whole call
 * path belong to the generated package; no `org.omg.CORBA` is in sight, and
 * cannot be — JDK 11 removed it (JEP 320) and the only one on this machine is
 * JacORB's jar, which this does not put on its classpath.
 */
public final class CPeerAdd {
    /** argv: idl, ior, bridge — the order the other cells pass them in. */
    public static void main(String[] argv) throws Exception {
        try (_Rt.Bridge bridge = _Rt.Bridge.connect(argv[2], argv[0], argv[1])) {
            CPeerEcho echo = new CPeerEcho(bridge);
            int got = echo.add(40, 2);
            System.out.println("  ok   add(40, 2) -> " + got);
            System.out.println("java cpeer: " + (got == 42 ? "PASS" : "FAIL"));
            System.exit(got == 42 ? 0 : 1);
        }
    }
}
