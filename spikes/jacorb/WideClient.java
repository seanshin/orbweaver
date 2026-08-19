// A JacORB client for spikes/wide.idl: one wchar per argument, echoed and
// compared as a UTF-16 code unit. TEST FIXTURE — see Client.java; JacORB is
// LGPL and runs as a separate process, never linked.
//
// D010 B5, second half. Java's `char` is one UTF-16 code unit, which is
// exactly what a GIOP 1.1 `wchar` is under UTF-16 — so a character above the
// BMP cannot be *asked for* here at all: the nearest thing is a lone surrogate,
// and passing one is a measurement of what JacORB's writer does with it, not a
// pass or a fail. Every unit is an argument in hex so that this file's
// encoding takes no part in the octets.
//
// Like Client11 this asserts nothing about the version it spoke: JacORB
// follows the profile it dials, and the server on the other end
// (spikes/jacorb_wchar11.py) reads the version and the octets off the wire.
//
// Usage: java WideClient <ior-file> <hex-unit>...      e.g. D55C FEFF 0077 D83D
import java.nio.file.*;
import java.util.Properties;
import org.omg.CORBA.ORB;
import spike.*;

public class WideClient {
    static int fails = 0;
    static int asserted = 0;

    public static void main(String[] args) throws Exception {
        Properties p = new Properties();
        p.setProperty("org.omg.CORBA.ORBClass", "org.jacorb.orb.ORB");
        p.setProperty("org.omg.CORBA.ORBSingletonClass", "org.jacorb.orb.ORBSingleton");

        ORB orb = ORB.init(args, p);
        String ior = Files.readString(Path.of(args[0])).trim();
        Wide wide = WideHelper.narrow(orb.string_to_object(ior));
        if (wide == null) { System.out.println("FAIL narrow returned nil"); System.exit(1); }

        for (int i = 1; i < args.length; i++) {
            char sent = (char) Integer.parseInt(args[i], 16);
            String label = String.format("echo_wchar[U+%04X]", (int) sent);
            asserted++;
            try {
                char back = wide.echo_wchar(sent);
                if (back == sent) {
                    System.out.println(String.format("  ok   %s -> U+%04X", label, (int) back));
                } else {
                    System.out.println(String.format("  FAIL %s -> U+%04X", label, (int) back));
                    fails++;
                }
            } catch (org.omg.CORBA.SystemException e) {
                System.out.println("  FAIL " + label + " raised " + e.getClass().getName()
                                   + " minor=0x" + Integer.toHexString(e.minor));
                fails++;
            }
        }

        System.out.println("\nasserted cases: " + asserted + ", failures: " + fails);
        System.exit(fails == 0 ? 0 : 1);
    }
}
