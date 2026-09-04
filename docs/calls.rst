Calls
=====

Lucid keeps Python's call syntax but tightens a few of its looser corners:
argument elision, argument order, partial application, and one grammar
special-case Python carries for generator expressions.

.. contents:: Table of contents
   :depth: 2
   :local:

``skip`` in calls
--------------------

The ``skip`` keyword may appear as a positional or keyword argument. A
positional argument that reaches ``skip`` is omitted. A keyword argument whose
value reaches ``skip`` is omitted.

.. code-block:: python

   print(1, skip, 3, file=skip)

means:

.. code-block:: python

   print(1, 3)

Positional arguments precede keyword arguments
----------------------------------------------------

Python's rule looks simple — positional arguments come before keyword
arguments — but its grammar (PEP 448) treats star-unpacking as an
exception: an unpacked iterable still binds positionally, but is allowed to
appear in the source *after* a keyword argument, as long as nothing
double-star-unpacked has appeared yet. Source order and binding order can
disagree:

.. code-block:: python

   def f(a, b, c):
       ...

   f(c=3, *[1, 2])   # legal Python: a=1, b=2, c=3

``c=3`` is written first but binds to the last parameter; ``*[1, 2]``,
written last, supplies the first two. The call has to be read back to
front to see what it does.

Lucid closes the exception: positional arguments — plain and
star-unpacked alike — must precede every keyword argument, including
double-star unpacking, with no interleaving permitted.

.. code-block:: python

   f(*[1, 2], c=3)   # fine
   f(c=3, *[1, 2])   # error: positional argument follows keyword argument

Within the positional section, plain arguments and ``*``-unpacking can mix
freely in any relative order, since both bind purely by position, in the
order written. The same holds for named arguments and ``**``-unpacking
within the keyword section. The one rule that never bends is between the
two sections: positional, then keyword, always.

Partial application with ``_``
------------------------------------

Python's tool for fixing some of a call's arguments ahead of time is
``functools.partial``: ``partial(score, weights)`` returns a
``functools.partial`` object, not an ordinary function, and type checkers
have long struggled to treat it as the ``Callable`` it behaves like.

Lucid reuses ``_`` for this — the same "unspecified" marker already used
for black-hole assignment (see `Ordinary binding <source-and-names.rst>`_)
and the match wildcard (see
`Exhaustive pattern matching <control-flow.rst>`_), now in a call's
argument list. An argument position filled with ``_`` is not a value; it
leaves that position open, and the call itself becomes a new callable
awaiting whatever positions were left unfilled:

.. code-block:: python

   sorted(items, key=score(weights, _))

``score(weights, _)`` is not a call to ``score`` — it is a value of type
``Callable[(Item,), float]``, built without calling ``score`` at all, ready
to be called once ``sorted`` supplies the missing argument itself. No
import, no wrapper object, no separate type to teach a checker about.

Multiple holes fill left to right, matching the order they appear:

.. code-block:: python

   def combine(a: A, b: B, c: C) -> R:
       ...

   combine(_, y, _)   # Callable[(A, C), R]

A keyword hole stays keyword in the result:

.. code-block:: python

   combine(x, c=_)    # Callable[(C,), R]

The type of a partial application is ordinary generic inference, not a
special case: given ``f: Callable[(A, B), R]``, ``f(x, _)`` has type
``Callable[(B,), R]``, the same way substituting one type parameter of any
other generic leaves the rest.

Generator call expansion
----------------------------

Python's grammar special-cases exactly this shape: a bare generator
expression is allowed as a call's sole argument, borrowing the call's own
parentheses as the generator expression's parentheses. It is not a
consequence of how expressions normally work in a call — it is a dedicated
grammar production for this one case, and it is fragile in a way that shows
the special-casing: ``sum(x for x in items)`` works, but adding a second
argument breaks it, because now there are two arguments and the borrowed
parentheses no longer apply. ``sum(x for x in items, start=0)`` is a syntax
error; the generator expression needs its own parentheses back —
``sum((x for x in items), start=0)``. Whether the parentheses can be omitted
depends on argument count, not on what the expression means.

Lucid needs no special case: a generator expression written as a call
argument is treated the same as any other expression written there, and
argument expansion follows regardless of how many other arguments are
present.

.. code-block:: python

   f(x for x in [x_1, x_2, x_3])

means:

.. code-block:: python

   f(x_1, x_2, x_3)

To pass an actual generator object, parenthesize the generator expression:

.. code-block:: python

   f((x for x in items))

