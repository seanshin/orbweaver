// The SSL twin of Server.java: the same Echo servant, published over JacORB's
// SSLIOP.
//
// TEST FIXTURE. JacORB is LGPL and is a separate process we dial; nothing here
// is linked into Orbweaver and nothing built from it is published. See
// CLAUDE.md's licensing boundary.
//
// What it is for. `spikes/tls/PEER-STATUS.md` names JacORB's encoder as unblock
// option 3 — a SECOND independent producer of `TAG_SSL_SEC_TRANS`. omniORB's
// was measured 2026-09-03 through `spikes/echo_server_ssl.py`; one producer is
// what the residue asked for, and two is a comparison: whether our decoder reads
// the association-option bits and the port convention the same way off two
// encoders that never shared a line of code.
//
// The keystores are DERIVED from the PEM fixtures in spikes/tls/ by
// `spikes/jacorb/ssl_keystores.sh` (JKS, password `fixture`), never committed:
// `regen.sh` owns the originals, and JSSE's format is a packaging of them and not
// a second identity.
//
// *Server.java의 SSL 쌍둥이. `TAG_SSL_SEC_TRANS`의 두 번째 독립 생산자 — 하나는 잔여가
// 요구한 것이고, 둘은 비교다: 코드를 한 줄도 공유하지 않은 두 인코더의 출력을 우리
// 디코더가 같은 방식으로 읽는가. 키스토어는 PEM 픽스처에서 파생되며 커밋되지 않는다.*
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Properties;

import org.omg.CORBA.ORB;
import org.omg.PortableServer.POA;
import org.omg.PortableServer.POAHelper;

import spike.EchoPOA;
import spike.Ragged;

public final class SslServer {
    static final class EchoImpl extends EchoPOA {
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
        public spike.Echo get_self() { return _this(); }
        public boolean same_as(spike.Echo other) { return _this()._is_equivalent(other); }
        public int blob_sum(byte[] b) {
            long s = 0;
            for (byte x : b) s += (x & 0xff);
            return (int) (s % 2147483647L);
        }
    }

    public static void main(String[] args) throws Exception {
        // args: <tls dir> <ior out>
        Path tls = Path.of(args[0]);
        Properties p = new Properties();
        p.setProperty("org.omg.CORBA.ORBClass", "org.jacorb.orb.ORB");
        p.setProperty("org.omg.CORBA.ORBSingletonClass", "org.jacorb.orb.ORBSingleton");

        // JacORB's SSLIOP, through the JSSE factories it ships. The association
        // options are the peer's to choose; what is measured is what it WRITES
        // into the component, not what we asked for.
        p.setProperty("jacorb.security.support_ssl", "on");
        p.setProperty("jacorb.security.ssl.server.supported_options", "60");
        p.setProperty("jacorb.security.ssl.server.required_options", "60");
        p.setProperty("jacorb.security.ssl.client.supported_options", "60");
        p.setProperty("jacorb.security.ssl.client.required_options", "60");
        p.setProperty("jacorb.ssl.socket_factory",
                "org.jacorb.security.ssl.sun_jsse.SSLSocketFactory");
        p.setProperty("jacorb.ssl.server_socket_factory",
                "org.jacorb.security.ssl.sun_jsse.SSLServerSocketFactory");
        // **JKS, not PKCS12, and not by preference.** JacORB 3.9's
        // `KeyStoreUtil.getKeyStore` loads from a file ONLY when the type is
        // `JKS` (read off the bytecode: `ldc "JKS"; equalsIgnoreCase; ifeq`);
        // any other type gets `KeyStore.load(null)` — an empty store — and the
        // handshake then fails with `No available authentication scheme`. Found
        // 2026-09-03 by calling that loader directly from its own package:
        // `type=PKCS12 size=0` where `type=JKS size=1`. The keystores are still
        // derived from the PEM fixtures; only the container changed.
        p.setProperty("jacorb.security.keystore", tls.resolve(".server.jks").toString());
        p.setProperty("jacorb.security.keystore_password", "fixture");
        p.setProperty("jacorb.security.keystore_type", "JKS");
        p.setProperty("jacorb.security.jsse.trustees_from_ks", "on");
        p.setProperty("jacorb.security.truststore", tls.resolve(".trust.jks").toString());
        p.setProperty("jacorb.security.truststore_password", "fixture");
        p.setProperty("jacorb.security.truststore_type", "JKS");
        // Loopback by name, for the same reason echo_server_ssl.py binds it:
        // the fixture certificate is issued for localhost/127.0.0.1.
        p.setProperty("OAIAddr", "127.0.0.1");
        p.setProperty("OASSLPort", "0");

        ORB orb = ORB.init(args, p);
        POA poa = POAHelper.narrow(orb.resolve_initial_references("RootPOA"));
        poa.the_POAManager().activate();

        org.omg.CORBA.Object ref = poa.servant_to_reference(new EchoImpl());
        Path out = Path.of(args[1]);
        Files.writeString(out, orb.object_to_string(ref));
        System.out.println("IOR written to " + out);
        System.out.println("READY");
        System.out.flush();
        orb.run();
    }
}
