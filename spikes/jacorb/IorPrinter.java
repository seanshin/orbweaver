// Prints one JacORB-published IOR and exits. TEST FIXTURE — see Client.java.
//
// Used by spikes/codeset_peer_probe.py to read what JacORB advertises in
// TAG_CODE_SETS under a given configuration (D009 §8 batch 4). It deliberately
// uses `create_reference` rather than a generated skeleton: the question is
// what the ORB puts in a profile, which needs no servant and therefore no
// generated stubs, so this runs without spikes/jacorb/setup.sh's IDL pass.
import java.util.Properties;
import org.omg.CORBA.ORB;
import org.omg.PortableServer.POA;
import org.omg.PortableServer.POAHelper;

public class IorPrinter {
    public static void main(String[] args) throws Exception {
        Properties p = new Properties();
        p.setProperty("org.omg.CORBA.ORBClass", "org.jacorb.orb.ORB");
        p.setProperty("org.omg.CORBA.ORBSingletonClass", "org.jacorb.orb.ORBSingleton");
        ORB orb = ORB.init(args, p);
        POA poa = POAHelper.narrow(orb.resolve_initial_references("RootPOA"));
        poa.the_POAManager().activate();
        org.omg.CORBA.Object ref = poa.create_reference("IDL:spike/Echo:1.0");
        System.out.println(orb.object_to_string(ref));
        System.out.flush();
        orb.shutdown(false);
        System.exit(0);
    }
}
