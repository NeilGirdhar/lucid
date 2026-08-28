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

Python treats ``f(x for x in items)`` as a call with one generator object
argument. Lucid treats a generator expression written directly as a call
argument as argument expansion.

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

This removes a common ambiguity in Python APIs. In Python, ``grid[row, col]``
and ``grid[(row, col)]`` both arrive as one tuple argument, so a container must
guess whether the user meant a two-dimensional index or a single tuple key.
Lucid keeps those meanings separate:

.. code-block:: python

   grid[row, col]       # two index arguments
   grid[(row, col)]     # one tuple index

Assignment follows the same rule:

.. code-block:: python

   grid[row, col] = value      # grid.__setitem__(row, col, value)
   table[(row, col)] = value   # table.__setitem__((row, col), value)

This makes multidimensional containers, tuple-keyed mappings, type checking,
and error messages agree on the same syntax instead of relying on library-side
tuple parsing conventions.

Explicit tuple indexing
---------------------------

An explicit tuple index remains available by writing the tuple explicitly:

.. code-block:: python

   x[(1, 2, 3)]

Single indexing
-------------------

Single indexing passes one index:

.. code-block:: python

   x[0]

means:

.. code-block:: python

   x.__getitem__(0)

No ``__getitem__`` iteration fallback
------------------------------------------

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

Lucid removes that sequence protocol. Indexing and iteration are separate
capabilities. A type is iterable only if it implements or inherits from
``Iterable``.

Unpacking precedence
------------------------

Lucid uses Python's operators and Python's order of operations unless this
document says otherwise. Unpacking binds tighter than binary operators:

.. code-block:: python

   *x + y

means:

.. code-block:: python

   (*x) + y

not:

.. code-block:: python

   *(x + y)

Multiple-dispatch operators
--------------------------------

Binary operators are relations between two operand types.

.. code-block:: python

   def dispatch __add__(lhs: X, rhs: Y) -> Z:
       ...

Then:

.. code-block:: python

   x + y

dispatches on both runtime types.

``declare dispatch`` requirements
--------------------------------------

Interfaces can require one element of a multiple-dispatch operation with
``declare dispatch``:

.. code-block:: python

   interface Addable:
       declare dispatch __add__(lhs: Self, rhs: Self) -> Self

A concrete implementation satisfies that requirement when the generic operation
has an applicable dispatch definition after substituting the concrete type for
``Self``.

No reflected binary methods
--------------------------------

The matching dispatch can be supplied wherever dispatch definitions for that
generic operation are allowed. It is not owned by the left operand. Lucid does
not need reflected binary methods such as ``__radd__``.

No ``NotImplemented`` operator negotiation
------------------------------------------------

Lucid does not use ``NotImplemented`` as an operator negotiation protocol.
Dispatch applicability decides whether an operation is available.
