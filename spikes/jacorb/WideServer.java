// A JacORB server for spikes/wide.idl — the single wide character, echoed.
// TEST FIXTURE — see Client.java; JacORB is LGPL and runs as a separate
// process, never linked.
//
// D010 B5, second half. echo.idl has a wstring and no wchar, so nothing in
// the tree had ever put a GIOP 1.1 `wchar` (two octets, no length indication,
// nowhere for a mark) in front of a peer. Started with
// -Djacorb.giop_minor_version=1 this server's IOR advertises IIOP 1.1, and a
// client that follows the profile — ours does — speaks 1.1 to it. The IOR
// version is not asserted here; spikes/jacorb_wchar11.py reads it from the
// bytes and asserts the reply headers on top.
//
// Both operations are echoes, deliberately: an echo puts JacORB's decoder and
// its encoder under test in one exchange, and what JacORB *read* is visible
// in what it wrote back.
//
// Usage: java [-Djacorb.giop_minor_version=1] WideServer <ior-out-file>
import java.nio.file.*;
import java.util.Properties;
import org.omg.CORBA.ORB;
import org.omg.PortableServer.POA;
import org.omg.PortableServer.POAHelper;
import spike.*;

public class WideServer {
    static class WideImpl extends WidePOA {
        public char echo_wchar(char c) { return c; }
        public String echo_wstring(String s) { return s; }
    }

    public static void main(String[] args) throws Exception {
        Properties p = new Properties();
        p.setProperty("org.omg.CORBA.ORBClass", "org.jacorb.orb.ORB");
        p.setProperty("org.omg.CORBA.ORBSingletonClass", "org.jacorb.orb.ORBSingleton");

        ORB orb = ORB.init(args, p);
        POA poa = POAHelper.narrow(orb.resolve_initial_references("RootPOA"));
        poa.the_POAManager().activate();

        org.omg.CORBA.Object ref = poa.servant_to_reference(new WideImpl());
        Path out = Path.of(args[args.length - 1]);
        Files.writeString(out, orb.object_to_string(ref));
        System.out.println("IOR written to " + out);
        System.out.println("READY");
        System.out.flush();
        orb.run();
    }
}
