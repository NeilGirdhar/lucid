Multiple dispatch
====================

.. contents:: Table of contents
   :depth: 2
   :local:

The problem with left-operand dispatch
------------------------------------------

Ordinary method resolution dispatches on one type: the type of ``self``.
That is not enough for a binary operator, which has two operand types to
consider. Python's workaround is a protocol built from three separate
pieces: ``__add__`` tries to handle the right operand by type-checking it,
``NotImplemented`` signals "not my type" so the interpreter can retry with
the right operand's own ``__radd__``, and library authors are expected to
implement both methods, symmetrically, on every type that wants to
participate:

.. code-block:: python

   class X:
       def __add__(self, other):
           if isinstance(other, X):
               return ...
           return NotImplemented

       def __radd__(self, other):
           if isinstance(other, X):
               return ...
           return NotImplemented

This protocol is invisible to static analysis. A type checker sees a method
that accepts ``object`` and returns ``NotImplemented`` or a value — it has
no way to know which right-hand types actually work, so it accepts
combinations that fail at runtime and gives up on combinations that would
succeed:

.. code-block:: python

   x = X()
   3 + x             # accepted by type checkers, then TypeError at runtime
   reveal_type(x + x) # Any, even though the real return type is known

Multiple-dispatch operators
------------------------------

Binary operators are relations between two operand types.

.. code-block:: python

   def dispatch __add__(lhs: X, rhs: Y) -> Z:
       ...

Then:

.. code-block:: python

   x + y

dispatches on both runtime types, and a type checker can read the same
dispatch definitions the runtime uses: it knows exactly which operand-type
pairs are supported and what each one returns, because that is all the
declaration says.

Dispatch requirements
--------------------------------------

Interfaces can require one element of a multiple-dispatch operation by
writing a ``dispatch`` member without a body, the same as any other
interface member:

.. code-block:: python

   interface Addable:
       def dispatch __add__(lhs: Self, rhs: Self) -> Self

A concrete implementation satisfies that requirement when the generic operation
has an applicable dispatch definition after substituting the concrete type for
``Self``.

Ambiguous dispatch is an error
------------------------------------

Two dispatch definitions can each be applicable to the same call without
either being more specific than the other:

.. code-block:: python

   def dispatch __add__(lhs: Cat, rhs: Animal) -> str:
       ...

   def dispatch __add__(lhs: Animal, rhs: Dog) -> str:
       ...

   Cat() + Dog()  # error: ambiguous between both definitions above

Python has no equivalent failure mode: ordinary method resolution always
picks exactly one method, so a case like this either silently favors
whichever definition happens to run first, or was never possible to express
in the first place. Lucid raises an error instead of silently favoring one
definition over the other, since guessing would not be so different from
guessing wrong.

Dispatch across projects and hierarchies
--------------------------------------------

Because a dispatch definition is not owned by either operand's type, two
projects can each contribute an overload for their own pair of types without
either project importing the other:

.. code-block:: python

   # in project p_x
   def dispatch __add__(lhs: X, rhs: Y) -> X:
       ...

   # in project p_y
   def dispatch __add__(lhs: Y, rhs: X) -> X:
       ...

Method resolution cannot do this: only ``X`` can define ``X.__add__``, so
combining ``X`` and ``Y`` requires one of the two projects to depend on the
other just to write the method. The same gap shows up inside one hierarchy.
Given a library with ``B`` and ``C`` both inheriting from ``A``, and ``A``
implementing addition with itself, adding a type ``D`` that also inherits
from ``A`` cannot specialize ``A + D`` through inheritance alone — ``D``
would just be treated as a plain ``A`` — without building a parallel class
hierarchy that already knows about ``D``. A dispatch definition for
``A``/``D`` needs no such hierarchy.

No reflected binary methods
--------------------------------

The matching dispatch can be supplied wherever dispatch definitions for that
generic operation are allowed. It is not owned by the left operand. Lucid does
not need reflected binary methods such as ``__radd__``.

No ``NotImplemented`` operator negotiation
------------------------------------------------

Lucid does not use ``NotImplemented`` as an operator negotiation protocol.
Dispatch applicability decides whether an operation is available.
