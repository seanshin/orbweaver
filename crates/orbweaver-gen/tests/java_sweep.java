// The Java half of the cross-implementation oracle. Compiled by
// `crates/orbweaver-gen/tests/java_target.rs` against one generated package
// (always named `contract` there) and driven one line at a time from Rust.
//
// **Nothing here is generated**, which is the point: a test the generator wrote
// for itself proves nothing, so the driver is hand-written and reaches the
// generated code the way a consumer does — by the names the emitter gave it,
// looked up at run time.
//
// The protocol is one tab-separated request per line on stdin, one JSON answer
// per line on stdout:
//
//   value <TAB> <type form> <TAB> <document>
//       {"value": …}      the document read as that type and written back out
//   call  <TAB> <class> <TAB> <method> <TAB> <args> <TAB> <reply> <TAB> <returns>
//       {"request": …, "returned": …}   what the stub sent, and what it read
//   open  <TAB> <type form> <TAB> <document>
//       {"refused": …}    a peer-fed `any` whose value cannot cross
//   words <TAB> <subject>
//       the runtime's refusal sentences, for comparison against the functions
//       that own them
//
// A type is passed as AnyJSON v1.1's **structural form** rather than as a Java
// descriptor, because a Java type does not say which IDL type it came from:
// `int` is both `long` and `unsigned short`, and `long` is `unsigned long`,
// `long long` and `unsigned long long` — which cross as a number, a number, a
// string and a string. Rebuilding an argument from its Java type would have
// compared two spellings of one value and called the difference a divergence.

import contract._Rt;

import java.lang.reflect.Method;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

public final class java_sweep {
    private java_sweep() {}

    public static void main(String[] _argv) throws Exception {
        contract._Types._ensure();
        java.io.BufferedReader _in = new java.io.BufferedReader(
                new java.io.InputStreamReader(System.in, java.nio.charset.StandardCharsets.UTF_8));
        java.io.PrintStream _out = new java.io.PrintStream(System.out, true, "UTF-8");
        String _line;
        while ((_line = _in.readLine()) != null) {
            if (_line.trim().isEmpty()) {
                continue;
            }
            String[] _f = _line.split("\t", -1);
            try {
                _out.println(answer(_f));
            } catch (Throwable _t) {
                LinkedHashMap<String, Object> _err = new LinkedHashMap<String, Object>();
                _err.put("error", _t.getClass().getName() + ": " + _t.getMessage());
                _out.println(_Rt._writeJson(_err));
            }
        }
    }

    static String answer(String[] _f) throws Exception {
        String _mode = _f[0];
        if (_mode.equals("value")) {
            _Rt.Desc _d = _Rt._descOfForm(_Rt._parseJson(_f[1]), "");
            Object _v = _Rt._fromJson(_d, _Rt._parseJson(_f[2]), "");
            LinkedHashMap<String, Object> _ok = new LinkedHashMap<String, Object>();
            _ok.put("value", _Rt._toJson(_d, _v, ""));
            return _Rt._writeJson(_ok);
        }
        if (_mode.equals("words")) {
            LinkedHashMap<String, Object> _w = new LinkedHashMap<String, Object>();
            _w.put("deferred", _Rt._deferred(_f[1]));
            _w.put("unmarshallable", _Rt._unmarshallable(_f[1]));
            _w.put("withdrawn", _Rt._withdrawn(_f[1]));
            _w.put("principal_subject", _Rt._principalSubject());
            return _Rt._writeJson(_w);
        }
        if (_mode.equals("open")) {
            // A peer-fed `any` whose `_t` names something whose value cannot
            // cross. The refusal is the product's, not the test's.
            LinkedHashMap<String, Object> _r = new LinkedHashMap<String, Object>();
            try {
                _r.put("opened", _Rt._writeJson(
                        _Rt._toJson(_Rt._descOfForm(_Rt._parseJson(_f[1]), ""),
                                new _Rt.Any(_Rt._parseJson(_f[1]),
                                        _Rt._parseJson(_f[2])).open(), "")));
            } catch (_Rt.Error _e) {
                _r.put("refused", _e.getMessage());
            }
            return _Rt._writeJson(_r);
        }
        if (_mode.equals("call")) {
            return call(_f[1], _f[2], _f[3], _f[4], _f[5]);
        }
        throw new IllegalArgumentException("unknown mode " + _mode);
    }

    /**
     * One generated stub method, driven through a Loopback.
     *
     * `_args` is a JSON array of `{"t": <form>, "v": <document>}`; `_reply` is
     * the document the Loopback answers with; `_returns` is the declared
     * result's structural form, or `"void"` when there is nothing single to
     * read back. What comes back is the request the stub built and, where the
     * operation answers with exactly one value, that value written out as
     * AnyJSON — so Rust can hold both halves to its own mapping.
     */
    @SuppressWarnings("unchecked")
    static String call(String _class, String _method, String _args, String _reply,
            String _returns) throws Exception {
        List<Object> _in = (List<Object>) _Rt._parseJson(_args);
        Object[] _values = new Object[_in.size()];
        for (int _i = 0; _i < _in.size(); _i++) {
            Map<String, Object> _a = (Map<String, Object>) _in.get(_i);
            _Rt.Desc _d = _Rt._descOfForm(_a.get("t"), "arg" + _i);
            _values[_i] = _Rt._fromJson(_d, _a.get("v"), "arg" + _i);
        }

        Class<?> _stub = Class.forName(_class);
        _Rt.Loopback _loop = new _Rt.Loopback();
        _loop.reply(_reply);
        Object _instance = _stub.getConstructor(_Rt.Invoker.class).newInstance(_loop);

        Method _target = null;
        for (Method _m : _stub.getMethods()) {
            if (_m.getName().equals(_method) && _m.getParameterCount() == _values.length
                    && !java.lang.reflect.Modifier.isStatic(_m.getModifiers())) {
                _target = _m;
                break;
            }
        }
        if (_target == null) {
            throw new NoSuchMethodException(
                    _class + " has no method " + _method + " taking " + _values.length
                    + " argument(s) — the emitter and the name a caller was given disagree");
        }

        LinkedHashMap<String, Object> _r = new LinkedHashMap<String, Object>();
        Object _returned;
        try {
            _returned = _target.invoke(_instance, _values);
        } catch (java.lang.reflect.InvocationTargetException _e) {
            Throwable _cause = _e.getCause();
            _r.put("raised", _cause.getClass().getName());
            _r.put("message", String.valueOf(_cause.getMessage()));
            if (_cause instanceof _Rt.UserException) {
                _Rt.UserException _u = (_Rt.UserException) _cause;
                _r.put("id", _u._id());
                if (_Rt._type(_u._id()) != null) {
                    _r.put("members", _Rt._toJson(new _Rt.Ref(_u._id()), _u, ""));
                }
            }
            _r.put("request", _loop.requests.isEmpty() ? null : _loop.requests.get(0));
            return _Rt._writeJson(_r);
        }

        _r.put("request", _loop.requests.isEmpty() ? null : _loop.requests.get(0));
        if (!_returns.equals("void")) {
            _r.put("returned",
                    _Rt._toJson(_Rt._descOfForm(_Rt._parseJson(_returns), "<return>"),
                            _returned, "<return>"));
        }
        return _Rt._writeJson(_r);
    }
}
