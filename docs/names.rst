Names, binding, and scope
==========================

.. contents:: Table of contents
   :depth: 3
   :local:

Ordinary binding
-------------------

Names bind through definitions, imports, assignments, and parameters.
Assignment binds names, updates declared fields, or delegates to explicit
assignment behavior such as setters and item assignment.

Final local variables
-------------------------

``final`` marks an ordinary local variable that can be assigned once and
never reassigned:

.. code-block:: python

   final total = 0
   total = total + 1  # error: total is final

Like a final field, this is a property of the binding, not of whatever
value it holds: a ``final`` binding to a mutable object still lets that
object be mutated through it; it only rules out pointing the name somewhere
else.

Black-hole assignment with ``_``
-----------------------------------

Python treats ``_`` as an ordinary name, so a value assigned to it stays
readable — a stray later use (a copy-pasted line, a half-finished rename)
silently reads a discarded value instead of failing:

.. code-block:: python

   _ = expensive_setup()
   if _:      # Python: silently reads the discarded value
       proceed()

Lucid makes ``_`` a black-hole assignment target: assigning to it discards
the value, and ``_`` is not an expression, so reading it is a syntax error
rather than a silent bug.

.. code-block:: python

   _ = compute()
   result, _ = split_pair()
   use(_)  # error

No scope-rebinding declarations
-----------------------------------

A statement's meaning should not depend on some other statement elsewhere in
the same function. Python's ``global``/``nonlocal`` violate this: whether
``x = value`` binds a new local or rebinds an enclosing name depends on a
declaration that can sit anywhere in the function body, so an assignment can
retroactively change what an *earlier, unrelated-looking* read meant:

.. code-block:: python

   counter = 0

   def increment():
       print(counter)    # UnboundLocalError
       counter += 1      # this line is why

Lucid removes the declaration rather than live with that coupling: assignment
is always local, independent of anything else in the function. Sharing
mutable state across scopes instead goes through an explicit object, so the
sharing is visible at the object's declaration and at each call site that
touches it, rather than hidden behind a keyword elsewhere in the function.

No ``global``
~~~~~~~~~~~~~~~~

Python uses ``global`` to let assignment inside a function rebind a module
binding:

.. code-block:: python

   counter = 0

   def next_id() -> int:
       global counter
       counter += 1
       return counter

Lucid represents shared module state with an explicit mutable object:

.. code-block:: python

   counter = Cell(0)

   def next_id() -> int:
       counter.value += 1
       return counter.value

No ``nonlocal``
~~~~~~~~~~~~~~~~~~

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

Closure state uses the same pattern:

.. code-block:: python

   def make_counter() -> () -> int:
       count = Cell(0)

       def next() -> int:
           count.value += 1
           return count.value

       return next
