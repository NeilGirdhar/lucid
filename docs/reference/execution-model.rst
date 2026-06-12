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
       return construct(x=x, y=y)

``construct`` is a keyword, not an ordinary function. It can only appear inside
a factory. At runtime, a ``construct`` expression creates an instance of the
exact class whose factory is running, assigns the supplied values to that
class's declared fields, and returns the fully initialized object.
``construct`` can use positional field order or explicit field names, but a
factory must provide exactly one value for each declared field and cannot supply
undeclared fields.

Custom construction is written with a factory:

.. code-block:: python

   class User:
       name: str
       email: str

       factory __init__(cls, name: str, raw_email: str):
           return construct(name=name, email=raw_email.lower())

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
       return construct(x=x, y=y)

``replace`` is a factory, so it is not inherited. Each class has its own
``replace`` factory for its own declared fields and exact construction rules.

4.4. Runtime components
-----------------------

Lucid's runtime model is intentionally smaller than Python's. Object shape, class creation, type relationships, attribute access, and cleanup are not programmable through hidden object-model hooks.
