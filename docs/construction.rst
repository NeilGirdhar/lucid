Construction
==============

.. contents:: Table of contents
   :depth: 2
   :local:

Building an instance, introspecting a class's own fields, and filling a
factory field with a value captured from the call site are three
related jobs, all centered on the factory as where they meet — none of
them about what kinds of members a class body can declare, which
`Classes <classes.rst>`_ covers on its own.

Factory construction
------------------------

Python splits construction across ``__new__``, ``__init__``,
dataclass-generated initializers, ``__post_init__``, ``InitVar``, and field
options such as ``init=False`` or ``kw_only``. Lucid has one construction model:
factories return fully constructed objects through the factory-only
``construct`` keyword.

Factories receive ``cls``, but they construct the exact class where they are
defined. Calling a class calls its ``__init__`` factory. Calling a named factory
uses the factory name on the class.

.. code-block:: python

   class Point:
       x: float
       y: float

       factory __init__(cls, x: float, y: float):
           return construct(x, y)

       factory origin(cls):
           return construct(0.0, 0.0)

   p = Point(1.0, 2.0)
   origin = Point.origin()

If ``__init__`` is unspecified, Lucid generates the obvious field-based
constructor. This class:

.. code-block:: python

   class Point:
       x: float
       y: float

gets this default ``__init__`` factory:

.. code-block:: python

   factory __init__(cls, x: float, y: float):
       return construct(x, y)

``construct`` is a keyword, not an ordinary function. It can only appear inside
a factory. At runtime, a ``construct`` expression creates an instance of the
exact class whose factory is running, assigns the supplied values to that
class's declared fields in field order, and returns the fully initialized
object.

Factories are not inherited.

Every class has a generated ``replace`` factory. It works like Python's
``__replace__`` protocol: given an existing instance and any changed field
values, it constructs a new instance of the same exact class with unchanged
fields copied from the original object.

.. code-block:: python

   class Point:
       x: float
       y: float

   p = Point(1.0, 2.0)
   q = Point.replace(p, y=3.0)

Field reflection with ``fields``
-------------------------------------

``replace`` and the default constructor both already have to walk a
class's fields generically. ``fields`` exposes that same walk directly,
the way Python's ``dataclasses.fields`` does, dispatched on whether it is
given an instance or the class itself:

.. code-block:: python

   def dispatch fields[T](obj: T) -> Iterable[(name: str, value: object, doc: str | none, metadata: dict[str, object])]:
       ...

   def dispatch fields[T](cls: type[T]) -> Iterable[(name: str, doc: str | none, metadata: dict[str, object])]:
       ...

Both yield fields in declaration order. The instance form pairs each
field's name with its current value; the class form has no instance to
read a value from, so it yields only names. Both carry ``doc`` and
``metadata`` from `Field docstrings and metadata <classes.rst>`_, ``none``
and ``{:}`` respectively when a field declares neither.

.. code-block:: python

   class Config:
       name: str:
           "the user's display name"

   c = Config("Ada")
   list(fields(c))[0]      # (name="name", value="Ada", doc="the user's display name", metadata={:})
   list(fields(Config))[0]  # (name="name", doc="the user's display name", metadata={:})

Call-site captured values
------------------------------

A factory field can be filled with a reserved value that resolves fresh
at each call site instead of once, at definition time, the way an
ordinary default would. This is not a member-declaration mechanic
either — it belongs here because a factory is where these values are
actually consumed.

Caller-captured source locations
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

A factory field typed ``SourceLocation`` can be filled with ``caller``, a
reserved value meaning "the module and line of this call expression."
Unlike an ordinary default, evaluated once at definition time and reused
for every call, ``caller`` resolves fresh at each call site:

.. code-block:: python

   class Traceback:
       location: SourceLocation

       factory __init__(cls):
           return construct(caller)

   Traceback()   # Traceback at config.lcd:12

This is the same category of mechanism as Rust's ``file!()``/``line!()``
and ``#[track_caller]``: the substitution is fixed and entirely local to
the one call expression it appears in — understanding what it does
requires reading nothing else in the codebase, unlike attribute hooks or
behavior inherited from elsewhere in a class hierarchy. ``caller`` is
always available: every call happens somewhere, so there is always a
module and line to substitute.

Name-captured identifiers
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

A factory field typed ``VarName`` can be filled with ``from_var_name``, a
reserved value meaning "the identifier this call's result is being
assigned to." It resolves the same way ``caller`` does, fresh at each call
site, but it is not always available: a call is only the direct
right-hand side of a simple assignment sometimes, not always — it might
instead be an argument, a return value, or a target of some other shape,
such as a tuple or chained assignment. Those have no single identifier to
substitute, and using ``from_var_name`` there is a compile-time error at
that call site: the checker already knows, from the call's syntax alone,
whether a name exists to capture, the same way it already knows whether
``caller`` fills a ``SourceLocation``-typed field.

.. code-block:: python

   class Sentinel:
       name: VarName

       factory __init__(cls):
           return construct(from_var_name)

       def __repr__(self: &Self) -> str:
           return f"<Sentinel {self.name}>"

   missing = Sentinel()   # <Sentinel missing>
   log(Sentinel())        # error: Sentinel() has no named assignment target

Every ``Sentinel()`` gets the name it was assigned to, with nothing to
write twice or let drift out of sync — the same category of mechanism as
Python's ``__set_name__``, just restricted to exactly the one call
expression it substitutes into instead of a class body.
