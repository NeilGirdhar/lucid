4. Execution model
==================

4.1. Structure of a program
---------------------------

Lucid modules contain ordinary definitions: functions, classes, interfaces, traits, and imports.

Definitions are private by default. A definition becomes part of the public module API when it is marked with ``export``.

.. code-block:: python

   export def parse_user(raw: str) -> User:
       ...

   export class User:
       name: str
       email: str

Only exported names are included in ``import *``.

4.2. Naming and binding
-----------------------

Names bind through ordinary definitions, imports, assignments, and parameters. Lucid does not use a separate ``__all__`` list for public API control.

Lucid does not have ``global`` or ``nonlocal`` declarations. Reading bindings
from an enclosing lexical scope is allowed, but assigning to a name always binds
or updates the name in the current local scope. An inner function cannot rebind
a name from an outer function or module scope.

Shared state is represented explicitly. Code that needs mutable module state,
closure state, counters, caches, or configuration stores that state in an
object and mutates the object's members or items:

.. code-block:: python

   counter = Cell(0)

   def next_id() -> int:
       counter.value += 1
       return counter.value

State owned by a domain object should usually be modeled as fields:

.. code-block:: python

   class IdSource:
       next: int = 0

       def take(self) -> int:
           value = self.next
           self.next += 1
           return value

The same rule handles closure state without a rebinding declaration:

.. code-block:: python

   def make_counter() -> Callable[[], int]:
       count = Cell(0)

       def next() -> int:
           count.value += 1
           return count.value

       return next

4.3. Construction
-----------------

Calling a class constructs an object. If no custom factory is present, Lucid generates the obvious field-based constructor.

.. code-block:: python

   class Point:
       x: float
       y: float

acts like:

.. code-block:: python

   factory __init__(cls, x: float, y: float):
       return construct(x, y)

``construct`` is a keyword, not an ordinary function. It can only appear inside a factory. At runtime, a ``construct`` expression creates an instance of the exact class whose factory is running, assigns the supplied values to that class's declared fields in field order, and returns the fully initialized object.

Custom construction is written with a factory:

.. code-block:: python

   class User:
       name: str
       email: str

       factory __init__(cls, name: str, raw_email: str):
           return construct(name, raw_email.lower())

``__init__`` is not a mutating post-allocation hook. It is a factory that returns the object.

Alternative constructors are factories too. Factories are not inherited. A factory constructs the exact class where it is defined.

Every class also has a generated ``replace`` factory. It works like Python's
``__replace__`` protocol: given an existing instance and any changed field
values, it constructs a new instance of the same exact class with unchanged
fields copied from the original object.

.. code-block:: python

   class Point:
       x: float
       y: float

   p = Point(1.0, 2.0)
   q = Point.replace(p, y=3.0)

For ``Point``, the generated factory has this behavior:

.. code-block:: python

   factory replace(cls, original: Point, **changes):
       x = changes.get("x", original.x)
       y = changes.get("y", original.y)
       return construct(x, y)

``replace`` is a factory, so it is not inherited. Each class has its own
``replace`` factory for its own declared fields and exact construction rules.

4.4. Runtime components
-----------------------

Lucid's runtime model is intentionally smaller than Python's. Object shape, class creation, type relationships, attribute access, and cleanup are not programmable through hidden object-model hooks.
