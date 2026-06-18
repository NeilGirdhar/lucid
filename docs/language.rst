The Lucid language
==================

Lucid is a Python-like language sketch that keeps Python easy to read and write
while removing the compatibility constraints that keep Python from adopting many
of its own best proposals.

Core principle:

    Keep Python's directness, make structure explicit, and choose the cleaner
    rule when compatibility no longer has to win.

Lucid borrows Python's readable surface syntax, Scala's habit of putting type
relationships on the abstraction itself, Julia's multiple-dispatch model,
dataclasses' transparent field-first object style, and the freedom to follow
Python Enhancement Proposals that Python could not adopt because of backward
compatibility. This document contains both the current rules and the reason for
each difference from Python.

.. contents:: Table of contents
   :depth: 4
   :local:

Design commitments
------------------

Python readability
~~~~~~~~~~~~~~~~~~

Lucid code should stay easy to read and write in the same way ordinary Python is
easy to read and write: indentation matters, definitions are direct, common
control flow is familiar, and simple programs do not need ceremony.

Scala-style type information
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Type relationships live where abstractions are defined. Generic parameters
carry definition-site variance with ``+K``, ``-K``, and ``=K``. Mutable,
read-only, and immutable views are visible in the type spelling with ``T``,
``T?``, and ``T!``.

Julia-style dynamic dispatch
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Operations can dispatch on all relevant runtime argument types. Binary
operators are generic multiple-dispatch operations, not methods owned by the
left operand.

Freedom to follow better PEPs
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Python often cannot adopt cleaner rules because existing programs depend on the
old behavior. Lucid treats those rejected or constrained improvements as design
space: when a PEP points toward a clearer language but compatibility blocks it,
Lucid can choose the clearer rule.

Exact construction
~~~~~~~~~~~~~~~~~~

Factories construct fully initialized instances of the exact class where the
factory is defined. Construction is not split across allocation, mutation, and
post-initialization hooks.

Visible public APIs
~~~~~~~~~~~~~~~~~~~

Definitions are private by default. Public module APIs are marked with
``export`` at the definition or re-export site.

Separate composition roles
~~~~~~~~~~~~~~~~~~~~~~~~~~

Classes own data and concrete behavior. Interfaces declare obligations. Traits
provide reusable method bodies. Trait conflicts are resolved explicitly by the
class.

Multiple-dispatch operators
~~~~~~~~~~~~~~~~~~~~~~~~~~~

Binary operators dispatch on both operands. They are not owned by the left
operand, and Lucid does not use reflected methods or ``NotImplemented`` as an
operator negotiation protocol.

Source basics
-------------

Python-like indentation
~~~~~~~~~~~~~~~~~~~~~~~

Lucid uses Python-like indentation and layout.

Names and scope
---------------

Ordinary binding
~~~~~~~~~~~~~~~~

Names bind through definitions, imports, assignments, and parameters.
Assignment binds names, updates declared fields, or delegates to explicit
assignment behavior such as setters and item assignment.

Black-hole assignment with ``_``
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Python treats ``_`` as an ordinary name by default, even though many codebases
use it by convention for ignored values. Lucid makes ``_`` a black-hole
assignment target: assigning to it discards the value instead of binding a name.

.. code-block:: python

   _ = compute()
   result, _ = split_pair()

``_`` is not an expression:

.. code-block:: python

   use(_)  # error

No ``global``
~~~~~~~~~~~~~

Python uses ``global`` to let assignment inside a function rebind a module
binding:

.. code-block:: python

   counter = 0

   def next_id() -> int:
       global counter
       counter += 1
       return counter

Lucid has no ``global`` declaration. Reading module bindings is allowed, but
assignment to a name inside a function is local to that function. Shared module
state is represented by an explicit mutable object:

.. code-block:: python

   counter = Cell(0)

   def next_id() -> int:
       counter.value += 1
       return counter.value

No ``nonlocal``
~~~~~~~~~~~~~~~

Python uses ``nonlocal`` to let assignment inside an inner function rebind a
name from an enclosing function:

.. code-block:: python

   def make_counter():
       count = 0

       def next():
           nonlocal count
           count += 1
           return count

       return next

Lucid has no ``nonlocal`` declaration. Reading enclosing bindings is allowed,
but assignment to a name is local to the current function. Closure state uses an
explicit mutable object:

.. code-block:: python

   def make_counter() -> Callable[[], int]:
       count = Cell(0)

       def next() -> int:
           count.value += 1
           return count.value

       return next

Object shape and attribute access
---------------------------------

All classes are dataclasses
~~~~~~~~~~~~~~~~~~~~~~~~~~~

Classes should make their stored state visible. Lucid takes the transparent
field-first style of dataclasses and makes it the normal object model rather
than a library convention layered over dynamic objects. Stored object state is
declared in the class body, and attribute access is structural and visible in
the class body, not programmable through dynamic lookup hooks.

Python instances usually have an open-ended ``__dict__`` unless a class uses
``__slots__``, dataclasses with options, extension types, or custom attribute
hooks. Lucid makes fixed shape the default: stored fields are declared directly
in the class body.

.. code-block:: python

   class Point:
       x: float
       y: float

No undeclared fields
~~~~~~~~~~~~~~~~~~~~

Assigning an undeclared field is an error. Lucid does not give instances an
implicit ``__dict__`` for arbitrary attributes. If a class needs dynamic keyed
data, declare that storage explicitly. Adding or removing class members after
definition is also not part of Lucid; class shape is closed. Assigning
``obj.__class__`` is not part of Lucid because object shape is fixed.

.. code-block:: python

   p = Point(1.0, 2.0)
   p.z = 3.0  # error: z is not a declared field

.. code-block:: python

   class Record:
       fields: dict[str, object]

       def get(self, name: str) -> object | None:
           return self.fields.get(name)

       def set(self, name: str, value: object):
           self.fields[name] = value

No descriptors
~~~~~~~~~~~~~~

Python descriptors can make attribute access programmable from many places.
Lucid does not include descriptors. Attribute behavior is visible through
fields, methods, getters, setters, class methods, factories, and class member
variables.

No ``property``
^^^^^^^^^^^^^^^

Python's ``property`` is descriptor-based. Lucid uses explicit getter and setter
member syntax instead.

.. code-block:: python

   class Circle:
       radius: float

       getter area(self) -> float:
           return pi * self.radius ** 2

       setter area(self, value: float):
           self.radius = sqrt(value / pi)

No ``__getattr__``
^^^^^^^^^^^^^^^^^^

Lucid does not include ``__getattr__`` fallback lookup. Missing attributes are
errors instead of calls into dynamic lookup code.

No ``__getattribute__``
^^^^^^^^^^^^^^^^^^^^^^^

Lucid does not include ``__getattribute__``. Attribute reads use visible members
from the class body and cannot be globally intercepted.

No ``__setattr__``
^^^^^^^^^^^^^^^^^^

Lucid does not include ``__setattr__``. Attribute assignment targets a declared
field or an explicit setter.

No ``__del__``
~~~~~~~~~~~~~~

Lucid does not include ``__del__`` finalizers. Cleanup should be explicit in the
API that owns the resource, instead of being hidden behind object destruction
timing.

Class members and construction
------------------------------

Closed member kinds
~~~~~~~~~~~~~~~~~~~

Class bodies contain a closed set of member kinds:

.. list-table::
   :header-rows: 1

   * - Member kind
     - Example
   * - instance field
     - ``x: int``
   * - method
     - ``def f(self): ...``
   * - class method
     - ``classmethod f(cls): ...``
   * - factory
     - ``factory f(cls): ...``
   * - getter
     - ``getter x(self) -> T: ...``
   * - setter
     - ``setter x(self, value: T): ...``
   * - class member variable
     - ``classvar count: int = 0``

Class member variables
~~~~~~~~~~~~~~~~~~~~~~

Class member variables are marked with ``classvar``. A plain annotated
assignment in the class body declares an instance field; if it has a value, that
value is the field's default.

.. code-block:: python

   class User:
       classvar count: int = 0
       name: str
       active: bool = True

This separates shared class state from stored instance fields.

No static methods for namespaced functions
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

If a function does not need access to an object or class, it stays a function.
Lucid does not use static methods as namespaced free functions.

Explicit class methods
~~~~~~~~~~~~~~~~~~~~~~

Class methods receive ``cls`` and are inherited as class-level behavior.

.. code-block:: python

   class User:
       classvar count: int = 0

       classmethod total_created(cls) -> int:
           return cls.count

Factory construction
~~~~~~~~~~~~~~~~~~~~

Python splits construction across ``__new__``, ``__init__``,
dataclass-generated initializers, ``__post_init__``, ``InitVar``, and field
options such as ``init=False`` or ``kw_only``. Lucid has one construction model:
factories return fully constructed objects through the factory-only
``construct`` keyword.

Factories receive ``cls``, but they construct the exact class where they are
defined. Calling a class calls its ``__init__`` factory. Calling a named factory
uses the factory name on the class.

.. code-block:: python

   class Point:
       x: float
       y: float

       factory __init__(cls, x: float, y: float):
           return construct(x, y)

       factory origin(cls):
           return construct(0.0, 0.0)

   p = Point(1.0, 2.0)
   origin = Point.origin()

If ``__init__`` is unspecified, Lucid generates the obvious field-based
constructor. This class:

.. code-block:: python

   class Point:
       x: float
       y: float

gets this default ``__init__`` factory:

.. code-block:: python

   factory __init__(cls, x: float, y: float):
       return construct(x, y)

``construct`` is a keyword, not an ordinary function. It can only appear inside
a factory. At runtime, a ``construct`` expression creates an instance of the
exact class whose factory is running, assigns the supplied values to that
class's declared fields in field order, and returns the fully initialized
object.

Factories are not inherited.

Every class has a generated ``replace`` factory. It works like Python's
``__replace__`` protocol: given an existing instance and any changed field
values, it constructs a new instance of the same exact class with unchanged
fields copied from the original object.

.. code-block:: python

   class Point:
       x: float
       y: float

   p = Point(1.0, 2.0)
   q = Point.replace(p, y=3.0)

Value semantics options
~~~~~~~~~~~~~~~~~~~~~~~

Classes can request common value semantics with class options. Immutability is
not a class option; it is represented by the ``T!`` view.

.. code-block:: python

   class Point(eq=True, order=True, hash=True):
       x: float
       y: float

Supported core options:

.. list-table::
   :header-rows: 1

   * - Option
     - Meaning
   * - ``eq``
     - equality is generated
   * - ``order``
     - ordering methods are generated
   * - ``hash``
     - hashing behavior is generated for immutable values

Representation is generated by default unless the class defines its own
representation method.

Interfaces, traits, and inheritance
-----------------------------------

Interfaces declare obligations
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Interfaces declare required APIs. They do not store data and do not provide
method bodies.

.. code-block:: python

   interface Sized:
       declare __len__(self) -> int

Interfaces, traits, and classes work together when a small required core can
support rich reusable behavior. A cache only has to say how to fetch, store, and
report freshness; traits can build higher-level behavior from those obligations:

.. code-block:: python

   interface Cache[=K, =V]:
       declare get(self, key: K) -> V | None
       declare put(self, key: K, value: V) -> None
       declare is_fresh(self, key: K) -> bool

   interface Sized:
       declare __len__(self) -> int

   trait CacheLookup[K, V](Cache[K, V]):
       def get_or_put(self, key: K, build: Callable[[], V]) -> V:
           cached = self.get(key)
           if cached is not None and self.is_fresh(key):
               return cached
           value = build()
           self.put(key, value)
           return value

   trait SizedCacheSummary(Sized):
       getter empty(self) -> bool:
           return self.__len__() == 0

   class MemoryCache[K, V](Cache[K, V], Sized, CacheLookup[K, V], SizedCacheSummary):
       entries: dict[K, V] = {:}
       fresh: set[K] = {}

       def get(self, key: K) -> V | None:
           return self.entries.get(key)

       def put(self, key: K, value: V) -> None:
           self.entries[key] = value
           self.fresh.add(key)

       def is_fresh(self, key: K) -> bool:
           return key in self.fresh

       def __len__(self) -> int:
           return len(self.entries)

   cache = MemoryCache[str, User]()
   user = cache.get_or_put("ada", lambda: load_user("ada"))

``declare`` as the abstraction marker
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

Interface members use ``declare``, not ``def``, because they specify a callable
requirement without implementing it. ``declare`` is also the abstraction marker:
Lucid does not need Python's ``@abstractmethod`` decorator.

Construction-time abstract checks
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

Every class is checked for unimplemented declared obligations before it can be
constructed. A class with any remaining ``declare`` member from an interface,
trait, or parent class is abstract for construction purposes, matching Python's
useful abstract-class instantiation check without using decorators. Lucid gets
that check without requiring classes to inherit from ``ABC`` or take on
``ABCMeta`` as a metaclass.

Traits provide reusable bodies
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Traits provide reusable method bodies. They do not declare fields.

.. code-block:: python

   interface Renderable:
       declare render(self) -> str

   trait DebugRenderable(Renderable):
       def debug(self) -> str:
           return "<debug " + self.render() + ">"

Explicit trait conflicts
^^^^^^^^^^^^^^^^^^^^^^^^

Trait conflicts are explicit. If two traits define the same method, the class
must resolve the collision.

Class inheritance and runtime hooks
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Class inheritance stays explicit and does not use Python's metaclass or MRO
customization hooks.

One concrete parent
^^^^^^^^^^^^^^^^^^^

A class may have at most one concrete parent. A class header can combine one
concrete parent, any number of interfaces, and any number of traits.

.. code-block:: python

   class FileLogger(LoggerBase, Closeable, Timestamped):
       path: str

No metaclasses
^^^^^^^^^^^^^^

Metaclasses are not part of Lucid. A restricted
``__init_subclass__(cls, **options)`` may remain for validation and registration
only, not for class rewriting.

No ``__prepare__``
^^^^^^^^^^^^^^^^^^

``__prepare__`` is not part of Lucid because metaclass machinery is absent.

No ``__mro_entries__``
^^^^^^^^^^^^^^^^^^^^^^

``__mro_entries__`` is not part of Lucid. Lucid does not allow MRO rewriting.

No programmable type checks
^^^^^^^^^^^^^^^^^^^^^^^^^^^

``__instancecheck__`` and ``__subclasscheck__`` are not part of Lucid. Type
relationships are not programmable through indirect hooks.

Types, mutability, and annotations
----------------------------------

Visible type contracts
~~~~~~~~~~~~~~~~~~~~~~

Lucid values have visible type contracts. Type annotations describe fields,
function parameters, return values, local variables, and interface
requirements.

.. code-block:: python

   name: str = "Ada"
   count: int = 3
   scores: list[float] = [10.0, 9.5]
   tags: set[str] = {"draft", "public"}
   metadata: dict[str, object] = {:}

Definition-site variance
~~~~~~~~~~~~~~~~~~~~~~~~

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
       declare put(self, value: K) -> None

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
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

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
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Annotations for concrete numeric types are exact: ``bool`` means ``bool``,
``int`` means ``int``, ``float`` means ``float``, and ``complex`` means
``complex``. Code that intentionally wants a broader numeric promise uses a
capability interface or an explicit union.

No numeric tower
^^^^^^^^^^^^^^^^

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
^^^^^^^^^^^^^^

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
   True + True                      # error: bool is not numeric
   True & flag                      # error: use boolean operators for logic

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
^^^^^^^^^^^^^

``int`` means integer, not ``int | bool`` and not every value that can be
converted with ``int(x)``. Integer operations are integer operations: indexing,
bitwise operations, shifts, and integer arithmetic are available for integer
values and for types that explicitly provide the relevant operation.

.. code-block:: python

   index: int = 3
   items[index]
   flags = READ | WRITE
   shifted = flags << 2

   index = True        # error: bool is not int
   index = "3"         # error: explicit conversion required
   index = int("3")

Exact ``float`` and float-like input
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

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
^^^^^^^^^^^^^^^^^

``complex`` means complex. Complex values support arithmetic but not ordering.
Code that accepts complex values should not accidentally promise ordering just
because other numeric types are orderable.

.. code-block:: python

   z: complex = 1 + 2j
   z < 3 + 4j  # error: complex is not orderable

Operation-specific numeric methods
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

Lucid does not assume that numeric-looking methods travel together. A type can
support conversion without supporting indexing, arithmetic without ordering, or
absolute value without rounding. APIs name the operation they need instead of
reaching for a broad tower class.

.. code-block:: python

   def repeat(count: SupportsIndex, action: Callable[[], None]) -> None:
       for _ in range(count.__index__()):
           action()

   def magnitude(x: SupportsAbs[float]) -> float:
       return abs(x)

   def rounded(x: SupportsRound[int]) -> int:
       return round(x)

Capability interfaces
^^^^^^^^^^^^^^^^^^^^^

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
       declare __round__(self, ndigits: int | None = None) -> K


No implicit cross-type numeric behavior
^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

Numeric equality and ordering are type-directed. Cross-type numeric equality,
cross-type hashing, and cross-type ordering exist only where explicitly defined.
``bool`` and ``complex`` are not orderable. Bitwise operators are integer-like
operations, not general numeric operations, and are not provided by ``bool``,
``float``, or ``complex``.

Strings and collections
-----------------------

No adjacent string literal concatenation
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Python concatenates adjacent string literals at compile time. Lucid rejects
adjacent string literals. Use an explicit concatenation operation when a string
is meant to be joined.

.. code-block:: python

   path = "/api/" "users"  # error
   path = "/api/" + "users"

Strings are not sequences
~~~~~~~~~~~~~~~~~~~~~~~~~

Python treats ``str`` as ``Sequence[str]`` for static typing. Lucid keeps
``str`` narrower: it is ``Container[str]`` and ``Sized``, but not
``Iterable[str]``, ``Collection[str]``, or ``Sequence[str]``. Strings do not
provide ``__iter__``. This catches accidental character-by-character behavior
where a function asks for general strings or general sequences:

.. code-block:: python

   def render_lines(lines: Iterable[str]) -> str:
       return "\n".join(lines)

   render_lines("hello")  # error: str is not Iterable[str]

Code that wants a character sequence asks for the ``chars`` property explicitly.
``chars`` returns a read-only sequence view, ``Sequence?[str]``:

.. code-block:: python

   text: str = "hello"

   for ch in text:          # error
       ...

   chars: Sequence?[str] = text.chars
   chars[0]

   for ch in text.chars:
       ...

String containment and length do not require string iteration:

.. code-block:: python

   "e" in text
   len(text)

``{}`` is a set and ``{:}`` is a dictionary
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Python uses ``{}`` for an empty dictionary and requires ``set()`` for an empty
set. Lucid uses ``{}`` for an empty set and ``{:}`` for an empty dictionary. A
braced literal with key-value pairs is also a dictionary.

Immutable collection literals
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

The immutable marker ``!`` before a collection literal constructs the immutable
variant. ``!{a, b}`` constructs a frozenset. ``!{a: b}`` constructs a
frozendict. ``!{}`` is an empty frozenset, and ``!{:}`` is an empty frozendict.

.. code-block:: python

   empty_set = {}
   empty_dict = {:}
   empty_frozenset = !{}
   empty_frozendict = !{:}
   names = {"Ada", "Grace"}
   scores = {"Ada": 10, "Grace": 9}
   immutable_names = !{"Ada", "Grace"}
   immutable_scores = !{"Ada": 10, "Grace": 9}

``skip`` in collection literals
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

``skip`` is not a value and cannot be returned, assigned, or passed through
ordinary expressions. It is valid only inside elidable collection entries and
call arguments. In list, tuple, and set literals, an entry that reaches
``skip`` is omitted:

.. code-block:: python

   [1, 2, 3 if False else skip, 4] == [1, 2, 4]

In dictionary literals, an entry is omitted if either the key expression or the
value expression reaches ``skip``:

.. code-block:: python

   {1: 2, 3: skip, skip: 6} == {1: 2}

Calls, indexing, and operators
------------------------------

``skip`` in calls
~~~~~~~~~~~~~~~~~

The ``skip`` keyword may appear as a positional or keyword argument. A
positional argument that reaches ``skip`` is omitted. A keyword argument whose
value reaches ``skip`` is omitted.

.. code-block:: python

   print(1, skip, 3, file=skip)

means:

.. code-block:: python

   print(1, 3)

Generator call expansion
~~~~~~~~~~~~~~~~~~~~~~~~

Python treats ``f(x for x in items)`` as a call with one generator object
argument. Lucid treats a generator expression written directly as a call
argument as argument expansion.

.. code-block:: python

   f(x for x in [x_1, x_2, x_3])

means:

.. code-block:: python

   f(x_1, x_2, x_3)

To pass an actual generator object, parenthesize the generator expression:

.. code-block:: python

   f((x for x in items))

Comma-separated indexing passes multiple arguments
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Python parses ``x[1, 2, 3]`` as a single tuple index. Lucid treats
comma-separated indexing as multiple index arguments.

.. code-block:: python

   x[1, 2, 3]

means:

.. code-block:: python

   x.__getitem__(1, 2, 3)

This removes a common ambiguity in Python APIs. In Python, ``grid[row, col]``
and ``grid[(row, col)]`` both arrive as one tuple argument, so a container must
guess whether the user meant a two-dimensional index or a single tuple key.
Lucid keeps those meanings separate:

.. code-block:: python

   grid[row, col]       # two index arguments
   grid[(row, col)]     # one tuple index

Assignment follows the same rule:

.. code-block:: python

   grid[row, col] = value      # grid.__setitem__(row, col, value)
   table[(row, col)] = value   # table.__setitem__((row, col), value)

This makes multidimensional containers, tuple-keyed mappings, type checking,
and error messages agree on the same syntax instead of relying on library-side
tuple parsing conventions.

Explicit tuple indexing
~~~~~~~~~~~~~~~~~~~~~~~

An explicit tuple index remains available by writing the tuple explicitly:

.. code-block:: python

   x[(1, 2, 3)]

Single indexing
~~~~~~~~~~~~~~~

Single indexing passes one index:

.. code-block:: python

   x[0]

means:

.. code-block:: python

   x.__getitem__(0)

No ``__getitem__`` iteration fallback
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

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

Lucid removes that sequence protocol. Indexing and iteration are separate
capabilities. A type is iterable only if it implements or inherits from
``Iterable``.

Unpacking precedence
~~~~~~~~~~~~~~~~~~~~

Lucid uses Python's operators and Python's order of operations unless this
document says otherwise. Unpacking binds tighter than binary operators:

.. code-block:: python

   *x + y

means:

.. code-block:: python

   (*x) + y

not:

.. code-block:: python

   *(x + y)

Multiple-dispatch operators
~~~~~~~~~~~~~~~~~~~~~~~~~~~

Binary operators are relations between two operand types.

.. code-block:: python

   def dispatch __add__(lhs: X, rhs: Y) -> Z:
       ...

Then:

.. code-block:: python

   x + y

dispatches on both runtime types.

``declare dispatch`` requirements
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Interfaces can require one element of a multiple-dispatch operation with
``declare dispatch``:

.. code-block:: python

   interface Addable:
       declare dispatch __add__(lhs: Self, rhs: Self) -> Self

A concrete implementation satisfies that requirement when the generic operation
has an applicable dispatch definition after substituting the concrete type for
``Self``.

No reflected binary methods
~~~~~~~~~~~~~~~~~~~~~~~~~~~

The matching dispatch can be supplied wherever dispatch definitions for that
generic operation are allowed. It is not owned by the left operand. Lucid does
not need reflected binary methods such as ``__radd__``.

No ``NotImplemented`` operator negotiation
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Lucid does not use ``NotImplemented`` as an operator negotiation protocol.
Dispatch applicability decides whether an operation is available.

Control flow and statements
---------------------------

Explicit boolean tests
~~~~~~~~~~~~~~~~~~~~~~

Boolean tests require ``bool`` or explicit truth behavior. This applies to
``if``, ``while``, and other conditional control-flow positions.

Explicit iteration
~~~~~~~~~~~~~~~~~~

``for`` loops require an explicitly iterable value. A type is iterable only if
it implements or inherits from ``Iterable``.

No loop ``else``
~~~~~~~~~~~~~~~~

Python loop ``else`` clauses run when a loop finishes without ``break``. Lucid
removes loop ``else``.

``if_broken`` loop clauses
~~~~~~~~~~~~~~~~~~~~~~~~~~

Lucid adds an optional ``if_broken`` clause for loops. ``if_broken`` is a
keyword. The ``if_broken`` suite runs if the loop exits by ``break``. Ordinary
fall-through code handles the case where a loop exits normally without
``break``.

.. code-block:: python

   def find_match(items: Iterable[Item]) -> Item | None:
       for item in items:
           if is_match(item):
               found = item
               break
       if_broken:
           return found
       return None

Nested ``if_broken`` clauses
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

In nested loops, an ``if_broken`` clause belongs to the loop immediately before
it. A ``break`` inside an inner loop can therefore be handled by the inner
loop's ``if_broken`` clause, and that clause can choose to break the outer loop:

.. code-block:: python

   for row in rows:
       while has_more(row):
           if matches(row.current):
               break
       if_broken:
           break

Unspecified simple statements
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

This sketch has not yet specified Lucid's full behavior for ``assert``,
``pass``, ``del``, ``raise``, ``break``, ``continue``, or type-alias statements.
The ``skip`` keyword is an expression-level elision marker, not a replacement
for the statement-level ``pass`` placeholder.

Modules and public APIs
-----------------------

Explicit exports
~~~~~~~~~~~~~~~~

Definitions are private by default. A definition becomes part of the public
module API when it is marked with ``export``.

.. code-block:: python

   export def parse_user(raw: str) -> User:
       ...

   export class User:
       name: str
       email: str

Only exported names are included in ``import *``.

Explicit re-exports
~~~~~~~~~~~~~~~~~~~

Packages can re-export public names:

.. code-block:: python

   export from .models import User
   export from .parsing import parse_user

This keeps the public API local to the definition or re-export site.

Lazy imports
~~~~~~~~~~~~

Python already has lazy-import building blocks, such as import hooks and lazy
loaders. Lucid makes laziness explicit in the source with ``lazy import`` and
``lazy from`` forms.

.. code-block:: python

   lazy import pandas as pd
   lazy from .reports import build_report

A lazy import binds the requested name immediately, but does not load the target
module until the name is first used. After the first use, the binding behaves
like an ordinary import.

Open specification work
-----------------------

Incomplete grammar
~~~~~~~~~~~~~~~~~~

Lucid does not yet have a complete grammar specification. The current docs
record the intended language shape and the syntax decisions made so far.

Unspecified interactive input
~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Interactive input is not yet specified.

Unspecified expression input
~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Expression input is not yet specified.

Keywords
--------

Preserved Python keywords
~~~~~~~~~~~~~~~~~~~~~~~~~

Lucid preserves Python's ordinary keyword vocabulary except where this document
explicitly replaces a Python behavior. The preserved Python keywords are:

.. code-block:: text

   False None True
   and as assert async await break class continue def del elif else except
   finally for from if import in is lambda not or pass raise return try while
   with yield

New Lucid keywords
~~~~~~~~~~~~~~~~~~

Lucid adds keywords for explicit module boundaries, construction, class member
kinds, abstraction, dispatch, and elision:

.. code-block:: text

   export lazy
   classmethod classvar factory construct
   getter setter
   interface trait declare
   dispatch
   if_broken
   skip
   _

Discarded Python keywords
~~~~~~~~~~~~~~~~~~~~~~~~~

Lucid discards Python's scope-rebinding declarations:

.. code-block:: text

   global nonlocal

``else`` is not discarded as a keyword. Lucid removes loop ``else`` clauses, but
``else`` remains available for the Python-like constructs that still use it.
