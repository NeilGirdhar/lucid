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
generic parameter lists, interface member signatures — Lucid parses a *type
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

Recursive type aliases
-----------------------------

A ``type`` alias may refer to itself within its own definition. Resolving a
type expression is deferred until the name is actually used, the same way
imports are lazy, so a self-reference inside the alias body is not a
forward-reference problem the way it would be for an ordinary, eagerly
evaluated assignment:

.. code-block:: python

   type PyTree[L] = L | list[PyTree[L]] | dict[str, PyTree[L]]

   leaves: PyTree[int] = [1, {"a": 2, "b": [3, 4]}, 5]

Recursion is what makes a type like ``PyTree`` expressible at all: at every
level, the shape is either a leaf, or one of the listed containers holding
that very same shape one level down.

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
       def get(self) -> K

   interface Consumer[-K]:
       def put(self, value: K) -> none

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

Higher-kinded parameters
-----------------------------

An ordinary generic parameter like ``K`` above stands for a type. Some
generic code needs a parameter that stands for a type *constructor*
instead — something that is not itself a complete type until it is applied
to one, the same way a plain function is not a value until it is called.
``F[_]`` declares that: a generic parameter that takes exactly one type
argument to become concrete, written with the same subscript syntax
ordinary type application already uses (``F[_, _]`` for a constructor that
takes two, and so on). The ``_`` is the same black-hole marker used
elsewhere for a binding that does not need a name (see
`Source basics and names <source-and-names.rst>`_) — here, in type
position, it means a type-argument slot the declaration does not need to
name either.

.. code-block:: python

   def tree_map[F[_]: Functor, A, B](tree: F[A], f: Callable[[A], B]) -> F[B]:
       return F.map(tree, f)

The same rule that governs variance markers governs ``F[_]``: it must be
written explicitly on a free-standing generic parameter like ``F`` above,
even though the checker could infer the arity from ``tree: F[A]`` during
drafting, because an unrelated later edit to ``tree_map``'s body could
otherwise silently change what ``F`` is required to be — exactly the danger
explicit variance markers already exist to rule out.

Existential types
-----------------------------

An ordinary interface can already be used directly as a type — ``count:
SupportsIndex`` already means "any type satisfying ``SupportsIndex``, caller's
choice, checker doesn't care which." That is existential quantification,
even though it is ordinary enough that nothing here has called it that
until now.

It stops working for a higher-kinded interface like ``Functor``, because
``Functor`` is not itself a complete type — the same reason bare ``list``
is not. What a recursive alias like ``PyTree`` wants to say is "a value of
type ``F[PyTree[L]]``, for *some* ``F`` satisfying ``Functor``," which
bundles two things an ordinary interface-as-type never had to: the
constraint, and what it is applied to. ``any`` spells that:

.. code-block:: python

   type PyTree[L] = L | any Functor[PyTree[L]]

``any Functor`` stands in for an unnamed ``F`` satisfying ``Functor``, the
same role ``Self`` plays inside ``Functor``'s own declaration, just
existentially bound instead of referring back to whatever class the
declaration is already inside. Subscripting it, ``any Functor[PyTree[L]]``,
applies that stand-in the same way ``F[PyTree[L]]`` would if ``F`` were a
real, named, universally-quantified parameter — the two are duals of each
other, one saying "works for every ``F``," the other "holds some particular
``F``, unspecified."

This costs nothing at runtime. Every value already carries its own concrete
class — the same fact `Multiple dispatch <dispatch.rst>`_ already relies
on — so a value of type ``any Functor[X]`` needs no extra representation;
whatever concretely `implement <type-specification.rst>`_\ s ``Functor`` is
already dispatchable the ordinary way. The only new thing ``any`` asks of
the checker is to accept any concrete ``F[X]`` under one annotation for a
higher-kinded interface, the same courtesy it already extends to ordinary
ones.

No ``Any`` escape hatch
-----------------------------

Python's ``Any`` turns off type checking for a value entirely: nothing about
it is verified, any method can be called on it, and it can be assigned to or
from anything with no proof required. Lucid has no equivalent, and does not
need one — there is no legacy untyped Lucid code for a new, fully-typed
language to interoperate with, which is the pressure that makes ``Any``
earn its place in a gradually-typed language like Python.

``object`` already covers "I don't know or care what this is" the sound
way: anything is assignable to an ``object``-typed slot, but only what
``object`` itself promises — equality, hashing, representation — can be
called on one, until it is narrowed back to something more specific. That
is exactly the check ``Any`` skips.

``any Interface`` (see `Existential types`_) looks similar and is not: it
erases which concrete type implements ``Interface``, not whether the value
is checked at all. A value of type ``any Functor[X]`` is fully verified
against everything ``Functor`` declares — nothing more, nothing less — the
same guarantee ``object`` gives, just narrower. Composing the two, ``any
object``, does not produce anything like ``Any`` either: it existentially
quantifies over the weakest possible bound, which adds nothing beyond what
``object`` already provides on its own.

The one place something ``Any``-shaped is actually needed is not inside
Lucid at all: a foreign, untyped value crossing the Python interop boundary
has to be accepted somehow, without static proof. That is a property of
whatever crosses that specific boundary, not a gap in the type system that
needs a general escape hatch to fill.

Python interop and ``trust``
-----------------------------------

A Python value crossing into Lucid with no further information is typed as
``object`` — nothing is assumed about it, the same as any other value whose
shape is genuinely unknown. That is not a new rule; it is the no-``Any``
principle above applied to values that happen to come from outside Lucid,
instead of values that happen to be under-specified inside it.

Getting anything more specific out of an ``object`` that actually came from
Python requires an explicit, unverified claim: ``trust``.

.. code-block:: python

   raw: object = some_python_function()
   items: list![int] = trust[list![int]](raw)

``trust`` asserts a type with no proof behind it — there is nothing on the
Python side for the checker to verify against — but the claim is visible,
written once, at the exact place it is made. That is the difference from
``Any``: ``Any`` lets a value be used as anything, anywhere, with no marker
recording that a leap was taken; ``trust`` requires writing down exactly
what is being trusted, and where, every time.

``trust`` only accepts an ``object``-typed operand. Python's ``typing.cast``
has no such restriction — it can assert any type in place of any other,
anywhere, purely between values that are already fully typed on the Python
side, with nothing foreign involved at all. Lucid has no equivalent
general-purpose ``cast``, on purpose: interfaces are nominal specifically so
that satisfying one is an explicit, checked act rather than an accidental
shape match, and an unrestricted cast would let any code route around that
check between two ordinary, already-sound Lucid values — ``trust[Dog](some_cat)``
between two well-typed Lucid values is not filling a real gap, it is
punching a hole where the checker already had real information. Restricting
``trust`` to ``object`` operands makes that impossible by construction: it
only ever gets to speak where the checker had nothing to say in the first
place, which is exactly and only the Python interop boundary.

Calling ``trust`` at every use site does not scale to a whole library.
Attaching a claim once, at the import, the way a ``.pyi`` stub does for
Python's own type checkers, is the natural next step — where such a stub
would live, and how it interacts with lazy imports, is not yet decided.

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
concrete annotations and small, focused capability interfaces.

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
       def __bool__(self) -> bool

A sized type can opt in explicitly:

.. code-block:: python

   interface Sized:
       def __len__(self) -> int

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

The numeric capability interfaces are builtins and are always available
without import. They are ordinary nominal interfaces, not structural ones:
a type satisfies ``SupportsInt`` because its class header explicitly declares
it, the same way any user-defined class declares any other interface —
``int`` itself explicitly inherits from ``SupportsInt`` (and the other
capability interfaces it satisfies), rather than qualifying merely by
happening to define a matching method.

.. code-block:: python

   interface SupportsInt:
       def __int__(self) -> int

   interface SupportsFloat:
       def __float__(self) -> float

   interface SupportsComplex:
       def __complex__(self) -> complex

   interface SupportsIndex:
       # Exact indexability, not just explicit int(x) conversion.
       def __index__(self) -> int

   interface SupportsAbs[+K]:
       def __abs__(self) -> K

   interface SupportsRound[+K]:
       def __round__(self, ndigits: int | none = none) -> K

   class int(SupportsInt, SupportsFloat, SupportsIndex):
       ...


No implicit cross-type numeric behavior
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Numeric equality and ordering are type-directed. Cross-type numeric equality,
cross-type hashing, and cross-type ordering exist only where explicitly defined.
``bool`` and ``complex`` are not orderable. Bitwise operators are integer-like
operations, not general numeric operations, and are not provided by ``bool``,
``float``, or ``complex``.
