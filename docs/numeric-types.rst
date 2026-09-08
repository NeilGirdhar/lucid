Numeric types
=============

.. contents:: Table of contents
   :depth: 3
   :local:

Annotations for concrete numeric types are exact: ``bool`` means ``bool``,
``int`` means ``int``, ``float`` means ``float``, and ``complex`` means
``complex``. Code that intentionally wants a broader numeric promise uses a
capability trait or an explicit union.

No numeric tower
------------------

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
concrete annotations and small, focused capability traits.

Exact ``bool``
----------------

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

   trait Truthy:
       def __bool__(self) -> bool

A sized type can opt in explicitly:

.. code-block:: python

   trait Sized:
       def __len__(self) -> int

   trait SizedTruthy(Sized, Truthy):
       def __bool__(self) -> bool:
           return self.__len__() != 0

Exact ``int``
---------------

``int`` means integer, not ``int | bool`` and not every value that can be
converted with ``int(x)``. Integer operations are integer operations: indexing,
bitwise operations, shifts, and integer arithmetic are available for integer
values and for types that explicitly provide the relevant operation.

.. code-block:: python

   index: int = 3
   items[index]
   flags = read | write
   shifted = flags << 2

   index = true        # error: bool is not int
   index = "3"         # error: explicit conversion required
   index = int("3")

Exact ``float`` and float-like input
--------------------------------------

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

``~float`` is the read-only view of ``float``. It does not mean
``int | float`` and does not turn integer values into floating-point values.
Scalar values are already immutable in practice, so ``~float`` is mainly useful
for uniform view syntax in generic APIs; it is not the way to spell
float-like input.

Infinity and NaN
-------------------

Python spells these two special ``float`` values inconsistently:
``math.inf`` and ``math.nan`` live in a separate module import, while
``float("inf")`` and ``float("nan")`` parse a string to reach the same
values a different way.

Lucid puts both directly on ``float``, as ordinary class member
variables — no import, no string parsing:

.. code-block:: python

   class float:
       classvar inf: float
       classvar nan: float

.. code-block:: python

   distance: float = float.inf
   result: float = float.nan

``float.nan`` is still IEEE 754 NaN and keeps the one property every
IEEE 754 float shares: ``float.nan != float.nan``. That is a fact about
the value, inherited from the standard ``float`` already follows, not a
Lucid-specific exception to ``Eq``'s usual reflexivity.

Infinity and NaN for ``int``
---------------------------------

``int`` is arbitrary-precision — it grows to whatever size a value
needs, with no fixed width to run out of. That already means adding
``int.inf``/``int.nan`` costs nothing a fixed-width integer would have
to pay: there is no bit pattern to reserve and no legitimate value to
give up, since the representation is free to carry an extra tag
alongside its digits, the same way it already carries a sign. A
reserved tag can never collide with a real integer — exactly the
property that lets ``float.nan``/``float.inf`` work safely, and the
one a fixed-width integer would not have.

.. code-block:: python

   class int:
       classvar inf: int
       classvar nan: int

Floor division and modulo by zero — the two ``int`` operations with no
defined answer today — produce them instead of raising:

.. code-block:: python

   5 // 0    # int.inf
   -5 // 0   # -int.inf
   0 // 0    # int.nan
   5 % 0     # int.nan

True division, ``/``, already promotes both operands to ``float``
before dividing (`Exact float and float-like input`_), so ``5 / 0``
was already ``float.inf``; this only fills in the two operators that
stay ``int``-typed and, until now, had no answer at all. ``int.inf``
and ``int.nan`` propagate through further arithmetic and compare the
same way their ``float`` counterparts do, ``int.nan != int.nan``
included — matching a contract callers already learned once, rather
than a second, subtly different one just for ``int``.

Because both are ordinary ``int`` values, not a separate wrapper or
sentinel type, they pass anywhere an ``int`` already does — a
``limit: int = int.inf`` default meaning "unlimited" reads the same
way ``float("inf")`` already gets used as a sentinel today, with no
special-casing needed at the call site.

``range``'s own ``stop`` parameter is a plain ``int``, not
``int | none``, for exactly this reason: an unbounded range is just
``range(start, int.inf, step)``, and every existing termination rule
already produces the right answer without a separate case for it.
Counting up needs nothing new — ``current < int.inf`` holds forever.
Counting down toward an unbounded upper stop terminates immediately,
correctly, from that same comparison:

.. code-block:: python

   range(0, int.inf, 1)     # counts up forever
   range(10, int.inf, -1)   # empty: 10 > int.inf is never true

``none`` cannot do either of those — it takes no part in a comparison
at all, so both directions would need their own branch to special-case
what "unbounded" means for that particular step's sign.

This replaces a raised ``ZeroDivisionError`` with an ordinary,
checkable value — the same trade `Errors: results and exceptions
<control-flow.rst>`_ makes everywhere else a recoverable outcome is
involved, later in this reading order, extended to the one place
integer arithmetic still had an unchecked exception instead of one.

Exact ``complex``
-------------------

``complex`` means complex. Complex values support arithmetic but not ordering.
Code that accepts complex values should not accidentally promise ordering just
because other numeric types are orderable.

.. code-block:: python

   z: complex = 1 + 2j
   z < 3 + 4j  # error: complex is not orderable

Operation-specific numeric methods
------------------------------------

Lucid does not assume that numeric-looking methods travel together. A type can
support conversion without supporting indexing, arithmetic without ordering, or
absolute value without rounding. APIs name the operation they need instead of
reaching for a broad tower class.

.. code-block:: python

   def repeat(count: SupportsIndex, action: () -> none) -> none:
       for _ in range(count.__index__()):
           action()

   def magnitude(x: SupportsAbs[float]) -> float:
       return abs(x)

   def rounded(x: SupportsRound[int]) -> int:
       return round(x)

Capability traits
-----------------------

The numeric capability traits are builtins and are always available
without import. They are ordinary nominal traits, not structural ones:
a type satisfies ``SupportsInt`` because its class header explicitly declares
it, the same way any user-defined class declares any other trait —
``int`` itself explicitly inherits from ``SupportsInt`` (and the other
capability traits it satisfies), rather than qualifying merely by
happening to define a matching method.

.. code-block:: python

   trait SupportsInt:
       def __int__(self) -> int

   trait SupportsFloat:
       def __float__(self) -> float

   trait SupportsComplex:
       def __complex__(self) -> complex

   trait SupportsIndex:
       # Exact indexability, not just explicit int(x) conversion.
       def __index__(self) -> int

   trait SupportsAbs[+K]:
       def __abs__(self) -> K

   trait SupportsRound[+K]:
       def __round__(self, ndigits: int | none = none) -> K

   class int(SupportsInt, SupportsFloat, SupportsIndex):
       ...


No implicit cross-type numeric behavior
-----------------------------------------

Numeric equality and ordering are type-directed. Cross-type numeric equality,
cross-type hashing, and cross-type ordering exist only where explicitly defined.
``bool`` and ``complex`` are not orderable. Bitwise operators are integer-like
operations, not general numeric operations, and are not provided by ``bool``,
``float``, or ``complex``.
