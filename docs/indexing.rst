Indexing
========

``x[...]`` desugars to ``__getitem__``/``__setitem__``, but Lucid changes
what the desugaring means for multiple indices, removes a legacy iteration
fallback, and covers how indexed and destructuring access relate.

.. contents:: Table of contents
   :depth: 2
   :local:

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

No ``__delitem__``
-------------------------

Python desugars ``del d[key]`` to ``d.__delitem__(key)`` — a third
indexing dunder, reached through a statement instead of a call, for an
operation every mapping or sequence type already needs to expose some
other way: ``dict.pop`` and ``set.remove`` exist because "did it work"
and "what was removed" matter more often than bare deletion says. The
same dunder handles a sequence slice too — ``del lst[i:j]`` calls
``lst.__delitem__(slice(i, j))`` — so a single removal, a slice removal,
and a mapping removal all route through one overloaded method reached
only through statement syntax. Lucid removes ``__delitem__`` and every
``del x[...]`` form with it. Removing a key or an element is an ordinary
method call, and removing a range is a third, ``dispatch``-resolved case
of the same method rather than a separate operation:

.. code-block:: python

   del d[key]      # not part of Lucid
   d.pop(key)      # removes key, returns its value

   del lst[i]      # not part of Lucid
   lst.pop(i)      # removes the element at i, returns it

   del lst[i:j]        # not part of Lucid
   lst.pop(i, j)       # removes the range [i, j), returns it as a list

``lst.pop(i)`` and ``lst.pop(i, j)`` are two ``dispatch`` definitions, not
one method with an optional second parameter — they return different
types (``T`` versus ``list[T]``), the same reason `No @overload
<dispatch.rst>`_ replaces ``@overload`` with ``dispatch`` rather than a
default argument: a caller who wrote ``pop(i, j)`` should get a
``list[T]`` back without narrowing a union it never needed.

``pop`` is not the whole story: it addresses by key or index, the same
handle ``del`` already needed, so it is the direct replacement wherever
``del`` applied. Removing by value instead — no index in hand, or none to
have, the way a set has none at all — is a different lookup and keeps its
own method, ``remove``:

.. code-block:: python

   lst.remove(value)  # removes the first occurrence of value
   s.remove(item)      # the only way to take a specific item out of a set

Indexing keeps exactly the two dunders ``x[...]`` and ``x[...] = value``
actually desugar to, ``__getitem__`` and ``__setitem__``; nothing reached
through the syntax goes further than what the syntax itself spells out.

Unpacking
------------

Unpacking's assignment-target semantics and its precedence relative to
binary operators are both covered together in
`No tuple or namedtuple type <collections.rst>`__.

