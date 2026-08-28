Source basics and names
========================

.. contents:: Table of contents
   :depth: 2
   :local:

File extension
----------------

Lucid source files use the ``.lcd`` extension.

Python-like indentation
--------------------------

Lucid uses Python-like indentation and layout.

Ordinary binding
-------------------

Names bind through definitions, imports, assignments, and parameters.
Assignment binds names, updates declared fields, or delegates to explicit
assignment behavior such as setters and item assignment.

Black-hole assignment with ``_``
-----------------------------------

Python treats ``_`` as an ordinary name by default, even though many codebases
use it by convention for ignored values. Lucid makes ``_`` a black-hole
assignment target: assigning to it discards the value instead of binding a name.

.. code-block:: python

   _ = compute()
   result, _ = split_pair()

``_`` is not an expression:

.. code-block:: python

   use(_)  # error

No ``global``
----------------

Python uses ``global`` to let assignment inside a function rebind a module
binding:

.. code-block:: python

   counter = 0

   def next_id() -> int:
       global counter
       counter += 1
       return counter

Lucid has no ``global`` declaration. Reading module bindings is allowed, but
assignment to a name inside a function is local to that function. Shared module
state is represented by an explicit mutable object:

.. code-block:: python

   counter = Cell(0)

   def next_id() -> int:
       counter.value += 1
       return counter.value

No ``nonlocal``
------------------

Python uses ``nonlocal`` to let assignment inside an inner function rebind a
name from an enclosing function:

.. code-block:: python

   def make_counter():
       count = 0

       def next():
           nonlocal count
           count += 1
           return count

       return next

Lucid has no ``nonlocal`` declaration. Reading enclosing bindings is allowed,
but assignment to a name is local to the current function. Closure state uses an
explicit mutable object:

.. code-block:: python

   def make_counter() -> Callable[[], int]:
       count = Cell(0)

       def next() -> int:
           count.value += 1
           return count.value

       return next
