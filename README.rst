The Lucid language
==================

Lucid is a Python-like language sketch built around explicit structure, exact construction, and a smaller object model.

Core principle:

    Make structure explicit, make construction exact, and remove hidden object-model magic.

.. contents::
   :local:
   :depth: 2

Overview
--------

Lucid keeps Python's readable surface syntax, but makes object shape, public APIs, and construction rules visible in the program text.

A small Lucid program looks like this:

.. code-block:: python

   export class User(frozen=True):
       name: str
       email: str

       factory from_email(cls, raw: str):
           normalized = raw.lower()
           return construct(normalized.split("@")[0], normalized)

       getter domain(self) -> str:
           return self.email.split("@")[1]

This defines a public fixed-shape class with two stored fields, one factory, and one computed property.

Lucid's main commitments are:

* stored object state is declared in the class body
* construction returns fully built objects
* public module APIs are marked with ``export``
* interfaces declare required behavior
* traits provide reusable method bodies
* binary operators dispatch on both operands
* dynamic object-model hooks are not part of the language

Values and type annotations
---------------------------

Lucid uses Python-like expression syntax and type annotations.

.. code-block:: python

   name: str = "Ada"
   count: int = 3
   scores: list[float] = [10.0, 9.5]
   tags: set[str] = {"draft", "public"}
   metadata: dict[str, object] = {:}

Type annotations are part of the visible program contract. They describe fields, function parameters, return values, local variables, and interface requirements.

.. code-block:: python

   def distance(x: float, y: float) -> float:
       return sqrt(x * x + y * y)

When a value is intentionally dynamic, the type should say so. For example, open-ended per-object data belongs in an explicit dictionary field:

.. code-block:: python

   class DynamicThing:
       attrs: dict[str, object]

Lucid avoids implicit object structure. A dictionary is the visible way to say "this data has dynamic keys."

Collection literals keep empty sets and empty dictionaries distinct.

.. code-block:: python

   empty_set = {}
   empty_dict = {:}
   names = {"Ada", "Grace"}
   scores = {"Ada": 10, "Grace": 9}

``{}`` constructs a set. ``{:}`` constructs a dictionary. A braced literal with key-value pairs is also a dictionary.

Functions
---------

Functions are ordinary callable definitions.

.. code-block:: python

   def normalize_email(raw: str) -> str:
       return raw.strip().lower()

If a function does not need access to an object or class, it stays a function. Lucid does not use static methods as namespaced free functions.

.. code-block:: python

   def clamp(value: float, low: float, high: float) -> float:
       return min(max(value, low), high)

Methods are functions attached to classes, interfaces, or traits. Instance methods receive ``self``.

.. code-block:: python

   class Rectangle:
       width: float
       height: float

       def area(self) -> float:
           return self.width * self.height

Class methods receive ``cls`` and are inherited as class-level behavior.

.. code-block:: python

   class User:
       count = 0

       classmethod total_created(cls) -> int:
           return cls.count

Factories also receive ``cls``, but they are constructors for the exact class where they are defined.

Programs and modules
--------------------

Lucid modules contain ordinary definitions: functions, classes, interfaces, traits, and imports.

Definitions are private by default. A definition becomes part of the public module API when it is marked with ``export``.

.. code-block:: python

   export def parse_user(raw: str) -> User:
       ...

   export class User:
       name: str
       email: str

Only exported names are included in ``import *``.

Packages can re-export public names:

.. code-block:: python

   export from .models import User
   export from .parsing import parse_user

This keeps the public API local to the definition or re-export site. Lucid does not use a separate ``__all__`` list.

Imports can be marked lazy when a dependency is expensive or optional.

.. code-block:: python

   lazy import pandas as pd
   lazy from .reports import build_report

A lazy import binds the requested name immediately, but does not load the target module until the name is first used. After the first use, the binding behaves like an ordinary import.

Classes and fields
------------------

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

Object construction
-------------------

Calling a class constructs an object.

If no custom factory is present, Lucid generates the obvious field-based constructor.

.. code-block:: python

   class Point:
       x: float
       y: float

acts like:

.. code-block:: python

   factory __init__(cls, x: float, y: float):
       return construct(x, y)

``construct`` is a keyword, not an ordinary function. It can only appear inside a factory. At runtime, a ``construct`` expression creates an instance of the exact class whose factory is running, assigns the supplied values to that class's declared fields in field order, and returns the fully initialized object.

Custom construction is written with a factory:

.. code-block:: python

   class User:
       name: str
       email: str

       factory __init__(cls, name: str, raw_email: str):
           return construct(name, raw_email.lower())

``__init__`` is not a mutating post-allocation hook. It is a factory that returns the object.

Alternative constructors are factories too:

.. code-block:: python

   class User:
       name: str
       email: str

       factory from_email(cls, raw: str):
           normalized = raw.lower()
           return construct(normalized.split("@")[0], normalized)

Factories are not inherited. A factory constructs the exact class where it is defined.

Members and properties
----------------------

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

Getters define computed readable attributes.

.. code-block:: python

   class Circle:
       radius: float

       getter area(self) -> float:
           return pi * self.radius ** 2

Setters define assignment behavior.

.. code-block:: python

   class Circle:
       radius: float

       setter area(self, value: float):
           self.radius = sqrt(value / pi)

Setter-only attributes are valid.

.. code-block:: python

   class Account:
       password_hash: str

       setter password(self, raw: str):
           self.password_hash = hash_password(raw)

Attribute access is structural and visible in the class body. Lucid does not include descriptors or dynamic attribute hooks.

Value semantics
---------------

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

The fixed object model makes separate shape options unnecessary. There is no ``slots`` option because fixed shape is the default.

Control flow and truth
----------------------

Lucid conditionals require a boolean value or an explicit boolean protocol.

.. code-block:: python

   if ready:
       run()

Values are not automatically truthy just because they are nonzero, non-empty, or non-null.

.. code-block:: python

   if 1:        # error
   if "hello":  # error unless str explicitly implements truth behavior
   if items:    # error unless the type explicitly implements truth behavior

Types that want truth behavior define ``__bool__``.

.. code-block:: python

   interface Truthy:
       declare __bool__(self) -> bool

Length does not imply truth. A sized type can opt in explicitly:

.. code-block:: python

   interface Sized:
       declare __len__(self) -> int

   trait SizedTruthy(Sized, Truthy):
       def __bool__(self) -> bool:
           return self.__len__() != 0

This keeps control flow from depending on accidental conversions.

Indexing
--------

Single indexing passes one index.

.. code-block:: python

   x[0]

means:

.. code-block:: python

   x.__getitem__(0)

Comma-separated indexing passes multiple arguments.

.. code-block:: python

   x[1, 2, 3]

means:

.. code-block:: python

   x.__getitem__(1, 2, 3)

An explicit tuple index remains available by writing the tuple explicitly.

.. code-block:: python

   x[(1, 2, 3)]

means:

.. code-block:: python

   x.__getitem__((1, 2, 3))

Assignment follows the same rule:

.. code-block:: python

   x[1, 2, 3] = v

means:

.. code-block:: python

   x.__setitem__(1, 2, 3, v)

Interfaces
----------

Interfaces declare required APIs. They do not store data and do not provide method bodies.

.. code-block:: python

   interface Sized:
       declare __len__(self) -> int

Interface members use ``declare``, not ``def``, because they specify a callable requirement without implementing it.

.. code-block:: python

   interface Closeable:
       declare close(self) -> None

Classes implement interfaces explicitly.

.. code-block:: python

   class Buffer(Sized):
       data: bytes

       def __len__(self) -> int:
           return len(self.data)

Lucid does not use structural protocols. If a class implements an interface, it says so in the class header.

Traits
------

Traits provide reusable method bodies. They do not declare fields.

.. code-block:: python

   trait SizedTruthy(Sized, Truthy):
       def __bool__(self) -> bool:
           return self.__len__() != 0

A trait can depend on an interface by inheriting it. That makes the trait's requirements visible.

.. code-block:: python

   interface Renderable:
       declare render(self) -> str

   trait DebugRenderable(Renderable):
       def debug(self) -> str:
           return "<debug " + self.render() + ">"

Classes can include traits alongside interfaces.

.. code-block:: python

   class Buffer(Sized, Truthy, SizedTruthy):
       data: bytes

       def __len__(self) -> int:
           return len(self.data)

Trait conflicts are explicit. If two traits define the same method, the class must resolve the collision.

.. code-block:: python

   trait A:
       def render(self) -> str:
           return "A"

   trait B:
       def render(self) -> str:
           return "B"

   class C(A, B):
       def render(self) -> str:
           return A.render(self) + B.render(self)

There is no method-resolution-order magic for combining trait bodies. The class states which trait methods it calls.

Concrete inheritance and super
------------------------------

A class may have at most one concrete parent.

.. code-block:: python

   class Animal:
       def speak(self) -> str:
           return "?"

   class Dog(Animal):
       def speak(self) -> str:
           return super.speak() + " woof"

``super`` refers to the concrete parent. It does not need parentheses, and it does not search a computed multiple-inheritance order.

A class header can combine one concrete parent, any number of interfaces, and any number of traits.

.. code-block:: python

   class FileLogger(LoggerBase, Closeable, Timestamped):
       path: str

The concrete parent supplies inherited state and behavior. Interfaces supply obligations. Traits supply reusable method bodies.

.. list-table::
   :header-rows: 1

   * - Construct
     - Data?
     - Method bodies?
     - Purpose
   * - ``class``
     - yes
     - yes
     - concrete object type
   * - ``interface``
     - no
     - no
     - required API
   * - ``trait``
     - no
     - yes
     - reusable behavior

Operators and dispatch
----------------------

Lucid uses Python's operators and Python's order of operations unless this document says otherwise.

Unpacking binds tighter than binary operators.

.. code-block:: python

   *x + y

means:

.. code-block:: python

   (*x) + y

not:

.. code-block:: python

   *(x + y)

Binary operators are relations between two operand types.

.. code-block:: python

   @dispatch
   def __add__(lhs: X, rhs: Y) -> Z:
       ...

Then:

.. code-block:: python

   x + y

dispatches on both runtime types.

This lets cross-type operations be defined directly:

.. code-block:: python

   @dispatch
   def __add__(lhs: Duration, rhs: Timestamp) -> Timestamp:
       ...

   @dispatch
   def __add__(lhs: Timestamp, rhs: Duration) -> Timestamp:
       ...

The operation is not owned by the left operand. Lucid does not need reflected binary methods such as ``__radd__``, and does not use ``NotImplemented`` as an operator negotiation protocol.

For Python programmers
----------------------

Lucid borrows Python's readable syntax, but changes the parts of Python where behavior can be hidden behind dynamic object-model hooks.

Object shape
~~~~~~~~~~~~

Python instances usually have an open-ended ``__dict__``, unless a class uses ``__slots__``, dataclasses with options, extension types, or custom attribute hooks. Lucid makes fixed shape the default: stored fields are declared in the class body, and undeclared fields are rejected.

Use an explicit dictionary field when dynamic keys are part of the model.

Construction
~~~~~~~~~~~~

Python splits construction across ``__new__``, ``__init__``, dataclass-generated initializers, ``__post_init__``, ``InitVar``, and field options such as ``init=False`` or ``kw_only``.

Lucid has one construction model: factories return fully constructed objects through the factory-only ``construct`` keyword.

A Python backend can lower this however it needs to. For example, generated Python might give the class a private construction class method that allocates the object, assigns fields, and returns it, then compile Lucid factories into calls to that helper.

Class-level behavior
~~~~~~~~~~~~~~~~~~~~

Python represents class methods and static methods through decorators on ordinary functions.

Lucid has syntax for class-level behavior:

* ``classmethod`` is inherited behavior on the class object
* ``factory`` is a non-inherited constructor for the exact defining class
* helper functions that do not use ``self`` or ``cls`` remain ordinary functions

Properties and descriptors
~~~~~~~~~~~~~~~~~~~~~~~~~~

Python descriptors, ``property``, ``__getattribute__``, ``__getattr__``, ``__setattr__``, and ``__delattr__`` can make attribute access programmable from many places.

Lucid replaces that with explicit member kinds: fields, methods, class methods, factories, getters, setters, and class member variables.

Composition
~~~~~~~~~~~

Python uses one inheritance system for concrete inheritance, abstract base classes, mixins, and method-resolution-order composition.

Lucid separates those roles:

* classes own data and concrete behavior
* interfaces declare required APIs
* traits provide reusable method bodies
* trait conflicts are resolved explicitly by the class

Truth
~~~~~

Python truthiness falls back through ``__bool__``, ``__len__``, and built-in emptiness rules. Lucid requires boolean control flow to use ``bool`` or an explicit ``__bool__``.

Collection literals
~~~~~~~~~~~~~~~~~~~

Python uses ``{}`` for an empty dictionary and requires ``set()`` for an empty set. Lucid uses ``{}`` for an empty set and ``{:}`` for an empty dictionary.

Lazy imports
~~~~~~~~~~~~

Python already has lazy-import building blocks, such as import hooks and lazy loaders. Lucid makes laziness explicit in the source with ``lazy import`` and ``lazy from`` forms, so a Python backend can lower them to those mechanisms or to a generated proxy binding.

Indexing
~~~~~~~~

Python parses ``x[1, 2, 3]`` as a single tuple index. Lucid treats comma-separated indexing as multiple index arguments. Use ``x[(1, 2, 3)]`` when the intended index is a tuple.

Operators
~~~~~~~~~

Python binary operators are left-owned first, then may try reflected methods on the right operand. Lucid treats binary operators as multiple-dispatch functions over both operands.

Removed object-model hooks
~~~~~~~~~~~~~~~~~~~~~~~~~~

Lucid removes object-model features that can rewrite class creation, type relationships, object identity, or cleanup from indirect locations.

.. list-table::
   :header-rows: 1

   * - Python feature
     - Lucid approach
   * - metaclasses
     - not part of the language
   * - ``__prepare__``
     - metaclass machinery absent
   * - ``__mro_entries__``
     - no MRO rewriting
   * - ``__instancecheck__``, ``__subclasscheck__``
     - no programmable type checks
   * - assigning ``obj.__class__``
     - object shape is fixed
   * - adding/removing class members after definition
     - class shape is closed
   * - ``__del__``
     - cleanup must be explicit
   * - implicit instance ``__dict__``
     - use explicit dictionary fields

A restricted ``__init_subclass__(cls, **options)`` may remain for validation and registration only, not for class rewriting.

Current slogan
--------------

    Classes store data. Interfaces specify obligations. Traits provide reusable behavior. Factories construct exact classes. Attribute access is structural, not magical. Exports define the public API.
