"""The runtime generated Python marshals through — AnyJSON v1, and nothing else.

Copyright (c) Orbweaver contributors. MIT.

This file is **hand-written and shipped verbatim**; the generator copies it
next to the package it emits and never rewrites it. That is the same rule
`orbweaver_gen::rt` follows for Rust and for the same reason: a generated file
holds names and order, never encoding rules, because a code generator is a
machine for duplicating things.

# What is *not* here

CDR, GIOP, IIOP, IORs, byte order, alignment, codeset negotiation. None of it.
A Python client here never touches the wire: it renders its arguments as
AnyJSON (``docs/PLAN.md`` §4.5, the project's own normative JSON<->CDR mapping)
and hands them to a bridge process which does the invocation through the same
dynamic path every other client in this workspace uses. So the wire knowledge
still exists exactly once, in Rust, and the second target language did not buy
a second ORB.

What this file *is*, honestly stated: a second implementation of **§4.5**, in
Python. That is unavoidable — something in Python has to turn Python objects
into whatever crosses the seam — and it is the smallest thing that could be
duplicated, because §4.5 is a written specification with a round-trip
acceptance criterion. ``tests/python_target.rs`` holds this implementation and
the Rust one to the same verdicts over the golden corpus.

# The type language

Generated code describes IDL types with **descriptors**, which are plain Python
data so that a generated module is readable and needs nothing at import time:

    "long"                     a primitive, by its IDL spelling
    ("string", 16)             string<16>; 0 is unbounded
    ("wstring", 0)
    ("seq", <desc>, bound)     sequence<T, bound>
    ("array", <desc>, length)  T[length]
    ("ref", "IDL:m/T:1.0")     a named type, resolved through TYPES
    ("objref", "IDL:m/I:1.0")  an interface reference

References are by repository id rather than by Python name so that a module can
name a type declared after it — IDL recursion is legal and Python has no
forward declaration.
"""

import base64
import json
import math
import os
import subprocess

__all__ = [
    "Error", "MarshalError", "TransportError", "SystemException", "UserException",
    "Struct", "Union", "Enum", "EnumItem", "ObjectRef", "LongDouble",
    "TYPES", "register", "register_alias", "resolve",
    "to_json", "from_json", "call", "Bridge", "Loopback", "connect",
]


# ── errors ──────────────────────────────────────────────────────────────────

class Error(Exception):
    """Base of everything this runtime raises."""


class MarshalError(Error):
    """A value did not match the IDL type it was being sent or read as.

    Carries the member path (``order.lines[2].qty``) rather than only a
    message, because a caller who is guessing gets nothing from "marshalling
    failed" — §3.3 counts diagnostics as a product.
    """

    def __init__(self, path, message):
        self.path = path
        self.message = message
        super().__init__("%s: %s" % (path, message) if path else message)


class TransportError(Error):
    """The bridge, or the connection under it, could not complete the call."""


class SystemException(Error):
    """A CORBA system exception the target's ORB raised.

    ``completed`` is the field a retry loop reads, and it is **the ordinal the
    peer sent**, passed through without interpretation: 0 is COMPLETED_YES,
    1 is COMPLETED_NO, 2 is COMPLETED_MAYBE (§4.11.4). This project has had
    those first two transposed once already, in an enum whose comment is now
    longer than the enum, so the number crosses as a number rather than
    becoming a second name to get wrong.
    """

    def __init__(self, id, minor=0, completed=2):
        self.id = id
        self.minor = minor
        self.completed = completed
        super().__init__("%s (minor=%d, completed=%s)" % (id, minor, completed))


class UserException(Error):
    """Base of every generated IDL exception.

    A generated subclass sets ``_idl_id`` and ``_idl_members`` and takes its
    members as constructor arguments, in declaration order.

    ``_idl_members`` is ``((wire_name, attribute_name, descriptor), ...)``. The
    first two differ exactly when the IDL name is a Python keyword: the OMG
    mapping escapes ``lambda`` to ``_lambda``, and the wire still says
    ``lambda``.
    """

    _idl_id = ""
    _idl_members = ()

    def __repr__(self):
        return _repr_members(self)

    def __eq__(self, other):
        return _eq_members(self, other)


# ── value types ─────────────────────────────────────────────────────────────

def _repr_members(self):
    inner = ", ".join(
        "%s=%r" % (attr, getattr(self, attr, None))
        for _, attr, _ in self._idl_members
    )
    return "%s(%s)" % (type(self).__name__, inner)


def _eq_members(self, other):
    if type(self) is not type(other):
        return NotImplemented
    return all(
        getattr(self, attr, None) == getattr(other, attr, None)
        for _, attr, _ in self._idl_members
    )


class Struct(object):
    """Base of every generated IDL struct.

    The generated subclass writes its own ``__init__`` with the members named
    and ordered as the IDL declares them, so the signature is the contract and
    an editor can complete it. Declaration order is wire order — the §5.3
    differ measured that swapping two same-sized members is a silent breaking
    change — so the order in the signature is load-bearing, not cosmetic.
    """

    _idl_id = ""
    _idl_members = ()

    def __repr__(self):
        return _repr_members(self)

    def __eq__(self, other):
        return _eq_members(self, other)


class EnumItem(object):
    """One enumerator: a named object, never an ordinal.

    §4.5 crosses enumerators **by name** and §5.3 measured what happens when
    meaning is attached to the ordinal, so the ordinal is kept for the wire's
    sake and is not what equality or printing use.

    ``_enum`` is the enum's **repository id**, not its class. An id identifies
    the type without needing it to exist yet, which is what lets a constant in
    one module name an enumerator declared in another, and it makes two
    enumerators of the same contract equal across two imports of it — which
    class identity would not.
    """

    __slots__ = ("_name", "_ord", "_enum")

    def __init__(self, name, ordinal, enum):
        self._name = name
        self._ord = ordinal
        self._enum = enum

    def __repr__(self):
        return self._name

    def __eq__(self, other):
        return (isinstance(other, EnumItem) and other._name == self._name
                and other._enum == self._enum)

    def __hash__(self):
        return hash((self._enum, self._name))


class Enum(object):
    """Base of every generated IDL enum.

    The OMG Python mapping puts the *enumerators* in the enclosing scope and
    the enum *type* alongside them, so ``m.RED`` is the value and ``m.Colour``
    is the type. Generated code does exactly that; this class exists so the
    runtime can find the members of a type it is handed.
    """

    _idl_id = ""
    _idl_members = ()

    @classmethod
    def _item(cls, name):
        items = getattr(cls, "_items", {})
        if name not in items:
            raise MarshalError("", "%r is not an enumerator of %s; it has %s"
                               % (name, cls.__name__, ", ".join(cls._idl_members)))
        return items[name]


class Union(object):
    """Base of every generated IDL union.

    A union carries its discriminator explicitly (``_d``) and its value
    (``_v``), which is the OMG mapping and also §4.5's rule: the active branch
    is a fact about the value, never something to infer from which member
    happens to be set.

    A discriminator matching no case is legal IDL. It is represented here by
    ``_v is None``, and it marshals as a document with a ``_d`` and no ``_v``.

    # Why the case labels are stored in their AnyJSON form

    A ``case`` label of an enum-discriminated union names an enumerator, and an
    enumerator is an object that exists only once its module has been executed.
    Storing labels as Python expressions would therefore make a class body
    depend on definition order — which IDL does not promise across modules and
    Python cannot defer. Stored as the scalar §4.5 already defines for that
    discriminator (a number, a boolean, or an enumerator's name), a label needs
    nothing to exist yet, and the conversion each way is the mapping's own.
    """

    _idl_id = ""
    _idl_disc = "long"
    #: ``((labels, member_name, descriptor), ...)`` with labels in AnyJSON
    #: form; the ``default:`` branch has no labels.
    _idl_cases = ()
    #: Index into ``_idl_cases`` of the ``default:`` branch, or -1.
    _idl_default = -1

    def __init__(self, d, v=None):
        self._d = d
        self._v = v

    def __repr__(self):
        return "%s(_d=%r, _v=%r)" % (type(self).__name__, self._d, self._v)

    def __eq__(self, other):
        if type(self) is not type(other):
            return NotImplemented
        return self._d == other._d and self._v == other._v

    @classmethod
    def _case_at(cls, label):
        """The branch an AnyJSON discriminator selects, or None for no branch."""
        for i, case in enumerate(cls._idl_cases):
            if i == cls._idl_default:
                continue
            if label in case[0]:
                return case
        if cls._idl_default >= 0:
            return cls._idl_cases[cls._idl_default]
        return None

    @classmethod
    def _case_for(cls, d):
        """The branch a Python discriminator value selects."""
        return cls._case_at(to_json(cls._idl_disc, d, "_d"))

    def _branch(self, member):
        case = self._case_for(self._d)
        if case is None or case[1] != member:
            raise MarshalError("", "the active branch is %r, not %r"
                               % (case[1] if case else None, member))
        return self._v

    def _set_branch(self, member, value):
        for labels, name, _ in self._idl_cases:
            if name == member:
                if labels:
                    self._d = from_json(self._idl_disc, labels[0], "_d")
                self._v = value
                return
        raise MarshalError("", "%s has no branch %r" % (type(self).__name__, member))


class ObjectRef(object):
    """An object reference, as a **handle** — never as an IOR.

    §4.7: an IOR is a bearer address, so anything holding one can dial the
    target directly, bypassing authorisation, approval and the audit log. §4.5
    is therefore physically incapable of emitting one, and this is what a
    Python client gets instead: a name the bridge that issued it can turn back
    into an address, and nobody else can.

    The consequence is worth stating rather than hiding: a handle is **not** a
    proxy. It can be passed back as an argument through the bridge that issued
    it; it cannot be dialled, stored across bridge lifetimes, or narrowed to a
    stub. See the run record for why that is a v1 boundary and not a defect.
    """

    __slots__ = ("handle", "type_id")

    def __init__(self, handle, type_id=""):
        self.handle = handle
        self.type_id = type_id

    def __repr__(self):
        return "ObjectRef(%r, %r)" % (self.handle, self.type_id)

    def __eq__(self, other):
        return isinstance(other, ObjectRef) and other.handle == self.handle

    def __hash__(self):
        return hash(self.handle)

    @staticmethod
    def nil():
        """The nil reference: a truthful "there is no such object"."""
        return None


class LongDouble(object):
    """``long double``: 16 raw octets, with no portable Python equivalent.

    Held as bytes rather than converted to ``float``, because converting is
    lossy in a way nothing downstream could detect.
    """

    __slots__ = ("octets",)

    def __init__(self, octets):
        if len(octets) != 16:
            raise MarshalError("", "a long double is 16 octets")
        self.octets = bytes(octets)

    def __repr__(self):
        return "LongDouble(%r)" % (self.octets,)

    def __eq__(self, other):
        return isinstance(other, LongDouble) and other.octets == self.octets


# ── the type table ──────────────────────────────────────────────────────────

#: Repository id -> generated class, or -> descriptor for a typedef.
TYPES = {}


def register(cls):
    """Records a generated class under its repository id. Usable as a decorator."""
    TYPES[cls._idl_id] = cls
    return cls


def register_alias(id, desc):
    """Records a typedef: an id that resolves to another descriptor."""
    TYPES[id] = desc


def resolve(desc):
    """Follows ``("ref", id)`` and typedef chains to a class or a descriptor.

    Aliases are transparent to §4.5 exactly as they are to CDR — a ``typedef
    sequence<octet> Payload`` still crosses as base64 — so resolution happens
    once, here, rather than at every use.
    """
    seen = 0
    while isinstance(desc, tuple) and desc[0] == "ref":
        if desc[1] not in TYPES:
            raise MarshalError("", "no type is registered under %r" % (desc[1],))
        desc = TYPES[desc[1]]
        seen += 1
        if seen > 64:
            raise MarshalError("", "typedef chain does not terminate")
    return desc


# ── AnyJSON v1 (docs/PLAN.md §4.5) ──────────────────────────────────────────

_INT_RANGE = {
    "octet": (0, 255),
    "char": (0, 255),
    "short": (-32768, 32767),
    "ushort": (0, 65535),
    "long": (-2147483648, 2147483647),
    "ulong": (0, 4294967295),
    "longlong": (-(2 ** 63), 2 ** 63 - 1),
    "ulonglong": (0, 2 ** 64 - 1),
}

#: The two 64-bit types cross as JSON **strings**. A JSON number is a double in
#: every mainstream implementation, so anything past 2^53 loses digits silently.
_WIDE = ("longlong", "ulonglong")

#: The name an ``any`` carries in ``_t``. These spellings are the mapping's,
#: not Python's, and must match the Rust side exactly.
_ANY_NAME = {
    "boolean": "boolean", "octet": "octet", "char": "char", "wchar": "wchar",
    "short": "short", "ushort": "unsigned short", "long": "long",
    "ulong": "unsigned long", "longlong": "long long",
    "ulonglong": "unsigned long long", "float": "float", "double": "double",
    "longdouble": "long double", "string": "string", "wstring": "wstring",
}
_ANY_DESC = dict((v, k) for k, v in _ANY_NAME.items())


def _member(path, name):
    return name if not path else "%s.%s" % (path, name)


def _index(path, i):
    return "%s[%d]" % (path, i)


def _int(v, kind, path):
    if isinstance(v, bool) or not isinstance(v, int):
        raise MarshalError(path, "expected an %s, got %r" % (kind, v))
    lo, hi = _INT_RANGE[kind]
    if not lo <= v <= hi:
        raise MarshalError(path, "%d is outside %s" % (v, kind))
    return v


def _float_out(x, path):
    if not isinstance(x, (int, float)) or isinstance(x, bool):
        raise MarshalError(path, "expected a float, got %r" % (x,))
    x = float(x)
    if math.isnan(x):
        return {"_f": "nan"}
    if math.isinf(x):
        return {"_f": "+inf" if x > 0 else "-inf"}
    return x


def _float_in(j, path):
    if isinstance(j, dict) and "_f" in j:
        tag = j["_f"]
        if tag == "nan":
            return float("nan")
        if tag == "+inf":
            return float("inf")
        if tag == "-inf":
            return float("-inf")
        raise MarshalError(path, "%r is not nan, +inf or -inf" % (tag,))
    if isinstance(j, bool) or not isinstance(j, (int, float)):
        raise MarshalError(path, "expected a number, got %r" % (j,))
    return float(j)


def _one_char(v, path, what):
    if not isinstance(v, str) or len(v) != 1:
        raise MarshalError(path, "a %s is a string of exactly one character" % what)
    return v


def to_json(desc, value, path=""):
    """Renders a Python value as its AnyJSON form, or says exactly what is wrong."""
    d = resolve(desc)

    if isinstance(d, str):
        if d == "boolean":
            if not isinstance(value, bool):
                raise MarshalError(path, "expected a boolean, got %r" % (value,))
            return value
        if d == "char":
            # A char is one octet of the negotiated codeset, not a Unicode
            # scalar, so it crosses as a number. Python holds it as a
            # one-character string because that is the OMG language mapping —
            # the two layers disagree on purpose and this is the seam.
            return ord(_one_char(value, path, "char"))
        if d == "wchar":
            return _one_char(value, path, "wchar")
        if d in _WIDE:
            return str(_int(value, d, path))
        if d in _INT_RANGE:
            return _int(value, d, path)
        if d in ("float", "double"):
            return _float_out(value, path)
        if d == "longdouble":
            if not isinstance(value, LongDouble):
                raise MarshalError(path, "expected a LongDouble, got %r" % (value,))
            return base64.b64encode(value.octets).decode("ascii")
        if d == "any":
            return _any_out(value, path)
        if d == "void":
            return None
        raise MarshalError(path, "no AnyJSON form for %r" % (d,))

    if isinstance(d, tuple):
        kind = d[0]
        if kind in ("string", "wstring"):
            if not isinstance(value, str):
                raise MarshalError(path, "expected a %s, got %r" % (kind, value))
            return value
        if kind == "objref":
            if value is None:
                return {"_ref": None}
            if not isinstance(value, ObjectRef):
                raise MarshalError(path, "expected an ObjectRef or None, got %r" % (value,))
            return {"_ref": value.handle, "_type": value.type_id or d[1]}
        if kind in ("seq", "array"):
            # base64 is the **sequence<octet>** rule and not the array rule:
            # §4.5 gives it to a sequence because a megabyte of binary must not
            # become a million JSON numbers, and an IDL array has a length in
            # its type, so `octet[16]` crosses as an array of numbers. The two
            # were one branch here until the batch measured the difference.
            elem = resolve(d[1])
            if kind == "seq" and elem == "octet":
                if not isinstance(value, (bytes, bytearray)):
                    raise MarshalError(path, "a sequence<octet> is bytes, got %r" % (value,))
                return base64.b64encode(bytes(value)).decode("ascii")
            if isinstance(value, (str, bytes, bytearray)) or not hasattr(value, "__iter__"):
                raise MarshalError(path, "expected a list, got %r" % (value,))
            items = list(value)
            if kind == "array" and len(items) != d[2]:
                raise MarshalError(path, "this array has %d elements, %d given" % (d[2], len(items)))
            return [to_json(d[1], x, _index(path, i)) for i, x in enumerate(items)]
        raise MarshalError(path, "no AnyJSON form for %r" % (kind,))

    if isinstance(d, type) and issubclass(d, Enum):
        if not isinstance(value, EnumItem) or value._enum != d._idl_id:
            raise MarshalError(path, "expected an enumerator of %s, got %r" % (d.__name__, value))
        return value._name

    if isinstance(d, type) and issubclass(d, (Struct, UserException)):
        if not isinstance(value, d):
            raise MarshalError(path, "expected a %s, got %r" % (d.__name__, value))
        out = {}
        for name, attr, mdesc in d._idl_members:
            out[name] = to_json(mdesc, getattr(value, attr), _member(path, name))
        return out

    if isinstance(d, type) and issubclass(d, Union):
        if not isinstance(value, d):
            raise MarshalError(path, "expected a %s, got %r" % (d.__name__, value))
        out = {"_d": to_json(d._idl_disc, value._d, _member(path, "_d"))}
        case = d._case_for(value._d)
        if value._v is not None:
            if case is None:
                raise MarshalError(path, "a union with a value but no selected branch")
            out["_v"] = to_json(case[2], value._v, _member(path, "_v"))
        return out

    raise MarshalError(path, "no AnyJSON form for %r" % (d,))


def _any_out(value, path):
    """``any`` carries its own type: ``{"_t": <name>, "_v": <value>}``."""
    if not isinstance(value, tuple) or len(value) != 2:
        raise MarshalError(path, "an any is the pair (descriptor, value)")
    inner, v = value
    name = _ANY_NAME.get(inner if isinstance(inner, str) else
                         (inner[0] if isinstance(inner, tuple) else None))
    if name is None:
        raise MarshalError(
            path,
            "only primitive types may cross in an any until the registry is consulted; "
            "%r is not one" % (inner,))
    return {"_t": name, "_v": to_json(inner, v, _member(path, "_v"))}


def from_json(desc, j, path=""):
    """Reads an AnyJSON document as the Python value ``desc`` describes."""
    d = resolve(desc)

    if isinstance(d, str):
        if d == "boolean":
            if not isinstance(j, bool):
                raise MarshalError(path, "expected a boolean, got %r" % (j,))
            return j
        if d == "char":
            return chr(_int(j, "char", path))
        if d == "wchar":
            return _one_char(j, path, "wchar")
        if d in _WIDE:
            # A string is what the mapping emits; a number is accepted because
            # a peer that has not read the spec will send one, and it is safe
            # exactly when it survives the trip.
            if isinstance(j, str):
                try:
                    j = int(j)
                except ValueError:
                    raise MarshalError(path, "%r is not an integer" % (j,))
            return _int(j, d, path)
        if d in _INT_RANGE:
            return _int(j, d, path)
        if d in ("float", "double"):
            return _float_in(j, path)
        if d == "longdouble":
            return LongDouble(_unbase64(j, path))
        if d == "any":
            return _any_in(j, path)
        if d == "void":
            return None
        raise MarshalError(path, "no AnyJSON form for %r" % (d,))

    if isinstance(d, tuple):
        kind = d[0]
        if kind in ("string", "wstring"):
            if not isinstance(j, str):
                raise MarshalError(path, "expected a %s, got %r" % (kind, j))
            return j
        if kind == "objref":
            if not isinstance(j, dict) or "_ref" not in j:
                raise MarshalError(path, "an object reference is {\"_ref\": ...}")
            if j["_ref"] is None:
                return None
            return ObjectRef(j["_ref"], j.get("_type", d[1]))
        if kind in ("seq", "array"):
            elem = resolve(d[1])
            if kind == "seq" and elem == "octet":
                return _unbase64(j, path)
            if not isinstance(j, list):
                raise MarshalError(path, "expected an array, got %r" % (j,))
            if kind == "array" and len(j) != d[2]:
                raise MarshalError(path, "this array has %d elements, %d given" % (d[2], len(j)))
            return [from_json(d[1], x, _index(path, i)) for i, x in enumerate(j)]
        raise MarshalError(path, "no AnyJSON form for %r" % (kind,))

    if isinstance(d, type) and issubclass(d, Enum):
        if not isinstance(j, str):
            raise MarshalError(path, "an enumerator of %s is named, not numbered" % d.__name__)
        return d._item(j)

    if isinstance(d, type) and issubclass(d, (Struct, UserException)):
        if not isinstance(j, dict):
            raise MarshalError(path, "expected an object, got %r" % (j,))
        extra = [k for k in j if k not in [n for n, _, _ in d._idl_members]]
        if extra:
            # Not ignored: an unknown member is either a typo or a peer built
            # against a different contract, and both are worth knowing.
            raise MarshalError(path, "%s has no member(s) %s" % (d.__name__, ", ".join(sorted(extra))))
        args = []
        for name, _attr, mdesc in d._idl_members:
            if name not in j:
                raise MarshalError(path, "%s needs a member %r" % (d.__name__, name))
            args.append(from_json(mdesc, j[name], _member(path, name)))
        return d(*args)

    if isinstance(d, type) and issubclass(d, Union):
        if not isinstance(j, dict) or "_d" not in j:
            raise MarshalError(path, "a %s needs an explicit discriminator in \"_d\"" % d.__name__)
        disc = from_json(d._idl_disc, j["_d"], _member(path, "_d"))
        case = d._case_at(j["_d"])
        if case is None:
            if "_v" in j:
                raise MarshalError(path, "the selected branch of %s has no member" % d.__name__)
            return d(disc, None)
        if "_v" not in j:
            raise MarshalError(path, "branch %r of %s needs a \"_v\"" % (case[1], d.__name__))
        return d(disc, from_json(case[2], j["_v"], _member(path, "_v")))

    raise MarshalError(path, "no AnyJSON form for %r" % (d,))


def _any_in(j, path):
    if not isinstance(j, dict) or "_t" not in j or "_v" not in j:
        raise MarshalError(path, "an any is {\"_t\": <type>, \"_v\": <value>}")
    inner = _ANY_DESC.get(j["_t"])
    if inner is None:
        raise MarshalError(
            path,
            "unknown type %r; only primitives may cross in an any until the "
            "registry is consulted" % (j["_t"],))
    if inner in ("string", "wstring"):
        inner = (inner, 0)
    return (inner, from_json(inner, j["_v"], _member(path, "_v")))


def _unbase64(j, path):
    if not isinstance(j, str):
        raise MarshalError(path, "expected a base64 string, got %r" % (j,))
    if len(j) % 4 != 0:
        raise MarshalError(path, "base64 length must be a multiple of 4")
    try:
        return base64.b64decode(j.encode("ascii"), validate=True)
    except Exception:
        raise MarshalError(path, "%r is not base64" % (j,))


# ── invocation ──────────────────────────────────────────────────────────────

def call(invoker, id, operation, args=(), returns="void", outs=(), raises=(), oneway=False):
    """One operation, from the arguments a caller passed to the value it gets back.

    Everything a generated stub does goes through here: it renders the
    arguments, hands the request to the invoker, and reads the reply. The
    generated method contributes names, order and descriptors — the facts of
    one contract — and no conversion logic at all.

    The reply shape follows §7.9.1 and the OMG Python mapping: the declared
    result first when it is not ``void``, then the ``out`` and ``inout``
    values in declaration order, as a tuple when there is more than one.
    """
    body = {}
    for name, desc, value in args:
        body[name] = to_json(desc, value, name)
    request = {"id": id, "op": operation, "args": body}
    if oneway:
        request["oneway"] = True
    reply = invoker.invoke(request)

    if "error" in reply:
        raise TransportError(reply["error"].get("message", "the bridge reported a failure"))
    if "system_exception" in reply:
        s = reply["system_exception"]
        raise SystemException(s.get("id", ""), s.get("minor", 0), s.get("completed", 2))
    if "user_exception" in reply:
        u = reply["user_exception"]
        cls = TYPES.get(u.get("id"))
        if cls is None or not (isinstance(cls, type) and issubclass(cls, UserException)):
            # An id we cannot decode still names a contract the caller was not
            # built against, which is the useful half of the message.
            raise SystemException("IDL:omg.org/CORBA/UNKNOWN:1.0", 0x4f4d0001, "YES")
        raise from_json(cls, u.get("members") or {}, "")
    if "ok" not in reply:
        raise TransportError("the bridge answered with neither a result nor a failure")

    ok = reply["ok"]
    values = []
    if returns != "void":
        values.append(from_json(returns, ok.get("returns"), "<return>"))
    for name, desc in outs:
        if name not in ok.get("outputs", {}):
            raise TransportError("the reply is missing the out parameter %r" % (name,))
        values.append(from_json(desc, ok["outputs"][name], name))
    if not values:
        return None
    if len(values) == 1:
        return values[0]
    return tuple(values)


class Loopback(object):
    """An invoker that answers from a script instead of from a peer.

    Present so that generated code can be **executed** by a test with no ORB,
    no fixture and no network: the requests it records are the AnyJSON a real
    call would have sent, which is exactly what the cross-implementation
    oracle compares against the Rust mapping.
    """

    def __init__(self, replies=None):
        self.requests = []
        self.replies = list(replies or [])

    def invoke(self, request):
        self.requests.append(request)
        if self.replies:
            return self.replies.pop(0)
        return {"ok": {"returns": None, "outputs": {}}}


class Bridge(object):
    """An invoker backed by the ``orbweaver-py-bridge`` process.

    The bridge is where the wire is. It is started with the contract and the
    target's IOR, it holds one connection, and it speaks one JSON document per
    line in each direction. Python never sees an IOR, a GIOP header or a byte
    of CDR.

    Use it as a context manager, or call :meth:`close`; the child process is
    the only resource here and leaking one is how a test suite ends up with
    forty orphaned peers.
    """

    def __init__(self, idl, ior, command=None, cwd=None):
        command = command or os.environ.get("ORBWEAVER_PY_BRIDGE", "orbweaver-py-bridge")
        argv = command if isinstance(command, list) else [command]
        argv = argv + ["--idl", str(idl), "--ior", str(ior)]
        self._proc = subprocess.Popen(
            argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, cwd=cwd,
            text=True, bufsize=1)
        hello = self._proc.stdout.readline()
        if not hello.strip():
            raise TransportError("the bridge did not start: %s" % (self._stderr(),))
        banner = json.loads(hello)
        if "ready" not in banner:
            raise TransportError("the bridge refused to start: %s" % (hello.strip(),))
        self.ready = banner["ready"]

    def _stderr(self):
        try:
            self._proc.kill()
        except Exception:
            pass
        return "exit status %r" % (self._proc.poll(),)

    def invoke(self, request):
        if self._proc.poll() is not None:
            raise TransportError("the bridge process has exited (%r)" % (self._proc.returncode,))
        self._proc.stdin.write(json.dumps(request) + "\n")
        self._proc.stdin.flush()
        line = self._proc.stdout.readline()
        if not line:
            raise TransportError("the bridge closed its output; %s" % (self._stderr(),))
        return json.loads(line)

    def close(self):
        if self._proc.poll() is None:
            try:
                self._proc.stdin.close()
            except Exception:
                pass
            try:
                self._proc.wait(timeout=5)
            except Exception:
                self._proc.kill()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()
        return False


def connect(idl, ior, command=None, cwd=None):
    """A :class:`Bridge` over ``ior``, speaking the contract in ``idl``."""
    return Bridge(idl, ior, command=command, cwd=cwd)
