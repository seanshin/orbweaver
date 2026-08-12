// Second interoperability peer, other direction: a JacORB server for the Rust
// client to call. TEST FIXTURE — see Client.java.
//
// Every operation is an echo, deliberately: an echo puts the peer's decoder and
// encoder both under test, so a round-trip failure means our bytes were wrong
// rather than the peer's logic being wrong.
import java.nio.file.*;
import java.util.Properties;
import org.omg.CORBA.ORB;
import org.omg.PortableServer.POA;
import org.omg.PortableServer.POAHelper;
import spike.*;

public class Server {
    static class EchoImpl extends EchoPOA {
        public int ping() { return 42; }
        public int add(int a, int b) { return a + b; }
        public String echo_string(String msg) { return msg; }
        public double scale(double v, double by) { return v * by; }
        public Ragged echo_ragged(Ragged v) { return v; }
        public org.omg.CORBA.Any echo_any(org.omg.CORBA.Any v) { return v; }
        public byte[] blob(int size) {
            byte[] b = new byte[size];
            for (int i = 0; i < size; i++) b[i] = (byte) (i % 251);
            return b;
        }
        public String echo_wstring(String w) { return w; }
        public int blob_sum(byte[] b) {
            long s = 0;
            for (byte x : b) s += (x & 0xff);
            return (int) (s % 2147483647L);
        }
    }

    public static void main(String[] args) throws Exception {
        Properties p = new Properties();
        p.setProperty("org.omg.CORBA.ORBClass", "org.jacorb.orb.ORB");
        p.setProperty("org.omg.CORBA.ORBSingletonClass", "org.jacorb.orb.ORBSingleton");

        ORB orb = ORB.init(args, p);
        POA poa = POAHelper.narrow(orb.resolve_initial_references("RootPOA"));
        poa.the_POAManager().activate();

        org.omg.CORBA.Object ref = poa.servant_to_reference(new EchoImpl());
        Path out = Path.of(args[args.length - 1]);
        Files.writeString(out, orb.object_to_string(ref));
        System.out.println("IOR written to " + out);
        System.out.println("READY");
        System.out.flush();
        orb.run();
    }
}
