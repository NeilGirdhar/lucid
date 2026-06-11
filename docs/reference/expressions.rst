6. Expressions
==============

6.1. Atoms and primaries
------------------------

Lucid uses Python-like expression syntax unless this reference says otherwise.

6.2. Calls
----------

Calling a class invokes Lucid construction. Calling an ordinary function, method, class method, or factory uses Python-like call syntax.

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

   @dispatch
   def __add__(lhs: X, rhs: Y) -> Z:
       ...

Then:

.. code-block:: python

   x + y

dispatches on both runtime types.

This lets cross-type operations be defined directly:

.. code-block:: python

   @dispatch
   def __add__(lhs: Duration, rhs: Timestamp) -> Timestamp:
       ...

   @dispatch
   def __add__(lhs: Timestamp, rhs: Duration) -> Timestamp:
       ...

The operation is not owned by the left operand. Lucid does not need reflected binary methods such as ``__radd__``, and does not use ``NotImplemented`` as an operator negotiation protocol.
