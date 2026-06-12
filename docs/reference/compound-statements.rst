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

Every class has a generated ``replace`` factory for copy-with-update
construction. Like other factories, ``replace`` is not inherited; each class's
``replace`` constructs that exact class.

8.4. Interfaces
---------------

Interfaces declare required APIs. They do not store data and do not provide method bodies.

.. code-block:: python

   interface Closeable:
       declare close(self) -> None

Interfaces can also require multiple-dispatch operations with ``declare
dispatch``. Such members declare obligations on generic operations, not
instance methods owned by the left operand.

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

``for`` loops require an explicitly iterable value. A type is iterable only if
it implements or inherits from ``Iterable``. Lucid does not use Python's
sequence fallback where repeated integer ``__getitem__`` calls create iteration
behavior.

Loops may include an optional ``if_broken`` clause. The ``if_broken`` suite runs
if the loop exits by ``break``. Lucid does not have loop ``else`` clauses.
Ordinary fall-through code handles the case where a loop exits normally without
``break``.

.. code-block:: python

   def find_match(items: Iterable[Item]) -> Item | None:
       for item in items:
           if is_match(item):
               found = item
               break
       if_broken:
           return found
       return None

``if_broken`` is a soft keyword in this position only. It is a single clause
keyword, not the two keywords ``if break`` placed next to each other.

In nested loops, an ``if_broken`` clause belongs to the loop immediately before
it. A ``break`` inside an inner loop can therefore be handled by the inner
loop's ``if_broken`` clause, and that clause can choose to break the outer loop:

.. code-block:: python

   for row in rows:
       while has_more(row):
           if matches(row.current):
               break
       if_broken:
           break

``if_broken`` is optional.

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
