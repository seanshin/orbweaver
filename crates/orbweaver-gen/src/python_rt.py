"""The runtime generated Python marshals through — AnyJSON v1.1, and nothing else.

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

# The type language and the wire's own (AnyJSON v1.1, D008)

An ``any`` carries its type in ``_t``: a name for a primitive, and for
everything else the **structural form** — ``{"kind": "struct", "id": ..,
"name": .., "members": [..]}`` — the same document ``::CORBA::TypeCode``
crosses as when it is a value. Descriptors and forms describe the same types
in two spellings, one for a generated module to be readable in and one for a
document to be self-contained in, and this file converts each way:
``_desc_of`` reads a form as a descriptor (the reverse of the generator's
``descriptor``) and ``_form_of`` writes a descriptor as a form. A form naming a
type this package never declared is **synthesised** — a class built from the
document and registered under its id — because the point of a type that
describes itself is that the reader needs no prior copy of it.

That includes the two constructs §4.4 defers. A ``valuetype`` (``tk_value``)
and an abstract interface (``tk_abstract_interface``) are read as descriptions
and written back unchanged; a **value** of either is refused, by name, with the
section quoted. The asymmetry is the specification, not a gap being papered
over: the description is a TypeCode and a TypeCode is a value the v1 wire
carries, while the state behind it goes inline behind a value tag that this
wire has no encoding for. Refusing the description too would have made a peer's
``any`` unreadable for carrying a type we merely cannot instantiate.
"""

import base64
import builtins
import json
import keyword
import math
import os
import subprocess

__all__ = [
    "Error", "MarshalError", "TransportError", "SystemException", "UserException",
    "Struct", "Union", "Enum", "EnumItem", "ObjectRef", "LongDouble", "TypeCode",
    "ValueType",
    "TYPES", "NAMES", "register", "register_alias", "register_name", "resolve",
    "to_json", "from_json", "call", "Bridge", "Loopback", "connect", "property",
    # The serving direction. `Servant` and `Op` are what a generated servant
    # class is built from, `Raise`/`Raising` are how one refuses, and
    # `dispatch_call` is the whole of it as a pure function.
    "Servant", "ServantError", "Op", "Raise", "Raising", "OMG_VMCID",
    "dispatch_call", "Host", "serve",
]

#: The builtin ``property``, reachable through this module.
#:
#: A generated union writes one ``@_rt.property`` per branch, and not
#: ``@property``, because a union's class body is a scope the *contract* writes
#: into: a branch named ``property`` binds the name, and the next branch's
#: decorator then calls a property object (``TypeError: 'property' object is
#: not callable``). An item named ``property`` in the enclosing module does the
#: same from one scope out. ``_rt`` is the one name a generated module holds
#: that no IDL identifier can spell — a leading underscore is IDL's escape
#: character rather than an identifier character — so reaching the builtin
#: through it is not shadowable by any contract.
property = builtins.property


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


#: The one sentence **all three** of §4.4's deferrals are refused with — one
#: format string, so the ``valuetype``, ``abstract interface`` and ``fixed``
#: refusals cannot drift apart, and so all three read as the Rust mapping's do
#: (`orbweaver_dynamic::decode`, verbatim). The asymmetry is the message: the
#: description crosses, the value does not, and a reader that met only "no
#: AnyJSON form for <class>" would read that as a hole in this runtime rather
#: than as the wire boundary it is.
#:
#: The slot takes the whole subject — ``"valuetype Money (IDL:m/Money:1.0)"``
#: from ``_subject``, ``"fixed<9,2>"`` bare — which is what `orbweaver_dynamic`'s
#: ``deferred_wire_name`` produces, because `fixed` has no kind word or id in
#: front of it and a multi-slot format could only spell it with holes. ``fixed``
#: wrote its own sentence here until 2026-08-21 for exactly that reason.
_DEFERRED = ("%s is not marshalled by the v1 wire (docs/PLAN.md §4.4); the TypeCode "
             "describing it reads, the value behind it does not")

#: The fourth family, and **not** a fourth caller of the string above.
#:
#: A ``native`` is not deferred: §4.4's three have a wire form the
#: specification defines and this version has not implemented, and a native has
#: none to implement in any version, because it names a type only a language
#: mapping knows. So the tail says the opposite of ``_DEFERRED``'s — there is
#: nothing to wait for and nothing to keep sending — and the word "yet" must
#: never appear in it. See `orbweaver_dynamic::unmarshallable_wire_sentence`,
#: which this is held equal to by `orbweaver-gen`'s ``python_target``:
#: Python cannot import a Rust constant, so the equality is a test.
_UNMARSHALLABLE = ("%s has no wire form at all: it names a type only a language mapping knows, "
                   "and no version of the wire marshals one; this is not one of docs/PLAN.md "
                   "§4.4's deferrals — those have a wire form this version has not implemented, "
                   "and there is none here to implement")

#: The fifth family, and **not** a third caller of either string above.
#:
#: ``::CORBA::Principal`` was neither deferred nor never-marshallable: GIOP 1.0
#: carried one in every request header and CORBA 3.0 removed the type. So the
#: head names the withdrawal, and the tail keeps D008's asymmetry (the
#: description crosses — ``_desc_of`` reads ``{"kind": "principal"}`` — and the
#: value does not) while denying §4.4 out loud, because a reader who met the
#: section in this runtime's other refusals will search for it here.
#:
#: Equal to `orbweaver_dynamic::withdrawn_wire_sentence` by ``python_target``'s
#: comparison, for the reason the two above are: Python cannot import a Rust
#: constant, so the equality is a test.
_WITHDRAWN = ("%s was withdrawn from CORBA: GIOP 1.0 carried one in every request header, "
              "GIOP 1.1 dropped that field and CORBA 3.0 removed the type — so this version "
              "marshals no value for one, and no later version will; the TypeCode describing "
              "it reads, the value behind it does not. This is not one of docs/PLAN.md §4.4's "
              "deferrals: those wait on this project, and a type the specification has removed "
              "waits on nobody")


def _subject(kind, name, id):
    """How a construct is spelled as the subject of a refusal: kind word,
    simple name, repository id in parentheses — or the id alone when the name
    is empty, which a peer-built TypeCode may be.

    The one Python home of ``orbweaver_dynamic``'s ``construct_subject``
    spelling, held equal across the crate boundary by ``python_target``'s
    comparison. The id is in the subject because a simple name is ambiguous:
    two modules declaring ``Describable`` produced one string (2026-08-25).
    """
    return "%s %s (%s)" % (kind, name, id) if name else "%s %s" % (kind, id)


#: The subject ``_WITHDRAWN`` is always filled with. A ``("principal",)``
#: descriptor carries no id — ``tk_Principal`` is a primitive kind and has none
#: on the wire — so unlike ``native`` there is nothing to look up in ``NAMES``,
#: and the spelling is fixed. Equal to `orbweaver_dynamic::principal_subject`.
_PRINCIPAL = _subject("predeclared type", "::CORBA::Principal",
                      "IDL:omg.org/CORBA/Principal:1.0")


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

    ``stated`` records whether the completion status was *given* rather than
    defaulted, and it exists for the serving side. A Rust servant cannot reach
    a `SystemException` without naming the status — `rt::Raising` has no
    ``Default`` and no ``From``, and its ``#[must_use]`` makes a forgotten one
    a warning — because a generator-chosen COMPLETED_NO on a raise that fired
    halfway through a mutation is how a well-behaved retry loop corrupts state.
    Python has no type system to enforce that, so :func:`dispatch_call` refuses
    an unstated one instead, and this flag is how it can tell. Reading a peer's
    reply always states it, so a client is unaffected.
    """

    def __init__(self, id, minor=0, completed=None):
        self.id = id
        self.minor = minor
        self.stated = completed is not None
        self.completed = 2 if completed is None else completed
        super().__init__("%s (minor=%d, completed=%s)" % (id, minor, self.completed))


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


class TypeCode(object):
    """A ``::CORBA::TypeCode`` as a value — AnyJSON v1.1's structural form.

    Held as the document, exactly as it arrived, so that relaying one is exact
    even where this runtime has nothing to say about a kind (``fixed``, say).
    ``tc.kind`` answers "what is this", ``tc.form`` is the structure underneath,
    and :meth:`descriptor` is the same type in the language generated code
    speaks — which is what lets an ``any`` be read against a TypeCode a peer
    described rather than against a class this package happened to declare.
    :meth:`of` goes the other way, for a TypeCode a caller wants to send.

    None of this decides a CDR question: a descriptor is a spelling of a type,
    and the bytes are still the bridge's business (D007).
    """

    __slots__ = ("form",)

    def __init__(self, form, path=""):
        if not isinstance(form, (str, dict)):
            raise MarshalError(path, "a TypeCode is a name or a type object, got %r" % (form,))
        self.form = form

    @classmethod
    def of(cls, desc):
        """The TypeCode of a descriptor: ``TypeCode.of(("ref", "IDL:m/T:1.0"))``."""
        return cls(_form_of(desc, ""))

    def descriptor(self):
        """This type as a descriptor, synthesising any type not declared here."""
        return _desc_of(self.form, "")

    @property
    def kind(self):
        """``"long"``, ``"struct"``, ``"seq"`` — the shape, in one word."""
        return self.form if isinstance(self.form, str) else self.form.get("kind")

    @property
    def id(self):
        """The repository id, for the kinds that have one."""
        return None if isinstance(self.form, str) else self.form.get("id")

    def __eq__(self, other):
        return isinstance(other, TypeCode) and other.form == self.form

    def __ne__(self, other):
        return not self.__eq__(other)

    def __hash__(self):
        return hash(repr(self.form))

    def __repr__(self):
        return "TypeCode(%r)" % (self.kind,)


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
    #: form. A bare ``default:`` branch has no labels; a branch that is both
    #: labelled and ``default:`` (``case 2: default: string rest;``) keeps its
    #: labels, and is the default as well.
    _idl_cases = ()
    #: Index into ``_idl_cases`` of the ``default:`` branch, or -1.
    _idl_default = -1
    #: Where ``default:`` was written among the default branch's labels: the
    #: number of labels that precede it (0 for a bare ``default:`` and for
    #: ``default: case 5: case 6:``, 1 for ``case 2: default:``). The TypeCode
    #: lists one member per label with the default as a member of its own, in
    #: source order — omniidl's list and the registry's — so ``_class_form``
    #: needs the slot to put the default member back where ``default_index``
    #: will find it.
    _idl_default_slot = 0

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
        for case in cls._idl_cases:
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


class ValueType(object):
    """The **description** of an IDL ``valuetype`` — and deliberately nothing else.

    §4.4 defers the value, not its description. A peer's ``any`` can carry the
    TypeCode of a ``valuetype`` (``tk_value``, 29) and this runtime reads it,
    synthesises this class from it and can write the same TypeCode back; what
    it will not do is marshal an *instance*, because the v1 wire has no
    encoding for one. So there is no constructor, no member attribute and no
    ``to_json`` path — asking for a value of this type raises, by name.

    It is pointedly **not** a :class:`Struct`. A valuetype's state does travel
    member by member, so a struct base would have marshalled something
    plausible and wrong, which is the same silent-wrong-answer shape the
    deferral was hiding in until 2026-08-20 (a valuetype was recorded as an
    object reference, and an IOR went out where a peer sends a value).

    ``_idl_members`` is ``(name, descriptor, visibility)`` per member — no
    Python attribute name, because nothing is ever constructed to hold one.
    """

    #: The repository id, the IDL name, the `ValueModifier`, the concrete base
    #: (a descriptor, or ``None`` for ``tk_null``) and the members.
    _idl_id = ""
    _idl_name = ""
    _idl_modifier = 0
    _idl_base = None
    _idl_members = ()

    def __init__(self, *args, **kw):
        raise MarshalError("", _DEFERRED % _subject("valuetype", self._idl_name, self._idl_id))


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

#: Repository id -> the IDL name a TypeCode carries beside it.
#:
#: A descriptor names a type by id alone, and that is enough to marshal a
#: value; it is not enough to *describe* one, because the TypeCode a peer
#: receives inside an ``any`` carries the short name too and a peer that
#: compares TypeCodes compares names. So the name is a fact the generated
#: module states once, here, and never derives from the id: ``#pragma ID``
#: exists precisely to make the two disagree.
#:
#: Seeded with the two references the IDL language itself types and no
#: contract ever declares — the same two the registry spells out for
#: ``Object`` and ``ValueBase``. Found by the sweep, not by reading: a struct
#: with an ``Object`` member came back with an unnamed TypeCode.
NAMES = {
    "IDL:omg.org/CORBA/Object:1.0": "Object",
    "IDL:omg.org/CORBA/ValueBase:1.0": "ValueBase",
}


def register(cls):
    """Records a generated class under its repository id. Usable as a decorator."""
    TYPES[cls._idl_id] = cls
    NAMES[cls._idl_id] = getattr(cls, "_idl_name", cls.__name__)
    return cls


def register_alias(id, desc, name=""):
    """Records a typedef: an id that resolves to another descriptor."""
    TYPES[id] = desc
    if name:
        NAMES[id] = name


def register_name(id, name):
    """Records the name of a type that has an id and no body here — an interface
    declared and never defined, whose descriptor is a bare ``("objref", id)``."""
    NAMES[id] = name


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

#: The name a type whose whole identity fits in one carries in ``_t``, keyed by
#: its descriptor. These spellings are the mapping's, not Python's, and must
#: match the Rust side's ``short_name`` exactly — every name that one writes,
#: this table must read, or the mapping writes a document it cannot read back
#: (the defect D008 was drafted from). ``string`` and ``wstring`` are the
#: unbounded case only; a bound is a fact the name would lose, so a bounded
#: string crosses in the structural form like a constructed type.
_ANY_NAME = {
    "boolean": "boolean", "octet": "octet", "char": "char", "wchar": "wchar",
    "short": "short", "ushort": "unsigned short", "long": "long",
    "ulong": "unsigned long", "longlong": "long long",
    "ulonglong": "unsigned long long", "float": "float", "double": "double",
    "longdouble": "long double", "string": "string", "wstring": "wstring",
    "any": "any", "typecode": "typecode", "void": "void", "null": "null",
}
_ANY_DESC = dict((v, k) for k, v in _ANY_NAME.items())

#: The kinds whose structural form has a repository id and a body, and so map
#: to a ``("ref", id)`` descriptor: a class, or a typedef's descriptor.
#:
#: ``value`` is here because a valuetype's *description* is read like any
#: other — §4.4 defers the value, not the TypeCode — and the class it
#: synthesises to, :class:`ValueType`, is the one that refuses to marshal.
_NAMED_KINDS = ("struct", "except", "enum", "union", "alias", "value")


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
        if d == "typecode":
            if not isinstance(value, TypeCode):
                raise MarshalError(path, "expected a TypeCode, got %r" % (value,))
            return value.form
        if d in ("void", "null"):
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
        if kind == "abstract_interface":
            raise MarshalError(path, _DEFERRED % _subject("abstract interface", NAMES.get(d[1], ""), d[1]))
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
        if kind == "fixed":
            raise MarshalError(path, _DEFERRED % ("fixed<%d,%d>" % (d[1], d[2])))
        if kind == "native":
            raise MarshalError(path, _UNMARSHALLABLE % _subject("native", NAMES.get(d[1], ""), d[1]))
        if kind == "principal":
            raise MarshalError(path, _WITHDRAWN % _PRINCIPAL)
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

    if isinstance(d, type) and issubclass(d, ValueType):
        raise MarshalError(path, _DEFERRED % _subject("valuetype", d._idl_name, d._idl_id))

    raise MarshalError(path, "no AnyJSON form for %r" % (d,))


def _any_out(value, path):
    """``any`` carries its own type: ``{"_t": <type>, "_v": <value>}``.

    The pair is ``(descriptor, value)``, or ``(TypeCode, value)`` for a caller
    relaying a type a peer described — in which case the document that arrived
    is the document that leaves, unrebuilt, so nothing this runtime does not
    understand about it can be lost on the way through.
    """
    if not isinstance(value, tuple) or len(value) != 2:
        raise MarshalError(path, "an any is the pair (descriptor, value)")
    inner, v = value
    if isinstance(inner, TypeCode):
        form, inner = inner.form, inner.descriptor()
    else:
        form = _form_of(inner, _member(path, "_t"))
    return {"_t": form, "_v": to_json(inner, v, _member(path, "_v"))}


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
        if d == "typecode":
            return TypeCode(j, path)
        if d in ("void", "null"):
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
        if kind == "abstract_interface":
            raise MarshalError(path, _DEFERRED % _subject("abstract interface", NAMES.get(d[1], ""), d[1]))
        if kind in ("seq", "array"):
            elem = resolve(d[1])
            if kind == "seq" and elem == "octet":
                return _unbase64(j, path)
            if not isinstance(j, list):
                raise MarshalError(path, "expected an array, got %r" % (j,))
            if kind == "array" and len(j) != d[2]:
                raise MarshalError(path, "this array has %d elements, %d given" % (d[2], len(j)))
            return [from_json(d[1], x, _index(path, i)) for i, x in enumerate(j)]
        if kind == "fixed":
            # The direction that matters: the document was a peer's, and the
            # reader has to be told the `_t` half was understood and `_v` is
            # where v1 stops — `_DEFERRED` is that sentence, equal to the Rust
            # layers' by `python_target`'s comparison.
            raise MarshalError(path, _DEFERRED % ("fixed<%d,%d>" % (d[1], d[2])))
        if kind == "native":
            raise MarshalError(path, _UNMARSHALLABLE % _subject("native", NAMES.get(d[1], ""), d[1]))
        # The fifth family, on the direction that matters: the document was a
        # peer's. Until 2026-08-26 this fell to the line below and answered
        # "no AnyJSON form for 'principal'" — a hole in this runtime, which is
        # not what happened. The type was removed from CORBA; the reader has to
        # learn that the `_t` half was understood and that no release restores
        # the `_v` half.
        if kind == "principal":
            raise MarshalError(path, _WITHDRAWN % _PRINCIPAL)
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

    if isinstance(d, type) and issubclass(d, ValueType):
        raise MarshalError(path, _DEFERRED % _subject("valuetype", d._idl_name, d._idl_id))

    raise MarshalError(path, "no AnyJSON form for %r" % (d,))


def _any_in(j, path):
    if not isinstance(j, dict) or "_t" not in j or "_v" not in j:
        raise MarshalError(path, "an any is {\"_t\": <type>, \"_v\": <value>}")
    inner = _desc_of(j["_t"], _member(path, "_t"))
    return (inner, from_json(inner, j["_v"], _member(path, "_v")))


# ── descriptors <-> the structural TypeCode form (AnyJSON v1.1, D008) ────────

def _py_attr(name):
    """The attribute a synthesised member is held under: the OMG mapping's
    leading-underscore escape, the same rule the generator applies."""
    return "_" + name if keyword.iskeyword(name) or name in ("self", "cls") else name


def _form_field(form, key, path, types):
    if key not in form:
        raise MarshalError(path, "a %r type object needs a %r field" % (form.get("kind"), key))
    v = form[key]
    if not isinstance(v, types) or isinstance(v, bool):
        raise MarshalError(path, "%r in a %r type object is %r, not %s"
                           % (key, form.get("kind"), v, types))
    return v


def _desc_of(form, path):
    """A structural type form as a descriptor — the reverse of the generator's
    ``descriptor``, so ``("ref", id)`` for anything with an id and a body.

    A type this package does not declare is synthesised from the form and
    registered, so it resolves the same way a generated one does. A type it
    does declare is used as declared: the id is the contract, and a peer whose
    document disagrees with it about the members is met at the member, with a
    path, by the ordinary struct check.
    """
    if isinstance(form, str):
        d = _ANY_DESC.get(form)
        if d is None:
            raise MarshalError(path, "%r is not a type name AnyJSON knows" % (form,))
        return (d, 0) if d in ("string", "wstring") else d
    if not isinstance(form, dict) or not isinstance(form.get("kind"), str):
        raise MarshalError(path, "a type is a name or an object with a \"kind\", got %r" % (form,))
    kind = form["kind"]
    if kind in ("string", "wstring"):
        return (kind, _form_field(form, "bound", path, int))
    if kind == "seq":
        return ("seq", _desc_of(_form_field(form, "element", path, (str, dict)),
                                _member(path, "element")),
                _form_field(form, "bound", path, int))
    if kind == "array":
        return ("array", _desc_of(_form_field(form, "element", path, (str, dict)),
                                  _member(path, "element")),
                _form_field(form, "length", path, int))
    if kind == "objref":
        id = _form_field(form, "id", path, str)
        NAMES.setdefault(id, _form_field(form, "name", path, str))
        return ("objref", id)
    if kind == "abstract_interface":
        # An id and a name and nothing else, exactly like `objref` — which is
        # why it needs no class and no synthesis. What it is *not* is an
        # `objref`: on the wire an abstract interface is the union of a value
        # and a reference, so a descriptor that spelled it as a reference
        # would marshal an IOR where a peer may send a value. It is its own
        # descriptor so that the refusal below can name it.
        id = _form_field(form, "id", path, str)
        NAMES.setdefault(id, _form_field(form, "name", path, str))
        return ("abstract_interface", id)
    if kind == "recursive":
        # Resolved when the value is marshalled, like every other reference:
        # by then the type it re-enters has been registered, because the form
        # that carried the marker is the form that declared the type.
        return ("ref", _form_field(form, "id", path, str))
    if kind in _NAMED_KINDS:
        id = _form_field(form, "id", path, str)
        if id not in TYPES:
            _synthesise(kind, id, form, path)
        return ("ref", id)
    if kind == "fixed":
        # §4.4 defers the *value*, not the TypeCode (D008): the form reads to
        # a descriptor like every other kind's, and the value legs refuse with
        # `_DEFERRED`. This arm raised that refusal itself until 2026-08-24 —
        # so a peer whose document *described* a fixed was told their `_t`
        # half was not understood, which is the opposite of what the Rust
        # side answers for the same document (it reads the description and
        # stops at `_v`).
        return ("fixed", _form_field(form, "digits", path, int),
                _form_field(form, "scale", path, int))
    if kind == "native":
        # The fourth family, same shape: the description reads — it is a value
        # the wire carries — and the value legs refuse with `_UNMARSHALLABLE`,
        # which names the construct and its boundary.
        id = _form_field(form, "id", path, str)
        NAMES.setdefault(id, _form_field(form, "name", path, str))
        return ("native", id)
    if kind == "principal":
        # Withdrawn from CORBA, and still a kind `tc_to_json` writes: the
        # description crosses and the value legs refuse with ``_WITHDRAWN`` —
        # the same division of labour the Rust side applies to it. No id is
        # read here because ``tk_Principal`` carries none; the subject is
        # ``_PRINCIPAL``, fixed.
        return ("principal",)
    raise MarshalError(path, "no AnyJSON value form for a %r type" % (kind,))


def _init_members(self, *args, **kw):
    """The constructor of a synthesised struct or exception: members in
    declaration order, by position or by name, all of them required."""
    attrs = [a for _, a, _ in self._idl_members]
    if len(args) > len(attrs):
        raise TypeError("%s takes %d member(s), %d given"
                        % (type(self).__name__, len(attrs), len(args)))
    for a, v in zip(attrs, args):
        setattr(self, a, v)
    for k, v in kw.items():
        if k not in attrs:
            raise TypeError("%s has no member %r" % (type(self).__name__, k))
        setattr(self, k, v)
    missing = [a for a in attrs if not hasattr(self, a)]
    if missing:
        raise TypeError("%s needs %s" % (type(self).__name__, ", ".join(missing)))


def _synthesise(kind, id, form, path):
    """Builds and registers the class (or typedef) a structural form describes.

    Registered **before** its members are read for a struct, union or
    exception, so that a member which re-enters the type — directly, or through
    a ``recursive`` marker naming it — finds it there.
    """
    name = _form_field(form, "name", path, str)
    if kind in ("struct", "except"):
        base = Struct if kind == "struct" else UserException
        cls = type(name, (base,), {
            "_idl_id": id, "_idl_name": name, "_idl_members": (),
            "__init__": _init_members,
            "__doc__": "IDL %s `%s`, synthesised from the type an any described." % (kind, id),
        })
        register(cls)
        members = []
        for i, m in enumerate(_form_field(form, "members", path, list)):
            at = _index(_member(path, "members"), i)
            if not isinstance(m, dict):
                raise MarshalError(at, "a member is {\"name\": .., \"type\": ..}")
            mname = _form_field(m, "name", at, str)
            members.append((mname, _py_attr(mname),
                            _desc_of(_form_field(m, "type", at, (str, dict)), at)))
        cls._idl_members = tuple(members)
        return
    if kind == "enum":
        members = _form_field(form, "members", path, list)
        if not all(isinstance(m, str) for m in members):
            raise MarshalError(path, "an enum's members are names")
        cls = type(name, (Enum,), {
            "_idl_id": id, "_idl_name": name, "_idl_members": tuple(members),
            "__doc__": "IDL enum `%s`, synthesised from the type an any described." % (id,),
        })
        cls._items = {}
        for i, m in enumerate(members):
            item = EnumItem(m, i, id)
            cls._items[m] = item
            # The enumerators live on the class, since there is no module for
            # the OMG mapping to put them in.
            setattr(cls, _py_attr(m), item)
        register(cls)
        return
    if kind == "union":
        cls = type(name, (Union,), {
            "_idl_id": id, "_idl_name": name, "_idl_disc": "long",
            "_idl_cases": (), "_idl_default": -1, "_idl_default_slot": 0,
            "__doc__": "IDL union `%s`, synthesised from the type an any described." % (id,),
        })
        register(cls)
        cls._idl_disc = _desc_of(_form_field(form, "discriminator", path, (str, dict)),
                                 _member(path, "discriminator"))
        default = _form_field(form, "default", path, int)
        # The wire's cases are one per label, the default a case of its own
        # with the registry's empty label (`case 2: default: string rest;` is
        # `(2, rest)` then the default `rest`, as omniidl lists it); a class
        # holds one branch per member with its labels together, which is what
        # the generator writes, and remembers where among them the default sat
        # (`_idl_default_slot`) so `_class_form` can put it back there. A
        # default case that arrives with a label of its own — the folded shape
        # this runtime read until 2026-08-19 — keeps the label as a label.
        branches = []
        slot = 0
        for i, c in enumerate(_form_field(form, "cases", path, list)):
            at = _index(_member(path, "cases"), i)
            if not isinstance(c, dict):
                raise MarshalError(at, "a case is {\"label\": .., \"name\": .., \"type\": ..}")
            cname = _form_field(c, "name", at, str)
            ctype = _desc_of(_form_field(c, "type", at, (str, dict)), at)
            label = c.get("label")
            labels = [] if label == {"_raw": ""} else [label]
            if branches and branches[-1][1] == cname:
                if i == default and not branches[-1][3]:
                    slot = len(branches[-1][0])
                    branches[-1] = branches[-1][:3] + (True,)
                branches[-1][0].extend(labels)
            else:
                if i == default:
                    slot = 0
                branches.append((labels, cname, ctype, i == default))
        cls._idl_cases = tuple((tuple(ls), n, t) for ls, n, t, _ in branches)
        cls._idl_default = next((i for i, b in enumerate(branches) if b[3]), -1)
        cls._idl_default_slot = slot
        return
    if kind == "alias":
        register_alias(id, _desc_of(_form_field(form, "aliased", path, (str, dict)),
                                    _member(path, "aliased")), name)
        return
    if kind == "value":
        # Registered before its base and members are read, for the same reason
        # a struct is: `valuetype Node { public Node next; };` describes itself
        # through a `recursive` marker naming an id that must already be there.
        cls = type(name, (ValueType,), {
            "_idl_id": id, "_idl_name": name, "_idl_modifier": 0,
            "_idl_base": None, "_idl_members": (),
            "__doc__": "IDL valuetype `%s`, synthesised from the type an any described. "
                       "Its TypeCode crosses; no value of it does (§4.4)." % (id,),
        })
        register(cls)
        cls._idl_modifier = _form_field(form, "modifier", path, int)
        # `base` is required and may be JSON null: null is this document's
        # spelling for the `tk_null` the wire puts in that slot when there is
        # no concrete base, and absent is a malformed form rather than none.
        if "base" not in form:
            raise MarshalError(path, "a 'value' type object needs a 'base' field")
        base = form["base"]
        cls._idl_base = (None if base is None
                         else _desc_of(base, _member(path, "base")))
        members = []
        for i, m in enumerate(_form_field(form, "members", path, list)):
            at = _index(_member(path, "members"), i)
            if not isinstance(m, dict):
                raise MarshalError(
                    at, "a value member is {\"name\": .., \"type\": .., \"visibility\": ..}")
            members.append((_form_field(m, "name", at, str),
                            _desc_of(_form_field(m, "type", at, (str, dict)), at),
                            _form_field(m, "visibility", at, int)))
        cls._idl_members = tuple(members)
        return
    raise MarshalError(path, "no synthesis for a %r type" % (kind,))


def _form_of(desc, path, visiting=()):
    """A descriptor as the structural form the wire carries — what the
    generator's ``descriptor`` would have been written from.

    ``visiting`` is the chain of ids being described; a reference back into it
    is a ``recursive`` marker, which is exactly where the registry puts one:
    at the point a type re-enters something still being defined.
    """
    if isinstance(desc, str):
        if desc in _ANY_NAME:
            return _ANY_NAME[desc]
        raise MarshalError(path, "no TypeCode form for %r" % (desc,))
    if isinstance(desc, tuple):
        kind = desc[0]
        if kind in ("string", "wstring"):
            return kind if desc[1] == 0 else {"kind": kind, "bound": desc[1]}
        if kind == "seq":
            return {"kind": "seq", "element": _form_of(desc[1], path, visiting),
                    "bound": desc[2]}
        if kind == "array":
            return {"kind": "array", "element": _form_of(desc[1], path, visiting),
                    "length": desc[2]}
        if kind == "objref":
            return {"kind": "objref", "id": desc[1], "name": NAMES.get(desc[1], "")}
        if kind == "abstract_interface":
            return {"kind": "abstract_interface", "id": desc[1],
                    "name": NAMES.get(desc[1], "")}
        if kind == "fixed":
            return {"kind": "fixed", "digits": desc[1], "scale": desc[2]}
        if kind == "native":
            return {"kind": "native", "id": desc[1], "name": NAMES.get(desc[1], "")}
        if kind == "principal":
            return {"kind": "principal"}
        if kind == "ref":
            id = desc[1]
            if id in visiting:
                return {"kind": "recursive", "id": id}
            if id not in TYPES:
                raise MarshalError(path, "no type is registered under %r" % (id,))
            target = TYPES[id]
            if isinstance(target, type):
                return _class_form(target, path, visiting + (id,))
            return {"kind": "alias", "id": id, "name": NAMES.get(id, ""),
                    "aliased": _form_of(target, path, visiting + (id,))}
        raise MarshalError(path, "no TypeCode form for %r" % (kind,))
    if isinstance(desc, type):
        return _class_form(desc, path, visiting + (getattr(desc, "_idl_id", ""),))
    raise MarshalError(path, "no TypeCode form for %r" % (desc,))


def _class_form(cls, path, visiting):
    id = cls._idl_id
    named = {"id": id, "name": NAMES.get(id, getattr(cls, "_idl_name", cls.__name__))}
    if issubclass(cls, (Struct, UserException)):
        named["kind"] = "struct" if issubclass(cls, Struct) else "except"
        named["members"] = [{"name": n, "type": _form_of(d, _member(path, n), visiting)}
                            for n, _, d in cls._idl_members]
        return named
    if issubclass(cls, Enum):
        named["kind"] = "enum"
        named["members"] = list(cls._idl_members)
        return named
    if issubclass(cls, Union):
        named["kind"] = "union"
        named["discriminator"] = _form_of(cls._idl_disc, _member(path, "_d"), visiting)
        cases = []
        default = -1
        for i, (labels, member, d) in enumerate(cls._idl_cases):
            t = _form_of(d, _member(path, member), visiting)
            for k, label in enumerate(labels):
                if i == cls._idl_default and k == cls._idl_default_slot:
                    # The default is a case of its own among the branch's
                    # labels, where `default:` was written. It has no label:
                    # the registry gives it none, and none is what base64 of
                    # nothing spells.
                    default = len(cases)
                    cases.append({"label": {"_raw": ""}, "name": member, "type": t})
                cases.append({"label": label, "name": member, "type": t})
            if i == cls._idl_default and cls._idl_default_slot >= len(labels):
                # After the branch's labels, or a bare `default:` with none.
                default = len(cases)
                cases.append({"label": {"_raw": ""}, "name": member, "type": t})
        named["cases"] = cases
        named["default"] = default
        return named
    if issubclass(cls, ValueType):
        # The writing half of the deferral: the description goes back out
        # whole — modifier, concrete base and every member's visibility —
        # because a TypeCode a peer sent must survive being relayed through a
        # reader that cannot marshal one instance of it.
        named["kind"] = "value"
        named["modifier"] = cls._idl_modifier
        named["base"] = (None if cls._idl_base is None
                         else _form_of(cls._idl_base, _member(path, "<base>"), visiting))
        named["members"] = [
            {"name": n, "type": _form_of(d, _member(path, n), visiting), "visibility": vis}
            for n, d, vis in cls._idl_members]
        return named
    raise MarshalError(path, "%s is not a type a value can be described by" % (cls.__name__,))


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
            #
            # ``0`` and not ``"YES"``. This was the one place in the runtime
            # that built a `SystemException` itself, and it was the one place
            # that put a *name* in the field every other path fills with the
            # ordinal the peer sent — so a caller writing the retry test this
            # class's docstring describes, ``exc.completed == 1``, could never
            # match here, and ``exc.completed`` came back as a string only on
            # this one path. §4.11.4 numbers COMPLETED_YES 0, which is what
            # `orbweaver_giop`'s `SystemException::unknown_user_exception`
            # answers with; found 2026-08-26 while building the servant
            # direction, by mirroring this branch on the serving side.
            raise SystemException("IDL:omg.org/CORBA/UNKNOWN:1.0", 0x4f4d0001, 0)
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


# ── serving ─────────────────────────────────────────────────────────────────
#
# The other protocol direction. Everything above this line is a client: it
# renders a request, hands it to an invoker and reads a reply. Everything below
# is a servant: it reads a call, hands it to a Python object and renders the
# answer. The two halves share ``to_json`` and ``from_json`` and share the
# three reply shapes — ``ok``, ``user_exception``, ``system_exception`` —
# because they are the same documents read from the other end. That reuse is
# why a servant was a seam question rather than a language question: the
# mapping did not need a second half, the *protocol* needed a second direction.
#
# What is **not** here is any wire knowledge at all. The bridge decoded the
# CDR, resolved the operation, answered ``_is_a`` and chose the reply status
# before this module saw anything. See ``orbweaver_gen::pyservant``.


#: OMG's own vendor minor code space; ``orbweaver_gen::rt::OMG_VMCID``'s twin.
OMG_VMCID = 0x4f4d0000


class Raising(object):
    """A system exception that has not yet said whether the operation ran.

    The mirror of Rust's ``rt::Raising``, and it exists for the same reason
    that one does: whether a refusal landed before or after the state changed
    is knowledge the **servant** has and no generator does, so there is no
    default that is right. Reach a :class:`SystemException` through
    :meth:`did_not_run`, :meth:`ran_to_completion` or :meth:`may_have_run`.

    Python cannot make a forgotten one a compile error the way ``#[must_use]``
    does, so the check moved to the seam: :func:`dispatch_call` refuses to
    serialise a :class:`SystemException` whose status was never stated.
    """

    def __init__(self, id, minor=0):
        self.id = id
        self._minor = minor

    def minor(self, minor):
        """Attaches a vendor minor code."""
        return Raising(self.id, minor)

    def omg_minor(self, minor):
        """Attaches a minor code in OMG's own space, which is the half of the
        code that makes it portable between ORBs."""
        return Raising(self.id, OMG_VMCID | minor)

    def did_not_run(self):
        """COMPLETED_NO: nothing was touched and re-sending is safe."""
        return SystemException(self.id, self._minor, 1)

    def ran_to_completion(self):
        """COMPLETED_YES: it ran and only the answer is lost."""
        return SystemException(self.id, self._minor, 0)

    def may_have_run(self):
        """COMPLETED_MAYBE: it cannot be determined.

        The right answer whenever a failure lands in the middle of a mutation —
        it is worse for a client to be told "safe to retry" wrongly than to be
        told nobody knows.
        """
        return SystemException(self.id, self._minor, 2)

    def with_completion(self, completed):
        """The same decision made from an ordinal, for a servant whose
        completion status is itself computed."""
        return SystemException(self.id, self._minor, completed)


class Raise(object):
    """The system exceptions a servant raises, as ``rt::raise`` spells them.

    A ``raises`` clause gives an interface its *user* exceptions; this is the
    other half, the vocabulary every servant needs and no contract declares.
    Each answers a :class:`Raising`, which becomes a :class:`SystemException`
    only once the completion status is stated::

        raise _rt.Raise.no_permission().did_not_run()
        raise _rt.Raise.bad_param().omg_minor(3).did_not_run()

    The set is the one the servants in this workspace actually raise, not the
    whole of §4.11; :meth:`other` is the door for the rest of it rather than a
    reason to grow the list to thirty constructors.
    """

    @staticmethod
    def object_not_exist():
        """The object key names nothing this servant holds."""
        return Raising("IDL:omg.org/CORBA/OBJECT_NOT_EXIST:1.0")

    @staticmethod
    def no_permission():
        """The caller may not make this call, and retrying will not change it."""
        return Raising("IDL:omg.org/CORBA/NO_PERMISSION:1.0")

    @staticmethod
    def bad_param():
        """An argument is outside what the operation accepts. Usually worth a
        minor code, since the caller cannot see which argument otherwise."""
        return Raising("IDL:omg.org/CORBA/BAD_PARAM:1.0")

    @staticmethod
    def transient():
        """Refused now; a retry may well succeed."""
        return Raising("IDL:omg.org/CORBA/TRANSIENT:1.0")

    @staticmethod
    def bad_inv_order():
        """Legal call, wrong state."""
        return Raising("IDL:omg.org/CORBA/BAD_INV_ORDER:1.0")

    @staticmethod
    def no_implement():
        """In the contract, not in this servant.

        Distinct from BAD_OPERATION, and the difference is visible on the wire:
        BAD_OPERATION says *no such operation*, which an oversight and a
        decision both used to say. This one says the operation exists and this
        servant does not implement it, on purpose.
        """
        return Raising("IDL:omg.org/CORBA/NO_IMPLEMENT:1.0")

    @staticmethod
    def internal():
        """The servant broke and the caller can do nothing about it."""
        return Raising("IDL:omg.org/CORBA/INTERNAL:1.0")

    @staticmethod
    def other(id):
        """Any other standard system exception, by repository id."""
        return Raising(id)


class Op(object):
    """One operation's shape, as a generated servant class records it.

    The same facts a generated client method passes to :func:`call` — names,
    order, descriptors — read from the other end. A generated servant class
    contributes exactly this and no conversion logic, which is the rule the
    whole target is built on.
    """

    __slots__ = ("method", "ins", "returns", "outs", "raises", "oneway")

    def __init__(self, method, ins=(), returns="void", outs=(), raises=(), oneway=False):
        self.method = method
        self.ins = tuple(ins)
        self.returns = returns
        self.outs = tuple(outs)
        self.raises = tuple(raises)
        self.oneway = oneway


class Servant(object):
    """Base of every generated servant class.

    A generated subclass sets ``_idl_id``, ``_idl_name`` and
    ``_idl_operations`` — a map from the name that travels to an :class:`Op` —
    and the application subclasses *that* and writes the method bodies.

    Inherited members are flattened into ``_idl_operations`` rather than
    expressed as Python inheritance, exactly as a client stub flattens them,
    because one function computes both sets: a servant answers precisely the
    names a client of the same contract can send.

    **This class does not answer ``_is_a``.** The bridge answers it from the
    registry's resolved inheritance chain and it never reaches Python, because
    a servant that answered it differently would be an object that could not be
    narrowed through a base-typed reference — a caller being able to tell what
    language it is talking to.
    """

    _idl_id = ""
    _idl_name = ""
    _idl_operations = {}


class ServantError(Error):
    """A servant answered in a way the seam cannot carry.

    Not a refusal — a refusal is a well-formed answer. This is the servant
    getting the *shape* wrong: a completion status never stated, an out
    parameter never returned, a tuple of the wrong length. Raised in the
    servant's own process, where the traceback is, rather than reaching a
    caller as an opaque failure with nothing to act on.
    """


def dispatch_call(servant, call):
    """One call document to one reply document, with no process in sight.

    The whole of the servant direction's Python half, as a pure function of a
    servant object and a JSON-shaped dict. That is what lets a test execute
    every branch below — every refusal, every conversion — with no bridge, no
    socket and no peer, which is the argument :class:`Loopback` makes for the
    client half.
    """
    op_name = call.get("op")
    op = servant._idl_operations.get(op_name)
    if op is None:
        # The bridge resolves the operation against the registry before it
        # writes a call, so reaching here means the servant class and the
        # contract the bridge loaded disagree. That is a wiring mistake inside
        # one process and it is raised there rather than mapped onto the wire.
        raise ServantError(
            "%s was asked for %r, which its contract does not declare"
            % (type(servant).__name__, op_name))

    args = call.get("args") or {}
    values = []
    for name, desc in op.ins:
        if name not in args:
            raise ServantError("the call for %r is missing the argument %r" % (op_name, name))
        values.append(from_json(desc, args[name], name))

    try:
        answer = getattr(servant, op.method)(*values)
    except UserException as ex:
        id = getattr(ex, "_idl_id", "")
        if id not in op.raises:
            # §4.11's mapping for an exception the caller's contract cannot
            # name. A Rust servant cannot reach this state at all — its
            # generated error enum has no variant for an undeclared raise — so
            # this is one of the differences a Python servant keeps, and the
            # answer is the one the specification already fixes.
            return {"system_exception": {
                "id": "IDL:omg.org/CORBA/UNKNOWN:1.0",
                "minor": OMG_VMCID | 1,
                "completed": 0,
            }}
        return {"user_exception": {"id": id, "members": to_json(type(ex), ex, "")}}
    except SystemException as ex:
        if not ex.stated:
            raise ServantError(
                "%s raised %s without saying whether the operation ran; reach one "
                "through _rt.Raise — .did_not_run(), .ran_to_completion() or "
                ".may_have_run() — so a caller's retry logic has an answer"
                % (type(servant).__name__, ex.id))
        return {"system_exception": {
            "id": ex.id, "minor": ex.minor, "completed": ex.completed}}

    if op.oneway:
        # §9.4.1 gives a oneway no reply to travel in. An answer is rendered
        # anyway and the bridge drops it — visibly, through the same
        # ``oneway_fault_dropped`` a generated Rust skeleton calls — because a
        # server whose oneway operations fail invisibly is one nobody can debug.
        return {"ok": {"returns": None, "outputs": {}}}

    # The declared result first when it is not void, then the out and inout
    # values in declaration order (§7.9.1) — the same tuple shape a Python
    # *client* receives from :func:`call`, so a servant returns what a client
    # reads. One rule, read from both ends.
    wanted = (0 if op.returns == "void" else 1) + len(op.outs)
    if wanted <= 1:
        parts = [answer] if wanted else []
    else:
        if not isinstance(answer, tuple) or len(answer) != wanted:
            raise ServantError(
                "%s.%s must answer a tuple of %d — the result then the out and inout "
                "values in declaration order — and answered %r"
                % (type(servant).__name__, op.method, wanted, answer))
        parts = list(answer)

    at = 0
    returns = None
    if op.returns != "void":
        returns = to_json(op.returns, parts[at], "<return>")
        at += 1
    outputs = {}
    for name, desc in op.outs:
        outputs[name] = to_json(desc, parts[at], name)
        at += 1
    return {"ok": {"returns": returns, "outputs": outputs}}


class Host(object):
    """The ``orbweaver-py-bridge`` process, serving a Python object.

    The mirror of :class:`Bridge`, and the same program in its other mode. In
    the client direction Python writes a request line and the bridge answers;
    here the bridge writes a call line and Python answers. A bridge is in one
    mode or the other for its whole life, never both, so the pipes never carry
    two conversations at once.

    ``ior`` is the reference to hand a caller. Python never sees a byte of CDR
    on this side either.
    """

    def __init__(self, idl, interface, command=None, cwd=None, include=(), endpoint=None):
        command = command or os.environ.get("ORBWEAVER_PY_BRIDGE", "orbweaver-py-bridge")
        argv = command if isinstance(command, list) else [command]
        argv = list(argv) + ["--idl", str(idl), "--serve", str(interface)]
        for d in include:
            argv += ["-I", str(d)]
        if endpoint:
            argv += ["--endpoint", str(endpoint)]
        self._proc = subprocess.Popen(
            argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE, cwd=cwd,
            text=True, bufsize=1)
        # The banner is a synchronisation point, not decoration: a caller told
        # the IOR before the listener existed would dial a closed port, and
        # "the fixture had not started yet" is the phantom failure this project
        # has paid for most often.
        hello = self._proc.stdout.readline()
        if not hello.strip():
            raise TransportError("the bridge did not start: %s" % (self._stderr(),))
        banner = json.loads(hello)
        if "ready" not in banner:
            raise TransportError("the bridge refused to start: %s" % (hello.strip(),))
        self.ready = banner["ready"]
        self.ior = self.ready.get("ior", "")
        self.type_id = self.ready.get("type_id", "")

    def _stderr(self):
        try:
            self._proc.kill()
        except Exception:
            pass
        return "exit status %r" % (self._proc.poll(),)

    def run(self, servant, stop=None):
        """Answers calls until the bridge closes its output, or ``stop()``.

        ``stop`` is consulted after each call rather than between reads,
        because this loop blocks on a line; a servant that wants to stop while
        idle closes the host from another thread.
        """
        while True:
            line = self._proc.stdout.readline()
            if not line:
                return
            if not line.strip():
                continue
            document = json.loads(line)
            call = document.get("call")
            if call is None:
                continue
            try:
                reply = dispatch_call(servant, call)
            except ServantError as e:
                # The seam could not carry the answer. The caller is told the
                # least wrong true thing — UNKNOWN, completion MAYBE, because
                # the servant's method may well have run before the shape
                # failed — and the message stays in this process, where
                # somebody can act on it.
                reply = {"system_exception": {
                    "id": "IDL:omg.org/CORBA/UNKNOWN:1.0", "minor": 0, "completed": 2}}
                self._note(str(e))
            self._proc.stdin.write(json.dumps(reply) + "\n")
            self._proc.stdin.flush()
            if stop is not None and stop():
                return

    def _note(self, message):
        import sys
        sys.stderr.write("orbweaver servant: %s\n" % (message,))
        sys.stderr.flush()

    def close(self):
        """Stops serving.

        Closing stdin is enough for a :class:`Bridge`, which is always either
        answering a request or waiting for one. A :class:`Host` is usually
        waiting to *accept*, where it has no reason to look at stdin at all —
        so an idle one would notice the closed pipe only at the next call,
        which may never come. There is no graceful "stop accepting" document
        because inventing one would be protocol nobody needs: the parent owns
        this child and terminates it.
        """
        if self._proc.poll() is None:
            try:
                self._proc.stdin.close()
            except Exception:
                pass
            self._proc.terminate()
            try:
                self._proc.wait(timeout=5)
            except Exception:
                self._proc.kill()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()
        return False


def serve(idl, interface, command=None, cwd=None, include=(), endpoint=None):
    """A :class:`Host` serving ``interface`` from the contract in ``idl``."""
    return Host(idl, interface, command=command, cwd=cwd, include=include,
                endpoint=endpoint)
