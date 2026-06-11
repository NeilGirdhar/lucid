8. Compound statements
======================

8.1. Function definitions
-------------------------

Functions are ordinary callable definitions.

.. code-block:: python

   def normalize_email(raw: str) -> str:
       return raw.strip().lower()

If a function does not need access to an object or class, it stays a function. Lucid does not use static methods as namespaced free functions.

Methods are functions attached to classes, interfaces, or traits. Instance methods receive ``self``.

.. code-block:: python

   class Rectangle:
       width: float
       height: float

       def area(self) -> float:
           return self.width * self.height

8.2. Class definitions
----------------------

Class definitions describe concrete object types with fixed stored fields, member definitions, and optional value-semantics options.

8.3. Class methods and factories
--------------------------------

Class methods receive ``cls`` and are inherited as class-level behavior.

.. code-block:: python

   class User:
       count = 0

       classmethod total_created(cls) -> int:
           return cls.count

Factories also receive ``cls``, but they are constructors for the exact class where they are defined.

8.4. Interfaces
---------------

Interfaces declare required APIs. They do not store data and do not provide method bodies.

.. code-block:: python

   interface Closeable:
       declare close(self) -> None

8.5. Traits
-----------

Traits provide reusable method bodies. They do not declare fields.

.. code-block:: python

   trait SizedTruthy(Sized, Truthy):
       def __bool__(self) -> bool:
           return self.__len__() != 0

8.6. Control flow
-----------------

Lucid uses Python-like compound control-flow syntax where specified. Boolean tests require ``bool`` or explicit truth behavior.

8.7. Type parameter lists
-------------------------

Type parameter lists appear on generic definitions. Variance markers are part of the parameter syntax:

.. code-block:: python

   interface Producer[+K]:
       declare get(self) -> K

   interface Consumer[-K]:
       declare put(self, value: K) -> None

   class Cell[=K]:
       value: K

8.8. Annotations
----------------

Annotations are part of the visible program contract. They describe fields, function parameters, return values, local variables, and interface requirements.
