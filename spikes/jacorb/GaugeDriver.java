// A JacORB client for corpus/golden/24-skeleton-surface.idl, driven against a
// servant behind our ORB — and deliberately not told which language wrote it.
//
// TEST FIXTURE. JacORB is LGPL and is never linked into Orbweaver; this is a
// separate process speaking the published GIOP wire over TCP. See PLAN 10.
//
// Why this file exists. `crates/orbweaver-gen/tests/python_servant_wire.rs`
// measured omniORB's client calling a Python servant and reported its own
// limit: omniORB emits its native order, and our server replies in the
// *request's* order, so both directions of that exchange are little-endian on
// this host. D030 3 asks for "both byte orders against a peer that is not us",
// and the missing half is foreign peer x big-endian. Java's ORBs write
// big-endian, so JacORB is the peer that can reach it — but that is a belief
// until the request's flag byte says so, which is why the Rust side of this
// fixture reads the order off the wire rather than off the language.
//
// The printed lines are deliberately the same sentences omniORB's driver
// prints (OMNIORB_DRIVER in that file), so one assertion list covers both
// peers: if a caller cannot tell the two servants apart, it cannot tell the
// two *peers'* transcripts apart either.
//
// Nothing here mentions the servant's language, the bridge, or a byte order.
// That is the transparency claim, made against the only thing a peer has.
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Properties;

import org.omg.CORBA.DoubleHolder;
import org.omg.CORBA.ORB;
import org.omg.CORBA.StringHolder;

import gc24.Busy;
import gc24.Gauge;
import gc24.GaugeHelper;
import gc24.Reading;
import gc24.Rejected;

public class GaugeDriver {
    public static void main(String[] args) throws Exception {
        Properties p = new Properties();
        p.setProperty("org.omg.CORBA.ORBClass", "org.jacorb.orb.ORB");
        p.setProperty("org.omg.CORBA.ORBSingletonClass", "org.jacorb.orb.ORBSingleton");

        ORB orb = ORB.init(args, p);
        String text = Files.readString(Path.of(args[0])).trim();
        Gauge gauge = GaugeHelper.narrow(orb.string_to_object(text));
        if (gauge == null) {
            System.out.println("narrow failed");
            System.exit(1);
        }

        gauge.label("driven by JacORB");
        System.out.println("label = " + gauge.label());

        Reading r = gauge.record(21.5, "C");
        System.out.println("record -> " + r.at + " " + r.sequence_no + " " + r.unit);
        System.out.println("scale_all -> " + gauge.scale_all(2.0));
        System.out.println("latest.at -> " + gauge.latest().at);
        DoubleHolder at = new DoubleHolder();
        StringHolder unit = new StringHolder();
        gauge.split(at, unit);
        System.out.println("split -> " + at.value + " " + unit.value);

        try {
            gauge.record(-1.0, "C");
            System.out.println("a negative sample was not refused");
        } catch (Rejected e) {
            System.out.println("Rejected " + e.why + " " + e.code);
        }
        try {
            gauge.record(1.0, "");
            System.out.println("an empty unit was not refused");
        } catch (Busy e) {
            System.out.println("Busy");
        }

        // A readonly attribute. The generated Java interface has `latest()` and
        // no `latest(Reading)`, so the refusal is a compile error and never
        // reaches the wire — the same place omniORB's stub refuses it, one
        // language over. The wire-level refusal of `_set_latest` with
        // BAD_OPERATION is measured in python_servant.rs, where the request can
        // be built by hand. Printed so the transcript says which layer refused.
        System.out.println("readonly refused client-side with no such method");

        gauge.reset();
        System.out.println("after the oneway, sequence_no = " + gauge.latest().sequence_no);

        System.out.println("is_a NamingContext -> "
            + gauge._is_a("IDL:omg.org/CosNaming/NamingContext:1.0"));
        System.out.println("is_a Gauge -> " + gauge._is_a("IDL:gc24/Gauge:1.0"));
        System.out.println("non_existent -> " + gauge._non_existent());
        System.out.println("OK");
        System.out.flush();
        System.exit(0);
    }
}
