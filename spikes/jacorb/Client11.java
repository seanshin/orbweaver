// A JacORB client whose GIOP minor version is chosen on the command line, so
// that the Rust server can be reached at GIOP 1.1 by a peer we did not write.
// TEST FIXTURE — see Client.java; JacORB is LGPL and runs as a separate
// process, never linked.
//
// D010 B5. Every other JacORB group runs at JacORB's default of 1.2, so until
// this file existed nothing in the tree had driven JacORB at 1.1 in either
// direction, and 1.1 is the version whose wide-character rule differs (no
// per-character length, count in wide characters plus a terminator). This
// client asserts nothing about the version it spoke: it cannot see its own
// wire, and a JacORB that ignored the property would still pass here. The
// version is measured by whoever reads the bytes — spikes/jacorb_giop11.sh,
// from the Rust server's "first request at GIOP x.y" line and from the tap
// that records the messages.
//
// Usage: java [-Djacorb.giop_minor_version=1] Client11 <ior-file> [text]...
//
// The wide text is an argument rather than a literal because the octets are
// the measurement, and a literal would make this file's encoding part of it.
// Each text is round-tripped through echo_wstring and compared as a decoded
// String; the code points are printed so a wrong answer is legible without a
// hex dump.
import java.nio.file.*;
import java.util.Properties;
import org.omg.CORBA.ORB;
import spike.*;

public class Client11 {
    static int fails = 0;
    static int asserted = 0;

    static String codepoints(String s) {
        StringBuilder b = new StringBuilder();
        s.codePoints().forEach(cp -> b.append(String.format("U+%04X ", cp)));
        return b.toString().trim();
    }

    static void check(String label, Object got, Object want) {
        asserted++;
        if (got.equals(want)) System.out.println("  ok   " + label + " -> " + got);
        else { System.out.println("  FAIL " + label + " -> " + got + ", expected " + want); fails++; }
    }

    public static void main(String[] args) throws Exception {
        Properties p = new Properties();
        p.setProperty("org.omg.CORBA.ORBClass", "org.jacorb.orb.ORB");
        p.setProperty("org.omg.CORBA.ORBSingletonClass", "org.jacorb.orb.ORBSingleton");
        // Printed, not asserted: this is what was *asked* of JacORB. What it
        // *did* is on the wire, and only the other end can say.
        String minor = System.getProperty("jacorb.giop_minor_version");
        System.out.println("jacorb.giop_minor_version=" + (minor == null ? "(unset)" : minor));

        ORB orb = ORB.init(args, p);
        String ior = Files.readString(Path.of(args[0])).trim();
        Echo echo = EchoHelper.narrow(orb.string_to_object(ior));
        if (echo == null) { System.out.println("FAIL narrow returned nil"); System.exit(1); }

        check("ping()", echo.ping(), 42);
        check("echo_string(\"narrow probe\")", echo.echo_string("narrow probe"), "narrow probe");

        for (int i = 1; i < args.length; i++) {
            String text = args[i];
            asserted++;
            try {
                String back = echo.echo_wstring(text);
                if (back.equals(text)) {
                    System.out.println("  ok   echo_wstring[" + i + "] -> " + codepoints(back));
                } else {
                    System.out.println("  FAIL echo_wstring[" + i + "] -> " + codepoints(back)
                                       + ", expected " + codepoints(text));
                    fails++;
                }
            } catch (org.omg.CORBA.SystemException e) {
                System.out.println("  FAIL echo_wstring[" + i + "] raised " + e.getClass().getName()
                                   + " minor=0x" + Integer.toHexString(e.minor));
                fails++;
            }
        }

        System.out.println("\nasserted cases: " + asserted + ", failures: " + fails);
        System.exit(fails == 0 ? 0 : 1);
    }
}
