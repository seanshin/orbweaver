// Calls `echo_string` once on a stringified IOR given on the command line, so
// that a listener on the other end can record which char transmission code set
// JacORB negotiated. TEST FIXTURE — see Client.java.
//
// Used by spikes/codeset_advertise_probe.py (D009 §8 batch 4). The text is
// passed as an argument rather than compiled in, because the octets are the
// measurement: "café" is four bytes in ISO-8859-1 and five in UTF-8.
import java.util.Properties;
import org.omg.CORBA.ORB;
import spike.Echo;
import spike.EchoHelper;

public class CodesetCaller {
    public static void main(String[] args) throws Exception {
        Properties p = new Properties();
        p.setProperty("org.omg.CORBA.ORBClass", "org.jacorb.orb.ORB");
        p.setProperty("org.omg.CORBA.ORBSingletonClass", "org.jacorb.orb.ORBSingleton");
        ORB orb = ORB.init(args, p);
        String ior = args[args.length - 2];
        String text = args[args.length - 1];
        Echo echo = EchoHelper.narrow(orb.string_to_object(ior));
        try {
            System.out.println("REPLY " + echo.echo_string(text));
        } catch (Exception e) {
            System.out.println("RAISED " + e.getClass().getName() + " " + e.getMessage());
        }
        System.out.flush();
        System.exit(0);
    }
}
