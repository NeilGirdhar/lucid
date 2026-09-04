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

Positional arguments precede keyword arguments
----------------------------------------------------

Python's rule looks simple — positional arguments come before keyword
arguments — but its grammar (PEP 448) treats star-unpacking as an
exception: an unpacked iterable still binds positionally, but is allowed to
appear in the source *after* a keyword argument, as long as nothing
double-star-unpacked has appeared yet. Source order and binding order can
disagree:

.. code-block:: python

   def f(a, b, c):
       ...

   f(c=3, *[1, 2])   # legal Python: a=1, b=2, c=3

``c=3`` is written first but binds to the last parameter; ``*[1, 2]``,
written last, supplies the first two. The call has to be read back to
front to see what it does.

Lucid closes the exception: positional arguments — plain and
star-unpacked alike — must precede every keyword argument, including
double-star unpacking, with no interleaving permitted.

.. code-block:: python

   f(*[1, 2], c=3)   # fine
   f(c=3, *[1, 2])   # error: positional argument follows keyword argument

Within the positional section, plain arguments and ``*``-unpacking can mix
freely in any relative order, since both bind purely by position, in the
order written. The same holds for named arguments and ``**``-unpacking
within the keyword section. The one rule that never bends is between the
two sections: positional, then keyword, always.

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
``Callable[(Item,), float]``, built without calling ``score`` at all, ready
to be called once ``sorted`` supplies the missing argument itself. No
import, no wrapper object, no separate type to teach a checker about.

Multiple holes fill left to right, matching the order they appear:

.. code-block:: python

   def combine(a: A, b: B, c: C) -> R:
       ...

   combine(_, y, _)   # Callable[(A, C), R]

A keyword hole stays keyword in the result:

.. code-block:: python

   combine(x, c=_)    # Callable[(C,), R]

The type of a partial application is ordinary generic inference, not a
special case: given ``f: Callable[(A, B), R]``, ``f(x, _)`` has type
``Callable[(B,), R]``, the same way substituting one type parameter of any
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

Anonymous class
---------------------

`Anonymous record shapes <collections.rst>`_ already give a value shape
without declaring a name for it: ``(x: int, y: int)``. A callable's own
parameter list uses the same grammar to fill a ``Callable``'s parameter
position directly, whenever a function has no variadic zoning of its
own:

.. code-block:: python

   Callable[(str, bool), R]

— the type of a function like ``greet(name: str, loud: bool)``.

A ``/`` marks the end of a positional-only zone; a bare ``*`` marks the
start of a keyword-only one — the same two markers an ordinary parameter
list already uses, and both stay within this same, plain shape:

.. code-block:: python

   (c: int, /, a: int, *, b: int)

- ``c: int, /`` — positional-only.
- ``a: int`` — ordinary: callable by position or by keyword.
- ``*`` — the keyword-only zone begins.
- ``b: int`` — keyword-only.

Every field here has a name, because every field here is one fixed,
individually addressable slot — whether or not a caller can ever use
that name. An ordinary or keyword-only field needs one because the
checker has to know it to verify a keyword call. A positional-only field
needs one for a different reason: callers can never use it, but the name
is what marks it as *one* slot. The order — positional-only, then
ordinary, then keyword-only — isn't a style rule: nothing can be
keyword-only until the positional zone is finished.

A signature with an unbounded, unnamed tail — the case a decorator's
generic forwarding needs, or a genuine overflow catch-all — extends this
grammar further; see
`Gathering arguments with Arguments and Parameters`_.

Gathering arguments with ``Arguments`` and ``Parameters``
------------------------------------------------------------------

Call arguments already spread two ways: ``*expr`` distributes a sequence
across positional slots, and ``**expr`` distributes a mapping across
keyword arguments — the same two operations
`Positional arguments precede keyword arguments`_ assumes throughout its
examples.

Python collects a function's own leftover arguments the same way, but
splits them into two separate, untyped catch-alls: ``*args`` for the
leftover positional values, ``**kwargs`` for the leftover keyword values.
The keyword half stayed essentially ``Any``-typed for most of Python's
history, and forwarding both together to another call — the entire point
of a decorator or a proxy — means carrying two separate names everywhere
they travel.

Lucid gathers leftover arguments into one typed value instead of two
untyped ones, and splits that into two classes depending on whether
there's a fixed, named prefix to keep separate from the open overflow:

.. code-block:: python

   class Arguments[Y, Z: dict[str, object]]:
       vpargs: list[Y]
       kwargs: Z

   class Parameters[X, Y, Z: dict[str, object]](Arguments[Y, Z]):
       pargs: X

Neither declaration needs a bespoke type-system primitive — both are
ordinary generic classes, ``Parameters`` inheriting from ``Arguments`` the
ordinary way (see
`Class inheritance and runtime hooks <type-specification.rst>`_).
``vpargs`` holds the leftover positional arguments, homogeneously typed;
``kwargs`` holds the leftover keyword arguments, typed as a full shape —
``{str: str}``, or a TypedDict shape when some of those keywords are
individually named — rather than a bare per-value type. Writing
``kwargs: str`` would say each value is a ``str``, not that ``kwargs``
itself is a mapping; the field has to be annotated with what it actually
holds. ``pargs`` holds whatever fixed, possibly zoned prefix a signature
has, typed as an `Anonymous class`_ itself when it needs its own
positional-only or ordinary zoning.

``Arguments`` on its own is for genuinely unnamed overflow: arguments
beyond anything a function declared, with no name available to give them,
and no fixed prefix left to keep separate. ``Parameters`` is for
describing a signature whole — the case a decorator's generic forwarding
needs, where there's no way to declare the fixed part as ordinary,
separately named parameters up front, because the wrapper doesn't know
what they will be (see `Decorators`_). ``Parameters`` is usable as a
bound the same way ``dict`` or ``Functor`` are, and a plain nominal
class's unzoned field shape satisfies it as the simplest case.

A signature's full shape, open-ended content included, can be written
inline by extending `Anonymous class`_'s grammar with its two remaining
zones: a bare, unnamed type followed by ``...`` for the variadic
positional tail, and ``_: type, ...`` for the variadic keyword one, after
the fixed prefix and its keyword-only zone respectively — the same
positional-before-keyword order every zone already follows:

.. code-block:: python

   (c: int, /, a: int, int, ..., *, b: int, _: int, ...)

- ``c: int, /, a: int`` — the fixed, possibly zoned prefix:
  `Anonymous class`_, exactly as written there.
- ``int, ...`` — variadic positional: zero or more further ``int``\ s,
  after every fixed position — nothing fixed can follow it, since it
  consumes every remaining positional slot. The trailing ``...`` is the
  same marker that opens a TypedDict shape
  (`TypedDict shapes in type position <collections.rst>`_), here meaning
  "and more of the type just written," not "anything."
- ``*, b: int`` — the keyword-only zone, `Anonymous class`_ again.
- ``_: int, ...`` — variadic keyword: zero or more further keyword
  arguments, each ``int``. ``_`` marks the slot as nameless the same way
  it already does for black-hole assignment
  (`Ordinary binding <source-and-names.rst>`__) — the key isn't fixed,
  only the value's type is.

This is inline sugar for a direct ``Parameters`` instantiation:

.. code-block:: python

   Parameters[(c: int, /, a: int), int, {"b": int, _: int, ...}]

Only the two variadic markers — a bare, unnamed run and ``_: type,
...`` — trigger this desugaring. ``/`` and a named keyword-only zone
don't: they stay inside the fixed prefix, an ordinary `Anonymous class`_
shape in its own right, which is why ``pargs``'s own type can have a
``/`` in it without becoming another ``Parameters`` — a fixed, zoned but
non-variadic prefix needs no aggregation, only genuinely open-ended
content does. ``query``'s full type follows the same rule, with an empty
prefix, since ``path`` is already named separately:

.. code-block:: python

   Callable[(Path, str, ..., *, _: str, ...), Response]

What isn't ordinary about either is how ``***`` treats ``pargs``,
``vpargs``, and ``kwargs`` specifically, on both the gathering and the
spreading side.

Gather
~~~~~~~~~

A function names its own catch-all with a leading ``***``, declaring it
directly as an ``Arguments`` value rather than as two separate parameters:

.. code-block:: python

   def query(path: Path, ***rest: Arguments[str, {str: str}]) -> Response:
       ...

``***`` can mark at most one parameter, and it must be last — there is
nothing left to catch after it. A bare ``*`` with no name still marks the
start of a keyword-only zone for ordinary named parameters, independent of
``***``: a function is never forced to route a keyword-only parameter
through the bundle just to give it one. ``query`` needs only ``Arguments``
here, since ``path`` is already an ordinary, separately named parameter —
there is no fixed prefix left for a ``pargs`` field to hold.

For an ordinary class (see `Anonymous class`_ and `Decorators`_),
``***name: SomeClass`` gathers one-to-one: each of ``SomeClass``'s fields
corresponds to exactly one of the function's own remaining parameters, in
whichever positional or keyword zone that class itself declares.
``pargs``, ``vpargs``, and ``kwargs`` are recognized field names that mean
something else: rather than expecting parameters literally named that,
the gather binds ``pargs`` to whatever fixed prefix remains, one-to-one,
the ordinary way, and aggregates however many leftover positional and
keyword arguments the caller actually supplied — an unbounded number —
into ``vpargs`` and ``kwargs``. That's the one place ``Arguments`` and
``Parameters`` aren't just ordinary classes: the aggregating behavior is
tied specifically to those field names.

Spread
~~~~~~~~~

The same sigil spreads a value back out at a call site by calling a
method every class has — ``__spread__`` — and using the ``Parameters``
instance it returns. Every class gets one generated for free, the same
way every class already gets a generated ``__init__`` and ``replace``
factory (`Factory construction <type-specification.rst>`_). By default,
``__spread__`` wraps the instance as its own fixed prefix, with nothing
variadic:

.. code-block:: python

   def __spread__(self: &Self) -> &Parameters[Self, Never, {:}]:
       return Parameters.from_fields(self)

which is why an ordinary class like `Decorators`_'s ``(name: str, loud:
bool)`` spreads as one argument per field: ``pargs`` is the instance
itself, ``vpargs``/``kwargs`` are empty.

``Arguments`` and ``Parameters`` override the default instead of using
it:

.. code-block:: python

   class Arguments[Y, Z: dict[str, object]]:
       vpargs: list[Y]
       kwargs: Z

       def __spread__(self: &Self) -> &Parameters[(), Y, Z]:
           return Parameters.from_arguments(self)

   class Parameters[X, Y, Z: dict[str, object]](Arguments[Y, Z]):
       pargs: X

       factory from_arguments(cls, args: &Arguments[Y, Z]) -> Parameters[(), Y, Z]:
           return construct(args.vpargs, args.kwargs, ())

       def __spread__(self: &Self) -> &Self:
           return self

``Arguments`` converts itself into the ``Parameters`` it structurally is
— the same shape, with an empty ``pargs`` — reusing
``Parameters.from_arguments`` rather than duplicating the logic.
``from_arguments`` is a named factory, so ``construct`` assigns
positionally in ``Parameters``'s own field order: inherited fields first
(``vpargs``, ``kwargs``, from ``Arguments``), then ``pargs``, the field
``Parameters`` adds — the same top-to-bottom order the class hierarchy
declares them in. ``pargs=()`` is fixed by this factory specifically, the
same way ``Point.origin()`` (`Factory construction
<type-specification.rst>`__) fixes ``x=0.0, y=0.0`` while still
constructing the exact class generically over whatever else varies.
``Parameters`` is already the target shape its own ``__spread__`` needs
to produce, so it just returns itself.

Nothing about either class is special at the type level anymore — what's
special is that they implement ``__spread__`` differently from the
generated default, exactly the way any class can override any other
method. Once ``***value`` has ``value.__spread__()``'s result, spreading
follows the zones in reverse: ``pargs``'s own fields spread first, in
their own order — positional-only, then ordinary — then ``vpargs``'s
elements, positionally, then ``kwargs``'s entries, as keywords. Never a
plain ``**`` mapping-spread on its own: a class with any positional-only
or variadic-positional field could never be satisfied that way.

.. code-block:: python

   query(***rest)

means exactly:

.. code-block:: python

   query(*rest.vpargs, **rest.kwargs)

— no ``pargs`` here, since a plain ``Arguments`` value doesn't have one.
A ``Parameters`` value spreads the same way with ``pargs`` added at the
front: for a signature matching
``(c: int, /, a: int, int, ..., *, b: int, _: int, ...)``, with
``pargs = (c=1, a=2)``, ``vpargs = [3, 4]``, and
``kwargs = {"b": 5, "x": 6}``,

.. code-block:: python

   f(***params)

means:

.. code-block:: python

   f(1, 2, 3, 4, b=5, x=6)

This degenerates to just ``pargs``'s own fields whenever
``vpargs``/``kwargs`` are empty — exactly `Decorators`_'s ``f(***args)``
for a function with no variadic tail of its own.

Neither field becomes a keyword argument in its own name — ``pargs``'s
fields, ``vpargs``'s elements, and ``kwargs``'s entries spread as
themselves, not as arguments literally called ``pargs``, ``vpargs``, or
``kwargs``. Gathering leftover arguments into a parameter and spreading
them back out to forward them are both a single token, using the same
``*`` and ``**`` spreads every other call already relies on, just
performed together and aimed at the recognized fields.

Because ``pargs``/``vpargs``/``kwargs`` alone read too close to a
variable that holds an entire bundle, the pieces are kept distinct by
convention: those three name the fields, and ``args`` is reserved for a
variable that holds a full ``Arguments`` or ``Parameters`` instance —
exactly how `Decorators`_ uses it.

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

   def timed[P: Parameters, R](f: Callable[P, R]) -> Callable[P, R]:
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
``wrapper`` receive and forward exactly the arguments ``f`` accepts.

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

   def lru_cache[P: Parameters, R](f: Callable[P, R], *, maxsize: int) -> Callable[P, R]:
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

