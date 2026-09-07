Lazy parameters
================

.. contents:: Table of contents
   :depth: 2
   :local:

The eager-call problem
--------------------------

An ordinary function call evaluates every argument before the call
happens, always. That is almost never a problem — until an argument is
expensive to compute and the function's whole point is that it usually
doesn't need it. ``assert condition, message`` is Python's clearest
example: ``message`` is often a formatted string, sometimes an expensive
one, but it only matters when ``condition`` is false. Python solves this
by making ``assert`` a statement instead of a function, with its own
special grammar that skips evaluating ``message`` unless the assertion
actually fails.

That specialness has a cost of its own: ``assert`` reads enough like a
function call that wrapping it in parentheses — to line arguments up
across lines, the way any other call is formatted — is a natural mistake:

.. code-block:: python

   assert (condition, "message")   # always true: a non-empty tuple

Lucid cannot reproduce this particular accident even today, before
anything in this document — ``(condition, "message")`` would have to be a
tuple to be silently truthy, and Lucid has no tuple type at all (see
`No tuple or namedtuple type <collections.rst>`_). But that only removes
one symptom. The underlying reason ``assert`` needed bespoke statement
grammar in the first place — an ordinary call cannot skip evaluating one
of its own arguments — is still there, and it blocks ``assert`` from
being what it should be: a plain function, called exactly like ``print``.

``lazy[T]``
---------------

A parameter written ``lazy[T]`` instead of ``T`` changes what happens at
the call site, not what the parameter looks like inside the function.
The caller writes an ordinary expression of type ``T``; the compiler
wraps it in a thunk instead of evaluating it before the call. Inside the
function body, the parameter is a plain ``T`` — referencing it forces the
thunk, computing the expression the first time and caching the result
for every reference after that:

.. code-block:: python

   def report(label: str, value: lazy[int]) -> none:
       print(label, value)   # forced here, once

   report("count", expensive_count())   # expensive_count() not called yet

Nothing about the call site marks ``value`` as lazy — ``report("count",
expensive_count())`` reads exactly like calling any other function with
two ordinary arguments. The laziness is a property of ``report``'s own
signature, not something a caller opts into per call.

``assert`` as an ordinary function
----------------------------------------

With ``lazy[T]``, ``assert`` needs no grammar of its own:

.. code-block:: python

   def assert(condition: bool, message: lazy[str]) -> none:
       if not condition:
           raise AssertionError(message)

   assert(x > 0, f"x must be positive, got {x}")

The f-string is never built when ``x > 0`` — the same guarantee Python's
``assert`` statement gives, without a statement to give it. Because
``assert`` is now an ordinary name rather than a keyword, it no longer
appears among `Preserved Python keywords <keywords.rst>`_.

Logging needs this more than ``assert`` does
--------------------------------------------------

Python's own workaround for expensive log messages is incomplete.
Deferred ``%``-style formatting —
``logger.debug("x: %s", expensive())`` — only defers building the final
string; ``expensive()`` itself is an ordinary call argument, and Python
evaluates it unconditionally, whether or not debug logging is even
enabled. The only fully correct pattern in Python today is an explicit
guard:

.. code-block:: python

   if logger.isEnabledFor(logging.DEBUG):
       logger.debug(f"x: {expensive()}")

which is exactly the boilerplate a caller has to remember, every time,
at every call site. A ``lazy[str]`` parameter fixes this once, in
``debug``'s own signature, instead of at every place that calls it:

.. code-block:: python

   class Logger:
       level: Level

       def debug(self, message: lazy[str]) -> none:
           if self.level <= Level.debug:
               self.emit(message)

   log.debug(f"x: {expensive()}")   # expensive() runs only if debug logging is active

Other lazy parameters
--------------------------

The same shape — compute this only if a cheap check says it is actually
needed — shows up wherever Python's eager calls force an unconditional
computation for what is meant to be a fallback:

.. code-block:: python

   def setdefault(self, key: K, default: lazy[V]) -> V:
       ...

   def min(items: Iterable[T], default: lazy[T]) -> T:
       ...

``dict.setdefault(key, expensive_default())`` in Python calls
``expensive_default()`` even when ``key`` is already present; ``min``
and ``max``'s ``default=`` argument, for the empty-iterable case, has the
same problem. ``warnings.warn`` and any precondition- or
postcondition-style contract check share ``assert``'s exact shape.
``lazy[T]`` fixes all of them the same way: once, in the signature, not
separately at every call site that happens to remember to guard itself.

None of this touches `Multiple dispatch <dispatch.rst>`_ or the
`anonymous class and Arguments/Parameters machinery
<parameters.rst>`_ that gathers a signature's leftover arguments —
``lazy[T]`` changes when one already-typed parameter's expression runs,
not how a signature is shaped or gathered.
