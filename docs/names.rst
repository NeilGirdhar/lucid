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

Destructuring with ``let``
------------------------------

Assignment binds one name to one value; it has no way to pull several
named fields out of a class instance in one statement. ``let`` binds a
class's fields directly, by position — the same order a factory call
already uses to construct one:

.. code-block:: python

   let Point(x, y) = origin

which binds ``x`` to ``origin.x`` and ``y`` to ``origin.y``, the same
positional correspondence ``Point(1.0, 2.0)`` already uses going the
other way. The names inside the pattern are ordinary new bindings, not
required to match the class's own field names, the same way tuple
unpacking's ``a, b = pair`` never cared what ``pair``'s own names were:

.. code-block:: python

   let Point(px, py) = origin   # binds px, py -- Point's own fields stay x, y

``let`` is already the signal that the left side is a pattern rather
than an assignment target, so the binding itself stays the ordinary
``=`` every other binding already uses — no second operator like
basedpython's ``:=`` is needed to mark it twice.

The pattern has to be one the checker can already prove: ``origin``'s
static type must already be (a subtype of) ``Point``, the same way an
unconditional destructure has no ``case _:`` to fall back to if it were
wrong. A value whose type could be one of several variants needs
`Exhaustive pattern matching <control-flow.rst>`_ instead, which handles
"this could be any of these" — ``let`` is only for "this already is one
specific thing, pull it apart." Anonymous records destructure the same
way, by their own declared order:

.. code-block:: python

   let (x, y) = midpoint   # midpoint: (x: int, y: int)

The same pattern works anywhere a name would otherwise bind — a ``for``
target:

.. code-block:: python

   for Point(x, y) in points:
       ...

or a parameter:

.. code-block:: python

   def distance(Point(x1, y1), Point(x2, y2)) -> float:
       ...

`Exhaustive pattern matching <control-flow.rst>`__'s own ``case`` gains
the same grammar for the same reason: ``case Point(x, y):`` narrows and
destructures in one step, instead of narrowing in ``case`` and then
destructuring in a separate ``let`` right after it.

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
