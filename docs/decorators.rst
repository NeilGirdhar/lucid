Decorators
==========

.. contents:: Table of contents
   :depth: 2
   :local:

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
-----------------------

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

   def timed[P: Parameters, R](f: P -> R) -> P -> R:
       def wrapper(***args: P) -> R:
           start = now()
           result = f(***args)
           log(f.__name__, now() - start)
           return result
       return wrapper

   @timed
   def slow_query(id: int) -> Row:
       ...

   slow_query.__name__  # "slow_query" — guaranteed by @, not opted into

``P``, bounded by ``Parameters`` rather than a bespoke ParamSpec, is
inferred at each ``@timed`` application to whatever shape ``f``'s real
parameter list turns out to be — here, ``(id: int)`` — which is what lets
``wrapper`` receive and forward exactly the arguments ``f`` accepts. A
bare, unparenthesized name on a `function type's <types.rst>`_ left side,
like ``P`` here, stands for a whole parameter shape not yet known —
distinct from ``(A, B) -> R``'s parenthesized list of already-concrete
types.

There is nothing to forget: identity preservation is not a convention a
decorator can skip, it is what ``@`` means. A decorator that genuinely
wants to produce something with a different identity is free to — just not
through ``@``; plain function application (``f = dec(f)``) carries no such
guarantee.

Decorator factories need no partial application
----------------------------------------------------

A parameterized decorator in Python — ``@lru_cache(maxsize=128)`` — is
built from a function that returns a function that returns a function: one
level to take the configuration, one to take the function being decorated,
one to wrap the call. The middle level exists only to delay receiving the
decorated function, which is manual partial application done by hand.

Lucid's decorator factories are just ordinary functions, with the decorated
function as an ordinary parameter:

.. code-block:: python

   def lru_cache[P: Parameters, R](f: P -> R, *, maxsize: int) -> P -> R:
       ...

   @lru_cache(maxsize=128)
   def slow_query(id: int) -> Row:
       ...

``lru_cache(maxsize=128)`` is missing exactly one required parameter,
``f``, and it is shaped like the decoration target — a ``P -> R``
producing a ``P -> R``. At a ``@`` site, that missing parameter is
filled with the decorated function automatically, with no ``_`` needed:
unlike `Partial application with _ <calls.rst>`_ in an ordinary call, ``@`` already
knows exactly which argument is missing and what has to go there, so
there is nothing ambiguous left to mark. If a decorator factory leaves more
than one ``P -> R``-shaped parameter unfilled, which one is the
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

