// CSIv2-advertising JacORB peer (D010 B2, PLAN-FIRST-COMPLETION §G lane D).
// TEST FIXTURE — see Client.java for the licence terms this fixture lives under:
// JacORB is LGPL, runs as a separate process, and nothing of it is linked in.
//
// Same echo servant as Server.java; the one difference is the POA it sits on.
// JacORB 3.9 adds TAG_CSI_SEC_MECH_LIST (tag 33) to an IOR only when three
// things hold at once (read out of jacorb-3.9.jar's own bytecode, 2026-09-04,
// org.jacorb.orb.standardInterceptors.SASComponentInterceptor):
//
//   1. the SAS ORB initializer is registered, which is what installs the IOR
//      interceptor and the SASPolicy factory (policy type 102);
//   2. `jacorb.security.sas.contextClass` names an ISASContext — GssUpContext
//      is the GSSUP (username/password) mechanism, OID 2.23.130.1.1.1;
//   3. the object's POA carries a SASPolicy whose targetSupports or
//      targetRequires is non-zero. No policy, or 0/0, and the interceptor
//      returns before building the component — which is why the stock
//      Server.java advertises nothing and the harness measures exactly that.
//
// This server supports EstablishTrustInClient and requires nothing, so a
// caller that sends no SAS context (our Rust client, spike-dump's ping) is
// still served: the advertisement is the measurement, not a wall.
import java.nio.file.*;
import java.util.Properties;
import org.jacorb.sasPolicy.SASPolicyValues;
import org.jacorb.sasPolicy.SASPolicyValuesHelper;
import org.jacorb.sasPolicy.SAS_POLICY_TYPE;
import org.omg.CORBA.Any;
import org.omg.CORBA.ORB;
import org.omg.CORBA.Policy;
import org.omg.CSIIOP.EstablishTrustInClient;
import org.omg.PortableServer.POA;
import org.omg.PortableServer.POAHelper;

public class CsiServer {
    public static void main(String[] args) throws Exception {
        Properties p = new Properties();
        p.setProperty("org.omg.CORBA.ORBClass", "org.jacorb.orb.ORB");
        p.setProperty("org.omg.CORBA.ORBSingletonClass", "org.jacorb.orb.ORBSingleton");
        // OMG PI registration form: the class name is the key's suffix, the
        // value is ignored. SASInitializer registers SASTargetInterceptor,
        // SASClientInterceptor, SASComponentInterceptor and the policy
        // factories for types 102 (SAS) and 103 (ATLAS).
        p.setProperty(
            "org.omg.PortableInterceptor.ORBInitializerClass."
                + "org.jacorb.security.sas.SASInitializer",
            "");
        p.setProperty("jacorb.security.sas.contextClass",
                      "org.jacorb.security.sas.GssUpContext");

        ORB orb = ORB.init(args, p);
        POA root = POAHelper.narrow(orb.resolve_initial_references("RootPOA"));
        root.the_POAManager().activate();

        // struct SASPolicyValues { targetRequires, targetSupports, stateful }
        // — requires nothing (a bare caller is served), supports
        // EstablishTrustInClient (0x40), which is the non-zero that makes the
        // interceptor emit the component.
        Any sas = orb.create_any();
        SASPolicyValuesHelper.insert(
            sas, new SASPolicyValues((short) 0, EstablishTrustInClient.value, true));
        Policy sasPolicy = orb.create_policy(SAS_POLICY_TYPE.value, sas);
        // A child POA does not inherit RootPOA's IMPLICIT_ACTIVATION, and
        // servant_to_reference below needs it.
        POA poa = root.create_POA("CsiPOA", root.the_POAManager(),
                                  new Policy[] { sasPolicy,
                                                 root.create_implicit_activation_policy(
                                                     org.omg.PortableServer.ImplicitActivationPolicyValue
                                                         .IMPLICIT_ACTIVATION) });

        org.omg.CORBA.Object ref = poa.servant_to_reference(new Server.EchoImpl());
        Path out = Path.of(args[args.length - 1]);
        Files.writeString(out, orb.object_to_string(ref));
        System.out.println("IOR written to " + out);
        System.out.println("READY");
        System.out.flush();
        orb.run();
    }
}
