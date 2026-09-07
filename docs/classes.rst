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
post-initialization hooks — see `Construction <construction.rst>`_ for the
full factory model.

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

No ``del`` on fields
~~~~~~~~~~~~~~~~~~~~

A field is part of its class's fixed shape, not an optional slot present
only when set — deleting one would leave an object whose layout no
longer matches its own class, the same violation `Classes have visible
state`_ already rules out for adding one. ``del obj.field`` is a
compile-time error for every declared field, checked the same way
assigning an undeclared one already is:

.. code-block:: python

   p = Point(1.0, 2.0)
   del p.x  # error: fields are fixed, not deletable

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

Lucid implements the bracketing pattern — acquire, use, release — with
`context managers <context-managers.rst>`_, not RAII-style cleanup tied
to an object's lifetime. Python's ``__del__`` is non-deterministic — the
garbage collector decides when, or whether, to run it — so a file
descriptor, socket, or lock can leak for the rest of the process. Lucid
has no ``__del__``.

A Swift-style ``deinit``, run when an object's last reference drops,
looks like a fix, but "last reference" is only well-defined once the
language tracks reference counts or ownership for every value, and
copying anything that owns a resource has to be restricted so two
owners can't both release it. A context manager gets the same
determinism from a lexical scope instead: cleanup runs at the
``with``-block's boundary, visible at the call site, with none of that
machinery needed.

Class members
-----------------

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

Read-only methods with ``~Self``
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``Self`` is a builtin type, referring to the enclosing class. A method's
``self`` parameter is ``Self`` by default — the ordinary mutable type — but
a method that only reads its object, never writing to it, should annotate
it ``self: ~Self``, the read-only view (see
`Mutable, read-only, and immutable views <mutability.rst>`_):

.. code-block:: python

   class Counter:
       value: int

       def get(self: ~Self) -> int:
           return self.value

       def increment(self):
           self.value += 1

This is what lets a frozen object know which of its methods are safe to
call. Since both ``Self`` and ``!Self`` are subtypes of ``~Self``, a method
declared ``self: ~Self`` is callable on a mutable, read-only, or frozen
receiver, while a method left at the default ``self: Self`` requires a
mutable receiver and is not available on a read-only or frozen value at
all:

.. code-block:: python

   counter: Counter = Counter(0)
   frozen: !Counter = freeze(counter)

   frozen.get()        # fine: get takes self: ~Self
   frozen.increment()  # error: increment takes self: Self, frozen is not mutable

A ``getter`` is read-only by construction — computing a value from an
object should never require write access — so it behaves as though
``self: ~Self`` were already implied.

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
regardless of which view — ``T``, ``~T``, or ``!T`` — the caller holds. It
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

A source file is already the unit module-private names are scoped to
(`Module-private names <modules.rst>`_), so there is no separate notion
of "module" for this to disagree with, the ambiguity languages with
multi-file modules have to resolve one way or another — the file sealing
restricts to is the same file every other module boundary in Lucid
already uses.

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

Private members
~~~~~~~~~~~~~~~~~~

A member named with a leading ``_`` is private to the class: readable
and writable from the class's own methods, and from nowhere else, not
even other files in the same project.

.. code-block:: python

   class Cache:
       _entries: dict[str, float]

       def get(self, key: str) -> float | none:
           return self._entries.get(key)

   cache = Cache({:})
   cache._entries  # error: _entries is private to Cache

Python's single leading underscore is a convention nothing enforces —
``cache._entries`` already works fine, the same as any other attribute.
Lucid checks it: the name alone is the complete, checked declaration,
the same way it already is for a module-private definition at the top
level of a file (see `Module-private names <modules.rst>`__).

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

