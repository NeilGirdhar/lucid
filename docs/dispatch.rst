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

A trait can require one element of a multiple-dispatch operation by
writing a ``dispatch`` member without a body, the same as any other
trait obligation:

.. code-block:: python

   trait Addable:
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

Dispatch beyond operators
--------------------------------

Every example so far has been a binary operator, but dispatch is not
specific to operators — it is a general way to give an ordinary function a
growing, independently-checked set of cases, one per argument type. Walking
a nested structure built from unrelated container types is a natural fit:
each container gets its own case, and recursion resolves the next case by
whatever type shows up at that level:

.. code-block:: python

   def dispatch tree_map[A, B](tree: list[A], f: (A) -> B) -> list[B]:
       return [tree_map(item, f) for item in tree]

   def dispatch tree_map[X, A, B](tree: dict[X, A], f: (A) -> B) -> dict[X, B]:
       return {k: tree_map(v, f) for k, v in tree.items()}

   def dispatch tree_map[A, B](tree: A, f: (A) -> B) -> B:
       return f(tree)

Each case only has to be correct on its own — there is no single signature
that has to hold for every case at once, present and future, the way a
bounded generic parameter would require (see
`Higher-kinded traits <traits.rst>`_ for that alternative,
and when it is worth the extra cost). The leaf case's ``tree: A`` is fully
generic, not narrowed to some concrete leaf type, and that does not
conflict with the two cases above it: ``list[A]`` and ``dict[X, A]`` are
each strictly more specific than a bare ``A`` for any argument that
actually is a list or a dict, so this is an ordinary specificity-ordered
fallback, not the kind of tie `Ambiguous dispatch is an error`_ describes
— it only ever applies to whatever reaches it as neither a list nor a
dict. Recursion resolves the next case independently at each level, so
nothing here commits up front to what a "leaf" is the way ``PyTree``'s
declaration does.

A third party can add a ``tree_map(tree: SomeClass[A], ...)`` case for
their own container type without touching ``list``, ``dict``, or this code
at all — the same extensibility `Dispatch across projects and hierarchies`_
already described, applied to a plain function instead of an operator:

.. code-block:: python

   def dispatch tree_map[A, B](tree: SomeTree[A], f: (A) -> B) -> SomeTree[B]:
       return SomeTree(tree_map(tree.left, f), tree_map(tree.right, f))

``tree_reduce`` follows the same shape, folding instead of rebuilding:

.. code-block:: python

   def dispatch tree_reduce[A, B](tree: list[A], f: (B, A) -> B, init: B) -> B:
       acc = init
       for item in tree:
           acc = tree_reduce(item, f, acc)
       return acc

   def dispatch tree_reduce[X, A, B](tree: dict[X, A], f: (B, A) -> B, init: B) -> B:
       acc = init
       for v in tree.values():
           acc = tree_reduce(v, f, acc)
       return acc

   def dispatch tree_reduce[A, B](tree: A, f: (B, A) -> B, init: B) -> B:
       return f(init, tree)

If the set of container shapes is fixed and known instead of open to third
parties, `Exhaustive pattern matching <control-flow.rst>`_ is the better
fit: it checks that every shape is handled, which an open set of dispatch
cases cannot do, at the cost of not being extensible the way this version
is.

No ``@overload``
--------------------

Python's ``@overload`` fakes multiple signatures for one function: each
``@overload``-decorated stub has a body of ``...``, existing only for the
type checker, while a single, separately-written implementation
underneath does the real work for every case:

.. code-block:: python

   @overload
   def parse(s: str) -> int: ...
   @overload
   def parse(s: bytes) -> int: ...
   def parse(s):
       return int(s)

Nothing keeps the stubs and the real implementation in sync but the
author's own care, and a type checker resolves an ambiguous call by
picking the first overload that matches, in declaration order — silently
favoring whichever definition happens to come first, the same failure
mode `Ambiguous dispatch is an error`_ already rejects for ordinary
method resolution.

Lucid needs no separate mechanism for this: it is exactly
`Dispatch beyond operators`_, applied to a function with no shared body
across its cases:

.. code-block:: python

   def dispatch parse(s: str) -> int:
       return int(s)

   def dispatch parse(s: bytes) -> int:
       return int(s.decode())

Every case has a real body — nothing exists only to satisfy a checker —
and an overlap a type checker would resolve by declaration order is an
error here instead, the same as any other ambiguous dispatch. It is also
open the way an ``@overload`` cluster never is: a third party can add
``parse(s: SomeFormat) -> int`` later without touching this code, the
same extensibility `Dispatch across projects and hierarchies`_ already
described.

Applicability includes how many arguments a call passes, not just their
types — every example above happens to keep that fixed, but nothing
requires it. A call with two arguments is simply not applicable to a
dispatch definition with one parameter, the same way a call with a
``str`` argument is not applicable to a definition typed for ``bytes``;
different-arity definitions can never be ambiguous with each other, since
a given call is applicable to at most one arity to begin with.

.. code-block:: python

   def dispatch pop(self) -> T:
       ...

   def dispatch pop(self, i: int) -> T:
       ...

   def dispatch pop(self, i: int, j: int) -> list[T]:
       ...

Promotion
------------

A binary operator between two *different* numeric types — ``int32 +
float32`` — still looks like a job for one dispatch case per type pair.
For a family with even a handful of members, that is quadratic: every
pair of numeric types needs its own case, most of them following the
exact same rule ("convert both to the wider type, then add"), duplicated
once per pair instead of stated once.

The fix is to stop enumerating pairs and instead give each type exactly
one fact about itself: what it promotes to when combined with something
wider. ``Promotes`` demands nothing but that fact, declared as an
associated type rather than a method — a type relationship belongs at
the definition site, the same principle already behind definition-site
variance and mutability views, not something computed by running code:

.. code-block:: python

   trait Promotes:
       type Wider

   class float32(Promotes):
       type Wider = float64 | complex64

   class int32(Promotes):
       type Wider = int64 | float32

   class float64(Promotes):
       type Wider = complex128

   class complex128(Promotes):
       type Wider = Never

A type can widen in more than one direction — ``float32`` gains precision
toward ``float64`` or gains an imaginary part toward ``complex64`` —
because real promotion is a lattice, not a chain: ``Wider`` names a union
of immediate neighbors, not a single next step. ``Never``, already used
elsewhere for "provably no value," marks the top of the lattice.

``promote[A, B]`` is a parameterized type like any other — ``Array[A]``,
``list[A]``, ``PyTree[L]`` — living in the same type-expression grammar
every generic type already does. Nothing about *where* it lives is new;
what is new is *how* it resolves: given two ``Promotes`` types, to their
least upper bound, the unique type reachable by following ``Wider`` from
both, closest to both of them, rather than by plain substitution. A
generic arithmetic operator can then be written once, for the whole
family, instead of once per pair:

.. code-block:: python

   def dispatch __add__[A: Promotes, B: Promotes](lhs: A, rhs: B) -> promote[A, B]:
       common = type promote[A, B]
       return common(lhs) + common(rhs)

``-> promote[A, B]`` is the checker resolving that type; ``type
promote[A, B]`` inside the body is the same computation, reified into an
ordinary value the way `Type expressions and the type keyword <types.rst>`__
already lets any type expression become one, here to get the concrete
class ``common(lhs)`` needs to call. This case only ever fires when ``A``
and ``B`` differ — ``int64.__add__(int64, int64)`` is strictly more
specific, the same rule that already lets ``list[A]`` beat a bare ``A``.

``promote[A, B]`` itself is a `match type <types.rst>`_: walk upward from
``B``, testing at each step whether the current candidate is reachable
by walking upward from ``A``, and stop at the first one that is —
``B``'s own chain is visited narrowest first, so the first hit is the
least upper bound, not just some common ancestor:

.. code-block:: python

   type ReachableFrom[X: Promotes, From: Promotes] = match From:
       case X: X
       case _: match From.Wider:
           case Never: Never
           case W: ReachableFrom[X, W]

   type promote[A: Promotes, B: Promotes] = match ReachableFrom[B, A]:
       case Never: match B.Wider:
           case Never: error   # A and B share no common promotion target
           case NextB: promote[A, NextB]
       case found: found

``ReachableFrom`` is exactly what makes ``Wider``'s branching safe:
``float32``'s ``Wider`` is a union, so testing reachability through it
distributes over both branches, and a ``Never`` from a branch that misses
disappears against a hit from the branch that doesn't, the same way
``Never | X`` always collapses to ``X``. ``int32 + complex64`` resolves
to ``complex64`` directly this way — ``int32 → float32 → complex64`` is
already a path, so no promotion has to reach all the way to
``complex128`` for it. This assumes the graph a ``Promotes`` hierarchy
declares is acyclic with a genuine top, the same well-formedness Julia's
own ``promote_type`` quietly requires of its authors — a cycle in
``Wider`` would make ``promote`` search forever instead of erroring.
