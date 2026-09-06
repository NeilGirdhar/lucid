Classes
=======

.. contents:: Table of contents
   :depth: 3
   :local:

Classes define concrete state, construction, and identity. A class can
satisfy interfaces, use traits, and use *class inheritance* to extend at
most one other class, but stored fields and construction belong to the
class.

.. code-block:: python

   class Point:
       x: float
       y: float

       factory __init__(cls, x: float, y: float):
           return construct(x, y)

       getter distance_from_origin(self) -> float:
           return sqrt(self.x ** 2 + self.y ** 2)

Factories construct fully initialized instances of the exact class where the
factory is defined. Construction is not split across allocation, mutation, and
post-initialization hooks.

Classes own data and concrete behavior. Interfaces declare obligations. Traits
provide reusable method bodies. Trait conflicts are resolved explicitly by the
class.

Object shape and attribute access
---------------------------------

Classes have visible state
~~~~~~~~~~~~~~~~~~~~~~~~~~

Classes should make their stored state visible. Lucid takes the transparent
field-first style of dataclasses and makes it the normal object model rather
than a library convention layered over dynamic objects. Stored object state is
declared in the class body, and attribute access is structural and visible in
the class body, not programmable through dynamic lookup hooks.

Python instances usually have an open-ended ``__dict__`` unless a class uses
``__slots__``, dataclasses with options, extension types, or custom attribute
hooks. Lucid makes fixed shape the default: stored fields are declared directly
in the class body.

.. code-block:: python

   class Point:
       x: float
       y: float

No undeclared fields
~~~~~~~~~~~~~~~~~~~~

Assigning an undeclared field is an error. Lucid does not give instances an
implicit ``__dict__`` for arbitrary attributes. If a class needs dynamic keyed
data, declare that storage explicitly. Adding or removing class members after
definition is also not part of Lucid; class shape is closed. Assigning
``obj.__class__`` is not part of Lucid because object shape is fixed.

.. code-block:: python

   p = Point(1.0, 2.0)
   p.z = 3.0  # error: z is not a declared field

.. code-block:: python

   class Record:
       fields: dict[str, object]

       def get(self, name: str) -> object | none:
           return self.fields.get(name)

       def set(self, name: str, value: object):
           self.fields[name] = value

No descriptors
~~~~~~~~~~~~~~

Python descriptors can make attribute access programmable from many places.
Lucid does not include descriptors. Attribute behavior is visible through
fields, methods, getters, setters, class methods, factories, and class member
variables.

No ``property``
~~~~~~~~~~~~~~~

Python's ``property`` is descriptor-based. Lucid uses explicit getter and setter
member syntax instead.

.. code-block:: python

   class Circle:
       radius: float

       getter area(self) -> float:
           return pi * self.radius ** 2

       setter area(self, value: float):
           self.radius = sqrt(value / pi)

No ``__getattr__``
~~~~~~~~~~~~~~~~~~

Lucid does not include ``__getattr__`` fallback lookup. Missing attributes are
errors instead of calls into dynamic lookup code.

No ``__getattribute__``
~~~~~~~~~~~~~~~~~~~~~~~

Lucid does not include ``__getattribute__``. Attribute reads use visible members
from the class body and cannot be globally intercepted.

No ``__setattr__``
~~~~~~~~~~~~~~~~~~

Lucid does not include ``__setattr__``. Attribute assignment targets a declared
field or an explicit setter.

No ``__del__``
~~~~~~~~~~~~~~

Lucid does not include ``__del__`` finalizers. Cleanup should be explicit in the
API that owns the resource, instead of being hidden behind object destruction
timing.

Class members and construction
------------------------------

Closed member kinds
~~~~~~~~~~~~~~~~~~~

Class bodies contain a closed set of member kinds:

.. list-table::
   :header-rows: 1

   * - Member kind
     - Example
   * - instance field
     - ``x: int``
   * - method
     - ``def f(self): ...``
   * - class method
     - ``classmethod f(cls): ...``
   * - factory
     - ``factory f(cls): ...``
   * - getter
     - ``getter x(self) -> T: ...``
   * - setter
     - ``setter x(self, value: T): ...``
   * - class member variable
     - ``classvar count: int = 0``

Read-only methods with ``&Self``
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``Self`` is a builtin type, referring to the enclosing class. A method's
``self`` parameter is ``Self`` by default — the ordinary mutable type — but
a method that only reads its object, never writing to it, should annotate
it ``self: &Self``, the read-only view (see
`Mutable, read-only, and immutable views <mutability.rst>`_):

.. code-block:: python

   class Counter:
       value: int

       def get(self: &Self) -> int:
           return self.value

       def increment(self):
           self.value += 1

This is what lets a frozen object know which of its methods are safe to
call. Since both ``Self`` and ``!Self`` are subtypes of ``&Self``, a method
declared ``self: &Self`` is callable on a mutable, read-only, or frozen
receiver, while a method left at the default ``self: Self`` requires a
mutable receiver and is not available on a read-only or frozen value at
all:

.. code-block:: python

   counter: Counter = Counter(0)
   frozen: !Counter = freeze(counter)

   frozen.get()        # fine: get takes self: &Self
   frozen.increment()  # error: increment takes self: Self, frozen is not mutable

A ``getter`` is read-only by construction — computing a value from an
object should never require write access — so it behaves as though
``self: &Self`` were already implied.

Class member variables
~~~~~~~~~~~~~~~~~~~~~~

Class member variables are marked with ``classvar``. A plain annotated
assignment in the class body declares an instance field; if it has a value, that
value is the field's default — evaluated fresh per instance or shared
across all of them depending on the field's own type (see
`Default values <mutability.rst>`_).

.. code-block:: python

   class User:
       classvar count: int = 0
       name: str
       active: bool = true

This separates shared class state from stored instance fields.

Field docstrings and metadata
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

A field declaration can open an indented block with ``:``, the same suite
grammar every other compound statement already has. The block holds up to
two bare statements, in this order and both optional: a string literal,
the field's docstring, and a dict literal, its metadata:

.. code-block:: python

   class Config:
       name: str:
           "the user's display name"

       retries: int = 3:
           "how many times to retry a failed request"
           {"cli_flag": "--retries"}

No new keyword or builtin call is needed — the block reads the same way a
function body's leading string literal already reads as a docstring in
Python, just made a real, checked part of the field declaration instead
of an unenforced convention. The ``fields()`` builtin reports both
alongside each field's name and value.

Final fields
~~~~~~~~~~~~

``final`` marks a field that can be set once and never reassigned again,
regardless of which view — ``T``, ``&T``, or ``!T`` — the caller holds. It
is a property of the binding, not the type: ``!T`` says the value on the
other end of a reference cannot change; ``final`` says the reference itself
cannot be pointed somewhere else. The two compose independently:

.. code-block:: python

   class Session:
       final id: str
       final model: InferenceModel
       final config: !InferenceModel

``id`` and ``model`` cannot be rebound after construction, but ``model``'s
own fields can still be mutated in place, since ``InferenceModel`` on its own
is an ordinary mutable type. ``config`` cannot be rebound, and the object it
points to cannot change either.

A ``final`` field is normally set once, in a ``factory``. Assigning to it
again anywhere afterward — from any method, through any view — is an error.

No static methods for namespaced functions
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

If a function does not need access to an object or class, it stays a function.
Lucid does not use static methods as namespaced free functions.

Explicit class methods
~~~~~~~~~~~~~~~~~~~~~~

Class methods receive ``cls`` and are inherited as class-level behavior.

.. code-block:: python

   class User:
       classvar count: int = 0

       classmethod total_created(cls) -> int:
           return cls.count

Factory construction
~~~~~~~~~~~~~~~~~~~~

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
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

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
``metadata`` from `Field docstrings and metadata`_, ``none`` and ``{:}``
respectively when a field declares neither.

.. code-block:: python

   class Config:
       name: str:
           "the user's display name"

   c = Config("Ada")
   list(fields(c))[0]      # (name="name", value="Ada", doc="the user's display name", metadata={:})
   list(fields(Config))[0]  # (name="name", doc="the user's display name", metadata={:})

Value semantics options
~~~~~~~~~~~~~~~~~~~~~~~

Classes can request common value semantics with class options. Immutability is
not a class option; it is represented by the ``!T`` view.

.. code-block:: python

   class Point(eq=true, order=true, hash=true):
       x: float
       y: float

Supported core options:

.. list-table::
   :header-rows: 1

   * - Option
     - Meaning
   * - ``eq``
     - equality is generated
   * - ``order``
     - ordering methods are generated
   * - ``hash``
     - hashing behavior is generated for immutable values

Representation is generated by default unless the class defines its own
representation method.

Call-site captured values
------------------------------

A factory field can be filled with a reserved value that resolves fresh
at each call site instead of once, at definition time, the way an
ordinary default would — a capability classes happen to be the vehicle
for, since a factory is where these values are consumed, not something
about member declaration or construction mechanics.

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

Class inheritance and runtime hooks
-----------------------------------

*Class inheritance* stays explicit and does not use Python's metaclass or MRO
customization hooks.

One class parent
~~~~~~~~~~~~~~~~~

*Class inheritance* is limited to one parent: a class may have at most one
class parent. A class header can combine one class parent, any number of
interfaces, and any number of traits.

.. code-block:: python

   class FileLogger(LoggerBase, Closeable, Timestamped):
       path: str

Final classes
~~~~~~~~~~~~~

``final`` applies to a class the same way it applies to a field: a
relationship is fixed permanently, no matter who is asking. On a field, that
relationship is the binding; on a class, it is class inheritance. A
``final`` class may not be used as anyone's class parent:

.. code-block:: python

   final class Point:
       x: float
       y: float

   class Point3D(Point):  # error: Point is final
       z: float

This lets an author close off a class specifically because a subclass could
violate an invariant the implementation depends on, without that decision
touching anything about fields or ordinary variable bindings — the same
``final`` keyword, applied one level up.

Sealed classes
~~~~~~~~~~~~~~~~

``sealed`` sits between the default and ``final`` on the same axis: an
ordinary class may be subclassed from anywhere, a ``final`` class cannot
be subclassed at all, and a ``sealed`` class may be subclassed only by
classes declared in the same file:

.. code-block:: python

   sealed class Shape:
       def area(self) -> float

   class Circle(Shape):
       radius: float
       def area(self) -> float:
           return pi * self.radius ** 2

   class Rectangle(Shape):
       w: float
       h: float
       def area(self) -> float:
           return self.w * self.h

A file elsewhere in the project cannot add a fourth direct subclass of
``Shape`` — verified the same way ``final`` already verifies that no
subclass exists anywhere, just narrower in scope. The restriction applies
only to ``Shape``'s direct subclasses: ``Circle`` and ``Rectangle`` stay
ordinarily subclassable elsewhere unless they are separately marked
``sealed`` or ``final``, since ``sealed`` only has to pin down ``Shape``'s
own variant set, not freeze every subtree beneath it.

A source file is already the unit ``export`` operates on
(`Project configuration <project-configuration.rst>`_), so there is no
separate notion of "module" for this to disagree with, the ambiguity
languages with multi-file modules have to resolve one way or another —
the file sealing restricts to is the same file every other module
boundary in Lucid already uses.

Sealing gives `Exhaustive pattern matching <control-flow.rst>`_ a second
source of closed types, alongside recursive union aliases: a ``match``
over ``Shape`` with a case for every direct subclass needs no ``case _:``,
because the checker can see the complete set the same way it already can
for a union.

No private-name mangling
~~~~~~~~~~~~~~~~~~~~~~~~

Python rewrites a name such as ``self.__a`` to include the defining class's
name. This name mangling helps prevent accidental collisions between members
introduced by different classes, especially in multiple-inheritance
hierarchies.

A class can have at most one class parent. The restriction is not about the
parent being non-abstract — it is that a class, unlike an interface or a
trait, owns stored data, and combining stored data from more than one parent
is exactly what produces the collisions above. Interfaces do not provide
storage, traits do not own stored state, and trait member conflicts must be
resolved explicitly. Because class shape is declared and inherited member
collisions can therefore be reported directly, Lucid does not need automatic
name mangling. Declare and use ``a`` as ``self.a``; ``self.__a`` is not a
special spelling for private state.

No metaclasses
~~~~~~~~~~~~~~

Metaclasses are not part of Lucid. A restricted
``__init_subclass__(cls, **options)`` may remain for validation and registration
only, not for class rewriting.

No ``__prepare__``
~~~~~~~~~~~~~~~~~~

``__prepare__`` is not part of Lucid because metaclass machinery is absent.

No ``__mro_entries__``
~~~~~~~~~~~~~~~~~~~~~~

``__mro_entries__`` is not part of Lucid. Lucid does not allow MRO rewriting.

No programmable type checks
~~~~~~~~~~~~~~~~~~~~~~~~~~~

``__instancecheck__`` and ``__subclasscheck__`` are not part of Lucid. Type
relationships are not programmable through indirect hooks.

