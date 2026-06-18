3. Data model
=============

3.1. Objects, values, and types
-------------------------------

Lucid values have visible type contracts. Type annotations describe fields, function parameters, return values, local variables, and interface requirements.

.. code-block:: python

   name: str = "Ada"
   count: int = 3
   scores: list[float] = [10.0, 9.5]
   tags: set[str] = {"draft", "public"}
   metadata: dict[str, object] = {:}

When a value is intentionally dynamic, the type should say so. Open-ended per-object data belongs in an explicit dictionary field:

.. code-block:: python

   class DynamicThing:
       attrs: dict[str, object]

Lucid avoids implicit object structure. A dictionary is the visible way to say "this data has dynamic keys."

3.2. Boolean values and numeric protocols
-----------------------------------------

``bool`` is a distinct logical type, not a numeric subtype.

Lucid does not have a numeric tower. Numeric types do not inherit from abstract
numeric base classes such as ``Integral``, ``Real``, or ``Complex``.
Annotations for concrete numeric types are exact: ``int`` means ``int``,
``float`` means ``float``, and ``complex`` means ``complex``. Code that
intentionally wants more than one concrete numeric type must say so explicitly:

.. code-block:: python

   count: int = 1
   ready: bool = True
   mixed: int | bool = ready
   real: int | float = count
   scalar: int | float | complex = 1.0

   count = ready  # error
   real_float: float = count  # error
   complex_value: complex = real_float  # error
   count = int(ready)

This keeps annotations literal: ``int`` means integer, not ``int | bool``;
``float`` does not mean ``int | float``; ``complex`` does not mean
``int | float | complex``.

Generic code that needs a numeric capability uses structural ``Supports...``
interfaces instead of numeric-tower base classes. These interfaces are builtins
and are always available without import:

.. code-block:: python

   interface SupportsInt:
       declare __int__(self) -> int

   interface SupportsFloat:
       declare __float__(self) -> float

   interface SupportsComplex:
       declare __complex__(self) -> complex

   interface SupportsIndex:
       declare __index__(self) -> int

   interface SupportsAbs[+K]:
       declare __abs__(self) -> K

   interface SupportsRound[+K]:
       declare __round__(self, ndigits: int | None = None) -> K

   interface IntLike(SupportsInt, SupportsIndex, SupportsAbs[int], SupportsRound[int]):
       declare dispatch __add__(lhs: Self, rhs: Self) -> Self
       declare dispatch __sub__(lhs: Self, rhs: Self) -> Self
       declare dispatch __mul__(lhs: Self, rhs: Self) -> Self
       declare dispatch __floordiv__(lhs: Self, rhs: Self) -> Self
       declare dispatch __truediv__(lhs: Self, rhs: Self) -> float
       declare dispatch __mod__(lhs: Self, rhs: Self) -> Self
       declare dispatch __pow__(lhs: Self, rhs: Self) -> Self
       declare dispatch __and__(lhs: Self, rhs: Self) -> Self
       declare dispatch __or__(lhs: Self, rhs: Self) -> Self
       declare dispatch __xor__(lhs: Self, rhs: Self) -> Self
       declare dispatch __lshift__(lhs: Self, rhs: Self) -> Self
       declare dispatch __rshift__(lhs: Self, rhs: Self) -> Self
       declare dispatch __lt__(lhs: Self, rhs: Self) -> bool
       declare dispatch __le__(lhs: Self, rhs: Self) -> bool
       declare dispatch __gt__(lhs: Self, rhs: Self) -> bool
       declare dispatch __ge__(lhs: Self, rhs: Self) -> bool

   interface FloatLike(SupportsFloat, SupportsAbs[float], SupportsRound[float]):
       declare dispatch __add__(lhs: Self, rhs: Self) -> Self
       declare dispatch __sub__(lhs: Self, rhs: Self) -> Self
       declare dispatch __mul__(lhs: Self, rhs: Self) -> Self
       declare dispatch __truediv__(lhs: Self, rhs: Self) -> Self
       declare dispatch __pow__(lhs: Self, rhs: Self) -> Self
       declare dispatch __lt__(lhs: Self, rhs: Self) -> bool
       declare dispatch __le__(lhs: Self, rhs: Self) -> bool
       declare dispatch __gt__(lhs: Self, rhs: Self) -> bool
       declare dispatch __ge__(lhs: Self, rhs: Self) -> bool

   interface ComplexLike(SupportsComplex, SupportsAbs[float]):
       declare dispatch __add__(lhs: Self, rhs: Self) -> Self
       declare dispatch __sub__(lhs: Self, rhs: Self) -> Self
       declare dispatch __mul__(lhs: Self, rhs: Self) -> Self
       declare dispatch __truediv__(lhs: Self, rhs: Self) -> Self
       declare dispatch __pow__(lhs: Self, rhs: Self) -> Self

These interfaces describe available operations without implying subtype
relationships among ``bool``, ``int``, ``float``, and ``complex``.
``SupportsIndex`` is separate from ``SupportsInt`` because exact indexability is
not the same as explicit integer conversion. ``SupportsInt`` permits explicit
conversion with ``int(x)``; it does not make a value acceptable where an ``int``
annotation is required.

Inside an interface, ``Self`` names the implementing type. A ``declare
dispatch`` member is not an instance method. It is an obligation that a matching
multiple-dispatch implementation exists for the named generic operation and
argument types. Satisfaction is checked against the generic operation's dispatch
table after substituting the concrete implementing type for ``Self``. A dispatch
definition satisfies the obligation if its signature is applicable to the
resulting argument types, including through parent classes or interfaces. The
matching dispatch definition does not need to be written on, or owned by, the
implementing type.

Numeric equality is type-directed. Cross-type numeric equality exists only where
an explicit equality operation is defined for those two operand types. Lucid
does not assume Python's ``1 == 1.0 == 1+0j`` rule or the matching cross-type
hash behavior.

3.3. Strings and character access
---------------------------------

``str`` is a sized container of strings, not a sequence of strings.

``str`` implements ``Container[str]`` and ``Sized``. It does not implement
``Iterable[str]``, ``Collection[str]``, or ``Sequence[str]``, and it does not
provide ``__iter__``. Code that wants a sequence view of a string's characters
uses ``chars()`` explicitly:

.. code-block:: python

   text: str = "hello"

   "e" in text
   len(text)
   text.chars()[0]

   for ch in text:          # error
       ...

   for ch in text.chars():
       ...

``chars()`` returns ``Sequence[str]``.

3.4. Classes and object shape
-----------------------------

Classes define concrete object types. Stored instance state is declared directly in the class body.

.. code-block:: python

   class Point:
       x: float
       y: float

Every class has a fixed shape. Assigning an undeclared field is an error.

.. code-block:: python

   p = Point(1.0, 2.0)
   p.z = 3.0  # error: z is not a declared field

There is no implicit instance dictionary. If a class needs dynamic attributes, declare that storage explicitly:

.. code-block:: python

   class Record:
       fields: dict[str, object]

Class member variables are written as assignments in the class body.

.. code-block:: python

   class User:
       count = 0
       name: str

This separates shared class state from stored instance fields.

3.5. Generic types and variance
-------------------------------

Generic type parameters are written on the definition. Variance is explicit:

.. code-block:: python

   interface Producer[+K]:
       declare get(self) -> K

   interface Consumer[-K]:
       declare put(self, value: K) -> None

   class Cell[=K]:
       value: K

``+K`` is covariant, ``-K`` is contravariant, and ``=K`` is invariant. If a parameter is written without a variance marker, the checker warns, infers the narrowest valid variance, and offers an autofix.

.. code-block:: python

   interface Cache[K]:
       declare get(self, key: str) -> K

3.6. Mutable, read-only, and immutable views
--------------------------------------------

Mutable and immutable variants of the same abstraction are declared as one type family. The short name is the ordinary mutable type.

.. code-block:: python

   class InferenceModel[=K]:
       weights: Tensor
       metadata: dict[str, object]
       cache: dict[str, Tensor]

       def warm(self, key: str, value: Tensor) -> None:
           self.cache[key] = value

       frozen:
           hash=True

At use sites, punctuation marks the less common views:

.. code-block:: python

   working: InferenceModel[str] = InferenceModel(weights, metadata, {:})
   stable: InferenceModel![str] = freeze(working)
   view: InferenceModel?[str] = working

``T`` is the mutable/default type. ``T?`` is the read-only view: code can observe it but cannot mutate it and cannot rely on it being permanently frozen. ``T!`` is the immutable type: code can rely on stability for operations such as hashing, memoization, and persistent sharing.

The variants are siblings under the read-only view, not an inheritance chain where mutable is a subtype of immutable:

.. code-block:: text

   InferenceModel[K]  <: InferenceModel?[K]
   InferenceModel![K] <: InferenceModel?[K]

Variance is computed separately for each view. Mutable types are usually invariant because they both produce and consume their type parameters. Read-only and immutable views can often be covariant:

.. code-block:: text

   InferenceModel[=K]
   InferenceModel?[+K]
   InferenceModel![+K]

3.7. Members and properties
---------------------------

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
     - ``count = 0``

Getters define computed readable attributes. Setters define assignment behavior. Setter-only attributes are valid.

.. code-block:: python

   class Circle:
       radius: float

       getter area(self) -> float:
           return pi * self.radius ** 2

       setter area(self, value: float):
           self.radius = sqrt(value / pi)

Attribute access is structural and visible in the class body. Lucid does not
include descriptors or dynamic attribute hooks such as ``__getattr__``,
``__getattribute__``, or ``__setattr__``. Missing attributes are errors, and
assignment to an attribute is valid only for declared fields or explicit
setters. If a type needs open-ended keyed data, model that data as an explicit
dictionary field.

3.8. Interfaces, traits, and inheritance
----------------------------------------

Interfaces declare required APIs. They do not store data and do not provide method bodies.

.. code-block:: python

   interface Sized:
       declare __len__(self) -> int

Interface members use ``declare``, not ``def``, because they specify a callable requirement without implementing it. ``declare`` is also the abstraction marker: Lucid does not need Python's ``@abstractmethod`` decorator.

Traits provide reusable method bodies. They do not declare fields.

.. code-block:: python

   interface Renderable:
       declare render(self) -> str

   trait DebugRenderable(Renderable):
       def debug(self) -> str:
           return "<debug " + self.render() + ">"

Classes implement interfaces and include traits explicitly.

.. code-block:: python

   class Buffer(Sized, Truthy, SizedTruthy):
       data: bytes

       def __len__(self) -> int:
           return len(self.data)

Every class is checked for unimplemented declared obligations before it can be constructed. A class with any remaining ``declare`` member from an interface, trait, or parent class is abstract for construction purposes, matching the instantiation check Python performs for ``abc.ABC`` classes with abstract methods.

Trait conflicts are explicit. If two traits define the same method, the class must resolve the collision.

A class may have at most one concrete parent. A class header can combine one concrete parent, any number of interfaces, and any number of traits.

.. code-block:: python

   class FileLogger(LoggerBase, Closeable, Timestamped):
       path: str

3.7. Value semantics
--------------------

Classes can request common value semantics with class options.

.. code-block:: python

   class Point(frozen=True, eq=True, order=True, hash=True):
       x: float
       y: float

Supported core options:

.. list-table::
   :header-rows: 1

   * - Option
     - Meaning
   * - ``frozen``
     - instances cannot be mutated
   * - ``eq``
     - equality is generated
   * - ``order``
     - ordering methods are generated
   * - ``hash``
     - hashing behavior is generated

Representation is generated by default unless the class defines its own representation method.
