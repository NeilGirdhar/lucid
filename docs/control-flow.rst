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

Exhaustive pattern matching
-----------------------------------

Python's ``match``/``case`` cannot check that every case is covered: Python
classes are open, so there is no way to enumerate every shape a value could
have, and a non-exhaustive match with no matching case just does nothing at
runtime. Lucid's recursive union aliases are closed — every alternative is
named in one place — so the same check Rust and Swift already give their
closed enums is possible here, and Lucid does it:

.. code-block:: python

   type PyTree[L] = L | list[PyTree[L]] | dict[str, PyTree[L]]

   def tree_map(tree: PyTree[Array], f: Callable[[Array], bool]) -> PyTree[bool]:
       match tree:
           case Array:
               return f(tree)
           case list[PyTree[Array]]:
               return [tree_map(item, f) for item in tree]
           case dict[str, PyTree[Array]]:
               return {k: tree_map(v, f) for k, v in tree.items()}

The three cases are exactly ``PyTree``'s three alternatives, and that is
checked: leaving one out is an error, not a silent no-op the way an
incomplete Python ``match`` is. If ``PyTree`` ever grows a fourth
alternative, every ``match`` over it that lacks a matching case becomes an
error at the point it stops being exhaustive, rather than a bug found later
at runtime.

A ``case`` pattern is just a type — ``case Type:`` narrows the subject to
``Type`` for that case, using the subject's own name, without introducing
any pattern grammar of its own. That only works when the subject already is
a name, the way ``tree`` is above. When it is some other expression — a
call, an attribute access, anything without a name of its own to reuse —
``match`` needs one supplied, with ``as``:

.. code-block:: python

   match parse(text) as outcome:
       case ParseError:
           ...
       case Value:
           ...

``as outcome`` names the subject itself, for the cases to narrow and refer
to — it does not name a value the ``match`` produces, since ``match``
does not produce one. ``as`` is required whenever the subject is not
already a bare name, and has nothing to do otherwise: writing
``match tree as t:`` to rename an already-bare subject buys nothing that
``match tree:`` did not already have.

A bare ``_`` matches anything, satisfying exhaustiveness for a match over a
type that is not a closed union at all. Matching against a non-closed type
— ``object``, an interface, anything without a known, finite set of
alternatives — cannot be checked for exhaustiveness the way ``PyTree`` can,
and requires an explicit ``case _:`` for the same reason a Rust ``match``
over an integer needs one: there is no finite set of cases to exhaust.

``match`` is a statement, not an expression, and each ``case`` is an
ordinary statement block — any sequence of statements, exactly like the
body of an ``if``, a loop, or a function — not a single expression. It is
purely the exhaustive, type-narrowing form of an ``if``/``elif`` chain,
nothing more. Producing a value from it works exactly the way producing a value
from an ``if``/``elif`` chain already does: ``return``, as above, or an
assignment repeated in every branch. That repetition is a real, known cost,
not an oversight — the alternative was a case body that is secretly only
ever a single implicit-value expression, which nothing else in the language
does, and which is worse than the repetition it would save.

Pattern matching and dispatch solve related problems differently, and the
choice between them is the same one type theory calls the expression
problem. ``match`` requires every alternative in one place and checks that
nothing is missing; it is the right tool when the whole set of shapes is
closed and known ahead of time. `Dispatch beyond operators <dispatch.rst>`_
builds the same kind of function the opposite way: an open, growing set of
independently-checked cases that anyone can add to later, with no
exhaustiveness check possible, because the set is never closed. A fixed
``PyTree`` with two known container shapes is exactly matched to ``match``;
a version meant to stay open to third parties (see
`Existential types <types.rst>`_) is exactly matched to dispatch instead.

Unspecified simple statements
-----------------------------------

This sketch has not yet specified Lucid's full behavior for ``assert``,
``pass``, ``del``, ``raise``, ``break``, or ``continue``.
The ``skip`` keyword is an expression-level elision marker, not a replacement
for the statement-level ``pass`` placeholder.
