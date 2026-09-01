import java.util.LinkedHashMap;
import java.util.Map;

import echo._Rt;
import echo.spike.EchoServant;

/**
 * Drives the Java serving half with no bridge, no socket and no peer.
 *
 * `_Rt.dispatchCall` is a pure function of a servant and a parsed call document,
 * for the same reason `python_rt.dispatch_call` is one: it lets every branch —
 * every conversion, every refusal — be executed without a process in sight. This
 * is that execution, and it is what makes the serving half a measurement rather
 * than code that compiles.
 *
 * It is NOT the `servant × self` cell. That needs a Rust client and a spawner
 * that starts `java` as a seam child, which does not exist yet; claiming it
 * would be the *green because nothing happened* shape this project keeps
 * finding. What this shows is that the half a servant cell would sit on works.
 *
 * Prints one `name\tjson` line per case. The shell asserts the documents.
 */
public final class EchoServantProbe {
    /** A servant that implements two of the contract's operations and no more. */
    static final class Node extends EchoServant {
        @Override
        public int add(int a, int b) {
            return a + b;
        }

        @Override
        public String echo_string(String msg) {
            return "java:" + msg;
        }
    }

    static Map<String, Object> call(String op, Object... pairs) {
        LinkedHashMap<String, Object> args = new LinkedHashMap<String, Object>();
        for (int i = 0; i + 1 < pairs.length; i += 2) {
            args.put((String) pairs[i], pairs[i + 1]);
        }
        LinkedHashMap<String, Object> c = new LinkedHashMap<String, Object>();
        c.put("id", EchoServant._ID);
        c.put("op", op);
        c.put("args", args);
        return c;
    }

    static void say(String name, Map<String, Object> reply) {
        System.out.println(name + "\t" + _Rt._writeJson(reply));
    }

    public static void main(String[] argv) {
        Node servant = new Node();

        // An operation the servant implements: the arguments are converted, the
        // method runs, and the result is shaped into an `ok` reply.
        say("implemented", _Rt.dispatchCall(servant,
                call("add", "a", _Rt.Num.of(2), "b", _Rt.Num.of(40))));

        // A string, so the conversion is exercised on something that is not a
        // number — a servant that only ever answered longs would pass a test
        // that only ever sent them.
        say("string", _Rt.dispatchCall(servant, call("echo_string", "msg", "hello")));

        // In the contract and not implemented: NO_IMPLEMENT, deliberately not
        // BAD_OPERATION. The operation exists and this servant has not written
        // it, which is a different thing from there being no such operation.
        say("not-implemented", _Rt.dispatchCall(servant,
                call("scale", "v", _Rt.Num.of(1), "by", _Rt.Num.of(2))));

        // Not in the contract at all: BAD_OPERATION, from the table rather than
        // from the servant, so a name that never reaches a method cannot be
        // confused with one that reached an unwritten one.
        say("no-such-operation", _Rt.dispatchCall(servant, call("no_such_op")));
    }
}
