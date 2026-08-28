Types, mutability, and annotations
====================================

.. contents:: Table of contents
   :depth: 3
   :local:

Visible type contracts
-------------------------

Lucid's annotation syntax is Python's, unchanged: a colon after a name
annotates a field, a parameter, or a local variable, and ``->`` annotates a
function's return value. Interface requirements are annotated the same way
(see `Modern type specification <type-specification.rst>`_).

.. code-block:: python

   class User:
       name: str
       tags: set[str] = {"draft"}

   def greet(user: User, times: int = 1) -> str:
       lines: list[str] = [f"Hello, {user.name}"] * times
       return "\n".join(lines)

Type expressions and the ``type`` keyword
---------------------------------------------

Wherever a type is expected — variable, parameter, and return annotations,
generic parameter lists, ``declare`` signatures — Lucid parses a *type
expression* rather than an ordinary expression. Most syntax means the same
thing in both grammars (``dict[str, int]``, ``T?``, ``T!``, and
``Producer[+K]`` all evaluate identically either way), but a type expression
can use forms that mean something else, or nothing at all, as an ordinary
expression — for example the TypedDict shape literal in
`Strings and collections <collections.rst>`_.

The ``type`` keyword crosses between the two grammars, in the two directions
that matter:

``type Name = <type expression>``
    A type alias statement. The right-hand side is parsed as a type
    expression, and ``Name`` becomes usable in future type positions exactly
    as if the aliased expression had been written inline there.

``type <type expression>``
    A prefix operator usable inside an ordinary expression. It parses its
    operand as a type expression and evaluates it to a first-class *type
    form*: an ordinary value, usable wherever ordinary values are, for example
    when passing a type to a metaprogramming function.

.. code-block:: python

   type Shape = InferenceModel![str]
   value: Shape = freeze(model)

   form = type list[str]                                # an ordinary value: a reified type
   handlers = {"json": JSONHandler, "xml": XMLHandler}   # an ordinary dict, not a type

An ordinary assignment such as ``Shape = {"name": str, "year": int}``, without
``type``, parses its right-hand side as an ordinary expression: it produces a
plain dict whose values happen to be type objects, it does not register
``Shape`` as a type alias, and its ``{}`` does not get TypedDict-shape
parsing. Which grammar applies is always visible at the point where a name is
bound, rather than depending on where the name is used later.

Definition-site variance
----------------------------

Lucid writes variance on the generic parameter where it is declared: ``+K``
for covariant, ``-K`` for contravariant, and ``=K`` for invariant. If a
parameter is written without a variance marker, the checker warns, infers
the narrowest valid variance, and offers an autofix — keeping most of the
convenience of inferred variance during drafting, while still requiring the
marker to be written into the source, and preserved or intentionally
changed on future edits, before the API is accepted.

.. code-block:: python

   interface Producer[+K]:
       declare get(self) -> K

   interface Consumer[-K]:
       declare put(self, value: K) -> none

   class Cell[=K]:
       value: K

Python's generic variance is often hidden in library declarations or stubs,
inferred from the current member set rather than written down. Lucid puts
variance on the definition instead because variance is part of the public
contract: if a checker infers it from the current members, an ordinary edit
to an interface can silently change assignability for downstream code —
adding a method that consumes ``K`` can turn an inferred covariant interface
into an invariant one, breaking users who never touched their code. Writing
the marker up front makes the author choose the intended contract, rather
than letting it shift underneath callers as the interface evolves.

Mutable, read-only, and immutable views
-------------------------------------------

Lucid makes mutability part of the type spelling. Mutable and immutable
variants of the same abstraction are declared as one type family, spelled
with a marker on the short, unqualified name:

- ``T`` — mutable, the default. Code can read and write it.
- ``T?`` — read-only view. Code can observe it but cannot mutate it, and
  cannot rely on it being permanently immutable.
- ``T!`` — immutable. Code can rely on stability for operations such as
  hashing, memoization, and persistent sharing.

.. code-block:: python

   working: InferenceModel[str] = InferenceModel(weights, metadata, {:})
   stable: InferenceModel![str] = freeze(working)
   view: InferenceModel?[str] = working

Python's ``collections.abc`` models mutability as a single inheritance
chain: ``MutableMapping`` is a subclass of ``Mapping``. That gets the
mutable-to-read-only direction right, but leaves no sound place for genuine
immutability. ``Mapping`` doesn't even promise "no mutation methods on this
object" — it only hides mutation methods from the type checker's view of a
reference. The underlying object keeps whatever mutation methods its real
class has, and anyone else holding a reference to it can still call them:

.. code-block:: python

   from collections.abc import Mapping

   underlying = {"a": 1}
   view: Mapping[str, int] = underlying
   underlying["a"] = 2
   view["a"]  # 2 -- "read-only", but not stable

Python has no ABC for the stronger guarantee, and adding one to the chain
would not work: a mutable mapping cannot be a subtype of an immutable one
(mutation would break the promise), and an immutable mapping cannot be a
subtype of a mutable one (nothing could ever be written to it). Mutable and
immutable are incomparable, not a chain, so Lucid puts them side by side as
siblings under the read-only view instead of trying to line them up:

.. code-block:: text

   InferenceModel[K]  <: InferenceModel?[K]
   InferenceModel![K] <: InferenceModel?[K]

Read-only view
~~~~~~~~~~~~~~~~

As a function parameter
^^^^^^^^^^^^^^^^^^^^^^^^^^

Without a distinct read-only view, a parameter type is stuck between two bad
options. Make it invariant, and a function that only reads ``Animal``\ s
can't accept a ``list[Cat]`` argument even though reading is always safe.
Make it covariant instead, and nothing stops the function from writing a
``Dog`` into what is actually the caller's ``list[Cat]``, corrupting it.
``T?`` escapes that dilemma: it is the natural type for a parameter that
only reads its argument, and because both ``T`` and ``T!`` are subtypes of
``T?``, a single ``T?``-typed parameter accepts a mutable value, an
immutable value, or another read-only view, with no conversion at the call
site — while the callee gets a compile-time guarantee that it cannot mutate
an object it does not own:

.. code-block:: python

   def report(model: InferenceModel?[str]) -> str:
       return f"{model.label_count} labels"

   report(working)  # mutable
   report(stable)   # immutable
   report(view)      # already a read-only view

Safe covariance
^^^^^^^^^^^^^^^^^^

Variance is computed separately for each view. Mutable types are usually
invariant because they both produce and consume their type parameters, but
read-only and immutable views can often be covariant — this is exactly the
covariance the parameter dilemma above needed, made sound because the view
itself blocks writes. The same pattern applies to any type family with these
three views: the mutable variant is typically invariant, while the
read-only and immutable views can each be declared with the narrowest
variance their own operations support:

.. code-block:: text

   InferenceModel[=K]
   InferenceModel?[+K]
   InferenceModel![+K]

Read-only dictionaries
^^^^^^^^^^^^^^^^^^^^^^^^

This avoids the old split between mutable dictionaries and read-only mapping
interfaces. A mutable ``dict[str, Cat]`` should not be usable as a
``dict[str, Animal]`` because the receiver could write a ``Dog`` into it. But a
read-only view can safely widen the produced value type:

.. code-block:: python

   cats: dict[str, Cat] = {:}
   animals: dict?[str, Animal] = cats

   animal = animals["ada"]
   animals["turing"] = Dog()  # error: read-only view

Python's ``Mapping`` does not fully solve this. It is a separate abstraction
from ``dict``, and its key parameter is still invariant because the mapping API
both accepts keys for lookup and produces keys through views such as
``keys()``. Library authors still have to choose a different name and API
surface to ask for read-only dictionary access, and they only get the variance
that ``Mapping`` happened to declare.

Lucid keeps these as views of the same collection abstraction and computes
variance from each view's actual operations. Mutable ``dict`` stays invariant.
A full read-only dictionary view such as ``dict?[K, +V]`` is covariant in the
value type while keeping the key type invariant if the view both consumes and
produces keys. A narrower view that only produces keys or values can expose
different variance. Code does not need a separate ``Mapping`` type just to ask
for a read-only dictionary-shaped view.

Immutable view
~~~~~~~~~~~~~~~~

``T!`` is for code that needs to rely on stability, not just observe a
snapshot of it. An immutable value can be hashed and used as a dict key or
set member, memoized safely since a cached result can never go stale, and
shared freely across threads, caches, and closures without defensive
copying — nothing holding a ``T!`` can ever see it change underneath it.

.. code-block:: python

   cache: dict[InferenceModel![str], float] = {:}
   cache[stable] = evaluate(stable)

A mutable value becomes a ``T!`` through ``freeze``, which takes a ``T`` and
returns the immutable view.

.. code-block:: python

   stable: InferenceModel![str] = freeze(working)

Exact annotations and numeric capabilities
----------------------------------------------

Annotations for concrete numeric types are exact: ``bool`` means ``bool``,
``int`` means ``int``, ``float`` means ``float``, and ``complex`` means
``complex``. Code that intentionally wants a broader numeric promise uses a
capability interface or an explicit union.

No numeric tower
~~~~~~~~~~~~~~~~~~

Lucid does not have a numeric tower. Numeric types do not inherit from abstract
numeric base classes such as ``Integral``, ``Real``, or ``Complex``.

Python's numeric tower tries to describe numbers as a single mathematical
hierarchy, but practical APIs usually need narrower promises. An API that needs
an exact index wants ``__index__``, not every value that can be converted with
``int(x)``. An API that can add and multiply values may not support ordering,
bitwise operations, hashing, or lossless conversion. ``float`` and ``complex``
are especially awkward in a tower: ``complex`` supports arithmetic but not
ordering, while ``float`` accepts many integer-like values at runtime without
making every integer an appropriate value for a floating-point API.

The tower also makes annotations less literal. If ``Real`` is used because
``float`` feels too narrow, the API may accidentally accept integers, booleans,
fractions, decimals, or third-party numeric objects even when the implementation
only works for a smaller operation set. Lucid replaces the tower with exact
concrete annotations and small structural capability interfaces.

Exact ``bool``
~~~~~~~~~~~~~~~~

``bool`` is a distinct logical type, not a numeric subtype.

Python lets boolean values leak into numeric code because ``bool`` is a subtype
of ``int``. Lucid rejects those cases so flags cannot silently become counts,
indexes, or bit masks. The numeric and bitwise operators ``+``, ``-``, ``*``,
``/``, ``%``, ``|``, ``&``, and ``^`` are invalid for boolean operands.

.. code-block:: python

   retries: int = is_retry          # error: bool is not int
   total = completed + failed       # error if both names are bool flags
   page = pages[is_admin]           # error: bool is not an index
   mask = can_read | can_write      # error: use boolean operators for flags
   ratio: float = is_ready          # error: bool is not a numeric value
   true + true                      # error: bool is not numeric
   true & flag                      # error: use boolean operators for logic

Write the conversion when the numeric interpretation is intentional:

.. code-block:: python

   retry_count = int(is_retry)
   total = int(completed) + int(failed)
   mask = int(can_read) | (int(can_write) << 1)

Python truthiness falls back through ``__bool__``, ``__len__``, and built-in
emptiness rules. Lucid conditionals require a boolean value or an explicit
boolean protocol. Length does not imply truth, and values are not automatically
truthy just because they are nonzero, non-empty, or non-null.

.. code-block:: python

   if ready:
       run()

   if 1:        # error
   if "hello":  # error unless str explicitly implements truth behavior
   if items:    # error unless the type explicitly implements truth behavior

Types that want truth behavior define ``__bool__``.

.. code-block:: python

   interface Truthy:
       declare __bool__(self) -> bool

A sized type can opt in explicitly:

.. code-block:: python

   interface Sized:
       declare __len__(self) -> int

   trait SizedTruthy(Sized, Truthy):
       def __bool__(self) -> bool:
           return self.__len__() != 0

Exact ``int``
~~~~~~~~~~~~~~~

``int`` means integer, not ``int | bool`` and not every value that can be
converted with ``int(x)``. Integer operations are integer operations: indexing,
bitwise operations, shifts, and integer arithmetic are available for integer
values and for types that explicitly provide the relevant operation.

.. code-block:: python

   index: int = 3
   items[index]
   flags = READ | WRITE
   shifted = flags << 2

   index = true        # error: bool is not int
   index = "3"         # error: explicit conversion required
   index = int("3")

Exact ``float`` and float-like input
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Code that wants "anything I can convert to a float" asks for
``SupportsFloat``. ``int`` can satisfy ``SupportsFloat`` because it provides
explicit float conversion; that does not make ``int`` a subtype or view of
``float``.

.. code-block:: python

   def mean(xs: Iterable[SupportsFloat]) -> float:
       total = 0.0
       count = 0
       for x in xs:
           total += float(x)
           count += 1
       return total / count

   mean([1, 2.5, Decimal("3.5")])

By contrast, a ``float`` annotation means exactly ``float``:

.. code-block:: python

   scale: float = 2.0
   scale = 2              # error: int is not float
   scale = float(2)

``float?`` is the read-only view of ``float``. It does not mean
``int | float`` and does not turn integer values into floating-point values.
Scalar values are already immutable in practice, so ``float?`` is mainly useful
for uniform view syntax in generic APIs; it is not the way to spell
float-like input.

Exact ``complex``
~~~~~~~~~~~~~~~~~~~

``complex`` means complex. Complex values support arithmetic but not ordering.
Code that accepts complex values should not accidentally promise ordering just
because other numeric types are orderable.

.. code-block:: python

   z: complex = 1 + 2j
   z < 3 + 4j  # error: complex is not orderable

Operation-specific numeric methods
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Lucid does not assume that numeric-looking methods travel together. A type can
support conversion without supporting indexing, arithmetic without ordering, or
absolute value without rounding. APIs name the operation they need instead of
reaching for a broad tower class.

.. code-block:: python

   def repeat(count: SupportsIndex, action: Callable[[], none]) -> none:
       for _ in range(count.__index__()):
           action()

   def magnitude(x: SupportsAbs[float]) -> float:
       return abs(x)

   def rounded(x: SupportsRound[int]) -> int:
       return round(x)

Capability interfaces
~~~~~~~~~~~~~~~~~~~~~~~

The structural numeric capability interfaces are builtins and are always
available without import:

.. code-block:: python

   interface SupportsInt:
       declare __int__(self) -> int

   interface SupportsFloat:
       declare __float__(self) -> float

   interface SupportsComplex:
       declare __complex__(self) -> complex

   interface SupportsIndex:
       # Exact indexability, not just explicit int(x) conversion.
       declare __index__(self) -> int

   interface SupportsAbs[+K]:
       declare __abs__(self) -> K

   interface SupportsRound[+K]:
       declare __round__(self, ndigits: int | none = none) -> K


No implicit cross-type numeric behavior
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Numeric equality and ordering are type-directed. Cross-type numeric equality,
cross-type hashing, and cross-type ordering exist only where explicitly defined.
``bool`` and ``complex`` are not orderable. Bitwise operators are integer-like
operations, not general numeric operations, and are not provided by ``bool``,
``float``, or ``complex``.
