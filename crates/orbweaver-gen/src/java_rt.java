// The generated Java client's runtime, shipped verbatim beside every generated
// package as `_Rt.java`. Do not edit a copy: `crates/orbweaver-gen/src/java_rt.java`
// is the one home, and `orbweaver_gen::java::RUNTIME` is how it reaches a
// consumer.
//
// ── What this is, and what it deliberately is not ───────────────────────────
//
// This is the third implementation of AnyJSON v1 (`docs/PLAN.md` §4.5) — after
// the Rust reference and the generated Python runtime — and it is the ONLY
// per-language part of a Java binding (D032 §3): the contract is the corpus's,
// the value representation is AnyJSON's, and the dispatch binding is here.
//
// **There is no GIOP in this file, and there must never be.** No socket, no
// CDR, no IOR, no codeset negotiation. A binding that speaks GIOP is a second
// ORB wearing a binding's name (D032 §3), and the wire exists once, in Rust.
// Java speaks one JSON document per line to `orbweaver-py-bridge`, which owns
// the connection. That is also why this file imports nothing outside `java.base`:
// JDK 11 removed `org.omg.CORBA` (JEP 320), the only `org.omg.CORBA` on a
// machine like this one is JacORB's jar, and JacORB is an LGPL **fixture, never
// a dependency**. Generated Java that needed it would have made a fixture into
// a dependency by the back door.
//
// ── The refusal sentences are not written here ──────────────────────────────
//
// Five families of construct cannot cross, and their heads are `pub` functions
// in `orbweaver-dynamic`, read by five Rust layers and the generated Python
// runtime. This file's `_DEFERRED`, `_UNMARSHALLABLE` and `_WITHDRAWN` are the
// same sentences — Java cannot call a Rust function, so the equality is held by
// `java_target.rs`, which computes the expected text by calling that function
// and fails the moment a wording changes. That test is the reason these are
// literals and not an invention: the generated Python runtime once wrote its
// own fourth wording for `fixed`, measured by nothing until it was broken on
// purpose, and a third target triples that exposure.
//
// ── Every local this file and the emitter bind is `_`-prefixed ──────────────
//
// D030 §5 L2 predicted Java's hazard would be its reserved words and was
// falsified before the target existed: JacORB 3.9's own stub template writes
// `catch (java.io.IOException e)` into the same scope as an operation's
// parameters, so an IDL parameter named `e` makes its generated Java fail to
// compile — and `e` is not a Java keyword. The hazard is every identifier the
// template puts in scope. An IDL identifier can never begin with `_` (the
// leading underscore in `_struct` is IDL's own keyword escape and is stripped),
// so a template local that is `_`-prefixed **cannot** collide with a contract
// name. That is why every parameter, field and local below is spelled that way:
// the class of defect is designed out rather than escaped, and
// `corpus/golden/28-target-keywords.idl`'s template-locals section executes the
// claim by naming contract members `e`, `o`, `v` and the rest.
//
// *생성된 Java 클라이언트의 런타임. 여기에 GIOP는 없다 — 와이어는 Rust에 한 번만
// 존재하고, Java는 브리지에 JSON 한 줄을 말한다. 거부 문장은 여기서 지어내지 않는다.
// 템플릿이 스코프에 넣는 모든 이름은 `_`로 시작하므로 계약의 식별자와 절대 충돌하지
// 않는다 — IDL 식별자는 `_`로 시작할 수 없기 때문이다.*

import java.io.BufferedReader;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.io.OutputStreamWriter;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

/** AnyJSON v1 and the seam to `orbweaver-py-bridge`, for generated Java. */
public final class _Rt {
    private _Rt() {}

    // ── the refusal sentences, equal to orbweaver-dynamic's published heads ──

    /** `orbweaver_dynamic::deferred_wire_sentence`. */
    public static String _deferred(String _what) {
        return _what + " is not marshalled by the v1 wire (docs/PLAN.md §4.4); the TypeCode"
                + " describing it reads, the value behind it does not";
    }

    /** `orbweaver_dynamic::unmarshallable_wire_sentence`. */
    public static String _unmarshallable(String _what) {
        return _what + " has no wire form at all: it names a type only a language mapping"
                + " knows, and no version of the wire marshals one; this is not one of"
                + " docs/PLAN.md §4.4's deferrals — those have a wire form this version has"
                + " not implemented, and there is none here to implement";
    }

    /** `orbweaver_dynamic::withdrawn_wire_sentence`. */
    public static String _withdrawn(String _what) {
        return _what + " was withdrawn from CORBA: GIOP 1.0 carried one in every request"
                + " header, GIOP 1.1 dropped that field and CORBA 3.0 removed the type — so"
                + " this version marshals no value for one, and no later version will; the"
                + " TypeCode describing it reads, the value behind it does not. This is not"
                + " one of docs/PLAN.md §4.4's deferrals: those wait on this project, and a"
                + " type the specification has removed waits on nobody";
    }

    /** `orbweaver_dynamic::PRINCIPAL_ID`. */
    public static final String _PRINCIPAL_ID = "IDL:omg.org/CORBA/Principal:1.0";

    /** The one spelling of a refused construct's subject. */
    public static String _subject(String _kind, String _name, String _id) {
        return _name.isEmpty() ? _kind + " " + _id : _kind + " " + _name + " (" + _id + ")";
    }

    /** How `::CORBA::Principal` is named as the subject of a refusal. */
    public static String _principalSubject() {
        return _subject("predeclared type", "::CORBA::Principal", _PRINCIPAL_ID);
    }

    // ── the seam protocol ───────────────────────────────────────────────────
    //
    // The names below are the ONLY spelling of the seam's document keys
    // anywhere in this runtime: every call this file reads and every reply it
    // builds goes through one of them. `seamProtocol()` is assembled from
    // exactly these, so a runtime that started reading a different key would
    // change the document it publishes rather than drift from the one the ORB
    // dispatches with.
    //
    // Compared against `orbweaver_gen::seam::protocol()` by
    // `crates/orbweaver-gen/tests/the_seam_is_one_protocol.rs`. Java cannot
    // import a Rust constant, which is why this is an equality across the
    // crate boundary rather than a shared symbol — the same shape and the same
    // reason as the refusal families above.
    //
    // Added 2026-09-02. Before it, this file read `_call.get("call")` and
    // thirty-six other literals, and published no document at all, so nothing
    // could go red when it disagreed with the ORB. The test's own header had
    // said since it was written that a third language "adds a function and a
    // row here, and nothing else" — Java landed on 2026-09-01 without either.

    /** The protocol version this runtime speaks. */
    //
    // **1, not 2, and deliberately.** 2 is the version with a message from the
    // far side (`invoke`), and this runtime does not serve one yet: a servant
    // here cannot invoke a reference it was handed. A runtime announcing a
    // version it does not implement is the *claimed versus observed*
    // distinction the acceptance grid exists to refuse, one layer down. It
    // becomes "2" in the same change that makes it true, and not before.
    public static final String _SEAM_VERSION = "1";

    /** The envelope the bridge wraps a call in. */
    public static final String _SEAM_ENVELOPE_CALL = "call";

    public static final String _SEAM_CALL_INTERFACE = "id";
    public static final String _SEAM_CALL_OPERATION = "op";
    // No `_SEAM_CALL_OBJECT`, and its absence is a finding rather than an
    // oversight. Python's runtime reads the call's `oid` and a servant reaches
    // it through `Servant.own_oid()`; **Java's `Servant` interface has no such
    // member and `dispatchCall` never reads the key**, so a Java servant cannot
    // tell which object of its interface it was addressed to. Publishing
    // `call.object` here would make this document a description of the protocol
    // rather than a statement of what this file reads — which is the one thing
    // it must not be. The gap is named in the agreement test's pinned
    // differences and is work, not a property.
    public static final String _SEAM_CALL_ARGUMENTS = "args";
    public static final String _SEAM_CALL_ONEWAY = "oneway";

    public static final String _SEAM_REPLY_OK = "ok";
    public static final String _SEAM_REPLY_RETURNS = "returns";
    public static final String _SEAM_REPLY_OUTPUTS = "outputs";
    public static final String _SEAM_REPLY_USER_EXCEPTION = "user_exception";
    public static final String _SEAM_REPLY_SYSTEM_EXCEPTION = "system_exception";
    public static final String _SEAM_REPLY_ERROR = "error";

    public static final String _SEAM_EXCEPTION_ID = "id";
    public static final String _SEAM_EXCEPTION_MEMBERS = "members";
    public static final String _SEAM_EXCEPTION_MINOR = "minor";
    public static final String _SEAM_EXCEPTION_COMPLETED = "completed";

    /** §4.11.4's ordinals, crossing as numbers rather than as second names. */
    public static final long _SEAM_COMPLETED_YES = 0L;
    public static final long _SEAM_COMPLETED_NO = 1L;
    public static final long _SEAM_COMPLETED_MAYBE = 2L;

    /** The prefix a servant names one of its own objects with. */
    public static final String _SEAM_OWN_OBJECT_PREFIX = "oid:";

    /**
     * This runtime's copy of the seam's document shape, as data.
     *
     * <p>Built from the constants above and from nothing else, so it is a
     * statement about what this file actually reads rather than a description
     * of it.
     */
    public static Map<String, Object> seamProtocol() {
        Map<String, Object> _d = new LinkedHashMap<String, Object>();
        _d.put("version", _SEAM_VERSION);

        Map<String, Object> _env = new LinkedHashMap<String, Object>();
        _env.put("call", _SEAM_ENVELOPE_CALL);
        _d.put("envelope", _env);

        Map<String, Object> _call = new LinkedHashMap<String, Object>();
        _call.put("interface", _SEAM_CALL_INTERFACE);
        _call.put("operation", _SEAM_CALL_OPERATION);
        _call.put("arguments", _SEAM_CALL_ARGUMENTS);
        _call.put("oneway", _SEAM_CALL_ONEWAY);
        _d.put("call", _call);

        Map<String, Object> _reply = new LinkedHashMap<String, Object>();
        _reply.put("ok", _SEAM_REPLY_OK);
        _reply.put("returns", _SEAM_REPLY_RETURNS);
        _reply.put("outputs", _SEAM_REPLY_OUTPUTS);
        _reply.put("user_exception", _SEAM_REPLY_USER_EXCEPTION);
        _reply.put("system_exception", _SEAM_REPLY_SYSTEM_EXCEPTION);
        _reply.put("error", _SEAM_REPLY_ERROR);
        _d.put("reply", _reply);

        Map<String, Object> _exc = new LinkedHashMap<String, Object>();
        _exc.put("id", _SEAM_EXCEPTION_ID);
        _exc.put("members", _SEAM_EXCEPTION_MEMBERS);
        _exc.put("minor", _SEAM_EXCEPTION_MINOR);
        _exc.put("completed", _SEAM_EXCEPTION_COMPLETED);
        _d.put("exception", _exc);

        Map<String, Object> _done = new LinkedHashMap<String, Object>();
        _done.put("yes", Num.of(_SEAM_COMPLETED_YES));
        _done.put("no", Num.of(_SEAM_COMPLETED_NO));
        _done.put("maybe", Num.of(_SEAM_COMPLETED_MAYBE));
        _d.put("completed", _done);

        Map<String, Object> _ref = new LinkedHashMap<String, Object>();
        _ref.put("own_object_prefix", _SEAM_OWN_OBJECT_PREFIX);
        _d.put("reference", _ref);

        // No `invoke` section: this runtime does not serve that message. Its
        // absence is what `_SEAM_VERSION` being 1 means, said in data.
        return _d;
    }

    /** Prints {@link #seamProtocol} as one JSON line, for the agreement test. */
    public static void main(String[] _argv) {
        System.out.println(_writeJson(seamProtocol()));
    }

    // ── errors ──────────────────────────────────────────────────────────────

    /** Every failure this runtime raises, so a caller can catch one thing. */
    public static class Error extends RuntimeException {
        private static final long serialVersionUID = 1L;

        public Error(String _message) {
            super(_message);
        }
    }

    /** A value that does not match the contract, with the path that found it. */
    public static final class MarshalError extends Error {
        private static final long serialVersionUID = 1L;
        public final String path;

        public MarshalError(String _path, String _message) {
            super(_path.isEmpty() ? _message : _path + ": " + _message);
            this.path = _path;
        }
    }

    /** The seam failed: the bridge would not start, died, or answered nothing. */
    public static final class TransportError extends Error {
        private static final long serialVersionUID = 1L;

        public TransportError(String _message) {
            super(_message);
        }
    }

    /** A CORBA system exception, as the reply named it. */
    public static final class SystemException extends Error {
        private static final long serialVersionUID = 1L;
        public final String id;
        public final long minor;
        /** §4.11.4's ordinal, passed through: 0 YES, 1 NO, 2 MAYBE. */
        public final int completed;

        public SystemException(String _id, long _minor, int _completed) {
            super(_id + " (minor " + _minor + ", completed " + _completed + ")");
            this.id = _id;
            this.minor = _minor;
            this.completed = _completed;
        }
    }

    /** The base of every generated exception class. */
    public abstract static class UserException extends Error {
        private static final long serialVersionUID = 1L;

        protected UserException(String _message) {
            super(_message);
        }

        /** The repository id the reply names it by. */
        public abstract String _id();
    }

    // ── JSON ────────────────────────────────────────────────────────────────
    //
    // A number keeps the text it arrived as. That is not fussiness: an `any`
    // crosses this runtime unopened (see `Any`), and a double re-printed by a
    // second language is a different string for the same value — which would
    // make the byte-for-byte comparison the cross-implementation oracle rests
    // on a comparison of two languages' float formatters instead.

    /** A JSON number, holding the exact text it was written with. */
    public static final class Num {
        public final String text;

        public Num(String _text) {
            this.text = _text;
        }

        public static Num of(long _v) {
            return new Num(Long.toString(_v));
        }

        public long asLong() {
            return Long.parseLong(text);
        }

        public double asDouble() {
            return Double.parseDouble(text);
        }

        public boolean isIntegral() {
            return text.indexOf('.') < 0 && text.indexOf('e') < 0 && text.indexOf('E') < 0;
        }

        @Override
        public boolean equals(Object _other) {
            return _other instanceof Num && ((Num) _other).text.equals(text);
        }

        @Override
        public int hashCode() {
            return text.hashCode();
        }

        @Override
        public String toString() {
            return text;
        }
    }

    /** Reads one JSON document. */
    public static Object _parseJson(String _text) {
        _Parser _p = new _Parser(_text);
        _p.ws();
        Object _v = _p.value();
        _p.ws();
        if (_p.at < _p.src.length()) {
            throw new Error("trailing text after a JSON document at " + _p.at);
        }
        return _v;
    }

    private static final class _Parser {
        final String src;
        int at;

        _Parser(String _src) {
            this.src = _src;
            this.at = 0;
        }

        void ws() {
            while (at < src.length()) {
                char _c = src.charAt(at);
                if (_c == ' ' || _c == '\t' || _c == '\n' || _c == '\r') {
                    at++;
                } else {
                    break;
                }
            }
        }

        Object value() {
            if (at >= src.length()) {
                throw new Error("a JSON document ended early");
            }
            char _c = src.charAt(at);
            switch (_c) {
                case '{':
                    return object();
                case '[':
                    return array();
                case '"':
                    return string();
                case 't':
                    expect("true");
                    return Boolean.TRUE;
                case 'f':
                    expect("false");
                    return Boolean.FALSE;
                case 'n':
                    expect("null");
                    return null;
                default:
                    return number();
            }
        }

        void expect(String _word) {
            if (!src.startsWith(_word, at)) {
                throw new Error("expected " + _word + " at " + at);
            }
            at += _word.length();
        }

        Map<String, Object> object() {
            LinkedHashMap<String, Object> _out = new LinkedHashMap<String, Object>();
            at++;
            ws();
            if (at < src.length() && src.charAt(at) == '}') {
                at++;
                return _out;
            }
            while (true) {
                ws();
                String _k = string();
                ws();
                if (at >= src.length() || src.charAt(at) != ':') {
                    throw new Error("expected ':' at " + at);
                }
                at++;
                ws();
                _out.put(_k, value());
                ws();
                if (at >= src.length()) {
                    throw new Error("an object never closed");
                }
                char _c = src.charAt(at++);
                if (_c == '}') {
                    return _out;
                }
                if (_c != ',') {
                    throw new Error("expected ',' or '}' at " + (at - 1));
                }
            }
        }

        List<Object> array() {
            ArrayList<Object> _out = new ArrayList<Object>();
            at++;
            ws();
            if (at < src.length() && src.charAt(at) == ']') {
                at++;
                return _out;
            }
            while (true) {
                ws();
                _out.add(value());
                ws();
                if (at >= src.length()) {
                    throw new Error("an array never closed");
                }
                char _c = src.charAt(at++);
                if (_c == ']') {
                    return _out;
                }
                if (_c != ',') {
                    throw new Error("expected ',' or ']' at " + (at - 1));
                }
            }
        }

        String string() {
            if (at >= src.length() || src.charAt(at) != '"') {
                throw new Error("expected a string at " + at);
            }
            at++;
            StringBuilder _b = new StringBuilder();
            while (true) {
                if (at >= src.length()) {
                    throw new Error("a string never closed");
                }
                char _c = src.charAt(at++);
                if (_c == '"') {
                    return _b.toString();
                }
                if (_c != '\\') {
                    _b.append(_c);
                    continue;
                }
                char _e = src.charAt(at++);
                switch (_e) {
                    case '"': _b.append('"'); break;
                    case '\\': _b.append('\\'); break;
                    case '/': _b.append('/'); break;
                    case 'b': _b.append('\b'); break;
                    case 'f': _b.append('\f'); break;
                    case 'n': _b.append('\n'); break;
                    case 'r': _b.append('\r'); break;
                    case 't': _b.append('\t'); break;
                    case 'u':
                        _b.append((char) Integer.parseInt(src.substring(at, at + 4), 16));
                        at += 4;
                        break;
                    default:
                        throw new Error("unknown escape \\" + _e);
                }
            }
        }

        Num number() {
            int _start = at;
            if (at < src.length() && (src.charAt(at) == '-' || src.charAt(at) == '+')) {
                at++;
            }
            while (at < src.length()) {
                char _c = src.charAt(at);
                if ((_c >= '0' && _c <= '9') || _c == '.' || _c == 'e' || _c == 'E'
                        || _c == '-' || _c == '+') {
                    at++;
                } else {
                    break;
                }
            }
            if (_start == at) {
                throw new Error("expected a value at " + _start);
            }
            return new Num(src.substring(_start, at));
        }
    }

    /** Writes one JSON document, with object members in the order they were put. */
    public static String _writeJson(Object _v) {
        StringBuilder _b = new StringBuilder();
        _write(_b, _v);
        return _b.toString();
    }

    @SuppressWarnings("unchecked")
    private static void _write(StringBuilder _b, Object _v) {
        if (_v == null) {
            _b.append("null");
        } else if (_v instanceof Boolean) {
            _b.append(((Boolean) _v).booleanValue() ? "true" : "false");
        } else if (_v instanceof Num) {
            _b.append(((Num) _v).text);
        } else if (_v instanceof String) {
            _writeString(_b, (String) _v);
        } else if (_v instanceof Map) {
            _b.append('{');
            boolean _first = true;
            for (Map.Entry<String, Object> _e : ((Map<String, Object>) _v).entrySet()) {
                if (!_first) {
                    _b.append(',');
                }
                _first = false;
                _writeString(_b, _e.getKey());
                _b.append(':');
                _write(_b, _e.getValue());
            }
            _b.append('}');
        } else if (_v instanceof List) {
            _b.append('[');
            boolean _first = true;
            for (Object _x : (List<Object>) _v) {
                if (!_first) {
                    _b.append(',');
                }
                _first = false;
                _write(_b, _x);
            }
            _b.append(']');
        } else {
            throw new Error("not a JSON value: " + _v.getClass().getName());
        }
    }

    private static void _writeString(StringBuilder _b, String _s) {
        _b.append('"');
        for (int _i = 0; _i < _s.length(); _i++) {
            char _c = _s.charAt(_i);
            switch (_c) {
                case '"': _b.append("\\\""); break;
                case '\\': _b.append("\\\\"); break;
                case '\n': _b.append("\\n"); break;
                case '\r': _b.append("\\r"); break;
                case '\t': _b.append("\\t"); break;
                case '\b': _b.append("\\b"); break;
                case '\f': _b.append("\\f"); break;
                default:
                    if (_c < 0x20) {
                        _b.append(String.format("\\u%04x", (int) _c));
                    } else {
                        _b.append(_c);
                    }
            }
        }
        _b.append('"');
    }

    // ── descriptors ─────────────────────────────────────────────────────────
    //
    // A descriptor is the Java target's type language, and it names other types
    // by **repository id** rather than by class, for the reason the Python
    // target does: IDL scopes are mutually recursive, and an id needs nothing to
    // be loaded yet.

    /** What a value is, in AnyJSON's terms. */
    public abstract static class Desc {}

    /** One of the type system's leaves: `"long"`, `"boolean"`, `"any"`, … */
    public static final class Prim extends Desc {
        public final String kind;

        public Prim(String _kind) {
            this.kind = _kind;
        }
    }

    /** `string` or `wstring`, with a bound (0 for unbounded). */
    public static final class Str extends Desc {
        public final boolean wide;
        public final long bound;

        public Str(boolean _wide, long _bound) {
            this.wide = _wide;
            this.bound = _bound;
        }
    }

    /** `sequence<T>`, with a bound (0 for unbounded). */
    public static final class Seq extends Desc {
        public final Desc element;
        public final long bound;

        public Seq(Desc _element, long _bound) {
            this.element = _element;
            this.bound = _bound;
        }
    }

    /** `T[n]`. */
    public static final class Arr extends Desc {
        public final Desc element;
        public final int length;

        public Arr(Desc _element, int _length) {
            this.element = _element;
            this.length = _length;
        }
    }

    /** An interface: the value is a handle, never an address (§4.7). */
    public static final class ObjRef extends Desc {
        public final String id;

        public ObjRef(String _id) {
            this.id = _id;
        }
    }

    /** A named type, looked up in the registry when it is used. */
    public static final class Ref extends Desc {
        public final String id;

        public Ref(String _id) {
            this.id = _id;
        }
    }

    /** `fixed<d,s>` — §4.4, deferred. */
    public static final class FixedD extends Desc {
        public final int digits;
        public final int scale;

        public FixedD(int _digits, int _scale) {
            this.digits = _digits;
            this.scale = _scale;
        }
    }

    /** A construct with no wire form in any version, or one CORBA withdrew. */
    public static final class NoWire extends Desc {
        /** `deferred`, `unmarshallable` or `withdrawn`. */
        public final String family;
        public final String subject;

        public NoWire(String _family, String _subject) {
            this.family = _family;
            this.subject = _subject;
        }
    }

    public static final Desc BOOLEAN = new Prim("boolean");
    public static final Desc OCTET = new Prim("octet");
    public static final Desc CHAR = new Prim("char");
    public static final Desc WCHAR = new Prim("wchar");
    public static final Desc SHORT = new Prim("short");
    public static final Desc USHORT = new Prim("ushort");
    public static final Desc LONG = new Prim("long");
    public static final Desc ULONG = new Prim("ulong");
    public static final Desc LONGLONG = new Prim("longlong");
    public static final Desc ULONGLONG = new Prim("ulonglong");
    public static final Desc FLOAT = new Prim("float");
    public static final Desc DOUBLE = new Prim("double");
    public static final Desc LONGDOUBLE = new Prim("longdouble");
    public static final Desc ANY = new Prim("any");
    public static final Desc TYPECODE = new Prim("typecode");
    public static final Desc VOID = new Prim("void");

    /** A `long double`: sixteen octets, unread — the wire's, not Java's. */
    public static final class LongDouble {
        public final byte[] octets;

        public LongDouble(byte[] _octets) {
            if (_octets.length != 16) {
                throw new Error("a long double is 16 octets, got " + _octets.length);
            }
            this.octets = _octets;
        }

        @Override
        public boolean equals(Object _other) {
            return _other instanceof LongDouble
                    && Arrays.equals(((LongDouble) _other).octets, octets);
        }

        @Override
        public int hashCode() {
            return Arrays.hashCode(octets);
        }

        @Override
        public String toString() {
            return "LongDouble(" + Base64.getEncoder().encodeToString(octets) + ")";
        }
    }

    /**
     * An object reference, as a **handle** into the bridge's table.
     *
     * §4.5 cannot emit an IOR, so a reference crosses as a name the bridge
     * issued. It is not dialable, it does not outlive that process, and a
     * handle nobody issued is refused rather than resolved.
     */
    public static final class ObjectRef {
        public final String handle;
        public final String typeId;

        public ObjectRef(String _handle, String _typeId) {
            this.handle = _handle;
            this.typeId = _typeId;
        }

        public ObjectRef(String _handle) {
            this(_handle, "");
        }

        @Override
        public boolean equals(Object _other) {
            return _other instanceof ObjectRef && ((ObjectRef) _other).handle.equals(handle);
        }

        @Override
        public int hashCode() {
            return handle.hashCode();
        }

        @Override
        public String toString() {
            return "ObjectRef(" + handle + (typeId.isEmpty() ? "" : ", " + typeId) + ")";
        }
    }

    /**
     * An `any`, carried **unopened**: the `_t` half and the `_v` half exactly as
     * they crossed.
     *
     * # Why unopened
     *
     * AnyJSON v1.1 (D008) lets an `any` describe a type the receiver has never
     * heard of, structurally. The Python runtime rebuilds a class from that
     * form; this runtime relays the document instead, which is a smaller claim
     * and an exact one — nothing this version does not understand about a
     * peer's type can be lost on the way through, and a value round-tripped
     * through Java re-encodes to the peer's own bytes. What it cannot do is let
     * a Java caller *construct* an any whose type has no simple name; {@link
     * #of} says so rather than guessing.
     *
     * *열지 않고 그대로 중계한다 — 이 버전이 모르는 타입도 손실 없이 통과한다.*
     */
    public static final class Any {
        /** The `_t` half: a JSON document, a name or a structural form. */
        public final Object typeForm;
        /** The `_v` half: a JSON document. */
        public final Object valueForm;

        public Any(Object _typeForm, Object _valueForm) {
            this.typeForm = _typeForm;
            this.valueForm = _valueForm;
        }

        /**
         * The value behind the `_t` half, or the refusal that names why not.
         *
         * Relaying is the default (see the class comment) and this is what a
         * caller asks when it wants the value rather than the document. Two
         * outcomes are refusals rather than answers, and they are the point:
         *
         * * a type **the wire cannot carry** — a `fixed`, a `valuetype`, an
         *   abstract interface, a `native`, a `::CORBA::Principal` — is refused
         *   with the sentence whose home is `orbweaver-dynamic`, so a Java
         *   caller is told what a Rust or Python caller is told, in the same
         *   words. D008's asymmetry is why this is reachable at all: the
         *   *description* crossed, and only the value stops;
         * * a type this runtime does not rebuild — a struct a peer described
         *   structurally — says so, rather than pretending the document is
         *   opaque for the same reason a `fixed` is.
         */
        public Object open() {
            Desc _d = _descOfForm(typeForm, "_t");
            return _fromJson(_d, valueForm, "_v");
        }

        /** An `any` holding a value of a type whose whole identity is its name. */
        public static Any of(Desc _desc, Object _value) {
            Desc _d = _resolve(_desc, "");
            String _name = _anyName(_d);
            if (_name == null) {
                throw new MarshalError("",
                        "this runtime constructs an any only for a type whose identity is its"
                        + " name; relay a peer's document to carry any other type");
            }
            return new Any(_name, _toJson(_d, _value, "_v"));
        }

        @Override
        public boolean equals(Object _other) {
            if (!(_other instanceof Any)) {
                return false;
            }
            Any _o = (Any) _other;
            return _writeJson(typeForm).equals(_writeJson(_o.typeForm))
                    && _writeJson(valueForm).equals(_writeJson(_o.valueForm));
        }

        @Override
        public int hashCode() {
            return _writeJson(typeForm).hashCode() * 31 + _writeJson(valueForm).hashCode();
        }

        @Override
        public String toString() {
            return "Any(" + _writeJson(typeForm) + ", " + _writeJson(valueForm) + ")";
        }
    }

    /** A `TypeCode` value: the structural form, relayed rather than rebuilt. */
    public static final class TypeCodeValue {
        public final Object form;

        public TypeCodeValue(Object _form) {
            this.form = _form;
        }

        @Override
        public boolean equals(Object _other) {
            return _other instanceof TypeCodeValue
                    && _writeJson(((TypeCodeValue) _other).form).equals(_writeJson(form));
        }

        @Override
        public int hashCode() {
            return _writeJson(form).hashCode();
        }

        @Override
        public String toString() {
            return "TypeCode(" + _writeJson(form) + ")";
        }
    }

    /**
     * The descriptor a peer's `_t` half describes, or the refusal for a type
     * whose value cannot cross.
     *
     * AnyJSON v1.1 (D008) writes a type either as a **name**, when its whole
     * identity is one, or as a **structural form** — an object with a `"kind"`.
     * This reads the first completely and the second only far enough to answer
     * honestly: the five families whose values cannot cross are refused with
     * their published sentences, and everything else says that this runtime
     * relays a structural type rather than rebuilding it.
     *
     * Rebuilding is what the Python runtime does, and it is a larger claim than
     * this target makes today: a Java class cannot be synthesised at run time
     * without a class loader, and a `Map` pretending to be a struct would be a
     * second value representation for the same type. The relay keeps the
     * document exact — a peer's `any` round-trips through Java to the peer's own
     * bytes — and this method is where a caller that wants more is told what it
     * is getting.
     */
    public static Desc _descOfForm(Object _form, String _path) {
        if (_form instanceof String) {
            Desc _named = _descOfName((String) _form);
            if (_named == null) {
                throw new MarshalError(_path, "no type is named " + _form);
            }
            return _named;
        }
        if (!(_form instanceof Map)) {
            throw new MarshalError(_path, "a type is a name or an object with a \"kind\"");
        }
        Map<?, ?> _m = (Map<?, ?>) _form;
        Object _kind = _m.get("kind");
        if (!(_kind instanceof String)) {
            throw new MarshalError(_path, "a structural type needs a \"kind\"");
        }
        String _k = (String) _kind;
        String _id = _m.get("id") instanceof String ? (String) _m.get("id") : "";
        String _name = _m.get("name") instanceof String ? (String) _m.get("name") : "";
        if (_k.equals("fixed")) {
            long _digits = _m.get("digits") instanceof Num ? ((Num) _m.get("digits")).asLong() : 0;
            long _scale = _m.get("scale") instanceof Num ? ((Num) _m.get("scale")).asLong() : 0;
            return new FixedD((int) _digits, (int) _scale);
        }
        if (_k.equals("value")) {
            return new NoWire("deferred", _subject("valuetype", _name, _id));
        }
        if (_k.equals("abstract_interface")) {
            return new NoWire("deferred", _subject("abstract interface", _name, _id));
        }
        if (_k.equals("native")) {
            return new NoWire("unmarshallable", _subject("native", _name, _id));
        }
        if (_k.equals("principal")) {
            return new NoWire("withdrawn", _principalSubject());
        }
        if (_k.equals("string") || _k.equals("wstring")) {
            long _bound = _m.get("bound") instanceof Num ? ((Num) _m.get("bound")).asLong() : 0;
            return new Str(_k.equals("wstring"), _bound);
        }
        if (_k.equals("objref")) {
            return new ObjRef(_id);
        }
        if (_k.equals("seq") || _k.equals("array")) {
            Desc _elem = _descOfForm(_m.get("element"), _path + ".element");
            if (_k.equals("seq")) {
                long _bound = _m.get("bound") instanceof Num ? ((Num) _m.get("bound")).asLong() : 0;
                return new Seq(_elem, _bound);
            }
            long _len = _m.get("length") instanceof Num ? ((Num) _m.get("length")).asLong() : 0;
            return new Arr(_elem, (int) _len);
        }
        if (_k.equals("recursive")) {
            return new Ref(_id);
        }
        // A named aggregate the receiving package already declares is resolved
        // by id — the ordinary case, and the one that costs nothing.
        if (!_id.isEmpty() && TYPES.containsKey(_id)) {
            return new Ref(_id);
        }
        throw new MarshalError(_path,
                "this runtime relays a structural " + _k + " rather than rebuilding it: the"
                + " document crossed whole and can be sent back unchanged, but no Java type"
                + " is synthesised for a " + _k + " the generated package does not declare"
                + (_id.isEmpty() ? "" : " (" + _id + ")"));
    }

    /** The descriptor for a type whose whole identity is its name, or null. */
    public static Desc _descOfName(String _name) {
        if (_name.equals("boolean")) {
            return BOOLEAN;
        }
        if (_name.equals("octet")) {
            return OCTET;
        }
        if (_name.equals("char")) {
            return CHAR;
        }
        if (_name.equals("wchar")) {
            return WCHAR;
        }
        if (_name.equals("short")) {
            return SHORT;
        }
        if (_name.equals("unsigned short")) {
            return USHORT;
        }
        if (_name.equals("long")) {
            return LONG;
        }
        if (_name.equals("unsigned long")) {
            return ULONG;
        }
        if (_name.equals("long long")) {
            return LONGLONG;
        }
        if (_name.equals("unsigned long long")) {
            return ULONGLONG;
        }
        if (_name.equals("float")) {
            return FLOAT;
        }
        if (_name.equals("double")) {
            return DOUBLE;
        }
        if (_name.equals("long double")) {
            return LONGDOUBLE;
        }
        if (_name.equals("string")) {
            return new Str(false, 0);
        }
        if (_name.equals("wstring")) {
            return new Str(true, 0);
        }
        if (_name.equals("any")) {
            return ANY;
        }
        if (_name.equals("typecode")) {
            return TYPECODE;
        }
        if (_name.equals("void") || _name.equals("null")) {
            return VOID;
        }
        return null;
    }

    // ── the type registry ───────────────────────────────────────────────────

    /** Builds a value of a generated type from its members, in order. */
    public interface Make {
        Object make(Object[] _parts);
    }

    /** Takes a generated value apart into its members, in order. */
    public interface Parts {
        Object[] parts(Object _value);
    }

    /** One member of a struct, an exception or a union branch. */
    public static final class Member {
        /** The name that travels — the IDL one, never the Java spelling. */
        public final String name;
        public final Desc desc;

        public Member(String _name, Desc _desc) {
            this.name = _name;
            this.desc = _desc;
        }
    }

    public static Member _member(String _name, Desc _desc) {
        return new Member(_name, _desc);
    }

    /** One union branch: its labels, its member and where a default sits. */
    public static final class Branch {
        public final Object[] labels;
        public final String name;
        public final Desc desc;
        /** How many of this branch's labels precede `default:`, or -1. */
        public final int defaultSlot;

        public Branch(Object[] _labels, String _name, Desc _desc, int _defaultSlot) {
            this.labels = _labels;
            this.name = _name;
            this.desc = _desc;
            this.defaultSlot = _defaultSlot;
        }
    }

    public static Branch _branch(Object[] _labels, String _name, Desc _desc, int _defaultSlot) {
        return new Branch(_labels, _name, _desc, _defaultSlot);
    }

    /** What the registry holds for one named type. */
    public static final class Type {
        public final String kind;   // struct, except, enum, union, alias
        public final String id;
        public final String name;
        public final Class<?> cls;
        public final Member[] members;
        public final String[] enumerators;
        public final Desc disc;
        public final Branch[] branches;
        public final Desc alias;
        public final Make make;
        public final Parts parts;

        Type(String _kind, String _id, String _name, Class<?> _cls, Member[] _members,
                String[] _enumerators, Desc _disc, Branch[] _branches, Desc _alias,
                Make _make, Parts _parts) {
            this.kind = _kind;
            this.id = _id;
            this.name = _name;
            this.cls = _cls;
            this.members = _members;
            this.enumerators = _enumerators;
            this.disc = _disc;
            this.branches = _branches;
            this.alias = _alias;
            this.make = _make;
            this.parts = _parts;
        }
    }

    private static final Map<String, Type> TYPES = new TreeMap<String, Type>();
    private static final Map<String, String> NAMES = new TreeMap<String, String>();

    /** Registers a struct or an exception. `_kind` is `struct` or `except`. */
    public static void _registerRecord(String _kind, String _id, String _name, Class<?> _cls,
            Member[] _members, Make _make, Parts _parts) {
        TYPES.put(_id, new Type(_kind, _id, _name, _cls, _members, null, null, null, null,
                _make, _parts));
        NAMES.put(_id, _name);
    }

    /** Registers an enum. `_make` takes the enumerator's IDL name. */
    public static void _registerEnum(String _id, String _name, Class<?> _cls,
            String[] _enumerators, Make _make, Parts _parts) {
        TYPES.put(_id, new Type("enum", _id, _name, _cls, null, _enumerators, null, null, null,
                _make, _parts));
        NAMES.put(_id, _name);
    }

    /** Registers a union. `_parts` answers `{discriminator, value-or-null}`. */
    public static void _registerUnion(String _id, String _name, Class<?> _cls, Desc _disc,
            Branch[] _branches, Make _make, Parts _parts) {
        TYPES.put(_id, new Type("union", _id, _name, _cls, null, null, _disc, _branches, null,
                _make, _parts));
        NAMES.put(_id, _name);
    }

    /** Registers a typedef: transparent to the wire, as it is to CDR. */
    public static void _registerAlias(String _id, String _name, Desc _alias) {
        TYPES.put(_id, new Type("alias", _id, _name, null, null, null, null, null, _alias,
                null, null));
        NAMES.put(_id, _name);
    }

    /**
     * Registers a name with no type behind it.
     *
     * An interface that could not be emitted is still named by every reference
     * to it, and a TypeCode with an empty name is a byte the Rust target does
     * not write.
     */
    public static void _registerName(String _id, String _name) {
        NAMES.put(_id, _name);
    }

    public static String _nameOf(String _id) {
        String _n = NAMES.get(_id);
        return _n == null ? "" : _n;
    }

    public static Type _type(String _id) {
        return TYPES.get(_id);
    }

    /** Follows `Ref` and typedef chains to something with a shape. */
    static Desc _resolve(Desc _desc, String _path) {
        Desc _d = _desc;
        int _seen = 0;
        while (_d instanceof Ref) {
            String _id = ((Ref) _d).id;
            Type _t = TYPES.get(_id);
            if (_t == null) {
                throw new MarshalError(_path, "no type is registered under " + _id);
            }
            if (!"alias".equals(_t.kind)) {
                return _d;
            }
            _d = _t.alias;
            if (++_seen > 64) {
                throw new MarshalError(_path, "typedef chain does not terminate");
            }
        }
        return _d;
    }

    private static Type _named(Desc _d, String _path) {
        Type _t = TYPES.get(((Ref) _d).id);
        if (_t == null) {
            throw new MarshalError(_path, "no type is registered under " + ((Ref) _d).id);
        }
        return _t;
    }

    // ── AnyJSON v1 (docs/PLAN.md §4.5) ──────────────────────────────────────

    private static String _member(String _path, String _name) {
        return _path.isEmpty() ? _name : _path + "." + _name;
    }

    private static String _index(String _path, int _i) {
        return _path + "[" + _i + "]";
    }

    /** The name a type whose whole identity fits in one carries in `_t`. */
    private static String _anyName(Desc _d) {
        if (!(_d instanceof Prim)) {
            if (_d instanceof Str) {
                Str _s = (Str) _d;
                return _s.bound == 0 ? (_s.wide ? "wstring" : "string") : null;
            }
            return null;
        }
        String _k = ((Prim) _d).kind;
        if (_k.equals("ushort")) {
            return "unsigned short";
        }
        if (_k.equals("ulong")) {
            return "unsigned long";
        }
        if (_k.equals("longlong")) {
            return "long long";
        }
        if (_k.equals("ulonglong")) {
            return "unsigned long long";
        }
        if (_k.equals("longdouble")) {
            return "long double";
        }
        return _k;
    }

    private static long _intOf(Object _value, String _path, String _kind) {
        if (_value instanceof Byte) {
            return ((Byte) _value).longValue() & 0xFFL;
        }
        if (_value instanceof Short) {
            return ((Short) _value).longValue();
        }
        if (_value instanceof Integer) {
            return ((Integer) _value).longValue();
        }
        if (_value instanceof Long) {
            return ((Long) _value).longValue();
        }
        if (_value instanceof Character) {
            return (long) ((Character) _value).charValue();
        }
        throw new MarshalError(_path, "expected an " + _kind + ", got " + _describe(_value));
    }

    private static String _describe(Object _value) {
        return _value == null ? "null" : _value.getClass().getSimpleName() + " " + _value;
    }

    private static void _range(long _v, long _lo, long _hi, String _kind, String _path) {
        if (_v < _lo || _v > _hi) {
            throw new MarshalError(_path, _v + " is outside " + _kind);
        }
    }

    /** Renders a Java value as its AnyJSON form, or says exactly what is wrong. */
    public static Object _toJson(Desc _desc, Object _value, String _path) {
        Desc _d = _resolve(_desc, _path);

        if (_d instanceof NoWire) {
            NoWire _n = (NoWire) _d;
            throw new MarshalError(_path, _refusal(_n));
        }
        if (_d instanceof FixedD) {
            FixedD _f = (FixedD) _d;
            throw new MarshalError(_path,
                    _deferred("fixed<" + _f.digits + "," + _f.scale + ">"));
        }
        if (_d instanceof Prim) {
            return _primToJson(((Prim) _d).kind, _value, _path);
        }
        if (_d instanceof Str) {
            if (!(_value instanceof String)) {
                throw new MarshalError(_path, "expected a "
                        + (((Str) _d).wide ? "wstring" : "string") + ", got " + _describe(_value));
            }
            return _value;
        }
        if (_d instanceof ObjRef) {
            LinkedHashMap<String, Object> _out = new LinkedHashMap<String, Object>();
            if (_value == null) {
                _out.put("_ref", null);
                return _out;
            }
            if (!(_value instanceof ObjectRef)) {
                throw new MarshalError(_path,
                        "expected an ObjectRef or null, got " + _describe(_value));
            }
            ObjectRef _r = (ObjectRef) _value;
            _out.put("_ref", _r.handle);
            _out.put("_type", _r.typeId.isEmpty() ? ((ObjRef) _d).id : _r.typeId);
            return _out;
        }
        if (_d instanceof Seq || _d instanceof Arr) {
            boolean _isSeq = _d instanceof Seq;
            Desc _elem = _isSeq ? ((Seq) _d).element : ((Arr) _d).element;
            Desc _re = _resolve(_elem, _path);
            if (_isSeq && _re instanceof Prim && ((Prim) _re).kind.equals("octet")) {
                if (!(_value instanceof byte[])) {
                    throw new MarshalError(_path,
                            "a sequence<octet> is a byte[], got " + _describe(_value));
                }
                return Base64.getEncoder().encodeToString((byte[]) _value);
            }
            if (!(_value instanceof List)) {
                throw new MarshalError(_path, "expected a List, got " + _describe(_value));
            }
            List<?> _items = (List<?>) _value;
            if (!_isSeq && _items.size() != ((Arr) _d).length) {
                throw new MarshalError(_path, "this array has " + ((Arr) _d).length
                        + " elements, " + _items.size() + " given");
            }
            ArrayList<Object> _out = new ArrayList<Object>(_items.size());
            for (int _i = 0; _i < _items.size(); _i++) {
                _out.add(_toJson(_elem, _items.get(_i), _index(_path, _i)));
            }
            return _out;
        }
        if (_d instanceof Ref) {
            Type _t = _named(_d, _path);
            if (_t.kind.equals("enum")) {
                Object[] _p = _t.parts.parts(_value);
                return _p[0];
            }
            if (_t.kind.equals("union")) {
                Object[] _p = _t.parts.parts(_value);
                LinkedHashMap<String, Object> _out = new LinkedHashMap<String, Object>();
                Object _discJson = _toJson(_t.disc, _p[0], _member(_path, "_d"));
                _out.put("_d", _discJson);
                Branch _b = _caseFor(_t, _discJson);
                if (_p[1] != null) {
                    if (_b == null) {
                        throw new MarshalError(_path,
                                "a union with a value but no selected branch");
                    }
                    _out.put("_v", _toJson(_b.desc, _p[1], _member(_path, "_v")));
                }
                return _out;
            }
            // struct or exception
            Object[] _p = _t.parts.parts(_value);
            LinkedHashMap<String, Object> _out = new LinkedHashMap<String, Object>();
            for (int _i = 0; _i < _t.members.length; _i++) {
                Member _m = _t.members[_i];
                _out.put(_m.name, _toJson(_m.desc, _p[_i], _member(_path, _m.name)));
            }
            return _out;
        }
        throw new MarshalError(_path, "no AnyJSON form for this type");
    }

    private static String _refusal(NoWire _n) {
        if (_n.family.equals("unmarshallable")) {
            return _unmarshallable(_n.subject);
        }
        if (_n.family.equals("withdrawn")) {
            return _withdrawn(_n.subject);
        }
        return _deferred(_n.subject);
    }

    private static Object _primToJson(String _kind, Object _value, String _path) {
        if (_kind.equals("boolean")) {
            if (!(_value instanceof Boolean)) {
                throw new MarshalError(_path, "expected a boolean, got " + _describe(_value));
            }
            return _value;
        }
        if (_kind.equals("char")) {
            long _c = _intOf(_value, _path, "char");
            _range(_c, 0, 255, "char", _path);
            return Num.of(_c);
        }
        if (_kind.equals("wchar")) {
            if (!(_value instanceof Character)) {
                throw new MarshalError(_path, "expected a wchar, got " + _describe(_value));
            }
            return String.valueOf(((Character) _value).charValue());
        }
        if (_kind.equals("octet")) {
            long _v = _intOf(_value, _path, "octet");
            _range(_v, 0, 255, "octet", _path);
            return Num.of(_v);
        }
        if (_kind.equals("short")) {
            long _v = _intOf(_value, _path, "short");
            _range(_v, -32768, 32767, "short", _path);
            return Num.of(_v);
        }
        if (_kind.equals("ushort")) {
            long _v = _intOf(_value, _path, "ushort");
            _range(_v, 0, 65535, "ushort", _path);
            return Num.of(_v);
        }
        if (_kind.equals("long")) {
            long _v = _intOf(_value, _path, "long");
            _range(_v, -2147483648L, 2147483647L, "long", _path);
            return Num.of(_v);
        }
        if (_kind.equals("ulong")) {
            long _v = _intOf(_value, _path, "ulong");
            _range(_v, 0, 4294967295L, "ulong", _path);
            return Num.of(_v);
        }
        if (_kind.equals("longlong")) {
            // The two 64-bit types cross as JSON **strings**: a JSON number is a
            // double in every mainstream implementation, so anything past 2^53
            // loses digits silently.
            return Long.toString(_intOf(_value, _path, "longlong"));
        }
        if (_kind.equals("ulonglong")) {
            if (!(_value instanceof Long)) {
                throw new MarshalError(_path,
                        "an unsigned long long is a Java long read as unsigned, got "
                        + _describe(_value));
            }
            return Long.toUnsignedString(((Long) _value).longValue());
        }
        if (_kind.equals("float") || _kind.equals("double")) {
            double _x;
            if (_value instanceof Float) {
                _x = ((Float) _value).doubleValue();
            } else if (_value instanceof Double) {
                _x = ((Double) _value).doubleValue();
            } else {
                throw new MarshalError(_path, "expected a float, got " + _describe(_value));
            }
            if (Double.isNaN(_x)) {
                return _tagged("_f", "nan");
            }
            if (Double.isInfinite(_x)) {
                return _tagged("_f", _x > 0 ? "+inf" : "-inf");
            }
            // A float is printed as a float: `Double.toString((double) 0.1f)` is
            // 0.10000000149011612, which is the same VALUE and a document the
            // reference mapping's shortest spelling would never write.
            String _text = _value instanceof Float
                    ? Float.toString(((Float) _value).floatValue())
                    : Double.toString(_x);
            return new Num(_text);
        }
        if (_kind.equals("longdouble")) {
            if (!(_value instanceof LongDouble)) {
                throw new MarshalError(_path, "expected a LongDouble, got " + _describe(_value));
            }
            return Base64.getEncoder().encodeToString(((LongDouble) _value).octets);
        }
        if (_kind.equals("any")) {
            if (!(_value instanceof Any)) {
                throw new MarshalError(_path, "expected an Any, got " + _describe(_value));
            }
            LinkedHashMap<String, Object> _out = new LinkedHashMap<String, Object>();
            _out.put("_t", ((Any) _value).typeForm);
            _out.put("_v", ((Any) _value).valueForm);
            return _out;
        }
        if (_kind.equals("typecode")) {
            if (!(_value instanceof TypeCodeValue)) {
                throw new MarshalError(_path, "expected a TypeCode, got " + _describe(_value));
            }
            return ((TypeCodeValue) _value).form;
        }
        if (_kind.equals("void")) {
            return null;
        }
        throw new MarshalError(_path, "no AnyJSON form for " + _kind);
    }

    private static Map<String, Object> _tagged(String _key, String _value) {
        LinkedHashMap<String, Object> _out = new LinkedHashMap<String, Object>();
        _out.put(_key, _value);
        return _out;
    }

    /** Reads an AnyJSON document as the Java value `_desc` describes. */
    public static Object _fromJson(Desc _desc, Object _j, String _path) {
        Desc _d = _resolve(_desc, _path);

        if (_d instanceof NoWire) {
            throw new MarshalError(_path, _refusal((NoWire) _d));
        }
        if (_d instanceof FixedD) {
            FixedD _f = (FixedD) _d;
            throw new MarshalError(_path,
                    _deferred("fixed<" + _f.digits + "," + _f.scale + ">"));
        }
        if (_d instanceof Prim) {
            return _primFromJson(((Prim) _d).kind, _j, _path);
        }
        if (_d instanceof Str) {
            if (!(_j instanceof String)) {
                throw new MarshalError(_path, "expected a "
                        + (((Str) _d).wide ? "wstring" : "string") + ", got " + _json(_j));
            }
            return _j;
        }
        if (_d instanceof ObjRef) {
            if (!(_j instanceof Map) || !((Map<?, ?>) _j).containsKey("_ref")) {
                throw new MarshalError(_path, "an object reference is {\"_ref\": ...}");
            }
            Map<?, ?> _m = (Map<?, ?>) _j;
            Object _h = _m.get("_ref");
            if (_h == null) {
                return null;
            }
            Object _t = _m.get("_type");
            return new ObjectRef((String) _h, _t instanceof String ? (String) _t
                    : ((ObjRef) _d).id);
        }
        if (_d instanceof Seq || _d instanceof Arr) {
            boolean _isSeq = _d instanceof Seq;
            Desc _elem = _isSeq ? ((Seq) _d).element : ((Arr) _d).element;
            Desc _re = _resolve(_elem, _path);
            if (_isSeq && _re instanceof Prim && ((Prim) _re).kind.equals("octet")) {
                return _unbase64(_j, _path);
            }
            if (!(_j instanceof List)) {
                throw new MarshalError(_path, "expected an array, got " + _json(_j));
            }
            List<?> _items = (List<?>) _j;
            if (!_isSeq && _items.size() != ((Arr) _d).length) {
                throw new MarshalError(_path, "this array has " + ((Arr) _d).length
                        + " elements, " + _items.size() + " given");
            }
            ArrayList<Object> _out = new ArrayList<Object>(_items.size());
            for (int _i = 0; _i < _items.size(); _i++) {
                _out.add(_fromJson(_elem, _items.get(_i), _index(_path, _i)));
            }
            return _out;
        }
        if (_d instanceof Ref) {
            Type _t = _named(_d, _path);
            if (_t.kind.equals("enum")) {
                if (!(_j instanceof String)) {
                    throw new MarshalError(_path,
                            "an enumerator of " + _t.name + " is named, not numbered");
                }
                return _t.make.make(new Object[] {_j});
            }
            if (_t.kind.equals("union")) {
                if (!(_j instanceof Map) || !((Map<?, ?>) _j).containsKey("_d")) {
                    throw new MarshalError(_path,
                            "a " + _t.name + " needs an explicit discriminator in \"_d\"");
                }
                Map<?, ?> _m = (Map<?, ?>) _j;
                Object _disc = _fromJson(_t.disc, _m.get("_d"), _member(_path, "_d"));
                Branch _b = _caseFor(_t, _m.get("_d"));
                if (_b == null) {
                    if (_m.containsKey("_v")) {
                        throw new MarshalError(_path,
                                "the selected branch of " + _t.name + " has no member");
                    }
                    return _t.make.make(new Object[] {_disc, null});
                }
                if (!_m.containsKey("_v")) {
                    throw new MarshalError(_path,
                            "branch " + _b.name + " of " + _t.name + " needs a \"_v\"");
                }
                Object _v = _fromJson(_b.desc, _m.get("_v"), _member(_path, "_v"));
                return _t.make.make(new Object[] {_disc, _v});
            }
            if (!(_j instanceof Map)) {
                throw new MarshalError(_path, "expected an object, got " + _json(_j));
            }
            Map<?, ?> _m = (Map<?, ?>) _j;
            for (Object _k : _m.keySet()) {
                boolean _known = false;
                for (Member _mem : _t.members) {
                    if (_mem.name.equals(_k)) {
                        _known = true;
                        break;
                    }
                }
                if (!_known) {
                    // Not ignored: an unknown member is either a typo or a peer
                    // built against a different contract, and both are worth
                    // knowing.
                    throw new MarshalError(_path, _t.name + " has no member " + _k);
                }
            }
            Object[] _parts = new Object[_t.members.length];
            for (int _i = 0; _i < _t.members.length; _i++) {
                Member _mem = _t.members[_i];
                if (!_m.containsKey(_mem.name)) {
                    throw new MarshalError(_path,
                            _t.name + " needs a member " + _mem.name);
                }
                _parts[_i] = _fromJson(_mem.desc, _m.get(_mem.name),
                        _member(_path, _mem.name));
            }
            return _t.make.make(_parts);
        }
        throw new MarshalError(_path, "no AnyJSON form for this type");
    }

    private static String _json(Object _j) {
        return _writeJson(_j);
    }

    private static Object _primFromJson(String _kind, Object _j, String _path) {
        if (_kind.equals("boolean")) {
            if (!(_j instanceof Boolean)) {
                throw new MarshalError(_path, "expected a boolean, got " + _json(_j));
            }
            return _j;
        }
        if (_kind.equals("char")) {
            long _v = _num(_j, _path, "char");
            _range(_v, 0, 255, "char", _path);
            return Character.valueOf((char) _v);
        }
        if (_kind.equals("wchar")) {
            if (!(_j instanceof String) || ((String) _j).length() != 1) {
                throw new MarshalError(_path, "a wchar is a string of exactly one character");
            }
            return Character.valueOf(((String) _j).charAt(0));
        }
        if (_kind.equals("octet")) {
            long _v = _num(_j, _path, "octet");
            _range(_v, 0, 255, "octet", _path);
            return Byte.valueOf((byte) _v);
        }
        if (_kind.equals("short")) {
            long _v = _num(_j, _path, "short");
            _range(_v, -32768, 32767, "short", _path);
            return Short.valueOf((short) _v);
        }
        if (_kind.equals("ushort")) {
            long _v = _num(_j, _path, "ushort");
            _range(_v, 0, 65535, "ushort", _path);
            return Integer.valueOf((int) _v);
        }
        if (_kind.equals("long")) {
            long _v = _num(_j, _path, "long");
            _range(_v, -2147483648L, 2147483647L, "long", _path);
            return Integer.valueOf((int) _v);
        }
        if (_kind.equals("ulong")) {
            long _v = _num(_j, _path, "ulong");
            _range(_v, 0, 4294967295L, "ulong", _path);
            return Long.valueOf(_v);
        }
        if (_kind.equals("longlong")) {
            return Long.valueOf(_wide(_j, _path, false));
        }
        if (_kind.equals("ulonglong")) {
            return Long.valueOf(_wide(_j, _path, true));
        }
        if (_kind.equals("float") || _kind.equals("double")) {
            double _x = _floatIn(_j, _path);
            return _kind.equals("float") ? (Object) Float.valueOf((float) _x)
                    : (Object) Double.valueOf(_x);
        }
        if (_kind.equals("longdouble")) {
            return new LongDouble(_unbase64(_j, _path));
        }
        if (_kind.equals("any")) {
            if (!(_j instanceof Map) || !((Map<?, ?>) _j).containsKey("_t")
                    || !((Map<?, ?>) _j).containsKey("_v")) {
                throw new MarshalError(_path, "an any is {\"_t\": <type>, \"_v\": <value>}");
            }
            Map<?, ?> _m = (Map<?, ?>) _j;
            return new Any(_m.get("_t"), _m.get("_v"));
        }
        if (_kind.equals("typecode")) {
            return new TypeCodeValue(_j);
        }
        if (_kind.equals("void")) {
            return null;
        }
        throw new MarshalError(_path, "no AnyJSON form for " + _kind);
    }

    private static long _num(Object _j, String _path, String _kind) {
        if (!(_j instanceof Num) || !((Num) _j).isIntegral()) {
            throw new MarshalError(_path, "expected an " + _kind + ", got " + _json(_j));
        }
        try {
            return ((Num) _j).asLong();
        } catch (NumberFormatException _bad) {
            throw new MarshalError(_path, "expected an " + _kind + ", got " + _json(_j));
        }
    }

    private static long _wide(Object _j, String _path, boolean _unsigned) {
        // A string is what the mapping emits; a number is accepted because a
        // peer that has not read the specification will send one, and it is safe
        // exactly when it survives the trip.
        String _text;
        if (_j instanceof String) {
            _text = (String) _j;
        } else if (_j instanceof Num && ((Num) _j).isIntegral()) {
            _text = ((Num) _j).text;
        } else {
            throw new MarshalError(_path, _json(_j) + " is not an integer");
        }
        try {
            return _unsigned ? Long.parseUnsignedLong(_text) : Long.parseLong(_text);
        } catch (NumberFormatException _bad) {
            throw new MarshalError(_path, _text + " is outside "
                    + (_unsigned ? "unsigned long long" : "long long"));
        }
    }

    private static double _floatIn(Object _j, String _path) {
        if (_j instanceof Map && ((Map<?, ?>) _j).containsKey("_f")) {
            Object _tag = ((Map<?, ?>) _j).get("_f");
            if ("nan".equals(_tag)) {
                return Double.NaN;
            }
            if ("+inf".equals(_tag)) {
                return Double.POSITIVE_INFINITY;
            }
            if ("-inf".equals(_tag)) {
                return Double.NEGATIVE_INFINITY;
            }
            throw new MarshalError(_path, _json(_tag) + " is not nan, +inf or -inf");
        }
        if (!(_j instanceof Num)) {
            throw new MarshalError(_path, "expected a number, got " + _json(_j));
        }
        return ((Num) _j).asDouble();
    }

    private static byte[] _unbase64(Object _j, String _path) {
        if (!(_j instanceof String)) {
            throw new MarshalError(_path, "expected base64 text, got " + _json(_j));
        }
        try {
            return Base64.getDecoder().decode((String) _j);
        } catch (IllegalArgumentException _bad) {
            throw new MarshalError(_path, "not valid base64: " + _bad.getMessage());
        }
    }

    /** The branch a discriminator's *document* selects, or null for none. */
    private static Branch _caseFor(Type _t, Object _discJson) {
        String _want = _writeJson(_discJson);
        Branch _default = null;
        for (Branch _b : _t.branches) {
            if (_b.defaultSlot >= 0) {
                _default = _b;
            }
            for (Object _label : _b.labels) {
                if (_writeJson(_label).equals(_want)) {
                    return _b;
                }
            }
        }
        return _default;
    }

    // ── a call ──────────────────────────────────────────────────────────────

    /** Where a request goes and a reply comes from. */
    public interface Invoker {
        Map<String, Object> invoke(Map<String, Object> _request);
    }

    /** One argument of a call: its wire name, its type and its value. */
    public static final class Arg {
        public final String name;
        public final Desc desc;
        public final Object value;

        public Arg(String _name, Desc _desc, Object _value) {
            this.name = _name;
            this.desc = _desc;
            this.value = _value;
        }
    }

    public static Arg _arg(String _name, Desc _desc, Object _value) {
        return new Arg(_name, _desc, _value);
    }

    /** One `out` or `inout` value the reply carries back. */
    public static final class Out {
        public final String name;
        public final Desc desc;

        public Out(String _name, Desc _desc) {
            this.name = _name;
            this.desc = _desc;
        }
    }

    public static Out _out(String _name, Desc _desc) {
        return new Out(_name, _desc);
    }

    /**
     * One operation, from the arguments a caller passed to the values it gets
     * back — the declared result first when it is not `void`, then the `out` and
     * `inout` values in declaration order.
     *
     * Everything a generated stub does goes through here. The stub contributes
     * names, order and descriptors — the facts of one contract — and no
     * conversion logic at all.
     */
    public static Object[] _call(Invoker _invoker, String _id, String _operation, Arg[] _args,
            Desc _returns, Out[] _outs, boolean _oneway) {
        LinkedHashMap<String, Object> _body = new LinkedHashMap<String, Object>();
        for (Arg _a : _args) {
            _body.put(_a.name, _toJson(_a.desc, _a.value, _a.name));
        }
        LinkedHashMap<String, Object> _request = new LinkedHashMap<String, Object>();
        _request.put(_SEAM_CALL_INTERFACE, _id);
        _request.put(_SEAM_CALL_OPERATION, _operation);
        _request.put(_SEAM_CALL_ARGUMENTS, _body);
        if (_oneway) {
            _request.put(_SEAM_CALL_ONEWAY, Boolean.TRUE);
        }
        Map<String, Object> _reply = _invoker.invoke(_request);

        if (_reply.containsKey(_SEAM_REPLY_ERROR)) {
            Object _e = _reply.get(_SEAM_REPLY_ERROR);
            Object _message = _e instanceof Map ? ((Map<?, ?>) _e).get("message") : null;
            throw new TransportError(_message instanceof String ? (String) _message
                    : "the bridge reported a failure");
        }
        if (_reply.containsKey(_SEAM_REPLY_SYSTEM_EXCEPTION)) {
            Map<?, ?> _s = (Map<?, ?>) _reply.get(_SEAM_REPLY_SYSTEM_EXCEPTION);
            Object _sid = _s.get(_SEAM_EXCEPTION_ID);
            Object _minor = _s.get(_SEAM_EXCEPTION_MINOR);
            Object _completed = _s.get(_SEAM_EXCEPTION_COMPLETED);
            throw new SystemException(_sid instanceof String ? (String) _sid : "",
                    _minor instanceof Num ? ((Num) _minor).asLong() : 0L,
                    _completed instanceof Num ? (int) ((Num) _completed).asLong() : 2);
        }
        if (_reply.containsKey(_SEAM_REPLY_USER_EXCEPTION)) {
            Map<?, ?> _u = (Map<?, ?>) _reply.get(_SEAM_REPLY_USER_EXCEPTION);
            Object _uid = _u.get(_SEAM_EXCEPTION_ID);
            Type _t = _uid instanceof String ? TYPES.get(_uid) : null;
            if (_t == null || !_t.kind.equals("except")) {
                // An id we cannot decode still names a contract the caller was
                // not built against, which is the useful half of the message.
                // `0` and not "YES": §4.11.4 numbers COMPLETED_YES 0, and this
                // is the one path that could put a name where every other one
                // puts the peer's ordinal.
                throw new SystemException("IDL:omg.org/CORBA/UNKNOWN:1.0", 0x4f4d0001L, 0);
            }
            Object _members = _u.get(_SEAM_EXCEPTION_MEMBERS);
            Object _raised = _fromJson(new Ref((String) _uid),
                    _members == null ? new LinkedHashMap<String, Object>() : _members, "");
            throw (UserException) _raised;
        }
        if (!_reply.containsKey(_SEAM_REPLY_OK)) {
            throw new TransportError("the bridge answered with neither a result nor a failure");
        }

        Map<?, ?> _ok = (Map<?, ?>) _reply.get(_SEAM_REPLY_OK);
        ArrayList<Object> _values = new ArrayList<Object>();
        boolean _isVoid = _resolve(_returns, "<return>") instanceof Prim
                && ((Prim) _resolve(_returns, "<return>")).kind.equals("void");
        if (!_isVoid) {
            _values.add(_fromJson(_returns, _ok.get(_SEAM_REPLY_RETURNS), "<return>"));
        }
        Object _outputs = _ok.get(_SEAM_REPLY_OUTPUTS);
        Map<?, ?> _outMap = _outputs instanceof Map ? (Map<?, ?>) _outputs
                : new LinkedHashMap<String, Object>();
        for (Out _o : _outs) {
            if (!_outMap.containsKey(_o.name)) {
                throw new TransportError("the reply is missing the out parameter " + _o.name);
            }
            _values.add(_fromJson(_o.desc, _outMap.get(_o.name), _o.name));
        }
        return _values.toArray();
    }

    // ── the serving direction ───────────────────────────────────────────────
    //
    // The inverse of `_call`, and split the same way: this runtime owns every
    // conversion and the shape of every reply, and generated code contributes
    // only names, order, descriptors and a `switch`. That split is what let the
    // client half say *the stub contributes no conversion logic at all*, and it
    // is the reason a third language enrols by adding one function and one row
    // rather than by reimplementing the seam.
    //
    // Added 2026-09-01. `COMPONENTS.md` had recorded the gap as *"an `Answerer`
    // over the bridge's pipes and a `_Rt.Host`/`dispatchCall` in
    // `java_rt.java` — the two things `python_rt.py` has and `java_rt.java`
    // does not — and NOT anything in the seam's definition."* That was right:
    // nothing below changes the protocol, which `seam::protocol()` publishes and
    // `tests/the_seam_is_one_protocol.rs` asserts both implementations against.

    /** One parameter of an operation, as the servant side needs it. */
    public static final class Param {
        public final String name;
        public final Desc desc;
        /** `true` for `out` and `inout` — what the reply carries back. */
        public final boolean isOut;
        /** `true` for `in` and `inout` — what the call carries in. */
        public final boolean isIn;

        public Param(String _name, Desc _desc, boolean _isIn, boolean _isOut) {
            this.name = _name;
            this.desc = _desc;
            this.isIn = _isIn;
            this.isOut = _isOut;
        }
    }

    /** One operation a servant answers: its parameters, result and mode. */
    public static final class Op {
        public final String name;
        public final Param[] params;
        public final Desc returns;
        public final boolean oneway;

        public Op(String _name, Param[] _params, Desc _returns, boolean _oneway) {
            this.name = _name;
            this.params = _params;
            this.returns = _returns;
            this.oneway = _oneway;
        }
    }

    /**
     * What a generated `<Name>Servant` supplies so this runtime can dispatch.
     *
     * `_idlOperations` is the resolved set — inherited operations flattened —
     * which is the same table the client stub carries, because one function
     * decides both. `_invokeOp` is a generated `switch` and not reflection: a
     * name that reaches it has already been found in that table.
     */
    public interface Servant {
        String _idlId();

        Map<String, Op> _idlOperations();

        /**
         * Calls one operation. `argv` holds the `in` and `inout` values in
         * declaration order; the return is the declared result, or an
         * `Object[]` of the result followed by the `out` and `inout` values
         * when there is more than one thing to answer with — the same tuple
         * shape a client receives, read from the other end.
         */
        Object _invokeOp(String _operation, Object[] _argv);
    }

    /** Raised by a servant to answer with a system exception. */
    public static final class Raise extends RuntimeException {
        private static final long serialVersionUID = 1L;
        public final String id;
        public final long minor;
        public final int completed;

        private Raise(String _id, long _minor, int _completed) {
            super(_id);
            this.id = _id;
            this.minor = _minor;
            this.completed = _completed;
        }

        /**
         * The operation did not run. §4.11.4's COMPLETED_NO is `1`.
         *
         * The three constructors exist rather than one taking an int because
         * the completion status decides whether a caller may retry, and a
         * generator-picked default gets that wrong silently — which is the
         * argument `#[must_use] Raising` makes on the Rust side and the reason
         * `python_rt.py` refuses a raise that names no status.
         */
        public static Raise didNotRun(String _id, long _minor) {
            return new Raise(_id, _minor, 1);
        }

        /** The operation ran to completion. COMPLETED_YES is `0`. */
        public static Raise ranToCompletion(String _id, long _minor) {
            return new Raise(_id, _minor, 0);
        }

        /** It may have run. COMPLETED_MAYBE is `2`. */
        public static Raise mayHaveRun(String _id, long _minor) {
            return new Raise(_id, _minor, 2);
        }
    }

    /** The seam could not carry the answer — this side's fault, not the caller's. */
    public static final class ServantError extends RuntimeException {
        private static final long serialVersionUID = 1L;

        public ServantError(String _message) {
            super(_message);
        }
    }

    /**
     * One call document to one reply document, with no process in sight.
     *
     * A pure function of a servant and a parsed map, for the same reason
     * `python_rt.dispatch_call` is one: it lets a test execute every branch —
     * every refusal, every conversion — with no bridge, no socket and no peer.
     */
    public static Map<String, Object> dispatchCall(Servant _servant, Map<?, ?> _call) {
        Object _opName = _call.get(_SEAM_CALL_OPERATION);
        if (!(_opName instanceof String)) {
            throw new ServantError("a call document needs an \"op\"");
        }
        Op _op = _servant._idlOperations().get(_opName);
        if (_op == null) {
            // The operation is not in this contract at all, which is a
            // different answer from one the servant has not implemented.
            return _systemReply("IDL:omg.org/CORBA/BAD_OPERATION:1.0", 0L, 1);
        }
        Object _rawArgs = _call.get(_SEAM_CALL_ARGUMENTS);
        Map<?, ?> _args = _rawArgs instanceof Map ? (Map<?, ?>) _rawArgs
                : new LinkedHashMap<String, Object>();

        ArrayList<Object> _argv = new ArrayList<Object>();
        for (Param _p : _op.params) {
            if (!_p.isIn) {
                continue;
            }
            if (!_args.containsKey(_p.name)) {
                throw new ServantError(_op.name + " needs an argument " + _p.name);
            }
            _argv.add(_fromJson(_p.desc, _args.get(_p.name), _p.name));
        }

        Object _answer;
        try {
            _answer = _servant._invokeOp(_op.name, _argv.toArray());
        } catch (Raise _r) {
            return _systemReply(_r.id, _r.minor, _r.completed);
        } catch (UserException _u) {
            LinkedHashMap<String, Object> _body = new LinkedHashMap<String, Object>();
            _body.put(_SEAM_EXCEPTION_ID, _u._id());
            _body.put(_SEAM_EXCEPTION_MEMBERS, _toJson(new Ref(_u._id()), _u, "<raised>"));
            LinkedHashMap<String, Object> _reply = new LinkedHashMap<String, Object>();
            _reply.put(_SEAM_REPLY_USER_EXCEPTION, _body);
            return _reply;
        }

        if (_op.oneway) {
            // §9.4.1 gives a oneway no reply to travel in. One is rendered
            // anyway and the bridge drops it — visibly — because a server whose
            // oneway operations fail invisibly is one nobody can debug.
            return _okReply(null, new LinkedHashMap<String, Object>());
        }

        boolean _isVoid = _resolve(_op.returns, "<return>") instanceof Prim
                && ((Prim) _resolve(_op.returns, "<return>")).kind.equals("void");
        int _wanted = (_isVoid ? 0 : 1);
        for (Param _p : _op.params) {
            if (_p.isOut) {
                _wanted++;
            }
        }
        Object[] _parts;
        if (_wanted <= 1) {
            _parts = _wanted == 0 ? new Object[0] : new Object[] {_answer};
        } else {
            if (!(_answer instanceof Object[]) || ((Object[]) _answer).length != _wanted) {
                throw new ServantError(_op.name + " must answer " + _wanted
                        + " values — the result then the out and inout values in declaration"
                        + " order — and answered " + _answer);
            }
            _parts = (Object[]) _answer;
        }

        int _at = 0;
        Object _returns = null;
        if (!_isVoid) {
            _returns = _toJson(_op.returns, _parts[_at], "<return>");
            _at++;
        }
        LinkedHashMap<String, Object> _outputs = new LinkedHashMap<String, Object>();
        for (Param _p : _op.params) {
            if (!_p.isOut) {
                continue;
            }
            _outputs.put(_p.name, _toJson(_p.desc, _parts[_at], _p.name));
            _at++;
        }
        return _okReply(_returns, _outputs);
    }

    private static Map<String, Object> _okReply(Object _returns, Map<String, Object> _outputs) {
        LinkedHashMap<String, Object> _body = new LinkedHashMap<String, Object>();
        _body.put(_SEAM_REPLY_RETURNS, _returns);
        _body.put(_SEAM_REPLY_OUTPUTS, _outputs);
        LinkedHashMap<String, Object> _reply = new LinkedHashMap<String, Object>();
        _reply.put(_SEAM_REPLY_OK, _body);
        return _reply;
    }

    private static Map<String, Object> _systemReply(String _id, long _minor, int _completed) {
        LinkedHashMap<String, Object> _body = new LinkedHashMap<String, Object>();
        _body.put(_SEAM_EXCEPTION_ID, _id);
        _body.put(_SEAM_EXCEPTION_MINOR, Num.of(_minor));
        _body.put(_SEAM_EXCEPTION_COMPLETED, Num.of(_completed));
        LinkedHashMap<String, Object> _reply = new LinkedHashMap<String, Object>();
        _reply.put(_SEAM_REPLY_SYSTEM_EXCEPTION, _body);
        return _reply;
    }

    /**
     * Answer seam calls arriving on **this process's own** stdin.
     *
     * The inverse of `Bridge`, and the mirror of `python_rt.serve_on_pipes`: a
     * Rust process spawns `java`, hands it a servant, and mounts the result as a
     * `Dispatch` in a server it owns. No listener, no address — which is what
     * keeps a language swap a language swap rather than a move to another
     * endpoint, and those are different rows of D029 §6.1.
     *
     * **stdout is the protocol.** Anything the servant prints there corrupts the
     * conversation; print to `System.err`. This does not redirect stdout on the
     * servant's behalf, because silently moving a stream is worse than a garbled
     * line somebody can see.
     */
    public static void serveOnPipes(Servant _servant) throws IOException {
        BufferedReader _in = new BufferedReader(
                new InputStreamReader(System.in, StandardCharsets.UTF_8));
        PrintStream _out = new PrintStream(new FileOutputStream(FileDescriptor.out), true, "UTF-8");
        String _line;
        while ((_line = _in.readLine()) != null) {
            if (_line.trim().isEmpty()) {
                continue;
            }
            Object _document = _parseJson(_line);
            if (!(_document instanceof Map)) {
                continue;
            }
            Object _call = ((Map<?, ?>) _document).get(_SEAM_ENVELOPE_CALL);
            if (!(_call instanceof Map)) {
                continue;
            }
            Map<String, Object> _reply;
            try {
                _reply = dispatchCall(_servant, (Map<?, ?>) _call);
            } catch (ServantError _e) {
                // The seam could not carry the answer. The caller is told the
                // least wrong true thing — UNKNOWN, completion MAYBE, because
                // the servant's method may well have run before the shape
                // failed — and the message stays in this process, where
                // somebody can act on it.
                _reply = _systemReply("IDL:omg.org/CORBA/UNKNOWN:1.0", 0L, 2);
                System.err.println("orbweaver servant: " + _e.getMessage());
            }
            _out.println(_writeJson(_reply));
            _out.flush();
        }
    }

    /**
     * An invoker that answers from a script instead of from a peer.
     *
     * Present so that generated code can be **executed** by a test with no ORB,
     * no fixture and no network: the requests it records are the AnyJSON a real
     * call would have sent, which is what the cross-implementation oracle
     * compares against the Rust mapping.
     */
    public static final class Loopback implements Invoker {
        public final List<Map<String, Object>> requests = new ArrayList<Map<String, Object>>();
        private final List<Map<String, Object>> replies = new ArrayList<Map<String, Object>>();

        public Loopback() {}

        public Loopback(List<Map<String, Object>> _replies) {
            replies.addAll(_replies);
        }

        /** Queues one reply document, written as JSON text. */
        @SuppressWarnings("unchecked")
        public Loopback reply(String _json) {
            replies.add((Map<String, Object>) _parseJson(_json));
            return this;
        }

        @Override
        public Map<String, Object> invoke(Map<String, Object> _request) {
            requests.add(_request);
            if (!replies.isEmpty()) {
                return replies.remove(0);
            }
            LinkedHashMap<String, Object> _ok = new LinkedHashMap<String, Object>();
            LinkedHashMap<String, Object> _body = new LinkedHashMap<String, Object>();
            _body.put(_SEAM_REPLY_RETURNS, null);
            _body.put(_SEAM_REPLY_OUTPUTS, new LinkedHashMap<String, Object>());
            _ok.put(_SEAM_REPLY_OK, _body);
            return _ok;
        }
    }

    /**
     * An invoker backed by the `orbweaver-py-bridge` process.
     *
     * The bridge is where the wire is. It is started with the contract and the
     * target's IOR, it holds one connection, and it speaks one JSON document per
     * line in each direction. Java never sees an IOR, a GIOP header or a byte of
     * CDR.
     */
    public static final class Bridge implements Invoker, AutoCloseable {
        private final Process proc;
        private final BufferedReader in;
        private final Writer out;
        /** What the bridge said when it was ready: its type id and contract. */
        public final Map<String, Object> ready;

        @SuppressWarnings("unchecked")
        public Bridge(List<String> _command, String _idl, String _ior) {
            ArrayList<String> _argv = new ArrayList<String>(_command);
            _argv.add("--idl");
            _argv.add(_idl);
            _argv.add("--ior");
            _argv.add(_ior);
            try {
                ProcessBuilder _pb = new ProcessBuilder(_argv);
                _pb.redirectErrorStream(false);
                proc = _pb.start();
                in = new BufferedReader(new InputStreamReader(proc.getInputStream(),
                        StandardCharsets.UTF_8));
                out = new OutputStreamWriter(proc.getOutputStream(), StandardCharsets.UTF_8);
            } catch (IOException _e) {
                throw new TransportError("the bridge did not start: " + _e.getMessage());
            }
            String _hello;
            try {
                _hello = in.readLine();
            } catch (IOException _e) {
                throw new TransportError("the bridge did not start: " + _e.getMessage());
            }
            if (_hello == null || _hello.trim().isEmpty()) {
                proc.destroy();
                throw new TransportError("the bridge did not start");
            }
            Object _banner = _parseJson(_hello);
            if (!(_banner instanceof Map) || !((Map<?, ?>) _banner).containsKey("ready")) {
                proc.destroy();
                throw new TransportError("the bridge refused to start: " + _hello.trim());
            }
            ready = (Map<String, Object>) ((Map<?, ?>) _banner).get("ready");
        }

        /** A bridge over `_ior`, speaking the contract in `_idl`. */
        public static Bridge connect(String _binary, String _idl, String _ior) {
            return new Bridge(Arrays.asList(_binary), _idl, _ior);
        }

        @Override
        @SuppressWarnings("unchecked")
        public Map<String, Object> invoke(Map<String, Object> _request) {
            if (!proc.isAlive()) {
                throw new TransportError("the bridge process has exited");
            }
            try {
                out.write(_writeJson(_request));
                out.write("\n");
                out.flush();
            } catch (IOException _e) {
                throw new TransportError("the bridge closed its input: " + _e.getMessage());
            }
            String _line;
            try {
                _line = in.readLine();
            } catch (IOException _e) {
                throw new TransportError("the bridge closed its output: " + _e.getMessage());
            }
            if (_line == null) {
                throw new TransportError("the bridge closed its output");
            }
            Object _reply = _parseJson(_line);
            if (!(_reply instanceof Map)) {
                throw new TransportError("the bridge answered with a " + _line.trim());
            }
            return (Map<String, Object>) _reply;
        }

        @Override
        public void close() {
            try {
                out.close();
            } catch (IOException _ignored) {
                // Closing the pipe is how the bridge is asked to stop; a failure
                // here means it has already gone, which is the same outcome.
            }
            try {
                if (!proc.waitFor(5, java.util.concurrent.TimeUnit.SECONDS)) {
                    proc.destroyForcibly();
                }
            } catch (InterruptedException _e) {
                Thread.currentThread().interrupt();
                proc.destroyForcibly();
            }
        }
    }

    // ── helpers the generated code calls ────────────────────────────────────

    /** Whether two values a generated `equals` compares are equal. */
    public static boolean _eq(Object _a, Object _b) {
        if (_a instanceof byte[] && _b instanceof byte[]) {
            return Arrays.equals((byte[]) _a, (byte[]) _b);
        }
        return _a == null ? _b == null : _a.equals(_b);
    }

    /** How a generated `toString` renders one member. */
    public static String _show(Object _v) {
        if (_v instanceof byte[]) {
            return Base64.getEncoder().encodeToString((byte[]) _v);
        }
        return String.valueOf(_v);
    }
}
