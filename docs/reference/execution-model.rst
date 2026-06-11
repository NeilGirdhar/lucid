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

4.4. Runtime components
-----------------------

Lucid's runtime model is intentionally smaller than Python's. Object shape, class creation, type relationships, attribute access, and cleanup are not programmable through hidden object-model hooks.
