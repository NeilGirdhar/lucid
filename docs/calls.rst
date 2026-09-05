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
for black-hole assignment (see `Ordinary binding <names.rst>`_)
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

Anonymous functions
------------------------

Python's ``lambda`` is a second, narrower construct for the same idea a
``def`` already covers: an expression-only body, its own separate keyword
borrowed from lambda calculus, no statements allowed at all. Lucid has no
``lambda``. An anonymous function is just a ``def`` with no name, used as
an expression instead of a statement — one construct, whether or not it's
bound to a name:

.. code-block:: python

   add = def(x: int, y: int): x + y

Parameter types can be omitted when the position already supplies a
``Callable[...]`` to check against — the same ordinary generic inference
partial application above already relies on, not a new mechanism, and
never inference from how the body uses them:

.. code-block:: python

   add: Callable[[int, int], int] = def(x, y): x + y   # x, y: int, from add's own annotation
   cache.get_or_put("ada", def(): load_user("ada"))    # (): User, from get_or_put's signature

With nothing to check against, types are written out, same as any named
``def``:

.. code-block:: python

   f = def(x, y): x + y            # error: no expected type to infer x, y from
   f = def(x: int, y: int): x + y  # fine

A body written on the same line as the ``:`` is a bare expression, whose
value the anonymous function returns — this is the one shorthand that
exists only here. `Exhaustive pattern matching <control-flow.rst>`__
deliberately has no equivalent: a ``match`` case is always a statement
block, so letting its last line double as a return value would make that
block "secretly" an expression some of the time. An anonymous function
has no such ambiguity, because the two forms are never the same shape: a
body on the ``:`` line is an expression, full stop, and a body on an
indented block below it is an ordinary statement suite needing its own
``return``, exactly like a named ``def``:

.. code-block:: python

   def(x: int) -> int:
       log(x)
       return x + 1

Zero parameters can drop the empty ``()`` entirely, in either form —
there is nothing to write between ``def`` and ``:`` when there are no
parameters to name:

.. code-block:: python

   log(info_level, def: f"Some string {blah()}")

   def:
       log("called")
       return 1

None of this applies to a named ``def``: naming one still means an
ordinary statement, an indented block, and an explicit ``return``,
unchanged. The expression-bodied form is deliberately scoped to anonymous
functions, the short-lived, immediately-consumed values ``lambda`` was
for — not a second way to write an ordinary function.

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

