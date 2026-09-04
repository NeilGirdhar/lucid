Calls, indexing, and operators
=================================

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
``Callable[[Item], float]``, built without calling ``score`` at all, ready
to be called once ``sorted`` supplies the missing argument itself. No
import, no wrapper object, no separate type to teach a checker about.

Multiple holes fill left to right, matching the order they appear:

.. code-block:: python

   def combine(a: A, b: B, c: C) -> R:
       ...

   combine(_, y, _)   # Callable[[A, C], R]

A keyword hole stays keyword in the result:

.. code-block:: python

   combine(x, c=_)    # Callable[[C], R]

The type of a partial application is ordinary generic inference, not a
special case: given ``f: Callable[[A, B], R]``, ``f(x, _)`` has type
``Callable[[B], R]``, the same way substituting one type parameter of any
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

Comma-separated indexing passes multiple arguments
--------------------------------------------------------

Python parses ``x[1, 2, 3]`` as a single tuple index. Lucid treats
comma-separated indexing as multiple index arguments.

.. code-block:: python

   x[1, 2, 3]

means:

.. code-block:: python

   x.__getitem__(1, 2, 3)

Python's rule doesn't just create ambiguity, it destroys information: ``x[1,
2, 3]`` and ``x[(1, 2, 3)]`` compile to the identical call,
``x.__getitem__((1, 2, 3))``, so ``__getitem__`` has no way to tell which one
the caller wrote — not "must guess," since by the time it runs there is
nothing left to guess from. NumPy hits this wall directly: ``arr[i, j, k]``
(three-axis indexing) and ``arr[(i, j, k)]`` (looking up one tuple-valued
key) are the same call, so NumPy simply cannot support both; indexing by an
actual tuple object as a single key needs a workaround like wrapping it in
another tuple. Lucid has no tuple to reach for either, so it keeps the two
meanings separate by construction: multiple arguments are comma-separated,
and one combined argument is built explicitly, with the immutable list
marker (see `No tuple or namedtuple type <collections.rst>`_):

.. code-block:: python

   grid[row, col]         # two index arguments
   grid[![row, col]]      # one combined index

Assignment follows the same rule:

.. code-block:: python

   grid[row, col] = value        # grid.__setitem__(row, col, value)
   table[![row, col]] = value    # table.__setitem__(![row, col], value)

This makes multidimensional containers, list-keyed mappings, type checking,
and error messages agree on the same syntax instead of relying on library-side
tuple parsing conventions.

An explicit combined index remains available by constructing one directly,
as ``grid[![row, col]]`` does above. Single indexing passes one index the
same way:

.. code-block:: python

   x[0]

means:

.. code-block:: python

   x.__getitem__(0)

No ``__getitem__`` iteration fallback
-------------------------------------------

Python has an old sequence-iteration fallback: if an object has ``__getitem__``
but no ``__iter__``, iteration may call ``__getitem__`` with successive integers
until indexing fails.

.. code-block:: python

   class Pages:
       def __getitem__(self, index):
           if index >= 3:
               raise IndexError
           return "page " + str(index)

   pages = Pages()

   for page in pages:
       print(page)

Even though ``Pages`` never says it is iterable, Python prints ``page 0``,
``page 1``, and ``page 2``. The ``for`` loop silently tries ``pages[0]``,
``pages[1]``, ``pages[2]``, and stops when ``pages[3]`` raises
``IndexError``.

The fallback means Python cannot even define "iterable" as "has
``__iter__``": ``Pages`` is iterable by that test's own behavior — ``for``
accepts it — yet Python's own ABC disagrees with its own ``for`` loop:

.. code-block:: python

   from collections.abc import Iterable

   isinstance(pages, Iterable)  # False
   iter(pages)                  # succeeds anyway

There is no attribute check that answers "is this iterable" correctly;
the only way to find out is to try iterating and see. Lucid removes that
sequence protocol. Indexing and iteration are separate capabilities. A type
is iterable only if it implements or inherits from ``Iterable``, and that
check is finally trustworthy.

Unpacking
------------

Unpacking's assignment-target semantics and its precedence relative to
binary operators are both covered together in
`No tuple or namedtuple type <collections.rst>`__.

Decorators
-------------

``@dec`` above a ``def`` means what it means in Python: ``f`` is
rebound to ``dec(f)``. Stacked decorators apply bottom-up, the closest one
to ``def`` first:

.. code-block:: python

   @a
   @b
   def f():
       ...

means:

.. code-block:: python

   f = a(b(f))

Preserving identity
~~~~~~~~~~~~~~~~~~~~~~~

A decorator that wraps a function in a new closure replaces it with an
object that has its own ``__name__``, ``__doc__``, and signature, unrelated
to the function it replaced. Python's fix is ``functools.wraps`` — a
decorator author has to remember to call it inside their own decorator, on
every decorator, or the replacement silently carries the wrong identity.

Lucid makes ``@`` itself responsible for identity instead of the decorator.
Applying ``@`` always carries the pre-decoration function's name, qualname,
doc, module, and signature onto whatever the decorator returns, along with
a link back to the original:

.. code-block:: python

   def timed[**P, R](f: Callable[P, R]) -> Callable[P, R]:
       def wrapper(*args: P.args, **kwargs: P.kwargs) -> R:
           start = now()
           result = f(*args, **kwargs)
           log(f.__name__, now() - start)
           return result
       return wrapper

   @timed
   def slow_query(id: int) -> Row:
       ...

   slow_query.__name__  # "slow_query" — guaranteed by @, not opted into

There is nothing to forget: identity preservation is not a convention a
decorator can skip, it is what ``@`` means. A decorator that genuinely
wants to produce something with a different identity is free to — just not
through ``@``; plain function application (``f = dec(f)``) carries no such
guarantee.

Decorator factories need no partial application
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

A parameterized decorator in Python — ``@lru_cache(maxsize=128)`` — is
built from a function that returns a function that returns a function: one
level to take the configuration, one to take the function being decorated,
one to wrap the call. The middle level exists only to delay receiving the
decorated function, which is manual partial application done by hand.

Lucid's decorator factories are just ordinary functions, with the decorated
function as an ordinary parameter:

.. code-block:: python

   def lru_cache(f: Callable[P, R], *, maxsize: int) -> Callable[P, R]:
       ...

   @lru_cache(maxsize=128)
   def slow_query(id: int) -> Row:
       ...

``lru_cache(maxsize=128)`` is missing exactly one required parameter,
``f``, and it is shaped like the decoration target — a ``Callable[P, R]``
producing a ``Callable[P, R]``. At a ``@`` site, that missing parameter is
filled with the decorated function automatically, with no ``_`` needed:
unlike `Partial application with _`_ in an ordinary call, ``@`` already
knows exactly which argument is missing and what has to go there, so
there is nothing ambiguous left to mark. If a decorator factory leaves more
than one ``Callable[P, R]``-shaped parameter unfilled, which one is the
decoration target is genuinely ambiguous, and that is a compile error;
``_`` is the way to disambiguate explicitly, the same as any other partial
application:

.. code-block:: python

   @lru_cache(_, maxsize=128)
   def slow_query(id: int) -> Row:
       ...

Python needs ``functools.wraps`` inside every wrapping decorator and a
nested closure inside every parameterized one, both by convention, both
easy to get wrong and invisible at the call site when they are. Lucid
makes both guarantees instead: identity survives ``@`` because that is
what ``@`` does, and a decorator factory is just a function, because
partial application is a general call-site mechanism rather than a pattern
decorators have to hand-roll.

