Types, mutability, and annotations
====================================

.. contents:: Table of contents
   :depth: 3
   :local:

Visible type contracts
-------------------------

Lucid values have visible type contracts. Type annotations describe fields,
function parameters, return values, local variables, and interface
requirements.

.. code-block:: python

   name: str = "Ada"
   count: int = 3
   scores: list[float] = [10.0, 9.5]
   tags: set[str] = {"draft", "public"}
   metadata: dict[str, object] = {:}

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

Python's generic variance is often hidden in library declarations or stubs.
Lucid puts variance on the definition because variance is part of the public
contract. If a checker infers variance from the current member set, then an
ordinary edit to an interface can change assignability for downstream code. For
example, adding a method that consumes ``K`` can turn an inferred covariant
interface into an invariant one, breaking users who never touched their code.
Lucid makes the author choose the intended contract up front.

.. code-block:: python

   interface Producer[+K]:
       declare get(self) -> K

   interface Consumer[-K]:
       declare put(self, value: K) -> none

   class Cell[=K]:
       value: K

``+K`` is covariant, ``-K`` is contravariant, and ``=K`` is invariant. If a
parameter is written without a variance marker, the checker warns, infers the
narrowest valid variance, and offers an autofix. This keeps most of the
convenience of inferred variance while avoiding an inferred public contract:
during drafting, the checker can discover the right marker; before the API is
accepted, the marker is written into the source and future edits must preserve
or intentionally change it.

Mutable, read-only, and immutable views
-------------------------------------------

Lucid makes mutability part of the type spelling. Mutable and immutable
variants of the same abstraction are declared as one type family. The short name
is the ordinary mutable type.

.. code-block:: python

   working: InferenceModel[str] = InferenceModel(weights, metadata, {:})
   stable: InferenceModel![str] = freeze(working)
   view: InferenceModel?[str] = working

``T`` is the mutable/default type. ``T?`` is the read-only view: code can
observe it but cannot mutate it and cannot rely on it being permanently
immutable.
``T!`` is the immutable type: code can rely on stability for operations such as
hashing, memoization, and persistent sharing.

The variants are siblings under the read-only view, not an inheritance chain
where mutable is a subtype of immutable:

.. code-block:: text

   InferenceModel[K]  <: InferenceModel?[K]
   InferenceModel![K] <: InferenceModel?[K]

Variance is computed separately for each view. Mutable types are usually
invariant because they both produce and consume their type parameters.
Read-only and immutable views can often be covariant:

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

.. code-block:: text

   InferenceModel[=K]
   InferenceModel?[+K]
   InferenceModel![+K]

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
