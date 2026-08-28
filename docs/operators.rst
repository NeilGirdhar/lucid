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

