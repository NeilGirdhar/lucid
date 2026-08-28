Control flow and statements
==============================

.. contents:: Table of contents
   :depth: 2
   :local:

Explicit boolean tests
--------------------------

Boolean tests require ``bool`` or explicit truth behavior. This applies to
``if``, ``while``, and other conditional control-flow positions.

Explicit iteration
----------------------

``for`` loops require an explicitly iterable value. A type is iterable only if
it implements or inherits from ``Iterable``.

No loop ``else``
--------------------

Python loop ``else`` clauses run when a loop finishes without ``break``. Lucid
removes loop ``else``.

``if_broken`` loop clauses
-------------------------------

Lucid adds an optional ``if_broken`` clause for loops. ``if_broken`` is a
keyword. The ``if_broken`` suite runs if the loop exits by ``break``. Ordinary
fall-through code handles the case where a loop exits normally without
``break``.

.. code-block:: python

   def find_match(items: Iterable[Item]) -> Item | none:
       for item in items:
           if is_match(item):
               found = item
               break
       if_broken:
           return found
       return none

Nested ``if_broken`` clauses
----------------------------------

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

Unspecified simple statements
-----------------------------------

This sketch has not yet specified Lucid's full behavior for ``assert``,
``pass``, ``del``, ``raise``, ``break``, ``continue``, or type-alias statements.
The ``skip`` keyword is an expression-level elision marker, not a replacement
for the statement-level ``pass`` placeholder.
