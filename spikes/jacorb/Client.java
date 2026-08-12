// Second interoperability peer: a JacORB client calling the Rust server.
//
// TEST FIXTURE. JacORB is LGPL and is never linked into Orbweaver; it talks to
// our server over TCP using the published GIOP specification. See PLAN 10.
//
// Every prior interop result carried the caveat "proves compatibility with
// omniORB". An independent implementation is the only thing that removes it.
import java.nio.file.*;
import java.util.Properties;
import org.omg.CORBA.ORB;
import spike.*;

public class Client {
    static int fails = 0;

    static void check(String label, Object got, Object want) {
        if (got.equals(want)) System.out.println("  ok   " + label + " -> " + got);
        else { System.out.println("  FAIL " + label + " -> " + got + ", expected " + want); fails++; }
    }

    public static void main(String[] args) throws Exception {
        Properties p = new Properties();
        p.setProperty("org.omg.CORBA.ORBClass", "org.jacorb.orb.ORB");
        p.setProperty("org.omg.CORBA.ORBSingletonClass", "org.jacorb.orb.ORBSingleton");

        ORB orb = ORB.init(args, p);
        String ior = Files.readString(Path.of(args[args.length - 1])).trim();
        Echo echo = EchoHelper.narrow(orb.string_to_object(ior));
        if (echo == null) { System.out.println("FAIL narrow returned nil"); System.exit(1); }

        check("ping()", echo.ping(), 42);
        check("add(1000000, 337)", echo.add(1000000, 337), 1000337);
        check("echo_string(...)", echo.echo_string("hello from JacORB"), "hello from JacORB");
        check("scale(1.5, 4.0)", echo.scale(1.5, 4.0), 6.0);

        Ragged r = new Ragged((byte) 0xAA, -7, (short) 9, 2.5, (byte) 0xBB);
        Ragged back = echo.echo_ragged(r);
        boolean raggedOk = back.a == (byte) 0xAA && back.b == -7 && back.c == 9
                        && back.d == 2.5 && back.e == (byte) 0xBB;
        if (raggedOk) System.out.println("  ok   echo_ragged() preserved struct padding");
        else { System.out.println("  FAIL echo_ragged() -> " + back.a + "," + back.b + "," + back.c + "," + back.d + "," + back.e); fails++; }

        System.out.println("\nasserted cases: 5, failures: " + fails);
        System.exit(fails == 0 ? 0 : 1);
    }
}
