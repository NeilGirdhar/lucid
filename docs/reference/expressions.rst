6. Expressions
==============

6.1. Atoms and primaries
------------------------

Lucid uses Python-like expression syntax unless this reference says otherwise.

The black-hole target ``_`` is not an expression. It is valid only as an
assignment target, where it discards the assigned value.

.. code-block:: python

   _ = compute()
   use(_)  # error

6.2. Calls
----------

Calling a class invokes Lucid construction. Calling an ordinary function, method, class method, or factory uses Python-like call syntax.

A generator expression written directly as a call argument expands into
positional arguments for the call:

.. code-block:: python

   f(x for x in [x_1, x_2, x_3])

means:

.. code-block:: python

   f(x_1, x_2, x_3)

To pass an actual generator object, bind it first or use another explicit
generator-producing expression before the call.

6.3. Indexing
-------------

Single indexing passes one index.

.. code-block:: python

   x[0]

means:

.. code-block:: python

   x.__getitem__(0)

Comma-separated indexing passes multiple arguments.

.. code-block:: python

   x[1, 2, 3]

means:

.. code-block:: python

   x.__getitem__(1, 2, 3)

An explicit tuple index remains available by writing the tuple explicitly.

.. code-block:: python

   x[(1, 2, 3)]

means:

.. code-block:: python

   x.__getitem__((1, 2, 3))

Assignment follows the same rule:

.. code-block:: python

   x[1, 2, 3] = v

means:

.. code-block:: python

   x.__setitem__(1, 2, 3, v)

Indexing does not imply iteration. Defining ``__getitem__`` for integer indexes
does not make a type iterable.

6.4. Boolean operations
-----------------------

Lucid conditionals require a boolean value or an explicit boolean protocol.

.. code-block:: python

   if ready:
       run()

Values are not automatically truthy just because they are nonzero, non-empty, or non-null.

.. code-block:: python

   if 1:        # error
   if "hello":  # error unless str explicitly implements truth behavior
   if items:    # error unless the type explicitly implements truth behavior

Types that want truth behavior define ``__bool__``.

.. code-block:: python

   interface Truthy:
       declare __bool__(self) -> bool

Length does not imply truth. A sized type can opt in explicitly:

.. code-block:: python

   interface Sized:
       declare __len__(self) -> int

   trait SizedTruthy(Sized, Truthy):
       def __bool__(self) -> bool:
           return self.__len__() != 0

``bool`` does not support numeric arithmetic or bitwise arithmetic operators.
The operators ``+``, ``-``, ``*``, ``/``, ``%``, ``|``, ``&``, and ``^`` are
invalid for boolean operands. Use boolean operators for logic and an explicit
conversion for intentional integer arithmetic:

.. code-block:: python

   True + True       # error
   True & flag       # error
   True or flag
   int(True) + 1

``bool`` is not orderable. Comparisons such as ``False < True`` are invalid.

Numeric ordering is defined only for operand types that provide an explicit
ordering operation for those two types. ``complex`` is not orderable.

Bitwise operators are integer-like operations, not general numeric operations.
The operators ``|``, ``&``, ``^``, ``~``, ``<<``, and ``>>`` are valid only for
types that explicitly provide those operations. ``bool``, ``float``, and
``complex`` do not provide them.

6.5. Operator precedence
------------------------

Lucid uses Python's operators and Python's order of operations unless this document says otherwise.

Unpacking binds tighter than binary operators.

.. code-block:: python

   *x + y

means:

.. code-block:: python

   (*x) + y

not:

.. code-block:: python

   *(x + y)

6.6. Binary operators
---------------------

Binary operators are relations between two operand types.

.. code-block:: python

   def dispatch __add__(lhs: X, rhs: Y) -> Z:
       ...

Then:

.. code-block:: python

   x + y

dispatches on both runtime types.

Interfaces can require one element of a multiple-dispatch operation with
``declare dispatch``:

.. code-block:: python

   interface Addable:
       declare dispatch __add__(lhs: Self, rhs: Self) -> Self

A concrete implementation satisfies that requirement when the generic operation
has an applicable dispatch definition after substituting the concrete type for
``Self``:

.. code-block:: python

   def dispatch __add__(lhs: Vector2, rhs: Vector2) -> Vector2:
       ...

The matching dispatch can be supplied wherever dispatch definitions for that
generic operation are allowed. It is not owned by the left operand. Lucid follows
Julia's method-applicability model here: a dispatch definition applies when its
declared parameter types accept the runtime argument types. If an interface
requires ``__add__(Self, C)``, then a concrete ``A`` satisfies that requirement
if the ``__add__`` dispatch table contains a method applicable to ``(A, C)``.
That may be an exact ``(A, C)`` method, a method written for parent classes or
interfaces of ``A`` or ``C``, a method provided with ``C``, or a method in
another module that is allowed to extend the generic operation.

This lets cross-type operations be defined directly:

.. code-block:: python

   def dispatch __add__(lhs: Duration, rhs: Timestamp) -> Timestamp:
       ...

   def dispatch __add__(lhs: Timestamp, rhs: Duration) -> Timestamp:
       ...

The operation is not owned by the left operand. Lucid does not need reflected binary methods such as ``__radd__``, and does not use ``NotImplemented`` as an operator negotiation protocol.
